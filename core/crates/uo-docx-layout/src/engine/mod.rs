//! The engine (`engine.ts`, frontiers F7a–c): the pass loop and the column-balancing search of
//! `layoutDocument`, and one pass, `class Engine`.
//!
//! **Shape (LA4, K18).** `layout_document` keeps the inter-pass inputs on a [`LayoutRun`]; every
//! pass is a fresh [`Engine`] — plain data, `Clone`, no borrows — and only its [`PassFindings`]
//! survive it. Engine methods take a [`Cx`] (the derived document, the pass input, the face table
//! and provider, the resources, the hyphenation sessions, the work budget, the options), so a
//! table layout can call back into the engine for its cells without aliasing an arena inside it.
//! The result is the **last executed** pass (K19). Recursion (cell → static flow → table → cell,
//! textboxes, notes) stays recursive, bounded by `LayoutLimits::max_depth` (LQ13).
//!
//! **Field classes (K18).** Every field of [`Engine`] is one of: **output** (pages and what they
//! hold), **carried** flow state (what a milestone-2 page checkpoint must hold and compare),
//! **cache** (a memo of a pure function of the document within the pass), **finding** (read by the
//! pass loop after the pass). The TS constructor parameters are the [`PassInput`]. The table with
//! the TS line of each field is in the record ("Milestone 1 — F0", "Engine field classification").

pub mod fields;
pub mod floats;
pub mod flow;
pub mod hf;
pub mod notes;
pub mod page;
pub mod tables;

use indexmap::{IndexMap, IndexSet};
use rustc_hash::FxHashMap;

use crate::context::DocContext;
use crate::error::{Frontier, LResult, unported};
use crate::fonts::{FaceResolverCache, FaceTable, FontProvider};
use crate::hyphenate::HyphenSessions;
use crate::keys::{ChainId, PageIx, RunKey, TableLayoutId, TbKey};
use crate::ldoc::LDoc;
use crate::model::CellKey;
use crate::options::{Budget, LayoutOptions};
use crate::output::Layout;
use crate::table::CellContent;
use crate::word::NumberMarker;
use uo_render::Resources;

use fields::ChapterMark;
use floats::Pinned;
use flow::{CellNeighbors, HiddenJoin, LastBox, PrevPara};
use notes::{EnFlow, NoteCarry, NoteCut, SepMetrics};
use page::{PageState, SectGeom};
use tables::{FloatCarry, FloatFlow, LastRowEmit};

/// `ChapterIndex` (engine.ts:177): heading level → the pages where each heading starts, in order.
pub type ChapterIndex = IndexMap<i64, Vec<ChapterMark>>;

/// `BalanceSpec` (engine.ts:296): a balanced section's last page and column height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BalanceSpec {
    pub page: u32,
    pub h: f64,
    pub last_free: bool,
}

/// `BalanceInfo` (engine.ts:298): what a section occupies on its last page (unbalanced pass).
#[derive(Clone, Debug, Default)]
pub struct BalanceInfo {
    pub page: u32,
    pub col_top: f64,
    pub bottom: f64,
    pub sum: f64,
    pub n: u32,
    pub used: Vec<f64>,
    pub content: Vec<f64>,
}

/// `LateTable` (engine.ts:301): a floating table placed at the top of its page in the next pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LateTable {
    pub page: u32,
    pub flow_y: f64,
}

/// The constructor parameters of `Engine` and the guesses `layoutDocument` sets on it: the input
/// of one pass, from the previous one (engine.ts:197–251, 385–396).
#[derive(Clone, Debug, Default)]
pub struct PassInput {
    /// `totalPagesGuess` (NUMPAGES)
    pub total_pages_guess: f64,
    /// `marksGuess`: the displayed page of each bookmark (PAGEREF)
    pub marks_guess: IndexMap<Box<str>, Box<[u16]>>,
    /// `lateFloats`: page index → floats placed at the page start
    pub late_floats: IndexMap<u32, Vec<RunKey>>,
    /// `lateTables`: block → page and reference y
    pub late_tables: IndexMap<u32, LateTable>,
    /// `balance`: balance key → spec
    pub balance: IndexMap<i64, BalanceSpec>,
    /// `noteRefGuess`: note marks of the previous pass (NOTEREF)
    pub note_ref_guess: IndexMap<Box<str>, Box<[u16]>>,
    /// `chaptersGuess`
    pub chapters_guess: ChapterIndex,
    /// `sectionPagesGuess` (SECTIONPAGES)
    pub section_pages_guess: IndexMap<u32, f64>,
    /// `noteMarks` (`noteMarkOverride` of the new context): marks of "restart each page" notes
    pub note_marks: Option<IndexMap<Box<str>, Box<[u16]>>>,
}

