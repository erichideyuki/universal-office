//! Edit contexts (D2a): `enterCell`/`exitCell`, `enterNote`/`exitNote`,
//! `enterHeaderFooter`/`exitHeaderFooter`, `enterTextbox`/`exitTextbox` (U16: private entries of
//! the TS, reached by the shell through [`ContextCmd`]), the story each context edits and how a
//! history step carries and restores it (`ce`, `nt`, `hp`, `te` of `stringifyState`).
//!
//! The TS swaps `this.model` to the context's paragraphs and keeps the body in `saved`; here the
//! context is a path ([`EditContext`]) and the story is reached through it, copy-on-write down to
//! it ([`story_of_mut`]). The TS edits a cell, note or header through its paragraph view and
//! re-aligns the block view by paragraph identity (`reconcileBlocks`); here every story is stored
//! once, so there is nothing to reconcile (A2, U3). The undo stack is the document's: a step
//! carries the body and the context ([`crate::session::ContextStep`]).
//!
//! What the layout decides (which header/footer part a page shows, which box of a chain gets the
//! caret) arrives as arguments from the shell: D2a takes no layout decision here (declared
//! D2a-C2, C3 in the record).

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::state::{CellHop, ContextStep, Selection};
use super::super::text::{DocPosition, para_length};
use super::super::{ExecCx, Session};
use crate::ids::HfPartId;
use crate::js::NumKey;
use crate::model::Document;
use crate::model::para::Paragraph;
use crate::model::story::{Block, Story};
use crate::model::table::Table;
use crate::word::refs::cell_at_grid;

/// Where the edits go.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum EditContext {
    #[default]
    Body,
    /// a cell of a body table (`cellEdit`): the anchor of the top table and the hops down to it;
    /// `saved` is the body selection to return to
    Cell { anchor: u32, hops: Arc<[CellHop]>, saved: Selection },
    /// a footnote/endnote (`noteEdit`)
    Note { footnote: bool, id: f64, saved: Selection },
    /// a header/footer part (`hfEdit`); `page` is the page it was entered on (`hfActivePage`)
    HeaderFooter { part: HfPartId, page: u32, saved: Selection },
    /// a textbox (`tbEdit`): the run that holds it in the outer context's story; `host`/`focus`
    /// are the box's order among that story's textbox runs and the focused box of a chain
    /// (`te.h`/`te.f`), `page` the page it was entered on; `saved` the outer selection
    Textbox { outer: Box<EditContext>, para: u32, run: u32, host: u32, focus: u32, page: u32, saved: Selection },
}

/// The context commands (D2a).
#[derive(Debug, Clone, PartialEq)]
pub enum ContextCmd {
    /// `enterCell(anchor, hops, caret?)`
    EnterCell { anchor: u32, hops: Vec<CellHop>, caret: Option<DocPosition> },
    /// `exitCell()`
    ExitCell,
    /// `enterNote(kind, id, caret?)`: `caret` `None` = the start; `at_end` = `'end'`
    EnterNote { footnote: bool, id: f64, caret: Option<DocPosition>, at_end: bool },
    /// `exitNote()`
    ExitNote,
    /// `enterHeaderFooter(kind, page)`: the part the layout shows on that page (G21b: the shell
    /// asks the layout)
    EnterHeaderFooter { part: HfPartId, page: u32 },
    /// `exitHeaderFooter()`
    ExitHeaderFooter,
    /// `enterTextbox(run, …)`: the textbox run at `para`/`run` of the current story; `host`/`focus`
    /// its order among the textbox runs (`contextTextboxRuns`), `page` the page
    EnterTextbox { para: u32, run: u32, host: u32, focus: u32, page: u32 },
    /// `exitTextbox()`
    ExitTextbox,
}

/// The block index `bi` of a story as a table anchor index.
pub(crate) fn table_index_of_block(s: &Story, bi: usize) -> Option<usize> {
    let mut t = 0usize;
    for (i, b) in s.blocks().enumerate() {
        if let Block::Table(_) = b {
            if i == bi {
                return Some(t);
            }
            t += 1;
        } else if i == bi {
            return None;
        }
    }
    None
}

