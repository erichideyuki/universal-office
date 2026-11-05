//! The paint exports of uo-render for the canvas (frontier E, uo-render §5): a view handle
//! (`UoRenderView`) over a layout's pages, the encoder of the per-page message, and the
//! **item-addressed** paint operations the TS canvas painter calls at paint time — the zoom
//! dependent rasters (text-effect layers, patterns, the rectangular gradient, blip tiles, page-fill
//! tiles) and the picture helpers — so no item travels back to JS.
//!
//! Conventions (the same as the rest of the module): handles are `Box`es released by their `*_free`
//! (JS: `free()` and a `FinalizationRegistry`); input bytes come from [`crate::uo_alloc_bytes`] and
//! are **consumed** by the call that takes them; results are [`UoBuf`] handles read through
//! `uo_buf_status`/`uo_buf_ptr`/`uo_buf_len` and released with `uo_buf_free`. Nothing here panics
//! on its input (a panic aborts the module).
//!
//! # Status codes
//! 0 ok · 1 nothing to paint (a valid answer: no layer, no pattern for that preset…) · 2 schema
//! version mismatch · 3 malformed message · 4 limit exceeded · 5 invalid argument · 6 no such page or
//! item · 7 the item is not what the export reads · 8 a resource is missing · 9 not a font · 10 the
//! page did not encode · 11 null handle.
//!
//! # Result layouts (little endian)
//! - `uo_render_encode_page`: the message bytes.
//! - `uo_render_fx_layers`: `x0 f64, y0 f64` (twips, the grid corner), `w u32, h u32`, RGBA (straight alpha).
//! - `uo_render_pattern`: `x f64, y f64` (the grid corner), `cells_w f64, cells_h f64`, `w u32, h u32`, RGBA.
//! - `uo_render_rect_gradient`: `w u32, h u32`, RGBA.
//! - `uo_render_blip_tiles`: `n u32`, then `n x (x, y, w, h: f64)`.
//! - `uo_render_page_fill_tile_tw`: `w f64, h f64`.
//! - `uo_render_page_fill_raster`: `w u32, h u32`, RGBA.
//! - `uo_render_picture_bounds`: `x0, y0, x1, y1: f64` (pt, relative to the item corner).
//! - `uo_render_picture_subs`: `n u32` subpaths, each `m u32` segments, each a tag byte (`M L C Q E Z`)
//!   and its numbers (`M`/`L` x y · `C` x1 y1 x2 y2 x y · `Q` x1 y1 x y · `E` cx cy rx ry a0 a1 then a
//!   byte ccw · `Z` none), all `f64`.
//! - `uo_render_colored_mask`: RGBA.
//! - `uo_render_dash`: `n u32`, then `n x f64`.

use std::sync::Arc;
use uo_codec::Limits;
use uo_drawingml::{Mask, PathSeg};
use uo_render::paint::PaintError;
use uo_render::paint::picture::{ChannelStore, colored_mask, dash};
use uo_render::view::{RenderView, ViewError};

/// A view over a layout's pages (see the module documentation).
pub struct UoRenderView {
    view: RenderView,
}

/// A result: a status and its bytes.
pub struct UoBuf {
    status: i32,
    data: Vec<u8>,
}

pub(crate) const OK: i32 = 0;
pub(crate) const NONE: i32 = 1;
pub(crate) const NULL_HANDLE: i32 = 11;

fn code(e: &ViewError) -> i32 {
    match e {
        ViewError::Decode(uo_codec::DecodeError::VersionMismatch { .. } | uo_codec::DecodeError::BadMagic) => 2,
        ViewError::Decode(uo_codec::DecodeError::Limit(_)) => 4,
        ViewError::Decode(_) => 3,
        ViewError::Paint(PaintError::Limit(_) | PaintError::Draw(uo_drawingml::Error::Limit(_))) => 4,
        ViewError::Paint(_) => 5,
        ViewError::NoPage | ViewError::NoItem => 6,
        ViewError::WrongItem => 7,
        ViewError::NoResource => 8,
        ViewError::BadFont => 9,
        ViewError::Encode(_) => 10,
    }
}

pub(crate) fn buf(status: i32, data: Vec<u8>) -> *mut UoBuf {
    Box::into_raw(Box::new(UoBuf { status, data }))
}

