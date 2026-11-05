//! docx-core `odf/index.ts`: `readOdt`, an OpenDocument Text (`.odt`) read into the [`Document`].
//!
//! The container is the uo-opc package (lazy parts, `Package::verify` for the TS's "every entry is
//! inflated at load"), the XML the `docx-core` reader preset of uo-opc ([`crate::read::DOCX`]):
//! prefixed names are used literally (`text:p`, `style:style`), as the TS `parseXml` does.
//!
//! Shapes: the TS returns a `FullDocument` with `blocks`, `sections`, `comments`, `styles` and
//! empty `footnotes`/`endnotes`/`bookmarks`; here the body is the story of the document, a table
//! cell is a story (the TS cell has `paragraphs` then `tables`, no `blocks`: paragraphs first, the
//! tables anchored after the last), a text frame is a [`DrawingTextbox`] with its content story.
//!
//! Recursion: the TS recurses on everything. The inline walk (`collectRuns`) is a loop over an
//! explicit stack of frames (the TS call order and its buffer-flush behaviour kept, including the
//! text that an unknown element makes come out after the nested runs); the block walk (lists,
//! tables, sections, text frames, annotations) counts its nesting and, past
//! `DocxLimits::max_nesting`, reads the element as plain text (a list, table or section as one
//! paragraph of its text, a frame as inline text, an annotation dropped): declared in the record.

use std::sync::Arc;

use uo_jsnum::conv::to_uint32;
use uo_jsnum::math::{max, round};
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, split_js_space, trim};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::xml::{Element, Node};

use crate::error::DocxError;
use crate::ids::IdAlloc;
use crate::js::{JsMap, JsNumber, JsObject, JsRecord, Str};
use crate::limits::DocxLimits;
use crate::model::drawing::{AnchorPos, DrawingAnchor, Ltrb};
use crate::model::formatting::ResolvedStyle as ModelStyle;
use crate::model::media::{MediaRef, MediaStore};
use crate::model::para::{BorderEdge, NumberingProps, ParagraphBorders};
use crate::model::props::{PPr, RFonts, RPr};
use crate::model::run::{DrawingTextbox, InlineImage, RunProps, TextboxView};
use crate::model::section::{SectPr, Section};
use crate::model::story::OwnedBlock;
use crate::model::table::{Cell, Row, Table, TableBorders};
use crate::model::{Comment, Document, ParaProps, Paragraph, Run, Story};
use crate::read::DOCX;
use crate::read::plain::default_run_props;
use crate::text::Utf16;

use super::styles::{ListKind, OdfStyleIndex, RawStyle, ResolvedStyle, build_style_index, norm_hex, overlay};
use super::units::length_to_twips;

/// `ODT_MIMETYPE`.
pub const ODT_MIMETYPE: &str = "application/vnd.oasis.opendocument.text";

/// `DEFAULT_CONTENT_WIDTH_TWIPS`: the useful width columns without a width share.
const DEFAULT_CONTENT_WIDTH_TWIPS: f64 = 9026.0;

/// The most spaces one `text:s` stands for: the TS builds `' '.repeat(n)`, which throws past
/// 2^29 - 24 and is as large as memory allows below; a count over this limit is an error here.
pub const MAX_SPACE_REPEAT: f64 = 16_777_216.0;

/// The most columns one table may have (declared by `table:table-column` repeats or spanned by
/// cells): the TS pushes one array entry per column, so a hostile `number-columns-repeated` or
/// `number-columns-spanned` exhausts its memory; here it is an error past this limit.
pub const MAX_COLUMNS: f64 = 65_536.0;

/// `HEADING_LOOKS[level]`: the look baked into the run of a heading (size in half points, bold,
/// colour), so that it renders bold and larger even when no style entry matches.
const HEADING_SIZES: [f64; 6] = [40.0, 32.0, 28.0, 26.0, 24.0, 22.0];
const HEADING_COLOR: &str = "0F4761";

/// `DEFAULT_SECTPR`: A4 with 2.5 cm margins when `styles.xml` carries no page layout.
fn default_sect_pr() -> SectPr {
    SectPr {
        page_width_twips: JsNumber::Num(11906.0),
        page_height_twips: JsNumber::Num(16838.0),
        margin_left_twips: JsNumber::Num(1417.0),
        margin_right_twips: JsNumber::Num(1417.0),
        margin_top_twips: JsNumber::Num(1417.0),
        margin_bottom_twips: JsNumber::Num(1417.0),
        ..SectPr::default()
    }
}

