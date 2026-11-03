//! `DocContext` (context.ts:100, frontier F1): the shared state of one pass — the flow state that
//! advances with the content (lists, SEQ, headings, notes), the style resolver, the revision
//! display and the warnings. New per pass and per balancing probe (`newCtx()`, engine.ts:214),
//! so plain data: `pure()` clones the flow state and restores it (LA4; the clone count is charged
//! to the work budget by the engine).
//!
//! Not here (K4, LQ19): the TS `faces` field (`FaceResolver`, new per pass) — its cache lives per
//! layout call on the engine's `LayoutRun` ([`crate::fonts::FaceResolverCache`]); `opts` is the
//! call's [`crate::LayoutOptions`]; `directRPr`/`directPPr` are Word semantics (F2).

use indexmap::{IndexMap, IndexSet};

use crate::model::NumKey;
use crate::word::{NumberingState, RevisionDisplay, StyleResolver};

/// `NoteNumbering` (context.ts:79): the numbering of one note kind in the current section.
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

/// `'footnote' | 'endnote'` of the note flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteKindFlow {
    Footnote,
    Endnote,
}

impl NoteKindFlow {
    /// `kind[0]`: the prefix of the `noteMarkOverride` keys (`f<id>`, `e<id>`).
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            NoteKindFlow::Footnote => 'f',
            NoteKindFlow::Endnote => 'e',
        }
    }
}

/// The section's numbering of both kinds (`noteNumbering`, context.ts:122).
#[derive(Clone, Debug, PartialEq)]
pub struct NoteNumberings {
    pub footnote: NoteNumbering,
    pub endnote: NoteNumbering,
}

impl Default for NoteNumberings {
    /// decimal from 1 and lower roman from 1, both continuous (context.ts:122–125)
    fn default() -> NoteNumberings {
        NoteNumberings {
            footnote: NoteNumbering { fmt: "decimal".into(), start: 1.0, restart: NoteRestart::Continuous },
            endnote: NoteNumbering { fmt: "lowerRoman".into(), start: 1.0, restart: NoteRestart::Continuous },
        }
    }
}

impl NoteNumberings {
    #[must_use]
    pub fn of(&self, kind: NoteKindFlow) -> &NoteNumbering {
        match kind {
            NoteKindFlow::Footnote => &self.footnote,
            NoteKindFlow::Endnote => &self.endnote,
        }
    }
}

/// `NoteFlow` (context.ts:86): marks of the notes already referenced and the counters. The note
/// ids are JS numbers (`Map<number, string>`: SameValueZero keys, so `NumKey`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteFlow {
    /// `marks.footnote`, `marks.endnote` (id → mark, first-reference order)
    pub footnote_marks: IndexMap<NumKey, Box<[u16]>>,
    pub endnote_marks: IndexMap<NumKey, Box<[u16]>>,
    /// `seq.footnote`, `seq.endnote`
    pub footnote_seq: f64,
    pub endnote_seq: f64,
    /// `endnoteOrder`
    pub endnote_order: Vec<NumKey>,
}

impl NoteFlow {
    fn marks(&self, kind: NoteKindFlow) -> &IndexMap<NumKey, Box<[u16]>> {
        match kind {
            NoteKindFlow::Footnote => &self.footnote_marks,
            NoteKindFlow::Endnote => &self.endnote_marks,
        }
    }

    fn seq_mut(&mut self, kind: NoteKindFlow) -> &mut f64 {
        match kind {
            NoteKindFlow::Footnote => &mut self.footnote_seq,
            NoteKindFlow::Endnote => &mut self.endnote_seq,
        }
    }
}

/// `FlowState` (context.ts:68): what `saveFlow`/`restoreFlow` copy.
#[derive(Clone, Debug, Default)]
pub struct FlowState {
    pub numbering: NumberingState,
    /// `seqCounters`
    pub seq: IndexMap<Box<str>, f64>,
    pub heads: Vec<f64>,
    pub seq_heads: IndexMap<Box<str>, f64>,
    pub notes: NoteFlow,
}

/// `DocContext`: one per pass. Field classes (the K18 table, record "Milestone 1 — F0"): `flow`
/// is **carried flow state**; `resolver` is a **cache** of pure functions; `rev` is **pass
/// input**; `warnings` is **output**; `note_numbering` is carried (set per section);
/// `note_mark_override` is pass input.
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
    pub note_numbering: NoteNumberings,
    /// pass input: `noteMarkOverride` (marks of the previous pass, `f<id>`/`e<id>` → mark)
    pub note_mark_override: Option<IndexMap<Box<str>, Box<[u16]>>>,
}

impl DocContext {
    /// `new DocContext(fmt, provider, opts)` (context.ts:109) with the Word-semantics parts built
    /// by the caller (F2: `new StyleResolver(fmt)`, `new NumberingState(fmt.numbering,
    /// fmt.styles.styles, lang)`, `new RevisionDisplay(opts.revisions, order)`). The face resolver
    /// is per layout call (K4); the unused `docDefault` of the TS `FaceResolver` (`void
    /// this.docDefault`) is not computed.
    #[must_use]
    pub fn new(resolver: StyleResolver, numbering: NumberingState, rev: RevisionDisplay) -> DocContext {
        DocContext {
            flow: FlowState { numbering, ..FlowState::default() },
            resolver,
            rev,
            warnings: IndexSet::new(),
            note_numbering: NoteNumberings::default(),
            note_mark_override: None,
        }
    }

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

