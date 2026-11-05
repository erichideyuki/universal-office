//! `DocReader` → the `.docx` package (`docx-writer.ts`): `document.xml`, `styles.xml`,
//! `numbering.xml`, `fontTable.xml`, `settings.xml`, headers and footers, notes, comments and
//! media. The text of every story (main, headers, notes, text boxes) goes through the same
//! paragraph and table renderer.
//!
//! Files: this one (the package, the relationships, `fontTable.xml`), `body` (paragraphs, runs,
//! fields), `tables`, `stories` (sections, headers, notes, comments), `drawing` (anchors,
//! `wsp`/`wpg`/`pic`, fills, lines, WordArt, the page background) and `media`.
//!
//! The parts are compared one by one with the TS, so the order of the zip entries, of the
//! `[Content_Types].xml` entries and of the relationships is the order of the TS calls: every
//! function keeps the call order of its source (the style resolver's memo and the revision ids
//! depend on it).

mod body;
mod drawing;
mod fields;
mod media;
mod stories;
mod tables;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use uo_opc::zip::{WriteOptions, write};

use super::fonts::DocFont;
use super::numbering::build_numbering;
use super::ooxml_props::PropCtx;
use super::props::{Chp, ChpKey, KeySet};
use super::reader::{DocReader, Fspa, NoteKind};
use super::settings::build_settings;
use super::styles_xml::{StyleIds, build_styles, style_id_map};
use super::xmlw::{AttrVal, Attrs, XML_DECL, attrs, content_ns, el_of, ns};
use super::{DocError, DocLimitKind, DocLimits};
use crate::text::Utf16;

/// Options of the conversion (`ConvertOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    /// includes a minimal `docProps/app.xml` (default: true)
    pub doc_props: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions { doc_props: true }
    }
}

/// The relationships of a part (`document.xml`, `header1.xml`…).
#[derive(Debug, Default)]
pub(super) struct PartRels {
    items: Vec<String>,
    n: u32,
    by_key: HashMap<(String, Vec<u16>, bool), String>,
}

impl PartRels {
    pub(super) fn new() -> PartRels {
        PartRels::default()
    }

    /// `add`: the id of the relationship (the same for the same type, target and mode).
    pub(super) fn add(&mut self, rel_type: &str, target: &[u16], external: bool) -> String {
        let key = (rel_type.to_owned(), target.to_vec(), external);
        if let Some(hit) = self.by_key.get(&key) {
            return hit.clone();
        }
        self.n += 1;
        let id = format!("rId{}", self.n);
        let full = format!("{}/{}", ns::REL_TYPE, rel_type);
        self.items.push(el_of(
            "Relationship",
            &[
                ("Id", Some(AttrVal::Str(&id))),
                ("Type", Some(AttrVal::Str(&full))),
                ("Target", Some(AttrVal::Units(target))),
                ("TargetMode", external.then_some(AttrVal::Str("External"))),
            ],
            "",
        ));
        self.by_key.insert(key, id.clone());
        id
    }

    /// `add` of a target that is plain ASCII text.
    pub(super) fn add_str(&mut self, rel_type: &str, target: &str, external: bool) -> String {
        let units: Vec<u16> = target.encode_utf16().collect();
        self.add(rel_type, &units, external)
    }

    /// `size`.
    pub(super) fn size(&self) -> usize {
        self.items.len()
    }

    pub(super) fn xml(&self) -> String {
        format!("{XML_DECL}<Relationships xmlns=\"{}\">{}</Relationships>", ns::REL, self.items.concat())
    }
}

/// Which story a part renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Main,
    Hdr,
    Ftn,
    Edn,
    Atn,
    Txbx,
}

/// The state of rendering a part that is not its relationships (`StoryCtx` minus `rels`): the kind
/// of story and whether drawings come from the header drawing.
#[derive(Debug, Clone, Copy)]
pub(super) struct Story {
    pub(super) kind: Kind,
    pub(super) hdr: bool,
}

/// The content of a file of the package.
pub(super) enum FileData<'a> {
    Text(String),
    Bytes(Cow<'a, [u8]>),
}

/// The files of the package in the order they were first set (a JS `Map`: setting a name again
/// keeps its place).
#[derive(Default)]
pub(super) struct Files<'a> {
    items: Vec<(String, FileData<'a>)>,
    index: HashMap<String, usize>,
}

