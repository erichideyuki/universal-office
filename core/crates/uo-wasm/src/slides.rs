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
//! - `uo_slides_emit_suppress`: as `uo_slides_emit`, with the text of one shape left out (the editor's inline overlay).
//! - `uo_slides_view_is_recipe(view, handle)`: 1 when bitmap `handle` is a metafile/PICT/TIFF recipe (B5b), 0 for RGBA.
//!
//! # Read model (wave 4: `<universal-slides-viewer engine="rust">` without the TS model)
//! `bool3` is a `u8`: 0 false, 1 true, 2 unset. Every slide index is a viewer index.
//! - `uo_slides_outline(deck, i)`: `n u32` shapes, per shape `flags u8` (bit 0 inherited, 1 picture, 2 media, 3 text body, 4 table,
//!   5 alternative text present), `ph_type str`, `descr str`, `n u32` paragraphs (`str` each: the runs' text joined), then with bit 4
//!   `rows u32`, per row `cells u32`, per cell `flags u8` (bit 0 hMerge, 1 vMerge) and its paragraphs.
//! - `uo_slides_links(deck, i, scale, ox, oy)`: `n u32`, per box `shape u32, source u8` (0 run address = the TS viewer's hit-test,
//!   1 the shape's click, 2 a run's jump), `x, y, w, h f64` (the unrotated shape box at the view), `kind u8` and 0 `url str`, 1 `slide
//!   i32` (-1: a slide the viewer does not show), 2 `jump u8` (next, previous, first, last, last viewed, end) — `uo_pptx::links`.
//! - `uo_slides_assets(deck, i)`: `n u32` pictures to decode (each part once, in the order of the TS `warmSlideImages`): `part str,
//!   mime str, flags u8` (bit 0 an animated-GIF candidate, bit 1 rasterized by the emission: a metafile, PICT or TIFF); then `n u32`
//!   media/GIF shapes: `shape u32, inherited u8, spid str, gif u8` (+ `part str`), `kind u8` (0 none, 1 video, 2 audio; then `part
//!   str, url str, mime str, trim_start f64, trim_end f64, fade_in f64, fade_out f64`, NaN = none), `info u8` (+ `vol f64, mute
//!   bool3, show_when_stopped bool3, rewind bool3, loop bool3, full_screen bool3`: the slide timing model's media entry).
//! - `uo_slides_media_show(deck, show, now, which, latency, scale)` / `uo_slides_media_timeline(deck, tl, t, which, ...)`: the
//!   media states of the stage (`present u8`; then `index i32, anim_ms f64, gif_ms f64, n u32`, per media shape `shape u32,
//!   started u8, playing u8, ended u8, time_sec f64, dur_ms f64` (NaN = unknown)): `uo_pptx::media::media_state_at`.
//! - `uo_slides_fonts(deck)`: the embedded fonts (layout at the export); `uo_slides_typefaces(deck)`: `n u32` names;
//!   `uo_slides_show_settings(deck)`: `present u8`, `loop, use_timings, show_animation, kiosk` (bool3), `lo f64, hi f64`.

use std::cell::RefCell;
use std::sync::Arc;

use uo_dml_render::emit::EmitCaches;
use uo_dml_render::text::{FaceKey, Kerning, MAX_ROUNDS, MeasureReq, MeasureTable, StringKey, TextMetrics};
use uo_dml_render::{MediaId, PartSource};
use uo_pptx::anim_engine::AnimCaches;
use uo_pptx::emit::emit_slide_suppressed;
use uo_pptx::media::MediaClock;
use uo_pptx::playback::{PlaybackOptions, PlaybackSlide, PlaybackTimeline, playback_state_at};
use uo_pptx::show::{SlideShow, SlideShowAction, SlideShowFrame, SlideShowOptions};
use uo_pptx::stage::{Stage, anim_plan, emit_stage, has_media, stage_frame, wire_values};
use uo_pptx::timing::{TimingModel, TransitionInfo};
use uo_pptx::{Deck, EmitFail, Limits, PptxError, View};
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

