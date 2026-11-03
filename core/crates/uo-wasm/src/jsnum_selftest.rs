//! Parity self-test of `uo-jsnum` inside the wasm module (feature `jsnum-selftest`, stage 4):
//! the module recomputes the digests of the committed captures (`tests/captures/*.capture` of
//! the crate) with the same generators the native tests use (the test sources are included here
//! with `#[path]`, so there is one copy), and `tests/jsnum_selftest.test.mjs` compares them with
//! the capture files, in Node and in the browser engines. Native, wasm and the capture must give
//! the same digests. The functions take and return plain numbers and bytes in JS-provided
//! buffers (`uo_alloc_bytes`); no handle crosses the boundary. Not the binding API of
//! `uo-jsnum`; never in the shipped module.
//!
//! Groups of [`uo_jsnum_selftest_count`]: 0 numeric records of `jsnum.capture` (`num/*`), 1 the
//! generated strings of `adv.capture` (`adv/*`), 2 the targeted math distributions (`ext/*`),
//! 3 the `pow` distributions of `pow.capture` (`dist/*`), 4 the integer conversion sweep
//! (`conv/exp_sweep`), and of `format.capture` (increment B): 5 the formatter records (`fmt/*`), 6
//! the `parseFloat` string records (`rand/pf_*`), 7 the sweeps of powers of two and ten
//! (`sweep/*`). [`uo_jsnum_selftest_format`] and [`uo_jsnum_selftest_parse_float`] run single calls
//! that JS feeds in (the written-out edge values and the `parseFloat` corpus).
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::expect_used)]

use uo_jsnum::conv::canonicalize_nan;
use uo_jsnum::{parse, text};
use uo_testkit::Stream;

#[path = "../../uo-jsnum/tests/pow.rs"]
mod pow_replay;
#[path = "../../uo-jsnum/tests/support/mod.rs"]
mod support;

const GROUPS: u32 = 8;

const SWEEPS: [(&str, &str); 6] = [
    ("fixed", "pow2"),
    ("fixed", "pow10"),
    ("precision", "pow2"),
    ("precision", "pow10"),
    ("exponential", "pow2"),
    ("exponential", "pow10"),
];

fn numeric_records() -> Vec<support::Rec> {
    support::numeric_records()
}

fn numeric_digest(rec: &support::Rec, n: usize) -> String {
    let specials = support::inputs().specials;
    let mut rng = support::make_rng(&rec.key);
    let mut s = Stream::new();
    let f = support::op(rec.op);
    let mut args = vec![0.0; rec.gens.len()];
    for _ in 0..n {
        for (a, g) in args.iter_mut().zip(&rec.gens) {
            *a = g.draw(&mut rng, &specials);
        }
        s.f64(canonicalize_nan(f(&args)));
    }
    s.finish().short()
}

/// Number of records of `group` (0 for an unknown group).
#[unsafe(no_mangle)]
pub extern "C" fn uo_jsnum_selftest_count(group: u32) -> u32 {
    (match group {
        0 => numeric_records().len(),
        1 => support::adv::generated_records().len(),
        2 => support::adv::ext_records().len(),
        3 => pow_replay::selftest_distributions().len(),
        4 => 1,
        5 => support::format::records().len(),
        6 => support::format::STRING_KEYS.len(),
        7 => SWEEPS.len(),
        _ => 0,
    }) as u32
}

fn key_of(group: u32, index: usize) -> Option<String> {
    match group {
        0 => numeric_records().get(index).map(|r| format!("num/{}", r.key)),
        1 => support::adv::generated_records().get(index).map(|r| r.key.clone()),
        2 => support::adv::ext_records().get(index).map(|r| r.key.clone()),
        3 => pow_replay::selftest_distributions().get(index).map(|n| format!("dist/{n}")),
        4 => (index == 0).then(|| "conv/exp_sweep".to_owned()),
        5 => support::format::records().get(index).map(support::format::capture_key),
        6 => support::format::STRING_KEYS.get(index).map(|k| (*k).to_owned()),
        7 => SWEEPS.get(index).map(|(op, set)| format!("sweep/{op}/{set}")),
        _ => None,
    }
}

fn write_out(text: &str, out: *mut u8, cap: u32) -> u32 {
    let bytes = text.as_bytes();
    if bytes.len() > cap as usize || out.is_null() {
        return 0;
    }
    // SAFETY: `out` is a JS-provided buffer of `cap` bytes (`uo_alloc_bytes(cap)`) and `bytes.len() <= cap`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
    bytes.len() as u32
}