impl<'a> Files<'a> {
    /// The bytes of a file set as bytes.
    pub(super) fn bytes_of(&self, name: &str) -> Option<&[u8]> {
        match &self.items.get(*self.index.get(name)?)?.1 {
            FileData::Bytes(b) => Some(b),
            FileData::Text(_) => None,
        }
    }

    pub(super) fn set(&mut self, name: String, data: FileData<'a>) {
        match self.index.get(&name) {
            Some(&i) => self.items[i].1 = data,
            None => {
                self.index.insert(name.clone(), self.items.len());
                self.items.push((name, data));
            }
        }
    }
}

/// The CHP of a run with the properties it touched (`chpCache` values).
pub(super) struct ChpEntry {
    pub(super) chp: Chp,
    pub(super) touched: KeySet<ChpKey>,
}

/// One note id of the document: the story of the body refers to notes by CP.
pub(super) type NoteIds = HashMap<i64, u32>;

/// The writer: one per conversion (no counter or table outlives it).
pub struct DocxWriter<'a> {
    pub(super) r: DocReader<'a>,
    pub(super) limits: DocLimits,
    pub(super) work_left: u64,
    pub(super) nest: u32,
    pub(super) files: Files<'a>,
    pub(super) overrides: Vec<(String, String)>,
    pub(super) media_ext: Vec<&'static str>,
    /// media already in the package: (extension, length, hash of the bytes) → names; equal bytes are one file
    pub(super) media: HashMap<(&'static str, usize, u64), Vec<String>>,
    pub(super) media_n: u32,
    pub(super) hdr_n: u32,
    pub(super) ftr_n: u32,
    pub(super) doc_pr_id: i64,
    pub(super) style_ids: StyleIds,
    #[expect(clippy::type_complexity, reason = "the TS memo key (style, grpprl spans) as is")]
    pub(super) chp_cache: HashMap<(u16, Vec<(usize, usize)>), Arc<ChpEntry>>,
    pub(super) footnote_ids: NoteIds,
    pub(super) endnote_ids: NoteIds,
    pub(super) comment_by_ref: HashMap<i64, usize>,
    pub(super) comment_starts: HashMap<i64, Vec<usize>>,
    pub(super) comment_ends: HashMap<i64, Vec<usize>>,
    pub(super) bk_starts: HashMap<i64, Vec<(i64, Rc<Utf16>)>>,
    pub(super) bk_ends: HashMap<i64, Vec<i64>>,
    pub(super) fspa_main: HashMap<i64, Fspa>,
    pub(super) fspa_hdr: HashMap<i64, Fspa>,
    pub(super) txbx_main: HashMap<i64, (i64, i64)>,
    pub(super) txbx_hdr: HashMap<i64, (i64, i64)>,
    pub(super) rev_authors: Vec<Rc<Utf16>>,
    /// the next id of a revision mark (`w:ins`/`w:del`): unique in the document
    pub(super) rev_next: i64,
    pub(super) lfo_count: u64,
    /// CP ranges dropped (`_PictureBullets`)
    pub(super) skip_ranges: Vec<(i64, i64)>,
    /// open fields (they cross paragraphs): whether each is still in its instruction
    pub(super) field_stack: Vec<bool>,
    /// where the last `indexOf('\x13')` looked: `(from, found)`, so a paragraph does not scan the rest
    /// of the text again
    pub(super) next_begin: (i64, Option<i64>),
}

const MIME_DEFAULT: &str = "application/octet-stream";

/// `MIME[ext]`.
fn mime_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "pict" => "image/pict",
        _ => MIME_DEFAULT,
    }
}

const OVERRIDE_PREFIX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

