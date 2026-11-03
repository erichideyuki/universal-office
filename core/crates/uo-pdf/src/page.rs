//! `PdfPage` of `pdf-writer/document.ts`: the drawing operations of one page.
//!
//! Input coordinates have the origin at the TOP left of the page (y grows downwards, as in the
//! canvas of the apps); the conversion to the PDF system (origin at the bottom left) is made here.
//!
//! **Limits (record PQ11).** Every drawing call counts as one operation (`glyphs` counts its
//! glyphs) and the content stream is capped in bytes. A page that goes over stops drawing: later
//! calls write nothing, [`PageWriter::failure`] says why (a painter loop that may run for millions
//! of iterations polls it), and [`PageWriter::finish`] and `build` return the error.

use std::f64::consts::PI;

use uo_jsnum::math;

use crate::color::PdfColor;
use crate::document::{LinkRec, PageRec, PdfDocument};
use crate::error::{LimitKind, PdfError, Result};
use crate::font::Encoded;
use crate::num::{F, FList, J, Rgb, put};
use crate::sink::PdfSink;
use crate::types::{FontId, GlyphOptions, PathCmd, PdfImage, PdfLink, PdfShading, PlacedGlyph, Stroke};

/// One page being drawn. It borrows the document: resources the page needs while it is painted
/// (images, shadings, fonts) are created through [`PageWriter::document`], which keeps the object
/// numbers in paint order as the TS does.
pub struct PageWriter<'d, S: PdfSink> {
    doc: &'d mut PdfDocument<S>,
    width_pt: f64,
    height_pt: f64,
    content: Vec<u8>,
    links: Vec<LinkRec>,
    ops: u64,
    failed: Option<PdfError>,
    done: bool,
}

impl<'d, S: PdfSink> PageWriter<'d, S> {
    pub(crate) fn new(doc: &'d mut PdfDocument<S>, width_pt: f64, height_pt: f64) -> Self {
        PageWriter { doc, width_pt, height_pt, content: Vec::new(), links: Vec::new(), ops: 0, failed: None, done: false }
    }

    pub fn width_pt(&self) -> f64 {
        self.width_pt
    }

    pub fn height_pt(&self) -> f64 {
        self.height_pt
    }

    /// The document, to create the resources this page uses (embed an image, add a shading).
    pub fn document(&mut self) -> &mut PdfDocument<S> {
        self.doc
    }

    /// The limit this page went over, if any (it draws nothing after that).
    pub fn failure(&self) -> Option<PdfError> {
        self.failed
    }

    /// Operations counted so far.
    pub fn operations(&self) -> u64 {
        self.ops
    }

    /// Bytes of content written so far.
    pub fn content_len(&self) -> usize {
        self.content.len()
    }

    /// Counts `weight` operations; false when the page has failed or this goes over the limit.
    fn begin(&mut self, weight: u64) -> bool {
        if self.failed.is_some() {
            return false;
        }
        self.ops = self.ops.saturating_add(weight);
        if self.ops > self.doc.limits.max_ops_per_page {
            self.failed = Some(PdfError::Limit(LimitKind::Ops));
            return false;
        }
        true
    }

    /// Checks the size of the content after an operation.
    fn end(&mut self) {
        if self.content.len() as u64 > self.doc.limits.max_content_bytes && self.failed.is_none() {
            self.failed = Some(PdfError::Limit(LimitKind::Content));
        }
    }

    fn flip_y(&self, y_top: f64) -> f64 {
        self.height_pt - y_top
    }

    /// A clickable area (external link, or a destination in a page). An address that is not
    /// http, https or mailto is ignored.
    pub fn add_link(&mut self, link: &PdfLink) {
        let Some(rec) = LinkRec::from_link(link) else { return };
        if self.begin(1) {
            self.links.push(rec);
        }
    }

