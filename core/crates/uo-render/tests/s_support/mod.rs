//! Shared by the Slides list tests (R-S1/R-S2): a sample slide the way the producer will emit it (a background, a gradient
//! shape with a shadow, a picture with a colour effect, text runs with decorations, a layer, an animated paragraph slot, a
//! warp effect), and a random generator of lists with hostile numbers and indices.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use uo_render::slides::*;
use uo_testkit::generate::Rng;

pub fn p(x: f64, y: f64) -> Pair {
    Pair { x, y }
}

pub fn r4(x: f64, y: f64, w: f64, h: f64) -> Rect4 {
    Rect4 { x, y, w, h }
}

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// One slide at 1280 x 720: what `uo-pptx` would emit for a card with a gradient and a shadow, a pattern fill, a
/// recoloured picture, a layered shape, an animated two-run paragraph and a warped title.
pub fn sample_list(res: &mut SlidesResources) -> SlidesList {
    let white = res.sym_str("#FFFFFF").unwrap();
    let navy = res.sym_str("#1F3864").unwrap();
    let shadow = res.sym_str("rgba(0,0,0,0.35)").unwrap();
    let fam = res.sym_str("Calibri").unwrap();
    let calibri = res.face(fam, false, false).unwrap();
    let calibri_b = res.face(fam, true, false).unwrap();
    let part = res.sym_str("/ppt/media/image1.png").unwrap();
    let ext = res.sym_str("png").unwrap();
    let media = res.media(Media { part, ext, byte_len: 48_213, has_svg: false, natural: Some(PxSize { w: 640, h: 480 }) }).unwrap();
    let from = res.sym_str("#FF0000").unwrap();
    let to = res.sym_str("#00FF00").unwrap();
    let derived = res
        .derived(
            media,
            ImageFx {
                duotone: None,
                gray: false,
                bi_level: None,
                bright: Some(0.2),
                contrast: None,
                clr_change: Some(ClrChange { from, to, to_alpha: 1.0 }),
            },
        )
        .unwrap();
    let tile = res.bitmap(8, 8, vec![0x7F; 8 * 8 * 4].into()).unwrap();
    let wa = res.sym_str("textArchUp").unwrap();
    let align = res.sym_str("ctr").unwrap();

    let mut l = SlidesList {
        view: ViewBox { ox: 0.0, oy: 0.0, scale: 96.0 / 9525.0, sw: 1280.0, sh: 720.0 },
        items: Vec::new(),
        paths: Vec::new(),
        stops: vec![Stop { offset: 0.0, color: navy }, Stop { offset: 1.0, color: white }],
        dashes: vec![4.0, 2.0],
        texts: Vec::new(),
        gradients: vec![Gradient { g0: 100.0, g1: 100.0, g2: 100.0, g3: 400.0, g4: 0.0, g5: 0.0, stops: Span { start: 0, len: 2 } }],
        patterns: vec![PatternRes {
            src: PatternSrc::Bitmap(tile),
            repeat: Repeat::Repeat,
            transform: Some(Mat6 { a: 0.96, b: 0.0, c: 0.0, d: 0.96, e: 0.0, f: 0.0 }),
        }],
        runs: Vec::new(),
        effects: Vec::new(),
        slots: Vec::new(),
    };
    l.paths.extend([
        PathOp::MoveTo(p(120.0, 100.0)),
        PathOp::LineTo(p(480.0, 100.0)),
        PathOp::BezierTo(Cubic { x1: 500.0, y1: 100.0, x2: 520.0, y2: 120.0, x: 520.0, y: 140.0 }),
        PathOp::LineTo(p(520.0, 420.0)),
        PathOp::Arc(ArcOp { cx: 480.0, cy: 420.0, r: 40.0, a0: 0.0, a1: 1.5, ccw: false }),
        PathOp::Rect(r4(120.0, 460.0, 10.0, 10.0)),
        PathOp::Close,
    ]);
    let all_path = Span { start: 0, len: 7 };
    let mut it = vec![
        // slide background
        SItem::Save,
        SItem::FillStyle(Paint::Css(white)),
        SItem::FillRect(r4(0.0, 0.0, 1280.0, 720.0)),
        SItem::Restore,
        // the card: the off-screen shadow idiom, then the gradient fill
        SItem::Save,
        SItem::ShadowColor(shadow),
        SItem::ShadowBlur(6.4),
        SItem::ShadowOffset(ShadowOff {
            x: CtmOperand { add: 3.0, k: SHADOW_OFFSET_K, axis: CtmAxis::A },
            y: CtmOperand { add: 3.0, k: SHADOW_OFFSET_K, axis: CtmAxis::B },
        }),
        SItem::Translate(p(-SHADOW_OFFSET_K, 0.0)),
        SItem::BeginPath,
        SItem::AppendPath(all_path.clone()),
        SItem::FillStyle(Paint::Linear(0)),
        SItem::Fill(FillRule::NonZero),
        SItem::Restore,
        // the outline
        SItem::Save,
        SItem::LineWidth(1.0),
        SItem::LineJoin(Join::Round),
        SItem::LineCap(Cap::Butt),
        SItem::MiterLimit(8.0),
        SItem::LineDash(Span { start: 0, len: 2 }),
        SItem::StrokeStyle(Paint::Css(navy)),
        SItem::BeginPath,
        SItem::AppendPath(all_path),
        SItem::Stroke,
        SItem::Restore,
        // pattern-filled rectangle
        SItem::FillStyle(Paint::Pattern(0)),
        SItem::FillRect(r4(540.0, 100.0, 200.0, 100.0)),
        // the recoloured picture, cropped, and a plain one
        SItem::Save,
        SItem::GlobalAlpha(0.8),
        SItem::ImageSmoothing(true),
        SItem::DrawImage(DrawImageItem {
            src: ImageSrc::Derived(derived),
            s: Some(r4(0.0, 0.0, 640.0, 360.0)),
            d: r4(560.0, 240.0, 320.0, 180.0),
        }),
        SItem::DrawImage(DrawImageItem { src: ImageSrc::Media(media), s: None, d: r4(900.0, 240.0, 160.0, 120.0) }),
        SItem::Restore,
    ];
    // a layered shape: with no layer available the painter jumps past the layer body
    let blur = res.sym_str("blur(1.50px)").unwrap();
    let begin = it.len();
    it.push(SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: 0 }));
    it.push(SItem::FilterSet(blur));
    it.push(SItem::FillRect(r4(560.0, 450.0, 100.0, 40.0)));
    it.push(SItem::LayerEnd(LayerEndItem { layer: 0, then: LayerThen::Composite }));
    let after = u32::try_from(it.len()).unwrap();
    if let Some(SItem::LayerBegin(b)) = it.get_mut(begin) {
        b.fallback_end = after;
    }
    // an animated paragraph: a slot around two text runs
    l.slots.push(SlotDef {
        kind: SlotKind::Paragraph,
        shape: 2,
        index: 0,
        base: r4(0.5, 0.15, 0.3, 0.1),
        bx: r4(120.0, 80.0, 400.0, 60.0),
        base_rot_rad: 0.0,
        words: false,
    });
    it.push(SItem::SlotBegin(0));
    for (i, (text, bold)) in [("Quarterly results", true), (" and outlook", false)].into_iter().enumerate() {
        let start = u32::try_from(l.texts.len()).unwrap();
        l.texts.extend(utf16(text));
        let len = u32::try_from(text.encode_utf16().count()).unwrap();
        let face = if bold { calibri_b } else { calibri };
        l.runs.push(TextRun {
            text: Span { start, len },
            font: FontRef { face, size_px: 32.0 },
            x: 130.0 + 260.0 * i as f64,
            y: 120.0,
            laid_w: Some(250.0),
            fit_allowed: true,
            letter_spacing_px: 0.0,
            kern: true,
            fill: Paint::Css(navy),
            alpha_mul: None,
            shadow: if i == 0 { Some(TextShadow { color: shadow, blur: 2.0, dx: 1.0, dy: 1.0 }) } else { None },
            fill_text: true,
            outline: None,
            underline: if i == 1 { Some(Decoration { color: navy, width: 2.0, y: 124.5 }) } else { None },
            strike: None,
        });
        it.push(SItem::Font(FontRef { face, size_px: 32.0 }));
        it.push(SItem::TextAlign(Align::Left));
        it.push(SItem::TextBaseline(Baseline::Alphabetic));
        it.push(SItem::Text(u32::try_from(l.runs.len() - 1).unwrap()));
    }
    it.push(SItem::SlotEnd);
    // a warped title (shell-executed effect)
    l.effects.push(Effect::Warp(WarpEffect {
        prst: wa,
        adj: Some(vec![10_800_000.0]),
        from_word_art: false,
        bx: r4(120.0, 520.0, 480.0, 120.0),
        px_per_pt: 4.0 / 3.0,
        lines: vec![WarpLine {
            runs: vec![WarpRun {
                text: utf16("Arc"),
                font: WarpFont { face: calibri_b, size_pt: 40.0 },
                x: 0.0,
                color: navy,
                stroke: None,
                pattern: None,
                spc: None,
                paint: None,
                alpha: None,
            }],
            baseline: 40.0,
            size_pt: 40.0,
            height: Some(48.0),
            ln_pct: None,
            align,
            trailing: None,
        }],
        shadow: None,
        tile: None,
    }));
    it.push(SItem::Effect(0));
    l.items = it;
    l
}

