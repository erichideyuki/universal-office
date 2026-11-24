//! Sheet cell store and calculation engine of the UniversalOffice Rust core (plan
//! `core/docs/slides-sheet-port-plan.md` §4.0 and track A §5). Record: `core/docs/modules/uo-calc.md`.
//!
//! **Frontier S0 done** (2026-11-24): the store ([`store`], [`cell`], [`load`], [`formula`], [`strings`]).
//! **Frontier A1 done**: values and coercion ([`value`], [`coerce`]), the text order ([`collate`]), criteria
//! ([`criteria`]), dates ([`dates`]), the statistics base ([`stats`]), special functions ([`special`]), the
//! double-double libm ([`libm`]) and the math support ([`math_util`]).
//! **Frontier A2 done**: addresses ([`a1`]), the formula tree arena ([`ast`]), the parser ([`parse`], explicit
//! stack) and printer ([`print`]), the function registry contract ([`fnreg`], table generated from the TS), the
//! operators ([`ops`]), argument application ([`apply`]) and the evaluator ([`eval`], `trait EvalHost`). The
//! function bodies ([`functions`]) are A4; the engine is still the architecture skeleton (A3), its operations
//! return [`CalcError::NotPorted`].
//!
//! One cell store ([`store::Store`]) holds every cell of every sheet ONCE: its file representation (what the
//! surgical save writes back) and its calc value are the same lanes. The engine ([`engine::Engine`]) owns only
//! what is calc-specific (parsed formulas, flags, the dependency index) and reads and writes the store through
//! `&Store`/`&mut Store` — there is no bridge copy (the TS keeps three copies: the sheet-core model, the
//! component's cell index and the engine's `cells` map, `sheet-calc.ts` copying between them).
//!
//! Layers (bottom up): [`key`] (cell keys, areas) → [`strings`] (JS-exact compact strings, the interned pool)
//! → [`value`] (scalars, arrays, references) → [`store`] (the column-chunked cell store; [`cell`]: the file
//! model ↔ lanes and `cellValue`; [`load`]: file-order streaming) → [`formula`] (the file side of formulas) →
//! [`engine`] (graph, recalc). [`locale`] is the C1 host hook. Feature `facet`: [`facet`], the parity facet.
#![forbid(unsafe_code)]

pub mod a1;
pub mod apply;
pub mod ast;
mod cast;
pub mod cell;
pub mod coerce;
pub mod collate;
pub mod criteria;
pub mod dates;
pub mod engine;
pub mod error;
pub mod eval;
#[cfg(feature = "facet")]
pub mod facet;
mod fn_table;
pub mod fnreg;
pub mod formula;
pub mod functions;
mod jstext;
pub mod key;
pub mod libm;
pub mod load;
pub mod locale;
pub mod math_util;
pub mod ops;
pub mod parse;
pub mod print;
pub mod special;
pub mod stats;
pub mod store;
pub mod strings;
pub mod value;

pub use cell::{CellWrite, FileCell, WriteKind};
pub use error::{CalcError, Result};
pub use key::{Area, CellKey, EXT_STRIDE, EXTERNAL_BASE, MAX_COL, MAX_ROW, SheetIx};
pub use locale::{Clock, DateOrder, FormatLetters, HostHooks, R1c1Letters, RandomSource, ValueLocale};
pub use store::{RawCell, Store};
pub use strings::{JsStr, JsString, StrId, StrPool};
pub use value::{ErrorCode, Scalar, ScalarView, StrRef};

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    /// The sizes the record's bytes/cell budget is computed with (`uo-calc.md` "S2").
    #[test]
    fn layout_sizes() {
        assert_eq!(size_of::<crate::store::CellKind>(), 1);
        assert_eq!(size_of::<Option<crate::StrId>>(), 4);
        assert_eq!(size_of::<crate::Scalar>(), 16);
        assert_eq!(size_of::<crate::CellKey>(), 8);
        assert_eq!(size_of::<crate::strings::StrEntry>(), 12);
    }
}
