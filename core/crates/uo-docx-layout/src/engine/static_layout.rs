//! The static flow (frontier F7c, handed on by F7a): `layoutStatic` (engine.ts:1202) — blocks laid
//! out without pagination, `y` relative to 0. Used by headers/footers (`hf.rs`), notes
//! (`notes.rs`), and by F7b for cell contents (`layoutCellIn`) and textbox contents.
//!
//! **Stable entry point.** [`Engine::layout_static`] takes the blocks as [`SBlock`]s (a derived
//! paragraph by its [`LParaId`], or a model table), the frame (`x`, `width`), the paragraph env,
//! the TS ref of the flow as a [`LineRef`] (`hdr:default`, `fn3`, `b3/r0/c1`, …: each line gets
//! `<ref>/b<i>`), the page the anchored objects go to (`ps`, `None` for a measure), `y_origin` and
//! the cell neighbours. [`Engine::story_blocks`] turns a story of the derived document into
//! blocks.
//!
//! The float, frame, wrap and table sites of the flow call F7b's code (`place_float`,
//! `place_floats`, `static_cell_floats`, `wrap_frame`, `paint_inline_drawings`, `static_table`),
//! wired in by F7b.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{PassItems, PassLine};
use crate::keys::{CellContentId, ContainerId, LParaId, PageIx};
use crate::ldoc::{LBlock, ParaFlags};
use crate::lines::LineBreaker;
use crate::model::{Run, Table};
use crate::output::{LineParaFormat, LineRef, LineStory, RefPath};
use crate::para::ParaEnv;
use crate::table::{CellContent, ParaSplitInfo};
use uo_docx::model::story::Block;
use uo_jsnum::math as jm;
use uo_render::OptF64;

use super::flow::{CellNeighbors, HiddenJoin, PSrc, PrevPara, RtPara};
use super::page::PageState;
use super::{Cx, Engine};

