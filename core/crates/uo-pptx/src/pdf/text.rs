//! `TextRun` as glyphs: the face the host answered for the run's family, embedded once per font file (subset at
//! `build`), the glyph ids from its cmap, every glyph placed where the layout's measure puts it (advances, the pair
//! kerning, the letter spacing and the fit: the spread of the difference between the laid-out width and the natural
//! one) through `TJ` adjustments. A character the face lacks is drawn with the host's fallback face for it
//! ([`super::measure::FontSource::fallback`], the chain of the Write PDF, then the open fonts available), switching
//! fonts inside the text object; with no face at all it keeps the `.notdef` advance and is counted. Fills are a
//! colour, a gradient (the text as a clip, then the shading) or a pattern; the outline is a stroked second pass;
//! underline and strike are lines; a text shadow is the glyph outlines' shadow image ([`Interp::draw_shadow`]).

use std::sync::Arc;

use uo_fonts::Face;
use uo_jsnum::math::abs;
use uo_render::slides::{Align, TextRun};

use super::FontEntry;
use super::css::{Rgba, parse_css};
use super::effects::ShadowSrc;
use super::geom::{Mat, n};
use super::interp::{Interp, PaintVal, Shadow, span_of};
use super::measure::{FaceChooser, place_glyphs, text_width_in};

/// A font of the list as the PDF uses it: the embedded entry and the variant asked for.
#[derive(Clone)]
pub(super) struct RunFont {
    pub entry: Arc<FontEntry>,
    pub bold: bool,
    pub italic: bool,
}

impl Interp<'_, '_> {
    /// The embedded font of a face of the list (the host's answer for the family, one embedding per file).
    fn font_of(&mut self, face: u32) -> Option<RunFont> {
        let fk = self.res.face_entries().get(usize::try_from(face).ok()?)?.clone();
        let family = self.sym(fk.family);
        let entry = self.font_named(&family, fk.bold, fk.italic)?;
        Some(RunFont { entry, bold: fk.bold, italic: fk.italic })
    }

