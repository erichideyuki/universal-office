//! Shared by the frontier-E parity tests: the generators that mirror `tools/capture-e.mjs` (the
//! testkit `Rng`, the same pools, the same consumption order), the recording log, and
//! `shell`, a transcription of the parts of `canvas.ts` (`paintGlyphs`, `paintShape`) that read the
//! encoder's hints — the contract the TS shell (frontier D) implements.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

pub mod full;
pub mod shell;

use crate::g_support::*;
use std::sync::Arc;
use uo_fonts::{Face, FontLimits};
use uo_render::types::{
    CustCmd, CustPath, FxOutline, FxStop, GeomEval, GradFill, GradStop, Item, LineCap, LineJoin, Ltrb, PathCmd, ResolvedFill,
    ResolvedTextFx, RunFlags, ShapeGeom, ShapeItem, ShapeShadow, Stroke,
};
use uo_render::{FaceRes, GlyphRunHead, OptF64, Page, Resources, Sym};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

pub const FONT_FILES: [&str; 4] = ["liberationserif-regular", "carlito-regular", "liberationmono-bold", "caladea-italic"];

pub fn fonts() -> Vec<Face> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    FONT_FILES
        .iter()
        .map(|n| {
            let bytes: Arc<[u8]> = std::fs::read(root.join("shared/fonts/files").join(format!("{n}.ttf"))).unwrap().into();
            Face::parse(bytes, &FontLimits::DEFAULT).unwrap()
        })
        .collect()
}

/// Resources with the four faces registered (ids 1..=4 in `FONT_FILES` order).
pub fn resources_with_fonts(fonts: &[Face]) -> Resources {
    let mut res = Resources::new();
    for (i, f) in fonts.iter().enumerate() {
        let key = res.symbols.intern_str(&format!("font{i}")).unwrap();
        let family = res.symbols.intern_str(FONT_FILES.get(i).copied().unwrap_or("f")).unwrap();
        res.add_face(FaceRes {
            key,
            family,
            bold: false,
            italic: false,
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            face: Some(f.clone()),
            bytes: None,
            advances: None,
        })
        .unwrap();
    }
    res
}

// ── the recording log (JS: `recorder` + `logOut`) ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    N(f64),
    S(Vec<u32>),
    A(Vec<Arg>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub name: &'static str,
    pub args: Vec<Arg>,
}

pub fn op(name: &'static str, args: Vec<Arg>) -> Op {
    Op { name, args }
}

pub fn n(v: f64) -> Arg {
    Arg::N(v)
}

pub fn st(s: &str) -> Arg {
    Arg::S(s.chars().map(u32::from).collect())
}

fn arg_out(s: &mut Stream, a: &Arg) {
    match a {
        Arg::N(v) => {
            s.tag(b'n');
            f(s, *v);
        }
        Arg::S(cps) => {
            s.tag(b's');
            s.u32(u32n(cps.len()));
            for c in cps {
                s.u32(*c);
            }
        }
        Arg::A(v) => {
            s.tag(b'a');
            s.u32(u32n(v.len()));
            for e in v {
                arg_out(s, e);
            }
        }
    }
}

pub fn log_out(s: &mut Stream, log: &[Op]) {
    s.u32(u32n(log.len()));
    for o in log {
        s.str(o.name);
        s.u32(u32n(o.args.len()));
        for a in &o.args {
            arg_out(s, a);
        }
    }
}

// ── glyph runs (JS: genGlyphRun) ────────────────────────────────────────────────────────────────

const GLYPH_TAIL: [u32; 14] =
    [0xf020, 0xf041, 0xf0b7, 0xf0ff, 0x200c, 0x4e00, 0x20ac, 0x0416, 0x03b1, 0x2022, 0x05d0, 0x0300, 0x1f600, 0x00e9];
const JITTER: [f64; 10] = [0.0, 0.0, 0.0, 0.2, -0.3, 0.26, 1.0, 700.0, 0.25, NAN];

pub fn glyph_cps() -> Vec<u32> {
    "Hello, World! fi 123 gjpqy".chars().map(u32::from).chain(GLYPH_TAIL).collect()
}

pub struct GlyphCase {
    pub font: usize,
    pub size_pt: f64,
    pub y: f64,
    pub color: &'static str,
    pub scale_x: OptF64,
    pub synth_italic: bool,
    pub synth_bold: bool,
    pub glyphs: Vec<(u32, f64, Option<u32>)>,
}

