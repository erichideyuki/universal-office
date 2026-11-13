//! Footnotes and endnotes (frontier F7c): the note area of a page (separator, notes in columns,
//! notes split between pages), the note references of the flow, and the endnote flow
//! (engine.ts:2214–2513, 912–944).

use std::sync::Arc;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{ItemSide, PassItems, PassLine};
use crate::keys::{LParaId, PageIx};
use crate::ldoc::{LBlock, LDoc, LPara, ParaFlags, ParaSrc};
use crate::lines::{Line, LineBreaker};
use crate::model::{NoteKind, NumKey, PPr, Paragraph};
use crate::output::{LineRef, LineStory, RefPath};
use crate::para::{FieldSource, ParaEnv, PreparedPara, prepare_paragraph};
use crate::table::CellContent;
use crate::word;
use uo_jsnum::math as jm;
use uo_render::OptF64;
use uo_render::types::Item;

use super::flow::with_ldoc;
use super::page::SectGeom;
use super::{Cx, Engine, js_eq};

/// `NoteCut` (engine.ts:4145): a new note that does not fit whole with its reference line — its
/// beginning (on this page) and the rest (at the foot of the next).
#[derive(Clone, Debug)]
pub struct NoteCut {
    pub id: NumKey,
    pub first: CellContent,
    pub rest: CellContent,
}

/// `enFlow` (engine.ts:2442): the endnote flow in progress.
#[derive(Clone, Copy, Debug)]
pub struct EnFlow {
    /// the separator is out: following pages and columns get the continuation separator
    pub sep_done: bool,
}

/// `sepCache` (engine.ts:2301): the separator line.
#[derive(Clone, Copy, Debug)]
pub struct SepMetrics {
    pub h: f64,
    pub mid: f64,
    pub thick: f64,
}

/// A note continuing on the next page (`noteCarry` entry).
#[derive(Clone, Debug)]
pub struct NoteCarry {
    pub id: NumKey,
    pub content: CellContent,
}

/// A block of the endnote flow (`blocks`, `refs`, `owner` of `placeEndnotes`).
#[derive(Clone, Debug)]
pub struct EnBlock {
    pub block: LBlock,
    /// `ensep` or `en<id>/b<k>`
    pub at: LineRef,
    /// the endnote the block belongs to (`curEndnote`)
    pub owner: Option<NumKey>,
    /// the story of the block (a table is read from its model story)
    pub story: Option<u32>,
}

/// The runs of a paragraph as `(text length, footnote id)` (`paraByRef`'s answer).
type RunNotes = Vec<(f64, Option<NumKey>)>;

/// `noteCols(g)`: the note area's columns.
struct NoteCols {
    n: f64,
    w: f64,
    dx: Vec<f64>,
}

/// `{ spacing: { before: 0, after: 0, line: 240, lineRule: 'auto' } }` (+ `keepNext: true`).
fn sep_ppr(keep_next: bool) -> PPr {
    let mut sp = uo_docx::model::props::SpacingSpec::default();
    word::obj::set(&mut sp, "before", |s| s.before = Some(crate::model::JsNumber::Num(0.0)));
    word::obj::set(&mut sp, "after", |s| s.after = Some(crate::model::JsNumber::Num(0.0)));
    word::obj::set(&mut sp, "line", |s| s.line = Some(crate::model::JsNumber::Num(240.0)));
    word::obj::set(&mut sp, "lineRule", |s| s.line_rule = Some("auto".into()));
    let mut ppr = PPr::default();
    word::obj::set(&mut ppr, "spacing", |p| p.spacing = Some(Box::new(sp)));
    if keep_next {
        word::obj::set(&mut ppr, "keepNext", |p| p.keep_next = Some(true));
    }
    ppr
}

