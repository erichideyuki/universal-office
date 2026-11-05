//! D2b family: review (frontier D2b-F6): `acceptRevisionAt`/`rejectRevisionAt`,
//! `acceptChange`/`rejectChange` (at the caret or over the selection, then to the next revision),
//! `acceptAllChanges`/`rejectAllChanges` (deep steps), `nextChange`/`previousChange`,
//! `selectRevision`, `setMarkupOptions`, and the queries over the revisions of every story
//! (`revisionSpans`, `revisionAuthors`, `authorColor`, `markupOptions`). `toggleTrackChanges`,
//! `author` and `setMarkupMode` are D2a's (`ops::settings`); the rules of accepting and rejecting
//! are D2a's `session::revisions` (docx-core `revisions.ts`).
//!
//! TS source: `universal-write.ts` 7944–8300 (the model half) and `editor/src/revisions.ts`
//! (`MarkupMode`, `MarkupOptions`, `DEFAULT_MARKUP`; the balloon texts of `formatDescription` stay
//! in the shell, declared F6-D4).
//!
//! The stories the TS lists as `Paragraph[]` (the body, every table cell, the header/footer parts,
//! the notes) are stories stored once here (A2): an entry is a [`StoryLoc`] that reaches its story
//! copy-on-write, and "the list is the active model" is the story's address.

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guard_runs;
use super::super::policy::GuardArg;
use super::super::revisions::{
    RevisionKind, RevisionScope, RevisionSpan, accept_revisions, collect_revisions, reject_revisions, revision_at, span_scope,
};
use super::super::state::Selection;
use super::super::text::{DocPosition, compare_pos, normalize_range, para_length};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use crate::ids::HfPartId;
use crate::js::{JsNumber, NumKey, Str};
use crate::model::Document;
use crate::model::para::Paragraph;
use crate::model::story::Story;
use crate::pvec::PVec;

// ───────────────────────── markup options ─────────────────────────

/// `MarkupOptions.balloons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Balloons {
    /// revisions in the balloons
    Revisions,
    /// everything inline
    Inline,
    /// only comments and formatting in the balloons (`DEFAULT_MARKUP`)
    #[default]
    Mixed,
}

/// `MarkupOptions` ("Show Markup"): which kinds of markup the views draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupOptions {
    /// insertions and deletions
    pub ins_del: bool,
    pub formatting: bool,
    pub comments: bool,
    pub balloons: Balloons,
    /// specific people: only the markup of these authors (`None` = all)
    pub authors: Option<Vec<Str>>,
}

impl Default for MarkupOptions {
    /// `DEFAULT_MARKUP`.
    fn default() -> Self {
        MarkupOptions { ins_del: true, formatting: true, comments: true, balloons: Balloons::Mixed, authors: None }
    }
}

impl MarkupOptions {
    /// Whether the options change what the caret skips (insertions/deletions off, or a person
    /// filter): the views of D2a's `hiddenChar` fast path.
    #[must_use]
    pub fn filters_text(&self) -> bool {
        !self.ins_del || self.authors.is_some()
    }
}

/// `Partial<MarkupOptions>`: a field left `None` is absent from the TS object. `authors` is a
/// double option: `Some(None)` is `authors: null`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkupPatch {
    pub ins_del: Option<bool>,
    pub formatting: Option<bool>,
    pub comments: Option<bool>,
    pub balloons: Option<Balloons>,
    pub authors: Option<Option<Vec<Str>>>,
}

// ───────────────────────── where a story is ─────────────────────────

/// One hop down into a table cell: the table anchor of the enclosing story, the row and the cell
/// of the row (indices into the stored shape, not grid columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStep {
    pub table: u32,
    pub row: u32,
    pub cell: u32,
}

