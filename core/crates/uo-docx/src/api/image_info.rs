//! C# `ImageInfo`: format, size in pixels and resolution read from an image header (no
//! decoding). The C# reads with `ReadOnlySpan` slices that throw past the end
//! (`ArgumentOutOfRangeException`, caught → `None`); its `int` arithmetic wraps. Here every read
//! is checked and the arithmetic wraps the same way. `Math.Abs(int.MinValue)` (a BMP height)
//! throws `OverflowException` in the C#, which escapes `TryRead`: kept as an error.

use super::error::{ApiError, ErrorKind, Result};
use super::net;

/// What an image header says.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub extension: &'static str,
    pub content_type: &'static str,
    pub width_px: i32,
    pub height_px: i32,
    pub dpi_x: f64,
    pub dpi_y: f64,
}

impl ImageInfo {
    /// Physical width in EMU (by the image resolution; 96 dpi when it does not say).
    pub fn width_emu(&self) -> i64 {
        net::to_i64_saturating(net::round_half_even(f64::from(self.width_px) * 914_400.0 / self.dpi_x))
    }

    pub fn height_emu(&self) -> i64 {
        net::to_i64_saturating(net::round_half_even(f64::from(self.height_px) * 914_400.0 / self.dpi_y))
    }

    /// PNG, JPEG, GIF or BMP; `Ok(None)` for another format or an invalid header.
    pub fn try_read(d: &[u8]) -> Result<Option<ImageInfo>> {
        if d.len() >= 24 && d.get(..8) == Some(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A][..]) {
            return Ok(png(d));
        }
        if d.len() >= 4 && d.first() == Some(&0xFF) && d.get(1) == Some(&0xD8) {
            return Ok(jpeg(d));
        }
        if d.len() >= 10 && d.get(..3) == Some(b"GIF") {
            let w = u16le(d, 6).map(i32::from);
            let h = u16le(d, 8).map(i32::from);
            return Ok(match (w, h) {
                (Some(w), Some(h)) => {
                    valid(ImageInfo { extension: "gif", content_type: "image/gif", width_px: w, height_px: h, dpi_x: 96.0, dpi_y: 96.0 })
                }
                _ => None,
            });
        }
        if d.len() >= 26 && d.get(..2) == Some(b"BM") {
            return bmp(d);
        }
        Ok(None)
    }
}

fn valid(i: ImageInfo) -> Option<ImageInfo> {
    (i.width_px > 0 && i.height_px > 0 && i.dpi_x > 0.0 && i.dpi_y > 0.0).then_some(i)
}

fn at(d: &[u8], pos: i64, len: usize) -> Option<&[u8]> {
    let p = usize::try_from(pos).ok()?;
    d.get(p..p.checked_add(len)?)
}

fn a4(d: &[u8], pos: i64) -> Option<[u8; 4]> {
    at(d, pos, 4).and_then(|b| b.try_into().ok())
}
fn a2(d: &[u8], pos: i64) -> Option<[u8; 2]> {
    at(d, pos, 2).and_then(|b| b.try_into().ok())
}
fn i32be(d: &[u8], pos: i64) -> Option<i32> {
    a4(d, pos).map(i32::from_be_bytes)
}
fn u32be(d: &[u8], pos: i64) -> Option<u32> {
    a4(d, pos).map(u32::from_be_bytes)
}
fn u16be(d: &[u8], pos: i64) -> Option<u16> {
    a2(d, pos).map(u16::from_be_bytes)
}
fn u16le(d: &[u8], pos: i64) -> Option<u16> {
    a2(d, pos).map(u16::from_le_bytes)
}
fn i32le(d: &[u8], pos: i64) -> Option<i32> {
    a4(d, pos).map(i32::from_le_bytes)
}
fn byte(d: &[u8], pos: i64) -> Option<u8> {
    at(d, pos, 1).and_then(|b| b.first().copied())
}

