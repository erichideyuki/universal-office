//! `numbering-store.ts`: the numbering part (`word/numbering.xml`) edited without loss. The text
//! of the part is the truth; what is not touched is written back byte for byte. The TS class
//! `DocNumbering` keeps the versions; here they are the session's [`VersionStore`] (U12), and
//! these are its operations over the text.
//!
//! Every regex of the TS is a hand matcher of [`super::rx`]. `Math.random` is the session's
//! [`Env`] (`hex8`).

use std::sync::Arc;

use uo_jsnum::format::to_js_string;

use super::super::super::env::Env;
use super::super::super::versions::VersionStore;
use super::rx::{all_digits_attr, digits_to_f64, element_range, next_open, num_range};
use crate::js::JsNumber;
use crate::model::formatting::{NumLevel, Numbering};

/// The start of a new part: only the namespaces the content uses.
const ROOT_OPEN: &str = "<w:numbering xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\" \
mc:Ignorable=\"w14 w15\">";
const PART_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// `hex8()`: `Math.floor(Math.random() * 0xffffffff)` as 8 upper-case hex digits.
pub(crate) fn hex8(env: &mut dyn Env) -> String {
    // the same IEEE operations as the TS: `(s / 2^32) * 0xffffffff`
    let r = f64::from(env.random_u32()) / 4_294_967_296.0;
    let v = (r * 4_294_967_295.0).floor();
    format!("{:08X}", uo_jsnum::conv::to_uint32(v))
}

/// The id as a JS number prints it.
pub(crate) fn id_str(x: f64) -> String {
    to_js_string(x)
}

/// `ensure()`: the part, or a new empty one.
pub(crate) fn ensure(st: &VersionStore) -> String {
    st.xml().map_or_else(|| format!("{PART_HEAD}{ROOT_OPEN}</w:numbering>"), |x| x.to_string())
}

/// `SHAPETYPE_75` of numbering-store.ts (the picture frame type Word writes once per part).
const SHAPETYPE_75: &str = "<v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\">\
<v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/><v:f eqn=\"sum @0 1 0\"/><v:f eqn=\"sum 0 0 @1\"/>\
<v:f eqn=\"prod @2 1 2\"/><v:f eqn=\"prod @3 21600 pixelWidth\"/><v:f eqn=\"prod @3 21600 pixelHeight\"/><v:f eqn=\"sum @0 0 1\"/>\
<v:f eqn=\"prod @6 1 2\"/><v:f eqn=\"prod @7 21600 pixelWidth\"/><v:f eqn=\"sum @8 21600 0\"/><v:f eqn=\"prod @7 21600 pixelHeight\"/>\
<v:f eqn=\"sum @10 21600 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" o:connecttype=\"rect\"/><o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>";

/// The prefixes a picture bullet needs on the root (`NS` of numbering-store.ts, in its order).
const PIC_NS: [(&str, &str); 3] = [
    ("v", "urn:schemas-microsoft-com:vml"),
    ("o", "urn:schemas-microsoft-com:office:office"),
    ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
];

