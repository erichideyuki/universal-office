//! Myers O(ND) with backtracking over OPAQUE TOKENS (compare `myers.ts`, a port of OpenCompare's
//! `diff-core`), Eugene W. Myers, "An O(ND) Difference Algorithm and Its Variations" (1986).
//!
//! The TS keeps a snapshot of the whole vector `V` at the start of every step `D`; the backtrack only
//! ever reads, at step `d`, the diagonals of parity `d − 1` inside `[−(d−1), d−1]` (the ones step
//! `d − 1` wrote), so the snapshot here is those `d` values: the same script with O(D²) memory instead
//! of O(D·(N+M)). The trace is limited to a number of cells and every step is charged to the
//! [`Budget`]; past either limit [`myers_diff`] answers `None` and the caller falls back to the coarse
//! script (see [`super::budget`]).

use super::budget::Budget;

/// One operation of an edit script (`DiffOp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOpKind {
    /// consumes `a[i]` and `b[j]`
    Equal,
    /// consumes `a[i]`
    Delete,
    /// consumes `b[j]`
    Insert,
}

/// `{ op, aIndex, bIndex }`: `a_index` is `None` for an insert and `b_index` for a delete (the TS
/// `-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOp {
    pub op: DiffOpKind,
    pub a_index: Option<usize>,
    pub b_index: Option<usize>,
}

impl DiffOp {
    #[must_use]
    pub fn equal(i: usize, j: usize) -> DiffOp {
        DiffOp { op: DiffOpKind::Equal, a_index: Some(i), b_index: Some(j) }
    }

    #[must_use]
    pub fn delete(i: usize) -> DiffOp {
        DiffOp { op: DiffOpKind::Delete, a_index: Some(i), b_index: None }
    }

    #[must_use]
    pub fn insert(j: usize) -> DiffOp {
        DiffOp { op: DiffOpKind::Insert, a_index: None, b_index: Some(j) }
    }
}

/// `EqFn`.
pub type EqFn<'a> = &'a dyn Fn(usize, usize) -> bool;

/// All deletes of `a[i0..i1]`.
pub(super) fn all_deletes(i0: usize, i1: usize, out: &mut Vec<DiffOp>) {
    out.extend((i0..i1).map(DiffOp::delete));
}

/// All inserts of `b[j0..j1]`.
pub(super) fn all_inserts(j0: usize, j1: usize, out: &mut Vec<DiffOp>) {
    out.extend((j0..j1).map(DiffOp::insert));
}

/// The coarse script of `a[a0..a1]` against `b[b0..b1]`: the common prefix and suffix (by `eq`), the
/// middle deleted then inserted. Linear in the range; the answer past a limit.
pub(super) fn coarse_script(a0: usize, a1: usize, b0: usize, b1: usize, eq: EqFn<'_>, out: &mut Vec<DiffOp>) {
    let (mut i, mut j) = (a0, b0);
    while i < a1 && j < b1 && eq(i, j) {
        out.push(DiffOp::equal(i, j));
        i += 1;
        j += 1;
    }
    let (mut ea, mut eb) = (a1, b1);
    while ea > i && eb > j && eq(ea - 1, eb - 1) {
        ea -= 1;
        eb -= 1;
    }
    all_deletes(i, ea, out);
    all_inserts(j, eb, out);
    while ea < a1 && eb < b1 {
        out.push(DiffOp::equal(ea, eb));
        ea += 1;
        eb += 1;
    }
}

/// The largest `D` whose trace (`D·(D+1)/2` cells) fits `cells`.
fn max_d_for(cells: usize, cap: usize) -> usize {
    // `cap` is the longest script there is (`n + m`): with no cap a limit of `usize::MAX` cells overflowed
    // `total` (a panic in debug, a loop that never ends in release)
    let mut d = 0usize;
    let mut total = 0usize;
    while d < cap && total.saturating_add(d + 1) <= cells {
        d += 1;
        total = total.saturating_add(d);
    }
    d
}

