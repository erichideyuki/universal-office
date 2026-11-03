//! Parity self-test of `uo-fonts` inside the wasm module (feature `fonts-selftest`, stage 4): the
//! module recomputes the facets of the stage-4 captures (`uo-fonts/tests/parity/fonts.cap` and
//! `legacy_kern.cap`) with the same code the native replay uses (`fonts_parity_support/facets.rs`
//! and `legacy.rs`, included with `#[path]`: one copy), and `tests/fonts_selftest.test.mjs`
//! compares them with the capture files, in Node and in the browser engines. The generated fonts
//! are built inside the module (the generators of the native tests are included too); the real fonts
//! come from JS, through buffers it owns (`uo_alloc_bytes`). Not the binding API of `uo-fonts`;
//! never in the shipped module.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::expect_used)]

use std::sync::Arc;

use uo_fonts::{Hyphenator, hyphen::parse_patterns};
use uo_testkit::Stream;

#[allow(
    clippy::duplicate_mod,
    reason = "test-only: with fonts-memtest also on, the memory support includes the same uo-fonts test file; each feature stays self-contained"
)]
#[path = "../../uo-fonts/tests/b_support/mod.rs"]
mod b_support;
#[path = "../../uo-fonts/tests/fonts_parity_support/facets.rs"]
mod facets;
#[allow(
    clippy::duplicate_mod,
    reason = "test-only: with fonts-memtest also on, the memory support includes the same uo-fonts test file; each feature stays self-contained"
)]
#[path = "../../uo-fonts/tests/gpos_support/mod.rs"]
mod gpos_support;
#[path = "../../uo-fonts/tests/fonts_parity_support/legacy.rs"]
mod legacy;

/// The generated fonts of the capture whose depth is `full`: `gen/b/*` (the quirk fonts of frontier B)
/// and `gen/c/*` (the kerning and ligature fonts of frontier C), in the corpus order.
fn generated() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = b_support::quirk_fonts().into_iter().map(|(n, b)| (format!("gen/b/{n}"), b)).collect();
    out.extend(gpos_support::all_named().into_iter().map(|c| (format!("gen/c/{}", c.name), c.bytes)));
    out
}

/// The G16 corpus built in the module: the generated legacy-kern cases and the quirk fonts (the open
/// fonts of `legacy_kern.cap` come from JS).
fn legacy_generated() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> =
        gpos_support::legacy_cases().into_iter().map(|c| (format!("legacy/{}", c.name), c.bytes)).collect();
    out.extend(b_support::quirk_fonts().into_iter().map(|(n, b)| (format!("quirk/{n}"), b)));
    out
}

fn write_out(text: &str, out: *mut u8, cap: u32) -> u32 {
    let bytes = text.as_bytes();
    if bytes.len() > cap as usize || out.is_null() {
        return 0;
    }
    // SAFETY: `out` is a JS-provided buffer of `cap` bytes (`uo_alloc_bytes(cap)`) and `bytes.len() <= cap`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
    bytes.len() as u32
}

fn input<'a>(ptr: *const u8, len: u32) -> &'a [u8] {
    if ptr.is_null() {
        return &[];
    }
    // SAFETY: the caller guarantees `ptr` points to `len` initialized bytes that stay valid for the call.
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }
}

fn hang_list(ptr: *const u8, len: u32) -> Vec<usize> {
    std::str::from_utf8(input(ptr, len)).unwrap_or("").split(',').filter_map(|n| n.parse().ok()).collect()
}

fn lines(facets: Vec<(String, String)>) -> String {
    facets.into_iter().map(|(f, v)| format!("{f}\t{v}\n")).collect()
}

/// Number of generated fonts of the self-test.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_selftest_generated_count() -> u32 {
    generated().len() as u32
}

/// The capture key of generated font `index`; 0 when there is none or it does not fit.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_generated_key(index: u32, out: *mut u8, cap: u32) -> u32 {
    generated().get(index as usize).map_or(0, |(k, _)| write_out(k, out, cap))
}

/// The facets of generated font `index` at full depth, one `facet<TAB>value` line each; `hang` is the
/// comma list of the capture's `subset.hang` (cases the TS never returns from).
///
/// # Safety
/// `hang` must be null or point to `hang_len` initialized bytes; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_generated_facets(index: u32, hang: *const u8, hang_len: u32, out: *mut u8, cap: u32) -> u32 {
    let Some((_, bytes)) = generated().into_iter().nth(index as usize) else { return 0 };
    let bytes: Arc<[u8]> = Arc::from(bytes);
    write_out(&lines(facets::font_facets(&bytes, &facets::FULL, &hang_list(hang, hang_len), &[])), out, cap)
}

