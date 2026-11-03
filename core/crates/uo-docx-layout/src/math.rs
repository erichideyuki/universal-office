//! Equations (`math.ts`, OMML layout, frontier F4) and its own font reader `parseFont`
//! (uo-fonts S14, frontier F1).

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::error::{LResult, LayoutError};
use crate::para::StyleIx;
use uo_fonts::read::View;
use uo_render::FaceId;

/// Steps of the italic-correction coverage walk past which `parseFont` gives up and the font
/// counts as malformed (the TS would walk up to 65,535 ranges × 65,536 glyphs; declared limit,
/// record "Wave 1 — F1").
pub const MAX_MATH_COVERAGE_STEPS: u64 = 1 << 22;

/// `FontInfo` of `math.ts` (34): what the equation layout reads of a face with its own reader
/// (`parseFont`, uo-fonts S14 — not the uo-fonts `Face` rules).
#[derive(Clone, Debug)]
pub struct MathFontInfo {
    pub upm: f64,
    /// the MathConstants after the four 16-bit fields (37 values, font units); `None` = no MATH
    pub k: Option<Box<[f64; 37]>>,
    pub script_pct: f64,
    /// italic correction per glyph (font units); only `get` is observable (TS `Map`)
    pub ital: FxHashMap<u32, f64>,
    /// what `box(gid)` reads: `None` = the TS `box` that always answers `null`
    glyf: Option<GlyfBox>,
}

#[derive(Clone, Debug)]
struct GlyfBox {
    bytes: Arc<[u8]>,
    glyf: u64,
    loca: u64,
    long_loca: bool,
}

impl MathFontInfo {
    /// The info without `parseFont` (no MATH, no outlines): `upm = unitsPerEm || 2048`.
    #[must_use]
    pub fn fallback(units_per_em: u16) -> MathFontInfo {
        MathFontInfo {
            upm: if units_per_em == 0 { 2048.0 } else { f64::from(units_per_em) },
            k: None,
            script_pct: 73.0,
            ital: FxHashMap::default(),
            glyf: None,
        }
    }

    /// `fontInfo(face)` (math.ts:45): `parseFont(face.bytes)`, or the fallback when it gives
    /// `null` or throws (malformed font: no MATH and no outlines).
    #[must_use]
    pub fn of_face(face: &uo_fonts::Face, bytes: &Arc<[u8]>) -> MathFontInfo {
        match parse_math_font(bytes) {
            Ok(Some(info)) => info,
            Ok(None) | Err(_) => MathFontInfo::fallback(face.metrics().units_per_em),
        }
    }

    /// `box(gid)` (math.ts:86): the glyph's vertical ink `[yMin, yMax]` in font units, `None`
    /// without `glyf`/`loca`, for an empty glyph or one whose header is past the end. A `loca`
    /// read past the end throws in the TS (outside `fontInfo`'s `try`): `Err(Internal)`.
    pub fn glyph_box(&self, gid: u32) -> LResult<Option<(f64, f64)>> {
        let Some(g) = &self.glyf else { return Ok(None) };
        let v = View::new(&g.bytes);
        let oob = |_| LayoutError::Internal("math::glyph_box: loca read outside the font");
        let at = |i: u64| -> LResult<u64> {
            if g.long_loca {
                v.u32(g.loca + 4 * i).map(u64::from).map_err(oob)
            } else {
                v.u16(g.loca + 2 * i).map(|x| u64::from(x) * 2).map_err(oob)
            }
        };
        let s = at(u64::from(gid))?;
        let e = at(u64::from(gid) + 1)?;
        if e <= s || g.glyf + s + 10 > v.len() {
            return Ok(None);
        }
        let ink =
            |o: u64| v.i16(g.glyf + s + o).map(f64::from).map_err(|_| LayoutError::Internal("math::glyph_box: glyf read outside the font"));
        Ok(Some((ink(4)?, ink(8)?)))
    }

    /// Whether `box` can answer anything (the font has `glyf` and `loca`).
    #[must_use]
    pub fn has_outlines(&self) -> bool {
        self.glyf.is_some()
    }
}

/// A read past the end of the font: the TS `DataView` throws `RangeError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathFontError;

impl From<uo_fonts::OutOfBounds> for MathFontError {
    fn from(_: uo_fonts::OutOfBounds) -> MathFontError {
        MathFontError
    }
}

