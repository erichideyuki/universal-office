//! Test-only exports (feature `dml-memtest`) for the wasm memory gate of `uo-drawingml` (hybrid
//! mode, continuous verification): `tests/dml_memory.test.mjs` runs the soak of the module record
//! (M2: the presets through every function, colour sequences, custom guides, flatten and fill,
//! the four effect masks on 512 x 512, `shade_bevel`, the four adjusts on 1024 x 768 RGBA, the
//! blip effects and the patterns), the big effects at 1, 4 and (on request) 8 and 12 Mpx with the
//! peak live bytes of each call, the wasm32 limit rows and the worst case per limit, and samples
//! `uo_memory_live_bytes` and `memory.buffer.byteLength` (wasm memory never shrinks, so the
//! high-water mark is what a phone tab feels). These are not the binding API of `uo-drawingml`;
//! never in the shipped module.

use std::cell::RefCell;
use std::hint::black_box;

use uo_alloc::measure;
use uo_drawingml::{
    Affine, Align, BevelSpec, BoxF, ClrChange, ColorModKind, DEFAULT_TOL, Error, GuideDef, ImageEffects, LimitKind, Limits, Mask, PathSeg,
    PictureAdjust, Polygons, ShadowSpec, Subpaths, adjust_brightness_contrast, adjust_saturation, adjust_sharpen, adjust_temperature,
    apply_color_mods, apply_image_effects, apply_picture_adjust, custom_guides, distance_to, effect_decode, eval_preset, fill_polygons,
    flatten_path, gaussian_blur, glow_mask, inner_shadow_mask, morph, outer_shadow_mask, pattern_density, pattern_rows, preset_guides,
    preset_names, preset_outline, preset_path, preset_patterns, preset_text_rect, shade_bevel, soft_edge_mask,
};

/// The long-lived inputs of the soak (built once, like the native test builds them outside the
/// measured loop): a disc silhouette of 512 x 512 and of 256 x 256, and a 1024 x 768 RGBA image.
struct State {
    sil512: Mask,
    sil256: Mask,
    rgba: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static LAST_ALLOCATED: RefCell<u64> = const { RefCell::new(0) };
    static LAST_CODE: RefCell<f64> = const { RefCell::new(0.0) };
}

/// A disc of `side` x `side` with an anti-aliased edge (`sqrt` and arithmetic only).
fn disc(side: u32) -> Mask {
    let r = f64::from(side) / 2.0;
    let mut data = Vec::with_capacity(usize::try_from(u64::from(side) * u64::from(side)).unwrap_or(0));
    for y in 0..side {
        for x in 0..side {
            let (dx, dy) = (f64::from(x) + 0.5 - r, f64::from(y) + 0.5 - r);
            let c = r - (dx * dx + dy * dy).sqrt() + 0.5;
            data.push(if c < 0.0 {
                0.0
            } else if c > 1.0 {
                1.0
            } else {
                c as f32
            });
        }
    }
    Mask::from_vec(side, side, data).unwrap_or_else(|_| Mask::from_vec(0, 0, Vec::new()).unwrap_or_else(|_| unreachable!()))
}

fn rgba_image(w: u32, h: u32) -> Vec<u8> {
    let n = usize::try_from(u64::from(w) * u64::from(h) * 4).unwrap_or(0);
    (0..n).map(|i| ((i * 31 + i / 7) % 251) as u8).collect()
}

/// A small generator (xorshift32) so the scenario depends only on `round`.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap_or(0) % n.max(1)
    }
}

/// Builds the long-lived inputs (idempotent).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_prepare() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            *s = Some(State { sil512: disc(512), sil256: disc(256), rgba: rgba_image(1024, 768) });
        }
    });
}

/// Drops them: the module returns to its start.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_reset() {
    STATE.with(|s| *s.borrow_mut() = None);
}

// ───────────────────────────── the soak ─────────────────────────────

fn part_presets(round: u32) -> f64 {
    let hostile = [f64::NAN, 1e9, -1e9, f64::INFINITY];
    let mut acc = 0usize;
    for name in preset_names() {
        for (w, h) in [(200.0, 100.0), (21600.0, 21600.0), (0.0, 1e6)] {
            let w = w + f64::from(round % 3);
            acc += usize::from(eval_preset(name, w, h, None, None).is_some());
            acc += usize::from(matches!(preset_path(Some(name), w, h, None, None), uo_drawingml::PresetPath::Path(_)));
            acc += preset_guides(name, w, h, None, None).map_or(0, |g| g.len());
            acc += usize::from(preset_text_rect(Some(name), w, h, None, None).is_some());
            acc += preset_outline(Some(name), w, h, None).subs.len();
            acc += eval_preset(name, w, h, Some(&hostile), None).map_or(0, |g| g.subs.len());
            acc += usize::from(!matches!(
                preset_path(Some(name), w, h, Some(&hostile), Some(&["adj", "adj1", "adj2", "adj3"])),
                uo_drawingml::PresetPath::Rect
            ));
        }
    }
    acc as f64
}

