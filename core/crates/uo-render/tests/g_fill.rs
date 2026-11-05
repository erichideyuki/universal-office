//! R-P3 parity of `paint::fill` (frontier G): `gradientSamples`, `gradientGeometry`,
//! `patternRaster`, `blipTiles`, `isRectGradient`, `rectGradientRaster` against
//! `tests/captures/g_fill.capture` (the frozen TS, taken with the pow reference environment: the
//! gradient ramp uses `**`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use g_support::*;
use uo_render::SymbolTable;
use uo_render::paint::fill::{self, GradBox};
use uo_render::paint::{GradGeom, PaintLimits};
use uo_render::types::{BlipTile, GradFill, GradStop, Ltrb};
use uo_testkit::Replay;
use uo_testkit::generate::Rng;

const CAPTURE: &str = include_str!("captures/g_fill.capture");
const L: PaintLimits = PaintLimits::DEFAULT;

/// `genStops(r)`: `(pos, colour)` pairs.
fn gen_stops(r: &mut Rng) -> Vec<(f64, &'static str)> {
    let k = r.below(10);
    let n = if k == 0 || k == 1 { 2 } else { r.below(6) };
    let mut stops = Vec::new();
    for i in 0..n {
        let mut pos = pick(r, &POS);
        if k == 0 {
            pos = if i == 0 { 0.0 } else { 1.0 };
        }
        if k == 1 {
            pos = if i == 0 { -1.0 } else { 2.0 };
        }
        stops.push((pos, pick(r, &COLORS)));
    }
    stops
}

const FILLTO: [f64; 8] = [0.0, 0.25, 0.5, 0.75, 1.0, -0.1, NAN, 0.3];

fn gen_fill_to(r: &mut Rng) -> Option<Ltrb> {
    if r.below(2) == 0 { None } else { Some(Ltrb { l: pick(r, &FILLTO), t: pick(r, &FILLTO), r: pick(r, &FILLTO), b: pick(r, &FILLTO) }) }
}

const PATHS: [Option<&str>; 6] = [None, Some("circle"), Some("rect"), Some("shape"), Some("path"), Some("")];

fn grad(
    syms: &mut SymbolTable,
    stops: &[(f64, &str)],
    angle_deg: f64,
    scaled: bool,
    rot_with_shape: bool,
    path: Option<&str>,
    fill_to: Option<Ltrb>,
) -> GradFill {
    GradFill {
        stops: stops.iter().map(|&(pos, c)| GradStop { pos, color: syms.intern_str(c).unwrap() }).collect(),
        angle_deg,
        scaled,
        rot_with_shape,
        path: path.map(|p| syms.intern_str(p).unwrap()),
        fill_to,
    }
}

