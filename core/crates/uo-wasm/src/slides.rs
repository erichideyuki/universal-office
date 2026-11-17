//! The Slides viewer module for the browser (frontier B7, first part: static rendering; record
//! `core/docs/modules/uo-bindings.md` "Slides viewer (B7)"): `uo-slides.wasm`, the build with the feature `slides`.
//!
//! Handles (each a `Box` released by its `*_free`; JS: `free()` and a `FinalizationRegistry`, `js/slides.mjs`):
//!
//! | Handle | Created by | Owns |
//! |---|---|---|
//! | [`UoSlidesDeck`] | `uo_slides_open` | the [`Deck`] (package, slide cache), the shell's [`MeasureTable`], the pending measurement requests |
//! | [`UoSlidesView`] | `uo_slides_view_new` | one generation of the list stream: `SlidesResources`, `EmitCaches`, `SlidesEncoder` |
//! | [`UoSlidesShow`] | `uo_slides_show_new` | a click slide show (`uo_pptx::SlideShow`, state in Rust, every time is an argument) |
//! | [`UoSlidesTimeline`] | `uo_slides_timeline_new` | the automatic-playback timeline (`PlaybackTimeline`) |
//!
//! A view also holds the two STAGES of a playback (B4b, `uo_pptx::stage`): the slide on stage and the origin of the running
//! transition, each a list with slots, its slot template and the `AnimCaches` of the frames (`uo_slides_stage_show`,
//! `uo_slides_stage_timeline`; `uo_slides_stage_clear` empties them).
//! | `UoBuf` (`buf.rs`) | every query | status and bytes |
//!
//! Input bytes come from `uo_alloc_bytes` and are consumed by the call that takes them. Nothing here panics on its input.
//!
//! # The two-phase measure loop (decision T1)
//! `uo_slides_emit` returns status 12 ([`NEED`]) when the layout lacks advances; the buffer is the request list (layout
//! below) and the deck keeps it as *pending*. JS measures with the canvas (`measureText`, the font strings of the TS) and
//! calls `uo_slides_answer` with one `f64` group per request in the same order, then emits again (at most
//! `uo_dml_render::text::MAX_ROUNDS` rounds). The table survives between slides (and is emptied by
//! `uo_slides_set_generation` when a `FontFace` loaded).
//!
//! # Status codes
//! 0 ok · 1 none · 3 the deck (or the slide) could not be read · 4 a limit · 5 invalid argument · 6 no such slide, part or
//! media · 10 the list did not encode · 11 null handle · 12 measurements needed · 14 not a OOXML package (a `.ppt`:
//! `uo_slides_ppt_convert` of the lazily loaded module turns it into a `.pptx` first).
//!
//! # Result layouts (little endian)
//! - `uo_slides_emit` ok: `flags u8` (bit 0: the caches overflowed, start a new generation after painting this one; bit 1:
//!   the slide has chart frames the Rust list does not paint), `n_charts u32`, per chart `shape u32, x f64, y f64, w f64,
//!   h f64`, then the `SlidesList` message (`uo_render::slides`).
//! - the requests (status 12): `n u32`, per request a tag `u8` and `1` CharAt1000 (`face`, `ch u32`), `2` KernAt1000
//!   (`face`, `text`), `3` Width and `4` Metrics (`face`, `size f64`, `kerning u8` 0 current / 1 none / 2 normal,
//!   `letter_spacing f64`, `text`), `5` FontAvailable (`family`), `6` ImageSize (`media u32`, `part`), `7` MetaText (`font`, `text`: a RAW CSS font string, the
//!   metafile players' probes), `8` MetaDecode (`key`, `len u32` and the compressed bitmap bytes); `face` is
//!   `family str, bold u8, italic u8`, `str` is `len u32` and UTF-8.
//! - the answers (input of `uo_slides_answer`): per request in order `f64`s: CharAt1000 1, KernAt1000 2 (`w0`, `w1`),
//!   Width 1, Metrics 7 (`width`, `actualLeft`, `actualRight`, `actualAscent`, `actualDescent`, `fontAscent`,
//!   `fontDescent`; NaN = the browser has no `fontBoundingBox*`), FontAvailable 1 (0/1), ImageSize 2 (`w`, `h`), MetaText 3 (`width`,
//!   `fontBoundingBoxAscent`, `fontBoundingBoxDescent`), MetaDecode 2 (`w`, `h` of the decoded bitmap; NaN, NaN = it cannot be decoded).
//! - `uo_slides_size`: `w f64, h f64` (EMU), `n u32` slides. `uo_slides_notes`: UTF-8 text.
//! - `uo_slides_show_frame`: `index i32` (-1 = the end screen), `step u32`, `steps u32`, `ended u8`, `anim_ms f64`,
//!   `progress f64`, `n_effects u32`, `has_from u8` then `index i32, anim_ms f64, key u32, n_effects u32`, then the
//!   transition (`present u8`, then `kind str, ns str, n u32, n x (str, str), dur_ms f64, adv_tm_ms f64` (NaN = none),
//!   `adv_click u8`).
//! - `uo_slides_stage_show` / `uo_slides_stage_timeline` ok: `flags u8` (bit 0: the caches overflowed; bit 1: nothing on
//!   stage (the end screen, or no origin): nothing follows; bit 2: a new list follows), `anim_ms f64` (the frame's animation
//!   time), then with bit 2 `len u32` and the `SlidesList` message (the stage's list, with slots), then the `SlotFrame` message
//!   (`uo_render::slides`, schema `slots.uos`: one value per slot of the stage's list; each replacement list is a `SlidesList`
//!   message of the same stream, to be decoded after the stage's list). Status 12: the measurement requests, as `uo_slides_emit`.
//! - `uo_slides_timeline_info`: `duration f64, tick f64, media_clock f64, n u32`, per slide `index u32, start f64, end f64,
//!   anim_start f64, transition_ms f64`; `uo_slides_timeline_state`: `pos i32, index i32, from_index i32, progress f64,
//!   anim_ms f64, ended u8, n_effects u32`, then the transition of the slide.

