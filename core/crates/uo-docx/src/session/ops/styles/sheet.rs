//! docx-core `style-sheet.ts`: the style sheet (`word/styles.xml`) edited without loss. Each
//! operation replaces only the `w:style` elements involved (and the `lsdException` of a latent
//! style when the gallery changes), in the `CT_Style` order; the rest of the part is written byte
//! for byte. Rules measured in Word 365 pt-BR (record `word-styles-oracle-facts`).
//!
//! The regexes of the TS are hand matchers; the DOM is `uo-opc`'s, with positions.

use std::collections::HashSet;

use uo_jsnum::format::to_js_string;
use uo_jsnum::text::{is_js_space, is_js_word};
use uo_opc::Limits;
use uo_opc::xml::raw::{Kid, join_kids, set_kid, split_element, with_attrs};

use crate::data::word_styles::WORD_BUILTIN_STYLES;
use crate::read::DOCX_POS;

/// `esc`: `& < > "`.
pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

/// `STYLE_ORDER` (CT_Style).
const STYLE_ORDER: &[&str] = &[
    "w:name",
    "w:aliases",
    "w:basedOn",
    "w:next",
    "w:link",
    "w:autoRedefine",
    "w:hidden",
    "w:uiPriority",
    "w:semiHidden",
    "w:unhideWhenUsed",
    "w:qFormat",
    "w:locked",
    "w:personal",
    "w:personalCompose",
    "w:personalReply",
    "w:rsid",
    "w:pPr",
    "w:rPr",
    "w:tblPr",
    "w:trPr",
    "w:tcPr",
    "w:tblStylePr",
];

/// The CT_RPr order of the style sheet (the same list as `lossless.ts`).
const RPR_ORDER: &[&str] = &[
    "w:rStyle",
    "w:rFonts",
    "w:b",
    "w:bCs",
    "w:i",
    "w:iCs",
    "w:caps",
    "w:smallCaps",
    "w:strike",
    "w:dstrike",
    "w:outline",
    "w:shadow",
    "w:emboss",
    "w:imprint",
    "w:noProof",
    "w:snapToGrid",
    "w:vanish",
    "w:webHidden",
    "w:color",
    "w:spacing",
    "w:w",
    "w:kern",
    "w:position",
    "w:sz",
    "w:szCs",
    "w:highlight",
    "w:u",
    "w:effect",
    "w:bdr",
    "w:shd",
    "w:fitText",
    "w:vertAlign",
    "w:rtl",
    "w:cs",
    "w:em",
    "w:lang",
    "w:eastAsianLayout",
    "w:specVanish",
    "w:oMath",
    "w:rPrChange",
];

/// The CT_PPr order of the style sheet.
const PPR_ORDER: &[&str] = &[
    "w:pStyle",
    "w:keepNext",
    "w:keepLines",
    "w:pageBreakBefore",
    "w:framePr",
    "w:widowControl",
    "w:numPr",
    "w:suppressLineNumbers",
    "w:pBdr",
    "w:shd",
    "w:tabs",
    "w:suppressAutoHyphens",
    "w:kinsoku",
    "w:wordWrap",
    "w:overflowPunct",
    "w:topLinePunct",
    "w:autoSpaceDE",
    "w:autoSpaceDN",
    "w:bidi",
    "w:adjustRightInd",
    "w:snapToGrid",
    "w:spacing",
    "w:ind",
    "w:contextualSpacing",
    "w:mirrorIndents",
    "w:suppressOverlap",
    "w:jc",
    "w:textDirection",
    "w:textAlignment",
    "w:textboxTightWrap",
    "w:outlineLvl",
    "w:divId",
    "w:cnfStyle",
    "w:rPr",
    "w:sectPr",
    "w:pPrChange",
];

/// One `w:style` as the interface needs it (`StyleMeta` and its position).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StyleMeta {
    pub id: String,
    /// paragraph | character | table | numbering
    pub ty: String,
    pub name: String,
    pub aliases: Option<String>,
    pub custom: bool,
    pub is_default: bool,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub link: Option<String>,
    pub auto_redefine: bool,
    pub hidden: bool,
    pub ui_priority: f64,
    pub semi_hidden: bool,
    pub unhide_when_used: bool,
    pub q_format: bool,
    pub start: usize,
    pub end: usize,
}

