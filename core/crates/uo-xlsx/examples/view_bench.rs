//! C4 benchmark (`cargo run --release -p uo-xlsx --features view --example view_bench -- <workbook.xlsx>...`): open -> first
//! viewport list, and the emission of a 1080p viewport (warm, scrolling), with the bytes allocated per frame and the bytes
//! that cross the wasm boundary (the encoded message). The TS side is `js/bench-viewport.mjs` (same workbook, same viewport).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout, clippy::cast_precision_loss, clippy::panic)]

use std::time::Instant;

use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontDesc, IdentityEnv, Measurer, ViewportRequest};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

#[path = "../tests/support/stub_measure.rs"]
mod stub;

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

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn params(sheet: usize, sx: f64, sy: f64, zoom: f64) -> ViewParams {
    ViewParams {
        req: ViewportRequest {
            sheet: sheet as u32,
            width: 1920.0,
            height: 1080.0,
            dpr: 1.0,
            scroll_x: sx,
            scroll_y: sy,
            zoom,
            selection: [2, 2, 2, 2, 4, 6],
        },
        accent: "#006d35".to_owned(),
        capture: false,
        editing: None,
        spill: None,
        mirrored: false,
    }
}

fn frame(v: &mut Viewer, book: &Book, p: &ViewParams, out: &mut Vec<u8>) {
    for _ in 0..16 {
        out.clear();
        match v.emit(book, p, &IdentityEnv, out) {
            Ok(()) => return,
            Err(ViewError::NeedMeasures) => {
                v.measure.satisfy(&Fake);
                let n = v.dml_needs();
                stub::answer(v, &n);
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("does not settle");
}

fn main() {
    let clock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };
    for path in std::env::args().skip(1) {
        let raw = std::fs::read(&path).unwrap();
        let t0 = Instant::now();
        let mut book = Book::open(&raw, ReadLimits::default()).unwrap();
        let si = book.active_sheet().unwrap_or(0);
        let t_open = t0.elapsed();
        let mut v = Viewer::new(&book, 1, clock);
        for _ in 0..8 {
            match v.activate(&mut book, si, &IdentityEnv) {
                Ok(()) => break,
                Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
                Err(e) => panic!("{e:?}"),
            }
        }
        let mut out = Vec::new();
        let p0 = params(si, 0.0, 0.0, 1.0);
        frame(&mut v, &book, &p0, &mut out);
        let t_first = t0.elapsed();
        println!(
            "{path}: open {:.1} ms, open -> first list {:.1} ms, first list {} bytes",
            t_open.as_secs_f64() * 1e3,
            t_first.as_secs_f64() * 1e3,
            out.len()
        );
        for (name, zoom) in [("1080p z1.00", 1.0), ("1080p z0.50", 0.5)] {
            // warm
            for k in 0..20 {
                frame(&mut v, &book, &params(si, f64::from(k) * 7.0, f64::from(k) * 21.0, zoom), &mut out);
            }
            let mut ts = Vec::new();
            let mut alloc = Vec::new();
            let mut bytes = Vec::new();
            for k in 0..200 {
                let p = params(si, f64::from(k % 40) * 11.5, f64::from(k % 90) * 37.0, zoom);
                let t = Instant::now();
                let ((), u) = uo_alloc::measure(|| frame(&mut v, &book, &p, &mut out));
                ts.push(t.elapsed().as_secs_f64() * 1e3);
                alloc.push(u.allocated as f64);
                bytes.push(out.len() as f64);
            }
            println!(
                "  {name}: emit {:.3} ms median ({:.3} min), allocated {:.0} B/frame (median), message {:.0} B (median, max {:.0})",
                median(&mut ts.clone()),
                ts.iter().cloned().fold(f64::MAX, uo_jsnum::math::min),
                median(&mut alloc),
                median(&mut bytes.clone()),
                bytes.iter().cloned().fold(0.0, uo_jsnum::math::max)
            );
            let l = v.last_list();
            {
                let mut enc = uo_render::slides::SlidesEncoder::new(1);
                let mut o2 = Vec::with_capacity(1 << 20);
                let _ = enc.encode(l, v.resources(), &mut o2);
                o2.clear();
                let (_, u) = uo_alloc::measure(|| {
                    let _ = enc.encode(l, v.resources(), &mut o2);
                });
                println!("    encode alone: allocated {} B", u.allocated);
            }
            let glyphs = l.items.iter().filter(|i| matches!(i, uo_render::slides::SItem::GlyphRun(_))).count();
            println!(
                "    list: {} items ({} glyph runs), {} paths, {} text units, {} advances, {} runs",
                l.items.len(),
                glyphs,
                l.paths.len(),
                l.texts.len(),
                l.advances.len(),
                l.runs.len()
            );
        }
        let l = v.last_list();
        if std::env::var_os("UO_VB_HIST").is_some() {
            let mut h: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            for it in &l.items {
                let d = format!("{it:?}");
                let k = d.split(['(', ' ', '{']).next().unwrap_or("").to_owned();
                *h.entry(k).or_default() += 1;
            }
            println!("  items by kind: {h:?}");
        }
        println!(
            "  last list: {} items, {} paths, {} texts, {} advances, {} stops",
            l.items.len(),
            l.paths.len(),
            l.texts.len(),
            l.advances.len(),
            l.stops.len()
        );
    }
}
