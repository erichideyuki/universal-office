//! The Slides EDITOR exports (B8 routing; record `core/docs/modules/uo-bindings.md` "Slides editor (B8 routing)"): the
//! editing session of `<universal-slides engine="rust">` in the SAME instance as the viewer's deck, so the emission of
//! the display lists (`uo_slides_emit*`, the stage of `present()`/`play()`) reads the edited package directly. The
//! editor module `uo-slides-editor.wasm` is the build with the features `slides` + `slides-edit`.
//!
//! | Handle | Created by | Owns |
//! |---|---|---|
//! | [`UoSlidesEditor`] | `uo_slides_editor_new(deck)` | the [`Editor`] (undo history, current slide, selection, font option, `Math.random`) and the snapshot of the package the shell last saw |
//!
//! The deck stays the viewer handle's ([`UoSlidesDeck`], opened by `uo_slides_open`): every editing export takes the
//! editor AND that deck (the one it was created over; another deck answers status 11) and lends the deck to the session
//! for the call ([`Editor::with_deck`]); a call that changed the package re-reads the deck's slide list
//! ([`UoSlidesDeck::refresh`]). JS frees the deck and the editor separately (`free()` and a `FinalizationRegistry`,
//! `js/slides-editor.mjs`); freeing the deck first leaves the editor unusable (every call answers 11), never unsafe:
//! the editor keeps only the deck's address to check it, not a pointer it dereferences.
//!
//! # Messages (schema `schema/slides_edit.uos`, generated codec in `slides_edit/codec`)
//! - `uo_slides_editor_apply(ed, deck, Apply)`, `uo_slides_editor_undo`, `_redo`, `_set_ui`, `_mark_clean`,
//!   `_set_font_embedding`, `_state` answer an `EditResult`: the outcome (`Ok(Outcome)` or `Err(message)`: a failed
//!   command leaves the package as it was, decision B8-1), the UI state the shell mirrors, and the parts that changed
//!   since the previous answer (the shell keeps its own copy of the package in step with them).
//! - `uo_slides_editor_copy_shape(ed, deck, id)` answers a `Clip` (status 1: nothing to copy); `Op.PasteShape` takes it
//!   back, in this editor or another one of the page.
//! - `uo_slides_editor_query(ed, deck, Query)` answers a `QueryResult` (`findText`, `defaultNewLayout`, the font option).
//! - The save with fonts in two phases (the protocol of Write): `uo_slides_editor_font_requests` answers the faces the
//!   plan wants (`FontRequests`); the host answers each with the font file (`FontFaces`, same order) in
//!   `uo_slides_editor_save_with_fonts`, which answers a `SaveResult` (the bytes and the `us-font-embedding` report).
//!   `uo_slides_editor_save` answers the package as it is.
//! - `uo_slides_editor_blank(Blank)` answers the bytes of a blank presentation (`blankDeck`), which JS opens as any deck.
//!
//! # Status codes
//! 0 ok · 1 none · 3 the package could not be written · 5 an invalid message or argument (the detail is UTF-8 in the
//! buffer) · 7 the command failed (the message is UTF-8 in the buffer; the package is unchanged) · 11 null handle, or a
//! deck that is not the editor's.

pub mod codec;
pub mod wire;

use std::sync::Arc;

use uo_pptx::edit::blank::{BlankDeckOptions, blank_deck_store};
use uo_pptx::edit::{EditError, Editor, Op, Outcome, changes_since};
use uo_pptx::package::PartsSnapshot;

use self::codec as w;
use self::wire::{FromWire, IntoWire};
use crate::buf::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};
use crate::slides::UoSlidesDeck;

const SAVE: i32 = 3;
const BAD_ARG: i32 = 5;
const EDIT: i32 = 7;

/// The editing session over a viewer deck (see the module documentation).
pub struct UoSlidesEditor {
    editor: Editor,
    /// the address of the deck handle the editor was created over (checked, never dereferenced)
    deck_addr: usize,
    /// the package as the shell last saw it (the parts every answer reports changed are relative to it)
    synced: PartsSnapshot,
}

