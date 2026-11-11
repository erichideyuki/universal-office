//! The engine (`engine.ts`, frontiers F7a–d): the pass loop and the column-balancing search of
//! `layoutDocument`, and one pass, `class Engine`.
//!
//! **Shape (LA4, K18).** `layout_document` keeps the inter-pass inputs on a [`PassInput`]; every
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
//!
//! The modules, by frontier (records "Stage 3 — F7a" … "F7d"): the pass loop, the balancing
//! search, `run` with the page assembly (this file), the geometry (`page.rs`), the paragraph
//! flow (`flow.rs`) and the F7a fields (`fields.rs`) — F7a; tables (`tables.rs`), floats,
//! wraps, frames and objects (`floats.rs`, `objects.rs`) — F7b; the static flow
//! (`static_layout.rs`), headers/footers (`hf.rs`), notes and the endnote flow (`notes.rs`), the
//! reference fields (`field_refs.rs`) and V8's date parser (`date_legacy.rs`) — F7c; linked
//! textbox chains (`chain.rs`) and the tables of the endnote flow — F7d. Nothing of the engine
//! stops with `LayoutError::Unported` any more: the whole of `layoutDocument` is ported.

pub mod chain;
pub mod date_legacy;
pub mod field_refs;
pub mod fields;
pub mod floats;
pub mod flow;
pub mod hf;
pub mod notes;
pub mod objects;
pub mod page;
pub mod static_layout;
pub mod tables;

use indexmap::{IndexMap, IndexSet};
use rustc_hash::FxHashMap;

use crate::context::DocContext;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::fonts::{FaceResolverCache, FaceTable, FontProvider};
use crate::hyphenate::HyphenSessions;
use crate::items::{ItemSide, PassItems};
use crate::keys::{ChainId, PageIx, RunKey, TableLayoutId, TbKey};
use crate::ldoc::LDoc;
use crate::model::CellKey;
use crate::options::{Budget, LayoutOptions, RevisionMode, WorkInput};
use crate::output::{CharsSoA, GlyphMeta, Layout, LayoutPage, LayoutStamp, LineGeom, LineParaFormat};
use crate::table::CellContent;
use crate::word::{self, NumberMarker};
use uo_jsnum::math as jm;
use uo_render::types::Item;
use uo_render::{ItemList, Resources, Span};

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
    /// `lineSeq` (paint.ts:16, module state in the TS): the last `lineBox.line` handed out, per
    /// layout call (LA3); `paint_line` increments it (added by F5)
    pub line_seq: &'c mut u32,
    /// the image identities of the call and their origins (RQ3; added by F7b, F7b-1)
    pub images: &'c mut objects::ImageTable,
    /// the paragraph memo of an incremental relayout (LA7 level 1): `None` on the full path, in
    /// scratch documents and while a memoized paragraph is being prepared
    pub memo: Option<&'c mut crate::incr::ParaMemo>,
}

/// `LATE_ITEMS` entries (table.ts:1383): the z order of an item anchored in a cell, by
/// `ItemSide::meta` (F7b sets them; the page assembly reads them).
#[derive(Clone, Copy, Debug)]
pub struct LateMark {
    pub z: f64,
    pub seq: f64,
    pub behind: bool,
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
    /// carried: `colBottoms` (a sparse JS array: `None` = hole)
    pub col_bottoms: Vec<Option<f64>>,
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
    pub cur_endnote: Option<crate::model::NumKey>,
    /// carried: `endnotesDone`
    pub endnotes_done: u32,
    /// carried: `pinnedFloats` (`Map<Run, { ps, y }>`, K2c)
    pub pinned_floats: IndexMap<RunKey, Pinned>,
    /// carried (memo within the pass, held by a checkpoint): `chainSlices`
    /// (`Map<TextboxChain, Map<DrawingTextbox, CellContent>>`, K2d)
    pub chain_slices: IndexMap<ChainId, IndexMap<TbKey, CellContent>>,
    /// carried: the page and body block the `fieldValue(ps, block)` closure of the paragraph being
    /// prepared captured (`ParaEnv.field` = `FieldSource::Page`, LA3)
    pub field_at: Option<(PageIx, i64)>,
    /// carried (F7b): the page the cells of the table being laid out anchor their objects on (the
    /// `ps` of the TS `layoutCell` closure; `None` = no table, `Some(None)` = a measure)
    pub cell_ps: Option<Option<PageIx>>,
    /// carried (F7b): the `paras` of the cells of a table a static flow is laying out (merged into
    /// the flow's own, engine.ts:1446)
    pub cell_collect: Option<IndexMap<crate::output::LineRef, crate::table::ParaSplitInfo>>,
    /// carried: `LATE_ITEMS` marks by `ItemMeta` (F7b fills them)
    pub late_marks: Vec<LateMark>,
    /// carried (F7c): `scratchPage(ps)` of a header/footer measure, reachable as [`SCRATCH`]
    pub scratch: Option<Box<PageState>>,
    /// carried (F7c): the next `CellContentId` of a static flow's items array (`boxPage`)
    pub static_seq: u32,
    /// carried (F7c): the page of the `fieldValue(ps)` closure without a block (header/footer
    /// paragraphs, `FieldSource::HeaderFooter`)
    pub field_page: Option<PageIx>,
    /// carried (F7c): the blocks of the endnote flow while it comes out (`this.full = { ...saved,
    /// blocks }`, engine.ts:2472): what the flow walks instead of `story`
    pub en_blocks: Option<Vec<notes::EnBlock>>,
    /// cache (F7c): the number of section geometries (`this.geoms.length`; the endnote flows'
    /// copies `gEn` come after them)
    pub sect_geoms: u32,

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

    // ── output (F8) ─────────────────────────────────────────────────────────────────────────
    /// `onField`: the field values this pass reported (`Some` while the call collects them),
    /// moved into the call's map after the pass (`Call::take_fields`)
    pub fields: Option<IndexMap<crate::model::RunRef, crate::model::Utf16>>,

    // ── trace (LA7; not TS state) ───────────────────────────────────────────────────────────
    /// trace: what a recorded pass read and wrote between two page checkpoints of the
    /// incremental relayout (the body blocks read besides the one being placed, the earliest page
    /// written). `None` on the full path; never compared, never part of a checkpoint.
    pub trace: Option<Box<crate::incr::Trace>>,
}

/// What a [`BlockHook`] tells the body loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Continue,
    /// end the pass here (the incremental relayout converged on the previous run, LA7)
    Stop,
}

/// Called before each body block of the main flow (never inside cells, static flows or the
/// endnote flow): the page checkpoints of the incremental relayout (LA7). The full path uses
/// [`NoHook`].
pub trait BlockHook {
    /// Before body block `b` of section `gi`.
    ///
    /// # Errors
    /// What the hook's own work fails with (a capacity limit).
    fn before_block(&mut self, eng: &mut Engine, cx: &Cx<'_, '_>, gi: u32, b: usize) -> LResult<Control>;
}

/// The hook of the full path: nothing.
pub struct NoHook;

impl BlockHook for NoHook {
    fn before_block(&mut self, _: &mut Engine, _: &Cx<'_, '_>, _: u32, _: usize) -> LResult<Control> {
        Ok(Control::Continue)
    }
}

