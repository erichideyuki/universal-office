//! Footnotes and endnotes (frontier F7c): note areas, separators, note columns, continuation of
//! split notes, the endnote flow (engine.ts:2214–2890).

use crate::table::CellContent;

/// `NoteCut` (engine.ts:4145): a note split by the table row about to be emitted.
#[derive(Clone, Debug)]
pub struct NoteCut {
    pub id: i32,
    pub first: CellContent,
    pub rest: CellContent,
}

/// `enFlow` (engine.ts:2442): the endnote flow in progress.
#[derive(Clone, Copy, Debug)]
pub struct EnFlow {
    /// the separator is out: following pages and columns get the continuation separator
    pub sep_done: bool,
}

/// `sepCache` (engine.ts:2301): the separator line.
#[derive(Clone, Copy, Debug)]
pub struct SepMetrics {
    pub h: f64,
    pub mid: f64,
    pub thick: f64,
}

/// A note continuing on the next page (`noteCarry` entry).
#[derive(Clone, Debug)]
pub struct NoteCarry {
    pub id: i32,
    pub content: CellContent,
}
