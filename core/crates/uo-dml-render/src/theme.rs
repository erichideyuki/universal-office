//! The DrawingML theme as the Slides reader keeps it (`Theme`, `model.ts:540`), read by
//! `readTheme` (`theme.ts:9`) and specialised per part by `applyClrMap` (`theme.ts:66`).
//!
//! The colour resolution of this crate is **the PowerPoint profile** of `colorFromDml`
//! (`theme.ts:85`): shade/tint in RGB linear (sRGB between `<a:gamma/>` and `<a:invGamma/>`),
//! lum/sat/hue in HSL without a ceiling, `comp`/`inv`/`gray`, and an untouched colour keeps the
//! file's spelling. It differs from Word's HSL tint (uo-drawingml `dml`) and Excel's; it is a named
//! profile here, not a branch in `uo-drawingml` (frontier B1a).

use std::sync::Arc;

use uo_jsnum::math;
use uo_opc::xml::{Document, Element, ElementId};

use crate::color::CssColor;
use crate::source::{PartSource, R, int_attr, pint};

/// A JS `Record<string, V>`: insertion order with overwrite in place (`obj[k] = v`). The keys of
/// the records of this crate (colour names, clrMap attributes) are never integer-like in a real
/// file; the canonical JSON sorts keys anyway.
#[derive(Clone, PartialEq, Debug)]
pub struct JsRecord<V> {
    pub entries: Vec<(Box<str>, V)>,
}

impl<V> Default for JsRecord<V> {
    fn default() -> Self {
        JsRecord { entries: Vec::new() }
    }
}

impl<V> JsRecord<V> {
    /// `obj[key]`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| &**k == key).map(|(_, v)| v)
    }

    /// `obj[key] = v` (an existing key keeps its position).
    pub fn set(&mut self, key: &str, v: V) {
        match self.entries.iter_mut().find(|(k, _)| &**k == key) {
            Some(e) => e.1 = v,
            None => self.entries.push((key.into(), v)),
        }
    }

    /// `Object.keys(obj).length`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `Object.keys(obj).length === 0`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `Object.entries(obj)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (&**k, v))
    }
}

/// `+mj-lt` / `+mn-lt` faces of the font scheme (`Theme.fonts`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ThemeFonts {
    pub major: Option<Box<str>>,
    pub minor: Option<Box<str>>,
}

/// The theme part's style lists: the TS keeps the `XmlElement`s themselves (`model.ts:552–555`);
/// here they are element ids into the theme part's parsed document (shared with the parse cache).
/// Each list is optional on its own (`fillStyles?`, `bgFillStyles?`, `effectStyles?`).
#[derive(Clone, Debug)]
pub struct ThemeStyles {
    pub doc: Arc<Document>,
    /// `<a:fillStyleLst>` children (resolve `fillRef idx` 1…).
    pub fill: Option<Vec<ElementId>>,
    /// `<a:bgFillStyleLst>` children (resolve `bgRef idx` 1001…).
    pub bg_fill: Option<Vec<ElementId>>,
    /// `<a:effectStyleLst>` `effectStyle` children (resolve `effectRef idx`).
    pub effect: Option<Vec<ElementId>>,
}

impl PartialEq for ThemeStyles {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.doc, &other.doc) && self.fill == other.fill && self.bg_fill == other.bg_fill && self.effect == other.effect
    }
}

impl ThemeStyles {
    /// `theme.fillStyles?.[i]`.
    pub fn fill_style(&self, i: usize) -> Option<Element<'_>> {
        self.fill.as_ref()?.get(i).and_then(|&id| self.doc.element(id))
    }
    /// `theme.bgFillStyles?.[i]`.
    pub fn bg_fill_style(&self, i: usize) -> Option<Element<'_>> {
        self.bg_fill.as_ref()?.get(i).and_then(|&id| self.doc.element(id))
    }
    /// `theme.effectStyles?.[i]`.
    pub fn effect_style(&self, i: usize) -> Option<Element<'_>> {
        self.effect.as_ref()?.get(i).and_then(|&id| self.doc.element(id))
    }
}

