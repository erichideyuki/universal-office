//! "Recolor picture" of PowerPoint 97 (`recolorMetafile` of `metafile.ts`): the colours of the
//! records of an EMF or WMF — text, background, pens, brushes, pixels, palettes and gradient
//! vertices — replaced through a table (COLORREF `0x00BBGGRR` → COLORREF, the 24 colour bits
//! compared, the high byte of a PALETTERGB kept). Bitmaps do not change. Gives a COPY, or `None`
//! when nothing changed.
//!
//! The TS reads through a `DataView`, which throws a `RangeError` outside the buffer; the
//! reads here return `Err` in the same places and the converter turns it into the same format
//! error (a truncated metafile record at the very end of the data).

use super::error::PptError;

fn dv_err() -> PptError {
    PptError::Internal("Offset is outside the bounds of the DataView".to_owned())
}

fn get(b: &[u8], o: usize) -> Option<u8> {
    b.get(o).copied()
}

fn u16le(b: &[u8], o: usize) -> Result<u16, PptError> {
    match (get(b, o), get(b, o + 1)) {
        (Some(x), Some(y)) => Ok(u16::from_le_bytes([x, y])),
        _ => Err(dv_err()),
    }
}

fn u32le(b: &[u8], o: usize) -> Result<u32, PptError> {
    match (get(b, o), get(b, o + 1), get(b, o + 2), get(b, o + 3)) {
        (Some(x), Some(y), Some(z), Some(w)) => Ok(u32::from_le_bytes([x, y, z, w])),
        _ => Err(dv_err()),
    }
}

fn is_emf(b: &[u8]) -> bool {
    b.len() > 44 && b[0] == 1 && b[1] == 0 && b[2] == 0 && b[3] == 0 && b[40] == 0x20 && b[41] == 0x45 && b[42] == 0x4d && b[43] == 0x46
}

fn is_wmf(b: &[u8]) -> bool {
    if b.len() < 40 {
        return false;
    }
    if b[0] == 0xd7 && b[1] == 0xcd && b[2] == 0xc6 && b[3] == 0x9a {
        return true;
    }
    (b[0] == 1 || b[0] == 2) && b[1] == 0 && b[2] == 9 && b[3] == 0
}

struct Recolor<'m> {
    b: Vec<u8>,
    map: &'m [(u32, u32)],
    changed: bool,
}

impl Recolor<'_> {
    /// `map.get(c)`.
    fn lookup(&self, c: u32) -> Option<u32> {
        self.map.iter().find(|(k, _)| *k == c).map(|&(_, v)| v)
    }

    fn put_u32(&mut self, o: usize, v: u32) {
        if let Some(s) = self.b.get_mut(o..o + 4) {
            s.copy_from_slice(&v.to_le_bytes());
        }
    }

    fn cref(&mut self, o: i64) {
        let Ok(o) = usize::try_from(o) else { return };
        let Ok(v) = u32le(&self.b, o) else { return };
        let hi = v >> 24;
        if hi == 1 || hi > 2 {
            return; // PALETTEINDEX / invalid
        }
        let Some(to) = self.lookup(v & 0xff_ffff) else { return };
        if to == (v & 0xff_ffff) {
            return;
        }
        self.put_u32(o, (hi << 24) | (to & 0xff_ffff));
        self.changed = true;
    }

    /// A colour of 16 bits per channel (TriVertex) or of a palette entry (8 bits).
    fn rgb_at(&mut self, o: usize, wide: bool) {
        if o + if wide { 6 } else { 3 } > self.b.len() {
            return;
        }
        let g = |k: usize| -> u32 { u32::from(get(&self.b, if wide { o + k * 2 + 1 } else { o + k }).unwrap_or(0)) };
        let key = g(0) | (g(1) << 8) | (g(2) << 16);
        let Some(to) = self.lookup(key) else { return };
        for k in 0..3usize {
            let c = (to >> (k * 8)) & 0xff;
            if wide {
                let v = u16::try_from(c * 257).unwrap_or(0).to_le_bytes();
                if let Some(s) = self.b.get_mut(o + k * 2..o + k * 2 + 2) {
                    s.copy_from_slice(&v);
                }
            } else if let Some(s) = self.b.get_mut(o + k) {
                *s = u8::try_from(c).unwrap_or(0);
            }
        }
        self.changed = true;
    }
}

