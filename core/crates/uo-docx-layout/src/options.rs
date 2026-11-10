//! The options of a layout call (plan §3 `LayoutOptions`) and its limits (§7, LQ14, K16).
//!
//! Defaults equal the TS ones (`DocContext` constructor, context.ts:117): `lang: 'pt-BR'`,
//! hyphenation on, the final revision view. The lab-only TS options (`onWordFit`,
//! `advanceEm: 'device'`, `advanceRound` other than `'word'`, a `justifyShrink` override) are gone
//! (LQ12): the engine always runs `advanceEm: 'nominal'`, `advanceRound: 'word'`,
//! `justifyShrink: { ratio: 0.25, expandRatio: 0.5 }`. The callbacks became data (`onField` →
//! `collect_fields`, `hyphenationSkip` → `hyphenation_skips`). Two fields have no TS counterpart (G4):
//! `cancel` (a shared flag) and `cancel_poll` (a host function), both read in `Budget::charge`; `None`
//! changes nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use indexmap::{IndexMap, IndexSet};

use crate::error::{LResult, LayoutError, LimitKind};
use crate::model::{FieldScope, HyphenSkips};

/// `hyphenation?: boolean | 'force'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hyphenation {
    #[default]
    On,
    Off,
    /// as if the document had `w:autoHyphenation` (the editor's Manual Hyphenation search)
    Force,
}

/// `RevisionMode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RevisionMode {
    #[default]
    Final,
    Simple,
    All,
    Original,
}

/// `RevisionView` (revisions.ts:21): how tracked changes show.
#[derive(Clone, Debug, Default)]
pub struct RevisionView {
    pub mode: RevisionMode,
    /// `insDel` (default true)
    pub ins_del: Option<bool>,
    /// `formatting` (default true)
    pub formatting: Option<bool>,
    /// `authors` (`null`/absent = all)
    pub authors: Option<Vec<Box<str>>>,
    /// `colors` per author (the TS Record, key order kept)
    pub colors: Option<IndexMap<Box<str>, Box<str>>>,
    /// `order` (absent: the document's read order, `revisionAuthors`)
    pub order: Option<Vec<Box<str>>>,
}

/// The view the engine lays out (LQ2: the legacy canvas layout is gone; every view is the engine
/// over a transformed document).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ViewTransform {
    #[default]
    Print,
    /// the viewer's `webDocument(full, widthTw)` (views.ts:27)
    Web { width_tw: f64 },
    /// Web at the section's text width
    Draft,
    /// designed with milestone 2
    Outline,
}

/// The time the date fields see (LQ10): the JS time value of `opts.now` and the rule that gives
/// the local offset of any instant (the TS reads local getters of `Date`s: `now`, `CREATEDATE`…).
#[derive(Clone, Debug)]
pub struct Clock {
    /// `now.getTime()` (ms since the epoch)
    pub now_ms: f64,
    pub tz: TimeZoneRule,
}

/// Local offset of an instant (the binding builds it from the host: `Env`).
#[derive(Clone, Debug)]
pub enum TimeZoneRule {
    /// a fixed offset, minutes east of UTC (UTC = 0; the captures run under TZ=UTC)
    Fixed { offset_min: i32 },
    /// transitions `(from_ms, offset_min)`, ascending: the offset of the last one at or before
    /// the instant (the São Paulo facet of the captures)
    Table(Arc<[(f64, i32)]>),
}

impl Default for Clock {
    fn default() -> Clock {
        Clock { now_ms: 0.0, tz: TimeZoneRule::Fixed { offset_min: 0 } }
    }
}

/// A hyphenation pattern set (`HYPHENATION_PATTERN_IDS`: `pt`, `en-us`, `es`, `fr`, `de-1996`,
/// `it`, `nl`).
pub type PatternId = Box<str>;

