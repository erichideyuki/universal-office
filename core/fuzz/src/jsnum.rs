//! Fuzz targets of `uo-jsnum` (stage 4 security, record `core/docs/modules/uo-jsnum.md`).
//!
//! The crate parses attribute values of untrusted documents and runs in wasm and under .NET,
//! where a panic kills the host. Besides "no panic" (overflow checks are on in the fuzz
//! builds), each target checks the results against references written here independently of
//! the crate: the ECMA-262 algorithms for `Number()`, `parseInt` and the string helpers (exact
//! where a cheap exact reference exists), and for the math functions the ranges, symmetries and
//! special cases of the spec plus the platform libm within a few ulps.

use uo_jsnum::{conv, math, parse, text};

/// Reads the selector byte (0 when the input is empty).
fn split1(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&b, rest)) => (b, rest),
        None => (0, &[]),
    }
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

// ───────────────────────────── jsnum_parse ─────────────────────────────

/// The JS white space set (StrWhiteSpaceChar), written out again for the reference.
fn ref_space(c: char) -> bool {
    matches!(
        c as u32,
        0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

fn ref_trim_start(s: &str) -> &str {
    let i = s.char_indices().find(|&(_, c)| !ref_space(c)).map_or(s.len(), |(i, _)| i);
    &s[i..]
}

fn ref_trim_end(s: &str) -> &str {
    let i = s.char_indices().rev().find(|&(_, c)| !ref_space(c)).map_or(0, |(i, c)| i + c.len_utf8());
    &s[..i]
}

fn ref_digit(c: u8, radix: u32) -> Option<u32> {
    let d = match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'z' => c - b'a' + 10,
        b'A'..=b'Z' => c - b'A' + 10,
        _ => return None,
    };
    (u32::from(d) < radix).then_some(u32::from(d))
}

/// The value of the digits (radix 2^k), correctly rounded half to even: the bits written out,
/// the first 53 kept, the rest decide the rounding.
fn ref_pow2_digits(digits: &[u8], radix: u32) -> f64 {
    let k = radix.trailing_zeros();
    let mut bits: Vec<bool> = Vec::with_capacity(digits.len() * k as usize);
    for &c in digits {
        let d = ref_digit(c, radix).unwrap();
        for i in (0..k).rev() {
            bits.push((d >> i) & 1 == 1);
        }
    }
    let first = bits.iter().position(|&b| b).unwrap_or(bits.len());
    let bits = &bits[first..];
    if bits.len() <= 53 {
        return bits.iter().fold(0u64, |acc, &b| (acc << 1) | u64::from(b)) as f64;
    }
    let mut mantissa = bits[..53].iter().fold(0u64, |acc, &b| (acc << 1) | u64::from(b));
    let round = bits[53];
    let sticky = bits[54..].iter().any(|&b| b);
    let mut exponent = bits.len() - 53;
    if round && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
        if mantissa == 1 << 53 {
            mantissa >>= 1;
            exponent += 1;
        }
    }
    let mut v = mantissa as f64;
    for _ in 0..exponent {
        v *= 2.0;
        if v.is_infinite() {
            break;
        }
    }
    v
}

/// Up to 38 significant decimal digits as an integer, correctly rounded (`u128 as f64` rounds
/// to nearest even); `None` above that.
fn ref_decimal_digits(digits: &[u8]) -> Option<f64> {
    let first = digits.iter().position(|&c| c != b'0').unwrap_or(digits.len());
    let sig = &digits[first..];
    if sig.len() > 38 {
        return None;
    }
    Some(sig.iter().fold(0u128, |acc, &c| acc * 10 + u128::from(c - b'0')) as f64)
}

/// The parts of a StrUnsignedDecimalLiteral.
struct Decimal<'a> {
    int: &'a [u8],
    frac: &'a [u8],
    neg_exp: bool,
    exp: &'a [u8],
}

