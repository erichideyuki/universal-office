//! D2b family: pictures: `insertImage` (two-phase), wrap/position/size/rotation/flip/crop/borders/format/adjust/transparency/recolour (`setImageAdjust` two-phase), `changeImage*` (two-phase), `compressImages` (two-phase), `resetImage`, contour points; editor `picture-ops.ts`, docx-core `picture-xml.ts`.
//!
//! Owned by frontier D2b-F7 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`PictureCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum PictureCmd {}

impl Session {
    pub(crate) fn exec_pictures(&mut self, cmd: PictureCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// The second phase of a two-phase command of this family (G25): the shell's results are in.
    pub(crate) fn complete_pictures(
        &mut self,
        cmd: PictureCmd,
        _job: JobId,
        _result: JobResult,
        _cx: &mut ExecCx<'_>,
    ) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl PictureCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
