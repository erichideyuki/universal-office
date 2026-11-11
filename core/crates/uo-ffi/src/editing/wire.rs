//! The packed little-endian format of lists and records, in and out, and the raw-pointer readers
//! every export uses (the only place that turns `(ptr, len)` into a slice).
//!
//! - string: `u32 len` (`0xFFFF_FFFF` = null) and that many UTF-8 bytes;
//! - string list: `u32 count`, then the strings;
//! - record lists: `u32 count`, then each record's fields in the order the export documents
//!   (`u32`, `u8` and strings);
//! - an INPUT list whose length is `usize::MAX` is a null list (`None`): null is never signalled
//!   through the pointer (the empty array pins to a null pointer in .NET).
//!
//! Nothing trusts a length: a count larger than the bytes left is refused before any allocation.

use uo_docx::api::ApiError;

use super::super::{UoBuf, buf_out, bytes_in};

type Res<T> = Result<T, ApiError>;

/// `len` value that means "null" for strings and lists coming IN.
pub const NULL_LEN: usize = usize::MAX;
/// The null string of the wire.
pub const NULL_STR: u32 = u32::MAX;

/// Builds a packed buffer.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    /// A count (saturating at `u32::MAX - 1`).
    pub fn count(&mut self, n: usize) -> &mut Self {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX - 1))
    }
    pub fn str(&mut self, s: Option<&str>) -> &mut Self {
        match s {
            None => self.u32(NULL_STR),
            Some(s) => {
                self.count(s.len());
                self.buf.extend_from_slice(s.as_bytes());
                self
            }
        }
    }
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Reads a packed buffer (bounds-checked).
#[derive(Debug)]
pub struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

fn trunc() -> ApiError {
    ApiError::argument("truncated or malformed list")
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(trunc)?;
        let s = self.b.get(self.at..end).ok_or_else(trunc)?;
        self.at = end;
        Ok(s)
    }
    pub fn u32(&mut self) -> Res<u32> {
        let s = self.take(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(s);
        Ok(u32::from_le_bytes(a))
    }
    pub fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }
    /// A count of items that take at least `min_item` bytes each.
    pub fn count(&mut self, min_item: usize) -> Res<usize> {
        let n = self.u32()? as usize;
        if n.checked_mul(min_item.max(1)).is_none_or(|need| need > self.remaining()) {
            return Err(trunc());
        }
        Ok(n)
    }
    pub fn str_opt(&mut self) -> Res<Option<&'a str>> {
        let n = self.u32()?;
        if n == NULL_STR {
            return Ok(None);
        }
        let s = self.take(n as usize)?;
        std::str::from_utf8(s).map(Some).map_err(|_| ApiError::argument("invalid UTF-8"))
    }
    pub fn str(&mut self) -> Res<&'a str> {
        self.str_opt()?.ok_or_else(|| ApiError::argument("null string in a list"))
    }
    /// A string list whose items may not be null.
    pub fn str_list(&mut self) -> Res<Vec<&'a str>> {
        let n = self.count(4)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.str()?);
        }
        Ok(v)
    }
    /// `(name, value?)` pairs.
    pub fn pairs(&mut self) -> Res<Vec<(&'a str, Option<&'a str>)>> {
        let n = self.count(8)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.str()?;
            v.push((k, self.str_opt()?));
        }
        Ok(v)
    }
}

// ───────────────────────── raw pointers (the contract of every export) ─────────────────────────
//
// Every helper below is `unsafe fn`: it relies on the contract of the exports (module doc of
// `editing`, `# Safety` of each export): each `(ptr, len)` is null or readable for the call, each
// handle is null or live (and unique for `rm`), each `out` is null or writable. The exports are
// `unsafe extern "C" fn`, so the caller of an export is the one who promises it.

/// `(ptr, len)` as bytes; a null pointer with a length is an `ArgumentException`.
///
/// # Safety
/// `p` is null or readable for `n` bytes for `'a` (`n == NULL_LEN` is refused before any read).
pub unsafe fn raw<'a>(p: *const u8, n: usize) -> Res<&'a [u8]> {
    if n == NULL_LEN {
        return Err(ApiError::argument("null buffer"));
    }
    // SAFETY: forwarded from the caller (`p` is readable for `n` bytes).
    unsafe { bytes_in(p, n) }.ok_or_else(|| ApiError::argument("null buffer"))
}

