//! Parity replay of `raster.rs` and `effects.rs` against `tests/captures/raster.capture`, written by
//! `tools/capture-raster.mjs` from the frozen TS (`shared/drawingml/dist`, Node 24.15.0, V8 13.6):
//! `flattenPath`, `fillPolygons`, `gaussianBlur`, `distanceTo`, `morph`, `shiftMask`, `warpMask`,
//! `mulAffine`, `invertAffine` and the effects. The inputs are regenerated here exactly as in the
//! script (every generator below mirrors one of it, with the `Rng` of `uo-testkit`); outputs are
//! hashed with the canonical stream (exact f64 bits with `-0` distinguished, f32 buffers as LE
//! bits, NaN canonicalized) and compared digest by digest. A TS `RangeError` is the tag `E`, which
//! is any `Err` here (the variants are tested in `raster_effects.rs`).
//!
//! No facet calls `pow` (`exp`, `sin`, `cos`, `tan` are fdlibm in V8, `hypot` the Torque builtin):
//! valid from any Node x64; the replay checks V8, Node and the architecture only.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use uo_drawingml::{
    Affine, Align, BoxF, DEFAULT_TOL, Limits, Mask, PathSeg, Polygons, Reflection, Result, ShadowSpec, Subpaths, align_point, distance_to,
    fill_polygons, flatten_path, gaussian_blur, glow_mask, inner_shadow_mask, morph, outer_shadow_mask, preset_names, preset_outline,
    reflection_alpha, shadow_affine, shift_mask, soft_edge_mask, warp_mask,
};
use uo_jsnum::conv::canonicalize_nan;
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Replay, Report, Stream};

pub const CAPTURE: &str = include_str!("captures/raster.capture");
const L: Limits = Limits::NATIVE;

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    cap
}

fn f(s: &mut Stream, v: f64) {
    s.f64(canonicalize_nan(v));
}

fn u(s: &mut Stream, v: usize) {
    s.u32(u32::try_from(v).unwrap());
}

