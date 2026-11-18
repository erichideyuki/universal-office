//! PowerPoint-over-TS switches of the Slides read model and emission (Slides wave 5), in the manner of Write's
//! `LayoutOptions::ts_*`.
//!
//! Each `ts_*` field, when `true`, keeps the frozen TS behaviour (`ts-ref-092`) where PowerPoint was measured to
//! differ; the default (`false`) follows PowerPoint, which decides where the two disagree. [`SlidesRules::ts_strict`]
//! turns every switch on (charts included): the parity replays against the TS captures use it, no product path does.
//! A rule is added here only with its measurement (records `docs/modules/uo-dml-render.md`, `uo-pptx.md`).

use uo_charts::rules::ChartRules;

/// The PowerPoint-over-TS switches read by the emission ([`crate::emit::Env::rules`]) and the model readers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SlidesRules {
    /// `a:sysClr` resolved from `lastClr` as the TS does (PowerPoint uses the system colour).
    pub ts_sys_clr: bool,
    /// First-line baseline as the TS places it (PowerPoint measured per font).
    pub ts_first_baseline: bool,
    /// Fallback fonts for scripts the face lacks (CJK, Thai, Arabic…) as the TS picks them.
    pub ts_fallback_fonts: bool,
    /// Date/time fields formatted as the TS does (PowerPoint uses the field's language / system locale).
    pub ts_date_locale: bool,
    /// WordArt warp geometry as the TS computes it.
    pub ts_warp: bool,
    /// The chart engine's switches.
    pub charts: ChartRules,
}

impl SlidesRules {
    /// Every switch on: the frozen TS behaviour, for parity replays only.
    #[must_use]
    pub fn ts_strict() -> SlidesRules {
        SlidesRules {
            ts_sys_clr: true,
            ts_first_baseline: true,
            ts_fallback_fonts: true,
            ts_date_locale: true,
            ts_warp: true,
            charts: ChartRules::ts_strict(),
        }
    }
}
