//! No panic and no hang on hostile geometry (R10) for `geom` and `paint` (frontier G): NaN, the
//! infinities, the extremes and random bit patterns in every number; degenerate paths (empty,
//! commands without points, unclosed, huge arc sweeps); colour strings of random UTF-16 (lone
//! surrogates included); sizes and steps that would exhaust the TS; runs with glyph ids past the
//! face; limits small enough that every refusal path runs. The functions are only called: what they
//! return is compared with the TS by the `g_*` parity tests.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use std::hint::black_box;
use std::sync::Arc;

use g_support::u16s;
use uo_drawingml::Mask;
use uo_fonts::{Face, FontLimits};
use uo_render::geom::{self, LineUnit, ShapeXf};
use uo_render::paint::PaintLimits;
use uo_render::paint::fill::{self, GradBox};
use uo_render::paint::page_fill as pf;
use uo_render::paint::picture::{self, ChannelStore, Painter};
use uo_render::paint::text_fx::{self as tx, RunView};
use uo_render::types::{
    BlipTile, CustCmd, CustPath, FxGlow, FxOutline, FxReflection, FxShadow, GeomEval, GradFill, GradStop, LineCap, LineJoin, Ltrb,
    PageFill, PageFillKind, PathCmd, PicGlow, PicLine, PicReflection, PicShadow, PictureLook, PresetSpec, ResolvedFill, ResolvedTextFx,
    ShadowKind, ShapeGeom,
};
use uo_render::{NO_GID, OptF64, Resources, SymbolTable};
use uo_testkit::generate::Rng;

/// Small enough that the refusal paths run and nothing allocates much.
fn tight() -> PaintLimits {
    PaintLimits {
        max_raster_px: 1 << 16,
        max_tiles: 1 << 12,
        max_samples: 1 << 10,
        max_dilate_offsets: 1 << 10,
        max_dilate_work: 1 << 22,
        max_fx_px: 1 << 18,
        draw: uo_drawingml::Limits {
            max_mask_pixels: 1 << 18,
            max_kernel_radius: 64,
            max_path_points: 1 << 16,
            max_fill_work: 1 << 24,
            ..uo_drawingml::Limits::NATIVE
        },
    }
}

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

/// A modest number or a hostile one.
fn val(r: &mut Rng) -> f64 {
    if r.below(3) == 0 { hostile(r) } else { (r.below(4001) as f64 - 500.0) / 4.0 }
}

fn units(r: &mut Rng) -> Vec<u16> {
    let n = r.below(9);
    (0..n)
        .map(|_| match r.below(4) {
            0 => 0xD800 + (r.below(0x800) as u16),
            1 => r.next_u32() as u16,
            _ => u16::from(b"0123456789abcdefABCDEF"[r.below(22)]),
        })
        .collect()
}

fn cmds(r: &mut Rng) -> Vec<PathCmd> {
    (0..r.below(8))
        .map(|_| match r.below(4) {
            0 => PathCmd::M { x: val(r), y: val(r) },
            1 => PathCmd::L { x: val(r), y: val(r) },
            2 => PathCmd::C { x1: val(r), y1: val(r), x2: val(r), y2: val(r), x: val(r), y: val(r) },
            _ => PathCmd::Z,
        })
        .collect()
}

fn cust(r: &mut Rng) -> Vec<CustPath> {
    (0..r.below(4))
        .map(|_| CustPath {
            w: val(r),
            h: val(r),
            fill: r.below(2) == 0,
            stroke: r.below(2) == 0,
            cmds: (0..r.below(10))
                .map(|_| match r.below(6) {
                    0 => CustCmd::M { x: val(r), y: val(r) },
                    1 => CustCmd::L { x: val(r), y: val(r) },
                    2 => CustCmd::C { pts: (0..r.below(5)).map(|_| [val(r), val(r)]).collect() },
                    3 => CustCmd::Q { pts: (0..r.below(4)).map(|_| [val(r), val(r)]).collect() },
                    4 => CustCmd::A { w_r: val(r), h_r: val(r), st: val(r), sw: hostile(r) },
                    _ => CustCmd::Z,
                })
                .collect(),
        })
        .collect()
}

