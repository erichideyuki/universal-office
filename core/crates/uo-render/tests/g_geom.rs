//! R-P3 parity of `geom` (frontier G): `presetPathLocal`, `presetPath`, `custPaths`, `arcTo`,
//! `rotatePath`, the line path, `pathBounds`, `mapPath`, `OPEN_PRESETS` and the shape parts of both
//! painters in their own unit, replayed against `tests/captures/g_geom.capture`, written by
//! `tools/capture-g.mjs` from the frozen TS (Node 24.15.0, V8 13.6). The inputs are regenerated here
//! in the order of the script's generators.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use g_support::*;
use std::sync::Arc;
use uo_render::geom::{self, LineUnit, ShapeXf};
use uo_render::types::{CustCmd, CustPath, GeomEval, PathCmd, ShapeGeom};
use uo_render::{OptF64, Resources};
use uo_testkit::generate::Rng;
use uo_testkit::{Replay, Stream};

const CAPTURE: &str = include_str!("captures/g_geom.capture");

fn gen_cmds(r: &mut Rng) -> Vec<PathCmd> {
    let n = r.below(7);
    let mut out = Vec::new();
    for _ in 0..n {
        match r.below(4) {
            0 => out.push(PathCmd::M { x: num(r), y: num(r) }),
            1 => out.push(PathCmd::L { x: num(r), y: num(r) }),
            2 => {
                let (x1, y1, x2, y2, x, y) = (num(r), num(r), num(r), num(r), num(r), num(r));
                out.push(PathCmd::C { x1, y1, x2, y2, x, y });
            }
            _ => out.push(PathCmd::Z),
        }
    }
    out
}

const SWEEPS: [f64; 15] = [90.0, -90.0, 180.0, 270.0, 360.0, 45.0, 720.0, 1e-12, 89.999_999_999_9, 91.0, -360.0, 0.0, NAN, 30.5, 1000.0];
const CUST_DIM: [f64; 9] = [0.0, 1.0, 100.0, NAN, -50.0, 2000.0, 1e9, 21600.0, 3.5];

fn gen_cust(r: &mut Rng) -> Vec<CustPath> {
    let mut paths = Vec::new();
    let np = r.below(4);
    for _ in 0..np {
        let w = pick(r, &CUST_DIM);
        let h = pick(r, &CUST_DIM);
        let fill = boolean(r);
        let stroke = boolean(r);
        let mut cmds = Vec::new();
        let nc = r.below(9);
        for _ in 0..nc {
            match r.below(8) {
                0 | 6 => cmds.push(CustCmd::M { x: num(r), y: num(r) }),
                1 | 7 => cmds.push(CustCmd::L { x: num(r), y: num(r) }),
                2 => {
                    let k = r.below(5);
                    let pts = (0..k).map(|_| [num(r), num(r)]).collect();
                    cmds.push(CustCmd::C { pts });
                }
                3 => {
                    let k = r.below(4);
                    let pts = (0..k).map(|_| [num(r), num(r)]).collect();
                    cmds.push(CustCmd::Q { pts });
                }
                4 => {
                    let (w_r, h_r, st) = (num(r), num(r), num(r));
                    cmds.push(CustCmd::A { w_r, h_r, st, sw: pick(r, &SWEEPS) });
                }
                _ => cmds.push(CustCmd::Z),
            }
        }
        paths.push(CustPath { w, h, fill, stroke, cmds });
    }
    paths
}

const PRESETS: [&str; 29] = [
    "triangle",
    "rtTriangle",
    "diamond",
    "parallelogram",
    "trapezoid",
    "homePlate",
    "chevron",
    "hexagon",
    "octagon",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "upDownArrow",
    "leftRightArrow",
    "ellipse",
    "roundRect",
    "round2SameRect",
    "frame",
    "bentConnector3",
    "line",
    "straightConnector1",
    "rect",
    "bentConnector2",
    "curvedConnector3",
    "star5",
    "",
    "custom",
    "Triangle",
];

