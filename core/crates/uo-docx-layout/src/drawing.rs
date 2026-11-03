//! Drawing values resolved into display-list values (frontier F3): shapes and groups
//! (`geometry.ts` 356–475: `substPhClr`, `withThemeStyles`, `shapeExtras`, `groupChild`, building
//! the `uo_render::types::ShapeGeom` spec — the path evaluation itself moved to `uo_render::geom`,
//! RQ1), pictures (`picture.ts` `pictureLook`), the page fill (`page-fill.ts` `resolvePageFill`,
//! `imagePixelInfo`, `patternRowsFromImage`: a derived image per call, K12/RQ3) and text effects
//! (`textfx.ts` `fxColor`, `fillOf`, `resolveTextFx`). Colours through uo-drawingml; strings kept
//! as the document wrote them (RQ6).
//!
//! Ported 1:1 in the TS operation order. Every TS `??` treats a model `null` (`JsNumber::Null`,
//! only after an undo's JSON round trip) as absent, every `!== undefined` as present and every
//! arithmetic use as 0, as JS does. Identity (RQ3): what creates a TS object here (page-fill
//! images, EMF bitmaps) registers a new image through [`ResWriter`]; what passes a model object
//! through (`ShapeStyle.blip`) returns a reference ([`BlipRef`]) and the engine gives it the id of
//! that model object.

use std::borrow::Cow;
use std::sync::Arc;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::model::{
    DrawingColor, DrawingGroup, FxColor, FxFill, GroupItem, ImageData, JsNumber, MediaStore, Nullable, PageBackground, PicBlipFx, PicColor,
    Run, ShapeStyle, TextEffects, Theme,
};
use crate::output::ImageOrigin;
use uo_drawingml::dml::{SpecHead, WordSpec, hex6_rgb, is_hex6, resolve_word_spec, to_hex_upper, word_theme_slot};
use uo_drawingml::{ColorModKind, apply_color_mods};
use uo_jsnum::math::{abs, cos, floor, max, min, rem, round, sin};
use uo_render::types::{
    BlipTile, ClrChange, CustCmd, CustPath, FxGlow, FxOutline, FxReflection, FxShadow, FxStop, GeomEval, GradFill, GradStop, LineCap,
    LineJoin, Ltrb, PageFill, PageFillKind, Pattern, PicBevel, PicGlow, PicLine, PicReflection, PicShadow, PictureLook, PresetSpec,
    ResolvedFill, ResolvedTextFx, ShadowKind, ShapeFx, ShapeGeom, ShapeShadow,
};
use uo_render::{CustId, ImageId, ImageRes, OptF64, Resources, Sym};

const EMU_PT: f64 = 12700.0;

// ───────────────────────── resources ─────────────────────────

/// Where F3 writes what it creates: the layout's resource tables (symbols, images, custom paths)
/// and the origin of each image (`Layout::image_origins`, same index as `Resources::images`).
pub struct ResWriter<'r> {
    pub res: &'r mut Resources,
    pub origins: &'r mut Vec<ImageOrigin>,
}

fn capacity() -> LayoutError {
    LayoutError::Limit(LimitKind::Capacity)
}

impl<'r> ResWriter<'r> {
    pub fn new(res: &'r mut Resources, origins: &'r mut Vec<ImageOrigin>) -> ResWriter<'r> {
        ResWriter { res, origins }
    }

    /// The symbol of a string.
    pub fn sym(&mut self, s: &str) -> LResult<Sym> {
        self.res.symbols.intern_str(s).ok_or_else(capacity)
    }

    /// The symbol of UTF-16 units.
    pub fn sym16(&mut self, s: &[u16]) -> LResult<Sym> {
        self.res.symbols.intern(s).ok_or_else(capacity)
    }

    /// A new image object the layout built (an EMF bitmap record, an EMF+ image, K12): a new
    /// `ImageId` per call (the TS object identity, RQ3), the bytes shared with an equal image of
    /// the layout (deduplicated by content, K12).
    pub fn derived_image(&mut self, bytes: Vec<u8>, ext: &str, width_twips: f64, height_twips: f64) -> LResult<ImageId> {
        let shared = self.res.images.iter().find(|i| i.bytes.len() == bytes.len() && *i.bytes == *bytes).map(|i| i.bytes.clone());
        let bytes = shared.unwrap_or_else(|| Arc::from(bytes));
        self.derived_image_shared(bytes, ext, width_twips, height_twips)
    }

    /// A new image object over bytes that already exist (a page-fill image over the model's media).
    pub fn derived_image_shared(&mut self, bytes: Arc<[u8]>, ext: &str, width_twips: f64, height_twips: f64) -> LResult<ImageId> {
        let natural_px = uo_image::image_pixel_size(&bytes).map(|p| (p.width, p.height));
        let ext = self.sym(ext)?;
        let id = self
            .res
            .add_image(ImageRes { bytes, ext, svg: None, effects: None, width_twips, height_twips, natural_px })
            .ok_or_else(capacity)?;
        self.origins.push(ImageOrigin::Derived);
        Ok(id)
    }

    /// The id of custom paths (`ShapeStyle.cust`), shared with equal paths already in the layout
    /// (the canonical stream writes them by value).
    pub fn cust(&mut self, paths: &[uo_drawingml::CustPath]) -> LResult<CustId> {
        let conv: Vec<CustPath> = paths.iter().map(cust_path).collect();
        if let Some(i) = self.res.cust.iter().position(|c| cust_eq(c, &conv)) {
            return CustId::from_index(i).ok_or_else(capacity);
        }
        self.res.add_cust(Arc::from(conv)).ok_or_else(capacity)
    }
}

fn cust_path(p: &uo_drawingml::CustPath) -> CustPath {
    use uo_drawingml::CustCmd as M;
    CustPath {
        w: p.w,
        h: p.h,
        fill: p.fill,
        stroke: p.stroke,
        cmds: p
            .cmds
            .iter()
            .map(|c| match c {
                M::MoveTo { x, y } => CustCmd::M { x: *x, y: *y },
                M::LineTo { x, y } => CustCmd::L { x: *x, y: *y },
                M::CubicTo(pts) => CustCmd::C { pts: pts.clone() },
                M::QuadTo(pts) => CustCmd::Q { pts: pts.clone() },
                M::ArcTo { w_r, h_r, st, sw } => CustCmd::A { w_r: *w_r, h_r: *h_r, st: *st, sw: *sw },
                M::Close => CustCmd::Z,
            })
            .collect(),
    }
}

fn bits_eq(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn pts_eq(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| bits_eq(x, y))
}

