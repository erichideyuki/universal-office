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
use crate::write::emit::background::write_page_background;
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
    /// wave 7c (S): the frozen TS's `pendingTableStyles` outside the history (an undone table's style is still written).
    /// Default `false`: Word's (the style goes with the undo; `save_parts.rs`). For the strict replays of TS captures.
    pub ts_table_style_survives_undo: bool,
    /// wave 7c (S): the frozen TS's package reuse between saves (the styles, numbering and settings parts a save wrote stay
    /// after the edit is undone). Default `false`: Word's (`save_parts.rs`). For the strict replays of TS captures.
    pub ts_stale_parts: bool,
    /// wave 7c (W, item 2, fix round 1): the frozen TS's reading of a package without `word/settings.xml` or without
    /// `w:defaultTabStop` (the model's mode 15 and 720 tw): the reader's facts are dropped at the open, so the session
    /// (`getDefaultTabStop`, the manual hyphenation minimums, the table style gallery) and the layout answer the TS's
    /// values. Default `false`: Word's (mode 12, 708 tw, measured: oracle `nosettings`). For the strict replays of TS captures.
    pub ts_missing_settings: bool,
    /// WordCraft study (fields, B25): the frozen TS's table-formula texts ("!Operador ausente", "!Erro de sintaxe" without
    /// the token, the bookmark without its name) and ROUND without the decimal correction. Default `false`: Word's
    /// (measured: "!Está faltando operador", "!Erro de sintaxe, <token>", "!Indicador não definido, <NAME>", ROUND(2,345;2)
    /// = 2,35). For the strict replays of TS captures.
    pub ts_formula_errors: bool,
    /// WordCraft study (fields, B26/B27): the frozen TS's list words (pt-BR up to 99) and `w:isLgl` in the session's
    /// outline, TOC sources and cross-reference items. Default `false`: Word's. For the strict replays of TS captures.
    pub ts_numbering: bool,
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
            ts_table_style_survives_undo: false,
            ts_stale_parts: false,
            ts_missing_settings: false,
            ts_formula_errors: false,
            ts_numbering: false,
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

    /// `loadOdt(bytes, opts)` (universal-write.ts 2275; wave 7, A1): an `.odt` read by the ODF reader over the empty
    /// package (`DocxPackage.empty()`: the TS `pkg = null`, saved from scratch), no styles or numbering part
    /// (`new DocStyles(null)`, `resetNumbering(null)`), tracking off.
    #[cfg(feature = "odf")]
    pub fn open_odt(bytes: &[u8], o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError> {
        let doc = crate::odf::reader::read_odt(bytes, &o.limits)?;
        let pkg = crate::read::blank::empty_docx_package();
        let mut s = Session::from_parts(pkg, doc, VersionStore::new(None), VersionStore::new(None), false, o, env)?;
        // `this.loadedFormat = 'odt'` (2248): the sheet is the ODF's, read-only (`stylesEditable()` 4582)
        s.loaded_odt = true;
        // `this.pkg = null` (2240; wave 7c, S): each save starts from an empty package
        s.pkg_scratch = true;
        Ok(s)
    }

    /// `loadedFormat` (universal-write.ts 2129; wave 7, A1): `"odt"` after [`Session::open_odt`], else `"docx"`
    /// (`loadDocx`/`newDocument` reset it; each is a new session here).
    #[must_use]
    pub fn loaded_format(&self) -> &'static str {
        if self.loaded_odt { "odt" } else { "docx" }
    }

    /// `stylesEditable()` (universal-write.ts 4582): the `.odt` has no editable style sheet.
    #[must_use]
    pub fn styles_editable(&self) -> bool {
        !self.loaded_odt
    }

    fn from_package(pkg: Package, o: OpenOptions, env: Box<dyn Env>) -> Result<Session, OpenError> {
        let doc = read_document(&pkg, &ReadOptions { sources: true, limits: o.limits.clone() })?;
        let styles_path = part_by_rel_type(&pkg, "styles", "word/styles.xml")?;
        let styles = VersionStore::new(styles_path.as_deref().and_then(|p| part_text(&pkg, p)));
        let numbering_path = part_by_rel_type(&pkg, "numbering", "word/numbering.xml")?;
        let numbering = VersionStore::new(numbering_path.as_deref().and_then(|p| part_text(&pkg, p)));
        let settings_path = part_by_rel_type(&pkg, "settings", "word/settings.xml")?;
        let tracking = settings_path.as_deref().and_then(|p| part_text(&pkg, p)).is_some_and(|x| track_revisions_on(&x));
        Session::from_parts(pkg, doc, styles, numbering, tracking, o, env)
    }

    /// The session over a read document and its package (`from_package`, and `open_odt` since wave 7).
    fn from_parts(
        pkg: Package,
        doc: Document,
        styles: VersionStore,
        numbering: VersionStore,
        tracking: bool,
        o: OpenOptions,
        env: Box<dyn Env>,
    ) -> Result<Session, OpenError> {
        let mut opts = o.session.clone();
        opts.tracking = tracking;
        opts.protected = o.protected;
        opts.ts_formula_errors |= o.ts_formula_errors; // WordCraft study (fields, B25)
        opts.ts_numbering |= o.ts_numbering; // WordCraft study (fields, B26/B27)
        let settings_path = part_by_rel_type(&pkg, "settings", "word/settings.xml")?;
        let loaded_settings = settings_path.and_then(|p| part_text(&pkg, &p).filter(|t| !t.is_empty()).map(|t| (p, t)));
        let numbering_path = part_by_rel_type(&pkg, "numbering", "word/numbering.xml")?.filter(|p| pkg.contains(p));
        let save_parts = super::save_parts::SaveParts {
            ts_stale_parts: o.ts_stale_parts,
            ts_table_style_survives_undo: o.ts_table_style_survives_undo,
            loaded_settings,
            numbering_written: false,
            numbering_path,
            table_styles: None,
            note_parts: false,
            display_background: false,
            note_styles: Vec::new(),
        };
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
            f9: super::ops::navigation::F9State::default(),
            f10: super::ops::references::F10State::default(),
            f5: super::ops::notes::F5State::default(),
            f4: super::ops::sections::F4State::default(),
            f7: super::ops::drawing::F7State::default(),
            f8: super::ops::clipboard::F8State::default(),
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
            flat_box: None,
            policy: None,
            session_id: o.session_id,
            generation: 0,
            quiet: false,
            tx: Tx::default(),
            loaded_odt: false,
            pkg_scratch: false,
            pending_tables: Arc::default(),
            save_parts,
        };
        let mut doc = doc;
        if o.ts_missing_settings {
            doc.read_facts = crate::model::ReadFacts::default();
        }
        s.install(doc, o.sig_mode);
        // the style index of the lists/styles family (D2b-F2), so that Enter's `applyNextStyle` and the
        // list hooks never parse `styles.xml` inside a keystroke
        s.warm_style_index();
        Ok(s)
    }

    /// `installFullDoc()` (see the module comment).
    fn install(&mut self, mut doc: Document, mode: SigMode) {
        self.generation = self.generation.wrapping_add(1);
        // `this.lastPaste = null` with the new `docToken` (D2b-F8; the paste defaults and the
        // document path are the element's, they survive a load)
        self.f8.reset_for_install();
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
        self.sync_bookmarks(); // wave 7 (B, gate-10): the offsets of the marked note runs (engine-doc.ts)
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
        self.f5 = super::ops::notes::F5State::install(&self.doc);
        self.f4 = super::ops::sections::F4State::install(&self.doc);
        self.tx = Tx::default();
    }

    fn doc_view_for_authors(&self) -> crate::view::DocView<'_> {
        crate::view::DocView::new(&self.doc)
    }

    /// `saveDocx()` (`saveDocxBody`): writes the document into the session's package (the same
    /// package at every save, as the TS) and returns its bytes. An `.odt` without a package of its own writes into a
    /// fresh empty one each time (`this.pkg ?? DocxPackage.empty()`, 8552; wave 7c, S).
    pub fn save_docx(&mut self) -> Result<Vec<u8>, DocxError> {
        if !self.pkg_scratch {
            return self.save_docx_into();
        }
        let keep = std::mem::replace(&mut self.pkg, crate::read::blank::empty_docx_package());
        let bg = self.f4.bg_written.take();
        let out = self.save_docx_into();
        self.pkg = keep;
        self.f4.bg_written = bg;
        out
    }

    /// Wave 7c (S): the frozen TS's save behaviour for the strict replays of TS captures on an open session
    /// (`OpenOptions::ts_stale_parts`, `OpenOptions::ts_table_style_survives_undo`; the wasm self-test sets them through
    /// `uo_session_test_ts_save_compat`, the production open has no field for them).
    pub fn set_ts_save_compat(&mut self, stale_parts: bool, table_style_survives_undo: bool) {
        self.save_parts.ts_stale_parts = stale_parts;
        self.save_parts.ts_table_style_survives_undo = table_style_survives_undo;
    }

    /// WordCraft study (fields, B25–B27): the frozen TS's table-formula texts and list rules on an open session
    /// (`OpenOptions::ts_formula_errors`, `OpenOptions::ts_numbering`), for the strict replays of TS captures; the wasm
    /// self-test sets them through `uo_session_test_ts_wc_fields`, the production open has no field for them.
    pub fn set_ts_wc_fields(&mut self, formula_errors: bool, numbering: bool) {
        self.opts.ts_formula_errors = formula_errors;
        self.opts.ts_numbering = numbering;
    }

    /// Whether the session's package is the `.odt`'s scratch one (the TS `this.pkg === null`; wave 7c, S).
    #[must_use]
    pub fn package_is_scratch(&self) -> bool {
        self.pkg_scratch
    }

    /// `this.pkg ?? (this.pkg = DocxPackage.empty())` (the watermark 11877, the cover page 12041; wave 7c, S): the
    /// `.odt`'s scratch package becomes the document's (a fresh empty one, as the TS makes).
    pub(crate) fn own_package(&mut self) {
        if self.pkg_scratch {
            self.pkg = crate::read::blank::empty_docx_package();
            self.pkg_scratch = false;
        }
    }

    fn save_docx_into(&mut self) -> Result<Vec<u8>, DocxError> {
        // a fresh package (the `.odt`'s) has nothing of an earlier save to take back
        let stale = self.save_parts.ts_stale_parts || self.pkg_scratch;
        // wave 7c (S; Word manda sobre o TS): what an earlier save wrote outside the writer goes back first, so the saved
        // file holds what the document has now (an undone edit's part is not in it)
        if !stale {
            super::save_parts::settings_back(&mut self.pkg, self.save_parts.loaded_settings.as_ref())?;
            if self.save_parts.note_parts {
                // the separators' ids the note commands put in `settings.xml` (idempotent: the parts exist already)
                crate::write::emit::notes::ensure_note_parts(&mut self.pkg)?;
            }
            if self.save_parts.display_background {
                write_display_background_shape(&mut self.pkg, true)?;
            }
            if let Some(a) = self.save_parts.table_styles.take() {
                super::save_parts::take_back_styles(&mut self.pkg, &a)?;
            }
            if self.save_parts.numbering_written && !(self.numbering.dirty() && self.numbering.xml().is_some()) {
                let path = self.save_parts.numbering_path.clone();
                let loaded = self.numbering.base_text().map(|t| t.to_string());
                super::save_parts::numbering_back(&mut self.pkg, path.as_deref(), loaded.as_deref())?;
                self.save_parts.numbering_written = false;
            }
        }
        // `syncSections()` (the body context only; in a context the body paragraphs are untouched)
        // (`saveDocx` leaves a header/footer edit first and returns to it: the body is in context
        // while it saves, D2b-F4)
        if matches!(self.ctx, super::EditContext::Body | super::EditContext::HeaderFooter { .. }) {
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
            if !stale {
                self.save_parts.numbering_written = true;
            }
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
        // the page colour: only if it changed since the open (the file's `w:background` stays byte for
        // byte); the package is the same between saves, so what the last save added leaves first
        if super::ops::page::background_differs(self.doc.background.as_ref(), self.f4.loaded.background.as_ref())
            || self.f4.bg_written.is_some()
        {
            let prev = self.f4.bg_written.take();
            self.f4.bg_written = write_page_background(&mut self.pkg, &self.doc.media, self.doc.background.as_deref(), prev.as_ref())?;
            if self.doc.background.is_some() {
                write_display_background_shape(&mut self.pkg, true)?;
                if !stale {
                    self.save_parts.display_background = true;
                }
            }
        }
        // parts no section refers to any more leave the package (Word, oracle e07)
        let used: HashSet<&str> =
            self.doc.sections.iter().flat_map(|s| s.header_refs.iter().chain(s.footer_refs.iter()).map(|r| r.rid.as_ref())).collect();
        let gone: HashSet<String> =
            self.sections.hf_rids_at_load.iter().filter(|r| !used.contains(r.as_ref())).map(|r| r.to_string()).collect();
        if !gone.is_empty() {
            drop_unused_hf_parts(&mut self.pkg, &gone)?;
        }
        // relationships copied for unlinked parts that were not written leave the package
        let copied: Vec<String> = self.f4.hf_rels_copied.iter().cloned().collect();
        for r in copied {
            let part = format!("word/{}", r.rsplit('/').next().unwrap_or(&r).trim_end_matches(".rels"));
            if !self.pkg.contains(&part) {
                self.pkg.remove_part(&r);
            }
        }
        if let Some(ns) = &settings.note_settings
            && ns.json() != self.f5.loaded_json()
        {
            // `patchSettingsNotes`, only when the options differ from the file's (D2b-F5)
            super::ops::notes::save_note_settings(&mut self.pkg, ns)?;
        }
        if let Some(xml) = self.styles.xml() {
            // wave 7c (S, fix round 1; Word manda sobre o TS): the note styles a note command defined stay in the file
            // after the command is undone, whether or not a save came in between (measured; `SaveParts::note_styles`)
            let notes = (!stale).then_some(&self.save_parts.note_styles).filter(|n| !n.is_empty());
            if self.styles.dirty() {
                // `writeStylesPart(pkg, docStyles.xml)` (D2b-F2): only when a style changed
                match notes {
                    Some(n) => super::ops::styles::write_styles_part(&mut self.pkg, &super::save_parts::put_missing_styles(xml, n))?,
                    None => super::ops::styles::write_styles_part(&mut self.pkg, xml)?,
                }
            } else if !stale && let Some(held) = super::ops::styles::styles_part_text(&self.pkg)? {
                // wave 7c (S, Word manda sobre o TS): the package may hold an earlier save's text of an edit undone since
                // (the TS leaves it): the sheet of now, with the note styles that save had and the ones a note command
                // defined (Word keeps them, measured)
                let mut out = if held == **xml { held.clone() } else { super::save_parts::keep_note_styles(&held, xml) };
                if let Some(n) = notes {
                    out = super::save_parts::put_missing_styles(&out, n);
                }
                if out != held {
                    super::ops::styles::write_styles_part(&mut self.pkg, &out)?;
                }
            }
        } else if !stale && !self.save_parts.note_styles.is_empty() {
            // no style sheet now (a document without a styles part, the note insertion undone): the part with the note
            // styles (idempotent: `addStylesXml` appends only what the part lacks)
            let defs: indexmap::IndexMap<String, String> = self.save_parts.note_styles.iter().cloned().collect();
            super::save_parts::add_styles_xml(&mut self.pkg, &defs)?;
        }
        if let Some(t) = settings.default_tab {
            write_default_tab_stop(&mut self.pkg, t)?;
        }
        if let Some(h) = &settings.hyphenation {
            write_hyphenation_settings(&mut self.pkg, h)?;
        }
        // `addStylesXml(pkg, this.pendingTableStyles)` (8641; wave 7c, S): last, as the TS
        let added = super::save_parts::add_styles_xml(&mut self.pkg, &self.pending_tables)?;
        if !stale {
            self.save_parts.table_styles = added;
        }
        Ok(self.pkg.save(&SaveOptions::default())?)
    }
}