pub fn gen_glyph_run(r: &mut Rng, fonts: &[Face], max_glyphs: usize) -> GlyphCase {
    let cps = glyph_cps();
    let font = r.below(fonts.len());
    let size_pt = pick(r, &[8.0, 11.0, 14.5, 36.0, 0.5, 0.0]);
    let kind = r.below(8);
    let count = 1 + r.below(max_glyphs);
    let face = &fonts[font];
    let upem = {
        let u = face.metrics().units_per_em;
        if u == 0 { 1000.0 } else { f64::from(u) }
    };
    let mut x = pick(r, &[0.0, 1440.0, -200.0, 5000.5]);
    let mut glyphs = Vec::new();
    for _ in 0..count {
        let cp = pick(r, &cps);
        let gid = if r.below(7) == 0 { Some(pick(r, &[0u32, 1, 36, 70, 499, 3])) } else { None };
        glyphs.push((cp, x, gid));
        let adv = (f64::from(face.advance_width(face.glyph_id(cp))) * size_pt * 20.0) / upem;
        let j = pick(r, &JITTER);
        x = x + adv + j;
    }
    let y = pick(r, &[0.0, 240.0, 1440.5]);
    let color = pick(r, &VALID);
    let mut scale_x = OptF64::NONE;
    let mut synth_italic = false;
    if kind == 0 {
        scale_x = OptF64::some(pick(r, &[2.0, 0.5]));
    }
    if kind == 1 {
        synth_italic = true;
    }
    if kind == 2 {
        scale_x = OptF64::some(1.0);
    }
    let synth_bold = r.below(5) == 0;
    GlyphCase { font, size_pt, y, color, scale_x, synth_italic, synth_bold, glyphs }
}

impl GlyphCase {
    pub fn push(&self, list: &mut uo_render::ItemList, res: &mut Resources, fx: Option<uo_render::TextFxId>) {
        let color = res.symbols.intern_str(self.color).unwrap();
        let flags = RunFlags::default().with(RunFlags::SYNTH_ITALIC, self.synth_italic).with(RunFlags::SYNTH_BOLD, self.synth_bold);
        let head = GlyphRunHead {
            face: uo_render::FaceId::from_index(self.font).unwrap(),
            color,
            size_pt: self.size_pt,
            y: self.y,
            text: &[],
            scale_x: self.scale_x,
            alpha: OptF64::NONE,
            fx,
            flags,
        };
        list.push_glyphs(head, self.glyphs.iter().copied()).unwrap();
    }
}

// ── fx runs (JS: genFxGrad, genFxFill) ──────────────────────────────────────────────────────────

const GRAD_PATHS: [Option<&str>; 5] = [None, None, Some("circle"), Some("path"), Some("rect")];

fn gen_fx_grad(r: &mut Rng, syms: &mut uo_render::SymbolTable) -> ResolvedFill {
    let count = 2 + r.below(2);
    let mut stops = Vec::new();
    for _ in 0..count {
        let pos = pick(r, &POS);
        let hex = syms.intern_str(pick(r, &VALID)).unwrap();
        let alpha = pick(r, &[1.0, 0.5, 0.0]);
        stops.push(FxStop { pos, hex, alpha });
    }
    let angle_deg = pick(r, &[0.0, 45.0, 90.0, 135.0, 270.0, 30.5]);
    let scaled = boolean(r);
    let path = pick(r, &GRAD_PATHS).map(|p| syms.intern_str(p).unwrap());
    let fill_to = if boolean(r) {
        let l = pick(r, &[0.0, 0.5, 0.25]);
        let t = pick(r, &[0.0, 0.5, 0.25]);
        let rr = pick(r, &[0.0, 0.5, 1.0]);
        let b = pick(r, &[0.5, 0.25, 0.0]);
        Some(Ltrb { l, t, r: rr, b })
    } else {
        None
    };
    ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to }
}

fn gen_fx_fill(r: &mut Rng, syms: &mut uo_render::SymbolTable) -> ResolvedFill {
    match r.below(4) {
        0 => ResolvedFill::None,
        1 => {
            let hex = syms.intern_str(pick(r, &VALID)).unwrap();
            let alpha = pick(r, &[1.0, 0.5]);
            ResolvedFill::Solid { hex, alpha }
        }
        _ => gen_fx_grad(r, syms),
    }
}