/// Random numbers with the awkward bit patterns (NaN with a payload, infinities, -0, subnormals) and plain ones.
pub fn num(r: &mut Rng) -> f64 {
    match r.below(6) {
        0 => f64::from_bits(u64::from(r.next_u32()) << 32 | u64::from(r.next_u32())),
        1 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 1e300, 5e-324][r.below(6)],
        2 => f64::from(r.next_u32() as i32),
        _ => [0.0, 1.0, 100.0, 0.5, -3.0, 1280.0][r.below(6)],
    }
}

/// An index that is mostly in range and sometimes far outside.
pub fn idx(r: &mut Rng, len: usize) -> u32 {
    if len == 0 || r.below(40) == 0 { r.next_u32() } else { r.below(len) as u32 }
}

pub fn span(r: &mut Rng, len: usize) -> Span {
    if r.below(40) == 0 {
        Span { start: r.next_u32(), len: r.next_u32() }
    } else {
        let start = r.below(len + 1);
        Span { start: start as u32, len: r.below(len - start + 1) as u32 }
    }
}

/// Resources with a few entries in every table.
pub fn small_resources() -> SlidesResources {
    let mut res = SlidesResources::new();
    let syms: Vec<u32> = (0..8).map(|i| res.sym_str(&format!("s{i}")).unwrap()).collect();
    res.face(syms[0], false, false).unwrap();
    res.face(syms[0], true, false).unwrap();
    for i in 0..3 {
        res.media(Media { part: syms[i], ext: syms[7], byte_len: i as u32, has_svg: false, natural: None }).unwrap();
    }
    res.derived(0, ImageFx { duotone: None, gray: true, bi_level: None, bright: None, contrast: None, clr_change: None }).unwrap();
    res.bitmap(1, 1, vec![1, 2, 3, 4].into()).unwrap();
    res
}

