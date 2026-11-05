//! One undo step (architecture A3 `EditState`): the fields of the TS `stringifyState`, as shared
//! roots (U19: every collection persistent, so a snapshot costs O(touched chunks)). The TS keys are
//! kept as field docs; the fields D2b families own are typed now and filled by them.

use std::sync::Arc;

use crate::ids::HfPartId;
use crate::js::{JsMap, Str};
use crate::model::Story;
use crate::model::para::Paragraph;
use crate::model::story::TableAnchor;
use crate::pvec::PVec;

use super::history::StepCost;
use super::text::DocPosition;

/// The selection of the edited story (`{ anchor, focus }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: DocPosition,
    pub focus: DocPosition,
}

impl Selection {
    #[must_use]
    pub const fn caret(pos: DocPosition) -> Selection {
        Selection { anchor: pos, focus: pos }
    }

    /// `hasSelection()`.
    #[must_use]
    pub fn is_range(&self) -> bool {
        self.anchor != self.focus
    }
}

/// Bookmark → body paragraph (`bookmarkParas`), shifted by `shiftAnchors` (Enter, joins,
/// deletes across paragraphs). The TS rewrites every value after the edit point on each shift;
/// here a shift is appended to a short log (shared by the history steps) and folded into the base
/// map once the log is long (U19: a structural keystroke then costs O(log) instead of a copy of
/// every bookmark name; D2a-M1). Values are exactly the TS ones: each shift is
/// `v > from ? max(0, v + delta) : v`, applied in order.
#[derive(Debug, Clone, Default)]
pub struct BookmarkParas {
    base: Arc<JsMap<Str, u32>>,
    shifts: Arc<Vec<(u32, i64)>>,
}

/// Shifts kept before the base map is rewritten.
pub const BOOKMARK_SHIFT_LOG: usize = 512;

impl BookmarkParas {
    #[must_use]
    pub fn new(base: JsMap<Str, u32>) -> BookmarkParas {
        BookmarkParas { base: Arc::new(base), shifts: Arc::new(Vec::new()) }
    }

    fn apply(&self, mut v: u32) -> u32 {
        for &(from, delta) in self.shifts.iter() {
            if v > from {
                v = u32::try_from((i64::from(v) + delta).max(0)).unwrap_or(u32::MAX);
            }
        }
        v
    }

    /// `shiftAnchors` on the bookmarks.
    pub fn shift(&mut self, from: u32, delta: i64) {
        if delta == 0 || self.base.is_empty() {
            return;
        }
        if self.shifts.len() >= BOOKMARK_SHIFT_LOG {
            self.compact();
        }
        Arc::make_mut(&mut self.shifts).push((from, delta));
    }

    /// Folds the shift log into the base map.
    pub fn compact(&mut self) {
        if self.shifts.is_empty() {
            return;
        }
        let map: JsMap<Str, u32> = self.base.iter().map(|(k, v)| (k.clone(), self.apply(*v))).collect();
        self.base = Arc::new(map);
        self.shifts = Arc::new(Vec::new());
    }

    /// The paragraph of a bookmark.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<u32> {
        self.base.get(name).map(|v| self.apply(*v))
    }

    /// The bookmarks in TS `Map` order with their paragraphs.
    pub fn iter(&self) -> impl Iterator<Item = (&Str, u32)> + '_ {
        self.base.iter().map(|(k, v)| (k, self.apply(*v)))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.base.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
    }

    /// The map for a change (the bookmarks family: insert, delete): the log folded first.
    pub fn map_mut(&mut self) -> &mut JsMap<Str, u32> {
        self.compact();
        Arc::make_mut(&mut self.base)
    }

    /// Shares its storage with `other` (history pricing).
    #[must_use]
    pub fn shares(&self, other: &BookmarkParas) -> bool {
        Arc::ptr_eq(&self.base, &other.base)
    }

    /// An estimate of the bytes it keeps.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.base.iter().map(|(k, _)| k.len() + 16).sum::<usize>() + self.shifts.len() * 16
    }
}

