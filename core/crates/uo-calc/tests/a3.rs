//! A3 parity: the `calc-order` capture group (`js/capture-order.mjs`) replayed through the Rust engine over a store
//! built from the captured `CalcWorkbook`: the evaluation order of every run (`recalc`, `calculate`, the 12 edits),
//! the random values consumed, `circular`, internal errors, every cell value, the full recalc of the final inputs and
//! incremental = full (record `uo-calc.md` "A3 — as built").
//!
//! Function bodies are frontier A4 and the legacy `@` rewrite is A5: a cell whose evaluation reached a body
//! (`Engine::not_ported`), a legacy formula (the capture's `legacy` facet; edit formulas by a conservative text test)
//! and everything that depends on them — through the static graph, or through a spill footprint that differs — is
//! **deferred** and counted, not compared. Group `ops` (operator-only workbooks) has no body: it must be exact.
//!
//! Local run: `node core/crates/uo-calc/js/capture-order.mjs --gen x1-big --ops 400 --out
//! core/target/sheet-cap/calc-order-big.capture`, then `UO_A3_BIG=1 cargo test -p uo-calc --release --test a3`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use support::calcwb::{self, build};
use support::{J, list_digest, parse, sidecar};
use uo_calc::engine::{CellInput, Engine, EngineOptions, FormulaInput};
use uo_calc::key::Area;
use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};
use uo_testkit::Capture;

type Cell = (u32, u32, u32);

fn cell_of(s: &str) -> Cell {
    let v: Vec<u32> = s.split(',').map(|x| x.parse().unwrap()).collect();
    (v[0], v[1], v[2])
}

fn key_of(line: &str) -> Cell {
    cell_of(&line[..line.find('=').unwrap()])
}

fn hexf(h: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(h, 16).unwrap())
}

#[derive(Default, Debug)]
struct Tally {
    workbooks: usize,
    runs: usize,
    ord_equal: usize,
    ord_deferred: usize,
    ord_mismatch: Vec<String>,
    evaluations: usize,
    rnd_equal: usize,
    rnd_deferred: usize,
    rnd_mismatch: Vec<String>,
    circ_equal: usize,
    circ_deferred: usize,
    circ_mismatch: Vec<String>,
    vals_compared: usize,
    vals_deferred: usize,
    vals_mismatch: Vec<String>,
    incfull_equal: usize,
    incfull_deferred: usize,
    incfull_mismatch: Vec<String>,
    ierr_mismatch: Vec<String>,
}

struct Book<'a> {
    key: &'a str,
    ops: bool,
    store: Store,
    engine: Engine,
    tainted: BTreeSet<Cell>,
    clock: Clock,
    /// Cells whose formula may consume random values (RAND, RANDBETWEEN, RANDARRAY, directly, through a shared
    /// formula's anchor or a defined name): when a run's random stream is deferred they are deferred too (A4a).
    rand: BTreeSet<Cell>,
}

/// The cells of a workbook whose formula may consume random values (a superset).
fn rand_cells(wb: &J) -> BTreeSet<Cell> {
    let up = |j: Option<&J>| j.and_then(J::js).map(|x| x.as_js().to_string_lossy().to_uppercase()).unwrap_or_default();
    let rand_names: Vec<String> = wb
        .get("names")
        .map(J::arr)
        .unwrap_or(&[])
        .iter()
        .filter(|n| up(n.get("formula")).contains("RAND"))
        .map(|n| up(n.get("name")))
        .collect();
    let is_rand = |t: &str| t.contains("RAND") || rand_names.iter().any(|n| t.contains(n.as_str()));
    let mut out = BTreeSet::new();
    for (si, sh) in wb.get("sheets").unwrap().arr().iter().enumerate() {
        let mut shared: HashMap<String, bool> = HashMap::new();
        let cells = sh.get("cells").unwrap().arr();
        for c in cells {
            let a = c.arr();
            if let Some(f) = a[2].get("f")
                && let Some(si_) = f.get("si").and_then(J::num)
            {
                let t = up(f.get("text"));
                if !t.is_empty() {
                    shared.insert(format!("{si_}"), is_rand(&t));
                }
            }
        }
        for c in cells {
            let a = c.arr();
            let Some(f) = a[2].get("f") else { continue };
            let t = up(f.get("text"));
            let via_shared = f.get("si").and_then(J::num).is_some_and(|x| shared.get(&format!("{x}")).copied().unwrap_or(false));
            if is_rand(&t) || via_shared {
                out.insert((si as u32, a[0].num().unwrap() as u32, a[1].num().unwrap() as u32));
            }
        }
    }
    out
}

