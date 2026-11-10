//! `pdf.ts` (`renderPdf` and everything it paints with): the display-list PDF painter. One call
//! makes a PDF out of `uo_render` pages, their [`Resources`] and the optional [`Navigation`]; the
//! writer of frontier W does the file.
//!
//! The painter is a 1:1 port, inside these shapes:
//!
//! - the caches the TS keeps in closures (`fonts` by `face.key` + `#pua`, `images` by the identity of
//!   the `InlineImage` = [`ImageId`], `adjustedFor` by image and the JSON of the look, `bgImages` by
//!   the JSON key of the page-fill tile) live in [`Painter`] for the one call (record §2: nothing
//!   crosses calls);
//! - the closures that paint "inside" an alpha scope become closures over the page
//!   (`PageWriter::with_alpha`);
//! - what the TS does with a JS number that is a count (raster sizes, loop bounds) goes through
//!   `uo_jsnum::conv`;
//! - a failure of the image decoders leaves the image out and the PDF goes on (the TS `catch`,
//!   PQ11); a limit of the writer or the sink stops the render, and a raised cancellation flag too
//!   (PQ9).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashMap;
use uo_drawingml::{apply_image_effects, apply_picture_adjust, effect_decode};
use uo_fonts::{Face, OutlineCmd};
use uo_image::{ImageLimits, JpegInfo, Rgba, decode_raster, image_pixel_size, jpeg_info};
use uo_jsnum::conv::{js_index, truthy};
use uo_jsnum::json::{push_number, push_string_utf16};
use uo_jsnum::math;
use uo_jsnum::text::is_js_space_unit;
use uo_render::paint::fill::{
    GradBox, blip_tiles, gradient_geometry, gradient_samples, is_rect_gradient, pattern_raster, rect_gradient_raster,
};
use uo_render::paint::page_fill::{gradient_tile_rgba, page_fill_tile_tw, pattern_rgba};
use uo_render::paint::picture::AdjustParts;
use uo_render::paint::text_fx::{RunView, dash_pattern, fx_layers, grad_of, run_box};
use uo_render::paint::{GradGeom, PaintLimits};
use uo_render::types::{
    ImageItem, Item, Ltrb, PageFill, PageFillKind, PathCmd as RPath, ResolvedFill, ResolvedTextFx, ShapeItem, Stroke as RStroke,
};
use uo_render::{FaceId, ImageId, ItemList, NO_GID, Navigation, OptF64, Page, Resources, Sym, TextFxId};
use uo_render::{LinkTarget, geom};

use crate::color::{PdfColor, hex_color_units, rgb255};
use crate::document::PdfDocument;
use crate::error::{PdfError, PdfLimits, Result};
use crate::font::FontProgram;
use crate::page::PageWriter;
use crate::sink::PdfSink;
use crate::types::{
    FontId, GlyphOptions, JpegImage, PathCmd, PdfImage, PdfLink, PdfOutlineItem, PdfShading, PlacedGlyph, ShadingGeom, SoftMask, Stroke,
};
use crate::word_spaces::{WordSpaceCursor, with_word_spaces};

/// Twips per point.
pub(crate) const T: f64 = 20.0;

/// The resolution of the text-effects layer in the PDF (the Word writes about 200 dpi).
const FX_PPT: f64 = 200.0 / 72.0;

/// How much the Word lowers the resolution of a blurred text effect in the PDF: the layer goes at
/// `200 / k` dpi. Measured in Word 365 (2026-11-10, one run per radius, `ExportAsFixedFormat`):
/// glow up to 19 pt at 200 dpi, 20–40 pt at 100, 41–60 pt at 66.7, 61–80 pt at 50, 81 pt at 40,
/// 100 pt at 40, 150 pt at 25; shadow blur 2 pt at 200, 10–20 pt at 100, 21–30 pt at 66.7, 40 pt
/// at 50, 41 pt at 40, 60 pt at 33.3, 61 pt at 28.6, 80 pt at 25. So `k = 1` under `step`, else
/// `max(2, ceil(x / step))` with `step` = 20 pt of glow radius and 10 pt of shadow blur (the same
/// Gaussian sigma, 10/3 pt, in both). A larger blur has no fine detail: the image looks the same
/// and the work falls with k³ (k² pixels, a kernel k times shorter). Not in the frozen TS (which
/// always writes 200 dpi): the PDF of a run with such an effect differs from it on purpose.
#[must_use]
pub(crate) fn fx_downscale(x_pt: f64, step_pt: f64) -> f64 {
    // NaN, infinite and the small ones stay at 200 dpi (as before); the EMU of the file can land a
    // hair off a multiple
    if !x_pt.is_finite() || x_pt < step_pt - 1e-9 {
        return 1.0;
    }
    math::max(2.0, math::ceil(x_pt / step_pt - 1e-9))
}

/// `fx_downscale` of the glow (radius, step 20 pt) and of the outer shadow (blur, step 10 pt).
fn fx_downscales(fx: &ResolvedTextFx) -> (f64, f64) {
    (fx.glow.as_ref().map_or(1.0, |g| fx_downscale(g.rad_pt, 20.0)), fx.shadow.as_ref().map_or(1.0, |s| fx_downscale(s.blur_pt, 10.0)))
}

/// What a render takes besides the pages.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// compress the content streams (default true)
    pub compress: bool,
    /// the limits of the writer (PQ11)
    pub limits: PdfLimits,
    /// the limits of the raster and fill helpers (patterns, gradients, tiles, text-effect layers
    /// and the masks of the picture effects)
    pub paint: PaintLimits,
    /// the limits of the image decoders
    pub image: ImageLimits,
    /// a flag the caller raises to stop the render (PQ9): checked before each page, every 256
    /// items and before each raster effect (text, shape and picture effects); the render then returns [`PdfError::Cancelled`]
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions { compress: true, limits: PdfLimits::DEFAULT, paint: PaintLimits::DEFAULT, image: ImageLimits::DEFAULT, cancel: None }
    }
}

/// `renderPdf(result, { compress, navigation })`: the PDF of the pages.
///
/// # Errors
/// A limit of [`PdfLimits`] (or of the sink), the cancellation flag, or an object the writer cannot
/// build. Undecodable or oversized images are left out of the file, as in the TS.
pub fn render_pdf(pages: &[Page], res: &Resources, nav: Option<&Navigation>, o: &PdfOptions) -> Result<Vec<u8>> {
    render_pdf_to(pages, res, nav, o, Vec::new())
}

/// [`render_pdf`] to a sink: the bytes leave as the objects become writable, so with a chunked
/// sink the memory of the render does not grow with the file.
///
/// # Errors
/// As [`render_pdf`].
pub fn render_pdf_to<S: PdfSink>(pages: &[Page], res: &Resources, nav: Option<&Navigation>, o: &PdfOptions, sink: S) -> Result<S> {
    let mut doc = PdfDocument::with_limits(sink, o.compress, o.limits);
    let mut painter = Painter::new(res, o);
    let links = nav.map(|n| links_by_page(n, pages.len())).unwrap_or_default();
    for (page_index, lp) in pages.iter().enumerate() {
        painter.check_cancel()?;
        let mut page = doc.add_page(lp.w / T, lp.h / T);
        for &li in links.get(page_index).map(Vec::as_slice).unwrap_or_default() {
            if let Some(l) = nav.and_then(|n| n.links.get(li)) {
                page.add_link(&pdf_link(&l.link));
            }
        }
        if let Some(bg) = lp.background.map(|s| res.sym(s)).filter(|b| !b.is_empty()) {
            page.fill_rect(0.0, 0.0, lp.w / T, lp.h / T, &hex_color_units(bg));
        }
        if let Some(f) = lp.background_fill.and_then(|id| res.page_fill(id)) {
            painter.paint_fill(&mut page, lp, f);
        }
        let mut spaces = WordSpaceCursor::default();
        for (n, it) in lp.list.items.iter().enumerate() {
            if n % 256 == 255 {
                painter.check_cancel()?;
            }
            painter.paint_item(&mut page, &lp.list, it, Some(&mut spaces));
            if page.failure().is_some() || painter.fatal.is_some() {
                break;
            }
        }
        page.finish()?;
        if let Some(e) = painter.fatal {
            return Err(e);
        }
    }
    if let Some(n) = nav {
        for e in &n.outline {
            doc.add_outline(PdfOutlineItem {
                title: String::from_utf16_lossy(&e.title),
                level: e.level,
                page_index: i64::from(e.page_index),
                top_pt: e.top_pt,
            });
        }
    }
    doc.build()
}

