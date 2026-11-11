//! Cells and HTML tables (D2b-F8): `pasteCells`, `tablesFromHtml` (the grid rules after the
//! shell's DOM queries) and `pasteHtmlTables` (universal-write.ts 20484–20597).

use std::collections::HashMap;
use std::sync::Arc;

use super::super::super::Session;
use super::super::super::clipboard::ClipCells;
use super::super::super::command::{CommandError, Outcome};
use super::super::super::guard::guarded;
use super::super::super::state::Selection;
use super::super::super::text::DocPosition;
use super::super::context::EditContext;
use super::super::tables::table_ops::{self, CellRect, NewTableOptions};
use super::super::tables::{After, Op, table_at_mut};
use super::clone::{self, Fresh, MediaMap};
use super::copy::table_clip_text;
use super::tree::{HtmlTable, PasteHtml};
use super::{CellsPasteMode, PasteLayoutIn, rx};
use crate::ids::IdAlloc;
use crate::model::MediaStore;
use crate::model::para::{ParaProps, Paragraph};
use crate::model::run::Run;
use crate::model::story::OwnedBlock;
use crate::model::table::Table;
use crate::text::Utf16;
use crate::word::obj;

/// The most grid cells a pasted HTML table may occupy (F8-D12; the TS has no limit and hangs).
const MAX_GRID_MARKS: usize = 4 << 20;

/// A copy of a clip table for the document: `TOps.cloneTable` (new ids) and its pictures' media in
/// the document.
fn clone_in(t: &Table, ids: &mut IdAlloc, media: Option<&[Arc<[u8]>]>, store: &mut MediaStore) -> Table {
    let mut c = table_ops::clone_table(t, ids);
    if let Some(list) = media {
        let mut mm = MediaMap::FromClip { list, to: store, seen: HashMap::new() };
        clone::table_media(&mut c, &mut mm);
    }
    c
}

/// `structuredClone(TOps.cellContent(cell))` into the document.
fn clone_blocks(blocks: Vec<OwnedBlock>, ids: &mut IdAlloc, media: Option<&[Arc<[u8]>]>, store: &mut MediaStore) -> Vec<OwnedBlock> {
    let mut shift = MediaMap::Shift { by: 0 };
    let mut from = media.map(|list| MediaMap::FromClip { list, to: store, seen: HashMap::new() });
    let mm = match from.as_mut() {
        Some(m) => m,
        None => &mut shift,
    };
    blocks
        .into_iter()
        .map(|b| match b {
            OwnedBlock::Paragraph(mut p) => {
                p.id = ids.para();
                for r in &mut p.runs {
                    clone::run(r, &mut Fresh { ids: Some(&mut *ids), media: &mut *mm });
                }
                OwnedBlock::Paragraph(p)
            }
            OwnedBlock::Table(t) => {
                let mut c = (*t).clone();
                clone::table(&mut c, &mut Fresh { ids: Some(&mut *ids), media: &mut *mm });
                OwnedBlock::Table(Arc::new(c))
            }
        })
        .collect()
}

/// `parseInt(s ?? dflt, 10) || 1` of a span attribute.
fn span_of(s: Option<&str>) -> f64 {
    let n = uo_jsnum::parse::parse_int(s.unwrap_or("1"), 10);
    if uo_jsnum::conv::truthy(n) { n } else { 1.0 }
}