/// An `lsdException` (`LatentMeta`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LatentMeta {
    pub name: String,
    pub ui_priority: f64,
    pub semi_hidden: bool,
    pub unhide_when_used: bool,
    pub q_format: bool,
}

/// `readStyleIndex(xml)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct StyleIndex {
    pub styles: Vec<StyleMeta>,
    pub latent: Vec<LatentMeta>,
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true" | "on"))
}

/// `parseInt(v, 10) || 0`.
fn int_or0(v: &str) -> f64 {
    let n = uo_jsnum::parse::parse_int(v, 10);
    if n.is_nan() { 0.0 } else { n }
}

/// `readStyleIndex(xml)`; a part that does not parse gives the empty index (the TS throws; no
/// session document has such a part, the reader refused it first).
pub(crate) fn read_style_index(xml: &str) -> StyleIndex {
    let Ok(doc) = uo_opc::xml::parse(xml, &Limits::DEFAULT.bounded(&DOCX_POS)) else { return StyleIndex::default() };
    let root = doc.root();
    fn val<'a>(el: uo_opc::xml::Element<'a>, n: &str) -> Option<&'a str> {
        el.first_child(n).and_then(|k| k.attr("w:val"))
    }
    fn on_off(el: uo_opc::xml::Element<'_>, n: &str) -> bool {
        match el.first_child(n) {
            None => false,
            Some(k) => !matches!(k.attr("w:val"), Some("0" | "false" | "off")),
        }
    }
    // `w:styleId` repeated: the LAST definition is the one that counts (measured in Word)
    let all: Vec<_> = root.children_named("w:style").collect();
    let mut last: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, st) in all.iter().enumerate() {
        if let Some(id) = st.attr("w:styleId").filter(|s| !s.is_empty()) {
            last.insert(id, i);
        }
    }
    let mut styles = Vec::new();
    for (i, st) in all.iter().enumerate() {
        let Some(id) = st.attr("w:styleId").filter(|s| !s.is_empty()) else { continue };
        if last.get(id) != Some(&i) {
            continue;
        }
        let Some(p) = st.positions() else { continue };
        let prio = val(*st, "w:uiPriority");
        styles.push(StyleMeta {
            id: id.to_owned(),
            ty: st.attr("w:type").unwrap_or("paragraph").to_owned(),
            name: val(*st, "w:name").map_or_else(|| id.to_owned(), str::to_owned),
            aliases: val(*st, "w:aliases").map(str::to_owned),
            custom: truthy(st.attr("w:customStyle")),
            is_default: truthy(st.attr("w:default")),
            based_on: val(*st, "w:basedOn").map(str::to_owned),
            next: val(*st, "w:next").map(str::to_owned),
            link: val(*st, "w:link").map(str::to_owned),
            auto_redefine: on_off(*st, "w:autoRedefine"),
            hidden: on_off(*st, "w:hidden"),
            ui_priority: prio.map_or(0.0, int_or0),
            semi_hidden: on_off(*st, "w:semiHidden"),
            unhide_when_used: on_off(*st, "w:unhideWhenUsed"),
            q_format: on_off(*st, "w:qFormat"),
            start: p.start,
            end: p.end,
        });
    }
    let ls = root.first_child("w:latentStyles");
    let a = |k: &str| ls.and_then(|l| l.attr(k));
    let def_prio = a("w:defUIPriority").map_or(99.0, int_or0);
    let (def_semi, def_unhide, def_q) = (truthy(a("w:defSemiHidden")), truthy(a("w:defUnhideWhenUsed")), truthy(a("w:defQFormat")));
    let mut latent = Vec::new();
    if let Some(ls) = ls {
        for e in ls.children_named("w:lsdException") {
            let Some(n) = e.attr("w:name").filter(|s| !s.is_empty()) else { continue };
            let g = |k: &str, d: bool| e.attr(k).map_or(d, |v| truthy(Some(v)));
            latent.push(LatentMeta {
                name: n.to_owned(),
                ui_priority: e.attr("w:uiPriority").map_or(def_prio, int_or0),
                semi_hidden: g("w:semiHidden", def_semi),
                unhide_when_used: g("w:unhideWhenUsed", def_unhide),
                q_format: g("w:qFormat", def_q),
            });
        }
    }
    StyleIndex { styles, latent }
}

