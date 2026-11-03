//! Frontier R3 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `watermark.ts`,
//! read side: `isWatermarkXml` and `readWatermarkXml` (the editor reads the VML `w:pict` of a
//! header run, as text, to show the state of the Watermark dialog). The generator half of the
//! file (`watermarkRunXml`, the gallery, the box sizes) is D1b/D2b.
//!
//! Every regex of the file is a hand-written matcher (G9): the sites and their rows are in
//! `core/docs/modules/uo-docx-d1a/R3.md` and `js/regex-sites/r3.mjs`.

use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::{is_js_space, trim};

use super::matchers::{Cur, find_lit_then_boundary};
use crate::js::{JsNumber, JsValue, JsonWriter, Str};
use crate::js_object;

js_object! {
    /// The text watermark read from a header (`WatermarkInfo`, `kind: 'text'`; the spec's `lang`
    /// is never set by the reader).
    pub struct WatermarkTextInfo {
        req kind: Str = "kind",
        req text: Str = "text",
        req font: Str = "font",
        /// absent = automatic size (`font-size:1pt`)
        opt size_pt: JsNumber = "sizePt",
        req color: Str = "color",
        req semitransparent: bool = "semitransparent",
        /// diagonal | horizontal
        req layout: Str = "layout",
        req width_pt: JsNumber = "widthPt",
        req height_pt: JsNumber = "heightPt",
    }
}

js_object! {
    /// The picture watermark read from a header (`WatermarkInfo`, `kind: 'picture'`).
    pub struct WatermarkPictureInfo {
        req kind: Str = "kind",
        req rid: Str = "rid",
        req title: Str = "title",
        req washout: bool = "washout",
        req width_pt: JsNumber = "widthPt",
        req height_pt: JsNumber = "heightPt",
    }
}

/// `WatermarkInfo`: the watermark a header holds (a result type of the editor's dialogs, not
/// part of `FullDocument`).
#[derive(Debug, Clone, PartialEq)]
pub enum WatermarkInfo {
    Text(WatermarkTextInfo),
    Picture(WatermarkPictureInfo),
}

impl JsValue for WatermarkInfo {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match self {
            WatermarkInfo::Text(t) => t.canon(w),
            WatermarkInfo::Picture(p) => p.canon(w),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match self {
            WatermarkInfo::Text(t) => t.json(w),
            WatermarkInfo::Picture(p) => p.json(w),
        }
    }
}

const POWER_PLUS: &str = "PowerPlusWaterMarkObject";
const WORD_PICTURE: &str = "WordPictureWatermark";

/// After `id="`: one of the two object names, `\d*` and the closing quote; the name matched.
fn watermark_id(rest: &str) -> Option<&'static str> {
    let (name, after) =
        if let Some(r) = rest.strip_prefix(POWER_PLUS) { (POWER_PLUS, r) } else { (WORD_PICTURE, rest.strip_prefix(WORD_PICTURE)?) };
    let mut c = Cur::new(after);
    c.take_while(|ch| ch.is_ascii_digit());
    c.opt('"').then_some(name)
}

/// `isWatermarkXml(xml)`: `!!xml && /\bid="(?:PowerPlusWaterMarkObject|WordPictureWatermark)\d*"/.test(xml)`.
#[must_use]
pub fn is_watermark_xml(xml: Option<&str>) -> bool {
    let Some(xml) = xml.filter(|x| !x.is_empty()) else { return false };
    let mut from = 0;
    while let Some(rel) = xml.get(from..).and_then(|r| r.find("id=\"")) {
        let at = from + rel;
        from = at + 1;
        if Cur::at(xml, at).boundary() && xml.get(at + 4..).and_then(watermark_id).is_some() {
            return true;
        }
    }
    false
}

