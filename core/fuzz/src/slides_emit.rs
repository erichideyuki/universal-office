//! Fuzz target of the Slides emission (`uo-dml-render::emit`, frontier B4; record `core/docs/modules/uo-dml-render.md`
//! "B4 — render").
//!
//! `slides_emit`: structure-aware. The bytes drive a list of shapes (hostile transforms with NaN/±∞/negative/huge numbers,
//! every kind of fill, outlines, dashes, text with vertical/rotated bodies, reflections and soft edges that make
//! layers, glow and shadows, tables of random size) and a hostile `MeasureTable`, then the two-phase emission of a
//! slide at a hostile viewport.
//!
//! Invariants beyond "no panic": a list the emitter built always passes `validate` and encodes, an emission is a
//! pure function of its inputs (two runs with fresh resources are equal), `save` and `restore` balance, the item
//! count is bounded by the input, and the rounds converge or the call keeps asking.
//!
//! B4b: the same shapes are emitted again as a playback list with a hostile `AnimPlan` (every hook: background,
//! paragraph ranges, letters/words with ranges, soft reveals, media frames) and each shape re-emitted with hostile states
//! (colours, font weight/style/family, underline, NaN/huge font scales): the list validates and encodes, `SlotBegin` and
//! `SlotEnd` balance, `Emission::slots` matches `list.slots`, and two runs are equal.
use std::sync::Arc;

use uo_dml_render::emit::{
    AnimPlan, EmitCaches, EmitParams, EmitSlot, Env, PartStyle, ShapeAnim, SlideBg, SlotStates, UnitHook, emit_shape_anim, emit_slide,
    emit_slide_anim,
};
use uo_dml_render::ids::MediaId;
use uo_dml_render::model::{
    ChartRef, Fill, FillKind, GradientStop, Paragraph, PatternFill, Reflection, Run, Shape, ShapeExtras, Table, TableCell, TableRow,
    TextBody, Xfrm,
};
use uo_dml_render::source::{PartSource, R};
use uo_dml_render::text::{MAX_ROUNDS, MeasureReq, MeasureTable, TextMetrics};
use uo_dml_render::{CssColor, ReadError, Theme};
use uo_opc::xml::{Document, ParseOptions, parse};
use uo_render::slides::{SItem, SlidesEncoder, SlidesResources, validate};

/// The chart part of the shapes with a chart frame (C-6: the emission paints it through `uo-charts`): a titled bar chart with a
/// legend, two series and an axis pair, so the hostile viewport and the hostile measure answers reach the engine's layout.
const CHART: &str = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Title of the chart</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>First</c:v></c:tx><c:cat><c:strLit><c:ptCount val="3"/><c:pt idx="0"><c:v>Alpha</c:v></c:pt><c:pt idx="1"><c:v>Bravo</c:v></c:pt><c:pt idx="2"><c:v>Charlie</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="1"><c:v>9</c:v></c:pt><c:pt idx="2"><c:v>5</c:v></c:pt></c:numLit></c:val></c:ser><c:ser><c:idx val="1"/><c:order val="1"/><c:tx><c:v>Second</c:v></c:tx><c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>4</c:v></c:pt><c:pt idx="2"><c:v>7</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:crossAx val="1"/><c:majorGridlines/></c:valAx></c:plotArea><c:legend><c:legendPos val="r"/></c:legend></c:chart></c:chartSpace>"#;

/// A package with no media.
struct EmptySource;

impl PartSource for EmptySource {
    fn part(&self, _name: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        if name == "ppt/charts/chart1.xml" {
            return Ok(parse(CHART.to_owned(), &ParseOptions::LENIENT).ok().map(Arc::new));
        }
        Err(ReadError::Missing("x".into()))
    }
    fn media(&self, _part: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

struct Bytes<'a> {
    d: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        let b = self.d.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        b
    }
    fn below(&mut self, n: usize) -> usize {
        usize::from(self.u8()) % n.max(1)
    }
    fn flag(&mut self) -> bool {
        self.u8() & 1 == 1
    }
    fn num(&mut self, scale: f64) -> f64 {
        match self.u8() {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => -scale * 3.0,
            4 => 1e300,
            5 => -0.0,
            b => f64::from(b) * scale / 64.0,
        }
    }
    fn opt(&mut self, scale: f64) -> Option<f64> {
        if self.flag() { Some(self.num(scale)) } else { None }
    }
    fn pick<'s>(&mut self, v: &[&'s str]) -> &'s str {
        v[self.below(v.len())]
    }
    fn color(&mut self) -> CssColor {
        let v = ["#112233", "#FFFFFF", "#zz", "#abc", "rgb(1,2,3)", "", "#00000"];
        CssColor::new(v[self.below(v.len())])
    }
}

