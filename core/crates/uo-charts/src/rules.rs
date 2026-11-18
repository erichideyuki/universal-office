//! Office-over-TS switches of the chart engine (Slides wave 5).
//!
//! Each `ts_*` field, when `true`, keeps the frozen TS behaviour (`ts-ref-092`) where PowerPoint was measured to
//! differ; the default (`false`) follows PowerPoint. [`ChartRules::ts_strict`] turns every switch on: the parity
//! replays against the TS captures use it, no product path does. A rule is added here only with its measurement
//! (record `docs/modules/uo-charts.md`, "PowerPoint rules (wave 5)").

/// The chart engine's PowerPoint-over-TS switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ChartRules {
    /// Chart-space insets as the TS computes them. PowerPoint (measured, lab `c-pie`/`c-r2`): the title box from 6 pt
    /// below the top (its text 1.5 pt lower), a bottom/top legend box 6 pt from its edge, the plot box 11 pt from the
    /// frame and 11 pt from the title and the legends.
    pub ts_insets: bool,
    /// The automatic title as the TS creates it (only in Office 2007 files). PowerPoint: any file with
    /// `<c:autoTitleDeleted val="0"/>` written and no `<c:title>` shows the name of the only series (or of the first
    /// series of a pie), bold, 18 pt with `c14:style` (or an Office 2007 file), else the chart's base size.
    pub ts_auto_title: bool,
    /// Pie and doughnut size as the TS computes it (97 % of the plot square, minus 1.6 label lines a side for outside
    /// labels). PowerPoint: the whole plot square; with labels that may go outside (pie: `bestFit`/`outEnd`/no
    /// position; a doughnut with any label) the diameter is `0.875 × side − 2.75 pt`, whatever the label size.
    pub ts_pie_size: bool,
    /// Error bars (`c:errBars`) ignored, as the TS does. PowerPoint draws them (measured, lab `c-err`/`c-err2`): ±amount
    /// lines from the point (a bar: its centre at the value end), 4.5-pt end caps whatever the line width, fixed /
    /// percentage of |value| / std error (sample σ ÷ √n, on each point) / std dev (val × sample σ around the series MEAN,
    /// every point) / custom `c:plus`·`c:minus`; the auto scale includes their ends.
    pub ts_err_bars: bool,
}

impl ChartRules {
    /// Every switch on: the frozen TS behaviour, for parity replays only.
    #[must_use]
    pub fn ts_strict() -> ChartRules {
        ChartRules { ts_insets: true, ts_auto_title: true, ts_pie_size: true, ts_err_bars: true }
    }
}
