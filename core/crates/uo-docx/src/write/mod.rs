//! The writer (D1b). Its first piece is the font-embedding step over a saved package
//! ([`fonts`], from uo-fonts G2): the lossless document writer (`serialize.ts`, `lossless.ts`…)
//! joins this module in milestone 2.

pub mod fonts;
