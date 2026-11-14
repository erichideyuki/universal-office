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
    /// Wave 7 (B): the flat editing view of the note in edit while it holds a table (the TS note
    /// model is `flattenBlocks(blocks)`, universal-write.ts `enterNote`); uo-docx `session/ops/hf_flat.rs`.
    pub flat: super::section::EditSlot<NoteFlat>,
}

/// The flat editing view of one note (wave 7, B): `footnote`/`id` name the note.
#[derive(Debug, Clone)]
pub struct NoteFlat {
    pub footnote: bool,
    pub id: NumKey,
    pub view: Arc<super::section::HfFlat>,
}

/// Wave 7c (W, item 2): facts of the package the reader saw that the TS model does not keep, for the layout's Word rules.
/// Outside the model's JS shape and equality (two documents with the same model are equal whatever package they came
/// from), like [`super::section::EditSlot`]; never written.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadFacts {
    /// the package has no `word/settings.xml` (Word opens it in compatibility mode 12, measured; the TS model says 15)
    pub settings_absent: bool,
    /// the settings (part or not) have no `w:defaultTabStop` (Word uses 708 tw, measured; the TS model says 720)
    pub default_tab_absent: bool,
    /// WordCraft study (page): `w:settings/w:gutterAtTop` (Word puts the gutter above the top margin and ignores
    /// mirrorMargins and rtlGutter; measured)
    pub gutter_at_top: bool,
    /// WordCraft study (page): `w:bookFoldPrinting` (`Some(false)`) or `w:bookFoldRevPrinting` (`Some(true)`): the margins
    /// Word's PDF applies (measured)
    pub book_fold: Option<bool>,
    /// WordCraft study (page): `w:printTwoOnOne` (measured in Word's PDF)
    pub print_two_on_one: bool,
}

impl PartialEq for ReadFacts {
    fn eq(&self, _: &Self) -> bool {
        true
    }
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
    /// wave 7c (W, item 2): what the reader saw of the package that the model does not keep ([`ReadFacts`])
    pub read_facts: ReadFacts,
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

impl Document {
    /// The compatibility mode Word follows (wave 7c, W, item 2; oracle `nosettings`): 12 for a package without
    /// `word/settings.xml` ([`ReadFacts::settings_absent`]), else the model's `settings.compatibilityMode`. The session's
    /// reads (manual hyphenation minimums, the table style gallery) and the layout (`uo-docx-layout` `LDoc::compat_mode`)
    /// use the same answer; a strict replay of TS captures clears the facts (`OpenOptions::ts_missing_settings`).
    #[must_use]
    pub fn word_compat_mode(&self) -> f64 {
        if self.read_facts.settings_absent { 12.0 } else { self.formatting.settings.compatibility_mode.to_number() }
    }

    /// The default tab stop Word follows while the settings give none (wave 7c, W, item 2): 708 tw
    /// ([`ReadFacts::default_tab_absent`]; the session clears the fact when it writes an explicit value), else `None` (the
    /// model's `settings.defaultTabStop`).
    #[must_use]
    pub fn word_default_tab_fallback(&self) -> Option<f64> {
        self.read_facts.default_tab_absent.then_some(708.0)
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