fn cust_eq(a: &[CustPath], b: &[CustPath]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            bits_eq(&[p.w, p.h], &[q.w, q.h])
                && p.fill == q.fill
                && p.stroke == q.stroke
                && p.cmds.len() == q.cmds.len()
                && p.cmds.iter().zip(&q.cmds).all(|(c, d)| match (c, d) {
                    (CustCmd::M { x, y }, CustCmd::M { x: u, y: v }) | (CustCmd::L { x, y }, CustCmd::L { x: u, y: v }) => {
                        bits_eq(&[*x, *y], &[*u, *v])
                    }
                    (CustCmd::C { pts }, CustCmd::C { pts: o }) | (CustCmd::Q { pts }, CustCmd::Q { pts: o }) => pts_eq(pts, o),
                    (CustCmd::A { w_r, h_r, st, sw }, CustCmd::A { w_r: a, h_r: b, st: c2, sw: d2 }) => {
                        bits_eq(&[*w_r, *h_r, *st, *sw], &[*a, *b, *c2, *d2])
                    }
                    (CustCmd::Z, CustCmd::Z) => true,
                    _ => false,
                })
        })
}

// ───────────────────────── JS helpers ─────────────────────────

/// `x ?? d` of a model number (`null` is nullish).
fn nn(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) => v,
        _ => d,
    }
}

/// A model number in arithmetic (`null` is 0).
fn num(x: JsNumber) -> f64 {
    x.to_number()
}

/// `x ? … : …` of a model number.
fn truthy(x: Option<JsNumber>) -> bool {
    x.is_some_and(JsNumber::truthy)
}

/// An optional model number where the TS writes it only when `!== undefined` (`null` kept: the
/// canonical stream writes `optNum(null)` as absent, so `null` reads as absent there too).
fn defined(x: Option<JsNumber>) -> OptF64 {
    match x {
        Some(JsNumber::Num(v)) => OptF64::some(v),
        _ => OptF64::NONE,
    }
}

/// `String(n)` of a model number (`null` → `"null"`).
fn js_string(x: JsNumber) -> String {
    match x {
        JsNumber::Num(v) => uo_jsnum::format::to_js_string(v),
        JsNumber::Null => "null".to_owned(),
    }
}

/// The truthiness of a Word colour spec string (`''` is the only falsy one).
fn color_truthy(c: &DrawingColor) -> bool {
    !(matches!(&c.0.head, SpecHead::Hex(h) if h.is_empty()) && c.0.mods.is_empty())
}

/// `resolveDrawingColor(spec, theme)` of a model colour (`resolve.ts`): the structured spec through
/// uo-drawingml (the accepted named difference of uo-docx G12 applies).
#[must_use]
pub fn resolve_color(c: &DrawingColor, theme: &Theme) -> Option<String> {
    resolve_word_spec(&c.0.head, c.0.parsed_mods(), &theme.colors)
}

/// `resolveDrawingColor(spec, theme)` over a spec **string** built by the layout (`dmlColorSpec`,
/// `fxColor`): `split('|')`, `split(',')` and `split('=')` as the TS does.
#[must_use]
pub fn resolve_color_str(spec: &str, theme: &Theme) -> Option<String> {
    if spec.is_empty() {
        return None;
    }
    let mut parts = spec.split('|');
    let head = parts.next().unwrap_or("");
    let mods = parts.next();
    let hex = match head.strip_prefix('@') {
        Some(k) => theme.colors.get(word_theme_slot(k)).map(|v| &**v)?,
        None => head,
    };
    if !is_hex6(hex) {
        return None;
    }
    let Some(mods) = mods.filter(|m| !m.is_empty()) else { return Some(hex.to_ascii_uppercase()) };
    let base = hex6_rgb(hex)?;
    let mut list: Vec<(ColorModKind, Option<f64>)> = Vec::new();
    for m in mods.split(',') {
        let mut kv = m.split('=');
        let k = kv.next().unwrap_or("");
        let v = kv.next();
        let n = match v {
            None | Some("undefined") => None,
            Some(v) => Some(uo_jsnum::parse::parse_int(v, 10)).filter(|n| n.is_finite()),
        };
        if let Some(kind) = ColorModKind::from_name(k) {
            list.push((kind, n));
        }
    }
    let [r, g, b] = apply_color_mods(base, &list);
    Some(to_hex_upper(r * 255.0, g * 255.0, b * 255.0))
}

// ───────────────────────── shapes (geometry.ts 356–475) ─────────────────────────

/// `substPhClr(spec, color)`: the theme style's colour with the `phClr` placeholder replaced by
/// the reference's colour — the reference's modifiers, then the style's.
#[must_use]
pub fn subst_ph_clr(spec: &DrawingColor, color: &DrawingColor) -> DrawingColor {
    let ph = matches!(&spec.0.head, SpecHead::Scheme(k) if k.starts_with("phClr"));
    if !ph {
        return spec.clone();
    }
    if spec.0.mods.is_empty() {
        return color.clone();
    }
    let mut mods = color.0.mods.clone();
    mods.extend(spec.0.mods.iter().cloned());
    DrawingColor(WordSpec { head: color.0.head.clone(), mods, alpha: color.0.alpha })
}

/// `list?.[i]` with a JS number index.
fn js_at<T>(list: Option<&Vec<T>>, i: f64) -> Option<&T> {
    let l = list?;
    uo_jsnum::conv::js_index(i, l.len()).and_then(|k| l.get(k))
}

/// `withThemeStyles(s, theme)`: the style with the fill and the shadow of the theme's styles
/// (`fillRef` idx 2/3 and 1001…, `effectRef`) put in — Shape Styles (oracle textbox-lab style-*).
/// Borrowed when nothing changes (the TS returns `s` itself); the derived style's key order is
/// not kept (nothing reads it as JSON).
#[must_use]
pub fn with_theme_styles<'s>(s: &'s ShapeStyle, theme: &Theme) -> Cow<'s, ShapeStyle> {
    let mut out: Cow<'s, ShapeStyle> = Cow::Borrowed(s);
    if let Some(fr) = &s.fill_ref {
        let idx = num(fr.idx);
        let list = if idx >= 1000.0 { theme.bg_fill_styles.as_ref() } else { theme.fill_styles.as_ref() };
        if let Some(ts) = js_at(list, rem(idx, 1000.0) - 1.0) {
            let c = &fr.color;
            let o = out.to_mut();
            if let Some(f) = ts.fill.as_ref().filter(|f| color_truthy(f)) {
                o.fill = Some(subst_ph_clr(f, c));
            }
            if let Some(g) = &ts.gradient {
                let mut g = (**g).clone();
                g.from = subst_ph_clr(&g.from, c);
                g.to = subst_ph_clr(&g.to, c);
                if let Some(stops) = &mut g.stops {
                    for st in stops.iter_mut() {
                        st.color = subst_ph_clr(&st.color, c);
                    }
                }
                o.gradient = Some(Box::new(g));
            }
            if let Some(p) = &ts.pattern {
                let mut p = (**p).clone();
                p.fg = subst_ph_clr(&p.fg, c);
                p.bg = subst_ph_clr(&p.bg, c);
                o.pattern = Some(Box::new(p));
            }
        }
    }
    if let Some(er) = &s.effect_ref
        && s.shadow.is_none()
        && let Some(Some(sh)) = js_at(theme.effect_styles.as_ref(), num(er.idx) - 1.0)
    {
        let mut sh = sh.clone();
        sh.color = subst_ph_clr(&sh.color, &er.color);
        out.to_mut().shadow = Some(Box::new(sh));
    }
    out
}

