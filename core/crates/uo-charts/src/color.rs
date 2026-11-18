//! Colour arithmetic of the chart engine: `shared/charts/src/color.ts` (the AUTOMATIC colours: HSL
//! luminance, white mix, DrawingML tint, the Office series shade) and `dml-color.ts` (`dmlColor`, the
//! resolver of a host that has none: the Sheet, whose workbook theme is indexed, without DrawingML).
//!
//! Same operation order as the TS: every channel is an `f64` through the same expressions, `Math.*`
//! through `uo_jsnum::math` (`pow` with the V8 result, NaN-propagating `max`/`min`), and the hex strings
//! as the TS prints them (`toString(16)` of a number that is NaN is `"NaN"`: a hostile `val="x"` modifier
//! reaches `#NANNANNAN`, as the TS gives).
//!
//! JS traps kept (record `uo-charts.md`, "C-1"): `slice(-6)` and `parseInt(…, 16)` over UTF-16 code
//! units; `replace('#', '')` replaces the first `#` only; the `scheme[v]` lookup of a JS object is a
//! plain map lookup here (the TS would also see `Object.prototype` names such as `constructor`: not
//! ported, declared).

use uo_jsnum::conv::{to_int32, to_uint8};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{abs, floor, max, min, pow, round};
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{utf16_len, utf16_slice};
use uo_opc::xml::Element;

use crate::host::ColorSource;
use crate::model::ChartColor;

/// The result of `String(x)` of a colour channel.
type Rgb = [f64; 3];

/// `hexToRgb(h)` (color.ts:3): `parseInt` of the six hex digits (three are doubled), NaN as 0 in the
/// bit operations.
pub fn hex_to_rgb(h: &str) -> Rgb {
    // `h.replace('#', '')`: the first `#` only
    let s = match h.find('#') {
        Some(i) => format!("{}{}", h.get(..i).unwrap_or(""), h.get(i + 1..).unwrap_or("")),
        None => h.to_owned(),
    };
    let n = if utf16_len(&s) == 3 {
        // `s.split('').map((c) => c + c).join('')`: each code unit twice
        let doubled: String = s.chars().flat_map(|c| [c, c]).collect();
        parse_int(&utf16_slice(&doubled, 0, 6), 16)
    } else {
        parse_int(&utf16_slice(&s, 0, 6), 16)
    };
    let n = to_int32(n);
    [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)]
}

/// `Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')`.
fn hex_channel(v: f64) -> String {
    let c = max(0.0, min(255.0, round(v)));
    if c.is_nan() { "NaN".to_owned() } else { format!("{:02x}", to_uint8(c)) }
}

/// `rgbToHex(r, g, b)` (color.ts:9): `#RRGGBB` in upper case.
pub fn rgb_to_hex(r: f64, g: f64, b: f64) -> String {
    let s = format!("#{}{}{}", hex_channel(r), hex_channel(g), hex_channel(b));
    uo_text::case::to_uppercase_str(&s)
}

/// `rgbToHsl` (color.ts:14).
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> [f64; 3] {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let mx = max(max(r, g), b);
    let mn = min(min(r, g), b);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let mut h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    [h, s, l]
}

/// `hslToRgb` (color.ts:25): channels in 0–255, not rounded.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    if s == 0.0 {
        return [l * 255.0, l * 255.0, l * 255.0];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };
    [f(h + 1.0 / 3.0) * 255.0, f(h) * 255.0, f(h - 1.0 / 3.0) * 255.0]
}

/// `adjustLum(hex, f)` (color.ts:40): HSL luminance, `f < 0` darkens by a fraction, `f > 0` lightens
/// towards white.
pub fn adjust_lum(hex: &str, f: f64) -> String {
    let [r, g, b] = hex_to_rgb(hex);
    let [h, s, l] = rgb_to_hsl(r, g, b);
    let l2 = if f < 0.0 { l * (1.0 + f) } else { l + (1.0 - l) * f };
    let [r2, g2, b2] = hsl_to_rgb(h, s, l2);
    rgb_to_hex(r2, g2, b2)
}

/// `mixWhite(hex, t)` (color.ts:47): linear mix with white (`t = 1` is white).
pub fn mix_white(hex: &str, t: f64) -> String {
    let [r, g, b] = hex_to_rgb(hex);
    rgb_to_hex(r + (255.0 - r) * t, g + (255.0 - g) * t, b + (255.0 - b) * t)
}

