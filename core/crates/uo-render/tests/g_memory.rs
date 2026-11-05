//! Memory gate of frontier G (M1, M2, M3): every `geom` and `paint` function (and each of its
//! error paths) returns the live bytes exactly to their starting value, outputs `black_box`ed; the
//! limit checks come before the allocation they guard; a soak of the pipeline is flat with a
//! tolerance of 0 B; the bytes allocated and the peak of the big rasters are measured (the numbers
//! of the module record) and kept under budgets.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use std::hint::black_box;
use std::sync::Arc;

use g_support::{COLORS, VALID, u16s};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::Mask;
use uo_fonts::{Face, FontLimits};
use uo_render::geom::{self, LineUnit, ShapeXf};
use uo_render::paint::fill::{self, GradBox};
use uo_render::paint::page_fill as pf;
use uo_render::paint::picture::{self, ChannelStore, Painter};
use uo_render::paint::text_fx::{self as tx, RunView};
use uo_render::paint::{LimitKind, PaintError, PaintLimits};
use uo_render::types::{
    BlipTile, CustCmd, CustPath, FxGlow, FxOutline, FxReflection, FxShadow, GeomEval, GradFill, GradStop, LineCap, LineJoin, PageFill,
    PageFillKind, PictureLook, ResolvedFill, ResolvedTextFx, ShapeGeom,
};
use uo_render::{OptF64, Resources, SymbolTable};

const L: PaintLimits = PaintLimits::DEFAULT;

fn face() -> Face {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let bytes: Arc<[u8]> = std::fs::read(root.join("shared/fonts/files/liberationserif-regular.ttf")).unwrap().into();
    Face::parse(bytes, &FontLimits::DEFAULT).unwrap()
}

struct Cols {
    cps: Vec<u32>,
    xs: Vec<f64>,
    gids: Vec<u32>,
}

fn cols(text: &str) -> Cols {
    let cps: Vec<u32> = text.chars().map(u32::from).collect();
    let xs = (0..cps.len()).map(|i| 1440.0 + 130.0 * i as f64).collect();
    Cols { cps, xs, gids: Vec::new() }
}

fn run<'a>(face: &'a Face, c: &'a Cols, size_pt: f64) -> RunView<'a> {
    RunView { face, size_pt, y: 2000.0, scale_x: OptF64::NONE, synth_italic: false, cps: &c.cps, xs: &c.xs, gids: &c.gids }
}

fn fx_all(syms: &mut SymbolTable) -> ResolvedTextFx {
    let mut s = |t: &str| syms.intern_str(t).unwrap();
    ResolvedTextFx {
        fill: Some(ResolvedFill::None),
        outline: Some(FxOutline { width_pt: 1.5, fill: ResolvedFill::None, dash: None, cap: LineCap::Butt, join: LineJoin::Miter }),
        shadow: Some(FxShadow {
            blur_pt: 3.0,
            dist_pt: 3.0,
            dir_deg: 45.0,
            sx: 1.0,
            sy: 0.5,
            kx_deg: 10.0,
            ky_deg: 0.0,
            algn: s("b"),
            hex: s("808080"),
            alpha: 0.5,
        }),
        glow: Some(FxGlow { rad_pt: 4.0, hex: s("FF0000"), alpha: 0.5 }),
        reflection: Some(FxReflection {
            blur_pt: 1.0,
            st_a: 0.5,
            st_pos: 0.0,
            end_a: 0.0,
            end_pos: 0.9,
            dist_pt: 1.0,
            dir_deg: 90.0,
            algn: s("b"),
        }),
    }
}

fn cust() -> Vec<CustPath> {
    vec![CustPath {
        w: 100.0,
        h: 50.0,
        fill: true,
        stroke: true,
        cmds: vec![
            CustCmd::M { x: 0.0, y: 0.0 },
            CustCmd::L { x: 100.0, y: 0.0 },
            CustCmd::C { pts: vec![[100.0, 20.0], [80.0, 50.0], [50.0, 50.0]] },
            CustCmd::Q { pts: vec![[10.0, 50.0], [0.0, 30.0]] },
            CustCmd::A { w_r: 10.0, h_r: 10.0, st: 180.0, sw: 90.0 },
            CustCmd::Z,
        ],
    }]
}