/// The endnote separator paragraph of the derived document (built by `derive::build` when the
/// document has endnotes): a copy of the first model paragraph with the separator's `ppr` and
/// `props` and no runs — so nothing of the base is read (`fields_of` takes both overrides).
pub fn endnote_separator(ldoc: &mut LDoc<'_>) -> LResult<Option<LParaId>> {
    let Some(base) = ldoc.paras.iter().position(|p| matches!(p.src, ParaSrc::Model { .. })) else { return Ok(None) };
    let base = LParaId::of(base).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
    let (story, path) = match ldoc.para(base) {
        Some(p) => (p.story, p.path.clone()),
        None => return Ok(None),
    };
    let ppr = sep_ppr(true);
    let props = uo_docx::read::props_map::para_props_from_ppr(Some(&ppr));
    let id = LParaId::of(ldoc.paras.len()).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
    ldoc.paras.push(LPara {
        src: ParaSrc::Copy { base },
        ppr: Some(Arc::new(ppr)),
        props: Some(Arc::new(props)),
        runs: Vec::new(),
        rev_joins: Vec::new(),
        flags: ParaFlags::default(),
        frame_carry: None,
        story,
        path,
    });
    Ok(Some(id))
}

/// The note-area columns past which the layout stops (as the page columns, F7a-6).
const MAX_NOTE_COLUMNS: f64 = 4096.0;

/// `ToLength(x)`.
fn to_length(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return 0.0;
    }
    jm::min(jm::trunc(x), 9_007_199_254_740_991.0)
}

/// `packNotes(hs, n)`: whole notes per column — each column takes notes until it passes the
/// average (oracle notefmt, cols2); one column = in sequence.
fn pack_notes(hs: &[f64], n: f64) -> (Vec<usize>, Vec<f64>, f64) {
    let (mut col, mut top) = (Vec::with_capacity(hs.len()), Vec::with_capacity(hs.len()));
    let target = if n > 1.0 { hs.iter().fold(0.0, |a, b| a + b) / n } else { f64::INFINITY };
    let (mut c, mut y, mut max) = (0usize, 0.0, 0.0);
    for &h in hs {
        if y > 0.0 && y >= target - 0.5 && (c as f64) < n - 1.0 {
            c += 1;
            y = 0.0;
        }
        col.push(c);
        top.push(y);
        y += h;
        max = jm::max(max, y);
    }
    (col, top, max)
}

/// `noteLineFit(p, line)`: the height the line bringing a NEW note must fit above the notes — the
/// whole line (with its extra leading) and, on the paragraph's last line, its "after" (oracle
/// fnsep, sweeps fsep-top* / fsep-a0-*).
pub(crate) fn note_line_fit(p: &PreparedPara, line: &Line) -> f64 {
    line.height + if line.last { if p.after_auto { 280.0 } else { p.space_after } } else { 0.0 }
}

impl Engine {
    /// `noteCols(g)` (engine.ts:2223): with `w15:footnoteColumns`, N equal columns 720 tw apart
    /// (oracle notefmt, cols2); without ("match section layout"), the section's own columns
    /// (sect-cols2). `x` relative to the left margin; `w` = the first column's.
    fn note_cols(g: &SectGeom) -> LResult<NoteCols> {
        let content_w = g.page_w - g.mar_l - g.mar_r;
        let ncols = f64::from(u32::try_from(g.cols.len()).unwrap_or(u32::MAX));
        let n = g.notes.fn_cols.get().unwrap_or(ncols);
        if g.notes.fn_cols.get().is_none() && n > 1.0 {
            let w = g.cols.first().map_or(f64::NAN, |c| c.w);
            return Ok(NoteCols { n, w, dx: g.cols.iter().map(|c| c.x - g.mar_l).collect() });
        }
        let gap = 720.0;
        let w = if n > 1.0 { (content_w - gap * (n - 1.0)) / n } else { content_w };
        // `Array.from({ length: n })`: past MAX_NOTE_COLUMNS the layout stops (F7c-13; the TS
        // allocates one entry per column of `w15:footnoteColumns`)
        let len = to_length(n);
        if len > MAX_NOTE_COLUMNS {
            return Err(LayoutError::Limit(LimitKind::Capacity));
        }
        let mut dx = Vec::new();
        let mut k = 0.0;
        while k < len {
            dx.push(k * (w + gap));
            k += 1.0;
        }
        Ok(NoteCols { n, w, dx })
    }