use std::cell::RefCell;
use std::sync::Arc;

use uo_dml_render::emit::EmitCaches;
use uo_dml_render::text::{FaceKey, Kerning, MAX_ROUNDS, MeasureReq, MeasureTable, StringKey, TextMetrics};
use uo_dml_render::{MediaId, PartSource};
use uo_pptx::anim_engine::AnimCaches;
use uo_pptx::media::MediaClock;
use uo_pptx::playback::{PlaybackOptions, PlaybackSlide, PlaybackTimeline, playback_state_at};
use uo_pptx::show::{SlideShow, SlideShowAction, SlideShowFrame, SlideShowOptions};
use uo_pptx::stage::{Stage, anim_plan, emit_stage, has_media, stage_frame, wire_values};
use uo_pptx::timing::{TimingModel, TransitionInfo};
use uo_pptx::{Deck, EmitFail, Limits, PptxError, View, emit_slide};
use uo_render::slides::{FrameEncoder, Replacement, SlidesEncoder, SlidesResources, SlotFrame};

use crate::buf::{NULL_HANDLE, OK, Out, UoBuf, buf, take_bytes};

/// The status of an emission that needs the shell's measurements.
pub const NEED: i32 = 12;
const DECK: i32 = 3;
const LIMIT: i32 = 4;
const BAD_ARG: i32 = 5;
const NO_ITEM: i32 = 6;
const ENCODE: i32 = 10;
const NOT_OOXML: i32 = 14;

/// The deck and everything the shell's answers are kept in (see the module documentation).
pub struct UoSlidesDeck {
    deck: Deck,
    table: MeasureTable,
    pending: Vec<MeasureReq>,
    /// presentation index of each viewer slide: the slides that have a part (the TS viewer skips the others)
    map: Vec<u32>,
    /// slide width and height in EMU
    size: (f64, f64),
}

/// One generation of the list stream of a view.
pub struct UoSlidesView {
    res: SlidesResources,
    caches: EmitCaches,
    enc: SlidesEncoder,
    /// B4b: the slide on stage (0) and the transition origin (1)
    stages: [Option<Stage>; 2],
    /// B4b: the caches of the slot evaluation (formulas, motion paths), capped
    anim: AnimCaches,
    /// B4b: the frame stream (`SlotFrame` messages) and its frame counter
    frames: FrameEncoder,
    frame_no: u32,
}

/// The click slide show.
pub struct UoSlidesShow {
    show: SlideShow,
    /// viewer index of the slide on stage when the last frame was taken (for `n_effects`)
    models: Vec<Option<Arc<TimingModel>>>,
}

/// The playback timeline.
pub struct UoSlidesTimeline {
    tl: PlaybackTimeline,
    /// the timeline's slides shared with the stages (B4b)
    slides: Vec<Arc<PlaybackSlide>>,
}

thread_local! {
    /// The status and message of the last failed `uo_slides_open`.
    static LAST: RefCell<(i32, String)> = const { RefCell::new((0, String::new())) };
}

fn set_last(code: i32, message: String) {
    LAST.with(|l| *l.borrow_mut() = (code, message));
}

fn error_code(e: &PptxError) -> i32 {
    match e {
        PptxError::Limit(_) => LIMIT,
        PptxError::NoSuchSlide(_) => NO_ITEM,
        PptxError::NotAPackage | PptxError::NoPresentation | PptxError::MissingPart(_) | PptxError::PptFormat(_) | PptxError::Read(_) => {
            DECK
        }
    }
}

// ───────────────────────────── the deck ─────────────────────────────

/// Opens a `.pptx` (OOXML). Null on failure; `uo_slides_error` has the status and the message.
///
/// # Safety
/// `ptr`/`len` must come from one call to `uo_alloc_bytes(len)`, filled with `len` bytes and not consumed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_open(ptr: *mut u8, len: usize) -> *mut UoSlidesDeck {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let bytes = unsafe { take_bytes(ptr, len) };
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        set_last(NOT_OOXML, "a .ppt (Compound File): convert it with the ppt module first".to_owned());
        return std::ptr::null_mut();
    }
    let mut deck = match Deck::open(&bytes, Limits::default()) {
        Ok(d) => d,
        Err(e) => {
            set_last(error_code(&e), e.to_string());
            return std::ptr::null_mut();
        }
    };
    drop(bytes);
    let (size, map) = match deck.presentation() {
        Ok(p) => {
            let map = p.slides.iter().enumerate().filter(|(_, r)| r.target.is_some()).filter_map(|(i, _)| u32::try_from(i).ok()).collect();
            ((p.slide_w, p.slide_h), map)
        }
        Err(e) => {
            set_last(error_code(&e), e.to_string());
            return std::ptr::null_mut();
        }
    };
    Box::into_raw(Box::new(UoSlidesDeck { deck, table: MeasureTable::default(), pending: Vec::new(), map, size }))
}

