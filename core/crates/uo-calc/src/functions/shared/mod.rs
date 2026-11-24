//! The shared function layer (frontier A4a): what every function family uses to read its arguments, walk ranges
//! and arrays, lift scalar bodies over arrays and return values. One file per TS support module:
//!
//! - this file: argument access, the result type [`R`]/[`Stop`] (an Excel error or a host throw), outputs, the
//!   scalar view of a value;
//! - [`mathu`]: the context walkers and `FnDef` factories of `functions/math-util.ts` (`eachNumber`, `asMatrix`,
//!   `atpRaw`, `atpList`, `liftScalars`, `numFn`, `atpFn`), on top of the pure rules of [`crate::math_util`];
//! - [`statw`]: the context walkers of `stats.ts` (`collectNumbers`/`collectInto` with `skipCell`, `scalarsOf`,
//!   `gridOf`, `zipCells`, `numericPairs`), on top of [`crate::stats`];
//! - [`tdl`]: `functions/tdl-util.ts` (`liftAtp`, its `atpNum`/`num`/`txt`/`int`, `scalarsOf`, `existingScalars`,
//!   `asArray`) — shared by the date, text, lookup, dynamic and information families;
//! - [`agg`]: the aggregation shapes of `functions/statistical.ts` that `database.ts` also uses (`aggMax`…).
//!
//! The family contract is in [`super`] (`functions/mod.rs`).

pub mod agg;
pub mod mathu;
pub mod statw;
pub mod tdl;

use crate::coerce::to_number;
use crate::fnreg::{Arg, CallArgs, EvalResult, FnContext, Throw};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, ScalarView, Value};

/// `null` as a value (the empty argument; `args[i] ?? null`).
pub static EMPTY: Value = Value::Scalar(Scalar::Empty);

/// Why a body stops early: an Excel error (a value: `Ok(#ERR)` for the evaluator) or a host throw (`NeedCell`, a
/// `RangeError`…: `Err` for the evaluator).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// An Excel error value.
    Err(ErrorCode),
    /// A JS exception of the TS (or the engine's `NeedCell`).
    Throw(Throw),
}

impl From<ErrorCode> for Stop {
    fn from(e: ErrorCode) -> Self {
        Self::Err(e)
    }
}

impl From<Throw> for Stop {
    fn from(t: Throw) -> Self {
        Self::Throw(t)
    }
}

/// The result of a body written with `?` over both Excel errors and host throws.
pub type R<T> = Result<T, Stop>;

/// [`R<Value>`] → the evaluator's [`EvalResult`] (an Excel error becomes its value).
#[inline]
pub fn done(r: R<Value>) -> EvalResult {
    match r {
        Ok(v) => Ok(v),
        Err(Stop::Err(e)) => Ok(Value::Scalar(Scalar::Err(e))),
        Err(Stop::Throw(t)) => Err(t),
    }
}

/// `R` of a host call that returns an [`EvalResult`] (`callLambda`, `force`).
#[inline]
pub fn lift_eval(r: EvalResult) -> R<Value> {
    r.map_err(Stop::Throw)
}

/// A number value.
#[inline]
#[must_use]
pub fn num(x: f64) -> Value {
    Value::Scalar(Scalar::Num(x))
}

/// An error value.
#[inline]
#[must_use]
pub fn err(e: ErrorCode) -> Value {
    Value::Scalar(Scalar::Err(e))
}

/// A boolean value.
#[inline]
#[must_use]
pub fn boolean(b: bool) -> Value {
    Value::Scalar(Scalar::Bool(b))
}

/// A `Num` (`number | CalcError`) as a value, as is (no `fin`).
#[inline]
#[must_use]
pub fn num_or_err(r: Result<f64, ErrorCode>) -> Value {
    match r {
        Ok(x) => num(x),
        Err(e) => err(e),
    }
}

/// `typeof r === 'number' ? fin(r) : r`: the numeric result of a `numFn`/`atpFn` body.
#[inline]
#[must_use]
pub fn fin_value(v: Value) -> Value {
    match v {
        Value::Scalar(Scalar::Num(x)) => num_or_err(crate::math_util::fin(x)),
        v => v,
    }
}

/// A text value (a temporary string of the pass).
pub fn text(ctx: &mut dyn FnContext, s: JsStr<'_>) -> R<Value> {
    Ok(Value::Scalar(Scalar::Str(ctx.new_text(s)?)))
}

