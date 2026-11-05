//! C# `DocxPictures` / `DocxPageArea`: insert inline pictures (DrawingML) and shrink the ones
//! that do not fit the page. Integer arithmetic is the C# `int`/`long` (unchecked: wraps);
//! `Math.Round` is half to even; `Math.Abs(int.MinValue)` throws (`OverflowException`).

use super::document::{DocxDocument, relative_target};
use super::dom::{Dom, NodeId, n};
use super::error::{ApiError, ErrorKind, Result};
use super::image_info::ImageInfo;
use super::net;
use super::ooxml::rel_types;
use super::{numbering, sections, styles};

pub const EMU_PER_TWIP: i64 = 635;
pub const EMU_PER_POINT: i64 = 12_700;

/// The page area of a section (twips).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageArea {
    pub page_width: i32,
    pub page_height: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub gutter: i32,
}

fn abs_checked(v: i32) -> Result<i32> {
    v.checked_abs().ok_or_else(|| ApiError::new(ErrorKind::Overflow, "Negating the minimum value of a twos complement number is invalid."))
}

impl PageArea {
    pub fn client_width(&self) -> i32 {
        0.max(self.page_width.wrapping_sub(self.margin_left).wrapping_sub(self.margin_right).wrapping_sub(self.gutter))
    }

    pub fn client_height(&self) -> Result<i32> {
        Ok(0.max(self.page_height.wrapping_sub(abs_checked(self.margin_top)?).wrapping_sub(abs_checked(self.margin_bottom)?)))
    }

    /// The area of the section (Word's default, Letter with 1" margins, where it does not say).
    pub fn from(dom: &Dom, sect: Option<NodeId>) -> PageArea {
        let size = sect.and_then(|s| dom.element(s, n::pgSz));
        let mar = sect.and_then(|s| dom.element(s, n::pgMar));
        let tw = |el: Option<NodeId>, attr, fallback: i32| el.and_then(|e| dom.attr(e, attr)).and_then(net::parse_int).unwrap_or(fallback);
        PageArea {
            page_width: tw(size, n::w, 12240),
            page_height: tw(size, n::h, 15840),
            margin_left: tw(mar, n::left, 1440),
            margin_right: tw(mar, n::right, 1440),
            margin_top: tw(mar, n::top, 1440),
            margin_bottom: tw(mar, n::bottom, 1440),
            gutter: tw(mar, n::gutter, 0),
        }
    }
}

/// Stores the image in `word/media` related to the part; returns the relationship id.
pub(crate) fn add_image(doc: &mut DocxDocument, part: &str, image: &[u8]) -> Result<(String, ImageInfo)> {
    let info = ImageInfo::try_read(image)?.ok_or_else(|| ApiError::argument("image in an unsupported format (PNG, JPEG, GIF or BMP)"))?;
    let media = doc.unique_part_path(&format!("word/media/image1.{}", info.extension));
    doc.set_binary_part(&media, image.to_vec(), Some(info.content_type))?;
    let rel = doc.add_relationship(part, rel_types::IMAGE, &relative_target(part, &media), false)?;
    Ok((rel, info))
}

