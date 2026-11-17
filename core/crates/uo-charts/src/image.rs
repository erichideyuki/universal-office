//! `image.ts` `imageDpi`: the resolution recorded in an image file (dots per inch), by which the Office measures
//! the "natural size" of a picture (the tile of a picture fill has that size). PNG `pHYs` (unit metre), JPEG JFIF
//! APP0 (1 = dpi, 2 = dpcm) and, with no unit in the JFIF, the EXIF `XResolution`/`YResolution`, BMP
//! `biX/YPelsPerMeter`. No information (or absurd values) is `None`; the caller uses 96 dpi.
//!
//! A read past the end of the buffer is the TS `undefined` in a bitwise expression: 0. `tilePattern` and
//! `flippedTile` (the other half of `image.ts`) are the shell painter's (record C-6).

/// `b[i]`, with `undefined` as 0 (every use is a bitwise or a compare with a non-zero constant).
fn at(b: &[u8], i: u64) -> u32 {
    usize::try_from(i).ok().and_then(|i| b.get(i)).map_or(0, |v| u32::from(*v))
}

fn u16be(b: &[u8], i: u64) -> u32 {
    (at(b, i) << 8) | at(b, i + 1)
}

fn u32be(b: &[u8], i: u64) -> u32 {
    (at(b, i) << 24) | (at(b, i + 1) << 16) | (at(b, i + 2) << 8) | at(b, i + 3)
}

/// `i32le` (signed, as the TS `|` and `<<` produce).
fn i32le(b: &[u8], i: u64) -> i32 {
    i32::from_ne_bytes(((at(b, i)) | (at(b, i + 1) << 8) | (at(b, i + 2) << 16) | (at(b, i + 3) << 24)).to_ne_bytes())
}

fn sane(d: Option<(f64, f64)>) -> Option<(f64, f64)> {
    d.filter(|(x, y)| *x >= 1.0 && *y >= 1.0 && *x <= 100_000.0 && *y <= 100_000.0)
}

/// `imageDpi(b)`: `(x, y)` in dots per inch.
pub fn image_dpi(b: &[u8]) -> Option<(f64, f64)> {
    let n = b.len() as u64;
    if n > 24 && at(b, 0) == 0x89 && at(b, 1) == 0x50 && at(b, 2) == 0x4e && at(b, 3) == 0x47 {
        return sane(png_dpi(b));
    }
    if n > 4 && at(b, 0) == 0xff && at(b, 1) == 0xd8 {
        return sane(jpeg_dpi(b));
    }
    if n > 46 && at(b, 0) == 0x42 && at(b, 1) == 0x4d {
        let x = i32le(b, 38);
        let y = i32le(b, 42);
        return if x > 0 && y > 0 { sane(Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254))) } else { None };
    }
    None
}

fn png_dpi(b: &[u8]) -> Option<(f64, f64)> {
    let n = b.len() as u64;
    let mut p: u64 = 8;
    while p + 12 <= n {
        let len = u64::from(u32be(b, p));
        let ty = [at(b, p + 4), at(b, p + 5), at(b, p + 6), at(b, p + 7)];
        if ty == [0x70, 0x48, 0x59, 0x73] && len >= 9 {
            let x = f64::from(u32be(b, p + 8));
            let y = f64::from(u32be(b, p + 12));
            let unit = at(b, p + 16);
            return if unit == 1 { Some((x * 0.0254, y * 0.0254)) } else { None };
        }
        if ty == [0x49, 0x44, 0x41, 0x54] || ty == [0x49, 0x45, 0x4e, 0x44] {
            return None;
        }
        p += 12 + len;
    }
    None
}

fn jpeg_dpi(b: &[u8]) -> Option<(f64, f64)> {
    let n = b.len() as u64;
    let mut p: u64 = 2;
    let mut exif: Option<(f64, f64)> = None;
    while p + 4 <= n && at(b, p) == 0xff {
        let m = at(b, p + 1);
        if m == 0xff {
            p += 1;
            continue;
        }
        if m == 0xd8 || (0xd0..=0xd7).contains(&m) || m == 0x01 {
            p += 2;
            continue;
        }
        if m == 0xda || m == 0xd9 {
            break;
        }
        let len = u64::from(u16be(b, p + 2));
        let d = p + 4;
        if m == 0xe0
            && len >= 14
            && at(b, d) == 0x4a
            && at(b, d + 1) == 0x46
            && at(b, d + 2) == 0x49
            && at(b, d + 3) == 0x46
            && at(b, d + 4) == 0
        {
            let unit = at(b, d + 7);
            let x = f64::from(u16be(b, d + 8));
            let y = f64::from(u16be(b, d + 10));
            if unit == 1 {
                return Some((x, y));
            }
            if unit == 2 {
                return Some((x * 2.54, y * 2.54));
            }
        } else if m == 0xe1 && len >= 16 && at(b, d) == 0x45 && at(b, d + 1) == 0x78 && at(b, d + 2) == 0x69 && at(b, d + 3) == 0x66 {
            exif = exif_dpi(b, d + 6, n.min(p + 2 + len));
        }
        p += 2 + len;
    }
    exif
}

/// `XResolution`/`YResolution`/`ResolutionUnit` of the IFD0 of a TIFF block (EXIF).
fn exif_dpi(b: &[u8], t: u64, end: u64) -> Option<(f64, f64)> {
    if t + 8 > end {
        return None;
    }
    let le = at(b, t) == 0x49;
    let u16 = |i: u64| -> u32 { if le { at(b, i) | (at(b, i + 1) << 8) } else { u16be(b, i) } };
    let u32v =
        |i: u64| -> u32 { if le { at(b, i) | (at(b, i + 1) << 8) | (at(b, i + 2) << 16) | (at(b, i + 3) << 24) } else { u32be(b, i) } };
    let ifd = t + u64::from(u32v(t + 4));
    if ifd + 2 > end {
        return None;
    }
    let n = u16(ifd);
    let (mut x, mut y, mut unit): (Option<f64>, Option<f64>, u32) = (None, None, 2);
    let rational = |off: u32| -> Option<f64> {
        let a = t + u64::from(off);
        if a + 8 > end {
            return None;
        }
        let den = u32v(a + 4);
        (den != 0).then(|| f64::from(u32v(a)) / f64::from(den))
    };
    for k in 0..u64::from(n) {
        let e = ifd + 2 + 12 * k;
        if e + 12 > end {
            break;
        }
        match u16(e) {
            0x011a => x = rational(u32v(e + 8)),
            0x011b => y = rational(u32v(e + 8)),
            0x0128 => unit = u16(e + 8),
            _ => {}
        }
    }
    // `!x || !y`: undefined, 0 and NaN are falsy
    let (x, y) = (x.filter(|v| *v != 0.0 && !v.is_nan())?, y.filter(|v| *v != 0.0 && !v.is_nan())?);
    if unit != 2 && unit != 3 {
        return None;
    }
    Some(if unit == 3 { (x * 2.54, y * 2.54) } else { (x, y) })
}
