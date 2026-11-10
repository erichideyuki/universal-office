//! Errors of a layout call (plan §3, §7, LQ9, LQ14).

/// Which limit stopped a layout (`LayoutLimits`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LimitKind {
    /// the work budget (input-proportional, K16): where the TS would hang
    Work,
    /// pages past `max_pages`
    Pages,
    /// a table past `max_table_cells`
    Table,
    /// recursion past the layout depth guard or a recursive parser's guard (LQ13, K11c)
    Depth,
    /// the display list past `u32` entries in one column (uo-render `CapacityError`)
    Capacity,
}

/// The F0 skeleton's unported stages (stage 3 only: the variant and every use of it are gone
/// before gate 2; `tests/stubs.rs` lists them so the count can only go down).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Frontier {
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7a,
    F7b,
    F7c,
    F8,
}

/// A layout that produced no pages. After the switch a `LayoutError` means no page and no PDF
/// (LQ2: the engine-failure fallbacks of the TS shells are gone).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LayoutError {
    /// a limit of `LayoutLimits` (LQ14): the TS hangs, exhausts memory or recurses without end
    Limit(LimitKind),
    /// where the TS would throw (`x[i]!` on a missing element, an explicit `throw`): the site
    Internal(&'static str),
    /// no face at all for the document's default font and every fallback (the TS throws when the
    /// provider has nothing, fonts.ts)
    NoFace,
    /// G4: the caller raised `LayoutOptions::cancel`, or `cancel_poll` answered true; no pages
    Cancelled,
    /// **stage 3 only**: the stage is not ported yet (frontier, function)
    Unported(Frontier, &'static str),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::Limit(k) => write!(f, "layout limit: {k:?}"),
            LayoutError::Internal(site) => write!(f, "layout failed at {site}"),
            LayoutError::NoFace => f.write_str("no font face"),
            LayoutError::Cancelled => f.write_str("layout cancelled"),
            LayoutError::Unported(fr, what) => write!(f, "not ported yet ({fr:?}): {what}"),
        }
    }
}

impl std::error::Error for LayoutError {}

impl From<uo_render::CapacityError> for LayoutError {
    fn from(_: uo_render::CapacityError) -> LayoutError {
        LayoutError::Limit(LimitKind::Capacity)
    }
}

/// `Result` of the layout stages.
pub type LResult<T> = Result<T, LayoutError>;
