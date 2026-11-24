//! Function bodies (frontier A4, `functions/*.ts`), one module per family, and the dispatch the evaluator asks
//! ([`body`]): a table indexed by [`FnIx`] (the row of [`crate::fnreg::FN_TABLE`]), built once from the families'
//! lists. A function without a body makes the evaluation stop with [`crate::fnreg::Throw::NotPorted`].
//!
//! # The family contract
//!
//! A family is a module `functions::<family>` (a file or a directory) that exports exactly one item for this file,
//! one list per TS file of the family:
//!
//! ```ignore
//! const TEXT: &[(&str, FnBody)] = shared::bodies![ "NAME" => function, … ];
//! pub(super) const BODIES: &[&[(&str, FnBody)]] = &[TEXT, …];
//! ```
//!
//! - `"NAME"` is the canonical registry name (upper case, no `_xlfn.` prefix) — its metadata (argument modes,
//!   lift, volatile, min/max) is the generated [`crate::fnreg::FN_TABLE`] row, so a body never re-declares it;
//! - `function` is `fn(&CallArgs<'_>, &mut dyn FnContext) -> shared::R<Value>` (the `bodies!` macro wraps it into
//!   the evaluator's [`FnBody`]); `R` carries an Excel error ([`shared::Stop::Err`], returned as a value) or a host
//!   throw ([`shared::Stop::Throw`]: the engine's `NeedCell`, a `RangeError`) through `?`;
//! - the shared layer ([`shared`]) holds what more than one family uses: argument access and outputs, the
//!   `math-util.ts` walkers and factories ([`shared::mathu`]), the `stats.ts` walkers ([`shared::statw`]),
//!   `tdl-util.ts` ([`shared::tdl`]) and the basic aggregations ([`shared::agg`]). A family adds helpers inside its
//!   own module; a helper a second family needs moves into `shared` (owned by whoever needs it second).
//!
//! The table is checked by `tests` below: every name exists in the registry, none is declared twice. A family never
//! edits this file: its list is wired here already.
//!
//! | Module | Frontier | TS |
//! |---|---|---|
//! | [`math`] | A4a | `math.ts`, `math-util.ts` (bodies), `complex.ts`, `engineering.ts` (`libm.ts` is [`crate::libm`]) |
//! | [`stats`] | A4b | `statistical.ts`, `distributions.ts`, `regression.ts`, `ets.ts` |
//! | [`finance`] | A4c | `financial.ts`, `daycount.ts`, `datetime.ts` |
//! | [`text`] | A4d | `text.ts`, `text-format.ts`, `regex.ts`, `filterxml.ts`, `regional.ts` (`tdl-util.ts` is [`shared::tdl`]) |
//! | [`lookup`] | A4e | `lookup.ts`, `reference.ts`, `dynamic.ts`, `database.ts`, `conditional.ts` |
//! | [`logical`] | A4f | `logical.ts`, `info.ts`, `cube.ts` |

use std::sync::OnceLock;

use crate::fnreg::{FN_TABLE, FnBody, FnIx};

pub mod finance;
pub mod logical;
pub mod lookup;
pub mod math;
pub mod shared;
pub mod stats;
pub mod text;

/// The families' lists, in a fixed order.
const FAMILIES: [&[&[(&str, FnBody)]]; 6] = [math::BODIES, stats::BODIES, finance::BODIES, text::BODIES, lookup::BODIES, logical::BODIES];

/// The dispatch table: one slot per row of [`FN_TABLE`].
fn table() -> &'static [Option<FnBody>] {
    static TABLE: OnceLock<Box<[Option<FnBody>]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Vec<Option<FnBody>> = vec![None; FN_TABLE.len()];
        for list in FAMILIES.iter().flat_map(|f| f.iter()) {
            for &(name, f) in *list {
                let ix = crate::fnreg::find(crate::strings::JsStr::Latin1(name.as_bytes()));
                debug_assert!(ix.is_some(), "function body for an unknown name: {name}");
                if let Some(slot) = ix.and_then(|ix| t.get_mut(ix.0 as usize)) {
                    debug_assert!(slot.is_none(), "function body declared twice: {name}");
                    *slot = Some(f);
                }
            }
        }
        t.into_boxed_slice()
    })
}

/// The body of a registered function, once ported (a table lookup: no allocation, no search).
#[inline]
#[must_use]
pub fn body(ix: FnIx) -> Option<FnBody> {
    table().get(ix.0 as usize).copied().flatten()
}

/// The names that have a body, per family (reports, coverage tests).
pub fn ported() -> impl Iterator<Item = &'static str> {
    FAMILIES.into_iter().flat_map(|f| f.iter()).flat_map(|l| l.iter().map(|&(n, _)| n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every declared name is a registered function and appears once (the release build skips a bad entry).
    #[test]
    fn family_lists_are_consistent() {
        let mut seen = std::collections::HashSet::new();
        for name in ported() {
            let ix = crate::fnreg::find(crate::strings::JsStr::Latin1(name.as_bytes()));
            assert!(ix.is_some_and(|ix| ix.entry().def.is_some()), "{name} is not in the registry");
            assert!(seen.insert(name), "{name} declared twice");
            assert!(body(ix.unwrap()).is_some(), "{name} not dispatched");
        }
    }
}