fn grad(syms: &mut SymbolTable, path: Option<&str>) -> GradFill {
    GradFill {
        stops: [(0.0, "FF0000"), (0.5, "00FF00"), (1.0, "0000FF")]
            .iter()
            .map(|&(pos, c)| GradStop { pos, color: syms.intern_str(c).unwrap() })
            .collect(),
        angle_deg: 45.0,
        scaled: true,
        rot_with_shape: true,
        path: path.map(|p| syms.intern_str(p).unwrap()),
        fill_to: None,
    }
}

fn page_fill(syms: &mut SymbolTable, kind: PageFillKind) -> PageFill {
    PageFill {
        kind,
        c1: syms.intern_str("FF0000").unwrap(),
        c2: syms.intern_str("0000FF").unwrap(),
        stops: None,
        angle: 45.0,
        focus: 50.0,
        focus_position: Some([0.25, 0.75]),
        gradient_center: true,
        image: None,
        image_px: Some([64.0, 64.0, 96.0]),
        pattern: Some(vec![0.0, 255.0, 170.0, 85.0, 1.0, 128.0, 3.0, 4.0]),
    }
}

#[test]
fn m1_geom() {
    for prst in [
        "triangle",
        "hexagon",
        "rightArrow",
        "upDownArrow",
        "ellipse",
        "roundRect",
        "round2SameRect",
        "frame",
        "bentConnector3",
        "line",
        "nope",
    ] {
        for (w, h) in [(1440.0, 720.0), (0.0, 0.0), (f64::NAN, 5.0), (f64::INFINITY, 1.0), (1e300, 1e300)] {
            let name = u16s(prst);
            assert_no_leak(|| black_box(geom::preset_path_local(&name, w, h, &[])));
            assert_no_leak(|| black_box(geom::preset_path(&name, 1.0, 2.0, w, h, &[], ShapeXf { flip_h: true, flip_v: false, rot: 33.0 })));
        }
    }
    assert_no_leak(|| {
        let mut out = Vec::new();
        black_box(geom::arc_to(&mut out, (1.0, 2.0), 3.0, 4.0, 10.0, 270.0));
        out
    });
    // a sweep over the piece cap emits nothing; a NaN sweep emits nothing
    for sw in [1e300, f64::NAN, f64::INFINITY, 16384.0 * 90.0 + 1.0e6] {
        assert_no_leak(|| {
            let mut o = Vec::new();
            black_box(geom::arc_to(&mut o, (0.0, 0.0), 1.0, 1.0, 0.0, sw));
            o
        });
    }
    let c = cust();
    assert_no_leak(|| black_box(geom::cust_paths(&c, 0.0, 0.0, 200.0, 100.0, ShapeXf::default())));
    assert_no_leak(|| black_box(geom::cust_paths(&[], 0.0, 0.0, 200.0, 100.0, ShapeXf::default())));
    let cmds = geom::preset_path_local(&u16s("roundRect"), 100.0, 50.0, &[]);
    assert_no_leak(|| {
        black_box((geom::rotate_path(&cmds, 1.0, 2.0, 30.0), geom::path_bounds(&cmds), geom::map_path(&cmds, |x, y| (y, x))))
    });
    for unit in [LineUnit::CanvasTwips, LineUnit::PdfPoints] {
        assert_no_leak(|| black_box(geom::line_path(unit, [1.0, 2.0, 3.0, 4.0], Some([0.0, 1.0, 2.0, 3.0]), true, true)));
        assert_no_leak(|| black_box(geom::line_path(unit, [1.0, 2.0, 3.0, 4.0], None, true, false)));
    }
    assert_no_leak(|| black_box(geom::is_open_preset(&u16s("curvedConnector3"))));
    // shape parts: preset, line, custom
    for (name, with_cust) in [("rect", false), ("star5", false), ("straightConnector1", false), ("custom", true), ("custom", false)] {
        let mut res = Resources::new();
        let sg = ShapeGeom {
            geom: res.symbols.intern_str(name).unwrap(),
            adj: Some(vec![(res.symbols.intern_str("adj").unwrap(), 12.5)]),
            cust: with_cust.then(|| res.add_cust(Arc::from(cust())).unwrap()),
            flip_h: true,
            flip_v: false,
            rot: OptF64::some(20.0),
            fill_alpha: OptF64::NONE,
            line_alpha: OptF64::NONE,
            eval: GeomEval::Compact,
        };
        for unit in [LineUnit::CanvasTwips, LineUnit::PdfPoints] {
            assert_no_leak(|| black_box(geom::shape_parts(unit, &sg, &res, [0.0, 0.0, 300.0, 200.0], Some([0.0, 0.0, 10.0, 10.0]))));
        }
    }
}

