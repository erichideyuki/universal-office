//! `TextRun` as glyphs: the face the host answered for the run's family, embedded once per font file (subset at
//! `build`), the glyph ids from its cmap, the kerning pairs as `TJ` adjustments, the letter spacing and the fit
//! (the spread of the difference between the laid-out width and the natural one) as `Tc`. Fills are a colour, a
//! gradient (the text as a clip, then the shading) or a pattern; the outline is a stroked second pass; underline
//! and strike are lines.

use std::sync::Arc;

use uo_jsnum::math::abs;
use uo_render::slides::{Align, TextRun};

use super::FontEntry;
use super::css::Rgba;
use super::geom::n;
use super::interp::{Interp, PaintVal, span_of};
use super::measure::text_width;

impl Interp<'_, '_> {
    /// The embedded font of a face of the list (the host's answer for the family, one embedding per file).
    fn font_of(&mut self, face: u32) -> Option<Arc<FontEntry>> {
        let fk = self.res.face_entries().get(usize::try_from(face).ok()?)?.clone();
        let family = self.sym(fk.family);
        self.font_named(&family, fk.bold, fk.italic)
    }

    /// The embedded font of a family and variant by name (the host's answer, one embedding per file).
    pub(super) fn font_named(&mut self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontEntry>> {
        let key = (family.to_owned(), bold, italic);
        if let Some(e) = self.w.fonts.get(&key) {
            return e.clone();
        }
        let entry = self.make_font(family, bold, italic);
        self.w.fonts.insert(key, entry.clone());
        entry
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
        self.w.report.text_runs += 1;
        let width = text_width(&entry.face, size, true, 0.0, text);
        let fill = self.t.cur.fill.clone();
        self.fill_glyphs(&entry, size, x, y, text, true, 0.0, &fill, 1.0, width);
    }

    fn make_font(&mut self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontEntry>> {
        let Some(face) = self.w.opts.fonts.face(family, bold, italic) else {
            self.count("font:missing");
            return None;
        };
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
        let fid = doc.embed_font(&name, file, (*face).clone());
        let entry = Arc::new(FontEntry { id: fid, face });
        self.w.font_files.insert(id, Arc::clone(&entry));
        self.w.report.fonts += 1;
        Some(entry)
    }

    /// `BT ... ET` of the glyphs of `text` at `(x, y)` (user space, baseline) and the number of glyphs drawn.
    #[allow(clippy::too_many_arguments)]
    fn glyph_block(&mut self, e: &FontEntry, size: f64, x: f64, y: f64, text: &str, kern: bool, spacing: f64, mode: u8) -> (String, u64) {
        let upem = f64::from(e.face.metrics().units_per_em);
        let mut out = String::new();
        out.push_str("BT\n");
        out.push_str(&format!("/{} {} Tf\n", e.id.resource_name(), n(size)));
        if spacing != 0.0 {
            out.push_str(&format!("{} Tc\n", n(spacing)));
        }
        if mode != 0 {
            out.push_str(&format!("{mode} Tr\n"));
        }
        out.push_str(&format!("1 0 0 -1 {} {} Tm\n[", n(x), n(y)));
        let mut glyphs = 0u64;
        let mut missing = 0u64;
        let mut open = false;
        let mut prev: Option<u32> = None;
        for ch in text.chars() {
            let cp = u32::from(ch);
            let gid = e.face.glyph_id(cp);
            if gid == 0 || gid > 0xFFFF {
                missing += 1;
                // nothing to draw: advance by the width of .notdef
                if open {
                    out.push('>');
                    open = false;
                }
                let adv = f64::from(e.face.advance_width(0));
                out.push_str(&n(-adv * 1000.0 / upem));
                prev = None;
                continue;
            }
            if kern && let Some(p) = prev {
                let k = e.face.kern_units(p, gid);
                if k != 0 {
                    if open {
                        out.push('>');
                        open = false;
                    }
                    out.push_str(&n(-f64::from(k) * 1000.0 / upem));
                }
            }
            if let Some(doc) = self.w.doc.as_mut()
                && let Some(f) = doc.font_mut(e.id)
            {
                f.encode(cp);
            }
            if !open {
                out.push('<');
                open = true;
            }
            out.push_str(&format!("{gid:04x}"));
            glyphs += 1;
            prev = Some(gid);
        }
        if open {
            out.push('>');
        }
        out.push_str("] TJ\n");
        if mode != 0 {
            out.push_str("0 Tr\n");
        }
        if spacing != 0.0 {
            out.push_str("0 Tc\n");
        }
        out.push_str("ET\n");
        for _ in 0..missing {
            self.count("glyph:missing");
        }
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
        let Some(entry) = self.font_of(face) else { return };
        self.w.report.text_runs += 1;
        let width = text_width(&entry.face, size, true, 0.0, text);
        let m = entry.face.metrics();
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
        self.fill_glyphs(&entry, size, x, y, text, true, 0.0, &fill, 1.0, width);
    }

    pub(super) fn text_run(&mut self, r: &TextRun) {
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
        let Some(entry) = self.font_of(r.font.face) else { return };
        self.w.report.text_runs += 1;
        let kern = r.kern || r.bullet;
        let nchars = text.chars().count();
        let natural = text_width(&entry.face, size, kern, 0.0, &text);
        // the style the TS leaves on the context
        if let Some(v) = self.paint_val(&r.fill) {
            self.t.cur.fill = v;
            self.t.cur.pdf_fill = None;
        }
        let fill = self.t.cur.fill.clone();
        if r.bullet {
            self.fill_glyphs(&entry, size, x0, y, &text, true, 0.0, &fill, 1.0, natural);
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
        if r.shadow.is_some() {
            self.count("shadow");
        }
        if self.placed.len() < 1_000_000 {
            self.placed.push(super::interp::Placed { text: text.clone(), x, y, size, w: r.laid_w.unwrap_or(width) });
        }
        let a_mul = r.alpha_mul.unwrap_or(1.0);
        if r.fill_text {
            self.fill_glyphs(&entry, size, x, y, &text, kern, spacing, &fill, a_mul, width);
        }
        if let Some(o) = &r.outline
            && let Some(c) = super::css::parse_css(&self.sym(o.color))
            && self.t.cur.alpha * c.3 > 0.0
            && o.width.is_finite()
            && o.width > 0.0
        {
            let a = self.t.cur.alpha * c.3;
            let (blk, g) = self.glyph_block(&entry, size, x, y, &text, kern, spacing, 1);
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

    /// The fill pass of a run in the paint now.
    #[allow(clippy::too_many_arguments)]
    fn fill_glyphs(
        &mut self,
        e: &FontEntry,
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
        let alpha = self.t.cur.alpha * a_mul;
        match fill {
            PaintVal::Linear(i) | PaintVal::Radial(i) => {
                let radial = matches!(fill, PaintVal::Radial(_));
                let (blk, g) = self.glyph_block(e, size, x, y, text, kern, spacing, 7);
                let bounds = [x, y - size, x + width, y + size * 0.3];
                if self.fill_gradient(radial, *i, &blk, bounds, a_mul) {
                    self.w.report.glyphs += g;
                }
            }
            PaintVal::Spec(g) => {
                let (blk, gl) = self.glyph_block(e, size, x, y, text, kern, spacing, 7);
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
                let (blk, g) = self.glyph_block(e, size, x, y, text, kern, spacing, 0);
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
                let (blk, g) = self.glyph_block(e, size, x, y, text, kern, spacing, 0);
                self.w.report.glyphs += g;
                self.sync_gs(a, None);
                self.sync_fill_color(c);
                self.put(&blk);
            }
        }
    }
}
