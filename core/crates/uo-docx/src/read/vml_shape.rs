//! Frontier R3 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `vml-shape.ts`,
//! the `w:drawing` (wps) equivalent of a floating VML text box (`vmlTextboxToDrawing`) and the
//! test for "a `w:pict` that holds one text box" (`vmlSingleTextbox`).
//!
//! `vmlTextboxToDrawing` builds XML text with two functions of the writer side of docx-core,
//! `newTextboxDrawingXml` (`shape-xml.ts`) and `patchShape` → `patchPicture` (`picture-xml.ts`,
//! `XmlPatch`), which belong to the writer (D1b) and the drawing operations (D2b). Only what this
//! call reaches is here, as private helpers, with the same output text: the generator of the new
//! box (a fill, a line, the geometry, the body properties) and the two edits `patchShape` makes
//! for the fields this caller sets (`setAttrs` on the `wp:docPr` and on the `a:xfrm`, by
//! the positions of a real parse of the generated text, like the TS: a malformed text, which a
//! VML attribute copied raw into it can make, is the same error as the TS exception). The
//! declarations of the nine kinds are in `core/docs/modules/uo-docx-d1a/R3.md`.

use std::cmp::Ordering;

use uo_jsnum::conv::truthy;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::{is_js_space, trim};
use uo_opc::xml::{Element, parse};

use super::DOCX_POS;
use super::drawing::{VmlStyle, ends_with_ascii_lower, parse_vml_style, vml_color, vml_len_twips};
use crate::error::DocxError;

/// The language of the default box name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VmlLang {
    #[default]
    Pt,
    En,
    Es,
}

const EMU_PER_TWIP: f64 = 635.0;

/// `emu`: a VML length in EMU (`Math.round(twips * 635)`).
fn emu(v: Option<&str>) -> Option<f64> {
    vml_len_twips(v).map(|t| math::round(t * EMU_PER_TWIP))
}

/// `frac`: a VML fraction (`0.75`, `49152f`) in 0..1; `None` when absent or not a finite number.
fn frac(v: Option<&str>) -> Option<f64> {
    let v = v.filter(|s| !s.is_empty())?;
    let x = if v.ends_with('f') { parse_int(v, 10) / 65536.0 } else { parse_float(v) };
    x.is_finite().then_some(x)
}

fn js(x: f64) -> String {
    to_js_string(x)
}

/// `escAttr` of `picture-xml.ts`.
fn esc_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `String(Object.prototype[key])` for the names a plain object inherits: a lookup such as
/// `REL_H[rel]` with such a document value finds a function (or the prototype) and the TS
/// template literal prints it.
fn proto_string(key: &str) -> Option<&'static str> {
    Some(match key {
        "constructor" => "function Object() { [native code] }",
        "__defineGetter__" => "function __defineGetter__() { [native code] }",
        "__defineSetter__" => "function __defineSetter__() { [native code] }",
        "hasOwnProperty" => "function hasOwnProperty() { [native code] }",
        "__lookupGetter__" => "function __lookupGetter__() { [native code] }",
        "__lookupSetter__" => "function __lookupSetter__() { [native code] }",
        "isPrototypeOf" => "function isPrototypeOf() { [native code] }",
        "propertyIsEnumerable" => "function propertyIsEnumerable() { [native code] }",
        "toString" => "function toString() { [native code] }",
        "valueOf" => "function valueOf() { [native code] }",
        "__proto__" => "[object Object]",
        "toLocaleString" => "function toLocaleString() { [native code] }",
        _ => return None,
    })
}

/// `REL_H`.
fn rel_h(key: &str) -> Option<&'static str> {
    Some(match key {
        "text" => "column",
        "page" => "page",
        "margin" => "margin",
        "char" => "character",
        "left-margin-area" => "leftMargin",
        "right-margin-area" => "rightMargin",
        "inner-margin-area" => "insideMargin",
        "outer-margin-area" => "outsideMargin",
        _ => return None,
    })
}

