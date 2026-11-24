//! C1b parity: the surgical save of every committed capture workbook (`sheet-x1`, `sheet-samples`) against the
//! frozen TS `saveXlsx`: after the open (open-recalc applied, the sheets it wrote are the dirty ones, as the TS's
//! `touched` hook marks them) every part of the output, decompressed, must equal the TS's (name, length, SHA-256
//! prefix, in archive order), and the set of parts that differ from the input must be the same.
//!
//! The scripted edit steps of the captures (`e<k>.save`) that this crate can reproduce with the APIs that exist
//! are replayed by `replay_edits` below; the rest need the C5 session and are counted as deferred.
//!
//! Run: `cargo test -p uo-xlsx --features save,facet --test save_parity` (the giants of `sheet-samples` only with
//! `UO_XLSX_BIG=1`, in release).
#![cfg(all(feature = "save", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::engine::EngineOptions;
use uo_calc::{Clock, JsString, RandomSource, ValueLocale};
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::save::engine::open_recalc;
use uo_xlsx::save::{SaveRequest, save};

#[path = "save_parity/edits.rs"]
mod edits;

type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap();
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap();
    String::from_utf8(raw).unwrap()
}

fn sidecar(path: &Path) -> Side {
    let mut out: Side = BTreeMap::new();
    for line in gunzip(path).lines() {
        let mut it = line.splitn(3, '\t');
        if let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next()) {
            out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
        }
    }
    out
}

fn list_dig(lines: &[String]) -> String {
    let mut h = uo_testkit::sha256::Sha256::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            h.update(b"\n");
        }
        h.update(l.as_bytes());
    }
    format!("{}:{}", lines.len(), &uo_testkit::stream::hex(&h.finish())[..32])
}

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

/// `name length sha256[..32]` per part, in archive order, and the names of the parts that differ from `base`.
fn parts_of(out: &[u8], base: &BTreeMap<String, Vec<u8>>) -> (Vec<String>, Vec<String>, BTreeMap<String, Vec<u8>>) {
    let pkg = uo_opc::Package::open(out, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut lines = Vec::new();
    let mut changed = Vec::new();
    let mut data = BTreeMap::new();
    for n in names {
        let b = pkg.part(&n).unwrap().unwrap().to_vec();
        let h = uo_testkit::sha256::sha256(&b);
        lines.push(format!("{n} {} {}", b.len(), &uo_testkit::stream::hex(&h)[..32]));
        if base.get(&n).is_none_or(|x| *x != b) {
            changed.push(n.clone());
        }
        data.insert(n, b);
    }
    (lines, changed, data)
}

fn base_of(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.part_names()
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|n| (n.clone(), pkg.part(&n).unwrap().unwrap().to_vec()))
        .collect()
}

/// The TS's `Math.random` of one workbook: xorshift32 seeded from `sha256(key)`, `/ 2^32`.
fn random_stream(key: &str) -> Vec<f64> {
    let h = uo_testkit::sha256::sha256(key.as_bytes());
    let seed = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
    let mut g = uo_testkit::generate::Rng::new(if seed == 0 { 1 } else { seed });
    (0..4096).map(|_| f64::from(g.next_u32()) / 4_294_967_296.0).collect()
}

/// The TS's stream, from where the open consumed it: the capture records the digest of the values the open drew
/// (`rnd = <count>:<digest>`); anything the element drew before the calc shifts the stream, so the offset is
/// found by matching the digest.
fn random_for(key: &str, rnd_facet: Option<&str>) -> RandomSource {
    let all = random_stream(key);
    let mut skip = 0;
    if let Some((n, _)) = rnd_facet.and_then(|f| f.split_once(':')).and_then(|(n, d)| Some((n.parse::<usize>().ok()?, d)))
        && n > 0
    {
        for k in 0..64usize {
            let lines: Vec<String> = all[k..k + n].iter().map(|x| format!("{:016x}", x.to_bits())).collect();
            if Some(list_dig(&lines).as_str()) == rnd_facet {
                skip = k;
                break;
            }
        }
    }
    RandomSource::sequence(all[skip..].to_vec())
}

fn engine_opts() -> EngineOptions {
    let mut o = EngineOptions::new(ValueLocale::pt_br());
    o.file_name = Some(JsString::from_str_js("Pasta1.xlsx"));
    o
}

