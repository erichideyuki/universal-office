//! Pictures: the BLIP store (`BStoreContainer` → `FBSE`, [MS-ODRAW] 2.2.20/2.2.32) and the
//! BLIP records turned into image files (`readFbse`, `readBStore`, `decodeBlip` of
//! `docx-core/src/doc/officeart.ts`).
//!
//! A metafile BLIP (EMF, WMF, PICT) is usually deflated: it is inflated with a cap per picture
//! ([`OdrawLimits::max_blip_bytes`], the TS `MAX_BLIP`) and the document's [`BlipBudget`] (the
//! TS `blipBudget`), zlib first and raw DEFLATE second, as the TS `inflateCapped`
//! (`uo_opc::flate::inflate_capped`). Raster BLIPs are returned as borrowed slices of the input.

use std::borrow::Cow;

use uo_jsnum::conv::{to_int16, to_uint16};
use uo_jsnum::math::{max, min, round};

use super::record::{Rec, read_rec, rt, sub, u16_at, u32_at};
use super::{Flavor, OdrawLimits};

/// One BLIP store entry (`OfficeArtFBSE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bse<'a> {
    /// `btWin32`: the BLIP type for Windows
    pub bt_win32: u8,
    /// `size` of the BLIP
    pub size: u32,
    /// `cRef`: references to the entry (read by Slides)
    pub c_ref: u32,
    /// `foDelay`: offset of the BLIP in the delay stream (Word: the WordDocument stream;
    /// Slides: "Pictures"); `0xFFFFFFFF` when there is none
    pub fo_delay: u32,
    /// the BLIP stored inside the FBSE (after the name)
    pub embedded: Option<Rec<'a>>,
}

/// `OfficeArtFBSE` (an already read record) → entry (`readFbse`). The embedded BLIP is the
/// record after the 36 fixed bytes and the name, kept when its type is a BLIP type.
pub fn read_fbse<'a>(c: &Rec<'a>) -> Bse<'a> {
    let o = c.start;
    let cb_name = u64::from(c.u8(o + 33));
    let emb_off = o + 36 + cb_name;
    let embedded = if emb_off + 8 <= c.end { read_rec(c.data, emb_off, c.end).filter(Rec::is_blip) } else { None };
    Bse { bt_win32: c.u8(o), size: c.u32(o + 20), c_ref: c.u32(o + 24), fo_delay: c.u32(o + 28), embedded }
}

/// `BStoreContainer` → entries, one per child, `None` for a child that is not an FBSE
/// (`readBStore`; the index is `pib - 1`).
pub fn read_bstore<'a>(rec: &Rec<'a>) -> Vec<Option<Bse<'a>>> {
    rec.children().map(|c| (c.rec_type == rt::FBSE).then(|| read_fbse(&c))).collect()
}

/// The image format of a decoded BLIP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlipKind {
    Png,
    Jpeg,
    Tiff,
    /// a DIB with the `BITMAPFILEHEADER` added
    Bmp,
    Emf,
    /// with the Aldus placeable header added
    Wmf,
    Pict,
}

impl BlipKind {
    /// The file extension the TS gives (`ext`).
    pub fn ext(self) -> &'static str {
        match self {
            BlipKind::Png => "png",
            BlipKind::Jpeg => "jpeg",
            BlipKind::Tiff => "tiff",
            BlipKind::Bmp => "bmp",
            BlipKind::Emf => "emf",
            BlipKind::Wmf => "wmf",
            BlipKind::Pict => "pict",
        }
    }

    /// The content type the TS gives (`mime`).
    pub fn mime(self) -> &'static str {
        match self {
            BlipKind::Png => "image/png",
            BlipKind::Jpeg => "image/jpeg",
            BlipKind::Tiff => "image/tiff",
            BlipKind::Bmp => "image/bmp",
            BlipKind::Emf => "image/x-emf",
            BlipKind::Wmf => "image/x-wmf",
            BlipKind::Pict => "image/pict",
        }
    }
}

/// An image file extracted from a BLIP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blip<'a> {
    pub kind: BlipKind,
    /// the file bytes: borrowed from the input when nothing was added or inflated
    pub bytes: Cow<'a, [u8]>,
    /// PICT only: `ptSize` of the metafile header in EMU (`cx`, `cy`); Slides fits the picture
    /// frame to it (`fitPictFrame`, not part of this module)
    pub pict_size: Option<(i32, i32)>,
}

/// What a document may inflate in all (`blipBudget = { left }`); every inflated metafile is
/// subtracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlipBudget {
    pub left: u64,
}

impl BlipBudget {
    /// The budget of one document in the three TS readers: 512 MiB.
    pub const fn document() -> BlipBudget {
        BlipBudget { left: 512 << 20 }
    }