type Adj = Option<Vec<(&'static str, f64)>>;

fn gen_adj(r: &mut Rng) -> Adj {
    match r.below(6) {
        0 => None,
        1 => Some(vec![("adj", num(r))]),
        2 => {
            let (a, b) = (num(r), num(r));
            Some(vec![("adj1", a), ("adj2", b)])
        }
        3 => {
            let (a, b) = (num(r), num(r));
            Some(vec![("adj", a), ("adj1", b)])
        }
        4 => {
            let (a, b) = (num(r), num(r));
            Some(vec![("vf", a), ("adj", b)])
        }
        _ => {
            let (a, b, c) = (num(r), num(r), num(r));
            Some(vec![("adj1", a), ("adj2", b), ("adj", c)])
        }
    }
}

fn adj_units(a: &Adj) -> Vec<(Vec<u16>, f64)> {
    a.as_ref().map_or_else(Vec::new, |v| v.iter().map(|&(n, x)| (u16s(n), x)).collect())
}

fn adj_refs(a: &[(Vec<u16>, f64)]) -> Vec<(&[u16], f64)> {
    a.iter().map(|(n, v)| (n.as_slice(), *v)).collect()
}

fn gen_line(r: &mut Rng) -> Option<[f64; 4]> {
    if r.below(3) == 0 { Some([num(r), num(r), num(r), num(r)]) } else { None }
}

fn parts_digest(s: &mut Stream, parts: &[(Vec<PathCmd>, bool, bool, Option<bool>)]) {
    s.u32(u32n(parts.len()));
    for (cmds, fill, stroke, even_odd) in parts {
        cmd_out(s, cmds);
        s.bool(*fill);
        s.bool(*stroke);
        if let Some(e) = even_odd {
            s.bool(*e);
        }
    }
}

#[test]
fn geom_parity() {
    let cap = capture(CAPTURE);
    let mut rep = Replay::new(&cap);
    let v = |rep: &mut Replay<'_>, key: &str, got: String| {
        rep.check(key, "v", &got);
    };

    // presetPathLocal and presetPath
    for (pi, prst) in PRESETS.iter().enumerate() {
        let mut r = Rng::new(1000 + u32n(pi));
        let name = u16s(prst);
        let got = dig(|s| {
            for _ in 0..300 {
                let (w, h) = (num(&mut r), num(&mut r));
                let adj = gen_adj(&mut r);
                let au = adj_units(&adj);
                let ar = adj_refs(&au);
                cmd_out(s, &geom::preset_path_local(&name, w, h, &ar));
                let (x, y) = (num(&mut r), num(&mut r));
                let (flip_h, flip_v) = (boolean(&mut r), boolean(&mut r));
                let rot = if r.below(4) == 0 { 0.0 } else { num(&mut r) };
                cmd_out(s, &geom::preset_path(&name, x, y, w, h, &ar, ShapeXf { flip_h, flip_v, rot }));
            }
        });
        v(&mut rep, &format!("preset/{prst}"), got);
    }

    // custPaths
    {
        let mut r = Rng::new(5000);
        let got = dig(|s| {
            for _ in 0..3000 {
                let cust = gen_cust(&mut r);
                let (x, y, w, h) = (num(&mut r), num(&mut r), num(&mut r), num(&mut r));
                let (flip_h, flip_v) = (boolean(&mut r), boolean(&mut r));
                let rot = if r.below(4) == 0 { 0.0 } else { num(&mut r) };
                let parts = geom::cust_paths(&cust, x, y, w, h, ShapeXf { flip_h, flip_v, rot });
                let parts: Vec<_> = parts.into_iter().map(|p| (p.cmds, p.fill, p.stroke, None)).collect();
                parts_digest(s, &parts);
            }
        });
        v(&mut rep, "cust", got);
    }

    // arcTo
    {
        let mut r = Rng::new(6000);
        let got = dig(|s| {
            for _ in 0..3000 {
                let mut out = Vec::new();
                let cur = (num(&mut r), num(&mut r));
                let (w_r, h_r, st) = (num(&mut r), num(&mut r), num(&mut r));
                let sw = pick(&mut r, &SWEEPS);
                let end = geom::arc_to(&mut out, cur, w_r, h_r, st, sw);
                cmd_out(s, &out);
                f(s, end.0);
                f(s, end.1);
            }
        });
        v(&mut rep, "arc", got);
    }

    // rotatePath (both painters: the same arithmetic) and the line paths
    for name in ["rotate.canvas", "rotate.pdf"] {
        let mut r = Rng::new(7000);
        let got = dig(|s| {
            for _ in 0..2000 {
                let cmds = gen_cmds(&mut r);
                let (cx, cy, deg) = (num(&mut r), num(&mut r), num(&mut r));
                cmd_out(s, &geom::rotate_path(&cmds, cx, cy, deg));
            }
        });
        v(&mut rep, name, got);
    }
    for (name, unit) in [("line.canvas", LineUnit::CanvasTwips), ("line.pdf", LineUnit::PdfPoints)] {
        let mut r = Rng::new(7100);
        let got = dig(|s| {
            for _ in 0..2000 {
                let frame = [num(&mut r), num(&mut r), num(&mut r), num(&mut r)];
                let line = gen_line(&mut r);
                let (fh, fv) = (boolean(&mut r), boolean(&mut r));
                cmd_out(s, &geom::line_path(unit, frame, line, fh, fv));
            }
        });
        v(&mut rep, name, got);
    }

    // pathBounds, mapPath, OPEN_PRESETS
    {
        let mut r = Rng::new(7200);
        let got = dig(|s| {
            for _ in 0..2000 {
                let b = geom::path_bounds(&gen_cmds(&mut r));
                for x in [b.0, b.1, b.2, b.3] {
                    f(s, x);
                }
            }
        });
        v(&mut rep, "bounds", got);
    }
    {
        let mut r = Rng::new(7300);
        let got = dig(|s| {
            for _ in 0..2000 {
                let cmds = gen_cmds(&mut r);
                cmd_out(s, &geom::map_path(&cmds, |x, y| (x * 1.5 + y * 0.25, y - 3.0)));
            }
        });
        v(&mut rep, "map", got);
    }
    {
        let got = dig(|s| {
            for n in PRESETS.iter().chain(["bentConnector3 ", "curvedConnector3", "straightConnector3"].iter()) {
                s.bool(geom::is_open_preset(&u16s(n)));
            }
        });
        v(&mut rep, "open", got);
    }

    // shape parts of both painters
    const GEOMS: [&str; 12] = [
        "rect",
        "line",
        "straightConnector1",
        "straightConnector3",
        "custom",
        "ellipse",
        "triangle",
        "roundRect",
        "bentConnector3",
        "",
        "straightConnect",
        "star5",
    ];
    for (name, unit) in [("parts.canvas", LineUnit::CanvasTwips), ("parts.pdf", LineUnit::PdfPoints)] {
        let mut r = Rng::new(8000);
        let got = dig(|s| {
            for _ in 0..3000 {
                let gname = pick(&mut r, &GEOMS);
                let adj = if r.below(3) == 0 {
                    let (a, b) = (num(&mut r), num(&mut r));
                    Some(vec![("adj", a), ("adj1", b)])
                } else {
                    None
                };
                let cust = if gname == "custom" && r.below(4) > 0 { Some(gen_cust(&mut r)) } else { None };
                let (flip_h, flip_v) = (boolean(&mut r), boolean(&mut r));
                let rot = if r.below(3) == 0 { None } else { Some(num(&mut r)) };
                let frame = [num(&mut r), num(&mut r), num(&mut r), num(&mut r)];
                let line = gen_line(&mut r);
                let mut res = Resources::new();
                let sg = ShapeGeom {
                    geom: res.symbols.intern_str(gname).unwrap(),
                    adj: adj.map(|a: Vec<(&str, f64)>| a.into_iter().map(|(n, v)| (res.symbols.intern_str(n).unwrap(), v)).collect()),
                    cust: cust.map(|c| res.add_cust(Arc::from(c)).unwrap()),
                    flip_h,
                    flip_v,
                    rot: OptF64::from(rot),
                    fill_alpha: OptF64::NONE,
                    line_alpha: OptF64::NONE,
                    eval: GeomEval::Compact,
                };
                let parts: Vec<_> = geom::shape_parts(unit, &sg, &res, frame, line)
                    .into_iter()
                    .map(|p| (p.cmds, p.fill, p.stroke, Some(p.even_odd)))
                    .collect();
                parts_digest(s, &parts);
            }
        });
        v(&mut rep, name, got);
    }

    let report = rep.finish();
    eprintln!("g_geom: {}", report.summary());
    report.assert_clean();
}

#[test]
fn pending_list_is_empty() {
    assert!(geom::PENDING.is_empty());
}
