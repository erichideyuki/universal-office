//! DrawingML colour elements (`a:srgbClr`, `a:schemeClr`, `a:sysClr`, `a:prstClr`, plus
//! `a:scrgbClr`/`a:hslClr`) read from XML into a structured [`DmlColor`], and their resolution to
//! RGB with a theme (uo-docx plan G12: the resolver lives here, shared by Write, Slides and the
//! charts; the call sites' quirks are named variants until stage 5).
//!
//! [`DmlColor`] keeps the attribute strings **as written** (`val`, `lastClr`, every modifier's
//! `val`): the TS readers parse them differently (`srgbOf` in docx-core `drawing.ts` keeps the raw
//! modifier strings and the layout parses them with `parseInt` when it resolves; `readDmlColor` in
//! `picture-xml.ts` parses with `parseInt(...) || 0` at read; the charts read `parseInt / 100000`),
//! and an invalid value has to reach the writer unchanged. Each variant parses at use.
//!
//! Implemented variants (Write, the first consumer):
//! - [`word_spec`]: `srgbOf` of `drawing.ts` (the head colour, the alpha and the raw modifiers of
//!   the "RRGGBB|mods" spec the Word model keeps, structured);
//! - [`resolve_word_spec`]: `resolveDrawingColor` of `docx-layout/src/resolve.ts` over that
//!   structure (theme alias table, six-digit check, `applyColorMods`, `toHex`).
//!
//! Not yet (stub list, each with its first consumer): the charts' `dmlColor`
//! (`shared/charts/src/dml-color.ts`, uo-charts), the Slides resolver with `clrMap` and `phClr`
//! (`slides-core/src/theme.ts`, the Slides port), and the converter's (not ported).

use uo_jsnum::math::{max, min, round};
use uo_jsnum::parse::parse_int;
use uo_opc::xml::Element;

use crate::color::{ColorModKind, Rgb, apply_color_mods};

/// The colour element kinds of DrawingML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKind {
    Srgb,
    Scheme,
    Sys,
    Prst,
    ScRgb,
    Hsl,
}

impl ColorKind {
    /// The kind of a local element name (`srgbClr`...), `None` for any other name.
    pub fn from_local(local: &str) -> Option<ColorKind> {
        Some(match local {
            "srgbClr" => ColorKind::Srgb,
            "schemeClr" => ColorKind::Scheme,
            "sysClr" => ColorKind::Sys,
            "prstClr" => ColorKind::Prst,
            "scrgbClr" => ColorKind::ScRgb,
            "hslClr" => ColorKind::Hsl,
            _ => return None,
        })
    }
}

/// One child of a colour element (`a:lumMod val="75000"`, `a:alpha`, `a:gray`...), as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColorMod {
    /// the element name as the XML reader gives it (with the canonical prefix: `a:lumMod`)
    pub name: Box<str>,
    /// the `val` attribute (with the reader's attribute prefix), as written
    pub val: Option<Box<str>>,
}

impl ColorMod {
    /// The local name (after the prefix and its colon).
    pub fn local_name(&self) -> &str {
        self.name.rsplit_once(':').map_or(&*self.name, |(_, l)| l)
    }

    /// The modifier `apply_color_mods` knows, by local name.
    pub fn kind(&self) -> Option<ColorModKind> {
        ColorModKind::from_name(self.local_name())
    }

    /// `parseInt(val, 10)`; `None` when the attribute is absent.
    pub fn int_val(&self) -> Option<f64> {
        self.val.as_deref().map(|v| parse_int(v, 10))
    }
}

/// A DrawingML colour element as written (see the module comment).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DmlColor {
    /// the element name as the XML reader gives it (`a:srgbClr`, `w14:schemeClr`...)
    pub name: Box<str>,
    /// `val` (with the reader's attribute prefix)
    pub val: Option<Box<str>>,
    /// `lastClr` (`a:sysClr`)
    pub last_clr: Option<Box<str>>,
    /// every child element in document order (`a:alpha` included)
    pub mods: Vec<ColorMod>,
}

impl DmlColor {
    /// The local name of the element (after the prefix).
    pub fn local_name(&self) -> &str {
        self.name.rsplit_once(':').map_or(&*self.name, |(_, l)| l)
    }

    /// The kind by local name (`None` when the element is not a colour element).
    pub fn kind(&self) -> Option<ColorKind> {
        ColorKind::from_local(self.local_name())
    }
}