/// Runs Myers O(ND) and returns the canonical edit script (equal/delete/insert in scan order), or
/// `None` when the work budget or the trace limit (`trace_cells`) is hit. Empty inputs give the empty
/// script.
#[must_use]
pub fn myers_diff(n: usize, m: usize, eq: EqFn<'_>, budget: &Budget, trace_cells: usize) -> Option<Vec<DiffOp>> {
    if n == 0 && m == 0 {
        return Some(Vec::new());
    }
    if n == 0 {
        let mut out = Vec::with_capacity(m);
        all_inserts(0, m, &mut out);
        return Some(out);
    }
    if m == 0 {
        let mut out = Vec::with_capacity(n);
        all_deletes(0, n, &mut out);
        return Some(out);
    }
    let max = n.checked_add(m)?;
    if max >= (1usize << 30) {
        return None;
    }
    let d_cap = max_d_for(trace_cells, max);
    let offset = isize::try_from(d_cap).ok()? + 1;
    let size = usize::try_from(offset).ok()? * 2 + 2;
    let at = |k: isize| usize::try_from(k + offset).unwrap_or(0);
    let (n_i, m_i) = (isize::try_from(n).ok()?, isize::try_from(m).ok()?);
    let mut v = vec![0isize; size];
    let mut snap: Vec<i32> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut found = false;
    'outer: for d in 0..=d_cap {
        let di = isize::try_from(d).ok()?;
        starts.push(snap.len());
        if d >= 1 {
            let mut k = -(di - 1);
            while k < di {
                snap.push(i32::try_from(*v.get(at(k))?).ok()?);
                k += 2;
            }
        }
        if snap.len() > trace_cells {
            return None;
        }
        let mut k = -di;
        while k <= di {
            let mut x = if k == -di {
                *v.get(at(k + 1))?
            } else if k == di {
                *v.get(at(k - 1))? + 1
            } else {
                let down = *v.get(at(k + 1))?;
                let right = *v.get(at(k - 1))? + 1;
                if down > right { down } else { right }
            };
            let mut y = x - k;
            budget.spend(1);
            while x < n_i && y < m_i && y >= 0 {
                budget.spend(1);
                if !eq(usize::try_from(x).ok()?, usize::try_from(y).ok()?) {
                    break;
                }
                x += 1;
                y += 1;
            }
            *v.get_mut(at(k))? = x;
            if x >= n_i && y >= m_i {
                found = true;
                break 'outer;
            }
            if budget.exhausted() {
                return None;
            }
            k += 2;
        }
    }
    if !found {
        return None;
    }

    // the value of V at the start of step `d` for the diagonal `k` (the snapshot)
    let get = |d: usize, k: isize| -> isize {
        if d == 0 {
            return 0;
        }
        let Ok(di) = isize::try_from(d) else { return 0 };
        if k < -(di - 1) || k > di - 1 || (k + di - 1) % 2 != 0 {
            return 0;
        }
        let idx = starts.get(d).copied().unwrap_or(0) + usize::try_from((k + di - 1) / 2).unwrap_or(0);
        snap.get(idx).map_or(0, |&x| isize::try_from(x).unwrap_or(0))
    };
    let mut ops: Vec<DiffOp> = Vec::new();
    let (mut x, mut y) = (n_i, m_i);
    for d in (0..starts.len()).rev() {
        let di = isize::try_from(d).ok()?;
        let k = x - y;
        let prev_k = if k == -di {
            k + 1
        } else if k == di {
            k - 1
        } else {
            let down = get(d, k + 1);
            let right = get(d, k - 1);
            if down > right { k + 1 } else { k - 1 }
        };
        let prev_x = get(d, prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(DiffOp::equal(usize::try_from(x - 1).ok()?, usize::try_from(y - 1).ok()?));
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == prev_x {
                ops.push(DiffOp::insert(usize::try_from(y - 1).ok()?));
            } else {
                ops.push(DiffOp::delete(usize::try_from(x - 1).ok()?));
            }
            x = prev_x;
            y = prev_y;
        }
    }
    ops.reverse();
    Some(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(a: &str, b: &str) -> Vec<DiffOp> {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let eq = |i: usize, j: usize| a.get(i) == b.get(j);
        myers_diff(a.len(), b.len(), &eq, &Budget::new(u64::MAX), 1 << 20).unwrap()
    }

    fn apply(a: &str, b: &str, ops: &[DiffOp]) {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let (mut i, mut j) = (0, 0);
        for op in ops {
            match op.op {
                DiffOpKind::Equal => {
                    assert_eq!(op.a_index, Some(i));
                    assert_eq!(op.b_index, Some(j));
                    assert_eq!(a[i], b[j]);
                    i += 1;
                    j += 1;
                }
                DiffOpKind::Delete => {
                    assert_eq!(op.a_index, Some(i));
                    i += 1;
                }
                DiffOpKind::Insert => {
                    assert_eq!(op.b_index, Some(j));
                    j += 1;
                }
            }
        }
        assert_eq!((i, j), (a.len(), b.len()));
    }

    #[test]
    fn scripts_consume_both_sides() {
        for (a, b) in [
            ("", ""),
            ("abc", ""),
            ("", "abc"),
            ("abc", "abc"),
            ("abcabba", "cbabac"),
            ("kitten", "sitting"),
            ("aaaa", "aa"),
            ("xyz", "abc"),
        ] {
            let ops = run(a, b);
            apply(a, b, &ops);
        }
    }

    #[test]
    fn known_script() {
        // the classic example of the paper: the shortest script has 5 edits
        let ops = run("abcabba", "cbabac");
        let edits = ops.iter().filter(|o| o.op != DiffOpKind::Equal).count();
        assert_eq!(edits, 5);
    }

    #[test]
    fn limits_answer_none() {
        let a: Vec<u32> = (0..200).collect();
        let b: Vec<u32> = (1000..1200).collect();
        let eq = |i: usize, j: usize| a[i] == b[j];
        assert!(myers_diff(200, 200, &eq, &Budget::new(u64::MAX), 1000).is_none());
        assert!(myers_diff(200, 200, &eq, &Budget::new(500), 1 << 20).is_none());
        assert!(myers_diff(200, 200, &eq, &Budget::new(u64::MAX), 1 << 20).is_some());
    }

    #[test]
    fn coarse_is_a_valid_script() {
        let (a, b) = ("the abc end", "the xyz end");
        let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let eq = |i: usize, j: usize| ac[i] == bc[j];
        let mut ops = Vec::new();
        coarse_script(0, ac.len(), 0, bc.len(), &eq, &mut ops);
        apply(a, b, &ops);
        assert_eq!(ops.iter().filter(|o| o.op != DiffOpKind::Equal).count(), 6);
    }
}
