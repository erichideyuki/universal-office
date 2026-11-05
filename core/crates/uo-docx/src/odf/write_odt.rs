//! docx-core `odf/write-odt.ts`: `writeOdt`, the OpenDocument Text writer. It takes the document and
//! gives the bytes of a valid ODF package to open in LibreOffice or Office:
//!
//! - `mimetype` (stored, the FIRST entry: an ODF requirement);
//! - `content.xml`: `office:automatic-styles` and `office:body/office:text`;
//! - `styles.xml`: the master style and the page layout (geometry of section 0);
//! - `META-INF/manifest.xml`: every part of the zip;
//! - `Pictures/imgN.<ext>`: the raw bytes of the images.
//!
//! Automatic styles: each distinct combination of run or paragraph properties is an entry of
//! `office:automatic-styles` (`R1`, `R2`…, `P1`, `P2`…); a text span refers to it by
//! `text:style-name`. Headings are `text:h` with `text:outline-level`.
//!
//! The TS builds JS strings; a text run can hold a lone surrogate, and two runs of plain text are
//! concatenated without markup in between, so a high surrogate at the end of one and a low one at
//! the start of the next form a pair in the string. The body is therefore built in UTF-16 units and
//! encoded once, a lone surrogate becoming U+FFFD, as `TextEncoder` does ([`Out`]).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use uo_jsnum::format::{to_fixed, to_js_string};
use uo_jsnum::text::is_js_space_unit;
use uo_opc::zip::{self, WriteOptions};

use crate::error::DocxError;
use crate::final_view::final_paragraphs;
use crate::js::{JsMap, JsNumber, NumKey};
use crate::model::media::MediaStore;
use crate::model::para::{BorderEdge, ParagraphBorders};
use crate::model::run::{DrawingTextbox, InlineImage, RunProps};
use crate::model::section::SectPr;
use crate::model::story::{Story, TableAnchor};
use crate::model::table::{Cell, Table};
use crate::model::{Comment, Document, ParaProps, Paragraph, Run};
use crate::pvec::PVec;
use crate::u16buf::U16Buf;

use super::reader::{MAX_COLUMNS, ODT_MIMETYPE};

/// How `writeOdt` treats images (`WriteOdtOptions.images`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OdtImages {
    /// include them in `Pictures/` (default)
    #[default]
    Embed,
    /// drop them
    Omit,
}

/// `WriteOdtOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OdtOptions {
    pub images: OdtImages,
}

/// How deep tables and text boxes may nest in what the writer is given (the reader stops its own
/// nesting at 64 and reads the rest as text); past it the save fails, where the TS recurses until
/// its stack ends.
const MAX_WRITE_DEPTH: u32 = 256;

/// The body being built: UTF-16 units (see the module comment).
type Out = U16Buf;

fn put(out: &mut Out, s: &str) {
    out.push_str(s);
}

fn fixed(x: f64, digits: u32) -> String {
    to_fixed(x, digits).unwrap_or_default()
}

/// `String(n)` of a number field: a `null` (after an undo round trip) is the string `null`.
fn js_str(n: JsNumber) -> String {
    match n {
        JsNumber::Num(x) => to_js_string(x),
        JsNumber::Null => "null".to_owned(),
    }
}

/// `x ?? ''` of a number field in a template or a key: absent and `null` are the empty string.
fn num_or_empty(n: Option<JsNumber>) -> String {
    match n {
        Some(JsNumber::Num(x)) => to_js_string(x),
        _ => String::new(),
    }
}

fn truthy_num(n: Option<JsNumber>) -> bool {
    n.is_some_and(JsNumber::truthy)
}

/// A string field that is truthy in JS (present and not empty).
fn truthy_str(s: &Option<crate::js::Str>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty())
}

/// `xmlEscape(s)`.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn twips_to_pt(t: f64) -> String {
    format!("{}pt", fixed(t / 20.0, 2))
}

fn twips_to_cm(t: f64) -> String {
    format!("{}cm", fixed(t / 1440.0 * 2.54, 4))
}

fn half_pt_to_pt(hp: f64) -> String {
    format!("{}pt", fixed(hp / 2.0, 2))
}

fn eighth_pt_to_pt(ep: f64) -> f64 {
    ep / 8.0
}

// ───────────────────────── automatic styles ─────────────────────────

/// `runStyleKey(p)`.
fn run_style_key(p: &RunProps) -> String {
    let flag = |b: Option<bool>, on: &'static str| if b == Some(true) { on } else { "" };
    [
        flag(p.bold, "B").to_owned(),
        flag(p.italic, "I").to_owned(),
        flag(p.underline, "U").to_owned(),
        p.underline_style.as_deref().unwrap_or("").to_owned(),
        p.underline_color.as_deref().unwrap_or("").to_owned(),
        flag(p.strike, "S").to_owned(),
        flag(p.double_strike, "DS").to_owned(),
        p.vert_align.as_deref().unwrap_or("").to_owned(),
        num_or_empty(p.sz_half_pt),
        p.color.as_deref().unwrap_or("").to_owned(),
        p.highlight.as_deref().unwrap_or("").to_owned(),
        p.font_ascii.as_deref().unwrap_or("").to_owned(),
        flag(p.all_caps, "C").to_owned(),
        flag(p.small_caps, "SC").to_owned(),
        num_or_empty(p.char_spacing_twips),
    ]
    .join("|")
}

const HIGHLIGHTS: [(&str, &str); 16] = [
    ("yellow", "#ffff00"),
    ("green", "#00ff00"),
    ("cyan", "#00ffff"),
    ("magenta", "#ff00ff"),
    ("blue", "#0000ff"),
    ("red", "#ff0000"),
    ("darkBlue", "#000080"),
    ("darkCyan", "#008080"),
    ("darkGreen", "#008000"),
    ("darkMagenta", "#800080"),
    ("darkRed", "#800000"),
    ("darkYellow", "#808000"),
    ("darkGray", "#808080"),
    ("lightGray", "#c0c0c0"),
    ("black", "#000000"),
    ("white", "#ffffff"),
];