/// The index in its row of the cell `cellAtGrid` finds.
pub(crate) fn cell_index(t: &Table, h: &CellHop) -> Option<(usize, usize)> {
    let cell = cell_at_grid(t, f64::from(h.row), f64::from(h.col))?;
    let r = h.row as usize;
    let k = t.rows.get(r)?.cells.iter().position(|c| std::ptr::eq(c, cell))?;
    Some((r, k))
}

/// `resolveCell(t, hops)`: the cell's story.
pub(crate) fn resolve_cell<'a>(t: &'a Table, hops: &[CellHop]) -> Option<&'a Story> {
    let mut table = t;
    for (i, h) in hops.iter().enumerate() {
        let (r, k) = cell_index(table, h)?;
        let cell = table.rows.get(r)?.cells.get(k)?;
        if i + 1 == hops.len() {
            return Some(&cell.content);
        }
        let bi = h.block? as usize;
        match cell.content.block_at(bi)? {
            Block::Table(t) => table = t,
            Block::Paragraph(_) => return None,
        }
    }
    None
}

fn resolve_cell_mut<'a>(t: &'a mut Table, hops: &[CellHop]) -> Option<&'a mut Story> {
    let (h, rest) = hops.split_first()?;
    let (r, k) = cell_index(t, h)?;
    let cell = t.rows.get_mut(r)?.cells.get_mut(k)?;
    if rest.is_empty() {
        return Some(&mut cell.content);
    }
    let ti = table_index_of_block(&cell.content, h.block? as usize)?;
    let a = cell.content.tables.make_mut(ti)?;
    resolve_cell_mut(Arc::make_mut(&mut a.table), rest)
}

fn textbox_story(outer: &Story, para: u32, run: u32) -> Option<&Story> {
    let r = outer.paras.get(para as usize)?.runs.get(run as usize)?;
    r.ext().and_then(|e| e.textbox.as_deref()).map(|t| &t.content)
}

/// The story a context edits.
#[must_use]
pub fn story_of<'a>(doc: &'a Document, ctx: &EditContext) -> Option<&'a Story> {
    match ctx {
        EditContext::Body => Some(&doc.body),
        EditContext::Cell { anchor, hops, .. } => {
            let a = doc.body.tables.get(*anchor as usize)?;
            resolve_cell(&a.table, hops)
        }
        EditContext::Note { footnote, id, .. } => {
            let map = if *footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
            map.get(&NumKey::new(*id))
        }
        EditContext::HeaderFooter { part, .. } => doc.hf.get(part.index()).map(|h| &h.story),
        EditContext::Textbox { outer, para, run, .. } => textbox_story(story_of(doc, outer)?, *para, *run),
    }
}

/// The story a context edits, for a change (copy-on-write down to it).
pub fn story_of_mut<'a>(doc: &'a mut Document, ctx: &EditContext) -> Option<&'a mut Story> {
    match ctx {
        EditContext::Body => Some(&mut doc.body),
        EditContext::Cell { anchor, hops, .. } => {
            let a = doc.body.tables.make_mut(*anchor as usize)?;
            resolve_cell_mut(Arc::make_mut(&mut a.table), hops)
        }
        EditContext::Note { footnote, id, .. } => {
            let notes = Arc::make_mut(&mut doc.notes);
            let map = if *footnote { &mut notes.footnotes } else { &mut notes.endnotes };
            map.get_mut(&NumKey::new(*id))
        }
        EditContext::HeaderFooter { part, .. } => doc.hf.make_mut(part.index()).map(|h| &mut h.story),
        EditContext::Textbox { outer, para, run, .. } => {
            let story = story_of_mut(doc, outer)?;
            let p = story.paras.make_mut(*para as usize)?;
            let r = p.runs.get_mut(*run as usize)?;
            let tb = r.ext.as_deref_mut().and_then(|e| e.textbox.as_deref_mut())?;
            Some(&mut tb.content)
        }
    }
}

/// `setNoteBlocks(full, kind, id, blocks)`.
pub fn set_note_story(doc: &mut Document, footnote: bool, id: f64, story: Story) {
    let notes = Arc::make_mut(&mut doc.notes);
    let map = if footnote { &mut notes.footnotes } else { &mut notes.endnotes };
    map.insert(NumKey::new(id), story);
}

/// The body selection kept while a context is edited.
#[must_use]
pub fn saved_selection(ctx: &EditContext) -> Option<Selection> {
    match ctx {
        EditContext::Body => None,
        EditContext::Cell { saved, .. } | EditContext::Note { saved, .. } | EditContext::HeaderFooter { saved, .. } => Some(*saved),
        EditContext::Textbox { outer, saved, .. } => saved_selection(outer).or(Some(*saved)),
    }
}