const KINDS: [ColorModKind; 16] = [
    ColorModKind::Gamma,
    ColorModKind::InvGamma,
    ColorModKind::Comp,
    ColorModKind::Inv,
    ColorModKind::Gray,
    ColorModKind::Shade,
    ColorModKind::Tint,
    ColorModKind::LumMod,
    ColorModKind::LumOff,
    ColorModKind::Lum,
    ColorModKind::SatMod,
    ColorModKind::SatOff,
    ColorModKind::Sat,
    ColorModKind::HueMod,
    ColorModKind::HueOff,
    ColorModKind::Hue,
];

fn part_colour(round: u32) -> f64 {
    let mut rng = Lcg(round.wrapping_mul(2_654_435_761) | 1);
    let mut acc = 0.0;
    for _ in 0..1000 {
        let n = 1 + rng.below(5);
        let mods: Vec<(ColorModKind, Option<f64>)> = (0..n)
            .map(|_| {
                let raw = if rng.below(8) == 0 { None } else { Some(f64::from(u32::try_from(rng.below(250_000)).unwrap_or(0)) - 20_000.0) };
                (KINDS[rng.below(KINDS.len())], raw)
            })
            .collect();
        let c = [f64::from(rng.next() % 256) / 255.0, f64::from(rng.next() % 256) / 255.0, f64::from(rng.next() % 256) / 255.0];
        let out = apply_color_mods(c, &mods);
        acc += if out[0].is_nan() { 0.0 } else { out[0] };
    }
    acc
}

const OPS: [&str; 18] =
    ["val", "*/", "+-", "+/", "?:", "abs", "at2", "cat2", "sat2", "cos", "sin", "tan", "max", "min", "mod", "pin", "sqrt", "bogus"];
const TOKENS: [&str; 16] = ["0", "1", "w", "h", "ss", "wd2", "hd4", "cd4", "a", "b", "adj1", "5400000", "2.5", "0x1A", "1e400", "Infinity"];
const GNAMES: [&str; 6] = ["a", "b", "c", "d", "x1", "y1"];

fn part_guides(round: u32) -> f64 {
    let mut rng = Lcg(round.wrapping_mul(40_503) | 1);
    let mut acc = 0.0;
    for _ in 0..200 {
        let programs: Vec<(&str, Vec<&str>)> = (0..4)
            .map(|_| {
                let mut f = vec![OPS[rng.below(OPS.len())]];
                for _ in 0..rng.below(4) {
                    f.push(TOKENS[rng.below(TOKENS.len())]);
                }
                (GNAMES[rng.below(GNAMES.len())], f)
            })
            .collect();
        let defs: Vec<GuideDef<'_>> = programs.iter().map(|(n, f)| GuideDef { name: n, fmla: f }).collect();
        let g = custom_guides(&defs[..1], &defs[1..], 200.0, 100.0, &Limits::DEFAULT);
        acc += g.val("a") + g.val("x1");
    }
    acc
}

fn part_flatten_fill() -> f64 {
    let limits = Limits::DEFAULT;
    let mut acc = 0usize;
    for name in ["star5", "heart", "cloud", "gear9", "circularArrow", "rect", "ellipse", "chevron", "sun", "moon"] {
        for (w, h) in [(256.0, 256.0), (240.0, 120.0)] {
            let subs = preset_outline(Some(name), w, h, None).subs;
            if let Ok(polys) = flatten_path(&subs, &Affine([1.0, 0.0, 0.0, 1.0, 2.0, 2.0]), DEFAULT_TOL, &limits)
                && let Ok(mut m) = Mask::new(260, 260, &limits)
            {
                let _ = fill_polygons(&mut m, &polys, false, &limits);
                acc += distance_to(&m, true).len() + polys.len();
            }
        }
    }
    acc as f64
}

