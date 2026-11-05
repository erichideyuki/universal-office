//! C# `DocxShapes` / `DocxShape` (shapes, text boxes, headers and footers of a template) and
//! `Colors` (CSS colours → OOXML `RRGGBB`).

use std::collections::HashMap;

use super::document::DocxDocument;
use super::dom::{Dom, NS_A, NS_V, NodeId, n};
use super::error::{ApiError, ErrorKind, Result};
use super::net;
use super::ooxml::{self, order};
use super::paragraph_text::{self as pt, ParagraphText};
use super::pictures::{self, EMU_PER_POINT};
use super::text::{self, TextScope};

const NAMED: &[(&str, &str)] = &[
    ("black", "000000"),
    ("white", "FFFFFF"),
    ("red", "FF0000"),
    ("green", "008000"),
    ("blue", "0000FF"),
    ("yellow", "FFFF00"),
    ("orange", "FFA500"),
    ("purple", "800080"),
    ("gray", "808080"),
    ("grey", "808080"),
    ("silver", "C0C0C0"),
    ("maroon", "800000"),
    ("olive", "808000"),
    ("lime", "00FF00"),
    ("aqua", "00FFFF"),
    ("cyan", "00FFFF"),
    ("teal", "008080"),
    ("navy", "000080"),
    ("fuchsia", "FF00FF"),
    ("magenta", "FF00FF"),
];

/// `^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*[0-9.]+\s*)?\)$` (IgnoreCase;
/// `\d` and `\s` are Unicode in .NET): the three digit groups.
fn match_rgb(v: &[u16]) -> Option<[Vec<u16>; 3]> {
    let mut i = 0;
    let lower = |c: u16| if (0x41..=0x5A).contains(&c) { c + 32 } else { c };
    let lit = |i: &mut usize, s: &str| -> bool {
        for b in s.bytes() {
            if v.get(*i).map(|&c| lower(c)) != Some(u16::from(b)) {
                return false;
            }
            *i += 1;
        }
        true
    };
    let ws = |i: &mut usize| {
        while v.get(*i).is_some_and(|&c| net::is_white_space(c)) {
            *i += 1;
        }
    };
    let digits = |i: &mut usize| -> Option<Vec<u16>> {
        let s = *i;
        while *i - s < 3 && v.get(*i).is_some_and(|&c| net::is_decimal_digit_unit(c)) {
            *i += 1;
        }
        (*i > s).then(|| v.get(s..*i).unwrap_or(&[]).to_vec())
    };
    if !lit(&mut i, "rgb") {
        return None;
    }
    if v.get(i).map(|&c| lower(c)) == Some(u16::from(b'a')) {
        i += 1;
    }
    if !lit(&mut i, "(") {
        return None;
    }
    let mut groups: [Vec<u16>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (k, g) in groups.iter_mut().enumerate() {
        ws(&mut i);
        *g = digits(&mut i)?;
        ws(&mut i);
        if k < 2 && !lit(&mut i, ",") {
            return None;
        }
    }
    // optional alpha
    let save = i;
    if lit(&mut i, ",") {
        ws(&mut i);
        let s = i;
        while v.get(i).is_some_and(|&c| (0x30..=0x39).contains(&c) || c == u16::from(b'.')) {
            i += 1;
        }
        if i == s {
            i = save;
        } else {
            ws(&mut i);
        }
    }
    if !lit(&mut i, ")") {
        return None;
    }
    // `$`: the end, or before a final line feed
    let at_end = i == v.len() || (i + 1 == v.len() && v.get(i) == Some(&0x0A));
    at_end.then_some(groups)
}

/// C# `Colors.NormalizeHex`: upper-case `RRGGBB`, or `None` if it is not a recognized colour
/// (`FormatException` for a non-ASCII digit inside `rgb(…)`, as `int.Parse` does).
pub fn normalize_hex(css: Option<&str>) -> Result<Option<String>> {
    let Some(css) = css else { return Ok(None) };
    if net::is_white_space_only(css) {
        return Ok(None);
    }
    let v = net::trim(css);
    if let Some((_, hex)) = NAMED.iter().find(|(k, _)| net::eq_ignore_case(k, v)) {
        return Ok(Some((*hex).to_owned()));
    }
    let u = net::units(v);
    if let Some(groups) = match_rgb(&u) {
        let mut out = String::new();
        for g in groups {
            let s = net::from_units(&g);
            let value = net::parse_int(&s)
                .ok_or_else(|| ApiError::new(ErrorKind::Format, format!("The input string '{s}' was not in a correct format.")))?;
            out.push_str(&format!("{:02X}", value.clamp(0, 255)));
        }
        return Ok(Some(out));
    }
    let v = v.strip_prefix('#').unwrap_or(v);
    let n = net::units(v).len();
    if n == 3 && v.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Some(v.chars().flat_map(|c| [c.to_ascii_uppercase(); 2]).collect()));
    }
    if n == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Some(v.to_ascii_uppercase()));
    }
    Ok(None)
}

