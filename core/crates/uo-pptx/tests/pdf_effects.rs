//! The effects of the Slides PDF that need pixels (R-S5 wave 2): canvas shadows (an image of the shadow colour with the
//! blurred alpha as its soft mask), layers composited with raster-only modes (soft edges, reflections: the layer as an
//! image), the 3D bevel (the page under the shape shaded), WordArt warps (vector outlines) and the per-character font
//! fallback. Pixel values are read back from uncompressed PDFs. `UO_PDF_SYSTEM_FONTS=1` adds a measure of the
//! characters that still have no face when the machine's installed fonts join the open ones (local only: nothing is
//! copied).
#![cfg(feature = "pdf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::sync::Arc;

use uo_dml_render::PartSource;
use uo_dml_render::ids::MediaId;
use uo_dml_render::source::R;
use uo_pptx::pdf::{FaceSet, FontSource, PdfOptions, PdfReport, pdf_from_lists};
use uo_render::slides::{
    BevelEffect, CompositeOp, CtmAxis, CtmOperand, Effect, FillRule, FontRef, Gradient, LayerBeginItem, LayerEndItem, LayerThen, Mat6,
    Paint, PathOp, Rect4, SItem, ShadowOff, SlidesList, SlidesResources, Span, Stop, TextRun, ViewBox, WarpEffect, WarpFont, WarpLine,
    WarpRun,
};

#[path = "support/open_fonts.rs"]
mod open_fonts;
#[path = "support/pdf_view.rs"]
mod pdf_view;

use open_fonts::open_fonts;

struct NoParts;

impl PartSource for NoParts {
    fn part(&self, _: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, _: &str) -> R<Option<Arc<uo_opc::xml::Document>>> {
        Ok(None)
    }
    fn media(&self, _: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

fn list(items: Vec<SItem>) -> SlidesList {
    SlidesList {
        view: ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: 200.0, sh: 150.0 },
        items,
        paths: Vec::new(),
        stops: Vec::new(),
        dashes: Vec::new(),
        texts: Vec::new(),
        gradients: Vec::new(),
        patterns: Vec::new(),
        runs: Vec::new(),
        effects: Vec::new(),
        slots: Vec::new(),
        advances: Vec::new(),
    }
}

fn render_with(l: &SlidesList, res: &SlidesResources, fonts: &dyn FontSource) -> (Vec<u8>, PdfReport) {
    let mut o = PdfOptions::new(fonts);
    o.compress = false;
    let out = pdf_from_lists(&[(l, res)], &NoParts, &o).unwrap();
    pdf_view::view(&out.bytes).unwrap();
    (out.bytes, out.report)
}

fn render(l: &SlidesList, res: &SlidesResources) -> (Vec<u8>, PdfReport) {
    render_with(l, res, &open_fonts())
}

fn rect(l: &mut SlidesList, x: f64, y: f64, w: f64, h: f64) -> Span {
    let start = u32::try_from(l.paths.len()).unwrap();
    l.paths.push(PathOp::Rect(Rect4 { x, y, w, h }));
    Span { start, len: 1 }
}

fn content(bytes: &[u8]) -> String {
    let v = pdf_view::view(bytes).unwrap();
    v.content_streams().into_iter().map(|(_, c)| c).collect::<Vec<_>>().join("\n")
}

/// The raw streams of the image XObjects (uncompressed PDF): `(dict, bytes)`.
fn images(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    let needle = b"/Subtype /Image";
    while let Some(p) = bytes[at..].windows(needle.len()).position(|w| w == needle) {
        let start = at + p;
        let dict_end = start + bytes[start..].windows(6).position(|w| w == b"stream").unwrap();
        let dict = String::from_utf8_lossy(&bytes[start..dict_end]).into_owned();
        let mut s = dict_end + 6;
        if bytes[s] == b'\r' {
            s += 1;
        }
        if bytes[s] == b'\n' {
            s += 1;
        }
        let e = s + bytes[s..].windows(9).position(|w| w == b"endstream").unwrap();
        let mut body = bytes[s..e].to_vec();
        while body.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            body.pop();
        }
        out.push((dict, body));
        at = e;
    }
    out
}