/// A story of the document as a path (the TS keeps the `Paragraph[]` itself).
#[derive(Debug, Clone, PartialEq)]
pub enum StoryLoc {
    Body,
    /// a table cell of the body (nested tables: more hops)
    Cell(Vec<CellStep>),
    /// a header/footer part
    Hf(HfPartId),
    Note {
        footnote: bool,
        id: NumKey,
    },
}

fn cell_at<'a>(story: &'a Story, path: &[CellStep]) -> Option<&'a Story> {
    let (h, rest) = path.split_first()?;
    let a = story.tables.get(h.table as usize)?;
    let cell = a.table.rows.get(h.row as usize)?.cells.get(h.cell as usize)?;
    if rest.is_empty() { Some(&cell.content) } else { cell_at(&cell.content, rest) }
}

fn cell_at_mut<'a>(story: &'a mut Story, path: &[CellStep]) -> Option<&'a mut Story> {
    let (h, rest) = path.split_first()?;
    let a = story.tables.make_mut(h.table as usize)?;
    let t = Arc::make_mut(&mut a.table);
    let cell = t.rows.get_mut(h.row as usize)?.cells.get_mut(h.cell as usize)?;
    if rest.is_empty() { Some(&mut cell.content) } else { cell_at_mut(&mut cell.content, rest) }
}

/// The story at `loc`.
#[must_use]
pub fn story_at<'a>(doc: &'a Document, loc: &StoryLoc) -> Option<&'a Story> {
    match loc {
        StoryLoc::Body => Some(&doc.body),
        StoryLoc::Cell(path) => cell_at(&doc.body, path),
        StoryLoc::Hf(p) => doc.hf.get(p.index()).map(|h| &h.story),
        StoryLoc::Note { footnote, id } => {
            let map = if *footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
            map.get(id)
        }
    }
}

/// The story at `loc` for a change (copy-on-write down to it).
pub fn story_at_mut<'a>(doc: &'a mut Document, loc: &StoryLoc) -> Option<&'a mut Story> {
    match loc {
        StoryLoc::Body => Some(&mut doc.body),
        StoryLoc::Cell(path) => cell_at_mut(&mut doc.body, path),
        StoryLoc::Hf(p) => doc.hf.make_mut(p.index()).map(|h| &mut h.story),
        StoryLoc::Note { footnote, id } => {
            let notes = Arc::make_mut(&mut doc.notes);
            let map = if *footnote { &mut notes.footnotes } else { &mut notes.endnotes };
            map.get_mut(id)
        }
    }
}

/// Nesting deeper than this is not walked (the reader never builds it; a hostile hand-built
/// model must not overflow the stack).
const MAX_TABLE_DEPTH: usize = 64;

// ───────────────────────── the entries ─────────────────────────

/// `WriteRevision.story`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionStory {
    Body,
    Table,
    Header,
    Footer,
    Footnote,
    Endnote,
}

impl RevisionStory {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RevisionStory::Body => "body",
            RevisionStory::Table => "table",
            RevisionStory::Header => "header",
            RevisionStory::Footer => "footer",
            RevisionStory::Footnote => "footnote",
            RevisionStory::Endnote => "endnote",
        }
    }
}

/// `RevisionEntry` (`revisionSpans()`): a revision with the story that holds it.
#[derive(Debug, Clone)]
pub struct RevisionEntry {
    pub span: RevisionSpan,
    pub story: RevisionStory,
    pub loc: StoryLoc,
}

fn num_u32(x: JsNumber) -> u32 {
    uo_jsnum::conv::to_uint32(x.to_number())
}

fn len_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn push_list(out: &mut Vec<RevisionEntry>, paras: &PVec<Paragraph>, story: RevisionStory, loc: &StoryLoc) {
    for span in collect_revisions(paras.iter()) {
        out.push(RevisionEntry { span, story, loc: loc.clone() });
    }
}

