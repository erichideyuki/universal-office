//! R-P3 parity of `paint::text_fx` (frontier G): `gradOf`, `fxMarginPt`, `runBox`,
//! `runOutlineSegs`, `fxLayers` (with the face bytes), `dilate` and `dashPattern` against
//! `tests/captures/g_text_fx.capture` (the frozen TS).
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod g_support;

use g_support::*;
use std::sync::Arc;
use uo_drawingml::{Mask, PathSeg};
use uo_fonts::{Face, FontLimits};
use uo_render::NO_GID;
use uo_render::paint::PaintLimits;
use uo_render::paint::text_fx::{self as tx, RunView};
use uo_render::types::{FxGlow, FxOutline, FxReflection, FxShadow, FxStop, LineCap, LineJoin, ResolvedFill, ResolvedTextFx};
use uo_render::{OptF64, SymbolTable};
use uo_testkit::generate::Rng;
use uo_testkit::{Replay, Stream};

const CAPTURE: &str = include_str!("captures/g_text_fx.capture");
const L: PaintLimits = PaintLimits::DEFAULT;
const FONT_FILES: [&str; 4] = ["liberationserif-regular", "carlito-regular", "liberationmono-bold", "caladea-italic"];
const CPS_BAD: [u32; 5] = [0xffff, 0x4e00, 0x10ffff, 0x20, 0];

fn fonts() -> Vec<Face> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    FONT_FILES
        .iter()
        .map(|n| {
            let bytes: Arc<[u8]> = std::fs::read(root.join("shared/fonts/files").join(format!("{n}.ttf"))).unwrap().into();
            Face::parse(bytes, &FontLimits::DEFAULT).unwrap()
        })
        .collect()
}

/// An owned run (the columns a `RunView` borrows).
struct Run {
    font: usize,
    size_pt: f64,
    y: f64,
    scale_x: OptF64,
    synth_italic: bool,
    cps: Vec<u32>,
    xs: Vec<f64>,
    gids: Vec<u32>,
}

impl Run {
    fn view<'a>(&'a self, fonts: &'a [Face]) -> RunView<'a> {
        RunView {
            face: &fonts[self.font],
            size_pt: self.size_pt,
            y: self.y,
            scale_x: self.scale_x,
            synth_italic: self.synth_italic,
            cps: &self.cps,
            xs: &self.xs,
            gids: &self.gids,
        }
    }
}

fn gen_run(r: &mut Rng, n_fonts: usize, max_glyphs: usize) -> Run {
    let text: Vec<u32> = "Hello, Wörld! fi ffl ÀÉÇ 123 gjpqy".chars().map(u32::from).collect();
    let font = r.below(n_fonts);
    let n = r.below(max_glyphs + 1);
    let mut x = pick(r, &[0.0, 1440.0, -200.0, 5000.5]);
    let (mut cps, mut xs, mut gids) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let cp = if r.below(12) == 0 { pick(r, &CPS_BAD) } else { pick(r, &text) };
        let gid = if r.below(5) == 0 { pick(r, &[0u32, 1, 36, 70, 499, 70000, 3]) } else { NO_GID };
        cps.push(cp);
        xs.push(x);
        gids.push(gid);
        x += r.below(700) as f64;
    }
    if gids.iter().all(|&g| g == NO_GID) {
        gids.clear();
    }
    let size_pt = pick(r, &[0.0, 9.0, 11.0, 14.5, 36.0, 100.0, 250.0, 0.5]);
    let y = pick(r, &[0.0, 240.0, 1440.5, -100.0]);
    let scale_x = if r.below(2) == 0 { OptF64::some(pick(r, &[1.0, 0.5, 2.0, 0.0, NAN, -1.0])) } else { OptF64::NONE };
    let synth_italic = boolean(r);
    Run { font, size_pt, y, scale_x, synth_italic, cps, xs, gids }
}

const ALGN: [&str; 6] = ["b", "t", "tl", "ctr", "zz", "br"];

