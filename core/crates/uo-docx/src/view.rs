//! The layout-facing API (architecture A4, G21 as approved; uo-docx-layout "Requirements on
//! uo-docx"): what uo-docx-layout reads ([`DocView`]), the references from its output back to
//! the model ([`DocRef`], [`StoryRef`], [`BlockPath`], [`RunRef`], [`CellKey`]) and the
//! [`LayoutOracle`] the session calls for the layout-dependent mutations (implemented in the
//! bindings by uo-docx-layout; uo-docx never depends on layout).

use std::collections::HashSet;
use std::sync::Arc;

use crate::ids::{DrawingId, HfPartId, MediaId, ParaId, TableId};
use crate::js::NumKey;
use crate::model::background::PageBackground;
use crate::model::document::{Bookmarks, Comment, CoreProps, Notes};
use crate::model::formatting::{DocFormatting, StylesMap};
use crate::model::run::DrawingTextbox;
use crate::model::section::{HfPart, Section};
use crate::model::story::{Block, Story};
use crate::model::{Cell, Document, Paragraph, Run, Table};
use crate::text::Utf16;

/// Footnote or endnote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Footnote,
    Endnote,
}

/// Header or footer (the TS `hdr:`/`ftr:` prefix of a line ref, K13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HfWhich {
    Header,
    Footer,
}

/// Which story a reference points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryRef {
    Body,
    /// a header/footer part (the slot is in the layout's line ref, K13)
    HeaderFooter(HfPartId),
    Note(NoteKind, NumKey),
    /// the content of a textbox, by its drawing
    Textbox(DrawingId),
    /// the story edited in the session's context (editor only)
    Context,
}

/// The path of a block in a story, as the TS ref `b3/r0/c1/b0`: a block index, then
/// (row, cell, block) per table level. Up to two table levels are stored inline.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BlockPath {
    len: u8,
    inline: [u32; 7],
    heap: Option<Arc<[u32]>>,
}

impl BlockPath {
    /// A top-level block.
    #[must_use]
    pub fn root(block: u32) -> BlockPath {
        BlockPath { len: 1, inline: [block, 0, 0, 0, 0, 0, 0], heap: None }
    }

    /// The steps (`[b, r, c, b, r, c, b, …]`).
    #[must_use]
    pub fn steps(&self) -> &[u32] {
        match &self.heap {
            Some(h) => h,
            None => self.inline.get(..usize::from(self.len)).unwrap_or(&[]),
        }
    }

    /// The path of block `block` of cell (`row`, `cell`) of the table at this path.
    #[must_use]
    pub fn cell_block(&self, row: u32, cell: u32, block: u32) -> BlockPath {
        let steps = self.steps();
        let n = steps.len() + 3;
        if n <= 7 {
            let mut inline = self.inline;
            if let Some(dst) = inline.get_mut(steps.len()..n) {
                dst.copy_from_slice(&[row, cell, block]);
            }
            BlockPath { len: n as u8, inline, heap: None }
        } else {
            let mut v = steps.to_vec();
            v.extend_from_slice(&[row, cell, block]);
            BlockPath { len: u8::MAX, inline: [0; 7], heap: Some(v.into()) }
        }
    }

    /// The top-level block index.
    #[must_use]
    pub fn top(&self) -> u32 {
        self.steps().first().copied().unwrap_or(0)
    }

    /// Table levels crossed (0 for a top-level block).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.steps().len() / 3
    }
}

impl std::fmt::Debug for BlockPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

/// The TS ref text: `b3/r0/c1/b0`.
impl std::fmt::Display for BlockPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, s) in self.steps().iter().enumerate() {
            let p = match i % 3 {
                0 => 'b',
                1 => 'r',
                _ => 'c',
            };
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{p}{s}")?;
        }
        Ok(())
    }
}

/// A position in the model: a story, a block path, a UTF-16 offset in the paragraph's text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocRef {
    pub story: StoryRef,
    pub path: BlockPath,
    pub offset: u32,
}

