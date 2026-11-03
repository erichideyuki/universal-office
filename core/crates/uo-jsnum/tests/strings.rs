//! `to_number`, `parse_int` and `text` against Node: the corpus (inputs and results written in
//! `tests/captures/jsnum.capture`), the white space set over every code point, digests over 10^5
//! (10^6 with `--include-ignored`) random strings from token alphabets, and the cases of the
//! skeptic (S9) and of the plan written out by hand.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::{capture, hex};
use uo_jsnum::conv::{canonicalize_nan, to_int32};
use uo_jsnum::parse::{parse_int, to_number};
use uo_jsnum::text::{is_js_space, js_space_len, split_js_space, trim, trim_end, trim_start, utf16_len, utf16_slice};
use uo_testkit::generate::Rng;
use uo_testkit::stream::json_string;
use uo_testkit::{Replay, Stream};

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

fn json_array(parts: impl Iterator<Item = String>) -> String {
    format!("[{}]", parts.map(|p| json_string(&p)).collect::<Vec<_>>().join(","))
}

#[test]
fn corpus_matches_node() {
    let cap = capture().subset(|k| k.starts_with("corpus/"));
    assert!(cap.len() > 150);
    let mut replay = Replay::new(&cap);
    for rec in cap.records() {
        let key = rec.key.as_str();
        let s = json_unescape(rec.get("s").unwrap());
        replay.skip(key, "s");
        replay.check(key, "num", &hex(to_number(&s)));
        for (facet, _) in &rec.facets {
            if let Some(radix) = facet.strip_prefix("pi_") {
                let radix = to_int32(radix.parse::<f64>().unwrap());
                replay.check(key, facet, &hex(parse_int(&s, radix)));
            }
        }
        replay.check(key, "trim", &json_string(trim(&s)));
        replay.check(key, "trim_start", &json_string(trim_start(&s)));
        replay.check(key, "trim_end", &json_string(trim_end(&s)));
        replay.check(key, "split", &json_array(split_js_space(&s).map(str::to_owned)));
        replay.check(key, "len", &utf16_len(&s).to_string());
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn white_space_set_is_the_js_set_over_every_code_point() {
    let cap = capture();
    let want: Vec<u32> = cap.get("text/space_set", "chars").unwrap().split(',').map(|h| u32::from_str_radix(h, 16).unwrap()).collect();
    let mut got = Vec::new();
    for cp in 0..=0x10_FFFFu32 {
        let Some(c) = char::from_u32(cp) else { continue };
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes().to_vec();
        let len = js_space_len(&bytes, 0);
        if is_js_space(c) {
            got.push(cp);
            assert_eq!(len, bytes.len(), "js_space_len of U+{cp:04X}");
        } else {
            assert_eq!(len, 0, "js_space_len of U+{cp:04X}");
        }
        assert_eq!(trim(&format!("x{c}")) == "x", is_js_space(c));
    }
    assert_eq!(got, want);
    assert_eq!(got.len(), 25);
    assert!(!is_js_space('\u{85}') && !is_js_space('\u{180E}') && is_js_space('\u{FEFF}'));
}

const TOK_JUNK: [&str; 26] = [
    "0", "1", "7", "9", "a", "f", "z", "x", "b", "o", "e", "E", "+", "-", ".", " ", "\t", "\u{a0}", "\u{feff}", "\u{85}", "\u{180e}", "_",
    "Infinity", "0x", "\u{2003}", "\u{661}",
];
const TOK_NUM: [&str; 20] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "9", "0", ".", "e", "-", "+", " ", "x", "a", "f"];
const TOK_TEXT: [&str; 12] = ["a", "b", "\u{e9}", " ", "\u{a0}", "\u{feff}", "\t", "\u{1f600}", "\u{10FFFF}", "\u{4e2d}", "\u{2028}", "x"];

fn rand_string(r: &mut Rng, tokens: &[&str], max_tokens: u32) -> String {
    let n = r.next_u32() % max_tokens;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(tokens[(r.next_u32() as usize) % tokens.len()]);
    }
    s
}

type Produce = fn(&mut Rng, &mut Stream);

