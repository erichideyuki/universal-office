//! The metafile recipes (B5b): a EMF, EMF+, WMF, PICT or TIFF picture arrives as a bitmap whose bytes are the canvas calls
//! the TS rasterizer made (`uo_dml_render::metafile::rec`, "UOMF"; the shell replays them in `meta-player.ts`). The PDF
//! plays them as vector content: each canvas of the recipe is a form XObject (a transparency group in the canvas space,
//! y down), the picture is the root canvas, and paths, fills, strokes, clips, transforms, gradients, patterns, text and
//! images go through the same interpreter as the slide itself. `putImageData` is an image at device coordinates; the
//! exact XOR (`xorRect`) has no PDF form and is counted (`meta:xor-rect`; the approximation that follows it is drawn).

use std::rc::Rc;

use uo_jsnum::math::{max, min};
use uo_render::slides::{Cap, CompositeOp, FillRule, Join, Rect4, Repeat};

use super::css::parse_css;
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
        let bytes = Rc::new(self.res.bitmap_bytes(b.data)?.to_vec());
        let mut r = Rd { b: &bytes, p: 0 };
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
        self.w.report.recipes += 1;
        for _ in 0..n_ops.min(bytes.len()) {
            if self.halt || self.recipe_op(&mut r, &mut run, &blobs, &bytes).is_none() {
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
                if let (Some(gb), Some(c)) = (run.grads.get_mut(g), parse_css(&color))
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
                    0 => parse_css(&r.str()?).map(PaintVal::Css),
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
                    self.embed_picture(data, None)
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
                    self.put_pixels(data.get(..want)?, w, h, m[0], m[1]);
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
}

impl Recipe {
    fn new(w: f64, h: f64) -> Recipe {
        Recipe {
            canvases: vec![Target::new(Mat::IDENTITY, None, Mat::IDENTITY)],
            dims: vec![(w, h)],
            forms: vec![None],
            cur: 0,
            font: "10px sans-serif".to_owned(),
            grads: Vec::new(),
            pats: Vec::new(),
        }
    }
}