/// The context part of `stringifyState` (`ce`, `nt`, `hp`, `te`), with the selection of now.
#[must_use]
pub fn context_step(doc: &Document, ctx: &EditContext, selection: Selection) -> ContextStep {
    match ctx {
        EditContext::Body => ContextStep::Body,
        EditContext::Cell { anchor, hops, .. } => ContextStep::Cell { anchor: *anchor, hops: Arc::clone(hops), selection },
        EditContext::Note { footnote, id, .. } => {
            ContextStep::Note { footnote: *footnote, id: *id, story: story_of(doc, ctx).cloned().unwrap_or_default(), selection }
        }
        EditContext::HeaderFooter { part, page, .. } => {
            ContextStep::HeaderFooter { part: *part, story: story_of(doc, ctx).cloned().unwrap_or_default(), selection, page: *page }
        }
        EditContext::Textbox { outer, host, focus, page, .. } => {
            let outer = match &**outer {
                EditContext::HeaderFooter { part, .. } => Some(*part),
                _ => None,
            };
            ContextStep::Textbox { outer, host: *host, focus: *focus, page: *page, selection }
        }
    }
}

/// Before `popHistory` takes the state of now: a textbox being edited is left (the state is
/// taken from outside it, `exitTextbox(true)`).
pub fn leave_for_history(s: &mut Session, _cx: &mut ExecCx<'_>) {
    if let EditContext::Textbox { outer, saved, .. } = s.ctx.clone() {
        s.ctx = *outer;
        s.selection = saved;
        s.tx.events.push(Event::TbMode { active: false });
    }
}

