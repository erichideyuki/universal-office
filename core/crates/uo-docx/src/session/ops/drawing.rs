//! D2b family: shapes and textboxes: every `setShape*`, `changeShape`, `setTextboxOptions`, `insertTextbox`, `drawTextbox`, textbox links, gallery, `deleteSelectedShape`, `alignObjects`, selection pane; editor `shape-ops.ts`, docx-core `shape-xml.ts` (the D1b W2 emitters).
//!
//! Owned by frontier D2b-F7 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`DrawingCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum DrawingCmd {}

impl Session {
    pub(crate) fn exec_drawing(&mut self, cmd: DrawingCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// The second phase of a two-phase command of this family (G25): the shell's results are in.
    pub(crate) fn complete_drawing(
        &mut self,
        cmd: DrawingCmd,
        _job: JobId,
        _result: JobResult,
        _cx: &mut ExecCx<'_>,
    ) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl DrawingCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
