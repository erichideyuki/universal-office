//! The JavaScript semantics the TS fonts code relies on (R-JS, gate 1 G8): the ECMAScript
//! whitespace set, `trim`, `split(/\s+/)`, the `lowerCp` rule of liang.ts, `toLowerCase`, and
//! `parseInt(s, 16)` on UTF-16 code units followed by the `Uint8Array` store (ToUint8).
//!
//! Private to this crate: the helpers move to `uo-jsnum` once that crate passes gate 2 and offers
//! the same functions. `tests/js_support/gen_js.mjs` writes what V8 answers for every code point
//! (and for the Final_Sigma contexts, 2-unit hex strings, the numbers a `Uint8Array` stores and
//! `split(/\s+/)`); the tests below replay it. No `f64` transcendental, rounding or `trim` of std
//! is used (the workspace bans them for the layout crates; here `f64` only multiplies by 16).

/// The ECMAScript WhiteSpace and LineTerminator set (`/\s/`, the characters `trim` removes):
/// U+0009..U+000D, U+0020, U+00A0, U+1680, U+2000..U+200A, U+2028, U+2029, U+202F, U+205F,
/// U+3000 and U+FEFF. Not U+0085, U+180E or U+200B. Lone surrogates are not whitespace.
#[inline]
pub(crate) fn is_js_space(c: u32) -> bool {
    matches!(c, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

/// `String.prototype.trim`: both ends, the [`is_js_space`] set.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| is_js_space(u32::from(c)))
}

/// `s.split(/\s+/)` with the empty tokens skipped (`if (p)`): the runs of [`is_js_space`]
/// characters separate, the tokens in between are returned.
pub(crate) fn split_js_space(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| is_js_space(u32::from(c))).filter(|token| !token.is_empty())
}

/// `lowerCp` of liang.ts: the lowercase of one code point, kept when it is exactly one code point
/// or when its length in UTF-16 units equals the original's (then its first code point);
/// otherwise the original (U+0130 stays U+0130, its lowercase being two code points of 2 units
/// against 1).
pub(crate) fn lower_cp(c: char) -> char {
    let (mut first, mut count, mut units) = (c, 0usize, 0usize);
    for l in c.to_lowercase() {
        if count == 0 {
            first = l;
        }
        count += 1;
        units += l.len_utf16();
    }
    if units == c.len_utf16() || count == 1 { first } else { c }
}

/// `String.prototype.toLowerCase`: the full Unicode lowercase mapping, Final_Sigma included
/// (what `str::to_lowercase` implements). The Unicode version of the Rust std and of V8 (ICU)
/// can differ; the test over every code point names any difference.
pub(crate) fn js_to_lowercase(s: &str) -> String {
    s.to_lowercase()
}

#[inline]
fn hex_digit(unit: u16) -> Option<u8> {
    match unit {
        0x30..=0x39 => u8::try_from(unit - 0x30).ok(),
        0x41..=0x46 => u8::try_from(unit - 0x41 + 10).ok(),
        0x61..=0x66 => u8::try_from(unit - 0x61 + 10).ok(),
        _ => None,
    }
}

/// `parseInt(s, 16)` over the UTF-16 code units of `s`: leading whitespace skipped (a lone
/// surrogate is not whitespace), an optional `+` or `-`, an optional `0x`/`0X`, then hex digits
/// up to the first other unit; no digit gives `NaN`; `-0` stays `-0.0`. Digits beyond the 30
/// most significant ones only decide the rounding (sticky bit), so the result is the correctly
/// rounded value, `Infinity` included, as V8 computes it for a power-of-two radix.
pub(crate) fn parse_int_hex(units: &[u16]) -> f64 {
    let mut i = 0;
    while units.get(i).is_some_and(|&u| is_js_space(u32::from(u))) {
        i += 1;
    }
    let negative = match units.get(i) {
        Some(0x2B) => {
            i += 1;
            false
        }
        Some(0x2D) => {
            i += 1;
            true
        }
        _ => false,
    };
    if units.get(i) == Some(&0x30) && matches!(units.get(i + 1), Some(0x78 | 0x58)) {
        i += 2;
    }
    let (mut acc, mut kept, mut dropped, mut sticky, mut any) = (0u128, 0u32, 0u32, false, false);
    while let Some(d) = units.get(i).and_then(|&u| hex_digit(u)) {
        any = true;
        if kept < 30 {
            acc = acc * 16 + u128::from(d);
            if acc != 0 {
                kept += 1;
            }
        } else {
            dropped = dropped.saturating_add(1);
            sticky |= d != 0;
        }
        i += 1;
    }
    if !any {
        return f64::NAN;
    }
    let mut value = ((acc << 1) | u128::from(sticky)) as f64 * 0.5;
    for _ in 0..dropped {
        value *= 16.0;
        if value == f64::INFINITY {
            break;
        }
    }
    if negative { -value } else { value }
}