/// StrUnsignedDecimalLiteral, as a state machine.
fn ref_decimal(b: &[u8]) -> Option<Decimal<'_>> {
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int_end = digits(0);
    let mut i = int_end;
    let mut frac = &b[0..0];
    if b.get(i) == Some(&b'.') {
        let n = digits(i + 1);
        frac = &b[i + 1..i + 1 + n];
        i += 1 + n;
    }
    if int_end == 0 && frac.is_empty() {
        return None;
    }
    let (mut neg_exp, mut exp) = (false, &b[0..0]);
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if let Some(&s @ (b'+' | b'-')) = b.get(i) {
            neg_exp = s == b'-';
            i += 1;
        }
        let n = digits(i);
        if n == 0 {
            return None;
        }
        exp = &b[i..i + n];
        i += n;
    }
    (i == b.len()).then_some(Decimal { int: &b[..int_end], frac, neg_exp, exp })
}

/// `Number(s)` where the reference is exact (`None` when only the class of the result is
/// known: a valid decimal literal outside the fast path, which is not NaN and has the sign of
/// the literal).
enum RefNumber {
    Exact(f64),
    Decimal { negative: bool },
}

fn ref_to_number(s: &str) -> RefNumber {
    let t = ref_trim_end(ref_trim_start(s)).as_bytes();
    if t.is_empty() {
        return RefNumber::Exact(0.0);
    }
    if t.len() >= 2 && t[0] == b'0' {
        let radix = match t[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            let rest = &t[2..];
            if rest.is_empty() || rest.iter().any(|&c| ref_digit(c, radix).is_none()) {
                return RefNumber::Exact(f64::NAN);
            }
            return RefNumber::Exact(ref_pow2_digits(rest, radix));
        }
    }
    let (negative, u) = match t[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if u == b"Infinity" {
        return RefNumber::Exact(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    let Some(Decimal { int, frac, neg_exp, exp }) = ref_decimal(u) else { return RefNumber::Exact(f64::NAN) };
    // the fast path: at most 15 significant digits and |exponent| ≤ 22 are both exact, so one
    // multiplication or division rounds correctly
    let mut all: Vec<u8> = int.iter().chain(frac).copied().collect();
    let lead = all.iter().position(|&c| c != b'0').unwrap_or(all.len());
    all.drain(..lead);
    let trail = all.iter().rev().position(|&c| c != b'0').unwrap_or(all.len());
    all.truncate(all.len() - trail);
    let e_lit = if exp.len() > 6 { None } else { exp.iter().try_fold(0i64, |a, &c| Some(a * 10 + i64::from(c - b'0'))) };
    if let Some(e_lit) = e_lit {
        let e = if neg_exp { -e_lit } else { e_lit } - frac.len() as i64 + trail as i64;
        if all.is_empty() {
            return RefNumber::Exact(if negative { -0.0 } else { 0.0 });
        }
        if all.len() <= 15 && (-22..=22).contains(&e) {
            let m = all.iter().fold(0u64, |a, &c| a * 10 + u64::from(c - b'0')) as f64;
            let p = (0..e.unsigned_abs()).fold(1.0f64, |p, _| p * 10.0);
            let v = if e < 0 { m / p } else { m * p };
            return RefNumber::Exact(if negative { -v } else { v });
        }
    }
    RefNumber::Decimal { negative }
}

/// `parseInt(s, radix)` by the ECMA-262 steps; `None` when the value is not cheap to get
/// exactly (generic radix above 2^53, decimal above 38 digits): then only the class is checked.
fn ref_parse_int(s: &str, radix: i32) -> Option<f64> {
    let mut b = ref_trim_start(s).as_bytes();
    let mut negative = false;
    if let Some(&c @ (b'+' | b'-')) = b.first() {
        negative = c == b'-';
        b = &b[1..];
    }
    let mut strip_prefix = true;
    let mut r = radix;
    if r != 0 {
        if !(2..=36).contains(&r) {
            return Some(f64::NAN);
        }
        if r != 16 {
            strip_prefix = false;
        }
    } else {
        r = 10;
    }
    if strip_prefix && b.len() >= 2 && b[0] == b'0' && (b[1] == b'x' || b[1] == b'X') {
        b = &b[2..];
        r = 16;
    }
    let r = r as u32;
    let n = b.iter().take_while(|&&c| ref_digit(c, r).is_some()).count();
    if n == 0 {
        return Some(f64::NAN);
    }
    let z = &b[..n];
    let v = if r.is_power_of_two() {
        ref_pow2_digits(z, r)
    } else if r == 10 {
        let first = z.iter().position(|&c| c != b'0').unwrap_or(z.len());
        if z.len() - first > 310 { f64::INFINITY } else { ref_decimal_digits(z)? }
    } else {
        let mut acc = 0u64;
        for &c in z {
            acc = acc.checked_mul(u64::from(r))?.checked_add(u64::from(ref_digit(c, r).unwrap()))?;
            if acc > 1 << 53 {
                return None;
            }
        }
        acc as f64
    };
    Some(if negative { -v } else { v })
}

fn ref_split(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_run = false;
    for (i, c) in s.char_indices() {
        if ref_space(c) {
            if !in_run {
                out.push(&s[start..i]);
                in_run = true;
            }
            start = i + c.len_utf8();
        } else {
            in_run = false;
        }
    }
    out.push(&s[start..]);
    out
}

fn ref_utf16_slice(units: &[u16], start: i64, end: i64) -> String {
    let len = units.len() as i128;
    let clamp = |i: i64| {
        let i = i128::from(i);
        if i < 0 { (i + len).max(0) } else { i.min(len) }
    };
    let (from, to) = (clamp(start), clamp(end));
    if from >= to {
        return String::new();
    }
    String::from_utf16_lossy(&units[from as usize..to as usize])
}

const RADIXES: [i32; 7] = [-1, 1, 37, i32::MIN, i32::MAX, 0x1_0000 + 10, -16];

/// `Number()`, `parseInt` in every radix, the white space helpers and `utf16_slice` with
/// hostile ranges on any text (the input after a selector byte, decoded lossily): no panic,
/// work linear in the input, and every result equal to the reference above.
pub fn jsnum_parse(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let s = String::from_utf8_lossy(bytes);
    let s = s.as_ref();

    // Number(s)
    let n = parse::to_number(s);
    match ref_to_number(s) {
        RefNumber::Exact(v) => assert!(same(n, v), "Number({s:?}) = {n:?}, reference {v:?}"),
        RefNumber::Decimal { negative } => {
            assert!(!n.is_nan() && n.is_sign_negative() == negative, "Number({s:?}) = {n:?}")
        }
    }
    let padded = format!("\u{FEFF}\t{s}\u{3000}\n");
    assert!(same(parse::to_number(&padded), n), "white space changed Number({s:?})");

    // parseInt in every radix and the hostile ones
    for radix in (0..=36).chain(RADIXES) {
        let v = parse::parse_int(s, radix);
        match ref_parse_int(s, radix) {
            Some(r) => assert!(same(v, r), "parseInt({s:?}, {radix}) = {v:?}, reference {r:?}"),
            None => assert!(!v.is_nan() && (v.is_infinite() || v.trunc() == v), "parseInt({s:?}, {radix}) = {v:?}"),
        }
        let junk = format!("{s}!");
        assert!(same(parse::parse_int(&junk, radix), v), "trailing junk changed parseInt({s:?}, {radix})");
    }
    // the selector as a radix and as a ToInt32-converted double
    let r = i32::from(sel as i8);
    let _ = parse::parse_int(s, r);
    let _ = parse::parse_int(s, conv::to_int32(f64::from(sel) * 1e9));

    // white space helpers
    assert_eq!(text::trim_start(s), ref_trim_start(s));
    assert_eq!(text::trim_end(s), ref_trim_end(s));
    assert_eq!(text::trim(s), ref_trim_end(ref_trim_start(s)));
    assert_eq!(text::split_js_space(s).collect::<Vec<_>>(), ref_split(s), "split of {s:?}");
    for c in s.chars() {
        assert_eq!(text::is_js_space(c), ref_space(c), "{c:?}");
    }
    let b = s.as_bytes();
    for i in (0..=b.len() + 1).chain([usize::MAX]) {
        let want = if s.is_char_boundary(i) { s[i..].chars().next().filter(|&c| ref_space(c)).map_or(0, char::len_utf8) } else { 0 };
        assert_eq!(text::js_space_len(b, i), want, "js_space_len({s:?}, {i})");
    }
    // the raw bytes too: js_space_len is called on byte buffers that need not be UTF-8
    for i in 0..bytes.len() {
        let l = text::js_space_len(bytes, i);
        assert!(l == 0 || std::str::from_utf8(&bytes[i..i + l]).is_ok_and(|t| t.chars().all(ref_space)));
    }

    // UTF-16 length and slices
    let units: Vec<u16> = s.encode_utf16().collect();
    assert_eq!(text::utf16_len(s), units.len());
    let len = units.len() as i64;
    let mut idx: Vec<i64> = vec![i64::MIN, i64::MIN + 1, -len - 1, -len, -1, 0, 1, len - 1, len, len + 1, i64::MAX];
    if units.len() <= 24 {
        idx.extend(-len - 2..=len + 2);
    }
    if units.len() > 4096 {
        idx.truncate(4);
        idx.extend([len / 2, len / 2 + 1, len]);
    }
    for &a in &idx {
        for &z in &idx {
            let got = text::utf16_slice(s, a, z);
            assert_eq!(got, ref_utf16_slice(&units, a, z), "utf16_slice({s:?}, {a}, {z})");
        }
    }
}

// ───────────────────────────── jsnum_math ─────────────────────────────

/// Interesting doubles: a byte of the input that is not part of a full 8-byte value picks one.
const SPECIALS: [u64; 32] = [
    0x0000_0000_0000_0000,
    0x8000_0000_0000_0000,
    0x7FF0_0000_0000_0000,
    0xFFF0_0000_0000_0000,
    0x7FF8_0000_0000_0000,
    0x7FF0_0000_0000_0001, // signaling NaN
    0xFFFF_FFFF_FFFF_FFFF, // negative NaN with a payload
    0x0000_0000_0000_0001,
    0x000F_FFFF_FFFF_FFFF,
    0x0010_0000_0000_0000,
    0x7FEF_FFFF_FFFF_FFFF,
    0x3FF0_0000_0000_0000,
    0xBFF0_0000_0000_0000,
    0x3FE0_0000_0000_0000,
    0xBFE0_0000_0000_0000,
    0x3FDF_FFFF_FFFF_FFFF, // 0.49999999999999994
    0x4000_0000_0000_0000,
    0x400921FB54442D18,    // pi
    0x3FF921FB54442D18,    // pi/2
    0x3FE921FB54442D18,    // pi/4
    0x4139_21FB_5444_2D18, // the end of the medium rem_pio2 path
    0x7FE9_21FB_5444_2D18, // huge multiple-ish of pi
    0x4086_2E42_FEFA_39EF, // ln(MAX)
    0xC087_4910_D52D_3051, // ln(MIN subnormal)
    0x4330_0000_0000_0000, // 2^52
    0x4340_0000_0000_0000, // 2^53
    0x41E0_0000_0000_0000, // 2^31
    0x41F0_0000_0000_0000, // 2^32
    0xC1E0_0000_0000_0000, // -2^31
    0x406F_E000_0000_0000, // 255
    0x406F_F000_0000_0000, // 255.5
    0x3FF0_0000_0000_0001, // 1 + ulp
];

/// Distance in ulps on the ordered line of doubles (−0 = +0); NaN is handled by the callers.
fn ulps(a: f64, b: f64) -> u128 {
    let ord = |x: f64| {
        let b = x.to_bits();
        if b >> 63 == 1 { -i128::from(b & !(1 << 63)) } else { i128::from(b) }
    };
    (ord(a) - ord(b)).unsigned_abs()
}

/// The platform libm (glibc on the Linux fuzz job, the UCRT on Windows): an independent
/// implementation, so a mistyped constant or branch of the fdlibm port shows as a distance of
/// many ulps where the capture's distributions did not reach.
#[expect(clippy::disallowed_methods, reason = "the platform libm is the oracle of the fuzz target")]
fn libm(name: &str, x: f64) -> f64 {
    match name {
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
        "exp" => x.exp(),
        "expm1" => x.exp_m1(),
        "log" => x.ln(),
        "log1p" => x.ln_1p(),
        "log2" => x.log2(),
        "log10" => x.log10(),
        "sinh" => x.sinh(),
        "cosh" => x.cosh(),
        "tanh" => x.tanh(),
        // the UCRT `atanh` is off by up to 10^15 ulps near ±1 (atanh(1 − 2^-53) = 18.37 instead of
        // 18.71, which V8 and the port give): the identity with log1p on |x| (where 1 − |x| is
        // exact near 1) is the oracle instead
        "atanh" => {
            let a = x.abs();
            let r = 0.5 * (2.0 * a / (1.0 - a)).ln_1p();
            if x.is_sign_negative() { -r } else { r }
        }
        "cbrt" => x.cbrt(),
        _ => unreachable!(),
    }
}

#[expect(clippy::disallowed_methods, reason = "the platform libm is the oracle of the fuzz target")]
fn libm2(name: &str, a: f64, b: f64) -> f64 {
    match name {
        "pow" => a.powf(b),
        "atan2" => a.atan2(b),
        "hypot" => a.hypot(b),
        _ => unreachable!(),
    }
}

fn near(got: f64, want: f64, tol: u128) -> bool {
    if got.is_nan() || want.is_nan() {
        return got.is_nan() && want.is_nan();
    }
    // ±∞ is one ulp past ±MAX on the ordered line: V8's hypot is not correctly rounded and
    // gives MAX where the exact value rounds to ∞ (hypot(2.1e300, MAX), checked in Node)
    ulps(got, want) <= tol
}

type Unary = (&'static str, fn(f64) -> f64);

const FDLIBM: [Unary; 17] = [
    ("sin", math::sin),
    ("cos", math::cos),
    ("tan", math::tan),
    ("asin", math::asin),
    ("acos", math::acos),
    ("atan", math::atan),
    ("exp", math::exp),
    ("expm1", math::expm1),
    ("log", math::log),
    ("log1p", math::log1p),
    ("log2", math::log2),
    ("log10", math::log10),
    ("sinh", math::sinh),
    ("cosh", math::cosh),
    ("tanh", math::tanh),
    ("atanh", math::atanh),
    ("cbrt", math::cbrt),
];

const ODD: [&str; 8] = ["sin", "tan", "asin", "atan", "sinh", "tanh", "atanh", "cbrt"];
const EVEN: [&str; 2] = ["cos", "cosh"];

/// ToInt32 by the spec: truncate, then modulo 2^32 (`%` on doubles is exact).
fn ref_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let mut m = x.trunc() % 4_294_967_296.0;
    if m < 0.0 {
        m += 4_294_967_296.0;
    }
    (m as u64 as u32) as i32
}

fn ref_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 && x < 0.0 { -0.0 } else { r }
}

fn ref_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && b.is_sign_negative()) {
        a
    } else {
        b
    }
}