/// The links of each page, in the order of the navigation (`for (l of links) if (l.page === i)`),
/// without visiting every link once per page.
fn links_by_page(nav: &Navigation, pages: usize) -> Vec<Vec<usize>> {
    let mut by_page: Vec<Vec<usize>> = Vec::new();
    by_page.resize_with(pages, Vec::new);
    for (i, l) in nav.links.iter().enumerate() {
        if let Some(v) = usize::try_from(l.page).ok().and_then(|p| by_page.get_mut(p)) {
            v.push(i);
        }
    }
    by_page
}

fn pdf_link(l: &uo_render::PdfLink) -> PdfLink {
    let base = PdfLink { x_pt: l.x_pt, y_top_pt: l.y_top_pt, w_pt: l.w_pt, h_pt: l.h_pt, ..PdfLink::default() };
    match &l.target {
        LinkTarget::Uri(u) => PdfLink { uri: Some(String::from_utf16_lossy(u)), ..base },
        LinkTarget::Dest { page_index, dest_top_pt } => {
            PdfLink { page_index: Some(i64::from(*page_index)), dest_top_pt: Some(*dest_top_pt), ..base }
        }
    }
}

// ---- small shared helpers -----------------------------------------------------------------

/// `/^[0-9A-Fa-f]{6}$/`
pub(crate) fn is_hex6(u: &[u16]) -> bool {
    u.len() == 6 && u.iter().all(|&c| matches!(c, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66))
}

/// A `Float32Array` element store (`Math.fround`).
#[allow(clippy::cast_possible_truncation, reason = "the rounding to f32 is the Float32Array store")]
pub(crate) fn f32_store(x: f64) -> f32 {
    x as f32
}

/// The writer's path from a display-list path in twips (`c / T`, the `path` item).
fn path_points(cmds: &[RPath]) -> Vec<PathCmd> {
    cmds.iter()
        .map(|c| match *c {
            RPath::Z => PathCmd::Z,
            RPath::C { x1, y1, x2, y2, x, y } => PathCmd::C { x1: x1 / T, y1: y1 / T, x2: x2 / T, y2: y2 / T, x: x / T, y: y / T },
            RPath::M { x, y } => PathCmd::M { x: x / T, y: y / T },
            RPath::L { x, y } => PathCmd::L { x: x / T, y: y / T },
        })
        .collect()
}

/// The writer's path from a geometry path already in points.
pub(crate) fn pt_path(cmds: &[RPath]) -> Vec<PathCmd> {
    cmds.iter()
        .map(|c| match *c {
            RPath::Z => PathCmd::Z,
            RPath::C { x1, y1, x2, y2, x, y } => PathCmd::C { x1, y1, x2, y2, x, y },
            RPath::M { x, y } => PathCmd::M { x, y },
            RPath::L { x, y } => PathCmd::L { x, y },
        })
        .collect()
}

/// A stroke ready for the writer: colour, width and the dash it owns.
struct StrokeSpec {
    color: PdfColor,
    width_pt: f64,
    dash: Vec<f64>,
}

impl StrokeSpec {
    fn as_pdf(&self) -> Stroke<'_> {
        Stroke { color: self.color, width_pt: self.width_pt, dash: &self.dash }
    }

    fn with_color(&self, color: PdfColor) -> StrokeSpec {
        StrokeSpec { color, width_pt: self.width_pt, dash: self.dash.clone() }
    }
}

/// `{ color: hexColor(c), widthPt: Math.max(0.25, widthPt), dash: dash ? [w*3, w*2] : none }`
fn stroke_of(res: &Resources, s: &RStroke, require_hex6: bool) -> Option<StrokeSpec> {
    let c = res.sym(s.color);
    if require_hex6 && !is_hex6(c) {
        return None;
    }
    let dash = if s.dash.is_some_and(|d| !res.sym(d).is_empty()) { vec![s.width_pt * 3.0, s.width_pt * 2.0] } else { Vec::new() };
    Some(StrokeSpec { color: hex_color_units(c), width_pt: math::max(0.25, s.width_pt), dash })
}

/// A glyph run as the painter reads it: the item with its columns, and the overrides of
/// `drawPlain` (the TS spreads the item with `fx` removed and a new colour/alpha).
#[derive(Clone, Copy)]
pub(crate) struct Run<'a> {
    pub face: FaceId,
    pub color: Sym,
    pub size_pt: f64,
    pub y: f64,
    pub cps: &'a [u32],
    pub xs: &'a [f64],
    /// empty, or as long as `cps` with [`NO_GID`] where absent
    pub gids: &'a [u32],
    pub scale_x: OptF64,
    pub alpha: OptF64,
    pub flags: uo_render::types::RunFlags,
    pub fx: Option<TextFxId>,
}

impl<'a> Run<'a> {
    fn of(list: &'a ItemList, it: &Item) -> Option<Run<'a>> {
        let Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } = it else { return None };
        let r = glyphs.range()?;
        let side = list.runs.get(usize::try_from(*run).ok()?).copied().unwrap_or_default();
        let cps = list.glyphs.cp.get(r.clone()).unwrap_or(&[]);
        let xs = list.glyphs.x.get(r.clone()).unwrap_or(&[]);
        let gids = if list.glyphs.gid.is_empty() { &[][..] } else { list.glyphs.gid.get(r).unwrap_or(&[]) };
        Some(Run {
            face: *face,
            color: *color,
            size_pt: *size_pt,
            y: *y,
            cps,
            xs,
            gids,
            scale_x: side.scale_x,
            alpha: side.alpha,
            flags: *flags,
            fx: side.fx,
        })
    }

    /// `g.gid` of glyph `i`.
    fn gid_at(&self, i: usize) -> Option<u32> {
        self.gids.get(i).copied().filter(|&g| g != NO_GID)
    }

    /// The glyphs as `(cp, x)`, `gid` or not.
    fn pairs(&self) -> impl Iterator<Item = (usize, u32, f64)> + '_ {
        self.cps.iter().zip(self.xs).enumerate().map(|(i, (&c, &x))| (i, c, x))
    }

    fn view<'f>(&'f self, face: &'f Face) -> RunView<'f> {
        RunView {
            face,
            size_pt: self.size_pt,
            y: self.y,
            scale_x: self.scale_x,
            synth_italic: self.flags.has(uo_render::types::RunFlags::SYNTH_ITALIC),
            cps: self.cps,
            xs: self.xs,
            gids: self.gids,
        }
    }
}

/// The font program the writer embeds: the face, with the symbol-font fallback of `fontFor` (an
/// F0xx code the font lacks in its cmap takes the glyph of the code minus 0xF000 in a symbol font).
struct SymbolFallback(Face);

