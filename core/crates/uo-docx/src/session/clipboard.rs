//! The clipboard model (D2a): the Office Clipboard of the TS (`OFFICE_CLIPBOARD`: the current
//! rich copy and the last 24 items, shared by every element of the page) as one
//! [`ClipboardRing`] per wasm instance (G7), held by the binding and lent to each command
//! ([`super::ExecCx::clipboard`]). A [`Clip`] holds values, never references into a document: the
//! TS document identity (`RichClip.doc`) and the source `WeakRef` (`src.el`) become the
//! [`ClipOrigin`] (session id + generation, U3). The paste rules that read a clip are the
//! clipboard family's (D2b-F8).
//!
//! Besides the TS count cap (24, `rememberClip`), the ring has a byte cap (M5): past it the
//! oldest items go (the current clip stays).

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::js::Str;
use crate::model::Story;
use crate::model::TableAnchor;
use crate::model::para::ParaProps;
use crate::model::para::Paragraph;
use crate::model::run::{
    CustomInline, DrawingGroup, DrawingShape, DrawingTextbox, GroupItem, InlineImage, NoteRef, RprLink, Run, RunExt, RunProps,
};
use crate::model::table::{Cell, Row, Table};
use crate::text::Utf16;

use super::env::LocalNow;
use super::text::DocRange;

/// The TS ring length (`if (ring.length > 24) ring.length = 24`).
pub const MAX_CLIPS: usize = 24;
/// The byte cap of the ring (M5): 32 MiB.
pub const DEFAULT_MAX_CLIP_BYTES: usize = 32 << 20;

/// Where a clip was copied (`RichClip.doc`, `src`): the session and its document generation (a
/// session that loads another document bumps the generation, the TS `docToken`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipOrigin {
    pub session: u32,
    pub generation: u32,
}

/// Selected cells copied as a new table (`cells`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipCells {
    pub table: Arc<Table>,
    pub rows: bool,
    pub whole: bool,
}

/// One run of a rich copy (`ClipRun`, universal-write.ts:1213). Its `props` are the TS spread
/// `{ ...run.props }`, which copies the symbol keys too (`link`, [`Run::rpr_link`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipRun {
    pub text: Utf16,
    pub props: Arc<RunProps>,
    /// the symbol keys the props spread carries (`RPR_REF`/`RPR_EDITED`)
    pub link: Option<Box<RprLink>>,
    /// a note reference (`noteRef`) and a copy of the note's content (`note`)
    pub note_ref: Option<NoteRef>,
    pub note: Option<Story>,
    /// a picture/shape/box/group copied whole (`obj`)
    pub obj: Option<Box<Run>>,
    /// a plugin inline copied from this editor (`custom`)
    pub custom: Option<CustomInline>,
    /// the bold of an HTML block (`blockBold`)
    pub block_bold: bool,
}

/// An HTML list item of a copy (`ClipPara.htmlList`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipHtmlList {
    pub ordered: bool,
    pub level: f64,
    pub key: f64,
}

/// One paragraph of a rich copy (`ClipPara`): the runs and, when the mark (¶) was copied, its
/// formatting (`mark`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipPara {
    pub runs: Vec<ClipRun>,
    pub mark: Option<Arc<ParaProps>>,
    /// the list number as text (`marker`, "Keep Text Only")
    pub marker: Option<Utf16>,
    pub html_list: Option<ClipHtmlList>,
}

/// A whole table between the copied paragraphs (`tables[i]`): it goes in front of paragraph
/// `before` of the copy.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipTable {
    pub before: u32,
    pub table: Arc<Table>,
}