/// `===` between two JS numbers (NaN differs from itself, `-0 === 0`).
#[must_use]
pub(crate) fn js_eq(a: f64, b: f64) -> bool {
    a == b
}

/// `x | 0`-free index of a JS number that must be an array index (`blocks[b]`): `None` when the
/// TS read would be `undefined` (fractional, negative, NaN or past `u32`).
/// The page index of `scratchPage(ps)` (F7c): a throwaway copy of a page held in
/// `Engine::scratch`, never in `pages` (`pages.indexOf(scratch)` is −1).
pub(crate) const SCRATCH: PageIx = PageIx(u32::MAX);

#[must_use]
pub(crate) fn index_of(x: f64) -> Option<usize> {
    let i = uo_jsnum::conv::to_uint32(x);
    (js_eq(f64::from(i), x)).then(|| usize::try_from(i).ok()).flatten()
}

impl Engine {
    /// `new Engine(ctx, …)` with the pass state at its TS initial values.
    #[must_use]
    pub fn new(ctx: DocContext, body_story: u32) -> Engine {
        Engine { ctx, story: body_story, at_top: true, first_in_doc: true, ln_sect: -1, first_page_number: 1.0, ..Engine::default() }
    }

    /// The current page (`this.cur`, asserted non-null by the TS: an absent page is a TS
    /// `TypeError`).
    pub(crate) fn cur_ix(&self) -> LResult<PageIx> {
        self.cur.ok_or(LayoutError::Internal("engine: no current page"))
    }

    pub(crate) fn ps(&self, ix: PageIx) -> LResult<&PageState> {
        if ix == SCRATCH {
            return self.scratch.as_deref().ok_or(LayoutError::Internal("engine: no scratch page"));
        }
        self.pages.get(ix.ix()).ok_or(LayoutError::Internal("engine: page index"))
    }

    pub(crate) fn ps_mut(&mut self, ix: PageIx) -> LResult<&mut PageState> {
        if ix == SCRATCH {
            return self.scratch.as_deref_mut().ok_or(LayoutError::Internal("engine: no scratch page"));
        }
        if let Some(t) = self.trace.as_deref_mut() {
            t.touch(ix.0);
        }
        self.pages.get_mut(ix.ix()).ok_or(LayoutError::Internal("engine: page index"))
    }

    /// The trace notes a read of body block `i` of the main flow (LA7; nothing on the full path
    /// or in the endnote flow).
    pub(crate) fn note_read(&self, i: usize) {
        if self.en_blocks.is_none()
            && let Some(t) = self.trace.as_deref()
        {
            t.read(i);
        }
    }

    /// The trace notes a read of the whole body (LA7).
    pub(crate) fn note_read_all(&self) {
        if let Some(t) = self.trace.as_deref() {
            t.read_all();
        }
    }

    pub(crate) fn cur_ps(&self) -> LResult<&PageState> {
        self.ps(self.cur_ix()?)
    }

    pub(crate) fn cur_ps_mut(&mut self) -> LResult<&mut PageState> {
        let ix = self.cur_ix()?;
        self.ps_mut(ix)
    }

    pub(crate) fn geom(&self, gi: u32) -> LResult<&SectGeom> {
        self.geoms.get(gi as usize).ok_or(LayoutError::Internal("engine: section geometry"))
    }

    /// `noteMarkFix()` (engine.ts:588): with "restart each page" footnotes, the mark of each note
    /// by the page it came out on — `None` when this pass's marks already hold.
    #[must_use]
    pub fn note_mark_fix(&self, cx: &Cx<'_, '_>) -> Option<IndexMap<Box<str>, Box<[u16]>>> {
        let mut want: IndexMap<Box<str>, Box<[u16]>> = IndexMap::new();
        let mut differs = false;
        let mut seen: Vec<crate::model::NumKey> = Vec::new();
        let mut cap = word::text_cap(&cx.opts.limits);
        for ps in &self.pages {
            let Some(g) = self.geoms.get(ps.geom as usize) else { continue };
            let nn = &g.notes.footnote;
            let mut k = 0.0;
            for n in &ps.notes {
                if seen.contains(&n.id) {
                    continue;
                }
                seen.push(n.id);
                let id = n.id;
                let Some(cur) = self.ctx.note_mark_of(crate::context::NoteKindFlow::Footnote, id) else { continue };
                if cur.is_empty() || nn.restart != crate::context::NoteRestart::EachPage {
                    continue;
                }
                k += 1.0;
                let m = word::format_number(nn.start - 1.0 + k, Some(&nn.fmt), &cx.opts.lang, &mut cap);
                if m.units() != cur {
                    differs = true;
                }
                want.insert(crate::context::note_key(crate::context::NoteKindFlow::Footnote, id).into_boxed_str(), m.units().into());
            }
        }
        differs.then_some(want)
    }

    /// `chapterIndex()` (engine.ts:622): numbered headings of this pass per level and the page
    /// where each starts.
    #[must_use]
    pub fn chapter_index(&self) -> ChapterIndex {
        let mut out = ChapterIndex::new();
        let mut seen: IndexSet<String> = IndexSet::new();
        for (i, ps) in self.pages.iter().enumerate() {
            for l in &ps.page.lines {
                let r = l.at.to_ts();
                let Some((level, text)) = self.found.heading_marks.get(r.as_str()) else { continue };
                if seen.contains(&r) {
                    continue;
                }
                seen.insert(r);
                // `level` is `+lv` of a `[1-9]` match: an integer
                let key = if *level >= 1.0 && *level <= 9.0 { uo_jsnum::conv::to_int32(*level).into() } else { 0 };
                out.entry(key).or_default().push(ChapterMark { page: f64::from(u32::try_from(i).unwrap_or(u32::MAX)), text: text.clone() });
            }
        }
        out
    }

    /// `sectionPageCounts()` (engine.ts:1083).
    #[must_use]
    pub fn section_page_counts(&self) -> IndexMap<u32, f64> {
        let mut out: IndexMap<u32, f64> = IndexMap::new();
        for p in &self.pages {
            let idx = self.geoms.get(p.geom as usize).map_or(0, |g| g.index);
            *out.entry(idx).or_insert(0.0) += 1.0;
        }
        out
    }

    /// `run()` (engine.ts:669): one pass over the document (the page assembly is
    /// [`Engine::finish`]).
    pub fn run(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        self.prologue(cx)?;
        self.run_from(cx, None, &mut NoHook).map(|_| ())
    }