/// The facets of the font bytes JS moved in, at full (`light` 0) or light depth.
///
/// # Safety
/// `ptr`/`hang` must be null or point to initialized bytes of the given lengths; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_font_facets(
    ptr: *const u8,
    len: u32,
    light: u32,
    hang: *const u8,
    hang_len: u32,
    out: *mut u8,
    cap: u32,
) -> u32 {
    let bytes: Arc<[u8]> = Arc::from(input(ptr, len));
    let depth = if light == 0 { facets::FULL } else { facets::LIGHT };
    write_out(&lines(facets::font_facets(&bytes, &depth, &hang_list(hang, hang_len), &[])), out, cap)
}

/// The G16 facet (`legacykern` of `legacy_kern.cap`) of the font bytes.
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_legacy(ptr: *const u8, len: u32, out: *mut u8, cap: u32) -> u32 {
    let bytes: Arc<[u8]> = Arc::from(input(ptr, len));
    write_out(&legacy::legacy_facet(&bytes), out, cap)
}

/// The `fields` facet of a hyphenation data file (the text of `data/hyphen/<id>.txt`).
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_hyphen_fields(ptr: *const u8, len: u32, out: *mut u8, cap: u32) -> u32 {
    let text = String::from_utf8_lossy(input(ptr, len)).into_owned();
    let Some(p) = parse_patterns(&text) else { return 0 };
    let mut f = Stream::new();
    f.str(p.id).u32(p.left).u32(p.right);
    let pats: Vec<&str> = tokens(p.patterns);
    let excs: Vec<&str> = tokens(p.exceptions);
    f.u64(pats.len() as u64);
    for t in &pats {
        f.str(t);
    }
    f.u64(excs.len() as u64);
    for t in &excs {
        f.str(t);
    }
    let items = f.items();
    write_out(&format!("{items}:{}", f.finish().short()), out, cap)
}

/// The digest of chunk `chunk` (250 words) of the words (one per line) hyphenated with the data
/// file: points and `hyphenate` for minimums 1..=3 (the `chunkN` facets of `hyphen/<id>`).
///
/// # Safety
/// `file`/`words` must be null or point to initialized bytes of the given lengths; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_hyphen_chunk(
    file: *const u8,
    file_len: u32,
    words: *const u8,
    words_len: u32,
    chunk: u32,
    out: *mut u8,
    cap: u32,
) -> u32 {
    let text = String::from_utf8_lossy(input(file, file_len)).into_owned();
    let Some(p) = parse_patterns(&text) else { return 0 };
    let h = Hyphenator::new(&p);
    let all = String::from_utf8_lossy(input(words, words_len)).into_owned();
    let all: Vec<&str> = all.split('\n').collect();
    let Some(group) = all.chunks(250).nth(chunk as usize) else { return 0 };
    let mut st = Stream::new();
    for w in group {
        st.tag(b'w');
        let points = h.points(w);
        st.u32(points.len() as u32);
        for p in &points {
            st.u32(*p);
        }
        for l in 1..=3 {
            for r in 1..=3 {
                let hy = h.hyphenate(w, l, r);
                st.u32(hy.len() as u32);
                for p in &hy {
                    st.u32(*p);
                }
            }
        }
    }
    let items = st.items();
    write_out(&format!("{items}:{}", st.finish().short()), out, cap)
}

/// Number of generated fonts of the G16 self-test.
#[unsafe(no_mangle)]
pub extern "C" fn uo_fonts_selftest_legacy_count() -> u32 {
    legacy_generated().len() as u32
}

/// The key of generated G16 font `index` in `legacy_kern.cap`.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_legacy_key(index: u32, out: *mut u8, cap: u32) -> u32 {
    legacy_generated().get(index as usize).map_or(0, |(k, _)| write_out(k, out, cap))
}

/// The G16 facet of generated font `index`.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_fonts_selftest_legacy_generated(index: u32, out: *mut u8, cap: u32) -> u32 {
    let Some((_, bytes)) = legacy_generated().into_iter().nth(index as usize) else { return 0 };
    write_out(&legacy::legacy_facet(&Arc::from(bytes)), out, cap)
}

/// The tokens of a pattern or exception list: the data files separate them by spaces and line breaks.
fn tokens(text: &str) -> Vec<&str> {
    text.split([' ', '\n', '\t']).filter(|t| !t.is_empty()).collect()
}
