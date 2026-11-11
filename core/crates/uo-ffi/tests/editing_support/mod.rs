//! Safe wrappers over the Editing exports, for the editing_* tests (not a test target).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::PathBuf;

use uo_ffi::UoBuf;
use uo_ffi::editing::*;
use uo_ffi::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_message};
use uo_ffi::{uo_buf_free, uo_buf_len, uo_buf_ptr};

pub const NULL_LEN: usize = usize::MAX;

pub fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture_dir() -> PathBuf {
    core().join("crates/uo-docx/tests/parity/d3/fixtures")
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_dir().join(format!("{name}.docx"))).unwrap()
}

pub fn fixture_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "docx").then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    v.sort();
    v
}

/// `(ptr, len)` of a string.
pub fn s(x: &str) -> (*const u8, usize) {
    (x.as_ptr(), x.len())
}

/// The null string.
pub fn null() -> (*const u8, usize) {
    (std::ptr::null(), NULL_LEN)
}

/// The packed string list.
pub fn pack(items: &[&str]) -> Vec<u8> {
    let mut v = (items.len() as u32).to_le_bytes().to_vec();
    for i in items {
        v.extend((i.len() as u32).to_le_bytes());
        v.extend(i.as_bytes());
    }
    v
}

/// The packed `(name, value?)` pairs.
pub fn pairs(items: &[(&str, Option<&str>)]) -> Vec<u8> {
    let mut v = (items.len() as u32).to_le_bytes().to_vec();
    for (k, val) in items {
        v.extend((k.len() as u32).to_le_bytes());
        v.extend(k.as_bytes());
        match val {
            Some(x) => {
                v.extend((x.len() as u32).to_le_bytes());
                v.extend(x.as_bytes());
            }
            None => v.extend(u32::MAX.to_le_bytes()),
        }
    }
    v
}

/// Reads a packed string list.
pub fn unpack(b: &[u8]) -> Vec<String> {
    let n = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
    let mut at = 4;
    let mut out = Vec::new();
    for _ in 0..n {
        let l = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        out.push(String::from_utf8(b[at..at + l].to_vec()).unwrap());
        at += l;
    }
    out
}

/// Copies a buffer and frees it.
pub fn take(b: *mut UoBuf) -> Vec<u8> {
    // SAFETY: a live buffer from an export, released once here.
    unsafe {
        let v = std::slice::from_raw_parts(uo_buf_ptr(b), uo_buf_len(b)).to_vec();
        uo_buf_free(b);
        v
    }
}

pub fn take_str(b: *mut UoBuf) -> String {
    String::from_utf8(take(b)).unwrap()
}

/// `(code, message)` of the last error of this thread.
pub fn last() -> (i32, String) {
    let mut info = UoErrorInfo::default();
    // SAFETY: `info` is writable; the message buffer has its full length.
    unsafe {
        let c = uo_last_error(&raw mut info);
        let mut msg = vec![0u8; usize::try_from(info.message_len).unwrap()];
        uo_last_error_message(msg.as_mut_ptr(), msg.len());
        (c, String::from_utf8(msg).unwrap())
    }
}

/// `(limit ordinal, max, part)` of the last error.
pub fn last_limit() -> (i32, u64, Option<String>) {
    let mut info = UoErrorInfo::default();
    let mut li = UoLimitInfo { limit: 0, reserved: 0, part_len: 0 };
    // SAFETY: writable out structs.
    let l = unsafe {
        uo_last_error(&raw mut info);
        uo_last_error_limit(&raw mut li)
    };
    let part = if li.part_len == u64::MAX {
        None
    } else {
        let mut b = vec![0u8; li.part_len as usize];
        // SAFETY: `b` is writable for its length.
        unsafe { uo_last_error_part(b.as_mut_ptr(), b.len()) };
        Some(String::from_utf8(b).unwrap())
    };
    (l, info.max, part)
}

pub fn open(bytes: &[u8]) -> Result<*mut UoDocx, i32> {
    open_with(bytes, std::ptr::null())
}

pub fn open_with(bytes: &[u8], limits: *const UoDocxLimits) -> Result<*mut UoDocx, i32> {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut d = std::ptr::null_mut();
        let c = uo_docx_open(bytes.as_ptr(), bytes.len(), limits, &raw mut d);
        if c == code::OK { Ok(d) } else { Err(c) }
    }
}

pub fn save(d: *mut UoDocx) -> Result<Vec<u8>, i32> {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut b = std::ptr::null_mut();
        let c = uo_docx_save(d, &raw mut b);
        if c == code::OK { Ok(take(b)) } else { Err(c) }
    }
}

pub fn sanitize_opts(flags: u32, allowed_fields: &[u8], schemes: &[u8]) -> UoSanitizeOptions {
    UoSanitizeOptions {
        size: size_of::<UoSanitizeOptions>() as u32,
        flags,
        allowed_fields: Str { ptr: allowed_fields.as_ptr(), len: if allowed_fields.is_empty() { NULL_LEN } else { allowed_fields.len() } },
        allowed_schemes: Str { ptr: schemes.as_ptr(), len: schemes.len() },
    }
}

/// The flags of `DocxSanitizeOptions.Content` (the C# defaults).
pub const CONTENT_FLAGS: u32 = 0x3FF;

pub fn defaults(which: u32) -> Vec<u8> {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut b = std::ptr::null_mut();
        assert_eq!(uo_editing_defaults(which, &raw mut b), code::OK);
        take(b)
    }
}
