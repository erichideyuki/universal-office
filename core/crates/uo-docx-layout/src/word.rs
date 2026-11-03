//! The Word semantics shared with the session (LQ3) live in **`uo_docx::word`** (frontier F2,
//! record "Wave 1 — F2"): `resolve.ts`, `paraformat.ts`, `directRPr`/`directPPr`/`resolvePPr`,
//! `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `outline.ts`, `numpages.ts`, `revisions.ts`
//! and the `documentHyphenationLanguages` walk. The engine reaches them through this facade only
//! (like `model`, the one place that names that module of uo-docx).
//!
//! The glue to the layout's own types is here: the local date parts of the [`Clock`] (LQ10: the
//! TS reads local `Date` getters in the process time zone; Rust takes the offset at the instant
//! from the [`TimeZoneRule`]), the [`RevisionDisplay`] of the options' [`RevisionView`]
//! (`new RevisionDisplay(opts.revisions, opts.revisions?.order ?? [])`, context.ts:115), and the
//! text cap of the numbering formats from the layout limits (`max_field_text`, K11b).

pub use uo_docx::word::{
    AUTHOR_PALETTE, BAR_COLOR, BAR_WIDTH, COVER_PAGES_GALLERY, MOVE_COLOR, RevisionDisplay, SIMPLE_BAR_COLOR, cell_at_grid,
    paragraph_at_path,
};
pub use uo_docx::word::{
    COND_ORDER, CellNumber, ColorRef, FieldValue, FontSlot, FormulaError, LevelCount, LevelSrc, ListItemValue, LocalDateTime, MarkKind,
    NumberMarker, NumberedItem, NumberingState, OutlineHeading, OutlineParagraph, ResolvedPPr, ResolvedParaStyle, ResolvedTableStyle,
    RevisionRanges, RunRevision, StyleResolver, TOGGLES, TextCap, apply_tint_shade, author_color, bar_x, cascade_rpr, date_field_value,
    direct_ppr, direct_rpr, direct_rpr_with, document_hyphenation_languages, document_outline, effective_ppr, evaluate_field_formula,
    field_name, font_of_slot, format_formula_result, format_number, format_picture, format_word_date, has_cover_page, has_nested_field,
    heading_text, highlight_hex, inherited_ppr, join_offset, list_item_values, merge_ppr, merge_rfonts, merge_rpr, merge_tbl_pr,
    merge_tc_pr, merge_tr_pr, num_pages_shown, numbered_items, outline_level_of, outline_paragraphs, own, resolve_color,
    resolve_drawing_color, resolve_ppr, revision_ranges, shading_fill, style_chain, time_clip,
};

use crate::options::{Clock, LayoutLimits, RevisionMode, RevisionView, TimeZoneRule};

impl TimeZoneRule {
    /// The local offset (minutes east of UTC) at the time value `t` (ms): the fixed offset, or the
    /// offset of the last transition at or before `t` (the first one's before the table).
    #[must_use]
    pub fn offset_min_at(&self, t: f64) -> i32 {
        match self {
            TimeZoneRule::Fixed { offset_min } => *offset_min,
            TimeZoneRule::Table(tr) => {
                let mut off = tr.first().map_or(0, |e| e.1);
                for &(from, o) in tr.iter() {
                    if from <= t {
                        off = o;
                    } else {
                        break;
                    }
                }
                off
            }
        }
    }
}

impl Clock {
    /// The local parts of the time value `t` (ms since the epoch) in this clock's zone: what the
    /// `Date` getters return (`LocalTime(t)` = `t` + the offset at `t`); an invalid time (NaN, past
    /// ±8.64e15 ms) gives [`LocalDateTime::INVALID`].
    #[must_use]
    pub fn local(&self, t: f64) -> LocalDateTime {
        let t = time_clip(t);
        if t.is_nan() {
            return LocalDateTime::INVALID;
        }
        LocalDateTime::at(t, f64::from(self.tz.offset_min_at(t)) * 60_000.0)
    }

    /// The local parts of `now` (`opts.now`, the DATE/TIME fields).
    #[must_use]
    pub fn local_now(&self) -> LocalDateTime {
        self.local(self.now_ms)
    }
}

/// `new RevisionDisplay(opts.revisions, opts.revisions?.order ?? [])` (context.ts:115).
#[must_use]
pub fn revision_display(view: Option<&RevisionView>) -> RevisionDisplay {
    let Some(v) = view else { return RevisionDisplay::default() };
    let mode = match v.mode {
        RevisionMode::Final => uo_docx::word::RevisionMode::Final,
        RevisionMode::Simple => uo_docx::word::RevisionMode::Simple,
        RevisionMode::All => uo_docx::word::RevisionMode::All,
        RevisionMode::Original => uo_docx::word::RevisionMode::Original,
    };
    let order = v.order.as_deref().unwrap_or(&[]);
    RevisionDisplay::new(mode, v.ins_del, v.formatting, v.authors.as_deref(), v.colors.as_ref(), v.order.as_deref(), order)
}

/// The cap of the generated texts (roman, alphabetic and chicago counters, K11b) from the limits.
#[must_use]
pub fn text_cap(limits: &LayoutLimits) -> TextCap {
    TextCap::new(usize::try_from(limits.max_field_text).unwrap_or(usize::MAX))
}

/// `new NumberingState(fmt.numbering, fmt.styles.styles, lang)` with the limits' text cap.
#[must_use]
pub fn numbering_state(lang: &str, limits: &LayoutLimits) -> NumberingState {
    NumberingState::new(lang, text_cap(limits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn clock_local_parts() {
        let c = Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: -180 } };
        let d = c.local_now();
        assert_eq!((d.year, d.month, d.date, d.hours), (2026.0, 0.0, 15.0, 9.0));
        let t = TimeZoneRule::Table(Arc::from([(0.0, -120), (100.0, -180)]));
        assert_eq!(t.offset_min_at(-5.0), -120);
        assert_eq!(t.offset_min_at(100.0), -180);
        assert!(Clock { now_ms: f64::NAN, tz: t }.local_now().year.is_nan());
    }
}
