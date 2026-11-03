//! Underline geometry (`underline.ts`, frontier F5): underlines (`w:u`) as Word draws them in the
//! PDF, measured with the `write/tools/home-lab` probe — everything in 600-DPI pixels (2.4 twips)
//! from the baseline; rectangles for solid and dashed strokes, stroked cubic curves for waves.
//! Ported 1:1; what the TS reads of `face.font` goes through [`crate::paint::PaintEnv`].

use crate::error::LResult;
use crate::items::PassItems;
use crate::lines::{PlacedGlyph, glyph_of};
use crate::paint::PaintEnv;
use crate::para::{GlyphKind, PreparedPara};
use uo_jsnum::math as jm;
use uo_render::types::{PathCmd, Stroke};
use uo_render::{FaceId, Item, OptF64, Sym};

/// twips per 600-DPI pixel
const PX: f64 = 2.4;

/// `UlMetrics`: top (P), thickness (T) and descent limit (L), px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UlMetrics {
    pub p: f64,
    pub t: f64,
    pub l: f64,
}

/// `UlGeom`: derived geometry (px): single `[P, P+T]`; double `[top1, top1+Td]` and
/// `[top2, top2+Td]`; thick/Heavy `[top1, top2]`; waves with the centre line between top1 and B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UlGeom {
    pub p: f64,
    pub t: f64,
    pub td: f64,
    pub top1: f64,
    pub top2: f64,
    pub b: f64,
}

/// What `ulMetrics` reads of `face.font` besides `faceMetrics`: `metrics.unitsPerEm`,
/// `usWinDescent`, `descender` and `vdmx?.(em)?.yMin`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UlFont {
    pub units_per_em: f64,
    pub us_win_descent: f64,
    pub descender: f64,
    /// `vdmx(em)` (`None` = no VDMX, or no record for this em)
    pub vdmx_y_min: Option<f64>,
}

/// `ulMetrics(face, sizePt)` (underline.ts:20): em = round(size·600/72) px; P and T from the post
/// table rounded at the em; L = the descent GDI gives the realised font (VDMX 1:1 when it has this
/// em; else usWinDescent at the em, rounded).
pub fn ul_metrics<E: PaintEnv + ?Sized>(env: &mut E, face: FaceId, size_pt: f64) -> LResult<UlMetrics> {
    let m = env.face_metrics(face)?;
    let em = jm::round((size_pt * 600.0) / 72.0);
    let f = env.ul_font(face, em)?;
    // `fm.unitsPerEm || 1000`
    let upm = if uo_jsnum::conv::truthy(f.units_per_em) { f.units_per_em } else { 1000.0 };
    let p = jm::round(m.underline_pos * em);
    let t = jm::max(1.0, jm::round(m.underline_thick * em));
    let desc = if f.us_win_descent > 0.0 { f.us_win_descent } else { jm::max(0.0, -f.descender) };
    let l = match f.vdmx_y_min {
        Some(y_min) => -y_min,
        None => jm::round((desc * em) / upm),
    };
    Ok(UlMetrics { p, t, l })
}

/// `ulGeometry({ P, T, L })` (underline.ts:36).
#[must_use]
pub fn ul_geometry(m: UlMetrics) -> UlGeom {
    let UlMetrics { p: p0, t, l } = m;
    let p = if p0 + t > l { jm::max(0.0, l - t) } else { p0 };
    let mut td = jm::floor(t / 2.0 + 0.5);
    let mut top1 = jm::ceil(p / 2.0);
    // a gap of at least 8 px between the two lines; the lower one does not pass the descent
    let mut top2 = jm::max(p + t - td, top1 + td + 8.0);
    let over = top2 + td - l;
    if over > 0.0 {
        top1 -= over;
        top2 -= over;
    }
    if top1 < 0.0 {
        top1 = 0.0;
        td = jm::max(1.0, jm::min(td, jm::floor((l - 8.0) / 2.0)));
        top2 = l - td;
    }
    let b = jm::floor((top1 + top2 + td) / 2.0) - 1.0;
    UlGeom { p, t, td, top1, top2, b }
}

