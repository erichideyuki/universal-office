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

// ───────────────────────── the equation layout (F4) ─────────────────────────

/// `MathEnv` (math.ts:186): what the layout asks of the paragraph (para.ts:855).
pub trait MathEnv {
    /// `styleOf(rpr)`: the cascade of a `m:r`'s `w:rPr` (spacing, scale and ligatures removed) and
    /// its resolved style (a new `RunStyle` per call, as in the TS)
    fn style_of(&mut self, r: Option<&crate::model::RPr>) -> LResult<(Arc<crate::model::RPr>, StyleIx)>;
    /// a style the paragraph holds
    fn style(&self, s: StyleIx) -> LResult<&crate::para::RunStyle>;
    /// `face(bold, italic)`: the equation font
    fn face(&mut self, bold: bool, italic: bool) -> LResult<FaceId>;
    /// the face table of the layout (glyph ids, `fontInfo`)
    fn fonts(&mut self) -> &mut crate::fonts::FaceTable;
    /// `adv(face, cp, size)`
    fn adv(&mut self, face: FaceId, cp: u32, size: f64) -> LResult<f64>;
    /// `kern(face, cp, next, size)`
    fn kern(&mut self, face: FaceId, cp: u32, next: u32, size: f64) -> LResult<f64>;
    /// the recursion guard of nested fractions (LQ13)
    fn max_depth(&self) -> u32;
    /// the work budget
    fn charge(&mut self, units: u64) -> LResult<()>;
}

/// The style level (`Lvl`): D display, T text, S script (reduced size).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lvl {
    D,
    T,
    S,
}

/// `Cls` (math.ts:116): the spacing class of an atom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cls {
    Ord,
    Bin,
    Rel,
    Open,
    Close,
}

/// The MathConstants indices (`K`, math.ts:29).
mod k {
    pub const LEADING: usize = 0;
    pub const AXIS: usize = 1;
    pub const NUM_UP: usize = 28;
    pub const NUM_UP_D: usize = 29;
    pub const DEN_DOWN: usize = 30;
    pub const DEN_DOWN_D: usize = 31;
    pub const NUM_GAP: usize = 32;
    pub const NUM_GAP_D: usize = 33;
    pub const RULE: usize = 34;
    pub const DEN_GAP: usize = 35;
    pub const DEN_GAP_D: usize = 36;
}

/// `CM` (math.ts:31): Cambria Math's proportions (units of 2048) for a font without MATH.
fn cm(i: usize) -> f64 {
    match i {
        0 => 300.0,
        1 => 585.0,
        28 => 1200.0,
        29 => 1550.0,
        30 => 1030.0,
        31 => 1370.0,
        32 => 133.0,
        33 => 260.0,
        34 => 133.0,
        35 => 133.0,
        36 => 260.0,
        // `CM[i]!` of an index the table lacks: `undefined / 2048` = NaN
        _ => f64::NAN,
    }
}

/// `kEm(info, i)` (math.ts:107): a constant as a fraction of the em.
fn k_em(info: &MathFontInfo, i: usize) -> f64 {
    match &info.k {
        Some(k) => k.get(i).copied().unwrap_or(f64::NAN) / info.upm,
        None => cm(i) / 2048.0,
    }
}

/// `q600(tw)` (math.ts:112): rounded UP to 1/600 in (2.4 twips).
fn q600(tw: f64) -> f64 {
    uo_jsnum::math::ceil(tw / 2.4 - 1e-6) * 2.4
}

/// `fontInfo(face)` of a layout face.
fn info_of(fonts: &mut crate::fonts::FaceTable, face: FaceId) -> LResult<Arc<MathFontInfo>> {
    fonts.math_info(face).ok_or(LayoutError::Internal("math: face not in the layout"))
}

