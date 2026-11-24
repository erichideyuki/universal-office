//! `reference.ts`: INDEX, OFFSET, INDIRECT, ROW, COLUMN, ROWS, COLUMNS, AREAS, SHEET, SHEETS, FORMULATEXT, ISREF,
//! ISFORMULA, CELL, INFO, SINGLE (`@x`). ANCHORARRAY (`A1#`) and `_TRO_*` are operators of the evaluator.

use crate::a1::cell_to_a1;
use crate::apply::implicit_intersection;
use crate::coerce::to_text;
use crate::fnreg::{Arg, CallArgs, FnBody, FnContext, Throw};
use crate::key::{Area, MAX_COL, MAX_ROW};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

use super::super::shared::{R, Stop, arg, ascii, bodies, boolean, num, text, to_num};
use super::search::letter_name;
use super::{area_ref, fail, trunc, view};

pub(super) const REFERENCE: &[(&str, FnBody)] = bodies![
    "INDEX" => index,
    "OFFSET" => offset,
    "INDIRECT" => indirect,
    "ROW" => row,
    "COLUMN" => column,
    "ROWS" => rows,
    "COLUMNS" => columns,
    "AREAS" => areas,
    "SHEET" => sheet,
    "SHEETS" => sheets,
    "FORMULATEXT" => formulatext,
    "ISREF" => isref,
    "ISFORMULA" => isformula,
    "CELL" => cell,
    "INFO" => info,
    "SINGLE" => single,
];

/// JS truthiness of a number (`0`, `-0` and NaN are false).
#[inline]
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `scalarArg(v, ctx)`: a raw argument as a scalar (a one-cell reference → its value; more → implicit intersection).
fn scalar_arg(v: &Value, ctx: &mut dyn FnContext) -> R<Scalar> {
    Ok(match v {
        Value::Ref(r) => match r.single() {
            Some(a) if a.r1 == a.r2 && a.c1 == a.c2 => ctx.cell(a.sheet, a.r1, a.c1)?,
            _ => implicit_intersection(r, ctx)?,
        },
        Value::Array(a) => {
            if a.rows > 0 && a.cols > 0 {
                a.get(0, 0)
            } else {
                Scalar::Err(ErrorCode::Value)
            }
        }
        Value::Lambda(_) => Scalar::Err(ErrorCode::Calc),
        Value::Scalar(s) => *s,
    })
}

/// `arrayOfRefs(args, ctx)`: OFFSET/INDIRECT with an array argument (or a range in an array context) — each element
/// would be a reference, which an array cannot hold → `#VALUE!` at every position (measured).
fn array_of_refs(vals: &[&Value], ctx: &dyn FnContext) -> R<Option<Value>> {
    let (mut rows, mut cols) = (0u32, 0u32);
    for v in vals {
        let (mut r, mut c) = (0, 0);
        match v {
            Value::Array(a) => {
                r = a.rows;
                c = a.cols;
            }
            Value::Ref(x) if ctx.array_context() != Some(false) => {
                if let Some(a) = x.single()
                    && (a.r1 != a.r2 || a.c1 != a.c2)
                {
                    r = a.r2 - a.r1 + 1;
                    c = a.c2 - a.c1 + 1;
                }
            }
            _ => {}
        }
        rows = rows.max(r);
        cols = cols.max(c);
    }
    if rows == 0 {
        return Ok(None);
    }
    if crate::value::array_too_big(rows, cols) {
        return Err(Stop::Throw(Throw::Range));
    }
    Ok(Some(Value::Array(ArrayValue::fill(rows, cols, |_, _| Scalar::Err(ErrorCode::Value)))))
}

/// `intArg(v, ctx)` of `reference.ts`: an error stays; `toNumber(v, ctx.locale)` (no `date1904`), truncated.
#[inline]
fn int_arg(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    if let Scalar::Err(e) = v {
        return Err(e);
    }
    to_num(ctx, v).map(trunc)
}

