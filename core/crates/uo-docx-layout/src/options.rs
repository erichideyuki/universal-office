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
    /// wave 7c (V): the EDITOR's continuous views (Web, Draft, Outline) as the frozen TS shows them — ONE sheet for the
    /// whole document (no new sheet at a section, page or column break), the TS margin (`contMarginTwips`) — with the
    /// content as Word lays it out in those views (record `uo-bindings.md` "Wave 7c — V"). The viewer's `Web`/`Draft`
    /// above stay as they are (its Web view, the f8 sweeps).
    Continuous(ContinuousView),
}

/// Which continuous view of the editor (see [`ViewTransform::Continuous`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContinuousKind {
    Web,
    Draft,
    Outline,
}

/// The editor's continuous view (see [`ViewTransform::Continuous`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ContinuousView {
    pub kind: ContinuousKind,
    /// Web: the sheet's width in twips, the TS `contentWidthTwips() + 2 × contMarginTwips()` (universal-write.ts 10777,
    /// 21559); Draft and Outline: unused (the sheet is the last section's page width, as the TS canvas)
    pub width_tw: f64,
    /// the TS `contMarginTwips()` (10764): 600, or 240 on a narrow element
    pub margin_tw: f64,
    /// `true`: the frozen TS's rules where Word differs (Draft/Outline wrap every section at the LAST section's text
    /// width; Outline = the TS `+720` rule for the unstyled paragraphs, spacing kept). The default `false` follows Word
    /// (each section at its own text width; Outline by outline level, paragraph spacing dropped). Strict replays only.
    pub ts_strict: bool,
}

impl ViewTransform {
    /// Whether the view is one of the editor's continuous views (one sheet, the forced breaks inside it).
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self, ViewTransform::Continuous(_))
    }

    /// Wave 7c (V, fix round 2): Word's Draft or Outline view (the editor's continuous view of those kinds without
    /// `ts_strict`), where a page, column or section break takes a line of its own (measured: record "Wave 7c — V, fix
    /// round 2").
    #[must_use]
    pub fn word_break_lines(&self) -> bool {
        matches!(self, ViewTransform::Continuous(v) if !v.ts_strict && matches!(v.kind, ContinuousKind::Draft | ContinuousKind::Outline))
    }
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
    /// Wave 7 (B, gate-O3): `true` = the frozen TS's empty theme for a package without a theme part (its theme colours
    /// fall back to `w:color`); the default `false` follows Word (the Office 2023 colour scheme, measured;
    /// [`crate::theme_word`]). Only the strict parity replays against the frozen TS captures set it.
    pub ts_empty_theme: bool,
    /// Wave 7 (merge fixer F2, round 2; ED-713): `true` = the frozen TS's PAGEREF page (`bookmarkPages`, engine.ts:638:
    /// the page's format without the chapter numbers); the default `false` follows Word, which writes a PAGEREF as the
    /// PAGE field of the bookmark's page, chapter numbers (`w:chapStyle`/`w:chapSep`) included (measured: `1-1`,
    /// `1-i`; uo-bindings.md "Wave 7 — merge fixer F2, round 2"). Only the strict parity replays against the frozen
    /// TS captures set it.
    pub ts_pageref_without_chapter: bool,
    /// Wave 7c (W, item 1): reserved for the theme tint/shade rounding (the frozen TS's `applyTintShade`, resolve.ts:327:
    /// HSL lightness, channels floored). Word's rule is NOT found yet (3,356 measured samples in
    /// `tests/oracle_w7c/tint`: the TS rule hits 2,323, the best candidate 2,662; item 1 is OPEN, uo-bindings.md
    /// "Wave 7c — W"), so both values give the TS rule today; the strict replays set it already, so the rule can land
    /// without touching them.
    pub ts_theme_tint: bool,
    /// Wave 7c (W, item 2): `true` = the frozen TS's compatibility mode 15 for a package WITHOUT `word/settings.xml`
    /// (parts.ts:411); the default `false` follows Word, which opens such a package in mode 12 (COM
    /// `Document.CompatibilityMode` = 12, measured). Only the strict parity replays set it.
    pub ts_missing_settings_mode15: bool,
    /// Wave 7c (W, item 3): `true` = the frozen TS's chain of linked text boxes anchored in a table cell (mode 15: the
    /// first box stays empty, the chain starts in the second); the default `false` follows Word (the first box holds
    /// the chain's start; also in the body: a box of a chain is never left empty). It also holds the second rule of the
    /// `txbxcell` series (fix round 1, reviewer W/6): in mode 15 Word wraps a table cell's text around ANY floating box
    /// anchored in the cell with `layoutInCell="0"` (linked or not; `engine/floats.rs`), which the frozen TS does not.
    /// Only the strict parity replays set it.
    pub ts_linked_box_cell_chain: bool,
    /// Wave 7c (W, item 4): `true` = the frozen TS's REF/PAGEREF/NOTEREF to a missing, unterminated or inverted
    /// bookmark (the start paragraph's text, page 1, the cached result); the default `false` writes Word's error texts
    /// (bold) and a multi-paragraph range as Word does. Only the strict parity replays set it.
    pub ts_ref_bookmarks: bool,
    /// Wave 7c (W, item 5): reserved for hyphenation pattern sets closer to Word's (proprietary) hyphenator; none ships
    /// yet (item 5 is OPEN, uo-bindings.md "Wave 7c — W"), so both values use the frozen TS's sets today. The strict
    /// replays set it already.
    pub ts_hyphenation_patterns: bool,
    /// Wave 7c (W, item 6; F2's f7a__7__0): `true` = the frozen TS's keepLines rule (engine.ts:1860: a keep-together
    /// paragraph moves only when it fits a whole page); the default `false` follows Word, which moves it to the next
    /// page even when it is taller than a page (measured). Only the strict parity replays set it.
    pub ts_keep_lines_within_page: bool,
}

impl LayoutOptions {
    /// Wave 7c (W): every `ts_*` switch on — the frozen TS's behaviour where Word (the authority) differs. For the
    /// strict parity replays against the frozen TS captures ONLY; no product path calls it.
    #[must_use]
    pub fn ts_strict(self) -> LayoutOptions {
        LayoutOptions {
            ts_empty_theme: true,
            ts_pageref_without_chapter: true,
            ts_theme_tint: true,
            ts_missing_settings_mode15: true,
            ts_linked_box_cell_chain: true,
            ts_ref_bookmarks: true,
            ts_hyphenation_patterns: true,
            ts_keep_lines_within_page: true,
            ..self
        }
    }
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
            ts_empty_theme: false,
            ts_pageref_without_chapter: false,
            ts_theme_tint: false,
            ts_missing_settings_mode15: false,
            ts_linked_box_cell_chain: false,
            ts_ref_bookmarks: false,
            ts_hyphenation_patterns: false,
            ts_keep_lines_within_page: false,
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
