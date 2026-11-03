//! The generators and digests of the adversarial parity tests (`tests/adv.rs`, the wasm self-test
//! of `uo-wasm`): structured strings for `to_number` and `parseInt`, the math functions on the
//! arguments where the algorithms branch, and the integer conversions over every exponent. Each
//! mirrors `js/capture-jsnum-adv.mjs` draw for draw.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use uo_jsnum::conv::{canonicalize_nan, to_int8, to_int16, to_int32, to_uint8, to_uint8_clamp, to_uint16, to_uint32};
use uo_jsnum::math;
use uo_jsnum::parse::{parse_int, to_number};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

use super::{TWO53, fnv, from_words};

const LENS: [usize; 16] = [0, 1, 1, 2, 3, 5, 8, 15, 16, 17, 18, 19, 22, 40, 120, 330];
const SIGN: [&str; 4] = ["", "", "+", "-"];
const EXPLENS: [usize; 10] = [1, 1, 2, 2, 3, 3, 4, 5, 10, 20];
const WS_ALL: [&str; 25] = [
    "\t", "\n", "\u{b}", "\u{c}", "\r", " ", "\u{a0}", "\u{1680}", "\u{2000}", "\u{2001}", "\u{2002}", "\u{2003}", "\u{2004}", "\u{2005}",
    "\u{2006}", "\u{2007}", "\u{2008}", "\u{2009}", "\u{200a}", "\u{2028}", "\u{2029}", "\u{202f}", "\u{205f}", "\u{3000}", "\u{feff}",
];
const WS_NEG: [&str; 6] = ["\u{85}", "\u{180e}", "\u{200b}", "\u{2060}", "\u{ad}", "\u{0}"];

fn pick<'a, T>(r: &mut Rng, a: &'a [T]) -> &'a T {
    &a[(r.next_u32() as usize) % a.len()]
}

fn lenpick(r: &mut Rng) -> usize {
    LENS[(r.next_u32() % 16) as usize]
}

fn digits_rand(r: &mut Rng, n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push(char::from(b'0' + (r.next_u32() % 10) as u8));
    }
    s
}

fn zeroish(r: &mut Rng, n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        if r.next_u32().is_multiple_of(16) {
            s.push(char::from(b'0' + (r.next_u32() % 10) as u8));
        } else {
            s.push('0');
        }
    }
    s
}

fn nd(r: &mut Rng, max: u32) -> String {
    let n = 1 + (r.next_u32() % max) as usize;
    digits_rand(r, n)
}

fn pad(r: &mut Rng) -> String {
    if r.next_u32().is_multiple_of(2) {
        return String::new();
    }
    let n = r.next_u32() % 4;
    let mut s = String::new();
    for _ in 0..n {
        if r.next_u32().is_multiple_of(16) {
            s.push_str(pick(r, &WS_NEG));
        } else {
            s.push_str(pick(r, &WS_ALL));
        }
    }
    s
}

fn decimal(r: &mut Rng, zero_heavy: bool) -> String {
    let gen_digits = |r: &mut Rng, n: usize| if zero_heavy { zeroish(r, n) } else { digits_rand(r, n) };
    let mut s = (*pick(r, &SIGN)).to_owned();
    let mut n = lenpick(r);
    s += &gen_digits(r, n);
    if r.next_u32() % 2 == 1 {
        s.push('.');
        n = lenpick(r);
        s += &gen_digits(r, n);
    }
    if r.next_u32().is_multiple_of(3) {
        s += pick(r, &["e", "E"]);
        s += pick(r, &["", "", "+", "-"]);
        n = *pick(r, &EXPLENS);
        s += &digits_rand(r, n);
    }
    s
}

const BOUND: [(&str, i32); 20] = [
    ("17976931348623157", 292),
    ("17976931348623158", 292),
    ("17976931348623159", 292),
    ("22250738585072014", -324),
    ("22250738585072011", -324),
    ("49406564584124654", -340),
    ("24703282292062328", -340),
    ("24703282292062327", -340),
    ("24703282292062329", -340),
    ("9007199254740993", 0),
    ("9007199254740995", 0),
    ("9007199254740992", 0),
    ("18014398509481985", 0),
    ("9", 23),
    ("1", 22),
    ("1", 23),
    ("5", -324),
    ("1", -323),
    ("3", -324),
    ("2", -324),
];