fn opts(ignore: bool) -> EngineOptions {
    let mut o = EngineOptions::new(ValueLocale::pt_br());
    o.ignore_cache = ignore;
    o.file_name = Some(JsString::from_str_js("Pasta1.xlsx"));
    o.trace = true;
    o
}

/// An edit formula that may get the legacy `@` (conservative: ranges, tables, multi-cell or sheet-scoped names).
fn maybe_legacy(text: &str) -> bool {
    text.contains(':') || text.contains('[') || text.contains("Local") || text.contains("Faixa") || text.contains("ANCHORARRAY")
}

impl Book<'_> {
    /// The deferred cells after a step: the persistent set ∪ this step's not-ported cells ∪ `extra`, closed under the
    /// static dependents and the owned cells of tainted anchors.
    fn taint(&mut self, extra: &[Cell]) {
        let mut seeds: Vec<Cell> = self.engine.not_ported().iter().map(|&(s, r, c, _)| (s.0, r, c)).collect();
        self.engine.clear_not_ported();
        seeds.extend_from_slice(extra);
        self.tainted.extend(seeds);
        if self.tainted.is_empty() {
            return;
        }
        let mut areas: Vec<Area> = self.tainted.iter().map(|&(s, r, c)| Area { sheet: SheetIx(s), r1: r, c1: c, r2: r, c2: c }).collect();
        for &(s, r, c) in &self.tainted {
            if let Some(a) = self.engine.spill_of(SheetIx(s), r, c) {
                areas.push(a);
            }
        }
        for (s, r, c) in self.engine.dependents_of(&mut self.store, &areas) {
            self.tainted.insert((s.0, r, c));
        }
        // owned cells of tainted anchors
        let mut owned = Vec::new();
        self.engine.for_each_value(&self.store, |s, r, c, _| {
            if let Some((os, or, oc)) = self.engine.owner(s, r, c)
                && self.tainted.contains(&(os.0, or, oc))
            {
                owned.push((s.0, r, c));
            }
        });
        self.tainted.extend(owned);
    }
}

