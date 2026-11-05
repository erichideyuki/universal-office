//! D2b family: tables: `insertTable`, rows/columns/cells insert and delete, `mergeCells`, `splitCell`, `splitTable`, selection of cells/rows/columns, sizes, `autoFitTable`, alignment, text direction, header rows, margins, `sort` (table-sort.ts), `convertTableToText`/`convertTextToTable`, styles (table-style-edit.ts), shading, borders, quick tables, formulas (table-formula.ts), `pasteCells`, `setTableProperties`; docx-core `table-ops.ts` (incl. `TOps.relink`).
//!
//! Owned by frontier D2b-F3 (record "D2a — session (architecture)", "D2b frontier briefs"): the
//! frontier adds the variants of [`TableCmd`] and their arms below, in this file only.

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};

/// The commands of this family (empty until its frontier ports them).
#[derive(Debug, Clone, PartialEq)]
pub enum TableCmd {}

impl Session {
    pub(crate) fn exec_tables(&mut self, cmd: TableCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {}
    }

    /// `clearSelectedCells(false)` (typing over selected cells): the tables family's (F3). No D2a
    /// command sets a table selection, so this is reached only once F3 lands.
    pub(crate) fn clear_selected_cells(&mut self) -> Result<(), CommandError> {
        Err(CommandError::NotPorted("tables"))
    }
}

impl TableCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match *self {}
    }
}