fn fail(e: &ViewError) -> *mut UoBuf {
    buf(code(e), Vec::new())
}

fn none() -> *mut UoBuf {
    buf(NONE, Vec::new())
}

/// Little-endian writer of the result layouts.
#[derive(Default)]
pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    pub(crate) fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn u32(&mut self, v: usize) -> &mut Self {
        self.0.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_le_bytes());
        self
    }

    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
}

/// Takes back an input buffer of `uo_alloc_bytes(len)` that JS filled completely.
///
/// # Safety
/// `ptr` must be null or come from one call to `uo_alloc_bytes(len)`, filled with `len` bytes and not
/// released or consumed yet.
pub(crate) unsafe fn take_bytes(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len
    // initialized bytes; it is taken back here exactly once.
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

/// # Safety
/// `view` must be null or a live handle.
unsafe fn view_ref<'a>(view: *const UoRenderView) -> Option<&'a RenderView> {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    unsafe { view.as_ref() }.map(|v| &v.view)
}

/// # Safety
/// `view` must be null or a live handle with no other reference in use.
unsafe fn view_mut<'a>(view: *mut UoRenderView) -> Option<&'a mut RenderView> {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    unsafe { view.as_mut() }.map(|v| &mut v.view)
}

// ───────────────────────────── handles ─────────────────────────────

/// A view with no pages, fed by [`uo_render_view_push_message`] (`generation` = the layout id the
/// encoder stamps on its messages).
#[unsafe(no_mangle)]
pub extern "C" fn uo_render_view_new(generation: u32) -> *mut UoRenderView {
    Box::into_raw(Box::new(UoRenderView { view: RenderView::from_messages(generation) }))
}

/// A view handle by value (the layout handle owns one; uo-bindings.md).
pub(crate) fn view_handle(view: RenderView) -> UoRenderView {
    UoRenderView { view }
}

/// The view inside a handle.
pub(crate) fn view_of(h: &UoRenderView) -> &RenderView {
    &h.view
}

/// Adopts a view built elsewhere in the module (the layout handle, a fixture).
pub fn adopt_view(view: RenderView) -> *mut UoRenderView {
    Box::into_raw(Box::new(UoRenderView { view }))
}

/// # Safety
/// `view` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_free(view: *mut UoRenderView) {
    if !view.is_null() {
        // SAFETY: the caller guarantees `view` came from this module and is released once.
        drop(unsafe { Box::from_raw(view) });
    }
}

/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_page_count(view: *const UoRenderView) -> usize {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    unsafe { view_ref(view) }.map_or(0, RenderView::page_count)
}

/// Decodes a page message (consumed) and appends the page: its index, or `-status`.
///
/// # Safety
/// `view` must be a live handle; `ptr`/`len` a buffer from `uo_alloc_bytes(len)` that JS filled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_push_message(view: *mut UoRenderView, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: the caller guarantees the buffer; it is consumed even when the view is null.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return -NULL_HANDLE };
    match v.push_message(&bytes, &Limits::default()) {
        Ok(i) => i32::try_from(i).unwrap_or(i32::MAX),
        Err(e) => -code(&e),
    }
}

/// Attaches the font file (consumed) of face `face`: a status.
///
/// # Safety
/// As [`uo_render_view_push_message`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_set_face_bytes(view: *mut UoRenderView, face: usize, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as in uo_render_view_push_message.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return NULL_HANDLE };
    v.set_face_bytes(face, Arc::from(bytes)).map_or_else(|e| code(&e), |()| OK)
}

/// Attaches the bytes (consumed) of image `image`; `natural_w`/`natural_h` (0 = unknown) are its
/// size in pixels, which the blip tiles need.
///
/// # Safety
/// As [`uo_render_view_push_message`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_set_image_bytes(
    view: *mut UoRenderView,
    image: usize,
    ptr: *mut u8,
    len: usize,
    natural_w: u32,
    natural_h: u32,
) -> i32 {
    // SAFETY: as in uo_render_view_push_message.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return NULL_HANDLE };
    let natural = if natural_w > 0 && natural_h > 0 { Some((natural_w, natural_h)) } else { None };
    v.set_image_bytes(image, Arc::from(bytes), natural).map_or_else(|e| code(&e), |()| OK)
}