fn dim(dict: &str, key: &str) -> usize {
    let i = dict.find(key).unwrap() + key.len();
    dict[i..].split(|c: char| c == ' ' || c == '/' || c.is_ascii_control()).find(|t| !t.is_empty()).unwrap().parse().unwrap()
}

#[test]
fn a_shadow_is_an_image_of_its_colour_with_the_blurred_alpha_under_the_shape() {
    let mut res = SlidesResources::new();
    let blue = res.sym_str("#0000ff").unwrap();
    let shadow = res.sym_str("rgba(255,0,0,0.5)").unwrap();
    let mut l = list(Vec::new());
    let p = rect(&mut l, 20.0, 20.0, 60.0, 40.0);
    l.items = vec![
        SItem::Save,
        SItem::ShadowColor(shadow),
        SItem::ShadowBlur(8.0),
        SItem::ShadowOffset(ShadowOff {
            x: CtmOperand { add: 10.0, k: 0.0, axis: CtmAxis::A },
            y: CtmOperand { add: 6.0, k: 0.0, axis: CtmAxis::B },
        }),
        SItem::FillStyle(Paint::Css(blue)),
        SItem::BeginPath,
        SItem::AppendPath(p),
        SItem::Fill(FillRule::NonZero),
        SItem::Restore,
    ];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.shadows, 1);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    let imgs = images(&bytes);
    let color = imgs.iter().find(|(d, _)| d.contains("/Width 1 /Height 1")).expect("the 1x1 colour image");
    assert_eq!(color.1, vec![255, 0, 0], "the shadow colour");
    let mask = imgs.iter().find(|(d, _)| d.contains("/DeviceGray")).expect("the soft mask");
    let (w, h) = (dim(&mask.0, "/Width "), dim(&mask.0, "/Height "));
    assert_eq!(mask.1.len(), w * h);
    // the centre of the moved shape: half alpha (0.5 of the colour), the corners of the region nearly clear
    let peak = *mask.1.iter().max().unwrap();
    assert!((120..=129).contains(&peak), "peak alpha {peak}");
    assert!(mask.1[0] < 8, "corner {}", mask.1[0]);
    // the shadow is drawn before the shape
    let c = content(&bytes);
    let (do_at, fill_at) = (c.find(" Do").unwrap(), c.find("\nf\n").unwrap());
    assert!(do_at < fill_at, "{c}");
}

#[test]
fn the_offscreen_idiom_draws_only_the_shadow() {
    let mut res = SlidesResources::new();
    let black = res.sym_str("#000").unwrap();
    let glow = res.sym_str("rgba(0,128,0,1)").unwrap();
    let mut l = list(Vec::new());
    let p = rect(&mut l, 50.0, 50.0, 40.0, 40.0);
    l.items = vec![
        SItem::Save,
        SItem::ShadowColor(glow),
        SItem::ShadowBlur(6.0),
        SItem::ShadowOffset(ShadowOff {
            x: CtmOperand { add: 0.0, k: 20000.0, axis: CtmAxis::A },
            y: CtmOperand { add: 0.0, k: 20000.0, axis: CtmAxis::B },
        }),
        SItem::Translate(uo_render::slides::Pair { x: -20000.0, y: 0.0 }),
        SItem::FillStyle(Paint::Css(black)),
        SItem::BeginPath,
        SItem::AppendPath(p),
        SItem::Fill(FillRule::NonZero),
        SItem::Restore,
    ];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.shadows, 1);
    let c = content(&bytes);
    assert!(!c.contains("\nf\n"), "the shape itself is not drawn: {c}");
    let mask = images(&bytes).into_iter().find(|(d, _)| d.contains("/DeviceGray")).unwrap();
    assert!(mask.1.iter().any(|a| *a > 200), "the glow is opaque at its centre");
}