    /// The start of `run()`: the cover page, the first page number and the section geometries
    /// (a pure function of the view and the options, LA7 compares it before a resume).
    pub(crate) fn prologue(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        cx.budget.charge(1)?;
        let view = cx.ldoc.view;
        self.cover_page = word::has_cover_page(view.body().blocks().filter_map(|b| match b {
            uo_docx::model::Block::Paragraph(p) => Some(p),
            uo_docx::model::Block::Table(_) => None,
        }));
        let sections = view.sections();
        self.first_page_number =
            sections.first().and_then(|s| s.sect_pr.pg_num_start).and_then(crate::model::JsNumber::as_f64).unwrap_or(1.0);
        // sequential: every section inherits the header/footer of the one before (geomOf reads
        // this.geoms)
        self.geoms = Vec::new();
        for (i, s) in sections.iter().enumerate() {
            let g = self.geom_of(cx, Some(s), i)?;
            self.geoms.push(g);
        }
        if self.geoms.is_empty() {
            let g = self.geom_of(cx, None, 0)?;
            self.geoms.push(g);
        }
        self.sect_geoms = u32::try_from(self.geoms.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        Ok(())
    }

    /// The rest of `run()` after the prologue: the sections and their body blocks, from the
    /// start or (`start` = section, block; LA7) from a page checkpoint taken before that block,
    /// whose section start already ran. `hook` runs before each body block; `Ok(true)` when it
    /// stopped the pass.
    pub(crate) fn run_from(&mut self, cx: &mut Cx<'_, '_>, start: Option<(u32, usize)>, hook: &mut dyn BlockHook) -> LResult<bool> {
        let view = cx.ldoc.view;
        let sections = view.sections();
        let blocks_len = self.story_len(cx);
        let en_sect_end = view.formatting().settings.endnote_pr.as_ref().and_then(|e| e.pos.as_deref()) == Some("sectEnd");
        let n_geoms = self.sect_geoms;
        for gi in start.map_or(0, |s| s.0)..n_geoms {
            let g = self.geom(gi)?.clone();
            let resume = start.filter(|s| s.0 == gi).map(|s| s.1);
            if resume.is_none() {
                self.section_start(cx, gi, &g)?;
            }
            let mut b = match resume {
                Some(sb) => f64::from(u32::try_from(sb).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?),
                None => g.blocks.0,
            };
            while b <= g.blocks.1 && b < blocks_len {
                let bi = index_of(b).ok_or(LayoutError::Internal("engine: block index"))?;
                if hook.before_block(self, cx, gi, bi)? == Control::Stop {
                    return Ok(true);
                }
                cx.budget.charge(1)?;
                let block = cx
                    .ldoc
                    .stories
                    .get(self.story as usize)
                    .and_then(|s| s.blocks.get(bi))
                    .copied()
                    .ok_or(LayoutError::Internal("engine: block"))?;
                match block {
                    crate::ldoc::LBlock::Paragraph(p) => self.place_paragraph(cx, p, bi, gi)?,
                    crate::ldoc::LBlock::Table { block } => {
                        // `placeTable(block, 'b' + b, b)` (F7b)
                        let t = tables::story_table(cx, self.story, block)?;
                        self.place_table(cx, t, &tables::body_ref(bi), Some(bi))?;
                    }
                }
                b += 1.0;
            }
            // the section that suppresses its endnotes passes them to the next one; at the end of
            // the document they all come out
            let last_sect = gi + 1 == n_geoms;
            let no_endnote = sections.get(g.index as usize).and_then(|s| s.sect.as_ref()).and_then(|s| s.no_endnote) == Some(true);
            if last_sect || (en_sect_end && !no_endnote) {
                self.place_endnotes(cx, gi)?;
            }
        }
        // a floating table split at the end of the document: the remaining rows get pages
        let mut k = 0;
        while k < 100 && !self.float_carry.is_empty() && self.cur.is_some() {
            let gi = self.cur_ps()?.geom;
            self.new_page(cx, gi, "nextPage")?;
            k += 1;
        }
        if self.cur.is_some() {
            self.close_columns(cx)?;
            let c = self.cur_ix()?;
            self.paint_notes(cx, c)?;
        }
        Ok(false)
    }

    /// The start of section `gi` in `run()`: its note numbering, then a new page, the next
    /// column or the continuous start.
    fn section_start(&mut self, cx: &mut Cx<'_, '_>, gi: u32, g: &SectGeom) -> LResult<()> {
        // the numbering of the section's notes (a per-section restart counts from here)
        self.ctx.begin_note_section(
            crate::context::NoteNumberings { footnote: g.notes.footnote.clone(), endnote: g.notes.endnote.clone() },
            g.index == 0,
        );
        // continuous (or next column) to another paper size/orientation: Word starts a new page
        let same_paper = match self.cur {
            Some(c) => {
                let p = &self.ps(c)?.page;
                js_eq(p.w, g.page_w) && js_eq(p.h, g.page_h)
            }
            None => false,
        };
        if g.index > 0 && &*g.kind == "nextColumn" && same_paper && self.next_column_fits(gi)? {
            self.next_column_section(gi)?;
        } else if g.index == 0 || &*g.kind != "continuous" || !same_paper {
            let last_after = self.prev.as_ref().map_or(0.0, |p| p.after);
            self.new_page(cx, gi, &g.kind)?;
            // a new section: its first paragraph keeps the space before, less the "after" of
            // the previous section's last one (tocref-sectgap)
            if g.index > 0 {
                self.top_gap_prev_after = Some(last_after);
            }
        } else {
            self.continuous_section(cx, gi)?;
        }
        Ok(())
    }

    /// The block count of the story the flow walks (`full.blocks.length`).
    pub(crate) fn story_len(&self, cx: &Cx<'_, '_>) -> f64 {
        // (the endnote flow walks its own blocks, F7c)
        if let Some(en) = &self.en_blocks {
            return f64::from(u32::try_from(en.len()).unwrap_or(u32::MAX));
        }
        cx.ldoc.stories.get(self.story as usize).map_or(0.0, |s| f64::from(u32::try_from(s.blocks.len()).unwrap_or(u32::MAX)))
    }
}

// ───────────────────────── the page assembly (the end of `run()`) ─────────────────────────

/// A z-ordered group of items (`ZObj`) being assembled: its items are ranges of a source list.
struct ZGroup<'p> {
    z: f64,
    seq: f64,
    parts: Vec<(&'p PassItems, usize)>,
}

/// `zItems([...groups, ...back])`: the items of the groups in z order.
fn flat<'p>(groups: Vec<ZGroup<'p>>, back: Vec<ZGroup<'p>>) -> Vec<(&'p PassItems, usize)> {
    let mut all = groups;
    all.extend(back);
    z_sorted(all).into_iter().flat_map(|g| g.parts).collect()
}

