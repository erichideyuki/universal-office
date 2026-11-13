//! The incremental relayout of the editor (plan LA7, milestone 2; record `uo-docx-layout.md`
//! "Stage M2 — incremental relayout (LA7)"). Criterion: the `Layout` of an incremental relayout
//! is the same, bit for bit, as a full layout of the same view.
//!
//! **Level 2 — page checkpoints.** A full relayout records, per pass, the engine state before
//! the first body block of every new page ([`record::Checkpoint`]), a trace of what each interval
//! read and wrote ([`trace::Trace`]), and keeps the pass's final pages and engine and the call's
//! tables. The next relayout, when nothing but body blocks changed in place (same options, same
//! provider answers, the other document parts the same objects, the same derived-document shape):
//! for each pass of the previous call, resumes at the latest checkpoint before the first changed
//! block that nothing before it made depend on the changed blocks and whose earlier pages nothing
//! after it wrote; restores the tables as prefixes; runs until a later checkpoint of the previous
//! run where the whole state (engine, open pages, tables, line counter) is equal and nothing after
//! it reads the changed blocks; splices the previous pages and final state after it; runs the
//! pass loop's reading on the result (the same code as the full path) and goes on with the next
//! pass when its input is the previous call's. The page assembly reuses the previous `LayoutPage`s
//! of the pages not laid out again (their resource-touching steps run again, in order) and
//! checks the final tables are the previous ones. Anything else is a full (recording) layout.
//!
//! **Level 1 — the paragraph memo** ([`memo`]): every relayout (full or resumed) prepares the
//! body paragraphs through a memo of `prepare_paragraph` keyed by the paragraph node, its
//! environment and everything it read (field values, numbering markers, note marks, the faces it
//! asked for), replaying the recorded flow effects on a hit. It is what makes a structure edit
//! (Enter, a merge, a paste) cheaper: those are full layouts for level 2. The line breaking is
//! not memoized (record: measured at 6 % of a full layout).

mod bits;
mod keys;
pub mod memo;
pub(crate) mod record;
mod relabel;
pub(crate) mod trace;

use std::ops::Range;

pub use memo::{MemoCaps, MemoReport, ParaMemo};
pub use record::{CallLens, CallTables, Recorder};
pub use trace::{Epoch, Trace};

use crate::engine::page::PageState;
use crate::engine::{BlockHook, Call, Control, Cx, Engine, effective_opts, layout_document_with};
use crate::error::{LResult, LayoutError};
use crate::fonts::{FontProvider, ProviderAsk};
use crate::hyphenate::HyphenSessions;
use crate::ldoc::LDoc;
use crate::model::StoryRef;
use crate::options::LayoutOptions;
use crate::output::{Layout, LayoutStamp};
use record::{Checkpoint, PassRec, restore, same_resources};
use relabel::{LineMap, Relabel, map_id, relabel_debug};

/// The memory caps of the incremental state (M5). The defaults are first numbers (record:
/// measured on the 50- and 500-page documents).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrConfig {
    /// `false`: every relayout is a full layout and nothing is kept
    pub enabled: bool,
    /// the estimated heap of the checkpoints plus the retained pages of the recorded passes;
    /// past it the checkpoints are kept every k pages, and past it with the pages alone nothing is
    /// kept
    pub checkpoint_bytes: usize,
    /// the paragraph memo (level 1) and its caps; off with `enabled: false` too
    pub memo: MemoCaps,
}

impl Default for IncrConfig {
    fn default() -> IncrConfig {
        #[cfg(target_arch = "wasm32")]
        let (checkpoint_bytes, memo_bytes) = (96 << 20, 64 << 20);
        #[cfg(not(target_arch = "wasm32"))]
        let (checkpoint_bytes, memo_bytes) = (512 << 20, 384 << 20);
        IncrConfig { enabled: true, checkpoint_bytes, memo: MemoCaps { enabled: true, entries: 1 << 20, bytes: memo_bytes } }
    }
}

impl IncrConfig {
    /// Wave 7 (B, decision D-L7): the phone font profile's caps — a third of the wasm32 default (32 MiB of checkpoints and
    /// retained pages, 16 MiB of paragraph memo). Past a cap the relayout keeps fewer checkpoints or memo entries and falls
    /// back to more full layouts: the pages stay bit-identical (la7 `m5_memo_caps`), only the typing latency of very long
    /// documents grows. The desktop profile keeps [`IncrConfig::default`].
    #[must_use]
    pub fn phone() -> IncrConfig {
        let d = IncrConfig::default();
        IncrConfig { checkpoint_bytes: d.checkpoint_bytes / 3, memo: MemoCaps { bytes: d.memo.bytes / 4, ..d.memo }, ..d }
    }
}

/// How a relayout was computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RelayoutPath {
    /// a full layout (recording the checkpoints of the next one)
    #[default]
    Full,
    /// resumed and spliced on the previous run
    Incremental,
}