/// `tablesFromHtml(html, availTwips)` after the shell's DOM half: the occupancy grid of
/// `rowspan`/`colspan`, a new table of the grid, the spans merged (last cell first), the text of
/// each cell (TH bold).
pub fn tables_from_html(tables: &[HtmlTable], avail_twips: f64, ids: &mut IdAlloc) -> Vec<Table> {
    struct HCell {
        r: usize,
        c: usize,
        cs: usize,
        rs: usize,
        paras: Vec<Vec<u16>>,
        bold: bool,
    }
    let defaults = crate::read::plain::default_run_props();
    let mut out = Vec::new();
    'tables: for tbl in tables {
        let n_rows = tbl.rows.len();
        if n_rows == 0 {
            continue;
        }
        let mut occ: Vec<Vec<bool>> = vec![Vec::new(); n_rows];
        let mut marks = 0usize;
        let mut n_cols = 0usize;
        let mut cells: Vec<HCell> = Vec::new();
        for (r, tr) in tbl.rows.iter().enumerate() {
            let mut c = 0usize;
            for td in tr {
                while occ.get(r).and_then(|o| o.get(c)).copied().unwrap_or(false) {
                    c += 1;
                }
                let cs = uo_jsnum::math::max(1.0, uo_jsnum::math::min(63.0, span_of(td.colspan.as_deref())));
                let rs = uo_jsnum::math::max(1.0, span_of(td.rowspan.as_deref()));
                let cs = uo_jsnum::conv::to_uint32(uo_jsnum::math::floor(cs)) as usize;
                // the rows past the last one are never read (F8-D8: same result, no 1e9 loop)
                let rs = if rs >= (n_rows - r) as f64 { n_rows - r } else { uo_jsnum::conv::to_uint32(uo_jsnum::math::floor(rs)) as usize };
                marks = marks.saturating_add(rs.saturating_mul(cs));
                if marks > MAX_GRID_MARKS {
                    continue 'tables;
                }
                for i in 0..rs {
                    if let Some(o) = occ.get_mut(r + i) {
                        if o.len() < c + cs {
                            o.resize(c + cs, false);
                        }
                        for j in 0..cs {
                            if let Some(x) = o.get_mut(c + j) {
                                *x = true;
                            }
                        }
                    }
                }
                let mut paras: Vec<Vec<u16>> = td.paras.iter().map(|p| rx::squash_trim(p.units())).collect();
                if paras.is_empty() {
                    paras.push(Vec::new());
                }
                cells.push(HCell { r, c, cs, rs, paras, bold: td.th });
                c += cs;
                n_cols = n_cols.max(c);
            }
        }
        if n_cols == 0 {
            continue;
        }
        let rows_f = n_rows as f64;
        let cols_f = n_cols as f64;
        let mut t = table_ops::new_table(rows_f, cols_f, &mut NewTableOptions::new(avail_twips), ids);
        // each HTML cell: the first grid cell takes the text; the covered ones merge
        for x in cells.iter().rev() {
            if x.cs > 1 || x.rs > 1 {
                let rect = CellRect {
                    r1: x.r as i64,
                    c1: x.c as i64,
                    r2: (n_rows - 1).min(x.r + x.rs - 1) as i64,
                    c2: (n_cols - 1).min(x.c + x.cs - 1) as i64,
                };
                if table_ops::can_merge(&t, rect) {
                    let _ = table_ops::merge_cells(&mut t, rect, None, ids);
                }
            }
        }
        let m = table_ops::map_table(&t);
        for x in &cells {
            let Some(g) = table_ops::cell_at(&m, x.r as i64, x.c as i64) else { continue };
            let blocks: Vec<OwnedBlock> = x
                .paras
                .iter()
                .map(|s| {
                    let mut p = Paragraph::empty(ids.para());
                    p.props = Arc::new(ParaProps::default());
                    if !s.is_empty() {
                        let mut props = defaults.clone();
                        if x.bold {
                            obj::set(&mut props, "bold", |q| q.bold = Some(true));
                        }
                        p.runs.push(Run { text: Utf16::from_units(s.clone()), props: Arc::new(props), ..Run::default() });
                    }
                    OwnedBlock::Paragraph(p)
                })
                .collect();
            if let Some(cell) = t.rows.get_mut(g.ri).and_then(|r| r.cells.get_mut(g.ci)) {
                table_ops::set_cell_content(cell, blocks, ids);
            }
        }
        table_ops::sync_legacy(&mut t);
        out.push(t);
    }
    out
}

