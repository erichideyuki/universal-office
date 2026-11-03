//! `String.prototype.toUpperCase` and `toLowerCase` over UTF-16: the full Unicode case mapping with
//! SpecialCasing (`ß` -> `SS`, `ŉ` -> `ʼN`, `ﬁ` -> `FI`, `İ` -> `i̇`) and the Final_Sigma context of
//! `toLowerCase`, as V8 13.6 (ICU, root locale, Unicode 17.0) computes them. There is no locale: the Turkish
//! dotless i stays `i` -> `I` and `ı` -> `I` as in V8's `toUpperCase`. A lone surrogate maps to itself.
//!
//! Lengths change (`ß` is one unit and `SS` two), which moves every later offset: callers that keep
//! UTF-16 offsets must map text and offsets together.
//!
//! The boundary with `uo_jsnum::text`: that module's `to_lowercase` works on `&str` with the Unicode
//! version of the Rust standard library; this one works on `&[u16]` with Node 24.15's tables and is the
//! authority wherever a result has to be what V8 gives.

use std::borrow::Cow;

use crate::tables as t;
use crate::utf16::{self, code_points, decode_at, decode_before};

const SIGMA: u32 = 0x03A3;
const FINAL_SIGMA: u32 = 0x03C2;

/// The result of the case mapping of one code point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMapping {
    /// no mapping: the code point stays
    Same,
    /// one code point
    One(u32),
    /// more than one code point, as UTF-16 code units
    Many(&'static [u16]),
}

fn mapping(cp: u32, runs: &'static [(u32, u32, i32)], special: &'static [(u32, u16, u16)], pool: &'static [u16]) -> CaseMapping {
    if let Ok(i) = special.binary_search_by_key(&cp, |&(key, _, _)| key)
        && let Some(&(_, off, len)) = special.get(i)
    {
        let start = usize::from(off);
        return pool.get(start..start + usize::from(len)).map_or(CaseMapping::Same, CaseMapping::Many);
    }
    let idx = runs.partition_point(|&(first, _, _)| first <= cp);
    if let Some(&(first, tail, delta)) = idx.checked_sub(1).and_then(|i| runs.get(i)) {
        let last = tail & 0x00FF_FFFF;
        let step = (tail >> 24).max(1);
        if cp <= last
            && (cp - first).is_multiple_of(step)
            && let Ok(mapped) = u32::try_from(i64::from(cp) + i64::from(delta))
        {
            return CaseMapping::One(mapped);
        }
    }
    CaseMapping::Same
}

/// `String.fromCodePoint(cp).toUpperCase()`.
#[must_use]
pub fn upper_mapping(cp: u32) -> CaseMapping {
    mapping(cp, &t::UPPER_RUNS, &t::UPPER_SPECIAL, &t::UPPER_SPECIAL_POOL)
}

/// `String.fromCodePoint(cp).toLowerCase()` (no context: a capital sigma gives `σ`).
#[must_use]
pub fn lower_mapping(cp: u32) -> CaseMapping {
    mapping(cp, &t::LOWER_RUNS, &t::LOWER_SPECIAL, &t::LOWER_SPECIAL_POOL)
}

fn push_mapped(out: &mut Vec<u16>, cp: u32, m: CaseMapping) {
    match m {
        CaseMapping::Same => utf16::push(out, cp),
        CaseMapping::One(mapped) => utf16::push(out, mapped),
        CaseMapping::Many(units) => out.extend_from_slice(units),
    }
}

/// The length in UTF-16 units of `String.fromCodePoint(cp).toUpperCase()`.
#[must_use]
pub fn upper_len(cp: u32) -> usize {
    match upper_mapping(cp) {
        CaseMapping::Same => {
            if cp > 0xFFFF {
                2
            } else {
                1
            }
        }
        CaseMapping::One(mapped) => {
            if mapped > 0xFFFF {
                2
            } else {
                1
            }
        }
        CaseMapping::Many(units) => units.len(),
    }
}

