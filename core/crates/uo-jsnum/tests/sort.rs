//! The shared sort core (`uo_jsnum::sort`) against the element-wise `js_sort` it replaced
//! (`uo_docx::word::obj::js_sort` before size-optimization item 3a, kept here verbatim as the
//! reference) and against `slice::sort_by` for consistent comparators.

use std::cell::Cell;
use uo_jsnum::sort::{js_sort_by, js_sort_order, permute, sort_stable_by, sort_stable_by_key};

/// The previous `word::obj::js_sort`, unchanged.
fn reference_js_sort<T>(v: &mut Vec<T>, cmp: impl Fn(&T, &T) -> f64) {
    let n = v.len();
    if n < 2 {
        return;
    }
    if n >= 64 {
        let items: Vec<T> = std::mem::take(v);
        *v = merge_sort(items, &cmp);
        return;
    }
    let mut run = 2usize;
    let descending = match (v.get(1), v.first()) {
        (Some(b), Some(a)) => cmp(b, a) < 0.0,
        _ => false,
    };
    while run < n {
        let (Some(cur), Some(prev)) = (v.get(run), v.get(run - 1)) else { break };
        let order = cmp(cur, prev);
        if descending {
            if order >= 0.0 {
                break;
            }
        } else if order < 0.0 {
            break;
        }
        run += 1;
    }
    if descending && let Some(r) = v.get_mut(..run) {
        r.reverse();
    }
    let mut start = run;
    while start < n {
        let mut left = 0usize;
        let mut right = start;
        while left < right {
            let mid = left + ((right - left) >> 1);
            let order = match (v.get(start), v.get(mid)) {
                (Some(p), Some(m)) => cmp(p, m),
                _ => 0.0,
            };
            if order < 0.0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        if let Some(r) = v.get_mut(left..=start) {
            r.rotate_right(1);
        }
        start += 1;
    }
}

fn merge_sort<T>(mut items: Vec<T>, cmp: &impl Fn(&T, &T) -> f64) -> Vec<T> {
    if items.len() < 2 {
        return items;
    }
    let right = items.split_off(items.len() / 2);
    let (l, r) = (merge_sort(items, cmp), merge_sort(right, cmp));
    let mut out = Vec::with_capacity(l.len() + r.len());
    let (mut li, mut ri) = (l.into_iter().peekable(), r.into_iter().peekable());
    loop {
        let take_right = match (li.peek(), ri.peek()) {
            (Some(a), Some(b)) => cmp(b, a) < 0.0,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        let next = if take_right { ri.next() } else { li.next() };
        if let Some(x) = next {
            out.push(x);
        }
    }
    out
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const SPECIAL: [f64; 8] = [f64::NAN, 0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, 1.0, -1.0, 2.5];

fn value(r: &mut Rng, wide: bool) -> f64 {
    match r.below(4) {
        0 => SPECIAL[r.below(8) as usize],
        1 => r.below(6) as f64,
        _ => r.below(if wide { 1000 } else { 12 }) as f64 - 5.0,
    }
}

#[test]
fn same_permutation_as_the_previous_js_sort() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for n in 0..300usize {
        for round in 0..6 {
            let v: Vec<(f64, usize)> = (0..n).map(|i| (value(&mut r, round % 2 == 0), i)).collect();
            let mut a = v.clone();
            let mut b = v.clone();
            reference_js_sort(&mut a, |x, y| x.0 - y.0);
            js_sort_by(&mut b, |x, y| x.0 - y.0);
            let ids = |v: &Vec<(f64, usize)>| v.iter().map(|x| x.1).collect::<Vec<_>>();
            assert_eq!(ids(&a), ids(&b), "n = {n}, round {round}");
        }
    }
}

#[test]
fn sorted_and_reversed_inputs_match() {
    for n in [0usize, 1, 2, 3, 63, 64, 65, 127, 128, 200] {
        for rev in [false, true] {
            let mut v: Vec<(f64, usize)> = (0..n).map(|i| (if rev { (n - i) as f64 } else { i as f64 }, i)).collect();
            let mut w = v.clone();
            reference_js_sort(&mut v, |x, y| x.0 - y.0);
            js_sort_by(&mut w, |x, y| x.0 - y.0);
            assert_eq!(v, w);
        }
    }
}

/// A comparator whose answer depends only on how many times it was called: the two sorts must make the
/// same calls in the same order for the permutations to agree.
#[test]
fn call_count_dependent_comparator_gives_the_same_permutation() {
    for seed in 1..40u64 {
        for n in [2usize, 3, 7, 31, 63, 64, 65, 100, 129, 257] {
            let answers = {
                let mut r = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
                (0..4000).map(|_| [-1.0, 0.0, 1.0, f64::NAN][r.below(4) as usize]).collect::<Vec<_>>()
            };
            let run = |reference: bool| {
                let calls = Cell::new(0usize);
                let mut v: Vec<usize> = (0..n).collect();
                let cmp = |_: &usize, _: &usize| {
                    let c = calls.get();
                    calls.set(c + 1);
                    answers[c % answers.len()]
                };
                if reference {
                    reference_js_sort(&mut v, cmp);
                } else {
                    js_sort_by(&mut v, cmp);
                }
                (v, calls.get())
            };
            assert_eq!(run(true), run(false), "seed {seed}, n {n}");
        }
    }
}

#[test]
fn v8_vectors() {
    // V8 (Node 24.15) on the same tagged pairs: "beacd" and "afcbde"
    let mut v = vec![(3.0, 'a'), (1.0, 'b'), (3.0, 'c'), (f64::NAN, 'd'), (2.0, 'e')];
    js_sort_by(&mut v, |a, b| a.0 - b.0);
    assert_eq!(v.iter().map(|x| x.1).collect::<String>(), "beacd");
    let mut w = vec![(f64::NAN, 'a'), (1.0, 'b'), (0.0, 'c'), (f64::NAN, 'd'), (2.0, 'e'), (-1.0, 'f')];
    js_sort_by(&mut w, |a, b| a.0 - b.0);
    assert_eq!(w.iter().map(|x| x.1).collect::<String>(), "afcbde");
}

#[test]
fn permute_applies_any_permutation() {
    let mut r = Rng(7);
    for n in 0..40usize {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            order.swap(i, r.below(i as u64 + 1) as usize);
        }
        let mut v: Vec<usize> = (0..n).map(|i| i * 10).collect();
        permute(&mut v, order.clone());
        let want: Vec<usize> = order.iter().map(|&o| o * 10).collect();
        assert_eq!(v, want);
    }
    // not a permutation: never panics, never loops
    let mut v = vec![1, 2, 3];
    permute(&mut v, vec![5, 5, 5, 9]);
    let mut v = vec![1, 2, 3];
    permute(&mut v, vec![1, 1]);
    let _ = js_sort_order(0, &mut |_, _| 0.0);
}

#[test]
fn ties_agree_with_slice_sort_by() {
    let mut r = Rng(0xDEAD_BEEF_1234_5678);
    for n in (0..200usize).chain([255, 256, 500]) {
        // u32 keys with many ties
        let v: Vec<(u32, usize)> = (0..n).map(|i| (r.below(9) as u32, i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by_key(|x| x.0);
        sort_stable_by_key(&mut b, |x| x.0);
        assert_eq!(a, b, "u32 n={n}");
        // i64 Reverse
        let v: Vec<(i64, usize)> = (0..n).map(|i| (r.below(7) as i64 - 3, i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by_key(|x| std::cmp::Reverse(x.0));
        sort_stable_by_key(&mut b, |x| std::cmp::Reverse(x.0));
        assert_eq!(a, b, "i64 Reverse n={n}");
        // usize Reverse
        let v: Vec<(usize, usize)> = (0..n).map(|i| (r.below(5) as usize, i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by_key(|x| std::cmp::Reverse(x.0));
        sort_stable_by_key(&mut b, |x| std::cmp::Reverse(x.0));
        assert_eq!(a, b, "usize Reverse n={n}");
        // 4-byte tags
        let v: Vec<([u8; 4], usize)> = (0..n).map(|i| ([b'a' + r.below(3) as u8, 0, 0, b'x' + r.below(2) as u8], i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by_key(|x| x.0);
        sort_stable_by_key(&mut b, |x| x.0);
        assert_eq!(a, b, "tag n={n}");
        // Option<u32> keys (array_index shape)
        let v: Vec<(Option<u32>, usize)> = (0..n).map(|i| (if r.below(3) == 0 { None } else { Some(r.below(6) as u32) }, i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by_key(|x| x.0);
        sort_stable_by_key(&mut b, |x| x.0);
        assert_eq!(a, b, "option n={n}");
        // f64 partial_cmp with +-Infinity, -0/+0 and duplicates (no NaN: consistent)
        let v: Vec<(f64, usize)> =
            (0..n).map(|i| ([0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, 1.5, -2.0][r.below(6) as usize], i)).collect();
        let cmp = |a: &(f64, usize), b: &(f64, usize)| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_by(cmp);
        sort_stable_by(&mut b, cmp);
        assert_eq!(a, b, "f64 n={n}");
        // unique tuples: sort_unstable == stable
        let v: Vec<(u64, u64, usize)> = (0..n).map(|i| (r.below(5), r.below(5), i)).collect();
        let (mut a, mut b) = (v.clone(), v.clone());
        a.sort_unstable();
        sort_stable_by(&mut b, |x, y| x.cmp(y));
        assert_eq!(a, b, "tuples n={n}");
    }
}
