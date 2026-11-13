//! Drawn objects (frontier F7b): the image identities of a layout call (RQ3), the pass's face
//! lookup for the metafile players, and the items every object painter builds — the image item
//! with its `pictureLook`, the metafile inside its clip, the shape item with its fill, stroke and
//! extras (engine.ts:3602–3640, paint.ts:196–221). `paintObject` (textboxes, groups) is
//! `Engine::paint_object` in `floats.rs`.
//!
//! **Image identity (RQ3, F7b-1).** The TS labels an image by its object: a model `InlineImage`
//! (also when a header repeats it on every page), the `ImageData` of a shape's `blip`, and every
//! object the layout builds (a picture-bullet image per prepared paragraph, a group child copied
//! under the group's rotation, the EMF bitmaps). [`ImageTable`] gives a model object one
//! `ImageId` per layout call (keyed by its `DrawingId` and slot) and every built object a new one;
//! the bytes are the model's `Arc`s, never copied.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::drawing::{ResWriter, picture_look, resolve_color, shape_extras, shape_geom, with_theme_styles};
use crate::error::{LResult, LayoutError, LimitKind};
use crate::fonts::FontTable;
use crate::fonts::{FaceEnv, FaceResolverCache, FaceTable, FontProvider};
use crate::items::{ItemSide, PassItems};
use crate::metafile::{FaceLookup, MetafileFrame, MetafileLimits, paint_metafile};
use crate::model::{DrawingId, ImageData, InlineImage, JsNumber, ShapeStyle, Theme};
use crate::output::ImageOrigin;
use uo_render::types::{Blip, ImageItem, Item, ShapeItem, Stroke};
use uo_render::{FaceId, ImageId, ImageRes, OptF64, Resources};

use super::Cx;

/// Which image of a model drawing: its picture, or the blip fill of its shape style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImgSlot {
    /// `run.image` (an `InlineImage`)
    Picture,
    /// `run.shape.style.blip.image`
    ShapeBlip,
    /// `run.textbox.shape.blip.image`
    TextboxBlip,
}

/// The images of one layout call (F7b-1): the origin of every `ImageId` (same index as
/// `Resources::images`), the id of each model image object, and the warnings the metafile players
/// raised during the pass (moved into the pass's `DocContext` when it ends).
#[derive(Debug, Default)]
pub struct ImageTable {
    pub origins: Vec<ImageOrigin>,
    by_model: FxHashMap<(DrawingId, ImgSlot), ImageId>,
    pub warnings: Vec<Box<str>>,
}

impl ImageTable {
    #[must_use]
    pub fn new() -> ImageTable {
        ImageTable::default()
    }

    /// Model image objects with an id (M5: bounded by the document's drawings).
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_model.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_model.is_empty()
    }

    /// A copy as it was with `images` images and `warnings` warnings (both only grow within a
    /// call; the ids of `by_model` are indices of the images): a checkpoint's (LA7).
    #[must_use]
    pub fn prefix(&self, images: usize, warnings: usize) -> ImageTable {
        ImageTable {
            origins: self.origins.iter().take(images).cloned().collect(),
            by_model: self.by_model.iter().filter(|(_, id)| id.index() < images).map(|(k, v)| (*k, *v)).collect(),
            warnings: self.warnings.iter().take(warnings).cloned().collect(),
        }
    }

    /// The same images, identities and pending warnings as `other` (LA7).
    #[must_use]
    pub fn same_as(&self, other: &ImageTable) -> bool {
        self.warnings == other.warnings
            && format!("{:?}", self.origins) == format!("{:?}", other.origins)
            && self.by_model.len() == other.by_model.len()
            && self.by_model.iter().all(|(k, v)| other.by_model.get(k) == Some(v))
    }
}

fn capacity() -> LayoutError {
    LayoutError::Limit(LimitKind::Capacity)
}

/// `{ ...effects }` of the model as the painters' `ImageEffects` (`undefined`/`null` = absent).
fn effects_of(e: &crate::model::ImageEffects) -> uo_drawingml::ImageEffects {
    uo_drawingml::ImageEffects {
        gray: e.gray == Some(true),
        gain: e.gain.and_then(JsNumber::as_f64),
        bright: e.bright.and_then(JsNumber::as_f64),
        floor: e.floor == Some(true),
        bi_level: e.bi_level.and_then(JsNumber::as_f64),
    }
}

/// A new image object over a media payload of the document.
#[expect(clippy::too_many_arguments, reason = "the fields of the TS image object")]
fn add_image(
    cx: &mut Cx<'_, '_>,
    media: crate::model::MediaId,
    ext: &str,
    svg: Option<crate::model::MediaId>,
    effects: Option<uo_drawingml::ImageEffects>,
    width_twips: f64,
    height_twips: f64,
    drawing: Option<DrawingId>,
) -> LResult<ImageId> {
    let view = cx.ldoc.view;
    let bytes: Arc<[u8]> = view.media_arc(media).cloned().unwrap_or_else(|| Arc::from(Vec::new()));
    let svg = svg.and_then(|m| view.media_arc(m).cloned());
    let natural_px = uo_image::image_pixel_size(&bytes).map(|p| (p.width, p.height));
    let ext = cx.res.symbols.intern_str(ext).ok_or_else(capacity)?;
    let id = cx.res.add_image(ImageRes { bytes, ext, svg, effects, width_twips, height_twips, natural_px }).ok_or_else(capacity)?;
    cx.images.origins.push(ImageOrigin::Media { media, drawing });
    Ok(id)
}

