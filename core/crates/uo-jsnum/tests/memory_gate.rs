//! Stage-4 memory gate of uo-jsnum (increment A), on top of the porter's `tests/memory.rs`.
//!
//! - M1 per function: `m1_every_function_allocates_nothing` and
//!   `m1_text_and_parsers_allocate_nothing_on_every_path` measure each public function on its own,
//!   with black-boxed inputs and outputs (so the optimiser cannot drop the call or its allocation),
//!   over a grid of inputs including every error path (junk, empty, out-of-range radix);
//!   `m1_parsers_on_huge_and_hostile_strings_allocate_nothing` runs the parsers and text functions on
//!   10 MB inputs; `m1_covers_every_public_function` fails when a `pub fn` of the crate is not
//!   called (module-qualified) in `tests/memory.rs`, `tests/memory_format.rs` (the formatters: they allocate
//!   exactly their output) or here.
//! - M3: `m3_utf16_slice_owned_output_is_the_output_plus_at_most_three_bytes` records the one path
//!   that allocates.
//! - M5: `m5_no_caches_no_interior_mutability` scans the sources.
//! - M2 on the error paths: `m2_soak_hostile_*`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::borrow::Cow;
use std::hint::black_box;

use uo_alloc::{assert_flat, measure, soak};
use uo_jsnum::{conv, math, object, parse, text};

/// Runs `$call` under `measure` with its output black-boxed; asserts 0 bytes allocated and no peak.
macro_rules! zero {
    ($label:expr, $call:expr) => {{
        let (out, usage) = measure(|| black_box($call));
        black_box(out);
        assert_eq!(usage.allocated, 0, "{} allocated {} bytes", $label, usage.allocated);
        assert_eq!(usage.peak_above_start, 0, "{} peaked at {} bytes", $label, usage.peak_above_start);
    }};
}

const GRID: [f64; 22] = [
    0.0,
    -0.0,
    0.5,
    -0.5,
    1.0,
    -1.5,
    1e-9,
    3.0,
    12345.678,
    -2e22,
    1e308,
    -1e308,
    f64::MAX,
    f64::MIN_POSITIVE,
    4.9e-324,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    2147483648.0,
    4294967296.5,
    9007199254740993.0,
    1.0e21,
];

#[test]
fn m1_every_function_allocates_nothing() {
    uo_jsnum::clear_caches();
    let rows = [[1.0, 2.0, f64::NAN, 4.0], [f64::INFINITY, -0.0, 1e300, 1e300], [0.0; 4]];
    let many: Vec<f64> = (0..10_000).map(|i| f64::from(i) * 0.37 - 900.0).collect();
    for &x in &GRID {
        for &y in &GRID {
            let (x, y) = (black_box(x), black_box(y));
            zero!("math::atan2", math::atan2(x, y));
            zero!("math::pow", math::pow(x, y));
            zero!("math::hypot", math::hypot(x, y));
            zero!("math::hypot3", math::hypot3(x, y, x));
            zero!("math::max", math::max(x, y));
            zero!("math::min", math::min(x, y));
            zero!("math::imul", math::imul(x, y));
            zero!("math::rem", math::rem(x, y));
        }
        let x = black_box(x);
        zero!("math::sin", math::sin(x));
        zero!("math::cos", math::cos(x));
        zero!("math::tan", math::tan(x));
        zero!("math::asin", math::asin(x));
        zero!("math::acos", math::acos(x));
        zero!("math::atan", math::atan(x));
        zero!("math::exp", math::exp(x));
        zero!("math::expm1", math::expm1(x));
        zero!("math::log", math::log(x));
        zero!("math::log1p", math::log1p(x));
        zero!("math::log2", math::log2(x));
        zero!("math::log10", math::log10(x));
        zero!("math::sinh", math::sinh(x));
        zero!("math::cosh", math::cosh(x));
        zero!("math::tanh", math::tanh(x));
        zero!("math::atanh", math::atanh(x));
        zero!("math::cbrt", math::cbrt(x));
        zero!("math::round", math::round(x));
        zero!("math::sign", math::sign(x));
        zero!("math::clz32", math::clz32(x));
        zero!("math::fround", math::fround(x));
        zero!("math::floor", math::floor(x));
        zero!("math::ceil", math::ceil(x));
        zero!("math::trunc", math::trunc(x));
        zero!("math::abs", math::abs(x));
        zero!("math::sqrt", math::sqrt(x));
        zero!("conv::to_int32", conv::to_int32(x));
        zero!("conv::to_uint32", conv::to_uint32(x));
        zero!("conv::to_int16", conv::to_int16(x));
        zero!("conv::to_uint16", conv::to_uint16(x));
        zero!("conv::to_int8", conv::to_int8(x));
        zero!("conv::to_uint8", conv::to_uint8(x));
        zero!("conv::to_uint8_clamp", conv::to_uint8_clamp(x));
        zero!("conv::js_index", conv::js_index(x, black_box(8)));
        zero!("conv::js_index (len 0)", conv::js_index(x, black_box(0)));
        zero!("conv::js_index (usize::MAX)", conv::js_index(x, black_box(usize::MAX)));
        zero!("conv::canonicalize_nan", conv::canonicalize_nan(x));
        zero!("conv::truthy", conv::truthy(x));
        zero!("conv::is_integer", conv::is_integer(x));
        zero!("conv::is_safe_integer", conv::is_safe_integer(x));
    }
    zero!("math::hypot_n (empty)", math::hypot_n(black_box(&[])));
    zero!("math::max_n (empty)", math::max_n(black_box(&[])));
    zero!("math::min_n (empty)", math::min_n(black_box(&[])));
    for row in &rows {
        zero!("math::hypot_n", math::hypot_n(black_box(row)));
        zero!("math::max_n", math::max_n(black_box(row)));
        zero!("math::min_n", math::min_n(black_box(row)));
    }
    zero!("math::hypot_n (10,000)", math::hypot_n(black_box(&many)));
    zero!("math::max_n (10,000)", math::max_n(black_box(&many)));
    zero!("math::min_n (10,000)", math::min_n(black_box(&many)));
}

