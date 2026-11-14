//! Word semantics shared by the layout and the session (uo-docx-layout LQ3), ported 1:1 from
//! `write/packages/docx-layout/src` by uo-docx-layout frontier F2 (Opus); record
//! `core/docs/modules/uo-docx-layout.md`, "Wave 1 — F2".
//!
//! | TS | module |
//! |---|---|
//! | `resolve.ts` (merges, `StyleResolver`, `cascadeRPr`, `fontOfSlot`, colours) | [`resolve`] |
//! | `context.ts` `directRPr`/`directPPr`, `para.ts` `resolvePPr`, `paraformat.ts` | [`direct`] |
//! | `numbering.ts` | [`numbering`] |
//! | `datefmt.ts` | [`datefmt`] |
//! | `field-formula.ts` (+ docx-core `table-formula.ts` `formatFormulaResult`, `formatPicture`) | [`field_formula`] |
//! | `outline.ts` | [`outline`] |
//! | `numpages.ts` | [`numpages`] |
//! | `revisions.ts` | [`revisions`] |
//! | `hyphenate.ts` `documentHyphenationLanguages` (K5: an explicit `lang`-site list) | [`hyphlangs`] |
//! | `refs.ts` `cellAtGrid`, `paragraphAtPath` (`cellBlocks` = `Story::blocks`) | [`refs`] |
//!
//! The JS object operations they are written in (`{ ...a, ...b }`, `Object.assign`, `delete`, key
//! order, `JSON.stringify` equality, V8's sort) are [`obj`]; JS number/string helpers beyond
//! uo-jsnum are [`num`].
//!
//! Everything here is plain data or pure functions over the model: per-call caches only (K3,
//! LQ18), no global state, no panics on document data, every recursion over untrusted text bounded
//! (K11c).

pub mod datefmt;
pub mod direct;
pub mod field_formula;
pub mod field_switch;
pub mod hyphlangs;
pub mod num;
pub mod numbering;
pub mod numpages;
pub mod numwords;
pub mod obj;
pub mod outline;
pub mod refs;
pub mod resolve;
pub mod revisions;
pub mod style_names;

pub use datefmt::{LocalDateTime, date_field_value, format_word_date, time_clip};
pub use direct::{ResolvedPPr, direct_ppr, direct_rpr, direct_rpr_with, effective_ppr, inherited_ppr, resolve_ppr};
pub use field_formula::{
    CellNumber, FieldValue, FormulaError, WordFormulaError, evaluate_field_formula, evaluate_field_formula_word, field_name,
    format_formula_result, format_picture, has_nested_field, word_function,
};
pub use hyphlangs::document_hyphenation_languages;
pub use numbering::{LevelCount, LevelSrc, NumRules, NumberMarker, NumberingState, TextCap, format_number, format_number_word};
pub use numpages::{COVER_PAGES_GALLERY, has_cover_page, num_pages_shown};
pub use outline::{
    ListItemValue, NumberedItem, OutlineHeading, OutlineParagraph, document_outline, heading_text, list_item_values, numbered_items,
    outline_level_of, outline_paragraphs,
};
pub use refs::{cell_at_grid, paragraph_at_path};
pub use resolve::{
    COND_ORDER, ColorRef, FontSlot, ResolvedParaStyle, ResolvedTableStyle, StyleResolver, TOGGLES, apply_tint_shade, cascade_rpr,
    font_of_slot, highlight_hex, merge_ppr, merge_rfonts, merge_rpr, merge_tbl_pr, merge_tc_pr, merge_tr_pr, own, resolve_color,
    resolve_drawing_color, shading_fill, style_chain,
};
pub use revisions::{
    AUTHOR_PALETTE, BAR_COLOR, BAR_WIDTH, MOVE_COLOR, MarkKind, RevisionDisplay, RevisionMode, RevisionRanges, RunRevision,
    SIMPLE_BAR_COLOR, author_color, bar_x, join_offset, revision_ranges,
};
