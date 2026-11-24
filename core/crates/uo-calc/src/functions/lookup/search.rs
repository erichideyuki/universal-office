//! `lookup.ts`: VLOOKUP, HLOOKUP, MATCH, LOOKUP, XLOOKUP, XMATCH, ADDRESS, TRANSPOSE, HYPERLINK, GETPIVOTDATA, RTD,
//! IMAGE, IMPORTCSV, IMPORTTEXT, FIELDVALUE.
//!
//! The searches are the TS's step for step (the binary search of MATCH 1/VLOOKUP/HLOOKUP/LOOKUP over unsorted
//! mixed vectors, measured in Excel: middle = floor, a middle of another type walks forward to the first of the
//! searched type inside `[lo, hi]`; MATCH −1 linear; exact = first equal with the `* ? ~` wildcards; XLOOKUP/XMATCH
//! over the total order number < text < boolean < error < empty, linear or binary).

use std::sync::Arc;

use crate::apply::implicit_intersection;
use crate::coerce::{same_text_ci, to_bool, to_text};
use crate::collate::text_equals;
use crate::criteria::Wildcard;
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::key::{Area, MAX_COL, MAX_ROW};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

use super::super::shared::{R, Stop, arg, bodies, err, num, scalar, tdl, text, to_num_d};
use super::line::{LineVec, of_area_cached};
use super::{Kind, cmp_same, cmp_total, fail, kind, trunc, view, xrank};

pub(super) const LOOKUP: &[(&str, FnBody)] = bodies![
    "VLOOKUP" => vlookup,
    "HLOOKUP" => hlookup,
    "MATCH" => match_,
    "LOOKUP" => lookup,
    "XLOOKUP" => xlookup,
    "XMATCH" => xmatch,
    "ADDRESS" => address,
    "TRANSPOSE" => transpose,
    "HYPERLINK" => hyperlink,
    "GETPIVOTDATA" => getpivotdata,
    "RTD" => rtd,
    "IMAGE" => image,
    "IMPORTCSV" => connect,
    "IMPORTTEXT" => connect,
    "FIELDVALUE" => fieldvalue,
];

// ───────────────────────────── vectors ─────────────────────────────

/// `Shape` (rows × columns) of an array value or a one-area reference.
#[derive(Clone, Copy)]
struct Shape {
    rows: u32,
    cols: u32,
}

/// `shapeOf(v)`: a reference of several areas → `#VALUE!`, a LAMBDA → `#CALC!`, a scalar → 1 × 1.
fn shape_of(v: &Value) -> Result<Shape, ErrorCode> {
    match v {
        Value::Ref(r) => r.single().map(|a| Shape { rows: a.r2 - a.r1 + 1, cols: a.c2 - a.c1 + 1 }).ok_or(ErrorCode::Value),
        Value::Array(a) => Ok(Shape { rows: a.rows, cols: a.cols }),
        Value::Lambda(_) => Err(ErrorCode::Calc),
        Value::Scalar(_) => Ok(Shape { rows: 1, cols: 1 }),
    }
}

/// The array of a non-reference value (`v instanceof ArrayValue ? v : new ArrayValue(1, 1, [v])`).
fn arr_of(v: &Value) -> ArrayValue {
    match v {
        Value::Array(a) => a.clone(),
        Value::Scalar(s) => super::super::shared::one(*s),
        _ => super::super::shared::one(Scalar::Err(ErrorCode::Value)),
    }
}

/// `lineOf(v, ctx, horizontal, k)`: row `k` (horizontal) or column `k` of an array or a one-area reference.
fn line_of(v: &Value, ctx: &mut dyn FnContext, horizontal: bool, k: u32) -> R<Arc<LineVec>> {
    if let Value::Ref(r) = v {
        let Some(a) = r.single() else { return fail(ErrorCode::Value) };
        let line = if horizontal { Area { r1: a.r1 + k, r2: a.r1 + k, ..a } } else { Area { c1: a.c1 + k, c2: a.c1 + k, ..a } };
        return of_area_cached(line, ctx);
    }
    let arr = arr_of(v);
    let xs: Vec<Scalar> =
        if horizontal { (0..arr.cols).map(|c| arr.get(k, c)).collect() } else { (0..arr.rows).map(|r| arr.get(r, k)).collect() };
    Ok(Arc::new(LineVec::of_array(xs)))
}