/// An inline `w:drawing` (`wp:inline` + `pic:pic`) for an image already related.
pub(crate) fn inline_drawing(dom: &mut Dom, rel_id: &str, cx: i64, cy: i64, doc_pr_id: i32, name: &str) -> NodeId {
    let a_uri = dom.ns_uri(super::dom::NS_A).to_owned();
    let pic_uri = dom.ns_uri(super::dom::NS_PIC).to_owned();
    let (cx, cy) = (cx.to_string(), cy.to_string());
    let id = doc_pr_id.to_string();
    let el = |dom: &mut Dom, name, attrs: &[(super::dom::Name, &str)], kids: &[NodeId]| {
        let e = dom.new_element_with(name, attrs);
        dom.append_all(e, kids);
        e
    };
    let extent = el(dom, n::wp_extent, &[(n::a_cx, &cx), (n::a_cy, &cy)], &[]);
    let effect = el(dom, n::wp_effectExtent, &[(n::a_l, "0"), (n::a_t, "0"), (n::a_r, "0"), (n::a_b, "0")], &[]);
    let doc_pr = el(dom, n::wp_docPr, &[(n::a_id, &id), (n::a_name, &format!("{name} {id}"))], &[]);
    let locks = el(dom, n::a_graphicFrameLocks, &[(n::xmlns_a, &a_uri), (n::noChangeAspect, "1")], &[]);
    let frame = el(dom, n::wp_cNvGraphicFramePr, &[], &[locks]);
    let c_nv_pr = el(dom, n::pic_cNvPr, &[(n::a_id, "0"), (n::a_name, name)], &[]);
    let c_nv_pic_pr = el(dom, n::pic_cNvPicPr, &[], &[]);
    let nv = el(dom, n::pic_nvPicPr, &[], &[c_nv_pr, c_nv_pic_pr]);
    let blip = el(dom, n::a_blip, &[(n::r_embed, rel_id)], &[]);
    let fill_rect = el(dom, n::a_fillRect, &[], &[]);
    let stretch = el(dom, n::a_stretch, &[], &[fill_rect]);
    let blip_fill = el(dom, n::pic_blipFill, &[], &[blip, stretch]);
    let off = el(dom, n::a_off, &[(n::a_x, "0"), (n::a_y, "0")], &[]);
    let ext = el(dom, n::a_ext, &[(n::a_cx, &cx), (n::a_cy, &cy)], &[]);
    let xfrm = el(dom, n::a_xfrm, &[], &[off, ext]);
    let av = el(dom, n::a_avLst, &[], &[]);
    let geom = el(dom, n::a_prstGeom, &[(n::a_prst, "rect")], &[av]);
    let sp_pr = el(dom, n::pic_spPr, &[], &[xfrm, geom]);
    let pic = el(dom, n::pic_pic, &[(n::xmlns_pic, &pic_uri)], &[nv, blip_fill, sp_pr]);
    let data = el(dom, n::a_graphicData, &[(n::a_uri, &pic_uri)], &[pic]);
    let graphic = el(dom, n::a_graphic, &[(n::xmlns_a, &a_uri)], &[data]);
    let inline = el(
        dom,
        n::wp_inline,
        &[(n::distT, "0"), (n::distB, "0"), (n::distL, "0"), (n::distR, "0")],
        &[extent, effect, doc_pr, frame, graphic],
    );
    el(dom, n::drawing, &[], &[inline])
}

/// Shrinks (never enlarges) keeping the aspect ratio.
pub fn fit(cx: i64, cy: i64, max_cx: Option<i64>, max_cy: Option<i64>) -> (i64, i64) {
    let mut scale = 1.0f64;
    #[allow(clippy::cast_precision_loss)]
    let f = |v: i64| v as f64;
    if let Some(m) = max_cx
        && m > 0
        && cx > m
    {
        scale = uo_jsnum::math::min(scale, f(m) / f(cx));
    }
    if let Some(m) = max_cy
        && m > 0
        && cy > m
    {
        scale = uo_jsnum::math::min(scale, f(m) / f(cy));
    }
    if scale < 1.0 {
        (net::to_i64_saturating(net::round_half_even(f(cx) * scale)), net::to_i64_saturating(net::round_half_even(f(cy) * scale)))
    } else {
        (cx, cy)
    }
}

/// Shrinks every body picture (DrawingML inline/anchor with `pic:pic`, VML with `v:imagedata`)
/// larger than the area of its section; returns how many.
pub fn fit_to_page(doc: &mut DocxDocument) -> Result<usize> {
    let body = doc.body()?;
    fit_within(doc, &[body])
}

