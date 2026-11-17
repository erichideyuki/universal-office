//! Errors of the parse and the emit.
//!
//! The TS throws in a few places on hostile data (a style number that indexes no accent and reaches
//! `hexToRgb(undefined)`, `arc`/`ellipse`/`roundRect` with a negative radius, `addColorStop` with a NaN
//! offset, a histogram whose bin count exhausts memory…); the slide's `guarded` then skips the whole
//! shape. Rust returns the error class instead and the host truncates the shape's items (record "JS
//! traps").

use crate::measure::ChartMeasureReq;

/// The class of the exception the TS would have thrown (the replay compares the class only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsErrorKind {
    TypeError,
    RangeError,
    /// a canvas `DOMException` (`IndexSizeError` of a negative radius, `SyntaxError` of a bad stop)
    DomException,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ChartError {
    /// answers are missing: measure these and emit again (two-phase; the rounds are capped by the
    /// caller, `uo_dml_render::text::MAX_ROUNDS`)
    NeedMeasures(Vec<ChartMeasureReq>),
    /// the TS threw: the host skips the shape. The text is the message of the recording canvas
    /// (`record-ctx.mjs`: `IndexSizeError: color stop offset`, `RangeError: negative radius`) or of the engine's
    /// own TypeError; the parity facets compare it.
    Throws(JsErrorKind, &'static str),
}
