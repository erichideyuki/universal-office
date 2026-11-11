//! `paste-html.ts` (1:1, D2b-F8) over the paste tree ([`super::tree`]): the HTML of the clipboard
//! (browsers, Google Docs, Word itself) → paragraphs with the formatting of each piece. The TS
//! recursion `walk → walkChildren → walk` (with its post-actions: `lists.pop()`, `close()`, the
//! `cur = p` of a table row) is an explicit frame stack here: no recursion on hostile depth.

use std::collections::HashMap;
use std::sync::Arc;

use super::image::atob;
use super::rx;
use super::tree::{self, NodeKind, PasteHtml, PasteTree};
use crate::js::Str;
use crate::model::run::RunProps;
use crate::session::clipboard::ClipHtmlList;
use crate::text::Utf16;
use crate::word::obj;

/// `{ data, ext, wPx?, hPx? }` of an `<img>` with a `data:` URL.
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlImage {
    pub data: Arc<[u8]>,
    pub ext: Str,
    pub w_px: Option<f64>,
    pub h_px: Option<f64>,
}

/// `HtmlRun`.
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlRun {
    pub text: Utf16,
    /// `Partial<RunProps>` in the TS assignment order (U6: it reaches the saved runs)
    pub props: Arc<RunProps>,
    pub block_bold: bool,
    pub image: Option<HtmlImage>,
}

/// `HtmlPara.style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleStyle {
    Title,
    Subtitle,
}

/// The block fields of `HtmlPara` (`Omit<HtmlPara, 'runs'>`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HtmlBlock {
    pub heading: Option<f64>,
    pub style: Option<TitleStyle>,
    pub list: Option<ClipHtmlList>,
    /// left | center | right | both
    pub align: Option<&'static str>,
    pub pre: bool,
    pub web: bool,
}

/// `HtmlPara`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HtmlPara {
    pub runs: Vec<HtmlRun>,
    pub block: HtmlBlock,
}

const BLOCK: [&str; 32] = [
    "P",
    "DIV",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    "LI",
    "PRE",
    "BLOCKQUOTE",
    "TR",
    "UL",
    "OL",
    "TABLE",
    "THEAD",
    "TBODY",
    "SECTION",
    "ARTICLE",
    "HEADER",
    "FOOTER",
    "MAIN",
    "ASIDE",
    "NAV",
    "DD",
    "DT",
    "DL",
    "FIGURE",
    "FIGCAPTION",
    "ADDRESS",
    "HR",
    "CENTER",
];
const SKIP: [&str; 21] = [
    "SCRIPT", "STYLE", "HEAD", "TITLE", "META", "LINK", "TEMPLATE", "NOSCRIPT", "svg", "SVG", "IFRAME", "OBJECT", "EMBED", "VIDEO",
    "AUDIO", "CANVAS", "SELECT", "TEXTAREA", "INPUT", "BUTTON", "DATALIST",
];

const NAMED: [(&str, &str); 18] = [
    ("black", "000000"),
    ("white", "FFFFFF"),
    ("red", "FF0000"),
    ("green", "008000"),
    ("blue", "0000FF"),
    ("yellow", "FFFF00"),
    ("gray", "808080"),
    ("grey", "808080"),
    ("silver", "C0C0C0"),
    ("maroon", "800000"),
    ("navy", "000080"),
    ("purple", "800080"),
    ("teal", "008080"),
    ("olive", "808000"),
    ("orange", "FFA500"),
    ("lime", "00FF00"),
    ("aqua", "00FFFF"),
    ("fuchsia", "FF00FF"),
];