/// Why a relayout was a full one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    /// no previous run (the first layout, after `clear`, after an error)
    NoBase,
    /// [`IncrConfig::enabled`] is off
    Disabled,
    /// a view other than Print Layout
    View,
    /// other options than the previous call's (or hyphenation skips)
    Options,
    /// the provider answers a face differently, or its budget refusals changed
    Provider,
    /// a document part besides the body changed (sections, styles, notes, headers, bookmarks…)
    Document,
    /// the view edits a context story (cell, note, header/footer, textbox)
    Context,
    /// the document has linked textbox chains
    Chains,
    /// paragraphs, runs or tables were added or removed (the derived document's keys moved)
    Shape,
    /// the section geometries, the cover page or the first page number changed
    Prologue,
    /// no checkpoint before the change is usable
    NoCheckpoint,
    /// a pass's input differs from the previous call's
    PassInput,
    /// the pass loop stops at another pass than the previous call's
    Unstable,
    /// the call's tables differ from the previous call's where pages are reused
    Tables,
    /// column balancing ran (no checkpoints are kept, D-LA7-5)
    Balancing,
    /// past [`IncrConfig::checkpoint_bytes`] with the pages alone
    Memory,
    /// the incremental attempt itself failed (an internal check of the splice or the restore, or
    /// a limit of its own budget): the state was dropped and the call laid out in full (D-LA7-15)
    Failed,
    /// the previous result was lent ([`crate::LayoutEngine::lend_last`]) and not given back
    /// (wave 6, L; appended last: its wire code in uo-wasm is 17)
    Lent,
}

/// A lent result of [`crate::LayoutEngine::lend_last`] (wave 6, L): what
/// [`crate::LayoutEngine::give_back`] checks before taking the `Layout` back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LendTicket {
    /// unique over every engine of the process
    id: u64,
    /// the page count of the lent `Layout`
    pages: usize,
    /// where it came from: the kept run (`Base`) or the last result without one
    in_base: bool,
}

static NEXT_LEND: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// What the last relayout did (for the shell: repaint `relaid_pages`, or every page when the
/// path is `Full`).
#[derive(Clone, Debug, Default)]
pub struct RelayoutReport {
    pub path: RelayoutPath,
    /// why the path is `Full`
    pub reason: Option<FallbackReason>,
    /// passes run (or replayed)
    pub passes: u32,
    /// the body blocks the edit replaced
    pub changed_blocks: Option<(u32, u32)>,
    /// the page the final pass resumed at, and the page where it met the previous run
    pub resumed_page: Option<u32>,
    pub converged_page: Option<u32>,
    /// the pages of the result that were laid out again (`0..pages` for a full layout)
    pub relaid_pages: Range<u32>,
    /// the pages of `relaid_pages` whose `LayoutPage` differs from the previous result's page of the
    /// same index (wave 6, L; every page of `relaid_pages` on a full layout): a page outside it shows
    /// the same items, lines and geometry as before (`Debug`-equal, the LA7 equality)
    pub changed_pages: Vec<u32>,
    /// checkpoints kept for the next relayout, and their spacing in pages
    pub checkpoints: usize,
    pub spacing: usize,
    /// estimated heap bytes kept for the next relayout (checkpoints; pages of the recorded passes,
    /// besides the `Layout` itself)
    pub checkpoint_bytes: usize,
    pub retained_page_bytes: usize,
    /// how far the line ids after the edit moved (lines added or removed)
    pub line_shift: i64,
    /// estimated heap bytes of the result `Layout`'s pages (the ratio of the above)
    pub layout_bytes: usize,
    /// what the paragraph memo (level 1) did, and what it keeps
    pub memo: MemoReport,
}

/// The previous run.
struct Base {
    layout: Layout,
    passes: Vec<PassRec>,
    tables: CallTables,
    parts: keys::DocParts,
    shape: Vec<u64>,
    options: String,
    prologue: String,
    refusals: bool,
    spacing: usize,
}

/// The incremental state of a [`crate::LayoutEngine`].
pub struct IncrState {
    pub(crate) cfg: IncrConfig,
    base: Option<Box<Base>>,
    /// the last relayout's result when no base is kept
    layout: Option<Layout>,
    report: Option<RelayoutReport>,
    /// why the last full relayout kept no checkpoints (the next one says it again)
    no_base: Option<FallbackReason>,
    /// the paragraph memo (level 1)
    memo: ParaMemo,
    /// the result is lent out (wave 6, L): `layout()` answers `None` and the next relayout without a
    /// give-back is a full one (reason `Lent`)
    lent: Option<LendTicket>,
    /// the result was given back since the last relayout
    given_back: bool,
    /// the given-back result when the relayout that used it failed before consuming it (the shell
    /// lends it to the previous handle again)
    spare: Option<Layout>,
}

impl Default for IncrState {
    fn default() -> IncrState {
        IncrState::new(IncrConfig::default())
    }
}

impl IncrState {
    pub(crate) fn new(cfg: IncrConfig) -> IncrState {
        let mut caps = cfg.memo;
        caps.enabled &= cfg.enabled;
        IncrState {
            cfg,
            base: None,
            layout: None,
            report: None,
            no_base: None,
            memo: ParaMemo::new(caps),
            lent: None,
            given_back: false,
            spare: None,
        }
    }

    /// The last relayout's result (`None` while it is lent).
    #[must_use]
    pub fn layout(&self) -> Option<&Layout> {
        if self.lent.is_some() {
            return None;
        }
        self.base.as_deref().map(|b| &b.layout).or(self.layout.as_ref())
    }

