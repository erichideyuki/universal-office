//! Family `lookup` (frontier A4e: lookup.ts, reference.ts, dynamic.ts, database.ts, conditional.ts): not ported yet. The frontier fills [`BODIES`] (contract in
//! [`super`]); it may turn this file into a directory `lookup/mod.rs`.

use crate::fnreg::FnBody;

/// The bodies of the family: one list per TS file (`shared::bodies!["NAME" => function, …]`, see [`super`]).
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[];