    /// `noteContent(id)` (engine.ts:2232): the laid-out content of a footnote (the width of the
    /// body or of the note column), cached by id and width.
    pub(crate) fn note_content(&mut self, cx: &mut Cx<'_, '_>, id: NumKey) -> LResult<CellContent> {
        let gi = self.cur_ps()?.geom;
        let g = self.geom(gi)?;
        let w = Self::note_cols(g)?.w;
        let mar_l = g.mar_l;
        let key: Box<str> = format!("{}:{}", uo_jsnum::format::to_js_string(id.get()), uo_jsnum::format::to_js_string(jm::round(w))).into();
        if let Some(c) = self.note_cache.get(&key) {
            return Ok(c.clone());
        }
        let blocks = match cx.ldoc.note_stories.get(&(NoteKind::Footnote, id)) {
            Some(&s) => self.story_blocks(cx, s)?,
            None => Vec::new(),
        };
        // the note's own number is the reference's mark (in its section's format)
        let saved = std::mem::replace(&mut self.ctx.flow.numbering, word::numbering_state(&cx.opts.lang, &cx.opts.limits));
        let env = ParaEnv { field: Some(FieldSource::Footnote(id)), ..ParaEnv::default() };
        let at = LineRef { story: LineStory::Note(NoteKind::Footnote, id), path: RefPath::default() };
        let r = self.layout_static(cx, &blocks, mar_l, w, &env, &at, None, 0.0, None);
        self.ctx.flow.numbering = saved;
        let c = r?;
        self.note_cache.insert(key, c.clone());
        Ok(c)
    }

    /// `rowNoteIds(row)` (engine.ts:2255): the NEW footnotes referenced by the text lines of a
    /// table row (each line names its source paragraph in its ref: `b3/r2/c0/b1`…). For F7b's
    /// row emission.
    pub(crate) fn row_note_ids(&self, cx: &Cx<'_, '_>, lines: &[PassLine]) -> LResult<Vec<NumKey>> {
        let mut ids: Vec<NumKey> = Vec::new();
        let cur = self.cur_ps()?;
        for l in lines {
            let Some(runs) = self.para_by_ref(cx, &l.at)? else { continue };
            let mut off = 0.0;
            for (len, note) in runs {
                if let Some(id) = note
                    && off + jm::max(len, 1.0) > f64::from(l.start)
                    && off < jm::max(f64::from(l.end), f64::from(l.start) + 1.0)
                    && !ids.contains(&id)
                    && !cur.notes.iter().any(|n| js_eq(n.id.get(), id.get()))
                {
                    ids.push(id);
                }
                off += len;
            }
        }
        Ok(ids)
    }

    /// `rowNoteExtra(row)` (engine.ts:2272).
    pub(crate) fn row_note_extra(&mut self, cx: &mut Cx<'_, '_>, lines: &[PassLine]) -> LResult<f64> {
        let ids = self.row_note_ids(cx, lines)?;
        self.notes_extra(cx, &ids)
    }

