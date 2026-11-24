//! S0 parity: the store facet replayed against the frozen TS (record `uo-calc.md` "Parity").
//!
//! `store.capture` (`js/capture-store.mjs`): every committed sample (giants in the local run) and every
//! generated x1 workbook, streamed into the store in FILE order through the test-only loader (S0 has no xlsx
//! reader), then `st<i>` (the store facet), `fo<i>` (file order), `rw<i>` (row attributes) and `cv<i>`
//! (`cellValue` of every cell, shared strings decoded on demand) compared with the TS; the same `st`/`fo`/`rw`
//! digests are also checked against uo-xlsx's `sheet-*.capture` (made by `capture-sheet.mjs`, an independent
//! adapter), and the `pv<i>` facet (the model after the open's write-back) is replayed from its sidecar lines.
//! The `traps` record replays `numberLiteral`/`numberText`/`fileCellValue`/`errorOf`/`cellValue` over the
//! per-parameter traps.
//!
//! Local run (giants + x1-big): `node --max-old-space-size=8192 core/crates/uo-calc/js/capture-store.mjs --big
//! --out core/target/sheet-cap/store-big.capture`, then `UO_STORE_BIG=1 cargo test -p uo-calc --test parity`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::Path;

use support::{J, Props, list_digest, load_sheet, parse, sidecar, sst_of};
use uo_calc::cell::{CellWrite, FileCell, error_of, file_cell_value, number_literal, number_text};
use uo_calc::facet::{file_order, row_meta_lines, store_facet, value_text};
use uo_calc::key::{Area, MAX_COL, MAX_ROW};
use uo_calc::store::SstDecoded;
use uo_calc::{JsString, Scalar, SheetIx, Store, StrRef};
use uo_testkit::{Capture, Replay, Report};

/// A sidecar: key → facet → lines.
type Side = std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>;

fn fo_text(seq: &[(u32, u32)]) -> String {
    let sorted = seq.windows(2).all(|w| u64::from(w[0].0) * 20000 + u64::from(w[0].1) < u64::from(w[1].0) * 20000 + u64::from(w[1].1));
    if sorted { "sorted".to_owned() } else { list_digest(&seq.iter().map(|(r, c)| format!("{r}:{c}")).collect::<Vec<_>>()) }
}

struct Totals {
    workbooks: usize,
    sheets: usize,
    cells: u64,
    lines: usize,
}