impl FontProgram for SymbolFallback {
    fn units_per_em(&self) -> f64 {
        f64::from(self.0.metrics().units_per_em)
    }

    fn glyph_id(&self, code_point: u32) -> u32 {
        let g = self.0.glyph_id(code_point);
        if g == 0 && (0xf000..=0xf0ff).contains(&code_point) && self.0.is_symbol_font() { self.0.glyph_id(code_point - 0xf000) } else { g }
    }

    fn advance_width(&self, glyph_id: u32) -> f64 {
        f64::from(self.0.advance_width(glyph_id))
    }

    fn ascender(&self) -> Option<f64> {
        Some(f64::from(self.0.metrics().ascender))
    }

    fn descender(&self) -> Option<f64> {
        Some(f64::from(self.0.metrics().descender))
    }
}

/// The state of one render: the TS closures' caches.
pub(crate) struct Painter<'r> {
    pub(crate) res: &'r Resources,
    pub(crate) opts: &'r PdfOptions,
    /// `fonts`: by `face.key` (+ `#pua`); `None` = not embeddable
    fonts: FxHashMap<(Sym, bool), Option<FontId>>,
    /// `images`: by the identity of the image
    images: FxHashMap<ImageId, Option<PdfImage>>,
    /// `adjustedFor`: by image and the JSON of the look's pixel adjustments
    adjusted: FxHashMap<(ImageId, String), Option<PdfImage>>,
    /// `bgImages`: the page-fill tiles, by their key
    bg_images: FxHashMap<String, PdfImage>,
    /// a limit or sink failure met while a resource was made (the page loop stops)
    pub(crate) fatal: Option<PdfError>,
}

impl<'r> Painter<'r> {
    fn new(res: &'r Resources, opts: &'r PdfOptions) -> Self {
        Painter {
            res,
            opts,
            fonts: FxHashMap::default(),
            images: FxHashMap::default(),
            adjusted: FxHashMap::default(),
            bg_images: FxHashMap::default(),
            fatal: None,
        }
    }

    /// Checks the cancel flag before a raster effect (the unbudgeted cost of a page): on a raised
    /// flag the render is marked [`PdfError::Cancelled`] and the caller skips the effect; the page
    /// loop then stops after the item.
    pub(crate) fn stop_for_cancel(&mut self) -> bool {
        if self.check_cancel().is_err() {
            self.fatal.get_or_insert(PdfError::Cancelled);
            return true;
        }
        false
    }