/// `addPicBullet(img, wPt, hPt, title)` (numbering-store.ts 90, wave 7 A2): a new `w:numPicBullet` with the VML Word
/// writes (`v:shape o:bullet` of the natural size, `v:imagedata` by the part's relationship `rIdUwPb<n>`); `taken`
/// says whether a relationship id is already one of the store's images. Answers the `w:numPicBulletId` and the id.
pub(crate) fn add_pic_bullet(st: &mut VersionStore, taken: &dyn Fn(&str) -> bool, w_pt: f64, h_pt: f64, title: &str) -> (f64, String) {
    let mut xml = ensure(st);
    // `/<w:numPicBullet\b[^>]*\bw:numPicBulletId="(\d+)"/g`
    let mut max: Option<f64> = None;
    let mut at = 0;
    while let Some(k) = xml.get(at..).and_then(|r| r.find("<w:numPicBullet")) {
        let from = at + k + "<w:numPicBullet".len();
        at = from;
        if xml.as_bytes().get(from).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
            continue;
        }
        let Some(end) = xml.get(from..).and_then(|r| r.find('>')) else { break };
        let tag = xml.get(from..from + end).unwrap_or("");
        if let Some(i) = tag.find("w:numPicBulletId=\"") {
            let v = tag.get(i + "w:numPicBulletId=\"".len()..).unwrap_or("");
            let digits: String = v.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() && v.get(digits.len()..).is_some_and(|r| r.starts_with('"')) {
                let n: f64 = uo_jsnum::parse::to_number(&digits);
                max = Some(max.map_or(n, |m: f64| uo_jsnum::math::max(m, n)));
            }
        }
    }
    let id = max.map_or(0.0, |m| m + 1.0);
    let mut n = 1u32;
    while taken(&format!("rIdUwPb{n}")) || xml.contains(&format!("\"rIdUwPb{n}\"")) {
        n += 1;
    }
    let rid = format!("rIdUwPb{n}");
    // the root needs the v:, o: and r: prefixes
    if let Some(o) = xml.find("<w:numbering") {
        let o_end = xml.get(o..).and_then(|r| r.find('>')).map(|e| o + e + 1);
        if let Some(o_end) = o_end {
            let open = xml.get(o..o_end).unwrap_or("").to_owned();
            let mut open2 = open.clone();
            for (pfx, uri) in PIC_NS {
                if !open2.contains(&format!("xmlns:{pfx}=")) {
                    open2.pop();
                    open2.push_str(&format!(" xmlns:{pfx}=\"{uri}\">"));
                }
            }
            if open2 != open {
                xml = format!("{}{open2}{}", xml.get(..o).unwrap_or(""), xml.get(o_end..).unwrap_or(""));
            }
        }
    }
    let num = |v: f64| to_js_string(uo_jsnum::math::round(v * 100.0) / 100.0);
    let esc_t = title.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
    let shapetype = if xml.contains("id=\"_x0000_t75\"") { "" } else { SHAPETYPE_75 };
    let pb = format!(
        "<w:numPicBullet w:numPicBulletId=\"{}\"><w:pict>{shapetype}<v:shape id=\"_x0000_i{}\" type=\"#_x0000_t75\" style=\"width:{}pt;height:{}pt\" o:bullet=\"t\"><v:imagedata r:id=\"{rid}\" o:title=\"{esc_t}\"/></v:shape></w:pict></w:numPicBullet>",
        to_js_string(id),
        to_js_string(1025.0 + id),
        num(w_pt),
        num(h_pt),
    );
    // the w:numPicBullet come first (CT_Numbering)
    let at = match xml.rfind("</w:numPicBullet>") {
        Some(i) => i + "</w:numPicBullet>".len(),
        None => xml.find("<w:numbering").and_then(|o| xml.get(o..).and_then(|r| r.find('>')).map(|e| o + e + 1)).unwrap_or(xml.len()),
    };
    let out = format!("{}{pb}{}", xml.get(..at).unwrap_or(""), xml.get(at..).unwrap_or(""));
    set(st, out);
    (id, rid)
}

/// `set(xml)`: a new version unless the text is the current one.
pub(crate) fn set(st: &mut VersionStore, xml: String) {
    if st.xml().is_some_and(|x| **x == *xml) {
        return;
    }
    st.set(Some(Arc::from(xml)));
}

/// `/\sxmlns:w15="/.test(xml)`.
fn has_w15(xml: &str) -> bool {
    let mut at = 0;
    while let Some(p) = xml.get(at..).and_then(|r| r.find("xmlns:w15=\"")) {
        let q = at + p;
        if xml.get(..q).and_then(|h| h.chars().next_back()).is_some_and(uo_jsnum::text::is_js_space) {
            return true;
        }
        at = q + 1;
    }
    false
}