#[test]
fn a_destination_in_layer_is_rasterized_with_the_right_alpha() {
    let mut res = SlidesResources::new();
    let red = res.sym_str("#ff0000").unwrap();
    let half = res.sym_str("rgba(0,0,0,0.5)").unwrap();
    let mut l = list(Vec::new());
    let r = Rect4 { x: 10.0, y: 10.0, w: 50.0, h: 50.0 };
    l.items = vec![
        SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: 10 }),
        SItem::FillStyle(Paint::Css(red)),
        SItem::FillRect(r.clone()),
        SItem::Save,
        SItem::Composite(CompositeOp::DestinationIn),
        SItem::FillStyle(Paint::Css(half)),
        SItem::FillRect(Rect4 { x: 10.0, y: 10.0, w: 25.0, h: 50.0 }),
        SItem::Restore,
        SItem::LayerEnd(LayerEndItem { layer: 0, then: LayerThen::Composite }),
        SItem::FillRect(r),
    ];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.raster_layers, 1);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    let imgs = images(&bytes);
    let rgb = imgs.iter().find(|(d, _)| d.contains("/DeviceRGB") && !d.contains("/Width 1 ")).expect("the layer image");
    let mask = imgs.iter().find(|(d, _)| d.contains("/DeviceGray")).expect("its alpha");
    let w = dim(&rgb.0, "/Width ");
    // the left half of the square kept at half alpha, the right half removed (destination-in clears outside)
    let mid_row = dim(&rgb.0, "/Height ") / 2;
    let a_left = mask.1[mid_row * w + w / 4];
    assert!((126..=129).contains(&a_left), "left alpha {a_left}");
    assert_eq!(&rgb.1[(mid_row * w + w / 4) * 3..][..3], &[255, 0, 0]);
    // the right half was cleared: the placed image is only the left half (cropped to what is not clear)
    assert!((48..=53).contains(&w), "width {w}");
    assert_eq!(mask.1[mid_row * w + w - 1], 255 / 2 + 1, "the edge of the kept half");
    assert!(!content(&bytes).contains("/Fm"), "no vector form for that layer");
}

#[test]
fn a_reflection_is_the_kept_layer_flipped_blurred_and_faded() {
    let mut res = SlidesResources::new();
    let red = res.sym_str("#ff0000").unwrap();
    let a0 = res.sym_str("rgba(0,0,0,1)").unwrap();
    let a1 = res.sym_str("rgba(0,0,0,0)").unwrap();
    let blur = res.sym_str("blur(2.00px)").unwrap();
    let mut l = list(Vec::new());
    l.stops = vec![Stop { offset: 0.0, color: a0 }, Stop { offset: 1.0, color: a1 }];
    l.gradients = vec![Gradient { g0: 0.0, g1: 70.0, g2: 0.0, g3: 120.0, g4: 0.0, g5: 0.0, stops: Span { start: 0, len: 2 } }];
    l.items = vec![
        // the shape's layer, kept
        SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: 4 }),
        SItem::FillStyle(Paint::Css(red)),
        SItem::FillRect(Rect4 { x: 20.0, y: 20.0, w: 50.0, h: 50.0 }),
        SItem::LayerEnd(LayerEndItem { layer: 0, then: LayerThen::Keep }),
        SItem::CaptureCtm(0),
        // the reflection: the kept layer flipped below, blurred, faded by a destination-in gradient
        SItem::LayerBegin(LayerBeginItem { layer: 1, fallback_end: 20 }),
        SItem::Save,
        SItem::Translate(uo_render::slides::Pair { x: 0.0, y: 70.0 }),
        SItem::Scale(uo_render::slides::Pair { x: 1.0, y: -1.0 }),
        SItem::Translate(uo_render::slides::Pair { x: 0.0, y: -70.0 }),
        SItem::TransformByInverseOf(0),
        SItem::FilterSet(blur),
        SItem::DrawLayer(0),
        SItem::Restore,
        SItem::Save,
        SItem::Composite(CompositeOp::DestinationIn),
        SItem::FillStyle(Paint::Linear(0)),
        SItem::FillRect(Rect4 { x: -30.0, y: 69.0, w: 150.0, h: 52.0 }),
        SItem::Restore,
        SItem::LayerEnd(LayerEndItem { layer: 1, then: LayerThen::Composite }),
        // the shape itself
        SItem::DrawLayer(0),
        SItem::ReleaseLayer(0),
    ];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.raster_layers, 1, "{report:?}");
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    let c = content(&bytes);
    assert!(c.contains("/Fm"), "the kept layer stays vector: {c}");
    let imgs = images(&bytes);
    let rgb = imgs.iter().find(|(d, _)| d.contains("/DeviceRGB") && !d.contains("/Width 1 ")).expect("the reflection");
    let mask = imgs.iter().find(|(d, _)| d.contains("/DeviceGray")).unwrap();
    let (w, h) = (dim(&rgb.0, "/Width "), dim(&rgb.0, "/Height "));
    // faded downwards: the column in the middle loses alpha row by row
    let col = w / 2;
    let top = (0..h / 3).map(|y| mask.1[y * w + col]).max().unwrap();
    let bottom = mask.1[(h - 3) * w + col];
    assert!(top > 150 && bottom < top / 3, "top {top} bottom {bottom}");
}

