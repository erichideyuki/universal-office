//! The document (architecture A2): the TS `FullDocument` of `readFullDocument`, each story stored
//! once. Read by frontier R5 (`fulldoc.ts`).

use std::sync::Arc;

use crate::ids::IdAlloc;
use crate::js::{JsMap, JsNumber, JsValue, JsonWriter, NumKey, Str};
use crate::js_object;
use crate::pvec::PVec;
use crate::text::Utf16;

use super::background::PageBackground;
use super::formatting::{DocFormatting, StylesMap};
use super::media::MediaStore;
use super::section::{HfPart, Section};
use super::sources::SourceStore;
use super::story::Story;

js_object! {
    /// A comment (`CommentInfo`): the reader writes `author` even when `undefined`.
    pub struct Comment {
        req id: JsNumber = "id",
        opt author: Str = "author",
        /// the paragraphs' run texts, joined with `\n`
        req text: Utf16 = "text",
    }
}

js_object! {
    /// Where a bookmark ends (`bookmarkEnds` values).
    pub struct BookmarkEnd {
        req block: JsNumber = "block",
        req offset: JsNumber = "offset",
    }
}

/// The start of a bookmark: block index and offset in the paragraph's text.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BookmarkStart {
    pub block: JsNumber,
    pub offset: JsNumber,
}

/// The bookmarks of the body (`bookmarks` + `bookmarkOffsets`, same keys and order; and
/// `bookmarkEnds`, its own order).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bookmarks {
    pub starts: JsMap<Str, BookmarkStart>,
    pub ends: JsMap<Str, BookmarkEnd>,
}

js_object! {
    /// `noteSpecialIds` (separators and continuation notices).
    pub struct NoteSpecialIds {
        req footnote: Vec<JsNumber> = "footnote",
        req endnote: Vec<JsNumber> = "endnote",
    }
}

/// Footnotes and endnotes: one story per note id, in TS `Map` order (`footnoteBlocks`; the TS
/// `footnotes` map is the view of their top-level paragraphs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    pub footnotes: JsMap<NumKey, Story>,
    pub endnotes: JsMap<NumKey, Story>,
    pub special_ids: NoteSpecialIds,
}

js_object! {
    /// `docProps/core.xml` dates (`coreProps`).
    pub struct CoreProps {
        opt created: Str = "created",
        opt modified: Str = "modified",
        opt last_printed: Str = "lastPrinted",
    }
}

/// The document (see the module comment).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// the body (`blocks`)
    pub body: Story,
    pub sections: Arc<[Section]>,
    /// the header/footer parts, by [`crate::ids::HfPartId`]
    pub hf: PVec<HfPart>,
    pub notes: Arc<Notes>,
    pub comments: Arc<[Comment]>,
    /// the resolved styles (`styles`, `readStyles`)
    pub styles: Arc<StylesMap>,
    pub bookmarks: Arc<Bookmarks>,
    pub formatting: Arc<DocFormatting>,
    pub core_props: Option<CoreProps>,
    pub background: Option<Arc<PageBackground>>,
    pub media: MediaStore,
    pub sources: Option<Arc<SourceStore>>,
    pub ids: IdAlloc,
    /// bumps on any style, numbering, settings, font table, theme or section change (A4; the
    /// session's; 0 after the read)
    pub formatting_version: u64,
}

impl JsValue for BookmarkStart {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.block.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        self.block.json(w)
    }
}

#[cfg(feature = "canon")]
impl Document {
    /// The canonical stream of the document (A6), the TS `FullDocument` minus `sources` (its own
    /// facet, [`Document::canon_sources`]). The writer needs the document's media
    /// (`CanonWriter::with_media`).
    pub fn canon_full(&self, w: &mut crate::canon::CanonWriter<'_>) {
        use crate::canon::CanonWriter;
        let sections = |w: &mut CanonWriter<'_>| {
            w.array(self.sections.len());
            for s in self.sections.iter() {
                super::section::SectionCanon { section: s, parts: &self.hf }.canon(w);
            }
        };
        let note_map = |m: &JsMap<NumKey, Story>, w: &mut CanonWriter<'_>| {
            w.map(m.len());
            for (k, v) in m {
                k.canon(w);
                v.canon(w);
            }
        };
        let starts = &self.bookmarks.starts;
        // the TS key order of `readFullDocument`'s literal (the order facet); `@views` last
        let mut entries: Vec<crate::canon::Entry<'_>> = vec![
            ("blocks", Box::new(|w| self.body.canon(w))),
            ("sections", Box::new(sections)),
            ("comments", Box::new(|w| self.comments.canon(w))),
            ("styles", Box::new(|w| self.styles.canon(w))),
            (
                "bookmarks",
                Box::new(move |w| {
                    w.map(starts.len());
                    for (k, v) in starts {
                        w.str(k);
                        v.block.canon(w);
                    }
                }),
            ),
            (
                "bookmarkOffsets",
                Box::new(move |w| {
                    w.map(starts.len());
                    for (k, v) in starts {
                        w.str(k);
                        v.offset.canon(w);
                    }
                }),
            ),
            ("bookmarkEnds", Box::new(|w| self.bookmarks.ends.canon(w))),
            ("formatting", Box::new(|w| self.formatting.canon(w))),
            ("footnoteBlocks", Box::new(move |w| note_map(&self.notes.footnotes, w))),
            ("endnoteBlocks", Box::new(move |w| note_map(&self.notes.endnotes, w))),
            ("noteSpecialIds", Box::new(|w| self.notes.special_ids.canon(w))),
        ];
        if let Some(c) = &self.core_props {
            entries.push(("coreProps", Box::new(move |w| c.canon(w))));
        }
        if let Some(b) = &self.background {
            entries.push(("background", Box::new(move |w| b.canon(w))));
        }
        entries.push(("@views", Box::new(|w| w.str("synced"))));
        crate::canon::canon_boxed(w, entries);
    }

    /// The `sources` facet (A6): the source store, or `undefined` without one. The writer needs
    /// the store's parts (`CanonWriter::with_parts`).
    pub fn canon_sources(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match &self.sources {
            Some(s) => s.canon(w),
            None => w.undefined(),
        }
    }
}
