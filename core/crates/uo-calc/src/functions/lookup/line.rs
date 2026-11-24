//! The lookup vector of `lookup.ts` (the TS `Vec`): one row or column of an array (dense) or of a reference (only the
//! existing cells, `idx`/`val` in order — a whole column does not cost a million reads), with the hash index of the
//! exact match built at its first exact search; and the engine's per-pass memory ([`PassCache`], the TS
//! `ctx.cache` with the key `tdl:vec:<area>`), where the vectors of references are kept for the rest of the pass.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::fnreg::FnContext;
use crate::key::Area;
use crate::strings::JsStr;
use crate::value::Scalar;

use super::super::shared::R;

/// The cap of the per-pass memory (record "Memory plan", M5): 64 MiB on wasm32, 256 MiB native. Past it a vector is
/// not kept (it is rebuilt by the next search: the same cells).
const CACHE_CAP: usize = if cfg!(target_pointer_width = "32") { 64 << 20 } else { 256 << 20 };

/// The engine's per-pass memory (`cache`, `engine.ts:220`): cleared when a pass starts and ends (`run`,
/// `computeDirty`, each sweep of the iteration), and by `clear_caches`.
pub struct PassCache {
    vecs: HashMap<Area, Arc<LineVec>>,
    bytes: usize,
    cap: usize,
}

impl Default for PassCache {
    fn default() -> Self {
        Self { vecs: HashMap::new(), bytes: 0, cap: CACHE_CAP }
    }
}

impl core::fmt::Debug for PassCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PassCache").field("vectors", &self.vecs.len()).field("bytes", &self.bytes).finish()
    }
}

impl PassCache {
    /// A cache with another cap (the M5 test).
    #[doc(hidden)]
    #[must_use]
    pub fn with_cap(cap: usize) -> Self {
        Self { cap, ..Self::default() }
    }

    /// Empties it (`cache.clear()`).
    pub fn clear(&mut self) {
        if !self.vecs.is_empty() {
            self.vecs.clear();
        }
        self.bytes = 0;
    }

    /// Vectors kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vecs.len()
    }

    /// Nothing kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vecs.is_empty()
    }

    /// The bytes accounted for the kept vectors (their exact index included).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    fn get(&self, a: &Area) -> Option<Arc<LineVec>> {
        self.vecs.get(a).cloned()
    }

    fn put(&mut self, a: Area, v: &Arc<LineVec>) {
        let b = v.budget_bytes();
        if self.bytes.saturating_add(b) <= self.cap {
            self.bytes += b;
            self.vecs.insert(a, Arc::clone(v));
        }
    }
}

/// The exact-match index (`exactIndex`: key → first index).
#[derive(Default)]
struct Exact {
    nums: HashMap<u64, u32>,
    strs: HashMap<Box<[u16]>, u32>,
    bools: [Option<u32>; 2],
}

/// The collation index of XLOOKUP/XMATCH's exact match (`exactPred(x, …, collate = true)`): per key the first and
/// the last index — texts by their collation key (equal keys ⇔ `compareText` = 0), numbers by value (`-0` = `0`;
/// NaN never equal), booleans.
#[derive(Default)]
struct Collate {
    nums: HashMap<u64, (u32, u32)>,
    strs: HashMap<String, (u32, u32)>,
    bools: [Option<(u32, u32)>; 2],
}

/// The TS `Vec`: `n` positions; `dense` for an array line, else the existing cells (`idx` ascending, `val`).
pub struct LineVec {
    /// Number of positions.
    pub(super) n: u32,
    dense: Option<Box<[Scalar]>>,
    idx: Vec<u32>,
    val: Vec<Scalar>,
    exact: OnceLock<Exact>,
    collate: OnceLock<Collate>,
}

