//! D2b family: sections and page setup: `setPageSetup`, `setPageBorders`, `insertSectionBreak`, `insertColumnBreak`, `setColumns`, `insertPageBreak`, `insertBlankPage`, `setSectionStart`, `setVerticalAlignment`, `setLineNumbering*`, `toggleLineNumberSuppression`; docx-core `section-ops.ts`; the rest of `effectiveSection`.
//!
//! Owned by frontier D2b-F4 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`SectionCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum SectionCmd {}

impl Session {
    pub(crate) fn exec_sections(&mut self, cmd: SectionCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl SectionCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
