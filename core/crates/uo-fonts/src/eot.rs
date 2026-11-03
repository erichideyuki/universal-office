//! Embedded OpenType (EOT) 2.2 writing, uncompressed: the container PowerPoint uses in
//! `ppt/fonts/*.fntdata`. PowerPoint writes MicroType Express; measured on the reference program,
//! an EOT without compression opens without repair and is kept when saved again. The reader
//! (with MTX) stays deferred (G3); `eot::read` will live here.
//!
//! Layout (all little-endian): a fixed header of 82 bytes (EOTSize, FontDataSize, Version
//! 0x00020002, Flags, PANOSE, Charset, Italic, Weight, fsType, the magic 0x504C, the four Unicode
//! ranges, the two code page ranges, CheckSumAdjustment, four reserved words, Padding1), then
//! the four names (family, style, version, full) as a size and UTF-16LE code units with a zero
//! word between them, and the root string, signature and EUDC fields all empty, then the font.

use crate::error::Result;
use crate::info::{FaceInfo, face_info};
use uo_jsnum::conv::to_uint8;
use uo_jsnum::parse::parse_int_utf16;

const TTEMBED_SUBSET: u32 = 0x1;

/// The sfnt wrapped as EOT 2.2, uncompressed. `subsetted` sets the `TTEMBED_SUBSET` flag. The
/// OS/2, head and name values come from [`face_info`], whose `Err` (a malformed table) is the
/// uncaught TS throw.
pub fn sfnt_to_eot(font: &[u8], subsetted: bool) -> Result<Vec<u8>> {
    let info = face_info(font)?;
    Ok(eot_with_info(font, &info, subsetted))
}

/// `n` modulo 2^32, what `DataView.setUint32` stores.
fn wrap32(n: usize) -> u32 {
    u32::try_from((n as u64) & 0xFFFF_FFFF).unwrap_or(0)
}

