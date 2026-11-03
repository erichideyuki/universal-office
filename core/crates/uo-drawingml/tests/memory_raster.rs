//! Memory gate of `raster.rs` and `effects.rs` (M1, M2, M3): every public operation and every row of
//! the error table returns the live bytes exactly to their starting value, outputs `black_box`ed;
//! the error paths allocate nothing before their check (`flatten_path` frees what it built); a soak
//! of the raster pipeline is flat with a tolerance of 0 B; the allocated and peak bytes of the big
//! effects are measured at 1 and 4 Mpx (the numbers of the module record) and kept under the
//! budgets of the plan.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::*;

const L: Limits = Limits::NATIVE;

fn disc(w: u32, h: u32) -> Mask {
    let mut m = Mask::new(w, h, &L).unwrap();
    let (cx, cy, r) = (f64::from(w) / 2.0, f64::from(h) / 2.0, f64::from(w.min(h)) / 3.0);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            if dx * dx + dy * dy < r * r {
                m.data_mut()[y * w as usize + x] = 1.0;
            }
        }
    }
    m
}

fn outline(name: &str, w: f64, h: f64) -> Subpaths {
    preset_outline(Some(name), w, h, None).subs
}

fn spec(blur_pt: f64, dist_pt: f64, dir_deg: f64) -> ShadowSpec {
    ShadowSpec { blur_pt, dist_pt, dir_deg, sx: Some(1.2), sy: None, kx_deg: Some(10.0), ky_deg: None, algn: Align::from_attr(Some("br")) }
}

#[test]
fn m1_flatten_and_fill() {
    clear_caches();
    for name in ["rect", "star5", "heart", "cloud", "gear9", "circularArrow", "nope"] {
        for (w, h) in [(1.0, 1.0), (64.0, 48.0), (400.0, 150.0)] {
            let subs = outline(name, w, h);
            for m in [Affine::IDENTITY, Affine([1.7, 0.3, -0.3, 1.7, 3.0, 4.0]), Affine([0.0; 6])] {
                assert_no_leak(|| black_box(flatten_path(&subs, &m, DEFAULT_TOL, &L)));
            }
            assert_no_leak(|| {
                let polys = flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap();
                let mut mask = Mask::new(w as u32 + 1, h as u32 + 1, &L).unwrap();
                black_box(fill_polygons(&mut mask, &polys, false, &L)).unwrap();
                black_box(fill_polygons(&mut mask, &polys, true, &L)).unwrap();
                black_box((polys.len(), polys.get(0).map(<[f64]>::len), polys.iter().count(), mask.clone()))
            });
        }
    }
    let empty = Subpaths::new();
    assert_no_leak(|| black_box(flatten_path(&empty, &Affine::IDENTITY, DEFAULT_TOL, &L)));
    assert_no_leak(|| black_box(Polygons::new()));
}

#[test]
fn m1_flatten_error_path_releases_what_it_built() {
    let subs = outline("heart", 200.0, 200.0);
    for max in [0u64, 1, 7, 50] {
        let (r, usage) = measure(|| flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &Limits { max_path_points: max, ..L }));
        assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::PathPoints));
        assert_eq!(usage.live_delta, 0, "max {max}: the partial output is dropped");
    }
}

#[test]
fn m1_blur_distance_morph() {
    let m = disc(48, 40);
    for sigma in [-1.0, 0.0, 0.04, 0.05, 0.3, 1.0, 2.5, 10.0, 33.3, f64::NAN] {
        assert_no_leak(|| black_box(gaussian_blur(&m, sigma, &L)));
    }
    assert_no_leak(|| black_box(gaussian_blur(&m, f64::INFINITY, &L)));
    assert_no_leak(|| black_box(gaussian_blur(&m, 1e6, &L)));
    for inside in [true, false] {
        assert_no_leak(|| black_box(distance_to(&m, inside)));
    }
    for r in [0.0, 0.005, 0.5, -0.5, 3.7, -3.7, 20.0, -20.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e9] {
        assert_no_leak(|| black_box(morph(&m, r, &L)));
    }
    for e in [Mask::new(0, 0, &L).unwrap(), Mask::new(0, 9, &L).unwrap(), Mask::new(9, 0, &L).unwrap()] {
        assert_no_leak(|| black_box((gaussian_blur(&e, 2.0, &L), distance_to(&e, true), morph(&e, 2.0, &L), morph(&e, f64::NAN, &L))));
    }
}

#[test]
fn m1_shift_warp() {
    let m = disc(33, 21);
    for (dx, dy) in [(0.0, 0.0), (3.5, -2.25), (-100.0, 100.0), (f64::NAN, 1.0), (f64::INFINITY, 0.0)] {
        for size in [None, Some((40, 30)), Some((0, 5))] {
            assert_no_leak(|| black_box(shift_mask(&m, dx, dy, size, &L)));
        }
    }
    for a in [Affine::IDENTITY, Affine([0.5, 0.2, -0.2, 0.5, 4.0, 5.0]), Affine([0.0; 6]), Affine([f64::NAN; 6])] {
        assert_no_leak(|| black_box(warp_mask(&m, &a, 33, 21, &L)));
        assert_no_leak(|| black_box(warp_mask(&m, &a, 7, 9, &L)));
    }
}

