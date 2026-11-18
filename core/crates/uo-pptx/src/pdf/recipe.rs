//! The metafile recipes (B5b): a EMF, EMF+, WMF, PICT or TIFF picture arrives as a bitmap whose bytes are the canvas calls
//! the TS rasterizer made (`uo_dml_render::metafile::rec`, "UOMF"; the shell replays them in `meta-player.ts`). The PDF
//! plays them as vector content: each canvas of the recipe is a form XObject (a transparency group in the canvas space,
//! y down), the picture is the root canvas, and paths, fills, strokes, clips, transforms, gradients, patterns, text and
//! images go through the same interpreter as the slide itself. `putImageData` is an image at device coordinates; the
//! exact XOR (`xorRect`) has no PDF form and is counted (`meta:xor-rect`; the approximation that follows it is drawn).
//!
//! A derived recipe (version 2, `uo_dml_render::metafile::rec::encode_fx_recipe`: a metafile picture with the blip's colour
//! effects, gap B5b-G1) plays its base recipe with every colour mapped through `fxImage` (`images::apply_fx`): the CSS
//! colours of fills, strokes and gradient stops, and the pixels of its bitmaps. The effects are per-pixel maps, so mapping
//! the inputs equals mapping the raster except at anti-aliased edges and under transparency for the maps that are not
//! affine (bi-level, colour change).

use std::rc::Rc;

use uo_jsnum::math::{max, min};
use uo_render::slides::{Cap, CompositeOp, FillRule, Join, Rect4, Repeat};

use super::css::{Rgba, parse_css};
use super::geom::{Mat, nm};
use super::interp::{Interp, PaintVal, Target};
use super::paint::{GradSpec, PatternVal, RECIPE_GRAD_BASE};
use super::{ImgHandle, PicSrc};

/// Canvases of one recipe, and gradients and patterns it makes: bounded, the recipe is untrusted.
const MAX_CANVASES: usize = 256;
const MAX_OBJECTS: usize = 65_536;

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.p.checked_add(n)?;
        let s = self.b.get(self.p..end)?;
        self.p = end;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
    }

    fn f64(&mut self) -> Option<f64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Some(f64::from_le_bytes(a))
    }

    fn f64s(&mut self, n: usize) -> Option<Vec<f64>> {
        if n > self.b.len() / 8 + 1 {
            return None;
        }
        (0..n).map(|_| self.f64()).collect()
    }

    /// A string of UTF-16 code units (`u32` count, then the units).
    fn str(&mut self) -> Option<String> {
        let n = usize::try_from(self.u32()?).ok()?;
        let raw = self.take(n.checked_mul(2)?)?;
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        Some(String::from_utf16_lossy(&units))
    }
}

struct GradBuild {
    id: u32,
    coords: Vec<f64>,
    stops: Vec<(f64, super::css::Rgba)>,
}

impl Interp<'_, '_> {
    /// Plays recipe bitmap `i` into a form: the picture as vector content (`None` when the bytes are no recipe).
    pub(super) fn play_recipe(&mut self, i: u32) -> Option<ImgHandle> {
        let b = self.res.bitmap_entries().get(usize::try_from(i).ok()?)?;
        let bytes = self.res.bitmap_bytes(b.data)?;
        if let Some((base, fx)) = uo_dml_render::metafile::rec::decode_fx_recipe(bytes) {
            // a derived recipe: its base (a plain recipe, never another derived one) with the colours mapped
            let bb = self.res.bitmap_entries().get(usize::try_from(base).ok()?)?;
            let base_bytes = self.res.bitmap_bytes(bb.data)?;
            if base_bytes.get(4) != Some(&1) {
                return None;
            }
            let base_bytes = Rc::new(base_bytes.to_vec());
            return self.play_recipe_bytes(&base_bytes, Some(Rc::new(FxMap::new(&fx))));
        }
        let bytes = Rc::new(bytes.to_vec());
        self.play_recipe_bytes(&bytes, None)
    }