    /// Lends the last relayout's result out (moved, not copied; wave 6, L): `None` when there is
    /// none or it is lent already.
    pub(crate) fn lend(&mut self) -> Option<(LendTicket, Layout)> {
        if self.lent.is_some() {
            return None;
        }
        let (layout, in_base) = match (self.base.as_deref_mut(), self.layout.take()) {
            (Some(b), None) => (std::mem::take(&mut b.layout), true),
            (None, Some(l)) => (l, false),
            (Some(_), Some(l)) => {
                // never both (`set` keeps one); answer none rather than guess
                self.layout = Some(l);
                return None;
            }
            (None, None) => return None,
        };
        let t = LendTicket { id: NEXT_LEND.fetch_add(1, std::sync::atomic::Ordering::Relaxed), pages: layout.pages.len(), in_base };
        self.lent = Some(t);
        Some((t, layout))
    }

    /// The lend in course, if any.
    pub(crate) fn lent(&self) -> Option<LendTicket> {
        self.lent
    }

    /// Takes a lent result back; `Err` (the layout returned) when `t` is not the lend in course or
    /// the page count differs.
    pub(crate) fn give_back(&mut self, t: LendTicket, layout: Layout) -> Result<(), Box<Layout>> {
        if self.lent != Some(t) || layout.pages.len() != t.pages {
            return Err(Box::new(layout));
        }
        if t.in_base {
            match self.base.as_deref_mut() {
                Some(b) => b.layout = layout,
                None => return Err(Box::new(layout)),
            }
        } else {
            self.layout = Some(layout);
        }
        self.lent = None;
        self.given_back = true;
        Ok(())
    }

    /// The given-back result a failed relayout did not consume.
    pub(crate) fn take_spare(&mut self) -> Option<Layout> {
        self.spare.take()
    }

    /// A relayout failed outside [`relayout`] (the derived document): drops the state, keeping a
    /// given-back result aside.
    pub(crate) fn fail(&mut self) {
        if std::mem::take(&mut self.given_back) {
            self.spare = self.base.take().map(|b| b.layout).or_else(|| self.layout.take());
        }
        let spare = self.spare.take();
        self.clear();
        self.spare = spare;
    }

    #[must_use]
    pub fn report(&self) -> Option<&RelayoutReport> {
        self.report.as_ref()
    }

    /// Drops everything kept (M1), the paragraph memo included.
    pub fn clear(&mut self) {
        self.drop_base();
        self.memo.clear();
        self.spare = None;
    }

    /// Drops the previous run (level 2) and the last result; the paragraph memo stays. A lend in
    /// course ends (its give-back is refused: the borrower keeps the layout).
    fn drop_base(&mut self) {
        self.base = None;
        self.layout = None;
        self.report = None;
        self.no_base = None;
        self.lent = None;
        self.given_back = false;
    }

    /// A full relayout outside this state (another view).
    pub(crate) fn full_report(&mut self, why: FallbackReason) {
        self.report = Some(RelayoutReport { reason: Some(why), ..RelayoutReport::default() });
    }

    fn set(&mut self, mut base: Box<Base>, mut report: RelayoutReport) {
        // the cap (M5) again: a resumed run recorded a checkpoint per page
        match record::fit_cap(&mut base.passes, self.cfg.checkpoint_bytes, base.spacing) {
            Some(k) => base.spacing = k,
            None => {
                self.base = None;
                self.layout = Some(base.layout);
                self.no_base = Some(FallbackReason::Memory);
                self.report = Some(report);
                return;
            }
        }
        let (cp, pg) = base.passes.iter().map(PassRec::bytes).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        report.checkpoints = base.passes.iter().map(|p| p.cps.len()).sum();
        report.spacing = base.spacing;
        report.checkpoint_bytes = cp;
        report.retained_page_bytes = pg;
        report.layout_bytes = base.layout.pages.iter().map(record::layout_page_bytes).sum();
        self.layout = None;
        self.base = Some(base);
        self.report = Some(report);
    }
}

/// `LayoutEngine::relayout` over a derived document (Print Layout).
pub(crate) fn relayout(
    st: &mut IncrState,
    ldoc: &LDoc<'_>,
    provider: &dyn FontProvider,
    o: &LayoutOptions,
    hyph: &mut HyphenSessions,
) -> LResult<()> {
    if st.lent.take().is_some() {
        // the previous result is out and was not given back: the kept run has no pages (wave 6, L)
        st.base = None;
        st.layout = None;
        st.no_base = Some(FallbackReason::Lent);
    }
    let r = relayout_in(st, ldoc, provider, o, hyph);
    st.given_back = false;
    match &r {
        Ok(()) => {
            st.spare = None;
            if let Some(rep) = st.report.as_mut()
                && rep.path == RelayoutPath::Full
            {
                rep.changed_pages = rep.relaid_pages.clone().collect();
            }
        }
        Err(_) => {
            // on any error nothing is kept, the paragraph memo included (decision 7); a given-back
            // result the call did not consume stays aside for the borrower
            let spare = st.spare.take();
            st.clear();
            st.spare = spare;
        }
    }
    r
}