#[test]
fn m1_effects() {
    let sil = disc(64, 48);
    let bx = BoxF { x: 0.0, y: 0.0, w: 64.0, h: 48.0 };
    assert_no_leak(|| black_box((align_point(Align::Br, 1.0, 2.0, 3.0, 4.0), Align::from_attr(Some("tl")), Align::from_attr(None))));
    assert_no_leak(|| black_box(shadow_affine(&spec(1.0, 2.0, 30.0), &bx, 1.5)));
    assert_no_leak(|| black_box(reflection_alpha(0.4, &Reflection { st_a: 0.5, st_pos: 0.0, end_a: 0.0, end_pos: 0.9 })));
    for ppt in [1.0, 2.5] {
        for (b, d, a) in [(0.0, 0.0, 0.0), (6.0, 3.0, 45.0), (12.5, -4.0, f64::NAN)] {
            assert_no_leak(|| black_box(outer_shadow_mask(&sil, &bx, &spec(b, d, a), ppt, &L)));
            assert_no_leak(|| black_box(inner_shadow_mask(&sil, b, d, a, ppt, &L)));
        }
        for rad in [0.0, 0.004, 3.7, 10.0, -5.0, f64::NAN, f64::INFINITY] {
            assert_no_leak(|| black_box(glow_mask(&sil, rad, ppt, &L)));
            assert_no_leak(|| black_box(soft_edge_mask(&sil, rad, ppt, &L)));
        }
    }
    assert_no_leak(|| black_box(outer_shadow_mask(&sil, &bx, &spec(1e300, 0.0, 0.0), 1.0, &L)));
    assert_no_leak(|| black_box(inner_shadow_mask(&sil, f64::INFINITY, 0.0, 0.0, 1.0, &L)));
}

/// Every `Err` row of the table of the record: the check comes before the first allocation, so
/// nothing was allocated at all.
#[test]
fn m1_error_paths_allocate_nothing() {
    let m = disc(16, 16);
    let small = Limits { max_mask_pixels: 300, max_kernel_radius: 8, max_fill_work: 5, ..L };
    let polys = {
        let subs = outline("rect", 16.0, 16.0);
        flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap()
    };
    let mut target = m.clone();
    let none = |u: &uo_alloc::Usage| assert_eq!(u.allocated, 0);
    let (r, u) = measure(|| Mask::new(100, 100, &small));
    assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    none(&u);
    let (r, u) = measure(|| fill_polygons(&mut target, &polys, false, &small));
    assert_eq!(r, Err(Error::Limit(LimitKind::FillWork)));
    none(&u);
    let (r, u) = measure(|| gaussian_blur(&m, 3.0, &small));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::KernelRadius));
    none(&u);
    let (r, u) = measure(|| gaussian_blur(&m, f64::INFINITY, &L));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::KernelRadius));
    none(&u);
    for (r, why) in [
        (f64::NAN, Error::InvalidArgument),
        (f64::INFINITY, Error::Limit(LimitKind::MaskPixels)),
        (f64::NEG_INFINITY, Error::Limit(LimitKind::MaskPixels)),
        (5.0, Error::Limit(LimitKind::MaskPixels)),
    ] {
        let (res, u) = measure(|| morph(&m, r, &small));
        assert_eq!(res.unwrap_err(), why, "r {r}");
        none(&u);
    }
    let (r, u) = measure(|| shift_mask(&m, 1.0, 1.0, Some((100, 100)), &small));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::MaskPixels));
    none(&u);
    let (r, u) = measure(|| warp_mask(&m, &Affine::IDENTITY, 100, 100, &small));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::MaskPixels));
    none(&u);
    let bx = BoxF { x: 0.0, y: 0.0, w: 16.0, h: 16.0 };
    let (r, u) = measure(|| glow_mask(&m, 10.0, 1.0, &small));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::MaskPixels));
    none(&u);
    let (r, u) = measure(|| soft_edge_mask(&m, f64::NAN, 1.0, &L));
    assert_eq!(r.unwrap_err(), Error::InvalidArgument);
    none(&u);
    let (r, u) = measure(|| outer_shadow_mask(&m, &bx, &spec(1e300, 0.0, 0.0), 1.0, &L));
    assert_eq!(r.unwrap_err(), Error::Limit(LimitKind::KernelRadius));
    assert!(u.allocated > 0 && u.live_delta == 0, "the warped plane was built and released");
}