#[test]
fn m1_fill() {
    let mut syms = SymbolTable::new();
    let g = grad(&mut syms, None);
    let gp = grad(&mut syms, Some("rect"));
    let stops = fill::stops_of(&g, &syms);
    for n in [0usize, 1, 2, 64, 256, 512] {
        assert_no_leak(|| black_box(fill::gradient_samples(&stops, n, &L)));
        assert_no_leak(|| black_box(fill::gradient_samples(&[], n, &L)));
    }
    let two: Vec<(f64, &[u16])> = vec![(0.0, &[0x46, 0x46, 0x30, 0x30, 0x30, 0x30]), (1.0, &[0x30; 6])];
    assert_no_leak(|| black_box(fill::gradient_samples(&two, 512, &L)));
    // error: n over max_samples allocates nothing
    let small = PaintLimits { max_samples: 100, ..L };
    let (r, u) = measure(|| fill::gradient_samples(&stops, 101, &small));
    assert_eq!(r, Err(PaintError::Limit(LimitKind::Samples)));
    assert_eq!(u.allocated, 0);
    drop(r);
    for g in [&g, &gp] {
        assert_no_leak(|| {
            black_box(fill::gradient_geometry(g, &syms, GradBox { x: 0.0, y: 0.0, w: 100.0, h: 50.0, rot: 30.0 }, (0.0, 0.0, 100.0, 50.0)))
        });
    }
    // pattern raster: ok, empty, unknown, errors (the checks come before the allocation)
    assert_no_leak(|| {
        black_box(fill::pattern_raster(&u16s("pct50"), &u16s("000000"), &u16s("FFFFFF"), 0.0, 0.0, 612.0, 792.0, 0.75, 4.0, &L))
    });
    assert_no_leak(|| black_box(fill::pattern_raster(&u16s("nope"), &[], &[], 0.0, 0.0, 612.0, 792.0, 0.75, 4.0, &L)));
    assert_no_leak(|| black_box(fill::pattern_raster(&u16s("pct50"), &[], &[], 0.0, 0.0, 0.0, 792.0, 0.75, 4.0, &L)));
    for (w, px, k, want) in [
        (1e9, 0.75, 4.0, PaintError::Limit(LimitKind::RasterPx)),
        (100.0, 0.0, 1.0, PaintError::Invalid),
        (101.0, 1.0, 0.5, PaintError::Invalid),
        (100.0, f64::NAN, 1.0, PaintError::Invalid),
    ] {
        let (r, u) = measure(|| fill::pattern_raster(&u16s("pct50"), &[], &[], 0.0, 0.0, w, w, px, k, &L));
        assert_eq!(r.as_ref().err(), Some(&want), "w {w} px {px} k {k}");
        // (the preset-name vector of the call is the only allocation)
        assert!(u.allocated < 256, "{} bytes", u.allocated);
    }
    // tiles
    let mut s2 = SymbolTable::new();
    let tile = BlipTile {
        tx: 0.0,
        ty: 0.0,
        sx: 100_000.0,
        sy: 100_000.0,
        algn: s2.intern_str("ctr").unwrap(),
        flip: s2.intern_str("none").unwrap(),
    };
    assert_no_leak(|| black_box(fill::blip_tiles(Some(&tile), &s2, 0.0, 0.0, 12000.0, 15000.0, 1.0 / 635.0, Some((16.0, 16.0)), &L)));
    assert_no_leak(|| black_box(fill::blip_tiles(None, &s2, 0.0, 0.0, 12000.0, 15000.0, 1.0, None, &L)));
    let tight = PaintLimits { max_tiles: 10, ..L };
    assert_no_leak(|| black_box(fill::blip_tiles(Some(&tile), &s2, 0.0, 0.0, 12000.0, 15000.0, 1.0 / 12700.0, Some((1.0, 1.0)), &tight)));
    // rect gradient
    assert_no_leak(|| black_box(fill::rect_gradient_raster(&gp, &syms, 100.0, 60.0, 2.0, &L)));
    assert_no_leak(|| black_box(fill::rect_gradient_raster(&gp, &syms, 0.0, 0.0, 2.0, &L)));
    assert_no_leak(|| black_box(fill::rect_gradient_raster(&gp, &syms, f64::NAN, 1.0, 2.0, &L)));
    assert_no_leak(|| black_box(fill::is_rect_gradient(Some(&gp), &syms)));
}

