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

impl Session {
    /// `refBookmark(paras, para, from, to, prefix)` (hook declared by D2b-F6 for `insertHyperlink`
    /// to a heading; F10 owns it with `newHiddenName`/`refSeed`): the hidden `_Ref`/`_Toc`
    /// bookmark that covers exactly `[from, to]` of the paragraph, reused or new. "Not handled"
    /// until F10 lands.
    pub(crate) fn ref_bookmark(&mut self, _para: u32, _from: u32, _to: u32, _prefix: &str) -> Result<crate::js::Str, CommandError> {
        Err(CommandError::NotPorted("references (D2b-F10: refBookmark)"))
    }
}

impl ReferenceCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
