//! COLR version 0 colour glyphs with the first CPAL palette.

use crate::read::View;
use crate::sfnt::TableRecord;

/// Where the COLR and CPAL records are (the TS sums of `u32` offsets do not wrap: `u64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColrInfo {
    pub base: u64,
    pub n_base: u16,
    pub layers: u64,
    pub n_layers: u16,
    pub colors: u64,
    pub first_color: u16,
    pub n_colors: u16,
}

/// A colour layer: a glyph, an RGB colour (`None`: the text colour) and the opacity (`a / 255`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorLayer {
    pub glyph_id: u16,
    pub color: Option<[u8; 3]>,
    pub alpha: f64,
}

/// `colrInfo`: both tables present with at least 14 bytes, a non-zero base count and non-zero
/// base and layer offsets; a read past the end of the input is `None` (the TS `catch`).
pub(crate) fn info(view: View<'_>, colr: Option<TableRecord>, cpal: Option<TableRecord>) -> Option<ColrInfo> {
    let t = colr?;
    let cp = cpal?;
    if t.length < 14 || cp.length < 14 {
        return None;
    }
    let (to, co) = (u64::from(t.offset), u64::from(cp.offset));
    let n_base = view.u16(to + 2).ok()?;
    let base_offset = u64::from(view.u32(to + 4).ok()?);
    let layer_offset = u64::from(view.u32(to + 8).ok()?);
    let n_layers = view.u16(to + 12).ok()?;
    if n_base == 0 || base_offset == 0 || layer_offset == 0 {
        return None;
    }
    let colors = co + u64::from(view.u32(co + 8).ok()?);
    let first_color = view.u16(co + 12).ok()?;
    let n_colors = view.u16(co + 2).ok()?;
    Some(ColrInfo { base: to + base_offset, n_base, layers: to + layer_offset, n_layers, colors, first_color, n_colors })
}

/// `colorLayers`: binary search of the base glyph records, then up to `n` layers while
/// `first + k < nLayers`; palette index 0xFFFF or `>= nColors` is no colour with opacity 1,
/// otherwise the BGRA entry gives the RGB colour and `a / 255`. An empty list is `None`. A read
/// past the end of the input is `None` too (the TS throws, G4).
pub(crate) fn layers(view: View<'_>, c: &ColrInfo, glyph_id: u32) -> Option<Vec<ColorLayer>> {
    let (mut lo, mut hi) = (0i64, i64::from(c.n_base) - 1);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let p = c.base + u64::try_from(mid).ok()? * 6;
        let g = u32::from(view.u16(p).ok()?);
        if g < glyph_id {
            lo = mid + 1;
            continue;
        }
        if g > glyph_id {
            hi = mid - 1;
            continue;
        }
        let first = u64::from(view.u16(p + 2).ok()?);
        let n = u64::from(view.u16(p + 4).ok()?);
        let mut out: Vec<ColorLayer> = Vec::new();
        let mut k = 0;
        while k < n && first + k < u64::from(c.n_layers) {
            let q = c.layers + (first + k) * 4;
            let pal = view.u16(q + 2).ok()?;
            if pal == 0xFFFF || pal >= c.n_colors {
                out.push(ColorLayer { glyph_id: view.u16(q).ok()?, color: None, alpha: 1.0 });
                k += 1;
                continue;
            }
            let r = c.colors + (u64::from(c.first_color) + u64::from(pal)) * 4;
            let glyph = view.u16(q).ok()?;
            let red = view.u8(r + 2).ok()?;
            let green = view.u8(r + 1).ok()?;
            let blue = view.u8(r).ok()?;
            let alpha = f64::from(view.u8(r + 3).ok()?) / 255.0;
            out.push(ColorLayer { glyph_id: glyph, color: Some([red, green, blue]), alpha });
            k += 1;
        }
        return if out.is_empty() { None } else { Some(out) };
    }
    None
}
