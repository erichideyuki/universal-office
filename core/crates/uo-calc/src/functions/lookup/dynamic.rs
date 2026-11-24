//! `dynamic.ts`: FILTER, SORT, SORTBY, UNIQUE, SEQUENCE, RANDARRAY, TAKE, DROP, EXPAND, VSTACK, HSTACK, TOCOL,
//! TOROW, WRAPROWS, WRAPCOLS, CHOOSEROWS, CHOOSECOLS, TRIMRANGE, PERCENTOF, GROUPBY, PIVOTBY.
//!
//! Sorts replay V8's `Array.prototype.sort` (`uo_jsnum::sort::js_sort_order`: the TS comparators are not total
//! when a NaN is compared); UNIQUE/GROUPBY group by the TS string keys (`cellKey` joined by U+0001), texts by their
//! collation key.

use std::collections::HashMap;

use uo_jsnum::math::{max as jmax, min as jmin};
use uo_jsnum::sort::js_sort_order;

use crate::coerce::to_bool;
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::key::{MAX_COL, MAX_ROW};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value, array_too_big};

use super::super::shared::{R, Stop, arg, bodies, len, lift_eval, num, tdl, to_num_d};
use super::{area_ref, cmp_total, fail, trunc, view};

pub(super) const DYNAMIC: &[(&str, FnBody)] = bodies![
    "FILTER" => filter,
    "SORT" => sort,
    "SORTBY" => sortby,
    "UNIQUE" => unique,
    "SEQUENCE" => sequence,
    "RANDARRAY" => randarray,
    "TAKE" => take,
    "DROP" => drop_,
    "EXPAND" => expand,
    "VSTACK" => vstack,
    "HSTACK" => hstack,
    "TOCOL" => tocol,
    "TOROW" => torow,
    "WRAPROWS" => wraprows,
    "WRAPCOLS" => wrapcols,
    "CHOOSEROWS" => chooserows,
    "CHOOSECOLS" => choosecols,
    "TRIMRANGE" => trimrange,
    "PERCENTOF" => percentof,
    "GROUPBY" => groupby,
    "PIVOTBY" => pivotby,
];

// ───────────────────────────── support ─────────────────────────────

/// `A(v, ctx)`: the matrix of an argument (a scalar error stays an error).
fn mat(v: &Value, ctx: &mut dyn FnContext) -> R<ArrayValue> {
    if let Value::Scalar(Scalar::Err(e)) = v {
        return fail(*e);
    }
    tdl::as_array(v, ctx)
}

/// The scalar an `intOr`/`boolOr`/`scalarOf` argument stands for: an array → its first element, a reference → the
/// first cell of its first area; `None` = a LAMBDA.
fn first_scalar(v: &Value, ctx: &mut dyn FnContext) -> R<Option<Scalar>> {
    Ok(match v {
        Value::Array(a) => Some(a.get(0, 0)),
        Value::Ref(r) => match r.areas().first() {
            Some(a) => Some(ctx.cell(a.sheet, a.r1, a.c1)?),
            None => Some(Scalar::Empty),
        },
        Value::Lambda(_) => None,
        Value::Scalar(s) => Some(*s),
    })
}

/// `undefined` or `null`.
#[inline]
fn absent(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Scalar(Scalar::Empty)))
}

/// `intOr(v, def, ctx)`: an integer argument (empty/omitted → `def`), truncated.
fn int_or(v: Option<&Value>, def: f64, ctx: &mut dyn FnContext) -> R<f64> {
    let Some(v) = v.filter(|v| !matches!(v, Value::Scalar(Scalar::Empty))) else { return Ok(def) };
    let Some(s) = first_scalar(v, ctx)? else { return fail(ErrorCode::Value) };
    Ok(trunc(to_num_d(ctx, s)?))
}

/// `boolOr(v, def, ctx)`.
fn bool_or(v: Option<&Value>, def: bool, ctx: &mut dyn FnContext) -> R<bool> {
    let Some(v) = v.filter(|v| !matches!(v, Value::Scalar(Scalar::Empty))) else { return Ok(def) };
    let Some(s) = first_scalar(v, ctx)? else { return fail(ErrorCode::Value) };
    Ok(to_bool(view(ctx, s), ctx.locale())?)
}

/// `scalarOf(v, ctx)`: the pad value (`pad_with`, `if_empty`).
fn scalar_of(v: &Value, ctx: &mut dyn FnContext) -> R<Scalar> {
    Ok(first_scalar(v, ctx)?.unwrap_or(Scalar::Err(ErrorCode::Calc)))
}

/// `ArrayValue.fill(rows, cols, f)` with JS sizes: a NaN, negative or fractional size is the `new Array` `RangeError`;
/// past [`crate::value::MAX_ARRAY_CELLS`] the `RangeError` of decision A4a-4.
fn fill_js(rows: f64, cols: f64, f: impl FnMut(u32, u32) -> Scalar) -> R<ArrayValue> {
    let (r, c) = js_shape(rows, cols)?;
    Ok(ArrayValue::fill(r, c, f))
}

/// The shape `ArrayValue.fill` accepts (see [`fill_js`]).
fn js_shape(rows: f64, cols: f64) -> R<(u32, u32)> {
    let n = rows * cols;
    // `!(n >= 0)` is the NaN test of `new Array(n)` too
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    let bad = !(n >= 0.0) || n != trunc(n) || n > 4_294_967_295.0 || rows < 0.0 || cols < 0.0;
    if bad {
        return Err(Stop::Throw(Throw::Range));
    }
    // a 0-cell array keeps its shape (sizes are bounded by every caller: rows or columns past u32 cannot occur)
    let to = |x: f64| if x <= f64::from(u32::MAX) { crate::cast::to_u32(x) } else { u32::MAX };
    let (r, c) = (to(rows), to(cols));
    if n > 0.0 && array_too_big(r, c) {
        return Err(Stop::Throw(Throw::Range));
    }
    Ok((r, c))
}

/// `fromRows(rows, cols)`: ragged rows padded with `#N/A`; no rows or no columns → `#CALC!`.
fn from_rows(rows: &[Vec<Scalar>], cols: u32) -> R<Value> {
    if rows.is_empty() || cols == 0 {
        return fail(ErrorCode::Calc);
    }
    let out = fill_js(rows.len() as f64, f64::from(cols), |r, c| {
        rows[r as usize].get(c as usize).copied().unwrap_or(Scalar::Err(ErrorCode::NA))
    })?;
    Ok(Value::Array(out))
}