/// `fx = {}; if bool: fill; if bool: outline` of the capture.
pub fn gen_fx(r: &mut Rng, syms: &mut uo_render::SymbolTable) -> ResolvedTextFx {
    let mut fx = ResolvedTextFx::default();
    if boolean(r) {
        fx.fill = Some(gen_fx_fill(r, syms));
    }
    if boolean(r) {
        let width_pt = pick(r, &[0.5, 1.0, 2.25]);
        let fill = gen_fx_fill(r, syms);
        fx.outline = Some(FxOutline { width_pt, fill, dash: None, cap: LineCap::Butt, join: LineJoin::Miter });
    }
    fx
}

// ── shapes (JS: genShape) ───────────────────────────────────────────────────────────────────────

const GEOMS: [&str; 13] = [
    "rect",
    "ellipse",
    "roundRect",
    "triangle",
    "line",
    "straightConnector1",
    "custom",
    "rightArrow",
    "bentConnector3",
    "star5",
    "unknownShape",
    "frame",
    "custom",
];
const SWEEPS: [f64; 8] = [90.0, -90.0, 180.0, 270.0, 360.0, 45.0, 30.5, 0.0];

fn gen_cust(r: &mut Rng) -> Vec<CustPath> {
    let mut paths = Vec::new();
    let np = 1 + r.below(3);
    for _ in 0..np {
        let w = pick(r, &[100.0, 1.0, 21600.0, 3.5, 0.0]);
        let h = pick(r, &[100.0, 1.0, 21600.0, 3.5, 0.0]);
        let fill = boolean(r);
        let stroke = boolean(r);
        let mut cmds = Vec::new();
        let nc = 2 + r.below(7);
        for _ in 0..nc {
            match r.below(8) {
                0 | 6 => {
                    let x = num_bad(r, 3);
                    let y = num_bad(r, 3);
                    cmds.push(CustCmd::M { x, y });
                }
                1 | 7 => {
                    let x = num_bad(r, 3);
                    let y = num_bad(r, 3);
                    cmds.push(CustCmd::L { x, y });
                }
                2 => {
                    let k = r.below(5);
                    let mut pts = Vec::new();
                    for _ in 0..k {
                        let x = num_bad(r, 3);
                        let y = num_bad(r, 3);
                        pts.push([x, y]);
                    }
                    cmds.push(CustCmd::C { pts });
                }
                3 => {
                    let k = r.below(4);
                    let mut pts = Vec::new();
                    for _ in 0..k {
                        let x = num_bad(r, 3);
                        let y = num_bad(r, 3);
                        pts.push([x, y]);
                    }
                    cmds.push(CustCmd::Q { pts });
                }
                4 => {
                    let w_r = num_bad(r, 3);
                    let h_r = num_bad(r, 3);
                    let st = num_bad(r, 3);
                    let sw = pick(r, &SWEEPS);
                    cmds.push(CustCmd::A { w_r, h_r, st, sw });
                }
                _ => cmds.push(CustCmd::Z),
            }
        }
        paths.push(CustPath { w, h, fill, stroke, cmds });
    }
    paths
}

fn gen_adj(r: &mut Rng, syms: &mut uo_render::SymbolTable) -> Option<Vec<(Sym, f64)>> {
    let mut s = |n: &str| syms.intern_str(n).unwrap();
    match r.below(5) {
        0 => None,
        1 => {
            let v = num_bad(r, 3);
            Some(vec![(s("adj"), v)])
        }
        2 => {
            let a = num_bad(r, 3);
            let b = num_bad(r, 3);
            Some(vec![(s("adj1"), a), (s("adj2"), b)])
        }
        3 => {
            let a = num_bad(r, 3);
            let b = num_bad(r, 3);
            Some(vec![(s("vf"), a), (s("adj"), b)])
        }
        _ => {
            let a = num_bad(r, 3);
            let b = num_bad(r, 3);
            let c = num_bad(r, 3);
            Some(vec![(s("adj1"), a), (s("adj2"), b), (s("adj"), c)])
        }
    }
}

const SHAPE_GRAD_PATHS: [Option<&str>; 4] = [None, None, Some("circle"), Some("path")];

