//! A message of `render.uos` -> Rust values ([`Page`] + [`Resources`]): the inverse of the encoder,
//! for tests (encode -> decode = input), tools, fuzzing and the TS adapter of R-P2c (a layout
//! encoded by the generated TS encoder is painted through the wasm exports of uo-wasm).
//!
//! What the wire does not carry is not rebuilt: the canvas hints (derived data), the glyph text
//! (RQ7), and, of the resources, the bytes (`FaceRes.face`/`bytes`, `ImageRes.bytes`/`svg` are
//! pulled separately by the shell and attached by the host with `RenderView`'s setters).

use crate::codec as w;
use crate::ids::{CustId, FaceId, ImageId, PageFillId, Sym, TextFxId};
use crate::list::GlyphRunHead;
use crate::resources::{FaceRes, ImageRes, Resources};
use crate::types as t;
use std::sync::Arc;
use uo_codec::{DecodeError, Limits};

/// The state of one stream of messages: the wire tables (the codec's) and the Rust resources
/// built from them.
#[derive(Default)]
pub struct DecodeTables {
    wire: w::Tables,
    res: Resources,
    /// wire symbol index -> interned [`Sym`] (a foreign producer may send duplicates)
    sym_map: Vec<Sym>,
}

impl DecodeTables {
    #[must_use]
    pub fn new() -> DecodeTables {
        DecodeTables::default()
    }

    /// The resources decoded so far.
    #[must_use]
    pub fn resources(&self) -> &Resources {
        &self.res
    }

    /// The resources, for the host to attach what the wire does not carry (font and image bytes).
    pub fn resources_mut(&mut self) -> &mut Resources {
        &mut self.res
    }

    /// Takes the resources out (the stream restarts empty).
    pub fn take_resources(&mut self) -> Resources {
        self.wire = w::Tables::default();
        self.sym_map.clear();
        std::mem::take(&mut self.res)
    }
}

/// One decoded page and the producer's section.
#[derive(Debug)]
pub struct DecodedPage {
    pub page: t::Page,
    pub app: Vec<u8>,
}

struct Ctx<'a> {
    map: &'a [Sym],
}

fn bad(table: &'static str, index: u32) -> DecodeError {
    DecodeError::BadRef { table, index, bound: 0 }
}