const CORPUS: [&str; 34] = [
    "",
    " ",
    "\u{a0}\u{feff}\u{2028}\u{3000}",
    "12.5",
    "-12.5e-3",
    "+.5",
    "5.",
    ".",
    "1e",
    "1e+",
    "1e400",
    "-1e400",
    "1e-400",
    "Infinity",
    "-Infinity",
    "infinity",
    "NaN",
    "abc",
    "0x",
    "0x1F",
    "0X1f ",
    "-0x1F",
    "0o17",
    "0b101",
    "0b102",
    "00012",
    "9007199254740993",
    "123456789012345678901234567890",
    "1_000",
    "\u{1f600}",
    "12\u{1f600}",
    "a\u{1f600}b\u{1f600}c",
    "\u{e9}\u{e8}\u{ea}\u{ebcd}",
    "  7 8\t\n9  ",
];

const RADIXES: [i32; 17] = [0, 1, 2, 3, 7, 8, 10, 16, 32, 36, 37, 100, -1, -36, 2 << 20, i32::MIN, i32::MAX];

#[test]
fn m1_text_and_parsers_allocate_nothing_on_every_path() {
    uo_jsnum::clear_caches();
    for s in CORPUS {
        let s = black_box(s);
        zero!(format!("parse::to_number({s:?})"), parse::to_number(s));
        zero!(format!("parse::parse_float({s:?})"), parse::parse_float(s));
        for radix in RADIXES {
            zero!(format!("parse::parse_int({s:?}, {radix})"), parse::parse_int(s, black_box(radix)));
        }
        zero!("text::trim", text::trim(s));
        zero!("text::trim_start", text::trim_start(s));
        zero!("text::trim_end", text::trim_end(s));
        zero!("text::utf16_len", text::utf16_len(s));
        zero!("text::split_js_space", text::split_js_space(s).map(str::len).sum::<usize>());
        zero!("text::split_js_space (last)", text::split_js_space(s).last());
        let bytes = s.as_bytes();
        for i in 0..=bytes.len() {
            zero!("text::js_space_len", text::js_space_len(black_box(bytes), black_box(i)));
        }
        let len = text::utf16_len(s) as i64;
        let mut owned = 0;
        for start in [i64::MIN, -len - 1, -len, -1, 0, 1, 2, len - 1, len, len + 1, i64::MAX] {
            for end in [i64::MIN, -len, -1, 0, 1, 2, 3, len - 1, len, len + 1, i64::MAX] {
                let (out, usage) = measure(|| black_box(text::utf16_slice(s, black_box(start), black_box(end))));
                if matches!(out, Cow::Borrowed(_)) {
                    assert_eq!(usage.allocated, 0, "utf16_slice({s:?}, {start}, {end}) is borrowed but allocated");
                } else {
                    owned += 1;
                    assert!(usage.allocated > 0);
                }
                drop(out);
            }
        }
        if s == "a\u{1f600}b\u{1f600}c" {
            assert!(owned > 0, "the grid must reach the owned path");
        }
    }
    for c in ['a', ' ', '\u{a0}', '\u{1680}', '\u{180e}', '\u{feff}', '\u{1f600}', '\u{10ffff}'] {
        zero!("text::is_js_space", text::is_js_space(black_box(c)));
    }
}