    fn play_recipe_bytes(&mut self, bytes: &Rc<Vec<u8>>, fx: Option<Rc<FxMap>>) -> Option<ImgHandle> {
        let mut r = Rd { b: bytes, p: 0 };
        if r.take(5)? != [0x55, 0x4f, 0x4d, 0x46, 1] {
            return None;
        }
        let (w, h) = (f64::from(r.u32()?), f64::from(r.u32()?));
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        if r.u8()? != 0 {
            self.count("image:recipe-truncated");
        }
        let n_blobs = usize::try_from(r.u32()?).ok()?;
        if n_blobs > bytes.len() {
            return None;
        }
        let mut blobs: Vec<(usize, usize)> = Vec::with_capacity(n_blobs);
        for _ in 0..n_blobs {
            let n = usize::try_from(r.u32()?).ok()?;
            blobs.push((r.p, n));
            r.take(n)?;
        }
        let n_ops = usize::try_from(r.u32()?).ok()?;
        let mut run = Recipe::new(w, h);
        run.fx = fx;
        self.w.report.recipes += 1;
        for _ in 0..n_ops.min(bytes.len()) {
            if self.halt || self.recipe_op(&mut r, &mut run, &blobs, bytes).is_none() {
                break;
            }
        }
        // the picture is the root canvas, what was drawn before a cut or an error included
        let root = self.canvas_form(&mut run, 0)?;
        Some(root)
    }

    /// The form of canvas `k` as it is now (a transparency group of the canvas's size).
    fn canvas_form(&mut self, run: &mut Recipe, k: usize) -> Option<ImgHandle> {
        let (w, h) = *run.dims.get(k)?;
        let t = run.canvases.get(k)?;
        let mut content = t.buf.clone();
        for _ in 0..t.saves.len() {
            content.extend_from_slice(b"Q\n");
        }
        if let Some((len, form)) = run.forms.get(k).copied().flatten()
            && len == t.buf.len() + t.saves.len() * 2
        {
            return Some(ImgHandle { src: PicSrc::Form(form), w, h });
        }
        let tag = t.buf.len() + t.saves.len() * 2;
        let doc = self.w.doc.as_mut()?;
        let r = doc.add_form([0.0, 0.0, w, h], content, Some("DeviceRGB"));
        let form = self.w.note(r)?;
        if let Some(slot) = run.forms.get_mut(k) {
            *slot = Some((tag, form));
        }
        Some(ImgHandle { src: PicSrc::Form(form), w, h })
    }

    /// Runs one op on the canvas selected; `None` ends the play (bytes that do not parse).
    #[allow(clippy::too_many_lines)]
    fn recipe_op(&mut self, r: &mut Rd<'_>, run: &mut Recipe, blobs: &[(usize, usize)], all: &[u8]) -> Option<()> {
        let op = r.u8()?;
        // ops that do not touch a canvas
        match op {
            0 => {
                let k = usize::try_from(r.u32()?).ok()?;
                if k >= run.canvases.len() {
                    return None;
                }
                run.cur = k;
                return Some(());
            }
            1 => {
                let (w, h) = (f64::from(r.u32()?), f64::from(r.u32()?));
                if run.canvases.len() >= MAX_CANVASES {
                    return None;
                }
                run.canvases.push(Target::new(Mat::IDENTITY, None, Mat::IDENTITY));
                run.dims.push((w, h));
                run.forms.push(None);
                return Some(());
            }
            36 => {
                let m = r.f64s(4)?;
                if run.grads.len() >= MAX_OBJECTS {
                    return None;
                }
                let id = RECIPE_GRAD_BASE.wrapping_add(self.recipe_serial);
                self.recipe_serial = self.recipe_serial.wrapping_add(1);
                run.grads.push(GradBuild { id, coords: m, stops: Vec::new() });
                return Some(());
            }
            37 => {
                let g = usize::try_from(r.u32()?).ok()?;
                let off = r.f64()?;
                let color = r.str()?;
                let c = parse_css(&color).map(|c| run.map(c));
                if let (Some(gb), Some(c)) = (run.grads.get_mut(g), c)
                    && gb.stops.len() < MAX_OBJECTS
                {
                    let o = if off.is_nan() { 0.0 } else { max(0.0, min(1.0, off)) };
                    let o = gb.stops.last().map_or(o, |(prev, _)| max(*prev, o));
                    gb.stops.push((o, c));
                }
                return Some(());
            }
            38 => {
                let k = usize::try_from(r.u32()?).ok()?;
                if run.pats.len() >= MAX_OBJECTS {
                    return None;
                }
                let pat =
                    self.canvas_form(run, k).map(|img| Rc::new(PatternVal { img, mat: Mat::IDENTITY, flip: None, repeat: Repeat::Repeat }));
                run.pats.push(pat);
                return Some(());
            }
            39 => {
                let p = usize::try_from(r.u32()?).ok()?;
                let m = r.f64s(6)?;
                if let Some(Some(old)) = run.pats.get(p).cloned() {
                    let mat = Mat([m[0], m[1], m[2], m[3], m[4], m[5]]);
                    if mat.finite() {
                        run.pats[p] = Some(Rc::new(PatternVal { img: old.img.clone(), mat, flip: old.flip, repeat: old.repeat }));
                    }
                }
                return Some(());
            }
            41 => {
                // `xorRect(color, x0, y0, x1, y1, skip)`: no exact XOR in a PDF; the approximation that follows is played
                r.u32()?;
                r.f64s(4)?;
                r.u32()?;
                self.count("meta:xor-rect");
                return Some(());
            }
            _ => {}
        }
        // the ops on the selected canvas: it becomes the interpreter's target for the op
        let cur = run.cur;
        let canvas = run.canvases.get_mut(cur)?;
        std::mem::swap(&mut self.t, canvas);
        let out = self.recipe_canvas_op(op, r, run, blobs, all);
        if let Some(canvas) = run.canvases.get_mut(cur) {
            std::mem::swap(&mut self.t, canvas);
        }
        out
    }