impl Ctx<'_> {
    fn sym(&self, i: u32) -> Result<Sym, DecodeError> {
        usize::try_from(i).ok().and_then(|i| self.map.get(i)).copied().ok_or_else(|| bad("sym", i))
    }

    fn osym(&self, i: Option<u32>) -> Result<Option<Sym>, DecodeError> {
        i.map(|i| self.sym(i)).transpose()
    }

    fn pairs(&self, a: &Option<Vec<w::AdjValue>>) -> Result<Option<Vec<(Sym, f64)>>, DecodeError> {
        a.as_ref().map(|v| v.iter().map(|p| Ok((self.sym(p.name)?, p.value))).collect()).transpose()
    }

    fn ltrb(l: &w::Ltrb) -> t::Ltrb {
        t::Ltrb { l: l.l, t: l.t, r: l.r, b: l.b }
    }

    fn image(i: u32) -> Result<ImageId, DecodeError> {
        ImageId::from_index(i as usize).ok_or_else(|| bad("images", i))
    }

    fn stroke(&self, s: &w::Stroke) -> Result<t::Stroke, DecodeError> {
        Ok(t::Stroke { color: self.sym(s.color)?, width_pt: s.width_pt, dash: self.osym(s.dash)? })
    }

    fn shadow(&self, s: &w::PicShadow) -> Result<t::PicShadow, DecodeError> {
        Ok(t::PicShadow {
            kind: match s.kind {
                w::ShadowKind::Outer => t::ShadowKind::Outer,
                w::ShadowKind::Inner => t::ShadowKind::Inner,
            },
            color: self.sym(s.color)?,
            alpha: s.alpha,
            blur_pt: s.blur_pt,
            dist_pt: s.dist_pt,
            dir_deg: s.dir_deg,
            sx: s.sx.into(),
            sy: s.sy.into(),
            kx_deg: s.kx_deg.into(),
            ky_deg: s.ky_deg.into(),
            algn: self.osym(s.algn)?,
            rot_with_shape: s.rot_with_shape,
        })
    }

    fn glow(&self, g: &w::PicGlow) -> Result<t::PicGlow, DecodeError> {
        Ok(t::PicGlow { color: self.sym(g.color)?, alpha: g.alpha, rad_pt: g.rad_pt })
    }

    fn reflection(&self, r: &w::PicReflection) -> Result<t::PicReflection, DecodeError> {
        Ok(t::PicReflection {
            blur_pt: r.blur_pt,
            st_a: r.st_a,
            st_pos: r.st_pos,
            end_a: r.end_a,
            end_pos: r.end_pos,
            dist_pt: r.dist_pt,
            dir_deg: r.dir_deg,
            sy: r.sy,
            algn: self.osym(r.algn)?,
            rot_with_shape: r.rot_with_shape,
        })
    }

    fn look(&self, l: &w::PictureLook) -> Result<t::PictureLook, DecodeError> {
        Ok(t::PictureLook {
            geom: l.geom.as_ref().map(|g| Ok(t::PresetSpec { prst: self.sym(g.prst)?, adj: self.pairs(&g.adj)? })).transpose()?,
            line: l
                .line
                .as_ref()
                .map(|p| {
                    Ok(t::PicLine {
                        color: self.sym(p.color)?,
                        alpha: p.alpha,
                        width_pt: p.width_pt,
                        dash: self.osym(p.dash)?,
                        cmpd: self.osym(p.cmpd)?,
                        cap: self.osym(p.cap)?,
                        join: self.osym(p.join)?,
                    })
                })
                .transpose()?,
            shadow: l.shadow.as_ref().map(|s| self.shadow(s)).transpose()?,
            glow: l.glow.as_ref().map(|g| self.glow(g)).transpose()?,
            soft_edge_pt: l.soft_edge_pt.into(),
            bevel: l
                .bevel
                .as_ref()
                .map(|b| {
                    Ok(t::PicBevel {
                        w_pt: b.w_pt,
                        h_pt: b.h_pt,
                        prst: self.osym(b.prst)?,
                        rig: self.osym(b.rig)?,
                        dir: self.osym(b.dir)?,
                        material: self.osym(b.material)?,
                    })
                })
                .transpose()?,
            reflection: l.reflection.as_ref().map(|r| self.reflection(r)).transpose()?,
            alpha: l.alpha.into(),
            duotone: l.duotone.as_ref().map(|d| Ok([self.sym(d.first)?, self.sym(d.second)?])).transpose()?,
            clr_change: l
                .clr_change
                .as_ref()
                .map(|c| Ok(t::ClrChange { from: self.sym(c.from)?, to: self.sym(c.to)?, to_alpha: c.to_alpha }))
                .transpose()?,
        })
    }

    fn grad(&self, g: &w::GradFill) -> Result<t::GradFill, DecodeError> {
        Ok(t::GradFill {
            stops: g.stops.iter().map(|s| Ok(t::GradStop { pos: s.pos, color: self.sym(s.color)? })).collect::<Result<_, DecodeError>>()?,
            angle_deg: g.angle_deg,
            scaled: g.scaled,
            rot_with_shape: g.rot_with_shape,
            path: self.osym(g.path)?,
            fill_to: g.fill_to.as_ref().map(Self::ltrb),
        })
    }

    fn shape(&self, sh: &w::ShapeItem) -> Result<t::ShapeItem, DecodeError> {
        let st = &sh.style;
        Ok(t::ShapeItem {
            x: sh.x,
            y: sh.y,
            w: sh.w,
            h: sh.h,
            geom: t::ShapeGeom {
                geom: self.sym(st.geom)?,
                adj: self.pairs(&st.adj)?,
                cust: st.cust.map(|c| CustId::from_index(c as usize).ok_or_else(|| bad("cust", c))).transpose()?,
                flip_h: st.flip_h,
                flip_v: st.flip_v,
                rot: st.rot.into(),
                fill_alpha: st.fill_alpha.into(),
                line_alpha: st.line_alpha.into(),
                eval: t::GeomEval::Compact,
            },
            fill: self.osym(sh.fill)?,
            stroke: sh.stroke.as_ref().map(|s| self.stroke(s)).transpose()?,
            line: sh.line.as_ref().map(|s| [s.x1, s.y1, s.x2, s.y2]),
            grad: sh.grad.as_ref().map(|g| self.grad(g)).transpose()?,
            shadow: sh
                .shadow
                .as_ref()
                .map(|s| Ok(t::ShapeShadow { dx: s.dx, dy: s.dy, color: self.sym(s.color)?, alpha: s.alpha.into(), blur: s.blur.into() }))
                .transpose()?,
            pattern: sh
                .pattern
                .as_ref()
                .map(|p| Ok(t::Pattern { prst: self.sym(p.prst)?, fg: self.sym(p.fg)?, bg: self.sym(p.bg)? }))
                .transpose()?,
            blip: sh
                .blip
                .as_ref()
                .map(|b| {
                    Ok(t::Blip {
                        image: Self::image(b.image)?,
                        tile: b
                            .tile
                            .as_ref()
                            .map(|k| {
                                Ok(t::BlipTile { tx: k.tx, ty: k.ty, sx: k.sx, sy: k.sy, algn: self.sym(k.algn)?, flip: self.sym(k.flip)? })
                            })
                            .transpose()?,
                        alpha: b.alpha.into(),
                    })
                })
                .transpose()?,
            fx: sh
                .fx
                .as_ref()
                .map(|f| {
                    Ok(t::ShapeFx {
                        shadow: f.shadow.as_ref().map(|s| self.shadow(s)).transpose()?,
                        glow: f.glow.as_ref().map(|g| self.glow(g)).transpose()?,
                        soft_edge_pt: f.soft_edge_pt.into(),
                        reflection: f.reflection.as_ref().map(|r| self.reflection(r)).transpose()?,
                    })
                })
                .transpose()?,
        })
    }

    fn path_cmd(c: &w::PathCmd) -> t::PathCmd {
        match c {
            w::PathCmd::M(p) => t::PathCmd::M { x: p.x, y: p.y },
            w::PathCmd::L(p) => t::PathCmd::L { x: p.x, y: p.y },
            w::PathCmd::C(c) => t::PathCmd::C { x1: c.x1, y1: c.y1, x2: c.x2, y2: c.y2, x: c.x, y: c.y },
            w::PathCmd::Z => t::PathCmd::Z,
        }
    }

    fn resolved_fill(&self, f: &w::ResolvedFill) -> Result<t::ResolvedFill, DecodeError> {
        Ok(match f {
            w::ResolvedFill::None => t::ResolvedFill::None,
            w::ResolvedFill::Solid(s) => t::ResolvedFill::Solid { hex: self.sym(s.hex)?, alpha: s.alpha },
            w::ResolvedFill::Grad(g) => t::ResolvedFill::Grad {
                stops: g
                    .stops
                    .iter()
                    .map(|s| Ok(t::FxStop { pos: s.pos, hex: self.sym(s.hex)?, alpha: s.alpha }))
                    .collect::<Result<_, DecodeError>>()?,
                angle_deg: g.angle_deg,
                scaled: g.scaled,
                path: self.osym(g.path)?,
                fill_to: g.fill_to.as_ref().map(Self::ltrb),
            },
        })
    }

    fn text_fx(&self, fx: &w::TextFx) -> Result<t::ResolvedTextFx, DecodeError> {
        Ok(t::ResolvedTextFx {
            fill: fx.fill.as_ref().map(|f| self.resolved_fill(f)).transpose()?,
            outline: fx
                .outline
                .as_ref()
                .map(|o| {
                    Ok(t::FxOutline {
                        width_pt: o.width_pt,
                        fill: self.resolved_fill(&o.fill)?,
                        dash: self.osym(o.dash)?,
                        cap: match o.cap {
                            w::LineCap::Butt => t::LineCap::Butt,
                            w::LineCap::Round => t::LineCap::Round,
                            w::LineCap::Square => t::LineCap::Square,
                        },
                        join: match o.join {
                            w::LineJoin::Miter => t::LineJoin::Miter,
                            w::LineJoin::Round => t::LineJoin::Round,
                            w::LineJoin::Bevel => t::LineJoin::Bevel,
                        },
                    })
                })
                .transpose()?,
            shadow: fx
                .shadow
                .as_ref()
                .map(|s| {
                    Ok(t::FxShadow {
                        blur_pt: s.blur_pt,
                        dist_pt: s.dist_pt,
                        dir_deg: s.dir_deg,
                        sx: s.sx,
                        sy: s.sy,
                        kx_deg: s.kx_deg,
                        ky_deg: s.ky_deg,
                        algn: self.sym(s.algn)?,
                        hex: self.sym(s.hex)?,
                        alpha: s.alpha,
                    })
                })
                .transpose()?,
            glow: fx.glow.as_ref().map(|g| Ok(t::FxGlow { rad_pt: g.rad_pt, hex: self.sym(g.hex)?, alpha: g.alpha })).transpose()?,
            reflection: fx
                .reflection
                .as_ref()
                .map(|r| {
                    Ok(t::FxReflection {
                        blur_pt: r.blur_pt,
                        st_a: r.st_a,
                        st_pos: r.st_pos,
                        end_a: r.end_a,
                        end_pos: r.end_pos,
                        dist_pt: r.dist_pt,
                        dir_deg: r.dir_deg,
                        algn: self.sym(r.algn)?,
                    })
                })
                .transpose()?,
        })
    }

    fn page_fill(&self, f: &w::PageFill) -> Result<t::PageFill, DecodeError> {
        Ok(t::PageFill {
            kind: match f.kind {
                w::PageFillKind::Gradient => t::PageFillKind::Gradient,
                w::PageFillKind::GradientRadial => t::PageFillKind::GradientRadial,
                w::PageFillKind::Pattern => t::PageFillKind::Pattern,
                w::PageFillKind::Frame => t::PageFillKind::Frame,
                w::PageFillKind::Tile => t::PageFillKind::Tile,
            },
            c1: self.sym(f.c1)?,
            c2: self.sym(f.c2)?,
            stops: f
                .stops
                .as_ref()
                .map(|v| {
                    v.iter().map(|s| Ok(t::GradStop { pos: s.pos, color: self.sym(s.color)? })).collect::<Result<Vec<_>, DecodeError>>()
                })
                .transpose()?,
            angle: f.angle,
            focus: f.focus,
            focus_position: f.focus_position.as_ref().map(|p| [p.x, p.y]),
            gradient_center: f.gradient_center,
            image: f.image.map(Self::image).transpose()?,
            image_px: f.image_px.as_ref().map(|p| [p.w, p.h, p.dpi]),
            pattern: f.pattern.clone(),
        })
    }
}