fn relayout_in(
    st: &mut IncrState,
    ldoc: &LDoc<'_>,
    provider: &dyn FontProvider,
    o: &LayoutOptions,
    hyph: &mut HyphenSessions,
) -> LResult<()> {
    let mut report = RelayoutReport::default();
    let given = st.given_back;
    let attempt = match st.base.take() {
        Some(mut b) if st.cfg.enabled => {
            st.memo.begin_call(ldoc, &effective_opts(ldoc, o));
            // the previous result apart from the kept run: when the attempt falls back before the page
            // assembly takes its pages, a given-back result goes back to its borrower on an error
            let mut prev = std::mem::take(&mut b.layout);
            let r = incremental(b, &mut prev, ldoc, provider, o, hyph, &mut st.memo, &mut report);
            if given && !prev.pages.is_empty() {
                st.spare = Some(prev);
            }
            r
        }
        Some(_) => Ok(Err(FallbackReason::Disabled)),
        None => Ok(Err(if st.cfg.enabled { st.no_base.unwrap_or(FallbackReason::NoBase) } else { FallbackReason::Disabled })),
    };
    match attempt {
        Ok(Ok(base)) => {
            report.path = RelayoutPath::Incremental;
            report.memo = st.memo.end_call(false);
            st.set(base, report);
            Ok(())
        }
        Ok(Err(why)) => {
            report = RelayoutReport { reason: Some(why), ..RelayoutReport::default() };
            full(st, ldoc, provider, o, hyph, report)
        }
        // a cancellation stands (the caller asked to stop; a full layout would only be cancelled
        // again)
        Err(LayoutError::Cancelled) => {
            st.clear();
            Err(LayoutError::Cancelled)
        }
        // any other failure of the attempt: drop the state and lay out in full; the call fails only
        // when the full layout fails too (D-LA7-15)
        Err(_) => {
            st.clear();
            full(st, ldoc, provider, o, hyph, RelayoutReport { reason: Some(FallbackReason::Failed), ..RelayoutReport::default() })
        }
    }
}

/// A full layout, recording the checkpoints of the next relayout (when enabled).
fn full(
    st: &mut IncrState,
    ldoc: &LDoc<'_>,
    provider: &dyn FontProvider,
    o: &LayoutOptions,
    hyph: &mut HyphenSessions,
    mut report: RelayoutReport,
) -> LResult<()> {
    st.drop_base();
    if !st.cfg.enabled {
        let layout = crate::engine::layout_document(ldoc, o, provider, hyph)?;
        report.relaid_pages = 0..u32::try_from(layout.pages.len()).unwrap_or(u32::MAX);
        st.layout = Some(layout);
        st.report = Some(report);
        return Ok(());
    }
    if ldoc.view.story(&StoryRef::Context).is_some() {
        // a view with a context story keeps no base (D-LA7-16): the next relayout outside the
        // context would otherwise depend on the session replacing the body or part `Arc`s when the
        // context's content is written back (the paragraph memo is used: its keys are the body
        // paragraph nodes)
        st.memo.begin_call(ldoc, &effective_opts(ldoc, o));
        let layout = match layout_document_with(ldoc, o, provider, hyph, None, Some(&mut st.memo)) {
            Err(e) if memo::failed(&e) => {
                // a check of the memo failed (never seen): the same call without it (D-LA7-21)
                st.memo.clear();
                crate::engine::layout_document(ldoc, o, provider, hyph)?
            }
            r => r?,
        };
        report.memo = st.memo.end_call(true);
        report.relaid_pages = 0..u32::try_from(layout.pages.len()).unwrap_or(u32::MAX);
        if report.reason.is_none() {
            report.reason = Some(FallbackReason::Context);
        }
        st.no_base = Some(FallbackReason::Context);
        st.layout = Some(layout);
        st.report = Some(report);
        return Ok(());
    }
    let mut rec = Recorder::new(st.cfg);
    st.memo.begin_call(ldoc, &effective_opts(ldoc, o));
    let layout = match layout_document_with(ldoc, o, provider, hyph, Some(&mut rec), Some(&mut st.memo)) {
        Err(e) if memo::failed(&e) => {
            // a check of the memo failed (never seen): the same call without it (D-LA7-21)
            st.memo.clear();
            rec = Recorder::new(st.cfg);
            layout_document_with(ldoc, o, provider, hyph, Some(&mut rec), None)?
        }
        r => r?,
    };
    report.memo = st.memo.end_call(true);
    report.passes = u32::try_from(rec.passes.len()).unwrap_or(u32::MAX);
    report.relaid_pages = 0..u32::try_from(layout.pages.len()).unwrap_or(u32::MAX);
    let prologue = rec.passes.first().and_then(|p| p.eng.as_deref()).map(bits::prologue_string);
    match (rec.discarded, rec.tables.take(), keys::options_key(o), prologue) {
        (None, Some(tables), Some(options), Some(prologue)) => {
            let base = Base {
                layout,
                passes: std::mem::take(&mut rec.passes),
                tables,
                parts: keys::DocParts::of(ldoc.view.document()),
                shape: keys::shape(ldoc),
                options,
                prologue,
                refusals: provider.budget_refusals() > 0,
                spacing: rec.spacing,
            };
            st.set(Box::new(base), report);
        }
        (why, ..) => {
            let why = why.unwrap_or(FallbackReason::Options);
            if report.reason.is_none() {
                report.reason = Some(why);
            }
            st.no_base = Some(why);
            st.layout = Some(layout);
            st.report = Some(report);
        }
    }
    Ok(())
}