/// `attr` of `readWatermarkXml`: `new RegExp(`\s${name}="([^"]*)"`).exec(xml)` (the name is a
/// constant without metacharacters) and the four entity replacements, in the TS order.
#[must_use]
pub fn attr(xml: &str, name: &str) -> Option<String> {
    for (i, ch) in xml.char_indices() {
        if !is_js_space(ch) {
            continue;
        }
        let Some(rest) = xml.get(i + ch.len_utf8()..) else { continue };
        let Some(r) = rest.strip_prefix(name).and_then(|r| r.strip_prefix("=\"")) else { continue };
        let Some(end) = r.find('"') else { continue };
        let value = r.get(..end).unwrap_or("");
        return Some(value.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"));
    }
    None
}

/// `styleProp`: `new RegExp(`(?:^|;)\s*${k}\s*:\s*([^;]+)`).exec(style)?.[1]?.trim()` (`k` a
/// constant without metacharacters). The leftmost start wins (`^`, then each `;`); at a start the
/// group takes the non-`;` text after the colon and its white space, or, when nothing but `;`
/// or the end follows, the last white-space character (the greedy `\s*` gives one back).
#[must_use]
pub fn style_prop<'a>(style: &'a str, k: &str) -> Option<&'a str> {
    let at = |from: usize| -> Option<&'a str> {
        let mut c = Cur::at(style, from);
        c.spaces();
        if !c.lit(k) {
            return None;
        }
        c.spaces();
        if !c.opt(':') {
            return None;
        }
        let ws_start = c.pos();
        c.spaces();
        let body_start = c.pos();
        let body = c.take_while(|ch| ch != ';');
        if !body.is_empty() {
            return Some(trim(style.get(body_start..body_start + body.len())?));
        }
        let ws = style.get(ws_start..body_start)?;
        ws.chars().next_back().map(|_| "")
    };
    if let Some(v) = at(0) {
        return Some(v);
    }
    for (i, ch) in style.char_indices() {
        if ch == ';'
            && let Some(v) = at(i + 1)
        {
            return Some(v);
        }
    }
    None
}

/// `ptOf`: a CSS length of the watermark style in pt.
fn pt_of(v: Option<&str>) -> f64 {
    match v.filter(|s| !s.is_empty()) {
        None => 0.0,
        Some(v) => {
            let factor = if v.ends_with("in") {
                72.0
            } else if v.ends_with("cm") {
                72.0 / 2.54
            } else if v.ends_with("mm") {
                72.0 / 25.4
            } else {
                1.0
            };
            parse_float(v) * factor
        }
    }
}

/// The first `<name` followed by a word boundary and, up to the first `>`, its tag
/// (`/<name\b[^>]*>/.exec(xml)?.[0]`).
#[must_use]
pub fn find_open_tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let at = find_lit_then_boundary(xml, &format!("<{name}"))?;
    let gt = xml.get(at..)?.find('>')?;
    xml.get(at..=at + gt)
}

/// A match of the `v:shape` regex of `readWatermarkXml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeMatch<'a> {
    /// the whole match (`shape[0]`)
    pub text: &'a str,
    /// group 1: the object name
    pub kind: &'static str,
}

/// `/<v:shape\b[^>]*\bid="(PowerPlusWaterMarkObject|WordPictureWatermark)\d*"[^>]*>[\s\S]*?<\/v:shape>/.exec(xml)`:
/// per `<v:shape\b` the id is looked for inside the start tag (the greedy `[^>]*` makes the
/// rightmost occurrence win), and the match ends at the first `</v:shape>` after the tag.
#[must_use]
pub fn find_shape(xml: &str) -> Option<ShapeMatch<'_>> {
    let mut from = 0;
    while let Some(rel) = xml.get(from..).and_then(|r| r.find("<v:shape")) {
        let at = from + rel;
        let after = at + "<v:shape".len();
        from = at + 1;
        if !Cur::at(xml, after).boundary() {
            continue;
        }
        let Some(gt_rel) = xml.get(after..).and_then(|r| r.find('>')) else { continue };
        let gt = after + gt_rel;
        let region = xml.get(after..gt).unwrap_or("");
        let mut kind = None;
        let mut search = 0;
        while let Some(r) = region.get(search..).and_then(|s| s.find("id=\"")) {
            let p = search + r;
            search = p + 1;
            if Cur::at(xml, after + p).boundary()
                && let Some(k) = region.get(p + 4..).and_then(watermark_id)
            {
                kind = Some(k);
            }
        }
        let Some(kind) = kind else { continue };
        let Some(close) = xml.get(gt + 1..).and_then(|r| r.find("</v:shape>")) else { continue };
        let end = gt + 1 + close + "</v:shape>".len();
        return Some(ShapeMatch { text: xml.get(at..end)?, kind });
    }
    None
}

