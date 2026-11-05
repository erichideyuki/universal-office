//! Rust display-list types -> the generated wire types (`crate::codec`), without the canvas
//! hints (those are `hints.rs`). Ids become table indices (`raw - 1`); every id met is recorded in
//! [`Refs`] so the encoder can send the table entries a page needs and refuse an id the
//! [`Resources`] do not have.

use super::hints::{fx_hints, glyph_hints, shape_hints};
use super::{CanvasOptions, EncodeError};
use crate::codec as w;
use crate::ids::Sym;
use crate::list::{ItemList, NO_GID, Span};
use crate::opt::OptF64;
use crate::resources::{FaceRes, ImageRes, Resources};
use crate::types as t;

/// How many entries of each table the converted values refer to (`max index + 1`).
#[derive(Clone, Copy, Default, Debug)]
pub struct Refs {
    pub sym: usize,
    pub faces: usize,
    pub images: usize,
    pub text_fx: usize,
    pub page_fills: usize,
    pub cust: usize,
}

impl Refs {
    pub fn as_array(&self) -> [usize; 6] {
        [self.sym, self.faces, self.images, self.text_fx, self.page_fills, self.cust]
    }
}

pub struct Conv<'a> {
    pub res: &'a Resources,
    pub refs: Refs,
    pub opts: &'a CanvasOptions,
}

fn of(v: OptF64) -> Option<f64> {
    v.get()
}

