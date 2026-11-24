//! Styles of the `.xls` (FONT/FORMAT/XF/PALETTE/XFEXT) → `xl/styles.xml` (`styles-xml.ts`).
//!
//! Each XF of the BIFF (style or cell) becomes the `cellXfs[i]` of the SAME index: the cells point at the `ixfe`
//! directly. Indexed colours are resolved to RGB through the effective palette; the full colours of the XFEXT (Excel
//! 2007+) win.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::biff::RichString;
use super::globals::{FullColor, XlsFont, XlsGlobals, XlsXf, font_index};
use super::xmlw::{NS_MAIN, XML_HEAD, xml_text};

const FILL_PATTERNS: [&str; 19] = [
    "none",
    "solid",
    "mediumGray",
    "darkGray",
    "lightGray",
    "darkHorizontal",
    "darkVertical",
    "darkDown",
    "darkUp",
    "darkGrid",
    "darkTrellis",
    "lightHorizontal",
    "lightVertical",
    "lightDown",
    "lightUp",
    "lightGrid",
    "lightTrellis",
    "gray125",
    "gray0625",
];
const BORDER_STYLES: [&str; 14] = [
    "",
    "thin",
    "medium",
    "dashed",
    "dotted",
    "thick",
    "double",
    "hair",
    "mediumDashed",
    "dashDot",
    "mediumDashDot",
    "dashDotDot",
    "mediumDashDotDot",
    "slantDashDot",
];
const H_ALIGN: [&str; 8] = ["", "left", "center", "right", "fill", "justify", "centerContinuous", "distributed"];
const V_ALIGN: [&str; 5] = ["top", "center", "", "justify", "distributed"];

/// The role of an indexed colour (`icvColor`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Font,
    Fg,
    Bg,
    Border,
}

/// `icvColor`: indexed colour (icv) → `#RRGGBB`; `None` = automatic.
pub fn icv_color(icv: u32, palette: &[String], role: Role) -> Option<String> {
    if icv < 64 {
        return palette.get(usize::try_from(icv).unwrap_or(usize::MAX)).cloned();
    }
    match icv {
        0x40 => Some(if role == Role::Bg { return None } else { "#000000".to_owned() }), // system foreground
        0x41 => Some("#FFFFFF".to_owned()),                                              // system background
        0x50 => Some("#FFFFE1".to_owned()),                                              // tooltip background
        0x51 => Some("#000000".to_owned()),                                              // tooltip text
        0x7fff => None,                                                                  // automatic
        _ => Some(if role == Role::Bg { "#FFFFFF" } else { "#000000" }.to_owned()),
    }
}

/// `+x.toFixed(6)` printed as a JS number (`0.5`, `-0.249977`).
pub fn fixed_num(x: f64, digits: u32) -> String {
    let s = uo_jsnum::format::to_fixed(x, digits).unwrap_or_default();
    uo_jsnum::format::to_js_string(uo_jsnum::parse::to_number(&s))
}

/// `colorEl`: the `<color …/>` of a full XFEXT colour or of the indexed colour.
pub fn color_el(tag: &str, ext: Option<&FullColor>, rgb: Option<&str>) -> String {
    if let Some(e) = ext {
        let tint = if e.tint != 0.0 { format!(" tint=\"{}\"", fixed_num(e.tint, 6)) } else { String::new() };
        if e.ty == 2
            && let Some(v) = &e.rgb
        {
            return format!("<{tag} rgb=\"FF{v}\"{tint}/>");
        }
        if e.ty == 3
            && let Some(i) = e.index
        {
            return format!("<{tag} theme=\"{i}\"{tint}/>");
        }
    }
    match rgb {
        Some(c) if !c.is_empty() => format!("<{tag} rgb=\"FF{}\"/>", c.get(1..).unwrap_or("")),
        _ => String::new(),
    }
}