    /// No budget (`decodeBlip(rec)` without one): only the per-picture cap applies.
    pub const fn unlimited() -> BlipBudget {
        BlipBudget { left: u64::MAX }
    }
}

/// UIDs (16 bytes each) before the BLIP data, by type and instance (`blipUidCount`).
fn uid_count(rec_type: u16, inst: u16) -> u64 {
    let two = match rec_type {
        rt::BLIP_EMF => inst == 0x3d5,
        rt::BLIP_WMF => inst == 0x217,
        rt::BLIP_PICT => inst == 0x543,
        rt::BLIP_JPEG | rt::BLIP_JPEG_CMYK => inst == 0x46b || inst == 0x6e3,
        rt::BLIP_PNG => inst == 0x6e1,
        rt::BLIP_DIB => inst == 0x7a9,
        rt::BLIP_TIFF => inst == 0x6e5,
        _ => false,
    };
    if two { 2 } else { 1 }
}

/// A BLIP record → image file bytes (`decodeBlip`); `None` for a type that is not a picture or
/// a metafile that inflates to nothing usable (both attempts failed, the cap or the budget
/// exceeded). The flavors differ in the DIB header defaults and the WMF placeable header
/// ([`Flavor`]).
pub fn decode_blip<'a>(rec: &Rec<'a>, budget: &mut BlipBudget, flavor: Flavor, limits: &OdrawLimits) -> Option<Blip<'a>> {
    let uids = uid_count(rec.rec_type, rec.inst) * 16;
    let data = |off: u64| sub(rec.data, rec.start + off, rec.end);
    let simple = |kind, bytes: &'a [u8]| Some(Blip { kind, bytes: Cow::Borrowed(bytes), pict_size: None });
    match rec.rec_type {
        rt::BLIP_PNG => simple(BlipKind::Png, data(uids + 1)),
        rt::BLIP_JPEG | rt::BLIP_JPEG_CMYK => simple(BlipKind::Jpeg, data(uids + 1)),
        rt::BLIP_TIFF => simple(BlipKind::Tiff, data(uids + 1)),
        rt::BLIP_DIB => Some(Blip { kind: BlipKind::Bmp, bytes: Cow::Owned(dib_to_bmp(data(uids + 1), flavor)), pict_size: None }),
        rt::BLIP_EMF | rt::BLIP_WMF | rt::BLIP_PICT => {
            // OfficeArtMetafileHeader (34 bytes): cbSize, rcBounds, ptSize, cbSave, compression, filter
            let h = rec.start + uids;
            let cb_size = rec.u32(h);
            let compression = rec.u8(h + 32);
            let raw = sub(rec.data, h + 34, rec.end);
            let mut bytes: Cow<'a, [u8]> = Cow::Borrowed(raw);
            if compression == 0 {
                let cap = limits.max_blip_bytes.min(budget.left);
                let out = uo_opc::flate::inflate_capped(raw, cap)?;
                budget.left -= (out.len() as u64).min(budget.left);
                bytes = Cow::Owned(out);
            }
            if cb_size > 0 && u64::from(cb_size) < bytes.len() as u64 {
                let n = usize::try_from(cb_size).unwrap_or(usize::MAX);
                bytes = match bytes {
                    Cow::Borrowed(b) => Cow::Borrowed(b.get(..n).unwrap_or(b)),
                    Cow::Owned(mut v) => {
                        v.truncate(n);
                        v.shrink_to_fit();
                        Cow::Owned(v)
                    }
                };
            }
            match rec.rec_type {
                rt::BLIP_EMF => Some(Blip { kind: BlipKind::Emf, bytes, pict_size: None }),
                rt::BLIP_WMF => {
                    let bounds = [rec.i32(h + 4), rec.i32(h + 8), rec.i32(h + 12), rec.i32(h + 16)];
                    let inch = match flavor {
                        Flavor::Word => word_wmf_inch(bounds, rec.i32(h + 20)),
                        Flavor::Slides | Flavor::Sheet => 1440.0,
                    };
                    let placeable = flavor == Flavor::Word && bytes.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]);
                    let bytes = if placeable { bytes } else { Cow::Owned(with_placeable_header(&bytes, bounds, inch)) };
                    Some(Blip { kind: BlipKind::Wmf, bytes, pict_size: None })
                }
                _ => Some(Blip { kind: BlipKind::Pict, bytes, pict_size: Some((rec.i32(h + 20), rec.i32(h + 24))) }),
            }
        }
        _ => None,
    }
}

/// Word: units per inch of the placeable header from the metafile's `ptSize` (EMU): the width
/// of `rcBounds` over the physical width (`Math.round(((r - l) * 914400) / ptX)`, every factor
/// exact in `f64`), else 1,440.
fn word_wmf_inch(b: [i32; 4], pt_x: i32) -> f64 {
    let [l, _, r, _] = b;
    if pt_x > 0 && r > l { round(((f64::from(r) - f64::from(l)) * 914_400.0) / f64::from(pt_x)) } else { 1440.0 }
}

