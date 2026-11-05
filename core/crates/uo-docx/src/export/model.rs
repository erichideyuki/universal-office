//! Input types and helpers shared by the exporters (docx-core `export/model.ts`): [`ExportDoc`],
//! the final view of the track-changes state (`finalExportDoc`), `orderedBlocks`, `headingLevel`,
//! the escapes, the image data URI and the length conversions.

use std::collections::{HashMap, HashSet};

use uo_jsnum::format::to_fixed;
use uo_jsnum::math;
use uo_jsnum::parse::to_number;

use crate::final_view::{FinalParagraphs, final_paragraphs};
use crate::ids::DrawingId;
use crate::js::{JsMap, JsNumber, NumKey};
use crate::model::run::InlineImage;
use crate::model::{Comment, Document, MediaStore, Paragraph, Story, Table};
use crate::u16buf::U16Buf;

use super::matchers::heading_level_of;

/// The input of the exporters: the document as the editor's live state (paragraphs + the tables
/// anchored "before paragraph N", the notes, the comments). Borrows what it reads; `sectPr` of the TS
/// shape is not here (no exporter reads it).
#[derive(Debug, Clone, Copy)]
pub struct ExportDoc<'a> {
    pub title: Option<&'a str>,
    /// the body: its paragraphs, and its tables as anchors (`paragraphs`, `tableAnchors`)
    pub body: &'a Story,
    pub comments: &'a [Comment],
    /// the TS `Map<number, Paragraph[]>` of the top-level paragraphs of each note
    pub footnotes: Option<&'a JsMap<NumKey, Story>>,
    pub endnotes: Option<&'a JsMap<NumKey, Story>>,
    /// where the image bytes are
    pub media: &'a MediaStore,
    /// pre-computed data URIs by image (the TS `WeakMap` keyed by the image object: the drawing id)
    pub image_data_uris: Option<&'a HashMap<DrawingId, Box<str>>>,
}

impl<'a> ExportDoc<'a> {
    /// `exportDocFromFullDocument(full)`, see [`super::from_docx::export_doc_from_full_document`].
    #[must_use]
    pub fn from_document(doc: &'a Document) -> ExportDoc<'a> {
        super::from_docx::export_doc_from_full_document(doc)
    }
}

/// A body block in document order.
#[derive(Debug, Clone, Copy)]
pub enum OrderedBlock<'a> {
    Para(&'a Paragraph),
    Table(&'a Table),
}

/// `orderedBlocks`: paragraphs and tables interleaved in the original order (anchors past the end
/// of the paragraphs are dropped, as in the TS).
fn interleave<'a>(paras: &[&'a Paragraph], anchors: &[(usize, &'a Table)]) -> Vec<OrderedBlock<'a>> {
    let mut by_para: HashMap<usize, Vec<&'a Table>> = HashMap::new();
    for &(before, table) in anchors {
        by_para.entry(before).or_default().push(table);
    }
    let mut out = Vec::with_capacity(paras.len() + anchors.len());
    for (i, &p) in paras.iter().enumerate() {
        if let Some(ts) = by_para.get(&i) {
            out.extend(ts.iter().map(|&t| OrderedBlock::Table(t)));
        }
        out.push(OrderedBlock::Para(p));
    }
    if let Some(ts) = by_para.get(&paras.len()) {
        out.extend(ts.iter().map(|&t| OrderedBlock::Table(t)));
    }
    out
}

/// `orderedBlocks(doc)` over the body as it is (no final view).
#[must_use]
pub fn ordered_blocks<'a>(doc: &ExportDoc<'a>) -> Vec<OrderedBlock<'a>> {
    let paras: Vec<&Paragraph> = doc.body.paras.iter().collect();
    let anchors: Vec<(usize, &Table)> = doc.body.tables.iter().map(|a| (a.before_para as usize, &*a.table)).collect();
    interleave(&paras, &anchors)
}

/// The document in the FINAL view of the track-changes state (`finalExportDoc`): the body's
/// paragraphs and anchors; the notes are read through [`FinalDoc::note_paragraphs`]. The cells of the
/// tables are finalized when they are rendered (their top-level paragraphs only: the TS keeps the
/// nested tables of a cell as they are).
pub(crate) struct FinalDoc<'a> {
    pub doc: ExportDoc<'a>,
    body: FinalParagraphs<'a>,
}

impl<'a> FinalDoc<'a> {
    pub(crate) fn new(doc: &ExportDoc<'a>) -> FinalDoc<'a> {
        FinalDoc { doc: *doc, body: final_paragraphs(doc.body.paras.iter()) }
    }

    /// The body blocks in order, with the tables anchored on the final paragraph list.
    pub(crate) fn ordered_blocks(&self) -> Vec<OrderedBlock<'_>> {
        let paras: Vec<&Paragraph> = self.body.paras.iter().map(|c| &**c).collect();
        let n = self.doc.body.paras.len();
        let anchors: Vec<(usize, &Table)> = self
            .doc
            .body
            .tables
            .iter()
            .map(|a| {
                let before = a.before_para as usize;
                let at = if before < n { self.body.index.get(before).copied().unwrap_or(paras.len()) } else { paras.len() };
                (at, &*a.table)
            })
            .collect();
        interleave(&paras, &anchors)
    }

