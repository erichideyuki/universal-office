//! Wave 7 (A2) of the editor switch: the table gestures of the TS shell that edit a table which is NOT the
//! caret's (the Draw Table pencil, the Eraser, the Border Painter, the edge drag, the move and size handles)
//! and the Quick Tables, as session operations. Each is a 1:1 port of the TS code named in its doc comment
//! (`universal-write.ts` of the frozen TS root); the shell gives what the TS read from its layout (row
//! heights, the cell under the pointer, the pointer's travel). Reached through
//! [`super::super::shell_edits::ShellEdit`].

use std::sync::Arc;

use uo_jsnum::conv::truthy;
use uo_jsnum::math::{abs, max, round};

use super::super::super::command::{CommandError, Outcome};
use super::super::super::state::CellHop;
use super::super::super::{ExecCx, Session};
use super::table_ops::{self, AutoFit, BorderKind, CellRect, SplitOptions};
use super::{TableLayoutIn, table_at};
use crate::js::{JsNumber, Str};
use crate::model::para::{ParaProps, Paragraph};
use crate::model::props::{BorderSpec, TblLook, TblPr, TblpPr, WidthSpec};
use crate::model::run::{Run, RunProps};
use crate::model::story::OwnedBlock;
use crate::text::Utf16;
use crate::word::obj;

/// The side of a cell the Border Painter or the Eraser hit (`tableToolClick`: the nearest side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSide {
    Top,
    Bottom,
    Left,
    Right,
}

impl CellSide {
    fn kind(self) -> BorderKind {
        match self {
            CellSide::Top => BorderKind::Top,
            CellSide::Bottom => BorderKind::Bottom,
            CellSide::Left => BorderKind::Left,
            CellSide::Right => BorderKind::Right,
        }
    }
}

/// A Quick Table as the shell built it (`insertQuickTable`, universal-write.ts 20394: the rows in the language of
/// the interface, the style by English name, the merges, the calendar's centring).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilledTable {
    pub rows: Vec<Vec<String>>,
    pub style: String,
    /// `TOps.setLook(t, { firstColumn: false })` (double, calendar)
    pub first_column_off: bool,
    /// `{ r, c1, c2 }` merged in order
    pub merges: Vec<(i64, i64, i64)>,
    /// the calendar: every paragraph of the rows after the first centred
    pub center_body: bool,
}

/// The table edge or handle gestures and the tools (`finishTableDrag`, `tableToolClick`, `finishDraw`).
#[derive(Debug, Clone, PartialEq)]
pub enum TableGesture {
    /// `resizeTableEdge(kind, anchor, path, boundary, delta)` (universal-write.ts 20800); `row_height` = the
    /// displayed height of row `boundary` (`rowHeights(anchor, path, boundary, boundary)[0]`)
    Edge { col: bool, anchor: u32, path: Vec<CellHop>, boundary: i64, delta: f64, row_height: f64 },
    /// `finishTableDrag` 'move': a click selects the table, a drag floats it at the drop point
    Move { anchor: u32, dx: f64, dy: f64 },
    /// `finishTableDrag` 'size': the columns scale with `dx`; the rows with `dy` when `row_heights` (the
    /// table on one page: every row's displayed height)
    Size { anchor: u32, dx: f64, dy: f64, row_heights: Option<Vec<f64>> },
    /// `tableToolClick` with the Border Painter: `pen` the shell's `borderPen`
    Paint { anchor: u32, path: Vec<CellHop>, r: i64, c: i64, side: CellSide, pen: BorderSpec },
    /// `tableToolClick` with the Eraser: an inside edge merges the two cells, an outside one loses its border
    Erase { anchor: u32, path: Vec<CellHop>, r: i64, c: i64, side: CellSide },
    /// `finishDraw` inside a cell: a vertical stroke splits it in two columns at `left` (twips from the cell's
    /// left, already clamped by the shell as the TS), a horizontal one in two rows
    DrawSplit { anchor: u32, path: Vec<CellHop>, r: i64, c: i64, vertical: bool, left: f64, box_w: f64, row_heights: Vec<f64> },
    /// `finishDraw` outside a table: a fixed 1 × 1 table `w` wide; a second step for the height when `h` > 300
    DrawNew { w: f64, h: f64 },
}