/// The status of the last failed open (and clears it): `code i32`, then the message in UTF-8.
#[unsafe(no_mangle)]
pub extern "C" fn uo_slides_error() -> *mut UoBuf {
    let (code, msg) = LAST.with(|l| std::mem::take(&mut *l.borrow_mut()));
    buf(code, msg.into_bytes())
}

/// # Safety
/// `deck` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_deck_free(deck: *mut UoSlidesDeck) {
    if !deck.is_null() {
        // SAFETY: the caller guarantees `deck` came from uo_slides_open and is released once.
        drop(unsafe { Box::from_raw(deck) });
    }
}

/// # Safety
/// `deck` must be null or a live handle.
unsafe fn deck_mut<'a>(deck: *mut UoSlidesDeck) -> Option<&'a mut UoSlidesDeck> {
    // SAFETY: the caller guarantees `deck` is null or a live handle with no other reference in use.
    unsafe { deck.as_mut() }
}

/// Number of slides the viewer shows.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_count(deck: *const UoSlidesDeck) -> usize {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    unsafe { deck.as_ref() }.map_or(0, |d| d.map.len())
}

/// `w f64, h f64` (EMU) and the slide count.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_size(deck: *const UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut o = Out::default();
    o.f64(d.size.0).f64(d.size.1).u32(d.map.len());
    buf(OK, o.0)
}

/// The speaker notes of slide `index` (UTF-8; empty when there are none).
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_notes(deck: *mut UoSlidesDeck, index: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(&pi) = d.map.get(index) else { return buf(NO_ITEM, Vec::new()) };
    match d.deck.slide_notes(pi) {
        Ok(t) => buf(OK, t.into_bytes()),
        Err(e) => buf(error_code(&e), Vec::new()),
    }
}

/// The bytes of a package part (the media of the list are loaded by their part name).
///
/// # Safety
/// `deck` must be null or a live handle; `ptr`/`len` a consumed `uo_alloc_bytes` buffer with the UTF-8 part name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_part(deck: *const UoSlidesDeck, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let name = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(name) = std::str::from_utf8(&name) else { return buf(BAD_ARG, Vec::new()) };
    match d.deck.parts().bytes(name) {
        Some(b) => buf(OK, b.to_vec()),
        None => buf(NO_ITEM, Vec::new()),
    }
}

/// The canvas capabilities (bit 0 `fontKerning`, 1 `letterSpacing`, 2 `Intl.Segmenter`, 3 `fontBoundingBox*`): they
/// change the layout, so they are part of the table; changing them empties it.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_set_caps(deck: *mut UoSlidesDeck, bits: u32) -> i32 {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return NULL_HANDLE };
    let caps = uo_dml_render::text::Capabilities {
        font_kerning: bits & 1 != 0,
        letter_spacing: bits & 2 != 0,
        segmenter: bits & 4 != 0,
        font_bounding_box: bits & 8 != 0,
    };
    if d.table.caps != caps {
        d.table.clear();
        d.table.caps = caps;
    }
    OK
}

/// A new font generation (a `FontFace` loaded): every advance may change, the table empties.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_set_generation(deck: *mut UoSlidesDeck, generation: u32) -> i32 {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return NULL_HANDLE };
    d.table.set_generation(generation);
    d.pending.clear();
    OK
}

/// Records the decoded size of a picture (media id of an `ImageSize` request): the shell calls it when an image that was
/// not ready (answered 0 x 0) finished decoding.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_set_image_size(deck: *mut UoSlidesDeck, media: u32, w: f64, h: f64) -> i32 {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return NULL_HANDLE };
    d.table.set_image_size(MediaId(media), w, h);
    OK
}

/// Frees what a long session accumulated that is not needed to paint again: the deck's XML and slide caches.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_clear_caches(deck: *mut UoSlidesDeck) -> i32 {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return NULL_HANDLE };
    d.deck.clear_caches();
    OK
}

// ───────────────────────────── the view and the emission ─────────────────────────────

/// A new generation of the list stream (`generation` stamps the messages).
#[unsafe(no_mangle)]
pub extern "C" fn uo_slides_view_new(generation: u32) -> *mut UoSlidesView {
    Box::into_raw(Box::new(UoSlidesView {
        res: SlidesResources::new(),
        caches: EmitCaches::new(),
        enc: SlidesEncoder::new(generation),
        stages: [None, None],
        anim: AnimCaches::default(),
        frames: FrameEncoder::new(generation),
        frame_no: 0,
    }))
}

