//! A4 parity (the function bodies, every family): the `bank` capture (the TS `runCase` over the Excel oracle bank:
//! `callFn` over the mock context, or parse + `Engine.recalc` for whole-formula cases) and the `calc-traps` capture
//! (every function × parameter × trap value) replayed through the same entry points in Rust
//! (`tests/support/fnmock.rs`). A case whose evaluation reaches a function without a body yet is **deferred**
//! (counted, not compared); everything else must be equal. A family that lands needs no harness change: its
//! cases turn from deferred into compared.
//!
//! Accepted, by decision (record `uo-calc.md` "A4a — as built"): NaN payloads (A1-2); a TS call that never returned
//! (`timeout:5000ms`) — Rust must return, and quickly (A4a-3). `UO_A4_FN=SUM,ROUND` restricts the replay and prints
//! every difference.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use support::fnmock::{self, Mock, Rnd, TestArg, a1_cell, call_fn, clip, is_nan_text, outcome_text, ref_union, ts_outcome};
use support::{J, parse};
use uo_calc::cell::error_from_text;
use uo_calc::value::{ArrayValue, ErrorCode, Value};
use uo_calc::{JsStr, Scalar, ValueLocale};
use uo_testkit::Capture;

fn oracle_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/packages/calc/test/oracle")
}

/// Every oracle case by id (and its file order).
fn cases() -> HashMap<String, J> {
    let mut out = HashMap::new();
    let mut files: Vec<_> = std::fs::read_dir(oracle_dir()).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for f in files {
        if !f.to_string_lossy().ends_with(".cases.json") {
            continue;
        }
        let j = parse(uo_jsnum::text::trim(&std::fs::read_to_string(&f).unwrap()));
        for c in j.arr() {
            let id = c.get("id").unwrap().js().unwrap().as_js().to_string_lossy();
            out.insert(id, c.clone());
        }
    }
    out
}

fn only() -> Option<HashSet<String>> {
    std::env::var("UO_A4_FN").ok().map(|s| s.split(',').map(|x| uo_jsnum::text::trim(x).to_uppercase()).collect())
}

#[derive(Default, Debug)]
struct Tally {
    compared: usize,
    equal: usize,
    nan: usize,
    deferred: usize,
    /// Legacy formulas the TS rewrites with `@` (frontier A5).
    deferred_a5: usize,
    /// Whole-column trap calls skipped in a debug build.
    skipped_debug: usize,
    timeouts: usize,
    slowest_ms: f64,
    random: usize,
    mismatch: Vec<String>,
    /// Compared and equal, per function.
    per_fn: BTreeMap<String, (usize, usize)>,
}

impl Tally {
    fn report(&self, what: &str) -> String {
        format!(
            "{what}: compared {} equal {} (NaN payload accepted {}) deferred {} (+ {} legacy @, A5; {} whole-column calls skipped in debug) | TS timeouts answered {} (slowest {:.1} ms) | random records {} | mismatch {}",
            self.compared,
            self.equal,
            self.nan,
            self.deferred,
            self.deferred_a5,
            self.skipped_debug,
            self.timeouts,
            self.slowest_ms,
            self.random,
            self.mismatch.len()
        )
    }

    fn check(&mut self, f: &str, key: &str, ours: Option<String>, ts: &str) {
        let Some(ours) = ours else {
            self.deferred += 1;
            return;
        };
        self.compared += 1;
        let e = self.per_fn.entry(f.to_owned()).or_default();
        e.0 += 1;
        if ours == ts {
            self.equal += 1;
            e.1 += 1;
        } else if is_nan_text(&ours) && is_nan_text(ts) {
            self.nan += 1;
            e.1 += 1;
        } else {
            self.mismatch.push(format!("{key}: rust {ours} ts {ts}"));
        }
    }
}

// ---------------------------------------------------------------- bank