#[test]
fn m1_page_fill() {
    let mut syms = SymbolTable::new();
    for kind in [PageFillKind::Gradient, PageFillKind::GradientRadial, PageFillKind::Pattern, PageFillKind::Frame, PageFillKind::Tile] {
        let fl = page_fill(&mut syms, kind);
        assert_no_leak(|| {
            black_box((pf::fill_param(&fl, 0.3, 0.6, false), pf::gradient_tile_px(&fl, true), pf::page_fill_tile_tw(&fl, 2.4, 20.0, true)))
        });
        assert_no_leak(|| black_box(pf::pattern_rgba(&fl, &syms)));
        for (w, h) in [(1, 1), (64, 48), (0, 5), (5, 0), (257, 3)] {
            for pdf in [false, true] {
                assert_no_leak(|| black_box(pf::gradient_tile_rgba(&fl, &syms, w, h, pdf, &L)));
            }
        }
    }
    let fl = page_fill(&mut syms, PageFillKind::Gradient);
    let small = PaintLimits { max_raster_px: 100, ..L };
    let (r, u) = measure(|| pf::gradient_tile_rgba(&fl, &syms, 11, 10, false, &small));
    assert_eq!(r, Err(PaintError::Limit(LimitKind::RasterPx)));
    assert_eq!(u.allocated, 0);
}

