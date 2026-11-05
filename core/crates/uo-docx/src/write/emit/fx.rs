//! Frontier **W6** (DrawingML write halves): `text-effects.ts` (`fxColorXml`, `fillXml`,
//! `textEffectsXml`; `W14_RPR` is [`crate::write::lossless::W14_RPR`]) and `picture-xml.ts`
//! (`escAttr`, `XmlPatch`, `ANCHOR_ORDER`, `SPPR_ORDER`, `dmlColorXml`, `picLineXml`,
//! `shadowXml`, `reflectionXml`, `effectLstXml`, `scene3dXml`, `sp3dXml`, `a14Xml`, `blipFxKids`,
//! `DEFAULT_WRAP_POLYGON`, `polygonXml`, `wrapXml`, `posXml`, `sizeRelXml`, `convertContainer`,
//! `patchPicture`, `switchWrap`, `patchContainer`, `patchLocks`, `patchNvPr`, `patchDecorative`,
//! `patchPic`, `patchBlipAttrs`, `patchBlipFx`, `patchA14`, `newPictureXml`). The read halves are
//! R1's (`read::text_effects`) and R4b's (`read::picture_format`, which also holds `dmlColorSpec`).
//!
//! The declarations of the nine kinds are in `core/docs/modules/uo-docx-d1b/W6.md`.
//!
//! `XmlPatch` is public: `shape-xml.ts` (W2) imports it from `picture-xml.ts`. A TS `throw`
//! (a drawing that does not parse, one without `wp:inline`/`wp:anchor`, a fill whose required
//! fields are missing: `TypeError`) is [`DocxError::Write`]; a text edit whose positions
//! overlap in a way the TS `slice` arithmetic would still make into some string is refused the
//! same way (declared, item 9: the Rust never slices inside a character).

use uo_jsnum::format::to_js_string;
use uo_jsnum::math;
use uo_jsnum::text::{is_js_space, trim_end};
use uo_opc::xml::Element;

use crate::error::DocxError;
use crate::js::{JsNumber, JsRecord, Nullable};
use crate::model::effects::{FxColor, FxFill, OpenTypeProps, TextEffects};
use crate::model::picture::{
    A14Props, Pic3D, PicBlipFx, PicColor, PicEffects, PicLine, PicPos, PicReflection, PicShadow, PictureFormat, SizeRel, WrapPolygon,
};
use crate::read::picture_format::{parse_drawing, parts_of, read_effects, read_picture_format};
use crate::write::js::{num_str, or_num, truthy_str};
use crate::write::lossless::{WResult, self_close_to_open_tail};

// ───────────────────────── helpers ─────────────────────────

/// `String(Math.round(x))` of a model number (`null` rounds to 0).
fn round_str(x: JsNumber) -> String {
    to_js_string(math::round(x.to_number()))
}

/// `String(Math.round(x))`.
fn round_f(x: f64) -> String {
    to_js_string(math::round(x))
}

fn upper(s: &str) -> String {
    uo_text::case::to_uppercase_str(s)
}

fn truthy_num(x: Option<JsNumber>) -> bool {
    x.is_some_and(JsNumber::truthy)
}

fn err(what: &'static str) -> DocxError {
    DocxError::Write(what)
}

// ───────────────────────── text-effects.ts ─────────────────────────

/// ` w14:${k}="${v}"` (the value unescaped).
fn at(k: &str, v: &str) -> String {
    format!(" w14:{k}=\"{v}\"")
}

/// `fxColorXml(c)`: `w14:srgbClr` or `w14:schemeClr` with the modifiers in the order stored (`val`
/// rounded). The scheme name and the modifier names are printed unescaped.
#[must_use]
pub fn fx_color_xml(c: &FxColor) -> String {
    let mods: String = c.mods.iter().flatten().map(|(k, v)| format!("<w14:{k}{}/>", at("val", &round_str(*v)))).collect();
    let tag = if truthy_str(c.scheme.as_deref()) { "schemeClr" } else { "srgbClr" };
    let val = match c.scheme.as_deref() {
        Some(s) => s.to_owned(),
        None => upper(c.rgb.as_deref().unwrap_or("000000")),
    };
    if mods.is_empty() { format!("<w14:{tag}{}/>", at("val", &val)) } else { format!("<w14:{tag}{}>{mods}</w14:{tag}>", at("val", &val)) }
}

/// `fillXml(f)`: a fill whose required parts are missing throws a `TypeError` in the TS (here
/// [`DocxError::Write`]).
fn fill_xml(f: &FxFill) -> WResult<String> {
    match &*f.r#type {
        "none" => return Ok("<w14:noFill/>".to_owned()),
        "solid" => {
            let c = f.color.as_ref().ok_or(err("fillXml: a solid fill without a colour (TS TypeError)"))?;
            return Ok(format!("<w14:solidFill>{}</w14:solidFill>", fx_color_xml(c)));
        }
        _ => {}
    }
    let stops_in = f.stops.as_ref().ok_or(err("fillXml: a gradient without stops (TS TypeError)"))?;
    let stops: String =
        stops_in.iter().map(|s| format!("<w14:gs{}>{}</w14:gs>", at("pos", &round_str(s.pos)), fx_color_xml(&s.color))).collect();
    let shade = if let Some(p) = &f.path {
        format!(
            "<w14:path{}><w14:fillToRect{}{}{}{}/></w14:path>",
            at("path", &p.kind),
            at("l", &num_str(p.l)),
            at("t", &num_str(p.t)),
            at("r", &num_str(p.r)),
            at("b", &num_str(p.b))
        )
    } else {
        let ang = f.lin.as_ref().map_or(5_400_000.0, |l| l.ang.to_number());
        let scaled = f.lin.as_ref().is_some_and(|l| l.scaled);
        format!("<w14:lin{}{}/>", at("ang", &round_f(ang)), at("scaled", if scaled { "1" } else { "0" }))
    };
    Ok(format!("<w14:gradFill><w14:gsLst>{stops}</w14:gsLst>{shade}</w14:gradFill>"))
}

/// `textEffectsXml(fx, ot)`: the w14 children of a `w:rPr` in the schema order (glow, shadow,
/// reflection, textOutline, textFill, scene3d, props3d, ligatures, numForm, numSpacing,
/// stylisticSets, cntxtAlts); empty without effects and typography.
pub fn text_effects_xml(fx: Option<&TextEffects>, ot: Option<&OpenTypeProps>) -> WResult<String> {
    let mut x = String::new();
    if let Some(fx) = fx {
        if let Some(g) = &fx.glow {
            x.push_str(&format!("<w14:glow{}>{}</w14:glow>", at("rad", &round_str(g.rad)), fx_color_xml(&g.color)));
        }
        if let Some(s) = &fx.shadow {
            x.push_str(&format!(
                "<w14:shadow{}{}{}{}{}{}{}{}>{}</w14:shadow>",
                at("blurRad", &round_str(s.blur_rad)),
                at("dist", &round_str(s.dist)),
                at("dir", &round_str(s.dir)),
                at("sx", &num_str(s.sx)),
                at("sy", &num_str(s.sy)),
                at("kx", &num_str(s.kx)),
                at("ky", &num_str(s.ky)),
                at("algn", &s.algn),
                fx_color_xml(&s.color)
            ));
        }
        if let Some(r) = &fx.reflection {
            x.push_str(&format!(
                "<w14:reflection{}{}{}{}{}{}{}{}{}{}{}{}{}/>",
                at("blurRad", &num_str(r.blur_rad)),
                at("stA", &num_str(r.st_a)),
                at("stPos", &num_str(r.st_pos)),
                at("endA", &num_str(r.end_a)),
                at("endPos", &num_str(r.end_pos)),
                at("dist", &num_str(r.dist)),
                at("dir", &num_str(r.dir)),
                at("fadeDir", &num_str(r.fade_dir)),
                at("sx", &num_str(r.sx)),
                at("sy", &num_str(r.sy)),
                at("kx", &num_str(r.kx)),
                at("ky", &num_str(r.ky)),
                at("algn", &r.algn)
            ));
        }
        if let Some(o) = &fx.outline {
            x.push_str("<w14:textOutline");
            if let Some(w) = o.w {
                x.push_str(&at("w", &round_str(w)));
            }
            if let Some(c) = o.cap.as_deref().filter(|c| !c.is_empty()) {
                x.push_str(&at("cap", c));
            }
            if let Some(c) = o.cmpd.as_deref().filter(|c| !c.is_empty()) {
                x.push_str(&at("cmpd", c));
            }
            if let Some(c) = o.algn.as_deref().filter(|c| !c.is_empty()) {
                x.push_str(&at("algn", c));
            }
            x.push('>');
            if let Some(f) = &o.fill {
                x.push_str(&fill_xml(f)?);
            }
            if let Some(d) = o.dash.as_deref().filter(|d| !d.is_empty()) {
                x.push_str(&format!("<w14:prstDash{}/>", at("val", d)));
            }
            match o.join.as_deref() {
                Some("round") => x.push_str("<w14:round/>"),
                Some("bevel") => x.push_str("<w14:bevel/>"),
                Some("miter") => {
                    let lim = o.miter_lim.map(|l| at("lim", &num_str(l))).unwrap_or_default();
                    x.push_str(&format!("<w14:miter{lim}/>"));
                }
                _ => {}
            }
            x.push_str("</w14:textOutline>");
        }
        if let Some(f) = &fx.fill {
            x.push_str(&format!("<w14:textFill>{}</w14:textFill>", fill_xml(f)?));
        }
        if let Some(s) = fx.scene3d.as_deref().filter(|s| !s.is_empty()) {
            x.push_str(s);
        }
        if let Some(s) = fx.props3d.as_deref().filter(|s| !s.is_empty()) {
            x.push_str(s);
        }
    }
    if let Some(ot) = ot {
        if let Some(v) = ot.ligatures.as_deref().filter(|v| !v.is_empty()) {
            x.push_str(&format!("<w14:ligatures{}/>", at("val", v)));
        }
        if let Some(v) = ot.num_form.as_deref().filter(|v| !v.is_empty()) {
            x.push_str(&format!("<w14:numForm{}/>", at("val", v)));
        }
        if let Some(v) = ot.num_spacing.as_deref().filter(|v| !v.is_empty()) {
            x.push_str(&format!("<w14:numSpacing{}/>", at("val", v)));
        }
        if let Some(sets) = ot.stylistic_sets.as_ref().filter(|s| !s.is_empty()) {
            let items: String = sets.iter().map(|n| format!("<w14:styleSet{}/>", at("id", &num_str(*n)))).collect();
            x.push_str(&format!("<w14:stylisticSets>{items}</w14:stylisticSets>"));
        }
        if ot.cntxt_alts == Some(true) {
            x.push_str("<w14:cntxtAlts/>");
        }
    }
    Ok(x)
}