/// `zItems(objs)`: by `z`, then `seq` (a stable sort; NaN comparisons give 0 as the TS `||`).
fn z_sorted(mut objs: Vec<ZGroup<'_>>) -> Vec<ZGroup<'_>> {
    objs.sort_by(|a, b| {
        let d = a.z - b.z;
        let d = if uo_jsnum::conv::truthy(d) { d } else { a.seq - b.seq };
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    objs
}

/// Copies item `i` of `src` to `dst` unchanged (a JS array move keeps the object: no `+ 0`
/// arithmetic that would turn `-0` into `0`).
pub(crate) fn push_copy(dst: &mut PassItems, src: &PassItems, i: usize) -> LResult<()> {
    let Some(it) = src.list.items.get(i) else { return Ok(()) };
    copy_list_item(&mut dst.list, &src.list, it)?;
    dst.side.push(src.side.get(i).copied().unwrap_or_default());
    Ok(())
}

fn copy_list_item(dst: &mut ItemList, src: &ItemList, it: &Item) -> LResult<()> {
    match it {
        Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
            let side = src.run(*run).copied().unwrap_or_default();
            let head = uo_render::GlyphRunHead {
                face: *face,
                color: *color,
                size_pt: *size_pt,
                y: *y,
                text: src.text_of(side.text),
                scale_x: side.scale_x,
                alpha: side.alpha,
                fx: side.fx,
                flags: *flags,
            };
            dst.push_glyphs(head, src.glyphs_of(*glyphs))?;
        }
        Item::Path { cmds, fill, stroke, even_odd } => {
            let c: Vec<_> = src.cmds_of(*cmds).to_vec();
            dst.push_path(&c, *fill, *stroke, *even_odd)?;
        }
        other => dst.push_plain(other.clone()),
    }
    Ok(())
}

/// `out.splice(at, 0, ...ins)`.
pub(crate) fn splice_items(out: &mut PassItems, at: usize, ins: &PassItems) -> LResult<()> {
    if ins.is_empty() {
        return Ok(());
    }
    if at >= out.len() {
        for i in 0..ins.len() {
            push_copy(out, ins, i)?;
        }
        return Ok(());
    }
    let old = std::mem::take(out);
    for i in 0..at {
        push_copy(out, &old, i)?;
    }
    for i in 0..ins.len() {
        push_copy(out, ins, i)?;
    }
    for i in at..old.len() {
        push_copy(out, &old, i)?;
    }
    Ok(())
}

/// `cullOffPage(items, pageW, pageH)` (engine.ts:4000): the indices of `items` that stay — a glyph
/// run entirely off the page goes unless another run of its line (`lineBox.line`) crosses it.
fn cull_off_page(items: &[(&PassItems, usize)], page_w: f64, page_h: f64) -> Vec<bool> {
    type M = [f64; 6];
    let mut stack: Vec<Option<M>> = Vec::new();
    let mut m: Option<M> = None;
    let mut outside = vec![false; items.len()];
    let mut line_in: IndexSet<u32> = IndexSet::new();
    for (k, &(src, i)) in items.iter().enumerate() {
        let Some(it) = src.list.items.get(i) else { continue };
        let side = src.side.get(i).copied().unwrap_or_default();
        match it {
            Item::XformPush { m: Some(n) } => {
                stack.push(m);
                let n = **n;
                m = Some(match m {
                    Some(m) => [
                        m[0] * n[0] + m[2] * n[1],
                        m[1] * n[0] + m[3] * n[1],
                        m[0] * n[2] + m[2] * n[3],
                        m[1] * n[2] + m[3] * n[3],
                        m[0] * n[4] + m[2] * n[5] + m[4],
                        m[1] * n[4] + m[3] * n[5] + m[5],
                    ],
                    None => n,
                });
                continue;
            }
            Item::XformPush { m: None } => continue,
            Item::XformPop => {
                m = stack.pop().flatten();
                continue;
            }
            _ => {}
        }
        let Item::Glyphs { size_pt, y, glyphs, .. } = it else { continue };
        let gm = side.glyph;
        // (the line number stays even off the page — oracle lnnum2/ln-dist-wide)
        if glyphs.len == 0 || gm.is_some_and(|g| g.line_num) {
            continue;
        }
        let em = size_pt * 20.0;
        let lb = gm.and_then(|g| g.line_box);
        let (asc, desc, lead) = lb.map_or((em, em * 0.3, 0.0), |b| (b.asc, b.desc, b.lead));
        let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut first_x = None;
        for (_, gx, _) in src.list.glyphs_of(*glyphs) {
            first_x.get_or_insert(gx);
            x0 = jm::min(x0, gx);
            x1 = jm::max(x1, gx + em);
        }
        // the left edge of the frame (the text may overflow to either side)
        let frame_l = first_x.unwrap_or(f64::NAN) - lead;
        x0 = jm::min(x0, frame_l);
        x1 = jm::max(x1, frame_l);
        let (y0, y1) = (y - asc, y + desc);
        let (mut bx0, mut bx1, mut by0, mut by1) = (x0, x1, y0, y1);
        if let Some(mm) = m {
            let pts =
                [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(px, py)| (mm[0] * px + mm[2] * py + mm[4], mm[1] * px + mm[3] * py + mm[5]));
            bx0 = jm::min_n(&pts.map(|q| q.0));
            bx1 = jm::max_n(&pts.map(|q| q.0));
            by0 = jm::min_n(&pts.map(|q| q.1));
            by1 = jm::max_n(&pts.map(|q| q.1));
        }
        if bx1 <= 0.0 || bx0 >= page_w || by1 <= 0.0 || by0 >= page_h {
            if let Some(o) = outside.get_mut(k) {
                *o = true;
            }
        } else if let Some(b) = lb {
            line_in.insert(b.line.0);
        }
    }
    items
        .iter()
        .enumerate()
        .map(|(k, &(src, i))| {
            if !outside.get(k).copied().unwrap_or(false) {
                return true;
            }
            let lb = src.side.get(i).and_then(|s| s.glyph).and_then(|g| g.line_box);
            matches!(src.list.items.get(i), Some(Item::Glyphs { .. })) && lb.is_some_and(|b| line_in.contains(&b.line.0))
        })
        .collect()
}

impl Engine {
    /// The `LATE_ITEMS` split of an items array (`late(arr)` in `run()`): the items without a mark
    /// stay; the marked ones go, grouped by consecutive mark, to the front or behind layer.
    fn late_split<'p>(&self, arr: &'p PassItems) -> (Vec<(&'p PassItems, usize)>, Vec<ZGroup<'p>>, Vec<ZGroup<'p>>) {
        let (mut keep, mut front, mut back) = (Vec::new(), Vec::<ZGroup<'p>>::new(), Vec::<ZGroup<'p>>::new());
        let mut cur: Option<(u32, bool)> = None;
        for i in 0..arr.len() {
            let mk = arr.side.get(i).and_then(|s| s.meta).and_then(|m| self.late_marks.get(m.ix()).map(|lm| (m.0, *lm)));
            let Some((id, lm)) = mk else {
                keep.push((arr, i));
                continue;
            };
            if cur.map(|c| c.0) != Some(id) {
                let g = ZGroup { z: lm.z, seq: lm.seq, parts: Vec::new() };
                if lm.behind {
                    back.push(g);
                } else {
                    front.push(g);
                }
                cur = Some((id, lm.behind));
            }
            let target = if lm.behind { back.last_mut() } else { front.last_mut() };
            if let Some(g) = target {
                g.parts.push((arr, i));
            }
        }
        (keep, front, back)
    }

    fn late_z<'p>(&self, objs: &'p [page::ZObj], front: &mut Vec<ZGroup<'p>>, back: &mut Vec<ZGroup<'p>>) -> Vec<ZGroup<'p>> {
        objs.iter()
            .map(|o| {
                let (keep, f, b) = self.late_split(&o.items);
                front.extend(f);
                back.extend(b);
                ZGroup { z: o.z, seq: o.seq, parts: keep }
            })
            .collect()
    }

    /// The end of `run()` (engine.ts:714–764): per page the layers in Word's order (the whole
    /// header/footer under the body; behind the text < text < in front), off-page culling, page
    /// borders, revision bars, the background; the warnings. Builds the [`Layout`] of this pass.
    pub fn finish(&mut self, cx: &mut Cx<'_, '_>) -> LResult<Vec<LayoutPage>> {
        let env = self.finish_prep(cx)?;
        let pages = std::mem::take(&mut self.pages);
        let mut out = Vec::with_capacity(pages.len());
        let mut r = Ok(());
        for ps in &pages {
            match self.finish_page(cx, ps, &env, None) {
                Ok(lp) => out.push(lp),
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        self.pages = pages;
        r.map(|()| out)
    }

    /// What `finish` computes once before the pages: the page background, its fill (interned)
    /// and its warnings, the revision-bar colour.
    pub(crate) fn finish_prep(&mut self, cx: &mut Cx<'_, '_>) -> LResult<FinishEnv> {
        let view = cx.ldoc.view;
        let fmt = view.formatting();
        // the page background (`w:background`): Word writes it in the PDF; white = nothing
        let bg_spec = view.background();
        let bg = bg_spec.and_then(|b| {
            word::resolve_color(
                Some(word::ColorRef {
                    val: b.color.as_deref().filter(|s| !s.is_empty()),
                    theme_color: b.theme_color.as_deref().filter(|s| !s.is_empty()),
                    theme_tint: b.theme_tint.as_deref().filter(|s| !s.is_empty()),
                    theme_shade: b.theme_shade.as_deref().filter(|s| !s.is_empty()),
                }),
                &fmt.theme,
            )
        });
        let bg_fill = match bg_spec {
            Some(b) if b.fill.is_some() => {
                let mut warns: Vec<String> = Vec::new();
                let fill = crate::drawing::resolve_page_fill(
                    b,
                    bg.as_deref(),
                    &view.document().media,
                    &mut crate::drawing::ResWriter { res: cx.res, origins: &mut cx.images.origins },
                    &mut |w: &str| warns.push(w.to_owned()),
                )?;
                for w in warns {
                    self.ctx.warn(&w);
                }
                match fill {
                    Some(f) => Some(cx.res.add_page_fill(f).ok_or(LayoutError::Limit(LimitKind::Capacity))?),
                    None => None,
                }
            }
            _ => None,
        };
        Ok(FinishEnv { bg, bg_fill, bg_show: fmt.settings.display_background_shape, bar_color: self.ctx.rev.bar_color() })
    }

    /// One page of `finish`. With `reuse` (LA7: a page the incremental relayout did not lay out
    /// again, equal to the one it assembled before), only the resource-touching steps run, in the
    /// same order (page borders, revision-bar colour, background), and `reuse` is returned.
    pub(crate) fn finish_page(
        &self,
        cx: &mut Cx<'_, '_>,
        ps: &PageState,
        env: &FinishEnv,
        reuse: Option<LayoutPage>,
    ) -> LResult<LayoutPage> {
        let view = cx.ldoc.view;
        let (bg, bg_fill, bg_show, bar_color) = (&env.bg, env.bg_fill, env.bg_show, env.bar_color);
        if let Some(lp) = reuse {
            let geom = self.geoms.get(ps.geom as usize).ok_or(LayoutError::Internal("engine: page geometry"))?;
            let pb = view.sections().get(geom.index as usize).and_then(|s| s.sect.as_ref()).and_then(|s| s.pg_borders.as_deref());
            if let Some(pb) = pb {
                drop(self.page_borders(cx, ps, geom, pb)?);
            }
            if let Some(color) = bar_color
                && ps.page.lines.iter().any(|l| l.rev.is_some())
            {
                cx.res.symbols.intern(&color.encode_utf16().collect::<Vec<_>>()).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
            }
            if let Some(bg) = bg.as_deref()
                && uo_text::case::to_uppercase_str(bg) != "FFFFFF"
                && bg != "auto"
            {
                cx.res.symbols.intern(&bg.encode_utf16().collect::<Vec<_>>()).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
            }
            return Ok(lp);
        }
        {
            let (page_w, page_h) = (ps.page.w, ps.page.h);
            let (lh_keep, mut lh_front, mut lh_back) = self.late_split(&ps.hf);
            let (lb_keep, mut lb_front, mut lb_back) = self.late_split(&ps.body);
            let hf_b = self.late_z(&ps.hf_behind, &mut lh_front, &mut lh_back);
            let hf_f = self.late_z(&ps.hf_front, &mut lh_front, &mut lh_back);
            let b_b = self.late_z(&ps.behind, &mut lb_front, &mut lb_back);
            let b_f = self.late_z(&ps.front, &mut lb_front, &mut lb_back);
            let mut hf_seq = flat(hf_b, lh_back);
            hf_seq.extend(lh_keep);
            hf_seq.extend(flat(hf_f, lh_front));
            let keep_hf = cull_off_page(&hf_seq, page_w, page_h);
            let hf_layer: Vec<_> = hf_seq.iter().zip(&keep_hf).filter(|(_, k)| **k).map(|(x, _)| *x).collect();
            // page borders: behind the text (zOrder back) or in front (default)
            let geom = self.geoms.get(ps.geom as usize).ok_or(LayoutError::Internal("engine: page geometry"))?;
            let pb = view.sections().get(geom.index as usize).and_then(|s| s.sect.as_ref()).and_then(|s| s.pg_borders.as_deref());
            let pb_items = match pb {
                Some(pb) => self.page_borders(cx, ps, geom, pb)?,
                None => PassItems::new(),
            };
            let pb_back = pb.and_then(|p| p.z_order.as_deref()) == Some("back");
            let mut body_seq = flat(b_b, lb_back);
            body_seq.extend(lb_keep);
            body_seq.extend(flat(b_f, lb_front));
            let keep_body = cull_off_page(&body_seq, page_w, page_h);
            let mut items = PassItems::new();
            for &(src, i) in &hf_layer {
                push_copy(&mut items, src, i)?;
            }
            if pb_back {
                for i in 0..pb_items.len() {
                    push_copy(&mut items, &pb_items, i)?;
                }
            }
            for (&(src, i), k) in body_seq.iter().zip(&keep_body) {
                if *k {
                    push_copy(&mut items, src, i)?;
                }
            }
            if !pb_back {
                for i in 0..pb_items.len() {
                    push_copy(&mut items, &pb_items, i)?;
                }
            }
            // change bars (tracked changes): in front, in the left margin
            if let Some(color) = bar_color
                && ps.page.lines.iter().any(|l| l.rev.is_some())
            {
                let ml =
                    view.sections().get(geom.index as usize).map_or(1701.0, |s| s.sect_pr.margin_left_twips.as_f64().unwrap_or(1701.0));
                let c = cx.res.symbols.intern(&color.encode_utf16().collect::<Vec<_>>()).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
                flow::revision_bar_items(&ps.page.lines, word::bar_x(ml), c, &mut items);
            }
            let mut lp = LayoutPage {
                section: ps.page.section,
                number: ps.page.number,
                hf_items: Some(u32::try_from(hf_layer.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?),
                objects: ps.page.objects.clone(),
                cells: ps.page.cells.clone(),
                ..LayoutPage::default()
            };
            lp.page.w = page_w;
            lp.page.h = page_h;
            if let Some(bg) = bg.as_deref()
                && uo_text::case::to_uppercase_str(bg) != "FFFFFF"
                && bg != "auto"
            {
                lp.page.background =
                    Some(cx.res.symbols.intern(&bg.encode_utf16().collect::<Vec<_>>()).ok_or(LayoutError::Limit(LimitKind::Capacity))?);
            }
            if let Some(f) = bg_fill {
                lp.page.background_fill = Some(f);
            }
            if bg_show && (lp.page.background.is_some() || bg_fill.is_some()) {
                lp.page.background_show = true;
            }
            emit_page(&mut lp, items, &ps.page.lines, &ps.page.content_lines)?;
            Ok(lp)
        }
    }
}

/// What [`Engine::finish_prep`] computes once for every page.
pub(crate) struct FinishEnv {
    bg: Option<String>,
    bg_fill: Option<uo_render::PageFillId>,
    bg_show: bool,
    bar_color: Option<&'static str>,
}

/// The pass-scope items and lines of a page → the uo-render page and the Write section.
fn emit_page(lp: &mut LayoutPage, items: PassItems, lines: &[crate::items::PassLine], content: &[crate::items::PassLine]) -> LResult<()> {
    let cap = |n: usize| u32::try_from(n).map_err(|_| LayoutError::Limit(LimitKind::Capacity));
    for (i, s) in items.side.iter().enumerate() {
        let i = cap(i)?;
        if let Some(g) = s.glyph
            && (g.line_box.is_some() || g.line_num)
        {
            lp.item_meta.push((i, GlyphMeta { line_box: g.line_box, line_num: g.line_num }));
        }
        if let Some(src) = s.clip_src {
            lp.clip_src.push((i, src));
        }
    }
    lp.page.list = items.list;
    let mut last_fmt: Option<(*const LineParaFormat, u32)> = None;
    // the page's lines, then the textbox content lines (`PlacedObject.content`, F7b)
    for (l, is_content) in lines.iter().map(|l| (l, false)).chain(content.iter().map(|l| (l, true))) {
        let chars = match &l.chars {
            Some(cs) => {
                let s0 = cap(lp.chars.o.len())?;
                for &(o, x, w) in cs.iter() {
                    lp.chars.o.push(o);
                    lp.chars.x.push(x);
                    lp.chars.w.push(w);
                }
                Some(Span { start: s0, len: cap(cs.len())? })
            }
            None => None,
        };
        let pfmt = match &l.pfmt {
            Some(f) => {
                let ptr = std::sync::Arc::as_ptr(f);
                match last_fmt {
                    Some((p, ix)) if std::ptr::eq(p, ptr) => Some(ix),
                    _ => {
                        lp.pfmts.push((**f).clone());
                        let ix = cap(lp.pfmts.len() - 1)?;
                        last_fmt = Some((ptr, ix));
                        Some(ix)
                    }
                }
            }
            None => None,
        };
        let dst = if is_content { &mut lp.content_lines } else { &mut lp.lines };
        dst.push(LineGeom {
            at: l.at.clone(),
            start: l.start,
            end: l.end,
            x: l.x,
            y: l.y,
            height: l.height,
            baseline: l.baseline,
            width: l.width,
            chars,
            hyph: l.hyph,
            rev: l.rev,
            frame: l.frame,
            pfmt,
        });
    }
    let _ = CharsSoA::default;
    let _ = ItemSide::default;
    Ok(())
}

// ───────────────────────── layoutDocument ─────────────────────────

/// The per-call state of one `layoutDocument`: the faces, resources and budget every pass shares.
pub(crate) struct Call<'r, 'a> {
    pub(crate) ldoc: &'r LDoc<'a>,
    pub(crate) opts: &'r LayoutOptions,
    pub(crate) provider: &'r dyn FontProvider,
    pub(crate) hyph: &'r mut HyphenSessions,
    pub(crate) fonts: FaceTable,
    pub(crate) resolver: FaceResolverCache,
    pub(crate) res: Resources,
    pub(crate) images: objects::ImageTable,
    pub(crate) budget: Budget,
    pub(crate) line_seq: u32,
    /// `onField` over every pass of the call (the editor's `Map<Run, string>`: first-set order,
    /// last value; F8)
    pub(crate) fields: Option<IndexMap<crate::model::RunRef, crate::model::Utf16>>,
    /// the page checkpoints of an incremental relayout's recording call (LA7); `None` on the
    /// full path
    pub(crate) rec: Option<&'r mut crate::incr::Recorder>,
    /// the paragraph memo of an incremental relayout (LA7 level 1); `None` on the full path
    pub(crate) memo: Option<&'r mut crate::incr::ParaMemo>,
}

impl<'r, 'a> Call<'r, 'a> {
    /// A call with fresh tables (the full path).
    pub(crate) fn new(
        ldoc: &'r LDoc<'a>,
        opts: &'r LayoutOptions,
        provider: &'r dyn FontProvider,
        hyph: &'r mut HyphenSessions,
    ) -> Call<'r, 'a> {
        Call {
            ldoc,
            opts,
            provider,
            hyph,
            fonts: FaceTable::new(),
            resolver: FaceResolverCache::new(),
            res: Resources::new(),
            images: objects::ImageTable::new(),
            budget: Budget::new(&opts.limits, &work_input(ldoc)).with_cancel(opts.cancel.clone(), opts.cancel_poll),
            line_seq: 0,
            fields: opts.collect_fields.as_ref().map(|_| IndexMap::new()),
            rec: None,
            memo: None,
        }
    }

    /// The context of an engine method over this call's tables.
    pub(crate) fn cx<'c>(&'c mut self, input: &'c PassInput) -> Cx<'c, 'a> {
        Cx {
            ldoc: self.ldoc,
            input,
            opts: self.opts,
            fonts: &mut self.fonts,
            provider: self.provider,
            resolver: &mut self.resolver,
            res: &mut self.res,
            hyph: &mut *self.hyph,
            budget: &mut self.budget,
            depth: 0,
            line_seq: &mut self.line_seq,
            images: &mut self.images,
            memo: self.memo.as_deref_mut(),
        }
    }

    /// Moves what a pass reported through `onField` into the call's map (F8).
    pub(crate) fn take_fields(&mut self, eng: &mut Engine) {
        if let (Some(all), Some(pass)) = (self.fields.as_mut(), eng.fields.as_mut()) {
            for (k, v) in pass.drain(..) {
                all.insert(k, v);
            }
        }
    }

    /// The engine of a new pass (`new Engine(newCtx(), full, …)`).
    pub(crate) fn new_engine(&self, input: &PassInput) -> Engine {
        let mut ctx = DocContext::new(
            word::StyleResolver::new(),
            word::numbering_state(&self.opts.lang, &self.opts.limits),
            word::revision_display(self.opts.revisions.as_ref()),
        );
        ctx.note_mark_override.clone_from(&input.note_marks);
        let mut eng = Engine::new(ctx, crate::derive::BODY);
        if self.opts.collect_fields.is_some() {
            eng.fields = Some(IndexMap::new());
        }
        eng
    }

    /// `new Engine(newCtx(), full, …)` + `eng.run()` (recorded when the call records, LA7).
    fn pass(&mut self, input: &PassInput) -> LResult<Engine> {
        let mut eng = self.new_engine(input);
        let ran = match self.rec.take() {
            None => {
                let mut cx = self.cx(input);
                eng.run(&mut cx)
            }
            Some(rec) => {
                rec.begin_pass(&mut eng, input, self.line_seq);
                let r = {
                    let mut cx = self.cx(input);
                    eng.prologue(&mut cx).and_then(|()| eng.run_from(&mut cx, None, &mut *rec)).map(|_| ())
                };
                self.rec = Some(rec);
                r
            }
        };
        self.end_pass(&mut eng, ran)?;
        Ok(eng)
    }

    /// The end of a pass: the fields move to the call, the pass's metafile warnings to its
    /// context; a recording call closes the pass's record first.
    pub(crate) fn end_pass(&mut self, eng: &mut Engine, ran: LResult<()>) -> LResult<()> {
        if ran.is_ok()
            && let Some(rec) = self.rec.take()
        {
            let lens = crate::incr::CallLens::of_call(self);
            rec.end_pass(eng, lens);
            self.rec = Some(rec);
        }
        self.take_fields(eng);
        ran?;
        // the warnings of the pass's metafiles go to its context (F7b-1)
        for w in self.images.warnings.drain(..) {
            eng.ctx.warn(&w);
        }
        Ok(())
    }

    /// The pass loop's reading of a pass (engine.ts:224–251): whether the layout is stable and
    /// the input of the next pass. Shared by the full path and the incremental replay (LA7).
    pub(crate) fn next_input(&mut self, eng: &Engine, input: &PassInput) -> (bool, PassInput) {
        let (ldoc, opts) = (self.ldoc, self.opts);
        let new_chaps = eng.found.used_chapter.then(|| eng.chapter_index());
        let new_sect_pages = eng.found.used_section_pages.then(|| eng.section_page_counts());
        let f = &eng.found;
        let new_marks: Option<IndexMap<Box<str>, Box<[u16]>>> = if f.used_page_ref { Some(eng.bookmark_pages(ldoc, opts)) } else { None };
        let new_note_refs = f.used_note_ref.then(|| eng.ctx.all_note_marks());
        let new_late = if f.late_found.is_empty() { None } else { merge_late(&input.late_floats, &f.late_found) };
        let new_late_tables = (!f.late_tables_found.is_empty()).then(|| {
            let mut m = input.late_tables.clone();
            for (k, v) in &f.late_tables_found {
                m.insert(*k, *v);
            }
            m
        });
        let cx_fix = {
            let cx = self.cx(input);
            eng.note_mark_fix(&cx)
        };
        let total = f64::from(u32::try_from(eng.pages.len()).unwrap_or(u32::MAX));
        let stable = (!f.used_num_pages || js_eq(total, input.total_pages_guess))
            && new_sect_pages.as_ref().is_none_or(|n| same_counts(n, &input.section_pages_guess))
            && new_marks.as_ref().is_none_or(|n| same_map(n, &input.marks_guess))
            && new_note_refs.as_ref().is_none_or(|n| same_map(n, &input.note_ref_guess))
            && new_late.is_none()
            && new_late_tables.is_none()
            && cx_fix.is_none()
            && new_chaps.as_ref().is_none_or(|n| same_chapters(n, &input.chapters_guess));
        let mut next = input.clone();
        if stable {
            return (true, next);
        }
        if let Some(c) = new_chaps {
            next.chapters_guess = c;
        }
        if let Some(s) = new_sect_pages {
            next.section_pages_guess = s;
        }
        next.total_pages_guess = total;
        if let Some(m) = new_marks {
            next.marks_guess = m;
        }
        if let Some(n) = new_note_refs {
            next.note_ref_guess = n;
        }
        if let Some(l) = new_late {
            next.late_floats = l;
        }
        if let Some(l) = new_late_tables {
            next.late_tables = l;
        }
        if let Some(fix) = cx_fix {
            next.note_marks = Some(fix);
        }
        (false, next)
    }

    /// The fields of the call as the `Layout` reports them (the scope filter of F8).
    pub(crate) fn layout_fields(&mut self) -> Option<Vec<(crate::model::RunRef, crate::model::Utf16)>> {
        let opts = self.opts;
        self.fields.take().map(|m| {
            m.into_iter()
                .filter(|(r, _)| match &opts.collect_fields {
                    Some(crate::model::FieldScope::Runs(only)) => only.contains(r),
                    _ => true,
                })
                .collect()
        })
    }
}

/// `mergeLate(a, b)` (engine.ts:304): `None` when nothing new (the TS returns `a` itself).
fn merge_late(a: &IndexMap<u32, Vec<RunKey>>, b: &IndexMap<u32, Vec<RunKey>>) -> Option<IndexMap<u32, Vec<RunKey>>> {
    let mut seen: IndexSet<RunKey> = a.values().flatten().copied().collect();
    let mut out: Option<IndexMap<u32, Vec<RunKey>>> = None;
    for (pg, runs) in b {
        for r in runs {
            if seen.contains(r) {
                continue;
            }
            seen.insert(*r);
            out.get_or_insert_with(|| a.clone()).entry(*pg).or_default().push(*r);
        }
    }
    out
}

fn same_counts(a: &IndexMap<u32, f64>, b: &IndexMap<u32, f64>) -> bool {
    a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| js_eq(*v, *w)))
}

