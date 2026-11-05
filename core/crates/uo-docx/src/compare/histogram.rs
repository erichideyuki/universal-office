//! Histogram diff over OPAQUE TOKENS (compare `histogram.ts`, a port of OpenCompare's `diff-core`,
//! after git's `xdiff/xhistogram.c`).
//!
//! For a range `[a0, a1) × [b0, b1)` it builds a histogram of the occurrences of the tokens of side A,
//! scans side B for the token rarest in A that also occurs in B, takes that pair as a PARTITION ANCHOR,
//! extends it to the longest contiguous run and recurses on the partitions before and after. Without a
//! usable anchor it falls back to Myers, which is always correct.
//!
//! The recursion is an explicit stack (the depth is up to the number of blocks). Every step is
//! charged to the [`Budget`]; a range that cannot be finished within the budget (or whose Myers
//! trace is over the limit) is answered with the coarse script, see [`super::budget`]. The keys are
//! interned ids: tokens that compare equal under `eq` have the same key, as the TS requires of
//! `getKeyA`/`getKeyB`.

use std::collections::HashMap;

use super::budget::Budget;
use super::myers::{DiffOp, EqFn, all_deletes, all_inserts, coarse_script, myers_diff};

enum Task {
    Range { a0: usize, a1: usize, b0: usize, b1: usize },
    Equal { a: usize, b: usize, len: usize },
}

struct Best {
    count: usize,
    span: usize,
    shift: usize,
    a_l: usize,
    a_r: usize,
    b_l: usize,
}

/// Runs the histogram diff and returns the canonical edit script (equal/delete/insert in scan
/// order). `key_a(i)` and `key_b(j)` must be equal for two tokens that compare equal under `eq`.
#[must_use]
pub fn histogram_diff(
    n: usize,
    m: usize,
    eq: EqFn<'_>,
    key_a: &dyn Fn(usize) -> u32,
    key_b: &dyn Fn(usize) -> u32,
    budget: &Budget,
    trace_cells: usize,
) -> Vec<DiffOp> {
    let mut out = Vec::new();
    if n == 0 && m == 0 {
        return out;
    }
    if n == 0 {
        all_inserts(0, m, &mut out);
        return out;
    }
    if m == 0 {
        all_deletes(0, n, &mut out);
        return out;
    }
    let mut stack = vec![Task::Range { a0: 0, a1: n, b0: 0, b1: m }];
    while let Some(task) = stack.pop() {
        match task {
            Task::Equal { a, b, len } => {
                for off in 0..len {
                    out.push(DiffOp::equal(a + off, b + off));
                }
            }
            Task::Range { a0, a1, b0, b1 } => range(a0, a1, b0, b1, eq, key_a, key_b, budget, trace_cells, &mut stack, &mut out),
        }
    }
    out
}