/// One byte of the PANOSE: `parseInt(hex.slice(2i, 2i + 2), 16)` stored in a `Uint8Array`. The hex
/// is always valid, but clamped at the end of the input (S10), so the pair can be short or empty
/// (`NaN`, stored as 0).
fn panose_byte(units: &[u16], i: usize) -> u8 {
    let start = (i * 2).min(units.len());
    let end = (i * 2 + 2).min(units.len());
    to_uint8(parse_int_utf16(units.get(start..end).unwrap_or(&[]), 16))
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn set_u32(out: &mut [u8], at: usize, v: u32) {
    if let Some(slot) = out.get_mut(at..at + 4) {
        slot.copy_from_slice(&v.to_le_bytes());
    }
}

/// The EOT of a font whose values are already read (the part of [`sfnt_to_eot`] that does not
/// depend on the font parser).
pub(crate) fn eot_with_info(font: &[u8], info: &FaceInfo, subsetted: bool) -> Vec<u8> {
    let names = [&info.names.family, &info.names.style, &info.names.version, &info.names.full];
    let names_size: usize = names.iter().map(|n| 2 + n.len() * 2).sum();
    let mut out: Vec<u8> = Vec::with_capacity(82 + names_size + 6 + 4 + 20 + font.len());

    out.extend_from_slice(&[0u8; 8]); // EOTSize and FontDataSize, set at the end
    push_u32(&mut out, 0x0002_0002); // Version 2.2
    push_u32(&mut out, if subsetted { TTEMBED_SUBSET } else { 0 });
    let panose: Vec<u16> = info.panose.encode_utf16().collect();
    for i in 0..10 {
        out.push(panose_byte(&panose, i));
    }
    out.push(1); // Charset: DEFAULT_CHARSET
    out.push(u8::from(info.italic));
    push_u32(&mut out, u32::from(info.weight));
    push_u16(&mut out, info.fs_type);
    push_u16(&mut out, 0x504C); // MagicNumber
    for r in info.unicode_range {
        push_u32(&mut out, r);
    }
    for r in info.code_page_range {
        push_u32(&mut out, r);
    }
    push_u32(&mut out, info.check_sum_adjustment);
    out.resize(82, 0); // Reserved1-4 and Padding1

    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            push_u16(&mut out, 0); // Padding2..4 (Padding1 is in the fixed block)
        }
        push_u16(&mut out, u16::try_from((name.len() * 2) & 0xFFFF).unwrap_or(0));
        for unit in name.iter() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    push_u16(&mut out, 0); // Padding5
    push_u16(&mut out, 0); // RootStringSize: no site restriction
    push_u32(&mut out, 0); // RootStringCheckSum
    push_u32(&mut out, 0); // EUDCCodePage
    push_u16(&mut out, 0); // Padding6
    push_u16(&mut out, 0); // SignatureSize
    push_u32(&mut out, 0); // EUDCFlags
    push_u32(&mut out, 0); // EUDCFontSize
    out.extend_from_slice(font);

    let total = wrap32(out.len());
    set_u32(&mut out, 0, total);
    set_u32(&mut out, 4, wrap32(font.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::FaceNames;
    use uo_alloc::assert_no_leak;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn info() -> FaceInfo {
        FaceInfo {
            panose: "02040503050406030204".to_owned(),
            weight: 700,
            italic: true,
            fs_type: 0x0008,
            unicode_range: [0xA000_02FF, 0x5000_ECFB, 0x0000_0001, 0x0000_0000],
            code_page_range: [0x6000_019F, 0xDFD7_0000],
            check_sum_adjustment: 0x1234_5678,
            names: FaceNames {
                family: u16s("Carlito"),
                style: u16s("Bold Italic"),
                version: u16s("Version 1.0"),
                full: u16s("Carlito Bold Italic"),
            },
        }
    }

    struct Parsed {
        size: u32,
        font_size: u32,
        names: Vec<Vec<u16>>,
        font: Vec<u8>,
        rest: Vec<u8>,
    }

    fn le16(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn parse(eot: &[u8]) -> Parsed {
        let mut at = 82;
        let mut names = Vec::new();
        for i in 0..4 {
            if i > 0 {
                assert_eq!(le16(eot, at), 0, "padding {i}");
                at += 2;
            }
            let n = usize::from(le16(eot, at));
            at += 2;
            let units = eot[at..at + n].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            names.push(units);
            at += n;
        }
        let rest = eot[at..at + 24].to_vec();
        at += 24;
        Parsed { size: le32(eot, 0), font_size: le32(eot, 4), names, font: eot[at..].to_vec(), rest }
    }

    #[test]
    fn the_fixed_header_has_the_eot_2_2_fields() {
        let font = vec![0xAB; 37];
        let eot = eot_with_info(&font, &info(), true);
        assert_eq!(le32(&eot, 8), 0x0002_0002);
        assert_eq!(le32(&eot, 12), 1);
        assert_eq!(&eot[16..26], &[2, 4, 5, 3, 5, 4, 6, 3, 2, 4]);
        assert_eq!(eot[26], 1);
        assert_eq!(eot[27], 1);
        assert_eq!(le32(&eot, 28), 700);
        assert_eq!(le16(&eot, 32), 8);
        assert_eq!(le16(&eot, 34), 0x504C);
        assert_eq!([le32(&eot, 36), le32(&eot, 40), le32(&eot, 44), le32(&eot, 48)], [0xA000_02FF, 0x5000_ECFB, 1, 0]);
        assert_eq!([le32(&eot, 52), le32(&eot, 56)], [0x6000_019F, 0xDFD7_0000]);
        assert_eq!(le32(&eot, 60), 0x1234_5678);
        assert!(eot[64..82].iter().all(|&b| b == 0));
    }

    #[test]
    fn names_tail_and_sizes() {
        let font: Vec<u8> = (0..50u8).collect();
        let eot = eot_with_info(&font, &info(), false);
        assert_eq!(le32(&eot, 12), 0);
        let p = parse(&eot);
        assert_eq!(p.size as usize, eot.len());
        assert_eq!(p.font_size, 50);
        assert_eq!(p.font, font);
        assert_eq!(p.names, [u16s("Carlito"), u16s("Bold Italic"), u16s("Version 1.0"), u16s("Carlito Bold Italic")]);
        assert!(p.rest.iter().all(|&b| b == 0));
        assert_eq!(eot.len(), 82 + (2 + 14) + (2 + 22) + (2 + 22) + (2 + 38) + 6 + 4 + 20 + 50);
    }

    #[test]
    fn names_keep_lone_surrogates_and_non_ascii_as_utf16le() {
        let mut i = info();
        i.names.family = vec![0xD800, 0x00E9, 0x4E2D, 0x0041];
        i.names.style = Vec::new();
        let eot = eot_with_info(&[1, 2, 3], &i, false);
        let p = parse(&eot);
        assert_eq!(p.names[0], [0xD800, 0x00E9, 0x4E2D, 0x0041]);
        assert!(p.names[1].is_empty());
        assert_eq!(&eot[82..92], &[8, 0, 0x00, 0xD8, 0xE9, 0x00, 0x2D, 0x4E, 0x41, 0x00]);
    }

    #[test]
    fn a_clamped_panose_gives_zero_bytes_for_the_missing_pairs() {
        let mut i = info();
        i.panose = "0204A".to_owned();
        let eot = eot_with_info(&[], &i, false);
        assert_eq!(&eot[16..26], &[2, 4, 10, 0, 0, 0, 0, 0, 0, 0]);
        i.panose = String::new();
        assert_eq!(&eot_with_info(&[], &i, false)[16..26], &[0u8; 10]);
        i.panose = "0".repeat(20);
        assert_eq!(&eot_with_info(&[], &i, false)[16..26], &[0u8; 10]);
        i.panose = "FFFFFFFFFFFFFFFFFFFF".to_owned();
        assert_eq!(&eot_with_info(&[], &i, false)[16..26], &[255u8; 10]);
    }

    #[test]
    fn a_name_longer_than_65535_bytes_wraps_its_size_as_the_ts_does() {
        let mut i = info();
        i.names.full = vec![0x41; 32_768 + 3];
        let eot = eot_with_info(&[], &i, false);
        let padding_at = 82 + (2 + 14) + (2 + 2 + 22) + (2 + 2 + 22);
        assert_eq!(le16(&eot, padding_at), 0);
        assert_eq!(le16(&eot, padding_at + 2), 6);
        assert_eq!(le32(&eot, 0) as usize, eot.len());
    }

    #[test]
    fn m1_no_leak() {
        let font = vec![7u8; 100];
        let i = info();
        assert_no_leak(|| eot_with_info(&font, &i, true));
        assert_no_leak(|| eot_with_info(&[], &i, false));
        assert_no_leak(|| sfnt_to_eot(&font, true).is_ok());
    }
}