// ───────────────────────── picture-xml.ts: constants and escapes ─────────────────────────

pub const EMU_PER_PT: f64 = 12700.0;
pub const EMU_PER_TW: f64 = 635.0;

/// `escAttr(s)`: `& < > "`.
#[must_use]
pub fn esc_attr(s: &str) -> String {
    crate::write::js::esc_attr(s)
}

/// `ANCHOR_ORDER`: CT_Anchor.
pub const ANCHOR_ORDER: [&str; 15] = [
    "wp:simplePos",
    "wp:positionH",
    "wp:positionV",
    "wp:extent",
    "wp:effectExtent",
    "wp:wrapNone",
    "wp:wrapSquare",
    "wp:wrapTight",
    "wp:wrapThrough",
    "wp:wrapTopAndBottom",
    "wp:docPr",
    "wp:cNvGraphicFramePr",
    "a:graphic",
    "wp14:sizeRelH",
    "wp14:sizeRelV",
];
/// `INLINE_ORDER`: CT_Inline.
pub const INLINE_ORDER: [&str; 5] = ["wp:extent", "wp:effectExtent", "wp:docPr", "wp:cNvGraphicFramePr", "a:graphic"];
/// `SPPR_ORDER`: CT_ShapeProperties.
pub const SPPR_ORDER: [&str; 15] = [
    "a:xfrm",
    "a:custGeom",
    "a:prstGeom",
    "a:noFill",
    "a:solidFill",
    "a:gradFill",
    "a:blipFill",
    "a:pattFill",
    "a:grpFill",
    "a:ln",
    "a:effectLst",
    "a:effectDag",
    "a:scene3d",
    "a:sp3d",
    "a:extLst",
];
/// `EFFECT_ORDER`.
pub const EFFECT_ORDER: [&str; 8] =
    ["a:blur", "a:fillOverlay", "a:glow", "a:innerShdw", "a:outerShdw", "a:prstShdw", "a:reflection", "a:softEdge"];
/// `BLIPFILL_ORDER`.
pub const BLIPFILL_ORDER: [&str; 4] = ["a:blip", "a:srcRect", "a:tile", "a:stretch"];
/// `WRAP_NAMES`.
pub const WRAP_NAMES: [&str; 5] = ["wp:wrapNone", "wp:wrapSquare", "wp:wrapTight", "wp:wrapThrough", "wp:wrapTopAndBottom"];
/// `BLIP_ORDER`: the children of an `a:blip` in the order Word writes them (declared in the TS,
/// not read by it).
pub const BLIP_ORDER: [&str; 18] = [
    "a:alphaBiLevel",
    "a:alphaCeiling",
    "a:alphaFloor",
    "a:alphaInv",
    "a:alphaMod",
    "a:alphaRepl",
    "a:clrChange",
    "a:clrRepl",
    "a:duotone",
    "a:grayscl",
    "a:biLevel",
    "a:blur",
    "a:fillOverlay",
    "a:hsl",
    "a:tint",
    "a:lum",
    "a:alphaModFix",
    "a:extLst",
];

const A14: &str = "xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\"";
const ADEC_EXT: &str = "<a:ext uri=\"{C183D7F6-B498-43B3-948B-1728B52AA6E4}\"><adec:decorative xmlns:adec=\"http://schemas.microsoft.com/office/drawing/2017/decorative\" val=\"1\"/></a:ext>";
const IMGPROPS_URI: &str = "{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}";
const ADEC_URI: &str = "{C183D7F6-B498-43B3-948B-1728B52AA6E4}";
const NS_A: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

// ───────────────────────── XmlPatch ─────────────────────────

/// Replacements by position in a raw XML text (`parseXml` with positions), applied from the end
/// to the start so the positions read stay valid until [`XmlPatch::apply`] (`XmlPatch`).
#[derive(Debug)]
pub struct XmlPatch<'s> {
    src: &'s str,
    edits: Vec<(usize, usize, String)>,
}

struct Pos {
    s: usize,
    cs: usize,
    ce: usize,
    e: usize,
}

fn pos(el: Element<'_>) -> WResult<Pos> {
    let p = el.positions().ok_or(err("an element without source positions (TS `!` on `s`)"))?;
    Ok(Pos { s: p.start, cs: p.content_start, ce: p.content_end, e: p.end })
}