/// `abstractXml(id)`: the raw `w:abstractNum` by its id.
pub(crate) fn abstract_xml(st: &VersionStore, id: f64) -> Option<String> {
    let x = st.xml()?;
    abstract_in(x, id).map(str::to_owned)
}

pub(crate) fn abstract_in(x: &str, id: f64) -> Option<&str> {
    let (s, e) = element_range(x, "w:abstractNum", "w:abstractNumId", &id_str(id), "</w:abstractNum>")?;
    x.get(s..e)
}

/// `numXml(numId)`: the raw `w:num` (the paste family's, F8).
#[allow(dead_code)]
pub(crate) fn num_xml(st: &VersionStore, id: f64) -> Option<String> {
    let x = st.xml()?;
    let (s, e) = num_range(x, &id_str(id))?;
    x.get(s..e).map(str::to_owned)
}

/// The range of the `w:lvl` of `ilvl` in a raw abstract.
fn level_range(raw: &str, ilvl: f64) -> Option<(usize, usize)> {
    element_range(raw, "w:lvl", "w:ilvl", &id_str(ilvl), "</w:lvl>")
}

/// `levelXml(absId, ilvl)`: the raw `w:lvl` of an abstract.
pub(crate) fn level_xml(st: &VersionStore, abs: f64, ilvl: f64) -> Option<String> {
    let raw = abstract_xml(st, abs)?;
    let (s, e) = level_range(&raw, ilvl)?;
    raw.get(s..e).map(str::to_owned)
}

/// `nextId('w:abstractNumId' | 'w:numId')`.
pub(crate) fn next_id(st: &VersionStore, abstract_ids: bool) -> f64 {
    let x = st.xml().map_or("", |x| &**x);
    let (name, attr, mut max) = if abstract_ids { ("w:abstractNum", "w:abstractNumId", -1.0_f64) } else { ("w:num", "w:numId", 0.0) };
    for d in all_digits_attr(x, name, attr) {
        // `Math.max` of numbers (never NaN here: digits)
        max = uo_jsnum::math::max(max, digits_to_f64(d));
    }
    max + 1.0
}

/// The options of `addList`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ListOpts<'a> {
    pub multi_level_type: &'a str,
    pub tmpl: Option<&'a str>,
    pub style_link: Option<&'a str>,
    pub num_style_link: Option<&'a str>,
}

/// `insertAbstract(abs)`: before the first `w:num`, `w:numIdMacAtCleanup` or the closing tag at
/// the end of the part; a part without any of them takes it one character before its end (the
/// TS `slice(0, -1)` on `search() === -1`).
fn insert_abstract(st: &mut VersionStore, abs: &str) {
    let x = ensure(st);
    let at = search_anchor(&x, true);
    set(st, splice_at(&x, at, abs));
}

/// `x.search(/<w:num\b|<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/)` (`with_num`) or
/// `x.search(/<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/)`; `-1` as `None`.
pub(crate) fn search_anchor(x: &str, with_num: bool) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut take = |p: Option<usize>| {
        if let Some(p) = p {
            best = Some(best.map_or(p, |b| b.min(p)));
        }
    };
    if with_num {
        take(next_open(x, "w:num", 0));
    }
    take(next_open(x, "w:numIdMacAtCleanup", 0));
    // `<\/w:numbering>\s*$`: the first closing tag followed only by white space up to the end
    let mut at = 0;
    while let Some(p) = x.get(at..).and_then(|r| r.find("</w:numbering>")) {
        let q = at + p;
        let tail = x.get(q + "</w:numbering>".len()..).unwrap_or("");
        if uo_jsnum::text::trim(tail).is_empty() {
            take(Some(q));
            break;
        }
        at = q + 1;
    }
    best
}

/// `x.slice(0, at) + ins + x.slice(at)` with `at = -1` meaning `slice(0, -1)` / `slice(-1)`.
fn splice_at(x: &str, at: Option<usize>, ins: &str) -> String {
    let at = at.unwrap_or_else(|| x.char_indices().next_back().map_or(0, |(i, _)| i));
    let mut out = String::with_capacity(x.len() + ins.len());
    out.push_str(x.get(..at).unwrap_or(""));
    out.push_str(ins);
    out.push_str(x.get(at..).unwrap_or(""));
    out
}