fn same_map(a: &IndexMap<Box<str>, Box<[u16]>>, b: &IndexMap<Box<str>, Box<[u16]>>) -> bool {
    a.len() == b.len() && a.iter().all(|(k, v)| b.get(k) == Some(v))
}

fn same_chapters(a: &ChapterIndex, b: &ChapterIndex) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, v)| {
            b.get(k).is_some_and(|w| w.len() == v.len() && v.iter().zip(w).all(|(x, y)| js_eq(x.page, y.page) && x.text == y.text))
        })
}

/// The work estimate of a call (K16): the UTF-16 units of every run, the sections with columns,
/// the table nesting, the tallest page.
pub(crate) fn work_input(ldoc: &LDoc<'_>) -> WorkInput {
    let glyphs = (0..ldoc.runs.len()).map(|k| ldoc.run_text(RunKey(u32::try_from(k).unwrap_or(u32::MAX))).len() as u64).sum::<u64>();
    let secs = ldoc.view.sections();
    let balanced = secs.iter().filter(|s| s.sect_pr.cols.as_ref().is_some_and(|c| c.count.to_number() > 1.0)).count();
    let nesting = ldoc.paras.iter().map(|p| p.path.depth()).max().unwrap_or(0);
    let max_page_h = secs.iter().map(|s| s.sect_pr.page_height_twips.to_number()).fold(16838.0, jm::max);
    WorkInput {
        glyphs,
        balanced_sections: u32::try_from(balanced).unwrap_or(u32::MAX),
        nesting: u32::try_from(nesting).unwrap_or(u32::MAX),
        max_page_h,
    }
}

