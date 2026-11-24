//! Track C editor session (feature `edit`, frontier C5). Architecture skeleton (2026-11-24); record
//! `core/docs/modules/uo-xlsx.md` "Track C architecture" (C5).
//!
//! Undo is the TS CHANGE SET, not a snapshot of the store (`universal-sheet.ts:1900-2040`): per touched cell
//! the edit text (formula in the UI language or the constant's edit text) + style + existence, restored by
//! re-entering the text through the same entry path; structural changes snapshot the sheet structure
//! (`StructSnap`, `universal-sheet.ts:4442-4454`). 100 entries (`UNDO_LIMIT`, `universal-sheet.ts:4431`) and a
//! byte cap (new).

/// One cell before/after an action (`CellSnap`, `universal-sheet.ts:4436`).
#[derive(Clone, Debug)]
pub struct CellSnap {
    /// 1-based column.
    pub col: u32,
    /// 1-based row.
    pub row: u32,
    /// Edit text (UTF-16).
    pub edit: Vec<u16>,
    /// `s` + 1 (0 = none).
    pub style: u32,
    /// The cell existed.
    pub exists: bool,
}

/// One undo step (`UndoEntry`, `universal-sheet.ts:4455-4460`); the structural part is boxed (rare).
#[derive(Clone, Debug)]
pub struct UndoEntry {
    /// Label key of the action.
    pub label: Box<str>,
    /// `(sheet, cells)` before.
    pub before: Vec<(u32, Vec<CellSnap>)>,
    /// `(sheet, cells)` after.
    pub after: Vec<(u32, Vec<CellSnap>)>,
    /// Bytes this entry holds (for the byte cap).
    pub bytes: usize,
}

/// Bounds of the history.
#[derive(Clone, Copy, Debug)]
pub struct HistoryLimits {
    /// Steps (100, the TS and Excel).
    pub max_steps: u32,
    /// Bytes of all entries (oldest dropped first).
    pub max_bytes: usize,
}