#[test]
fn m1_text_fx_and_picture() {
    let face = face();
    let c = cols("Hello, fi World");
    let mut syms = SymbolTable::new();
    let fx = fx_all(&mut syms);
    let hex = u16s("000000");
    // warm-up: the face keeps the outlines it has read (an outline cache, bounded by the face)
    black_box(tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, 2.0, &hex, &L)).unwrap();
    assert_no_leak(|| black_box(tx::fx_margin_pt(&fx, 12.0)));
    assert_no_leak(|| black_box(tx::run_box(&run(&face, &c, 24.0))));
    assert_no_leak(|| black_box(tx::run_outline_segs(&run(&face, &c, 24.0))));
    assert_no_leak(|| black_box(tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, 2.0, &hex, &L)));
    assert_no_leak(|| black_box(tx::fx_layers(&run(&face, &c, 24.0), &ResolvedTextFx::default(), &syms, 2.0, &hex, &L)));
    let empty = cols("");
    assert_no_leak(|| black_box(tx::fx_layers(&run(&face, &empty, 24.0), &fx, &syms, 2.0, &hex, &L)));
    // the layer over 2.4e7 px is refused before any mask
    let (r, u) = measure(|| tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, 1000.0, &hex, &L));
    assert!(matches!(r, Ok(None)));
    assert!(u.allocated < 1 << 16, "{} bytes before the size check", u.allocated);
    // a mask limit error releases what it built
    let tight = PaintLimits { draw: uo_drawingml::Limits { max_mask_pixels: 100, ..L.draw }, ..L };
    assert_no_leak(|| black_box(tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, 2.0, &hex, &tight)));
    // dilate
    let m = Mask::new(40, 30, &L.draw).unwrap();
    for r in [0.0, 0.5, 2.2, 12.0, -1.0, f64::NAN] {
        assert_no_leak(|| black_box(tx::dilate(&m, r, &L)));
    }
    for (r, lim) in [
        (f64::INFINITY, L),
        (1e9, L),
        (12.0, PaintLimits { max_dilate_work: 10, ..L }),
        (12.0, PaintLimits { max_dilate_offsets: 10, ..L }),
    ] {
        let (res, u) = measure(|| tx::dilate(&m, r, &lim));
        assert_eq!(res.unwrap_err(), PaintError::Limit(LimitKind::DilateWork));
        assert!(u.allocated < 1 << 20, "{} bytes (the offsets of the disc)", u.allocated);
    }
    assert_no_leak(|| black_box(tx::dash_pattern(Some(&u16s("lgDashDot")))));
    assert_no_leak(|| black_box(tx::grad_of(&ResolvedFill::None)));
    // picture
    let mut ps = SymbolTable::new();
    let look = PictureLook::default();
    for painter in [Painter::Canvas, Painter::Pdf] {
        assert_no_leak(|| black_box(picture::effect_bounds(painter, Some(&look), 100.0, 50.0)));
    }
    let preset = uo_render::types::PresetSpec { prst: ps.intern_str("blockArc").unwrap(), adj: None };
    for (fh, fv) in [(false, false), (true, false), (true, true)] {
        assert_no_leak(|| black_box(picture::shape_subs(Some(&preset), &ps, 120.0, 80.0, fh, fv)));
    }
    assert_no_leak(|| black_box(picture::shape_subs(None, &ps, 120.0, 80.0, true, true)));
    let mask = Mask::from_vec(4, 3, vec![0.5; 12]).unwrap();
    for store in [ChannelStore::Clamp, ChannelStore::Wrap] {
        assert_no_leak(|| black_box(picture::colored_mask(&mask, &u16s("12AB34"), 0.7, store)));
    }
    assert_no_leak(|| black_box(picture::dash(&u16s("dash"))));
}