/// # Safety
/// `view` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_view_free(view: *mut UoSlidesView) {
    if !view.is_null() {
        // SAFETY: the caller guarantees `view` came from uo_slides_view_new and is released once.
        drop(unsafe { Box::from_raw(view) });
    }
}

/// The RGBA bytes of a generated bitmap of the view's resources (`Bitmap.data` of the list's tables).
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_view_bitmap(view: *const UoSlidesView, handle: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    match v.res.bitmap_bytes(handle) {
        Some(b) => buf(OK, b.to_vec()),
        None => buf(NO_ITEM, Vec::new()),
    }
}

/// `paintSlideTo` of viewer slide `index` at `scale` px per EMU and origin `(ox, oy)` px: the encoded list (status 0), the
/// measurement requests (status 12), or an error status (module documentation).
///
/// # Safety
/// `deck` and `view` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_emit(
    deck: *mut UoSlidesDeck,
    view: *mut UoSlidesView,
    index: usize,
    scale: f64,
    ox: f64,
    oy: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees both handles are null or live, with no other reference in use.
    let (Some(d), Some(v)) = (unsafe { (deck_mut(deck), view.as_mut()) }) else { return buf(NULL_HANDLE, Vec::new()) };
    if !(scale.is_finite() && scale > 0.0 && ox.is_finite() && oy.is_finite()) {
        return buf(BAD_ARG, Vec::new());
    }
    let Some(&pi) = d.map.get(index) else { return buf(NO_ITEM, Vec::new()) };
    match emit_slide(&mut d.deck, pi, View { scale, ox, oy }, &d.table, &mut v.res, &mut v.caches) {
        Ok(em) => {
            let mut o = Out::default();
            let flags = u8::from(v.caches.overflowed) | (u8::from(!em.chart_frames.is_empty()) << 1);
            o.u8(flags).u32(em.chart_frames.len());
            for c in &em.chart_frames {
                o.u32(usize::try_from(c.shape).unwrap_or(usize::MAX)).f64(c.rect[0]).f64(c.rect[1]).f64(c.rect[2]).f64(c.rect[3]);
            }
            match v.enc.encode(&em.list, &v.res, &mut o.0) {
                Ok(()) => buf(OK, o.0),
                Err(_) => buf(ENCODE, Vec::new()),
            }
        }
        Err(EmitFail::Need(n)) => {
            let mut o = Out::default();
            put_requests(&mut o, d, &n.0);
            d.pending = n.0;
            buf(NEED, o.0)
        }
        Err(EmitFail::Deck(e)) => buf(error_code(&e), Vec::new()),
    }
}

// ───────────────────────────── the stage (B4b) ─────────────────────────────

/// The stage `which` of `v` for playback slide `ps` at `anim_ms`: the list (emitted again when the slide, the viewport, the
/// media flag or the hooks' structure changed) and the frame's values.
#[allow(clippy::too_many_arguments)]
fn stage_out(
    d: &mut UoSlidesDeck,
    v: &mut UoSlidesView,
    ps: Option<Arc<PlaybackSlide>>,
    anim_ms: f64,
    which: usize,
    view: View,
    media: bool,
    clock: &MediaClock,
) -> *mut UoBuf {
    let Some(ps) = ps else {
        if let Some(s) = v.stages.get_mut(which) {
            *s = None;
        }
        let mut o = Out::default();
        o.u8(2).f64(anim_ms);
        return buf(OK, o.0);
    };
    let Some(&pi) = d.map.get(usize::try_from(ps.index).unwrap_or(usize::MAX)) else { return buf(NO_ITEM, Vec::new()) };
    let shapes = match d.deck.slide(pi) {
        Ok(s) => s,
        Err(e) => return buf(error_code(&e), Vec::new()),
    };
    let fresh = match v.stages.get(which).and_then(Option::as_ref) {
        Some(s) => {
            s.index != pi
                || s.view != view
                || s.media != (media && has_media(&shapes.shapes))
                || s.slide.effects.len() != ps.effects.len()
                || s.plan != anim_plan(&ps, &shapes.shapes)
        }
        None => true,
    };
    let mut o = Out::default();
    // a new stage is kept (and its list encoded) only once its first frame is complete: a pass that needs measurements
    // delivers nothing, so the stream's tables advance only with what the shell decodes
    let new_stage = if fresh {
        match emit_stage(&mut d.deck, pi, view, &d.table, &mut v.res, &mut v.caches, ps, media) {
            Ok(st) => Some(st),
            Err(EmitFail::Need(n)) => return need_out(d, n.0),
            Err(EmitFail::Deck(e)) => return buf(error_code(&e), Vec::new()),
        }
    } else {
        if let Some(Some(s)) = v.stages.get_mut(which) {
            // the same structure: the newer placement (fired steps) drives the values
            s.slide = ps;
        }
        None
    };
    let Some(stage) = new_stage.as_ref().or_else(|| v.stages.get(which).and_then(Option::as_ref)) else { return buf(NO_ITEM, Vec::new()) };
    let frame = match stage_frame(&mut d.deck, stage, &d.table, &mut v.res, &mut v.caches, anim_ms, clock, &mut v.anim, new_stage.is_some())
    {
        Ok(f) => f,
        Err(EmitFail::Need(n)) => return need_out(d, n.0),
        Err(EmitFail::Deck(e)) => return buf(error_code(&e), Vec::new()),
    };
    let mut list_bytes: Option<Vec<u8>> = None;
    if let Some(st) = &new_stage {
        let mut b = Vec::new();
        if v.enc.encode(&st.emission.list, &v.res, &mut b).is_err() {
            return buf(ENCODE, Vec::new());
        }
        list_bytes = Some(b);
    }
    let mut replacements = Vec::with_capacity(frame.replacements.len());
    for r in &frame.replacements {
        let mut lb = Vec::new();
        if v.enc.encode(&r.emission.list, &v.res, &mut lb).is_err() {
            return buf(ENCODE, Vec::new());
        }
        replacements.push(Replacement { shape: r.shape, list: lb, values: wire_values(&r.template, &r.table, &mut v.frames) });
    }
    v.frame_no = v.frame_no.wrapping_add(1);
    let msg = SlotFrame { frame: v.frame_no, values: wire_values(&stage.template, &frame.table, &mut v.frames), replacements };
    if let Some(st) = new_stage
        && let Some(s) = v.stages.get_mut(which)
    {
        *s = Some(st);
    }
    let flags = u8::from(v.caches.overflowed) | (u8::from(list_bytes.is_some()) << 2);
    o.u8(flags).f64(anim_ms);
    if let Some(b) = &list_bytes {
        o.u32(b.len()).bytes(b);
    }
    if v.frames.encode(&msg, &mut o.0).is_err() {
        return buf(ENCODE, Vec::new());
    }
    buf(OK, o.0)
}