/// The values present among the raw arguments (`raw.slice(from)`; a lazy one cannot occur: no position is lazy).
fn present<'a>(a: &'a CallArgs<'_>, from: usize) -> Vec<&'a Value> {
    a.args[from.min(a.args.len())..].iter().filter_map(|x| if let Arg::Val(v) = x { Some(v) } else { None }).collect()
}

/// `perCell(a, f)`: a reference of several cells → an array of `f` per cell (ROW(A1:A3) → {1;2;3}).
fn per_cell(a: Area, ctx: &mut dyn FnContext, f: &mut dyn FnMut(&mut dyn FnContext, u32, u32) -> R<Scalar>) -> R<Value> {
    if a.r1 == a.r2 && a.c1 == a.c2 {
        return Ok(Value::Scalar(f(ctx, a.r1, a.c1)?));
    }
    let (rows, cols) = (a.r2 - a.r1 + 1, a.c2 - a.c1 + 1);
    if crate::value::array_too_big(rows, cols) {
        return Err(Stop::Throw(Throw::Range));
    }
    Ok(Value::Array(ArrayValue::try_fill(rows, cols, |r, c| f(ctx, a.r1 + r, a.c1 + c))?))
}

// ───────────────────────────── INDEX ─────────────────────────────

/// `indexArea(a, row, col, colGiven)`: a reference (row/column 0 = whole).
fn index_area(a: Area, mut row: f64, mut col: f64, col_given: bool) -> R<Value> {
    let rows = f64::from(a.r2 - a.r1 + 1);
    let cols = f64::from(a.c2 - a.c1 + 1);
    if !col_given {
        // one index: a one-row vector takes it as the column; a 2-D area → #REF! (measured)
        if rows == 1.0 && cols > 1.0 {
            col = row;
            row = 1.0;
        } else if cols == 1.0 {
            col = 1.0;
        } else {
            return fail(ErrorCode::Ref);
        }
    }
    if row < 0.0 || col < 0.0 {
        return fail(ErrorCode::Value);
    }
    if row > rows || col > cols {
        return fail(ErrorCode::Ref);
    }
    // 1 ≤ row ≤ rows, 1 ≤ col ≤ cols when truthy
    let r1 = if truthy(row) { a.r1 + crate::cast::to_u32(row) - 1 } else { a.r1 };
    let r2 = if truthy(row) { r1 } else { a.r2 };
    let c1 = if truthy(col) { a.c1 + crate::cast::to_u32(col) - 1 } else { a.c1 };
    let c2 = if truthy(col) { c1 } else { a.c2 };
    Ok(area_ref(Area { sheet: a.sheet, r1, c1, r2, c2 }))
}

/// `indexArray(m, row, col, colGiven)`.
fn index_array(m: &ArrayValue, mut row: f64, mut col: f64, col_given: bool) -> R<Value> {
    if !col_given {
        if m.rows == 1 && m.cols > 1 {
            col = row;
            row = 1.0;
        } else {
            col = if m.cols == 1 { 1.0 } else { 0.0 };
        }
    }
    if row < 0.0 || col < 0.0 {
        return fail(ErrorCode::Value);
    }
    if row > f64::from(m.rows) || col > f64::from(m.cols) {
        return fail(ErrorCode::Ref);
    }
    let (r, c) = (truthy(row), truthy(col));
    let ri = if r { crate::cast::to_u32(row) - 1 } else { 0 };
    let ci = if c { crate::cast::to_u32(col) - 1 } else { 0 };
    Ok(match (r, c) {
        (true, true) => Value::Scalar(m.get(ri, ci)),
        (true, false) => Value::Array(ArrayValue::fill(1, m.cols, |_, c| m.get(ri, c))),
        (false, true) => Value::Array(ArrayValue::fill(m.rows, 1, |r, _| m.get(r, ci))),
        (false, false) => Value::Array(m.clone()),
    })
}