/// `styleRaw(xml, id)`.
pub(crate) fn style_raw(xml: &str, id: &str) -> Option<String> {
    let idx = read_style_index(xml);
    let s = idx.styles.iter().find(|x| x.id == id)?;
    xml.get(s.start..s.end).map(str::to_owned)
}

/// `putStyle(xml, id, styleXml)`: replaces the `w:style` or appends it before `</w:styles>`.
/// `None` when the part has no `</w:styles>` (the TS throws).
pub(crate) fn put_style(xml: &str, id: &str, style_xml: &str) -> Option<String> {
    let idx = read_style_index(xml);
    if let Some(s) = idx.styles.iter().find(|x| x.id == id) {
        return Some(format!("{}{style_xml}{}", xml.get(..s.start)?, xml.get(s.end..)?));
    }
    let at = xml.rfind("</w:styles>")?;
    Some(format!("{}{style_xml}{}", &xml[..at], &xml[at..]))
}

/// `dropStyle(xml, id)`: every definition of the id.
pub(crate) fn drop_style(xml: &str, id: &str) -> String {
    let Ok(doc) = uo_opc::xml::parse(xml, &Limits::DEFAULT.bounded(&DOCX_POS)) else { return xml.to_owned() };
    let mut spans: Vec<(usize, usize)> = doc
        .root()
        .children_named("w:style")
        .filter(|st| st.attr("w:styleId") == Some(id))
        .filter_map(|st| st.positions().map(|p| (p.start, p.end)))
        .collect();
    spans.reverse();
    let mut out = xml.to_owned();
    for (s, e) in spans {
        out = format!("{}{}", &out[..s], &out[e..]);
    }
    out
}

/// `setLatentQFormat(xml, name, on)`: the `lsdException` of the latent style `name`.
pub(crate) fn set_latent_q_format(xml: &str, name: &str, on: bool) -> String {
    // the name goes into the pattern with `"` as `&quot;` and nothing else changed
    let needle = format!("w:name=\"{}\"", name.replace('"', "&quot;"));
    let mut at = 0;
    while let Some(s) = super::super::lists::rx::next_open(xml, "w:lsdException", at) {
        let a = s + "<w:lsdException".len();
        let Some(gt) = xml.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
        let attrs = xml.get(a..gt).unwrap_or("");
        let has_name = attrs.match_indices(&needle).any(|(q, _)| q > 0 && attrs.as_bytes().get(q - 1).is_some_and(|b| !is_js_word(*b)));
        // `[^>]*\/>`: a self-closing tag
        if has_name && gt > a && xml.as_bytes().get(gt - 1) == Some(&b'/') {
            let tag = &xml[s..=gt];
            let mut out = remove_q_format(tag);
            if on {
                let body = out.strip_suffix("/>").unwrap_or(&out);
                out = format!("{} w:qFormat=\"1\"/>", uo_jsnum::text::trim_end(body));
            }
            return if out == tag { xml.to_owned() } else { format!("{}{out}{}", &xml[..s], &xml[gt + 1..]) };
        }
        at = s + 1;
    }
    xml.to_owned()
}

/// `tag.replace(/\s+w:qFormat="[^"]*"/, '')`.
pub(crate) fn remove_q_format(tag: &str) -> String {
    let needle = "w:qFormat=\"";
    let mut from = 0;
    while let Some(p) = tag.get(from..).and_then(|r| r.find(needle)) {
        let q = from + p;
        let ws = tag.get(..q).map_or(0, |h| h.chars().rev().take_while(|c| is_js_space(*c)).map(char::len_utf8).sum::<usize>());
        if ws > 0 {
            let vs = q + needle.len();
            if let Some(close) = tag.get(vs..).and_then(|r| r.find('"')) {
                return format!("{}{}", &tag[..q - ws], &tag[vs + close + 1..]);
            }
        }
        from = q + 1;
    }
    tag.to_owned()
}