/// `/\bkey=/.test(s)`: `key` starts with a word character.
pub fn has_word_prefixed(s: &str, key: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s.get(from..).and_then(|r| r.find(key)) {
        let at = from + rel;
        if Cur::at(s, at).boundary() {
            return true;
        }
        from = at + 1;
    }
    false
}

/// `s.replace(/^["']|["']$/g, '')`: one leading and one trailing quote, never the same character.
#[must_use]
pub fn strip_quotes(s: &str) -> &str {
    let is_q = |c: char| c == '"' || c == '\'';
    let s = s.strip_prefix(is_q).unwrap_or(s);
    s.strip_suffix(is_q).unwrap_or(s)
}

/// `readWatermarkXml(xml, color)`: the watermark of a `w:pict` XML text, or `None`. `color`
/// resolves a VML colour (the editor passes `vmlColor`).
pub fn read_watermark_xml(xml: &str, color: &dyn Fn(Option<&str>) -> Option<String>) -> Option<WatermarkInfo> {
    let shape = find_shape(xml)?;
    let tag = find_open_tag(shape.text, "v:shape").unwrap_or("");
    let style = attr(tag, "style").unwrap_or_default();
    let width_pt = pt_of(style_prop(&style, "width"));
    let height_pt = pt_of(style_prop(&style, "height"));
    if shape.kind == WORD_PICTURE {
        let img = find_open_tag(shape.text, "v:imagedata").unwrap_or("");
        return Some(WatermarkInfo::Picture(WatermarkPictureInfo {
            kind: "picture".into(),
            rid: attr(img, "r:id").unwrap_or_default().into(),
            title: attr(img, "o:title").unwrap_or_default().into(),
            washout: has_word_prefixed(img, "gain=") || has_word_prefixed(img, "blacklevel="),
            width_pt: JsNumber::Num(width_pt),
            height_pt: JsNumber::Num(height_pt),
            ..WatermarkPictureInfo::default()
        }));
    }
    let tp = find_open_tag(shape.text, "v:textpath").unwrap_or("");
    let tps = attr(tp, "style").unwrap_or_default();
    let font = strip_quotes(style_prop(&tps, "font-family").unwrap_or("Calibri")).to_string();
    let fs = parse_float(style_prop(&tps, "font-size").unwrap_or("1"));
    let fill = find_open_tag(shape.text, "v:fill").unwrap_or("");
    let op = attr(fill, "opacity").filter(|o| !o.is_empty());
    let rot = parse_float(style_prop(&style, "rotation").unwrap_or("0"));
    let semitransparent = op.as_deref().is_some_and(|op| {
        let v = if op.ends_with('f') { parse_int(op, 10) / 65536.0 } else { parse_float(op) };
        v < 1.0
    });
    let diagonal = truthy(rot) && math::round(math::rem(math::rem(rot, 180.0) + 180.0, 180.0)) != 0.0;
    let mut info = WatermarkTextInfo {
        kind: "text".into(),
        text: attr(tp, "string").unwrap_or_default().into(),
        font: font.into(),
        color: color(attr(tag, "fillcolor").as_deref()).unwrap_or_else(|| "C0C0C0".to_string()).into(),
        semitransparent,
        layout: (if diagonal { "diagonal" } else { "horizontal" }).into(),
        width_pt: JsNumber::Num(width_pt),
        height_pt: JsNumber::Num(height_pt),
        ..WatermarkTextInfo::default()
    };
    if fs > 1.0 {
        info.size_pt = Some(JsNumber::Num(fs));
    }
    Some(WatermarkInfo::Text(info))
}