/// `ShapeItem.blip`: the model's blip object passed through (`out.blip = s.blip`); the engine gives
/// it the `ImageId` of that model object (RQ3).
#[derive(Clone, Copy, Debug)]
pub struct BlipRef<'a> {
    pub image: &'a ImageData,
    pub tile: Option<BlipTile>,
    pub alpha: OptF64,
}

/// `Pick<ShapeItem, 'grad' | 'shadow' | 'pattern' | 'blip' | 'fx'>`.
#[derive(Clone, Debug, Default)]
pub struct ShapeExtras<'a> {
    pub grad: Option<GradFill>,
    pub shadow: Option<ShapeShadow>,
    pub pattern: Option<Pattern>,
    pub blip: Option<BlipRef<'a>>,
    pub fx: Option<ShapeFx>,
}

fn resolve_or(c: &DrawingColor, theme: &Theme, d: &str, w: &mut ResWriter<'_>) -> LResult<Sym> {
    match resolve_color(c, theme) {
        Some(h) => w.sym(&h),
        None => w.sym(d),
    }
}

/// `shapeExtras(s0, theme)`: gradient, pattern, picture and shadow of a style with the theme
/// colours resolved (`shape` items). The blip is read from `s0` (`withThemeStyles` never sets it,
/// so it is the same object).
pub fn shape_extras<'a>(s0: &'a ShapeStyle, theme: &Theme, w: &mut ResWriter<'_>) -> LResult<ShapeExtras<'a>> {
    let s = with_theme_styles(s0, theme);
    let mut out = ShapeExtras::default();
    if let Some(f) = &s.fx {
        let mut fx = ShapeFx::default();
        let mut any = false;
        if let Some(i) = &f.inner {
            fx.shadow = Some(PicShadow {
                kind: ShadowKind::Inner,
                color: resolve_or(&i.color, theme, "000000", w)?,
                alpha: num(i.alpha),
                blur_pt: num(i.blur_pt),
                dist_pt: num(i.dist_pt),
                dir_deg: num(i.dir_deg),
                sx: OptF64::NONE,
                sy: OptF64::NONE,
                kx_deg: OptF64::NONE,
                ky_deg: OptF64::NONE,
                algn: None,
                rot_with_shape: false,
            });
            any = true;
        } else if let Some(o) = &f.outer {
            fx.shadow = Some(PicShadow {
                kind: ShadowKind::Outer,
                color: resolve_or(&o.color, theme, "000000", w)?,
                alpha: num(o.alpha),
                blur_pt: num(o.blur_pt),
                dist_pt: num(o.dist_pt),
                dir_deg: num(o.dir_deg),
                sx: defined(o.sx),
                sy: defined(o.sy),
                kx_deg: if truthy(o.kx_deg) { defined(o.kx_deg) } else { OptF64::NONE },
                ky_deg: if truthy(o.ky_deg) { defined(o.ky_deg) } else { OptF64::NONE },
                algn: match o.algn.as_deref() {
                    Some(a) if !a.is_empty() => Some(w.sym(a)?),
                    _ => None,
                },
                rot_with_shape: o.rot_with_shape == Some(true),
            });
            any = true;
        }
        if let Some(g) = &f.glow {
            fx.glow = Some(PicGlow { color: resolve_or(&g.color, theme, "000000", w)?, alpha: num(g.alpha), rad_pt: num(g.rad_pt) });
            any = true;
        }
        if truthy(f.soft_edge_pt) {
            fx.soft_edge_pt = defined(f.soft_edge_pt);
            any = true;
        }
        if let Some(r) = &f.reflection {
            fx.reflection = Some(PicReflection {
                blur_pt: num(r.blur_pt),
                st_a: num(r.st_a),
                st_pos: num(r.st_pos),
                end_a: num(r.end_a),
                end_pos: num(r.end_pos),
                dist_pt: num(r.dist_pt),
                dir_deg: num(r.dir_deg),
                sy: num(r.sy),
                algn: match r.algn.as_deref() {
                    Some(a) => Some(w.sym(a)?),
                    None => None,
                },
                rot_with_shape: r.rot_with_shape == Some(true),
            });
            any = true;
        }
        if any {
            out.fx = Some(fx);
        }
    }
    if let Some(p) = &s.pattern {
        out.pattern =
            Some(Pattern { prst: w.sym(&p.prst)?, fg: resolve_or(&p.fg, theme, "000000", w)?, bg: resolve_or(&p.bg, theme, "FFFFFF", w)? });
    }
    if let Some(b) = &s0.blip {
        let tile = match &b.tile {
            Some(t) => {
                Some(BlipTile { tx: num(t.tx), ty: num(t.ty), sx: num(t.sx), sy: num(t.sy), algn: w.sym(&t.algn)?, flip: w.sym(&t.flip)? })
            }
            None => None,
        };
        out.blip = Some(BlipRef { image: &b.image, tile, alpha: defined(b.alpha) });
    }
    if let Some(g) = &s.gradient
        && let Some(stops) = g.stops.as_ref().filter(|s| !s.is_empty())
    {
        let mut st = Vec::with_capacity(stops.len());
        for x in stops {
            st.push(GradStop { pos: num(x.pos), color: resolve_or(&x.color, theme, "000000", w)? });
        }
        out.grad = Some(GradFill {
            stops: st,
            angle_deg: num(g.angle_deg),
            scaled: g.scaled == Some(true),
            rot_with_shape: g.rot_with_shape == Some(true),
            path: match g.path.as_deref() {
                Some(p) if !p.is_empty() => Some(w.sym(p)?),
                _ => None,
            },
            fill_to: g.fill_to.as_ref().map(|f| Ltrb { l: num(f.l), t: num(f.t), r: num(f.r), b: num(f.b) }),
        });
    }
    if let Some(sh) = &s.shadow
        && let Some(c) = resolve_color(&sh.color, theme)
    {
        out.shadow =
            Some(ShapeShadow { dx: num(sh.dx), dy: num(sh.dy), color: w.sym(&c)?, alpha: defined(sh.alpha), blur: defined(sh.blur) });
    }
    Ok(out)
}

/// The geometry spec of a shape item (RQ1: what the painters read of `ShapeItem.style`): preset,
/// adjust values in the TS key order, custom paths, flips, rotation and the two alphas.
pub fn shape_geom(style: &ShapeStyle, w: &mut ResWriter<'_>) -> LResult<ShapeGeom> {
    let adj = match &style.adj {
        Some(a) => {
            let mut v = Vec::with_capacity(a.len());
            for (k, x) in a.iter() {
                v.push((w.sym(k)?, num(*x)));
            }
            Some(v)
        }
        None => None,
    };
    let cust = match &style.cust {
        Some(c) => Some(w.cust(c)?),
        None => None,
    };
    Ok(ShapeGeom {
        geom: w.sym(&style.geom)?,
        adj,
        cust,
        flip_h: style.flip_h == Some(true),
        flip_v: style.flip_v == Some(true),
        rot: defined(style.rot),
        fill_alpha: defined(style.fill_alpha),
        line_alpha: defined(style.line.as_ref().and_then(|l| l.alpha)),
        eval: GeomEval::Compact,
    })
}