// used by the editor routing (`slides_edit.rs`, feature `slides-edit`); unused in the viewer module
#[cfg_attr(not(feature = "slides-edit"), allow(dead_code))]
impl UoSlidesDeck {
    /// The editor module (slides_edit.rs): the deck the editor operations run on — the same deck the emission reads.
    pub(crate) fn deck_mut(&mut self) -> &mut Deck {
        &mut self.deck
    }

    /// After an edit changed the package: the slide list and the size are read again, pending requests dropped.
    ///
    /// # Errors
    /// The presentation can no longer be read.
    pub(crate) fn refresh(&mut self) -> Result<(), PptxError> {
        let p = self.deck.presentation()?;
        self.map = p.slides.iter().enumerate().filter(|(_, r)| r.target.is_some()).filter_map(|(i, _)| u32::try_from(i).ok()).collect();
        self.size = (p.slide_w, p.slide_h);
        self.pending.clear();
        Ok(())
    }
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

/// The PowerPoint-over-TS switches of the deck (Slides wave 5; the default after `uo_slides_open` = PowerPoint's rules):
/// `flags` bit 0 = every TS switch (`SlidesRules::ts_strict`, the parity harnesses), bit 1 `ts_sys_clr`, bit 2
/// `ts_first_baseline`, bit 3 `ts_fallback_fonts`, bit 4 `ts_date_locale`, bit 5 `ts_warp`. A change empties the deck's
/// caches and the measure table; views made before keep their lists (start a new generation). 0 ok, 11 null handle.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_set_rules(deck: *mut UoSlidesDeck, flags: u32) -> i32 {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return 11 };
    let rules = if flags & 1 != 0 {
        uo_dml_render::SlidesRules::ts_strict()
    } else {
        uo_dml_render::SlidesRules {
            ts_sys_clr: flags & 2 != 0,
            ts_first_baseline: flags & 4 != 0,
            ts_fallback_fonts: flags & 8 != 0,
            ts_date_locale: flags & 16 != 0,
            ts_warp: flags & 32 != 0,
            ..uo_dml_render::SlidesRules::default()
        }
    };
    if d.deck.rules() != rules {
        d.deck.set_rules(rules);
        d.table = MeasureTable::default();
        d.pending.clear();
    }
    0
}

/// The clock of the date/time fields (Slides wave 5: PowerPoint shows the current date in each field's language): the
/// host's LOCAL date and time and the language of fields without one (`lang` UTF-8, from `uo_alloc_bytes`, consumed);
/// `year` 0 removes the clock (the fields keep their cached text, the state after `uo_slides_open`). 0 ok, 5 invalid
/// argument, 11 null handle.
///
/// # Safety
/// `deck` must be null or a live handle; `lang`/`lang_len` as for `uo_slides_open` (or null and 0).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_slides_set_field_clock(
    deck: *mut UoSlidesDeck,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    lang: *mut u8,
    lang_len: usize,
) -> i32 {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(lang_len) (or is null with 0).
    let lang_bytes = if lang.is_null() { Vec::new() } else { unsafe { take_bytes(lang, lang_len) } };
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return 11 };
    let clock = if year == 0 {
        None
    } else {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
            return 5;
        }
        let lang = String::from_utf8_lossy(&lang_bytes);
        let lang = if lang.is_empty() || lang.len() > 64 { "en-US".into() } else { lang.into_owned().into_boxed_str() };
        Some(uo_dml_render::datefield::FieldClock { year, month, day, hour, minute, second, default_lang: lang })
    };
    if d.deck.field_clock() != clock.as_ref() {
        d.deck.set_field_clock(clock);
    }
    0
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