fn dig(build: impl FnOnce(&mut Stream)) -> String {
    let mut s = Stream::new();
    build(&mut s);
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

fn f32bytes(a: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(a.len() * 4);
    for v in a {
        let bits = if v.is_nan() { 0x7fc0_0000u32 } else { v.to_bits() };
        out.extend_from_slice(&bits.to_le_bytes());
    }
    out
}

fn mask_out(s: &mut Stream, m: &Mask) {
    s.u32(m.w());
    s.u32(m.h());
    s.bytes(&f32bytes(m.data()));
}

fn write_mask(s: &mut Stream, m: Result<Mask>) {
    match m {
        Ok(m) => {
            s.tag(b'K');
            mask_out(s, &m);
        }
        Err(_) => {
            s.tag(b'E');
        }
    }
}

fn affine_out(s: &mut Stream, a: &[f64; 6]) {
    for v in a {
        f(s, *v);
    }
}

fn polys_out(s: &mut Stream, p: &Polygons) {
    u(s, p.len());
    for poly in p.iter() {
        u(s, poly.len());
        for v in poly {
            f(s, *v);
        }
    }
}

fn new_mask(w: u32, h: u32) -> Mask {
    Mask::new(w, h, &L).unwrap()
}

fn st(v: f64) -> f32 {
    v as f32
}

// ── generators (mirrored from tools/capture-raster.mjs) ─────────────────────────────────────────

const MASK_DIMS: [(u32, u32); 8] = [(1, 1), (2, 2), (17, 9), (40, 40), (1, 30), (30, 1), (0, 0), (64, 48)];
const HOSTILE: [f64; 10] = [0.0, -0.0, 0.25, 0.5, 1.0, 2.0, -1.0, f64::NAN, 0.5, 0.4999999];

fn gen_mask(w: u32, h: u32, kind: u32, seed: u32) -> Mask {
    let mut m = new_mask(w, h);
    let mut r = Rng::new(seed);
    let (wu, hu) = (w as usize, h as usize);
    match kind {
        0 => {
            for i in 0..wu * hu {
                m.data_mut()[i] = st(r.below(9) as f64 / 8.0);
            }
        }
        1 => {
            let k = 1 + r.below(3);
            for _ in 0..k {
                let cx = r.below(wu * 2 + 1) as f64 / 2.0;
                let cy = r.below(hu * 2 + 1) as f64 / 2.0;
                let rad = 1.0 + r.below(wu.max(hu) * 2 + 1) as f64 / 2.0;
                let r2 = rad * rad;
                for y in 0..hu {
                    for x in 0..wu {
                        let dx = x as f64 + 0.5 - cx;
                        let dy = y as f64 + 0.5 - cy;
                        let d2 = dx * dx + dy * dy;
                        if d2 < r2 {
                            let c = ((r2 - d2) / r2) * 4.0;
                            let cov = if c > 1.0 { 1.0 } else { c };
                            let i = y * wu + x;
                            let cur = f64::from(m.data()[i]);
                            m.data_mut()[i] = st(if cur > cov { cur } else { cov });
                        }
                    }
                }
            }
        }
        2 => {
            for i in 0..wu * hu {
                m.data_mut()[i] = if r.below(3) == 0 { 1.0 } else { 0.0 };
            }
        }
        3 => {
            for i in 0..wu * hu {
                m.data_mut()[i] = st(HOSTILE[r.below(HOSTILE.len())]);
            }
        }
        _ => panic!("kind"),
    }
    m
}

const PENTA: [[f64; 2]; 5] = [[0.0, -1.0], [0.9510565, -0.309017], [0.5877853, 0.809017], [-0.5877853, 0.809017], [-0.9510565, -0.309017]];

fn gen_polys(seed: u32, w: u32, h: u32) -> Polygons {
    let mut r = Rng::new(seed);
    let (wu, hu) = (w as usize, h as usize);
    let mut polys = Polygons::new();
    let kind = r.below(8);
    let np = 1 + r.below(3);
    for _ in 0..np {
        let mut pts: Vec<f64> = Vec::new();
        macro_rules! x {
            () => {
                r.below((wu + 4) * 8 + 1) as f64 / 8.0 - 2.0
            };
        }
        macro_rules! y {
            () => {
                r.below((hu + 4) * 8 + 1) as f64 / 8.0 - 2.0
            };
        }
        match kind {
            0 => {
                let x0 = f64::floor(x!());
                let y0 = f64::floor(y!());
                let x1 = x0 + 1.0 + r.below(wu + 1) as f64;
                let y1 = y0 + 1.0 + r.below(hu + 1) as f64;
                pts.extend([x0, y0, x1, y0, x1, y1, x0, y1]);
            }
            1 => {
                let x0 = x!();
                let y0 = y!();
                let x1 = x0 + r.below(wu * 8 + 1) as f64 / 8.0;
                let y1 = y0 + r.below(hu * 8 + 1) as f64 / 8.0;
                pts.extend([x0, y0, x1, y0, x1, y1, x0, y1]);
            }
            2 => {
                for _ in 0..3 {
                    let px = x!();
                    let py = y!();
                    pts.extend([px, py]);
                }
            }
            3 => {
                let cx = x!();
                let cy = y!();
                let rr = 1.0 + r.below((wu + hu) * 4 + 1) as f64 / 8.0;
                for i in [0usize, 2, 4, 1, 3] {
                    pts.extend([cx + rr * PENTA[i][0], cy + rr * PENTA[i][1]]);
                }
            }
            4 => {
                let x0 = x!();
                let y0 = y!();
                let a = 1.0 + r.below(wu * 4 + 1) as f64 / 8.0;
                let dx = r.below(17) as f64 / 8.0;
                let dy = r.below(17) as f64 / 8.0;
                pts.extend([x0, y0, x0 + a, y0, x0 + a, y0 + a, x0, y0 + a]);
                pts.extend([x0 + dx, y0 + dy, x0 + dx, y0 + a + dy, x0 + a + dx, y0 + a + dy, x0 + a + dx, y0 + dy]);
            }
            5 => {
                let k = 3 + r.below(7);
                for _ in 0..k {
                    let px = x!();
                    let py = y!();
                    pts.extend([px, py]);
                }
            }
            6 => {
                let a = x!();
                let b = y!();
                let c = x!();
                let d = y!();
                pts.extend([a, b, c, b, c, b, c, d, a, d, a, b]);
            }
            _ => {
                let yy = y!();
                let xx = x!();
                pts.extend([-1e6, yy, 1e6, yy + 0.25, xx, 1e6, -1e6, -1e6]);
            }
        }
        for c in pts.chunks(2) {
            polys.push_point(c[0], c[1]);
        }
        polys.end_poly();
    }
    polys
}

const FILL_DIMS: [(u32, u32); 8] = [(1, 1), (5, 3), (16, 16), (33, 17), (64, 48), (0, 5), (5, 0), (3, 40)];

const PRESET_SIZES: [(f64, f64); 5] = [(1.0, 1.0), (7.0, 13.0), (64.0, 64.0), (200.0, 100.0), (400.0, 150.0)];
const AFFINES: [[f64; 6]; 10] = [
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    [1.7, 0.0, 0.0, 1.7, 3.25, -2.0],
    [0.01, 0.0, 0.0, 0.01, 0.0, 0.0],
    [0.866025403784, 0.5, -0.5, 0.866025403784, 10.0, 20.0],
    [1.0, 0.3, 0.7, 1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0, 1.0, 100.0, 0.0],
    [1.0, 2.0, 2.0, 4.0, 0.0, 0.0],
    [1.0, 0.0, f64::NAN, 1.0, 0.0, 0.0],
    [1e6, 0.0, 0.0, 1e6, 0.0, 0.0],
    [3.0, 0.0, 0.0, 0.25, -7.5, 11.0],
];
const TOLS: [f64; 5] = [0.35, 0.0, 1e-3, f64::NAN, -0.0];

const POOL: [f64; 15] = [0.0, 1.0, -1.0, 0.5, 2.5, 10.0, 100.25, 1e3, -37.5, 1e-3, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308, -0.0];

fn gen_subs(seed: u32) -> Subpaths {
    let mut r = Rng::new(seed);
    let mut subs = Subpaths::new();
    macro_rules! val {
        () => {
            if r.below(12) == 0 { POOL[r.below(POOL.len())] } else { (r.below(2001) as f64 - 1000.0) / 8.0 }
        };
    }
    macro_rules! ang {
        () => {
            if r.below(12) == 0 { POOL[r.below(POOL.len())] } else { (r.below(1257) as f64 - 628.0) / 100.0 }
        };
    }
    macro_rules! rad {
        () => {
            if r.below(12) == 0 { POOL[r.below(POOL.len())] } else { r.below(801) as f64 / 8.0 }
        };
    }
    let ns = 1 + r.below(4);
    for _ in 0..ns {
        let k = 1 + r.below(8);
        for _ in 0..k {
            let seg = match r.below(6) {
                0 => {
                    let x = val!();
                    let y = val!();
                    PathSeg::M { x, y }
                }
                1 => {
                    let x = val!();
                    let y = val!();
                    PathSeg::L { x, y }
                }
                2 => {
                    let x1 = val!();
                    let y1 = val!();
                    let x2 = val!();
                    let y2 = val!();
                    let x = val!();
                    let y = val!();
                    PathSeg::C { x1, y1, x2, y2, x, y }
                }
                3 => {
                    let x1 = val!();
                    let y1 = val!();
                    let x = val!();
                    let y = val!();
                    PathSeg::Q { x1, y1, x, y }
                }
                4 => {
                    let cx = val!();
                    let cy = val!();
                    let rx = rad!();
                    let ry = rad!();
                    let a0 = ang!();
                    let a1 = ang!();
                    let ccw = r.below(2) == 1;
                    PathSeg::E { cx, cy, rx, ry, a0, a1, ccw }
                }
                _ => PathSeg::Z,
            };
            subs.push(seg);
        }
        subs.end_sub();
    }
    subs
}

fn flatten(subs: &Subpaths, m: &[f64; 6], tol: f64) -> Polygons {
    flatten_path(subs, &Affine(*m), tol, &L).unwrap()
}

fn preset_sil(name: &str, w: u32, h: u32, even_odd: bool) -> Mask {
    let subs = preset_outline(Some(name), f64::from(w), f64::from(h), None).subs;
    let polys = flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap();
    let mut m = new_mask(w, h);
    fill_polygons(&mut m, &polys, even_odd, &L).unwrap();
    m
}

fn run_on(cap: &Capture, prefix: &str, body: impl FnOnce(&mut Replay<'_>)) -> Report {
    let cap = cap.subset(|k| k.starts_with(prefix));
    assert!(!cap.is_empty(), "no records for {prefix}");
    let mut replay = Replay::new(&cap);
    body(&mut replay);
    replay.finish()
}

fn run(prefix: &str, body: impl FnOnce(&mut Replay<'_>)) {
    let report = run_on(&capture(), prefix, body);
    eprintln!("{prefix}: {}", report.summary());
    report.assert_clean();
}

// ── flatten ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn flatten_matches_the_ts_bit_for_bit() {
    run("flatten/", |replay| flatten_body(replay, 1));
}

/// The replay of `flatten/`: every `every`-th preset (of 187) and every `every`-th block of 100 random subpath lists.
pub fn replay_flatten(cap: &Capture, every: usize) -> Report {
    run_on(cap, "flatten/", |replay| flatten_body(replay, every))
}

fn flatten_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for (index, name) in preset_names().enumerate() {
        if !index.is_multiple_of(every) {
            continue;
        }
        for (si, &(w, h)) in PRESET_SIZES.iter().enumerate() {
            let subs = preset_outline(Some(name), w, h, None).subs;
            let v = dig(|s| {
                for m in &AFFINES {
                    for tol in TOLS {
                        polys_out(s, &flatten(&subs, m, tol));
                    }
                }
            });
            replay.check(&format!("flatten/preset/{name}/{si}"), "v", &v);
        }
    }
    for b in 0..40u32 {
        if !(b as usize).is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for i in b * 100..(b + 1) * 100 {
                let m = &AFFINES[(i % 10) as usize];
                let tol = TOLS[((i / 10) % 5) as usize];
                s.u32(i);
                polys_out(s, &flatten(&gen_subs(i + 1), m, tol));
            }
        });
        replay.check(&format!("flatten/rand/{b}"), "v", &v);
    }
}