fn run_book(key: &str, rec: &uo_testkit::capture::Record, side: &BTreeMap<String, Vec<String>>, t: &mut Tally) {
    let Some(wb_text) = side.get("wb").and_then(|v| v.first()) else { return };
    if rec.get("engine").is_some() {
        return;
    }
    let wb = parse(wb_text);
    let ops = key.starts_with("ops/");
    let ignore = rec.get("opts") == Some("ignoreCache");
    let (mut store, input) = build(&wb, None);
    let engine = Engine::new(&mut store, &input, &opts(ignore), None).unwrap();
    t.workbooks += 1;
    let legacy: Vec<Cell> = match rec.get("legacy") {
        Some("-") | None => Vec::new(),
        Some(l) => l.split(' ').map(cell_of).collect(),
    };
    let rand = rand_cells(&wb);
    let mut b = Book { key, ops, store, engine, tainted: BTreeSet::new(), clock: Clock { now_serial: 45658.5 }, rand };
    let empty = Vec::new();
    let lines = |f: &str| side.get(f).unwrap_or(&empty).clone();

    step(&mut b, rec, side, &mut *t, "r", &legacy, &mut |b, rnd| {
        b.engine.recalc(&mut b.store, b.clock, rnd).unwrap();
    });
    // circular of the recalc
    // `circular` sorted as the capture's strings
    let mut circ: Vec<Cell> = b.engine.circular().iter().map(|&(s, r, c)| (s.0, r, c)).collect();
    circ.sort_by_key(|&(s, r, c)| format!("{s},{r},{c}"));
    let ts_circ: Vec<Cell> = match rec.get("r.circ") {
        Some("-") | None => Vec::new(),
        Some(l) => l.split(' ').map(cell_of).collect(),
    };
    let keep = |v: &[Cell]| -> Vec<Cell> { v.iter().copied().filter(|c| !b.tainted.contains(c)).collect() };
    if circ == ts_circ {
        t.circ_equal += 1;
    } else if keep(&circ) == keep(&ts_circ) && !b.ops {
        t.circ_deferred += 1;
    } else {
        t.circ_mismatch.push(format!("{key}: rust {circ:?} ts {ts_circ:?}"));
    }
    let ierr = b.engine.internal_errors().len();
    if (ierr == 0) != (rec.get("r.ierr") == Some("-")) {
        t.ierr_mismatch.push(format!("{key}: {ierr} / {:?}", rec.get("r.ierr")));
    }
    step(&mut b, rec, side, &mut *t, "c", &[], &mut |b, rnd| {
        b.engine.calculate(&mut b.store, b.clock, rnd).unwrap();
    });

    // the edits, and the final inputs (a JS Map: delete removes, a new key appends, an old key keeps its place)
    let mut finals: Vec<Vec<(u32, u32, J)>> = Vec::new();
    for sh in wb.get("sheets").unwrap().arr() {
        let mut m: Vec<(u32, u32, J)> = Vec::new();
        let mut at: HashMap<(u32, u32), usize> = HashMap::new();
        for c in sh.get("cells").unwrap().arr() {
            let a = c.arr();
            let k = (a[0].num().unwrap() as u32, a[1].num().unwrap() as u32);
            match at.get(&k) {
                Some(&i) => m[i].2 = a[2].clone(),
                None => {
                    at.insert(k, m.len());
                    m.push((k.0, k.1, a[2].clone()));
                }
            }
        }
        finals.push(m);
    }
    for k in 0.. {
        let Some(desc) = rec.get(&format!("i{k}")) else { break };
        let desc = parse(desc).js().unwrap().as_js().to_string_lossy();
        let (verb, rest) = desc.split_once(' ').unwrap();
        let (pos, json) = rest.split_once(' ').unwrap_or((rest, ""));
        let (s, r, c) = cell_of(pos);
        let inp = (verb == "set").then(|| parse(json));
        let fm = &mut finals[s as usize];
        let i = fm.iter().position(|x| (x.0, x.1) == (r, c));
        match (&inp, i) {
            (None, Some(i)) => {
                fm.remove(i);
            }
            (Some(j), Some(i)) => fm[i].2 = j.clone(),
            (Some(j), None) => fm.push((r, c, j.clone())),
            (None, None) => {}
        }
        let mut extra = Vec::new();
        let input = inp.as_ref().map(|j| match j.get("f") {
            Some(f) => {
                let text = f.get("text").unwrap().js().unwrap();
                if text.as_js().to_string_lossy().to_uppercase().contains("RAND") {
                    b.rand.insert((s, r, c));
                }
                let array = f.get("t").and_then(J::js).is_some_and(|x| x.as_js().eq_str("array"));
                if !array && maybe_legacy(&text.as_js().to_string_lossy()) {
                    extra.push((s, r, c));
                }
                CellInput::Formula(FormulaInput {
                    text,
                    array,
                    dynamic: f.get("dynamic").is_some_and(J::bool),
                    r#ref: f.get("ref").and_then(J::js),
                    ca: f.get("ca").is_some_and(J::bool),
                })
            }
            None => match calcwb::value_of(j.get("v")) {
                calcwb::V::Str(t) => CellInput::Text(t),
                calcwb::V::Num(x) => CellInput::Value(Scalar::Num(x)),
                calcwb::V::Bool(x) => CellInput::Value(Scalar::Bool(x)),
                calcwb::V::Err(e) => CellInput::Value(Scalar::Err(e)),
                calcwb::V::Null => CellInput::Value(Scalar::Empty),
            },
        });
        step(&mut b, rec, side, &mut *t, &format!("i{k}"), &extra, &mut |b, rnd| {
            b.engine.set_cell(&mut b.store, SheetIx(s), r, c, input.clone()).unwrap();
            b.engine.calculate(&mut b.store, b.clock, rnd).unwrap();
        });
    }
    let inc = calcwb::values(&b.engine, &b.store);
    // a fresh engine over the final inputs
    let (mut store2, input2) = build(&wb, Some(&finals));
    let mut e2 = Engine::new(&mut store2, &input2, &opts(ignore), None).unwrap();
    let ts_rnd: Vec<f64> = lines("full.rnd").iter().map(|h| hexf(h)).collect();
    e2.recalc(&mut store2, b.clock, &mut RandomSource::sequence(ts_rnd)).unwrap();
    let mut b2 = Book { key, ops, store: store2, engine: e2, tainted: b.tainted.clone(), clock: b.clock, rand: b.rand.clone() };
    let mut leg2: Vec<Cell> = legacy.clone();
    if !b2.engine.not_ported().is_empty() {
        // a body that did not run in Rust may have consumed random values in the TS: the stream is deferred
        leg2.extend(b2.rand.iter().copied());
    }
    b2.taint(&leg2);
    compare_vals(&mut b2, "full", &lines("full.vals"), rec.get("full"), t);
    let full = calcwb::values(&b2.engine, &b2.store);
    // incremental = full (the Rust engine against itself), against the TS's own verdict
    let fm: HashMap<&str, &str> = full.iter().map(|l| (&l[..l.find('=').unwrap()], l.as_str())).collect();
    let im: BTreeSet<&str> = inc.iter().map(|l| &l[..l.find('=').unwrap()]).collect();
    let diff = inc.iter().filter(|l| fm.get(&l[..l.find('=').unwrap()]) != Some(&l.as_str())).count()
        + full.iter().filter(|l| !im.contains(&l[..l.find('=').unwrap()])).count();
    let ours = if diff == 0 { "equal".to_owned() } else { format!("differ:{diff}") };
    if Some(ours.as_str()) == rec.get("incfull") {
        t.incfull_equal += 1;
    } else if !b.tainted.is_empty() || !b2.tainted.is_empty() {
        t.incfull_deferred += 1;
    } else {
        t.incfull_mismatch.push(format!("{key}: rust {ours} ts {:?}", rec.get("incfull")));
    }
}

