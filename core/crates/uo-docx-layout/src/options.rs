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
    /// WordCraft study (fields) C1: `true` = the frozen TS's line metrics of a Symbol/Wingdings run or bullet drawn by
    /// a stand-in face (the stand-in's own); the default `false` follows Word, which keeps Symbol's ascent 2059/2048 em
    /// and descent 450/2048 em (Wingdings 1841/432) on the line (`oracle_wc/c1`).
    pub ts_symbol_font_metrics: bool,
    /// WordCraft study (fields) C1b: `true` = the frozen TS's "multiple" line spacing over (max ascent + max descent)
    /// of the line's text; the default `false` follows Word: (factor − 1) × the largest OWN height (ascent + descent)
    /// of the line's text runs (`oracle_wc/c1`, Calibri + Symbol/Tahoma/Courier New).
    pub ts_multiple_text_height: bool,
    /// WordCraft study (fields) C5: `true` = the frozen TS's PostScript names ("Arial-BoldMT" adds bold); the default
    /// `false` follows Word: a PostScript or camel-case name is the family at the caller's weight, a full name ("Arial
    /// Negrito") the face it names.
    pub ts_postscript_names: bool,
    /// WordCraft study (fields) B21: `true` = the frozen TS's STYLEREF (the cached result in headers/footers, heading
    /// levels only); the default `false` follows Word: per page in headers/footers, style names (UI name, custom name,
    /// alias) and Word's error texts (`oracle_wc/styleref`).
    pub ts_styleref_per_page: bool,
    /// WordCraft study (fields) C2: `true` = the frozen TS's names with a charset suffix ("Arial CE" is a missing
    /// family); the default `false` follows Word: the Windows FontSubstitutes names and a trailing " (Hebrew)"-style
    /// script resolve to the base family (`oracle_wc/c2`).
    pub ts_charset_suffix_names: bool,
    /// WordCraft study (fields) B22: `true` = the frozen TS's `\*` switches (SEQ and formulas only, positive integers,
    /// pt-BR words); the default `false` follows Word: every number field, case switches on text results, number words
    /// in the field's language (`oracle_wc/fields22`).
    pub ts_field_format_switches: bool,
    /// WordCraft study (fields) B26: `true` = the frozen TS's list number words (pt-BR up to 99, "1º" for ordinalText);
    /// the default `false` follows Word: the words of the paragraph mark's language (`oracle_wc/lists26`).
    pub ts_list_number_language: bool,
    /// WordCraft study (fields) B27: `true` = the frozen TS's `w:isLgl` (the parents only); the default `false` follows
    /// Word: every placeholder Arabic, decimalZero kept, a bullet placeholder empty (`oracle_wc/lists27`).
    pub ts_is_lgl: bool,
    /// WordCraft study (fields) B23: `true` = the frozen TS's SECTIONPAGES and continuous restart; the default `false`
    /// follows Word: a page shared through a continuous break counts for both sections, and a continuous section's
    /// `pgNumType start` numbers its first (shared) page (`oracle_wc/sections23`).
    pub ts_section_pages_shared: bool,
    /// WordCraft study (fields) C6: `true` = the frozen TS's missing symbol-charset font; the default `false` follows
    /// Word: a missing charset-02 font resolves to Symbol (roman family, or Symbol's PANOSE) or Wingdings
    /// (`oracle_wc/c6`).
    pub ts_symbol_charset_fallback: bool,
    /// WordCraft study (fields) B24: `true` = the frozen TS's REF `\r` (the level's number) and `\p` (the UI language);
    /// the default `false` follows Word: `\r` drops the leading levels the field's own list position shares, `\p` in
    /// the language of the field paragraph's style (`oracle_wc/refs24`).
    pub ts_ref_relative: bool,
    /// WordCraft study (fields) B25: `true` = the frozen TS's formula fields (+ - * / only, the cached result on any
    /// error); the default `false` follows Word: comparisons, `^`, `%`, the 18 functions and Word's error texts
    /// (`oracle_wc/formula25`).
    pub ts_hf_formula: bool,
    /// WordCraft study (fields) A2 aside: `true` = the frozen TS's font slots; the default `false` follows Word: a
    /// ja/zh/ko document default language draws every character with the eastAsia font (MS Mincho for a font without
    /// CJK), and a CJK fallback from a Latin font is MS Mincho (`fonts_sys`).
    pub ts_east_asian_default_lang: bool,
    /// WordCraft study (fields) C4: `true` = the frozen TS's missing Thai font; the default `false` follows Word: a
    /// charset-DE font resolves to Angsana New (roman), Browallia New (swiss) or Leelawadee UI, and Thai text falls
    /// back to Leelawadee UI (`fonts_sys`).
    pub ts_thai_fallback: bool,
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
    /// Wave 7c (W, item 5), given its meaning by the WordCraft study (fields, D1): `true` = the frozen TS's English
    /// patterns (hyph-utf8 `en-us`); the default `false` takes `en-us-word` for English when it is loaded (DesignCraft's
    /// CC0 patterns + the Moby Hyphenator II exceptions: 95.80 % of Word's paragraphs on `hyphword1k` against 88.27 %).
    /// pt and es are unchanged. The strict replays set it.
    pub ts_hyphenation_patterns: bool,
    /// Wave 7c (W, item 6; F2's f7a__7__0): `true` = the frozen TS's keepLines rule (engine.ts:1860: a keep-together
    /// paragraph moves only when it fits a whole page); the default `false` follows Word, which moves it to the next
    /// page even when it is taller than a page (measured). Only the strict parity replays set it.
    pub ts_keep_lines_within_page: bool,
    /// WordCraft study (lines, L1/A24; `wcl/l-spaces-*`): `true` = the frozen TS's line height, where only U+0020 and the
    /// tab give a line no height; the default `false` follows Word: none of the SPACE_LIKE set (U+0020, U+2002, U+2003,
    /// U+2005, U+200B, U+3000) does. Strict replays only.
    pub ts_space_like_height: bool,
    /// WordCraft study (lines, L2/A21; `wcl/l-numsp`): `true` = the frozen TS's SPACE suffix after a list number (the
    /// number face's space); the default `false` follows Word: Arial's space (569/2048 em) at the number's size, and with
    /// lvlJc right/center the number and the suffix aligned as one unit. Strict replays only.
    pub ts_number_suffix_font_space: bool,
    /// WordCraft study (lines, L3/A1; jsweep + `wcl/l-jboth-*`): `true` = the frozen TS's jc=both compression slacks
    /// (0.0175 tw on the 25 % cap, 0.0075 tw per space); the default `false` follows Word (0.035 and 0.025). Strict only.
    pub ts_justify_tolerance: bool,
    /// WordCraft study (lines, L4; `wcl2/l-kinsoku-*`, `wcl3/m-kinsoku-*`): `true` = the frozen TS's kinsoku (no-end only
    /// before a space, one hang list); the default `false` follows Word's per-language no-start / no-end / hang lists
    /// (ja, zh-CN, zh-TW; ko none). Strict replays only.
    pub ts_kinsoku_lists: bool,
    /// WordCraft study (lines, L5; `wcl2/l-cgrid-*`, `wcl3/m-cgrid-*`): `true` = the frozen TS ignores the character grid
    /// (`w:docGrid` linesAndChars / snapToChars and snapToChars' line grid); the default `false` follows Word. Strict only.
    pub ts_no_char_grid: bool,
    /// WordCraft study (lines, L6; `wcl2/l-charsind-*`, `wcl3/m-charsind2`): `true` = the frozen TS ignores
    /// firstLineChars / leftChars / hangingChars / rightChars; the default `false` applies them as Word. Strict only.
    pub ts_no_chars_indent: bool,
    /// WordCraft study (lines, L7; `wcl2/l-shiftret*`): `true` = the frozen TS expands a jc=both line ending in a `w:br`
    /// even with `w:doNotExpandShiftReturn`; the default `false` follows Word (not expanded). Strict replays only.
    pub ts_shift_return_expands: bool,
    /// WordCraft study (lines, L8/A20; `wcl/l-ptab-*`): `true` = the frozen TS's `w:ptab` (targets in the line frame, no
    /// break); the default `false` follows Word's targets (relativeTo margin/indent) and its line break. Strict only.
    pub ts_ptab_indent_frame: bool,
    /// WordCraft study (lines, L9/A19 + A1; `wcl/l-jdist-*`, `wcl3/m-jdist-*`, `m-jspread48-*`): `true` = the frozen TS's
    /// jc=distribute (spaces only, as "both"); the default `false` follows Word (the two-phase spread over the letter
    /// gaps, also on the last line, and the compression factor by xAvgCharWidth). Strict replays only.
    pub ts_distribute_spaces_only: bool,
    /// WordCraft study (lines, L10/A22; `wcl/l-negtab`): `true` = the frozen TS's default stop left of the margin
    /// (strictly after the pen); the default `false` follows Word (the first default stop AT or after a negative pen).
    pub ts_default_tab_after_negative: bool,
    /// WordCraft study (lines, L11/A23; `wcl/l-bartab*`): `true` = the frozen TS counts bar tabs as the last custom stop
    /// for the default grid; the default `false` follows Word (bar tabs do not count). Strict replays only.
    pub ts_bar_tab_counts: bool,
    /// WordCraft study (lines, L12; `wcl3/m-emph-*`, `wcl4/n-emph-mix`, `wcl5/o-emph-sp-*`): `true` = the frozen TS ignores
    /// `w:em`; the default `false` follows Word (the line's extra ascent/descent and the marks). Strict replays only.
    pub ts_no_emphasis_marks: bool,
    /// WordCraft study (lines, L13/A11; `L/suptable-word.json`): `true` = the frozen TS's superscript/subscript formula
    /// above 24 pt; the default `false` follows Word's measured per-font table. Strict replays only.
    pub ts_supsub_formula_above_24: bool,
    /// WordCraft study (page, A15): `true` = the frozen TS's space before a paragraph that OPENS with a page break
    /// (`before − previous after` in every mode); the default `false` follows Word 16.0.20527: in mode 15 such a paragraph
    /// starts its page with no space before. Strict replays only.
    pub ts_page_break_keeps_space_before: bool,
    /// WordCraft study (page, A16): `true` = the frozen TS's `newPage` (a blank page whenever an odd/even section's number
    /// has the wrong parity, never for evenAndOddHeaders/mirrorMargins; the blank page carries the section's header and
    /// counts in SECTIONPAGES); the default `false` follows Word. Strict replays only.
    pub ts_section_parity: bool,
    /// WordCraft study (page): `true` = the frozen TS ignores `w:gutter`, `w:rtlGutter`, `w:gutterAtTop` and
    /// `w:mirrorMargins`; the default `false` applies them as Word does. Strict replays only.
    pub ts_no_gutter_mirror: bool,
    /// WordCraft study (page): `true` = inside/outside of floats by the displayed page number (the frozen TS); the
    /// default `false` uses the document's physical parity, as Word. Strict replays only.
    pub ts_page_parity_by_number: bool,
    /// WordCraft study (page, B10): `true` = a float belongs to the page of its paragraph's FIRST line and a
    /// line-relative float measures from the paragraph top (the frozen TS); the default `false` follows Word (the page,
    /// column and line of the anchor run). Strict replays only.
    pub ts_float_anchor_first_line: bool,
    /// WordCraft study (page, B11): `true` = only absolute floats are laid out again at their page start (the frozen
    /// TS); the default `false` follows Word, which lays the page out again around a paragraph/line-relative wrapping
    /// float that overlaps text already laid there. Strict replays only.
    pub ts_float_no_reflow: bool,
    /// WordCraft study (page): `true` = the frozen TS ignores `w:mirrorIndents`; the default `false` mirrors the indents
    /// as Word does. Strict replays only.
    pub ts_no_mirror_indents: bool,
    /// WordCraft study (page): `true` = the frozen TS ignores `w:bookFoldPrinting`, `w:bookFoldRevPrinting` and
    /// `w:printTwoOnOne`; the default `false` applies their margins as Word's PDF does. Strict replays only.
    pub ts_no_print_fold: bool,
    /// WordCraft study (tables, B5; `oracle_wc/rowkeep`): `true` = the frozen TS keeps a table row with the next when ANY
    /// paragraph of ANY of its cells has keepNext (table.ts, oracle keeptbl); the default `false` follows Word: only the
    /// first paragraph (in document order) of the row's first cell decides. Strict replays only.
    pub ts_row_keep_any_paragraph: bool,
    /// WordCraft study (tables, B6; `oracle_wc/condbd`): `true` = the frozen TS gives each conditional part's
    /// `w:tcBorders` sides to EVERY cell of the part (insideH/insideV never read, wholeTable's applied); the default
    /// `false` follows Word: a part's borders are its REGION's (outer sides on the region's edges, insideH/insideV
    /// between its cells) and wholeTable `w:tcBorders` are ignored. Strict replays only.
    pub ts_table_style_cell_borders: bool,
    /// WordCraft study (tables, B12; `oracle_wc/ftedge`): `true` = the frozen TS's wrap box of a floating table (the
    /// grid ± fromText); the default `false` follows Word's edges (the right border's width, or 15 tw without one, and
    /// at least 10 tw of fromText on each side; mode ≤ 14 from the painted outer borders). Strict replays only.
    pub ts_float_table_wrap_edge: bool,
    /// WordCraft study (tables, B15 rule A; `oracle_wc/cellclamp`): `true` = the frozen TS leaves a wrapping object in a
    /// table cell where its offset puts it; the default `false` follows Word, which clamps it horizontally into the
    /// cell's interior (between the inner edges of the painted left and right borders). Strict replays only.
    pub ts_cell_float_no_clamp: bool,
    /// WordCraft study (tables, B15 rule B; `oracle_wc/cellclamp` `fw-*`): `true` = the frozen TS's word that does not
    /// fit the strip beside a float (the body: the line goes below the float in every mode; cells: split by characters);
    /// the default `false` follows Word: mode 15 goes below (body and cells), mode ≤ 14 splits (body and cells).
    pub ts_gap_word_any_mode: bool,
    /// WordCraft study (tables, the TCond aside; `oracle_wc/tblorder`): `true` = the frozen TS reads `w:tblPr` in any
    /// order; the default `false` follows Word, for which a `w:tblStyle` naming an existing table style AFTER other
    /// children discards the direct properties read before it. Strict replays only.
    pub ts_tbl_style_order: bool,
}

