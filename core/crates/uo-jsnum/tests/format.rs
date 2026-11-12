//! Parity of the formatters (`String(x)`, `toFixed`, `toPrecision`, `toExponential`, `toString(radix)`
//! of integers) and of `parseFloat` against `tests/captures/format.capture`, written by
//! `js/capture-jsnum-b.mjs` in Node 24.15.0 (V8 13.6.233.17, x64):
//!
//! - `fmt/*`: digests over inputs drawn with the script's PRNG (bits, ranges, `k / 10^d`, integers,
//!   ties `k + 0.5` and dyadic `k / 2^j`) and digits arguments (all 0..=100 or 1..=100), 10^5 in
//!   `cargo test`, 10^6 (10^7 for `String(x)`) with `--include-ignored`;
//! - `edge/*`: the outputs in full for the written-out values and every digits argument 0..=105,
//!   1000 and 4294967295 (RangeError included);
//! - `sweep/*`: every power of two (both signs) and of ten (and the neighbours) times every digits
//!   argument;
//! - `pf/*`, `rand/pf_*`: `parseFloat` over the corpus of the other captures, written-out strings and
//!   random strings.
//!
//! On a digest mismatch, `UO_JSNUM_DUMP=fmt/<key>` in the script run writes the samples to
//! `core/target/jsnum-dump/` and this test prints the first 20 differing ones.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::format::{self, CAPTURE, FULL_STRINGS, SMALL, SMALL_STRINGS, STRING_KEYS};
use support::{fnv, hex};
use uo_jsnum::format::{Js, RangeError, to_exponential, to_fixed, to_js_string, to_precision, to_radix_string};
use uo_jsnum::parse::parse_float;
use uo_testkit::{Capture, PlatformSpec, Replay};

const JSNUM_CAPTURE: &str = include_str!("captures/jsnum.capture");
const ADV_CAPTURE: &str = include_str!("captures/adv.capture");

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    assert!(cap.meta("tiers-agree").unwrap().contains("--no-opt --no-maglev"));
    cap
}

fn parallel<T: Send, R: Sync>(items: &[R], job: impl Fn(&R) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, usize::from).min(16);
    let mut out: Vec<Option<T>> = (0..items.len()).map(|_| None).collect();
    let chunks: Vec<Vec<(usize, &R)>> =
        (0..threads).map(|t| items.iter().enumerate().filter(|(i, _)| i % threads == t).collect()).collect();
    let job = &job;
    let results: Vec<Vec<(usize, T)>> = std::thread::scope(|sc| {
        let handles: Vec<_> =
            chunks.into_iter().map(|chunk| sc.spawn(move || chunk.into_iter().map(|(i, r)| (i, job(r))).collect::<Vec<_>>())).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, v) in results.into_iter().flatten() {
        out[i] = Some(v);
    }
    out.into_iter().map(Option::unwrap).collect()
}

