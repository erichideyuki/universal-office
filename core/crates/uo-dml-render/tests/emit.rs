//! Frontier B4 tests of the shape emission (`uo_dml_render::emit`) that need no deck: the memory gate (M1 on
//! `emit_shapes`/`emit_slide`, M5 on the pattern and path-gradient caches), the item structure of the TS call
//! sequence (`save`/`restore` per shape, layers with their fallback block) and bounded no-panic over random shape
//! lists (`UO_DML_LONG=1` runs 20x longer). The exact parity with the frozen TS is `core/crates/uo-pptx/js/emit-uolf.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_dml_render::emit::{EmitCaches, EmitParams, Env, MAX_PATH_GRADIENT_CACHE, MAX_PATTERN_CACHE, SlideBg, emit_shapes, emit_slide};
use uo_dml_render::ids::MediaId;
use uo_dml_render::model::{Fill, FillKind, GradientStop, Paragraph, PatternFill, Reflection, Run, Shape, ShapeFx, TextBody, Xfrm};
use uo_dml_render::source::{PartSource, R};
use uo_dml_render::text::{MeasureReq, MeasureTable};
use uo_dml_render::{CssColor, ReadError};
use uo_render::slides::{SItem, SlidesEncoder, SlidesResources, validate};

/// A package without media.
struct NoParts;

impl PartSource for NoParts {
    fn part(&self, _: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, _: &str) -> R<Option<Arc<uo_opc::xml::Document>>> {
        Err(ReadError::Missing("x".into()))
    }
    fn media(&self, _: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

fn params() -> EmitParams {
    EmitParams { scale: 0.0001, ox: 0.0, oy: 0.0, slide_w: 9_144_000.0, slide_h: 6_858_000.0, ..EmitParams::default() }
}

fn rect(x: f64) -> Shape {
    Shape {
        id: "1".into(),
        name: "r".into(),
        xfrm: Some(Xfrm { x, y: 100_000.0, cx: 2_000_000.0, cy: 1_000_000.0, ..Xfrm::default() }),
        ..Shape::default()
    }
}

fn solid(c: &str) -> Fill {
    let mut f = Fill::of_kind(FillKind::Solid);
    f.color = Some(CssColor::new(c));
    f
}

fn pattern_shape(i: u32) -> Shape {
    let mut f = solid("#808080");
    f.pattern = Some(Box::new(PatternFill {
        prst: "ltUpDiag".into(),
        fg: CssColor::new(&format!("#{:06X}", i & 0xFF_FFFF)),
        bg: CssColor::new("#FFFFFF"),
        fg_a: None,
        bg_a: None,
    }));
    Shape { fill: Some(Box::new(f)), ..rect(f64::from(i)) }
}

fn path_gradient_shape(i: u32) -> Shape {
    let mut f = Fill::of_kind(FillKind::Gradient);
    f.path = Some("circle".into());
    f.stops = Some(
        vec![
            GradientStop { pos: 0.0, color: CssColor::new("#FF0000"), alpha: None },
            GradientStop { pos: 1.0, color: CssColor::new(&format!("#00{:04X}", i & 0xFFFF)), alpha: None },
        ]
        .into(),
    );
    Shape { fill: Some(Box::new(f)), ..rect(f64::from(i)) }
}

fn answer(t: &mut MeasureTable, need: &[MeasureReq]) {
    for r in need {
        match r {
            MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, 500.0),
            MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, 1000.0, 990.0),
            MeasureReq::Width(k) => t.set_width(k, k.size_px.value() * 0.5 * k.text.chars().count() as f64),
            MeasureReq::ImageSize { media } => t.set_image_size(*media, 10.0, 10.0),
            _ => {}
        }
    }
}

fn shapes_list(shapes: &[Shape], res: &mut SlidesResources, caches: &mut EmitCaches) -> uo_render::slides::SlidesList {
    let mut table = MeasureTable::default();
    let src = NoParts;
    loop {
        let env = Env { src: &src, table: &table, rules: uo_dml_render::SlidesRules::ts_strict() };
        match emit_shapes(shapes, &params(), &env, res, caches) {
            Ok(e) => return e.list,
            Err(n) => answer(&mut table, &n.0),
        }
    }
}

#[test]
fn each_shape_is_one_save_restore_pair() {
    let shapes = vec![rect(0.0), rect(1_000_000.0), Shape { fill: Some(Box::new(solid("#336699"))), ..rect(2_000_000.0) }];
    let mut res = SlidesResources::new();
    let l = shapes_list(&shapes, &mut res, &mut EmitCaches::new());
    let saves = l.items.iter().filter(|i| matches!(i, SItem::Save)).count();
    let restores = l.items.iter().filter(|i| matches!(i, SItem::Restore)).count();
    assert_eq!(saves, restores, "balanced");
    // the host-state reset of `paintShapesTo` plus one pair per shape (the filled one has its body's own pairs)
    assert!(saves > shapes.len());
    validate(&l, &res.lens()).unwrap();
}

