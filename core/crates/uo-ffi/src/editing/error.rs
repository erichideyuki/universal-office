//! Errors of the Editing exports: the code and message go to the shared last-error slot
//! ([`crate::pipeline`]); the `Limit` and the part name, which the 24-byte `UoErrorInfo` cannot
//! carry, go to a detail slot of this module (`uo_last_error_limit` / `uo_last_error_part`).

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use uo_docx::api::{ApiError, ErrorKind, Limit};

use super::handles::UoDocx;
use super::wire::{copy_out, put, rm};
use crate::pipeline::{self, LastError, code, stage};

#[derive(Default)]
struct Detail {
    /// the C# `OfficeLimit` ordinal, -1 when the last error is not a limit
    limit: i32,
    part: Option<String>,
}

thread_local! {
    static DETAIL: RefCell<Detail> = const { RefCell::new(Detail { limit: -1, part: None }) };
}

fn clear_detail() {
    DETAIL.with(|d| {
        let mut d = d.borrow_mut();
        d.limit = -1;
        // dropped, not cleared: no allocation kept between calls
        d.part = None;
    });
}

/// The ordinal of a limit (the order of the C# `OfficeLimit`).
fn ordinal(l: Limit) -> i32 {
    match l {
        Limit::PartBytes => 0,
        Limit::TotalBytes => 1,
        Limit::XmlDepth => 2,
        Limit::FieldsPerStory => 3,
        Limit::FieldNesting => 4,
        Limit::Entries => 5,
        Limit::XmlElements => 6,
    }
}

/// Records `e` (code, message, maximum, limit, part) and returns its code.
pub fn fail_api(e: &ApiError) -> i32 {
    let (max, limit, part) = match &e.kind {
        ErrorKind::LimitExceeded { limit, max, part } => (*max, ordinal(*limit), part.clone()),
        _ => (0, -1, None),
    };
    DETAIL.with(|d| *d.borrow_mut() = Detail { limit, part });
    pipeline::set_last(LastError { code: e.code(), stage: stage::NONE, max, message: e.message.clone() })
}

/// Runs an export body: clears both error slots, catches a panic, records an `ApiError`.
pub fn api(f: impl FnOnce() -> Result<(), ApiError>) -> i32 {
    clear_detail();
    super::super::guard(|| match f() {
        Ok(()) => code::OK,
        Err(e) => fail_api(&e),
    })
}

/// Runs an export body over a document: a null handle is `ArgumentException`, a handle poisoned
/// by an earlier internal error is `InvalidOperationException`, and a panic in the body poisons it.
///
/// # Safety
/// `d` is null or the only live reference to a `UoDocx` for the call (the export contract).
pub unsafe fn with_doc(d: *mut UoDocx, f: impl FnOnce(&mut UoDocx) -> Result<(), ApiError>) -> i32 {
    clear_detail();
    super::super::guard(|| {
        // SAFETY: forwarded from the caller.
        let u = match unsafe { rm(d) } {
            Ok(u) => u,
            Err(e) => return fail_api(&e),
        };
        if u.poisoned {
            return fail_api(&ApiError::invalid_operation("the document handle was poisoned by an earlier internal error"));
        }
        match catch_unwind(AssertUnwindSafe(|| f(u))) {
            Ok(Ok(())) => code::OK,
            Ok(Err(e)) => fail_api(&e),
            Err(_) => {
                u.poisoned = true;
                pipeline::fail_with(code::INTERNAL, "internal error (a panic was caught at the boundary)")
            }
        }
    })
}

/// The limit of the last Editing error (`UoLimitInfo` on the C side).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UoLimitInfo {
    /// the C# `OfficeLimit` ordinal, or -1
    pub limit: i32,
    pub reserved: i32,
    /// bytes of the part name (`uo_last_error_part`), `u64::MAX` = no part
    pub part_len: u64,
}

/// The limit and the part name length of the last error of this thread.
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_last_error_limit(out: *mut UoLimitInfo) -> i32 {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        DETAIL.with(|d| {
            let d = d.borrow();
            let info = UoLimitInfo {
                limit: d.limit,
                reserved: 0,
                part_len: d.part.as_ref().map_or(u64::MAX, |p| u64::try_from(p.len()).unwrap_or(u64::MAX)),
            };
            match put(out, info) {
                Ok(()) => d.limit,
                Err(_) => -1,
            }
        })
    }
}

/// Copies up to `cap` bytes of the part name into `buf`; returns its length (0 when there is none).
///
/// # Safety
/// The module contract: each `(ptr, len)` is null or readable for `len` bytes during the call, each handle is null
/// or live and used by one thread at a time, each output slot is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_last_error_part(buf: *mut u8, cap: usize) -> usize {
    // SAFETY: the caller upholds the export contract (# Safety), which is what the wire helpers rely on.
    unsafe {
        DETAIL.with(|d| {
            let d = d.borrow();
            d.part.as_ref().map_or(0, |p| copy_out(p.as_bytes(), buf, cap))
        })
    }
}

/// Clears BOTH error slots of this thread (the pipeline slot and the detail) and drops their strings.
#[unsafe(no_mangle)]
pub extern "C" fn uo_editing_error_clear() {
    pipeline::clear_last();
    clear_detail();
}
