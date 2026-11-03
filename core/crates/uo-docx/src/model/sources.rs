//! The original XML of the document (docx-core `source.ts`): the basis of the lossless save.
//! The reader (frontier R6) records raw slices of every paragraph, run, table, row, cell and
//! top-level child of each part; the model carries only [`SourceKey`]s (`src`), values that
//! survive undo. Immutable after the read (A2); the baseline signatures (`sig`) are taken by the
//! session at install (milestone 2, G23), not here.
//!
//! Raw strings are [`SrcText`]: a span of the part's text (UTF-8 offsets into the text the XML
//! reader parsed), or an owned string when the TS builds one (`openOf`'s `/>` → `>`). The
//! digest writes them by content (A6); positions (`at`) are UTF-8 offsets here and UTF-16 in the
//! TS (the walker converts them).

use indexmap::IndexSet;
use uo_opc::xml::Source;

use crate::ids::SourceKey;
use crate::js::{JsMap, JsNumber, JsValue, JsonWriter, NumKey, Str};
use crate::js_object;

/// A raw XML string of the source store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SrcText {
    /// bytes `start..end` of part `part` (an index into [`SourceStore::parts`])
    Span { part: u32, start: u32, end: u32 },
    /// a string the reader built
    Owned(Str),
}

impl Default for SrcText {
    fn default() -> Self {
        SrcText::Owned("".into())
    }
}

/// The text of a part of the store (`word/document.xml` …), as the XML reader parsed it: the
/// reader's own shared buffer (a reference-counted handle, never a copy).
#[derive(Debug, Clone)]
pub struct SourcePart {
    pub path: Str,
    pub text: Source,
}

impl PartialEq for SourcePart {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && *self.text == *other.text
    }
}

impl SrcText {
    /// The text, given the store's parts.
    pub fn resolve<'a>(&'a self, parts: &'a [SourcePart]) -> &'a str {
        match self {
            SrcText::Owned(s) => s,
            SrcText::Span { part, start, end } => {
                parts.get(*part as usize).and_then(|p| p.text.get(*start as usize..*end as usize)).unwrap_or("")
            }
        }
    }
}

impl JsValue for SrcText {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let parts = w.parts();
        w.text_digest(self.resolve(parts));
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        let parts = w.parts();
        w.str(self.resolve(parts));
        true
    }
}

/// A key of the process-global counter `wrapIds` (fulldoc.ts): per read here, relabelled in the
/// digest (G7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WrapId(pub u32);

impl JsValue for WrapId {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.id(crate::canon::IdSpace::Wrap, &self.0.to_string());
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.num(f64::from(self.0));
        true
    }
}

js_object! {
    /// A top-level child of a `w:p` and the model runs it produced (`SegSource`).
    pub struct SegSource {
        req raw: SrcText = "raw",
        req sids: Vec<JsNumber> = "sids",
        opt open: bool = "open",
        opt wrap_open: SrcText = "wrapOpen",
        opt wrap_close: SrcText = "wrapClose",
        opt link: Str = "link",
        opt tip: Str = "tip",
    }
}

js_object! {
    /// The source of a paragraph (`ParaSource`; `sig` is the session's).
    pub struct ParaSource {
        req raw: SrcText = "raw",
        req open: SrcText = "open",
        /// UTF-8 offset in the part (UTF-16 in the TS)
        opt at: u32 = "at",
        opt p_pr: SrcText = "pPr",
        req segs: Vec<SegSource> = "segs",
    }
}

js_object! {
    /// The source of a run (`RunSource`).
    pub struct RunSource {
        opt r_pr: SrcText = "rPr",
        opt obj: SrcText = "obj",
        opt obj_head: JsNumber = "objHead",
    }
}

js_object! {
    /// The source of a table (`TableSource`; `sig` is the session's).
    pub struct TableSource {
        req raw: SrcText = "raw",
        /// UTF-8 offset in the part (UTF-16 in the TS)
        opt at: u32 = "at",
        req open: SrcText = "open",
        opt tbl_pr: SrcText = "tblPr",
        opt tbl_grid: SrcText = "tblGrid",
    }
}

js_object! {
    /// A raw piece around a row or cell (`WrapSeg`).
    pub struct WrapSeg {
        req raw: SrcText = "raw",
        opt open: WrapId = "open",
        opt close: WrapId = "close",
    }
}

js_object! {
    /// The source of a table row (`RowSource`).
    pub struct RowSource {
        req open: SrcText = "open",
        opt tr_pr: SrcText = "trPr",
        opt tbl_pr_ex: SrcText = "tblPrEx",
        opt pre: Vec<WrapSeg> = "pre",
        opt post: Vec<WrapSeg> = "post",
    }
}

js_object! {
    /// The source of a table cell (`CellSource`).
    pub struct CellSource {
        req open: SrcText = "open",
        opt tc_pr: SrcText = "tcPr",
        opt groups: Vec<StoryGroup> = "groups",
        opt pre: Vec<WrapSeg> = "pre",
        opt post: Vec<WrapSeg> = "post",
    }
}