/// Reads a colour element: its name, `{attr_prefix}val`, `{attr_prefix}lastClr` and every child
/// with its `{attr_prefix}val` (DrawingML `a:` elements have unprefixed attributes: `""`; the w14
/// colours of Word's text effects use `"w14:"`).
pub fn read_color(el: Element<'_>, attr_prefix: &str) -> DmlColor {
    let mut key = String::with_capacity(attr_prefix.len() + 7);
    let mut attr = |name: &str, e: Element<'_>| -> Option<Box<str>> {
        key.clear();
        key.push_str(attr_prefix);
        key.push_str(name);
        e.attr(&key).map(Box::from)
    };
    let val = attr("val", el);
    let last_clr = attr("lastClr", el);
    let mods = el.child_elements().map(|m| ColorMod { name: m.name().into(), val: attr("val", m) }).collect();
    DmlColor { name: el.name().into(), val, last_clr, mods }
}

/// The first child of `parent` named `names[0]`, else the first named `names[1]`, ... (the
/// `firstChild(el, a) ?? firstChild(el, b) ?? …` chains of the TS readers: priority by name,
/// not document order).
pub fn first_child_by_priority<'d>(parent: Element<'d>, names: &[&str]) -> Option<Element<'d>> {
    names.iter().find_map(|n| parent.first_child(n))
}

/// The first child of `parent` whose name is one of `names` (document order: `childElements(…)
/// .find((k) => names.includes(k.name))`).
pub fn first_child_in<'d>(parent: Element<'d>, names: &[&str]) -> Option<Element<'d>> {
    parent.child_elements().find(|k| names.contains(&k.name()))
}

/// Theme colours by slot name, as the app's theme reader stored them (Write: `Theme.colors`,
/// `RRGGBB`). The lookup is of own keys only (the TS `own()` helper).
pub trait SchemeColors {
    fn scheme_color(&self, key: &str) -> Option<&str>;
}

impl SchemeColors for [(Box<str>, Box<str>)] {
    fn scheme_color(&self, key: &str) -> Option<&str> {
        self.iter().find(|(k, _)| &**k == key).map(|(_, v)| &**v)
    }
}

/// No theme (`srgbOf` called without one).
pub struct NoScheme;

impl SchemeColors for NoScheme {
    fn scheme_color(&self, _key: &str) -> Option<&str> {
        None
    }
}

/// The head of a Word colour spec: a colour already resolved at read (`RRGGBB` as written,
/// upper-cased) or a scheme reference (`@accent1`), resolved by the layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecHead {
    /// the `hex` of `srgbOf` (upper-cased; not validated: `"12345"` stays and fails at resolve)
    Hex(Box<str>),
    /// `@key` (the key after the read-time alias of `srgbOf`, without the `@`)
    Scheme(Box<str>),
}

/// The Word colour spec `srgbOf` (docx-core `drawing.ts`) produces, structured: the TS string
/// `head|name=val,…` is `head` + `mods` (`name` = the element name without its first two UTF-16
/// units, `val` = the raw attribute, `"undefined"` when absent), plus the alpha read from the
/// first `a:alpha`.
#[derive(Debug, Clone, PartialEq)]
pub struct WordSpec {
    pub head: SpecHead,
    /// every child except `a:alpha`, in document order
    pub mods: Vec<ColorMod>,
    /// `alpha`: `(parseInt(val ?? '100000', 10) || 0) / 100000` of the first `a:alpha` child
    pub alpha: Option<f64>,
}

