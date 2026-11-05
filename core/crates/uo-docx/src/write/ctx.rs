//! The state of one save (G7, U17): what the TS keeps in module variables of `serialize.ts`
//! (`mediaOut`, `drawingId`, `active`, `shapeTheme`, `chainNext`, `vmlSpid`, `vmlOnly`,
//! `emittedShapes`), `rev-xml.ts` (`nextId`, `usedUtc`) and `source.ts` (`memo`) is a
//! [`Writer`] built by `write_document` for one call and dropped after it. What outlives a save
//! — the baseline signatures, the chosen numbering ids (`store.genNumIds`) and the counter of
//! `freshDrawingIds` — is the caller's [`SaveState`] (the session keeps one per document).
//!
//! Identity: the TS `WeakMap`/`WeakSet`/`Map` keyed by model objects are keyed here by
//! [`DrawingId`] (`chainNext`, `emittedShapes`) or by [`SourceKey`] (the lossless sets, already
//! numeric in the TS). The textbox groups are found by the textbox's `TxbxSrc`, a value that
//! survives undo (U4c: the TS lookup by object loses them after an undo — fixed by name).

use std::collections::{HashMap, HashSet};

use uo_opc::{Limits, Package};

use super::rev::RevState;
use super::sig::{Baseline, Pristine, SigMode};
use crate::ids::{DrawingId, SourceKey};
use crate::js::Str;
use crate::model::Document;
use crate::model::run::Run;
use crate::model::sources::SourceStore;

/// The numIds of the lists the editor creates (`GenNumIds`, lossless.ts:296): bullet, decimal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenNumIds {
    pub bullet: f64,
    pub decimal: f64,
}

/// `DEFAULT_NUM_IDS` (1001 / 1002).
pub const DEFAULT_NUM_IDS: GenNumIds = GenNumIds { bullet: 1001.0, decimal: 1002.0 };

/// What outlives one save of a document: the baseline (taken at install), the numbering ids the
/// first lossless save chose (`store.genNumIds ??= …`), and the `freshDrawingIds` counter (a
/// process-global `let` in the TS, never reset: per document here, G7 — declared).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SaveState {
    pub baseline: Baseline,
    pub gen_num_ids: Option<GenNumIds>,
    pub fresh_id: f64,
}

impl SaveState {
    /// The state right after an install: the baseline of `doc` in `mode`.
    #[must_use]
    pub fn install(doc: &Document, mode: SigMode) -> SaveState {
        SaveState { baseline: Baseline::take(doc, mode), gen_num_ids: None, fresh_id: 0.0 }
    }
}

/// A media part queued by the generator outside the lossless save (`MediaOut`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaOut {
    pub rid: String,
    pub path: String,
    pub data: std::sync::Arc<[u8]>,
    pub ext: String,
    pub rel_type: Option<String>,
    pub mime: Option<String>,
}

/// The theme colours of the save (`shapeTheme`: the defaults below with the document's theme
/// over them; per save — the TS keeps the last document's colours in the module, carried to the
/// next document without a theme: G7, declared).
pub const DEFAULT_SHAPE_THEME: &[(&str, &str)] = &[
    ("dk1", "000000"),
    ("lt1", "FFFFFF"),
    ("dk2", "0E2841"),
    ("lt2", "E8E8E8"),
    ("accent1", "156082"),
    ("accent2", "E97132"),
    ("accent3", "196B24"),
    ("accent4", "0F9ED5"),
    ("accent5", "A02B93"),
    ("accent6", "4EA72E"),
    ("hlink", "467886"),
    ("folHlink", "96607D"),
];

/// The module state of `serialize.ts` and `rev-xml.ts` for one save.
#[derive(Debug, Clone)]
pub struct WriteCtx {
    /// the package's limits (every raw parse of the writer is bounded by them, uo-opc S1)
    pub limits: Limits,
    /// the signature mode of the save (pristine and `sameJson`, G23)
    pub mode: SigMode,
    pub rev: RevState,
    pub media_out: Vec<MediaOut>,
    /// `drawingId` (`rIdWnImg${n}`)
    pub drawing_id: u32,
    /// `shapeTheme`, in JS object order
    pub shape_theme: indexmap::IndexMap<Str, Str>,
    /// `chainNext`: textbox → the `docPr` name of the next box of its chain (this part)
    pub chain_next: HashMap<DrawingId, Str>,
    /// `vmlSpid` (`o:spid` of the rebuilt shapes)
    pub vml_spid: u32,
    /// compatibility mode ≤ 12: rebuilt shapes in VML only
    pub vml_only: bool,
    /// `emittedShapes`: rebuilt objects already written in this save (a pasted copy gets new ids)
    pub emitted_shapes: HashSet<DrawingId>,
    /// how many rebuilt text boxes are being written, one inside the other (W2: `editedShapeXml`
    /// writes a box's content twice, so n levels are 2^n work; bounded, W-D15)
    pub shape_depth: u32,
}

impl WriteCtx {
    #[must_use]
    pub fn new(limits: Limits, mode: SigMode) -> WriteCtx {
        WriteCtx {
            limits,
            mode,
            rev: RevState::default(),
            media_out: Vec::new(),
            drawing_id: 0,
            shape_theme: DEFAULT_SHAPE_THEME.iter().map(|&(k, v)| (Str::from(k), Str::from(v))).collect(),
            chain_next: HashMap::new(),
            vml_spid: 4096,
            vml_only: false,
            emitted_shapes: HashSet::new(),
            shape_depth: 0,
        }
    }
}

/// The state of the lossless save (`active`, serialize.ts:48–68), present when the document was
/// read with sources.
pub struct Lossless<'a> {
    pub store: &'a SourceStore,
    pub pristine: Pristine<'a>,
    /// the `.rels` of the part being written (new hyperlinks and media)
    pub rels_path: String,
    /// block ids already written (the first copy of a split paragraph owns the source)
    pub claimed: HashSet<SourceKey>,
    /// some generated `w:numPr` points at the new lists
    pub gen_numbering: bool,
    pub num_ids: GenNumIds,
    /// raw segments already written (`"paragraph:segment"`)
    pub claimed_segs: HashSet<(SourceKey, usize)>,
    /// copies of each source paragraph still to write (the last carries the marks)
    pub remaining: HashMap<SourceKey, i64>,
    /// the runs of the paragraph being written (`paraRuns`)
    pub para_runs: Option<&'a [Run]>,
    /// source objects already written (a pasted copy goes out with new drawing ids)
    pub emitted_obj: HashSet<SourceKey>,
}

/// One save: the document (borrowed), the package (edited in place, as the TS saves into its
/// `DocxPackage`), the module state and the lossless state.
pub struct Writer<'a> {
    pub doc: &'a Document,
    pub pkg: &'a mut Package,
    pub cx: WriteCtx,
    pub fresh_id: &'a mut f64,
    pub lossless: Option<Lossless<'a>>,
}

impl<'a> Writer<'a> {
    /// `active !== null`.
    #[must_use]
    pub fn active(&self) -> bool {
        self.lossless.is_some()
    }

    /// The source store when the save is lossless.
    #[must_use]
    pub fn store(&self) -> Option<&'a SourceStore> {
        self.lossless.as_ref().map(|l| l.store)
    }

    /// The text of a store span.
    #[must_use]
    pub fn src_text(&self, t: &'a crate::model::sources::SrcText) -> &'a str {
        match self.store() {
            Some(s) => t.resolve(&s.parts),
            None => t.resolve(&[]),
        }
    }
}