/// Sets the producer's section (consumed) of a page, carried by `uo_render_encode_page`.
///
/// # Safety
/// As [`uo_render_view_push_message`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_set_app(view: *mut UoRenderView, page: usize, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as in uo_render_view_push_message.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return NULL_HANDLE };
    v.set_app(page, bytes).map_or_else(|e| code(&e), |()| OK)
}

/// Lowers (or raises) the pixel cap of one text-effects layer for this view: a phone-sized host sets
/// it under the 2.4e7 pixels the TS allowed (the layer peaks at about 37 bytes a pixel).
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_view_set_max_fx_px(view: *mut UoRenderView, px: f64) -> i32 {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return NULL_HANDLE };
    match uo_jsnum::conv::js_index(px, usize::MAX) {
        Some(n) => {
            v.limits.max_fx_px = u64::try_from(n).unwrap_or(u64::MAX);
            OK
        }
        None => 5,
    }
}

// ───────────────────────────── results ─────────────────────────────

/// # Safety
/// `b` must be null or a live result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_status(b: *const UoBuf) -> i32 {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(NULL_HANDLE, |b| b.status)
}

/// # Safety
/// `b` must be null or a live result; the pointer is valid until the result is released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_ptr(b: *const UoBuf) -> *const u8 {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(std::ptr::null(), |b| b.data.as_ptr())
}

/// # Safety
/// `b` must be null or a live result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_len(b: *const UoBuf) -> usize {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(0, |b| b.data.len())
}

/// # Safety
/// `b` must be null or a live result, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_free(b: *mut UoBuf) {
    if !b.is_null() {
        // SAFETY: the caller guarantees `b` came from this module and is released once.
        drop(unsafe { Box::from_raw(b) });
    }
}

// ───────────────────────────── the paint operations ─────────────────────────────

/// The message of page `page` (the resource deltas since the last one this view sent).
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_encode_page(view: *mut UoRenderView, page: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_mut(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut out = Vec::new();
    match v.encode_page(page, &mut out) {
        Ok(()) => buf(OK, out),
        Err(e) => fail(&e),
    }
}

/// `fxLayers(item, fx, ppt)` of a glyph run with text effects.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_fx_layers(view: *const UoRenderView, page: usize, item: usize, ppt: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.fx_layers(page, item, ppt) {
        Ok(Some(l)) => {
            let mut o = Out::default();
            o.f64(l.x0).f64(l.y0).u32(l.w).u32(l.h).bytes(&l.data);
            buf(OK, o.0)
        }
        Ok(None) => none(),
        Err(e) => fail(&e),
    }
}

/// `patternRaster(pattern, x, y, w, h, 15)` of a shape.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_pattern(view: *const UoRenderView, page: usize, item: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.pattern_raster(page, item) {
        Ok(Some(p)) => {
            let mut o = Out::default();
            o.f64(p.x).f64(p.y).f64(p.cells_w).f64(p.cells_h).u32(p.w).u32(p.h).bytes(&p.data);
            buf(OK, o.0)
        }
        Ok(None) => none(),
        Err(e) => fail(&e),
    }
}

/// `rectGradientRaster(grad, w / 20, h / 20, 2)` of a shape.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_rect_gradient(view: *const UoRenderView, page: usize, item: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.rect_gradient(page, item) {
        Ok(r) => {
            let mut o = Out::default();
            o.u32(r.w).u32(r.h).bytes(&r.data);
            buf(OK, o.0)
        }
        Err(e) => fail(&e),
    }
}

/// `blipTiles(blip, x, y, w, h, 1 / 635, natural)` of a shape.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_blip_tiles(view: *const UoRenderView, page: usize, item: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.blip_tiles(page, item) {
        Ok(tiles) => {
            let mut o = Out::default();
            o.u32(tiles.len());
            for t in &tiles {
                o.f64(t.x).f64(t.y).f64(t.w).f64(t.h);
            }
            buf(OK, o.0)
        }
        Err(e) => fail(&e),
    }
}

/// `pageFillTileTw(fill, devicePxTw, devicePxTw)` of the page background effect.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_page_fill_tile_tw(view: *const UoRenderView, page: usize, device_px_tw: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.page_fill_tile_tw(page, device_px_tw) {
        Ok(Some((w, h))) => {
            let mut o = Out::default();
            o.f64(w).f64(h);
            buf(OK, o.0)
        }
        Ok(None) => none(),
        Err(e) => fail(&e),
    }
}

