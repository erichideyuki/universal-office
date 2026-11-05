//! The `unsafe` of the Write exports and of the shared last-error slot under Miri (no font, no layout:
//! Miri runs it in seconds): every consumed input buffer is taken back once, null handles answer,
//! the error info is written to an unaligned byte buffer and the message is copied with a short
//! capacity, a handle is released once. Also runs as a normal test.
//!
//!   cargo +nightly miri test -p uo-wasm --features write --test write_unsafe --target x86_64-unknown-linux-gnu
#![cfg(all(feature = "write", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use uo_wasm::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_clear, uo_last_error_message};
use uo_wasm::render::{uo_buf_free, uo_buf_len, uo_buf_status};
use uo_wasm::write::*;
use uo_wasm::{uo_alloc_bytes, uo_free_bytes};

fn put(b: &[u8]) -> (*mut u8, usize) {
    let p = uo_alloc_bytes(b.len());
    // SAFETY: `p` has room for `b.len()` bytes.
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    (p, b.len())
}

#[test]
fn consumed_buffers_null_handles_and_the_error_slot() {
    // SAFETY: every buffer comes from uo_alloc_bytes and is given to exactly one consuming call; the
    // handles are live until their single free; the error buffers have the documented sizes.
    unsafe {
        let env = uo_write_env_new(1);
        // a faces buffer that is cut short: refused, consumed
        let (fp, fl) = put(b"Carlito");
        let (bp, bl) = put(&[0u8, 0xff, 0xff, 0xff, 0xff, 9, 0, 0, 0, 1, 2]);
        assert_eq!(uo_write_env_push_family(env, fp, fl, 2, 1, bp, bl), code::ARGUMENT);
        // a family with an empty face list is accepted
        let (fp, fl) = put(b"Empty");
        assert_eq!(uo_write_env_push_family(env, fp, fl, 6, 0, std::ptr::null_mut(), 0), code::OK);
        // an asset that is not a font
        let (fp, fl) = put(b"X");
        let (bp, bl) = put(b"not a font file");
        assert_eq!(uo_write_env_push_asset(env, fp, fl, 0, bp, bl, std::ptr::null_mut(), 0), code::INVALID_DATA);
        // the error info through an unaligned byte buffer, the message with a short capacity
        let raw = uo_alloc_bytes(1 + size_of::<UoErrorInfo>());
        assert_eq!(uo_last_error(raw.add(1).cast::<UoErrorInfo>()), code::INVALID_DATA);
        let info = raw.add(1).cast::<UoErrorInfo>().read_unaligned();
        uo_free_bytes(raw, 1 + size_of::<UoErrorInfo>());
        assert_eq!(info.code, code::INVALID_DATA);
        let mut short = [0u8; 4];
        let full = uo_last_error_message(short.as_mut_ptr(), short.len());
        assert_eq!(u64::try_from(full).unwrap(), info.message_len);
        assert_eq!(uo_last_error_message(std::ptr::null_mut(), 99), full);
        uo_last_error_clear();
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::OK);
        // missing families: none asked yet
        let b = uo_write_env_take_missing(env);
        assert_eq!((uo_buf_status(b), uo_buf_len(b)), (0, 0));
        uo_buf_free(b);
        // patterns that do not parse
        let (pp, pl) = put(&[0xff, 0xfe]);
        assert_eq!(uo_write_env_register_hyphenation(env, pp, pl), code::INVALID_DATA);
        // not a docx: null, consumed
        let (dp, dl) = put(b"PK\x03\x04garbage");
        assert!(uo_write_open(dp, dl).is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::INVALID_DATA);
        // layout of a null document: the option buffers are consumed all the same
        let (lp, ll) = put(b"pt-BR");
        let (hp, hl) = put(b"pt-BR\nen");
        assert!(uo_write_layout(std::ptr::null(), env, 0.0, 0, 0, 0.0, lp, ll, hp, hl, 0, 0).is_null());
        assert_eq!(uo_last_error(std::ptr::null_mut()), code::ARGUMENT);
        let (lp, ll) = put(b"en");
        let b = uo_write_hyphenation_patterns(std::ptr::null(), env, lp, ll);
        assert_eq!(uo_buf_status(b), 11);
        uo_buf_free(b);
        // every layout query on a null handle
        for b in [
            uo_layout_navigation(std::ptr::null()),
            uo_layout_warnings(std::ptr::null()),
            uo_layout_missing_faces(std::ptr::null()),
            uo_layout_run_boxes(std::ptr::null(), 0),
            uo_layout_page_info(std::ptr::null(), 0),
            uo_layout_block_line(std::ptr::null(), 0.0),
            uo_layout_face_bytes(std::ptr::null(), 0),
            uo_layout_image_bytes(std::ptr::null(), 0),
        ] {
            assert_eq!(uo_buf_status(b), 11);
            uo_buf_free(b);
        }
        assert_eq!(uo_layout_page_count(std::ptr::null()), 0);
        assert!(uo_layout_view(std::ptr::null_mut()).is_null());
        assert_eq!(uo_layout_pdf(std::ptr::null(), 0, 0), -f64::from(code::ARGUMENT));
        uo_last_error_clear();
        uo_write_env_clear_fonts(env);
        uo_write_env_free(env);
        uo_write_env_free(std::ptr::null_mut());
        uo_write_document_free(std::ptr::null_mut());
        uo_layout_free(std::ptr::null_mut());
    }
}
