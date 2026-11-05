//! R-P3 parity of `paint::picture` (frontier G, RQ10): the effect bounds of both painters, the
//! shape subpaths (the two TS copies are proved equal by the capture script), the coloured mask of
//! both painters and the dash table, against `tests/captures/g_picture.capture`. The TS functions
//! are private to `canvas-picture.ts` / `pdf-picture.ts`: the script runs their source text.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use g_support::*;
use uo_drawingml::{Mask, PathSeg, Subpaths};
use uo_render::paint::picture::{self as pic, Bounds, ChannelStore, Painter};
use uo_render::types::{PicGlow, PicLine, PicReflection, PicShadow, PictureLook, PresetSpec, ShadowKind};
use uo_render::{OptF64, SymbolTable};
use uo_testkit::generate::Rng;
use uo_testkit::{Replay, Stream};

const CAPTURE: &str = include_str!("captures/g_picture.capture");

fn gen_look(r: &mut Rng, syms: &mut SymbolTable) -> PictureLook {
    let color = syms.intern_str("000000").unwrap();
    let mut look = PictureLook::default();
    if r.below(2) == 0 {
        look.line = Some(PicLine { color, alpha: 1.0, width_pt: num(r), dash: None, cmpd: None, cap: None, join: None });
    }
    if r.below(2) == 0 {
        look.glow = Some(PicGlow { color, alpha: 1.0, rad_pt: num(r) });
    }
    if r.below(3) > 0 {
        let kind = pick(r, &[ShadowKind::Outer, ShadowKind::Outer, ShadowKind::Inner]);
        let (blur_pt, dist_pt, dir_deg) = (num(r), num(r), num(r));
        let mut sh = PicShadow {
            kind,
            color,
            alpha: 1.0,
            blur_pt,
            dist_pt,
            dir_deg,
            sx: OptF64::NONE,
            sy: OptF64::NONE,
            kx_deg: OptF64::NONE,
            ky_deg: OptF64::NONE,
            algn: None,
            rot_with_shape: false,
        };
        if boolean(r) {
            sh.sx = OptF64::some(num(r));
        }
        if boolean(r) {
            sh.sy = OptF64::some(if boolean(r) { -num_bad(r, 0).abs() } else { num(r) });
        }
        if boolean(r) {
            sh.kx_deg = OptF64::some(num(r));
        }
        if boolean(r) {
            sh.ky_deg = OptF64::some(num(r));
        }
        look.shadow = Some(sh);
    }
    if r.below(2) == 0 {
        let (blur_pt, dist_pt, dir_deg) = (num(r), num(r), num(r));
        look.reflection = Some(PicReflection {
            blur_pt,
            st_a: 0.0,
            st_pos: 0.0,
            end_a: 0.0,
            end_pos: 0.0,
            dist_pt,
            dir_deg,
            sy: 1.0,
            algn: None,
            rot_with_shape: false,
        });
    }
    look
}

fn bounds_out(s: &mut Stream, b: Bounds) {
    for v in [b.x0, b.y0, b.x1, b.y1] {
        f(s, v);
    }
}

fn subs_out(s: &mut Stream, subs: &Subpaths) {
    s.u32(u32n(subs.len()));
    for sub in subs.iter() {
        s.u32(u32n(sub.len()));
        for g in sub {
            match *g {
                PathSeg::M { x, y } => {
                    s.tag(b'M');
                    f(s, x);
                    f(s, y);
                }
                PathSeg::L { x, y } => {
                    s.tag(b'L');
                    f(s, x);
                    f(s, y);
                }
                PathSeg::C { x1, y1, x2, y2, x, y } => {
                    s.tag(b'C');
                    for v in [x1, y1, x2, y2, x, y] {
                        f(s, v);
                    }
                }
                PathSeg::Q { x1, y1, x, y } => {
                    s.tag(b'Q');
                    for v in [x1, y1, x, y] {
                        f(s, v);
                    }
                }
                PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
                    s.tag(b'E');
                    for v in [cx, cy, rx, ry, a0, a1] {
                        f(s, v);
                    }
                    s.bool(ccw);
                }
                PathSeg::Z => {
                    s.tag(b'Z');
                }
            }
        }
    }
}

const SHAPES: [Option<&str>; 17] = [
    Some("rect"),
    Some("ellipse"),
    Some("star5"),
    Some("heart"),
    Some("roundRect"),
    Some("rightArrow"),
    Some("cloud"),
    Some("nope"),
    Some(""),
    None,
    Some("wedgeRectCallout"),
    Some("arc"),
    Some("pie"),
    Some("chord"),
    Some("blockArc"),
    Some("triangle"),
    Some("flowChartDecision"),
];