/// `groupChild(gr, item, x, y, w, h)`: a group member in the page rectangle with the GROUP's
/// rotation/flip applied. `run` is the model run when the group is neither rotated nor flipped,
/// else a new run object (`Cow::Owned`: the TS `{ ...r0 }` copy, a new identity — chains and
/// object lookups must not take it for the model's).
#[derive(Clone, Debug)]
pub struct GroupChild<'a> {
    pub run: Cow<'a, Run>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn rot_of(x: Option<JsNumber>) -> f64 {
    nn(x, 0.0)
}

/// `groupChild` (geometry.ts:438).
#[must_use]
pub fn group_child<'a>(gr: &DrawingGroup, item: &'a GroupItem, x: f64, y: f64, w: f64, h: f64) -> GroupChild<'a> {
    let (gw, gh) = (num(gr.width_twips), num(gr.height_twips));
    let sx = if uo_jsnum::conv::truthy(gw) { w / gw } else { 1.0 };
    let sy = if uo_jsnum::conv::truthy(gh) { h / gh } else { 1.0 };
    let iw = num(item.w) * sx;
    let ih = num(item.h) * sy;
    let mut ix = x + num(item.x) * sx;
    let mut iy = y + num(item.y) * sy;
    let (gflip_h, gflip_v) = (gr.flip_h == Some(true), gr.flip_v == Some(true));
    if !truthy(gr.rot) && !gflip_h && !gflip_v {
        return GroupChild { run: Cow::Borrowed(&item.run), x: ix, y: iy, w: iw, h: ih };
    }
    let gcx = x + w / 2.0;
    let gcy = y + h / 2.0;
    let mut cx = ix + iw / 2.0;
    let mut cy = iy + ih / 2.0;
    if gflip_h {
        cx = 2.0 * gcx - cx;
    }
    if gflip_v {
        cy = 2.0 * gcy - cy;
    }
    let grot = rot_of(gr.rot);
    let a = (grot * std::f64::consts::PI) / 180.0;
    let (c, s) = (cos(a), sin(a));
    let (ncx, ncy) = (gcx + (cx - gcx) * c - (cy - gcy) * s, gcy + (cx - gcx) * s + (cy - gcy) * c);
    ix = ncx - iw / 2.0;
    iy = ncy - ih / 2.0;
    // an odd flip reverses the child's rotation
    let mirror = gflip_h != gflip_v;
    let xf = |st: &ShapeStyle| -> ShapeStyle {
        let r0 = rot_of(st.rot);
        let r = rem((if mirror { -r0 } else { r0 }) + grot, 360.0);
        ShapeStyle {
            rot: Some(JsNumber::Num(r)),
            flip_h: Some((st.flip_h == Some(true)) != gflip_h),
            flip_v: Some((st.flip_v == Some(true)) != gflip_v),
            ..st.clone()
        }
    };
    let r0 = &item.run;
    let mut run = r0.clone();
    if let Some(e0) = r0.ext() {
        let e = run.ext_mut();
        if let Some(sh) = &e0.shape {
            let mut sh = (**sh).clone();
            sh.style = xf(&sh.style);
            e.shape = Some(Box::new(sh));
        }
        if let Some(im) = &e0.image {
            let mut im = (**im).clone();
            let st = match &im.style {
                Some(st) => xf(st),
                None => xf(&ShapeStyle { geom: "rect".into(), ..ShapeStyle::default() }),
            };
            im.style = Some(Box::new(st));
            e.image = Some(Box::new(im));
        }
        if let Some(tb0) = &e0.textbox {
            let mut tb = (**tb0).clone();
            if let Some(shp) = &tb.shape {
                tb.shape = Some(Box::new(xf(shp)));
            }
            let t0 = rot_of(tb.rot);
            let tr = rem((if mirror { -t0 } else { t0 }) + grot, 360.0);
            if uo_jsnum::conv::truthy(tr) {
                tb.rot = Some(JsNumber::Num(tr));
            }
            e.textbox = Some(Box::new(tb));
        }
        if let Some(g0) = &e0.group {
            let mut g = (**g0).clone();
            g.rot = Some(JsNumber::Num(rem(rot_of(g0.rot) + grot, 360.0)));
            g.flip_h = Some((g0.flip_h == Some(true)) != gflip_h);
            g.flip_v = Some((g0.flip_v == Some(true)) != gflip_v);
            e.group = Some(Box::new(g));
        }
    }
    GroupChild { run: Cow::Owned(run), x: ix, y: iy, w: iw, h: ih }
}

// ───────────────────────── pictures (picture.ts) ─────────────────────────

/// `dmlColorSpec(c)` of docx-core `picture-xml.ts`: the layout spec string and the alpha.
#[must_use]
pub fn dml_color_spec(c: &PicColor) -> (String, f64) {
    const PRST: [(&str, &str); 7] = [
        ("black", "000000"),
        ("white", "FFFFFF"),
        ("red", "FF0000"),
        ("green", "008000"),
        ("blue", "0000FF"),
        ("yellow", "FFFF00"),
        ("gray", "808080"),
    ];
    const ALIAS: [(&str, &str); 4] = [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")];
    let look = |t: &[(&'static str, &'static str)], k: &str| t.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);
    let mut head = match &*c.kind {
        "srgbClr" => uo_text::case::to_uppercase_str(&c.val),
        "schemeClr" => format!("@{}", look(&ALIAS, &c.val).unwrap_or(&c.val)),
        // a key of `Object.prototype` reads a function (or the prototype) from `PRST_RGB`: a head
        // that never resolves (`__proto__` without modifiers makes the TS throw a `TypeError`)
        "prstClr" if OBJECT_PROTO.contains(&&*c.val) => "function".to_owned(),
        "prstClr" => look(&PRST, &c.val).unwrap_or("000000").to_owned(),
        _ => match &c.last_clr {
            Some(l) => l.to_string(),
            None => (if &*c.val == "window" { "FFFFFF" } else { "000000" }).to_owned(),
        },
    };
    let mut alpha = 1.0;
    let mut mods: Vec<String> = Vec::new();
    for (n, v) in c.mods.iter().flatten() {
        if &**n == "alpha" {
            alpha = num(*v) / 100_000.0;
        } else {
            mods.push(format!("{n}={}", js_string(*v)));
        }
    }
    if !mods.is_empty() {
        head.push('|');
        head.push_str(&mods.join(","));
    }
    (head, alpha)
}

