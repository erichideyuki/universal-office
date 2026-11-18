//! The Slides editor exports (B8 routing, `src/slides_edit.rs`) natively: the B8 parity captures replayed through the
//! wasm ABI — every operation encoded with the schema codec (`schema/slides_edit.uos`), applied by
//! `uo_slides_editor_apply` on the editor over a viewer deck handle (`uo_slides_open`), and the package compared as the
//! SHELL sees it: a mirror kept only from the parts each answer reports changed (what `<universal-slides engine="rust">`
//! keeps in its TS package). After every step: the mirror's bytes, the UI state of the answer, the models read fresh
//! from the mirror and the step's result against the frozen TS capture; the deck's slide count against the state.
//! Plus the handle rules (a foreign deck, null handles), the codec round trip of random messages and M4 (10,000 editor
//! handles created and dropped, live bytes back exactly).
#![cfg(feature = "slides-edit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

#[path = "../../uo-pptx/tests/support/edit_capture.rs"]
mod edit_capture;

use edit_capture::{Driver, Init, face_bytes, replay, state_text, store_model_facets};
use uo_jsnum::format::to_js_string;
use uo_pptx::edit::Op;
use uo_pptx::{Deck, Limits, PartStore};
use uo_wasm::buf::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_buf_status};
use uo_wasm::slides::{UoSlidesDeck, uo_slides_count, uo_slides_deck_free, uo_slides_open};
use uo_wasm::slides_edit::codec as w;
use uo_wasm::slides_edit::wire::IntoWire;
use uo_wasm::slides_edit::*;
use uo_wasm::uo_alloc_bytes;

fn put(b: &[u8]) -> (*mut u8, usize) {
    if b.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let p = uo_alloc_bytes(b.len());
    // SAFETY: `p` has room for `b.len()` bytes.
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    (p, b.len())
}

fn take(b: *mut UoBuf) -> (i32, Vec<u8>) {
    // SAFETY: `b` is a live result from this module, released here.
    unsafe {
        let status = uo_buf_status(b);
        let len = uo_buf_len(b);
        let data = if len == 0 { Vec::new() } else { std::slice::from_raw_parts(uo_buf_ptr(b), len).to_vec() };
        uo_buf_free(b);
        (status, data)
    }
}

fn enc<M>(f: fn(&mut Vec<u8>, &w::Delta, &M) -> Result<(), uo_codec::EncodeError>, m: &M) -> Vec<u8> {
    let mut out = Vec::new();
    f(&mut out, &w::Delta::default(), m).unwrap();
    out
}