/// A key of `blockSdtKeys` in a story group (`StoryGroup.sdtKey`).
pub use super::para::SdtKey;

js_object! {
    /// A top-level child of a part and the blocks it produced (`StoryGroup`).
    pub struct StoryGroup {
        req raw: SrcText = "raw",
        /// `src` of each block, in order (0 when none)
        req blocks: Vec<JsNumber> = "blocks",
        opt wrap_open: SrcText = "wrapOpen",
        opt wrap_close: SrcText = "wrapClose",
        opt inner: Vec<StoryGroup> = "inner",
        opt sdt_key: SdtKey = "sdtKey",
    }
}

js_object! {
    /// The source of a part's story (`StorySource`).
    pub struct StorySource {
        req open: SrcText = "open",
        req close: SrcText = "close",
        req groups: Vec<StoryGroup> = "groups",
        opt sect_pr: SrcText = "sectPr",
    }
}

js_object! {
    /// One `w:footnote`/`w:endnote` of a notes part (`NotesPartSource.items[i]`).
    pub struct NoteItemSource {
        req id: JsNumber = "id",
        req raw: SrcText = "raw",
        opt separator: bool = "separator",
        opt story: StorySource = "story",
    }
}

/// A TS `Set<number>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NumSet(pub IndexSet<NumKey>);

impl JsValue for NumSet {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.set(self.0.len());
        for k in &self.0 {
            k.canon(w);
        }
    }
    /// `JSON.stringify(new Set(…))` is `{}`.
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.raw("{}");
        true
    }
}

js_object! {
    /// A notes part (`NotesPartSource`).
    pub struct NotesPartSource {
        req open: SrcText = "open",
        req close: SrcText = "close",
        req items: Vec<NoteItemSource> = "items",
        req refs_at_load: NumSet = "refsAtLoad",
    }
}

/// The source store of one read (`SourceStore`), immutable afterwards.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceStore {
    /// the TS `next` (the next `src` id; the TS starts at 1, so that 0 is "no source")
    pub next: u32,
    pub paras: JsMap<SourceKey, ParaSource>,
    pub runs: JsMap<SourceKey, RunSource>,
    pub tables: JsMap<SourceKey, TableSource>,
    pub trs: JsMap<SourceKey, RowSource>,
    pub tcs: JsMap<SourceKey, CellSource>,
    /// by part path
    pub stories: JsMap<Str, StorySource>,
    /// `txbxGroups` (the textbox contents with groups; `DrawingTextbox::txbx_src` indexes it)
    pub txbx_groups: Vec<Vec<StoryGroup>>,
    pub note_parts: JsMap<Str, NotesPartSource>,
    pub comment_ids: NumSet,
    /// `JSON.stringify(comments)` at read
    pub comments_sig: Option<Str>,
    /// the texts the spans point into
    pub parts: Vec<SourcePart>,
}

impl Default for SourceStore {
    fn default() -> Self {
        SourceStore {
            next: 1,
            paras: JsMap::default(),
            runs: JsMap::default(),
            tables: JsMap::default(),
            trs: JsMap::default(),
            tcs: JsMap::default(),
            stories: JsMap::default(),
            txbx_groups: Vec::new(),
            note_parts: JsMap::default(),
            comment_ids: NumSet::default(),
            comments_sig: None,
            parts: Vec::new(),
        }
    }
}

impl SourceStore {
    /// `id()`: the next source key.
    pub fn id(&mut self) -> SourceKey {
        let k = SourceKey(self.next);
        self.next = self.next.saturating_add(1);
        k
    }
}

/// The digest facet `sources` (A6): the store's maps; the private `paraRefs`/`tableRefs`/`memo`
/// of the TS class are not part of it. Spans need the store's parts in the writer
/// (`CanonWriter::with_parts`).
impl JsValue for SourceStore {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let next = self.next;
        let mut entries: Vec<crate::canon::Entry<'_>> = vec![
            ("next", Box::new(move |w| w.num(f64::from(next)))),
            ("paras", Box::new(|w| self.paras.canon(w))),
            ("runs", Box::new(|w| self.runs.canon(w))),
            ("tables", Box::new(|w| self.tables.canon(w))),
            ("trs", Box::new(|w| self.trs.canon(w))),
            ("tcs", Box::new(|w| self.tcs.canon(w))),
            ("stories", Box::new(|w| self.stories.canon(w))),
            ("txbxGroups", Box::new(|w| self.txbx_groups.canon(w))),
            ("noteParts", Box::new(|w| self.note_parts.canon(w))),
            ("commentIds", Box::new(|w| self.comment_ids.canon(w))),
        ];
        if let Some(sig) = self.comments_sig.as_deref() {
            entries.push(("commentsSig", Box::new(move |w| w.str(sig))));
        }
        crate::canon::canon_boxed(w, entries);
    }
    /// Never stringified by the TS.
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.raw("{}");
        true
    }
}
