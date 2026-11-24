//! C1a parity: the model and the store of every committed capture (`sheet-x1`, `sheet-samples`) as the frozen TS
//! reads them. Facets compared per workbook: `read`, `wb`, `styles`, `theme`, `sst`, `sheets`, and per worksheet
//! `ws<i>` (the model minus rows), `st<i>` (the store facet), `fo<i>` (file order), `rw<i>` (row attributes).
//! Run: `cargo test -p uo-xlsx --features facet --test read_parity` (the giants of `sheet-samples` only with
//! `UO_XLSX_BIG=1`, in release).
#![cfg(feature = "facet")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::facet::{file_order, row_meta_lines, store_facet};
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::read::facet;

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

fn dig(text: &str) -> String {
    let h = uo_testkit::sha256::sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_testkit::stream::hex(&h)[..32])
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

fn fo_text(seq: &[(u32, u32)]) -> String {
    let sorted = seq.windows(2).all(|w| u64::from(w[0].0) * 20000 + u64::from(w[0].1) < u64::from(w[1].0) * 20000 + u64::from(w[1].1));
    if sorted { "sorted".to_owned() } else { list_dig(&seq.iter().map(|(r, c)| format!("{r}:{c}")).collect::<Vec<_>>()) }
}

struct Totals {
    workbooks: usize,
    sheets: usize,
    cells: u64,
    facets: usize,
}

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

/// Shows the first differing place of a model text against the sidecar's, when it has it.
fn explain(side: &Side, key: &str, facet: &str, ours: &str) {
    if let Some(want) = side.get(key).and_then(|m| m.get(facet)).and_then(|v| v.first()) {
        let at = ours.bytes().zip(want.bytes()).position(|(a, b)| a != b).unwrap_or(ours.len().min(want.len()));
        let from = at.saturating_sub(60);
        let cut = |s: &str| s.get(from..(at + 140).min(s.len())).unwrap_or("").to_owned();
        eprintln!("{key} {facet} differs at byte {at}:\n  ts:   ...{}\n  rust: ...{}", cut(want), cut(ours));
    }
}

fn chk(rep: &mut Replay<'_>, t: &mut Totals, side: &Side, key: &str, facet: &str, text: &str) {
    t.facets += 1;
    if !rep.check(key, facet, &dig(text)) {
        explain(side, key, facet, text);
    }
}

