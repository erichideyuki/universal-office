//! `database.ts`: DSUM, DAVERAGE, DMAX, DMIN, DPRODUCT, DSTDEV, DSTDEVP, DVAR, DVARP, DCOUNT, DCOUNTA, DGET.
//!
//! The database's first row holds the field labels, the records follow. A field is a label (case-insensitive) or a
//! 1-based number. The criteria range: first row = labels; each row below is an alternative (OR) whose filled cells
//! must all match (AND). A computed criterion (a label that is no field, with a TRUE/FALSE formula relative to the
//! first record) is evaluated by the engine ([`FnContext::computed_criterion`], the TS `computedCriterion` hook =
//! the engine's `criterionAt`); without an engine only a constant TRUE matches.
//!
//! The reads are the TS's, in its order (the field labels, the criteria labels and rows, the records); the criteria
//! hold a copy of the locale (they are matched between cell reads, decision A4e-4).

use crate::criteria::{Criterion, parse_criterion};
use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::key::{Area, SheetIx};
use crate::locale::ValueLocale;
use crate::stats::arg_number;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

use super::super::shared::agg::{self, Agg};
use super::super::shared::{R, arg, bodies, num, scalar};
use super::{fail, trunc, view};

pub(super) const DATABASE: &[(&str, FnBody)] = bodies![
    "DSUM" => dsum,
    "DAVERAGE" => daverage,
    "DMAX" => dmax,
    "DMIN" => dmin,
    "DPRODUCT" => dproduct,
    "DSTDEV" => dstdev,
    "DSTDEVP" => dstdevp,
    "DVAR" => dvar,
    "DVARP" => dvarp,
    "DCOUNT" => dcount,
    "DCOUNTA" => dcounta,
    "DGET" => dget,
];

/// A condition: a column of the database with its criterion, or a computed criterion (the criterion cell and
/// whether its constant is TRUE, the answer without an engine).
enum Cond<'l> {
    Field(u32, Criterion<'l>),
    Computed { sheet: SheetIx, row: u32, col: u32, is_true: bool },
}

/// `String(h)` of a primitive label (number, text, boolean) compared with `textEquals`; an error or empty is no
/// label.
fn label_equals(ctx: &dyn FnContext, h: Scalar, other: &[u16]) -> bool {
    let other = crate::strings::JsStr::Utf16(other);
    match h {
        Scalar::Num(n) => crate::collate::text_equals(crate::strings::JsStr::Latin1(uo_jsnum::format::to_js_string(n).as_bytes()), other),
        Scalar::Str(s) => crate::collate::text_equals(ctx.text(s), other),
        Scalar::Bool(b) => crate::collate::text_equals(crate::strings::JsStr::Latin1(if b { b"true" } else { b"false" }), other),
        Scalar::Empty | Scalar::Err(_) => false,
    }
}

/// `String(h)` of a criteria label (text or number only).
fn label_text(ctx: &dyn FnContext, h: Scalar) -> Option<Vec<u16>> {
    match h {
        Scalar::Num(n) => Some(uo_jsnum::format::to_js_string(n).encode_utf16().collect()),
        Scalar::Str(s) => Some(ctx.text(s).to_utf16()),
        _ => None,
    }
}

/// `fieldIndex(field, db, ctx)`: the 0-based column of the field, or an error.
fn field_index(field: Scalar, db: Area, ctx: &mut dyn FnContext) -> R<u32> {
    let ncols = db.c2 - db.c1 + 1;
    if let Scalar::Err(e) = field {
        return fail(e);
    }
    if let Scalar::Str(s) = field {
        let f = ctx.text(s).to_utf16();
        for c in 0..ncols {
            let h = ctx.cell(db.sheet, db.r1, db.c1 + c)?;
            if label_equals(ctx, h, &f) {
                return Ok(c);
            }
        }
        return fail(ErrorCode::Value);
    }
    let n = arg_number(view(ctx, field), ctx.locale(), ctx.year())?;
    let i = trunc(n);
    if i >= 1.0 && i <= f64::from(ncols) { Ok(crate::cast::to_u32(i) - 1) } else { fail(ErrorCode::Value) }
}

