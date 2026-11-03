//! Stage-4 parity of uo-fonts at the corpus level: the facets of one font (`facets`) and the font
//! corpus (`corpus`). See `tests/fonts_parity.rs` and `tests/parity_support/capture_fonts.mjs`.
#![allow(dead_code)]

pub mod corpus;
pub mod facets;
pub mod legacy;
pub mod legacy_corpus;