/// The helpers moved here from uo-opc and uo-fonts (stage 5, D2): the predicates, `lower_cp`,
/// `parse_int_utf16`, `array_index`, `decode_utf8` of valid bytes and `js_object_order` without an
/// array index allocate nothing; `to_lowercase`, `decode_latin1`, `decode_utf8` of invalid bytes
/// and `js_object_order` with an index allocate their output only (nothing kept once it is dropped).
#[test]
fn m1_stage5_moved_helpers() {
    uo_jsnum::clear_caches();
    for u in [0u16, 0x20, 0x41, 0xA0, 0x180E, 0x2028, 0xD800, 0xDFFF, 0xFEFF, 0xFFFF] {
        zero!("text::is_js_space_unit", text::is_js_space_unit(black_box(u)));
    }
    for b in [b'a', b'_', b'0', b' ', 0x80, 0xFF] {
        zero!("text::is_js_word", text::is_js_word(black_box(b)));
    }
    for c in ['A', '\u{130}', '\u{3a3}', '\u{1e9e}', '\u{10400}', '\u{10ffff}'] {
        zero!("text::lower_cp", text::lower_cp(black_box(c)));
    }
    for s in CORPUS {
        let units: Vec<u16> = s.encode_utf16().chain([0xD83D, 0x31]).collect();
        for radix in RADIXES {
            zero!(format!("parse::parse_int_utf16({s:?}, {radix})"), parse::parse_int_utf16(black_box(&units), black_box(radix)));
        }
        zero!("object::array_index", object::array_index(black_box(s)));
        zero!("text::decode_utf8 (valid)", text::decode_utf8(black_box(s.as_bytes())));
        let (out, usage) = measure(|| black_box(text::to_lowercase(black_box(s))));
        assert!(usage.allocated <= (out.capacity() as u64).max(1) * 2, "to_lowercase({s:?}) allocated {} B", usage.allocated);
        drop(out);
        let (out, usage) = measure(|| black_box(text::decode_latin1(black_box(s.as_bytes()))));
        assert!(usage.allocated <= out.capacity() as u64 * 2 + 8, "decode_latin1({s:?}) allocated {} B", usage.allocated);
        drop(out);
    }
    for key in ["4294967294", "4294967295", "01", "0", "-1", "1.5"] {
        zero!("object::array_index", object::array_index(black_box(key)));
    }
    let (_, usage) = measure(|| black_box(text::decode_utf8(black_box(&[0x61, 0xFF, 0x62]))).len());
    assert_eq!(usage.live_delta, 0);
    let names: Vec<(String, ())> = ["b", "a", "c"].iter().map(|n| (n.to_string(), ())).collect();
    let (out, usage) = measure(|| black_box(object::js_object_order(names)));
    assert_eq!(usage.allocated, 0, "js_object_order without an index allocated");
    drop(out);
    let names: Vec<(String, ())> = ["b", "2", "a", "1"].iter().map(|n| (n.to_string(), ())).collect();
    let (_, usage) = measure(|| black_box(object::js_object_order(names)).len());
    assert!(usage.live_delta <= 0, "js_object_order kept {} B", usage.live_delta);
}

/// `unit` repeated up to `len` bytes (built outside `measure`).
fn repeated(unit: &str, len: usize) -> String {
    unit.repeat(len / unit.len())
}