fn need_out(d: &mut UoSlidesDeck, reqs: Vec<MeasureReq>) -> *mut UoBuf {
    let mut o = Out::default();
    put_requests(&mut o, d, &reqs);
    d.pending = reqs;
    buf(NEED, o.0)
}

fn view_of(scale: f64, ox: f64, oy: f64) -> Option<View> {
    (scale.is_finite() && scale > 0.0 && ox.is_finite() && oy.is_finite()).then_some(View { scale, ox, oy })
}

/// The stage of a click slide show at `now`: `which` 0 = the slide on stage at its `anim_ms`, 1 = the origin of the running
/// transition at its frozen `anim_ms`. `media` != 0 wires the media frames (`hasMedia`); `latency`/`clock_scale` are the
/// media clock (NaN = default). Layout in the module documentation.
///
/// # Safety
/// `deck`, `view` and `show` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_stage_show(
    deck: *mut UoSlidesDeck,
    view: *mut UoSlidesView,
    show: *const UoSlidesShow,
    now: f64,
    which: u32,
    scale: f64,
    ox: f64,
    oy: f64,
    media: u32,
    latency: f64,
    clock_scale: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live, with no other reference in use.
    let (Some(d), Some(v), Some(s)) = (unsafe { (deck_mut(deck), view.as_mut(), show.as_ref()) }) else {
        return buf(NULL_HANDLE, Vec::new());
    };
    let (Some(vw), Ok(w @ 0..=1)) = (view_of(scale, ox, oy), usize::try_from(which)) else { return buf(BAD_ARG, Vec::new()) };
    let f = s.show.frame(now);
    let (ps, anim_ms) = if w == 0 { (f.slide, f.anim_ms) } else { f.from.map_or((None, 0.0), |o| (Some(o.slide), o.anim_ms)) };
    let clock = MediaClock { latency_ms: opt_f(latency), scale: opt_f(clock_scale) };
    stage_out(d, v, ps, anim_ms, w, vw, media != 0, &clock)
}

/// The stage of a playback timeline at `t`: `which` 0 = the slide on stage, 1 = the transition origin (in its final state,
/// `anim_ms` infinity, as the viewer paints it). Otherwise as [`uo_slides_stage_show`].
///
/// # Safety
/// `deck`, `view` and `tl` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_stage_timeline(
    deck: *mut UoSlidesDeck,
    view: *mut UoSlidesView,
    tl: *const UoSlidesTimeline,
    t: f64,
    which: u32,
    scale: f64,
    ox: f64,
    oy: f64,
    media: u32,
    latency: f64,
    clock_scale: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live, with no other reference in use.
    let (Some(d), Some(v), Some(tl)) = (unsafe { (deck_mut(deck), view.as_mut(), tl.as_ref()) }) else {
        return buf(NULL_HANDLE, Vec::new());
    };
    let (Some(vw), Ok(w @ 0..=1)) = (view_of(scale, ox, oy), usize::try_from(which)) else { return buf(BAD_ARG, Vec::new()) };
    let st = playback_state_at(&tl.tl, t);
    let pos = if w == 0 { st.pos } else { st.from };
    let ps = pos.and_then(|p| tl.slides.get(p)).cloned();
    let anim_ms = if w == 0 { st.anim_ms } else { f64::INFINITY };
    let clock = MediaClock { latency_ms: opt_f(latency), scale: opt_f(clock_scale) };
    stage_out(d, v, ps, anim_ms, w, vw, media != 0, &clock)
}