/// `/\sKEY="[^"]*"/` (the key's `:` and `.` escaped, the rest literal): the first match in `tag`.
#[must_use]
pub fn find_attr_span(tag: &str, key: &str) -> Option<(usize, usize)> {
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

/// One entry of the `set` of `XmlPatch.setAttrs` applied to the start tag `tag`: `None` removes the
/// first `\sKEY="…"` (its leading white space included); a value (escaped with `escAttr`) replaces
/// the first match or is appended before the `/>` (`self_close`, the white space before it dropped)
/// or the `>` of the tag; a tag that does not end so is left as it was.
#[must_use]
pub fn set_attr_in_tag(tag: &str, k: &str, v: Option<&str>, self_close: bool) -> String {
    let mut tag = tag.to_owned();
    match v {
        None => {
            if let Some((a, b)) = find_attr_span(&tag, k) {
                tag.replace_range(a..b, "");
            }
        }
        Some(v) => {
            let a = format!(" {k}=\"{}\"", esc_attr(v));
            if let Some((x, y)) = find_attr_span(&tag, k) {
                tag.replace_range(x..y, &a);
            } else if self_close {
                if let Some(head) = tag.strip_suffix("/>") {
                    let kept = trim_end(head).to_owned();
                    tag = format!("{kept}{a}/>");
                }
            } else if let Some(head) = tag.strip_suffix('>') {
                tag = format!("{head}{a}>");
            }
        }
    }
    tag
}

impl<'s> XmlPatch<'s> {
    #[must_use]
    pub fn new(src: &'s str) -> XmlPatch<'s> {
        XmlPatch { src, edits: Vec::new() }
    }

    /// `XmlPatch.openEnd(el)`: just after the `>` of the start tag.
    pub fn open_end(el: Element<'_>) -> WResult<usize> {
        Ok(pos(el)?.cs)
    }

    /// `replace(el, text)`.
    pub fn replace(&mut self, el: Element<'_>, text: &str) -> WResult<()> {
        let p = pos(el)?;
        self.edits.push((p.s, p.e, text.to_owned()));
        Ok(())
    }

    /// `remove(el)`.
    pub fn remove(&mut self, el: Element<'_>) -> WResult<()> {
        self.replace(el, "")
    }

    /// `insertAt(at, text)`.
    pub fn insert_at(&mut self, at: usize, text: &str) {
        self.edits.push((at, at, text.to_owned()));
    }

    /// `setAttrs(el, set)`: `None` removes the attribute (the first `\sKEY="…"` match, its leading
    /// white space included); a value replaces the first match or is appended before the `>` /
    /// `/>` of the start tag (an inexistent end leaves the tag as it was), escaped with `escAttr`.
    /// `set` is in the object's insertion order.
    pub fn set_attrs(&mut self, el: Element<'_>, set: &[(&str, Option<String>)]) -> WResult<()> {
        let p = pos(el)?;
        let end = p.cs;
        let self_close = p.cs == p.e;
        let mut tag = self.src.get(p.s..end).ok_or(err("start tag outside the source"))?.to_owned();
        for (k, v) in set {
            tag = set_attr_in_tag(&tag, k, v.as_deref(), self_close);
        }
        self.edits.push((p.s, end, tag));
        Ok(())
    }

    /// `setContent(el, inner)`: the whole content of an element (a self-closing one becomes
    /// open/close).
    pub fn set_content(&mut self, el: Element<'_>, inner: &str) -> WResult<()> {
        let p = pos(el)?;
        if p.cs == p.e {
            let tag = self_close_to_open_tail(self.src.get(p.s..p.e).ok_or(err("element outside the source"))?);
            let name = open_tag_name(&tag).ok_or(err("setContent: no element name (TS TypeError)"))?;
            self.edits.push((p.s, p.e, format!("{tag}{inner}</{name}>")));
        } else {
            self.edits.push((p.cs, p.ce, inner.to_owned()));
        }
        Ok(())
    }

    /// `insertChild(parent, name, xml, order)`: `xml` as a child of `parent` at the place of the
    /// schema order: after the last child that is in `order` at or before `name`, else first.
    pub fn insert_child(&mut self, parent: Element<'_>, name: &str, xml: &str, order: &[&str]) -> WResult<()> {
        let rank = order.iter().position(|n| *n == name);
        let after =
            rank.and_then(|rank| parent.child_elements().rfind(|k| order.iter().position(|n| *n == k.name()).is_some_and(|i| i <= rank)));
        if let Some(after) = after {
            self.insert_at(pos(after)?.e, xml);
            return Ok(());
        }
        let p = pos(parent)?;
        if p.cs == p.e {
            return self.set_content(parent, xml);
        }
        self.insert_at(p.cs, xml);
        Ok(())
    }

    /// `putChild(parent, name, xml, order)`: replaces (or inserts, or removes with empty `xml`)
    /// the child `name`.
    pub fn put_child(&mut self, parent: Element<'_>, name: &str, xml: &str, order: &[&str]) -> WResult<()> {
        if let Some(cur) = parent.first_child(name) {
            return self.replace(cur, xml);
        }
        if !xml.is_empty() {
            return self.insert_child(parent, name, xml, order);
        }
        Ok(())
    }

    /// `apply()`: the edits from the last position to the first (stable for equal ones).
    pub fn apply(mut self) -> WResult<String> {
        self.edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut out = self.src.to_owned();
        for (s, e, text) in &self.edits {
            let (s, e) = ((*s).min(out.len()), (*e).min(out.len()));
            let head = out.get(..s).ok_or(err("overlapping edits cut a character"))?;
            let tail = out.get(e..).ok_or(err("overlapping edits cut a character"))?;
            out = format!("{head}{text}{tail}");
        }
        Ok(out)
    }
}

// ───────────────────────── colours, lines, effects ─────────────────────────

/// `dmlColorXml(c)`: `a:srgbClr`, `a:schemeClr`, … with its modifiers (`val` escaped, `lastClr`
/// and the kind unescaped).
#[must_use]
pub fn dml_color_xml(c: &PicColor) -> String {
    let last = c.last_clr.as_deref().filter(|l| !l.is_empty()).map(|l| format!(" lastClr=\"{l}\"")).unwrap_or_default();
    let attrs = format!(" val=\"{}\"{last}", esc_attr(&c.val));
    let mods: String = c
        .mods
        .iter()
        .flatten()
        .map(|(n, v)| {
            if matches!(&**n, "gray" | "comp" | "inv" | "gamma" | "invGamma") {
                format!("<a:{n}/>")
            } else {
                format!("<a:{n} val=\"{}\"/>", num_str(*v))
            }
        })
        .collect();
    if mods.is_empty() { format!("<a:{}{attrs}/>", c.kind) } else { format!("<a:{}{attrs}>{mods}</a:{}>", c.kind, c.kind) }
}

/// `num(k, v)`: ` k="${Math.round(v)}"`, nothing when `v` is `undefined`.
fn num_attr(k: &str, v: Option<JsNumber>) -> String {
    v.map(|v| format!(" {k}=\"{}\"", round_str(v))).unwrap_or_default()
}

fn str_attr(k: &str, v: Option<&str>) -> String {
    v.filter(|v| !v.is_empty()).map(|v| format!(" {k}=\"{v}\"")).unwrap_or_default()
}

fn bool_attr(k: &str, v: Option<bool>) -> String {
    v.map(|v| format!(" {k}=\"{}\"", i32::from(v))).unwrap_or_default()
}

/// `picLineXml(l)`: `a:ln`.
#[must_use]
pub fn pic_line_xml(l: &PicLine) -> String {
    let attrs = format!(
        "{}{}{}{}",
        num_attr("w", l.w),
        str_attr("cap", l.cap.as_deref()),
        str_attr("cmpd", l.cmpd.as_deref()),
        str_attr("algn", l.algn.as_deref())
    );
    let mut inner = String::new();
    match &l.fill {
        Some(Nullable::Null) => inner.push_str("<a:noFill/>"),
        Some(Nullable::Val(c)) => inner.push_str(&format!("<a:solidFill>{}</a:solidFill>", dml_color_xml(c))),
        None => {
            if let Some(raw) = l.fill_raw.as_deref().filter(|r| !r.is_empty()) {
                inner.push_str(raw);
            }
        }
    }
    if let Some(d) = l.dash.as_deref().filter(|d| !d.is_empty()) {
        inner.push_str(&format!("<a:prstDash val=\"{d}\"/>"));
    }
    match l.join.as_deref() {
        Some("round") => inner.push_str("<a:round/>"),
        Some("bevel") => inner.push_str("<a:bevel/>"),
        Some("miter") => match l.miter_lim {
            Some(lim) => inner.push_str(&format!("<a:miter lim=\"{}\"/>", num_str(lim))),
            None => inner.push_str("<a:miter/>"),
        },
        _ => {}
    }
    if let Some(t) = l.tail_raw.as_deref().filter(|t| !t.is_empty()) {
        inner.push_str(t);
    }
    if inner.is_empty() { format!("<a:ln{attrs}/>") } else { format!("<a:ln{attrs}>{inner}</a:ln>") }
}

/// `shadowXml(s)`: `a:outerShdw` / `a:innerShdw` / `a:prstShdw`.
#[must_use]
pub fn shadow_xml(s: &PicShadow) -> String {
    let tag = match &*s.kind {
        "outer" => "a:outerShdw",
        "inner" => "a:innerShdw",
        _ => "a:prstShdw",
    };
    let mut attrs =
        if &*s.kind == "prst" { format!(" prst=\"{}\"", s.prst.as_deref().unwrap_or("shdw1")) } else { num_attr("blurRad", s.blur_rad) };
    attrs.push_str(&num_attr("dist", s.dist));
    attrs.push_str(&num_attr("dir", s.dir));
    if &*s.kind == "outer" {
        attrs.push_str(&num_attr("sx", s.sx));
        attrs.push_str(&num_attr("sy", s.sy));
        attrs.push_str(&num_attr("kx", s.kx));
        attrs.push_str(&num_attr("ky", s.ky));
        attrs.push_str(&str_attr("algn", s.algn.as_deref()));
        attrs.push_str(&bool_attr("rotWithShape", s.rot_with_shape));
    }
    format!("<{tag}{attrs}>{}</{tag}>", dml_color_xml(&s.color))
}

/// `reflectionXml(r)`: `a:reflection`.
#[must_use]
pub fn reflection_xml(r: &PicReflection) -> String {
    format!(
        "<a:reflection{}{}{}{}{}{}{}{}{}{}{}{}{}{}/>",
        num_attr("blurRad", r.blur_rad),
        num_attr("stA", r.st_a),
        num_attr("stPos", r.st_pos),
        num_attr("endA", r.end_a),
        num_attr("endPos", r.end_pos),
        num_attr("dist", r.dist),
        num_attr("dir", r.dir),
        num_attr("fadeDir", r.fade_dir),
        num_attr("sx", r.sx),
        num_attr("sy", r.sy),
        num_attr("kx", r.kx),
        num_attr("ky", r.ky),
        str_attr("algn", r.algn.as_deref()),
        bool_attr("rotWithShape", r.rot_with_shape)
    )
}

/// `/^<([^\s/>]+)/.exec(tag)?.[1]`: the name right after the `<`.
#[must_use]
pub fn open_tag_name(tag: &str) -> Option<&str> {
    let rest = tag.strip_prefix('<')?;
    let n = rest.find(|c: char| is_js_space(c) || c == '/' || c == '>').unwrap_or(rest.len());
    (n > 0).then(|| rest.get(..n)).flatten()
}

/// The name of a raw effect element: `/^<([^\s/>]+)/.exec(raw)?.[1] ?? 'a:blur'`.
#[must_use]
pub fn raw_name(raw: &str) -> &str {
    open_tag_name(raw).unwrap_or("a:blur")
}

/// `effectLstXml(fx)`: `a:effectLst` in the schema order (empty without effects); the kinds the
/// order does not know (index −1) come first, stably.
#[must_use]
pub fn effect_lst_xml(fx: &PicEffects) -> String {
    let mut kids: Vec<(String, String)> = Vec::new();
    for raw in fx.other_raw.iter().flatten() {
        kids.push((raw_name(raw).to_owned(), raw.to_string()));
    }
    if let Some(g) = &fx.glow {
        kids.push(("a:glow".to_owned(), format!("<a:glow rad=\"{}\">{}</a:glow>", round_str(g.rad), dml_color_xml(&g.color))));
    }
    if let Some(s) = &fx.shadow {
        let name = match &*s.kind {
            "outer" => "a:outerShdw",
            "inner" => "a:innerShdw",
            _ => "a:prstShdw",
        };
        kids.push((name.to_owned(), shadow_xml(s)));
    }
    if let Some(r) = &fx.reflection {
        kids.push(("a:reflection".to_owned(), reflection_xml(r)));
    }
    if let Some(s) = fx.soft_edge {
        kids.push(("a:softEdge".to_owned(), format!("<a:softEdge rad=\"{}\"/>", round_str(s))));
    }
    let rank = |n: &str| EFFECT_ORDER.iter().position(|e| *e == n).map_or(-1i64, |i| i64::try_from(i).unwrap_or(0));
    kids.sort_by_key(|k| rank(&k.0));
    if kids.is_empty() {
        return String::new();
    }
    let inner: String = kids.into_iter().map(|k| k.1).collect();
    format!("<a:effectLst>{inner}</a:effectLst>")
}

fn rot_xml(r: Option<&crate::model::picture::Rot3>) -> String {
    r.map(|r| format!("<a:rot lat=\"{}\" lon=\"{}\" rev=\"{}\"/>", num_str(r.lat), num_str(r.lon), num_str(r.rev))).unwrap_or_default()
}

/// `scene3dXml(t)`: `a:scene3d` (empty without a camera and a light rig).
#[must_use]
pub fn scene3d_xml(t: &Pic3D) -> String {
    if t.camera.is_none() && t.light_rig.is_none() {
        return String::new();
    }
    let (cam_prst, fov, zoom, cam_rot) = match &t.camera {
        Some(c) => (c.prst.to_string(), c.fov, c.zoom, c.rot.as_ref()),
        None => ("orthographicFront".to_owned(), None, None, None),
    };
    let (rig, dir, lr_rot) = match &t.light_rig {
        Some(l) => (l.rig.to_string(), l.dir.to_string(), l.rot.as_ref()),
        None => ("threePt".to_owned(), "t".to_owned(), None),
    };
    let cam_attrs = format!("prst=\"{cam_prst}\"{}{}", num_attr("fov", fov), num_attr("zoom", zoom));
    let camera = if cam_rot.is_some() {
        format!("<a:camera {cam_attrs}>{}</a:camera>", rot_xml(cam_rot))
    } else {
        format!("<a:camera {cam_attrs}/>")
    };
    let light = if lr_rot.is_some() {
        format!("<a:lightRig rig=\"{rig}\" dir=\"{dir}\">{}</a:lightRig>", rot_xml(lr_rot))
    } else {
        format!("<a:lightRig rig=\"{rig}\" dir=\"{dir}\"/>")
    };
    format!("<a:scene3d>{camera}{light}</a:scene3d>")
}

fn bevel_xml(name: &str, b: Option<&crate::model::picture::Bevel>) -> String {
    b.map(|b| format!("<a:{name}{}{}{}/>", num_attr("w", b.w), num_attr("h", b.h), str_attr("prst", b.prst.as_deref()))).unwrap_or_default()
}

/// `sp3dXml(t)`: `a:sp3d` (empty when nothing is set).
#[must_use]
pub fn sp3d_xml(t: &Pic3D) -> String {
    let attrs = format!(
        "{}{}{}{}",
        num_attr("z", t.z),
        num_attr("extrusionH", t.extrusion_h),
        num_attr("contourW", t.contour_w),
        str_attr("prstMaterial", t.prst_material.as_deref())
    );
    let mut inner = bevel_xml("bevelT", t.bevel_t.as_ref());
    inner.push_str(&bevel_xml("bevelB", t.bevel_b.as_ref()));
    if let Some(c) = &t.extrusion_clr {
        inner.push_str(&format!("<a:extrusionClr>{}</a:extrusionClr>", dml_color_xml(c)));
    }
    if let Some(c) = &t.contour_clr {
        inner.push_str(&format!("<a:contourClr>{}</a:contourClr>", dml_color_xml(c)));
    }
    if attrs.is_empty() && inner.is_empty() {
        return String::new();
    }
    if inner.is_empty() { format!("<a:sp3d{attrs}/>") } else { format!("<a:sp3d{attrs}>{inner}</a:sp3d>") }
}

fn truthy_attr(k: &str, v: Option<JsNumber>) -> String {
    v.filter(|v| v.truthy()).map(|v| format!(" {k}=\"{}\"", num_str(v))).unwrap_or_default()
}

/// `a14Xml(a)`: the `a:ext` with `a14:imgProps` (empty without layer effects).
fn a14_xml(a: &A14Props) -> String {
    let mut fx: Vec<String> = a.other_raw.iter().flatten().map(ToString::to_string).collect();
    if let Some(v) = a.saturation {
        fx.push(format!("<a14:saturation sat=\"{}\"/>", num_str(v)));
    }
    if let Some(v) = a.color_temp {
        fx.push(format!("<a14:colorTemperature colorTemp=\"{}\"/>", num_str(v)));
    }
    if let Some(v) = a.sharpen {
        fx.push(format!("<a14:sharpenSoften amount=\"{}\"/>", num_str(v)));
    }
    if a.bright.is_some() || a.contrast.is_some() {
        fx.push(format!("<a14:brightnessContrast{}{}/>", truthy_attr("bright", a.bright), truthy_attr("contrast", a.contrast)));
    }
    if fx.is_empty() {
        return String::new();
    }
    let layer = a.layer_rid.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" r:embed=\"{r}\"")).unwrap_or_default();
    let effects: String = fx.iter().map(|f| format!("<a14:imgEffect>{f}</a14:imgEffect>")).collect();
    format!("<a:ext uri=\"{IMGPROPS_URI}\"><a14:imgProps {A14}><a14:imgLayer{layer}>{effects}</a14:imgLayer></a14:imgProps></a:ext>")
}