/// As [`fit_to_page`], only inside the given elements.
pub(crate) fn fit_within(doc: &mut DocxDocument, containers: &[NodeId]) -> Result<usize> {
    let mut count = 0;
    let drawings: Vec<NodeId> =
        containers.iter().flat_map(|&r| doc.dom().descendants_and_self(r).into_iter().filter(|&e| doc.dom().is(e, n::drawing))).collect();
    for drawing in drawings {
        let Some(paragraph) = doc.dom().ancestor(drawing, n::p) else { continue };
        let (width, height) = available_area(doc, paragraph)?;
        let max_cx = i64::from(width) * EMU_PER_TWIP;
        let max_cy = i64::from(height) * EMU_PER_TWIP;
        let dom = doc.dom_mut();
        let shape = dom.elements(drawing).find(|&e| dom.is_any(e, &[n::wp_inline, n::wp_anchor]));
        let Some(shape) = shape else { continue };
        let Some(extent) = dom.element(shape, n::wp_extent) else { continue };
        let pic = dom.element(shape, n::a_graphic).and_then(|g| dom.element(g, n::a_graphicData)).and_then(|g| dom.element(g, n::pic_pic));
        let Some(pic) = pic else { continue };
        let (Some(cx), Some(cy)) =
            (dom.attr(extent, n::a_cx).and_then(net::parse_long), dom.attr(extent, n::a_cy).and_then(net::parse_long))
        else {
            continue;
        };
        let (nx, ny) = fit(cx, cy, Some(max_cx), Some(max_cy));
        if nx == cx && ny == cy {
            continue;
        }
        dom.set_attr(extent, n::a_cx, Some(&nx.to_string()));
        dom.set_attr(extent, n::a_cy, Some(&ny.to_string()));
        if let Some(ext) = dom.element(pic, n::pic_spPr).and_then(|s| dom.element(s, n::a_xfrm)).and_then(|x| dom.element(x, n::a_ext)) {
            dom.set_attr(ext, n::a_cx, Some(&nx.to_string()));
            dom.set_attr(ext, n::a_cy, Some(&ny.to_string()));
        }
        count += 1;
    }
    let shapes: Vec<NodeId> = containers
        .iter()
        .flat_map(|&r| doc.dom().descendants_named(r, n::v_shape))
        .filter(|&s| doc.dom().element(s, n::v_imagedata).is_some() && doc.dom().ancestor(s, n::pict).is_some())
        .collect();
    for shape in shapes {
        let Some(paragraph) = doc.dom().ancestor(shape, n::p) else { continue };
        let (width, height) = available_area(doc, paragraph)?;
        if fit_vml(doc.dom_mut(), shape, f64::from(width) / 20.0, f64::from(height) / 20.0)? {
            count += 1;
        }
    }
    Ok(count)
}

/// Width and height (twips) available for a picture in the paragraph.
fn available_area(doc: &mut DocxDocument, paragraph: NodeId) -> Result<(i32, i32)> {
    let section = sections::section_of(doc, paragraph)?;
    let area = PageArea::from(doc.dom(), section);
    let col = sections::column_width(doc.dom(), section, area.client_width());
    let width = col.wrapping_sub(0.max(left_indent(doc, paragraph)?));
    Ok((width, area.client_height()?))
}

/// The effective left indent (twips): direct → list level → style.
pub(crate) fn left_indent(doc: &mut DocxDocument, paragraph: NodeId) -> Result<i32> {
    let dom = doc.dom();
    let direct = dom.element(paragraph, n::pPr).and_then(|p| dom.element(p, n::ind));
    if let Some(d) = indent_value(dom, direct) {
        return Ok(d);
    }
    let num_pr = match dom.element(paragraph, n::pPr).and_then(|p| dom.element(p, n::numPr)) {
        Some(x) => Some(x),
        None => styles::resolve_ppr_child(doc, paragraph, n::numPr)?,
    };
    if let Some(np) = num_pr {
        let dom = doc.dom();
        let num_id = dom.element(np, n::numId).and_then(|x| dom.attr(x, n::val)).map(str::to_owned);
        let ilvl = dom.element(np, n::ilvl).and_then(|x| dom.attr(x, n::val)).and_then(net::parse_int).unwrap_or(0);
        if let Some(id) = num_id
            && let Some(v) = numbering::level_indent(doc, &id, ilvl)?
        {
            return Ok(v);
        }
    }
    let ind = styles::resolve_ppr_child(doc, paragraph, n::ind)?;
    Ok(indent_value(doc.dom(), ind).unwrap_or(0))
}

fn indent_value(dom: &Dom, ind: Option<NodeId>) -> Option<i32> {
    let i = ind?;
    dom.attr(i, n::left).or_else(|| dom.attr(i, n::start)).and_then(net::parse_int)
}

/// One match of `(?<prop>width|height)\s*:\s*(?<num>[0-9.]+)\s*(?<unit>pt|in|cm|mm|px)?`
/// (IgnoreCase, CultureInvariant), as UTF-16 positions in the style.
struct VmlMatch {
    start: usize,
    end: usize,
    prop: Vec<u16>,
    num: Vec<u16>,
    unit: Vec<u16>,
}

fn lower_ascii(c: u16) -> u16 {
    if (0x41..=0x5A).contains(&c) { c + 32 } else { c }
}

fn match_word(s: &[u16], at: usize, word: &str) -> bool {
    let w: Vec<u16> = word.encode_utf16().collect();
    s.get(at..at + w.len()).is_some_and(|x| x.iter().zip(&w).all(|(&a, &b)| lower_ascii(a) == b))
}