fn bad(detail: &str) -> *mut UoBuf {
    buf(BAD_ARG, detail.as_bytes().to_vec())
}

/// The editor and its deck, when both are live and belong together.
///
/// # Safety
/// `ed` and `deck` must be null or live handles with no other reference in use.
unsafe fn pair<'a>(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck) -> Option<(&'a mut UoSlidesEditor, &'a mut UoSlidesDeck)> {
    // SAFETY: the caller guarantees both are null or live handles with no other reference in use.
    let (e, d) = unsafe { (ed.as_mut()?, deck.as_mut()?) };
    (e.deck_addr == deck as usize).then_some((e, d))
}

/// Encodes a message with one of the generated encoders (no tables: the protocol interns nothing).
fn message<M>(enc: fn(&mut Vec<u8>, &w::Delta, &M) -> Result<(), uo_codec::EncodeError>, m: &M) -> *mut UoBuf {
    let mut out = Vec::new();
    match enc(&mut out, &w::Delta::default(), m) {
        Ok(()) => buf(OK, out),
        Err(e) => bad(&format!("encode: {e:?}")),
    }
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl UoSlidesEditor {
    /// The UI state after a call (the deck lent).
    fn state(&mut self, d: &mut UoSlidesDeck) -> w::UiState {
        let ed = &mut self.editor;
        let slides = ed.with_deck(d.deck_mut(), Editor::slide_count);
        let emb = ed.font_embedding();
        w::UiState {
            index: ed.index(),
            selected: ed.selected().map(utf16),
            can_undo: ed.can_undo(),
            can_redo: ed.can_redo(),
            undo_label: ed.undo_label().map(utf16),
            redo_label: ed.redo_label().map(utf16),
            dirty: ed.is_dirty(),
            slides: u32::try_from(slides).unwrap_or(u32::MAX),
            embed: emb.embed,
            subset: emb.subset,
        }
    }

    /// The `EditResult` of a call: the outcome, the state and the parts that changed since the previous answer.
    fn answer(&mut self, d: &mut UoSlidesDeck, outcome: w::ApplyOutcome) -> *mut UoBuf {
        let mut changes = Vec::new();
        let any = {
            let store = d.deck_mut().parts();
            let (changed, gone) = changes_since(store, &self.synced);
            let any = !changed.is_empty() || !gone.is_empty();
            for e in changed {
                changes.push(w::PartChange { name: utf16(&e.name), bytes: Some(e.bytes.to_vec()) });
            }
            for n in gone {
                changes.push(w::PartChange { name: utf16(&n), bytes: None });
            }
            if any {
                self.synced = store.snapshot();
            }
            any
        };
        if any {
            // the slide list or the size may have changed; a package whose presentation no longer reads keeps the
            // previous list (the command that broke it was restored, B8-1, so this only happens on hostile input)
            let _ = d.refresh();
        }
        let state = self.state(d);
        message(w::encode_edit_result, &w::EditResult { outcome, state, changes })
    }
}

fn outcome_wire(r: Result<Outcome, EditError>) -> Result<w::ApplyOutcome, String> {
    match r {
        Ok(o) => o.into_wire().map(w::ApplyOutcome::Ok).map_err(|e| e.to_string()),
        Err(e) => Ok(w::ApplyOutcome::Err(utf16(&e.to_string()))),
    }
}

// ───────────────────────────── the handle ─────────────────────────────

/// A session over the open deck `deck` (`loadPptx`: empty history, no selection, the font option read from the
/// package). Null when `deck` is null.
///
/// # Safety
/// `deck` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_new(deck: *mut UoSlidesDeck) -> *mut UoSlidesEditor {
    // SAFETY: the caller guarantees `deck` is null or a live handle.
    let Some(d) = (unsafe { deck.as_mut() }) else { return std::ptr::null_mut() };
    let editor = Editor::detached(d.deck_mut());
    let synced = d.deck_mut().parts().snapshot();
    Box::into_raw(Box::new(UoSlidesEditor { editor, deck_addr: deck as usize, synced }))
}