/// A text value built from ASCII bytes.
pub fn ascii(ctx: &mut dyn FnContext, s: &[u8]) -> R<Value> {
    text(ctx, JsStr::Latin1(s))
}

/// The argument at `i` (`args[i]`): `None` past the end (`undefined`, not `null`). A lazy argument is forced only
/// by [`FnContext::force`]; here it reads as absent.
#[inline]
#[must_use]
pub fn arg<'a>(a: &'a CallArgs<'_>, i: usize) -> Option<&'a Value> {
    match a.args.get(i) {
        Some(Arg::Val(v)) => Some(v),
        _ => None,
    }
}

/// The scalar at `i` of a `value`-mode argument (`None` past the end; any non-scalar reads as its top-left, which
/// `applyFn` already did for `value` arguments).
#[inline]
#[must_use]
pub fn scalar(a: &CallArgs<'_>, i: usize) -> Option<Scalar> {
    match arg(a, i)? {
        Value::Scalar(s) => Some(*s),
        Value::Array(x) => Some(if x.rows > 0 && x.cols > 0 { x.get(0, 0) } else { Scalar::Err(ErrorCode::Value) }),
        Value::Ref(_) | Value::Lambda(_) => Some(Scalar::Err(ErrorCode::Value)),
    }
}

/// Number of arguments (`args.length`).
#[inline]
#[must_use]
pub fn len(a: &CallArgs<'_>) -> usize {
    a.args.len()
}

/// The view of a scalar (its text resolved in the context).
#[inline]
#[must_use]
pub fn view(ctx: &dyn FnContext, s: Scalar) -> ScalarView<'_> {
    match s {
        Scalar::Empty => ScalarView::Empty,
        Scalar::Num(n) => ScalarView::Num(n),
        Scalar::Bool(b) => ScalarView::Bool(b),
        Scalar::Err(e) => ScalarView::Err(e),
        Scalar::Str(r) => ScalarView::Str(ctx.text(r)),
    }
}

/// `toNumber(v, ctx.locale)` (`date1904 = false`, the `num` of `math-util.ts`).
#[inline]
pub fn to_num(ctx: &dyn FnContext, s: Scalar) -> Result<f64, ErrorCode> {
    match s {
        Scalar::Num(n) => Ok(n),
        Scalar::Empty => Ok(0.0),
        Scalar::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        Scalar::Err(e) => Err(e),
        Scalar::Str(_) => to_number(view(ctx, s), ctx.locale(), false, ctx.year()),
    }
}

/// `toNumber(v, ctx.locale, ctx.date1904)` (the `num` of `tdl-util.ts`).
#[inline]
pub fn to_num_d(ctx: &dyn FnContext, s: Scalar) -> Result<f64, ErrorCode> {
    match s {
        Scalar::Str(_) => to_number(view(ctx, s), ctx.locale(), ctx.date1904(), ctx.year()),
        s => to_num(ctx, s),
    }
}

/// `new ArrayValue(1, 1, [v])`.
#[inline]
#[must_use]
pub fn one(v: Scalar) -> ArrayValue {
    ArrayValue::new(1, 1, vec![v])
}

/// The top-left scalar of a function result inside a lifted fill (`ArrayValue` → `get(0, 0)`; reference or LAMBDA
/// → `#VALUE!`).
#[inline]
#[must_use]
pub fn top(v: &Value) -> Scalar {
    match v {
        Value::Scalar(s) => *s,
        Value::Array(a) => a.get(0, 0),
        Value::Ref(_) | Value::Lambda(_) => Scalar::Err(ErrorCode::Value),
    }
}

/// `true` when the number is an integer (`Number.isInteger`).
#[inline]
#[must_use]
pub fn is_int(x: f64) -> bool {
    uo_jsnum::conv::is_integer(x)
}

/// Declares the bodies of a family: `bodies![ "NAME" => function, … ]` where each `function` is
/// `fn(&CallArgs<'_>, &mut dyn FnContext) -> R<Value>`; the macro wraps each in an [`crate::fnreg::FnBody`] (the
/// `?`-friendly [`R`] becomes the evaluator's result).
macro_rules! bodies {
    ($($name:literal => $f:path),* $(,)?) => {
        &[$(($name, {
            fn w(a: &$crate::fnreg::CallArgs<'_>, c: &mut dyn $crate::fnreg::FnContext) -> $crate::fnreg::EvalResult {
                $crate::functions::shared::done($f(a, c))
            }
            w as $crate::fnreg::FnBody
        })),*]
    };
}
pub(crate) use bodies;