const SRGBOF_ALIAS: [(&str, &str); 4] = [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")];
const SRGBOF_PRESET: [(&str, &str); 7] = [
    ("black", "000000"),
    ("white", "FFFFFF"),
    ("red", "FF0000"),
    ("green", "00FF00"),
    ("blue", "0000FF"),
    ("yellow", "FFFF00"),
    ("gray", "808080"),
];

fn lookup<'a>(table: &'a [(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// `srgbOf(el, theme)` of docx-core `drawing.ts`: the colour child of `parent`
/// (`a:srgbClr` ?? `a:schemeClr` ?? `a:sysClr` ?? `a:prstClr`, by priority) as a [`WordSpec`];
/// `None` where the TS returns `undefined` (no colour child, an `a:srgbClr` without `val`, an empty
/// hex). A scheme colour is looked up in `theme` at read time (`own(theme, alias ?? key)`); when
/// the theme has no such key the spec keeps the reference (`@key`).
pub fn word_spec<S: SchemeColors + ?Sized>(parent: Element<'_>, theme: &S) -> Option<WordSpec> {
    let c = first_child_by_priority(parent, &["a:srgbClr", "a:schemeClr", "a:sysClr", "a:prstClr"])?;
    let hex: Box<str> = match c.name() {
        "a:srgbClr" => c.attr("val")?.into(),
        "a:sysClr" => match c.attr("lastClr") {
            Some(l) => l.into(),
            None => (if c.attr("val") == Some("window") { "FFFFFF" } else { "000000" }).into(),
        },
        "a:schemeClr" => {
            let key = c.attr("val").unwrap_or("");
            let slot = lookup(&SRGBOF_ALIAS, key).unwrap_or(key);
            match theme.scheme_color(slot) {
                Some(h) => h.into(),
                None => format!("@{key}").into(),
            }
        }
        _ => lookup(&SRGBOF_PRESET, c.attr("val").unwrap_or("")).unwrap_or("000000").into(),
    };
    if hex.is_empty() {
        return None;
    }
    let alpha = c.first_child("a:alpha").map(|a| {
        let v = parse_int(a.attr("val").unwrap_or("100000"), 10);
        (if uo_jsnum::conv::truthy(v) { v } else { 0.0 }) / 100_000.0
    });
    let head = match hex.strip_prefix('@') {
        Some(key) => SpecHead::Scheme(key.into()),
        None => SpecHead::Hex(hex.to_uppercase().into()),
    };
    let mods = c
        .child_elements()
        .filter(|m| m.name() != "a:alpha")
        .map(|m| ColorMod { name: m.name().into(), val: m.attr("val").map(Box::from) })
        .collect();
    Some(WordSpec { head, mods, alpha })
}

impl WordSpec {
    /// The TS spec string (`RRGGBB|lumMod=75000,…`, `@accent1|…`): what the TS model stores, for
    /// the canonical digest and the JSON keys of the Word model.
    pub fn to_ts_string(&self) -> String {
        let mut s = String::new();
        match &self.head {
            SpecHead::Hex(h) => s.push_str(h),
            SpecHead::Scheme(k) => {
                s.push('@');
                s.push_str(k);
            }
        }
        for (i, m) in self.mods.iter().enumerate() {
            s.push(if i == 0 { '|' } else { ',' });
            s.push_str(&slice2(&m.name));
            s.push('=');
            s.push_str(m.val.as_deref().unwrap_or("undefined"));
        }
        s
    }

    /// The modifiers as `resolveDrawingColor` parses them: `parseInt(v, 10)` when present and
    /// finite, else none (the raw `"undefined"` counts as absent, as in the TS).
    pub fn parsed_mods(&self) -> impl Iterator<Item = (&str, Option<f64>)> + '_ {
        self.mods.iter().map(|m| {
            let n = m.val.as_deref().filter(|v| *v != "undefined").map(|v| parse_int(v, 10)).filter(|n| n.is_finite());
            (m.name.get(2..).unwrap_or(""), n)
        })
    }
}

/// `name.slice(2)` of a JS string (UTF-16 units); element names are ASCII in practice.
fn slice2(name: &str) -> std::borrow::Cow<'_, str> {
    uo_jsnum::text::utf16_slice(name, 2, i64::MAX)
}

/// The theme aliases of `resolve.ts` (`THEME_ALIAS`).
const RESOLVE_ALIAS: [(&str, &str); 14] = [
    ("text1", "dk1"),
    ("background1", "lt1"),
    ("text2", "dk2"),
    ("background2", "lt2"),
    ("dark1", "dk1"),
    ("light1", "lt1"),
    ("dark2", "dk2"),
    ("light2", "lt2"),
    ("hyperlink", "hlink"),
    ("followedHyperlink", "folHlink"),
    ("tx1", "dk1"),
    ("bg1", "lt1"),
    ("tx2", "dk2"),
    ("bg2", "lt2"),
];

/// The theme slot `resolve.ts` looks up for a theme colour name (`own(THEME_ALIAS, k) ?? k`).
pub fn word_theme_slot(key: &str) -> &str {
    lookup(&RESOLVE_ALIAS, key).unwrap_or(key)
}

/// Six hexadecimal digits (`/^[0-9A-Fa-f]{6}$/`).
pub fn is_hex6(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `rgb(hex)` of `resolve.ts` on a validated six-digit hex, as 0..1 channels.
pub fn hex6_rgb(hex: &str) -> Option<Rgb> {
    if !is_hex6(hex) {
        return None;
    }
    let ch = |i: usize| hex.get(i..i + 2).map_or(f64::NAN, |p| parse_int(p, 16)) / 255.0;
    Some([ch(0), ch(2), ch(4)])
}

/// `toHex(r, g, b)` of `resolve.ts` (channels 0..255): `max(0, min(255, round(v)))` in hex,
/// two digits, upper case; a NaN channel prints `NAN` as `NaN.toString(16).toUpperCase()` does.
pub fn to_hex_upper(r: f64, g: f64, b: f64) -> String {
    let mut s = String::with_capacity(6);
    for v in [r, g, b] {
        let c = max(0.0, min(255.0, round(v)));
        if c.is_nan() {
            s.push_str("NAN");
        } else {
            let n = uo_jsnum::conv::to_uint8(c);
            s.push(char::from(b"0123456789ABCDEF"[usize::from(n >> 4)]));
            s.push(char::from(b"0123456789ABCDEF"[usize::from(n & 15)]));
        }
    }
    s
}

/// `resolveDrawingColor(spec, theme)` of `docx-layout/src/resolve.ts` over the structured spec:
/// the head (a scheme reference through `THEME_ALIAS` and the theme), the six-digit check, then
/// `applyColorMods` with the parsed modifiers (unknown names have no effect) and `toHex`.
///
/// Named difference (accepted, uo-docx record): the TS splits the spec string on `|`, `,` and
/// `=`, so a raw modifier value containing one of them (or a head with `|`) is split into other
/// modifiers there; here the structure read from the XML is used.
pub fn resolve_word_spec<'a, S: SchemeColors + ?Sized>(
    head: &SpecHead,
    mods: impl IntoIterator<Item = (&'a str, Option<f64>)>,
    theme: &S,
) -> Option<String> {
    let hex = match head {
        SpecHead::Hex(h) => &**h,
        SpecHead::Scheme(k) => theme.scheme_color(word_theme_slot(k))?,
    };
    let base = hex6_rgb(hex)?;
    let mut list: Vec<(ColorModKind, Option<f64>)> = Vec::new();
    let mut any = false;
    for (name, v) in mods {
        any = true;
        if let Some(k) = ColorModKind::from_name(name) {
            list.push((k, v));
        }
    }
    if !any {
        return Some(hex.to_ascii_uppercase());
    }
    let [r, g, b] = apply_color_mods(base, &list);
    Some(to_hex_upper(r * 255.0, g * 255.0, b * 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_opc::xml::{ParseOptions, parse};

    fn spec(xml: &str, theme: &[(Box<str>, Box<str>)]) -> Option<WordSpec> {
        let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
        word_spec(doc.root(), theme)
    }

    #[test]
    fn srgbof_shapes() {
        let none: [(Box<str>, Box<str>); 0] = [];
        let s =
            spec(r#"<x><a:srgbClr val="4472c4"><a:alpha val="50000"/><a:lumMod val="75000"/><a:lumOff/></a:srgbClr></x>"#, &none).unwrap();
        assert_eq!(s.head, SpecHead::Hex("4472C4".into()));
        assert_eq!(s.alpha, Some(0.5));
        assert_eq!(s.to_ts_string(), "4472C4|lumMod=75000,lumOff=undefined");
        let theme = [(Box::from("dk1"), Box::from("000000"))];
        assert_eq!(spec(r#"<x><a:schemeClr val="tx1"/></x>"#, &theme).unwrap().to_ts_string(), "000000");
        assert_eq!(
            spec(r#"<x><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></x>"#, &theme).unwrap().to_ts_string(),
            "@accent1|shade=50000"
        );
        assert_eq!(spec(r#"<x><a:prstClr val="green"/><a:srgbClr val="ff0000"/></x>"#, &none).unwrap().to_ts_string(), "FF0000");
        assert_eq!(spec(r#"<x><a:sysClr val="window"/></x>"#, &none).unwrap().to_ts_string(), "FFFFFF");
        assert!(spec(r#"<x><a:srgbClr/></x>"#, &none).is_none());
        assert!(spec(r#"<x><a:srgbClr val=""/></x>"#, &none).is_none());
        assert!(spec(r#"<x/>"#, &none).is_none());
    }

    #[test]
    fn resolve_follows_resolve_ts() {
        let theme = [(Box::from("accent1"), Box::from("4472C4")), (Box::from("dk1"), Box::from("000000"))];
        let s = spec(r#"<x><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></x>"#, &[]).unwrap();
        // the outline of Word's default shape (drawingml color.ts comment): accent1 shade 50% = #2F528F
        assert_eq!(resolve_word_spec(&s.head, s.parsed_mods(), &theme[..]).as_deref(), Some("2F528F"));
        assert_eq!(resolve_word_spec(&SpecHead::Scheme("text1".into()), [], &theme[..]).as_deref(), Some("000000"));
        assert_eq!(resolve_word_spec(&SpecHead::Hex("12345".into()), [], &theme[..]), None);
        assert_eq!(resolve_word_spec(&SpecHead::Hex("abcdef".into()), [], &theme[..]).as_deref(), Some("ABCDEF"));
        assert_eq!(resolve_word_spec(&SpecHead::Hex("ABCDEF".into()), [("bogus", Some(1.0))], &theme[..]).as_deref(), Some("ABCDEF"));
        assert_eq!(to_hex_upper(f64::NAN, 0.4, 300.0), "NAN00FF");
    }
}