fn highlight_to_hex(name: &str) -> String {
    match HIGHLIGHTS.iter().find(|(k, _)| *k == name) {
        Some((_, v)) => (*v).to_owned(),
        None if name.starts_with('#') => name.to_owned(),
        None => format!("#{name}"),
    }
}

/// `runStyleXml(name, p)`.
fn run_style_xml(name: &str, p: &RunProps) -> String {
    let mut text: Vec<String> = Vec::new();
    if p.bold == Some(true) {
        text.push("fo:font-weight=\"bold\"".into());
    }
    if p.italic == Some(true) {
        text.push("fo:font-style=\"italic\"".into());
    }
    if p.underline == Some(true) {
        let u = match p.underline_style.as_deref() {
            Some("double") => "double",
            Some("dotted") => "dotted",
            Some("dash") => "dash",
            Some("wave") => "wave",
            _ => "solid",
        };
        text.push(format!("style:text-underline-style=\"{u}\""));
        text.push("style:text-underline-width=\"auto\"".into());
        match truthy_str(&p.underline_color) {
            Some(c) => text.push(format!("style:text-underline-color=\"#{c}\"")),
            None => text.push("style:text-underline-color=\"font-color\"".into()),
        }
    }
    if p.strike == Some(true) || p.double_strike == Some(true) {
        text.push("style:text-line-through-style=\"solid\"".into());
        if p.double_strike == Some(true) {
            text.push("style:text-line-through-type=\"double\"".into());
        }
    }
    match p.vert_align.as_deref() {
        Some("superscript") => text.push("style:text-position=\"super 58%\"".into()),
        Some("subscript") => text.push("style:text-position=\"sub 58%\"".into()),
        _ => {}
    }
    if truthy_num(p.sz_half_pt) {
        text.push(format!("fo:font-size=\"{}\"", half_pt_to_pt(p.sz_half_pt.map_or(0.0, JsNumber::to_number))));
    }
    if let Some(c) = truthy_str(&p.color) {
        text.push(format!("fo:color=\"#{c}\""));
    }
    if let Some(h) = truthy_str(&p.highlight) {
        text.push(format!("fo:background-color=\"{}\"", highlight_to_hex(h)));
    }
    if let Some(f) = truthy_str(&p.font_ascii) {
        text.push(format!("style:font-name=\"{}\"", xml_escape(f)));
    }
    if p.all_caps == Some(true) {
        text.push("fo:text-transform=\"uppercase\"".into());
    }
    if p.small_caps == Some(true) {
        text.push("fo:font-variant=\"small-caps\"".into());
    }
    if truthy_num(p.char_spacing_twips) {
        text.push(format!("fo:letter-spacing=\"{}\"", twips_to_pt(p.char_spacing_twips.map_or(0.0, JsNumber::to_number))));
    }
    let attrs = if text.is_empty() { String::new() } else { format!(" {}", text.join(" ")) };
    format!("<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties{attrs}/></style:style>")
}

/// `borderKey(b)`.
fn border_key(b: Option<&ParagraphBorders>) -> String {
    let Some(b) = b else { return String::new() };
    let e = |x: &Option<BorderEdge>| match x {
        Some(x) => format!("{}-{}-{}", x.style, js_str(x.size_eighth_pt), x.color.as_deref().unwrap_or("")),
        None => String::new(),
    };
    format!("{}/{}/{}/{}", e(&b.top), e(&b.right), e(&b.bottom), e(&b.left))
}

/// `paraStyleKey(p)`.
fn para_style_key(p: &ParaProps) -> String {
    [
        p.alignment.as_deref().unwrap_or("").to_owned(),
        num_or_empty(p.indent_left_twips),
        num_or_empty(p.indent_right_twips),
        num_or_empty(p.indent_first_line_twips),
        num_or_empty(p.space_before_twips),
        num_or_empty(p.space_after_twips),
        num_or_empty(p.line_twentieths),
        p.shading_hex.as_deref().unwrap_or("").to_owned(),
        if p.page_break_before == Some(true) { "PBB".to_owned() } else { String::new() },
        border_key(p.borders.as_deref()),
    ]
    .join("|")
}

/// `cssStyle(s)`.
fn css_style(s: &str) -> &'static str {
    match s {
        "dashed" => "dashed",
        "dotted" => "dotted",
        "double" => "double",
        "none" | "nil" => "none",
        _ => "solid",
    }
}

/// The border value of `paraStyleXml` and `tableCellXml`: `0.5pt solid #RRGGBB`.
fn edge_value(e: &BorderEdge) -> String {
    format!(
        "{}pt {} #{}",
        fixed(eighth_pt_to_pt(e.size_eighth_pt.to_number()), 2),
        css_style(&e.style),
        e.color.as_deref().unwrap_or("000000")
    )
}

