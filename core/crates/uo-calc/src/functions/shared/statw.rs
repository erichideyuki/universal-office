//! The context walkers of `stats.ts` (the per-value rules are [`crate::stats::Collect`]): `collectNumbers`,
//! `collectInto` (with `skipCell`), `scalarsOf`, `gridOf`, `zipCells`, `numericPairs`. Families A4b (statistics,
//! distributions, regression, ETS) and A4e (database) use them.

use crate::coerce::parse_number_text;
use crate::fnreg::{CallArgs, FnContext};
use crate::key::{Area, SheetIx};
use crate::stats::{BIG_AREA, Collect, DirectText, Take, area_size, zip_sparse};
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

use super::{R, Stop, arg, one, view};

/// `collectNumbers(args, ctx, o)`: the numbers of an aggregation's arguments in Excel's order (arguments left to
/// right, references row by row), or the first error.
pub fn collect_numbers(args: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect, out: &mut Vec<f64>) -> R<()> {
    for i in 0..args.args.len() {
        if let Some(a) = arg(args, i) {
            collect_into(out, a, ctx, o)?;
        } else {
            // a lazy argument never reaches an aggregation; `undefined` reads as a direct null (0)
            collect_into(out, &Value::Scalar(Scalar::Empty), ctx, o)?;
        }
    }
    Ok(())
}

/// `collectInto(out, a, ctx, o)` without `skipCell`.
pub fn collect_into(out: &mut Vec<f64>, a: &Value, ctx: &mut dyn FnContext, o: &Collect) -> R<()> {
    match a {
        Value::Ref(r) => {
            for &area in r.areas() {
                let mut e: Option<ErrorCode> = None;
                ctx.for_each_cell(area, &mut |v, _, _| {
                    if e.is_none() {
                        ref_cell(o, out, v, &mut e);
                    }
                })?;
                if let Some(e) = e {
                    return Err(Stop::Err(e));
                }
            }
            Ok(())
        }
        Value::Array(x) => {
            for &v in x.data.iter() {
                match v {
                    Scalar::Num(n) => out.push(n),
                    Scalar::Err(e) => {
                        if !o.skip_errors {
                            return Err(Stop::Err(e));
                        }
                    }
                    Scalar::Str(_) => {
                        if o.arr_text == Take::Count {
                            out.push(0.0);
                        }
                    }
                    Scalar::Bool(b) => {
                        if o.arr_bool == Take::Count {
                            out.push(if b { 1.0 } else { 0.0 });
                        }
                    }
                    Scalar::Empty => {}
                }
            }
            Ok(())
        }
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Value)),
        Value::Scalar(s) => collect_direct(out, *s, ctx, o),
    }
}

/// A cell of a reference (`collectInto`'s reference branch).
#[inline]
fn ref_cell(o: &Collect, out: &mut Vec<f64>, v: Scalar, e: &mut Option<ErrorCode>) {
    match v {
        Scalar::Num(n) => out.push(n),
        Scalar::Err(x) => {
            if !o.skip_errors {
                *e = Some(x);
            }
        }
        Scalar::Str(_) => {
            if o.ref_text == Take::Count {
                out.push(0.0);
            }
        }
        Scalar::Bool(b) => {
            if o.ref_bool == Take::Count {
                out.push(if b { 1.0 } else { 0.0 });
            }
        }
        Scalar::Empty => {}
    }
}

/// A direct scalar argument (`collectInto`'s last branch).
fn collect_direct(out: &mut Vec<f64>, s: Scalar, ctx: &dyn FnContext, o: &Collect) -> R<()> {
    match s {
        Scalar::Empty => out.push(0.0),
        Scalar::Num(n) => out.push(n),
        Scalar::Bool(b) => {
            if !o.direct_bool_skip {
                out.push(if b { 1.0 } else { 0.0 });
            }
        }
        Scalar::Err(e) => {
            if !o.skip_errors {
                return Err(Stop::Err(e));
            }
        }
        Scalar::Str(_) => {
            if o.direct_text == DirectText::Ignore {
                return Ok(());
            }
            let super::ScalarView::Str(t) = view(ctx, s) else { return Ok(()) };
            match parse_number_text(t, ctx.locale(), false, ctx.year()) {
                Some(n) => out.push(n),
                None if o.direct_text != DirectText::Skip => return Err(Stop::Err(ErrorCode::Value)),
                None => {}
            }
        }
    }
    Ok(())
}

