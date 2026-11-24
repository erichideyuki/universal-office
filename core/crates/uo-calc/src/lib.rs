//! Sheet cell store and calculation engine of the UniversalOffice Rust core (plan
//! `core/docs/slides-sheet-port-plan.md` §4.0 and track A §5). Record: `core/docs/modules/uo-calc.md`.
//!
//! **Architecture skeleton** (2026-11-24): the types that make the later frontiers closed scope. No logic is
//! ported yet; operations that exist only as signatures return [`CalcError::NotPorted`].
//!
//! One cell store ([`store::Store`]) holds every cell of every sheet ONCE: its file representation (what the
//! surgical save writes back) and its calc value are the same lanes. The engine ([`engine::Engine`]) owns only
//! what is calc-specific (parsed formulas, flags, the dependency index) and reads and writes the store through
//! `&Store`/`&mut Store` — there is no bridge copy (the TS keeps three copies: the sheet-core model, the
//! component's cell index and the engine's `cells` map, `sheet-calc.ts` copying between them).
//!
//! Layers (bottom up): [`key`] (cell keys, areas) → [`strings`] (JS-exact compact strings, the interned pool)
//! → [`value`] (scalars, arrays, references) → [`store`] (the column-chunked cell store) → [`formula`] (the
//! file side of formulas) → [`engine`] (graph, recalc). [`locale`] is the C1 host hook.

pub mod engine;
pub mod error;
pub mod formula;
pub mod key;
pub mod locale;
pub mod store;
pub mod strings;
pub mod value;

pub use error::{CalcError, Result};
pub use key::{Area, CellKey, EXT_STRIDE, EXTERNAL_BASE, MAX_COL, MAX_ROW, SheetIx};
pub use locale::{Clock, DateOrder, FormatLetters, HostHooks, R1c1Letters, RandomSource, ValueLocale};
pub use store::Store;
pub use strings::{JsStr, JsString, StrId, StrPool};
pub use value::{ErrorCode, Scalar, StrRef};

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
