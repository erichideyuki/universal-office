//! `functions/tdl-util.ts`: the support of the text, date, information, lookup and dynamic-array libraries
//! (families A4c, A4d, A4e, A4f) — `liftAtp`, its argument rules (`atpNum`, `num`, `txt`, `int`) and the value
//! walkers (`scalarsOf`, `existingScalars`, `asArray`).

use crate::coerce::{JsCow, to_text};
use crate::fnreg::{CallArgs, FnContext};
use crate::value::{ArrayValue, ErrorCode, Scalar, Value, broadcast_at};

use super::{R, Stop, arg, one, to_num_d, top, view};

/// An argument of [`lift_atp`] as the function body receives it (`AtpArg`, plus the positions passed through).
#[derive(Clone, Debug)]
pub enum AtpArg {
    /// `OMITTED`: the argument was empty in the formula (`=EDATE(A1;)`), ≠ an empty cell.
    Omitted,
    /// A scalar (an empty cell is `Scalar::Empty`; errors arrive as values: the body validates in order).
    S(Scalar),
    /// A position the caller passes through unlifted (`pass(i)`: WORKDAY's holidays…), as it came.
    Raw(Value),
}

impl AtpArg {
    /// The scalar (a passed-through value that is not a scalar reads as `#VALUE!`).
    #[must_use]
    pub fn scalar(&self) -> Option<Scalar> {
        match self {
            Self::Omitted => None,
            Self::S(s) => Some(*s),
            Self::Raw(Value::Scalar(s)) => Some(*s),
            Self::Raw(_) => Some(Scalar::Err(ErrorCode::Value)),
        }
    }
}

/// The body [`lift_atp`] calls per element.
pub type AtpBody<'a> = dyn FnMut(&[AtpArg], &mut dyn FnContext) -> R<Value> + 'a;

/// `liftAtp(args, ctx, f, pass)` (ISEVEN, EDATE, EOMONTH, WORKDAY, NETWORKDAYS, WEEKNUM, YEARFRAC…): a reference of
/// SEVERAL cells → `#VALUE!` (no implicit intersection, not even in a dynamic formula); an array → element by
/// element (broadcast 1×N with M×1; leftovers `#N/A`). The arguments reach `f` raw (errors included); an omitted one
/// as [`AtpArg::Omitted`], an empty cell as `Scalar::Empty`; `pass(i)` positions arrive unlifted.
pub fn lift_atp(args: &CallArgs<'_>, ctx: &mut dyn FnContext, pass: &dyn Fn(usize) -> bool, f: &mut AtpBody<'_>) -> R<Value> {
    let n = args.args.len();
    let mut xs: Vec<AtpArg> = Vec::with_capacity(n);
    let mut arrs: Vec<(usize, ArrayValue)> = Vec::new();
    for i in 0..n {
        let v = arg(args, i).unwrap_or(&super::EMPTY);
        if pass(i) {
            xs.push(AtpArg::Raw(v.clone()));
            continue;
        }
        let v = match v {
            Value::Scalar(Scalar::Empty) => {
                xs.push(AtpArg::Omitted);
                continue;
            }
            Value::Ref(r) => match r.single() {
                Some(a) if a.r1 == a.r2 && a.c1 == a.c2 => Value::Scalar(ctx.cell(a.sheet, a.r1, a.c1)?),
                _ => return Err(Stop::Err(ErrorCode::Value)),
            },
            Value::Lambda(_) => Value::Scalar(Scalar::Err(ErrorCode::Calc)),
            v => v.clone(),
        };
        match v {
            Value::Array(a) => {
                arrs.push((i, a));
                xs.push(AtpArg::S(Scalar::Empty));
            }
            Value::Scalar(s) => xs.push(AtpArg::S(s)),
            _ => xs.push(AtpArg::S(Scalar::Err(ErrorCode::Value))),
        }
    }
    if arrs.is_empty() {
        return f(&xs, ctx);
    }
    let (mut rows, mut cols) = (1u32, 1u32);
    for (_, a) in &arrs {
        rows = rows.max(a.rows);
        cols = cols.max(a.cols);
    }
    if crate::value::array_too_big(rows, cols) {
        return Err(Stop::Throw(crate::fnreg::Throw::Range));
    }
    let mut ys = xs.clone();
    let out = ArrayValue::try_fill(rows, cols, |r, c| -> R<Scalar> {
        for (i, a) in &arrs {
            ys[*i] = AtpArg::S(broadcast_at(a, r, c));
        }
        Ok(top(&f(&ys, ctx)?))
    })?;
    Ok(Value::Array(out))
}