const NAMES: [&str; 14] = [
    "triangle",
    "hexagon",
    "rightArrow",
    "ellipse",
    "roundRect",
    "round2SameRect",
    "frame",
    "bentConnector3",
    "line",
    "straightConnector9",
    "custom",
    "",
    "star5",
    "blockArc",
];

#[test]
fn geom_hostile() {
    let mut r = Rng::new(0x6e0_9a11);
    for _ in 0..20_000 {
        let name = u16s(NAMES[r.below(NAMES.len())]);
        let adj: Vec<(Vec<u16>, f64)> =
            (0..r.below(4)).map(|_| (u16s(["adj", "adj1", "adj2", "vf"][r.below(4)]), hostile(&mut r))).collect();
        let adj_refs: Vec<(&[u16], f64)> = adj.iter().map(|(n, v)| (n.as_slice(), *v)).collect();
        let (x, y, w, h) = (val(&mut r), val(&mut r), val(&mut r), val(&mut r));
        let xf = ShapeXf { flip_h: r.below(2) == 0, flip_v: r.below(2) == 0, rot: hostile(&mut r) };
        black_box(geom::preset_path_local(&name, w, h, &adj_refs));
        black_box(geom::preset_path(&name, x, y, w, h, &adj_refs, xf));
        black_box(geom::cust_paths(&cust(&mut r), x, y, w, h, xf));
        let c = cmds(&mut r);
        black_box((geom::rotate_path(&c, x, y, w), geom::path_bounds(&c), geom::map_path(&c, |a, b| (b * h, a + w))));
        for unit in [LineUnit::CanvasTwips, LineUnit::PdfPoints] {
            black_box(geom::line_path(unit, [x, y, w, h], Some([w, h, x, y]), xf.flip_h, xf.flip_v));
        }
        let mut out = Vec::new();
        black_box(geom::arc_to(&mut out, (x, y), w, h, hostile(&mut r), hostile(&mut r)));
        let mut res = Resources::new();
        let sg = ShapeGeom {
            geom: res.symbols.intern(&name).unwrap(),
            adj: (r.below(2) == 0).then(|| vec![(res.symbols.intern_str("adj").unwrap(), hostile(&mut r))]),
            cust: (r.below(2) == 0).then(|| res.add_cust(Arc::from(cust(&mut r))).unwrap()),
            flip_h: xf.flip_h,
            flip_v: xf.flip_v,
            rot: if r.below(2) == 0 { OptF64::NONE } else { OptF64::some(xf.rot) },
            fill_alpha: OptF64::NONE,
            line_alpha: OptF64::NONE,
            eval: GeomEval::Compact,
        };
        for unit in [LineUnit::CanvasTwips, LineUnit::PdfPoints] {
            black_box(geom::shape_parts(unit, &sg, &res, [x, y, w, h], (r.below(2) == 0).then_some([x, y, w, h])));
        }
    }
    // a huge sweep ends at once
    for sw in [f64::INFINITY, 1e300, f64::MAX, 1e9] {
        let mut out = Vec::new();
        black_box(geom::arc_to(&mut out, (0.0, 0.0), 1.0, 1.0, 0.0, sw));
        assert!(out.len() <= 16384);
    }
}

fn grad(r: &mut Rng, syms: &mut SymbolTable) -> GradFill {
    GradFill {
        stops: (0..r.below(6)).map(|_| GradStop { pos: hostile(r), color: syms.intern(&units(r)).unwrap() }).collect(),
        angle_deg: hostile(r),
        scaled: r.below(2) == 0,
        rot_with_shape: r.below(2) == 0,
        path: (r.below(2) == 0).then(|| syms.intern_str(["circle", "rect", "shape", ""][r.below(4)]).unwrap()),
        fill_to: (r.below(2) == 0).then(|| Ltrb { l: hostile(r), t: hostile(r), r: hostile(r), b: hostile(r) }),
    }
}

