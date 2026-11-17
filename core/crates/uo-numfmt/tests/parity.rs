//! Parity replay of uo-numfmt against `tests/captures/numfmt.capture`, written by
//! `js/capture-numfmt.mjs` from the frozen TS engine (`shared/charts/dist/numfmt.js`): the Excel
//! oracle (1,709 cases), the cases of numfmt.test.ts and a seeded differential corpus (codes x values
//! x locales). 100% equal is required; the Excel agreement of the TS and of this port is compared
//! case by case (`oracle_agreement`).
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::{Locales, Val, capture, f64_hex, f64_of, json, opts_of, parts_text, units_of};
use uo_numfmt::{
    DEFAULT_GENERAL_BUDGET, FormatOptions, Value, builtin_format_code, decimal_digits, format_general, format_value, format_value_parts,
    pt_br, serial_to_date_ms, utc_fields,
};
use uo_testkit::Replay;

fn code_of(s: &str) -> Option<Vec<u16>> {
    (s != "~").then(|| units_of(s))
}

fn is_kind(k: &str, kinds: &str) -> bool {
    let (kind, num) = k.split_at(1);
    kinds.contains(kind) && !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit())
}

fn replay_codes(cap: &uo_testkit::Capture) -> (u64, u64) {
    let locs = Locales::new();
    let cap = &cap.subset(|k| is_kind(k, "k"));
    let mut r = Replay::new(cap);
    let (mut codes, mut cases) = (0, 0);
    for rec in cap.records() {
        let k = rec.key.as_str();
        if !(k.starts_with('k') && k[1..].bytes().all(|b| b.is_ascii_digit())) {
            continue;
        }
        codes += 1;
        let code = code_of(rec.get("code").unwrap());
        let loc = locs.get(rec.get("loc").unwrap());
        let opts = opts_of(rec.get("opts").unwrap(), loc);
        r.skip(k, "code");
        r.skip(k, "loc");
        r.skip(k, "opts");
        let mut i = 0;
        while let Some(v) = rec.get(&format!("v{i}")) {
            r.skip(k, &format!("v{i}"));
            let val = Val::parse(v);
            let t = format_value(val.value(), code.as_deref(), &opts);
            let p = format_value_parts(val.value(), code.as_deref(), &opts);
            r.check(k, &format!("t{i}"), &json(&t.text));
            r.check(k, &format!("c{i}"), t.color.as_deref().unwrap_or("-"));
            r.check(k, &format!("p{i}"), &parts_text(&p));
            cases += 1;
            i += 1;
        }
    }
    r.finish().assert_clean_summary(codes, cases)
}

trait Clean {
    fn assert_clean_summary(self, codes: u64, cases: u64) -> (u64, u64);
}
impl Clean for uo_testkit::Report {
    fn assert_clean_summary(self, codes: u64, cases: u64) -> (u64, u64) {
        println!("codes {codes}, cases {cases}: {}", self.summary());
        self.assert_clean();
        (codes, cases)
    }
}

#[test]
fn parity_format_value_and_parts() {
    let cap = capture();
    let (codes, cases) = replay_codes(&cap);
    assert_eq!(codes.to_string(), cap.meta("codes").unwrap());
    assert_eq!(cases.to_string(), cap.meta("format-cases").unwrap());
    assert!(cases > 50_000);
}

#[test]
fn oracle_agreement() {
    let full = capture();
    let cap = full.subset(|k| is_kind(k, "o"));
    let mut r = Replay::new(&cap);
    let (mut n, mut ts_xl, mut rust_xl, mut both) = (0u32, 0u32, 0u32, 0u32);
    for rec in cap.records() {
        let k = rec.key.as_str();
        if !(k.starts_with('o') && k[1..].bytes().all(|b| b.is_ascii_digit())) {
            continue;
        }
        n += 1;
        let code = code_of(rec.get("code").unwrap());
        let val = Val::parse(rec.get("v").unwrap());
        r.skip(k, "code");
        r.skip(k, "v");
        let text = json(&format_value(val.value(), code.as_deref(), &FormatOptions::default()).text);
        r.check(k, "ts", &text);
        r.skip(k, "xl");
        let xl = rec.get("xl").unwrap();
        ts_xl += u32::from(rec.get("ts") == Some(xl));
        rust_xl += u32::from(text == xl);
        both += u32::from(rec.get("ts") == Some(xl) && text == xl);
    }
    r.finish().assert_clean();
    println!("oracle cases {n}: TS == Excel {ts_xl}, Rust == Excel {rust_xl}, both {both}");
    assert_eq!(n.to_string(), full.meta("oracle-cases").unwrap());
    assert_eq!(n, 1709);
    // identical agreement, case by case: whatever the TS gets right the port gets right, and vice versa
    assert_eq!((ts_xl, rust_xl), (both, both));
}