/// The key of a number in the exact index (`'n' + v`: `-0` is `0`, every NaN one key).
#[inline]
fn num_key(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

/// The JS `toLowerCase` of a text into `out` (`'s' + v.toLowerCase()` of the exact index; `sameTextCI`).
fn lower_into(s: JsStr<'_>, out: &mut Vec<u16>) {
    out.clear();
    match s {
        JsStr::Latin1(b) => out.extend(b.iter().map(|&c| u16::from(latin1_lower(c)))),
        JsStr::Utf16(u) => out.extend_from_slice(&crate::jstext::lower(u)),
    }
}

/// Latin-1 lower case unit by unit (no Latin-1 letter lower-cases outside Latin-1 or to two units).
#[inline]
fn latin1_lower(c: u8) -> u8 {
    if c.is_ascii_uppercase() || ((0xC0..=0xDE).contains(&c) && c != 0xD7) { c + 0x20 } else { c }
}

impl LineVec {
    /// `Vec.ofArray(xs)`.
    pub(super) fn of_array(xs: Vec<Scalar>) -> Self {
        Self {
            n: xs.len() as u32,
            dense: Some(xs.into_boxed_slice()),
            idx: Vec::new(),
            val: Vec::new(),
            exact: OnceLock::new(),
            collate: OnceLock::new(),
        }
    }

    /// `Vec.ofArea(a, ctx)`: the existing non-empty cells of a line area (horizontal = one row of several columns).
    pub(super) fn of_area(a: Area, ctx: &mut dyn FnContext) -> R<Self> {
        let horiz = a.r1 == a.r2 && a.c1 != a.c2;
        let mut idx = Vec::new();
        let mut val = Vec::new();
        ctx.for_each_cell(a, &mut |v, r, c| {
            if matches!(v, Scalar::Empty) {
                return;
            }
            idx.push(if horiz { c - a.c1 } else { r - a.r1 });
            val.push(v);
        })?;
        let n = if horiz { a.c2 - a.c1 + 1 } else { a.r2 - a.r1 + 1 };
        Ok(Self { n, dense: None, idx, val, exact: OnceLock::new(), collate: OnceLock::new() })
    }

    /// What the cache accounts for it: the vector and an estimate of its exact index.
    fn budget_bytes(&self) -> usize {
        let k = self.val.len();
        k * (core::mem::size_of::<Scalar>() + 4) + k * 96 + 64
    }

    /// `lower(i)`: the position in the existing cells of the first index ≥ `i`.
    #[inline]
    fn lower(&self, i: i64) -> usize {
        self.idx.partition_point(|&x| i64::from(x) < i)
    }

    /// `get(i)` (`null` outside).
    pub(super) fn get(&self, i: i64) -> Scalar {
        if let Some(d) = &self.dense {
            return usize::try_from(i).ok().and_then(|i| d.get(i)).copied().unwrap_or(Scalar::Empty);
        }
        let k = self.lower(i);
        match (self.idx.get(k), self.val.get(k)) {
            (Some(&x), Some(&v)) if i64::from(x) == i => v,
            _ => Scalar::Empty,
        }
    }

    /// `scan(from, to, f, reverse)`: the non-empty values of `[from, to]` in order (or backwards) until `f` answers
    /// `true`.
    pub(super) fn scan(&self, from: i64, to: i64, reverse: bool, f: &mut dyn FnMut(i64, Scalar) -> bool) {
        if let Some(d) = &self.dense {
            let get = |i: i64| usize::try_from(i).ok().and_then(|i| d.get(i)).copied().unwrap_or(Scalar::Empty);
            if reverse {
                let mut i = to;
                while i >= from {
                    let v = get(i);
                    if !matches!(v, Scalar::Empty) && f(i, v) {
                        return;
                    }
                    i -= 1;
                }
            } else {
                let mut i = from;
                while i <= to {
                    let v = get(i);
                    if !matches!(v, Scalar::Empty) && f(i, v) {
                        return;
                    }
                    i += 1;
                }
            }
            return;
        }
        if reverse {
            let mut k = self.lower(to.saturating_add(1));
            while k > 0 {
                k -= 1;
                let i = i64::from(self.idx[k]);
                if i < from {
                    return;
                }
                if f(i, self.val[k]) {
                    return;
                }
            }
        } else {
            let mut k = self.lower(from);
            while k < self.idx.len() {
                let i = i64::from(self.idx[k]);
                if i > to {
                    return;
                }
                if f(i, self.val[k]) {
                    return;
                }
                k += 1;
            }
        }
    }

    /// `blank(reverse)`: the first (last) empty index, or −1.
    pub(super) fn blank(&self, reverse: bool) -> i64 {
        let n = i64::from(self.n);
        if let Some(d) = &self.dense {
            let hit = if reverse {
                d.iter().take(self.n as usize).rposition(|v| matches!(v, Scalar::Empty))
            } else {
                d.iter().take(self.n as usize).position(|v| matches!(v, Scalar::Empty))
            };
            return hit.map_or(-1, |i| i as i64);
        }
        if reverse {
            let (mut i, mut k) = (n - 1, self.idx.len() as i64 - 1);
            while i >= 0 && k >= 0 && i64::from(self.idx[k as usize]) == i {
                i -= 1;
                k -= 1;
            }
            i
        } else {
            let mut i = 0;
            while i < n {
                match self.idx.get(i as usize) {
                    Some(&x) if i64::from(x) == i => i += 1,
                    _ => return i,
                }
            }
            -1
        }
    }

    /// `exactFirst(x)`: the first index whose exact key equals `x`'s (texts by lower case, numbers by value with
    /// `-0` = `0`, booleans); −1 when none or `x` is empty or an error. A dense line (an array of one call) is
    /// scanned; a line of existing cells builds its hash index at the first search (kept with the vector).
    pub(super) fn exact_first(&self, x: Scalar, ctx: &dyn FnContext) -> i64 {
        if matches!(x, Scalar::Empty | Scalar::Err(_)) {
            return -1;
        }
        let mut xl = Vec::new();
        if let Scalar::Str(r) = x {
            lower_into(ctx.text(r), &mut xl);
        }
        if let Some(d) = &self.dense {
            let mut buf = Vec::new();
            for (i, &v) in d.iter().enumerate() {
                let hit = match (x, v) {
                    (Scalar::Num(a), Scalar::Num(b)) => num_key(a) == num_key(b),
                    (Scalar::Bool(a), Scalar::Bool(b)) => a == b,
                    (Scalar::Str(_), Scalar::Str(s)) => {
                        lower_into(ctx.text(s), &mut buf);
                        buf == xl
                    }
                    _ => false,
                };
                if hit {
                    return i as i64;
                }
            }
            return -1;
        }
        let ix = self.exact.get_or_init(|| {
            let mut m = Exact::default();
            let mut buf = Vec::new();
            for (&i, &v) in self.idx.iter().zip(&self.val) {
                match v {
                    Scalar::Num(a) => {
                        m.nums.entry(num_key(a)).or_insert(i);
                    }
                    Scalar::Bool(b) => {
                        let s = &mut m.bools[usize::from(b)];
                        if s.is_none() {
                            *s = Some(i);
                        }
                    }
                    Scalar::Str(r) => {
                        lower_into(ctx.text(r), &mut buf);
                        if !m.strs.contains_key(buf.as_slice()) {
                            m.strs.insert(buf.as_slice().into(), i);
                        }
                    }
                    Scalar::Empty | Scalar::Err(_) => {}
                }
            }
            m
        });
        let hit = match x {
            Scalar::Num(a) => ix.nums.get(&num_key(a)).copied(),
            Scalar::Bool(b) => ix.bools[usize::from(b)],
            Scalar::Str(_) => ix.strs.get(xl.as_slice()).copied(),
            Scalar::Empty | Scalar::Err(_) => None,
        };
        hit.map_or(-1, i64::from)
    }
}

impl LineVec {
    /// Whether the exact searches of this vector go through its indexes (a vector of existing cells, kept for the
    /// pass); a dense line of one call is scanned.
    pub(super) fn indexed(&self) -> bool {
        self.dense.is_none()
    }

    /// The first (or last) index `i` where `exactPred(x, false, true)(v)` holds — XLOOKUP/XMATCH match mode 0 (and 2
    /// without wildcards), search mode ±1: texts equal by the collation, numbers and booleans `===`; −1 when none.
    /// Only for an [`Self::indexed`] vector (the result equals the TS's linear scan: the same predicate, the first
    /// or the last hit).
    pub(super) fn collate_find(&self, x: Scalar, ctx: &dyn FnContext, last: bool) -> i64 {
        let ix = self.collate.get_or_init(|| {
            let mut m = Collate::default();
            let put = |e: &mut (u32, u32), i: u32| e.1 = i;
            for (&i, &v) in self.idx.iter().zip(&self.val) {
                match v {
                    Scalar::Num(a) if !a.is_nan() => put(m.nums.entry(num_key(a)).or_insert((i, i)), i),
                    Scalar::Bool(b) => {
                        let s = &mut m.bools[usize::from(b)];
                        match s {
                            Some(e) => e.1 = i,
                            None => *s = Some((i, i)),
                        }
                    }
                    Scalar::Str(r) => put(m.strs.entry(crate::collate::collation_key(ctx.text(r))).or_insert((i, i)), i),
                    _ => {}
                }
            }
            m
        });
        let hit = match x {
            Scalar::Num(a) if !a.is_nan() => ix.nums.get(&num_key(a)).copied(),
            Scalar::Bool(b) => ix.bools[usize::from(b)],
            Scalar::Str(r) => ix.strs.get(crate::collate::collation_key(ctx.text(r)).as_str()).copied(),
            _ => None,
        };
        hit.map_or(-1, |(f, l)| i64::from(if last { l } else { f }))
    }
}

/// `Vec.ofAreaCached(a, ctx)`: the vector of a line area, kept in the engine's pass memory when there is one.
pub(super) fn of_area_cached(a: Area, ctx: &mut dyn FnContext) -> R<Arc<LineVec>> {
    match ctx.pass_cache() {
        Some(c) => {
            if let Some(v) = c.get(&a) {
                return Ok(v);
            }
        }
        None => return Ok(Arc::new(LineVec::of_area(a, ctx)?)),
    }
    let v = Arc::new(LineVec::of_area(a, ctx)?);
    if let Some(c) = ctx.pass_cache() {
        c.put(a, &v);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_scan_of_existing_cells() {
        let v = LineVec {
            n: 5,
            dense: None,
            idx: vec![0, 2, 3],
            val: vec![Scalar::Num(1.0); 3],
            exact: OnceLock::new(),
            collate: OnceLock::new(),
        };
        assert_eq!(v.blank(false), 1);
        assert_eq!(v.blank(true), 4);
        let full = LineVec {
            n: 2,
            dense: None,
            idx: vec![0, 1],
            val: vec![Scalar::Num(1.0); 2],
            exact: OnceLock::new(),
            collate: OnceLock::new(),
        };
        assert_eq!(full.blank(false), -1);
        assert_eq!(full.blank(true), -1);
        let tail = LineVec {
            n: 5,
            dense: None,
            idx: vec![2, 3, 4],
            val: vec![Scalar::Num(1.0); 3],
            exact: OnceLock::new(),
            collate: OnceLock::new(),
        };
        assert_eq!(tail.blank(true), 1);
        let mut seen = Vec::new();
        v.scan(1, 3, true, &mut |i, _| {
            seen.push(i);
            false
        });
        assert_eq!(seen, [3, 2]);
        assert!(matches!(v.get(2), Scalar::Num(_)));
        assert!(matches!(v.get(1), Scalar::Empty));
        assert!(matches!(v.get(-1), Scalar::Empty));
    }

    #[test]
    fn cache_cap_is_kept() {
        let mut c = PassCache::with_cap(4096);
        for k in 0..100u32 {
            let v = Arc::new(LineVec {
                n: 10,
                dense: None,
                idx: vec![0; 10],
                val: vec![Scalar::Num(1.0); 10],
                exact: OnceLock::new(),
                collate: OnceLock::new(),
            });
            c.put(Area { sheet: crate::key::SheetIx(0), r1: k, c1: 0, r2: k, c2: 0 }, &v);
        }
        assert!(c.bytes() <= 4096 && c.len() < 100 && !c.is_empty());
        c.clear();
        assert!(c.is_empty() && c.bytes() == 0);
    }
}