fn text_para(ids: &mut crate::ids::IdAlloc, text: &str) -> Paragraph {
    let mut p = Paragraph::empty(ids.para());
    p.props = Arc::new(ParaProps::default());
    p.runs.push(Run { text: Utf16::from(text), props: Arc::new(RunProps::default()), ..Run::default() });
    p
}

fn dxa_ind(t: &crate::model::table::Table) -> f64 {
    match t.tbl_pr.as_deref().and_then(|p| p.tbl_ind.as_ref()) {
        Some(w) if &*w.r#type == "dxa" => w.w.to_number(),
        _ => 0.0,
    }
}

impl Session {
    /// `insertQuickTable(id)` after the shell built the content: `insertTable(rows, cols)`, then the style, the
    /// look, the text, the merges and the centring in the SAME undo step, the caret in cell (0, 0).
    pub(crate) fn shell_filled_table(&mut self, q: &FilledTable, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        if q.rows.is_empty()
            || self.paras().is_none()
            || self.locked()
            || !matches!(self.ctx, super::EditContext::Body | super::EditContext::Cell { .. })
        {
            return super::done(self);
        }
        let depth = self.history.undo_len();
        let cols = q.rows.first().map_or(1, Vec::len);
        let rows = q.rows.len();
        let to_f = |n: usize| f64::from(u32::try_from(n).unwrap_or(u32::MAX));
        let first = self.insert_table(cx, to_f(rows), to_f(cols), None, None, &TableLayoutIn::default())?;
        let Outcome::Done(cs0) = first else { return Ok(first) };
        let Some(tg) = self.table_target() else { return Ok(Outcome::Done(cs0)) };
        if self.history.undo_len() == depth {
            // nothing inserted
            return Ok(Outcome::Done(cs0));
        }
        self.begin_step(false, &[]);
        let sid = self.table_style_id(&q.style);
        let normal = self.normal_spacing();
        let r = self.with_table(tg.anchor, &tg.path, |t, ids| {
            if let Some(id) = &sid {
                let mut patch = TblPr::default();
                obj::set(&mut patch, "tblStyle", |p| p.tbl_style = Some(id.as_str().into()));
                table_ops::set_table_props(t, &patch, ids);
            }
            if q.first_column_off {
                let mut look = TblLook::default();
                obj::set(&mut look, "firstColumn", |p| p.first_column = Some(false));
                table_ops::set_look(t, &look, ids);
            }
            for (r, row) in q.rows.iter().enumerate() {
                for (c, text) in row.iter().enumerate() {
                    if text.is_empty() {
                        continue;
                    }
                    let p = text_para(ids, text);
                    if let Some(cell) = t.rows.get_mut(r).and_then(|row| row.cells.get_mut(c)) {
                        table_ops::set_cell_content(cell, vec![OwnedBlock::Paragraph(p)], ids);
                    }
                }
            }
            for &(r, c1, c2) in &q.merges {
                table_ops::merge_cells(t, CellRect { r1: r, c1, r2: r, c2 }, Some(normal), ids)?;
            }
            if q.center_body {
                for row in t.rows.iter_mut().skip(1) {
                    for cell in &mut row.cells {
                        for i in 0..cell.content.paras.len() {
                            if let Some(p) = cell.content.paras.make_mut(i) {
                                Arc::make_mut(&mut p.props).alignment = Some(Str::from("center"));
                            }
                        }
                    }
                }
            }
            table_ops::sync_legacy(t);
            Ok(())
        });
        if r.is_err() {
            self.discard_step();
            return Ok(Outcome::Done(self.finish()));
        }
        self.exit_cell();
        self.enter_cell_at(tg.anchor, &tg.path, 0, 0, false);
        self.touch_structure();
        let mut cs = self.finish();
        self.history.collapse_to(depth + 1);
        // the TS fills the table in the insertion's step: one `uw-change` (the insertion's), the cell mode events of the
        // exit and the re-entry after it
        cs.events.retain(|e| !matches!(e, super::super::super::command::Event::Change));
        let mut cs = merge_change_sets(cs0, cs);
        cs.can_undo = self.history.undo_len() > 0;
        Ok(Outcome::Done(cs))
    }

