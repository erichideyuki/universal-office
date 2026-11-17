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
    let out = uo_pptx::ppt::convert(&bytes).and_then(|parts| Deck::from_parts(parts, Limits::default())).and_then(|d| d.save());
    match out {
        Ok(zip) => buf(OK, zip),
        Err(e) => buf(FORMAT, e.to_string().into_bytes()),
    }
}