/// What a pass found, read by the pass loop (the TS public fields it reads after `run()`).
#[derive(Clone, Debug, Default)]
pub struct PassFindings {
    pub used_num_pages: bool,
    pub used_section_pages: bool,
    pub used_page_ref: bool,
    pub used_chapter: bool,
    pub used_note_ref: bool,
    /// `lateFound`
    pub late_found: IndexMap<u32, Vec<RunKey>>,
    /// `lateTablesFound`
    pub late_tables_found: IndexMap<u32, LateTable>,
    /// `balanceFound`
    pub balance_found: IndexMap<i64, BalanceInfo>,
    /// `headingMarks`: ref → level and number (for `chapterIndex()`)
    pub heading_marks: IndexMap<Box<str>, (f64, Box<[u16]>)>,
}

/// The context every engine method takes (K18).
pub struct Cx<'c, 'a> {
    pub ldoc: &'c LDoc<'a>,
    pub input: &'c PassInput,
    pub opts: &'c LayoutOptions,
    pub fonts: &'c mut FaceTable,
    pub provider: &'c dyn FontProvider,
    /// per layout call (K4, LQ19)
    pub resolver: &'c mut FaceResolverCache,
    /// the layout's resource tables (symbols, faces, images, text effects, page fills)
    pub res: &'c mut Resources,
    pub hyph: &'c mut HyphenSessions,
    pub budget: &'c mut Budget,
    /// the layout recursion depth (LQ13)
    pub depth: u32,
}