// ── fill ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn fill_matches_the_ts_bit_for_bit() {
    run("fill/", |replay| fill_body(replay, 1));
}

/// The replay of `fill/`: every `every`-th block of 60 random polygon sets and every `every`-th preset silhouette.
pub fn replay_fill(cap: &Capture, every: usize) -> Report {
    run_on(cap, "fill/", |replay| fill_body(replay, every))
}

fn fill_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for b in 0..50u32 {
        if !(b as usize).is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for i in b * 60..(b + 1) * 60 {
                let (w, h) = FILL_DIMS[i as usize % FILL_DIMS.len()];
                let mut m = new_mask(w, h);
                if i % 3 == 0 {
                    let mut r = Rng::new(i + 7);
                    for k in 0..(w * h) as usize {
                        m.data_mut()[k] = st(r.below(7) as f64 / 4.0);
                    }
                }
                fill_polygons(&mut m, &gen_polys(i + 1, w, h), i % 2 == 1, &L).unwrap();
                s.u32(i);
                mask_out(s, &m);
            }
        });
        replay.check(&format!("fill/rand/{b}"), "v", &v);
    }
    for (index, name) in preset_names().enumerate() {
        if !index.is_multiple_of(every) {
            continue;
        }
        for (si, &(w, h)) in [(48u32, 32u32), (21, 21)].iter().enumerate() {
            for eo in [false, true] {
                let v = dig(|s| mask_out(s, &preset_sil(name, w, h, eo)));
                replay.check(&format!("fill/preset/{name}/{si}/{}", if eo { "eo" } else { "nz" }), "v", &v);
            }
        }
    }
}

