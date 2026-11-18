//! The `.ppt` converter as its own wasm module (frontier B7; `uo-slides-ppt.wasm`, feature `slides-ppt`): the viewer
//! loads it only when a user opens a legacy file. It turns the bytes of a `.ppt` (97-2003, a Compound File) into the
//! bytes of the equivalent `.pptx`, which the main module (`slides.rs`) then opens like any other deck: the two
//! modules share no memory, the zip is the interface.
//!
//! `uo_slides_ppt_convert` returns a `UoBuf`: status 0 and the `.pptx` bytes, or status 3 and the message of the
//! converter in UTF-8 (the TS messages: encrypted file, unsupported version, corrupt file).

use uo_pptx::{Deck, Limits};

use crate::buf::{OK, UoBuf, buf, take_bytes};

const FORMAT: i32 = 3;

/// Converts a `.ppt` to `.pptx` bytes.
///
/// # Safety
/// `ptr`/`len` must come from one call to `uo_alloc_bytes(len)`, filled with `len` bytes and not consumed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_ppt_convert(ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let bytes = unsafe { take_bytes(ptr, len) };
    convert(&bytes, &uo_pptx::ppt::ConvertOptions::default())
}

/// [`uo_slides_ppt_convert`] with options (Slides wave 5): `flags` bit 0 = the TS converter's rules
/// (`ConvertOptions::ts_strict`: the parity harnesses; the default is PowerPoint's, e.g. embedded charts converted natively).
///
/// # Safety
/// As [`uo_slides_ppt_convert`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_slides_ppt_convert_with(ptr: *mut u8, len: usize, flags: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len bytes.
    let bytes = unsafe { take_bytes(ptr, len) };
    let o = if flags & 1 != 0 { uo_pptx::ppt::ConvertOptions::ts_strict() } else { uo_pptx::ppt::ConvertOptions::default() };
    convert(&bytes, &o)
}

fn convert(bytes: &[u8], o: &uo_pptx::ppt::ConvertOptions) -> *mut UoBuf {
    let out = uo_pptx::ppt::convert_with(bytes, o).and_then(|parts| Deck::from_parts(parts, Limits::default())).and_then(|d| d.save());
    match out {
        Ok(zip) => buf(OK, zip),
        Err(e) => buf(FORMAT, e.to_string().into_bytes()),
    }
}