fn index(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let arr = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = arr {
        return fail(*e);
    }
    let s = |i: usize| super::super::shared::scalar(a, i);
    let row = match s(1) {
        Some(Scalar::Empty) | None => 0.0,
        Some(v) => int_arg(ctx, v)?,
    };
    let c = s(2);
    let col_given = c.is_some();
    let col = match c {
        None | Some(Scalar::Empty) => 0.0,
        Some(v) => int_arg(ctx, v)?,
    };
    let ar = s(3);
    if let Value::Ref(r) = arr {
        let k = match ar {
            None | Some(Scalar::Empty) => 1.0,
            Some(v) => int_arg(ctx, v)?,
        };
        if k < 1.0 {
            return fail(ErrorCode::Value);
        }
        // `arr.areas[k - 1]` (a fraction or NaN is no index)
        let area = if super::super::shared::is_int(k) && k <= 1e9 { r.areas().get(crate::cast::trunc_usize(k) - 1) } else { None };
        let Some(&area) = area else { return fail(ErrorCode::Ref) };
        return index_area(area, row, col, col_given);
    }
    if let Some(v) = ar
        && !matches!(v, Scalar::Empty)
    {
        let k = int_arg(ctx, v)?;
        if k != 1.0 {
            return fail(ErrorCode::Ref);
        }
    }
    match arr {
        Value::Lambda(_) => fail(ErrorCode::Value),
        Value::Array(m) => index_array(m, row, col, col_given),
        Value::Scalar(x) => index_array(&super::super::shared::one(*x), row, col, col_given),
        Value::Ref(_) => fail(ErrorCode::Value),
    }
}

// ───────────────────────────── OFFSET / INDIRECT ─────────────────────────────

