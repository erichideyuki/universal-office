//! Frontier R2 (D1a reader, record "Milestone 1 — D1a foundation"): `table-styles.ts` (read side):
//! `CORE_TABLE_STYLES`, `localStyleId`, `loadBuiltinTableStyles` (over `data::table_styles_builtin`),
//! `themeHex`, `materializeStyleXml`, `styleDefOf` and `styleIdByName` (the declarations are in
//! `core/docs/modules/uo-docx-d1a/R2.md`). `ensureTableStyle` and `addStylesXml` write the
//! package and the sheet: they belong to the session (milestone 2).
//!
//! Regex sites: `localStyleId` (`/[^A-Za-z0-9-]/g`), the `{A}` and `{ID}` substitutions, the two
//! marker replaces of `materializeStyleXml` (`/w:color="~"([^/>]*)/g`, `/w:fill="~"([^/>]*)/g`)
//! and its `attr` lookup (`new RegExp(`${name}="([^"]*)"`)`, `name` a constant), written as
//! plain scans; each has a row in `js/regex-sites/r2.mjs`.

use uo_jsnum::conv;
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{to_lowercase, utf16_slice};
use uo_opc::xml::parse;

use crate::data::table_styles_builtin::BUILTIN_TABLE_STYLES;
use crate::error::DocxError;
use crate::js::{JsRecord, Str};
use crate::model::formatting::{DocFormatting, StyleDef};

use super::DOCX;
use super::parts::read_style_sheet;

/// The `xmlns:w` attribute the TS wraps a style in (`W`).
const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

/// One entry of `CORE_TABLE_STYLES`: a style always embedded (the gallery comes on demand).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreTableStyle {
    /// the key of the TS record: the `w:name` (English)
    pub name: &'static str,
    pub id: &'static str,
    pub local: &'static str,
    pub xml: &'static str,
}

/// `CORE_TABLE_STYLES`, in the TS record order.
pub const CORE_TABLE_STYLES: [CoreTableStyle; 2] = [
    CoreTableStyle {
        name: "Normal Table",
        id: "Tabelanormal",
        local: "Tabela normal",
        xml: r#"<w:style w:type="table" w:default="1" w:styleId="Tabelanormal"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#,
    },
    CoreTableStyle {
        name: "Table Grid",
        id: "Tabelacomgrade",
        local: "Tabela com grade",
        xml: r#"<w:style w:type="table" w:styleId="Tabelacomgrade"><w:name w:val="Table Grid"/><w:basedOn w:val="Tabelanormal"/><w:uiPriority w:val="39"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr></w:style>"#,
    },
];

/// `localStyleId`: the Word pt-BR id of a local name (`local.replace(/[^A-Za-z0-9-]/g, '')`).
#[must_use]
pub fn local_style_id(local: &str) -> String {
    local.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

/// A built-in table style ready to use (`BuiltinTableStyle`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinTableStyle {
    /// `w:name` (English)
    pub name: String,
    /// the Word pt-BR local name
    pub local: String,
    pub id: String,
    /// the modern gallery (compatibility 15)
    pub modern: bool,
    /// the `w:style` with `~` in place of the hex of theme colours
    pub xml: String,
}

/// `loadBuiltinTableStyles` (the TS caches the promise in a module variable; here each call
/// builds the list and the caller keeps it): the 247 built-in styles, each source expanded over
/// its accents (`{A}` replaced), with the id from the local name and `{ID}` filled.
#[must_use]
pub fn builtin_table_styles() -> Vec<BuiltinTableStyle> {
    let mut out = Vec::new();
    for s in BUILTIN_TABLE_STYLES {
        let accents: Vec<Option<u8>> = match s.accents {
            Some(list) => list.iter().map(|&a| Some(a)).collect(),
            None => vec![None],
        };
        for a in accents {
            let sub = |x: &str| match a {
                None => x.to_string(),
                Some(n) => x.replace("{A}", &n.to_string()),
            };
            let local = sub(s.local);
            let id = local_style_id(&local);
            let xml = sub(s.xml).replacen("{ID}", &id, 1);
            out.push(BuiltinTableStyle { name: sub(s.name), local, id, modern: s.modern, xml });
        }
    }
    out
}

const THEME_KEY: [(&str, &str); 10] = [
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
];

/// The Office 2023 theme (Word uses it without `theme1.xml`).
const OFFICE_THEME: [(&str, &str); 12] = [
    ("dk1", "000000"),
    ("lt1", "FFFFFF"),
    ("dk2", "0E2841"),
    ("lt2", "E8E8E8"),
    ("accent1", "156082"),
    ("accent2", "E97132"),
    ("accent3", "196B24"),
    ("accent4", "0F9ED5"),
    ("accent5", "A02B93"),
    ("accent6", "4EA72E"),
    ("hlink", "467886"),
    ("folHlink", "96607D"),
];

/// The names a plain JS object inherits from `Object.prototype`: `THEME_KEY[name]` is then a
/// function or an object, never a theme key, and the colour lookup falls to `'000000'`.
const OBJECT_PROTOTYPE_NAMES: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// `parseInt(base.slice(i, i + 2), 16) / 255`.
fn hex_channel(base: &str, i: i64) -> f64 {
    parse_int(&utf16_slice(base, i, i + 2), 16) / 255.0
}

/// `Math.max(0, Math.min(255, Math.floor(v * 255))).toString(16).padStart(2, '0')` (`NaN` prints
/// as `NaN`).
fn channel_hex(v: f64) -> String {
    let c = math::max(0.0, math::min(255.0, math::floor(v * 255.0)));
    if c.is_nan() { "NaN".to_string() } else { format!("{:02x}", conv::to_uint8(c)) }
}

