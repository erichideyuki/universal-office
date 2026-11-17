//! The result buffer every byte-returning export shares (moved out of `render.rs` when the Slides module came: the
//! Slides wasm has no use for the paint exports, so the four `uo_buf_*` exports and their helpers live here, behind
//! the feature `buf` that `render` and `slides` both enable).
//!
//! A result is a [`UoBuf`] handle: a status (the space of each module, 0 = ok, 1 = none, 11 = null handle) and its
//! bytes, read through `uo_buf_status`/`uo_buf_ptr`/`uo_buf_len` and released with `uo_buf_free`.

/// A result: a status and its bytes.
pub struct UoBuf {
    status: i32,
    data: Vec<u8>,
}

pub(crate) const OK: i32 = 0;
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(crate) const NONE: i32 = 1;
pub(crate) const NULL_HANDLE: i32 = 11;

pub(crate) fn buf(status: i32, data: Vec<u8>) -> *mut UoBuf {
    Box::into_raw(Box::new(UoBuf { status, data }))
}

/// Little-endian writer of the result layouts.
#[derive(Default)]
pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    pub(crate) fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn u32(&mut self, v: usize) -> &mut Self {
        self.0.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_le_bytes());
        self
    }

    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
}

/// Takes back an input buffer of `uo_alloc_bytes(len)` that JS filled completely.
///
/// # Safety
/// `ptr` must be null or come from one call to `uo_alloc_bytes(len)`, filled with `len` bytes and not
/// released or consumed yet.
pub(crate) unsafe fn take_bytes(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len
    // initialized bytes; it is taken back here exactly once.
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

/// # Safety
/// `b` must be null or a live result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_status(b: *const UoBuf) -> i32 {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(NULL_HANDLE, |b| b.status)
}

/// # Safety
/// `b` must be null or a live result; the pointer is valid until the result is released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_ptr(b: *const UoBuf) -> *const u8 {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(std::ptr::null(), |b| b.data.as_ptr())
}

/// # Safety
/// `b` must be null or a live result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_len(b: *const UoBuf) -> usize {
    // SAFETY: the caller guarantees `b` is null or a live result.
    unsafe { b.as_ref() }.map_or(0, |b| b.data.len())
}

/// # Safety
/// `b` must be null or a live result, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_buf_free(b: *mut UoBuf) {
    if !b.is_null() {
        // SAFETY: the caller guarantees `b` came from this module and is released once.
        drop(unsafe { Box::from_raw(b) });
    }
}
