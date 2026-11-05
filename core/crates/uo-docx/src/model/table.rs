//! Tables (docx-core `fulldoc.ts`: `TableBlock`, `TableRow`, `TableCell`), read by frontier R5.
//! Every table has a [`TableId`] and every cell is addressed by (table, row index, cell index)
//! (uo-docx-layout K2b, `CellKey`). A cell's content is one [`Story`]; the TS `paragraphs`,
//! `tables` and `blocks` are views of it ([`CellView`], U3).

use std::sync::Arc;

use crate::ids::{SourceKey, TableId};
use crate::js::{JsNumber, JsValue, JsonWriter, Str};
use crate::js_object;

use super::para::{BorderEdge, Paragraph};
use super::props::{TblPr, TcPr, TrPr};
use super::story::Story;

js_object! {
    /// `w:tblBorders` / `w:tcBorders` in editing form (`TableBorders`).
    pub struct TableBorders {
        opt top: BorderEdge = "top",
        opt left: BorderEdge = "left",
        opt bottom: BorderEdge = "bottom",
        opt right: BorderEdge = "right",
        opt inside_h: BorderEdge = "insideH",
        opt inside_v: BorderEdge = "insideV",
    }
}

/// The constant `type: 'table'` of a `TableBlock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableTag;

impl JsValue for TableTag {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.str("table");
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str("table");
        true
    }
}

/// How a cell's TS views relate (U3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellView {
    /// `paragraphs`, `tables` and `blocks` are the same objects (what the reader builds)
    #[default]
    Synced,
}

js_object! {
    custom_value:
    /// A table cell (`TableCell`). The reader writes `widthTwips`, `shadingHex` and `borders`
    /// even when `undefined` (cleared bits).
    pub struct Cell {
        req grid_span: JsNumber = "gridSpan",
        opt width_twips: JsNumber = "widthTwips",
        opt shading_hex: Str = "shadingHex",
        opt borders: Box<TableBorders> = "borders",
        opt tc_pr: Arc<TcPr> = "tcPr",
        opt src: SourceKey = "src",
    }
    hidden {
        /// the content, stored once (always at least one paragraph after the read)
        pub content: Story,
        pub view: CellView
    }
}

impl JsValue for Cell {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let content = &self.content;
        crate::js::object_canon_with(self, &[("blocks", &|w| content.canon(w)), ("@views", &|w| w.str("synced"))], w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        if w.is_signature() {
            // D1b signature text: the content once (its block view)
            let blocks = super::story::BlocksJson(&self.content);
            crate::js::object_json_with(self, &[], &[("blocks", &blocks)], w);
            return true;
        }
        let paragraphs: Vec<&Paragraph> = self.content.paras.iter().collect();
        let tables: Vec<&Table> = self.content.tables.iter().map(|a| &*a.table).collect();
        let blocks = super::story::BlocksJson(&self.content);
        crate::js::object_json_with(self, &[("paragraphs", &paragraphs), ("tables", &tables)], &[("blocks", &blocks)], w);
        true
    }
}

impl JsValue for &Table {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        (**self).canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        (**self).json(w)
    }
}

js_object! {
    /// A table row (`TableRow`).
    pub struct Row {
        req cells: Vec<Cell> = "cells",
        opt tr_pr: Arc<TrPr> = "trPr",
        opt tbl_pr_ex: Arc<TblPr> = "tblPrEx",
        opt src: SourceKey = "src",
    }
}

js_object! {
    /// A table (`TableBlock`). Identity is [`TableId`] (G22).
    pub struct Table {
        req r#type: TableTag = "type",
        req rows: Vec<Row> = "rows",
        req grid_cols_twips: Vec<JsNumber> = "gridColsTwips",
        /// written even when `undefined` by the reader (cleared bit)
        opt borders: Box<TableBorders> = "borders",
        opt tbl_pr: Arc<TblPr> = "tblPr",
        opt src: SourceKey = "src",
    }
    hidden {
        pub id: TableId
    }
}

impl Table {
    /// Cell `(row, cell)` (the layout's `CellKey` without the table).
    #[must_use]
    pub fn cell(&self, row: usize, cell: usize) -> Option<&Cell> {
        self.rows.get(row)?.cells.get(cell)
    }
}