fn records() -> Vec<(String, usize, usize, Produce)> {
    let mut v: Vec<(String, usize, usize, Produce)> = Vec::new();
    v.push(("rand/to_number_junk".into(), 100_000, 1_000_000, |r, s| {
        s.f64(canonicalize_nan(to_number(&rand_string(r, &TOK_JUNK, 12))));
    }));
    v.push(("rand/to_number_digits".into(), 100_000, 1_000_000, |r, s| {
        s.f64(canonicalize_nan(to_number(&rand_string(r, &TOK_NUM, 16))));
    }));
    macro_rules! pi {
        ($radix:literal) => {
            v.push((format!("rand/parse_int_{}_junk", $radix), 100_000, 1_000_000, |r, s| {
                s.f64(canonicalize_nan(parse_int(&rand_string(r, &TOK_JUNK, 12), $radix)));
            }));
            v.push((format!("rand/parse_int_{}_digits", $radix), 100_000, 1_000_000, |r, s| {
                s.f64(canonicalize_nan(parse_int(&rand_string(r, &TOK_NUM, 16), $radix)));
            }));
        };
    }
    pi!(0);
    pi!(2);
    pi!(7);
    pi!(8);
    pi!(10);
    pi!(16);
    pi!(36);
    v.push(("rand/text".into(), 10_000, 100_000, |r, s| {
        let t = rand_string(r, &TOK_TEXT, 8);
        let len = utf16_len(&t) as i64;
        s.f64(len as f64).str(trim(&t)).str(trim_start(&t)).str(trim_end(&t));
        let parts: Vec<&str> = split_js_space(&t).collect();
        s.u32(parts.len() as u32);
        for p in parts {
            s.str(p);
        }
        for a in -len - 1..=len + 1 {
            for b in -len - 1..=len + 1 {
                s.str(&utf16_slice(&t, a, b));
            }
        }
    }));
    v
}

