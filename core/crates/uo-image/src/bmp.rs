//! `decodeBmp` of `images.ts`: uncompressed and BITFIELDS bitmaps of 1 to 32 bits per pixel.
//!
//! Reads past the end of the file give 0, as the TS typed array does after a bit operation, so
//! a truncated bitmap keeps its decoded rows and the rest is opaque black (the palette of a
//! missing colour table is black). A bit depth the TS does not handle (for example 12) leaves
//! the pixels transparent black.

use uo_jsnum::conv::to_uint8;
use uo_jsnum::math;

use crate::{ImageError, ImageLimits, Result, Rgba, at0, zeroed};

/// `dv.getUint32(i, true)`: a read past the end throws.
fn le32(b: &[u8], i: u64) -> Result<u32> {
    match (crate::at(b, i), crate::at(b, i + 1), crate::at(b, i + 2), crate::at(b, i + 3)) {
        (Some(a), Some(b), Some(c), Some(d)) => Ok(u32::from_le_bytes([a, b, c, d])),
        _ => Err(ImageError::Malformed("BMP header read past the end")),
    }
}

fn le16(b: &[u8], i: u64) -> Result<u32> {
    match (crate::at(b, i), crate::at(b, i + 1)) {
        (Some(a), Some(b)) => Ok(u32::from(u16::from_le_bytes([a, b]))),
        _ => Err(ImageError::Malformed("BMP header read past the end")),
    }
}

/// `chan`: one channel of a 16 or 32 bit pixel by its mask, scaled to 8 bits.
fn chan(v: u32, m: u32, dflt: u8) -> u8 {
    if m == 0 {
        return dflt;
    }
    let shift = m.trailing_zeros();
    let bits = 32 - (m >> shift).leading_zeros();
    let max = if bits == 32 { 4_294_967_295.0 } else { f64::from((1u32 << bits) - 1) };
    to_uint8(math::round(f64::from((v & m) >> shift) * 255.0 / max))
}

/// Decodes a BMP to RGBA. `Unsupported` for another format, a compression other than none or
/// BITFIELDS, or a non-positive width or zero height (the TS returns `null`); `Malformed` when
/// the header is cut off (`DataView` throws).
pub fn decode_bmp(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    if at0(b, 0) != 0x42 || at0(b, 1) != 0x4d || b.len() < 2 {
        return Err(ImageError::Unsupported);
    }
    let off = u64::from(le32(b, 10)?);
    let width = i64::from(le32(b, 18)? as i32);
    let height = i64::from(le32(b, 22)? as i32);
    let bpp = u64::from(le16(b, 28)?);
    let comp = le32(b, 30)?;
    if comp != 0 && comp != 3 {
        return Err(ImageError::Unsupported);
    }
    if width <= 0 || height == 0 {
        return Err(ImageError::Unsupported);
    }
    let top_down = height < 0;
    let height = height.unsigned_abs();
    let width = width.unsigned_abs();
    limits.check_size(width, height)?;
    let stride = (width * bpp).div_ceil(32) * 4;
    let hdr = u64::from(le32(b, 14)?);
    let pal_off = 14 + hdr;
    // 16/32 bpp: channels by mask (BITFIELDS: after the 40-byte header, or inside V4/V5; without
    // BITFIELDS: 5-5-5 and 8-8-8, no alpha)
    let mut masks: [u32; 4] = if bpp == 16 { [0x7c00, 0x3e0, 0x1f, 0] } else { [0xff0000, 0xff00, 0xff, 0] };
    if comp == 3 && (bpp == 16 || bpp == 32) {
        masks = [le32(b, 54)?, le32(b, 58)?, le32(b, 62)?, if hdr >= 56 { le32(b, 66)? } else { 0 }];
    }
    let mut out = zeroed(width * height * 4)?;
    let mut k: usize = 0;
    for y in 0..height {
        let row = off + (if top_down { y } else { height - 1 - y }) * stride;
        for x in 0..width {
            let Some(px) = out.get_mut(k..k + 4) else { break };
            k += 4;
            match bpp {
                24 => {
                    let p = row + x * 3;
                    px.copy_from_slice(&[at0(b, p + 2) as u8, at0(b, p + 1) as u8, at0(b, p) as u8, 255]);
                }
                16 | 32 => {
                    let p = row + x * (bpp / 8);
                    let v = if bpp == 16 {
                        at0(b, p) | (at0(b, p + 1) << 8)
                    } else {
                        at0(b, p) | (at0(b, p + 1) << 8) | (at0(b, p + 2) << 16) | (at0(b, p + 3) << 24)
                    };
                    px.copy_from_slice(&[chan(v, masks[0], 0), chan(v, masks[1], 0), chan(v, masks[2], 0), chan(v, masks[3], 255)]);
                }
                0..=8 => {
                    let bit = x * bpp;
                    let shift = (8 - bpp as i64 - (bit & 7) as i64) as u32;
                    let v = u64::from((at0(b, row + (bit >> 3)) >> (shift & 31)) & ((1u32 << bpp) - 1));
                    let p = pal_off + v * 4;
                    px.copy_from_slice(&[at0(b, p + 2) as u8, at0(b, p + 1) as u8, at0(b, p) as u8, 255]);
                }
                _ => {}
            }
        }
    }
    Ok(Rgba { width: width as u32, height: height as u32, data: out })
}
