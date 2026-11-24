//! The Sheet exports of the C ABI (record uo-bindings.md, "Sheet .NET facade"): contract (null handles, short structs, status
//! and last error), open/info/read/edit/undo/save/`.xls` conversion, the native digests of the fixture workbooks the C# facade
//! also checks (`tests/parity/sheet-native.tsv`), hostile input (truncation and bit flips, noise, a lying zip, a `.xls` that is
//! not one), threads, and the memory gate: M4 (handles created and released in loops), M1 (every export leaves nothing once
//! its buffer is freed).
//!
//! `UO_BLESS=1` rewrites `tests/parity/sheet-native.tsv`; `UO_SHEET_M4=<n>` changes the handle loop.

#![cfg(all(not(target_arch = "wasm32"), feature = "sheet"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::{Path, PathBuf};

use uo_alloc::assert_no_leak_global;
use uo_ffi::pipeline::{UoErrorInfo, code, uo_last_error, uo_last_error_clear};
use uo_ffi::sheet::{
    CLEAR_CONTENTS, MERGE_ALLOW_DATA_LOSS, MERGE_CELLS, READ_FORMULAS, READ_STYLES, UoWorkbook, UoXlsxOptions, XLSX_TRUSTED,
    uo_xls_to_xlsx, uo_xlsx_add_sheet, uo_xlsx_clear, uo_xlsx_free, uo_xlsx_info, uo_xlsx_is_legacy, uo_xlsx_mark_saved, uo_xlsx_merge,
    uo_xlsx_new, uo_xlsx_open, uo_xlsx_read_range, uo_xlsx_recalculate, uo_xlsx_redo, uo_xlsx_save, uo_xlsx_set_cell_text,
    uo_xlsx_set_number_format, uo_xlsx_undo, uo_xlsx_unmerge,
};
use uo_ffi::{UoBuf, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_cancel_free, uo_cancel_new, uo_cancel_raise};

/// The tests read the global live bytes: one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn workbooks_dir() -> PathBuf {
    repo().join("core/crates/uo-xlsx/tests/captures/workbooks")
}

/// The clock and seed every parity run uses (the C# tests pass the same).
const NOW: f64 = 45672.5;
const YEAR: f64 = 2025.0;
const SEED: u64 = 1;

// ───────────────────────────── fixtures ─────────────────────────────

/// `(key, path)` of the fixture workbooks: the 30 `x1` workbooks, the committed samples (not the giant ones), the `.xls` group.
fn fixtures() -> Vec<(String, PathBuf)> {
    let root = repo();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |dir: PathBuf, exts: &[&str], skip: &[&str]| {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        for p in names {
            let ok = p.extension().is_some_and(|e| exts.iter().any(|x| e == *x));
            let skipped = p.file_name().is_some_and(|n| skip.iter().any(|s| n.to_string_lossy().starts_with(s)));
            if ok && !skipped {
                let key = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((key, p));
            }
        }
    };
    add(workbooks_dir().join("x1"), &["xlsx"], &[]);
    add(root.join("sheet/samples"), &["xlsx"], &["giant"]);
    add(workbooks_dir().join("xls"), &["xls"], &[]);
    out
}

fn small_book() -> Vec<u8> {
    std::fs::read(workbooks_dir().join("x1/x1-0000.xlsx")).unwrap()
}

fn digest(bytes: &[u8]) -> String {
    let h = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(bytes));
    format!("{}:{}", bytes.len(), &h[..32])
}

// ───────────────────────────── the ABI, called from Rust ─────────────────────────────

fn opts(flags: u32, lang: &str) -> UoXlsxOptions {
    UoXlsxOptions {
        size: u32::try_from(size_of::<UoXlsxOptions>()).unwrap(),
        flags,
        lang: lang.as_ptr(),
        lang_len: lang.len(),
        now_serial: NOW,
        year: YEAR,
        seed: SEED,
    }
}

fn last_error(status: i32) -> i32 {
    let mut info = UoErrorInfo::default();
    // SAFETY: `info` is writable.
    assert_eq!(unsafe { uo_last_error(&raw mut info) }, status);
    assert!(info.message_len > 0, "status {status} without a message");
    uo_last_error_clear();
    status
}

/// A buffer taken as bytes and released; on a failure the last error must say something.
fn take(status: i32, out: *mut UoBuf) -> Result<Vec<u8>, i32> {
    if status != code::OK {
        assert!(out.is_null(), "an output on failure {status}");
        return Err(last_error(status));
    }
    // SAFETY: a live buffer, released once.
    unsafe {
        let v = std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)).to_vec();
        uo_buf_free(out);
        Ok(v)
    }
}