/// The colour map of a theme plus its clrMap (`model.ts:540`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Theme {
    /// Colour name (`dk1`, `accent1`, `tx1`, `bg1`, …) → colour, after the clrMap.
    pub colors: JsRecord<CssColor>,
    /// The raw `<a:clrScheme>` before the clrMap (base of `applyClrMap`).
    pub scheme: Option<JsRecord<CssColor>>,
    pub fonts: Option<ThemeFonts>,
    /// The applied clrMap (alias → scheme name), re-applied to chart themes (`themeOverride`).
    pub clr_map: Option<JsRecord<Box<str>>>,
    /// `<a:lnStyleLst>` widths in EMU (resolve `lnRef idx`).
    pub line_widths: Option<Vec<f64>>,
    pub styles: Option<ThemeStyles>,
    /// The theme part (the `r:embed`s of its image styles resolve through its rels).
    pub part: Option<Box<str>>,
}

/// clrMap default (master): aliases tx/bg → dk/lt (`STD_MAP`).
const STD_MAP: [(&str, &str); 4] = [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")];

fn std_map(v: &str) -> Option<&'static str> {
    STD_MAP.iter().find(|(a, _)| *a == v).map(|(_, s)| *s)
}

/// `readTheme` (`theme.ts:9`): `ppt/theme/themeN.xml` → colours, line widths, style lists, fonts.
pub fn read_theme(src: &dyn PartSource, theme_path: &str) -> R<Theme> {
    let mut colors: JsRecord<CssColor> = JsRecord::default();
    let Some(doc) = src.xml(theme_path)? else { return Ok(Theme { colors, ..Theme::default() }) };
    let root = doc.root();
    // normal theme or a chart's `<a:themeOverride>` (clrScheme/fontScheme straight under the root)
    let theme_els = root.first_child("a:themeElements").or(if root.name() == "a:themeOverride" { Some(root) } else { None });
    let scheme = theme_els.and_then(|t| t.first_child("a:clrScheme"));
    if let Some(scheme) = scheme {
        for el in scheme.child_elements() {
            let name = el.name();
            let key = name.strip_prefix("a:").unwrap_or(name);
            let c = color_in(el.child_elements().next(), &colors);
            if let Some(c) = c {
                colors.set(key, c);
            }
        }
    }
    let raw = colors.clone();
    // aliases (tx1 = dk1, bg1 = lt1, …) for schemeClr values that use bg/tx; the REAL clrMap of the
    // master/slide enters later (applyClrMap)
    for (alias, s) in STD_MAP {
        if let Some(c) = colors.get(s).cloned() {
            colors.set(alias, c);
        }
    }
    let mut theme = Theme { colors, scheme: Some(raw), part: Some(theme_path.into()), ..Theme::default() };
    // fmtScheme > lnStyleLst: the widths `<a:lnRef idx>` points at
    let fmt = theme_els.and_then(|t| t.first_child("a:fmtScheme"));
    if let Some(ln_list) = fmt.and_then(|f| f.first_child("a:lnStyleLst")) {
        let widths: Vec<f64> = ln_list.children_named("a:ln").map(|ln| ln.attr("w").map_or(9525.0, pint)).collect();
        if !widths.is_empty() {
            theme.line_widths = Some(widths);
        }
    }
    let ids = |list: Option<Element<'_>>, name: Option<&str>| -> Option<Vec<ElementId>> {
        let l = list?;
        Some(match name {
            Some(n) => l.children_named(n).map(|e| e.id()).collect(),
            None => l.child_elements().map(|e| e.id()).collect(),
        })
    };
    let fill = ids(fmt.and_then(|f| f.first_child("a:fillStyleLst")), None);
    let bg_fill = ids(fmt.and_then(|f| f.first_child("a:bgFillStyleLst")), None);
    let effect = ids(fmt.and_then(|f| f.first_child("a:effectStyleLst")), Some("a:effectStyle"));
    if fill.is_some() || bg_fill.is_some() || effect.is_some() {
        theme.styles = Some(ThemeStyles { doc: Arc::clone(&doc), fill, bg_fill, effect });
    }
    // fontScheme: major/minor typography to resolve "+mj-lt" / "+mn-lt"
    if let Some(font_scheme) = theme_els.and_then(|t| t.first_child("a:fontScheme")) {
        let face = |n: &str| -> Option<Box<str>> {
            let f = font_scheme.first_child(n)?;
            f.first_child("a:latin")?.attr("typeface").map(Into::into)
        };
        let (major, minor) = (face("a:majorFont"), face("a:minorFont"));
        let truthy = |o: &Option<Box<str>>| o.as_ref().is_some_and(|s| !s.is_empty());
        if truthy(&major) || truthy(&minor) {
            theme.fonts = Some(ThemeFonts { major: major.filter(|s| !s.is_empty()), minor: minor.filter(|s| !s.is_empty()) });
        }
    }
    Ok(theme)
}

