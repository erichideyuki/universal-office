//! `theme.ts`: the theme palette, the indexed palette and color resolution (with the Windows integer HLS tint).

use super::model::{ColorContext, Theme, XChild, XEl};
use super::{El, SHEET_XML, owned, pf, pi};
use uo_opc::Package;
use uo_opc::xml::Node;

/// `DEFAULT_THEME_COLORS`: the Office 2023 palette, in `theme="N"` order.
pub const DEFAULT_THEME_COLORS: [&str; 12] =
    ["#FFFFFF", "#000000", "#E8E8E8", "#0E2841", "#156082", "#E97132", "#196B24", "#0F9ED5", "#A02B93", "#4EA72E", "#467886", "#96607D"];

/// `INDEXED_COLORS_DEFAULT` (0..=65).
pub const INDEXED_COLORS_DEFAULT: [&str; 66] = [
    "#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF", "#000000", "#FFFFFF", "#FF0000", "#00FF00",
    "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF", "#800000", "#008000", "#000080", "#808000", "#800080", "#008080", "#C0C0C0", "#808080",
    "#9999FF", "#993366", "#FFFFCC", "#CCFFFF", "#660066", "#FF8080", "#0066CC", "#CCCCFF", "#000080", "#FF00FF", "#FFFF00", "#00FFFF",
    "#800080", "#800000", "#008080", "#0000FF", "#00CCFF", "#CCFFFF", "#CCFFCC", "#FFFF99", "#99CCFF", "#FF99CC", "#CC99FF", "#FFCC99",
    "#3366FF", "#33CCCC", "#99CC00", "#FFCC00", "#FF9900", "#FF6600", "#666699", "#969696", "#003366", "#339966", "#003300", "#333300",
    "#993300", "#993366", "#333399", "#333333", "#000000", "#FFFFFF",
];

/// Slot name -> `theme="N"` index (the order of the TS record).
const SLOTS: [(&str, usize); 12] = [
    ("lt1", 0),
    ("dk1", 1),
    ("lt2", 2),
    ("dk2", 3),
    ("accent1", 4),
    ("accent2", 5),
    ("accent3", 6),
    ("accent4", 7),
    ("accent5", 8),
    ("accent6", 9),
    ("hlink", 10),
    ("folHlink", 11),
];

fn default_theme() -> Theme {
    let colors: Vec<String> = DEFAULT_THEME_COLORS.iter().map(|s| (*s).to_owned()).collect();
    let by_name = SLOTS.iter().map(|&(n, i)| (n.to_owned(), colors[i].clone())).collect();
    Theme { colors, by_name, ..Theme::default() }
}

/// `'#' + s.slice(-6).toUpperCase()`.
fn hex6_upper(s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let tail = &u[u.len().saturating_sub(6)..];
    format!("#{}", String::from_utf16_lossy(tail).to_uppercase())
}

fn scheme_slot_color(slot: El<'_>) -> Option<String> {
    for c in slot.child_elements() {
        if c.name() == "a:srgbClr" {
            return Some(hex6_upper(c.attr("val").unwrap_or("000000")));
        }
        if c.name() == "a:sysClr" {
            return Some(match c.attr("lastClr") {
                Some(last) => hex6_upper(last),
                None => if c.attr("val") == Some("window") { "#FFFFFF" } else { "#000000" }.to_owned(),
            });
        }
    }
    None
}

/// An owned copy of a DOM subtree (`XmlElement`).
pub fn to_xel(e: El<'_>) -> XEl {
    to_xel_with(e, &|n| n.to_owned())
}

/// [`to_xel`] with a renaming of the element names.
pub fn to_xel_with(e: El<'_>, rename: &dyn Fn(&str) -> String) -> XEl {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for a in e.attrs() {
        match attrs.iter_mut().find(|(k, _)| k == a.name) {
            Some(slot) => slot.1 = a.value.to_owned(),
            None => attrs.push((a.name.to_owned(), a.value.to_owned())),
        }
    }
    let children = e
        .children()
        .filter_map(|n| match n {
            Node::Element(c) => Some(XChild::El(to_xel_with(c, rename))),
            Node::Text(t) | Node::CData(t) => Some(XChild::Text(t.to_owned())),
            _ => None,
        })
        .collect();
    XEl { name: rename(e.name()), attrs, children }
}

/// The part of the theme (`themePath`): the canonical name, else the workbook rels' `/theme` target.
pub fn theme_path(pkg: &Package) -> String {
    if matches!(pkg.find("xl/theme/theme1.xml"), Ok(Some(_))) {
        return "xl/theme/theme1.xml".to_owned();
    }
    if let Ok(doc) = pkg.xml_part("xl/_rels/workbook.xml.rels", &SHEET_XML) {
        for rel in doc.root().children_named("Relationship") {
            if rel.attr("Type").unwrap_or("").ends_with("/theme")
                && let Some(t) = rel.attr("Target")
            {
                return super::workbook::resolve_rel_target("xl/workbook.xml", t);
            }
        }
    }
    "xl/theme/theme1.xml".to_owned()
}

