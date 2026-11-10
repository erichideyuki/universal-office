//! The view over a layout's pages that the shell holds (uo-render §5 "wasm exports for the
//! canvas"): the pages and their [`Resources`], the per-view [`DeltaMark`] of the encoder, and the
//! **item-addressed paint operations** the canvas calls at paint time, so no item is marshalled
//! back to JS: zoom-dependent rasters (`fx_layers`, `pattern_raster(15, 1)`,
//! `rect_gradient_raster(2)`, `blip_tiles(1/635)`, the page-fill tiles) and the picture helpers
//! with `Painter::Canvas`. The uo-wasm exports are thin wrappers over these methods.
//!
//! A view is fed by the layout (`RenderView::new`) or, for tests, tools and the TS adapter of
//! R-P2c, by encoded messages ([`RenderView::push_message`]: a layout encoded by the generated TS
//! encoder is painted by the same exports). The font and image bytes the wire does not carry are
//! attached with [`RenderView::set_face_bytes`] / [`RenderView::set_image_bytes`].

use crate::encode::decode::{DecodeTables, decode_page};
use crate::encode::hints::shape_has_effects;
use crate::encode::{AppSection, CanvasOptions, DeltaMark, EncodeError, encode_page_for_canvas_with};
use crate::list::ItemList;
use crate::paint::fill::{PatternRaster, RectRaster, Tile, blip_tiles, pattern_raster, rect_gradient_raster};
use crate::paint::page_fill::{gradient_tile_px, gradient_tile_rgba, page_fill_tile_tw, pattern_rgba};
use crate::paint::picture::{AdjustParts, BevelParts, Bounds, Painter, effect_bounds, shadow_spec, shape_subs};
use crate::paint::text_fx::{FxLayer, RunView, fx_layer_set, fx_layers};
use crate::paint::{PaintError, PaintLimits};
use crate::resources::Resources;
use crate::types::{Item, Page, PageFillKind, PictureLook, ShapeItem};
use std::sync::Arc;
use uo_codec::{DecodeError, Limits};
use uo_drawingml::{
    Affine, BoxF, Mask, Polygons, Reflection, Subpaths, apply_image_effects, apply_picture_adjust, fill_polygons, flatten_path,
    gaussian_blur, glow_mask, inner_shadow_mask, outer_shadow_mask, reflection_alpha, shade_bevel, soft_edge_mask,
};

/// Why a view operation refused.
#[derive(Debug)]
pub enum ViewError {
    /// no page with that index
    NoPage,
    /// no item with that index on the page
    NoItem,
    /// the item is not the kind the operation reads, or lacks the field it paints
    WrongItem,
    /// an id of the item is past its table, or the face has no font
    NoResource,
    Paint(PaintError),
    Decode(DecodeError),
    Encode(EncodeError),
    /// the bytes given for a face are not a font
    BadFont,
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewError::NoPage => f.write_str("no such page"),
            ViewError::NoItem => f.write_str("no such item"),
            ViewError::WrongItem => f.write_str("the item does not have what the operation paints"),
            ViewError::NoResource => f.write_str("a resource of the item is missing"),
            ViewError::Paint(e) => write!(f, "{e}"),
            ViewError::Decode(e) => write!(f, "{e}"),
            ViewError::Encode(e) => write!(f, "{e}"),
            ViewError::BadFont => f.write_str("the bytes are not a font"),
        }
    }
}

impl std::error::Error for ViewError {}

impl From<PaintError> for ViewError {
    fn from(e: PaintError) -> ViewError {
        ViewError::Paint(e)
    }
}

/// The raster of a page fill: a gradient tile or the 8 x 8 pattern, RGBA.
#[derive(Clone, Debug)]
pub struct FillRaster {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// Pages, resources and encoder state of one layout.
pub struct RenderView {
    res: Resources,
    dec: Option<DecodeTables>,
    pages: Vec<Page>,
    apps: Vec<Vec<u8>>,
    mark: DeltaMark,
    opts: CanvasOptions,
    /// limits of the paint operations (a host short of memory lowers `max_fx_px`)
    pub limits: PaintLimits,
}

struct Bytes<'a>(&'a [u8]);
impl AppSection for Bytes<'_> {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.0);
    }
}

