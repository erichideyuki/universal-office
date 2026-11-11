//! The recording side of the incremental relayout (LA7 level 2): per pass, a page checkpoint
//! before the first body block of every new page (the engine without its pages, the pages still
//! open, the lengths of the call's tables), the trace of each interval, the pass's final pages
//! and engine; the call's final tables.

use indexmap::IndexMap;

use super::trace::{Epoch, Trace};
use super::{FallbackReason, IncrConfig};
use crate::engine::objects::ImageTable;
use crate::engine::page::PageState;
use crate::engine::{BlockHook, Call, Control, Cx, Engine, PassInput};
use crate::error::{LResult, LayoutError};
use crate::fonts::{FaceResolverCache, FaceTable};
use crate::items::PassItems;
use crate::model::{RunRef, Utf16};
use crate::output::LayoutPage;
use uo_render::{Resources, Sym, SymbolTable};

/// The lengths of the call's tables at one moment: they only grow within a call, so a
/// checkpoint restores them as prefixes of the final tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallLens {
    pub faces: usize,
    pub resolved: usize,
    pub misses: usize,
    pub symbols: usize,
    pub images: usize,
    pub text_fx: usize,
    pub page_fills: usize,
    pub cust: usize,
    pub origins: usize,
    pub img_warnings: usize,
    pub line_seq: u32,
}

impl CallLens {
    pub(crate) fn of(res: &Resources, resolver: &FaceResolverCache, images: &ImageTable, line_seq: u32) -> CallLens {
        let (resolved, misses) = resolver.lens();
        CallLens {
            faces: res.faces.len(),
            resolved,
            misses,
            symbols: res.symbols.len(),
            images: res.images.len(),
            text_fx: res.text_fx.len(),
            page_fills: res.page_fills.len(),
            cust: res.cust.len(),
            origins: images.origins.len(),
            img_warnings: images.warnings.len(),
            line_seq,
        }
    }

    pub(crate) fn of_call(c: &Call<'_, '_>) -> CallLens {
        CallLens::of(&c.res, &c.resolver, &c.images, c.line_seq)
    }

    pub(crate) fn of_cx(cx: &Cx<'_, '_>) -> CallLens {
        CallLens::of(cx.res, cx.resolver, cx.images, *cx.line_seq)
    }
}

/// The call's tables a `Layout` does not keep (its resources and image origins are in it).
pub struct CallTables {
    pub fonts: FaceTable,
    pub resolver: FaceResolverCache,
    pub images: ImageTable,
    pub line_seq: u32,
}

/// The call's tables as they were at `l`: prefixes of the final ones (`res` = the final
/// resources, a `Layout`'s). The symbol table is rebuilt by interning its first strings again
/// (same ids: first-use order).
pub(crate) fn restore(t: &CallTables, res: &Resources, l: &CallLens) -> LResult<(FaceTable, FaceResolverCache, Resources, ImageTable)> {
    let mut symbols = SymbolTable::new();
    for i in 0..l.symbols {
        let s = Sym::from_index(i).ok_or(LayoutError::Internal("incr: symbol id"))?;
        symbols.intern(res.symbols.get(s)).ok_or(LayoutError::Internal("incr: symbol"))?;
    }
    let pre = |n: usize, len: usize| -> LResult<usize> { if n <= len { Ok(n) } else { Err(LayoutError::Internal("incr: table prefix")) } };
    let r = Resources {
        symbols,
        faces: res.faces.get(..pre(l.faces, res.faces.len())?).unwrap_or(&[]).to_vec(),
        images: res.images.get(..pre(l.images, res.images.len())?).unwrap_or(&[]).to_vec(),
        text_fx: res.text_fx.get(..pre(l.text_fx, res.text_fx.len())?).unwrap_or(&[]).to_vec(),
        page_fills: res.page_fills.get(..pre(l.page_fills, res.page_fills.len())?).unwrap_or(&[]).to_vec(),
        cust: res.cust.get(..pre(l.cust, res.cust.len())?).unwrap_or(&[]).to_vec(),
    };
    let mut resolver = t.resolver.prefix(l.resolved, l.misses);
    resolver.log = Some(Vec::new());
    Ok((t.fonts.prefix(l.faces), resolver, r, t.images.prefix(l.origins, l.img_warnings)))
}