/// `toLin` (color.ts:67).
fn to_lin(c: f64) -> f64 {
    let v = c / 255.0;
    if v <= 0.04045 { v / 12.92 } else { pow((v + 0.055) / 1.055, 2.4) }
}

/// `toSrgb` (color.ts:68).
fn to_srgb(v: f64) -> f64 {
    255.0 * if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * pow(v, 1.0 / 2.4) - 0.055 }
}

/// `tintLinear(hex, keep)` (color.ts:54): the DrawingML `a:tint` in linear RGB; `keep` = fraction of the
/// colour that stays.
pub fn tint_linear(hex: &str, keep: f64) -> String {
    let [r, g, b] = hex_to_rgb(hex);
    let f = |v: f64| to_srgb(to_lin(v) * keep + (1.0 - keep));
    rgb_to_hex(f(r), f(g), f(b))
}

/// `rgba(hex, alpha)` (color.ts:62): `hex` itself when opaque, else `rgba(r,g,b,a)` with the JS
/// number printing.
pub fn rgba(hex: &str, alpha: f64) -> String {
    if alpha >= 1.0 {
        return hex.to_owned();
    }
    let [r, g, b] = hex_to_rgb(hex);
    format!("rgba({},{},{},{})", to_js_string(r), to_js_string(g), to_js_string(b), to_js_string(alpha))
}

/// `chartShadeTint(hex, f)` (color.ts:76): the automatic variation of Office, in LINEAR RGB with weight
/// about `1.41·|f|` (measured).
pub fn chart_shade_tint(hex: &str, f: f64) -> String {
    if f == 0.0 {
        return hex.to_owned();
    }
    let k = min(1.0, abs(f) * 1.41);
    let [r, g, b] = hex_to_rgb(hex).map(to_lin);
    let m = |v: f64| if f < 0.0 { v * (1.0 - k) } else { v + (1.0 - v) * k };
    rgb_to_hex(to_srgb(m(r)), to_srgb(m(g)), to_srgb(m(b)))
}

/// The theme colours by name (`Record<string, string>` of `dmlColor`): `dk1`, `lt1`, `accent1`… (the
/// aliases `tx1`/`bg1`/`tx2`/`bg2` fall back to the standard clrMap). A later `set` of a name replaces.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Scheme {
    entries: Vec<(Box<str>, Box<str>)>,
}

impl Scheme {
    /// An empty scheme.
    pub fn new() -> Scheme {
        Scheme::default()
    }

    /// `scheme[name] = value`.
    pub fn set(&mut self, name: &str, value: &str) {
        match self.entries.iter_mut().find(|(k, _)| &**k == name) {
            Some(slot) => slot.1 = value.into(),
            None => self.entries.push((name.into(), value.into())),
        }
    }

    /// `scheme[name]`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| &**k == name).map(|(_, v)| &**v)
    }

    /// A scheme from `(name, value)` pairs.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Scheme {
        let mut s = Scheme::new();
        for (k, v) in pairs {
            s.set(k, v);
        }
        s
    }
}

/// `STD_MAP` (dml-color.ts:15): the standard clrMap aliases.
fn std_map(v: &str) -> Option<&'static str> {
    match v {
        "tx1" => Some("dk1"),
        "bg1" => Some("lt1"),
        "tx2" => Some("dk2"),
        "bg2" => Some("lt2"),
        _ => None,
    }
}

/// `PRESET` (dml-color.ts:17).
fn preset(v: &str) -> Option<&'static str> {
    Some(match v {
        "black" => "#000000",
        "white" => "#FFFFFF",
        "red" => "#FF0000",
        "green" => "#008000",
        "blue" => "#0000FF",
        "yellow" => "#FFFF00",
        "gray" | "grey" => "#808080",
        "darkGray" => "#A9A9A9",
        "lightGray" => "#D3D3D3",
        _ => return None,
    })
}

/// `clamp01` (dml-color.ts:74).
fn clamp01(v: f64) -> f64 {
    max(0.0, min(1.0, v))
}