fn fill(b: &mut Bytes<'_>) -> Fill {
    let mut f = Fill::of_kind(match b.below(3) {
        0 => FillKind::Solid,
        1 => FillKind::None,
        _ => FillKind::Gradient,
    });
    if b.flag() {
        f.color = Some(b.color());
    }
    if f.kind == FillKind::Gradient || b.flag() {
        f.stops =
            Some((0..b.below(5)).map(|_| GradientStop { pos: b.num(1.0), color: b.color(), alpha: b.opt(1.0) }).collect::<Vec<_>>().into());
        f.angle_deg = b.opt(360.0);
        f.scaled = b.flag().then(|| b.flag());
        if b.flag() {
            f.path = Some(b.pick(&["circle", "rect", "shape", ""]).into());
        }
    }
    if b.flag() {
        f.pattern = Some(Box::new(PatternFill {
            prst: b.pick(&["ltUpDiag", "pct50", "constructor", "", "cross"]).into(),
            fg: b.color(),
            bg: b.color(),
            fg_a: b.opt(1.0),
            bg_a: b.opt(1.0),
        }));
    }
    f
}

fn shape(b: &mut Bytes<'_>) -> Shape {
    let geoms =
        ["rect", "ellipse", "roundRect", "triangle", "star5", "can", "cube", "line", "bentConnector3", "pie", "arc", "bogus", "cloud", ""];
    let mut s = Shape {
        id: "1".into(),
        name: "s".into(),
        xfrm: b.flag().then(|| Xfrm {
            x: b.num(9_000_000.0),
            y: b.num(5_000_000.0),
            cx: b.num(4_000_000.0),
            cy: b.num(3_000_000.0),
            rot: b.opt(21_600_000.0),
            flip_h: b.flag().then(|| b.flag()),
            flip_v: b.flag().then(|| b.flag()),
        }),
        geom: b.flag().then(|| geoms[b.below(geoms.len())].into()),
        adj: b.flag().then(|| (0..b.below(4)).map(|_| b.num(100_000.0)).collect::<Vec<_>>().into()),
        fill: b.flag().then(|| Box::new(fill(b))),
        line_color: b.flag().then(|| b.color()),
        line_width: b.opt(100_000.0),
        line_alpha: b.opt(1.0),
        line_dash: b.flag().then(|| b.pick(&["dash", "sysDot", "bogus", ""]).into()),
        alpha: b.opt(1.0),
        fill_alpha: b.opt(1.0),
        use_bg_fill: b.flag().then(|| b.flag()),
        fill_no_rot: b.flag().then(|| b.flag()),
        ..Shape::default()
    };
    if b.flag() {
        s.tx_body = Some(Box::new(TextBody {
            paragraphs: (0..1 + b.below(3))
                .map(|_| Paragraph {
                    runs: (0..1 + b.below(3))
                        .map(|_| Run {
                            text: ["Hello world", "Ab \u{1F600}\u{301} é", "", "long long long long long text", "\t"][b.below(5)].into(),
                            sz: b.opt(60.0),
                            bold: b.flag().then(|| b.flag()),
                            ..Run::default()
                        })
                        .collect(),
                    level: b.opt(8.0),
                    ..Paragraph::default()
                })
                .collect(),
            vert: b.flag().then(|| b.pick(&["vert", "vert270", "eaVert", "wordArtVert"]).into()),
            rot: b.opt(21_600_000.0),
            ..TextBody::default()
        }));
    }
    if b.flag() {
        s.extras = Some(Box::new(ShapeExtras {
            reflection: b.flag().then(|| Reflection {
                blur_rad: b.num(100_000.0),
                st_a: b.num(1.0),
                end_a: b.num(1.0),
                st_pos: b.num(1.0),
                end_pos: b.num(1.0),
                dist: b.num(100_000.0),
                dir_deg: 90.0,
                sy: b.num(1.0),
                fade_dir_deg: 90.0,
            }),
            soft_edge: b.opt(100_000.0),
            chart: b.flag().then(|| ChartRef {
                part: "ppt/charts/chart1.xml".into(),
                theme: Arc::new(Theme::default()),
                images: None,
                format: None,
            }),
            table: b.flag().then(|| {
                let cols = 1 + b.below(4);
                let rows = 1 + b.below(4);
                Box::new(Table {
                    col_widths_emu: (0..cols).map(|_| b.num(1_000_000.0)).collect(),
                    rows: (0..rows)
                        .map(|_| TableRow {
                            height_emu: b.num(300_000.0),
                            cells: (0..cols)
                                .map(|_| TableCell {
                                    grid_span: b.opt(3.0),
                                    row_span: b.opt(3.0),
                                    tx_body: b.flag().then(|| {
                                        Box::new(TextBody {
                                            paragraphs: vec![Paragraph {
                                                runs: vec![Run { text: "cell text".into(), ..Run::default() }],
                                                ..Paragraph::default()
                                            }],
                                            ..TextBody::default()
                                        })
                                    }),
                                    fill: b.flag().then(|| b.color()),
                                    ..TableCell::default()
                                })
                                .collect(),
                        })
                        .collect(),
                    ..Table::default()
                })
            }),
            ..ShapeExtras::default()
        }));
    }
    s
}

