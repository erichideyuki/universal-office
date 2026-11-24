//! Text helpers of the save: JS strings (UTF-16 units, lone surrogates) to the UTF-8 the TS writes
//! (`TextEncoder`: a lone surrogate is U+FFFD), the XML escapes of `serialize.ts` (`escXml`, `escAttr`),
//! `encodeXstring`, and the JS number print.

use uo_calc::strings::JsStr;

/// Appends `n` printed as JS prints a number (`String(n)`).
pub(crate) fn put_num(out: &mut Vec<u8>, n: f64) {
    if n.fract() == 0.0 && n.abs() < 1e15 && !(n == 0.0 && n.is_sign_negative()) {
        put_u64_signed(out, n as i64);
    } else if n == 0.0 {
        out.push(b'0');
    } else {
        out.extend_from_slice(uo_jsnum::format::to_js_string(n).as_bytes());
    }
}

/// Appends a non-negative integer.
pub(crate) fn put_u64(out: &mut Vec<u8>, mut n: u64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

fn put_u64_signed(out: &mut Vec<u8>, n: i64) {
    if n < 0 {
        out.push(b'-');
    }
    put_u64(out, n.unsigned_abs());
}

/// What to escape and how.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Esc {
    /// `escXml`: `&`, `<`, `>`.
    Text,
    /// `escAttr`: also `"`.
    Attr,
    /// `escXml(encodeXstring(s))`.
    Xstring,
    /// No escape (the TS writes `t="${c.type}"` as it is).
    Raw,
}

fn push_cp(out: &mut Vec<u8>, cp: u32) {
    match char::from_u32(cp) {
        Some(c) => {
            let mut b = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
        }
        None => out.extend_from_slice("\u{FFFD}".as_bytes()),
    }
}

fn hex4(u: u16) -> bool {
    u < 0x80 && (u as u8).is_ascii_hexdigit()
}

/// Appends the UTF-8 of a JS string with the escapes of `esc` applied per UTF-16 unit.
pub(crate) fn put_js(out: &mut Vec<u8>, s: JsStr<'_>, esc: Esc) {
    let n = s.len();
    let mut i = 0;
    let unit = |k: usize| s.unit(k).unwrap_or(0);
    while i < n {
        let u = unit(i);
        if esc == Esc::Xstring {
            // `/_x[0-9A-Fa-f]{4}_/g` -> `_x005F$&` (the matched 7 units are copied as they are)
            if u == 0x5F && n - i >= 7 && unit(i + 1) == 0x78 && (2..6).all(|k| hex4(unit(i + k))) && unit(i + 6) == 0x5F {
                out.extend_from_slice(b"_x005F");
                for k in 0..7 {
                    out.push(unit(i + k) as u8);
                }
                i += 7;
                continue;
            }
            if matches!(u, 0..=8 | 0xB | 0xC | 0xE..=0x1F | 0xFFFE | 0xFFFF) {
                out.extend_from_slice(format!("_x{u:04X}_").as_bytes());
                i += 1;
                continue;
            }
        }
        match u {
            0x26 if esc != Esc::Raw => out.extend_from_slice(b"&amp;"),
            0x3C if esc != Esc::Raw => out.extend_from_slice(b"&lt;"),
            0x3E if esc != Esc::Raw => out.extend_from_slice(b"&gt;"),
            0x22 if esc == Esc::Attr => out.extend_from_slice(b"&quot;"),
            0..=0x7F => out.push(u as u8),
            0x80..=0x7FF => {
                out.push(0xC0 | (u >> 6) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
            0xD800..=0xDBFF => {
                let lo = if i + 1 < n { unit(i + 1) } else { 0 };
                if (0xDC00..=0xDFFF).contains(&lo) {
                    push_cp(out, 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00));
                    i += 1;
                } else {
                    push_cp(out, 0xFFFD);
                }
            }
            0xDC00..=0xDFFF => push_cp(out, 0xFFFD),
            _ => {
                out.push(0xE0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        }
        i += 1;
    }
}

/// Appends a Rust string with `esc` applied (no xstring).
pub(crate) fn put_str(out: &mut Vec<u8>, s: &str, esc: Esc) {
    for &b in s.as_bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' if esc == Esc::Attr => out.extend_from_slice(b"&quot;"),
            _ => out.push(b),
        }
    }
}

/// `escAttr` of a Rust string.
pub(crate) fn esc_attr(s: &str) -> String {
    let mut v = Vec::with_capacity(s.len() + 8);
    put_str(&mut v, s, Esc::Attr);
    String::from_utf8(v).unwrap_or_default()
}

/// `escXml` of a Rust string.
pub(crate) fn esc_text(s: &str) -> String {
    let mut v = Vec::with_capacity(s.len() + 8);
    put_str(&mut v, s, Esc::Text);
    String::from_utf8(v).unwrap_or_default()
}

/// `/^\s|\s$/.test(s)` (JS white space).
pub(crate) fn edge_js_space(s: JsStr<'_>) -> bool {
    let n = s.len();
    n > 0 && (s.unit(0).is_some_and(uo_jsnum::text::is_js_space_unit) || s.unit(n - 1).is_some_and(uo_jsnum::text::is_js_space_unit))
}

/// `/^[ \t\n\r]|[ \t\n\r]$/.test(s)`.
pub(crate) fn edge_xml_space(s: JsStr<'_>) -> bool {
    let n = s.len();
    let sp = |u: Option<u16>| matches!(u, Some(0x20 | 0x09 | 0x0A | 0x0D));
    n > 0 && (sp(s.unit(0)) || sp(s.unit(n - 1)))
}

/// A cell reference writer: column letters cached per column.
#[derive(Default)]
pub(crate) struct ColLetters {
    cache: Vec<([u8; 3], u8)>,
}

impl ColLetters {
    /// The letters of 0-based column `col`.
    pub(crate) fn get(&mut self, col: u32) -> ([u8; 3], usize) {
        let c = col as usize;
        if self.cache.len() <= c {
            self.cache.resize(c + 1, ([0; 3], 0));
        }
        if self.cache[c].1 == 0 {
            let mut letters = [0u8; 8];
            let mut n = 0;
            let mut k = col + 1;
            while k > 0 {
                letters[n] = b'A' + ((k - 1) % 26) as u8;
                n += 1;
                k = (k - 1) / 26;
            }
            let mut e = ([0u8; 3], 0u8);
            for (j, l) in letters[..n.min(3)].iter().rev().enumerate() {
                e.0[j] = *l;
            }
            e.1 = n.min(3) as u8;
            self.cache[c] = e;
        }
        let e = self.cache[c];
        (e.0, usize::from(e.1))
    }
}