/// `fontBody`: the content of a `<font>`/`<rPr>` (without the outer tag). `run`: explicit booleans.
pub fn font_body(f: &XlsFont, palette: &[String], run: bool, color_override: Option<&FullColor>) -> String {
    let b = f.weight >= 600;
    let mut x = String::new();
    if run {
        x.push_str(if b { "<b/>" } else { "<b val=\"0\"/>" });
        x.push_str(if f.italic { "<i/>" } else { "<i val=\"0\"/>" });
        x.push_str(if f.strike { "<strike/>" } else { "<strike val=\"0\"/>" });
    } else {
        if b {
            x.push_str("<b/>");
        }
        if f.italic {
            x.push_str("<i/>");
        }
        if f.strike {
            x.push_str("<strike/>");
        }
    }
    if f.outline {
        x.push_str("<outline/>");
    }
    if f.shadow {
        x.push_str("<shadow/>");
    }
    let u = match f.underline {
        1 => Some(""),
        2 => Some("double"),
        0x21 => Some("singleAccounting"),
        0x22 => Some("doubleAccounting"),
        _ => None,
    };
    match (f.underline != 0, u) {
        (true, Some(u)) => {
            if u.is_empty() {
                x.push_str("<u/>");
            } else {
                x.push_str(&format!("<u val=\"{u}\"/>"));
            }
        }
        _ => {
            if run {
                x.push_str("<u val=\"none\"/>");
            }
        }
    }
    match f.script {
        1 => x.push_str("<vertAlign val=\"superscript\"/>"),
        2 => x.push_str("<vertAlign val=\"subscript\"/>"),
        _ => {}
    }
    x.push_str(&format!("<sz val=\"{}\"/>", uo_jsnum::format::to_js_string(f64::from(f.height) / 20.0)));
    // in a run the automatic colour is EXPLICIT (the black of the window text): the run does not inherit the cell's
    // colour (measured: ".." white + automatic text = black text)
    let auto = icv_color(u32::from(f.color), palette, Role::Font).or_else(|| if run { Some("#000000".to_owned()) } else { None });
    x.push_str(&color_el("color", color_override, auto.as_deref()));
    let tag = if run { "rFont" } else { "name" };
    x.push_str(&format!("<{tag} val=\""));
    xml_text(&mut x, &f.name);
    x.push_str("\"/>");
    if f.family != 0 {
        x.push_str(&format!("<family val=\"{}\"/>", f.family));
    }
    if f.charset != 0 {
        x.push_str(&format!("<charset val=\"{}\"/>", f.charset));
    }
    x
}

static DEFAULT_FONT: LazyLock<XlsFont> = LazyLock::new(|| XlsFont {
    height: 200,
    italic: false,
    strike: false,
    outline: false,
    shadow: false,
    weight: 400,
    script: 0,
    underline: 0,
    family: 2,
    charset: 0,
    color: 0x7fff,
    name: "Arial".to_owned(),
});

/// `fontAt`: the font of a BIFF `ifnt` (falling back to font 0 / Arial 10).
pub fn font_at(g: &XlsGlobals, ifnt: u16) -> &XlsFont {
    g.fonts.get(font_index(ifnt)).or_else(|| g.fonts.first()).unwrap_or(&DEFAULT_FONT)
}

fn default_xf() -> XlsXf {
    XlsXf { valign: 2, fore: 64, back: 65, ..XlsXf::default() }
}

