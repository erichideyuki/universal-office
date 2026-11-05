//! D2b family: headings and navigation: `moveHeading`, `promoteHeading`/`demoteHeading`, `insertHeading`, `deleteHeading`, `selectAll`, `setSelectionRanges` (extra ranges), the caret moves that need no layout; `navigation.ts`.
//!
//! Owned by frontier D2b-F9 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`NavigationCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationCmd {}

impl Session {
    pub(crate) fn exec_navigation(&mut self, cmd: NavigationCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// `extras()`: the selected ranges beyond the main one (non-contiguous selection), each
    /// normalised; valid only while the main selection is unchanged (F9). Declared by F1
    /// (`applyRun`/`applyPara`/`mapRunProps` read it); until F9 lands: none.
    pub(crate) fn extra_ranges(&self) -> Vec<super::super::text::DocRange> {
        Vec::new()
    }

    /// `setExtraSelection(ranges)`: the ranges beyond the main selection (F9; F1's
    /// `selectSimilarFormatting` sets them). Until F9 lands: not stored.
    pub(crate) fn set_extra_selection(&mut self, _ranges: Vec<super::super::text::DocRange>) {}
}

impl NavigationCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