/// `parseCriteria(cr, db, ctx)`: the alternatives (criteria rows), each a list of conditions.
fn parse_criteria<'l>(cr: Area, db: Area, ctx: &mut dyn FnContext, loc: &'l ValueLocale) -> R<Vec<Vec<Cond<'l>>>> {
    let ncols = db.c2 - db.c1 + 1;
    let mut headers: Vec<Option<u32>> = Vec::with_capacity((cr.c2 - cr.c1 + 1) as usize);
    for c in cr.c1..=cr.c2 {
        let h = ctx.cell(cr.sheet, cr.r1, c)?;
        let mut idx = None;
        if let Some(t) = label_text(ctx, h) {
            for k in 0..ncols {
                let dh = ctx.cell(db.sheet, db.r1, db.c1 + k)?;
                if label_equals(ctx, dh, &t) {
                    idx = Some(k);
                    break;
                }
            }
        }
        headers.push(idx);
    }
    // the criteria rows: only existing ones count; an all-empty row matches everything
    let mut cells: Vec<(u32, u32, Scalar)> = Vec::new();
    if cr.r1 < cr.r2 {
        ctx.for_each_cell(Area { r1: cr.r1 + 1, ..cr }, &mut |v, r, c| {
            if !matches!(v, Scalar::Empty) {
                cells.push((r, c, v));
            }
        })?;
    }
    let total_rows = cr.r2 - cr.r1;
    if total_rows == 0 {
        return fail(ErrorCode::Value);
    }
    let year = ctx.year();
    // rows in ascending order (`[...rows.keys()].sort()`); the cells of a row keep their walk order
    let mut rows: Vec<(u32, Vec<Cond<'l>>)> = Vec::new();
    for (r, c, v) in cells {
        let cond = match headers.get((c - cr.c1) as usize).copied().flatten() {
            // a label that is no field: a computed criterion (a TRUE/FALSE formula relative to the first record); a
            // constant: only TRUE matches (measured: "Apple" → nothing)
            None => Cond::Computed { sheet: cr.sheet, row: r, col: c, is_true: v == Scalar::Bool(true) },
            Some(idx) => Cond::Field(idx, parse_criterion(view(ctx, v), loc, year, true)),
        };
        match rows.iter_mut().find(|x| x.0 == r) {
            Some(x) => x.1.push(cond),
            None => rows.push((r, vec![cond])),
        }
    }
    rows.sort_by_key(|x| x.0);
    let mut alts: Vec<Vec<Cond<'l>>> = Vec::with_capacity(rows.len() + 1);
    // empty rows (no condition) match any record
    if (rows.len() as u64) < u64::from(total_rows) {
        alts.push(Vec::new());
    }
    alts.extend(rows.into_iter().map(|x| x.1));
    Ok(alts)
}

/// `matched(args, ctx, fieldRequired)`: the field's values of the matching records (row order); `None` in a value
/// is the TS `true` pushed when there is no field (DCOUNT/DCOUNTA without one).
fn matched(a: &CallArgs<'_>, ctx: &mut dyn FnContext, field_required: bool) -> R<Vec<Option<Scalar>>> {
    let db_ref = arg(a, 0).unwrap_or(&super::super::shared::EMPTY);
    let field = scalar(a, 1).unwrap_or(Scalar::Empty);
    let cr_ref = arg(a, 2).unwrap_or(&super::super::shared::EMPTY);
    // an error in the database or the criteria propagates (measured: DCOUNT(undefined_name;2;A1:A2) = #NAME?)
    if let Value::Scalar(Scalar::Err(e)) = db_ref {
        return fail(*e);
    }
    if let Scalar::Err(e) = field {
        return fail(e);
    }
    if let Value::Scalar(Scalar::Err(e)) = cr_ref {
        return fail(*e);
    }
    let one_area = |v: &Value| match v {
        Value::Ref(r) if r.areas().len() == 1 => Some(r.areas()[0]),
        _ => None,
    };
    let Some(db) = one_area(db_ref) else { return fail(ErrorCode::Value) };
    let Some(cr) = one_area(cr_ref) else { return fail(ErrorCode::Value) };
    // a database with only the label row → #VALUE! (measured)
    if db.r2 <= db.r1 {
        return fail(ErrorCode::Value);
    }
    let mut fi = None;
    if !matches!(field, Scalar::Empty) || field_required {
        fi = Some(field_index(field, db, ctx)?);
    }
    let loc = ctx.locale().clone();
    let alts = parse_criteria(cr, db, ctx, &loc)?;
    // the records: the database rows that have some cell
    let mut rec_rows: Vec<u32> = Vec::new();
    ctx.for_each_cell(Area { r1: db.r1 + 1, ..db }, &mut |_, r, _| {
        if rec_rows.last() != Some(&r) {
            rec_rows.push(r);
        }
    })?;
    rec_rows.sort_unstable();
    rec_rows.dedup();
    let mut vals = Vec::new();
    for r in rec_rows {
        let rec = r - db.r1 - 1;
        let mut ok = false;
        for conds in &alts {
            let mut all = true;
            for cond in conds {
                let hit = match cond {
                    Cond::Field(col, crit) => {
                        let v = ctx.cell(db.sheet, r, db.c1 + col)?;
                        crit.matches(view(ctx, v))
                    }
                    Cond::Computed { sheet, row, col, is_true } => match ctx.computed_criterion(*sheet, *row, *col, rec)? {
                        Some(b) => b,
                        None => *is_true,
                    },
                };
                if !hit {
                    all = false;
                    break;
                }
            }
            if all {
                ok = true;
                break;
            }
        }
        if !ok {
            continue;
        }
        vals.push(match fi {
            None => None,
            Some(fi) => Some(ctx.cell(db.sheet, r, db.c1 + fi)?),
        });
    }
    Ok(vals)
}