    /// Draws with fill and stroke opacity 0..1 (`None` is 1): `q /GSn gs ... Q` around what `draw`
    /// paints; opaque drawing goes straight through.
    pub fn with_alpha(&mut self, fill_alpha: Option<f64>, stroke_alpha: Option<f64>, draw: impl FnOnce(&mut Self)) {
        let fa = fill_alpha.unwrap_or(1.0);
        let sa = stroke_alpha.unwrap_or(1.0);
        if fa >= 1.0 && sa >= 1.0 {
            draw(self);
            return;
        }
        let index = self.doc.alpha_state(math::max(0.0, fa), math::max(0.0, sa));
        if self.begin(1) {
            put!(self.content, "q\n/GS{index} gs\n");
        }
        draw(self);
        if self.begin(0) {
            self.content.extend_from_slice(b"Q\n");
        }
        self.end();
    }

    /// A filled rectangle (shading, highlight); `y_top_pt` is the top of the rectangle.
    pub fn fill_rect(&mut self, x_pt: f64, y_top_pt: f64, w_pt: f64, h_pt: f64, color: &PdfColor) {
        if !self.begin(1) {
            return;
        }
        let y = self.flip_y(y_top_pt + h_pt);
        put!(self.content, "{} rg\n{} {} {} {} re f\n", Rgb(color), F(x_pt), F(y), F(w_pt), F(h_pt));
        self.end();
    }

    /// A line segment (borders, underline, dividers); y from the top.
    #[allow(clippy::too_many_arguments)]
    pub fn line(&mut self, x0: f64, y0_top: f64, x1: f64, y1_top: f64, color: &PdfColor, width_pt: f64, dash: &[f64]) {
        if !self.begin(1) {
            return;
        }
        put!(self.content, "{} RG\n{} w\n", Rgb(color), F(width_pt));
        self.put_dash(dash);
        let (ya, yb) = (self.flip_y(y0_top), self.flip_y(y1_top));
        put!(self.content, "{} {} m {} {} l S\n", F(x0), F(ya), F(x1), F(yb));
        self.end();
    }

    /// `[a b] 0 d\n`, or `[] 0 d\n` with no dash.
    fn put_dash(&mut self, dash: &[f64]) {
        if dash.is_empty() {
            self.content.extend_from_slice(b"[] 0 d\n");
        } else {
            put!(self.content, "[{}] 0 d\n", FList(dash));
        }
    }

    /// Encodes a code point with the font (recording the use for `/W` and ToUnicode).
    pub fn encode(&mut self, font: FontId, code_point: u32) -> Option<Encoded> {
        Some(self.doc.font_mut(font)?.encode(code_point))
    }

    /// Glyphs already placed, all with the same font, size and colour. Each run gets its own text
    /// matrix, reproducing the positions the layout computed (the 1/600" snap included).
    ///
    /// With `opts.runs`, as in the Word's PDF: ONE matrix (`Tm`) at the first glyph of each stretch
    /// of the same baseline and increasing x, and a `TJ` with INTEGER adjustments (thousandths of
    /// the size) that bring each glyph to the layout position. Each adjustment starts from the real
    /// position of the pen (the widths of `/W` plus the adjustments already written), so rounding
    /// does not accumulate.
    pub fn glyphs(&mut self, font: FontId, size_pt: f64, color: &PdfColor, glyphs: &[PlacedGlyph], opts: &GlyphOptions) {
        if glyphs.is_empty() || !self.begin(glyphs.len() as u64) {
            return;
        }
        let Some(font) = self.doc.font(font) else { return };
        let height = self.height_pt;
        let c = &mut self.content;
        let col = Rgb(color);
        put!(*c, "BT\n/F{} {} Tf\n{col} rg\n", font.index(), F(size_pt));
        // synthetic bold: fill + stroke (mode 2) with a line proportional to the size
        if opts.synth_bold {
            put!(*c, "{col} RG {} w 2 Tr\n", F(size_pt / 35.0));
        }
        let sx = opts.scale_x.unwrap_or(1.0);
        let a = F(sx);
        let skew = if opts.synth_italic { "0.21" } else { "0" };
        if opts.runs {
            let mut i = 0;
            while let Some(g0) = glyphs.get(i) {
                let mut j = i + 1;
                while let (Some(g), Some(prev)) = (glyphs.get(j), glyphs.get(j - 1)) {
                    if g.baseline_from_top_pt == g0.baseline_from_top_pt && g.x_pt >= prev.x_pt {
                        j += 1;
                    } else {
                        break;
                    }
                }
                put!(*c, "{a} 0 {skew} 1 {} {} Tm [", F(g0.x_pt), F(height - g0.baseline_from_top_pt));
                // pen: the offset (pt, text space) from the first glyph of the run
                let mut pen = 0.0f64;
                for k in i..j {
                    let Some(g) = glyphs.get(k) else { break };
                    if k > i {
                        let n = math::round(((pen - (g.x_pt - g0.x_pt) / sx) * 1000.0) / size_pt);
                        if n != 0.0 {
                            put!(*c, "{}", J(n));
                            pen -= (n * size_pt) / 1000.0;
                        }
                    }
                    put!(*c, "<{:04x}>", g.gid);
                    pen += (font.width_thousandths(g.gid) * size_pt) / 1000.0;
                }
                c.extend_from_slice(b"] TJ\n");
                i = j;
            }
        } else {
            for g in glyphs {
                let y = height - g.baseline_from_top_pt;
                put!(*c, "{a} 0 {skew} 1 {} {} Tm <{:04x}> Tj\n", F(g.x_pt), F(y), g.gid);
            }
        }
        if opts.synth_bold {
            c.extend_from_slice(b"0 Tr\n");
        }
        c.extend_from_slice(b"ET\n");
        self.end();
    }

