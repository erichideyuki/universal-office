//! C2 parity: the `.xls` converter against the frozen TS (`js/capture-xls.mjs`). For every file of the capture the package parts
//! (names in order, every part's bytes) and then the C1a facets of the model and the store read from the converted package
//! (`wb`, `styles`, `theme`, `sst`, `ws<i>`, `st<i>`, `fo<i>`, `rw<i>`).
//!
//! Run: `cargo test -p uo-xlsx --features xls,facet --test xls_parity`. Bigger local runs:
//! `UO_XLS_CAPTURE=core/target/sheet-cap/xls-xls1.capture UO_XLS_DIR=core/target/sheet-cap/xls-out/xls1 cargo test … --release`.
#![cfg(all(feature = "xls", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::facet::{file_order, row_meta_lines, store_facet};
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::read::facet;
use uo_xlsx::xls::{self, XlsError, XlsOpenError};

type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap();
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 31).unwrap();
    String::from_utf8(raw).unwrap()
}

fn sidecar(path: &Path) -> Side {
    let mut out: Side = BTreeMap::new();
    if !path.exists() {
        return out;
    }
    for line in gunzip(path).lines() {
        let mut it = line.splitn(3, '\t');
        if let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next()) {
            out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
        }
    }
    out
}

fn dig_bytes(b: &[u8]) -> String {
    let h = uo_testkit::sha256::sha256(b);
    format!("{}:{}", b.len(), &uo_testkit::stream::hex(&h)[..32])
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

/// The text the capture script keeps in the sidecar (`\`, newline, CR and tab escaped).
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

fn explain(side: &Side, key: &str, facet: &str, ours: &str) {
    if let Some(want) = side.get(key).and_then(|m| m.get(facet)).and_then(|v| v.first()) {
        let at = ours.bytes().zip(want.bytes()).position(|(a, b)| a != b).unwrap_or(ours.len().min(want.len()));
        let from = at.saturating_sub(80);
        let cut = |s: &str| {
            let (a, b) = (from.min(s.len()), (at + 160).min(s.len()));
            String::from_utf8_lossy(s.as_bytes().get(a..b).unwrap_or(&[])).into_owned()
        };
        eprintln!("{key} {facet} differs at byte {at}:\n  ts:   ...{}\n  rust: ...{}", cut(want), cut(ours));
    }
}

#[derive(Default)]
struct Totals {
    files: usize,
    errors: usize,
    parts: usize,
    parts_equal: usize,
    sheets: usize,
    cells: u64,
    facets: usize,
}

fn chk(rep: &mut Replay<'_>, t: &mut Totals, side: &Side, key: &str, facet_name: &str, text: &str) {
    t.facets += 1;
    if !rep.check(key, facet_name, &dig_bytes(text.as_bytes())) {
        explain(side, key, facet_name, text);
    }
}

fn err_class(e: &XlsError) -> &'static str {
    match e {
        XlsError::Encrypted => "XlsEncryptedError",
        XlsError::Format(_) | XlsError::Limit(_) => "XlsFormatError",
    }
}