/// Options of one layout call.
#[derive(Clone, Debug)]
pub struct LayoutOptions {
    /// `lang` for numbers and dates (default `pt-BR`)
    pub lang: Box<str>,
    pub clock: Clock,
    pub hyphenation: Hyphenation,
    /// the pattern sets the TS process had loaded (K5, LQ16): computed by the binding as its
    /// shell does, recorded in every capture
    pub hyphenation_loaded: IndexSet<PatternId>,
    /// `hyphenationLanguages`
    pub hyphenation_languages: Option<Vec<Box<str>>>,
    /// `hyphenationSkip`
    pub hyphenation_skips: HyphenSkips,
    pub revisions: Option<RevisionView>,
    pub view: ViewTransform,
    /// `onField`: which field values to collect into `Layout::fields` (`None` = none)
    pub collect_fields: Option<FieldScope>,
    pub limits: LayoutLimits,
    /// G4: raised by the caller from any thread; checked at every work charge. The same shape
    /// as uo-pdf's `PdfOptions::cancel` and uo-ffi's `UoCancel`. A raised flag ends the layout
    /// with `LayoutError::Cancelled`.
    pub cancel: Option<Arc<AtomicBool>>,
    /// G4: polled every `CANCEL_POLL_UNITS` work units (and at the first charge); `true`
    /// cancels. For hosts that cannot share a flag (wasm: the `uo_host.layout_poll` import).
    pub cancel_poll: Option<fn() -> bool>,
}

/// Work units between two polls of `LayoutOptions::cancel_poll` (G4).
pub const CANCEL_POLL_UNITS: u64 = 1 << 12;

impl Default for LayoutOptions {
    fn default() -> LayoutOptions {
        LayoutOptions {
            lang: "pt-BR".into(),
            clock: Clock::default(),
            hyphenation: Hyphenation::On,
            hyphenation_loaded: IndexSet::new(),
            hyphenation_languages: None,
            hyphenation_skips: HyphenSkips::default(),
            revisions: None,
            view: ViewTransform::Print,
            collect_fields: None,
            limits: LayoutLimits::default(),
            cancel: None,
            cancel_poll: None,
        }
    }
}

/// Limits on hostile documents (§7). Each sits above everything the corpus and the stress set
/// reach (the capture records the per-counter maxima); results differ from the TS only where it
/// hangs, exhausts memory or throws a `RangeError` (LQ14, K11).
#[derive(Clone, Copy, Debug)]
pub struct LayoutLimits {
    /// pages of one layout
    pub max_pages: u32,
    /// cells of one table (rows × cells)
    pub max_table_cells: u32,
    /// layout recursion (cell → static flow → table → cell, textboxes, notes) on top of the
    /// model's `max_nesting` 64 (LQ13)
    pub max_depth: u32,
    /// recursion of every parser over untrusted text (`parseArithmetic`, K11c)
    pub max_parse_depth: u32,
    /// EMF/WMF records per metafile; past it the drawing is truncated with a warning
    pub max_metafile_records: u32,
    /// items one metafile may emit
    pub max_metafile_items: u32,
    /// UTF-16 units of one field result (roman/ordinal of 1e15, `\#` and date pictures)
    pub max_field_text: u32,
    /// the work budget (K16): `work_floor + work_per_glyph × glyphs × passes`, where `passes` is
    /// what the TS loop can legitimately run for the document (5 + the balancing bound of its
    /// balanced sections), times `work_nesting_factor` per table nesting level in use
    pub work_floor: u64,
    pub work_per_glyph: u64,
    pub work_nesting_factor: u64,
}

impl Default for LayoutLimits {
    fn default() -> LayoutLimits {
        LayoutLimits {
            max_pages: 100_000,
            max_table_cells: 4_000_000,
            max_depth: 192,
            max_parse_depth: 256,
            max_metafile_records: 1_000_000,
            max_metafile_items: 1_000_000,
            max_field_text: 65_536,
            work_floor: 50_000_000,
            work_per_glyph: 4_000,
            work_nesting_factor: 4,
        }
    }
}

/// The inputs of the work budget, measured once per call before the first pass (K16).
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkInput {
    /// UTF-16 units of every story (an upper bound of the prepared glyphs)
    pub glyphs: u64,
    /// sections ended by a continuous break with columns (each may run a balancing search)
    pub balanced_sections: u32,
    /// deepest table nesting in use
    pub nesting: u32,
    /// height of the tallest page in twips (the balancing search is a bisection over it)
    pub max_page_h: f64,
}

/// The work counter of one layout call (§7: where the TS would hang, Rust stops with
/// `Limit(Work)`). Every pass, probe and `pure()` re-layout charges it; the milestone-2
/// checkpoints follow the last executed pass (K19).
#[derive(Clone, Debug)]
pub struct Budget {
    left: u64,
    spent: u64,
    /// the `pure()` flow clones (LA4: part of the budget)
    pub pure_clones: u64,
    /// G4
    cancel: Option<Arc<AtomicBool>>,
    poll: Option<fn() -> bool>,
    next_poll: u64,
    cancelled: bool,
}