/// `mathTextMetrics(face)` (math.ts:115): the vertical metrics of equation text (fractions of the
/// em) — the font's typo metrics when it has a MATH table, else `faceMetrics`.
pub fn math_text_metrics(fonts: &mut crate::fonts::FaceTable, face: FaceId) -> LResult<(f64, f64)> {
    let info = info_of(fonts, face)?;
    let font = fonts.face(face).ok_or(LayoutError::Internal("math: face not in the layout"))?;
    let fm = font.metrics();
    if info.k.is_some() && fm.s_typo_ascender > 0 {
        let u = if fm.units_per_em == 0 { 2048.0 } else { f64::from(fm.units_per_em) };
        return Ok((
            (f64::from(fm.s_typo_ascender) + uo_jsnum::math::max(0.0, f64::from(fm.s_typo_line_gap))) / u,
            -f64::from(fm.s_typo_descender) / u,
        ));
    }
    let m = fonts.metrics(face).ok_or(LayoutError::Internal("math: face not in the layout"))?;
    Ok((m.ascent, m.descent))
}

/// `REL`, `BIN`, `OPEN`, `CLOSE` → `clsOf(cp)` (math.ts:117–121).
fn cls_of(cp: u32) -> Cls {
    const REL: &str = "=<>≤≥≠≈≡∼≃≅∝←→↔⇐⇒⇔∈∉∋⊂⊃⊆⊇≪≫≔";
    const BIN: &str = "+-−±∓×÷·∗∘∙∩∪∧∨⊕⊗⊖*";
    const OPEN: &str = "([{⟨⌈⌊";
    const CLOSE: &str = ")]}⟩⌉⌋";
    let has = |s: &str| s.chars().any(|c| u32::from(c) == cp);
    if has(REL) {
        Cls::Rel
    } else if has(BIN) {
        Cls::Bin
    } else if has(OPEN) {
        Cls::Open
    } else if has(CLOSE) {
        Cls::Close
    } else {
        Cls::Ord
    }
}

/// `mathAlpha(cp, sty)` (math.ts:124): a letter in the mathematical alphabet of `m:sty`.
fn math_alpha(cp: u32, sty: Option<&str>) -> u32 {
    let up = (0x41..=0x5a).contains(&cp);
    let lo = (0x61..=0x7a).contains(&cp);
    match sty.unwrap_or("i") {
        "i" => {
            if cp == 0x68 {
                return 0x210e;
            }
            if up {
                return 0x1d434 + cp - 0x41;
            }
            if lo {
                return 0x1d44e + cp - 0x61;
            }
            if (0x3b1..=0x3c9).contains(&cp) {
                return 0x1d6fc + cp - 0x3b1;
            }
        }
        "b" => {
            if up {
                return 0x1d400 + cp - 0x41;
            }
            if lo {
                return 0x1d41a + cp - 0x61;
            }
            if (0x30..=0x39).contains(&cp) {
                return 0x1d7ce + cp - 0x30;
            }
        }
        "bi" => {
            if up {
                return 0x1d468 + cp - 0x41;
            }
            if lo {
                return 0x1d482 + cp - 0x61;
            }
        }
        _ => {}
    }
    cp
}

/// A `RawAtom` (math.ts:196): the atom with its class, italic correction and ink.
struct RawAtom {
    a: MathAtom,
    cls: Cls,
    ital: f64,
    italic: bool,
    top: f64,
    bottom: f64,
}

/// `mathAtoms(env, nodes, lvl, scale, off)` (math.ts:202): the atoms of one equation line, the
/// spacing to the next atom added to each advance; `off` counts the index in the flattened text.
pub fn math_atoms<E: MathEnv + ?Sized>(
    env: &mut E,
    nodes: &[&crate::model::MathNode],
    lvl: Lvl,
    scale: f64,
    off: &mut u32,
    depth: u32,
) -> LResult<Vec<MathAtom>> {
    Ok(math_atoms_raw(env, nodes, lvl, scale, off, depth)?.into_iter().map(|r| r.a).collect())
}