/// A required UTF-8 string.
///
/// # Safety
/// As [`raw`].
pub unsafe fn utf8<'a>(p: *const u8, n: usize) -> Res<&'a str> {
    // SAFETY: forwarded from the caller.
    std::str::from_utf8(unsafe { raw(p, n) }?).map_err(|_| ApiError::argument("invalid UTF-8"))
}

/// An optional UTF-8 string (`len == usize::MAX` is null).
///
/// # Safety
/// As [`raw`].
pub unsafe fn utf8_opt<'a>(p: *const u8, n: usize) -> Res<Option<&'a str>> {
    if n == NULL_LEN {
        Ok(None)
    } else {
        // SAFETY: forwarded from the caller.
        unsafe { utf8(p, n) }.map(Some)
    }
}

/// An optional packed string list (`len == usize::MAX` is null).
///
/// # Safety
/// As [`raw`].
pub unsafe fn list_opt<'a>(p: *const u8, n: usize) -> Res<Option<Vec<&'a str>>> {
    if n == NULL_LEN {
        return Ok(None);
    }
    // SAFETY: forwarded from the caller.
    let mut r = Reader::new(unsafe { raw(p, n) }?);
    let v = r.str_list()?;
    Ok(Some(v))
}

/// A required packed string list.
///
/// # Safety
/// As [`raw`].
pub unsafe fn list<'a>(p: *const u8, n: usize) -> Res<Vec<&'a str>> {
    // SAFETY: forwarded from the caller.
    unsafe { list_opt(p, n) }?.ok_or_else(|| ApiError::argument("null list"))
}

/// A reference to a handle or struct the caller owns (null is `ArgumentException`).
///
/// # Safety
/// `p` is null or points to a live `T` for `'a`.
pub unsafe fn rf<'a, T>(p: *const T) -> Res<&'a T> {
    // SAFETY: forwarded from the caller.
    unsafe { p.as_ref() }.ok_or_else(|| ApiError::argument("null handle"))
}

/// A unique reference to a handle (null is `ArgumentException`).
///
/// # Safety
/// `p` is null or the only live reference to a `T` for `'a` (the facade locks each document).
pub unsafe fn rm<'a, T>(p: *mut T) -> Res<&'a mut T> {
    // SAFETY: forwarded from the caller.
    unsafe { p.as_mut() }.ok_or_else(|| ApiError::argument("null handle"))
}

/// An optional reference (null is `None`).
///
/// # Safety
/// As [`rf`].
pub unsafe fn rf_opt<'a, T>(p: *const T) -> Option<&'a T> {
    // SAFETY: forwarded from the caller.
    unsafe { p.as_ref() }
}

/// Copies up to `cap` bytes into the caller's buffer (null `buf` copies nothing); returns the full length.
///
/// # Safety
/// `buf` is null or writable for `cap` bytes.
pub unsafe fn copy_out(src: &[u8], buf: *mut u8, cap: usize) -> usize {
    if !buf.is_null() {
        let n = src.len().min(cap);
        // SAFETY: the caller guarantees `buf` has `cap` writable bytes; `n <= cap`; no overlap (`src` is Rust-owned).
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), buf, n) };
    }
    src.len()
}

/// A struct that starts with its size: refused when the caller's struct is shorter than ours.
///
/// # Safety
/// As [`rf`].
pub unsafe fn sized<'a, T>(p: *const T, size_of_t: usize, size: impl FnOnce(&T) -> u32) -> Res<&'a T> {
    // SAFETY: forwarded from the caller.
    let t = unsafe { rf(p) }?;
    if (size(t) as usize) < size_of_t {
        return Err(ApiError::argument("options struct is shorter than expected"));
    }
    Ok(t)
}

/// Writes a plain value to an output slot.
///
/// # Safety
/// `out` is null or writable (alignment is not required).
pub unsafe fn put<T>(out: *mut T, v: T) -> Res<()> {
    if out.is_null() {
        return Err(ApiError::argument("null output"));
    }
    // SAFETY: `out` is non-null and writable by the caller's guarantee.
    unsafe { out.write_unaligned(v) };
    Ok(())
}