fn boundary(r: &mut Rng) -> String {
    let (mant, e) = *pick(r, &BOUND);
    let mut m = mant.to_owned();
    let mut exp = e;
    if !r.next_u32().is_multiple_of(3) {
        let k = lenpick(r);
        m += &"0".repeat(k);
        exp -= k as i32;
        if r.next_u32() % 2 == 1 {
            m.push('1');
            exp -= 1;
        }
    }
    let mut s = (*pick(r, &SIGN)).to_owned() + &m;
    s += pick(r, &["e", "E"]);
    if exp >= 0 && r.next_u32() % 2 == 1 {
        s.push('+');
    }
    s += &exp.to_string();
    s
}

const PREFIX: [&str; 6] = ["0x", "0X", "0o", "0O", "0b", "0B"];
const RADIX_OF: [u32; 6] = [16, 16, 8, 8, 2, 2];

fn prefixed(r: &mut Rng) -> String {
    let i = (r.next_u32() % 6) as usize;
    let radix = RADIX_OF[i];
    let upper = r.next_u32().is_multiple_of(4);
    let ch = |d: u32| {
        let c = char::from_digit(d, 16).unwrap();
        if upper { c.to_ascii_uppercase() } else { c }
    };
    let mut s = String::new();
    if r.next_u32().is_multiple_of(8) {
        s += pick(r, &["+", "-"]);
    }
    s += PREFIX[i];
    let mode = r.next_u32() % 3;
    if mode == 0 {
        let n = lenpick(r);
        for _ in 0..n {
            s.push(ch(r.next_u32() % radix));
        }
    } else if mode == 1 {
        s.push(ch(1 + (r.next_u32() % (radix - 1))));
        let mut n = lenpick(r);
        s += &"0".repeat(n);
        s.push(ch(r.next_u32() % radix));
        n = lenpick(r);
        s += &"0".repeat(n);
        if r.next_u32() % 2 == 1 {
            s.push('1');
        }
    } else {
        let n = lenpick(r);
        s += &ch(radix - 1).to_string().repeat(n);
    }
    if r.next_u32().is_multiple_of(8) {
        s += pick(r, &["g", "_", " ", ".", "8", "9", "\u{a0}", "x", "z"]);
        if r.next_u32() % 2 == 1 {
            let n = lenpick(r);
            for _ in 0..n {
                s.push(ch(r.next_u32() % radix));
            }
        }
    }
    s
}

const INF: [&str; 13] = [
    "Infinity",
    "Infinity",
    "Infinity",
    "infinity",
    "INFINITY",
    "Infinit",
    "Infinityx",
    "Infinity ",
    "Infinity.0",
    "Infinity1",
    "Infinity\u{a0}",
    "Inf",
    "InfinityInfinity",
];

fn infinity(r: &mut Rng) -> String {
    let mut s = (*pick(r, &SIGN)).to_owned();
    s += pick(r, &INF);
    s
}

const TOKS: [&str; 36] = [
    "0", "1", "5", "9", "a", "f", "z", "x", "b", "o", "e", "E", "+", "-", ".", " ", "\t", "\u{a0}", "\u{feff}", "\u{85}", "\u{180e}", "_",
    "Infinity", "0x", "0b", "0o", "\u{2003}", "\u{661}", "\u{ff11}", "1e5", ".5", "5.", "e+", "e-", "NaN", "\u{200b}",
];

fn soup(r: &mut Rng) -> String {
    let n = r.next_u32() % 14;
    let mut s = String::new();
    for _ in 0..n {
        s += pick(r, &TOKS);
    }
    s
}

fn exponent(r: &mut Rng) -> String {
    let mut s = (*pick(r, &SIGN)).to_owned();
    let mut n = 1 + (r.next_u32() % 20) as usize;
    s += &digits_rand(r, n);
    if r.next_u32() % 2 == 1 {
        n = 1 + (r.next_u32() % 20) as usize;
        s.push('.');
        s += &digits_rand(r, n);
    }
    let kind = r.next_u32() % 4;
    if kind == 3 {
        s += pick(r, &["e", "E"]);
        s += pick(r, &["", "+", "-"]);
        n = 6 + (r.next_u32() % 20) as usize;
        s += &digits_rand(r, n);
    } else {
        let e: i64 = if kind == 0 {
            300 + i64::from(r.next_u32() % 15)
        } else if kind == 1 {
            -(300 + i64::from(r.next_u32() % 35))
        } else {
            i64::from(r.next_u32() % 2000) - 1000
        };
        s.push('e');
        s += &e.to_string();
    }
    s
}