/// `addNum(absId, overrides)`: a new `w:num` for an abstract.
pub(crate) fn add_num(st: &mut VersionStore, abs: f64, overrides: &str) -> f64 {
    let num_id = next_id(st, false);
    let num = format!("<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{}\"/>{overrides}</w:num>", id_str(num_id), id_str(abs));
    let x = ensure(st);
    let at = search_anchor(&x, false);
    set(st, splice_at(&x, at, &num));
    num_id
}

/// `addList(levels, opts)`: a new `w:abstractNum` (random `nsid` and `tmpl`, as Word; the heading
/// models have a fixed `tmpl`) and its `w:num`. Returns the numId.
pub(crate) fn add_list(st: &mut VersionStore, env: &mut dyn Env, levels: &[String], o: &ListOpts<'_>) -> f64 {
    let abs_id = next_id(st, true);
    let w15 = if has_w15(&ensure(st)) { " w15:restartNumberingAfterBreak=\"0\"" } else { "" };
    // template literal order: nsid first, then the tmpl when none is given
    let nsid = hex8(env);
    let tmpl = match o.tmpl {
        Some(t) => t.to_owned(),
        None => hex8(env),
    };
    let mut abs = format!(
        "<w:abstractNum w:abstractNumId=\"{}\"{w15}><w:nsid w:val=\"{nsid}\"/><w:multiLevelType w:val=\"{}\"/><w:tmpl w:val=\"{tmpl}\"/>",
        id_str(abs_id),
        o.multi_level_type
    );
    if let Some(l) = o.style_link.filter(|s| !s.is_empty()) {
        abs.push_str(&format!("<w:styleLink w:val=\"{l}\"/>"));
    }
    if let Some(l) = o.num_style_link.filter(|s| !s.is_empty()) {
        abs.push_str(&format!("<w:numStyleLink w:val=\"{l}\"/>"));
    }
    for l in levels {
        abs.push_str(l);
    }
    abs.push_str("</w:abstractNum>");
    insert_abstract(st, &abs);
    add_num(st, abs_id, "")
}

/// `/w:abstractNumId="\d+"/` replaced once.
pub(crate) fn replace_abstract_id(raw: &str, id: f64) -> String {
    let needle = "w:abstractNumId=\"";
    let mut at = 0;
    while let Some(p) = raw.get(at..).and_then(|r| r.find(needle)) {
        let q = at + p;
        let vs = q + needle.len();
        let digits = raw.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if digits > 0 && raw.as_bytes().get(vs + digits) == Some(&b'"') {
            return format!("{}w:abstractNumId=\"{}\"{}", &raw[..q], id_str(id), &raw[vs + digits + 1..]);
        }
        at = q + 1;
    }
    raw.to_owned()
}

/// `raw.replace(/<w:nsid w:val="[0-9A-Fa-f]+"\/>/, () => to)`.
pub(crate) fn replace_hex_element(raw: &str, prefix: &str, to: &str) -> String {
    let mut at = 0;
    while let Some(p) = raw.get(at..).and_then(|r| r.find(prefix)) {
        let q = at + p;
        let vs = q + prefix.len();
        let hex = raw.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_hexdigit).count());
        if hex > 0 && raw.get(vs + hex..).is_some_and(|r| r.starts_with("\"/>")) {
            return format!("{}{to}{}", &raw[..q], &raw[vs + hex + 3..]);
        }
        at = q + 1;
    }
    raw.to_owned()
}

