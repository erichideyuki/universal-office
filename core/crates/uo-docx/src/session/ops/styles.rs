//! D2b family: styles: `applyStyle`, `createStyle`, `modifyStyle`, `setDefaultFont`, `updateStyleFromSelection`, `renameStyle`, `deleteStyle`, `setStyleInGallery`, `clearParagraphStyle`, `clearCharacterStyle`; `styles.ts` (the store), docx-core `style-sheet.ts`, `word-styles.ts`; the Enter `applyNextStyle` hook.
//!
//! Owned by frontier D2b-F2 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`StyleCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum StyleCmd {}

impl Session {
    pub(crate) fn exec_styles(&mut self, cmd: StyleCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// `applyNextStyle(prev, next)`: Enter at the end of a paragraph whose style has `w:next`
    /// (F2). Until F2 lands: nothing (record D2a-T4).
    pub(crate) fn apply_next_style(&mut self, _at: usize) -> Result<(), CommandError> {
        Ok(())
    }
}

impl StyleCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