/// Where a run sits inside its paragraph's run (nested model runs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RunSub {
    /// the paragraph's run itself
    #[default]
    None,
    /// run `i` of its `fieldResult`
    FieldResult(u32),
    /// the run of item `i` of its group
    GroupItem(u32),
}

/// A model run (`onField(run, …)`, `PlacedObject.run`): every model run reachable from a
/// layout copy is addressable (K1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunRef {
    pub story: StoryRef,
    /// the paragraph
    pub path: BlockPath,
    pub run: u32,
    pub sub: RunSub,
}

/// A table cell (`cellNb`, K2b): the table, the row index, the cell index in its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub table: TableId,
    pub row: u32,
    pub cell: u32,
}

/// The story edited by the session (editor only, milestone 2).
#[derive(Debug, Clone, Copy)]
pub struct ContextView<'a> {
    pub story: &'a Story,
}

/// The read-only view of what the layout needs, borrowed from a [`Document`] (or a session).
/// Always the reconciled block order (never a stale cell view).
#[derive(Debug, Clone, Copy)]
pub struct DocView<'a> {
    doc: &'a Document,
    context: Option<ContextView<'a>>,
}

impl<'a> DocView<'a> {
    #[must_use]
    pub fn new(doc: &'a Document) -> DocView<'a> {
        DocView { doc, context: None }
    }

    /// With the session's edited story.
    #[must_use]
    pub fn with_context(doc: &'a Document, context: ContextView<'a>) -> DocView<'a> {
        DocView { doc, context: Some(context) }
    }

