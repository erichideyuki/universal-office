//! D2b family: bookmarks and links: `insertBookmark`, `deleteBookmark`, `insertHyperlink`, `setLink`, `removeLink`.
//!
//! Owned by frontier D2b-F6 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`BookmarkCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum BookmarkCmd {}

impl Session {
    pub(crate) fn exec_bookmarks(&mut self, cmd: BookmarkCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl BookmarkCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