/// `collectInto` with `skipCell` (SUBTOTAL/AGGREGATE: hidden rows, nested subtotals): `skip(ctx, sheet, row, col)`
/// is asked for each existing cell of a reference before its value counts. The cells of an area are read first
/// (the predicate needs the context, which the walk borrows).
pub fn collect_into_skipping(
    out: &mut Vec<f64>,
    a: &Value,
    ctx: &mut dyn FnContext,
    o: &Collect,
    skip: &mut dyn FnMut(&mut dyn FnContext, SheetIx, u32, u32) -> bool,
) -> R<()> {
    let Value::Ref(r) = a else { return collect_into(out, a, ctx, o) };
    let mut cells: Vec<(Scalar, u32, u32)> = Vec::new();
    for &area in r.areas() {
        cells.clear();
        ctx.for_each_cell(area, &mut |v, row, col| cells.push((v, row, col)))?;
        let mut e: Option<ErrorCode> = None;
        for &(v, row, col) in &cells {
            if e.is_some() {
                break;
            }
            if skip(ctx, area.sheet, row, col) {
                continue;
            }
            ref_cell(o, out, v, &mut e);
        }
        if let Some(e) = e {
            return Err(Stop::Err(e));
        }
    }
    Ok(())
}

/// `scalarsOf(a, ctx)` of `stats.ts`: every scalar of an argument in order (a reference → its areas' cells, empty
/// ones included; a LAMBDA → `[#VALUE!]`).
pub fn scalars_of(a: &Value, ctx: &mut dyn FnContext, out: &mut Vec<Scalar>) -> R<()> {
    match a {
        Value::Ref(r) => {
            for i in 0..r.areas().len() {
                out.extend_from_slice(&ctx.values(r, i)?.data);
            }
        }
        Value::Array(x) => out.extend_from_slice(&x.data),
        Value::Lambda(_) => out.push(Scalar::Err(ErrorCode::Value)),
        Value::Scalar(s) => out.push(*s),
    }
    Ok(())
}

/// `gridOf(v, ctx)`: the values of an argument as a matrix (a reference of one area, an array or a scalar).
pub fn grid_of(v: &Value, ctx: &mut dyn FnContext) -> R<ArrayValue> {
    match v {
        Value::Ref(r) => {
            if r.areas().len() != 1 {
                return Err(Stop::Err(ErrorCode::Value));
            }
            Ok(ctx.values(r, 0)?)
        }
        Value::Array(a) => Ok(a.clone()),
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Value)),
        Value::Scalar(s) => Ok(one(*s)),
    }
}

/// `zipCells(xv, yv, ctx, visit)`: the two arguments position by position (same number of cells, else `#N/A`); two
/// big references are walked over their existing cells only (positions empty on both sides do not appear).
/// `visit` returning `Err` stops.
pub fn zip_cells(xv: &Value, yv: &Value, ctx: &mut dyn FnContext, visit: &mut dyn FnMut(&dyn FnContext, Scalar, Scalar) -> R<()>) -> R<()> {
    if let (Value::Ref(rx), Value::Ref(ry)) = (xv, yv)
        && let (Some(ax), Some(ay)) = (rx.single(), ry.single())
        && area_size(&ax) > BIG_AREA
        && area_size(&ay) > BIG_AREA
    {
        if area_size(&ax) != area_size(&ay) {
            return Err(Stop::Err(ErrorCode::NA));
        }
        let mut xm = existing(ctx, ax)?;
        let mut ym = existing(ctx, ay)?;
        let c: &dyn FnContext = ctx;
        return zip_sparse(&mut xm, &mut ym, Scalar::Empty, |a, b| visit(c, a, b));
    }
    let gx = grid_of(xv, ctx)?;
    let gy = grid_of(yv, ctx)?;
    if gx.data.len() != gy.data.len() {
        return Err(Stop::Err(ErrorCode::NA));
    }
    let c: &dyn FnContext = ctx;
    for (a, b) in gx.data.iter().zip(gy.data.iter()) {
        visit(c, *a, *b)?;
    }
    Ok(())
}

/// The non-empty existing cells of an area as `(offset, value)`, row by row.
fn existing(ctx: &mut dyn FnContext, a: Area) -> R<Vec<(u64, Scalar)>> {
    let w = u64::from(a.c2 - a.c1) + 1;
    let mut v = Vec::new();
    ctx.for_each_cell(a, &mut |s, r, c| {
        if !matches!(s, Scalar::Empty) {
            v.push((u64::from(r - a.r1) * w + u64::from(c - a.c1), s));
        }
    })?;
    Ok(v)
}

/// `numericPairs(xv, yv, ctx)`: the numeric pairs (x, y) at the same position; different sizes → `#N/A`; an error
/// propagates.
pub fn numeric_pairs(xv: &Value, yv: &Value, ctx: &mut dyn FnContext, xs: &mut Vec<f64>, ys: &mut Vec<f64>) -> R<()> {
    zip_cells(xv, yv, ctx, &mut |_, a, b| {
        if let Scalar::Err(e) = a {
            return Err(Stop::Err(e));
        }
        if let Scalar::Err(e) = b {
            return Err(Stop::Err(e));
        }
        if let (Scalar::Num(x), Scalar::Num(y)) = (a, b) {
            xs.push(x);
            ys.push(y);
        }
        Ok(())
    })
}
