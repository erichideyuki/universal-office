//! Tokenization of the blocks for the diff engine (compare `tokens.ts`).
//!
//! Each block (paragraph or table) becomes a [`BlockToken`] with two keys:
//!
//! - `content_key`: the identity of the CONTENT: the text of the runs, each image replaced by a marker
//!   derived from the BYTES of the media (length and FNV-1a hash). It aligns the documents; an image is
//!   equal iff its bytes are (the hash only speeds up: [`tokens_equal`] compares the bytes);
//! - `format_key`: the signature of the FORMATTING (`ParaProps` + `RunProps` per segment + the display
//!   size of the images). Equal content and different formatting is a "format only" change.
//!
//! The keys are spoof-proof as in the TS: markers start and end with U+001F (illegal in XML 1.0, so
//! never legitimate text; removed from literal text), and the composition of cells and tables is
//! length-prefixed. `commentIds` stay out of the format key (a comment is an annotation).
//!
//! The keys are UTF-16 strings (text with lone surrogates is distinct text); they are interned to
//! dense ids ([`Interner`]), so that the algorithms compare integers: two keys are equal iff their
//! ids are.

use std::collections::HashMap;

use uo_jsnum::format::to_js_string;

use crate::js::{JsNumber, JsonWriter};
use crate::model::run::RunProps;
use crate::model::{Cell, MediaStore, ParaProps, Paragraph, Row, Table};
use crate::u16buf::U16Buf;

use super::DiffBlock;

/// U+001F, the delimiter of markers and frames.
const SEP: u16 = 0x1f;

/// The deepest table nesting keyed (the reader stops at `max_nesting`; a model built otherwise is
/// cut here: the cell contributes nothing below).
const MAX_DEPTH: usize = 512;

/// A key's id in the [`Interner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyId(pub u32);

/// Strings to dense ids: equal strings, equal ids.
#[derive(Debug, Default)]
pub struct Interner {
    map: HashMap<Box<[u16]>, u32>,
}

impl Interner {
    /// The id of `key`.
    pub fn intern(&mut self, key: &U16Buf) -> KeyId {
        if let Some(&id) = self.map.get(key.units()) {
            return KeyId(id);
        }
        let id = u32::try_from(self.map.len()).unwrap_or(u32::MAX);
        self.map.insert(key.units().into(), id);
        KeyId(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// What the diff knows of a block.
#[derive(Debug, Clone)]
pub struct BlockToken<'m> {
    pub is_table: bool,
    pub content_key: KeyId,
    pub format_key: KeyId,
    /// the bytes of the images of the block, in document order (exact equality)
    pub images: Vec<&'m [u8]>,
}

/// `fnv1a(bytes)`: FNV-1a 32 bits, in hex.
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:x}")
}

fn push_sanitized(out: &mut U16Buf, units: &[u16]) {
    for &u in units {
        if u != SEP {
            out.push_unit(u);
        }
    }
}

fn sanitized_str(s: &str) -> Vec<u16> {
    s.encode_utf16().filter(|&u| u != SEP).collect()
}

/// `${SEP}${tag}${s.length}${SEP}${s}`: a frame with a length prefix.
fn frame(tag: &str, s: &U16Buf) -> U16Buf {
    let mut out = U16Buf::new();
    out.push_unit(SEP);
    out.push_str(tag);
    out.push_str(&s.len().to_string());
    out.push_unit(SEP);
    out.push_buf(s);
    out
}

fn image_bytes<'m>(media: &'m MediaStore, r: &crate::model::run::InlineImage) -> &'m [u8] {
    media.get(r.data.0).unwrap_or(&[])
}

/// `${SEP}img:${length}:${fnv1a}${SEP}`.
fn image_content_marker(data: &[u8]) -> String {
    format!("\u{1f}img:{}:{}\u{1f}", data.len(), fnv1a(data))
}

fn flag(b: Option<bool>) -> bool {
    b.unwrap_or(false)
}

/// `x ?? ''` of an optional number, as the join of an array writes it.
fn opt_num(n: Option<JsNumber>) -> String {
    match n {
        Some(JsNumber::Num(x)) => to_js_string(x),
        _ => String::new(),
    }
}

fn opt_str(s: Option<&str>) -> &str {
    s.unwrap_or("")
}