/// `.replace(/(<w:lvl\b[^>]*?) w:tplc="[0-9A-Fa-f]+"/g, '$1 w:tplc="FFFFFFFF"')`.
pub(crate) fn blank_tplc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    while let Some(s) = next_open(raw, "w:lvl", at) {
        let a = s + "<w:lvl".len();
        let Some(gt) = raw.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
        // lazy `[^>]*?` then ` w:tplc="HEX+"`: the first occurrence in the tag
        let mut hit: Option<(usize, usize)> = None;
        let mut from = a;
        while let Some(p) = raw.get(from..gt).and_then(|r| r.find(" w:tplc=\"")) {
            let q = from + p;
            let vs = q + " w:tplc=\"".len();
            let hex = raw.get(vs..gt).map_or(0, |r| r.bytes().take_while(u8::is_ascii_hexdigit).count());
            if hex > 0 && raw.as_bytes().get(vs + hex) == Some(&b'"') {
                hit = Some((q, vs + hex + 1));
                break;
            }
            from = q + 1;
        }
        match hit {
            Some((q, end)) => {
                out.push_str(raw.get(at..q).unwrap_or(""));
                out.push_str(" w:tplc=\"FFFFFFFF\"");
                at = end;
            }
            None => {
                out.push_str(raw.get(at..gt + 1).unwrap_or(""));
                at = gt + 1;
            }
        }
    }
    out.push_str(raw.get(at..).unwrap_or(""));
    out
}

/// `copyAbstract(absId)`: a copy with a new `nsid` (same `tmpl`; the `w:tplc` become FFFFFFFF).
pub(crate) fn copy_abstract(st: &mut VersionStore, env: &mut dyn Env, abs: f64) -> Option<f64> {
    let raw = abstract_xml(st, abs)?;
    let id = next_id(st, true);
    let copy = replace_abstract_id(&raw, id);
    let nsid = format!("<w:nsid w:val=\"{}\"/>", hex8(env));
    let copy = replace_hex_element(&copy, "<w:nsid w:val=\"", &nsid);
    let copy = blank_tplc(&copy);
    insert_abstract(st, &copy);
    Some(id)
}

/// `raw.replace(/<w:abstractNum\b[^>]*>/, to)`: the first `<w:abstractNum` tag (a word boundary
/// after the name) up to its first `>` (D2b-F8).
pub(crate) fn replace_abstract_open(raw: &str, to: &str) -> String {
    let mut at = 0;
    while let Some(s) = next_open(raw, "w:abstractNum", at) {
        let a = s + "<w:abstractNum".len();
        if let Some(gt) = raw.get(a..).and_then(|r| r.find('>')).map(|p| a + p) {
            return format!("{}{to}{}", raw.get(..s).unwrap_or(""), raw.get(gt + 1..).unwrap_or(""));
        }
        at = s + 1;
    }
    raw.to_owned()
}

/// `raw.replace(/<w:(?:styleLink|numStyleLink) w:val="[^"]*"\/>/g, '')` (D2b-F8).
pub(crate) fn strip_style_links(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    let mut from = 0;
    while let Some(p) = raw.get(from..).and_then(|r| r.find("<w:")) {
        let s = from + p;
        let rest = raw.get(s + 3..).unwrap_or("");
        let name = if rest.starts_with("styleLink") {
            "styleLink"
        } else if rest.starts_with("numStyleLink") {
            "numStyleLink"
        } else {
            from = s + 1;
            continue;
        };
        let v = s + 3 + name.len();
        let hit = raw.get(v..).filter(|r| r.starts_with(" w:val=\"")).and_then(|r| {
            let q = 8 + r.get(8..)?.find('"')?;
            r.get(q + 1..)?.starts_with("/>").then_some(v + q + 3)
        });
        match hit {
            Some(end) => {
                out.push_str(raw.get(at..s).unwrap_or(""));
                at = end;
                from = end;
            }
            None => from = s + 1,
        }
    }
    out.push_str(raw.get(at..).unwrap_or(""));
    out
}