fn subnormal(r: &mut Rng) -> String {
    let mut s = (*pick(r, &SIGN)).to_owned();
    let kind = r.next_u32() % 3;
    if kind == 0 {
        let z = 300 + (r.next_u32() % 30) as usize;
        s += "0.";
        s += &"0".repeat(z);
        s += &nd(r, 20);
    } else if kind == 1 {
        s += &nd(r, 20);
        s += "e-";
        s += &(300 + r.next_u32() % 30).to_string();
    } else {
        s += &nd(r, 20);
        s.push('.');
        s += &nd(r, 20);
        s += "e-";
        s += &(300 + r.next_u32() % 30).to_string();
    }
    s
}

fn dots(r: &mut Rng) -> String {
    let mut s = (*pick(r, &SIGN)).to_owned();
    let k = r.next_u32() % 8;
    if k == 0 {
        s.push('.');
        s += &nd(r, 5);
    } else if k == 1 {
        s += &nd(r, 5);
        s.push('.');
    } else if k == 2 {
        s.push('.');
    } else if k == 3 {
        s += ".e";
        s += &nd(r, 3);
    } else if k == 4 {
        s += &nd(r, 3);
        s += ".e";
        s += &nd(r, 3);
    } else if k == 5 {
        s.push('.');
        s += &nd(r, 3);
        s.push('e');
        s += pick(r, &["", "+", "-"]);
        s += &nd(r, 3);
    } else if k == 6 {
        s += &nd(r, 3);
        s.push('.');
        s += &nd(r, 3);
        s.push('.');
        s += &nd(r, 3);
    } else {
        s += &nd(r, 3);
        s += pick(r, &["e", "e+", "E-"]);
    }
    s
}

const TOTAL: [usize; 11] = [305, 308, 309, 309, 310, 310, 311, 312, 320, 400, 800];
const MANT9: [&str; 9] =
    ["17976931348623157", "17976931348623158", "17976931348623159", "18", "1", "9", "99", "12345678901234567", "17976931348623156"];
const TAIL9: [&str; 10] = ["", "", ".5", "e5", "e-5", "x", " 7", ".", "e+400", "_1"];

fn long_integer(r: &mut Rng) -> String {
    let mant = *pick(r, &MANT9);
    let total = *pick(r, &TOTAL);
    let mut s = mant.to_owned();
    if r.next_u32().is_multiple_of(3) {
        s += &"9".repeat(total - mant.len());
    } else {
        s += &zeroish(r, total - mant.len());
    }
    if r.next_u32().is_multiple_of(4) {
        let n = lenpick(r);
        s = "0".repeat(n) + &s;
    }
    s += pick(r, &TAIL9);
    (*pick(r, &SIGN)).to_owned() + &s
}

const SEPS: [&str; 12] = [" ", "\u{a0}", "\t", "_", ",", "'", " e ", "-", "+", "\u{2028}", "\u{85}", "\u{feff}"];

fn separated(r: &mut Rng) -> String {
    let mut s = nd(r, 5);
    s += pick(r, &SEPS);
    s += &nd(r, 5);
    if r.next_u32() % 2 == 1 {
        s += pick(r, &SEPS);
    }
    s
}

const UDIGITS: [&str; 5] = ["\u{661}", "\u{ff11}", "\u{663}", "\u{6f4}", "\u{966}"];

fn unicode_digits(r: &mut Rng) -> String {
    let n = 1 + (r.next_u32() % 8);
    let mut s = String::new();
    for _ in 0..n {
        if r.next_u32().is_multiple_of(2) {
            s += &digits_rand(r, 1);
        } else {
            s += pick(r, &UDIGITS);
        }
    }
    s
}

const NCATS: u32 = 12;
const RADICES: [i32; 9] = [0, 10, 16, 2, 8, 36, 7, 32, 3];

fn category(r: &mut Rng, cat: u32) -> String {
    match cat {
        0 => decimal(r, false),
        1 => decimal(r, true),
        2 => boundary(r),
        3 => prefixed(r),
        4 => infinity(r),
        5 => soup(r),
        6 => exponent(r),
        7 => subnormal(r),
        8 => dots(r),
        9 => long_integer(r),
        10 => separated(r),
        _ => unicode_digits(r),
    }
}

