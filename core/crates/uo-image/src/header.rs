//! Header reads: `jpegInfo` and `imagePixelSize` of `images.ts`.

use crate::{at, at0};

/// What the PDF needs of a JPEG without decoding it (`JpegInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegInfo {
    pub width: u32,
    pub height: u32,
    pub components: u32,
    /// the Adobe APP14 marker (inverted CMYK)
    pub adobe: bool,
}

/// The size in pixels of a raster image, from its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

/// `jpegInfo`: scans the markers up to the first SOFn (not DHT, JPG or DAC) for the size and the
/// number of components. `None` when the signature is wrong, the SOF is cut off or there is none.
pub fn jpeg_info(b: &[u8]) -> Option<JpegInfo> {
    if at(b, 0) != Some(0xff) || at(b, 1) != Some(0xd8) {
        return None;
    }
    let n = b.len() as u64;
    let mut i: u64 = 2;
    let mut adobe = false;
    while i + 4 < n {
        if at(b, i) != Some(0xff) {
            i += 1;
            continue;
        }
        let m = at0(b, i + 1);
        // a repeated 0xFF is fill before the marker
        if m == 0xff {
            i += 1;
            continue;
        }
        if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) {
            i += 2;
            continue;
        }
        let len = (at0(b, i + 2) << 8) | at0(b, i + 3);
        if m == 0xee && at(b, i + 4) == Some(0x41) && at(b, i + 5) == Some(0x64) {
            adobe = true;
        }
        if (0xc0..=0xcf).contains(&m) && m != 0xc4 && m != 0xc8 && m != 0xcc {
            // truncated SOF
            if i + 9 >= n {
                return None;
            }
            let height = (at0(b, i + 5) << 8) | at0(b, i + 6);
            let width = (at0(b, i + 7) << 8) | at0(b, i + 8);
            let components = at0(b, i + 9);
            return Some(JpegInfo { width, height, components, adobe });
        }
        i += 2 + u64::from(len);
    }
    None
}

/// `imagePixelSize`: the size from the header of a JPEG, PNG, GIF or BMP (by the first bytes, as
/// the TS does: `0x89 0x50`, `GI`, `BM`). `None` for anything else.
pub fn image_pixel_size(b: &[u8]) -> Option<PixelSize> {
    if let Some(jp) = jpeg_info(b) {
        return Some(PixelSize { width: jp.width, height: jp.height });
    }
    let n = b.len();
    if n > 24 && at(b, 0) == Some(0x89) && at(b, 1) == Some(0x50) {
        return Some(PixelSize { width: be32(b, 16), height: be32(b, 20) });
    }
    if n > 10 && at(b, 0) == Some(0x47) && at(b, 1) == Some(0x49) {
        return Some(PixelSize { width: at0(b, 6) | (at0(b, 7) << 8), height: at0(b, 8) | (at0(b, 9) << 8) });
    }
    if n > 26 && at(b, 0) == Some(0x42) && at(b, 1) == Some(0x4d) {
        return Some(PixelSize { width: abs_le32(b, 18), height: abs_le32(b, 22) });
    }
    None
}

fn be32(b: &[u8], i: u64) -> u32 {
    (at0(b, i) << 24) | (at0(b, i + 1) << 16) | (at0(b, i + 2) << 8) | at0(b, i + 3)
}

/// `Math.abs(b[i] | b[i+1] << 8 | b[i+2] << 16 | b[i+3] << 24)`: the int32, then its absolute
/// value (2^31 for the minimum).
fn abs_le32(b: &[u8], i: u64) -> u32 {
    let v = at0(b, i) | (at0(b, i + 1) << 8) | (at0(b, i + 2) << 16) | (at0(b, i + 3) << 24);
    i32::from_ne_bytes(v.to_ne_bytes()).unsigned_abs()
}