/// Whether `a` (the running call's resources) equals `b` (the previous call's) — lengths, and
/// the entries from `from` on (the earlier ones are the same prefix by construction).
pub(crate) fn same_resources(a: &Resources, b: &Resources, from: &CallLens) -> bool {
    let sym_ok = a.symbols.len() == b.symbols.len()
        && (from.symbols..a.symbols.len()).all(|i| Sym::from_index(i).is_some_and(|s| a.symbols.get(s) == b.symbols.get(s)));
    let faces_ok = a.faces.len() == b.faces.len()
        && a.faces.iter().zip(&b.faces).skip(from.faces).all(|(x, y)| {
            x.key == y.key
                && x.family == y.family
                && x.bold == y.bold
                && x.italic == y.italic
                && x.synth_bold == y.synth_bold
                && x.synth_italic == y.synth_italic
                && x.stand_in == y.stand_in
                && match (&x.bytes, &y.bytes) {
                    (Some(p), Some(q)) => std::sync::Arc::ptr_eq(p, q),
                    (None, None) => true,
                    _ => false,
                }
        });
    let dbg = |x: &dyn std::fmt::Debug, y: &dyn std::fmt::Debug| format!("{x:?}") == format!("{y:?}");
    sym_ok
        && faces_ok
        && a.images.len() == b.images.len()
        && a.text_fx.len() == b.text_fx.len()
        && a.page_fills.len() == b.page_fills.len()
        && a.cust.len() == b.cust.len()
        && a.images.iter().zip(&b.images).skip(from.images).all(|(x, y)| {
            x.bytes == y.bytes
                && x.svg == y.svg
                && x.ext == y.ext
                && dbg(&(x.effects, x.width_twips, x.height_twips, x.natural_px), &(y.effects, y.width_twips, y.height_twips, y.natural_px))
        })
        && dbg(&a.text_fx.get(from.text_fx..), &b.text_fx.get(from.text_fx..))
        && dbg(&a.page_fills.get(from.page_fills..), &b.page_fills.get(from.page_fills..))
        && dbg(&a.cust.get(from.cust..), &b.cust.get(from.cust..))
}

/// One page checkpoint: the state before body block `b` of section `gi`, taken when the pass had
/// started a page since the previous one.
pub(crate) struct Checkpoint {
    pub gi: u32,
    pub b: u32,
    pub cur: u32,
    pub pages_len: u32,
    /// the engine without its pages (and without its trace)
    pub eng: Box<Engine>,
    /// the pages from `cur` on, as they were (still being filled)
    pub open: Vec<PageState>,
    pub lens: CallLens,
    /// the metafile warnings of the pass not yet moved to its context
    pub img_warnings: Vec<Box<str>>,
    /// the reads and writes from here to the next checkpoint (or the end of the pass)
    pub after: Epoch,
    /// estimated heap bytes (the cap of [`IncrConfig::checkpoint_bytes`])
    pub bytes: usize,
}

impl Checkpoint {
    pub(crate) fn take(eng: &mut Engine, cx: &Cx<'_, '_>, gi: u32, b: usize) -> LResult<Checkpoint> {
        let cur = eng.cur_ix()?.0;
        let pages = std::mem::take(&mut eng.pages);
        let trace = eng.trace.take();
        let clone = Box::new(eng.clone());
        eng.pages = pages;
        eng.trace = trace;
        let open = eng.pages.get(cur as usize..).unwrap_or(&[]).to_vec();
        let bytes = engine_bytes(&clone) + open.iter().map(page_state_bytes).sum::<usize>();
        Ok(Checkpoint {
            gi,
            b: u32::try_from(b).map_err(|_| LayoutError::Internal("incr: block"))?,
            cur,
            pages_len: u32::try_from(eng.pages.len()).map_err(|_| LayoutError::Internal("incr: pages"))?,
            eng: clone,
            open,
            lens: CallLens::of_cx(cx),
            img_warnings: cx.images.warnings.clone(),
            after: Epoch::default(),
            bytes,
        })
    }
}

/// One recorded pass.
pub(crate) struct PassRec {
    pub input: PassInput,
    /// the call's line counter when the pass started
    pub line_start: u32,
    /// the reads and writes before the first checkpoint
    pub pre: Epoch,
    pub cps: Vec<Checkpoint>,
    /// the pass's final pages
    pub pages: Vec<PageState>,
    /// the pass's final engine, without its pages (its warnings as at the end of the pass)
    pub eng: Option<Box<Engine>>,
    /// what the pass reported through `onField` (before the call took it)
    pub fields: Option<IndexMap<RunRef, Utf16>>,
    /// the tables at the end of the pass
    pub end: CallLens,
}

impl PassRec {
    /// Estimated heap bytes of the checkpoints and of the retained pages.
    pub(crate) fn bytes(&self) -> (usize, usize) {
        (self.cps.iter().map(|c| c.bytes).sum(), self.pages.iter().map(page_state_bytes).sum())
    }

