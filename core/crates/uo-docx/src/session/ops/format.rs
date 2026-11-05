//! D2b family: character formatting: `toggleRunProp`, `setUnderline*`, `setTextEffects`/presets, `setOpenType`, `setTextGradient`, `setFontFormat`, `toggleVertAlign`, `setFontSize`/`growFont`, `setFontFamily`, `setColor`, `setHighlight`, `clearFormatting`, `changeCase` (text-case.ts), `setCharSpacing`, `copyFormat`/format painter, `selectSimilarFormatting`; the `pendingProps` of a caret.
//!
//! Owned by frontier D2b-F1 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`FormatCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum FormatCmd {}

impl Session {
    pub(crate) fn exec_format(&mut self, cmd: FormatCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl FormatCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