#[test]
fn m1_parsers_on_huge_and_hostile_strings_allocate_nothing() {
    const N: usize = 10_000_000;
    let cases: Vec<(&str, String)> = vec![
        ("10 MB of '1'", repeated("1", N)),
        ("10 MB of '0'", repeated("0", N)),
        ("10 MB of spaces then 1", repeated(" ", N) + "1"),
        ("10 MB of spaces", repeated(" ", N)),
        ("10 MB of U+3000 then junk", repeated("\u{3000}", N) + "x"),
        ("0x + 10 MB of f", format!("0x{}", repeated("f", N))),
        ("0b + 10 MB of 1", format!("0b{}", repeated("1", N))),
        ("0o + 10 MB of 7", format!("0o{}", repeated("7", N))),
        ("1e + 5 MB of 9", format!("1e{}", repeated("9", N / 2))),
        ("1. + 5 MB of 0 + e-99", format!("1.{}e-99", repeated("0", N / 2))),
        ("5 MB of 9 . 5 MB of 9", format!("{}.{}", repeated("9", N / 2), repeated("9", N / 2))),
        ("10 MB of z (radix 36)", repeated("z", N)),
        ("10 MB of 6 (radix 7)", repeated("6", N)),
        ("10 MB of '-'", repeated("-", N)),
        ("10 MB of U+00E9", repeated("\u{e9}", N)),
        ("10 MB of U+1F600", repeated("\u{1f600}", N)),
        ("10 MB of 'a '", repeated("a ", N)),
        ("10 MB of 7 then junk", repeated("7", N) + "!"),
    ];
    for (name, s) in &cases {
        let s = black_box(s.as_str());
        zero!(format!("to_number, {name}"), parse::to_number(s));
        zero!(format!("parse_float, {name}"), parse::parse_float(s));
        for radix in [0, 2, 3, 7, 8, 10, 16, 32, 36, 37] {
            zero!(format!("parse_int radix {radix}, {name}"), parse::parse_int(s, black_box(radix)));
        }
        zero!(format!("trim, {name}"), text::trim(s));
        zero!(format!("split_js_space, {name}"), text::split_js_space(s).map(str::len).sum::<usize>());
        zero!(format!("utf16_len, {name}"), text::utf16_len(s));
        zero!(format!("utf16_slice borrowed, {name}"), text::utf16_slice(s, 0, black_box(i64::MAX)));
    }
}

/// The one allocating path of the crate: `utf16_slice` cutting a surrogate pair builds one `String`
/// with capacity `middle + 6`, so output ≤ allocated ≤ output + 3, one allocation, no growth.
#[test]
fn m3_utf16_slice_owned_output_is_the_output_plus_at_most_three_bytes() {
    let mut rows = Vec::new();
    for (name, pairs) in [("tiny", 2usize), ("4 KB", 1_000), ("4 MB", 1_000_000)] {
        let s = format!("a{}b", "\u{1f600}".repeat(pairs));
        let units = text::utf16_len(&s) as i64;
        for (label, start, end) in [("both cut", 2, units - 2), ("start cut", 2, units), ("end cut", 0, units - 2)] {
            let (out, usage) = measure(|| black_box(text::utf16_slice(black_box(&s), start, end)));
            let Cow::Owned(out) = out else { panic!("{name}/{label}: expected the owned path") };
            let allocated = usage.allocated as usize;
            assert!(allocated >= out.len() && allocated <= out.len() + 3, "{name}/{label}: {allocated} B for {} B", out.len());
            assert_eq!(usage.peak_above_start, usage.allocated as i64, "{name}/{label}: one allocation, no growth");
            assert_eq!(usage.live_delta, usage.allocated as i64);
            rows.push(format!("{name}/{label}: input {} B, output {} B, allocated {allocated} B", s.len(), out.len()));
            drop(out);
        }
    }
    println!("# utf16_slice owned path: {}", rows.join("; "));
    let s = "\u{1f600}".repeat(1_000_000);
    zero!("utf16_slice, cuts on pair boundaries", text::utf16_slice(black_box(&s), 2, -2));
}