fn replay(name: &str, big: bool, t: &mut Totals) {
    let cap_path = dir().join(format!("{name}.capture"));
    let cap = Capture::read(&cap_path).unwrap();
    let side = sidecar(&dir().join(format!("{name}.side.gz")));
    let mut rep = Replay::new(&cap);
    let mut rng_keys = 0;
    for rec in cap.records() {
        let key = rec.key.as_str();
        if !big && (key.ends_with("giant-1m.xlsx") || key.ends_with("giant-200k.xlsx")) {
            continue;
        }
        let Some(bytes) = bytes_of(key) else { continue };
        rng_keys += 1;
        t.workbooks += 1;
        let opened = Book::open(&bytes, ReadLimits::default());
        let want_read = rec.get("read").unwrap_or("ok");
        let mut book = match opened {
            Ok(b) => {
                rep.check(key, "read", "ok");
                b
            }
            Err(e) => {
                let got = format!("err:{e}");
                // the class of the error, not its text
                rep.check(key, "read", if want_read.starts_with("err") { want_read } else { &got });
                continue;
            }
        };
        chk(&mut rep, t, &side, key, "wb", &facet::workbook(&book.workbook));
        chk(&mut rep, t, &side, key, "styles", &facet::styles(&book.styles));
        chk(&mut rep, t, &side, key, "theme", &facet::theme(&book.cx.theme));
        // shared strings: every entry decoded
        let n_sst = book.sst.as_ref().map_or(0, uo_xlsx::read::sst::SstIndex::len);
        let mut entries = Vec::new();
        for i in 0..n_sst {
            let it = book.sst.as_ref().unwrap().decode(&book.pkg, i as u32, &book.cx, &mut book.run_props).unwrap().unwrap();
            entries.push((i, it));
        }
        let sst_json = facet::jarr(entries.iter().map(|(_, it)| facet::shared_string(it, &book.run_props)));
        chk(&mut rep, t, &side, key, "sst", &sst_json);
        // worksheets: only the refs with a part count (`n` of the capture script)
        let mut n = 0usize;
        for i in 0..book.workbook.sheets.len() {
            if book.workbook.sheets[i].target.is_none() {
                continue;
            }
            let loaded = book.load_sheet(i);
            let wf = format!("ws{n}");
            if rec.get(&wf).is_some_and(|v| v.starts_with("err:")) {
                rep.check(key, &wf, if loaded.is_err() { rec.get(&wf).unwrap() } else { "loaded" });
                n += 1;
                continue;
            }
            loaded.unwrap_or_else(|e| panic!("{key} sheet {i}: {e}"));
            t.sheets += 1;
            let ws = book.sheets[i].as_ref().unwrap();
            chk(&mut rep, t, &side, key, &wf, &facet::worksheet(ws));
            let ix = uo_calc::key::SheetIx(i as u32);
            let rp = &book.run_props;
            let props = |id: u32| rp.get(id).map(facet::run_props_json).unwrap_or_default();
            let st = store_facet(&book.store, ix, &props);
            t.cells += st.len() as u64;
            t.facets += 1;
            if !rep.check(key, &format!("st{n}"), &list_dig(&st)) {
                let want = side.get(key).and_then(|m| m.get(&format!("st{n}"))).cloned().unwrap_or_default();
                if let Some(k) = st.iter().zip(want.iter()).position(|(a, b)| a != b) {
                    eprintln!("{key} st{n} line {k}:\n  ts:   {}\n  rust: {}", want[k], st[k]);
                } else {
                    eprintln!("{key} st{n}: {} lines here, {} in the sidecar", st.len(), want.len());
                }
            }
            t.facets += 1;
            if !rep.check(key, &format!("fo{n}"), &fo_text(&file_order(&book.store, ix))) {
                eprintln!("{key} fo{n}");
            }
            let rw = row_meta_lines(&book.store, ix);
            t.facets += 1;
            if !rep.check(key, &format!("rw{n}"), &list_dig(&rw)) {
                eprintln!("{key} rw{n}: {rw:?}\n  ts {:?}", side.get(key).and_then(|m| m.get(&format!("rw{n}"))));
            }
            n += 1;
        }
        rep.check(key, "sheets", &n.to_string());
        t.facets += 1;
        book.store.check_invariants().unwrap_or_else(|e| panic!("{key}: {e}"));
    }
    let report = rep.finish();
    eprintln!("{name}: {} workbooks replayed; {}", rng_keys, report.summary());
    assert!(report.mismatches.is_empty(), "{name}: {}", report.describe(10));
}

#[test]
fn x1_model_and_store() {
    let mut t = Totals { workbooks: 0, sheets: 0, cells: 0, facets: 0 };
    replay("sheet-x1", false, &mut t);
    eprintln!("x1: {} workbooks, {} sheets, {} cells, {} facets", t.workbooks, t.sheets, t.cells, t.facets);
    assert!(t.workbooks >= 30 && t.cells > 3000);
}

#[test]
fn samples_model_and_store() {
    let big = uo_testkit::env_flag("UO_XLSX_BIG");
    let mut t = Totals { workbooks: 0, sheets: 0, cells: 0, facets: 0 };
    replay("sheet-samples", big, &mut t);
    eprintln!(
        "samples: {} workbooks, {} sheets, {} cells, {} facets (giants {})",
        t.workbooks,
        t.sheets,
        t.cells,
        t.facets,
        if big { "included" } else { "skipped: UO_XLSX_BIG=1" }
    );
    assert!(t.workbooks >= 5);
}
