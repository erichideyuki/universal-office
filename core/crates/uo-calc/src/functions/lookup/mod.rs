//! Family `lookup` (frontier A4e): the bodies of `lookup.ts` ([`search`]: VLOOKUP … FIELDVALUE), `reference.ts`
//! ([`reference`]: INDEX … SINGLE), `dynamic.ts` ([`dynamic`]: FILTER … PIVOTBY, RANDARRAY), `database.ts`
//! ([`database`]: the D-functions) and `conditional.ts` ([`conditional`]: COUNTIF … MINIFS). The lookup vector of
//! `lookup.ts` (the TS `Vec`, cached per pass in the engine's `cache`) is [`line`]; the engine holds its
//! [`PassCache`].
//!
//! Record: `core/docs/modules/uo-calc.md`, "A4e — as built".

use crate::fnreg::{FnBody, FnContext};
use crate::key::Area;
use crate::value::{ErrorCode, RefValue, Scalar, ScalarView, Value};

mod conditional;
mod database;
mod dynamic;
mod line;
mod reference;
mod search;

pub use line::PassCache;

/// The bodies of the family: one list per TS file.
pub(super) const BODIES: &[&[(&str, FnBody)]] =
    &[search::LOOKUP, reference::REFERENCE, dynamic::DYNAMIC, database::DATABASE, conditional::CONDITIONAL];

/// `Math.trunc`.
#[inline]
fn trunc(x: f64) -> f64 {
    uo_jsnum::math::trunc(x)
}

/// An Excel error as the body's early return.
#[inline]
fn fail<T>(e: ErrorCode) -> super::shared::R<T> {
    Err(super::shared::Stop::Err(e))
}

/// `new RefValue([area])`.
#[inline]
fn area_ref(a: Area) -> Value {
    Value::Ref(RefValue::One(a))
}

/// The JS `typeof` class of a scalar for the type tests of the family (`n`, `s`, `b`, `x` = empty or error).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    N,
    S,
    B,
    X,
}

#[inline]
fn kind(v: Scalar) -> Kind {
    match v {
        Scalar::Num(_) => Kind::N,
        Scalar::Str(_) => Kind::S,
        Scalar::Bool(_) => Kind::B,
        Scalar::Empty | Scalar::Err(_) => Kind::X,
    }
}

/// `typeof a === typeof b` comparison of two scalars of the same kind (`cmpSame` of `lookup.ts`, `sortCmp`'s
/// branches of `dynamic.ts`): numbers by `<`/`>` (NaN = 0), texts by the Excel collation, booleans `false < true`.
#[inline]
fn cmp_same(ctx: &dyn FnContext, a: Scalar, b: Scalar) -> i32 {
    match (a, b) {
        (Scalar::Num(x), Scalar::Num(y)) => {
            if x < y {
                -1
            } else if x > y {
                1
            } else {
                0
            }
        }
        (Scalar::Str(x), Scalar::Str(y)) => crate::collate::compare_text_js(ctx.text(x), ctx.text(y)),
        (Scalar::Bool(x), Scalar::Bool(y)) => {
            if x == y {
                0
            } else if x {
                1
            } else {
                -1
            }
        }
        _ => 0,
    }
}

/// The rank of the total order of XLOOKUP/XMATCH and SORT: number < text < boolean < error < empty.
#[inline]
fn xrank(v: Scalar) -> u8 {
    match v {
        Scalar::Num(_) => 0,
        Scalar::Str(_) => 1,
        Scalar::Bool(_) => 2,
        Scalar::Err(_) => 3,
        Scalar::Empty => 4,
    }
}

/// `cmpTotal` (`lookup.ts`) = `sortCmp` (`dynamic.ts`): the rank first, then the same-kind comparison (errors and
/// empties equal among themselves).
#[inline]
fn cmp_total(ctx: &dyn FnContext, a: Scalar, b: Scalar) -> i32 {
    let (ra, rb) = (xrank(a), xrank(b));
    if ra != rb {
        return if ra < rb { -1 } else { 1 };
    }
    if ra >= 3 { 0 } else { cmp_same(ctx, a, b) }
}

/// The view of a scalar.
#[inline]
fn view(ctx: &dyn FnContext, v: Scalar) -> ScalarView<'_> {
    super::shared::view(ctx, v)
}
