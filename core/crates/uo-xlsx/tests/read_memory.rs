//! Memory gate of the C1a reader (`--features read`). M1: opening a workbook, loading every sheet and decoding
//! every shared string leaves nothing behind once the book is dropped (every committed workbook, and the error
//! paths: truncated and bit-flipped packages). M2: opening and dropping the same workbook 300 times is flat.
//! M3: the budgets of record T13 on the giants (opt-in `UO_XLSX_BIG=1`, release): the store lanes are what the
//! open retains.
#![cfg(feature = "read")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_xlsx::book::{Book, ReadLimits};

fn x1_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks/x1"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn open_all(bytes: &[u8]) -> bool {
    let Ok(mut b) = Book::open(bytes, ReadLimits::default()) else { return false };
    let _ = b.load_all();
    for i in 0..b.sst.as_ref().map_or(0, uo_xlsx::read::sst::SstIndex::len) {
        let _ = b.resolve_sst(i as u32);
    }
    true
}

#[test]
fn m1_open_load_drop_leaves_nothing() {
    let mut n = 0;
    for f in x1_files() {
        let bytes = std::fs::read(&f).unwrap();
        assert_no_leak(|| assert!(open_all(&bytes), "{}", f.display()));
        n += 1;
    }
    for name in ["demo.xlsx", "smoke.xlsx", "merge-smoke.xlsx", "gaps-smoke.xlsx", "showcase.xlsx"] {
        if let Ok(bytes) = std::fs::read(samples_dir().join(name)) {
            assert_no_leak(|| assert!(open_all(&bytes), "{name}"));
            n += 1;
        }
    }
    assert!(n >= 30);
}

#[test]
fn m1_error_paths_leave_nothing() {
    let files = x1_files();
    let mut rng = 0x9E37_79B9u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let mut failed = 0;
    for f in files.iter().take(12) {
        let bytes = std::fs::read(f).unwrap();
        for k in 0..6 {
            let mut b = bytes.clone();
            match k {
                0 => b.truncate(b.len() / 2),
                1 => b.truncate(b.len().saturating_sub(30)),
                _ => {
                    for _ in 0..8 {
                        let at = (next() as usize) % b.len();
                        b[at] ^= (next() & 0xFF) as u8 | 1;
                    }
                }
            }
            assert_no_leak(|| {
                if !open_all(&b) {
                    failed += 1;
                }
            });
        }
    }
    assert!(failed > 0, "some of the damaged packages must be refused");
}

#[test]
fn m2_open_drop_soak_is_flat() {
    let bytes = std::fs::read(&x1_files()[3]).unwrap();
    let series = soak(300, 20, |_| {
        assert!(open_all(&bytes));
    });
    assert_flat(&series, 3, 0);
}

#[test]
fn m3_budgets_on_the_giants() {
    if !uo_testkit_flag("UO_XLSX_BIG") {
        eprintln!("SKIPPED: giants: set UO_XLSX_BIG=1 (release)");
        return;
    }
    for (name, per_cell_max, extra_max) in [("giant-1m.xlsx", 6.0, 64 * 1024), ("giant-200k.xlsx", 16.0, 8 << 20)] {
        let Ok(bytes) = std::fs::read(samples_dir().join(name)) else { continue };
        let (book, usage) = measure(|| Book::open_owned(bytes, ReadLimits::default()).unwrap());
        let cells = book.store.cell_count() as f64;
        let lanes = book.store.lane_bytes() as f64;
        let per_cell = usage.live_delta as f64 / cells;
        eprintln!(
            "{name}: {} cells; retained {} B = {:.2} B/cell (lanes {:.2}); peak above start {} B; allocated {} B",
            book.store.cell_count(),
            usage.live_delta,
            per_cell,
            lanes / cells,
            usage.peak_above_start,
            usage.allocated
        );
        assert!(per_cell <= per_cell_max, "{name}: {per_cell} B/cell");
        // retained beyond the store's lanes (strings pool included for the 200k)
        if name == "giant-1m.xlsx" {
            assert!(usage.live_delta as f64 - lanes <= f64::from(extra_max), "{name}");
        }
        drop(book);
    }
}

fn uo_testkit_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}