/// The source tile of the page background effect (gradient or 8 x 8 pattern).
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_page_fill_raster(view: *const UoRenderView, page: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.page_fill_raster(page) {
        Ok(Some(r)) => {
            let mut o = Out::default();
            o.u32(r.w).u32(r.h).bytes(&r.data);
            buf(OK, o.0)
        }
        Ok(None) => none(),
        Err(e) => fail(&e),
    }
}

/// `bounds(look, w, h)` of an image with a picture look or a shape with effects.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_picture_bounds(view: *const UoRenderView, page: usize, item: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.picture_bounds(page, item) {
        Ok(b) => {
            let mut o = Out::default();
            o.f64(b.x0).f64(b.y0).f64(b.x1).f64(b.y1);
            buf(OK, o.0)
        }
        Err(e) => fail(&e),
    }
}

/// `shapeSubs(look, w, h, flipH, flipV)` of an image with a picture look.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_picture_subs(view: *const UoRenderView, page: usize, item: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view_ref(view) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.picture_subs(page, item) {
        Ok(subs) => {
            let mut o = Out::default();
            o.u32(subs.len());
            for sub in subs.iter() {
                o.u32(sub.len());
                for s in sub {
                    match *s {
                        PathSeg::M { x, y } => {
                            o.u8(b'M').f64(x).f64(y);
                        }
                        PathSeg::L { x, y } => {
                            o.u8(b'L').f64(x).f64(y);
                        }
                        PathSeg::C { x1, y1, x2, y2, x, y } => {
                            o.u8(b'C').f64(x1).f64(y1).f64(x2).f64(y2).f64(x).f64(y);
                        }
                        PathSeg::Q { x1, y1, x, y } => {
                            o.u8(b'Q').f64(x1).f64(y1).f64(x).f64(y);
                        }
                        PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
                            o.u8(b'E').f64(cx).f64(cy).f64(rx).f64(ry).f64(a0).f64(a1).u8(u8::from(ccw));
                        }
                        PathSeg::Z => {
                            o.u8(b'Z');
                        }
                    }
                }
            }
            buf(OK, o.0)
        }
        Err(e) => fail(&e),
    }
}

/// `maskToCanvas(mask, hex, alpha)` of the canvas (`Uint8ClampedArray` channels): `mask` is
/// `w * h` little-endian `f32` (consumed), `hex` is UTF-16LE code units (consumed).
///
/// # Safety
/// `mask_ptr`/`mask_len` and `hex_ptr`/`hex_len` must be buffers from `uo_alloc_bytes` that JS
/// filled completely (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_colored_mask(
    mask_ptr: *mut u8,
    mask_len: usize,
    w: u32,
    h: u32,
    hex_ptr: *mut u8,
    hex_len: usize,
    alpha: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees both buffers; they are consumed here.
    let (mask_bytes, hex_bytes) = unsafe { (take_bytes(mask_ptr, mask_len), take_bytes(hex_ptr, hex_len)) };
    let a: Vec<f32> = mask_bytes.chunks_exact(4).filter_map(|c| <[u8; 4]>::try_from(c).ok().map(f32::from_le_bytes)).collect();
    let hex: Vec<u16> = hex_bytes.chunks_exact(2).filter_map(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes)).collect();
    match Mask::from_vec(w, h, a) {
        Ok(m) => buf(OK, colored_mask(&m, &hex, alpha, ChannelStore::Clamp)),
        Err(_) => buf(5, Vec::new()),
    }
}

/// `DASH[name]` of the picture outline (UTF-16LE name, consumed): the multiples of the thickness.
///
/// # Safety
/// `name_ptr`/`name_len` must be a buffer from `uo_alloc_bytes` that JS filled completely (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_render_dash(name_ptr: *mut u8, name_len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(name_ptr, name_len) };
    let name: Vec<u16> = bytes.chunks_exact(2).filter_map(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes)).collect();
    match dash(&name) {
        Some(d) => {
            let mut o = Out::default();
            o.u32(d.len());
            for v in d {
                o.f64(*v);
            }
            buf(OK, o.0)
        }
        None => none(),
    }
}