#[test]
fn parity_general_digits_builtins_serials() {
    let cap = capture().subset(|k| is_kind(k, "gdbs"));
    let locs = Locales::new();
    let mut r = Replay::new(&cap);
    let counts = [0usize; 4];
    let mut counts = counts;
    for rec in cap.records() {
        let k = rec.key.as_str();
        let (kind, num) = k.split_at(1);
        if !num.bytes().all(|b| b.is_ascii_digit()) || num.is_empty() {
            continue;
        }
        match kind {
            "g" => {
                let x = match Val::parse(rec.get("x").unwrap()) {
                    Val::Num(x) => x,
                    Val::Str(_) => panic!(),
                };
                let loc = locs.get(rec.get("loc").unwrap()).unwrap_or_else(|| pt_br());
                let b = match rec.get("b").unwrap() {
                    "~" => DEFAULT_GENERAL_BUDGET,
                    s => s.parse().unwrap(),
                };
                let s = format_general(x, loc, b);
                r.check(k, "out", &json(&uo_text::utf16::from_str(&s)));
                r.skip(k, "x");
                r.skip(k, "loc");
                r.skip(k, "b");
                counts[0] += 1;
            }
            "d" => {
                let Val::Num(x) = Val::parse(rec.get("x").unwrap()) else { panic!() };
                let n: usize = rec.get("n").unwrap().parse().unwrap();
                let (i, f) = decimal_digits(x, n);
                r.check(k, "out", &format!("{i}|{f}"));
                r.skip(k, "x");
                r.skip(k, "n");
                counts[1] += 1;
            }
            "b" => {
                let id: u32 = rec.get("id").unwrap().parse().unwrap();
                let loc = locs.get(rec.get("loc").unwrap()).unwrap_or_else(|| pt_br());
                let out = builtin_format_code(id, loc).map_or("-".to_owned(), |s| json(&uo_text::utf16::from_str(s)));
                r.check(k, "out", &out);
                r.skip(k, "id");
                r.skip(k, "loc");
                counts[2] += 1;
            }
            "s" => {
                let Val::Num(x) = Val::parse(rec.get("x").unwrap()) else { panic!() };
                let t = serial_to_date_ms(x);
                r.check(k, "t", &f64_hex(t));
                let f = utc_fields(t).map_or("-".to_owned(), |f| {
                    format!("{},{},{},{},{},{},{},{}", f.year, f.month, f.day, f.weekday, f.hour, f.minute, f.second, f.millisecond)
                });
                r.check(k, "f", &f);
                r.skip(k, "x");
                counts[3] += 1;
            }
            _ => {}
        }
    }
    println!("general {}, digits {}, builtins {}, serials {}: checked", counts[0], counts[1], counts[2], counts[3]);
    let rep = r.finish();
    println!("{}", rep.summary());
    rep.assert_clean();
    assert!(counts.iter().all(|&c| c > 100));
    let _ = (f64_of, Value::Num(0.0));
}

/// A bigger local run with another seed (not committed): `node js/capture-numfmt.mjs --n 60000 --seed 7 --out <file>`,
/// then `UO_NUMFMT_BIG=1 UO_NUMFMT_CAPTURE=<file> cargo test -p uo-numfmt --release --test parity big`.
#[test]
fn parity_big_local_capture() {
    if !uo_testkit::opt_in("UO_NUMFMT_BIG", "replay of a big local capture (UO_NUMFMT_CAPTURE=<file>)") {
        return;
    }
    let path = std::env::var("UO_NUMFMT_CAPTURE").unwrap();
    let cap = uo_testkit::Capture::read(path).unwrap();
    let (codes, cases) = replay_codes(&cap);
    assert!(codes > 0 && cases > 0);
}