/// `importAbstract(raw)` (numbering-store.ts:232): a `w:abstractNum` of another document (paste)
/// with a new id and `nsid`, without its list-style links. The TS order of the side effects:
/// `nextId`, `ensure` (the `w15` test), then `hex8()` (D2b-F8).
pub(crate) fn import_abstract(st: &mut VersionStore, env: &mut dyn Env, raw: &str) -> f64 {
    let id = next_id(st, true);
    let w15 = if has_w15(&ensure(st)) { " w15:restartNumberingAfterBreak=\"0\"" } else { "" };
    let open = format!("<w:abstractNum w:abstractNumId=\"{}\"{w15}>", id_str(id));
    let abs = replace_abstract_open(raw, &open);
    let nsid = format!("<w:nsid w:val=\"{}\"/>", hex8(env));
    let abs = replace_hex_element(&abs, "<w:nsid w:val=\"", &nsid);
    let abs = strip_style_links(&abs);
    insert_abstract(st, &abs);
    id
}

/// `replaceAbstract(absId, raw, next)`: the first occurrence of `raw` in the part.
fn replace_abstract(st: &mut VersionStore, raw: &str, next: &str) {
    if raw == next {
        return;
    }
    let Some(x) = st.xml().map(|x| x.to_string()) else { return };
    let Some(at) = x.find(raw) else { return };
    set(st, format!("{}{next}{}", &x[..at], &x[at + raw.len()..]));
}

/// `setLevel(absId, ilvl, lvl)`: replaces the `w:lvl` (or appends it before the closing tag).
pub(crate) fn set_level(st: &mut VersionStore, abs: f64, ilvl: f64, lvl: &str) {
    let Some(raw) = abstract_xml(st, abs) else { return };
    let next = match level_range(&raw, ilvl) {
        Some((s, e)) => format!("{}{lvl}{}", &raw[..s], &raw[e..]),
        None => match raw.strip_suffix("</w:abstractNum>") {
            Some(head) => format!("{head}{lvl}</w:abstractNum>"),
            None => raw.clone(),
        },
    };
    replace_abstract(st, &raw, &next);
}

/// `/<w:multiLevelType w:val="\w+"\/>/` replaced once.
pub(crate) fn replace_multi_level_type(raw: &str, to: &str) -> String {
    let prefix = "<w:multiLevelType w:val=\"";
    let mut at = 0;
    while let Some(p) = raw.get(at..).and_then(|r| r.find(prefix)) {
        let q = at + p;
        let vs = q + prefix.len();
        let w = raw.get(vs..).map_or(0, |r| r.bytes().take_while(|b| uo_jsnum::text::is_js_word(*b)).count());
        if w > 0 && raw.get(vs + w..).is_some_and(|r| r.starts_with("\"/>")) {
            return format!("{}<w:multiLevelType w:val=\"{to}\"/>{}", &raw[..q], &raw[vs + w + 3..]);
        }
        at = q + 1;
    }
    raw.to_owned()
}

/// `raw.replace(/<w:lvl\b[\s\S]*<\/w:lvl>/, () => levels.join(''))`: from the first level to the
/// last closing tag.
pub(crate) fn replace_levels(raw: &str, levels: &[String]) -> String {
    if let Some(s) = next_open(raw, "w:lvl", 0)
        && let Some(c) = raw.rfind("</w:lvl>")
        && c >= s + "<w:lvl".len()
    {
        return format!("{}{}{}", &raw[..s], levels.concat(), &raw[c + "</w:lvl>".len()..]);
    }
    raw.to_owned()
}

/// `setLevels(absId, levels, { multiLevelType, tmpl })`: every level replaced.
pub(crate) fn set_levels(st: &mut VersionStore, abs: f64, levels: &[String], multi_level_type: Option<&str>, tmpl: Option<&str>) {
    let Some(raw) = abstract_xml(st, abs) else { return };
    let mut next = replace_levels(&raw, levels);
    if let Some(m) = multi_level_type.filter(|m| !m.is_empty()) {
        next = replace_multi_level_type(&next, m);
    }
    if let Some(t) = tmpl.filter(|t| !t.is_empty()) {
        next = replace_hex_element(&next, "<w:tmpl w:val=\"", &format!("<w:tmpl w:val=\"{t}\"/>"));
    }
    replace_abstract(st, &raw, &next);
}