    /// `paraByRef(ref)` (engine.ts:2277): the body paragraph of a ref path (`b3/r2/c0/b1`; `cN` is
    /// the GRID column — oracle cellnote, corpus t34), as its runs' `(text length, footnote id)`.
    fn para_by_ref(&self, cx: &Cx<'_, '_>, at: &LineRef) -> LResult<Option<RunNotes>> {
        if at.story != LineStory::Body {
            return Ok(None);
        }
        let seg = &at.path.0;
        let Some(&b0) = seg.first() else { return Ok(None) };
        let note_of = |r: &crate::model::Run| -> Option<NumKey> {
            let nr = r.ext()?.note_ref.as_ref()?;
            (&*nr.kind == "footnote").then(|| NumKey::new(nr.id.to_number()))
        };
        let Some(first) = self.flow_block(cx, b0 as usize) else { return Ok(None) };
        if seg.len() == 1 {
            let LBlock::Paragraph(k) = first else { return Ok(None) };
            let lp = cx.ldoc.para(k).ok_or(LayoutError::Internal("paraByRef: paragraph"))?;
            let mut out = Vec::with_capacity(lp.runs.len());
            for &rk in &lp.runs {
                let len = f64::from(u32::try_from(cx.ldoc.run_text(rk).len()).unwrap_or(u32::MAX));
                out.push((len, crate::paint::model_run(cx.ldoc, rk).and_then(note_of)));
            }
            return Ok(Some(out));
        }
        let LBlock::Table { block } = first else { return Ok(None) };
        let Some(t0) = self.flow_table(cx, b0 as usize, block) else { return Ok(None) };
        let mut blk = uo_docx::model::story::Block::Table(t0);
        // `for (let i = 1; i + 2 < seg.length + 1 && block; i += 3)` over the segments after b<n>
        let mut i = 1;
        while i + 1 < seg.len() {
            let uo_docx::model::story::Block::Table(t) = blk else { return Ok(None) };
            let (Some(&ri), Some(&ci), Some(&bi)) = (seg.get(i), seg.get(i + 1), seg.get(i + 2)) else { return Ok(None) };
            let Some(row) = t.rows.get(ri as usize) else { return Ok(None) };
            let mut col = row.tr_pr.as_ref().and_then(|p| p.grid_before).map_or(0.0, crate::model::JsNumber::to_number);
            let mut cell = None;
            for c in &row.cells {
                if js_eq(col, f64::from(ci)) {
                    cell = Some(c);
                    break;
                }
                let span = c.tc_pr.as_ref().and_then(|p| p.grid_span).or(Some(c.grid_span)).map(crate::model::JsNumber::to_number);
                col += jm::max(1.0, span.unwrap_or(1.0));
            }
            let Some(cell) = cell else { return Ok(None) };
            let Some(next) = cell.content.blocks().nth(bi as usize) else { return Ok(None) };
            blk = next;
            i += 3;
        }
        match blk {
            uo_docx::model::story::Block::Paragraph(p) => {
                Ok(Some(p.runs.iter().map(|r| (f64::from(u32::try_from(r.text.len()).unwrap_or(u32::MAX)), note_of(r))).collect()))
            }
            uo_docx::model::story::Block::Table(_) => Ok(None),
        }
    }

