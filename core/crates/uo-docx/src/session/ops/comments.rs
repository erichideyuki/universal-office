//! D2b family: comments: `addComment`, `removeComment`.
//!
//! Owned by frontier D2b-F6 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`CommentCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum CommentCmd {}

impl Session {
    pub(crate) fn exec_comments(&mut self, cmd: CommentCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl CommentCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