/// Builds `xl/styles.xml`.
pub fn styles_xml(g: &XlsGlobals, dxfs: &[String]) -> String {
    let pal = &g.palette;
    // fonts: the file's in order (fontId = position) + the colour variants of the XFEXT
    let mut fonts: Vec<String> = Vec::new();
    let mut font_key: HashMap<String, usize> = HashMap::new();
    let base_fonts: Vec<&XlsFont> = if g.fonts.is_empty() { vec![&*DEFAULT_FONT] } else { g.fonts.iter().collect() };
    for f in &base_fonts {
        // the file's fonts are not deduplicated: fontId = BIFF position
        let body = font_body(f, pal, false, None);
        fonts.push(body.clone());
        font_key.entry(body).or_insert(fonts.len() - 1);
    }

    let mut fills: Vec<String> =
        vec!["<patternFill patternType=\"none\"/>".to_owned(), "<patternFill patternType=\"gray125\"/>".to_owned()];
    let mut fill_key: HashMap<String, usize> = fills.iter().enumerate().map(|(i, f)| (f.clone(), i)).collect();
    let mut borders: Vec<String> = vec!["<left/><right/><top/><bottom/><diagonal/>".to_owned()];
    let mut border_attrs: Vec<String> = vec![String::new()];
    let mut border_key: HashMap<String, usize> = HashMap::from([("|<left/><right/><top/><bottom/><diagonal/>".to_owned(), 0)]);

    let default = default_xf();
    let xfs: Vec<&XlsXf> = if g.xfs.is_empty() { vec![&default] } else { g.xfs.iter().collect() };
    let mut xf_xml: Vec<String> = Vec::with_capacity(xfs.len());
    for xf in &xfs {
        // font
        let mut font_id = font_index(xf.font).min(base_fonts.len() - 1);
        if let Some(fcol) = xf.ext(13).filter(|c| c.ty == 2 || c.ty == 3) {
            let body = font_body(base_fonts[font_id], pal, false, Some(fcol));
            font_id = match font_key.get(&body) {
                Some(&k) => k,
                None => {
                    fonts.push(body.clone());
                    font_key.insert(body, fonts.len() - 1);
                    fonts.len() - 1
                }
            };
        }
        // fill
        let mut fill_id = 0usize;
        if xf.pattern > 0 {
            let pt = FILL_PATTERNS.get(usize::try_from(xf.pattern).unwrap_or(usize::MAX)).copied().unwrap_or("solid");
            let fg = color_el("fgColor", xf.ext(4), icv_color(xf.fore, pal, Role::Fg).as_deref());
            let bg = color_el("bgColor", xf.ext(5), icv_color(xf.back, pal, Role::Bg).as_deref());
            let body = format!("<patternFill patternType=\"{pt}\">{fg}{bg}</patternFill>");
            fill_id = match fill_key.get(&body) {
                Some(&k) => k,
                None => {
                    fills.push(body.clone());
                    fill_key.insert(body, fills.len() - 1);
                    fills.len() - 1
                }
            };
        }
        // borders
        let edge = |tag: &str, dg: u32, icv: u32, ext_type: u16| -> String {
            let st = BORDER_STYLES.get(usize::try_from(dg).unwrap_or(usize::MAX)).copied().unwrap_or("");
            if dg == 0 || st.is_empty() {
                return format!("<{tag}/>");
            }
            let c = color_el("color", xf.ext(ext_type), icv_color(icv, pal, Role::Border).as_deref());
            format!("<{tag} style=\"{st}\">{}</{tag}>", if c.is_empty() { "<color auto=\"1\"/>" } else { &c })
        };
        let diag_on = xf.diag != 0 && xf.b_diag != 0;
        let mut b_body = edge("left", xf.b_left, xf.c_left, 9);
        b_body.push_str(&edge("right", xf.b_right, xf.c_right, 10));
        b_body.push_str(&edge("top", xf.b_top, xf.c_top, 7));
        b_body.push_str(&edge("bottom", xf.b_bottom, xf.c_bottom, 8));
        if diag_on {
            b_body.push_str(&edge("diagonal", xf.b_diag, xf.c_diag, 11));
        } else {
            b_body.push_str("<diagonal/>");
        }
        let mut b_attr = String::new();
        if diag_on {
            if xf.diag & 2 != 0 {
                b_attr.push_str(" diagonalUp=\"1\"");
            }
            if xf.diag & 1 != 0 {
                b_attr.push_str(" diagonalDown=\"1\"");
            }
        }
        let bk = format!("{b_attr}|{b_body}");
        let border_id = match border_key.get(&bk) {
            Some(&k) => k,
            None => {
                borders.push(b_body);
                border_attrs.push(b_attr);
                border_key.insert(bk, borders.len() - 1);
                borders.len() - 1
            }
        };
        // alignment
        let mut al = String::new();
        let h = H_ALIGN.get(usize::from(xf.halign)).copied().unwrap_or("");
        if !h.is_empty() {
            al.push_str(&format!(" horizontal=\"{h}\""));
        }
        let v = V_ALIGN.get(usize::from(xf.valign)).copied().unwrap_or("");
        if !v.is_empty() {
            al.push_str(&format!(" vertical=\"{v}\""));
        }
        if xf.wrap {
            al.push_str(" wrapText=\"1\"");
        }
        if xf.rot != 0 {
            al.push_str(&format!(" textRotation=\"{}\"", xf.rot));
        }
        if xf.indent != 0 {
            al.push_str(&format!(" indent=\"{}\"", xf.indent));
        }
        if xf.shrink {
            al.push_str(" shrinkToFit=\"1\"");
        }
        let mut a = format!("<xf numFmtId=\"{}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"{border_id}\" xfId=\"0\"", xf.fmt);
        if xf.fmt != 0 {
            a.push_str(" applyNumberFormat=\"1\"");
        }
        if font_id != 0 {
            a.push_str(" applyFont=\"1\"");
        }
        if fill_id != 0 {
            a.push_str(" applyFill=\"1\"");
        }
        if border_id != 0 {
            a.push_str(" applyBorder=\"1\"");
        }
        if !al.is_empty() {
            a.push_str(" applyAlignment=\"1\"");
        }
        xf_xml.push(if al.is_empty() { format!("{a}/>") } else { format!("{a}><alignment{al}/></xf>") });
    }

    let mut num_fmts = String::new();
    let mut n_num = 0usize;
    for (id, code) in &g.formats {
        if !include_format(*id) {
            continue;
        }
        num_fmts.push_str(&format!("<numFmt numFmtId=\"{id}\" formatCode=\""));
        xml_text(&mut num_fmts, &normalize_format(code));
        num_fmts.push_str("\"/>");
        n_num += 1;
    }

    let mut x = String::new();
    x.push_str(XML_HEAD);
    x.push_str(&format!("<styleSheet xmlns=\"{NS_MAIN}\">"));
    if n_num > 0 {
        x.push_str(&format!("<numFmts count=\"{n_num}\">{num_fmts}</numFmts>"));
    }
    x.push_str(&format!("<fonts count=\"{}\">", fonts.len()));
    for f in &fonts {
        x.push_str("<font>");
        x.push_str(f);
        x.push_str("</font>");
    }
    x.push_str("</fonts>");
    x.push_str(&format!("<fills count=\"{}\">", fills.len()));
    for f in &fills {
        x.push_str("<fill>");
        x.push_str(f);
        x.push_str("</fill>");
    }
    x.push_str("</fills>");
    x.push_str(&format!("<borders count=\"{}\">", borders.len()));
    for (i, b) in borders.iter().enumerate() {
        x.push_str("<border");
        x.push_str(&border_attrs[i]);
        x.push('>');
        x.push_str(b);
        x.push_str("</border>");
    }
    x.push_str("</borders>");
    x.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    x.push_str(&format!("<cellXfs count=\"{}\">", xf_xml.len()));
    for s in &xf_xml {
        x.push_str(s);
    }
    x.push_str("</cellXfs>");
    x.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>");
    if !dxfs.is_empty() {
        x.push_str(&format!("<dxfs count=\"{}\">", dxfs.len()));
        for d in dxfs {
            x.push_str(d);
        }
        x.push_str("</dxfs>");
    }
    x.push_str("</styleSheet>");
    x
}

