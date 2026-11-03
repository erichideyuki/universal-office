//! `DocContext` (context.ts:100, frontier F1): the shared state of one pass — the flow state that
//! advances with the content (lists, SEQ, headings, notes), the style resolver, the revision
//! display and the warnings. New per pass and per balancing probe (`newCtx()`, engine.ts:214),
//! so plain data: `pure()` clones the flow state and restores it (LA4; the clone count is charged
//! to the work budget).

use indexmap::{IndexMap, IndexSet};

use crate::word::{NumberingState, RevisionDisplay, StyleResolver};

/// `NoteNumbering` (context.ts:79).
#[derive(Clone, Debug, PartialEq)]
pub struct NoteNumbering {
    pub fmt: Box<str>,
    pub start: f64,
    pub restart: NoteRestart,
}

/// `'continuous' | 'eachSect' | 'eachPage'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteRestart {
    Continuous,
    EachSect,
    EachPage,
}

/// `NoteFlow` (context.ts:86): marks of the notes already referenced and the counters.
#[derive(Clone, Debug, Default)]
pub struct NoteFlow {
    /// `marks.footnote`, `marks.endnote` (id → mark)
    pub footnote_marks: IndexMap<i32, Box<[u16]>>,
    pub endnote_marks: IndexMap<i32, Box<[u16]>>,
    /// `seq`
    pub footnote_seq: f64,
    pub endnote_seq: f64,
    /// `endnoteOrder`
    pub endnote_order: Vec<i32>,
}

/// `FlowState` (context.ts:68): what `saveFlow`/`restoreFlow` copy.
#[derive(Clone, Debug, Default)]
pub struct FlowState {
    pub numbering: NumberingState,
    pub seq: IndexMap<Box<str>, f64>,
    pub heads: Vec<f64>,
    pub seq_heads: IndexMap<Box<str>, f64>,
    pub notes: NoteFlow,
}

/// `DocContext`: one per pass. Field classes (the K18 table, record "Milestone 1 — F0"): `flow`
/// is **carried flow state**; `resolver`, `rev` are **caches** of pure functions; `warnings` is
/// **output**; `note_numbering`, `note_mark_override` are **pass input** set per section/pass.
#[derive(Clone, Debug, Default)]
pub struct DocContext {
    /// carried flow state (`numbering`, `seqCounters`, `heads`, `seqHeads`, `notes`)
    pub flow: FlowState,
    /// cache: `resolver` (style cascade memo; per call, LQ18)
    pub resolver: StyleResolver,
    /// pass input: `rev`
    pub rev: RevisionDisplay,
    /// output: `warnings` (a `Set<string>`)
    pub warnings: IndexSet<Box<str>>,
    /// carried: `noteNumbering` of the current section
    pub note_numbering: Option<(NoteNumbering, NoteNumbering)>,
    /// pass input: `noteMarkOverride` (marks of the previous pass, `f<id>`/`e<id>` → mark)
    pub note_mark_override: Option<IndexMap<Box<str>, Box<[u16]>>>,
}

impl DocContext {
    /// `warn(msg)`.
    pub fn warn(&mut self, msg: &str) {
        if !self.warnings.contains(msg) {
            self.warnings.insert(msg.into());
        }
    }

    /// `saveFlow()`.
    #[must_use]
    pub fn save_flow(&self) -> FlowState {
        self.flow.clone()
    }

    /// `restoreFlow(s)` (the snapshot stays reusable).
    pub fn restore_flow(&mut self, s: &FlowState) {
        self.flow = s.clone();
    }
}