fn replay_random(full: bool) {
    let cap = capture().subset(|k| k.starts_with("rand/"));
    let mut replay = Replay::new(&cap);
    let recs = records();
    assert_eq!(recs.len(), cap.len());
    let results: Vec<(String, String, String, usize)> = std::thread::scope(|sc| {
        let handles: Vec<_> = recs
            .iter()
            .map(|(key, n_small, n_full, produce)| {
                sc.spawn(move || {
                    let mut rng = Rng::new(support::fnv(key));
                    let mut s = Stream::new();
                    let total = if full { *n_full } else { *n_small };
                    let mut small = String::new();
                    for i in 0..total {
                        produce(&mut rng, &mut s);
                        if i + 1 == *n_small {
                            small = s.digest().short();
                        }
                    }
                    let fullh = if full { s.finish().short() } else { String::new() };
                    (key.clone(), small, fullh, total)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (key, small, fullh, _) in &results {
        replay.check(key, "seed", &format!("{:x}", support::fnv(key)));
        replay.skip(key, "n_small");
        replay.skip(key, "n_full");
        replay.check(key, "small", small);
        if full {
            replay.check(key, "full", fullh);
        } else {
            replay.skip(key, "full");
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn random_strings_at_10_5_match_node() {
    replay_random(false);
}

#[test]
#[ignore = "10^6 strings per record: cargo test -p uo-jsnum --release -- --include-ignored"]
fn random_strings_at_10_6_match_node() {
    replay_random(true);
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn number_of_a_string_by_hand() {
    let nan = f64::NAN;
    let cases: &[(&str, f64)] = &[
        ("", 0.0),
        ("  \t\n\u{a0}\u{feff}", 0.0),
        ("-0x10", nan),
        ("+0x10", nan),
        ("0x10", 16.0),
        ("0X1a", 26.0),
        ("0b101", 5.0),
        ("0o17", 15.0),
        ("0x", nan),
        ("0b2", nan),
        ("1e", nan),
        ("5.", 5.0),
        (".", nan),
        (".5", 0.5),
        ("+.5e-3", 0.0005),
        ("1_000", nan),
        ("Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
        ("+Infinity", f64::INFINITY),
        ("infinity", nan),
        ("Infinityx", nan),
        ("1e1000", f64::INFINITY),
        ("1e-400", 0.0),
        ("1e99999999999999999999", f64::INFINITY),
        ("1e-99999999999999999999", 0.0),
        ("0e99999999999999999999", 0.0),
        ("\u{85}1", nan),
        ("\u{180e}1", nan),
        ("\u{feff}12\u{a0}", 12.0),
        ("9007199254740993", 9_007_199_254_740_992.0),
        ("0x20000000000001", 9_007_199_254_740_992.0),
        ("0x20000000000003", 9_007_199_254_740_996.0),
    ];
    for (s, want) in cases {
        assert!(same(to_number(s), *want), "Number({s:?}) = {} (want {want})", to_number(s));
    }
    assert!(to_number("-0").is_sign_negative() && to_number("-0") == 0.0);
    assert_eq!(to_number(&format!("0x{}", "f".repeat(300))), f64::INFINITY);
    assert_eq!(to_number(&format!("9007199254740993.{}1", "0".repeat(800))), 9_007_199_254_740_994.0);
    assert_eq!(to_number(&format!("9007199254740993.{}", "0".repeat(800))), 9_007_199_254_740_992.0);
    let long = "1".repeat(10_000_000);
    assert_eq!(to_number(&long), f64::INFINITY);
}

#[test]
fn parse_int_by_hand() {
    let nan = f64::NAN;
    let cases: &[(&str, i32, f64)] = &[
        ("-0x10", 0, -16.0),
        ("0x10", 10, 0.0),
        ("0x10", 16, 16.0),
        ("0x10", 0, 16.0),
        ("10", to_int32(4_294_967_312.0), 16.0),
        ("9007199254740993", 0, 9_007_199_254_740_992.0),
        ("  12px", 0, 12.0),
        ("-", 0, nan),
        ("0x", 0, nan),
        ("0xg", 0, nan),
        ("abc", 0, nan),
        ("abc", 16, 2748.0),
        ("z", 36, 35.0),
        ("12", 1, nan),
        ("12", 37, nan),
        ("0b11", 0, 0.0),
        ("0.9", 0, 0.0),
        ("1e3", 0, 1.0),
        ("\u{85}1", 0, nan),
        ("+7", 8, 7.0),
        ("19", 8, 1.0),
        ("0007", 0, 7.0),
        ("0z", 36, 35.0),
    ];
    for (s, radix, want) in cases {
        assert!(same(parse_int(s, *radix), *want), "parseInt({s:?}, {radix}) = {} (want {want})", parse_int(s, *radix));
    }
    assert!(parse_int("-0", 0).is_sign_negative() && parse_int("-0", 0) == 0.0);
    assert!(parse_int("-0abc", 0).is_sign_negative());
    assert_eq!(parse_int(&"9".repeat(400), 10), f64::INFINITY);
    assert_eq!(parse_int(&format!("0x{}", "f".repeat(300)), 16), f64::INFINITY);
    assert_eq!(parse_int(&"7".repeat(40), 8), f64::from_bits((1023u64 + 120) << 52));
}

#[test]
fn text_by_hand() {
    assert_eq!(split_js_space("").collect::<Vec<_>>(), [""]);
    assert_eq!(split_js_space("   ").collect::<Vec<_>>(), ["", ""]);
    assert_eq!(split_js_space(" a  b\u{a0}c ").collect::<Vec<_>>(), ["", "a", "b", "c", ""]);
    assert_eq!(split_js_space("abc").collect::<Vec<_>>(), ["abc"]);
    assert_eq!(utf16_len("a\u{1f600}b"), 4);
    assert_eq!(utf16_slice("a\u{1f600}b", 0, 4), "a\u{1f600}b");
    assert_eq!(utf16_slice("a\u{1f600}b", 1, 3), "\u{1f600}");
    assert_eq!(utf16_slice("a\u{1f600}b", 2, 4), "\u{fffd}b");
    assert_eq!(utf16_slice("a\u{1f600}b", 0, 2), "a\u{fffd}");
    assert_eq!(utf16_slice("a\u{1f600}b", 2, 3), "\u{fffd}");
    assert_eq!(utf16_slice("abc", -2, i64::MAX), "bc");
    assert_eq!(utf16_slice("abc", i64::MIN, i64::MAX), "abc");
    assert_eq!(utf16_slice("abc", 2, 1), "");
    assert_eq!(utf16_slice("abc", 5, 9), "");
    assert!(matches!(utf16_slice("a\u{1f600}b", 1, 3), std::borrow::Cow::Borrowed(_)));
    assert_eq!(trim("\u{feff} x \u{2028}"), "x");
    assert_eq!(trim("\u{85}x\u{85}"), "\u{85}x\u{85}");
}