    #[must_use]
    pub fn document(&self) -> &'a Document {
        self.doc
    }

    #[must_use]
    pub fn body(&self) -> &'a Story {
        &self.doc.body
    }

    /// Block ranges, `SectPr`, `SectionProps`, header/footer slots.
    #[must_use]
    pub fn sections(&self) -> &'a [Section] {
        &self.doc.sections
    }

    #[must_use]
    pub fn hf_part(&self, part: HfPartId) -> Option<&'a HfPart> {
        self.doc.hf.get(part.index())
    }

    #[must_use]
    pub fn hf_story(&self, part: HfPartId) -> Option<&'a Story> {
        self.hf_part(part).map(|p| &p.story)
    }

    /// The header (or footer) slots of a section in TS object order: slot type → part (K13).
    pub fn hf_slots(&self, section: usize, which: HfWhich) -> impl Iterator<Item = (&'a str, HfPartId)> + 'a {
        self.doc.sections.get(section).into_iter().flat_map(move |s| {
            let rec = match which {
                HfWhich::Header => &s.headers,
                HfWhich::Footer => &s.footers,
            };
            rec.iter().map(|(k, v)| (k, *v))
        })
    }

    #[must_use]
    pub fn notes(&self) -> &'a Notes {
        &self.doc.notes
    }

    #[must_use]
    pub fn note_story(&self, kind: NoteKind, id: NumKey) -> Option<&'a Story> {
        match kind {
            NoteKind::Footnote => self.doc.notes.footnotes.get(&id),
            NoteKind::Endnote => self.doc.notes.endnotes.get(&id),
        }
    }

    #[must_use]
    pub fn comments(&self) -> &'a [Comment] {
        &self.doc.comments
    }

    /// name → block/offset/end (REF, PAGEREF).
    #[must_use]
    pub fn bookmarks(&self) -> &'a Bookmarks {
        &self.doc.bookmarks
    }

    /// Styles (resolved and raw), numbering, theme, settings, fonts.
    #[must_use]
    pub fn formatting(&self) -> &'a DocFormatting {
        &self.doc.formatting
    }

    /// The resolved styles (`FullDocument.styles`).
    #[must_use]
    pub fn styles(&self) -> &'a StylesMap {
        &self.doc.styles
    }

    /// CREATEDATE/SAVEDATE/PRINTDATE.
    #[must_use]
    pub fn core_props(&self) -> Option<&'a CoreProps> {
        self.doc.core_props.as_ref()
    }

    #[must_use]
    pub fn background(&self) -> Option<&'a PageBackground> {
        self.doc.background.as_deref()
    }

    #[must_use]
    pub fn media(&self, id: MediaId) -> Option<&'a [u8]> {
        self.doc.media.get(id)
    }

    /// The shared bytes of a payload (resources of a layout share them, uo-render RQ3).
    #[must_use]
    pub fn media_arc(&self, id: MediaId) -> Option<&'a Arc<[u8]>> {
        self.doc.media.get_arc(id)
    }

    /// Bumps on any style, numbering, settings, font table, theme or section change.
    #[must_use]
    pub fn formatting_version(&self) -> u64 {
        self.doc.formatting_version
    }

    /// The story a reference points into.
    #[must_use]
    pub fn story(&self, r: &StoryRef) -> Option<&'a Story> {
        match *r {
            StoryRef::Body => Some(&self.doc.body),
            StoryRef::HeaderFooter(p) => self.hf_story(p),
            StoryRef::Note(k, id) => self.note_story(k, id),
            StoryRef::Textbox(d) => self.find_textbox(d).map(|t| &t.content),
            StoryRef::Context => self.context.map(|c| c.story),
        }
    }

    /// The block at `path` in `story`.
    #[must_use]
    pub fn block(&self, story: &StoryRef, path: &BlockPath) -> Option<Block<'a>> {
        let steps = path.steps();
        let mut s = self.story(story)?;
        let mut i = 0;
        loop {
            let b = s.block_at(*steps.get(i)? as usize)?;
            if i + 1 == steps.len() {
                return Some(b);
            }
            let Block::Table(t) = b else { return None };
            let (row, cell) = (*steps.get(i + 1)? as usize, *steps.get(i + 2)? as usize);
            s = &t.cell(row, cell)?.content;
            i += 3;
        }
    }

    /// The paragraph at `path` in `story`.
    #[must_use]
    pub fn paragraph(&self, story: &StoryRef, path: &BlockPath) -> Option<&'a Paragraph> {
        match self.block(story, path)? {
            Block::Paragraph(p) => Some(p),
            Block::Table(_) => None,
        }
    }

    /// The model run a [`RunRef`] names.
    #[must_use]
    pub fn run(&self, r: &RunRef) -> Option<&'a Run> {
        let run = self.paragraph(&r.story, &r.path)?.runs.get(r.run as usize)?;
        match r.sub {
            RunSub::None => Some(run),
            RunSub::FieldResult(i) => run.ext()?.field_result.as_ref()?.get(i as usize),
            RunSub::GroupItem(i) => run.ext()?.group.as_ref()?.items.get(i as usize).map(|it| &it.run),
        }
    }

    /// Every story of the document (body, header/footer parts, notes, and nested cells and
    /// textboxes, in document order), visited iteratively; `f` returns `true` to stop.
    fn walk_stories(&self, f: &mut dyn FnMut(&'a Story) -> bool) -> bool {
        let mut roots: Vec<&'a Story> = vec![&self.doc.body];
        roots.extend(self.doc.hf.iter().map(|p| &p.story));
        roots.extend(self.doc.notes.footnotes.values());
        roots.extend(self.doc.notes.endnotes.values());
        let mut stack: Vec<&'a Story> = roots.into_iter().rev().collect();
        while let Some(s) = stack.pop() {
            if f(s) {
                return true;
            }
            let mut nested: Vec<&'a Story> = Vec::new();
            for b in s.blocks() {
                match b {
                    Block::Paragraph(p) => {
                        for r in &p.runs {
                            push_run_stories(r, &mut nested);
                        }
                    }
                    Block::Table(t) => {
                        for row in &t.rows {
                            for c in &row.cells {
                                nested.push(&c.content);
                            }
                        }
                    }
                }
            }
            stack.extend(nested.into_iter().rev());
        }
        false
    }

    /// The table with this id (a walk of the document: O(document)).
    #[must_use]
    pub fn find_table(&self, id: TableId) -> Option<&'a Table> {
        let mut found = None;
        self.walk_stories(&mut |s| {
            found = s.tables.iter().map(|a| &*a.table).find(|t| t.id == id);
            found.is_some()
        });
        found
    }

    /// A cell by key (O(document), see [`DocView::find_table`]).
    #[must_use]
    pub fn cell(&self, key: CellKey) -> Option<&'a Cell> {
        self.find_table(key.table)?.cell(key.row as usize, key.cell as usize)
    }

    /// The textbox with this drawing id (O(document)).
    #[must_use]
    pub fn find_textbox(&self, id: DrawingId) -> Option<&'a DrawingTextbox> {
        let mut found = None;
        self.walk_stories(&mut |s| {
            for p in s.paras.iter() {
                for r in &p.runs {
                    if let Some(t) = find_textbox_in_run(r, id) {
                        found = Some(t);
                        return true;
                    }
                }
            }
            false
        });
        found
    }

    /// Revision authors in reading order (body, headers/footers, notes): `revisionAuthors` of
    /// docx-core `revisions.ts`, iterative.
    #[must_use]
    pub fn revision_authors(&self) -> Vec<&'a str> {
        let mut seen: Vec<&'a str> = Vec::new();
        let mut add = |a: Option<&'a str>| {
            if let Some(a) = a
                && !seen.contains(&a)
            {
                seen.push(a);
            }
        };
        enum W<'a> {
            Blocks(crate::model::story::Blocks<'a>),
            Para(&'a Paragraph),
            Runs(&'a Paragraph, usize),
            Marks(&'a Paragraph),
        }
        let mut roots: Vec<&'a Story> = vec![&self.doc.body];
        for s in self.doc.sections.iter() {
            for rec in [&s.headers, &s.footers] {
                for k in ["default", "first", "even"] {
                    if let Some(st) = rec.get(k).and_then(|id| self.hf_story(*id)) {
                        roots.push(st);
                    }
                }
            }
        }
        roots.extend(self.doc.notes.footnotes.values());
        roots.extend(self.doc.notes.endnotes.values());
        for root in roots {
            let mut stack = vec![W::Blocks(root.blocks())];
            while let Some(top) = stack.pop() {
                match top {
                    W::Blocks(mut it) => match it.next() {
                        None => {}
                        Some(Block::Paragraph(p)) => {
                            stack.push(W::Blocks(it));
                            stack.push(W::Para(p));
                        }
                        Some(Block::Table(t)) => {
                            stack.push(W::Blocks(it));
                            for row in t.rows.iter().rev() {
                                for c in row.cells.iter().rev() {
                                    stack.push(W::Blocks(c.content.blocks()));
                                }
                            }
                        }
                    },
                    W::Runs(p, i) => {
                        let Some(r) = p.runs.get(i) else { continue };
                        stack.push(W::Runs(p, i + 1));
                        add(r.props.del.as_ref().map(|x| &*x.author));
                        add(r.props.ins.as_ref().map(|x| &*x.author));
                        add(r.props.fmt.as_ref().map(|x| &*x.author));
                        if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                            match tb.view {
                                crate::model::run::TextboxView::ParasOnly => {
                                    for q in tb.content.paras.iter().rev() {
                                        stack.push(W::Para(q));
                                    }
                                }
                                _ => stack.push(W::Blocks(tb.content.blocks())),
                            }
                        }
                    }
                    W::Para(p) => {
                        add(p.props.pfmt.as_ref().map(|r| &*r.author));
                        stack.push(W::Marks(p));
                        stack.push(W::Runs(p, 0));
                    }
                    W::Marks(p) => {
                        add(p.props.mark_ins.as_ref().map(|x| &*x.author));
                        add(p.props.mark_del.as_ref().map(|x| &*x.author));
                    }
                }
            }
        }
        seen
    }
}

