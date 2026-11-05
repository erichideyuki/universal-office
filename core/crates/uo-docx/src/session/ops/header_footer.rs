//! D2b family: headers and footers: the options (`setDifferentFirstPage`, `setDifferentOddEven`, `setHeaderFooterDistance`, `setLinkToPrevious`), `removeHeaderFooter`, page numbers, `insertHeaderFooterTemplate` (hf-templates.ts), `insertDateTime`, `insertDocumentInfo`, `insertAlignmentTab`; docx-core `hf-ops.ts`.
//!
//! Owned by frontier D2b-F4 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`HeaderFooterCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderFooterCmd {}

impl Session {
    pub(crate) fn exec_header_footer(&mut self, cmd: HeaderFooterCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl HeaderFooterCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