/// `blipFxKids(fx)`: the children of the `a:blip` (but the `a:extLst`) for the adjustments, with
/// their names.
fn blip_fx_kids(fx: &PicBlipFx) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    if let Some(c) = &fx.clr_change {
        let use_a = c.use_a.map(|u| format!(" useA=\"{}\"", i32::from(u))).unwrap_or_default();
        out.push((
            "a:clrChange",
            format!(
                "<a:clrChange{use_a}><a:clrFrom>{}</a:clrFrom><a:clrTo>{}</a:clrTo></a:clrChange>",
                dml_color_xml(&c.from),
                dml_color_xml(&c.to)
            ),
        ));
    }
    if let Some(d) = &fx.duotone {
        out.push(("a:duotone", format!("<a:duotone>{}{}</a:duotone>", dml_color_xml(&d.0), dml_color_xml(&d.1))));
    }
    if fx.grayscl == Some(true) {
        out.push(("a:grayscl", "<a:grayscl/>".to_owned()));
    }
    if let Some(b) = fx.bi_level {
        out.push(("a:biLevel", format!("<a:biLevel thresh=\"{}\"/>", num_str(b))));
    }
    if let Some(l) = &fx.lum
        && (truthy_num(l.bright) || truthy_num(l.contrast))
    {
        out.push(("a:lum", format!("<a:lum{}{}/>", truthy_attr("bright", l.bright), truthy_attr("contrast", l.contrast))));
    }
    if let Some(a) = fx.alpha_mod_fix
        && a.to_number() < 100_000.0
    {
        out.push(("a:alphaModFix", format!("<a:alphaModFix amt=\"{}\"/>", num_str(a))));
    }
    out
}