fn replay_bank(path: &Path) -> Tally {
    let cap = Capture::read(path).unwrap();
    let side: HashMap<String, String> = support::gunzip(&path.with_extension("values.gz"))
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect();
    let all = cases();
    let filter = only();
    let mut t = Tally::default();
    for r in cap.records() {
        let (id, loc) = match r.key.split_once('@') {
            Some((id, l)) => (id, l),
            None => (r.key.as_str(), "pt-BR"),
        };
        let c = all.get(id).unwrap_or_else(|| panic!("case {id} not in the oracle files"));
        let f = c.get("fn").unwrap().js().unwrap().as_js().to_string_lossy().to_uppercase();
        if filter.as_ref().is_some_and(|s| !s.contains(&f)) {
            continue;
        }
        let ts_raw = r.get("ts").unwrap();
        let ts = if ts_raw.starts_with('#') {
            side.get(r.key.as_str()).map_or_else(|| ts_raw.to_owned(), |v| clip(v.clone()))
        } else {
            ts_raw.to_owned()
        };
        let ts = ts_outcome(&ts);
        let locale = if loc == "en-US" { ValueLocale::en_us() } else { ValueLocale::pt_br() };
        if fnmock::legacy_deferred(c) {
            t.deferred_a5 += 1;
            continue;
        }
        let ours = if fnmock::is_formula_case(c) { fnmock::run_formula(c, locale) } else { fnmock::run_plain(c, locale) };
        t.check(&f, &r.key, ours, &ts);
    }
    t
}

// ---------------------------------------------------------------- calc-traps

/// A trap/base encoding → a test argument (`toArg` of `capture-traps.mjs`).
fn trap_arg(m: &mut Mock, enc: &str) -> TestArg {
    match enc.as_bytes()[0] {
        b'O' => TestArg::Omitted,
        b'R' => TestArg::Val(Value::Ref(ref_union(&enc[1..], 0))),
        b'A' => {
            let rows: Vec<Vec<Scalar>> = parse(&enc[1..])
                .arr()
                .iter()
                .map(|r| r.arr().iter().map(|x| scalar_enc(m, &x.js().unwrap().as_js().to_string_lossy())).collect())
                .collect();
            TestArg::Val(Value::Array(ArrayValue::of(&rows)))
        }
        _ => TestArg::Val(Value::Scalar(scalar_enc(m, enc))),
    }
}

/// A value text (`n…`, `s…`, `b…`, `e…`, `_`) → scalar.
fn scalar_enc(m: &mut Mock, t: &str) -> Scalar {
    let b = t.as_bytes();
    match b[0] {
        b'_' => Scalar::Empty,
        b'n' => Scalar::Num(f64::from_bits(u64::from_str_radix(&t[1..17], 16).unwrap())),
        b'b' => Scalar::Bool(b[1] == b'1'),
        b'e' => Scalar::Err(error_from_text(JsStr::Latin1(&b[1..])).unwrap()),
        b's' => {
            let u = match parse(&t[1..]) {
                J::Str(u) => u,
                j => panic!("string encoding {j:?}"),
            };
            m.str(&u)
        }
        _ => panic!("value text {t}"),
    }
}

/// The mock sheet of a trap call: the trap cells, then the bank case's own cells (strings that name an error code
/// become the error, as `capture-traps.mjs` does).
fn trap_mock(trap_cells: &J, case: Option<&J>, loc: ValueLocale, rnd: Vec<f64>) -> Mock {
    let mut m = Mock::new(loc);
    m.at = a1_cell("Z100").unwrap();
    m.array_context = Some(true);
    m.rnd = Rnd::Seq(rnd, 0);
    if let J::Obj(o) = trap_cells {
        for (k, v) in o {
            let s = scalar_enc(&mut m, &v.js().unwrap().as_js().to_string_lossy());
            m.grids[0].insert(a1_cell(k).unwrap(), s);
        }
    }
    if let Some(J::Obj(o)) = case.and_then(|c| c.get("cells")) {
        for (k, v) in o {
            let s = match v {
                J::Str(u) => {
                    let t = String::from_utf16_lossy(u);
                    match error_from_text(JsStr::Latin1(t.as_bytes())) {
                        Some(e) if t.starts_with('#') && t.is_ascii() => Scalar::Err(e),
                        _ => fnmock::case_scalar(&mut m, v),
                    }
                }
                _ => fnmock::case_scalar(&mut m, v),
            };
            m.grids[0].insert(a1_cell(k).unwrap(), s);
        }
    }
    m
}

