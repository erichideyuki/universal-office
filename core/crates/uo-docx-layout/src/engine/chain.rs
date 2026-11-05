//! Linked textbox chains (frontier F7d): `chainOf`/`chainSlice` (engine.ts:3261–3344) and the
//! runtime parts of chain.ts — `sliceParagraphFrom` 57, `topBlockOf` 72, `topRowOf` 78,
//! `remapLines` 86. `indexChains` runs once per call in `derive` (`LDoc::chains`, F7b).
//!
//! The content of a chain's first box is laid out at the width of each box in `seq` order; the
//! cut (the one of a split table row: widows/orphans, the space after) sends the rest to the next
//! box, which breaks it again at its own width. A paragraph cut between two boxes continues as a
//! **runtime copy** (`RtPara` with `continuation` and the cut first run, `first_from`, F7d-2)
//! handed to the static flow as `SBlock::Rt`; a table cut between rows continues as an owned
//! row-sliced copy of the model table (`{ ...pb, rows: pb.rows.slice(row) }`, F7d-3). Both live in
//! this function's frame only: the static flow runs on a `Cx` reborrowed with the frame's
//! lifetime (as F7b-5). The slices are laid out once per pass and chain (`Engine::chain_slices`,
//! filled as the boxes are laid out, so a box of the same chain met again while its chain is being
//! laid out gets what is there — the TS `Map` is set before it is filled).

use indexmap::IndexMap;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{PassItems, PassLine};
use crate::keys::{ChainId, LParaId, PageIx, RunKey, TbKey};
use crate::ldoc::LDoc;
use crate::model::{DrawingTextbox, Run, Table};
use crate::output::{LineRef, LineStory, RefPath};
use crate::para::{FieldSource, ParaEnv};
use crate::table::CellContent;
use uo_jsnum::math as jm;
use uo_render::OptF64;

use super::flow::RtPara;
use super::static_layout::SBlock;
use super::{Cx, Engine};

/// The env the first caller of a pass hands to `chainSlice` (`env0`): `{ field:
/// this.fieldValue(ps), hf? }` of a float or a group child, `{}` of an inline box.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChainEnv {
    /// the page of `fieldValue(ps)` (`FieldSource::HeaderFooter` over `field_page`, F7b-4)
    pub field_ps: Option<PageIx>,
    pub hf: bool,
}

