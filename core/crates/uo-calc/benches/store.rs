//! S0 benchmarks (record `uo-calc.md` "Performance budgets", rows S0), native:
//!
//!   cargo bench -p uo-calc --bench store
//!
//! - store fill giant-1m from decoded lanes (≤ 40 ms): 10,000 × 100 integers streamed row-major through the
//!   reader's fast path (`SheetLoader::cell_raw`), as C1a will;
//! - column scan sum 1M (≤ 1 ms): `column_runs` + `fold_numbers` over one column of 1,000,000 integers;
//! - random get 1M (≤ 15 ms): 1,000,000 `Store::get` at seeded random positions of giant-1m;
//! - viewport gather 45×15 (≤ 20 µs) and 120×40 (≤ 120 µs): `Store::gather` at random origins of giant-1m.
//!
//! Medians of the rounds; one process, nothing else running. Informational rows: the model path
//! (`SheetLoader::cell` with a `FileCell` per cell) and giant-200k (inline strings).
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag, Viewport};
use uo_calc::{JsString, SheetIx, Store};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Median wall time of `f` in milliseconds over `rounds` (after one warm-up).
fn time_ms<R>(rounds: usize, mut f: impl FnMut() -> R) -> f64 {
    black_box(f());
    median(
        (0..rounds)
            .map(|_| {
                let t = Instant::now();
                black_box(f());
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect(),
    )
}

fn num(x: f64) -> RawCell {
    RawCell { kind: CellKind::new(ValueTag::None, CellKind::HAS_VALUE), payload: x.to_bits(), style: 0 }
}

fn fill_giant_1m() -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    for r in 0..10_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            l.cell_raw(c, num(f64::from((r + 1) * 1000 + c + 1))).unwrap();
        }
    }
    l.finish().unwrap();
    s
}

fn fill_giant_1m_model() -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    for r in 0..10_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            let v = ((r + 1) * 1000 + c + 1).to_string();
            l.cell(c, &FileCell { v: Some(JsString::from_str_js(&v)), ..FileCell::default() }).unwrap();
        }
    }
    l.finish().unwrap();
    s
}

fn fill_giant_200k() -> Store {
    let mut s = Store::new();
    let sheet = s.add_sheet().unwrap();
    let mut l = s.load_sheet(sheet).unwrap();
    for r in 0..2_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            if (c + 1).is_multiple_of(7) {
                let text = JsString::from_str_js(&format!("Texto {}-{}", r + 1, c + 1));
                let cell = FileCell {
                    t: Some(JsString::from_str_js("inlineStr")),
                    s: Some(0.0),
                    is: Some(InlineText { text: Some(text), runs: None }),
                    ..FileCell::default()
                };
                l.cell(c, &cell).unwrap();
            } else {
                l.cell_raw(c, num(f64::from((r + 1) * 1000 + c + 1))).unwrap();
            }
        }
    }
    l.finish().unwrap();
    s
}

struct Xs(u32);
impl Xs {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

fn main() {
    let mut rows: Vec<(&str, f64, &str, f64)> = Vec::new();

    let fill = time_ms(7, fill_giant_1m);
    rows.push(("store fill giant-1m from decoded lanes (cell_raw)", fill, "ms", 40.0));
    let fill_model = time_ms(5, fill_giant_1m_model);
    rows.push(("store fill giant-1m from model cells (cell + FileCell), info", fill_model, "ms", f64::NAN));
    let fill200 = time_ms(5, fill_giant_200k);
    rows.push(("store fill giant-200k (28,000 inline strings), info", fill200, "ms", f64::NAN));

    // one column of 1,000,000 integers
    let mut col = Store::new();
    let sh = col.add_sheet().unwrap();
    {
        let mut l = col.load_sheet(sh).unwrap();
        for r in 0..1_000_000u32 {
            l.row(r, RowMeta::default()).unwrap();
            l.cell_raw(0, num(f64::from(r % 1000))).unwrap();
        }
        l.finish().unwrap();
    }
    let scan = time_ms(31, || col.column_runs(sh, 0, 0, 999_999).fold(0.0, |a, run| run.fold_numbers(a, |a, x| a + x)));
    rows.push(("column scan sum 1M", scan, "ms", 1.0));
    let scan_get = time_ms(11, || {
        let mut a = 0.0;
        for r in 0..1_000_000u32 {
            if let Some(c) = col.get(sh, r, 0) {
                a += c.number();
            }
        }
        a
    });
    rows.push(("column sum 1M by get(), info", scan_get, "ms", f64::NAN));

    let g = fill_giant_1m();
    let sheet = SheetIx(0);
    let mut rng = Xs(0x9e37_79b9);
    let pos: Vec<(u32, u32)> = (0..1_000_000).map(|_| (rng.next() % 10_000, rng.next() % 100)).collect();
    let get = time_ms(11, || {
        let mut acc = 0u64;
        for &(r, c) in &pos {
            if let Some(x) = g.get(sheet, black_box(r), black_box(c)) {
                acc ^= x.payload;
            }
        }
        acc
    });
    rows.push(("random get 1M (giant-1m)", get, "ms", 15.0));

    let mut vp = Viewport::default();
    for (h, w, budget) in [(45u32, 15u32, 20.0), (120, 40, 120.0)] {
        let origins: Vec<(u32, u32)> = (0..1000).map(|_| (rng.next() % (10_000 - h), rng.next() % (100 - w))).collect();
        let mut per = Vec::new();
        for _ in 0..9 {
            let t = Instant::now();
            for &(r, c) in &origins {
                g.gather(sheet, r, r + h - 1, c, c + w - 1, &mut vp);
                black_box(&vp);
            }
            per.push(t.elapsed().as_secs_f64() * 1e6 / origins.len() as f64);
        }
        let name = if h == 45 { "viewport gather 45x15" } else { "viewport gather 120x40" };
        rows.push((name, median(per), "us", budget));
    }

    println!("| Benchmark (native) | median | budget | verdict |");
    println!("|---|---|---|---|");
    for (name, v, unit, budget) in &rows {
        let verdict = if budget.is_nan() {
            "-".to_owned()
        } else if v <= budget {
            "within".to_owned()
        } else {
            "OVER".to_owned()
        };
        let b = if budget.is_nan() { "-".to_owned() } else { format!("{budget} {unit}") };
        println!("| {name} | {v:.3} {unit} | {b} | {verdict} |");
    }
    black_box(&g);
}