fn sample(r: &mut Rng, fixed: Option<u32>) -> String {
    let cat = fixed.unwrap_or_else(|| r.next_u32() % NCATS);
    let lead = pad(r);
    let core = category(r, cat);
    let trail = pad(r);
    lead + &core + &trail
}

pub struct Rec {
    pub key: String,
    fixed: Option<u32>,
    pub n_small: usize,
}

pub fn generated_records() -> Vec<Rec> {
    let mut v: Vec<Rec> = (0..NCATS).map(|c| Rec { key: format!("adv/cat{c}"), fixed: Some(c), n_small: 20_000 }).collect();
    v.push(Rec { key: "adv/mixed".into(), fixed: None, n_small: 100_000 });
    v
}

/// The digest of the results of the first `n` strings of the record.
pub fn generated_digest(rec: &Rec, n: usize) -> String {
    let mut rng = Rng::new(fnv(&rec.key));
    let mut s = Stream::new();
    for _ in 0..n {
        let str = sample(&mut rng, rec.fixed);
        s.f64(canonicalize_nan(to_number(&str)));
        for radix in RADICES {
            s.f64(canonicalize_nan(parse_int(&str, radix)));
        }
    }
    s.finish().short()
}

/// `(values, digest)` of the integer conversions over every exponent (record `conv/exp_sweep`).
pub fn conv_sweep() -> (usize, String) {
    let key = "conv/exp_sweep";
    let mut rng = Rng::new(fnv(key));
    let mut mants: Vec<u64> = vec![
        0,
        1,
        2,
        0x7_ffff_ffff_ffff,
        0x8_0000_0000_0000,
        0x8_0000_0000_0001,
        0xf_ffff_ffff_ffff,
        0xf_ffff_ffff_fffe,
        0x1_0000_0000,
        0xffff_ffff,
        0x2_0000_0001,
    ];
    for _ in 0..8 {
        let hi = u64::from(rng.next_u32() & 0xf_ffff);
        mants.push((hi << 32) | u64::from(rng.next_u32()));
    }
    let mut s = Stream::new();
    let mut n = 0usize;
    for e in 0..2048u64 {
        for &m in &mants {
            for sign in [0u64, 1] {
                let x = f64::from_bits((sign << 63) | (e << 52) | m);
                s.f64(f64::from(to_int32(x)));
                s.f64(f64::from(to_uint32(x)));
                s.f64(f64::from(to_int16(x)));
                s.f64(f64::from(to_uint16(x)));
                s.f64(f64::from(to_int8(x)));
                s.f64(f64::from(to_uint8(x)));
                s.f64(f64::from(to_uint8_clamp(x)));
                n += 1;
            }
        }
    }
    (n, s.finish().short())
}

// ── the math functions on the arguments where the algorithms branch (mirrors the script) ───────

fn nudge(x: f64, j: i64) -> f64 {
    f64::from_bits(x.to_bits().wrapping_add(j as u64))
}

fn pow2(e: i64) -> f64 {
    f64::from_bits(((1023 + e) as u64) << 52)
}

fn jitter(r: &mut Rng) -> i64 {
    i64::from(r.next_u32() % 7) - 3
}

fn urange(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    let a = r.next_u32();
    let b = r.next_u32();
    let u = (f64::from(a >> 5) * 67_108_864.0 + f64::from(b >> 6)) / TWO53;
    lo + (hi - lo) * u
}

fn signed(r: &mut Rng, x: f64) -> f64 {
    if r.next_u32() & 1 == 1 { -x } else { x }
}

const PIO2: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);

#[derive(Clone, Copy)]
enum EGen {
    Pio2k20,
    Pio2kBig,
    Ln2k,
    Ln2Half,
    Thr,
    Near1,
    NearM1,
    NearSqrt2,
    Pow2Nudge,
    TinyK,
    Scaled,
    Cubes,
    ExpHi,
    ExpLo,
}

/// The generators that carry an `s` in the script (a random sign after the draw).
fn is_signed(g: EGen) -> bool {
    matches!(g, EGen::Pio2k20 | EGen::Pio2kBig | EGen::Scaled | EGen::Cubes)
}