/// `normalizeFormat`: a BIFF format code as Excel reads it — the keyword `General` is case-insensitive (LibreOffice
/// writes `GENERAL`); outside quotes, brackets and escapes it becomes the canonical spelling.
pub fn normalize_format(code: &str) -> String {
    if !contains_general(code) {
        return code.to_owned();
    }
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' || ch == '[' {
            let close = if ch == '"' { '"' } else { ']' };
            let e = chars[i + 1..].iter().position(|&c| c == close).map_or(chars.len(), |j| i + 1 + j + 1);
            out.extend(&chars[i..e]);
            i = e;
            continue;
        }
        if ch == '\\' || ch == '_' || ch == '*' {
            out.extend(&chars[i..(i + 2).min(chars.len())]);
            i += 2;
            continue;
        }
        if i + 7 <= chars.len() && chars[i..i + 7].iter().map(|c| c.to_ascii_lowercase()).eq("general".chars()) {
            out.push_str("General");
            i += 7;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// `/general/i`.
fn contains_general(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(7).any(|w| w.eq_ignore_ascii_case(b"general"))
}

/// The FORMATs of the file that go into `<numFmts>`: the BIFF writes the currency/accounting ones (5–8, 41–44) in the
/// locale of whoever saved, and Excel pt-BR USES those (measured: id 6 `"£"#,##0` shows "£1", not "R$"); the other
/// built-ins (< 50) follow the locale of whoever opens the file and keep the engine's built-in code.
fn include_format(id: u16) -> bool {
    id >= 50 || (5..=8).contains(&id) || (41..=44).contains(&id)
}

/// `richXml`: the `<si>` content (or `<is>`/`<text>`) of a rich string.
pub fn rich_xml(g: &XlsGlobals, out: &mut String, s: &RichString) {
    if s.runs.is_empty() {
        out.push_str("<t xml:space=\"preserve\">");
        xml_text(out, &s.text);
        out.push_str("</t>");
        return;
    }
    let mut sorted = s.runs.clone();
    sorted.sort_by_key(|r| r.ich); // stable, as Array.prototype.sort
    let units: Option<Vec<u16>> = if s.text.is_ascii() { None } else { Some(s.text.encode_utf16().collect()) };
    let len = units.as_ref().map_or(s.text.len(), Vec::len);
    let slice = |a: usize, b: usize| -> String {
        let b = b.min(len);
        let a = a.min(b);
        match &units {
            None => s.text.get(a..b).unwrap_or("").to_owned(),
            Some(u) => String::from_utf16_lossy(&u[a..b]),
        }
    };
    let start = out.len();
    let first = usize::from(sorted[0].ich);
    if first > 0 {
        out.push_str("<r><t xml:space=\"preserve\">");
        xml_text(out, &slice(0, first));
        out.push_str("</t></r>");
    }
    for i in 0..sorted.len() {
        let a = usize::from(sorted[i].ich);
        let b = if i + 1 < sorted.len() { usize::from(sorted[i + 1].ich) } else { len };
        if b <= a || a >= len {
            continue;
        }
        out.push_str("<r><rPr>");
        out.push_str(&font_body(font_at(g, sorted[i].ifnt), &g.palette, true, None));
        out.push_str("</rPr><t xml:space=\"preserve\">");
        xml_text(out, &slice(a, b));
        out.push_str("</t></r>");
    }
    if out.len() == start {
        out.push_str("<t xml:space=\"preserve\">");
        xml_text(out, &s.text);
        out.push_str("</t>");
    }
}