fn gen_fx(r: &mut Rng, syms: &mut SymbolTable) -> ResolvedTextFx {
    let mut fx = ResolvedTextFx::default();
    let mut sym = |s: &str| syms.intern_str(s).unwrap();
    if r.below(8) != 0 {
        if boolean(r) {
            let blur_pt = pick(r, &[0.0, 2.0, 5.5, 10.0]);
            let dist_pt = pick(r, &[0.0, 3.0, -4.0, 8.0]);
            let dir_deg = pick(r, &[0.0, 45.0, 90.0, 135.0, 180.0, 270.0, 30.5]);
            let sx = pick(r, &[1.0, 0.5, 1.2, -1.0, 0.8]);
            let sy = pick(r, &[1.0, 0.5, 1.2, -1.0, 0.8]);
            let kx_deg = pick(r, &[0.0, 10.0, -20.0]);
            let ky_deg = pick(r, &[0.0, 10.0, -20.0]);
            let algn = sym(pick(r, &ALGN));
            let hex = sym(pick(r, &VALID));
            let alpha = pick(r, &[1.0, 0.5, 0.25, 0.0, 0.9]);
            fx.shadow = Some(FxShadow { blur_pt, dist_pt, dir_deg, sx, sy, kx_deg, ky_deg, algn, hex, alpha });
        }
        if boolean(r) {
            let rad_pt = pick(r, &[0.0, 1.5, 4.0, 8.0]);
            let hex = sym(pick(r, &VALID));
            let alpha = pick(r, &[1.0, 0.5, 0.25, 0.0, 0.9]);
            fx.glow = Some(FxGlow { rad_pt, hex, alpha });
        }
        if boolean(r) {
            let blur_pt = pick(r, &[0.0, 1.0, 3.0]);
            let st_a = pick(r, &[0.5, 1.0, 0.3]);
            let st_pos = pick(r, &[0.0, 0.1]);
            let end_a = pick(r, &[0.0, 0.1]);
            let end_pos = pick(r, &[0.5, 0.9, 1.0]);
            let dist_pt = pick(r, &[0.0, 2.0, -1.0]);
            let dir_deg = pick(r, &[90.0, 0.0, 45.0]);
            let algn = sym(pick(r, &ALGN));
            fx.reflection = Some(FxReflection { blur_pt, st_a, st_pos, end_a, end_pos, dist_pt, dir_deg, algn });
        }
    }
    if r.below(3) == 0 {
        let width_pt = pick(r, &[0.5, 1.0, 2.25, 0.0, 6.0, 12.0]);
        fx.outline = Some(FxOutline { width_pt, fill: ResolvedFill::None, dash: None, cap: LineCap::Butt, join: LineJoin::Miter });
    }
    if r.below(4) == 0 {
        fx.fill = Some(ResolvedFill::None);
    }
    fx
}

fn sub_out(s: &mut Stream, subs: &uo_drawingml::Subpaths) {
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
                PathSeg::Q { x1, y1, x, y } => {
                    s.tag(b'Q');
                    for v in [x1, y1, x, y] {
                        f(s, v);
                    }
                }
                _ => panic!("the outline of a glyph has only M, L, Q"),
            }
        }
    }
}

