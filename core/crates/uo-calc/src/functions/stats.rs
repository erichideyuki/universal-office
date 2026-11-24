//! Family `stats` (frontier A4b: statistical.ts, distributions.ts, regression.ts, ets.ts): not ported yet. The frontier fills [`BODIES`] (contract in
//! [`super`]); it may turn this file into a directory `stats/mod.rs`.

use crate::fnreg::FnBody;

/// The bodies of the family: one list per TS file (`shared::bodies!["NAME" => function, …]`, see [`super`]).
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[];