/// `srgbToLin` (dml-color.ts:75).
fn srgb_to_lin(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { pow((v + 0.055) / 1.055, 2.4) }
}

/// `linToSrgb` (dml-color.ts:76).
fn lin_to_srgb(v: f64) -> f64 {
    let v = clamp01(v);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * pow(v, 1.0 / 2.4) - 0.055 }
}

/// `rgbF` (dml-color.ts:78): the first run of six hex digits of the string (`/#?([0-9a-fA-F]{6})/`),
/// as fractions; no run is black.
fn rgb_f(h: &str) -> Rgb {
    let b = h.as_bytes();
    let run = (0..b.len()).find(|&i| b.get(i..i + 6).is_some_and(|w| w.iter().all(u8::is_ascii_hexdigit)));
    let Some(i) = run else { return [0.0, 0.0, 0.0] };
    let n = parse_int(h.get(i..i + 6).unwrap_or("0"), 16);
    let n = to_int32(n);
    [f64::from((n >> 16) & 255) / 255.0, f64::from((n >> 8) & 255) / 255.0, f64::from(n & 255) / 255.0]
}

/// `hex(r, g, b)` (dml-color.ts:84): arguments in 0–255, `#rrggbb` lower case.
fn hex_lower(r: f64, g: f64, b: f64) -> String {
    let c = |x: f64| {
        let v = round(min(255.0, max(0.0, x)));
        if v.is_nan() { "NaN".to_owned() } else { format!("{:02x}", to_uint8(v)) }
    };
    format!("#{}{}{}", c(r), c(g), c(b))
}

/// `hsl(c, f)` (dml-color.ts:90): an adjustment in HSL (hue cyclic, saturation without ceiling: the
/// resulting RGB is clipped to 0–1).
fn hsl(c: Rgb, f: impl FnOnce(f64, f64, f64) -> [f64; 3]) -> Rgb {
    let [rf, gf, bf] = c;
    let mx = max(max(rf, gf), bf);
    let mn = min(min(rf, gf), bf);
    let d = mx - mn;
    let (mut h, mut s) = (0.0, 0.0);
    let l = (mx + mn) / 2.0;
    if d != 0.0 {
        s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
        if mx == rf {
            h = ((gf - bf) / d) % 6.0;
        } else if mx == gf {
            h = (bf - rf) / d + 2.0;
        } else {
            h = (rf - gf) / d + 4.0;
        }
        h /= 6.0;
        if h < 0.0 {
            h += 1.0;
        }
    }
    let [h2, s2, l2] = f(h, s, l);
    let h2 = ((h2 % 1.0) + 1.0) % 1.0;
    let l2 = clamp01(l2);
    let k = (1.0 - abs(2.0 * l2 - 1.0)) * s2;
    let x = k * (1.0 - abs(((h2 * 6.0) % 2.0) - 1.0));
    let m = l2 - k / 2.0;
    let seg = floor(h2 * 6.0) % 6.0;
    let [r2, g2, b2] = if seg == 0.0 {
        [k, x, 0.0]
    } else if seg == 1.0 {
        [x, k, 0.0]
    } else if seg == 2.0 {
        [0.0, k, x]
    } else if seg == 3.0 {
        [0.0, x, k]
    } else if seg == 4.0 {
        [x, 0.0, k]
    } else {
        [k, 0.0, x]
    };
    [clamp01(r2 + m), clamp01(g2 + m), clamp01(b2 + m)]
}

/// The elements a colour modifier may be (`a:alpha*` never count as "touched").
fn is_alpha_mod(name: &str) -> bool {
    matches!(name, "a:alpha" | "a:alphaMod" | "a:alphaOff")
}

/// `dmlColor(el, scheme)` (dml-color.ts:22) of the synthetic node the engine builds,
/// `<a:schemeClr val="…"/>` without modifiers: the scheme colour as is, in upper case.
fn scheme_color(v: &str, scheme: &Scheme) -> String {
    let base = scheme.get(v).or_else(|| scheme.get(std_map(v).unwrap_or(v))).unwrap_or("#000000");
    uo_text::case::to_uppercase_str(base)
}