    /// The embedded font of a family and variant by name (the host's answer, one embedding per file).
    pub(super) fn font_named(&mut self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontEntry>> {
        let key = (family.to_owned(), bold, italic);
        if let Some(e) = self.w.fonts.get(&key) {
            return e.clone();
        }
        let entry = match self.w.opts.fonts.face(family, bold, italic) {
            Some(face) => self.embed_face(&face, family, bold, italic),
            None => {
                self.count("font:missing");
                None
            }
        };
        self.w.fonts.insert(key, entry.clone());
        entry
    }

    /// The embedded entry of a face (one per font file); `None` when its licence forbids embedding (counted).
    fn embed_face(&mut self, face: &Arc<Face>, family: &str, bold: bool, italic: bool) -> Option<Arc<FontEntry>> {
        let file = Arc::clone(face.bytes());
        let id = Arc::as_ptr(&file).cast::<u8>() as usize;
        if let Some(e) = self.w.font_files.get(&id) {
            return Some(Arc::clone(e));
        }
        // a file whose licence forbids embedding is never put in the PDF
        if !uo_fonts::is_embeddable(&file) {
            self.count("font:not-embeddable");
            return None;
        }
        let name = {
            let base: String = family.chars().filter(char::is_ascii_alphanumeric).collect();
            let base = if base.is_empty() { "Font".to_owned() } else { base };
            format!("{base}{}{}", if bold { "-Bold" } else { "" }, if italic { "-Italic" } else { "" })
        };
        let doc = self.w.doc.as_mut()?;
        let fid = doc.embed_font(&name, file, (**face).clone());
        let entry = Arc::new(FontEntry { id: fid, face: Arc::clone(face) });
        self.w.font_files.insert(id, Arc::clone(&entry));
        self.w.report.fonts += 1;
        Some(entry)
    }

    /// The width of `text` in the run's font with the fallback faces (the layout's measure).
    fn width_of(&self, f: &RunFont, size: f64, kern: bool, spacing: f64, text: &str) -> f64 {
        text_width_in(self.w.opts.fonts, &f.entry.face, f.bold, f.italic, size, kern, spacing, text)
    }

    /// `fillText(text, x, y)` of a metafile recipe under the raw CSS font `font` (`40px "Name", monospace`): the
    /// glyphs in the fill now, with the font's natural advances and kerning.
    pub(super) fn recipe_text(&mut self, font: &str, text: &str, x: f64, y: f64) {
        if self.t.cur.dead || text.is_empty() || !(x.is_finite() && y.is_finite()) || !self.can_paint(false) {
            return;
        }
        let (family, bold, italic, size) = super::measure::raw_css_font(font);
        if !(size.is_finite() && size > 0.0) {
            return;
        }
        let Some(entry) = self.font_named(&family, bold, italic) else { return };
        let f = RunFont { entry, bold, italic };
        self.w.report.text_runs += 1;
        let width = self.width_of(&f, size, true, 0.0, text);
        let fill = self.t.cur.fill.clone();
        self.fill_glyphs(&f, size, x, y, text, true, 0.0, &fill, 1.0, width);
    }

    /// `BT ... ET` of the glyphs of `text` at `(x, y)` (user space, baseline) and the number of glyphs drawn: each
    /// glyph at the pen position of the layout, in its face's embedded font.
    #[allow(clippy::too_many_arguments)]
    fn glyph_block(&mut self, f: &RunFont, size: f64, x: f64, y: f64, text: &str, kern: bool, spacing: f64, mode: u8) -> (String, u64) {
        let fonts = self.w.opts.fonts;
        let mut ch = FaceChooser::new(fonts, Arc::clone(&f.entry.face), f.bold, f.italic);
        let (placed, _) = place_glyphs(&mut ch, size, kern, spacing, text);
        let mut out = String::new();
        out.push_str("BT\n");
        if spacing != 0.0 {
            out.push_str(&format!("{} Tc\n", n(spacing)));
        }
        if mode != 0 {
            out.push_str(&format!("{mode} Tr\n"));
        }
        out.push_str(&format!("1 0 0 -1 {} {} Tm\n", n(x), n(y)));
        let (mut glyphs, mut missing, mut fallback) = (0u64, 0u64, 0u64);
        let mut cur_font: Option<uo_pdf::FontId> = None;
        let (mut open_arr, mut open_hex) = (false, false);
        // where the PDF pen is after the last glyph shown, px from the run's start
        let mut pen = 0.0f64;
        for g in &placed {
            let Some(face) = &g.face else {
                missing += 1;
                continue;
            };
            let entry = if Arc::ptr_eq(face, &f.entry.face) {
                Some(Arc::clone(&f.entry))
            } else {
                let fam = format!("Fallback{}", face.bytes().len());
                self.embed_face(face, &fam, f.bold, f.italic)
            };
            let Some(entry) = entry else {
                missing += 1;
                continue;
            };
            if g.gid == 0 || g.gid > 0xFFFF {
                missing += 1;
                continue;
            }
            if cur_font != Some(entry.id) {
                if open_hex {
                    out.push('>');
                    open_hex = false;
                }
                if open_arr {
                    out.push_str("] TJ\n");
                }
                out.push_str(&format!("/{} {} Tf\n[", entry.id.resource_name(), n(size)));
                open_arr = true;
                cur_font = Some(entry.id);
            }
            let d = g.x - pen;
            if abs(d) > 1e-6 {
                if open_hex {
                    out.push('>');
                    open_hex = false;
                }
                out.push_str(&n(-d * 1000.0 / size));
            }
            if let Some(doc) = self.w.doc.as_mut()
                && let Some(ef) = doc.font_mut(entry.id)
            {
                ef.encode(g.cp);
            }
            if !open_hex {
                out.push('<');
                open_hex = true;
            }
            out.push_str(&format!("{:04x}", g.gid));
            glyphs += 1;
            if !Arc::ptr_eq(&entry, &f.entry) {
                fallback += 1;
            }
            let upem = f64::from(entry.face.metrics().units_per_em);
            pen = g.x + f64::from(entry.face.advance_width(g.gid)) * size / upem + spacing;
        }
        if open_hex {
            out.push('>');
        }
        if open_arr {
            out.push_str("] TJ\n");
        }
        // A clip mode (4-7) stays set until `ET`: pdfium (Chrome/Edge) adds the text clip at `ET` only when the mode
        // in force there is a clip mode, and otherwise paints what follows unclipped. Clip blocks are always inside a
        // `q`/`Q` pair (`fill_spec`), so the `Q` restores the mode.
        if mode != 0 && mode < 4 {
            out.push_str("0 Tr\n");
        }
        if spacing != 0.0 {
            out.push_str("0 Tc\n");
        }
        out.push_str("ET\n");
        for _ in 0..missing {
            self.count("glyph:missing");
        }
        self.w.report.fallback_glyphs += fallback;
        (out, glyphs)
    }

    /// `ctx.fillText(text, x, y)` under the current font, fill, alignment and baseline (the chart engine's labels):
    /// the font's natural advances with kerning, no fit.
    pub(super) fn fill_text(&mut self, text: &str, x0: f64, y0: f64) {
        if self.t.cur.dead || text.is_empty() || !(x0.is_finite() && y0.is_finite()) || !self.can_paint(false) {
            return;
        }
        let Some((face, size)) = self.t.cur.font else { return };
        if !(size.is_finite() && size > 0.0) {
            return;
        }
        let Some(f) = self.font_of(face) else { return };
        self.w.report.text_runs += 1;
        let width = self.width_of(&f, size, true, 0.0, text);
        let m = f.entry.face.metrics();
        let upem = f64::from(m.units_per_em);
        let (asc, desc) = (f64::from(m.ascender) * size / upem, -f64::from(m.descender) * size / upem);
        let x = match self.t.cur.align {
            Align::Center => x0 - width / 2.0,
            Align::Right | Align::End => x0 - width,
            _ => x0,
        };
        let y = match self.t.cur.baseline {
            uo_render::slides::Baseline::Alphabetic => y0,
            uo_render::slides::Baseline::Top => y0 + asc,
            uo_render::slides::Baseline::Hanging => y0 + asc * 0.8,
            uo_render::slides::Baseline::Middle => y0 + (asc - desc) / 2.0,
            uo_render::slides::Baseline::Ideographic | uo_render::slides::Baseline::Bottom => y0 - desc,
        };
        let fill = self.t.cur.fill.clone();
        self.fill_glyphs(&f, size, x, y, text, true, 0.0, &fill, 1.0, width);
    }

    pub(super) fn text_run(&mut self, index: u32, r: &TextRun) {
        if self.t.cur.dead || !self.can_paint(false) {
            return;
        }
        let Some(units) = span_of(&self.list.texts, &r.text) else { return };
        let text = String::from_utf16_lossy(units);
        if text.is_empty() {
            return;
        }
        let size = r.font.size_px;
        let (x0, y) = (r.x, r.y);
        if !(size.is_finite() && size > 0.0 && x0.is_finite() && y.is_finite()) {
            return;
        }
        if !r.bullet {
            self.links.mark(index, self.t.cur.ctm, self.t.cur.align);
        }
        let Some(f) = self.font_of(r.font.face) else { return };
        self.w.report.text_runs += 1;
        let kern = r.kern || r.bullet;
        let nchars = text.chars().count();
        let natural = self.width_of(&f, size, kern, 0.0, &text);
        // the style the TS leaves on the context
        if let Some(v) = self.paint_val(&r.fill) {
            self.t.cur.fill = v;
            self.t.cur.pdf_fill = None;
        }
        let fill = self.t.cur.fill.clone();
        if r.bullet {
            self.fill_glyphs(&f, size, x0, y, &text, true, 0.0, &fill, 1.0, natural);
            return;
        }
        let ls = r.letter_spacing_px;
        let mut spacing = if ls.is_finite() { ls } else { 0.0 };
        if spacing == 0.0
            && r.fit_allowed
            && let Some(laid) = r.laid_w
            && nchars > 1
        {
            let d = laid - natural;
            if abs(d) > 0.25 {
                spacing = d / nchars as f64;
            }
        }
        let width = natural + spacing * nchars as f64;
        let x = match self.t.cur.align {
            Align::Center => x0 - width / 2.0,
            Align::Right | Align::End => x0 - width,
            _ => x0,
        };
        let a_mul = r.alpha_mul.unwrap_or(1.0);
        if r.fill_text {
            // the run's own shadow is the canvas shadow of the fill pass (save, shadow*, fillText, restore)
            let saved = self.t.cur.shadow;
            if let Some(sh) = &r.shadow {
                let c = parse_css(&self.sym(sh.color)).unwrap_or(Rgba(0.0, 0.0, 0.0, 0.0));
                self.t.cur.shadow = Shadow { alpha: c.3, rgb: c.bytes(), blur: sh.blur, ox: sh.dx, oy: sh.dy };
            }
            let saved_alpha = self.t.cur.alpha;
            self.t.cur.alpha = saved_alpha * a_mul;
            self.fill_glyphs(&f, size, x, y, &text, kern, spacing, &fill, 1.0, width);
            self.t.cur.alpha = saved_alpha;
            self.t.cur.shadow = saved;
        }
        if let Some(o) = &r.outline
            && let Some(c) = parse_css(&self.sym(o.color))
            && self.t.cur.alpha * c.3 > 0.0
            && o.width.is_finite()
            && o.width > 0.0
        {
            let a = self.t.cur.alpha * c.3;
            let (blk, g) = self.glyph_block(&f, size, x, y, &text, kern, spacing, 1);
            self.w.report.glyphs += g;
            let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
            self.put("q\n");
            self.sync_gs(a, None);
            let b = c.bytes();
            self.put(&format!(
                "{} {} {} RG\n{} w\n1 j\n{blk}Q\n",
                super::geom::nm(f64::from(b[0]) / 255.0),
                super::geom::nm(f64::from(b[1]) / 255.0),
                super::geom::nm(f64::from(b[2]) / 255.0),
                n(o.width)
            ));
            (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
        }
        let line_w = r.laid_w.unwrap_or(width);
        for d in [&r.underline, &r.strike].into_iter().flatten() {
            self.text_line(d.color, d.width, d.y, x, line_w);
        }
    }

    fn text_line(&mut self, color: u32, width: f64, y: f64, x: f64, w: f64) {
        let Some(c) = super::css::parse_css(&self.sym(color)) else { return };
        let a = self.t.cur.alpha * c.3;
        if a <= 0.0 || !(width.is_finite() && width > 0.0 && y.is_finite() && w.is_finite()) {
            return;
        }
        let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
        self.put("q\n");
        self.sync_gs(a, None);
        let b = c.bytes();
        let cap = match self.t.cur.line.cap {
            uo_render::slides::Cap::Butt => 0,
            uo_render::slides::Cap::Round => 1,
            uo_render::slides::Cap::Square => 2,
        };
        self.put(&format!(
            "{} {} {} RG\n{} w\n{cap} J\n[] 0 d\n{} {} m {} {} l S\nQ\n",
            super::geom::nm(f64::from(b[0]) / 255.0),
            super::geom::nm(f64::from(b[1]) / 255.0),
            super::geom::nm(f64::from(b[2]) / 255.0),
            n(width),
            n(x),
            n(y),
            n(x + w),
            n(y)
        ));
        (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
    }

    /// The shadow of a fill pass of text: the glyph outlines (device) weighed by the fill's alpha.
    #[allow(clippy::too_many_arguments)]
    fn text_shadow(&mut self, f: &RunFont, size: f64, x: f64, y: f64, text: &str, kern: bool, spacing: f64, fill: &PaintVal) {
        let fonts = self.w.opts.fonts;
        let mut ch = FaceChooser::new(fonts, Arc::clone(&f.entry.face), f.bold, f.italic);
        let (placed, _) = place_glyphs(&mut ch, size, kern, spacing, text);
        let ctm = self.t.cur.ctm;
        let polys = super::raster::glyph_polys(&placed, size, x, y, ctm.then(Mat::IDENTITY), 0.1);
        let paint = self.alpha_paint(fill);
        self.draw_shadow(ShadowSrc::Polys(polys, false, paint));
    }

    /// The fill pass of a run in the paint now (and its shadow, when the context has one).
    #[allow(clippy::too_many_arguments)]
    fn fill_glyphs(
        &mut self,
        f: &RunFont,
        size: f64,
        x: f64,
        y: f64,
        text: &str,
        kern: bool,
        spacing: f64,
        fill: &PaintVal,
        a_mul: f64,
        width: f64,
    ) {
        if self.shadow_on() {
            self.text_shadow(f, size, x, y, text, kern, spacing, fill);
            if self.offscreen() {
                return;
            }
        }
        let alpha = self.t.cur.alpha * a_mul;
        match fill {
            PaintVal::Linear(i) | PaintVal::Radial(i) => {
                let radial = matches!(fill, PaintVal::Radial(_));
                let (blk, g) = self.glyph_block(f, size, x, y, text, kern, spacing, 7);
                // no glyph shown: an empty text clip (pdfium ignores it at `ET` and paints the shading unclipped)
                if g == 0 {
                    return;
                }
                let bounds = [x, y - size, x + width, y + size * 0.3];
                if self.fill_gradient(radial, *i, &blk, bounds, a_mul) {
                    self.w.report.glyphs += g;
                }
            }
            PaintVal::Spec(g) => {
                let (blk, gl) = self.glyph_block(f, size, x, y, text, kern, spacing, 7);
                if gl == 0 {
                    return;
                }
                let bounds = [x, y - size, x + width, y + size * 0.3];
                if self.fill_spec(g, &blk, bounds, a_mul) {
                    self.w.report.glyphs += gl;
                }
            }
            PaintVal::Pattern(pv) => {
                let Some(pt) = self.tiling_pattern(pv) else { return };
                if alpha <= 0.0 {
                    return;
                }
                let (blk, g) = self.glyph_block(f, size, x, y, text, kern, spacing, 0);
                self.w.report.glyphs += g;
                let saved = (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke);
                self.put("q\n");
                self.sync_gs(alpha, None);
                self.put(&format!("/Pattern cs /Pt{pt} scn\n{blk}Q\n"));
                (self.t.cur.pdf_gs, self.t.cur.pdf_fill, self.t.cur.pdf_stroke) = saved;
            }
            other => {
                let c: Rgba = self.solid_of(other).unwrap_or(Rgba::BLACK);
                if matches!(other, PaintVal::Conic(_)) {
                    self.count("gradient:conic");
                }
                let a = alpha * c.3;
                if a <= 0.0 {
                    return;
                }
                let (blk, g) = self.glyph_block(f, size, x, y, text, kern, spacing, 0);
                self.w.report.glyphs += g;
                self.sync_gs(a, None);
                self.sync_fill_color(c);
                self.put(&blk);
            }
        }
    }
}