fn offset(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if let Some(arr) = array_of_refs(&present(a, 1), ctx)? {
        return Ok(arr);
    }
    let rf = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    let mut sc = [Scalar::Empty; 4];
    let n = super::super::shared::len(a);
    for (i, slot) in sc.iter_mut().enumerate().take(n.saturating_sub(1)) {
        *slot = scalar_arg(arg(a, i + 1).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    }
    let [dr, dc, h, w] = sc;
    if let Value::Scalar(Scalar::Err(e)) = rf {
        return fail(*e);
    }
    let Value::Ref(r) = rf else { return fail(ErrorCode::Value) };
    let Some(area) = r.single() else { return fail(ErrorCode::Value) };
    let rows = int_arg(ctx, dr);
    let cols = int_arg(ctx, dc);
    let rows = rows?;
    let cols = cols?;
    let height = if n < 4 || matches!(h, Scalar::Empty) { f64::from(area.r2 - area.r1 + 1) } else { int_arg(ctx, h)? };
    let width = if n < 5 || matches!(w, Scalar::Empty) { f64::from(area.c2 - area.c1 + 1) } else { int_arg(ctx, w)? };
    if height == 0.0 || width == 0.0 {
        return fail(ErrorCode::Ref);
    }
    let mut r1 = f64::from(area.r1) + rows;
    let mut c1 = f64::from(area.c1) + cols;
    let mut r2 = r1 + height - 1.0;
    let mut c2 = c1 + width - 1.0;
    if height < 0.0 {
        r2 = r1;
        r1 = r1 + height + 1.0;
    }
    if width < 0.0 {
        c2 = c1;
        c1 = c1 + width + 1.0;
    }
    if r1 < 0.0 || c1 < 0.0 || r2 > f64::from(MAX_ROW) || c2 > f64::from(MAX_COL) {
        return fail(ErrorCode::Ref);
    }
    if [r1, c1, r2, c2].iter().any(|x| x.is_nan()) {
        // the TS returns an area with NaN coordinates (only a NaN offset or size from a trap value: no formula yields
        // NaN); reading it is `new Array(NaN)`, the RangeError the trap capture records (decision A4e-6)
        return Err(Stop::Throw(Throw::Range));
    }
    let to = crate::cast::to_u32;
    Ok(area_ref(Area { sheet: area.sheet, r1: to(r1), c1: to(c1), r2: to(r2), c2: to(c2) }))
}

fn indirect(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if let Some(arr) = array_of_refs(&present(a, 0), ctx)? {
        return Ok(arr);
    }
    let t = scalar_arg(arg(a, 0).unwrap_or(&super::super::shared::EMPTY), ctx)?;
    let a1 = match arg(a, 1) {
        Some(v) => Some(scalar_arg(v, ctx)?),
        None => None,
    };
    if let Scalar::Err(e) = t {
        return fail(e);
    }
    let txt = to_text(view(ctx, t), ctx.locale())?.as_js().to_utf16();
    let mut is_a1 = true;
    if let Some(v) = a1
        && !matches!(v, Scalar::Empty)
    {
        is_a1 = match v {
            Scalar::Err(e) => return fail(e),
            Scalar::Bool(b) => b,
            Scalar::Num(n) => n != 0.0,
            _ => return fail(ErrorCode::Value),
        };
    }
    // no trimming and no "=" (measured: INDIRECT(" A1 ") and INDIRECT("=A1") → #REF!)
    if txt.is_empty() || txt[0] == u16::from(b'=') || crate::jstext::js_trim(&txt).len() != txt.len() {
        return fail(ErrorCode::Ref);
    }
    match ctx.parse_ref(JsStr::Utf16(&txt), is_a1)? {
        Ok(r) => Ok(Value::Ref(r)),
        Err(e) => fail(e),
    }
}

// ───────────────────────────── ROW … SHEETS ─────────────────────────────

/// The optional reference of ROW/COLUMN/SHEET/SHEETS (`undefined` or `null` → `None`).
fn opt_ref<'a>(a: &'a CallArgs<'_>) -> Option<&'a Value> {
    arg(a, 0).filter(|v| !matches!(v, Value::Scalar(Scalar::Empty)))
}

fn row(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(v) = opt_ref(a) else { return Ok(num(f64::from(ctx.row()) + 1.0)) };
    match v {
        Value::Scalar(Scalar::Err(e)) => fail(*e),
        Value::Ref(r) => {
            let Some(x) = r.single() else { return fail(ErrorCode::Ref) };
            if x.r1 == x.r2 {
                return Ok(num(f64::from(x.r1) + 1.0));
            }
            Ok(Value::Array(ArrayValue::fill(x.r2 - x.r1 + 1, 1, |r, _| Scalar::Num(f64::from(x.r1 + r) + 1.0))))
        }
        _ => fail(ErrorCode::Value),
    }
}

fn column(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(v) = opt_ref(a) else { return Ok(num(f64::from(ctx.col()) + 1.0)) };
    match v {
        Value::Scalar(Scalar::Err(e)) => fail(*e),
        Value::Ref(r) => {
            let Some(x) = r.single() else { return fail(ErrorCode::Ref) };
            if x.c1 == x.c2 {
                return Ok(num(f64::from(x.c1) + 1.0));
            }
            Ok(Value::Array(ArrayValue::fill(1, x.c2 - x.c1 + 1, |_, c| Scalar::Num(f64::from(x.c1 + c) + 1.0))))
        }
        _ => fail(ErrorCode::Value),
    }
}

fn rows_cols(a: &CallArgs<'_>, by_rows: bool) -> R<Value> {
    Ok(match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Scalar(Scalar::Err(e)) => return fail(*e),
        Value::Ref(r) => match r.single() {
            Some(x) => num(f64::from(if by_rows { x.r2 - x.r1 } else { x.c2 - x.c1 }) + 1.0),
            None => return fail(ErrorCode::Ref),
        },
        Value::Array(x) => num(f64::from(if by_rows { x.rows } else { x.cols })),
        Value::Lambda(_) => return fail(ErrorCode::Value),
        Value::Scalar(_) => num(1.0),
    })
}