/// `applyClrMap` (`theme.ts:66`): applies a `<p:clrMap>` (bg1/tx1/accent1… → scheme name). A dark
/// theme is `bg1="dk1" tx1="lt1"`. Returns a NEW theme (the master's stays intact).
pub fn apply_clr_map(theme: Theme, map: Option<&JsRecord<Box<str>>>) -> Theme {
    let Some(map) = map else { return theme };
    let Some(scheme) = theme.scheme.as_ref() else { return theme };
    let mut colors = theme.colors.clone();
    for (alias, s) in map.iter() {
        if let Some(c) = scheme.get(s) {
            colors.set(alias, c.clone());
        }
    }
    Theme { colors, clr_map: Some(map.clone()), ..theme }
}

/// `clrMapAttrs` (`theme.ts:78`): attributes of a `<p:clrMap>` / `<a:overrideClrMapping>` → alias →
/// scheme colour name; `None` when there is none.
pub fn clr_map_attrs(el: Option<Element<'_>>) -> Option<JsRecord<Box<str>>> {
    let el = el?;
    let mut out = JsRecord::default();
    for a in el.attrs() {
        if !a.name.contains(':') && !a.value.is_empty() {
            out.set(a.name, a.value.into());
        }
    }
    (!out.is_empty()).then_some(out)
}

type Rgb = [f64; 3];

fn clamp01(v: f64) -> f64 {
    math::max(0.0, math::min(1.0, v))
}

/// The leftmost six hex digits of `s` (`/#?([0-9a-fA-F]{6})/`), as the JS `rgb()` reads them.
fn rgb_bytes(s: &str) -> [f64; 3] {
    let b = s.as_bytes();
    let is_hex = |c: &u8| c.is_ascii_hexdigit();
    let mut i = 0;
    while i + 6 <= b.len() {
        if b.get(i..i + 6).is_some_and(|w| w.iter().all(is_hex)) {
            let mut n: u32 = 0;
            for &c in b.get(i..i + 6).unwrap_or(&[]) {
                n = n * 16 + (char::from(c).to_digit(16).unwrap_or(0));
            }
            return [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)];
        }
        i += 1;
    }
    [0.0, 0.0, 0.0]
}

fn rgb_f(hex_in: &str) -> Rgb {
    let [r, g, b] = rgb_bytes(hex_in);
    [r / 255.0, g / 255.0, b / 255.0]
}

/// sRGB → linear (IEC 61966-2-1 exact curve).
fn srgb_to_lin(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { math::pow((v + 0.055) / 1.055, 2.4) }
}

fn lin_to_srgb(v: f64) -> f64 {
    let v = clamp01(v);
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * math::pow(v, 1.0 / 2.4) - 0.055 }
}