fn s16(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

fn open_deck(bytes: &[u8]) -> *mut UoSlidesDeck {
    let (p, n) = put(bytes);
    // SAFETY: a fresh uo_alloc_bytes buffer.
    let d = unsafe { uo_slides_open(p, n) };
    assert!(!d.is_null(), "the deck opens");
    d
}

/// The shell side of one editor: the deck and editor handles, the mirror of the package, the last state, the clip.
struct AbiDriver {
    deck: *mut UoSlidesDeck,
    ed: *mut UoSlidesEditor,
    mirror: PartStore,
    state: Option<w::UiState>,
    clip: Option<Vec<u8>>,
}

impl Default for AbiDriver {
    fn default() -> Self {
        AbiDriver { deck: std::ptr::null_mut(), ed: std::ptr::null_mut(), mirror: PartStore::new(), state: None, clip: None }
    }
}

impl Drop for AbiDriver {
    fn drop(&mut self) {
        self.close();
    }
}

impl AbiDriver {
    fn close(&mut self) {
        // SAFETY: live handles of this driver (or null), released once.
        unsafe {
            uo_slides_editor_free(self.ed);
            uo_slides_deck_free(self.deck);
        }
        self.ed = std::ptr::null_mut();
        self.deck = std::ptr::null_mut();
    }

    /// Decodes an `EditResult`, applies its changes to the mirror, keeps the state; the `ret` facet.
    fn result(&mut self, b: *mut UoBuf) -> String {
        let (status, data) = take(b);
        assert_eq!(status, 0, "{}", String::from_utf8_lossy(&data));
        let r = w::decode_edit_result(&data, &mut w::Tables::new(), &w::limits()).unwrap();
        for c in r.changes {
            let name = s16(&c.name);
            match c.bytes {
                Some(b) => {
                    self.mirror.set(&name, std::sync::Arc::from(b));
                }
                None => {
                    self.mirror.remove(&name);
                }
            }
        }
        // SAFETY: the live deck handle.
        let count = unsafe { uo_slides_count(self.deck) };
        assert_eq!(count, r.state.slides as usize, "the deck's slide list was re-read after the edit");
        self.state = Some(r.state);
        match r.outcome {
            w::ApplyOutcome::Err(m) => format!("err:{}", s16(&m)),
            w::ApplyOutcome::Ok(o) => match o {
                w::Outcome::Nothing => "none".into(),
                w::Outcome::Bool(b) => format!("b{}", i32::from(b)),
                w::Outcome::Index(i) => format!("n{i}"),
                w::Outcome::Shape(s) => match s.value {
                    Some(id) => format!("s{}", s16(&id)),
                    None => "s".into(),
                },
                w::Outcome::Count(c) => format!("n{}", to_js_string(c)),
            },
        }
    }

    fn apply_wire(&mut self, op: w::Op) -> String {
        let (p, n) = put(&enc(w::encode_apply, &w::Apply { op }));
        // SAFETY: live handles; a fresh buffer.
        let b = unsafe { uo_slides_editor_apply(self.ed, self.deck, p, n) };
        self.result(b)
    }

    fn set_ui(&mut self, index: u32, sel: Option<&str>) {
        let (p, n) = put(sel.unwrap_or("").as_bytes());
        // SAFETY: live handles; a fresh buffer.
        let b = unsafe { uo_slides_editor_set_ui(self.ed, self.deck, index, i32::from(sel.is_some()), p, n) };
        self.result(b);
    }

    fn st(&self) -> &w::UiState {
        self.state.as_ref().unwrap()
    }

    fn paste_clip(&mut self, clip: &[u8]) -> String {
        let c = w::decode_clip(clip, &mut w::Tables::new(), &w::limits()).unwrap();
        self.apply_wire(w::Op::PasteShape(w::OpPasteShape { clip: c.clip }))
    }
}

/// Copies shape `shape` of slide `slide` of a deck in a second editor: the clip bytes.
fn copy_from(bytes: &[u8], slide: u32, shape: &str) -> Option<Vec<u8>> {
    let mut src = AbiDriver::default();
    src.init(Init::Deck(bytes, "en-US".into()));
    src.set_ui(slide, None);
    let (p, n) = put(shape.as_bytes());
    // SAFETY: live handles; a fresh buffer.
    let (status, data) = take(unsafe { uo_slides_editor_copy_shape(src.ed, src.deck, p, n) });
    assert!(status == 0 || status == 1, "copy: {status}");
    (status == 0).then_some(data)
}

impl Driver for AbiDriver {
    fn init(&mut self, how: Init<'_>) {
        self.close();
        self.clip = None;
        let (bytes, lang) = match how {
            Init::Deck(b, lang) => (b.to_vec(), lang),
            Init::Blank(o) => {
                let lang = o.lang.clone();
                let msg = w::Blank { opts: o.into_wire().unwrap() };
                let (p, n) = put(&enc(w::encode_blank, &msg));
                // SAFETY: a fresh buffer.
                let (status, data) = take(unsafe { uo_slides_editor_blank(p, n) });
                assert_eq!(status, 0);
                (data, lang)
            }
        };
        self.deck = open_deck(&bytes);
        // SAFETY: the live deck.
        self.ed = unsafe { uo_slides_editor_new(self.deck) };
        assert!(!self.ed.is_null());
        // the shell's own copy of the package: the same file read by the shell
        self.mirror = Deck::open(&bytes, Limits::default()).unwrap().parts().clone();
        let (p, n) = put(lang.as_bytes());
        // SAFETY: the live editor; a fresh buffer.
        assert_eq!(unsafe { uo_slides_editor_set_text_lang(self.ed, p, n) }, 0);
        // SAFETY: the live editor.
        assert_eq!(unsafe { uo_slides_editor_set_random_seed(self.ed, 1) }, 0);
        // SAFETY: live handles.
        let b = unsafe { uo_slides_editor_state(self.ed, self.deck) };
        let (status, data) = take(b);
        assert_eq!(status, 0);
        let r = w::decode_edit_result(&data, &mut w::Tables::new(), &w::limits()).unwrap();
        assert!(r.changes.is_empty(), "nothing changed at the start");
        self.state = Some(r.state);
    }
    fn apply(&mut self, op: Op) -> String {
        self.apply_wire(op.into_wire().unwrap())
    }
    fn copy(&mut self, shape: &str) -> String {
        let (p, n) = put(shape.as_bytes());
        // SAFETY: live handles; a fresh buffer.
        let (status, data) = take(unsafe { uo_slides_editor_copy_shape(self.ed, self.deck, p, n) });
        match status {
            0 => {
                self.clip = Some(data);
                "b1".into()
            }
            1 => {
                self.clip = None;
                "b0".into()
            }
            _ => format!("err:{}", String::from_utf8_lossy(&data)),
        }
    }
    fn paste(&mut self) -> String {
        match self.clip.clone() {
            None => "none".into(),
            Some(c) => self.paste_clip(&c),
        }
    }
    fn xpaste(&mut self, deck: &[u8], slide: u32, shape: &str) -> String {
        match copy_from(deck, slide, shape) {
            None => "none".into(),
            Some(c) => {
                self.clip = Some(c.clone());
                self.paste_clip(&c)
            }
        }
    }
    fn select(&mut self, id: Option<&str>) {
        let i = self.st().index;
        self.set_ui(i, id);
    }
    fn goto(&mut self, i: u32) {
        let st = self.st();
        let keep = (st.index == i).then(|| st.selected.as_deref().map(s16)).flatten();
        self.set_ui(i, keep.as_deref());
    }
    fn undo(&mut self) {
        // SAFETY: live handles.
        let b = unsafe { uo_slides_editor_history(self.ed, self.deck, 0) };
        self.result(b);
    }
    fn redo(&mut self) {
        // SAFETY: live handles.
        let b = unsafe { uo_slides_editor_history(self.ed, self.deck, 1) };
        self.result(b);
    }
    fn mark_clean(&mut self) {
        // SAFETY: live handles.
        let b = unsafe { uo_slides_editor_mark_clean(self.ed, self.deck) };
        self.result(b);
    }
    fn embed(&mut self, embed: Option<bool>, subset: Option<bool>) {
        let v = |o: Option<bool>| o.map_or(-1, i32::from);
        // SAFETY: live handles.
        let b = unsafe { uo_slides_editor_set_font_embedding(self.ed, self.deck, v(embed), v(subset)) };
        self.result(b);
    }
    fn save_fonts(&mut self) -> Vec<u8> {
        // SAFETY: live handles.
        let (status, data) = take(unsafe { uo_slides_editor_font_requests(self.ed, self.deck) });
        assert_eq!(status, 0);
        let req = w::decode_font_requests(&data, &mut w::Tables::new(), &w::limits()).unwrap();
        let faces = req
            .requests
            .iter()
            .map(|r| {
                let v = match r.variant {
                    w::Variant::Regular => uo_fonts::Variant::Regular,
                    w::Variant::Bold => uo_fonts::Variant::Bold,
                    w::Variant::Italic => uo_fonts::Variant::Italic,
                    w::Variant::BoldItalic => uo_fonts::Variant::BoldItalic,
                };
                w::OptBytes { value: face_bytes(&s16(&r.family), v).map(|b| b.to_vec()) }
            })
            .collect();
        let (p, n) = put(&enc(w::encode_font_faces, &w::FontFaces { faces }));
        // SAFETY: live handles; a fresh buffer.
        let (status, data) = take(unsafe { uo_slides_editor_save_with_fonts(self.ed, self.deck, p, n) });
        assert_eq!(status, 0, "{}", String::from_utf8_lossy(&data));
        w::decode_save_result(&data, &mut w::Tables::new(), &w::limits()).unwrap().bytes
    }
    fn parts(&self) -> &PartStore {
        &self.mirror
    }
    fn state(&mut self) -> String {
        let s = self.st();
        let sel = s.selected.as_deref().map(s16);
        let ul = s.undo_label.as_deref().map(s16);
        let rl = s.redo_label.as_deref().map(s16);
        state_text(s.index, sel.as_deref(), s.can_undo, s.can_redo, ul.as_deref(), rl.as_deref(), s.dirty)
    }
    fn models(&mut self) -> (String, String) {
        store_model_facets(&self.mirror, self.st().index)
    }
}

/// Scripts per capture in a default run (all of them with `UO_EDIT_ALL=1`; the CI line sets it).
fn budget() -> usize {
    if std::env::var("UO_EDIT_ALL").is_ok() { usize::MAX } else { 6 }
}

fn run(stem: &str) {
    let (n, steps, report) = replay(stem, &mut AbiDriver::default(), budget());
    eprintln!("abi {stem}: {n} scripts, {steps} steps, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(30));
    assert!(n > 0);
}

#[test]
fn abi_samples_scripts_match_the_ts() {
    run("edit-samples");
}

#[test]
fn abi_generated_decks_scripts_match_the_ts() {
    run("edit-b1a+b6a");
}

#[test]
fn abi_blank_scripts_match_the_ts() {
    run("edit-blank");
}

#[test]
fn abi_save_with_fonts_matches_the_ts() {
    run("edit-fonts");
}

#[test]
fn foreign_deck_and_null_handles_are_refused() {
    let bytes = edit_capture::deck_bytes("gen/b1a/0000");
    let a = open_deck(&bytes);
    let b = open_deck(&bytes);
    // SAFETY: live handles of this test, released at the end.
    unsafe {
        let ed = uo_slides_editor_new(a);
        assert!(uo_slides_editor_new(std::ptr::null_mut()).is_null());
        assert_eq!(take(uo_slides_editor_state(ed, b)).0, 11, "another deck");
        assert_eq!(take(uo_slides_editor_state(std::ptr::null_mut(), a)).0, 11);
        assert_eq!(take(uo_slides_editor_history(ed, a, 9)).0, 5);
        let (status, _) = take(uo_slides_editor_apply(ed, a, std::ptr::null_mut(), 0));
        assert_eq!(status, 5, "an empty message");
        let junk = [0x55u8; 40];
        let (p, n) = put(&junk);
        assert_eq!(take(uo_slides_editor_apply(ed, a, p, n)).0, 5, "junk");
        uo_slides_editor_free(ed);
        uo_slides_deck_free(a);
        uo_slides_deck_free(b);
    }
}