fn vml_matches(s: &[u16]) -> Vec<VmlMatch> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let prop_len = if match_word(s, i, "width") {
            5
        } else if match_word(s, i, "height") {
            6
        } else {
            i += 1;
            continue;
        };
        let mut j = i + prop_len;
        while s.get(j).is_some_and(|&c| net::is_white_space(c)) {
            j += 1;
        }
        if s.get(j) != Some(&u16::from(b':')) {
            i += 1;
            continue;
        }
        j += 1;
        while s.get(j).is_some_and(|&c| net::is_white_space(c)) {
            j += 1;
        }
        let ns = j;
        while s.get(j).is_some_and(|&c| (0x30..=0x39).contains(&c) || c == u16::from(b'.')) {
            j += 1;
        }
        if j == ns {
            i += 1;
            continue;
        }
        let ne = j;
        while s.get(j).is_some_and(|&c| net::is_white_space(c)) {
            j += 1;
        }
        let mut unit = Vec::new();
        for u in ["pt", "in", "cm", "mm", "px"] {
            if match_word(s, j, u) {
                unit = s.get(j..j + 2).unwrap_or(&[]).to_vec();
                j += 2;
                break;
            }
        }
        out.push(VmlMatch {
            start: i,
            end: j,
            prop: s.get(i..i + prop_len).unwrap_or(&[]).to_vec(),
            num: s.get(ns..ne).unwrap_or(&[]).to_vec(),
            unit,
        });
        i = j.max(i + 1);
    }
    out
}

/// `double.Parse(num, InvariantCulture)` of `[0-9.]+`: more than one point (or none and no
/// digit) is a `FormatException`.
fn parse_vml_number(num: &[u16]) -> Result<f64> {
    let s = net::from_units(num);
    let points = s.matches('.').count();
    if points > 1 || !s.bytes().any(|b| b.is_ascii_digit()) {
        return Err(ApiError::new(ErrorKind::Format, format!("The input string '{s}' was not in a correct format.")));
    }
    Ok(uo_jsnum::parse::to_number(&s))
}

fn to_points(value: f64, unit: &[u16]) -> f64 {
    let u: String = net::from_units(unit).to_ascii_lowercase();
    match u.as_str() {
        "in" => value * 72.0,
        "cm" => value * 72.0 / 2.54,
        "mm" => value * 72.0 / 25.4,
        "px" => value * 0.75,
        _ => value,
    }
}

pub fn fit_vml(dom: &mut Dom, shape: NodeId, max_w: f64, max_h: f64) -> Result<bool> {
    let style = dom.attr(shape, n::a_style).unwrap_or("").to_owned();
    let su = net::units(&style);
    let matches = vml_matches(&su);
    let (mut width, mut height): (Option<f64>, Option<f64>) = (None, None);
    for m in &matches {
        let pt = to_points(parse_vml_number(&m.num)?, &m.unit);
        if net::eq_ignore_case(&net::from_units(&m.prop), "width") {
            width = width.or(Some(pt));
        } else {
            height = height.or(Some(pt));
        }
    }
    let (Some(w), Some(h)) = (width, height) else { return Ok(false) };
    if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
        return Ok(false);
    }
    let scale = uo_jsnum::math::min(1.0, uo_jsnum::math::min(max_w / w, max_h / h));
    if scale >= 1.0 {
        return Ok(false);
    }
    let mut out: Vec<u16> = Vec::with_capacity(su.len());
    let mut last = 0;
    for m in &matches {
        out.extend_from_slice(su.get(last..m.start).unwrap_or(&[]));
        let is_width = net::eq_ignore_case(&net::from_units(&m.prop), "width");
        let v = (if is_width { w } else { h }) * scale;
        let repl = format!("{}:{}pt", net::from_units(&m.prop), net::format_0_00(v));
        out.extend(repl.encode_utf16());
        last = m.end;
    }
    out.extend_from_slice(su.get(last..).unwrap_or(&[]));
    dom.set_attr(shape, n::a_style, Some(&net::from_units(&out)));
    Ok(true)
}

/// The next free `wp:docPr` id (unique in the whole document, as Word requires).
pub(crate) fn next_doc_pr_id(doc: &mut DocxDocument) -> Result<i32> {
    let mut max = 0i32;
    for (_, root) in doc.stories(true, true)? {
        let dom = doc.dom();
        for d in dom.descendants_named(root, n::wp_docPr) {
            if let Some(v) = dom.attr(d, n::a_id).and_then(net::parse_int) {
                max = max.max(v);
            }
        }
    }
    Ok(max.wrapping_add(1))
}
