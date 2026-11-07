//! `Array.prototype.sort` as V8 runs it (third_party/v8/builtins/array-sort.tq), over indices: one
//! instantiation for every element type, so the cold sorts of the engines (and `word::obj::js_sort`)
//! do not each bring their own copy of the standard library's sort into the binary. Plan:
//! `core/docs/modules/size-optimization.md`, item 3.
//!
//! The comparators answer like a JS comparator (`f64`: `< 0` puts the first argument first; NaN is
//! "not less"), and the core replays V8's decisions call for call, so even an inconsistent
//! comparator (NaN) gives the permutation of the previous element-wise implementation.

use core::cmp::Ordering;

/// The permutation `arr.sort(cmp)` gives: `result[k]` is the ORIGINAL index of the element that ends
/// at position `k`. `cmp(i, j)` compares the elements at original indices `i` and `j` and answers
/// like a JS comparator. Below 64 elements: `CountAndMakeRun` + `BinaryInsertionSort`; from 64: a
/// top-down stable merge (split at `len / 2`, the right element goes first only when
/// `cmp(right, left) < 0`) with one scratch buffer.
#[must_use]
pub fn js_sort_order(n: usize, cmp: &mut dyn FnMut(usize, usize) -> f64) -> Vec<usize> {
    let mut ord: Vec<usize> = (0..n).collect();
    if n < 2 {
        return ord;
    }
    if n >= 64 {
        let mut scratch = vec![0usize; n / 2];
        merge_sort(&mut ord, &mut scratch, cmp);
        return ord;
    }
    let mut at = |ord: &[usize], a: usize, b: usize| -> f64 {
        match (ord.get(a), ord.get(b)) {
            (Some(&x), Some(&y)) => cmp(x, y),
            _ => 0.0,
        }
    };
    // CountAndMakeRun(0, n)
    let mut run = 2usize;
    let descending = at(&ord, 1, 0) < 0.0;
    while run < n {
        let order = at(&ord, run, run - 1);
        // V8: a descending run stops at `order >= 0`, an ascending one at `order < 0` (NaN stops neither)
        if descending {
            if order >= 0.0 {
                break;
            }
        } else if order < 0.0 {
            break;
        }
        run += 1;
    }
    if descending && let Some(r) = ord.get_mut(..run) {
        r.reverse();
    }
    // BinaryInsertionSort(0, run, n)
    let mut start = run;
    while start < n {
        let mut left = 0usize;
        let mut right = start;
        while left < right {
            let mid = left + ((right - left) >> 1);
            if at(&ord, start, mid) < 0.0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        if let Some(r) = ord.get_mut(left..=start) {
            r.rotate_right(1);
        }
        start += 1;
    }
    ord
}

/// A top-down stable merge sort of `ord` (original indices): `b` goes before `a` only when
/// `cmp(b, a) < 0`. `scratch` holds at least `ord.len() / 2` entries.
fn merge_sort(ord: &mut [usize], scratch: &mut [usize], cmp: &mut dyn FnMut(usize, usize) -> f64) {
    let n = ord.len();
    if n < 2 {
        return;
    }
    let mid = n / 2;
    let (l, r) = ord.split_at_mut(mid);
    merge_sort(l, scratch, cmp);
    merge_sort(r, scratch, cmp);
    let Some(buf) = scratch.get_mut(..mid) else { return };
    buf.copy_from_slice(ord.get(..mid).unwrap_or(&[]));
    let (mut a, mut b, mut k) = (0usize, mid, 0usize);
    // the remaining right entries are already in place when the left ones run out
    while a < mid && b < n {
        let (Some(&x), Some(&y)) = (buf.get(a), ord.get(b)) else { break };
        let take_right = cmp(y, x) < 0.0;
        if let Some(slot) = ord.get_mut(k) {
            *slot = if take_right { y } else { x };
        }
        if take_right {
            b += 1;
        } else {
            a += 1;
        }
        k += 1;
    }
    while a < mid {
        if let (Some(&x), Some(slot)) = (buf.get(a), ord.get_mut(k)) {
            *slot = x;
        }
        a += 1;
        k += 1;
    }
}

/// Applies `order` in place (position `k` receives the element at original index `order[k]`), by
/// cycles of swaps. `order` must be a permutation of `0..v.len()`; anything else leaves the
/// remainder of the slice as it is (never panics).
pub fn permute<T>(v: &mut [T], mut order: Vec<usize>) {
    for start in 0..order.len().min(v.len()) {
        let mut i = start;
        while let Some(&j) = order.get(i) {
            if let Some(o) = order.get_mut(i) {
                *o = i;
            }
            if j == start || j == i || j >= v.len() {
                break;
            }
            v.swap(i, j);
            i = j;
        }
    }
}

/// `arr.sort(cmp)` with a JS comparator (what `uo_docx::word::obj::js_sort` was).
pub fn js_sort_by<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> f64) {
    if v.len() < 2 {
        return;
    }
    let order = js_sort_order(v.len(), &mut |i, j| match (v.get(i), v.get(j)) {
        (Some(a), Some(b)) => cmp(a, b),
        _ => 0.0,
    });
    permute(v, order);
}

/// A stable sort by a Rust ordering through the same core: identical to `slice::sort_by` for every
/// consistent comparator (a stable sort's output is unique).
pub fn sort_stable_by<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    if v.len() < 2 {
        return;
    }
    let order = js_sort_order(v.len(), &mut |i, j| match (v.get(i), v.get(j)) {
        (Some(a), Some(b)) => match cmp(a, b) {
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
            Ordering::Greater => 1.0,
        },
        _ => 0.0,
    });
    permute(v, order);
}

/// [`sort_stable_by`] on a key: identical to `slice::sort_by_key` (the key is computed per comparison).
pub fn sort_stable_by_key<T, K: Ord>(v: &mut [T], mut key: impl FnMut(&T) -> K) {
    sort_stable_by(v, |a, b| key(a).cmp(&key(b)));
}