/// A rich copy (`RichClip`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Clip {
    /// the plain text (`text`)
    pub text: Utf16,
    /// the copied paragraphs (`paras`)
    pub paras: Vec<ClipPara>,
    /// the tables between them (`tables`; empty = the TS absent key)
    pub tables: Vec<ClipTable>,
    pub origin: Option<ClipOrigin>,
    /// the document's path in the host (`path`)
    pub path: Option<Str>,
    /// lists used: numId → raw `w:abstractNum` (`lists`)
    pub lists: Vec<(f64, Arc<str>)>,
    /// paragraph styles used: styleId → `w:name` (`styleNames`)
    pub style_names: Vec<(Str, Str)>,
    /// where it came from (`src`, Paste Link)
    pub src: Option<(ClipOrigin, DocRange)>,
    pub cells: Option<ClipCells>,
    /// a selected drawing object (`drawing`)
    pub drawing: Option<Run>,
    /// the payloads the clip's pictures refer to (the TS objects hold the bytes; a run here refers
    /// to a media id, so the clip keeps its own list: D2b-F8 `clone.rs`)
    pub media: Vec<Arc<[u8]>>,
}

/// The size charged for one `Arc` allocation's header (strong + weak counts).
const ARC_HEADER: usize = 16;

/// A node still to price (an explicit stack: copied tables can nest deeper than the reader's 64).
enum Work<'a> {
    Story(&'a Story),
    Table(&'a Table),
    Run(&'a Run),
}

/// The estimate of [`Clip::bytes`]: every model node the clip keeps, each shared `Arc` (props,
/// marks, `w:rPr`/`w:pPr`, tables) priced once per clip, the props at their struct size.
struct Est<'a> {
    seen: HashSet<*const ()>,
    stack: Vec<Work<'a>>,
    n: usize,
}

fn str_len(s: Option<&Str>) -> usize {
    s.map_or(0, |s| s.len())
}

impl<'a> Est<'a> {
    fn shared<T>(&mut self, a: &Arc<T>) -> bool {
        let fresh = self.seen.insert(Arc::as_ptr(a).cast::<()>());
        if fresh {
            self.n += std::mem::size_of::<T>() + ARC_HEADER;
        }
        fresh
    }

    fn run(&mut self, r: &'a Run) {
        self.n += r.text.heap_bytes();
        self.shared(&r.props);
        if let Some(x) = &r.rpr {
            self.shared(x);
        }
        if let Some(l) = &r.rpr_link {
            self.n += std::mem::size_of::<RprLink>();
            if let Some(x) = &l.rpr {
                self.shared(x);
            }
        }
        let Some(e) = r.ext() else { return };
        self.n += std::mem::size_of::<RunExt>() + str_len(e.field.as_ref()) + str_len(e.field_code.as_ref());
        if let Some(i) = &e.image {
            self.n += std::mem::size_of::<InlineImage>() + str_len(i.raw_drawing_xml.as_ref());
        }
        if let Some(t) = &e.textbox {
            self.n += std::mem::size_of::<DrawingTextbox>() + str_len(t.raw_drawing_xml.as_ref());
            self.stack.push(Work::Story(&t.content));
        }
        if let Some(sh) = &e.shape {
            self.n += std::mem::size_of::<DrawingShape>() + str_len(sh.raw_drawing_xml.as_ref());
        }
        if let Some(g) = &e.group {
            self.n += std::mem::size_of::<DrawingGroup>()
                + str_len(g.raw_drawing_xml.as_ref())
                + g.items.capacity() * std::mem::size_of::<GroupItem>();
            for it in &g.items {
                self.stack.push(Work::Run(&it.run));
            }
        }
        if let Some(c) = &e.custom {
            self.n += std::mem::size_of_val(&**c);
        }
        if let Some(rs) = &e.field_result {
            self.n += rs.capacity() * std::mem::size_of::<Run>();
            for x in rs {
                self.stack.push(Work::Run(x));
            }
        }
    }

    fn story(&mut self, s: &'a Story) {
        for p in s.paras.iter() {
            self.n += std::mem::size_of::<Paragraph>() + ARC_HEADER + p.runs.capacity() * std::mem::size_of::<Run>();
            self.shared(&p.props);
            if let Some(x) = &p.ppr {
                self.shared(x);
            }
            for r in &p.runs {
                self.run(r);
            }
        }
        for a in s.tables.iter() {
            self.n += std::mem::size_of::<TableAnchor>() + ARC_HEADER;
            self.table(&a.table);
        }
    }

    fn table(&mut self, t: &'a Arc<Table>) {
        if self.shared(t) {
            self.stack.push(Work::Table(t));
        }
    }

    fn table_body(&mut self, t: &'a Table) {
        self.n += t.grid_cols_twips.capacity() * 8 + t.rows.capacity() * std::mem::size_of::<Row>();
        if let Some(x) = &t.tbl_pr {
            self.shared(x);
        }
        for row in &t.rows {
            self.n += row.cells.capacity() * std::mem::size_of::<Cell>();
            if let Some(x) = &row.tr_pr {
                self.shared(x);
            }
            if let Some(x) = &row.tbl_pr_ex {
                self.shared(x);
            }
            for c in &row.cells {
                if let Some(x) = &c.tc_pr {
                    self.shared(x);
                }
                self.n += str_len(c.shading_hex.as_ref()) + c.borders.as_ref().map_or(0, |b| std::mem::size_of_val(&**b));
                self.stack.push(Work::Story(&c.content));
            }
        }
    }

    fn drain(&mut self) {
        while let Some(w) = self.stack.pop() {
            match w {
                Work::Story(s) => self.story(s),
                Work::Table(t) => self.table_body(t),
                Work::Run(r) => {
                    self.n += std::mem::size_of::<Run>();
                    self.run(r);
                }
            }
        }
    }
}

impl Clip {
    /// An estimate of the bytes the clip keeps alive (for the ring's byte cap): the paragraphs
    /// with their runs, props, marks, notes and objects; the tables and the cells with their
    /// stories (nested tables, textboxes, note stories); the media, lists and style names.
    #[must_use]
    pub fn bytes(&self) -> usize {
        let mut e = Est { seen: HashSet::new(), stack: Vec::new(), n: std::mem::size_of::<Clip>() + self.text.heap_bytes() };
        e.n += self.paras.capacity() * std::mem::size_of::<ClipPara>();
        for p in &self.paras {
            e.n += p.marker.as_ref().map_or(0, Utf16::heap_bytes) + p.runs.capacity() * std::mem::size_of::<ClipRun>();
            if let Some(m) = &p.mark {
                e.shared(m);
            }
            for r in &p.runs {
                e.n += r.text.heap_bytes();
                e.shared(&r.props);
                if let Some(l) = &r.link {
                    e.n += std::mem::size_of::<RprLink>();
                    if let Some(x) = &l.rpr {
                        e.shared(x);
                    }
                }
                if let Some(s) = &r.note {
                    e.stack.push(Work::Story(s));
                }
                if let Some(o) = &r.obj {
                    e.n += std::mem::size_of::<Run>();
                    e.stack.push(Work::Run(o));
                }
                e.drain();
            }
        }
        e.n += self.tables.capacity() * std::mem::size_of::<ClipTable>();
        for t in &self.tables {
            e.table(&t.table);
            e.drain();
        }
        if let Some(c) = &self.cells {
            e.table(&c.table);
            e.drain();
        }
        if let Some(d) = &self.drawing {
            e.stack.push(Work::Run(d));
            e.drain();
        }
        e.n += self.media.iter().map(|b| b.len() + ARC_HEADER).sum::<usize>();
        e.n += self.lists.iter().map(|(_, s)| s.len() + ARC_HEADER + 8).sum::<usize>();
        e.n += self.style_names.iter().map(|(a, b)| a.len() + b.len() + 2 * std::mem::size_of::<Str>()).sum::<usize>();
        e.n
    }
}

/// One item of the ring (`{ id, clip, at }`).
#[derive(Debug, Clone, PartialEq)]
pub struct RingItem {
    pub id: u32,
    pub clip: Arc<Clip>,
    pub at: LocalNow,
}

/// The Office Clipboard of one wasm instance (see the module comment).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardRing {
    current: Option<Arc<Clip>>,
    ring: VecDeque<RingItem>,
    seq: u32,
    max_bytes: usize,
    bytes: usize,
    /// bumped on every change (the TS `listeners`: the shell polls it)
    version: u32,
}

