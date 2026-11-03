//! The Word semantics shared with the session (LQ3): `resolve.ts`, `paraformat.ts`,
//! `directRPr`/`directPPr`/`resolvePPr`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`,
//! `outline.ts`, `numpages.ts`, `revisions.ts` and the `documentHyphenationLanguages` walk live in
//! **`uo_docx::word`**, ported by frontier F2 (Opus). The engine reaches them through this facade
//! only.
//!
//! **F0 placeholders**: the types the engine's state holds, with the TS shapes the pass state
//! needs (`NumberingState` is cloned by `pure()`, `RevisionDisplay` is per pass). F2 replaces each
//! with a re-export of its `uo_docx::word` type.

use indexmap::{IndexMap, IndexSet};

/// One level's counter (`LevelCount`, numbering.ts:92).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelCount {
    pub v: f64,
    pub real: bool,
}

/// `NumberingState` (numbering.ts:94): list counters per abstract numbering, cloned by `pure()`.
#[derive(Clone, Debug, Default)]
pub struct NumberingState {
    /// counters per list key, per level (TS `Map` order)
    pub counters: IndexMap<Box<str>, Vec<Option<LevelCount>>>,
    /// the lists already started
    pub started: IndexSet<Box<str>>,
}

/// `NumberMarker` (numbering.ts:70): the marker text of a numbered paragraph.
#[derive(Clone, Debug, Default)]
pub struct NumberMarker {
    pub text: Box<[u16]>,
    /// `chapter` (PAGE with `w:chapStyle`)
    pub chapter: Option<Box<[u16]>>,
}

/// `StyleResolver` (resolve.ts:155): the per-pass style caches (K3: per call in Rust, LQ18).
#[derive(Clone, Debug, Default)]
pub struct StyleResolver {}

/// `RevisionDisplay` (revisions.ts:63): how revisions show in this pass.
#[derive(Clone, Debug, Default)]
pub struct RevisionDisplay {}
