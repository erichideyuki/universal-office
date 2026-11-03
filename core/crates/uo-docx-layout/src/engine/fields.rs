//! Dynamic fields (frontier F7c): SEQ (`seqValue`, engine.ts:423), REF, NOTEREF, STYLEREF,
//! PAGEREF, SECTIONPAGES, NUMPAGES (`numPagesShown`), chapter numbers, and the answer of
//! `ParaEnv.field` per `FieldSource` (the TS closure, LA3). Field instructions are matched by the
//! uo-docx `word` matchers (LQ3, G9); field results are capped by `max_field_text`.

/// A chapter heading on a page (`ChapterIndex` value entry).
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterMark {
    pub page: f64,
    pub text: Box<[u16]>,
}