/// The incremental relayout on `base`: `Ok(Err(reason))` when it cannot be done (the caller lays
/// out in full; `base` is consumed either way).
#[expect(clippy::too_many_arguments, reason = "the previous result travels apart from the kept run (wave 6, L)")]
fn incremental(
    mut base: Box<Base>,
    prev: &mut Layout,
    ldoc: &LDoc<'_>,
    provider: &dyn FontProvider,
    o: &LayoutOptions,
    hyph: &mut HyphenSessions,
    memo: &mut ParaMemo,
    report: &mut RelayoutReport,
) -> LResult<Result<Box<Base>, FallbackReason>> {
    use FallbackReason as F;
    // ── what must not have changed ──
    if keys::options_key(o).as_deref() != Some(base.options.as_str()) {
        return Ok(Err(F::Options));
    }
    if ldoc.view.story(&StoryRef::Context).is_some() {
        return Ok(Err(F::Context));
    }
    if !ldoc.chains.is_empty() {
        return Ok(Err(F::Chains));
    }
    let doc = ldoc.view.document();
    if !base.parts.same_parts(doc) {
        return Ok(Err(F::Document));
    }
    if (provider.budget_refusals() > 0) != base.refusals
        || !keys::provider_same(provider, base.tables.resolver.log.as_deref().unwrap_or(&[]))
    {
        return Ok(Err(F::Provider));
    }
    let Some(range) = base.parts.changed_blocks(&doc.body) else { return Ok(Err(F::Shape)) };
    // wave 7 (B): the bookmark index rebuilt from the marks moves with the edited blocks only
    if !base.parts.bookmarks_follow(doc, range) {
        return Ok(Err(F::Document));
    }
    if keys::shape(ldoc) != base.shape {
        return Ok(Err(F::Shape));
    }
    report.passes = u32::try_from(base.passes.len()).unwrap_or(u32::MAX);
    let Some((b0, b1)) = range else {
        // nothing the layout reads changed: the previous layout is this one
        base.parts = keys::DocParts::of(doc);
        base.layout = std::mem::take(prev);
        report.relaid_pages = 0..0;
        return Ok(Ok(base));
    };
    report.changed_blocks = Some((u32::try_from(b0).unwrap_or(u32::MAX), u32::try_from(b1).unwrap_or(u32::MAX)));
    let opts = effective_opts(ldoc, o);
    let input0 = base.passes.first().map(|p| p.input.clone()).unwrap_or_default();
    {
        let mut call = Call::new(ldoc, &opts, provider, hyph);
        let mut probe = call.new_engine(&input0);
        probe.prologue(&mut call.cx(&input0))?;
        if bits::prologue_string(&probe) != base.prologue {
            return Ok(Err(F::Prologue));
        }
    }

    // ── the passes, replayed ──
    let mut call = Call::new(ldoc, &opts, provider, hyph);
    call.memo = Some(memo);
    let m = base.passes.len();
    let mut input = input0;
    let mut new_passes: Vec<PassRec> = Vec::with_capacity(m);
    let mut asks: Vec<ProviderAsk> = Vec::new();
    let mut old = std::mem::take(&mut base.passes).into_iter();
    let mut i = 0;
    // how far the line ids of this pass moved: the lines the edit added (or removed) in the
    // passes before
    let mut shift: i64 = 0;
    let (mut fin, final_rec, resume_lens, prefix_end, tail_from, prefix_maps, tail_maps) = loop {
        let Some(mut pr) = old.next() else { return Ok(Err(F::Unstable)) };
        if i > 0 && format!("{:?}", input) != format!("{:?}", pr.input) {
            return Ok(Err(F::PassInput));
        }
        let suffix = pr.suffixes();
        let prefix = pr.prefixes();
        let usable = |j: usize| -> bool {
            let (Some(c), Some(p), Some(s)) = (pr.cps.get(j), prefix.get(j), suffix.get(j)) else { return false };
            c.b as usize <= b0 && p.reads.is_none_or(|(_, hi)| (hi as usize) < b0) && s.min_touch >= c.cur
        };
        let Some(c_ix) = (0..pr.cps.len()).rev().find(|&j| usable(j)) else { return Ok(Err(F::NoCheckpoint)) };
        let Some(c) = pr.cps.get(c_ix) else { return Ok(Err(F::NoCheckpoint)) };
        let (c_gi, c_b, c_cur, c_lens) = (c.gi, c.b as usize, c.cur as usize, c.lens);
        // the previous run's ids of this pass in the new numbering, up to the resume point
        let start_map = [LineMap { from: pr.line_start, by: shift }];
        let c_line = map_id(&start_map, c_lens.line_seq);
        // the tables and the engine at the checkpoint
        if let Some(l) = call.resolver.log.take() {
            asks.extend(l);
        }
        let (fonts, resolver, res, mut images) = restore(&base.tables, &prev.resources, &c.lens)?;
        images.warnings.clone_from(&c.img_warnings);
        (call.fonts, call.resolver, call.res, call.images, call.line_seq) = (fonts, resolver, res, images, c_line);
        let mut eng: Engine = (*c.eng).clone();
        let mut pages = std::mem::take(&mut pr.pages);
        if c_cur > pages.len() {
            return Ok(Err(F::NoCheckpoint));
        }
        let mut tail: Vec<PageState> = pages.split_off(c_cur);
        eng.pages = pages;
        eng.pages.extend(c.open.iter().cloned());
        if shift != 0 {
            eng.relabel(&start_map);
        }
        eng.trace = Some(Box::new(Trace::new()));
        let mut hook = Converge {
            cps: &pr.cps,
            suffix: &suffix,
            from: c_ix,
            b1,
            last_pages: eng.pages.len(),
            first: Epoch::default(),
            new_cps: Vec::new(),
            found: None,
            tables: &base.tables,
            res: &prev.resources,
            c_lens,
            line_start: pr.line_start,
            shift,
            d: 0,
        };
        let stopped = {
            let mut cx = call.cx(&input);
            eng.run_from(&mut cx, Some((c_gi, c_b)), &mut hook)?
        };
        let (first, new_cps, found, d) = (hook.first, hook.new_cps, hook.found, hook.d);
        let mut last_epoch = eng.trace.as_deref_mut().map(Trace::take_epoch).unwrap_or_default();
        let lowest = eng.trace.take().map_or(u32::MAX, |t| t.lowest_touch) as usize;
        let mut cps = std::mem::take(&mut pr.cps);
        let mut rest = match found.filter(|_| stopped) {
            Some(k_ix) => cps.split_off(k_ix),
            None => Vec::new(),
        };
        cps.truncate(c_ix + 1);
        if shift != 0 {
            for cp in &mut cps {
                relabel_checkpoint(cp, &start_map);
            }
        }
        // the intervals of the new run replace the one after the checkpoint
        let mut segs = new_cps;
        match segs.last_mut() {
            Some(l) => l.after = std::mem::take(&mut last_epoch),
            None => last_epoch = first.merge(last_epoch),
        }
        if let Some(c) = cps.last_mut() {
            c.after = if segs.is_empty() { last_epoch } else { first };
        }
        cps.extend(segs);
        report.resumed_page = Some(u32::try_from(c_cur).unwrap_or(u32::MAX));
        // the previous run's ids after the meeting point: moved by the lines this pass added too
        let tail_maps = [LineMap { from: pr.line_start, by: shift }, LineMap { from: c_line, by: d }];
        let line_start = map_id(&start_map, pr.line_start);
        let (f, rec, pass_tail, next_shift) = match (stopped, rest.first().map(|k| k.cur as usize)) {
            (true, Some(k_cur)) => {
                // splice: the new pages before the meeting checkpoint, the previous ones after it
                eng.pages.truncate(k_cur);
                let from = k_cur.checked_sub(c_cur).ok_or(LayoutError::Internal("incr: splice"))?;
                if from > tail.len() {
                    return Err(LayoutError::Internal("incr: splice tail"));
                }
                let mut moved: Vec<PageState> = tail.drain(from..).collect();
                let mut f = *pr.eng.take().ok_or(LayoutError::Internal("incr: previous engine"))?;
                if tail_maps.iter().any(|m| !m.is_identity()) {
                    moved.relabel(&tail_maps);
                    f.relabel(&tail_maps);
                    for cp in &mut rest {
                        relabel_checkpoint(cp, &tail_maps);
                    }
                }
                eng.pages.extend(moved);
                f.pages = std::mem::take(&mut eng.pages);
                cps.extend(rest);
                if let Some(l) = call.resolver.log.take() {
                    asks.extend(l);
                }
                let mut end = pr.end;
                end.line_seq = map_id(&tail_maps, end.line_seq);
                let (fonts, resolver, res, images) = restore(&base.tables, &prev.resources, &pr.end)?;
                (call.fonts, call.resolver, call.res, call.images, call.line_seq) = (fonts, resolver, res, images, end.line_seq);
                call.images.warnings.clear();
                if let (Some(all), Some(pf)) = (call.fields.as_mut(), pr.fields.as_ref()) {
                    for (k, v) in pf {
                        all.insert(k.clone(), v.clone());
                    }
                }
                report.converged_page = Some(u32::try_from(k_cur).unwrap_or(u32::MAX));
                let next_shift = i64::from(end.line_seq) - i64::from(pr.end.line_seq);
                let rec = PassRec { input: pr.input, line_start, pre: pr.pre, cps, pages: Vec::new(), eng: None, fields: pr.fields, end };
                (f, rec, Some(k_cur), next_shift)
            }
            _ => {
                // ran to the end of the pass
                drop(tail);
                report.converged_page = None;
                let fields = eng.fields.clone();
                let end = CallLens::of_call(&call);
                call.end_pass(&mut eng, Ok(()))?;
                if i + 1 < m && !same_tables_at(&call, &base.tables, &prev.resources, &pr.end, &c_lens) {
                    return Ok(Err(F::Tables));
                }
                let next_shift = i64::from(end.line_seq) - i64::from(pr.end.line_seq);
                let rec = PassRec { input: pr.input, line_start, pre: pr.pre, cps, pages: Vec::new(), eng: None, fields, end };
                (eng, rec, None, next_shift)
            }
        };
        let end_page = pass_tail.unwrap_or(f.pages.len());
        // from the lowest page the resumed run wrote (a float pinned on an earlier page is
        // assembled again too)
        report.relaid_pages = u32::try_from(lowest.min(c_cur)).unwrap_or(u32::MAX)..u32::try_from(end_page).unwrap_or(u32::MAX);
        report.line_shift = next_shift;
        if f.found.balance_found.values().any(|b| b.n > 1) {
            return Ok(Err(F::Balancing));
        }
        let (stable, next) = call.next_input(&f, &input);
        if stable != (i + 1 == m) {
            return Ok(Err(F::Unstable));
        }
        if stable {
            break (f, rec, c_lens, lowest.min(c_cur), pass_tail, start_map, tail_maps);
        }
        let mut rec = rec;
        let mut f = f;
        rec.pages = std::mem::take(&mut f.pages);
        rec.eng = Some(Box::new(f));
        new_passes.push(rec);
        input = next;
        shift = next_shift;
        i += 1;
    };

    // ── the page assembly, reusing the pages not laid out again ──
    let mut old_pages: Vec<Option<crate::output::LayoutPage>> = std::mem::take(&mut prev.pages).into_iter().map(Some).collect();
    let pass_end_warnings = fin.ctx.warnings.clone();
    let pages = std::mem::take(&mut fin.pages);
    let mut out = Vec::with_capacity(pages.len());
    {
        let mut cx = call.cx(&input);
        let env = fin.finish_prep(&mut cx)?;
        for (ix, ps) in pages.iter().enumerate() {
            let maps: &[LineMap] = if ix < prefix_end { &prefix_maps } else { &tail_maps };
            let reuse = if ix < prefix_end || tail_from.is_some_and(|t| ix >= t) {
                old_pages.get_mut(ix).and_then(Option::take).map(|mut lp| {
                    if maps.iter().any(|m| !m.is_identity()) {
                        lp.relabel(maps);
                    }
                    lp
                })
            } else {
                None
            };
            out.push(fin.finish_page(&mut cx, ps, &env, reuse)?);
        }
    }
    // wave 6 (L): which pages laid out again differ from the previous result's page (the shell repaints
    // only those); the LA7 equality, `Debug` (the bulk columns by bits: `bits::same_page`; a page not compared counts as changed)
    report.changed_pages = report
        .relaid_pages
        .clone()
        .filter(|&ix| match (out.get(ix as usize), old_pages.get(ix as usize).and_then(Option::as_ref)) {
            (Some(new), Some(old)) => !bits::same_page(new, old),
            _ => true,
        })
        .collect();
    fin.pages = pages;
    if !same_tables_final(&call, &base.tables, &prev.resources, &resume_lens) {
        return Ok(Err(F::Tables));
    }
    call.take_fields(&mut fin);
    let fields = call.layout_fields();
    let mut warnings = std::mem::take(&mut fin.ctx.warnings);
    if provider.budget_refusals() > 0 {
        warnings.insert("font budget".into());
    }
    fin.ctx.warnings = pass_end_warnings;
    let layout = Layout {
        pages: out,
        resources: std::mem::take(&mut call.res),
        image_origins: call.images.origins.clone(),
        warnings,
        missing_faces: call.resolver.misses(),
        fields,
        stamp: LayoutStamp { formatting_version: ldoc.view.formatting_version() },
    };
    let mut final_rec = final_rec;
    final_rec.pages = std::mem::take(&mut fin.pages);
    final_rec.eng = Some(Box::new(fin));
    new_passes.push(final_rec);
    // the provider requests of both calls (the next relayout asks them all again)
    let mut log = base.tables.resolver.log.take().unwrap_or_default();
    asks.extend(call.resolver.log.take().unwrap_or_default());
    for a in asks {
        if !log.contains(&a) {
            log.push(a);
        }
    }
    let mut resolver = std::mem::take(&mut call.resolver);
    resolver.log = Some(log);
    let tables =
        CallTables { fonts: std::mem::take(&mut call.fonts), resolver, images: std::mem::take(&mut call.images), line_seq: call.line_seq };
    Ok(Ok(Box::new(Base {
        layout,
        passes: new_passes,
        tables,
        parts: keys::DocParts::of(doc),
        shape: base.shape,
        options: base.options,
        prologue: base.prologue,
        refusals: base.refusals,
        spacing: base.spacing,
    })))
}

