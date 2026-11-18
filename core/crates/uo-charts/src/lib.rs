//! The shared chart engine of the UniversalOffice Rust core, used by Slides (through
//! `uo-dml-render`'s `emit::chart`) and by Sheet (charts in drawings and chart sheets): the model
//! of a DrawingML chart (`c:chartSpace`, and the Office 2016 `cx:chartSpace` converted into the same
//! model), its parser over the **host's parsed XML tree and theme colour resolver** (the repository
//! rule "Gráficos e formatação numérica: um lugar só": the host hands the tree and its resolver, the
//! engine never parses XML nor knows the clrMap), the automatic value scale, trendlines, histogram
//! bins, and the layout and painting of a chart as canvas calls ([`emit::Canvas`]) that the host's
//! emitter turns into its display list (`uo_render::slides` for Slides and the Sheet viewport). Text is
//! measured through advance tables (decision T1), with the two-phase protocol of `uo-dml-render`
//! (`need_measures`, then the emit).
//!
//! Architecture record: `core/docs/modules/uo-charts.md` (data model, host interface, measurement
//! sites, emission and schema additions, frontiers C-0..C-7, parity, oracle, memory, fuzz, JS traps).
//!
//! Behaviour source: the frozen TS `shared/charts/src` (0.9.2, `b92b60fa`, worktree `ts-ref-092`):
//! `model.ts`, `parse.ts`, `chartex.ts`, `scale.ts`, `trend.ts`, `color.ts`, `dml-color.ts`,
//! `image.ts` (`imageDpi`; `tilePattern` moves to the shell painter), `render.ts`; the host bridge
//! `slides/packages/slides-core/src/chart-engine.ts` (`chartPartBounds`). `numfmt.ts` is `uo-numfmt`.
//!
//! **Status**: C-1 done (2026-11-17): the model ([`model`]), the parser ([`parse`]: `parse_chart_space`,
//! `parse_paint`, [`CHART_PREFIXES`]), the colours ([`color`]: the automatic colours and `dml_color`) and
//! [`summarize`]. Still skeleton (types, no logic): the host's measurement protocol ([`measure`]), the
//! emission interface ([`emit`]) and the layout results ([`layout`]); `chartex` is a hook for C-2.
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

pub mod biff;
#[cfg(feature = "canon")]
pub mod canon;
pub mod chartex;
pub mod color;
pub mod emit;
pub mod error;
pub mod host;
pub mod image;
pub mod layout;
pub mod measure;
pub mod model;
pub mod parse;
pub mod render;
pub mod rules;
pub mod scale;
pub mod summary;
pub mod trend;

pub use color::{Scheme, dml_color, dml_color_source, dml_color_source_with, dml_color_with};
pub use emit::{Canvas, ChartSel, DataSel, EmitParams, HostProfile};
pub use error::{ChartError, JsErrorKind};
pub use host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
pub use measure::{ChartFace, ChartMeasure, ChartMeasureReq, FontBox};
pub use model::{ChartColor, ChartSpace};
pub use parse::{CHART_PREFIXES, parse_chart_space, parse_paint, parse_paint_node};
pub use render::{
    BoundsCanvas, LayoutCall, LayoutTrace, NullCanvas, ValLabel, emit_chart, need_measures, part_bounds, set_wrap_prefetch, trace_layout,
};
pub use summary::{ChartSummary, ChartType, summarize};

/// Empties every cache of this crate. The engine keeps none across calls (record "Memory": the TS
/// caches are per paint or belong to the hosts); kept for the workspace-wide `clear_caches` contract.
pub fn clear_caches() {}
