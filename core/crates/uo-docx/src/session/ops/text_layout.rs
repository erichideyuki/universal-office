//! D2b family: text layout options: `setDefaultTabStop`, `setHyphenation`/options (the settings half; manual hyphenation is the layout family), `text-layout.ts`.
//!
//! Owned by frontier D2b-F4 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`TextLayoutCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum TextLayoutCmd {}

impl Session {
    pub(crate) fn exec_text_layout(&mut self, cmd: TextLayoutCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl TextLayoutCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
