//! Track C grid model and viewport (feature `grid`): geometry, cell text, conditional formats, table/pivot
//! styles, borders/fills, the viewport display list. Architecture skeleton (2026-11-24); record
//! `core/docs/modules/uo-xlsx.md` "Track C architecture" (C3, C4).

/// Checkpointed prefix sums of row heights or column widths (px at the zoom), bit-equal to the TS
/// `Float64Array` of `recalcGrid` (`universal-sheet.ts:1172-1184`): the TS adds sizes one by one, so a
/// checkpoint every [`Geometry::STRIDE`] entries plus the same sequential additions inside the block give the
/// same `f64` at any index, for 8 / STRIDE bytes per row instead of 8.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// `edge(i * STRIDE)` for every block.
    pub checkpoints: Vec<f64>,
    /// Sizes that differ from the default (index, px), sorted; hidden = 0.
    pub custom: Vec<(u32, f64)>,
    /// Default size (px at the zoom).
    pub default_size: f64,
    /// Entries (`maxRow + 1` or `maxCol + 1`).
    pub count: u32,
}

impl Geometry {
    /// Entries per checkpoint.
    pub const STRIDE: u32 = 256;
}

/// What the shell asks for one frame (`paint`, `universal-sheet.ts:3222-3261`: up to four regions with frozen
/// panes, each `paintRegion` from `universal-sheet.ts:3390`).
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

/// Measurements the emission needs and the advance table lacks (two-phase, as Slides' `NeedMeasures`): per
/// cell font the `'Hg'` font box and the advances of characters without a GDI width
/// (`text-metrics.ts:54-99`), and whole-string widths for complex scripts.
#[derive(Clone, Debug, Default)]
pub struct NeedMeasures {
    /// `(font css index, UTF-16 text)` pairs; a one-unit text is a character advance.
    pub texts: Vec<(u32, Vec<u16>)>,
    /// Fonts whose `'Hg'` box is missing.
    pub font_boxes: Vec<u32>,
}