/// One trap call: key, the trapped positions and their encodings, the TS outcome, the random values it consumed.
type Call = (String, Vec<(usize, String)>, String, Vec<f64>);

fn replay_traps(path: &Path) -> Tally {
    let cap = Capture::read(path).unwrap();
    let trap_cells = parse(cap.meta("trap-cells").unwrap());
    let all = cases();
    let filter = only();
    let mut t = Tally::default();
    let mut base: HashMap<&str, (Vec<String>, Option<&J>)> = HashMap::new();
    for r in cap.records() {
        if r.key.contains('#') {
            continue;
        }
        let b: Vec<String> = parse(r.get("base").unwrap()).arr().iter().map(|x| x.js().unwrap().as_js().to_string_lossy()).collect();
        let case = r.get("case").and_then(|id| all.get(id));
        base.insert(r.key.as_str(), (b, case));
    }
    for r in cap.records() {
        let Some((fname, rest)) = r.key.split_once('#') else { continue };
        if filter.as_ref().is_some_and(|s| !s.contains(fname)) {
            continue;
        }
        let (pos, loc) = match rest.split_once('@') {
            Some((p, l)) => (p, l),
            None => (rest, "pt-BR"),
        };
        let (b, case) = &base[fname];
        let ix = uo_calc::fnreg::lookup_fn(JsStr::Latin1(fname.as_bytes())).unwrap();
        if uo_calc::functions::body(ix).is_none() {
            // no body yet: every call stops at it (deferred) — not run (the whole-column traps cost a 1M-cell fill)
            t.deferred += r.facets.iter().filter(|(f, _)| !f.ends_with(".rnd") && f != "at" && f != "t").count();
            continue;
        }
        let min = usize::from(ix.entry().def.unwrap().min_args);
        let loc_v = || if loc == "en-US" { ValueLocale::en_us() } else { ValueLocale::pt_br() };
        let hex_list = |s: Option<&str>| -> Vec<f64> {
            s.map(|s| s.split(',').map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap())).collect()).unwrap_or_default()
        };
        // the calls of the record: one per trap facet (`#p<i>`), or the pair (`#x<k>`: `at`, `t`, `v`)
        let mut calls: Vec<Call> = Vec::new();
        if let Some(p) = pos.strip_prefix('p') {
            let p: usize = p.parse().unwrap();
            for (facet, ts) in &r.facets {
                if facet.ends_with(".rnd") {
                    continue;
                }
                let trap = cap.meta(&format!("trap.{facet}")).unwrap().to_owned();
                calls.push((format!("{}/{facet}", r.key), vec![(p, trap)], ts.clone(), hex_list(r.get(&format!("{facet}.rnd")))));
            }
        } else if pos.starts_with('x') {
            let at: Vec<usize> = r.get("at").unwrap().split(',').map(|x| x.parse().unwrap()).collect();
            let ids: Vec<&str> = r.get("t").unwrap().split(',').collect();
            let traps = vec![
                (at[0], cap.meta(&format!("trap.{}", ids[0])).unwrap().to_owned()),
                (at[1], cap.meta(&format!("trap.{}", ids[1])).unwrap().to_owned()),
            ];
            calls.push((r.key.clone(), traps, r.get("v").unwrap().to_owned(), hex_list(r.get("rnd"))));
        }
        for (key, traps, ts, rnd) in calls {
            if cfg!(debug_assertions) && traps.iter().any(|(_, tr)| tr == "RAX:AX") {
                // a whole column: the mock fills 1,048,576 cells and the result text is hashed (≈ 10 s in a debug
                // build for the ported families); compared by the release runs (`cargo test --release`)
                t.skipped_debug += 1;
                continue;
            }
            let mut m = trap_mock(&trap_cells, *case, loc_v(), rnd.clone());
            let mut xs: Vec<String> = b.clone();
            while xs.len() < b.len().max(min) {
                xs.push("n4000000000000000".into());
            }
            for (p, trap) in &traps {
                if *p < xs.len() {
                    xs[*p] = trap.clone();
                } else {
                    xs.push(trap.clone());
                }
            }
            let args: Vec<TestArg> = xs.iter().map(|e| trap_arg(&mut m, e)).collect();
            let t0 = Instant::now();
            let res = call_fn(fname, &args, &mut m, true);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            if std::env::var("UO_A4_SLOW").is_ok() && ms > 20.0 {
                eprintln!("slow {key} {ms:.1} ms");
            }
            let res = match res {
                Ok(Value::Ref(rv)) => match rv.single() {
                    None => Ok(Value::Scalar(Scalar::Err(ErrorCode::Value))),
                    Some(a) if a.r1 == a.r2 && a.c1 == a.c2 => {
                        uo_calc::fnreg::FnContext::cell(&mut m, a.sheet, a.r1, a.c1).map(Value::Scalar)
                    }
                    Some(_) => uo_calc::fnreg::FnContext::values(&mut m, &rv, 0).map(Value::Array),
                },
                r => r,
            };
            let ours = outcome_text(&m, &res);
            if ts.starts_with("timeout:") {
                if ours.is_some() {
                    t.timeouts += 1;
                    t.slowest_ms = uo_jsnum::math::max(t.slowest_ms, ms);
                    assert!(ms < 2000.0, "{key}: the TS never returned; Rust took {ms:.0} ms");
                } else {
                    t.deferred += 1;
                }
                continue;
            }
            if ours.is_some() && !rnd.is_empty() {
                t.random += 1;
            }
            let ours = ours.map(|o| {
                // the random values consumed must be the TS's
                if m.consumed() != rnd.len() { format!("{o} [random {} / {}]", m.consumed(), rnd.len()) } else { o }
            });
            t.check(fname, &key, ours, &ts_outcome(&ts));
        }
    }
    t
}