/// `atpNum(v, ctx, def)` of `tdl-util.ts`: missing or omitted → `def` (none → `#N/A`); boolean → `#VALUE!`; empty
/// = 0; text in the locale (with `ctx.date1904`).
pub fn atp_num(ctx: &dyn FnContext, v: Option<&AtpArg>, def: Option<f64>) -> Result<f64, ErrorCode> {
    let s = match v {
        None | Some(AtpArg::Omitted) => return def.ok_or(ErrorCode::NA),
        Some(a) => a.scalar().unwrap_or(Scalar::Empty),
    };
    if let Scalar::Bool(_) = s {
        return Err(ErrorCode::Value);
    }
    to_num_d(ctx, s)
}

/// `num(v, ctx)` of `tdl-util.ts` (`toNumber` with `ctx.date1904`).
#[inline]
pub fn num(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    to_num_d(ctx, v)
}

/// `txt(v, ctx)`: the text of a scalar in the locale (an error stays an error).
#[inline]
pub fn txt(ctx: &dyn FnContext, v: Scalar) -> Result<JsCow<'_>, ErrorCode> {
    to_text(view(ctx, v), ctx.locale())
}

/// `int(v, ctx)`: the truncated number.
#[inline]
pub fn int(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    num(ctx, v).map(uo_jsnum::math::trunc)
}

/// `scalarsOf(v, ctx)` of `tdl-util.ts`: every scalar of a value row by row (a LAMBDA → `#CALC!`; differs from the
/// `stats.ts` one, which gives `[#VALUE!]`).
pub fn scalars_of(v: &Value, ctx: &mut dyn FnContext, out: &mut Vec<Scalar>) -> R<()> {
    match v {
        Value::Ref(r) => {
            for k in 0..r.areas().len() {
                out.extend_from_slice(&ctx.values(r, k)?.data);
            }
        }
        Value::Array(a) => out.extend_from_slice(&a.data),
        Value::Lambda(_) => return Err(Stop::Err(ErrorCode::Calc)),
        Value::Scalar(s) => out.push(*s),
    }
    Ok(())
}

/// `SPARSE_CELLS`: larger areas are read over their existing cells only (whole columns).
pub const SPARSE_CELLS: u64 = 65_536;

/// `existingScalars(v, ctx)` (CONCAT, TEXTJOIN): the NON-empty scalars of a value in order (row by row), and how
/// many positions there were in all — without materializing whole columns. Empties of arrays are skipped too.
pub fn existing_scalars(v: &Value, ctx: &mut dyn FnContext, out: &mut Vec<Scalar>) -> R<f64> {
    match v {
        Value::Ref(r) => {
            let mut total = 0.0;
            for &a in r.areas() {
                total += f64::from(a.r2 - a.r1 + 1) * f64::from(a.c2 - a.c1 + 1);
                ctx.for_each_cell(a, &mut |x, _, _| {
                    if !matches!(x, Scalar::Empty) {
                        out.push(x);
                    }
                })?;
            }
            Ok(total)
        }
        Value::Array(a) => {
            out.extend(a.data.iter().copied().filter(|x| !matches!(x, Scalar::Empty)));
            Ok(a.data.len() as f64)
        }
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Calc)),
        Value::Scalar(s) => {
            if !matches!(s, Scalar::Empty) {
                out.push(*s);
            }
            Ok(1.0)
        }
    }
}

/// `asArray(v, ctx)`: a value as a matrix (a reference of ONE area → its values; a scalar → 1×1; several areas →
/// `#VALUE!`; a LAMBDA → `#CALC!`).
pub fn as_array(v: &Value, ctx: &mut dyn FnContext) -> R<ArrayValue> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::Ref(r) => match r.single() {
            Some(_) => Ok(ctx.values(r, 0)?),
            None => Err(Stop::Err(ErrorCode::Value)),
        },
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Calc)),
        Value::Scalar(s) => Ok(one(*s)),
    }
}
