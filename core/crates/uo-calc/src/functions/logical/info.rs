//! `info.ts`: ISBLANK, ISERR, ISERROR, ISNA, ISTEXT, ISNONTEXT, ISNUMBER, ISLOGICAL, ISEVEN, ISODD, N, T, NA, TYPE,
//! ERROR.TYPE. (ISREF, ISFORMULA, CELL, INFO, SHEET, SHEETS and FORMULATEXT are `reference.ts`: family `lookup`.)

use crate::apply::implicit_intersection;
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, Value, error_type};

use super::super::shared::{R, Stop, arg, bodies, boolean, err, num, scalar, to_num_d};

pub(super) const INFO: &[(&str, FnBody)] = bodies![
    "ISBLANK" => isblank,
    "ISERR" => iserr,
    "ISERROR" => iserror,
    "ISNA" => isna,
    "ISTEXT" => istext,
    "ISNONTEXT" => isnontext,
    "ISNUMBER" => isnumber,
    "ISLOGICAL" => islogical,
    "ISEVEN" => iseven,
    "ISODD" => isodd,
    "N" => n,
    "T" => t,
    "NA" => na,
    "TYPE" => type_,
    "ERROR.TYPE" => error_type_,
];

/// The scalar of an `is(…)` body (`value` mode, lifted, errors accepted).
fn s0(a: &CallArgs<'_>) -> Scalar {
    scalar(a, 0).unwrap_or(Scalar::Empty)
}

fn isblank(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Empty)))
}

fn iserr(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Err(e) if e != ErrorCode::NA)))
}

fn iserror(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Err(_))))
}

fn isna(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Err(ErrorCode::NA))))
}

fn istext(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Str(_))))
}

fn isnontext(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(!matches!(s0(a), Scalar::Str(_))))
}

fn isnumber(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Num(_))))
}

fn islogical(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(matches!(s0(a), Scalar::Bool(_))))
}

/// An ISEVEN/ISODD argument as `liftAtp` gives it to the body: `None` = `OMITTED` (the argument is `null`).
type Atp = Option<Scalar>;

/// `parity(v, ctx)`: truncated, booleans → `#VALUE!`. `OMITTED` reaches `toNumber` as a `Symbol`, whose conversion
/// to a string throws the TS `TypeError`.
fn parity(ctx: &dyn FnContext, v: Atp) -> R<f64> {
    let Some(v) = v else { return Err(Stop::Throw(Throw::Internal("TypeError: Cannot convert a Symbol value to a string"))) };
    if let Scalar::Bool(_) = v {
        return Err(ErrorCode::Value.into());
    }
    let n = to_num_d(ctx, v)?;
    Ok(n.trunc().abs() % 2.0)
}

/// `liftAtp([v], ctx, f)` of one argument, without the argument list (no allocation on the scalar path): a reference
/// of several cells → `#VALUE!`; a LAMBDA → `#CALC!`; an array → element by element (at least 1 × 1, leftovers
/// `#N/A`).
fn lift1(a: &CallArgs<'_>, ctx: &mut dyn FnContext, f: fn(&dyn FnContext, Atp) -> R<Value>) -> R<Value> {
    let v = match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Scalar(Scalar::Empty) => return f(ctx, None),
        Value::Ref(r) => match r.single() {
            Some(x) if x.r1 == x.r2 && x.c1 == x.c2 => ctx.cell(x.sheet, x.r1, x.c1)?,
            _ => return Err(ErrorCode::Value.into()),
        },
        Value::Lambda(_) => Scalar::Err(ErrorCode::Calc),
        Value::Scalar(s) => *s,
        Value::Array(x) => {
            let (rows, cols) = (x.rows.max(1), x.cols.max(1));
            let out = ArrayValue::try_fill(rows, cols, |r, c| -> Result<Scalar, Throw> {
                let y = crate::value::broadcast_at(x, r, c);
                // an Excel error of the body is the element's value
                match f(ctx, Some(y)) {
                    Ok(v) => Ok(super::super::shared::top(&v)),
                    Err(Stop::Err(e)) => Ok(Scalar::Err(e)),
                    Err(Stop::Throw(t)) => Err(t),
                }
            })?;
            return Ok(Value::Array(out));
        }
    };
    f(ctx, Some(v))
}

