//! No panic on any sequence of calls (the `xlsx_write` fuzz target runs the same interpreter,
//! `tests/support/ops.rs`): seeded random byte strings, bounded to seconds. A finished workbook
//! must be a valid package. `UO_XLSX_NOPANIC_N` raises the count for a long local run.
#![allow(clippy::unwrap_used)]

mod support;

#[test]
fn random_call_sequences() {
    let n: u32 = std::env::var("UO_XLSX_NOPANIC_N").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
    let mut seed = 0x9E37_79B9u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let (mut finished, mut bytes) = (0u32, 0usize);
    for i in 0..n {
        let len = 16 + (next() % 2048) as usize;
        let data: Vec<u8> = (0..len).map(|_| (next() >> 7) as u8).collect();
        // a valid start every few cases: a sheet opened early reaches rows, styles and merges
        let data = if i % 3 == 0 { [&[0u8, 1, 0, b'A'][..], &data].concat() } else { data };
        if let Some(b) = support::ops::run(&data) {
            finished += 1;
            bytes += b.len();
        }
    }
    println!("{n} sequences, {finished} finished workbooks, {bytes} bytes");
    assert!(finished > n / 10, "the generator rarely reaches finish: {finished}");
}
