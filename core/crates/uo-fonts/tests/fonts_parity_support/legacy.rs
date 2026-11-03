//! G16: the C# `TtfProgram` legacy `kern` reader (`KernAdjust`, `HasKerning`) against
//! `FontProgram::legacy_kern`. The pairs asked are derived from the raw `kern` table bytes by a
//! scanner that both sides repeat (`tests/parity_support/legacy_kern.cs` has the same code), not
//! from either parser: every format-0 pair record of the last `kern` table, seeded random pairs
//! and fixed edge pairs (right above 0xFFFF, left above 0xFFFF).
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::HashSet;
use std::sync::Arc;

use uo_fonts::{FontLimits, FontProgram};
use uo_testkit::Stream;
use uo_testkit::generate::Rng;

use super::facets::num_glyphs_of;

const EDGE: [(u32, u32); 8] =
    [(0, 0x1_0000), (1, 0x1_0001), (0xFFFF, 0xFFFF), (0, 0), (0xFFFF, 0x1_0000), (0x1_0000, 0), (0x1_0000, 1), (0x7FFF_FFFF, 0x7FFF_FFFF)];

fn u16at(b: &[u8], o: usize) -> Option<usize> {
    b.get(o..o + 2).map(|x| usize::from(u16::from_be_bytes([x[0], x[1]])))
}

/// Every `(left, right)` of a format-0 subtable record of the last `kern` table, in first-seen
/// order, at most 100,000.
pub fn kern_pairs(b: &[u8]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let Some(n) = u16at(b, 4) else { return out };
    let mut kern = None;
    for i in 0..n {
        let p = 12 + i * 16;
        let (Some(tag), Some(off)) = (b.get(p..p + 4), b.get(p + 8..p + 12)) else { return out };
        if tag == b"kern" {
            kern = Some(u32::from_be_bytes([off[0], off[1], off[2], off[3]]) as usize);
        }
    }
    let Some(kern) = kern else { return out };
    let (Some(n_tables), mut p) = (u16at(b, kern + 2), kern + 4) else { return out };
    let mut seen = HashSet::new();
    for _ in 0..n_tables {
        let (Some(length), Some(coverage)) = (u16at(b, p + 2), u16at(b, p + 4)) else { break };
        if coverage >> 8 == 0 {
            let Some(n_pairs) = u16at(b, p + 6) else { break };
            for i in 0..n_pairs {
                let r = p + 14 + i * 6;
                let (Some(l), Some(rt)) = (u16at(b, r), u16at(b, r + 2)) else { break };
                if seen.insert((l, rt)) {
                    out.push((l as u32, rt as u32));
                    if out.len() >= 100_000 {
                        return out;
                    }
                }
            }
        }
        p += length;
    }
    out
}

/// The facet value: `E` when the program does not open, else `items:digest` of the flag and the
/// answers.
pub fn legacy_facet(bytes: &Arc<[u8]>) -> String {
    let Ok(p) = FontProgram::parse(bytes.clone(), &FontLimits::NATIVE) else { return "E".to_owned() };
    let mut s = Stream::new();
    s.tag(b'K').bool(p.has_legacy_kern());
    let ask = |s: &mut Stream, l: u32, r: u32| {
        s.u32(l).u32(r).i64(i64::from(p.legacy_kern(l, r)));
    };
    for (l, r) in kern_pairs(bytes) {
        ask(&mut s, l, r);
    }
    let n = num_glyphs_of(bytes) as usize;
    let mut rng = Rng::new(0x4C4B_4552);
    for _ in 0..10_000 {
        let l = rng.below(n + 2) as u32;
        let r = rng.below(n + 2) as u32;
        ask(&mut s, l, r);
    }
    for (l, r) in EDGE {
        ask(&mut s, l, r);
    }
    format!("{}:{}", s.items(), s.finish().short())
}