/// The keys every plain object literal inherits (`Object.prototype`): `kw[s]` of `cssSizeHalfPt`
/// finds them (a NaN result).
const PROTO: [&str; 12] = [
    "constructor",
    "__proto__",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "toLocaleString",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

fn eq(u: &[u16], s: &str) -> bool {
    u.len() == s.len() && u.iter().zip(s.bytes()).all(|(a, b)| *a == u16::from(b))
}

fn in_set(u: &[u16], set: &[&str]) -> bool {
    set.iter().any(|s| !s.is_empty() && eq(u, s))
}

fn lower(s: &str) -> Vec<u16> {
    uo_jsnum::text::to_lowercase(s).encode_utf16().collect()
}

fn js_trim(u: &[u16]) -> &[u16] {
    let a = u.iter().position(|c| !uo_jsnum::text::is_js_space_unit(*c)).unwrap_or(u.len());
    let b = u.iter().rposition(|c| !uo_jsnum::text::is_js_space_unit(*c)).map_or(a, |k| k + 1);
    u.get(a..b.max(a)).unwrap_or(&[])
}

fn to_num(u: &[u16]) -> f64 {
    uo_jsnum::parse::to_number(&String::from_utf16_lossy(u))
}

/// `cssColor(v)` → `RRGGBB` (`None`: transparent/inherit/unknown). The names of `Object.prototype`
/// (`NAMED['constructor']` is a function in the TS) are not colours here (F8-D10).
#[must_use]
pub fn css_color(v: &str) -> Option<String> {
    let s = lower(uo_jsnum::text::trim(v));
    if s.is_empty() || ["inherit", "initial", "transparent", "currentcolor"].iter().any(|k| eq(&s, k)) {
        return None;
    }
    if let Some(h) = rx::hex3(&s) {
        let mut out = String::new();
        for c in h {
            let ch = char::from_u32(u32::from(*c)).unwrap_or('0').to_ascii_uppercase();
            out.push(ch);
            out.push(ch);
        }
        return Some(out);
    }
    if let Some(h) = rx::hex6(&s) {
        return Some(String::from_utf16_lossy(h).to_ascii_uppercase());
    }
    if let Some((nums, alpha)) = rx::rgba(&s) {
        if let Some(a) = alpha
            && uo_jsnum::parse::parse_float(&String::from_utf16_lossy(&a)) == 0.0
        {
            return None;
        }
        let mut out = String::new();
        for n in &nums {
            let x = uo_jsnum::math::min(255.0, to_num(n));
            // `x.toString(16).padStart(2, '0')` of an integer 0..=255
            let hex = format!("{:x}", uo_jsnum::conv::to_uint32(x));
            if hex.len() < 2 {
                out.push('0');
            }
            out.push_str(&hex);
        }
        return Some(out.to_ascii_uppercase());
    }
    NAMED.iter().find(|(k, _)| eq(&s, k)).map(|(_, v)| (*v).to_owned())
}

/// `cssSizeHalfPt(v)` → half points (`Some(NaN)` for the `Object.prototype` names, as the TS).
#[must_use]
pub fn css_size_half_pt(v: &str) -> Option<f64> {
    let s = lower(uo_jsnum::text::trim(v));
    let kw: [(&str, f64); 7] =
        [("xx-small", 7.5), ("x-small", 7.5), ("small", 10.0), ("medium", 12.0), ("large", 13.5), ("x-large", 18.0), ("xx-large", 24.0)];
    if let Some((_, k)) = kw.iter().find(|(n, _)| eq(&s, n)) {
        return Some(uo_jsnum::math::round(k * 2.0));
    }
    if PROTO.iter().any(|p| eq(&s, p)) {
        return Some(f64::NAN);
    }
    let (num, pt) = rx::size_unit(&s)?;
    let n = uo_jsnum::parse::parse_float(&String::from_utf16_lossy(num));
    let pt = if pt { n } else { n * 0.75 };
    (pt > 0.0 && pt < 1640.0).then(|| uo_jsnum::math::round(pt * 2.0))
}

/// `cssFamily(v)`: the first family that is not generic, without quotes.
#[must_use]
pub fn css_family(v: &str) -> Option<String> {
    let u: Vec<u16> = v.encode_utf16().collect();
    for f in u.split(|c| *c == u16::from(b',')) {
        let n = rx::strip_quotes(js_trim(f));
        if !n.is_empty() && !rx::generic_family(n) {
            return Some(String::from_utf16_lossy(n));
        }
    }
    None
}

/// `<font size=1..7>` → half points.
const FONT_SIZES: [f64; 8] = [0.0, 15.0, 20.0, 24.0, 27.0, 36.0, 48.0, 72.0];

fn set_str(p: &mut RunProps, key: &str, v: &str) {
    let v: Str = v.into();
    match key {
        "fontAscii" => obj::set(p, key, |x| x.font_ascii = Some(v)),
        "color" => obj::set(p, key, |x| x.color = Some(v)),
        "link" => obj::set(p, key, |x| x.link = Some(v)),
        "vertAlign" => obj::set(p, key, |x| x.vert_align = Some(v)),
        _ => {}
    }
}

fn set_bool(p: &mut RunProps, key: &str, v: bool) {
    match key {
        "bold" => obj::set(p, key, |x| x.bold = Some(v)),
        "italic" => obj::set(p, key, |x| x.italic = Some(v)),
        "underline" => obj::set(p, key, |x| x.underline = Some(v)),
        "strike" => obj::set(p, key, |x| x.strike = Some(v)),
        "allCaps" => obj::set(p, key, |x| x.all_caps = Some(v)),
        "smallCaps" => obj::set(p, key, |x| x.small_caps = Some(v)),
        _ => {}
    }
}

fn set_sz(p: &mut RunProps, v: Option<f64>) {
    match v {
        Some(n) => obj::set(p, "szHalfPt", |x| x.sz_half_pt = Some(crate::js::JsNumber::Num(n))),
        // `FONT_SIZES[2.5]`: the key holds `undefined`
        None => obj::set_undefined(p, "szHalfPt", |x| x.sz_half_pt = None),
    }
}

fn contains(s: &str, lit: &str) -> bool {
    s.contains(lit)
}

/// `elementProps(el, base)`: the marks and the inline CSS of an element over the inherited props.
fn element_props(t: &PasteTree, i: usize, base: &RunProps) -> RunProps {
    let mut p = base.clone();
    let Some(el) = t.nodes.get(i) else { return p };
    let tag = el.tag.units();
    let is = |s: &str| eq(tag, s);
    if is("B") || is("STRONG") {
        set_bool(&mut p, "bold", true);
    } else if is("I") || is("EM") || is("CITE") || is("DFN") || is("VAR") {
        set_bool(&mut p, "italic", true);
    } else if is("U") || is("INS") {
        set_bool(&mut p, "underline", true);
    } else if is("S") || is("STRIKE") || is("DEL") {
        set_bool(&mut p, "strike", true);
    } else if is("SUB") {
        set_str(&mut p, "vertAlign", "subscript");
    } else if is("SUP") {
        set_str(&mut p, "vertAlign", "superscript");
    } else if is("CODE") || is("KBD") || is("SAMP") || is("TT") {
        set_str(&mut p, "fontAscii", "Courier New");
    } else if is("FONT") {
        if let Some(face) = el.attr(tree::A_FACE)
            && let Some(f) = css_family(face)
        {
            set_str(&mut p, "fontAscii", &f);
        }
        if let Some(color) = el.attr(tree::A_COLOR)
            && let Some(c) = css_color(color)
        {
            set_str(&mut p, "color", &c);
        }
        let size = el.attr(tree::A_SIZE).map_or(0.0, uo_jsnum::parse::to_number);
        if (1.0..=7.0).contains(&size) {
            let k = uo_jsnum::math::floor(size);
            set_sz(&mut p, if k == size { FONT_SIZES.get(uo_jsnum::conv::to_uint32(k) as usize).copied() } else { None });
        }
    } else if is("A")
        && let Some(href) = el.attr(tree::A_HREF)
    {
        let hu: Vec<u16> = href.encode_utf16().collect();
        if hu.first() != Some(&u16::from(b'#')) && !rx::is_javascript(&hu) {
            set_str(&mut p, "link", href);
        }
    }
    // `el.style` (every element of an HTML document has one)
    if let Some(fw) = el.css(tree::C_FONT_WEIGHT) {
        set_bool(&mut p, "bold", fw == "bold" || fw == "bolder" || uo_jsnum::parse::to_number(fw) >= 600.0);
    }
    if let Some(fs) = el.css(tree::C_FONT_STYLE) {
        set_bool(&mut p, "italic", fs == "italic" || fs == "oblique");
    }
    if let Some(td) = el.css(tree::C_TEXT_DECORATION_LINE).or_else(|| el.css(tree::C_TEXT_DECORATION)) {
        if contains(td, "underline") {
            set_bool(&mut p, "underline", true);
        } else if contains(td, "none") {
            set_bool(&mut p, "underline", false);
        }
        if contains(td, "line-through") {
            set_bool(&mut p, "strike", true);
        }
    }
    if let Some(c) = el.css(tree::C_COLOR).and_then(css_color) {
        set_str(&mut p, "color", &c);
    }
    if let Some(f) = el.css(tree::C_FONT_FAMILY).and_then(css_family) {
        set_str(&mut p, "fontAscii", &f);
    }
    if let Some(s) = el.css(tree::C_FONT_SIZE).and_then(css_size_half_pt)
        && uo_jsnum::conv::truthy(s)
    {
        set_sz(&mut p, Some(s));
    }
    match el.css(tree::C_VERTICAL_ALIGN) {
        Some("super") => set_str(&mut p, "vertAlign", "superscript"),
        Some("sub") => set_str(&mut p, "vertAlign", "subscript"),
        _ => {}
    }
    if el.css(tree::C_TEXT_TRANSFORM) == Some("uppercase") {
        set_bool(&mut p, "allCaps", true);
    }
    if el.css(tree::C_FONT_VARIANT) == Some("small-caps") || el.css(tree::C_FONT_VARIANT_CAPS) == Some("small-caps") {
        set_bool(&mut p, "smallCaps", true);
    }
    p
}

/// `decodeDataUrl(src)`.
#[must_use]
pub fn decode_data_url(src: &str) -> Option<(Vec<u8>, Str)> {
    let u: Vec<u16> = src.encode_utf16().collect();
    let (ty, payload) = rx::data_url(&u)?;
    let data = atob(&rx::remove_spaces(payload))?;
    let ext = String::from_utf16_lossy(ty).to_ascii_lowercase().replacen("jpeg", "jpg", 1);
    Some((data, ext.into()))
}

/// `fragmentOf(html)`: the part between `<!--StartFragment-->` and `<!--EndFragment-->` (CF_HTML).
#[must_use]
pub fn fragment_of(html: &[u16]) -> &[u16] {
    let find = |lit: &str| {
        let l: Vec<u16> = lit.encode_utf16().collect();
        html.windows(l.len()).position(|w| w == l.as_slice())
    };
    match (find("<!--StartFragment-->"), find("<!--EndFragment-->")) {
        (Some(a), Some(b)) if b > a => html.get(a + 20..b).unwrap_or(&[]),
        _ => html,
    }
}

/// `wordListFormats(html)`: `"list:level"` (the digits as written) → is a bullet.
#[must_use]
pub fn word_list_formats(html: &[u16]) -> Vec<(Vec<u16>, bool)> {
    // the TS `Map`: insertion order kept by `out`, the index makes `set` O(1) (hostile HTML
    // can carry a million `@list` definitions)
    let mut out: Vec<(Vec<u16>, bool)> = Vec::new();
    let mut index: HashMap<Vec<u16>, usize> = HashMap::new();
    for (l, v, fmt) in rx::word_list_defs(html) {
        let mut key = l;
        key.push(u16::from(b':'));
        key.extend_from_slice(&v);
        let bullet = matches!(fmt.as_deref(), Some("bullet" | "image"));
        match index.get(&key) {
            Some(&at) => {
                if let Some(e) = out.get_mut(at) {
                    e.1 = bullet;
                }
            }
            None => {
                index.insert(key.clone(), out.len());
                out.push((key, bullet));
            }
        }
    }
    out
}

/// `wordListOf(style)`: `{ list, level, lfo }` as numbers.
#[must_use]
pub fn word_list_of(style: &[u16]) -> Option<(f64, f64, f64)> {
    let [a, b, c] = rx::word_list_of(style)?;
    Some((to_num(&a), to_num(&b), to_num(&c)))
}

/// `looksOrdered(marker)`.
#[must_use]
pub fn looks_ordered(marker: &[u16]) -> bool {
    rx::looks_ordered_units(&rx::remove_spaces(marker))
}

/// `wordMarker(el)`: the text between the `[if !supportLists]` and `[endif]` comments, or of the
/// `mso-list:Ignore` span (a pre-order walk that stops at the first answer).
fn word_marker(t: &PasteTree, el: usize) -> Vec<u16> {
    let mut on = false;
    let mut text: Vec<u16> = Vec::new();
    let end = t.nodes.get(el).map_or(el, |n| el + n.size as usize);
    let mut j = el + 1;
    while j < end {
        let Some(n) = t.nodes.get(j) else { break };
        match n.kind {
            NodeKind::Comment => {
                let v = js_trim(n.value.units());
                if rx::is_support_lists(v) {
                    on = true;
                } else if rx::is_endif(v) && on {
                    return text;
                }
            }
            NodeKind::Text => {
                if on {
                    text.extend_from_slice(n.value.units());
                }
            }
            NodeKind::Cdata => {}
            NodeKind::Element => {
                if n.attr(tree::A_STYLE).is_some_and(|s| rx::is_mso_list_ignore(&s.encode_utf16().collect::<Vec<_>>())) {
                    text.extend(t.text_content(j));
                    return text;
                }
            }
        }
        j += 1;
    }
    text
}

#[derive(Clone)]
struct Ctx {
    props: Arc<RunProps>,
    pre: bool,
    block: HtmlBlock,
    block_bold: bool,
}

enum Post {
    None,
    /// `lists.pop(); close()`
    List,
    /// `close()`
    Block,
    /// `cur = p` (a cell of a table row)
    Cur(usize),
}

enum Frame {
    Walk(usize, Ctx),
    Children {
        next: usize,
        end: usize,
        ctx: Ctx,
        skipping: bool,
        post: Post,
    },
    /// the cells of a `<tr>`: `p` is the row's paragraph
    Row {
        p: usize,
        next: usize,
        end: usize,
        props: Arc<RunProps>,
        first: bool,
    },
}

struct W<'a> {
    t: &'a PasteTree,
    word_lists: HashMap<Vec<u16>, bool>,
    out: Vec<HtmlPara>,
    cur: Option<usize>,
    list_key: f64,
    lists: Vec<(bool, f64)>,
    stack: Vec<Frame>,
}