/// `styleIdFromName(name, taken)`: only `[A-Za-z0-9-]`; a collision takes a suffix 0, 1, 2…
pub(crate) fn style_id_from_name(name: &str, taken: &HashSet<String>) -> String {
    let mut base: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if base.is_empty() {
        base = "Estilo".to_owned();
    }
    if !taken.contains(&base) {
        return base;
    }
    let mut i = 0u64;
    loop {
        let c = format!("{base}{i}");
        if !taken.contains(&c) {
            return c;
        }
        i += 1;
    }
}

/// `NewStyle`.
#[derive(Debug, Clone, Default)]
pub(crate) struct NewStyle {
    pub ty: &'static str,
    pub id: String,
    pub name: String,
    pub custom: bool,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub link: Option<String>,
    pub auto_redefine: bool,
    pub ui_priority: Option<f64>,
    pub semi_hidden: bool,
    pub unhide_when_used: bool,
    pub q_format: bool,
    pub p_pr: String,
    pub r_pr: String,
}

/// `newStyleXml(o)`: a `w:style` in the CT_Style order.
pub(crate) fn new_style_xml(o: &NewStyle) -> String {
    let mut s = format!(
        "<w:style w:type=\"{}\"{} w:styleId=\"{}\"><w:name w:val=\"{}\"/>",
        o.ty,
        if o.custom { " w:customStyle=\"1\"" } else { "" },
        esc(&o.id),
        esc(&o.name)
    );
    let mut val = |tag: &str, v: &Option<String>| {
        if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
            s.push_str(&format!("<{tag} w:val=\"{}\"/>", esc(v)));
        }
    };
    val("w:basedOn", &o.based_on);
    val("w:next", &o.next);
    val("w:link", &o.link);
    if o.auto_redefine {
        s.push_str("<w:autoRedefine/>");
    }
    if let Some(p) = o.ui_priority {
        s.push_str(&format!("<w:uiPriority w:val=\"{}\"/>", to_js_string(p)));
    }
    if o.semi_hidden {
        s.push_str("<w:semiHidden/>");
    }
    if o.unhide_when_used {
        s.push_str("<w:unhideWhenUsed/>");
    }
    if o.q_format {
        s.push_str("<w:qFormat/>");
    }
    s.push_str(&o.p_pr);
    s.push_str(&o.r_pr);
    s.push_str("</w:style>");
    s
}

/// A change of a value: `undefined` (keep), `null` (remove) or a value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Ch<T> {
    #[default]
    Keep,
    Null,
    Val(T),
}

impl<T> Ch<T> {
    pub(crate) fn is_keep(&self) -> bool {
        matches!(self, Ch::Keep)
    }
}

/// `StyleHead`: the changes to the head of a `w:style` (`undefined` = untouched, `null` = remove).
#[derive(Debug, Clone, Default)]
pub(crate) struct StyleHead {
    pub style_id: Option<String>,
    pub name: Option<String>,
    pub aliases: Ch<String>,
    pub based_on: Ch<String>,
    pub next: Ch<String>,
    pub link: Ch<String>,
    pub auto_redefine: Option<bool>,
    pub semi_hidden: Option<bool>,
    pub q_format: Option<bool>,
    /// `Val("")` and `Null` remove
    pub p_pr: Ch<String>,
    pub r_pr: Ch<String>,
}

/// `splitElement(raw)` with the docx preset; `None` when the raw text does not parse.
fn split(raw: &str) -> Option<(String, String, Vec<Kid>)> {
    let s = split_element(raw, &DOCX_POS, &Limits::DEFAULT).ok()?;
    Some((s.open, s.close, s.kids))
}