/// `raw.replace(/<w:lvl\b([^>]*)>/g, …)`: the levels not in `used` are `w:tentative="1"`.
pub(crate) fn tentative_levels(raw: &str, u: &[f64]) -> String {
    let mut next = String::with_capacity(raw.len() + 64);
    let mut at = 0;
    while let Some(s) = next_open(raw, "w:lvl", at) {
        let a = s + "<w:lvl".len();
        let Some(gt) = raw.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
        let attrs = &raw[a..gt];
        let ilvl = ilvl_of(attrs);
        let clean = match attrs.find(" w:tentative=\"1\"") {
            Some(p) => format!("{}{}", &attrs[..p], &attrs[p + " w:tentative=\"1\"".len()..]),
            None => attrs.to_owned(),
        };
        next.push_str(&raw[at..s]);
        if u.contains(&ilvl) {
            next.push_str(&format!("<w:lvl{clean}>"));
        } else {
            next.push_str(&format!("<w:lvl{clean} w:tentative=\"1\">"));
        }
        at = gt + 1;
    }
    next.push_str(raw.get(at..).unwrap_or(""));
    next
}

/// `withTentative(touched, used)`: the text to write; levels without a paragraph are
/// `w:tentative="1"` in the hybrid lists the editor made or changed.
pub(crate) fn with_tentative(st: &VersionStore, touched: &[f64], used: &[(f64, Vec<f64>)]) -> Option<String> {
    let x0 = st.xml()?;
    if touched.is_empty() {
        return Some(x0.to_string());
    }
    let mut x = x0.to_string();
    for &abs in touched {
        let Some(raw) = abstract_in(&x, abs).map(str::to_owned) else { continue };
        if !raw.contains("hybridMultilevel") {
            continue;
        }
        let u: &[f64] = used.iter().find(|(a, _)| *a == abs).map_or(&[], |(_, v)| v);
        let next = tentative_levels(&raw, u);
        if next != raw
            && let Some(p) = x.find(&raw)
        {
            x = format!("{}{next}{}", &x[..p], &x[p + raw.len()..]);
        }
    }
    Some(x)
}

/// `Number(/w:ilvl="(\d+)"/.exec(attrs)?.[1] ?? 0)` (no `\b`: the first occurrence).
pub(crate) fn ilvl_of(attrs: &str) -> f64 {
    let needle = "w:ilvl=\"";
    let mut at = 0;
    while let Some(p) = attrs.get(at..).and_then(|r| r.find(needle)) {
        let q = at + p;
        let vs = q + needle.len();
        let digits = attrs.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if digits > 0 && attrs.as_bytes().get(vs + digits) == Some(&b'"') {
            return digits_to_f64(&attrs[vs..vs + digits]);
        }
        at = q + 1;
    }
    0.0
}

/// The `w:ind` element of a raw level: `/<w:ind\b[^>]*\/>/.exec(raw)?.[0]`.
pub(crate) fn ind_element(raw: &str) -> Option<&str> {
    let mut at = 0;
    while let Some(s) = next_open(raw, "w:ind", at) {
        let a = s + "<w:ind".len();
        let gt = a + raw.get(a..)?.find('>')?;
        // `[^>]*\/>`: the tag ends with `/>`
        if gt > a && raw.as_bytes().get(gt - 1) == Some(&b'/') {
            return raw.get(s..=gt);
        }
        at = s + 1;
    }
    None
}

/// `lvl.replace(/<w:ind\b[^>]*\/>/, () => ind)`.
pub(crate) fn replace_ind(lvl: &str, ind: &str) -> String {
    match ind_element(lvl) {
        Some(cur) => {
            let p = lvl.find(cur).unwrap_or(0);
            format!("{}{ind}{}", &lvl[..p], &lvl[p + cur.len()..])
        }
        None => lvl.to_owned(),
    }
}