fn print(t: &Tally, what: &str) {
    eprintln!("{}", t.report(what));
    let ported: HashSet<&str> = uo_calc::functions::ported().collect();
    let (mut c, mut e, mut nf) = (0, 0, 0);
    for (f, (cc, ee)) in &t.per_fn {
        if ported.contains(f.as_str()) {
            c += cc;
            e += ee;
            nf += 1;
        }
    }
    eprintln!(
        "  of which the {nf} functions with a body: compared {c} equal {e}; the rest are formula/operator cases or argument errors applyFn answers"
    );
    let n = if only().is_some() { usize::MAX } else { 40 };
    for m in t.mismatch.iter().take(n) {
        eprintln!("  {m}");
    }
}

/// The bank (35,299 cases in pt-BR): every case of a function with a body equal.
#[test]
fn a4_bank() {
    let t = replay_bank(&support::captures_dir().join("bank.capture"));
    print(&t, "bank");
    assert!(t.mismatch.is_empty(), "{}", t.report("bank"));
}

/// The calc-traps capture (CI: 54,120 calls): every call of a function with a body equal (or a TS hang answered).
#[test]
fn a4_traps() {
    let t = replay_traps(&support::captures_dir().join("calc-traps.capture"));
    print(&t, "calc-traps");
    assert!(t.mismatch.is_empty(), "{}", t.report("calc-traps"));
}

/// The local big captures (`UO_A4_BIG=1`: `core/target/sheet-cap/bank-big.capture`, `calc-traps-big.capture`).
#[test]
fn a4_big() {
    if !uo_testkit::opt_in("UO_A4_BIG", "the big bank and calc-traps captures") {
        return;
    }
    let dir = support::captures_dir().join("../../../../target/sheet-cap");
    let b = replay_bank(&dir.join("bank-big.capture"));
    print(&b, "bank-big");
    let t = replay_traps(&dir.join("calc-traps-big.capture"));
    print(&t, "calc-traps-big");
    assert!(b.mismatch.is_empty() && t.mismatch.is_empty());
}
