//! Test-only exports (feature `fonts-memtest`) for the wasm memory gate of `uo-fonts` (stage 4):
//! `tests/fonts_memory.test.mjs` moves the open fonts and the hyphenation pattern files into the
//! module, runs the soaks of the native test (`uo-fonts/tests/memory_support`, the same code) and the
//! worst-case fonts, and samples `uo_memory_live_bytes` and `memory.buffer.byteLength`. These are not
//! the binding API of `uo-fonts`; never in the shipped module.

use std::cell::RefCell;
use std::sync::Arc;

use uo_alloc::measure;
use uo_fonts::{Face, FontLimits, Tag, standalone_face, subset_true_type};

use crate::fonts_memory_support::{self as support, Corpus, Engine};

struct State {
    faces: Vec<(String, Vec<u8>)>,
    patterns: Vec<String>,
    corpus: Option<Corpus>,
    engine: Option<Engine>,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State { faces: Vec::new(), patterns: Vec::new(), corpus: None, engine: None }) };
}

/// Takes back a buffer from `uo_alloc_bytes(len)` that JS filled.
///
/// # Safety
/// `ptr`/`len` must be a buffer from one call to `uo_alloc_bytes(len)`, completely filled and not released.
unsafe fn take(ptr: *mut u8, len: usize) -> Vec<u8> {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) (capacity len) and holds len initialized bytes.
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

/// Drops the corpus, the engine and the buffers moved in.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_reset() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.faces = Vec::new();
        s.patterns = Vec::new();
        s.corpus = None;
        s.engine = None;
    });
}

/// Adds an open font file (consumes the buffer).
///
/// # Safety
/// `ptr`/`len` must be a buffer from `uo_alloc_bytes(len)`, completely filled and not released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_memtest_add_face(ptr: *mut u8, len: usize) {
    // SAFETY: forwarded from the contract of this function.
    let bytes = unsafe { take(ptr, len) };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let name = format!("face{}", s.faces.len());
        s.faces.push((name, bytes));
    });
}

/// Adds a hyphenation pattern file, UTF-8 (consumes the buffer); false when it is not UTF-8.
///
/// # Safety
/// `ptr`/`len` must be a buffer from `uo_alloc_bytes(len)`, completely filled and not released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_memtest_add_pattern(ptr: *mut u8, len: usize) -> u32 {
    // SAFETY: forwarded from the contract of this function.
    let bytes = unsafe { take(ptr, len) };
    match String::from_utf8(bytes) {
        Ok(text) => {
            STATE.with(|s| s.borrow_mut().patterns.push(text));
            1
        }
        Err(_) => 0,
    }
}

/// Builds the corpus from what was added (the open fonts, then six generated faces and a `.ttc`);
/// the moved-in bytes are kept once, in the corpus. Returns the number of faces.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_build_corpus() -> u32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let faces = std::mem::take(&mut s.faces);
        let patterns = std::mem::take(&mut s.patterns);
        let corpus = Corpus::new(faces, patterns);
        let n = corpus.faces.len() as u32;
        s.corpus = Some(corpus);
        n
    })
}

/// One round of the fresh scenario (everything created, used and dropped); a number that depends
/// on the work so it is not optimised away.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_fresh_round(round: u32) -> f64 {
    STATE.with(|s| s.borrow().corpus.as_ref().map_or(-1.0, |c| (support::fresh_round(c, round as usize) & 0xFFFF_FFFF) as f64))
}

/// Builds the long-lived engine (faces, hyphenators and sessions kept).
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_engine_new() -> u32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let engine = s.corpus.as_ref().map(Engine::new);
        let ok = engine.is_some();
        s.engine = engine;
        u32::from(ok)
    })
}

/// One round of the long-lived scenario.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_engine_round() -> f64 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let State { corpus, engine, .. } = &mut *s;
        match (corpus.as_ref(), engine.as_mut()) {
            (Some(c), Some(e)) => (e.round(c) & 0xFFFF_FFFF) as f64,
            _ => -1.0,
        }
    })
}

/// Words held by the sessions of the long-lived engine.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_engine_cached_words() -> f64 {
    STATE.with(|s| s.borrow().engine.as_ref().map_or(-1.0, |e| e.cached_words() as f64))
}

/// Drops the long-lived engine.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_engine_drop() {
    STATE.with(|s| s.borrow_mut().engine = None);
}

type HostileOp = fn(&[u8], &FontLimits) -> u64;

/// Names of the hostile fonts, by `kind`.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_hostile_count() -> u32 {
    support::HOSTILE.len() as u32
}

/// Builds hostile font `kind` (see `memory_support::HOSTILE`), runs the operation that makes it
/// expensive with the wasm32 limits, drops everything and returns the peak live bytes above the
/// start (the font is built before the measure). -1 for an unknown kind.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_memtest_hostile(kind: u32) -> f64 {
    let limits = FontLimits::WASM32;
    let (bytes, op): (Vec<u8>, HostileOp) = match kind {
        0 => (support::hostile_gsub(), |b, l| {
            Face::parse(Arc::from(b.to_vec()), l).map_or(0, |f| f.ligate(&[1, 2], &[Tag(*b"liga")]).map_or(0, |v| v.len() as u64))
        }),
        1 => (support::hostile_class0_gpos(), |b, l| Face::parse(Arc::from(b.to_vec()), l).map_or(0, |f| f.kern_units(1, 2) as u64)),
        2 => (support::pair_map_font(1024), |b, l| Face::parse(Arc::from(b.to_vec()), l).map_or(0, |f| f.kern_units(1, 2) as u64)),
        3 => (support::hostile_outline_font(4, 16, 65_535), |b, l| {
            Face::parse(Arc::from(b.to_vec()), l).map_or(0, |f| (2..6u32).map(|g| f.glyph_outline(g).map_or(0, |o| o.len() as u64)).sum())
        }),
        4 => (support::hostile_subset_font(2_000), |b, l| subset_true_type(b, 0..2_000u32, false, l).len() as u64),
        5 => (support::hostile_ttc(400), |b, l| standalone_face(b, None, l).map_or(0, |f| f.len() as u64)),
        _ => return -1.0,
    };
    let (r, usage) = measure(|| op(&bytes, &limits));
    std::hint::black_box(r);
    usage.peak_above_start as f64
}