/// DIB (`BITMAPINFOHEADER` + pixels) → `.bmp` (`dibToBmp`): the 14-byte `BITMAPFILEHEADER`
/// with the file size and the offset of the pixels. Header fields missing from a short DIB are
/// 40, 24, 0, 0; [`Flavor::Sheet`] also takes 40 and 24 for a zero `biSize`/`biBitCount`.
fn dib_to_bmp(dib: &[u8], flavor: Flavor) -> Vec<u8> {
    let n = dib.len();
    let (mut hdr_size, mut bit_count) = (if n >= 4 { u32_at(dib, 0) } else { 40 }, if n >= 16 { u16_at(dib, 14) } else { 24 });
    if flavor == Flavor::Sheet {
        if hdr_size == 0 {
            hdr_size = 40;
        }
        if bit_count == 0 {
            bit_count = 24;
        }
    }
    let compression = if n >= 20 { u32_at(dib, 16) } else { 0 };
    let mut clr_used = u64::from(if n >= 36 { u32_at(dib, 32) } else { 0 });
    if clr_used == 0 && bit_count <= 8 {
        clr_used = 1 << bit_count;
    }
    let masks = if compression == 3 && hdr_size == 40 { 12 } else { 0 };
    let off_bits = 14 + u64::from(hdr_size) + masks + clr_used * 4;
    let total = 14 + n as u64;
    let mut out = Vec::with_capacity(14 + n);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&wrap_u32(total).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&wrap_u32(off_bits).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// `DataView.setUint32` of an integer: modulo 2^32.
fn wrap_u32(v: u64) -> u32 {
    u32::try_from(v & 0xffff_ffff).unwrap_or(0)
}

/// The WMF of a BLIP has no Aldus placeable header: one is built from `rcBounds`
/// (`withPlaceableHeader`): key, bounding box as `i16` (wrapped, `setInt16`), units per inch
/// clamped to 1..=65535, checksum (XOR of the first ten words).
fn with_placeable_header(wmf: &[u8], b: [i32; 4], inch: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + wmf.len());
    out.extend_from_slice(&0x9ac6_cdd7u32.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for v in b {
        out.extend_from_slice(&to_int16(f64::from(v)).to_le_bytes());
    }
    out.extend_from_slice(&to_uint16(max(1.0, min(65535.0, inch))).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    let mut sum = 0u16;
    for i in 0..10u64 {
        sum ^= u16_at(&out, i * 2);
    }
    out.extend_from_slice(&sum.to_le_bytes());
    out.extend_from_slice(wmf);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeable_header_checksum_and_clamp() {
        let out = with_placeable_header(&[1, 2], [0, 0, 70_000, -5], 0.0);
        assert_eq!(out.len(), 24);
        assert_eq!(&out[..4], &[0xd7, 0xcd, 0xc6, 0x9a]);
        // 70000 wraps to 4464 as i16
        assert_eq!(i16::from_le_bytes([out[10], out[11]]), 4464);
        assert_eq!(u16::from_le_bytes([out[14], out[15]]), 1);
        let mut sum = 0u16;
        for i in 0..10 {
            sum ^= u16::from_le_bytes([out[i * 2], out[i * 2 + 1]]);
        }
        assert_eq!(u16::from_le_bytes([out[20], out[21]]), sum);
    }

    #[test]
    fn wmf_inch_large_values() {
        assert_eq!(word_wmf_inch([i32::MIN, 0, i32::MAX, 0], 1), 4_294_967_295.0 * 914_400.0);
        assert_eq!(word_wmf_inch([0, 0, 1440, 0], 914_400), 1440.0);
        assert_eq!(word_wmf_inch([0, 0, 3, 0], 2), 1_371_600.0);
        assert_eq!(word_wmf_inch([5, 0, 5, 0], 2), 1440.0);
    }

    #[test]
    fn dib_header_defaults_by_flavor() {
        let dib = [0u8; 40];
        let w = dib_to_bmp(&dib, Flavor::Word);
        let s = dib_to_bmp(&dib, Flavor::Sheet);
        // Word: biSize 0, bitCount 0 -> 1 << 0 = 1 colour: 14 + 0 + 4
        assert_eq!(u32::from_le_bytes([w[10], w[11], w[12], w[13]]), 18);
        // Sheet: 40 and 24 -> no palette
        assert_eq!(u32::from_le_bytes([s[10], s[11], s[12], s[13]]), 54);
        assert_eq!(u32::from_le_bytes([w[2], w[3], w[4], w[5]]), 54);
    }
}