fn answer(t: &mut MeasureTable, r: &MeasureReq, b: &mut Bytes<'_>) {
    let w = match b.u8() % 8 {
        0 => f64::NAN,
        1 => -500.0,
        2 => 1e308,
        3 => f64::INFINITY,
        4 => 0.0,
        _ => f64::from(b.u8()) * 7.5,
    };
    match r {
        MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, w),
        MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, w, w * 1.01),
        MeasureReq::Width(k) => t.set_width(k, w),
        // the chart's font box of `'Hg'` (`fontK`): hostile ascent and descent too
        MeasureReq::Metrics(k) => {
            t.set_metrics(
                k,
                TextMetrics {
                    width: w,
                    font_ascent: Some(w),
                    font_descent: (b.u8() & 1 == 0).then_some(w / 3.0),
                    ..TextMetrics::default()
                },
            );
        }
        MeasureReq::ImageSize { media } => t.set_image_size(*media, w, w),
        _ => {}
    }
}

fn emit_once(shapes: &[Shape], p: &EmitParams, table: &MeasureTable) -> Option<Vec<u8>> {
    let src = EmptySource;
    let env = Env { src: &src, table };
    let mut res = SlidesResources::new();
    let mut caches = EmitCaches::new();
    match emit_slide(&SlideBg::default(), shapes, p, &env, &mut res, &mut caches) {
        Ok(e) => {
            assert!(validate(&e.list, &res.lens()).is_ok(), "the emitter built an invalid list");
            let mut out = Vec::new();
            assert!(SlidesEncoder::new(1).encode(&e.list, &res, &mut out).is_ok(), "a valid list always encodes");
            let saves = e.list.items.iter().filter(|i| matches!(i, SItem::Save)).count();
            let restores = e.list.items.iter().filter(|i| matches!(i, SItem::Restore)).count();
            assert!(saves == restores, "unbalanced save/restore: {saves} / {restores}");
            assert!(e.list.items.len() < 2_000 * shapes.len() + 4_000, "{} items", e.list.items.len());
            // compared as wire bytes: the lists hold NaN, which is not equal to itself
            Some(out)
        }
        Err(_) => None,
    }
}

/// A hostile hook structure for one shape.
fn anim_of(b: &mut Bytes<'_>) -> Option<ShapeAnim> {
    let k = b.u8();
    if k & 1 == 0 {
        return None;
    }
    let range = |b: &mut Bytes<'_>| (b.num(3.0), b.num(3.0));
    Some(ShapeAnim {
        bg: k & 2 != 0,
        paras: (0..b.below(3)).map(|_| range(b)).collect(),
        unit: (k & 4 != 0).then(|| UnitHook {
            letters: k & 8 != 0,
            words: k & 16 != 0,
            ranges: (0..1 + b.below(3)).map(|_| if b.u8() & 1 == 0 { None } else { Some(range(b)) }).collect(),
        }),
        chart: None,
        soft: k & 32 != 0,
    })
}

/// Hostile states: a style per slot, a pure function of the slot (the emission stays deterministic).
struct States(u8);

impl SlotStates for States {
    fn style(&mut self, slot: &EmitSlot) -> PartStyle {
        let h = (slot.shape.wrapping_mul(31) ^ u32::from(self.0))
            .wrapping_add(u32::try_from(slot.base.iter().filter(|v| v.is_finite()).count()).unwrap_or(0));
        let pick = |n: u32| h % n;
        PartStyle {
            fill_color: (pick(3) == 0).then(|| CssColor::new("#123456")),
            line_color: (pick(5) == 1).then(|| CssColor::new("zz")),
            text_color: (pick(2) == 0).then(|| CssColor::new("#FF0000")),
            font_weight: (pick(7) < 3).then(|| "bold".into()),
            font_style: (pick(11) < 4).then(|| "".into()),
            underline: (pick(13) < 5).then_some(pick(2) == 0),
            font_scale: match pick(6) {
                0 => Some(f64::NAN),
                1 => Some(1e9),
                2 => Some(0.5),
                3 => Some(-1.0),
                _ => None,
            },
            font_family: (pick(17) < 3).then(|| "Some Family".into()),
        }
    }
}

