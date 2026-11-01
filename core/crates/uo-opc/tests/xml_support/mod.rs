//! Shared helpers of the XML tests: the TS presets (the tables live in the format crates later)
//! and the canonical dump the oracle generator (`gen_ts_oracle.mjs`) writes for the TS side.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use uo_opc::xml::{CanonicalPrefixes, CharRefPolicy, ContentPi, Element, Node, ParseOptions};

/// docx-core xml.ts `CANONICAL_PREFIX`.
pub const WORD_TABLE: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "wp"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://schemas.openxmlformats.org/drawingml/2006/picture", "pic"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/math", "m"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingShape", "wps"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingGroup", "wpg"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas", "wpc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing", "wp14"),
    ("http://schemas.microsoft.com/office/word/2010/wordml", "w14"),
    ("http://schemas.microsoft.com/office/word/2012/wordml", "w15"),
    ("urn:schemas-microsoft-com:vml", "v"),
    ("urn:schemas-microsoft-com:office:office", "o"),
    ("urn:schemas-microsoft-com:office:word", "w10"),
];

/// shared/charts xml.ts `CANONICAL`.
pub const CHART_TABLE: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/drawingml/2006/chart", "c"),
    ("http://purl.oclc.org/ooxml/drawingml/chart", "c"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://purl.oclc.org/ooxml/drawingml/main", "a"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://purl.oclc.org/ooxml/officeDocument/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing", "xdr"),
    ("http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing", "xdr"),
    ("http://schemas.openxmlformats.org/drawingml/2006/chartDrawing", "cdr"),
    ("http://purl.oclc.org/ooxml/drawingml/chartDrawing", "cdr"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/drawing/2007/8/2/chart", "c14"),
    ("http://schemas.microsoft.com/office/drawing/2012/chart", "c15"),
    ("http://schemas.microsoft.com/office/drawing/2014/chart", "c16"),
    ("http://schemas.microsoft.com/office/drawing/2017/03/chart", "c16r3"),
    ("http://schemas.microsoft.com/office/drawing/2014/chartex", "cx"),
    ("http://schemas.microsoft.com/office/drawing/2015/9/8/chartex", "cx1"),
    ("http://schemas.microsoft.com/office/drawing/2015/10/21/chartex", "cx2"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/9/chartex", "cx3"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/10/chartex", "cx4"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/11/chartex", "cx5"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/12/chartex", "cx6"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/13/chartex", "cx7"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/14/chartex", "cx8"),
    ("http://schemas.microsoft.com/office/drawing/2010/main", "a14"),
];

/// sheet-core xml.ts `MAIN_NS`.
pub const SHEET_MAIN: &[&str] = &[
    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "http://purl.oclc.org/ooxml/spreadsheetml/main",
    "http://schemas.openxmlformats.org/package/2006/relationships",
];

pub const DOCX: ParseOptions<'static> = ParseOptions {
    canonical: Some(CanonicalPrefixes { table: WORD_TABLE, default_namespace: false }),
    max_depth: Some(1000),
    raw_capture: &["w:drawing"],
    ..ParseOptions::LENIENT
};
pub const DOCX_POS: ParseOptions<'static> = ParseOptions { positions: true, ..DOCX };
pub const SHEET: ParseOptions<'static> = ParseOptions { strip_root_prefixes: SHEET_MAIN, normalize_eol: true, ..ParseOptions::LENIENT };
pub const SLIDES: ParseOptions<'static> =
    ParseOptions { bad_char_ref: CharRefPolicy::Replace, content_pi: ContentPi::Skip, ..ParseOptions::LENIENT };
pub const SHEET_CHART: ParseOptions<'static> =
    ParseOptions { canonical: Some(CanonicalPrefixes { table: CHART_TABLE, default_namespace: true }), ..SHEET };

pub fn preset(name: &str) -> ParseOptions<'static> {
    match name {
        "docx" => DOCX,
        "docx-pos" => DOCX_POS,
        "sheet" => SHEET,
        "slides" => SLIDES,
        "sheet-chart" => SHEET_CHART,
        _ => panic!("unknown preset {name}"),
    }
}

/// `JSON.stringify` of a string.
pub fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Decodes a JSON string literal (the inputs of the oracle).
pub fn unq(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    assert!(b.first() == Some(&'"') && b.last() == Some(&'"'), "not a JSON string: {s}");
    let mut out = String::new();
    let mut i = 1;
    let mut units: Vec<u16> = Vec::new();
    while i < b.len() - 1 {
        let c = b[i];
        if c == '\\' {
            i += 1;
            let e = b[i];
            let ch = match e {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'u' => {
                    let h: String = b[i + 1..i + 5].iter().collect();
                    i += 4;
                    units.push(u16::from_str_radix(&h, 16).unwrap());
                    i += 1;
                    continue;
                }
                other => other,
            };
            out.push_str(&String::from_utf16_lossy(&std::mem::take(&mut units)));
            out.push(ch);
        } else {
            out.push_str(&String::from_utf16_lossy(&std::mem::take(&mut units)));
            out.push(c);
        }
        i += 1;
    }
    out.push_str(&String::from_utf16_lossy(&units));
    out
}

/// The dump of `gen_ts_oracle.mjs`.
pub fn dump(e: Element<'_>, pos: bool) -> String {
    let mut out = String::new();
    // iterative, as trees in the tests can be deep
    enum Step<'d> {
        Open(Element<'d>),
        Text(String),
    }
    let mut stack = vec![Step::Open(e)];
    while let Some(s) = stack.pop() {
        match s {
            Step::Text(t) => out.push_str(&t),
            Step::Open(e) => {
                out.push_str(e.name());
                out.push('[');
                let attrs: Vec<String> = e.attrs().map(|a| format!("{}={}", a.name, q(a.value))).collect();
                out.push_str(&attrs.join(","));
                out.push(']');
                if pos {
                    let p = e.positions().unwrap();
                    out.push_str(&format!("@{},{},{},{}", p.start, p.content_start, p.content_end, p.end));
                    if let Some(r) = e.raw() {
                        out.push('^');
                        out.push_str(&q(r));
                    }
                }
                out.push('{');
                let kids: Vec<Node<'_>> = e.children().collect();
                stack.push(Step::Text("}".into()));
                for (k, c) in kids.iter().enumerate().rev() {
                    match c {
                        Node::Element(ch) => stack.push(Step::Open(*ch)),
                        n => stack.push(Step::Text(q(n.as_text().unwrap_or("")))),
                    }
                    if k > 0 {
                        stack.push(Step::Text(",".into()));
                    }
                }
            }
        }
    }
    out
}
