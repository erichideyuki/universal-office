//! The viewport frames of a workbook for the drawings parity (frontier C4b; record `core/docs/modules/uo-xlsx.md` "C4b — as
//! built"): every sheet at the four states of `js/capture-viewport.mjs` (640 x 360), the grid measured by the harness's fake
//! canvas and the drawings by the Slides capture stub, under `SlidesRules::ts_strict()`. Output (little endian): `UOVF`, `u8`
//! version 1, then per frame `sheet u32, state u32, n u32` drawing spans `(start u32, end u32)`, `len u32` and the list message
//! (one stream per workbook), `n u32` bitmaps added by this frame (`len u32` with bit 31 = recipe, the bytes). A sheet that does
//! not settle writes `len 0`.
//!
//!   cargo run -p uo-xlsx --features view --release --example view_dump -- <book.xlsx> <out.uovf>
#![allow(clippy::print_stderr, clippy::unwrap_used, clippy::cast_possible_truncation)]

use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontDesc, IdentityEnv, Measurer, ViewportRequest};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

#[path = "../tests/support/stub_measure.rs"]
mod stub;

const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };
const STATES: [(f64, f64, f64, f64, [u32; 6]); 4] = [
    (1.0, 0.0, 0.0, 1.0, [1, 1, 1, 1, 1, 1]),
    (1.75, 41.5, 17.25, 2.0, [3, 3, 2, 2, 4, 5]),
    (0.75, 123.0, 57.0, 1.0, [2, 2, 2, 2, 2, 2]),
    (1.25, 700.5, 333.0, 1.5, [5, 7, 5, 7, 8, 9]),
];

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

fn u32le(o: &mut Vec<u8>, v: usize) {
    o.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_le_bytes());
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: view_dump <book.xlsx> <out.uovf>");
        std::process::exit(2);
    }
    let raw = std::fs::read(&a[1]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", a[1]);
        std::process::exit(2)
    });
    let mut book = match Book::open(&raw, ReadLimits::default()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("open: {e:?}");
            std::process::exit(3)
        }
    };
    let _ = book.load_all();
    let mut v = Viewer::new(&book, 1, CLOCK);
    v.set_rules(uo_dml_render::SlidesRules::ts_strict());
    let mut out: Vec<u8> = b"UOVF".to_vec();
    out.push(1);
    let mut sent = 0usize;
    let sheets: Vec<usize> = (0..book.workbook.sheets.len()).filter(|&i| book.workbook.sheets[i].target.is_some()).collect();
    for &si in &sheets {
        let mut ok = false;
        for _ in 0..8 {
            match v.activate(&mut book, si, &IdentityEnv) {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(ViewError::NeedMeasures) => v.measure.satisfy(&Fake),
                Err(_) => break,
            }
        }
        let chart = book.sheets[si].as_ref().is_some_and(|w| w.chartsheet.is_some());
        // states 0-3 and the camera render (state 4, `renderRegionToCanvas(i, canvas, 8, 14)`; not for chart sheets)
        for k in 0..(if chart { 4 } else { 5 }) {
            let (z, sx, sy, dpr, sel) = STATES[k.min(3)];
            let p = ViewParams {
                req: ViewportRequest {
                    sheet: si as u32,
                    width: 640.0,
                    height: 360.0,
                    dpr,
                    scroll_x: sx,
                    scroll_y: sy,
                    zoom: z,
                    selection: sel,
                },
                accent: "#006d35".to_owned(),
                capture: false,
                editing: None,
                spill: None,
                mirrored: false,
            };
            let mut msg = Vec::new();
            if ok {
                for round in 0..24 {
                    msg.clear();
                    let r = if k < 4 {
                        v.emit(&book, &p, &IdentityEnv, &mut msg)
                    } else {
                        v.emit_region(&book, &p, 8, 14, &IdentityEnv, &mut msg).map(|_| ())
                    };
                    match r {
                        Ok(()) => break,
                        Err(ViewError::NeedMeasures) => {
                            v.measure.satisfy(&Fake);
                            let n = v.dml_needs();
                            if std::env::var_os("UO_VD_DEBUG").is_some() {
                                eprintln!("sheet {si} state {k} round {round}: {} dml requests, first {:?}", n.len(), n.first());
                            }
                            stub::answer(&mut v, &n);
                        }
                        Err(e) => {
                            eprintln!("sheet {si} state {k}: {e:?}");
                            break;
                        }
                    }
                }
            }
            u32le(&mut out, si);
            u32le(&mut out, k);
            let spans = v.drawing_spans().to_vec();
            u32le(&mut out, spans.len());
            for (s, e) in spans {
                u32le(&mut out, s as usize);
                u32le(&mut out, e as usize);
            }
            u32le(&mut out, msg.len());
            out.extend_from_slice(&msg);
            let res = v.resources();
            let now = res.lens()[4];
            u32le(&mut out, now.saturating_sub(sent));
            for i in sent..now {
                let h = u32::try_from(i).unwrap_or(0);
                let d: &[u8] = res.bitmap_bytes(h).map_or(&[], |d| &d[..]);
                let flag = if res.is_recipe(h) { 0x8000_0000u32 } else { 0 };
                out.extend_from_slice(&(u32::try_from(d.len()).unwrap_or(0) | flag).to_le_bytes());
                out.extend_from_slice(d);
            }
            sent = now.max(sent);
        }
    }
    std::fs::write(&a[2], &out).unwrap_or_else(|e| {
        eprintln!("{}: {e}", a[2]);
        std::process::exit(2)
    });
}
