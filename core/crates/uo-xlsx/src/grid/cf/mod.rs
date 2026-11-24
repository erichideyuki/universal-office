//! Conditional formats (frontier C3b, feature `grid`): `cond-format.ts` (the engine), `cf-formula.ts` (its own
//! formula evaluator, ported 1:1 — merging it into the calc is evolution after M5) and `cf-icons.ts`. Record
//! `core/docs/modules/uo-xlsx.md` "C3b — as built".
//!
//! | Module | TS | What |
//! |---|---|---|
//! | [`value`] | `cf-formula.ts:12-15, 175-220, 311-319` | values, coercions, `compare`, `keyOf`, V8 dates, wildcards |
//! | [`formula`] | `cf-formula.ts` | tokenizer, parser (arena), evaluation over [`formula::CfCells`] |
//! | [`engine`] | `cond-format.ts` | [`CfEngine`]: rules by priority, statistics once per version, the per-cell memo |
//! | [`layer`] | `applyCfLayer` + `cellLayout` (`universal-sheet.ts`) | the text layout of a cell under its CF layer |
//! | [`icons`] | `cf-icons.ts`, `paintDataBar` | the canvas operations of an icon and a data bar |
//!
//! The viewport (C4) asks [`CfEngine::at`] per visible cell, lays the text out with [`layer::layout_cell_cf`] and
//! paints the fill ([`CfEngine::fill_color`] or the dxf's fill), the font colour/borders of the dxfs, the bar
//! ([`CfEngine::bar_paint`] → [`icons::bar_rects`]) and the icon ([`CfEngine::icon_of`] → [`icons::icon_ops`]).
//! An edit calls [`CfEngine::invalidate`] (C5).

pub mod engine;
pub mod formula;
pub mod icons;
pub mod layer;
pub mod value;

pub use engine::{
    BarPaint, CfBar, CfCell, CfClock, CfContext, CfDxf, CfEngine, CfFill, CfIcon, CfLimits, Rect, StoreCells, cf_value, decode_strings,
    parse_sqref, prepare_book, rgb_text,
};
pub use icons::{BarFill, BarRect, CanvasOp, IconKind, bar_rects, icon_ops, icon_set};
pub use layer::layout_cell_cf;