/// `PATTERNS[val]` (underline.ts:54): the dash patterns (600-DPI px: dash, gap, …), fixed, not
/// scaled with the size. Names of `Object.prototype` are not looked up (F5-7).
fn pattern(val: &str) -> Option<&'static [f64]> {
    Some(match val {
        "dotted" | "dottedHeavy" => &[12.0, 12.0],
        "dash" | "dashedHeavy" => &[33.0, 16.0],
        "dashLong" | "dashLongHeavy" => &[66.0, 33.0],
        "dotDash" | "dashDotHeavy" => &[25.0, 8.0, 8.0, 8.0],
        "dotDotDash" | "dashDotDotHeavy" => &[18.0, 6.0, 6.0, 6.0, 6.0, 6.0],
        _ => return None,
    })
}

/// `HEAVY.has(val)` (underline.ts:61).
fn heavy(val: &str) -> bool {
    matches!(val, "thick" | "dottedHeavy" | "dashedHeavy" | "dashLongHeavy" | "dashDotHeavy" | "dashDotDotHeavy")
}

/// half a wave: 1.2 pt (peaks every 2.4 pt from the page's left edge)
const HALF_WAVE: f64 = 24.0;

/// A stretch of one colour (`Seg`, underline.ts:65).
#[derive(Clone, Copy, Debug)]
struct Seg {
    x0: f64,
    x1: f64,
    color: Sym,
}

/// A group of glyphs with one underline style (`Group`, underline.ts:77).
struct Group<'a> {
    val: &'a str,
    /// `low`: the shift of a lowered group (`null` = not lowered)
    low: Option<f64>,
    /// indices into `all`
    items: Vec<usize>,
}

/// `${face.key}|${style.sizePt}`: the metrics cache key (face key string, the size's JS string:
/// `-0` and `0` print alike, every NaN prints `NaN`).
#[derive(Clone, Copy, PartialEq, Eq)]
struct UlKey(Sym, u64);

fn num_key(v: f64) -> u64 {
    if v == 0.0 { 0 } else { uo_jsnum::conv::canonicalize_nan(v).to_bits() }
}