impl LayoutOptions {
    /// Wave 7c (W): every `ts_*` switch on — the frozen TS's behaviour where Word (the authority) differs. For the
    /// strict parity replays against the frozen TS captures ONLY; no product path calls it.
    #[must_use]
    pub fn ts_strict(self) -> LayoutOptions {
        LayoutOptions {
            ts_symbol_font_metrics: true,
            ts_multiple_text_height: true,
            ts_postscript_names: true,
            ts_styleref_per_page: true,
            ts_charset_suffix_names: true,
            ts_field_format_switches: true,
            ts_list_number_language: true,
            ts_is_lgl: true,
            ts_section_pages_shared: true,
            ts_symbol_charset_fallback: true,
            ts_ref_relative: true,
            ts_hf_formula: true,
            ts_east_asian_default_lang: true,
            ts_thai_fallback: true,
            ts_empty_theme: true,
            ts_pageref_without_chapter: true,
            ts_theme_tint: true,
            ts_missing_settings_mode15: true,
            ts_linked_box_cell_chain: true,
            ts_ref_bookmarks: true,
            ts_hyphenation_patterns: true,
            ts_keep_lines_within_page: true,
            ts_space_like_height: true,
            ts_number_suffix_font_space: true,
            ts_justify_tolerance: true,
            ts_kinsoku_lists: true,
            ts_no_char_grid: true,
            ts_no_chars_indent: true,
            ts_shift_return_expands: true,
            ts_ptab_indent_frame: true,
            ts_distribute_spaces_only: true,
            ts_default_tab_after_negative: true,
            ts_bar_tab_counts: true,
            ts_no_emphasis_marks: true,
            ts_supsub_formula_above_24: true,
            ts_page_break_keeps_space_before: true,
            ts_section_parity: true,
            ts_no_gutter_mirror: true,
            ts_page_parity_by_number: true,
            ts_float_anchor_first_line: true,
            ts_float_no_reflow: true,
            ts_no_mirror_indents: true,
            ts_no_print_fold: true,
            ts_row_keep_any_paragraph: true,
            ts_table_style_cell_borders: true,
            ts_float_table_wrap_edge: true,
            ts_cell_float_no_clamp: true,
            ts_gap_word_any_mode: true,
            ts_tbl_style_order: true,
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
            ts_symbol_font_metrics: false,
            ts_multiple_text_height: false,
            ts_postscript_names: false,
            ts_styleref_per_page: false,
            ts_charset_suffix_names: false,
            ts_field_format_switches: false,
            ts_list_number_language: false,
            ts_is_lgl: false,
            ts_section_pages_shared: false,
            ts_symbol_charset_fallback: false,
            ts_ref_relative: false,
            ts_hf_formula: false,
            ts_east_asian_default_lang: false,
            ts_thai_fallback: false,
            ts_empty_theme: false,
            ts_pageref_without_chapter: false,
            ts_theme_tint: false,
            ts_missing_settings_mode15: false,
            ts_linked_box_cell_chain: false,
            ts_ref_bookmarks: false,
            ts_hyphenation_patterns: false,
            ts_keep_lines_within_page: false,
            ts_space_like_height: false,
            ts_number_suffix_font_space: false,
            ts_justify_tolerance: false,
            ts_kinsoku_lists: false,
            ts_no_char_grid: false,
            ts_no_chars_indent: false,
            ts_shift_return_expands: false,
            ts_ptab_indent_frame: false,
            ts_distribute_spaces_only: false,
            ts_default_tab_after_negative: false,
            ts_bar_tab_counts: false,
            ts_no_emphasis_marks: false,
            ts_supsub_formula_above_24: false,
            ts_page_break_keeps_space_before: false,
            ts_section_parity: false,
            ts_no_gutter_mirror: false,
            ts_page_parity_by_number: false,
            ts_float_anchor_first_line: false,
            ts_float_no_reflow: false,
            ts_no_mirror_indents: false,
            ts_no_print_fold: false,
            ts_row_keep_any_paragraph: false,
            ts_table_style_cell_borders: false,
            ts_float_table_wrap_edge: false,
            ts_cell_float_no_clamp: false,
            ts_gap_word_any_mode: false,
            ts_tbl_style_order: false,
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
    /// WordCraft study (page, B10/B11): the passes Word's float re-layout may add (one per late float, at most 20; 0
    /// under the frozen TS's switches)
    pub extra_passes: u32,
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
        let passes = 5 + probes + u64::from(w.extra_passes.min(20));
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
