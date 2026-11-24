//! A1 parity: the `a1` capture group (`js/capture-a1.mjs`: every A1 entry point of the frozen TS with traps and
//! seeded inputs, four locales) and the A1 calls of the `calc-tests` group (the TS test suite's own calls), replayed
//! bit for bit (record `uo-calc.md` "A1").
//!
//! Local run: `node core/crates/uo-calc/js/capture-a1.mjs --big --out core/target/sheet-cap/a1-big.capture.gz`, then
//! `UO_A1_BIG=1 cargo test -p uo-calc --test a1` (also replays `calc-tests-big.calls.gz` when present).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::{J, gunzip, parse};
use uo_calc::cell::{error_from_text, error_text};
use uo_calc::coerce::{
    compare_scalars, number_to_text, parse_date_time_parts, parse_number_text, same_text_ci, to_bool, to_number, to_text,
};
use uo_calc::collate::{collation_key, compare_numbers, compare_text_js, round15};
use uo_calc::criteria::{compare15, has_wildcard, locale_error_code, parse_criterion};
use uo_calc::locale::{DateOrder, FormatLetters, ValueLocale};
use uo_calc::math_util::{RoundMode, approx15, decimal_round, excel_power, exponent_of, fin, fin_flush, snap_add};
use uo_calc::stats::{Collect, DirectText, Take};
use uo_calc::{ErrorCode, JsStr, JsString, ScalarView, dates, libm, special, stats};
use uo_testkit::{Capture, Replay};

const YEAR: f64 = 2025.0;

// ---------------------------------------------------------------- value texts

/// An owned scalar of a value text.
#[derive(Clone, Debug)]
enum V {
    Empty,
    Num(f64),
    Str(JsString),
    Bool(bool),
    Err(ErrorCode),
}

impl V {
    fn view(&self) -> ScalarView<'_> {
        match self {
            V::Empty => ScalarView::Empty,
            V::Num(x) => ScalarView::Num(*x),
            V::Str(s) => ScalarView::Str(s.as_js()),
            V::Bool(b) => ScalarView::Bool(*b),
            V::Err(e) => ScalarView::Err(*e),
        }
    }
    fn num(&self) -> f64 {
        match self {
            V::Num(x) => *x,
            v => panic!("not a number: {v:?}"),
        }
    }
    fn js(&self) -> JsStr<'_> {
        match self {
            V::Str(s) => s.as_js(),
            v => panic!("not a string: {v:?}"),
        }
    }
}

/// Parses one value text at the start of `s`; returns it and the rest.
fn value_at(s: &str) -> (V, &str) {
    let b = s.as_bytes();
    match b[0] {
        b'_' => (V::Empty, &s[1..]),
        b'n' => (V::Num(f64::from_bits(u64::from_str_radix(&s[1..17], 16).unwrap())), &s[17..]),
        b'b' => (V::Bool(b[1] == b'1'), &s[2..]),
        b'e' => {
            let end = s.find([',', ']']).unwrap_or(s.len());
            (V::Err(error_from_text(JsStr::Latin1(&b[1..end])).unwrap()), &s[end..])
        }
        b's' => {
            // a JSON string: up to the first unescaped quote after the opening one
            let mut i = 2;
            while b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            let J::Str(u) = parse(&s[1..=i]) else { panic!("{s}") };
            (V::Str(JsString::from_utf16(&u)), &s[i + 1..])
        }
        _ => panic!("value text {s}"),
    }
}

fn value(s: &str) -> V {
    let (v, rest) = value_at(s);
    assert!(rest.is_empty(), "trailing {rest} in {s}");
    v
}

/// `N[...]` / `V[...]` lists.
fn list(s: &str) -> Vec<V> {
    let mut rest = &s[2..s.len() - 1];
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (v, r) = value_at(rest);
        out.push(v);
        rest = r.strip_prefix(',').unwrap_or(r);
    }
    out
}

fn nums(s: &str) -> Vec<f64> {
    list(s).iter().map(V::num).collect()
}

/// The text with every NaN number token written `nNaN`.
fn nan_blind(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'n' && i + 17 <= b.len() && b[i + 1..i + 17].iter().all(u8::is_ascii_hexdigit) {
            let x = f64::from_bits(u64::from_str_radix(&s[i + 1..i + 17], 16).unwrap());
            if x.is_nan() {
                out.push_str("nNaN");
                i += 17;
                continue;
            }
        }
        out.push(char::from(b[i]));
        i += 1;
    }
    out
}