    /// The reads and writes of every interval from checkpoint `k` on (to the end of the pass).
    pub(crate) fn suffixes(&self) -> Vec<Epoch> {
        let mut out = vec![Epoch::default(); self.cps.len() + 1];
        for k in (0..self.cps.len()).rev() {
            let next = out.get(k + 1).copied().unwrap_or_default();
            if let (Some(o), Some(c)) = (out.get_mut(k), self.cps.get(k)) {
                *o = c.after.merge(next);
            }
        }
        out
    }

    /// The reads before checkpoint `k` (every interval up to it), for each `k`.
    pub(crate) fn prefixes(&self) -> Vec<Epoch> {
        let mut out = Vec::with_capacity(self.cps.len());
        let mut acc = self.pre;
        for c in &self.cps {
            out.push(acc);
            acc = acc.merge(c.after);
        }
        out
    }

    /// Keeps a checkpoint only `k` pages or more after the last one kept (the first always),
    /// merging the intervals of the dropped ones into the kept one before them (the memory cap,
    /// M5). Idempotent: a part already spaced by `k` keeps its checkpoints.
    pub(crate) fn thin(&mut self, k: usize) {
        if k <= 1 {
            return;
        }
        let old = std::mem::take(&mut self.cps);
        for c in old {
            let far = self.cps.last().is_none_or(|l| c.cur as usize >= l.cur as usize + k);
            if far {
                self.cps.push(c);
            } else if let Some(last) = self.cps.last_mut() {
                last.after = last.after.merge(c.after);
            } else {
                self.pre = self.pre.merge(c.after);
            }
        }
    }
}

/// Spaces the checkpoints of `passes` until they and the kept pages fit `cap` (estimated
/// bytes): the spacing in pages, or `None` when the pages alone do not fit.
pub(crate) fn fit_cap(passes: &mut [PassRec], cap: usize, spacing: usize) -> Option<usize> {
    let total = |ps: &[PassRec]| ps.iter().map(PassRec::bytes).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let (mut cp, pages) = total(passes);
    if pages >= cap {
        return None;
    }
    let room = cap - pages;
    let mut k = spacing.max(1);
    if cp > room {
        // a first guess from the density, then doubled until it fits
        k = k.max(cp.div_ceil(room.max(1)));
    }
    loop {
        if k > 1 {
            for p in passes.iter_mut() {
                p.thin(k);
            }
        }
        cp = total(passes).0;
        if cp <= room {
            return Some(k);
        }
        if passes.iter().all(|p| p.cps.len() <= 1) {
            // thinned as far as it goes and still over the cap: nothing is kept
            return None;
        }
        k = k.saturating_mul(2);
    }
}

/// The recorder of a call (a [`BlockHook`]): checkpoints, traces, the passes' pages and engines.
pub struct Recorder {
    cfg: IncrConfig,
    pub(crate) passes: Vec<PassRec>,
    retired: usize,
    last_pages: usize,
    pub(crate) discarded: Option<FallbackReason>,
    pub(crate) tables: Option<CallTables>,
    /// the spacing the memory cap imposed (1 = a checkpoint per page)
    pub(crate) spacing: usize,
}

impl Recorder {
    pub(crate) fn new(cfg: IncrConfig) -> Recorder {
        Recorder { cfg, passes: Vec::new(), retired: 0, last_pages: 0, discarded: None, tables: None, spacing: 1 }
    }

    /// No checkpoint of this call is kept (balancing probes ran; past the memory cap).
    pub(crate) fn discard(&mut self, why: FallbackReason) {
        if self.discarded.is_none() {
            self.discarded = Some(why);
        }
        self.passes.clear();
    }

    pub(crate) fn begin_pass(&mut self, eng: &mut Engine, input: &PassInput, line_start: u32) {
        self.last_pages = 0;
        if self.discarded.is_some() {
            return;
        }
        eng.trace = Some(Box::new(Trace::new()));
        self.passes.push(PassRec {
            input: input.clone(),
            line_start,
            pre: Epoch::default(),
            cps: Vec::new(),
            pages: Vec::new(),
            eng: None,
            fields: None,
            end: CallLens::default(),
        });
    }

    /// Closes the interval that ends now (the one after the last checkpoint, or before the
    /// first).
    pub(crate) fn close_epoch(pass: &mut PassRec, e: Epoch) {
        match pass.cps.last_mut() {
            Some(c) => c.after = c.after.merge(e),
            None => pass.pre = pass.pre.merge(e),
        }
    }