/// `dmlColor` (dml-color.ts:22): a DrawingML colour element (`a:srgbClr`, `a:schemeClr`, `a:sysClr`,
/// `a:prstClr` and their modifiers) → `#RRGGBB`, with the Slides math measured in PowerPoint: shade/tint in
/// linear RGB (the exact sRGB curve; `val` = fraction that stays), `lumMod`/`lumOff` in HSL luminance,
/// `sat`/`hue` in HSL without ceiling. `None` for any other element.
pub fn dml_color(el: Element<'_>, scheme: &Scheme) -> Option<ChartColor> {
    dml_color_with(el, scheme, true)
}

/// [`dml_color`] with the `a:sysClr` rule chosen: `ts_sys_clr` = `lastClr` (the TS, what [`dml_color`] does); else the
/// system colour PowerPoint and Excel draw ([`system_color`]; `lastClr` only outside the list).
pub fn dml_color_with(el: Element<'_>, scheme: &Scheme, ts_sys_clr: bool) -> Option<ChartColor> {
    dml_color_string(el, scheme, ts_sys_clr).map(|s| ChartColor::new(&s))
}

/// The stock Windows `GetSysColor` answers PowerPoint draws for `a:sysClr` (measured in wave 5 by the Slides theme frontier:
/// `uo_dml_render::theme::system_color`, the same table — `tests/w5_rules.rs` checks they stay equal; this crate cannot
/// depend on uo-dml-render).
pub const SYSTEM_COLORS: [(&str, &str); 30] = [
    ("scrollBar", "C8C8C8"),
    ("background", "000000"),
    ("activeCaption", "99B4D1"),
    ("inactiveCaption", "BFCDDB"),
    ("menu", "F0F0F0"),
    ("window", "FFFFFF"),
    ("windowFrame", "646464"),
    ("menuText", "000000"),
    ("windowText", "000000"),
    ("captionText", "000000"),
    ("activeBorder", "B4B4B4"),
    ("inactiveBorder", "F4F7FC"),
    ("appWorkspace", "ABABAB"),
    ("highlight", "0078D7"),
    ("highlightText", "FFFFFF"),
    ("btnFace", "F0F0F0"),
    ("btnShadow", "A0A0A0"),
    ("grayText", "6D6D6D"),
    ("btnText", "000000"),
    ("inactiveCaptionText", "000000"),
    ("btnHighlight", "FFFFFF"),
    ("3dDkShadow", "696969"),
    ("3dLight", "E3E3E3"),
    ("infoText", "000000"),
    ("infoBk", "FFFFE1"),
    ("hotLight", "0066CC"),
    ("gradientActiveCaption", "B9D1EA"),
    ("gradientInactiveCaption", "D7E4F2"),
    ("menuHighlight", "3399FF"),
    ("menuBar", "F0F0F0"),
];

/// The system colour (`RRGGBB`) of an `a:sysClr val` (`None` outside `ST_SystemColorVal`).
#[must_use]
pub fn system_color(val: &str) -> Option<&'static str> {
    SYSTEM_COLORS.iter().find(|(k, _)| *k == val).map(|(_, v)| *v)
}

/// [`dml_color`] over what the parser asks the resolver for ([`ColorSource`]): the synthetic scheme
/// colour included.
pub fn dml_color_source(src: ColorSource<'_>, scheme: &Scheme) -> Option<ChartColor> {
    dml_color_source_with(src, scheme, true)
}

/// [`dml_color_source`] with the `a:sysClr` rule chosen (see [`dml_color_with`]).
pub fn dml_color_source_with(src: ColorSource<'_>, scheme: &Scheme, ts_sys_clr: bool) -> Option<ChartColor> {
    match src {
        ColorSource::Element(el) => dml_color_with(el, scheme, ts_sys_clr),
        ColorSource::Scheme(v) => Some(ChartColor::new(&scheme_color(v, scheme))),
    }
}