/// `patchStyleXml(raw, h)`: only the touched children change (the rest byte for byte).
pub(crate) fn patch_style_xml(raw: &str, h: &StyleHead) -> String {
    let Some((open, close, mut kids)) = split(raw) else { return raw.to_owned() };
    let mut head = open;
    if let Some(id) = &h.style_id {
        head = with_attrs(&head, "w:style", &[("w:styleId", Some(id))]).unwrap_or(head);
    }
    fn set(kids: &mut Vec<Kid>, n: &str, xml: Option<&str>) {
        set_kid(kids, STYLE_ORDER, n, xml);
    }
    fn v(kids: &mut Vec<Kid>, n: &str, x: &Ch<String>) {
        match x {
            Ch::Keep => {}
            Ch::Null => set(kids, n, None),
            Ch::Val(x) => set(kids, n, Some(&format!("<{n} w:val=\"{}\"/>", esc(x)))),
        }
    }
    if let Some(n) = &h.name {
        set(&mut kids, "w:name", Some(&format!("<w:name w:val=\"{}\"/>", esc(n))));
    }
    v(&mut kids, "w:aliases", &h.aliases);
    v(&mut kids, "w:basedOn", &h.based_on);
    v(&mut kids, "w:next", &h.next);
    v(&mut kids, "w:link", &h.link);
    for (n, on) in [("w:autoRedefine", h.auto_redefine), ("w:semiHidden", h.semi_hidden), ("w:qFormat", h.q_format)] {
        if let Some(on) = on {
            let flag = format!("<{n}/>");
            set(&mut kids, n, if on { Some(flag.as_str()) } else { None });
        }
    }
    for (n, x) in [("w:pPr", &h.p_pr), ("w:rPr", &h.r_pr)] {
        match x {
            Ch::Keep => {}
            Ch::Null => set(&mut kids, n, None),
            Ch::Val(x) if x.is_empty() => set(&mut kids, n, None),
            Ch::Val(x) => set(&mut kids, n, Some(x)),
        }
    }
    let mut out = head;
    for k in &kids {
        out.push_str(&k.raw);
    }
    out.push_str(&close);
    out
}

/// `styleKid(raw, name)`.
pub(crate) fn style_kid(raw: &str, name: &str) -> Option<String> {
    let (_, _, kids) = split(raw)?;
    kids.into_iter().find(|k| k.name == name).map(|k| k.raw)
}

// ───────────── the style formatting (the Modify Style dialog) ─────────────

/// `StyleFormat`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleFormat {
    pub font_ascii: Ch<String>,
    pub sz_half_pt: Ch<f64>,
    pub bold: Ch<bool>,
    pub italic: Ch<bool>,
    pub underline: Ch<bool>,
    pub strike: Ch<bool>,
    pub all_caps: Ch<bool>,
    pub small_caps: Ch<bool>,
    pub color: Ch<String>,
    pub char_spacing_twips: Ch<f64>,
    pub alignment: Ch<String>,
    pub space_before_twips: Ch<f64>,
    pub space_after_twips: Ch<f64>,
    pub line_twentieths: Ch<f64>,
    pub indent_left_twips: Ch<f64>,
    pub indent_right_twips: Ch<f64>,
    /// > 0 first line, < 0 hanging
    pub indent_first_line_twips: Ch<f64>,
}

impl StyleFormat {
    /// `hasRunChanges(f)`.
    pub(crate) fn has_run_changes(&self) -> bool {
        !(self.font_ascii.is_keep()
            && self.sz_half_pt.is_keep()
            && self.bold.is_keep()
            && self.italic.is_keep()
            && self.underline.is_keep()
            && self.strike.is_keep()
            && self.all_caps.is_keep()
            && self.small_caps.is_keep()
            && self.color.is_keep()
            && self.char_spacing_twips.is_keep())
    }

    /// `hasParaChanges(f)`.
    pub(crate) fn has_para_changes(&self) -> bool {
        !(self.alignment.is_keep()
            && self.space_before_twips.is_keep()
            && self.space_after_twips.is_keep()
            && self.line_twentieths.is_keep()
            && self.indent_left_twips.is_keep()
            && self.indent_right_twips.is_keep()
            && self.indent_first_line_twips.is_keep())
    }
}

fn split_or_new(raw: Option<&str>, tag: &str) -> (String, String, Vec<Kid>) {
    raw.and_then(split).unwrap_or_else(|| (format!("<{tag}>"), format!("</{tag}>"), Vec::new()))
}

fn kid_raw<'k>(kids: &'k [Kid], n: &str) -> &'k str {
    kids.iter().find(|k| k.name == n).map_or("", |k| k.raw.as_str())
}

fn with_attr_changes(raw: &str, name: &str, changes: &[(&str, Option<String>)]) -> String {
    let ch: Vec<(&str, Option<&str>)> = changes.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    with_attrs(raw, name, &ch).unwrap_or_else(|| raw.to_owned())
}