fn ref_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

fn check_unary(x: f64) {
    let pi = std::f64::consts::PI;
    for (name, f) in FDLIBM {
        let y = f(x);
        if x.is_nan() {
            assert!(y.is_nan(), "{name}(NaN) = {y:?}");
            continue;
        }
        // sinh/cosh between ln(MAX) and the overflow threshold square exp(|x|/2): V8 and the
        // port are 3.5 ulps from glibc at sinh(709.8706054687281) (checked in Node)
        let tol = match name {
            "atanh" => 4,
            "sinh" | "cosh" => 8,
            _ => 2,
        };
        let want = libm(name, x);
        assert!(near(y, want, tol), "{name}({x:e} = {:#018x}) = {y:e}, libm {want:e}", x.to_bits());
        let in_range = match name {
            "sin" | "cos" | "tanh" => y.is_nan() || y.abs() <= 1.0,
            "asin" | "atan" => y.is_nan() || y.abs() <= pi / 2.0,
            "acos" => y.is_nan() || (0.0..=pi).contains(&y),
            "exp" => y >= 0.0,
            "expm1" => y >= -1.0,
            "cosh" => y >= 1.0,
            _ => true,
        };
        assert!(in_range, "{name}({x:e}) = {y:e} out of range");
        if ODD.contains(&name) {
            assert!(same(f(-x), -y), "{name} is not odd at {x:e}");
        }
        if EVEN.contains(&name) {
            assert!(same(f(-x), y), "{name} is not even at {x:e}");
        }
    }

    // exactly defined ones
    assert!(same(math::round(x), ref_round(x)), "round({x:e})");
    let sign = match x {
        x if x > 0.0 => 1.0,
        x if x < 0.0 => -1.0,
        x => x,
    };
    assert!(same(math::sign(x), sign), "sign({x:e})");
    assert!(same(math::floor(x), x.floor()) && same(math::ceil(x), x.ceil()) && same(math::trunc(x), x.trunc()));
    assert!(same(math::abs(x), x.abs()) && same(math::sqrt(x), x.sqrt()));
    let fr = math::fround(x);
    assert!(same(math::fround(fr), fr), "fround is not idempotent at {x:e}");
    assert!(same(math::clz32(x), f64::from((ref_int32(x) as u32).leading_zeros())), "clz32({x:e})");

    // conversions
    let i = ref_int32(x);
    assert_eq!(conv::to_int32(x), i, "to_int32({x:e})");
    assert_eq!(conv::to_uint32(x), i as u32);
    assert_eq!(conv::to_int16(x), i as i16);
    assert_eq!(conv::to_uint16(x), i as u16);
    assert_eq!(conv::to_int8(x), i as i8);
    assert_eq!(conv::to_uint8(x), i as u8);
    let clamp = if x.is_nan() || x <= 0.0 {
        0
    } else if x >= 255.0 {
        255
    } else {
        let f = x.floor();
        let d = x - f;
        (if d > 0.5 || (d == 0.5 && f % 2.0 == 1.0) { f + 1.0 } else { f }) as u8
    };
    assert_eq!(conv::to_uint8_clamp(x), clamp, "to_uint8_clamp({x:e})");
    let c = conv::canonicalize_nan(x);
    assert!(if x.is_nan() { c.to_bits() == 0x7FF8_0000_0000_0000 } else { c.to_bits() == x.to_bits() });
    assert_eq!(conv::truthy(x), !(x == 0.0 || x.is_nan()));
    assert_eq!(conv::is_integer(x), x.is_finite() && x.trunc() == x);
    assert_eq!(conv::is_safe_integer(x), x.is_finite() && x.trunc() == x && x.abs() <= 9_007_199_254_740_991.0);
    for len in [0usize, 1, 7, 255, u32::MAX as usize, 1 << 53, usize::MAX] {
        let got = conv::js_index(x, len);
        let exact = (!x.is_nan() && x >= 0.0 && x.trunc() == x && x < 18_446_744_073_709_551_616.0)
            .then_some(x as u64)
            .filter(|&k| u128::from(k) < len as u128);
        if len <= 1 << 53 {
            assert_eq!(got, exact.map(|k| k as usize), "js_index({x:e}, {len})");
        } else if let Some(k) = got {
            assert!(k < len, "js_index({x:e}, {len}) = {k}");
        }
    }

    // Number() of the shortest decimal Rust prints gives the same bits back
    if x.is_finite() {
        for s in [format!("{x:e}"), format!("{x}"), format!("{x:+.17e}"), format!("  {x:E}\u{2028}")] {
            let n = parse::to_number(&s);
            assert_eq!(n.to_bits(), x.to_bits(), "Number({s:?})");
        }
        if x.trunc() == x && x.abs() < 1e300 {
            let s = format!("{x:.0}");
            assert_eq!(parse::parse_int(&s, 10).to_bits(), x.to_bits(), "parseInt({s:?})");
        }
    }
}

