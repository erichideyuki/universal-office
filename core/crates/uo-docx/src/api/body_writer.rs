//! C# `DocxBodyWriter` with `DocxRunFormat`, `DocxParagraphFormat` and `DocxParagraphProperties`:
//! writes formatted content at the END of the body (before the final `w:sectPr`), line by line,
//! closed into paragraphs with the given formatting. A view over the document: every call
//! takes the document it was created for (ids of another document are refused).

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::{ApiError, Result};
use super::fields::{self, DEFAULT_ALLOWED_SCHEMES};
use super::image_info::ImageInfo;
use super::net;
use super::ooxml::{self, order, rel_types};
use super::paragraph_text as pt;
use super::shapes::normalize_hex;
use super::{pictures, styles, template};

/// How the writer writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyWriterOptions {
    /// schemes accepted in links (others become text; `www.…` gets `http://`)
    pub allowed_link_schemes: Vec<String>,
    /// colour of links (`RRGGBB` or CSS), no underline unless the text is underlined; `None` =
    /// the "Hyperlink" character style (created if missing)
    pub link_color: Option<String>,
    pub reuse_empty_last_paragraph: bool,
    pub trim_trailing_empty_paragraph: bool,
    pub fit_pictures: bool,
}

impl Default for BodyWriterOptions {
    fn default() -> Self {
        BodyWriterOptions {
            allowed_link_schemes: DEFAULT_ALLOWED_SCHEMES.iter().map(|s| (*s).to_owned()).collect(),
            link_color: None,
            reuse_empty_last_paragraph: true,
            trim_trailing_empty_paragraph: true,
            fit_pictures: true,
        }
    }
}

/// Vertical position of the text (`w:vertAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Superscript,
    Subscript,
}

/// Character formatting of the written text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunFormat {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<String>,
    pub background: Option<String>,
    pub vertical_align: Option<VerticalAlign>,
    pub link: Option<String>,
}

/// An opaque copy of a paragraph's properties (`w:pPr` without `w:sectPr`/`w:pPrChange`), to
/// reuse a template paragraph's formatting; owns its own small arena so it can be applied to
/// any document.
#[derive(Debug, Clone)]
pub struct ParagraphProperties {
    frag: Dom,
    ppr: NodeId,
}

impl ParagraphProperties {
    pub(crate) fn from_copy(src: &Dom, ppr: NodeId) -> Option<ParagraphProperties> {
        let mut frag = Dom::new();
        let p = frag.import(src, ppr)?;
        Some(ParagraphProperties { frag, ppr: p })
    }

    /// The paragraph style (`w:pStyle`).
    pub fn style_id(&self) -> Option<String> {
        self.frag.element(self.ppr, n::pStyle).and_then(|e| self.frag.attr(e, n::val)).map(str::to_owned)
    }

    /// The list instance (`w:numPr/w:numId`).
    pub fn num_id(&self) -> Option<String> {
        self.frag
            .element(self.ppr, n::numPr)
            .and_then(|x| self.frag.element(x, n::numId))
            .and_then(|e| self.frag.attr(e, n::val))
            .map(str::to_owned)
    }

    fn copy_into(&self, dom: &mut Dom) -> Option<NodeId> {
        dom.import(&self.frag, self.ppr)
    }
}

/// Paragraph formatting of the written content.
#[derive(Debug, Clone, Default)]
pub struct ParagraphFormat {
    pub base: Option<ParagraphProperties>,
    pub style_id: Option<String>,
    pub list_num_id: Option<String>,
    pub list_level: i32,
    pub left_indent_twips: i32,
    pub alignment: Option<String>,
    pub keep_with_next: bool,
}

const ALIGNMENTS: &[&str] = &[
    "start",
    "end",
    "left",
    "right",
    "center",
    "both",
    "mediumKashida",
    "distribute",
    "numTab",
    "highKashida",
    "lowKashida",
    "thaiDistribute",
];
const MAX_LIST_LEVEL: i32 = 8;

/// The writer state (C# `DocxBodyWriter`).
#[derive(Debug, Clone)]
pub struct BodyWriter {
    options: BodyWriterOptions,
    sect_pr: Option<NodeId>,
    line: Vec<NodeId>,
    written: Vec<NodeId>,
    reuse: Option<NodeId>,
    next_doc_pr_id: i32,
}

impl BodyWriter {
    pub fn new(doc: &mut DocxDocument, options: BodyWriterOptions) -> Result<BodyWriter> {
        let sect_pr = doc.last_section_properties()?;
        let mut reuse = None;
        if options.reuse_empty_last_paragraph
            && let Some(&last) = template::body_blocks(doc)?.last()
            && template::is_empty_paragraph(doc.dom(), last)
        {
            reuse = Some(last);
        }
        let next_doc_pr_id = pictures::next_doc_pr_id(doc)?;
        Ok(BodyWriter { options, sect_pr, line: Vec::new(), written: Vec::new(), reuse, next_doc_pr_id })
    }