    /// The table gestures (see [`TableGesture`]).
    pub(crate) fn shell_table_gesture(&mut self, g: TableGesture, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match g {
            TableGesture::Edge { col, anchor, path, boundary, delta, row_height } => {
                self.shell_resize_edge(col, anchor, &path, boundary, round(delta), row_height)
            }
            TableGesture::Move { anchor, dx, dy } => {
                let (dx, dy) = (round(dx), round(dy));
                if abs(dx) < 30.0 && abs(dy) < 30.0 {
                    // a click on the handle: the table is selected
                    if !self.enter_cell_at(anchor, &[], 0, 0, false) {
                        return super::done(self);
                    }
                    return self.exec_tables(super::TableCmd::SelectTable, cx);
                }
                self.edit_table_at(anchor, &[], move |t, ids| {
                    let pp = t.tbl_pr.as_deref().and_then(|p| p.tblp_pr.as_deref()).cloned();
                    let ind = dxa_ind(t);
                    let spec = |s: Option<&Str>| s.is_some_and(|v| !v.is_empty());
                    let x0 = match &pp {
                        Some(p) if spec(p.tblp_x_spec.as_ref()) => 0.0,
                        Some(p) => p.tblp_x.map_or(ind, JsNumber::to_number),
                        None => ind,
                    };
                    let y0 = match &pp {
                        Some(p) if spec(p.tblp_y_spec.as_ref()) => 0.0,
                        Some(p) => p.tblp_y.map_or(0.0, JsNumber::to_number),
                        None => 0.0,
                    };
                    // `{ leftFromText: 141, rightFromText: 141, ...pp, vertAnchor, horzAnchor, tblpX, tblpY }` without
                    // the specs and the undefined keys
                    let mut tp = TblpPr::default();
                    obj::set(&mut tp, "leftFromText", |p| p.left_from_text = Some(JsNumber::Num(141.0)));
                    obj::set(&mut tp, "rightFromText", |p| p.right_from_text = Some(JsNumber::Num(141.0)));
                    if let Some(p) = &pp {
                        tp = obj::spread(&tp, p);
                    }
                    let va = pp.as_ref().and_then(|p| p.vert_anchor.clone()).unwrap_or_else(|| Str::from("text"));
                    obj::set(&mut tp, "vertAnchor", |p| p.vert_anchor = Some(va));
                    let ha = pp.as_ref().and_then(|p| p.horz_anchor.clone());
                    match ha {
                        Some(h) => obj::set(&mut tp, "horzAnchor", |p| p.horz_anchor = Some(h)),
                        None => obj::delete(&mut tp, "horzAnchor"),
                    }
                    obj::set(&mut tp, "tblpX", |p| p.tblp_x = Some(JsNumber::Num(x0 + dx)));
                    obj::set(&mut tp, "tblpY", |p| p.tblp_y = Some(JsNumber::Num(y0 + dy)));
                    obj::delete(&mut tp, "tblpXSpec");
                    obj::delete(&mut tp, "tblpYSpec");
                    let overlap = t.tbl_pr.as_deref().and_then(|p| p.tbl_overlap.clone()).unwrap_or_else(|| Str::from("never"));
                    let mut patch = TblPr::default();
                    obj::set(&mut patch, "tblpPr", |p| p.tblp_pr = Some(Box::new(tp)));
                    obj::set(&mut patch, "tblOverlap", |p| p.tbl_overlap = Some(overlap));
                    obj::set_undefined(&mut patch, "tblInd", |p| p.tbl_ind = None);
                    table_ops::set_table_props(t, &patch, ids);
                    Ok(true)
                })
            }
            TableGesture::Size { anchor, dx, dy, row_heights } => {
                let (dx, dy) = (round(dx), round(dy));
                if abs(dx) < 2.0 && abs(dy) < 2.0 {
                    return super::done(self);
                }
                self.edit_table_at(anchor, &[], move |t, _| {
                    let g: Vec<f64> = t.grid_cols_twips.iter().map(|w| w.to_number()).collect();
                    let sum: f64 = g.iter().sum();
                    let w = if truthy(sum) { sum } else { 1.0 };
                    let fx = max(0.1, (w + dx) / w);
                    if truthy(dx) {
                        for (c, wc) in g.iter().enumerate() {
                            let c = i64::try_from(c).unwrap_or(0);
                            table_ops::set_column_width(t, c, c, max(100.0, round(wc * fx)))?;
                        }
                    }
                    if truthy(dy)
                        && let Some(hs) = &row_heights
                    {
                        let total: f64 = hs.iter().sum();
                        let hh = if truthy(total) { total } else { 1.0 };
                        let fy = max(0.1, (hh + dy) / hh);
                        for (r, h) in hs.iter().enumerate() {
                            let r = i64::try_from(r).unwrap_or(0);
                            table_ops::set_row_height(t, r, r, max(100.0, round(h * fy)), "atLeast");
                        }
                    }
                    Ok(true)
                })
            }
            TableGesture::Paint { anchor, path, r, c, side, pen } => self.edit_table_at(anchor, &path, move |t, ids| {
                let rect = CellRect { r1: r, c1: c, r2: r, c2: c };
                table_ops::set_borders(t, rect, side.kind(), Some(&pen), false, None, ids);
                Ok(true)
            }),
            TableGesture::Erase { anchor, path, r, c, side } => {
                let normal = self.normal_spacing();
                self.edit_table_at(anchor, &path, move |t, ids| {
                    let m = table_ops::map_table(t);
                    let Some(g) = table_ops::cell_at(&m, r, c) else { return Ok(false) };
                    let other = match side {
                        CellSide::Top => table_ops::cell_at(&m, r - 1, g.col),
                        CellSide::Bottom => table_ops::cell_at(&m, table_ops::v_extent(t, &m, r, g.col).1 + 1, g.col),
                        CellSide::Left => table_ops::cell_at(&m, r, g.col - 1),
                        CellSide::Right => table_ops::cell_at(&m, r, g.col + g.span),
                    };
                    let rect = CellRect { r1: r, c1: c, r2: r, c2: c };
                    let Some(o) = other else {
                        table_ops::set_borders(t, rect, side.kind(), None, false, None, ids);
                        return Ok(true);
                    };
                    let ori = i64::try_from(o.ri).unwrap_or(0);
                    let r2 = table_ops::norm_rect(
                        t,
                        &m,
                        CellRect { r1: r.min(ori), c1: g.col.min(o.col), r2: r.max(ori), c2: (g.col + g.span - 1).max(o.col + o.span - 1) },
                    );
                    if !table_ops::can_merge(t, r2) {
                        return Ok(false);
                    }
                    table_ops::merge_cells(t, r2, Some(normal), ids)?;
                    Ok(true)
                })
            }
            TableGesture::DrawSplit { anchor, path, r, c, vertical, left, box_w, row_heights } => {
                let normal = self.normal_spacing();
                self.edit_table_at(anchor, &path, move |t, ids| {
                    let rect = CellRect { r1: r, c1: c, r2: r, c2: c };
                    let o = SplitOptions { merge_first: Some(false), row_heights, normal_spacing: Some(normal) };
                    let out = table_ops::split_cells(t, rect, if vertical { 2.0 } else { 1.0 }, if vertical { 1.0 } else { 2.0 }, &o, ids)?;
                    if vertical {
                        table_ops::set_cell_width(t, CellRect { r1: out.r1, c1: out.c1, r2: out.r1, c2: out.c1 }, left)?;
                        table_ops::set_cell_width(
                            t,
                            CellRect { r1: out.r1, c1: out.c1 + 1, r2: out.r1, c2: out.c1 + 1 },
                            round(box_w - left),
                        )?;
                    }
                    Ok(true)
                })
            }
            TableGesture::DrawNew { w, h } => {
                let first = self.insert_table(cx, 1.0, 1.0, Some(AutoFit::Fixed), Some(max(200.0, round(w))), &TableLayoutIn::default())?;
                if h <= 300.0 {
                    return Ok(first);
                }
                let Outcome::Done(cs0) = first else { return Ok(first) };
                let Some(tg) = self.table_target() else { return Ok(Outcome::Done(cs0)) };
                let hh = round(h);
                let second = self.edit_table_at(tg.anchor, &tg.path, move |t, _| {
                    table_ops::set_row_height(t, 0, 0, hh, "atLeast");
                    Ok(true)
                })?;
                Ok(match second {
                    Outcome::Done(cs) => Outcome::Done(merge_change_sets(cs0, cs)),
                    o => o,
                })
            }
        }
    }