/// `REL_V`.
fn rel_v(key: &str) -> Option<&'static str> {
    Some(match key {
        "text" => "paragraph",
        "page" => "page",
        "margin" => "margin",
        "line" => "line",
        "top-margin-area" => "topMargin",
        "bottom-margin-area" => "bottomMargin",
        "inner-margin-area" => "insideMargin",
        "outer-margin-area" => "outsideMargin",
        _ => return None,
    })
}

/// `FLOW[key]` (keys contain `|`: no inherited name matches).
fn flow(key: &str) -> Option<&'static str> {
    Some(match key {
        "vertical|bottom-to-top" => "vert270",
        "vertical|top-to-bottom" => "mongolianVert",
        "vertical|" => "vert",
        "vertical-ideographic|" => "eaVert",
        _ => return None,
    })
}

/// A colour of the new box: `{ kind: 'srgbClr', val, mods? }` with the one `alpha` modifier.
struct Rgb {
    val: String,
    alpha: Option<f64>,
}

/// `rgb(hex, alpha)`.
fn rgb(hex: &str, alpha: Option<f64>) -> Rgb {
    let alpha = alpha.filter(|a| *a < 0.999).map(|a| math::round(a * 100_000.0));
    Rgb { val: hex.to_string(), alpha }
}

/// `dmlColorXml` of an [`Rgb`].
fn color_xml(c: &Rgb) -> String {
    let attrs = format!(" val=\"{}\"", esc_attr(&c.val));
    match c.alpha {
        Some(a) => format!("<a:srgbClr{attrs}><a:alpha val=\"{}\"/></a:srgbClr>", js(a)),
        None => format!("<a:srgbClr{attrs}/>"),
    }
}

/// The fill of the new box (`ShapeFill`).
enum Fill {
    None,
    Solid(Rgb),
    Gradient { first: Rgb, second: Rgb, ang: f64 },
}

/// `shapeFillXml`.
fn fill_xml(f: &Fill) -> String {
    match f {
        Fill::None => "<a:noFill/>".to_string(),
        Fill::Solid(c) => format!("<a:solidFill>{}</a:solidFill>", color_xml(c)),
        Fill::Gradient { first, second, ang } => {
            let gs = format!("<a:gs pos=\"0\">{}</a:gs><a:gs pos=\"100000\">{}</a:gs>", color_xml(first), color_xml(second));
            let shade = format!("<a:lin ang=\"{}\" scaled=\"1\"/>", js(math::round(*ang)));
            format!("<a:gradFill rotWithShape=\"1\"><a:gsLst>{gs}</a:gsLst>{shade}</a:gradFill>")
        }
    }
}

/// The outline of the new box (`PicLine`): width in EMU, solid colour, dash and compound.
struct Line {
    w: f64,
    fill: Rgb,
    dash: Option<String>,
    cmpd: Option<String>,
}

/// `picLineXmlLocal`.
fn line_xml(l: &Line) -> String {
    let mut attrs = format!(" w=\"{}\"", js(math::round(l.w)));
    if let Some(c) = l.cmpd.as_deref().filter(|c| !c.is_empty()) {
        attrs.push_str(&format!(" cmpd=\"{c}\""));
    }
    let mut inner = format!("<a:solidFill>{}</a:solidFill>", color_xml(&l.fill));
    if let Some(d) = l.dash.as_deref().filter(|d| !d.is_empty()) {
        inner.push_str(&format!("<a:prstDash val=\"{d}\"/>"));
    }
    format!("<a:ln{attrs}>{inner}</a:ln>")
}

/// One axis of the box position.
struct Pos {
    relative_from: String,
    offset: Option<f64>,
    align: Option<String>,
}

/// `posXml` of `shape-xml.ts` (no percentage position here).
fn pos_xml(tag: &str, p: &Pos) -> String {
    let inner = match &p.align {
        Some(a) => format!("<wp:align>{a}</wp:align>"),
        None => format!("<wp:posOffset>{}</wp:posOffset>", js(math::round(p.offset.unwrap_or(0.0)))),
    };
    format!("<{tag} relativeFrom=\"{}\">{inner}</{tag}>", p.relative_from)
}