#[test]
fn fill_hostile() {
    let lim = tight();
    let mut r = Rng::new(0x6e0_f111);
    for _ in 0..3000 {
        let mut syms = SymbolTable::new();
        let g = grad(&mut r, &mut syms);
        let n = r.below(700);
        let _ = black_box(fill::gradient_samples(&fill::stops_of(&g, &syms), n, &lim));
        black_box(fill::gradient_geometry(
            &g,
            &syms,
            GradBox { x: val(&mut r), y: val(&mut r), w: val(&mut r), h: val(&mut r), rot: hostile(&mut r) },
            (val(&mut r), val(&mut r), val(&mut r), val(&mut r)),
        ));
        let names = ["pct50", "nope", "", "dkHorz", "smCheck"];
        let (fg, bg) = (units(&mut r), units(&mut r));
        let _ = black_box(fill::pattern_raster(
            &u16s(names[r.below(5)]),
            &fg,
            &bg,
            val(&mut r),
            val(&mut r),
            val(&mut r),
            val(&mut r),
            hostile(&mut r),
            hostile(&mut r),
            &lim,
        ));
        let tile = BlipTile {
            tx: hostile(&mut r),
            ty: hostile(&mut r),
            sx: hostile(&mut r),
            sy: hostile(&mut r),
            algn: syms.intern(&units(&mut r)).unwrap(),
            flip: syms.intern_str("none").unwrap(),
        };
        let nat = (r.below(2) == 0).then(|| (hostile(&mut r), hostile(&mut r)));
        let _ = black_box(fill::blip_tiles(
            (r.below(4) > 0).then_some(&tile),
            &syms,
            val(&mut r),
            val(&mut r),
            val(&mut r),
            val(&mut r),
            hostile(&mut r),
            nat,
            &lim,
        ));
        black_box(fill::is_rect_gradient(Some(&g), &syms));
    }
    // rect gradients: the side is capped at 2000, so a few big ones only
    for _ in 0..20 {
        let mut syms = SymbolTable::new();
        let g = grad(&mut r, &mut syms);
        let _ = black_box(fill::rect_gradient_raster(&g, &syms, val(&mut r), val(&mut r), hostile(&mut r), &lim));
    }
    // steps under the precision of the coordinate end at the limit
    let mut syms = SymbolTable::new();
    let tile =
        BlipTile { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, algn: syms.intern_str("tl").unwrap(), flip: syms.intern_str("none").unwrap() };
    for (x, y) in [(1e20, 0.0), (0.0, 1e20), (5.0, 5.0)] {
        let out = fill::blip_tiles(Some(&tile), &syms, x, y, 1e30, 1e30, 1e-9, Some((1.0, 1.0)), &lim);
        assert!(out.is_err(), "{x} {y}");
    }
    assert!(fill::pattern_raster(&u16s("pct50"), &[], &[], 0.0, 0.0, 1e9, 1e9, 0.75, 4.0, &lim).is_err());
}

fn page_fill(r: &mut Rng, syms: &mut SymbolTable) -> PageFill {
    PageFill {
        kind: [PageFillKind::Gradient, PageFillKind::GradientRadial, PageFillKind::Pattern, PageFillKind::Frame, PageFillKind::Tile]
            [r.below(5)],
        c1: syms.intern(&units(r)).unwrap(),
        c2: syms.intern(&units(r)).unwrap(),
        stops: (r.below(2) == 0)
            .then(|| (0..r.below(4)).map(|_| GradStop { pos: hostile(r), color: syms.intern(&units(r)).unwrap() }).collect()),
        angle: hostile(r),
        focus: hostile(r),
        focus_position: (r.below(2) == 0).then(|| [hostile(r), hostile(r)]),
        gradient_center: r.below(2) == 0,
        image: None,
        image_px: (r.below(2) == 0).then(|| [hostile(r), hostile(r), hostile(r)]),
        pattern: (r.below(2) == 0).then(|| (0..r.below(12)).map(|_| hostile(r)).collect()),
    }
}

