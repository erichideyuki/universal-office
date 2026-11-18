//! Test-only exports (feature `jsnum-memtest`) for the wasm memory gate of `uo-jsnum` (stage 4):
//! `tests/jsnum_memory.test.mjs` runs a soak of every numeric function and of the string functions
//! over buffers it moves in, samples `uo_memory_live_bytes` and `memory.buffer.byteLength`, and
//! measures the stack depth of the deepest functions. These are not the binding API of uo-jsnum
//! (the parity self-test export is the feature `jsnum-selftest`); never in the shipped module.

use std::hint::black_box;

use uo_jsnum::{conv, math, parse, text};

const XS: [f64; 12] = [0.0, -0.0, 0.5, -1.5, 1e-9, 3.0, 12345.678, -2e22, 1e308, f64::NAN, f64::INFINITY, 4.9e-324];

fn fold(acc: f64, v: f64) -> f64 {
    acc + if v.is_nan() { 1.0 } else { math::min(v.abs(), 1e300) }
}

/// Every numeric function of the crate over a fixed grid; `seed` moves the inputs.
#[unsafe(no_mangle)]
pub extern "C" fn uo_jsnum_memtest_numeric(seed: u32) -> f64 {
    let mut acc = 0.0;
    for (i, &x) in XS.iter().enumerate() {
        let x = black_box(x + (seed as usize + i) as f64 * 1e-3);
        let y = black_box(XS[(i + seed as usize) % XS.len()]);
        for v in [
            math::sin(x),
            math::cos(x),
            math::tan(x),
            math::asin(x),
            math::acos(x),
            math::atan(x),
            math::atan2(x, y),
            math::exp(x),
            math::expm1(x),
            math::log(x),
            math::log1p(x),
            math::log2(x),
            math::log10(x),
            math::sinh(x),
            math::cosh(x),
            math::tanh(x),
            math::atanh(x),
            math::cbrt(x),
            math::pow(x, y),
            math::hypot(x, y),
            math::hypot3(x, y, x),
            math::hypot_n(&[x, y, x, y]),
            math::round(x),
            math::sign(x),
            math::max(x, y),
            math::min(x, y),
            math::max_n(&[x, y, 1.0]),
            math::min_n(&[x, y, 1.0]),
            math::imul(x, y),
            math::clz32(x),
            math::fround(x),
            math::floor(x),
            math::ceil(x),
            math::trunc(x),
            math::abs(x),
            math::sqrt(x),
            math::rem(x, y),
            f64::from(conv::to_int32(x)),
            f64::from(conv::to_uint32(x)),
            f64::from(conv::to_int16(x)),
            f64::from(conv::to_uint16(x)),
            f64::from(conv::to_int8(x)),
            f64::from(conv::to_uint8(x)),
            f64::from(conv::to_uint8_clamp(x)),
            conv::js_index(x, 8).map_or(-1.0, |v| v as f64),
            conv::canonicalize_nan(x),
        ] {
            acc = fold(acc, black_box(v));
        }
        acc += f64::from(u8::from(conv::truthy(x)) + u8::from(conv::is_integer(x)) + u8::from(conv::is_safe_integer(x)));
    }
    acc
}

fn utf8<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the test harness passes a buffer from `uo_alloc_bytes(len)` that JS filled with
    // `len` bytes and keeps alive (and unchanged) for the duration of the call.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
}

/// Every string function over the UTF-8 buffer; `-1` when it is not UTF-8. The cut-surrogate
/// `utf16_slice` outputs are owned and dropped here.
///
/// # Safety
/// `ptr`/`len` must be a buffer from `uo_alloc_bytes(len)` filled with `len` bytes, alive during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_jsnum_memtest_strings(ptr: *const u8, len: usize) -> f64 {
    let Some(s) = utf8(ptr, len) else { return -1.0 };
    let s = black_box(s);
    let mut acc = 0.0;
    for v in [
        parse::to_number(s),
        parse::parse_int(s, 0),
        parse::parse_int(s, 2),
        parse::parse_int(s, 7),
        parse::parse_int(s, 10),
        parse::parse_int(s, 16),
        parse::parse_int(s, 36),
        parse::parse_int(s, 99),
    ] {
        acc = fold(acc, black_box(v));
    }
    acc += text::trim(s).len() as f64;
    acc += text::trim_start(s).len() as f64;
    acc += text::trim_end(s).len() as f64;
    acc += text::split_js_space(s).count() as f64;
    acc += text::utf16_len(s) as f64;
    acc += text::utf16_slice(s, 1, -1).len() as f64;
    acc += text::utf16_slice(s, 2, 5).into_owned().len() as f64;
    acc += text::js_space_len(s.as_bytes(), 0) as f64;
    acc
}

/// Bytes allocated by the module since it started (feature `count`), to assert that a call allocated 0.
#[cfg(feature = "count")]
#[unsafe(no_mangle)]
pub extern "C" fn uo_jsnum_memtest_allocated_bytes() -> f64 {
    uo_alloc::global_stats().0 as f64
}

/// The address of a local of this frame: where the shadow stack is at an export call (a few bytes
/// below the stack pointer at entry). JS paints the stack below it and calls [`uo_jsnum_memtest_call`].
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn uo_jsnum_memtest_stack_probe() -> usize {
    let local = 0u8;
    black_box(&local) as *const u8 as usize
}

/// One function of the crate by number, for the stack-depth measurement. `x`/`y` for the numeric
/// ones, `ptr`/`len` for the string ones. Unknown `kind` returns -1.
///
/// # Safety
/// For kinds 20.., `ptr`/`len` must be a buffer from `uo_alloc_bytes(len)` filled with `len` bytes.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn uo_jsnum_memtest_call(kind: u32, x: f64, y: f64, ptr: *const u8, len: usize) -> f64 {
    let (x, y) = (black_box(x), black_box(y));
    let input = || utf8(ptr, len).unwrap_or("");
    black_box(match kind {
        0 => math::sin(x),
        1 => math::cos(x),
        2 => math::tan(x),
        3 => math::asin(x),
        4 => math::acos(x),
        5 => math::atan(x),
        6 => math::atan2(x, y),
        7 => math::exp(x),
        8 => math::expm1(x),
        9 => math::log(x),
        10 => math::log1p(x),
        11 => math::log2(x),
        12 => math::log10(x),
        13 => math::sinh(x),
        14 => math::cosh(x),
        15 => math::tanh(x),
        16 => math::atanh(x),
        17 => math::cbrt(x),
        18 => math::pow(x, y),
        19 => math::hypot3(x, y, x),
        20 => parse::to_number(input()),
        21 => parse::parse_int(input(), 10),
        22 => parse::parse_int(input(), 7),
        23 => parse::parse_int(input(), 16),
        24 => parse::parse_int(input(), 36),
        25 => text::split_js_space(input()).count() as f64,
        26 => text::utf16_slice(input(), 1, -1).len() as f64,
        27 => text::trim(input()).len() as f64,
        28 => f64::from(conv::to_int32(x)),
        // the two-argument Math.hypot (the serve page of slides_switch.mjs `?v8math=1` replaces the page's Math with these)
        29 => math::hypot(x, y),
        _ => -1.0,
    })
}