fn png(d: &[u8]) -> Option<ImageInfo> {
    let width = i32be(d, 16)?;
    let height = i32be(d, 20)?;
    let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
    let len = i64::try_from(d.len()).ok()?;
    // C# `int` positions: the sums wrap, and a read at a negative or past-the-end position
    // throws (caught: no image)
    let mut pos: i32 = 8;
    while i64::from(pos.wrapping_add(8)) <= len {
        let length = i32be(d, i64::from(pos))?;
        if length < 0 {
            break;
        }
        let ty = at(d, i64::from(pos.wrapping_add(4)), 4)?;
        if ty == b"pHYs" && i64::from(pos.wrapping_add(8).wrapping_add(9)) <= len {
            let ppu_x = u32be(d, i64::from(pos.wrapping_add(8)))?;
            let ppu_y = u32be(d, i64::from(pos.wrapping_add(12)))?;
            if byte(d, i64::from(pos.wrapping_add(16)))? == 1 && ppu_x > 0 && ppu_y > 0 {
                dpi_x = f64::from(ppu_x) * 0.0254;
                dpi_y = f64::from(ppu_y) * 0.0254;
            }
            break;
        }
        if ty == b"IDAT" || ty == b"IEND" {
            break;
        }
        pos = pos.wrapping_add(12i32.wrapping_add(length));
    }
    valid(ImageInfo { extension: "png", content_type: "image/png", width_px: width, height_px: height, dpi_x, dpi_y })
}

fn jpeg(d: &[u8]) -> Option<ImageInfo> {
    let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
    let len = i64::try_from(d.len()).ok()?;
    let mut pos: i64 = 2;
    while pos + 4 <= len {
        if byte(d, pos)? != 0xFF {
            pos += 1;
            continue;
        }
        let marker = byte(d, pos + 1)?;
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        let length = i64::from(u16be(d, pos + 2)?);
        if marker == 0xE0 && pos + 4 + 12 <= len && at(d, pos + 4, 5) == Some(b"JFIF\0") {
            let units = byte(d, pos + 11)?;
            let x = i32::from(u16be(d, pos + 12)?);
            let y = i32::from(u16be(d, pos + 14)?);
            if x > 0 && y > 0 && (units == 1 || units == 2) {
                dpi_x = if units == 1 { f64::from(x) } else { f64::from(x) * 2.54 };
                dpi_y = if units == 1 { f64::from(y) } else { f64::from(y) * 2.54 };
            }
        }
        let sof = (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if sof && pos + 9 <= len {
            let height = i32::from(u16be(d, pos + 5)?);
            let width = i32::from(u16be(d, pos + 7)?);
            return valid(ImageInfo { extension: "jpeg", content_type: "image/jpeg", width_px: width, height_px: height, dpi_x, dpi_y });
        }
        if length < 2 {
            break;
        }
        pos += 2 + length;
    }
    None
}

fn bmp(d: &[u8]) -> Result<Option<ImageInfo>> {
    let Some(header) = i32le(d, 14) else { return Ok(None) };
    let (width, height);
    let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
    if header == 12 {
        let (Some(w), Some(h)) = (u16le(d, 18), u16le(d, 20)) else { return Ok(None) };
        width = i32::from(w);
        height = i32::from(h);
    } else {
        let (Some(w), Some(h)) = (i32le(d, 18), i32le(d, 22)) else { return Ok(None) };
        width = w;
        height = h
            .checked_abs()
            .ok_or_else(|| ApiError::new(ErrorKind::Overflow, "Negating the minimum value of a twos complement number is invalid."))?;
        if d.len() >= 46 {
            let (Some(px), Some(py)) = (i32le(d, 38), i32le(d, 42)) else { return Ok(None) };
            if px > 0 && py > 0 {
                dpi_x = f64::from(px) * 0.0254;
                dpi_y = f64::from(py) * 0.0254;
            }
        }
    }
    Ok(valid(ImageInfo { extension: "bmp", content_type: "image/bmp", width_px: width, height_px: height, dpi_x, dpi_y }))
}