#[test]
fn page_fill_hostile() {
    let lim = tight();
    let mut r = Rng::new(0x6e0_9f11);
    for _ in 0..3000 {
        let mut syms = SymbolTable::new();
        let fl = page_fill(&mut r, &mut syms);
        for pdf in [false, true] {
            black_box(pf::fill_param(&fl, hostile(&mut r), hostile(&mut r), pdf));
            black_box(pf::gradient_tile_px(&fl, pdf));
            black_box(pf::page_fill_tile_tw(&fl, hostile(&mut r), hostile(&mut r), pdf));
            let _ = black_box(pf::gradient_tile_rgba(&fl, &syms, r.below(70), r.below(70), pdf, &lim));
        }
        black_box(pf::pattern_rgba(&fl, &syms));
    }
    assert!(
        pf::gradient_tile_rgba(&page_fill(&mut r, &mut SymbolTable::new()), &SymbolTable::new(), 1 << 20, 1 << 20, false, &lim).is_err()
    );
    assert!(pf::gradient_tile_rgba(&page_fill(&mut r, &mut SymbolTable::new()), &SymbolTable::new(), usize::MAX, 2, false, &lim).is_err());
}

fn face() -> Face {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let bytes: Arc<[u8]> = std::fs::read(root.join("shared/fonts/files/carlito-regular.ttf")).unwrap().into();
    Face::parse(bytes, &FontLimits::DEFAULT).unwrap()
}

#[test]
fn text_fx_hostile() {
    let lim = tight();
    let face = face();
    let mut r = Rng::new(0x6e0_7f11);
    for _ in 0..600 {
        let n = r.below(7);
        let cps: Vec<u32> = (0..n).map(|_| r.next_u32() % 0x12_0000).collect();
        let xs: Vec<f64> = (0..n).map(|_| val(&mut r)).collect();
        let gids: Vec<u32> =
            if r.below(2) == 0 { Vec::new() } else { (0..n).map(|_| if r.below(2) == 0 { NO_GID } else { r.next_u32() }).collect() };
        // short or mismatched columns must not panic either
        let xs_short = if r.below(8) == 0 { xs.get(..xs.len().saturating_sub(1)).unwrap_or(&[]).to_vec() } else { xs.clone() };
        let run = RunView {
            face: &face,
            size_pt: val(&mut r),
            y: val(&mut r),
            scale_x: if r.below(2) == 0 { OptF64::NONE } else { OptF64::some(hostile(&mut r)) },
            synth_italic: r.below(2) == 0,
            cps: &cps,
            xs: &xs_short,
            gids: &gids,
        };
        let mut syms = SymbolTable::new();
        let s = |syms: &mut SymbolTable, r: &mut Rng| syms.intern(&units(r)).unwrap();
        let fx = ResolvedTextFx {
            fill: (r.below(2) == 0).then_some(ResolvedFill::None),
            outline: (r.below(2) == 0).then(|| FxOutline {
                width_pt: hostile(&mut r),
                fill: ResolvedFill::None,
                dash: None,
                cap: LineCap::Round,
                join: LineJoin::Bevel,
            }),
            shadow: (r.below(2) == 0).then(|| FxShadow {
                blur_pt: hostile(&mut r),
                dist_pt: hostile(&mut r),
                dir_deg: hostile(&mut r),
                sx: hostile(&mut r),
                sy: hostile(&mut r),
                kx_deg: hostile(&mut r),
                ky_deg: hostile(&mut r),
                algn: s(&mut syms, &mut r),
                hex: s(&mut syms, &mut r),
                alpha: hostile(&mut r),
            }),
            glow: (r.below(2) == 0).then(|| FxGlow { rad_pt: hostile(&mut r), hex: s(&mut syms, &mut r), alpha: hostile(&mut r) }),
            reflection: (r.below(2) == 0).then(|| FxReflection {
                blur_pt: hostile(&mut r),
                st_a: hostile(&mut r),
                st_pos: hostile(&mut r),
                end_a: hostile(&mut r),
                end_pos: hostile(&mut r),
                dist_pt: hostile(&mut r),
                dir_deg: hostile(&mut r),
                algn: s(&mut syms, &mut r),
            }),
        };
        black_box(tx::fx_margin_pt(&fx, hostile(&mut r)));
        black_box(tx::run_box(&run));
        black_box(tx::run_outline_segs(&run));
        let hex = units(&mut r);
        let _ = black_box(tx::fx_layers(&run, &fx, &syms, [hostile(&mut r), 0.5, 1.0, 2.78][r.below(4)], &hex, &lim));
        let m = Mask::new(u32::try_from(r.below(30)).unwrap(), u32::try_from(r.below(30)).unwrap(), &lim.draw).unwrap();
        let _ = black_box(tx::dilate(&m, hostile(&mut r), &lim));
        black_box(tx::dash_pattern(Some(&units(&mut r))));
        black_box(tx::grad_of(&ResolvedFill::Grad {
            stops: Vec::new(),
            angle_deg: hostile(&mut r),
            scaled: true,
            path: None,
            fill_to: None,
        }));
    }
    // a run with no glyphs, a face-less state is not representable; an empty run gives none
    let run = RunView { face: &face, size_pt: 12.0, y: 0.0, scale_x: OptF64::NONE, synth_italic: false, cps: &[], xs: &[], gids: &[] };
    assert!(tx::run_box(&run).is_none());
    // the largest mask the limits allow
    assert!(tx::dilate(&Mask::new(10, 10, &lim.draw).unwrap(), f64::INFINITY, &lim).is_err());
}

