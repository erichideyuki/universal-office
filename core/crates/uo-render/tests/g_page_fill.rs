//! R-P3 parity of `paint::page_fill` (frontier G): `fillParam`, `gradientTilePx`,
//! `gradientTileRgba`, `patternRgba` and `pageFillTileTw` against
//! `tests/captures/g_page_fill.capture` (the frozen TS, with the pow reference environment).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use g_support::*;
use uo_render::SymbolTable;
use uo_render::paint::PaintLimits;
use uo_render::paint::page_fill as pf;
use uo_render::types::{GradStop, PageFill, PageFillKind};
use uo_testkit::Replay;
use uo_testkit::generate::Rng;

const CAPTURE: &str = include_str!("captures/g_page_fill.capture");
const L: PaintLimits = PaintLimits::DEFAULT;

const ANGLES: [f64; 23] = [
    0.0, 45.0, 90.0, 135.0, 180.0, -45.0, -90.0, -135.0, 225.0, 270.0, 315.0, 30.0, 60.0, 10.5, 360.0, 720.0, -360.0, NAN, 1e9, 359.7,
    44.6, 134.4, -44.6,
];
const FOCI: [f64; 12] = [0.0, 100.0, -100.0, 50.0, -50.0, 75.0, -75.0, 25.0, NAN, 74.9, 0.5, -0.4];
const UNIT: [f64; 8] = [0.0, 0.25, 0.5, 1.0, 0.9, -0.2, NAN, 0.1];

fn gen_page_fill(r: &mut Rng, syms: &mut SymbolTable) -> PageFill {
    let kind =
        pick(r, &[PageFillKind::Gradient, PageFillKind::GradientRadial, PageFillKind::Pattern, PageFillKind::Frame, PageFillKind::Tile]);
    let c1 = pick(r, &COLORS);
    let c2 = pick(r, &COLORS);
    let angle = pick(r, &ANGLES);
    let focus = pick(r, &FOCI);
    let mut fl = PageFill {
        kind,
        c1: syms.intern_str(c1).unwrap(),
        c2: syms.intern_str(c2).unwrap(),
        stops: None,
        angle,
        focus,
        focus_position: None,
        gradient_center: false,
        image: None,
        image_px: None,
        pattern: None,
    };
    if r.below(3) == 0 {
        let n = r.below(4);
        let mut stops = Vec::new();
        for _ in 0..n {
            let pos = pick(r, &POS);
            let color = pick(r, &COLORS);
            stops.push(GradStop { pos, color: syms.intern_str(color).unwrap() });
        }
        fl.stops = Some(stops);
    }
    if r.below(2) == 0 {
        fl.focus_position = Some([pick(r, &UNIT), pick(r, &UNIT)]);
    }
    if r.below(2) == 0 {
        fl.gradient_center = true;
    }
    if r.below(2) == 0 {
        let n = r.below(10);
        let pool = [0.0, 255.0, 170.0, 85.0, 1.0, 128.0, -1.0, 256.0, 0.5, NAN, 1e10];
        fl.pattern = Some((0..n).map(|_| pick(r, &pool)).collect());
    }
    if r.below(2) == 0 {
        let w = pick(r, &[0.0, 1.0, 64.0, 100.0, 640.0, -1.0, NAN]);
        let h = pick(r, &[0.0, 1.0, 64.0, 100.0, 480.0, -1.0, NAN]);
        let dpi = pick(r, &[0.0, 96.0, 300.0, 72.0, -5.0, NAN, 150.5]);
        fl.image_px = Some([w, h, dpi]);
    }
    fl
}

#[test]
fn page_fill_parity() {
    let cap = capture(CAPTURE);
    let mut rep = Replay::new(&cap);

    {
        let mut r = Rng::new(10);
        let got = dig(|s| {
            for _ in 0..600 {
                let mut syms = SymbolTable::new();
                let fl = gen_page_fill(&mut r, &mut syms);
                for _ in 0..21 {
                    let x = pick(&mut r, &UNIT);
                    let y = pick(&mut r, &UNIT);
                    f(s, pf::fill_param(&fl, x, y, false));
                    f(s, pf::fill_param(&fl, x, y, true));
                }
            }
        });
        rep.check("param", "v", &got);
    }
    {
        let mut r = Rng::new(20);
        let got = dig(|s| {
            for _ in 0..300 {
                let mut syms = SymbolTable::new();
                let fl = gen_page_fill(&mut r, &mut syms);
                for pdf in [false, true] {
                    let t = pf::gradient_tile_px(&fl, pdf);
                    f(s, t.0);
                    f(s, t.1);
                }
            }
        });
        rep.check("tile_px", "v", &got);
    }
    {
        let mut r = Rng::new(30);
        let dims: [(usize, usize); 9] = [(1, 1), (2, 3), (17, 9), (33, 1), (1, 40), (64, 48), (0, 5), (5, 0), (100, 80)];
        let got = dig(|s| {
            for _ in 0..160 {
                let mut syms = SymbolTable::new();
                let fl = gen_page_fill(&mut r, &mut syms);
                let (w, h) = pick(&mut r, &dims);
                s.bytes(&pf::gradient_tile_rgba(&fl, &syms, w, h, false, &L).unwrap());
                s.bytes(&pf::gradient_tile_rgba(&fl, &syms, w, h, true, &L).unwrap());
            }
        });
        rep.check("tile_rgba", "v", &got);
    }
    {
        let mut r = Rng::new(40);
        let got = dig(|s| {
            for _ in 0..400 {
                let mut syms = SymbolTable::new();
                let fl = gen_page_fill(&mut r, &mut syms);
                s.bytes(&pf::pattern_rgba(&fl, &syms));
            }
        });
        rep.check("pattern_rgba", "v", &got);
    }
    {
        let mut r = Rng::new(50);
        let dev = [2.4, 15.0, 10.0, 20.0, 0.0, NAN, 7.5];
        let got = dig(|s| {
            for _ in 0..600 {
                let mut syms = SymbolTable::new();
                let fl = gen_page_fill(&mut r, &mut syms);
                let a = pick(&mut r, &dev);
                let b = pick(&mut r, &dev);
                let pdf = boolean(&mut r);
                match pf::page_fill_tile_tw(&fl, a, b, pdf) {
                    None => {
                        s.tag(b'N');
                    }
                    Some((w, h)) => {
                        s.tag(b'T');
                        f(s, w);
                        f(s, h);
                    }
                }
            }
        });
        rep.check("tile_tw", "v", &got);
    }

    let report = rep.finish();
    eprintln!("g_page_fill: {}", report.summary());
    report.assert_clean();
}