/// A kept checkpoint in the new numbering of the line ids.
fn relabel_checkpoint(cp: &mut Checkpoint, maps: &[LineMap]) {
    cp.eng.relabel(maps);
    cp.open.relabel(maps);
    cp.lens.line_seq = map_id(maps, cp.lens.line_seq);
}

/// The tables of the running call equal the previous call's at `at` (lengths, line counter
/// included, and the entries from `from` on).
fn same_tables_at(call: &Call<'_, '_>, t: &CallTables, res: &uo_render::Resources, at: &CallLens, from: &CallLens) -> bool {
    CallLens { img_warnings: 0, line_seq: 0, ..CallLens::of_call(call) } == CallLens { img_warnings: 0, line_seq: 0, ..*at }
        && same_prefix_tables(&call.res, &call.fonts, &call.resolver, &call.images, t, res, at, from)
}

/// The final tables equal the previous call's (the line counter aside: it is not output, and the
/// reused pages only hold ids below the resume point).
fn same_tables_final(call: &Call<'_, '_>, t: &CallTables, res: &uo_render::Resources, from: &CallLens) -> bool {
    let have = CallLens::of_call(call);
    let want = CallLens::of(res, &t.resolver, &t.images, have.line_seq);
    have == CallLens { img_warnings: have.img_warnings, ..want }
        && same_prefix_tables(&call.res, &call.fonts, &call.resolver, &call.images, t, res, &want, from)
}