    fn check(&self, doc: &DocxDocument) -> Result<()> {
        for id in self.sect_pr.iter().chain(self.reuse.iter()).chain(self.line.iter()).chain(self.written.iter()) {
            doc.dom().check(*id)?;
        }
        Ok(())
    }

    /// The current line (the paragraph not closed yet) is empty.
    pub fn line_is_empty(&self) -> bool {
        self.line.is_empty()
    }

    /// Text on the current line (`\t` → `w:tab`, line breaks → `w:br`); empty text does nothing.
    pub fn add_text(&mut self, doc: &mut DocxDocument, text: &str, format: &RunFormat) -> Result<()> {
        self.check(doc)?;
        if text.is_empty() {
            return Ok(());
        }
        let run = doc.dom_mut().new_element(n::r);
        let rpr = self.run_properties(doc, format)?;
        let dom = doc.dom_mut();
        if dom.elements(rpr).next().is_some() {
            dom.append(run, rpr);
        }
        let content = ooxml::run_content(dom, text);
        dom.append_all(run, &content);
        let wrapped = self.wrap_link(doc, run, format.link.as_deref())?;
        self.line.push(wrapped);
        Ok(())
    }

    /// A line break (`w:br`) in the current paragraph.
    pub fn add_break(&mut self, doc: &mut DocxDocument) -> Result<()> {
        self.check(doc)?;
        let dom = doc.dom_mut();
        let br = dom.new_element(n::br);
        let r = dom.new_element(n::r);
        dom.append(r, br);
        self.line.push(r);
        Ok(())
    }

    /// An inline picture of its physical size (PNG, JPEG, GIF, BMP), optionally linked; false
    /// (nothing added) if the image is not recognized.
    pub fn add_image(&mut self, doc: &mut DocxDocument, image: &[u8], link: Option<&str>) -> Result<bool> {
        self.check(doc)?;
        if ImageInfo::try_read(image)?.is_none() {
            return Ok(false);
        }
        let main = doc.main_part_path().to_owned();
        let (rel, info) = pictures::add_image(doc, &main, image)?;
        let id = self.next_doc_pr_id;
        self.next_doc_pr_id = id.wrapping_add(1);
        let dom = doc.dom_mut();
        let drawing = pictures::inline_drawing(dom, &rel, info.width_emu(), info.height_emu(), id, "Picture");
        let run = dom.new_element(n::r);
        dom.append(run, drawing);
        let wrapped = self.wrap_link(doc, run, link)?;
        self.line.push(wrapped);
        Ok(true)
    }

    /// Closes the current line into a paragraph with the formatting (an empty line is an empty
    /// paragraph); the first one reuses the body's last empty paragraph when asked.
    pub fn end_paragraph(&mut self, doc: &mut DocxDocument, format: &ParagraphFormat) -> Result<()> {
        self.check(doc)?;
        let ppr = self.paragraph_properties(doc, format)?;
        let dom = doc.dom_mut();
        let paragraph = match self.reuse {
            Some(p) => p,
            None => dom.new_element(n::p),
        };
        let keep = dom.element(paragraph, n::pPr).and_then(|p| dom.element(p, n::sectPr));
        for e in dom.elements_named(paragraph, n::pPr) {
            dom.remove_if_attached(e);
        }
        if let Some(k) = keep {
            ooxml::set(dom, ppr, k, order::PPR);
        }
        if dom.elements(ppr).next().is_some() {
            dom.prepend(paragraph, &[ppr]);
        }
        let line = std::mem::take(&mut self.line);
        dom.append_all(paragraph, &line);
        if self.reuse.is_none() {
            match self.sect_pr {
                Some(s) => {
                    dom.add_before(s, &[paragraph])?;
                }
                None => {
                    let body = doc.body()?;
                    doc.dom_mut().append(body, paragraph);
                }
            }
        }
        self.reuse = None;
        self.written.push(paragraph);
        Ok(())
    }

    /// Closes the pending line, trims the last empty paragraph and fits the pictures (per the options).
    pub fn finish(&mut self, doc: &mut DocxDocument) -> Result<()> {
        self.check(doc)?;
        if !self.line.is_empty() {
            self.end_paragraph(doc, &ParagraphFormat::default())?;
        }
        if self.options.trim_trailing_empty_paragraph
            && let Some(&last) = self.written.last()
        {
            let dom = doc.dom();
            let blank =
                net::is_white_space_only(&pt::plain_text(dom, last)) && dom.first_descendant(last, |d, e| d.is(e, n::drawing)).is_none();
            if blank && template::body_blocks(doc)?.len() > 1 {
                doc.dom_mut().remove(last)?;
                self.written.pop();
            }
        }
        if self.options.fit_pictures {
            let written = self.written.clone();
            pictures::fit_within(doc, &written)?;
        }
        doc.mark_changed();
        Ok(())
    }