/// # Safety
/// `ed` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_free(ed: *mut UoSlidesEditor) {
    if !ed.is_null() {
        // SAFETY: the caller guarantees `ed` came from uo_slides_editor_new and is released once.
        drop(unsafe { Box::from_raw(ed) });
    }
}

/// The bytes of a blank presentation (`blankDeck`; a consumed `Blank` message): status 0 and the `.pptx`.
///
/// # Safety
/// `ptr`/`len` must be a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_blank(ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    let m = match w::decode_blank(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(e) => return bad(&format!("decode: {e:?}")),
    };
    let opts = match BlankDeckOptions::from_wire(m.opts) {
        Ok(o) => o,
        Err(e) => return bad(&e.to_string()),
    };
    match blank_deck_store(&opts).save() {
        Ok(b) => buf(OK, b),
        Err(e) => buf(SAVE, e.to_string().into_bytes()),
    }
}

// ───────────────────────────── editing ─────────────────────────────

/// One editing command (a consumed `Apply` message): an `EditResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles; `ptr`/`len` a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_apply(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let m = match w::decode_apply(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(err) => return bad(&format!("decode: {err:?}")),
    };
    drop(bytes);
    let op = match Op::from_wire(m.op) {
        Ok(op) => op,
        Err(err) => return bad(&err.to_string()),
    };
    let r = e.editor.with_deck(d.deck_mut(), |s| s.apply(op));
    match outcome_wire(r) {
        Ok(o) => e.answer(d, o),
        Err(err) => bad(&err),
    }
}

/// Undo (`cmd` 0) or redo (`cmd` 1): an `EditResult` whose outcome is `Bool` (whether there was a step).
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_history(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck, cmd: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let done = match cmd {
        0 => e.editor.with_deck(d.deck_mut(), Editor::undo).is_some(),
        1 => e.editor.with_deck(d.deck_mut(), Editor::redo).is_some(),
        _ => return bad("history command"),
    };
    e.answer(d, w::ApplyOutcome::Ok(w::Outcome::Bool(done)))
}

/// The shell's UI state before a command: the current slide `index` (the shell clamps) and the selected shape (`ptr`/`len`
/// the UTF-8 id, consumed; `has` 0 = no selection). The session keeps them for the next undo step: an `EditResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles; `ptr`/`len` a consumed `uo_alloc_bytes` buffer (or null, 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_set_ui(
    ed: *mut UoSlidesEditor,
    deck: *mut UoSlidesDeck,
    index: u32,
    has: i32,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(id) = std::str::from_utf8(&bytes) else { return bad("selected id: not UTF-8") };
    let sel = (has != 0).then_some(id);
    e.editor.with_deck(d.deck_mut(), |s| {
        s.set_index(index);
        s.select(sel);
    });
    e.answer(d, w::ApplyOutcome::Ok(w::Outcome::Nothing))
}

/// The current state (and the parts changed since the last answer): an `EditResult` with outcome `Nothing`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_state(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    e.answer(d, w::ApplyOutcome::Ok(w::Outcome::Nothing))
}

/// The document was saved by the host (`markClean`): an `EditResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_mark_clean(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    e.editor.mark_clean();
    e.answer(d, w::ApplyOutcome::Ok(w::Outcome::Nothing))
}

/// The option "fonts in the file" (`setFontEmbedding`; -1 keeps a value, 0/1 sets it): an `EditResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_set_font_embedding(
    ed: *mut UoSlidesEditor,
    deck: *mut UoSlidesDeck,
    embed: i32,
    subset: i32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let opt = |v: i32| (v >= 0).then_some(v != 0);
    e.editor.set_font_embedding(opt(embed), opt(subset));
    e.answer(d, w::ApplyOutcome::Ok(w::Outcome::Nothing))
}

