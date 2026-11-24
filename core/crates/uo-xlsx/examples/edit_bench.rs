//! C5b benchmark: the editor session on the benchmark workbooks (T13 rows "edit latency" and "paste / fill / replace-all
//! / undo"), the same scenarios as the TS baseline `js/bench-edit.mjs`.
//!
//! `cargo run -p uo-xlsx --features edit --release --example edit_bench -- sheet/samples/giant-1m.xlsx
//!  core/target/sheet-cap/bench/calc-heavy-20000.xlsx` (`UO_EDITS` edits per series, default 20; `UO_BIG_OPS=1` adds Fill
//! Down over 100,000 cells). Live bytes and the peak come from the counting allocator.
#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::cast_precision_loss)]

use std::time::Instant;

use uo_calc::{Clock, RandomSource};
use uo_xlsx::session::{FindOptions, Session, SessionOpts};

fn live() -> i64 {
    uo_alloc::thread_stats().live
}

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((p * s.len() as f64) as usize).min(s.len() - 1)]
}

fn ms<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let t = Instant::now();
    let r = f();
    (t.elapsed().as_secs_f64() * 1000.0, r)
}

fn main() {
    let n: usize = std::env::var("UO_EDITS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    for f in std::env::args().skip(1) {
        let bytes = std::fs::read(&f).unwrap();
        let live0 = live();
        let opts = SessionOpts { clock: Clock { now_serial: 45672.5 }, ..SessionOpts::default() };
        let (open, mut s) = ms(|| Session::open(&bytes, opts, RandomSource::seeded(1)).unwrap());
        s.recalculate();
        let live_open = live() - live0;
        let lat = |s: &mut Session, r: &str, texts: Vec<String>| -> Vec<f64> {
            texts
                .iter()
                .map(|t| {
                    s.select_range(r).unwrap();
                    ms(|| s.set_active_cell_text(&u(t))).0
                })
                .collect()
        };
        lat(&mut s, "A2", vec!["7".into(), "8".into()]);
        lat(&mut s, "Z1", vec!["1".into(), "2".into()]);
        let chain = lat(&mut s, "A2", (0..n).map(|i| (i + 3).to_string()).collect());
        let iso = lat(&mut s, "Z1", (0..n).map(|i| (i + 3).to_string()).collect());
        let formula = lat(&mut s, "Z2", (0..n).map(|i| format!("=Z1*{}", i + 2)).collect());
        println!(
            "{f}: open {open:.1} ms; live after open {:.1} MiB; edit p50/p95 ms: chain head {:.3}/{:.3}, isolated {:.3}/{:.3}, formula {:.3}/{:.3}",
            live_open as f64 / 1048576.0,
            pct(&chain, 0.5),
            pct(&chain, 0.95),
            pct(&iso, 0.5),
            pct(&iso, 0.95),
            pct(&formula, 0.5),
            pct(&formula, 0.95)
        );
        let op = |s: &mut Session, name: &str, f: &dyn Fn(&mut Session)| {
            let before = live();
            uo_alloc::reset_peak();
            let (t, ()) = ms(|| f(s));
            let peak = uo_alloc::thread_stats().peak - before;
            let (tu, _) = ms(|| s.undo());
            let (tr, _) = ms(|| s.redo());
            println!(
                "  {name}: {t:.1} ms (undo {tu:.1}, redo {tr:.1}); peak above start {:.1} MiB; history {:.1} MiB",
                peak as f64 / 1048576.0,
                s.history_bytes() as f64 / 1048576.0
            );
        };
        let heavy = f.contains("calc-heavy");
        op(&mut s, if heavy { "paste 30x30" } else { "paste 100x100" }, &|s| {
            s.select_range(if heavy { "A1:AD30" } else { "A1:CV100" }).unwrap();
            s.copy(false);
            s.select_range("DA1").unwrap();
            s.paste(None, false);
        });
        // every written cell recalculates (the TS `setCellValue` per cell): on calc-heavy, Fill Down of 20 cells of
        // column B (its 20,000 dependents recompute per cell) and of 1,000 cells of an isolated column
        if heavy {
            op(&mut s, "fill down B1:B21 (dependents)", &|s| {
                s.select_range("B1:B21").unwrap();
                s.fill(true);
            });
            op(&mut s, "fill down AA1:AA1000 (isolated)", &|s| {
                s.select_range("AA1").unwrap();
                s.set_active_cell_text(&u("5"));
                s.select_range("AA1:AA1000").unwrap();
                s.fill(true);
            });
        } else {
            op(&mut s, "fill down 10000", &|s| {
                s.select_range("B1:B10000").unwrap();
                s.fill(true);
            });
        }
        op(&mut s, "replaceAll (no hit, every cell scanned)", &|s| {
            s.replace_all(&u("§"), &u("8"), FindOptions::default());
        });
        let (ts, b) = ms(|| s.save().unwrap());
        println!("  save after the edits: {ts:.1} ms, {} bytes", b.len());
        drop(b);
        drop(s);
        uo_numfmt::clear_caches();
        println!("  live after drop and the format cache cleared: {} B", live() - live0);
    }
}