/// The options a call lays out with: with revisions shown and no author order given, one colour
/// per author in the document's reading order (Word's encounter order).
pub(crate) fn effective_opts<'o>(ldoc: &LDoc<'_>, opts0: &'o LayoutOptions) -> std::borrow::Cow<'o, LayoutOptions> {
    match &opts0.revisions {
        Some(r) if r.mode != RevisionMode::Final && r.order.is_none() => {
            let mut o = opts0.clone();
            if let Some(r) = o.revisions.as_mut() {
                r.order = Some(ldoc.revision_order.iter().map(|s| Box::<str>::from(*s)).collect());
            }
            std::borrow::Cow::Owned(o)
        }
        _ => std::borrow::Cow::Borrowed(opts0),
    }
}

/// `layoutDocument(full, provider, opts)` (engine.ts:189): the pass loop (≤ 5 passes until
/// stable), then the balancing search per balanced section; the result of the last executed pass
/// (K19).
pub fn layout_document(ldoc: &LDoc<'_>, opts0: &LayoutOptions, provider: &dyn FontProvider, hyph: &mut HyphenSessions) -> LResult<Layout> {
    layout_document_with(ldoc, opts0, provider, hyph, None, None)
}

/// [`layout_document`], recording the page checkpoints of an incremental relayout when `rec` is
/// given and preparing the body paragraphs through the paragraph memo when `memo` is (LA7).
/// Without them the path is the full one, unchanged.
pub(crate) fn layout_document_with(
    ldoc: &LDoc<'_>,
    opts0: &LayoutOptions,
    provider: &dyn FontProvider,
    hyph: &mut HyphenSessions,
    rec: Option<&mut crate::incr::Recorder>,
    memo: Option<&mut crate::incr::ParaMemo>,
) -> LResult<Layout> {
    let opts = effective_opts(ldoc, opts0);
    let opts = &*opts;
    let mut call = Call::new(ldoc, opts, provider, hyph);
    call.memo = memo;
    if let Some(rec) = rec {
        call.resolver.log = Some(Vec::new());
        call.rec = Some(rec);
    }
    // NUMPAGES and PAGEREF depend on the layout itself: the first pass guesses, the next ones use
    // the total and the bookmark pages of the one before
    let mut input = PassInput { total_pages_guess: 1.0, ..PassInput::default() };
    let mut exec: Option<Engine> = None;
    let mut last_found: Option<IndexMap<i64, BalanceInfo>> = None;
    for _pass in 0..5 {
        let eng = call.pass(&input)?;
        let (stable, next) = call.next_input(&eng, &input);
        last_found = Some(eng.found.balance_found.clone());
        retire(&mut call, exec.replace(eng));
        if stable {
            break;
        }
        input = next;
    }
    // multi-column sections ended by a CONTINUOUS break: Word balances the columns of the
    // section's last page (oracles colbal, colbal3)
    let mut balance: IndexMap<i64, BalanceSpec> = IndexMap::new();
    let mut done: IndexSet<i64> = IndexSet::new();
    let mut guard = 0;
    while guard < 20 {
        guard += 1;
        let Some(found) = last_found.as_ref() else { break };
        let Some((&s, info)) = found.iter().find(|(s, i)| !done.contains(*s) && i.n > 1) else { break };
        let info = info.clone();
        done.insert(s);
        // the probes are full passes: a call that ran them keeps no checkpoints (LA7, D-LA7-5)
        if let Some(rec) = call.rec.as_deref_mut() {
            rec.discard(crate::incr::FallbackReason::Balancing);
        }
        // three columns or more (the note area aside): the 1st column reaches the AVERAGE of the
        // content; two: the least height with which everything fits and the 2nd column is not
        // longer than the 1st (oracles colbal, colbal3)
        let by_avg = s >= 0 && info.n >= 3;
        let n = f64::from(info.n);
        let avg = info.content.iter().fold(0.0, |a, b| a + b) / n;
        let mut lo = jm::max(0.0, jm::floor(if by_avg { avg } else { info.sum / n }) - 1.0);
        let mut hi = jm::ceil(info.bottom - info.col_top);
        if hi <= lo + 1.0 {
            continue;
        }
        let page = info.page;
        let ok = |h: f64, call: &mut Call<'_, '_>, exec: &mut Option<Engine>| -> LResult<bool> {
            let mut b = balance.clone();
            b.insert(s, BalanceSpec { page, h, last_free: by_avg });
            let inp = PassInput { balance: b, ..input.clone() };
            let eng = call.pass(&inp)?;
            let f = eng.found.balance_found.get(&s).cloned();
            *exec = Some(eng);
            let Some(f) = f else { return Ok(false) };
            if f.page != page {
                return Ok(false);
            }
            Ok(if by_avg {
                f.content.first().copied().unwrap_or(f64::NAN) >= avg - 0.5
            } else {
                f.used.last().copied().unwrap_or(f64::NAN) <= f.used.first().copied().unwrap_or(f64::NAN) + 1.0
            })
        };
        if !ok(hi, &mut call, &mut exec)? {
            continue;
        }
        while hi - lo > 1.0 {
            let mid = jm::floor((lo + hi) / 2.0);
            if ok(mid, &mut call, &mut exec)? {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        balance.insert(s, BalanceSpec { page, h: hi, last_free: by_avg });
        let inp = PassInput { balance: balance.clone(), ..input.clone() };
        let eng = call.pass(&inp)?;
        last_found = Some(eng.found.balance_found.clone());
        exec = Some(eng);
    }
    let mut eng = exec.ok_or(LayoutError::Internal("layoutDocument: no pass"))?;
    // a recorded call keeps the final pass's state as it was before the page assembly
    let pass_end_warnings = call.rec.is_some().then(|| eng.ctx.warnings.clone());
    let pages = {
        let mut cx = call.cx(&input);
        eng.finish(&mut cx)?
    };
    call.take_fields(&mut eng);
    let fields = call.layout_fields();
    let mut warnings = std::mem::take(&mut eng.ctx.warnings);
    // past the document's face budget a face resolved as a miss (LA6, F8): said once, last
    if provider.budget_refusals() > 0 {
        warnings.insert("font budget".into());
    }
    let stamp = LayoutStamp { formatting_version: ldoc.view.formatting_version() };
    let Some(rec) = call.rec.take() else {
        return Ok(Layout {
            pages,
            resources: call.res,
            image_origins: call.images.origins,
            warnings,
            missing_faces: call.resolver.take_misses(),
            fields,
            stamp,
        });
    };
    if let Some(w) = pass_end_warnings {
        eng.ctx.warnings = w;
    }
    rec.retire(eng);
    let layout = Layout {
        pages,
        resources: call.res,
        image_origins: call.images.origins.clone(),
        warnings,
        missing_faces: call.resolver.misses(),
        fields,
        stamp,
    };
    rec.finish_call(crate::incr::CallTables { fonts: call.fonts, resolver: call.resolver, images: call.images, line_seq: call.line_seq });
    Ok(layout)
}

/// A finished pass's engine goes to the recorder (it keeps every pass's pages, LA7).
fn retire(call: &mut Call<'_, '_>, old: Option<Engine>) {
    if let (Some(rec), Some(old)) = (call.rec.as_deref_mut(), old) {
        rec.retire(old);
    }
}