    /// `resizeTableEdge` (universal-write.ts 20800) on a given table (`editTableAt`; the caret goes back).
    fn shell_resize_edge(
        &mut self,
        col: bool,
        anchor: u32,
        path: &[CellHop],
        boundary: i64,
        delta: f64,
        row_height: f64,
    ) -> Result<Outcome, CommandError> {
        if abs(delta) < 2.0 || !delta.is_finite() {
            return super::done(self);
        }
        if col {
            return self.edit_table_at(anchor, path, move |t, ids| {
                let g: Vec<f64> = t.grid_cols_twips.iter().map(|w| w.to_number()).collect();
                let n = i64::try_from(g.len()).unwrap_or(0);
                let at = |i: i64| usize::try_from(i).ok().and_then(|i| g.get(i)).copied().unwrap_or(0.0);
                let b = boundary;
                if b > 0 && b < n {
                    let (l, r) = (at(b - 1) + delta, at(b) - delta);
                    table_ops::set_column_width(t, b - 1, b - 1, l)?;
                    table_ops::set_column_width(t, b, b, r)?;
                } else if b == n {
                    table_ops::set_column_width(t, b - 1, b - 1, at(b - 1) + delta)?;
                } else {
                    let ind = dxa_ind(t);
                    let mut patch = TblPr::default();
                    if truthy(ind + delta) {
                        obj::set(&mut patch, "tblInd", |p| {
                            p.tbl_ind = Some(WidthSpec { w: JsNumber::Num(ind + delta), r#type: "dxa".into(), ..WidthSpec::default() })
                        });
                    } else {
                        obj::set_undefined(&mut patch, "tblInd", |p| p.tbl_ind = None);
                    }
                    table_ops::set_table_props(t, &patch, ids);
                    table_ops::set_column_width(t, 0, 0, at(0) - delta)?;
                }
                Ok(true)
            });
        }
        self.edit_table_at(anchor, path, move |t, _| {
            table_ops::set_row_height(t, boundary, boundary, max(100.0, round(row_height + delta)), "atLeast");
            Ok(true)
        })
    }

    /// The table at `(anchor, path)` exists (the shell's hit is still valid).
    #[must_use]
    pub fn shell_table_exists(&self, anchor: u32, path: &[CellHop]) -> bool {
        table_at(&self.doc, anchor, path).is_some()
    }

    /// The grid column widths of the table at `(anchor, path)` (the TS `startTableEdgeDrag` reads them for the
    /// drag's limits).
    #[must_use]
    pub fn shell_table_grid(&self, anchor: u32, path: &[CellHop]) -> Option<Vec<f64>> {
        table_at(&self.doc, anchor, path).map(|t| t.grid_cols_twips.iter().map(|w| w.to_number()).collect())
    }
}

/// Two change sets of one gesture as one (the stories united, the events in order, the rest of the last).
#[must_use]
pub fn merge_change_sets(
    mut a: super::super::super::command::ChangeSet,
    b: super::super::super::command::ChangeSet,
) -> super::super::super::command::ChangeSet {
    for st in b.stories {
        if !a.stories.contains(&st) {
            a.stories.push(st);
        }
    }
    a.events.extend(b.events);
    a.selection = b.selection;
    a.dirty = b.dirty;
    a.can_undo = b.can_undo;
    a.can_redo = b.can_redo;
    a.formatting |= b.formatting;
    a
}
