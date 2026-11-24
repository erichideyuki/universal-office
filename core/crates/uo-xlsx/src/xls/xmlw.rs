//! XML writing helpers of the converter (`xmlw.ts`): the escape, A1 references, `.rels` files and the ordered part map.

use std::collections::HashMap;
use std::fmt::Write as _;

pub const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
pub const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_PKG_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Does the text need the slow path of [`xml_text`]? (`/[&<>"\u0000-\u0008\u000b\u000c\u000e-\u001f…￾￿]/`)
fn needs_escape(c: char) -> bool {
    matches!(c, '&' | '<' | '>' | '"' | '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}

/// `xmlText`: escapes text for content or an attribute. Control characters except TAB/LF/CR become numeric references
/// (NUL is dropped), U+FFFE/U+FFFF become U+FFFD. A Rust string holds no lone surrogate (the decoders replaced it by
/// U+FFFD where the TS string had one, and `xmlText` writes U+FFFD for it: the same output).
pub fn xml_text(out: &mut String, s: &str) {
    if !s.chars().any(needs_escape) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{0}' => {}
            '\u{1}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {
                let _ = write!(out, "&#x{:x};", u32::from(c));
            }
            '\u{fffe}' | '\u{ffff}' => out.push('\u{fffd}'),
            _ => out.push(c),
        }
    }
}

/// [`xml_text`] into a new string.
pub fn xml_text_s(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    xml_text(&mut o, s);
    o
}

/// Appends an unsigned integer (no allocation).
pub fn push_u(out: &mut String, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + u8::try_from(v % 10).unwrap_or(0);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap_or(""));
}

/// Appends a signed integer.
pub fn push_i(out: &mut String, v: i64) {
    if v < 0 {
        out.push('-');
    }
    push_u(out, v.unsigned_abs());
}

/// `colName`: 0-based column → letters (0 → A, 26 → AA).
pub fn col_name(out: &mut String, c: u32) {
    let mut n = u64::from(c) + 1;
    let mut buf = [0u8; 8];
    let mut i = buf.len();
    while n > 0 && i > 0 {
        let m = (n - 1) % 26;
        i -= 1;
        buf[i] = b'A' + u8::try_from(m).unwrap_or(0);
        n = (n - 1) / 26;
    }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap_or(""));
}

/// `colName` as a string.
pub fn col_name_s(c: u32) -> String {
    let mut s = String::new();
    col_name(&mut s, c);
    s
}

/// `cellRef`: A1 reference of a 0-based (row, column).
pub fn cell_ref(out: &mut String, row: u32, col: u32) {
    col_name(out, col);
    push_u(out, u64::from(row) + 1);
}

/// `cellRef` as a string.
pub fn cell_ref_s(row: u32, col: u32) -> String {
    let mut s = String::new();
    cell_ref(&mut s, row, col);
    s
}

/// `rangeRef`: a single cell collapses to `A1`.
pub fn range_ref(out: &mut String, r1: u32, c1: u32, r2: u32, c2: u32) {
    cell_ref(out, r1, c1);
    if !(r1 == r2 && c1 == c2) {
        out.push(':');
        cell_ref(out, r2, c2);
    }
}

/// `rangeRef` as a string.
pub fn range_ref_s(r1: u32, c1: u32, r2: u32, c2: u32) -> String {
    let mut s = String::new();
    range_ref(&mut s, r1, c1, r2, c2);
    s
}

/// A relationship of a `.rels` part: `kind` is the last segment of the type URI after `…/relationships/`.
#[derive(Debug, Clone)]
pub struct Rel {
    pub id: String,
    pub kind: &'static str,
    pub target: String,
    pub external: bool,
}

/// `relsXml`.
pub fn rels_xml(rels: &[Rel]) -> String {
    let mut x = String::with_capacity(160 + rels.len() * 200);
    x.push_str(XML_HEAD);
    let _ = write!(x, "<Relationships xmlns=\"{NS_PKG_RELS}\">");
    for r in rels {
        let _ = write!(x, "<Relationship Id=\"{}\" Type=\"{REL}/{}\" Target=\"", r.id, r.kind);
        xml_text(&mut x, &r.target);
        x.push('"');
        if r.external {
            x.push_str(" TargetMode=\"External\"");
        }
        x.push_str("/>");
    }
    x.push_str("</Relationships>");
    x
}

/// An insertion-ordered map of parts (`Map<string, Uint8Array>`): setting an existing name replaces its bytes and
/// keeps its position.
#[derive(Debug, Default)]
pub struct Parts {
    pub items: Vec<(String, Vec<u8>)>,
    index: HashMap<String, usize>,
}

impl Parts {
    pub fn set(&mut self, name: &str, data: Vec<u8>) {
        if let Some(&i) = self.index.get(name) {
            if let Some(slot) = self.items.get_mut(i) {
                slot.1 = data;
            }
            return;
        }
        self.index.insert(name.to_owned(), self.items.len());
        self.items.push((name.to_owned(), data));
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.index.get(name).and_then(|&i| self.items.get(i)).map(|(_, d)| d.as_slice())
    }
}