#[test]
fn a_bevel_shades_the_page_under_the_shape() {
    let mut res = SlidesResources::new();
    let gray = res.sym_str("#808080").unwrap();
    let circle = res.sym_str("circle").unwrap();
    let mut l = list(Vec::new());
    let p = rect(&mut l, 30.0, 30.0, 80.0, 60.0);
    l.items = vec![
        SItem::FillStyle(Paint::Css(gray)),
        SItem::BeginPath,
        SItem::AppendPath(p.clone()),
        SItem::Fill(FillRule::NonZero),
        SItem::Effect(0),
    ];
    l.effects = vec![Effect::Bevel(BevelEffect {
        path: p,
        rule: FillRule::NonZero,
        bx: Rect4 { x: 30.0, y: 30.0, w: 80.0, h: 60.0 },
        w: 76200.0,
        h: 76200.0,
        scale: 1.0 / 12700.0,
        prst: Some(circle),
        rig: None,
        mat: None,
        dir: None,
    })];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.bevels, 1);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    let imgs = images(&bytes);
    let rgb = imgs.iter().find(|(d, _)| d.contains("/DeviceRGB") && !d.contains("/Width 1 ")).expect("the lit pixels");
    let w = dim(&rgb.0, "/Width ");
    let h = dim(&rgb.0, "/Height ");
    // the top edge lit differently from the bottom edge, the face uniform
    let px = |x: usize, y: usize| rgb.1[(y * w + x) * 3];
    let (top, bottom, face) = (px(w / 2, 4), px(w / 2, h - 5), px(w / 2, h / 2));
    assert_ne!(top, bottom, "top {top} bottom {bottom} face {face}");
}

fn warp_list(res: &mut SlidesResources, prst: &str) -> SlidesList {
    let fam = res.sym_str("Liberation Sans").unwrap();
    let face = res.face(fam, true, false).unwrap();
    let color = res.sym_str("#336699").unwrap();
    let align = res.sym_str("ctr").unwrap();
    let prst = res.sym_str(prst).unwrap();
    let mut l = list(vec![SItem::Effect(0)]);
    l.effects = vec![Effect::Warp(WarpEffect {
        prst,
        adj: None,
        from_word_art: false,
        bx: Rect4 { x: 20.0, y: 20.0, w: 160.0, h: 100.0 },
        px_per_pt: 1.0,
        lines: vec![WarpLine {
            runs: vec![WarpRun {
                text: "WordArt".encode_utf16().collect(),
                font: WarpFont { face, size_pt: 36.0 },
                x: 10.0,
                color,
                stroke: None,
                pattern: None,
                spc: None,
                paint: None,
                alpha: None,
            }],
            baseline: 50.0,
            size_pt: 36.0,
            height: None,
            ln_pct: None,
            align,
            trailing: None,
        }],
        shadow: None,
        tile: None,
    })];
    l
}