#[expect(clippy::too_many_arguments, reason = "the four tables of a call against the previous call's")]
fn same_prefix_tables(
    res: &uo_render::Resources,
    fonts: &crate::fonts::FaceTable,
    resolver: &crate::fonts::FaceResolverCache,
    images: &crate::engine::objects::ImageTable,
    t: &CallTables,
    base_res: &uo_render::Resources,
    at: &CallLens,
    from: &CallLens,
) -> bool {
    // the previous tables cut at `at` (they may hold later passes' entries)
    let Ok((bf, br, bres, bi)) = restore(t, base_res, at) else { return false };
    same_resources(res, &bres, from) && fonts.same_as(&bf) && resolver.same_from(&br, from.resolved, from.misses) && images.same_as(&bi)
}

/// The block hook of a resumed pass: records new checkpoints, and stops at the first checkpoint
/// of the previous run (after the changed blocks) whose whole state the new run has.
struct Converge<'b> {
    cps: &'b [Checkpoint],
    suffix: &'b [Epoch],
    /// the checkpoint the pass resumed at
    from: usize,
    /// the last changed block
    b1: usize,
    last_pages: usize,
    /// the interval from the resume to the first new checkpoint
    first: Epoch,
    new_cps: Vec<Checkpoint>,
    found: Option<usize>,
    tables: &'b CallTables,
    res: &'b uo_render::Resources,
    c_lens: CallLens,
    /// the previous run's line counter at the start of the pass, and how far this pass's ids
    /// moved before the resume point
    line_start: u32,
    shift: i64,
    /// at the meeting point: how far the lines of the new run moved the ids after the resume
    d: i64,
}

