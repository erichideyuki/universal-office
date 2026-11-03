//! Behaviour of `raster.rs` and `effects.rs`: one test per rule of the TS (hand-computed
//! expectations), every row of the error table of the module record (variant and "nothing
//! allocated before the check"), and a naive second implementation of the blur (the loops of the TS
//! written out) against the restructured vertical pass. The numbers against the TS itself are in
//! `raster_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use uo_alloc::measure;
use uo_drawingml::*;
use uo_jsnum::math::{exp, sqrt};
use uo_testkit::generate::Rng;

const L: Limits = Limits::NATIVE;

fn mask(w: u32, h: u32, a: &[f32]) -> Mask {
    Mask::from_vec(w, h, a.to_vec()).unwrap()
}

fn bits(m: &Mask) -> Vec<u32> {
    m.data().iter().map(|v| v.to_bits()).collect()
}

fn f32s(v: f64) -> f32 {
    v as f32
}

fn poly(points: &[(f64, f64)]) -> Polygons {
    let mut p = Polygons::new();
    for &(x, y) in points {
        p.push_point(x, y);
    }
    p.end_poly();
    p
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygons {
    poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn path(segs: &[PathSeg]) -> Subpaths {
    let mut s = Subpaths::new();
    for g in segs {
        s.push(*g);
    }
    s.end_sub();
    s
}

fn counts(p: &Polygons) -> Vec<usize> {
    p.iter().map(<[f64]>::len).collect()
}

// ── flatten ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn flatten_keeps_polygons_of_three_points_and_drops_shorter_ones() {
    let m = Affine::IDENTITY;
    let subs = path(&[PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::L { x: 5.0, y: 0.0 }]);
    assert_eq!(flatten_path(&subs, &m, DEFAULT_TOL, &L).unwrap().len(), 0);
    let subs = path(&[PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::L { x: 5.0, y: 0.0 }, PathSeg::L { x: 5.0, y: 5.0 }]);
    assert_eq!(counts(&flatten_path(&subs, &m, DEFAULT_TOL, &L).unwrap()), [6]);
    // a new M finishes the pending polygon only when it has at least 3 points
    let subs = path(&[
        PathSeg::M { x: 0.0, y: 0.0 },
        PathSeg::L { x: 5.0, y: 0.0 },
        PathSeg::M { x: 1.0, y: 1.0 },
        PathSeg::L { x: 2.0, y: 1.0 },
        PathSeg::L { x: 2.0, y: 2.0 },
        PathSeg::M { x: 9.0, y: 9.0 },
        PathSeg::L { x: 9.0, y: 8.0 },
    ]);
    let p = flatten_path(&subs, &m, DEFAULT_TOL, &L).unwrap();
    assert_eq!(p.get(0), Some(&[1.0, 1.0, 2.0, 1.0, 2.0, 2.0][..]));
    assert_eq!(p.len(), 1);
    // empty subpaths and an empty list
    let mut s = Subpaths::new();
    s.end_sub();
    assert_eq!(flatten_path(&s, &m, DEFAULT_TOL, &L).unwrap().len(), 0);
    assert_eq!(flatten_path(&Subpaths::new(), &m, DEFAULT_TOL, &L).unwrap().len(), 0);
}

#[test]
fn flatten_applies_the_affine_and_z_returns_the_pen_to_the_start() {
    let m = Affine([2.0, 0.0, 0.0, 3.0, 10.0, 20.0]);
    let subs = path(&[PathSeg::M { x: 1.0, y: 1.0 }, PathSeg::L { x: 2.0, y: 1.0 }, PathSeg::L { x: 2.0, y: 2.0 }]);
    let p = flatten_path(&subs, &m, DEFAULT_TOL, &L).unwrap();
    assert_eq!(p.get(0), Some(&[12.0, 23.0, 14.0, 23.0, 14.0, 26.0][..]));
    // after Z the next curve starts at the subpath start: the control points are measured from it
    let subs = path(&[
        PathSeg::M { x: 0.0, y: 0.0 },
        PathSeg::L { x: 10.0, y: 0.0 },
        PathSeg::L { x: 10.0, y: 10.0 },
        PathSeg::Z,
        PathSeg::Q { x1: 5.0, y1: 5.0, x: 0.0, y: 0.0 },
    ]);
    let p = flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap();
    // len = 2 * hypot(5, 5) = 14.14, n = ceil(14.14 / 0.35 / 4) = 11
    assert_eq!(counts(&p), [2 * (3 + 11)]);
}

#[test]
fn flatten_tolerance_rules() {
    let m = Affine::IDENTITY;
    let curve = |x: f64| {
        path(&[PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::L { x: 1.0, y: 0.0 }, PathSeg::C { x1: x, y1: 0.0, x2: x, y2: 0.0, x, y: 0.0 }])
    };
    let n = |subs: &Subpaths, tol: f64| flatten_path(subs, &m, tol, &L).unwrap().get(0).map(<[f64]>::len);
    // the last point is at x = 1: len = 0 (a curve of no length)
    assert_eq!(n(&curve(1.0), DEFAULT_TOL), Some(2 * (2 + 2)));
    // tol = 0 with a curve of no length: 0 / 0 is NaN, no points, and the 2 left are too few
    assert_eq!(n(&curve(1.0), 0.0), None);
    // a length over 0 divided by 0 is +infinity: capped at 256 points
    assert_eq!(n(&curve(5.0), 0.0), Some(2 * (2 + 256)));
    // tol = -0: a length over 0 gives -infinity: 2 points; a zero length gives NaN again
    assert_eq!(n(&curve(5.0), -0.0), Some(2 * (2 + 2)));
    assert_eq!(n(&curve(1.0), -0.0), None);
    // NaN tolerance: no points
    assert_eq!(n(&curve(5.0), f64::NAN), None);
    // a singular matrix has scale 1e-9: the minimum of 2 points
    let flat = Affine([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let p = flatten_path(&curve(5.0), &flat, DEFAULT_TOL, &L).unwrap();
    assert_eq!(p.get(0).map(<[f64]>::len), Some(2 * (2 + 2)));
}

#[test]
fn flatten_arcs_normalize_the_sweep() {
    let arc = |ccw: bool| {
        path(&[
            PathSeg::M { x: 0.0, y: 0.0 },
            PathSeg::E { cx: 0.0, cy: 0.0, rx: 10.0, ry: 10.0, a0: 0.0, a1: std::f64::consts::PI / 2.0, ccw },
        ])
    };
    let count = |ccw: bool| flatten_path(&arc(ccw), &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap().get(0).map(<[f64]>::len);
    // clockwise: 90 degrees, len 15.7, n = ceil(15.7 / 0.35 / 2) = 23, i = 0..=23 (24 points) and the M
    assert_eq!(count(false), Some(2 * (1 + 24)));
    // counter-clockwise: a1 - 2 pi, 270 degrees, len 47.1, n = 68
    assert_eq!(count(true), Some(2 * (1 + 69)));
    let p = flatten_path(&arc(false), &Affine::IDENTITY, DEFAULT_TOL, &L).unwrap();
    let g = p.get(0).unwrap();
    assert_eq!((g[2], g[3]), (10.0, 0.0));
    let last = g.len() - 2;
    assert!((g[last] - 0.0).abs() < 1e-9 && (g[last + 1] - 10.0).abs() < 1e-9);
}

#[test]
fn flatten_counts_every_point_against_the_limit() {
    let subs = path(&[
        PathSeg::M { x: 0.0, y: 0.0 },
        PathSeg::L { x: 1.0, y: 0.0 },
        PathSeg::L { x: 1.0, y: 1.0 },
        PathSeg::L { x: 0.0, y: 1.0 },
        PathSeg::L { x: 0.0, y: 0.5 },
    ]);
    let at = |max: u64| flatten_path(&subs, &Affine::IDENTITY, DEFAULT_TOL, &Limits { max_path_points: max, ..L });
    assert_eq!(at(5).map(|p| p.len()), Ok(1));
    assert_eq!(at(4).map(|p| p.len()), Err(Error::Limit(LimitKind::PathPoints)));
    assert_eq!(at(0).map(|p| p.len()), Err(Error::Limit(LimitKind::PathPoints)));
    // the points of a polygon dropped later still count
    let many = path(&[PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::L { x: 1.0, y: 0.0 }]);
    assert!(flatten_path(&many, &Affine::IDENTITY, DEFAULT_TOL, &Limits { max_path_points: 1, ..L }).is_err());
    // a curve checks before each point
    let curve = path(&[PathSeg::M { x: 0.0, y: 0.0 }, PathSeg::C { x1: 50.0, y1: 0.0, x2: 50.0, y2: 50.0, x: 0.0, y: 50.0 }]);
    assert_eq!(
        flatten_path(&curve, &Affine::IDENTITY, DEFAULT_TOL, &Limits { max_path_points: 10, ..L }),
        Err(Error::Limit(LimitKind::PathPoints))
    );
}

// ── fill ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn fill_covers_whole_pixels_and_fractional_edges() {
    let mut m = Mask::new(8, 8, &L).unwrap();
    fill_polygons(&mut m, &rect(2.0, 2.0, 6.0, 6.0), false, &L).unwrap();
    for y in 0..8usize {
        for x in 0..8usize {
            let inside = (2..6).contains(&x) && (2..6).contains(&y);
            assert_eq!(m.data()[y * 8 + x], if inside { 1.0 } else { 0.0 }, "({x},{y})");
        }
    }
    // half a pixel on the left and on the top
    let mut m = Mask::new(8, 8, &L).unwrap();
    fill_polygons(&mut m, &rect(2.5, 2.5, 6.0, 6.0), false, &L).unwrap();
    assert_eq!(m.data()[2 * 8 + 2], 0.25);
    assert_eq!(m.data()[2 * 8 + 3], 0.5);
    assert_eq!(m.data()[3 * 8 + 2], 0.5);
    assert_eq!(m.data()[3 * 8 + 3], 1.0);
    // a half pixel at the right end
    let mut m = Mask::new(2, 1, &L).unwrap();
    fill_polygons(&mut m, &rect(0.0, 0.0, 1.5, 1.0), false, &L).unwrap();
    assert_eq!(m.data(), [1.0, 0.5]);
}

#[test]
fn fill_rules_differ_on_overlaps() {
    let mut both = Polygons::new();
    for p in [rect(0.0, 0.0, 6.0, 6.0), rect(2.0, 2.0, 8.0, 8.0)] {
        for q in p.iter() {
            for c in q.chunks(2) {
                both.push_point(c[0], c[1]);
            }
            both.end_poly();
        }
    }
    let mut nz = Mask::new(8, 8, &L).unwrap();
    let mut eo = Mask::new(8, 8, &L).unwrap();
    fill_polygons(&mut nz, &both, false, &L).unwrap();
    fill_polygons(&mut eo, &both, true, &L).unwrap();
    assert_eq!(nz.data()[3 * 8 + 3], 1.0);
    assert_eq!(eo.data()[3 * 8 + 3], 0.0);
    assert_eq!(eo.data()[3 * 8 + 1], 1.0);
    assert_eq!(nz.data()[7 * 8 + 7], 1.0);
    // opposite windings cancel in the non-zero rule
    let mut rev = Polygons::new();
    for c in [(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)] {
        rev.push_point(c.0, c.1);
    }
    rev.end_poly();
    for c in [(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0)] {
        rev.push_point(c.0, c.1);
    }
    rev.end_poly();
    let mut m = Mask::new(8, 8, &L).unwrap();
    fill_polygons(&mut m, &rev, false, &L).unwrap();
    assert_eq!(m.data()[3 * 8 + 3], 0.0);
    assert_eq!(m.data()[8 + 1], 1.0);
}

#[test]
fn fill_accumulates_with_a_cap_at_one() {
    let mut m = mask(2, 1, &[0.75, 1.5]);
    fill_polygons(&mut m, &rect(0.0, 0.0, 1.0, 1.0), false, &L).unwrap();
    assert_eq!(m.data(), [1.0, 1.0], "min(1, a + row): 0.75 + 1 and 1.5 + 0 both give 1");
    let mut m = mask(2, 1, &[0.25, f32::NAN]);
    fill_polygons(&mut m, &rect(0.0, 0.0, 1.0, 1.0), false, &L).unwrap();
    assert_eq!(m.data()[0], 1.0);
    assert!(m.data()[1].is_nan(), "a NaN cell stays NaN");
    // every row of r0..=r1 is merged (the row of ceil(ymax) gets 0 and is clamped), a row past it is not touched
    let mut m = mask(1, 3, &[1.5, 1.5, 1.5]);
    fill_polygons(&mut m, &rect(0.0, 0.0, 1.0, 1.0), false, &L).unwrap();
    assert_eq!(m.data(), [1.0, 1.0, 1.5]);
}

#[test]
fn fill_without_edges_or_rows_changes_nothing() {
    let start = [1.5f32, 2.0, 0.5, f32::NAN];
    let mut m = mask(2, 2, &start);
    // only horizontal edges
    fill_polygons(&mut m, &poly(&[(0.0, 1.0), (2.0, 1.0)]), false, &L).unwrap();
    assert_eq!(bits(&m), start.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    fill_polygons(&mut m, &Polygons::new(), true, &L).unwrap();
    assert_eq!(bits(&m), start.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    // a NaN y range gives no rows (ymin is NaN)
    fill_polygons(&mut m, &poly(&[(0.0, f64::NAN), (2.0, 0.0), (2.0, 2.0)]), false, &L).unwrap();
    assert_eq!(bits(&m), start.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    // an empty mask
    let mut e = Mask::new(0, 4, &L).unwrap();
    fill_polygons(&mut e, &rect(0.0, 0.0, 3.0, 3.0), false, &L).unwrap();
    let mut e = Mask::new(4, 0, &L).unwrap();
    fill_polygons(&mut e, &rect(0.0, 0.0, 3.0, 3.0), false, &L).unwrap();
}

#[test]
fn fill_work_limit_is_rows_times_edges() {
    // a square has 2 edges that are not horizontal; its rows are 2..=6 (5)
    let work = |max: u64| {
        let mut m = Mask::new(8, 8, &L).unwrap();
        let r = fill_polygons(&mut m, &rect(2.0, 2.0, 6.0, 6.0), false, &Limits { max_fill_work: max, ..L });
        (r, m.data().iter().filter(|v| **v > 0.0).count())
    };
    assert_eq!(work(10), (Ok(()), 16));
    assert_eq!(work(9), (Err(Error::Limit(LimitKind::FillWork)), 0), "the mask is not touched on an error");
    assert_eq!(work(0), (Err(Error::Limit(LimitKind::FillWork)), 0));
    // no rows: no work, whatever the limit
    let mut m = Mask::new(8, 8, &L).unwrap();
    assert_eq!(fill_polygons(&mut m, &rect(2.0, 20.0, 6.0, 30.0), false, &Limits { max_fill_work: 0, ..L }), Ok(()));
    let mut m = Mask::new(8, 8, &L).unwrap();
    let polys = rect(2.0, 2.0, 6.0, 6.0);
    let (r, usage) = measure(|| fill_polygons(&mut m, &polys, false, &Limits { max_fill_work: 1, ..L }));
    assert_eq!(r, Err(Error::Limit(LimitKind::FillWork)));
    assert_eq!(usage.allocated, 0, "the check comes before the first allocation");
}

// ── blur ────────────────────────────────────────────────────────────────────────────────────────

fn naive_blur(m: &Mask, sigma: f64) -> Vec<f32> {
    let r = ((sigma * 3.0).ceil() as i64).max(1);
    let mut k = vec![0.0f32; (2 * r + 1) as usize];
    let mut sum = 0.0;
    for i in -r..=r {
        let v = exp(-((i * i) as f64) / (2.0 * sigma * sigma));
        k[(i + r) as usize] = v as f32;
        sum += v;
    }
    for kv in &mut k {
        *kv = (f64::from(*kv) / sum) as f32;
    }
    let (w, h) = (i64::from(m.w()), i64::from(m.h()));
    let a = m.data();
    let mut tmp = vec![0.0f32; a.len()];
    let mut out = vec![0.0f32; a.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for i in (-r).max(-x)..=r.min(w - 1 - x) {
                s += f64::from(a[(y * w + x + i) as usize]) * f64::from(k[(i + r) as usize]);
            }
            tmp[(y * w + x) as usize] = s as f32;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for i in (-r).max(-y)..=r.min(h - 1 - y) {
                s += f64::from(tmp[((y + i) * w + x) as usize]) * f64::from(k[(i + r) as usize]);
            }
            out[(y * w + x) as usize] = s as f32;
        }
    }
    out
}

#[test]
fn blur_equals_the_loops_of_the_ts_written_out() {
    let mut rng = Rng::new(77);
    for &(w, h) in &[(1u32, 1u32), (2, 7), (9, 3), (17, 13), (31, 31), (5, 40)] {
        let a: Vec<f32> = (0..w * h).map(|_| f32s(rng.below(9) as f64 / 8.0)).collect();
        let m = mask(w, h, &a);
        for sigma in [0.05, 0.3, 0.7, 1.0, 2.5, 4.4, 12.0] {
            let got = gaussian_blur(&m, sigma, &L).unwrap();
            let want: Vec<u32> = naive_blur(&m, sigma).iter().map(|v| v.to_bits()).collect();
            assert_eq!(bits(&got), want, "{w}x{h} sigma {sigma}");
        }
    }
}

#[test]
fn blur_small_sigma_is_a_copy_and_nan_is_zeros() {
    let m = mask(3, 2, &[0.1, 0.2, 0.3, 0.4, f32::NAN, -0.0]);
    for sigma in [0.0, 0.04, 0.0499, -1.0, -0.0, f64::NEG_INFINITY] {
        assert_eq!(bits(&gaussian_blur(&m, sigma, &L).unwrap()), bits(&m), "sigma {sigma}");
    }
    let z = gaussian_blur(&m, f64::NAN, &L).unwrap();
    assert_eq!((z.w(), z.h()), (3, 2));
    assert!(z.data().iter().all(|v| v.to_bits() == 0), "all +0");
    // 0.05 is not below the threshold: it blurs (radius 1)
    let one = mask(3, 1, &[0.0, 1.0, 0.0]);
    let b = gaussian_blur(&one, 0.05, &L).unwrap();
    assert_eq!(b.data()[1], 1.0, "the kernel is concentrated at the centre");
}

#[test]
fn blur_kernel_is_not_renormalized_at_the_edges() {
    let m = mask(5, 1, &[1.0; 5]);
    let b = gaussian_blur(&m, 1.0, &L).unwrap();
    let centre = f64::from(b.data()[2]);
    let edge = f64::from(b.data()[0]);
    // taps k0 + k1 + k2 + k3 at the edge against k0 + 2 k1 + 2 k2 in the middle (kernel of radius 3)
    assert!(edge < centre * 0.75 && edge > centre * 0.65, "edge {edge} centre {centre}: zero padding loses the outside part");
    assert_eq!(b.data()[0], b.data()[4]);
}

#[test]
fn blur_kernel_radius_limit() {
    let m = mask(4, 4, &[0.5; 16]);
    let lim = Limits { max_kernel_radius: 10, ..L };
    assert!(gaussian_blur(&m, 3.3, &lim).is_ok(), "ceil(9.9) = 10");
    assert_eq!(gaussian_blur(&m, 3.4, &lim), Err(Error::Limit(LimitKind::KernelRadius)));
    assert_eq!(gaussian_blur(&m, f64::INFINITY, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    assert_eq!(gaussian_blur(&m, 1e300, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    // the check holds for an empty mask too (the kernel is built first in the TS)
    let e = Mask::new(0, 0, &L).unwrap();
    assert_eq!(gaussian_blur(&e, f64::INFINITY, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    assert_eq!(gaussian_blur(&e, 2.0, &L).map(|m| (m.w(), m.h())), Ok((0, 0)));
}

// ── distance ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn distance_to_a_pixel_is_euclidean() {
    let mut a = vec![0.0f32; 25];
    a[2 * 5 + 2] = 1.0;
    let m = mask(5, 5, &a);
    let d = distance_to(&m, true);
    assert_eq!(d[2 * 5 + 2], 0.0);
    assert_eq!(d[0], f32s(sqrt(8.0)));
    assert_eq!(d[2 * 5 + 4], 2.0);
    assert_eq!(d[5 + 2], 1.0);
    // to the complement: the pixel itself is 1 away from the nearest background, the rest 0
    let o = distance_to(&m, false);
    assert_eq!(o[2 * 5 + 2], 1.0);
    assert_eq!(o.iter().filter(|v| **v == 0.0).count(), 24);
}

#[test]
fn distance_threshold_and_infinity() {
    let m = mask(3, 1, &[0.5, 0.49999997, f32::NAN]);
    // 0.5 is inside, below and NaN are not
    let d = distance_to(&m, true);
    assert_eq!(d[0], 0.0);
    assert_eq!(d[1], 1.0);
    assert_eq!(d[2], 2.0);
    // nothing inside: every distance is sqrt(1e20) = 1e10
    let none = distance_to(&mask(2, 2, &[0.0; 4]), true);
    assert!(none.iter().all(|v| *v == f32s(1e10)));
    assert!(distance_to(&Mask::new(0, 5, &L).unwrap(), true).is_empty());
    assert!(distance_to(&Mask::new(5, 0, &L).unwrap(), false).is_empty());
}

// ── morph ───────────────────────────────────────────────────────────────────────────────────────

#[test]
fn morph_dilates_and_erodes_with_a_half_unit_transition() {
    let mut a = vec![0.0f32; 49];
    a[3 * 7 + 3] = 1.0;
    let m = mask(7, 7, &a);
    let d = morph(&m, 1.0, &L).unwrap();
    assert_eq!(d.data()[3 * 7 + 3], 1.0);
    assert_eq!(d.data()[3 * 7 + 4], 0.5, "r - dd + 0.5 = 1 - 1 + 0.5");
    assert_eq!(d.data()[4 * 7 + 4], f32s(1.0 - f64::from(f32s(sqrt(2.0))) + 0.5), "the distance is stored as f32 first");
    assert_eq!(d.data()[3 * 7 + 5], 0.0);
    // erosion measures from the background, and the frame outside the mask is background
    let full = mask(4, 4, &[1.0; 16]);
    let e = morph(&full, -1.0, &L).unwrap();
    assert_eq!(e.data()[0], 0.5, "at the border: dd + r + 0.5 = 1 - 1 + 0.5");
    assert_eq!(e.data()[5], 1.0, "two away from the background: 2 - 1 + 0.5 is capped at 1");
    // never above the source for an erosion, never below for a dilation
    let g = mask(3, 1, &[0.25, 1.0, 0.0]);
    let d = morph(&g, 2.0, &L).unwrap();
    assert!(d.data().iter().zip(g.data()).all(|(a, b)| a >= b));
    let e = morph(&g, -2.0, &L).unwrap();
    assert!(e.data().iter().zip(g.data()).all(|(a, b)| a <= b));
}

#[test]
fn morph_tiny_radius_is_a_copy() {
    let m = mask(2, 2, &[0.1, 0.2, f32::NAN, 0.4]);
    for r in [0.0, 0.0099, -0.0099, 0.005, -0.0] {
        assert_eq!(bits(&morph(&m, r, &L).unwrap()), bits(&m), "r {r}");
    }
    // 0.01 is not below the threshold: a set pixel of coverage 0.5 grows to r + 0.5
    assert_eq!(morph(&mask(1, 1, &[0.5]), 0.01, &L).unwrap().data(), [f32s(0.51)]);
}

#[test]
fn morph_error_table() {
    let m = mask(3, 2, &[0.5; 6]);
    // NaN radius: RangeError at the copy into the padded plane, unless there is nothing to copy
    assert_eq!(morph(&m, f64::NAN, &L), Err(Error::InvalidArgument));
    for (w, h) in [(0, 0), (0, 5), (5, 0)] {
        let e = Mask::new(w, h, &L).unwrap();
        let r = morph(&e, f64::NAN, &L).unwrap();
        assert_eq!((r.w(), r.h(), r.data().len()), (w, h, 0));
    }
    // infinite radius: RangeError at the allocation of the padded plane, empty mask included
    for r in [f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(morph(&m, r, &L), Err(Error::Limit(LimitKind::MaskPixels)));
        assert_eq!(morph(&Mask::new(0, 0, &L).unwrap(), r, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    }
    // the padded plane (w + 2p) x (h + 2p), p = ceil(|r|) + 2, against max_mask_pixels
    let ten = Mask::new(10, 10, &L).unwrap();
    let lim = |n: u64| Limits { max_mask_pixels: n, ..L };
    assert!(morph(&ten, 3.0, &lim(400)).is_ok(), "20 x 20");
    assert_eq!(morph(&ten, 3.0, &lim(399)), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(morph(&ten, 2.5, &lim(399)), Err(Error::Limit(LimitKind::MaskPixels)), "ceil(2.5) = 3");
    assert_eq!(morph(&ten, 1e300, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(morph(&ten, -1e300, &L), Err(Error::Limit(LimitKind::MaskPixels)));
}

// ── shift and warp ──────────────────────────────────────────────────────────────────────────────

#[test]
fn shift_moves_and_interpolates() {
    let m = mask(4, 1, &[0.0, 1.0, 0.0, 0.0]);
    assert_eq!(shift_mask(&m, 1.0, 0.0, None, &L).unwrap().data(), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(shift_mask(&m, -1.0, 0.0, None, &L).unwrap().data(), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(shift_mask(&m, 0.5, 0.0, None, &L).unwrap().data(), [0.0, 0.5, 0.5, 0.0]);
    // vertical: out of range is 0
    assert_eq!(shift_mask(&m, 0.0, 1.0, None, &L).unwrap().data(), [0.0; 4]);
    // another size
    let big = shift_mask(&m, 1.0, 1.0, Some((6, 3)), &L).unwrap();
    assert_eq!((big.w(), big.h()), (6, 3));
    assert_eq!(big.data()[6 + 2], 1.0);
    assert_eq!(big.data().iter().filter(|v| **v != 0.0).count(), 1);
    assert_eq!(shift_mask(&m, 1.0, 1.0, Some((0, 3)), &L).map(|m| m.data().len()), Ok(0));
}

#[test]
fn shift_and_warp_read_nan_where_the_index_is_nan() {
    let m = mask(3, 3, &[0.5; 9]);
    for (dx, dy) in [(f64::NAN, 0.0), (0.0, f64::NAN), (f64::NAN, f64::NAN)] {
        let s = shift_mask(&m, dx, dy, None, &L).unwrap();
        assert!(s.data().iter().all(|v| v.is_nan()), "{dx} {dy}");
    }
    // infinities fall outside: 0, but the interpolation weights are NaN (inf - inf)
    let s = shift_mask(&m, f64::INFINITY, 0.0, None, &L).unwrap();
    assert!(s.data().iter().all(|v| v.is_nan()));
    let w = warp_mask(&m, &Affine([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]), 3, 3, &L).unwrap();
    assert!(w.data().iter().all(|v| v.is_nan()));
}

#[test]
fn warp_identity_translation_and_scale() {
    let a: Vec<f32> = (0..12).map(|i| f32s(f64::from(i + 1) / 12.0)).collect();
    let m = mask(4, 3, &a);
    assert_eq!(bits(&warp_mask(&m, &Affine::IDENTITY, 4, 3, &L).unwrap()), bits(&m));
    let t = warp_mask(&m, &Affine([1.0, 0.0, 0.0, 1.0, -1.0, 0.0]), 4, 3, &L).unwrap();
    assert_eq!(t.data()[0], 0.0);
    assert_eq!(t.data()[1], m.data()[0]);
    let s = warp_mask(&m, &Affine([0.5, 0.0, 0.0, 0.5, 0.0, 0.0]), 8, 6, &L).unwrap();
    assert_eq!((s.w(), s.h()), (8, 6));
    assert_eq!(s.data()[0], f32s(f64::from(m.data()[0]) * 0.75 * 0.75), "bilinear at the corner of the source (weights .75 and .75)");
    // a singular inverse
    let w = warp_mask(&m, &Affine([0.0, 0.0, 0.0, 0.0, 1.0, 1.0]), 4, 3, &L).unwrap();
    assert!(w.data().iter().all(|v| !v.is_nan()));
}

#[test]
fn shift_and_warp_limits() {
    let m = mask(2, 2, &[1.0; 4]);
    let lim = Limits { max_mask_pixels: 100, ..L };
    assert!(shift_mask(&m, 0.0, 0.0, Some((10, 10)), &lim).is_ok());
    assert_eq!(shift_mask(&m, 0.0, 0.0, Some((10, 11)), &lim), Err(Error::Limit(LimitKind::MaskPixels)));
    assert!(warp_mask(&m, &Affine::IDENTITY, 10, 10, &lim).is_ok());
    assert_eq!(warp_mask(&m, &Affine::IDENTITY, 11, 10, &lim), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(warp_mask(&m, &Affine::IDENTITY, u32::MAX, u32::MAX, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    for (r, usage) in [
        measure(|| shift_mask(&m, 0.0, 0.0, Some((10, 11)), &lim)),
        measure(|| warp_mask(&m, &Affine::IDENTITY, 11, 10, &lim)),
        measure(|| gaussian_blur(&m, f64::INFINITY, &L)),
        measure(|| morph(&m, f64::NAN, &L)),
        measure(|| morph(&m, f64::INFINITY, &L)),
        measure(|| morph(&m, 1e6, &lim)),
    ] {
        assert!(r.is_err());
        assert_eq!(usage.allocated, 0, "the check comes before the first allocation");
    }
}

// ── effects ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn align_attr_and_anchor_points() {
    let table = [
        (None, Align::B, (50.0, 100.0)),
        (Some("tl"), Align::Tl, (0.0, 0.0)),
        (Some("t"), Align::T, (50.0, 0.0)),
        (Some("tr"), Align::Tr, (100.0, 0.0)),
        (Some("l"), Align::L, (0.0, 50.0)),
        (Some("ctr"), Align::Ctr, (50.0, 50.0)),
        (Some("r"), Align::R, (100.0, 50.0)),
        (Some("bl"), Align::Bl, (0.0, 100.0)),
        (Some("b"), Align::B, (50.0, 100.0)),
        (Some("br"), Align::Br, (100.0, 100.0)),
        (Some(""), Align::Other, (50.0, 50.0)),
        (Some("TL"), Align::Other, (50.0, 50.0)),
        (Some("xx"), Align::Other, (50.0, 50.0)),
    ];
    for (attr, algn, at) in table {
        assert_eq!(Align::from_attr(attr), algn, "{attr:?}");
        assert_eq!(align_point(algn, 0.0, 0.0, 100.0, 100.0), at, "{attr:?}");
    }
    assert_eq!(align_point(Align::Br, 10.0, 20.0, 5.0, 7.0), (15.0, 27.0));
}

fn spec(blur_pt: f64, dist_pt: f64, dir_deg: f64) -> ShadowSpec {
    ShadowSpec { blur_pt, dist_pt, dir_deg, sx: None, sy: None, kx_deg: None, ky_deg: None, algn: Align::B }
}

const BX: BoxF = BoxF { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };

#[test]
fn shadow_affine_composes_skew_scale_and_offset_about_the_anchor() {
    assert_eq!(shadow_affine(&spec(0.0, 0.0, 0.0), &BX, 1.0).0.map(f64::to_bits), Affine::IDENTITY.0.map(f64::to_bits));
    let a = shadow_affine(&spec(0.0, 2.0, 0.0), &BX, 1.5).0;
    assert_eq!(a, [1.0, 0.0, 0.0, 1.0, 3.0, 0.0]);
    // scale 2 about the bottom centre (5, 10): p' = A + S (p - A)
    let s = ShadowSpec { sx: Some(2.0), sy: Some(2.0), ..spec(0.0, 0.0, 0.0) };
    let a = Affine(shadow_affine(&s, &BX, 1.0).0);
    assert_eq!(a.apply(5.0, 10.0), (5.0, 10.0));
    assert_eq!(a.apply(6.0, 9.0), (7.0, 8.0));
    // absent means 1 and 0, and 0 stays 0 (not replaced)
    let z = ShadowSpec { sx: Some(0.0), ..spec(0.0, 0.0, 0.0) };
    assert_eq!(shadow_affine(&z, &BX, 1.0).0[0], 0.0);
    // skew uses tan of the angle in degrees as (deg * PI) / 180
    let k = ShadowSpec { kx_deg: Some(45.0), ..spec(0.0, 0.0, 0.0) };
    let tan45 = uo_jsnum::math::tan((45.0 * std::f64::consts::PI) / 180.0);
    assert_eq!(shadow_affine(&k, &BX, 1.0).0[2], tan45);
}

#[test]
fn outer_shadow_is_the_silhouette_moved_in_the_direction() {
    let mut a = vec![0.0f32; 9 * 9];
    a[4 * 9 + 2] = 1.0;
    let sil = mask(9, 9, &a);
    let m = outer_shadow_mask(&sil, &BX, &spec(0.0, 3.0, 0.0), 1.0, &L).unwrap();
    assert_eq!(m.data()[4 * 9 + 5], 1.0, "3 px to the east (dir 0)");
    assert_eq!(m.data().iter().filter(|v| **v != 0.0).count(), 1);
    let s = outer_shadow_mask(&sil, &BX, &spec(0.0, 3.0, 90.0), 1.0, &L).unwrap();
    assert_eq!(s.data()[7 * 9 + 2], 1.0, "dir 90 is south");
    // no offset and no blur: the same mask
    let same = outer_shadow_mask(&sil, &BX, &spec(0.0, 0.0, 0.0), 1.0, &L).unwrap();
    assert_eq!(bits(&same), bits(&sil));
    // blurred: the energy spreads and the size is the size of the silhouette
    let blurred = outer_shadow_mask(&sil, &BX, &spec(6.0, 0.0, 0.0), 1.0, &L).unwrap();
    assert_eq!((blurred.w(), blurred.h()), (9, 9));
    assert!(blurred.data()[4 * 9 + 2] < 1.0 && blurred.data()[4 * 9 + 3] > 0.0);
}

#[test]
fn inner_shadow_shades_the_side_the_direction_points_to() {
    let sil = mask(4, 1, &[0.0, 1.0, 1.0, 0.0]);
    let east = inner_shadow_mask(&sil, 0.0, 1.0, 0.0, 1.0, &L).unwrap();
    assert_eq!(east.data(), [0.0, 0.0, 1.0, 0.0], "sil x (1 - sil shifted against the direction)");
    let west = inner_shadow_mask(&sil, 0.0, 1.0, 180.0, 1.0, &L).unwrap();
    assert_eq!(west.data()[1], 1.0);
    assert_eq!(west.data()[2], 0.0);
    // nothing moved and not blurred: the silhouette minus itself
    let none = inner_shadow_mask(&sil, 0.0, 0.0, 0.0, 1.0, &L).unwrap();
    assert_eq!(none.data(), [0.0; 4]);
}

#[test]
fn glow_and_soft_edge_are_the_compositions_of_the_primitives() {
    let mut a = vec![0.0f32; 21 * 21];
    for y in 8..13 {
        for x in 8..13 {
            a[y * 21 + x] = 1.0;
        }
    }
    let sil = mask(21, 21, &a);
    let g = glow_mask(&sil, 6.0, 1.5, &L).unwrap();
    let want = gaussian_blur(&morph(&sil, 4.5, &L).unwrap(), 1.5, &L).unwrap();
    assert_eq!(bits(&g), bits(&want));
    assert!(g.data()[10 * 21 + 5] > 0.0, "the glow reaches beyond the shape");
    let s = soft_edge_mask(&sil, 3.0, 1.0, &L).unwrap();
    let eroded = gaussian_blur(&morph(&sil, -0.86 * 3.0 * 1.0, &L).unwrap(), 1.0, &L).unwrap();
    for ((v, e), o) in s.data().iter().zip(eroded.data()).zip(sil.data()) {
        assert_eq!(*v, if e < o { *e } else { *o });
    }
    assert!(s.data().iter().zip(sil.data()).all(|(v, o)| v <= o), "soft edges never exceed the silhouette");
}

#[test]
fn effect_masks_pass_the_errors_of_their_primitives_on() {
    let sil = mask(4, 4, &[1.0; 16]);
    assert_eq!(glow_mask(&sil, f64::NAN, 1.0, &L), Err(Error::InvalidArgument));
    assert_eq!(glow_mask(&sil, f64::INFINITY, 1.0, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(soft_edge_mask(&sil, f64::NAN, 1.0, &L), Err(Error::InvalidArgument));
    assert_eq!(soft_edge_mask(&sil, f64::INFINITY, 1.0, &L), Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(outer_shadow_mask(&sil, &BX, &spec(1e300, 0.0, 0.0), 1.0, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    assert_eq!(inner_shadow_mask(&sil, f64::INFINITY, 0.0, 0.0, 1.0, &L), Err(Error::Limit(LimitKind::KernelRadius)));
    // glow on a small plane: the padded plane is over the limit
    assert_eq!(glow_mask(&sil, 4.0, 1.0, &Limits { max_mask_pixels: 16, ..L }), Err(Error::Limit(LimitKind::MaskPixels)));
    // an empty silhouette is fine, a NaN radius on it too
    let e = Mask::new(0, 0, &L).unwrap();
    assert_eq!(glow_mask(&e, f64::NAN, 1.0, &L).map(|m| m.data().len()), Ok(0));
}

#[test]
fn reflection_alpha_branches() {
    let r = Reflection { st_a: 0.5, st_pos: 0.2, end_a: 0.0, end_pos: 0.8 };
    assert_eq!(reflection_alpha(0.1, &r), 0.5, "before stPos");
    assert_eq!(reflection_alpha(0.2, &r), 0.5);
    assert_eq!(reflection_alpha(0.5, &r), 0.5 + ((0.0 - 0.5) * (0.5 - 0.2)) / (0.8 - 0.2));
    assert_eq!(reflection_alpha(0.8, &r), 0.5 + ((0.0 - 0.5) * (0.8 - 0.2)) / (0.8 - 0.2));
    assert_eq!(reflection_alpha(0.9, &r), 0.0, "past endPos");
    // a degenerate range: stA up to endPos, 0 after
    let d = Reflection { st_a: 0.7, st_pos: 0.5, end_a: 0.1, end_pos: 0.5 };
    assert_eq!(reflection_alpha(0.5, &d), 0.7);
    assert_eq!(reflection_alpha(0.51, &d), 0.0);
    // NaN: every comparison is false, so the interpolation runs and is NaN
    assert!(reflection_alpha(f64::NAN, &r).is_nan());
    assert!(reflection_alpha(0.5, &Reflection { st_pos: f64::NAN, ..r }).is_nan());
}