#[test]
fn picture_hostile() {
    let mut r = Rng::new(0x6e0_b111);
    for _ in 0..4000 {
        let mut syms = SymbolTable::new();
        let c = syms.intern_str("000000").unwrap();
        let look = PictureLook {
            geom: (r.below(2) == 0).then(|| PresetSpec {
                prst: syms.intern_str(NAMES[r.below(NAMES.len())]).unwrap(),
                adj: (r.below(2) == 0).then(|| {
                    vec![(syms.intern_str("adj1").unwrap(), hostile(&mut r)), (syms.intern(&units(&mut r)).unwrap(), hostile(&mut r))]
                }),
            }),
            line: (r.below(2) == 0).then(|| PicLine {
                color: c,
                alpha: 1.0,
                width_pt: hostile(&mut r),
                dash: None,
                cmpd: None,
                cap: None,
                join: None,
            }),
            shadow: (r.below(2) == 0).then(|| PicShadow {
                kind: if r.below(2) == 0 { ShadowKind::Outer } else { ShadowKind::Inner },
                color: c,
                alpha: 1.0,
                blur_pt: hostile(&mut r),
                dist_pt: hostile(&mut r),
                dir_deg: hostile(&mut r),
                sx: OptF64::some(hostile(&mut r)),
                sy: OptF64::some(hostile(&mut r)),
                kx_deg: OptF64::some(hostile(&mut r)),
                ky_deg: OptF64::NONE,
                algn: None,
                rot_with_shape: false,
            }),
            glow: (r.below(2) == 0).then(|| PicGlow { color: c, alpha: 1.0, rad_pt: hostile(&mut r) }),
            reflection: (r.below(2) == 0).then(|| PicReflection {
                blur_pt: hostile(&mut r),
                st_a: 0.0,
                st_pos: 0.0,
                end_a: 0.0,
                end_pos: 0.0,
                dist_pt: hostile(&mut r),
                dir_deg: hostile(&mut r),
                sy: 1.0,
                algn: None,
                rot_with_shape: false,
            }),
            ..PictureLook::default()
        };
        let (w, h) = (val(&mut r), val(&mut r));
        for painter in [Painter::Canvas, Painter::Pdf] {
            black_box(picture::effect_bounds(painter, Some(&look), w, h));
        }
        black_box(picture::shape_subs(look.geom.as_ref(), &syms, w, h, r.below(2) == 0, r.below(2) == 0));
        let mw = r.below(6);
        let mh = r.below(6);
        let a: Vec<f32> = (0..mw * mh).map(|_| hostile(&mut r) as f32).collect();
        let m = Mask::from_vec(u32::try_from(mw).unwrap(), u32::try_from(mh).unwrap(), a).unwrap();
        for store in [ChannelStore::Clamp, ChannelStore::Wrap] {
            black_box(picture::colored_mask(&m, &units(&mut r), hostile(&mut r), store));
        }
        black_box(picture::dash(&units(&mut r)));
    }
}
