//! C4 parity: the viewport op stream of the Rust emission against the frozen TS `paint()` over a recording context
//! (`sheet-viewport` capture from `js/capture-viewport.mjs`): per workbook, sheet and state (zoom, scroll, selection, DPR,
//! frozen panes from the model) the canonical op stream after `GlyphRun` expansion, by digest (and, with `UO_VP_LINES=<dir>`
//! holding the capture's `--lines` output, line by line for the first difference).
//! Run: `cargo test -p uo-xlsx --features view,facet --test viewport_parity`.
#![cfg(feature = "view")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation, clippy::too_many_lines)]

mod support;

use std::path::{Path, PathBuf};

use support::vp_canon::canon_skip;
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontDesc, IdentityEnv, Measurer, ViewportRequest};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

/// The states of `capture-viewport.mjs`.
const STATES: [(f64, f64, f64, f64, [u32; 6]); 4] = [
    (1.0, 0.0, 0.0, 1.0, [1, 1, 1, 1, 1, 1]),
    (1.75, 41.5, 17.25, 2.0, [3, 3, 2, 2, 4, 5]),
    (0.75, 123.0, 57.0, 1.0, [2, 2, 2, 2, 2, 2]),
    (1.25, 700.5, 333.0, 1.5, [5, 7, 5, 7, 8, 9]),
];
const ODD: &str = "a cell at a position named by another ref (the TS model follows the name, the store the file; C3a-9)";
const W: f64 = 640.0;
const H: f64 = 360.0;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/paint1/") {
        dir().join(format!("workbooks/paint1/paint1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/cf1/") {
        dir().join(format!("workbooks/cf1/cf1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/dr1/") {
        dir().join(format!("workbooks/dr1/dr1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/dr-big/") {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/sheet-cap/workbooks/dr-big/dr-big-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/x1-big/") {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/sheet-cap/workbooks/x1-big/x1-big-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/paint-big/") {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/sheet-cap/workbooks/paint-big/paint-big-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/cf-big/") {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/sheet-cap/workbooks/cf-big/cf-big-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

/// The fake canvas of the harness (6 px per UTF-16 unit, the `'Hg'` box 9/3).
struct Fake;

impl Measurer for Fake {
    fn advance(&self, _f: &FontDesc, cp: u32) -> i32 {
        if cp > 0xFFFF { 12 } else { 6 }
    }
    fn text_width(&self, _f: &FontDesc, s: &[u16]) -> i32 {
        6 * i32::try_from(s.len()).unwrap()
    }
    fn font_box(&self, _f: &FontDesc) -> (Option<f64>, Option<f64>) {
        (Some(9.0), Some(3.0))
    }
}

fn dig(text: &str) -> String {
    let h = uo_testkit::sha256::sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_testkit::stream::hex(&h)[..32])
}

fn gunzip(path: &Path) -> Option<String> {
    let b = std::fs::read(path).ok()?;
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).ok()?;
    String::from_utf8(raw).ok()
}

fn ref_of(col: u32, row: u32) -> String {
    let mut s = Vec::new();
    let mut c = col;
    while c > 0 {
        s.push(b'A' + ((c - 1) % 26) as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    format!("{}{row}", String::from_utf8(s).unwrap())
}

/// The sheet has a cell at a position named by another `r` (the TS model follows the name, the store the file: C3a-9).
fn odd_ref(book: &Book, si: usize) -> bool {
    use uo_calc::key::{Area, SheetIx};
    let ix = SheetIx(u32::try_from(si).unwrap());
    let mut odd = false;
    book.store.each(Area { sheet: ix, r1: 0, c1: 0, r2: 1_048_575, c2: 16_383 }, |row, col, cell| {
        if !odd
            && cell.kind.has(uo_calc::store::CellKind::HAS_EXTRA)
            && let Some(rr) = book.store.extra(ix, row, col).and_then(|e| e.raw_ref.as_ref())
            && rr.as_js().to_string_lossy() != ref_of(col + 1, row + 1)
        {
            odd = true;
        }
    });
    odd
}

#[derive(Default)]
struct Totals {
    odd: u64,
    workbooks: u64,
    streams: u64,
    ops: u64,
    equal: u64,
}

fn params(si: usize, st: usize) -> ViewParams {
    let (z, sx, sy, dpr, sel) = STATES[st];
    ViewParams {
        req: ViewportRequest { sheet: si as u32, width: W, height: H, dpr, scroll_x: sx, scroll_y: sy, zoom: z, selection: sel },
        accent: "#006d35".to_owned(),
        capture: false,
        editing: None,
        spill: None,
        mirrored: false,
    }
}

fn activate(v: &mut Viewer, book: &mut Book, si: usize) {
    for _ in 0..8 {
        match v.activate(book, si, &IdentityEnv) {
            Ok(()) => return,
            Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("activation does not settle");
}

fn frame(v: &mut Viewer, book: &Book, p: &ViewParams) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..16 {
        match v.emit(book, p, &IdentityEnv, &mut out) {
            Ok(()) => return out,
            Err(ViewError::NeedMeasures) => {
                v.measure.satisfy(&Fake);
                let n = v.dml_needs();
                support::stub_measure::answer(v, &n);
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("frame does not settle");
}

/// The camera render of the capture (`renderRegionToCanvas(i, canvas, 8, 14)`, state 4): the message and `w,h`.
fn camera(v: &mut Viewer, book: &Book, si: usize) -> (Vec<u8>, String) {
    let mut out = Vec::new();
    for _ in 0..16 {
        out.clear();
        match v.emit_region(book, &params(si, 0), 8, 14, &IdentityEnv, &mut out) {
            Ok((w, h)) => return (out, format!("{},{}", uo_jsnum::format::to_js_string(w), uo_jsnum::format::to_js_string(h))),
            Err(ViewError::NeedMeasures) => {
                v.measure.satisfy(&Fake);
                let n = v.dml_needs();
                support::stub_measure::answer(v, &n);
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("camera does not settle");
}

fn run(cap_path: &Path, lines_dir: Option<PathBuf>) -> (Totals, String) {
    let cap: &'static Capture = Box::leak(Box::new(Capture::read(cap_path).unwrap()));
    let mut rep = Replay::new(cap);
    let mut t = Totals::default();
    let mut dump: Vec<(String, String)> = Vec::new();
    let keys: Vec<String> =
        cap.records().iter().map(|r| r.key.clone()).filter(|k| k.starts_with("gen/") || k.starts_with("samples/")).collect();
    for key in keys {
        if cap.get(&key, "open") != Some("ok") {
            continue;
        }
        let Some(raw) = bytes_of(&key) else { continue };
        t.workbooks += 1;
        rep.skip(&key, "open");
        rep.skip(&key, "stacks");
        let mut book = Book::open(&raw, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let mut viewer = Viewer::new(&book, 1, CLOCK);
        viewer.set_rules(uo_dml_render::SlidesRules::ts_strict());
        let sheets: Vec<usize> = (0..book.workbook.sheets.len()).filter(|&i| book.workbook.sheets[i].target.is_some()).collect();
        let ts_lines = lines_dir.as_ref().and_then(|d| gunzip(&d.join(format!("{}.txt.gz", key.replace(['/', '\\'], "_")))));
        let mut all = String::new();
        for (vi, &si) in sheets.iter().enumerate() {
            activate(&mut viewer, &mut book, si);
            for k in 0..=STATES.len() {
                // state 4: the camera render (C4b), not for chart sheets
                if k == STATES.len() && cap.get(&key, &format!("v{vi}@{k}")).is_none() {
                    continue;
                }
                // the drawing phases are compared by `js/compare-drawings.mjs` (C4b)
                rep.skip(&key, &format!("d{vi}@{k}"));
                rep.skip(&key, &format!("dc{vi}@{k}"));
                let bytes = if k < STATES.len() {
                    frame(&mut viewer, &book, &params(si, k))
                } else {
                    let (b, wh) = camera(&mut viewer, &book, si);
                    rep.check(&key, &format!("w{vi}@{k}"), &wh);
                    b
                };
                assert!(!bytes.is_empty());
                let lines = canon_skip(viewer.last_list(), viewer.resources(), viewer.drawing_spans());
                t.streams += 1;
                all.push_str(&format!(
                    "# {vi}@{k}
{}
",
                    lines.join(
                        "
"
                    )
                ));
                t.ops += lines.len() as u64;
                let d = dig(&lines.join("\n"));
                let facet = format!("v{vi}@{k}");
                let odd = odd_ref(&book, si);
                let ok = if odd { rep.accept(&key, &facet, &d, ODD) } else { rep.check(&key, &facet, &d) };
                if odd && !ok {
                    t.odd += 1;
                    t.ops -= lines.len() as u64;
                    continue;
                }
                rep.skip(&key, &format!("n{vi}@{k}"));
                if ok {
                    t.equal += lines.len() as u64;
                } else if let Some(ts) = &ts_lines {
                    // the TS lines of this stream: between `# vi@k` and the next marker
                    let mark = format!("# {vi}@{k}");
                    let want: Vec<&str> = ts.lines().skip_while(|l| *l != mark).skip(1).take_while(|l| !l.starts_with("# ")).collect();
                    let mut shown = 0;
                    let mut same = 0;
                    for (i, (a, w)) in lines.iter().zip(want.iter()).enumerate() {
                        if a == w {
                            same += 1;
                        } else if shown < 3 {
                            shown += 1;
                            eprintln!("{key} {facet} op {i}:\n  ts:   {w}\n  rust: {a}");
                        }
                    }
                    if lines.len() != want.len() {
                        eprintln!("{key} {facet}: {} ops in Rust, {} in TS", lines.len(), want.len());
                    }
                    t.equal += same;
                }
            }
        }
        dump.push((key.clone(), all));
    }
    if let Some(d) = std::env::var_os("UO_VP_DUMP") {
        let d = PathBuf::from(d);
        std::fs::create_dir_all(&d).unwrap();
        for (k, v) in dump {
            std::fs::write(d.join(format!("{}.txt", k.replace('/', "_"))), v).unwrap();
        }
    }
    let r = rep.finish();
    (t, format!("{}\n{}", r.summary(), r.describe(8)))
}

#[test]
fn ci_viewport() {
    let cap = dir().join("sheet-viewport.capture");
    if !cap.exists() {
        eprintln!("no sheet-viewport.capture: run js/capture-viewport.mjs");
        return;
    }
    let (t, msg) = run(&cap, std::env::var_os("UO_VP_LINES").map(PathBuf::from));
    eprintln!(
        "viewport: {} workbooks, {} streams ({} accepted as odd-ref), {} ops compared, {} equal; {msg}",
        t.workbooks, t.streams, t.odd, t.ops, t.equal
    );
    assert_eq!(t.ops, t.equal, "op streams differ");
    assert!(t.workbooks >= 40);
}

/// A local capture (`UO_VP_CAPTURE=<file>`, e.g. `core/target/sheet-cap/viewport-big.capture`).
#[test]
fn local_viewport() {
    let Some(cap) = std::env::var_os("UO_VP_CAPTURE") else { return };
    let (t, msg) = run(Path::new(&cap), std::env::var_os("UO_VP_LINES").map(PathBuf::from));
    eprintln!(
        "viewport (local): {} workbooks, {} streams ({} accepted as odd-ref), {} ops compared, {} equal; {msg}",
        t.workbooks, t.streams, t.odd, t.ops, t.equal
    );
    assert_eq!(t.ops, t.equal, "op streams differ");
}