/// A block of a static flow: a derived paragraph, or a model table (the TS `Block`).
#[derive(Clone, Copy, Debug)]
pub enum SBlock<'a> {
    Para(LParaId),
    Table(&'a Table),
    /// a paragraph object the engine made: the continuation of a paragraph cut between the boxes
    /// of a linked chain (`sliceParagraphFrom`, chain.ts:57; F7d). Only the first block of a flow
    Rt(&'a RtPara),
}

/// `run.image?.floating || run.shape?.floating || run.group?.floating || run.textbox?.floating`.
pub(crate) fn run_floats(r: &Run) -> bool {
    let Some(e) = r.ext() else { return false };
    e.image.as_ref().and_then(|x| x.floating) == Some(true)
        || e.shape.as_ref().and_then(|x| x.floating) == Some(true)
        || e.group.as_ref().and_then(|x| x.floating) == Some(true)
        || e.textbox.as_ref().and_then(|x| x.floating) == Some(true)
}

/// The line ref of block `bi` of a flow whose ref is `at` (`` `${ref}/b${bi}` ``).
pub(crate) fn child_ref(at: &LineRef, bi: u32) -> LineRef {
    let mut p = at.path.0.to_vec();
    p.push(bi);
    LineRef { story: at.story, path: RefPath(p.into()) }
}

/// `/^(hdr|ftr):\w+$/.test(ref)`: a header/footer flow (the slot names are word characters).
fn is_hf_ref(at: &LineRef) -> bool {
    matches!(at.story, LineStory::HeaderFooter { .. }) && at.path.0.is_empty()
}

/// `/\/r\d+\/c\d+$/.test(ref)`: the ref ends in a table cell.
fn is_cell_ref(at: &LineRef) -> bool {
    // (the segment tags cycle b, r, c; a preview's path starts at the row)
    let first = usize::from(at.story == LineStory::Preview);
    let n = at.path.0.len();
    n >= 2 && (n + first).is_multiple_of(3)
}

/// `scratchPage(ps)` (engine.ts:4148): a throwaway copy of the page state (measures that place
/// floats): the page without items, lines, objects or cells, the layers empty, the floats copied.
fn scratch_of(ps: &PageState) -> PageState {
    let mut page = ps.page.clone();
    page.lines = Vec::new();
    page.objects = Some(Vec::new());
    page.cells = Some(Vec::new());
    PageState {
        page,
        behind: Vec::new(),
        hf: PassItems::new(),
        body: PassItems::new(),
        front: Vec::new(),
        hf_behind: Vec::new(),
        hf_front: Vec::new(),
        floats: ps.floats.clone(),
        notes: Vec::new(),
        ..ps.clone()
    }
}

impl Engine {
    /// The blocks of a story of the derived document (index into `LDoc::stories`): paragraphs by
    /// key, tables resolved in the story's model.
    pub fn story_blocks<'a>(&self, cx: &Cx<'_, 'a>, story: u32) -> LResult<Vec<SBlock<'a>>> {
        let st = cx.ldoc.stories.get(story as usize).ok_or(LayoutError::Internal("static: story"))?;
        let model = st.sref.as_ref().and_then(|r| cx.ldoc.view.story(r));
        let mut out = Vec::with_capacity(st.blocks.len());
        for b in &st.blocks {
            out.push(match *b {
                LBlock::Paragraph(k) => SBlock::Para(k),
                LBlock::Table { block } => match model.and_then(|m| m.blocks().nth(block as usize)) {
                    Some(Block::Table(t)) => SBlock::Table(t),
                    _ => return Err(LayoutError::Internal("static: table block")),
                },
            });
        }
        Ok(out)
    }

    /// Makes a scratch copy of page `ps` (`scratchPage(ps)`), reachable as [`super::SCRATCH`]
    /// until [`Self::drop_scratch`].
    pub(crate) fn make_scratch(&mut self, ps: PageIx) -> LResult<PageIx> {
        let s = scratch_of(self.ps(ps)?);
        self.scratch = Some(Box::new(s));
        Ok(super::SCRATCH)
    }

    pub(crate) fn drop_scratch(&mut self) {
        self.scratch = None;
    }

    /// `ctx.pure(fn)` on the engine: the flow state (lists, SEQ, headings, notes) saved and
    /// restored whatever `f` returns, the snapshot charged to the work budget (LA4).
    pub(crate) fn pure_static<'a, T>(
        &mut self,
        cx: &mut Cx<'_, 'a>,
        f: impl FnOnce(&mut Engine, &mut Cx<'_, 'a>) -> LResult<T>,
    ) -> LResult<T> {
        cx.budget.pure_clones = cx.budget.pure_clones.saturating_add(1);
        cx.budget.charge(1)?;
        let saved = self.ctx.save_flow();
        let r = f(self, cx);
        self.ctx.flow = saved;
        r
    }

    /// `layoutStatic(blocks, x, width, env, ref, ps, yOrigin, nb)` (engine.ts:1202): the blocks
    /// laid out without pagination (`y` relative to 0); anchored floats go to page `ps` (when
    /// given) with `y_origin` the absolute y of the top.
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub fn layout_static<'a>(
        &mut self,
        cx: &mut Cx<'_, 'a>,
        blocks: &[SBlock<'a>],
        x: f64,
        width: f64,
        env: &ParaEnv,
        at: &LineRef,
        ps: Option<PageIx>,
        y_origin: f64,
        nb: Option<&CellNeighbors>,
    ) -> LResult<CellContent> {
        if cx.depth > cx.opts.limits.max_depth {
            return Err(LayoutError::Limit(LimitKind::Depth));
        }
        cx.budget.charge(1)?;
        cx.depth += 1;
        let r = self.layout_static_in(cx, blocks, x, width, env, at, ps, y_origin, nb);
        cx.depth -= 1;
        r
    }

    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    fn layout_static_in<'a>(
        &mut self,
        cx: &mut Cx<'_, 'a>,
        blocks: &[SBlock<'a>],
        x: f64,
        width: f64,
        env: &ParaEnv,
        at: &LineRef,
        ps: Option<PageIx>,
        y_origin: f64,
        nb: Option<&CellNeighbors>,
    ) -> LResult<CellContent> {
        // (a frame inside a table cell: Word ignores it — a normal paragraph, corpus t43; outside
        // cells the stories come lifted from `derive`, F7b)
        let _ = y_origin; // (the floats placed from it are F7b's)
        let box_page = ContainerId::Static(CellContentId(self.static_seq));
        self.static_seq = self.static_seq.wrapping_add(1);
        let mut items = PassItems::new();
        let mut lines: Vec<PassLine> = Vec::new();
        let mut y = 0.0;
        let mut prev: Option<PrevPara> = None;
        // the list of the previous paragraph (automatic spacing between items)
        let mut prev_list: Option<Box<str>> = None;
        let mut keep_next = false;
        let n = blocks.len();
        let hf_flow = is_hf_ref(at);
        let is_hf = ps.is_some() && hf_flow;
        let compat = cx.ldoc.view.formatting().settings.compatibility_mode.to_number();
        // cell floats, floating tables nested in a cell, nested rows (F7b)
        let mut sf = super::tables::StaticFloats::default();
        let mut cell_shape = false;
        if is_hf {
            // header/footer: floats with an absolute vertical position (page/margin) go first — the
            // content BEFORE the anchor wraps around them too (measured in Word; F7b)
            let pix = ps.ok_or(LayoutError::Internal("static: page"))?;
            for b in blocks {
                // (a frame's paragraph: only the box it carries)
                let runs = match *b {
                    SBlock::Para(k) => match cx.ldoc.para(k) {
                        Some(lp) if lp.flags.has(ParaFlags::FRAME_SKIP) => lp.frame_carry.into_iter().collect(),
                        Some(lp) => lp.runs.clone(),
                        None => Vec::new(),
                    },
                    // (a cut first run is a text run: never a float)
                    SBlock::Rt(rt) => rt.runs.clone(),
                    SBlock::Table(_) => continue,
                };
                for r in runs {
                    let Some(run) = crate::paint::model_run(cx.ldoc, r) else { continue };
                    if !run_floats(run) {
                        continue;
                    }
                    let Some(anc) = super::floats::float_anchor(run) else { continue };
                    if !super::floats::absolute_v(&anc.pos_v.relative_from) || self.only_text_floats {
                        continue;
                    }
                    self.place_float(cx, r, pix, x, width, y_origin, Some(&mut items), y_origin, None, None)?;
                    self.pre_placed.insert(r);
                }
            }
        }
        // a hidden-mark paragraph joining the next one
        let mut join: Option<HiddenJoin> = None;
        // runs of deleted-mark paragraphs (revision) going to the next one
        let mut del_runs: Option<Vec<crate::keys::RunKey>> = None;
        // the carried runs start with a chain continuation's cut run (F7d-2)
        let mut del_cut = 0u32;
        let mut paras: IndexMap<LineRef, ParaSplitInfo> = IndexMap::new();
        let mut last_para: Option<LineRef> = None;
        for (bi, b) in blocks.iter().enumerate() {
            cx.budget.charge(1)?;
            let bi32 = u32::try_from(bi).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            let next_is_para = matches!(blocks.get(bi + 1), Some(SBlock::Para(_) | SBlock::Rt(_)));
            // a chain continuation (F7d): the copy, over its base paragraph
            let (b, rt): (SBlock<'_>, Option<&RtPara>) = match *b {
                SBlock::Rt(rt) => (SBlock::Para(rt.base), Some(rt)),
                other => (other, None),
            };
            match b {
                SBlock::Para(k) => {
                    // a frame: out of the flow (F7b); the box it carries goes here (`FRAME_SKIP` is
                    // per object: a continuation copy is never in it)
                    if rt.is_none()
                        && let Some(lp) = cx.ldoc.para(k)
                        && lp.flags.has(ParaFlags::FRAME_SKIP)
                    {
                        if let (Some(carry), Some(pix)) = (lp.frame_carry, ps)
                            && !self.pre_placed.contains(&carry)
                        {
                            let top = y_origin + y + prev.as_ref().map_or(0.0, |p| p.after);
                            let dx = self.static_dx;
                            self.place_float(cx, carry, pix, x + dx, width, top, Some(&mut items), y_origin, None, None)?;
                        }
                        // anchored in a table: the frame's text goes into its first row, in column order
                        if !self.hf_frame_after.is_empty() && matches!(blocks.get(bi + 1), Some(SBlock::Table(_))) {
                            let a = std::mem::take(&mut self.hf_frame_after);
                            for i in 0..a.len() {
                                super::push_copy(&mut self.hf_frame_tbl, &a, i)?;
                            }
                        }
                        if !self.hf_frame_after.is_empty() {
                            let a = std::mem::take(&mut self.hf_frame_after);
                            for i in 0..a.len() {
                                super::push_copy(&mut items, &a, i)?;
                            }
                        }
                        continue;
                    }
                    // a deleted mark (revision): the text goes to the next paragraph, with ITS
                    // properties (oracle delmark); all the text deleted: gone (numbering3, del-mark)
                    let own = match rt {
                        Some(rt) => PSrc::Rt(Box::new(rt.clone())),
                        None => PSrc::Doc(k),
                    };
                    let gone = {
                        let (_, props, _, _) = super::flow::fields_of(cx.ldoc, &own)?;
                        self.ctx.rev.mark_gone(&props)
                    };
                    if gone && join.is_none() && bi + 1 < n && next_is_para {
                        let runs = match rt {
                            Some(rt) => rt.runs.clone(),
                            None => cx.ldoc.para(k).map(|p| p.runs.clone()).unwrap_or_default(),
                        };
                        let carried = del_runs.get_or_insert_with(Vec::new);
                        if carried.is_empty() {
                            del_cut = rt.map_or(0, |rt| rt.first_from);
                        }
                        carried.extend(runs);
                        continue;
                    }
                    let shown_none = match rt {
                        Some(rt) => self.no_shown_run_keys(cx, &rt.runs),
                        None => self.no_shown_runs(cx, k),
                    };
                    if gone && shown_none && join.is_none() && del_runs.is_none() {
                        continue;
                    }
                    let merged = match del_runs.take() {
                        // (a continuation is only the first block: it never takes carried runs)
                        Some(carried) => self.join_deleted(cx, carried, std::mem::take(&mut del_cut), k)?,
                        None => own,
                    };
                    let para = match &join {
                        Some(j) => self.join_hidden(cx, j, &merged, env)?,
                        None => merged,
                    };
                    let continuation = match &para {
                        PSrc::Doc(pk) => cx.ldoc.para(*pk).is_some_and(|lp| lp.flags.has(ParaFlags::CONTINUATION)),
                        PSrc::Rt(r) => r.continuation,
                    };
                    let penv = ParaEnv {
                        no_page_breaks: true,
                        numbering: if join.is_some() { Some(false) } else { env.numbering },
                        continuation: continuation || env.continuation,
                        ..env.clone()
                    };
                    let mut p = self.prepare(cx, &para, &penv)?;
                    if let Some(j) = join.take() {
                        super::flow::attach_marker(&mut p, j.marker, &j.marker_styles);
                    }
                    // a hidden mark (not the last block): without visible content it takes
                    // nothing; with content it joins the next paragraph (oracle hidden)
                    let mark_hidden = p.styles.get(p.mark_style.0 as usize).is_some_and(|s| s.hidden);
                    if mark_hidden && bi + 1 < n {
                        if p.glyphs.is_empty() {
                            continue;
                        }
                        if next_is_para {
                            let (marker, marker_styles) = super::flow::detach_marker(&mut p);
                            join = Some(HiddenJoin { para, marker, marker_styles });
                            continue;
                        }
                    }
                    if p.keep_next {
                        keep_next = true;
                    }
                    let first_in_cell = bi == 0;
                    let last_in_cell = bi + 1 == n;
                    let mut before = if p.before_auto { if first_in_cell { 0.0 } else { 280.0 } } else { p.space_before };
                    let mut prev_after = prev.as_ref().map_or(0.0, |pv| pv.after);
                    // the "after" dropped by the previous one's contextual spacing: the "before"
                    // counts only past it (oracle ctxmark)
                    let mut ctx_after: Option<f64> = None;
                    if let Some(pv) = &prev
                        && pv.style_id == p.style_id
                    {
                        if pv.contextual {
                            ctx_after = Some(prev_after);
                            prev_after = 0.0;
                        }
                        if p.contextual {
                            before = 0.0;
                        }
                    }
                    // automatic spacing between items of the same list: zero (oracle autosplist)
                    let list = self.list_of(cx, &p);
                    if let Some(l) = list.as_deref().filter(|l| !l.is_empty())
                        && prev_list.as_deref() == Some(l)
                    {
                        if p.before_auto {
                            before = 0.0;
                        }
                        if prev.as_ref().is_some_and(|pv| pv.after_auto) {
                            prev_after = 0.0;
                        }
                    }
                    prev_list.clone_from(&list);
                    // the cell's first paragraph: its previous neighbour is the paragraph before
                    // the table, the last of the previous cell or the row-end mark (oracle autosp2)
                    if bi == 0
                        && let Some(pn) = nb.and_then(|nb| nb.prev.as_ref())
                        && p.contextual
                        && pn.style_id == p.style_id
                    {
                        before = 0.0;
                    }
                    // the effective "after" of the previous one (contextual drops it)
                    if let Some(lk) = &last_para
                        && let Some(lpi) = paras.get_mut(lk)
                    {
                        lpi.after = prev_after;
                    }
                    let gap_s = match ctx_after {
                        Some(ca) => jm::max(0.0, self.collapse(cx, ca, before) - ca),
                        None => self.collapse(cx, prev_after, before),
                    };
                    // the paragraph top for objects positioned from it (oracle anchorbefore)
                    let para_ref_s = y + jm::min(prev_after, gap_s);
                    y += gap_s;
                    let top = y;
                    let mut br = LineBreaker::new(&p, None);
                    let frame0 = super::page::WrapFrame {
                        left: p.ind_left,
                        right: width - p.ind_right,
                        skip_to: OptF64::NONE,
                        segs: None,
                        gap: false,
                    };
                    // header/footer: the paragraph's floats are placed BEFORE its lines and the text
                    // (the paragraph's own too) wraps around them — oracle hffloat (F7b)
                    let hf_wrap = ps.is_some() && hf_flow;
                    if hf_wrap && let Some(pix) = ps {
                        self.place_floats(cx, &p, pix, x, width, y_origin + top, Some(&mut items), y_origin, Some(y_origin + para_ref_s))?;
                    }
                    // cell: the paragraph's floats come BEFORE the lines and the cell's text wraps the
                    // wrapping ones (oracle cellwrap); drawn after the text (F7b)
                    let cell_anch = self.in_cell && ps.is_some() && !hf_wrap && is_cell_ref(at);
                    if self.in_cell && !cell_shape {
                        cell_shape =
                            p.glyphs.iter().any(|g| g.obj.and_then(|k| crate::paint::model_run(cx.ldoc, k)).is_some_and(run_floats));
                    }
                    let mut cell_anch_items = PassItems::new();
                    if cell_anch && let Some(pix) = ps {
                        self.static_cell_floats(cx, &p, pix, x, width, y_origin, top, para_ref_s, &mut sf, &mut cell_anch_items)?;
                    }
                    let box_top = y;
                    let box_at = items.len();
                    y += Self::border_top_extent(&p, prev.as_ref());
                    let l0 = lines.len();
                    let rf = child_ref(at, bi32);
                    let pfmt = Arc::new(LineParaFormat {
                        left: p.ind_left,
                        right: p.ind_right,
                        first_line: p.first_line,
                        tabs: p.tabs.clone().into(),
                    });
                    let mark_h = super::floats::mark_line_h(cx, &p)?;
                    loop {
                        cx.budget.charge(1)?;
                        // wrapping: the floats of the page (header/footer) or of the cell itself
                        let use_cell = !hf_wrap && !sf.cell_floats.is_empty();
                        let w_y0 = if hf_wrap { y_origin } else { 0.0 };
                        let wrap = |eng: &Engine, sf: &super::tables::StaticFloats, yy: f64, lh: f64| -> LResult<super::page::WrapFrame> {
                            if hf_wrap {
                                let pix = ps.ok_or(LayoutError::Internal("static: page"))?;
                                eng.wrap_frame_in(&eng.ps(pix)?.floats, &frame0, x, w_y0 + yy, lh)
                            } else if use_cell {
                                eng.wrap_frame_in(&sf.cell_floats, &frame0, x, yy, lh)
                            } else {
                                Ok(frame0.clone())
                            }
                        };
                        let w_any = hf_wrap || use_cell;
                        let mut frame = frame0.clone();
                        if w_any {
                            frame = wrap(self, &sf, y, mark_h)?;
                            let mut k = 0;
                            while k < 8
                                && let Some(s) = frame.skip_to.get()
                                && s - w_y0 > y
                            {
                                y = s - w_y0;
                                frame = wrap(self, &sf, y, mark_h)?;
                                k += 1;
                            }
                        }
                        let mark = br.mark();
                        let (mut seg_k, sf0) = Engine::pick_seg(&br, &p, &frame);
                        let mut line = br.next(cx, &p, sf0)?;
                        // a line taller than the estimate (an inline picture): the wrap holds for the
                        // real height — down or narrower (oracle tallwrap)
                        let text_h = jm::min(line.height, line.baseline + line.desc);
                        let w_floats =
                            if hf_wrap { !self.ps(ps.ok_or(LayoutError::Internal("static: page"))?)?.floats.is_empty() } else { use_cell };
                        if w_any && w_floats && text_h > mark_h + 1.0 {
                            let f2 = wrap(self, &sf, y, text_h)?;
                            if let Some(s) = f2.skip_to.get()
                                && s - w_y0 > y
                            {
                                y = s - w_y0;
                                frame = wrap(self, &sf, y, mark_h)?;
                                br.reset(mark);
                                let (k, f) = Engine::pick_seg(&br, &p, &frame);
                                seg_k = k;
                                line = br.next(cx, &p, f)?;
                            } else if super::floats::narrower(&f2, &frame) {
                                frame = f2;
                                br.reset(mark);
                                let (k, f) = Engine::pick_seg(&br, &p, &frame);
                                seg_k = k;
                                line = br.next(cx, &p, f)?;
                            }
                        }
                        let mut lh = 0.0_f64;
                        {
                            let yy = y;
                            let eng: &mut Engine = self;
                            let (items_r, lines_r, lh_r, pp, rf_r, pf) = (&mut items, &mut lines, &mut lh, &p, &rf, &pfmt);
                            let hf = env.hf;
                            let mut put = |cx: &mut Cx<'_, '_>, l: &crate::lines::Line| -> LResult<()> {
                                crate::paint::paint_line(cx, pp, l, crate::paint::LinePlacement { frame_x: x, top: yy, hf }, items_r)?;
                                eng.paint_inline_drawings(cx, pp, l, x, yy, ps, false, items_r)?;
                                lines_r.push(super::flow::line_record(rf_r, pp, l, x, yy, Some(width), pf));
                                *lh_r = jm::max(*lh_r, l.height);
                                Ok(())
                            };
                            put(cx, &line)?;
                            Engine::more_segs(cx, &mut br, pp, &frame, seg_k, line, &mut put)?;
                        }
                        y += lh;
                        if br.done(&p) {
                            break;
                        }
                    }
                    // frames of the header/footer right of the line start: after the anchor's lines
                    if !self.hf_frame_after.is_empty() {
                        let a = std::mem::take(&mut self.hf_frame_after);
                        for i in 0..a.len() {
                            super::push_copy(&mut items, &a, i)?;
                        }
                    }
                    let next_l = blocks.get(bi + 1).map(|b| match b {
                        SBlock::Para(k) => LBlock::Paragraph(*k),
                        SBlock::Rt(rt) => LBlock::Paragraph(rt.base),
                        SBlock::Table(_) => LBlock::Table { block: 0 },
                    });
                    let grouped_next = self.groups_with_next(cx, &p, next_l, env)?;
                    let band_b = Self::border_bottom_extent(&p, grouped_next);
                    y += band_b;
                    self.paint_box(cx, &mut items, &p, x, width, box_top, y, prev.as_ref(), box_at, grouped_next, Some(box_page))?;
                    // floats anchored in the paragraph (outside the header/footer: after the lines)
                    if let Some(pix) = ps
                        && !hf_wrap
                        && !cell_anch
                    {
                        let dx = self.static_dx;
                        self.place_floats(
                            cx,
                            &p,
                            pix,
                            x + dx,
                            width,
                            y_origin + top,
                            Some(&mut items),
                            y_origin,
                            Some(y_origin + para_ref_s),
                        )?;
                    }
                    for i in 0..cell_anch_items.len() {
                        super::push_copy(&mut items, &cell_anch_items, i)?;
                    }
                    let mut after = if p.after_auto { if last_in_cell && !hf_flow { 0.0 } else { 280.0 } } else { p.space_after };
                    // the cell's last one: the next neighbour is the next cell's first paragraph or
                    // the row-end mark
                    if last_in_cell
                        && let Some(nn) = nb.and_then(|nb| nb.next.as_ref())
                        && p.contextual
                        && nn.style_id == p.style_id
                    {
                        after = 0.0;
                    }
                    // a table row split between pages: widows/orphans (mode 15 only) and the
                    // effective "after" that must fit with the last line (oracle rowwidow)
                    let n_lines = f64::from(u32::try_from(lines.len() - l0).unwrap_or(u32::MAX));
                    let info = ParaSplitInfo {
                        lines: n_lines,
                        after,
                        band: if uo_jsnum::conv::truthy(band_b) { OptF64::some(band_b) } else { OptF64::NONE },
                        widow: compat >= 15.0 && (p.widow_control || p.ppr.widow_control.is_none()),
                    };
                    paras.insert(rf.clone(), info);
                    last_para = Some(rf);
                    let has_bdr = p.ppr.p_bdr.is_some();
                    prev = Some(PrevPara {
                        after,
                        after_auto: p.after_auto,
                        list: None,
                        style_id: p.style_id.clone().filter(|s| !s.is_empty()),
                        contextual: p.contextual,
                        p_bdr: has_bdr,
                        p_bdr_key: if has_bdr { super::flow::bdr_key(&p.ppr) } else { None },
                        box_bottom_item_idx: None,
                        box_bottom: if has_bdr { OptF64::some(y) } else { OptF64::NONE },
                        box_page: if has_bdr { Some(box_page) } else { None },
                    });
                }
                SBlock::Table(t) => {
                    // nested and floating tables, through the engine's `layoutCell` (F7b)
                    let mut acc = super::tables::StaticAcc {
                        items: &mut items,
                        lines: &mut lines,
                        y: &mut y,
                        prev: &mut prev,
                        paras: &mut paras,
                        last_para: &mut last_para,
                        sf: &mut sf,
                    };
                    if self.static_table(cx, t, &child_ref(at, bi32), x, width, ps, y_origin, is_hf, hf_flow, bi == 0, nb, &mut acc)? {
                        prev_list = None;
                    }
                }
                SBlock::Rt(_) => return Err(LayoutError::Internal("static: block")),
            }
        }
        let _ = (&mut last_para, &mut prev_list);
        // the space after the last paragraph counts in the height (cell/header/footer)
        if let Some(pv) = &prev {
            y += pv.after;
        }
        // floating tables nested in the cell: drawn after its text (F7b)
        for i in 0..sf.float_items.len() {
            super::push_copy(&mut items, &sf.float_items, i)?;
        }
        lines.append(&mut sf.float_lines);
        let height = jm::max(y, sf.float_bottom);
        Ok(CellContent {
            items,
            lines,
            height,
            keep_next,
            paras: (!paras.is_empty()).then(|| Arc::new(paras)),
            nest: (!sf.nest.is_empty()).then(|| std::mem::take(&mut sf.nest)),
            align_h: if sf.align_bottom > height { OptF64::some(sf.align_bottom) } else { OptF64::NONE },
            shape: cell_shape,
        })
    }
}