impl<'a> DocxWriter<'a> {
    /// The constructor of `DocxWriter`.
    ///
    /// # Errors
    /// [`DocError::Limit`] when the revision authors read pass the string budget.
    pub fn new(reader: DocReader<'a>, limits: &DocLimits) -> Result<DocxWriter<'a>, DocError> {
        let style_ids = style_id_map(&reader.styles.sheet.styles);
        let rev_authors = reader.revision_authors()?;
        let lfo_count = reader.lists.lfos.len() as u64;
        Ok(DocxWriter {
            r: reader,
            limits: *limits,
            work_left: limits.max_work,
            nest: 0,
            files: Files::default(),
            overrides: Vec::new(),
            media_ext: Vec::new(),
            media: HashMap::new(),
            media_n: 0,
            hdr_n: 0,
            ftr_n: 0,
            doc_pr_id: 0,
            style_ids,
            chp_cache: HashMap::new(),
            footnote_ids: HashMap::new(),
            endnote_ids: HashMap::new(),
            comment_by_ref: HashMap::new(),
            comment_starts: HashMap::new(),
            comment_ends: HashMap::new(),
            bk_starts: HashMap::new(),
            bk_ends: HashMap::new(),
            fspa_main: HashMap::new(),
            fspa_hdr: HashMap::new(),
            txbx_main: HashMap::new(),
            txbx_hdr: HashMap::new(),
            rev_authors,
            rev_next: 1,
            lfo_count,
            skip_ranges: Vec::new(),
            field_stack: Vec::new(),
            next_begin: (i64::MAX, None),
        })
    }

    /// Spends `n` units of the work budget of the conversion.
    pub(super) fn charge(&mut self, n: u64) -> Result<(), DocError> {
        match self.work_left.checked_sub(n) {
            Some(left) => {
                self.work_left = left;
                Ok(())
            }
            None => {
                self.work_left = 0;
                Err(DocError::Limit { kind: DocLimitKind::Work, max: self.limits.max_work })
            }
        }
    }

    /// `numIdOf`: the `numId` of an `ilfo`.
    pub(super) fn num_id_of(lfo_count: u64, ilfo: u32) -> i64 {
        if ilfo >= 1 && u64::from(ilfo) <= lfo_count { i64::from(ilfo) } else { 0 }
    }

    /// `build`: the package.
    ///
    /// # Errors
    /// [`DocError::Limit`] past the work or nesting limits, [`DocError::Corrupt`] when the zip cannot
    /// be written.
    pub fn build(mut self, opts: &ConvertOptions) -> Result<Vec<u8>, DocError> {
        for f in self.r.fspas(false) {
            self.fspa_main.insert(f.cp, f);
        }
        for f in self.r.fspas(true) {
            self.fspa_hdr.insert(f.cp, f);
        }
        for t in self.r.textboxes(false) {
            self.txbx_main.insert(i64::from(t.spid), (t.cp_start, t.cp_end));
        }
        for t in self.r.textboxes(true) {
            self.txbx_hdr.insert(i64::from(t.spid), (t.cp_start, t.cp_end));
        }
        let mut bk_id = 0i64;
        for b in self.r.bookmarks()? {
            // _PictureBullets: pictures of bullets kept as hidden text; the Word moves them to the
            // numbering and drops the range when it saves as .docx
            if b.name.eq_str("_PictureBullets") {
                self.skip_ranges.push((b.cp_start, b.cp_end));
                continue;
            }
            let id = bk_id;
            bk_id += 1;
            self.bk_starts.entry(b.cp_start).or_default().push((id, Rc::clone(&b.name)));
            self.bk_ends.entry(b.cp_end).or_default().push(id);
        }

        let mut doc_rels = PartRels::new();
        doc_rels.add_str("styles", "styles.xml", false);
        doc_rels.add_str("settings", "settings.xml", false);
        doc_rels.add_str("fontTable", "fontTable.xml", false);
        self.overrides.push(("/word/styles.xml".into(), format!("{OVERRIDE_PREFIX}.styles+xml")));
        self.overrides.push(("/word/settings.xml".into(), format!("{OVERRIDE_PREFIX}.settings+xml")));
        self.overrides.push(("/word/fontTable.xml".into(), format!("{OVERRIDE_PREFIX}.fontTable+xml")));

        // notes and comments (their ids come before the body, which refers to them)
        let footnotes = self.r.notes(NoteKind::Footnote);
        let endnotes = self.r.notes(NoteKind::Endnote);
        for (i, n) in footnotes.iter().enumerate() {
            self.footnote_ids.insert(n.ref_cp, u32::try_from(i + 1).unwrap_or(u32::MAX));
        }
        for (i, n) in endnotes.iter().enumerate() {
            self.endnote_ids.insert(n.ref_cp, u32::try_from(i + 1).unwrap_or(u32::MAX));
        }
        let comments = self.r.comments()?;
        for (i, c) in comments.iter().enumerate() {
            self.comment_by_ref.insert(c.ref_cp, i);
            if let (Some(start), Some(end)) = (c.range_start, c.range_end)
                && end >= start
            {
                self.comment_starts.entry(start).or_default().push(i);
                self.comment_ends.entry(end).or_default().push(i);
            }
        }

        // the body
        let body = self.render_main(&mut doc_rels)?;
        let background = self.background_xml();
        self.files.set(
            "word/document.xml".into(),
            FileData::Text(format!("{XML_DECL}<w:document{}>{background}<w:body>{body}</w:body></w:document>", content_ns())),
        );
        self.overrides.push(("/word/document.xml".into(), format!("{OVERRIDE_PREFIX}.document.main+xml")));

        if !footnotes.is_empty() {
            doc_rels.add_str("footnotes", "footnotes.xml", false);
            let x = self.render_notes(&footnotes, NoteKind::Footnote)?;
            self.files.set("word/footnotes.xml".into(), FileData::Text(x));
            self.overrides.push(("/word/footnotes.xml".into(), format!("{OVERRIDE_PREFIX}.footnotes+xml")));
        }
        if !endnotes.is_empty() {
            doc_rels.add_str("endnotes", "endnotes.xml", false);
            let x = self.render_notes(&endnotes, NoteKind::Endnote)?;
            self.files.set("word/endnotes.xml".into(), FileData::Text(x));
            self.overrides.push(("/word/endnotes.xml".into(), format!("{OVERRIDE_PREFIX}.endnotes+xml")));
        }
        if !comments.is_empty() {
            doc_rels.add_str("comments", "comments.xml", false);
            let x = self.render_comments(&comments)?;
            self.files.set("word/comments.xml".into(), FileData::Text(x));
            self.overrides.push(("/word/comments.xml".into(), format!("{OVERRIDE_PREFIX}.comments+xml")));
        }

        // styles, numbering, fonts, settings
        let styles_xml = {
            let ids = &self.style_ids;
            let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
            let ctx = PropCtx { fonts: &self.r.fonts, style_id: &sid };
            let lfo_count = self.lfo_count;
            let num_id = move |n: u32| DocxWriter::num_id_of(lfo_count, n);
            build_styles(&mut self.r.styles, &self.r.fonts, &ctx, ids, &num_id)
        };
        self.files.set("word/styles.xml".into(), FileData::Text(styles_xml));
        let numbering = {
            let ids = &self.style_ids;
            let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
            let ctx = PropCtx { fonts: &self.r.fonts, style_id: &sid };
            build_numbering(&self.r.lists, &ctx)
        };
        if let Some(numbering) = numbering {
            doc_rels.add_str("numbering", "numbering.xml", false);
            self.files.set("word/numbering.xml".into(), FileData::Text(numbering));
            self.overrides.push(("/word/numbering.xml".into(), format!("{OVERRIDE_PREFIX}.numbering+xml")));
        }
        let font_table = self.font_table();
        self.files.set("word/fontTable.xml".into(), FileData::Text(font_table));
        let settings = build_settings(
            self.r.dop,
            self.r.fib.n_fib,
            &self.r.fonts,
            self.r.styles.sheet.stshif.ftc_fe,
            !footnotes.is_empty(),
            !endnotes.is_empty(),
        );
        self.files.set("word/settings.xml".into(), FileData::Text(settings));
        self.files.set("word/_rels/document.xml.rels".into(), FileData::Text(doc_rels.xml()));

        if opts.doc_props {
            self.files.set(
                "docProps/app.xml".into(),
                FileData::Text(format!(
                    "{XML_DECL}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Universal Office (conversão de .doc)</Application></Properties>"
                )),
            );
            self.overrides
                .push(("/docProps/app.xml".into(), "application/vnd.openxmlformats-officedocument.extended-properties+xml".into()));
        }
        let mut root_rels = PartRels::new();
        root_rels.add_str("officeDocument", "word/document.xml", false);
        if opts.doc_props {
            root_rels.add_str("extended-properties", "docProps/app.xml", false);
        }
        self.files.set("_rels/.rels".into(), FileData::Text(root_rels.xml()));

        let mut ct = format!("{XML_DECL}<Types xmlns=\"{}\">", ns::CT);
        ct += &el_of(
            "Default",
            &[
                ("Extension", Some(AttrVal::Str("rels"))),
                ("ContentType", Some(AttrVal::Str("application/vnd.openxmlformats-package.relationships+xml"))),
            ],
            "",
        );
        ct += &el_of("Default", &[("Extension", Some(AttrVal::Str("xml"))), ("ContentType", Some(AttrVal::Str("application/xml")))], "");
        for ext in &self.media_ext {
            ct += &el_of("Default", &[("Extension", Some(AttrVal::Str(ext))), ("ContentType", Some(AttrVal::Str(mime_of(ext))))], "");
        }
        for (part, kind) in &self.overrides {
            ct += &el_of("Override", &[("PartName", Some(AttrVal::Str(part))), ("ContentType", Some(AttrVal::Str(kind)))], "");
        }
        ct += "</Types>";

        let mut entries: Vec<(&str, &[u8])> = vec![("[Content_Types].xml", ct.as_bytes())];
        for (path, data) in &self.files.items {
            entries.push((
                path,
                match data {
                    FileData::Text(s) => s.as_bytes(),
                    FileData::Bytes(b) => b.as_ref(),
                },
            ));
        }
        write(entries, WriteOptions::default()).map_err(|_| DocError::Corrupt("writing the package"))
    }

    /// `fontTable`.
    fn font_table(&self) -> String {
        const FAMS: [&str; 8] = ["auto", "roman", "swiss", "modern", "script", "decorative", "auto", "auto"];
        const PITCH: [&str; 4] = ["default", "fixed", "variable", "default"];
        let mut seen: HashSet<&[u16]> = HashSet::new();
        let mut x = String::new();
        for f in &self.r.fonts {
            font_entry(f, &mut seen, &mut x, &FAMS, &PITCH);
        }
        let a = attrs(&Attrs::of(&[("xmlns:w", Some(AttrVal::Str(ns::W))), ("xmlns:r", Some(AttrVal::Str(ns::R)))]));
        format!("{XML_DECL}<w:fonts{a}>{x}</w:fonts>")
    }
}

fn font_entry<'f>(f: &'f DocFont, seen: &mut HashSet<&'f [u16]>, x: &mut String, fams: &[&str; 8], pitch: &[&str; 4]) {
    if !seen.insert(f.name.units()) {
        return;
    }
    let mut c = String::new();
    if let Some(alt) = &f.alt_name
        && !alt.is_empty()
    {
        c += &el_of("w:altName", &[("w:val", Some(AttrVal::Units(alt.units())))], "");
    }
    if f.panose.iter().any(|&b| b != 0) {
        let p: String = f.panose.iter().map(|b| format!("{b:02X}")).collect();
        c += &el_of("w:panose1", &[("w:val", Some(AttrVal::Str(&p)))], "");
    }
    c += &el_of("w:charset", &[("w:val", Some(AttrVal::Str(&format!("{:02X}", f.charset))))], "");
    c += &el_of("w:family", &[("w:val", Some(AttrVal::Str(fams.get(usize::from(f.ff)).copied().unwrap_or("auto"))))], "");
    if !f.true_type {
        c += "<w:notTrueType/>";
    }
    c += &el_of("w:pitch", &[("w:val", Some(AttrVal::Str(pitch.get(usize::from(f.prq)).copied().unwrap_or("default"))))], "");
    let h = |v: u32| format!("{v:08X}");
    let (u0, u1, u2, u3, c0, c1) = (h(f.sig[0]), h(f.sig[1]), h(f.sig[2]), h(f.sig[3]), h(f.sig[4]), h(f.sig[5]));
    c += &el_of(
        "w:sig",
        &[
            ("w:usb0", Some(AttrVal::Str(&u0))),
            ("w:usb1", Some(AttrVal::Str(&u1))),
            ("w:usb2", Some(AttrVal::Str(&u2))),
            ("w:usb3", Some(AttrVal::Str(&u3))),
            ("w:csb0", Some(AttrVal::Str(&c0))),
            ("w:csb1", Some(AttrVal::Str(&c1))),
        ],
        "",
    );
    *x += &el_of("w:font", &[("w:name", Some(AttrVal::Units(f.name.units())))], &c);
}

/// The author of a comment or a revision that has none (output text of the converter, Portuguese, as
/// the TS writes it).
pub(super) const UNKNOWN_AUTHOR: &str = "Autor desconhecido";