/// `paintUnderlines(out, all, frameX, baseY)` (underline.ts:74): the underlines of a line. A
/// contiguous stretch with one style has ONE geometry: the mean of P/T/L weighted by the width of
/// the glyphs on the stretch's baseline (lowered ones — subscript, position < 0 — form their own
/// stretch; raised ones join the line's stretch without weight, or, alone, stay on their own
/// baseline). The colour only splits the drawing (and the pattern restarts).
pub fn paint_underlines<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    all: &[PlacedGlyph],
    frame_x: f64,
    base_y: f64,
) -> LResult<()> {
    let gl = |k: usize| all.get(k).and_then(|q| glyph_of(p, q.g));
    let style_of = |g: &crate::para::Glyph| p.styles.get(g.style.0 as usize);
    let mut last_content: Option<usize> = None;
    for (k, q) in all.iter().enumerate() {
        let Some(g) = glyph_of(p, q.g) else { continue };
        if !matches!(g.kind, GlyphKind::Space | GlyphKind::Lb | GlyphKind::Pb) {
            last_content = Some(k);
        }
    }
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut cur_end = 0.0;
    for (k, q) in all.iter().enumerate() {
        let Some(g) = gl(k) else {
            cur = None;
            continue;
        };
        let st = style_of(&g);
        let u = st.and_then(|st| st.underline.as_ref());
        let words = u.is_some_and(|u| &*u.0 == "words");
        let on = last_content.is_some_and(|lc| k <= lc)
            && u.is_some_and(|u| &*u.0 != "none")
            && q.w > 0.0
            && g.kind != GlyphKind::Lb
            && g.kind != GlyphKind::Pb
            && !(words && (g.kind == GlyphKind::Space || g.kind == GlyphKind::Tab));
        let (Some(st), Some(u), true) = (st, u, on) else {
            cur = None;
            continue;
        };
        let low = if st.shift < 0.0 { Some(st.shift) } else { None };
        let x = frame_x + q.x;
        let joins = cur.and_then(|c| groups.get(c)).is_some_and(|c| c.val == &*u.0 && c.low == low && jm::abs(cur_end - x) < 1.0);
        if let (true, Some(c)) = (joins, cur.and_then(|c| groups.get_mut(c))) {
            c.items.push(k);
        } else {
            groups.push(Group { val: &u.0, low, items: vec![k] });
            cur = Some(groups.len() - 1);
        }
        cur_end = x + q.w;
    }
    for gr in &groups {
        // the stretch's baseline and who weighs in the geometry
        let shift: f64;
        let weigh: Vec<usize>;
        let shift_of = |k: usize| gl(k).and_then(|g| style_of(&g).map(|st| st.shift)).unwrap_or(f64::NAN);
        if let Some(low) = gr.low {
            shift = low;
            weigh = gr.items.clone();
        } else {
            let base: Vec<usize> = gr.items.iter().copied().filter(|&k| shift_of(k) == 0.0).collect();
            if base.is_empty() {
                let shifts: Vec<f64> = gr.items.iter().map(|&k| shift_of(k)).collect();
                shift = jm::min_n(&shifts);
                weigh = gr.items.iter().copied().filter(|&k| shift_of(k) == shift).collect();
            } else {
                shift = 0.0;
                weigh = base;
            }
        }
        let (mut sw, mut sp, mut st, mut sl) = (0.0, 0.0, 0.0, 0.0);
        let mut cache: Vec<(UlKey, UlMetrics)> = Vec::new();
        for &k in &weigh {
            let (Some(q), Some(g)) = (all.get(k), gl(k)) else { continue };
            let size = style_of(&g).map_or(f64::NAN, |s| s.size_pt);
            let key = UlKey(env.face_key(g.face)?, num_key(size));
            let m = match cache.iter().find(|(c, _)| *c == key) {
                Some(&(_, m)) => m,
                None => {
                    let m = ul_metrics(env, g.face, size)?;
                    cache.push((key, m));
                    m
                }
            };
            sw += q.w;
            sp += q.w * m.p;
            st += q.w * m.t;
            sl += q.w * m.l;
        }
        if sw <= 0.0 {
            continue;
        }
        let geo = ul_geometry(UlMetrics { p: jm::round(sp / sw), t: jm::round(st / sw), l: jm::round(sl / sw) });
        let y0 = base_y - shift;
        // pieces by colour (the stroke changes colour; the pattern restarts)
        let mut segs: Vec<Seg> = Vec::new();
        for &k in &gr.items {
            let (Some(q), Some(g)) = (all.get(k), gl(k)) else { continue };
            let Some(sty) = style_of(&g) else { continue };
            let x = frame_x + q.x;
            // `underline.color ?? style.color`
            let color = match sty.underline.as_ref().and_then(|u| u.1.as_deref()) {
                Some(c) => env.intern(&c.encode_utf16().collect::<Vec<u16>>())?,
                None => env.intern(&sty.color)?,
            };
            match segs.last_mut() {
                Some(last) if last.color == color && jm::abs(last.x1 - x) < 1.0 => last.x1 = x + q.w,
                _ => segs.push(Seg { x0: x, x1: x + q.w, color }),
            }
        }
        for s in &segs {
            paint_style(env, out, gr.val, &geo, *s, y0)?;
        }
    }
    Ok(())
}