/// One pass (`class Engine`, engine.ts:326). See the module comment for the field classes; each
/// field names its class and its TS field.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    /// carried + cache + output: `ctx` (flow state, style caches, warnings; new per pass)
    pub ctx: DocContext,
    /// carried: `full` — the story the flow walks (the endnotes swap `this.full.blocks` while they
    /// come out, engine.ts:1088): index into `LDoc::stories`
    pub story: u32,

    // ── output ─────────────────────────────────────────────────────────────────────────────
    /// output: `pages`
    pub pages: Vec<PageState>,

    // ── carried flow state ─────────────────────────────────────────────────────────────────
    /// carried: `cur`
    pub cur: Option<PageIx>,
    /// carried: `col`
    pub col: u32,
    /// carried: `y`
    pub y: f64,
    /// carried: `colTop`
    pub col_top: f64,
    /// carried: `atTop`
    pub at_top: bool,
    /// carried: `topGapPrevAfter` (null = suppress)
    pub top_gap_prev_after: Option<f64>,
    /// carried: `prev`
    pub prev: Option<PrevPara>,
    /// carried: `lastBox` (K2f: `page` is the items array, a `ContainerId`)
    pub last_box: Option<LastBox>,
    /// carried: `firstInDoc`
    pub first_in_doc: bool,
    /// carried: `lnCount`
    pub ln_count: f64,
    /// carried: `lnPage` (`PageState` object → `PageIx`)
    pub ln_page: Option<PageIx>,
    /// carried: `lnSect`
    pub ln_sect: i64,
    /// carried: `lastRowEmit`
    pub last_row_emit: Option<LastRowEmit>,
    /// carried: `tblX0`
    pub tbl_x0: Option<f64>,
    /// carried: `inHf` (mode)
    pub in_hf: bool,
    /// carried: `floatCarry`
    pub float_carry: Vec<FloatCarry>,
    /// carried: `floatFlow` (`WeakMap<TableLayout>` → keyed by `TableLayoutId`, K18)
    pub float_flow: IndexMap<TableLayoutId, FloatFlow>,
    /// carried: the next `TableLayoutId`
    pub next_table_layout: u32,
    /// carried: `onlyTextFloats` (mode)
    pub only_text_floats: bool,
    /// carried: `prePlaced` (`Set<Run>`)
    pub pre_placed: IndexSet<RunKey>,
    /// carried: `zSeq` (K20: an Engine field, not module state)
    pub z_seq: f64,
    /// carried: `colUsed`
    pub col_used: Vec<f64>,
    /// carried: `balanceSaved`
    pub balance_saved: Option<f64>,
    /// carried: `balanceLastFree`
    pub balance_last_free: bool,
    /// carried: `placedTables`
    pub placed_tables: IndexSet<u32>,
    /// carried: `lateSplit`
    pub late_split: IndexSet<u32>,
    /// carried: `colBottoms`
    pub col_bottoms: Vec<f64>,
    /// carried: `inCell` (mode)
    pub in_cell: bool,
    /// carried: `hfFrameAfter`
    pub hf_frame_after: crate::items::PassItems,
    /// carried: `hfFrameTbl`
    pub hf_frame_tbl: crate::items::PassItems,
    /// carried: `staticDx`
    pub static_dx: f64,
    /// carried: `cellMar`
    pub cell_mar: Option<(f64, f64, Option<f64>)>,
    /// carried: `colMaxY`
    pub col_max_y: f64,
    /// carried: `sectGapAfter`
    pub sect_gap_after: Option<f64>,
    /// carried: `pendingJoin`
    pub pending_join: Option<HiddenJoin>,
    /// carried: `pendingDel` (`Run[]`)
    pub pending_del: Option<Vec<RunKey>>,
    /// carried: `floatTableGap`
    pub float_table_gap: bool,
    /// carried: `carryAnchorGap`
    pub carry_anchor_gap: bool,
    /// carried: `noteCarry`
    pub note_carry: Vec<NoteCarry>,
    /// carried: `rowNoteCut`
    pub row_note_cut: Option<NoteCut>,
    /// carried: `flowFills`
    pub flow_fills: bool,
    /// carried: `enFlow`
    pub en_flow: Option<EnFlow>,
    /// carried: `curEndnote`
    pub cur_endnote: Option<i32>,
    /// carried: `endnotesDone`
    pub endnotes_done: u32,
    /// carried: `pinnedFloats` (`Map<Run, { ps, y }>`, K2c)
    pub pinned_floats: IndexMap<RunKey, Pinned>,
    /// carried (memo within the pass, held by a checkpoint): `chainSlices`
    /// (`Map<TextboxChain, Map<DrawingTextbox, CellContent>>`, K2d)
    pub chain_slices: IndexMap<ChainId, IndexMap<TbKey, CellContent>>,

    // ── caches (pure functions of the document within the pass) ────────────────────────────
    /// cache: `geoms`
    pub geoms: Vec<SectGeom>,
    /// cache: `coverPage`, `firstPageNumber` (read at the start of `run()`)
    pub cover_page: bool,
    pub first_page_number: f64,
    /// cache: `listTexts` (block → marker)
    pub list_texts: Option<IndexMap<u32, Option<NumberMarker>>>,
    /// cache: `cellNb` (`WeakMap<TableCell>`, K2b)
    pub cell_nb: FxHashMap<CellKey, CellNeighbors>,
    /// cache: `noteCache` (note id and width → content)
    pub note_cache: IndexMap<Box<str>, CellContent>,
    /// cache: `sepCache`
    pub sep_cache: Option<SepMetrics>,
    // (`chains`, the engine's lazy `indexChains`, is `LDoc::chains`: built once per call)

    // ── findings ────────────────────────────────────────────────────────────────────────────
    pub found: PassFindings,
}

impl Engine {
    /// `new Engine(ctx, …)` with the pass state at its TS initial values.
    #[must_use]
    pub fn new(ctx: DocContext, body_story: u32) -> Engine {
        Engine { ctx, story: body_story, at_top: true, first_in_doc: true, ln_sect: -1, first_page_number: 1.0, ..Engine::default() }
    }

    /// `run()` (engine.ts): one pass over the document — F7a.
    pub fn run(&mut self, _cx: &mut Cx<'_, '_>) -> LResult<()> {
        unported(Frontier::F7a, "engine::Engine::run")
    }
}

/// One `layoutDocument` call (LA4): the derived document, the faces, the budget and the inputs
/// that go from pass to pass (engine.ts:197–251).
pub struct LayoutRun<'r, 'a> {
    pub ldoc: &'r LDoc<'a>,
    pub opts: &'r LayoutOptions,
    pub budget: Budget,
    pub resolver: FaceResolverCache,
    pub input: PassInput,
}

/// `layoutDocument(full, provider, opts)` (engine.ts:189): the pass loop (≤ 5 passes until
/// stable), then the balancing search per balanced section; the result of the last executed pass
/// (K19) — F7a.
pub fn layout_document(
    _ldoc: &LDoc<'_>,
    _opts: &LayoutOptions,
    _provider: &dyn FontProvider,
    _hyph: &mut HyphenSessions,
) -> LResult<Layout> {
    unported(Frontier::F7a, "engine::layout_document")
}