/// M2: the raster pipeline, flat with a tolerance of 0 B (no caches).
#[test]
fn m2_soak_of_raster_and_effects() {
    let names = ["star5", "heart", "cloud", "gear9", "circularArrow", "rect", "ellipse", "chevron"];
    let iterations: usize =
        std::env::var("UO_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 4 } else { 100 });
    let side: u32 = if cfg!(debug_assertions) { 128 } else { 512 };
    let sil = disc(side, side);
    let bx = BoxF { x: 0.0, y: 0.0, w: f64::from(side), h: f64::from(side) };
    let series = soak(iterations, if cfg!(debug_assertions) { 1 } else { 5 }, |_| {
        for name in names {
            let subs = outline(name, 256.0, 256.0);
            let polys = flatten_path(&subs, &Affine([1.0, 0.0, 0.0, 1.0, 2.0, 2.0]), DEFAULT_TOL, &L).unwrap();
            let mut m = Mask::new(260, 260, &L).unwrap();
            black_box(fill_polygons(&mut m, &polys, false, &L)).unwrap();
            black_box(distance_to(&m, true));
        }
        black_box(outer_shadow_mask(&sil, &bx, &spec(6.0, 3.0, 45.0), 1.0, &L)).unwrap();
        black_box(inner_shadow_mask(&sil, 6.0, 3.0, 45.0, 1.0, &L)).unwrap();
        black_box(glow_mask(&sil, 8.0, 1.0, &L)).unwrap();
        black_box(soft_edge_mask(&sil, 6.0, 1.0, &L)).unwrap();
        black_box(morph(&sil, f64::NAN, &L)).unwrap_err();
        black_box(gaussian_blur(&sil, f64::INFINITY, &L)).unwrap_err();
    });
    assert_flat(&series, 1, 0);
}

fn budget(label: &str, n: u64, usage: &uo_alloc::Usage, formula: u64) {
    eprintln!(
        "M3 {label}: N = {n} px, allocated {} B ({:.2} N), peak above start {} B ({:.2} N), budget {formula} B ({:.2} N)",
        usage.allocated,
        usage.allocated as f64 / n as f64,
        usage.peak_above_start,
        usage.peak_above_start as f64 / n as f64,
        formula as f64 / n as f64
    );
    assert!(usage.allocated <= formula + formula / 50 + 65_536, "{label}: allocated {} over the budget {formula}", usage.allocated);
}

/// M3: allocated and peak bytes of the big operations at 1 Mpx and 4 Mpx (512 x 512 and 1024 x 1024
/// in a debug build), against the table of the plan (N pixels, Np the padded plane).
#[test]
fn m3_budgets_of_the_big_operations() {
    let sides: &[u32] = if cfg!(debug_assertions) { &[512] } else { &[1024, 2048] };
    for &side in sides {
        let n = u64::from(side) * u64::from(side);
        let sil = disc(side, side);
        let bx = BoxF { x: 0.0, y: 0.0, w: f64::from(side), h: f64::from(side) };
        let (_, u) = measure(|| Mask::new(side, side, &L).unwrap());
        budget("Mask::new", n, &u, 4 * n);
        let polys = flatten_path(&outline("star5", f64::from(side), f64::from(side)), &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap();
        let mut m = Mask::new(side, side, &L).unwrap();
        let edges = polys.iter().map(|p| (p.len() / 2) as u64).sum::<u64>();
        let (r, u) = measure(|| fill_polygons(&mut m, &polys, false, &L));
        r.unwrap();
        budget("fill_polygons (star5)", n, &u, (u64::from(side) + 1) * 4 + edges * 40 + edges * 16);
        let (b, u) = measure(|| gaussian_blur(&sil, 4.0, &L).unwrap());
        budget("gaussian_blur sigma 4", n, &u, 8 * n + 4 * 13);
        drop(b);
        let (d, u) = measure(|| distance_to(&sil, true));
        budget("distance_to", n, &u, 12 * n + 28 * u64::from(side));
        drop(d);
        let np = (u64::from(side) + 2 * 6) * (u64::from(side) + 2 * 6);
        let (mo, u) = measure(|| morph(&sil, 4.0, &L).unwrap());
        budget("morph r 4", n, &u, 16 * np + 4 * n);
        drop(mo);
        let (o, u) = measure(|| outer_shadow_mask(&sil, &bx, &spec(6.0, 3.0, 45.0), 1.0, &L).unwrap());
        budget("outer_shadow_mask", n, &u, 12 * n + 8 * u64::from(side));
        drop(o);
        let (i, u) = measure(|| inner_shadow_mask(&sil, 6.0, 3.0, 45.0, 1.0, &L).unwrap());
        budget("inner_shadow_mask", n, &u, 16 * n);
        drop(i);
        let np = (u64::from(side) + 2 * 5) * (u64::from(side) + 2 * 5);
        let (g, u) = measure(|| glow_mask(&sil, 6.0, 1.0, &L).unwrap());
        budget("glow_mask rad 6", n, &u, 16 * np + 12 * n);
        drop(g);
        let np = (u64::from(side) + 2 * 8) * (u64::from(side) + 2 * 8);
        let (s, u) = measure(|| soft_edge_mask(&sil, 6.0, 1.0, &L).unwrap());
        budget("soft_edge_mask rad 6", n, &u, 16 * np + 12 * n);
        drop(s);
    }
}
