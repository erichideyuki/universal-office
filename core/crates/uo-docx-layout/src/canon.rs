//! The P1 canonical stream of a [`Layout`] (record §5): the same item sequence as the TS walker
//! `tools/lib/canon.mjs` writes from a TS `LayoutResult` — its header comment is the
//! specification of both. Faces and images are labelled by first appearance (the TS by object
//! identity, here by `FaceId`/`ImageId`, which are the same identities, K2a and RQ3), `lineBox.line`
//! is relabelled by first appearance (LA3), refs are printed as the TS strings, and the run of a
//! placed object by the caller's `run_name` (the model position of a `RunRef`, printed the way
//! `runRefs(full)` of the walker does).

use rustc_hash::FxHashMap;

use crate::keys::LineSeq;
use crate::model::RunRef;
use crate::output::{Layout, LayoutPage, LineGeom, PlacedObject};
use uo_render::types::{
    CustCmd, FxOutline, GradFill, Item, Ltrb, PageFill, PageFillKind, PathCmd, PicGlow, PicReflection, PicShadow, PictureLook,
    ResolvedFill, ResolvedTextFx, ShadowKind, ShapeFx, ShapeItem, Stroke,
};
use uo_render::{FaceId, ImageId, OptF64, Resources, Sym};

/// Version of the stream (`CANON_VERSION` of canon.mjs).
pub const CANON_VERSION: u32 = 1;

/// Where the stream goes: a uo-testkit `Stream` (tests), a raw buffer (the adapter check).
pub trait CanonSink {
    fn tag(&mut self, c: u8);
    fn u32(&mut self, v: u32);
    /// an f64 by its bits (the caller canonicalizes NaN through [`Canon::num`])
    fn f64(&mut self, v: f64);
    fn bool(&mut self, b: bool);
    fn bytes(&mut self, b: &[u8]);
    /// length and SHA-256
    fn digest_of(&mut self, b: &[u8]);
}

/// The writer of one layout's stream.
pub struct Canon<'s, 'l> {
    s: &'s mut dyn CanonSink,
    res: &'l Resources,
    faces: FxHashMap<FaceId, u32>,
    images: FxHashMap<ImageId, u32>,
    lines: FxHashMap<LineSeq, u32>,
    run_name: &'l dyn Fn(&RunRef) -> Option<Vec<u16>>,
}