// ───────────────────────── operations ─────────────────────────

/// `PicWrapKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PicWrapKind {
    Inline,
    Square,
    Tight,
    Through,
    TopAndBottom,
    Behind,
    Front,
}

impl PicWrapKind {
    /// The TS string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PicWrapKind::Inline => "inline",
            PicWrapKind::Square => "square",
            PicWrapKind::Tight => "tight",
            PicWrapKind::Through => "through",
            PicWrapKind::TopAndBottom => "topAndBottom",
            PicWrapKind::Behind => "behind",
            PicWrapKind::Front => "front",
        }
    }
}

/// A side of `PictureChange.dist`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistSide {
    T,
    B,
    L,
    R,
}

/// `{ l, t, r, b }` of numbers (`srcRect`, `effectExtent`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect4 {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// `PictureChange.geom`: `{ prst, adj? }`.
#[derive(Debug, Clone, PartialEq)]
pub struct GeomChange {
    pub prst: String,
    pub adj: Option<JsRecord<JsNumber>>,
}

/// `PictureChange` (EMU, as the XML): `None` = `undefined` (no change); where the TS accepts
/// `null` (remove) the field is `Option<Nullable<…>>`. `dist` keeps the order of the keys of the
/// TS object (it decides the order of the attributes added to the tag).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PictureChange {
    pub wrap: Option<PicWrapKind>,
    /// the current offset of the picture, relative to the column (x) and the paragraph (y)
    pub at: Option<(f64, f64)>,
    pub new_relative_height: Option<f64>,
    pub wrap_text: Option<String>,
    pub dist: Option<Vec<(DistSide, f64)>>,
    pub pos_h: Option<PicPos>,
    pub pos_v: Option<PicPos>,
    pub locked: Option<bool>,
    pub layout_in_cell: Option<bool>,
    pub allow_overlap: Option<bool>,
    pub behind_doc: Option<bool>,
    pub relative_height: Option<f64>,
    pub polygon: Option<Nullable<WrapPolygon>>,
    pub size: Option<(f64, f64)>,
    pub size_rel_h: Option<Nullable<SizeRel>>,
    pub size_rel_v: Option<Nullable<SizeRel>>,
    pub effect_extent: Option<Rect4>,
    pub name: Option<String>,
    pub descr: Option<String>,
    pub title: Option<String>,
    pub decorative: Option<bool>,
    pub hidden: Option<bool>,
    pub no_change_aspect: Option<bool>,
    pub rot: Option<f64>,
    pub flip_h: Option<bool>,
    pub flip_v: Option<bool>,
    pub src_rect: Option<Rect4>,
    pub geom: Option<GeomChange>,
    pub line: Option<Nullable<PicLine>>,
    pub shadow: Option<Nullable<PicShadow>>,
    pub reflection: Option<Nullable<PicReflection>>,
    pub glow: Option<Nullable<crate::model::picture::PicGlow>>,
    pub soft_edge: Option<Nullable<f64>>,
    pub three_d: Option<Nullable<Pic3D>>,
    /// the blip adjustments (all but `a14`, which is the next field)
    pub blip_fx: Option<PicBlipFx>,
    pub a14: Option<Nullable<A14Props>>,
    /// the new `r:embed` of the `a:blip`
    pub embed: Option<String>,
}

/// `DEFAULT_WRAP_POLYGON`: the rectangle Word writes in the contour polygon (measured).
pub const DEFAULT_WRAP_POLYGON: [(f64, f64); 5] = [(0.0, 0.0), (0.0, 21300.0), (21375.0, 21300.0), (21375.0, 0.0), (0.0, 0.0)];

fn or_zero(x: JsNumber) -> String {
    match x {
        JsNumber::Null => "0".to_owned(),
        JsNumber::Num(v) => to_js_string(v),
    }
}

fn polygon_pts_xml(edited: bool, pts: &[(JsNumber, JsNumber)]) -> String {
    let (fx, fy) = pts.first().map_or(("0".to_owned(), "0".to_owned()), |p| (or_zero(p.0), or_zero(p.1)));
    let rest: String = pts.iter().skip(1).map(|(x, y)| format!("<wp:lineTo x=\"{}\" y=\"{}\"/>", num_str(*x), num_str(*y))).collect();
    format!("<wp:wrapPolygon edited=\"{}\"><wp:start x=\"{fx}\" y=\"{fy}\"/>{rest}</wp:wrapPolygon>", i32::from(edited))
}

fn polygon_xml(poly: Option<&WrapPolygon>) -> String {
    match poly {
        Some(p) => polygon_pts_xml(p.edited, &p.pts),
        None => {
            let pts: Vec<(JsNumber, JsNumber)> = DEFAULT_WRAP_POLYGON.iter().map(|&(x, y)| (JsNumber::Num(x), JsNumber::Num(y))).collect();
            polygon_pts_xml(false, &pts)
        }
    }
}

/// `wrapXml(kind, wrapText, polygon)`.
fn wrap_xml(kind: &str, wrap_text: &str, polygon: Option<&WrapPolygon>) -> String {
    match kind {
        "square" => format!("<wp:wrapSquare wrapText=\"{wrap_text}\"/>"),
        "tight" | "through" => {
            let tag = if kind == "tight" { "wp:wrapTight" } else { "wp:wrapThrough" };
            format!("<{tag} wrapText=\"{wrap_text}\">{}</{tag}>", polygon_xml(polygon))
        }
        "topAndBottom" => "<wp:wrapTopAndBottom/>".to_owned(),
        _ => "<wp:wrapNone/>".to_owned(),
    }
}

/// `posXml(tag, p)`: `wp:positionH` / `wp:positionV` (a percentage position goes in an
/// `mc:AlternateContent` with the absolute page position in the fallback, as Word).
fn pos_xml(tag: &str, p: &PicPos) -> String {
    if p.align.is_none() && p.pct.is_some() {
        let pct = if tag == "wp:positionH" { "wp14:pctPosHOffset" } else { "wp14:pctPosVOffset" };
        return format!(
            "<mc:AlternateContent><mc:Choice Requires=\"wp14\"><{tag} relativeFrom=\"{}\"><{pct}>{}</{pct}></{tag}></mc:Choice><mc:Fallback><{tag} relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></{tag}></mc:Fallback></mc:AlternateContent>",
            p.relative_from,
            round_str(p.pct.unwrap_or_default()),
            to_js_string(math::round(or_num(p.fallback, 0.0)))
        );
    }
    let inner = match &p.align {
        Some(a) => format!("<wp:align>{a}</wp:align>"),
        None => format!("<wp:posOffset>{}</wp:posOffset>", to_js_string(math::round(or_num(p.offset, 0.0)))),
    };
    format!("<{tag} relativeFrom=\"{}\">{inner}</{tag}>", p.relative_from)
}

/// `posHolder(c, tag)`: the element that keeps the position (the `wp:positionH/V` itself or the
/// `mc:AlternateContent` around it).
fn pos_holder<'d>(c: Element<'d>, tag: &str) -> Option<Element<'d>> {
    c.first_child(tag).or_else(|| c.children_named("mc:AlternateContent").find(|ac| ac.descend(tag).is_some()))
}

fn size_rel_xml(tag: &str, r: &SizeRel) -> String {
    let k = if tag == "wp14:sizeRelH" { "wp14:pctWidth" } else { "wp14:pctHeight" };
    format!("<{tag} relativeFrom=\"{}\"><{k}>{}</{k}></{tag}>", r.relative_from, round_str(r.pct))
}

/// `/^<([^:\s>]+):/.exec(s)?.[1]`.
#[must_use]
pub fn tag_prefix(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('<')?;
    let n = rest.find(|c: char| c == ':' || is_js_space(c) || c == '>')?;
    (n > 0 && rest.get(n..)?.starts_with(':')).then(|| rest.get(..n)).flatten()
}