    pub(crate) fn end_pass(&mut self, eng: &mut Engine, lens: CallLens) {
        let trace = eng.trace.take();
        if self.discarded.is_some() {
            return;
        }
        let Some(pass) = self.passes.last_mut() else { return };
        if let Some(mut t) = trace {
            Recorder::close_epoch(pass, t.take_epoch());
        }
        pass.fields.clone_from(&eng.fields);
        pass.end = lens;
    }

    /// The engine of the next pass without one (passes retire in order).
    pub(crate) fn retire(&mut self, mut eng: Engine) {
        if self.discarded.is_some() {
            return;
        }
        if let Some(p) = self.passes.get_mut(self.retired) {
            p.pages = std::mem::take(&mut eng.pages);
            eng.trace = None;
            p.eng = Some(Box::new(eng));
        }
        self.retired += 1;
    }

    /// The call's tables at its end; applies the memory cap.
    pub(crate) fn finish_call(&mut self, t: CallTables) {
        self.tables = Some(t);
        if self.discarded.is_some() {
            return;
        }
        match fit_cap(&mut self.passes, self.cfg.checkpoint_bytes, self.spacing) {
            Some(k) => self.spacing = k,
            None => self.discard(FallbackReason::Memory),
        }
    }
}

impl BlockHook for Recorder {
    fn before_block(&mut self, eng: &mut Engine, cx: &Cx<'_, '_>, gi: u32, b: usize) -> LResult<Control> {
        if self.discarded.is_some() || eng.en_blocks.is_some() || eng.pages.len() <= self.last_pages {
            return Ok(Control::Continue);
        }
        self.last_pages = eng.pages.len();
        let e = eng.trace.as_deref_mut().map(Trace::take_epoch).unwrap_or_default();
        let cp = Checkpoint::take(eng, cx, gi, b)?;
        if let Some(pass) = self.passes.last_mut() {
            Recorder::close_epoch(pass, e);
            pass.cps.push(cp);
        }
        Ok(Control::Continue)
    }
}

// ───────────────────────── heap estimates (the caps; the measured numbers are in the record) ─────────────────────────

pub(crate) fn items_bytes(p: &PassItems) -> usize {
    p.list.heap_bytes() + p.side.capacity() * std::mem::size_of::<crate::items::ItemSide>()
}

fn lines_bytes(l: &[crate::items::PassLine]) -> usize {
    l.iter().map(|x| std::mem::size_of::<crate::items::PassLine>() + x.chars.as_ref().map_or(0, |c| c.len() * 24)).sum()
}

fn content_bytes(c: &crate::table::CellContent) -> usize {
    items_bytes(&c.items) + lines_bytes(&c.lines)
}

pub(crate) fn page_state_bytes(p: &PageState) -> usize {
    std::mem::size_of::<PageState>()
        + items_bytes(&p.hf)
        + items_bytes(&p.body)
        + [&p.behind, &p.front, &p.hf_behind, &p.hf_front].iter().flat_map(|v| v.iter()).map(|z| items_bytes(&z.items) + 24).sum::<usize>()
        + lines_bytes(&p.page.lines)
        + lines_bytes(&p.page.content_lines)
        + p.notes.iter().map(|n| content_bytes(&n.content)).sum::<usize>()
        + p.page.objects.as_ref().map_or(0, |o| o.len() * 128)
        + p.page.cells.as_ref().map_or(0, |c| c.len() * 64)
}

pub(crate) fn layout_page_bytes(p: &LayoutPage) -> usize {
    std::mem::size_of::<LayoutPage>()
        + p.page.list.heap_bytes()
        + p.lines.capacity() * std::mem::size_of::<crate::output::LineGeom>()
        + p.content_lines.capacity() * std::mem::size_of::<crate::output::LineGeom>()
        + p.chars.o.capacity() * 4
        + p.chars.x.capacity() * 8
        + p.chars.w.capacity() * 8
        + p.pfmts.capacity() * std::mem::size_of::<crate::output::LineParaFormat>()
        + p.item_meta.capacity() * 32
}

pub(crate) fn engine_bytes(e: &Engine) -> usize {
    std::mem::size_of::<Engine>()
        + e.note_cache.values().map(content_bytes).sum::<usize>()
        + e.chain_slices.values().flat_map(|m| m.values()).map(content_bytes).sum::<usize>()
        + e.scratch.as_deref().map_or(0, page_state_bytes)
        + items_bytes(&e.hf_frame_after)
        + items_bytes(&e.hf_frame_tbl)
        + e.list_texts.as_ref().map_or(0, |m| m.len() * 64)
        + e.ctx.flow.seq.len() * 32
        + e.pre_placed.len() * 8
        + e.geoms.len() * 512
}