/// Opens as the component does: load, open-recalc; returns the book and the dirty sheets (the ones the calc wrote).
fn open_like_component(key: &str, rnd_facet: Option<&str>, bytes: &[u8]) -> Option<(Book, Vec<usize>, RandomSource, String)> {
    let mut book = Book::open(bytes, ReadLimits::default()).ok()?;
    book.load_all().ok()?;
    let mut rnd = random_for(key, rnd_facet);
    let (plan, changes) = open_recalc(&mut book, &engine_opts(), Clock { now_serial: 45672.5 }, &mut rnd).ok()?;
    let dirty = changes.sheets.iter().map(|(s, ..)| s.0 as usize).collect();
    let mode = match plan.mode {
        uo_calc::open_recalc::OpenRecalcMode::None => "none".to_owned(),
        uo_calc::open_recalc::OpenRecalcMode::Full => "full".to_owned(),
        uo_calc::open_recalc::OpenRecalcMode::Partial => format!("partial:{}", plan.cells.len()),
    };
    Some((book, dirty, rnd, mode))
}

/// A cell whose `r` names another row than its `<row>` (SH1): the TS engine places it by the ref, the store (like the
/// component's index) by the element, so the open-recalc differs and the workbook is deferred to the SH1 decision.
fn has_sh1(book: &Book) -> bool {
    book.store.sheets.iter().any(|sh| {
        sh.extras.iter().any(|(k, e)| {
            e.raw_ref.as_ref().is_some_and(|r| {
                let text = r.as_js().to_string_lossy();
                uo_xlsx::read::ref_to_row_col(text.as_bytes()).is_some_and(|(_, row)| row - 1.0 != (*k / 16384) as f64)
            })
        })
    })
}

struct Totals {
    edits_compared: usize,
    edits_equal: usize,
    edits_sh1: usize,
    edits_deferred: usize,
    sh1: Vec<String>,
    workbooks: usize,
    compared: usize,
    equal: usize,
    mismatches: Vec<String>,
    skipped_err: usize,
    pv_compared: usize,
    pv_mismatches: Vec<String>,
}

fn replay(name: &str, big: bool, t: &mut Totals) {
    let cap = Capture::read(dir().join(format!("{name}.capture"))).unwrap();
    let side = sidecar(&dir().join(format!("{name}.side.gz")));
    let mut rep = Replay::new(&cap);
    for rec in cap.records() {
        let key = rec.key.as_str();
        if !big && (key.ends_with("giant-1m.xlsx") || key.ends_with("giant-200k.xlsx")) {
            continue;
        }
        let Some(bytes) = bytes_of(key) else { continue };
        let Some(want) = rec.get("save0") else { continue };
        if want.starts_with("err:") || rec.get("read").is_some_and(|r| r != "ok") {
            t.skipped_err += 1;
            continue;
        }
        t.workbooks += 1;
        let base = base_of(&bytes);
        let Some((mut book, dirty, _rnd, mode)) = open_like_component(key, rec.get("rnd"), &bytes) else {
            t.mismatches.push(format!("{key}: open failed"));
            continue;
        };
        if let Some(p) = rec.get("plan")
            && p != mode
        {
            t.mismatches.push(format!("{key}: plan {mode}, ts {p}"));
        }
        // the store after the open (the model the component holds): the part of the open this replay depends on
        let mut n = 0usize;
        let mut pv_bad = false;
        for i in 0..book.workbook.sheets.len() {
            if book.workbook.sheets[i].target.is_none() {
                continue;
            }
            let ix = uo_calc::key::SheetIx(i as u32);
            let rp = &book.run_props;
            let props = |id: u32| rp.get(id).map(uo_xlsx::read::facet::run_props_json).unwrap_or_default();
            let st = uo_calc::facet::store_facet(&book.store, ix, &props);
            if let Some(want) = rec.get(&format!("pv{n}")) {
                t.pv_compared += 1;
                if list_dig(&st) != want {
                    t.pv_mismatches.push(format!("{key} pv{n}"));
                    pv_bad = true;
                    let ts = side.get(key).and_then(|m| m.get(&format!("pv{n}"))).cloned().unwrap_or_default();
                    if let Some(k) = st.iter().zip(ts.iter()).position(|(a, b)| a != b) {
                        eprintln!(
                            "{key} pv{n} line {k}:
  ts:   {}
  rust: {}",
                            ts[k], st[k]
                        );
                    }
                }
            }
            n += 1;
        }
        if pv_bad && !has_sh1(&book) {
            t.mismatches.push(format!("{key}: store after the open differs"));
        }
        let saved = match save(&mut book, &SaveRequest { dirty_sheets: &dirty, ..SaveRequest::default() }) {
            Ok(s) => s,
            Err(e) => {
                t.mismatches.push(format!("{key}: save failed: {e}"));
                continue;
            }
        };
        let (lines, changed, data) = parts_of(&saved.bytes, &base);
        t.compared += 1;
        let ok1 = rep.check(key, "save0", &list_dig(&lines));
        let want_chg = rec.get("chg0").unwrap_or("-");
        let ours_chg = if changed.is_empty() { "-".to_owned() } else { changed.join(" ") };
        let ok2 = want_chg == ours_chg;
        if ok1 && ok2 {
            t.equal += 1;
            if !key.starts_with("samples/") {
                edits::replay_edits(rec, key, &base, &mut book, dirty.clone(), t);
            }
            continue;
        }
        if has_sh1(&book) {
            t.sh1.push(key.to_owned());
            continue;
        }
        t.mismatches.push(format!("{key}: save0 {} chg0 {}", if ok1 { "ok" } else { "DIFF" }, if ok2 { "ok" } else { "DIFF" }));
        // which part differs
        let ts_lines = side.get(key).and_then(|m| m.get("save0")).cloned().unwrap_or_default();
        for (a, b) in lines.iter().zip(ts_lines.iter()) {
            if a != b {
                let part = a.split(' ').next().unwrap_or("");
                eprintln!("{key}: part differs\n  ts:   {b}\n  rust: {a}");
                if let (Some(xml), Some(ts)) =
                    (data.get(part), side.get(key).and_then(|m| m.get(&format!("save0:{part}"))).and_then(|v| v.first()))
                {
                    let ours = String::from_utf8_lossy(xml).into_owned();
                    let ts: String = serde_like_unquote(ts);
                    let at = ours.bytes().zip(ts.bytes()).position(|(x, y)| x != y).unwrap_or(ours.len().min(ts.len()));
                    let from = at.saturating_sub(80);
                    eprintln!("    at byte {at}\n    ts:   ...{}\n    rust: ...{}", cut(&ts, from, 200), cut(&ours, from, 200));
                }
                break;
            }
        }
        if ts_lines.len() != lines.len() {
            eprintln!("{key}: {} parts here, {} in the TS", lines.len(), ts_lines.len());
        }
    }
    let _ = rep.finish();
}