/// `withCritArray`: a criteria argument that is an array (not a reference) → `#VALUE!` at each element (measured:
/// it spills).
fn crit_array(a: &CallArgs<'_>) -> Option<Value> {
    match arg(a, 2) {
        Some(Value::Array(x)) => Some(Value::Array(ArrayValue::fill(x.rows, x.cols, |_, _| Scalar::Err(ErrorCode::Value)))),
        _ => None,
    }
}

/// `plainSum(xs)`: the plain sum, WITHOUT Excel's zero (measured: 0.1 + 0.2 − 0.3 = 5.55E−17, as SUMIF).
fn plain_sum(xs: &[f64]) -> Result<f64, ErrorCode> {
    let mut t = 0.0;
    for &x in xs {
        t += x;
    }
    if t.is_finite() { Ok(t) } else { Err(ErrorCode::Num) }
}

/// `dbNumeric(name, agg)`: the numbers of the field in the matching records (an error stops), aggregated.
fn db_numeric(a: &CallArgs<'_>, ctx: &mut dyn FnContext, f: Agg) -> R<Value> {
    if let Some(v) = crit_array(a) {
        return Ok(v);
    }
    let vals = matched(a, ctx, true)?;
    let mut xs = Vec::with_capacity(vals.len());
    for v in vals.into_iter().flatten() {
        match v {
            Scalar::Err(e) => return fail(e),
            Scalar::Num(n) => xs.push(n),
            _ => {}
        }
    }
    Ok(num(f(&xs)?))
}

fn dsum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, plain_sum)
}

fn daverage(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, |xs| {
        if xs.is_empty() {
            return Err(ErrorCode::Div0);
        }
        Ok(plain_sum(xs)? / xs.len() as f64)
    })
}

fn dmax(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::max)
}

fn dmin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::min)
}

fn dproduct(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::product)
}

fn dstdev(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::stdev_s)
}

fn dstdevp(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::stdev_p)
}

fn dvar(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::var_s)
}

fn dvarp(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    db_numeric(a, ctx, agg::var_p)
}

/// DCOUNT/DCOUNTA: without a field, the matching records (measured); else the numbers (non-empty values).
fn dcount_with(a: &CallArgs<'_>, ctx: &mut dyn FnContext, counted: fn(Scalar) -> bool) -> R<Value> {
    if let Some(v) = crit_array(a) {
        return Ok(v);
    }
    let vals = matched(a, ctx, false)?;
    if matches!(scalar(a, 1), Some(Scalar::Empty)) {
        return Ok(num(vals.len() as f64));
    }
    // a value without a field (`true`) is no number and is not empty
    Ok(num(vals.iter().filter(|v| v.map_or(counted(Scalar::Bool(true)), counted)).count() as f64))
}

fn dcount(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    dcount_with(a, ctx, |v| matches!(v, Scalar::Num(_)))
}

fn dcounta(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    dcount_with(a, ctx, |v| !matches!(v, Scalar::Empty))
}

fn dget(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if let Some(v) = crit_array(a) {
        return Ok(v);
    }
    let vals = matched(a, ctx, true)?;
    if vals.is_empty() {
        return fail(ErrorCode::Value);
    }
    if vals.len() > 1 {
        return fail(ErrorCode::Num);
    }
    // measured: the found record with an empty field gives #VALUE!
    match vals[0] {
        None => Ok(Value::Scalar(Scalar::Bool(true))),
        Some(Scalar::Empty) => fail(ErrorCode::Value),
        Some(v) => Ok(Value::Scalar(v)),
    }
}
