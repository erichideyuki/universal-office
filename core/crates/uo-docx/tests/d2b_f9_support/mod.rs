//! Shared by the D2b-F9 tests: the tables the captures index (`FORMATS`, `REPLS`), the query codec
//! of `js/capture-d2b-f9-ops.mjs`, the hex/sha helpers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::find::engine::{FindFormat, FindQuery};
use uo_testkit::Digest;

pub const REPLS: [&str; 16] = [
    "x",
    "",
    "NOVO",
    "a^pb",
    "^&^&",
    r"\1-\2",
    "^c",
    "^t^l^m^n^s^-^~^+^=^^",
    "^0150^u8212^13^11^12^9^65",
    r"\\x",
    "^",
    "^q",
    "Ação ß ŉ",
    "😀x",
    r"\",
    r"ab\9c^u55357",
];
pub const CLIP: &str = "clip";

/// `FORMATS[i]` of the capture.
pub fn format(i: usize) -> Option<FindFormat> {
    let mut f = FindFormat::default();
    match i {
        0 => return None,
        1 => f.bold = Some(true),
        2 => f.italic = Some(true),
        3 => f.bold = Some(false),
        4 => f.underline = Some(true),
        5 => f.vert_align = Some(Some("superscript".into())),
        6 => f.vert_align = Some(None),
        7 => f.size_pt = Some(11.0),
        8 => f.size_pt = Some(12.0),
        9 => f.color = Some("000000".into()),
        10 => f.highlight = Some(true),
        11 => f.highlight = Some(false),
        12 => f.font = Some("Calibri".into()),
        13 => f.para_style = Some("Normal".into()),
        14 => f.para_style = Some("Heading1".into()),
        15 => f.char_style = Some(String::new()),
        16 => {
            f.bold = Some(true);
            f.italic = Some(true);
        }
        17 => {
            f.all_caps = Some(false);
            f.small_caps = Some(false);
            f.hidden = Some(false);
        }
        18 => f.para_style = Some("Ttulo1".into()),
        19 => {
            f.strike = Some(false);
            f.double_strike = Some(false);
        }
        20 => f.size_pt = Some(10.5),
        21 => f.vert_align = Some(Some("subscript".into())),
        22 => f.color = Some("ff0000".into()),
        23 => f.font = Some("arial".into()),
        _ => panic!("format {i}"),
    }
    Some(f)
}

/// The query of a capture op: option letters, text, format index.
pub fn query(flags: &str, text: &[u16], fmt: usize) -> FindQuery {
    let mut q = FindQuery { text: text.to_vec(), format: format(fmt).map(Box::new), ..FindQuery::default() };
    for c in flags.chars() {
        match c {
            'c' => q.match_case = true,
            'w' => q.whole_word = true,
            'x' => q.wildcards = true,
            's' => q.sounds_like = true,
            'a' => q.all_forms = true,
            'p' => q.prefix = true,
            'f' => q.suffix = true,
            'u' => q.ignore_punct = true,
            'e' => q.ignore_space = true,
            _ => panic!("flag {c}"),
        }
    }
    q
}

pub fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

pub fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

pub fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

/// `sha(t)` of the captures: the first 32 hex digits of SHA-256 over the UTF-8 of `t`.
pub fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

pub mod hostile;