#[test]
fn a_reflection_is_a_layer_with_its_fallback_block() {
    let mut s = rect(0.0);
    s.fill = Some(Box::new(solid("#336699")));
    s.fx_mut().reflection = Some(Reflection {
        blur_rad: 1.0,
        st_a: 0.5,
        end_a: 0.0,
        st_pos: 0.0,
        end_pos: 0.5,
        dist: 0.0,
        dir_deg: 90.0,
        sy: -1.0,
        fade_dir_deg: 90.0,
    });
    let mut res = SlidesResources::new();
    let l = shapes_list(&[s], &mut res, &mut EmitCaches::new());
    let begin = l.items.iter().position(|i| matches!(i, SItem::LayerBegin(_))).expect("a layer");
    let SItem::LayerBegin(b) = &l.items[begin] else { unreachable!() };
    assert!(usize::try_from(b.fallback_end).unwrap() > begin && usize::try_from(b.fallback_end).unwrap() <= l.items.len());
    assert!(l.items.iter().any(|i| matches!(i, SItem::DrawLayer(_))), "the reflection draws the shape's layer");
    assert!(l.items.iter().any(|i| matches!(i, SItem::TransformByInverseOf(0))));
    validate(&l, &res.lens()).unwrap();
}

#[test]
fn m5_the_pattern_cache_is_capped() {
    let n = u32::try_from(MAX_PATTERN_CACHE).unwrap() + 40;
    let shapes: Vec<Shape> = (0..n).map(pattern_shape).collect();
    let mut res = SlidesResources::new();
    let mut caches = EmitCaches::new();
    let l = shapes_list(&shapes, &mut res, &mut caches);
    let (patt, _, _) = caches.sizes();
    assert!(patt <= MAX_PATTERN_CACHE, "{patt}");
    assert!(caches.overflowed, "past the cap the host is told to start a new generation");
    assert_eq!(l.patterns.len(), shapes.len());
    // a second emission of the same shapes reuses the cached bitmaps (no new table entries for the entries still cached)
    let before = res.lens()[4];
    let _ = shapes_list(&shapes[..10], &mut res, &mut EmitCaches::new());
    assert!(res.lens()[4] >= before);
    caches.clear();
    assert_eq!(caches.sizes(), (0, 0, 0));
}

#[test]
fn m5_the_path_gradient_cache_is_capped() {
    let n = u32::try_from(MAX_PATH_GRADIENT_CACHE).unwrap() + 20;
    let shapes: Vec<Shape> = (0..n).map(path_gradient_shape).collect();
    let mut res = SlidesResources::new();
    let mut caches = EmitCaches::new();
    let _ = shapes_list(&shapes, &mut res, &mut caches);
    let (_, grads, bytes) = caches.sizes();
    assert!(grads <= MAX_PATH_GRADIENT_CACHE + 1, "{grads}");
    assert!(bytes <= uo_dml_render::emit::MAX_PATH_GRADIENT_BYTES);
    assert!(caches.overflowed);
}

#[test]
fn m1_emission_returns_live_bytes() {
    let mut text = rect(0.0);
    text.tx_body = Some(Box::new(TextBody {
        paragraphs: vec![Paragraph { runs: vec![Run { text: "Hello caches".into(), ..Run::default() }], ..Paragraph::default() }],
        ..TextBody::default()
    }));
    let shapes = vec![text, pattern_shape(1), path_gradient_shape(2)];
    // warm up the lazily built tables of the engines
    let _ = shapes_list(&shapes, &mut SlidesResources::new(), &mut EmitCaches::new());
    assert_no_leak(|| {
        let mut res = SlidesResources::new();
        let mut caches = EmitCaches::new();
        let l = shapes_list(&shapes, &mut res, &mut caches);
        let mut out = Vec::new();
        SlidesEncoder::new(1).encode(&l, &res, &mut out).unwrap();
        // the need-measures path
        let table = MeasureTable::default();
        let src = NoParts;
        let env = Env { src: &src, table: &table, rules: uo_dml_render::SlidesRules::ts_strict() };
        assert!(emit_shapes(&shapes, &params(), &env, &mut res, &mut caches).is_err());
        assert!(emit_slide(&SlideBg::default(), &shapes, &params(), &env, &mut res, &mut caches).is_err());
        caches.clear();
    });
}

// ------------------------------------------------------------------------------------------------ no panic

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::try_from(self.next() % n as u64).unwrap_or(0) }
    }
    fn num(&mut self) -> f64 {
        [0.0, -0.0, 1.0, -1.0, 0.5, 1e3, 12_700.0, 914_400.0, 1e9, 1e300, -1e300, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-300]
            [self.below(15)]
    }
}