/// The most a prefix replacement may produce (a prefix holding `` $` `` or `$'` copies the text
/// before or after each match: quadratic in the TS, refused here past this size).
const PREFIX_REPLACE_CAP: usize = 1 << 28;

/// `s.replace(/<(\/?)wp:/g, `<$1${pfx}:`)`: the replacement is a string template, so a `$` in
/// the prefix does what it does in the TS. `None` when the result would pass
/// [`PREFIX_REPLACE_CAP`] bytes (a `` $` `` or `$'` in the prefix on a large text).
#[must_use]
pub fn replace_wp_prefix(s: &str, pfx: &str) -> Option<String> {
    let template = format!("<$1{pfx}:");
    let plain = !pfx.contains('$');
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(rel) = s.get(from..).and_then(|r| r.find('<')) {
        let i = from + rel;
        from = i + 1;
        let after = s.get(i + 1..).unwrap_or("");
        let (slash, rest) = match after.strip_prefix('/') {
            Some(r) => ("/", r),
            None => ("", after),
        };
        if !rest.starts_with("wp:") {
            continue;
        }
        let end = i + 1 + slash.len() + 3;
        out.push_str(s.get(last..i).unwrap_or(""));
        if plain {
            out.push_str(&format!("<{slash}{pfx}:"));
        } else {
            let (before, matched, aft) = (s.get(..i).unwrap_or(""), s.get(i..end).unwrap_or(""), s.get(end..).unwrap_or(""));
            if out.len().saturating_add(before.len().max(aft.len())).saturating_add(template.len()) > PREFIX_REPLACE_CAP {
                return None;
            }
            out.push_str(&super::background::get_substitution(&template, matched, before, aft, &[Some(slash)]));
        }
        last = end;
        from = end;
    }
    out.push_str(s.get(last..).unwrap_or(""));
    Some(out)
}

fn drawing_err() -> DocxError {
    err("w:drawing without wp:inline/wp:anchor (or not XML)")
}

/// `convertContainer(raw, ch)`: converts the container (inline ↔ floating) keeping `wp:docPr`,
/// `wp:cNvGraphicFramePr` and `a:graphic` raw.
fn convert_container(raw: &str, ch: &PictureChange) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let c = p.container;
    let cp = pos(c)?;
    let slice = |n: &str| -> WResult<String> {
        match c.first_child(n) {
            Some(el) => {
                let q = pos(el)?;
                Ok(raw.get(q.s..q.e).ok_or(err("element outside the source"))?.to_owned())
            }
            None => Ok(String::new()),
        }
    };
    let keep = format!("{}{}", slice("wp:extent")?, slice("wp:effectExtent")?);
    let tail = format!("{}{}{}", slice("wp:docPr")?, slice("wp:cNvGraphicFramePr")?, slice("a:graphic")?);
    let head = raw.get(..cp.s).ok_or(err("container outside the source"))?;
    let foot = raw.get(cp.e..).ok_or(err("container outside the source"))?;
    let pfx = tag_prefix(raw.get(cp.s..).unwrap_or("")).unwrap_or("wp");
    let fix = |s: String| -> WResult<String> {
        if pfx == "wp" { Ok(s) } else { replace_wp_prefix(&s, pfx).ok_or(err("prefix replacement too large")) }
    };
    let wrap = ch.wrap.ok_or(err("convertContainer without a wrap (TS TypeError)"))?;
    if wrap == PicWrapKind::Inline {
        return Ok(format!(
            "{head}{}{foot}",
            fix(format!("<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">{keep}{tail}</wp:inline>"))?
        ));
    }
    let at = ch.at.unwrap_or((0.0, 0.0));
    let wrap_text = ch.wrap_text.as_deref().unwrap_or("bothSides");
    let behind = matches!(wrap, PicWrapKind::Behind | PicWrapKind::Tight | PicWrapKind::Through);
    let z = ch.new_relative_height.unwrap_or(251_658_240.0);
    let pos_h = pos_xml("wp:positionH", &PicPos { relative_from: "column".into(), offset: Some(JsNumber::Num(at.0)), ..PicPos::default() });
    let pos_v =
        pos_xml("wp:positionV", &PicPos { relative_from: "paragraph".into(), offset: Some(JsNumber::Num(at.1)), ..PicPos::default() });
    let anchor = format!(
        "<wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{}\" behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos x=\"0\" y=\"0\"/>{pos_h}{pos_v}{keep}{}{tail}\
         <wp14:sizeRelH relativeFrom=\"page\"><wp14:pctWidth>0</wp14:pctWidth></wp14:sizeRelH>\
         <wp14:sizeRelV relativeFrom=\"page\"><wp14:pctHeight>0</wp14:pctHeight></wp14:sizeRelV></wp:anchor>",
        to_js_string(z),
        i32::from(behind),
        wrap_xml(wrap.as_str(), wrap_text, None)
    );
    Ok(format!("{head}{}{foot}", fix(anchor)?))
}

/// `patchPicture(raw, ch)`: applies the changes to the raw `w:drawing` of a picture. Each step
/// parses the text again (the edits of a step never overlap).
pub fn patch_picture(raw: &str, ch: &PictureChange) -> WResult<String> {
    let mut xml = raw.to_owned();
    if let Some(wrap) = ch.wrap
        && let Some(cur) = read_picture_format(&xml, false)
    {
        if (wrap == PicWrapKind::Inline) != cur.inline {
            xml = convert_container(&xml, ch)?;
        } else if !cur.inline && wrap.as_str() != &*cur.wrap {
            xml = switch_wrap(&xml, &cur, wrap)?;
        }
    }
    xml = patch_container(&xml, ch)?;
    xml = patch_nv_pr(&xml, ch)?;
    if let Some(on) = ch.decorative {
        xml = patch_decorative(&xml, on)?;
    }
    if let Some(on) = ch.no_change_aspect {
        xml = patch_locks(&xml, on)?;
    }
    xml = patch_pic(&xml, ch)?;
    if let Some(e) = ch.embed.as_deref().filter(|e| !e.is_empty()) {
        xml = patch_blip_attrs(&xml, e)?;
    }
    if let Some(fx) = &ch.blip_fx {
        xml = patch_blip_fx(&xml, fx)?;
    }
    if let Some(a14) = &ch.a14 {
        xml = patch_a14(&xml, a14.as_val())?;
    }
    Ok(xml)
}

fn switch_wrap(raw: &str, cur: &PictureFormat, kind: PicWrapKind) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let c = p.container;
    let mut patch = XmlPatch::new(raw);
    let old = c.child_elements().find(|k| WRAP_NAMES.contains(&k.name()));
    let xml = wrap_xml(kind.as_str(), &cur.wrap_text, cur.polygon.as_deref());
    match old {
        Some(old) => patch.replace(old, &xml)?,
        None => patch.insert_child(c, "wp:wrapNone", &xml, &ANCHOR_ORDER)?,
    }
    let behind = matches!(kind, PicWrapKind::Behind | PicWrapKind::Tight | PicWrapKind::Through);
    if cur.behind_doc != behind {
        patch.set_attrs(c, &[("behindDoc", Some(i32::from(behind).to_string()))])?;
    }
    patch.apply()
}

fn flag(b: bool) -> Option<String> {
    Some(i32::from(b).to_string())
}

