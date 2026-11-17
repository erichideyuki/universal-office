//! JavaScript text semantics over UTF-16 for the Write (and later Sheet and Slides) engines of the
//! Rust core, with Unicode tables **generated from the ICU of Node 24.15** (V8 13.6, Unicode 17.0:
//! `js/gen-tables.mjs`, committed output, no parsing at run time):
//!
//! - [`nfc`]: `normalize('NFC')` (and `'NFD'`; `'NFKD'` with the `collation` feature), lone surrogates
//!   passing through.
//! - [`props`]: `\p{L}`, `\p{M}`, `\p{N}`, `\p{P}` as `RegExp` with the `u` flag matches them.
//! - [`case`]: `toUpperCase` / `toLowerCase` with SpecialCasing and Final_Sigma (root locale, as V8), and the
//!   `Canonicalize` of `/i` without `u`.
//! - [`space`]: the JS `\s` set over code units: `trim`, `split(/\s+/)`, `replace(/\s+/g, ...)`.
//! - [`collation`] (feature `collation`): the Windows word-sort order that Excel uses and Write's table sort
//!   borrows from Sheet, ported 1:1 from `sheet/packages/calc/src/collation.ts`.
//! - [`grapheme`]: extended grapheme clusters as `Intl.Segmenter('grapheme')` splits them (tables probed from the
//!   segmenter by `js/gen-grapheme.mjs`).
//! - [`utf16`]: code points over `&[u16]` with the JS view of lone surrogates.
//!
//! Everything works on `&[u16]` because a JS string can hold lone surrogates and a Rust `&str` cannot
//! (`_str` variants exist for valid text). Nothing here keeps state, caches, or panics. The record is
//! `core/docs/modules/uo-text.md`.
#![forbid(unsafe_code)]

pub mod case;
pub mod grapheme;
#[rustfmt::skip]
mod grapheme_tables;
#[cfg(feature = "collation")]
pub mod collation;
pub mod nfc;
pub mod props;
pub mod space;
#[rustfmt::skip]
mod tables;
#[cfg(feature = "collation")]
#[rustfmt::skip]
mod tables_compat;
pub mod utf16;

pub use tables::{ICU_VERSION, UNICODE_VERSION};

/// Nothing in this crate caches across calls; kept for the memory-gate convention (M1).
pub fn clear_caches() {}

/// The sizes in bytes of the generated tables, for the record (what ends up in `.rodata`; the linker
/// drops the tables a build does not reach).
#[must_use]
pub fn table_sizes() -> Vec<(&'static str, usize)> {
    use std::mem::size_of_val;
    let sizes = vec![
        ("CCC", size_of_val(&tables::CCC)),
        (
            "NFD",
            size_of_val(&tables::NFD_KEYS_BMP)
                + size_of_val(&tables::NFD_KEYS_ASTRAL)
                + size_of_val(&tables::NFD_ENDS)
                + size_of_val(&tables::NFD_POOL),
        ),
        ("COMP", size_of_val(&tables::COMP_BMP) + size_of_val(&tables::COMP_ASTRAL)),
        ("PROPS", size_of_val(&tables::PROPS)),
        ("SIGMA", size_of_val(&tables::SIGMA)),
        ("UPPER", size_of_val(&tables::UPPER_RUNS) + size_of_val(&tables::UPPER_SPECIAL) + size_of_val(&tables::UPPER_SPECIAL_POOL)),
        ("LOWER", size_of_val(&tables::LOWER_RUNS) + size_of_val(&tables::LOWER_SPECIAL) + size_of_val(&tables::LOWER_SPECIAL_POOL)),
        ("LOWER_LETTER", size_of_val(&tables::LOWER_LETTER)),
        ("GRAPHEME", size_of_val(&grapheme_tables::GCB) + size_of_val(&grapheme_tables::EXT_PICT) + size_of_val(&grapheme_tables::INCB)),
    ];
    #[cfg(feature = "collation")]
    let sizes = {
        let mut sizes = sizes;
        sizes.push((
            "COMPAT",
            size_of_val(&tables_compat::COMPAT_KEYS_BMP)
                + size_of_val(&tables_compat::COMPAT_KEYS_ASTRAL)
                + size_of_val(&tables_compat::COMPAT_ENDS)
                + size_of_val(&tables_compat::COMPAT_POOL),
        ));
        sizes
    };
    sizes
}
