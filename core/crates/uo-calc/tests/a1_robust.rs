//! A1 robustness (seeded, seconds): no panic and bounded time on hostile inputs — random UTF-16 text (lone
//! surrogates, digits of both scripts, every separator) through the coercions, dates and criteria under random host
//! locales (empty, multi-unit and digit separators), random bit patterns through every numeric function, and the
//! patterns that make V8's backtracking exponential (record `uo-calc.md` "A1", declared A1-1/A1-3).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use uo_calc::coerce::{compare_scalars, parse_date_time_parts, parse_number_text, same_text_ci, to_bool};
use uo_calc::collate::{compare_text_js, round15};
use uo_calc::criteria::{Wildcard, parse_criterion};
use uo_calc::math_util::{RoundMode, decimal_round, excel_power};
use uo_calc::{JsStr, JsString, ScalarView, ValueLocale, dates, libm, special, stats};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn f64(&mut self) -> f64 {
        match self.below(4) {
            0 => f64::from_bits(self.next()),
            1 => (self.next() % 2_000_000) as f64 / 1000.0 - 1000.0,
            2 => [f64::NAN, f64::INFINITY, -0.0, 1e308, 5e-324, 1e21, 9_007_199_254_740_992.0][self.below(7)],
            _ => (self.next() % 100) as f64,
        }
    }
}

const PIECES: &[&str] = &[
    "0",
    "1",
    "9",
    "123",
    "2024",
    "٣",
    "٫",
    ".",
    ",",
    " ",
    "\u{a0}",
    "\t",
    "'",
    "/",
    "-",
    ":",
    "%",
    "e",
    "E+",
    "(",
    ")",
    "+",
    "R$",
    "$",
    "€",
    "AM",
    "pm",
    "jan",
    "fevereiro",
    "März",
    "~",
    "*",
    "?",
    "<",
    ">=",
    "<>",
    "=",
    "#N/D",
    "#n/a",
    "VERDADEIRO",
    "true",
    "ß",
    "İ",
    "😀",
    "a",
];

fn text(r: &mut Rng) -> Vec<u16> {
    let mut u: Vec<u16> = Vec::new();
    for _ in 0..r.below(10) {
        if r.below(12) == 0 {
            u.push(0xD800 + r.below(0x800) as u16); // lone surrogate
        } else {
            u.extend(PIECES[r.below(PIECES.len())].encode_utf16());
        }
    }
    u
}

fn locale(r: &mut Rng) -> ValueLocale {
    let mut l = if r.below(2) == 0 { ValueLocale::pt_br() } else { ValueLocale::en_us() };
    let seps = ["", ".", ",", " ", "1", "e", "::", "٫", "\u{202f}", "-"];
    let pick = |r: &mut Rng| JsString::from_str_js(seps[r.below(seps.len())]);
    if r.below(2) == 0 {
        l.group = pick(r);
        l.decimal = pick(r);
        l.time_sep = pick(r);
        l.currency = pick(r);
    }
    l
}

fn timed(what: &str, budget: Duration, f: impl FnOnce()) {
    let t = Instant::now();
    f();
    let e = t.elapsed();
    assert!(e < budget, "{what} took {e:?}");
}

#[test]
fn text_paths_never_panic() {
    let mut r = Rng(0x00a1_0b5e);
    let mut worst = Duration::ZERO;
    for _ in 0..20_000 {
        let loc = locale(&mut r);
        let a = text(&mut r);
        let b = text(&mut r);
        let (sa, sb) = (JsStr::Utf16(&a), JsStr::Utf16(&b));
        let t = Instant::now();
        let _ = parse_number_text(sa, &loc, r.below(2) == 0, 2025.0);
        let _ = parse_date_time_parts(sa, &loc, false, 2024.0);
        let _ = to_bool(ScalarView::Str(sa), &loc);
        let _ = compare_text_js(sa, sb);
        let _ = same_text_ci(sa, sb);
        let _ = compare_scalars(ScalarView::Str(sa), ScalarView::Num(r.f64()));
        let c = parse_criterion(ScalarView::Str(sa), &loc, 2025.0, r.below(2) == 0);
        let _ = c.matches(ScalarView::Str(sb));
        let _ = c.matches(ScalarView::Num(r.f64()));
        let _ = Wildcard::new(&a).matches(sb);
        worst = worst.max(t.elapsed());
    }
    eprintln!("text paths: worst call group {worst:?}");
    assert!(worst < Duration::from_millis(250), "{worst:?}");
}

#[test]
fn catastrophic_patterns_are_bounded() {
    // an empty group separator over a long digit run: exponential backtracking in V8 (the TS hangs)
    let mut l = ValueLocale::en_us();
    l.group = JsString::default();
    let s: String = "1".repeat(4000) + "x";
    timed("empty-group digits", Duration::from_millis(500), || {
        assert_eq!(parse_number_text(JsStr::Latin1(s.as_bytes()), &l, false, 2025.0), None);
    });
    // thousands of groups: the nesting limit, not the stack
    let pt = ValueLocale::pt_br();
    let g: String = "1".to_owned() + &".000".repeat(20_000);
    timed("deep groups", Duration::from_millis(500), || {
        assert_eq!(parse_number_text(JsStr::Latin1(g.as_bytes()), &pt, false, 2025.0), None);
    });
    // a wildcard with many stars against a long text
    let p: Vec<u16> = "*a".repeat(200).encode_utf16().chain("b".encode_utf16()).collect();
    let t: Vec<u16> = "a".repeat(5000).encode_utf16().collect();
    timed("stars", Duration::from_millis(500), || {
        assert!(!Wildcard::new(&p).matches(JsStr::Utf16(&t)));
    });
    // a long date-like text
    let d: String = "1/".repeat(20_000);
    timed("long date", Duration::from_millis(500), || {
        assert_eq!(parse_date_time_parts(JsStr::Latin1(d.as_bytes()), &pt, false, 2025.0), None);
    });
}