/// `cells(t)` of `revisionSpans`: the cells of table `ti` of `story` (nested tables after their
/// cell), in order.
fn cells_of(story: &Story, ti: usize, path: &mut Vec<CellStep>, out: &mut Vec<RevisionEntry>) {
    let Some(a) = story.tables.get(ti) else { return };
    if path.len() >= MAX_TABLE_DEPTH {
        return;
    }
    for (ri, row) in a.table.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate() {
            path.push(CellStep { table: len_u32(ti), row: len_u32(ri), cell: len_u32(ci) });
            push_list(out, &cell.content.paras, RevisionStory::Table, &StoryLoc::Cell(path.clone()));
            for nti in 0..cell.content.tables.len() {
                cells_of(&cell.content, nti, path, out);
            }
            path.pop();
        }
    }
}

impl Session {
    /// `noteRefs(kind)` ids in document order without repeats: the references of the body and of
    /// the top-level cells of its tables (a table's cells come at its anchor).
    fn note_ids_in_order(&self, footnote: bool) -> Vec<NumKey> {
        let kind = if footnote { "footnote" } else { "endnote" };
        let mut out: Vec<NumKey> = Vec::new();
        let mut take = |p: &Paragraph| {
            for r in &p.runs {
                if let Some(n) = r.ext().and_then(|e| e.note_ref.as_ref())
                    && &*n.kind == kind
                {
                    let k = NumKey::new(n.id.to_number());
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
        };
        let body = &self.doc.body;
        let mut ai = 0usize;
        let n = body.paras.len();
        for pi in 0..=n {
            while let Some(a) = body.tables.get(ai)
                && a.before_para as usize == pi
            {
                for row in &a.table.rows {
                    for cell in &row.cells {
                        for p in cell.content.paras.iter() {
                            take(p);
                        }
                    }
                }
                ai += 1;
            }
            if let Some(p) = body.paras.get(pi) {
                take(p);
            }
        }
        out
    }

    /// `noteLists()`: the notes the body references that have paragraphs.
    fn note_locs(&self) -> Vec<(StoryLoc, RevisionStory)> {
        let mut out = Vec::new();
        for footnote in [true, false] {
            for id in self.note_ids_in_order(footnote) {
                let loc = StoryLoc::Note { footnote, id };
                if story_at(&self.doc, &loc).is_some_and(|s| !s.paras.is_empty()) {
                    out.push((loc, if footnote { RevisionStory::Footnote } else { RevisionStory::Endnote }));
                }
            }
        }
        out
    }

    /// `revisionSpans()`: the revisions of the whole document, in the order of `getRevisions`:
    /// the body's (with the tables' at their anchors), the header/footer parts', the notes'.
    #[must_use]
    pub fn revision_entries(&self) -> Vec<RevisionEntry> {
        let mut out: Vec<RevisionEntry> = Vec::new();
        let body = &self.doc.body;
        let mut ai = 0usize;
        let mut path: Vec<CellStep> = Vec::new();
        for span in collect_revisions(body.paras.iter()) {
            while let Some(a) = body.tables.get(ai)
                && a.before_para <= num_u32(span.para)
            {
                cells_of(body, ai, &mut path, &mut out);
                ai += 1;
            }
            out.push(RevisionEntry { span, story: RevisionStory::Body, loc: StoryLoc::Body });
        }
        while ai < body.tables.len() {
            cells_of(body, ai, &mut path, &mut out);
            ai += 1;
        }
        let mut seen: Vec<HfPartId> = Vec::new();
        for sec in self.doc.sections.iter() {
            for (set, refs, story) in
                [(&sec.headers, &sec.header_refs, RevisionStory::Header), (&sec.footers, &sec.footer_refs, RevisionStory::Footer)]
            {
                for r in refs {
                    if let Some(part) = set.get(&r.r#type).copied()
                        && !seen.contains(&part)
                    {
                        seen.push(part);
                        if let Some(h) = self.doc.hf.get(part.index()) {
                            push_list(&mut out, &h.story.paras, story, &StoryLoc::Hf(part));
                        }
                    }
                }
            }
        }
        for (loc, story) in self.note_locs() {
            if let Some(s) = story_at(&self.doc, &loc) {
                push_list(&mut out, &s.paras, story, &loc);
            }
        }
        out
    }

    /// The number of revisions of the document (`revisionSpans().length`).
    #[must_use]
    pub fn revision_count(&self) -> usize {
        self.revision_entries().len()
    }

    /// `revisionAuthors()`: the authors with revisions now, in colour order (those who revised in
    /// this session included).
    #[must_use]
    pub fn revision_authors_now(&self) -> Vec<Str> {
        let now: Vec<Str> = self.revision_entries().into_iter().map(|e| e.span.author).collect();
        self.rev_authors.iter().filter(|a| now.contains(a)).cloned().collect()
    }

    /// `authorColor(author)`: the markup colour (RRGGBB) of an author, the same as the text's and
    /// the bar's.
    #[must_use]
    pub fn author_color_of(&self, author: &str) -> &'static str {
        crate::word::revisions::author_color(&self.rev_authors, author)
    }

    /// `getMarkupOptions()`.
    #[must_use]
    pub fn markup_options(&self) -> &MarkupOptions {
        &self.opts.markup_options
    }

    /// Whether the story at `loc` is the story the context edits (`e.list === this.model.paragraphs`).
    fn is_active(&self, loc: &StoryLoc) -> bool {
        match (story_at(&self.doc, loc), self.story()) {
            (Some(a), Some(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }

    /// `applyRevisionScope(paras, scope, accept, body)` on the story at `target` (`None` = the
    /// context's own). The anchors follow the paragraph joins only in the body context.
    fn apply_revision_scope(&mut self, target: Option<&StoryLoc>, scope: &RevisionScope, accept: bool) -> bool {
        let follow = matches!(self.ctx, EditContext::Body) && target.is_none_or(|l| self.is_active(l));
        let mut merges: Vec<u32> = Vec::new();
        let changed = {
            let story = match target {
                None => self.story_mut(),
                Some(l) => story_at_mut(&mut self.doc, l),
            };
            story.is_some_and(|s| apply_list(&mut s.paras, scope, accept, &mut merges))
        };
        if follow {
            for removed in merges {
                self.shift_anchors(removed.saturating_sub(1), -1);
            }
        }
        changed
    }

    /// `clampPos(p)`: a position inside the context's paragraphs, in the view's canonical form.
    fn clamp_pos_ctx(&self, p: DocPosition) -> DocPosition {
        let Some(paras) = self.paras() else { return DocPosition::default() };
        let Some(last) = paras.len().checked_sub(1) else { return DocPosition::default() };
        let para = p.para.min(len_u32(last));
        let len = paras.get(para as usize).map_or(0, |x| len_u32(para_length(x)));
        self.canon_pos(DocPosition { para, offset: if para == p.para { p.offset.min(len) } else { 0 } })
    }

    fn clamp_selection_ctx(&mut self) {
        let sel = self.selection;
        self.selection = Selection { anchor: self.clamp_pos_ctx(sel.anchor), focus: self.clamp_pos_ctx(sel.focus) };
    }

    /// `activeSpans()`: the revisions of the context's story.
    fn active_spans(&self) -> Vec<RevisionSpan> {
        self.paras().map(|p| collect_revisions(p.iter())).unwrap_or_default()
    }

    /// `spanEndPos(s)`: the end of a revision's selection (a paragraph mark: the start of the next
    /// paragraph).
    fn span_end_pos(&self, s: &RevisionSpan) -> DocPosition {
        let p = s.end_para.map_or_else(|| num_u32(s.para), num_u32);
        let n = self.paras().map_or(0, PVec::len);
        if s.mark == Some(true) && (p as usize).saturating_add(1) < n {
            DocPosition::new(p + 1, 0)
        } else {
            DocPosition::new(p, num_u32(s.end))
        }
    }

    /// `stepChange(dir)` (`nextChange`/`previousChange`): selects the revision after (before) the
    /// caret, wrapping around. No model change, no step.
    fn step_change(&mut self, forward: bool) -> bool {
        if self.paras().is_none() {
            return false;
        }
        let spans = self.active_spans();
        if spans.is_empty() {
            return false;
        }
        let start = normalize_range(self.range()).start;
        let sel = self.has_selection();
        let at = |s: &RevisionSpan| compare_pos(DocPosition::new(num_u32(s.para), num_u32(s.start)), start);
        let target = if forward {
            spans.iter().find(|s| if sel { at(s).is_gt() } else { at(s).is_ge() }).or(spans.first())
        } else {
            spans.iter().rev().find(|s| at(s).is_lt()).or(spans.last())
        };
        let Some(target) = target else { return false };
        let a = DocPosition::new(num_u32(target.para), num_u32(target.start));
        let b = self.span_end_pos(target);
        self.selection = Selection { anchor: a, focus: b };
        self.pending_props = None;
        true
    }

    /// `shownScope()`: the kinds and authors turned on in Show Markup.
    fn shown_scope(&self) -> RevisionScope {
        let o = &self.opts.markup_options;
        let mut kinds: Vec<RevisionKind> = Vec::new();
        if o.ins_del {
            kinds.push(RevisionKind::Insert);
            kinds.push(RevisionKind::Delete);
        }
        if o.formatting {
            kinds.push(RevisionKind::Format);
            kinds.push(RevisionKind::ParaFormat);
        }
        RevisionScope { range: None, kinds: Some(kinds), authors: o.authors.clone(), runs_only: false }
    }

    /// `resolveEntry(index, accept)`: one revision of `getRevisions`, in any story.
    fn resolve_entry(&mut self, index: u32, accept: bool) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let entries = self.revision_entries();
        let Some(e) = entries.get(index as usize) else { return Ok(Outcome::Done(self.change_set())) };
        let active = self.is_active(&e.loc);
        self.begin_step(!active, &[]);
        // `spanScope({ ...e.span, para: e.idx })`: `idx` is the span's own paragraph
        let scope = span_scope(&e.span);
        let changed = self.apply_revision_scope(Some(&e.loc), &scope, accept);
        if !changed {
            self.discard_step();
            return Ok(Outcome::Done(self.change_set()));
        }
        if let StoryLoc::Hf(part) = e.loc {
            self.dirty_hf.insert(part);
        }
        self.clamp_selection_ctx();
        if active {
            self.touch_structure();
        } else {
            self.touch_all();
        }
        let mut cs = self.finish();
        cs.events.push(Event::RevisionsChanged);
        Ok(Outcome::Done(cs))
    }

    /// `resolveChange(accept, moveNext)`: the revision at the caret, or those of the selection.
    fn resolve_change(&mut self, accept: bool, move_next: bool) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let scope = if self.has_selection() {
            RevisionScope { range: Some(self.range()), ..RevisionScope::default() }
        } else {
            let spans = self.active_spans();
            match revision_at(&spans, self.selection.focus).and_then(|i| spans.get(i)) {
                Some(span) => span_scope(span),
                None => {
                    if move_next {
                        self.step_change(true);
                    }
                    return Ok(Outcome::Done(self.change_set()));
                }
            }
        };
        self.begin_step(false, &[]);
        let at = normalize_range(self.range()).start;
        let changed = self.apply_revision_scope(None, &scope, accept);
        if !changed {
            self.discard_step();
            if move_next {
                self.step_change(true);
            }
            return Ok(Outcome::Done(self.change_set()));
        }
        let pos = self.clamp_pos_ctx(at);
        self.selection = Selection::caret(pos);
        self.touch_structure();
        let mut cs = self.finish();
        cs.events.push(Event::RevisionsChanged);
        if move_next {
            self.step_change(true);
            cs.selection = self.selection;
        } else {
            cs.selection = self.selection;
        }
        Ok(Outcome::Done(cs))
    }

    /// `exitHeaderFooter()` (the arm of `ContextCmd::ExitHeaderFooter`).
    fn exit_header_footer_ctx(&mut self) {
        if let EditContext::Textbox { .. } = self.ctx {
            self.exit_textbox();
        }
        if let EditContext::HeaderFooter { saved, .. } = self.ctx {
            self.ctx = EditContext::Body;
            self.selection = saved;
            self.pending_props = None;
            self.emit(Event::HfMode { active: false });
        }
    }

    /// `resolveAll(accept, opts)`: every revision (or the shown ones) of the body, the tables, the
    /// notes and the headers/footers, in one deep step. `revision_count` before and after tells
    /// the number resolved (the TS return value).
    fn resolve_all(&mut self, accept: bool, shown: bool, stop_tracking: bool) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let n = self.revision_count();
        if stop_tracking && self.opts.tracking {
            // `toggleTrackChanges(false)`
            self.opts.tracking = false;
            self.history.note_edit();
            self.changed();
            self.emit(Event::TrackChanges { on: false });
        }
        if n == 0 {
            return Ok(Outcome::Done(self.change_set()));
        }
        if matches!(self.ctx, EditContext::Cell { .. }) {
            self.exit_cell();
        }
        if matches!(self.ctx, EditContext::Textbox { .. }) {
            self.exit_textbox();
        }
        if matches!(self.ctx, EditContext::HeaderFooter { .. }) {
            self.exit_header_footer_ctx();
        }
        if matches!(self.ctx, EditContext::Note { .. }) {
            self.exit_note();
        }
        let scope = if shown { self.shown_scope() } else { RevisionScope::default() };
        self.begin_step(true, &[]);
        self.apply_revision_scope(Some(&StoryLoc::Body), &scope, accept);
        for (loc, _) in self.note_locs() {
            self.apply_revision_scope(Some(&loc), &scope, accept);
        }
        // the cells of the body's tables (nested tables after their cell)
        let mut merges: Vec<u32> = Vec::new();
        for ai in 0..self.doc.body.tables.len() {
            if let Some(a) = self.doc.body.tables.make_mut(ai) {
                apply_table(Arc::make_mut(&mut a.table), &scope, accept, &mut merges, 0);
            }
        }
        let mut parts: Vec<HfPartId> = Vec::new();
        for sec in self.doc.sections.iter() {
            for (set, refs) in [(&sec.headers, &sec.header_refs), (&sec.footers, &sec.footer_refs)] {
                for r in refs {
                    if let Some(part) = set.get(&r.r#type).copied() {
                        parts.push(part);
                    }
                }
            }
        }
        for part in parts {
            if self.apply_revision_scope(Some(&StoryLoc::Hf(part)), &scope, accept) {
                self.dirty_hf.insert(part);
            }
        }
        self.clamp_selection_ctx();
        self.touch_all();
        let mut cs = self.finish();
        cs.events.push(Event::RevisionsChanged);
        Ok(Outcome::Done(cs))
    }

    pub(crate) fn exec_revision_cmds(&mut self, cmd: RevisionCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            RevisionCmd::AcceptAt { index } => self.resolve_entry(index, true),
            RevisionCmd::RejectAt { index } => self.resolve_entry(index, false),
            RevisionCmd::Accept { move_next } => self.resolve_change(true, move_next),
            RevisionCmd::Reject { move_next } => self.resolve_change(false, move_next),
            RevisionCmd::AcceptAll { shown, stop_tracking } => self.resolve_all(true, shown, stop_tracking),
            RevisionCmd::RejectAll { shown, stop_tracking } => self.resolve_all(false, shown, stop_tracking),
            RevisionCmd::NextChange => {
                self.step_change(true);
                Ok(Outcome::Done(self.change_set()))
            }
            RevisionCmd::PreviousChange => {
                self.step_change(false);
                Ok(Outcome::Done(self.change_set()))
            }
            RevisionCmd::SelectRevision { index } => {
                // the revision of the body's own list; another story's is scrolled to by the shell
                let entries = self.revision_entries();
                if let Some(e) = entries.get(index as usize)
                    && e.story == RevisionStory::Body
                    && matches!(self.ctx, EditContext::Body)
                {
                    let a = DocPosition::new(num_u32(e.span.para), num_u32(e.span.start));
                    let b = self.span_end_pos(&e.span);
                    self.selection = Selection { anchor: a, focus: b };
                    self.pending_props = None;
                }
                Ok(Outcome::Done(self.change_set()))
            }
            RevisionCmd::SetMarkupOptions(p) => {
                // `{ ...this.markup, ...opts }`, then `refresh()` and `uw-markup`
                let o = &mut self.opts.markup_options;
                if let Some(v) = p.ins_del {
                    o.ins_del = v;
                }
                if let Some(v) = p.formatting {
                    o.formatting = v;
                }
                if let Some(v) = p.comments {
                    o.comments = v;
                }
                if let Some(v) = p.balloons {
                    o.balloons = v;
                }
                if let Some(v) = p.authors {
                    o.authors = v;
                }
                let mut cs = self.finish();
                cs.events.push(Event::Markup);
                Ok(Outcome::Done(cs))
            }
        }
    }
}

/// Accepts or rejects in one list of paragraphs, with the run guard (`guardRuns(model, 0, last,
/// fn)`) over the WHOLE list even when nothing in it changes: the guard rebuilds the runs' props
/// (their key order is part of the TS run signature the save compares, D2a-U6), so skipping it for a
/// list without revisions would save a different file. `merges` collects the paragraphs joined
/// into the previous one.
fn apply_list(paras: &mut PVec<Paragraph>, scope: &RevisionScope, accept: bool, merges: &mut Vec<u32>) -> bool {
    let hi = len_u32(paras.len().saturating_sub(1));
    guard_runs(paras, 0, hi, |p| {
        let mut on_merge = |removed: u32| merges.push(removed);
        if accept { accept_revisions(p, scope, &mut on_merge) } else { reject_revisions(p, scope, &mut on_merge) }
    })
}

/// `cells(t)` of `resolveAll`: every cell's paragraphs, then the cell's nested tables.
fn apply_table(t: &mut crate::model::table::Table, scope: &RevisionScope, accept: bool, merges: &mut Vec<u32>, depth: usize) {
    for row in &mut t.rows {
        for cell in &mut row.cells {
            apply_list(&mut cell.content.paras, scope, accept, merges);
            if depth + 1 < MAX_TABLE_DEPTH {
                for ti in 0..cell.content.tables.len() {
                    if let Some(a) = cell.content.tables.make_mut(ti) {
                        apply_table(Arc::make_mut(&mut a.table), scope, accept, merges, depth + 1);
                    }
                }
            }
        }
    }
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum RevisionCmd {
    /// `acceptRevisionAt(index)` (the order of `getRevisions`)
    AcceptAt { index: u32 },
    /// `rejectRevisionAt(index)`
    RejectAt { index: u32 },
    /// `acceptChange({ moveNext })`: the revision at the caret / of the selection
    Accept { move_next: bool },
    /// `rejectChange({ moveNext })`
    Reject { move_next: bool },
    /// `acceptAllChanges({ shown, stopTracking })`
    AcceptAll { shown: bool, stop_tracking: bool },
    /// `rejectAllChanges({ shown, stopTracking })`
    RejectAll { shown: bool, stop_tracking: bool },
    /// `nextChange()`
    NextChange,
    /// `previousChange()`
    PreviousChange,
    /// `selectRevision(index)` (the model half: the body's revisions)
    SelectRevision { index: u32 },
    /// `setMarkupOptions(opts)`
    SetMarkupOptions(MarkupPatch),
}

impl RevisionCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads:
    /// none of the review methods is guarded.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}