fn patch_container(raw: &str, ch: &PictureChange) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let c = p.container;
    let anchor = c.name() != "wp:inline";
    let mut patch = XmlPatch::new(raw);
    let mut attrs: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(dist) = &ch.dist {
        for (side, v) in dist {
            let k = match side {
                DistSide::T => "distT",
                DistSide::B => "distB",
                DistSide::L => "distL",
                DistSide::R => "distR",
            };
            attrs.push((k, Some(round_f(*v))));
        }
    }
    if anchor {
        if let Some(v) = ch.locked {
            attrs.push(("locked", flag(v)));
        }
        if let Some(v) = ch.layout_in_cell {
            attrs.push(("layoutInCell", flag(v)));
        }
        if let Some(v) = ch.allow_overlap {
            attrs.push(("allowOverlap", flag(v)));
        }
        if let Some(v) = ch.behind_doc {
            attrs.push(("behindDoc", flag(v)));
        }
        if let Some(v) = ch.relative_height {
            attrs.push(("relativeHeight", Some(round_f(v))));
        }
    }
    if !attrs.is_empty() {
        patch.set_attrs(c, &attrs)?;
    }
    let order: &[&str] = if anchor { &ANCHOR_ORDER } else { &INLINE_ORDER };
    if anchor {
        for (tag, p) in [("wp:positionH", &ch.pos_h), ("wp:positionV", &ch.pos_v)] {
            let Some(p) = p else { continue };
            match pos_holder(c, tag) {
                Some(cur) => patch.replace(cur, &pos_xml(tag, p))?,
                None => patch.insert_child(c, tag, &pos_xml(tag, p), &ANCHOR_ORDER)?,
            }
        }
        let wrap_el = c.child_elements().find(|k| WRAP_NAMES.contains(&k.name()));
        if let Some(w) = wrap_el
            && w.name() != "wp:wrapNone"
            && w.name() != "wp:wrapTopAndBottom"
        {
            let poly = w.name() == "wp:wrapTight" || w.name() == "wp:wrapThrough";
            if let (Some(change), true) = (&ch.polygon, poly) {
                let wt = ch.wrap_text.as_deref().or_else(|| w.attr("wrapText")).unwrap_or("bothSides");
                let tag = w.name();
                patch.replace(w, &format!("<{tag} wrapText=\"{wt}\">{}</{tag}>", polygon_xml(change.as_val())))?;
            } else if let Some(wt) = &ch.wrap_text {
                patch.set_attrs(w, &[("wrapText", Some(wt.clone()))])?;
            }
        }
        for (tag, change) in [("wp14:sizeRelH", &ch.size_rel_h), ("wp14:sizeRelV", &ch.size_rel_v)] {
            if let Some(change) = change {
                let xml = change.as_val().map(|r| size_rel_xml(tag, r)).unwrap_or_default();
                patch.put_child(c, tag, &xml, &ANCHOR_ORDER)?;
            }
        }
    }
    if let Some((cx, cy)) = ch.size {
        match c.first_child("wp:extent") {
            Some(ext) => patch.set_attrs(ext, &[("cx", Some(round_f(cx))), ("cy", Some(round_f(cy)))])?,
            None => patch.insert_child(c, "wp:extent", &format!("<wp:extent cx=\"{}\" cy=\"{}\"/>", round_f(cx), round_f(cy)), order)?,
        }
    }
    if let Some(e) = &ch.effect_extent {
        let (l, t, r, b) = (round_f(e.l), round_f(e.t), round_f(e.r), round_f(e.b));
        match c.first_child("wp:effectExtent") {
            Some(ee) => patch.set_attrs(ee, &[("l", Some(l)), ("t", Some(t)), ("r", Some(r)), ("b", Some(b))])?,
            None => {
                patch.insert_child(c, "wp:effectExtent", &format!("<wp:effectExtent l=\"{l}\" t=\"{t}\" r=\"{r}\" b=\"{b}\"/>"), order)?
            }
        }
    }
    patch.apply()
}

fn patch_locks(raw: &str, on: bool) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let c = p.container;
    let mut patch = XmlPatch::new(raw);
    let on_val = on.then(|| "1".to_owned());
    match c.descend("a:graphicFrameLocks") {
        Some(locks) => patch.set_attrs(locks, &[("noChangeAspect", on_val.clone())])?,
        None if on => {
            let lx = format!("<a:graphicFrameLocks {NS_A} noChangeAspect=\"1\"/>");
            match c.first_child("wp:cNvGraphicFramePr") {
                Some(fp) => patch.set_content(fp, &lx)?,
                None => {
                    let order: &[&str] = if c.name() == "wp:inline" { &INLINE_ORDER } else { &ANCHOR_ORDER };
                    patch.insert_child(c, "wp:cNvGraphicFramePr", &format!("<wp:cNvGraphicFramePr>{lx}</wp:cNvGraphicFramePr>"), order)?;
                }
            }
        }
        None => {}
    }
    if let Some(nv) = p.wsp.and_then(|w| w.first_child("wps:cNvSpPr")) {
        match nv.first_child("a:spLocks") {
            Some(sl) => {
                let others = sl.attrs().any(|a| a.name != "noChangeAspect");
                let np = pos(sl)?;
                if !on && !others && np.cs == np.e {
                    patch.remove(sl)?;
                } else {
                    patch.set_attrs(sl, &[("noChangeAspect", on_val)])?;
                }
            }
            None if on => {
                let q = pos(nv)?;
                if q.cs == q.e {
                    patch.set_content(nv, "<a:spLocks noChangeAspect=\"1\"/>")?;
                } else {
                    patch.insert_at(q.cs, "<a:spLocks noChangeAspect=\"1\"/>");
                }
            }
            None => {}
        }
    }
    patch.apply()
}

/// Sets `k` in an ordered attribute list: a key already there keeps its place.
fn set_key<'a>(list: &mut Vec<(&'a str, Option<String>)>, k: &'a str, v: Option<String>) {
    if let Some(e) = list.iter_mut().find(|e| e.0 == k) {
        e.1 = v;
    } else {
        list.push((k, v));
    }
}

/// `patchNvPr`: `wp:docPr` and `pic:cNvPr`: name, alt text and hidden (`descr`/`title` in both,
/// as Word; the `name` of the `cNvPr` — empty by default — gets the one of the `docPr`).
fn patch_nv_pr(raw: &str, ch: &PictureChange) -> WResult<String> {
    let mut set: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(n) = &ch.name {
        set_key(&mut set, "name", Some(n.clone()));
    }
    if let Some(d) = &ch.descr {
        set_key(&mut set, "descr", (!d.is_empty()).then(|| d.clone()));
    }
    if let Some(t) = &ch.title {
        set_key(&mut set, "title", (!t.is_empty()).then(|| t.clone()));
    }
    if let Some(h) = ch.hidden {
        set_key(&mut set, "hidden", h.then(|| "1".to_owned()));
    }
    if set.is_empty() {
        return Ok(raw.to_owned());
    }
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let Some(doc_pr) = p.container.first_child("wp:docPr") else { return Ok(raw.to_owned()) };
    let c_nv_pr = p.pic.and_then(|pic| pic.first_child("pic:nvPicPr")).and_then(|nv| nv.first_child("pic:cNvPr"));
    let mut patch = XmlPatch::new(raw);
    patch.set_attrs(doc_pr, &set)?;
    if let Some(c_nv_pr) = c_nv_pr {
        let mut nset: Vec<(&str, Option<String>)> = set.iter().filter(|e| e.0 != "hidden").cloned().collect();
        let alt = ch.descr.as_deref().is_some_and(|d| !d.is_empty()) || ch.title.as_deref().is_some_and(|t| !t.is_empty());
        if alt && c_nv_pr.attr("name").is_none_or(str::is_empty) {
            let name = ch.name.as_deref().or_else(|| doc_pr.attr("name")).unwrap_or("");
            set_key(&mut nset, "name", Some(name.to_owned()));
        }
        if !nset.is_empty() {
            patch.set_attrs(c_nv_pr, &nset)?;
        }
    }
    patch.apply()
}

/// `patchDecorative(raw, on)`: the `a:extLst` with the decorative `a:ext` on the `wp:docPr` and on
/// the `pic:cNvPr`.
fn patch_decorative(raw: &str, on: bool) -> WResult<String> {
    let mut xml = raw.to_owned();
    if on {
        let doc = parse_drawing(&xml).ok_or_else(drawing_err)?;
        let p = parts_of(&doc).ok_or_else(drawing_err)?;
        let doc_pr = p.container.first_child("wp:docPr");
        let c_nv_pr = p.pic.and_then(|pic| pic.first_child("pic:nvPicPr")).and_then(|nv| nv.first_child("pic:cNvPr"));
        if let (Some(doc_pr), Some(c_nv_pr)) = (doc_pr, c_nv_pr)
            && c_nv_pr.attr("name").is_none_or(str::is_empty)
            && let Some(n) = doc_pr.attr("name").filter(|n| !n.is_empty())
        {
            let mut patch = XmlPatch::new(&xml);
            patch.set_attrs(c_nv_pr, &[("name", Some(n.to_owned()))])?;
            let next = patch.apply()?;
            xml = next;
        }
    }
    for which in ["docPr", "cNvPr"] {
        let doc = parse_drawing(&xml).ok_or_else(drawing_err)?;
        let p = parts_of(&doc).ok_or_else(drawing_err)?;
        let el = if which == "docPr" {
            p.container.first_child("wp:docPr")
        } else {
            p.pic.and_then(|pic| pic.first_child("pic:nvPicPr")).and_then(|nv| nv.first_child("pic:cNvPr"))
        };
        let Some(el) = el else { continue };
        let mut patch = XmlPatch::new(&xml);
        let ext = el.first_child("a:extLst");
        let dec_ext = ext.and_then(|e| e.children_named("a:ext").find(|x| x.attr("uri") == Some(ADEC_URI)));
        if on {
            if dec_ext.is_some() {
                continue;
            }
            let ns_a = if which == "docPr" { format!(" {NS_A}") } else { String::new() };
            if let Some(ext) = ext {
                patch.insert_at(pos(ext)?.ce, ADEC_EXT);
            } else {
                let hl = el.child_elements().rfind(|k| k.name() == "a:hlinkClick" || k.name() == "a:hlinkHover");
                let lst = format!("<a:extLst{ns_a}>{ADEC_EXT}</a:extLst>");
                if let Some(hl) = hl {
                    patch.insert_at(pos(hl)?.e, &lst);
                } else {
                    let q = pos(el)?;
                    if q.cs == q.e {
                        patch.set_content(el, &lst)?;
                    } else {
                        patch.insert_at(q.ce, &lst);
                    }
                }
            }
        } else if let Some(dec) = dec_ext {
            let Some(ext) = ext else { continue };
            if ext.child_elements().count() == 1 {
                patch.remove(ext)?;
            } else {
                patch.remove(dec)?;
            }
        }
        let next = patch.apply()?;
        xml = next;
    }
    Ok(xml)
}

