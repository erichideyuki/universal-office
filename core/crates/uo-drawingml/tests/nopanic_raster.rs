//! No panic and no hang on hostile input (R10) for `raster.rs` and `effects.rs`: polygons and
//! subpaths with NaN, the infinities, the extremes and random bit patterns (the fill of a polygon
//! with a non-finite coordinate is outside the parity corpus but must still end), masks of every
//! dimension (including `w x 0` with a huge `w`), hostile radii, sigmas and offsets, sizes and
//! limits that make every allocation fail its check. The functions are only called: what they
//! return is compared with the TS by `raster_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::hint::black_box;

use uo_drawingml::*;
use uo_testkit::generate::Rng;

const L: Limits = Limits::NATIVE;

fn bits(r: &mut Rng) -> f64 {
    f64::from_bits((u64::from(r.next_u32()) << 32) | u64::from(r.next_u32()))
}

fn hostile(r: &mut Rng) -> f64 {
    match r.below(14) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => f64::MAX,
        4 => f64::MIN,
        5 => f64::MIN_POSITIVE,
        6 => 5e-324,
        7 => -0.0,
        8 => 0.0,
        9 => bits(r),
        10 => f64::from(r.next_u32()) - 2_147_483_648.0,
        11 => 1e300,
        12 => (r.below(4001) as f64 - 2000.0) / 8.0,
        _ => (r.below(65) as f64 - 8.0) / 4.0,
    }
}

fn cap(v: f64, hi: f64) -> f64 {
    if v > hi { hi } else { v }
}

fn mask_of(r: &mut Rng, w: u32, h: u32) -> Mask {
    let a: Vec<f32> = (0..w * h)
        .map(|_| match r.below(8) {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => -1.0,
            3 => 2.0,
            _ => r.below(9) as f32 / 8.0,
        })
        .collect();
    Mask::from_vec(w, h, a).unwrap()
}

fn hostile_polys(r: &mut Rng) -> Polygons {
    let mut p = Polygons::new();
    for _ in 0..1 + r.below(4) {
        for _ in 0..r.below(12) {
            let (x, y) = (hostile(r), hostile(r));
            p.push_point(x, y);
        }
        p.end_poly();
    }
    p
}

fn hostile_subs(r: &mut Rng) -> Subpaths {
    let mut s = Subpaths::new();
    for _ in 0..r.below(4) {
        for _ in 0..r.below(9) {
            let seg = match r.below(6) {
                0 => PathSeg::M { x: hostile(r), y: hostile(r) },
                1 => PathSeg::L { x: hostile(r), y: hostile(r) },
                2 => PathSeg::C { x1: hostile(r), y1: hostile(r), x2: hostile(r), y2: hostile(r), x: hostile(r), y: hostile(r) },
                3 => PathSeg::Q { x1: hostile(r), y1: hostile(r), x: hostile(r), y: hostile(r) },
                4 => PathSeg::E {
                    cx: hostile(r),
                    cy: hostile(r),
                    rx: hostile(r),
                    ry: hostile(r),
                    a0: hostile(r),
                    a1: hostile(r),
                    ccw: r.below(2) == 1,
                },
                _ => PathSeg::Z,
            };
            s.push(seg);
        }
        s.end_sub();
    }
    s
}

fn hostile_affine(r: &mut Rng) -> Affine {
    Affine([hostile(r), hostile(r), hostile(r), hostile(r), hostile(r), hostile(r)])
}

#[test]
fn flatten_survives_hostile_subpaths_tolerances_and_limits() {
    let mut r = Rng::new(0x5eed_1001);
    let iterations = if cfg!(debug_assertions) { 3_000 } else { 60_000 };
    for i in 0..iterations {
        let subs = hostile_subs(&mut r);
        let m = if i % 3 == 0 { Affine::IDENTITY } else { hostile_affine(&mut r) };
        let tol = if i % 5 == 0 { DEFAULT_TOL } else { hostile(&mut r) };
        let limits = Limits { max_path_points: [0, 1, 5, 300, 1 << 22][r.below(5)], ..L };
        black_box(flatten_path(&subs, &m, tol, &limits)).ok();
    }
}