#[test]
fn picture_parity() {
    let cap = capture(CAPTURE);
    let mut rep = Replay::new(&cap);

    {
        let mut r = Rng::new(120);
        let got = dig(|s| {
            for _ in 0..3000 {
                let mut syms = SymbolTable::new();
                let look = if r.below(10) == 0 { None } else { Some(gen_look(&mut r, &mut syms)) };
                let (w, h) = (num(&mut r), num(&mut r));
                bounds_out(s, pic::effect_bounds(Painter::Canvas, look.as_ref(), w, h));
            }
        });
        rep.check("bounds.canvas", "v", &got);
    }
    {
        let mut r = Rng::new(120);
        let got = dig(|s| {
            for _ in 0..3000 {
                let mut syms = SymbolTable::new();
                let look = if r.below(10) == 0 { PictureLook::default() } else { gen_look(&mut r, &mut syms) };
                let (w, h) = (num(&mut r), num(&mut r));
                bounds_out(s, pic::effect_bounds(Painter::Pdf, Some(&look), w, h));
            }
        });
        rep.check("bounds.pdf", "v", &got);
    }
    {
        let mut r = Rng::new(130);
        let got = dig(|s| {
            for _ in 0..800 {
                let prst = pick(&mut r, &SHAPES);
                let adj = if r.below(3) == 0 {
                    let a1 = pick(&mut r, &[0.0, 25000.0, 50000.0, 100_000.0, NAN]);
                    let a2 = pick(&mut r, &[0.0, 25000.0, 50000.0, 16667.0]);
                    Some((a1, a2))
                } else {
                    None
                };
                let mut syms = SymbolTable::new();
                let geom = if r.below(10) == 0 {
                    None
                } else {
                    Some(PresetSpec {
                        prst: syms.intern_str(prst.unwrap_or("")).unwrap(),
                        adj: adj.map(|(a1, a2)| vec![(syms.intern_str("adj1").unwrap(), a1), (syms.intern_str("adj2").unwrap(), a2)]),
                    })
                };
                let dims = [10.0, 100.0, 150.5, 0.5, 0.0, 720.0];
                let (w, h) = (pick(&mut r, &dims), pick(&mut r, &dims));
                let (flip_h, flip_v) = (boolean(&mut r), boolean(&mut r));
                subs_out(s, &pic::shape_subs(geom.as_ref(), &syms, w, h, flip_h, flip_v));
            }
        });
        rep.check("subs", "v", &got);
    }
    for (name, store) in [("colored.canvas", ChannelStore::Clamp), ("colored.pdf", ChannelStore::Wrap)] {
        let mut r = Rng::new(140);
        let hostile = [0.0, -0.0, 0.25, 0.5, 1.0, 2.0, -1.0, NAN, 0.5, 0.499_999_9];
        let got = dig(|s| {
            for _ in 0..600 {
                let w = 1 + r.below(6);
                let h = 1 + r.below(5);
                let a: Vec<f32> = (0..w * h).map(|_| pick(&mut r, &hostile) as f32).collect();
                let m = Mask::from_vec(u32n(w), u32n(h), a).unwrap();
                let hex = u16s(pick(&mut r, &COLORS));
                let alpha = pick(&mut r, &[1.0, 0.5, 0.0, NAN, 2.0, -1.0, 0.3333]);
                s.bytes(&pic::colored_mask(&m, &hex, alpha, store));
            }
        });
        rep.check(name, "v", &got);
    }
    {
        let names = [
            "sysDot",
            "sysDash",
            "dash",
            "dashDot",
            "lgDash",
            "lgDashDot",
            "lgDashDotDot",
            "sysDashDot",
            "sysDashDotDot",
            "dot",
            "solid",
            "",
            "DASH",
        ];
        let got = dig(|s| {
            for n in names {
                match pic::dash(&u16s(n)) {
                    None => {
                        s.tag(b'N');
                    }
                    Some(a) => {
                        s.tag(b'D');
                        s.u32(u32n(a.len()));
                        for v in a {
                            f(s, *v);
                        }
                    }
                }
            }
        });
        rep.check("dash", "v", &got);
    }

    let report = rep.finish();
    eprintln!("g_picture: {}", report.summary());
    report.assert_clean();
}