/// `paraStyleXml(name, p, parentStyleName)`.
fn para_style_xml(name: &str, p: &ParaProps, parent: Option<&str>) -> String {
    let mut para: Vec<String> = Vec::new();
    match p.alignment.as_deref() {
        Some("center") => para.push("fo:text-align=\"center\"".into()),
        Some("right") => para.push("fo:text-align=\"end\"".into()),
        Some("both") => para.push("fo:text-align=\"justify\"".into()),
        Some("left") => para.push("fo:text-align=\"start\"".into()),
        _ => {}
    }
    let tw = |n: Option<JsNumber>| twips_to_pt(n.map_or(0.0, JsNumber::to_number));
    if truthy_num(p.indent_left_twips) {
        para.push(format!("fo:margin-left=\"{}\"", tw(p.indent_left_twips)));
    }
    if truthy_num(p.indent_right_twips) {
        para.push(format!("fo:margin-right=\"{}\"", tw(p.indent_right_twips)));
    }
    if truthy_num(p.indent_first_line_twips) {
        para.push(format!("fo:text-indent=\"{}\"", tw(p.indent_first_line_twips)));
    }
    if truthy_num(p.space_before_twips) {
        para.push(format!("fo:margin-top=\"{}\"", tw(p.space_before_twips)));
    }
    if truthy_num(p.space_after_twips) {
        para.push(format!("fo:margin-bottom=\"{}\"", tw(p.space_after_twips)));
    }
    if let Some(h) = truthy_str(&p.shading_hex) {
        para.push(format!("fo:background-color=\"#{h}\""));
    }
    if p.page_break_before == Some(true) {
        para.push("fo:break-before=\"page\"".into());
    }
    if let Some(b) = &p.borders {
        if let Some(e) = &b.top {
            para.push(format!("fo:border-top=\"{}\"", edge_value(e)));
        }
        if let Some(e) = &b.bottom {
            para.push(format!("fo:border-bottom=\"{}\"", edge_value(e)));
        }
        if let Some(e) = &b.left {
            para.push(format!("fo:border-left=\"{}\"", edge_value(e)));
        }
        if let Some(e) = &b.right {
            para.push(format!("fo:border-right=\"{}\"", edge_value(e)));
        }
    }
    let attrs = if para.is_empty() { String::new() } else { format!(" {}", para.join(" ")) };
    let parent = match parent.filter(|p| !p.is_empty()) {
        Some(p) => format!(" style:parent-style-name=\"{}\"", xml_escape(p)),
        None => String::new(),
    };
    format!("<style:style style:name=\"{name}\" style:family=\"paragraph\"{parent}><style:paragraph-properties{attrs}/></style:style>")
}

// ───────────────────────── the final view of the blocks ─────────────────────────

/// The top-level table of the final view: the cell paragraphs go through
/// [`final_paragraphs`], the nested tables of a cell stay as they are (the TS replaces
/// `cell.paragraphs` and `cell.blocks` and writes `cell.paragraphs` and `cell.tables`).
fn final_table(t: &Table) -> Cow<'_, Table> {
    let mut replaced: Vec<(usize, usize, PVec<Paragraph>)> = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate() {
            let n = cell.content.paras.len();
            let fin = final_paragraphs(cell.content.paras.iter());
            if fin.paras.len() != n || fin.paras.iter().any(|p| matches!(p, Cow::Owned(_))) {
                replaced.push((ri, ci, fin.paras.into_iter().map(Cow::into_owned).collect()));
            }
        }
    }
    if replaced.is_empty() {
        return Cow::Borrowed(t);
    }
    let mut nt = t.clone();
    for (ri, ci, paras) in replaced {
        if let Some(cell) = nt.rows.get_mut(ri).and_then(|r| r.cells.get_mut(ci)) {
            cell.content.paras = paras;
        }
    }
    Cow::Owned(nt)
}

/// A block of the final view.
enum FinalBlock<'a> {
    P(Cow<'a, Paragraph>),
    T(Cow<'a, Table>),
}

/// `finalBlocks(doc.blocks)`: runs of consecutive paragraphs go through [`final_paragraphs`], the
/// tables through [`final_table`].
fn final_blocks(story: &Story) -> Vec<FinalBlock<'_>> {
    let mut out: Vec<FinalBlock<'_>> = Vec::new();
    let mut run: Vec<&Paragraph> = Vec::new();
    for b in story.blocks() {
        match b {
            crate::model::Block::Paragraph(p) => run.push(p),
            crate::model::Block::Table(t) => {
                if !run.is_empty() {
                    out.extend(final_paragraphs(run.drain(..)).paras.into_iter().map(FinalBlock::P));
                }
                out.push(FinalBlock::T(final_table(t)));
            }
        }
    }
    if !run.is_empty() {
        out.extend(final_paragraphs(run.drain(..)).paras.into_iter().map(FinalBlock::P));
    }
    out
}