fn iseven(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift1(a, ctx, |ctx, v| Ok(boolean(parity(ctx, v)? == 0.0)))
}

fn isodd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift1(a, ctx, |ctx, v| Ok(boolean(parity(ctx, v)? == 1.0)))
}

/// `firstOrEach(v, ctx, f)`: a reference → its first cell (of the first area); an array → element by element; a
/// LAMBDA → `#CALC!`.
fn first_or_each(a: &CallArgs<'_>, ctx: &mut dyn FnContext, f: impl Fn(Scalar) -> Scalar) -> R<Value> {
    Ok(match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Ref(r) => Value::Scalar(f(first_cell(r, ctx)?)),
        Value::Array(x) => Value::Array(ArrayValue::fill(x.rows, x.cols, |r, c| f(x.get(r, c)))),
        Value::Lambda(_) => err(ErrorCode::Calc),
        Value::Scalar(s) => Value::Scalar(f(*s)),
    })
}

/// `firstCell(r, ctx)`.
fn first_cell(r: &RefValue, ctx: &mut dyn FnContext) -> Result<Scalar, Throw> {
    match r.areas().first() {
        Some(a) => ctx.cell(a.sheet, a.r1, a.c1),
        None => Ok(Scalar::Err(ErrorCode::Ref)),
    }
}

fn n(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    first_or_each(a, ctx, |x| match x {
        Scalar::Num(_) | Scalar::Err(_) => x,
        Scalar::Bool(true) => Scalar::Num(1.0),
        _ => Scalar::Num(0.0),
    })
}

fn t(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // `''` for everything but texts and errors (the empty text is made only when a result needs it)
    let keep = |x: &Scalar| matches!(x, Scalar::Str(_) | Scalar::Err(_));
    let x = match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Ref(r) => first_cell(r, ctx)?,
        Value::Array(x) => {
            let empty = if x.data.iter().all(keep) { Scalar::Empty } else { Scalar::Str(ctx.new_text(JsStr::EMPTY)?) };
            return Ok(Value::Array(ArrayValue::fill(x.rows, x.cols, |r, c| {
                let y = x.get(r, c);
                if keep(&y) { y } else { empty }
            })));
        }
        Value::Lambda(_) => return Ok(err(ErrorCode::Calc)),
        Value::Scalar(s) => *s,
    };
    Ok(Value::Scalar(if keep(&x) { x } else { Scalar::Str(ctx.new_text(JsStr::EMPTY)?) }))
}

fn na(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Err(ErrorCode::NA.into())
}

fn type_(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = match arg(a, 0).unwrap_or(&super::super::shared::EMPTY) {
        Value::Ref(r) => {
            let Some(s) = r.single() else { return Ok(num(16.0)) };
            if s.r1 == s.r2 && s.c1 == s.c2 {
                Value::Scalar(ctx.cell(s.sheet, s.r1, s.c1)?)
            } else if ctx.array_context() == Some(false) {
                Value::Scalar(implicit_intersection(r, ctx)?)
            } else {
                return Ok(num(64.0));
            }
        }
        v => v.clone(),
    };
    Ok(num(match x {
        Value::Array(_) => 64.0,
        Value::Lambda(_) => 128.0,
        Value::Scalar(Scalar::Err(_)) => 16.0,
        Value::Scalar(Scalar::Str(_)) => 2.0,
        Value::Scalar(Scalar::Bool(_)) => 4.0,
        _ => 1.0,
    }))
}

fn error_type_(a: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    match s0(a) {
        Scalar::Err(e) => Ok(num(f64::from(error_type(e)))),
        _ => Err(ErrorCode::NA.into()),
    }
}