fn n(x: f64) -> String {
    format!("n{:016x}", x.to_bits())
}

fn s_text(s: JsStr<'_>) -> String {
    let mut o = "s".to_owned();
    uo_jsnum::json::push_string_utf16(&mut o, &s.to_utf16());
    o
}

fn err(e: ErrorCode) -> String {
    format!("e{}", error_text(e))
}

fn num_or_err(r: Result<f64, ErrorCode>) -> String {
    r.map_or_else(err, n)
}

fn opt_num(r: Option<f64>) -> String {
    r.map_or_else(|| "u".to_owned(), n)
}

fn b(x: bool) -> String {
    (if x { "b1" } else { "b0" }).to_owned()
}

fn nlist(xs: &[f64]) -> String {
    format!("N[{}]", xs.iter().map(|&x| n(x)).collect::<Vec<_>>().join(","))
}

// ---------------------------------------------------------------- locales

fn js(j: &J) -> JsString {
    j.js().unwrap()
}

fn arr<const N: usize>(j: &J) -> [JsString; N] {
    let a = j.arr();
    core::array::from_fn(|i| js(&a[i]))
}

/// A `ValueLocale` from its JSON (the TS object).
fn locale_of(j: &J) -> ValueLocale {
    let g = |k: &str| js(j.get(k).unwrap_or_else(|| panic!("locale field {k}")));
    ValueLocale {
        id: String::from_utf16_lossy(&g("id").as_js().to_utf16()).into(),
        decimal: g("decimal"),
        group: g("group"),
        true_text: g("trueText"),
        false_text: g("falseText"),
        date_order: match String::from_utf16_lossy(&g("dateOrder").as_js().to_utf16()).as_str() {
            "MDY" => DateOrder::Mdy,
            "YMD" => DateOrder::Ymd,
            _ => DateOrder::Dmy,
        },
        date_sep: g("dateSep"),
        time_sep: g("timeSep"),
        am: g("am"),
        pm: g("pm"),
        month_names: arr(j.get("monthNames").unwrap()),
        month_abbr: arr(j.get("monthAbbr").unwrap()),
        day_names: arr(j.get("dayNames").unwrap()),
        day_abbr: arr(j.get("dayAbbr").unwrap()),
        currency: g("currency"),
        general_name: j.get("generalName").and_then(J::js),
        format_letters: j.get("formatLetters").map(|f| FormatLetters {
            year: js(f.get("year").unwrap()),
            month: js(f.get("month").unwrap()),
            day: js(f.get("day").unwrap()),
            hour: js(f.get("hour").unwrap()),
            minute: js(f.get("minute").unwrap()),
            second: js(f.get("second").unwrap()),
        }),
        r1c1: None,
    }
}

struct Locales {
    en: ValueLocale,
    pt: ValueLocale,
    de: ValueLocale,
    xx: ValueLocale,
}

impl Locales {
    fn of(cap: &Capture) -> Self {
        Self {
            en: ValueLocale::en_us(),
            pt: ValueLocale::pt_br(),
            de: locale_of(&parse(cap.meta("locale.de").unwrap())),
            xx: locale_of(&parse(cap.meta("locale.xx").unwrap())),
        }
    }
    fn get(&self, t: &str) -> &ValueLocale {
        match t {
            "Len" => &self.en,
            "Lpt" => &self.pt,
            "Lde" => &self.de,
            "Lxx" => &self.xx,
            _ => panic!("locale {t}"),
        }
    }
}

// ---------------------------------------------------------------- the a1 group

fn collect_opts(i: usize) -> Collect {
    let all = Collect {
        ref_text: Take::Count,
        arr_text: Take::Count,
        direct_text: DirectText::Skip,
        skip_errors: true,
        direct_bool_skip: true,
        ..Collect::default()
    };
    match i {
        0 => Collect::default(),
        1 => Collect { ref_text: Take::Count, ref_bool: Take::Count, ..Collect::default() },
        2 => Collect { arr_text: Take::Count, arr_bool: Take::Count, ..Collect::default() },
        3 => Collect { direct_text: DirectText::Skip, ..Collect::default() },
        4 => Collect { direct_text: DirectText::Ignore, ..Collect::default() },
        5 => Collect { direct_bool_skip: true, ..Collect::default() },
        6 => Collect { skip_errors: true, ..Collect::default() },
        7 => all,
        _ => panic!("collect options {i}"),
    }
}