/// `/\sw:/.test(x)`: a white space and `w:` somewhere.
fn has_ws_w(x: &str) -> bool {
    x.match_indices("w:").any(|(q, _)| x.get(..q).and_then(|h| h.chars().next_back()).is_some_and(is_js_space))
}

/// Finds `\s` + `attr="[^"]*"` (the first): `(start of the space, end)`.
fn find_ws_attr(x: &str, attr: &str) -> Option<(usize, usize)> {
    let needle = format!("{attr}=\"");
    let mut from = 0;
    while let Some(p) = x.get(from..).and_then(|r| r.find(&needle)) {
        let q = from + p;
        if let Some(c) = x.get(..q).and_then(|h| h.chars().next_back()).filter(|c| is_js_space(*c)) {
            let vs = q + needle.len();
            if let Some(close) = x.get(vs..).and_then(|r| r.find('"')) {
                return Some((q - c.len_utf8(), vs + close + 1));
            }
        }
        from = q + 1;
    }
    None
}

fn num(x: f64) -> String {
    to_js_string(x)
}

/// `patchStyleRPr(raw, f)`: the `w:rPr` with `f` applied, as Word writes on modify: the font
/// changes only ascii/hAnsi (their themes go; East Asian and complex stay), the size touches
/// `w:sz`, the colour loses its theme, the toggles only the Latin one.
pub(crate) fn patch_style_rpr(raw: Option<&str>, f: &StyleFormat) -> String {
    let (open, close, mut kids) = split_or_new(raw, "w:rPr");
    let set = |kids: &mut Vec<Kid>, n: &str, xml: Option<&str>| set_kid(kids, RPR_ORDER, n, xml);
    match &f.font_ascii {
        Ch::Keep => {}
        Ch::Null => {
            let cur = kid_raw(&kids, "w:rFonts").to_owned();
            let x =
                if cur.is_empty() { String::new() } else { with_attr_changes(&cur, "w:rFonts", &[("w:ascii", None), ("w:hAnsi", None)]) };
            let keep = has_ws_w(&x);
            set(&mut kids, "w:rFonts", if keep { Some(&x) } else { None });
        }
        Ch::Val(font) => {
            // Word swaps a theme attribute for the name IN PLACE
            let mut x = kid_raw(&kids, "w:rFonts").to_owned();
            for (theme, plain) in [("w:asciiTheme", "w:ascii"), ("w:hAnsiTheme", "w:hAnsi")] {
                if let Some((s, e)) = find_ws_attr(&x, theme)
                    && find_ws_attr(&x, plain).is_none()
                {
                    x = format!("{} {plain}=\"{}\"{}", &x[..s], esc(font), &x[e..]);
                }
            }
            let out = with_attr_changes(
                &x,
                "w:rFonts",
                &[("w:ascii", Some(font.clone())), ("w:hAnsi", Some(font.clone())), ("w:asciiTheme", None), ("w:hAnsiTheme", None)],
            );
            set(&mut kids, "w:rFonts", Some(&out));
        }
    }
    let tog = |kids: &mut Vec<Kid>, n: &str, v: &Ch<bool>| match v {
        Ch::Keep => {}
        Ch::Null => set(kids, n, None),
        Ch::Val(true) => set(kids, n, Some(&format!("<{n}/>"))),
        Ch::Val(false) => set(kids, n, Some(&format!("<{n} w:val=\"0\"/>"))),
    };
    tog(&mut kids, "w:b", &f.bold);
    tog(&mut kids, "w:i", &f.italic);
    tog(&mut kids, "w:caps", &f.all_caps);
    tog(&mut kids, "w:smallCaps", &f.small_caps);
    tog(&mut kids, "w:strike", &f.strike);
    match &f.color {
        Ch::Keep => {}
        Ch::Null => set(&mut kids, "w:color", None),
        Ch::Val(c) => set(&mut kids, "w:color", Some(&format!("<w:color w:val=\"{}\"/>", esc(c)))),
    }
    match f.char_spacing_twips {
        Ch::Keep => {}
        Ch::Null => set(&mut kids, "w:spacing", None),
        Ch::Val(v) => set(&mut kids, "w:spacing", Some(&format!("<w:spacing w:val=\"{}\"/>", num(v)))),
    }
    match f.sz_half_pt {
        Ch::Keep => {}
        Ch::Null => set(&mut kids, "w:sz", None),
        Ch::Val(v) => set(&mut kids, "w:sz", Some(&format!("<w:sz w:val=\"{}\"/>", num(v)))),
    }
    match f.underline {
        Ch::Keep => {}
        Ch::Null => set(&mut kids, "w:u", None),
        Ch::Val(u) => set(&mut kids, "w:u", Some(&format!("<w:u w:val=\"{}\"/>", if u { "single" } else { "none" }))),
    }
    join_kids(&open, &kids, &close)
}