/// Writes the canonical stream of `layout`.
pub fn canon_layout(layout: &Layout, run_name: &dyn Fn(&RunRef) -> Option<Vec<u16>>, s: &mut dyn CanonSink) {
    let mut c = Canon {
        s,
        res: &layout.resources,
        faces: FxHashMap::default(),
        images: FxHashMap::default(),
        lines: FxHashMap::default(),
        run_name,
    };
    c.layout(layout);
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Canon<'_, '_> {
    fn num(&mut self, v: f64) {
        self.s.f64(uo_jsnum::conv::canonicalize_nan(v));
    }

    fn opt_num(&mut self, v: Option<f64>) {
        self.s.bool(v.is_some());
        if let Some(v) = v {
            self.num(v);
        }
    }

    fn opt(&mut self, v: OptF64) {
        self.opt_num(v.get());
    }

    fn u16s(&mut self, u: &[u16]) {
        let mut b = Vec::with_capacity(u.len() * 2);
        for &c in u {
            b.extend_from_slice(&c.to_le_bytes());
        }
        self.s.bytes(&b);
    }

    fn sym(&mut self, s: Sym) {
        let u = self.res.sym(s).to_vec();
        self.u16s(&u);
    }

    fn opt_sym(&mut self, s: Option<Sym>) {
        self.s.bool(s.is_some());
        if let Some(s) = s {
            self.sym(s);
        }
    }

    fn str(&mut self, s: &str) {
        self.u16s(&units(s));
    }

    fn layout(&mut self, l: &Layout) {
        self.s.tag(b'L');
        self.s.u32(CANON_VERSION);
        self.s.u32(len32(l.pages.len()));
        for p in &l.pages {
            self.page(p);
        }
        self.s.u32(len32(l.warnings.len()));
        for w in &l.warnings {
            self.str(w);
        }
    }

    fn page(&mut self, p: &LayoutPage) {
        self.s.tag(b'P');
        self.num(p.page.w);
        self.num(p.page.h);
        self.num(f64::from(p.section));
        self.num(p.number);
        self.opt_num(p.hf_items.map(f64::from));
        self.opt_sym(p.page.background);
        self.s.bool(p.page.background_show);
        let fill = p.page.background_fill.and_then(|id| self.res.page_fill(id));
        self.s.bool(fill.is_some());
        if let Some(f) = fill {
            self.page_fill(f);
        }
        let list = &p.page.list;
        self.s.u32(len32(list.items.len()));
        for (i, it) in list.items.iter().enumerate() {
            self.item(p, i, it);
        }
        self.s.u32(len32(p.lines.len()));
        for l in &p.lines {
            self.line(p, l);
        }
        self.s.bool(p.objects.is_some());
        if let Some(os) = &p.objects {
            self.s.u32(len32(os.len()));
            for o in os {
                self.object(p, o);
            }
        }
        self.s.bool(p.cells.is_some());
        if let Some(cs) = &p.cells {
            self.s.u32(len32(cs.len()));
            for c in cs {
                self.s.tag(b'k');
                self.str(&c.at.to_ts());
                for v in [c.x, c.y, c.w, c.h, c.span] {
                    self.num(v);
                }
            }
        }
    }

    fn face(&mut self, id: FaceId) {
        let next = len32(self.faces.len());
        let label = *self.faces.entry(id).or_insert(next);
        self.s.u32(label);
        if label != next {
            return;
        }
        let Some(f) = self.res.face(id) else {
            // an id of another table: an empty face (the TS walker writes `f?.key ?? ''`)
            for _ in 0..2 {
                self.u16s(&[]);
            }
            for _ in 0..6 {
                self.s.bool(false);
            }
            return;
        };
        let (key, family) = (f.key, f.family);
        self.sym(key);
        self.sym(family);
        for b in [f.bold, f.italic, f.synth_bold, f.synth_italic, f.stand_in] {
            self.s.bool(b);
        }
        self.s.bool(f.bytes.is_some());
        if let Some(b) = &f.bytes {
            self.s.digest_of(b);
        }
    }

    fn image(&mut self, id: ImageId) {
        let next = len32(self.images.len());
        let label = *self.images.entry(id).or_insert(next);
        self.s.u32(label);
        if label != next {
            return;
        }
        let Some(im) = self.res.image(id) else {
            self.u16s(&[]);
            self.s.digest_of(&[]);
            self.s.bool(false);
            self.num(f64::NAN);
            self.num(f64::NAN);
            self.s.bool(false);
            return;
        };
        self.sym(im.ext);
        self.s.digest_of(&im.bytes);
        self.s.bool(im.svg.is_some());
        if let Some(svg) = &im.svg {
            self.s.digest_of(svg);
        }
        self.num(im.width_twips);
        self.num(im.height_twips);
        self.s.bool(im.effects.is_some());
        if let Some(e) = &im.effects {
            self.s.bool(e.gray);
            self.opt_num(e.gain);
            self.opt_num(e.bright);
            self.s.bool(e.floor);
            self.opt_num(e.bi_level);
        }
    }

    fn ltrb(&mut self, b: &Ltrb) {
        for v in [b.l, b.t, b.r, b.b] {
            self.num(v);
        }
    }

    fn adj(&mut self, a: &[(Sym, f64)]) {
        self.s.u32(len32(a.len()));
        for &(k, v) in a {
            self.sym(k);
            self.num(v);
        }
    }

    fn stroke(&mut self, st: &Stroke) {
        self.sym(st.color);
        self.num(st.width_pt);
        self.opt_sym(st.dash);
    }

    fn fill(&mut self, f: &ResolvedFill) {
        match f {
            ResolvedFill::None => self.s.tag(b'n'),
            ResolvedFill::Solid { hex, alpha } => {
                self.s.tag(b's');
                self.sym(*hex);
                self.num(*alpha);
            }
            ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to } => {
                self.s.tag(b'g');
                self.s.u32(len32(stops.len()));
                for st in stops {
                    self.num(st.pos);
                    self.sym(st.hex);
                    self.num(st.alpha);
                }
                self.num(*angle_deg);
                self.s.bool(*scaled);
                self.opt_sym(*path);
                self.s.bool(fill_to.is_some());
                if let Some(b) = fill_to {
                    self.ltrb(b);
                }
            }
        }
    }

    fn text_fx(&mut self, fx: &ResolvedTextFx) {
        self.s.bool(fx.fill.is_some());
        if let Some(f) = &fx.fill {
            self.fill(f);
        }
        self.s.bool(fx.outline.is_some());
        if let Some(FxOutline { width_pt, fill, dash, cap, join }) = &fx.outline {
            self.num(*width_pt);
            self.fill(fill);
            self.opt_sym(*dash);
            self.str(match cap {
                uo_render::types::LineCap::Butt => "butt",
                uo_render::types::LineCap::Round => "round",
                uo_render::types::LineCap::Square => "square",
            });
            self.str(match join {
                uo_render::types::LineJoin::Miter => "miter",
                uo_render::types::LineJoin::Round => "round",
                uo_render::types::LineJoin::Bevel => "bevel",
            });
        }
        self.s.bool(fx.shadow.is_some());
        if let Some(h) = &fx.shadow {
            for v in [h.blur_pt, h.dist_pt, h.dir_deg, h.sx, h.sy, h.kx_deg, h.ky_deg] {
                self.num(v);
            }
            self.sym(h.algn);
            self.sym(h.hex);
            self.num(h.alpha);
        }
        self.s.bool(fx.glow.is_some());
        if let Some(g) = &fx.glow {
            self.num(g.rad_pt);
            self.sym(g.hex);
            self.num(g.alpha);
        }
        self.s.bool(fx.reflection.is_some());
        if let Some(r) = &fx.reflection {
            for v in [r.blur_pt, r.st_a, r.st_pos, r.end_a, r.end_pos, r.dist_pt, r.dir_deg] {
                self.num(v);
            }
            self.sym(r.algn);
        }
    }

    fn shadow(&mut self, h: &PicShadow) {
        self.str(match h.kind {
            ShadowKind::Outer => "outer",
            ShadowKind::Inner => "inner",
        });
        self.sym(h.color);
        for v in [h.alpha, h.blur_pt, h.dist_pt, h.dir_deg] {
            self.num(v);
        }
        for v in [h.sx, h.sy, h.kx_deg, h.ky_deg] {
            self.opt(v);
        }
        self.opt_sym(h.algn);
        self.s.bool(h.rot_with_shape);
    }

    fn glow(&mut self, g: &PicGlow) {
        self.sym(g.color);
        self.num(g.alpha);
        self.num(g.rad_pt);
    }

    fn reflection(&mut self, r: &PicReflection) {
        for v in [r.blur_pt, r.st_a, r.st_pos, r.end_a, r.end_pos, r.dist_pt, r.dir_deg, r.sy] {
            self.num(v);
        }
        self.opt_sym(r.algn);
        self.s.bool(r.rot_with_shape);
    }

    fn pic(&mut self, p: &PictureLook) {
        self.s.bool(p.geom.is_some());
        if let Some(g) = &p.geom {
            self.sym(g.prst);
            self.s.bool(g.adj.is_some());
            if let Some(a) = &g.adj {
                self.adj(a);
            }
        }
        self.s.bool(p.line.is_some());
        if let Some(l) = &p.line {
            self.sym(l.color);
            self.num(l.alpha);
            self.num(l.width_pt);
            for s in [l.dash, l.cmpd, l.cap, l.join] {
                self.opt_sym(s);
            }
        }
        self.s.bool(p.shadow.is_some());
        if let Some(h) = &p.shadow {
            self.shadow(h);
        }
        self.s.bool(p.glow.is_some());
        if let Some(g) = &p.glow {
            self.glow(g);
        }
        self.opt(p.soft_edge_pt);
        self.s.bool(p.bevel.is_some());
        if let Some(b) = &p.bevel {
            self.num(b.w_pt);
            self.num(b.h_pt);
            for s in [b.prst, b.rig, b.dir, b.material] {
                self.opt_sym(s);
            }
        }
        self.s.bool(p.reflection.is_some());
        if let Some(r) = &p.reflection {
            self.reflection(r);
        }
        self.opt(p.alpha);
        self.s.bool(p.duotone.is_some());
        if let Some([a, b]) = p.duotone {
            self.sym(a);
            self.sym(b);
        }
        self.s.bool(p.clr_change.is_some());
        if let Some(c) = &p.clr_change {
            self.sym(c.from);
            self.sym(c.to);
            self.num(c.to_alpha);
        }
    }

    fn shape_fx(&mut self, f: &ShapeFx) {
        self.s.bool(f.shadow.is_some());
        if let Some(h) = &f.shadow {
            self.shadow(h);
        }
        self.s.bool(f.glow.is_some());
        if let Some(g) = &f.glow {
            self.glow(g);
        }
        self.opt(f.soft_edge_pt);
        self.s.bool(f.reflection.is_some());
        if let Some(r) = &f.reflection {
            self.reflection(r);
        }
    }

    fn grad(&mut self, g: &GradFill) {
        self.s.u32(len32(g.stops.len()));
        for st in &g.stops {
            self.num(st.pos);
            self.sym(st.color);
        }
        self.num(g.angle_deg);
        self.s.bool(g.scaled);
        self.s.bool(g.rot_with_shape);
        self.opt_sym(g.path);
        self.s.bool(g.fill_to.is_some());
        if let Some(b) = &g.fill_to {
            self.ltrb(b);
        }
    }

    fn shape(&mut self, sh: &ShapeItem) {
        for v in [sh.x, sh.y, sh.w, sh.h] {
            self.num(v);
        }
        let g = &sh.geom;
        self.sym(g.geom);
        self.s.bool(g.adj.is_some());
        if let Some(a) = &g.adj {
            self.adj(a);
        }
        let cust = g.cust.and_then(|id| self.res.cust(id)).cloned();
        self.s.bool(cust.is_some());
        if let Some(paths) = cust {
            self.s.u32(len32(paths.len()));
            for p in paths.iter() {
                self.num(p.w);
                self.num(p.h);
                self.s.bool(p.fill);
                self.s.bool(p.stroke);
                self.s.u32(len32(p.cmds.len()));
                for k in &p.cmds {
                    match k {
                        CustCmd::M { x, y } | CustCmd::L { x, y } => {
                            self.s.tag(if matches!(k, CustCmd::M { .. }) { b'M' } else { b'L' });
                            self.num(*x);
                            self.num(*y);
                        }
                        CustCmd::C { pts } | CustCmd::Q { pts } => {
                            self.s.tag(if matches!(k, CustCmd::C { .. }) { b'C' } else { b'Q' });
                            self.s.u32(len32(pts.len()));
                            for [px, py] in pts {
                                self.num(*px);
                                self.num(*py);
                            }
                        }
                        CustCmd::A { w_r, h_r, st, sw } => {
                            self.s.tag(b'A');
                            for v in [*w_r, *h_r, *st, *sw] {
                                self.num(v);
                            }
                        }
                        CustCmd::Z => self.s.tag(b'Z'),
                    }
                }
            }
        }
        self.s.bool(g.flip_h);
        self.s.bool(g.flip_v);
        self.opt(g.rot);
        self.opt(g.fill_alpha);
        self.opt(g.line_alpha);
        self.opt_sym(sh.fill);
        self.s.bool(sh.stroke.is_some());
        if let Some(st) = &sh.stroke {
            self.stroke(st);
        }
        self.s.bool(sh.line.is_some());
        if let Some(l) = sh.line {
            for v in l {
                self.num(v);
            }
        }
        self.s.bool(sh.grad.is_some());
        if let Some(gr) = &sh.grad {
            self.grad(gr);
        }
        self.s.bool(sh.shadow.is_some());
        if let Some(h) = &sh.shadow {
            self.num(h.dx);
            self.num(h.dy);
            self.sym(h.color);
            self.opt(h.alpha);
            self.opt(h.blur);
        }
        self.s.bool(sh.pattern.is_some());
        if let Some(p) = &sh.pattern {
            self.sym(p.prst);
            self.sym(p.fg);
            self.sym(p.bg);
        }
        self.s.bool(sh.blip.is_some());
        if let Some(b) = &sh.blip {
            self.image(b.image);
            self.s.bool(b.tile.is_some());
            if let Some(t) = &b.tile {
                for v in [t.tx, t.ty, t.sx, t.sy] {
                    self.num(v);
                }
                self.sym(t.algn);
                self.sym(t.flip);
            }
            self.opt(b.alpha);
        }
        self.s.bool(sh.fx.is_some());
        if let Some(f) = &sh.fx {
            self.shape_fx(f);
        }
    }

    fn cmd(&mut self, c: &PathCmd) {
        match *c {
            PathCmd::M { x, y } => {
                self.s.tag(b'M');
                self.num(x);
                self.num(y);
            }
            PathCmd::L { x, y } => {
                self.s.tag(b'L');
                self.num(x);
                self.num(y);
            }
            PathCmd::C { x1, y1, x2, y2, x, y } => {
                self.s.tag(b'C');
                for v in [x1, y1, x2, y2, x, y] {
                    self.num(v);
                }
            }
            PathCmd::Z => self.s.tag(b'Z'),
        }
    }

    fn item(&mut self, p: &LayoutPage, i: usize, it: &Item) {
        let list = &p.page.list;
        match it {
            Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
                self.s.tag(b'g');
                self.face(*face);
                self.num(*size_pt);
                self.sym(*color);
                self.num(*y);
                self.s.u32(glyphs.len);
                for (cp, x, gid) in list.glyphs_of(*glyphs) {
                    self.num(f64::from(cp));
                    self.num(x);
                    self.opt_num(gid.map(f64::from));
                }
                let side = list.run(*run).copied().unwrap_or_default();
                self.opt(side.scale_x);
                let text = list.text_of(side.text).to_vec();
                self.u16s(&text);
                use uo_render::types::RunFlags as F;
                self.s.bool(flags.has(F::SYNTH_BOLD));
                self.s.bool(flags.has(F::SYNTH_ITALIC));
                self.opt(side.alpha);
                self.s.bool(flags.has(F::AS_PATH));
                self.s.bool(flags.has(F::COLR));
                let fx = side.fx.and_then(|id| self.res.text_fx(id));
                self.s.bool(fx.is_some());
                if let Some(fx) = fx {
                    self.text_fx(fx);
                }
                let meta = item_meta(p, i);
                let lb = meta.and_then(|m| m.line_box);
                self.s.bool(lb.is_some());
                if let Some(b) = lb {
                    self.num(b.asc);
                    self.num(b.desc);
                    self.num(b.lead);
                    let next = len32(self.lines.len());
                    let label = *self.lines.entry(b.line).or_insert(next);
                    self.s.u32(label);
                }
                self.s.bool(meta.is_some_and(|m| m.line_num));
            }
            Item::Rect { x, y, w, h, fill, alpha } => {
                self.s.tag(b'r');
                for v in [*x, *y, *w, *h] {
                    self.num(v);
                }
                self.sym(*fill);
                self.opt(*alpha);
            }
            Item::Line { x1, y1, x2, y2, color, width_pt, dash } => {
                self.s.tag(b'l');
                for v in [*x1, *y1, *x2, *y2] {
                    self.num(v);
                }
                self.sym(*color);
                self.num(*width_pt);
                self.s.bool(dash.is_some());
                if let Some(d) = dash {
                    self.str(d.name());
                }
            }
            Item::Image(im) => {
                self.s.tag(b'i');
                for v in [im.x, im.y, im.w, im.h] {
                    self.num(v);
                }
                self.image(im.image);
                self.s.bool(im.crop.is_some());
                if let Some(c) = &im.crop {
                    self.ltrb(c);
                }
                self.opt(im.rot);
                self.s.bool(im.flip_h);
                self.s.bool(im.flip_v);
                self.s.bool(im.pic.is_some());
                if let Some(p) = &im.pic {
                    self.pic(p);
                }
            }
            Item::Shape(sh) => {
                self.s.tag(b's');
                self.shape(sh);
            }
            Item::Path { cmds, fill, stroke, even_odd } => {
                self.s.tag(b'p');
                let cs = list.cmds_of(*cmds);
                self.s.u32(len32(cs.len()));
                for c in cs {
                    self.cmd(c);
                }
                self.opt_sym(*fill);
                self.s.bool(stroke.is_some());
                if let Some(st) = stroke {
                    self.stroke(st);
                }
                self.s.bool(*even_odd);
            }
            Item::ClipPush { x, y, w, h } => {
                self.s.tag(b'c');
                self.s.bool(true);
                for v in [*x, *y, *w, *h] {
                    self.opt(v);
                }
                self.clip_src(p, i);
            }
            Item::ClipPop => {
                self.s.tag(b'c');
                self.s.bool(false);
                self.clip_src(p, i);
            }
            Item::XformPush { m } => {
                self.s.tag(b'x');
                self.s.bool(true);
                self.s.bool(m.is_some());
                if let Some(m) = m {
                    for v in **m {
                        self.num(v);
                    }
                }
            }
            Item::XformPop => {
                self.s.tag(b'x');
                self.s.bool(false);
                self.s.bool(false);
            }
        }
    }

    fn clip_src(&mut self, p: &LayoutPage, i: usize) {
        let src = p.clip_src.iter().find(|(k, _)| *k as usize == i).map(|&(_, id)| id);
        self.s.bool(src.is_some());
        if let Some(id) = src {
            self.image(id);
        }
    }

    fn line(&mut self, p: &LayoutPage, l: &LineGeom) {
        self.s.tag(b'n');
        self.str(&l.at.to_ts());
        self.num(f64::from(l.start));
        self.num(f64::from(l.end));
        for v in [l.x, l.y, l.height, l.baseline, l.width] {
            self.num(v);
        }
        self.s.bool(l.chars.is_some());
        if let Some(span) = l.chars {
            self.s.u32(span.len);
            if let Some(r) = span.range() {
                for k in r {
                    self.num(p.chars.o.get(k).map_or(f64::NAN, |&o| f64::from(o)));
                    self.num(p.chars.x.get(k).copied().unwrap_or(f64::NAN));
                    self.num(p.chars.w.get(k).copied().unwrap_or(f64::NAN));
                }
            }
        }
        self.opt_num(l.hyph.map(f64::from));
        self.s.bool(l.rev.is_some());
        if let Some((a, b)) = l.rev {
            self.num(a);
            self.num(b);
        }
        self.s.bool(l.frame.is_some());
        if let Some((a, b)) = l.frame {
            self.num(a);
            self.num(b);
        }
        let pf = l.pfmt.and_then(|k| p.pfmts.get(k as usize));
        self.s.bool(pf.is_some());
        if let Some(pf) = pf {
            self.num(pf.left);
            self.num(pf.right);
            self.num(pf.first_line);
            self.s.u32(len32(pf.tabs.len()));
            for t in pf.tabs.iter() {
                self.str(t.val.name());
                self.num(t.pos);
                self.s.bool(t.leader.is_some());
                if let Some(ld) = t.leader {
                    self.str(ld.name());
                }
            }
        }
    }

    fn object(&mut self, p: &LayoutPage, o: &PlacedObject) {
        self.s.tag(b'o');
        let name = o.run.as_ref().and_then(|r| (self.run_name)(r));
        self.s.bool(name.is_some());
        if let Some(n) = name {
            self.u16s(&n);
        }
        for v in [o.x, o.y, o.w, o.h] {
            self.num(v);
        }
        self.str(match o.layer {
            crate::output::Layer::Behind => "behind",
            crate::output::Layer::Front => "front",
        });
        self.s.bool(o.hf);
        self.s.bool(o.frame.is_some());
        if let Some(f) = &o.frame {
            for v in [f.col_x, f.col_w, f.para_top, f.page_w, f.page_h, f.mar_l, f.mar_r, f.mar_t, f.mar_b, f.body_top] {
                self.num(v);
            }
        }
        self.s.bool(o.content.is_some());
        if let Some((range, xf)) = &o.content {
            let lines = p.content_lines.get(range.start as usize..range.end as usize).unwrap_or(&[]);
            self.s.u32(len32(lines.len()));
            for l in lines {
                self.line(p, l);
            }
            for v in xf {
                self.num(*v);
            }
        }
    }

    fn page_fill(&mut self, f: &PageFill) {
        self.str(match f.kind {
            PageFillKind::Gradient => "gradient",
            PageFillKind::GradientRadial => "gradientRadial",
            PageFillKind::Pattern => "pattern",
            PageFillKind::Frame => "frame",
            PageFillKind::Tile => "tile",
        });
        self.sym(f.c1);
        self.sym(f.c2);
        self.s.bool(f.stops.is_some());
        if let Some(st) = &f.stops {
            self.s.u32(len32(st.len()));
            for x in st {
                self.num(x.pos);
                self.sym(x.color);
            }
        }
        self.num(f.angle);
        self.num(f.focus);
        self.s.bool(f.focus_position.is_some());
        if let Some([a, b]) = f.focus_position {
            self.num(a);
            self.num(b);
        }
        self.s.bool(f.gradient_center);
        self.s.bool(f.image.is_some());
        if let Some(id) = f.image {
            self.image(id);
        }
        self.s.bool(f.image_px.is_some());
        if let Some(px) = f.image_px {
            for v in px {
                self.num(v);
            }
        }
        self.s.bool(f.pattern.is_some());
        if let Some(rows) = &f.pattern {
            self.s.u32(len32(rows.len()));
            for &r in rows {
                self.num(r);
            }
        }
    }
}

fn item_meta(p: &LayoutPage, i: usize) -> Option<crate::output::GlyphMeta> {
    let k = u32::try_from(i).ok()?;
    p.item_meta.binary_search_by_key(&k, |&(j, _)| j).ok().and_then(|at| p.item_meta.get(at)).map(|&(_, m)| m)
}

fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
