//! D2b family: headings and navigation (D2b-F9, record `core/docs/modules/uo-docx-d2b/F9.md`):
//! `selectAll`, `setSelectionRanges` and the non-contiguous selection (`extras`, read by the
//! formatting family and Backspace/Delete), the Navigation pane (`getHeadings`, `moveHeading`,
//! `promoteHeading`/`demoteHeading`, `insertHeading`, `deleteHeading`, `selectHeading`,
//! `goToHeading`, `search`, `goToSearchResult`), and `navigation.ts` (the pure helpers below).
//! Scrolling, the page of a heading or result and the highlight are the shell's (layout).

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::policy::GuardArg;
use super::super::state::{CellHop, Selection};
use super::super::text::{DocPosition, DocRange, compare_pos, normalize_range, para_length, para_text};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use super::find::{FindOutcome, ReplaceOutcome};
use crate::model::para::{ParaProps, Paragraph};
use crate::model::story::{Block, Story};
use crate::model::table::Table;
use crate::pvec::PVec;
use crate::text::Utf16;
use crate::view::DocView;
use crate::word::obj;

/// Where `insertHeading` puts the new heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingWhere {
    /// Novo Título Antes: same level, before the heading
    Before,
    /// Novo Título Depois: same level, after its section
    After,
    /// Novo Subtítulo: one level down, at the end of its section
    Sub,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationCmd {
    /// `selectAll()`
    SelectAll,
    /// `setSelectionRanges(ranges)`: several ranges at once (the last is the main one)
    SetSelectionRanges { ranges: Vec<DocRange> },
    /// `moveHeading(index, before)`
    MoveHeading { index: i64, before: i64 },
    /// `promoteHeading(index)`
    PromoteHeading { index: i64 },
    /// `demoteHeading(index)`
    DemoteHeading { index: i64 },
    /// `insertHeading(index, where)`
    InsertHeading { index: i64, at: HeadingWhere },
    /// `deleteHeading(index)`
    DeleteHeading { index: i64 },
    /// `selectHeading(index)`
    SelectHeading { index: i64 },
    /// `goToHeading(index)` (the scroll is the shell's)
    GoToHeading { index: i64 },
    /// `search(text, opts)`: the Navigation pane's search (body and tables); the results are
    /// [`Session::search_results`], the highlight the shell's (`highlight`)
    Search { text: Vec<u16>, opts: FindOptions },
    /// `clearSearch()`
    ClearSearch,
    /// `goToSearchResult(index)`
    GoToSearchResult { index: i64 },
}

/// `HeadingInfo` without the page (the layout's; the shell adds it).
#[derive(Debug, Clone, PartialEq)]
pub struct HeadingInfo {
    pub index: u32,
    /// outline level 1–9
    pub level: f64,
    /// list number + text, as the pane shows it
    pub text: Utf16,
    /// the body paragraph
    pub para: u32,
}

/// Where a search result is (`loc` of `searchList`).
#[derive(Debug, Clone, PartialEq)]
pub enum SearchLoc {
    Body { para: u32 },
    Cell { anchor: u32, hops: Vec<CellHop>, para: u32 },
}

/// `SearchResult` without the page (the layout's).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub index: u32,
    pub start: u32,
    pub end: u32,
    pub snippet: Snippet,
    /// the heading of the section (`getHeadings` index, -1 before the first)
    pub heading: i64,
    pub in_table: bool,
    pub loc: SearchLoc,
}

/// What the last command of this family (or of find) answered: the TS return value.
#[derive(Debug, Clone, PartialEq)]
pub enum F9Result {
    Bool(bool),
    Count(u32),
    Find(FindOutcome),
    Replace(ReplaceOutcome),
}

/// The state of the find and navigation family (D2b-F9).
#[derive(Debug, Clone, Default)]
pub struct F9State {
    /// `extraSel`: the ranges beyond the main selection, normalised and non-empty
    extra: Vec<DocRange>,
    /// `extraBase`: the main selection (and its context) they belong to — the TS compares the
    /// selection OBJECT; here the value and the context, and every assignment that makes a new
    /// object without a new value drops them (`drop_extras`: undo/redo, `setSelection`)
    extra_base: Option<(Selection, EditContext)>,
    /// `searchList` (the last search of the Navigation pane)
    search: Vec<SearchResult>,
    /// `searchCurrent`
    search_current: i64,
    /// the answer of the last command
    pub(crate) result: Option<F9Result>,
    /// `spellcheck-default-lang` (`None`: `pt-BR`)
    pub(crate) spell_default_lang: Option<String>,
    /// the step budget of a find command (`None`: [`super::find::regex::DEFAULT_STEP_BUDGET`])
    pub(crate) find_budget: Option<u64>,
    /// the spans of the last `highlightMatches` (`searchHits`; the paint is the shell's)
    pub(crate) highlight: Vec<(SearchLoc, u32, u32)>,
}

impl F9State {
    /// The selection got a new identity with the same value (undo/redo, `setSelection`): the extra
    /// ranges go, as the TS's `selection !== extraBase` drops them.
    pub(crate) fn drop_extras(&mut self) {
        self.extra.clear();
        self.extra_base = None;
    }