fn open(data: &[u8], flags: u32, lang: &str) -> Result<*mut UoWorkbook, i32> {
    let o = opts(flags, lang);
    let mut w: *mut UoWorkbook = std::ptr::null_mut();
    // SAFETY: readable data, valid options, writable output.
    let s = unsafe { uo_xlsx_open(data.as_ptr(), data.len(), &raw const o, std::ptr::null(), &raw mut w) };
    if s == code::OK {
        assert!(!w.is_null());
        Ok(w)
    } else {
        assert!(w.is_null());
        Err(last_error(s))
    }
}

fn info(w: *const UoWorkbook) -> Vec<u8> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live workbook, writable output.
    take(unsafe { uo_xlsx_info(w, &raw mut out) }, out).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn read(w: *const UoWorkbook, sheet: u32, c1: u32, r1: u32, c2: u32, r2: u32, flags: u32) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live workbook, writable output.
    let s = unsafe { uo_xlsx_read_range(w, sheet, c1, r1, c2, r2, flags, std::ptr::null(), &raw mut out) };
    take(s, out)
}

fn save(w: *const UoWorkbook) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live workbook, writable output.
    take(unsafe { uo_xlsx_save(w, std::ptr::null(), &raw mut out) }, out)
}

fn set_text(w: *const UoWorkbook, sheet: u32, col: u32, row: u32, text: &str) -> Result<Vec<u8>, i32> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    // SAFETY: a live workbook, readable text, writable output.
    take(unsafe { uo_xlsx_set_cell_text(w, sheet, col, row, text.as_ptr(), text.len(), &raw mut out) }, out)
}

// the little reader of the info buffer
struct Rd<'a>(&'a [u8], usize);

impl Rd<'_> {
    fn u8(&mut self) -> u8 {
        self.1 += 1;
        self.0[self.1 - 1]
    }
    fn u32(&mut self) -> u32 {
        self.1 += 4;
        u32::from_le_bytes(self.0[self.1 - 4..self.1].try_into().unwrap())
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32();
        if n == u32::MAX {
            return None;
        }
        self.1 += n as usize;
        Some(String::from_utf8(self.0[self.1 - n as usize..self.1].to_vec()).unwrap())
    }
}

/// `(name, used_rows, used_cols)` of the sheets of an info buffer.
fn sheets_of(info: &[u8]) -> Vec<(String, u32, u32)> {
    let mut r = Rd(info, 0);
    assert_eq!(r.u32(), 1);
    r.u32();
    for _ in 0..4 {
        r.u8();
    }
    r.str();
    r.str();
    let n = r.u32();
    (0..n)
        .map(|_| {
            let name = r.str().unwrap();
            r.u8();
            let (rows, cols) = (r.u32(), r.u32());
            r.u32();
            r.u32();
            r.u8();
            let m = r.u32();
            for _ in 0..m {
                r.str();
            }
            (name, rows, cols)
        })
        .collect()
}

fn cell_count(buf: &[u8]) -> u32 {
    u32::from_le_bytes(buf[..4].try_into().unwrap())
}

// ───────────────────────────── contract ─────────────────────────────