type StepFn<'f> = &'f mut dyn FnMut(&mut Book<'_>, &mut RandomSource);

/// One run: the operation, then its order, random values and values against the capture.
fn step(
    b: &mut Book<'_>,
    rec: &uo_testkit::capture::Record,
    side: &BTreeMap<String, Vec<String>>,
    t: &mut Tally,
    p: &str,
    extra: &[Cell],
    f: StepFn<'_>,
) {
    let empty = Vec::new();
    let lines = |f: &str| side.get(f).unwrap_or(&empty);
    t.runs += 1;
    let ts_rnd: Vec<f64> = lines(&format!("{p}.rnd")).iter().map(|h| hexf(h)).collect();
    let mut rnd = RandomSource::sequence(ts_rnd.clone());
    let _ = b.engine.take_trace();
    f(b, &mut rnd);
    let ord: Vec<String> = b.engine.take_trace().iter().map(|(s, r, c)| format!("{s},{r},{c}")).collect();
    t.evaluations += ord.len();
    let had_np = !b.engine.not_ported().is_empty();
    let ord_ok = list_digest(&ord) == rec.get(&format!("{p}.ord")).unwrap_or("");
    let mut extra = extra.to_vec();
    if !ord_ok && had_np {
        // another order: the cells two formulas write (orphaned duplicates) may end with the other's value
        extra.extend(b.engine.orphans().iter().map(|&(s, r, c)| (s.0, r, c)));
    }
    if had_np && !(ord_ok && rnd.consumed() == ts_rnd.len()) {
        // a body that did not run in Rust (or ran in another order) shifted the random stream: every random cell of
        // the run is deferred (A4a; before A4 the random functions themselves were not ported)
        extra.extend(b.rand.iter().copied());
    }
    b.taint(&extra);
    // the order without the deferred cells (a tainted anchor's #SPILL! decides whether an edit dirties it)
    let untainted = |v: &[String]| -> Vec<String> { v.iter().filter(|x| !b.tainted.contains(&cell_of(x))).cloned().collect() };
    let same_rest = !ord_ok && !lines(&format!("{p}.ord")).is_empty() && untainted(&ord) == untainted(lines(&format!("{p}.ord")));
    if ord_ok {
        t.ord_equal += 1;
    } else if (had_np || same_rest) && !b.ops {
        t.ord_deferred += 1;
    } else {
        t.ord_mismatch.push(format!("{}:{p} ({} rust / {})", b.key, ord.len(), rec.get(&format!("{p}.ord")).unwrap_or("?")));
    }
    let used = rnd.consumed();
    if used == ts_rnd.len() {
        t.rnd_equal += 1;
    } else if had_np {
        t.rnd_deferred += 1;
    } else {
        t.rnd_mismatch.push(format!("{}:{p} {used} / {}", b.key, ts_rnd.len()));
    }
    compare_vals(b, p, lines(&format!("{p}.vals")), rec.get(&format!("{p}.vals")), t);
}

fn compare_vals(b: &mut Book<'_>, p: &str, ts: &[String], ts_digest: Option<&str>, t: &mut Tally) {
    let ours = calcwb::values(&b.engine, &b.store);
    if ts.is_empty() {
        // a big workbook: the digest only
        if b.tainted.is_empty() && Some(list_digest(&ours).as_str()) != ts_digest {
            t.vals_mismatch.push(format!("{}:{p} digest", b.key));
        }
        return;
    }
    let tsm: BTreeMap<Cell, &String> = ts.iter().map(|l| (key_of(l), l)).collect();
    let om: BTreeMap<Cell, &String> = ours.iter().map(|l| (key_of(l), l)).collect();
    // cells on one side only: a spill footprint that differs (a body-less anchor) → their readers are deferred too
    let one_side: Vec<Cell> =
        tsm.keys().filter(|k| !om.contains_key(k)).chain(om.keys().filter(|k| !tsm.contains_key(k))).copied().collect();
    let unexplained: Vec<Cell> = one_side.iter().copied().filter(|c| !b.tainted.contains(c)).collect();
    if !unexplained.is_empty() && !b.ops {
        // only explained when a tainted anchor could own them: taint them and their readers
        b.taint(&unexplained);
    }
    for (k, l) in &tsm {
        if b.tainted.contains(k) {
            t.vals_deferred += 1;
            continue;
        }
        t.vals_compared += 1;
        if om.get(k) != Some(l) {
            if t.vals_mismatch.len() < 400 {
                t.vals_mismatch.push(format!("{}:{p} ts {l} rust {:?}", b.key, om.get(k)));
            } else {
                t.vals_mismatch.push(String::new());
            }
        }
    }
    for (k, l) in &om {
        if !tsm.contains_key(k) && !b.tainted.contains(k) {
            t.vals_mismatch.push(format!("{}:{p} rust only {l}", b.key));
        }
    }
}

fn replay(cap: &Capture, side: &BTreeMap<String, BTreeMap<String, Vec<String>>>) -> Tally {
    let mut t = Tally::default();
    let empty = BTreeMap::new();
    for rec in cap.records() {
        run_book(&rec.key, rec, side.get(&rec.key).unwrap_or(&empty), &mut t);
    }
    t
}

fn report(t: &Tally) -> String {
    format!(
        "workbooks {} runs {} evaluations {} | ord equal {} deferred {} mismatch {} | rnd equal {} deferred {} mismatch {} | circ equal {} deferred {} mismatch {} | vals compared {} deferred {} mismatch {} | incfull equal {} deferred {} mismatch {} | ierr mismatch {}",
        t.workbooks,
        t.runs,
        t.evaluations,
        t.ord_equal,
        t.ord_deferred,
        t.ord_mismatch.len(),
        t.rnd_equal,
        t.rnd_deferred,
        t.rnd_mismatch.len(),
        t.circ_equal,
        t.circ_deferred,
        t.circ_mismatch.len(),
        t.vals_compared,
        t.vals_deferred,
        t.vals_mismatch.len(),
        t.incfull_equal,
        t.incfull_deferred,
        t.incfull_mismatch.len(),
        t.ierr_mismatch.len()
    )
}

fn check(path: &Path, filter: impl Fn(&str) -> bool) -> Tally {
    let cap = Capture::read(path).unwrap().subset(|k| filter(k));
    let side = sidecar(&path.with_extension("side.gz"));
    let t = replay(&cap, &side);
    eprintln!("{}: {}", path.display(), report(&t));
    for m in
        t.ord_mismatch.iter().chain(&t.rnd_mismatch).chain(&t.circ_mismatch).chain(&t.incfull_mismatch).chain(&t.ierr_mismatch).take(30)
    {
        eprintln!("  {m}");
    }
    for m in t.vals_mismatch.iter().filter(|m| !m.is_empty()).take(30) {
        eprintln!("  {m}");
    }
    t
}

fn ci_path() -> PathBuf {
    support::captures_dir().join("calc-order.capture")
}

fn clean(t: &Tally) -> bool {
    t.ord_mismatch.is_empty()
        && t.rnd_mismatch.is_empty()
        && t.circ_mismatch.is_empty()
        && t.vals_mismatch.is_empty()
        && t.incfull_mismatch.is_empty()
        && t.ierr_mismatch.is_empty()
}

/// Operator-only workbooks: nothing deferred but the legacy cells, everything equal.
#[test]
fn calc_order_ops() {
    let t = check(&ci_path(), |k| k.starts_with("ops/"));
    assert!(t.workbooks >= 40);
    assert!(clean(&t), "{}", report(&t));
    assert_eq!(t.ord_deferred + t.rnd_deferred + t.circ_deferred, 0);
}

/// The generated x1 workbooks: order, random values, cycles and values equal wherever no function body (A4) or
/// legacy `@` (A5) is involved.
#[test]
fn calc_order_x1() {
    let t = check(&ci_path(), |k| k.starts_with("gen/"));
    assert!(t.workbooks >= 30);
    assert!(clean(&t), "{}", report(&t));
}

/// The local big capture (`UO_A3_BIG=1`).
#[test]
fn calc_order_big() {
    if !uo_testkit::opt_in("UO_A3_BIG", "the big calc-order capture") {
        return;
    }
    let p = support::captures_dir().join("../../../../target/sheet-cap/calc-order-big.capture");
    let t = check(&p, |_| true);
    assert!(clean(&t), "{}", report(&t));
}

/// Debug aid: `UO_A3_KEY=ops/0000 cargo test -p uo-calc --test a3 debug_key -- --nocapture` prints the first run's
/// order against the TS with the formula texts.
#[test]
fn debug_key() {
    let Ok(key) = std::env::var("UO_A3_KEY") else { return };
    let path = std::env::var("UO_A3_CAP").map_or_else(|_| ci_path(), PathBuf::from);
    let cap = Capture::read(&path).unwrap();
    let side = sidecar(&path.with_extension("side.gz"));
    let rec = cap.record(&key).unwrap();
    let sd = &side[&key];
    let wb = parse(&sd["wb"][0]);
    let mut texts: HashMap<Cell, String> = HashMap::new();
    for (si, sh) in wb.get("sheets").unwrap().arr().iter().enumerate() {
        for c in sh.get("cells").unwrap().arr() {
            let a = c.arr();
            let k = (si as u32, a[0].num().unwrap() as u32, a[1].num().unwrap() as u32);
            let txt = |j: Option<&J>| j.and_then(J::js).map(|x| x.as_js().to_string_lossy()).unwrap_or_default();
            let f = a[2].get("f").map_or_else(
                || format!("v {:?}", a[2].get("v")),
                |f| {
                    format!(
                        "={} t={} ref={} si={:?} dyn={}",
                        txt(f.get("text")),
                        txt(f.get("t")),
                        txt(f.get("ref")),
                        f.get("si"),
                        f.get("dynamic").is_some()
                    )
                },
            );
            texts.entry(k).and_modify(|x| x.push_str(&format!(" || {f}"))).or_insert(f);
        }
    }
    let ignore = rec.get("opts") == Some("ignoreCache");
    let (mut store, input) = build(&wb, None);
    let mut e = Engine::new(&mut store, &input, &opts(ignore), None).unwrap();
    e.recalc(&mut store, Clock { now_serial: 45658.5 }, &mut RandomSource::sequence(Vec::new())).unwrap();
    let step = std::env::var("UO_A3_STEP").unwrap_or_else(|_| "r".into());
    if step == "c" {
        let _ = e.take_trace();
        e.calculate(&mut store, Clock { now_serial: 45658.5 }, &mut RandomSource::sequence(Vec::new())).unwrap();
    }
    let ord: Vec<String> = e.take_trace().iter().map(|(s, r, c)| format!("{s},{r},{c}")).collect();
    let ts = &sd[&format!("{step}.ord")];
    let n = ord.len().max(ts.len());
    for i in 0..n {
        let a = ord.get(i).map_or("", String::as_str);
        let b = ts.get(i).map_or("", String::as_str);
        let k = if b.is_empty() { a } else { b };
        eprintln!("{} {a:>10} {b:>10}  {}", if a == b { ' ' } else { '*' }, texts.get(&cell_of(k)).map_or("", String::as_str));
    }
    let vals = calcwb::values(&e, &store);
    let tsv: BTreeSet<&String> = sd[&format!("{step}.vals")].iter().collect();
    for v in &vals {
        if !tsv.contains(v) {
            eprintln!("rust {v}   ts {:?}", sd[&format!("{step}.vals")].iter().find(|l| key_of(l) == key_of(v)));
        }
    }
    eprintln!("circ rust {:?} ts {:?}", e.circular(), rec.get("r.circ"));
    if let Ok(c) = std::env::var("UO_A3_CELL") {
        let (s, r, cc) = cell_of(&c);
        eprintln!("deps of {c}: {:?} text {:?}", e.precedents_of(&mut store, SheetIx(s), r, cc), texts.get(&(s, r, cc)));
    }
}