fn rows(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    rows_cols(a, true)
}

fn columns(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    rows_cols(a, false)
}

fn areas(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Scalar(Scalar::Err(e)) => fail(*e),
        // a 3-D reference (several sheets) is not accepted (measured: #VALUE!)
        Value::Ref(r) => {
            let ar = r.areas();
            if ar.iter().any(|x| x.sheet != ar[0].sheet) { fail(ErrorCode::Value) } else { Ok(num(ar.len() as f64)) }
        }
        _ => fail(ErrorCode::Value),
    }
}

fn sheet(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(v) = opt_ref(a) else { return Ok(num(f64::from(ctx.sheet().0) + 1.0)) };
    match v {
        Value::Scalar(Scalar::Err(e)) => fail(*e),
        Value::Ref(r) => Ok(num(f64::from(r.areas().first().map_or(ctx.sheet(), |x| x.sheet).0) + 1.0)),
        Value::Scalar(Scalar::Str(s)) => match ctx.sheet_index(ctx.text(*s)) {
            Some(i) => Ok(num(f64::from(i.0) + 1.0)),
            None => fail(ErrorCode::NA),
        },
        _ => fail(ErrorCode::NA),
    }
}

fn sheets(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(v) = opt_ref(a) else { return Ok(num(f64::from(ctx.sheet_count()))) };
    match v {
        Value::Scalar(Scalar::Err(e)) => fail(*e),
        Value::Ref(r) => {
            let mut s: Vec<u32> = r.areas().iter().map(|x| x.sheet.0).collect();
            s.sort_unstable();
            s.dedup();
            Ok(num(s.len() as f64))
        }
        _ => fail(ErrorCode::NA),
    }
}

// ───────────────────────────── FORMULATEXT / ISREF / ISFORMULA ─────────────────────────────

fn formulatext(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let v = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = v {
        return fail(*e);
    }
    let Value::Ref(r) = v else { return fail(ErrorCode::NA) };
    let Some(&area) = r.areas().first() else { return fail(ErrorCode::NA) };
    let mut tr: Option<crate::i18n::Translator> = None;
    // in the calculating language, as the formula bar (Excel pt-BR: "=SOMA(A1:A3)")
    let mut one = |ctx: &mut dyn FnContext, row: u32, col: u32| -> R<Scalar> {
        let Some(f) = ctx.formula_at(area.sheet, row, col) else { return Ok(Scalar::Err(ErrorCode::NA)) };
        let mut src: Vec<u16> = vec![u16::from(b'=')];
        src.extend(f.as_js().units());
        let loc = ctx.locale();
        let opts = crate::i18n::SyntaxOptions {
            lang: crate::i18n::LangSpec::Id(&loc.id),
            decimal: Some(loc.decimal.as_js()),
            ..crate::i18n::SyntaxOptions::default()
        };
        let mut out = Vec::new();
        tr.get_or_insert_with(crate::i18n::Translator::new).to_local_into(JsStr::Utf16(&src), &opts, &mut out);
        Ok(Scalar::Str(ctx.new_text(JsStr::Utf16(&out))?))
    };
    if ctx.array_context() == Some(false) || r.areas().len() > 1 {
        return Ok(Value::Scalar(one(ctx, area.r1, area.c1)?));
    }
    per_cell(area, ctx, &mut one)
}

fn isref(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(arg(a, 0), Some(Value::Ref(_)))))
}

fn isformula(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let v = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = v {
        return fail(*e);
    }
    let Value::Ref(r) = v else { return fail(ErrorCode::Value) };
    let Some(&area) = r.areas().first() else { return fail(ErrorCode::Value) };
    let mut one =
        |ctx: &mut dyn FnContext, row: u32, col: u32| -> R<Scalar> { Ok(Scalar::Bool(ctx.formula_at(area.sheet, row, col).is_some())) };
    if ctx.array_context() == Some(false) || r.areas().len() > 1 {
        return Ok(Value::Scalar(one(ctx, area.r1, area.c1)?));
    }
    per_cell(area, ctx, &mut one)
}