fn transpose(a: &ArrayValue) -> ArrayValue {
    ArrayValue::fill(a.cols, a.rows, |r, c| a.get(c, r))
}

/// `sortDir(a, b, c, order)`: empty stays last in a descending sort too (measured).
#[inline]
fn sort_dir(a: Scalar, b: Scalar, c: i32, order: f64) -> i32 {
    if order > 0.0 || matches!(a, Scalar::Empty) || matches!(b, Scalar::Empty) { c } else { -c }
}

/// `sameCell(a, b)`: UNIQUE's equality (texts by the collation, numbers exact).
fn same_cell(ctx: &dyn FnContext, a: Scalar, b: Scalar) -> bool {
    match (a, b) {
        (Scalar::Empty, Scalar::Empty) => true,
        (Scalar::Num(x), Scalar::Num(y)) => x == y,
        (Scalar::Str(x), Scalar::Str(y)) => crate::collate::text_equals(ctx.text(x), ctx.text(y)),
        (Scalar::Bool(x), Scalar::Bool(y)) => x == y,
        (Scalar::Err(x), Scalar::Err(y)) => x == y,
        _ => false,
    }
}

/// `cellKey(v)` appended to `out`: `z`, `n<String(x)>`, `s<collation key>`, `bT`/`bF`, `e<code>`.
fn push_cell_key(ctx: &dyn FnContext, v: Scalar, out: &mut String) {
    match v {
        Scalar::Empty => out.push('z'),
        Scalar::Num(x) => {
            out.push('n');
            out.push_str(&uo_jsnum::format::to_js_string(x));
        }
        Scalar::Str(r) => {
            out.push('s');
            out.push_str(&crate::collate::collation_key(ctx.text(r)));
        }
        Scalar::Bool(b) => out.push_str(if b { "bT" } else { "bF" }),
        Scalar::Err(e) => {
            out.push('e');
            out.push_str(crate::cell::error_text(e));
        }
    }
}

/// An index of `pickIndexes`/`sort_index` (`None` = NaN: `a.get(NaN, c)` is `null`).
type Ix = Option<u32>;

#[inline]
fn get_ix(a: &ArrayValue, r: Ix, c: Ix) -> Scalar {
    match (r, c) {
        (Some(r), Some(c)) => a.get(r, c),
        _ => Scalar::Empty,
    }
}

/// `pickIndexes(args, n, ctx)`: CHOOSEROWS/CHOOSECOLS — negatives count from the end; 0 or out of range → `#VALUE!`.
fn pick_indexes(a: &CallArgs<'_>, n: u32, ctx: &mut dyn FnContext) -> R<Vec<Ix>> {
    let mut out = Vec::new();
    for i in 1..len(a) {
        let m = mat(arg(a, i).unwrap_or(&super::super::shared::EMPTY), ctx)?;
        for &x in m.data.iter() {
            if let Scalar::Err(e) = x {
                return fail(e);
            }
            let k = to_num_d(ctx, x)?;
            let i = trunc(k);
            if i == 0.0 || i.abs() > f64::from(n) {
                return fail(ErrorCode::Value);
            }
            out.push(if i.is_nan() {
                None
            } else if i > 0.0 {
                Some(crate::cast::to_u32(i) - 1)
            } else {
                Some(crate::cast::to_u32(f64::from(n) + i))
            });
        }
    }
    Ok(out)
}

/// `sumOf(v, ctx)` (PERCENTOF): the numbers of a value (texts and booleans of a reference ignored; an error → `#NUM!`).
fn sum_of(v: &Value, ctx: &mut dyn FnContext) -> R<f64> {
    let data: Vec<Scalar> = match v {
        Value::Scalar(Scalar::Err(e)) => return fail(*e),
        Value::Lambda(_) => return fail(ErrorCode::Calc),
        Value::Scalar(s) => return Ok(to_num_d(ctx, *s)?),
        Value::Ref(r) => {
            let mut d = Vec::new();
            for k in 0..r.areas().len() {
                d.extend_from_slice(&ctx.values(r, k)?.data);
            }
            d
        }
        Value::Array(a) => a.data.to_vec(),
    };
    let mut s = 0.0;
    for x in data {
        match x {
            // measured: PERCENTOF(C1:C7 with #N/A; …) = #NUM!
            Scalar::Err(_) => return fail(ErrorCode::Num),
            Scalar::Num(n) => s += n,
            _ => {}
        }
    }
    Ok(s)
}

// ───────────────────────────── FILTER / SORT / SORTBY / UNIQUE ─────────────────────────────