    /// `pure(fn)`: measuring without side effects — no list, SEQ or note number advances. The flow
    /// state is restored whatever `f` returns (the TS `finally`, so also on an `Err`). The engine,
    /// whose state holds this context, does the same with [`Self::save_flow`] and
    /// [`Self::restore_flow`] around its own call.
    pub fn pure<T>(&mut self, f: impl FnOnce(&mut DocContext) -> T) -> T {
        let s = self.save_flow();
        let out = f(self);
        // the snapshot is not reused: moved back instead of cloned (same state)
        self.flow = s;
        out
    }

    /// `beginNoteSection(n, first)` (context.ts:156): the section's note numbering, and the
    /// counters restart on the first section or with `eachSect`.
    pub fn begin_note_section(&mut self, n: NoteNumberings, first: bool) {
        self.note_numbering = n;
        for kind in [NoteKindFlow::Footnote, NoteKindFlow::Endnote] {
            if first || self.note_numbering.of(kind).restart == NoteRestart::EachSect {
                *self.flow.notes.seq_mut(kind) = 0.0;
            }
        }
    }

    /// `beginNotePage()` (context.ts:162): footnotes restarting "each page" count from 0.
    pub fn begin_note_page(&mut self) {
        if self.note_numbering.footnote.restart == NoteRestart::EachPage {
            self.flow.notes.footnote_seq = 0.0;
        }
    }

    /// `noteMark(kind, id, custom)` (context.ts:168): the displayed mark of a note, numbered in
    /// first-reference order in its section's format; a custom mark takes no number. The number
    /// is formatted by `format_number(n, fmt, lang)` (`formatNumber` of numbering.ts, F2) unless
    /// the previous pass fixed the mark (`noteMarkOverride`).
    pub fn note_mark(
        &mut self,
        kind: NoteKindFlow,
        id: NumKey,
        custom: bool,
        lang: &str,
        format_number: impl FnOnce(f64, &str, &str) -> Box<[u16]>,
    ) -> Box<[u16]> {
        if let Some(m) = self.flow.notes.marks(kind).get(&id) {
            return m.clone();
        }
        if kind == NoteKindFlow::Endnote {
            self.flow.notes.endnote_order.push(id);
        }
        let m: Box<[u16]> = if custom {
            Box::default()
        } else {
            let seq = {
                let s = self.flow.notes.seq_mut(kind);
                *s += 1.0;
                *s
            };
            let over = self.note_mark_override.as_ref().and_then(|o| o.get(note_key(kind, id).as_str()).cloned());
            match over {
                Some(m) => m,
                None => {
                    let nn = self.note_numbering.of(kind);
                    format_number(nn.start - 1.0 + seq, &nn.fmt, lang)
                }
            }
        };
        match kind {
            NoteKindFlow::Footnote => self.flow.notes.footnote_marks.insert(id, m.clone()),
            NoteKindFlow::Endnote => self.flow.notes.endnote_marks.insert(id, m.clone()),
        };
        m
    }

    /// `noteMarkOf(kind, id)` (context.ts:185): the mark already given, without numbering.
    #[must_use]
    pub fn note_mark_of(&self, kind: NoteKindFlow, id: NumKey) -> Option<&[u16]> {
        self.flow.notes.marks(kind).get(&id).map(|m| &**m)
    }

    /// `allNoteMarks()` (context.ts:190): every mark given (`f<id>`, then `e<id>`), for NOTEREF in
    /// the next pass.
    #[must_use]
    pub fn all_note_marks(&self) -> IndexMap<Box<str>, Box<[u16]>> {
        let mut out = IndexMap::with_capacity(self.flow.notes.footnote_marks.len() + self.flow.notes.endnote_marks.len());
        for (id, m) in &self.flow.notes.footnote_marks {
            out.insert(note_key(NoteKindFlow::Footnote, *id).into_boxed_str(), m.clone());
        }
        for (id, m) in &self.flow.notes.endnote_marks {
            out.insert(note_key(NoteKindFlow::Endnote, *id).into_boxed_str(), m.clone());
        }
        out
    }

    /// `endnoteOrder` (context.ts:198): the endnotes referenced so far, in first-reference order.
    #[must_use]
    pub fn endnote_order(&self) -> &[NumKey] {
        &self.flow.notes.endnote_order
    }
}

/// `` `${kind[0]}${id}` `` (`String(id)` of the JS number).
#[must_use]
pub fn note_key(kind: NoteKindFlow, id: NumKey) -> String {
    let mut s = String::with_capacity(8);
    s.push(kind.letter());
    s.push_str(&uo_jsnum::format::to_js_string(id.get()));
    s
}