fn math_atoms_raw<E: MathEnv + ?Sized>(
    env: &mut E,
    nodes: &[&crate::model::MathNode],
    lvl: Lvl,
    scale: f64,
    off: &mut u32,
    depth: u32,
) -> LResult<Vec<RawAtom>> {
    use uo_jsnum::math as jm;
    if depth > env.max_depth() {
        return Err(LayoutError::Limit(crate::error::LimitKind::Depth));
    }
    let mut atoms: Vec<RawAtom> = Vec::new();
    for nd in nodes {
        match &*nd.t {
            "br" => continue,
            "f" => {
                let o0 = *off;
                let fb = frac_box(env, nd, lvl, scale, off, depth + 1)?;
                let (_, st) = env.style_of(None)?;
                let size = env.style(st)?.size_pt * scale;
                let face = env.face(false, false)?;
                let (w, top, bottom) = (fb.w, fb.top, fb.bottom);
                atoms.push(RawAtom {
                    a: MathAtom {
                        kind: MathAtomKind::Frac,
                        cp: 0xfffc,
                        face,
                        size_pt: size,
                        style: st,
                        adv: w,
                        frac: Some(Box::new(fb)),
                        offset: o0,
                    },
                    cls: Cls::Ord,
                    ital: 0.0,
                    italic: false,
                    top,
                    bottom,
                });
            }
            _ => {
                let (_, st) = env.style_of(nd.rpr.as_deref())?;
                let size = env.style(st)?.size_pt * scale;
                let sty = nd.sty.as_deref();
                let bold = matches!(sty, Some("b" | "bi"));
                // `Array.from(nd.text)` of a node without text throws
                let text = nd.text.as_ref().ok_or(LayoutError::Internal("math: m:r without text"))?;
                let units = text.units();
                env.charge(units.len() as u64)?;
                let mut i = 0;
                while i < units.len() {
                    let (cp0, n) = code_point(units, i);
                    let mut cp = math_alpha(cp0, sty);
                    let mut face = env.face(bold && cp == cp0, false)?;
                    // a font without the mathematical alphabet: the plain letter, in italic
                    if cp != cp0 && glyph_id(env, face, cp)? == 0 {
                        face = env.face(bold, sty != Some("b"))?;
                        cp = cp0;
                    }
                    let info = info_of(env.fonts(), face)?;
                    let gid = glyph_id(env, face, cp)?;
                    let bx = info.glyph_box(gid)?;
                    let em = (size * 20.0) / info.upm;
                    let italic = cp != cp0 && sty != Some("b");
                    let adv = env.adv(face, cp, size)?;
                    atoms.push(RawAtom {
                        a: MathAtom { kind: MathAtomKind::Char, cp, face, size_pt: size, style: st, adv, frac: None, offset: *off },
                        cls: cls_of(cp0),
                        ital: if italic { info.ital.get(&gid).copied().unwrap_or(0.0) * em } else { 0.0 },
                        italic,
                        top: bx.map_or(0.0, |b| jm::max(0.0, b.1) * em),
                        bottom: bx.map_or(0.0, |b| jm::max(0.0, -b.0) * em),
                    });
                    *off = off.wrapping_add(u32::try_from(n).unwrap_or(2));
                    i += n;
                }
            }
        }
    }
    // a binary operator at the start, after another operator, a relation or an opening, or at the
    // end: unary (no space)
    for i in 0..atoms.len() {
        if atoms.get(i).is_none_or(|a| a.cls != Cls::Bin) {
            continue;
        }
        let pv = if i > 0 { atoms.get(i - 1).map(|a| a.cls) } else { None };
        let nx = atoms.get(i + 1).map(|a| a.cls);
        let unary = matches!(pv, None | Some(Cls::Bin | Cls::Rel | Cls::Open)) || matches!(nx, None | Some(Cls::Rel | Cls::Close));
        if unary && let Some(a) = atoms.get_mut(i) {
            a.cls = Cls::Ord;
        }
    }
    for i in 0..atoms.len().saturating_sub(1) {
        let (Some(a), Some(b)) = (atoms.get(i), atoms.get(i + 1)) else { break };
        let mut adv = a.a.adv;
        // kerning (w:kern) between neighbouring characters of the same face and size
        let kern = env.style(a.a.style)?.rpr.kern;
        #[expect(clippy::float_cmp, reason = "the TS compares the sizes exactly")]
        let same = a.a.face == b.a.face && a.a.size_pt == b.a.size_pt;
        if kern.is_some_and(crate::model::JsNumber::truthy)
            && a.a.kind == MathAtomKind::Char
            && b.a.kind == MathAtomKind::Char
            && same
            && a.a.size_pt * 2.0 >= kern.map_or(0.0, crate::model::JsNumber::to_number)
        {
            adv += env.kern(a.a.face, a.a.cp, b.a.cp, a.a.size_pt)?;
        }
        // the italic correction before an upright character
        if a.italic && !b.italic {
            adv += a.ital;
        }
        if lvl != Lvl::S {
            let mut sp = 0.0;
            if (a.cls == Cls::Rel) != (b.cls == Cls::Rel) && a.cls != Cls::Open && b.cls != Cls::Close {
                sp = 5.0 / 18.0;
            } else if (a.cls == Cls::Bin || b.cls == Cls::Bin) && a.cls != Cls::Open {
                sp = 4.0 / 18.0;
            }
            if uo_jsnum::conv::truthy(sp) {
                let em = if matches!(a.cls, Cls::Ord | Cls::Close) { b.a.size_pt * 20.0 } else { a.a.size_pt * 20.0 };
                adv += sp * em;
            }
        }
        if let Some(a) = atoms.get_mut(i) {
            a.a.adv = adv;
        }
    }
    Ok(atoms)
}