#[test]
fn warped_text_is_drawn_as_outlines_inside_the_box() {
    for prst in ["textArchUp", "textInflate", "textWave1", "textCircle", "textPlain"] {
        let mut res = SlidesResources::new();
        let l = warp_list(&mut res, prst);
        let (bytes, report) = render(&l, &res);
        assert_eq!(report.warps, 1, "{prst}");
        assert!(report.missing.is_empty(), "{prst}: {:?}", report.missing);
        let c = content(&bytes);
        assert!(!c.contains("BT"), "{prst}: outlines, not text");
        let fills = c.matches("\nf\n").count();
        assert!(fills >= 1, "{prst}: {c}");
        // every point of the outlines near the shape's box
        let mut n = 0;
        for line in c.lines() {
            let t: Vec<&str> = line.split(' ').filter(|t| !t.is_empty()).collect();
            if t.len() == 3 && (t[2] == "m" || t[2] == "l") {
                let (x, y) = (uo_jsnum::parse::parse_float(t[0]), uo_jsnum::parse::parse_float(t[1]));
                assert!((-40.0..=240.0).contains(&x) && (-40.0..=190.0).contains(&y), "{prst}: point {x} {y}");
                n += 1;
            }
        }
        assert!(n > 100, "{prst}: {n} points");
    }
}

#[test]
fn a_character_the_face_lacks_falls_back_to_another_open_face() {
    let fonts = open_fonts();
    let caladea = fonts.face("Caladea", false, false).unwrap();
    let libsans = fonts.face("Liberation Sans", false, false).unwrap();
    // a character Caladea lacks and Liberation Sans has
    let cp = (0x100u32..0x2000)
        .find(|&c| {
            caladea.glyph_id(c) == 0 && libsans.glyph_id(c) != 0 && char::from_u32(c).is_some_and(|ch| !uo_jsnum::text::is_js_space(ch))
        })
        .unwrap();
    let ch = char::from_u32(cp).unwrap();
    let text = format!("A{ch}B");
    let mut res = SlidesResources::new();
    let fam = res.sym_str("Caladea").unwrap();
    let face = res.face(fam, false, false).unwrap();
    let black = res.sym_str("#000").unwrap();
    let mut l = list(vec![SItem::Text(0)]);
    let units: Vec<u16> = text.encode_utf16().collect();
    l.texts = units.clone();
    l.runs = vec![TextRun {
        text: Span { start: 0, len: u32::try_from(units.len()).unwrap() },
        font: FontRef { face, size_px: 20.0 },
        x: 10.0,
        y: 40.0,
        laid_w: None,
        fit_allowed: false,
        letter_spacing_px: 0.0,
        kern: true,
        fill: Paint::Css(black),
        bullet: false,
        alpha_mul: None,
        shadow: None,
        fill_text: true,
        outline: None,
        underline: None,
        strike: None,
    }];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.missing.get("glyph:missing"), None, "U+{cp:04X}: {:?}", report.missing);
    assert_eq!(report.fonts, 2, "the run's face and the fallback");
    let (got, unknown) = pdf_view::view(&bytes).unwrap().text();
    assert_eq!(unknown, 0);
    assert_eq!(got, text);
    // with the fallback off the character is missing
    let mut nofb = open_fonts();
    nofb.set_glyph_fallback(false);
    let (_, report) = render_with(&l, &res, &nofb);
    assert_eq!(report.missing.get("glyph:missing"), Some(&1));
    // the measure follows the face that draws: the fallback's advance, not .notdef's
    let w_fb = uo_pptx::pdf::text_width_in(&fonts, &caladea, false, false, 20.0, true, 0.0, &text);
    let w_plain = uo_pptx::pdf::text_width(&caladea, 20.0, true, 0.0, &text);
    let adv = |f: &uo_fonts::Face, c: u32| f64::from(f.advance_width(f.glyph_id(c))) * 20.0 / f64::from(f.metrics().units_per_em);
    assert!((w_fb - (w_plain - adv(&caladea, cp) + adv(&libsans, cp))).abs() < 1e-6, "{w_fb} {w_plain}");
}