/// Writes the capture key of record `index` of `group` (UTF-8) into `out` (`cap` bytes) and
/// returns its length; 0 when there is no such record or the key does not fit.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_key(group: u32, index: u32, out: *mut u8, cap: u32) -> u32 {
    key_of(group, index as usize).map_or(0, |k| write_out(&k, out, cap))
}

/// Writes the digest of the first `n` samples of record `index` of `group` (the facet text of the
/// capture: 32 hex digits, `count:digest` for `pow`) into `out` and returns its length.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_digest(group: u32, index: u32, n: u32, out: *mut u8, cap: u32) -> u32 {
    let (index, n) = (index as usize, n as usize);
    let digest = match group {
        0 => numeric_records().get(index).map(|r| numeric_digest(r, n)),
        1 => support::adv::generated_records().get(index).map(|r| support::adv::generated_digest(r, n)),
        2 => {
            let thr = support::inputs().thresholds;
            support::adv::ext_records().get(index).map(|r| support::adv::ext_digest(r, n, &thr))
        }
        3 => (index < pow_replay::selftest_distributions().len()).then(|| pow_replay::selftest_digest(index, n)),
        4 => (index == 0).then(|| support::adv::conv_sweep().1),
        5 => support::format::records().get(index).map(|r| support::format::digest(r, n).1),
        6 => support::format::STRING_KEYS.get(index).map(|k| support::format::string_digest(k, n).1),
        7 => SWEEPS.get(index).map(|(op, set)| support::format::sweep(op, set).1),
        _ => None,
    };
    digest.map_or(0, |d| write_out(&d, out, cap))
}

fn input<'a>(ptr: *const u8, len: u32) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: the caller guarantees `ptr` points to `len` initialized bytes that stay valid for the call.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(bytes).unwrap_or("")
}

fn hex(v: f64) -> String {
    format!("{:016x}", canonicalize_nan(v).to_bits())
}

/// `Number(s)` of the UTF-8 string at `ptr`/`len`, as 16 hex digits of the bits (NaN canonicalized).
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to 16 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_number(ptr: *const u8, len: u32, out: *mut u8) -> u32 {
    write_out(&hex(parse::to_number(input(ptr, len))), out, 16)
}

/// `parseInt(s, radix)` (the radix already ToInt32'd) of the UTF-8 string, as 16 hex digits.
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to 16 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_parse_int(ptr: *const u8, len: u32, radix: i32, out: *mut u8) -> u32 {
    write_out(&hex(parse::parse_int(input(ptr, len), radix)), out, 16)
}

/// `parseFloat(s)` of the UTF-8 string at `ptr`/`len`, as 16 hex digits of the bits (NaN canonicalized).
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to 16 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_parse_float(ptr: *const u8, len: u32, out: *mut u8) -> u32 {
    write_out(&hex(parse::parse_float(input(ptr, len))), out, 16)
}

/// One formatter call: `op` 0 `String(x)`, 1 `toFixed(n)`, 2 `toPrecision(n)`, 3 `toExponential(n)`, 4
/// `toString(n)` (radix), 5 `format!("{}", Js(x))`; a JS `RangeError` is the text `RangeError`. The
/// text goes to `out` (`cap` bytes); returns its length (0 for an unknown `op` or a text that does not fit).
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_format(op: u32, x: f64, n: u32, out: *mut u8, cap: u32) -> u32 {
    let text = match op {
        0 => support::format::run("js", x, n),
        1 => support::format::run("fixed", x, n),
        2 => support::format::run("precision", x, n),
        3 => support::format::run("exponential", x, n),
        4 => support::format::run("radix", x, n),
        5 => format!("{}", uo_jsnum::format::Js(x)),
        _ => return 0,
    };
    write_out(&text, out, cap)
}

/// The text functions on the UTF-8 string: `op` 0 `trim`, 1 `trimStart`, 2 `trimEnd` (the text),
/// 3 `s.length` (decimal digits).
///
/// # Safety
/// `ptr` must be null or point to `len` initialized bytes; `out` to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_selftest_text(ptr: *const u8, len: u32, op: u32, out: *mut u8, cap: u32) -> u32 {
    let s = input(ptr, len);
    let r = match op {
        0 => text::trim(s).to_owned(),
        1 => text::trim_start(s).to_owned(),
        2 => text::trim_end(s).to_owned(),
        _ => text::utf16_len(s).to_string(),
    };
    write_out(&r, out, cap)
}

/// The number of groups, so the test can walk them all.
#[unsafe(no_mangle)]
pub extern "C" fn uo_jsnum_selftest_groups() -> u32 {
    GROUPS
}
