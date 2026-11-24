//! C1b save benchmark (budgets of record T13): one edit, then the surgical save of the edited sheet — time, peak
//! memory above the start and the output size (the TS: `saveXlsx` after `setActiveCellText`, `tools/bench/speed/sheet.mjs`
//! scenario `save`).
//!
//!   cargo run --release -p uo-xlsx --features save --example save_bench -- <file.xlsx>... [--rounds N]
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;

use uo_calc::engine::{CellInput, EngineOptions};
use uo_calc::key::SheetIx;
use uo_calc::value::Scalar;
use uo_calc::{Clock, RandomSource, ValueLocale};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::save::engine::build_engine;
use uo_xlsx::save::{SaveRequest, save};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = args.iter().position(|a| a == "--rounds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    let files: Vec<&String> = args.iter().filter(|a| a.ends_with(".xlsx")).collect();
    assert!(uo_alloc::is_counting(), "build with uo-alloc's counting allocator (dev-dependency, feature global)");
    for f in files {
        let bytes = std::fs::read(f).unwrap();
        let in_len = bytes.len();
        let open_t = Instant::now();
        let mut book = Book::open_owned(bytes, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let open_ms = open_t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        build_engine(&mut book, &EngineOptions::new(ValueLocale::pt_br())).unwrap();
        let engine_ms = t.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        let mut out_len = 0usize;
        let mut peak = 0usize;
        for r in 0..rounds + 1 {
            // the edit (not timed, as in the TS scenario): B2 = 200 + r through the engine
            let e = book.engine.as_mut().unwrap();
            e.set_cell(&mut book.store, SheetIx(0), 1, 1, Some(CellInput::Value(Scalar::Num(200.0 + r as f64)))).unwrap();
            e.calculate(&mut book.store, Clock { now_serial: 45672.5 }, &mut RandomSource::sequence(Vec::new())).unwrap();
            let t = Instant::now();
            let (saved, usage) =
                uo_alloc::measure(|| save(&mut book, &SaveRequest { dirty_sheets: &[0], ..SaveRequest::default() }).unwrap());
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            out_len = saved.bytes.len();
            peak = peak.max(usize::try_from(usage.peak_above_start).unwrap_or(0));
            if r > 0 {
                times.push(ms);
            }
        }
        times.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        println!(
            "{f}: input {in_len} B, load {open_ms:.0} ms, engine {engine_ms:.0} ms; save after one edit: median {median:.1} ms (min {:.1}, max {:.1}), output {out_len} B, peak above start {:.2} MiB (output + {:.2} MiB)",
            times[0],
            times[times.len() - 1],
            peak as f64 / 1048576.0,
            (peak as f64 - out_len as f64) / 1048576.0
        );
    }
}