/// Adjust in HSL (h 0..1 cyclic; s without a ceiling — the resulting RGB is clipped to 0..1).
fn hsl_f(c: Rgb, f: impl Fn(f64, f64, f64) -> [f64; 3]) -> Rgb {
    let [rf, gf, bf] = c;
    let max = math::max(math::max(rf, gf), bf);
    let min = math::min(math::min(rf, gf), bf);
    let d = max - min;
    let mut h = 0.0;
    let mut s = 0.0;
    let l = (max + min) / 2.0;
    if d != 0.0 {
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        h = if max == rf {
            math::rem((gf - bf) / d, 6.0)
        } else if max == gf {
            (bf - rf) / d + 2.0
        } else {
            (rf - gf) / d + 4.0
        };
        h /= 6.0;
        if h < 0.0 {
            h += 1.0;
        }
    }
    let [h0, s2, l0] = f(h, s, l);
    let h2 = math::rem(math::rem(h0, 1.0) + 1.0, 1.0);
    let l2 = clamp01(l0);
    let k = (1.0 - math::abs(2.0 * l2 - 1.0)) * s2;
    let x = k * (1.0 - math::abs(math::rem(h2 * 6.0, 2.0) - 1.0));
    let m = l2 - k / 2.0;
    let seg = math::rem(math::floor(h2 * 6.0), 6.0);
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

/// `#rrggbb` the way `hex()` writes it: each channel clipped, rounded, base 16, two digits (NaN
/// prints `NaN`, which the JS `padStart` leaves alone).
fn hex(r: f64, g: f64, b: f64) -> String {
    let mut out = String::from("#");
    for x in [r, g, b] {
        let v = math::round(math::min(255.0, math::max(0.0, x)));
        if v.is_nan() {
            out.push_str("NaN");
        } else {
            out.push_str(&format!("{:02x}", uo_jsnum::conv::to_uint8(v)));
        }
    }
    out
}

fn preset_color(name: &str) -> Option<&'static str> {
    Some(match name {
        "black" => "#000000",
        "white" => "#FFFFFF",
        "red" => "#FF0000",
        "green" => "#008000",
        "blue" => "#0000FF",
        "yellow" => "#FFFF00",
        "gray" | "grey" => "#808080",
        _ => return None,
    })
}

/// `colorFromDml` over a bare colour table: the only part of the theme the resolution reads.
pub(crate) fn color_in(el: Option<Element<'_>>, colors: &JsRecord<CssColor>) -> Option<CssColor> {
    let el = el?;
    let name = el.name();
    let base: String = match name {
        "a:srgbClr" => format!("#{}", el.attr("val").unwrap_or("000000")),
        "a:sysClr" => format!("#{}", el.attr("lastClr").unwrap_or("000000")),
        "a:prstClr" => preset_color(el.attr("val").unwrap_or("black")).unwrap_or("#000000").to_string(),
        "a:schemeClr" => {
            let v = el.attr("val").unwrap_or("");
            match colors.get(v).or_else(|| colors.get(std_map(v).unwrap_or(v))) {
                Some(c) => c.as_str().to_string(),
                None => "#000000".to_string(),
            }
        }
        _ => return None,
    };
    // modifiers in FLOATING POINT (no rounding to 8 bits between one and the next); between
    // `<a:gamma/>` and `<a:invGamma/>` the shade/tint operate in sRGB
    let mut c = rgb_f(&base);
    let mut srgb_mode = false;
    let mut touched = false;
    for m in el.child_elements() {
        let mn = m.name();
        if mn != "a:alpha" && mn != "a:alphaMod" && mn != "a:alphaOff" {
            touched = true;
        }
        match mn {
            "a:gamma" => {
                srgb_mode = true;
                continue;
            }
            "a:invGamma" => {
                srgb_mode = false;
                continue;
            }
            "a:comp" => {
                c = hsl_f(c, |h, s, l| [h + 0.5, s, l]);
                continue;
            }
            "a:inv" => {
                c = [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]];
                continue;
            }
            "a:gray" => {
                let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                c = [y, y, y];
                continue;
            }
            _ => {}
        }
        let Some(val) = int_attr(m, "val").map(|v| v / 100000.0) else { continue };
        match mn {
            "a:shade" => c = c.map(|v| if srgb_mode { v * val } else { lin_to_srgb(srgb_to_lin(v) * val) }),
            "a:tint" => c = c.map(|v| if srgb_mode { v * val + (1.0 - val) } else { lin_to_srgb(srgb_to_lin(v) * val + (1.0 - val)) }),
            "a:lumMod" => c = hsl_f(c, |h, s, l| [h, s, clamp01(l * val)]),
            "a:lumOff" => c = hsl_f(c, |h, s, l| [h, s, clamp01(l + val)]),
            "a:lum" => c = hsl_f(c, |h, s, _| [h, s, clamp01(val)]),
            "a:satMod" => c = hsl_f(c, |h, s, l| [h, math::max(0.0, s * val), l]),
            "a:satOff" => c = hsl_f(c, |h, s, l| [h, math::max(0.0, s + val), l]),
            "a:sat" => c = hsl_f(c, |h, _, l| [h, math::max(0.0, val), l]),
            "a:hueMod" => c = hsl_f(c, |h, s, l| [h * val, s, l]),
            "a:hueOff" => c = hsl_f(c, |h, s, l| [h + (val * 100000.0) / 21600000.0, s, l]),
            "a:hue" => c = hsl_f(c, |_, s, l| [(val * 100000.0) / 21600000.0, s, l]),
            _ => {}
        }
    }
    // no colour modifier: the colour as it came (original spelling)
    Some(if touched { CssColor::new(&hex(c[0] * 255.0, c[1] * 255.0, c[2] * 255.0)) } else { CssColor::new(&base) })
}