/// `runPropsSig`: a stable signature of the `RunProps` (fixed order, no `commentIds`).
fn run_props_sig(p: &RunProps) -> String {
    let underline = if flag(p.underline) {
        format!("u{}{}", p.underline_style.as_deref().unwrap_or("single"), opt_str(p.underline_color.as_deref()))
    } else {
        String::new()
    };
    [
        if flag(p.bold) { "b".to_owned() } else { String::new() },
        if flag(p.italic) { "i".to_owned() } else { String::new() },
        underline,
        if flag(p.strike) { "s".to_owned() } else { String::new() },
        if flag(p.double_strike) { "ds".to_owned() } else { String::new() },
        opt_str(p.vert_align.as_deref()).to_owned(),
        opt_num(p.sz_half_pt),
        opt_str(p.color.as_deref()).to_owned(),
        opt_str(p.highlight.as_deref()).to_owned(),
        opt_str(p.font_ascii.as_deref()).to_owned(),
        if flag(p.all_caps) { "ac".to_owned() } else { String::new() },
        if flag(p.small_caps) { "sc".to_owned() } else { String::new() },
        opt_num(p.char_spacing_twips),
        opt_str(p.link.as_deref()).to_owned(),
        if flag(p.is_math) { "m".to_owned() } else { String::new() },
    ]
    .join("|")
}

fn json_of(v: &dyn crate::js::JsValue, media: &MediaStore) -> String {
    JsonWriter::stringify(v, Some(media)).unwrap_or_default()
}

/// `paraPropsSig`: a stable signature of the `ParaProps`.
fn para_props_sig(p: &ParaProps, media: &MediaStore) -> String {
    let numbering = match &p.numbering {
        Some(n) => format!("{}:{}", n.kind, num_template(n.ilvl)),
        None => String::new(),
    };
    [
        opt_str(p.alignment.as_deref()).to_owned(),
        opt_str(p.style_id.as_deref()).to_owned(),
        numbering,
        opt_num(p.indent_left_twips),
        opt_num(p.indent_right_twips),
        opt_num(p.indent_first_line_twips),
        opt_num(p.line_twentieths),
        opt_num(p.space_after_twips),
        opt_num(p.space_before_twips),
        opt_str(p.toc_instr.as_deref()).to_owned(),
        if flag(p.toc_entry) { "t".to_owned() } else { String::new() },
        p.tab_stops.as_ref().map(|t| json_of(t, media)).unwrap_or_default(),
        opt_str(p.shading_hex.as_deref()).to_owned(),
        if flag(p.box_border) { "bb".to_owned() } else { String::new() },
        p.borders.as_ref().map(|b| json_of(b, media)).unwrap_or_default(),
        if flag(p.page_break_before) { "pb".to_owned() } else { String::new() },
        if flag(p.drop_cap) { "dc".to_owned() } else { String::new() },
        p.section_break.as_ref().map(|s| json_of(&s.sect_pr, media)).unwrap_or_default(),
    ]
    .join("|")
}

/// `${n}` in a template literal (`null` is `"null"`).
fn num_template(n: JsNumber) -> String {
    match n {
        JsNumber::Null => "null".to_owned(),
        JsNumber::Num(x) => to_js_string(x),
    }
}