#[test]
fn fill_survives_hostile_polygons_and_masks() {
    let mut r = Rng::new(0x5eed_1002);
    let iterations = if cfg!(debug_assertions) { 3_000 } else { 60_000 };
    let dims = [(0, 0), (1, 1), (3, 5), (16, 9), (0, 7), (7, 0), (40, 3)];
    for i in 0..iterations {
        let (w, h) = dims[r.below(dims.len())];
        let mut m = mask_of(&mut r, w, h);
        let polys = hostile_polys(&mut r);
        let limits = Limits { max_fill_work: [0, 3, 1 << 20, 1 << 30][r.below(4)], ..L };
        black_box(fill_polygons(&mut m, &polys, i % 2 == 0, &limits)).ok();
        black_box(m);
    }
    // a mask whose width does not fit a row buffer: nothing to fill, nothing allocated
    let mut wide = Mask::from_vec(u32::MAX, 0, Vec::new()).unwrap();
    let mut p = Polygons::new();
    for (x, y) in [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)] {
        p.push_point(x, y);
    }
    p.end_poly();
    assert_eq!(fill_polygons(&mut wide, &p, false, &L), Ok(()));
}

#[test]
fn flattened_hostile_paths_can_be_filled() {
    let mut r = Rng::new(0x5eed_1003);
    let iterations = if cfg!(debug_assertions) { 300 } else { 6_000 };
    for _ in 0..iterations {
        let subs = hostile_subs(&mut r);
        let m = hostile_affine(&mut r);
        if let Ok(polys) = flatten_path(&subs, &m, DEFAULT_TOL, &Limits { max_path_points: 5_000, ..L }) {
            let mut mask = Mask::new(24, 24, &L).unwrap();
            black_box(fill_polygons(&mut mask, &polys, r.below(2) == 0, &L)).ok();
        }
    }
}

#[test]
fn blur_distance_and_morph_survive_hostile_masks_and_parameters() {
    let mut r = Rng::new(0x5eed_1004);
    let iterations = if cfg!(debug_assertions) { 600 } else { 6_000 };
    let dims = [(0, 0), (1, 1), (2, 6), (13, 7), (0, 4), (4, 0), (24, 24)];
    for _ in 0..iterations {
        let (w, h) = dims[r.below(dims.len())];
        let m = mask_of(&mut r, w, h);
        let limits = Limits { max_kernel_radius: [0, 4, 64, 4096][r.below(4)], max_mask_pixels: [0, 100, 5_000, 1 << 26][r.below(4)], ..L };
        black_box(gaussian_blur(&m, cap(hostile(&mut r).abs(), 40.0), &limits)).ok();
        black_box(gaussian_blur(&m, hostile(&mut r), &limits)).ok();
        black_box(distance_to(&m, r.below(2) == 0));
        black_box(morph(&m, hostile(&mut r), &limits)).ok();
        black_box(morph(&m, (r.below(81) as f64 - 40.0) / 4.0, &limits)).ok();
    }
    // sizes that do not fit anything
    let wide = Mask::from_vec(u32::MAX, 0, Vec::new()).unwrap();
    let tall = Mask::from_vec(0, u32::MAX, Vec::new()).unwrap();
    for m in [&wide, &tall] {
        black_box(gaussian_blur(m, 2.0, &L)).ok();
        black_box(gaussian_blur(m, f64::NAN, &L)).ok();
        black_box(distance_to(m, true));
        black_box(morph(m, 3.0, &L)).ok();
        black_box(morph(m, f64::NAN, &L)).ok();
        black_box(shift_mask(m, 1.0, 1.0, None, &L)).ok();
        black_box(warp_mask(m, &Affine::IDENTITY, 0, 0, &L)).ok();
    }
}