/// The compressed bitmaps of the EMF+ picture in a package part (`uo_dml_render::metafile::compressed_bitmaps`: what the TS
/// `prepareSpecialImage` decodes): `u32` count, then `u32` length + bytes each, little endian; empty for any other part. The
/// viewer decodes them while it warms the slide's pictures (the `MetaDecode` prepare phase started ahead of the paint).
///
/// # Safety
/// `deck` must be null or a live handle; `ptr`/`len` a consumed `uo_alloc_bytes` buffer with the UTF-8 part name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_meta_bitmaps(deck: *const UoSlidesDeck, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let name = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(name) = std::str::from_utf8(&name) else { return buf(BAD_ARG, Vec::new()) };
    let Some(b) = d.deck.parts().bytes(name) else { return buf(NO_ITEM, Vec::new()) };
    let blobs = uo_dml_render::metafile::compressed_bitmaps(b);
    let mut out = Vec::with_capacity(4 + blobs.iter().map(|x| 4 + x.len()).sum::<usize>());
    let le = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();
    out.extend_from_slice(&le(blobs.len()));
    for x in &blobs {
        out.extend_from_slice(&le(x.len()));
        out.extend_from_slice(x);
    }
    buf(OK, out)
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

/// Whether the bytes of bitmap `handle` are a metafile/PICT/TIFF recipe (B5b: replayed by `meta-player.ts`), not RGBA: 1 yes, 0 no,
/// -6 no such bitmap, -11 a null view.
///
/// # Safety
/// `view` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_view_is_recipe(view: *const UoSlidesView, handle: u32) -> i32 {
    // SAFETY: the caller guarantees `view` is null or a live handle.
    let Some(v) = (unsafe { view.as_ref() }) else { return -NULL_HANDLE };
    if v.res.bitmap_bytes(handle).is_none() {
        return -NO_ITEM;
    }
    i32::from(v.res.is_recipe(handle))
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
    emit_out(d, v, index, scale, ox, oy, None)
}

/// `uo_slides_emit` with the text of shape `shape` (an index of the slide's shapes, inherited decorations included) left out:
/// the editor's inline overlay covers it (`suppressTextShapeIdx`). `shape` < 0 = nothing suppressed.
///
/// # Safety
/// `deck` and `view` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_emit_suppress(
    deck: *mut UoSlidesDeck,
    view: *mut UoSlidesView,
    index: usize,
    scale: f64,
    ox: f64,
    oy: f64,
    shape: i32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees both handles are null or live, with no other reference in use.
    let (Some(d), Some(v)) = (unsafe { (deck_mut(deck), view.as_mut()) }) else { return buf(NULL_HANDLE, Vec::new()) };
    emit_out(d, v, index, scale, ox, oy, u32::try_from(shape).ok())
}

