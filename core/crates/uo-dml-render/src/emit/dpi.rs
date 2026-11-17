//! `imageDpi` of `shared/charts/src/image.ts`: the resolution recorded in a PNG (`pHYs`), JPEG (JFIF or EXIF) or BMP
//! file. A read past the end is `undefined` in the TS, which is 0 in the bit operations and `NaN` elsewhere: every
//! byte read here is `Option<u8>` and the bit operations treat a missing byte as 0, as JS does.

/// `{ x, y }` in dots per inch; `None` = no resolution (96 dpi is then assumed by the caller).
pub fn image_dpi(b: &[u8]) -> Option<(f64, f64)> {
    if b.len() > 24 && b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return sane(png_dpi(b));
    }
    if b.len() > 4 && b.first() == Some(&0xff) && b.get(1) == Some(&0xd8) {
        return sane(jpeg_dpi(b));
    }
    if b.len() > 46 && b.first() == Some(&0x42) && b.get(1) == Some(&0x4d) {
        let x = i32le(b, 38);
        let y = i32le(b, 42);
        return if x > 0.0 && y > 0.0 { sane(Some((x * 0.0254, y * 0.0254))) } else { None };
    }
    None
}

fn sane(d: Option<(f64, f64)>) -> Option<(f64, f64)> {
    d.filter(|&(x, y)| x >= 1.0 && y >= 1.0 && x <= 100_000.0 && y <= 100_000.0)
}

fn at(b: &[u8], i: usize) -> f64 {
    b.get(i).map_or(0.0, |&v| f64::from(v))
}

fn u16be(b: &[u8], i: usize) -> f64 {
    at(b, i) * 256.0 + at(b, i + 1)
}

/// A JS number as an index (the values are 16/32-bit reads).
fn us(v: f64) -> usize {
    usize::try_from(uo_jsnum::conv::to_uint32(v)).unwrap_or(0)
}

fn byte(b: &[u8], i: usize) -> u32 {
    b.get(i).map_or(0, |&v| u32::from(v))
}

fn u32be(b: &[u8], i: usize) -> f64 {
    f64::from(byte(b, i) << 24 | byte(b, i + 1) << 16 | byte(b, i + 2) << 8 | byte(b, i + 3))
}

fn i32le(b: &[u8], i: usize) -> f64 {
    let v = byte(b, i) | byte(b, i + 1) << 8 | byte(b, i + 2) << 16 | byte(b, i + 3) << 24;
    f64::from(v.cast_signed())
}

fn png_dpi(b: &[u8]) -> Option<(f64, f64)> {
    let mut p = 8usize;
    while p + 12 <= b.len() {
        let len = u32be(b, p);
        let ty = [b.get(p + 4).copied()?, b.get(p + 5).copied()?, b.get(p + 6).copied()?, b.get(p + 7).copied()?];
        if &ty == b"pHYs" && len >= 9.0 {
            let x = u32be(b, p + 8);
            let y = u32be(b, p + 12);
            let unit = b.get(p + 16).copied();
            return if unit == Some(1) { Some((x * 0.0254, y * 0.0254)) } else { None };
        }
        if &ty == b"IDAT" || &ty == b"IEND" {
            return None;
        }
        let step = 12.0 + len;
        if step > 1e15 {
            return None;
        }
        p += us(step);
    }
    None
}

fn jpeg_dpi(b: &[u8]) -> Option<(f64, f64)> {
    let mut p = 2usize;
    let mut exif = None;
    while p + 4 <= b.len() && b.get(p) == Some(&0xff) {
        let m = *b.get(p + 1)?;
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
        let len = u16be(b, p + 2);
        let d = p + 4;
        if m == 0xe0 && len >= 14.0 && b.get(d..d + 5) == Some(&[0x4a, 0x46, 0x49, 0x46, 0]) {
            let unit = b.get(d + 7).copied();
            let x = u16be(b, d + 8);
            let y = u16be(b, d + 10);
            if unit == Some(1) {
                return Some((x, y));
            }
            if unit == Some(2) {
                return Some((x * 2.54, y * 2.54));
            }
        } else if m == 0xe1 && len >= 16.0 && b.get(d..d + 4) == Some(&[0x45, 0x78, 0x69, 0x66]) {
            exif = exif_dpi(b, d + 6, b.len().min(p + 2 + us(len)));
        }
        p += 2 + us(len);
    }
    exif
}

fn exif_dpi(b: &[u8], t: usize, end: usize) -> Option<(f64, f64)> {
    if t + 8 > end {
        return None;
    }
    let le = b.get(t) == Some(&0x49);
    let u16 = |i: usize| -> f64 { if le { at(b, i) + at(b, i + 1) * 256.0 } else { u16be(b, i) } };
    let u32 = |i: usize| -> f64 {
        if le { f64::from(byte(b, i) | byte(b, i + 1) << 8 | byte(b, i + 2) << 16 | byte(b, i + 3) << 24) } else { u32be(b, i) }
    };
    let ifd = t.checked_add(us(u32(t + 4)))?;
    if ifd + 2 > end {
        return None;
    }
    let n = us(u16(ifd));
    let (mut x, mut y, mut unit) = (None, None, 2.0);
    let rational = |off: f64| -> Option<f64> {
        let a = t.checked_add(us(off))?;
        if a + 8 > end {
            return None;
        }
        let den = u32(a + 4);
        if den != 0.0 { Some(u32(a) / den) } else { None }
    };
    for k in 0..n {
        let e = ifd + 2 + 12 * k;
        if e + 12 > end {
            break;
        }
        let tag = uo_jsnum::conv::to_uint32(u16(e));
        if tag == 0x011a {
            x = rational(u32(e + 8));
        } else if tag == 0x011b {
            y = rational(u32(e + 8));
        } else if tag == 0x0128 {
            unit = u16(e + 8);
        }
    }
    let (x, y) = (x?, y?);
    if x == 0.0 || y == 0.0 || (unit != 2.0 && unit != 3.0) {
        return None;
    }
    Some(if unit == 3.0 { (x * 2.54, y * 2.54) } else { (x, y) })
}