#[test]
fn null_arguments_and_short_structs_are_errors_not_crashes() {
    let _s = serial();
    let data = small_book();
    let o = opts(0, "pt-BR");
    let mut w: *mut UoWorkbook = std::ptr::null_mut();
    let mut buf: *mut UoBuf = std::ptr::null_mut();
    let mut did = 0u8;
    // SAFETY: null handles are part of the documented contract; the rest are live and valid.
    unsafe {
        assert_eq!(uo_xlsx_open(data.as_ptr(), data.len(), std::ptr::null(), std::ptr::null(), &raw mut w), code::ARGUMENT);
        assert_eq!(uo_xlsx_open(data.as_ptr(), data.len(), &raw const o, std::ptr::null(), std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_xlsx_open(std::ptr::null(), 5, &raw const o, std::ptr::null(), &raw mut w), code::ARGUMENT);
        let mut short = o;
        short.size = 4;
        assert_eq!(uo_xlsx_open(data.as_ptr(), data.len(), &raw const short, std::ptr::null(), &raw mut w), code::ARGUMENT);
        let mut bad_lang = o;
        bad_lang.lang = [0xFF_u8, 0xFE].as_ptr();
        bad_lang.lang_len = 2;
        assert_eq!(uo_xlsx_open(data.as_ptr(), data.len(), &raw const bad_lang, std::ptr::null(), &raw mut w), code::ARGUMENT);
        let mut bad_clock = o;
        bad_clock.now_serial = f64::NAN;
        assert_eq!(uo_xlsx_new(&raw const bad_clock, &raw mut w), code::ARGUMENT);
        assert!(w.is_null());
        assert_eq!(uo_xlsx_new(std::ptr::null(), &raw mut w), code::ARGUMENT);
        assert_eq!(uo_xlsx_info(std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_read_range(std::ptr::null(), 0, 1, 1, 2, 2, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_save(std::ptr::null(), std::ptr::null(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_set_cell_text(std::ptr::null(), 0, 1, 1, b"x".as_ptr(), 1, &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_set_number_format(std::ptr::null(), 0, 1, 1, 1, 1, b"0".as_ptr(), 1), code::ARGUMENT);
        assert_eq!(uo_xlsx_merge(std::ptr::null(), 0, 1, 1, 2, 2, 0, 0), code::ARGUMENT);
        assert_eq!(uo_xlsx_unmerge(std::ptr::null(), 0, 1, 1, 2, 2), code::ARGUMENT);
        assert_eq!(uo_xlsx_clear(std::ptr::null(), 0, 1, 1, 2, 2, 0), code::ARGUMENT);
        assert_eq!(uo_xlsx_add_sheet(std::ptr::null(), std::ptr::null(), 0, &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_undo(std::ptr::null(), &raw mut did), code::ARGUMENT);
        assert_eq!(uo_xlsx_redo(std::ptr::null(), &raw mut did), code::ARGUMENT);
        assert_eq!(uo_xlsx_recalculate(std::ptr::null()), code::ARGUMENT);
        assert_eq!(uo_xlsx_mark_saved(std::ptr::null()), code::ARGUMENT);
        assert_eq!(uo_xlsx_is_legacy(std::ptr::null()), -1);
        assert_eq!(uo_xls_to_xlsx(data.as_ptr(), data.len(), std::ptr::null(), &raw mut buf), code::ARGUMENT);
        uo_xlsx_free(std::ptr::null_mut());
    }
    uo_last_error_clear();
    // live handle: null outputs and bad arguments
    let w = open(&data, 0, "pt-BR").unwrap();
    // SAFETY: a live workbook; null outputs and out-of-range arguments are documented refusals.
    unsafe {
        assert_eq!(uo_xlsx_info(w, std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_xlsx_undo(w, std::ptr::null_mut()), code::ARGUMENT);
        assert_eq!(uo_xlsx_read_range(w, 99, 1, 1, 2, 2, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_read_range(w, 0, 0, 1, 2, 2, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_read_range(w, 0, 3, 1, 2, 2, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_read_range(w, 0, 1, 1, 16_385, 2, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_read_range(w, 0, 1, 1, 2, 1_048_577, 0, std::ptr::null(), &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_set_cell_text(w, 0, 1, 1, std::ptr::null(), 3, &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_set_cell_text(w, 0, 1, 1, [0xFF_u8].as_ptr(), 1, &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_set_cell_text(w, 7, 1, 1, b"x".as_ptr(), 1, &raw mut buf), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_set_number_format(w, 0, 1, 1, 1, 1, b"".as_ptr(), 0), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_merge(w, 0, 1, 1, 2, 2, 9, 0), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_clear(w, 0, 1, 1, 2, 2, 9), code::ARGUMENT_OUT_OF_RANGE);
        assert_eq!(uo_xlsx_add_sheet(w, [0xC3_u8].as_ptr(), 1, &raw mut buf), code::ARGUMENT);
        let long = "x".repeat(32);
        assert_eq!(uo_xlsx_add_sheet(w, long.as_ptr(), long.len(), &raw mut buf), code::ARGUMENT);
        assert_eq!(uo_xlsx_is_legacy(w), 0);
        uo_xlsx_free(w);
    }
    // the entry of a text beyond Excel's 32,767 characters
    let w = open(&data, 0, "pt-BR").unwrap();
    assert_eq!(set_text(w, 0, 1, 1, &"a".repeat(32_768)), Err(code::ARGUMENT_OUT_OF_RANGE));
    // SAFETY: released once.
    unsafe { uo_xlsx_free(w) };
}

#[test]
fn garbage_is_refused_with_the_right_code_and_a_message() {
    let _s = serial();
    for (what, bytes, want) in [
        ("empty", Vec::new(), code::INVALID_DATA),
        ("noise", vec![1u8, 2, 3, 4], code::INVALID_DATA),
        (
            "a zip that is not a workbook",
            uo_opc::zip::write([("a.txt", b"x".as_slice())], uo_opc::zip::WriteOptions::default()).unwrap(),
            code::INVALID_DATA,
        ),
        ("a CFB signature and nothing", vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1], code::INVALID_DATA),
    ] {
        assert_eq!(open(&bytes, 0, "").map(|_| ()), Err(want), "{what}");
        assert_eq!(open(&bytes, XLSX_TRUSTED, "").map(|_| ()), Err(want), "{what}");
    }
    // a .docx is not a workbook
    let docx = std::fs::read(repo().join("core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx")).unwrap();
    assert_eq!(open(&docx, 0, "").map(|_| ()), Err(code::INVALID_DATA));
    // .xls conversion refuses a .xlsx and noise
    let o = opts(0, "");
    let mut out: *mut UoBuf = std::ptr::null_mut();
    let x = small_book();
    // SAFETY: readable data, valid options, writable output.
    unsafe {
        assert_eq!(take(uo_xls_to_xlsx(x.as_ptr(), x.len(), &raw const o, &raw mut out), out), Err(code::INVALID_DATA));
        assert_eq!(take(uo_xls_to_xlsx([1u8].as_ptr(), 1, &raw const o, &raw mut out), out), Err(code::INVALID_DATA));
    }
}

#[test]
fn a_raised_flag_cancels_before_the_work_and_before_the_bytes_are_handed_over() {
    let _s = serial();
    let data = small_book();
    let o = opts(0, "pt-BR");
    let k = uo_cancel_new();
    // SAFETY: live handles, each released once; valid data, options and outputs.
    unsafe {
        uo_cancel_raise(k);
        let mut w: *mut UoWorkbook = std::ptr::null_mut();
        assert_eq!(uo_xlsx_open(data.as_ptr(), data.len(), &raw const o, k, &raw mut w), code::CANCELLED);
        assert!(w.is_null());
        uo_last_error_clear();
        let live = open(&data, 0, "pt-BR").unwrap();
        let mut out: *mut UoBuf = std::ptr::null_mut();
        assert_eq!(take(uo_xlsx_read_range(live, 0, 1, 1, 5, 5, 0, k, &raw mut out), out), Err(code::CANCELLED));
        assert_eq!(take(uo_xlsx_save(live, k, &raw mut out), out), Err(code::CANCELLED));
        // without the flag the same calls work, and the handle is intact
        assert!(read(live, 0, 1, 1, 5, 5, 0).is_ok());
        assert!(save(live).is_ok());
        uo_xlsx_free(live);
        uo_cancel_free(k);
    }
}

// ───────────────────────────── behaviour ─────────────────────────────

#[test]
fn a_workbook_reports_its_sheets_reads_cells_edits_and_saves_a_workbook_that_opens_again() {
    let _s = serial();
    let w = open(&small_book(), 0, "pt-BR").unwrap();
    let sheets = sheets_of(&info(w));
    assert!(!sheets.is_empty());
    // the entry rules of the language: a formula in Portuguese, a decimal comma, a refusal
    let ok = set_text(w, 0, 1, 1, "=SOMA(2;3)").unwrap();
    assert_eq!(ok[0], 1, "the formula was refused");
    let r = read(w, 0, 1, 1, 1, 1, READ_FORMULAS | READ_STYLES).unwrap();
    assert_eq!(cell_count(&r), 1);
    let text = String::from_utf8_lossy(&r);
    assert!(text.contains("=SOMA(2;3)") && text.contains("=SUM(2,3)"), "local and canonical formula: {text}");
    assert!(set_text(w, 0, 2, 2, "12,5").unwrap()[0] == 1);
    let refused = set_text(w, 0, 3, 3, "=SOMA(").unwrap();
    assert_eq!(refused[0], 0, "an invalid formula must be refused");
    // undo / redo bring the first entry back and forth
    let mut did = 0u8;
    // SAFETY: a live workbook, writable output.
    unsafe {
        assert_eq!(uo_xlsx_undo(w, &raw mut did), code::OK);
        assert_eq!(did, 1);
        assert_eq!(uo_xlsx_undo(w, &raw mut did), code::OK);
        assert_eq!(did, 1);
        assert_eq!(uo_xlsx_redo(w, &raw mut did), code::OK);
        assert_eq!(did, 1);
    }
    // the saved workbook opens again in the other language with the same cells
    let saved = save(w).unwrap();
    assert_eq!(&saved[..2], b"PK");
    let again = open(&saved, 0, "en-US").unwrap();
    assert_eq!(sheets_of(&info(again)).len(), sheets.len());
    let r = read(again, 0, 1, 1, 1, 1, READ_FORMULAS).unwrap();
    assert!(String::from_utf8_lossy(&r).contains("=SUM(2,3)"));
    // SAFETY: released once each.
    unsafe {
        uo_xlsx_free(again);
        uo_xlsx_free(w);
    }
}

#[test]
fn merge_clear_number_format_and_add_sheet_through_the_exports() {
    let _s = serial();
    let mut w: *mut UoWorkbook = std::ptr::null_mut();
    let o = opts(0, "en-US");
    // SAFETY: valid options, writable output.
    assert_eq!(unsafe { uo_xlsx_new(&raw const o, &raw mut w) }, code::OK);
    assert_eq!(sheets_of(&info(w))[0].0, "Sheet1");
    assert_eq!(set_text(w, 0, 1, 1, "3/15/2025").unwrap()[0], 1);
    assert_eq!(set_text(w, 0, 2, 1, "x").unwrap()[0], 1);
    // SAFETY: a live workbook; outputs writable.
    unsafe {
        // data would be lost: refused unless allowed
        assert_eq!(uo_xlsx_merge(w, 0, 1, 1, 2, 1, MERGE_CELLS, 0), code::INVALID_OPERATION);
        last_error(code::INVALID_OPERATION);
        assert_eq!(uo_xlsx_merge(w, 0, 1, 1, 2, 1, MERGE_CELLS, MERGE_ALLOW_DATA_LOSS), code::OK);
        assert_eq!(uo_xlsx_unmerge(w, 0, 1, 1, 2, 1), code::OK);
        assert_eq!(uo_xlsx_set_number_format(w, 0, 1, 1, 1, 1, b"0.00".as_ptr(), 4), code::OK);
        assert_eq!(uo_xlsx_clear(w, 0, 1, 1, 2, 1, CLEAR_CONTENTS), code::OK);
        let mut out: *mut UoBuf = std::ptr::null_mut();
        let name = take(uo_xlsx_add_sheet(w, std::ptr::null(), 0, &raw mut out), out).unwrap();
        assert_eq!(name, b"Sheet2");
        let name = take(uo_xlsx_add_sheet(w, b"Totals".as_ptr(), 6, &raw mut out), out).unwrap();
        assert_eq!(name, b"Totals");
        // a taken or invalid name is an argument error
        out = std::ptr::null_mut();
        assert_eq!(take(uo_xlsx_add_sheet(w, b"Totals".as_ptr(), 6, &raw mut out), out), Err(code::ARGUMENT));
        assert_eq!(take(uo_xlsx_add_sheet(w, b"a/b".as_ptr(), 3, &raw mut out), out), Err(code::ARGUMENT));
        assert_eq!(uo_xlsx_recalculate(w), code::OK);
        assert_eq!(uo_xlsx_mark_saved(w), code::OK);
    }
    let names: Vec<String> = sheets_of(&info(w)).into_iter().map(|s| s.0).collect();
    assert_eq!(names, ["Sheet1", "Sheet2", "Totals"]);
    // SAFETY: released once.
    unsafe { uo_xlsx_free(w) };
}

#[test]
fn a_xls_opens_as_legacy_and_converts_to_a_xlsx_that_opens_with_the_same_sheets() {
    let _s = serial();
    let dir = workbooks_dir().join("xls");
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    names.sort();
    let mut n = 0;
    for p in names.into_iter().filter(|p| p.extension().is_some_and(|e| e == "xls")) {
        let data = std::fs::read(&p).unwrap();
        let Ok(w) = open(&data, 0, "pt-BR") else { continue };
        // SAFETY: a live workbook.
        assert_eq!(unsafe { uo_xlsx_is_legacy(w) }, 1);
        let before = sheets_of(&info(w));
        let o = opts(0, "pt-BR");
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: readable data, valid options, writable output.
        let xlsx = take(unsafe { uo_xls_to_xlsx(data.as_ptr(), data.len(), &raw const o, &raw mut out) }, out).unwrap();
        let w2 = open(&xlsx, 0, "pt-BR").unwrap();
        // SAFETY: a live workbook.
        assert_eq!(unsafe { uo_xlsx_is_legacy(w2) }, 0);
        assert_eq!(sheets_of(&info(w2)), before, "{}", p.display());
        // saving the legacy workbook gives the same conversion's workbook
        assert_eq!(sheets_of(&info(open(&save(w).unwrap(), 0, "pt-BR").unwrap())), before);
        // SAFETY: released once each.
        unsafe {
            uo_xlsx_free(w);
            uo_xlsx_free(w2);
        }
        n += 1;
    }
    assert!(n >= 8, "only {n} .xls opened");
}

// ───────────────────────────── parity with the C# facade ─────────────────────────────

/// Rows read per call by the dump (the C# test pages the same way).
const DUMP_ROWS: u32 = 500;

/// Everything the facade can show of an opened workbook: the info buffer and every sheet's used range, paged by
/// [`DUMP_ROWS`] rows, with formulas and styles.
fn dump(w: *const UoWorkbook) -> Vec<u8> {
    let inf = info(w);
    let mut all = inf.clone();
    for (i, (_, rows, cols)) in sheets_of(&inf).into_iter().enumerate() {
        if rows == 0 || cols == 0 {
            continue;
        }
        let mut r1 = 1;
        while r1 <= rows {
            let r2 = (r1 + DUMP_ROWS - 1).min(rows);
            let chunk = read(w, u32::try_from(i).unwrap(), 1, r1, cols, r2, READ_FORMULAS | READ_STYLES).unwrap();
            all.extend_from_slice(&chunk);
            r1 = r2 + 1;
        }
    }
    all
}

/// A scripted editing session (the C# test runs the same): the bytes of every result and of the save.
fn edit_script(w: *const UoWorkbook) -> Vec<u8> {
    let mut all = Vec::new();
    fn push(all: &mut Vec<u8>, r: Result<Vec<u8>, i32>) {
        match r {
            Ok(b) => all.extend_from_slice(&b),
            Err(c) => all.extend_from_slice(format!("err{c}").as_bytes()),
        }
    }
    push(&mut all, set_text(w, 0, 1, 1, "=SOMA(2;3)"));
    push(&mut all, set_text(w, 0, 2, 2, "12,5"));
    push(&mut all, set_text(w, 0, 3, 3, "=SOMA("));
    push(&mut all, set_text(w, 0, 4, 4, "15/03/2025"));
    // SAFETY: a live workbook; the buffers are taken and released.
    unsafe {
        uo_xlsx_set_number_format(w, 0, 2, 2, 2, 3, b"0.00".as_ptr(), 4);
        uo_xlsx_merge(w, 0, 6, 6, 7, 6, MERGE_CELLS, MERGE_ALLOW_DATA_LOSS);
        uo_xlsx_clear(w, 0, 2, 2, 2, 2, CLEAR_CONTENTS);
        let mut out: *mut UoBuf = std::ptr::null_mut();
        push(&mut all, take(uo_xlsx_add_sheet(w, b"Parity".as_ptr(), 6, &raw mut out), out));
        let mut did = 0u8;
        uo_xlsx_undo(w, &raw mut did);
        all.push(did);
        uo_xlsx_redo(w, &raw mut did);
        all.push(did);
        uo_xlsx_recalculate(w);
    }
    push(&mut all, Ok(dump(w)));
    push(&mut all, save(w));
    all
}

fn digest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/sheet-native.tsv")
}

/// `key\tread\tsave\tedit`: the digests of the dump of each fixture workbook (pt-BR, fixed clock), of its untouched save and of
/// the scripted edit session, as native handles produce them; the C# facade's tests compare against them. `UO_BLESS=1` rewrites
/// the file.
#[test]
fn the_native_digests_of_the_fixture_workbooks_are_the_committed_ones() {
    let _s = serial();
    let mut text = String::new();
    let mut count = 0;
    for (key, path) in fixtures() {
        let data = std::fs::read(path).unwrap();
        let Ok(w) = open(&data, 0, "pt-BR") else {
            // hostile or unsupported members of the group: the C# test expects the refusal
            text.push_str(&format!("{key}\trefused\n"));
            continue;
        };
        let read_d = digest(&dump(w));
        let save_d = digest(&save(w).unwrap());
        // SAFETY: released once.
        unsafe { uo_xlsx_free(w) };
        let w = open(&data, 0, "pt-BR").unwrap();
        let edit_d = if sheets_of(&info(w)).is_empty() { "none".to_owned() } else { digest(&edit_script(w)) };
        // SAFETY: released once.
        unsafe { uo_xlsx_free(w) };
        text.push_str(&format!("{key}\t{read_d}\t{save_d}\t{edit_d}\n"));
        count += 1;
    }
    assert!(count >= 40, "{count} workbooks");
    if std::env::var_os("UO_BLESS").is_some() {
        std::fs::write(digest_path(), &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(digest_path()).unwrap_or_default().replace("\r\n", "\n");
    let stale: Vec<String> = text.lines().filter(|l| !committed.lines().any(|c| c == *l)).map(str::to_owned).collect();
    assert!(
        stale.is_empty() && text.lines().count() == committed.lines().count(),
        "{} of {} digests differ from {} (the Sheet read, edit or save changed? UO_BLESS=1 cargo test -p uo-ffi --test sheet_abi rewrites it): {:?}",
        stale.len(),
        text.lines().count(),
        digest_path().display(),
        stale.iter().take(3).collect::<Vec<_>>()
    );
}

/// The facade's save of an untouched workbook is the surgical save of the Rust session: the same bytes.
#[test]
fn the_save_of_an_untouched_workbook_equals_the_rust_session_save() {
    let _s = serial();
    let mut n = 0;
    for (key, path) in fixtures().into_iter().filter(|(k, _)| k.ends_with(".xlsx")).take(12) {
        let data = std::fs::read(path).unwrap();
        let w = open(&data, 0, "pt-BR").unwrap();
        let native = save(w).unwrap();
        let session_opts = uo_xlsx::session::SessionOpts {
            locale: "pt-BR".into(),
            clock: uo_calc::Clock { now_serial: NOW },
            year: YEAR,
            ..uo_xlsx::session::SessionOpts::default()
        };
        let mut s = uo_xlsx::session::Session::open(&data, session_opts, uo_calc::RandomSource::seeded(SEED)).unwrap();
        assert_eq!(native, s.save().unwrap(), "{key}");
        // SAFETY: released once.
        unsafe { uo_xlsx_free(w) };
        n += 1;
    }
    assert_eq!(n, 12);
}

// ───────────────────────────── hostile input ─────────────────────────────

/// A deterministic generator for the mutations.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// Every status is one of the documented ones; a success is a handle that reads and saves.
fn survives(data: &[u8], ok: &mut usize, refused: &mut usize) {
    match open(data, 0, "pt-BR") {
        Ok(w) => {
            let inf = info(w);
            for (i, (_, rows, cols)) in sheets_of(&inf).into_iter().enumerate().take(2) {
                let _ = read(w, u32::try_from(i).unwrap(), 1, 1, cols.clamp(1, 30), rows.clamp(1, 30), READ_FORMULAS);
            }
            drop(save(w));
            // SAFETY: released once.
            unsafe { uo_xlsx_free(w) };
            *ok += 1;
        }
        Err(s) => {
            assert!(
                matches!(s, code::INVALID_DATA | code::XML | code::LIMIT_EXCEEDED | code::INVALID_OPERATION),
                "unexpected status {s} on hostile input"
            );
            *refused += 1;
        }
    }
}

#[test]
fn truncated_and_bit_flipped_workbooks_never_crash() {
    let _s = serial();
    let (mut ok, mut refused) = (0, 0);
    let books: Vec<Vec<u8>> =
        ["x1/x1-0000.xlsx", "x1/x1-0007.xlsx", "x1/x1-0013.xlsx"].iter().map(|n| std::fs::read(workbooks_dir().join(n)).unwrap()).collect();
    let xls = std::fs::read(workbooks_dir().join("xls/a-cells.xls")).unwrap();
    let mut rng = Lcg(0x5EED_5EED);
    for data in books.iter().chain(std::iter::once(&xls)) {
        for _ in 0..60 {
            survives(&data[..rng.below(data.len())], &mut ok, &mut refused);
        }
        for _ in 0..80 {
            let mut m = data.clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(m.len());
                m[at] ^= 1 << rng.below(8);
            }
            survives(&m, &mut ok, &mut refused);
        }
    }
    // noise
    for _ in 0..40 {
        let n = 1 + rng.below(4096);
        let v: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        survives(&v, &mut ok, &mut refused);
    }
    println!("hostile: {ok} opened, {refused} refused");
    assert!(refused > 0);
}

/// A zip whose central directory declares a part larger than the untrusted cap is refused before anything is inflated; the
/// same bytes are fine for a trusted source (the declared size lies: the part is tiny, so the trusted open reads on).
#[test]
fn a_lying_central_directory_is_a_limit_for_untrusted_workbooks() {
    let _s = serial();
    let mut zip = small_book();
    // patch every uncompressed size of the central directory to 300 MiB
    let sig = [0x50u8, 0x4B, 0x01, 0x02];
    let mut patched = 0;
    let mut i = 0;
    while i + 46 <= zip.len() {
        if zip[i..i + 4] == sig {
            zip[i + 24..i + 28].copy_from_slice(&((300u32) << 20).to_le_bytes());
            patched += 1;
            i += 46;
        } else {
            i += 1;
        }
    }
    assert!(patched > 3);
    assert_eq!(open(&zip, 0, "").map(|_| ()), Err(code::LIMIT_EXCEEDED));
    // the trusted open does not refuse on the declared size (what it makes of the lie is another matter, not a crash)
    let (mut ok, mut refused) = (0, 0);
    match open(&zip, XLSX_TRUSTED, "") {
        Ok(w) => {
            // SAFETY: released once.
            unsafe { uo_xlsx_free(w) };
            ok += 1;
        }
        Err(s) => {
            assert_ne!(s, code::INTERNAL);
            refused += 1;
        }
    }
    assert_eq!(ok + refused, 1);
}

#[test]
#[cfg_attr(debug_assertions, ignore = "inflates 300 MiB: release builds only")]
fn an_inflation_bomb_is_a_limit_for_untrusted_workbooks() {
    let _s = serial();
    let big = vec![b'a'; 300 << 20];
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let zip = uo_opc::zip::write(
        [("[Content_Types].xml", ct.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("xl/workbook.xml", big.as_slice())],
        uo_opc::zip::WriteOptions::default(),
    )
    .unwrap();
    drop(big);
    assert_eq!(open(&zip, 0, "").map(|_| ()), Err(code::LIMIT_EXCEEDED));
}

#[test]
fn hostile_edits_are_refused_not_crashes() {
    let _s = serial();
    let w = open(&small_book(), 0, "pt-BR").unwrap();
    // formulas the entry must survive: deep nesting, huge ranges, circular, self-reference, odd characters
    let deep = format!("={}1{}", "(".repeat(5000), ")".repeat(5000));
    let long_sum = format!("=SOMA({})", (0..2000).map(|_| "A1").collect::<Vec<_>>().join(";"));
    for t in [
        deep.as_str(),
        long_sum.as_str(),
        "=A1",
        "=B1",
        "=SOMA(A1:XFD1048576)",
        "=INDIRETO(\"A1\")+LIN(A:A)",
        "=\u{0}",
        "\u{FFFD}\u{1F600}'=x",
        "=ÁREAS(1,2",
        "{=1}",
        "=1/0",
        "=SE(VERDADEIRO;1;)",
    ] {
        match set_text(w, 0, 1, 1, t) {
            Ok(b) => assert!(b[0] <= 1),
            Err(s) => assert!(matches!(s, code::ARGUMENT_OUT_OF_RANGE | code::LIMIT_EXCEEDED | code::INVALID_OPERATION), "status {s}"),
        }
    }
    assert!(save(w).is_ok());
    // SAFETY: released once.
    unsafe { uo_xlsx_free(w) };
}

// ───────────────────────────── threads ─────────────────────────────

#[test]
fn threads_share_a_workbook_and_open_their_own() {
    let _s = serial();
    let data = small_book();
    let w = open(&data, 0, "pt-BR").unwrap();
    let want = dump(w);
    let addr = w as usize;
    std::thread::scope(|s| {
        for t in 0..4 {
            let data = &data;
            let want = &want;
            s.spawn(move || {
                let w = addr as *const UoWorkbook;
                for i in 0..20 {
                    assert_eq!(&dump(w), want);
                    if (i + t) % 5 == 0 {
                        let own = open(data, 0, "en-US").unwrap();
                        assert!(save(own).is_ok());
                        // SAFETY: released once.
                        unsafe { uo_xlsx_free(own) };
                    }
                }
            });
        }
    });
    // SAFETY: released once, no thread left.
    unsafe { uo_xlsx_free(w) };
}

// ───────────────────────────── memory ─────────────────────────────

/// M4: ten thousand workbooks, buffers and cancel flags created and released through the exports; the live bytes come back
/// exactly (a workbook handle owns the package, the store, the engine and the history).
#[test]
fn m4_ten_thousand_handles_of_each_kind() {
    let _s = serial();
    let data = small_book();
    let count: usize = std::env::var("UO_SHEET_M4").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000);
    // warm: lazily built process-wide tables
    let w = open(&data, 0, "pt-BR").unwrap();
    drop(dump(w));
    drop(set_text(w, 0, 1, 1, "=SOMA(1;2)"));
    drop(save(w));
    // SAFETY: released once.
    unsafe { uo_xlsx_free(w) };
    let allocated = assert_no_leak_global(|| {
        for i in 0..count {
            let w = open(&data, 0, "pt-BR").unwrap();
            let k = uo_cancel_new();
            if i % 50 == 0 {
                // the workbook does real work now and then: the engine, the history and the caches fill and go with the handle
                drop(dump(w));
                drop(set_text(w, 0, 1, 1, "=SOMA(1;2)").unwrap());
                drop(save(w).unwrap());
            }
            // SAFETY: live handles, each released once.
            unsafe {
                uo_cancel_raise(k);
                uo_cancel_free(k);
                uo_xlsx_free(w);
            }
        }
    });
    println!("M4: {count} x (workbook + cancel) {allocated} B allocated, 0 B left");
}

/// M1: every export leaves nothing once its buffer is freed and the handle is released: ok, refused and cancelled.
#[test]
fn m1_every_export_leaves_nothing_ok_refused_or_cancelled() {
    let _s = serial();
    let data = small_book();
    let xls = std::fs::read(workbooks_dir().join("xls/a-cells.xls")).unwrap();
    // warm
    let w = open(&data, 0, "pt-BR").unwrap();
    drop(edit_script(w));
    // SAFETY: released once.
    unsafe { uo_xlsx_free(w) };
    let allocated = assert_no_leak_global(|| {
        let w = open(&data, 0, "pt-BR").unwrap();
        drop(edit_script(w));
        let k = uo_cancel_new();
        // SAFETY: live handles, each released once; valid arguments.
        unsafe {
            uo_cancel_raise(k);
            let mut out: *mut UoBuf = std::ptr::null_mut();
            let _ = take(uo_xlsx_save(w, k, &raw mut out), out);
            let _ = take(uo_xlsx_read_range(w, 0, 1, 1, 9, 9, 0, k, &raw mut out), out);
            uo_cancel_free(k);
            uo_xlsx_free(w);
        }
        // refused opens and a legacy open
        let _ = open(&[1, 2, 3], 0, "");
        let _ = open(&data[..data.len() / 2], 0, "");
        let lw = open(&xls, 0, "en-US").unwrap();
        drop(dump(lw));
        // SAFETY: released once.
        unsafe { uo_xlsx_free(lw) };
        let o = opts(0, "");
        let mut out: *mut UoBuf = std::ptr::null_mut();
        // SAFETY: readable data, valid options, writable output.
        drop(take(unsafe { uo_xls_to_xlsx(xls.as_ptr(), xls.len(), &raw const o, &raw mut out) }, out));
    });
    println!("M1: sheet exports {allocated} B allocated, 0 B left");
}
