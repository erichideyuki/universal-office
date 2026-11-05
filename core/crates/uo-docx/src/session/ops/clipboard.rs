//! D2b family: clipboard: `copy`/`cut` (fragment out), `paste` (two-phase: the shell reads the clipboard and sends the neutral paste tree, G15), `pastePlain`, `pasteText`, `pasteSpecial*`, the Office clipboard ring (G7); the paste rules of `paste-html.ts` (Opus boundary: the paste tree).
//!
//! Owned by frontier D2b-F8 (Opus for the tree boundary) (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`ClipboardCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardCmd {}

impl Session {
    pub(crate) fn exec_clipboard(&mut self, cmd: ClipboardCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// The second phase of a two-phase command of this family (G25): the shell's results are in.
    pub(crate) fn complete_clipboard(
        &mut self,
        cmd: ClipboardCmd,
        _job: JobId,
        _result: JobResult,
        _cx: &mut ExecCx<'_>,
    ) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl ClipboardCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