// ── blur, distance, morph, shift, warp, affine ──────────────────────────────────────────────────

const SIGMAS: [f64; 17] =
    [-1.0, 0.0, 0.04, 0.0499, 0.05, 0.3, 0.5, 1.0, 1.7, 2.5, 5.0, 10.0, 33.3, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1000.0];

#[test]
fn blur_matches_the_ts_bit_for_bit() {
    run("blur/", |replay| blur_body(replay, 1));
}

/// The replay of `blur/`: every `every`-th sigma (of 17).
pub fn replay_blur(cap: &Capture, every: usize) -> Report {
    run_on(cap, "blur/", |replay| blur_body(replay, every))
}

fn blur_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for (si, &sigma) in SIGMAS.iter().enumerate() {
        if !si.is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for &(w, h) in &MASK_DIMS {
                if sigma == 1000.0 && w * h > 200 {
                    continue;
                }
                for kind in 0..4u32 {
                    let m = gen_mask(w, h, kind, 100 + si as u32 * 10 + kind);
                    s.u32(w * 4 + kind);
                    write_mask(s, gaussian_blur(&m, sigma, &L));
                }
            }
        });
        replay.check(&format!("blur/{si}"), "v", &v);
    }
}

#[test]
fn distance_matches_the_ts_bit_for_bit() {
    run("edt/", |replay| distance_body(replay, 1));
}

