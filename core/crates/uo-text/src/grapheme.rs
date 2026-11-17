//! Extended grapheme clusters as `Intl.Segmenter(undefined, { granularity: 'grapheme' })` of Node 24.15 splits
//! them (ICU 78.2, Unicode 17.0: UAX #29 rules GB3–GB13 with GB9c, the Indic conjunct rule). The Slides text
//! layout breaks a word longer than the line by these clusters (`text-layout.ts:217`), so a base letter and its
//! combining marks, an emoji ZWJ sequence or a flag never split across lines.
//!
//! The property tables are probed from the segmenter itself (`js/gen-grapheme.mjs`, never a Unicode data file),
//! and the generator checks the rules below against the segmenter before writing; `tests/grapheme_v8.rs` replays
//! its oracle. A lone surrogate is its own code point (the segmenter gives the lone unit no property:
//! Any). No state, no allocation in the iterators, no panic on any input.

use crate::grapheme_tables as t;
use crate::utf16::decode_at;

pub use crate::grapheme_tables::{ICU_VERSION, UNICODE_VERSION};

/// `Grapheme_Cluster_Break` (the values the rules distinguish).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gcb {
    Any,
    Cr,
    Lf,
    Control,
    Extend,
    Zwj,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
}

/// `Indic_Conjunct_Break` (GB9c).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCb {
    None,
    Consonant,
    Extend,
    Linker,
}

/// The value of a run table `(first, last | value << 24)` at `cp`, 0 when absent.
fn run_value(table: &[(u32, u32)], cp: u32) -> u32 {
    let idx = table.partition_point(|&(first, _)| first <= cp);
    match idx.checked_sub(1).and_then(|i| table.get(i)) {
        Some(&(_, tail)) if cp <= tail & 0x00FF_FFFF => tail >> 24,
        _ => 0,
    }
}

/// The `Grapheme_Cluster_Break` of a code point (a surrogate code point = the lone unit).
#[must_use]
pub fn gcb(cp: u32) -> Gcb {
    match run_value(&t::GCB, cp) {
        1 => Gcb::Cr,
        2 => Gcb::Lf,
        3 => Gcb::Control,
        4 => Gcb::Extend,
        5 => Gcb::Zwj,
        6 => Gcb::RegionalIndicator,
        7 => Gcb::Prepend,
        8 => Gcb::SpacingMark,
        9 => Gcb::L,
        10 => Gcb::V,
        11 => Gcb::T,
        12 => Gcb::Lv,
        13 => Gcb::Lvt,
        _ => Gcb::Any,
    }
}

/// `\p{Extended_Pictographic}`.
#[must_use]
pub fn is_extended_pictographic(cp: u32) -> bool {
    let idx = t::EXT_PICT.partition_point(|&(first, _)| first <= cp);
    matches!(idx.checked_sub(1).and_then(|i| t::EXT_PICT.get(i)), Some(&(_, last)) if cp <= last)
}

/// The `Indic_Conjunct_Break` of a code point.
#[must_use]
pub fn incb(cp: u32) -> InCb {
    match run_value(&t::INCB, cp) {
        1 => InCb::Consonant,
        2 => InCb::Extend,
        3 => InCb::Linker,
        _ => InCb::None,
    }
}

/// The forward state of the rules: feed code points in order, each call says whether a cluster boundary falls
/// before that code point. The first code point never has one (the text start is not an interior boundary).
#[derive(Clone, Copy, Debug)]
pub struct Breaker {
    prev: Option<Gcb>,
    /// regional indicators in a row ending at the previous code point (GB12/GB13)
    ri: u32,
    /// the previous code point ends `ExtPict Extend*`
    ep_seq: bool,
    /// the previous code point is the ZWJ of `ExtPict Extend* ZWJ` (GB11)
    ep_zwj: bool,
    /// 0 none, 1 `Consonant [Extend Linker]*`, 2 the same with a Linker seen (GB9c)
    conj: u8,
}

impl Default for Breaker {
    fn default() -> Self {
        Breaker::new()
    }
}