#[test]
fn fill_parity() {
    let cap = capture(CAPTURE);
    let mut rep = Replay::new(&cap);

    // gradientSamples
    {
        let mut r = Rng::new(100);
        let got = dig(|s| {
            for _ in 0..1200 {
                let stops = gen_stops(&mut r);
                let n = pick(&mut r, &[1usize, 2, 3, 17, 64, 256, 512]);
                let u: Vec<(f64, Vec<u16>)> = stops.iter().map(|&(p, c)| (p, u16s(c))).collect();
                let refs: Vec<(f64, &[u16])> = u.iter().map(|(p, c)| (*p, c.as_slice())).collect();
                s.bytes(&fill::gradient_samples(&refs, n, &L).unwrap());
            }
        });
        rep.check("samples", "v", &got);
    }

    // gradientGeometry
    {
        let mut r = Rng::new(200);
        let got = dig(|s| {
            for _ in 0..3000 {
                let angle = num(&mut r);
                let scaled = boolean(&mut r);
                let rws = pick(&mut r, &[None, Some(true), Some(false)]);
                let path = pick(&mut r, &PATHS);
                let fill_to = gen_fill_to(&mut r);
                let frame_xywh = [num(&mut r), num(&mut r), num(&mut r), num(&mut r)];
                let rot = if r.below(3) == 0 { 0.0 } else { num(&mut r) };
                let pb = (num(&mut r), num(&mut r), num(&mut r), num(&mut r));
                let mut syms = SymbolTable::new();
                let g = grad(&mut syms, &[], angle, scaled, rws.unwrap_or(true), path, fill_to);
                let frame = GradBox { x: frame_xywh[0], y: frame_xywh[1], w: frame_xywh[2], h: frame_xywh[3], rot };
                match fill::gradient_geometry(&g, &syms, frame, pb) {
                    GradGeom::Axial { x0, y0, x1, y1 } => {
                        s.tag(b'a');
                        for v in [x0, y0, x1, y1] {
                            f(s, v);
                        }
                    }
                    GradGeom::Radial { x0, y0, r } => {
                        s.tag(b'r');
                        for v in [x0, y0, r] {
                            f(s, v);
                        }
                    }
                }
            }
        });
        rep.check("geometry", "v", &got);
    }

    // patternRaster
    {
        let mut names: Vec<&str> = uo_drawingml::preset_patterns().iter().map(|p| p.0).collect();
        names.sort_unstable();
        names.push("nope");
        names.push("");
        let mut r = Rng::new(300);
        let got = dig(|s| {
            for _ in 0..400 {
                let prst = pick(&mut r, &names);
                let fg = pick(&mut r, &COLORS);
                let bg = pick(&mut r, &COLORS);
                let xs = [0.0, 100.5, -37.0, 1234.5, 5000.25, -10000.0];
                let x = pick(&mut r, &xs);
                let y = pick(&mut r, &xs);
                let ws = [0.0, -5.0, 1.0, 7.5, 90.0, 300.0];
                let w = pick(&mut r, &ws);
                let h = pick(&mut r, &ws);
                let px = pick(&mut r, &[15.0, 0.75, 1.0, 20.0, 2.5]);
                let k = pick(&mut r, &[1.0, 4.0, 2.0, 3.0]);
                match fill::pattern_raster(&u16s(prst), &u16s(fg), &u16s(bg), x, y, w, h, px, k, &L).unwrap() {
                    None => {
                        s.tag(b'N');
                    }
                    Some(o) => {
                        s.tag(b'P');
                        s.bytes(&o.data);
                        s.u32(u32n(o.w));
                        s.u32(u32n(o.h));
                        f(s, o.x);
                        f(s, o.y);
                        f(s, o.cells_w);
                        f(s, o.cells_h);
                    }
                }
            }
        });
        rep.check("pattern", "v", &got);
    }

    // blipTiles
    {
        let nat: [Option<(f64, f64)>; 9] = [
            None,
            None,
            Some((1.0, 1.0)),
            Some((16.0, 16.0)),
            Some((48.0, 32.0)),
            Some((100.0, 640.0)),
            Some((0.0, 10.0)),
            Some((NAN, 10.0)),
            Some((640.0, 480.0)),
        ];
        let algns = [
            Some("tl"),
            Some("t"),
            Some("tr"),
            Some("l"),
            Some("ctr"),
            Some("r"),
            Some("bl"),
            Some("b"),
            Some("br"),
            Some("zz"),
            Some(""),
            None,
        ];
        let mut r = Rng::new(400);
        let got = dig(|s| {
            for _ in 0..1500 {
                let mut syms = SymbolTable::new();
                let tile = if r.below(4) == 0 {
                    None
                } else {
                    let emu_pool = [0.0, 9525.0, -9525.0, 127_000.0, 3175.0, 1e5];
                    let sc = [100_000.0, 50_000.0, 200_000.0, 12_345.0, 0.0, -100_000.0, NAN];
                    let tx = pick(&mut r, &emu_pool);
                    let ty = pick(&mut r, &emu_pool);
                    let sx = pick(&mut r, &sc);
                    let sy = pick(&mut r, &sc);
                    let algn = pick(&mut r, &algns).unwrap_or("tl");
                    let algn = syms.intern_str(algn).unwrap();
                    Some(BlipTile { tx, ty, sx, sy, algn, flip: syms.intern_str("none").unwrap() })
                };
                let natural = pick(&mut r, &nat);
                let fr = [0.0, 100.0, -50.5, 1440.0, 3000.0, 12000.0];
                let (x, y, w, h) = (pick(&mut r, &fr), pick(&mut r, &fr), pick(&mut r, &fr), pick(&mut r, &fr));
                let emu = pick(&mut r, &[1.0 / 635.0, 1.0 / 12700.0, 1.0, 0.5]);
                let out = fill::blip_tiles(tile.as_ref(), &syms, x, y, w, h, emu, natural, &L).unwrap();
                s.u32(u32n(out.len()));
                for t in &out {
                    for v in [t.x, t.y, t.w, t.h] {
                        f(s, v);
                    }
                }
            }
        });
        rep.check("tiles", "v", &got);
    }

    // isRectGradient, rectGradientRaster
    {
        let mut r = Rng::new(500);
        let got = dig(|s| {
            for _ in 0..300 {
                let stops = gen_stops(&mut r);
                let path = pick(&mut r, &PATHS);
                let fill_to = gen_fill_to(&mut r);
                let mut syms = SymbolTable::new();
                let g = grad(&mut syms, &stops, 0.0, false, true, path, fill_to);
                s.bool(fill::is_rect_gradient(Some(&g), &syms));
                let dims = [0.0, 1.0, 10.0, 33.5, 100.0, 150.0, -5.0, 4000.0];
                let w = pick(&mut r, &dims);
                let h = pick(&mut r, &dims);
                let ppu = pick(&mut r, &[2.0, 0.5, 1.0, 0.1, 15.0, 3.3]);
                let o = fill::rect_gradient_raster(&g, &syms, w, h, ppu, &L).unwrap();
                s.u32(u32n(o.w));
                s.u32(u32n(o.h));
                s.bytes(&o.data);
            }
            s.bool(fill::is_rect_gradient(None, &SymbolTable::new()));
        });
        rep.check("rect", "v", &got);
    }

    let report = rep.finish();
    eprintln!("g_fill: {}", report.summary());
    report.assert_clean();
}