fn push_run_stories<'a>(r: &'a Run, out: &mut Vec<&'a Story>) {
    let Some(e) = r.ext() else { return };
    if let Some(tb) = &e.textbox {
        out.push(&tb.content);
    }
    if let Some(g) = &e.group {
        for it in &g.items {
            push_run_stories(&it.run, out);
        }
    }
}

fn find_textbox_in_run(r: &Run, id: DrawingId) -> Option<&DrawingTextbox> {
    let e = r.ext()?;
    if let Some(tb) = e.textbox.as_deref()
        && tb.drawing == id
    {
        return Some(tb);
    }
    e.group.as_ref()?.items.iter().find_map(|it| find_textbox_in_run(&it.run, id))
}

/// Which dynamic fields the oracle reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldScope {
    All,
    /// only these runs (model runs inside layout copies included, K1)
    Runs(HashSet<RunRef>),
}

/// Manual hyphenation refusals: (paragraph, offset) (the TS `hyphenationSkip(para, offset)` by
/// paragraph object).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HyphenSkips(pub HashSet<(ParaId, u32)>);

/// The next point where the forced hyphenation breaks a body line (`ln.hyph`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyphenCandidate {
    pub at: DocRef,
}

/// The class of a layout failure (a `LayoutError` means no page and no PDF, LQ2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutErrorKind {
    /// an input limit (`LayoutLimits`)
    Limit,
    /// the work budget
    Budget,
    /// anything else (a bug)
    Internal,
}

