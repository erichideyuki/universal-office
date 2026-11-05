//! The flow of paragraphs (frontier F7a): `placeParagraph`, keep rules, contextual spacing,
//! paragraph border groups, line numbers, `lineRecord`, change bars (engine.ts:1599–2213,
//! 2514–2595, 3139–3250, 3932–3998, 4088–4145).
//!
//! **Runtime copies (decision F7a-2).** The TS makes paragraph objects while it lays out:
//! `joinDeleted(carried, next)` (a deleted paragraph mark), `joinHidden(join, next)` (a hidden
//! paragraph mark) and the line-number paragraph. They are [`RtPara`]s here — plain data over keys
//! of the call's `LDoc` (the base paragraph, the `ppr`/`props` overrides, the run keys, the
//! `REV_JOINS` offsets) — and are prepared through a scratch `LDoc` that relocates exactly the runs
//! they hold; the glyphs come back with the keys of the call's `LDoc` (`Glyph.obj`, `Glyph.ptab`),
//! so a model run keeps its identity through the copy (K1) and `Cx` keeps its shared `&LDoc`.

use std::sync::Arc;

use crate::error::{Frontier, LResult, LayoutError, LimitKind};
use crate::items::{ItemSide, PassItems, PassLine};
use crate::json::{JsonBuilder, JsonKey};
use crate::keys::{ContainerId, LParaId, RunKey};
use crate::ldoc::{LBlock, LDoc, LPara, ParaFlags, ParaSrc, RunSrc};
use crate::lines::{BreakKind, GlyphSrc, Line, LineBreaker, LineFrame, glyph_of};
use crate::model::{PPr, ParaProps, Paragraph, Run};
use crate::output::{GlyphMeta, LineParaFormat, LineRef};
use crate::para::{FieldSource, GlyphKind, LineRule, ParaEnv, ParaMarker, PrepFlow, PreparedPara, prepare_paragraph};
use crate::word;
use uo_jsnum::math as jm;
use uo_render::OptF64;
use uo_render::types::Item;

use super::page::SectGeom;
use super::{Cx, Engine, js_eq, splice_items};

/// `PrevPara` (engine.ts:154): the previous paragraph, for spacing collapse, contextual spacing
/// and border groups.
#[derive(Clone, Debug, Default)]
pub struct PrevPara {
    pub after: f64,
    pub after_auto: bool,
    /// the list (abstractNum) of a numbered paragraph
    pub list: Option<Box<str>>,
    pub style_id: Option<Box<str>>,
    pub contextual: bool,
    /// `pBdr` present
    pub p_bdr: bool,
    /// `pBdrKey`: `JSON.stringify([pBdr, ind])` with the JS rules (K9, `bdrKey`, engine.ts:4089)
    pub p_bdr_key: Option<JsonKey>,
    pub box_bottom_item_idx: Option<u32>,
    pub box_bottom: OptF64,
    /// `boxPage`: the items array the box went into (K2f)
    pub box_page: Option<ContainerId>,
}

/// `CtxNeighbor` (engine.ts:171).
#[derive(Clone, Debug, Default)]
pub struct CtxNeighbor {
    pub style_id: Option<Box<str>>,
}

/// `CellNeighbors` (engine.ts:173): before the first and after the last paragraph of a cell.
#[derive(Clone, Debug, Default)]
pub struct CellNeighbors {
    pub prev: Option<CtxNeighbor>,
    pub next: Option<CtxNeighbor>,
}

/// A paragraph object the engine holds: a paragraph of the derived document or a runtime copy.
#[derive(Clone, Debug)]
pub enum PSrc {
    Doc(LParaId),
    Rt(Box<RtPara>),
}

/// A paragraph copy made while laying out (`{ ...base, ppr?, props?, runs }`), over keys of the
/// call's `LDoc`. It has no `ParaId` (LA2): what the TS keys by paragraph object misses it.
#[derive(Clone, Debug)]
pub struct RtPara {
    /// the paragraph the copy spreads (its fields when not overridden)
    pub base: LParaId,
    pub ppr: Option<Arc<PPr>>,
    pub props: Option<Arc<ParaProps>>,
    pub runs: Vec<RunKey>,
    /// `REV_JOINS`
    pub rev_joins: Vec<u32>,
}

/// `HiddenJoin` (engine.ts:186): a hidden-mark paragraph waiting for the next one.
#[derive(Clone, Debug)]
pub struct HiddenJoin {
    pub para: PSrc,
    pub marker: Option<ParaMarker>,
    /// the run styles the marker glyphs point at (their `StyleIx` index this list; F7a-8)
    pub marker_styles: Vec<crate::para::RunStyle>,
}

/// The last paragraph border box (`lastBox`): its bottom and where it went (K2f).
#[derive(Clone, Copy, Debug)]
pub struct LastBox {
    pub bottom: f64,
    pub page: Option<ContainerId>,
}

/// One level of `nextBlockLead`.
enum Lead {
    Done(f64),
    Chain(f64),
}

/// `anchorAt` of `placeParagraph`: where the paragraph's anchored objects are measured from.
#[derive(Clone, Copy, Debug)]
struct AnchorAt {
    ps: crate::keys::PageIx,
    top: f64,
    x: f64,
    w: f64,
    para_ref: Option<f64>,
}

/// A line placed in the current fragment (`frag` of `placeParagraph`).
struct FragLine {
    line: Line,
    y: f64,
    frame_x: f64,
}

/// (`ppr`, `props`, `runs`, `REV_JOINS`) of a paragraph object.
type PFields<'l> = (Option<Arc<PPr>>, Arc<ParaProps>, &'l [RunKey], &'l [u32]);

pub(super) fn fields_of<'l>(ldoc: &'l LDoc<'_>, p: &'l PSrc) -> LResult<PFields<'l>> {
    let err = LayoutError::Internal("engine: paragraph not in the derived document");
    let (base, ppr0, props0, runs, joins) = match p {
        PSrc::Doc(k) => {
            let lp = ldoc.para(*k).ok_or(err.clone())?;
            (*k, None, None, &lp.runs[..], &lp.rev_joins[..])
        }
        PSrc::Rt(rt) => (rt.base, rt.ppr.clone(), rt.props.clone(), &rt.runs[..], &rt.rev_joins[..]),
    };
    let (mut ppr, mut props) = (ppr0, props0);
    let mut k = base;
    for _ in 0..64 {
        let lp = ldoc.para(k).ok_or(err.clone())?;
        ppr = ppr.or_else(|| lp.ppr.clone());
        props = props.or_else(|| lp.props.clone());
        match lp.src {
            ParaSrc::Model { para, .. } => {
                let ppr = ppr.or_else(|| para.ppr.clone());
                let props = props.unwrap_or_else(|| Arc::clone(&para.props));
                return Ok((ppr, props, runs, joins));
            }
            ParaSrc::Copy { base } => k = base,
        }
    }
    Err(err)
}

/// The model paragraph at the root of a derived paragraph (`Copy` chains end at one).
fn model_root<'a>(ldoc: &LDoc<'a>, mut k: LParaId) -> LResult<(&'a Paragraph, LParaId)> {
    for _ in 0..64 {
        match ldoc.para(k).map(|p| p.src) {
            Some(ParaSrc::Model { para, .. }) => return Ok((para, k)),
            Some(ParaSrc::Copy { base }) => k = base,
            None => break,
        }
    }
    Err(LayoutError::Internal("engine: paragraph without a model root"))
}

/// Duplicates the run `k` of `main` into `tiny` (its field results with it, F4-12); returns the
/// key in `tiny`.
fn relocate_run<'a>(main: &LDoc<'a>, tiny: &mut LDoc<'a>, back: &mut Vec<RunKey>, k: RunKey) -> LResult<RunKey> {
    let src = match main.run(k) {
        Some(RunSrc::Model { run, at }) => RunSrc::Model { run, at: at.clone() },
        Some(RunSrc::Synth(r)) => RunSrc::Synth(r.clone()),
        // continuation slices are made by the chains (F7b)
        Some(RunSrc::Slice { .. }) => return Err(LayoutError::Unported(Frontier::F7b, "a chain slice in a joined paragraph")),
        None => return Err(LayoutError::Internal("engine: run not in the derived document")),
    };
    let nk = RunKey(u32::try_from(tiny.runs.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?);
    tiny.runs.push(src);
    back.push(k);
    let mut j = 0u32;
    while let Some(fk) = main.field_result_key(k, j) {
        let src = match main.run(fk) {
            Some(RunSrc::Model { run, at }) => RunSrc::Model { run, at: at.clone() },
            _ => return Err(LayoutError::Internal("engine: field result")),
        };
        let fnk = RunKey(u32::try_from(tiny.runs.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?);
        tiny.runs.push(src);
        back.push(fk);
        tiny.field_results.insert((nk, j), fnk);
        j += 1;
    }
    Ok(nk)
}

/// The `Cx` of the same call over another derived document (the scratch `LDoc` of a copy).
macro_rules! with_ldoc {
    ($cx:expr, $ldoc:expr) => {
        Cx {
            ldoc: $ldoc,
            input: $cx.input,
            opts: $cx.opts,
            fonts: &mut *$cx.fonts,
            provider: $cx.provider,
            resolver: &mut *$cx.resolver,
            res: &mut *$cx.res,
            hyph: &mut *$cx.hyph,
            budget: &mut *$cx.budget,
            depth: $cx.depth,
            line_seq: &mut *$cx.line_seq,
            images: &mut *$cx.images,
        }
    };
}

pub(super) use with_ldoc;

/// Takes the marker out of a prepared paragraph with the run styles its glyphs point at (the TS
/// marker glyphs hold their `RunStyle` objects; here a `StyleIx` is an index into its paragraph's
/// list, so a marker that moves to another paragraph carries its styles, F7a-8).
pub(super) fn detach_marker(p: &mut PreparedPara) -> (Option<ParaMarker>, Vec<crate::para::RunStyle>) {
    let Some(mut m) = p.marker.take() else { return (None, Vec::new()) };
    let mut styles: Vec<crate::para::RunStyle> = Vec::new();
    let mut map: Vec<(u32, u32)> = Vec::new();
    for g in &mut m.glyphs {
        let old = g.style.0;
        let new = match map.iter().find(|(o, _)| *o == old) {
            Some(&(_, n)) => n,
            None => {
                let n = u32::try_from(styles.len()).unwrap_or(u32::MAX);
                styles.push(p.styles.get(old as usize).cloned().unwrap_or_default());
                map.push((old, n));
                n
            }
        };
        g.style = crate::para::StyleIx(new);
    }
    (Some(m), styles)
}

/// `p.marker = join.marker`: the moved marker's styles become new entries of `p.styles` (new
/// objects for `p`, as the TS ones are not `p`'s).
pub(super) fn attach_marker(p: &mut PreparedPara, m: Option<ParaMarker>, styles: &[crate::para::RunStyle]) {
    let Some(mut m) = m else {
        p.marker = None;
        return;
    };
    let base = u32::try_from(p.styles.len()).unwrap_or(u32::MAX);
    p.styles.extend_from_slice(styles);
    for g in &mut m.glyphs {
        g.style = crate::para::StyleIx(base.saturating_add(g.style.0));
    }
    p.marker = Some(m);
}

/// `bdrKey(ppr)` (engine.ts:4089): `JSON.stringify([ppr.pBdr, ppr.ind ?? null])`.
pub(super) fn bdr_key(ppr: &PPr) -> Option<JsonKey> {
    let b = ppr.p_bdr.as_deref()?;
    let mut j = JsonBuilder::new();
    j.begin_array();
    match crate::model::JsonWriter::stringify(b, None) {
        Some(s) => j.raw(&s),
        None => j.null(),
    };
    match ppr.ind.as_deref().and_then(|i| crate::model::JsonWriter::stringify(i, None)) {
        Some(s) => j.raw(&s),
        None => j.null(),
    };
    j.end_array();
    Some(j.finish())
}

fn same_key(a: Option<&JsonKey>, b: Option<&JsonKey>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => JsonKey::json_eq(a, b),
        _ => false,
    }
}