/// Computes the result text of one `a1` record.
fn eval_a1(name: &str, a: &[String], loc: &Locales, crit_values: &[V]) -> String {
    let v = |i: usize| value(&a[i]);
    let x = |i: usize| v(i).num();
    let flag = |i: usize| a[i] == "b1";
    let l = |i: usize| loc.get(&a[i]);
    let pair = |(p, q): (f64, f64)| format!("{};{}", n(p), n(q));
    match name {
        "parseNumberText" => opt_num(parse_number_text(v(0).js(), l(1), flag(2), YEAR)),
        "parseDateTimeParts" => match parse_date_time_parts(v(0).js(), l(1), flag(2), x(3)) {
            None => "u".to_owned(),
            Some(p) => format!("d{};t{}", p.date.map_or_else(|| "-".to_owned(), n), p.time.map_or_else(|| "-".to_owned(), n)),
        },
        "numberToText" => s_text(number_to_text(x(0), l(1)).as_js()),
        "toBool" => to_bool(v(0).view(), l(1)).map_or_else(err, b),
        "toNumber" => num_or_err(to_number(v(0).view(), l(1), false, YEAR)),
        "toText" => {
            let s = v(0);
            to_text(s.view(), l(1)).map_or_else(err, |t| s_text(t.as_js()))
        }
        "compareText" => n(f64::from(compare_text_js(v(0).js(), v(1).js()))),
        "sameTextCI" => b(same_text_ci(v(0).js(), v(1).js())),
        "collationKey" => s_text(JsString::from_str_js(&collation_key(v(0).js())).as_js()),
        "compareScalars" => n(f64::from(compare_scalars(v(0).view(), v(1).view()))),
        "compareNumbers" => n(f64::from(compare_numbers(x(0), x(1)))),
        "compare15" => n(f64::from(compare15(x(0), x(1)))),
        "round15" => n(round15(x(0))),
        "parseCriterion" | "parseCriterionPrefix" => {
            let c = v(0);
            let p = parse_criterion(c.view(), l(1), YEAR, name == "parseCriterionPrefix");
            let bits: String = crit_values.iter().map(|cv| if p.matches(cv.view()) { '1' } else { '0' }).collect();
            format!("{}:{bits}", u8::from(p.blank))
        }
        "localeErrorCode" => {
            let t = v(0);
            locale_error_code(&t.js().to_utf16(), l(1)).map_or_else(|| "u".to_owned(), |e| s_text(JsStr::Latin1(error_text(e).as_bytes())))
        }
        "hasWildcard" => b(has_wildcard(&v(0).js().to_utf16())),
        "ymdToSerial" => n(dates::ymd_to_serial(x(0), x(1), x(2), flag(3))),
        "excelDate" => opt_num(dates::excel_date(x(0), x(1), x(2), flag(3))),
        "daysInMonth" => n(dates::days_in_month(x(0), x(1), flag(2))),
        "isLeap" => b(dates::is_leap(x(0))),
        "serialToYmd" => {
            let r = dates::serial_to_ymd(x(0), flag(1));
            format!("y{};m{};d{}", n(r.y), n(r.m), n(r.d))
        }
        "weekday" => n(dates::weekday(x(0), flag(1))),
        "serialToHms" => {
            let r = dates::serial_to_hms(x(0));
            format!("h{};m{};s{}", n(r.h), n(r.mi), n(r.s))
        }
        "splitSerial" => dates::split_serial(x(0), flag(1)).map_or_else(|| "u".to_owned(), pair),
        "xlAdd" => n(stats::xl_add(x(0), x(1))),
        "snapAdd" => n(snap_add(x(0), x(1))),
        "statNum" => num_or_err(stats::stat_num(x(0))),
        "xlSum" => n(stats::xl_sum(&nums(&a[0]))),
        "mean" => n(stats::mean(&nums(&a[0]))),
        "devsq" => n(stats::devsq(&nums(&a[0]))),
        "varianceS" => n(stats::variance(&nums(&a[0]), true)),
        "varianceP" => n(stats::variance(&nums(&a[0]), false)),
        "skew" => num_or_err(stats::skew(&nums(&a[0]))),
        "skewP" => num_or_err(stats::skew_p(&nums(&a[0]))),
        "kurt" => num_or_err(stats::kurt(&nums(&a[0]))),
        "sorted" => nlist(&stats::sorted(&nums(&a[0]))),
        "collectNumbers" => {
            let o = collect_opts(a[0][1..].parse().unwrap());
            let lc = l(1);
            let mut out = Vec::new();
            let mut res = Ok(());
            for arg in &a[2..] {
                let (kind, body) = arg.split_at(1);
                res = match kind {
                    "d" => o.direct(&mut out, value(body).view(), lc, YEAR),
                    "a" => list(body).iter().try_for_each(|v| o.array_elem(&mut out, v.view())),
                    "r" => list(body).iter().try_for_each(|v| o.ref_cell(&mut out, v.view())),
                    _ => panic!("collect arg {arg}"),
                };
                if res.is_err() {
                    break;
                }
            }
            res.map_or_else(err, |()| nlist(&out))
        }
        "argNumber" => num_or_err(stats::arg_number(v(0).view(), l(1), YEAR)),
        "argInt" => num_or_err(stats::arg_int(v(0).view(), l(1), YEAR)),
        "argBool" => stats::arg_bool(v(0).view(), l(1), YEAR).map_or_else(err, b),
        "atpNum" => num_or_err(uo_calc::math_util::atp_num(v(0).view(), l(1), YEAR)),
        "pnormBoth" => pair(special::pnorm_both(x(0))),
        "dnorm" => n(special::dnorm(x(0))),
        "qnorm" => n(special::qnorm(x(0))),
        "lgamma" => n(special::lgamma(x(0))),
        "lgammacor" => n(special::lgammacor(x(0))),
        "gamma" => n(special::gamma(x(0))),
        "erf" => n(special::erf(x(0))),
        "erfc" => n(special::erfc(x(0))),
        "normCdfXl" => n(special::norm_cdf_xl(x(0))),
        "stirlerr" => n(special::stirlerr(x(0))),
        "bd0" => n(special::bd0(x(0), x(1))),
        "log1pmx" => n(special::log1pmx(x(0))),
        "lgamma1p" => n(special::lgamma1p(x(0))),
        "dbinomRaw" => n(special::dbinom_raw(x(0), x(1), x(2), x(3))),
        "dpoisRaw" => n(special::dpois_raw(x(0), x(1))),
        "pgamma" => n(special::pgamma(x(0), x(1), flag(2))),
        "gammaInc" => pair(special::gamma_inc(x(0), x(1))),
        "lbeta" => n(special::lbeta(x(0), x(1))),
        "betaInc" => pair(special::beta_inc(x(0), x(1), x(2))),
        "crLog" => n(libm::cr_log(x(0))),
        "crLog10" => n(libm::cr_log10(x(0))),
        "crLog1p" => n(libm::cr_log1p(x(0))),
        "crExp" => n(libm::cr_exp(x(0))),
        "xlExp" => n(libm::xl_exp(x(0))),
        "xlSin" => n(libm::xl_sin(x(0))),
        "xlCos" => n(libm::xl_cos(x(0))),
        "xlTan" => n(libm::xl_tan(x(0))),
        "crAtan" => n(libm::cr_atan(x(0))),
        "crAsin" => n(libm::cr_asin(x(0))),
        "crAcos" => n(libm::cr_acos(x(0))),
        "crSinh" => n(libm::cr_sinh(x(0))),
        "crCosh" => n(libm::cr_cosh(x(0))),
        "crExpm1" => n(libm::cr_expm1(x(0))),
        "crPow" => n(libm::cr_pow(x(0), x(1))),
        "crAtan2" => n(libm::cr_atan2(x(0), x(1))),
        "approx15" => n(approx15(x(0))),
        "exponentOf" => n(exponent_of(x(0))),
        "fin" => num_or_err(fin(x(0))),
        "finFlush" => num_or_err(fin_flush(x(0))),
        "decimalRound" => {
            let mode = match x(2) as u8 {
                0 => RoundMode::Half,
                1 => RoundMode::Up,
                _ => RoundMode::Down,
            };
            n(decimal_round(x(0), x(1), mode))
        }
        "excelPower" => num_or_err(excel_power(x(0), x(1))),
        _ => panic!("unknown A1 function {name}"),
    }
}

