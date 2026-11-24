//! Memory gate of the C4 viewport emission (`--features view`). M1: opening a workbook, emitting frames at several zooms and
//! dropping everything leaves no live byte. M2: 600 scrolling frames are flat at tolerance 0 after warm-up (the list, the
//! encoder and the scratch are reused; the resources of the stream are bounded by what the sheet uses). M3: a warm frame's
//! allocation, reported.
#![cfg(feature = "view")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, soak};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontDesc, IdentityEnv, Measurer, ViewportRequest};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

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

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

fn files() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for g in ["x1", "cf1", "paint1"] {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks").join(g);
        let mut f: Vec<PathBuf> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.path()).collect();
        f.sort();
        v.extend(f);
    }
    v
}

fn params(si: usize, sx: f64, sy: f64, zoom: f64) -> ViewParams {
    ViewParams {
        req: ViewportRequest {
            sheet: si as u32,
            width: 1280.0,
            height: 720.0,
            dpr: 1.0,
            scroll_x: sx,
            scroll_y: sy,
            zoom,
            selection: [2, 2, 2, 2, 3, 4],
        },
        accent: "#006d35".to_owned(),
        capture: false,
        editing: None,
        spill: None,
        mirrored: false,
    }
}

fn frame(v: &mut Viewer, book: &Book, p: &ViewParams, out: &mut Vec<u8>) {
    for _ in 0..8 {
        out.clear();
        match v.emit(book, p, &IdentityEnv, out) {
            Ok(()) => return,
            Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("does not settle");
}

fn run(bytes: &[u8]) -> usize {
    let mut book = Book::open(bytes, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let mut v = Viewer::new(&book, 1, CLOCK);
    let mut out = Vec::new();
    let mut n = 0;
    for si in 0..book.workbook.sheets.len() {
        if book.workbook.sheets[si].target.is_none() || book.sheets[si].as_ref().is_some_and(|w| w.chartsheet.is_some()) {
            continue;
        }
        for _ in 0..8 {
            match v.activate(&mut book, si, &IdentityEnv) {
                Ok(()) => break,
                Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
                Err(e) => panic!("{e:?}"),
            }
        }
        for z in [0.75, 1.0, 1.75] {
            frame(&mut v, &book, &params(si, 31.0, 17.0, z), &mut out);
            n += out.len();
        }
    }
    drop(v);
    drop(book);
    uo_numfmt::clear_caches();
    uo_text::clear_caches();
    n
}

#[test]
fn m1_open_emit_drop_leaves_nothing() {
    let all = files();
    for f in &all {
        run(&std::fs::read(f).unwrap());
    }
    let mut work = 0;
    for f in &all {
        let bytes = std::fs::read(f).unwrap();
        if let Err(e) = uo_alloc::check_no_leak(|| work += run(&bytes)) {
            panic!("{}: {e:?}", f.display());
        }
    }
    assert!(work > 100_000);
}

#[test]
fn m2_scrolling_frames_are_flat() {
    let raw = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples/showcase.xlsx")).unwrap();
    let mut book = Book::open(&raw, ReadLimits::default()).unwrap();
    book.load_all().unwrap();
    let si = book.active_sheet().unwrap_or(0);
    let mut v = Viewer::new(&book, 1, CLOCK);
    for _ in 0..8 {
        match v.activate(&mut book, si, &IdentityEnv) {
            Ok(()) => break,
            Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
            Err(e) => panic!("{e:?}"),
        }
    }
    let mut out = Vec::new();
    let series = soak(600, 20, |k| {
        let k = k as f64;
        frame(&mut v, &book, &params(si, (k % 7.0) * 9.0, (k % 11.0) * 13.0, 1.0), &mut out);
    });
    assert_flat(&series, 5, 0);
}