fn filter(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let inc = mat(arg(a, 1).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let by_row = inc.cols == 1 && inc.rows == m.rows;
    let by_col = !by_row && inc.rows == 1 && inc.cols == m.cols;
    if !by_row && !by_col {
        return fail(ErrorCode::Value);
    }
    let mut keep: Vec<u32> = Vec::new();
    for (i, &x) in inc.data.iter().enumerate() {
        match x {
            Scalar::Err(e) => return fail(e),
            Scalar::Str(_) => return fail(ErrorCode::Value),
            Scalar::Bool(true) => keep.push(i as u32),
            Scalar::Num(n) if n != 0.0 => keep.push(i as u32),
            _ => {}
        }
    }
    if keep.is_empty() {
        return match arg(a, 2) {
            None | Some(Value::Scalar(Scalar::Empty)) => fail(ErrorCode::Calc),
            Some(ev @ Value::Ref(_)) => Ok(Value::Array(mat(ev, ctx)?)),
            Some(ev) => Ok(ev.clone()),
        };
    }
    let k = keep.len() as u32;
    Ok(Value::Array(if by_row {
        ArrayValue::fill(k, m.cols, |r, c| m.get(keep[r as usize], c))
    } else {
        ArrayValue::fill(m.rows, k, |r, c| m.get(r, keep[c as usize]))
    }))
}

/// The SORT `sort_index`/`sort_order` matrices (`[1]` when absent).
fn list_mat(v: Option<&Value>, ctx: &mut dyn FnContext) -> R<ArrayValue> {
    if absent(v) {
        return Ok(super::super::shared::one(Scalar::Num(1.0)));
    }
    mat(v.unwrap_or(&super::super::shared::EMPTY), ctx)
}

/// An element of a SORT list: an error stays, else the truncated number.
fn list_int(ctx: &dyn FnContext, x: Scalar) -> R<f64> {
    if let Scalar::Err(e) = x {
        return fail(e);
    }
    Ok(trunc(to_num_d(ctx, x)?))
}

fn sort(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let a0 = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let by_col = bool_or(arg(a, 3), false, ctx)?;
    let m = if by_col { transpose(&a0) } else { a0 };
    let idx_m = list_mat(arg(a, 1), ctx)?;
    let ord_m = list_mat(arg(a, 2), ctx)?;
    // each element is converted and checked in turn (the first bad one wins), the indexes before the orders
    let mut idx: Vec<Ix> = Vec::with_capacity(idx_m.data.len());
    for &x in idx_m.data.iter() {
        let k = list_int(ctx, x)?;
        if k < 1.0 || k > f64::from(m.cols) {
            return fail(ErrorCode::Value);
        }
        idx.push(if k.is_nan() { None } else { Some(crate::cast::to_u32(k) - 1) });
    }
    let mut ord_raw: Vec<f64> = Vec::with_capacity(ord_m.data.len());
    for &x in ord_m.data.iter() {
        let k = list_int(ctx, x)?;
        if k != 1.0 && k != -1.0 {
            return fail(ErrorCode::Value);
        }
        ord_raw.push(k);
    }
    if ord_raw.len() != 1 && ord_raw.len() != idx.len() {
        return fail(ErrorCode::Value);
    }
    let order = {
        let c: &dyn FnContext = ctx;
        js_sort_order(m.rows as usize, &mut |x, y| {
            let (x32, y32) = (x as u32, y as u32);
            for (k, &ix) in idx.iter().enumerate() {
                let (p, q) = (get_ix(&m, Some(x32), ix), get_ix(&m, Some(y32), ix));
                if ix.is_none() {
                    continue;
                }
                let cmp = cmp_total(c, p, q);
                if cmp != 0 {
                    let o = if ord_raw.len() == 1 { ord_raw[0] } else { ord_raw[k] };
                    return f64::from(sort_dir(p, q, cmp, o));
                }
            }
            x as f64 - y as f64
        })
    };
    let out = ArrayValue::fill(m.rows, m.cols, |r, c| m.get(order[r as usize] as u32, c));
    Ok(Value::Array(if by_col { transpose(&out) } else { out }))
}

fn sortby(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let mut keys: Vec<(ArrayValue, f64)> = Vec::new();
    let mut by_col: Option<bool> = None;
    let mut i = 1;
    while i < len(a) {
        let k = mat(arg(a, i).unwrap_or(&super::super::shared::EMPTY), ctx)?;
        let o = int_or(arg(a, i + 1), 1.0, ctx)?;
        if o != 1.0 && o != -1.0 {
            return fail(ErrorCode::Value);
        }
        let row_wise = k.cols == 1 && k.rows == m.rows;
        let col_wise = !row_wise && k.rows == 1 && k.cols == m.cols;
        if !row_wise && !col_wise {
            return fail(ErrorCode::Value);
        }
        if by_col.is_some_and(|b| b != col_wise) {
            return fail(ErrorCode::Value);
        }
        by_col = Some(col_wise);
        keys.push((k, o));
        i += 2;
    }
    let by_col = by_col == Some(true);
    let n = if by_col { m.cols } else { m.rows };
    let order = {
        let c: &dyn FnContext = ctx;
        js_sort_order(n as usize, &mut |x, y| {
            for (k, o) in &keys {
                let (p, q) = (k.data.get(x).copied().unwrap_or(Scalar::Empty), k.data.get(y).copied().unwrap_or(Scalar::Empty));
                let cmp = cmp_total(c, p, q);
                if cmp != 0 {
                    return f64::from(sort_dir(p, q, cmp, *o));
                }
            }
            x as f64 - y as f64
        })
    };
    Ok(Value::Array(if by_col {
        ArrayValue::fill(m.rows, m.cols, |r, c| m.get(r, order[c as usize] as u32))
    } else {
        ArrayValue::fill(m.rows, m.cols, |r, c| m.get(order[r as usize] as u32, c))
    }))
}

fn unique(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let a0 = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let by_col = bool_or(arg(a, 1), false, ctx)?;
    let once = bool_or(arg(a, 2), false, ctx)?;
    let m = if by_col { transpose(&a0) } else { a0 };
    // grouping by the equality key in a hash (whole columns: a million rows)
    let mut groups: Vec<(u32, u32)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut key = String::new();
    for r in 0..m.rows {
        key.clear();
        for c in 0..m.cols {
            push_cell_key(ctx, m.get(r, c), &mut key);
            key.push('\u{1}');
        }
        if let Some(&g) = seen.get(key.as_str()) {
            groups[g].1 += 1;
        } else {
            seen.insert(key.clone(), groups.len());
            groups.push((r, 1));
        }
    }
    let keep: Vec<u32> = groups.iter().filter(|g| !once || g.1 == 1).map(|g| g.0).collect();
    if keep.is_empty() {
        return fail(ErrorCode::Calc);
    }
    if m.cols == 0 {
        return fail(ErrorCode::Calc);
    }
    let out = ArrayValue::fill(keep.len() as u32, m.cols, |r, c| m.get(keep[r as usize], c));
    Ok(Value::Array(if by_col { transpose(&out) } else { out }))
}

// ───────────────────────────── SEQUENCE / RANDARRAY ─────────────────────────────

/// A number argument of SEQUENCE/RANDARRAY (`value` mode): absent or empty → `def`.
fn num_or(a: &CallArgs<'_>, i: usize, def: f64, ctx: &dyn FnContext) -> R<f64> {
    match super::super::shared::scalar(a, i) {
        None | Some(Scalar::Empty) => Ok(def),
        Some(v) => Ok(to_num_d(ctx, v)?),
    }
}

fn sequence(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let r = int_or(arg(a, 0), 1.0, ctx)?;
    let c = int_or(arg(a, 1), 1.0, ctx)?;
    let s0 = num_or(a, 2, 1.0, ctx)?;
    let st = num_or(a, 3, 1.0, ctx)?;
    if r < 0.0 || c < 0.0 {
        return fail(ErrorCode::Value);
    }
    if r == 0.0 || c == 0.0 {
        return fail(ErrorCode::Calc);
    }
    // measured: 1048577 rows → #VALUE!; more columns than the sheet → #SPILL!
    if r > f64::from(MAX_ROW) + 1.0 {
        return fail(ErrorCode::Value);
    }
    if c > f64::from(MAX_COL) + 1.0 {
        return fail(ErrorCode::Spill);
    }
    let cf = c;
    Ok(Value::Array(fill_js(r, c, |i, j| Scalar::Num(s0 + (f64::from(i) * cf + f64::from(j)) * st))?))
}

fn randarray(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let r = int_or(arg(a, 0), 1.0, ctx)?;
    let c = int_or(arg(a, 1), 1.0, ctx)?;
    let lo = num_or(a, 2, 0.0, ctx)?;
    let hi = num_or(a, 3, 1.0, ctx)?;
    let int = bool_or(arg(a, 4), false, ctx)?;
    if r < 0.0 || c < 0.0 {
        return fail(ErrorCode::Value);
    }
    if r == 0.0 || c == 0.0 {
        return fail(ErrorCode::Calc);
    }
    if lo > hi {
        return fail(ErrorCode::Value);
    }
    if int && (lo != trunc(lo) || hi != trunc(hi)) {
        return fail(ErrorCode::Value);
    }
    // the size is checked before any random number is drawn (`new Array(rows * cols)`)
    let (rows, cols) = js_shape(r, c)?;
    let out = ArrayValue::fill(rows, cols, |_, _| {
        let x = ctx.random();
        Scalar::Num(if int { lo + (x * (hi - lo + 1.0)).floor() } else { lo + x * (hi - lo) })
    });
    Ok(Value::Array(out))
}

// ───────────────────────────── TAKE / DROP / EXPAND ─────────────────────────────

fn take_drop(a: &CallArgs<'_>, ctx: &mut dyn FnContext, take: bool) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let r = if absent(arg(a, 1)) { None } else { Some(int_or(arg(a, 1), 0.0, ctx)?) };
    let c = if absent(arg(a, 2)) { None } else { Some(int_or(arg(a, 2), 0.0, ctx)?) };
    let span = |n: f64, k: Option<f64>| -> (f64, f64) {
        let Some(k) = k else { return (0.0, n) };
        if take {
            if k >= 0.0 { (0.0, jmin(k, n)) } else { (jmax(0.0, n + k), n) }
        } else if k >= 0.0 {
            (jmin(k, n), n)
        } else {
            (0.0, jmax(0.0, n + k))
        }
    };
    if take && (r == Some(0.0) || c == Some(0.0)) {
        return fail(ErrorCode::Calc);
    }
    let (r1, r2) = span(f64::from(m.rows), r);
    let (c1, c2) = span(f64::from(m.cols), c);
    if r2 <= r1 || c2 <= c1 {
        return fail(ErrorCode::Calc);
    }
    // a NaN bound reaches `new Array(NaN)`: the RangeError
    let (rr1, cc1) = (
        if r1.is_nan() { 0 } else { crate::cast::to_u32(jmax(r1, 0.0)) },
        if c1.is_nan() { 0 } else { crate::cast::to_u32(jmax(c1, 0.0)) },
    );
    Ok(Value::Array(fill_js(r2 - r1, c2 - c1, |i, j| m.get(rr1 + i, cc1 + j))?))
}

fn take(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    take_drop(a, ctx, true)
}

fn drop_(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    take_drop(a, ctx, false)
}

fn expand(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let r = int_or(arg(a, 1), f64::from(m.rows), ctx)?;
    let c = int_or(arg(a, 2), f64::from(m.cols), ctx)?;
    if r < f64::from(m.rows) || c < f64::from(m.cols) {
        return fail(ErrorCode::Value);
    }
    if r > f64::from(MAX_ROW) + 1.0 || c > f64::from(MAX_COL) + 1.0 {
        return fail(ErrorCode::Num);
    }
    let pv = arg(a, 3);
    if let Some(Value::Array(p)) = pv
        && (p.rows > 1 || p.cols > 1)
    {
        // measured
        return fail(ErrorCode::Value);
    }
    let pad = match pv {
        None => Scalar::Err(ErrorCode::NA),
        Some(p) => scalar_of(p, ctx)?,
    };
    Ok(Value::Array(fill_js(r, c, |i, j| if i < m.rows && j < m.cols { m.get(i, j) } else { pad })?))
}

// ───────────────────────────── VSTACK / HSTACK / TOCOL / TOROW ─────────────────────────────

fn stack(a: &CallArgs<'_>, ctx: &mut dyn FnContext, vertical: bool) -> R<Value> {
    let mut parts: Vec<ArrayValue> = Vec::with_capacity(len(a));
    for i in 0..len(a) {
        let v = arg(a, i).unwrap_or(&super::super::shared::EMPTY);
        if matches!(v, Value::Scalar(Scalar::Empty)) {
            return fail(ErrorCode::Value);
        }
        parts.push(mat(v, ctx)?);
    }
    if vertical {
        let w = parts.iter().map(|p| p.cols).max().unwrap_or(0);
        let total: u64 = parts.iter().map(|p| u64::from(p.rows)).sum();
        if total == 0 || w == 0 {
            return fail(ErrorCode::Calc);
        }
        if total > u64::from(u32::MAX) || array_too_big(total as u32, w) {
            return Err(Stop::Throw(Throw::Range));
        }
        let mut data = Vec::with_capacity(total as usize * w as usize);
        for p in &parts {
            for r in 0..p.rows {
                for c in 0..w {
                    data.push(if c < p.cols { p.get(r, c) } else { Scalar::Err(ErrorCode::NA) });
                }
            }
        }
        return Ok(Value::Array(ArrayValue::new(total as u32, w, data)));
    }
    let h = parts.iter().map(|p| p.rows).max().unwrap_or(0);
    let w: u64 = parts.iter().map(|p| u64::from(p.cols)).sum();
    if w > u64::from(u32::MAX) || array_too_big(h, w as u32) {
        return Err(Stop::Throw(Throw::Range));
    }
    let mut data = Vec::with_capacity(h as usize * w as usize);
    for r in 0..h {
        for p in &parts {
            for c in 0..p.cols {
                data.push(if r < p.rows { p.get(r, c) } else { Scalar::Err(ErrorCode::NA) });
            }
        }
    }
    Ok(Value::Array(ArrayValue::new(h, w as u32, data)))
}

fn vstack(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    stack(a, ctx, true)
}

fn hstack(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    stack(a, ctx, false)
}

fn to_line(a: &CallArgs<'_>, ctx: &mut dyn FnContext, column: bool) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let ig = int_or(arg(a, 1), 0.0, ctx)?;
    let by_col = bool_or(arg(a, 2), false, ctx)?;
    if ig < 0.0 || ig > 3.0 {
        return fail(ErrorCode::Value);
    }
    let skip_blank = ig == 1.0 || ig == 3.0;
    let skip_err = ig == 2.0 || ig == 3.0;
    let mut xs: Vec<Scalar> = Vec::new();
    let mut push = |x: Scalar| {
        if skip_blank && matches!(x, Scalar::Empty) {
            return;
        }
        if skip_err && matches!(x, Scalar::Err(_)) {
            return;
        }
        xs.push(x);
    };
    if by_col {
        for c in 0..m.cols {
            for r in 0..m.rows {
                push(m.get(r, c));
            }
        }
    } else {
        for &x in m.data.iter() {
            push(x);
        }
    }
    if xs.is_empty() {
        return fail(ErrorCode::Calc);
    }
    let n = xs.len() as u32;
    Ok(Value::Array(if column { ArrayValue::new(n, 1, xs) } else { ArrayValue::new(1, n, xs) }))
}

fn tocol(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    to_line(a, ctx, true)
}

fn torow(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    to_line(a, ctx, false)
}

// ───────────────────────────── WRAP* / CHOOSE* ─────────────────────────────

fn wrap(a: &CallArgs<'_>, ctx: &mut dyn FnContext, rows: bool) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    if m.rows > 1 && m.cols > 1 {
        return fail(ErrorCode::Value);
    }
    let k = int_or(arg(a, 1), 0.0, ctx)?;
    if k < 1.0 {
        return fail(ErrorCode::Num);
    }
    let xs = &m.data;
    let pv = arg(a, 2);
    if let Some(Value::Array(p)) = pv
        && (p.rows > 1 || p.cols > 1)
    {
        // pad_with an array: Excel lifts the function over it (measured: every result shows the first element)
        let x0 = xs.first().copied().unwrap_or(Scalar::Empty);
        return Ok(Value::Array(ArrayValue::fill(p.rows, p.cols, |_, _| x0)));
    }
    let pad = match pv {
        None => Scalar::Err(ErrorCode::NA),
        Some(p) => scalar_of(p, ctx)?,
    };
    let n = xs.len() as f64;
    // a vector shorter than the wrap: no padding (measured)
    let w = jmin(k, n);
    let lines = (n / w).ceil();
    let wu = if w.is_nan() || w <= 0.0 { 0 } else { crate::cast::to_u32(jmin(w, f64::from(u32::MAX))) };
    let at = |i: u64| if i < xs.len() as u64 { xs[i as usize] } else { pad };
    if rows {
        Ok(Value::Array(fill_js(lines, w, |r, c| at(u64::from(r) * u64::from(wu) + u64::from(c)))?))
    } else {
        Ok(Value::Array(fill_js(w, lines, |r, c| at(u64::from(c) * u64::from(wu) + u64::from(r)))?))
    }
}