fn replay_a1(path: &Path) -> (usize, uo_testkit::Report) {
    let cap = Capture::parse(&gunzip(path)).unwrap();
    let loc = Locales::of(&cap);
    let crit_values: Vec<V> = parse(cap.meta("crit-values").unwrap())
        .arr()
        .iter()
        .map(|j| value(&String::from_utf16_lossy(&j.js().unwrap().as_js().to_utf16())))
        .collect();
    let mut rep = Replay::new(&cap);
    let mut calls = 0;
    for rec in cap.records() {
        let key = rec.key.as_str();
        let name = key.split('#').next().unwrap();
        let args: Vec<String> = parse(rec.get("in").unwrap())
            .arr()
            .iter()
            .map(|j| {
                let J::Str(u) = j else { panic!("arg {j:?}") };
                String::from_utf16(u).unwrap()
            })
            .collect();
        rep.skip(key, "in");
        let got = eval_a1(name, &args, &loc, &crit_values);
        let want = rec.get("out").unwrap_or("");
        if got != want && nan_blind(&got) == nan_blind(want) {
            rep.accept(
                key,
                "out",
                &got,
                "NaN payload (V8's NaN bits depend on the tier that ran the code; every NaN is the same JS value)",
            );
        } else {
            rep.check(key, "out", &got);
        }
        calls += 1;
    }
    (calls, rep.finish())
}

