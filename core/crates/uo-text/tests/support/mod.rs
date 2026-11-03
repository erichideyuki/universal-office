//! Helpers for replaying the V8 captures in `tests/captures/`.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

/// A capture file: its lines split on tabs.
pub struct Capture {
    text: &'static str,
}

impl Capture {
    pub fn new(text: &'static str) -> Self {
        assert!(text.ends_with('\n') && !text.contains('\r'), "captures are LF text");
        Capture { text }
    }

    /// The fields after the kind of every line of that kind.
    pub fn lines(&self, kind: &'static str) -> impl Iterator<Item = Vec<&'static str>> + use<> {
        self.text.lines().filter_map(move |l| {
            let mut it = l.split('\t');
            (it.next() == Some(kind)).then(|| it.collect())
        })
    }

    /// The single line of that kind.
    pub fn one(&self, kind: &'static str) -> Vec<&'static str> {
        let mut it = self.lines(kind);
        let first = it.next().unwrap_or_else(|| panic!("no `{kind}` line"));
        assert!(it.next().is_none(), "several `{kind}` lines");
        first
    }

    /// The `@name value` header lines.
    pub fn header(&self, name: &str) -> &'static str {
        let prefix = format!("@{name} ");
        self.text.lines().find_map(|l| l.strip_prefix(prefix.as_str())).unwrap_or_else(|| panic!("no @{name} header"))
    }
}

/// `-` is the empty list; otherwise comma-separated hex numbers.
pub fn hex_list(field: &str) -> Vec<u32> {
    if field == "-" { Vec::new() } else { field.split(',').map(|h| u32::from_str_radix(h, 16).unwrap()).collect() }
}

/// UTF-16 code units from a capture field.
pub fn units(field: &str) -> Vec<u16> {
    hex_list(field).into_iter().map(|u| u16::try_from(u).unwrap()).collect()
}

/// `=` means "the same as `input`".
pub fn units_or_same(field: &str, input: &[u16]) -> Vec<u16> {
    if field == "=" { input.to_vec() } else { units(field) }
}

/// Ranges `a-b,c-d` (hex) as a membership table over 0..=10FFFF.
pub fn set_of(ranges: &str) -> Vec<bool> {
    let mut set = vec![false; 0x11_0000];
    if ranges == "-" {
        return set;
    }
    for r in ranges.split(',') {
        let (a, b) = r.split_once('-').unwrap();
        for cp in u32::from_str_radix(a, 16).unwrap()..=u32::from_str_radix(b, 16).unwrap() {
            set[cp as usize] = true;
        }
    }
    set
}

/// The code point as the UTF-16 of a string.
pub fn cp_units(cp: u32) -> Vec<u16> {
    let mut v = Vec::new();
    uo_text::utf16::push(&mut v, cp);
    v
}

/// FNV-1a 64 over strings of code units (two bytes each, little endian, 0xFFFF after each string): the digest
/// `js/capture-oracle.mjs` writes for the bulk checks (Hangul).
pub fn fnv_strings<'a>(strings: impl IntoIterator<Item = &'a [u16]>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut byte = |b: u8| h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    for s in strings {
        for &u in s {
            byte((u & 0xFF) as u8);
            byte((u >> 8) as u8);
        }
        byte(0xFF);
        byte(0xFF);
    }
    format!("{h:016x}")
}

pub fn first_units(cps: impl IntoIterator<Item = u32>) -> Vec<u16> {
    let mut v = Vec::new();
    for cp in cps {
        uo_text::utf16::push(&mut v, cp);
    }
    v
}