/// One block to emit: a paragraph or a table.
#[derive(Clone, Copy)]
enum Item<'a> {
    P(&'a Paragraph),
    T(&'a Table),
}

// ───────────────────────── heading level ─────────────────────────

/// A pattern position of a `/.../i` regex: the characters any of which matches (after `Canonicalize`).
type Alts = &'static str;

fn ci_matches(u: u16, alts: Alts) -> bool {
    let c = uo_text::case::regexp_canonicalize(u);
    alts.encode_utf16().any(|a| uo_text::case::regexp_canonicalize(a) == c)
}

fn ci_at(units: &[u16], at: usize, pat: &[Alts]) -> bool {
    pat.iter().enumerate().all(|(k, alts)| units.get(at + k).is_some_and(|&u| ci_matches(u, alts)))
}

const HEADING: [Alts; 7] = ["h", "e", "a", "d", "i", "n", "g"];
const TITULO_ACC: [Alts; 6] = ["t", "\u{ed}i", "t", "u", "l", "o"];
const TITULO: [Alts; 6] = ["t", "i", "t", "u", "l", "o"];
const TITLE: [Alts; 5] = ["t", "i", "t", "l", "e"];
const SUBTITLE: [Alts; 8] = ["s", "u", "b", "t", "i", "t", "l", "e"];
const SUBTITULO: [Alts; 9] = ["s", "u", "b", "t", "\u{ed}i", "t", "u", "l", "o"];

fn ci_whole(units: &[u16], pat: &[Alts]) -> bool {
    units.len() == pat.len() && ci_at(units, 0, pat)
}

/// `headingLevelOf(p)`: the outline level of a paragraph by its style id (0: not a heading).
/// `Title`/`Subtitle` of the reference app carry no number: they map to 1 and 2.
fn heading_level_of(p: &Paragraph) -> u32 {
    let units: Vec<u16> = p.props.style_id.as_deref().unwrap_or("").encode_utf16().collect();
    if ci_whole(&units, &TITLE) || ci_whole(&units, &TITULO_ACC) {
        return 1;
    }
    if ci_whole(&units, &SUBTITLE) || ci_whole(&units, &SUBTITULO) {
        return 2;
    }
    // /(?:heading|t[íi]tulo|titulo)\s*([1-9])/i, anywhere in the id
    for start in 0..units.len() {
        for alt in [&HEADING[..], &TITULO_ACC[..], &TITULO[..]] {
            if !ci_at(&units, start, alt) {
                continue;
            }
            let mut j = start + alt.len();
            while units.get(j).is_some_and(|&u| is_js_space_unit(u)) {
                j += 1;
            }
            if let Some(&d) = units.get(j)
                && (u16::from(b'1')..=u16::from(b'9')).contains(&d)
            {
                return (u32::from(d) - u32::from(b'0')).min(6);
            }
        }
    }
    // /^h([1-6])$/i
    if let [h, d] = units[..]
        && ci_matches(h, "h")
        && (u16::from(b'1')..=u16::from(b'6')).contains(&d)
    {
        return u32::from(d) - u32::from(b'0');
    }
    0
}

// ───────────────────────── the writer ─────────────────────────

/// An image of `Pictures/`.
struct Picture<'a> {
    path: String,
    data: &'a [u8],
    ext: String,
}

/// `Ctx`.
struct Writer<'a> {
    /// canonical key → name (`P1`…)
    para_styles: HashMap<String, String>,
    /// canonical key → name (`R1`…)
    run_styles: HashMap<String, String>,
    /// the XML of each paragraph (and table) style, in order
    para_style_defs: Vec<String>,
    run_style_defs: Vec<String>,
    pictures: Vec<Picture<'a>>,
    omit_images: bool,
    comments: JsMap<NumKey, &'a Comment>,
    /// ids already emitted: the reader inlines the comment, it is emitted once
    emitted_comments: HashSet<NumKey>,
    /// text frames emitted (unique names)
    frames: u32,
    media: &'a MediaStore,
    depth: u32,
}

impl<'a> Writer<'a> {
    /// `getRunStyle(ctx, p)`.
    fn get_run_style(&mut self, p: &RunProps) -> String {
        let key = run_style_key(p);
        if let Some(name) = self.run_styles.get(&key) {
            return name.clone();
        }
        let name = format!("R{}", self.run_style_defs.len() + 1);
        self.run_styles.insert(key, name.clone());
        self.run_style_defs.push(run_style_xml(&name, p));
        name
    }

    /// `getParaStyle(ctx, p, parentStyleName)`.
    fn get_para_style(&mut self, p: &ParaProps, parent: Option<&str>) -> String {
        let key = format!("{}||{}", parent.unwrap_or(""), para_style_key(p));
        if let Some(name) = self.para_styles.get(&key) {
            return name.clone();
        }
        let name = format!("P{}", self.para_style_defs.len() + 1);
        self.para_styles.insert(key, name.clone());
        self.para_style_defs.push(para_style_xml(&name, p, parent));
        name
    }

    /// `annotationsForRun(run, ctx)`: the comments of the run not emitted yet, as `office:annotation`.
    fn annotations_for_run(&mut self, run: &Run, out: &mut Out) {
        let Some(ids) = &run.props.comment_ids else { return };
        for id in ids {
            let key = NumKey::new(id.to_number());
            if self.emitted_comments.contains(&key) {
                continue;
            }
            let Some(&c) = self.comments.get(&key) else { continue };
            self.emitted_comments.insert(key);
            put(out, "<office:annotation>");
            if let Some(a) = truthy_str(&c.author) {
                put(out, &format!("<dc:creator>{}</dc:creator>", xml_escape(a)));
            }
            put(out, "<text:p>");
            escape_text(c.text.units(), out);
            put(out, "</text:p></office:annotation>");
        }
    }

    /// `runXml(run, ctx)`.
    fn run_xml(&mut self, run: &Run, out: &mut Out) -> Result<(), DocxError> {
        if let Some(image) = run.ext().and_then(|e| e.image.as_deref()) {
            if self.omit_images {
                return Ok(());
            }
            self.annotations_for_run(run, out);
            self.image_xml(image, out);
            return Ok(());
        }
        if let Some(tb) = run.ext().and_then(|e| e.textbox.as_deref()) {
            self.annotations_for_run(run, out);
            return self.text_frame_xml(tb, out);
        }
        let style_name = if is_all_default(&run.props) { None } else { Some(self.get_run_style(&run.props)) };
        let mut text: Out = U16Buf::new();
        escape_text(run.text.units(), &mut text);
        if text.is_empty() {
            // still emits the comment of an empty run
            self.annotations_for_run(run, out);
            return Ok(());
        }
        self.annotations_for_run(run, out);
        if let Some(link) = truthy_str(&run.props.link) {
            let style_attr = style_name.as_deref().map(|n| format!(" text:style-name=\"{n}\"")).unwrap_or_default();
            put(out, &format!("<text:a xlink:type=\"simple\" xlink:href=\"{}\"><text:span{style_attr}>", xml_escape(link)));
            out.push_buf(&text);
            put(out, "</text:span></text:a>");
            return Ok(());
        }
        match style_name {
            None => out.push_buf(&text),
            Some(n) => {
                put(out, &format!("<text:span text:style-name=\"{n}\">"));
                out.push_buf(&text);
                put(out, "</text:span>");
            }
        }
        Ok(())
    }

