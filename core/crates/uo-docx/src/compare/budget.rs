//! The work limit of the diff (D5a, declared in the record): the TS algorithms are unbounded (Myers
//! keeps a snapshot of its vector for every step, O((N+M)·D) memory; the histogram diff rebuilds a
//! histogram per recursion and compares every candidate of a bucket, O(N·M·N) on repetitive text). A
//! hostile pair would take unbounded time and memory, so every step is charged to a [`Budget`] and
//! Myers' trace has a cell limit; past either, the range being diffed is answered with the coarse
//! script (common prefix and suffix, the rest deleted and inserted), which is a valid edit script,
//! not a minimal one. Below the limits the result is the TS's, exactly.

use std::cell::Cell;

/// Steps (comparisons and bucket visits) the block-level diff of one pair may make.
pub const DIFF_WORK_LIMIT: u64 = 1 << 26;

/// Cells (`i32`) of Myers' trace for one range at the block level (4 Mi cells = 16 MiB: `D` up to
/// about 2,900 edits).
pub const MYERS_TRACE_CELLS: usize = 4 << 20;

/// Steps the word-level refinement of all the changed paragraph pairs of one diff may make.
pub const INTRA_WORK_LIMIT: u64 = 1 << 25;

/// Cells of Myers' trace for the words of one paragraph pair (1 Mi cells: `D` up to about 1,400).
pub const INTRA_TRACE_CELLS: usize = 1 << 20;

/// A work budget shared by the algorithms of one diff.
#[derive(Debug)]
pub struct Budget {
    remaining: Cell<u64>,
    fell_back: Cell<bool>,
}

impl Budget {
    #[must_use]
    pub fn new(limit: u64) -> Budget {
        Budget { remaining: Cell::new(limit), fell_back: Cell::new(false) }
    }

    /// Charges `n` steps (saturating at the limit).
    pub fn spend(&self, n: u64) {
        self.remaining.set(self.remaining.get().saturating_sub(n));
    }

    /// Whether no step is left.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.remaining.get() == 0
    }

    /// Records that a result is the coarse one.
    pub fn mark_fallback(&self) {
        self.fell_back.set(true);
    }

    /// Whether any result was the coarse one.
    #[must_use]
    pub fn fell_back(&self) -> bool {
        self.fell_back.get()
    }

    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.remaining.get()
    }
}