/// The body properties the caller sets (`TextboxBody` over `DEFAULT_TEXTBOX_BODY`).
struct Body {
    l_ins: f64,
    t_ins: f64,
    r_ins: f64,
    b_ins: f64,
    anchor: &'static str,
    vert: &'static str,
    wrap_none: bool,
    autofit_shape: bool,
}

/// `bodyPrXml`.
fn body_xml(b: &Body) -> String {
    let wrap = if b.wrap_none { "none" } else { "square" };
    let fit = if b.autofit_shape { "<a:spAutoFit/>" } else { "<a:noAutofit/>" };
    format!(
        "<wps:bodyPr rot=\"0\" spcFirstLastPara=\"0\" vertOverflow=\"overflow\" horzOverflow=\"overflow\" vert=\"{}\" wrap=\"{wrap}\" lIns=\"{}\" tIns=\"{}\" rIns=\"{}\" bIns=\"{}\" numCol=\"1\" spcCol=\"0\" rtlCol=\"0\" fromWordArt=\"0\" anchor=\"{}\" anchorCtr=\"0\" forceAA=\"0\" compatLnSpc=\"1\"><a:prstTxWarp prst=\"textNoShape\"><a:avLst/></a:prstTxWarp>{fit}</wps:bodyPr>",
        b.vert,
        js(math::round(b.l_ins)),
        js(math::round(b.t_ins)),
        js(math::round(b.r_ins)),
        js(math::round(b.b_ins)),
        b.anchor,
    )
}

/// The options of `newTextboxDrawingXml` this caller passes.
struct NewTextbox {
    id: f64,
    name: String,
    cx: f64,
    cy: f64,
    pos_h: Pos,
    pos_v: Pos,
    relative_height: f64,
    behind_doc: bool,
    wrap: &'static str,
    dist: [f64; 4],
    fill: Fill,
    line: Option<Line>,
    prst: &'static str,
    adj: Option<f64>,
    body: Body,
}