#[test]
fn text_fx_parity() {
    let cap = capture(CAPTURE);
    let mut rep = Replay::new(&cap);
    let fonts = fonts();

    {
        let mut r = Rng::new(60);
        let got = dig(|s| {
            for _ in 0..1500 {
                let mut syms = SymbolTable::new();
                let fx = gen_fx(&mut r, &mut syms);
                let h = pick(&mut r, &[0.0, 12.0, 30.5, NAN, 100.0, -3.0]);
                let m = tx::fx_margin_pt(&fx, h);
                for v in [m.l, m.t, m.r, m.b] {
                    f(s, v);
                }
            }
        });
        rep.check("margin", "v", &got);
    }
    {
        let mut r = Rng::new(70);
        let got = dig(|s| {
            for _ in 0..1500 {
                let run = gen_run(&mut r, fonts.len(), 8);
                match tx::run_box(&run.view(&fonts)) {
                    None => {
                        s.tag(b'E');
                    }
                    Some(b) => {
                        s.tag(b'B');
                        for v in [b.x, b.y, b.w, b.h] {
                            f(s, v);
                        }
                    }
                }
            }
        });
        rep.check("box", "v", &got);
    }
    {
        let mut r = Rng::new(80);
        let got = dig(|s| {
            for _ in 0..600 {
                let run = gen_run(&mut r, fonts.len(), 8);
                sub_out(s, &tx::run_outline_segs(&run.view(&fonts)));
            }
        });
        rep.check("outline", "v", &got);
    }
    {
        let mut syms = SymbolTable::new();
        let mut hex = |h: &str| syms.intern_str(h).unwrap();
        let fills = [
            ResolvedFill::Grad {
                stops: vec![FxStop { pos: 0.0, hex: hex("FF0000"), alpha: 1.0 }, FxStop { pos: 1.0, hex: hex("0000FF"), alpha: 0.5 }],
                angle_deg: 90.0,
                scaled: true,
                path: None,
                fill_to: None,
            },
            ResolvedFill::Grad {
                stops: vec![],
                angle_deg: NAN,
                scaled: false,
                path: Some(hex("circle")),
                fill_to: Some(uo_render::types::Ltrb { l: 0.5, t: 0.5, r: 0.5, b: 0.5 }),
            },
        ];
        assert!(tx::grad_of(&ResolvedFill::None).is_none());
        let got = dig(|s| {
            for fl in &fills {
                let o = tx::grad_of(fl).unwrap();
                s.u32(u32n(o.stops.len()));
                for st in &o.stops {
                    f(s, st.pos);
                    s.str(&String::from_utf16(syms.get(st.color)).unwrap());
                }
                f(s, o.angle_deg);
                s.bool(o.scaled);
                s.str(&o.path.map_or("~".to_owned(), |p| String::from_utf16(syms.get(p)).unwrap()));
                s.bool(o.rot_with_shape);
                s.bool(o.fill_to.is_some());
                if let Some(ft) = o.fill_to {
                    for v in [ft.l, ft.t, ft.r, ft.b] {
                        f(s, v);
                    }
                }
            }
        });
        rep.check("gradof", "v", &got);
    }
    {
        let mut r = Rng::new(100);
        let got = dig(|s| {
            for _ in 0..300 {
                let (w, h) = (r.below(40), r.below(30));
                let mut m = Mask::new(u32n(w), u32n(h), &L.draw).unwrap();
                for cell in m.data_mut() {
                    *cell = if r.below(5) == 0 { pick(&mut r, &[1.0, 0.5, 0.25, 2.0, NAN, 0.999]) as f32 } else { 0.0 };
                }
                let rad = pick(&mut r, &[0.5, 1.0, 1.5, 2.2, 3.0, 5.5, 0.4, -1.0, 0.0, NAN, 12.0]);
                let o = tx::dilate(&m, rad, &L).unwrap();
                s.u32(o.w());
                s.u32(o.h());
                let bytes: Vec<u8> =
                    o.data().iter().flat_map(|v| if v.is_nan() { 0x7fc0_0000u32 } else { v.to_bits() }.to_le_bytes()).collect();
                s.bytes(&bytes);
            }
        });
        rep.check("dilate", "v", &got);
    }
    {
        let names: [Option<&str>; 15] = [
            Some("dot"),
            Some("sysDot"),
            Some("sysDash"),
            Some("dash"),
            Some("dashDot"),
            Some("lgDash"),
            Some("lgDashDot"),
            Some("lgDashDotDot"),
            Some("sysDashDot"),
            Some("sysDashDotDot"),
            Some("solid"),
            Some(""),
            None,
            Some("DASH"),
            Some("dash "),
        ];
        let got = dig(|s| {
            for n in names {
                let u = n.map(u16s);
                let a = tx::dash_pattern(u.as_deref());
                s.u32(u32n(a.len()));
                for v in a {
                    f(s, *v);
                }
            }
        });
        rep.check("dash", "v", &got);
    }
    {
        let mut r = Rng::new(110);
        let mut layers = 0;
        let got = dig(|s| {
            for _ in 0..260 {
                let mut run = gen_run(&mut r, fonts.len(), 6);
                run.size_pt = pick(&mut r, &[8.0, 11.0, 14.5, 24.0, 36.0]);
                if run.scale_x.get().is_some_and(f64::is_nan) {
                    run.scale_x = OptF64::some(1.0);
                }
                let mut syms = SymbolTable::new();
                let fx = gen_fx(&mut r, &mut syms);
                let ppt = pick(&mut r, &[0.5, 1.0, 2.78, 4.0]);
                let hex = u16s(pick(&mut r, &VALID));
                if run.cps.is_empty() {
                    s.tag(b'E');
                    continue;
                }
                match tx::fx_layers(&run.view(&fonts), &fx, &syms, ppt, &hex, &L).unwrap() {
                    None => {
                        s.tag(b'N');
                    }
                    Some(o) => {
                        layers += 1;
                        s.tag(b'L');
                        s.u32(u32n(o.w));
                        s.u32(u32n(o.h));
                        f(s, o.x0);
                        f(s, o.y0);
                        s.bytes(&o.data);
                    }
                }
            }
        });
        eprintln!("g_text_fx: {layers} layers of 260 cases");
        assert!(layers > 100, "the corpus must produce layers: {layers}");
        rep.check("layers", "v", &got);
    }

    let report = rep.finish();
    eprintln!("g_text_fx: {}", report.summary());
    report.assert_clean();
}