impl Session {
    /// `pasteCells(mode, clip)`: `Overwrite` replaces from the caret's cell (new rows when needed),
    /// `Rows` inserts the rows above the caret's row, `Nest` nests the table in the cell, `Text`
    /// pastes the text. Outside a table the table comes in at the caret. The flag: the TS `true`.
    pub(crate) fn paste_cells(
        &mut self,
        mode: CellsPasteMode,
        clip: &ClipCells,
        media: Option<&[Arc<[u8]>]>,
        _layout: &PasteLayoutIn,
    ) -> Result<(Outcome, bool), CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok((self.done_f8()?, false));
        }
        let src = Arc::clone(&clip.table);
        let text_of = |t: &Table| table_clip_text(t).0;
        if mode == CellsPasteMode::Text {
            self.paste_text_only(&text_of(&src))?;
            return Ok((self.done_f8()?, true));
        }
        let Some(_) = self.table_target() else {
            // in the body: the copied table comes in as a new table at the caret
            if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
                self.paste_text_only(&text_of(&src))?;
                return Ok((self.done_f8()?, true));
            }
            self.begin_step(false, &[]);
            if self.has_selection() {
                let pos = self.delete_selection()?;
                self.selection = Selection::caret(pos);
            }
            let DocPosition { mut para, offset } = self.selection.focus;
            if offset > 0 {
                let ctx = self.ctx.clone();
                let mut ids = std::mem::take(&mut self.doc.ids);
                let r = match super::super::context::story_of_mut(&mut self.doc, &ctx) {
                    Some(st) => guarded::split_paragraph(&mut st.paras, &mut ids, DocPosition { para, offset }).map(|_| ()),
                    None => Ok(()),
                };
                self.doc.ids = ids;
                r?;
                self.shift_anchors(para, 1);
                self.touch_structure();
                para += 1;
            }
            let mut ids = std::mem::take(&mut self.doc.ids);
            let table = clone_in(&src, &mut ids, media, &mut self.doc.media);
            self.doc.ids = ids;
            let anchors = &mut self.doc.body.tables;
            let at = anchors.iter().position(|a| a.before_para > para).unwrap_or(anchors.len());
            anchors.insert(at, Arc::new(crate::model::story::TableAnchor { before_para: para, table: Arc::new(table) }));
            self.touch_structure();
            self.enter_cell_at(u32::try_from(at).unwrap_or(u32::MAX), &[], 0, 0, false);
            return Ok((Outcome::Done(self.finish()), true));
        };
        let media_list: Option<Vec<Arc<[u8]>>> = media.map(<[Arc<[u8]>]>::to_vec);
        let out = match mode {
            CellsPasteMode::Nest => self.edit_table(move |s, tg| {
                let mut ids = std::mem::take(&mut s.doc.ids);
                let nested = clone_in(&src, &mut ids, media_list.as_deref(), &mut s.doc.media);
                let r = (|| {
                    let t = table_at_mut(&mut s.doc, tg.anchor, &tg.path)?;
                    let g = table_ops::cell_at(&table_ops::map_table(t), tg.rect.r1, tg.rect.c1)?;
                    let cell = t.rows.get_mut(g.ri)?.cells.get_mut(g.ci)?;
                    let mut blocks = vec![OwnedBlock::Table(Arc::new(nested))];
                    blocks.extend(table_ops::cell_content(cell));
                    table_ops::set_cell_content(cell, blocks, &mut ids);
                    Some(())
                })();
                s.doc.ids = ids;
                Ok(match r {
                    Some(()) => Op::Done(After::Caret { r: tg.rect.r1, c: tg.rect.c1, end: false }),
                    None => Op::Nothing,
                })
            })?,
            CellsPasteMode::Rows => self.edit_table(move |s, tg| {
                let mut ids = std::mem::take(&mut s.doc.ids);
                let add = clone_in(&src, &mut ids, media_list.as_deref(), &mut s.doc.media).rows;
                let n = add.len();
                let r = (|| {
                    let t = table_at_mut(&mut s.doc, tg.anchor, &tg.path)?;
                    let at = usize::try_from(tg.rect.r1).ok()?.min(t.rows.len());
                    t.rows.splice(at..at, add);
                    table_ops::sync_legacy(t);
                    Some(table_ops::map_table(t).n_cols)
                })();
                s.doc.ids = ids;
                Ok(match r {
                    Some(nc) => Op::Done(After::Select(CellRect {
                        r1: tg.rect.r1,
                        c1: 0,
                        r2: tg.rect.r1 + i64::try_from(n).unwrap_or(0) - 1,
                        c2: nc - 1,
                    })),
                    None => Op::Nothing,
                })
            })?,
            _ => self.edit_table(move |s, tg| {
                // replace cells: content and cell formatting, from the corner
                let mut ids = std::mem::take(&mut s.doc.ids);
                let sm = table_ops::map_table(&src);
                let r = (|| {
                    let t = table_at_mut(&mut s.doc, tg.anchor, &tg.path)?;
                    let mut m = table_ops::map_table(t);
                    let n_t = i64::try_from(t.rows.len()).unwrap_or(0);
                    let need = tg.rect.r1 + i64::try_from(src.rows.len()).unwrap_or(0) - n_t;
                    if need > 0 {
                        let rect = CellRect { r1: n_t - 1, c1: 0, r2: n_t - 1, c2: 0 };
                        let _ = table_ops::insert_rows(t, rect, false, Some(need), &mut ids);
                        m = table_ops::map_table(t);
                    }
                    Some(m)
                })();
                let Some(m) = r else {
                    s.doc.ids = ids;
                    return Ok(Op::Nothing);
                };
                for (i, gs) in sm.rows.iter().enumerate() {
                    for sg in gs {
                        let Some(dg) = table_ops::cell_at(&m, tg.rect.r1 + i64::try_from(i).unwrap_or(0), tg.rect.c1 + sg.col) else {
                            continue;
                        };
                        let Some(scell) = src.rows.get(sg.ri).and_then(|r| r.cells.get(sg.ci)) else { continue };
                        let blocks = clone_blocks(table_ops::cell_content(scell), &mut ids, media_list.as_deref(), &mut s.doc.media);
                        let mut patch = crate::model::props::TcPr::default();
                        let sp = scell.tc_pr.as_deref();
                        match sp.and_then(|p| p.shd.clone()) {
                            Some(v) => obj::set(&mut patch, "shd", |x| x.shd = Some(v)),
                            None => obj::set_undefined(&mut patch, "shd", |x| x.shd = None),
                        }
                        match sp.and_then(|p| p.tc_borders.clone()) {
                            Some(v) => obj::set(&mut patch, "tcBorders", |x| x.tc_borders = Some(v)),
                            None => obj::set_undefined(&mut patch, "tcBorders", |x| x.tc_borders = None),
                        }
                        match sp.and_then(|p| p.v_align.clone()) {
                            Some(v) => obj::set(&mut patch, "vAlign", |x| x.v_align = Some(v)),
                            None => obj::set_undefined(&mut patch, "vAlign", |x| x.v_align = None),
                        }
                        let Some(t) = table_at_mut(&mut s.doc, tg.anchor, &tg.path) else { continue };
                        if let Some(cell) = t.rows.get_mut(dg.ri).and_then(|r| r.cells.get_mut(dg.ci)) {
                            table_ops::set_cell_content(cell, blocks, &mut ids);
                        }
                        table_ops::set_cell_props(
                            t,
                            CellRect { r1: dg.ri as i64, c1: dg.col, r2: dg.ri as i64, c2: dg.col },
                            &patch,
                            &mut ids,
                        );
                    }
                }
                s.doc.ids = ids;
                let Some(t) = table_at_mut(&mut s.doc, tg.anchor, &tg.path) else { return Ok(Op::Nothing) };
                let rows = i64::try_from(t.rows.len()).unwrap_or(0);
                let r2 = (rows - 1).min(tg.rect.r1 + i64::try_from(src.rows.len()).unwrap_or(0) - 1);
                let c2 = (m.n_cols - 1).min(tg.rect.c1 + sm.n_cols - 1);
                let mm = table_ops::map_table(t);
                Ok(Op::Done(After::Select(table_ops::norm_rect(t, &mm, CellRect { r1: tg.rect.r1, c1: tg.rect.c1, r2, c2 }))))
            })?,
        };
        Ok((out, true))
    }

    /// `pasteHtmlTables(html)`: HTML with a table: in a table it replaces cells; outside, a new
    /// table. `None` when the HTML has no table with cells.
    pub(crate) fn paste_html_tables(&mut self, h: &PasteHtml, layout: &PasteLayoutIn) -> Result<Option<Outcome>, CommandError> {
        let avail = self.paste_content_width(layout);
        let mut ids = std::mem::take(&mut self.doc.ids);
        let mut tables = tables_from_html(&h.tables, avail, &mut ids);
        self.doc.ids = ids;
        if tables.is_empty() {
            return Ok(None);
        }
        let first = tables.swap_remove(0);
        let first = match self.policy.as_ref().map(|(_, r)| r.clone()) {
            Some(rp) => self.policy_table(first, &rp, None),
            None => first,
        };
        let cells = ClipCells { table: Arc::new(first), rows: true, whole: true };
        let (out, _) = self.paste_cells(CellsPasteMode::Overwrite, &cells, None, layout)?;
        Ok(Some(out))
    }
}