#[test]
fn numeric_paths_never_panic() {
    let mut r = Rng(0x5eed_a1a1);
    let mut worst = Duration::ZERO;
    for _ in 0..20_000 {
        let (x, y, z) = (r.f64(), r.f64(), r.f64());
        let t = Instant::now();
        let _ = (libm::cr_log(x), libm::cr_log10(x), libm::cr_log1p(x), libm::cr_exp(x), libm::xl_exp(x), libm::cr_expm1(x));
        let _ = (libm::xl_sin(x), libm::xl_cos(x), libm::xl_tan(x), libm::cr_atan(x), libm::cr_asin(x), libm::cr_acos(x));
        let _ = (libm::cr_sinh(x), libm::cr_cosh(x), libm::cr_pow(x, y), libm::cr_atan2(x, y));
        let _ = (special::pnorm_both(x), special::qnorm(x), special::lgamma(x), special::gamma(x), special::erfc(x));
        let _ = (special::stirlerr(x), special::bd0(x, y), special::log1pmx(x), special::lgamma1p(x));
        let _ = (special::dbinom_raw(x, y, z, 1.0 - z), special::dpois_raw(x, y), special::lbeta(x, y));
        let _ = (special::pgamma(x, y, true), special::pgamma(x, y, false), special::beta_inc(x, y, z));
        let _ = (
            dates::ymd_to_serial(x, y, z, false),
            dates::serial_to_ymd(x, true),
            dates::excel_date(x, y, z, false),
            dates::split_serial(x, false),
        );
        let _ = (round15(x), excel_power(x, y), decimal_round(x, y, RoundMode::Up));
        let xs = [x, y, z];
        let _ = (stats::variance(&xs, true), stats::kurt(&xs), stats::sorted(&xs));
        worst = worst.max(t.elapsed());
    }
    eprintln!("numeric paths: worst call group {worst:?}");
    assert!(worst < Duration::from_secs(2), "{worst:?}");
}

/// M1 for every A1 entry that allocates, and the zero-allocation guarantee of the hot paths (PERF): a number text,
/// a criterion match (number, wildcard, ASCII and Latin-1 text), ASCII and Latin-1 comparisons, `toNumber`.
#[test]
fn memory_m1_and_zero_alloc_hot_paths() {
    use uo_calc::coerce::{number_to_text, to_number};
    use uo_calc::collate::compare_text_ord;
    assert!(uo_alloc::is_counting());
    let pt = ValueLocale::pt_br();
    // warm the lazily built tables (Latin-1 collation elements, RegExp canonicalization): static, never freed
    let _ = compare_text_ord(JsStr::Latin1(b"\xe9a"), JsStr::Latin1(b"e"));
    let _ = Wildcard::new(&[0x61]).matches(JsStr::Latin1(b"\xc9"));
    let zero = |what: &str, f: &dyn Fn()| {
        let u = uo_alloc::assert_no_leak(f);
        assert_eq!(u.allocated, 0, "{what} allocated {} B", u.allocated);
    };
    zero("parseNumberText grouped", &|| assert!(parse_number_text(JsStr::Latin1(b"R$ 1.234,56"), &pt, false, 2025.0).is_some()));
    zero("parseNumberText word", &|| assert!(parse_number_text(JsStr::Latin1(b"abc"), &pt, false, 2025.0).is_none()));
    zero("toNumber", &|| assert!(to_number(ScalarView::Str(JsStr::Latin1(b"-1,5e3")), &pt, false, 2025.0).is_ok()));
    zero("compareText ASCII", &|| assert!(compare_text_js(JsStr::Latin1(b"co-op"), JsStr::Latin1(b"coop")) > 0));
    zero("compareText Latin-1", &|| assert!(compare_text_js(JsStr::Latin1(b"\xe1rvore"), JsStr::Latin1(b"arvore")) > 0));
    let crits: Vec<_> = [&b">=50"[..], b"a*", b"abc", b"\xe1rvore", b"=1"]
        .iter()
        .map(|c| parse_criterion(ScalarView::Str(JsStr::Latin1(c)), &pt, 2025.0, false))
        .collect();
    zero("criteria match", &|| {
        for c in &crits {
            let _ = c.matches(ScalarView::Num(51.0));
            let _ = c.matches(ScalarView::Str(JsStr::Latin1(b"ABC")));
            let _ = c.matches(ScalarView::Str(JsStr::Latin1(b"\xc1rvore")));
        }
    });
    // M1 (allocating paths: outputs dropped, live bytes back)
    for f in [
        &(|| drop(number_to_text(1234.5, &pt))) as &dyn Fn(),
        &|| {
            std::hint::black_box(parse_date_time_parts(JsStr::Latin1(b"1 mar\xe7o 2024 10:30"), &pt, false, 2025.0));
        },
        &|| drop(parse_criterion(ScalarView::Str(JsStr::Latin1(b"<>x*y")), &pt, 2025.0, true)),
        &|| {
            std::hint::black_box(compare_text_js(JsStr::Utf16(&[0x3042, 0x41]), JsStr::Utf16(&[0x30A2])));
        },
        &|| drop(stats::sorted(&[3.0, 1.0, 2.0])),
        &|| drop(uo_calc::collate::collation_key(JsStr::Latin1(b"Stra\xdfe"))),
        &|| {
            std::hint::black_box(uo_calc::math_util::decimal_round(2.675, 2.0, RoundMode::Half));
        },
    ] {
        uo_alloc::assert_no_leak(f);
    }
}