fn public_functions() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for (module, file) in [
        ("math", "math/mod.rs"),
        ("math", "math/hypot.rs"),
        ("math", "math/pow.rs"),
        ("conv", "conv.rs"),
        ("text", "text.rs"),
        ("parse", "parse.rs"),
        ("format", "format/mod.rs"),
        ("format", "format/radix.rs"),
    ] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("pub fn ") {
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                found.push((module.to_string(), name));
            }
        }
    }
    let math = std::fs::read_to_string(root.join("math/mod.rs")).unwrap();
    let reexports = math.lines().find(|l| l.starts_with("pub use crate::fdlibm::")).unwrap();
    let list = reexports.split('{').nth(1).unwrap().split('}').next().unwrap();
    for name in list.split(',') {
        found.push(("math".to_string(), text::trim(name).to_string()));
    }
    found
}

#[test]
fn m1_covers_every_public_function() {
    let functions = public_functions();
    assert!(functions.len() >= 65, "the scan found only {} functions", functions.len());
    let sources = [include_str!("memory.rs"), include_str!("memory_gate.rs"), include_str!("memory_format.rs")];
    let missing: Vec<String> = functions
        .iter()
        .filter(|(module, name)| {
            let call = format!("{module}::{name}(");
            !sources.iter().any(|s| s.contains(&call))
        })
        .map(|(module, name)| format!("{module}::{name}"))
        .collect();
    assert!(missing.is_empty(), "public functions not measured by the memory tests: {missing:?}");
}

#[test]
fn m5_no_caches_no_interior_mutability() {
    let mut stack = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    let mut statics = Vec::new();
    let mut files = 0;
    let banned = [
        "thread_local!",
        "OnceLock",
        "OnceCell",
        "LazyLock",
        "LazyCell",
        "lazy_static",
        "Mutex",
        "RwLock",
        "RefCell",
        "Cell<",
        "Atomic",
        "static mut",
        "Box::leak",
        "mem::forget",
        "HashMap",
        "BTreeMap",
        "Rc<",
        "Arc<",
    ];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&p).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for word in banned {
                    assert!(!code.contains(word), "{}:{}: `{word}`", p.display(), n + 1);
                }
                if text::trim_start(code).starts_with("pub(super) static ") || text::trim_start(code).starts_with("static ") {
                    statics.push(format!("{}:{}", p.file_name().unwrap().to_string_lossy(), n + 1));
                }
            }
        }
    }
    println!("# {files} source files scanned; immutable lookup tables: {statics:?}");
    assert_eq!(statics.len(), 5, "the four tables of pow and the powers of ten of dtoa, all arrays of plain numbers: {statics:?}");
}

fn hostile_batch(inputs: &[String]) -> f64 {
    let mut acc = 0.0;
    for s in inputs {
        let s = black_box(s.as_str());
        for v in [parse::to_number(s), parse::parse_int(s, 0), parse::parse_int(s, 7), parse::parse_int(s, 36), parse::parse_int(s, 99)] {
            acc += if v.is_nan() { 1.0 } else { math::min(v.abs(), 1e300) };
        }
        acc += text::trim(s).len() as f64 + text::split_js_space(s).count() as f64 + text::utf16_len(s) as f64;
        acc += text::utf16_slice(s, 1, -1).len() as f64 + text::utf16_slice(s, 2, 5).len() as f64;
    }
    acc
}

fn hostile_inputs(len: usize) -> Vec<String> {
    vec![
        repeated("1", len),
        repeated("z", len),
        repeated(" ", len) + "1",
        format!("0x{}", repeated("f", len)),
        format!("1e{}", repeated("9", len)),
        repeated("\u{1f600}", len),
        repeated("a ", len),
        String::new(),
    ]
}

fn soak_hostile(iterations: usize, len: usize) {
    let inputs = hostile_inputs(len);
    black_box(hostile_batch(&inputs));
    let series = soak(iterations, 50, |_| {
        black_box(hostile_batch(&inputs));
    });
    let distinct: std::collections::BTreeSet<i64> = series.iter().copied().collect();
    println!("# hostile soak: {iterations} iterations over {} strings of {len} B; distinct live-byte samples: {distinct:?}", inputs.len());
    assert_flat(&series, 0, 0);
    assert_eq!(series.last(), series.first());
}

#[test]
fn m2_soak_hostile_short() {
    soak_hostile(100, 4096);
}

#[test]
#[ignore = "1,000 iterations over 8 hostile strings of 64 KB: cargo test -p uo-jsnum --release -- --ignored m2_soak_hostile_1000"]
fn m2_soak_hostile_1000() {
    soak_hostile(1000, 65_536);
}