/// Seeds the editor's `Math.random()` (the creation ids of copies; the shell passes `crypto.getRandomValues`).
///
/// # Safety
/// `ed` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_set_random_seed(ed: *mut UoSlidesEditor, seed: u32) -> i32 {
    // SAFETY: the caller guarantees `ed` is null or a live handle.
    let Some(e) = (unsafe { ed.as_mut() }) else { return NULL_HANDLE };
    e.editor.set_random_seed(seed);
    OK
}

/// The language written in new text (`lang` of new runs; `ptr`/`len` the UTF-8 BCP 47 tag, consumed).
///
/// # Safety
/// `ed` must be null or a live handle; `ptr`/`len` a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_set_text_lang(ed: *mut UoSlidesEditor, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `ed` is null or a live handle.
    let Some(e) = (unsafe { ed.as_mut() }) else { return NULL_HANDLE };
    let Ok(lang) = std::str::from_utf8(&bytes) else { return BAD_ARG };
    e.editor.set_text_lang(lang);
    OK
}

// ───────────────────────────── clipboard and queries ─────────────────────────────

/// Copies shape `id` of the current slide (`copyShape`; `ptr`/`len` the UTF-8 id, consumed): a `Clip` (status 1 when the
/// shape cannot be copied: in a group, or not found; status 7 when the slide does not read).
///
/// # Safety
/// `ed`/`deck` must be null or live handles; `ptr`/`len` a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_copy_shape(
    ed: *mut UoSlidesEditor,
    deck: *mut UoSlidesDeck,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(id) = std::str::from_utf8(&bytes) else { return bad("shape id: not UTF-8") };
    match e.editor.with_deck(d.deck_mut(), |s| s.copy_shape(id)) {
        Ok(Some(clip)) => match clip.into_wire() {
            Ok(c) => message(w::encode_clip, &w::Clip { clip: c }),
            Err(err) => bad(&err.to_string()),
        },
        Ok(None) => buf(NONE, Vec::new()),
        Err(err) => buf(EDIT, err.to_string().into_bytes()),
    }
}

/// A query (a consumed `Query` message): a `QueryResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles; `ptr`/`len` a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_query(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let m = match w::decode_query(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(err) => return bad(&format!("decode: {err:?}")),
    };
    let r = match m.q {
        w::QueryOp::FindText(f) => {
            let query = String::from_utf16_lossy(&f.query);
            let hits = e.editor.with_deck(d.deck_mut(), |s| s.find_text(&query, f.match_case));
            match hits.into_wire() {
                Ok(h) => w::QueryAnswer::Hits(h),
                Err(err) => return bad(&err.to_string()),
            }
        }
        w::QueryOp::DefaultNewLayout => {
            let l = e.editor.with_deck(d.deck_mut(), Editor::default_new_layout);
            w::QueryAnswer::Layout(w::OptUtf16 { value: l.as_deref().map(utf16) })
        }
        w::QueryOp::FontEmbedding => {
            let s = e.editor.font_embedding();
            w::QueryAnswer::Embedding(w::EmbeddingState { embed: s.embed, subset: s.subset })
        }
    };
    message(w::encode_query_result, &w::QueryResult { r })
}

// ───────────────────────────── save ─────────────────────────────

/// Phase 1 of the save with fonts: a `FontRequests` (the option and the faces the plan wants; none when it is off).
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_font_requests(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let reqs = e.editor.with_deck(d.deck_mut(), |s| s.font_requests());
    let s = e.editor.font_embedding();
    match reqs.into_wire() {
        Ok(requests) => message(w::encode_font_requests, &w::FontRequests { embed: s.embed, subset: s.subset, requests }),
        Err(err) => bad(&err.to_string()),
    }
}