fn check_binary(a: f64, b: f64) {
    // pow: the special cases of ECMA-262 Number::exponentiate, else the libm
    let p = math::pow(a, b);
    if b.is_nan() {
        assert!(p.is_nan());
    } else if b == 0.0 {
        assert_eq!(p, 1.0, "pow({a:e}, ±0)");
    } else if a.is_nan() || (b.is_infinite() && a.abs() == 1.0) {
        assert!(p.is_nan(), "pow({a:e}, {b:e}) = {p:e}");
    } else {
        let want = libm2("pow", a, b);
        assert!(near(p, want, 2), "pow({a:e} = {:#018x}, {b:e} = {:#018x}) = {p:e}, libm {want:e}", a.to_bits(), b.to_bits());
    }
    let t = math::atan2(a, b);
    let want = libm2("atan2", a, b);
    assert!(near(t, want, 2), "atan2({a:e}, {b:e}) = {t:e}, libm {want:e}");
    assert!(t.is_nan() || t.abs() <= std::f64::consts::PI);

    let h = math::hypot(a, b);
    if a.is_infinite() || b.is_infinite() {
        assert_eq!(h, f64::INFINITY);
    } else if a.is_nan() || b.is_nan() {
        assert!(h.is_nan());
    } else {
        let want = libm2("hypot", a, b);
        assert!(near(h, want, 4), "hypot({a:e}, {b:e}) = {h:e}, libm {want:e}");
    }
    assert!(same(math::hypot_n(&[a, b]), h));

    assert!(same(math::max(a, b), ref_max(a, b)), "max({a:e}, {b:e})");
    assert!(same(math::min(a, b), ref_min(a, b)), "min({a:e}, {b:e})");
    assert_eq!(math::imul(a, b), f64::from(ref_int32(a).wrapping_mul(ref_int32(b))));
    let r = math::rem(a, b);
    assert!(r.is_nan() || r == 0.0 || r.is_sign_negative() == a.is_sign_negative(), "{a:e} % {b:e} = {r:e}");
}