#[test]
fn a1_ci() {
    let (calls, rep) = replay_a1(&support::captures_dir().join("a1.capture.gz"));
    eprintln!("a1 (CI): {calls} calls; {}", rep.summary());
    assert!(calls > 50_000);
    if !rep.is_clean() {
        eprintln!("{}", rep.describe(40));
    }
    rep.assert_clean();
}

#[test]
fn a1_big() {
    if std::env::var_os("UO_A1_BIG").is_none() {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/a1-big.capture.gz");
    let (calls, rep) = replay_a1(&path);
    eprintln!("a1 (big): {calls} calls; {}", rep.summary());
    if !rep.is_clean() {
        eprintln!("{}", rep.describe(40));
    }
    rep.assert_clean();
}

// ---------------------------------------------------------------- the calc-tests calls of A1

/// `JSON.stringify(canon(x))` of a number.
fn canon_num(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        return r#"{"$num":"-0"}"#.to_owned();
    }
    if !x.is_finite() {
        return format!(r#"{{"$num":"{}"}}"#, uo_jsnum::format::to_js_string(x));
    }
    uo_jsnum::json::number(x)
}

fn canon_str(s: JsStr<'_>) -> String {
    let mut o = String::new();
    uo_jsnum::json::push_string_utf16(&mut o, &s.to_utf16());
    o
}

fn canon_err(e: ErrorCode) -> String {
    format!(r#"{{"$err":"{}"}}"#, error_text(e))
}

/// A scalar argument of the recorder's canonical JSON.
fn arg_scalar(j: &J) -> V {
    match j {
        J::Null => V::Empty,
        J::Bool(b) => V::Bool(*b),
        J::Num(x) => V::Num(*x),
        J::Str(u) => V::Str(JsString::from_utf16(u)),
        J::Obj(_) => {
            if let Some(e) = j.get("$err").and_then(J::js) {
                return V::Err(error_from_text(e.as_js()).unwrap());
            }
            match j.get("$num").and_then(J::js).map(|s| String::from_utf16_lossy(&s.as_js().to_utf16())) {
                Some(s) if s == "-0" => V::Num(-0.0),
                Some(s) => V::Num(uo_jsnum::parse::to_number(&s)),
                None => panic!("scalar {j:?}"),
            }
        }
        J::Arr(_) => panic!("scalar {j:?}"),
    }
}

/// The canonical result of one calc-tests call of an A1 entry point, or `None` when the call is not replayable
/// (a closure or a `RegExp` result).
fn eval_calc_test(f: &str, args: &[J], en: &ValueLocale) -> Option<String> {
    let locs: Vec<ValueLocale> = args.iter().skip(1).filter(|j| j.get("decimal").is_some()).map(locale_of).collect();
    let loc = locs.first().unwrap_or(en);
    let num = |i: usize| arg_scalar(&args[i]).num();
    let opt_bool = |i: usize| args.get(i).is_some_and(J::bool);
    Some(match f {
        "calc/collation:compareText" => canon_num(f64::from(compare_text_js(arg_scalar(&args[0]).js(), arg_scalar(&args[1]).js()))),
        "calc/coerce:compareScalars" => canon_num(f64::from(compare_scalars(arg_scalar(&args[0]).view(), arg_scalar(&args[1]).view()))),
        "calc/coerce:parseDateTimeParts" => {
            let year = args.get(3).map_or(YEAR, |j| arg_scalar(j).num());
            match parse_date_time_parts(arg_scalar(&args[0]).js(), loc, opt_bool(2), year) {
                None => r#"{"$undef":true}"#.to_owned(),
                Some(p) => {
                    let mut parts = Vec::new();
                    if let Some(d) = p.date {
                        parts.push(format!(r#""date":{}"#, canon_num(d)));
                    }
                    if let Some(t) = p.time {
                        parts.push(format!(r#""time":{}"#, canon_num(t)));
                    }
                    format!("{{{}}}", parts.join(","))
                }
            }
        }
        "calc/coerce:parseNumberText" => {
            parse_number_text(arg_scalar(&args[0]).js(), loc, opt_bool(2), YEAR).map_or_else(|| r#"{"$undef":true}"#.to_owned(), canon_num)
        }
        "calc/coerce:numberToText" => canon_str(number_to_text(num(0), loc).as_js()),
        "calc/coerce:toBool" => to_bool(arg_scalar(&args[0]).view(), loc).map_or_else(canon_err, |b| b.to_string()),
        "calc/collate:compareNumbers" => canon_num(f64::from(compare_numbers(num(0), num(1)))),
        "calc/criteria:compare15" => canon_num(f64::from(compare15(num(0), num(1)))),
        "calc/dates:ymdToSerial" => canon_num(dates::ymd_to_serial(num(0), num(1), num(2), opt_bool(3))),
        "calc/dates:serialToYmd" => {
            let r = dates::serial_to_ymd(num(0), opt_bool(1));
            format!(r#"{{"d":{},"m":{},"y":{}}}"#, canon_num(r.d), canon_num(r.m), canon_num(r.y))
        }
        "calc/dates:weekday" => canon_num(dates::weekday(num(0), opt_bool(1))),
        "calc/dates:serialToHms" => {
            let r = dates::serial_to_hms(num(0));
            format!(r#"{{"h":{},"mi":{},"s":{}}}"#, canon_num(r.h), canon_num(r.mi), canon_num(r.s))
        }
        "calc/functions/math-util:snapAdd" => canon_num(snap_add(num(0), num(1))),
        "calc/functions/math-util:excelPower" => excel_power(num(0), num(1)).map_or_else(canon_err, canon_num),
        _ => return None,
    })
}

const A1_MODULES: [&str; 10] = [
    "calc/coerce:",
    "calc/collate:",
    "calc/collation:",
    "calc/criteria:",
    "calc/dates:",
    "calc/stats:",
    "calc/special-functions:",
    "calc/values:",
    "calc/functions/libm:",
    "calc/functions/math-util:",
];

fn replay_calc_tests(path: &Path) -> (usize, usize, Vec<String>) {
    let en = ValueLocale::en_us();
    let (mut equal, mut skipped) = (0, 0);
    let mut bad = Vec::new();
    for line in gunzip(path).lines() {
        let j = parse(line);
        let f = String::from_utf16_lossy(&j.get("fn").and_then(J::js).unwrap().as_js().to_utf16());
        if !A1_MODULES.iter().any(|m| f.starts_with(m)) {
            continue;
        }
        let args = j.get("args").unwrap().arr();
        let Some(got) = eval_calc_test(&f, args, &en) else {
            skipped += 1;
            continue;
        };
        let want = {
            // the recorder's ret, re-serialized canonically (keys sorted, as recorded)
            let start = line.find(r#""ret":"#).unwrap() + 6;
            line[start..line.len() - 1].to_owned()
        };
        if got == want {
            equal += 1;
        } else {
            bad.push(format!("{f} {:?}: want {want} got {got}", line.get(..200)));
        }
    }
    (equal, skipped, bad)
}

#[test]
fn a1_calc_tests() {
    let (equal, skipped, bad) = replay_calc_tests(&support::captures_dir().join("calc-tests.calls.gz"));
    eprintln!("calc-tests A1 calls: {equal} equal, {skipped} not replayable (closure/RegExp results), {} mismatches", bad.len());
    for b in bad.iter().take(20) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
    assert!(equal >= 540, "{equal}");
    let big: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/calc-tests-big.calls.gz");
    if std::env::var_os("UO_A1_BIG").is_some() && big.exists() {
        let (equal, skipped, bad) = replay_calc_tests(&big);
        eprintln!("calc-tests (big) A1 calls: {equal} equal, {skipped} not replayable, {} mismatches", bad.len());
        for b in bad.iter().take(20) {
            eprintln!("  {b}");
        }
        assert!(bad.is_empty());
    }
}