fn stage_once(shapes: &[Shape], plan: &[Option<ShapeAnim>], p: &EmitParams, table: &MeasureTable, media: bool, seed: u8) -> Vec<Vec<u8>> {
    let src = EmptySource;
    let env = Env { src: &src, table };
    let mut res = SlidesResources::new();
    let mut caches = EmitCaches::new();
    let mut out = Vec::new();
    let mut enc = SlidesEncoder::new(1);
    let mut check = |e: &uo_dml_render::emit::Emission, res: &SlidesResources, out: &mut Vec<Vec<u8>>| {
        assert!(validate(&e.list, &res.lens()).is_ok(), "the emitter built an invalid playback list");
        assert_eq!(e.slots.len(), e.list.slots.len(), "the slots and the wire slots differ");
        let begins = e.list.items.iter().filter(|i| matches!(i, SItem::SlotBegin(_))).count();
        let ends = e.list.items.iter().filter(|i| matches!(i, SItem::SlotEnd)).count();
        assert!(begins == ends, "unbalanced slots: {begins} / {ends}");
        let mut bytes = Vec::new();
        assert!(enc.encode(&e.list, res, &mut bytes).is_ok(), "a valid list always encodes");
        out.push(bytes);
    };
    if let Ok(e) =
        emit_slide_anim(&SlideBg::default(), shapes, p, &env, &mut res, &mut caches, AnimPlan { shapes: plan, media, states: None })
    {
        check(&e, &res, &mut out);
    }
    for i in 0..shapes.len() {
        let mut st = States(seed);
        let plan = AnimPlan { shapes: plan, media, states: Some(&mut st) };
        if let Ok(e) = emit_shape_anim(shapes, u32::try_from(i).unwrap_or(u32::MAX), p, &env, &mut res, &mut caches, plan) {
            check(&e, &res, &mut out);
        }
    }
    out
}

pub fn slides_emit(data: &[u8]) {
    let mut b = Bytes { d: data, at: 0 };
    let sel = b.u8();
    let shapes: Vec<Shape> = (0..1 + b.below(5)).map(|_| shape(&mut b)).collect();
    let mut table = MeasureTable::with_cap(4_096 >> (sel & 3), 1 << 20);
    table.caps.font_kerning = sel & 4 != 0;
    table.caps.segmenter = sel & 8 != 0;
    let p = EmitParams {
        scale: if sel & 16 != 0 { b.num(0.0002) } else { 0.0001 },
        ox: b.num(100.0),
        oy: b.num(100.0),
        slide_w: 9_144_000.0,
        slide_h: 6_858_000.0,
        clip_overflow: sel & 32 != 0,
        suppress_text: if sel & 64 != 0 { Some(u32::from(b.u8() % 6)) } else { None },
    };
    let src = EmptySource;
    for _ in 0..=MAX_ROUNDS {
        let env = Env { src: &src, table: &table };
        let mut res = SlidesResources::new();
        match emit_slide(&SlideBg::default(), &shapes, &p, &env, &mut res, &mut EmitCaches::new()) {
            Ok(_) => break,
            Err(n) => {
                for r in &n.0 {
                    answer(&mut table, r, &mut b);
                }
            }
        }
    }
    let first = emit_once(&shapes, &p, &table);
    let second = emit_once(&shapes, &p, &table);
    assert!(first == second, "two emissions of the same inputs differ");
    // B4b: the playback list and the re-emissions of the same shapes
    let plan: Vec<Option<ShapeAnim>> = shapes.iter().map(|_| anim_of(&mut b)).collect();
    let media = sel & 128 != 0;
    let seed = b.u8();
    let a = stage_once(&shapes, &plan, &p, &table, media, seed);
    let c = stage_once(&shapes, &plan, &p, &table, media, seed);
    assert!(a == c, "two playback emissions of the same inputs differ");
}

/// Seeds: a few shape lists (the generator reads zeros past the end, so short inputs are lists too).
pub fn slides_emit_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for sel in [0u8, 4, 12, 28, 60, 127] {
        let mut v = vec![sel];
        for i in 0..600u32 {
            v.push(u8::try_from((i * 53 + u32::from(sel) * 7) % 251).unwrap_or(0));
        }
        out.push(v);
    }
    out
}