/// `newTextboxDrawingXml` (content `<w:p/>`), as three pieces so the two edits of `patchShape`
/// (`docPr` and `xfrm` attributes) can be applied by position without a second generator.
fn new_textbox_xml(o: &NewTextbox) -> String {
    let cx = math::round(o.cx);
    let cy = math::round(o.cy);
    let [dt, db, dl, dr] = o.dist;
    let wrap = match o.wrap {
        "square" => "<wp:wrapSquare wrapText=\"bothSides\"/>",
        "topAndBottom" => "<wp:wrapTopAndBottom/>",
        _ => "<wp:wrapNone/>",
    };
    let ln = match &o.line {
        None => "<a:ln><a:noFill/></a:ln>".to_string(),
        Some(l) => line_xml(l),
    };
    let fill = fill_xml(&o.fill);
    let adj = o.adj.map(|v| format!("<a:gd name=\"adj\" fmla=\"val {}\"/>", js(math::round(v)))).unwrap_or_default();
    let av = if adj.is_empty() { "<a:avLst/>".to_string() } else { format!("<a:avLst>{adj}</a:avLst>") };
    let mut x = String::new();
    x.push_str("<w:drawing>");
    x.push_str(&format!(
        "<wp:anchor distT=\"{}\" distB=\"{}\" distL=\"{}\" distR=\"{}\" simplePos=\"0\" relativeHeight=\"{}\" behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">",
        js(dt),
        js(db),
        js(dl),
        js(dr),
        js(o.relative_height),
        u8::from(o.behind_doc)
    ));
    x.push_str("<wp:simplePos x=\"0\" y=\"0\"/>");
    x.push_str(&pos_xml("wp:positionH", &o.pos_h));
    x.push_str(&pos_xml("wp:positionV", &o.pos_v));
    x.push_str(&format!("<wp:extent cx=\"{}\" cy=\"{}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>{wrap}", js(cx), js(cy)));
    x.push_str(&format!("<wp:docPr id=\"{}\" name=\"{}\"/><wp:cNvGraphicFramePr/>", js(o.id), esc_attr(&o.name)));
    x.push_str("<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">");
    x.push_str("<wps:wsp><wps:cNvSpPr txBox=\"1\"/>");
    x.push_str(&format!(
        "<wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"{}\">{av}</a:prstGeom>{fill}{ln}</wps:spPr>",
        js(cx),
        js(cy),
        o.prst
    ));
    x.push_str("<wps:txbx><w:txbxContent><w:p/></w:txbxContent></wps:txbx>");
    x.push_str(&body_xml(&o.body));
    x.push_str("</wps:wsp></a:graphicData></a:graphic>");
    x.push_str("</wp:anchor></w:drawing>");
    x
}

/// The fields `vmlTextboxToDrawing` hands to `patchShape` (`ShapeChange`).
#[derive(Default)]
struct ShapeChange {
    rot: Option<f64>,
    flip_h: bool,
    flip_v: bool,
    descr: Option<String>,
    title: Option<String>,
    hidden: bool,
}

/// `\sKEY="[^"]*"`: the first match of `XmlPatch.setAttrs`'s per-attribute regex.
fn find_attr_span(tag: &str, key: &str) -> Option<(usize, usize)> {
    for (i, ch) in tag.char_indices() {
        if !is_js_space(ch) {
            continue;
        }
        let start = i + ch.len_utf8();
        let Some(rest) = tag.get(start..) else { continue };
        let Some(r) = rest.strip_prefix(key).and_then(|r| r.strip_prefix("=\"")) else { continue };
        let Some(close) = r.find('"') else { continue };
        return Some((i, start + key.len() + 2 + close + 1));
    }
    None
}

/// `XmlPatch.setAttrs` on the start tag of `el` (positions from the parse): the edit
/// `(start, end, new tag text)`.
fn set_attrs_edit(src: &str, el: Element<'_>, set: &[(&str, Option<String>)]) -> Option<(usize, usize, String)> {
    let p = el.positions()?;
    let end = p.content_start;
    let self_close = p.content_start == p.end;
    let mut tag = src.get(p.start..end)?.to_string();
    for (k, v) in set {
        match v {
            None => {
                if let Some((s, e)) = find_attr_span(&tag, k) {
                    tag.replace_range(s..e, "");
                }
            }
            Some(v) => {
                let a = format!(" {k}=\"{}\"", esc_attr(v));
                if let Some((s, e)) = find_attr_span(&tag, k) {
                    tag.replace_range(s..e, &a);
                } else if self_close {
                    if let Some(head) = tag.strip_suffix("/>") {
                        let kept = head.trim_end_matches(is_js_space).to_string();
                        tag = format!("{kept}{a}/>");
                    }
                } else if let Some(head) = tag.strip_suffix('>') {
                    tag = format!("{head}{a}>");
                }
            }
        }
    }
    Some((p.start, end, tag))
}

/// `XmlPatch.apply` for one edit.
fn apply_edit(src: &str, edit: (usize, usize, String)) -> Option<String> {
    let (s, e, text) = edit;
    Some(format!("{}{}{}", src.get(..s)?, text, src.get(e..)?))
}

fn parse_drawing(raw: &str) -> Result<uo_opc::xml::Document, DocxError> {
    Ok(parse(raw, &DOCX_POS)?)
}

fn container_of(doc: &uo_opc::xml::Document) -> Result<Element<'_>, DocxError> {
    let root = doc.root();
    root.first_child("wp:anchor")
        .or_else(|| root.first_child("wp:inline"))
        .ok_or(DocxError::Corrupt("w:drawing without wp:inline/wp:anchor"))
}