/// Hands a buffer out (`UoBuf`, freed by `uo_buf_free`).
///
/// # Safety
/// `out` is null or writable.
pub unsafe fn out_buf(out: *mut *mut UoBuf, data: Vec<u8>) -> Res<()> {
    if out.is_null() {
        return Err(ApiError::argument("null output"));
    }
    // `buf_out` is safe: `out` is non-null (checked above) and writable by the caller's guarantee.
    buf_out(out, data);
    Ok(())
}

/// Hands a buffer out, or leaves the slot null (the C# `null`).
///
/// # Safety
/// `out` is null or writable.
pub unsafe fn out_buf_opt(out: *mut *mut UoBuf, data: Option<Vec<u8>>) -> Res<()> {
    if out.is_null() {
        return Err(ApiError::argument("null output"));
    }
    match data {
        Some(d) => buf_out(out, d),
        // SAFETY: as above.
        None => unsafe { out.write(std::ptr::null_mut()) },
    }
    Ok(())
}

/// An optional string out: the raw UTF-8 bytes, or no buffer.
///
/// # Safety
/// As [`out_buf_opt`].
pub unsafe fn out_str_opt(out: *mut *mut UoBuf, s: Option<String>) -> Res<()> {
    // SAFETY: forwarded from the caller.
    unsafe { out_buf_opt(out, s.map(String::into_bytes)) }
}

/// Releases a handle the export created (null is a no-op).
///
/// # Safety
/// `p` is null or came from `Box::into_raw` of this library and is released once.
pub unsafe fn drop_box<T>(p: *mut T) {
    if !p.is_null() {
        // SAFETY: the caller guarantees `p` came from `Box::into_raw` and is released once.
        drop(unsafe { Box::from_raw(p) });
    }
}

/// Hands a new handle out.
///
/// # Safety
/// `out` is null or writable.
pub unsafe fn out_handle<T>(out: *mut *mut T, v: T) -> Res<()> {
    if out.is_null() {
        return Err(ApiError::argument("null output"));
    }
    // SAFETY: `out` is non-null and writable by the caller's guarantee.
    unsafe { out.write(Box::into_raw(Box::new(v))) };
    Ok(())
}

/// A count as the `int32` of the facade (saturating: decision D3A-11).
pub fn count_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_strings_pairs_and_nulls() {
        let mut w = Writer::new();
        w.count(3).str(Some("a\nb")).str(Some("")).str(Some("ç\u{1F600}"));
        let b = w.finish();
        let mut r = Reader::new(&b);
        assert_eq!(r.str_list().unwrap(), vec!["a\nb", "", "ç\u{1F600}"]);
        assert_eq!(r.remaining(), 0);
        let mut w = Writer::new();
        w.count(2).str(Some("k")).str(None).str(Some("j")).str(Some("v"));
        let b = w.finish();
        assert_eq!(Reader::new(&b).pairs().unwrap(), vec![("k", None), ("j", Some("v"))]);
    }

    #[test]
    fn truncated_and_huge_inputs_are_refused_before_allocating() {
        assert!(Reader::new(&[1, 0]).str_list().is_err());
        assert!(Reader::new(&[0xFF, 0xFF, 0xFF, 0x7F]).str_list().is_err());
        assert!(Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 1]).pairs().is_err());
        let mut w = Writer::new();
        w.count(1).u32(100);
        assert!(Reader::new(&w.finish()).str_list().is_err());
        assert!(Reader::new(&[1, 0, 0, 0, 2, 0, 0, 0, 0xFF, 0xFE]).str_list().is_err());
        assert!(Reader::new(&[1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]).str_list().is_err());
    }

    #[test]
    fn null_lengths_and_null_pointers() {
        // SAFETY: null pointers are only passed with a null length or a zero length (no read); `[0xFF]` is readable for 1 byte.
        unsafe {
            assert_eq!(utf8_opt(std::ptr::null(), NULL_LEN).unwrap(), None);
            assert_eq!(utf8(std::ptr::null(), 0).unwrap(), "");
            assert!(utf8(std::ptr::null(), 3).is_err());
            assert!(utf8(std::ptr::null(), NULL_LEN).is_err());
            assert!(utf8([0xFFu8].as_ptr(), 1).is_err());
            assert!(list_opt(std::ptr::null(), NULL_LEN).unwrap().is_none());
        }
    }
}