/// `recolorMetafile(bytes, map)`: `map` in `Map` order (`(from, to)` COLORREFs).
pub fn recolor_metafile(bytes: &[u8], map: &[(u32, u32)]) -> Result<Option<Vec<u8>>, PptError> {
    if map.is_empty() || !(is_emf(bytes) || is_wmf(bytes)) {
        return Ok(None);
    }
    let mut rc = Recolor { b: bytes.to_vec(), map, changed: false };
    let len = rc.b.len();
    if is_emf(&rc.b) {
        let mut pos = 0usize;
        let mut guard = 0;
        while pos + 8 <= len && guard < 500_000 {
            guard += 1;
            let typ = u32le(&rc.b, pos)?;
            let size = u32le(&rc.b, pos + 4)?;
            let size_u = usize::try_from(size).unwrap_or(usize::MAX);
            if size < 8 || pos.saturating_add(size_u) > len {
                break;
            }
            let r = pos + 8;
            let ri = i64::try_from(r).unwrap_or(i64::MAX);
            match typ {
                24 | 25 => rc.cref(ri),
                38 => rc.cref(ri + 16),
                95 => rc.cref(ri + 32),
                39 => rc.cref(ri + 8),
                15 | 53 => rc.cref(ri + 8),
                49 => {
                    // CREATEPALETTE: LogPalette (version, n, RGB + flags entries)
                    let n = usize::from(u16le(&rc.b, r + 6)?);
                    let mut k = 0usize;
                    while k < n && r + 8 + k * 4 + 4 <= pos + size_u {
                        rc.rgb_at(r + 8 + k * 4, false);
                        k += 1;
                    }
                }
                118 => {
                    // GRADIENTFILL: TriVertex x, y, R, G, B, A (16 bits)
                    let n_ver = u32le(&rc.b, r + 16)?;
                    let mut k = 0usize;
                    while u64::try_from(k).unwrap_or(u64::MAX) < u64::from(n_ver) && r + 28 + k * 16 + 16 <= pos + size_u {
                        rc.rgb_at(r + 28 + k * 16 + 8, true);
                        k += 1;
                    }
                }
                _ => {}
            }
            if typ == 14 {
                break;
            }
            pos += size_u;
        }
    } else {
        let mut pos = if rc.b.first() == Some(&0xd7) { 22usize } else { 0 };
        pos += usize::from(u16le(&rc.b, pos + 2)?) * 2;
        let mut guard = 0;
        while pos + 6 <= len && guard < 500_000 {
            guard += 1;
            let size_w = u64::from(u32le(&rc.b, pos)?) * 2;
            let size_u = usize::try_from(size_w).unwrap_or(usize::MAX);
            let f = u16le(&rc.b, pos + 4)?;
            let p = pos + 6;
            let pi = i64::try_from(p).unwrap_or(i64::MAX);
            if f == 0 || size_w < 6 || pos.saturating_add(size_u) > len {
                break;
            }
            match f {
                0x0209 | 0x0201 | 0x041f | 0x0419 => rc.cref(pi),
                0x02fa => rc.cref(pi + 6),
                0x02fc => rc.cref(pi + 2),
                0x0548 => rc.cref(pi + 2),
                0x00f7 => {
                    // CREATEPALETTE: start, n, entries
                    let n = usize::from(u16le(&rc.b, p + 2)?);
                    let mut k = 0usize;
                    while k < n && p + 4 + k * 4 + 4 <= pos + size_u {
                        rc.rgb_at(p + 4 + k * 4, false);
                        k += 1;
                    }
                }
                _ => {}
            }
            pos += size_u;
        }
    }
    Ok(if rc.changed { Some(rc.b) } else { None })
}