fn cust_of(g: &w::CustGeom) -> Arc<[t::CustPath]> {
    g.paths
        .iter()
        .map(|p| t::CustPath {
            w: p.w,
            h: p.h,
            fill: p.fill,
            stroke: p.stroke,
            cmds: p
                .cmds
                .iter()
                .map(|c| match c {
                    w::CustCmd::M(q) => t::CustCmd::M { x: q.x, y: q.y },
                    w::CustCmd::L(q) => t::CustCmd::L { x: q.x, y: q.y },
                    w::CustCmd::C(q) => t::CustCmd::C { pts: q.pts.iter().map(|p| [p.x, p.y]).collect() },
                    w::CustCmd::Q(q) => t::CustCmd::Q { pts: q.pts.iter().map(|p| [p.x, p.y]).collect() },
                    w::CustCmd::A(a) => t::CustCmd::A { w_r: a.w_r, h_r: a.h_r, st: a.st, sw: a.sw },
                    w::CustCmd::Z => t::CustCmd::Z,
                })
                .collect(),
        })
        .collect::<Vec<_>>()
        .into()
}

fn capacity(_: crate::list::CapacityError) -> DecodeError {
    DecodeError::Limit("display list column")
}

/// Decodes one page message, appending the resources it adds to `tables`.
///
/// # Errors
/// The codec's [`DecodeError`]s; on an error `tables` is as it was.
pub fn decode_page(buf: &[u8], tables: &mut DecodeTables, limits: &Limits) -> Result<DecodedPage, DecodeError> {
    let head = w::peek_header(buf)?;
    let reset = tables.wire.generation != Some(head.generation);
    let lens = if reset { [0; 6] } else { tables.wire.lens() };
    let msg = w::decode_page(buf, &mut tables.wire, limits)?;
    if reset {
        tables.res = Resources::new();
        tables.sym_map.clear();
    }
    match convert(tables, &lens, &msg) {
        Ok(page) => Ok(DecodedPage { page, app: msg.app }),
        Err(e) => {
            // the wire tables took the entries and the resources are half-built: restart the stream
            // (a message that decoded and does not convert only comes from a foreign producer)
            tables.wire = w::Tables::default();
            tables.res = Resources::new();
            tables.sym_map.clear();
            Err(e)
        }
    }
}

