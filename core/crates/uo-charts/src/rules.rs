//! Office-over-TS switches of the chart engine (Slides wave 5).
//!
//! Each `ts_*` field, when `true`, keeps the frozen TS behaviour (`ts-ref-092`) where PowerPoint was measured to
//! differ; the default (`false`) follows PowerPoint. [`ChartRules::ts_strict`] turns every switch on: the parity
//! replays against the TS captures use it, no product path does. A rule is added here only with its measurement
//! (record `docs/modules/uo-charts.md`).

/// The chart engine's PowerPoint-over-TS switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ChartRules {
    /// Plot-area insets as the TS computes them (PowerPoint measured otherwise).
    pub ts_insets: bool,
}

impl ChartRules {
    /// Every switch on: the frozen TS behaviour, for parity replays only.
    #[must_use]
    pub fn ts_strict() -> ChartRules {
        ChartRules { ts_insets: true }
    }
}