/// A shape or text box (DrawingML `wps:wsp` and/or the VML fallback `v:shape` of the same
/// `mc:AlternateContent`; the operations apply to both). A view: the run holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub run: NodeId,
}

fn is_own(dom: &Dom, run: NodeId, e: NodeId) -> bool {
    dom.ancestor(e, n::r) == Some(run)
}

fn is_vml_shape(dom: &Dom, e: NodeId) -> bool {
    dom.ns_of(e) == NS_V && matches!(dom.local_of(e), "shape" | "rect" | "roundrect")
}

pub(crate) fn has_own_shape(dom: &Dom, run: NodeId) -> bool {
    dom.descendants(run).into_iter().any(|e| (dom.is(e, n::wps_wsp) || is_vml_shape(dom, e)) && dom.ancestor(e, n::r) == Some(run))
}

const FILL_NAMES: &[&str] = &["noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill"];

impl Shape {
    fn drawing_shapes(&self, dom: &Dom) -> Vec<NodeId> {
        dom.descendants_named(self.run, n::wps_wsp).into_iter().filter(|&e| is_own(dom, self.run, e)).collect()
    }
    fn vml_shapes(&self, dom: &Dom) -> Vec<NodeId> {
        dom.descendants(self.run).into_iter().filter(|&e| is_vml_shape(dom, e) && is_own(dom, self.run, e)).collect()
    }
    fn text_boxes(&self, dom: &Dom) -> Vec<NodeId> {
        dom.descendants_named(self.run, n::txbxContent).into_iter().filter(|&e| is_own(dom, self.run, e)).collect()
    }

    /// The box's text (first representation), paragraphs joined by CRLF.
    pub fn text(&self, doc: &DocxDocument) -> String {
        let dom = doc.dom();
        match self.text_boxes(dom).first() {
            Some(&b) => dom.elements_named(b, n::p).into_iter().map(|p| pt::plain_text(dom, p)).collect::<Vec<_>>().join("\r\n"),
            None => String::new(),
        }
    }

    /// Empties the box's text (one empty paragraph left).
    pub fn clear_text(&self, doc: &mut DocxDocument) -> Result<()> {
        doc.dom().check(self.run)?;
        let dom = doc.dom_mut();
        for b in self.text_boxes(dom) {
            dom.remove_nodes(b);
            let p = dom.new_element(n::p);
            dom.append(b, p);
        }
        Ok(())
    }

    /// Solid fill with the RGB colour (`RRGGBB` or `#RRGGBB`, or a CSS colour).
    pub fn set_fill_color(&self, doc: &mut DocxDocument, rgb: &str) -> Result<()> {
        doc.dom().check(self.run)?;
        let hex = normalize_hex(Some(rgb))?.ok_or_else(|| ApiError::argument("invalid RGB colour (Parameter 'rgb')"))?;
        let dom = doc.dom_mut();
        for wsp in self.drawing_shapes(dom) {
            let clr = dom.new_element_with(n::a_srgbClr, &[(n::a_val, &hex)]);
            let fill = dom.new_element(n::a_solidFill);
            dom.append(fill, clr);
            set_drawing_fill(dom, wsp, fill)?;
        }
        for shape in self.vml_shapes(dom) {
            dom.set_attr(shape, n::fillcolor, Some(&format!("#{hex}")));
            dom.set_attr(shape, n::filled, None);
            for f in dom.elements_named(shape, n::v_fill) {
                dom.remove_if_attached(f);
            }
        }
        Ok(())
    }