/// `parseFont(b)` (math.ts:59): its own table directory (last entry of a tag wins, only `head`
/// required), `upm = head.unitsPerEm || 2048`, the MATH constants and italic corrections, and the
/// `glyf`/`loca` reader of `box`. `Ok(None)` without `head`; `Err` where a `DataView` read
/// throws (`fontInfo` then keeps the fallback).
pub fn parse_math_font(bytes: &Arc<[u8]>) -> Result<Option<MathFontInfo>, MathFontError> {
    let v = View::new(bytes);
    let (mut head, mut math, mut glyf, mut loca) = (None, None, None, None);
    let n = v.u16(4)?;
    for i in 0..u64::from(n) {
        let p = 12 + 16 * i;
        let off = u64::from(v.u32(p + 8)?);
        // the tag bytes are inside the input once the u32 at p + 8 was
        match v.slice(p, 4)? {
            b"head" => head = Some(off),
            b"MATH" => math = Some(off),
            b"glyf" => glyf = Some(off),
            b"loca" => loca = Some(off),
            _ => {}
        }
    }
    let Some(head) = head else { return Ok(None) };
    let upm = match v.u16(head + 18)? {
        0 => 2048.0,
        u => f64::from(u),
    };
    let mut k = None;
    let mut script_pct = 73.0;
    let mut ital = FxHashMap::default();
    if let Some(m) = math {
        let c = m + u64::from(v.u16(m + 4)?);
        script_pct = match v.i16(c)? {
            0 => 73.0,
            x => f64::from(x),
        };
        let mut kk = [0.0; 37];
        for (i, slot) in (0u64..).zip(kk.iter_mut()) {
            *slot = f64::from(v.i16(c + 8 + 4 * i)?);
        }
        k = Some(Box::new(kk));
        let gi = m + u64::from(v.u16(m + 6)?);
        let io = v.u16(gi)?;
        if io != 0 {
            let ic = gi + u64::from(io);
            let cov = ic + u64::from(v.u16(ic)?);
            let cnt = u64::from(v.u16(ic + 2)?);
            for_coverage(&v, cov, cnt, |g, idx| {
                ital.insert(g, f64::from(v.i16(ic + 4 + 4 * idx)?));
                Ok(())
            })?;
        }
    }
    let long_loca = v.i16(head + 50)? == 1;
    let glyf = match (glyf, loca) {
        (Some(glyf), Some(loca)) => Some(GlyfBox { bytes: Arc::clone(bytes), glyf, loca, long_loca }),
        _ => None,
    };
    Ok(Some(MathFontInfo { upm, k, script_pct, ital, glyf }))
}

/// `forCoverage(v, at, fn)` (math.ts:95) with the caller's `if (idx < cnt)` folded in: format 1
/// lists glyphs, any other format ranges `(start, end, startIndex)`. Only the steps with
/// `idx < cnt` call `f` (the others do nothing in the TS either), so a range is walked only that
/// far; past [`MAX_MATH_COVERAGE_STEPS`] the font counts as malformed.
fn for_coverage(v: &View<'_>, at: u64, cnt: u64, mut f: impl FnMut(u32, u64) -> Result<(), MathFontError>) -> Result<(), MathFontError> {
    let fmt = v.u16(at)?;
    let n = v.u16(at + 2)?;
    let mut steps = 0u64;
    for i in 0..u64::from(n) {
        if fmt == 1 {
            let g = v.u16(at + 4 + 2 * i)?;
            if i < cnt {
                f(u32::from(g), i)?;
            }
        } else {
            let s = u64::from(v.u16(at + 4 + 6 * i)?);
            let e = u64::from(v.u16(at + 6 + 6 * i)?);
            let si = u64::from(v.u16(at + 8 + 6 * i)?);
            // g from s to e with idx = si + g - s < cnt
            let take = if e < s || si >= cnt { 0 } else { (e - s + 1).min(cnt - si) };
            steps += take + 1;
            if steps > MAX_MATH_COVERAGE_STEPS {
                return Err(MathFontError);
            }
            for d in 0..take {
                let g = u32::try_from(s + d).map_err(|_| MathFontError)?;
                f(g, si + d)?;
            }
        }
    }
    Ok(())
}

/// `MathItem`: a glyph of a laid-out equation (x from the left edge, y from the baseline, + down).
#[derive(Clone, Copy, Debug)]
pub struct MathItem {
    pub cp: u32,
    pub x: f64,
    pub y: f64,
    pub face: FaceId,
    pub size_pt: f64,
    pub style: StyleIx,
}

/// A fraction bar or radical rule of an equation.
#[derive(Clone, Debug)]
pub struct MathRule {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: Box<[u16]>,
}

/// `MathBox` (math.ts:162): a laid-out equation box (twips).
#[derive(Clone, Debug, Default)]
pub struct MathBox {
    pub w: f64,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
    pub top: f64,
    pub bottom: f64,
    pub line_top: f64,
    pub line_bottom: f64,
}

/// `MathAtom.kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MathAtomKind {
    Char,
    Frac,
}

/// `MathAtom` (math.ts:172).
#[derive(Clone, Debug)]
pub struct MathAtom {
    pub kind: MathAtomKind,
    pub cp: u32,
    pub face: FaceId,
    pub size_pt: f64,
    pub style: StyleIx,
    pub adv: f64,
    pub frac: Option<Box<MathBox>>,
    pub offset: u32,
}