#[test]
fn a_text_shadow_follows_the_glyphs() {
    let mut res = SlidesResources::new();
    let fam = res.sym_str("Liberation Sans").unwrap();
    let face = res.face(fam, false, false).unwrap();
    let black = res.sym_str("#000").unwrap();
    let sc = res.sym_str("rgba(0,0,0,0.6)").unwrap();
    let mut l = list(vec![SItem::Text(0)]);
    let units: Vec<u16> = "Shadow".encode_utf16().collect();
    l.texts = units.clone();
    l.runs = vec![TextRun {
        text: Span { start: 0, len: u32::try_from(units.len()).unwrap() },
        font: FontRef { face, size_px: 30.0 },
        x: 10.0,
        y: 60.0,
        laid_w: None,
        fit_allowed: false,
        letter_spacing_px: 0.0,
        kern: true,
        fill: Paint::Css(black),
        bullet: false,
        alpha_mul: None,
        shadow: Some(uo_render::slides::TextShadow { color: sc, blur: 3.0, dx: 2.0, dy: 2.0 }),
        fill_text: true,
        outline: None,
        underline: None,
        strike: None,
    }];
    let (bytes, report) = render(&l, &res);
    assert_eq!(report.shadows, 1);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    assert_eq!(pdf_view::view(&bytes).unwrap().text().0, "Shadow", "the text stays text");
}

#[test]
fn hostile_effect_lists_never_panic() {
    // shadows, blurs, raster layers and bevels with extreme numbers
    let mut res = SlidesResources::new();
    let c = res.sym_str("rgba(10,20,30,0.7)").unwrap();
    let blur = res.sym_str("blur(1e9px)").unwrap();
    let blur2 = res.sym_str("blur(3px)").unwrap();
    for v in [f64::NAN, f64::INFINITY, -1e308, 1e308, 0.0, -3.0, 1e6] {
        let mut l = list(Vec::new());
        let p = rect(&mut l, v, 10.0, 50.0, v);
        l.items = vec![
            SItem::ShadowColor(c),
            SItem::ShadowBlur(v),
            SItem::ShadowOffset(ShadowOff {
                x: CtmOperand { add: v, k: v, axis: CtmAxis::A },
                y: CtmOperand { add: 1.0, k: 0.0, axis: CtmAxis::B },
            }),
            SItem::FillStyle(Paint::Css(c)),
            SItem::BeginPath,
            SItem::AppendPath(p.clone()),
            SItem::Fill(FillRule::NonZero),
            SItem::Stroke,
            SItem::LayerBegin(LayerBeginItem { layer: 2, fallback_end: 16 }),
            SItem::Transform(Mat6 { a: v, b: 0.0, c: 0.0, d: 1.0, e: v, f: 0.0 }),
            SItem::FilterSet(if v.is_nan() { blur } else { blur2 }),
            SItem::Composite(CompositeOp::DestinationOut),
            SItem::Fill(FillRule::EvenOdd),
            SItem::Effect(0),
            SItem::LayerEnd(LayerEndItem { layer: 2, then: LayerThen::Composite }),
            SItem::Effect(0),
        ];
        l.effects = vec![Effect::Bevel(BevelEffect {
            path: p,
            rule: FillRule::NonZero,
            bx: Rect4 { x: v, y: 0.0, w: 1e7, h: v },
            w: v,
            h: 1.0,
            scale: 1.0,
            prst: None,
            rig: None,
            mat: None,
            dir: None,
        })];
        let fonts = open_fonts();
        let out = pdf_from_lists(&[(&l, &res)], &NoParts, &PdfOptions::new(&fonts));
        if let Ok(o) = out {
            pdf_view::view(&o.bytes).unwrap();
        }
    }
}