fn emit_out(d: &mut UoSlidesDeck, v: &mut UoSlidesView, index: usize, scale: f64, ox: f64, oy: f64, suppress: Option<u32>) -> *mut UoBuf {
    if !(scale.is_finite() && scale > 0.0 && ox.is_finite() && oy.is_finite()) {
        return buf(BAD_ARG, Vec::new());
    }
    let Some(&pi) = d.map.get(index) else { return buf(NO_ITEM, Vec::new()) };
    match emit_slide_suppressed(&mut d.deck, pi, View { scale, ox, oy }, suppress, &d.table, &mut v.res, &mut v.caches) {
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

// ───────────────────────────── the read model (wave 4: the viewer without the TS model) ─────────────────────────────
//
// What `<universal-slides-viewer engine="rust">` used to read from the TS model (`PptxPackage` + `readSlide` of every slide):
// the accessible text, the link boxes, the pictures to decode, the media of the slides and their state in a playback frame,
// the embedded fonts, the typefaces and the show settings. Layouts in the module documentation ("Read model").

fn tri(v: Option<bool>) -> u8 {
    match v {
        None => 2,
        Some(b) => u8::from(b),
    }
}

fn opt_str(o: &mut Out, s: Option<&str>) {
    str_out(o, s.unwrap_or(""));
}

/// The paragraphs of a text body as the accessible text reads them: the runs' text joined (the shell normalises the spaces).
fn paras_out(o: &mut Out, tb: Option<&uo_dml_render::model::TextBody>) {
    let Some(tb) = tb else {
        o.u32(0);
        return;
    };
    o.u32(tb.paragraphs.len());
    let mut s = String::new();
    for p in &tb.paragraphs {
        s.clear();
        for r in &p.runs {
            s.push_str(&r.text);
        }
        str_out(o, &s);
    }
}

fn slide_of(d: &mut UoSlidesDeck, index: usize) -> Result<Arc<uo_pptx::SlideModel>, i32> {
    let &pi = d.map.get(index).ok_or(NO_ITEM)?;
    d.deck.slide(pi).map_err(|e| error_code(&e))
}

/// The accessible outline of viewer slide `index` (the TS `SlideA11y` input): per shape its flags, placeholder type, alternative
/// text, paragraphs and table cells.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_outline(deck: *mut UoSlidesDeck, index: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let slide = match slide_of(d, index) {
        Ok(s) => s,
        Err(code) => return buf(code, Vec::new()),
    };
    let mut o = Out::default();
    o.u32(slide.shapes.len());
    for sh in &slide.shapes {
        let ex = sh.extras.as_deref();
        let media = ex.is_some_and(|e| e.media.is_some());
        let table = ex.and_then(|e| e.table.as_deref());
        let flags = u8::from(sh.inherited.unwrap_or(false))
            | (u8::from(sh.paint().image.is_some()) << 1)
            | (u8::from(media) << 2)
            | (u8::from(sh.tx_body.is_some()) << 3)
            | (u8::from(table.is_some()) << 4)
            | (u8::from(ex.is_some_and(|e| e.descr.is_some())) << 5);
        o.u8(flags);
        opt_str(&mut o, sh.ph_type.as_deref());
        opt_str(&mut o, ex.and_then(|e| e.descr.as_deref()));
        paras_out(&mut o, sh.tx_body.as_deref());
        if let Some(t) = table {
            o.u32(t.rows.len());
            for row in &t.rows {
                o.u32(row.cells.len());
                for c in &row.cells {
                    o.u8(u8::from(c.h_merge.unwrap_or(false)) | (u8::from(c.v_merge.unwrap_or(false)) << 1));
                    paras_out(&mut o, c.tx_body.as_deref());
                }
            }
        }
    }
    buf(OK, o.0)
}

/// The link boxes of viewer slide `index` at `scale` px per EMU and origin `(ox, oy)` (`uo_pptx::links::slide_links`).
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_links(deck: *mut UoSlidesDeck, index: usize, scale: f64, ox: f64, oy: f64) -> *mut UoBuf {
    use uo_pptx::links::{LinkSource, LinkTarget, ShowJump};
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(view) = view_of(scale, ox, oy) else { return buf(BAD_ARG, Vec::new()) };
    let Some(&pi) = d.map.get(index) else { return buf(NO_ITEM, Vec::new()) };
    let links = match uo_pptx::links::slide_links(&mut d.deck, pi, view) {
        Ok(l) => l,
        Err(e) => return buf(error_code(&e), Vec::new()),
    };
    let mut o = Out::default();
    o.u32(links.len());
    for l in &links {
        o.u32(usize::try_from(l.shape).unwrap_or(usize::MAX));
        o.u8(match l.source {
            LinkSource::RunUrl => 0,
            LinkSource::ShapeClick => 1,
            LinkSource::RunJump => 2,
        });
        for v in l.rect {
            o.f64(v);
        }
        match &l.target {
            LinkTarget::Url(u) => {
                o.u8(0);
                str_out(&mut o, u);
            }
            LinkTarget::Slide(s) => {
                // the viewer index of the presentation slide (-1: a slide the viewer does not show)
                let vi = d.map.iter().position(|&m| m == *s).and_then(|v| i32::try_from(v).ok()).unwrap_or(-1);
                o.u8(1).bytes(&vi.to_le_bytes());
            }
            LinkTarget::Show(j) => {
                o.u8(2).u8(match j {
                    ShowJump::Next => 0,
                    ShowJump::Previous => 1,
                    ShowJump::First => 2,
                    ShowJump::Last => 3,
                    ShowJump::LastViewed => 4,
                    ShowJump::End => 5,
                });
            }
        }
    }
    buf(OK, o.0)
}

