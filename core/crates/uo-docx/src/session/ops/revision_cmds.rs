//! D2b family: review: `toggleTrackChanges` is Settings; `acceptChange`/`rejectChange`, `acceptAllChanges`/`rejectAllChanges` (deep steps), `acceptRevisionAt`/`rejectRevisionAt`, markup options; over D2a's `session::revisions`.
//!
//! Owned by frontier D2b-F6 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`RevisionCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum RevisionCmd {}

impl Session {
    pub(crate) fn exec_revision_cmds(&mut self, cmd: RevisionCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl RevisionCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