impl<'a> Conv<'a> {
    pub fn new(res: &'a Resources, opts: &'a CanvasOptions) -> Conv<'a> {
        Conv { res, refs: Refs::default(), opts }
    }

    fn idx(raw: u32) -> u32 {
        raw.saturating_sub(1)
    }

    pub fn sym(&mut self, s: Sym) -> u32 {
        let i = s.index();
        self.refs.sym = self.refs.sym.max(i.saturating_add(1));
        Self::idx(s.raw())
    }

    fn osym(&mut self, s: Option<Sym>) -> Option<u32> {
        s.map(|s| self.sym(s))
    }

    fn pairs(&mut self, a: &Option<Vec<(Sym, f64)>>) -> Option<Vec<w::AdjValue>> {
        a.as_ref().map(|v| v.iter().map(|&(n, value)| w::AdjValue { name: self.sym(n), value }).collect())
    }

    fn ltrb(l: t::Ltrb) -> w::Ltrb {
        w::Ltrb { l: l.l, t: l.t, r: l.r, b: l.b }
    }

    fn image_id(&mut self, i: crate::ids::ImageId) -> u32 {
        self.refs.images = self.refs.images.max(i.index().saturating_add(1));
        Self::idx(i.raw())
    }

    fn stroke(&mut self, s: &t::Stroke) -> w::Stroke {
        w::Stroke { color: self.sym(s.color), width_pt: s.width_pt, dash: self.osym(s.dash) }
    }

    fn shadow(&mut self, s: &t::PicShadow) -> w::PicShadow {
        w::PicShadow {
            kind: match s.kind {
                t::ShadowKind::Outer => w::ShadowKind::Outer,
                t::ShadowKind::Inner => w::ShadowKind::Inner,
            },
            color: self.sym(s.color),
            alpha: s.alpha,
            blur_pt: s.blur_pt,
            dist_pt: s.dist_pt,
            dir_deg: s.dir_deg,
            sx: of(s.sx),
            sy: of(s.sy),
            kx_deg: of(s.kx_deg),
            ky_deg: of(s.ky_deg),
            algn: self.osym(s.algn),
            rot_with_shape: s.rot_with_shape,
        }
    }

    fn glow(&mut self, g: &t::PicGlow) -> w::PicGlow {
        w::PicGlow { color: self.sym(g.color), alpha: g.alpha, rad_pt: g.rad_pt }
    }

    fn reflection(&mut self, r: &t::PicReflection) -> w::PicReflection {
        w::PicReflection {
            blur_pt: r.blur_pt,
            st_a: r.st_a,
            st_pos: r.st_pos,
            end_a: r.end_a,
            end_pos: r.end_pos,
            dist_pt: r.dist_pt,
            dir_deg: r.dir_deg,
            sy: r.sy,
            algn: self.osym(r.algn),
            rot_with_shape: r.rot_with_shape,
        }
    }

    fn look(&mut self, l: &t::PictureLook) -> w::PictureLook {
        w::PictureLook {
            geom: l.geom.as_ref().map(|g| w::PresetSpec { prst: self.sym(g.prst), adj: self.pairs(&g.adj) }),
            line: l.line.as_ref().map(|p| w::PicLine {
                color: self.sym(p.color),
                alpha: p.alpha,
                width_pt: p.width_pt,
                dash: self.osym(p.dash),
                cmpd: self.osym(p.cmpd),
                cap: self.osym(p.cap),
                join: self.osym(p.join),
            }),
            shadow: l.shadow.as_ref().map(|s| self.shadow(s)),
            glow: l.glow.as_ref().map(|g| self.glow(g)),
            soft_edge_pt: of(l.soft_edge_pt),
            bevel: l.bevel.as_ref().map(|b| w::PicBevel {
                w_pt: b.w_pt,
                h_pt: b.h_pt,
                prst: self.osym(b.prst),
                rig: self.osym(b.rig),
                dir: self.osym(b.dir),
                material: self.osym(b.material),
            }),
            reflection: l.reflection.as_ref().map(|r| self.reflection(r)),
            alpha: of(l.alpha),
            duotone: l.duotone.map(|[a, b]| w::Duotone { first: self.sym(a), second: self.sym(b) }),
            clr_change: l.clr_change.as_ref().map(|c| w::ClrChange { from: self.sym(c.from), to: self.sym(c.to), to_alpha: c.to_alpha }),
        }
    }

    fn grad(&mut self, g: &t::GradFill) -> w::GradFill {
        w::GradFill {
            stops: g.stops.iter().map(|s| w::GradStop { pos: s.pos, color: self.sym(s.color) }).collect(),
            angle_deg: g.angle_deg,
            scaled: g.scaled,
            rot_with_shape: g.rot_with_shape,
            path: self.osym(g.path),
            fill_to: g.fill_to.map(Self::ltrb),
        }
    }

    fn shape(&mut self, sh: &t::ShapeItem) -> w::ShapeItem {
        let hints = shape_hints(sh, self.res, &self.opts.limits);
        let style = w::ShapeGeom {
            geom: self.sym(sh.geom.geom),
            adj: self.pairs(&sh.geom.adj),
            cust: sh.geom.cust.map(|c| {
                self.refs.cust = self.refs.cust.max(c.index().saturating_add(1));
                Self::idx(c.raw())
            }),
            flip_h: sh.geom.flip_h,
            flip_v: sh.geom.flip_v,
            rot: of(sh.geom.rot),
            fill_alpha: of(sh.geom.fill_alpha),
            line_alpha: of(sh.geom.line_alpha),
        };
        w::ShapeItem {
            x: sh.x,
            y: sh.y,
            w: sh.w,
            h: sh.h,
            style,
            fill: self.osym(sh.fill),
            stroke: sh.stroke.as_ref().map(|s| self.stroke(s)),
            line: sh.line.map(|[x1, y1, x2, y2]| w::Segment { x1, y1, x2, y2 }),
            grad: sh.grad.as_ref().map(|g| self.grad(g)),
            shadow: sh.shadow.as_ref().map(|s| w::ShapeShadow {
                dx: s.dx,
                dy: s.dy,
                color: self.sym(s.color),
                alpha: of(s.alpha),
                blur: of(s.blur),
            }),
            pattern: sh.pattern.as_ref().map(|p| w::Pattern { prst: self.sym(p.prst), fg: self.sym(p.fg), bg: self.sym(p.bg) }),
            blip: sh.blip.as_ref().map(|b| w::Blip {
                image: self.image_id(b.image),
                tile: b.tile.as_ref().map(|k| w::BlipTile {
                    tx: k.tx,
                    ty: k.ty,
                    sx: k.sx,
                    sy: k.sy,
                    algn: self.sym(k.algn),
                    flip: self.sym(k.flip),
                }),
                alpha: of(b.alpha),
            }),
            fx: sh.fx.as_ref().map(|f| w::ShapeFx {
                shadow: f.shadow.as_ref().map(|s| self.shadow(s)),
                glow: f.glow.as_ref().map(|g| self.glow(g)),
                soft_edge_pt: of(f.soft_edge_pt),
                reflection: f.reflection.as_ref().map(|r| self.reflection(r)),
            }),
            parts: hints.parts,
            grad_geom: hints.grad_geom,
            grad_samples: hints.grad_samples,
        }
    }

    fn path_cmds(cmds: &[t::PathCmd]) -> Vec<w::PathCmd> {
        cmds.iter()
            .map(|c| match *c {
                t::PathCmd::M { x, y } => w::PathCmd::M(w::Pt { x, y }),
                t::PathCmd::L { x, y } => w::PathCmd::L(w::Pt { x, y }),
                t::PathCmd::C { x1, y1, x2, y2, x, y } => w::PathCmd::C(w::Cubic { x1, y1, x2, y2, x, y }),
                t::PathCmd::Z => w::PathCmd::Z,
            })
            .collect()
    }

    fn glyph_columns(&self, list: &ItemList, span: Span) -> Result<w::GlyphColumns, EncodeError> {
        let r = span.range().ok_or(EncodeError::BadSpan)?;
        let cp = list.glyphs.cp.get(r.clone()).ok_or(EncodeError::BadSpan)?.to_vec();
        let x = list.glyphs.x.get(r.clone()).ok_or(EncodeError::BadSpan)?.to_vec();
        let gid = if list.glyphs.gid.is_empty() {
            vec![None; cp.len()]
        } else {
            list.glyphs.gid.get(r).ok_or(EncodeError::BadSpan)?.iter().map(|&g| if g == NO_GID { None } else { Some(g) }).collect()
        };
        Ok(w::GlyphColumns { cp, x, gid })
    }

    pub fn item(&mut self, list: &ItemList, item: &t::Item) -> Result<w::Item, EncodeError> {
        Ok(match item {
            t::Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
                let res = self.res;
                let side = list.run(*run).copied().ok_or(EncodeError::BadSpan)?;
                self.refs.faces = self.refs.faces.max(face.index().saturating_add(1));
                let glyph_cols = self.glyph_columns(list, *glyphs)?;
                let (mut joins, mut remap, mut outlines) = (None, None, None);
                if let Some(f) = res.face(*face).and_then(|f| f.face.as_ref()) {
                    let h = glyph_hints(f, *size_pt, side.scale_x, flags.has(t::RunFlags::SYNTH_ITALIC), list, *glyphs);
                    (joins, remap, outlines) = (h.joins, h.remap, h.outlines);
                }
                let fx_id = side.fx;
                if let Some(id) = fx_id {
                    self.refs.text_fx = self.refs.text_fx.max(id.index().saturating_add(1));
                }
                let (mut r#box, mut fx_fill_geom, mut fx_outline_geom) = (None, None, None);
                if fx_id.is_some() {
                    let h = fx_hints(list, item, self.res);
                    (r#box, fx_fill_geom, fx_outline_geom) = (h.r#box, h.fill_geom, h.outline_geom);
                }
                let text = if self.opts.debug_text { Some(list.text_of(side.text).to_vec()) } else { None };
                w::Item::Glyphs(w::GlyphRun {
                    face: Self::idx(face.raw()),
                    size_pt: *size_pt,
                    color: self.sym(*color),
                    y: *y,
                    glyphs: glyph_cols,
                    scale_x: of(side.scale_x),
                    synth_bold: flags.has(t::RunFlags::SYNTH_BOLD),
                    synth_italic: flags.has(t::RunFlags::SYNTH_ITALIC),
                    alpha: of(side.alpha),
                    as_path: flags.has(t::RunFlags::AS_PATH),
                    colr: flags.has(t::RunFlags::COLR),
                    fx: fx_id.map(|id| Self::idx(id.raw())),
                    text,
                    joins,
                    remap,
                    outlines,
                    r#box,
                    fx_fill_geom,
                    fx_outline_geom,
                })
            }
            t::Item::Rect { x, y, w: rw, h, fill, alpha } => {
                w::Item::Rect(w::Rect { x: *x, y: *y, w: *rw, h: *h, fill: self.sym(*fill), alpha: of(*alpha) })
            }
            t::Item::Line { x1, y1, x2, y2, color, width_pt, dash } => w::Item::Line(w::Line {
                x1: *x1,
                y1: *y1,
                x2: *x2,
                y2: *y2,
                color: self.sym(*color),
                width_pt: *width_pt,
                dash: dash.map(|d| match d {
                    t::LineDash::Dotted => w::LineDash::Dotted,
                    t::LineDash::Dashed => w::LineDash::Dashed,
                    t::LineDash::DashDot => w::LineDash::DashDot,
                    t::LineDash::DashDotDot => w::LineDash::DashDotDot,
                    t::LineDash::BdrDot => w::LineDash::BdrDot,
                    t::LineDash::BdrDash => w::LineDash::BdrDash,
                    t::LineDash::BdrDashDot => w::LineDash::BdrDashDot,
                    t::LineDash::BdrDashDotDot => w::LineDash::BdrDashDotDot,
                    t::LineDash::BdrDashSmall => w::LineDash::BdrDashSmall,
                }),
            }),
            t::Item::Image(im) => w::Item::Image(w::ImageItem {
                x: im.x,
                y: im.y,
                w: im.w,
                h: im.h,
                image: self.image_id(im.image),
                crop: im.crop.map(Self::ltrb),
                rot: of(im.rot),
                flip_h: im.flip_h,
                flip_v: im.flip_v,
                pic: im.pic.as_ref().map(|p| self.look(p)),
            }),
            t::Item::Shape(sh) => w::Item::Shape(self.shape(sh)),
            t::Item::Path { cmds, fill, stroke, even_odd } => w::Item::Path(w::PathItem {
                cmds: Self::path_cmds(cmds.range().and_then(|r| list.path_cmds.get(r)).ok_or(EncodeError::BadSpan)?),
                fill: self.osym(*fill),
                stroke: stroke.as_ref().map(|s| self.stroke(s)),
                even_odd: *even_odd,
            }),
            t::Item::ClipPush { x, y, w: cw, h } => {
                w::Item::Clip(w::Clip { op: w::StackOp::Push, x: of(*x), y: of(*y), w: of(*cw), h: of(*h) })
            }
            t::Item::ClipPop => w::Item::Clip(w::Clip { op: w::StackOp::Pop, x: None, y: None, w: None, h: None }),
            t::Item::XformPush { m } => w::Item::Xform(w::Xform { op: w::StackOp::Push, m: m.as_ref().map(|m| m.to_vec()) }),
            t::Item::XformPop => w::Item::Xform(w::Xform { op: w::StackOp::Pop, m: None }),
        })
    }

    // ── resource entries ────────────────────────────────────────────────────────────────────────

    pub fn face(&mut self, f: &FaceRes) -> w::Face {
        let upem = f.face.as_ref().map_or(1000, |x| {
            let u = x.metrics().units_per_em;
            if u == 0 { 1000 } else { u32::from(u) }
        });
        w::Face {
            key: self.sym(f.key),
            family: self.sym(f.family),
            bold: f.bold,
            italic: f.italic,
            synth_bold: f.synth_bold,
            synth_italic: f.synth_italic,
            stand_in: f.stand_in,
            has_bytes: f.bytes.is_some(),
            upem,
        }
    }

    pub fn image(&mut self, im: &ImageRes) -> w::Image {
        w::Image {
            ext: self.sym(im.ext),
            byte_len: u32::try_from(im.bytes.len()).unwrap_or(u32::MAX),
            has_svg: im.svg.is_some(),
            width_twips: im.width_twips,
            height_twips: im.height_twips,
            natural: im.natural_px.map(|(w_, h)| w::PxSize { w: w_, h }),
            effects: im.effects.as_ref().map(|e| w::ImageEffects {
                gray: e.gray,
                gain: e.gain,
                bright: e.bright,
                floor: e.floor,
                bi_level: e.bi_level,
            }),
        }
    }

    fn resolved_fill(&mut self, f: &t::ResolvedFill) -> w::ResolvedFill {
        match f {
            t::ResolvedFill::None => w::ResolvedFill::None,
            t::ResolvedFill::Solid { hex, alpha } => w::ResolvedFill::Solid(w::SolidFill { hex: self.sym(*hex), alpha: *alpha }),
            t::ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to } => w::ResolvedFill::Grad(w::GradTextFill {
                stops: stops.iter().map(|s| w::FxStop { pos: s.pos, hex: self.sym(s.hex), alpha: s.alpha }).collect(),
                angle_deg: *angle_deg,
                scaled: *scaled,
                path: self.osym(*path),
                fill_to: fill_to.map(Self::ltrb),
            }),
        }
    }

    pub fn text_fx(&mut self, fx: &t::ResolvedTextFx) -> w::TextFx {
        w::TextFx {
            fill: fx.fill.as_ref().map(|f| self.resolved_fill(f)),
            outline: fx.outline.as_ref().map(|o| w::FxOutline {
                width_pt: o.width_pt,
                fill: self.resolved_fill(&o.fill),
                dash: self.osym(o.dash),
                cap: match o.cap {
                    t::LineCap::Butt => w::LineCap::Butt,
                    t::LineCap::Round => w::LineCap::Round,
                    t::LineCap::Square => w::LineCap::Square,
                },
                join: match o.join {
                    t::LineJoin::Miter => w::LineJoin::Miter,
                    t::LineJoin::Round => w::LineJoin::Round,
                    t::LineJoin::Bevel => w::LineJoin::Bevel,
                },
            }),
            shadow: fx.shadow.as_ref().map(|s| w::FxShadow {
                blur_pt: s.blur_pt,
                dist_pt: s.dist_pt,
                dir_deg: s.dir_deg,
                sx: s.sx,
                sy: s.sy,
                kx_deg: s.kx_deg,
                ky_deg: s.ky_deg,
                algn: self.sym(s.algn),
                hex: self.sym(s.hex),
                alpha: s.alpha,
            }),
            glow: fx.glow.as_ref().map(|g| w::FxGlow { rad_pt: g.rad_pt, hex: self.sym(g.hex), alpha: g.alpha }),
            reflection: fx.reflection.as_ref().map(|r| w::FxReflection {
                blur_pt: r.blur_pt,
                st_a: r.st_a,
                st_pos: r.st_pos,
                end_a: r.end_a,
                end_pos: r.end_pos,
                dist_pt: r.dist_pt,
                dir_deg: r.dir_deg,
                algn: self.sym(r.algn),
            }),
        }
    }

    pub fn page_fill(&mut self, f: &t::PageFill) -> w::PageFill {
        w::PageFill {
            kind: match f.kind {
                t::PageFillKind::Gradient => w::PageFillKind::Gradient,
                t::PageFillKind::GradientRadial => w::PageFillKind::GradientRadial,
                t::PageFillKind::Pattern => w::PageFillKind::Pattern,
                t::PageFillKind::Frame => w::PageFillKind::Frame,
                t::PageFillKind::Tile => w::PageFillKind::Tile,
            },
            c1: self.sym(f.c1),
            c2: self.sym(f.c2),
            stops: f.stops.as_ref().map(|v| v.iter().map(|s| w::GradStop { pos: s.pos, color: self.sym(s.color) }).collect()),
            angle: f.angle,
            focus: f.focus,
            focus_position: f.focus_position.map(|[x, y]| w::FocusPosition { x, y }),
            gradient_center: f.gradient_center,
            image: f.image.map(|i| self.image_id(i)),
            image_px: f.image_px.map(|[w_, h, dpi]| w::ImagePx { w: w_, h, dpi }),
            pattern: f.pattern.clone(),
        }
    }

    pub fn cust(paths: &[t::CustPath]) -> w::CustGeom {
        w::CustGeom {
            paths: paths
                .iter()
                .map(|p| w::CustPath {
                    w: p.w,
                    h: p.h,
                    fill: p.fill,
                    stroke: p.stroke,
                    cmds: p
                        .cmds
                        .iter()
                        .map(|c| match c {
                            t::CustCmd::M { x, y } => w::CustCmd::M(w::Pt { x: *x, y: *y }),
                            t::CustCmd::L { x, y } => w::CustCmd::L(w::Pt { x: *x, y: *y }),
                            t::CustCmd::C { pts } => w::CustCmd::C(w::CustPts { pts: pts.iter().map(|&[x, y]| w::Pt { x, y }).collect() }),
                            t::CustCmd::Q { pts } => w::CustCmd::Q(w::CustPts { pts: pts.iter().map(|&[x, y]| w::Pt { x, y }).collect() }),
                            t::CustCmd::A { w_r, h_r, st, sw } => w::CustCmd::A(w::CustArc { w_r: *w_r, h_r: *h_r, st: *st, sw: *sw }),
                            t::CustCmd::Z => w::CustCmd::Z,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
