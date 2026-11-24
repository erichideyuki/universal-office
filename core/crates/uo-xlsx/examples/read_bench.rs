//! C1a open benchmark (budgets of record T13): `Book::open` (zip directory, workbook, styles, theme, shared-string
//! index, the active sheet streamed into the store) + the first viewport gather, time and memory.
//!
//!   cargo run --release -p uo-xlsx --features read --example read_bench -- <file.xlsx>... [--rounds N] [--all]
//! `--all` loads every sheet too (the 20-sheet row of the budgets).
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;
use uo_calc::key::SheetIx;
use uo_calc::store::Viewport;
use uo_xlsx::book::{Book, ReadLimits};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = args.iter().position(|a| a == "--rounds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    let all = args.iter().any(|a| a == "--all");
    let files: Vec<&String> = args.iter().filter(|a| a.ends_with(".xlsx")).collect();
    assert!(uo_alloc::is_counting(), "build with uo-alloc's counting allocator (dev-dependency, feature global)");
    for f in files {
        let bytes = std::fs::read(f).unwrap();
        let mut times = Vec::new();
        let mut view_ns = Vec::new();
        for _ in 0..rounds {
            let copy = bytes.clone();
            let t = Instant::now();
            let mut book = Book::open_owned(copy, ReadLimits::default()).unwrap();
            if all {
                book.load_all().unwrap();
            }
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            let mut vp = Viewport::default();
            let t = Instant::now();
            book.store.gather(SheetIx(0), 0, 44, 0, 14, &mut vp);
            view_ns.push(t.elapsed().as_secs_f64() * 1e9);
            std::hint::black_box(&book);
        }
        times.sort_by(f64::total_cmp);
        view_ns.sort_by(f64::total_cmp);
        // memory: one open with the counting allocator
        let copy = bytes.clone();
        let (book, usage) = uo_alloc::measure(|| {
            let mut b = Book::open_owned(copy, ReadLimits::default()).unwrap();
            if all {
                b.load_all().unwrap();
            }
            b
        });
        // phases of the active sheet's part: inflate alone, inflate + tokenize
        {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let part = "xl/worksheets/sheet1.xml";
            let t = Instant::now();
            let mut n = 0usize;
            let mut ps = pkg.stream_part(part).unwrap().unwrap();
            while let Some(c) = ps.next_chunk().unwrap() {
                n += c.len();
            }
            let inflate = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut ps = pkg.stream_part(part).unwrap().unwrap();
            let mut pull = uo_opc::xml::pull::Pull::new();
            let mut tokens = 0usize;
            while let Some(c) = ps.next_chunk().unwrap() {
                pull.feed(c, false, |_| {
                    tokens += 1;
                    Ok(uo_opc::xml::pull::Flow::Continue)
                })
                .unwrap();
            }
            println!(
                "  phases: {n} bytes inflated in {inflate:.1} ms; inflate + tokenize {:.1} ms ({tokens} tokens)",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        let cells = book.store.cell_count();
        let lanes = book.store.lane_bytes();
        let name = std::path::Path::new(f).file_name().unwrap().to_string_lossy();
        println!(
            "{name}: input {} B, {cells} cells; open median {:.1} ms (min {:.1}, max {:.1}); first viewport gather {:.1} us; peak above start {} B; retained {} B = store lanes {} + {} B other",
            bytes.len(),
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            view_ns[view_ns.len() / 2] / 1000.0,
            usage.peak_above_start,
            usage.live_delta,
            lanes,
            usage.live_delta - i64::try_from(lanes).unwrap()
        );
        drop(book);
    }
}