    fn paragraph_properties(&self, doc: &mut DocxDocument, f: &ParagraphFormat) -> Result<NodeId> {
        let sroot = styles::root(doc, false)?;
        let default_id = styles::default_paragraph_style_id(doc.dom(), sroot);
        let dom = doc.dom_mut();
        let ppr = match f.base.as_ref().and_then(|b| b.copy_into(dom)) {
            Some(p) => p,
            None => dom.new_element(n::pPr),
        };
        if let Some(sid) = &f.style_id
            && Some(sid) != default_id.as_ref()
        {
            let e = dom.new_element_with(n::pStyle, &[(n::val, sid)]);
            ooxml::set(dom, ppr, e, order::PPR);
        }
        if f.keep_with_next {
            let e = dom.new_element(n::keepNext);
            ooxml::set(dom, ppr, e, order::PPR);
        }
        if let Some(num_id) = &f.list_num_id {
            let level = f.list_level.clamp(0, MAX_LIST_LEVEL);
            let num_pr = ooxml::ensure(dom, ppr, n::numPr, order::PPR);
            let il = dom.new_element_with(n::ilvl, &[(n::val, &level.to_string())]);
            ooxml::set(dom, num_pr, il, order::NUMPR);
            let ni = dom.new_element_with(n::numId, &[(n::val, num_id)]);
            ooxml::set(dom, num_pr, ni, order::NUMPR);
        }
        if f.left_indent_twips > 0 {
            let ind = ooxml::ensure(dom, ppr, n::ind, order::PPR);
            dom.set_attr(ind, n::left, Some(&f.left_indent_twips.to_string()));
        }
        if let Some(a) = &f.alignment {
            if !ALIGNMENTS.contains(&a.as_str()) {
                return Err(ApiError::argument(format!(
                    "invalid paragraph alignment: \"{a}\" (ST_Jc values, e.g. left, center, right, both). (Parameter 'format')"
                )));
            }
            let jc = dom.new_element_with(n::jc, &[(n::val, a)]);
            ooxml::set(dom, ppr, jc, order::PPR);
        }
        Ok(ppr)
    }

    fn run_properties(&self, doc: &mut DocxDocument, f: &RunFormat) -> Result<NodeId> {
        let rpr = doc.dom_mut().new_element(n::rPr);
        let linked = f.link.as_deref().is_some_and(|l| self.link_target(l).is_some());
        let color = if linked && self.options.link_color.is_some() {
            normalize_hex(self.options.link_color.as_deref())?
        } else {
            normalize_hex(f.color.as_deref())?
        };
        if linked && self.options.link_color.is_none() {
            let sid = styles::ensure_hyperlink_character_style(doc)?;
            let dom = doc.dom_mut();
            let e = dom.new_element_with(n::rStyle, &[(n::val, &sid)]);
            ooxml::set(dom, rpr, e, order::RPR);
        }
        let dom = doc.dom_mut();
        for (on, name) in [(f.bold, n::b), (f.italic, n::i), (f.strike, n::strike)] {
            if on {
                let e = dom.new_element(name);
                ooxml::set(dom, rpr, e, order::RPR);
            }
        }
        if let Some(c) = &color {
            let e = dom.new_element_with(n::color, &[(n::val, c)]);
            ooxml::set(dom, rpr, e, order::RPR);
        }
        if f.underline {
            let e = dom.new_element_with(n::u, &[(n::val, "single")]);
            ooxml::set(dom, rpr, e, order::RPR);
        } else if linked && self.options.link_color.is_some() {
            let e = dom.new_element_with(n::u, &[(n::val, "none")]);
            ooxml::set(dom, rpr, e, order::RPR);
        }
        if let Some(bg) = normalize_hex(f.background.as_deref())? {
            let e = dom.new_element_with(n::shd, &[(n::val, "clear"), (n::color, "auto"), (n::w_fill, &bg)]);
            ooxml::set(dom, rpr, e, order::RPR);
        }
        if let Some(v) = f.vertical_align {
            let e =
                dom.new_element_with(n::vertAlign, &[(n::val, if v == VerticalAlign::Subscript { "subscript" } else { "superscript" })]);
            ooxml::set(dom, rpr, e, order::RPR);
        }
        Ok(rpr)
    }

    fn wrap_link(&self, doc: &mut DocxDocument, run: NodeId, link: Option<&str>) -> Result<NodeId> {
        let Some(target) = link.and_then(|l| self.link_target(l)) else { return Ok(run) };
        let main = doc.main_part_path().to_owned();
        let id = doc.add_relationship(&main, rel_types::HYPERLINK, &target, true)?;
        let dom = doc.dom_mut();
        let h = dom.new_element_with(n::hyperlink, &[(n::r_id, &id), (n::history, "1")]);
        dom.append(h, run);
        Ok(h)
    }

    /// An allowed absolute URL; `www.…` gets `http://`; anything else is not a link.
    fn link_target(&self, link: &str) -> Option<String> {
        let schemes: Vec<&str> = self.options.allowed_link_schemes.iter().map(String::as_str).collect();
        let trimmed = net::trim(link);
        if fields::is_allowed_url(trimmed, &schemes) {
            return Some(trimmed.to_owned());
        }
        let with = format!("http://{trimmed}");
        if net::starts_with_ignore_case(trimmed, "www.") && fields::is_allowed_url(&with, &schemes) {
            return Some(with);
        }
        None
    }
}