    /// `searchHits.clear(); searchList = []` (and `searchCurrent`).
    pub(crate) fn clear_search(&mut self) {
        self.search.clear();
        self.search_current = -1;
    }

    /// Bytes owned (the M3 record).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.extra.capacity() * std::mem::size_of::<DocRange>() + self.search.capacity() * std::mem::size_of::<SearchResult>()
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn idx(i: i64) -> Option<usize> {
    usize::try_from(i).ok()
}

/// The cells of a table for the searches (`findStories`'s and `search`'s `cells`): every cell
/// with its hops, depth first, nested tables after the cell's paragraphs.
pub(crate) fn table_cells<'a>(t: &'a Table, path: &[CellHop], out: &mut Vec<(Vec<CellHop>, &'a Story)>) {
    let mut stack: Vec<(&'a Table, Vec<CellHop>)> = vec![(t, path.to_vec())];
    // the TS recursion visits a cell, then its nested tables, then the next cell: an explicit
    // stack of pending (table, path, row, cell) keeps that order
    let mut work: Vec<(&'a Table, Vec<CellHop>, usize, usize, f64)> = Vec::new();
    if let Some((t, p)) = stack.pop() {
        work.push((t, p, 0, 0, f64::NAN));
    }
    while let Some((t, path, r, k, c)) = work.pop() {
        let Some(row) = t.rows.get(r) else { continue };
        let c = if k == 0 { row.tr_pr.as_deref().and_then(|p| p.grid_before).map_or(0.0, |g| g.to_number()) } else { c };
        let Some(cell) = row.cells.get(k) else {
            work.push((t, path, r + 1, 0, f64::NAN));
            continue;
        };
        let span = cell.tc_pr.as_deref().and_then(|p| p.grid_span).unwrap_or(cell.grid_span).to_number();
        let next_c = c + uo_jsnum::math::max(1.0, span);
        let col = uo_jsnum::conv::to_uint32(c);
        let mut hops = path.clone();
        hops.push(CellHop { row: u32_of(r), col, block: None });
        // after this cell and its nested tables: the next cell
        work.push((t, path.clone(), r, k + 1, next_c));
        let nested: Vec<(usize, &'a Table)> = cell
            .content
            .blocks()
            .enumerate()
            .filter_map(|(bi, b)| match b {
                Block::Table(nt) => Some((bi, nt)),
                Block::Paragraph(_) => None,
            })
            .collect();
        for (bi, nt) in nested.into_iter().rev() {
            let mut p = path.clone();
            p.push(CellHop { row: u32_of(r), col, block: Some(u32_of(bi)) });
            work.push((nt, p, 0, 0, f64::NAN));
        }
        out.push((hops, &cell.content));
    }
}

impl Session {
    /// The family's answer to the last command (`F9Result`).
    #[must_use]
    pub fn navigation_result(&self) -> Option<&F9Result> {
        self.f9.result.as_ref()
    }

    /// `spellcheck-default-lang` for the searches (`allForms` reads the run language).
    pub fn set_spell_default_lang(&mut self, lang: Option<String>) {
        self.f9.spell_default_lang = lang;
    }

    /// The step budget of a find command (`None`: the default; tests set small ones).
    pub fn set_find_budget(&mut self, steps: Option<u64>) {
        self.f9.find_budget = steps;
    }

    /// `extras()`: the selected ranges beyond the main one (non-contiguous selection), each
    /// normalised; valid only while the main selection is unchanged.
    pub(crate) fn extra_ranges(&self) -> Vec<DocRange> {
        match &self.f9.extra_base {
            Some((sel, ctx)) if *sel == self.selection && *ctx == self.ctx => self.f9.extra.clone(),
            _ => Vec::new(),
        }
    }

    /// `setExtraSelection(ranges)`: the ranges beyond the main selection (`selectSimilarFormatting`,
    /// `selectAllMatches`, `setSelectionRanges`).
    pub(crate) fn set_extra_selection(&mut self, ranges: Vec<DocRange>) {
        self.f9.extra = ranges.into_iter().map(normalize_range).filter(|r| compare_pos(r.start, r.end).is_ne()).collect();
        self.f9.extra.shrink_to_fit();
        self.f9.extra_base = if self.f9.extra.is_empty() { None } else { Some((self.selection, self.ctx.clone())) };
    }

    /// `getSelectionRanges()`: every selected range (the extra ones and the main one) in document
    /// order.
    #[must_use]
    pub fn selection_ranges(&self) -> Vec<DocRange> {
        let mut all = self.extra_ranges();
        all.push(normalize_range(self.range()));
        all.sort_by(|a, b| compare_pos(a.start, b.start));
        all
    }

    /// `deleteSelectedRanges()`: Delete/Backspace and Cut over a non-contiguous selection, from the
    /// last range to the first; the caret at the start of the first. Called by the text family
    /// (`pressBackspace`/`pressDelete` with a selection).
    pub(crate) fn delete_selected_ranges(&mut self) -> Result<DocPosition, CommandError> {
        let more = self.extra_ranges();
        if more.is_empty() {
            return self.delete_selection();
        }
        let mut all = more;
        all.push(normalize_range(self.range()));
        all.sort_by(|a, b| compare_pos(b.start, a.start));
        self.f9.drop_extras();
        let mut pos = all.last().map_or(self.selection.focus, |r| r.start);
        for r in all {
            self.selection = Selection { anchor: r.start, focus: r.end };
            pos = self.delete_selection()?;
        }
        Ok(pos)
    }

    /// `toBody()`: out of a note, textbox, header/footer or cell to work on the body.
    pub(crate) fn back_to_body(&mut self) -> bool {
        if matches!(self.ctx, EditContext::Note { .. }) {
            self.exit_note();
        }
        if matches!(self.ctx, EditContext::Textbox { .. }) {
            self.exit_textbox();
        }
        if let EditContext::HeaderFooter { saved, .. } = self.ctx {
            self.ctx = EditContext::Body;
            self.selection = saved;
            self.pending_props = None;
            self.tx.events.push(Event::HfMode { active: false });
        }
        if matches!(self.ctx, EditContext::Cell { .. }) {
            self.exit_cell();
        }
        true
    }

    /// `getHeadings()`: the body's headings (level 1–9, the text as the pane shows it, the
    /// paragraph); headings in tables, headers or textboxes do not count.
    #[must_use]
    pub fn headings(&self) -> Vec<HeadingInfo> {
        let view = DocView::new(&self.doc);
        let outline = crate::word::outline::document_outline_with(view, None, self.num_rules());
        if outline.is_empty() {
            return Vec::new();
        }
        // block index → body paragraph (`doc.origins[block]`)
        let mut para_of: Vec<Option<u32>> = Vec::with_capacity(self.doc.body.paras.len() + self.doc.body.tables.len());
        let mut pi = 0u32;
        for b in self.doc.body.blocks() {
            match b {
                Block::Paragraph(_) => {
                    para_of.push(Some(pi));
                    pi += 1;
                }
                Block::Table(_) => para_of.push(None),
            }
        }
        let mut out = Vec::new();
        for h in outline {
            let Some(para) = para_of.get(h.block as usize).copied().flatten() else { continue };
            if self.doc.body.paras.get(para as usize).is_none() {
                continue;
            }
            out.push(HeadingInfo { index: u32_of(out.len()), level: h.level, text: h.text, para });
        }
        out
    }

    /// `currentHeading()`: the heading of the section where the caret is (-1 before the first).
    #[must_use]
    pub fn current_heading(&self) -> i64 {
        let para: f64 = match &self.ctx {
            EditContext::Note { saved, .. } => f64::from(saved.focus.para),
            EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } => -1.0,
            EditContext::Cell { anchor, .. } => self.doc.body.tables.get(*anchor as usize).map_or(0.0, |a| f64::from(a.before_para)) - 0.5,
            EditContext::Body => f64::from(self.selection.focus.para),
        };
        let mut cur = -1;
        for h in self.headings() {
            if f64::from(h.para) <= para {
                cur = i64::from(h.index);
            } else {
                break;
            }
        }
        cur
    }

    /// `bodyLevels(heads)`: the outline level of every body paragraph (0 = body text).
    fn body_levels(&self, heads: &[HeadingInfo]) -> Vec<f64> {
        let mut lv = vec![0.0; self.doc.body.paras.len()];
        for h in heads {
            if let Some(x) = lv.get_mut(h.para as usize) {
                *x = h.level;
            }
        }
        lv
    }

    /// `goToBodyPara(para, start, end)` / `goToBodyParaAfterEdit(para)`: the selection in a body
    /// paragraph (the scroll is the shell's).
    fn go_to_body_para(&mut self, para: u32, start: u32, end: u32) {
        self.selection = Selection { anchor: DocPosition::new(para, start), focus: DocPosition::new(para, end) };
        self.pending_props = None;
    }

    fn done_bool(&mut self, v: bool) -> Result<Outcome, CommandError> {
        self.f9.result = Some(F9Result::Bool(v));
        Ok(Outcome::Done(self.change_set()))
    }

    fn finish_bool(&mut self, v: bool) -> Result<Outcome, CommandError> {
        self.f9.result = Some(F9Result::Bool(v));
        Ok(Outcome::Done(self.finish()))
    }

    /// `moveHeading(index, before)`: the heading's section moves before heading `before`
    /// (`getHeadings().length` = the end). One undo step.
    fn move_heading(&mut self, index: i64, before: i64) -> Result<Outcome, CommandError> {
        if !self.back_to_body() || self.locked() {
            return self.done_bool(false);
        }
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)) else { return self.done_bool(false) };
        let n = self.doc.body.paras.len();
        let (s, e) = heading_span(&self.body_levels(&heads), h.para as usize);
        let t = match idx(before) {
            Some(b) if b >= heads.len() => n,
            Some(b) => heads.get(b).map_or(n, |x| x.para as usize),
            None => n,
        };
        if t >= s && t <= e {
            return self.done_bool(false);
        }
        self.begin_step(false, &[]);
        let map = move_map(n, s, e, t);
        let mut next: Vec<Option<Arc<Paragraph>>> = vec![None; n];
        for (i, a) in self.doc.body.paras.iter_arcs().enumerate() {
            if let Some(slot) = map.get(i).and_then(|&m| next.get_mut(m)) {
                *slot = Some(Arc::clone(a));
            }
        }
        self.doc.body.paras = PVec::from_arcs(next.into_iter().flatten());
        self.remap_body_paras(&map, n);
        self.touch_structure();
        let to = map.get(s).copied().unwrap_or(0);
        self.go_to_body_para(u32_of(to), 0, 0);
        self.finish_bool(true)
    }

    /// `remapBodyParas(map, n)`: table anchors and bookmarks follow the new order (a table stays
    /// right after the paragraph that preceded it). The floating images are the layout's.
    fn remap_body_paras(&mut self, map: &[usize], n: usize) {
        let tables = &mut self.doc.body.tables;
        for i in 0..tables.len() {
            let Some(bp) = tables.get(i).map(|a| a.before_para) else { continue };
            let nb = if bp == 0 {
                0
            } else if bp as usize > n {
                bp
            } else {
                map.get(bp as usize - 1).map_or(bp, |&m| u32_of(m + 1))
            };
            if nb != bp
                && let Some(a) = tables.make_mut(i)
            {
                a.before_para = nb;
            }
        }
        // `tableAnchors.sort((a, b) => a.beforePara - b.beforePara)` (stable)
        let mut arcs: Vec<_> = tables.iter_arcs().cloned().collect();
        if arcs.windows(2).any(|w| w.first().map(|a| a.before_para) > w.get(1).map(|b| b.before_para)) {
            arcs.sort_by_key(|a| a.before_para);
            *tables = PVec::from_arcs(arcs);
        }
        let marks = self.bookmark_paras.map_mut();
        for (_, v) in marks.iter_mut() {
            if let Some(&m) = map.get(*v as usize) {
                *v = u32_of(m);
            }
        }
    }

    /// `listStyles('all').find((st) => st.rawName.toLowerCase() === 'heading N')?.id`.
    fn heading_style_id(&mut self, level: f64) -> Option<String> {
        let want = format!("heading {}", uo_jsnum::format::to_js_string(level));
        let idx = self.style_index();
        super::styles::xform::list_styles_all(&idx, self.f2.lang)
            .into_iter()
            .find(|s| uo_text::case::to_lowercase_str(&s.raw_name) == want)
            .map(|s| s.id)
    }

    /// `shiftHeading(index, delta)`: promote (-1) or demote (+1) a heading and its subheadings
    /// (styles Heading 1–9).
    fn shift_heading(&mut self, index: i64, delta: f64) -> Result<Outcome, CommandError> {
        if !self.back_to_body() || self.locked() {
            return self.done_bool(false);
        }
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)).cloned() else { return self.done_bool(false) };
        if (delta < 0.0 && h.level <= 1.0) || (delta > 0.0 && h.level >= 9.0) {
            return self.done_bool(false);
        }
        let lv = self.body_levels(&heads);
        let (s, e) = heading_span(&lv, h.para as usize);
        let keep = self.selection;
        self.begin_step(false, &[]);
        for pi in s..e {
            let l = lv.get(pi).copied().unwrap_or(0.0);
            if !uo_jsnum::conv::truthy(l) {
                continue;
            }
            let to = uo_jsnum::math::max(1.0, uo_jsnum::math::min(9.0, l + delta));
            let Some(id) = self.heading_style_id(to) else { continue };
            self.selection = Selection::caret(DocPosition::new(u32_of(pi), 0));
            self.apply_style_core(Some(&id))?;
        }
        self.selection = keep;
        self.pending_props = None;
        self.finish_bool(true)
    }

    /// `insertHeading(index, where)`: an empty paragraph with the style Heading N, the caret in it.
    fn insert_heading(&mut self, index: i64, at: HeadingWhere) -> Result<Outcome, CommandError> {
        if !self.back_to_body() || self.locked() {
            return self.done_bool(false);
        }
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)).cloned() else { return self.done_bool(false) };
        let (_, e) = heading_span(&self.body_levels(&heads), h.para as usize);
        let pos = if at == HeadingWhere::Before { h.para as usize } else { e };
        let level = uo_jsnum::math::min(9.0, h.level + if at == HeadingWhere::Sub { 1.0 } else { 0.0 });
        let id = self.heading_style_id(level);
        self.begin_step(false, &[]);
        let mut props = ParaProps::default();
        if let Some(id) = &id {
            obj::set(&mut props, "styleId", |p| p.style_id = Some(id.as_str().into()));
        }
        if self.opts.tracking {
            let rev = self.new_rev();
            obj::set(&mut props, "markIns", |p| p.mark_ins = Some(Box::new(rev)));
        }
        // `if (id && this.stylesEditable())` (17810; wave 7, A1)
        if let Some(id) = id.as_ref().filter(|_| self.styles_editable()) {
            let base = self.styles_base();
            self.commit_styles(super::styles::xform::ensure_defined(&base, id, true));
        }
        let p = Paragraph { id: self.doc.ids.para(), props: Arc::new(props), ..Paragraph::default() };
        let pos = pos.min(self.doc.body.paras.len());
        self.doc.body.paras.insert(pos, Arc::new(p));
        self.shift_anchors(u32_of(pos), 1);
        self.touch_structure();
        self.go_to_body_para(u32_of(pos), 0, 0);
        self.finish_bool(true)
    }

    /// `deleteHeading(index)`: the heading and all its content (tables included); with tracking
    /// on, a tracked deletion of the section's text.
    fn delete_heading(&mut self, index: i64) -> Result<Outcome, CommandError> {
        if !self.back_to_body() || self.locked() {
            return self.done_bool(false);
        }
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)).cloned() else { return self.done_bool(false) };
        let n = self.doc.body.paras.len();
        let (s, e) = heading_span(&self.body_levels(&heads), h.para as usize);
        let e = e.min(n);
        self.begin_step(false, &[]);
        if self.opts.tracking {
            let last = self.doc.body.paras.get(e.saturating_sub(1)).map_or(0, para_length);
            self.selection =
                Selection { anchor: DocPosition::new(u32_of(s), 0), focus: DocPosition::new(u32_of(e.saturating_sub(1)), u32_of(last)) };
            self.delete_selection()?;
            return self.finish_bool(true);
        }
        let (s32, e32) = (u32_of(s), u32_of(e));
        let len = e32 - s32;
        let n32 = u32_of(n);
        {
            let tables = &mut self.doc.body.tables;
            let kept: Vec<_> = tables
                .iter_arcs()
                .filter(|a| !(a.before_para > s32 && a.before_para <= e32 && (e32 < n32 || a.before_para < e32)))
                .cloned()
                .collect();
            if kept.len() != tables.len() {
                *tables = PVec::from_arcs(kept);
            }
        }
        for _ in s..e {
            self.doc.body.paras.remove(s);
        }
        if self.doc.body.paras.is_empty() {
            let p = Paragraph::empty(self.doc.ids.para());
            self.doc.body.paras.push(p);
        }
        let tables = &mut self.doc.body.tables;
        for i in 0..tables.len() {
            if tables.get(i).is_some_and(|a| a.before_para > s32)
                && let Some(a) = tables.make_mut(i)
            {
                a.before_para = s32.max(a.before_para.saturating_sub(len));
            }
        }
        let marks = self.bookmark_paras.map_mut();
        for (_, v) in marks.iter_mut() {
            if *v >= e32 {
                *v -= len;
            } else if *v >= s32 {
                *v = s32;
            }
        }
        self.touch_structure();
        let to = s.min(self.doc.body.paras.len().saturating_sub(1));
        self.go_to_body_para(u32_of(to), 0, 0);
        self.finish_bool(true)
    }

    /// `selectHeading(index)`: the heading and its content.
    fn select_heading(&mut self, index: i64) -> Result<Outcome, CommandError> {
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)).cloned() else { return self.done_bool(false) };
        self.back_to_body();
        let (s, e) = heading_span(&self.body_levels(&heads), h.para as usize);
        let last = e.saturating_sub(1);
        let len = self.doc.body.paras.get(last).map_or(0, para_length);
        self.selection = Selection { anchor: DocPosition::new(u32_of(s), 0), focus: DocPosition::new(u32_of(last), u32_of(len)) };
        self.done_bool(true)
    }

    /// `goToHeading(index)`: the caret at the start of the heading.
    fn go_to_heading(&mut self, index: i64) -> Result<Outcome, CommandError> {
        let heads = self.headings();
        let Some(h) = idx(index).and_then(|i| heads.get(i)).cloned() else { return self.done_bool(false) };
        self.back_to_body();
        self.go_to_body_para(h.para, 0, 0);
        self.done_bool(true)
    }

    /// `selectAll()`.
    fn select_all(&mut self) -> Result<Outcome, CommandError> {
        let Some(paras) = self.paras() else { return self.done_bool(false) };
        let last = paras.len().saturating_sub(1);
        let len = paras.get(last).map_or(0, para_length);
        self.selection = Selection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(u32_of(last), u32_of(len)) };
        self.done_bool(true)
    }

    /// `setSelectionRanges(ranges)`: only the ranges inside the edited story count.
    fn set_selection_ranges(&mut self, ranges: Vec<DocRange>) -> Result<Outcome, CommandError> {
        let Some(paras) = self.paras() else { return self.done_bool(false) };
        let ok = |p: DocPosition| paras.get(p.para as usize).is_some_and(|x| (p.offset as usize) <= para_length(x));
        let mut rs: Vec<DocRange> = ranges.into_iter().filter(|r| ok(r.start) && ok(r.end)).collect();
        let Some(main) = rs.pop() else { return self.done_bool(false) };
        self.float_sel = None;
        self.selection = Selection { anchor: main.start, focus: main.end };
        self.set_extra_selection(rs);
        self.done_bool(true)
    }

    /// `overlapsDeleted(p, s, e)`.
    fn overlaps_deleted(p: &Paragraph, s: usize, e: usize) -> bool {
        let mut at = 0;
        for r in &p.runs {
            let (a, z) = (at, at + r.text.len());
            at = z;
            if r.props.del.is_some() && a < e && z > s {
                return true;
            }
        }
        false
    }

    /// `search(text, opts)`: the Navigation pane's search over the body and its tables.
    fn search(&mut self, text: &[u16], opts: FindOptions) -> Vec<SearchResult> {
        self.f9.clear_search();
        self.f9.highlight.clear();
        if text.is_empty() {
            return Vec::new();
        }
        let heads = self.headings();
        let mut list: Vec<SearchResult> = Vec::new();
        let mut add = |p: &Paragraph, loc: SearchLoc, body_para: f64| {
            let full = para_text(p);
            for s in find_in_text(&full, text, opts) {
                let e = s + text.len();
                if Session::overlaps_deleted(p, s, e) {
                    continue;
                }
                let snippet = match_snippet(&full, s, e);
                let mut heading = -1;
                for h in &heads {
                    if f64::from(h.para) <= body_para {
                        heading = i64::from(h.index);
                    } else {
                        break;
                    }
                }
                let in_table = matches!(loc, SearchLoc::Cell { .. });
                list.push(SearchResult {
                    index: u32_of(list.len()),
                    start: u32_of(s),
                    end: u32_of(e),
                    snippet,
                    heading,
                    in_table,
                    loc: loc.clone(),
                });
            }
        };
        let body = &self.doc.body;
        let n = body.paras.len();
        for i in 0..=n {
            for (ai, a) in body.tables.iter().enumerate() {
                if a.before_para as usize == i {
                    let mut cells = Vec::new();
                    table_cells(&a.table, &[], &mut cells);
                    for (hops, story) in cells {
                        for (k, p) in story.paras.iter().enumerate() {
                            add(p, SearchLoc::Cell { anchor: u32_of(ai), hops: hops.clone(), para: u32_of(k) }, i as f64 - 0.5);
                        }
                    }
                }
            }
            if let Some(p) = body.paras.get(i) {
                add(p, SearchLoc::Body { para: u32_of(i) }, i as f64);
            }
        }
        list.shrink_to_fit();
        self.f9.search = list.clone();
        list
    }

    /// The results of the last `search` (`searchList`).
    #[must_use]
    pub fn search_results(&self) -> &[SearchResult] {
        &self.f9.search
    }

    /// `goToSearchResult(index)`: selects a result of the last search (entering its cell).
    fn go_to_search_result(&mut self, index: i64) -> Result<Outcome, CommandError> {
        let Some(m) = idx(index).and_then(|i| self.f9.search.get(i)).cloned() else { return self.done_bool(false) };
        self.f9.search_current = index;
        match m.loc {
            SearchLoc::Body { para } => {
                self.back_to_body();
                // a stale result (the document changed since the search) is taken as it is, as the
                // TS does: an op on a position outside the story answers `Position` (D2a-T1)
                self.go_to_body_para(para, m.start, m.end);
                self.done_bool(true)
            }
            SearchLoc::Cell { anchor, hops, para } => {
                if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. }) {
                    self.back_to_body();
                }
                if !self.enter_cell(anchor, hops, Some(DocPosition::new(para, m.start))) {
                    return self.done_bool(false);
                }
                self.selection = Selection { anchor: DocPosition::new(para, m.start), focus: DocPosition::new(para, m.end) };
                self.done_bool(true)
            }
        }
    }

    pub(crate) fn exec_navigation(&mut self, cmd: NavigationCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            NavigationCmd::SelectAll => self.select_all(),
            NavigationCmd::SetSelectionRanges { ranges } => self.set_selection_ranges(ranges),
            NavigationCmd::MoveHeading { index, before } => self.move_heading(index, before),
            NavigationCmd::PromoteHeading { index } => self.shift_heading(index, -1.0),
            NavigationCmd::DemoteHeading { index } => self.shift_heading(index, 1.0),
            NavigationCmd::InsertHeading { index, at } => self.insert_heading(index, at),
            NavigationCmd::DeleteHeading { index } => self.delete_heading(index),
            NavigationCmd::SelectHeading { index } => self.select_heading(index),
            NavigationCmd::GoToHeading { index } => self.go_to_heading(index),
            NavigationCmd::Search { text, opts } => {
                let n = self.search(&text, opts).len();
                self.f9.result = Some(F9Result::Count(u32_of(n)));
                Ok(Outcome::Done(self.change_set()))
            }
            NavigationCmd::ClearSearch => {
                self.search(&[], FindOptions::default());
                self.f9.result = None;
                Ok(Outcome::Done(self.change_set()))
            }
            NavigationCmd::GoToSearchResult { index } => self.go_to_search_result(index),
        }
    }
}

