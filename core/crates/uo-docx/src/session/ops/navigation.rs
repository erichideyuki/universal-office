//! D2b family: headings and navigation: `moveHeading`, `promoteHeading`/`demoteHeading`, `insertHeading`, `deleteHeading`, `selectAll`, `setSelectionRanges` (extra ranges), the caret moves that need no layout; `navigation.ts`.
//!
//! Owned by frontier D2b-F9 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`NavigationCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationCmd {}

impl Session {
    pub(crate) fn exec_navigation(&mut self, cmd: NavigationCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl NavigationCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