    fn check_cancel(&mut self) -> Result<()> {
        if self.opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(PdfError::Cancelled);
        }
        Ok(())
    }

    /// The value of a writer call made while painting: a limit or sink error is kept for the page
    /// loop; an invalid resource (the TS `catch`) is just absent.
    pub(crate) fn note<V>(&mut self, r: Result<V>) -> Option<V> {
        match r {
            Ok(v) => Some(v),
            Err(PdfError::Invalid(_)) => None,
            Err(e) => {
                self.fatal.get_or_insert(e);
                None
            }
        }
    }

    /// `doc.embedRgba(data, w, h)`
    pub(crate) fn embed_rgba<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, rgba: &[u8], w: u32, h: u32) -> Option<PdfImage> {
        let r = page.document().embed_rgba(rgba, w, h);
        self.note(r)
    }

    /// `doc.addShading(g, samples, page.heightPt)`
    pub(crate) fn shade<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, g: GradGeom, samples: &[u8]) -> Option<PdfShading> {
        let geom = match g {
            GradGeom::Axial { x0, y0, x1, y1 } => ShadingGeom::Axial { x0, y0, x1, y1 },
            GradGeom::Radial { x0, y0, r } => ShadingGeom::Radial { x0, y0, r },
        };
        let h = page.height_pt();
        let r = page.document().add_shading(&geom, samples, h);
        self.note(r)
    }

    // ---- fonts --------------------------------------------------------------------------

    /// `fontFor(face, pua)`
    fn font_for<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, face: FaceId, pua: bool) -> Option<FontId> {
        let fr = self.res.face(face)?;
        let key = (fr.key, pua);
        if let Some(f) = self.fonts.get(&key) {
            return *f;
        }
        let made = match (&fr.bytes, &fr.face) {
            (Some(bytes), Some(sf)) => {
                let family: Vec<u16> = self.res.sym(fr.family).iter().copied().filter(|&u| !is_js_space_unit(u)).collect();
                let mut name = String::from_utf16_lossy(&family);
                if fr.bold {
                    name.push_str("-Bold");
                }
                if fr.italic {
                    name.push_str("-Italic");
                }
                Some(page.document().embed_font(&name, Arc::clone(bytes), SymbolFallback(sf.clone())))
            }
            _ => None,
        };
        self.fonts.insert(key, made);
        made
    }

    // ---- images -------------------------------------------------------------------------

    /// `imageFor(img)`
    pub(crate) fn image_for<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, id: ImageId) -> Option<PdfImage> {
        if let Some(h) = self.images.get(&id) {
            return *h;
        }
        let h = self.make_image(page, id);
        self.images.insert(id, h);
        h
    }

    fn make_image<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, id: ImageId) -> Option<PdfImage> {
        let ir = self.res.image(id)?;
        let data: &[u8] = &ir.bytes;
        let fx = ir.effects.as_ref();
        let jp = jpeg_info(data);
        // JPEG with a colour adjustment: brightness/contrast through /Decode; the others need the pixels
        let dec = match (&jp, fx) {
            (Some(j), Some(fx)) => effect_decode(fx, j.components),
            _ => None,
        };
        if let Some(j) = &jp
            && (fx.is_none() || dec.is_some())
        {
            return self.embed_jpeg(page, data, j, dec.as_deref().unwrap_or(&[]), None);
        }
        let r = if jp.is_some() { None } else { decode_raster(data, &self.opts.image).ok() };
        if let Some(mut r) = r {
            if let Some(fx) = fx {
                if self.stop_for_cancel() {
                    return None;
                }
                apply_image_effects(&mut r.data, fx);
            }
            return self.embed_rgba(page, &r.data, r.width, r.height);
        }
        match &jp {
            Some(j) => self.embed_jpeg(page, data, j, &[], None),
            None => None,
        }
    }

    fn embed_jpeg<S: PdfSink>(
        &mut self,
        page: &mut PageWriter<'_, S>,
        data: &[u8],
        j: &JpegInfo,
        decode: &[f64],
        smask: Option<SoftMask<'_>>,
    ) -> Option<PdfImage> {
        let img = JpegImage {
            bytes: data,
            width_px: j.width,
            height_px: j.height,
            components: j.components,
            adobe_inverted: j.adobe,
            decode,
            smask,
        };
        let r = page.document().embed_jpeg(&img);
        self.note(r)
    }

    // ---- the picture environment (pdf.ts `pictureEnv`) ------------------------------------

    /// `rgbaOf(it)`: the pixels with the colour adjustments, when they can be decoded.
    pub(crate) fn rgba_of(&self, it: &ImageItem) -> Option<Rgba> {
        let ir = self.res.image(it.image)?;
        let data: &[u8] = &ir.bytes;
        if jpeg_info(data).is_some() {
            return None;
        }
        let mut r = decode_raster(data, &self.opts.image).ok()?;
        if let Some(fx) = &ir.effects {
            apply_image_effects(&mut r.data, fx);
        }
        if let Some(pic) = &it.pic {
            apply_picture_adjust(&mut r.data, &AdjustParts::of(self.res, pic).spec());
        }
        Some(r)
    }

    /// `needPixels`
    fn need_pixels(it: &ImageItem) -> bool {
        it.pic.as_ref().is_some_and(|l| l.alpha.is_some() || l.duotone.is_some() || l.clr_change.is_some())
    }

    /// `env.image()`
    pub(crate) fn pic_image<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ImageItem) -> Option<PdfImage> {
        if !Self::need_pixels(it) {
            return self.image_for(page, it.image);
        }
        let key = (it.image, self.adjust_key(it));
        if let Some(h) = self.adjusted.get(&key) {
            return *h;
        }
        if self.stop_for_cancel() {
            return None;
        }
        let h = match self.rgba_of(it) {
            Some(r) => self.embed_rgba(page, &r.data, r.width, r.height),
            None => self.image_for(page, it.image),
        };
        self.adjusted.insert(key, h);
        h
    }

    /// `JSON.stringify([look?.alpha, look?.duotone, look?.clrChange])`
    fn adjust_key(&self, it: &ImageItem) -> String {
        let syms = &self.res.symbols;
        let mut k = String::from("[");
        let look = it.pic.as_deref();
        match look.and_then(|l| l.alpha.get()) {
            Some(a) => push_number(&mut k, a),
            None => k.push_str("null"),
        }
        k.push(',');
        match look.and_then(|l| l.duotone) {
            Some([a, b]) => {
                k.push('[');
                push_string_utf16(&mut k, syms.get(a));
                k.push(',');
                push_string_utf16(&mut k, syms.get(b));
                k.push(']');
            }
            None => k.push_str("null"),
        }
        k.push(',');
        match look.and_then(|l| l.clr_change) {
            Some(c) => {
                k.push_str("{\"from\":");
                push_string_utf16(&mut k, syms.get(c.from));
                k.push_str(",\"to\":");
                push_string_utf16(&mut k, syms.get(c.to));
                k.push_str(",\"toAlpha\":");
                push_number(&mut k, c.to_alpha);
                k.push('}');
            }
            None => k.push_str("null"),
        }
        k.push(']');
        k
    }

    /// `env.masked(alpha, mw, mh)`: the image with a soft mask that covers all of it.
    pub(crate) fn pic_masked<S: PdfSink>(
        &mut self,
        page: &mut PageWriter<'_, S>,
        it: &ImageItem,
        alpha: &[u8],
        mw: u32,
        mh: u32,
    ) -> Option<PdfImage> {
        let ir = self.res.image(it.image)?;
        let data: &[u8] = &ir.bytes;
        if let Some(j) = jpeg_info(data)
            && !Self::need_pixels(it)
        {
            let fx = ir.effects.as_ref();
            let dec = fx.and_then(|fx| effect_decode(fx, j.components));
            if fx.is_none() || dec.is_some() {
                return self.embed_jpeg(page, data, &j, dec.as_deref().unwrap_or(&[]), Some(SoftMask { alpha, width: mw, height: mh }));
            }
        }
        if self.stop_for_cancel() {
            return None;
        }
        let Some(mut r) = self.rgba_of(it) else { return self.image_for(page, it.image) };
        // the mask (at its resolution) multiplies the alpha of the image
        let (rw, rh) = (f64::from(r.width), f64::from(r.height));
        let (mwf, mhf) = (f64::from(mw), f64::from(mh));
        let width = usize::try_from(r.width).unwrap_or(0);
        for y in 0..r.height {
            let my = math::min(mhf - 1.0, math::floor(((f64::from(y) + 0.5) / rh) * mhf));
            for x in 0..r.width {
                let mx = math::min(mwf - 1.0, math::floor(((f64::from(x) + 0.5) / rw) * mwf));
                let k = (usize::try_from(y).unwrap_or(0) * width + usize::try_from(x).unwrap_or(0)) * 4 + 3;
                let a = alpha.get(js_index(my * mwf + mx, alpha.len()).unwrap_or(usize::MAX)).map_or(f64::NAN, |&v| f64::from(v));
                if let Some(px) = r.data.get_mut(k) {
                    *px = uo_jsnum::conv::to_uint8(math::round((f64::from(*px) * a) / 255.0));
                }
            }
        }
        self.embed_rgba(page, &r.data, r.width, r.height)
    }

    // ---- page fill -----------------------------------------------------------------------

    /// `bgImage(key, make)`
    fn bg_image<S: PdfSink>(
        &mut self,
        page: &mut PageWriter<'_, S>,
        key: String,
        make: impl FnOnce(&Self) -> Option<(Vec<u8>, u32, u32)>,
    ) -> Option<PdfImage> {
        if let Some(h) = self.bg_images.get(&key) {
            return Some(*h);
        }
        let (rgba, w, h) = make(self)?;
        let img = self.embed_rgba(page, &rgba, w, h)?;
        self.bg_images.insert(key, img);
        Some(img)
    }

    /// `paintFillPdf(page, lp, f, bgImage, imageFor)`: the page-fill effect as the Word writes it,
    /// tiles of a "device pixel" of 0.12 pt (gradient, picture) or 1 pt (pattern), the texture at
    /// its natural size, anchored at the top left corner.
    fn paint_fill<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, lp: &Page, f: &PageFill) {
        let Some((tile_w, tile_h)) = page_fill_tile_tw(f, 2.4, 20.0, true) else { return };
        let (mut tw, mut th) = (tile_w / T, tile_h / T);
        let res = self.res;
        let syms = &res.symbols;
        let img = match f.kind {
            PageFillKind::Gradient | PageFillKind::GradientRadial => {
                let pw = math::max(2.0, math::round((tw * 96.0) / 72.0));
                let ph = math::max(2.0, math::round((th * 96.0) / 72.0));
                let mut key = String::from("g|[");
                key.push_str(if f.kind == PageFillKind::Gradient { "\"gradient\"," } else { "\"gradientRadial\"," });
                push_string_utf16(&mut key, syms.get(f.c1));
                key.push(',');
                push_string_utf16(&mut key, syms.get(f.c2));
                key.push(',');
                match &f.stops {
                    Some(st) => {
                        key.push('[');
                        for (i, s) in st.iter().enumerate() {
                            if i > 0 {
                                key.push(',');
                            }
                            key.push_str("{\"pos\":");
                            push_number(&mut key, s.pos);
                            key.push_str(",\"color\":");
                            push_string_utf16(&mut key, syms.get(s.color));
                            key.push('}');
                        }
                        key.push(']');
                    }
                    None => key.push_str("null"),
                }
                key.push(',');
                push_number(&mut key, f.angle);
                key.push(',');
                push_number(&mut key, f.focus);
                key.push(',');
                match f.focus_position {
                    Some([a, b]) => {
                        key.push('[');
                        push_number(&mut key, a);
                        key.push(',');
                        push_number(&mut key, b);
                        key.push(']');
                    }
                    None => key.push_str("null"),
                }
                key.push(',');
                key.push_str(if f.gradient_center { "true" } else { "false" });
                key.push_str("]|");
                push_number(&mut key, pw);
                key.push('x');
                push_number(&mut key, ph);
                let (Some(pwu), Some(phu)) = (js_index(pw, 1 << 20), js_index(ph, 1 << 20)) else { return };
                self.bg_image(page, key, |p| {
                    let rgba = gradient_tile_rgba(f, &p.res.symbols, pwu, phu, true, &p.opts.paint).ok()?;
                    Some((rgba, u32::try_from(pwu).ok()?, u32::try_from(phu).ok()?))
                })
            }
            PageFillKind::Pattern => {
                // 8 repetitions in each direction per image (fewer draws on the page)
                let mut key = format!("p|{}|{}|", String::from_utf16_lossy(syms.get(f.c1)), String::from_utf16_lossy(syms.get(f.c2)));
                if let Some(p) = &f.pattern {
                    for (i, v) in p.iter().enumerate() {
                        if i > 0 {
                            key.push(',');
                        }
                        key.push_str(&uo_jsnum::format::to_js_string(*v));
                    }
                } else {
                    key.push_str("undefined");
                }
                let img = self.bg_image(page, key, |p| {
                    let base = pattern_rgba(f, &p.res.symbols);
                    let mut rgba = vec![0u8; 64 * 64 * 4];
                    for y in 0..64usize {
                        for x in 0..64usize {
                            let s = ((y % 8) * 8 + (x % 8)) * 4;
                            let d = (y * 64 + x) * 4;
                            if let (Some(src), Some(dst)) = (base.get(s..s + 4), rgba.get_mut(d..d + 4)) {
                                dst.copy_from_slice(src);
                            }
                        }
                    }
                    Some((rgba, 64, 64))
                });
                tw *= 8.0;
                th *= 8.0;
                img
            }
            PageFillKind::Frame | PageFillKind::Tile => f.image.and_then(|id| self.image_for(page, id)),
        };
        let Some(img) = img else { return };
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        let (w, h) = (lp.w / T, lp.h / T);
        page.push_clip_rect(0.0, 0.0, w, h);
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                page.draw_image(img, x, y, tw, th);
                if page.failure().is_some() {
                    break;
                }
                x += tw;
            }
            if page.failure().is_some() {
                break;
            }
            y += th;
        }
        page.pop_clip();
    }

    // ---- items ---------------------------------------------------------------------------

    /// `paintItem(page, it, …, spaces)`
    pub(crate) fn paint_item<S: PdfSink>(
        &mut self,
        page: &mut PageWriter<'_, S>,
        list: &ItemList,
        it: &Item,
        spaces: Option<&mut WordSpaceCursor>,
    ) {
        match it {
            Item::Glyphs { .. } => {
                if let Some(run) = Run::of(list, it) {
                    self.paint_run(page, &run, spaces);
                }
            }
            Item::Rect { x, y, w, h, fill, alpha } => {
                let color = hex_color_units(self.res.sym(*fill));
                page.with_alpha(alpha.get(), None, |p| p.fill_rect(x / T, y / T, w / T, h / T, &color));
            }
            Item::Line { x1, y1, x2, y2, color, width_pt, dash } => {
                let w = *width_pt;
                let dash: Vec<f64> = match dash {
                    Some(uo_render::types::LineDash::Dotted) => vec![w, w * 2.0],
                    Some(uo_render::types::LineDash::Dashed) => vec![w * 4.0, w * 3.0],
                    Some(uo_render::types::LineDash::DashDot) => vec![w * 4.0, w * 2.0, w, w * 2.0],
                    Some(uo_render::types::LineDash::DashDotDot) => vec![w * 4.0, w * 2.0, w, w * 2.0, w, w * 2.0],
                    Some(d) => d.border_dashes(w).unwrap_or_default(),
                    None => Vec::new(),
                };
                page.line(x1 / T, y1 / T, x2 / T, y2 / T, &hex_color_units(self.res.sym(*color)), w, &dash);
            }
            Item::Image(im) => self.paint_image(page, im),
            Item::Shape(sh) => self.paint_shape(page, sh),
            Item::Path { cmds, fill, stroke, even_odd: _ } => {
                let fill = fill.map(|f| self.res.sym(f)).filter(|f| is_hex6(f)).map(hex_color_units);
                let stroke = stroke.as_ref().and_then(|s| stroke_of(self.res, s, false));
                let cmds = path_points(list.cmds_of(*cmds));
                let st = stroke.as_ref().map(StrokeSpec::as_pdf);
                page.path(&cmds, fill.as_ref(), st.as_ref(), false);
            }
            Item::XformPush { m: Some(m) } => {
                // M in twips with the origin at the top -> the PDF `cm` (pt, origin at the bottom)
                let [a, b, c, d, e, f] = **m;
                let h = page.height_pt();
                page.push_transform(a, -b, -c, d, c * h + e / T, h - d * h - f / T);
            }
            Item::XformPush { m: None } | Item::XformPop | Item::ClipPop => page.pop_clip(),
            Item::ClipPush { x, y, w, h } => page.push_clip_rect(x.or(0.0) / T, y.or(0.0) / T, w.or(0.0) / T, h.or(0.0) / T),
        }
    }

    fn paint_image<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ImageItem) {
        let rot_t = it.rot.get().is_some_and(truthy);
        let crop_rot = it.crop.is_some() && (rot_t || it.flip_h || it.flip_v);
        // Picture Format (and a rotated crop): the composition in the frame of the image
        if it.pic.is_some() || crop_rot {
            // a mask/limit failure leaves the rest of this picture out (declared)
            let _ = self.paint_picture(page, it);
            return;
        }
        let Some(h) = self.image_for(page, it.image) else { return };
        let (mut x, mut y, mut w, mut hh) = (it.x / T, it.y / T, it.w / T, it.h / T);
        let rot = it.rot.or(0.0);
        match it.crop {
            Some(Ltrb { l, t, r, b }) if truthy(l) || truthy(t) || truthy(r) || truthy(b) => {
                page.push_clip_rect(x, y, w, hh);
                let fw = w / math::max(0.01, 1.0 - l - r);
                let fh = hh / math::max(0.01, 1.0 - t - b);
                x -= l * fw;
                y -= t * fh;
                w = fw;
                hh = fh;
                page.draw_image_transformed(h, x, y, w, hh, rot, it.flip_h, it.flip_v);
                page.pop_clip();
            }
            _ if rot_t || it.flip_h || it.flip_v => page.draw_image_transformed(h, x, y, w, hh, rot, it.flip_h, it.flip_v),
            _ => page.draw_image(h, x, y, w, hh),
        }
    }

    // ---- glyph runs ---------------------------------------------------------------------

    /// The `glyphs` case of `paintItem`.
    fn paint_run<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, run: &Run<'_>, spaces: Option<&mut WordSpaceCursor>) {
        // text effects (w14): layer behind, the text, the outline
        let res = self.res;
        if let Some(fx) = run.fx.and_then(|id| res.text_fx(id)) {
            self.paint_run_fx(page, run, fx);
            return;
        }
        if run.flags.has(uo_render::types::RunFlags::AS_PATH) && self.glyphs_as_path(page, run) {
            return;
        }
        let Some(ef) = self.font_for(page, run.face, false) else { return };
        // the size written in the PDF like the Word: a multiple of 1/600" (0.12 pt)
        let pdf_size = math::round(run.size_pt / 0.12) * 0.12;
        let Some(fr) = res.face(run.face) else { return };
        let Some(sf) = fr.face.as_ref() else { return };
        // a private-use code the font lacks in its cmap goes to the "pua" copy of the font
        let is_pua = |cp: u32| (0xf000..=0xf0ff).contains(&cp) && sf.glyph_id(cp) == 0;
        // a glyph by index (EMF) with no outline available has no character to write
        let by_char: Vec<(u32, f64)> = run.pairs().filter(|&(i, _, _)| run.gid_at(i).is_none()).map(|(_, c, x)| (c, x)).collect();
        let all = match spaces {
            Some(cur) => {
                let sx = if truthy(run.scale_x.or(0.0)) { run.scale_x.or(1.0) } else { 1.0 };
                with_word_spaces(&by_char, sf, fr.advances.as_deref(), run.size_pt, sx, run.y, cur)
            }
            None => by_char,
        };
        let color = hex_color_units(res.sym(run.color));
        let scale = run.scale_x.get().filter(|&s| truthy(s));
        let opts = GlyphOptions {
            scale_x: scale,
            synth_bold: run.flags.has(uo_render::types::RunFlags::SYNTH_BOLD),
            synth_italic: run.flags.has(uo_render::types::RunFlags::SYNTH_ITALIC),
            // text in runs (Tm + TJ) as in the Word's PDF: the extraction does not scramble overlapping lines
            runs: true,
        };
        let mut i = 0;
        while let Some(&(cp0, _)) = all.get(i) {
            let pua = is_pua(cp0);
            let mut j = i + 1;
            while all.get(j).is_some_and(|&(c, _)| is_pua(c) == pua) {
                j += 1;
            }
            let f = if pua { self.font_for(page, run.face, true).unwrap_or(ef) } else { ef };
            let placed: Vec<PlacedGlyph> = all
                .get(i..j)
                .unwrap_or_default()
                .iter()
                .map(|&(cp, x)| PlacedGlyph {
                    gid: page.encode(f, cp).map_or(0, |e| e.gid),
                    code_point: cp,
                    x_pt: x / T,
                    baseline_from_top_pt: run.y / T,
                })
                .collect();
            let a = run.alpha.get();
            page.with_alpha(a, a, |p| p.glyphs(f, pdf_size, &color, &placed, &opts));
            i = j;
            if page.failure().is_some() {
                return;
            }
        }
    }

    /// `outlineInto(cmds, font, gid, ox, base, sx, s, skew)`: a glyph outline as PDF path commands;
    /// false without outline.
    #[allow(clippy::too_many_arguments, reason = "the TS signature of outlineInto")]
    fn outline_into(cmds: &mut Vec<PathCmd>, font: &Face, gid: u32, ox: f64, base: f64, sx: f64, s: f64, skew: f64) -> bool {
        let Some(ol) = font.glyph_outline(gid) else { return false };
        let xf = |x: f64, y: f64| ox + x * sx + skew * y * s;
        let yf = |y: f64| base - y * s;
        let (mut px, mut py) = (0.0f64, 0.0f64);
        for c in ol.iter() {
            match *c {
                OutlineCmd::Z => cmds.push(PathCmd::Z),
                OutlineCmd::Q { cx, cy, x, y } => {
                    // quadratic -> cubic (control points at 2/3)
                    let c1x = px + (2.0 / 3.0) * (cx - px);
                    let c1y = py + (2.0 / 3.0) * (cy - py);
                    let c2x = x + (2.0 / 3.0) * (cx - x);
                    let c2y = y + (2.0 / 3.0) * (cy - y);
                    cmds.push(PathCmd::C { x1: xf(c1x, c1y), y1: yf(c1y), x2: xf(c2x, c2y), y2: yf(c2y), x: xf(x, y), y: yf(y) });
                    px = x;
                    py = y;
                }
                OutlineCmd::M { x, y } => {
                    cmds.push(PathCmd::M { x: xf(x, y), y: yf(y) });
                    px = x;
                    py = y;
                }
                OutlineCmd::L { x, y } => {
                    cmds.push(PathCmd::L { x: xf(x, y), y: yf(y) });
                    px = x;
                    py = y;
                }
            }
        }
        true
    }

    /// `runPathCmds(it)`: the outlines of the glyphs of the run in PDF commands (pt).
    fn run_path_cmds(&self, run: &Run<'_>) -> Option<Vec<PathCmd>> {
        let font = self.res.face(run.face)?.face.as_ref()?;
        let upm = f64::from(font.metrics().units_per_em);
        let sc = run.size_pt / upm;
        let sx = sc * run.scale_x.or(1.0);
        let skew = if run.flags.has(uo_render::types::RunFlags::SYNTH_ITALIC) { 0.2 } else { 0.0 };
        let mut cmds = Vec::new();
        for (i, cp, x) in run.pairs() {
            let gid = run.gid_at(i).unwrap_or_else(|| font.glyph_id(cp));
            Self::outline_into(&mut cmds, font, gid, x / T, run.y / T, sx, sc, skew);
        }
        Some(cmds)
    }

    /// `glyphsAsPath(page, it)`: text as CURVES (WordArt): each glyph its TrueType outline, filled;
    /// false if the font gives no outline (CFF) - then it goes as text.
    fn glyphs_as_path<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, run: &Run<'_>) -> bool {
        let res = self.res;
        let Some(font) = res.face(run.face).and_then(|f| f.face.as_ref()) else { return false };
        let upm = f64::from(font.metrics().units_per_em);
        let s = run.size_pt / upm;
        let sx = s * run.scale_x.or(1.0);
        let skew = if run.flags.has(uo_render::types::RunFlags::SYNTH_ITALIC) { 0.2 } else { 0.0 };
        let base = run.y / T;
        let mut cmds: Vec<PathCmd> = Vec::new();
        for (i, cp, gx) in run.pairs() {
            let gid = run.gid_at(i).unwrap_or_else(|| font.glyph_id(cp));
            // colour glyph (COLR): each layer with its colour, in the order of the table
            let layers = if run.flags.has(uo_render::types::RunFlags::COLR) { font.color_layers(gid) } else { None };
            if let Some(layers) = layers {
                for l in &layers {
                    let mut lc: Vec<PathCmd> = Vec::new();
                    if !Self::outline_into(&mut lc, font, u32::from(l.glyph_id), gx / T, base, sx, s, skew) || lc.is_empty() {
                        continue;
                    }
                    let a = l.alpha * run.alpha.or(1.0);
                    let color = match l.color {
                        Some([r, g, b]) => rgb255(f64::from(r), f64::from(g), f64::from(b)),
                        None => hex_color_units(res.sym(run.color)),
                    };
                    page.with_alpha(if a < 1.0 { Some(a) } else { None }, None, |p| p.path(&lc, Some(&color), None, false));
                }
                continue;
            }
            let Some(ol) = font.glyph_outline(gid) else { return false };
            let ox = gx / T;
            let xf = |x: f64, y: f64| ox + x * sx + skew * y * s;
            let yf = |y: f64| base - y * s;
            let (mut px, mut py) = (0.0f64, 0.0f64);
            for c in ol.iter() {
                match *c {
                    OutlineCmd::Z => cmds.push(PathCmd::Z),
                    OutlineCmd::Q { cx, cy, x, y } => {
                        // quadratic -> cubic (control points at 2/3)
                        let c1x = px + (2.0 / 3.0) * (cx - px);
                        let c1y = py + (2.0 / 3.0) * (cy - py);
                        let c2x = x + (2.0 / 3.0) * (cx - x);
                        let c2y = y + (2.0 / 3.0) * (cy - y);
                        cmds.push(PathCmd::C { x1: xf(c1x, c1y), y1: yf(c1y), x2: xf(c2x, c2y), y2: yf(c2y), x: xf(x, y), y: yf(y) });
                        px = x;
                        py = y;
                    }
                    OutlineCmd::M { x, y } => {
                        cmds.push(PathCmd::M { x: xf(x, y), y: yf(y) });
                        px = x;
                        py = y;
                    }
                    OutlineCmd::L { x, y } => {
                        cmds.push(PathCmd::L { x: xf(x, y), y: yf(y) });
                        px = x;
                        py = y;
                    }
                }
            }
            if page.failure().is_some() {
                return true;
            }
        }
        if cmds.is_empty() {
            return true;
        }
        let color = hex_color_units(res.sym(run.color));
        let synth_bold = run.flags.has(uo_render::types::RunFlags::SYNTH_BOLD);
        let stroke = if synth_bold { Some(StrokeSpec { color, width_pt: run.size_pt * 0.03, dash: Vec::new() }) } else { None };
        let st = stroke.as_ref().map(StrokeSpec::as_pdf);
        let a = run.alpha.get();
        page.with_alpha(a, a, |p| p.path(&cmds, Some(&color), st.as_ref(), false));
        true
    }

    /// One effect layer of the run at `ppt` pixels per point, drawn as an image with alpha (the
    /// cancel flag is checked before each layer: a blur is the long step).
    fn paint_fx_layer<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, rv: &RunView<'_>, fx: &ResolvedTextFx, ppt: f64, run: &Run<'_>) {
        if self.stop_for_cancel() {
            return;
        }
        let res = self.res;
        if let Ok(Some(l)) = fx_layers(rv, fx, &res.symbols, ppt, res.sym(run.color), &self.opts.paint)
            && let (Ok(w), Ok(h)) = (u32::try_from(l.w), u32::try_from(l.h))
            && let Some(img) = self.embed_rgba(page, &l.data, w, h)
        {
            page.draw_image(img, l.x0 / T, l.y0 / T, l.w as f64 / ppt, l.h as f64 / ppt);
        }
    }

    /// `paintGlyphsFxPdf`: shadow/glow/reflection in an image with alpha behind; the fill (solid =
    /// text; gradient = shaded curves; none = only the outline) and the outline in curves on top.
    fn paint_run_fx<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, run: &Run<'_>, fx: &ResolvedTextFx) {
        let res = self.res;
        let face = res.face(run.face).and_then(|f| f.face.as_ref());
        if let Some(face) = face
            && (fx.shadow.is_some() || fx.glow.is_some() || fx.reflection.is_some())
        {
            let rv = run.view(face);
            let (kg, ks) = fx_downscales(fx);
            if kg == 1.0 && ks == 1.0 {
                // the frozen TS: one layer of the three effects at 200 dpi
                self.paint_fx_layer(page, &rv, fx, FX_PPT, run);
            } else {
                // the Word: an image per effect, each at its own resolution, the reflection
                // (kept at 200 dpi), the shadow and then the glow (the order of the one layer)
                let only = |shadow: bool, glow: bool, reflection: bool| ResolvedTextFx {
                    fill: None,
                    outline: fx.outline.clone(),
                    shadow: if shadow { fx.shadow } else { None },
                    glow: if glow { fx.glow } else { None },
                    reflection: if reflection { fx.reflection } else { None },
                };
                if fx.reflection.is_some() {
                    self.paint_fx_layer(page, &rv, &only(false, false, true), FX_PPT, run);
                }
                if fx.shadow.is_some() {
                    self.paint_fx_layer(page, &rv, &only(true, false, false), FX_PPT / ks, run);
                }
                if fx.glow.is_some() {
                    self.paint_fx_layer(page, &rv, &only(false, true, false), FX_PPT / kg, run);
                }
            }
        }
        match &fx.fill {
            Some(ResolvedFill::Grad { stops, path, .. }) => {
                if let (Some(cmds), Some(face)) = (self.run_path_cmds(run), face)
                    && let Some(b) = run_box(&run.view(face))
                {
                    let bx = GradBox { x: b.x / T, y: b.y / T, w: b.w / T, h: b.h / T, rot: 0.0 };
                    let fill = fx.fill.as_ref();
                    let Some(g) = fill.and_then(grad_of) else { return };
                    let pb = (bx.x, bx.y, bx.x + bx.w, bx.y + bx.h);
                    let gg = gradient_geometry(&g, &res.symbols, bx, pb);
                    let st: Vec<(f64, &[u16])> = stops.iter().map(|s| (s.pos, res.sym(s.hex))).collect();
                    let n = if path.is_some_and(|p| !res.sym(p).is_empty()) { 256 } else { 512 };
                    let Ok(smp) = gradient_samples(&st, n, &self.opts.paint) else { return };
                    let Some(sh) = self.shade(page, gg, &smp) else { return };
                    let a0 = stops.first().map(|s| s.alpha).filter(|&a| a < 1.0);
                    page.with_alpha(a0, None, |p| p.fill_shaded(&cmds, sh, false));
                }
            }
            Some(ResolvedFill::None) => {}
            fill => {
                // drawPlain: the run without fx, with the colour of a solid fill and the alpha of the fill
                let mut plain = *run;
                plain.fx = None;
                let a = match fill {
                    Some(ResolvedFill::Solid { hex, alpha }) => {
                        plain.color = *hex;
                        Some(*alpha)
                    }
                    _ => run.alpha.get(),
                };
                if let Some(a) = a.filter(|&a| a < 1.0) {
                    plain.alpha = OptF64::some(a);
                    plain.flags = plain.flags.with(uo_render::types::RunFlags::AS_PATH, true);
                }
                self.paint_run(page, &plain, None);
            }
        }
        if let Some(o) = &fx.outline
            && !matches!(o.fill, ResolvedFill::None)
            && let Some(cmds) = self.run_path_cmds(run)
        {
            let (color, a) = match &o.fill {
                ResolvedFill::Solid { hex, alpha } => (*hex, *alpha),
                ResolvedFill::Grad { stops, .. } => (stops.first().map_or(run.color, |s| s.hex), 1.0),
                ResolvedFill::None => (run.color, 1.0),
            };
            let dash: Vec<f64> = dash_pattern(o.dash.map(|d| res.sym(d))).iter().map(|d| d * o.width_pt).collect();
            let stroke = Stroke { color: hex_color_units(res.sym(color)), width_pt: o.width_pt, dash: &dash };
            page.with_alpha(None, if a < 1.0 { Some(a) } else { None }, |p| p.path(&cmds, None, Some(&stroke), false));
        }
    }

    // ---- shapes ---------------------------------------------------------------------------

    fn paint_shape<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ShapeItem) {
        // Shape Effects: rasters of the effects around the vector body
        let geom_is_line = self.res.sym(it.geom.geom).iter().copied().eq("line".encode_utf16());
        if it.fx.is_some() && !geom_is_line {
            let _ = self.paint_shape_effects(page, it);
            return;
        }
        self.paint_shape_body(page, it);
    }

    /// The `shape` case of `paintItem` for an item without `fx`.
    pub(crate) fn paint_shape_body<S: PdfSink>(&mut self, page: &mut PageWriter<'_, S>, it: &ShapeItem) {
        let res = self.res;
        let st = &it.geom;
        let fill = it.fill.map(|f| res.sym(f)).filter(|f| is_hex6(f)).map(hex_color_units);
        let stroke = it.stroke.as_ref().and_then(|s| stroke_of(res, s, true));
        if fill.is_none() && stroke.is_none() && it.grad.is_none() && it.pattern.is_none() && it.blip.is_none() {
            return;
        }
        let (x, y, w, h) = (it.x / T, it.y / T, it.w / T, it.h / T);
        let parts = geom::shape_parts(geom::LineUnit::PdfPoints, st, res, [x, y, w, h], it.line);
        let rot = st.rot.or(0.0);
        let st_pdf = stroke.as_ref().map(StrokeSpec::as_pdf);
        // outer shadow: a shifted copy in the shadow colour (no blur); the outline casts a shadow too
        if let Some(sh) = &it.shadow {
            let (dx, dy) = (sh.dx / T, sh.dy / T);
            let col = hex_color_units(res.sym(sh.color));
            for p in &parts {
                if !p.fill && stroke.is_none() {
                    continue;
                }
                let sc: Vec<PathCmd> = pt_path(&p.cmds)
                    .into_iter()
                    .map(|c| match c {
                        PathCmd::Z => PathCmd::Z,
                        PathCmd::C { x1, y1, x2, y2, x, y } => {
                            PathCmd::C { x1: x1 + dx, y1: y1 + dy, x2: x2 + dx, y2: y2 + dy, x: x + dx, y: y + dy }
                        }
                        PathCmd::M { x, y } => PathCmd::M { x: x + dx, y: y + dy },
                        PathCmd::L { x, y } => PathCmd::L { x: x + dx, y: y + dy },
                    })
                    .collect();
                let shadow_fill = if p.fill && (fill.is_some() || it.grad.is_some()) { Some(col) } else { None };
                let shadow_stroke = if p.stroke { stroke.as_ref().map(|s| s.with_color(col)) } else { None };
                let ss = shadow_stroke.as_ref().map(StrokeSpec::as_pdf);
                let a = sh.alpha.get();
                page.with_alpha(a, a, |pg| pg.path(&sc, shadow_fill.as_ref(), ss.as_ref(), p.even_odd));
            }
        }
        // pattern (a:pattFill) and image/texture (a:blipFill): a raster clipped by the path
        if it.pattern.is_some() || it.blip.is_some() {
            for p in parts.iter().filter(|p| p.fill) {
                let cmds = pt_path(&p.cmds);
                page.push_clip_path(&cmds, p.even_odd);
                if let Some(pat) = &it.pattern {
                    let pr = pattern_raster(res.sym(pat.prst), res.sym(pat.fg), res.sym(pat.bg), x, y, w, h, 0.75, 4.0, &self.opts.paint);
                    if let Ok(Some(pr)) = pr
                        && let (Ok(pw), Ok(ph)) = (u32::try_from(pr.w), u32::try_from(pr.h))
                        && let Some(img) = self.embed_rgba(page, &pr.data, pw, ph)
                    {
                        page.draw_image(img, pr.x, pr.y, pr.cells_w * 0.75, pr.cells_h * 0.75);
                    }
                } else if let Some(blip) = &it.blip {
                    let im = self.image_for(page, blip.image);
                    let nat = self.res.image(blip.image).and_then(|i| image_pixel_size(&i.bytes));
                    if let Some(im) = im {
                        let tiles = blip_tiles(
                            blip.tile.as_ref(),
                            &res.symbols,
                            x,
                            y,
                            w,
                            h,
                            1.0 / 12700.0,
                            nat.map(|n| (f64::from(n.width), f64::from(n.height))),
                            &self.opts.paint,
                        );
                        if let Ok(tiles) = tiles {
                            page.with_alpha(blip.alpha.get(), None, |pg| {
                                for t in &tiles {
                                    pg.draw_image(im, t.x, t.y, t.w, t.h);
                                }
                            });
                        }
                    }
                }
                page.pop_clip();
                if page.failure().is_some() {
                    return;
                }
            }
            if let Some(s) = &st_pdf {
                for p in parts.iter().filter(|p| p.stroke) {
                    let cmds = pt_path(&p.cmds);
                    page.with_alpha(None, st.line_alpha.get(), |pg| pg.path(&cmds, None, Some(s), false));
                }
            }
            return;
        }
        // gradient in rectangles ("From Center"): a raster in the frame of the shape, rotated with it
        if let Some(g) = it.grad.as_ref().filter(|g| is_rect_gradient(Some(g), &res.symbols)) {
            if let Ok(r) = rect_gradient_raster(g, &res.symbols, w, h, 2.0, &self.opts.paint)
                && let (Ok(rw), Ok(rh)) = (u32::try_from(r.w), u32::try_from(r.h))
                && let Some(im) = self.embed_rgba(page, &r.data, rw, rh)
            {
                for p in parts.iter().filter(|p| p.fill) {
                    let cmds = pt_path(&p.cmds);
                    page.push_clip_path(&cmds, p.even_odd);
                    page.with_alpha(st.fill_alpha.get(), None, |pg| pg.draw_image_transformed(im, x, y, w, h, rot, st.flip_h, st.flip_v));
                    page.pop_clip();
                }
            }
            if let Some(s) = &st_pdf {
                for p in parts.iter().filter(|p| p.stroke) {
                    let cmds = pt_path(&p.cmds);
                    page.with_alpha(None, st.line_alpha.get(), |pg| pg.path(&cmds, None, Some(s), false));
                }
            }
            return;
        }
        if let Some(g) = &it.grad {
            let all: Vec<RPath> = parts.iter().flat_map(|p| p.cmds.iter().copied()).collect();
            let pb = geom::path_bounds(&all);
            let gg = gradient_geometry(g, &res.symbols, GradBox { x, y, w, h, rot }, pb);
            let stops: Vec<(f64, &[u16])> = g.stops.iter().map(|s| (s.pos, res.sym(s.color))).collect();
            let n = if g.path.is_some_and(|p| !res.sym(p).is_empty()) { 256 } else { 512 };
            if let Ok(smp) = gradient_samples(&stops, n, &self.opts.paint)
                && let Some(sh) = self.shade(page, gg, &smp)
            {
                for p in parts.iter().filter(|p| p.fill) {
                    let cmds = pt_path(&p.cmds);
                    page.with_alpha(st.fill_alpha.get(), None, |pg| pg.fill_shaded(&cmds, sh, p.even_odd));
                }
            }
            if let Some(s) = &st_pdf {
                for p in parts.iter().filter(|p| p.stroke) {
                    let cmds = pt_path(&p.cmds);
                    page.with_alpha(None, st.line_alpha.get(), |pg| pg.path(&cmds, None, Some(s), false));
                }
            }
            return;
        }
        // transparency of the fill/outline (a:alpha, v:fill opacity)
        for p in &parts {
            let cmds = pt_path(&p.cmds);
            page.with_alpha(st.fill_alpha.get(), st.line_alpha.get(), |pg| {
                pg.path(&cmds, if p.fill { fill.as_ref() } else { None }, if p.stroke { st_pdf.as_ref() } else { None }, p.even_odd);
            });
            if page.failure().is_some() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fx_downscale;

    /// The resolutions the Word wrote (dpi = 200 / k) for one run per radius (fx_downscale's record).
    #[test]
    fn the_downscale_follows_the_word() {
        let glow = [(2.0, 1.0), (18.0, 1.0), (19.0, 1.0), (20.0, 2.0), (21.0, 2.0), (36.0, 2.0), (40.0, 2.0), (41.0, 3.0), (60.0, 3.0), (61.0, 4.0), (79.0, 4.0), (80.0, 4.0), (81.0, 5.0), (100.0, 5.0), (150.0, 8.0)];
        for (rad, k) in glow {
            assert_eq!(fx_downscale(rad, 20.0), k, "glow {rad} pt");
            // the radius as the file stores it (EMU) and the reader converts it
            assert_eq!(fx_downscale(uo_jsnum::math::round(rad * 12_700.0) / 12_700.0, 20.0), k, "glow {rad} pt from EMU");
        }
        let shadow = [(2.0, 1.0), (10.0, 2.0), (18.0, 2.0), (20.0, 2.0), (21.0, 3.0), (30.0, 3.0), (40.0, 4.0), (41.0, 5.0), (60.0, 6.0), (61.0, 7.0), (80.0, 8.0)];
        for (blur, k) in shadow {
            assert_eq!(fx_downscale(blur, 10.0), k, "shadow blur {blur} pt");
        }
        assert_eq!(fx_downscale(f64::NAN, 20.0), 1.0);
        assert_eq!(fx_downscale(f64::INFINITY, 20.0), 1.0);
        assert_eq!(fx_downscale(0.0, 10.0), 1.0);
    }
}