fn dml_color_string(el: Element<'_>, scheme: &Scheme, ts_sys_clr: bool) -> Option<String> {
    let base: String = match el.name() {
        "a:srgbClr" => format!("#{}", utf16_slice(el.attr("val").unwrap_or("000000"), -6, i64::MAX)),
        "a:sysClr" if !ts_sys_clr && el.attr("val").and_then(system_color).is_some() => {
            format!("#{}", el.attr("val").and_then(system_color).unwrap_or("000000"))
        }
        "a:sysClr" => {
            let last = el.attr("lastClr").unwrap_or(if el.attr("val") == Some("window") { "FFFFFF" } else { "000000" });
            format!("#{last}")
        }
        "a:prstClr" => preset(el.attr("val").unwrap_or("black")).unwrap_or("#000000").to_owned(),
        "a:schemeClr" => {
            let v = el.attr("val").unwrap_or("");
            scheme.get(v).or_else(|| scheme.get(std_map(v).unwrap_or(v))).unwrap_or("#000000").to_owned()
        }
        _ => return None,
    };
    // in floating point, in document order; `<a:gamma/>` … `<a:invGamma/>`: shade/tint in sRGB
    let mut c = rgb_f(&base);
    let mut srgb_mode = false;
    let mut touched = false;
    for m in el.child_elements() {
        let name = m.name();
        if name == "a:gamma" {
            srgb_mode = true;
            continue;
        }
        if name == "a:invGamma" {
            srgb_mode = false;
            continue;
        }
        if !is_alpha_mod(name) {
            touched = true;
        }
        if name == "a:comp" {
            c = hsl(c, |h, s, l| [h + 0.5, s, l]);
            continue;
        }
        if name == "a:inv" {
            c = [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]];
            continue;
        }
        if name == "a:gray" {
            // Rec. 709 (measured)
            let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            c = [y, y, y];
            continue;
        }
        let Some(raw) = m.attr("val") else { continue };
        let val = parse_int(raw, 10) / 100_000.0;
        match name {
            "a:shade" => c = c.map(|v| if srgb_mode { v * val } else { lin_to_srgb(srgb_to_lin(v) * val) }),
            "a:tint" => {
                c = c.map(|v| if srgb_mode { v * val + (1.0 - val) } else { lin_to_srgb(srgb_to_lin(v) * val + (1.0 - val)) });
            }
            "a:lumMod" => c = hsl(c, |h, s, l| [h, s, clamp01(l * val)]),
            "a:lumOff" => c = hsl(c, |h, s, l| [h, s, clamp01(l + val)]),
            "a:lum" => c = hsl(c, |h, s, _| [h, s, clamp01(val)]),
            "a:satMod" => c = hsl(c, |h, s, l| [h, max(0.0, s * val), l]),
            "a:satOff" => c = hsl(c, |h, s, l| [h, max(0.0, s + val), l]),
            "a:sat" => c = hsl(c, |h, _, l| [h, max(0.0, val), l]),
            "a:hueMod" => c = hsl(c, |h, s, l| [h * val, s, l]),
            "a:hueOff" => c = hsl(c, |h, s, l| [h + (val * 100_000.0) / 21_600_000.0, s, l]),
            "a:hue" => c = hsl(c, |_, s, l| [(val * 100_000.0) / 21_600_000.0, s, l]),
            _ => {}
        }
    }
    let out = if touched { hex_lower(c[0] * 255.0, c[1] * 255.0, c[2] * 255.0) } else { base };
    Some(uo_text::case::to_uppercase_str(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_colours_match_the_measured_values() {
        // color.ts comments: tx1 tint 75% → #898989; f = ±1/6, ±1/4 of the series shade
        assert_eq!(tint_linear("#000000", 0.75), "#898989");
        assert_eq!(rgb_to_hex(0.0, 300.0, -4.0), "#00FF00");
        assert_eq!(rgb_to_hex(f64::NAN, 1.0, 2.0), "#NAN0102");
        assert_eq!(hex_to_rgb("#abc"), [170.0, 187.0, 204.0]);
        assert_eq!(hex_to_rgb("#4472C4"), [68.0, 114.0, 196.0]);
        assert_eq!(hex_to_rgb("zz"), [0.0, 0.0, 0.0]);
        assert_eq!(mix_white("#000000", 0.5), "#808080");
        assert_eq!(rgba("#FF8000", 0.5), "rgba(255,128,0,0.5)");
        assert_eq!(rgba("#FF8000", 1.0), "#FF8000");
        assert_eq!(chart_shade_tint("#4472C4", 0.0), "#4472C4");
        assert_eq!(adjust_lum("#808080", 0.0), "#808080");
        assert_eq!(adjust_lum("#FFFFFF", -0.5), "#808080");
    }
}
