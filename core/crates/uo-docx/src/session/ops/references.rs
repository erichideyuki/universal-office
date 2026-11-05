//! D2b family: the layout family (U1, after the uo-docx-layout gate, through the `LayoutOracle`): `insertField`, `updateFields`, TOC (`insertTableOfContents`, `updateToc`, levels, entries; `toc.ts`), table of figures, captions, cross-references (`references.ts`), manual hyphenation, galleries.
//!
//! Owned by frontier D2b-F10 (after uo-docx-layout) (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`ReferenceCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceCmd {}

impl Session {
    pub(crate) fn exec_references(&mut self, cmd: ReferenceCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl ReferenceCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