/// The replay of `edt/`: every `every`-th (kind, direction) pair (of 8).
pub fn replay_distance(cap: &Capture, every: usize) -> Report {
    run_on(cap, "edt/", |replay| distance_body(replay, every))
}

fn distance_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for kind in 0..4u32 {
        for inside in [true, false] {
            if !(kind as usize * 2 + usize::from(!inside)).is_multiple_of(every) {
                continue;
            }
            let v = dig(|s| {
                for (i, &(w, h)) in MASK_DIMS.iter().enumerate() {
                    for seed in 0..3u32 {
                        let m = gen_mask(w, h, kind, 500 + i as u32 * 7 + seed);
                        s.u32(i as u32 * 3 + seed);
                        s.bytes(&f32bytes(&distance_to(&m, inside)));
                    }
                }
            });
            replay.check(&format!("edt/{kind}/{}", if inside { "in" } else { "out" }), "v", &v);
        }
    }
}

const RADII: [f64; 17] =
    [0.0, 0.005, -0.005, 0.0099, 0.01, -0.01, 0.5, -0.5, 1.0, -1.0, 3.7, -3.7, 20.0, -20.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

#[test]
fn morph_matches_the_ts_bit_for_bit() {
    run("morph/", |replay| morph_body(replay, 1));
}

/// The replay of `morph/`: every `every`-th radius (of 17).
pub fn replay_morph(cap: &Capture, every: usize) -> Report {
    run_on(cap, "morph/", |replay| morph_body(replay, every))
}

fn morph_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for (ri, &r) in RADII.iter().enumerate() {
        if !ri.is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for &(w, h) in &MASK_DIMS {
                if w * h > 1600 {
                    continue;
                }
                for kind in 0..4u32 {
                    let m = gen_mask(w, h, kind, 900 + ri as u32 * 10 + kind);
                    s.u32(w * 4 + kind);
                    write_mask(s, morph(&m, r, &L));
                }
            }
        });
        replay.check(&format!("morph/{ri}"), "v", &v);
    }
}

const SHIFTS: [(f64, f64); 14] = [
    (0.0, 0.0),
    (1.0, 0.0),
    (0.0, -1.0),
    (0.5, 0.5),
    (-2.25, 3.75),
    (100.0, 100.0),
    (-1e3, 5.0),
    (0.001, -0.001),
    (f64::NAN, 0.0),
    (0.0, f64::NAN),
    (f64::INFINITY, 0.0),
    (0.0, f64::NEG_INFINITY),
    (1e308, 0.0),
    (-0.0, -0.0),
];

#[test]
fn shift_matches_the_ts_bit_for_bit() {
    run("shift/", |replay| shift_body(replay, 1));
}

/// The replay of `shift/`: every `every`-th offset (of 14).
pub fn replay_shift(cap: &Capture, every: usize) -> Report {
    run_on(cap, "shift/", |replay| shift_body(replay, every))
}

fn shift_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for (ki, &(dx, dy)) in SHIFTS.iter().enumerate() {
        if !ki.is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for &(w, h) in &MASK_DIMS {
                for kind in 0..4u32 {
                    let m = gen_mask(w, h, kind, 1300 + ki as u32 * 10 + kind);
                    for size in [None, Some((w + 3, h + 2)), Some((1, 1)), Some((0, 3))] {
                        s.u32(w * 4 + kind);
                        write_mask(s, shift_mask(&m, dx, dy, size, &L));
                    }
                }
            }
        });
        replay.check(&format!("shift/{ki}"), "v", &v);
    }
}

const WARPS: [[f64; 6]; 11] = [
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 1.0, 0.5, 0.5],
    [0.5, 0.0, 0.0, 0.5, 0.0, 0.0],
    [2.0, 0.0, 0.0, 2.0, -3.0, 1.0],
    [0.866025403784, -0.5, 0.5, 0.866025403784, 4.0, -6.0],
    [1.0, 0.3, 0.7, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 1.0, 2.0],
    [f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0],
    [f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0],
    [1e308, 0.0, 0.0, 1e308, 0.0, 0.0],
    [-1.0, 0.0, 0.0, 1.0, 40.0, 0.0],
];