fn wraprows(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    wrap(a, ctx, true)
}

fn wrapcols(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    wrap(a, ctx, false)
}

fn choose_lines(a: &CallArgs<'_>, ctx: &mut dyn FnContext, rows: bool) -> R<Value> {
    let m = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let ix = pick_indexes(a, if rows { m.rows } else { m.cols }, ctx)?;
    let k = f64::from(u32::try_from(ix.len()).unwrap_or(u32::MAX));
    Ok(Value::Array(if rows {
        fill_js(k, f64::from(m.cols), |r, c| get_ix(&m, ix[r as usize], Some(c)))?
    } else {
        fill_js(f64::from(m.rows), k, |r, c| get_ix(&m, Some(r), ix[c as usize]))?
    }))
}

fn chooserows(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    choose_lines(a, ctx, true)
}

fn choosecols(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    choose_lines(a, ctx, false)
}

// ───────────────────────────── TRIMRANGE / PERCENTOF ─────────────────────────────

/// `x & bit` of a JS number (`ToInt32`: NaN → 0).
#[inline]
fn bit(x: f64, b: i32) -> bool {
    uo_jsnum::conv::to_int32(x) & b != 0
}

fn trimrange(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let r = int_or(arg(a, 1), 3.0, ctx)?;
    let c = int_or(arg(a, 2), 3.0, ctx)?;
    if r < 0.0 || r > 3.0 || c < 0.0 || c > 3.0 {
        return fail(ErrorCode::Value);
    }
    let rv = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Ref(x) = rv {
        let Some(ar) = x.single() else { return fail(ErrorCode::Value) };
        let (mut min_r, mut max_r, mut min_c, mut max_c) = (u32::MAX, None::<u32>, u32::MAX, 0u32);
        ctx.for_each_cell(ar, &mut |v, row, col| {
            if matches!(v, Scalar::Empty) {
                return;
            }
            min_r = min_r.min(row);
            max_r = Some(max_r.map_or(row, |m| m.max(row)));
            min_c = min_c.min(col);
            max_c = max_c.max(col);
        })?;
        // all empty (measured)
        let Some(max_r) = max_r else { return fail(ErrorCode::Ref) };
        return Ok(area_ref(crate::key::Area {
            sheet: ar.sheet,
            r1: if bit(r, 1) { min_r } else { ar.r1 },
            r2: if bit(r, 2) { max_r } else { ar.r2 },
            c1: if bit(c, 1) { min_c } else { ar.c1 },
            c2: if bit(c, 2) { max_c } else { ar.c2 },
        }));
    }
    let m = mat(rv, ctx)?;
    let blank_row = |i: i64| (0..m.cols).all(|j| matches!(m.get(i as u32, j), Scalar::Empty));
    let blank_col = |j: i64| (0..m.rows).all(|i| matches!(m.get(i, j as u32), Scalar::Empty));
    let (mut r1, mut r2, mut c1, mut c2) = (0i64, i64::from(m.rows) - 1, 0i64, i64::from(m.cols) - 1);
    if bit(r, 1) {
        while r1 < r2 && blank_row(r1) {
            r1 += 1;
        }
    }
    if bit(r, 2) {
        while r2 > r1 && blank_row(r2) {
            r2 -= 1;
        }
    }
    if bit(c, 1) {
        while c1 < c2 && blank_col(c1) {
            c1 += 1;
        }
    }
    if bit(c, 2) {
        while c2 > c1 && blank_col(c2) {
            c2 -= 1;
        }
    }
    let (or1, oc1) = (r1.max(0) as u32, c1.max(0) as u32);
    Ok(Value::Array(fill_js((r2 - r1 + 1) as f64, (c2 - c1 + 1) as f64, |i, j| m.get(or1 + i, oc1 + j))?))
}