/// The spPr of a `w:drawing` (`partsOf`): `pic:spPr` of a picture, `wps:spPr` of a shape or
/// `wpg:grpSpPr` of a group, and whether any of the three holders exists.
fn sp_pr_of<'d>(container: Element<'d>) -> (bool, Option<Element<'d>>) {
    let gd = container.descend("a:graphicData");
    let pic = gd.and_then(|g| g.first_child("pic:pic"));
    let wsp = if pic.is_none() { gd.and_then(|g| g.first_child("wps:wsp")) } else { None };
    let wgp = if pic.is_none() && wsp.is_none() { gd.and_then(|g| g.first_child("wpg:wgp")) } else { None };
    let sp_pr = if let Some(p) = pic {
        p.first_child("pic:spPr")
    } else if let Some(w) = wsp {
        w.first_child("wps:spPr")
    } else {
        wgp.and_then(|g| g.first_child("wpg:grpSpPr"))
    };
    (pic.is_some() || wsp.is_some() || wgp.is_some(), sp_pr)
}

/// `patchShape(raw, ch)` for the fields of [`ShapeChange`]. `ch.wrap` (Tight/Through) is not
/// applied: `patchPicture` only acts on it when `readPictureFormat` (without `anyShape`) sees a
/// picture, and a generated `wps:wsp` is not one (declared, item 9).
fn patch_shape(raw: String, ch: &ShapeChange) -> Result<String, DocxError> {
    let mut xml = raw;
    check_container(&xml)?;
    let mut set: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(d) = ch.descr.as_deref().filter(|d| !d.is_empty()) {
        set.push(("descr", Some(d.to_string())));
    }
    if let Some(t) = ch.title.as_deref().filter(|t| !t.is_empty()) {
        set.push(("title", Some(t.to_string())));
    }
    if ch.hidden {
        set.push(("hidden", Some("1".to_string())));
    }
    if !set.is_empty() {
        let doc = parse_drawing(&xml)?;
        let container = container_of(&doc)?;
        if let Some(doc_pr) = container.first_child("wp:docPr")
            && let Some(edit) = set_attrs_edit(&xml, doc_pr, &set)
            && let Some(next) = apply_edit(&xml, edit)
        {
            xml = next;
        }
    }
    let mut xa: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(rot) = ch.rot {
        xa.push(("rot", truthy(rot).then(|| js(math::round(rot)))));
    }
    if ch.flip_h {
        xa.push(("flipH", Some("1".to_string())));
    }
    if ch.flip_v {
        xa.push(("flipV", Some("1".to_string())));
    }
    let doc = parse_drawing(&xml)?;
    let container = container_of(&doc)?;
    let (holder, sp_pr) = sp_pr_of(container);
    if holder
        && !xa.is_empty()
        && let Some(xfrm) = sp_pr.and_then(|s| s.first_child("a:xfrm"))
        && let Some(edit) = set_attrs_edit(&xml, xfrm, &xa)
        && let Some(next) = apply_edit(&xml, edit)
    {
        xml = next;
    }
    Ok(xml)
}

/// `patchContainer`'s `partsOf`: the text must parse and hold the container.
fn check_container(raw: &str) -> Result<(), DocxError> {
    let doc = parse_drawing(raw)?;
    container_of(&doc)?;
    Ok(())
}

