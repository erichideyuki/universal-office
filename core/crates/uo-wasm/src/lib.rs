//! WebAssembly exports of the Rust core for the JS products (browser and Node).
//!
//! Foundation skeleton: it proves the boundary before the first module — opaque handles
//! owned by Rust and released by JS (`free()` or a `FinalizationRegistry`), byte buffers
//! moved in through [`uo_alloc_bytes`], and the memory counters the M2/M4 tests read. The
//! `UoDocument` here is a placeholder resource; the real API arrives with the modules. In
//! wasm a panic aborts the module (the `wasm` profile uses `panic = "abort"`), so code
//! reached from these exports must not panic on input.

#[cfg(feature = "render")]
pub mod render;

#[cfg(feature = "render-fixture")]
pub mod render_fixture;

// the read path shared with uo-ffi (one file, two bindings: uo-bindings.md) and the Write exports
#[cfg(feature = "write")]
#[path = "../../uo-ffi/src/pipeline.rs"]
pub mod pipeline;
#[cfg(feature = "write")]
pub mod write;

// the editing session (milestone 2): src/session.rs and src/session/
#[cfg(feature = "session")]
pub mod session;

// the self-test of the session replays the committed D2a/D2b captures inside the module through the
// command scripts of the native parity tests: their support modules are included here under the names
// they refer to each other by (`crate::support`, `crate::d2a_support`...); their `#[test]` functions
// would run again as tests of this crate, so they are built for the module only
#[cfg(all(feature = "session-selftest", not(test)))]
#[path = "../../uo-docx/tests/support/mod.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod support;
#[cfg(all(feature = "session-selftest", not(test)))]
#[path = "../../uo-docx/tests/d2a_support/mod.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod d2a_support;
#[cfg(all(feature = "session-selftest", not(test)))]
#[path = "../../uo-docx/tests/d2b_f1_support/mod.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod d2b_f1_support;
#[cfg(all(feature = "session-selftest", not(test)))]
#[path = "../../uo-docx/tests/d2b_f2_support/mod.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod d2b_f2_support;
#[cfg(all(feature = "session-selftest", not(test)))]
#[path = "../../uo-docx/tests/d2b_f6_support/mod.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod d2b_f6_support;

#[cfg(feature = "jsnum-memtest")]
pub mod jsnum_memtest;

#[cfg(feature = "jsnum-selftest")]
pub mod jsnum_selftest;

#[cfg(feature = "fonts-memtest")]
pub mod fonts_memtest;

#[cfg(feature = "fonts-selftest")]
pub mod fonts_selftest;
// the scenarios and hostile fonts of the uo-fonts native memory test, compiled here too
#[cfg(feature = "fonts-memtest")]
#[path = "../../uo-fonts/tests/memory_support/mod.rs"]
mod fonts_memory_support;

#[cfg(feature = "dml-memtest")]
pub mod dml_memtest;

// the self-test includes the replay functions of the native parity tests of uo-drawingml, whose
// `#[test]` functions would run again as tests of this crate: it is built for the module only
#[cfg(all(feature = "dml-selftest", not(test)))]
pub mod dml_selftest;

#[cfg(feature = "opc-memtest")]
pub mod opc_memtest;

// the uo-opc benchmark (stage 5): its scenarios and the presets of the uo-opc tests
#[cfg(feature = "opc-bench")]
#[path = "../../uo-opc/tests/bench_support/mod.rs"]
mod bench_support;
#[cfg(feature = "opc-bench")]
#[path = "../../uo-opc/tests/memory_support/mod.rs"]
mod memory_support;
#[cfg(feature = "opc-bench")]
pub mod opc_bench;
#[cfg(feature = "opc-bench")]
#[path = "../../uo-opc/tests/xml_support/mod.rs"]
mod xml_support;

/// Placeholder resource owned by Rust (replaced by the real document type).
#[derive(Debug, Default)]
pub struct UoDocument {
    bytes: Vec<u8>,
}