impl NavigationCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads:
    /// none of this family's (the styles `promoteHeading`/`demoteHeading` apply go through
    /// `applyStyleCore`, which the TS does not guard either).
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}

/// `paraLength` of every paragraph of a story (the replacements' clamp).
#[allow(dead_code)]
fn lengths(paras: &PVec<Paragraph>) -> Vec<usize> {
    paras.iter().map(para_length).collect()
}

// ───────────────────────── navigation.ts (pure) ─────────────────────────
// `tabInfo`, `tabsDirect`, `dedupeTabs`, `indentsDirect` are F1's (`ops::paragraph`).

/// `headingSpan(levels, i)`: the paragraphs of a heading's section `[i, e)` — the heading and
/// everything up to the next heading of the same or a higher level (`levels[i]`: the outline level
/// of paragraph `i`, 0 = body text).
#[must_use]
pub fn heading_span(levels: &[f64], i: usize) -> (usize, usize) {
    let lv = levels.get(i).copied().unwrap_or(0.0);
    if !uo_jsnum::conv::truthy(lv) {
        return (i, i + 1);
    }
    let mut e = i + 1;
    while let Some(&l) = levels.get(e) {
        if l > 0.0 && l <= lv {
            break;
        }
        e += 1;
    }
    (i, e)
}