    /// The commands of a path in PDF coordinates.
    fn put_path(&mut self, cmds: &[PathCmd]) {
        let h = self.height_pt;
        let c = &mut self.content;
        for cmd in cmds {
            match *cmd {
                PathCmd::M { x, y } => put!(*c, "{} {} m\n", F(x), F(h - y)),
                PathCmd::L { x, y } => put!(*c, "{} {} l\n", F(x), F(h - y)),
                PathCmd::C { x1, y1, x2, y2, x, y } => {
                    put!(*c, "{} {} {} {} {} {} c\n", F(x1), F(h - y1), F(x2), F(h - y2), F(x), F(h - y));
                }
                PathCmd::Z => c.extend_from_slice(b"h\n"),
            }
        }
    }

    /// A path filled and/or stroked; `even_odd` is the even-odd fill rule (the holes of a:custGeom).
    pub fn path(&mut self, cmds: &[PathCmd], fill: Option<&PdfColor>, stroke: Option<&Stroke<'_>>, even_odd: bool) {
        if cmds.is_empty() || (fill.is_none() && stroke.is_none()) || !self.begin(1) {
            return;
        }
        if let Some(fill) = fill {
            put!(self.content, "{} rg\n", Rgb(fill));
        }
        if let Some(stroke) = stroke {
            put!(self.content, "{} RG {} w ", Rgb(&stroke.color), F(stroke.width_pt));
            self.put_dash(stroke.dash);
        }
        self.put_path(cmds);
        let paint: &[u8] = match (fill.is_some(), stroke.is_some(), even_odd) {
            (true, true, true) => b"B*\n",
            (true, true, false) => b"B\n",
            (true, false, true) => b"f*\n",
            (true, false, false) => b"f\n",
            _ => b"S\n",
        };
        self.content.extend_from_slice(paint);
        self.end();
    }

    /// Fills the path with a shading (clips to the path and paints with `sh`).
    pub fn fill_shaded(&mut self, cmds: &[PathCmd], shading: PdfShading, even_odd: bool) {
        if cmds.is_empty() || !self.begin(1) {
            return;
        }
        self.content.extend_from_slice(b"q\n");
        self.put_path(cmds);
        let rule = if even_odd { "W*" } else { "W" };
        put!(self.content, "{rule} n\n/Sh{} sh\nQ\n", shading.index);
        self.end();
    }

    /// Pushes a rectangular clip (until [`PageWriter::pop_clip`]).
    pub fn push_clip_rect(&mut self, x_pt: f64, y_top_pt: f64, w_pt: f64, h_pt: f64) {
        if !self.begin(1) {
            return;
        }
        let y = self.flip_y(y_top_pt + h_pt);
        put!(self.content, "q\n{} {} {} {} re W n\n", F(x_pt), F(y), F(w_pt), F(h_pt));
        self.end();
    }

    /// Ends a clip or a transform pushed before (`Q`).
    pub fn pop_clip(&mut self) {
        if self.begin(1) {
            self.content.extend_from_slice(b"Q\n");
            self.end();
        }
    }

