//! The font table ([MS-DOC] `SttbfFfn`, `fonts.ts`): an STTB of FFN. The index is the `ftc` the
//! font sprms use (`sprmCRgFtc0/1/2`, `sprmCFtcBi`, `sprmCSymbol`).

use super::bytes::ByteReader;
use super::fib::{Fib, fc};
use crate::text::Utf16;

/// One font of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocFont {
    pub name: Utf16,
    pub alt_name: Option<Utf16>,
    /// 0 default, 1 fixed, 2 variable
    pub prq: u8,
    pub true_type: bool,
    /// 0 auto/don't care, 1 roman, 2 swiss, 3 modern, 4 script, 5 decorative
    pub ff: u8,
    pub weight: i16,
    pub charset: u8,
    /// the PANOSE bytes (10 when the record is complete)
    pub panose: Vec<u8>,
    /// FONTSIGNATURE: usb0..3, csb0..1
    pub sig: [u32; 6],
}

/// A zero-terminated UTF-16 string in `[off, end)` and the offset after it (`cstr`).
fn cstr(r: ByteReader<'_>, off: u64, end: u64) -> (Vec<u16>, u64) {
    let mut s = Vec::new();
    let mut o = off;
    while o + 1 < end {
        let c = r.u16(o);
        o += 2;
        if c == 0 {
            break;
        }
        s.push(c);
    }
    (s, o)
}

/// The alternative name of a font with garbage in it (the 0xFD/0xCD/0xDD patterns of the MSVC
/// debug heap, e.g. U+FDFD repeated) or a control: the Word discards it. The TS tests one regex
/// class over code units: 0x0000-0x001F, 0xFDFD, 0xCDCD, 0xDDDD (a lone surrogate, the regex has
/// no `u` flag), 0xFFFD and 0xFFFF.
fn is_garbage_unit(c: u16) -> bool {
    c <= 0x1f || matches!(c, 0xfdfd | 0xcdcd | 0xdddd | 0xfffd | 0xffff)
}

/// `readFonts`.
#[must_use]
pub fn read_fonts(fib: &Fib, table: &[u8]) -> Vec<DocFont> {
    let p = fib.fc_lcb(fc::SttbfFfn);
    let (fc0, lcb) = (u64::from(p.fc), u64::from(p.lcb));
    let r = ByteReader::new(table);
    if lcb == 0 || fc0 + lcb > r.len() {
        return Vec::new();
    }
    let mut off = fc0;
    let end = fc0 + lcb;
    let mut c_data = r.u16(off);
    off += 2;
    let mut extended = false;
    if c_data == 0xffff {
        extended = true;
        c_data = r.u16(off);
        off += 2;
    }
    let cb_extra = u64::from(r.u16(off));
    off += 2;
    let mut fonts = Vec::new();
    let mut i = 0u32;
    while i < u32::from(c_data) && off < end {
        let cb = if extended { u64::from(r.u16(off)) * 2 } else { u64::from(r.u8(off)) };
        off += if extended { 2 } else { 1 };
        let o = off;
        let ffid = r.u8(o);
        let name_start = o + 39;
        let lim = (o + cb).min(end);
        let (name, next) = cstr(r, name_start, lim);
        let ix_alt = r.u8(o + 4);
        let mut alt_name = None;
        if ix_alt != 0 {
            let (alt, _) = cstr(r, next, lim);
            if !alt.is_empty() && !alt.iter().any(|&c| is_garbage_unit(c)) {
                alt_name = Some(Utf16::from_units(alt));
            }
        }
        let mut sig = [0u32; 6];
        for (k, s) in sig.iter_mut().enumerate() {
            *s = r.u32(o + 15 + k as u64 * 4);
        }
        fonts.push(DocFont {
            name: if name.is_empty() { Utf16::from(format!("Font{i}")) } else { Utf16::from_units(name) },
            alt_name,
            prq: ffid & 3,
            true_type: ffid & 4 != 0,
            ff: (ffid >> 4) & 7,
            weight: r.i16(o + 1),
            charset: r.u8(o + 3),
            panose: r.sub(o + 5, o + 15).to_vec(),
            sig,
        });
        off = o + cb + cb_extra;
        i += 1;
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::fib::read_fib;

    fn fib_with(fc0: u32, lcb: u32) -> Fib {
        let mut w = vec![0u8; 0x300];
        w[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        w[32..34].copy_from_slice(&14u16.to_le_bytes());
        w[62..64].copy_from_slice(&22u16.to_le_bytes());
        w[152..154].copy_from_slice(&93u16.to_le_bytes());
        let at = 154 + fc::SttbfFfn * 8;
        w[at..at + 4].copy_from_slice(&fc0.to_le_bytes());
        w[at + 4..at + 8].copy_from_slice(&lcb.to_le_bytes());
        read_fib(&w).unwrap()
    }

    fn ffn(name: &str, alt: Option<&[u16]>, ffid: u8) -> Vec<u8> {
        let mut b = vec![0u8; 39];
        b[0] = ffid;
        b[1..3].copy_from_slice(&400i16.to_le_bytes());
        b[3] = 0xee;
        for (i, v) in (1..=10u8).enumerate() {
            b[5 + i] = v;
        }
        for k in 0..6 {
            b[15 + k * 4..19 + k * 4].copy_from_slice(&(k as u32 + 1).to_le_bytes());
        }
        for u in name.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        b.extend([0, 0]);
        if let Some(alt) = alt {
            b[4] = 1;
            for u in alt {
                b.extend(u.to_le_bytes());
            }
            b.extend([0, 0]);
        }
        b
    }

    fn sttb(fonts: &[Vec<u8>]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend((fonts.len() as u16).to_le_bytes());
        t.extend(0u16.to_le_bytes());
        for f in fonts {
            t.push(f.len() as u8);
            t.extend(f);
        }
        t
    }

    #[test]
    fn reads_names_alternatives_and_flags() {
        let alt: Vec<u16> = "Alt".encode_utf16().collect();
        let t = sttb(&[ffn("Arial", Some(&alt), 0x26), ffn("", None, 0), ffn("Bad", Some(&[0xfdfd, 0x41]), 0)]);
        let fonts = read_fonts(&fib_with(0, t.len() as u32), &t);
        assert_eq!(fonts.len(), 3);
        assert!(fonts[0].name.eq_str("Arial"));
        assert!(fonts[0].alt_name.as_ref().unwrap().eq_str("Alt"));
        assert_eq!((fonts[0].prq, fonts[0].true_type, fonts[0].ff), (2, true, 2));
        assert_eq!((fonts[0].weight, fonts[0].charset), (400, 0xee));
        assert_eq!(fonts[0].panose, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(fonts[0].sig, [1, 2, 3, 4, 5, 6]);
        assert!(fonts[1].name.eq_str("Font1"));
        assert!(fonts[2].alt_name.is_none());
    }

    #[test]
    fn a_table_past_the_stream_is_empty() {
        assert!(read_fonts(&fib_with(0, 0), &[0; 8]).is_empty());
        assert!(read_fonts(&fib_with(4, 100), &[0; 8]).is_empty());
    }

    #[test]
    fn the_garbage_class() {
        for c in [0u16, 0x1f, 0xfdfd, 0xcdcd, 0xdddd, 0xfffd, 0xffff] {
            assert!(is_garbage_unit(c));
        }
        for c in [0x20u16, 0x41, 0xfdfc, 0xfffe] {
            assert!(!is_garbage_unit(c));
        }
    }
}