/// `vmlTextboxToDrawing(el, id, lang)`: the `w:drawing` wps XML of an absolutely positioned VML
/// text box (`v:shape`, `v:rect`, `v:roundrect`, `v:oval` with a `v:textbox`); `None` for what is
/// not a simple floating box. `Err` is the TS exception of the patch step (the generated text
/// does not parse), which a raw VML attribute value can cause.
pub fn vml_textbox_to_drawing(el: Element<'_>, id: f64, lang: VmlLang) -> Result<Option<String>, DocxError> {
    let st = parse_vml_style(el.attr("style"));
    if st.get("position") != Some("absolute") {
        return Ok(None);
    }
    let Some(txb) = el.first_child("v:textbox") else { return Ok(None) };
    let cx = emu(st.get("width")).unwrap_or(0.0);
    let cy = emu(st.get("height")).unwrap_or(0.0);
    if cx.partial_cmp(&0.0) != Some(Ordering::Greater) || cy.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Ok(None);
    }
    let h = pos_of(&st, true);
    let v = pos_of(&st, false);
    let z = {
        let x = parse_int(st.get("z-index").unwrap_or("0"), 10);
        if truthy(x) { x } else { 0.0 }
    };
    let wrap_el = el.first_child("w10:wrap");
    let wt = wrap_el.and_then(|w| w.attr("type"));
    let dist =
        |k: &str| emu(st.get(&format!("mso-wrap-distance-{k}"))).unwrap_or(if k == "left" || k == "right" { 114_300.0 } else { 0.0 });
    let fill_el = el.first_child("v:fill");
    let filled = el.attr("filled").or_else(|| fill_el.and_then(|f| f.attr("on")));
    let fill = if filled == Some("f") || filled == Some("false") {
        Fill::None
    } else {
        let c1 = vml_color(fill_el.and_then(|f| f.attr("color")).or_else(|| el.attr("fillcolor"))).unwrap_or_else(|| "FFFFFF".to_string());
        let op = frac(fill_el.and_then(|f| f.attr("opacity")));
        let kind = fill_el.and_then(|f| f.attr("type"));
        if let (Some(f), true) = (fill_el, kind == Some("gradient") || kind == Some("gradientRadial")) {
            let c2 = vml_color(f.attr("color2")).unwrap_or_else(|| "FFFFFF".to_string());
            let a = parse_float(f.attr("angle").unwrap_or("0"));
            let ang = if truthy(a) { a } else { 0.0 };
            let turned = math::rem(math::rem(90.0 - ang, 360.0) + 360.0, 360.0) * 60000.0;
            Fill::Gradient { first: rgb(&c1, op), second: rgb(&c2, op), ang: turned }
        } else {
            Fill::Solid(rgb(&c1, op))
        }
    };
    let stroke_el = el.first_child("v:stroke");
    let stroked = el.attr("stroked").or_else(|| stroke_el.and_then(|s| s.attr("on")));
    let sw = el.attr("strokeweight").or_else(|| stroke_el.and_then(|s| s.attr("weight"))).filter(|s| !s.is_empty());
    let w = match sw {
        Some(sw) => {
            let ends_alpha = ends_with_ascii_lower(sw);
            let len = if ends_alpha { emu(Some(sw)) } else { emu(Some(&format!("{sw}pt"))) };
            len.unwrap_or(9525.0)
        }
        None => 9525.0,
    };
    let line = if stroked == Some("f") || stroked == Some("false") {
        None
    } else {
        let color =
            vml_color(stroke_el.and_then(|s| s.attr("color")).or_else(|| el.attr("strokecolor"))).unwrap_or_else(|| "000000".to_string());
        let mut l = Line { w, fill: rgb(&color, None), dash: None, cmpd: None };
        if let Some(d) = stroke_el.and_then(|s| s.attr("dashstyle")).filter(|d| !d.is_empty() && *d != "solid") {
            l.dash = Some(d.to_string());
        }
        if let Some(ls) = stroke_el.and_then(|s| s.attr("linestyle")).filter(|l| !l.is_empty() && *l != "single") {
            l.cmpd = match ls {
                "thinThin" => Some("dbl".to_string()),
                "thinThick" => Some("thinThick".to_string()),
                "thickThin" => Some("thickThin".to_string()),
                "thickBetweenThin" => Some("tri".to_string()),
                other => proto_string(other).map(String::from),
            };
        }
        Some(l)
    };
    let ts = parse_vml_style(txb.attr("style"));
    let ins: Vec<&str> = txb.attr("inset").unwrap_or("").split(',').collect();
    let inset = |i: usize, d: f64| -> f64 {
        match ins.get(i).map(|v| trim(v)).filter(|v| !v.is_empty()) {
            Some(v) => emu(Some(v)).unwrap_or(d),
            None => d,
        }
    };
    let va = st.get("v-text-anchor").unwrap_or("");
    let vert = match ts.get("layout-flow").filter(|f| !f.is_empty()) {
        Some(f) => {
            let alt = ts.get("mso-layout-flow-alt").unwrap_or("");
            flow(&format!("{f}|{alt}")).or_else(|| flow(&format!("{f}|")))
        }
        None => None,
    };
    let (prst, adj) = match el.name() {
        "v:roundrect" => ("roundRect", Some(math::round(frac(el.attr("arcsize")).unwrap_or(0.1667) * 100_000.0))),
        "v:oval" => ("ellipse", None),
        _ => ("rect", None),
    };
    let vml_name = el.attr("id").filter(|i| !i.is_empty() && !i.starts_with("_x0000_"));
    let name = match vml_name {
        Some(n) => n.to_string(),
        None => {
            let base = match lang {
                VmlLang::Pt => "Caixa de Texto",
                VmlLang::Es => "Cuadro de texto",
                VmlLang::En => "Text Box",
            };
            format!("{base} {}", js(id))
        }
    };
    let new = NewTextbox {
        id,
        name,
        cx,
        cy,
        pos_h: h,
        pos_v: v,
        relative_height: math::abs(z),
        behind_doc: z < 0.0,
        wrap: match wt {
            Some("square") => "square",
            Some("topAndBottom") => "topAndBottom",
            _ => "none",
        },
        dist: [dist("top"), dist("bottom"), dist("left"), dist("right")],
        fill,
        line,
        prst,
        adj,
        body: Body {
            l_ins: inset(0, 91440.0),
            t_ins: inset(1, 45720.0),
            r_ins: inset(2, 91440.0),
            b_ins: inset(3, 45720.0),
            anchor: if va.starts_with("middle") {
                "ctr"
            } else if va.starts_with("bottom") {
                "b"
            } else {
                "t"
            },
            vert: vert.unwrap_or("horz"),
            wrap_none: st.get("mso-wrap-style") == Some("none"),
            autofit_shape: ts.get("mso-fit-shape-to-text") == Some("t"),
        },
    };
    let xml = new_textbox_xml(&new);
    let rot = {
        let r = parse_float(st.get("rotation").unwrap_or("0"));
        if truthy(r) { r } else { 0.0 }
    };
    let flip = st.get("flip").unwrap_or("");
    let ch = ShapeChange {
        rot: truthy(rot).then(|| math::round(math::rem(math::rem(rot, 360.0) + 360.0, 360.0) * 60000.0)),
        flip_h: flip.contains('x'),
        flip_v: flip.contains('y'),
        descr: el.attr("alt").filter(|a| !a.is_empty()).map(String::from),
        title: el.attr("o:title").filter(|a| !a.is_empty()).map(String::from),
        hidden: st.get("visibility") == Some("hidden"),
    };
    Ok(Some(patch_shape(xml, &ch)?))
}