/// `b.val !== 'none' && b.val !== 'nil'` of a border that is there.
fn border_on(b: Option<&crate::model::BorderSpec>) -> Option<&crate::model::BorderSpec> {
    b.filter(|b| &*b.val != "none" && &*b.val != "nil")
}

/// `(b.space ?? 0)`.
fn space_of(b: &crate::model::BorderSpec) -> f64 {
    b.space.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0)
}

/// `/^heading ([1-9])$/i` → the digit.
fn heading_level(name: &str) -> Option<f64> {
    let b = name.as_bytes();
    if b.len() != 9 || !b.get(..8).is_some_and(|h| h.eq_ignore_ascii_case(b"heading ")) {
        return None;
    }
    match b.get(8) {
        Some(d @ b'1'..=b'9') => Some(f64::from(d - b'0')),
        _ => None,
    }
}

/// `p.afterAuto ? 280 : p.spaceAfter`.
fn after_of(p: &PreparedPara) -> f64 {
    if p.after_auto { 280.0 } else { p.space_after }
}

/// `p.widowControl || p.ppr.widowControl === undefined`.
fn widow_on(p: &PreparedPara) -> bool {
    p.widow_control || p.ppr.widow_control.is_none()
}

/// The paragraph frame of a line: `{ left, right }` (no float on the page; `wrapFrame` with floats
/// is F7b).
pub(super) fn plain_frame(left: f64, right: f64) -> LineFrame {
    LineFrame { left, right, gap: false, next: None }
}

impl Engine {
    /// The block `i` of the story the flow walks.
    fn block_at(&self, cx: &Cx<'_, '_>, i: usize) -> Option<LBlock> {
        // (the endnote flow's blocks while it comes out, F7c)
        self.flow_block(cx, i)
    }

    /// Prepares a paragraph object (`prepareParagraph(this.ctx, para, env)`).
    pub(crate) fn prepare(&mut self, cx: &mut Cx<'_, '_>, para: &PSrc, env: &ParaEnv) -> LResult<PreparedPara> {
        match para {
            PSrc::Doc(k) => prepare_paragraph(cx, self, *k, env),
            PSrc::Rt(rt) => self.prepare_rt(cx, rt, env),
        }
    }

