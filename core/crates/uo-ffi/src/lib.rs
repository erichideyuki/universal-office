//! C ABI of the Rust core for the .NET facade.
//!
//! Foundation skeleton: it proves the boundary before the first module — opaque handles
//! owned by Rust and released by the caller, buffers passed as pointer + length, panics
//! caught before they cross the ABI (unwinding into .NET is undefined behaviour), and the
//! memory counters the M4 tests read. The `UoDocument` here is a placeholder resource; the
//! real API arrives with the modules.
//!
//! Rules for every export:
//! - handles are created by `uo_*_new` and released ONLY by `uo_*_free` (the .NET side wraps
//!   them in a `SafeHandle`, so both `Dispose()` and the finalizer end there);
//! - a null handle or buffer is an error status, never a crash;
//! - every body runs inside [`guard`].

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Result of an export, as an `int` on the C side.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UoStatus {
    Ok = 0,
    NullArgument = 1,
    /// a panic was caught at the boundary (a bug in the core)
    Panic = 2,
}

/// Placeholder resource owned by Rust (replaced by the real document type).
#[derive(Debug, Default)]
pub struct UoDocument {
    bytes: Vec<u8>,
}

fn guard(f: impl FnOnce() -> UoStatus) -> UoStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(UoStatus::Panic)
}

/// Version of the core (static, NUL-terminated).
#[unsafe(no_mangle)]
pub extern "C" fn uo_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}

/// New handle; release it with [`uo_document_free`].
#[unsafe(no_mangle)]
pub extern "C" fn uo_document_new() -> *mut UoDocument {
    Box::into_raw(Box::new(UoDocument::default()))
}

/// Appends `len` bytes from `data` to the document.
///
/// # Safety
/// `doc` must come from [`uo_document_new`] and not be freed; `data` must point to `len`
/// readable bytes (or be null with `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_append(doc: *mut UoDocument, data: *const u8, len: usize) -> UoStatus {
    guard(|| {
        // SAFETY: the caller guarantees `doc` is a live handle from uo_document_new.
        let Some(doc) = (unsafe { doc.as_mut() }) else { return UoStatus::NullArgument };
        if len == 0 {
            return UoStatus::Ok;
        }
        if data.is_null() {
            return UoStatus::NullArgument;
        }
        // SAFETY: the caller guarantees `data` points to `len` readable bytes.
        let src = unsafe { std::slice::from_raw_parts(data, len) };
        doc.bytes.extend_from_slice(src);
        UoStatus::Ok
    })
}

/// Number of bytes in the document (0 for a null handle).
///
/// # Safety
/// `doc` must be null or a live handle from [`uo_document_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_len(doc: *const UoDocument) -> usize {
    // SAFETY: the caller guarantees `doc` is null or a live handle.
    unsafe { doc.as_ref() }.map_or(0, |d| d.bytes.len())
}

/// Releases a handle (null is ignored).
///
/// # Safety
/// `doc` must be null or a live handle from [`uo_document_new`], released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_document_free(doc: *mut UoDocument) {
    if !doc.is_null() {
        // SAFETY: the caller guarantees `doc` came from Box::into_raw in uo_document_new and
        // is released once.
        drop(unsafe { Box::from_raw(doc) });
    }
}

/// 1 when this build counts memory (feature `count`), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn uo_memory_counting() -> i32 {
    #[cfg(feature = "count")]
    return i32::from(uo_alloc::is_counting());
    #[cfg(not(feature = "count"))]
    0
}

/// Live bytes of the whole library (all threads: .NET finalizers run on their own thread),
/// or -1 when this build does not count.
#[unsafe(no_mangle)]
pub extern "C" fn uo_memory_live_bytes() -> i64 {
    #[cfg(feature = "count")]
    return i64::try_from(uo_alloc::global_stats().1).unwrap_or(i64::MAX);
    #[cfg(not(feature = "count"))]
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_arguments_are_errors_not_crashes() {
        // SAFETY: null handles are part of the documented contract.
        unsafe {
            assert_eq!(uo_document_append(std::ptr::null_mut(), std::ptr::null(), 1), UoStatus::NullArgument);
            assert_eq!(uo_document_len(std::ptr::null()), 0);
            uo_document_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn a_panic_does_not_cross_the_boundary() {
        assert_eq!(guard(|| panic!("bug")), UoStatus::Panic);
    }

    #[test]
    fn append_and_free() {
        let d = uo_document_new();
        let data = [1u8, 2, 3];
        // SAFETY: `d` is live and `data` has 3 bytes.
        unsafe {
            assert_eq!(uo_document_append(d, data.as_ptr(), data.len()), UoStatus::Ok);
            assert_eq!(uo_document_len(d), 3);
            uo_document_free(d);
        }
    }
}
