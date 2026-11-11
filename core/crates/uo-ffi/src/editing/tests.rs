//! Unit tests of the Editing exports that need the crate's private items (they also run under Miri;
//! the parse-heavy ones are ignored there).

use super::error::with_doc;
use super::handles::UoDocx;
use super::*;
use crate::pipeline::{UoErrorInfo, code, uo_last_error};

fn minimal_docx() -> Vec<u8> {
    crate::tests::minimal_docx()
}

fn last_code() -> i32 {
    let mut info = UoErrorInfo::default();
    // SAFETY: `info` is writable.
    unsafe { uo_last_error(&raw mut info) }
}

#[test]
fn every_free_accepts_null() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        uo_docx_free(std::ptr::null_mut());
        uo_body_writer_free(std::ptr::null_mut());
        uo_row_template_free(std::ptr::null_mut());
        uo_stage_free(std::ptr::null_mut());
        uo_sample_set_free(std::ptr::null_mut());
        uo_paragraph_props_free(std::ptr::null_mut());
    }
}

#[test]
fn null_and_short_arguments_are_errors_not_crashes() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut out = std::ptr::null_mut();
        assert_eq!(uo_docx_open(std::ptr::null(), 4, std::ptr::null(), &raw mut out), code::ARGUMENT);
        assert_eq!(uo_docx_open([].as_ptr(), 0, std::ptr::null(), std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_docx_save(std::ptr::null_mut(), std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_docx_run(std::ptr::null_mut(), 0), code::ARGUMENT);
        assert_eq!(uo_body_writer_line_is_empty(std::ptr::null(), std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_docx_native_bytes(std::ptr::null()), 0);
        assert_eq!(uo_sample_set_count(std::ptr::null()), 0);
        assert_eq!(uo_last_error_limit(std::ptr::null_mut()), -1);
        assert_eq!(uo_last_error_part(std::ptr::null_mut(), 0), 0);
        uo_editing_error_clear();
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_panic_poisons_the_document_handle() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let mut d = std::ptr::null_mut();
        let bytes = minimal_docx();
        assert_eq!(uo_docx_open(bytes.as_ptr(), bytes.len(), std::ptr::null(), &raw mut d), code::OK);
        assert_eq!(with_doc(d, |_: &mut UoDocx| -> Result<(), uo_docx::api::ApiError> { panic!("bug") }), code::INTERNAL);
        assert_eq!(last_code(), code::INTERNAL);
        // every later call is refused, not run on a half-changed document
        assert_eq!(uo_docx_run(d, 1), code::INVALID_OPERATION);
        let mut out = std::ptr::null_mut();
        assert_eq!(uo_docx_save(d, &raw mut out), code::INVALID_OPERATION);
        assert!(out.is_null());
        uo_docx_free(d);
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn the_limit_of_a_deep_document_reaches_the_detail_slot() {
    // SAFETY: every pointer passed is null with a null/zero length, a live buffer of this test, or a handle this test created and still owns; outs are writable locals.
    unsafe {
        let deep = include_bytes!("../../../uo-docx/tests/parity/d3/fixtures/deep-257.docx");
        let mut d = std::ptr::null_mut();
        assert_eq!(uo_docx_open(deep.as_ptr(), deep.len(), std::ptr::null(), &raw mut d), code::LIMIT_EXCEEDED);
        let mut li = UoLimitInfo { limit: 0, reserved: 0, part_len: 0 };
        assert_eq!(uo_last_error_limit(&raw mut li), 2);
        assert_eq!(li.part_len, "word/document.xml".len() as u64);
        let mut name = vec![0u8; usize::try_from(li.part_len).unwrap()];
        assert_eq!(uo_last_error_part(name.as_mut_ptr(), name.len()), name.len());
        assert_eq!(name, b"word/document.xml");
        // the next call clears it
        assert_ne!(uo_docx_open([].as_ptr(), 0, std::ptr::null(), &raw mut d), code::OK);
        assert_eq!(uo_last_error_limit(&raw mut li), -1);
        assert_eq!(li.part_len, u64::MAX);
        uo_editing_error_clear();
    }
}