impl Budget {
    /// The budget of a document: input-proportional (K16).
    #[must_use]
    pub fn new(l: &LayoutLimits, w: &WorkInput) -> Budget {
        // passes the TS loop may run: 5, plus per balanced section (≤ 20 searches,
        // engine.ts:263) a bisection of ⌈log2 h⌉ + 2 full passes
        let probes = u64::from(w.balanced_sections.min(20)) * (log2_ceil(w.max_page_h) + 2);
        let passes = 5 + probes;
        let nesting = l.work_nesting_factor.saturating_pow(w.nesting.min(8));
        let left =
            l.work_floor.saturating_add(l.work_per_glyph.saturating_mul(w.glyphs.max(1)).saturating_mul(passes).saturating_mul(nesting));
        Budget { left, spent: 0, pure_clones: 0, cancel: None, poll: None, next_poll: 0, cancelled: false }
    }

    /// An unlimited budget (tests of single stages).
    #[must_use]
    pub fn unlimited() -> Budget {
        Budget { left: u64::MAX, spent: 0, pure_clones: 0, cancel: None, poll: None, next_poll: 0, cancelled: false }
    }

    /// G4: cancellation by a shared flag (every charge) and/or a poll (every `CANCEL_POLL_UNITS`;
    /// the first charge polls too).
    #[must_use]
    pub fn with_cancel(mut self, cancel: Option<Arc<AtomicBool>>, poll: Option<fn() -> bool>) -> Budget {
        self.cancel = cancel;
        self.poll = poll;
        self.next_poll = 0;
        self
    }

    /// Spends `units`; `Err(Cancelled)` once the caller cancelled (checked first, latched),
    /// `Err(Limit(Work))` once the budget is gone.
    pub fn charge(&mut self, units: u64) -> LResult<()> {
        if !self.cancelled {
            if self.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                self.cancelled = true;
            } else if let Some(p) = self.poll
                && self.spent >= self.next_poll
            {
                self.next_poll = self.spent.saturating_add(CANCEL_POLL_UNITS);
                self.cancelled = p();
            }
        }
        if self.cancelled {
            return Err(LayoutError::Cancelled);
        }
        if units > self.left {
            self.left = 0;
            return Err(LayoutError::Limit(LimitKind::Work));
        }
        self.left -= units;
        self.spent = self.spent.saturating_add(units);
        Ok(())
    }

    #[must_use]
    pub fn spent(&self) -> u64 {
        self.spent
    }
}

fn log2_ceil(h: f64) -> u64 {
    // the bisection runs while hi - lo > 1 over at most `h` twips (engine.ts:280)
    let mut n = 0u64;
    let mut v = 1.0f64;
    while v < h && n < 64 {
        v *= 2.0;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_raised_flag_cancels_and_latches() {
        let flag = Arc::new(AtomicBool::new(false));
        let mut b = Budget::unlimited().with_cancel(Some(flag.clone()), None);
        assert!(b.charge(1).is_ok());
        flag.store(true, Ordering::Relaxed);
        assert_eq!(b.charge(1), Err(LayoutError::Cancelled));
        flag.store(false, Ordering::Relaxed);
        assert_eq!(b.charge(1), Err(LayoutError::Cancelled));
    }

    #[test]
    fn cancelled_wins_over_an_exhausted_budget() {
        let flag = Arc::new(AtomicBool::new(true));
        let mut b = Budget::new(&LayoutLimits::default(), &WorkInput::default()).with_cancel(Some(flag), None);
        assert_eq!(b.charge(u64::MAX), Err(LayoutError::Cancelled));
    }

    static POLLS: AtomicUsize = AtomicUsize::new(0);
    fn counting() -> bool {
        POLLS.fetch_add(1, Ordering::Relaxed);
        false
    }

    #[test]
    fn the_poll_runs_at_the_first_charge_and_once_per_interval() {
        let mut b = Budget::unlimited().with_cancel(None, Some(counting));
        b.charge(1).unwrap();
        assert_eq!(POLLS.load(Ordering::Relaxed), 1);
        for _ in 0..(CANCEL_POLL_UNITS - 2) {
            b.charge(1).unwrap();
        }
        assert_eq!(POLLS.load(Ordering::Relaxed), 1);
        b.charge(1).unwrap();
        b.charge(1).unwrap();
        assert_eq!(POLLS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn unlimited_without_cancel_never_fails() {
        let mut b = Budget::unlimited();
        for _ in 0..10_000 {
            assert!(b.charge(1 << 20).is_ok());
        }
    }
}
