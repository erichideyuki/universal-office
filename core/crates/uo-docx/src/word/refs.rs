//! The model side of the layout's line references (`docx-layout/src/refs.ts`): the cell of a row
//! at a grid column (`cellAtGrid`) and the paragraph at a reference path (`paragraphAtPath`,
//! `b<i>[/r<r>/c<c>/b<k>…]`), used by the editor and the viewer to go from a laid-out line back to
//! the document. `cellBlocks(cell)` is the cell story's block order ([`Story::blocks`]). The
//! layout-side queries (line lookup, hit tests) are frontier F8's (`query.rs`).

use crate::js::JsNumber;
use crate::model::para::Paragraph;
use crate::model::story::{Block, Story};
use crate::model::table::{Cell, Table};

/// `list[i]` of a JS array with a numeric index (`undefined` unless an integral index in range).
fn index(i: f64) -> Option<usize> {
    if i >= 0.0 && uo_jsnum::conv::is_integer(i) && i < 4_294_967_295.0 { usize::try_from(uo_jsnum::conv::to_uint32(i)).ok() } else { None }
}

/// `x ?? d` over a model number (`null` and absent give `d`).
fn or(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) => v,
        _ => d,
    }
}

/// `cellAtGrid(t, r, c)` (refs.ts:8): the cell of row `r` that starts at grid column `c`
/// (`gridBefore` plus the spans).
#[must_use]
pub fn cell_at_grid(t: &Table, r: f64, c: f64) -> Option<&Cell> {
    let row = t.rows.get(index(r)?)?;
    let mut col = or(row.tr_pr.as_deref().and_then(|p| p.grid_before), 0.0);
    for cell in &row.cells {
        if col == c {
            return Some(cell);
        }
        let span = match cell.tc_pr.as_deref().and_then(|p| p.grid_span) {
            Some(JsNumber::Num(v)) => v,
            _ => match cell.grid_span {
                JsNumber::Num(v) => v,
                JsNumber::Null => 1.0,
            },
        };
        col += uo_jsnum::math::max(1.0, span);
    }
    None
}

/// `/^<p>(\d+)$/` of a path segment: the number (`+m[1]`).
fn seg(s: &str, p: char) -> Option<f64> {
    let d = s.strip_prefix(p)?;
    (!d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).then(|| uo_jsnum::parse::to_number(d))
}

/// `paragraphAtPath(blocks, path)` (refs.ts:22): the paragraph at the reference path in a story's
/// blocks (`None` = `null`).
#[must_use]
pub fn paragraph_at_path<'a>(story: &'a Story, path: &[&str]) -> Option<&'a Paragraph> {
    let mut list = story;
    let mut i = 0;
    while i < path.len() {
        let b = path.get(i).and_then(|s| seg(s, 'b'))?;
        let blk = list.block_at(index(b)?)?;
        if i + 1 >= path.len() {
            return match blk {
                Block::Paragraph(p) => Some(p),
                Block::Table(_) => None,
            };
        }
        let r = path.get(i + 1).and_then(|s| seg(s, 'r'));
        let c = path.get(i + 2).and_then(|s| seg(s, 'c'));
        let (Block::Table(t), Some(r), Some(c)) = (blk, r, c) else { return None };
        list = &cell_at_grid(t, r, c)?.content;
        i += 3;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ParaId, TableId};
    use crate::model::props::{TcPr, TrPr};
    use crate::model::table::Row;
    use std::sync::Arc;

    #[test]
    fn grid_and_paths() {
        let mut inner = Story::new();
        inner.push_paragraph(Paragraph::empty(ParaId(7)));
        let wide = Cell {
            grid_span: JsNumber::Num(1.0),
            tc_pr: Some(Arc::new(TcPr { grid_span: Some(JsNumber::Num(2.0)), ..TcPr::default() })),
            ..Cell::default()
        };
        let target = Cell { grid_span: JsNumber::Num(1.0), content: inner, ..Cell::default() };
        let row = Row {
            cells: vec![wide, target],
            tr_pr: Some(Arc::new(TrPr { grid_before: Some(JsNumber::Num(1.0)), ..TrPr::default() })),
            ..Row::default()
        };
        let table = Arc::new(Table { id: TableId(1), rows: vec![row], ..Table::default() });
        let mut body = Story::new();
        body.push_paragraph(Paragraph::empty(ParaId(1)));
        body.push_table(table);
        assert!(
            cell_at_grid(
                match body.block_at(1) {
                    Some(Block::Table(t)) => t,
                    _ => panic!(),
                },
                0.0,
                3.0
            )
            .is_some()
        );
        assert_eq!(paragraph_at_path(&body, &["b1", "r0", "c3", "b0"]).map(|p| p.id), Some(ParaId(7)));
        assert_eq!(paragraph_at_path(&body, &["b0"]).map(|p| p.id), Some(ParaId(1)));
        assert!(paragraph_at_path(&body, &["b1", "r0", "c2", "b0"]).is_none());
        assert!(paragraph_at_path(&body, &["b1"]).is_none());
        assert!(paragraph_at_path(&body, &["b99999999999999999999"]).is_none());
    }
}