    fn recipe_canvas_op(&mut self, op: u8, r: &mut Rd<'_>, run: &mut Recipe, blobs: &[(usize, usize)], all: &[u8]) -> Option<()> {
        let ctm = self.t.cur.ctm;
        match op {
            2 => self.save(),
            3 => self.restore(),
            4 => {
                let m = r.f64s(2)?;
                self.cm(Mat::translate(m[0], m[1]));
            }
            5 => {
                let a = r.f64()?;
                self.cm(Mat::rotate(a));
            }
            6 => {
                let m = r.f64s(2)?;
                self.cm(Mat::scale(m[0], m[1]));
            }
            7 => {
                let m = r.f64s(6)?;
                self.cm(Mat([m[0], m[1], m[2], m[3], m[4], m[5]]));
            }
            8 | 9 => {
                let t = r.u8()?;
                let v = match t {
                    0 => parse_css(&r.str()?).map(|c| PaintVal::Css(run.map(c))),
                    1 => {
                        let i = usize::try_from(r.u32()?).ok()?;
                        run.grads.get(i).map(|g| {
                            PaintVal::Spec(Rc::new(GradSpec { id: g.id, radial: false, coords: g.coords.clone(), stops: g.stops.clone() }))
                        })
                    }
                    _ => {
                        let i = usize::try_from(r.u32()?).ok()?;
                        run.pats.get(i).cloned().flatten().map(PaintVal::Pattern)
                    }
                };
                if let Some(v) = v {
                    if op == 8 {
                        self.t.cur.fill = v;
                        self.t.cur.pdf_fill = None;
                    } else {
                        self.t.cur.stroke = v;
                        self.t.cur.pdf_stroke = None;
                    }
                }
            }
            10 => {
                let v = r.f64()?;
                if v.is_finite() && v > 0.0 {
                    self.t.cur.line.width = v;
                }
            }
            11 => {
                self.t.cur.line.cap = [Cap::Butt, Cap::Round, Cap::Square].get(usize::from(r.u8()?)).copied().unwrap_or(Cap::Butt);
            }
            12 => {
                self.t.cur.line.join = [Join::Round, Join::Bevel, Join::Miter].get(usize::from(r.u8()?)).copied().unwrap_or(Join::Miter);
            }
            13 => {
                let v = r.f64()?;
                if v.is_finite() && v > 0.0 {
                    self.t.cur.line.miter = v;
                }
            }
            14 => {
                let v = r.f64()?;
                if v.is_finite() {
                    self.t.cur.line.phase = v;
                }
            }
            15 => {
                let v = r.f64()?;
                if v.is_finite() && (0.0..=1.0).contains(&v) {
                    self.t.cur.alpha = v;
                }
            }
            16 => {
                let c = r.u8()?;
                self.t.cur.diff = c == 1;
                self.t.cur.comp = match c {
                    2 => CompositeOp::Multiply,
                    3 => CompositeOp::Lighten,
                    _ => CompositeOp::SourceOver,
                };
            }
            17 => run.font = r.str()?,
            18 | 20 => {}
            19 => {
                r.u8()?;
            }
            21 => {
                let m = usize::try_from(r.u32()?).ok()?;
                let v = r.f64s(m)?;
                self.set_dash(&v);
            }
            22 => self.t.path.clear(),
            23 => self.t.path.close(ctm),
            24 => {
                let m = r.f64s(2)?;
                self.t.path.move_to(ctm, m[0], m[1]);
            }
            25 => {
                let m = r.f64s(2)?;
                self.t.path.line_to(ctm, m[0], m[1]);
            }
            26 => {
                let m = r.f64s(6)?;
                self.t.path.bezier_to(ctm, m[0], m[1], m[2], m[3], m[4], m[5]);
            }
            27 => {
                let m = r.f64s(4)?;
                self.t.path.rect(ctm, m[0], m[1], m[2], m[3]);
            }
            28 => {
                let m = r.f64s(5)?;
                let ccw = r.u8()? != 0;
                self.t.path.ellipse(ctm, m[0], m[1], m[2], m[2], 0.0, m[3], m[4], ccw);
            }
            29 => {
                let m = r.f64s(7)?;
                let ccw = r.u8()? != 0;
                self.t.path.ellipse(ctm, m[0], m[1], m[2], m[3], m[4], m[5], m[6], ccw);
            }
            30 => {
                let rule = if r.u8()? == 1 { FillRule::EvenOdd } else { FillRule::NonZero };
                self.fill_current(rule);
            }
            31 => self.stroke_current(),
            32 => {
                let rule = if r.u8()? == 1 { FillRule::EvenOdd } else { FillRule::NonZero };
                self.clip_current(rule);
            }
            33 => {
                let m = r.f64s(4)?;
                self.fill_rect(&Rect4 { x: m[0], y: m[1], w: m[2], h: m[3] });
            }
            34 => {
                let text = r.str()?;
                let m = r.f64s(2)?;
                let font = run.font.clone();
                self.recipe_text(&font, &text, m[0], m[1]);
            }
            35 => {
                let kind = r.u8()?;
                let k = usize::try_from(r.u32()?).ok()?;
                let n = usize::from(r.u8()?);
                let a = r.f64s(n.checked_sub(1)?)?;
                let img = if kind == 0 {
                    // a canvas of the recipe, as it is now (the current canvas cannot draw itself)
                    if k == run.cur {
                        None
                    } else {
                        // the canvas being swapped out is `run.cur`: its form is made from the slot, so swap back for a moment
                        self.canvas_form_of(run, k)
                    }
                } else {
                    let &(start, len) = blobs.get(k)?;
                    let data = all.get(start..start.checked_add(len)?)?;
                    match run.fx.clone() {
                        None => self.embed_picture(data, None),
                        Some(fx) => self.fx_picture(data, &fx),
                    }
                };
                if let Some(img) = img {
                    match n {
                        3 => self.draw_handle(&img, None, &Rect4 { x: a[0], y: a[1], w: img.w, h: img.h }),
                        5 => self.draw_handle(&img, None, &Rect4 { x: a[0], y: a[1], w: a[2], h: a[3] }),
                        _ if a.len() >= 8 => self.draw_handle(
                            &img,
                            Some(&Rect4 { x: a[0], y: a[1], w: a[2], h: a[3] }),
                            &Rect4 { x: a[4], y: a[5], w: a[6], h: a[7] },
                        ),
                        _ => {}
                    }
                }
            }
            40 => {
                let blob = usize::try_from(r.u32()?).ok()?;
                let (w, h) = (r.u32()?, r.u32()?);
                let m = r.f64s(2)?;
                let &(start, len) = blobs.get(blob)?;
                let want = usize::try_from(w).ok()?.checked_mul(usize::try_from(h).ok()?)?.checked_mul(4)?;
                let data = all.get(start..start.checked_add(len)?)?;
                if w > 0 && h > 0 && data.len() >= want {
                    match run.fx.clone() {
                        None => self.put_pixels(data.get(..want)?, w, h, m[0], m[1]),
                        Some(fx) => {
                            let mut px = data.get(..want)?.to_vec();
                            fx.map_px(&mut px);
                            self.put_pixels(&px, w, h, m[0], m[1]);
                        }
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// The form of canvas `k` while another canvas is swapped into the interpreter.
    fn canvas_form_of(&mut self, run: &mut Recipe, k: usize) -> Option<ImgHandle> {
        let cur = run.cur;
        if let Some(c) = run.canvases.get_mut(cur) {
            std::mem::swap(&mut self.t, c);
        }
        let out = self.canvas_form(run, k);
        if let Some(c) = run.canvases.get_mut(cur) {
            std::mem::swap(&mut self.t, c);
        }
        out
    }

    /// A bitmap of a derived recipe: decoded and mapped (a JPEG through uo-image's `jpeg` decoder; one that does not decode is
    /// drawn as it is and counted `image:jpeg-effect`).
    fn fx_picture(&mut self, data: &[u8], fx: &FxMap) -> Option<ImgHandle> {
        if uo_image::jpeg_info(data).is_some() {
            return match uo_image::decode_jpeg(data, &self.w.opts.image_limits) {
                Ok(mut r) => {
                    fx.map_px(&mut r.data);
                    self.embed_rgba_picture(&r)
                }
                Err(_) => {
                    self.count("image:jpeg-effect");
                    self.embed_picture(data, None)
                }
            };
        }
        let Ok(mut r) = uo_image::decode_raster(data, &self.w.opts.image_limits) else { return self.embed_picture(data, None) };
        fx.map_px(&mut r.data);
        let doc = self.w.doc.as_mut()?;
        let e = doc.embed_rgba(&r.data, r.width, r.height);
        let pdf = self.w.note(e)?;
        self.w.report.images += 1;
        Some(ImgHandle { src: PicSrc::Image(pdf), w: f64::from(r.width), h: f64::from(r.height) })
    }

    /// A JPEG picture whose colour effects need its pixels (grayscale, duotone, bi-level, colour change, or brightness/contrast
    /// of a CMYK JPEG): decoded (uo-image `jpeg`), `fxImage` applied, embedded as RGBA. `None` when it does not decode (the
    /// caller embeds it as it is and counts `image:jpeg-effect`). Called by `images::embed_picture`.
    pub(super) fn jpeg_fx_picture(&mut self, data: &[u8], fx: &uo_render::slides::ImageFx) -> Option<ImgHandle> {
        let mut r = uo_image::decode_jpeg(data, &self.w.opts.image_limits).ok()?;
        // the colour strings first (the symbol table is read before the pixels are mapped)
        let ids = [
            fx.duotone.as_ref().map(|d| d.first),
            fx.duotone.as_ref().map(|d| d.second),
            fx.clr_change.as_ref().map(|c| c.from),
            fx.clr_change.as_ref().map(|c| c.to),
        ];
        let names: Vec<(u32, String)> = ids.into_iter().flatten().map(|i| (i, String::from_utf16_lossy(self.res.sym_units(i)))).collect();
        super::images::apply_fx(&mut r.data, fx, |s| names.iter().find(|(i, _)| *i == s).map(|(_, n)| n.clone()).unwrap_or_default());
        self.embed_rgba_picture(&r)
    }

    fn embed_rgba_picture(&mut self, r: &uo_image::Rgba) -> Option<ImgHandle> {
        let doc = self.w.doc.as_mut()?;
        let e = doc.embed_rgba(&r.data, r.width, r.height);
        let pdf = self.w.note(e)?;
        self.w.report.images += 1;
        Some(ImgHandle { src: PicSrc::Image(pdf), w: f64::from(r.width), h: f64::from(r.height) })
    }

    /// `putImageData`: the pixels at device position `(x, y)`, ignoring the transform, alpha and clip of the context.
    fn put_pixels(&mut self, rgba: &[u8], w: u32, h: u32, x: f64, y: f64) {
        if self.t.cur.dead || !(x.is_finite() && y.is_finite()) {
            return;
        }
        let Some(inv) = self.t.cur.ctm.inverse() else { return };
        let Some(doc) = self.w.doc.as_mut() else { return };
        let e = doc.embed_rgba(rgba, w, h);
        let Some(img) = self.w.note(e) else { return };
        self.w.report.images += 1;
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(1.0, None);
        let m = inv.0;
        self.put(&format!(
            "{} {} {} {} {} {} cm\n{} 0 0 {} {} {} cm\n/Im{} Do\nQ\n",
            nm(m[0]),
            nm(m[1]),
            nm(m[2]),
            nm(m[3]),
            nm(m[4]),
            nm(m[5]),
            nm(f64::from(w)),
            nm(-f64::from(h)),
            nm(x),
            nm(y + f64::from(h)),
            img_index(&img)
        ));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }
}

fn img_index(i: &uo_pdf::PdfImage) -> u32 {
    // `Im<index>`: the resource name carries the index
    i.resource_name().trim_start_matches("Im").parse().unwrap_or(0)
}

/// The state of one recipe play: its canvases (each a paint target), the objects it made.
struct Recipe {
    canvases: Vec<Target>,
    dims: Vec<(f64, f64)>,
    /// per canvas: the length the content had when its form was made, and the form
    forms: Vec<Option<(usize, u32)>>,
    cur: usize,
    font: String,
    grads: Vec<GradBuild>,
    pats: Vec<Option<Rc<PatternVal>>>,
    /// the colour effects of a derived recipe
    fx: Option<Rc<FxMap>>,
}

impl Recipe {
    /// A CSS colour of the recipe through the derived recipe's effects.
    fn map(&self, c: Rgba) -> Rgba {
        self.fx.as_ref().map_or(c, |f| f.map(c))
    }

    fn new(w: f64, h: f64) -> Recipe {
        Recipe {
            canvases: vec![Target::new(Mat::IDENTITY, None, Mat::IDENTITY)],
            dims: vec![(w, h)],
            forms: vec![None],
            cur: 0,
            font: "10px sans-serif".to_owned(),
            grads: Vec::new(),
            pats: Vec::new(),
            fx: None,
        }
    }
}

/// The effects of a derived recipe as `fxImage` takes them (`images::apply_fx`; symbols 0-3 are the colour strings).
struct FxMap {
    fx: uo_render::slides::ImageFx,
    names: [String; 4],
}

impl FxMap {
    fn new(f: &uo_dml_render::metafile::rec::RecipeFx) -> FxMap {
        let mut names: [String; 4] = Default::default();
        if let Some([a, b]) = &f.duotone {
            names[0].clone_from(a);
            names[1].clone_from(b);
        }
        if let Some((from, to, _)) = &f.clr_change {
            names[2].clone_from(from);
            names[3].clone_from(to);
        }
        let fx = uo_render::slides::ImageFx {
            duotone: f.duotone.as_ref().map(|_| uo_render::slides::Duotone { first: 0, second: 1 }),
            gray: f.gray,
            bi_level: f.bi_level,
            bright: f.bright,
            contrast: f.contrast,
            clr_change: f.clr_change.as_ref().map(|c| uo_render::slides::ClrChange { from: 2, to: 3, to_alpha: c.2 }),
        };
        FxMap { fx, names }
    }

    fn map_px(&self, px: &mut [u8]) {
        super::images::apply_fx(px, &self.fx, |s| self.names.get(usize::try_from(s).unwrap_or(usize::MAX)).cloned().unwrap_or_default());
    }

    fn map(&self, c: Rgba) -> Rgba {
        let [r, g, b] = c.bytes();
        let a8 = uo_jsnum::conv::to_uint8_clamp(c.3 * 255.0);
        let mut px = [r, g, b, a8];
        self.map_px(&mut px);
        let alpha = if px[3] == a8 { c.3 } else { f64::from(px[3]) / 255.0 };
        Rgba(f64::from(px[0]), f64::from(px[1]), f64::from(px[2]), alpha)
    }
}