/// `cellOf(v, ctx, r, c)`: the value at (r, c) of an array or a reference (empty = `null`).
fn cell_of(v: &Value, ctx: &mut dyn FnContext, r: u32, c: u32) -> R<Scalar> {
    Ok(match v {
        Value::Ref(x) => match x.single() {
            Some(a) => ctx.cell(a.sheet, a.r1 + r, a.c1 + c)?,
            None => Scalar::Err(ErrorCode::Value),
        },
        Value::Array(a) => a.get(r, c),
        Value::Scalar(s) => *s,
        Value::Lambda(_) => Scalar::Err(ErrorCode::Value),
    })
}

// ───────────────────────────── comparisons ─────────────────────────────

/// `/[*?~]/.test(x)`.
fn has_wild_chars(s: JsStr<'_>) -> bool {
    s.units().any(|u| u == u16::from(b'*') || u == u16::from(b'?') || u == u16::from(b'~'))
}

/// `lookupRegExp(pattern)`: `^…$/i` with `*`/`?` and `~` making any unit literal; a trailing `~` disappears (the
/// criteria wildcard keeps it as a literal: same matcher once it is dropped).
fn lookup_wildcard(pattern: JsStr<'_>) -> Wildcard {
    let mut u = pattern.to_utf16();
    let mut i = 0;
    while i < u.len() {
        if u[i] == u16::from(b'~') {
            if i + 1 == u.len() {
                u.pop();
                break;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Wildcard::new(&u)
}

/// `exactPred(x, wildcard, collate)`: the equality of the exact searches.
enum Pred {
    /// A wildcard text.
    Wild(Wildcard),
    /// A text: by the collation (`collate`, XLOOKUP/XMATCH: `ß` = `ss`) or case-insensitively (`sameTextCI`).
    Text { collate: bool },
    /// A number or a boolean: `v === x`.
    Same,
    /// `() => false`.
    Never,
}

fn exact_pred(ctx: &dyn FnContext, x: Scalar, wildcard: bool, collate: bool) -> Pred {
    match x {
        Scalar::Str(r) => {
            let s = ctx.text(r);
            if wildcard && has_wild_chars(s) { Pred::Wild(lookup_wildcard(s)) } else { Pred::Text { collate } }
        }
        Scalar::Num(_) | Scalar::Bool(_) => Pred::Same,
        Scalar::Empty | Scalar::Err(_) => Pred::Never,
    }
}

impl Pred {
    fn test(&self, ctx: &dyn FnContext, x: Scalar, v: Scalar) -> bool {
        match self {
            Self::Wild(w) => matches!(v, Scalar::Str(s) if w.matches(ctx.text(s))),
            Self::Text { collate } => match (v, x) {
                (Scalar::Str(a), Scalar::Str(b)) => {
                    if *collate {
                        text_equals(ctx.text(a), ctx.text(b))
                    } else {
                        same_text_ci(ctx.text(a), ctx.text(b))
                    }
                }
                _ => false,
            },
            Self::Same => match (v, x) {
                (Scalar::Num(a), Scalar::Num(b)) => a == b,
                (Scalar::Bool(a), Scalar::Bool(b)) => a == b,
                _ => false,
            },
            Self::Never => false,
        }
    }
}

// ───────────────────────────── searches ─────────────────────────────

/// `findExact(vec, x, wildcard)`: MATCH 0 / VLOOKUP exact — the first equal (0-based, −1 = none).
fn find_exact(ctx: &dyn FnContext, vec: &LineVec, x: Scalar, wildcard: bool) -> i64 {
    let wild = wildcard && matches!(x, Scalar::Str(r) if has_wild_chars(ctx.text(r)));
    if !wild {
        return vec.exact_first(x, ctx);
    }
    let p = exact_pred(ctx, x, wildcard, false);
    let mut hit = -1;
    vec.scan(0, i64::from(vec.n) - 1, false, &mut |i, v| {
        if p.test(ctx, x, v) {
            hit = i;
            return true;
        }
        false
    });
    hit
}

/// `findApprox(vec, x)`: MATCH 1 / VLOOKUP / HLOOKUP / LOOKUP approximate — Excel's binary search (see the module).
fn find_approx(ctx: &dyn FnContext, vec: &LineVec, x: Scalar) -> i64 {
    let kx = kind(x);
    if kx == Kind::X {
        return -1;
    }
    let (mut lo, mut hi, mut r) = (0i64, i64::from(vec.n) - 1, -1i64);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let (mut k, mut kv) = (-1i64, Scalar::Empty);
        let here = vec.get(mid);
        if !matches!(here, Scalar::Empty) && kind(here) == kx {
            k = mid;
            kv = here;
        } else {
            vec.scan(mid, hi, false, &mut |i, v| {
                if kind(v) == kx {
                    k = i;
                    kv = v;
                    return true;
                }
                false
            });
        }
        if k < 0 {
            hi = mid - 1;
            continue;
        }
        let c = cmp_same(ctx, kv, x);
        if c == 0 {
            while k + 1 < i64::from(vec.n) {
                let nx = vec.get(k + 1);
                if matches!(nx, Scalar::Empty) || kind(nx) != kx || cmp_same(ctx, nx, x) != 0 {
                    break;
                }
                k += 1;
            }
            return k;
        }
        if c < 0 {
            r = k;
            lo = k + 1;
        } else {
            hi = k - 1;
        }
    }
    r
}

/// `findDesc(vec, x)`: MATCH −1, linear (other types ignored; the first equal, or the one before the first smaller).
fn find_desc(ctx: &dyn FnContext, vec: &LineVec, x: Scalar) -> i64 {
    let kx = kind(x);
    if kx == Kind::X {
        return -1;
    }
    let (mut r, mut out) = (-1i64, -2i64);
    vec.scan(0, i64::from(vec.n) - 1, false, &mut |i, v| {
        if kind(v) != kx {
            return false;
        }
        let c = cmp_same(ctx, v, x);
        if c == 0 {
            out = i;
            return true;
        }
        if c < 0 {
            out = r;
            return true;
        }
        r = i;
        false
    });
    if out == -2 { r } else { out }
}

/// `xfind(vec, x, mm, sm, ctx)`: XLOOKUP/XMATCH — `Ok(None)` = invalid combination (`#VALUE!`), else the 0-based
/// index or −1.
fn xfind(ctx: &dyn FnContext, vec: &LineVec, x: Scalar, mm: f64, sm: f64) -> R<Option<i64>> {
    if sm.abs() == 2.0 {
        if mm == 2.0 || mm == 3.0 {
            return Ok(None);
        }
        return Ok(Some(if sm == 2.0 { xbin(ctx, vec, x, mm, false) } else { xbin(ctx, vec, x, mm, true) }));
    }
    let rev = sm == -1.0;
    let last = i64::from(vec.n) - 1;
    if matches!(x, Scalar::Empty) && (mm == 0.0 || mm == 2.0 || mm == 3.0) {
        // empty searched: the first (last) empty cell
        return Ok(Some(vec.blank(rev)));
    }
    if mm == 0.0 || mm == 2.0 {
        let p = exact_pred(ctx, x, mm == 2.0, true);
        if vec.indexed() && !matches!(p, Pred::Wild(_)) {
            // the vector of a reference, kept for the pass: its collation index (the same first / last hit)
            return Ok(Some(vec.collate_find(x, ctx, rev)));
        }
        let mut hit = -1;
        vec.scan(0, last, rev, &mut |i, v| {
            if p.test(ctx, x, v) {
                hit = i;
                return true;
            }
            false
        });
        return Ok(Some(hit));
    }
    if mm == 3.0 {
        let pat: Vec<u16> = match x {
            Scalar::Str(r) => ctx.text(r).to_utf16(),
            Scalar::Empty => Vec::new(),
            x => match to_text(view(ctx, x), ctx.locale()) {
                Ok(t) => t.as_js().to_utf16(),
                // `toText(x) as string`: an error object would be stringified by the regex; never reached (errors
                // stop before the body)
                Err(e) => crate::cell::error_text(e).encode_utf16().collect(),
            },
        };
        let Some(re) = crate::functions::text::compile_pcre(&pat, false) else { return Ok(None) };
        let mut hit = -1;
        let mut buf: Vec<u16> = Vec::new();
        let mut throw = false;
        vec.scan(0, last, rev, &mut |i, v| {
            if matches!(v, Scalar::Err(_)) {
                return false;
            }
            buf.clear();
            match v {
                Scalar::Str(r) => buf.extend(ctx.text(r).units()),
                v => {
                    if let Ok(t) = to_text(view(ctx, v), ctx.locale()) {
                        buf.extend(t.as_js().units());
                    }
                }
            }
            // a step budget per test (decision A4d-1: the TS never returns from a catastrophic pattern)
            crate::functions::text::start_budget();
            let m = re.is_match(&buf);
            if crate::functions::text::check_budget().is_err() {
                throw = true;
                return true;
            }
            if m {
                hit = i;
                return true;
            }
            false
        });
        if throw {
            return Err(Stop::Throw(Throw::Range));
        }
        return Ok(Some(hit));
    }
    // linear approximate: exact at the first found; else the best (tie: the first found)
    let (mut best, mut best_v, mut exact) = (-1i64, Scalar::Empty, -1i64);
    vec.scan(0, last, rev, &mut |i, v| {
        let c = cmp_total(ctx, v, x);
        if c == 0 && xrank(v) < 3 {
            exact = i;
            return true;
        }
        if mm == -1.0 && c < 0 && (best < 0 || cmp_total(ctx, v, best_v) > 0) {
            best = i;
            best_v = v;
        }
        if mm == 1.0 && c > 0 && (best < 0 || cmp_total(ctx, v, best_v) < 0) {
            best = i;
            best_v = v;
        }
        false
    });
    if exact >= 0 {
        return Ok(Some(exact));
    }
    // empty is the largest of all: the "next larger" only when nothing else is
    if mm == 1.0 && best < 0 {
        return Ok(Some(vec.blank(rev)));
    }
    Ok(Some(best))
}

/// `xbinAsc`/`xbinDesc`: the binary search modes 2 (leftmost among equals) and −2 (descending, rightmost).
fn xbin(ctx: &dyn FnContext, vec: &LineVec, x: Scalar, mm: f64, desc: bool) -> i64 {
    let (mut lo, mut hi) = (0i64, i64::from(vec.n) - 1);
    let (mut eq, mut rlt, mut rgt) = (-1i64, -1i64, -1i64);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let v = vec.get(mid);
        let c = cmp_total(ctx, v, x);
        if c == 0 && xrank(v) < 3 {
            eq = mid;
            if desc {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        } else if desc {
            if c > 0 {
                rgt = mid;
                lo = mid + 1;
            } else {
                rlt = mid;
                hi = mid - 1;
            }
        } else if c < 0 {
            rlt = mid;
            lo = mid + 1;
        } else {
            rgt = mid;
            hi = mid - 1;
        }
    }
    if eq >= 0 {
        eq
    } else if mm == -1.0 {
        rlt
    } else if mm == 1.0 {
        rgt
    } else {
        -1
    }
}

// ───────────────────────────── arguments ─────────────────────────────

/// `lookupValue(v)`: the searched value of MATCH/VLOOKUP/HLOOKUP/LOOKUP — empty = 0.
#[inline]
fn lookup_value(v: Option<Scalar>) -> Scalar {
    match v {
        None | Some(Scalar::Empty) => Scalar::Num(0.0),
        Some(v) => v,
    }
}

/// `intArg(v, ctx)`: the truncated number (text numbers accepted).
#[inline]
fn int_arg(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    to_num_d(ctx, v).map(trunc)
}

/// `rangeLookup(args, i, ctx)`: absent → TRUE, omitted (`;`) → FALSE, else `toBool`.
fn range_lookup(a: &CallArgs<'_>, ctx: &dyn FnContext, i: usize) -> Result<bool, ErrorCode> {
    match scalar(a, i) {
        None => Ok(true),
        Some(Scalar::Empty) => Ok(false),
        Some(v) => to_bool(view(ctx, v), ctx.locale()),
    }
}

/// VLOOKUP/HLOOKUP.
fn vhlookup(horizontal: bool, a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = lookup_value(scalar(a, 0));
    let approx = range_lookup(a, ctx, 3)?;
    let k = int_arg(ctx, scalar(a, 2).unwrap_or(Scalar::Empty))?;
    let table = arg(a, 1).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = table {
        return fail(*e);
    }
    if !matches!(table, Value::Ref(_) | Value::Array(_)) {
        // a scalar table (measured)
        return fail(ErrorCode::NA);
    }
    let Ok(sh) = shape_of(table) else { return fail(ErrorCode::Ref) };
    // the search comes before the column check (measured: not found → #N/A even with column 0 or past the end)
    let key = line_of(table, ctx, horizontal, 0)?;
    let i = if approx { find_approx(ctx, &key, x) } else { find_exact(ctx, &key, x, true) };
    if i < 0 {
        return fail(ErrorCode::NA);
    }
    if k < 1.0 {
        return fail(ErrorCode::Value);
    }
    if k > f64::from(if horizontal { sh.rows } else { sh.cols }) {
        return fail(ErrorCode::Ref);
    }
    if k.is_nan() {
        // `cellOf(table, ctx, i, NaN)`: no such cell
        return Ok(Value::Scalar(Scalar::Empty));
    }
    // 1 ≤ k ≤ the shape, 0 ≤ i < the line: in range
    let (k, i) = (crate::cast::to_u32(k) - 1, i as u32);
    let v = if horizontal { cell_of(table, ctx, k, i)? } else { cell_of(table, ctx, i, k)? };
    Ok(Value::Scalar(v))
}

fn vlookup(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    vhlookup(false, a, ctx)
}

fn hlookup(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    vhlookup(true, a, ctx)
}

/// `vector1d(v, ctx)`: the 1-D vector of a MATCH/XMATCH/XLOOKUP argument; `None` = 2-D.
fn vector_1d(v: &Value, ctx: &mut dyn FnContext) -> R<Option<(Arc<LineVec>, bool)>> {
    if let Value::Scalar(Scalar::Err(e)) = v {
        return fail(*e);
    }
    let sh = shape_of(v)?;
    if sh.rows > 1 && sh.cols > 1 {
        return Ok(None);
    }
    let horizontal = sh.rows == 1 && sh.cols > 1;
    Ok(Some((line_of(v, ctx, horizontal, 0)?, horizontal)))
}

fn match_(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = lookup_value(scalar(a, 0));
    let mut ty = 1.0;
    if let Some(tv) = scalar(a, 2) {
        let n = to_num_d(ctx, tv)?;
        let s = uo_jsnum::math::sign(trunc(n));
        ty = if s != 0.0 && !s.is_nan() {
            s
        } else if n > 0.0 {
            1.0
        } else if n < 0.0 {
            -1.0
        } else {
            0.0
        };
    }
    let arr = arg(a, 1).unwrap_or(&super::super::shared::EMPTY);
    if !matches!(arr, Value::Ref(_) | Value::Array(_)) {
        // a scalar (measured)
        return match arr {
            Value::Scalar(Scalar::Err(e)) => fail(*e),
            _ => fail(ErrorCode::NA),
        };
    }
    let Some((vec, _)) = vector_1d(arr, ctx)? else { return fail(ErrorCode::NA) };
    let i = if ty == 0.0 {
        find_exact(ctx, &vec, x, true)
    } else if ty > 0.0 {
        find_approx(ctx, &vec, x)
    } else {
        find_desc(ctx, &vec, x)
    };
    if i < 0 { fail(ErrorCode::NA) } else { Ok(num(i as f64 + 1.0)) }
}

fn lookup(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = lookup_value(scalar(a, 0));
    let lv = arg(a, 1).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = lv {
        return fail(*e);
    }
    let sh = shape_of(lv)?;
    let Some(rv) = arg(a, 2) else {
        // array form: more columns than rows → first / last row; else first / last column
        let horizontal = sh.cols > sh.rows;
        let line = line_of(lv, ctx, horizontal, 0)?;
        let i = find_approx(ctx, &line, x);
        if i < 0 {
            return fail(ErrorCode::NA);
        }
        let i = i as u32;
        let v = if horizontal { cell_of(lv, ctx, sh.rows.saturating_sub(1), i)? } else { cell_of(lv, ctx, i, sh.cols.saturating_sub(1))? };
        return Ok(Value::Scalar(v));
    };
    let horizontal = sh.rows == 1 && sh.cols > 1;
    let line = line_of(lv, ctx, horizontal, 0)?;
    let i = find_approx(ctx, &line, x);
    if i < 0 {
        return fail(ErrorCode::NA);
    }
    let i = i as u32;
    if let Value::Scalar(Scalar::Err(e)) = rv {
        return fail(*e);
    }
    // result vector: a reference extends past its end (measured); a short array → #N/A
    if let Value::Ref(r) = rv {
        let Some(a) = r.single() else { return fail(ErrorCode::Ref) };
        let rh = a.r1 == a.r2 && a.c1 != a.c2;
        let (row, col) =
            if rh { (u64::from(a.r1), u64::from(a.c1) + u64::from(i)) } else { (u64::from(a.r1) + u64::from(i), u64::from(a.c1)) };
        if row > u64::from(MAX_ROW) || col > u64::from(MAX_COL) {
            return fail(ErrorCode::Ref);
        }
        return Ok(Value::Scalar(ctx.cell(a.sheet, row as u32, col as u32)?));
    }
    let rs = shape_of(rv)?;
    let rh = rs.rows == 1 && rs.cols > 1;
    if i >= (if rh { rs.cols } else { rs.rows }) {
        return fail(ErrorCode::NA);
    }
    let v = if rh { cell_of(rv, ctx, 0, i)? } else { cell_of(rv, ctx, i, 0)? };
    Ok(Value::Scalar(v))
}

/// `modeArg(v, def, ok, ctx)`: the XLOOKUP/XMATCH modes (integers); a text → `#VALUE!` unless it is a number.
fn mode_arg(ctx: &dyn FnContext, v: Option<Scalar>, def: f64, ok: &[f64]) -> Result<f64, ErrorCode> {
    let v = match v {
        None | Some(Scalar::Empty) => return Ok(def),
        Some(v @ Scalar::Str(_)) => Scalar::Num(to_num_d(ctx, v).map_err(|_| ErrorCode::Value)?),
        Some(v) => v,
    };
    let n = int_arg(ctx, v)?;
    if ok.contains(&n) { Ok(n) } else { Err(ErrorCode::Value) }
}

/// `sliceAt(ret, horizontal, i, ctx)`: line `i` of XLOOKUP's return (a reference when it is a reference).
fn slice_at(ret: &Value, horizontal: bool, i: u32) -> Value {
    if let Value::Ref(r) = ret {
        let Some(a) = r.single() else { return err(ErrorCode::Value) };
        return super::area_ref(if horizontal {
            Area { c1: a.c1 + i, c2: a.c1 + i, ..a }
        } else {
            Area { r1: a.r1 + i, r2: a.r1 + i, ..a }
        });
    }
    let arr = arr_of(ret);
    if horizontal {
        if arr.rows == 1 { Value::Scalar(arr.get(0, i)) } else { Value::Array(ArrayValue::fill(arr.rows, 1, |r, _| arr.get(r, i))) }
    } else if arr.cols == 1 {
        Value::Scalar(arr.get(i, 0))
    } else {
        Value::Array(ArrayValue::fill(1, arr.cols, |_, c| arr.get(i, c)))
    }
}

const MATCH_MODES: [f64; 5] = [0.0, -1.0, 1.0, 2.0, 3.0];
const SEARCH_MODES: [f64; 4] = [1.0, -1.0, 2.0, -2.0];

fn xlookup(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = scalar(a, 0).unwrap_or(Scalar::Empty);
    let mm = mode_arg(ctx, scalar(a, 4), 0.0, &MATCH_MODES)?;
    let sm = mode_arg(ctx, scalar(a, 5), 1.0, &SEARCH_MODES)?;
    let la = arg(a, 1).unwrap_or(&super::super::shared::EMPTY);
    let Some((vec, horizontal)) = vector_1d(la, ctx)? else { return fail(ErrorCode::Value) };
    let ra = arg(a, 2).unwrap_or(&super::super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = ra {
        return fail(*e);
    }
    let rs = shape_of(ra)?;
    if (if horizontal { rs.cols } else { rs.rows }) != vec.n {
        return fail(ErrorCode::Value);
    }
    let Some(i) = xfind(ctx, &vec, x, mm, sm)? else { return fail(ErrorCode::Value) };
    if i < 0 {
        return match arg(a, 3) {
            None | Some(Value::Scalar(Scalar::Empty)) => fail(ErrorCode::NA),
            Some(nf) => Ok(nf.clone()),
        };
    }
    Ok(slice_at(ra, horizontal, i as u32))
}

fn xmatch(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = scalar(a, 0).unwrap_or(Scalar::Empty);
    let mm = mode_arg(ctx, scalar(a, 2), 0.0, &MATCH_MODES)?;
    let sm = mode_arg(ctx, scalar(a, 3), 1.0, &SEARCH_MODES)?;
    let la = arg(a, 1).unwrap_or(&super::super::shared::EMPTY);
    let Some((vec, _)) = vector_1d(la, ctx)? else { return fail(ErrorCode::Value) };
    let Some(i) = xfind(ctx, &vec, x, mm, sm)? else { return fail(ErrorCode::Value) };
    if i < 0 { fail(ErrorCode::NA) } else { Ok(num(i as f64 + 1.0)) }
}

// ───────────────────────────── ADDRESS ─────────────────────────────

/// `colName(c)` (A, …, Z, AA…); empty for `c ≤ 0` or NaN.
fn push_col_name(out: &mut Vec<u16>, c: f64) {
    let start = out.len();
    let mut n = c;
    while n > 0.0 {
        let d = (n - 1.0) % 26.0;
        out.insert(start, 65 + crate::cast::to_u32(d) as u16);
        n = ((n - 1.0) / 26.0).floor();
    }
}

/// `colNumber(s)` of 1–3 ASCII letters.
fn col_number(s: &[u16]) -> u32 {
    s.iter().fold(0u32, |n, &u| n * 26 + u32::from(u & !0x20) - 64)
}

/// `sheetPrefix(name, locale)`: a sheet name as Excel writes it in a reference (quoted when needed).
fn sheet_prefix(ctx: &dyn FnContext, name: &[u16], out: &mut Vec<u16>) {
    if name.is_empty() {
        out.push(u16::from(b'!'));
        return;
    }
    // /^(\[[^\]]+\])(.*)$/ (`.` stops at a line terminator)
    let (book, base) = match name.iter().position(|&u| u == u16::from(b']')) {
        Some(e) if name[0] == u16::from(b'[') && e > 1 && !name[e + 1..].iter().any(|&u| matches!(u, 0x0a | 0x0d | 0x2028 | 0x2029)) => {
            (&name[..=e], &name[e + 1..])
        }
        _ => (&name[..0], name),
    };
    // looks like an A1 reference up to XFD1048576 (measured: "XFE1" unquoted) or a boolean of the language → quotes
    let letters = base.iter().take_while(|&&u| u < 128 && (u as u8).is_ascii_alphabetic()).count();
    let is_cell = (1..=3).contains(&letters)
        && base.len() > letters
        && base[letters..].iter().all(|&u| (0x30..=0x39).contains(&u))
        && col_number(&base[..letters]) <= MAX_COL + 1
        && {
            let d = crate::jstext::digits_value(&base[letters..]);
            d >= 1.0 && d <= f64::from(MAX_ROW) + 1.0
        };
    let up = uo_text::case::to_uppercase(base);
    let loc = ctx.locale();
    let is_bool = crate::jstext::eq(&up, JsStr::Latin1(b"TRUE"))
        || crate::jstext::eq(&up, JsStr::Latin1(b"FALSE"))
        || *up == *uo_text::case::to_uppercase(&loc.true_text.as_js().to_utf16())
        || *up == *uo_text::case::to_uppercase(&loc.false_text.as_js().to_utf16());
    let plain = letter_name(base) && !is_cell && !is_bool && !looks_rl_c(base);
    if plain {
        out.extend_from_slice(book);
        out.extend_from_slice(base);
    } else {
        out.push(u16::from(b'\''));
        for &u in book.iter().chain(base) {
            out.push(u);
            if u == u16::from(b'\'') {
                out.push(u);
            }
        }
        out.push(u16::from(b'\''));
    }
    out.push(u16::from(b'!'));
}

/// `/^[\p{L}_][\p{L}\p{N}_.]*$/u` (by code point; a lone surrogate is neither).
pub(super) fn letter_name(s: &[u16]) -> bool {
    let mut first = true;
    let mut i = 0;
    while i < s.len() {
        let Some((cp, n)) = uo_text::utf16::decode_at(s, i) else { return false };
        let ok =
            cp == u32::from(b'_') || uo_text::props::is_letter(cp) || (!first && (cp == u32::from(b'.') || uo_text::props::is_number(cp)));
        if !ok {
            return false;
        }
        first = false;
        i += n.max(1);
    }
    !first
}

/// `/^[RrLl]\d*[Cc]\d*$/`.
fn looks_rl_c(s: &[u16]) -> bool {
    if !s.first().is_some_and(|&u| matches!(u, 0x52 | 0x72 | 0x4c | 0x6c)) {
        return false;
    }
    let mut i = 1;
    while i < s.len() && (0x30..=0x39).contains(&s[i]) {
        i += 1;
    }
    if !s.get(i).is_some_and(|&u| u == 0x43 || u == 0x63) {
        return false;
    }
    i += 1;
    s[i..].iter().all(|&u| (0x30..=0x39).contains(&u))
}

fn push_num(out: &mut Vec<u16>, x: f64) {
    out.extend(uo_jsnum::format::to_js_string(x).bytes().map(u16::from));
}

fn address(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let s = |i: usize| scalar(a, i).unwrap_or(Scalar::Empty);
    let n = super::super::shared::len(a);
    let r = int_arg(ctx, s(0))?;
    let c = int_arg(ctx, s(1))?;
    let mut abs = 1.0;
    if n > 2 && !matches!(s(2), Scalar::Empty) {
        abs = int_arg(ctx, s(2))?;
    }
    let mut a1 = true;
    if n > 3 && !matches!(s(3), Scalar::Empty) {
        a1 = to_bool(view(ctx, s(3)), ctx.locale())?;
    }
    let mut sheet: Option<Vec<u16>> = None;
    if n > 4 && !matches!(s(4), Scalar::Empty) {
        sheet = Some(tdl::txt(ctx, s(4))?.as_js().to_utf16());
    }
    if r < 1.0 || r > f64::from(MAX_ROW) + 1.0 || c < 1.0 || c > f64::from(MAX_COL) + 1.0 || abs < 1.0 || abs > 4.0 {
        return fail(ErrorCode::Value);
    }
    let abs_row = abs == 1.0 || abs == 2.0;
    let abs_col = abs == 1.0 || abs == 3.0;
    let mut out: Vec<u16> = Vec::new();
    if let Some(sh) = &sheet {
        sheet_prefix(ctx, sh, &mut out);
    }
    if a1 {
        if abs_col {
            out.push(u16::from(b'$'));
        }
        push_col_name(&mut out, c);
        if abs_row {
            out.push(u16::from(b'$'));
        }
        push_num(&mut out, r);
    } else {
        let lang = crate::i18n::get_language(&ctx.locale().id);
        let (rl, cl) = lang.map_or(("R", "C"), |l| (l.row_letter(), l.col_letter()));
        out.extend(rl.encode_utf16());
        if abs_row {
            push_num(&mut out, r);
        } else {
            out.push(u16::from(b'['));
            push_num(&mut out, r);
            out.push(u16::from(b']'));
        }
        out.extend(cl.encode_utf16());
        if abs_col {
            push_num(&mut out, c);
        } else {
            out.push(u16::from(b'['));
            push_num(&mut out, c);
            out.push(u16::from(b']'));
        }
    }
    text(ctx, JsStr::Utf16(&out))
}

// ───────────────────────────── the rest ─────────────────────────────

fn transpose(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut x = arg(a, 0).unwrap_or(&super::super::shared::EMPTY).clone();
    // a legacy formula: the array argument takes the implicit intersection (measured: #VALUE! off the row/column)
    if let Value::Ref(r) = &x
        && ctx.array_context() == Some(false)
    {
        let s = r.single();
        if !s.is_some_and(|s| s.r1 == s.r2 && s.c1 == s.c2) {
            x = Value::Scalar(implicit_intersection(r, ctx)?);
        }
    }
    if let Value::Scalar(Scalar::Err(e)) = x {
        return fail(e);
    }
    let m = tdl::as_array(&x, ctx)?;
    Ok(Value::Array(ArrayValue::fill(m.cols, m.rows, |r, c| m.get(c, r))))
}

fn hyperlink(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if super::super::shared::len(a) > 1 {
        return Ok(Value::Scalar(scalar(a, 1).unwrap_or(Scalar::Empty)));
    }
    let t = tdl::txt(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    text(ctx, JsStr::Utf16(&t))
}

fn getpivotdata(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    fail(ErrorCode::Ref)
}

fn rtd(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    fail(ErrorCode::NA)
}

fn connect(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    fail(ErrorCode::Connect)
}

fn fieldvalue(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    fail(ErrorCode::Field)
}

/// IMAGE: `/^https?:\/\/\S+$/i` → `#CONNECT!` (no network), any other text → `#VALUE!`.
fn image(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = tdl::txt(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?;
    let u = t.as_js().to_utf16();
    let ci = |i: usize, c: u8| u.get(i).is_some_and(|&x| x == u16::from(c) || x == u16::from(c.to_ascii_uppercase()));
    let ok = ci(0, b'h') && ci(1, b't') && ci(2, b't') && ci(3, b'p') && {
        let i = if ci(4, b's') { 5 } else { 4 };
        u.get(i..i + 3).is_some_and(|p| p == [0x3a, 0x2f, 0x2f])
            && u.len() > i + 3
            && u[i + 3..].iter().all(|&x| !crate::a1::is_js_space(x))
    };
    fail(if ok { ErrorCode::Connect } else { ErrorCode::Value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_and_rl_c() {
        let mut v = Vec::new();
        push_col_name(&mut v, 16384.0);
        assert_eq!(String::from_utf16_lossy(&v), "XFD");
        v.clear();
        push_col_name(&mut v, f64::NAN);
        assert!(v.is_empty());
        let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert!(looks_rl_c(&u("L1C1")) && looks_rl_c(&u("rc")) && !looks_rl_c(&u("R1")) && !looks_rl_c(&u("RC1x")));
        assert!(letter_name(&u("Plan1")) && !letter_name(&u("1a")) && !letter_name(&u("")) && letter_name(&u("_a.b")));
    }
}
