//! Track C grid model (feature `grid`), frontier C3a: metrics, cell text, entry, geometry, resolved styles, the
//! automatic row height. The conditional formats (C3b), table/pivot styles, borders, fills and `grid-layout`
//! helpers (C3c) and the viewport display list (C4) build on it. Record `core/docs/modules/uo-xlsx.md` "Track C
//! architecture" (T6) and "C3a — as built".
//!
//! | Module | TS | What |
//! |---|---|---|
//! | [`metrics`] | `grid-metrics.ts`, `glyph-widths.ts` (+ data), `excel-metrics-data.ts`, `row-descent-data.ts` | the tables measured in Excel: MDW, row heights, GDI glyph widths, descents |
//! | [`text`] | `cell-text.ts` (+ `wrapLines`) | the displayed text for a width, `####`, General fit, `*x` fill |
//! | [`cellval`] | `universal-sheet.ts` `cellTextInput`/`displayText` | a store cell as the layout reads it |
//! | [`entry`] | `entry.ts`, `entry-format.ts`, `edit-text.ts`, `formula-locale.ts` | typed text → value/format/formula; the edit text |
//! | [`numbar`] | `number-formats.ts` | the toolbar number formats and ± decimals |
//! | [`measure`] | `text-metrics.ts` | cell fonts, the two-phase measurement table |
//! | [`style`] | `resolveStyle` | one resolved xf per `cellXfs` index |
//! | [`geometry`] | `recalcGrid`, `colWidth`/`rowHeight`, `mergeAt`, `bisectLE` | widths, heights, checkpointed edges |
//! | [`fit`] | `fitRowHeights` | the row heights Excel computes at open |
//! | [`layout`] | `cellLayout` | a store cell laid out for a width: xf → font → indent → text layout |

pub mod cellval;
pub mod cf;
#[rustfmt::skip]
mod data;
pub mod entry;
pub mod fit;
pub mod geometry;
pub mod layout;
pub mod measure;
pub mod metrics;
pub mod numbar;
pub mod style;
pub mod text;

pub use cellval::{CellCtx, CellRead, display_text, file_text_input, text_input};
pub use entry::{
    Arity, CanonicalSyntax, EditValue, Entry, EntryContext, EntryValue, FmtCategory, FormatLookup, FormulaLang, FormulaLocale,
    FormulaSyntax, NumFmtHint, auto_close, currency_hint, edit_text, error_text, file_prefix, fmt, format_category, infer_formula_format,
    number_edit_text, parse_entry, should_apply_format,
};
pub use fit::{FitEnv, RowFit, fit_row_heights};
pub use geometry::{DEFAULT_COL_W, DEFAULT_ROW_H, Geometry, MergeHit, MergeIndex, MergeRect, Runs, SheetGrid, worksheet};
pub use layout::{CellView, Laid, cell_xf_index, layout_cell};
pub use measure::{
    CellFont, FontDesc, FontEnv, FontId, FontRequest, IdentityEnv, MAX_CHARS_PER_FONT, MAX_FONTS, MeasureTable, Measurer, NeedMeasures,
    is_complex,
};
pub use metrics::{
    GlyphFixes, col_width_to_px, default_col_px, excel_mdw, excel_row_px, gdi_glyph_fixes, has_glyph_table, measured_family,
    normalize_family, ppem_of, row_descent_px, row_pt_to_px,
};
pub use numbar::{NumberFormatKind, NumberFormatSpec, adjust_decimals, number_format_for};
pub use style::{FitFacts, HAlign, NormalFont, ResolvedXf, StyleTable, VAlign};
pub use text::{
    CellKind, CellText, CellTextInput, FontMetrics, MAX_CELL_CHARS, TextPiece, autofit_wrap_width_px, cell_margin_px, iso_to_serial,
    layout_cell_text, localize_bool, localize_error, looks_like_number_pt_br, text_width, wrap_lines,
};

/// What the shell asks for one frame (`paint`, `universal-sheet.ts:3222-3261`: up to four regions with frozen
/// panes, each `paintRegion` from `universal-sheet.ts:3390`). The viewport emission is frontier C4.
#[derive(Clone, Debug, Default)]
pub struct ViewportRequest {
    /// Sheet.
    pub sheet: u32,
    /// Canvas size (CSS px) and device pixel ratio.
    pub width: f64,
    /// Canvas height.
    pub height: f64,
    /// Device pixel ratio.
    pub dpr: f64,
    /// Scroll offsets (px at the zoom).
    pub scroll_x: f64,
    /// Vertical scroll.
    pub scroll_y: f64,
    /// Zoom (0.4-3).
    pub zoom: f64,
    /// Selection (1-based active cell and range) for the selection layer.
    pub selection: [u32; 6],
}