fn percentof(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let s = sum_of(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let t = sum_of(arg(a, 1).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    if t == 0.0 { fail(ErrorCode::Div0) } else { Ok(num(s / t)) }
}

// ───────────────────────────── GROUPBY / PIVOTBY ─────────────────────────────

/// A group: its key (one scalar per key column) and its rows, in order of first appearance.
struct Group {
    key: Vec<Scalar>,
    rows: Vec<u32>,
}

/// `aggregate(fn, vals, rows, col, all, ctx)`: the aggregation called with the group's column (and the total, for a
/// function of two parameters: PERCENTOF, `LAMBDA(x;y;…)`).
fn aggregate(f: u32, vals: &ArrayValue, rows: &[u32], col: u32, all: &[u32], ctx: &mut dyn FnContext) -> R<Scalar> {
    let sub = ArrayValue::fill(rows.len() as u32, 1, |r, _| vals.get(rows[r as usize], col));
    let mut args = vec![Value::Array(sub)];
    if ctx.lambda_arity(f).0 >= 2 {
        args.push(Value::Array(ArrayValue::fill(all.len() as u32, 1, |r, _| vals.get(all[r as usize], col))));
    }
    let out = lift_eval(ctx.call_lambda(f, &args))?;
    Ok(match out {
        Value::Array(x) => {
            if x.rows == 1 && x.cols == 1 {
                x.get(0, 0)
            } else {
                Scalar::Err(ErrorCode::Calc)
            }
        }
        Value::Ref(r) => match r.single() {
            Some(x) if x.r1 == x.r2 && x.c1 == x.c2 => ctx.cell(x.sheet, x.r1, x.c1)?,
            _ => Scalar::Err(ErrorCode::Calc),
        },
        Value::Lambda(_) => Scalar::Err(ErrorCode::Calc),
        Value::Scalar(s) => s,
    })
}

/// `groupRows(keys, rows)`: rows grouped by key, in order of first appearance.
fn group_rows(ctx: &dyn FnContext, keys: &ArrayValue, rows: &[u32]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut k = String::new();
    for &r in rows {
        k.clear();
        for c in 0..keys.cols {
            if c > 0 {
                k.push('\u{1}');
            }
            push_cell_key(ctx, keys.get(r, c), &mut k);
        }
        if let Some(&g) = seen.get(k.as_str()) {
            out[g].rows.push(r);
        } else {
            seen.insert(k.clone(), out.len());
            out.push(Group { key: (0..keys.cols).map(|c| keys.get(r, c)).collect(), rows: vec![r] });
        }
    }
    out
}

/// `headerMode(v, vals, ctx)`: 0 no, 1 yes (hidden), 2 no (generate), 3 yes (show); absent = detect (a first row of
/// texts over numbers: used as headers but not shown, measured).
fn header_mode(v: Option<&Value>, vals: &ArrayValue, ctx: &mut dyn FnContext) -> R<f64> {
    if absent(v) {
        let first = vals.rows > 1 && vals.data.iter().take(vals.cols as usize).all(|x| matches!(x, Scalar::Str(_)));
        let rest = vals.rows > 1 && matches!(vals.get(1, 0), Scalar::Num(_));
        return Ok(if first && rest { 1.0 } else { 0.0 });
    }
    let n = int_or(v, 0.0, ctx)?;
    if (0.0..=3.0).contains(&n) { Ok(n) } else { fail(ErrorCode::Value) }
}

/// `genHeader(kind, i, ctx)`: the generated header names (`field_headers` = 2) in the language.
fn gen_header(ctx: &mut dyn FnContext, kind: u8, i: u32) -> R<Scalar> {
    let pt = ctx.locale().id.starts_with("pt");
    let base = match (kind, pt) {
        (b'r', true) => "Campo de linha ",
        (b'r', false) => "Row Field ",
        (b'c', true) => "Campo de coluna ",
        (b'c', false) => "Col Field ",
        (_, true) => "Valor ",
        (_, false) => "Value ",
    };
    let s = format!("{base}{i}");
    Ok(Scalar::Str(ctx.new_text(JsStr::Latin1(s.as_bytes()))?))
}

/// `filterRows(rows, flt, n, ctx)`: the rows that pass `filter_array` (booleans of the data's size).
fn filter_rows(rows: Vec<u32>, flt: Option<&Value>, n: u32, ctx: &mut dyn FnContext) -> R<Vec<u32>> {
    if absent(flt) {
        return Ok(rows);
    }
    let f = mat(flt.unwrap_or(&super::super::shared::EMPTY), ctx)?;
    if f.data.len() != n as usize {
        return fail(ErrorCode::Value);
    }
    let mut keep = Vec::new();
    for r in rows {
        let x = f.data[r as usize];
        if let Scalar::Err(e) = x {
            return fail(e);
        }
        if to_bool(view(ctx, x), ctx.locale())? {
            keep.push(r);
        }
    }
    Ok(keep)
}

/// `sortSpec(v, nkeys, width, ctx)`: the sort columns (1-based, negative = descending); default = every key.
fn sort_spec(v: Option<&Value>, nkeys: u32, width: u32, ctx: &mut dyn FnContext) -> R<Vec<f64>> {
    if absent(v) {
        return Ok((1..=nkeys).map(f64::from).collect());
    }
    let so = mat(v.unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let mut out = Vec::new();
    for &x in so.data.iter() {
        if let Scalar::Err(e) = x {
            return fail(e);
        }
        let k = trunc(to_num_d(ctx, x)?);
        if k == 0.0 || k.abs() > f64::from(width) {
            return fail(ErrorCode::Value);
        }
        out.push(k);
    }
    Ok(out)
}

/// The sort position of a spec entry (`Math.abs(o) - 1`; `None` = NaN: `x[NaN]` is `undefined` on both sides).
#[inline]
fn spec_ix(o: f64) -> Option<usize> {
    if o.is_nan() { None } else { Some(crate::cast::trunc_usize(o.abs()) - 1) }
}

/// `Array.prototype.sort` of the items by a JS comparator, in place.
fn js_sort<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> i32) {
    let order = js_sort_order(v.len(), &mut |i, j| f64::from(cmp(&v[i], &v[j])));
    uo_jsnum::sort::permute(v, order);
}

/// A row of `n` empty texts (`blanks(n)`).
fn blanks(out: &mut Vec<Scalar>, n: i64, empty: Scalar) {
    for _ in 0..n.max(0) {
        out.push(empty);
    }
}

fn groupby(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let keys = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let vals = mat(arg(a, 1).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let f = match arg(a, 2) {
        Some(Value::Lambda(f)) => *f,
        Some(Value::Scalar(Scalar::Err(e))) => return fail(*e),
        _ => return fail(ErrorCode::Value),
    };
    if keys.rows != vals.rows {
        return fail(ErrorCode::Value);
    }
    let hm = header_mode(arg(a, 3), &vals, ctx)?;
    let depth = int_or(arg(a, 4), 1.0, ctx)?;
    // measured: 2 levels with 1 key → #VALUE!
    if depth.abs() > f64::from(keys.cols) {
        return fail(ErrorCode::Value);
    }
    let rel = int_or(arg(a, 7), 0.0, ctx)?;
    if rel != 0.0 && rel != 1.0 {
        return fail(ErrorCode::Value);
    }
    let has_hdr = hm == 1.0 || hm == 3.0;
    let all: Vec<u32> = (u32::from(has_hdr).min(keys.rows)..keys.rows).collect();
    let rows = filter_rows(all, arg(a, 6), keys.rows, ctx)?;
    if rows.is_empty() {
        return fail(ErrorCode::Calc);
    }
    let width = keys.cols + vals.cols;
    let order = sort_spec(arg(a, 5), keys.cols, width, ctx)?;
    let groups = group_rows(ctx, &keys, &rows);
    struct Line {
        key: Vec<Scalar>,
        rows: Vec<u32>,
        vals: Vec<Scalar>,
    }
    let mut lines: Vec<Line> = Vec::with_capacity(groups.len());
    for g in groups {
        let mut v = Vec::with_capacity(vals.cols as usize);
        for c in 0..vals.cols {
            v.push(aggregate(f, &vals, &g.rows, c, &rows, ctx)?);
        }
        lines.push(Line { key: g.key, rows: g.rows, vals: v });
    }
    {
        let c: &dyn FnContext = ctx;
        let nk = keys.cols as usize;
        let at = |l: &Line, i: usize| if i < nk { l.key.get(i).copied() } else { l.vals.get(i - nk).copied() };
        js_sort(&mut lines, |x, y| {
            for &o in &order {
                let Some(i) = spec_ix(o) else { continue };
                let (Some(p), Some(q)) = (at(x, i), at(y, i)) else { continue };
                let cmp = cmp_total(c, p, q);
                if cmp != 0 {
                    return sort_dir(p, q, cmp, o);
                }
            }
            0
        });
    }
    let empty = Scalar::Str(ctx.new_text(JsStr::EMPTY)?);
    let total = Scalar::Str(ctx.new_text(JsStr::Latin1(b"Total"))?);
    let mut out: Vec<Vec<Scalar>> = Vec::new();
    if hm == 2.0 || hm == 3.0 {
        let mut hdr = Vec::with_capacity(width as usize);
        for c in 0..keys.cols {
            hdr.push(if hm == 3.0 { keys.get(0, c) } else { gen_header(ctx, b'r', c + 1)? });
        }
        for c in 0..vals.cols {
            hdr.push(if hm == 3.0 { vals.get(0, c) } else { gen_header(ctx, b'v', c + 1)? });
        }
        out.push(hdr);
    }
    let fill_n = i64::from(keys.cols) - 1;
    let total_line = |rr: &[u32], label: Scalar, ctx: &mut dyn FnContext| -> R<Vec<Scalar>> {
        let mut l = vec![label];
        blanks(&mut l, fill_n, empty);
        for c in 0..vals.cols {
            l.push(aggregate(f, &vals, rr, c, &rows, ctx)?);
        }
        Ok(l)
    };
    if depth < 0.0 {
        out.push(total_line(&rows, total, ctx)?);
    }
    // subtotals (|depth| ≥ 2): per group of the first level, after (or before) its lines
    let sub = depth.abs() >= 2.0 && keys.cols >= 2;
    let line_of = |l: &Line| -> Vec<Scalar> { l.key.iter().chain(&l.vals).copied().collect() };
    let mut i = 0;
    while i < lines.len() {
        if !sub {
            out.push(line_of(&lines[i]));
            i += 1;
            continue;
        }
        let k0 = lines[i].key.first().copied().unwrap_or(Scalar::Empty);
        let mut j = i;
        let mut rr: Vec<u32> = Vec::new();
        while j < lines.len() && same_cell(ctx, lines[j].key.first().copied().unwrap_or(Scalar::Empty), k0) {
            rr.extend_from_slice(&lines[j].rows);
            j += 1;
        }
        let st = total_line(&rr, k0, ctx)?;
        if depth < 0.0 {
            out.push(st.clone());
        }
        for l in &lines[i..j] {
            out.push(line_of(l));
        }
        if depth > 0.0 {
            out.push(st);
        }
        i = j;
    }
    if depth > 0.0 {
        out.push(total_line(&rows, total, ctx)?);
    }
    from_rows(&out, width)
}

fn pivotby(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let rk = mat(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let ck = mat(arg(a, 1).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let vals = mat(arg(a, 2).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let f = match arg(a, 3) {
        Some(Value::Lambda(f)) => *f,
        Some(Value::Scalar(Scalar::Err(e))) => return fail(*e),
        _ => return fail(ErrorCode::Value),
    };
    if rk.rows != vals.rows || ck.rows != vals.rows {
        return fail(ErrorCode::Value);
    }
    let hm = header_mode(arg(a, 4), &vals, ctx)?;
    let rdepth = int_or(arg(a, 5), 1.0, ctx)?;
    let cdepth = int_or(arg(a, 7), 1.0, ctx)?;
    if rdepth.abs() > f64::from(rk.cols) || cdepth.abs() > f64::from(ck.cols) {
        return fail(ErrorCode::Value);
    }
    let rso = sort_spec(arg(a, 6), rk.cols, rk.cols, ctx)?;
    let cso = sort_spec(arg(a, 8), ck.cols, ck.cols, ctx)?;
    let rel = int_or(arg(a, 10), 0.0, ctx)?;
    if rel < 0.0 || rel > 4.0 {
        return fail(ErrorCode::Value);
    }
    let has_hdr = hm == 1.0 || hm == 3.0;
    let all: Vec<u32> = (u32::from(has_hdr).min(vals.rows)..vals.rows).collect();
    let rows = filter_rows(all, arg(a, 9), vals.rows, ctx)?;
    if rows.is_empty() {
        return fail(ErrorCode::Calc);
    }
    let mut rg = group_rows(ctx, &rk, &rows);
    let mut cg = group_rows(ctx, &ck, &rows);
    {
        let c: &dyn FnContext = ctx;
        let key_cmp = |spec: &[f64], x: &Group, y: &Group| -> i32 {
            for &o in spec {
                let Some(i) = spec_ix(o) else { continue };
                let (Some(&p), Some(&q)) = (x.key.get(i), y.key.get(i)) else { continue };
                let cmp = cmp_total(c, p, q);
                if cmp != 0 {
                    return sort_dir(p, q, cmp, o);
                }
            }
            0
        };
        js_sort(&mut rg, |x, y| key_cmp(&rso, x, y));
        js_sort(&mut cg, |x, y| key_cmp(&cso, x, y));
    }
    let nv = vals.cols;
    let tot_cols = cg.len() as u64 + u64::from(cdepth != 0.0);
    let width64 = u64::from(rk.cols) + tot_cols * u64::from(nv);
    if width64 > u64::from(u32::MAX) {
        return Err(Stop::Throw(Throw::Range));
    }
    let width = width64 as u32;
    let empty = Scalar::Str(ctx.new_text(JsStr::EMPTY)?);
    let total = Scalar::Str(ctx.new_text(JsStr::Latin1(b"Total"))?);
    let mut out: Vec<Vec<Scalar>> = Vec::new();
    // headers: the column field names (modes 2/3), the column keys, the row names + values (modes 2/3)
    let show_hdr = hm == 2.0 || hm == 3.0;
    if show_hdr {
        for lvl in 0..ck.cols {
            let name = if hm == 3.0 { ck.get(0, lvl) } else { gen_header(ctx, b'c', lvl + 1)? };
            let mut l = Vec::new();
            blanks(&mut l, i64::from(rk.cols), empty);
            l.push(name);
            blanks(&mut l, i64::from(width) - i64::from(rk.cols) - 1, empty);
            out.push(l);
        }
    }
    for lvl in 0..ck.cols {
        let mut l = Vec::new();
        blanks(&mut l, i64::from(rk.cols), empty);
        for g in &cg {
            for _ in 0..nv {
                l.push(g.key.get(lvl as usize).copied().unwrap_or(Scalar::Empty));
            }
        }
        if cdepth != 0.0 {
            for _ in 0..nv {
                l.push(if lvl == 0 { total } else { empty });
            }
        }
        out.push(l);
    }
    if show_hdr {
        let mut l = Vec::new();
        for c in 0..rk.cols {
            l.push(if hm == 3.0 { rk.get(0, c) } else { gen_header(ctx, b'r', c + 1)? });
        }
        for _ in 0..tot_cols {
            for v in 0..nv {
                l.push(if hm == 3.0 { vals.get(0, v) } else { gen_header(ctx, b'v', v + 1)? });
            }
        }
        out.push(l);
    }
    // relative_to (PERCENTOF): 0 the column's total, 1 the row's, 2 the grand total
    let denom = |rr: &[u32], cc: Option<&[u32]>| -> Vec<u32> {
        if rel == 1.0 {
            rr.to_vec()
        } else if rel == 2.0 || cc.is_none() {
            rows.clone()
        } else {
            cc.map(<[u32]>::to_vec).unwrap_or_default()
        }
    };
    let mut mark: Vec<u32> = vec![0; vals.rows as usize];
    let mut epoch = 0u32;
    let mut body = |g: Option<&Group>, ctx: &mut dyn FnContext| -> R<Vec<Scalar>> {
        let rr: &[u32] = g.map_or(rows.as_slice(), |g| g.rows.as_slice());
        let mut l: Vec<Scalar> = match g {
            Some(g) => g.key.clone(),
            None => {
                let mut l = vec![total];
                blanks(&mut l, i64::from(rk.cols) - 1, empty);
                l
            }
        };
        for c in &cg {
            // `rr.filter((r) => cc.includes(r))`, through a mark per row
            epoch = epoch.wrapping_add(1);
            for &r in &c.rows {
                mark[r as usize] = epoch;
            }
            let both: Vec<u32> = rr.iter().copied().filter(|&r| mark[r as usize] == epoch).collect();
            for v in 0..nv {
                l.push(if both.is_empty() { empty } else { aggregate(f, &vals, &both, v, &denom(rr, Some(&c.rows)), ctx)? });
            }
        }
        if cdepth != 0.0 {
            for v in 0..nv {
                l.push(aggregate(f, &vals, rr, v, &denom(rr, None), ctx)?);
            }
        }
        Ok(l)
    };
    if rdepth < 0.0 {
        out.push(body(None, ctx)?);
    }
    for g in &rg {
        out.push(body(Some(g), ctx)?);
    }
    if rdepth > 0.0 {
        out.push(body(None, ctx)?);
    }
    from_rows(&out, width)
}