/// The open fonts plus the machine's installed fonts (read in place, never copied): the measure of what still has no
/// face once a server has the usual fonts.
struct WithSystem {
    open: FaceSet,
    system: std::collections::HashMap<String, Arc<uo_fonts::Face>>,
}

impl FontSource for WithSystem {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<uo_fonts::Face>> {
        let key: String = family.chars().flat_map(char::to_lowercase).collect();
        self.system.get(&key).cloned().or_else(|| self.open.face(family, bold, italic))
    }
}

#[test]
fn glyphs_without_a_face_with_the_system_fonts() {
    if std::env::var("UO_PDF_SYSTEM_FONTS").is_err() {
        return;
    }
    // the machine's font folder (Windows: %WINDIR%\Fonts; elsewhere UO_PDF_FONT_DIR), each file's first face by family
    let dir = std::env::var("UO_PDF_FONT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(&std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into())).join("Fonts"));
    let mut system = std::collections::HashMap::new();
    for e in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
        let p = e.path();
        if !p.extension().is_some_and(|x| ["ttf", "ttc", "otf"].iter().any(|k| x.eq_ignore_ascii_case(k))) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let Ok(one) = uo_fonts::standalone_face(&bytes, None, &uo_fonts::FontLimits::NATIVE) else { continue };
        let one = one.into_owned();
        let Ok(info) = uo_fonts::face_info(&one) else { continue };
        if info.weight > 500 || info.italic {
            continue;
        }
        let key: String = String::from_utf16_lossy(&info.names.family).chars().flat_map(char::to_lowercase).collect();
        if system.contains_key(&key) {
            continue;
        }
        let Ok(face) = uo_fonts::Face::parse(Arc::from(one), &uo_fonts::FontLimits::NATIVE) else { continue };
        system.insert(key, Arc::new(face));
    }
    let fonts = WithSystem { open: open_fonts(), system };
    let root = open_fonts::root();
    let mut decks: Vec<std::path::PathBuf> = Vec::new();
    for g in ["b1a", "b3a", "b5a", "b5b"] {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks").join(g);
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        decks.extend(names);
    }
    let samples = root.join("slides/samples");
    for d in [samples.clone(), samples.join("playback")] {
        let mut names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pptx"))
            .collect();
        names.sort();
        decks.extend(names);
    }
    let mut missing = 0u64;
    for p in &decks {
        let mut deck = uo_pptx::Deck::open(&std::fs::read(p).unwrap(), uo_pptx::Limits::default()).unwrap();
        let out = uo_pptx::pdf(&mut deck, &PdfOptions::new(&fonts)).unwrap();
        missing += out.report.missing.get("glyph:missing").copied().unwrap_or(0);
    }
    eprintln!("with the system fonts: {missing} characters without a face over {} decks", decks.len());
}

#[test]
fn a_raised_cancel_flag_stops_the_export() {
    let mut res = SlidesResources::new();
    let l = warp_list(&mut res, "textArchUp");
    let fonts = open_fonts();
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mut o = PdfOptions::new(&fonts);
    o.cancel = Some(Arc::clone(&flag));
    let r = pdf_from_lists(&[(&l, &res)], &NoParts, &o);
    assert!(matches!(r, Err(uo_pptx::pdf::PdfFail::Pdf(uo_pdf::PdfError::Cancelled))), "{:?}", r.err());
    flag.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(pdf_from_lists(&[(&l, &res)], &NoParts, &o).is_ok());
}