/// `patchStylePPr(raw, f)`: the `w:pPr` with `f` applied (spacing and indent merge attributes).
pub(crate) fn patch_style_ppr(raw: Option<&str>, f: &StyleFormat) -> String {
    let (open, close, mut kids) = split_or_new(raw, "w:pPr");
    match &f.alignment {
        Ch::Keep => {}
        Ch::Null => set_kid(&mut kids, PPR_ORDER, "w:jc", None),
        Ch::Val(a) => set_kid(&mut kids, PPR_ORDER, "w:jc", Some(&format!("<w:jc w:val=\"{a}\"/>"))),
    }
    let n = |v: &Ch<f64>| -> Option<String> {
        match v {
            Ch::Val(x) => Some(num(*x)),
            _ => None,
        }
    };
    if !f.space_before_twips.is_keep() || !f.space_after_twips.is_keep() || !f.line_twentieths.is_keep() {
        let mut ch: Vec<(&str, Option<String>)> = Vec::new();
        if !f.space_before_twips.is_keep() {
            ch.push(("w:before", n(&f.space_before_twips)));
        }
        if !f.space_after_twips.is_keep() {
            ch.push(("w:after", n(&f.space_after_twips)));
        }
        if !f.line_twentieths.is_keep() {
            ch.push(("w:line", n(&f.line_twentieths)));
            ch.push(("w:lineRule", if matches!(f.line_twentieths, Ch::Null) { None } else { Some("auto".to_owned()) }));
        }
        let x = with_attr_changes(kid_raw(&kids, "w:spacing"), "w:spacing", &ch);
        set_kid(&mut kids, PPR_ORDER, "w:spacing", if has_ws_w(&x) { Some(&x) } else { None });
    }
    if !f.indent_left_twips.is_keep() || !f.indent_right_twips.is_keep() || !f.indent_first_line_twips.is_keep() {
        let mut ch: Vec<(&str, Option<String>)> = Vec::new();
        if !f.indent_left_twips.is_keep() {
            ch.push(("w:left", n(&f.indent_left_twips)));
        }
        if !f.indent_right_twips.is_keep() {
            ch.push(("w:right", n(&f.indent_right_twips)));
        }
        if !f.indent_first_line_twips.is_keep() {
            let v = match &f.indent_first_line_twips {
                Ch::Val(v) => Some(*v),
                _ => None,
            };
            ch.push(("w:firstLine", v.filter(|v| *v > 0.0).map(num)));
            ch.push(("w:hanging", v.filter(|v| *v < 0.0).map(|v| num(-v))));
        }
        let x = with_attr_changes(kid_raw(&kids, "w:ind"), "w:ind", &ch);
        set_kid(&mut kids, PPR_ORDER, "w:ind", if has_ws_w(&x) { Some(&x) } else { None });
    }
    join_kids(&open, &kids, &close)
}

// ───────────────────────── Word's built-ins ─────────────────────────

/// `BuiltinStyle`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltinStyle {
    pub name: &'static str,
    pub pt: &'static str,
    pub xml: &'static str,
    pub twin: Option<&'static str>,
}

/// `builtins()`: the table, in order. The TS builds a `Map` by lower-case name (a repeated name would
/// keep its first place with the last value); the 121 names of the table are distinct (checked by
/// `tests/data_tables.rs`-style test `builtin_names_are_distinct`), so the table is walked as it
/// is, without a process-wide cache.
pub(crate) fn builtins() -> impl Iterator<Item = BuiltinStyle> + Clone {
    WORD_BUILTIN_STYLES.iter().map(|(name, pt, xml, twin)| BuiltinStyle { name, pt, xml, twin: *twin })
}