fn draw(g: EGen, r: &mut Rng, thr: &[u32]) -> f64 {
    let x = match g {
        EGen::Pio2k20 => {
            let k = 1 + (r.next_u32() % 1_048_576);
            let j = jitter(r);
            nudge(f64::from(k) * PIO2, j)
        }
        EGen::Pio2kBig => {
            let k = urange(r, 1.0, 1_152_921_504_606_846_976.0).floor();
            let j = jitter(r);
            nudge(k * PIO2, j)
        }
        EGen::Ln2k => {
            let k = urange(r, -1075.0, 1025.0).floor();
            let j = jitter(r);
            nudge(k * std::f64::consts::LN_2, j)
        }
        EGen::Ln2Half => {
            let k = urange(r, -1075.0, 1025.0).floor();
            let j = jitter(r);
            nudge((k + 0.5) * std::f64::consts::LN_2, j)
        }
        EGen::Thr => {
            let hi = thr[(r.next_u32() as usize) % thr.len()];
            let d = r.next_u32() % 2;
            let lo = r.next_u32();
            let neg = r.next_u32() & 1;
            from_words(hi.wrapping_sub(d) | if neg == 1 { 0x8000_0000 } else { 0 }, lo)
        }
        EGen::Near1 => {
            let k = 1 + i64::from(r.next_u32() % 52);
            let u = urange(r, -1.0, 1.0);
            1.0 + u * pow2(-k)
        }
        EGen::NearM1 => {
            let k = 1 + i64::from(r.next_u32() % 52);
            let u = urange(r, 0.0, 1.0);
            -1.0 + pow2(-k) * (1.0 + u)
        }
        EGen::NearSqrt2 => {
            let e = i64::from(r.next_u32() % 2001) - 1000;
            let half = r.next_u32() % 2 == 1;
            let j = jitter(r);
            nudge(if half { std::f64::consts::FRAC_1_SQRT_2 } else { std::f64::consts::SQRT_2 }, j) * pow2(e)
        }
        EGen::Pow2Nudge => {
            let e = i64::from(r.next_u32() % 2001) - 1000;
            let j = jitter(r);
            nudge(pow2(e), j)
        }
        EGen::TinyK => {
            let k = 1 + i64::from(r.next_u32() % 60);
            let u = urange(r, -1.0, 1.0);
            u * pow2(-k)
        }
        EGen::Scaled => {
            let e = i64::from(r.next_u32() % 2041) - 1020;
            let u = urange(r, 1.0, 2.0);
            u * pow2(e)
        }
        EGen::Cubes => {
            let n = f64::from(1 + (r.next_u32() % 131_072));
            n * n * n
        }
        EGen::ExpHi => urange(r, 709.7, 709.8),
        EGen::ExpLo => urange(r, -745.2, -745.1),
    };
    if is_signed(g) { signed(r, x) } else { x }
}

fn gen_name(g: EGen) -> &'static str {
    match g {
        EGen::Pio2k20 => "pio2k20s",
        EGen::Pio2kBig => "pio2kbigs",
        EGen::Ln2k => "ln2k",
        EGen::Ln2Half => "ln2half",
        EGen::Thr => "thr",
        EGen::Near1 => "near1",
        EGen::NearM1 => "nearm1",
        EGen::NearSqrt2 => "nearsqrt2",
        EGen::Pow2Nudge => "pow2nudge",
        EGen::TinyK => "tinyk",
        EGen::Scaled => "scaleds",
        EGen::Cubes => "cubess",
        EGen::ExpHi => "exphi",
        EGen::ExpLo => "explo",
    }
}

fn ext_op(name: &str) -> fn(&[f64]) -> f64 {
    match name {
        "sin" => |a| math::sin(a[0]),
        "cos" => |a| math::cos(a[0]),
        "tan" => |a| math::tan(a[0]),
        "asin" => |a| math::asin(a[0]),
        "acos" => |a| math::acos(a[0]),
        "atan" => |a| math::atan(a[0]),
        "atan2" => |a| math::atan2(a[0], a[1]),
        "exp" => |a| math::exp(a[0]),
        "expm1" => |a| math::expm1(a[0]),
        "log" => |a| math::log(a[0]),
        "log1p" => |a| math::log1p(a[0]),
        "log2" => |a| math::log2(a[0]),
        "log10" => |a| math::log10(a[0]),
        "sinh" => |a| math::sinh(a[0]),
        "cosh" => |a| math::cosh(a[0]),
        "tanh" => |a| math::tanh(a[0]),
        "atanh" => |a| math::atanh(a[0]),
        "cbrt" => |a| math::cbrt(a[0]),
        "hypot" => |a| math::hypot(a[0], a[1]),
        "hypot3" => |a| math::hypot3(a[0], a[1], a[2]),
        "hypot_n4" => math::hypot_n,
        other => panic!("no operation {other}"),
    }
}