fn run(capture: &Path, side_path: &Path, dir: &Path, t: &mut Totals) {
    let cap = Capture::read(capture).unwrap();
    let side = sidecar(side_path);
    let mut rep = Replay::new(&cap);
    for rec in cap.records() {
        let key = rec.key.as_str();
        let Some(name) = key.strip_prefix("xls/") else { continue };
        let Ok(bytes) = std::fs::read(dir.join(name)) else { continue };
        t.files += 1;
        rep.check(key, "input", &format!("{}:{}", bytes.len(), &uo_testkit::stream::hex(&uo_testkit::sha256::sha256(&bytes))[..16]));
        let want_read = rec.get("read").unwrap_or("ok");
        let converted = xls::convert(&bytes);
        let parts = match converted {
            Ok(p) => {
                rep.check(key, "read", "ok");
                p
            }
            Err(e) => {
                t.errors += 1;
                // the class of the error, not its text
                let got = format!("err:{}", err_class(&e));
                let same_class = want_read.starts_with(&format!("{got}:")) || want_read == got;
                rep.check(key, "read", if same_class { want_read } else { &got });
                continue;
            }
        };
        // the part names in order and every part's bytes
        let names: Vec<&str> = parts.items.iter().map(|(n, _)| n.as_str()).collect();
        rep.check(key, "names", &names.join("|"));
        let lines: Vec<String> = parts.items.iter().map(|(n, d)| format!("{n} {}", dig_bytes(d))).collect();
        rep.check(key, "parts", &list_dig(&lines));
        for (i, (n, d)) in parts.items.iter().enumerate() {
            t.parts += 1;
            let ok = rep.check(key, &format!("p{i}"), &dig_bytes(d));
            if ok {
                t.parts_equal += 1;
            } else if n.ends_with(".xml") || n.ends_with(".rels") {
                explain(&side, key, &format!("x:{n}"), &esc(&String::from_utf8_lossy(d)));
            } else {
                eprintln!("{key}: part {n} differs ({} bytes)", d.len());
            }
        }
        // the model of the converted package, as the normal reader sees it
        let opened = Book::open_xls(&bytes, ReadLimits::default());
        let mut book = match opened {
            Ok(b) => b,
            Err(XlsOpenError::Xls(e)) => panic!("{key}: converted once, failed the second time: {e}"),
            Err(e) => {
                eprintln!("{key}: the reader refuses the converted package: {e}");
                if rec.get("wb").is_some() {
                    rep.check(key, "wb", "reader-error");
                }
                continue;
            }
        };
        chk(&mut rep, t, &side, key, "wb", &facet::workbook(&book.workbook));
        chk(&mut rep, t, &side, key, "styles", &facet::styles(&book.styles));
        chk(&mut rep, t, &side, key, "theme", &facet::theme(&book.cx.theme));
        let n_sst = book.sst.as_ref().map_or(0, uo_xlsx::read::sst::SstIndex::len);
        let mut entries = Vec::new();
        for i in 0..n_sst {
            let it =
                book.sst.as_ref().unwrap().decode(&book.pkg, u32::try_from(i).unwrap(), &book.cx, &mut book.run_props).unwrap().unwrap();
            entries.push(it);
        }
        let sst_json = facet::jarr(entries.iter().map(|it| facet::shared_string(it, &book.run_props)));
        chk(&mut rep, t, &side, key, "sst", &sst_json);
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
            let ix = uo_calc::key::SheetIx(u32::try_from(i).unwrap());
            let rp = &book.run_props;
            let props = |id: u32| rp.get(id).map(facet::run_props_json).unwrap_or_default();
            let st = store_facet(&book.store, ix, &props);
            t.cells += st.len() as u64;
            t.facets += 1;
            if !rep.check(key, &format!("st{n}"), &list_dig(&st)) {
                let want = side.get(key).and_then(|m| m.get(&format!("st{n}"))).cloned().unwrap_or_default();
                match st.iter().zip(want.iter()).position(|(a, b)| a != b) {
                    Some(k) => eprintln!("{key} st{n} line {k}:\n  ts:   {}\n  rust: {}", want[k], st[k]),
                    None => eprintln!("{key} st{n}: {} lines here, {} in the sidecar", st.len(), want.len()),
                }
            }
            t.facets += 1;
            if !rep.check(key, &format!("fo{n}"), &fo_text(&file_order(&book.store, ix))) {
                eprintln!("{key} fo{n}");
            }
            let rw = row_meta_lines(&book.store, ix);
            t.facets += 1;
            if !rep.check(key, &format!("rw{n}"), &list_dig(&rw)) {
                eprintln!("{key} rw{n}: {rw:?}");
            }
            n += 1;
        }
        rep.check(key, "sheets", &n.to_string());
        t.facets += 1;
        book.store.check_invariants().unwrap_or_else(|e| panic!("{key}: {e}"));
    }
    let report = rep.finish();
    eprintln!("{}: {}", capture.display(), report.summary());
    assert!(report.mismatches.is_empty(), "{}", report.describe(12));
}

#[test]
fn xls_parts_and_model() {
    let capture = std::env::var_os("UO_XLS_CAPTURE").map_or_else(|| captures().join("xls.capture"), PathBuf::from);
    let dir = std::env::var_os("UO_XLS_DIR").map_or_else(|| captures().join("workbooks/xls"), PathBuf::from);
    let side = PathBuf::from(capture.to_string_lossy().replace(".capture", ".side.gz"));
    let mut t = Totals::default();
    run(&capture, &side, &dir, &mut t);
    eprintln!(
        "xls: {} files ({} refused), {} parts ({} equal), {} sheets, {} cells, {} model facets",
        t.files, t.errors, t.parts, t.parts_equal, t.sheets, t.cells, t.facets
    );
    assert!(t.files >= 1);
}