    /// Pushes a clip by path (until [`PageWriter::pop_clip`]); `even_odd` is the even-odd rule.
    pub fn push_clip_path(&mut self, cmds: &[PathCmd], even_odd: bool) {
        if !self.begin(1) {
            return;
        }
        self.content.extend_from_slice(b"q\n");
        self.put_path(cmds);
        self.content.extend_from_slice(if even_odd { b"W* n\n" } else { b"W n\n" });
        self.end();
    }

    /// An image by the matrix (a b c d e f) that takes the unit square to the page (y from the
    /// TOP; (0, 0) is the top left corner of the image).
    #[allow(clippy::many_single_char_names, clippy::too_many_arguments)]
    pub fn draw_image_matrix(&mut self, img: PdfImage, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        if !self.begin(1) {
            return;
        }
        let h = self.height_pt;
        put!(self.content, "q\n{} {} {} {} {} {} cm\n/Im{} Do\nQ\n", F(a), F(-b), F(-c), F(d), F(c + e), F(h - d - f), img.index);
        self.end();
    }

    /// Pushes an affine transform (matrix `cm`, in points) until [`PageWriter::pop_clip`].
    #[allow(clippy::many_single_char_names, clippy::too_many_arguments)]
    pub fn push_transform(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        if !self.begin(1) {
            return;
        }
        put!(self.content, "q\n{} {} {} {} {} {} cm\n", F(a), F(b), F(c), F(d), F(e), F(f));
        self.end();
    }

    /// An image with rotation (degrees, clockwise, about the centre) and flips.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_transformed(
        &mut self,
        img: PdfImage,
        x_pt: f64,
        y_top_pt: f64,
        w_pt: f64,
        h_pt: f64,
        rot_deg: f64,
        flip_h: bool,
        flip_v: bool,
    ) {
        if !self.begin(1) {
            return;
        }
        let cx = x_pt + w_pt / 2.0;
        let cy = self.flip_y(y_top_pt + h_pt / 2.0);
        let r = (-rot_deg * PI) / 180.0;
        let (cos, sin) = (math::cos(r), math::sin(r));
        let sx = if flip_h { -w_pt } else { w_pt };
        let sy = if flip_v { -h_pt } else { h_pt };
        // M = T(centre) · R · S · T(-1/2, -1/2)
        let (a, b, c, d) = (cos * sx, sin * sx, -sin * sy, cos * sy);
        let (e, f) = (cx - (a + c) / 2.0, cy - (b + d) / 2.0);
        put!(self.content, "q\n{} {} {} {} {} {} cm\n/Im{} Do\nQ\n", F(a), F(b), F(c), F(d), F(e), F(f), img.index);
        self.end();
    }

    /// Draws an embedded image; (x, y) is the top left corner.
    pub fn draw_image(&mut self, img: PdfImage, x_pt: f64, y_top_pt: f64, w_pt: f64, h_pt: f64) {
        if !self.begin(1) {
            return;
        }
        let y = self.flip_y(y_top_pt + h_pt);
        put!(self.content, "q\n{} 0 0 {} {} {} cm\n/Im{} Do\nQ\n", F(w_pt), F(h_pt), F(x_pt), F(y), img.index);
        self.end();
    }

    /// Ends the page: its content is compressed now (when that makes it smaller) and held with the
    /// document until `build`. Returns the limit the page went over, if any.
    pub fn finish(mut self) -> Result<()> {
        self.finish_inner()
    }

    fn finish_inner(&mut self) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        if let Some(e) = self.failed {
            self.doc.failed.get_or_insert(e);
            return Err(e);
        }
        let content = std::mem::take(&mut self.content);
        match self.doc.w.compress_body(content, true) {
            Ok(body) => {
                let links = std::mem::take(&mut self.links);
                self.doc.pages.push(PageRec { width_pt: self.width_pt, height_pt: self.height_pt, content: body, links });
                Ok(())
            }
            Err(e) => {
                self.doc.failed.get_or_insert(e);
                Err(e)
            }
        }
    }
}

/// A page dropped without `finish` is finished as it is (an error is kept for `build`).
impl<S: PdfSink> Drop for PageWriter<'_, S> {
    fn drop(&mut self) {
        let _ = self.finish_inner();
    }
}