/// `readTheme`: the palette; any failure falls back to the Office palette.
pub fn read_theme(pkg: &Package) -> Theme {
    let Ok(doc) = pkg.xml_part(&theme_path(pkg), &SHEET_XML) else { return default_theme() };
    let root = doc.root();
    let Some(theme_els) = root.first_child("a:themeElements") else { return default_theme() };
    let Some(scheme) = theme_els.first_child("a:clrScheme") else { return default_theme() };
    let mut theme = default_theme();
    for slot in scheme.child_elements() {
        let name = slot.name().strip_prefix("a:").unwrap_or_else(|| slot.name());
        let Some(color) = scheme_slot_color(slot) else { continue };
        match theme.by_name.iter_mut().find(|(k, _)| k == name) {
            Some(e) => e.1.clone_from(&color),
            None => theme.by_name.push((name.to_owned(), color.clone())),
        }
        if let Some(&(_, idx)) = SLOTS.iter().find(|(n, _)| *n == name) {
            theme.colors[idx] = color;
        }
    }
    if let Some(fs) = theme_els.first_child("a:fontScheme") {
        let face =
            |k: &str| fs.first_child(k).and_then(|f| f.first_child("a:latin")).and_then(|l| l.attr("typeface")).filter(|s| !s.is_empty());
        theme.major = owned(face("a:majorFont"));
        theme.minor = owned(face("a:minorFont"));
    }
    if let Some(fmt) = theme_els.first_child("a:fmtScheme") {
        if let Some(fills) = fmt.first_child("a:fillStyleLst") {
            theme.fill_styles = Some(fills.child_elements().map(to_xel).collect());
        }
        if let Some(lns) = fmt.first_child("a:lnStyleLst") {
            theme.line_widths = Some(
                lns.children_named("a:ln")
                    .map(|l| {
                        let w = pi(l.attr("w").unwrap_or("0"));
                        (if w.is_nan() { 0.0 } else { w }) / 12700.0
                    })
                    .collect(),
            );
        }
    }
    theme
}

/// `readIndexedColors` over an already parsed `styles.xml`.
pub fn indexed_from(root: Option<El<'_>>) -> Vec<String> {
    let mut out: Vec<String> = INDEXED_COLORS_DEFAULT.iter().map(|s| (*s).to_owned()).collect();
    let Some(idx) = root.and_then(|r| r.first_child("colors")).and_then(|c| c.first_child("indexedColors")) else { return out };
    for (i, rc) in idx.children_named("rgbColor").enumerate() {
        if let Some(rgb) = rc.attr("rgb") {
            let c = hex6_upper(rgb);
            if i < out.len() {
                out[i] = c;
            } else {
                out.resize(i, "#000000".to_owned());
                out.push(c);
            }
        }
    }
    out
}

/// `colorContext(pkg)`: the theme and the effective indexed palette (`styles.xml` parsed once by the caller).
pub fn color_context(pkg: &Package, styles_root: Option<El<'_>>) -> ColorContext {
    ColorContext { theme: read_theme(pkg), indexed: indexed_from(styles_root) }
}

/// `resolveColorEl`: `rgb`/`theme`/`indexed`/`auto` of a `<color>` with its tint; `None` = no color declared.
pub fn resolve_color_el(c: El<'_>, cx: &ColorContext) -> Option<String> {
    resolve_color(&|k| c.attr(k).map(str::to_owned), cx)
}

/// [`resolve_color_el`] over any attribute source.
pub fn resolve_color(attr: &dyn Fn(&str) -> Option<String>, cx: &ColorContext) -> Option<String> {
    let base = if let Some(rgb) = attr("rgb") {
        let u: Vec<u16> = rgb.encode_utf16().collect();
        format!("#{}", String::from_utf16_lossy(&u[u.len().saturating_sub(6)..]))
    } else if let Some(t) = attr("theme") {
        uo_jsnum::conv::js_index(pi(&t), cx.theme.colors.len())
            .and_then(|i| cx.theme.colors.get(i))
            .cloned()
            .unwrap_or_else(|| "#000000".to_owned())
    } else if let Some(t) = attr("indexed") {
        uo_jsnum::conv::js_index(pi(&t), cx.indexed.len()).and_then(|i| cx.indexed.get(i)).cloned().unwrap_or_else(|| "#000000".to_owned())
    } else if matches!(attr("auto").as_deref(), Some("1" | "true")) {
        "#000000".to_owned()
    } else {
        return None;
    };
    Some(match attr("tint") {
        Some(t) => apply_tint(&base, pf(&t)),
        None => base,
    })
}