    /// Prepares a runtime copy through a scratch `LDoc` holding its root model paragraph and its
    /// relocated runs; the glyphs come back with the keys of the call's `LDoc`.
    fn prepare_rt(&mut self, cx: &mut Cx<'_, '_>, rt: &RtPara, env: &ParaEnv) -> LResult<PreparedPara> {
        let main = cx.ldoc;
        let (root, root_k) = model_root(main, rt.base)?;
        let (ppr, props) = {
            let probe = PSrc::Rt(Box::new(RtPara { runs: Vec::new(), rev_joins: Vec::new(), ..rt.clone() }));
            let (a, b, _, _) = fields_of(main, &probe)?;
            (a, b)
        };
        let base_lp = main.para(root_k).ok_or(LayoutError::Internal("engine: base paragraph"))?;
        let mut tiny = LDoc::empty(main.view);
        let mut back = Vec::with_capacity(rt.runs.len());
        let mut keys = Vec::with_capacity(rt.runs.len());
        for &k in &rt.runs {
            keys.push(relocate_run(main, &mut tiny, &mut back, k)?);
        }
        tiny.paras.push(LPara {
            src: ParaSrc::Model { para: root, id: root.id },
            ppr: None,
            props: None,
            runs: Vec::new(),
            rev_joins: Vec::new(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: base_lp.story,
            path: base_lp.path.clone(),
        });
        tiny.paras.push(LPara {
            src: ParaSrc::Copy { base: LParaId(0) },
            // an absent `ppr` stays absent: the copy reads the root's (the same `undefined`)
            ppr,
            props: Some(props),
            runs: keys,
            rev_joins: rt.rev_joins.clone(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: base_lp.story,
            path: base_lp.path.clone(),
        });
        let mut tcx = with_ldoc!(cx, &tiny);
        let mut p = prepare_paragraph(&mut tcx, self, LParaId(1), env)?;
        let map = |k: Option<RunKey>| k.and_then(|k| back.get(k.ix()).copied());
        for g in &mut p.glyphs {
            g.obj = map(g.obj);
            g.ptab = map(g.ptab);
        }
        if let Some(m) = &mut p.marker {
            for g in &mut m.glyphs {
                g.obj = map(g.obj);
                g.ptab = map(g.ptab);
            }
        }
        p.para = rt.base;
        Ok(p)
    }

    /// `joinDeleted(carried, next)` (revisions.ts:155): the text of the deleted-mark paragraphs in
    /// front, the formatting of the next one; the join offset kept.
    pub(super) fn join_deleted(&self, cx: &Cx<'_, '_>, carried: Vec<RunKey>, next: LParaId) -> LResult<PSrc> {
        let lp = cx.ldoc.para(next).ok_or(LayoutError::Internal("engine: paragraph"))?;
        let n = carried.iter().fold(0u32, |n, k| n.wrapping_add(u32::try_from(cx.ldoc.run_text(*k).len()).unwrap_or(u32::MAX)));
        let mut runs = carried;
        runs.extend_from_slice(&lp.runs);
        Ok(PSrc::Rt(Box::new(RtPara { base: next, ppr: None, props: None, runs, rev_joins: vec![n] })))
    }

    /// `resolvePPr(ctx, para, env).ppr` (para.ts:681).
    fn resolve_ppr_of(&mut self, cx: &Cx<'_, '_>, para: &PSrc, env: &ParaEnv) -> LResult<word::ResolvedPPr> {
        let (ppr, props, _, _) = fields_of(cx.ldoc, para)?;
        Ok(word::resolve_ppr(cx.ldoc.view.formatting(), &mut self.ctx.resolver, ppr.as_deref(), &props, env.table_ppr.as_deref()))
    }

    /// `joinHidden(join, next, env)` (engine.ts:1730): one paragraph with the properties of the
    /// first and the mark of the next; the next one's number is consumed too.
    pub(super) fn join_hidden(&mut self, cx: &Cx<'_, '_>, join: &HiddenJoin, next: &PSrc, env: &ParaEnv) -> LResult<PSrc> {
        let r = self.resolve_ppr_of(cx, next, env)?;
        if let Some(n) = r.ppr.num_pr.as_deref()
            && n.num_id.is_some_and(crate::model::JsNumber::truthy)
            && env.numbering != Some(false)
        {
            let ilvl = n.ilvl.filter(|v| !v.is_null()).unwrap_or(crate::model::JsNumber::Num(0.0));
            let fmt = cx.ldoc.view.formatting();
            self.ctx.flow.numbering.next(fmt, n.num_id.unwrap_or(crate::model::JsNumber::Num(0.0)), ilvl);
        }
        let (jppr, _, jruns, joins) = fields_of(cx.ldoc, &join.para)?;
        let (nppr, _, nruns, _) = fields_of(cx.ldoc, next)?;
        let mut ppr = jppr.as_deref().cloned().unwrap_or_default();
        match nppr.as_deref().and_then(|p| p.r_pr.clone()) {
            Some(rpr) => word::obj::set(&mut ppr, "rPr", |p| p.r_pr = Some(rpr)),
            None => word::obj::delete(&mut ppr, "rPr"),
        }
        let base = match &join.para {
            PSrc::Doc(k) => *k,
            PSrc::Rt(rt) => rt.base,
        };
        let props = match &join.para {
            PSrc::Rt(rt) => rt.props.clone(),
            PSrc::Doc(_) => None,
        };
        let mut runs = jruns.to_vec();
        runs.extend_from_slice(nruns);
        Ok(PSrc::Rt(Box::new(RtPara { base, ppr: Some(Arc::new(ppr)), props, runs, rev_joins: joins.to_vec() })))
    }

    /// `noShownRuns(p)` (engine.ts:1739): no run shows in this view.
    pub(super) fn no_shown_runs(&self, cx: &Cx<'_, '_>, p: LParaId) -> bool {
        let Some(lp) = cx.ldoc.para(p) else { return true };
        lp.runs.iter().all(|k| {
            crate::paint::model_run(cx.ldoc, *k)
                .is_none_or(|r| crate::para::is_mark_run(r) || matches!(self.ctx.rev.run(&r.props), Some(word::RunRevision::Hide)))
        })
    }

    /// `collapse(after, before)` (engine.ts:1614).
    pub(super) fn collapse(&self, cx: &Cx<'_, '_>, after: f64, before: f64) -> f64 {
        if cx.ldoc.view.formatting().settings.compat.get("doNotUseHTMLParagraphAutoSpacing") == Some(&true) {
            return after + before;
        }
        jm::max(after, before)
    }

    /// `borderTopExtent(p, prev)` (engine.ts:1624).
    pub(super) fn border_top_extent(p: &PreparedPara, prev: Option<&PrevPara>) -> f64 {
        let b = p.ppr.p_bdr.as_deref();
        if let Some(prev) = prev
            && prev.p_bdr_key.is_some()
            && same_key(prev.p_bdr_key.as_ref(), bdr_key(&p.ppr).as_ref())
        {
            return match border_on(b.and_then(|b| b.between.as_deref())) {
                Some(bt) => 2.0 * space_of(bt) * 20.0 + crate::paint::border_width(Some(bt)),
                None => 0.0,
            };
        }
        let Some(top) = border_on(b.and_then(|b| b.top.as_deref())) else { return 0.0 };
        // with a shadow the width counts twice (oracle pbdrshadow)
        space_of(top) * 20.0 + crate::paint::border_width(Some(top)) * if top.shadow == Some(true) { 2.0 } else { 1.0 }
    }

    /// `borderBottomExtent(p, groupedNext)` (engine.ts:1635).
    pub(super) fn border_bottom_extent(p: &PreparedPara, grouped_next: bool) -> f64 {
        let b = p.ppr.p_bdr.as_deref();
        let Some(bot) = border_on(b.and_then(|b| b.bottom.as_deref())) else { return 0.0 };
        if grouped_next {
            return 0.0;
        }
        space_of(bot) * 20.0 + crate::paint::border_width(Some(bot)) * if bot.shadow == Some(true) { 2.0 } else { 1.0 }
    }

    /// `groupsWithNext(p, next, env)` (engine.ts:1642).
    pub(super) fn groups_with_next(&mut self, cx: &Cx<'_, '_>, p: &PreparedPara, next: Option<LBlock>, env: &ParaEnv) -> LResult<bool> {
        let Some(LBlock::Paragraph(n)) = next else { return Ok(false) };
        if p.ppr.p_bdr.is_none() {
            return Ok(false);
        }
        let r = self.resolve_ppr_of(cx, &PSrc::Doc(n), env)?;
        let (a, b) = (bdr_key(&r.ppr), bdr_key(&p.ppr));
        Ok(match (a, b) {
            (Some(a), Some(b)) => JsonKey::json_eq(&a, &b),
            (None, None) => true,
            _ => false,
        })
    }

    /// `listOf(p)` (engine.ts:1704): the list (abstractNum) of a numbered paragraph.
    pub(super) fn list_of(&self, cx: &Cx<'_, '_>, p: &PreparedPara) -> Option<Box<str>> {
        let id = p.ppr.num_pr.as_deref()?.num_id?;
        if !id.truthy() {
            return None;
        }
        self.ctx.flow.numbering.list_key(cx.ldoc.view.formatting(), id)
    }

    /// `gridFor(p)` (engine.ts:1710): the line pitch of the document grid (snapToGrid=0 off).
    fn grid_for(&self, p: &PreparedPara) -> Option<f64> {
        let ps = self.cur.and_then(|c| self.pages.get(c.ix()))?;
        let pitch = self.geoms.get(ps.geom as usize)?.line_pitch.get()?;
        (uo_jsnum::conv::truthy(pitch) && p.ppr.snap_to_grid != Some(false)).then_some(pitch)
    }

    /// `paragraphLines(p, width)` (engine.ts:2514).
    fn paragraph_lines(&self, cx: &mut Cx<'_, '_>, p: &PreparedPara, width: f64) -> LResult<Vec<Line>> {
        let mut br = LineBreaker::new(p, self.grid_for(p));
        let mut ls = Vec::new();
        loop {
            ls.push(br.next(cx, p, plain_frame(p.ind_left, width - p.ind_right))?);
            if br.done(p) {
                break;
            }
        }
        Ok(ls)
    }

    /// `closingFit(p, l)` (engine.ts:2523).
    fn closing_fit(p: &PreparedPara, l: &Line) -> f64 {
        if matches!(p.line_rule, LineRule::Exact | LineRule::AtLeast) { l.height } else { jm::min(l.height, l.baseline + l.desc) }
    }

    /// `nextBlockLead(blockIdx, g, width)` (engine.ts:2536): the height of the next block's first
    /// line(s) that must stay with a keepNext paragraph (a pure preview: lists, SEQ and notes are
    /// restored). The TS recursion over a keepNext chain is a loop here (F7a-5): the inner
    /// save/restore pairs are invisible under the outer one, and the sum is folded from the
    /// innermost term out, as `gap + h + nextBlockLead(…)` adds.
    fn next_block_lead(&mut self, cx: &mut Cx<'_, '_>, block_idx: usize, g: &SectGeom, width: f64) -> LResult<f64> {
        if !self.lead_bound(cx, block_idx, g) {
            return Ok(0.0);
        }
        cx.budget.pure_clones += 1;
        let saved = self.ctx.save_flow();
        let mut terms: Vec<f64> = Vec::new();
        let mut at = block_idx;
        let r = loop {
            match self.next_block_lead_step(cx, at, width) {
                Ok(Lead::Done(v)) => break Ok(v),
                Ok(Lead::Chain(v)) => {
                    terms.push(v);
                    at += 1;
                    if !self.lead_bound(cx, at, g) {
                        break Ok(0.0);
                    }
                }
                Err(e) => break Err(e),
            }
        };
        self.ctx.restore_flow(&saved);
        Ok(terms.iter().rev().fold(r?, |acc, t| t + acc))
    }

    /// `nb && blockIdx + 1 <= g.blocks[1]` of `nextBlockLead`.
    fn lead_bound(&self, cx: &Cx<'_, '_>, block_idx: usize, g: &SectGeom) -> bool {
        self.block_at(cx, block_idx + 1).is_some() && f64::from(u32::try_from(block_idx + 1).unwrap_or(u32::MAX)) <= g.blocks.1
    }

    /// One level of `nextBlockLead`: the lead of block `block_idx + 1`, or `gap + height` of a
    /// keepNext paragraph whose own lead follows.
    fn next_block_lead_step(&mut self, cx: &mut Cx<'_, '_>, block_idx: usize, width: f64) -> LResult<Lead> {
        cx.budget.charge(1)?;
        let nb = self.block_at(cx, block_idx + 1).ok_or(LayoutError::Internal("nextBlockLead: block"))?;
        let none = ParaEnv::default();
        // the keepNext paragraph's own "after" counts up to the next one; contextual with the
        // next one in the same style drops it (oracle keepgap)
        let from = match self.block_at(cx, block_idx) {
            Some(LBlock::Paragraph(k)) => Some(prepare_paragraph(cx, self, k, &none)?),
            _ => None,
        };
        let mut after = from.as_ref().map_or(0.0, after_of);
        let q = match nb {
            LBlock::Table { block } => {
                // a title before a table: it stays with the first row (F7b, oracle keeptbl)
                if self.en_blocks.is_some() {
                    return Err(LayoutError::Unported(Frontier::F7b, "nextBlockLead: a table in the endnote flow"));
                }
                let t = super::tables::story_table(cx, self.story, block)?;
                return Ok(Lead::Done(self.table_lead(cx, t, width, from.as_ref(), after)?));
            }
            LBlock::Paragraph(k) => prepare_paragraph(cx, self, k, &none)?,
        };
        let mut before = q.space_before;
        let mut ctx_after: Option<f64> = None;
        if let Some(from) = &from
            && from.style_id == q.style_id
        {
            if from.contextual {
                ctx_after = Some(after);
                after = 0.0;
            }
            if q.contextual {
                before = 0.0;
            }
        }
        let gap = match ctx_after {
            Some(ca) => jm::max(0.0, self.collapse(cx, ca, before) - ca),
            None => self.collapse(cx, after, before),
        };
        if q.keep_next {
            let h = self.paragraph_height(cx, &q, width)?;
            return Ok(Lead::Chain(gap + h));
        }
        // the next one starts on this page only with the lines it cannot leave alone: whole with
        // keepLines; with widow/orphan control 2 lines (all of them up to 3) — oracle keepwidow
        if q.keep_lines {
            let ls = self.paragraph_lines(cx, &q, width)?;
            let last = ls.last().ok_or(LayoutError::Internal("nextBlockLead: no line"))?;
            let sum = ls.iter().fold(Self::border_top_extent(&q, None), |a, l| a + l.height);
            return Ok(Lead::Done(gap + sum - last.height + Self::closing_fit(&q, last)));
        }
        let mut br = LineBreaker::new(&q, self.grid_for(&q));
        let mut qs = Vec::new();
        loop {
            qs.push(br.next(cx, &q, plain_frame(q.ind_left, width - q.ind_right))?);
            if br.done(&q) || qs.len() >= 4 {
                break;
            }
        }
        let n = if !widow_on(&q) {
            1
        } else if br.done(&q) && qs.len() <= 3 {
            qs.len()
        } else {
            2
        };
        let take = qs.get(..n).ok_or(LayoutError::Internal("nextBlockLead: lines"))?;
        let last = take.last().ok_or(LayoutError::Internal("nextBlockLead: no line"))?;
        Ok(Lead::Done(gap + take.iter().fold(0.0, |a, l| a + l.height) - last.height + Self::closing_fit(&q, last)))
    }

    /// `paragraphHeight(p, width)` (engine.ts:2528).
    fn paragraph_height(&self, cx: &mut Cx<'_, '_>, p: &PreparedPara, width: f64) -> LResult<f64> {
        let mut br = LineBreaker::new(p, self.grid_for(p));
        let mut h = Self::border_top_extent(p, None) + Self::border_bottom_extent(p, false);
        loop {
            h += br.next(cx, p, plain_frame(p.ind_left, width - p.ind_right))?.height;
            if br.done(p) {
                break;
            }
        }
        Ok(h)
    }

    /// `paintLineNumber(colX, baseline)` (engine.ts:1658): the line number (w:lnNumType), right
    /// aligned at (column − distance), same baseline, style Normal + "Line Number".
    fn paint_line_number(&mut self, cx: &mut Cx<'_, '_>, col_x: f64, baseline: f64) -> LResult<()> {
        let cur = self.cur_ix()?;
        let gi = self.ps(cur)?.geom;
        let g = self.geom(gi)?;
        let ln = g.ln_num.clone().ok_or(LayoutError::Internal("paintLineNumber: no lnNumType"))?;
        let idx = g.index;
        let new_sect = self.ln_sect != i64::from(idx);
        let fresh = self.ln_sect < 0
            || (new_sect && idx.checked_sub(1).and_then(|i| self.geoms.get(i as usize)).is_none_or(|p| p.ln_num.is_none()));
        let restart = fresh || (&*ln.restart == "newSection" && new_sect) || (&*ln.restart == "newPage" && self.ln_page != Some(cur));
        if restart {
            self.ln_count = ln.start;
        }
        self.ln_page = Some(cur);
        self.ln_sect = i64::from(idx);
        self.ln_count += 1.0;
        if !js_eq(jm::rem(self.ln_count, ln.count_by), 0.0) {
            return Ok(());
        }
        let fmt = cx.ldoc.view.formatting();
        let sid = fmt
            .styles
            .styles
            .values()
            .find(|d| &*d.r#type == "character" && d.name.as_deref().is_some_and(|n| uo_text::case::to_lowercase_str(n) == "line number"))
            .map(|d| d.style_id.clone());
        let mut rpr = crate::model::RPr::default();
        if let Some(sid) = sid {
            word::obj::set(&mut rpr, "rStyle", |r| r.r_style = Some(sid));
        }
        let text: Vec<u16> = uo_jsnum::format::to_js_string(self.ln_count).encode_utf16().collect();
        let run = Run {
            text: crate::model::Utf16::from_units(text),
            props: Arc::new(uo_docx::read::props_map::run_props_from_rpr(Some(&rpr))),
            rpr: Some(Arc::new(rpr)),
            ..Run::default()
        };
        // `{ runs: [run], props: {}, ppr: {} }`: no model id (F7a-4)
        let para = Paragraph { runs: vec![run], ppr: Some(Arc::new(PPr::default())), ..Paragraph::empty(crate::model::ParaId(u32::MAX)) };
        let mut tiny = LDoc::empty(cx.ldoc.view);
        tiny.runs.push(RunSrc::Synth(Box::new(para.runs.first().cloned().unwrap_or_default())));
        tiny.paras.push(LPara {
            src: ParaSrc::Model { para: &para, id: para.id },
            ppr: None,
            props: None,
            runs: vec![RunKey(0)],
            rev_joins: Vec::new(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: crate::model::StoryRef::Body,
            path: crate::model::BlockPath::root(0),
        });
        let mut tcx = with_ldoc!(cx, &tiny);
        let np = prepare_paragraph(&mut tcx, self, LParaId(0), &ParaEnv { numbering: Some(false), ..ParaEnv::default() })?;
        let line = LineBreaker::new(&np, None).next(&mut tcx, &np, plain_frame(0.0, 20000.0))?;
        let x = col_x - ln.distance - line.width;
        let at = self.ps(cur)?.body.len();
        let mut body = std::mem::take(&mut self.ps_mut(cur)?.body);
        let r = crate::paint::paint_line(
            &mut tcx,
            &np,
            &line,
            crate::paint::LinePlacement { frame_x: x, top: baseline - line.baseline, hf: false },
            &mut body,
        );
        for k in at..body.len() {
            if matches!(body.list.items.get(k), Some(Item::Glyphs { .. }))
                && let Some(s) = body.side.get_mut(k)
            {
                let lb = s.glyph.and_then(|g| g.line_box);
                s.glyph = Some(GlyphMeta { line_box: lb, line_num: true });
            }
        }
        self.ps_mut(cur)?.body = body;
        r
    }

    /// `paintBox(out, p, x, width, top, bottom, prev, at, groupedNext, page)` (engine.ts:1687):
    /// the paragraph's shading/borders, inserted at `at` (before the text already painted).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(super) fn paint_box(
        &mut self,
        cx: &mut Cx<'_, '_>,
        out: &mut PassItems,
        p: &PreparedPara,
        x: f64,
        width: f64,
        mut top: f64,
        bottom: f64,
        prev: Option<&PrevPara>,
        at: usize,
        grouped_next: bool,
        page: Option<ContainerId>,
    ) -> LResult<()> {
        let has_fill = word::shading_fill(p.ppr.shd.as_deref(), &cx.ldoc.view.formatting().theme).is_some_and(|f| !f.is_empty());
        self.last_box = Some(LastBox { bottom, page });
        if p.ppr.p_bdr.is_none() && !has_fill {
            return Ok(());
        }
        let key = bdr_key(&p.ppr);
        let grouped = prev.is_some_and(|pv| pv.p_bdr_key.is_some() && same_key(pv.p_bdr_key.as_ref(), key.as_ref()));
        // in a group the box starts at the foot of the one before: the sides run through the gap
        if grouped
            && let Some(pv) = prev
            && let Some(bb) = pv.box_bottom.get()
            && pv.box_page == page
            && bb < top
        {
            top = bb;
        }
        let left = x + p.ind_left + jm::min(0.0, p.first_line);
        let right = x + width - p.ind_right;
        let mut bx = PassItems::new();
        crate::paint::paint_para_box(cx, &mut bx, p, p.ppr.p_bdr.as_deref(), left, right, top, bottom, !grouped, !grouped_next, grouped)?;
        splice_items(out, at, &bx)
    }

    /// `placeParagraph(para0, ref, blockIdx, g)` (engine.ts:1743).
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    pub(crate) fn place_paragraph(&mut self, cx: &mut Cx<'_, '_>, para0: LParaId, block_idx: usize, gi: u32) -> LResult<()> {
        let g = self.geom(gi)?.clone();
        let bi = f64::from(u32::try_from(block_idx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?);
        let fmt = cx.ldoc.view.formatting();
        let settings = &fmt.settings;
        let compat = settings.compatibility_mode.to_number();
        // a frame: out of the flow (the box goes with the anchor paragraph; without an anchor it
        // stays here — liftFrames, F7b)
        if let Some(lp) = cx.ldoc.para(para0)
            && lp.flags.has(ParaFlags::FRAME_SKIP)
        {
            if let Some(carry) = lp.frame_carry {
                let fr = self.col_frame()?;
                let cur = self.cur_ix()?;
                let y = self.y;
                self.place_float(cx, carry, cur, fr.x, fr.w, y, None, 0.0, None, None)?;
            }
            return Ok(());
        }
        // (the paragraph object's own fields: a copy, like the endnote separator, has no model
        // fields of its own — F7c)
        let (p0_ppr, p0_props) = {
            let (a, b, _, _) = fields_of(cx.ldoc, &PSrc::Doc(para0))?;
            (a, b)
        };
        let next_block = self.block_at(cx, block_idx + 1);
        let next_is_para = matches!(next_block, Some(LBlock::Paragraph(_)));
        let mark_gone = self.ctx.rev.mark_gone(&p0_props);
        // a deleted mark (revision): the text goes to the next paragraph, which keeps ITS
        // properties; the deleted one's number does not count (oracle delmark)
        if mark_gone && self.pending_join.is_none() && bi < g.blocks.1 && next_is_para {
            let runs = cx.ldoc.para(para0).map(|p| p.runs.clone()).unwrap_or_default();
            self.pending_del.get_or_insert_with(Vec::new).extend(runs);
            return Ok(());
        }
        if mark_gone && self.no_shown_runs(cx, para0) && self.pending_join.is_none() && self.pending_del.is_none() {
            return Ok(());
        }
        let join = self.pending_join.take();
        let cur0 = self.cur_ix()?;
        let env = ParaEnv { field: Some(FieldSource::Page), ..ParaEnv::default() };
        let merged = match self.pending_del.take() {
            Some(carried) => self.join_deleted(cx, carried, para0)?,
            None => PSrc::Doc(para0),
        };
        let para = match &join {
            Some(j) => self.join_hidden(cx, j, &merged, &env)?,
            None => merged,
        };
        // the `fieldValue(this.cur, blockIdx)` closure of `env`
        self.field_at = Some((cur0, i64::from(u32::try_from(block_idx).unwrap_or(u32::MAX))));
        let penv = if join.is_some() { ParaEnv { numbering: Some(false), ..env.clone() } } else { env.clone() };
        let mut p = self.prepare(cx, &para, &penv)?;
        if let Some(j) = &join {
            attach_marker(&mut p, j.marker.clone(), &j.marker_styles);
        }
        // a numbered heading (style Heading N): the chapter number of the PAGE field
        if let (Some(ch), Some(sid)) = (p.marker.as_ref().and_then(|m| m.chapter.as_ref()), p.style_id.as_deref())
            && let Some(lv) = fmt.styles.styles.get(sid).and_then(|s| s.name.as_deref()).and_then(heading_level)
        {
            let r = self.flow_ref(block_idx);
            self.found.heading_marks.insert(r.to_ts().into(), (lv, ch.clone()));
        }
        // a heading ("heading N" style) laid out: SEQ \s n of the following paragraphs resets
        let hl = heading_style_level_of(cx, &p0_props, p0_ppr.as_deref());
        if hl > 0.0 {
            self.ctx.flow.heads.push(hl);
        }
        // hidden text (oracle hidden): a hidden-mark paragraph without visible content takes
        // nothing; with visible content it joins the next one (joinHidden)
        let mark_hidden = p.styles.get(p.mark_style.0 as usize).is_some_and(|s| s.hidden);
        if mark_hidden && bi < g.blocks.1 {
            if p.glyphs.is_empty() {
                return Ok(());
            }
            if next_is_para {
                let (marker, marker_styles) = detach_marker(&mut p);
                self.pending_join = Some(HiddenJoin { para, marker, marker_styles });
                return Ok(());
            }
        }
        // an EMPTY paragraph that only carries the break to a CONTINUOUS section: no height and
        // no spacing of its own, but it separates its neighbours (oracle colbreak, cb-emptysect-*)
        // (`this.geoms[g.index + 1]`: the section geometries only — the endnote flows' copies come
        // after them, F7c)
        let next_geom = self.geoms.get(g.index as usize + 1).filter(|_| g.index + 1 < self.sect_geoms);
        let prev_is_para = block_idx.checked_sub(1).and_then(|i| self.block_at(cx, i)).is_some_and(|b| matches!(b, LBlock::Paragraph(_)));
        if js_eq(bi, g.blocks.1)
            && p.glyphs.is_empty()
            && self.en_flow.is_none()
            && next_geom.is_some_and(|n| &*n.kind == "continuous")
            && (prev_is_para || compat < 15.0)
        {
            let sb = if p.before_auto { 280.0 } else { p.space_before };
            let sa = after_of(&p);
            if let Some(prev) = &self.prev
                && !self.at_top
            {
                self.y += jm::max(0.0, self.collapse(cx, prev.after, sb) - sb);
            }
            self.prev = Some(PrevPara { after: 0.0, after_auto: false, contextual: false, ..PrevPara::default() });
            // the next section in several columns: the column's first paragraph takes the whole
            // "before" (corpus t70)
            self.sect_gap_after = if next_geom.map_or(1, |n| n.cols.len()) > 1 { None } else { Some(sa) };
            return Ok(());
        }
        // the document's FINAL mark hidden and without visible content takes nothing either
        if mark_hidden && p.glyphs.is_empty() && js_eq(bi, self.story_len(cx) - 1.0) && !self.at_top {
            return Ok(());
        }
        // a paragraph that STARTS with a page break: Word takes the whole paragraph to the next
        // page; the break leaves no line on the page above (oracle tocref-pbstart)
        if p.glyphs.first().is_some_and(|gl| gl.kind == GlyphKind::Pb) && p.glyphs.len() > 1 {
            let last_after = self.prev.as_ref().map_or(0.0, |pv| pv.after);
            p.glyphs.remove(0);
            let cg = self.cur_ps()?.geom;
            self.new_page(cx, cg, "nextPage")?;
            self.top_gap_prev_after = Some(last_after);
        }
        // a numbered paragraph with only the page break: Word draws no number (oracle numpgbr)
        if p.marker.is_some()
            && p.glyphs.len() == 1
            && p.glyphs.first().is_some_and(|gl| gl.kind == GlyphKind::Pb)
            && !(compat < 15.0 && settings.compat.get("splitPgBreakAndParaMark") == Some(&true))
        {
            p.marker = None;
        }
        // page break before (space before at the top: dropped in mode 15, kept in ≤ 14)
        if p.page_break_before && !self.at_top {
            let last_after = self.prev.as_ref().map_or(0.0, |pv| pv.after);
            let cg = self.cur_ps()?.geom;
            self.new_page(cx, cg, "nextPage")?;
            if compat < 15.0 {
                self.top_gap_prev_after = Some(last_after);
            }
        }
        // keepNext / keepLines: decided before placing the first line
        let frame_base = self.col_frame()?;
        let mut before = if p.before_auto { if self.first_in_doc { 0.0 } else { 280.0 } } else { p.space_before };
        let mut prev_after = self.prev.as_ref().map_or(0.0, |pv| pv.after);
        // the previous one's "after" that ITS contextual spacing dropped: this one's "before" is
        // worth only what exceeds it (oracle ctxmark)
        let mut ctx_after: Option<f64> = None;
        if let Some(prev) = &self.prev
            && prev.style_id == p.style_id
        {
            if prev.contextual {
                ctx_after = Some(prev_after);
                prev_after = 0.0;
            }
            if p.contextual {
                before = 0.0;
            }
        }
        // automatic ("HTML") spacing between items of the same list: zero (oracle autosplist)
        let list = self.list_of(cx, &p).filter(|l| !l.is_empty());
        if let (Some(l), Some(prev)) = (&list, &self.prev)
            && prev.list.as_deref() == Some(&**l)
        {
            if p.before_auto {
                before = 0.0;
            }
            if prev.after_auto {
                prev_after = 0.0;
            }
        }
        // top of page: space before dropped, except for the document's first paragraph and after
        // a section break / a break at the paragraph start (oracle tocref-sectgap)
        let mut gap = self.collapse(cx, prev_after, before);
        if let Some(ca) = ctx_after {
            gap = jm::max(0.0, self.collapse(cx, ca, before) - ca);
        }
        if let Some(sga) = self.sect_gap_after.take() {
            gap = jm::max(0.0, self.collapse(cx, sga, before) - sga);
        }
        let keep_after = self.top_gap_prev_after.take();
        if self.at_top && !self.first_in_doc {
            gap = match keep_after {
                Some(ka) => jm::max(0.0, self.collapse(cx, ka, before) - ka),
                None => 0.0,
            };
        }
        // keepNext without keepLines (mode 15): from this line on the paragraph goes down with
        // the next one
        let mut tail_at: i64 = -1;
        if p.keep_lines || p.keep_next {
            let ls = self.paragraph_lines(cx, &p, frame_base.w)?;
            let last_l = ls.last().ok_or(LayoutError::Internal("placeParagraph: no line"))?;
            let full_h = ls.iter().fold(Self::border_top_extent(&p, None) + Self::border_bottom_extent(&p, false), |a, l| a + l.height);
            // the line that closes the block need only fit to its text (oracle keepfit)
            let closed_h = full_h - last_l.height + Self::closing_fit(&p, last_l);
            let need = gap + if p.keep_next { full_h + self.next_block_lead(cx, block_idx, &g, frame_base.w)? } else { closed_h };
            let ps = self.cur_ps()?;
            let full = ps.body_bottom - self.col_top;
            let avail = ps.body_bottom - self.y;
            if need > avail && need <= full && !self.at_top {
                // keepNext WITHOUT keepLines (oracle keepsplit): a paragraph that does not fit whole
                // splits as usual; if it fits and only the next one's start does not, in mode 15
                // only its last lines go down
                if !p.keep_lines && gap + closed_h > avail {
                    // splits as usual
                } else if !p.keep_lines && compat >= 15.0 && ls.len() > 1 {
                    let n = ls.len();
                    let m = if widow_on(&p) { if n >= 4 { 2 } else { n } } else { 1 };
                    if m >= n {
                        self.break_column(cx)?;
                        gap = 0.0;
                    } else {
                        tail_at = i64::try_from(n - m).unwrap_or(i64::MAX);
                    }
                } else {
                    self.break_column(cx)?;
                    gap = 0.0;
                }
            }
        }
        let y_pre = self.y;
        let gap_in = if !self.at_top || self.first_in_doc || keep_after.is_some() { gap } else { 0.0 };
        self.y += gap_in;
        // the page top under a header/footer float of modes ≤ 14: the paragraph goes below it from
        // ITS top (before the "before") and the "before" holds there (oracle hff14wrap)
        if self.at_top && gap_in > 0.0 && self.cur_ps()?.floats.iter().any(|f| f.body_only) {
            let fr = self.col_frame()?;
            let lh = super::floats::mark_line_h(cx, &p)?;
            let cur = self.cur_ix()?;
            let f0 = self.wrap_frame(&Self::para_wrap(&p, fr.w), fr.x, y_pre, gap_in + lh, cur)?;
            if let Some(s) = f0.skip_to.get()
                && s > y_pre
            {
                self.y = jm::max(self.y, s + gap_in);
            }
        }
        // the paragraph top for objects relative to it: the end of the previous one's "after"
        let para_ref = y_pre + jm::min(prev_after, gap_in);
        let self_gap = gap_in - jm::min(prev_after, gap_in);
        // a floating table's anchor: if the table pushes it down, the space before holds again
        // there (oracle ftable2)
        let ft_gap = if self.float_table_gap && !self.at_top {
            gap
        } else if self.carry_anchor_gap {
            before
        } else {
            0.0
        };
        self.float_table_gap = false;
        self.carry_anchor_gap = false;
        self.first_in_doc = false;

        let mut br = LineBreaker::new(&p, self.grid_for(&p));
        let grouped_next = bi + 1.0 <= g.blocks.1 && self.groups_with_next(cx, &p, next_block, &ParaEnv::default())?;
        let widow = widow_on(&p);
        let rf = self.flow_ref(block_idx);
        let pfmt = Arc::new(LineParaFormat { left: p.ind_left, right: p.ind_right, first_line: p.first_line, tabs: p.tabs.clone().into() });
        // the lines placed on the current page (moved for widows/orphans)
        let mut frag: Vec<FragLine> = Vec::new();
        let mut frag_top = self.y;
        // the current fragment holds the paragraph's first line
        let mut frag_has_first = true;
        let prev_top = Self::border_top_extent(&p, self.prev.as_ref());
        self.y += prev_top;
        let mut first = true;
        let mut line_no: i64 = 0;
        let tail_ps = self.cur_ix()?;
        // where the first line went: the objects anchored in the paragraph go to THAT page (not
        // the last line's — a paragraph with a page break at its end)
        let mut anchor_at = self.anchor_here(Some(para_ref))?;
        let mut early = self.place_floats_early(cx, &p, anchor_at.ps, anchor_at.x, anchor_at.w, self.y, None, Some(para_ref))?;
        // a wrapping object relative to the paragraph would pass the bottom margin: Word LIFTS it to
        // the margin if it then does not cross the paragraph's own lines; else the paragraph goes
        // down with it to the next page/column (oracle selffloat)
        let body_bottom0 = self.cur_ps()?.body_bottom;
        let over: Vec<f64> = early
            .iter()
            .filter(|e| {
                let a = crate::paint::model_run(cx.ldoc, e.run).and_then(super::floats::float_anchor);
                super::floats::follows_text(a) && e.obj.y + e.obj.h > body_bottom0 + 0.5
            })
            .map(|e| e.obj.h)
            .collect();
        if !self.at_top && !over.is_empty() {
            let para_bottom = self.y + self.paragraph_height(cx, &p, frame_base.w)?;
            let lift = over.iter().all(|h| body_bottom0 - h >= para_bottom - 0.5);
            for e in &early {
                self.unplace_float(e)?;
            }
            if lift {
                // (the other objects keep measuring from the top before the "before" — oracle liftref)
                let a = self.anchor_here(None)?;
                early = self.place_floats_early(cx, &p, a.ps, a.x, a.w, self.y, Some(body_bottom0), Some(para_ref))?;
            } else {
                self.break_column(cx)?;
                self.y += Self::border_top_extent(&p, self.prev.as_ref());
                anchor_at = self.anchor_here(None)?;
                early = self.place_floats_early(cx, &p, anchor_at.ps, anchor_at.x, anchor_at.w, self.y, None, None)?;
            }
        }
        let mut early_at = (self.cur_ix()?, self.col_frame()?.x);
        let early_runs: Vec<RunKey> = early.iter().map(|e| e.run).collect();
        // an EMPTY paragraph carrying the section break does not go to the next page: it hangs
        // below the margin (oracle sectfit)
        let n_sections = cx.ldoc.view.sections().len();
        let hangs = js_eq(bi, g.blocks.1)
            && (g.index as usize) < n_sections.saturating_sub(1)
            && p.glyphs.iter().all(|gl| gl.kind == GlyphKind::Obj && js_eq(gl.adv, 0.0));
        let mark_h = super::floats::mark_line_h(cx, &p)?;
        let mut last_line: Option<Line>;
        loop {
            let y0 = self.y;
            let mut frame = self.frame_here(&p, mark_h)?;
            // pushed below a float: the "before" holds again there — a floating table's anchor's
            // whole; else the part of the gap INSIDE the paragraph (oracles ftable2, ftaftdist)
            let push_gap = if ft_gap > 0.0 { ft_gap } else { self_gap };
            if first && push_gap > 0.0 && self.y > y0 {
                self.y += push_gap;
                frame = self.frame_here(&p, mark_h)?;
            }
            let mark = br.mark();
            let (mut seg_k, sf) = Self::pick_seg(&br, &p, &frame);
            let mut line = br.next(cx, &p, sf)?;
            let has_floats = !self.cur_ps()?.floats.is_empty();
            // the line's text taller than the estimate (the mark's font) crosses a float the
            // estimated height did not: it goes below it (oracle ftable2); narrower: the line narrows
            // (oracle tallwrap)
            let text_h = jm::min(line.height, line.baseline + line.desc);
            if has_floats && text_h > mark_h + 1.0 {
                let fr = self.col_frame()?;
                let cur = self.cur_ix()?;
                let f2 = self.wrap_frame(&Self::para_wrap(&p, fr.w), fr.x, self.y, text_h, cur)?;
                if let Some(s) = f2.skip_to.get()
                    && s > self.y
                {
                    self.y = s + if first { self_gap } else { 0.0 };
                    frame = self.frame_here(&p, mark_h)?;
                    br.reset(mark);
                    let (k, sf) = Self::pick_seg(&br, &p, &frame);
                    seg_k = k;
                    line = br.next(cx, &p, sf)?;
                } else if super::floats::narrower(&f2, &frame) {
                    frame = f2;
                    br.reset(mark);
                    let (k, sf) = Self::pick_seg(&br, &p, &frame);
                    seg_k = k;
                    line = br.next(cx, &p, sf)?;
                }
            }
            // text LOWER than the mark: the jump below the float was by the mark's height; if the
            // real line fits above it, it stays there (oracle liftref, lr-mark11)
            if has_floats && self.y > y0 + 0.5 {
                let text_h2 = jm::min(line.height, line.baseline + line.desc);
                if text_h2 < mark_h - 1.0 {
                    let fr = self.col_frame()?;
                    let cur = self.cur_ix()?;
                    let f0 = self.wrap_frame(&Self::para_wrap(&p, fr.w), fr.x, y0, text_h2, cur)?;
                    if !f0.skip_to.is_some() && !super::floats::narrower(&f0, &frame) {
                        let yb = self.y;
                        self.y = y0;
                        br.reset(mark);
                        let (k0, sf0) = Self::pick_seg(&br, &p, &f0);
                        let line0 = br.next(cx, &p, sf0)?;
                        if jm::min(line0.height, line0.baseline + line0.desc) <= text_h2 + 0.5 {
                            frame = f0;
                            seg_k = k0;
                            line = line0;
                        } else {
                            self.y = yb;
                            br.reset(mark);
                            let (k, sf) = Self::pick_seg(&br, &p, &frame);
                            seg_k = k;
                            line = br.next(cx, &p, sf)?;
                        }
                    }
                }
            }
            // a word that does not fit the gap beside a float: Word does not split it by character —
            // the line goes below it (oracle gapind, gi-*-r0)
            if has_floats && !line.last && line.end > line.start && (line.end as usize) < p.glyphs.len() {
                let fr = self.col_frame()?;
                let narrowed = frame.left > p.ind_left + 0.5 || frame.right < fr.w - p.ind_right - 0.5;
                let word = |g: &crate::para::Glyph| {
                    g.kind == GlyphKind::Char
                        && !g.ch.units.get(..usize::from(g.ch.len)).unwrap_or(&[]).iter().any(|&u| uo_text::space::is_space(u))
                };
                let g0 = p.glyphs.get(line.end as usize - 1);
                let g1 = p.glyphs.get(line.end as usize);
                let last_auto = line.placed.last().is_some_and(|t| matches!(t.g, crate::lines::GlyphSrc::AutoHyphen(_)));
                if narrowed
                    && g0.is_some_and(|g| word(g) && !g.flags.has(crate::para::GlyphFlags::BREAK_AFTER))
                    && g1.is_some_and(word)
                    && !last_auto
                {
                    let below = self.below_wrapping(jm::max(mark_h, line.height))?;
                    if below < f64::INFINITY && below > self.y {
                        self.y = below;
                        frame = self.frame_here(&p, mark_h)?;
                        br.reset(mark);
                        let (k, sf) = Self::pick_seg(&br, &p, &frame);
                        seg_k = k;
                        line = br.next(cx, &p, sf)?;
                    }
                }
            }
            // a tab opening the line in the gap with its stop past it: the line goes below the float
            // (oracle tabgap)
            if line.gap_tab == Some(crate::lines::GapTab::Start) && has_floats {
                let below = self.below_wrapping(jm::max(mark_h, line.height))?;
                if below < f64::INFINITY && below > self.y {
                    self.y = below;
                    frame = self.frame_here(&p, mark_h)?;
                    br.reset(mark);
                    let (k, sf) = Self::pick_seg(&br, &p, &frame);
                    seg_k = k;
                    line = br.next(cx, &p, sf)?;
                }
            }
            // modes < 15: the paragraph's last line stays above/beside a float only if the WHOLE
            // line plus the "after" stay off it; else it goes down (oracle ftafter)
            if has_floats && line.last && compat < 15.0 {
                let aft = after_of(&p);
                if aft > 0.0 {
                    let fr = self.col_frame()?;
                    let cur = self.cur_ix()?;
                    let f3 = self.wrap_frame(&Self::para_wrap(&p, fr.w), fr.x, self.y, line.height + aft, cur)?;
                    if let Some(s) = f3.skip_to.get()
                        && s > self.y
                    {
                        self.y = s + if first { self_gap } else { 0.0 };
                        frame = self.frame_here(&p, mark_h)?;
                        br.reset(mark);
                        let (k, sf) = Self::pick_seg(&br, &p, &frame);
                        seg_k = k;
                        line = br.next(cx, &p, sf)?;
                    }
                }
            }
            let mut page_changed = false;
            // the NEW notes of the line and the room they need (F7c)
            let mut note_need = self.note_need(cx, &p, &line)?;
            // a line with only the page break and more content after it: no height in the test —
            // the break stays on this page (oracle pbfit)
            let break_only = line.break_after == Some(BreakKind::Page)
                && !line.last
                && line.placed.iter().all(|pg| glyph_of(&p, pg.g).is_some_and(|gl| gl.kind == GlyphKind::Pb));
            let fit_h = if break_only {
                0.0
            } else if matches!(p.line_rule, LineRule::Exact | LineRule::AtLeast) {
                line.height
            } else {
                jm::min(line.height, line.baseline + line.desc)
            };
            let body_bottom = self.cur_ps()?.body_bottom;
            // a new note that does not fit whole: its beginning stays with the line (oracle fncont)
            let nfit = super::notes::note_line_fit(&p, &line);
            let mut note_cut: Option<super::notes::NoteCut> = None;
            if note_need.1 > 0.0
                && !self.at_top
                && !hangs
                && self.y + nfit > body_bottom - note_need.1 + 0.05
                && self.y + nfit <= body_bottom + 0.05
            {
                note_cut = self.note_partial(cx, &note_need.0, self.y + nfit)?;
            }
            if line_no == tail_at && self.cur == Some(tail_ps) && !frag.is_empty() && !self.at_top {
                // keepNext: the last lines go down with the next one's start (oracle keepsplit)
                self.flush_frag(cx, &p, &rf, &pfmt, &mut frag, frag_top, false, grouped_next)?;
                frag_has_first = false;
                self.break_column(cx)?;
                frag_top = self.y;
                page_changed = true;
            // (no tolerance: 0.21 tw past the margin already goes down — oracle bodyfit)
            } else if note_cut.is_none()
                && self.y + if note_need.1 > 0.0 { nfit } else { fit_h } > body_bottom - note_need.1 + 0.05
                && !self.at_top
                && !hangs
            {
                // it does not fit: widows/orphans
                let placed_here = frag.len();
                let mut moved: Vec<Line> = Vec::new();
                if widow && placed_here == 1 && frag_has_first && !first {
                    // orphan: the first line does not stay alone at the page end
                    if let Some(f) = frag.pop() {
                        moved.push(f.line);
                    }
                } else if widow && line.last && placed_here >= 2 {
                    // widow: the last line does not go alone to the next page
                    if let Some(f) = frag.pop() {
                        moved.push(f.line);
                    }
                    // …and if that leaves the first line alone (a 3-line paragraph), all go down
                    if frag.len() == 1
                        && frag_has_first
                        && let Some(f) = frag.pop()
                    {
                        moved.insert(0, f.line);
                    }
                }
                // all went down: the first line stays in the fragment (now on the new page)
                let all_moved = frag_has_first && frag.is_empty();
                self.flush_frag(cx, &p, &rf, &pfmt, &mut frag, frag_top, false, grouped_next)?;
                frag_has_first = all_moved;
                self.break_column(cx)?;
                frag_top = self.y;
                if all_moved {
                    anchor_at = self.anchor_here(None)?;
                }
                for m in moved {
                    let fx = self.col_frame()?.x;
                    let h = m.height;
                    frag.push(FragLine { line: m, y: self.y, frame_x: fx });
                    self.y += h;
                }
                page_changed = true;
            }
            if first {
                // the first line went to another page/column: the floats go along
                let here = (self.cur_ix()?, self.col_frame()?.x);
                if !early.is_empty() && (early_at.0 != here.0 || !js_eq(early_at.1, here.1)) {
                    for e in &early {
                        self.unplace_float(e)?;
                    }
                    let a = self.anchor_here(None)?;
                    early = self.place_floats_early(cx, &p, a.ps, a.x, a.w, self.y, None, None)?;
                    early_at = here;
                }
                // the first line pushed below a float (table or figure) on the SAME page: the
                // paragraph's objects keep measuring from where it began (oracle ftanchor); pushed past
                // the page foot, from there (corpus t20)
                let cur = self.cur_ix()?;
                if anchor_at.ps != cur || self.y <= anchor_at.top + 0.5 || self.y > self.cur_ps()?.body_bottom {
                    anchor_at = self.anchor_here(None)?;
                }
            }
            if page_changed {
                // the line was broken in the previous page's frame (wrap around objects): redone in
                // the new one's
                let y_top = self.y;
                let mut nf = self.frame_here(&p, mark_h)?;
                // the first line pushed below a float at the new page's top: the "before" holds
                // there, as on the same page (oracle hff14wrap)
                if first && frag_has_first && frag.is_empty() && push_gap > 0.0 && self.y > y_top + 0.01 {
                    self.y += push_gap;
                    nf = self.frame_here(&p, mark_h)?;
                }
                if !js_eq(nf.left, frame.left) || !js_eq(nf.right, frame.right) || nf.segs.is_some() || frame.segs.is_some() {
                    br.reset(mark);
                    frame = nf;
                    let (k, sf) = Self::pick_seg(&br, &p, &frame);
                    seg_k = k;
                    line = br.next(cx, &p, sf)?;
                    note_need = self.note_need(cx, &p, &line)?;
                }
            }
            if !note_need.0.is_empty() {
                self.reserve_notes(cx, &note_need.0, if page_changed { None } else { note_cut.as_ref() })?;
            }
            let fx = self.col_frame()?.x;
            let mut line_h = line.height;
            frag.push(FragLine { line: line.clone(), y: self.y, frame_x: fx });
            // the other gaps of the same line (an object in the middle: text on both sides)
            let y_line = self.y;
            let last = {
                let frag_ref = &mut frag;
                let line_h_ref = &mut line_h;
                let eng: &mut Engine = self;
                let pp = &p;
                let mut put = |cx: &mut Cx<'_, '_>, l: &Line| -> LResult<()> {
                    let nn = eng.note_need(cx, pp, l)?;
                    if !nn.0.is_empty() {
                        eng.reserve_notes(cx, &nn.0, None)?;
                    }
                    let fx = eng.col_frame()?.x;
                    frag_ref.push(FragLine { line: l.clone(), y: y_line, frame_x: fx });
                    *line_h_ref = jm::max(*line_h_ref, l.height);
                    Ok(())
                };
                Self::more_segs(cx, &mut br, pp, &frame, seg_k, line, &mut put)?
            };
            self.y += line_h;
            self.at_top = false;
            first = false;
            line_no += 1;
            // a line break w:clear: the next line starts below the wrapping floats that reach this
            // height (oracle brclear)
            if last.clear {
                for _ in 0..8 {
                    let yy = self.y;
                    let below: Vec<f64> = self
                        .cur_ps()?
                        .floats
                        .iter()
                        .filter(|f| &*f.wrap != "none" && f.y - f.dist_t < yy && f.y + f.h + f.dist_b > yy)
                        .map(|f| f.y + f.h + f.dist_b)
                        .collect();
                    if below.is_empty() {
                        break;
                    }
                    self.y = jm::max_n(&below);
                }
            }
            let brk = last.break_after;
            last_line = Some(last);
            match brk {
                Some(BreakKind::Page) => {
                    self.flush_frag(cx, &p, &rf, &pfmt, &mut frag, frag_top, false, grouped_next)?;
                    frag_has_first = false;
                    let cg = self.cur_ps()?.geom;
                    self.new_page(cx, cg, "nextPage")?;
                    frag_top = self.y;
                    self.at_top = true;
                }
                Some(BreakKind::Column) => {
                    self.flush_frag(cx, &p, &rf, &pfmt, &mut frag, frag_top, false, grouped_next)?;
                    frag_has_first = false;
                    self.break_column(cx)?;
                    frag_top = self.y;
                }
                None => {}
            }
            if br.done(&p) {
                break;
            }
        }
        let _ = (frag_has_first, last_line);
        self.flush_frag(cx, &p, &rf, &pfmt, &mut frag, frag_top, true, grouped_next)?;
        self.y += Self::border_bottom_extent(&p, grouped_next);
        // the column ends at the paragraph end: the "after" counts for the line between columns
        if !self.at_top {
            let a = after_of(&p);
            self.note_col_content(self.y + a)?;
        }
        // the floating objects anchored in this paragraph
        for r in &early_runs {
            self.pre_placed.insert(*r);
        }
        self.place_floats(cx, &p, anchor_at.ps, anchor_at.x, anchor_at.w, anchor_at.top, None, 0.0, anchor_at.para_ref)?;
        for r in &early_runs {
            self.pre_placed.shift_remove(r);
        }
        // a paragraph with only a page break: the next one starts at the top (with
        // splitPgBreakAndParaMark the mark already took the page's first line — oracle pbmark)
        let split_pg = compat < 15.0 && settings.compat.get("splitPgBreakAndParaMark") == Some(&true);
        if p.glyphs.last().is_some_and(|gl| gl.kind == GlyphKind::Pb) && !split_pg {
            self.at_top = true;
        }
        let has_bdr = p.ppr.p_bdr.is_some();
        let lb = self.last_box.filter(|_| has_bdr);
        self.prev = Some(PrevPara {
            after: after_of(&p),
            after_auto: p.after_auto,
            list,
            style_id: p.style_id.clone().filter(|s| !s.is_empty()),
            contextual: p.contextual,
            p_bdr: has_bdr,
            p_bdr_key: if has_bdr { bdr_key(&p.ppr) } else { None },
            box_bottom_item_idx: None,
            box_bottom: lb.map_or(OptF64::NONE, |b| OptF64::some(b.bottom)),
            box_page: lb.and_then(|b| b.page),
        });
        Ok(())
    }

    /// `{ left: p.indLeft, right: fr.w − p.indRight }` as a wrap frame.
    fn para_wrap(p: &PreparedPara, w: f64) -> super::page::WrapFrame {
        super::page::WrapFrame { left: p.ind_left, right: w - p.ind_right, skip_to: OptF64::NONE, segs: None, gap: false }
    }

    /// `frameHere()` of `placeParagraph`: the line's frame at `this.y` around the current page's
    /// floats; an object with no room beside it (or "top and bottom"): below it.
    fn frame_here(&mut self, p: &PreparedPara, line_h: f64) -> LResult<super::page::WrapFrame> {
        let fr = self.col_frame()?;
        let cur = self.cur_ix()?;
        let base = Self::para_wrap(p, fr.w);
        let mut frame = self.wrap_frame(&base, fr.x, self.y, line_h, cur)?;
        let mut k = 0;
        while k < 8
            && let Some(s) = frame.skip_to.get()
            && s > self.y
        {
            self.y = s;
            frame = self.wrap_frame(&base, fr.x, self.y, line_h, cur)?;
            k += 1;
        }
        Ok(frame)
    }

    /// The bottom of the nearest square/tight/through float crossing `[y, y + lineH]` (the
    /// `below` loops of `placeParagraph`).
    fn below_wrapping(&self, line_h: f64) -> LResult<f64> {
        let mut below = f64::INFINITY;
        for f in &self.cur_ps()?.floats {
            if !matches!(&*f.wrap, "square" | "tight" | "through") {
                continue;
            }
            if self.y + line_h <= f.y - f.dist_t + 0.5 || self.y >= f.y + f.h + f.dist_b - 0.5 {
                continue;
            }
            below = jm::min(below, f.y + f.h + f.dist_b);
        }
        Ok(below)
    }

    /// `{ ps: this.cur, top: this.y, x: colFrame().x, w: colFrame().w, ref }`.
    fn anchor_here(&self, para_ref: Option<f64>) -> LResult<AnchorAt> {
        let fr = self.col_frame()?;
        Ok(AnchorAt { ps: self.cur_ix()?, top: self.y, x: fr.x, w: fr.w, para_ref })
    }

    /// `flushFrag(isLast)` of `placeParagraph`: paints the fragment's lines on the current page,
    /// records them, and paints the paragraph box around them.
    #[expect(clippy::too_many_arguments, reason = "the TS closure's captures")]
    fn flush_frag(
        &mut self,
        cx: &mut Cx<'_, '_>,
        p: &PreparedPara,
        rf: &LineRef,
        pfmt: &Arc<LineParaFormat>,
        frag: &mut Vec<FragLine>,
        frag_top: f64,
        is_last: bool,
        grouped_next: bool,
    ) -> LResult<()> {
        let Some(f0) = frag.first() else { return Ok(()) };
        let frame_x = f0.frame_x;
        let box_top = frag_top;
        let cur = self.cur_ix()?;
        let box_at = self.ps(cur)?.body.len();
        let ln_on = self.geom(self.ps(cur)?.geom)?.ln_num.is_some() && p.ppr.suppress_line_numbers != Some(true);
        let col_w = self.col_frame()?.w;
        for f in frag.iter() {
            if ln_on {
                self.paint_line_number(cx, f.frame_x, f.y + f.line.baseline)?;
            }
            let mut body = std::mem::take(&mut self.ps_mut(cur)?.body);
            let r = crate::paint::paint_line(
                cx,
                p,
                &f.line,
                crate::paint::LinePlacement { frame_x: f.frame_x, top: f.y, hf: false },
                &mut body,
            );
            self.ps_mut(cur)?.body = body;
            r?;
            // inline groups and textboxes (F7b): to the front layer, after the page's text
            let mut none = PassItems::new();
            self.paint_inline_drawings(cx, p, &f.line, f.frame_x, f.y, Some(cur), true, &mut none)?;
            let l = line_record(rf, p, &f.line, f.frame_x, f.y, Some(col_w), pfmt);
            self.ps_mut(cur)?.page.lines.push(l);
        }
        let last = frag.last().ok_or(LayoutError::Internal("flushFrag"))?;
        let mut bottom = last.y + last.line.height;
        self.note_col_content(bottom)?;
        if is_last {
            bottom += Self::border_bottom_extent(p, grouped_next);
        }
        let prev = self.prev.clone();
        let mut body = std::mem::take(&mut self.ps_mut(cur)?.body);
        let r = self.paint_box(
            cx,
            &mut body,
            p,
            frame_x,
            col_w,
            box_top,
            bottom,
            prev.as_ref(),
            box_at,
            is_last && grouped_next,
            Some(ContainerId::PageBody(cur)),
        );
        self.ps_mut(cur)?.body = body;
        r?;
        frag.clear();
        Ok(())
    }
}

/// `headingStyleLevel(p)` (engine.ts:551): N of the paragraph's own "heading N" style, else 0
/// (over the paragraph object's `props` and `ppr`).
pub(super) fn heading_style_level_of(cx: &Cx<'_, '_>, props: &ParaProps, ppr: Option<&PPr>) -> f64 {
    let id = props.style_id.as_deref().or_else(|| ppr.and_then(|q| q.p_style.as_deref()));
    let id = id.filter(|s| !s.is_empty());
    let name = id.and_then(|id| cx.ldoc.view.formatting().styles.styles.get(id)).and_then(|s| s.name.as_deref());
    name.and_then(heading_level).unwrap_or(0.0)
}

/// `lineRecord(ref, p, line, frameX, top, frameW)` (engine.ts:3938).
#[must_use]
pub(crate) fn line_record(
    rf: &LineRef,
    p: &PreparedPara,
    line: &Line,
    frame_x: f64,
    top: f64,
    frame_w: Option<f64>,
    pfmt: &Arc<LineParaFormat>,
) -> PassLine {
    let gs = &p.glyphs;
    let first = gs.get(line.start as usize);
    let last_ix = line.end.checked_sub(1).map_or(line.start, |e| e.max(line.start));
    let last_g = gs.get(last_ix as usize);
    let start = first.map_or(0, |g| g.offset);
    let end = last_g.map_or(start, |g| g.offset.wrapping_add(if g.ch.len == 0 { 1 } else { u32::from(g.ch.len) }));
    // the position of each character (offset in the paragraph → absolute x and width)
    let chars: Box<[(u32, f64, f64)]> = line
        .placed
        .iter()
        .filter_map(|pg| {
            let g = glyph_of(p, pg.g)?;
            if g.kind == GlyphKind::Pb || g.kind == GlyphKind::Cb || g.flags.has(crate::para::GlyphFlags::AUTO_HYPHEN) {
                return None;
            }
            Some((g.offset, frame_x + pg.x, pg.w))
        })
        .collect();
    // change bar: the line crosses a revision (hidden deleted text included)
    let first_line = line.start == 0;
    let last_line = line.end as usize >= gs.len();
    let (fs, fe) = (f64::from(start), f64::from(end));
    let rev = p.rev_bars.as_ref().is_some_and(|rb| {
        rb.whole
            || (rb.mark && last_line)
            || rb.ranges.iter().any(|&(a, b, hidden)| if hidden { a <= fe && b >= fs } else { a < fe && b > fs })
    });
    let x = frame_x + line.x_start;
    // a line ending in an AUTOMATIC hyphen: where the word broke (Manual Hyphenation)
    let hyph = line.placed.last().filter(|t| matches!(t.g, GlyphSrc::AutoHyphen(_))).and_then(|t| glyph_of(p, t.g)).map(|g| g.offset);
    PassLine {
        at: rf.clone(),
        start,
        end,
        x,
        y: top,
        height: line.height,
        baseline: top + line.baseline,
        width: line.width,
        chars: Some(chars),
        hyph,
        rev: rev.then_some((if first_line { p.space_before } else { 0.0 }, if last_line { p.space_after } else { 0.0 })),
        frame: frame_w.map(|w| (frame_x - x, frame_x + w - x)),
        pfmt: frame_w.map(|_| Arc::clone(pfmt)),
        meta: None,
        obj: None,
    }
}

/// `revisionBarItems(lines, x, color)` (engine.ts:3974): a 0.72 pt bar in the left margin per run
/// of consecutive lines with a revision, over each line's box plus the space before/after —
/// without passing the neighbouring line.
pub(crate) fn revision_bar_items(lines: &[PassLine], x: f64, color: uo_render::Sym, out: &mut PassItems) {
    let mut all: Vec<&PassLine> = lines.iter().collect();
    all.sort_by(|a, b| {
        let d = a.y - b.y;
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut segs: Vec<(f64, f64)> = Vec::new();
    for (i, l) in all.iter().enumerate() {
        let Some((rb, ra)) = l.rev else { continue };
        let prev_bottom = i.checked_sub(1).and_then(|j| all.get(j)).map_or(f64::NEG_INFINITY, |q| q.y + q.height);
        let next_top = all.get(i + 1).map_or(f64::INFINITY, |q| q.y);
        let top = jm::max(l.y - rb, jm::min(l.y, prev_bottom));
        let bottom = jm::min(l.y + l.height + ra, jm::max(l.y + l.height, next_top));
        match segs.last_mut() {
            // (the border between table rows does not split the bar — oracle tbl)
            Some(last) if top <= last.1 + 20.0 => last.1 = jm::max(last.1, bottom),
            _ => segs.push((top, bottom)),
        }
    }
    for (y0, y1) in segs {
        out.list.push_plain(Item::Rect { x, y: y0, w: word::BAR_WIDTH, h: y1 - y0, fill: color, alpha: OptF64::NONE });
        out.side.push(ItemSide::default());
    }
}

/// `PrepFlow` over the pass (F4-1): the context, the field values of the page flow, and the
/// default numbering/note marks over the context.
impl PrepFlow for Engine {
    fn ctx(&mut self) -> &mut crate::context::DocContext {
        &mut self.ctx
    }

    fn field(&mut self, cx: &mut Cx<'_, '_>, src: FieldSource, name: &[u16], instr: &[u16], at: f64) -> LResult<Option<Box<[u16]>>> {
        match src {
            FieldSource::None => Ok(None),
            FieldSource::Page => {
                let (ps, block) = self.field_at.ok_or(LayoutError::Internal("field: no page"))?;
                self.field_value(cx, ps, Some(block), name, Some(instr), Some(at))
            }
            // `fieldValue(ps)` without a block: a header/footer's page (F7c)
            FieldSource::HeaderFooter => {
                let ps = self.field_page.ok_or(LayoutError::Internal("field: no header/footer page"))?;
                self.field_value(cx, ps, None, name, Some(instr), Some(at))
            }
            // a footnote's content: its own mark, else `fieldValue(this.cur)(name)` (F7c)
            FieldSource::Footnote(id) => {
                if name == "FOOTNOTEREF_MARK".encode_utf16().collect::<Vec<_>>().as_slice() {
                    return Ok(Some(self.footnote_mark(id)));
                }
                let ps = self.cur_ix()?;
                self.field_value(cx, ps, None, name, None, None)
            }
            FieldSource::Endnote(_) => {
                let ps = self.cur_ix()?;
                self.field_value(cx, ps, None, name, None, None)
            }
        }
    }
}