impl PartialEq for BookmarkParas {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

/// A version of a part kept by a [`super::versions::VersionStore`] (`sv`, `nv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct VersionId(pub u32);

/// The edit context a step was taken in, and how to return to it (`ce`, `nt`, `hp`, `te`).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ContextStep {
    #[default]
    Body,
    /// `ce: { a, h, s }`: the anchor of the top table, the hops down to the cell, the selection in it
    Cell { anchor: u32, hops: Arc<[CellHop]>, selection: Selection },
    /// `nt: { k, id, p, s }`: the note being edited, its content, the selection in it
    Note { footnote: bool, id: f64, story: Story, selection: Selection },
    /// `hp: { k, b, s }`: the header/footer part being edited, its content, the selection in it,
    /// and the page it was entered on (`hfActivePage`)
    HeaderFooter { part: HfPartId, story: Story, selection: Selection, page: u32 },
    /// `te: { h, f, pg, s }`: the textbox run (its order among the outer story's textbox runs), the
    /// focus box of a chain, the page, the selection in it; `outer` is the header/footer part the
    /// box lives in (`None`: the body)
    Textbox { outer: Option<HfPartId>, host: u32, focus: u32, page: u32, selection: Selection },
}

impl ContextStep {
    /// The undo normalisation of the stories the step carries (A3 "Restore").
    pub fn normalize_stories(&mut self, ids: &mut crate::ids::IdAlloc) {
        if let ContextStep::Note { story, .. } | ContextStep::HeaderFooter { story, .. } = self {
            super::normalize::normalize(story, ids);
        }
    }
}

/// One hop from a table to one of its cells (`CellHop { r, c, b? }` of engine-doc.ts): the row,
/// the grid column the cell starts at, and for every hop but the last the block index of the
/// nested table inside that cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellHop {
    pub row: u32,
    pub col: u32,
    pub block: Option<u32>,
}

/// The table selection (`TableSel` of the editor, D2b-F3): the anchor of the top table, the hops
/// down to the cell that CONTAINS the table (empty: a body table), the cell rectangle and the fixed
/// corner it grows from (`origin`: where the drag or Shift+arrow began).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableSel {
    pub anchor: u32,
    pub path: Arc<[CellHop]>,
    pub row0: u32,
    pub col0: u32,
    pub row1: u32,
    pub col1: u32,
    pub origin_row: u32,
    pub origin_col: u32,
}

/// The document-level settings an undo step carries (`eo`, `dt`, `hy`, `np`, `bg`, `bgd`), behind
/// one `Arc` (they change rarely: a step shares it with the next). Each field is the EDITED value
/// (`None`: the file's, untouched since the install): the save writes only what is `Some` (the TS
/// `evenOdd !== null`, `defaultTab !== null`, `hyph`, `noteSettings` changed, `bgKey` changed).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsStep {
    /// `eo`: different odd and even pages
    pub even_odd: Option<bool>,
    /// `dt`: the default tab stop (twips)
    pub default_tab: Option<f64>,
    /// `hy`: the hyphenation options (D2b text layout)
    pub hyphenation: Option<crate::write::emit::settings::HyphenationSettings>,
    /// `np`: the note options of the document (D2b notes): footnote, endnote
    pub note_settings: Option<super::ops::notes::NoteSettings>,
    /// `bg`: the page background (D2b page), `Some(None)` = removed
    pub background: Option<Option<Arc<crate::model::background::PageBackground>>>,
    /// `bgd`: `w:displayBackgroundShape`
    pub display_background: Option<bool>,
}

/// One undo step (see the module comment).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditState {
    /// `m` + `tb` + `anc.t`: the body story (paragraphs and table anchors), always the body (in a
    /// cell or a note the TS `m` is the saved body model too)
    pub body: Story,
    /// `s`: the body selection
    pub selection: Selection,
    /// `anc.b`: bookmark → paragraph (shifted by Enter/Backspace)
    pub bookmarks: BookmarkParas,
    /// `ce`/`nt`/`hp`/`te`
    pub context: ContextStep,
    /// `ts`
    pub table_sel: Option<TableSel>,
    /// `fs`: the selected drawing object (its order among the context's drawing runs)
    pub float_sel: Option<u32>,
    /// `sv`, `nv`: the style sheet and numbering versions (U12)
    pub styles: Option<VersionId>,
    pub numbering: Option<VersionId>,
    pub settings: Arc<SettingsStep>,
    /// the body's last-section `sectPr` (`m.sectPr`: in the TS it travels with the model; the other
    /// sections' are in the break paragraphs of `body`; D2b-F5 `setNoteOptions` edits it)
    pub body_sect_pr: Arc<crate::model::section::SectPr>,
    /// `hf` (deep steps: accept/reject all): every header/footer part (the TS lists the six slots
    /// of every section; a part is stored once here)
    pub deep_hf: Option<Arc<[(HfPartId, Story)]>>,
    /// `nb` (deep steps): every note but the one being edited
    pub deep_notes: Option<Arc<[(bool, f64, Story)]>>,
    /// `hx`: the header/footer parts the step changes (other than the one being edited)
    pub hf_parts: Arc<[(HfPartId, Story)]>,
}