/// The numbers of the module record (M3): bytes allocated and peak above the start for the
/// biggest rasters, and the budgets they must stay under (the output plus a small factor).
#[test]
fn m3_budgets() {
    let mut syms = SymbolTable::new();
    let gp = grad(&mut syms, Some("rect"));
    // PDF pattern over a Letter page: 816 x 1056 cells x 16 px
    let (r, u) = measure(|| {
        fill::pattern_raster(&u16s("pct50"), &u16s("000000"), &u16s("FFFFFF"), 0.0, 0.0, 612.0, 792.0, 0.75, 4.0, &L).unwrap().unwrap()
    });
    let out = r.data.len() as u64;
    eprintln!("M3 pattern_raster Letter PDF: output {out} B, allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.allocated <= out + 4096 && u.peak_above_start <= i64::try_from(out + 4096).unwrap());
    drop(r);
    let (r, u) = measure(|| fill::rect_gradient_raster(&gp, &syms, 3000.0, 3000.0, 1.0, &L).unwrap());
    let out = r.data.len() as u64;
    eprintln!("M3 rect_gradient_raster 2000x2000: output {out} B, allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.allocated <= out + 8192 && u.peak_above_start <= i64::try_from(out + 8192).unwrap());
    drop(r);
    let fl = page_fill(&mut syms, PageFillKind::Gradient);
    let (r, u) = measure(|| pf::gradient_tile_rgba(&fl, &syms, 1024, 768, false, &L).unwrap());
    let out = r.len() as u64;
    eprintln!("M3 gradient_tile_rgba 1024x768: output {out} B, allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.allocated <= out + 8192 && u.peak_above_start <= i64::try_from(out + 8192).unwrap());
    drop(r);
    let face = face();
    let c = cols("Hello, fi World");
    let fx = fx_all(&mut syms);
    let hex = u16s("000000");
    black_box(tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, 2.78, &hex, &L)).unwrap();
    for ppt in [2.78, 8.33] {
        let (r, u) = measure(|| tx::fx_layers(&run(&face, &c, 24.0), &fx, &syms, ppt, &hex, &L).unwrap().unwrap());
        let px = (r.w * r.h) as u64;
        eprintln!(
            "M3 fx_layers 24pt run at {ppt} px/pt: {}x{} = {px} px, allocated {} B ({} B/px), peak {} B ({} B/px)",
            r.w,
            r.h,
            u.allocated,
            u.allocated / px,
            u.peak_above_start,
            u64::try_from(u.peak_above_start).unwrap() / px
        );
        // the float RGBA layer (16 B/px), the silhouette and the effect masks (4 B/px each), the output (4 B/px)
        assert!(u.peak_above_start <= i64::try_from(px * 64 + (1 << 20)).unwrap());
    }
    let m = Mask::new(1000, 1000, &L.draw).unwrap();
    let (r, u) = measure(|| tx::dilate(&m, 6.0, &L).unwrap());
    eprintln!("M3 dilate 1000x1000 r6: allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.allocated <= 4_000_000 + (1 << 16));
    drop(r);
    let _ = COLORS;
    let _ = VALID;
}

/// M2: the helpers keep no state: a soak of the whole pipeline is flat with a tolerance of 0 B.
#[test]
fn m2_soak() {
    let face = face();
    let c = cols("Hello, fi World");
    let mut syms = SymbolTable::new();
    let fx = fx_all(&mut syms);
    let g = grad(&mut syms, None);
    let gp = grad(&mut syms, Some("rect"));
    let fl = page_fill(&mut syms, PageFillKind::GradientRadial);
    let cu = cust();
    let hex = u16s("000000");
    let iterations: usize =
        std::env::var("UO_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 6 } else { 100 });
    // warm-up outside the series (the face's outline cache)
    black_box(tx::fx_layers(&run(&face, &c, 18.0), &fx, &syms, 2.0, &hex, &L)).unwrap();
    let series = soak(iterations, 1, |_| {
        for prst in ["roundRect", "hexagon", "rightArrow", "frame"] {
            black_box(geom::preset_path(&u16s(prst), 0.0, 0.0, 1440.0, 720.0, &[], ShapeXf::default()));
        }
        black_box(geom::cust_paths(&cu, 0.0, 0.0, 200.0, 100.0, ShapeXf::default()));
        black_box(fill::gradient_samples(&fill::stops_of(&g, &syms), 512, &L)).unwrap();
        black_box(fill::pattern_raster(&u16s("pct50"), &u16s("000000"), &u16s("FFFFFF"), 0.0, 0.0, 200.0, 200.0, 0.75, 4.0, &L)).unwrap();
        black_box(fill::rect_gradient_raster(&gp, &syms, 100.0, 100.0, 2.0, &L)).unwrap();
        black_box(pf::gradient_tile_rgba(&fl, &syms, 128, 96, true, &L)).unwrap();
        black_box(tx::fx_layers(&run(&face, &c, 18.0), &fx, &syms, 2.0, &hex, &L)).unwrap();
        black_box(picture::shape_subs(None, &syms, 100.0, 100.0, true, false));
    });
    assert_flat(&series, 1, 0);
}