/// The keys `Object.prototype` has (a plain-object table lookup finds them).
const OBJECT_PROTO: [&str; 12] = [
    "constructor",
    "__proto__",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// `color(c, theme)` of picture.ts.
fn pic_color(c: Option<&PicColor>, theme: &Theme) -> Option<(String, f64)> {
    let (spec, alpha) = dml_color_spec(c?);
    resolve_color_str(&spec, theme).map(|hex| (hex, alpha))
}

fn opt_sym(s: Option<&str>, w: &mut ResWriter<'_>) -> LResult<Option<Sym>> {
    match s {
        Some(s) if !s.is_empty() => Ok(Some(w.sym(s)?)),
        _ => Ok(None),
    }
}

/// `pictureLook(im, theme)`: the picture's look beyond the bitmap with the theme colours resolved
/// (`None` = only the bitmap).
pub fn picture_look(im: &crate::model::InlineImage, theme: &Theme, w: &mut ResWriter<'_>) -> LResult<Option<PictureLook>> {
    let Some(f) = &im.format else { return Ok(None) };
    let mut out = PictureLook::default();
    let mut any = false;
    if !f.geom.is_empty() && &*f.geom != "rect" && &*f.geom != "custom" {
        let adj = match &f.adj {
            Some(a) => {
                let mut v = Vec::with_capacity(a.len());
                for (k, x) in a.iter() {
                    v.push((w.sym(k)?, num(*x)));
                }
                Some(v)
            }
            None => None,
        };
        out.geom = Some(PresetSpec { prst: w.sym(&f.geom)?, adj });
        any = true;
    }
    if let Some(Nullable::Val(ln)) = &f.line
        && let Some(Nullable::Val(fill)) = &ln.fill
        && let Some((color, alpha)) = pic_color(Some(fill), theme)
    {
        out.line = Some(PicLine {
            color: w.sym(&color)?,
            alpha,
            width_pt: nn(ln.w, 9525.0) / EMU_PT,
            dash: opt_sym(ln.dash.as_deref().filter(|d| *d != "solid"), w)?,
            cmpd: opt_sym(ln.cmpd.as_deref().filter(|d| *d != "sng"), w)?,
            cap: opt_sym(ln.cap.as_deref(), w)?,
            join: opt_sym(ln.join.as_deref(), w)?,
        });
        any = true;
    }
    let fx = &f.effects;
    if let Some(s) = &fx.shadow
        && (&*s.kind == "outer" || &*s.kind == "inner")
        && let Some((color, alpha)) = pic_color(Some(&s.color), theme)
    {
        out.shadow = Some(PicShadow {
            kind: if &*s.kind == "outer" { ShadowKind::Outer } else { ShadowKind::Inner },
            color: w.sym(&color)?,
            alpha,
            blur_pt: nn(s.blur_rad, 0.0) / EMU_PT,
            dist_pt: nn(s.dist, 0.0) / EMU_PT,
            dir_deg: nn(s.dir, 0.0) / 60000.0,
            sx: if s.sx.is_some() { OptF64::some(num_opt(s.sx) / 100_000.0) } else { OptF64::NONE },
            sy: if s.sy.is_some() { OptF64::some(num_opt(s.sy) / 100_000.0) } else { OptF64::NONE },
            kx_deg: if truthy(s.kx) { OptF64::some(num_opt(s.kx) / 60000.0) } else { OptF64::NONE },
            ky_deg: if truthy(s.ky) { OptF64::some(num_opt(s.ky) / 60000.0) } else { OptF64::NONE },
            algn: opt_sym(s.algn.as_deref(), w)?,
            rot_with_shape: s.rot_with_shape == Some(true),
        });
        any = true;
    }
    if let Some(g) = &fx.glow
        && let Some((color, alpha)) = pic_color(Some(&g.color), theme)
    {
        out.glow = Some(PicGlow { color: w.sym(&color)?, alpha, rad_pt: num(g.rad) / EMU_PT });
        any = true;
    }
    if truthy(fx.soft_edge) {
        out.soft_edge_pt = OptF64::some(num_opt(fx.soft_edge) / EMU_PT);
        any = true;
    }
    // bevel (front camera; 3D rotation/perspective is not drawn)
    if let Some(td) = &f.three_d
        && let Some(bt) = &td.bevel_t
        && td.camera.as_ref().is_none_or(|c| &*c.prst == "orthographicFront")
        && td.camera.as_ref().is_none_or(|c| c.rot.is_none())
    {
        out.bevel = Some(PicBevel {
            w_pt: nn(bt.w, 76200.0) / EMU_PT,
            h_pt: nn(bt.h, 76200.0) / EMU_PT,
            prst: opt_sym(bt.prst.as_deref(), w)?,
            rig: match &td.light_rig {
                Some(l) => Some(w.sym(&l.rig)?),
                None => None,
            },
            dir: match &td.light_rig {
                Some(l) => Some(w.sym(&l.dir)?),
                None => None,
            },
            material: opt_sym(td.prst_material.as_deref(), w)?,
        });
        any = true;
    }
    if let Some(r) = &fx.reflection {
        out.reflection = Some(PicReflection {
            blur_pt: nn(r.blur_rad, 0.0) / EMU_PT,
            st_a: nn(r.st_a, 100_000.0) / 100_000.0,
            st_pos: nn(r.st_pos, 0.0) / 100_000.0,
            end_a: nn(r.end_a, 0.0) / 100_000.0,
            end_pos: nn(r.end_pos, 100_000.0) / 100_000.0,
            dist_pt: nn(r.dist, 0.0) / EMU_PT,
            dir_deg: nn(r.dir, 5_400_000.0) / 60000.0,
            sy: nn(r.sy, 100_000.0) / 100_000.0,
            algn: opt_sym(r.algn.as_deref(), w)?,
            rot_with_shape: r.rot_with_shape == Some(true),
        });
        any = true;
    }
    any |= blip_look(&f.blip, theme, &mut out, w)?;
    Ok(any.then_some(out))
}

fn num_opt(x: Option<JsNumber>) -> f64 {
    x.map_or(f64::NAN, JsNumber::to_number)
}

/// `blipLook(b, theme)`: transparency, duotone and the transparent colour of the `a:blip`;
/// `Object.assign` into `out`. `true` when it set a key.
fn blip_look(b: &PicBlipFx, theme: &Theme, out: &mut PictureLook, w: &mut ResWriter<'_>) -> LResult<bool> {
    let mut any = false;
    if let Some(a) = b.alpha_mod_fix
        && num(a) < 100_000.0
    {
        out.alpha = OptF64::some(num(a) / 100_000.0);
        any = true;
    }
    if let Some(d) = &b.duotone
        && let (Some(c0), Some(c1)) = (pic_color(Some(&d.0), theme), pic_color(Some(&d.1), theme))
    {
        out.duotone = Some([w.sym(&c0.0)?, w.sym(&c1.0)?]);
        any = true;
    }
    if let Some(cc) = &b.clr_change
        && let (Some(from), Some(to)) = (pic_color(Some(&cc.from), theme), pic_color(Some(&cc.to), theme))
    {
        out.clr_change = Some(ClrChange { from: w.sym(&from.0)?, to: w.sym(&to.0)?, to_alpha: to.1 });
        any = true;
    }
    Ok(any)
}

// ───────────────────────── page fill (page-fill.ts) ─────────────────────────

/// `vmlColor(v)` of docx-core `drawing.ts` ("#rrggbb", "#rgb", a name, "#xxxxxx [n]" → RRGGBB),
/// over UTF-16 units (the TS string operations split by unit).
#[must_use]
pub fn vml_color(v: Option<&str>) -> Option<Vec<u16>> {
    const NAMES: [(&str, &str); 18] = [
        ("black", "000000"),
        ("white", "FFFFFF"),
        ("red", "FF0000"),
        ("green", "008000"),
        ("blue", "0000FF"),
        ("yellow", "FFFF00"),
        ("gray", "808080"),
        ("grey", "808080"),
        ("silver", "C0C0C0"),
        ("navy", "000080"),
        ("maroon", "800000"),
        ("teal", "008080"),
        ("olive", "808000"),
        ("purple", "800080"),
        ("fuchsia", "FF00FF"),
        ("aqua", "00FFFF"),
        ("lime", "00FF00"),
        ("window", "FFFFFF"),
    ];
    let v = v.filter(|v| !v.is_empty())?;
    let first = uo_jsnum::text::split_js_space(uo_jsnum::text::trim(v)).next().unwrap_or("");
    let s = uo_jsnum::text::to_lowercase(first);
    if let Some(h) = s.strip_prefix('#') {
        let h16: Vec<u16> = h.encode_utf16().collect();
        if h16.len() == 3 {
            let doubled: Vec<u16> = h16.iter().flat_map(|&u| [u, u]).collect();
            return Some(uo_text::case::to_uppercase(&doubled).into_owned());
        }
        if h16.len() >= 6 {
            return Some(uo_text::case::to_uppercase(h16.get(..6).unwrap_or(&h16)).into_owned());
        }
    }
    NAMES.iter().find(|(k, _)| *k == s).map(|(_, c)| c.encode_utf16().collect())
}

/// `/^\s*fill(?:\s+(darken|lighten)\((\d+)\))?\s*$/i` (ASCII case-insensitive, JS `\s`):
/// `Some(None)` for plain `fill`, `Some(Some((darken, digits)))` for the function form.
fn match_fill(s: &str) -> Option<Option<(bool, &str)>> {
    let t = ws(s);
    let t = strip_ci(t, "fill")?;
    if ws(t).is_empty() {
        return Some(None);
    }
    let t2 = ws(t);
    if t2.len() == t.len() {
        return None;
    }
    let (darken, t3) = match strip_ci(t2, "darken") {
        Some(r) => (true, r),
        None => (false, strip_ci(t2, "lighten")?),
    };
    let t4 = t3.strip_prefix('(')?;
    let n = t4.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    let (digits, rest) = t4.split_at(n);
    let rest = rest.strip_prefix(')')?;
    if !ws(rest).is_empty() {
        return None;
    }
    Some(Some((darken, digits)))
}

fn ws(t: &str) -> &str {
    t.trim_start_matches(uo_jsnum::text::is_js_space)
}

fn strip_ci<'s>(s: &'s str, word: &str) -> Option<&'s str> {
    let head = s.get(..word.len())?;
    head.eq_ignore_ascii_case(word).then(|| s.get(word.len()..).unwrap_or(""))
}