fn paragraph_content_key(p: &Paragraph, media: &MediaStore) -> U16Buf {
    let mut out = U16Buf::new();
    // a field: the reader gives every run of the cached result the `field`, and `fldSimple` carries
    // the raw instruction while begin/separate/end carries ' INSTR ': trim unifies them, and runs of
    // the same field emit the marker once (a split of runs is not a change of content)
    let mut prev_field: Option<Vec<u16>> = None;
    for r in &p.runs {
        let ext = r.ext();
        if let Some(field) = ext.and_then(|e| e.field.as_deref()) {
            let instr = sanitized_str(uo_jsnum::text::trim(field));
            if prev_field.as_deref() != Some(instr.as_slice()) {
                out.push_unit(SEP);
                out.push_str("fld:");
                out.push_units(&instr);
                out.push_unit(SEP);
            }
            prev_field = Some(instr);
            continue;
        }
        prev_field = None;
        if let Some(img) = ext.and_then(|e| e.image.as_deref()) {
            out.push_str(&image_content_marker(image_bytes(media, img)));
        } else if let Some(nr) = ext.and_then(|e| e.note_ref.as_ref()) {
            out.push_unit(SEP);
            out.push_str("note:");
            out.push_str(&nr.kind);
            out.push_unit(SEP);
        } else if let Some(c) = ext.and_then(|e| e.custom.as_deref()) {
            let t = sanitized_str(&c.r#type);
            out.push_unit(SEP);
            out.push_str("custom:");
            out.push_str(&t.len().to_string());
            out.push_str(":");
            out.push_units(&t);
            out.push_str(":");
            push_sanitized(&mut out, c.fallback_text.units());
            out.push_unit(SEP);
        } else {
            push_sanitized(&mut out, r.text.units());
        }
    }
    out
}

fn paragraph_format_key(p: &Paragraph, media: &MediaStore) -> U16Buf {
    // coalesces adjacent runs with the same signature: the same visual formatting split into more or
    // fewer runs is not a change; the length counted is that of the displayed text
    let mut segs: Vec<String> = Vec::new();
    let mut cur_sig = String::new();
    let mut cur_len = 0usize;
    let mut cur_img = String::new();
    let mut flush = |cur_len: &mut usize, cur_sig: &str, cur_img: &mut String| {
        if *cur_len > 0 || !cur_img.is_empty() {
            segs.push(format!("{cur_len}:{cur_sig}{cur_img}"));
        }
        *cur_len = 0;
        cur_img.clear();
    };
    for r in &p.runs {
        let sig = run_props_sig(&r.props);
        let img = match r.ext().and_then(|e| e.image.as_deref()) {
            Some(i) => {
                let pos = if flag(i.floating) {
                    format!("@{},{}", opt_num_or_zero(i.offset_x_twips), opt_num_or_zero(i.offset_y_twips))
                } else {
                    String::new()
                };
                format!("img[{}x{}{}]", num_template(i.width_twips), num_template(i.height_twips), pos)
            }
            None => String::new(),
        };
        if sig != cur_sig || !img.is_empty() {
            flush(&mut cur_len, &cur_sig, &mut cur_img);
            cur_sig = sig;
            cur_img = img;
        }
        cur_len += r.text.len();
        if !cur_img.is_empty() {
            flush(&mut cur_len, &cur_sig, &mut cur_img);
        }
    }
    flush(&mut cur_len, &cur_sig, &mut cur_img);
    let mut out = U16Buf::new();
    out.push_str(&para_props_sig(&p.props, media));
    out.push_unit(SEP);
    out.push_str(&segs.join("\u{1f}"));
    out
}

/// `${x ?? 0}` in a template literal.
fn opt_num_or_zero(n: Option<JsNumber>) -> String {
    match n {
        Some(JsNumber::Num(x)) => to_js_string(x),
        _ => "0".to_owned(),
    }
}

/// The paragraphs of a cell as the key sees them: the final view when the table is a finalized one.
fn cell_paragraphs<'a>(cell: &'a Cell, final_cells: bool) -> Vec<std::borrow::Cow<'a, Paragraph>> {
    if final_cells {
        crate::final_view::final_paragraphs(cell.content.paras.iter()).paras
    } else {
        cell.content.paras.iter().map(std::borrow::Cow::Borrowed).collect()
    }
}

fn cell_content_key(c: &Cell, final_cells: bool, media: &MediaStore, depth: usize) -> U16Buf {
    let mut out = U16Buf::new();
    for p in cell_paragraphs(c, final_cells) {
        out.push_buf(&frame("P", &paragraph_content_key(&p, media)));
    }
    if depth < MAX_DEPTH {
        for a in c.content.tables.iter() {
            out.push_buf(&frame("T", &table_content_key(&a.table, false, media, depth + 1)));
        }
    }
    out.push_unit(SEP);
    out.push_str("span");
    out.push_str(&num_template(c.grid_span));
    out
}

/// The content key of a row: its cells' frames (`rowContentKey`).
#[must_use]
pub fn row_content_key(row: &Row, final_cells: bool, media: &MediaStore) -> U16Buf {
    let mut out = U16Buf::new();
    for c in &row.cells {
        out.push_buf(&frame("C", &cell_content_key(c, final_cells, media, 0)));
    }
    out
}

fn table_content_key(t: &Table, final_cells: bool, media: &MediaStore, depth: usize) -> U16Buf {
    let mut out = U16Buf::new();
    for row in &t.rows {
        let mut cells = U16Buf::new();
        for c in &row.cells {
            cells.push_buf(&frame("C", &cell_content_key(c, final_cells, media, depth)));
        }
        out.push_buf(&frame("R", &cells));
    }
    out
}

fn cell_format_key(c: &Cell, final_cells: bool, media: &MediaStore, depth: usize) -> U16Buf {
    let mut parts = U16Buf::new();
    for p in cell_paragraphs(c, final_cells) {
        parts.push_buf(&frame("P", &paragraph_format_key(&p, media)));
    }
    if depth < MAX_DEPTH {
        for a in c.content.tables.iter() {
            parts.push_buf(&frame("T", &table_format_key(&a.table, false, media, depth + 1)));
        }
    }
    let mut out = parts;
    for piece in [
        opt_num(c.width_twips),
        opt_str(c.shading_hex.as_deref()).to_owned(),
        c.borders.as_ref().map(|b| json_of(b, media)).unwrap_or_default(),
    ] {
        out.push_unit(SEP);
        out.push_str(&piece);
    }
    out
}