/// Drops both stages of `view` and empties its slot-evaluation caches (memory gate M1; a new stage is emitted on the next
/// frame).
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_stage_clear(view: *mut UoSlidesView) -> i32 {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view.as_mut() }) else { return NULL_HANDLE };
    v.stages = [None, None];
    v.anim.clear();
    OK
}

fn str_out(o: &mut Out, s: &str) {
    o.u32(s.len()).bytes(s.as_bytes());
}

fn face_out(o: &mut Out, f: &FaceKey) {
    str_out(o, &f.family);
    o.u8(u8::from(f.bold)).u8(u8::from(f.italic));
}

fn key_out(o: &mut Out, k: &StringKey) {
    face_out(o, &k.face);
    o.f64(k.size_px.value()).u8(match k.kerning {
        Kerning::Current => 0,
        Kerning::None => 1,
        Kerning::Normal => 2,
    });
    o.f64(k.letter_spacing_px.value());
    str_out(o, &k.text);
}

fn put_requests(o: &mut Out, d: &UoSlidesDeck, reqs: &[MeasureReq]) {
    o.u32(reqs.len());
    for r in reqs {
        match r {
            MeasureReq::CharAt1000 { face, ch } => {
                o.u8(1);
                face_out(o, face);
                o.u32(usize::try_from(u32::from(*ch)).unwrap_or(0));
            }
            MeasureReq::KernAt1000 { face, text } => {
                o.u8(2);
                face_out(o, face);
                str_out(o, text);
            }
            MeasureReq::Width(k) => {
                o.u8(3);
                key_out(o, k);
            }
            MeasureReq::Metrics(k) => {
                o.u8(4);
                key_out(o, k);
            }
            MeasureReq::FontAvailable { family } => {
                o.u8(5);
                str_out(o, family);
            }
            MeasureReq::MetaText { font, text } => {
                o.u8(7);
                str_out(o, font);
                str_out(o, text);
            }
            MeasureReq::MetaDecode { key, data } => {
                o.u8(8);
                str_out(o, key);
                o.u32(data.len()).bytes(data);
            }
            MeasureReq::ImageSize { media } => {
                o.u8(6);
                o.u32(usize::try_from(media.0).unwrap_or(0));
                let part = d.deck.source().media_entry(*media).map_or_else(|| Box::<str>::from(""), |(p, _)| p);
                str_out(o, &part);
            }
        }
    }
}

/// The shell's answers to the pending requests of the last emission (layout in the module documentation).
///
/// # Safety
/// `deck` must be null or a live handle; `ptr`/`len` a consumed `uo_alloc_bytes` buffer of `f64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_answer(deck: *mut UoSlidesDeck, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return NULL_HANDLE };
    let nums: Vec<f64> = bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect();
    let pending = std::mem::take(&mut d.pending);
    let mut at = 0usize;
    let mut next = |n: usize| -> Option<&[f64]> {
        let s = nums.get(at..at.checked_add(n)?)?;
        at += n;
        Some(s)
    };
    for r in &pending {
        let ok = match r {
            MeasureReq::CharAt1000 { face, ch } => next(1).map(|a| d.table.set_char_at_1000(face, *ch, a[0])).is_some(),
            MeasureReq::KernAt1000 { face, text } => next(2).map(|a| d.table.set_kern_at_1000(face, text, a[0], a[1])).is_some(),
            MeasureReq::Width(k) => next(1).map(|a| d.table.set_width(k, a[0])).is_some(),
            MeasureReq::Metrics(k) => next(7)
                .map(|a| {
                    let opt = |v: f64| (!v.is_nan()).then_some(v);
                    d.table.set_metrics(
                        k,
                        TextMetrics {
                            width: a[0],
                            actual_left: a[1],
                            actual_right: a[2],
                            actual_ascent: a[3],
                            actual_descent: a[4],
                            font_ascent: opt(a[5]),
                            font_descent: opt(a[6]),
                        },
                    );
                })
                .is_some(),
            MeasureReq::FontAvailable { family } => next(1).map(|a| d.table.set_font_available(family, a[0] != 0.0)).is_some(),
            MeasureReq::ImageSize { media } => next(2).map(|a| d.table.set_image_size(*media, a[0], a[1])).is_some(),
            MeasureReq::MetaText { font, text } => next(3).map(|a| d.table.set_meta_text(font, text, a[0], a[1], a[2])).is_some(),
            MeasureReq::MetaDecode { key, .. } => {
                next(2).map(|a| d.table.set_meta_decoded(key, (!a[0].is_nan() && !a[1].is_nan()).then_some((a[0], a[1])))).is_some()
            }
        };
        if !ok {
            return BAD_ARG;
        }
    }
    OK
}

/// The rounds the host may take before giving up (`MAX_ROUNDS` of the protocol).
#[unsafe(no_mangle)]
pub extern "C" fn uo_slides_max_rounds() -> u32 {
    MAX_ROUNDS
}

// ───────────────────────────── timing and playback ─────────────────────────────

fn models_of(d: &mut UoSlidesDeck) -> Vec<Option<Arc<TimingModel>>> {
    let all = d.deck.timing_models();
    d.map.iter().map(|&i| all.get(usize::try_from(i).unwrap_or(usize::MAX)).cloned().flatten()).collect()
}

