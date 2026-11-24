//! The reading side of Sheet track C (feature `read`, frontier C1a): the workbook model, the readers of the
//! parts and the streaming worksheet reader. Behaviour source: the frozen TS `sheet-core` (`read.ts`, `xml.ts`,
//! `styles.ts`, `theme.ts`, `read-cf.ts`, `read-pivot.ts`, `drawing.ts`, `package.ts`). Record
//! `core/docs/modules/uo-xlsx.md` "C1a — as built".

pub mod cf;
pub mod drawing;
pub mod model;
pub mod sheet;
pub mod sst;
pub mod stream;
pub mod styles;
pub mod theme;
pub mod workbook;

#[cfg(feature = "facet")]
pub mod facet;

use uo_opc::xml::{CharRefPolicy, ContentPi, Element, Mode, ParseOptions};

pub(crate) type El<'a> = Element<'a>;

/// `sheet-core` `MAIN_NS`: namespaces whose prefix the root may declare and the TS strips from element names.
pub const MAIN_NS: &[&str] = &[
    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "http://purl.oclc.org/ooxml/spreadsheetml/main",
    "http://schemas.openxmlformats.org/package/2006/relationships",
];

/// The sheet-core XML grammar for the DOM readers (`xml.ts`): lenient, end-of-line and attribute normalization,
/// the main prefix stripped, a numeric reference past U+10FFFF an error.
pub const SHEET_XML: ParseOptions<'static> = ParseOptions {
    mode: Mode::Lenient,
    canonical: None,
    strip_root_prefixes: MAIN_NS,
    normalize_eol: true,
    bad_char_ref: CharRefPolicy::Error,
    content_pi: ContentPi::AsElement,
    max_depth: None,
    max_elements: None,
    raw_capture: &[],
    positions: false,
    keep_comments: false,
    keep_pis: false,
};

/// `parseInt(v, 10)`.
pub(crate) fn pi(v: &str) -> f64 {
    uo_jsnum::parse::parse_int(v, 10)
}

/// `parseFloat(v)`.
pub(crate) fn pf(v: &str) -> f64 {
    uo_jsnum::parse::parse_float(v)
}

/// `v === '1' || v === 'true'`.
pub(crate) fn on(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true"))
}

/// `v === '0' || v === 'false'`.
pub(crate) fn off(v: Option<&str>) -> bool {
    matches!(v, Some("0" | "false"))
}

pub(crate) fn owned(v: Option<&str>) -> Option<String> {
    v.map(str::to_owned)
}

/// `String.prototype.trim` (JS white space).
pub(crate) fn trim(s: &str) -> &str {
    uo_jsnum::text::trim(s)
}

/// `refToRowCol` (`model.ts:530-538`): the first `[A-Z]+\d+` anywhere in `s`; `col` 1-based as the JS reduce
/// computes it (a double), `row` the `parseInt` of the digits.
pub fn ref_to_row_col(s: &[u8]) -> Option<(f64, f64)> {
    let mut i = 0;
    while i < s.len() {
        if s[i].is_ascii_uppercase() {
            let mut j = i;
            while j < s.len() && s[j].is_ascii_uppercase() {
                j += 1;
            }
            if s.get(j).is_some_and(u8::is_ascii_digit) {
                let col = s[i..j].iter().fold(0.0f64, |acc, &c| acc * 26.0 + f64::from(c - 64));
                let mut k = j;
                while k < s.len() && s[k].is_ascii_digit() {
                    k += 1;
                }
                let row = std::str::from_utf8(&s[j..k]).map_or(f64::NAN, pi);
                return Some((col, row));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

/// `decodeXstring` (`read.ts:21-23`): `_xHHHH_` → the UTF-16 unit.
pub fn decode_xstring(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    let hex = |u: u16| char::from_u32(u32::from(u)).and_then(|c| c.to_digit(16));
    while i < units.len() {
        if units[i] == u16::from(b'_')
            && units.get(i + 1) == Some(&u16::from(b'x'))
            && i + 6 < units.len()
            && units[i + 6] == u16::from(b'_')
            && let Some(v) = units[i + 2..i + 6].iter().try_fold(0u32, |a, &u| Some(a * 16 + hex(u)?))
        {
            out.push(u16::try_from(v).unwrap_or(0xFFFD));
            i += 7;
            continue;
        }
        out.push(units[i]);
        i += 1;
    }
    out
}

/// `decodeXstring` of a Rust string (lone surrogates cannot occur in it; a decoded `_xD800_` becomes U+FFFD).
pub fn decode_xstring_str(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_owned();
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&decode_xstring(&units))
}