/// The format key of a row (`rowFormatKey`).
#[must_use]
pub fn row_format_key(row: &Row, final_cells: bool, media: &MediaStore) -> U16Buf {
    let mut out = U16Buf::new();
    for c in &row.cells {
        out.push_buf(&frame("C", &cell_format_key(c, final_cells, media, 0)));
    }
    out
}

fn table_format_key(t: &Table, final_cells: bool, media: &MediaStore, depth: usize) -> U16Buf {
    let mut rows = U16Buf::new();
    for row in &t.rows {
        let mut cells = U16Buf::new();
        for c in &row.cells {
            cells.push_buf(&frame("C", &cell_format_key(c, final_cells, media, depth)));
        }
        rows.push_buf(&frame("R", &cells));
    }
    let grid: Vec<String> = t.grid_cols_twips.iter().map(|n| if n.is_null() { String::new() } else { num_template(*n) }).collect();
    let mut out = U16Buf::new();
    out.push_str(&grid.join(","));
    out.push_unit(SEP);
    out.push_str(&t.borders.as_ref().map(|b| json_of(b, media)).unwrap_or_default());
    out.push_unit(SEP);
    out.push_buf(&rows);
    out
}

fn collect_paragraph_images<'m>(p: &Paragraph, media: &'m MediaStore, out: &mut Vec<&'m [u8]>) {
    for r in &p.runs {
        if let Some(img) = r.ext().and_then(|e| e.image.as_deref()) {
            out.push(image_bytes(media, img));
        }
    }
}

/// The images of a table in document order: each cell's paragraphs, then its nested tables.
fn collect_table_images<'m>(t: &Table, final_cells: bool, media: &'m MediaStore, out: &mut Vec<&'m [u8]>, depth: usize) {
    for row in &t.rows {
        for cell in &row.cells {
            for p in cell_paragraphs(cell, final_cells) {
                collect_paragraph_images(&p, media, out);
            }
            if depth < MAX_DEPTH {
                for a in cell.content.tables.iter() {
                    collect_table_images(&a.table, false, media, out, depth + 1);
                }
            }
        }
    }
}

/// `tokenizeBlock` (the keys interned in `interner`).
pub fn tokenize_block<'m>(block: &DiffBlock<'_>, media: &'m MediaStore, interner: &mut Interner) -> BlockToken<'m> {
    match block {
        DiffBlock::Paragraph(p) => {
            let mut images = Vec::new();
            collect_paragraph_images(p, media, &mut images);
            let content = frame("p", &paragraph_content_key(p, media));
            let format = paragraph_format_key(p, media);
            BlockToken { is_table: false, content_key: interner.intern(&content), format_key: interner.intern(&format), images }
        }
        DiffBlock::Table { table, final_cells } => {
            let mut images = Vec::new();
            collect_table_images(table, *final_cells, media, &mut images, 0);
            let content = frame("t", &table_content_key(table, *final_cells, media, 0));
            let format = table_format_key(table, *final_cells, media, 0);
            BlockToken { is_table: true, content_key: interner.intern(&content), format_key: interner.intern(&format), images }
        }
    }
}

/// `tokenizeBlocks`.
pub fn tokenize_blocks<'m>(blocks: &[DiffBlock<'_>], media: &'m MediaStore, interner: &mut Interner) -> Vec<BlockToken<'m>> {
    blocks.iter().map(|b| tokenize_block(b, media, interner)).collect()
}

/// Equality of CONTENT of two tokens: equal keys and equal images byte by byte. Returns the cost in
/// bytes compared as well (for the work limit of the diff).
#[must_use]
pub fn tokens_equal_cost(a: &BlockToken<'_>, b: &BlockToken<'_>) -> (bool, u64) {
    if a.content_key != b.content_key || a.images.len() != b.images.len() {
        return (false, 0);
    }
    let mut cost = 0u64;
    for (x, y) in a.images.iter().zip(&b.images) {
        cost = cost.saturating_add(u64::try_from(x.len().min(y.len())).unwrap_or(u64::MAX));
        if x != y {
            return (false, cost);
        }
    }
    (true, cost)
}

/// `tokensEqual(a, b)`.
#[must_use]
pub fn tokens_equal(a: &BlockToken<'_>, b: &BlockToken<'_>) -> bool {
    tokens_equal_cost(a, b).0
}

/// `bytesEqual(a, b)`.
#[must_use]
pub fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}
