//! C2 benchmark (budgets of record T13, row ".xls conversion"): `uo_xlsx::xls::convert` and `Book::open_xls` (convert + the normal reader
//! over the converted parts, every sheet loaded), time and memory (the counting allocator of `uo-alloc`).
//!
//!   cargo run --release -p uo-xlsx --features xls --example xls_bench -- <file.xls>... [--rounds N]
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;
use uo_xlsx::book::{Book, ReadLimits};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = args.iter().position(|a| a == "--rounds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    assert!(uo_alloc::is_counting(), "build with uo-alloc's counting allocator (dev-dependency, feature global)");
    for f in args.iter().filter(|a| a.ends_with(".xls")) {
        let bytes = std::fs::read(f).unwrap();
        let (mut conv, mut open) = (Vec::new(), Vec::new());
        for _ in 0..rounds {
            let t = Instant::now();
            let p = uo_xlsx::xls::convert(&bytes).unwrap();
            conv.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(&p);
            drop(p);
            let t = Instant::now();
            let mut b = Book::open_xls(&bytes, ReadLimits::default()).unwrap();
            b.load_all().unwrap();
            open.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(&b);
        }
        conv.sort_by(f64::total_cmp);
        open.sort_by(f64::total_cmp);
        let (parts, uc) = uo_alloc::measure(|| uo_xlsx::xls::convert(&bytes).unwrap());
        let out_bytes: usize = parts.items.iter().map(|(_, d)| d.len()).sum();
        drop(parts);
        let (book, uo) = uo_alloc::measure(|| {
            let mut b = Book::open_xls(&bytes, ReadLimits::default()).unwrap();
            b.load_all().unwrap();
            b
        });
        drop(book);
        let mb = |x: u64| x as f64 / 1_048_576.0;
        println!(
            "{f}: input {:.2} MiB, converted {:.2} MiB | convert median {:.1} ms (min {:.1}); peak above start {:.1} MiB, retained {:.1} MiB | convert+open+load_all median {:.1} ms (min {:.1}); peak {:.1} MiB, retained {:.1} MiB",
            bytes.len() as f64 / 1_048_576.0,
            out_bytes as f64 / 1_048_576.0,
            conv[conv.len() / 2],
            conv[0],
            mb(uc.peak_above_start.unsigned_abs()),
            mb(uc.live_delta.unsigned_abs()),
            open[open.len() / 2],
            open[0],
            mb(uo.peak_above_start.unsigned_abs()),
            mb(uo.live_delta.unsigned_abs())
        );
    }
}
