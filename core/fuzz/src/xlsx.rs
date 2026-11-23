//! Fuzz target of `uo-xlsx` (record `core/docs/modules/uo-xlsx.md`): the bytes are a sequence of
//! writer calls (styles, sheets with options and names, rows of every cell kind with links,
//! merges, sheet ends) interpreted by `crates/uo-xlsx/tests/support/ops.rs`, shared with the
//! seeded no-panic test. Beyond "no panic": a finished workbook is a valid package (entries
//! inflate to their CRC, every part parses in strict XML mode, every part has its override).

#[path = "../../crates/uo-xlsx/tests/support/ops.rs"]
mod ops;

/// Seeds: a workbook with one sheet and rows of each kind, and the hostile corners (an empty
/// script, a sheet never ended, names and texts made of escapes).
pub fn xlsx_seeds() -> Vec<Vec<u8>> {
    let mut out = vec![vec![], vec![0], vec![4, 1, 0, b'A']];
    let mut s = vec![0u8, 1, 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for k in 0..40u8 {
        s.extend_from_slice(&[2, k % 9, k & 1, k, 6, 3, 13, 22, 9, 1, k, 2, 0x40, k, k, k, k, 0, 0, 0, 0, 5, k, k, 1, 1]);
    }
    s.extend_from_slice(&[6, 1, 1, 0, 0, 7]);
    out.push(s.clone());
    s[0] = 4; // inline strings
    out.push(s);
    out.push((0..=255u8).cycle().take(4096).collect());
    out
}

pub fn xlsx_write(data: &[u8]) {
    let _ = ops::run(data);
}