/// `s.toUpperCase()`. Borrows when nothing changes.
#[must_use]
pub fn to_uppercase(units: &[u16]) -> Cow<'_, [u16]> {
    if units.iter().all(|&u| u < 0x80 && !is_ascii_lower(u)) {
        return Cow::Borrowed(units);
    }
    let mut out: Vec<u16> = Vec::with_capacity(units.len() + 2);
    let mut changed = false;
    for cp in code_points(units) {
        let m = upper_mapping(cp);
        changed |= m != CaseMapping::Same;
        push_mapped(&mut out, cp, m);
    }
    if changed { Cow::Owned(out) } else { Cow::Borrowed(units) }
}

fn class_of(cp: u32) -> u8 {
    let idx = t::SIGMA.partition_point(|&(first, _)| first <= cp);
    match idx.checked_sub(1).and_then(|i| t::SIGMA.get(i)) {
        Some(&(_, tail)) if cp <= tail & 0x00FF_FFFF => u8::try_from(tail >> 24).unwrap_or(0),
        _ => 0,
    }
}

/// Final_Sigma for the capital sigma at `units[at..at + 1]`: a cased letter before it (skipping
/// case-ignorable characters) and none after it.
fn is_final_sigma(units: &[u16], at: usize) -> bool {
    let mut end = at;
    let preceded = loop {
        let Some((cp, width)) = decode_before(units, end) else { break false };
        end -= width;
        match class_of(cp) {
            1 => break true,
            2 => {}
            _ => break false,
        }
    };
    if !preceded {
        return false;
    }
    let mut pos = at + 1;
    loop {
        let Some((cp, width)) = decode_at(units, pos) else { return true };
        pos += width;
        match class_of(cp) {
            1 => return false,
            2 => {}
            _ => return true,
        }
    }
}

/// `s.toLowerCase()`: the full mapping, with Final_Sigma (`ΑΣ` -> `ας`). Borrows when nothing changes.
#[must_use]
pub fn to_lowercase(units: &[u16]) -> Cow<'_, [u16]> {
    if units.iter().all(|&u| u < 0x80 && !is_ascii_upper(u)) {
        return Cow::Borrowed(units);
    }
    let mut out: Vec<u16> = Vec::with_capacity(units.len() + 2);
    let mut changed = false;
    let mut pos = 0;
    while let Some((cp, width)) = decode_at(units, pos) {
        let m = if cp == SIGMA && is_final_sigma(units, pos) { CaseMapping::One(FINAL_SIGMA) } else { lower_mapping(cp) };
        changed |= m != CaseMapping::Same;
        push_mapped(&mut out, cp, m);
        pos += width;
    }
    if changed { Cow::Owned(out) } else { Cow::Borrowed(units) }
}

/// `to_uppercase` of a Rust string.
#[must_use]
pub fn to_uppercase_str(s: &str) -> String {
    String::from_utf16_lossy(&to_uppercase(&utf16::from_str(s)))
}

/// `to_lowercase` of a Rust string.
#[must_use]
pub fn to_lowercase_str(s: &str) -> String {
    String::from_utf16_lossy(&to_lowercase(&utf16::from_str(s)))
}

/// The `Canonicalize` of a `RegExp` with the `i` flag and without `u` (ES2025 22.2.2.7.3): the single code unit
/// of `String.fromCharCode(ch).toUpperCase()` unless that is longer than one unit or would take a unit
/// of 128 or more into ASCII (`ſ`, the Kelvin sign), in which case `ch` itself. Two units match under `/i`
/// exactly when their canonical forms are equal.
#[must_use]
pub fn regexp_canonicalize(ch: u16) -> u16 {
    match upper_mapping(u32::from(ch)) {
        CaseMapping::One(mapped) => match u16::try_from(mapped) {
            Ok(cu) if !(ch >= 128 && cu < 128) => cu,
            _ => ch,
        },
        CaseMapping::Same | CaseMapping::Many(_) => ch,
    }
}

#[inline]
fn is_ascii_lower(u: u16) -> bool {
    (u16::from(b'a')..=u16::from(b'z')).contains(&u)
}

#[inline]
fn is_ascii_upper(u: u16) -> bool {
    (u16::from(b'A')..=u16::from(b'Z')).contains(&u)
}