/// `lvl.replace(/w:ilvl="\d+"/, 'w:ilvl="N"')` (the first one).
pub(crate) fn replace_ilvl(lvl: &str, ilvl: f64) -> String {
    let needle = "w:ilvl=\"";
    let mut at = 0;
    while let Some(p) = lvl.get(at..).and_then(|r| r.find(needle)) {
        let q = at + p;
        let vs = q + needle.len();
        let digits = lvl.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if digits > 0 && lvl.as_bytes().get(vs + digits) == Some(&b'"') {
            return format!("{}w:ilvl=\"{}\"{}", &lvl[..q], id_str(ilvl), &lvl[vs + digits + 1..]);
        }
        at = q + 1;
    }
    lvl.to_owned()
}

/// `sameLevelFormat(a, b)`.
pub(crate) fn same_level_format(a: Option<&NumLevel>, b: Option<&NumLevel>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    let fmt = |l: &NumLevel| l.num_fmt.as_deref().map_or("decimal".to_owned(), str::to_owned);
    let txt = |l: &NumLevel| l.lvl_text.as_deref().map_or(String::new(), str::to_owned);
    let font = |l: &NumLevel| {
        l.r_pr.as_deref().and_then(|r| r.r_fonts.as_deref()).and_then(|f| f.ascii.as_deref()).map_or(String::new(), str::to_owned)
    };
    fmt(a) == fmt(b) && txt(a) == txt(b) && font(a) == font(b) && a.lvl_pic_bullet_id == b.lvl_pic_bullet_id
}

/// `levelIndent(l)`: `{ left, hanging }` in twips.
pub(crate) fn level_indent(l: Option<&NumLevel>) -> (f64, f64) {
    let ind = l.and_then(|l| l.p_pr.as_deref()).and_then(|p| p.ind.as_deref());
    let left = ind.and_then(|i| i.left).map_or(0.0, JsNumber::to_number);
    let hanging = match ind.and_then(|i| i.hanging) {
        Some(h) => h.to_number(),
        None => ind.and_then(|i| i.first_line).map_or(0.0, |f| -f.to_number()),
    };
    (left, hanging)
}

/// The numbering as the engine reads it plus the resolver of `numStyleLink` (`abstractOf`,
/// `level`).
pub(crate) struct NumView<'a> {
    pub parsed: &'a Numbering,
    /// `styleNum(styleId)`: the numId of a numbering style
    pub style_num: &'a dyn Fn(&str) -> Option<f64>,
}

impl NumView<'_> {
    /// `abstractOf(numId)`: follows `w:numStyleLink` to the abstract of the list style.
    pub(crate) fn abstract_of(&self, num_id: f64) -> Option<f64> {
        let p = self.parsed;
        let mut n = p.nums.get(&id_str(num_id))?;
        for _ in 0..4 {
            let abs = n.abstract_num_id.to_number();
            let link = p.abstract_nums.get(&id_str(abs)).and_then(|a| a.num_style_link.as_deref());
            let to = link.filter(|l| !l.is_empty()).and_then(self.style_num);
            let next = to.filter(|t| n.num_id.as_f64() != Some(*t)).and_then(|t| p.nums.get(&id_str(t)));
            match next {
                None => return Some(abs),
                Some(m) => n = m,
            }
        }
        Some(n.abstract_num_id.to_number())
    }

    /// `level(numId, ilvl)`: the effective level (the `w:lvlOverride` over the abstract).
    pub(crate) fn level(&self, num_id: f64, ilvl: f64) -> Option<&NumLevel> {
        let n = self.parsed.nums.get(&id_str(num_id))?;
        if let Some(l) = n.overrides.get(&id_str(ilvl)).and_then(|o| o.lvl.as_deref()) {
            return Some(l);
        }
        let abs = self.abstract_of(num_id)?;
        self.parsed.abstract_nums.get(&id_str(abs))?.levels.get(&id_str(ilvl))
    }
}