/// `moveMap(n, s, e, t)`: the new order when `[s, e)` moves before `t` (`t` outside `[s, e]`):
/// `map[old] = new`.
#[must_use]
pub fn move_map(n: usize, s: usize, e: usize, t: usize) -> Vec<usize> {
    let len = e.saturating_sub(s);
    (0..n)
        .map(|i| {
            if i >= s && i < e {
                (if t > e { t - len } else { t }) + (i - s)
            } else if t > e && i >= e && i < t {
                i - len
            } else if t < s && i >= t && i < s {
                i + len
            } else {
                i
            }
        })
        .collect()
}

/// `findInText`'s options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

/// `WORD_CHAR.test(text[k])`: `/[\p{L}\p{N}_]/u` over ONE unit (`undefined` reads as the string
/// "undefined", which matches).
fn word_unit(text: &[u16], k: Option<usize>) -> bool {
    match k.and_then(|k| text.get(k)) {
        Some(&u) => u == u16::from(b'_') || uo_text::props::mask(u32::from(u)) & (uo_text::props::LETTER | uo_text::props::NUMBER) != 0,
        None => true,
    }
}

fn index_of(hay: &[u16], nee: &[u16], from: usize) -> Option<usize> {
    if nee.is_empty() {
        return Some(from.min(hay.len()));
    }
    let last = hay.len().checked_sub(nee.len())?;
    (from..=last).find(|&i| hay.get(i..i + nee.len()) == Some(nee))
}