    /// No fill (DocIO's "100% transparency").
    pub fn set_fill_transparent(&self, doc: &mut DocxDocument) -> Result<()> {
        doc.dom().check(self.run)?;
        let dom = doc.dom_mut();
        for wsp in self.drawing_shapes(dom) {
            let fill = dom.new_element(n::a_noFill);
            set_drawing_fill(dom, wsp, fill)?;
        }
        for shape in self.vml_shapes(dom) {
            dom.set_attr(shape, n::filled, Some("f"));
            for f in dom.elements_named(shape, n::v_fill) {
                dom.remove_if_attached(f);
            }
        }
        Ok(())
    }

    /// The text colour of the box's runs (of the last paragraph only, the default, or all).
    pub fn set_text_color(&self, doc: &mut DocxDocument, rgb: &str, last_paragraph_only: bool) -> Result<()> {
        doc.dom().check(self.run)?;
        let hex = normalize_hex(Some(rgb))?.ok_or_else(|| ApiError::argument("invalid RGB colour (Parameter 'rgb')"))?;
        let dom = doc.dom_mut();
        for b in self.text_boxes(dom) {
            let ps = dom.elements_named(b, n::p);
            let Some(&last) = ps.last() else { continue };
            let chosen = if last_paragraph_only { vec![last] } else { ps };
            for p in chosen {
                let runs: Vec<NodeId> =
                    dom.descendants_named(p, n::r).into_iter().filter(|&r| dom.ancestor(r, n::txbxContent) == Some(b)).collect();
                for r in runs {
                    let rpr = ooxml::ensure_rpr(dom, r);
                    let c = dom.new_element_with(n::color, &[(n::val, &hex)]);
                    ooxml::set(dom, rpr, c, order::RPR);
                }
            }
        }
        Ok(())
    }

    /// Removes the shape (its run) from the document.
    pub fn remove(&self, doc: &mut DocxDocument) -> Result<()> {
        doc.dom().check(self.run)?;
        doc.dom_mut().remove(self.run)
    }
}

fn set_drawing_fill(dom: &mut Dom, wsp: NodeId, fill: NodeId) -> Result<()> {
    let sp_pr = match dom.element(wsp, n::wps_spPr) {
        Some(s) => s,
        None => {
            let s = dom.new_element(n::wps_spPr);
            match dom.element(wsp, n::wps_cNvSpPr).or_else(|| dom.element(wsp, n::wps_cNvCnPr)) {
                Some(after) => {
                    dom.add_after(after, &[s])?;
                }
                None => {
                    dom.prepend(wsp, &[s]);
                }
            }
            s
        }
    };
    let existing = dom.elements(sp_pr).find(|&e| dom.ns_of(e) == NS_A && FILL_NAMES.contains(&dom.local_of(e)));
    if let Some(existing) = existing {
        dom.replace_with(existing, &[fill])?;
        return Ok(());
    }
    let geometry = dom.elements(sp_pr).rfind(|&e| dom.is_any(e, &[n::a_xfrm, n::a_custGeom, n::a_prstGeom]));
    match geometry {
        Some(g) => {
            dom.add_after(g, &[fill])?;
        }
        None => {
            dom.prepend(sp_pr, &[fill]);
        }
    }
    Ok(())
}

/// Shapes (text boxes) whose text contains `text` (case-sensitive, whole word), in document
/// order — body, headers and footers.
pub fn find_containing(doc: &mut DocxDocument, text: &str) -> Result<Vec<Shape>> {
    let mut runs: Vec<NodeId> = Vec::new();
    for p in text::find_paragraphs(doc, text, true, true, TextScope::Document)? {
        let dom = doc.dom();
        let Some(b) = dom.last_ancestor(p, n::txbxContent) else { continue };
        if let Some(run) = dom.ancestor(b, n::r)
            && !runs.contains(&run)
        {
            runs.push(run);
        }
    }
    Ok(runs.into_iter().map(|run| Shape { run }).collect())
}

/// Removes the shapes whose text contains `text`; returns how many.
pub fn remove_containing(doc: &mut DocxDocument, text: &str) -> Result<usize> {
    let shapes = find_containing(doc, text)?;
    for s in &shapes {
        s.remove(doc)?;
    }
    if !shapes.is_empty() {
        doc.mark_changed();
    }
    Ok(shapes.len())
}