impl Default for ClipboardRing {
    fn default() -> Self {
        ClipboardRing::new(DEFAULT_MAX_CLIP_BYTES)
    }
}

impl ClipboardRing {
    #[must_use]
    pub fn new(max_bytes: usize) -> ClipboardRing {
        ClipboardRing { current: None, ring: VecDeque::new(), seq: 0, max_bytes, bytes: 0, version: 0 }
    }

    /// The current rich copy (`richClip`).
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Clip>> {
        self.current.as_ref()
    }

    pub fn set_current(&mut self, clip: Option<Arc<Clip>>) {
        self.current = clip;
        self.version = self.version.wrapping_add(1);
    }

    /// The items, newest first.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &RingItem> + '_ {
        self.ring.iter()
    }

    /// The item with an id.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&RingItem> {
        self.ring.iter().find(|x| x.id == id)
    }

    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// `rememberClip(clip)`: the copy goes first; an equal item (the same clip, or the same
    /// non-empty text without a drawing on either side) leaves its old place; at most 24 items
    /// and the byte cap.
    pub fn remember(&mut self, clip: Arc<Clip>, at: LocalNow) -> u32 {
        let dup = self.ring.iter().position(|x| {
            Arc::ptr_eq(&x.clip, &clip)
                || (x.clip.text == clip.text && clip.drawing.is_none() && x.clip.drawing.is_none() && !x.clip.text.is_empty())
        });
        if let Some(i) = dup
            && let Some(old) = self.ring.remove(i)
        {
            self.bytes = self.bytes.saturating_sub(old.clip.bytes());
        }
        self.seq = self.seq.wrapping_add(1);
        self.bytes = self.bytes.saturating_add(clip.bytes());
        self.ring.push_front(RingItem { id: self.seq, clip, at });
        while self.ring.len() > MAX_CLIPS || (self.bytes > self.max_bytes && self.ring.len() > 1) {
            match self.ring.pop_back() {
                Some(old) => self.bytes = self.bytes.saturating_sub(old.clip.bytes()),
                None => break,
            }
        }
        self.version = self.version.wrapping_add(1);
        self.seq
    }

    /// Removes one item (`deleteClipboardItem`).
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(i) = self.ring.iter().position(|x| x.id == id) else { return false };
        if let Some(old) = self.ring.remove(i) {
            self.bytes = self.bytes.saturating_sub(old.clip.bytes());
        }
        self.version = self.version.wrapping_add(1);
        true
    }

    /// Empties the ring (`clearClipboard`).
    pub fn clear(&mut self) {
        self.ring.clear();
        self.bytes = 0;
        self.version = self.version.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(t: &str) -> Arc<Clip> {
        Arc::new(Clip { text: Utf16::from(t), ..Clip::default() })
    }

    #[test]
    fn ring_semantics() {
        let mut r = ClipboardRing::default();
        for i in 0..30 {
            r.remember(clip(&format!("c{i}")), LocalNow::default());
        }
        assert_eq!(r.len(), MAX_CLIPS);
        // the same text moves to the front
        let first = r.items().last().map(|x| x.clip.text.clone()).unwrap_or_default();
        r.remember(Arc::new(Clip { text: first.clone(), ..Clip::default() }), LocalNow::default());
        assert_eq!(r.len(), MAX_CLIPS);
        assert_eq!(r.items().next().map(|x| x.clip.text.clone()), Some(first));
        // empty texts never dedupe
        r.remember(clip(""), LocalNow::default());
        r.remember(clip(""), LocalNow::default());
        assert_eq!(r.items().take(2).filter(|x| x.clip.text.is_empty()).count(), 2);
    }

    #[test]
    fn byte_cap() {
        let mut r = ClipboardRing::new(4096);
        for i in 0..20 {
            r.remember(clip(&"x".repeat(1000 + i)), LocalNow::default());
        }
        assert!(r.bytes() <= 4096 || r.len() == 1);
        assert!(r.len() < 20);
    }
}