fn opt_f(v: f64) -> Option<f64> {
    (!v.is_nan()).then_some(v)
}

fn opt_b(v: i32) -> Option<bool> {
    (v >= 0).then_some(v != 0)
}

fn transition_out(o: &mut Out, t: Option<&TransitionInfo>) {
    let Some(t) = t else {
        o.u8(0);
        return;
    };
    o.u8(1);
    str_out(o, &t.kind);
    str_out(o, &t.ns);
    o.u32(t.attrs.len());
    for (k, v) in &t.attrs {
        str_out(o, k);
        str_out(o, v);
    }
    o.f64(t.dur_ms).f64(t.adv_tm_ms.unwrap_or(f64::NAN)).u8(u8::from(t.adv_click));
}

/// A click slide show over the deck's timing models. `from` is a viewer slide index (or -1 for the first VISIBLE slide),
/// `use_timings`/`loop_`/`animations` are -1 (default), 0 or 1, `range_lo`/`range_hi` the `<p:sldRg>` (both -1: none; a slide
/// outside it is hidden).
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_new(
    deck: *mut UoSlidesDeck,
    from: f64,
    now: f64,
    use_timings: i32,
    loop_: i32,
    animations: i32,
    range_lo: i32,
    range_hi: i32,
) -> *mut UoSlidesShow {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return std::ptr::null_mut() };
    let mut models = models_of(d);
    if range_lo >= 0 && range_hi >= range_lo {
        for (i, m) in models.iter_mut().enumerate() {
            let inside = i64::try_from(i).is_ok_and(|i| i >= i64::from(range_lo) && i <= i64::from(range_hi));
            if !inside {
                let mut hidden = m.as_deref().cloned().unwrap_or_default();
                hidden.hidden = true;
                *m = Some(Arc::new(hidden));
            }
        }
    }
    let start = if from < 0.0 { SlideShow::first_visible(&models) as f64 } else { from };
    let opts = SlideShowOptions { use_timings: opt_b(use_timings), loop_: opt_b(loop_), animations: opt_b(animations) };
    let show = SlideShow::new(models.clone(), start, now, opts);
    Box::into_raw(Box::new(UoSlidesShow { show, models }))
}

/// # Safety
/// `show` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_free(show: *mut UoSlidesShow) {
    if !show.is_null() {
        // SAFETY: the caller guarantees `show` came from uo_slides_show_new and is released once.
        drop(unsafe { Box::from_raw(show) });
    }
}

/// A presenter command: `cmd` 0 next (`arg` unused, `mouse` = a click), 1 prev, 2 go to slide `arg`, 3 first, 4 last, 5
/// update (the clock only). Returns the action: 0 none, 1 transition completed, 2 step, 3 slide, 4 end screen, 5 exit; a
/// negative value is an error status.
///
/// # Safety
/// `show` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_command(show: *mut UoSlidesShow, cmd: u32, arg: f64, now: f64, mouse: i32) -> i32 {
    // SAFETY: the caller guarantees `show` is null or a live handle.
    let Some(s) = (unsafe { show.as_mut() }) else { return -NULL_HANDLE };
    let a = match cmd {
        0 => s.show.next(now, mouse != 0),
        1 => s.show.prev(now),
        2 => {
            // an `i64` index as the TS `Math.trunc` of a number; a NaN or an infinity is "no slide"
            let t = arg.trunc();
            let idx = if t.is_finite() && t.abs() < 9.0e15 { t as i64 } else { -1 };
            s.show.go_to(idx, now)
        }
        3 => s.show.first(now),
        4 => s.show.last(now),
        5 => s.show.update(now),
        _ => return -BAD_ARG,
    };
    match a {
        SlideShowAction::None => 0,
        SlideShowAction::Transition => 1,
        SlideShowAction::Step => 2,
        SlideShowAction::Slide => 3,
        SlideShowAction::End => 4,
        SlideShowAction::Exit => 5,
    }
}

/// The frame at `now` (layout in the module documentation).
///
/// # Safety
/// `show` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_frame(show: *const UoSlidesShow, now: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees `show` is null or a live handle.
    let Some(s) = (unsafe { show.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let f: SlideShowFrame = s.show.frame(now);
    let mut o = Out::default();
    let index = f.slide.as_ref().and_then(|p| i32::try_from(p.index).ok()).unwrap_or(-1);
    o.bytes(&index.to_le_bytes());
    o.u32(s.show.step()).u32(s.show.steps()).u8(u8::from(f.ended)).f64(f.anim_ms).f64(f.transition_progress);
    o.u32(f.slide.as_ref().map_or(0, |p| p.effects.len()));
    match &f.from {
        Some(fr) => {
            o.u8(1).bytes(&i32::try_from(fr.slide.index).unwrap_or(-1).to_le_bytes());
            o.f64(fr.anim_ms).u32(usize::try_from(fr.key).unwrap_or(0)).u32(fr.slide.effects.len());
        }
        None => {
            o.u8(0);
        }
    }
    transition_out(&mut o, f.slide.as_ref().and_then(|p| p.transition.as_ref()));
    let _ = &s.models;
    buf(OK, o.0)
}

/// The next instant at which the show's clock acts (`due_at`; infinity = waits for the presenter).
///
/// # Safety
/// `show` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_due_at(show: *const UoSlidesShow) -> f64 {
    // SAFETY: the caller guarantees `show` is null or a live handle.
    unsafe { show.as_ref() }.map_or(f64::INFINITY, |s| s.show.due_at())
}

