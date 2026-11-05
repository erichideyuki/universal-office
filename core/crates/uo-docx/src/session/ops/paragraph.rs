//! D2b family: paragraph formatting: `setAlignment`, `indent`, `setLineSpacing`, `setSpaceAfter/Before`, `setShading`, `setSpecialIndent`, `setIndentRight`, `setParagraphIndents`, `setTabStops`, `setParagraphFlow`, `setParagraphLayout`, `setParagraphBorders`, `insertHorizontalLine`, `toggleBorder`, `clearParagraphFormatting`.
//!
//! Owned by frontier D2b-F1 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`ParagraphCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum ParagraphCmd {}

impl Session {
    pub(crate) fn exec_paragraph(&mut self, cmd: ParagraphCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }
}

impl ParagraphCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
