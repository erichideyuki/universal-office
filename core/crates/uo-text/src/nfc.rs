//! `String.prototype.normalize('NFC')` (and NFD, and NFKD with the `collation` feature) over UTF-16 with
//! the tables of Node 24.15's ICU. A lone surrogate is a starter that combines with nothing and passes
//! through, as in V8.
//!
//! The algorithm is UAX #15 (decompose, put each run of combining marks in class order, compose with
//! the blocking rule); Hangul syllables are algorithmic and the rest is table lookups. The combining
//! class is stored as a rank, not as Unicode's value: only the order of the classes is used.

use std::borrow::Cow;

use crate::tables as t;
use crate::utf16::{self, code_points};

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// A decomposition table: sorted keys (BMP keys, then astral ones), end offsets and a UTF-16 pool.
struct Decomp {
    keys_bmp: &'static [u16],
    keys_astral: &'static [u32],
    ends: &'static [u16],
    pool: &'static [u16],
}

impl Decomp {
    fn get(&self, cp: u32) -> Option<&'static [u16]> {
        let idx = match u16::try_from(cp) {
            Ok(key) => self.keys_bmp.binary_search(&key).ok()?,
            Err(_) => self.keys_bmp.len() + self.keys_astral.binary_search(&cp).ok()?,
        };
        let end = usize::from(*self.ends.get(idx)?);
        let start = match idx.checked_sub(1) {
            Some(prev) => usize::from(*self.ends.get(prev)?),
            None => 0,
        };
        self.pool.get(start..end)
    }
}

static NFD: Decomp = Decomp { keys_bmp: &t::NFD_KEYS_BMP, keys_astral: &t::NFD_KEYS_ASTRAL, ends: &t::NFD_ENDS, pool: &t::NFD_POOL };

#[cfg(feature = "collation")]
static COMPAT: Decomp = Decomp {
    keys_bmp: &crate::tables_compat::COMPAT_KEYS_BMP,
    keys_astral: &crate::tables_compat::COMPAT_KEYS_ASTRAL,
    ends: &crate::tables_compat::COMPAT_ENDS,
    pool: &crate::tables_compat::COMPAT_POOL,
};

/// The canonical combining class of `cp` as a rank: 0 for a starter, otherwise a number that orders the
/// classes as Unicode's values do (the values themselves differ). Only meaningful for a code point that
/// has no canonical decomposition (the NFD form of the others is what has classes).
#[must_use]
pub fn combining_class_rank(cp: u32) -> u8 {
    let idx = t::CCC.partition_point(|&(first, _)| (first & 0x00FF_FFFF) <= cp);
    match idx.checked_sub(1).and_then(|i| t::CCC.get(i)) {
        Some(&(first, last)) if cp <= last => u8::try_from(first >> 24).unwrap_or(0),
        _ => 0,
    }
}

#[inline]
fn is_hangul_syllable(cp: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&cp)
}

fn push_decomposed(buf: &mut Vec<u32>, cp: u32, compat: bool) {
    if is_hangul_syllable(cp) {
        let s = cp - S_BASE;
        buf.push(L_BASE + s / N_COUNT);
        buf.push(V_BASE + (s % N_COUNT) / T_COUNT);
        let t_index = s % T_COUNT;
        if t_index != 0 {
            buf.push(T_BASE + t_index);
        }
        return;
    }
    let hit = if compat { compat_get(cp).or_else(|| NFD.get(cp)) } else { NFD.get(cp) };
    match hit {
        Some(units) => buf.extend(code_points(units)),
        None => buf.push(cp),
    }
}

#[cfg(feature = "collation")]
fn compat_get(cp: u32) -> Option<&'static [u16]> {
    COMPAT.get(cp)
}

#[cfg(not(feature = "collation"))]
fn compat_get(_cp: u32) -> Option<&'static [u16]> {
    None
}

/// A stable sort of a run of combining marks by class: insertion sort for short runs and a counting sort (the
/// class is a byte) for long ones, so a hostile run of a million marks costs O(n) and the wasm build does not
/// carry the library's generic merge sort.
fn sort_by_class(run: &mut [u32]) {
    if run.len() <= 24 {
        for i in 1..run.len() {
            let Some(&item) = run.get(i) else { break };
            let class = combining_class_rank(item);
            let mut j = i;
            while j > 0 && run.get(j - 1).is_some_and(|&prev| combining_class_rank(prev) > class) {
                run.swap(j, j - 1);
                j -= 1;
            }
        }
        return;
    }
    let mut start = [0usize; 257];
    for &c in run.iter() {
        if let Some(n) = start.get_mut(usize::from(combining_class_rank(c)) + 1) {
            *n += 1;
        }
    }
    for k in 1..start.len() {
        start[k] += start[k - 1];
    }
    let mut sorted = vec![0u32; run.len()];
    for &c in run.iter() {
        if let Some(slot) = start.get_mut(usize::from(combining_class_rank(c)))
            && let Some(dst) = sorted.get_mut(*slot)
        {
            *dst = c;
            *slot += 1;
        }
    }
    run.copy_from_slice(&sorted);
}