/// `resolveColor2(color2, color1)` of docx-core `page-background.ts`: VML colour 2 against colour
/// 1 ("fill darken(n)", "fill lighten(n)", "fill", a colour).
#[must_use]
pub fn resolve_color2(color2: Option<&str>, color1: &[u16]) -> Vec<u16> {
    let white = || "FFFFFF".encode_utf16().collect::<Vec<u16>>();
    let Some(c2) = color2.filter(|c| !c.is_empty()) else { return white() };
    let Some(m) = match_fill(c2) else { return vml_color(Some(c2)).unwrap_or_else(white) };
    let Some((darken, digits)) = m else { return color1.to_vec() };
    // truncated as Word: darken = ⌊c·n/255⌋, lighten = 255 − ⌊(255−c)·n/255⌋ (tl-one-*)
    let n = max(0.0, min(255.0, uo_jsnum::parse::parse_int(digits, 10)));
    let mut out = String::new();
    for i in [0usize, 2, 4] {
        let part: Vec<u16> = color1.get(i..(i + 2).min(color1.len())).unwrap_or(&[]).to_vec();
        let c = uo_jsnum::parse::parse_int_utf16(&part, 16);
        let v = if darken { floor((c * n) / 255.0) } else { 255.0 - floor(((255.0 - c) * n) / 255.0) };
        let hex = uo_jsnum::format::to_radix_string(v, 16).unwrap_or_else(|_| uo_jsnum::format::to_js_string(v));
        if hex.len() < 2 {
            out.push('0');
        }
        out.push_str(&hex);
    }
    uo_text::case::to_uppercase_str(&out).encode_utf16().collect()
}

/// `imagePixelInfo(b)`: size in px and DPI of an image (PNG/JPEG/GIF/BMP; 96 DPI when not
/// declared). `[w, h, dpi]`.
#[must_use]
pub fn image_pixel_info(b: &[u8]) -> Option<[f64; 3]> {
    let at = |i: usize| b.get(i).copied().map_or(0u32, u32::from);
    let be = |i: usize| (at(i) << 24) | (at(i + 1) << 16) | (at(i + 2) << 8) | at(i + 3);
    if b.first() == Some(&0x89) && b.get(1) == Some(&0x50) {
        let (w, h) = (f64::from(be(16)), f64::from(be(20)));
        let mut dpi = 96.0;
        let mut i: u64 = 8;
        while i + 8 < len_u64(b) {
            let Ok(iu) = usize::try_from(i) else { break };
            let len = be(iu);
            let ty = [at(iu + 4), at(iu + 5), at(iu + 6), at(iu + 7)];
            if ty == [0x70, 0x48, 0x59, 0x73] && b.get(iu + 16) == Some(&1) {
                dpi = round(f64::from(be(iu + 8)) * 0.0254);
            }
            if ty == [0x49, 0x44, 0x41, 0x54] || ty == [0x49, 0x45, 0x4e, 0x44] {
                break;
            }
            i += 12 + u64::from(len);
        }
        return Some([w, h, dpi]);
    }
    if let Some(jp) = uo_image::jpeg_info(b) {
        let mut dpi = 96.0;
        // JFIF APP0: units 1 = dpi
        if b.get(2) == Some(&0xff) && b.get(3) == Some(&0xe0) && b.get(6) == Some(&0x4a) && b.get(7) == Some(&0x46) && b.get(13) == Some(&1)
        {
            dpi = f64::from((at(14) << 8) | at(15));
        }
        return Some([f64::from(jp.width), f64::from(jp.height), if uo_jsnum::conv::truthy(dpi) { dpi } else { 96.0 }]);
    }
    if b.first() == Some(&0x47) && b.get(1) == Some(&0x49) && b.get(2) == Some(&0x46) {
        return Some([f64::from(at(6) | (at(7) << 8)), f64::from(at(8) | (at(9) << 8)), 96.0]);
    }
    if b.first() == Some(&0x42) && b.get(1) == Some(&0x4d) {
        let w = f64::from(at(18) | (at(19) << 8) | (at(20) << 16));
        let h32 = at(22) | (at(23) << 8) | (at(24) << 16) | (at(25) << 24);
        let h = abs(f64::from(i32::from_le_bytes(h32.to_le_bytes())));
        let ppm = at(38) | (at(39) << 8) | (at(40) << 16) | (at(41) << 24);
        return Some([w, h, if ppm != 0 { round(f64::from(ppm) * 0.0254) } else { 96.0 }]);
    }
    // `decodePng(b)`: never decodes here (the PNG signature was handled above), but a damaged
    // file does not reach it either
    match uo_image::decode_png(b, &uo_image::ImageLimits::DEFAULT) {
        Ok(png) => Some([f64::from(png.width), f64::from(png.height), 96.0]),
        Err(_) => None,
    }
}