fn patch_pic(raw: &str, ch: &PictureChange) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    if p.pic.is_none() && p.wsp.is_none() && p.wgp.is_none() {
        return Ok(raw.to_owned());
    }
    let mut patch = XmlPatch::new(raw);
    if let Some(sp) = p.sp_pr {
        let xfrm = sp.first_child("a:xfrm");
        let mut xa: Vec<(&str, Option<String>)> = Vec::new();
        if let Some(r) = ch.rot {
            xa.push(("rot", uo_jsnum::conv::truthy(r).then(|| round_f(r))));
        }
        if let Some(v) = ch.flip_h {
            xa.push(("flipH", v.then(|| "1".to_owned())));
        }
        if let Some(v) = ch.flip_v {
            xa.push(("flipV", v.then(|| "1".to_owned())));
        }
        if let Some(xfrm) = xfrm {
            if !xa.is_empty() {
                patch.set_attrs(xfrm, &xa)?;
            }
            if let (Some(ext), Some((cx, cy))) = (xfrm.first_child("a:ext"), ch.size) {
                patch.set_attrs(ext, &[("cx", Some(round_f(cx))), ("cy", Some(round_f(cy)))])?;
            }
        }
        if let Some(g) = &ch.geom {
            let adj: String = g
                .adj
                .iter()
                .flat_map(JsRecord::iter)
                .map(|(n, v)| format!("<a:gd name=\"{n}\" fmla=\"val {}\"/>", round_str(*v)))
                .collect();
            let body = if adj.is_empty() { "<a:avLst/>".to_owned() } else { format!("<a:avLst>{adj}</a:avLst>") };
            let xml = format!("<a:prstGeom prst=\"{}\">{body}</a:prstGeom>", g.prst);
            match sp.first_child("a:custGeom") {
                Some(cust) => patch.replace(cust, &xml)?,
                None => patch.put_child(sp, "a:prstGeom", &xml, &SPPR_ORDER)?,
            }
        }
        if let Some(line) = &ch.line {
            patch.put_child(sp, "a:ln", &line.as_val().map(pic_line_xml).unwrap_or_default(), &SPPR_ORDER)?;
        }
        if ch.shadow.is_some() || ch.reflection.is_some() || ch.glow.is_some() || ch.soft_edge.is_some() {
            let mut fx = read_effects(sp.first_child("a:effectLst"));
            if let Some(s) = &ch.shadow {
                fx.shadow = s.as_val().map(|s| Box::new(s.clone()));
            }
            if let Some(r) = &ch.reflection {
                fx.reflection = r.as_val().map(|r| Box::new(r.clone()));
            }
            if let Some(g) = &ch.glow {
                fx.glow = g.as_val().map(|g| Box::new(g.clone()));
            }
            if let Some(s) = &ch.soft_edge {
                fx.soft_edge = s.as_val().filter(|v| uo_jsnum::conv::truthy(**v)).map(|v| JsNumber::Num(*v));
            }
            patch.put_child(sp, "a:effectLst", &effect_lst_xml(&fx), &SPPR_ORDER)?;
        }
        if let Some(t) = &ch.three_d {
            patch.put_child(sp, "a:scene3d", &t.as_val().map(scene3d_xml).unwrap_or_default(), &SPPR_ORDER)?;
            patch.put_child(sp, "a:sp3d", &t.as_val().map(sp3d_xml).unwrap_or_default(), &SPPR_ORDER)?;
        }
    }
    if let (Some(bf), Some(s)) = (p.blip_fill, &ch.src_rect) {
        let a = |k: &str, v: f64| if uo_jsnum::conv::truthy(v) { format!(" {k}=\"{}\"", round_f(v)) } else { String::new() };
        let attrs = format!("{}{}{}{}", a("l", s.l), a("t", s.t), a("r", s.r), a("b", s.b));
        patch.put_child(bf, "a:srcRect", &format!("<a:srcRect{attrs}/>"), &BLIPFILL_ORDER)?;
    }
    patch.apply()
}

fn patch_blip_attrs(raw: &str, embed: &str) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let Some(blip) = p.blip else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    patch.set_attrs(blip, &[("r:embed", Some(embed.to_owned()))])?;
    patch.apply()
}

/// `patchBlipFx`: replaces the set of colour adjustments of the `a:blip` (the `a:extLst` and the
/// rest stay).
fn patch_blip_fx(raw: &str, fx: &PicBlipFx) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let Some(blip) = p.blip else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    const MANAGED: [&str; 6] = ["a:alphaModFix", "a:lum", "a:grayscl", "a:biLevel", "a:duotone", "a:clrChange"];
    for k in blip.child_elements() {
        if MANAGED.contains(&k.name()) {
            patch.remove(k)?;
        }
    }
    let xml: String = blip_fx_kids(fx).into_iter().map(|k| k.1).collect();
    if !xml.is_empty() {
        let bp = pos(blip)?;
        if let Some(ext) = blip.first_child("a:extLst") {
            patch.insert_at(pos(ext)?.s, &xml);
        } else if bp.cs == bp.e {
            patch.set_content(blip, &xml)?;
        } else {
            patch.insert_at(bp.ce, &xml);
        }
    }
    patch.apply()
}

/// `patchA14`: `a14:imgProps` (the layer with the original and the Office 2010 effects).
fn patch_a14(raw: &str, a14: Option<&A14Props>) -> WResult<String> {
    let doc = parse_drawing(raw).ok_or_else(drawing_err)?;
    let p = parts_of(&doc).ok_or_else(drawing_err)?;
    let Some(blip) = p.blip else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    let ext = blip.first_child("a:extLst");
    let props = ext.and_then(|e| e.children_named("a:ext").find(|x| x.attr("uri") == Some(IMGPROPS_URI)));
    let xml = a14.map(a14_xml).unwrap_or_default();
    if let Some(props) = props {
        patch.replace(props, &xml)?;
    } else if !xml.is_empty() {
        let bp = pos(blip)?;
        if let Some(ext) = ext {
            patch.insert_at(pos(ext)?.cs, &xml);
        } else if bp.cs == bp.e {
            patch.set_content(blip, &format!("<a:extLst>{xml}</a:extLst>"))?;
        } else {
            patch.insert_at(bp.ce, &format!("<a:extLst>{xml}</a:extLst>"));
        }
    }
    patch.apply()
}

/// `newPictureXml(o)`: the `w:drawing` of a new inline picture as Word writes it on insert.
#[must_use]
pub fn new_picture_xml(id: f64, name: &str, rid: &str, cx: f64, cy: f64) -> String {
    let (cx, cy) = (round_f(cx), round_f(cy));
    let id = to_js_string(id);
    format!(
        "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
         <wp:docPr id=\"{id}\" name=\"{}\"/>\
         <wp:cNvGraphicFramePr><a:graphicFrameLocks {NS_A} noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
         <a:graphic {NS_A}><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"\"/><pic:cNvPicPr/></pic:nvPicPr>\
         <pic:blipFill><a:blip r:embed=\"{rid}\" cstate=\"print\"><a:extLst><a:ext uri=\"{{28A0092B-C50C-407E-A947-70E740481C1C}}\"><a14:useLocalDpi {A14} val=\"0\"/></a:ext></a:extLst></a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
         </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>",
        esc_attr(name)
    )
}