/// The code point at `i` (lone surrogates as themselves) and its length.
fn code_point(s: &[u16], i: usize) -> (u32, usize) {
    let a = s.get(i).copied().unwrap_or(0);
    if (0xd800..0xdc00).contains(&a)
        && let Some(&b) = s.get(i + 1)
        && (0xdc00..0xe000).contains(&b)
    {
        return (0x10000 + ((u32::from(a) - 0xd800) << 10) + (u32::from(b) - 0xdc00), 2);
    }
    (u32::from(a), 1)
}

fn glyph_id<E: MathEnv + ?Sized>(env: &mut E, face: FaceId, cp: u32) -> LResult<u32> {
    Ok(env.fonts().face(face).ok_or(LayoutError::Internal("math: face not in the layout"))?.glyph_id(cp))
}

/// `rowBox(atoms)` (math.ts:258): a line of atoms → a box (x from 0, baseline at y = 0).
fn row_box(atoms: Vec<RawAtom>) -> MathBox {
    use uo_jsnum::math as jm;
    let mut b = MathBox::default();
    let mut x = 0.0;
    for a in atoms {
        if a.a.kind == MathAtomKind::Frac
            && let Some(fb) = &a.a.frac
        {
            for it in &fb.items {
                b.items.push(MathItem { x: it.x + x, ..*it });
            }
            for r in &fb.rules {
                b.rules.push(MathRule { x: r.x + x, ..r.clone() });
            }
        } else if a.a.cp != 0x20 {
            b.items.push(MathItem { cp: a.a.cp, x, y: 0.0, face: a.a.face, size_pt: a.a.size_pt, style: a.a.style });
        }
        b.top = jm::max(b.top, a.top);
        b.bottom = jm::max(b.bottom, a.bottom);
        x += a.a.adv;
    }
    b.w = x;
    b
}