    /// `imageXml(img, ctx)`.
    fn image_xml(&mut self, img: &InlineImage, out: &mut Out) {
        let idx = self.pictures.len() + 1;
        let lower = uo_text::case::to_lowercase_str(&img.ext);
        let ext = if lower == "jpg" { "jpeg".to_owned() } else { lower };
        let path = format!("Pictures/img{idx}.{ext}");
        let media: &'a MediaStore = self.media;
        let data = media.get(img.data.0).unwrap_or(&[]);
        self.pictures.push(Picture { path: path.clone(), data, ext });
        put(
            out,
            &format!(
                "<draw:frame draw:name=\"Image{idx}\" text:anchor-type=\"as-char\" svg:width=\"{}\" svg:height=\"{}\">\
<draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
                twips_to_cm(img.width_twips.to_number()),
                twips_to_cm(img.height_twips.to_number())
            ),
        );
    }

    /// `textFrameXml(tb, ctx)`: a text box as a text frame (`draw:frame` + `draw:text-box`) in its place.
    fn text_frame_xml(&mut self, tb: &DrawingTextbox, out: &mut Out) -> Result<(), DocxError> {
        if self.depth >= MAX_WRITE_DEPTH {
            return Err(DocxError::Write("text boxes nested too deep"));
        }
        self.depth += 1;
        let mut inner: Out = U16Buf::new();
        let r = {
            // `tb.blocks ?? tb.paras.map(…)`: the content in block order; a box read without blocks has its paragraphs
            let items: Vec<Item<'_>> = if tb.view == crate::model::run::TextboxView::ParasOnly {
                tb.content.paras.iter().map(Item::P).collect()
            } else {
                story_items(&tb.content)
            };
            self.emit_blocks(&items, &mut inner)
        };
        self.depth -= 1;
        r?;
        let anchor = if tb.floating.unwrap_or(false) { tb.anchor.as_deref() } else { None };
        let at = match anchor {
            None => "as-char",
            Some(a) if &*a.pos_v.relative_from == "page" => "page",
            Some(_) => "paragraph",
        };
        let pos = anchor
            .map(|a| {
                format!(
                    " svg:x=\"{}\" svg:y=\"{}\"",
                    twips_to_cm(a.pos_h.offset.map_or(0.0, JsNumber::to_number)),
                    twips_to_cm(a.pos_v.offset.map_or(0.0, JsNumber::to_number))
                )
            })
            .unwrap_or_default();
        self.frames = self.frames.saturating_add(1);
        put(
            out,
            &format!(
                "<draw:frame draw:name=\"Moldura{}\" text:anchor-type=\"{at}\"{pos} svg:width=\"{}\" svg:height=\"{}\"><draw:text-box>",
                self.frames,
                twips_to_cm(tb.width_twips.to_number()),
                twips_to_cm(tb.height_twips.to_number())
            ),
        );
        out.push_buf(&inner);
        put(out, "</draw:text-box></draw:frame>");
        Ok(())
    }

    /// `paragraphXml(para, ctx, opts)`.
    fn paragraph_xml(&mut self, para: &Paragraph, parent: &str, as_heading: u32, out: &mut Out) -> Result<(), DocxError> {
        let style_name = self.get_para_style(&para.props, Some(parent));
        if as_heading > 0 {
            put(out, &format!("<text:h text:style-name=\"{style_name}\" text:outline-level=\"{as_heading}\">"));
        } else {
            put(out, &format!("<text:p text:style-name=\"{style_name}\">"));
        }
        for r in &para.runs {
            self.run_xml(r, out)?;
        }
        put(out, if as_heading > 0 { "</text:h>" } else { "</text:p>" });
        Ok(())
    }

    /// `emitParagraph(para, out, ctx)`: a loose (non-list) paragraph.
    fn emit_paragraph(&mut self, para: &Paragraph, out: &mut Out) -> Result<(), DocxError> {
        let lvl = heading_level_of(para);
        if lvl > 0 {
            self.paragraph_xml(para, &format!("Heading_20_{lvl}"), lvl, out)
        } else {
            self.paragraph_xml(para, "Standard", 0, out)
        }
    }

    /// `emitList(items, out, ctx)`: contiguous numbered paragraphs as nested `text:list`s (built as a tree
    /// by `ilvl`, written with a loop where the TS recurses).
    fn emit_list(&mut self, items: &[&Paragraph], out: &mut Out) -> Result<(), DocxError> {
        if items.is_empty() {
            return Ok(());
        }
        struct Node<'p> {
            para: &'p Paragraph,
            ilvl: f64,
            bullet: bool,
            children: Vec<usize>,
        }
        let mut arena: Vec<Node<'_>> = Vec::with_capacity(items.len());
        let mut roots: Vec<usize> = Vec::new();
        // the open node of each level
        let mut stack: Vec<usize> = Vec::new();
        for &p in items {
            let Some(num) = p.props.numbering.as_ref() else { continue };
            let ilvl = num.ilvl.to_number();
            let id = arena.len();
            arena.push(Node { para: p, ilvl, bullet: &*num.kind == "bullet", children: Vec::new() });
            while let Some(&top) = stack.last()
                && arena.get(top).is_some_and(|n| n.ilvl >= ilvl)
            {
                stack.pop();
            }
            match stack.last() {
                None => roots.push(id),
                Some(&parent) => {
                    if let Some(n) = arena.get_mut(parent) {
                        n.children.push(id);
                    }
                }
            }
            stack.push(id);
        }
        let open = |arena: &[Node<'_>], group: &[usize], out: &mut Out| {
            let bullet = group.first().and_then(|&i| arena.get(i)).is_some_and(|n| n.bullet);
            put(out, &format!("<text:list text:style-name=\"{}\">", if bullet { "WNListBullet" } else { "WNListNumber" }));
        };
        struct Frame<'g> {
            group: &'g [usize],
            pos: usize,
            in_children: bool,
        }
        if roots.is_empty() {
            return Ok(());
        }
        open(&arena, &roots, out);
        let mut frames: Vec<Frame<'_>> = vec![Frame { group: &roots, pos: 0, in_children: false }];
        while let Some(f) = frames.last_mut() {
            let Some(&node_id) = f.group.get(f.pos) else {
                put(out, "</text:list>");
                frames.pop();
                continue;
            };
            let Some(node) = arena.get(node_id) else {
                f.pos += 1;
                continue;
            };
            if f.in_children {
                put(out, "</text:list-item>");
                f.in_children = false;
                f.pos += 1;
                continue;
            }
            put(out, "<text:list-item>");
            self.paragraph_xml(node.para, "Standard", 0, out)?;
            if node.children.is_empty() {
                put(out, "</text:list-item>");
                f.pos += 1;
            } else {
                f.in_children = true;
                open(&arena, &node.children, out);
                frames.push(Frame { group: &node.children, pos: 0, in_children: false });
            }
        }
        Ok(())
    }

    /// `tableXml(t, ctx)`.
    fn table_xml(&mut self, t: &Table, out: &mut Out) -> Result<(), DocxError> {
        if self.depth >= MAX_WRITE_DEPTH {
            return Err(DocxError::Write("tables nested too deep"));
        }
        self.depth += 1;
        let r = self.table_xml_inner(t, out);
        self.depth -= 1;
        r
    }

    fn table_xml_inner(&mut self, t: &Table, out: &mut Out) -> Result<(), DocxError> {
        // one style for the table plus the columns with their widths (style:column-width)
        let table_name = format!("T{}", self.para_style_defs.len() + self.run_style_defs.len() + 1);
        let mut total = 0.0;
        for w in &t.grid_cols_twips {
            total += w.to_number();
        }
        self.para_style_defs.push(format!(
            "<style:style style:name=\"{table_name}\" style:family=\"table\"><style:table-properties style:width=\"{}\" table:align=\"left\"/></style:style>",
            twips_to_pt(total)
        ));
        put(out, &format!("<table:table table:name=\"{table_name}\" table:style-name=\"{table_name}\">"));
        for (i, w) in t.grid_cols_twips.iter().enumerate() {
            let col_name = format!("{table_name}.C{}", i + 1);
            self.para_style_defs.push(format!(
                "<style:style style:name=\"{col_name}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{}\"/></style:style>",
                twips_to_pt(w.to_number())
            ));
            put(out, &format!("<table:table-column table:style-name=\"{col_name}\"/>"));
        }
        for row in &t.rows {
            put(out, "<table:table-row>");
            for cell in &row.cells {
                self.table_cell_xml(cell, &table_name, out)?;
                let span = cell.grid_span.to_number();
                if span > MAX_COLUMNS {
                    return Err(DocxError::Write("a table cell spans too many columns"));
                }
                let mut i = 1.0;
                while i < span {
                    put(out, "<table:covered-table-cell/>");
                    i += 1.0;
                }
            }
            put(out, "</table:table-row>");
        }
        put(out, "</table:table>");
        Ok(())
    }

    /// `tableCellXml(cell, ctx, tableName)`.
    fn table_cell_xml(&mut self, cell: &Cell, table_name: &str, out: &mut Out) -> Result<(), DocxError> {
        let cell_name = format!("{table_name}.A{}", self.para_style_defs.len());
        let mut attrs: Vec<String> = Vec::new();
        if let Some(h) = truthy_str(&cell.shading_hex) {
            attrs.push(format!("fo:background-color=\"#{h}\""));
        }
        if let Some(b) = &cell.borders {
            if let Some(e) = &b.top {
                attrs.push(format!("fo:border-top=\"{}\"", edge_value(e)));
            }
            if let Some(e) = &b.bottom {
                attrs.push(format!("fo:border-bottom=\"{}\"", edge_value(e)));
            }
            if let Some(e) = &b.left {
                attrs.push(format!("fo:border-left=\"{}\"", edge_value(e)));
            }
            if let Some(e) = &b.right {
                attrs.push(format!("fo:border-right=\"{}\"", edge_value(e)));
            }
        }
        attrs.push("fo:padding=\"0.05cm\"".into());
        self.para_style_defs.push(format!(
            "<style:style style:name=\"{cell_name}\" style:family=\"table-cell\"><style:table-cell-properties {}/></style:style>",
            attrs.join(" ")
        ));
        put(out, &format!("<table:table-cell table:style-name=\"{cell_name}\""));
        let span = cell.grid_span;
        if span.to_number() > 1.0 && !span.is_null() {
            put(out, &format!(" table:number-columns-spanned=\"{}\"", js_str(span)));
        }
        put(out, ">");
        // the paragraphs of the cell, then its tables (not interleaved)
        let mut items: Vec<Item<'_>> = cell.content.paras.iter().map(Item::P).collect();
        items.extend(cell.content.tables.iter().map(|a: &TableAnchor| Item::T(&a.table)));
        self.emit_blocks(&items, out)?;
        put(out, "</table:table-cell>");
        Ok(())
    }

    /// `emitBlocks(blocks, out, ctx)`: numbered paragraphs are grouped into lists.
    fn emit_blocks(&mut self, blocks: &[Item<'_>], out: &mut Out) -> Result<(), DocxError> {
        let mut list_buf: Vec<&Paragraph> = Vec::new();
        for b in blocks {
            if let Item::P(p) = b
                && p.props.numbering.is_some()
            {
                list_buf.push(p);
                continue;
            }
            if !list_buf.is_empty() {
                self.emit_list(&list_buf, out)?;
                list_buf.clear();
            }
            match b {
                Item::P(p) => self.emit_paragraph(p, out)?,
                Item::T(t) => self.table_xml(t, out)?,
            }
        }
        if !list_buf.is_empty() {
            self.emit_list(&list_buf, out)?;
        }
        Ok(())
    }
}