/// `band(out, s, y0, top, h, pattern?)` (underline.ts:130).
fn band<E: PaintEnv + ?Sized>(env: &mut E, out: &mut PassItems, s: Seg, y0: f64, top: f64, h: f64, pat: Option<&[f64]>) -> LResult<()> {
    let y = y0 + top * PX;
    let hh = h * PX;
    let Some(pat) = pat else {
        crate::paint::push_item(out, Item::Rect { x: s.x0, y, w: s.x1 - s.x0, h: hh, fill: s.color, alpha: OptF64::NONE });
        return Ok(());
    };
    let mut x = s.x0;
    let mut i = 0usize;
    while x < s.x1 - 0.01 {
        env.charge(1)?;
        let len = pat.get(i % pat.len()).copied().unwrap_or(f64::NAN) * PX;
        if i.is_multiple_of(2) {
            crate::paint::push_item(out, Item::Rect { x, y, w: jm::min(len, s.x1 - x), h: hh, fill: s.color, alpha: OptF64::NONE });
        }
        x += len;
        i += 1;
    }
    Ok(())
}

/// `wave(out, s, yTop, yBot, widthPt)` (underline.ts:143): a wave between the centre lines yTop and
/// yBot (twips), peaks on the page grid, clipped to the stretch.
fn wave<E: PaintEnv + ?Sized>(env: &mut E, out: &mut PassItems, s: Seg, y_top: f64, y_bot: f64, width_pt: f64) -> LResult<()> {
    let mut cmds: Vec<PathCmd> = Vec::new();
    let mut k = jm::floor(s.x0 / (2.0 * HALF_WAVE)) * 2.0;
    let mut x = k * HALF_WAVE;
    cmds.push(PathCmd::M { x, y: y_top });
    while x < s.x1 {
        env.charge(1)?;
        // `k % 2 === 0` (`-0 === 0`)
        let up = jm::rem(k, 2.0) == 0.0;
        let (y1, y2) = if up { (y_top, y_bot) } else { (y_bot, y_top) };
        cmds.push(PathCmd::C { x1: x + HALF_WAVE / 2.0, y1, x2: x + HALF_WAVE / 2.0, y2, x: x + HALF_WAVE, y: y2 });
        x += HALF_WAVE;
        k += 1.0;
    }
    let pad = width_pt * 20.0;
    crate::paint::push_item(
        out,
        Item::ClipPush {
            x: OptF64::some(s.x0),
            y: OptF64::some(y_top - pad - HALF_WAVE),
            w: OptF64::some(s.x1 - s.x0),
            h: OptF64::some(y_bot - y_top + 2.0 * pad + 2.0 * HALF_WAVE),
        },
    );
    crate::paint::push_path(out, &cmds, None, Some(Stroke { color: s.color, width_pt, dash: None }), false)?;
    crate::paint::push_item(out, Item::ClipPop);
    Ok(())
}

/// `paintStyle(out, val, g, s, y0)` (underline.ts:161).
fn paint_style<E: PaintEnv + ?Sized>(env: &mut E, out: &mut PassItems, val: &str, g: &UlGeom, s: Seg, y0: f64) -> LResult<()> {
    match val {
        "double" => {
            band(env, out, s, y0, g.top1, g.td, None)?;
            band(env, out, s, y0, g.top2, g.td, None)
        }
        "wave" | "wavyHeavy" => wave(env, out, s, y0 + g.top1 * PX, y0 + g.b * PX, if val == "wavyHeavy" { 0.96 } else { 0.24 }),
        "wavyDouble" => {
            let a = jm::floor((g.b - g.top1) / 2.0) - 1.0;
            wave(env, out, s, y0 + g.top1 * PX, y0 + (g.top1 + a) * PX, 0.24)?;
            wave(env, out, s, y0 + (g.b - a) * PX, y0 + g.b * PX, 0.24)
        }
        _ => {
            let hv = heavy(val);
            let top = if hv { g.top1 } else { g.p };
            let h = if hv { g.top2 - g.top1 } else { g.t };
            band(env, out, s, y0, top, h, pattern(val))
        }
    }
}
