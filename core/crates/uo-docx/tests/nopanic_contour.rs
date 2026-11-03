//! No-panic loops of `uo_docx::drawing` (security plan: nothing panics on document data):
//! seeded random pictures at random sizes, buffers of every nearby length, sizes at the edges of
//! `u32` (the byte count of the picture overflows `u64` arithmetic in a careless port and exceeds
//! every real buffer), hostile gallery numbers and strings. A call returns, in time proportional
//! to the pixels of the buffer it was given, and a polygon's vertices stay in `0..=21600`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::drawing::{ContourError, SHAPE_STYLE_PRESETS, find_shape_style, shape_style_preset, wrap_contour};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn check(pts: &[[f64; 2]]) {
    assert!(pts.len() >= 2);
    assert_eq!(pts.first(), pts.last());
    for p in pts {
        assert!((0.0..=21600.0).contains(&p[0]) && (0.0..=21600.0).contains(&p[1]) && p[0].fract() == 0.0 && p[1].fract() == 0.0);
    }
}

#[test]
fn random_pictures_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut some, mut none) = (0, 0);
    for _ in 0..1500 {
        let (bw, bh) = (if rng.below(4) == 0 { 700 } else { 130 }, if rng.below(4) == 0 { 700 } else { 130 });
        let w = u32::try_from(1 + rng.below(bw)).unwrap();
        let h = u32::try_from(1 + rng.below(bh)).unwrap();
        let n = usize::try_from(u64::from(w) * u64::from(h) * 4).unwrap();
        let mode = rng.below(5);
        let mut d = vec![0u8; n];
        match mode {
            0 => d.iter_mut().for_each(|b| *b = u8::try_from(rng.below(256)).unwrap()),
            1 => d.chunks_exact_mut(4).for_each(|p| p[3] = if rng.below(50) == 0 { 255 } else { 0 }),
            2 => d.chunks_exact_mut(4).for_each(|p| p[3] = if rng.below(50) == 0 { 0 } else { 7 }),
            3 => {
                let (cx, cy) = (rng.below(u64::from(w)), rng.below(u64::from(h)));
                for (i, p) in d.chunks_exact_mut(4).enumerate() {
                    let (x, y) = (u64::try_from(i).unwrap() % u64::from(w), u64::try_from(i).unwrap() / u64::from(w));
                    p[3] = u8::from(x.abs_diff(cx) + y.abs_diff(cy) < 1 + (u64::from(w) + u64::from(h)) / 8) * 255;
                }
            }
            _ => {}
        }
        let cut = match rng.below(6) {
            0 => n.saturating_sub(1),
            1 => usize::try_from(rng.below(u64::try_from(n).unwrap() + 1)).unwrap(),
            _ => n,
        };
        match wrap_contour(&d[..cut], w, h) {
            Ok(Some(pts)) => {
                some += 1;
                check(&pts);
            }
            Ok(None) => none += 1,
            Err(e) => assert!(cut < n && e == ContourError::BufferSize),
        }
    }
    assert!(some > 100 && none > 50, "{some} {none}");
}

#[test]
fn sizes_at_the_edges_of_u32() {
    for (w, h) in [
        (0, 0),
        (0, u32::MAX),
        (u32::MAX, 0),
        (1, u32::MAX),
        (u32::MAX, 1),
        (u32::MAX, u32::MAX),
        (65536, 65536),
        (1 << 31, 2),
        (2, 1 << 31),
    ] {
        for buf in [&[][..], &[0u8; 3][..], &[255u8; 16][..]] {
            let r = wrap_contour(buf, w, h);
            if w == 0 || h == 0 {
                assert_eq!(r, Ok(None));
            } else {
                assert_eq!(r, Err(ContourError::BufferSize), "{w}x{h} with {} bytes", buf.len());
            }
        }
    }
}

#[test]
fn degenerate_pictures_that_do_fit() {
    for (w, h) in [(1u32, 1u32), (1, 2), (2, 1), (1, 20_000), (20_000, 1), (96, 1), (1, 96), (97, 1), (3, 500), (500, 3)] {
        let n = usize::try_from(w * h * 4).unwrap();
        for fill in [0u8, 255] {
            let d = vec![fill; n];
            let r = wrap_contour(&d, w, h).unwrap();
            if fill == 0 {
                assert_eq!(r, None, "{w}x{h} transparent");
            } else if let Some(pts) = r {
                // fewer pixels than grid cells leave cells empty, so a uniform picture can have a contour
                check(&pts);
            }
        }
        let mut d = vec![0u8; n];
        d[3] = 255;
        if let Some(pts) = wrap_contour(&d, w, h).unwrap() {
            check(&pts);
        }
        let mut d = vec![0u8; n];
        let last = n - 1;
        d[last] = 255;
        if let Some(pts) = wrap_contour(&d, w, h).unwrap() {
            check(&pts);
        }
    }
}

#[test]
fn gallery_lookups_never_panic() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..20_000 {
        let n = f64::from_bits(rng.next());
        if let Some(p) = shape_style_preset(n) {
            assert!(SHAPE_STYLE_PRESETS.contains(p));
        }
    }
    for n in [f64::NAN, f64::MAX, f64::MIN, f64::MIN_POSITIVE, 5e-324, -5e-324, 1e15, 9_007_199_254_740_993.0, 18_446_744_073_709_551_616.0]
    {
        assert_eq!(shape_style_preset(n), None, "{n}");
    }
    assert_eq!(find_shape_style("\u{0}", "\u{10ffff}"), None);
}