/// Reserves `len` bytes for JS to write into; ownership passes back to Rust when the buffer
/// is given to an export that consumes it ([`uo_document_append_owned`]) or to
/// [`uo_free_bytes`].
#[unsafe(no_mangle)]
pub extern "C" fn uo_alloc_bytes(len: usize) -> *mut u8 {
    let mut v = std::mem::ManuallyDrop::new(Vec::<u8>::with_capacity(len));
    v.as_mut_ptr()
}

/// Releases a buffer from [`uo_alloc_bytes`] that was not consumed.
///
/// # Safety
/// `ptr`/`len` must come from one call to [`uo_alloc_bytes`] and not be released yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len).
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn uo_document_new() -> *mut UoDocument {
    Box::into_raw(Box::new(UoDocument::default()))
}

/// Appends a buffer from [`uo_alloc_bytes`] (filled with `len` bytes) and releases it.
///
/// # Safety
/// `doc` must be a live handle; `ptr`/`len` a buffer from `uo_alloc_bytes(len)` that JS
/// filled completely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_append_owned(doc: *mut UoDocument, ptr: *mut u8, len: usize) -> i32 {
    if ptr.is_null() {
        return 1;
    }
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len
    // initialized bytes; it is taken back here exactly once.
    let data = unsafe { Vec::from_raw_parts(ptr, len, len) };
    // SAFETY: the caller guarantees `doc` is null or a live handle.
    let Some(doc) = (unsafe { doc.as_mut() }) else { return 1 };
    doc.bytes.extend_from_slice(&data);
    0
}

/// # Safety
/// `doc` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_len(doc: *const UoDocument) -> usize {
    // SAFETY: the caller guarantees `doc` is null or a live handle.
    unsafe { doc.as_ref() }.map_or(0, |d| d.bytes.len())
}

/// # Safety
/// `doc` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_free(doc: *mut UoDocument) {
    if !doc.is_null() {
        // SAFETY: the caller guarantees `doc` came from uo_document_new and is released once.
        drop(unsafe { Box::from_raw(doc) });
    }
}

/// Live bytes of the module, or -1 when this build does not count (feature `count`).
#[unsafe(no_mangle)]
pub extern "C" fn uo_memory_live_bytes() -> f64 {
    #[cfg(feature = "count")]
    return uo_alloc::global_stats().1 as f64;
    #[cfg(not(feature = "count"))]
    -1.0
}

/// Test-only (count build, wasm32): executes `unreachable`, the trap a panic (`panic = abort`) or an
/// allocation failure ends in, so the JS tests can check that the viewer drops the instance
/// (`js/viewer.ts`, `__trapForTest`). Never in the shipped module.
#[cfg(all(feature = "count", target_arch = "wasm32"))]
#[unsafe(no_mangle)]
pub extern "C" fn uo_test_trap() {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_and_documents_round_trip() {
        let doc = uo_document_new();
        let p = uo_alloc_bytes(3);
        // SAFETY: `p` holds 3 bytes from uo_alloc_bytes; it is filled and handed to
        // uo_document_append_owned, which takes it back; `doc` is live until freed.
        unsafe {
            std::ptr::copy_nonoverlapping([7u8, 8, 9].as_ptr(), p, 3);
            assert_eq!(uo_document_append_owned(doc, p, 3), 0);
            assert_eq!(uo_document_len(doc), 3);
            uo_document_free(doc);
        }
    }

    #[test]
    fn an_unused_buffer_is_released() {
        let p = uo_alloc_bytes(64);
        // SAFETY: `p` came from uo_alloc_bytes(64) and was not consumed.
        unsafe { uo_free_bytes(p, 64) };
    }

    #[test]
    fn null_handles_are_tolerated() {
        // SAFETY: null handles are part of the documented contract.
        unsafe {
            assert_eq!(uo_document_len(std::ptr::null()), 0);
            uo_document_free(std::ptr::null_mut());
            uo_free_bytes(std::ptr::null_mut(), 0);
        }
    }
}