/// The `pos(axis)` closure of `vmlTextboxToDrawing`.
fn pos_of(st: &VmlStyle, horizontal: bool) -> Pos {
    let dir = if horizontal { "horizontal" } else { "vertical" };
    let rel = st.get(&format!("mso-position-{dir}-relative")).unwrap_or("text");
    let table = if horizontal { rel_h(rel) } else { rel_v(rel) };
    let relative_from = match table.or_else(|| proto_string(rel)) {
        Some(r) => r.to_string(),
        None => (if horizontal { "column" } else { "paragraph" }).to_string(),
    };
    let al = st.get(&format!("mso-position-{dir}")).filter(|a| !a.is_empty() && *a != "absolute");
    if let Some(al) = al {
        return Pos { relative_from, offset: None, align: Some(al.to_string()) };
    }
    let (margin, edge) = if horizontal { ("margin-left", "left") } else { ("margin-top", "top") };
    let off = emu(st.get(margin)).unwrap_or(0.0) + emu(st.get(edge)).unwrap_or(0.0);
    Pos { relative_from, offset: Some(off), align: None }
}

/// `vmlSingleTextbox(pict)`: the shape element of a `w:pict` that has one shape with a
/// `v:textbox` (`v:shapetype` and non-VML children do not count).
#[must_use]
pub fn vml_single_textbox<'d>(pict: Element<'d>) -> Option<Element<'d>> {
    let mut shapes = pict.child_elements().filter(|k| k.name() != "v:shapetype" && k.name().starts_with("v:"));
    let first = shapes.next()?;
    if shapes.next().is_some() {
        return None;
    }
    first.first_child("v:textbox").map(|_| first)
}