/// A layout failure reported through the oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    pub kind: LayoutErrorKind,
    pub detail: Box<str>,
}

/// The layout-dependent part of the session (G21 as approved: two methods): implemented by
/// uo-docx-layout in the bindings.
pub trait LayoutOracle {
    /// A full layout reporting the last value of each dynamic field run (the TS `onField`,
    /// `refreshFieldCache`): first-set order, model runs inside copied paragraphs included,
    /// sliced and frame runs absent.
    fn field_values(&mut self, doc: DocView<'_>, scope: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError>;

    /// Forced hyphenation (`nextManualHyphenation`): the first body hyphenation point at or after
    /// `from`, skipping `skip`; `None` when there is none.
    fn next_hyphenation(&mut self, doc: DocView<'_>, from: &DocRef, skip: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError>;

    /// The page number the shell last showed for each paragraph (the TS `pageLabelOf`,
    /// universal-write.ts:6623: `page.number` of the first line of the paragraph in the editor's
    /// last layout, matched by paragraph identity; `None` when the paragraph has no line there).
    /// D2b-F10: the TOC, table-of-figures and gallery sources read it for every entry (G21 revisit:
    /// K17c dropped `pages_of` because TOC numbers come through `onField`, but this call site
    /// exists and is observable). The session calls it ONCE, at the start of a command that builds
    /// TOC sources, with the unchanged view. The default answers an error so that an oracle that
    /// does not implement it fails loudly instead of showing page "1" everywhere.
    ///
    /// # Errors
    /// The default always errs (`Internal`).
    fn shown_page_numbers(&mut self, _doc: DocView<'_>, _paras: &[DocRef]) -> Result<Vec<Option<f64>>, LayoutError> {
        Err(LayoutError { kind: LayoutErrorKind::Internal, detail: "shown_page_numbers: not implemented by this oracle".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_paths() {
        let p = BlockPath::root(3).cell_block(0, 1, 0);
        assert_eq!(p.to_string(), "b3/r0/c1/b0");
        let deep = p.cell_block(2, 0, 5).cell_block(1, 1, 1);
        assert_eq!(deep.to_string(), "b3/r0/c1/b0/r2/c0/b5/r1/c1/b1");
        assert_eq!(deep.depth(), 3);
        assert_eq!(deep.top(), 3);
        assert_eq!(BlockPath::root(7).steps(), &[7]);
    }
}