/// Every picture use of a slide, in the order of the TS `warmSlideImages` (background, then per shape: the picture, the fill,
/// the table's, the chart's, the bullets' and the letters').
fn slide_images(slide: &uo_pptx::SlideModel) -> Vec<(&uo_dml_render::model::ImageRef, bool)> {
    use uo_dml_render::model::{ImageRef, TextBody};
    fn body<'a>(tb: Option<&'a TextBody>, out: &mut Vec<(&'a ImageRef, bool)>) {
        for p in tb.map_or(&[][..], |t| &t.paragraphs[..]) {
            if let Some(b) = p.bu().bullet_image.as_deref() {
                out.push((b, false));
            }
            for r in &p.runs {
                if let Some(f) = r.extras.as_deref().and_then(|e| e.fx().fill_image.as_ref()) {
                    out.push((f, false));
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Some(b) = slide.bg_image.as_ref() {
        out.push((b, false));
    }
    for sh in &slide.shapes {
        if let Some(i) = sh.paint().image.as_deref() {
            out.push((i, true));
        }
        if let Some(f) = sh.paint().fill_image.as_deref() {
            out.push((f, false));
        }
        let ex = sh.extras.as_deref();
        if let Some(t) = ex.and_then(|e| e.table.as_deref()) {
            if let Some(i) = t.bg_paint.as_deref().and_then(|b| b.image.as_deref()) {
                out.push((i, false));
            }
            for row in &t.rows {
                for c in &row.cells {
                    if let Some(f) = c.fill_image.as_deref() {
                        out.push((f, false));
                    }
                    body(c.tx_body.as_deref(), &mut out);
                }
            }
        }
        for ci in ex.and_then(|e| e.chart.as_ref()).and_then(|c| c.images.as_deref()).unwrap_or(&[]) {
            out.push((&ci.image, false));
        }
        body(sh.tx_body.as_deref(), &mut out);
    }
    out
}

/// The media of viewer slide `index`: the pictures the shell decodes (each part once; metafiles, PICT and TIFF are
/// rasterized by the emission and listed with flag 2), then the media shapes and the animated-GIF shapes with their entry of
/// the timing model. Layout in the module documentation.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_assets(deck: *mut UoSlidesDeck, index: usize) -> *mut UoBuf {
    use uo_dml_render::model::MediaKind;
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let slide = match slide_of(d, index) {
        Ok(s) => s,
        Err(code) => return buf(code, Vec::new()),
    };
    let src = d.deck.source();
    let mut o = Out::default();
    let mut seen: Vec<MediaId> = Vec::new();
    let mut pics = Out::default();
    let mut n = 0usize;
    for (r, is_pic) in slide_images(&slide) {
        if seen.contains(&r.media) {
            continue;
        }
        seen.push(r.media);
        let Some((part, bytes)) = src.media_entry(r.media) else { continue };
        let special = uo_dml_render::metafile::is_special_image(&bytes);
        let flags = u8::from(is_pic && &*r.mime == "image/gif") | (u8::from(special) << 1);
        str_out(&mut pics, &part);
        str_out(&mut pics, &r.mime);
        pics.u8(flags);
        n += 1;
    }
    o.u32(n).bytes(&pics.0);
    let infos = slide.timing.as_deref().and_then(|t| t.media.as_deref()).unwrap_or(&[]);
    let mut media = Out::default();
    let mut nm = 0usize;
    for (i, sh) in slide.shapes.iter().enumerate() {
        let gif = sh.paint().image.as_deref().filter(|im| &*im.mime == "image/gif");
        let m = sh.extras.as_deref().and_then(|e| e.media.as_ref());
        if m.is_none() && gif.is_none() {
            continue;
        }
        nm += 1;
        media.u32(i).u8(u8::from(sh.inherited.unwrap_or(false)));
        str_out(&mut media, &sh.id);
        match gif.and_then(|g| src.media_entry(g.media)) {
            Some((part, _)) => {
                media.u8(1);
                str_out(&mut media, &part);
            }
            None => {
                media.u8(0);
            }
        }
        match m {
            None => {
                media.u8(0);
            }
            Some(m) => {
                media.u8(match m.kind {
                    MediaKind::Video => 1,
                    MediaKind::Audio => 2,
                });
                opt_str(&mut media, m.part.as_deref());
                opt_str(&mut media, m.url.as_deref());
                str_out(&mut media, &m.mime);
                let (ts, te) = m.trim.unwrap_or((f64::NAN, f64::NAN));
                let (fi, fo) = m.fade.unwrap_or((f64::NAN, f64::NAN));
                media.f64(ts).f64(te).f64(fi).f64(fo);
            }
        }
        match infos.iter().find(|x| x.spid == sh.id) {
            None => {
                media.u8(0);
            }
            Some(mi) => {
                media.u8(1).f64(mi.vol.unwrap_or(f64::NAN));
                media.u8(tri(mi.mute)).u8(tri(mi.show_when_stopped)).u8(tri(mi.rewind)).u8(tri(mi.loop_)).u8(tri(mi.full_screen));
            }
        }
    }
    o.u32(nm).bytes(&media.0);
    buf(OK, o.0)
}

/// The media states of a playback slide at `anim_ms` (the TS `mediaStateAt` per media shape of the slide, inherited ones left
/// out as the viewer's media player does).
fn media_states_out(d: &mut UoSlidesDeck, ps: Option<Arc<PlaybackSlide>>, anim_ms: f64, clock: &MediaClock) -> *mut UoBuf {
    let mut o = Out::default();
    let Some(ps) = ps else {
        o.u8(0);
        return buf(OK, o.0);
    };
    let slide = match slide_of(d, usize::try_from(ps.index).unwrap_or(usize::MAX)) {
        Ok(s) => s,
        Err(code) => return buf(code, Vec::new()),
    };
    // the GIF clock: from the start of the slide's animations; in the final state the length of the slide (`media.frame`)
    let gif_ms = if anim_ms.is_finite() { anim_ms } else { ps.end_ms - ps.anim_start_ms };
    o.u8(1).bytes(&i32::try_from(ps.index).unwrap_or(-1).to_le_bytes()).f64(anim_ms).f64(gif_ms);
    let infos = ps.model.media.as_deref().unwrap_or(&[]);
    let shapes: Vec<(usize, &str)> = slide
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, sh)| !sh.inherited.unwrap_or(false) && sh.extras.as_deref().is_some_and(|e| e.media.is_some()))
        .map(|(i, sh)| (i, &*sh.id))
        .collect();
    o.u32(shapes.len());
    for (i, spid) in shapes {
        let looping = infos.iter().find(|x| &*x.spid == spid).is_some_and(|x| x.loop_ == Some(true));
        let st = uo_pptx::media::media_state_at(&ps.model.nodes, &ps.effects, spid, anim_ms, clock, looping);
        o.u32(i).u8(u8::from(st.started)).u8(u8::from(st.playing)).u8(u8::from(st.ended == Some(true)));
        o.f64(st.time_sec).f64(st.dur_ms.unwrap_or(f64::NAN));
    }
    buf(OK, o.0)
}

/// The media states of the show's stage `which` (0 the slide on stage, 1 the transition origin at its frozen time) at `now`.
///
/// # Safety
/// `deck` and `show` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_media_show(
    deck: *mut UoSlidesDeck,
    show: *const UoSlidesShow,
    now: f64,
    which: u32,
    latency: f64,
    clock_scale: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live, with no other reference in use.
    let (Some(d), Some(s)) = (unsafe { (deck_mut(deck), show.as_ref()) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let f = s.show.frame(now);
    let (ps, anim_ms) = match which {
        0 => (f.slide, f.anim_ms),
        1 => f.from.map_or((None, 0.0), |o| (Some(o.slide), o.anim_ms)),
        _ => return buf(BAD_ARG, Vec::new()),
    };
    media_states_out(d, ps, anim_ms, &MediaClock { latency_ms: opt_f(latency), scale: opt_f(clock_scale) })
}

/// The media states of the timeline's stage `which` at `t` (the origin in its final state, as [`uo_slides_stage_timeline`]).
///
/// # Safety
/// `deck` and `tl` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_media_timeline(
    deck: *mut UoSlidesDeck,
    tl: *const UoSlidesTimeline,
    t: f64,
    which: u32,
    latency: f64,
    clock_scale: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live, with no other reference in use.
    let (Some(d), Some(tl)) = (unsafe { (deck_mut(deck), tl.as_ref()) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let st = playback_state_at(&tl.tl, t);
    let (pos, anim_ms) = match which {
        0 => (st.pos, st.anim_ms),
        1 => (st.from, f64::INFINITY),
        _ => return buf(BAD_ARG, Vec::new()),
    };
    let ps = pos.and_then(|p| tl.slides.get(p)).cloned();
    media_states_out(d, ps, anim_ms, &MediaClock { latency_ms: opt_f(latency), scale: opt_f(clock_scale) })
}

/// The fonts embedded in the package (`readEmbeddedFonts`): `n u32`, per font `family str, variant u8 (0 regular, 1 bold, 2
/// italic, 3 bold italic), weight i32 (-1 unknown), italic u8 (0, 1, 2 unknown), len u32` and the sfnt bytes.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_fonts(deck: *const UoSlidesDeck) -> *mut UoBuf {
    use uo_pptx::fonts::FontVariant;
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let fonts = d.deck.embedded_fonts();
    let mut o = Out::default();
    o.u32(fonts.len());
    for f in &fonts {
        str_out(&mut o, &f.family);
        o.u8(match f.variant {
            FontVariant::Regular => 0,
            FontVariant::Bold => 1,
            FontVariant::Italic => 2,
            FontVariant::BoldItalic => 3,
        });
        o.bytes(&f.weight.and_then(|w| i32::try_from(w).ok()).unwrap_or(-1).to_le_bytes()).u8(tri(f.italic));
        o.u32(f.data.len()).bytes(&f.data);
    }
    buf(OK, o.0)
}

/// The typefaces the slides, layouts and masters name literally (`collectDeckTypefaces`): `n u32` and the names.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_typefaces(deck: *const UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let names = d.deck.deck_typefaces();
    let mut o = Out::default();
    o.u32(names.len());
    for n in &names {
        str_out(&mut o, n);
    }
    buf(OK, o.0)
}

/// The show settings of `ppt/presProps.xml` (`<p:showPr>`): `present u8`, then `loop`, `use_timings`, `show_animation`, `kiosk`
/// (u8 each: 0, 1, 2 unset) and the slide range `lo f64, hi f64` (NaN = none).
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_show_settings(deck: *mut UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck_mut(deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let p = match d.deck.presentation() {
        Ok(p) => p,
        Err(e) => return buf(error_code(&e), Vec::new()),
    };
    let mut o = Out::default();
    match p.show.as_ref() {
        None => {
            o.u8(0);
        }
        Some(s) => {
            let (lo, hi) = s.range.unwrap_or((f64::NAN, f64::NAN));
            o.u8(1).u8(tri(s.loop_)).u8(tri(s.use_timings)).u8(tri(s.show_animation)).u8(tri(s.kiosk)).f64(lo).f64(hi);
        }
    }
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
            assert_eq!(uo_slides_view_is_recipe(std::ptr::null(), 0), -NULL_HANDLE);
            for b in [
                uo_slides_outline(std::ptr::null_mut(), 0),
                uo_slides_links(std::ptr::null_mut(), 0, 1.0, 0.0, 0.0),
                uo_slides_assets(std::ptr::null_mut(), 0),
                uo_slides_media_show(std::ptr::null_mut(), std::ptr::null(), 0.0, 0, f64::NAN, f64::NAN),
                uo_slides_media_timeline(std::ptr::null_mut(), std::ptr::null(), 0.0, 1, f64::NAN, f64::NAN),
                uo_slides_fonts(std::ptr::null()),
                uo_slides_typefaces(std::ptr::null()),
                uo_slides_show_settings(std::ptr::null_mut()),
                uo_slides_emit_suppress(std::ptr::null_mut(), std::ptr::null_mut(), 0, 1.0, 0.0, 0.0, 0),
            ] {
                assert!(!b.is_null());
                crate::buf::uo_buf_free(b);
            }
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