    /// The final paragraphs of a note (`None` when the note does not exist or `id` cannot be a key).
    pub(crate) fn note_paragraphs(&self, footnote: bool, id: JsNumber) -> Option<FinalParagraphs<'a>> {
        let map = if footnote { self.doc.footnotes } else { self.doc.endnotes }?;
        let JsNumber::Num(x) = id else { return None };
        let story = map.get(&NumKey::new(x))?;
        Some(final_paragraphs(story.paras.iter()))
    }
}

/// A number as a key of a TS `Map`/`Set` (`null` included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NumberKey {
    Null,
    Num(NumKey),
}

impl From<JsNumber> for NumberKey {
    fn from(n: JsNumber) -> Self {
        match n {
            JsNumber::Null => NumberKey::Null,
            JsNumber::Num(x) => NumberKey::Num(NumKey::new(x)),
        }
    }
}

/// A TS `Set` of numbers: insertion order, no duplicates.
#[derive(Debug, Default)]
pub(crate) struct OrderedNumbers {
    seen: HashSet<NumberKey>,
    order: Vec<JsNumber>,
}

impl OrderedNumbers {
    pub(crate) fn add(&mut self, n: JsNumber) {
        if self.seen.insert(NumberKey::from(n)) {
            self.order.push(n);
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = JsNumber> + '_ {
        self.order.iter().copied()
    }
}

/// The comments by id (`new Map(comments.map(c => [c.id, c]))`: a later duplicate wins).
pub(crate) fn comments_by_id(comments: &[Comment]) -> HashMap<NumberKey, &Comment> {
    let mut m = HashMap::new();
    for c in comments {
        m.insert(NumberKey::from(c.id), c);
    }
    m
}

/// The heading level (1..6) of a paragraph, or 0 (`headingLevel`).
#[must_use]
pub fn heading_level(p: &Paragraph) -> u32 {
    heading_level_of(p.props.style_id.as_deref().unwrap_or(""))
}

/// The plain text of a paragraph (`paraPlainText`: the runs' text), in code units.
#[must_use]
pub fn para_plain_text(p: &Paragraph) -> Vec<u16> {
    let mut out = Vec::with_capacity(p.text_len());
    for r in &p.runs {
        out.extend_from_slice(r.text.units());
    }
    out
}

/// Whether the paragraph is a list item (`isListItem`).
#[must_use]
pub fn is_list_item(p: &Paragraph) -> bool {
    p.props.numbering.is_some()
}

// ───────────────────────── encoding ─────────────────────────

/// `escapeHtml`: `&`, `<`, `>`.
pub fn escape_html_into(out: &mut U16Buf, s: &[u16]) {
    for &u in s {
        match u {
            0x26 => out.push_str("&amp;"),
            0x3c => out.push_str("&lt;"),
            0x3e => out.push_str("&gt;"),
            _ => out.push_unit(u),
        }
    }
}

/// `escapeAttr`: `escapeHtml` and `"` as `&quot;`.
pub fn escape_attr_into(out: &mut U16Buf, s: &[u16]) {
    for &u in s {
        match u {
            0x26 => out.push_str("&amp;"),
            0x3c => out.push_str("&lt;"),
            0x3e => out.push_str("&gt;"),
            0x22 => out.push_str("&quot;"),
            _ => out.push_unit(u),
        }
    }
}

/// `escapeHtml` of UTF-8 text.
pub fn escape_html_str_into(out: &mut U16Buf, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    escape_html_into(out, &u);
}

/// `escapeAttr` of UTF-8 text.
pub fn escape_attr_str_into(out: &mut U16Buf, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    escape_attr_into(out, &u);
}

/// `imgMime(ext)`.
#[must_use]
pub fn img_mime(ext: &str) -> String {
    let e = uo_jsnum::text::to_lowercase(ext);
    match e.as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_owned(),
        "png" => "image/png".to_owned(),
        "gif" => "image/gif".to_owned(),
        "bmp" => "image/bmp".to_owned(),
        "svg" => "image/svg+xml".to_owned(),
        "webp" => "image/webp".to_owned(),
        "tif" | "tiff" => "image/tiff".to_owned(),
        _ => format!("image/{e}"),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `btoa` of the bytes as a binary string (standard alphabet, `=` padding).
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let sextet = |shift: u32| char::from(B64.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'='));
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// `dataUri(img)`.
#[must_use]
pub fn data_uri(ext: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", img_mime(ext), base64(bytes))
}

/// `resolveImageUri(img, cache)`: the cached URI of the image when there is one.
pub(crate) fn resolve_image_uri(img: &InlineImage, doc: &ExportDoc<'_>) -> String {
    if let Some(u) = doc.image_data_uris.and_then(|m| m.get(&img.drawing)) {
        return u.to_string();
    }
    let bytes = doc.media.get(img.data.0).unwrap_or(&[]);
    data_uri(&img.ext, bytes)
}

/// `twipsToPx`: `Math.round((t / 1440) * 96)`.
#[must_use]
pub fn twips_to_px(t: JsNumber) -> f64 {
    math::round((t.to_number() / 1440.0) * 96.0)
}

/// `twipsToPt`: `+(t / 20).toFixed(2)`.
#[must_use]
pub fn twips_to_pt(t: JsNumber) -> f64 {
    let fixed = to_fixed(t.to_number() / 20.0, 2).unwrap_or_default();
    to_number(&fixed)
}

/// `halfPtToPt`: `h / 2`.
#[must_use]
pub fn half_pt_to_pt(h: JsNumber) -> f64 {
    h.to_number() / 2.0
}