/// `findInText(text, needle, opts)`: the start of each occurrence, without overlap. The indices
/// are those of the lowercased text (`toLocaleLowerCase('pt-BR')`), as the TS returns them.
#[must_use]
pub fn find_in_text(text: &[u16], needle: &[u16], opts: FindOptions) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let (hay, nee) = if opts.match_case {
        (std::borrow::Cow::Borrowed(text), std::borrow::Cow::Borrowed(needle))
    } else {
        (uo_text::case::to_lowercase(text), uo_text::case::to_lowercase(needle))
    };
    let mut out = Vec::new();
    let mut at = index_of(&hay, &nee, 0);
    while let Some(i) = at {
        let ok = !opts.whole_word
            || ((i == 0 || !word_unit(text, Some(i - 1))) && (i + needle.len() >= text.len() || !word_unit(text, Some(i + needle.len()))));
        if ok {
            out.push(i);
            at = index_of(&hay, &nee, i + nee.len().max(1));
        } else {
            at = index_of(&hay, &nee, i + 1);
        }
    }
    out
}

/// A snippet of `matchSnippet`: before (cut at a word start), the match, the rest of the line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snippet {
    pub before: Vec<u16>,
    pub r#match: Vec<u16>,
    pub after: Vec<u16>,
}

/// `text.slice(a, b)`.
fn slice(text: &[u16], a: usize, b: usize) -> &[u16] {
    let a = a.min(text.len());
    let b = b.min(text.len());
    if b <= a { &[] } else { text.get(a..b).unwrap_or(&[]) }
}