/// Replays one store capture; `xlsx` are the uo-xlsx sheet captures for the cross-check.
fn replay(cap_path: &Path, xlsx: &[(Capture, Side)]) -> (Report, Report, Totals) {
    let cap = Capture::read(cap_path).unwrap();
    let side = sidecar(&cap_path.with_extension("side.gz"));
    let mut rep = Replay::new(&cap);
    let mut cross_caps: Vec<Replay<'_>> = xlsx.iter().map(|(c, _)| Replay::new(c)).collect();
    let mut t = Totals { workbooks: 0, sheets: 0, cells: 0, lines: 0 };
    let empty = std::collections::BTreeMap::new();
    for rec in cap.records() {
        let key = rec.key.as_str();
        if key == "traps" {
            replay_traps(&mut rep, side.get(key).unwrap_or(&empty));
            continue;
        }
        let sx = side.get(key).unwrap_or(&empty);
        let none: Vec<String> = Vec::new();
        let lines = |f: &str| sx.get(f).unwrap_or(&none);
        t.workbooks += 1;
        let mut store = Store::new();
        store.date1904 = rec.get("date1904") == Some("1");
        let mut sst = sst_of(lines("sst"));
        store.set_sst_len(sst.0.len() as u32);
        let mut props = Props::default();
        for f in ["input", "date1904", "sst", "sheets", "read"] {
            rep.skip(key, f);
        }
        let sheets: usize = rec.get("sheets").and_then(|s| s.parse().ok()).unwrap_or(0);
        for i in 0..sheets {
            let inf = format!("in{i}");
            rep.skip(key, &inf);
            if rec.get(&inf).is_some_and(|v| v.starts_with("err:")) {
                continue;
            }
            let sheet = load_sheet(&mut store, lines(&inf), &mut props);
            t.sheets += 1;
            let pt = |id: u32| props.text(id);
            let st = store_facet(&store, sheet, &pt);
            t.cells += st.len() as u64;
            t.lines += st.len();
            let want = lines(&format!("st{i}"));
            if let Some(k) = st.iter().zip(want.iter()).position(|(a, b)| a != b) {
                eprintln!("{key} st{i} line {k}:\n  ts:   {}\n  rust: {}", want[k], st[k]);
            }
            let st_d = list_digest(&st);
            rep.check(key, &format!("st{i}"), &st_d);
            let fo = fo_text(&file_order(&store, sheet));
            rep.check(key, &format!("fo{i}"), &fo);
            let rw = row_meta_lines(&store, sheet);
            if rw.iter().zip(lines(&format!("rw{i}"))).any(|(a, b)| a != b) {
                eprintln!("{key} rw{i}: {rw:?}\n  ts {:?}", lines(&format!("rw{i}")));
            }
            let rw_d = list_digest(&rw);
            rep.check(key, &format!("rw{i}"), &rw_d);
            // cellValue of every cell (shared strings decoded on demand through the lazy source)
            let mut keys = Vec::new();
            store.each(Area { sheet, r1: 0, c1: 0, r2: MAX_ROW, c2: MAX_COL }, |r, c, _| keys.push((r, c)));
            let cv: Vec<String> = keys
                .iter()
                .map(|&(r, c)| {
                    let v = store.cell_value_decoding(sheet, r, c, &mut sst).unwrap();
                    format!("{r},{c}={}", value_text(&store.strings, &v))
                })
                .collect();
            let want = lines(&format!("cv{i}"));
            if let Some(k) = cv.iter().zip(want.iter()).position(|(a, b)| a != b) {
                eprintln!("{key} cv{i} line {k}:\n  ts:   {}\n  rust: {}", want[k], cv[k]);
            }
            rep.check(key, &format!("cv{i}"), &list_digest(&cv));
            // the same facets made by capture-sheet.mjs (uo-xlsx), when that capture has the workbook
            for (x, (xc, _)) in cross_caps.iter_mut().zip(xlsx) {
                if xc.record(key).is_some() {
                    x.check(key, &format!("st{i}"), &st_d);
                    x.check(key, &format!("fo{i}"), &fo);
                    x.check(key, &format!("rw{i}"), &rw_d);
                }
            }
        }
        store.check_invariants().unwrap_or_else(|e| panic!("{key}: {e}"));
    }
    // pv<i> of the uo-xlsx captures: the model after the open (write-back applied), from its sorted lines
    let mut cross = Report::default();
    for (x, (xc, xside)) in cross_caps.into_iter().zip(xlsx) {
        let mut x = x;
        for rec in xc.records() {
            for (f, v) in &rec.facets {
                let Some(i) = f.strip_prefix("pv") else { continue };
                let Some(lines) = xside.get(&rec.key).and_then(|s| s.get(f.as_str())) else { continue };
                let mut store = Store::new();
                let sheet = store.add_sheet().unwrap();
                let mut props = Props::default();
                for l in lines {
                    let j = parse(l);
                    let (r, c) = (j.get("r").and_then(J::num).unwrap() as u32, j.get("c").and_then(J::num).unwrap() as u32);
                    let mut cell: FileCell = support::file_cell(&j, &mut props);
                    if cell.r#ref.is_none() {
                        cell.r#ref = Some(JsString::from_str_js(&uo_calc::cell::a1_ref(r - 1, c - 1)));
                    }
                    store.set_file_cell(sheet, r - 1, c - 1, &cell).unwrap();
                }
                let pt = |id: u32| props.text(id);
                let st = store_facet(&store, sheet, &pt);
                x.check(&rec.key, f, &list_digest(&st));
                let _ = (i, v);
                store.check_invariants().unwrap();
            }
        }
        let r = x.finish();
        cross.equal += r.equal;
        cross.mismatches.extend(r.mismatches);
    }
    (rep.finish(), cross, t)
}

fn js(s: &str) -> JsString {
    JsString::from_str_js(s)
}