impl RenderView {
    /// A view over pages the layout produced.
    #[must_use]
    pub fn new(generation: u32, res: Resources, pages: Vec<Page>) -> RenderView {
        let apps = vec![Vec::new(); pages.len()];
        RenderView {
            res,
            dec: None,
            pages,
            apps,
            mark: DeltaMark::new(generation),
            opts: CanvasOptions::default(),
            limits: PaintLimits::DEFAULT,
        }
    }

    /// An empty view fed by [`RenderView::push_message`].
    #[must_use]
    pub fn from_messages(generation: u32) -> RenderView {
        RenderView::new(generation, Resources::new(), Vec::new())
    }

    #[must_use]
    pub fn resources(&self) -> &Resources {
        self.dec.as_ref().map_or(&self.res, |d| d.resources())
    }

    fn resources_mut(&mut self) -> &mut Resources {
        match self.dec.as_mut() {
            Some(d) => d.resources_mut(),
            None => &mut self.res,
        }
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    #[must_use]
    pub fn page(&self, i: usize) -> Option<&Page> {
        self.pages.get(i)
    }

    /// Every page (the PDF of the view reads `&[Page]`; added by the bindings, uo-bindings.md).
    #[must_use]
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// Sets the producer's section of a page (what `encode_page` carries as `app`).
    pub fn set_app(&mut self, page: usize, bytes: Vec<u8>) -> Result<(), ViewError> {
        *self.apps.get_mut(page).ok_or(ViewError::NoPage)? = bytes;
        Ok(())
    }

    /// Appends the message of page `i` to `out` (resource deltas from this view's mark).
    ///
    /// # Errors
    /// See [`EncodeError`]; `out` and the mark are untouched.
    pub fn encode_page(&mut self, i: usize, out: &mut Vec<u8>) -> Result<(), ViewError> {
        let page = self.pages.get(i).ok_or(ViewError::NoPage)?;
        let app = self.apps.get(i).map_or(&[][..], Vec::as_slice);
        let res = self.dec.as_ref().map_or(&self.res, |d| d.resources());
        encode_page_for_canvas_with(page, res, &Bytes(app), &mut self.mark, &self.opts, out).map_err(ViewError::Encode)
    }

    /// Decodes one page message and appends the page; returns its index.
    ///
    /// # Errors
    /// The codec's errors; the view is unchanged.
    pub fn push_message(&mut self, buf: &[u8], limits: &Limits) -> Result<usize, ViewError> {
        let dec = self.dec.get_or_insert_with(DecodeTables::new);
        let got = decode_page(buf, dec, limits).map_err(ViewError::Decode)?;
        self.pages.push(got.page);
        self.apps.push(got.app);
        Ok(self.pages.len() - 1)
    }

    /// Attaches the font file of face `i` (the wire carries `has_bytes` only).
    ///
    /// # Errors
    /// [`ViewError::BadFont`] when the bytes do not parse; [`ViewError::NoResource`] for a bad index.
    pub fn set_face_bytes(&mut self, i: usize, bytes: Arc<[u8]>) -> Result<(), ViewError> {
        let face = uo_fonts::Face::parse(Arc::clone(&bytes), &uo_fonts::FontLimits::DEFAULT).map_err(|_| ViewError::BadFont)?;
        let f = self.resources_mut().faces.get_mut(i).ok_or(ViewError::NoResource)?;
        f.face = Some(face);
        f.bytes = Some(bytes);
        Ok(())
    }

    /// Attaches the bytes of image `i` and, when known, its natural size in pixels.
    ///
    /// # Errors
    /// [`ViewError::NoResource`] for a bad index.
    pub fn set_image_bytes(&mut self, i: usize, bytes: Arc<[u8]>, natural: Option<(u32, u32)>) -> Result<(), ViewError> {
        let im = self.resources_mut().images.get_mut(i).ok_or(ViewError::NoResource)?;
        im.bytes = bytes;
        if natural.is_some() {
            im.natural_px = natural;
        }
        Ok(())
    }

    fn item(&self, page: usize, item: usize) -> Result<(&ItemList, &Item), ViewError> {
        let p = self.pages.get(page).ok_or(ViewError::NoPage)?;
        Ok((&p.list, p.list.items.get(item).ok_or(ViewError::NoItem)?))
    }

    fn shape(&self, page: usize, item: usize) -> Result<&ShapeItem, ViewError> {
        match self.item(page, item)?.1 {
            Item::Shape(s) => Ok(s),
            _ => Err(ViewError::WrongItem),
        }
    }

    /// The frame a shape paints in: the item's, or, for a shape with effects, the local one at the
    /// origin (canvas.ts:430).
    fn frame(&self, sh: &ShapeItem) -> (f64, f64, f64, f64) {
        if shape_has_effects(sh, self.resources()) { (0.0, 0.0, sh.w, sh.h) } else { (sh.x, sh.y, sh.w, sh.h) }
    }

    /// `fxLayers(it, fx, ppt, it.color)` of a glyph run with text effects.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a run without effects; the helper's limits.
    pub fn fx_layers(&self, page: usize, item: usize, ppt: f64) -> Result<Option<FxLayer>, ViewError> {
        let (list, it) = self.item(page, item)?;
        let Item::Glyphs { run, color, .. } = it else { return Err(ViewError::WrongItem) };
        let res = self.resources();
        let fx = list.run(*run).and_then(|s| s.fx).and_then(|id| res.text_fx(id)).ok_or(ViewError::WrongItem)?;
        let view = RunView::of(list, it, res).ok_or(ViewError::NoResource)?;
        Ok(fx_layers(&view, fx, &res.symbols, ppt, res.sym(*color), &self.limits)?)
    }

    /// The layers the screen paints a glyph run's effects in at `ppt` ([`fx_layer_set`]: one layer
    /// as [`Self::fx_layers`] while every blur is under the Word's step, else a layer per effect at
    /// its own resolution), each with its pixels per point.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a run without effects; the helper's limits.
    pub fn fx_layer_set(&self, page: usize, item: usize, ppt: f64) -> Result<Vec<(f64, FxLayer)>, ViewError> {
        let (list, it) = self.item(page, item)?;
        let Item::Glyphs { run, color, .. } = it else { return Err(ViewError::WrongItem) };
        let res = self.resources();
        let fx = list.run(*run).and_then(|s| s.fx).and_then(|id| res.text_fx(id)).ok_or(ViewError::WrongItem)?;
        let view = RunView::of(list, it, res).ok_or(ViewError::NoResource)?;
        Ok(fx_layer_set(&view, fx, &res.symbols, ppt, res.sym(*color), &self.limits)?)
    }

    /// `patternRaster(pattern, x, y, w, h, 15)` of a shape with an `a:pattFill` (canvas.ts:464).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a shape without a pattern; the helper's limits.
    pub fn pattern_raster(&self, page: usize, item: usize) -> Result<Option<PatternRaster>, ViewError> {
        let sh = self.shape(page, item)?;
        let p = sh.pattern.ok_or(ViewError::WrongItem)?;
        let (x, y, w, h) = self.frame(sh);
        let s = &self.resources().symbols;
        Ok(pattern_raster(s.get(p.prst), s.get(p.fg), s.get(p.bg), x, y, w, h, 15.0, 1.0, &self.limits)?)
    }

    /// `rectGradientRaster(grad, w / 20, h / 20, 2)` of a shape with the rectangular gradient.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a shape without a gradient; the helper's limits.
    pub fn rect_gradient(&self, page: usize, item: usize) -> Result<RectRaster, ViewError> {
        let sh = self.shape(page, item)?;
        let g = sh.grad.as_ref().ok_or(ViewError::WrongItem)?;
        Ok(rect_gradient_raster(g, &self.resources().symbols, sh.w / 20.0, sh.h / 20.0, 2.0, &self.limits)?)
    }

    /// `blipTiles(blip, x, y, w, h, 1 / 635, imagePixelSize(image))` of a shape with a picture fill.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a shape without a blip; the helper's limits.
    pub fn blip_tiles(&self, page: usize, item: usize) -> Result<Vec<Tile>, ViewError> {
        let sh = self.shape(page, item)?;
        let b = sh.blip.as_ref().ok_or(ViewError::WrongItem)?;
        let res = self.resources();
        let natural = res.image(b.image).and_then(|im| im.natural_px).map(|(w, h)| (f64::from(w), f64::from(h)));
        let (x, y, w, h) = self.frame(sh);
        Ok(blip_tiles(b.tile.as_ref(), &res.symbols, x, y, w, h, 1.0 / 635.0, natural, &self.limits)?)
    }

    fn fill_of(&self, page: usize) -> Result<&crate::types::PageFill, ViewError> {
        let p = self.pages.get(page).ok_or(ViewError::NoPage)?;
        let id = p.background_fill.ok_or(ViewError::WrongItem)?;
        self.resources().page_fill(id).ok_or(ViewError::NoResource)
    }

    /// `pageFillTileTw(f, devicePxTw, devicePxTw)` of the page's background fill (the screen).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a page without a fill.
    pub fn page_fill_tile_tw(&self, page: usize, device_px_tw: f64) -> Result<Option<(f64, f64)>, ViewError> {
        Ok(page_fill_tile_tw(self.fill_of(page)?, device_px_tw, device_px_tw, false))
    }

    /// The source tile of the page's fill: `gradientTileRgba(f, t.w, t.h)` for a gradient,
    /// `patternRgba(f)` (8 x 8) for a pattern; `None` for the image fills (the shell draws the image).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for a page without a fill; the helper's limits.
    pub fn page_fill_raster(&self, page: usize) -> Result<Option<FillRaster>, ViewError> {
        let f = self.fill_of(page)?;
        let syms = &self.resources().symbols;
        match f.kind {
            PageFillKind::Gradient | PageFillKind::GradientRadial => {
                let (tw, th) = gradient_tile_px(f, false);
                let cap = self.limits.max_raster_px;
                let (Some(w), Some(h)) = (crate::paint::count(tw, cap), crate::paint::count(th, cap)) else {
                    return Err(PaintError::Invalid.into());
                };
                let data = gradient_tile_rgba(f, syms, w, h, false, &self.limits)?;
                Ok(Some(FillRaster { w, h, data }))
            }
            PageFillKind::Pattern => Ok(Some(FillRaster { w: 8, h: 8, data: pattern_rgba(f, syms) })),
            PageFillKind::Frame | PageFillKind::Tile => Ok(None),
        }
    }

    /// The look `canvas-picture.ts` reads for an item: an image's `pic`, or a shape with effects
    /// `{ ...fx, line? }` (canvas-picture.ts:279).
    fn look_of(&self, page: usize, item: usize) -> Result<(PictureLook, f64, f64, bool, bool), ViewError> {
        match self.item(page, item)?.1 {
            Item::Image(im) => Ok((im.pic.as_deref().cloned().unwrap_or_default(), im.w / 20.0, im.h / 20.0, im.flip_h, im.flip_v)),
            Item::Shape(sh) => {
                let fx = sh.fx.ok_or(ViewError::WrongItem)?;
                let mut look = PictureLook {
                    shadow: fx.shadow,
                    glow: fx.glow,
                    soft_edge_pt: fx.soft_edge_pt,
                    reflection: fx.reflection,
                    ..PictureLook::default()
                };
                let lw = sh.stroke.map_or(0.0, |s| s.width_pt);
                if uo_jsnum::conv::truthy(lw) {
                    let color = sh.stroke.map(|s| s.color).ok_or(ViewError::WrongItem)?;
                    look.line = Some(crate::types::PicLine {
                        color,
                        alpha: 1.0,
                        width_pt: lw / 2.0,
                        dash: None,
                        cmpd: None,
                        cap: None,
                        join: None,
                    });
                }
                Ok((look, sh.w / 20.0, sh.h / 20.0, sh.geom.flip_h, sh.geom.flip_v))
            }
            _ => Err(ViewError::WrongItem),
        }
    }

    /// `bounds(look, w, h)` of `canvas-picture.ts` (pt, relative to the item corner) for an image
    /// with a picture look or a shape with effects.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for any other item.
    pub fn picture_bounds(&self, page: usize, item: usize) -> Result<Bounds, ViewError> {
        let (look, w, h, _, _) = self.look_of(page, item)?;
        Ok(effect_bounds(Painter::Canvas, Some(&look), w, h))
    }

    /// `shapeSubs(look, w, h, flipH, flipV)` of an image with a picture look (the filled subpaths of
    /// its shape, pt).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for any other item.
    pub fn picture_subs(&self, page: usize, item: usize) -> Result<Subpaths, ViewError> {
        let Item::Image(im) = self.item(page, item)?.1 else { return Err(ViewError::WrongItem) };
        Ok(shape_subs(
            im.pic.as_deref().and_then(|p| p.geom.as_ref()),
            &self.resources().symbols,
            im.w / 20.0,
            im.h / 20.0,
            im.flip_h,
            im.flip_v,
        ))
    }
    // ───────────── the picture layer of canvas-picture.ts (frontier D): masks over the item's look ─────────────

    fn image_item(&self, page: usize, item: usize) -> Result<&crate::types::ImageItem, ViewError> {
        match self.item(page, item)?.1 {
            Item::Image(im) => Ok(im),
            _ => Err(ViewError::WrongItem),
        }
    }

    /// `flattenPath(shapeSubs(look, w, h, flipH, flipV), [ppt, 0, 0, ppt, ox, oy], 0.35)` of an image
    /// (canvas-picture.ts:151-152).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for any other item; the mask limits.
    pub fn picture_polys(&self, page: usize, item: usize, ppt: f64, ox: f64, oy: f64) -> Result<Polygons, ViewError> {
        let subs = self.picture_subs(page, item)?;
        Ok(flatten_path(&subs, &Affine([ppt, 0.0, 0.0, ppt, ox, oy]), 0.35, &self.limits.draw).map_err(PaintError::from)?)
    }

    /// `fillPolygons(newMask(W, H), polys)` of the image's polygons (canvas-picture.ts:172, 178).
    ///
    /// # Errors
    /// As [`RenderView::picture_polys`].
    #[allow(clippy::too_many_arguments)]
    pub fn picture_silhouette(&self, page: usize, item: usize, ppt: f64, ox: f64, oy: f64, w: u32, h: u32) -> Result<Mask, ViewError> {
        let polys = self.picture_polys(page, item, ppt, ox, oy)?;
        let mut m = Mask::new(w, h, &self.limits.draw).map_err(PaintError::from)?;
        fill_polygons(&mut m, &polys, false, &self.limits.draw).map_err(PaintError::from)?;
        Ok(m)
    }

    /// The bevel of `buildLayer` (canvas-picture.ts:170-175): the shape's silhouette and
    /// `shadeBevel(rgba, silhouette, bevel)` over the layer's pixels.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for an image without a bevel; the mask limits; `Paint(Invalid)` when `rgba` is not `w * h * 4`.
    #[allow(clippy::too_many_arguments)]
    pub fn picture_bevel(
        &self,
        page: usize,
        item: usize,
        ppt: f64,
        ox: f64,
        oy: f64,
        w: u32,
        h: u32,
        rgba: &mut [u8],
    ) -> Result<(), ViewError> {
        let im = self.image_item(page, item)?;
        let bv = im.pic.as_deref().and_then(|p| p.bevel).ok_or(ViewError::WrongItem)?;
        let want = u64::from(w).checked_mul(u64::from(h)).and_then(|n| n.checked_mul(4));
        if u64::try_from(rgba.len()).ok() != want {
            return Err(PaintError::Invalid.into());
        }
        let sil = self.picture_silhouette(page, item, ppt, ox, oy, w, h)?;
        let parts = BevelParts::of(self.resources(), &bv, ppt);
        shade_bevel(rgba, &sil, &parts.spec(), &self.limits.draw).map_err(PaintError::from)?;
        Ok(())
    }

    /// `adjustedBitmap` (canvas-picture.ts:122-140): `applyImageEffects` when the image carries effects, then
    /// `applyPictureAdjust` when the item has a look. `false` (nothing applies: the TS returns the bitmap
    /// itself) leaves `rgba` untouched.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for any other item.
    pub fn picture_adjust(&self, page: usize, item: usize, rgba: &mut [u8]) -> Result<bool, ViewError> {
        let im = self.image_item(page, item)?;
        let res = self.resources();
        let fx = res.image(im.image).and_then(|r| r.effects);
        let look = im.pic.as_deref();
        if fx.is_none() && look.is_none_or(|l| l.alpha.get().is_none() && l.duotone.is_none() && l.clr_change.is_none()) {
            return Ok(false);
        }
        if let Some(fx) = &fx {
            apply_image_effects(rgba, fx);
        }
        if let Some(l) = look {
            apply_picture_adjust(rgba, &AdjustParts::of(res, l).spec());
        }
        Ok(true)
    }

    /// `outerShadowMask(sil, box, spec, ppt)` with the shadow of the item's look (an image's `pic` or a
    /// shape's effects); `dir_deg` is the caller's (the rotation rule stays in the shell).
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for an item without an outer shadow; the mask limits.
    pub fn outer_shadow(&self, page: usize, item: usize, sil: &Mask, bx: BoxF, dir_deg: f64, ppt: f64) -> Result<Mask, ViewError> {
        let (look, ..) = self.look_of(page, item)?;
        let sh = look.shadow.ok_or(ViewError::WrongItem)?;
        Ok(outer_shadow_mask(sil, &bx, &shadow_spec(self.resources(), &sh, dir_deg), ppt, &self.limits.draw).map_err(PaintError::from)?)
    }

    /// The alpha of each row of the reflection (canvas-picture.ts:241-245): `t = (y + 0.5 - top) / hPpt`,
    /// 0 for `t < 0`, else `reflectionAlpha(t, reflection)`.
    ///
    /// # Errors
    /// [`ViewError::WrongItem`] for an item without a reflection; `Paint(Invalid)` over 2^24 rows.
    pub fn reflection_alphas(&self, page: usize, item: usize, top: f64, h_ppt: f64, rows: u32) -> Result<Vec<f64>, ViewError> {
        let (look, ..) = self.look_of(page, item)?;
        let r = look.reflection.ok_or(ViewError::WrongItem)?;
        if rows > 1 << 24 {
            return Err(PaintError::Invalid.into());
        }
        let r = Reflection { st_a: r.st_a, st_pos: r.st_pos, end_a: r.end_a, end_pos: r.end_pos };
        let mut out = Vec::new();
        out.reserve_exact(usize::try_from(rows).unwrap_or(0));
        for y in 0..rows {
            let t = (f64::from(y) + 0.5 - top) / h_ppt;
            out.push(if t < 0.0 { 0.0 } else { reflection_alpha(t, &r) });
        }
        Ok(out)
    }

    /// `innerShadowMask(sil, { blurPt, distPt, dirDeg }, ppt)`.
    ///
    /// # Errors
    /// The mask limits.
    pub fn inner_shadow(&self, sil: &Mask, blur_pt: f64, dist_pt: f64, dir_deg: f64, ppt: f64) -> Result<Mask, ViewError> {
        Ok(inner_shadow_mask(sil, blur_pt, dist_pt, dir_deg, ppt, &self.limits.draw).map_err(PaintError::from)?)
    }

    /// `glowMask(sil, radPt, ppt)`.
    ///
    /// # Errors
    /// The mask limits.
    pub fn glow(&self, sil: &Mask, rad_pt: f64, ppt: f64) -> Result<Mask, ViewError> {
        Ok(glow_mask(sil, rad_pt, ppt, &self.limits.draw).map_err(PaintError::from)?)
    }

    /// `softEdgeMask(sil, radPt, ppt)`.
    ///
    /// # Errors
    /// The mask limits.
    pub fn soft_edge(&self, sil: &Mask, rad_pt: f64, ppt: f64) -> Result<Mask, ViewError> {
        Ok(soft_edge_mask(sil, rad_pt, ppt, &self.limits.draw).map_err(PaintError::from)?)
    }

    /// `gaussianBlur(mask, sigma)`.
    ///
    /// # Errors
    /// The mask limits.
    pub fn blur(&self, m: &Mask, sigma: f64) -> Result<Mask, ViewError> {
        Ok(gaussian_blur(m, sigma, &self.limits.draw).map_err(PaintError::from)?)
    }
}
