//! D2b family: page design: `setPageColor`, `setPageFill`, watermarks, cover pages (cover-templates.ts); `page-design.ts`, docx-core `page-background.ts`, `watermark.ts`.
//!
//! Owned by frontier D2b-F4 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`PageCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum PageCmd {}

impl Session {
    pub(crate) fn exec_page(&mut self, cmd: PageCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl PageCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