#[test]
fn shift_warp_and_effects_survive_hostile_values_and_sizes() {
    let mut r = Rng::new(0x5eed_1005);
    let iterations = if cfg!(debug_assertions) { 600 } else { 6_000 };
    let dims = [(0, 0), (1, 1), (3, 5), (16, 9), (0, 7), (7, 0), (30, 20)];
    for _ in 0..iterations {
        let (w, h) = dims[r.below(dims.len())];
        let m = mask_of(&mut r, w, h);
        let limits = Limits { max_mask_pixels: [0, 50, 5_000, 1 << 26][r.below(4)], ..L };
        let size = (r.below(2) == 0).then(|| (r.below(40) as u32, r.below(40) as u32));
        black_box(shift_mask(&m, hostile(&mut r), hostile(&mut r), size, &limits)).ok();
        black_box(warp_mask(&m, &hostile_affine(&mut r), r.below(40) as u32, r.below(40) as u32, &limits)).ok();
        let spec = ShadowSpec {
            blur_pt: cap(hostile(&mut r).abs(), 30.0),
            dist_pt: hostile(&mut r),
            dir_deg: hostile(&mut r),
            sx: (r.below(2) == 0).then(|| hostile(&mut r)),
            sy: (r.below(2) == 0).then(|| hostile(&mut r)),
            kx_deg: (r.below(2) == 0).then(|| hostile(&mut r)),
            ky_deg: (r.below(2) == 0).then(|| hostile(&mut r)),
            algn: Align::from_attr(["tl", "t", "tr", "l", "ctr", "r", "bl", "b", "br", "", "zz"].get(r.below(12)).copied()),
        };
        let bx = BoxF { x: hostile(&mut r), y: hostile(&mut r), w: hostile(&mut r), h: hostile(&mut r) };
        let ppt = hostile(&mut r);
        black_box((align_point(spec.algn, bx.x, bx.y, bx.w, bx.h), shadow_affine(&spec, &bx, ppt)));
        black_box(outer_shadow_mask(&m, &bx, &spec, 1.0 + (r.below(8) as f64) / 4.0, &limits)).ok();
        black_box(inner_shadow_mask(&m, spec.blur_pt, spec.dist_pt, spec.dir_deg, 1.0, &limits)).ok();
        let rad = if r.below(3) == 0 { hostile(&mut r) } else { r.below(60) as f64 / 4.0 - 2.0 };
        black_box(glow_mask(&m, rad, 1.0, &limits)).ok();
        black_box(soft_edge_mask(&m, rad, 1.0, &limits)).ok();
        black_box(reflection_alpha(
            hostile(&mut r),
            &Reflection { st_a: hostile(&mut r), st_pos: hostile(&mut r), end_a: hostile(&mut r), end_pos: hostile(&mut r) },
        ));
    }
}

#[test]
fn sizes_over_the_limits_fail_their_check() {
    let m = Mask::new(4, 4, &L).unwrap();
    let big = (u32::MAX, u32::MAX);
    assert_eq!(shift_mask(&m, 0.0, 0.0, Some(big), &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(warp_mask(&m, &Affine::IDENTITY, big.0, big.1, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(morph(&m, 1e9, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(morph(&m, f64::MAX, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(gaussian_blur(&m, 1e9, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    // the largest kernel the default limit accepts, on a tiny mask
    assert!(gaussian_blur(&m, f64::from(L.max_kernel_radius) / 3.0, &L).is_ok());
    // zero limits: everything with a size is refused
    let zero = Limits { max_mask_pixels: 0, max_kernel_radius: 0, max_path_points: 0, max_fill_work: 0, ..L };
    assert_eq!(Mask::new(1, 1, &zero), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(gaussian_blur(&m, 1.0, &zero), Err(Error::Limit(LimitKind::KernelRadius)));
    assert_eq!(shift_mask(&m, 0.0, 0.0, None, &zero), Err(Error::Limit(LimitKind::MaskPixels)));
}
