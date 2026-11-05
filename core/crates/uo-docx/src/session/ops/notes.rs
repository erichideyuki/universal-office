//! D2b family: notes: `insertFootnote`/`insertEndnote`, `setNoteOptions`, `convertNotes`, the note-run marks; editor `notes.ts`, docx-core `notes.ts`.
//!
//! Owned by frontier D2b-F5 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`NoteCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum NoteCmd {}

impl Session {
    pub(crate) fn exec_notes(&mut self, cmd: NoteCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl NoteCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
