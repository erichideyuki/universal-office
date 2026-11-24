//! Function bodies (frontier A4, `functions/*.ts`): none is ported yet. The evaluator asks [`body`] for the row
//! of [`crate::fnreg::FN_TABLE`] it applies; `None` makes the evaluation stop with
//! [`crate::fnreg::Throw::NotPorted`] (the A2 parity harness records which evaluations need a body).

use crate::fnreg::{FnBody, FnIx};

/// The body of a registered function, once ported.
#[must_use]
pub const fn body(_ix: FnIx) -> Option<FnBody> {
    None
}