fn convert(tables: &mut DecodeTables, lens: &[usize; 6], msg: &w::Page) -> Result<t::Page, DecodeError> {
    let DecodeTables { wire, res, sym_map } = tables;
    for u in wire.sym.iter().skip(lens[0]) {
        let id = res.symbols.intern(u).ok_or(DecodeError::Limit("symbols"))?;
        sym_map.push(id);
    }
    let cx = Ctx { map: sym_map };
    for f in wire.faces.iter().skip(lens[1]) {
        res.faces.push(FaceRes {
            key: cx.sym(f.key)?,
            family: cx.sym(f.family)?,
            bold: f.bold,
            italic: f.italic,
            synth_bold: f.synth_bold,
            synth_italic: f.synth_italic,
            stand_in: f.stand_in,
            face: None,
            bytes: None,
            advances: None,
        });
    }
    for im in wire.images.iter().skip(lens[2]) {
        res.images.push(ImageRes {
            bytes: Arc::from(Vec::new()),
            ext: cx.sym(im.ext)?,
            svg: None,
            effects: im.effects.as_ref().map(|e| uo_drawingml::ImageEffects {
                gray: e.gray,
                gain: e.gain,
                bright: e.bright,
                floor: e.floor,
                bi_level: e.bi_level,
            }),
            width_twips: im.width_twips,
            height_twips: im.height_twips,
            natural_px: im.natural.as_ref().map(|n| (n.w, n.h)),
        });
    }
    for fx in wire.text_fx.iter().skip(lens[3]) {
        res.text_fx.push(cx.text_fx(fx)?);
    }
    for f in wire.page_fills.iter().skip(lens[4]) {
        res.page_fills.push(cx.page_fill(f)?);
    }
    for c in wire.cust.iter().skip(lens[5]) {
        res.cust.push(cust_of(c));
    }

    let mut page = t::Page { w: msg.w, h: msg.h, ..t::Page::default() };
    page.background = cx.osym(msg.background)?;
    page.background_fill =
        msg.background_fill.map(|f| PageFillId::from_index(f as usize).ok_or_else(|| bad("page_fills", f))).transpose()?;
    page.background_show = msg.background_show;
    for it in &msg.items {
        match it {
            w::Item::Glyphs(g) => {
                let mut flags = t::RunFlags::default();
                flags = flags.with(t::RunFlags::SYNTH_BOLD, g.synth_bold).with(t::RunFlags::SYNTH_ITALIC, g.synth_italic);
                flags = flags.with(t::RunFlags::AS_PATH, g.as_path).with(t::RunFlags::COLR, g.colr);
                let head = GlyphRunHead {
                    face: FaceId::from_index(g.face as usize).ok_or_else(|| bad("faces", g.face))?,
                    color: cx.sym(g.color)?,
                    size_pt: g.size_pt,
                    y: g.y,
                    text: g.text.as_deref().unwrap_or(&[]),
                    scale_x: g.scale_x.into(),
                    alpha: g.alpha.into(),
                    fx: g.fx.map(|f| TextFxId::from_index(f as usize).ok_or_else(|| bad("text_fx", f))).transpose()?,
                    flags,
                };
                let c = &g.glyphs;
                page.list.push_glyphs(head, c.cp.iter().zip(&c.x).zip(&c.gid).map(|((&cp, &x), &gid)| (cp, x, gid))).map_err(capacity)?;
            }
            w::Item::Rect(r) => {
                page.list.push_plain(t::Item::Rect { x: r.x, y: r.y, w: r.w, h: r.h, fill: cx.sym(r.fill)?, alpha: r.alpha.into() })
            }
            w::Item::Line(l) => page.list.push_plain(t::Item::Line {
                x1: l.x1,
                y1: l.y1,
                x2: l.x2,
                y2: l.y2,
                color: cx.sym(l.color)?,
                width_pt: l.width_pt,
                dash: l.dash.as_ref().map(|d| match d {
                    w::LineDash::Dotted => t::LineDash::Dotted,
                    w::LineDash::Dashed => t::LineDash::Dashed,
                    w::LineDash::DashDot => t::LineDash::DashDot,
                    w::LineDash::DashDotDot => t::LineDash::DashDotDot,
                    w::LineDash::BdrDot => t::LineDash::BdrDot,
                    w::LineDash::BdrDash => t::LineDash::BdrDash,
                    w::LineDash::BdrDashDot => t::LineDash::BdrDashDot,
                    w::LineDash::BdrDashDotDot => t::LineDash::BdrDashDotDot,
                    w::LineDash::BdrDashSmall => t::LineDash::BdrDashSmall,
                }),
            }),
            w::Item::Image(im) => page.list.push_plain(t::Item::Image(Box::new(t::ImageItem {
                x: im.x,
                y: im.y,
                w: im.w,
                h: im.h,
                image: Ctx::image(im.image)?,
                crop: im.crop.as_ref().map(Ctx::ltrb),
                rot: im.rot.into(),
                flip_h: im.flip_h,
                flip_v: im.flip_v,
                pic: im.pic.as_ref().map(|p| cx.look(p)).transpose()?.map(Box::new),
            }))),
            w::Item::Shape(s) => page.list.push_plain(t::Item::Shape(Box::new(cx.shape(s)?))),
            w::Item::Path(p) => {
                let cmds: Vec<t::PathCmd> = p.cmds.iter().map(Ctx::path_cmd).collect();
                page.list
                    .push_path(&cmds, cx.osym(p.fill)?, p.stroke.as_ref().map(|s| cx.stroke(s)).transpose()?, p.even_odd)
                    .map_err(capacity)?;
            }
            w::Item::Clip(c) => page.list.push_plain(match c.op {
                w::StackOp::Push => t::Item::ClipPush { x: c.x.into(), y: c.y.into(), w: c.w.into(), h: c.h.into() },
                w::StackOp::Pop => t::Item::ClipPop,
            }),
            w::Item::Xform(x) => page.list.push_plain(match x.op {
                w::StackOp::Push => t::Item::XformPush {
                    m: match &x.m {
                        Some(m) => match m.as_slice() {
                            [a, b, c, d, e, f] => Some(Box::new([*a, *b, *c, *d, *e, *f])),
                            _ => return Err(DecodeError::Limit("xform matrix is not six numbers")),
                        },
                        None => None,
                    },
                },
                w::StackOp::Pop => t::Item::XformPop,
            }),
        }
    }
    Ok(page)
}
