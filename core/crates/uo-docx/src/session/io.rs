//! Open, install and save (D2a): `loadDocx` + `installFullDoc` (universal-write.ts 2156, 2340)
//! and `saveDocx`/`saveDocxBody` (8530, 8546) over the D1b writer (`write_document`, with the
//! session's [`SaveState`]).
//!
//! Install, in TS order: `syncRunRPr` (body, tables, every section's header/footer slots) →
//! `markSectionBreaks` → `resetSections` + section ids → the DATE/PAGE placeholders → the
//! note-run marks (body, tables, every note, with `syncRunRPr`) → `syncSections` → selection,
//! history and dirty reset → the lossless baseline.

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use uo_opc::Package;
use uo_opc::package::{OpenOptions as PkgOpenOptions, SaveOptions};

use super::history::{History, HistoryLimits};
use super::settle::{self, SectionState};
use super::state::{Selection, SettingsStep};
use super::versions::VersionStore;
use super::{Env, Session, SessionOptions, Tx};
use crate::error::DocxError;
use crate::js::{JsMap, NumKey, Str};
use crate::limits::DocxLimits;
use crate::model::Document;
use crate::model::para::Paragraph;
use crate::model::story::{Block, Story};
use crate::read::blank::{BlankDocxOptions, blank_docx_package};
use crate::read::fulldoc::note_ref_ids;
use crate::read::{ReadOptions, read_document};
use crate::write::ctx::SaveState;
use crate::write::emit::settings::{
    drop_unused_hf_parts, write_default_tab_stop, write_display_background_shape, write_even_and_odd_headers, write_hyphenation_settings,
};
use crate::write::lossless::HfRefXml;
use crate::write::parts::{HfPartOut, NotesOut, WriteOptions, part_by_rel_type, write_document};
use crate::write::rev::track_revisions_on;
use crate::write::sig::SigMode;

/// How a document is opened.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenOptions {
    pub limits: DocxLimits,
    /// `{ protected }` (Modo Protegido; the TS default for `loadDocx` is `true`, the host APIs pass
    /// `false`)
    pub protected: bool,
    pub session: SessionOptions,
    pub history: HistoryLimits,
    /// the signature mode of the lossless baseline and save: `Ts` (order-sensitive, the TS
    /// `runSig`), the D2a-U6 decision: the session harness found saved bytes that differ under
    /// the canonical signatures (G23's condition), and none under `Ts` with the run key orders the
    /// TS ops build (record "D2a — session (architecture)", U6 count)
    pub sig_mode: SigMode,
    /// the clipboard identity of the session (G7)
    pub session_id: u32,
    /// the language of a new document (`newDocument`, `spellcheck-default-lang`)
    pub lang: Option<Str>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            limits: DocxLimits::default(),
            protected: false,
            session: SessionOptions::default(),
            history: HistoryLimits::default(),
            sig_mode: SigMode::Ts,
            session_id: 0,
            lang: None,
        }
    }
}

/// Why a document did not open.
#[derive(Debug)]
pub enum OpenError {
    Docx(DocxError),
    /// the `.doc` converter refused the file
    #[cfg(feature = "doc")]
    Doc(crate::doc::DocError),
}

impl From<DocxError> for OpenError {
    fn from(e: DocxError) -> Self {
        OpenError::Docx(e)
    }
}