impl Converge<'_> {
    fn meets(&mut self, eng: &Engine, cx: &Cx<'_, '_>, k: &Checkpoint, after: &Epoch) -> LResult<bool> {
        // sealed after it, and nothing after it reads a block up to the last changed one
        if after.min_touch < k.cur || after.reads.is_some_and(|(lo, _)| lo as usize <= self.b1) {
            return Ok(false);
        }
        if eng.cur_ix()?.0 != k.cur || eng.pages.len() != k.pages_len as usize {
            return Ok(false);
        }
        let have = CallLens::of_cx(cx);
        if (CallLens { line_seq: k.lens.line_seq, ..have }) != k.lens || cx.images.warnings != k.img_warnings {
            return Ok(false);
        }
        if !same_prefix_tables(cx.res, cx.fonts, cx.resolver, cx.images, self.tables, self.res, &k.lens, &self.c_lens) {
            return Ok(false);
        }
        // the previous run's ids in the new numbering: moved by the passes before, and past the
        // resume point by the lines the new run added
        let start = LineMap { from: self.line_start, by: self.shift };
        let d = i64::from(have.line_seq) - i64::from(start.apply(k.lens.line_seq));
        let maps = [start, LineMap { from: start.apply(self.c_lens.line_seq), by: d }];
        if bits::state_string(eng) != relabel_debug(&bits::state_string(&k.eng), &maps) {
            return Ok(false);
        }
        let open = eng.pages.get(k.cur as usize..).unwrap_or(&[]);
        if format!("{open:?}") != relabel_debug(&format!("{:?}", k.open), &maps) {
            return Ok(false);
        }
        self.d = d;
        Ok(true)
    }
}

impl BlockHook for Converge<'_> {
    fn before_block(&mut self, eng: &mut Engine, cx: &Cx<'_, '_>, gi: u32, b: usize) -> LResult<Control> {
        if eng.en_blocks.is_some() || eng.pages.len() <= self.last_pages {
            return Ok(Control::Continue);
        }
        self.last_pages = eng.pages.len();
        let bu = u32::try_from(b).unwrap_or(u32::MAX);
        if b > self.b1
            && let Ok(k_ix) = self.cps.binary_search_by(|c| (c.gi, c.b).cmp(&(gi, bu)))
            && k_ix > self.from
            && let (Some(k), Some(after)) = (self.cps.get(k_ix), self.suffix.get(k_ix))
            && self.meets(eng, cx, k, after)?
        {
            self.found = Some(k_ix);
            return Ok(Control::Stop);
        }
        let e = eng.trace.as_deref_mut().map(Trace::take_epoch).unwrap_or_default();
        let cp = Checkpoint::take(eng, cx, gi, b)?;
        match self.new_cps.last_mut() {
            Some(l) => l.after = l.after.merge(e),
            None => self.first = self.first.merge(e),
        }
        self.new_cps.push(cp);
        Ok(Control::Continue)
    }
}