fn reorder(buf: &mut [u32]) {
    let mut i = 0;
    while i < buf.len() {
        if combining_class_rank(buf.get(i).copied().unwrap_or(0)) == 0 {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < buf.len() && combining_class_rank(buf.get(j).copied().unwrap_or(0)) != 0 {
            j += 1;
        }
        if let Some(run) = buf.get_mut(i..j) {
            sort_by_class(run);
        }
        i = j;
    }
}

fn compose_pair(a: u32, b: u32) -> Option<u32> {
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return Some(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if is_hangul_syllable(a) && (a - S_BASE).is_multiple_of(T_COUNT) && (T_BASE + 1..T_BASE + T_COUNT).contains(&b) {
        return Some(a + (b - T_BASE));
    }
    if let (Ok(a16), Ok(b16)) = (u16::try_from(a), u16::try_from(b))
        && let Ok(i) = t::COMP_BMP.binary_search_by(|&(x, y, _)| (x, y).cmp(&(a16, b16)))
    {
        return t::COMP_BMP.get(i).map(|&(_, _, c)| u32::from(c));
    }
    let i = t::COMP_ASTRAL.binary_search_by(|&(x, y, _)| (x, y).cmp(&(a, b))).ok()?;
    t::COMP_ASTRAL.get(i).map(|&(_, _, c)| c)
}

fn compose(buf: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(buf.len());
    let mut starter: Option<usize> = None;
    let mut last_class = 0u8;
    for &cp in buf {
        let class = combining_class_rank(cp);
        if let Some(s) = starter {
            let adjacent = out.len() == s + 1;
            let blocked = !adjacent && (last_class == 0 || last_class >= class);
            if !blocked
                && let Some(&first) = out.get(s)
                && let Some(composed) = compose_pair(first, cp)
            {
                if let Some(slot) = out.get_mut(s) {
                    *slot = composed;
                }
                continue;
            }
        }
        if class == 0 {
            starter = Some(out.len());
        }
        last_class = class;
        out.push(cp);
    }
    out
}

fn normalize(units: &[u16], quick_below: u16, compat: bool, composed: bool) -> Cow<'_, [u16]> {
    if units.iter().all(|&u| u < quick_below) {
        return Cow::Borrowed(units);
    }
    let mut buf: Vec<u32> = Vec::with_capacity(units.len() + units.len() / 4);
    for cp in code_points(units) {
        push_decomposed(&mut buf, cp, compat);
    }
    reorder(&mut buf);
    let cps = if composed { compose(&buf) } else { buf };
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    for cp in cps {
        utf16::push(&mut out, cp);
    }
    if out == units { Cow::Borrowed(units) } else { Cow::Owned(out) }
}

/// `s.normalize('NFC')`. Borrows when `units` is already in NFC.
#[must_use]
pub fn nfc(units: &[u16]) -> Cow<'_, [u16]> {
    normalize(units, 0x300, false, true)
}

/// `s.normalize('NFD')`. Borrows when `units` is already in NFD.
#[must_use]
pub fn nfd(units: &[u16]) -> Cow<'_, [u16]> {
    normalize(units, 0xC0, false, false)
}

/// `s.normalize('NFKD')`: compatibility decomposition (the collation feature's table). Borrows when
/// `units` is already in NFKD.
#[cfg(feature = "collation")]
#[must_use]
pub fn nfkd(units: &[u16]) -> Cow<'_, [u16]> {
    normalize(units, 0xA0, true, false)
}

/// `s.normalize('NFC') === s`.
#[must_use]
pub fn is_nfc(units: &[u16]) -> bool {
    matches!(nfc(units), Cow::Borrowed(_))
}

/// [`nfc`] for text that is valid Unicode (a Rust string): borrows when it is already in NFC.
#[must_use]
pub fn nfc_str(s: &str) -> Cow<'_, str> {
    if s.chars().all(|c| u32::from(c) < 0x300) {
        return Cow::Borrowed(s);
    }
    let units = utf16::from_str(s);
    match nfc(&units) {
        Cow::Borrowed(_) => Cow::Borrowed(s),
        Cow::Owned(out) => Cow::Owned(String::from_utf16_lossy(&out)),
    }
}