impl From<uo_opc::Error> for OpenError {
    fn from(e: uo_opc::Error) -> Self {
        OpenError::Docx(DocxError::Package(e))
    }
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Docx(e) => write!(f, "{e}"),
            #[cfg(feature = "doc")]
            OpenError::Doc(e) => write!(f, "{e:?}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// The text of a part (`getPartText`), `None` when absent or unreadable.
fn part_text(pkg: &Package, name: &str) -> Option<Arc<str>> {
    pkg.part_text(name).ok().flatten().map(|t| Arc::from(&*t))
}

impl Session {
    /// `loadDocx(bytes, opts)`: a `.docx` (or, with the `doc` feature, a `.doc` converted first).
    pub fn open(bytes: Vec<u8>, o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError> {
        #[cfg(feature = "doc")]
        let bytes = if crate::doc::is_word_binary(&bytes) {
            crate::doc::convert_doc_to_docx(&bytes, &crate::doc::DocLimits::default()).map_err(OpenError::Doc)?
        } else {
            bytes
        };
        let pkg = Package::open_owned(bytes, &o.limits.package, &PkgOpenOptions::default())?;
        Session::from_package(pkg, o, env)
    }

    /// `newDocument()`: the blank package of `blank.ts` in the given language.
    pub fn new_blank(o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError> {
        let pkg = blank_docx_package(&BlankDocxOptions { lang: o.lang.as_deref() });
        Session::from_package(pkg, o, env)
    }

    fn from_package(pkg: Package, o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError> {
        let doc = read_document(&pkg, &ReadOptions { sources: true, limits: o.limits.clone() })?;
        let styles_path = part_by_rel_type(&pkg, "styles", "word/styles.xml")?;
        let styles = VersionStore::new(styles_path.as_deref().and_then(|p| part_text(&pkg, p)));
        let numbering_path = part_by_rel_type(&pkg, "numbering", "word/numbering.xml")?;
        let numbering = VersionStore::new(numbering_path.as_deref().and_then(|p| part_text(&pkg, p)));
        let settings_path = part_by_rel_type(&pkg, "settings", "word/settings.xml")?;
        let tracking = settings_path.as_deref().and_then(|p| part_text(&pkg, p)).is_some_and(|x| track_revisions_on(&x));
        let mut opts = o.session.clone();
        opts.tracking = tracking;
        opts.protected = o.protected;
        let mut s = Session {
            pkg,
            doc: Document::default(),
            ctx: super::EditContext::Body,
            selection: Selection::default(),
            table_sel: None,
            float_sel: None,
            bookmark_paras: super::state::BookmarkParas::default(),
            styles,
            numbering,
            f2: super::ops::lists::F2State::default(),
            settings: Arc::new(SettingsStep::default()),
            sections: SectionState::default(),
            save: SaveState { baseline: crate::write::sig::Baseline::default(), gen_num_ids: None, fresh_id: 0.0 },
            history: History::new(o.history),
            env,
            opts,
            rev_authors: Vec::new(),
            next_bm_id: -1.0,
            pending_props: None,
            pending: None,
            next_job: 0,
            dirty_hf: HashSet::new(),
            policy: None,
            session_id: o.session_id,
            generation: 0,
            quiet: false,
            tx: Tx::default(),
        };
        s.install(doc, o.sig_mode);
        // the style index of the lists/styles family (D2b-F2), so that Enter's `applyNextStyle` and the
        // list hooks never parse `styles.xml` inside a keystroke
        s.warm_style_index();
        Ok(s)
    }

    /// `installFullDoc()` (see the module comment).
    fn install(&mut self, mut doc: Document, mode: SigMode) {
        self.generation = self.generation.wrapping_add(1);
        // bookmarks: block → paragraph (`paraOfBlock`), before the empty paragraph is added
        let mut para_of_block: Vec<u32> = Vec::with_capacity(doc.body.block_count());
        let mut n = 0u32;
        for b in doc.body.blocks() {
            para_of_block.push(n);
            if matches!(b, Block::Paragraph(_)) {
                n = n.saturating_add(1);
            }
        }
        let empty = doc.body.paras.is_empty();
        let n_paras = if empty { 1 } else { u32::try_from(doc.body.paras.len()).unwrap_or(u32::MAX) };
        let mut bm: JsMap<Str, u32> = JsMap::new();
        for (name, start) in &doc.bookmarks.starts {
            // `Math.min(paraOfBlock[b] ?? paragraphs.length - 1, paragraphs.length - 1)`
            let at = match start.block {
                crate::js::JsNumber::Num(b) if b >= 0.0 && b.fract() == 0.0 && b < 4_294_967_295.0 => {
                    para_of_block.get(uo_jsnum::conv::to_uint32(b) as usize).copied()
                }
                _ => None,
            };
            bm.insert(name.clone(), at.unwrap_or(n_paras - 1).min(n_paras - 1));
        }
        drop(para_of_block);
        // syncRunRPr: body paragraphs and tables, every section's header/footer slots
        settle::sync_story(&mut doc.body, true);
        let mut seen: Vec<crate::ids::HfPartId> = Vec::new();
        let sections = Arc::clone(&doc.sections);
        for s in sections.iter() {
            for (_, part) in s.headers.iter().chain(s.footers.iter()) {
                if !seen.contains(part) {
                    seen.push(*part);
                    if let Some(h) = doc.hf.make_mut(part.index()) {
                        settle::sync_paras(&mut h.story.paras);
                    }
                }
            }
        }
        crate::write::roundtrip::mark_section_breaks(&mut doc);
        let mut sections = SectionState::install(&mut doc);
        // the DATE/PAGE placeholders of the top-level body paragraphs
        let today = self.env.now().local.pt_br_date();
        settle::field_placeholders(&mut doc.body.paras, &today);
        // the note runs take one character; the notes' own runs get their rPr links
        settle::mark_note_runs(&mut doc.body, true);
        let notes = Arc::make_mut(&mut doc.notes);
        for map in [&mut notes.footnotes, &mut notes.endnotes] {
            for story in map.values_mut() {
                settle::mark_note_runs(story, true);
                settle::sync_story(story, true);
            }
        }
        if empty {
            doc.body.paras.push(Paragraph { id: doc.ids.para(), ..Paragraph::default() });
        }
        sections.sync(&mut doc);
        self.sections = sections;
        self.doc = doc;
        self.bookmark_paras = super::state::BookmarkParas::new(bm);
        self.ctx = super::EditContext::Body;
        self.selection = Selection::default();
        self.table_sel = None;
        self.float_sel = None;
        self.pending_props = None;
        self.pending = None;
        self.dirty_hf.clear();
        self.settings = Arc::new(SettingsStep::default());
        let _ = self.history.reset();
        self.rev_authors = super::revisions::revision_authors(self.doc_view_for_authors()).into_iter().map(Into::into).collect();
        self.save = SaveState::install(&self.doc, mode);
        self.tx = Tx::default();
    }

    fn doc_view_for_authors(&self) -> crate::view::DocView<'_> {
        crate::view::DocView::new(&self.doc)
    }

    /// `saveDocx()` (`saveDocxBody`): writes the document into the session's package (the same
    /// package at every save, as the TS) and returns its bytes.
    pub fn save_docx(&mut self) -> Result<Vec<u8>, DocxError> {
        // `syncSections()` (the body context only; in a context the body paragraphs are untouched)
        if matches!(self.ctx, super::EditContext::Body) {
            self.sections.sync(&mut self.doc);
        }
        let doc = &self.doc;
        let mut hf: Vec<HfPartOut<'_>> = Vec::new();
        let mut seen_path: Vec<&str> = Vec::new();
        for s in doc.sections.iter() {
            for (kind, refs, slots) in [("header", &s.header_refs, &s.headers), ("footer", &s.footer_refs, &s.footers)] {
                for r in refs {
                    if seen_path.contains(&r.path.as_ref()) {
                        continue;
                    }
                    let Some(&part) = slots.get(&r.r#type) else { continue };
                    let Some(h) = doc.hf.get(part.index()) else { continue };
                    seen_path.push(&r.path);
                    hf.push(HfPartOut {
                        kind: kind.into(),
                        r#type: r.r#type.clone(),
                        rid: r.rid.clone(),
                        path: r.path.clone(),
                        story: &h.story,
                        dirty: self.dirty_hf.contains(&part),
                    });
                }
            }
        }
        let last = doc.sections.last();
        let body_parts: Vec<HfRefXml> = match last {
            Some(l) => {
                let paths: Vec<&str> = l.header_refs.iter().chain(l.footer_refs.iter()).map(|r| r.path.as_ref()).collect();
                hf.iter()
                    .filter(|h| paths.contains(&h.path.as_ref()))
                    .map(|h| HfRefXml { kind: h.kind.clone(), r#type: h.r#type.clone(), rid: h.rid.clone() })
                    .collect()
            }
            None => hf.iter().map(|h| HfRefXml { kind: h.kind.clone(), r#type: h.r#type.clone(), rid: h.rid.clone() }).collect(),
        };
        if self.numbering.dirty() && self.numbering.xml().is_some() {
            // `writeNumberingPart(pkg, numberingOut(blocks, hfParts))` (D2b-F2): only when the
            // editor changed the part, before the body (the generated lists join it)
            super::ops::lists::save_numbering(&mut self.pkg, &self.numbering, &self.f2, doc, &hf)?;
        }
        let notes_out = |kind: &str| -> NotesOut<'_> {
            let used = note_ref_ids(&doc.body, kind);
            let map = if kind == "footnote" { &doc.notes.footnotes } else { &doc.notes.endnotes };
            let mut blocks: IndexMap<NumKey, &Story> = IndexMap::new();
            for id in &used {
                if let Some(s) = map.get(id) {
                    blocks.insert(*id, s);
                }
            }
            NotesOut { blocks, used: used.iter().map(|k| k.get()).collect() }
        };
        let opts = WriteOptions {
            comments: (!doc.comments.is_empty()).then_some(&*doc.comments),
            body_sect_refs: Some(body_parts),
            track_revisions: Some(self.opts.tracking),
            footnotes: Some(notes_out("footnote")),
            endnotes: Some(notes_out("endnote")),
            notes: true,
            blocks: !doc.body.tables.is_empty() || doc.sources.is_some(),
        };
        let sect_pr = last.map_or_else(|| Arc::clone(&self.sections.body_sect_pr), |l| Arc::clone(&l.sect_pr));
        write_document(&mut self.pkg, doc, &doc.body, &sect_pr, &hf, &opts, &mut self.save)?;
        let settings = Arc::clone(&self.settings);
        if let Some(on) = settings.even_odd {
            write_even_and_odd_headers(&mut self.pkg, on)?;
        }
        if settings.background.is_some() {
            // `writePageBackground`: the page family's (F4); no D2a command changes it
            return Err(DocxError::Write("page background: frontier D2b-F4"));
        }
        if let Some(on) = settings.display_background {
            write_display_background_shape(&mut self.pkg, on)?;
        }
        // parts no section refers to any more leave the package (Word, oracle e07)
        let used: HashSet<&str> =
            self.doc.sections.iter().flat_map(|s| s.header_refs.iter().chain(s.footer_refs.iter()).map(|r| r.rid.as_ref())).collect();
        let gone: HashSet<String> =
            self.sections.hf_rids_at_load.iter().filter(|r| !used.contains(r.as_ref())).map(|r| r.to_string()).collect();
        if !gone.is_empty() {
            drop_unused_hf_parts(&mut self.pkg, &gone)?;
        }
        if settings.note_settings.is_some() {
            // `patchSettingsNotes`: the notes family's (F5)
            return Err(DocxError::Write("note settings: frontier D2b-F5"));
        }
        if self.styles.dirty()
            && let Some(xml) = self.styles.xml()
        {
            // `writeStylesPart(pkg, docStyles.xml)` (D2b-F2): only when a style changed
            super::ops::styles::write_styles_part(&mut self.pkg, xml)?;
        }
        if let Some(t) = settings.default_tab {
            write_default_tab_stop(&mut self.pkg, t)?;
        }
        if let Some(h) = &settings.hyphenation {
            write_hyphenation_settings(&mut self.pkg, h)?;
        }
        Ok(self.pkg.save(&SaveOptions::default())?)
    }
}
