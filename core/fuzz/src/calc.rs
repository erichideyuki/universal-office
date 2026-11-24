//! Fuzz target of the `uo-calc` cell store (record `core/docs/modules/uo-calc.md` "Security and fuzz targets",
//! `store_ops`): the bytes are a sequence of store operations (file cells of every type and hostile text, removes,
//! engine writes, styles, owned bits, dense runs across chunk boundaries, gathers and column runs, compaction)
//! interpreted by `crates/uo-calc/tests/support/ops.rs`, shared with the seeded test, and checked differentially
//! against a `BTreeMap` model: same file cells, same `cellValue`, same iteration; invariants (sorted non-empty
//! chunks, lane lengths, side-table bits, pool reference counts = holders) every 64 operations.

#[path = "../../crates/uo-calc/tests/support/ops.rs"]
mod ops;

/// Seeds: empty, each operation once, a dense run over a chunk boundary, and every byte.
pub fn calc_seeds() -> Vec<Vec<u8>> {
    let mut out = vec![vec![], vec![0, 0, 0, 1, 2, 3]];
    for op in 0u8..10 {
        out.push(vec![op, 4, 1, 3, 5, 7, 9, op, 2, 4]);
    }
    out.push(vec![7, 3, 0, 200, 8, 3, 0, 9, 0, 0, 0]);
    out.push((0..=255u8).cycle().take(2048).collect());
    out
}

pub fn store_ops(data: &[u8]) {
    let _ = ops::run(data);
}