/// The per-parameter traps (`capture-store.mjs` `traps()`).
fn replay_traps(rep: &mut Replay<'_>, side: &std::collections::BTreeMap<String, Vec<String>>) {
    let none = Vec::new();
    let want = side.get("traps").unwrap_or(&none);
    let mut got = Vec::new();
    // the trap SST: [{text:'zero'}, {runs:['r','uns']}, {}, {text:''}]
    let sst = ["zero", "runs", "", ""];
    for line in want {
        let (call, _) = line.rsplit_once('=').unwrap();
        let (f, args) = call.split_once(' ').unwrap();
        let j = parse(args);
        let result = match f {
            "numberLiteral" => uo_jsnum::json::string(&number_literal(j.num().unwrap())),
            "numberText" => uo_jsnum::json::string(&number_text(j.num().unwrap())),
            "fileCellValue" => {
                let mut store = Store::new();
                let v = match &j {
                    J::Null => Scalar::Empty,
                    J::Bool(b) => Scalar::Bool(*b),
                    J::Str(u) if String::from_utf16_lossy(u).starts_with("#err ") => {
                        Scalar::Err(error_of(JsString::from_utf16(&u[5..]).as_js()))
                    }
                    J::Str(u) => Scalar::Str(StrRef::Pooled(store.strings.intern(JsString::from_utf16(u).as_js()).unwrap())),
                    _ => Scalar::Num(j.num().unwrap()),
                };
                let fv = file_cell_value(&v, &store.strings, &|_| JsString::default());
                let mut s = String::from("{");
                if let Some(t) = fv.t {
                    s.push_str(&format!("\"type\":\"{t}\","));
                }
                s.push_str("\"value\":");
                uo_calc::facet::push_str(&mut s, fv.v.as_js());
                s.push('}');
                s
            }
            "errorOf" => format!("e{}", uo_calc::cell::error_text(error_of(j.js().unwrap().as_js()))),
            "cellValue" => {
                let mut store = Store::new();
                store.date1904 = j.get("d1904").is_some_and(J::bool);
                store.set_sst_len(4);
                let sheet: SheetIx = store.add_sheet().unwrap();
                let mut props = Props::default();
                let mut cell = support::file_cell(&j, &mut props);
                cell.r#ref = Some(js("A1"));
                store.set_file_cell(sheet, 0, 0, &cell).unwrap();
                let mut src = support::Sst(sst.iter().map(|s| js(s)).collect());
                let v = store.cell_value_decoding(sheet, 0, 0, &mut src).unwrap();
                store.check_invariants().unwrap();
                value_text(&store.strings, &v)
            }
            other => panic!("unknown trap {other}"),
        };
        got.push(format!("{call}={result}"));
    }
    for (a, b) in got.iter().zip(want) {
        if a != b {
            eprintln!("trap:\n  ts:   {b}\n  rust: {a}");
        }
    }
    rep.check("traps", "traps", &list_digest(&got));
    let _ = SstDecoded::default();
    let _ = CellWrite::Empty;
}

fn xlsx_caps() -> Vec<(Capture, Side)> {
    ["sheet-samples", "sheet-x1"]
        .iter()
        .map(|s| {
            let p = support::xlsx_captures_dir().join(format!("{s}.capture"));
            (Capture::read(&p).unwrap(), sidecar(&p.with_extension("side.gz")))
        })
        .collect()
}

#[test]
fn store_facet_ci() {
    let (rep, cross, t) = replay(&support::captures_dir().join("store.capture"), &xlsx_caps());
    eprintln!(
        "store.capture: {} workbooks, {} sheets, {} cells; {}; cross-check with sheet-*.capture: {} equal, {} mismatches",
        t.workbooks,
        t.sheets,
        t.cells,
        rep.summary(),
        cross.equal,
        cross.mismatches.len()
    );
    rep.assert_clean();
    assert!(cross.mismatches.is_empty(), "cross-check: {:?}", &cross.mismatches[..cross.mismatches.len().min(5)]);
    assert!(t.cells > 6000 && t.lines == t.cells as usize);
}

#[test]
fn store_facet_big() {
    if !uo_testkit::opt_in("UO_STORE_BIG", "store facet over the giants + x1-big (core/target/sheet-cap/store-big.capture)") {
        return;
    }
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/store-big.capture");
    let (rep, cross, t) = replay(&p, &xlsx_caps());
    eprintln!(
        "store-big: {} workbooks, {} sheets, {} cells; {}; cross {} equal, {} mismatches",
        t.workbooks,
        t.sheets,
        t.cells,
        rep.summary(),
        cross.equal,
        cross.mismatches.len()
    );
    rep.assert_clean();
    assert!(cross.mismatches.is_empty(), "cross-check: {:?}", &cross.mismatches[..cross.mismatches.len().min(5)]);
}