fn random_shape(r: &mut Rng) -> Shape {
    let geoms = [
        "rect",
        "ellipse",
        "roundRect",
        "triangle",
        "star5",
        "can",
        "cube",
        "line",
        "straightConnector1",
        "pie",
        "arc",
        "bogus",
        "wedgeRectCallout",
        "cloud",
        "",
    ];
    let mut s = Shape {
        xfrm: if r.below(10) == 0 {
            None
        } else {
            Some(Xfrm {
                x: r.num(),
                y: r.num(),
                cx: r.num(),
                cy: r.num(),
                rot: if r.below(3) == 0 { Some(r.num()) } else { None },
                flip_h: Some(r.below(2) == 0),
                flip_v: Some(r.below(2) == 0),
            })
        },
        geom: Some(geoms[r.below(geoms.len())].into()),
        ..rect(0.0)
    };
    // the same draws in the same order as the flat literal before the compaction
    let adj: Option<Box<[f64]>> = if r.below(3) == 0 { Some(vec![r.num(), r.num()].into()) } else { None };
    s.line_color = if r.below(2) == 0 { Some(CssColor::new("#112233")) } else { None };
    s.line_width = if r.below(2) == 0 { Some(r.num()) } else { None };
    let line_dash: Option<Box<str>> = if r.below(4) == 0 { Some("sysDash".into()) } else { None };
    let alpha = if r.below(4) == 0 { Some(r.num()) } else { None };
    if adj.is_some() || line_dash.is_some() {
        s.x_mut().adj = adj;
        s.x_mut().line_dash = line_dash;
    }
    if alpha.is_some() {
        s.paint_mut().alpha = alpha;
    }
    match r.below(5) {
        0 => s.fill = Some(Box::new(solid("#AABBCC"))),
        1 => s.fill = Some(pattern_shape(r.below(1000) as u32).fill.unwrap()),
        2 => s.fill = path_gradient_shape(r.below(10) as u32).fill,
        3 => {
            let mut f = Fill::of_kind(FillKind::Gradient);
            f.stops = Some(
                vec![
                    GradientStop { pos: r.num(), color: CssColor::new("#102030"), alpha: Some(r.num()) },
                    GradientStop { pos: r.num(), color: CssColor::new("#zz"), alpha: None },
                ]
                .into(),
            );
            f.angle_deg = Some(r.num());
            f.scaled = Some(r.below(2) == 0);
            s.fill = Some(Box::new(f));
        }
        _ => {}
    }
    if r.below(2) == 0 {
        s.tx_body = Some(Box::new(TextBody {
            paragraphs: vec![Paragraph {
                runs: vec![Run { text: "Ab \u{1F600}\u{0301} é".into(), sz: Some(r.num()), ..Run::default() }],
                level: Some(r.num()),
                ..Paragraph::default()
            }],
            ..TextBody::default()
        }));
        let vert: Option<Box<str>> = if r.below(4) == 0 { Some("vert270".into()) } else { None };
        let rot = if r.below(4) == 0 { Some(r.num()) } else { None };
        if let Some(tb) = s.tx_body.as_deref_mut()
            && (vert.is_some() || rot.is_some())
        {
            tb.x_mut().vert = vert;
            tb.x_mut().rot = rot;
        }
    }
    if r.below(4) == 0 {
        let reflection = Some(Reflection {
            blur_rad: r.num(),
            st_a: r.num(),
            end_a: r.num(),
            st_pos: r.num(),
            end_pos: r.num(),
            dist: r.num(),
            dir_deg: 90.0,
            sy: r.num(),
            fade_dir_deg: 90.0,
        });
        // merged into the shape's extras (the adj/line_dash/alpha drawn above live there now)
        s.x_mut().fx = Some(Box::new(ShapeFx { reflection, soft_edge: Some(r.num()), ..ShapeFx::default() }));
    }
    s
}

#[test]
fn random_shape_lists_emit_without_panic() {
    let long = std::env::var_os("UO_DML_LONG").is_some();
    let lists = if long { 2_000 } else { 100 };
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let t0 = std::time::Instant::now();
    for _ in 0..lists {
        let shapes: Vec<Shape> = (0..1 + r.below(8)).map(|_| random_shape(&mut r)).collect();
        let mut res = SlidesResources::new();
        let mut caches = EmitCaches::new();
        let mut table = MeasureTable::default();
        let src = NoParts;
        let p =
            EmitParams { scale: r.num(), ox: r.num(), oy: r.num(), slide_w: 9_144_000.0, slide_h: 6_858_000.0, ..EmitParams::default() };
        let mut done = false;
        for _ in 0..=uo_dml_render::text::MAX_ROUNDS {
            let env = Env { src: &src, table: &table, rules: uo_dml_render::SlidesRules::ts_strict() };
            match emit_slide(&SlideBg::default(), &shapes, &p, &env, &mut res, &mut caches) {
                Ok(e) => {
                    // bounded by the input: a shape makes a bounded number of items (the text of one shape one run per word)
                    assert!(e.list.items.len() < 200 * shapes.len() + 100, "{} items for {} shapes", e.list.items.len(), shapes.len());
                    validate(&e.list, &res.lens()).expect("a valid list");
                    done = true;
                    break;
                }
                Err(n) => answer(&mut table, &n.0),
            }
        }
        assert!(done, "the rounds converge");
    }
    assert!(t0.elapsed().as_secs() < 60);
}