fn part_effects(sil: &Mask) -> f64 {
    let limits = Limits::DEFAULT;
    let bx = BoxF { x: 0.0, y: 0.0, w: f64::from(sil.w()), h: f64::from(sil.h()) };
    let spec = ShadowSpec { blur_pt: 6.0, dist_pt: 3.0, dir_deg: 45.0, sx: None, sy: None, kx_deg: None, ky_deg: None, algn: Align::Other };
    let mut acc = 0.0;
    if let Ok(m) = outer_shadow_mask(sil, &bx, &spec, 1.0, &limits) {
        acc += f64::from(m.data().first().copied().unwrap_or(0.0));
    }
    if let Ok(m) = inner_shadow_mask(sil, 6.0, 3.0, 45.0, 1.0, &limits) {
        acc += f64::from(m.data().get(7).copied().unwrap_or(0.0));
    }
    if let Ok(m) = glow_mask(sil, 8.0, 1.0, &limits) {
        acc += f64::from(m.data().get(9).copied().unwrap_or(0.0));
    }
    if let Ok(m) = soft_edge_mask(sil, 6.0, 1.0, &limits) {
        acc += f64::from(m.data().get(11).copied().unwrap_or(0.0));
    }
    black_box(morph(sil, f64::NAN, &limits).is_err());
    black_box(gaussian_blur(sil, f64::INFINITY, &limits).is_err());
    acc
}

fn part_bevel(sil: &Mask, round: u32) -> f64 {
    let limits = Limits::DEFAULT;
    let n = usize::try_from(u64::from(sil.w()) * u64::from(sil.h()) * 4).unwrap_or(0);
    let mut rgba = vec![128u8; n];
    let word = BevelSpec { w: 10.0, h: 10.0, prst: Some("circle"), rig: None, dir: None, material: None, word: true };
    let rig = BevelSpec {
        w: 10.0,
        h: 10.0,
        prst: Some("relaxedInset"),
        rig: Some("harsh"),
        dir: Some("br"),
        material: None,
        word: round.is_multiple_of(2),
    };
    let ok = shade_bevel(&mut rgba, sil, &word, &limits).is_ok() && shade_bevel(&mut rgba, sil, &rig, &limits).is_ok();
    f64::from(rgba.get(1000).copied().unwrap_or(0)) + f64::from(u8::from(ok))
}

fn part_adjust(d: &mut [u8], round: u32) -> f64 {
    let limits = Limits::DEFAULT;
    let t = f64::from(round % 9);
    adjust_saturation(d, 0.3 * t);
    adjust_temperature(d, 4000.0 + 700.0 * t);
    adjust_brightness_contrast(d, 0.2 * t - 0.8, 0.8 - 0.2 * t);
    let a = adjust_sharpen(d, 1024, 768, 0.25 * t - 1.0, &limits).is_ok();
    let b = adjust_sharpen(d, 1024, 768, -0.5, &limits).is_ok();
    f64::from(d.get(4000).copied().unwrap_or(0)) + f64::from(u8::from(a && b))
}

fn part_blip(d: &mut [u8], round: u32) -> f64 {
    let n = d.len().min(4 * 256 * 256);
    let d = &mut d[..n];
    let i = round as usize;
    let t = f64::from(round % 9);
    let fx = ImageEffects {
        gray: i.is_multiple_of(2),
        gain: Some(1.0 + t / 10.0),
        bright: Some(t / 20.0),
        floor: i.is_multiple_of(3),
        bi_level: if i.is_multiple_of(4) { Some(0.5) } else { None },
    };
    apply_image_effects(d, &fx);
    let dec = effect_decode(&fx, 3).map_or(0, |v| v.len());
    let adjust = PictureAdjust {
        alpha: Some(0.9),
        duotone: Some(("102030", "f0e0d0")),
        clr_change: Some(ClrChange { from: "808080", to: "ffffff", to_alpha: 0.5 }),
    };
    apply_picture_adjust(d, &adjust);
    let mut acc = dec as f64;
    for (name, _) in preset_patterns() {
        acc += pattern_density(name) + f64::from(pattern_rows(name).map_or(0, |r| r[0]));
    }
    acc + f64::from(d.get(100).copied().unwrap_or(0))
}

/// Number of parts of one soak round.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_part_count() -> u32 {
    8
}

/// One part of a soak round (0 presets, 1 colour, 2 custom guides, 3 flatten and fill, 4 effects on
/// 512 x 512, 5 `shade_bevel` 256 x 256, 6 adjusts on 1024 x 768, 7 blip and patterns); a number
/// that depends on the work so it is not optimised away, -1 before `uo_dml_memtest_prepare`.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_part(part: u32, round: u32) -> f64 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return -1.0 };
        let v = match part {
            0 => part_presets(round),
            1 => part_colour(round),
            2 => part_guides(round),
            3 => part_flatten_fill(),
            4 => part_effects(&st.sil512),
            5 => part_bevel(&st.sil256, round),
            6 => part_adjust(&mut st.rgba, round),
            7 => part_blip(&mut st.rgba, round),
            _ => return -1.0,
        };
        if v.is_finite() { v.abs() } else { 0.0 }
    })
}