/// `colorFromDml` (`theme.ts:85`): a DrawingML colour element (srgbClr/schemeClr/sysClr/prstClr) →
/// `#RRGGBB` with its modifiers applied; `None` for anything else.
pub fn color_from_dml(el: Option<Element<'_>>, theme: &Theme) -> Option<CssColor> {
    color_in(el, &theme.colors)
}

/// Text colour of an `rPr`/`defRPr`: `<a:solidFill>`; with `<a:gradFill>` (2007–2013 theme titles)
/// the colour of the middle stop (`textFillColor`, `theme.ts:166`).
pub fn text_fill_color(parent: Element<'_>, theme: &Theme) -> Option<CssColor> {
    if let Some(c) = solid_fill_color(parent, theme) {
        return Some(c);
    }
    let gf = parent.first_child("a:gradFill")?;
    let gs = gf.first_child("a:gsLst")?;
    let stops: Vec<Element<'_>> = gs.children_named("a:gs").collect();
    let mid = stops.get(stops.len() / 2)?;
    color_from_dml(mid.child_elements().next(), theme)
}

/// The `<a:solidFill>` inside `parent`, resolved (`solidFillColor`, `theme.ts:178`).
pub fn solid_fill_color(parent: Element<'_>, theme: &Theme) -> Option<CssColor> {
    let sf = parent.first_child("a:solidFill")?;
    color_from_dml(sf.child_elements().next(), theme)
}

/// `lumHsl` (`theme.ts:188`): luminance transform L' = L·mod + off (hue and saturation kept), for
/// the shading of 3D faces.
pub fn lum_hsl(hex_in: &str, md: f64, off: f64) -> String {
    let [r, g, b] = rgb_bytes(hex_in);
    let (rf, gf, bf) = (r / 255.0, g / 255.0, b / 255.0);
    let max = math::max(math::max(rf, gf), bf);
    let min = math::min(math::min(rf, gf), bf);
    let d = max - min;
    let mut hh = 0.0;
    let mut s = 0.0;
    let mut l = (max + min) / 2.0;
    if d != 0.0 {
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        hh = if max == rf {
            math::rem((gf - bf) / d, 6.0)
        } else if max == gf {
            (bf - rf) / d + 2.0
        } else {
            (rf - gf) / d + 4.0
        };
        hh /= 6.0;
        if hh < 0.0 {
            hh += 1.0;
        }
    }
    l = math::max(0.0, math::min(1.0, l * md + off));
    let c = (1.0 - math::abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - math::abs(math::rem(hh * 6.0, 2.0) - 1.0));
    let m = l - c / 2.0;
    let (mut r2, mut g2, mut b2) = (0.0, 0.0, 0.0);
    let seg = math::rem(math::floor(hh * 6.0), 6.0);
    if seg == 0.0 {
        r2 = c;
        g2 = x;
    } else if seg == 1.0 {
        r2 = x;
        g2 = c;
    } else if seg == 2.0 {
        g2 = c;
        b2 = x;
    } else if seg == 3.0 {
        g2 = x;
        b2 = c;
    } else if seg == 4.0 {
        r2 = x;
        b2 = c;
    } else {
        r2 = c;
        b2 = x;
    }
    hex((r2 + m) * 255.0, (g2 + m) * 255.0, (b2 + m) * 255.0)
}