/// A list with hostile numbers and indices (mostly valid ones), for tables of `lens`.
pub fn hostile_list(r: &mut Rng, lens: [usize; 5]) -> SlidesList {
    let n_stops = r.below(4);
    let n_paths = r.below(6);
    let n_dashes = r.below(4);
    let n_texts = r.below(12);
    let mut l = SlidesList {
        view: ViewBox { ox: num(r), oy: num(r), scale: num(r), sw: num(r), sh: num(r) },
        items: Vec::new(),
        paths: (0..n_paths)
            .map(|_| match r.below(4) {
                0 => PathOp::MoveTo(p(num(r), num(r))),
                1 => PathOp::Arc(ArcOp { cx: num(r), cy: num(r), r: num(r), a0: num(r), a1: num(r), ccw: r.below(2) == 0 }),
                2 => PathOp::Rect(r4(num(r), num(r), num(r), num(r))),
                _ => PathOp::Close,
            })
            .collect(),
        stops: (0..n_stops).map(|_| Stop { offset: num(r), color: idx(r, lens[0]) }).collect(),
        dashes: (0..n_dashes).map(|_| num(r)).collect(),
        texts: (0..n_texts).map(|_| r.next_u32() as u16).collect(),
        gradients: Vec::new(),
        patterns: Vec::new(),
        runs: Vec::new(),
        effects: Vec::new(),
        slots: (0..r.below(3))
            .map(|_| SlotDef {
                kind: SlotKind::Shape,
                shape: r.next_u32(),
                index: r.next_u32(),
                base: r4(num(r), num(r), num(r), num(r)),
                bx: r4(num(r), num(r), num(r), num(r)),
                base_rot_rad: num(r),
                words: r.below(2) == 0,
            })
            .collect(),
    };
    for _ in 0..r.below(3) {
        l.gradients.push(Gradient { g0: num(r), g1: num(r), g2: num(r), g3: num(r), g4: num(r), g5: num(r), stops: span(r, n_stops) });
    }
    for _ in 0..r.below(3) {
        let src = if r.below(2) == 0 { PatternSrc::Bitmap(idx(r, lens[4])) } else { PatternSrc::Image(ImageSrc::Media(idx(r, lens[2]))) };
        l.patterns.push(PatternRes { src, repeat: Repeat::NoRepeat, transform: None });
    }
    for _ in 0..r.below(3) {
        let fill = match r.below(3) {
            0 => Paint::Css(idx(r, lens[0])),
            1 => Paint::Linear(idx(r, l.gradients.len())),
            _ => Paint::Pattern(idx(r, l.patterns.len())),
        };
        l.runs.push(TextRun {
            text: span(r, n_texts),
            font: FontRef { face: idx(r, lens[1]), size_px: num(r) },
            x: num(r),
            y: num(r),
            laid_w: if r.below(2) == 0 { Some(num(r)) } else { None },
            fit_allowed: r.below(2) == 0,
            letter_spacing_px: num(r),
            kern: r.below(2) == 0,
            fill,
            alpha_mul: None,
            shadow: None,
            fill_text: true,
            outline: None,
            underline: None,
            strike: None,
        });
    }
    for _ in 0..r.below(2) {
        l.effects.push(Effect::MetafileRop(MetafileRopEffect {
            rop: r.next_u32(),
            rect: r4(num(r), num(r), num(r), num(r)),
            operand: idx(r, lens[4]),
        }));
    }
    let n = r.below(40);
    for i in 0..n {
        let it = match r.below(14) {
            0 => SItem::Save,
            1 => SItem::Restore,
            2 => SItem::Translate(p(num(r), num(r))),
            3 => SItem::Transform(Mat6 { a: num(r), b: num(r), c: num(r), d: num(r), e: num(r), f: num(r) }),
            4 => SItem::CaptureCtm(r.below(5) as u32),
            5 => SItem::FillStyle(Paint::Css(idx(r, lens[0]))),
            6 => SItem::AppendPath(span(r, l.paths.len())),
            7 => SItem::LineDash(span(r, l.dashes.len())),
            8 => SItem::Text(idx(r, l.runs.len())),
            9 => SItem::Effect(idx(r, l.effects.len())),
            10 => SItem::SlotBegin(idx(r, l.slots.len())),
            11 => SItem::LayerBegin(LayerBeginItem { layer: r.below(18) as u32, fallback_end: (i + 1 + r.below(n + 1)) as u32 }),
            12 => {
                SItem::DrawImage(DrawImageItem { src: ImageSrc::Bitmap(idx(r, lens[4])), s: None, d: r4(num(r), num(r), num(r), num(r)) })
            }
            _ => SItem::FillRect(r4(num(r), num(r), num(r), num(r))),
        };
        l.items.push(it);
    }
    l
}
