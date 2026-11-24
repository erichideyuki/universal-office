//! Family `logical` (frontier A4f: logical.ts, info.ts, cube.ts): not ported yet. The frontier fills [`BODIES`] (contract in
//! [`super`]); it may turn this file into a directory `logical/mod.rs`.

use crate::fnreg::FnBody;

/// The bodies of the family: one list per TS file (`shared::bodies!["NAME" => function, …]`, see [`super`]).
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[];