/// `isAllDefault(p)`.
fn is_all_default(p: &RunProps) -> bool {
    p.bold != Some(true)
        && p.italic != Some(true)
        && p.underline != Some(true)
        && p.strike != Some(true)
        && p.double_strike != Some(true)
        && truthy_str(&p.vert_align).is_none()
        && p.sz_half_pt.is_none()
        && truthy_str(&p.color).is_none()
        && truthy_str(&p.highlight).is_none()
        && truthy_str(&p.font_ascii).is_none()
        && p.all_caps != Some(true)
        && p.small_caps != Some(true)
        && !truthy_num(p.char_spacing_twips)
}

/// `escapeText(s)`: text with the ODF handling of spaces (`text:s` for runs of two or more) and the
/// characters XML 1.0 forbids dropped. A form feed (how the editor encodes a page break in
/// `run.text`) becomes `<text:soft-page-break/>`.
fn escape_text(s: &[u16], out: &mut Out) {
    let mut i = 0;
    while let Some(&ch) = s.get(i) {
        match ch {
            0x09 => put(out, "<text:tab/>"),
            0x0a => put(out, "<text:line-break/>"),
            0x20 => {
                let mut n = 1;
                while s.get(i + n) == Some(&0x20) {
                    n += 1;
                }
                if n == 1 {
                    out.push_unit(0x20);
                } else {
                    put(out, &format!(" <text:s text:c=\"{}\"/>", n - 1));
                    i += n - 1;
                }
            }
            0x26 => put(out, "&amp;"),
            0x3c => put(out, "&lt;"),
            0x3e => put(out, "&gt;"),
            0x0c => put(out, "<text:soft-page-break/>"),
            c if c < 0x20 && c != 0x0d => {}
            c => out.push_unit(c),
        }
        i += 1;
    }
}