/// `fracBox(env, f, lvl, scale, off)` (math.ts:277): numerator and denominator centred, the bar on
/// the axis.
fn frac_box<E: MathEnv + ?Sized>(
    env: &mut E,
    f: &crate::model::MathNode,
    lvl: Lvl,
    scale: f64,
    off: &mut u32,
    depth: u32,
) -> LResult<MathBox> {
    use uo_jsnum::math as jm;
    let face = env.face(false, false)?;
    let info = info_of(env.fonts(), face)?;
    let (_, sti) = env.style_of(None)?;
    let (st_size, st_color) = {
        let s = env.style(sti)?;
        (s.size_pt, s.color.clone())
    };
    let display = lvl == Lvl::D;
    // numerator/denominator: in display, the text style (same size); else the script style
    let inner = if display { Lvl::T } else { Lvl::S };
    let iscale = if display { scale } else { scale * (info.script_pct / 100.0) };
    // `f.num`/`f.den` of a node without them: `for … of undefined` throws
    let num_nodes: Vec<&crate::model::MathNode> =
        f.num.as_deref().ok_or(LayoutError::Internal("math: m:f without m:num"))?.iter().collect();
    let num = row_box(math_atoms_raw(env, &num_nodes, inner, iscale, off, depth)?);
    let den_nodes: Vec<&crate::model::MathNode> =
        f.den.as_deref().ok_or(LayoutError::Internal("math: m:f without m:den"))?.iter().collect();
    let den = row_box(math_atoms_raw(env, &den_nodes, inner, iscale, off, depth)?);
    // the fraction's size: the first character inside (the reduction undone), or the zone's
    let first = num.items.first().or_else(|| den.items.first());
    let size = match first {
        Some(it) => it.size_pt * (scale / iscale),
        None => st_size * scale,
    };
    let em = size * 20.0;
    let c = |i: usize| k_em(&info, i) * em;
    let axis = c(k::AXIS);
    let rule = c(k::RULE);
    let no_bar = f.r#type.as_deref() == Some("noBar");
    let rule_q = if no_bar { 0.0 } else { q600(rule) };
    let mut num_shift = c(if display { k::NUM_UP_D } else { k::NUM_UP });
    let mut den_shift = c(if display { k::DEN_DOWN_D } else { k::DEN_DOWN });
    // the minimum gaps between the ink and the bar
    num_shift = jm::max(num_shift, axis + rule / 2.0 + c(if display { k::NUM_GAP_D } else { k::NUM_GAP }) + num.bottom);
    den_shift = jm::max(den_shift, den.top + c(if display { k::DEN_GAP_D } else { k::DEN_GAP }) - (axis - rule / 2.0));
    let num_q = q600(num_shift);
    let den_q = q600(den_shift);
    let w = jm::max(num.w, den.w);
    let nx = (w - num.w) / 2.0;
    let dx = (w - den.w) / 2.0;
    let mut items = Vec::with_capacity(num.items.len() + den.items.len());
    items.extend(num.items.iter().map(|it| MathItem { x: it.x + nx, y: it.y - num_q, ..*it }));
    items.extend(den.items.iter().map(|it| MathItem { x: it.x + dx, y: it.y + den_q, ..*it }));
    let mut rules = Vec::with_capacity(num.rules.len() + den.rules.len() + 1);
    rules.extend(num.rules.iter().map(|r| MathRule { x: r.x + nx, y: r.y - num_q, ..r.clone() }));
    rules.extend(den.rules.iter().map(|r| MathRule { x: r.x + dx, y: r.y + den_q, ..r.clone() }));
    let mut b = MathBox {
        w,
        items,
        rules,
        top: num_q + num.top,
        bottom: den_q + den.bottom,
        line_top: num_q + num.top + c(k::LEADING),
        line_bottom: den_shift + den.bottom,
    };
    if !no_bar {
        b.rules.push(MathRule { x: 0.0, y: -(axis + rule_q / 2.0), w, h: rule_q, color: st_color });
    }
    Ok(b)
}