const HLSMAX: i32 = 240;
const RGBMAX: i32 = 255;

fn rgb_to_hls(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let c_max = r.max(g).max(b);
    let c_min = r.min(g).min(b);
    let l = ((c_max + c_min) * HLSMAX + RGBMAX) / (2 * RGBMAX);
    if c_max == c_min {
        return (HLSMAX * 2 / 3, l, 0);
    }
    let s = if l <= HLSMAX / 2 {
        ((c_max - c_min) * HLSMAX + (c_max + c_min) / 2) / (c_max + c_min)
    } else {
        ((c_max - c_min) * HLSMAX + (2 * RGBMAX - c_max - c_min) / 2) / (2 * RGBMAX - c_max - c_min)
    };
    let d = |x: i32| ((c_max - x) * (HLSMAX / 6) + (c_max - c_min) / 2) / (c_max - c_min);
    let mut h = if r == c_max {
        d(b) - d(g)
    } else if g == c_max {
        HLSMAX / 3 + d(r) - d(b)
    } else {
        2 * HLSMAX / 3 + d(g) - d(r)
    };
    if h < 0 {
        h += HLSMAX;
    }
    if h > HLSMAX {
        h -= HLSMAX;
    }
    (h, l, s)
}

fn hue_to_rgb(n1: i32, n2: i32, mut h: i32) -> i32 {
    if h < 0 {
        h += HLSMAX;
    }
    if h > HLSMAX {
        h -= HLSMAX;
    }
    if h < HLSMAX / 6 {
        return n1 + ((n2 - n1) * h + HLSMAX / 12) / (HLSMAX / 6);
    }
    if h < HLSMAX / 2 {
        return n2;
    }
    if h < HLSMAX * 2 / 3 {
        return n1 + ((n2 - n1) * (HLSMAX * 2 / 3 - h) + HLSMAX / 12) / (HLSMAX / 6);
    }
    n1
}

fn hls_to_rgb(h: i32, l: i32, s: i32) -> (i32, i32, i32) {
    if s == 0 {
        let v = (l * RGBMAX + HLSMAX / 2) / HLSMAX;
        return (v, v, v);
    }
    let m2 = if l <= HLSMAX / 2 { (l * (HLSMAX + s) + HLSMAX / 2) / HLSMAX } else { l + s - (l * s + HLSMAX / 2) / HLSMAX };
    let m1 = 2 * l - m2;
    let ch = |hh: i32| (hue_to_rgb(m1, m2, hh) * RGBMAX + HLSMAX / 2) / HLSMAX;
    (ch(h + HLSMAX / 3), ch(h), ch(h - HLSMAX / 3))
}

/// `Math.round`: halves toward +infinity.
fn round_js(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `applyTint` (`theme.ts:210-221`): the Windows integer HLS conversion (KB Q29240), tint to 4 places, L'
/// truncated. A text without six hex digits is returned unchanged.
pub fn apply_tint(hex: &str, tint: f64) -> String {
    if tint == 0.0 || tint.is_nan() {
        return hex.to_owned();
    }
    let b = hex.as_bytes();
    let six = |s: usize| b.get(s..s + 6).is_some_and(|w| w.iter().all(u8::is_ascii_hexdigit));
    let Some(s) = (0..b.len()).find_map(|i| {
        if b[i] == b'#' && six(i + 1) {
            Some(i + 1)
        } else if six(i) {
            Some(i)
        } else {
            None
        }
    }) else {
        return hex.to_owned();
    };
    let n = i32::from_str_radix(std::str::from_utf8(&b[s..s + 6]).unwrap_or("0"), 16).unwrap_or(0);
    let (h, l, sat) = rgb_to_hls((n >> 16) & 255, (n >> 8) & 255, n & 255);
    let t = round_js(tint * 1e4) / 1e4;
    let lf = f64::from(l);
    let hm = f64::from(HLSMAX);
    let l2 = if t < 0.0 { lf * (1.0 + t) } else { lf * (1.0 - t) + (hm - hm * (1.0 - t)) }.floor();
    let l2 = if l2 < 0.0 {
        0.0
    } else if l2 > hm {
        hm
    } else {
        l2
    };
    let (r, g, bl) = hls_to_rgb(h, uo_jsnum::conv::to_int32(l2), sat);
    let to = |x: i32| format!("{:02x}", x.clamp(0, 255));
    format!("#{}{}{}", to(r), to(g), to(bl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_matches_the_measured_example() {
        // accent1 #156082 + 0.8 = #C0E6F5 (the doc comment of theme.ts)
        assert_eq!(apply_tint("#156082", 0.799_981_688_894_314_4), "#c0e6f5");
        assert_eq!(apply_tint("#123456", 0.0), "#123456");
    }
}