fn num(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

fn s(x: &str) -> Str {
    x.into()
}

/// `isOdt(bytes)`: whether the bytes are an `.odt` package (`mimetype` is
/// `application/vnd.oasis.opendocument.text`). A damaged archive is not one.
#[must_use]
pub fn is_odt(bytes: &[u8], limits: &DocxLimits) -> bool {
    if bytes.len() < 4 || bytes.first() != Some(&0x50) || bytes.get(1) != Some(&0x4b) {
        return false;
    }
    let Ok(pkg) = Package::open(bytes, &limits.package, &OpenOptions::default()) else { return false };
    if pkg.verify().is_err() {
        return false;
    }
    match pkg.part_text("mimetype") {
        Ok(Some(mime)) => trim(&mime) == ODT_MIMETYPE,
        _ => false,
    }
}

/// `readOdt(bytes)`: reads an `.odt` (the bytes of the zip) into the document model.
///
/// # Errors
/// A damaged zip, no `content.xml`, malformed XML, no `office:body/office:text`, a base64 image
/// that is not base64, or a limit (package, nesting of style chains, columns, spaces).
pub fn read_odt(bytes: &[u8], limits: &DocxLimits) -> Result<Document, DocxError> {
    let pkg = Package::open(bytes, &limits.package, &OpenOptions::default())?;
    pkg.verify()?;
    read_package(&pkg, limits)
}

/// [`read_odt`] over a package already open.
///
/// # Errors
/// As [`read_odt`], but for the zip.
pub fn read_package(pkg: &Package, limits: &DocxLimits) -> Result<Document, DocxError> {
    let content_doc = match pkg.xml_part("content.xml", &DOCX) {
        Ok(d) => d,
        Err(uo_opc::Error::MissingPart(_)) => return Err(DocxError::Corrupt("ODF package without content.xml")),
        Err(e) => return Err(e.into()),
    };
    let content_root = content_doc.root();
    let styles_doc = match pkg.xml_part("styles.xml", &DOCX) {
        Ok(d) => Some(d),
        Err(uo_opc::Error::MissingPart(_)) => None,
        Err(e) => return Err(e.into()),
    };
    let styles_root = styles_doc.as_ref().map(uo_opc::xml::Document::root);

    let index = build_style_index(content_root, styles_root);

    let body = content_root.first_child("office:body");
    let Some(office_text) = body.and_then(|b| b.first_child("office:text")) else {
        return Err(DocxError::Corrupt("content.xml without office:body/office:text"));
    };

    let mut rd =
        Reader { pkg, index, comments: Vec::new(), next_comment_id: 0, ids: IdAlloc::new(), media: MediaStore::new(), depth: 0, limits };
    let mut blocks: Vec<OwnedBlock> = Vec::new();
    for child in office_text.child_elements() {
        rd.read_body_child(child, &mut blocks, 0)?;
    }
    if blocks.is_empty() {
        blocks.push(OwnedBlock::Paragraph(Paragraph::empty(rd.ids.para())));
    }

    let sect_pr = read_page_geometry(styles_root, content_root);
    let mut body_story = Story::new();
    let block_count = blocks.len();
    for b in blocks {
        body_story.push_block(b);
    }
    let section = Section {
        sect_pr: Arc::new(sect_pr),
        block_start: num(0.0),
        block_end: num(block_count as f64),
        header_refs: Vec::new(),
        footer_refs: Vec::new(),
        headers: JsRecord::new(),
        footers: JsRecord::new(),
        ..Section::default()
    };
    let styles = rd.build_styles_map()?;
    Ok(Document {
        body: body_story,
        sections: Arc::from(vec![section]),
        comments: Arc::from(rd.comments),
        styles: Arc::new(styles),
        media: rd.media,
        ids: rd.ids,
        ..Document::default()
    })
}

/// One child of an element for the inline walk: a string or an element (what the TS
/// `children: (XmlElement | string)[]` holds; a pseudo container of text from the note flattening
/// is just a list of these).
#[derive(Clone, Copy)]
enum Child<'d> {
    Text(&'d str),
    Elem(Element<'d>),
}

fn children_of(el: Element<'_>) -> Vec<Child<'_>> {
    el.children()
        .filter_map(|n| match n {
            Node::Element(e) => Some(Child::Elem(e)),
            Node::Text(t) | Node::CData(t) => Some(Child::Text(t)),
            Node::Comment(_) | Node::Pi(_) => None,
        })
        .collect()
}

/// The reading state (the TS `ReadCtx`).
struct Reader<'p, 'd> {
    pkg: &'p Package,
    index: OdfStyleIndex<'d>,
    comments: Vec<Comment>,
    next_comment_id: u32,
    ids: IdAlloc,
    media: MediaStore,
    /// open lists, tables, sections, frames and annotations (past `max_nesting`: plain text)
    depth: u32,
    limits: &'p DocxLimits,
}

/// One level of the inline walk: the TS call `collectRuns(container, runs, ctx, base, pendingLink)`.
struct Frame<'d> {
    items: Vec<Child<'d>>,
    pos: usize,
    base: Arc<RunProps>,
    link: Option<&'d str>,
    buffer: Vec<u16>,
}

impl<'d> Frame<'d> {
    fn new(items: Vec<Child<'d>>, base: Arc<RunProps>, link: Option<&'d str>) -> Frame<'d> {
        Frame { items, pos: 0, base, link, buffer: Vec::new() }
    }

    /// `flush()`: the buffered text as a run (with the link look when a link is pending).
    fn flush(&mut self, runs: &mut Vec<Run>) {
        if self.buffer.is_empty() {
            return;
        }
        let props = match self.link.filter(|l| !l.is_empty()) {
            Some(link) => {
                let mut p = (*self.base).clone();
                p.link = Some(s(link));
                p.underline = Some(true);
                if p.color.is_none() {
                    p.color = Some(s("0563C1"));
                }
                Arc::new(p)
            }
            None => self.base.clone(),
        };
        runs.push(Run { text: Utf16::from_units(std::mem::take(&mut self.buffer)), props, ..Run::default() });
    }
}

/// `mergeRunProps(base, over)`.
fn merge_run_props(base: &RunProps, over: &RunProps) -> RunProps {
    let mut out = base.clone();
    overlay(&mut out, over);
    out
}

/// `clampLevel(n)`.
fn clamp_level(n: f64) -> f64 {
    if n.is_nan() || n < 1.0 {
        1.0
    } else if n > 6.0 {
        6.0
    } else {
        n
    }
}

fn empty_paragraph(ids: &mut IdAlloc) -> Paragraph {
    Paragraph::empty(ids.para())
}

impl<'p, 'd> Reader<'p, 'd> {
    fn max_chain(&self) -> u32 {
        self.limits.max_based_on_chain
    }

    fn resolve(&mut self, name: Option<&'d str>) -> Result<Arc<ResolvedStyle<'d>>, DocxError> {
        let max = self.max_chain();
        self.index.resolve_style(name, max)
    }

    /// Whether another level of block nesting may be opened.
    fn can_nest(&self) -> bool {
        self.depth < self.limits.max_nesting
    }

    /// The element read as plain text: one paragraph of its text (the fallback past the nesting
    /// limit).
    fn plain_paragraph(&mut self, el: Element<'d>) -> Paragraph {
        let text = el.text_content();
        let mut p = empty_paragraph(&mut self.ids);
        if !text.is_empty() {
            p.runs.push(Run { text: Utf16::from(text), props: Arc::new(default_run_props()), ..Run::default() });
        }
        p
    }

    /// `readBodyChild(el, blocks, ctx, listDepth)`.
    fn read_body_child(&mut self, el: Element<'d>, blocks: &mut Vec<OwnedBlock>, list_depth: u32) -> Result<(), DocxError> {
        match el.name() {
            "text:p" => {
                let p = self.read_paragraph(el, None)?;
                blocks.push(OwnedBlock::Paragraph(p));
            }
            "text:h" => {
                let p = self.read_heading(el)?;
                blocks.push(OwnedBlock::Paragraph(p));
            }
            "text:list" => {
                if self.can_nest() {
                    self.depth += 1;
                    let r = self.read_list(el, blocks, list_depth);
                    self.depth -= 1;
                    r?;
                } else {
                    let p = self.plain_paragraph(el);
                    blocks.push(OwnedBlock::Paragraph(p));
                }
            }
            "table:table" => {
                if self.can_nest() {
                    self.depth += 1;
                    let r = self.read_table(el);
                    self.depth -= 1;
                    blocks.push(OwnedBlock::Table(Arc::new(r?)));
                } else {
                    let p = self.plain_paragraph(el);
                    blocks.push(OwnedBlock::Paragraph(p));
                }
            }
            "text:section" => {
                if self.can_nest() {
                    self.depth += 1;
                    let mut r = Ok(());
                    for c in el.child_elements() {
                        r = self.read_body_child(c, blocks, list_depth);
                        if r.is_err() {
                            break;
                        }
                    }
                    self.depth -= 1;
                    r?;
                } else {
                    let p = self.plain_paragraph(el);
                    blocks.push(OwnedBlock::Paragraph(p));
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ───────────────────────── paragraphs / runs ─────────────────────────

    /// `readParagraph(pEl, ctx, list)`.
    fn read_paragraph(&mut self, p_el: Element<'d>, list: Option<(ListKind, u32)>) -> Result<Paragraph, DocxError> {
        let style_name = p_el.attr("text:style-name");
        let resolved = self.resolve(style_name)?;
        let mut props = para_props_from_style(&resolved, style_name);
        if let Some((kind, ilvl)) = list {
            props.numbering = Some(NumberingProps { kind: s(kind.as_str()), ilvl: num(f64::from(ilvl)), ..NumberingProps::default() });
        }
        self.apply_para_borders(style_name, &mut props)?;

        let mut runs: Vec<Run> = Vec::new();
        let base = base_run_props_for(&resolved);
        self.collect_runs(children_of(p_el), &mut runs, Arc::new(base.clone()), None)?;
        // the paragraph mark carries the body and font of the style: an empty paragraph measures by it
        let mut r_pr = RPr::default();
        if base.sz_half_pt.is_some_and(JsNumber::truthy) {
            r_pr.sz = base.sz_half_pt;
            r_pr.sz_cs = base.sz_half_pt;
        }
        if let Some(font) = base.font_ascii.as_ref().filter(|f| !f.is_empty()) {
            r_pr.r_fonts = Some(Box::new(RFonts {
                ascii: Some(font.clone()),
                h_ansi: Some(font.clone()),
                cs: Some(font.clone()),
                ..RFonts::default()
            }));
        }
        if base.bold == Some(true) {
            r_pr.b = Some(true);
        }
        if base.italic == Some(true) {
            r_pr.i = Some(true);
        }
        let ppr = if r_pr == RPr::default() { None } else { Some(Arc::new(PPr { r_pr: Some(Box::new(r_pr)), ..PPr::default() })) };
        Ok(Paragraph { runs, props: Arc::new(props), ppr, id: self.ids.para(), ..Paragraph::default() })
    }

    /// `readHeading(hEl, ctx)`.
    fn read_heading(&mut self, h_el: Element<'d>) -> Result<Paragraph, DocxError> {
        let level = clamp_level(parse_int(h_el.attr("text:outline-level").unwrap_or("1"), 10));
        let style_name = h_el.attr("text:style-name");
        let resolved = self.resolve(style_name)?;
        let mut props = para_props_from_style(&resolved, style_name);
        props.style_id = Some(format!("Heading{}", uo_jsnum::format::to_js_string(level)).into());
        self.apply_para_borders(style_name, &mut props)?;

        // the look of the level: the style resolved first, the bold and the size and colour of the level under it
        let mut base = base_run_props_for(&resolved);
        base.bold = Some(true);
        if base.sz_half_pt.is_none() {
            let idx = (to_uint32(level) as usize).saturating_sub(1);
            base.sz_half_pt = Some(num(HEADING_SIZES.get(idx).copied().unwrap_or(40.0)));
        }
        if base.color.is_none() {
            base.color = Some(s(HEADING_COLOR));
        }
        let mut runs: Vec<Run> = Vec::new();
        self.collect_runs(children_of(h_el), &mut runs, Arc::new(base), None)?;
        Ok(Paragraph { runs, props: Arc::new(props), id: self.ids.para(), ..Paragraph::default() })
    }

    /// `collectRuns(container, runs, ctx, base, pendingLink)` as a loop: one [`Frame`] per TS call.
    fn collect_runs(
        &mut self,
        container: Vec<Child<'d>>,
        runs: &mut Vec<Run>,
        base: Arc<RunProps>,
        link: Option<&'d str>,
    ) -> Result<(), DocxError> {
        let mut stack: Vec<Frame<'d>> = vec![Frame::new(container, base, link)];
        while let Some(top) = stack.last_mut() {
            let Some(&child) = top.items.get(top.pos) else {
                top.flush(runs);
                stack.pop();
                continue;
            };
            top.pos += 1;
            let el = match child {
                Child::Text(t) => {
                    top.buffer.extend(t.encode_utf16());
                    continue;
                }
                Child::Elem(e) => e,
            };
            match el.name() {
                "text:span" => {
                    top.flush(runs);
                    let (base, link) = (top.base.clone(), top.link);
                    let span_style = self.resolve(el.attr("text:style-name"))?;
                    let span_base = Arc::new(merge_run_props(&base, &span_style.run));
                    stack.push(Frame::new(children_of(el), span_base, link));
                }
                "text:a" => {
                    top.flush(runs);
                    let (base, link) = (top.base.clone(), top.link);
                    stack.push(Frame::new(children_of(el), base, el.attr("xlink:href").or(link)));
                }
                "text:s" => {
                    let count = parse_int(el.attr("text:c").unwrap_or("1"), 10);
                    let n = if count.is_nan() || count < 1.0 { 1.0 } else { count };
                    if n > MAX_SPACE_REPEAT {
                        return Err(DocxError::Corrupt("text:s repeat over the limit"));
                    }
                    let n = to_uint32(n) as usize;
                    top.buffer.resize(top.buffer.len() + n, 0x20);
                }
                "text:tab" => top.buffer.push(0x09),
                "text:line-break" => top.buffer.push(0x0a),
                "draw:frame" => {
                    top.flush(runs);
                    let (base, link) = (top.base.clone(), top.link);
                    if let Some(img) = self.read_image(el)? {
                        let mut run = Run { text: Utf16::from("\u{FFFC}"), props: base, ..Run::default() };
                        run.ext_mut().image = Some(Box::new(img));
                        runs.push(run);
                    } else if let Some(tb) = self.read_text_frame(el)? {
                        let mut run = Run { text: Utf16::from("\u{FFFC}"), props: base, ..Run::default() };
                        run.ext_mut().textbox = Some(Box::new(tb));
                        runs.push(run);
                    } else {
                        stack.push(Frame::new(children_of(el), base, link));
                    }
                }
                "office:annotation" => {
                    top.flush(runs);
                    self.read_annotation(el)?;
                }
                "text:note" => {
                    top.flush(runs);
                    let (base, link) = (top.base.clone(), top.link);
                    // footnote/endnote: the text of the note body stays inline (best effort)
                    if let Some(body) = el.first_child("text:note-body") {
                        let mut items: Vec<Child<'d>> = Vec::new();
                        for p in body.child_elements() {
                            items.extend(children_of(p));
                            items.push(Child::Text(" "));
                        }
                        stack.push(Frame::new(items, base, link));
                    }
                }
                "text:bookmark"
                | "text:bookmark-start"
                | "text:bookmark-end"
                | "text:annotation-end"
                | "text:soft-page-break"
                | "office:annotation-end" => {}
                _ => {
                    // unknown elements: recursion, so as not to lose nested text (no flush before it)
                    let (base, link) = (top.base.clone(), top.link);
                    stack.push(Frame::new(children_of(el), base, link));
                }
            }
        }
        Ok(())
    }

    /// `readAnnotation(el, ctx)`: a comment, its text the paragraphs' run texts joined by `\n`.
    fn read_annotation(&mut self, el: Element<'d>) -> Result<(), DocxError> {
        if !self.can_nest() {
            return Ok(());
        }
        self.depth += 1;
        let r = self.read_annotation_inner(el);
        self.depth -= 1;
        r
    }

    fn read_annotation_inner(&mut self, el: Element<'d>) -> Result<(), DocxError> {
        let author = el.first_child("dc:creator");
        let mut text: Vec<u16> = Vec::new();
        for (i, p) in el.children_named("text:p").enumerate() {
            let mut r: Vec<Run> = Vec::new();
            self.collect_runs(children_of(p), &mut r, Arc::new(default_run_props()), None)?;
            if i > 0 {
                text.push(0x0a);
            }
            for run in &r {
                text.extend_from_slice(run.text.units());
            }
        }
        let id = self.next_comment_id;
        self.next_comment_id = self.next_comment_id.saturating_add(1);
        let mut info = Comment { id: num(f64::from(id)), text: Utf16::from_units(text), ..Comment::default() };
        if let Some(a) = author {
            info.author = Some(a.text_content().into());
        }
        self.comments.push(info);
        Ok(())
    }

    // ───────────────────────── lists ─────────────────────────

    /// `readList(listEl, blocks, ctx, depth)`.
    fn read_list(&mut self, list_el: Element<'d>, blocks: &mut Vec<OwnedBlock>, depth: u32) -> Result<(), DocxError> {
        let ilvl = depth;
        let kind = self.resolve_list_kind(list_el, depth)?;
        for item in list_el.children_named("text:list-item") {
            self.read_list_item(item, blocks, depth, ilvl, kind)?;
        }
        // text:list-header (no marker): its paragraphs are at the current level without numbering
        for header in list_el.children_named("text:list-header") {
            for c in header.child_elements() {
                if c.name() == "text:p" || c.name() == "text:h" {
                    let p = self.read_paragraph(c, None)?;
                    blocks.push(OwnedBlock::Paragraph(p));
                }
            }
        }
        Ok(())
    }

    /// `readListItem(item, blocks, ctx, depth, ilvl, kind)`.
    fn read_list_item(
        &mut self,
        item: Element<'d>,
        blocks: &mut Vec<OwnedBlock>,
        depth: u32,
        ilvl: u32,
        kind: ListKind,
    ) -> Result<(), DocxError> {
        for c in item.child_elements() {
            if c.name() == "text:p" || c.name() == "text:h" {
                let p = self.read_paragraph(c, Some((kind, ilvl)))?;
                blocks.push(OwnedBlock::Paragraph(p));
            } else if c.name() == "text:list" {
                if self.can_nest() {
                    self.depth += 1;
                    let r = self.read_list(c, blocks, depth.saturating_add(1));
                    self.depth -= 1;
                    r?;
                } else {
                    let p = self.plain_paragraph(c);
                    blocks.push(OwnedBlock::Paragraph(p));
                }
            }
        }
        Ok(())
    }

    /// `resolveListKind(ctx, listEl, depth)`: bullet against decimal for a level, from the list's
    /// own `text:style-name`, else the `style:list-style-name` the style of a paragraph of it
    /// inherits; bullet when nothing matches.
    fn resolve_list_kind(&mut self, list_el: Element<'d>, depth: u32) -> Result<ListKind, DocxError> {
        let level = f64::from(depth) + 1.0;
        let direct = list_el.attr("text:style-name");
        if let Some(k) = self.lookup_list_kind(direct, level) {
            return Ok(k);
        }
        for item in list_el.children_named("text:list-item") {
            for c in item.children_named("text:p") {
                let resolved = self.resolve(c.attr("text:style-name"))?;
                if let Some(k) = self.lookup_list_kind(resolved.list_style_name, level) {
                    return Ok(k);
                }
            }
        }
        Ok(ListKind::Bullet)
    }

    fn lookup_list_kind(&self, list_style_name: Option<&str>, level: f64) -> Option<ListKind> {
        let name = list_style_name.filter(|n| !n.is_empty())?;
        let levels = self.index.list_styles.get(name)?;
        levels.get(&crate::js::NumKey::new(level)).or_else(|| levels.get(&crate::js::NumKey::new(1.0))).copied()
    }

    // ───────────────────────── tables ─────────────────────────

    /// `readTable(tblEl, ctx)`.
    fn read_table(&mut self, tbl_el: Element<'d>) -> Result<Table, DocxError> {
        let col_widths = self.read_columns(tbl_el)?;
        let mut rows: Vec<Row> = Vec::new();
        for row_el in collect_rows(tbl_el) {
            let mut cells: Vec<Cell> = Vec::new();
            for cell_el in row_el.child_elements() {
                if cell_el.name() == "table:table-cell" {
                    cells.push(self.read_cell(cell_el)?);
                }
                // table:covered-table-cell: covered by a horizontal span (the gridSpan of the cell before is it)
            }
            if !cells.is_empty() {
                rows.push(Row { cells, ..Row::default() });
            }
        }
        // column widths: style:column-width when there is one, else shared out
        let mut total_cols = col_widths.len() as f64;
        for r in &rows {
            let mut sum = 0.0;
            for c in &r.cells {
                sum += c.grid_span.to_number();
            }
            total_cols = max(total_cols, sum);
        }
        let grid_cols = build_grid_cols(&col_widths, total_cols)?;
        Ok(Table { rows, grid_cols_twips: grid_cols.into_iter().map(num).collect(), id: self.ids.table(), ..Table::default() })
    }

    /// `readColumns(tblEl, ctx)`: the widths the `table:table-column`s declare (with repeats), in
    /// document order, through the `table:table-columns` / `table:table-header-columns` groups
    /// (a loop over an explicit stack where the TS recurses).
    fn read_columns(&self, tbl_el: Element<'d>) -> Result<Vec<Option<f64>>, DocxError> {
        let mut widths: Vec<Option<f64>> = Vec::new();
        let mut stack: Vec<Element<'d>> = tbl_el.child_elements().collect();
        stack.reverse();
        while let Some(c) = stack.pop() {
            match c.name() {
                "table:table-column" => {
                    let repeat = parse_int(c.attr("table:number-columns-repeated").unwrap_or("1"), 10);
                    let w = column_width(c, &self.index);
                    let n = if repeat.is_nan() || repeat < 1.0 { 1.0 } else { repeat };
                    if widths.len() as f64 + n > MAX_COLUMNS {
                        return Err(DocxError::Corrupt("table columns over the limit"));
                    }
                    let n = to_uint32(n) as usize;
                    widths.resize(widths.len() + n, w);
                }
                "table:table-columns" | "table:table-header-columns" => {
                    let before = stack.len();
                    stack.extend(c.child_elements());
                    if let Some(tail) = stack.get_mut(before..) {
                        tail.reverse();
                    }
                }
                _ => {}
            }
        }
        Ok(widths)
    }

    /// `readCell(cellEl, ctx)`.
    fn read_cell(&mut self, cell_el: Element<'d>) -> Result<Cell, DocxError> {
        let grid_span = parse_int(cell_el.attr("table:number-columns-spanned").unwrap_or("1"), 10);
        let mut inner: Vec<OwnedBlock> = Vec::new();
        for c in cell_el.child_elements() {
            self.read_body_child(c, &mut inner, 0)?;
        }
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        let mut tables: Vec<Arc<Table>> = Vec::new();
        for b in inner {
            match b {
                OwnedBlock::Paragraph(p) => paragraphs.push(p),
                OwnedBlock::Table(t) => tables.push(t),
            }
        }
        if paragraphs.is_empty() && tables.is_empty() {
            paragraphs.push(empty_paragraph(&mut self.ids));
        }
        let mut content = Story::new();
        for p in paragraphs {
            content.push_paragraph(p);
        }
        for t in tables {
            content.push_table(t);
        }
        let mut cell =
            Cell { grid_span: num(if grid_span.is_nan() || grid_span < 1.0 { 1.0 } else { grid_span }), content, ..Cell::default() };
        let cell_style = cell_el.attr("table:style-name");
        if let Some(shading) = cell_shading(&self.index, cell_style, self.max_chain())? {
            cell.shading_hex = Some(shading);
        }
        if let Some(borders) = cell_borders(&self.index, cell_style, self.max_chain())? {
            cell.borders = Some(Box::new(borders));
        }
        Ok(cell)
    }

    // ───────────────────────── images and frames ─────────────────────────

    /// `readImage(frame, ctx)`.
    fn read_image(&mut self, frame: Element<'d>) -> Result<Option<InlineImage>, DocxError> {
        let Some(image_el) = frame.first_child("draw:image") else { return Ok(None) };

        let mut data: Option<Vec<u8>> = None;
        let mut ext = s("png");
        if let Some(href) = image_el.attr("xlink:href").filter(|h| !h.is_empty()) {
            let path = href.strip_prefix("./").unwrap_or(href);
            data = self.pkg.part(path)?.map(<[u8]>::to_vec);
            if let Some(e) = path.rsplit('.').next().filter(|e| !e.is_empty()) {
                ext = uo_text::case::to_lowercase_str(e).into();
            }
        }
        if data.is_none() {
            // image embedded in base64 (office:binary-data)
            if let Some(binary) = image_el.first_child("office:binary-data") {
                let b64: String = binary.text_content().chars().filter(|c| !is_js_space(*c)).collect();
                data = Some(decode_base64(&b64)?);
            }
        }
        let Some(data) = data else { return Ok(None) };

        let width = length_to_twips(frame.attr("svg:width")).unwrap_or(0.0);
        let height = length_to_twips(frame.attr("svg:height")).unwrap_or(0.0);
        let id = self.media.add(&data);
        Ok(Some(InlineImage {
            data: MediaRef(id),
            ext,
            width_twips: num(width),
            height_twips: num(height),
            drawing: self.ids.drawing(),
            ..InlineImage::default()
        }))
    }

    /// `readTextFrame(frame, ctx)`: a text frame (`draw:frame` + `draw:text-box`) as a text box with
    /// its paragraphs and styles, placed as Word opens it. No graphic style: no inner margins, no
    /// outline, text not pushed away. `None` (the frame's text goes inline) when it holds no text
    /// box, or past the nesting limit.
    fn read_text_frame(&mut self, frame: Element<'d>) -> Result<Option<DrawingTextbox>, DocxError> {
        let Some(bx) = frame.first_child("draw:text-box") else { return Ok(None) };
        if !self.can_nest() {
            return Ok(None);
        }
        self.depth += 1;
        let r = self.read_text_frame_inner(frame, bx);
        self.depth -= 1;
        r.map(Some)
    }

    fn read_text_frame_inner(&mut self, frame: Element<'d>, bx: Element<'d>) -> Result<DrawingTextbox, DocxError> {
        let mut blocks: Vec<OwnedBlock> = Vec::new();
        for c in bx.child_elements() {
            self.read_body_child(c, &mut blocks, 0)?;
        }
        if blocks.is_empty() {
            blocks.push(OwnedBlock::Paragraph(empty_paragraph(&mut self.ids)));
        }
        let gp: Option<Element<'d>> =
            self.index.styles.get(frame.attr("draw:style-name").unwrap_or("")).and_then(|st: &RawStyle<'d>| st.graphic);
        let ga = |k: &str| gp.and_then(|g| g.attr(k));
        let pad = |side: &str| {
            let specific = ga(&format!("fo:padding-{side}"));
            length_to_twips(specific.or_else(|| ga("fo:padding"))).unwrap_or(0.0)
        };
        let mut content = Story::new();
        for b in blocks {
            content.push_block(b);
        }
        let mut tb = DrawingTextbox {
            width_twips: num(length_to_twips(frame.attr("svg:width").or_else(|| bx.attr("fo:min-width"))).unwrap_or(0.0)),
            height_twips: num(length_to_twips(frame.attr("svg:height").or_else(|| bx.attr("fo:min-height"))).unwrap_or(0.0)),
            insets: Some(Ltrb { l: num(pad("left")), t: num(pad("top")), r: num(pad("right")), b: num(pad("bottom")), ..Ltrb::default() }),
            content,
            // `paras`: the top-level paragraphs of the blocks (odf/index.ts:594)
            view: TextboxView::TopLevel,
            drawing: self.ids.drawing(),
            ..DrawingTextbox::default()
        };
        let at = frame.attr("text:anchor-type").unwrap_or("paragraph");
        if at == "as-char" {
            return Ok(tb);
        }
        // style:wrap: none = above and below; run-through = without pushing away; the others = on the sides
        let w = ga("style:wrap");
        let wrap = match w {
            Some("none") => "topAndBottom",
            None | Some("run-through") => "none",
            _ => "square",
        };
        let page_default = if at == "page" { "page" } else { "paragraph" };
        let h_rel = ga("style:horizontal-rel").unwrap_or(page_default);
        let v_rel = ga("style:vertical-rel").unwrap_or(page_default);
        let h_pos = ga("style:horizontal-pos");
        let v_pos = ga("style:vertical-pos");
        tb.floating = Some(true);
        let mut anchor = DrawingAnchor { wrap: s(wrap), ..DrawingAnchor::default() };
        if wrap == "square" {
            anchor.wrap_text = Some(s(match w {
                Some("left") => "left",
                Some("right") => "right",
                Some("biggest" | "dynamic") => "largest",
                _ => "bothSides",
            }));
        }
        if w == Some("run-through") && ga("style:run-through") == Some("background") {
            anchor.behind_doc = Some(true);
        }
        anchor.pos_h = AnchorPos {
            relative_from: s(match h_rel {
                "page" => "page",
                "page-content" => "margin",
                "char" => "character",
                _ => "column",
            }),
            ..AnchorPos::default()
        };
        match h_pos.filter(|p| !p.is_empty() && *p != "from-left") {
            Some(p) => {
                anchor.pos_h.align = Some(s(if p == "center" {
                    "center"
                } else if p == "right" {
                    "right"
                } else {
                    "left"
                }))
            }
            None => anchor.pos_h.offset = Some(num(length_to_twips(frame.attr("svg:x")).unwrap_or(0.0))),
        }
        anchor.pos_v = AnchorPos {
            relative_from: s(match v_rel {
                "page" => "page",
                "page-content" => "margin",
                "line" | "char" => "line",
                _ => "paragraph",
            }),
            ..AnchorPos::default()
        };
        match v_pos.filter(|p| !p.is_empty() && *p != "from-top") {
            Some(p) => {
                anchor.pos_v.align = Some(s(if p == "middle" {
                    "center"
                } else if p == "bottom" {
                    "bottom"
                } else {
                    "top"
                }))
            }
            None => anchor.pos_v.offset = Some(num(length_to_twips(frame.attr("svg:y")).unwrap_or(0.0))),
        }
        // `parseInt(…) || 0`
        let z = parse_int(frame.attr("draw:z-index").unwrap_or("0"), 10);
        anchor.relative_height = Some(num(if z.is_nan() || z == 0.0 { 0.0 } else { z }));
        anchor.layout_in_cell = Some(true);
        anchor.allow_overlap = Some(true);
        tb.anchor = Some(Box::new(anchor));
        Ok(tb)
    }

    // ───────────────────────── borders and shading ─────────────────────────

    /// `applyParaBorders(index, styleName, props)`.
    fn apply_para_borders(&self, style_name: Option<&'d str>, props: &mut ParaProps) -> Result<(), DocxError> {
        if let Some(b) = borders_from_style(&self.index, style_name, self.max_chain())? {
            props.borders = Some(Box::new(b));
        }
        Ok(())
    }

    // ───────────────────────── StylesMap for the editor ─────────────────────────

    /// `buildStylesMap(index)`: the styles resolved, `Heading 1` as `Heading1` (and under its own
    /// name), plus entries `Heading1`..`Heading6` with a reasonable look, so that the editor resolves
    /// headings even without a matching style.
    fn build_styles_map(&mut self) -> Result<JsMap<Str, ModelStyle>, DocxError> {
        let mut out: JsMap<Str, ModelStyle> = JsMap::new();
        let names: Vec<&'d str> = self.index.styles.keys().copied().collect();
        for name in names {
            let resolved = self.resolve(Some(name))?;
            let id = normalize_style_id(name);
            let entry = ModelStyle { run: resolved.run.clone(), para: resolved.para.clone(), ..ModelStyle::default() };
            if id != name {
                out.insert(id.into(), entry.clone());
                out.insert(name.into(), entry);
            } else {
                out.insert(id.into(), entry);
            }
        }
        for (i, size) in HEADING_SIZES.iter().enumerate() {
            let id = format!("Heading{}", i + 1);
            if !out.contains_key(id.as_str()) {
                let run = RunProps { bold: Some(true), sz_half_pt: Some(num(*size)), color: Some(s(HEADING_COLOR)), ..RunProps::default() };
                out.insert(id.into(), ModelStyle { run, para: ParaProps::default(), ..ModelStyle::default() });
            }
        }
        Ok(out)
    }
}

/// `normalizeStyleId(name)`: `name.replace(/\s+/g, '')`.
fn normalize_style_id(name: &str) -> String {
    name.chars().filter(|c| !is_js_space(*c)).collect()
}

/// `paraPropsFromStyle(resolved, styleId)`: the paragraph properties of the resolved style (no run).
fn para_props_from_style(resolved: &ResolvedStyle<'_>, style_id: Option<&str>) -> ParaProps {
    let mut p = ParaProps::default();
    let st = &resolved.para;
    if st.alignment.is_some() {
        p.alignment.clone_from(&st.alignment);
    }
    p.indent_left_twips = st.indent_left_twips;
    p.indent_right_twips = st.indent_right_twips;
    p.indent_first_line_twips = st.indent_first_line_twips;
    p.space_before_twips = st.space_before_twips;
    p.space_after_twips = st.space_after_twips;
    p.shading_hex.clone_from(&st.shading_hex);
    if st.page_break_before == Some(true) {
        p.page_break_before = Some(true);
    }
    if let Some(id) = style_id.filter(|i| !i.is_empty()) {
        p.style_id = Some(s(id));
    }
    p
}

/// `baseRunPropsFor(resolved)`: `DEFAULT_RUN_PROPS` and the run properties of the paragraph's style.
fn base_run_props_for(resolved: &ResolvedStyle<'_>) -> RunProps {
    merge_run_props(&default_run_props(), &resolved.run)
}

/// `collectRows(tblEl)`: `table:table-header-rows` and `table:table-row-group` flattened into the
/// rows (one level, as the TS).
fn collect_rows(tbl_el: Element<'_>) -> Vec<Element<'_>> {
    let mut out = Vec::new();
    for c in tbl_el.child_elements() {
        match c.name() {
            "table:table-row" => out.push(c),
            "table:table-header-rows" | "table:table-row-group" => out.extend(c.children_named("table:table-row")),
            _ => {}
        }
    }
    out
}

/// `columnWidth(colEl, index)`: `style:column-width` of the column's style.
fn column_width(col_el: Element<'_>, index: &OdfStyleIndex<'_>) -> Option<f64> {
    let style_name = col_el.attr("table:style-name").filter(|n| !n.is_empty())?;
    let col_props = index.styles.get(style_name)?.column?;
    length_to_twips(col_props.attr("style:column-width"))
}

/// `buildGridCols(colWidths, totalCols)`.
fn build_grid_cols(col_widths: &[Option<f64>], total_cols: f64) -> Result<Vec<f64>, DocxError> {
    let n = max(max(total_cols, col_widths.len() as f64), 1.0);
    if n > MAX_COLUMNS {
        return Err(DocxError::Corrupt("table columns over the limit"));
    }
    let known: Vec<f64> = col_widths.iter().filter_map(|w| *w).collect();
    if known.len() as f64 == n {
        return Ok(known);
    }
    // the default width is shared out equally among the columns without one
    let each = round(DEFAULT_CONTENT_WIDTH_TWIPS / n);
    let count = to_uint32(n) as usize;
    Ok((0..count).map(|i| col_widths.get(i).copied().flatten().unwrap_or(each)).collect())
}

/// `decodeBase64(b64)`: `atob(b64)` as bytes (the forgiving base64 of the Infra standard, with the
/// checks of Node's `atob`): no `=` but at the end (two at most, and only when the length is a
/// multiple of 4), no remainder of 1.
fn decode_base64(b64: &str) -> Result<Vec<u8>, DocxError> {
    const BAD: DocxError = DocxError::Corrupt("image data is not valid base64");
    let mut vals: Vec<u8> = Vec::with_capacity(b64.len());
    let mut equals = 0usize;
    for &b in b64.as_bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                equals += 1;
                if equals > 2 {
                    return Err(BAD);
                }
                continue;
            }
            _ => return Err(BAD),
        };
        if equals > 0 {
            return Err(BAD);
        }
        vals.push(v);
    }
    let total = vals.len() + equals;
    if !total.is_multiple_of(4) && equals > 0 {
        return Err(BAD);
    }
    if (total - equals) % 4 == 1 {
        return Err(BAD);
    }
    let mut out = Vec::with_capacity(vals.len() / 4 * 3 + 2);
    for chunk in vals.chunks(4) {
        let a = chunk.first().copied().unwrap_or(0);
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let d = chunk.get(3).copied().unwrap_or(0);
        out.push((a << 2) | (b >> 4));
        if chunk.len() > 2 {
            out.push(((b & 0x0f) << 4) | (c >> 2));
        }
        if chunk.len() > 3 {
            out.push(((c & 0x03) << 6) | d);
        }
    }
    Ok(out)
}

// ───────────────────────── borders / shading ─────────────────────────

fn is_hex6(part: &str) -> bool {
    let h = part.strip_prefix('#').unwrap_or(part);
    h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `parseBorder(spec)`: `0.5pt solid #000000` as a [`BorderEdge`] (`sizeEighthPt` = pt × 8).
fn parse_border(spec: Option<&str>) -> Option<BorderEdge> {
    let spec = spec.filter(|sp| !sp.is_empty() && *sp != "none")?;
    let mut width_twips: Option<f64> = None;
    let mut style = "single";
    let mut color: Option<Str> = None;
    for part in split_js_space(trim(spec)) {
        if let Some(w) = length_to_twips(Some(part)) {
            width_twips = Some(w);
            continue;
        }
        if is_hex6(part) {
            color = Some(norm_hex(part));
            continue;
        }
        match part {
            "solid" => style = "single",
            "double" => style = "double",
            "dashed" => style = "dashed",
            "dotted" => style = "dotted",
            "none" | "hidden" => return None,
            _ => {}
        }
    }
    let pt = width_twips.map_or(0.5, |w| w / 20.0);
    let mut edge =
        BorderEdge { style: s(style), size_eighth_pt: num(max(2.0, round(pt * 8.0))), color, space_pt: num(0.0), ..BorderEdge::default() };
    if edge.color.is_none() {
        edge.mark_undefined("color");
    }
    Some(edge)
}

/// `bordersFromStyle(index, styleName)`: `fo:border` / `fo:border-{side}` of the paragraph
/// properties of the style, climbing the parent chain while a style has no borders (a loop; a chain
/// longer than `max_chain`, or a cycle, is a limit error, where the TS overflows its stack).
fn borders_from_style(index: &OdfStyleIndex<'_>, style_name: Option<&str>, max_chain: u32) -> Result<Option<ParagraphBorders>, DocxError> {
    let mut cur = style_name.filter(|n| !n.is_empty());
    let mut steps: u64 = 0;
    while let Some(name) = cur {
        let Some(raw) = index.styles.get(name) else { return Ok(None) };
        if let Some(para_el) = raw.para {
            let all = parse_border(para_el.attr("fo:border"));
            let mut out = ParagraphBorders::default();
            let mut any = false;
            for (side, attr) in
                [("top", "fo:border-top"), ("left", "fo:border-left"), ("bottom", "fo:border-bottom"), ("right", "fo:border-right")]
            {
                let edge = parse_border(para_el.attr(attr)).or_else(|| all.clone());
                if let Some(e) = edge {
                    match side {
                        "top" => out.top = Some(e),
                        "left" => out.left = Some(e),
                        "bottom" => out.bottom = Some(e),
                        _ => out.right = Some(e),
                    }
                    any = true;
                }
            }
            if any {
                return Ok(Some(out));
            }
        }
        cur = raw.parent.filter(|p| !p.is_empty());
        steps += 1;
        if steps > u64::from(max_chain) {
            return Err(DocxError::Limit { kind: crate::error::DocxLimitKind::BasedOnChain, max: u64::from(max_chain), part: None });
        }
    }
    Ok(None)
}

/// `cellPropsEl(index, styleName)`: the `style:table-cell-properties` of a style, climbing the
/// parent chain (with a seen set, as the TS).
fn cell_props_el<'d>(index: &OdfStyleIndex<'d>, style_name: Option<&'d str>, max_chain: u32) -> Result<Option<Element<'d>>, DocxError> {
    let mut seen: std::collections::HashSet<&'d str> = std::collections::HashSet::new();
    let mut cur = style_name.filter(|n| !n.is_empty());
    while let Some(name) = cur {
        if !seen.insert(name) {
            return Ok(None);
        }
        let Some(raw) = index.styles.get(name) else { return Ok(None) };
        if let Some(cell) = raw.cell {
            return Ok(Some(cell));
        }
        cur = raw.parent.filter(|p| !p.is_empty());
        if seen.len() as u64 > u64::from(max_chain) {
            return Err(DocxError::Limit { kind: crate::error::DocxLimitKind::BasedOnChain, max: u64::from(max_chain), part: None });
        }
    }
    Ok(None)
}

/// `cellShading(index, styleName)`.
fn cell_shading<'d>(index: &OdfStyleIndex<'d>, style_name: Option<&'d str>, max_chain: u32) -> Result<Option<Str>, DocxError> {
    let el = cell_props_el(index, style_name, max_chain)?;
    Ok(el.and_then(|e| e.attr("fo:background-color")).filter(|bg| !bg.is_empty() && *bg != "transparent").map(norm_hex))
}

/// `cellBorders(index, styleName)`: cell borders from `style:table-cell-properties` (`fo:border*`).
fn cell_borders<'d>(index: &OdfStyleIndex<'d>, style_name: Option<&'d str>, max_chain: u32) -> Result<Option<TableBorders>, DocxError> {
    let Some(el) = cell_props_el(index, style_name, max_chain)? else { return Ok(None) };
    let all = parse_border(el.attr("fo:border"));
    let mut out = TableBorders::default();
    let mut any = false;
    for (side, attr) in [("top", "fo:border-top"), ("left", "fo:border-left"), ("bottom", "fo:border-bottom"), ("right", "fo:border-right")]
    {
        let edge = parse_border(el.attr(attr)).or_else(|| all.clone());
        if let Some(e) = edge {
            match side {
                "top" => out.top = Some(e),
                "left" => out.left = Some(e),
                "bottom" => out.bottom = Some(e),
                _ => out.right = Some(e),
            }
            any = true;
        }
    }
    Ok(any.then_some(out))
}

// ───────────────────────── page geometry ─────────────────────────

/// `findPageLayout(root)`.
fn find_page_layout(root: Option<Element<'_>>) -> Option<Element<'_>> {
    let root = root?;
    if let Some(found) = root.first_child("office:automatic-styles").and_then(|a| a.first_child("style:page-layout")) {
        return Some(found);
    }
    // also `office:styles` (rare)
    root.first_child("office:styles").and_then(|st| st.first_child("style:page-layout"))
}

/// `readPageGeometry(stylesRoot, contentRoot)`.
fn read_page_geometry(styles_root: Option<Element<'_>>, content_root: Element<'_>) -> SectPr {
    let layout = find_page_layout(styles_root).or_else(|| find_page_layout(Some(content_root)));
    let Some(layout) = layout else { return default_sect_pr() };
    let Some(props_el) = layout.first_child("style:page-layout-properties") else { return default_sect_pr() };
    let d = default_sect_pr();
    let get = |attr: &str, default: JsNumber| num(length_to_twips(props_el.attr(attr)).unwrap_or(default.to_number()));
    SectPr {
        page_width_twips: get("fo:page-width", d.page_width_twips),
        page_height_twips: get("fo:page-height", d.page_height_twips),
        margin_left_twips: get("fo:margin-left", d.margin_left_twips),
        margin_right_twips: get("fo:margin-right", d.margin_right_twips),
        margin_top_twips: get("fo:margin-top", d.margin_top_twips),
        margin_bottom_twips: get("fo:margin-bottom", d.margin_bottom_twips),
        ..SectPr::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_follows_atob() {
        assert_eq!(decode_base64("").ok(), Some(vec![]));
        assert_eq!(decode_base64("QQ==").ok(), Some(vec![b'A']));
        assert_eq!(decode_base64("QUI=").ok(), Some(b"AB".to_vec()));
        assert_eq!(decode_base64("QUJD").ok(), Some(b"ABC".to_vec()));
        assert_eq!(decode_base64("QUJDRA").ok(), Some(b"ABCD".to_vec()));
        assert!(decode_base64("Q").is_err());
        assert!(decode_base64("QUJDR").is_err());
        assert!(decode_base64("QQ=").is_err());
        assert!(decode_base64("Q=Q=").is_err());
        assert!(decode_base64("====").is_err());
        assert!(decode_base64("QQ*=").is_err());
    }

    #[test]
    fn style_ids_lose_white_space() {
        assert_eq!(normalize_style_id("Heading 1"), "Heading1");
        assert_eq!(normalize_style_id("a\u{a0}b\tc"), "abc");
    }
}