/// The `ImageId` of a picture: the model object's (one per call), or a new one for an object the
/// layout built from it (`copy`: a group child under rotation/flip, `{ ...r0.image, style }`).
pub(crate) fn picture_id(cx: &mut Cx<'_, '_>, im: &InlineImage, copy: bool) -> LResult<ImageId> {
    let key = (im.drawing, ImgSlot::Picture);
    if !copy && let Some(&id) = cx.images.by_model.get(&key) {
        return Ok(id);
    }
    let id = add_image(
        cx,
        im.data.0,
        &im.ext,
        im.svg.map(|s| s.0),
        im.effects.as_ref().map(effects_of),
        im.width_twips.to_number(),
        im.height_twips.to_number(),
        Some(im.drawing),
    )?;
    if !copy {
        cx.images.by_model.insert(key, id);
    }
    Ok(id)
}

/// The `ImageId` of a shape's blip image (`ImageData`, no size: `widthTwips` is `undefined`).
pub(crate) fn blip_id(cx: &mut Cx<'_, '_>, owner: DrawingId, slot: ImgSlot, img: &ImageData) -> LResult<ImageId> {
    let key = (owner, slot);
    if let Some(&id) = cx.images.by_model.get(&key) {
        return Ok(id);
    }
    let id = add_image(cx, img.data.0, &img.ext, None, None, f64::NAN, f64::NAN, Some(owner))?;
    cx.images.by_model.insert(key, id);
    Ok(id)
}

/// The image of a picture bullet (para.ts:1119): a new object per prepared paragraph
/// (`{ data, ext, widthTwips, heightTwips }`), painted once.
pub(crate) fn pic_bullet_id(cx: &mut Cx<'_, '_>, img: &ImageData, w: f64, h: f64) -> LResult<ImageId> {
    add_image(cx, img.data.0, &img.ext, None, None, w, h, None)
}

/// The face lookup of the metafile players over the pass (`ctx.faces.get`, the call's resolver).
struct PassFaces<'x> {
    provider: &'x dyn FontProvider,
    font_table: &'x FontTable,
    table: &'x mut FaceTable,
    resolver: &'x mut FaceResolverCache,
    warnings: &'x mut Vec<Box<str>>,
}

impl FaceLookup for PassFaces<'_> {
    fn face(&mut self, res: &mut Resources, family: &[u16], bold: bool, italic: bool) -> LResult<FaceId> {
        // (a lone surrogate in a font record's family becomes U+FFFD: no font name has one)
        let fam = String::from_utf16_lossy(family);
        let mut env = FaceEnv { provider: self.provider, font_table: self.font_table, table: &mut *self.table, res };
        self.resolver.get(&mut env, Some(&fam), bold, italic)
    }

    fn warn(&mut self, msg: &str) {
        if !self.warnings.iter().any(|w| &**w == msg) {
            self.warnings.push(msg.into());
        }
    }
}

/// `{ t: 'clip', op: 'push', ...r, src: im }, ...renderEmf|renderWmf(ctx, data, r), { t: 'clip',
/// op: 'pop' }`. `Ok(false)` when the bytes are neither EMF nor WMF.
pub(crate) fn push_metafile(cx: &mut Cx<'_, '_>, data: &[u8], src: ImageId, r: MetafileFrame, out: &mut PassItems) -> LResult<bool> {
    let limits = MetafileLimits::of(&cx.opts.limits);
    let mut faces = PassFaces {
        provider: cx.provider,
        font_table: &cx.ldoc.view.formatting().font_table,
        table: &mut *cx.fonts,
        resolver: &mut *cx.resolver,
        warnings: &mut cx.images.warnings,
    };
    let mut w = ResWriter::new(&mut *cx.res, &mut cx.images.origins);
    paint_metafile(data, src, r, limits, &mut faces, &mut w, out)
}