/// A rough heap size of a paragraph (the cap is an estimate: runs, texts, rare fields).
fn para_bytes(p: &Paragraph) -> usize {
    let mut n = std::mem::size_of::<Paragraph>() + p.runs.capacity() * std::mem::size_of::<crate::model::run::Run>();
    for r in &p.runs {
        n += r.text.heap_bytes();
        if let Some(e) = r.ext() {
            n += std::mem::size_of_val(e);
            if let Some(tb) = &e.textbox {
                n += story_bytes(&tb.content, 1);
            }
        }
    }
    n
}

fn story_bytes(s: &Story, depth: u32) -> usize {
    let mut n = 0;
    for p in s.paras.iter() {
        n += para_bytes(p);
    }
    for a in s.tables.iter() {
        n += table_bytes(a, depth);
    }
    n
}

fn table_bytes(a: &TableAnchor, depth: u32) -> usize {
    // nesting is bounded by the reader (64); the estimate stops counting deeper than 64
    if depth > 64 {
        return 0;
    }
    let mut n = std::mem::size_of::<crate::model::table::Table>();
    for row in &a.table.rows {
        for cell in &row.cells {
            n += std::mem::size_of_val(cell) + story_bytes(&cell.content, depth + 1);
        }
    }
    n
}

/// The bytes of `a` not held by `b` by pointer, per element: an element is shared when it is the
/// same `Arc` as `b`'s element at its index or a neighbour (one insertion or deletion moves the
/// rest of its chunk by one; the other chunks are shared whole).
fn pvec_unshared<T>(a: &PVec<T>, b: &PVec<T>, size: impl Fn(&T) -> usize) -> usize {
    let mut n = 0;
    for (i, x) in a.unshared_chunk_elements(b) {
        let shared =
            [i.checked_sub(1), Some(i), i.checked_add(1)].into_iter().flatten().any(|j| b.get_arc(j).is_some_and(|y| Arc::ptr_eq(x, y)));
        if !shared {
            n += size(x);
        }
    }
    n
}

impl StepCost for EditState {
    fn unshared_bytes(&self, next: &EditState) -> usize {
        let mut n = std::mem::size_of::<EditState>();
        n += pvec_unshared(&self.body.paras, &next.body.paras, para_bytes);
        n += pvec_unshared(&self.body.tables, &next.body.tables, |a| table_bytes(a, 0));
        if !self.bookmarks.shares(&next.bookmarks) {
            n += self.bookmarks.bytes();
        } else if !Arc::ptr_eq(&self.bookmarks.shifts, &next.bookmarks.shifts) {
            n += self.bookmarks.shifts.len() * 16;
        }
        if !Arc::ptr_eq(&self.hf_parts, &next.hf_parts) {
            n += self.hf_parts.iter().map(|(_, s)| story_bytes(s, 0)).sum::<usize>();
        }
        if let Some(d) = &self.deep_hf
            && !next.deep_hf.as_ref().is_some_and(|n| Arc::ptr_eq(d, n))
        {
            n += d.iter().map(|(_, s)| story_bytes(s, 0)).sum::<usize>();
        }
        if let Some(d) = &self.deep_notes
            && !next.deep_notes.as_ref().is_some_and(|n| Arc::ptr_eq(d, n))
        {
            n += d.iter().map(|(_, _, s)| story_bytes(s, 0)).sum::<usize>();
        }
        if let ContextStep::Note { story, .. } | ContextStep::HeaderFooter { story, .. } = &self.context {
            n += story_bytes(story, 0);
        }
        n
    }
}