/// `themeHex` (table-styles.ts:64): a theme colour with tint and shade as Word does (HSL,
/// channels truncated); `tint` and `shade` are the `w:themeTint`/`w:themeShade` hex strings.
#[must_use]
pub fn theme_hex(colors: &JsRecord<Str>, theme_color: &str, tint: Option<&str>, shade: Option<&str>) -> String {
    let key = THEME_KEY.iter().find(|(k, _)| *k == theme_color).map_or(theme_color, |(_, v)| *v);
    let base: &str = if OBJECT_PROTOTYPE_NAMES.contains(&theme_color) {
        "000000"
    } else {
        match colors.get(key) {
            Some(c) => c,
            None => OFFICE_THEME.iter().find(|(k, _)| *k == key).map_or("000000", |(_, v)| *v),
        }
    };
    let (tint, shade) = (tint.filter(|s| !s.is_empty()), shade.filter(|s| !s.is_empty()));
    if tint.is_none() && shade.is_none() {
        return base.to_uppercase();
    }
    let (r, g, b) = (hex_channel(base, 0), hex_channel(base, 2), hex_channel(base, 4));
    let mx = math::max_n(&[r, g, b]);
    let mn = math::min_n(&[r, g, b]);
    let mut l = (mx + mn) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if mx != mn {
        let d = mx - mn;
        s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
        h = (if mx == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        }) / 6.0;
    }
    if let Some(shade) = shade {
        l *= parse_int(shade, 16) / 255.0;
    }
    if let Some(tint) = tint {
        let t = parse_int(tint, 16) / 255.0;
        l = l * t + 1.0 - t;
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let ch = |mut x: f64| -> String {
        if x < 0.0 {
            x += 1.0;
        }
        if x > 1.0 {
            x -= 1.0;
        }
        let v = if x < 1.0 / 6.0 {
            p + (q - p) * 6.0 * x
        } else if x < 1.0 / 2.0 {
            q
        } else if x < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - x) * 6.0
        } else {
            p
        };
        channel_hex(v)
    };
    format!("{}{}{}", ch(h + 1.0 / 3.0), ch(h), ch(h - 1.0 / 3.0)).to_uppercase()
}

/// `attr(tag, name)` of `materializeStyleXml`: `new RegExp(`${name}="([^"]*)"`).exec(tag)?.[1]`
/// (`name` is a constant without metacharacters): the first `name="` and the text up to the next
/// quote (there is always one: a later occurrence has its own).
#[must_use]
pub fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let start = tag.find(&pat)? + pat.len();
    let rest = tag.get(start..)?;
    rest.get(..rest.find('"')?)
}

/// `xml.replace(/MARKER([^/>]*)/g, (_m, rest) => f(rest))`: every occurrence of `marker`, left
/// to right without overlap, with the longest run of characters other than `/` and `>` after it.
#[must_use]
pub fn replace_marker(xml: &str, marker: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut i = 0;
    while let Some(k) = xml.get(i..).and_then(|t| t.find(marker)) {
        let at = i + k;
        out.push_str(xml.get(i..at).unwrap_or(""));
        let from = at + marker.len();
        let tail = xml.get(from..).unwrap_or("");
        let len = tail.find(['/', '>']).unwrap_or(tail.len());
        out.push_str(&f(tail.get(..len).unwrap_or("")));
        i = from + len;
    }
    out.push_str(xml.get(i..).unwrap_or(""));
    out
}

/// `materializeStyleXml` (table-styles.ts:86): the `w:style` with the hex of its theme colours
/// computed from the document's theme (`w:color="~"` and `w:fill="~"`).
#[must_use]
pub fn materialize_style_xml(xml: &str, colors: &JsRecord<Str>) -> String {
    let colored = replace_marker(xml, "w:color=\"~\"", |rest| {
        let hex = theme_hex(
            colors,
            attr_value(rest, "w:themeColor").unwrap_or("text1"),
            attr_value(rest, "w:themeTint"),
            attr_value(rest, "w:themeShade"),
        );
        format!("w:color=\"{hex}\"{rest}")
    });
    replace_marker(&colored, "w:fill=\"~\"", |rest| {
        let hex = theme_hex(
            colors,
            attr_value(rest, "w:themeFill").unwrap_or("background1"),
            attr_value(rest, "w:themeFillTint"),
            attr_value(rest, "w:themeFillShade"),
        );
        format!("w:fill=\"{hex}\"{rest}")
    })
}

/// `styleDefOf` (table-styles.ts:99): the typed definition of a raw `w:style`, the first style
/// of the sheet it forms (`Object.values(sheet.styles)[0]`).
pub fn style_def_of(xml: &str) -> Result<Option<StyleDef>, DocxError> {
    let doc = parse(format!("<w:styles {W}>{xml}</w:styles>"), &DOCX)?;
    Ok(read_style_sheet(Some(&doc)).styles.values().next().cloned())
}

/// `styleIdByName` (table-styles.ts:105): the id of the document's table style with that
/// `w:name` (compared without case), the first in the sheet's order.
#[must_use]
pub fn style_id_by_name<'a>(f: Option<&'a DocFormatting>, name: &str) -> Option<&'a str> {
    let n = to_lowercase(name);
    f?.styles
        .styles
        .values()
        .find(|s| &*s.r#type == "table" && s.name.as_deref().is_some_and(|x| to_lowercase(x) == n))
        .map(|s| &*s.style_id)
}