#[test]
fn warp_matches_the_ts_bit_for_bit() {
    run("warp/", |replay| warp_body(replay, 1));
}

/// The replay of `warp/`: every `every`-th inverse affine (of 11).
pub fn replay_warp(cap: &Capture, every: usize) -> Report {
    run_on(cap, "warp/", |replay| warp_body(replay, every))
}

fn warp_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    for (ai, a) in WARPS.iter().enumerate() {
        if !ai.is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            for &(w, h) in &MASK_DIMS {
                for kind in 0..4u32 {
                    let m = gen_mask(w, h, kind, 1700 + ai as u32 * 10 + kind);
                    for (tw, th) in [(w, h), (w + 5, h + 1), (1, 1), (0, 4)] {
                        s.u32(w * 4 + kind);
                        write_mask(s, warp_mask(&m, &Affine(*a), tw, th, &L));
                    }
                }
            }
        });
        replay.check(&format!("warp/{ai}"), "v", &v);
    }
}

const APOOL: [f64; 12] = [0.0, 1.0, -1.0, 0.5, 2.0, -3.5, 1e-9, 1e9, f64::NAN, f64::INFINITY, 0.3, -0.0];

#[test]
fn affine_matches_the_ts_bit_for_bit() {
    run("affine/", |replay| affine_body(replay, 1));
}

/// The replay of `affine/`: `mulAffine` and `invertAffine` over 3,000 random pairs each (whole).
pub fn replay_affine(cap: &Capture, every: usize) -> Report {
    run_on(cap, "affine/", |replay| affine_body(replay, every))
}

fn affine_body(replay: &mut Replay<'_>, _every: usize) {
    let v = dig(|s| {
        let mut r = Rng::new(31);
        for _ in 0..3000 {
            let mut m = [0.0; 6];
            let mut n = [0.0; 6];
            for c in &mut m {
                *c = APOOL[r.below(APOOL.len())];
            }
            for c in &mut n {
                *c = APOOL[r.below(APOOL.len())];
            }
            affine_out(s, &Affine(m).mul(&Affine(n)).0);
        }
    });
    replay.check("affine/mul", "v", &v);
    let v = dig(|s| {
        let mut r = Rng::new(37);
        for _ in 0..3000 {
            let mut m = [0.0; 6];
            for c in &mut m {
                *c = APOOL[r.below(APOOL.len())];
            }
            affine_out(s, &Affine(m).invert().0);
        }
    });
    replay.check("affine/invert", "v", &v);
}

// ── effects ─────────────────────────────────────────────────────────────────────────────────────

const ALGNS: [Option<&str>; 13] = [
    None,
    Some("tl"),
    Some("t"),
    Some("tr"),
    Some("l"),
    Some("ctr"),
    Some("r"),
    Some("bl"),
    Some("b"),
    Some("br"),
    Some(""),
    Some("xx"),
    Some("TL"),
];

fn gen_spec(r: &mut Rng) -> (ShadowSpec, BoxF, f64) {
    fn pick<T: Copy>(r: &mut Rng, a: &[T]) -> T {
        a[r.below(a.len())]
    }
    let blur_pt = pick(r, &[0.0, 1.0, 6.0, 12.5]);
    let dist_pt = pick(r, &[0.0, 3.0, -4.0, 7.25]);
    let dir_deg = pick(r, &[0.0, 45.0, 90.0, 135.0, 200.0, 360.0, -30.0, f64::NAN]);
    let scales = [None, Some(1.0), Some(0.5), Some(1.5), Some(0.0), Some(-1.0)];
    let sx = pick(r, &scales);
    let sy = pick(r, &scales);
    let skews = [None, Some(0.0), Some(30.0), Some(89.9), Some(-20.0), Some(90.0)];
    let kx_deg = pick(r, &skews);
    let ky_deg = pick(r, &skews);
    let algn = pick(r, &ALGNS);
    let bx = BoxF { x: r.below(5) as f64, y: r.below(5) as f64, w: 10.0 + r.below(60) as f64, h: 10.0 + r.below(60) as f64 };
    let ppt = pick(r, &[1.0, 1.5, 2.5, 0.75]);
    (ShadowSpec { blur_pt, dist_pt, dir_deg, sx, sy, kx_deg, ky_deg, algn: Align::from_attr(algn) }, bx, ppt)
}