/// The items of a story in TS block order.
fn story_items(story: &Story) -> Vec<Item<'_>> {
    story
        .blocks()
        .map(|b| match b {
            crate::model::Block::Paragraph(p) => Item::P(p),
            crate::model::Block::Table(t) => Item::T(t),
        })
        .collect()
}

// ───────────────────────── package files ─────────────────────────

/// `styleStandardXml()`: the `Standard` paragraph style (the parent of the cascade), the headings 1–6
/// and the two list styles (bullet and decimal).
fn style_standard_xml() -> String {
    let mut s = String::new();
    s.push_str("<style:style style:name=\"Standard\" style:family=\"paragraph\"><style:paragraph-properties/></style:style>");
    for (n, size) in [(1, "20pt"), (2, "16pt"), (3, "14pt"), (4, "13pt"), (5, "12pt"), (6, "11pt")] {
        s.push_str(&format!(
            "<style:style style:name=\"Heading_20_{n}\" style:display-name=\"Heading {n}\" style:family=\"paragraph\" style:parent-style-name=\"Standard\"><style:text-properties fo:font-size=\"{size}\" fo:font-weight=\"bold\" fo:color=\"#0F4761\"/></style:style>"
        ));
    }
    let margins = ["0.5", "1", "1.5", "2", "2.5"];
    s.push_str("<text:list-style style:name=\"WNListBullet\">");
    for (lvl, m) in margins.iter().enumerate() {
        s.push_str(&format!(
            "<text:list-level-style-bullet text:level=\"{}\" text:bullet-char=\"\u{2022}\"><style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:margin-left=\"{m}cm\" fo:text-indent=\"-0.5cm\"/></style:list-level-properties></text:list-level-style-bullet>",
            lvl + 1
        ));
    }
    s.push_str("</text:list-style><text:list-style style:name=\"WNListNumber\">");
    for (lvl, m) in margins.iter().enumerate() {
        s.push_str(&format!(
            "<text:list-level-style-number text:level=\"{}\" style:num-format=\"1\" style:num-suffix=\".\"><style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:margin-left=\"{m}cm\" fo:text-indent=\"-0.5cm\"/></style:list-level-properties></text:list-level-style-number>",
            lvl + 1
        ));
    }
    s.push_str("</text:list-style>");
    s
}

/// `pageLayoutXml(doc)`: the geometry of section 0 (A4 with 2.5 cm margins without one).
fn page_layout_xml(doc: &Document) -> String {
    let default = SectPr {
        page_width_twips: JsNumber::Num(11906.0),
        page_height_twips: JsNumber::Num(16838.0),
        margin_left_twips: JsNumber::Num(1417.0),
        margin_right_twips: JsNumber::Num(1417.0),
        margin_top_twips: JsNumber::Num(1417.0),
        margin_bottom_twips: JsNumber::Num(1417.0),
        ..SectPr::default()
    };
    let sp: &SectPr = doc.sections.first().map_or(&default, |s| &s.sect_pr);
    format!(
        "<style:page-layout style:name=\"WNPage\"><style:page-layout-properties fo:page-width=\"{}\" fo:page-height=\"{}\" fo:margin-left=\"{}\" fo:margin-right=\"{}\" fo:margin-top=\"{}\" fo:margin-bottom=\"{}\"/></style:page-layout>",
        twips_to_cm(sp.page_width_twips.to_number()),
        twips_to_cm(sp.page_height_twips.to_number()),
        twips_to_cm(sp.margin_left_twips.to_number()),
        twips_to_cm(sp.margin_right_twips.to_number()),
        twips_to_cm(sp.margin_top_twips.to_number()),
        twips_to_cm(sp.margin_bottom_twips.to_number())
    )
}

