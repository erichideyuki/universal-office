//! C# `DocxTableRowTemplate`: a table row holding a marker leaves the table and is the mold of
//! rows appended at the end, with their `#{Name}` parameters replaced. A view: the detached
//! template row and the table, nodes of the document's arena.

use super::document::DocxDocument;
use super::dom::{NodeId, n};
use super::error::Result;
use super::paragraph_text as pt;
use super::text::{self, TextScope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowTemplate {
    row: NodeId,
    table: NodeId,
    appended_rows: usize,
}

impl RowTemplate {
    /// Takes the row of the first cell paragraph holding `marker` (case-sensitive, whole word;
    /// body first, then headers/footers) out of its table; `None` when the marker is not in a cell.
    pub fn take(doc: &mut DocxDocument, marker: &str) -> Result<Option<RowTemplate>> {
        let paragraph = text::find_paragraphs(doc, marker, true, true, TextScope::Document)?.first().copied();
        let dom = doc.dom_mut();
        let Some(cell) = paragraph.and_then(|p| dom.ancestor(p, n::tc)) else { return Ok(None) };
        let Some(row) = dom.ancestor(cell, n::tr) else { return Ok(None) };
        let Some(table) = dom.ancestor(row, n::tbl) else { return Ok(None) };
        let template = dom.deep_clone(row);
        dom.remove(row)?;
        doc.mark_changed();
        Ok(Some(RowTemplate { row: template, table, appended_rows: 0 }))
    }

    /// Rows appended so far.
    pub fn appended_rows(&self) -> usize {
        self.appended_rows
    }

    /// Appends a copy of the row at the end of the table, replacing in each paragraph the
    /// parameters `#{key}` (case-insensitive, whole word, in order; `None` = empty) and, with
    /// `remove_remaining_parameters`, erasing the `#{…}` left.
    pub fn append(&mut self, doc: &mut DocxDocument, parameters: &[(&str, Option<&str>)], remove_remaining_parameters: bool) -> Result<()> {
        doc.dom().check(self.row)?;
        doc.dom().check(self.table)?;
        let dom = doc.dom_mut();
        let row = dom.deep_clone(self.row);
        dom.append(self.table, row);
        for p in pt::paragraphs(dom, row) {
            for (name, value) in parameters {
                text::replace_in_paragraph(dom, p, &text::parameter(name), value.unwrap_or(""), false, true)?;
            }
            if remove_remaining_parameters {
                text::replace_any_parameter(dom, p, "")?;
            }
        }
        self.appended_rows += 1;
        Ok(())
    }
}