// ───────────────────────────── CELL / INFO ─────────────────────────────

/// `s.trim().toLowerCase()` of an info text.
fn info_kind(s: JsStr<'_>) -> Vec<u16> {
    let u = s.to_utf16();
    crate::jstext::lower(crate::jstext::js_trim(&u)).into_owned()
}

fn is_word(k: &[u16], w: &str) -> bool {
    k.len() == w.len() && k.iter().zip(w.bytes()).all(|(&a, b)| a == u16::from(b))
}

/// `quoteName(n)` of CELL("address").
fn push_quoted_name(out: &mut Vec<u16>, n: &[u16]) {
    let letters = n.iter().take_while(|&&u| u < 128 && (u as u8).is_ascii_alphabetic()).count();
    let a1 = (1..=3).contains(&letters) && n.len() > letters && n[letters..].iter().all(|&u| (0x30..=0x39).contains(&u));
    if letter_name(n) && !a1 {
        out.extend_from_slice(n);
    } else {
        push_single_quoted(out, n);
    }
}

fn push_single_quoted(out: &mut Vec<u16>, n: &[u16]) {
    out.push(u16::from(b'\''));
    for &u in n {
        out.push(u);
        if u == u16::from(b'\'') {
            out.push(u);
        }
    }
    out.push(u16::from(b'\''));
}

fn cell(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let info = super::super::shared::scalar(a, 0).unwrap_or(Scalar::Empty);
    if let Scalar::Err(e) = info {
        return fail(e);
    }
    let Scalar::Str(info) = info else { return fail(ErrorCode::Value) };
    let area = match arg(a, 1) {
        None | Some(Value::Scalar(Scalar::Empty)) => {
            Area { sheet: ctx.sheet(), r1: ctx.row(), c1: ctx.col(), r2: ctx.row(), c2: ctx.col() }
        }
        Some(Value::Scalar(Scalar::Err(e))) => return fail(*e),
        Some(Value::Ref(r)) => match r.areas().first() {
            Some(&x) => x,
            None => return fail(ErrorCode::Value),
        },
        Some(_) => return fail(ErrorCode::Value),
    };
    let kind = info_kind(ctx.text(info));
    let k = |w: &str| is_word(&kind, w);
    let a1 = || cell_to_a1(area.r1, area.c1, true, true);
    if k("address") {
        let mut out: Vec<u16> = Vec::new();
        if area.sheet != ctx.sheet() {
            let name = ctx.sheet_name(area.sheet.0).as_js().to_utf16();
            // another sheet: Excel includes the workbook ("[Pasta1]Plan2!$A$1") when the host knows its name
            match ctx.cell_info(area.sheet, area.r1, area.c1, JsStr::Latin1(b"bookname")) {
                Some(Scalar::Str(book)) => {
                    let mut body: Vec<u16> = vec![u16::from(b'[')];
                    body.extend(ctx.text(book).units());
                    body.push(u16::from(b']'));
                    body.extend_from_slice(&name);
                    if letter_name(&name) {
                        out.extend_from_slice(&body);
                    } else {
                        push_single_quoted(&mut out, &body);
                    }
                }
                _ => push_quoted_name(&mut out, &name),
            }
            out.push(u16::from(b'!'));
        }
        out.extend(a1().encode_utf16());
        return text(ctx, JsStr::Utf16(&out));
    }
    if k("col") {
        return Ok(num(f64::from(area.c1) + 1.0));
    }
    if k("row") {
        return Ok(num(f64::from(area.r1) + 1.0));
    }
    if k("contents") {
        let x = ctx.cell(area.sheet, area.r1, area.c1)?;
        return Ok(Value::Scalar(if matches!(x, Scalar::Empty) { Scalar::Num(0.0) } else { x }));
    }
    if k("type") {
        let x = ctx.cell(area.sheet, area.r1, area.c1)?;
        return ascii(
            ctx,
            match x {
                Scalar::Empty => b"b",
                Scalar::Str(_) => b"l",
                _ => b"v",
            },
        );
    }
    if k("filename") {
        return Ok(Value::Scalar(match ctx.cell_info(area.sheet, area.r1, area.c1, JsStr::Latin1(b"filename")) {
            Some(x) => x,
            None => return ascii(ctx, b""),
        }));
    }
    for w in ["format", "color", "parentheses", "prefix", "protect", "width"] {
        if !k(w) {
            continue;
        }
        if let Some(x) = ctx.cell_info(area.sheet, area.r1, area.c1, JsStr::Latin1(w.as_bytes())) {
            return Ok(Value::Scalar(x));
        }
        // the default of a cell without formatting (the Normal style)
        return match w {
            "format" => ascii(ctx, b"G"),
            "color" | "parentheses" => Ok(num(0.0)),
            "prefix" => {
                let y = ctx.cell(area.sheet, area.r1, area.c1)?;
                ascii(ctx, if matches!(y, Scalar::Str(_)) { b"'" } else { b"" })
            }
            "protect" => Ok(num(1.0)),
            // 365: {width; is it the default width?} (measured)
            _ => Ok(Value::Array(ArrayValue::new(1, 2, vec![Scalar::Num(8.0), Scalar::Bool(true)]))),
        };
    }
    fail(ErrorCode::Value)
}