/// A whole soak round: every part (the sum of their numbers).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_round(round: u32) -> f64 {
    (0..uo_dml_memtest_part_count()).map(|p| uo_dml_memtest_part(p, round)).sum()
}

// ───────────────────────────── big effects ─────────────────────────────

fn code(e: Error) -> f64 {
    match e {
        Error::Limit(LimitKind::MaskPixels) => 1.0,
        Error::Limit(LimitKind::KernelRadius) => 2.0,
        Error::Limit(LimitKind::PathPoints) => 3.0,
        Error::Limit(LimitKind::FillWork) => 4.0,
        Error::Limit(LimitKind::ImagePixels) => 5.0,
        Error::BufferSize => 6.0,
        Error::InvalidArgument => 7.0,
    }
}

fn result_code<T>(r: &Result<T, Error>) -> f64 {
    r.as_ref().map_or_else(|e| code(*e), |_| 0.0)
}

/// Names of the big-effect kinds (the order of `uo_dml_memtest_big`).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_big_count() -> u32 {
    10
}

/// Runs one big effect on a silhouette (or image) of `side` x `side` pixels with the wasm32 limits
/// (`Limits::DEFAULT`): 0 outer shadow, 1 inner shadow, 2 glow, 3 soft edge, 4 morph, 5 blur,
/// 6 `shade_bevel`, 7 sharpen, 8 soften, 9 distance transform. The input is built before the
/// measure (the caller holds it); returns the peak live bytes above the start of the call (also when
/// it was refused: `uo_dml_memtest_last_code`). `uo_dml_memtest_big_allocated` gives the bytes the call
/// allocated in total.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_big(kind: u32, side: u32) -> f64 {
    let limits = Limits::DEFAULT;
    let sil = disc(side);
    let bx = BoxF { x: 0.0, y: 0.0, w: f64::from(side), h: f64::from(side) };
    let spec = ShadowSpec { blur_pt: 6.0, dist_pt: 3.0, dir_deg: 45.0, sx: None, sy: None, kx_deg: None, ky_deg: None, algn: Align::Other };
    let mut rgba =
        if (6..=8).contains(&kind) { vec![100u8; usize::try_from(u64::from(side) * u64::from(side) * 4).unwrap_or(0)] } else { Vec::new() };
    let (r, usage) = measure(|| -> f64 {
        match kind {
            0 => {
                let r = outer_shadow_mask(&sil, &bx, &spec, 1.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            1 => {
                let r = inner_shadow_mask(&sil, 6.0, 3.0, 45.0, 1.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            2 => {
                let r = glow_mask(&sil, 8.0, 1.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            3 => {
                let r = soft_edge_mask(&sil, 6.0, 1.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            4 => {
                let r = morph(&sil, 4.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            5 => {
                let r = gaussian_blur(&sil, 4.0, &limits);
                let c = result_code(&r);
                drop(black_box(r));
                c
            }
            6 => {
                let spec = BevelSpec {
                    w: f64::from(side) / 8.0,
                    h: f64::from(side) / 8.0,
                    prst: Some("circle"),
                    rig: None,
                    dir: None,
                    material: None,
                    word: false,
                };
                let r = shade_bevel(&mut rgba, &sil, &spec, &limits);
                result_code(&r)
            }
            7 => result_code(&adjust_sharpen(&mut rgba, side, side, 0.5, &limits)),
            8 => result_code(&adjust_sharpen(&mut rgba, side, side, -0.5, &limits)),
            _ => {
                let d = distance_to(&sil, true);
                let n = d.len();
                black_box(d);
                if n == 0 { 7.0 } else { 0.0 }
            }
        }
    });
    LAST_ALLOCATED.with(|c| *c.borrow_mut() = usage.allocated);
    LAST_CODE.with(|c| *c.borrow_mut() = r);
    black_box(&rgba);
    usage.peak_above_start as f64
}

/// The error code of the last `uo_dml_memtest_big` or `uo_dml_memtest_hostile` call (0 = it ran, 1 mask
/// pixels, 2 kernel radius, 3 path points, 4 fill work, 5 image pixels, 6 buffer size, 7 invalid argument).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_last_code() -> f64 {
    LAST_CODE.with(|c| *c.borrow())
}

/// Bytes allocated in total by the last `uo_dml_memtest_big`.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_big_allocated() -> f64 {
    LAST_ALLOCATED.with(|c| *c.borrow() as f64)
}

// ───────────────────────────── limits and the worst case ─────────────────────────────

/// `max_mask_pixels` and `max_image_pixels` of the module's `Limits::DEFAULT` (wasm32: 2^25 each).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_default_limit(which: u32) -> f64 {
    let l = Limits::DEFAULT;
    match which {
        0 => l.max_mask_pixels as f64,
        1 => l.max_image_pixels as f64,
        2 => f64::from(l.max_kernel_radius),
        3 => l.max_path_points as f64,
        4 => l.max_fill_work as f64,
        _ => f64::from(l.max_custom_guides),
    }
}

/// The error rows of the wasm32 limits that need no big allocation; the result is the error code of
/// the call (1 mask pixels, 2 kernel radius, 3 path points, 4 fill work, 0 no error), and none of
/// them may allocate before its check: 0 `Mask::new(8192, 8192)`, 1 blur with sigma 2000, 2 morph
/// with radius 1e5, 3 `shade_bevel` on a mask of width `u32::MAX`, 4 `fill_polygons` with 70,000
/// edges over 65,536 rows. Returns -1 for an unknown row.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_limit_row(row: u32) -> f64 {
    let limits = Limits::DEFAULT;
    match row {
        0 => result_code(&Mask::new(8192, 8192, &limits)),
        1 => result_code(&gaussian_blur(&disc(8), 2000.0, &limits)),
        2 => result_code(&morph(&disc(16), 1e5, &limits)),
        3 => {
            let sil = Mask::from_vec(u32::MAX, 0, Vec::new());
            match sil {
                Ok(sil) => {
                    let spec = BevelSpec { w: 4.0, h: 4.0, prst: None, rig: None, dir: None, material: None, word: false };
                    result_code(&shade_bevel(&mut [], &sil, &spec, &limits))
                }
                Err(e) => code(e),
            }
        }
        4 => {
            let mut polys = Polygons::new();
            for i in 0..70_000u32 {
                polys.push_point(f64::from(i % 2), f64::from(i % 65_000));
            }
            polys.end_poly();
            match Mask::new(1, 65_536, &limits) {
                Ok(mut m) => result_code(&fill_polygons(&mut m, &polys, false, &limits)),
                Err(e) => code(e),
            }
        }
        _ => -1.0,
    }
}

/// The worst cases per limit: peak live bytes above the start with everything dropped afterwards
/// (-1 for an unknown kind; `uo_dml_memtest_last_code` is the error when the call was refused):
/// 0 `flatten_path` into its 4 Mi point limit (8,192 ellipse arcs, refused at the limit), 1 a
/// kernel at the radius limit (sigma 1365 on a 64 x 64 mask), 2 `morph` to the padded-plane limit
/// of 2^25 pixels on a 512 x 512 silhouette (about 0.5 GB: only when `allow_big` is not 0).
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_memtest_hostile(kind: u32, allow_big: u32) -> f64 {
    let limits = Limits::DEFAULT;
    match kind {
        0 => {
            let mut subs = Subpaths::new();
            for _ in 0..8192 {
                subs.push(PathSeg::E { cx: 100.0, cy: 100.0, rx: 90.0, ry: 90.0, a0: 0.0, a1: 6.0, ccw: false });
                subs.end_sub();
            }
            let (r, usage) = measure(|| flatten_path(&subs, &Affine([100.0, 0.0, 0.0, 100.0, 0.0, 0.0]), 0.01, &limits));
            let c = result_code(&r);
            drop(black_box(r));
            LAST_ALLOCATED.with(|a| *a.borrow_mut() = usage.allocated);
            LAST_CODE.with(|a| *a.borrow_mut() = c);
            usage.peak_above_start as f64
        }
        1 => {
            let sil = disc(64);
            let (r, usage) = measure(|| gaussian_blur(&sil, 1365.0, &limits));
            let c = result_code(&r);
            drop(black_box(r));
            LAST_ALLOCATED.with(|a| *a.borrow_mut() = usage.allocated);
            LAST_CODE.with(|a| *a.borrow_mut() = c);
            usage.peak_above_start as f64
        }
        2 if allow_big != 0 => {
            let sil = disc(512);
            let (r, usage) = measure(|| morph(&sil, 2638.0, &limits));
            let c = result_code(&r);
            drop(black_box(r));
            LAST_ALLOCATED.with(|a| *a.borrow_mut() = usage.allocated);
            LAST_CODE.with(|a| *a.borrow_mut() = c);
            usage.peak_above_start as f64
        }
        _ => -1.0,
    }
}