impl W<'_> {
    fn open(&mut self, b: &HtmlBlock) -> usize {
        self.out.push(HtmlPara { runs: Vec::new(), block: b.clone() });
        let i = self.out.len() - 1;
        self.cur = Some(i);
        i
    }
    fn close(&mut self) {
        self.cur = None;
    }
    fn ensure(&mut self, ctx: &Ctx) -> usize {
        match self.cur {
            Some(i) => i,
            None => self.open(&ctx.block),
        }
    }
    fn push(&mut self, at: usize, run: HtmlRun) {
        if let Some(p) = self.out.get_mut(at) {
            p.runs.push(run);
        }
    }
    fn run(ctx: &Ctx, text: Vec<u16>) -> HtmlRun {
        HtmlRun { text: Utf16::from_units(text), props: Arc::clone(&ctx.props), block_bold: ctx.block_bold, image: None }
    }

    fn text(&mut self, raw: &[u16], ctx: &Ctx) {
        let mut t = if ctx.pre { rx::normalize_newlines(raw) } else { rx::collapse_html_space(raw) };
        if !ctx.pre {
            let p = self.cur.and_then(|i| self.out.get(i));
            let reset = match p {
                None => true,
                Some(p) => p.runs.last().is_none_or(|r| rx::ends_space_or_newline(r.text.units())),
            };
            if reset {
                t = rx::trim_leading_spaces(&t).to_vec();
            }
        }
        if t.is_empty() {
            return;
        }
        if ctx.pre && t.contains(&10) {
            for (i, part) in t.split(|u| *u == 10).enumerate() {
                if i > 0 {
                    self.close();
                    self.open(&ctx.block);
                }
                if !part.is_empty() {
                    let at = self.ensure(ctx);
                    self.push(at, Self::run(ctx, part.to_vec()));
                }
            }
            return;
        }
        let at = self.ensure(ctx);
        self.push(at, Self::run(ctx, t));
    }

    fn children(&mut self, i: usize, ctx: Ctx, post: Post) {
        let end = self.t.nodes.get(i).map_or(i, |n| i + n.size as usize);
        self.stack.push(Frame::Children { next: i + 1, end, ctx, skipping: false, post });
    }

    fn walk(&mut self, i: usize, ctx: Ctx) {
        let t = self.t;
        let Some(el) = t.nodes.get(i) else { return };
        match el.kind {
            NodeKind::Text => {
                self.text(el.value.units(), &ctx);
                return;
            }
            NodeKind::Element => {}
            _ => return,
        }
        let raw = el.tag.units();
        let tag = uo_text::case::to_uppercase(raw);
        let tag: &[u16] = &tag;
        if in_set(tag, &SKIP) || in_set(raw, &SKIP) {
            return;
        }
        if el.css(tree::C_DISPLAY) == Some("none") {
            return;
        }
        let style: Vec<u16> = el.attr(tree::A_STYLE).map(|s| s.encode_utf16().collect()).unwrap_or_default();
        if rx::is_mso_list_ignore(&style) {
            return;
        }
        if eq(tag, "BR") {
            let at = self.ensure(&ctx);
            self.push(at, HtmlRun { text: Utf16::from("\n"), props: Arc::clone(&ctx.props), block_bold: false, image: None });
            return;
        }
        if eq(tag, "IMG") {
            if let Some((data, ext)) = decode_data_url(el.attr(tree::A_SRC).unwrap_or("")) {
                let num = |k: usize| {
                    let n = el.attr(k).map_or(0.0, uo_jsnum::parse::to_number);
                    uo_jsnum::conv::truthy(n).then_some(n)
                };
                let (w, h) = (num(tree::A_WIDTH), num(tree::A_HEIGHT));
                let at = self.ensure(&ctx);
                let image = HtmlImage { data: Arc::from(data), ext, w_px: w, h_px: h };
                self.push(at, HtmlRun { text: Utf16::new(), props: Arc::clone(&ctx.props), block_bold: false, image: Some(image) });
            }
            return;
        }
        if eq(tag, "HR") {
            self.close();
            return;
        }
        let mut props = element_props(t, i, &ctx.props);
        if !in_set(tag, &BLOCK) {
            let inner = Ctx { props: Arc::new(props), ..ctx };
            self.children(i, inner, Post::None);
            return;
        }
        // ── block ──
        if eq(tag, "UL") || eq(tag, "OL") {
            if self.lists.is_empty() {
                self.list_key += 1.0;
            }
            self.lists.push((eq(tag, "OL"), self.list_key));
            self.close();
            let inner = Ctx { props: Arc::new(props), ..ctx };
            self.children(i, inner, Post::List);
            return;
        }
        let ta = el.css(tree::C_TEXT_ALIGN).or_else(|| el.attr(tree::A_ALIGN)).unwrap_or(if eq(tag, "CENTER") { "center" } else { "" });
        let align = match ta {
            "center" => Some("center"),
            "right" | "end" => Some("right"),
            "justify" => Some("both"),
            "left" | "start" => Some("left"),
            _ => ctx.block.align,
        };
        let mut block = HtmlBlock { align, ..HtmlBlock::default() };
        let h = rx::heading_tag(tag);
        let mut block_bold = ctx.block_bold;
        if let Some(n) = h {
            block.heading = Some(f64::from(n));
            block_bold = true;
        }
        let cls: Vec<u16> = el.attr(tree::A_CLASS).map(|s| s.encode_utf16().collect()).unwrap_or_default();
        if h.is_none()
            && let Some(d) = rx::mso_heading(&cls)
        {
            block.heading = Some(f64::from(d));
            block_bold = true;
        }
        if rx::mso_title(&cls) {
            block.style = Some(TitleStyle::Title);
        } else if rx::mso_subtitle(&cls) {
            block.style = Some(TitleStyle::Subtitle);
        }
        let wl = if block.heading.is_none() { word_list_of(&style) } else { None };
        if let Some((list, level, lfo)) = wl {
            let mut key: Vec<u16> = uo_jsnum::format::to_js_string(list).encode_utf16().collect();
            key.push(u16::from(b':'));
            key.extend(uo_jsnum::format::to_js_string(level).encode_utf16());
            let bullet = match self.word_lists.get(&key) {
                Some(&b) => b,
                None => !looks_ordered(&word_marker(t, i)),
            };
            let lv = uo_jsnum::math::max(0.0, uo_jsnum::math::min(8.0, level - 1.0));
            block.list = Some(ClipHtmlList { ordered: !bullet, level: lv, key: 100_000.0 + lfo });
        }
        if eq(tag, "PRE") {
            block.pre = true;
            if props.font_ascii.is_none() {
                obj::set(&mut props, "fontAscii", |x| x.font_ascii = Some("Courier New".into()));
            }
        }
        if eq(tag, "LI")
            && let Some(&(ordered, key)) = self.lists.last()
        {
            let n = self.lists.len() as f64;
            block.list = Some(ClipHtmlList { ordered, level: uo_jsnum::math::min(8.0, n - 1.0), key });
        }
        if ["P", "DIV", "BLOCKQUOTE", "ADDRESS", "DD", "DT", "FIGCAPTION"].iter().any(|s| eq(tag, s)) {
            block.web = true;
        }
        if eq(tag, "TR") {
            // a table row in the middle of the text: the cells on one line, tab-separated
            self.close();
            let p = self.open(&block);
            let end = i + el.size as usize;
            self.stack.push(Frame::Row { p, next: i + 1, end, props: Arc::new(props), first: true });
            return;
        }
        self.close();
        let inner = Ctx { props: Arc::new(props), pre: ctx.pre || block.pre, block: block.clone(), block_bold };
        // a list item: the paragraph starts now (even empty)
        if block.list.is_some() {
            self.open(&inner.block);
        }
        self.children(i, inner, Post::Block);
    }

    fn run_all(&mut self) {
        while let Some(f) = self.stack.pop() {
            match f {
                Frame::Walk(i, ctx) => self.walk(i, ctx),
                Frame::Children { next, end, ctx, mut skipping, post } => {
                    let mut k = next;
                    let mut descend: Option<usize> = None;
                    while k < end {
                        let Some(n) = self.t.nodes.get(k) else { break };
                        let size = (n.size as usize).max(1);
                        if n.kind == NodeKind::Comment {
                            let v = js_trim(n.value.units());
                            if rx::is_support_lists(v) {
                                skipping = true;
                            } else if rx::is_endif(v) {
                                skipping = false;
                            }
                            k += size;
                            continue;
                        }
                        if !skipping {
                            descend = Some(k);
                            k += size;
                            break;
                        }
                        k += size;
                    }
                    match descend {
                        Some(c) => {
                            let walk_ctx = ctx.clone();
                            self.stack.push(Frame::Children { next: k, end, ctx, skipping, post });
                            self.stack.push(Frame::Walk(c, walk_ctx));
                        }
                        None => match post {
                            Post::None => {}
                            Post::List => {
                                self.lists.pop();
                                self.close();
                            }
                            Post::Block => self.close(),
                            Post::Cur(p) => self.cur = Some(p),
                        },
                    }
                }
                Frame::Row { p, next, end, props, first } => {
                    // the next element child (`el.children`)
                    let mut k = next;
                    let mut cell: Option<usize> = None;
                    while k < end {
                        let Some(n) = self.t.nodes.get(k) else { break };
                        let size = (n.size as usize).max(1);
                        if n.kind == NodeKind::Element {
                            cell = Some(k);
                            k += size;
                            break;
                        }
                        k += size;
                    }
                    let Some(c) = cell else {
                        self.close();
                        continue;
                    };
                    if !first {
                        self.push(p, HtmlRun { text: Utf16::from("\t"), props: Arc::clone(&props), block_bold: false, image: None });
                    }
                    let th = self.t.nodes.get(c).is_some_and(|n| eq(&uo_text::case::to_uppercase(n.tag.units()), "TH"));
                    let inner =
                        Ctx { props: Arc::new(element_props(self.t, c, &props)), pre: false, block: HtmlBlock::default(), block_bold: th };
                    self.cur = Some(p);
                    self.stack.push(Frame::Row { p, next: k, end, props, first: false });
                    self.children(c, inner, Post::Cur(p));
                }
            }
        }
    }
}

/// `htmlToParas(html)` over the paste tree: the paragraphs of the fragment (images only from
/// `data:` URLs; Word's lists become lists without the marker it writes as text).
#[must_use]
pub fn html_to_paras(h: &PasteHtml) -> Vec<HtmlPara> {
    let mut w = W {
        t: &h.fragment,
        word_lists: word_list_formats(h.raw.units()).into_iter().collect(),
        out: Vec::new(),
        cur: None,
        list_key: 0.0,
        lists: Vec::new(),
        stack: Vec::new(),
    };
    if !h.fragment.nodes.is_empty() {
        let ctx = Ctx { props: Arc::new(RunProps::default()), pre: false, block: HtmlBlock::default(), block_bold: false };
        w.stack.push(Frame::Walk(0, ctx));
        w.run_all();
    }
    // without text or image: out
    w.out.retain(|p| p.block.list.is_some() || p.runs.iter().any(|r| r.image.is_some() || !rx::remove_spaces(r.text.units()).is_empty()));
    w.out
}