fn info(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = super::super::shared::scalar(a, 0).unwrap_or(Scalar::Empty);
    if let Scalar::Err(e) = t {
        return fail(e);
    }
    let Scalar::Str(t) = t else { return fail(ErrorCode::Value) };
    let kind = info_kind(ctx.text(t));
    let k = |w: &str| is_word(&kind, w);
    if k("directory") {
        return match ctx.cell_info(ctx.sheet(), 0, 0, JsStr::Latin1(b"directory")) {
            Some(x) => Ok(Value::Scalar(x)),
            None => fail(ErrorCode::NA),
        };
    }
    if k("numfile") {
        return Ok(num(f64::from(ctx.sheet_count())));
    }
    if k("origin") {
        return ascii(ctx, b"$A:$A$1");
    }
    if k("osversion") {
        return ascii(ctx, b"Windows (64-bit) NT 10.00");
    }
    if k("recalc") {
        return if &*ctx.locale().id == "pt-BR" { text(ctx, JsStr::Latin1(b"Autom\xe1tico")) } else { ascii(ctx, b"Automatic") };
    }
    if k("release") {
        return ascii(ctx, b"16.0");
    }
    if k("system") {
        return ascii(ctx, b"pcdos");
    }
    fail(ErrorCode::Value)
}

fn single(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    Ok(match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Ref(r) => {
            let Some(x) = r.single() else { return fail(ErrorCode::Value) };
            if x.r1 == x.r2 && x.c1 == x.c2 {
                return Ok(Value::Ref(r.clone()));
            }
            let v = implicit_intersection(r, ctx)?;
            if v == Scalar::Err(ErrorCode::Value) {
                return Ok(Value::Scalar(v));
            }
            let rr = if x.r1 == x.r2 { x.r1 } else { ctx.row() };
            let cc = if x.c1 == x.c2 { x.c1 } else { ctx.col() };
            area_ref(Area { sheet: x.sheet, r1: rr, c1: cc, r2: rr, c2: cc })
        }
        Value::Array(m) => Value::Scalar(if m.rows > 0 && m.cols > 0 { m.get(0, 0) } else { Scalar::Err(ErrorCode::Value) }),
        Value::Lambda(_) => Value::Scalar(Scalar::Err(ErrorCode::Calc)),
        Value::Scalar(s) => Value::Scalar(*s),
    })
}