fn check_list(v: &[f64]) {
    let mx = v.iter().fold(f64::NEG_INFINITY, |acc, &x| ref_max(acc, x));
    let mn = v.iter().fold(f64::INFINITY, |acc, &x| ref_min(acc, x));
    assert!(same(math::max_n(v), mx));
    assert!(same(math::min_n(v), mn));

    let h = math::hypot_n(v);
    let big = v.iter().fold(0.0f64, |acc, &x| if x.abs() > acc { x.abs() } else { acc });
    if v.iter().any(|x| x.is_infinite()) {
        assert_eq!(h, f64::INFINITY);
    } else if v.iter().any(|x| x.is_nan()) {
        assert!(h.is_nan());
    } else if v.len() > 1 {
        let eps = f64::EPSILON * 8.0;
        let n = v.len() as f64;
        assert!(h >= big * (1.0 - eps), "hypot_n {v:?} = {h:e} below the largest");
        assert!(h <= big * n.sqrt() * (1.0 + eps) || h.is_infinite(), "hypot_n {v:?} = {h:e} above √n·max");
    }
    if let [a, b, c] = *v {
        assert!(same(math::hypot3(a, b, c), h));
    }
}

/// Every math function and conversion on arbitrary bit patterns (8 bytes each; a leftover
/// byte picks a special value): no panic in debug arithmetic, the spec's special cases and
/// ranges, odd/even symmetry of the fdlibm functions, the exact ones against the spec, and the
/// transcendental ones within 2 ulps of the platform libm (4 for `hypot`).
pub fn jsnum_math(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let chunks = bytes.chunks_exact(8);
    let rest = chunks.remainder();
    let mut v: Vec<f64> = chunks.map(|c| f64::from_bits(u64::from_le_bytes(c.try_into().unwrap()))).collect();
    v.extend(rest.iter().map(|&b| f64::from_bits(SPECIALS[usize::from(b) % SPECIALS.len()])));
    let mut k = usize::from(sel);
    while v.len() < 3 {
        v.push(f64::from_bits(SPECIALS[k % SPECIALS.len()]));
        k = k / 2 + 7;
    }
    v.truncate(64);
    for &x in &v {
        check_unary(x);
    }
    for w in v.windows(2) {
        check_binary(w[0], w[1]);
        check_binary(w[1], w[0]);
    }
    check_list(&v);
    check_list(&v[..3]);
    check_list(&v[..2]);
}