fn dump_report(rec: &format::Rec) {
    let Ok(wanted) = std::env::var("UO_JSNUM_DUMP") else { return };
    if wanted != format::capture_key(rec) {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/jsnum-dump")
        .join(format!("fmt_{}.tsv", rec.key.replace('/', "_")));
    let Ok(text) = std::fs::read_to_string(&path) else { return eprintln!("no dump at {}", path.display()) };
    let lines: Vec<&str> = text.lines().collect();
    let mine = format::samples(rec, lines.len());
    let mut shown = 0;
    for ((x, n, got), line) in mine.iter().zip(&lines) {
        let mut f = line.splitn(3, '\t');
        let (_, _, want) = (f.next(), f.next(), f.next().unwrap_or(""));
        if got != want {
            eprintln!("{} x={} n={n}: rust {got:?} node {want:?}", rec.key, hex(*x));
            shown += 1;
            if shown == 20 {
                break;
            }
        }
    }
}

fn replay_digests(full: bool) {
    let cap = capture().subset(|k| k.starts_with("fmt/"));
    let recs = format::records();
    assert_eq!(recs.len(), cap.len());
    let results = parallel(&recs, |rec| {
        let n = if full { cap.get(&format::capture_key(rec), "n_full").unwrap().parse::<usize>().unwrap() } else { SMALL };
        format::digest(rec, n)
    });
    let mut replay = Replay::new(&cap);
    for (rec, (small, fullh)) in recs.iter().zip(&results) {
        let key = format::capture_key(rec);
        replay.check(&key, "seed", &format!("{:x}", fnv(&key)));
        replay.skip(&key, "n_small");
        replay.skip(&key, "n_full");
        let ok = if full {
            replay.check(&key, "small", small.as_deref().unwrap_or(""));
            replay.check(&key, "full", fullh)
        } else {
            replay.skip(&key, "full");
            replay.check(&key, "small", fullh)
        };
        if !ok {
            dump_report(rec);
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn digests_at_10_5_samples_match_node() {
    replay_digests(false);
}

#[test]
#[ignore = "10^6 samples per distribution (10^7 for String(x)): cargo test -p uo-jsnum --release -- --include-ignored"]
fn digests_at_10_6_samples_match_node() {
    replay_digests(true);
}

#[test]
fn every_record_of_the_table_is_in_the_capture_and_back() {
    let cap = capture();
    let keys: Vec<String> = format::records().iter().map(format::capture_key).collect();
    for k in &keys {
        assert!(cap.record(k).is_some(), "{k} is not in the capture");
    }
    assert_eq!(cap.records().iter().filter(|r| r.key.starts_with("fmt/")).count(), keys.len());
}

fn x_of(rec: &uo_testkit::capture::Record) -> f64 {
    f64::from_bits(u64::from_str_radix(rec.get("x").unwrap(), 16).unwrap())
}

#[test]
fn edge_values_match_node_for_every_digits_argument() {
    let cap = capture().subset(|k| k.starts_with("edge/"));
    assert!(cap.len() > 300);
    let mut replay = Replay::new(&cap);
    let mut compared = 0;
    for rec in cap.records() {
        let key = rec.key.as_str();
        let op = key.split('/').nth(1).unwrap();
        let x = x_of(rec);
        for (facet, _) in &rec.facets {
            if facet == "x" {
                replay.skip(key, facet);
            } else if op == "js" {
                replay.check(key, facet, &to_js_string(x));
                assert_eq!(format!("{}", Js(x)), rec.get(facet).unwrap(), "Display of {}", hex(x));
            } else {
                let n: u32 = facet.strip_prefix('n').unwrap().parse().unwrap();
                replay.check(key, facet, &format::run(op, x, n));
                compared += 1;
            }
        }
    }
    let report = replay.finish();
    eprintln!("{} ({compared} formatter outputs)", report.summary());
    report.assert_clean();
}

#[test]
fn sweeps_of_powers_of_two_and_ten_match_node() {
    let cap = capture().subset(|k| k.starts_with("sweep/"));
    assert_eq!(cap.len(), 6);
    let keys: Vec<(String, &str, &str)> = ["fixed", "precision", "exponential"]
        .into_iter()
        .flat_map(|op| ["pow2", "pow10"].into_iter().map(move |set| (format!("sweep/{op}/{set}"), op, set)))
        .collect();
    let results = parallel(&keys, |(_, op, set)| format::sweep(op, set));
    let mut replay = Replay::new(&cap);
    for ((key, _, _), (count, digest)) in keys.iter().zip(&results) {
        replay.check(key, "count", &count.to_string());
        replay.check(key, "digest", digest);
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

// ── parseFloat ──────────────────────────────────────────────────────────────────────────────────
fn json_unescape(s: &str) -> String {
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap();
    let mut out = String::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next().unwrap() {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'u' => {
                let code: String = it.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&code, 16).unwrap()).unwrap());
            }
            other => panic!("escape \\{other}"),
        }
    }
    out
}

#[test]
fn parse_float_matches_node_on_the_corpora() {
    let cap = capture().subset(|k| k.starts_with("pf/"));
    let jsnum = Capture::parse(JSNUM_CAPTURE).unwrap();
    let adv = Capture::parse(ADV_CAPTURE).unwrap();
    let mut replay = Replay::new(&cap);
    let (mut own, mut corpus, mut hard) = (0, 0, 0);
    for rec in cap.records() {
        let key = rec.key.as_str();
        let s = if let Some(rest) = key.strip_prefix("pf/own/") {
            own += 1;
            let _ = rest;
            replay.skip(key, "s");
            json_unescape(rec.get("s").unwrap())
        } else if let Some(source) = key.strip_prefix("pf/") {
            let origin = if source.starts_with("corpus/") {
                corpus += 1;
                &jsnum
            } else {
                hard += 1;
                &adv
            };
            json_unescape(origin.get(source, "s").unwrap_or_else(|| panic!("{source} is not in its capture")))
        } else {
            unreachable!()
        };
        replay.check(key, "pf", &hex(parse_float(&s)));
    }
    assert!(own > 100 && corpus > 150 && hard > 900, "{own} {corpus} {hard}");
    let report = replay.finish();
    eprintln!("{} ({own} written-out, {corpus} corpus, {hard} midpoint strings)", report.summary());
    report.assert_clean();
}

fn replay_random_strings(full: bool) {
    let cap = capture().subset(|k| k.starts_with("rand/"));
    assert_eq!(cap.len(), STRING_KEYS.len());
    let results = parallel(&STRING_KEYS, |key| format::string_digest(key, if full { FULL_STRINGS } else { SMALL_STRINGS }));
    let mut replay = Replay::new(&cap);
    for (key, (small, fullh)) in STRING_KEYS.iter().zip(&results) {
        replay.check(key, "seed", &format!("{:x}", fnv(key)));
        replay.skip(key, "n_small");
        replay.skip(key, "n_full");
        if full {
            replay.check(key, "small", small.as_deref().unwrap_or(""));
            replay.check(key, "full", fullh);
        } else {
            replay.skip(key, "full");
            replay.check(key, "small", fullh);
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn random_strings_at_10_5_match_node() {
    replay_random_strings(false);
}

#[test]
#[ignore = "10^6 strings per record: cargo test -p uo-jsnum --release -- --include-ignored"]
fn random_strings_at_10_6_match_node() {
    replay_random_strings(true);
}

// ── by hand ─────────────────────────────────────────────────────────────────────────────────────
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn parse_float_by_hand() {
    let nan = f64::NAN;
    let cases: &[(&str, f64)] = &[
        ("", nan),
        ("  ", nan),
        ("2.71abc", 2.71),
        ("  \t\n\u{a0}\u{feff}-1.5e3x", -1500.0),
        ("1e", 1.0),
        ("1e+", 1.0),
        ("1e-", 1.0),
        ("1.e3", 1000.0),
        (".5", 0.5),
        ("5.", 5.0),
        (".", nan),
        (".e3", nan),
        ("+.5e-3", 0.0005),
        ("-.5", -0.5),
        ("0x10", 0.0),
        ("0b11", 0.0),
        ("1_0", 1.0),
        ("1 5", 1.0),
        ("Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
        ("+Infinity", f64::INFINITY),
        ("Infinityx", f64::INFINITY),
        ("Infinit", nan),
        ("infinity", nan),
        ("+-1", nan),
        ("- 1", nan),
        ("\u{85}1", nan),
        ("\u{180e}1", nan),
        ("1e1000", f64::INFINITY),
        ("-1e1000", f64::NEG_INFINITY),
        ("1e-400", 0.0),
        ("12e3e4", 12000.0),
        ("9007199254740993", 9_007_199_254_740_992.0),
        ("1.7976931348623159e308", f64::INFINITY),
    ];
    for (s, want) in cases {
        assert!(same(parse_float(s), *want), "parseFloat({s:?}) = {} (want {want})", parse_float(s));
    }
    assert!(parse_float("-0").is_sign_negative() && parse_float("-0") == 0.0);
    assert!(parse_float("-1e-400").is_sign_negative() && parse_float("-1e-400") == 0.0);
    assert!(parse_float("-0abc").is_sign_negative());
    assert_eq!(parse_float(&format!("1{}", "0".repeat(400))), f64::INFINITY);
    assert_eq!(parse_float(&format!("9007199254740993.{}1", "0".repeat(800))), 9_007_199_254_740_994.0);
    let long = format!("{}x", "1".repeat(10_000_000));
    assert_eq!(parse_float(&long), f64::INFINITY);
}

const FORMAT_CASES: &[(f64, &str, u32, &str)] = &[
    (-0.0, "toFixed", 2, "0.00"),
    (-1e-10, "toFixed", 2, "-0.00"),
    (0.5, "toFixed", 0, "1"),
    (-0.5, "toFixed", 0, "-1"),
    (1.005, "toFixed", 2, "1.00"),
    (2.5, "toFixed", 0, "3"),
    (1.45, "toFixed", 1, "1.4"),
    (8.345, "toFixed", 2, "8.35"),
    (1e21, "toFixed", 2, "1e+21"),
    (999999999999999900000.0, "toFixed", 2, "999999999999999868928.00"),
    (123.456, "toFixed", 0, "123"),
    (0.0, "toFixed", 0, "0"),
    (0.0, "toFixed", 100, "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"),
    (5e-324, "toFixed", 100, "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"),
    (1e-7, "toFixed", 2, "0.00"),
    (0.000001, "toFixed", 7, "0.0000010"),
    (-1.5, "toFixed", 0, "-2"),
    (99.99, "toFixed", 1, "100.0"),
    (9.995, "toFixed", 2, "9.99"),
    (100000000000000000000.0, "toFixed", 0, "100000000000000000000"),
    (f64::NAN, "toFixed", 2, "NaN"),
    (f64::INFINITY, "toFixed", 2, "Infinity"),
    (f64::NEG_INFINITY, "toFixed", 0, "-Infinity"),
    (f64::NAN, "toFixed", 101, "RangeError"),
    (1.0, "toFixed", 101, "RangeError"),
    (0.1, "toFixed", 20, "0.10000000000000000555"),
    (0.3333333333333333, "toFixed", 25, "0.3333333333333333148296163"),
    (-1e21, "toFixed", 0, "-1e+21"),
    (123456789012345680000.0, "toFixed", 3, "123456789012345683968.000"),
    (1.1805916207174113e21, "toFixed", 1, "1.1805916207174113e+21"),
    (5e-324, "toFixed", 5, "0.00000"),
    (1e-100, "toFixed", 100, "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001"),
    (5e-7, "toFixed", 6, "0.000000"),
    (4.9999e-7, "toFixed", 6, "0.000000"),
    (4.35, "toFixed", 1, "4.3"),
    (10.235, "toFixed", 2, "10.23"),
    (1.255, "toFixed", 2, "1.25"),
    (-0.00001, "toFixed", 2, "-0.00"),
    (-0.0, "toPrecision", 3, "0.00"),
    (0.5, "toPrecision", 1, "0.5"),
    (1234.5678, "toPrecision", 2, "1.2e+3"),
    (1234.5678, "toPrecision", 4, "1235"),
    (1234.5678, "toPrecision", 10, "1234.567800"),
    (0.000001234, "toPrecision", 2, "0.0000012"),
    (1e-7, "toPrecision", 3, "1.00e-7"),
    (123456.0, "toPrecision", 3, "1.23e+5"),
    (1e21, "toPrecision", 5, "1.0000e+21"),
    (25.0, "toPrecision", 1, "3e+1"),
    (35.0, "toPrecision", 1, "4e+1"),
    (0.00001, "toPrecision", 1, "0.00001"),
    (9.5, "toPrecision", 1, "1e+1"),
    (99.5, "toPrecision", 2, "1.0e+2"),
    (-1.5, "toPrecision", 1, "-2"),
    (f64::NAN, "toPrecision", 0, "NaN"),
    (f64::INFINITY, "toPrecision", 200, "Infinity"),
    (1.0, "toPrecision", 0, "RangeError"),
    (1.0, "toPrecision", 101, "RangeError"),
    (123.456, "toPrecision", 100, "123.4560000000000030695446184836328029632568359375000000000000000000000000000000000000000000000000000"),
    (5e-324, "toPrecision", 3, "4.94e-324"),
    (1.7976931348623157e308, "toPrecision", 5, "1.7977e+308"),
    (0.000001, "toPrecision", 2, "0.0000010"),
    (1.5e-7, "toPrecision", 2, "1.5e-7"),
    (1.1805916207174113e21, "toPrecision", 25, "1180591620717411303424.000"),
    (0.3333333333333333, "toPrecision", 50, "0.33333333333333331482961625624739099293947219848633"),
    (0.1, "toPrecision", 30, "0.100000000000000005551115123126"),
    (123456789012345680000.0, "toPrecision", 17, "1.2345678901234568e+20"),
    (-1e21, "toPrecision", 3, "-1.00e+21"),
    (25.0, "toExponential", 0, "3e+1"),
    (-0.0, "toExponential", 0, "0e+0"),
    (0.0, "toExponential", 2, "0.00e+0"),
    (123456.0, "toExponential", 2, "1.23e+5"),
    (0.00015, "toExponential", 1, "1.5e-4"),
    (1.5, "toExponential", 0, "2e+0"),
    (2.5, "toExponential", 0, "3e+0"),
    (-1e-7, "toExponential", 3, "-1.000e-7"),
    (5e-324, "toExponential", 14, "4.94065645841247e-324"),
    (
        1.7976931348623157e308,
        "toExponential",
        100,
        "1.7976931348623157081452742373170435679807056752584499659891747680315726078002853876058955863276687817e+308",
    ),
    (f64::NAN, "toExponential", 200, "NaN"),
    (1.0, "toExponential", 101, "RangeError"),
    (
        1.0,
        "toExponential",
        100,
        "1.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e+0",
    ),
    (9.5, "toExponential", 0, "1e+1"),
    (99.5, "toExponential", 1, "1.0e+2"),
    (1e21, "toExponential", 14, "1.00000000000000e+21"),
    (0.3333333333333333, "toExponential", 14, "3.33333333333333e-1"),
    (f64::NEG_INFINITY, "toExponential", 0, "-Infinity"),
    (0.1, "toExponential", 30, "1.000000000000000055511151231258e-1"),
    (1.1805916207174113e21, "toExponential", 40, "1.1805916207174113034240000000000000000000e+21"),
    (
        123456789.0,
        "toExponential",
        100,
        "1.2345678900000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e+8",
    ),
];

#[test]
fn formatters_by_hand() {
    for &(x, method, n, want) in FORMAT_CASES {
        let op = match method {
            "toFixed" => "fixed",
            "toPrecision" => "precision",
            _ => "exponential",
        };
        assert_eq!(format::run(op, x, n), want, "({x:?}).{method}({n})");
    }
    assert_eq!(to_fixed(f64::NAN, 101), Err(RangeError));
    assert_eq!(to_fixed(1.0, u32::MAX), Err(RangeError));
    assert_eq!(to_precision(1.0, 0), Err(RangeError));
    assert_eq!(to_precision(f64::NAN, 0).as_deref(), Ok("NaN"));
    assert_eq!(to_exponential(1.0, 101), Err(RangeError));
    assert_eq!(to_exponential(f64::INFINITY, 101).as_deref(), Ok("Infinity"));
    assert_eq!(to_fixed(-0.0, 2).as_deref(), Ok("0.00"));
    assert_eq!(to_fixed(-1e-10, 2).as_deref(), Ok("-0.00"));
    assert_eq!(to_precision(-0.0, 3).as_deref(), Ok("0.00"));
    assert_eq!(to_exponential(25.0, 0).as_deref(), Ok("3e+1"));
    assert_eq!(to_fixed(0.5, 0).as_deref(), Ok("1"));
    assert_eq!(to_fixed(-0.5, 0).as_deref(), Ok("-1"));
}

#[test]
fn number_to_string_by_hand() {
    let cases: &[(f64, &str)] = &[
        (-0.0, "0"),
        (0.0, "0"),
        (1e21, "1e+21"),
        (1e-7, "1e-7"),
        (123_456_789_012_345_680_000.0, "123456789012345680000"),
        (1.5e-7, "1.5e-7"),
        (0.000001, "0.000001"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (-1e-7, "-1e-7"),
        (4_294_967_296.0, "4294967296"),
        (9_007_199_254_740_992.0, "9007199254740992"),
        (100.0, "100"),
        (-0.5, "-0.5"),
        (1e300, "1e+300"),
        (0.1 + 0.2, "0.30000000000000004"),
        (f64::NAN, "NaN"),
        (f64::NEG_INFINITY, "-Infinity"),
        (f64::INFINITY, "Infinity"),
    ];
    for &(x, want) in cases {
        assert_eq!(to_js_string(x), want);
        assert_eq!(format!("{}", Js(x)), want);
        assert_eq!(format!("[{}]", Js(x)), format!("[{want}]"));
    }
}

#[test]
fn radix_by_hand() {
    let cases: &[(f64, u32, &str)] = &[
        (0.0, 2, "0"),
        (-0.0, 16, "0"),
        (1.0, 36, "1"),
        (-1.0, 2, "-1"),
        (255.0, 16, "ff"),
        (-255.0, 16, "-ff"),
        (255.0, 36, "73"),
        (1295.0, 36, "zz"),
        (4_294_967_296.0, 36, "1z141z4"),
        (9_007_199_254_740_991.0, 2, "11111111111111111111111111111111111111111111111111111"),
        (-9_007_199_254_740_991.0, 36, "-2gosa7pa2gv"),
        (123_456_789.0, 10, "123456789"),
    ];
    for &(x, radix, want) in cases {
        assert_eq!(to_radix_string(x, radix).as_deref(), Ok(want), "({x}).toString({radix})");
    }
    for bad in [0.5, -2.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 9_007_199_254_740_992.0, -9_007_199_254_740_992.0, 1e21] {
        assert_eq!(to_radix_string(bad, 10), Err(RangeError), "{bad}");
    }
    for radix in [0, 1, 37, 100, u32::MAX] {
        assert_eq!(to_radix_string(10.0, radix), Err(RangeError), "radix {radix}");
    }
    for radix in 2..=36u32 {
        assert!(to_radix_string(9_007_199_254_740_991.0, radix).unwrap().len() <= 53);
    }
}

/// The outputs of every formatter, on all the bit patterns of the double exponent range and the
/// digits arguments of the API, never panic (debug build: overflow checks, bounds checks).
#[test]
fn no_panic_on_any_double_and_any_argument() {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut checked = 0u64;
    for biased in 0..=0x7FFu64 {
        for _ in 0..4 {
            let mantissa = next() & 0x000F_FFFF_FFFF_FFFF;
            for sign in [0u64, 1] {
                let x = f64::from_bits((sign << 63) | (biased << 52) | mantissa);
                for n in [0u32, 1, 2, 7, 15, 17, 20, 21, 22, 50, 99, 100, 101, 1000, u32::MAX] {
                    let _ = to_fixed(x, n);
                    let _ = to_precision(x, n);
                    let _ = to_exponential(x, n);
                    let _ = to_radix_string(x, n);
                    checked += 4;
                }
                let _ = to_js_string(x);
                let _ = parse_float(&to_js_string(x));
            }
        }
    }
    for exp in 0..=0x7FFu64 {
        for mantissa in [0u64, 1, 0x000F_FFFF_FFFF_FFFF, 0x0008_0000_0000_0000] {
            let x = f64::from_bits((exp << 52) | mantissa);
            for n in 0..=100 {
                let _ = to_fixed(x, n);
                let _ = to_precision(x, n.max(1));
                let _ = to_exponential(x, n);
                checked += 3;
            }
        }
    }
    assert!(checked > 100_000);
}

/// Wave 6 (L): the integer fast path of `String(x)` (`|x| < 2^53`, integral, `-0`) prints what the
/// `ryu_js` path prints, for random integers of every magnitude and the edges.
#[test]
fn integer_fast_path_equals_ryu() {
    let ryu = |x: f64| ryu_js::Buffer::new().format(x).to_owned();
    let two53 = 9_007_199_254_740_992.0_f64;
    let mut edges =
        vec![0.0, -0.0, 1.0, -1.0, 9.0, 10.0, 99.0, 100.0, 1e15, 1e16, two53 - 1.0, -(two53 - 1.0), two53, -two53, two53 + 2.0, 1e20, 1e21];
    // the powers by repeated products (exact: 2^0..2^53 and 10^0..10^22 are doubles)
    let mut p = 1.0_f64;
    for _ in 0..=53 {
        edges.extend([p, -p, p - 1.0, p + 1.0, -(p - 1.0)]);
        p *= 2.0;
    }
    let mut p = 1.0_f64;
    for _ in 0..=22 {
        edges.extend([p, -p, p - 1.0, p + 1.0]);
        p *= 10.0;
    }
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for _ in 0..200_000 {
        let r = next();
        // magnitudes 2^0..2^60: below and above the fast path's bound
        let bits = (r % 61) as u32;
        let v = (next() >> (64 - bits.max(1))) as f64;
        edges.push(if r & 1 == 0 { v } else { -v });
    }
    for x in edges {
        assert_eq!(uo_jsnum::format::to_js_string(x), ryu(x), "{x:?}");
        assert_eq!(format!("{}", uo_jsnum::format::Js(x)), ryu(x), "Js({x:?})");
    }
}