fn len_u64(b: &[u8]) -> u64 {
    u64::try_from(b.len()).unwrap_or(u64::MAX)
}

/// `patternRowsFromImage(b)`: the 8 rows of a Word pattern GIF (a light pixel = foreground).
/// `Err` where the TS throws (a malformed image); a decoder limit is `Ok(None)` with `warn` set
/// (the TS has no limit).
pub fn pattern_rows_from_image(b: &[u8], warn: &mut bool) -> LResult<Option<Vec<f64>>> {
    let lim = uo_image::ImageLimits::DEFAULT;
    let decoded = match uo_image::decode_gif(b, &lim) {
        Ok(img) => Some(img),
        Err(uo_image::ImageError::Unsupported) => match uo_image::decode_png(b, &lim) {
            Ok(img) => Some(img),
            Err(uo_image::ImageError::Unsupported) => None,
            Err(uo_image::ImageError::Malformed(_)) => return Err(LayoutError::Internal("page fill: malformed pattern image")),
            Err(uo_image::ImageError::Limit(_)) => {
                *warn = true;
                None
            }
        },
        Err(uo_image::ImageError::Malformed(_)) => return Err(LayoutError::Internal("page fill: malformed pattern image")),
        Err(uo_image::ImageError::Limit(_)) => {
            *warn = true;
            None
        }
    };
    let Some(img) = decoded else { return Ok(None) };
    if img.width < 8 || img.height < 8 {
        return Ok(None);
    }
    let mut rows = Vec::with_capacity(8);
    for y in 0..8u64 {
        let mut r: u32 = 0;
        for x in 0..8u64 {
            let o = usize::try_from((y * u64::from(img.width) + x) * 4).unwrap_or(usize::MAX);
            let px = |k: usize| img.data.get(o + k).copied().map_or(f64::NAN, f64::from);
            let lum = 0.299 * px(0) + 0.587 * px(1) + 0.114 * px(2);
            if lum >= 128.0 {
                r |= 0x80 >> x;
            }
        }
        rows.push(f64::from(r));
    }
    Ok(Some(rows))
}

/// The warning of a page-fill image past the decoder limits.
pub const PAGE_FILL_LIMIT: &str = "page fill image too large";

/// `resolvePageFill(bg, pageColor)`: the `w:background` effect resolved (colour 1 = `fillcolor`,
/// or the page colour already resolved). The image is a new object per call (K12): a derived
/// image over the model's media bytes. `None` for a type outside the five the reader produces
/// (the TS keeps any string; only an edit could make one).
pub fn resolve_page_fill(
    bg: &PageBackground,
    page_color: Option<&str>,
    media: &MediaStore,
    w: &mut ResWriter<'_>,
    warn: &mut dyn FnMut(&str),
) -> LResult<Option<PageFill>> {
    let Some(f) = &bg.fill else { return Ok(None) };
    let c1_src = f.color.as_deref().or(page_color).unwrap_or("FFFFFF");
    let c1: Vec<u16> = uo_text::case::to_uppercase_str(c1_src).encode_utf16().collect();
    let kind = match &*f.r#type {
        "gradient" => PageFillKind::Gradient,
        "gradientRadial" => PageFillKind::GradientRadial,
        "pattern" => PageFillKind::Pattern,
        "frame" => PageFillKind::Frame,
        "tile" => PageFillKind::Tile,
        _ => return Ok(None),
    };
    let c2 = resolve_color2(f.color2.as_deref(), &c1);
    let stops = match &f.colors {
        Some(cs) if !cs.is_empty() => {
            let mut v = Vec::with_capacity(cs.len());
            for s in cs {
                v.push(GradStop { pos: num(s.pos), color: w.sym(&s.color)? });
            }
            Some(v)
        }
        _ => None,
    };
    let mut out = PageFill {
        kind,
        c1: w.sym16(&c1)?,
        c2: w.sym16(&c2)?,
        stops,
        angle: nn(f.angle, 0.0),
        focus: nn(f.focus, 0.0),
        focus_position: f.focus_position.map(|(a, b)| [num(a), num(b)]),
        gradient_center: f.gradient_center == Some(true),
        image: None,
        image_px: None,
        pattern: None,
    };
    let mut image = None;
    if let Some(img) = &f.image {
        let bytes: Arc<[u8]> = media.get_arc(img.data.0).cloned().unwrap_or_else(|| Arc::from(&[][..]));
        let info = image_pixel_info(&bytes);
        let (wt, ht) = match info {
            Some([iw, ih, dpi]) => ((iw * 1440.0) / dpi, (ih * 1440.0) / dpi),
            None => (1440.0, 1440.0),
        };
        out.image_px = info;
        if kind == PageFillKind::Pattern {
            let mut lim = false;
            let rows = pattern_rows_from_image(&bytes, &mut lim)?;
            if lim {
                warn(PAGE_FILL_LIMIT);
            }
            out.pattern = rows;
        }
        image = Some((bytes, &img.ext, wt, ht));
    }
    if kind == PageFillKind::Pattern && out.pattern.is_none() {
        return Ok(None);
    }
    if (kind == PageFillKind::Frame || kind == PageFillKind::Tile) && out.image_px.is_none() {
        return Ok(None);
    }
    // the TS object is built before the checks; registered only when the fill is kept (an unused
    // TS object is garbage, an unused entry here would be retained by the layout)
    if let Some((bytes, ext, wt, ht)) = image {
        out.image = Some(w.derived_image_shared(bytes, ext, wt, ht)?);
    }
    Ok(Some(out))
}

// ───────────────────────── text effects (textfx.ts) ─────────────────────────

const ANG: f64 = 60000.0;