/// A block of what is left of a chain's content (`rem`).
#[derive(Clone, Debug)]
enum RBlock<'a> {
    Para(LParaId),
    Table(&'a Table),
    /// `sliceParagraphFrom(p, off)`
    Slice(RtPara),
    /// `{ ...t, rows: t.rows.slice(from) }` of a model table (slices of slices add up)
    Rows(&'a Table, usize),
}

/// `{ items: [], lines: [], height: 0 }`.
fn empty() -> CellContent {
    CellContent {
        items: PassItems::new(),
        lines: Vec::new(),
        height: 0.0,
        keep_next: false,
        paras: None,
        nest: None,
        align_h: OptF64::NONE,
        shape: false,
    }
}

/// `topBlockOf(ref)` (chain.ts:72): `/^[^/]+\/b(\d+)/` — the top block, or −1.
fn top_block_of(at: &LineRef) -> Option<u32> {
    at.path.0.first().copied()
}

/// `topRowOf(ref)` (chain.ts:78): `/^[^/]+\/b\d+\/r(\d+)\//` — the row of the top table, or −1
/// (the row segment must be followed by another one: the cell).
fn top_row_of(at: &LineRef) -> Option<u32> {
    if at.path.0.len() >= 3 { at.path.0.get(1).copied() } else { None }
}

/// `topRowOf` as the TS number (−1 when absent).
fn row_num(at: &LineRef) -> f64 {
    top_row_of(at).map_or(-1.0, f64::from)
}

/// `topBlockOf` as the TS number (−1 when absent).
fn block_num(at: &LineRef) -> f64 {
    top_block_of(at).map_or(-1.0, f64::from)
}

/// `r.field || r.fieldResult || r.image || r.textbox || r.shape || r.group || r.custom || r.math`:
/// a run `sliceParagraphFrom` never cuts.
fn whole_run(r: &Run) -> bool {
    let Some(e) = r.ext() else { return false };
    e.field.as_deref().is_some_and(|f| !f.is_empty())
        || e.field_result.is_some()
        || e.image.is_some()
        || e.textbox.is_some()
        || e.shape.is_some()
        || e.group.is_some()
        || e.custom.is_some()
        || e.math.is_some()
}

/// `sliceParagraphFrom(p, offset)` (chain.ts:57): the paragraph from `offset` (in the concatenated
/// text of its runs) — the continuation in the next box. A run cut in the middle becomes its tail
/// (`first_from`); a field or an object is never cut (it stays whole). `p` is a derived paragraph
/// (`base`, its runs, no cut) or an earlier slice (its runs with their cut).
fn slice_paragraph_from(
    ldoc: &LDoc<'_>,
    base: LParaId,
    runs: &[RunKey],
    first_from: u32,
    rev_joins: &[u32],
    offset: u32,
) -> LResult<RtPara> {
    let mut out: Vec<RunKey> = Vec::new();
    let mut cut = 0u32;
    let mut pos: u64 = 0;
    let off = u64::from(offset);
    for (i, &k) in runs.iter().enumerate() {
        let full = u32::try_from(ldoc.run_text(k).len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        let own_from = if i == 0 { first_from } else { 0 };
        let len = u64::from(full.saturating_sub(own_from));
        if pos + len <= off {
            pos += len;
            continue;
        }
        let whole = pos >= off || crate::paint::model_run(ldoc, k).is_some_and(whole_run);
        let from = if whole {
            own_from
        } else {
            // `{ ...r, text: r.text.slice(offset - pos) }` (of the run's own text: a cut run's
            // tail is cut again from its base)
            let d = u32::try_from(off - pos).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            own_from.checked_add(d).ok_or(LayoutError::Limit(LimitKind::Capacity))?
        };
        if out.is_empty() {
            cut = from;
        } else if from != 0 {
            // (a cut only ever falls on the first run kept: every run before it was skipped)
            return Err(LayoutError::Internal("chain: a cut after the first run"));
        }
        out.push(k);
        pos += len;
    }
    // `{ ...p, runs }`: the copy keeps `p`'s fields (and its `REV_JOINS`); `CONTINUATION.add(out)`
    Ok(RtPara { base, ppr: None, props: None, runs: out, rev_joins: rev_joins.to_vec(), first_from: cut, continuation: true })
}

/// `remapLines(lines, map, cont, rows)` (chain.ts:86): the refs and offsets of the rest's lines
/// back to the ORIGINAL blocks — block `j` of the rest is `map[j]` of the first box's content; the
/// first, when it continues a paragraph, adds `cont` to the offsets; when it is the rest of a
/// table cut between the boxes, adds `rows` to its rows. A changed line is a copy (`{ ...l }`: no
/// object identity).
fn remap_lines(lines: Vec<PassLine>, map: &[u32], cont: u32, rows: u32) -> Vec<PassLine> {
    lines
        .into_iter()
        .map(|mut l| {
            let Some(&j) = l.at.path.0.first() else { return l };
            let tail = l.at.path.0.len() > 1;
            let mut path = l.at.path.0.to_vec();
            if let Some(b) = path.first_mut() {
                *b = map.get(j as usize).copied().unwrap_or(j);
            }
            if j == 0
                && rows != 0
                && let Some(r) = path.get_mut(1)
            {
                *r = r.saturating_add(rows);
            }
            let changed = path.as_slice() != &*l.at.path.0;
            // offsets only in the continuation (the paragraph itself, not table cells)
            if j != 0 || cont == 0 || tail {
                if changed {
                    l.at = LineRef { story: l.at.story, path: RefPath(path.into()) };
                    l.obj = None;
                }
                return l;
            }
            l.at = LineRef { story: l.at.story, path: RefPath(path.into()) };
            l.start = l.start.saturating_add(cont);
            l.end = l.end.saturating_add(cont);
            if let Some(cs) = &mut l.chars {
                for c in cs.iter_mut() {
                    c.0 = c.0.saturating_add(cont);
                }
            }
            l.obj = None;
            l
        })
        .collect()
}

/// `{ ...t, rows: t.rows.slice(from) }`: an owned copy of a model table without its first rows
/// (the same object identity as the TS copy needs: the table id and the cells' paragraphs keep
/// their ids, so `cellNb`, re-assigned before each layout, and the derived paragraphs are found).
fn rows_copy(t: &Table, from: usize) -> Table {
    Table {
        r#type: t.r#type,
        rows: t.rows.get(from..).unwrap_or(&[]).to_vec(),
        grid_cols_twips: t.grid_cols_twips.clone(),
        borders: t.borders.clone(),
        tbl_pr: t.tbl_pr.clone(),
        src: t.src,
        cleared: t.cleared,
        keys: t.keys.clone(),
        id: t.id,
    }
}

impl Engine {
    /// `chainOf(tb)` (engine.ts:3265): the chain of a textbox (only a box with a content story or
    /// a link can be in one).
    pub(crate) fn chain_of(cx: &Cx<'_, '_>, tb: &DrawingTextbox) -> Option<(ChainId, TbKey)> {
        if tb.story.is_none() && tb.link.is_none() {
            return None;
        }
        let k = *cx.ldoc.tb_of_drawing.get(&tb.drawing)?;
        Some((cx.ldoc.textboxes.get(k.ix())?.chain?, k))
    }

    /// `chainOf(tb) ? chainSlice(chain, tb, env0) : …`: the content of `tb` when it belongs to a
    /// linked chain, `None` otherwise.
    pub(crate) fn chain_content(&mut self, cx: &mut Cx<'_, '_>, tb: &DrawingTextbox, env0: ChainEnv) -> LResult<Option<CellContent>> {
        let Some((cid, k)) = Self::chain_of(cx, tb) else { return Ok(None) };
        self.chain_slice(cx, cid, k, env0).map(Some)
    }

    /// `chainSlice(chain, tb, env0)` (engine.ts:3278): the piece of the chain's content that fits in
    /// box `tb` (laid out once per pass, F7d-4).
    pub(crate) fn chain_slice(&mut self, cx: &mut Cx<'_, '_>, cid: ChainId, tb: TbKey, env0: ChainEnv) -> LResult<CellContent> {
        if !self.chain_slices.contains_key(&cid) {
            self.chain_slices.insert(cid, IndexMap::new());
            self.chain_layout(cx, cid, env0)?;
        }
        Ok(self.chain_slices.get(&cid).and_then(|m| m.get(&tb)).cloned().unwrap_or_else(empty))
    }

    /// The body of `chainSlice`: every box of the chain in order.
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    fn chain_layout<'a>(&mut self, cx: &mut Cx<'_, 'a>, cid: ChainId, env0: ChainEnv) -> LResult<()> {
        let (members, boxes): (Vec<TbKey>, Vec<&'a DrawingTextbox>) = {
            let ch = cx.ldoc.chains.get(cid.ix()).ok_or(LayoutError::Internal("chain"))?;
            (ch.members.clone(), ch.boxes.clone())
        };
        let head = *members.first().ok_or(LayoutError::Internal("chain: no member"))?;
        let head_story = cx.ldoc.textboxes.get(head.ix()).ok_or(LayoutError::Internal("chain: head"))?.story;
        // `head.blocks ?? head.paras.map(…)`: the head's derived content story
        let mut rem: Vec<RBlock<'a>> = self
            .story_blocks(cx, head_story)?
            .into_iter()
            .map(|b| match b {
                SBlock::Para(k) => Ok(RBlock::Para(k)),
                SBlock::Table(t) => Ok(RBlock::Table(t)),
                SBlock::Rt(_) => Err(LayoutError::Internal("chain: story block")),
            })
            .collect::<LResult<_>>()?;
        let mut map: Vec<u32> = (0..u32::try_from(rem.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?).collect();
        let (mut cont, mut rows) = (0u32, 0u32);
        // `'txbx'`: the lines name the head's content (LineStory::Textbox of the first box)
        let txbx = LineRef { story: LineStory::Textbox(head), path: RefPath::default() };
        let b0 = LineRef { story: LineStory::Textbox(head), path: RefPath(vec![0].into()) };
        let n_members = members.len();
        for (k, (&m, &mtb)) in members.iter().zip(boxes.iter()).enumerate() {
            cx.budget.charge(1)?;
            if rem.is_empty() {
                self.chain_put(cid, m, empty());
                continue;
            }
            let theme = &cx.ldoc.view.formatting().theme;
            let fr = super::floats::tb_frame(mtb, theme, mtb.width_twips.to_number(), mtb.height_twips.to_number());
            // `{ ...env0, ...this.tbEnv(m) }`
            let (dark, dc) = Self::tb_env(cx, mtb);
            let env = ParaEnv {
                field: env0.field_ps.map(|_| FieldSource::HeaderFooter),
                hf: env0.hf,
                dark_background: dark,
                default_color: dc,
                ..ParaEnv::default()
            };
            let (cur_map, cur_cont, cur_rows) = (map.clone(), cont, rows);
            let mut c = {
                // the copies of this box's blocks live in this frame (a `Cx` reborrowed with its
                // lifetime, F7d-3)
                let owned: Vec<Table> = rem
                    .iter()
                    .filter_map(|b| match b {
                        RBlock::Rows(t, from) => Some(rows_copy(t, *from)),
                        _ => None,
                    })
                    .collect();
                let mut ot = owned.iter();
                let mut blocks: Vec<SBlock<'_>> = Vec::with_capacity(rem.len());
                for b in &rem {
                    blocks.push(match b {
                        RBlock::Para(p) => SBlock::Para(*p),
                        RBlock::Table(t) => SBlock::Table(t),
                        RBlock::Slice(rt) => SBlock::Rt(rt),
                        RBlock::Rows(..) => SBlock::Table(ot.next().ok_or(LayoutError::Internal("chain: rows"))?),
                    });
                }
                let mut cx2 = super::flow::with_ldoc!(cx, cx.ldoc);
                let iw = jm::max(1.0, fr.iw);
                let field_ps = env0.field_ps;
                self.pure_static(&mut cx2, |e, cx| {
                    let saved = e.field_page;
                    if field_ps.is_some() {
                        e.field_page = field_ps;
                    }
                    let r = e.layout_static(cx, &blocks, 0.0, iw, &env, &txbx, None, 0.0, None);
                    e.field_page = saved;
                    r
                })?
            };
            // the continuation's first line is not the paragraph's first (widows/orphans)
            if cur_cont != 0
                && let Some(paras) = c.paras.as_mut()
                && let Some(ci) = paras.get(&b0).copied()
            {
                let mut ci = ci;
                ci.lines += 1.0;
                std::sync::Arc::make_mut(paras).insert(b0.clone(), ci);
            }
            let mut slice: Option<CellContent> = None;
            let blocks = std::mem::take(&mut rem);
            if k + 1 < n_members {
                let Some(mut parts) = crate::table::split_content(cx, &c, fr.ih)? else {
                    // not even the first line fits: the box stays empty and everything goes on
                    self.chain_put(cid, m, empty());
                    rem = blocks;
                    continue;
                };
                let mut cut = parts.0.height;
                let lowest = |c: &CellContent, cut: f64| -> Option<usize> {
                    // `moved.reduce((a, b) => (b.y < a.y ? b : a))`: the first of the lowest y
                    let mut best: Option<usize> = None;
                    // (`l.y >= cut - 0.01` keeps it; NaN never does — the filter)
                    for (i, l) in c.lines.iter().enumerate().filter(|(_, l)| l.y >= cut - 0.01) {
                        // the first line, or one strictly above the best so far
                        let replace = best.and_then(|b| c.lines.get(b)).is_none_or(|a| l.y < a.y);
                        if replace {
                            best = Some(i);
                        }
                    }
                    best
                };
                let mut first = lowest(&c, cut);
                // a cut in a table: its rows that did not fit whole go to the next box (the split
                // row goes down whole — otherwise its top part would show twice)
                let mut row = first.and_then(|i| c.lines.get(i)).map_or(-1.0, |l| row_num(&l.at));
                if let Some(fi) = first
                    && row >= 0.0
                {
                    let fl = c.lines.get(fi).ok_or(LayoutError::Internal("chain: line"))?;
                    let (jt, r0) = (block_num(&fl.at), row);
                    let nr = c
                        .nest
                        .as_ref()
                        .and_then(|ns| ns.iter().find(|n| n.lines.iter().any(|l| block_num(&l.at) == jt && row_num(&l.at) == r0)));
                    if let Some(nr) = nr
                        && nr.top < cut - 0.01
                    {
                        let top = nr.top;
                        let p2 = if top > 0.5 { crate::table::split_content(cx, &c, top)? } else { None };
                        let Some(p2) = p2 else {
                            self.chain_put(cid, m, empty());
                            rem = blocks;
                            continue;
                        };
                        parts = p2;
                        cut = parts.0.height;
                        // (`reduce` of an empty array throws in the TS)
                        first = Some(lowest(&c, cut).ok_or(LayoutError::Internal("chain: no moved line"))?);
                        row = first.and_then(|i| c.lines.get(i)).map_or(-1.0, |l| row_num(&l.at));
                    }
                }
                if let Some(fi) = first {
                    let fl = c.lines.get(fi).ok_or(LayoutError::Internal("chain: line"))?;
                    let mut s0 = parts.0;
                    s0.height = cut;
                    slice = Some(s0);
                    let j = jm::max(0.0, block_num(&fl.at));
                    // `/^[^/]+\/b\d+$/`: a line of the paragraph itself
                    let off = if fl.at.path.0.len() == 1 { fl.start } else { 0 };
                    let ju = super::index_of(j).ok_or(LayoutError::Internal("chain: block"))?;
                    map = cur_map.get(ju..).unwrap_or(&[]).to_vec();
                    let pb = blocks.get(ju);
                    cont = 0;
                    rows = 0;
                    let rest = || blocks.get(ju + 1..).unwrap_or(&[]).to_vec();
                    match pb {
                        Some(RBlock::Para(p)) if off > 0 => {
                            cont = if ju == 0 { cur_cont } else { 0 }.saturating_add(off);
                            let lp = cx.ldoc.para(*p).ok_or(LayoutError::Internal("chain: paragraph"))?;
                            let s = slice_paragraph_from(cx.ldoc, *p, &lp.runs, 0, &lp.rev_joins, off)?;
                            rem = std::iter::once(RBlock::Slice(s)).chain(rest()).collect();
                        }
                        Some(RBlock::Slice(rt)) if off > 0 => {
                            cont = if ju == 0 { cur_cont } else { 0 }.saturating_add(off);
                            let s = slice_paragraph_from(cx.ldoc, rt.base, &rt.runs, rt.first_from, &rt.rev_joins, off)?;
                            rem = std::iter::once(RBlock::Slice(s)).chain(rest()).collect();
                        }
                        Some(RBlock::Table(t)) if row > 0.0 => {
                            let r = super::index_of(row).ok_or(LayoutError::Internal("chain: row"))?;
                            rows = if ju == 0 { cur_rows } else { 0 }.saturating_add(u32::try_from(r).unwrap_or(u32::MAX));
                            rem = std::iter::once(RBlock::Rows(t, r)).chain(rest()).collect();
                        }
                        Some(RBlock::Rows(t, from)) if row > 0.0 => {
                            let r = super::index_of(row).ok_or(LayoutError::Internal("chain: row"))?;
                            rows = if ju == 0 { cur_rows } else { 0 }.saturating_add(u32::try_from(r).unwrap_or(u32::MAX));
                            rem = std::iter::once(RBlock::Rows(t, from.saturating_add(r))).chain(rest()).collect();
                        }
                        _ => rem = blocks.get(ju..).unwrap_or(&[]).to_vec(),
                    }
                }
            }
            let mut out = match slice {
                Some(s) => s,
                None => std::mem::take(&mut c),
            };
            out.lines = remap_lines(std::mem::take(&mut out.lines), &cur_map, cur_cont, cur_rows);
            self.chain_put(cid, m, out);
        }
        Ok(())
    }

    /// `out.set(m, content)`.
    fn chain_put(&mut self, cid: ChainId, m: TbKey, c: CellContent) {
        if let Some(out) = self.chain_slices.get_mut(&cid) {
            out.insert(m, c);
        }
    }
}
