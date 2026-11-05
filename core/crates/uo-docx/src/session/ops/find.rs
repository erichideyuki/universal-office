//! D2b family: find/replace: `findNext`, `replaceAll`, `search`, `replaceMatch`, `replaceAllMatches`, `selectAllMatches`, `goTo`; `find-engine.ts` (Opus matcher, uo-text tables).
//!
//! Owned by frontier D2b-F9 (Opus matcher) (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`FindCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum FindCmd {}

impl Session {
    pub(crate) fn exec_find(&mut self, cmd: FindCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl FindCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