/// `clean(s)`: `[\t\n\r\u000b\u000c￼]` → space.
fn clean(s: &[u16]) -> Vec<u16> {
    s.iter().map(|&u| if matches!(u, 0x09 | 0x0A | 0x0D | 0x0B | 0x0C | 0xFFFC) { 0x20 } else { u }).collect()
}

/// `matchSnippet(text, start, end)`.
#[must_use]
pub fn match_snippet(text: &[u16], start: usize, end: usize) -> Snippet {
    let len = text.len();
    let space = 0x20u16;
    let mut b = start.saturating_sub(40);
    if b > 0 {
        let sp = text.iter().skip(b).position(|&u| u == space).map(|k| k + b);
        b = match sp {
            Some(sp) if sp < start => sp + 1,
            _ => b,
        };
    }
    let mut a = (end + 60).min(len);
    if a < len {
        let sp = text.get(..=a).and_then(|t| t.iter().rposition(|&u| u == space));
        a = match sp {
            Some(sp) if sp > end => sp,
            _ => a,
        };
    }
    let mut before: Vec<u16> = if b > 0 { vec![0x2026] } else { Vec::new() };
    before.extend(clean(slice(text, b, start)));
    let mut after = clean(slice(text, end, a));
    if a < len {
        after.push(0x2026);
    }
    Snippet { before, r#match: clean(slice(text, start, end)), after }
}

/// `DrawingGrid` (Exibir › Linhas de Grade), from `settings.xml` (twips).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DrawingGrid {
    pub h_spacing: f64,
    pub v_spacing: f64,
    pub every_h: f64,
    pub every_v: f64,
    pub from_margins: bool,
    pub origin_x: f64,
    pub origin_y: f64,
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `new RegExp('<w:' + name + '\\b[^>]*\\bw:val="(-?\\d+)"').exec(x)`: the value (`+m[1]`) of the
/// first tag `<w:name` whose attributes (up to the first `>`) hold `w:val="<integer>"` after a word
/// boundary; the rightmost such attribute (the `[^>]*` is greedy).
fn grid_val(x: &str, name: &str) -> Option<f64> {
    let open = format!("<w:{name}");
    let b = x.as_bytes();
    let mut from = 0;
    while let Some(k) = x.get(from..).and_then(|s| s.find(&open)).map(|k| k + from) {
        from = k + 1;
        let after = k + open.len();
        if b.get(after).copied().is_some_and(is_word_byte) {
            continue;
        }
        let tag_end = b.iter().skip(after).position(|&c| c == b'>').map_or(b.len(), |p| p + after);
        let mut best: Option<f64> = None;
        let mut p = after;
        while let Some(q) = x.get(p..tag_end).and_then(|s| s.find("w:val=\"")).map(|q| q + p) {
            p = q + 1;
            if q > 0 && b.get(q - 1).copied().is_some_and(is_word_byte) {
                continue;
            }
            let v = q + 7;
            let neg = b.get(v) == Some(&b'-');
            let ds = v + usize::from(neg);
            let nd = b.iter().skip(ds).take_while(|c| c.is_ascii_digit()).count();
            if nd == 0 || b.get(ds + nd) != Some(&b'"') || ds + nd >= tag_end {
                continue;
            }
            best = x.get(v..ds + nd).map(uo_jsnum::parse::to_number);
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

/// `/<w:doNotUseMarginsForDrawingGridOrigin\b(?![^>]*w:val="(?:0|false|off)")/.test(x)`.
fn grid_flag(x: &str) -> bool {
    let open = "<w:doNotUseMarginsForDrawingGridOrigin";
    let b = x.as_bytes();
    let mut from = 0;
    while let Some(k) = x.get(from..).and_then(|s| s.find(open)).map(|k| k + from) {
        from = k + 1;
        let after = k + open.len();
        if b.get(after).copied().is_some_and(is_word_byte) {
            continue;
        }
        let tag_end = b.iter().skip(after).position(|&c| c == b'>').map_or(b.len(), |p| p + after);
        let tag = x.get(after..tag_end).unwrap_or("");
        let off = ["w:val=\"0\"", "w:val=\"false\"", "w:val=\"off\""].iter().any(|s| tag.contains(s));
        if !off {
            return true;
        }
    }
    false
}

/// `drawingGridOf(settingsXml)`.
#[must_use]
pub fn drawing_grid_of(settings_xml: Option<&str>) -> DrawingGrid {
    let x = settings_xml.unwrap_or("");
    let max = uo_jsnum::math::max;
    DrawingGrid {
        h_spacing: max(1.0, grid_val(x, "drawingGridHorizontalSpacing").unwrap_or(180.0)),
        v_spacing: max(1.0, grid_val(x, "drawingGridVerticalSpacing").unwrap_or(180.0)),
        every_h: max(0.0, grid_val(x, "displayHorizontalDrawingGridEvery").unwrap_or(1.0)),
        every_v: max(0.0, grid_val(x, "displayVerticalDrawingGridEvery").unwrap_or(1.0)),
        from_margins: !grid_flag(x),
        origin_x: grid_val(x, "drawingGridHorizontalOrigin").unwrap_or(0.0),
        origin_y: grid_val(x, "drawingGridVerticalOrigin").unwrap_or(0.0),
    }
}

/// A rectangle of the page (twips).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GridBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// `gridLines(g, box, origin)`: the x of the vertical lines and the y of the horizontal ones.
#[must_use]
pub fn grid_lines(g: &DrawingGrid, bx: GridBox, origin: (f64, f64)) -> (Vec<f64>, Vec<f64>) {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let step_x = g.h_spacing * g.every_v;
    let step_y = g.v_spacing * g.every_h;
    let lines = |out: &mut Vec<f64>, step: f64, lo: f64, hi: f64, o: f64| {
        if step > 0.0 {
            let k0 = uo_jsnum::math::ceil((lo - o) / step - 1e-9);
            let mut k = k0;
            let mut v = o + k * step;
            while v <= hi + 1e-6 && out.len() < 2000 {
                out.push(v);
                k += 1.0;
                v = o + k * step;
            }
        }
    };
    lines(&mut xs, step_x, bx.x0, bx.x1, origin.0);
    lines(&mut ys, step_y, bx.y0, bx.y1, origin.1);
    (xs, ys)
}

/// `rulerSnap(unit)`: the ruler's step (twips): 0.25 cm or 1/16".
#[must_use]
pub fn ruler_snap(cm: bool) -> f64 {
    if cm { 1440.0 / 2.54 / 4.0 } else { 1440.0 / 16.0 }
}

/// `snapTo(v, step)`.
#[must_use]
pub fn snap_to(v: f64, step: f64) -> f64 {
    uo_jsnum::math::round(uo_jsnum::math::round(v / step) * step)
}

/// The hand matchers of the regex sites of `navigation.ts` (`js/regex-sites/d2b-f9.mjs`).
pub(crate) fn site(id: &str, s: &[u16]) -> Option<String> {
    let hx = |t: &[u16]| t.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".");
    let text = String::from_utf16_lossy(s);
    Some(match id {
        "d2b-f9.word_char" => word_unit(s, Some(0)).to_string(),
        "d2b-f9.clean" => hx(&clean(s)),
        "d2b-f9.grid_val" => grid_val(&text, "drawingGridHorizontalSpacing").map_or("none".to_owned(), uo_jsnum::format::to_js_string),
        "d2b-f9.grid_flag" => grid_flag(&text).to_string(),
        _ => return None,
    })
}