/// `clampSel(s)`: a selection inside the paragraphs of the current story.
#[must_use]
pub fn clamp_selection(paras: Option<&crate::pvec::PVec<Paragraph>>, s: Selection) -> Selection {
    let c = |p: DocPosition| -> DocPosition {
        let Some(ps) = paras else { return DocPosition::default() };
        let last = u32::try_from(ps.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let para = p.para.min(last);
        let len = ps.get(para as usize).map_or(0, |x| u32::try_from(para_length(x)).unwrap_or(u32::MAX));
        DocPosition { para, offset: p.offset.min(len) }
    };
    Selection { anchor: c(s.anchor), focus: c(s.focus) }
}

/// The context half of `popHistory`'s restore (`ce`, `nt`, `hp`, `te`): the session is in the
/// body with the step's body and selection; `prev` is the context before the pop.
pub fn restore_context(s: &mut Session, step: ContextStep, prev: &EditContext) {
    let body_sel = s.selection;
    match step {
        ContextStep::Body => {}
        ContextStep::Cell { anchor, hops, selection } => {
            let ok = s.doc.body.tables.get(anchor as usize).is_some_and(|a| resolve_cell(&a.table, &hops).is_some());
            if ok {
                s.ctx = EditContext::Cell { anchor, hops, saved: body_sel };
                s.selection = selection;
            }
        }
        ContextStep::Note { footnote, id, story, selection } => {
            set_note_story(&mut s.doc, footnote, id, story);
            s.ctx = EditContext::Note { footnote, id, saved: body_sel };
            s.selection = selection;
        }
        ContextStep::HeaderFooter { part, story, selection, .. } => {
            if let Some(h) = s.doc.hf.make_mut(part.index()) {
                h.story = story;
            }
            s.dirty_hf.insert(part);
            // the TS re-enters the header/footer only when it was being edited (`hf`), on the
            // same page: D2a keeps the part edited before the pop (D2a-C2)
            if let EditContext::HeaderFooter { part: p0, page, .. } = prev {
                s.ctx = EditContext::HeaderFooter { part: *p0, page: *page, saved: body_sel };
                s.selection = if part == *p0 { selection } else { s.selection };
            }
        }
        ContextStep::Textbox { outer, page, .. } => {
            // re-entering the box (`reenterTextbox`) needs `contextTextboxRuns` and the layout's
            // object: the drawing family's (D2b-F7); D2a returns to the outer context
            if let (Some(p), EditContext::HeaderFooter { part: p0, .. }) = (outer, prev)
                && p == *p0
            {
                s.ctx = EditContext::HeaderFooter { part: p, page, saved: body_sel };
            }
        }
    }
    if !matches!(s.ctx, EditContext::HeaderFooter { .. })
        && let EditContext::HeaderFooter { part: p0, page, .. } = prev
        && !matches!(s.ctx, EditContext::Cell { .. } | EditContext::Note { .. })
    {
        // the step was not taken in the header: still re-entered, the cursor of then
        s.ctx = EditContext::HeaderFooter { part: *p0, page: *page, saved: body_sel };
    }
    let paras = story_of(&s.doc, &s.ctx).map(|st| &st.paras);
    s.selection = clamp_selection(paras, s.selection);
}

fn empty_paragraph(s: &mut Session) -> Paragraph {
    Paragraph { id: s.doc.ids.para(), ..Paragraph::default() }
}

impl Session {
    /// `exitCell()` (no change set: the caller's).
    pub(crate) fn exit_cell(&mut self) {
        if let EditContext::Cell { saved, .. } = self.ctx {
            self.ctx = EditContext::Body;
            self.selection = saved;
            self.pending_props = None;
            self.tx.events.push(Event::CellMode { active: false });
        }
    }

    /// `exitNote()`.
    pub(crate) fn exit_note(&mut self) {
        if let EditContext::Note { saved, .. } = self.ctx {
            self.ctx = EditContext::Body;
            self.selection = saved;
            self.pending_props = None;
            self.tx.events.push(Event::NoteMode { active: false, footnote: false, id: 0.0 });
        }
    }

    /// `enterNote(kind, id, caret)`: whether the note is now edited.
    pub(crate) fn enter_note(&mut self, footnote: bool, id: f64, caret: Option<DocPosition>, at_end: bool) -> bool {
        if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
            return false;
        }
        self.exit_cell();
        let same = matches!(self.ctx, EditContext::Note { footnote: f, id: i, .. } if f == footnote && NumKey::new(i) == NumKey::new(id));
        if !same {
            self.exit_note();
            let has = story_of(&self.doc, &EditContext::Note { footnote, id, saved: Selection::default() })
                .is_some_and(|s| s.paras.len() + s.tables.len() > 0);
            if !has {
                let p = empty_paragraph(self);
                let mut story = Story::new();
                story.push_paragraph(p);
                set_note_story(&mut self.doc, footnote, id, story);
            }
            self.float_sel = None;
            self.ctx = EditContext::Note { footnote, id, saved: self.selection };
            self.tx.events.push(Event::NoteMode { active: true, footnote, id });
        }
        let at = match (at_end, caret) {
            (true, _) => {
                let paras = self.paras();
                let last = paras.map_or(0, |p| p.len().saturating_sub(1));
                let len = paras.and_then(|p| p.get(last)).map_or(0, para_length);
                DocPosition::new(u32::try_from(last).unwrap_or(u32::MAX), u32::try_from(len).unwrap_or(u32::MAX))
            }
            (false, Some(c)) => c,
            (false, None) => DocPosition::default(),
        };
        // a caret from the shell is clamped to the note (the TS takes it as given; D2a-C4)
        self.selection = clamp_selection(self.paras(), Selection::caret(at));
        self.pending_props = None;
        true
    }

    /// `enterCell(anchor, hops, caret)`: whether the cell is now edited (a cell that does not exist
    /// or an editor in another context changes nothing). Shared with the notes family
    /// (`placeBodyCaret`, D2b-F5).
    pub(crate) fn enter_cell(&mut self, anchor: u32, hops: Vec<CellHop>, caret: Option<DocPosition>) -> bool {
        let ok = self.doc.body.tables.get(anchor as usize).is_some_and(|a| resolve_cell(&a.table, &hops).is_some());
        if !ok || !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. }) {
            return false;
        }
        self.exit_cell();
        let hops: Arc<[CellHop]> = hops.into();
        let probe = EditContext::Cell { anchor, hops: Arc::clone(&hops), saved: self.selection };
        if story_of(&self.doc, &probe).is_some_and(|s| s.paras.is_empty()) {
            // an empty cell gets a paragraph (the TS pushes it into the cell)
            let p = empty_paragraph(self);
            if let Some(st) = story_of_mut(&mut self.doc, &probe) {
                st.push_paragraph(p);
            }
        }
        self.float_sel = None;
        self.ctx = probe;
        self.selection = clamp_selection(self.paras(), Selection::caret(caret.unwrap_or_default()));
        self.pending_props = None;
        self.tx.events.push(Event::CellMode { active: true });
        true
    }

    /// `enterCell(anchor, hops, caret)` without the change set (D2b-F3 enters cells from its own
    /// commands): whether the cell is now edited.
    pub(crate) fn enter_cell_hops(&mut self, anchor: u32, hops: Vec<CellHop>, caret: Option<DocPosition>) -> bool {
        let ok = self.doc.body.tables.get(anchor as usize).is_some_and(|a| resolve_cell(&a.table, &hops).is_some());
        if !ok || !matches!(self.ctx, EditContext::Body | EditContext::Cell { .. }) {
            return false;
        }
        self.exit_cell();
        let hops: Arc<[CellHop]> = hops.into();
        let probe = EditContext::Cell { anchor, hops: Arc::clone(&hops), saved: self.selection };
        if story_of(&self.doc, &probe).is_some_and(|s| s.paras.is_empty()) {
            // an empty cell gets a paragraph (the TS pushes it into the cell)
            let p = empty_paragraph(self);
            if let Some(st) = story_of_mut(&mut self.doc, &probe) {
                st.push_paragraph(p);
            }
        }
        self.float_sel = None;
        self.ctx = probe;
        self.selection = clamp_selection(self.paras(), Selection::caret(caret.unwrap_or_default()));
        self.pending_props = None;
        self.tx.events.push(Event::CellMode { active: true });
        true
    }

    pub(crate) fn exec_context(&mut self, cmd: ContextCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            ContextCmd::EnterCell { anchor, hops, caret } => {
                self.enter_cell_hops(anchor, hops, caret);
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::ExitCell => {
                self.exit_cell();
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::EnterNote { footnote, id, caret, at_end } => {
                self.enter_note(footnote, id, caret, at_end);
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::ExitNote => {
                self.exit_note();
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::EnterHeaderFooter { part, page } => {
                if self.doc.hf.get(part.index()).is_none() {
                    return Ok(Outcome::Done(self.change_set()));
                }
                self.exit_cell();
                self.exit_note();
                if let EditContext::Textbox { .. } = self.ctx {
                    self.exit_textbox();
                }
                let saved = saved_selection(&self.ctx).unwrap_or(self.selection);
                self.float_sel = None;
                self.ctx = EditContext::HeaderFooter { part, page, saved };
                self.selection = Selection::default();
                self.pending_props = None;
                self.tx.events.push(Event::HfMode { active: true });
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::ExitHeaderFooter => {
                if let EditContext::Textbox { .. } = self.ctx {
                    self.exit_textbox();
                }
                if let EditContext::HeaderFooter { saved, .. } = self.ctx {
                    self.ctx = EditContext::Body;
                    // the body may have changed while the part was edited (an undo): clamped (D2a-C4)
                    self.selection = clamp_selection(self.paras(), saved);
                    self.pending_props = None;
                    self.tx.events.push(Event::HfMode { active: false });
                    // `exitHeaderFooter(false, true)` relays out: `syncSections` runs with the body in
                    // context again (what the edit of the page setup left pending, D2b-F4)
                    self.settle();
                }
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::EnterTextbox { para, run, host, focus, page } => {
                if !matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. }) {
                    return Ok(Outcome::Done(self.change_set()));
                }
                let probe = EditContext::Textbox { outer: Box::new(self.ctx.clone()), para, run, host, focus, page, saved: self.selection };
                if story_of(&self.doc, &probe).is_none() {
                    return Ok(Outcome::Done(self.change_set()));
                }
                self.float_sel = None;
                self.ctx = probe;
                self.selection = Selection::default();
                self.pending_props = None;
                self.tx.events.push(Event::TbMode { active: true });
                Ok(Outcome::Done(self.change_set()))
            }
            ContextCmd::ExitTextbox => {
                self.exit_textbox();
                Ok(Outcome::Done(self.change_set()))
            }
        }
    }

    /// `exitTextbox()`.
    pub(crate) fn exit_textbox(&mut self) {
        if let EditContext::Textbox { outer, saved, .. } = self.ctx.clone() {
            self.ctx = *outer;
            self.selection = saved;
            self.pending_props = None;
            self.tx.events.push(Event::TbMode { active: false });
        }
    }
}