const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"";

/// `stylesXml(doc)`.
fn styles_xml(doc: &Document) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles>{}</office:styles><office:automatic-styles>{}</office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"WNPage\"/></office:master-styles></office:document-styles>",
        style_standard_xml(),
        page_layout_xml(doc)
    )
}

/// `manifestXml(pictures)`.
fn manifest_xml(pictures: &[Picture<'_>]) -> String {
    let mut entries = String::new();
    for p in pictures {
        let mime = match p.ext.as_str() {
            "jpeg" | "jpg" => "image/jpeg".to_owned(),
            "png" => "image/png".to_owned(),
            "gif" => "image/gif".to_owned(),
            other => format!("image/{other}"),
        };
        entries.push_str(&format!("<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{mime}\"/>", p.path));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{ODT_MIMETYPE}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>{entries}</manifest:manifest>"
    )
}

// ───────────────────────── entry point ─────────────────────────

/// `writeOdt(doc, options)`: the bytes of an `.odt` for the document, in the final view of the
/// revisions (no deleted text).
///
/// # Errors
/// The zip writer's (a name or a size past the format), or a model nested past the writer's depth
/// (256) or a table cell spanning over 65,536 columns (the TS has no limit and runs out of memory).
pub fn write_odt(doc: &Document, options: &OdtOptions) -> Result<Vec<u8>, DocxError> {
    // revisions: the .odt comes out in the final view (without the deleted text)
    let blocks = final_blocks(&doc.body);
    let mut comments: JsMap<NumKey, &Comment> = JsMap::new();
    for c in doc.comments.iter() {
        comments.insert(NumKey::new(c.id.to_number()), c);
    }
    let mut w = Writer {
        para_styles: HashMap::new(),
        run_styles: HashMap::new(),
        para_style_defs: Vec::new(),
        run_style_defs: Vec::new(),
        pictures: Vec::new(),
        omit_images: options.images == OdtImages::Omit,
        comments,
        emitted_comments: HashSet::new(),
        frames: 0,
        media: &doc.media,
        depth: 0,
    };
    // content.xml DEPENDS on the style lists (filled while the body is emitted)
    let items: Vec<Item<'_>> = blocks
        .iter()
        .map(|b| match b {
            FinalBlock::P(p) => Item::P(p),
            FinalBlock::T(t) => Item::T(t),
        })
        .collect();
    let mut body: Out = U16Buf::new();
    w.emit_blocks(&items, &mut body)?;
    let mut content: Out = U16Buf::new();
    put(&mut content, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    put(&mut content, &format!("<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>"));
    for d in w.para_style_defs.iter().chain(&w.run_style_defs) {
        put(&mut content, d);
    }
    put(&mut content, "</office:automatic-styles><office:body><office:text>");
    content.push_buf(&body);
    put(&mut content, "</office:text></office:body></office:document-content>");
    drop(body);
    // TextEncoder: a lone surrogate is U+FFFD
    let content_xml = content.into_string_lossy();
    let styles = styles_xml(doc);
    let manifest = manifest_xml(&w.pictures);

    // 'mimetype' FIRST and stored
    let mut files: Vec<(&str, &[u8])> = vec![
        ("mimetype", ODT_MIMETYPE.as_bytes()),
        ("content.xml", content_xml.as_bytes()),
        ("styles.xml", styles.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ];
    for p in &w.pictures {
        files.push((&p.path, p.data));
    }
    // `parts[pic.path] = …` of a JS object: array-index names first (none here: they start with `Pictures/`)
    let opts = WriteOptions { stored: vec!["mimetype".to_owned()], ..WriteOptions::default() };
    Ok(zip::write(files, opts)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn escape_text_follows_the_ts() {
        let mut out = U16Buf::new();
        escape_text(&units("a  b\t&<>\n\u{c}\u{1}   c"), &mut out);
        assert_eq!(
            out.into_string_lossy(),
            "a <text:s text:c=\"1\"/>b<text:tab/>&amp;&lt;&gt;<text:line-break/><text:soft-page-break/> <text:s text:c=\"2\"/>c"
        );
        // dropping a control character joins a split pair
        let mut out = U16Buf::new();
        escape_text(&[0xd83d, 0x01, 0xde00], &mut out);
        assert_eq!(out.into_string_lossy(), "\u{1f600}");
    }

    #[test]
    fn heading_levels() {
        let p = |id: &str| Paragraph {
            props: Arc::new(ParaProps { style_id: Some(id.into()), ..ParaProps::default() }),
            ..Paragraph::default()
        };
        assert_eq!(heading_level_of(&p("Heading1")), 1);
        assert_eq!(heading_level_of(&p("heading 3")), 3);
        assert_eq!(heading_level_of(&p("HEADING9")), 6);
        assert_eq!(heading_level_of(&p("Título2")), 2);
        assert_eq!(heading_level_of(&p("TÍTULO 4")), 4);
        assert_eq!(heading_level_of(&p("Title")), 1);
        assert_eq!(heading_level_of(&p("Subtítulo")), 2);
        assert_eq!(heading_level_of(&p("MyHeading5x")), 5);
        assert_eq!(heading_level_of(&p("h3")), 3);
        assert_eq!(heading_level_of(&p("H7")), 0);
        assert_eq!(heading_level_of(&p("Normal")), 0);
        assert_eq!(heading_level_of(&p("")), 0);
    }
}