/// Phase 2: the font files of the requests of phase 1 (a consumed `FontFaces` message, one answer per request in order;
/// the package must not have changed in between): a `SaveResult`.
///
/// # Safety
/// `ed`/`deck` must be null or live handles; `ptr`/`len` a consumed `uo_alloc_bytes` buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_save_with_fonts(
    ed: *mut UoSlidesEditor,
    deck: *mut UoSlidesDeck,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let m = match w::decode_font_faces(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(err) => return bad(&format!("decode: {err:?}")),
    };
    drop(bytes);
    let faces: Vec<Option<Arc<[u8]>>> = match FromWire::from_wire(m.faces) {
        Ok(f) => f,
        Err(err) => return bad(&err.to_string()),
    };
    let r = e.editor.with_deck(d.deck_mut(), |s| {
        let reqs = s.font_requests();
        if reqs.len() != faces.len() {
            return Err(None);
        }
        let lookup = |r: &uo_fonts::FaceRequest| reqs.iter().position(|q| q == r).and_then(|i| faces.get(i).cloned().flatten());
        s.save_with_fonts(&lookup).map_err(Some)
    });
    let saved = match r {
        Ok(s) => s,
        Err(None) => return bad("one font answer per request of uo_slides_editor_font_requests"),
        Err(Some(err)) => return buf(SAVE, err.to_string().into_bytes()),
    };
    let embedded = saved.embedded.iter().map(|f| utf16(f)).collect();
    match saved.skipped.into_wire() {
        Ok(skipped) => message(w::encode_save_result, &w::SaveResult { bytes: saved.bytes, embedded, skipped }),
        Err(err) => bad(&err.to_string()),
    }
}

/// The `.pptx` bytes as they are (no font option applied): status 0 and the bytes.
///
/// # Safety
/// `ed`/`deck` must be null or live handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_editor_save(ed: *mut UoSlidesEditor, deck: *mut UoSlidesDeck) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles.
    let Some((e, d)) = (unsafe { pair(ed, deck) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match e.editor.with_deck(d.deck_mut(), |s| s.save()) {
        Ok(b) => buf(OK, b),
        Err(err) => buf(SAVE, err.to_string().into_bytes()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::codec::generated_support::{dump_apply, random_delta};
    use super::*;
    use crate::slides::{uo_slides_deck_free, uo_slides_open};

    fn take(b: *mut UoBuf) -> i32 {
        // SAFETY: a live result of this module, released here.
        unsafe {
            let s = crate::buf::uo_buf_status(b);
            crate::buf::uo_buf_free(b);
            s
        }
    }

    fn put(b: &[u8]) -> (*mut u8, usize) {
        let p = crate::uo_alloc_bytes(b.len());
        // SAFETY: the buffer has room for every byte of the slice.
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
        (p, b.len())
    }

    /// Random messages of the schema (every variant, hostile strings and numbers): the codec round trip is exact and the
    /// export answers ok or a refusal, never a panic, over a real deck.
    #[test]
    fn random_messages_round_trip_and_never_panic() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../uo-pptx/tests/captures/decks/b1a/0001.pptx");
        let bytes = std::fs::read(path).unwrap();
        let (p, n) = put(&bytes);
        // SAFETY: a fresh buffer; the handles are released at the end.
        let d = unsafe { uo_slides_open(p, n) };
        assert!(!d.is_null());
        // SAFETY: the live deck.
        let ed = unsafe { uo_slides_editor_new(d) };
        let mut rng = uo_codec::testing::Rng(7);
        for _ in 0..400 {
            let (delta, b) = random_delta(&mut rng, &w::Tables::new(), 0, true);
            let m = w::Apply::random(&mut rng, &b, 0);
            let mut out = Vec::new();
            w::encode_apply(&mut out, &delta, &m).unwrap();
            let back = w::decode_apply(&out, &mut w::Tables::new(), &w::limits()).unwrap();
            assert_eq!(dump_apply(&back, &w::Tables::new()), dump_apply(&m, &w::Tables::new()));
            let (p, n) = put(&out);
            // SAFETY: live handles; a fresh buffer.
            let status = take(unsafe { uo_slides_editor_apply(ed, d, p, n) });
            assert!(status == OK || status == BAD_ARG, "status {status}");
        }
        // SAFETY: live handles, released once.
        unsafe {
            uo_slides_editor_free(ed);
            uo_slides_deck_free(d);
        }
    }
}