/// ToUint8, what a number becomes when stored in a `Uint8Array`: `NaN` and the infinities give 0,
/// the value is truncated toward zero and taken modulo 256 (negatives wrap: -1 gives 255).
pub(crate) fn to_uint8(x: f64) -> u8 {
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if !(x > -LIMIT && x < LIMIT) {
        // NaN fails both comparisons; every f64 of at least 2^63 in magnitude is a multiple of 256
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "x is finite and within i64 here: truncation toward zero, then modulo 256, is ToUint8"
    )]
    let wrapped = (x as i64) as u8;
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uo_alloc::assert_no_leak;

    const EXPECTED: &str = include_str!("../tests/js_support/js_expected.txt");

    fn cps(field: &str) -> Vec<u32> {
        if field == "-" { Vec::new() } else { field.split(',').map(|h| u32::from_str_radix(h, 16).unwrap()).collect() }
    }

    fn text(field: &str) -> String {
        cps(field).into_iter().map(|c| char::from_u32(c).unwrap()).collect()
    }

    fn units(field: &str) -> Vec<u16> {
        cps(field).into_iter().map(|c| u16::try_from(c).unwrap()).collect()
    }

    fn set_of(ranges: &str) -> Vec<bool> {
        let mut set = vec![false; 0x11_0000];
        for r in ranges.split(',') {
            let (a, b) = r.split_once('-').unwrap();
            for cp in u32::from_str_radix(a, 16).unwrap()..=u32::from_str_radix(b, 16).unwrap() {
                set[cp as usize] = true;
            }
        }
        set
    }

    fn lines(kind: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
        EXPECTED.lines().filter_map(move |l| {
            let mut it = l.split('\t');
            (it.next() == Some(kind)).then(|| it.collect())
        })
    }

    fn js_number(bits: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(bits, 16).unwrap())
    }

    fn same(a: f64, b: f64) -> bool {
        (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
    }

    #[test]
    fn expected_file_names_its_origin() {
        let head: Vec<&str> = EXPECTED.lines().take(2).collect();
        assert!(head[0].starts_with("@node v"), "{head:?}");
        assert!(head[1].starts_with("@unicode "), "{head:?}");
        assert!(EXPECTED.ends_with('\n') && !EXPECTED.contains('\r'));
    }

    #[test]
    fn whitespace_matches_v8_over_every_code_point() {
        let space = set_of(lines("space").next().unwrap()[0]);
        let trim = set_of(lines("trim").next().unwrap()[0]);
        assert_eq!(space, trim);
        let mut wrong = Vec::new();
        for cp in 0..0x11_0000u32 {
            if is_js_space(cp) != space[cp as usize] {
                wrong.push(cp);
            }
            if let Some(c) = char::from_u32(cp) {
                let s = c.to_string();
                if js_trim(&s).is_empty() != trim[cp as usize] {
                    wrong.push(cp);
                }
            }
        }
        assert!(wrong.is_empty(), "differences from V8 at {wrong:x?}");
    }

    #[test]
    fn lower_cp_matches_v8_over_every_code_point() {
        let expected: HashMap<u32, u32> =
            lines("lower").map(|f| (u32::from_str_radix(f[0], 16).unwrap(), u32::from_str_radix(f[1], 16).unwrap())).collect();
        let mut wrong = Vec::new();
        for cp in (0..0x11_0000u32).filter_map(char::from_u32).map(u32::from) {
            let want = expected.get(&cp).copied().unwrap_or(cp);
            let got = u32::from(lower_cp(char::from_u32(cp).unwrap()));
            if got != want {
                wrong.push((cp, got, want));
            }
        }
        assert!(expected.len() > 1_000);
        assert!(wrong.is_empty(), "lowerCp differs from V8 (cp, rust, v8): {wrong:x?}");
        assert_eq!(lower_cp('\u{130}'), '\u{130}');
        assert_eq!(lower_cp('A'), 'a');
        assert_eq!(lower_cp('\u{3a3}'), '\u{3c3}');
    }

    #[test]
    fn to_lowercase_matches_v8_over_every_code_point() {
        let expected: HashMap<u32, String> = lines("lowerstr").map(|f| (u32::from_str_radix(f[0], 16).unwrap(), text(f[1]))).collect();
        let mut wrong = Vec::new();
        for c in (0..0x11_0000u32).filter_map(char::from_u32) {
            let s = c.to_string();
            let want = expected.get(&u32::from(c)).cloned().unwrap_or_else(|| s.clone());
            let got = js_to_lowercase(&s);
            if got != want {
                wrong.push((u32::from(c), got, want));
            }
        }
        assert!(expected.len() > 1_000);
        assert!(wrong.is_empty(), "toLowerCase differs from V8 (cp, rust, v8): {wrong:x?}");
    }

    #[test]
    fn final_sigma_contexts_match_v8() {
        let mut n = 0;
        let mut wrong = Vec::new();
        for f in lines("sigma") {
            let (input, want) = (text(f[0]), text(f[1]));
            let got = js_to_lowercase(&input);
            if got != want {
                wrong.push((input, got, want));
            }
            n += 1;
        }
        assert!(n > 1_500);
        assert!(wrong.is_empty(), "Final_Sigma differs from V8 (input, rust, v8): {wrong:?}");
        assert_eq!(js_to_lowercase("\u{3a3}"), "\u{3c3}");
        assert_eq!(js_to_lowercase("A\u{3a3}"), "a\u{3c2}");
        assert_eq!(js_to_lowercase("A\u{3a3}A"), "a\u{3c3}a");
    }

    #[test]
    fn parse_int_matches_v8() {
        let mut n = 0;
        let mut wrong = Vec::new();
        for f in lines("pi2") {
            let u1 = u16::from_str_radix(f[0], 16).unwrap();
            let results: Vec<&str> = f[1].split(',').collect();
            assert_eq!(results.len(), 128);
            for (u2, want) in results.iter().enumerate() {
                let got = parse_int_hex(&[u1, u2 as u16]);
                if !same(got, js_number(want)) {
                    wrong.push((u1, u2, got, *want));
                }
                n += 1;
            }
        }
        for f in lines("pi") {
            let u = units(f[0]);
            let got = parse_int_hex(&u);
            if !same(got, js_number(f[1])) {
                wrong.push((0xFFFF, u.len(), got, f[1]));
            }
            n += 1;
        }
        assert!(n > 20_000, "{n}");
        assert!(wrong.is_empty(), "parseInt differs from V8 (u1, u2 or units, rust, v8): {wrong:?}");
    }

    #[test]
    fn parse_int_cases_by_hand() {
        let p = |s: &str| parse_int_hex(&s.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(p("ff"), 255.0);
        assert_eq!(p("1g"), 1.0);
        assert_eq!(p("-1"), -1.0);
        assert!(p("-0").is_sign_negative() && p("-0") == 0.0);
        assert!(p("").is_nan() && p("g").is_nan() && p("0x").is_nan() && p("-").is_nan());
        assert_eq!(p(" \u{a0}0x1f"), 31.0);
        assert_eq!(p(&"f".repeat(300)), f64::INFINITY);
        assert_eq!(p(&format!("-{}", "f".repeat(300))), f64::NEG_INFINITY);
        assert!(parse_int_hex(&[0xD83D, 0x31]).is_nan());
        assert_eq!(parse_int_hex(&[0x31, 0xD83D]), 1.0);
    }

    #[test]
    fn to_uint8_matches_v8() {
        let mut n = 0;
        let mut wrong = Vec::new();
        for f in lines("u8") {
            let x = js_number(f[0]);
            let want: u8 = f[1].parse().unwrap();
            if to_uint8(x) != want {
                wrong.push((f[0], to_uint8(x), want));
            }
            n += 1;
        }
        assert!(n > 400);
        assert!(wrong.is_empty(), "ToUint8 differs from V8 (value, rust, v8): {wrong:?}");
        assert_eq!(to_uint8(-1.0), 255);
        assert_eq!(to_uint8(f64::NAN), 0);
        assert_eq!(to_uint8(f64::NEG_INFINITY), 0);
        assert_eq!(to_uint8(256.9), 0);
        assert_eq!(to_uint8(-255.9), 1);
    }

    #[test]
    fn split_matches_v8() {
        let mut n = 0;
        let mut wrong = Vec::new();
        for f in lines("split") {
            let input = text(f[0]);
            let want: Vec<String> = if f[1] == "-" { Vec::new() } else { f[1].split(';').map(text).collect() };
            let got: Vec<&str> = split_js_space(&input).collect();
            if got != want {
                wrong.push((input.clone(), got.iter().map(|s| s.to_string()).collect::<Vec<_>>(), want));
            }
            n += 1;
        }
        assert!(n > 700);
        assert!(wrong.is_empty(), "split differs from V8 (input, rust, v8): {wrong:?}");
        assert_eq!(js_trim("\u{feff} a b \u{2028}"), "a b");
        assert_eq!(js_trim("\u{85}a\u{180e}"), "\u{85}a\u{180e}");
        assert_eq!(js_trim(""), "");
    }

    #[test]
    fn m1_no_leak() {
        assert_no_leak(|| is_js_space(0x20));
        assert_no_leak(|| js_trim("  a  ").len());
        assert_no_leak(|| split_js_space(" a  b ").count());
        assert_no_leak(|| lower_cp('\u{130}'));
        assert_no_leak(|| js_to_lowercase("A\u{3a3} B"));
        assert_no_leak(|| parse_int_hex(&[0x30, 0x78, 0x66, 0x66]));
        assert_no_leak(|| parse_int_hex(&"f".repeat(100).encode_utf16().collect::<Vec<u16>>()));
        assert_no_leak(|| to_uint8(-1.0));
    }
}