impl Breaker {
    #[must_use]
    pub const fn new() -> Breaker {
        Breaker { prev: None, ri: 0, ep_seq: false, ep_zwj: false, conj: 0 }
    }

    /// The rules in order: does the previous code point (`p`) join the current one (`g`, `ep`, `ic`)?
    fn joins(&self, p: Gcb, g: Gcb, ep: bool, ic: InCb) -> bool {
        let ctl = |x: Gcb| matches!(x, Gcb::Cr | Gcb::Lf | Gcb::Control);
        if p == Gcb::Cr && g == Gcb::Lf {
            return true; // GB3
        }
        if ctl(p) || ctl(g) {
            return false; // GB4, GB5
        }
        let hangul = (p == Gcb::L && matches!(g, Gcb::L | Gcb::V | Gcb::Lv | Gcb::Lvt)) // GB6
            || (matches!(p, Gcb::Lv | Gcb::V) && matches!(g, Gcb::V | Gcb::T)) // GB7
            || (matches!(p, Gcb::Lvt | Gcb::T) && g == Gcb::T); // GB8
        let marks = matches!(g, Gcb::Extend | Gcb::Zwj | Gcb::SpacingMark) || p == Gcb::Prepend; // GB9, GB9a, GB9b
        let conjunct = ic == InCb::Consonant && self.conj == 2; // GB9c
        let emoji = p == Gcb::Zwj && ep && self.ep_zwj; // GB11
        if hangul || marks || conjunct || emoji {
            return true;
        }
        // GB12, GB13, else GB999
        p == Gcb::RegionalIndicator && g == Gcb::RegionalIndicator && self.ri % 2 == 1
    }

    /// Is there a boundary before `cp`? Advances the state past it.
    pub fn step(&mut self, cp: u32) -> bool {
        let g = gcb(cp);
        let ep = is_extended_pictographic(cp);
        let ic = incb(cp);
        let boundary = self.prev.is_some_and(|p| !self.joins(p, g, ep, ic));
        self.ep_zwj = g == Gcb::Zwj && self.ep_seq;
        self.ep_seq = ep || (self.ep_seq && g == Gcb::Extend);
        self.ri = if g == Gcb::RegionalIndicator { self.ri.saturating_add(1) } else { 0 };
        self.conj = match ic {
            InCb::Consonant => 1,
            InCb::Linker if self.conj > 0 => 2,
            InCb::Extend if self.conj > 0 => self.conj,
            _ => 0,
        };
        self.prev = Some(g);
        boundary
    }
}

/// The interior cluster boundaries of a UTF-16 string as offsets (0 and the length excluded), in order.
pub fn boundaries(units: &[u16]) -> impl Iterator<Item = usize> + '_ {
    let mut at = 0usize;
    let mut br = Breaker::new();
    core::iter::from_fn(move || {
        while let Some((cp, width)) = decode_at(units, at) {
            let here = at;
            at += width;
            if br.step(cp) {
                return Some(here);
            }
        }
        None
    })
}

/// The grapheme clusters of a valid string (`Array.from(segmenter.segment(s), (x) => x.segment)`).
#[must_use]
pub fn graphemes(s: &str) -> Graphemes<'_> {
    Graphemes { s, br: Breaker::new(), pending: None }
}

/// Iterator of [`graphemes`].
#[derive(Clone, Debug)]
pub struct Graphemes<'a> {
    s: &'a str,
    br: Breaker,
    /// the first code point of the next cluster, already stepped through the breaker
    pending: Option<char>,
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let mut chars = self.s.char_indices();
        let mut end = match self.pending.take() {
            Some(c) => {
                chars.next();
                c.len_utf8()
            }
            None => {
                let (_, c) = chars.next()?;
                self.br.step(u32::from(c));
                c.len_utf8()
            }
        };
        for (i, c) in chars {
            if self.br.step(u32::from(c)) {
                self.pending = Some(c);
                let (head, tail) = self.s.split_at(i);
                self.s = tail;
                return Some(head);
            }
            end = i + c.len_utf8();
        }
        let head = self.s.get(..end).unwrap_or(self.s);
        self.s = "";
        Some(head)
    }
}