/// `builtinStyle(name)`.
pub(crate) fn builtin_style(name: &str) -> Option<BuiltinStyle> {
    let k = uo_jsnum::text::to_lowercase(name);
    builtins().find(|b| uo_jsnum::text::to_lowercase(b.name) == k)
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_names_are_distinct() {
        let mut names: Vec<String> = super::builtins().map(|b| uo_jsnum::text::to_lowercase(b.name)).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }
}

/// The interface language of the lists and styles (`UiLang`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLang {
    #[default]
    En,
    Pt,
    Es,
}

const EXTRA_PT: &[(&str, &str)] = &[
    ("default paragraph font", "Fonte parág. padrão"),
    ("normal table", "Tabela normal"),
    ("no list", "Sem lista"),
    ("table grid", "Tabela com grade"),
];

fn extra_pt(k: &str) -> Option<&'static str> {
    EXTRA_PT.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
}

/// `EN_WORDS[w.toLowerCase()]`.
fn en_word(w: &str) -> Option<&'static str> {
    match w {
        "toc" => Some("TOC"),
        "toa" => Some("TOA"),
        "html" => Some("HTML"),
        "of" => Some("of"),
        "annotation" => Some("Comment"),
        _ => None,
    }
}

/// `builtinDisplayName(name, lang)`: the display name of a built-in by its canonical `w:name`
/// (`None` = not a known built-in; D2b-F2: keys of `Object.prototype` are not names).
pub(crate) fn builtin_display_name(name: &str, lang: UiLang) -> Option<String> {
    let k = uo_jsnum::text::to_lowercase(name);
    let b = builtin_style(name);
    match lang {
        UiLang::Pt => b.map(|b| b.pt.to_owned()).or_else(|| extra_pt(&k).map(str::to_owned)),
        UiLang::Es => {
            if b.is_some() || extra_pt(&k).is_some() {
                crate::read::style_names_es::builtin_style_name_es(&k).map(str::to_owned).or_else(|| builtin_display_name(name, UiLang::En))
            } else {
                None
            }
        }
        UiLang::En => {
            if b.is_none() && extra_pt(&k).is_none() {
                return None;
            }
            match k.as_str() {
                "macro" => return Some("Macro Text".to_owned()),
                "annotation text" => return Some("Comment Text".to_owned()),
                "annotation reference" => return Some("Comment Reference".to_owned()),
                "annotation subject" => return Some("Comment Subject".to_owned()),
                _ => {}
            }
            let words: Vec<String> = name
                .split(' ')
                .enumerate()
                .map(|(i, w)| {
                    let lw = uo_jsnum::text::to_lowercase(w);
                    match en_word(&lw) {
                        Some(r) if i > 0 || lw != "of" => r.to_owned(),
                        _ => {
                            let mut cs = w.chars();
                            cs.next()
                                .map_or_else(String::new, |c| format!("{}{}", uo_text::case::to_uppercase_str(&c.to_string()), cs.as_str()))
                        }
                    }
                })
                .collect();
            Some(words.join(" "))
        }
    }
}

/// `builtinXml(xml, { id, used, refs })`: the built-in `w:style` with the id the file uses.
pub(crate) fn builtin_xml(xml: &str, id: Option<&str>, used: bool, refs: &[(String, String)]) -> String {
    let mut out = xml.to_owned();
    if let Some(id) = id.filter(|i| !i.is_empty()) {
        let needle = "w:styleId=\"";
        if let Some(p) = out.find(needle) {
            let vs = p + needle.len();
            if let Some(close) = out[vs..].find('"') {
                out = format!("{}{needle}{}\"{}", &out[..p], esc(id), &out[vs + close + 1..]);
            }
        }
    }
    // basedOn/next/link to the ids the document uses (English × pt-BR)
    for (from, to) in refs {
        for tag in ["w:basedOn", "w:next", "w:link"] {
            let pat = format!("<{tag} w:val=\"{from}\"");
            if out.contains(&pat) {
                out = out.replace(&pat, &format!("<{tag} w:val=\"{}\"", esc(to)));
            }
        }
    }
    if used && out.contains("<w:unhideWhenUsed/>") {
        out = out.replacen("<w:semiHidden/>", "", 1);
    }
    out
}