fn cut(s: &str, from: usize, n: usize) -> String {
    let mut a = from.min(s.len());
    while !s.is_char_boundary(a) {
        a -= 1;
    }
    let mut b = (from + n).min(s.len());
    while !s.is_char_boundary(b) {
        b += 1;
    }
    s[a..b].to_owned()
}

/// The sidecar stores `JSON.stringify(text)`: undo the quoting.
fn serde_like_unquote(j: &str) -> String {
    let b = uo_jsnum::text::trim(j);
    let b = b.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(b);
    let mut out = String::with_capacity(b.len());
    let mut it = b.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                let mut cp = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
                if (0xD800..0xDC00).contains(&cp) {
                    // a surrogate pair
                    let rest: String = it.clone().take(6).collect();
                    if let Some(lo) = rest.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok()) {
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        for _ in 0..6 {
                            it.next();
                        }
                    }
                }
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn report(name: &str, t: &Totals) {
    eprintln!(
        "  edit steps: {} compared, {} equal, {} on SH1 workbooks, {} deferred to C5 (from the first step that needs the session)",
        t.edits_compared, t.edits_equal, t.edits_sh1, t.edits_deferred
    );
    eprintln!(
        "{name}: {} workbooks, {} saves compared, {} equal, {} deferred to SH1 {:?}, {} mismatches, {} skipped (TS error)",
        t.workbooks,
        t.compared,
        t.equal,
        t.sh1.len(),
        t.sh1,
        t.mismatches.len(),
        t.skipped_err
    );
    eprintln!("  store after the open: {} sheets compared, {} mismatches {:?}", t.pv_compared, t.pv_mismatches.len(), t.pv_mismatches);
    for m in t.mismatches.iter().take(20) {
        eprintln!("  {m}");
    }
}

#[test]
fn x1_noop_save() {
    let mut t = Totals {
        edits_compared: 0,
        edits_equal: 0,
        edits_sh1: 0,
        edits_deferred: 0,
        sh1: Vec::new(),
        workbooks: 0,
        compared: 0,
        equal: 0,
        mismatches: Vec::new(),
        skipped_err: 0,
        pv_compared: 0,
        pv_mismatches: Vec::new(),
    };
    replay("sheet-x1", false, &mut t);
    report("x1", &t);
    assert!(t.workbooks >= 30);
    assert!(t.mismatches.is_empty(), "{:?}", t.mismatches);
}

#[test]
fn samples_noop_save() {
    let big = uo_testkit::env_flag("UO_XLSX_BIG");
    let mut t = Totals {
        edits_compared: 0,
        edits_equal: 0,
        edits_sh1: 0,
        edits_deferred: 0,
        sh1: Vec::new(),
        workbooks: 0,
        compared: 0,
        equal: 0,
        mismatches: Vec::new(),
        skipped_err: 0,
        pv_compared: 0,
        pv_mismatches: Vec::new(),
    };
    replay("sheet-samples", big, &mut t);
    report("samples", &t);
    assert!(t.workbooks >= 5);
    assert!(t.mismatches.is_empty(), "{:?}", t.mismatches);
}
