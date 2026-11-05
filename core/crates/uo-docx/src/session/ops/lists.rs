//! D2b family: numbering: `toggleList`, `applyListLibrary` (list-library.ts), `removeList`, `applyMultilevelLibrary`, `applyListLevels`, `setListLevel`, `changeListLevel`, `restartNumbering`, `continueNumbering`, `setNumberingValue`, `defineListLevel`, `defineMultilevelList`, list styles; `numbering-store.ts`; the Enter/Backspace list hooks of the text family.
//!
//! Owned by frontier D2b-F2 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`ListCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum ListCmd {}

impl Session {
    pub(crate) fn exec_lists(&mut self, cmd: ListCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// `listEnterOnEmpty()`: Enter on an empty list item ends the list (F2). Until F2 lands: not
    /// handled (record D2a-T4).
    pub(crate) fn list_enter_on_empty(&mut self) -> Result<bool, CommandError> {
        Ok(false)
    }

    /// `listBackspaceAtStart()`: Backspace at the start of a list item removes the number first
    /// (F2). Until F2 lands: not handled (record D2a-T4).
    pub(crate) fn list_backspace_at_start(&mut self) -> Result<bool, CommandError> {
        Ok(false)
    }
}

impl ListCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