    /// The prepared separator paragraph and its first line (`sepGeom`'s measure).
    fn sep_measure(&mut self, cx: &mut Cx<'_, '_>) -> LResult<(PreparedPara, Line)> {
        let ppr = sep_ppr(false);
        let props = uo_docx::read::props_map::para_props_from_ppr(Some(&ppr));
        let para = Paragraph { ppr: Some(Arc::new(ppr)), props: Arc::new(props), ..Paragraph::empty(crate::model::ParaId(u32::MAX)) };
        let mut tiny = LDoc::empty(cx.ldoc.view);
        tiny.theme = cx.ldoc.theme; // wave 7 (B, gate-O3)
        tiny.paras.push(LPara {
            src: ParaSrc::Model { para: &para, id: para.id },
            ppr: None,
            props: None,
            runs: Vec::new(),
            rev_joins: Vec::new(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: crate::model::StoryRef::Body,
            path: crate::model::BlockPath::root(0),
        });
        let mut tcx = with_ldoc!(cx, &tiny);
        let p = prepare_paragraph(&mut tcx, self, LParaId(0), &ParaEnv::default())?;
        let l =
            LineBreaker::new(&p, None).next(&mut tcx, &p, crate::lines::LineFrame { left: 0.0, right: 10000.0, gap: false, next: None })?;
        Ok((p, l))
    }

    /// `sepGeom()` (engine.ts:2308): the separator line — its height (one SINGLE line of the
    /// Normal font, no spacing: oracle fnsep), the middle of the rule (0.23 em above the line's
    /// baseline) and its thickness (0.05 em in 1/600" pixels, rounded up: oracle endnotes, sep-*).
    fn sep_geom(&mut self, cx: &mut Cx<'_, '_>) -> LResult<SepMetrics> {
        if let Some(s) = self.sep_cache {
            return Ok(s);
        }
        let (p, l) = self.sep_measure(cx)?;
        let size = p.styles.get(p.mark_style.0 as usize).map_or(0.0, |s| s.size_pt);
        let em = (if uo_jsnum::conv::truthy(size) { size } else { 10.0 }) * 20.0;
        let s = SepMetrics { h: l.height, mid: l.baseline - 0.23 * em, thick: jm::max(2.4, jm::ceil((0.05 * em) / 2.4 - 0.01) * 2.4) };
        self.sep_cache = Some(s);
        Ok(s)
    }

    /// `separatorHeight()` (engine.ts:2320).
    pub(crate) fn separator_height(&mut self, cx: &mut Cx<'_, '_>) -> LResult<f64> {
        Ok(self.sep_geom(cx)?.h)
    }

    /// `sepLine(x, top, w)` (engine.ts:2326): the separator's rule from the top of its line.
    fn sep_line(&mut self, cx: &mut Cx<'_, '_>, x: f64, top: f64, w: f64, out: &mut PassItems) -> LResult<()> {
        let sg = self.sep_geom(cx)?;
        let fill = cx.res.symbols.intern(&[0x30; 6]).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
        out.list.push_plain(Item::Rect { x, y: top + sg.mid - sg.thick / 2.0, w, h: sg.thick, fill, alpha: OptF64::NONE });
        out.side.push(ItemSide::default());
        Ok(())
    }

    /// `noteAreaOf(contents, g)` (engine.ts:2332): separator + notes (in columns, the tallest).
    fn note_area_of(&mut self, cx: &mut Cx<'_, '_>, heights: &[f64], gi: u32) -> LResult<f64> {
        if heights.is_empty() {
            return Ok(0.0);
        }
        let n = Self::note_cols(self.geom(gi)?)?.n;
        Ok(self.separator_height(cx)? + pack_notes(heights, n).2)
    }

    /// `notesExtra(ids)` (engine.ts:2353): the room the NEW notes `ids` add to the page's area.
    fn notes_extra(&mut self, cx: &mut Cx<'_, '_>, ids: &[NumKey]) -> LResult<f64> {
        if ids.is_empty() {
            return Ok(0.0);
        }
        let ps = self.cur_ps()?;
        let gi = ps.geom;
        let cur: Vec<f64> = ps.notes.iter().map(|n| n.content.height).collect();
        let mut all = cur.clone();
        for &id in ids {
            all.push(self.note_content(cx, id)?.height);
        }
        Ok(self.note_area_of(cx, &all, gi)? - self.note_area_of(cx, &cur, gi)?)
    }

    /// `noteNeed(p, line)` (engine.ts:2367): the NEW notes referenced in the line and the room
    /// they need.
    pub(crate) fn note_need(&mut self, cx: &mut Cx<'_, '_>, p: &PreparedPara, line: &Line) -> LResult<(Vec<NumKey>, f64)> {
        let mut ids: Vec<NumKey> = Vec::new();
        {
            let cur = self.cur_ps()?;
            for k in line.start..line.end {
                let Some(id) = p.glyphs.get(k as usize).and_then(|g| g.note_id.get()).map(NumKey::new) else { continue };
                if !ids.contains(&id) && !cur.notes.iter().any(|n| js_eq(n.id.get(), id.get())) {
                    ids.push(id);
                }
            }
        }
        let extra = self.notes_extra(cx, &ids)?;
        Ok((ids, extra))
    }

    /// `reserveNotes(ids, cut)` (engine.ts:2376).
    pub(crate) fn reserve_notes(&mut self, cx: &mut Cx<'_, '_>, ids: &[NumKey], cut: Option<&NoteCut>) -> LResult<()> {
        let pix = self.cur_ix()?;
        for &id in ids {
            if self.ps(pix)?.notes.iter().any(|n| js_eq(n.id.get(), id.get())) {
                continue;
            }
            let content = match cut {
                Some(c) if js_eq(c.id.get(), id.get()) => c.first.clone(),
                _ => self.note_content(cx, id)?,
            };
            self.ps_mut(pix)?.notes.push(super::page::PageNote { id, content });
            if let Some(c) = cut.filter(|c| js_eq(c.id.get(), id.get())) {
                self.note_carry.push(NoteCarry { id, content: c.rest.clone() });
            }
        }
        self.update_note_area(cx, pix)
    }

    /// `ps.noteArea = noteAreaOf(ps.notes…, ps.geom); ps.bodyBottom = ps.bodyBottomBase − ps.noteArea`.
    fn update_note_area(&mut self, cx: &mut Cx<'_, '_>, pix: PageIx) -> LResult<()> {
        let ps = self.ps(pix)?;
        let gi = ps.geom;
        let hs: Vec<f64> = ps.notes.iter().map(|n| n.content.height).collect();
        let area = self.note_area_of(cx, &hs, gi)?;
        let ps = self.ps_mut(pix)?;
        ps.note_area = area;
        ps.body_bottom = ps.body_bottom_base - area;
        Ok(())
    }

    /// `notePartial(ids, bottom)` (engine.ts:2402): a NEW note that does not fit whole with the
    /// reference line (ending at `bottom`): its beginning stays on this page — what fits, with
    /// the note's own widows/orphans — and the rest continues at the next page's foot (oracle
    /// fncont). Only the last note of the line splits; none in a note area in columns.
    pub(crate) fn note_partial(&mut self, cx: &mut Cx<'_, '_>, ids: &[NumKey], bottom: f64) -> LResult<Option<NoteCut>> {
        let gi = self.cur_ps()?.geom;
        if ids.is_empty() || Self::note_cols(self.geom(gi)?)?.n > 1.0 {
            return Ok(None);
        }
        let mut h = if self.cur_ps()?.notes.is_empty() { self.separator_height(cx)? } else { 0.0 };
        let (last, init) = ids.split_last().ok_or(LayoutError::Internal("notePartial"))?;
        for &id in init {
            h += self.note_content(cx, id)?.height;
        }
        let avail = self.cur_ps()?.body_bottom - bottom - h;
        if avail <= 0.0 {
            return Ok(None);
        }
        let c = self.note_content(cx, *last)?;
        let sp = crate::table::split_content(cx, &c, avail)?;
        Ok(match sp {
            Some((a, b)) if !a.lines.is_empty() && !b.lines.is_empty() => Some(NoteCut { id: *last, first: a, rest: b }),
            _ => None,
        })
    }

    /// `paintNotes(ps)` (engine.ts:2418): separator + notes, aligned to the body's bottom (measured
    /// in Word); "beneath text" (`w:pos beneathText`): right after the page's content, like the
    /// endnotes (oracle notefmt, pos-beneath*). The notes' lines go into the page.
    pub(crate) fn paint_notes(&mut self, cx: &mut Cx<'_, '_>, pix: PageIx) -> LResult<()> {
        if self.ps(pix)?.notes.is_empty() {
            return Ok(());
        }
        let ps = self.ps(pix)?;
        let gi = ps.geom;
        let g = self.geom(gi)?.clone();
        let mut y = ps.body_bottom_base - ps.note_area;
        if &*g.notes.fn_pos == "beneathText" && self.cur == Some(pix) {
            let end = jm::max(self.y, self.col_max_y)
                + match &self.prev {
                    Some(p) if !self.at_top => p.after,
                    _ => 0.0,
                };
            y = jm::max(ps.body_top, jm::min(y, end));
        }
        // the 2" (144 pt) rule of the separator
        let mut body = std::mem::take(&mut self.ps_mut(pix)?.body);
        let r = self.sep_line(cx, g.mar_l, y, 2880.0, &mut body);
        self.ps_mut(pix)?.body = body;
        r?;
        y += self.separator_height(cx)?;
        let cols = Self::note_cols(&g)?;
        let notes = std::mem::take(&mut self.ps_mut(pix)?.notes);
        let hs: Vec<f64> = notes.iter().map(|n| n.content.height).collect();
        let (col, top, _) = pack_notes(&hs, cols.n);
        let ps = self.ps_mut(pix)?;
        for (i, n) in notes.iter().enumerate() {
            let dx = col.get(i).and_then(|c| cols.dx.get(*c)).copied().unwrap_or(0.0);
            let dy = y + top.get(i).copied().unwrap_or(f64::NAN);
            for k in 0..n.content.items.len() {
                ps.body.push_shifted(&n.content.items, k, dy, dx).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
            }
            for l in &n.content.lines {
                ps.page.lines.push(l.shifted_xy(dx, dy));
            }
        }
        ps.notes = notes;
        Ok(())
    }

    /// The continuation of notes split on the previous page, first at this page's foot
    /// (`makePage`, engine.ts:924): a note that does not fit whole here either fills the page
    /// (notes only, aligned to the foot) and the rest goes on (oracle notefmt, cont-sep-3p).
    pub(crate) fn place_note_carry(&mut self, cx: &mut Cx<'_, '_>, gi: u32) -> LResult<()> {
        let pix = self.cur_ix()?;
        let carry = std::mem::take(&mut self.note_carry);
        for n in carry {
            let ps = self.ps(pix)?;
            let hs: Vec<f64> = ps.notes.iter().map(|x| x.content.height).collect();
            let (base, top, empty, full) = (ps.body_bottom_base, ps.body_top, ps.notes.is_empty(), ps.notes_full);
            let sep = if empty { self.separator_height(cx)? } else { 0.0 };
            let avail = base - top - self.note_area_of(cx, &hs, gi)? - sep;
            let sp = if n.content.height > avail + 0.05 && !full { crate::table::split_content(cx, &n.content, avail)? } else { None };
            match sp {
                Some((a, b)) if !a.lines.is_empty() && !b.lines.is_empty() => {
                    let ps = self.ps_mut(pix)?;
                    ps.notes.push(super::page::PageNote { id: n.id, content: a });
                    ps.notes_full = true;
                    self.note_carry.push(NoteCarry { id: n.id, content: b });
                }
                _ if full => self.note_carry.push(n),
                _ => self.ps_mut(pix)?.notes.push(super::page::PageNote { id: n.id, content: n.content }),
            }
        }
        let ps = self.ps(pix)?;
        let hs: Vec<f64> = ps.notes.iter().map(|x| x.content.height).collect();
        let area = self.note_area_of(cx, &hs, gi)?;
        let ps = self.ps_mut(pix)?;
        ps.note_area = area;
        ps.body_bottom = ps.body_bottom_base - area;
        Ok(())
    }

    /// `continuationSeparator(draw)` (engine.ts:2507): the continuation separator at the top of a
    /// new page/column of the endnote flow (`draw`: the rule, as wide as the column; in the next
    /// column only the room).
    pub(crate) fn continuation_separator(&mut self, cx: &mut Cx<'_, '_>, draw: bool) -> LResult<()> {
        let fr = self.col_frame()?;
        if draw {
            let pix = self.cur_ix()?;
            let y = self.y;
            let mut body = std::mem::take(&mut self.ps_mut(pix)?.body);
            let r = self.sep_line(cx, fr.x, y, fr.w, &mut body);
            self.ps_mut(pix)?.body = body;
            r?;
        }
        self.y += self.separator_height(cx)?;
        Ok(())
    }

    /// The block `i` of the flow being walked: the endnote flow's while it comes out, else the
    /// story's (`this.full.blocks[i]`).
    pub(crate) fn flow_block(&self, cx: &Cx<'_, '_>, i: usize) -> Option<LBlock> {
        self.note_read(i);
        match &self.en_blocks {
            Some(en) => en.get(i).map(|b| b.block),
            None => cx.ldoc.stories.get(self.story as usize).and_then(|s| s.blocks.get(i)).copied(),
        }
    }

    /// The TS ref of block `i` of the flow (`b<i>`, or the endnote flow's `ensep` / `en<id>/b<k>`).
    pub(crate) fn flow_ref(&self, i: usize) -> LineRef {
        match self.en_blocks.as_ref().and_then(|en| en.get(i)) {
            Some(b) => b.at.clone(),
            None => LineRef { story: LineStory::Body, path: RefPath(vec![u32::try_from(i).unwrap_or(u32::MAX)].into()) },
        }
    }

    /// `placeEndnotes(g)` (engine.ts:2455): the endnotes pending in the flow, after the
    /// section's/document's content — the separator (one single line of the Normal font, after
    /// the last paragraph's "after", keeping the first line of the first note with it) and the
    /// notes in reference order; the continuation on another page has the continuation separator
    /// (oracle endnotes). Lines with ref `en<id>/b<k>`.
    pub(crate) fn place_endnotes(&mut self, cx: &mut Cx<'_, '_>, gi: u32) -> LResult<()> {
        let order = self.ctx.endnote_order().to_vec();
        let done = self.endnotes_done as usize;
        self.endnotes_done = u32::try_from(order.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        let ids = order.get(done..).unwrap_or(&[]);
        if ids.is_empty() {
            return Ok(());
        }
        let mut blocks: Vec<EnBlock> = Vec::new();
        for &id in ids {
            let Some(&s) = cx.ldoc.note_stories.get(&(NoteKind::Endnote, id)) else { continue };
            let st = cx.ldoc.stories.get(s as usize).ok_or(LayoutError::Internal("placeEndnotes: story"))?;
            for (k, b) in st.blocks.iter().enumerate() {
                let k = u32::try_from(k).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
                blocks.push(EnBlock {
                    block: *b,
                    at: LineRef { story: LineStory::Note(NoteKind::Endnote, id), path: RefPath(vec![k].into()) },
                    owner: Some(id),
                    story: Some(s),
                });
            }
        }
        if blocks.is_empty() {
            return Ok(());
        }
        // the separator paragraph first (`derive` builds it whenever there is an endnote story)
        let sep = cx.ldoc.en_sep.ok_or(LayoutError::Internal("placeEndnotes: no separator paragraph"))?;
        blocks.insert(
            0,
            EnBlock {
                block: LBlock::Paragraph(sep),
                at: LineRef { story: LineStory::EndnoteSep, path: RefPath::default() },
                owner: None,
                story: None,
            },
        );
        let n = blocks.len();
        let g = self.geom(gi)?.clone();
        // in columns the note area is like a continuous section: the text before and the notes are
        // balanced apart (oracle endnotes, en-cols)
        let multi = self.page_cols()?.len() > 1 || g.cols.len() > 1;
        let mut g_en = g.clone();
        g_en.blocks = (0.0, f64::from(u32::try_from(n - 1).unwrap_or(u32::MAX)));
        if multi {
            g_en.bal_key = Some(-1 - i64::from(g.index));
        }
        let gen_ix = u32::try_from(self.geoms.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        self.geoms.push(g_en);
        // the flow of the notes uses the body's machinery, over their blocks
        self.en_blocks = Some(blocks);
        let r = self.place_endnotes_in(cx, gen_ix, n, multi);
        self.en_blocks = None;
        self.en_flow = None;
        self.cur_endnote = None;
        r
    }

    fn place_endnotes_in(&mut self, cx: &mut Cx<'_, '_>, gen_ix: u32, n: usize, multi: bool) -> LResult<()> {
        if multi {
            self.continuous_section(cx, gen_ix)?;
        }
        self.en_flow = Some(EnFlow { sep_done: false });
        for b in 0..n {
            cx.budget.charge(1)?;
            let (block, owner, story) = match self.en_blocks.as_ref().and_then(|e| e.get(b)) {
                Some(e) => (e.block, e.owner, e.story),
                None => return Err(LayoutError::Internal("placeEndnotes: block")),
            };
            self.cur_endnote = owner;
            match block {
                LBlock::Paragraph(p) => self.place_paragraph(cx, p, b, gen_ix)?,
                LBlock::Table { block: tb } => {
                    // `placeTable(block, refs[b])` (no body block index)
                    let st = story.ok_or(LayoutError::Internal("placeEndnotes: table story"))?;
                    let t = super::tables::story_table(cx, st, tb)?;
                    let at = self.flow_ref(b);
                    self.place_table(cx, t, &at, None)?;
                }
            }
            if b == 0 {
                // the rule on the separator's line (the line leaves the page: it is not text)
                let pix = self.cur_ix()?;
                let at = self.ps(pix)?.page.lines.iter().rposition(|l| l.at.story == LineStory::EndnoteSep);
                if let Some(at) = at {
                    let l = self.ps_mut(pix)?.page.lines.remove(at);
                    let mut body = std::mem::take(&mut self.ps_mut(pix)?.body);
                    let r = self.sep_line(cx, l.x, l.y, 2880.0, &mut body);
                    self.ps_mut(pix)?.body = body;
                    r?;
                }
                if let Some(f) = self.en_flow.as_mut() {
                    f.sep_done = true;
                }
            }
        }
        if multi {
            self.record_balance(gen_ix)?;
        }
        Ok(())
    }

    /// The footnote mark of a note's own `FOOTNOTEREF_MARK` field.
    pub(crate) fn footnote_mark(&self, id: NumKey) -> Box<[u16]> {
        self.ctx.note_mark_of(crate::context::NoteKindFlow::Footnote, id).map(Into::into).unwrap_or_default()
    }
}