/// The next instant at which the frame changes by itself (`wake_at`; `now` = animating, infinity = stopped).
///
/// # Safety
/// `show` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_wake_at(show: *const UoSlidesShow, now: f64) -> f64 {
    // SAFETY: the caller guarantees `show` is null or a live handle.
    unsafe { show.as_ref() }.map_or(f64::INFINITY, |s| s.show.wake_at(now))
}

/// A playback timeline (`play()`): `use_timings` -1/0/1; `default_slide_ms`, `tick_ms`, `transition_fps` NaN = default.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_timeline_new(
    deck: *mut UoSlidesDeck,
    use_timings: i32,
    default_slide_ms: f64,
    tick_ms: f64,
    transition_fps: f64,
) -> *mut UoSlidesTimeline {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return std::ptr::null_mut() };
    let models = models_of(d);
    let opts = PlaybackOptions {
        use_timings: opt_b(use_timings),
        default_slide_ms: opt_f(default_slide_ms),
        tick_ms: opt_f(tick_ms),
        transition_fps: opt_f(transition_fps),
    };
    let tl = uo_pptx::playback::build_playback_timeline(&models, &opts);
    let slides = tl.slides.iter().cloned().map(Arc::new).collect();
    Box::into_raw(Box::new(UoSlidesTimeline { tl, slides }))
}

/// # Safety
/// `tl` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_timeline_free(tl: *mut UoSlidesTimeline) {
    if !tl.is_null() {
        // SAFETY: the caller guarantees `tl` came from uo_slides_timeline_new and is released once.
        drop(unsafe { Box::from_raw(tl) });
    }
}

/// The timeline's totals and slides (layout in the module documentation).
///
/// # Safety
/// `tl` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_timeline_info(tl: *const UoSlidesTimeline) -> *mut UoBuf {
    // SAFETY: the caller guarantees `tl` is null or a live handle.
    let Some(t) = (unsafe { tl.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut o = Out::default();
    o.f64(t.tl.duration_ms).f64(t.tl.tick_ms).f64(t.tl.media_clock).u32(t.tl.slides.len());
    for s in &t.tl.slides {
        o.u32(usize::try_from(s.index).unwrap_or(0)).f64(s.start_ms).f64(s.end_ms).f64(s.anim_start_ms).f64(s.transition_ms);
    }
    buf(OK, o.0)
}

/// The state at `t` ms (layout in the module documentation).
///
/// # Safety
/// `tl` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_timeline_state(tl: *const UoSlidesTimeline, t: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees `tl` is null or a live handle.
    let Some(tl) = (unsafe { tl.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let st = playback_state_at(&tl.tl, t);
    let slide = st.pos.and_then(|p| tl.tl.slides.get(p));
    let at = |p: Option<usize>| p.and_then(|p| tl.tl.slides.get(p)).and_then(|s| i32::try_from(s.index).ok()).unwrap_or(-1);
    let mut o = Out::default();
    o.bytes(&st.pos.and_then(|p| i32::try_from(p).ok()).unwrap_or(-1).to_le_bytes());
    o.bytes(&at(st.pos).to_le_bytes()).bytes(&at(st.from).to_le_bytes());
    o.f64(st.transition_progress).f64(st.anim_ms).u8(u8::from(st.ended)).u32(slide.map_or(0, |s| s.effects.len()));
    transition_out(&mut o, slide.and_then(|s| s.transition.as_ref()));
    buf(OK, o.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_handles_are_tolerated() {
        // SAFETY: null handles are part of the documented contract.
        unsafe {
            assert_eq!(uo_slides_count(std::ptr::null()), 0);
            uo_slides_deck_free(std::ptr::null_mut());
            uo_slides_view_free(std::ptr::null_mut());
            uo_slides_show_free(std::ptr::null_mut());
            uo_slides_timeline_free(std::ptr::null_mut());
            assert_eq!(uo_slides_set_caps(std::ptr::null_mut(), 15), NULL_HANDLE);
            assert_eq!(uo_slides_show_command(std::ptr::null_mut(), 0, 0.0, 0.0, 0), -NULL_HANDLE);
            assert_eq!(uo_slides_stage_clear(std::ptr::null_mut()), NULL_HANDLE);
            for b in [
                uo_slides_stage_show(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0.0,
                    0,
                    1.0,
                    0.0,
                    0.0,
                    1,
                    f64::NAN,
                    f64::NAN,
                ),
                uo_slides_stage_timeline(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(), 0.0, 1, 1.0, 0.0, 0.0, 0, 0.0, 1.0),
            ] {
                assert!(!b.is_null());
                crate::buf::uo_buf_free(b);
            }
        }
    }
}