/// Replaces each occurrence of the text (case-insensitive, whole word) by an inline picture of
/// its physical size, shrunk to fit `max_width_pt` × `max_height_pt` when given (C# `float`
/// arithmetic). The image is stored once per part; returns how many.
pub fn replace_text_with_image(
    doc: &mut DocxDocument,
    text: &str,
    image: &[u8],
    max_width_pt: Option<f32>,
    max_height_pt: Option<f32>,
) -> Result<usize> {
    let mut count = 0;
    let mut rel_by_part: HashMap<String, (String, super::image_info::ImageInfo)> = HashMap::new();
    #[allow(clippy::cast_precision_loss)]
    let emu = EMU_PER_POINT as f32;
    for (part, root) in doc.stories(true, true)? {
        for p in pt::paragraphs(doc.dom(), root) {
            let matches = ParagraphText::build(doc.dom(), p).find(text, false, true);
            for &(s, l) in matches.iter().rev() {
                let key = net::fold_key(&part);
                if !rel_by_part.contains_key(&key) {
                    let added = pictures::add_image(doc, &part, image)?;
                    rel_by_part.insert(key.clone(), added);
                }
                let Some((rel, info)) = rel_by_part.get(&key).cloned() else { continue };
                let (cx, cy) = pictures::fit(
                    info.width_emu(),
                    info.height_emu(),
                    max_width_pt.map(|w| net::f32_to_i64_saturating(w * emu)),
                    max_height_pt.map(|h| net::f32_to_i64_saturating(h * emu)),
                );
                let doc_pr = pictures::next_doc_pr_id(doc)?;
                let dom = doc.dom_mut();
                ParagraphText::build(dom, p).replace_with_element(dom, s, l, |d, rpr| {
                    let run = d.new_element(n::r);
                    if let Some(r) = rpr {
                        d.append(run, r);
                    }
                    let drawing = pictures::inline_drawing(d, &rel, cx, cy, doc_pr, "Picture");
                    d.append(run, drawing);
                    run
                })?;
                count += 1;
            }
        }
    }
    if count > 0 {
        doc.mark_changed();
    }
    Ok(count)
}

/// The header or footer type of a section (`w:type` of the reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeaderFooterType {
    #[default]
    Default,
    First,
    Even,
}

/// The first shape (DrawingML or VML) of the header or footer of the LAST section.
pub fn first_shape_in(doc: &mut DocxDocument, footer: bool, ty: HeaderFooterType) -> Result<Option<Shape>> {
    let Some(root) = header_footer_root(doc, footer, ty)? else { return Ok(None) };
    let dom = doc.dom();
    Ok(dom.descendants_named(root, n::r).into_iter().find(|&r| has_own_shape(dom, r)).map(|run| Shape { run }))
}

/// Empties the header or footer of the last section (one empty paragraph with the properties
/// of the first). False if the section has no such header/footer.
pub fn clear_header_footer(doc: &mut DocxDocument, footer: bool, ty: HeaderFooterType) -> Result<bool> {
    let Some(root) = header_footer_root(doc, footer, ty)? else { return Ok(false) };
    let dom = doc.dom_mut();
    let ppr = dom.elements_named(root, n::p).first().and_then(|&p| dom.element(p, n::pPr));
    dom.remove_nodes(root);
    let p = dom.new_element(n::p);
    if let Some(x) = ppr {
        let c = dom.deep_clone(x);
        dom.append(p, c);
    }
    dom.append(root, p);
    doc.mark_changed();
    Ok(true)
}

/// The root (`w:hdr`/`w:ftr`) of the last section's header/footer (inherited from earlier
/// sections, as in Word).
pub(crate) fn header_footer_root(doc: &mut DocxDocument, footer: bool, ty: HeaderFooterType) -> Result<Option<NodeId>> {
    let ref_name = if footer { n::footerReference } else { n::headerReference };
    let type_name = match ty {
        HeaderFooterType::First => "first",
        HeaderFooterType::Even => "even",
        HeaderFooterType::Default => "default",
    };
    let body = doc.body()?;
    let sections = doc.dom().descendants_named(body, n::sectPr);
    let main = doc.main_part_path().to_owned();
    for &s in sections.iter().rev() {
        let dom = doc.dom();
        let reference = dom.elements_named(s, ref_name).into_iter().find(|&r| dom.attr(r, n::type_).unwrap_or("default") == type_name);
        let Some(id) = reference.and_then(|r| dom.attr(r, n::r_id)).map(str::to_owned) else { continue };
        let rel = doc.get_relationship(&main, &id)?;
        let part = rel.and_then(|r| doc.resolve_target(&main, &r));
        return match part {
            None => Ok(None),
            Some(p) => doc.root_of(&p),
        };
    }
    Ok(None)
}