/// One step of the driver for the range `[a0, a1) × [b0, b1)`: emits its ops, or pushes its parts.
#[allow(clippy::too_many_arguments, reason = "the TS `recurse` takes the same")]
fn range(
    a0: usize,
    a1: usize,
    b0: usize,
    b1: usize,
    eq: EqFn<'_>,
    key_a: &dyn Fn(usize) -> u32,
    key_b: &dyn Fn(usize) -> u32,
    budget: &Budget,
    trace_cells: usize,
    stack: &mut Vec<Task>,
    out: &mut Vec<DiffOp>,
) {
    // fast paths
    if a0 >= a1 && b0 >= b1 {
        return;
    }
    if a0 >= a1 {
        all_inserts(b0, b1, out);
        return;
    }
    if b0 >= b1 {
        all_deletes(a0, a1, out);
        return;
    }
    if budget.exhausted() {
        budget.mark_fallback();
        coarse_script(a0, a1, b0, b1, eq, out);
        return;
    }

    // no greedy trim of the prefix and suffix (xhistogram.c does not either: with duplicated tokens
    // it pairs the wrong copies); the anchor absorbs them through the longest contiguous run

    // histogram of the occurrences of A[a0..a1)
    let mut hist_a: HashMap<u32, Vec<usize>> = HashMap::new();
    for i in a0..a1 {
        hist_a.entry(key_a(i)).or_default().push(i);
    }
    budget.spend(u64::try_from(a1 - a0).unwrap_or(u64::MAX));

    // scan B in order for the best partition anchor
    let mut best: Option<Best> = None;
    for j in b0..b1 {
        budget.spend(1);
        let Some(bucket) = hist_a.get(&key_b(j)) else { continue };
        let count = bucket.len();
        for &a_idx in bucket {
            if budget.exhausted() {
                budget.mark_fallback();
                coarse_script(a0, a1, b0, b1, eq, out);
                return;
            }
            budget.spend(1);
            // `eq` is stronger than the key (the bytes of the images against their hash): a pair
            // that is not equal is no anchor
            if !eq(a_idx, j) {
                continue;
            }
            // extend the run backwards
            let (mut a_l, mut b_l) = (a_idx, j);
            // every comparison is charged BEFORE it is made, the one that ends the run too (charging only the
            // successes let a hostile input make up to three comparisons per step)
            while a_l > a0 && b_l > b0 {
                budget.spend(1);
                if !eq(a_l - 1, b_l - 1) {
                    break;
                }
                a_l -= 1;
                b_l -= 1;
            }
            // and forwards
            let (mut a_r, mut b_r) = (a_idx + 1, j + 1);
            while a_r < a1 && b_r < b1 {
                budget.spend(1);
                if !eq(a_r, b_r) {
                    break;
                }
                a_r += 1;
                b_r += 1;
            }
            let span = a_r - a_l;
            let shift = a_l.abs_diff(b_l);
            // order of selection: (1) the RAREST token (smallest count in A); (2) the LONGEST run among
            // equally rare candidates; (3) the SMALLEST positional shift; (4) the smallest bL; (5) the
            // smallest aL
            let better = match &best {
                None => true,
                Some(b) => {
                    if count != b.count {
                        count < b.count
                    } else if span != b.span {
                        span > b.span
                    } else if shift != b.shift {
                        shift < b.shift
                    } else if b_l != b.b_l {
                        b_l < b.b_l
                    } else {
                        a_l < b.a_l
                    }
                }
            };
            if better {
                best = Some(Best { count, span, shift, a_l, a_r, b_l });
            }
        }
    }

    let Some(best) = best else {
        // no anchor: Myers, which is correct on any input
        let eq_off = |i: usize, j: usize| eq(a0 + i, b0 + j);
        match myers_diff(a1 - a0, b1 - b0, &eq_off, budget, trace_cells) {
            Some(ops) => {
                out.extend(ops.into_iter().map(|op| DiffOp {
                    op: op.op,
                    a_index: op.a_index.map(|i| i + a0),
                    b_index: op.b_index.map(|j| j + b0),
                }));
            }
            None => {
                budget.mark_fallback();
                coarse_script(a0, a1, b0, b1, eq, out);
            }
        }
        return;
    };

    // before the anchor, the anchor's run, after the anchor (in this order of output)
    let b_right = best.b_l + (best.a_r - best.a_l);
    stack.push(Task::Range { a0: best.a_r, a1, b0: b_right, b1 });
    stack.push(Task::Equal { a: best.a_l, b: best.b_l, len: best.a_r - best.a_l });
    stack.push(Task::Range { a0, a1: best.a_l, b0, b1: best.b_l });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::myers::DiffOpKind;

    fn run(a: &[u32], b: &[u32], budget: &Budget, cells: usize) -> Vec<DiffOp> {
        let eq = |i: usize, j: usize| a[i] == b[j];
        histogram_diff(a.len(), b.len(), &eq, &|i| a[i], &|j| b[j], budget, cells)
    }

    fn consumed(ops: &[DiffOp]) -> (usize, usize) {
        let a = ops.iter().filter(|o| o.op != DiffOpKind::Insert).count();
        let b = ops.iter().filter(|o| o.op != DiffOpKind::Delete).count();
        (a, b)
    }

    #[test]
    fn anchors_on_the_rarest_token() {
        let a = [1, 2, 3, 4, 5];
        let b = [1, 2, 9, 4, 5];
        let ops = run(&a, &b, &Budget::new(u64::MAX), 1 << 20);
        let equal: Vec<_> = ops.iter().filter(|o| o.op == DiffOpKind::Equal).map(|o| o.a_index.unwrap()).collect();
        assert_eq!(equal, [0, 1, 3, 4]);
        assert_eq!(consumed(&ops), (5, 5));
    }

    #[test]
    fn repetitive_input_finishes_within_the_budget() {
        let a = vec![7u32; 3000];
        let mut b = vec![7u32; 3000];
        b[1500] = 8;
        let budget = Budget::new(1 << 22);
        let ops = run(&a, &b, &budget, 1 << 20);
        assert_eq!(consumed(&ops), (3000, 3000));
        assert!(budget.fell_back() || ops.len() >= 3000);
    }

    #[test]
    fn deep_recursion_is_iterative() {
        // every anchor is at the edge: a recursion as deep as the input
        let a: Vec<u32> = (0..4_000).collect();
        let b: Vec<u32> = (0..4_000).filter(|x| x % 2 == 0).collect();
        let ops = run(&a, &b, &Budget::new(1 << 30), 1 << 20);
        assert_eq!(consumed(&ops), (4_000, 2_000));
    }

    #[test]
    fn exhausted_budget_gives_the_coarse_script() {
        let a: Vec<u32> = (0..100).collect();
        let b: Vec<u32> = (100..200).collect();
        let budget = Budget::new(0);
        let ops = run(&a, &b, &budget, 1 << 20);
        assert_eq!(consumed(&ops), (100, 100));
        assert!(budget.fell_back());
    }
}