/// `{ t: 'image', x, y, w, h, image: im, crop?, rot?, flipH?, flipV?, pic? }` (`pictureLook(im,
/// theme)`; `im.style?.rot` truthy).
pub(crate) fn push_image_item(
    cx: &mut Cx<'_, '_>,
    im: Option<&InlineImage>,
    id: ImageId,
    r: MetafileFrame,
    out: &mut PassItems,
) -> LResult<()> {
    let theme = cx.ldoc.theme; // wave 7 (B, gate-O3)
    let pic = match im {
        Some(im) => picture_look(im, theme, &mut ResWriter::new(&mut *cx.res, &mut cx.images.origins))?,
        None => None,
    };
    let st = im.and_then(|i| i.style.as_deref());
    let rot = st.and_then(|s| s.rot).map_or(0.0, JsNumber::to_number);
    out.list.push_plain(Item::Image(Box::new(ImageItem {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        image: id,
        crop: im.and_then(|i| i.crop.as_ref()).map(ltrb),
        rot: if uo_jsnum::conv::truthy(rot) { OptF64::some(rot) } else { OptF64::NONE },
        flip_h: st.and_then(|s| s.flip_h) == Some(true),
        flip_v: st.and_then(|s| s.flip_v) == Some(true),
        pic: pic.map(Box::new),
    })));
    out.side.push(ItemSide::default());
    Ok(())
}

fn ltrb(c: &crate::model::Ltrb) -> uo_render::types::Ltrb {
    let n = |v: JsNumber| v.to_number();
    uo_render::types::Ltrb { l: n(c.l), t: n(c.t), r: n(c.r), b: n(c.b) }
}

/// The resolved look of a drawn shape beyond its geometry (`paintObject`: fill, stroke, the VML
/// line ends, `shapeExtras`); the inline shapes of `paintInlineObject` carry none of it.
pub(crate) struct ShapeLook {
    /// the drawing whose `blip` it is (its image identity)
    pub owner: DrawingId,
    pub slot: ImgSlot,
    pub line: Option<[f64; 4]>,
}

/// `resolveDrawingColor(withThemeStyles(s, theme).fill, theme)`.
fn fill_of(s: &ShapeStyle, theme: &Theme) -> Option<String> {
    let ts = with_theme_styles(s, theme);
    ts.fill.as_ref().and_then(|c| resolve_color(c, theme))
}

/// `s.line ? { color: resolveDrawingColor(s.line.color) ?? '000000', widthPt: lw, dash? }` with the
/// theme line width of a `lnRef` without `a:ln@w` (oracle txbxln).
fn stroke_of(s: &ShapeStyle, theme: &Theme, w: &mut ResWriter<'_>) -> LResult<Option<Stroke>> {
    let Some(ln) = s.line.as_deref() else { return Ok(None) };
    let lw = match ln.r#ref {
        Some(r) if r.truthy() => {
            let i = r.to_number() - 1.0;
            let from_theme = theme.line_widths_pt.as_ref().and_then(|v| crate::engine::index_of(i).and_then(|k| v.get(k))).copied();
            // `?.[ref - 1] ?? s.line.widthPt`: a theme entry of `null` is nullish too
            from_theme.and_then(JsNumber::as_f64).unwrap_or_else(|| ln.width_pt.to_number())
        }
        _ => ln.width_pt.to_number(),
    };
    let color = resolve_color(&ln.color, theme).unwrap_or_else(|| "000000".to_owned());
    let dash = match ln.dash.as_deref() {
        Some(d) if !d.is_empty() => Some(w.sym(d)?),
        _ => None,
    };
    Ok(Some(Stroke { color: w.sym(&color)?, width_pt: lw, dash }))
}

/// A shape item: `{ t: 'shape', x, y, w, h, style }` (inline shapes) or, with `look`, the full one of
/// `paintObject` (`fill`, `stroke`, `line`, `...shapeExtras(s, theme)`).
pub(crate) fn push_shape_item(
    cx: &mut Cx<'_, '_>,
    style: &ShapeStyle,
    r: [f64; 4],
    look: Option<&ShapeLook>,
    out: &mut PassItems,
) -> LResult<()> {
    let theme = cx.ldoc.theme; // wave 7 (B, gate-O3)
    let [x, y, w, h] = r;
    let mut item = {
        let mut rw = ResWriter::new(&mut *cx.res, &mut cx.images.origins);
        let geom = shape_geom(style, &mut rw)?;
        ShapeItem { x, y, w, h, geom, fill: None, stroke: None, line: None, grad: None, shadow: None, pattern: None, blip: None, fx: None }
    };
    if let Some(lk) = look {
        let blip_ref = {
            let mut rw = ResWriter::new(&mut *cx.res, &mut cx.images.origins);
            item.fill = match fill_of(style, theme) {
                Some(f) => Some(rw.sym(&f)?),
                None => None,
            };
            item.stroke = stroke_of(style, theme, &mut rw)?;
            item.line = lk.line;
            let ex = shape_extras(style, theme, &mut rw)?;
            item.grad = ex.grad;
            item.shadow = ex.shadow;
            item.pattern = ex.pattern;
            item.fx = ex.fx;
            ex.blip.map(|b| (b.image.clone(), b.tile, b.alpha))
        };
        if let Some((img, tile, alpha)) = blip_ref {
            let image = blip_id(cx, lk.owner, lk.slot, &img)?;
            item.blip = Some(Blip { image, tile, alpha });
        }
    }
    out.list.push_plain(Item::Shape(Box::new(item)));
    out.side.push(ItemSide::default());
    Ok(())
}