/// `fxColor(c, theme, fallback)`: a w14 colour → hex + opacity (DrawingML modifiers; the alpha
/// apart). In w14 `w14:alpha` is the TRANSPARENCY. The hex is the resolved colour or `fallback`
/// as given.
#[must_use]
pub fn fx_color(c: &FxColor, theme: &Theme, fallback: &[u16]) -> (Vec<u16>, f64) {
    let all = c.mods.as_deref().unwrap_or(&[]);
    let mods: Vec<&(crate::model::Str, JsNumber)> = all.iter().filter(|(k, _)| &**k != "alpha").collect();
    let a = c.mods.as_ref().and_then(|m| m.iter().find(|(k, _)| &**k == "alpha")).map(|(_, v)| *v);
    let mut spec = match (&c.scheme, &c.rgb) {
        (Some(s), _) if !s.is_empty() => format!("@{s}"),
        (_, Some(r)) => r.to_string(),
        _ => String::from_utf16_lossy(fallback),
    };
    if !mods.is_empty() {
        spec.push('|');
        let parts: Vec<String> = mods.iter().map(|(k, v)| format!("{k}={}", js_string(*v))).collect();
        spec.push_str(&parts.join(","));
    }
    let hex = match resolve_color_str(&spec, theme) {
        Some(h) => h.encode_utf16().collect(),
        None => fallback.to_vec(),
    };
    let alpha = match a {
        Some(a) => max(0.0, min(1.0, 1.0 - num(a) / 100_000.0)),
        None => 1.0,
    };
    (hex, alpha)
}

/// `fillOf(f, theme, text)`.
fn fill_of(f: &FxFill, theme: &Theme, text: &[u16], w: &mut ResWriter<'_>) -> LResult<ResolvedFill> {
    const NO_COLOR: LayoutError = LayoutError::Internal("text effects: fill without colour");
    if &*f.r#type == "none" {
        return Ok(ResolvedFill::None);
    }
    if &*f.r#type == "solid" {
        let (hex, alpha) = fx_color(f.color.as_ref().ok_or(NO_COLOR)?, theme, text);
        return Ok(ResolvedFill::Solid { hex: w.sym16(&hex)?, alpha });
    }
    let src = f.stops.as_ref().ok_or(LayoutError::Internal("text effects: gradient without stops"))?;
    let mut stops: Vec<(f64, Vec<u16>, f64)> = src
        .iter()
        .map(|s| {
            let (hex, alpha) = fx_color(&s.color, theme, text);
            (num(s.pos) / 100_000.0, hex, alpha)
        })
        .collect();
    stops.sort_by(|a, b| {
        let d = a.0 - b.0;
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut st = Vec::with_capacity(stops.len());
    for (pos, hex, alpha) in stops {
        st.push(FxStop { pos, hex: w.sym16(&hex)?, alpha });
    }
    let (path, fill_to) = match &f.path {
        Some(p) => (
            Some(w.sym(&p.kind)?),
            Some(Ltrb { l: num(p.l) / 100_000.0, t: num(p.t) / 100_000.0, r: num(p.r) / 100_000.0, b: num(p.b) / 100_000.0 }),
        ),
        None => (None, None),
    };
    Ok(ResolvedFill::Grad {
        stops: st,
        angle_deg: nn(f.lin.as_ref().map(|l| l.ang), 5_400_000.0) / ANG,
        scaled: f.lin.as_ref().is_some_and(|l| l.scaled),
        path,
        fill_to,
    })
}

/// `resolveTextFx(fx, theme, text)`: the resolved effects of a run (`None`: only the solid opaque
/// colour, the common path). `text` is the run's colour (the fallback of the fills). The caller
/// registers the value in `Resources::text_fx` (the TS builds a new object per call; the canonical
/// stream writes it by value).
pub fn resolve_text_fx(fx: Option<&TextEffects>, theme: &Theme, text: &[u16], w: &mut ResWriter<'_>) -> LResult<Option<ResolvedTextFx>> {
    let Some(fx) = fx else { return Ok(None) };
    let mut out = ResolvedTextFx::default();
    let mut any = false;
    if let Some(f) = &fx.fill
        && &*f.r#type != "solid"
    {
        out.fill = Some(fill_of(f, theme, text, w)?);
        any = true;
    }
    if let Some(o) = &fx.outline
        && let Some(of) = &o.fill
        && &*of.r#type != "none"
    {
        // w="0": the device's thinnest stroke (Word writes 0 in the "no outline" presets with a colour)
        let width_pt = nn(o.w, 9525.0) / EMU_PT;
        let fill = fill_of(of, theme, text, w)?;
        let dash = match o.dash.as_deref() {
            Some(d) if !d.is_empty() && d != "solid" => Some(w.sym(d)?),
            _ => None,
        };
        let cap = match o.cap.as_deref().unwrap_or("flat") {
            "rnd" => LineCap::Round,
            "sq" => LineCap::Square,
            _ => LineCap::Butt,
        };
        let join = match o.join.as_deref() {
            Some("round") => LineJoin::Round,
            Some("bevel") => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        // `if (widthPt <= 0) delete out.outline`: NaN is kept
        if width_pt > 0.0 || width_pt.is_nan() {
            out.outline = Some(FxOutline { width_pt, fill, dash, cap, join });
            any = true;
        }
    }
    if let Some(s) = &fx.shadow {
        let (hex, alpha) = fx_color(&s.color, theme, &BLACK);
        if num_ne0(s.sx) && num_ne0(s.sy) {
            out.shadow = Some(FxShadow {
                blur_pt: num(s.blur_rad) / EMU_PT,
                dist_pt: num(s.dist) / EMU_PT,
                dir_deg: num(s.dir) / ANG,
                sx: num(s.sx) / 100_000.0,
                sy: num(s.sy) / 100_000.0,
                kx_deg: num(s.kx) / ANG,
                ky_deg: num(s.ky) / ANG,
                algn: w.sym(&s.algn)?,
                hex: w.sym16(&hex)?,
                alpha,
            });
            any = true;
        }
    }
    if let Some(g) = &fx.glow
        && num_gt0(g.rad)
    {
        let (hex, alpha) = fx_color(&g.color, theme, &BLACK);
        out.glow = Some(FxGlow { rad_pt: num(g.rad) / EMU_PT, hex: w.sym16(&hex)?, alpha });
        any = true;
    }
    if let Some(r) = &fx.reflection {
        out.reflection = Some(FxReflection {
            blur_pt: num(r.blur_rad) / EMU_PT,
            st_a: num(r.st_a) / 100_000.0,
            st_pos: num(r.st_pos) / 100_000.0,
            end_a: num(r.end_a) / 100_000.0,
            end_pos: num(r.end_pos) / 100_000.0,
            dist_pt: num(r.dist) / EMU_PT,
            dir_deg: num(r.dir) / ANG,
            algn: w.sym(&r.algn)?,
        });
        any = true;
    }
    Ok(any.then_some(out))
}

/// `'000000'` (the default fallback of `fxColor`).
const BLACK: [u16; 6] = [0x30; 6];

/// `x !== 0` of a model number (`null !== 0`).
fn num_ne0(x: JsNumber) -> bool {
    match x {
        JsNumber::Num(v) => v != 0.0,
        JsNumber::Null => true,
    }
}

/// `x > 0` of a model number (`null > 0` is false).
fn num_gt0(x: JsNumber) -> bool {
    x.to_number() > 0.0
}