fn gen_grad(r: &mut Rng, syms: &mut uo_render::SymbolTable) -> GradFill {
    let count = 2 + r.below(2);
    let mut stops = Vec::new();
    for _ in 0..count {
        let pos = pick(r, &POS);
        let color = syms.intern_str(pick(r, &COLORS)).unwrap();
        stops.push(GradStop { pos, color });
    }
    let angle_deg = pick(r, &[0.0, 45.0, 90.0, 135.0, 270.0, 30.5]);
    let scaled = boolean(r);
    let rot_with_shape = boolean(r);
    let path = pick(r, &SHAPE_GRAD_PATHS).map(|p| syms.intern_str(p).unwrap());
    let fill_to = if boolean(r) {
        let l = pick(r, &[0.0, 0.5, 0.25]);
        let t = pick(r, &[0.0, 0.5, 0.25]);
        let rr = pick(r, &[0.0, 0.5, 1.0]);
        let b = pick(r, &[0.5, 0.25, 0.0]);
        Some(Ltrb { l, t, r: rr, b })
    } else {
        None
    };
    GradFill { stops, angle_deg, scaled, rot_with_shape, path, fill_to }
}

/// A generated shape and whether the canvas paints it through the effects path.
pub struct ShapeCase {
    pub item: ShapeItem,
    pub effects: bool,
}

pub fn gen_shape(r: &mut Rng, res: &mut Resources) -> ShapeCase {
    let geom_name = pick(r, &GEOMS);
    let flip_h = boolean(r);
    let flip_v = boolean(r);
    let adj = gen_adj(r, &mut res.symbols);
    let rot = pick(r, &[None, None, Some(0.0), Some(15.0), Some(90.0), Some(-30.0), Some(45.5)]);
    let cust = if geom_name == "custom" {
        let paths = gen_cust(r);
        Some(res.add_cust(paths.into()).unwrap())
    } else {
        None
    };
    let x = num_bad(r, 4);
    let y = num_bad(r, 4);
    let w = num_bad(r, 4);
    let h = num_bad(r, 4);
    let fill = pick(r, &[None, Some("FF0000"), Some("zz"), Some("00B050")]);
    let stroke = if r.below(3) != 0 {
        let color = pick(r, &["000000", "FF0000", "qq", "12AB34"]);
        let width_pt = pick(r, &[0.5, 1.0, 3.0, 0.0]);
        let dash = if boolean(r) { Some(pick(r, &["dash", ""])) } else { None };
        Some((color, width_pt, dash))
    } else {
        None
    };
    let grad = if r.below(3) == 0 { Some(gen_grad(r, &mut res.symbols)) } else { None };
    let shadow = if r.below(4) == 0 {
        let dx = num_bad(r, 3);
        let dy = num_bad(r, 3);
        let color = pick(r, &["808080", "zz"]);
        let alpha = pick(r, &[1.0, 0.5]);
        Some((dx, dy, color, alpha))
    } else {
        None
    };
    let line = if ["line", "straightConnector1", "bentConnector3"].contains(&geom_name) && boolean(r) {
        let x1 = num_bad(r, 3);
        let y1 = num_bad(r, 3);
        let x2 = num_bad(r, 3);
        let y2 = num_bad(r, 3);
        Some([x1, y1, x2, y2])
    } else {
        None
    };
    let has_fx = r.below(5) == 0;
    let mut sym = |s: &str| res.symbols.intern_str(s).unwrap();
    let geom = ShapeGeom {
        geom: sym(geom_name),
        adj,
        cust,
        flip_h,
        flip_v,
        rot: rot.map_or(OptF64::NONE, OptF64::some),
        fill_alpha: OptF64::NONE,
        line_alpha: OptF64::NONE,
        eval: GeomEval::Compact,
    };
    let item = ShapeItem {
        x,
        y,
        w,
        h,
        geom,
        fill: fill.map(&mut sym),
        stroke: stroke.map(|(c, wp, d)| Stroke { color: sym(c), width_pt: wp, dash: d.map(&mut sym) }),
        line,
        grad,
        shadow: shadow.map(|(dx, dy, c, a)| ShapeShadow { dx, dy, color: sym(c), alpha: OptF64::some(a), blur: OptF64::NONE }),
        pattern: None,
        blip: None,
        fx: if has_fx { Some(uo_render::types::ShapeFx::default()) } else { None },
    };
    let effects = has_fx && geom_name != "line";
    ShapeCase { item, effects }
}

pub fn page_of(list: uo_render::ItemList) -> Page {
    Page { w: 12240.0, h: 15840.0, list, background: None, background_fill: None, background_show: false }
}

pub fn item_shape(sh: ShapeItem) -> Item {
    Item::Shape(Box::new(sh))
}

#[allow(dead_code)]
pub fn cmd_count(c: &[PathCmd]) -> usize {
    c.len()
}