pub struct Ext {
    pub key: String,
    op: &'static str,
    gens: Vec<EGen>,
}

pub fn ext_records() -> Vec<Ext> {
    use EGen::*;
    let mut v = Vec::new();
    let mut ext = |op: &'static str, dists: &[&[EGen]]| {
        for d in dists {
            let names: Vec<&str> = d.iter().map(|g| gen_name(*g)).collect();
            v.push(Ext { key: format!("ext/{op}/{}", names.join("+")), op, gens: d.to_vec() });
        }
    };
    for op in ["sin", "cos", "tan"] {
        ext(op, &[&[Pio2k20], &[Pio2kBig]]);
    }
    for op in [
        "sin", "cos", "tan", "asin", "acos", "atan", "exp", "expm1", "log", "log1p", "log2", "log10", "sinh", "cosh", "tanh", "atanh",
        "cbrt",
    ] {
        ext(op, &[&[Thr]]);
    }
    for op in ["exp", "expm1", "sinh", "cosh", "tanh"] {
        ext(op, &[&[Ln2k], &[Ln2Half]]);
    }
    ext("exp", &[&[ExpHi], &[ExpLo]]);
    ext("expm1", &[&[ExpHi]]);
    for op in ["log", "log2", "log10"] {
        ext(op, &[&[Near1], &[NearSqrt2], &[Pow2Nudge]]);
    }
    ext("log1p", &[&[TinyK], &[NearM1], &[Near1]]);
    for op in ["asin", "acos"] {
        ext(op, &[&[Near1]]);
    }
    ext("atanh", &[&[Near1], &[TinyK]]);
    ext("atan", &[&[Scaled]]);
    ext("atan2", &[&[Thr, Thr], &[Scaled, Scaled]]);
    ext("hypot", &[&[Scaled, Scaled]]);
    ext("hypot3", &[&[Scaled, Scaled, Scaled]]);
    ext("hypot_n4", &[&[Scaled, Scaled, Scaled, Scaled]]);
    ext("cbrt", &[&[Cubes], &[Scaled]]);
    v
}

/// The digest of the results of the first `n` samples of the record.
pub fn ext_digest(rec: &Ext, n: usize, thresholds: &[u32]) -> String {
    let mut rng = Rng::new(fnv(&rec.key));
    let mut s = Stream::new();
    let f = ext_op(rec.op);
    let mut args = vec![0.0; rec.gens.len()];
    for _ in 0..n {
        for (a, g) in args.iter_mut().zip(&rec.gens) {
            *a = draw(*g, &mut rng, thresholds);
        }
        s.f64(canonicalize_nan(f(&args)));
    }
    s.finish().short()
}

/// `(cases, digest)` of typed-array access by number (record `js_index/sweep`).
pub fn js_index_sweep() -> (usize, String) {
    use uo_jsnum::conv::js_index;
    let mut values = super::inputs().specials;
    values.extend([
        -0.0,
        0.0,
        0.5,
        1.5,
        2.999_999_999_999_999_6,
        3.0,
        4_294_967_295.0,
        4_294_967_296.0,
        9_007_199_254_740_992.0,
        9_007_199_254_740_994.0,
        1e21,
        1e300,
        5e-324,
        -5e-324,
        -1.0,
        -2.0,
        1.0 / 3.0,
        19.0,
        20.0,
        21.0,
        19.5,
        20.5,
    ]);
    let mut s = Stream::new();
    let mut n = 0usize;
    for len in [0usize, 1, 2, 3, 20] {
        for &i in &values {
            let mut ta: Vec<f64> = (0..len).map(|k| (k + 1) as f64).collect();
            let idx = js_index(i, len);
            s.f64(idx.map_or(-1.0, |k| ta[k]));
            if let Some(k) = idx {
                ta[k] = 99.0;
            }
            s.f64(ta.iter().fold(0.0, |a, &b| a + b));
            n += 1;
        }
    }
    (n, s.finish().short())
}
