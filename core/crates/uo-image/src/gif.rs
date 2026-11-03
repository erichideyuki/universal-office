//! `decodeGif` and `lzw` of `images.ts`: the first frame of a GIF, cropped to the screen.

use crate::lzw::Lzw;
use crate::{ImageError, ImageLimits, LimitKind, Result, Rgba, at, at0, sub, zeroed};

/// Decodes the first frame of a GIF to RGBA. `Unsupported` for another format, a minimum code
/// size outside 1..=11 or a file with no frame (the TS returns `null`).
pub fn decode_gif(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    if at(b, 0) != Some(0x47) || at(b, 1) != Some(0x49) || at(b, 2) != Some(0x46) {
        return Err(ImageError::Unsupported);
    }
    let n = b.len() as u64;
    let width = at0(b, 6) | (at0(b, 7) << 8);
    let height = at0(b, 8) | (at0(b, 9) << 8);
    let flags = at0(b, 10);
    let mut pos: u64 = 13;
    let mut gct: Option<&[u8]> = None;
    if flags & 0x80 != 0 {
        let len = 3u64 << ((flags & 7) + 1);
        gct = Some(sub(b, pos, pos + len));
        pos += len;
    }
    let mut transparent: i32 = -1;
    limits.check_size(u64::from(width), u64::from(height))?;
    let mut out = zeroed(u64::from(width) * u64::from(height) * 4)?;
    while pos < n {
        let t = at0(b, pos);
        pos += 1;
        if t == 0x21 {
            let label = at0(b, pos);
            pos += 1;
            if label == 0xf9 && at0(b, pos + 1) & 1 != 0 {
                transparent = at(b, pos + 4).map_or(-1, i32::from);
            }
            while pos < n && at0(b, pos) != 0 {
                pos += at0(b, pos) as u64 + 1;
            }
            pos += 1;
        } else if t == 0x2c {
            let ix = u64::from(at0(b, pos) | (at0(b, pos + 1) << 8));
            let iy = u64::from(at0(b, pos + 2) | (at0(b, pos + 3) << 8));
            let iw = u64::from(at0(b, pos + 4) | (at0(b, pos + 5) << 8));
            let ih = u64::from(at0(b, pos + 6) | (at0(b, pos + 7) << 8));
            let lf = at0(b, pos + 8);
            pos += 9;
            let mut ct = gct;
            if lf & 0x80 != 0 {
                let len = 3u64 << ((lf & 7) + 1);
                ct = Some(sub(b, pos, pos + len));
                pos += len;
            }
            let interlaced = lf & 0x40 != 0;
            let min_code = at(b, pos).map(u32::from);
            pos += 1;
            // a minimum code size outside 1..=11 is an invalid file (the dictionary would have 2^n entries)
            let Some(min_code @ 1..=11) = min_code else { return Err(ImageError::Unsupported) };
            // the data blocks, copied once into a buffer of the exact size
            let mut total: u64 = 0;
            let mut scan = pos;
            while scan < n && at0(b, scan) != 0 {
                total += u64::from(at0(b, scan));
                scan += u64::from(at0(b, scan)) + 1;
            }
            let mut chunks: Vec<u8> = Vec::new();
            chunks
                .try_reserve_exact(usize::try_from(total).map_err(|_| ImageError::Limit(LimitKind::Allocation))?)
                .map_err(|_| ImageError::Limit(LimitKind::Allocation))?;
            while pos < n && at0(b, pos) != 0 {
                let len = u64::from(at0(b, pos));
                // a block cut off by the end of the file reads as zeros (`Uint8Array.from` of `undefined`)
                for k in 1..=len {
                    chunks.push(at(b, pos + k).unwrap_or(0));
                }
                pos += len + 1;
            }
            let frame_px = limits.check_size(iw, ih)?;
            let idx = lzw(&chunks, min_code, frame_px)?;
            drop(chunks);
            let rows = frame_rows(ih, interlaced);
            for (r, &dest) in rows.iter().enumerate() {
                let yy = iy + u64::from(dest);
                if yy >= u64::from(height) {
                    continue;
                }
                for x in 0..iw {
                    if ix + x >= u64::from(width) {
                        break;
                    }
                    let ci = at(&idx, r as u64 * iw + x).unwrap_or(0);
                    let k = (yy * u64::from(width) + ix + x) * 4;
                    let Some(px) = usize::try_from(k).ok().and_then(|k| out.get_mut(k..k + 4)) else { continue };
                    let color = ct.filter(|_| i32::from(ci) != transparent);
                    match color {
                        None => {
                            if let Some(a) = px.get_mut(3) {
                                *a = 0;
                            }
                        }
                        Some(ct) => {
                            let c = u64::from(ci) * 3;
                            px.copy_from_slice(&[at(ct, c).unwrap_or(0), at(ct, c + 1).unwrap_or(0), at(ct, c + 2).unwrap_or(0), 255]);
                        }
                    }
                }
            }
            return Ok(Rgba { width, height, data: out });
        } else {
            break;
        }
    }
    Err(ImageError::Unsupported)
}

/// The destination row of each row of the frame: in order, or the four interlace passes.
fn frame_rows(ih: u64, interlaced: bool) -> Vec<u32> {
    let mut rows = Vec::new();
    let ih = u32::try_from(ih).unwrap_or(0);
    if interlaced {
        for (start, step) in [(0u32, 8u32), (4, 8), (2, 4), (1, 2)] {
            let mut r = start;
            while r < ih {
                rows.push(r);
                r += step;
            }
        }
    } else {
        rows.extend(0..ih);
    }
    rows
}

/// The GIF LZW decoder of the TS: `n` bytes out (zero where the data ends early), codes read
/// least significant bit first, the code size growing at the dictionary sizes, a code beyond the
/// dictionary read as the previous entry plus its first byte, whatever the code.
fn lzw(data: &[u8], min_code: u32, n: u64) -> Result<Vec<u8>> {
    let mut out = zeroed(n)?;
    let n = out.len();
    let clear = 1usize << min_code;
    let eoi = clear + 1;
    let mut size = min_code + 1;
    let mut lz = Lzw::new(clear);
    let total_bits = data.len() as u64 * 8;
    let (mut bit, mut o) = (0u64, 0usize);
    while o < n {
        let mut code = 0usize;
        for k in 0..u64::from(size) {
            if (at0(data, (bit + k) >> 3) >> ((bit + k) & 7)) & 1 != 0 {
                code |= 1 << k;
            }
        }
        bit += u64::from(size);
        if bit > total_bits {
            break;
        }
        if code == clear {
            lz.reset();
            size = min_code + 1;
            continue;
        }
        if code == eoi {
            break;
        }
        let Some(entry) = lz.entry(code) else { break };
        let take = entry.len().min(n - o);
        if let (Some(dst), Some(src)) = (out.get_mut(o..o + take), entry.get(..take)) {
            dst.copy_from_slice(src);
        }
        o += take;
        lz.advance();
        if lz.len() >= (1usize << size) && size < 12 {
            size += 1;
        }
    }
    Ok(out)
}