#[test]
fn effects_match_the_ts_bit_for_bit() {
    run("fx/", |replay| effects_body(replay, 1));
}

/// The replay of `fx/`: `alignPoint`, `shadowAffine` and the reflection whole; every `every`-th
/// silhouette (of 5) for the four masks.
pub fn replay_effects(cap: &Capture, every: usize) -> Report {
    run_on(cap, "fx/", |replay| effects_body(replay, every))
}

fn effects_body(replay: &mut Replay<'_>, every: usize) {
    let every = every.max(1);
    let v = dig(|s| {
        for a in ALGNS {
            for (x, y, w, h) in [
                (0.0, 0.0, 100.0, 50.0),
                (10.5, -3.0, 7.0, 0.0),
                (1e3, 1e3, 1e-3, 1e9),
                (f64::NAN, 0.0, 1.0, 1.0),
                (0.0, 0.0, f64::INFINITY, 2.0),
            ] {
                let (px, py) = align_point(Align::from_attr(a), x, y, w, h);
                f(s, px);
                f(s, py);
            }
        }
    });
    replay.check("fx/alignPoint", "v", &v);
    let v = dig(|s| {
        let mut r = Rng::new(41);
        for _ in 0..2000 {
            let (spec, bx, ppt) = gen_spec(&mut r);
            affine_out(s, &shadow_affine(&spec, &bx, ppt).0);
        }
    });
    replay.check("fx/shadowAffine", "v", &v);

    let sils: [(&str, Mask); 5] = [
        ("blob1", gen_mask(40, 30, 1, 2001)),
        ("blob2", gen_mask(64, 48, 1, 2002)),
        ("star5", preset_sil("star5", 48, 32, false)),
        ("heart", preset_sil("heart", 33, 21, false)),
        ("binary", gen_mask(17, 9, 2, 2003)),
    ];
    let rads = [0.0, 0.004, 1.0, 3.7, 10.0, -5.0, f64::NAN, f64::INFINITY];
    let ppts = [1.0, 2.5, 0.75];
    for (index, (name, sil)) in sils.iter().enumerate() {
        if !index.is_multiple_of(every) {
            continue;
        }
        let v = dig(|s| {
            let mut r = Rng::new(51);
            for i in 0..40u32 {
                let (spec, bx, ppt) = gen_spec(&mut r);
                s.u32(i);
                write_mask(s, outer_shadow_mask(sil, &bx, &spec, ppt, &L));
            }
        });
        replay.check(&format!("fx/outer/{name}"), "v", &v);
        let v = dig(|s| {
            let mut r = Rng::new(53);
            for i in 0..40u32 {
                let (spec, _, ppt) = gen_spec(&mut r);
                s.u32(i);
                write_mask(s, inner_shadow_mask(sil, spec.blur_pt, spec.dist_pt, spec.dir_deg, ppt, &L));
            }
        });
        replay.check(&format!("fx/inner/{name}"), "v", &v);
        let v = dig(|s| {
            for rad in rads {
                for ppt in ppts {
                    s.u32(0);
                    write_mask(s, glow_mask(sil, rad, ppt, &L));
                }
            }
        });
        replay.check(&format!("fx/glow/{name}"), "v", &v);
        let v = dig(|s| {
            for rad in rads {
                for ppt in ppts {
                    s.u32(0);
                    write_mask(s, soft_edge_mask(sil, rad, ppt, &L));
                }
            }
        });
        replay.check(&format!("fx/soft/{name}"), "v", &v);
    }

    let v = dig(|s| {
        let ts = [-1.0, 0.0, 0.1, 0.5, 0.9, 1.0, 1.5, f64::NAN, f64::INFINITY];
        let pos = [0.0, 0.2, 0.5, 1.0, f64::NAN, -0.1];
        let al = [0.0, 0.3, 1.0, -1.0, f64::NAN];
        for t in ts {
            for st_pos in pos {
                for end_pos in pos {
                    for st_a in al {
                        for end_a in al {
                            f(s, reflection_alpha(t, &Reflection { st_a, st_pos, end_a, end_pos }));
                        }
                    }
                }
            }
        }
    });
    replay.check("fx/reflection", "v", &v);
}
