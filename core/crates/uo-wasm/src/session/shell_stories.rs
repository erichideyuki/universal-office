//! Wave 5, frontier S (record `core/docs/modules/uo-bindings.md`, "Wave 5 — S: stories"): the exports the
//! editor shell needs to put the caret in another story or scope — table cells, headers/footers, notes and
//! the text of a text box — over the session's last layout. Each export ports the TS editor code that
//! reads the layout (`universal-write.ts` line in its doc comment); the session itself (uo-docx) keeps
//! every command (`ContextCmd`, `TableCmd`, `HeaderFooterCmd`, `NoteCmd`) and takes no layout decision
//! (D2a-C2), so the decisions the TS takes from its `eng.lines` / `page.cells` are taken here.
//!
//! Declared (ED-101..ED-103 of the record): the answers are JSON text (UTF-8, numbers and strings through
//! `uo_jsnum::json`, model objects through `uo_docx::js::JsonWriter`: the TS `JSON.stringify` copies), no
//! codec or schema change (frontier R owns them); the layout's page section indexes the model's sections
//! (the `Locator` assumes the same: no `sectionMap` step); the content width of a section is the body
//! section's print width (as the session's own `content_width`, ED-16). Conventions as `handle.rs`:
//! `UoBuf` statuses 0 ok, 1 none, 5 bad argument, 11 null handle; nothing panics on hostile arguments
//! (pages out of range and NaN coordinates answer none).

use uo_docx::model::Block;
use uo_docx::model::Document;
use uo_docx::model::table::Table;
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::tables::TableCmd;
use uo_docx::session::ops::tables::table_ops::{self, CellRect, VMerge};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::state::CellHop;
use uo_docx::session::text::{DocPosition, para_length};
use uo_docx::session::{ChangeSet, Command, Outcome};
use uo_docx::view::HfWhich;
use uo_docx::word::refs::cell_at_grid;
use uo_docx_layout::hit::{EditLine, EditLines, EditPos, LineIndex};
use uo_docx_layout::output::{HfSlot, Layout, LineStory};
use uo_docx_layout::refs::{LocKind, Locator};
use uo_jsnum::json::{push_number, push_string};

use super::codec as w;
use super::handle::{UoWriteSession, exec_result};
use super::wire::IntoWire;
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf};
use crate::write::{UoLayout, layout_geom};

// ───────────────────────── JSON text ─────────────────────────

/// A JSON text written in order (objects, arrays, keys, values), numbers and strings as `JSON.stringify`.
struct Js {
    out: String,
    first: Vec<bool>,
    after_key: bool,
}

impl Js {
    fn new() -> Js {
        Js { out: String::new(), first: Vec::new(), after_key: false }
    }
    fn sep(&mut self) {
        if self.after_key {
            self.after_key = false;
            return;
        }
        if let Some(f) = self.first.last_mut() {
            if !*f {
                self.out.push(',');
            }
            *f = false;
        }
    }
    fn open(&mut self, c: char) {
        self.sep();
        self.out.push(c);
        self.first.push(true);
    }
    fn close(&mut self, c: char) {
        self.first.pop();
        self.out.push(c);
    }
    fn key(&mut self, k: &str) {
        self.sep();
        push_string(&mut self.out, k);
        self.out.push(':');
        self.after_key = true;
    }
    fn num(&mut self, x: f64) {
        self.sep();
        push_number(&mut self.out, x);
    }
    fn int(&mut self, x: impl Into<f64>) {
        self.num(x.into());
    }
    fn boolean(&mut self, b: bool) {
        self.sep();
        self.out.push_str(if b { "true" } else { "false" });
    }
    fn string(&mut self, s: &str) {
        self.sep();
        push_string(&mut self.out, s);
    }
    fn null(&mut self) {
        self.sep();
        self.out.push_str("null");
    }
    /// `JSON.stringify(v)` of a model value (`fallback` when it is `undefined`).
    fn model(&mut self, v: &dyn uo_docx::js::JsValue, fallback: &str) {
        self.sep();
        match uo_docx::js::JsonWriter::stringify(v, None) {
            Some(s) => self.out.push_str(&s),
            None => self.out.push_str(fallback),
        }
    }
    fn done(self) -> *mut UoBuf {
        buf(OK, self.out.into_bytes())
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn f(n: usize) -> f64 {
    f64::from(u32_of(n))
}

/// `[{ r, c, b? }]`: the TS `CellHop[]` of the hops (`b` only where the session has a block).
fn write_hops(j: &mut Js, hops: &[CellHop]) {
    j.open('[');
    for h in hops {
        j.open('{');
        j.key("r");
        j.int(h.row);
        j.key("c");
        j.int(h.col);
        if let Some(b) = h.block {
            j.key("b");
            j.int(b);
        }
        j.close('}');
    }
    j.close(']');
}

fn write_rect(j: &mut Js, r: CellRect) {
    j.open('{');
    for (k, v) in [("r1", r.r1), ("c1", r.c1), ("r2", r.r2), ("c2", r.c2)] {
        j.key(k);
        j.num(table_ops::num(v));
    }
    j.close('}');
}

// ───────────────────────── addressing (the TS `tableAnchors`, `tableAt`, `locateRef`) ─────────────────────────

/// `tableAt(anchor, path)`: the table at the end of the path (`path` = the hops to the cell that CONTAINS
/// the table; empty = a body table). uo-docx's `table_at` is crate-private: the same walk over the public
/// model.
fn table_at<'a>(doc: &'a Document, anchor: u32, path: &[CellHop]) -> Option<&'a Table> {
    let mut t: &Table = &doc.body.tables.get(anchor as usize)?.table;
    for h in path {
        let cell = cell_at_grid(t, f64::from(h.row), f64::from(h.col))?;
        match cell.content.block_at(h.block? as usize)? {
            Block::Table(n) => t = n,
            Block::Paragraph(_) => return None,
        }
    }
    Some(t)
}

/// The body block index of table anchor `anchor` (`origins.findIndex(o => o.anchor === anchor)`): the
/// tables before it plus the paragraphs before it.
fn block_of_anchor(doc: &Document, anchor: u32) -> Option<u32> {
    let a = doc.body.tables.get(anchor as usize)?;
    a.before_para.checked_add(anchor)
}

/// The table anchor of body block `b` (`origins[b].anchor`), when the block is a table.
fn anchor_of_block(doc: &Document, b: u32) -> Option<u32> {
    doc.body.tables.iter().enumerate().find_map(|(i, a)| {
        let i = u32::try_from(i).ok()?;
        (a.before_para.checked_add(i) == Some(b)).then_some(i)
    })
}

/// `locateRef` of a body cell ref (`b<n>/r/c/b…`, the TS `{ kind: 'cell', anchor, hops, block }`): the
/// anchor and the hops (the `b` of each hop but the last is the block of the next level).
pub(crate) fn cell_ref(doc: &Document, segs: &[u32]) -> Option<(u32, Vec<CellHop>, u32)> {
    let (&b, rest) = segs.split_first()?;
    let anchor = anchor_of_block(doc, b)?;
    let mut hops: Vec<CellHop> = Vec::new();
    let mut block = 0u32;
    // `for (i = 1; i + 2 < parts.length; i += 3)` over the parts after the root
    let mut i = 0;
    while i + 2 < rest.len() {
        let (Some(&r), Some(&c), Some(&nb)) = (rest.get(i), rest.get(i + 1), rest.get(i + 2)) else { break };
        if let Some(last) = hops.last_mut() {
            last.block = Some(block);
        }
        hops.push(CellHop { row: r, col: c, block: None });
        block = nb;
        i += 3;
    }
    (!hops.is_empty()).then_some((anchor, hops, block))
}

/// `sameHops(a, b)` (engine-doc.ts 364): rows and columns equal, and the blocks but the last.
fn same_hops(a: &[CellHop], b: &[CellHop]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).enumerate().all(|(i, (x, y))| x.row == y.row && x.col == y.col && (i + 1 == a.len() || x.block == y.block))
}

/// The TS `tableRefOf(anchor, path)` as segments: `b<bi>` then `r, c, b ?? 0` per hop.
fn table_ref(doc: &Document, anchor: u32, path: &[CellHop]) -> Option<Vec<u32>> {
    let mut v = vec![block_of_anchor(doc, anchor)?];
    for h in path {
        v.extend_from_slice(&[h.row, h.col, h.block.unwrap_or(0)]);
    }
    Some(v)
}

/// A placed cell of a table (`tableBoxes`): page, row, grid column, the box.
struct CellBox {
    pg: u32,
    r: u32,
    c: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    span: f64,
}

/// `tableBoxes(anchor, path)` (universal-write.ts 19390): every placed cell box of the table, page by page.
fn table_boxes(doc: &Document, layout: &Layout, anchor: u32, path: &[CellHop]) -> Vec<CellBox> {
    let Some(pre) = table_ref(doc, anchor, path) else { return Vec::new() };
    let mut out = Vec::new();
    for (pg, p) in layout.pages.iter().enumerate() {
        for b in p.cells.as_deref().unwrap_or(&[]) {
            if b.at.story != LineStory::Body {
                continue;
            }
            let segs: &[u32] = &b.at.path.0;
            if segs.len() != pre.len() + 2 || !segs.starts_with(&pre) {
                continue;
            }
            let (Some(&r), Some(&c)) = (segs.get(pre.len()), segs.get(pre.len() + 1)) else { continue };
            out.push(CellBox { pg: u32_of(pg), r, c, x: b.x, y: b.y, w: b.w, h: b.h, span: b.span });
        }
    }
    out
}

/// `rowHeights(anchor, path, r1, r2)`: the displayed height of each row (the sum of its parts per page).
fn row_heights(boxes: &[CellBox], r1: i64, r2: i64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut r = r1;
    while r <= r2 {
        let mut per_page: Vec<(u32, f64)> = Vec::new();
        for b in boxes.iter().filter(|b| i64::from(b.r) == r) {
            match per_page.iter_mut().find(|(p, _)| *p == b.pg) {
                Some((_, h)) => *h = uo_jsnum::math::max(*h, b.h),
                None => per_page.push((b.pg, b.h)),
            }
        }
        out.push(per_page.iter().fold(0.0, |a, (_, h)| a + h));
        r += 1;
    }
    out
}

// ───────────────────────── the table target (`tableTarget()`, 19346) ─────────────────────────

struct Target<'a> {
    anchor: u32,
    path: Vec<CellHop>,
    rect: CellRect,
    table: &'a Table,
    cells: bool,
}

fn table_target(s: &UoWriteSession) -> Option<Target<'_>> {
    let session = &s.session;
    let doc = session.document();
    let ctx = session.context();
    // the TS: no target in a header/footer or a text box; in a note the body's selection of cells still is one (the TS
    // keeps `tableSel` when a note is entered from it)
    if matches!(ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
        return None;
    }
    if let Some(ts) = session.table_selection() {
        let table = table_at(doc, ts.anchor, &ts.path)?;
        let rect = CellRect { r1: i64::from(ts.row0), c1: i64::from(ts.col0), r2: i64::from(ts.row1), c2: i64::from(ts.col1) };
        return Some(Target { anchor: ts.anchor, path: ts.path.to_vec(), rect, table, cells: true });
    }
    let EditContext::Cell { anchor, hops, .. } = ctx else { return None };
    let (me, path) = hops.split_last()?;
    let table = table_at(doc, *anchor, path)?;
    let m = table_ops::map_table(table);
    let r = CellRect { r1: i64::from(me.row), c1: i64::from(me.col), r2: i64::from(me.row), c2: i64::from(me.col) };
    Some(Target { anchor: *anchor, path: path.to_vec(), rect: table_ops::norm_rect(table, &m, r), table, cells: false })
}

/// The body section's print width (`contentWidthTwips()` of a single-section document; ED-16).
fn content_width(doc: &Document) -> f64 {
    doc.sections.last().map_or(0.0, |s| {
        let p = &s.sect_pr;
        p.page_width_twips.to_number() - p.margin_left_twips.to_number() - p.margin_right_twips.to_number()
    })
}

/// `tableAvail(tg)` (21332): the text column (or the cell outside) minus the indent.
fn table_avail(doc: &Document, tg: &Target<'_>) -> f64 {
    let ind = match tg.table.tbl_pr.as_deref().and_then(|p| p.tbl_ind.as_ref()) {
        Some(w) if &*w.r#type == "dxa" => w.w.to_number(),
        _ => 0.0,
    };
    if let Some((h, outer_path)) = tg.path.split_last()
        && let Some(outer) = table_at(doc, tg.anchor, outer_path)
    {
        let m = table_ops::map_table(outer);
        if let Some(g) = table_ops::cell_at(&m, i64::from(h.row), i64::from(h.col)) {
            return table_ops::grid_width(outer, g.col, g.span) - 216.0 - ind;
        }
    }
    content_width(doc) - ind
}

// ───────────────────────── handles ─────────────────────────

/// # Safety
/// `h` null or live.
unsafe fn session_mut<'a>(h: *mut UoWriteSession) -> Option<&'a mut UoWriteSession> {
    // SAFETY: the caller guarantees `h` is null or live.
    unsafe { h.as_mut() }
}

/// # Safety
/// `l` null or live.
unsafe fn layout_ref<'a>(l: *const UoLayout) -> Option<&'a UoLayout> {
    // SAFETY: the caller guarantees `l` is null or live.
    unsafe { l.as_ref() }
}

// ───────────────────────── the context ─────────────────────────

fn write_context(j: &mut Js, doc: &Document, ctx: &EditContext) {
    j.open('{');
    match ctx {
        EditContext::Body => {
            j.key("kind");
            j.string("body");
        }
        EditContext::Cell { anchor, hops, .. } => {
            j.key("kind");
            j.string("cell");
            j.key("anchor");
            j.int(*anchor);
            j.key("hops");
            write_hops(j, hops);
        }
        EditContext::Note { footnote, id, .. } => {
            j.key("kind");
            j.string("note");
            j.key("footnote");
            j.boolean(*footnote);
            j.key("id");
            j.num(*id);
        }
        EditContext::HeaderFooter { part, page, .. } => {
            j.key("kind");
            j.string("hf");
            j.key("hfKind");
            j.string(match hf_which_of(doc, *part) {
                HfWhich::Header => "header",
                HfWhich::Footer => "footer",
            });
            j.key("part");
            j.int(part.0);
            j.key("page");
            j.int(*page);
        }
        EditContext::Textbox { outer, at, host, focus, page, .. } => {
            j.key("kind");
            j.string("textbox");
            // wave 6 (C): `para`/`run` of a top-level box, null for a box in a cell, a group or a box (`at`: its path)
            let top = at.top_level();
            for (k, v) in [("para", top.map(|t| t.0)), ("run", top.map(|t| t.1))] {
                j.key(k);
                match v {
                    Some(v) => j.int(v),
                    None => j.null(),
                }
            }
            j.key("at");
            j.string(&at.to_string());
            for (k, v) in [("host", *host), ("focus", *focus), ("page", *page)] {
                j.key(k);
                j.int(v);
            }
            j.key("outer");
            write_context(j, doc, outer);
        }
    }
    j.close('}');
}

/// Which kind of part `part` is (a section slot names it; else its path, as `edit_scope`).
fn hf_which_of(doc: &Document, part: uo_docx::ids::HfPartId) -> HfWhich {
    if doc.sections.iter().any(|sec| sec.headers.iter().any(|(_, p)| *p == part)) {
        return HfWhich::Header;
    }
    if doc.sections.iter().any(|sec| sec.footers.iter().any(|(_, p)| *p == part)) {
        return HfWhich::Footer;
    }
    let path = doc.hf.get(part.index()).map(|p| &*p.path).unwrap_or("");
    if path.contains("footer") { HfWhich::Footer } else { HfWhich::Header }
}

/// The session's edit context and the selection of cells, as JSON: `{ kind: 'body' | 'cell' | 'note' |
/// 'hf' | 'textbox', … , tableSel: { anchor, path, rect, origin } | null, locked }` (the TS `cellEdit`,
/// `noteEdit`, `hfEdit`/`hfActivePage`, `tbEdit`, `tableSel`).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_story_context(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let session = &s.session;
    let doc = session.document();
    let mut j = Js::new();
    j.open('{');
    j.key("ctx");
    write_context(&mut j, doc, session.context());
    j.key("tableSel");
    match session.table_selection() {
        Some(ts) => {
            j.open('{');
            j.key("anchor");
            j.int(ts.anchor);
            j.key("path");
            write_hops(&mut j, &ts.path);
            j.key("rect");
            write_rect(&mut j, CellRect { r1: i64::from(ts.row0), c1: i64::from(ts.col0), r2: i64::from(ts.row1), c2: i64::from(ts.col1) });
            j.key("origin");
            j.open('{');
            j.key("r");
            j.int(ts.origin_row);
            j.key("c");
            j.int(ts.origin_col);
            j.close('}');
            j.close('}');
        }
        None => j.null(),
    }
    j.key("locked");
    j.boolean(session.locked());
    j.close('}');
    j.done()
}

// ───────────────────────── hit routing (`engLineAt` + `locateRef`, 18726) ─────────────────────────

/// The line under `(x, y)` on `page` of `layout` (`engLineAt`: the nearest line of any story but `other`)
/// and where it is, as JSON: `{ kind: 'body' | 'cell' | 'hf' | 'note', line, para?, anchor?, hops?,
/// block?, which?, slot?, part?, footnote?, id? }`. Status 1 when the page has no such line.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_locate_at(h: *const UoWriteSession, layout: *const UoLayout, page: u32, x: f64, y: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    if !x.is_finite() || !y.is_finite() {
        return buf(NONE, Vec::new());
    }
    let geom = layout_geom(l);
    let view = s.session.view();
    let doc = s.session.document();
    let loc = Locator::new(view);
    let index = LineIndex::new(&loc, geom);
    let Some(li) = index.line_at(geom, page, x, y) else { return buf(NONE, Vec::new()) };
    let (Some(at), Some(line)) = (index.located(page, li), geom.pages.get(page as usize).and_then(|p| p.lines.get(li as usize))) else {
        return buf(NONE, Vec::new());
    };
    let mut j = Js::new();
    j.open('{');
    j.key("line");
    j.int(li);
    match (at.kind, line.at.story) {
        (LocKind::Body, _) => {
            j.key("kind");
            j.string("body");
            if let Some(&b) = line.at.path.0.first() {
                // the body paragraph index (`origins[b].para`): block b minus the tables before it (table i
                // is block `before_para + i`)
                let tables =
                    doc.body.tables.iter().enumerate().filter(|(i, a)| u64::from(a.before_para) + (*i as u64) < u64::from(b)).count();
                j.key("para");
                j.int(b.saturating_sub(u32_of(tables)));
            }
        }
        (LocKind::Cell, _) => match cell_ref(doc, &line.at.path.0) {
            Some((anchor, hops, block)) => {
                j.key("kind");
                j.string("cell");
                j.key("anchor");
                j.int(anchor);
                j.key("hops");
                write_hops(&mut j, &hops);
                j.key("block");
                j.int(block);
            }
            None => {
                j.key("kind");
                j.string("other");
            }
        },
        (LocKind::HeaderFooter, LineStory::HeaderFooter { which, slot, part }) => {
            j.key("kind");
            j.string("hf");
            j.key("which");
            j.string(if which == HfWhich::Header { "hdr" } else { "ftr" });
            j.key("slot");
            j.string(slot_str(slot));
            j.key("part");
            j.int(part.0);
        }
        (LocKind::Note, LineStory::Note(kind, id)) => {
            j.key("kind");
            j.string("note");
            j.key("footnote");
            j.boolean(kind == uo_docx::view::NoteKind::Footnote);
            j.key("id");
            j.num(id.get());
        }
        _ => {
            j.key("kind");
            j.string("other");
        }
    }
    j.close('}');
    j.done()
}

fn slot_str(s: HfSlot) -> &'static str {
    match s {
        HfSlot::Default => "default",
        HfSlot::First => "first",
        HfSlot::Even => "even",
    }
}

/// `cellBoxAtPoint(pg, x, y)` (20949): the innermost body table cell box holding `(x, y)`, as JSON
/// `{ anchor, path, r, c, x, y, w, h, span }` (`path` = the hops to the cell holding the table); status 1
/// when no cell holds the point.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_cell_at(h: *const UoWriteSession, layout: *const UoLayout, page: u32, x: f64, y: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let geom = layout_geom(l);
    let doc = s.session.document();
    let Some(p) = geom.pages.get(page as usize) else { return buf(NONE, Vec::new()) };
    if !x.is_finite() || !y.is_finite() {
        return buf(NONE, Vec::new());
    }
    let mut best: Option<(u32, Vec<CellHop>, &uo_docx_layout::output::PlacedCell)> = None;
    for b in p.cells.as_deref().unwrap_or(&[]) {
        if x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h {
            continue;
        }
        if b.at.story != LineStory::Body {
            continue;
        }
        // `locateRef(box.ref + '/b0')`
        let mut segs: Vec<u32> = b.at.path.0.to_vec();
        segs.push(0);
        let Some((anchor, hops, _)) = cell_ref(doc, &segs) else { continue };
        if best.as_ref().is_none_or(|(_, bh, _)| hops.len() > bh.len()) {
            best = Some((anchor, hops, b));
        }
    }
    let Some((anchor, hops, b)) = best else { return buf(NONE, Vec::new()) };
    let Some((last, path)) = hops.split_last() else { return buf(NONE, Vec::new()) };
    let mut j = Js::new();
    j.open('{');
    j.key("anchor");
    j.int(anchor);
    j.key("path");
    write_hops(&mut j, path);
    j.key("r");
    j.int(last.row);
    j.key("c");
    j.int(last.col);
    for (k, v) in [("x", b.x), ("y", b.y), ("w", b.w), ("h", b.h), ("span", b.span)] {
        j.key(k);
        j.num(v);
    }
    j.close('}');
    j.done()
}

// ───────────────────────── the table state (`getTableState()`, 21305) ─────────────────────────

/// `getTableState()` as JSON (the TS `TableState`), plus `layoutIn: { rowHeights }` (the displayed height of
/// every row of the target table, the `TableLayoutIn` of the session's table commands) and `anchor`,
/// `path`; status 1 = null (no target: a header/footer or text box context, or not in a table).
///
/// # Safety
/// `h`, `layout` null or live (`layout` may be null: the displayed heights are then 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_state(h: *const UoWriteSession, layout: *const UoLayout) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    // SAFETY: the caller guarantees `layout` is null or live.
    let l = unsafe { layout_ref(layout) };
    let Some(tg) = table_target(s) else { return buf(NONE, Vec::new()) };
    let doc = s.session.document();
    let t = tg.table;
    let m = table_ops::map_table(t);
    let g = table_ops::cell_at(&m, tg.rect.r1, tg.rect.c1);
    let cell = g.and_then(|g| t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci)));
    let tc_pr = cell.and_then(|c| c.tc_pr.as_deref());
    let r1 = usize::try_from(tg.rect.r1.max(0)).unwrap_or(0);
    let r2 = usize::try_from(tg.rect.r2.max(0)).unwrap_or(0);
    let first_row = t.rows.get(r1);
    let trh = first_row.and_then(|r| r.tr_pr.as_deref()).and_then(|p| p.tr_height.as_ref());
    let style = t.tbl_pr.as_deref().and_then(|p| p.tbl_style.as_deref());
    let boxes = l.map(|l| table_boxes(doc, layout_geom(l), tg.anchor, &tg.path)).unwrap_or_default();
    let n_rows = i64::try_from(t.rows.len()).unwrap_or(0);

    let mut j = Js::new();
    j.open('{');
    j.key("rows");
    j.num(f(t.rows.len()));
    j.key("cols");
    j.num(table_ops::num(m.n_cols));
    j.key("rect");
    write_rect(&mut j, tg.rect);
    j.key("cells");
    j.boolean(tg.cells);
    j.key("nested");
    j.boolean(!tg.path.is_empty());
    j.key("canMerge");
    j.boolean(table_ops::can_merge(t, tg.rect));
    if let Some(st) = style {
        j.key("style");
        j.string(st);
    }
    // `{ ...DEFAULT_LOOK, ...tblLook }`
    let d = table_ops::default_look();
    let look = t.tbl_pr.as_deref().and_then(|p| p.tbl_look.as_deref());
    j.key("look");
    j.open('{');
    for (k, dv, v) in [
        ("firstRow", d.first_row, look.and_then(|l| l.first_row)),
        ("lastRow", d.last_row, look.and_then(|l| l.last_row)),
        ("firstColumn", d.first_column, look.and_then(|l| l.first_column)),
        ("lastColumn", d.last_column, look.and_then(|l| l.last_column)),
        ("noHBand", d.no_h_band, look.and_then(|l| l.no_h_band)),
        ("noVBand", d.no_v_band, look.and_then(|l| l.no_v_band)),
    ] {
        if let Some(b) = v.or(dv) {
            j.key(k);
            j.boolean(b);
        }
    }
    j.close('}');
    j.key("vAlign");
    j.string(tc_pr.and_then(|p| p.v_align.as_deref()).unwrap_or("top"));
    j.key("textDirection");
    j.string(tc_pr.and_then(|p| p.text_direction.as_deref()).unwrap_or("lrTb"));
    j.key("headerRow");
    j.boolean(t.rows.iter().take(r2 + 1).all(|r| r.tr_pr.as_deref().and_then(|p| p.tbl_header).unwrap_or(false)));
    j.key("rowHeight");
    j.open('{');
    match trh {
        // `trh ? { twips: trh.val, rule: trh.hRule }`: an object with a value
        Some(th) => {
            j.key("twips");
            j.num(th.val.to_number());
            j.key("rule");
            j.string(&th.h_rule);
        }
        None => {
            j.key("twips");
            j.num(row_heights(&boxes, tg.rect.r1, tg.rect.r1).first().copied().unwrap_or(0.0));
            j.key("rule");
            j.string("auto");
        }
    }
    j.close('}');
    j.key("colWidth");
    j.num(table_ops::grid_width(t, tg.rect.c1, tg.rect.c2 - tg.rect.c1 + 1));
    if let Some(fill) = tc_pr.and_then(|p| p.shd.as_deref()).and_then(|s| s.fill.as_deref())
        && !fill.is_empty()
        && fill != "auto"
    {
        j.key("shading");
        j.string(fill);
    }
    j.key("tblPr");
    match t.tbl_pr.as_deref() {
        Some(p) => j.model(p, "{}"),
        None => j.model(&uo_docx::model::props::TblPr::default(), "{}"),
    }
    j.key("trPr");
    match first_row.and_then(|r| r.tr_pr.as_deref()) {
        Some(p) => j.model(p, "{}"),
        None => j.model(&uo_docx::model::props::TrPr::default(), "{}"),
    }
    j.key("tcPr");
    match tc_pr {
        Some(p) => j.model(p, "{}"),
        None => j.model(&uo_docx::model::props::TcPr::default(), "{}"),
    }
    if let Some(st) = style {
        let borders = doc.formatting.styles.styles.get(st).and_then(|d| d.tbl_pr.as_deref()).and_then(|p| p.tbl_borders.as_deref());
        j.key("styleBorders");
        match borders {
            Some(b) => j.model(b, "{}"),
            None => j.model(&uo_docx::model::props::TblBorders::default(), "{}"),
        }
    }
    j.key("availTwips");
    j.num(table_avail(doc, &tg));
    // not in the TS state: the shell's arguments of the table commands
    j.key("anchor");
    j.int(tg.anchor);
    j.key("path");
    write_hops(&mut j, &tg.path);
    j.key("layoutIn");
    j.open('{');
    j.key("rowHeights");
    j.open('[');
    for v in row_heights(&boxes, 0, n_rows - 1) {
        j.num(v);
    }
    j.close(']');
    j.close('}');
    j.close('}');
    j.done()
}

/// The quads `x, y, w, h` (f64 little endian) of the selection of cells on `page` (`paintTableSel`,
/// 18628): the placed boxes of the selected rows and grid columns. Empty without a selection of cells.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_sel_rects(h: *const UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(ts) = s.session.table_selection() else { return buf(OK, Vec::new()) };
    let doc = s.session.document();
    let mut out = Vec::new();
    for b in table_boxes(doc, layout_geom(l), ts.anchor, &ts.path) {
        let span = b.span;
        if b.pg != page || b.r < ts.row0 || b.r > ts.row1 || f64::from(b.c) + span - 1.0 < f64::from(ts.col0) || b.c > ts.col1 {
            continue;
        }
        for v in [b.x, b.y, b.w, b.h] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    buf(OK, out)
}

/// The placed cell boxes of `page` (x, y, w, h quads, f64 little endian): the table gridlines of View
/// Gridlines (`paintGridlines`).
///
/// # Safety
/// `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_cell_boxes(layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `layout` is null or live.
    let Some(l) = (unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut out = Vec::new();
    if let Some(p) = layout_geom(l).pages.get(page as usize) {
        for b in p.cells.as_deref().unwrap_or(&[]) {
            for v in [b.x, b.y, b.w, b.h] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    buf(OK, out)
}

// ───────────────────────── table navigation (`tableNavigate` 21100, `moveCell` 19272) ─────────────────────────

/// The change sets of the steps of one shell gesture, as one: the events in order, the stories united,
/// the selection and flags of the last.
struct Steps {
    cs: Option<ChangeSet>,
}

impl Steps {
    fn new() -> Steps {
        Steps { cs: None }
    }

    /// Runs one command (the lines cache is dropped by `command`).
    fn run(&mut self, s: &mut UoWriteSession, c: Command) -> bool {
        match s.command(c) {
            Ok(Outcome::Done(cs)) => {
                self.add(cs);
                true
            }
            _ => false,
        }
    }

    fn add(&mut self, cs: ChangeSet) {
        match &mut self.cs {
            None => self.cs = Some(cs),
            Some(acc) => {
                for st in cs.stories {
                    if !acc.stories.contains(&st) {
                        acc.stories.push(st);
                    }
                }
                acc.events.extend(cs.events);
                acc.selection = cs.selection;
                acc.dirty = cs.dirty;
                acc.can_undo = cs.can_undo;
                acc.can_redo = cs.can_redo;
                acc.formatting |= cs.formatting;
            }
        }
    }

    /// The `ExecResult` of the gesture (a gesture that did nothing visible still answers: the TS `true`).
    fn answer(mut self, s: &mut UoWriteSession) -> *mut UoBuf {
        let cs = match self.cs.take() {
            Some(cs) => cs,
            None => s.session.finish(),
        };
        exec_result(cs.into_wire().map(w::ExecOutcome::Done).map_err(|e| e.to_string()))
    }
}

/// The geometry of the edit lines the navigation reads (copied out of the lines cache: the commands drop it).
struct LineGeom {
    para: u32,
    /// index of the line in its paragraph, and the paragraph's line count
    k: usize,
    n: usize,
    /// first or last line of all the edited lines
    first_all: bool,
    last_all: bool,
}

fn line_info(el: &EditLines, pos: EditPos) -> Option<LineGeom> {
    let line = el.line_of(pos)?;
    for (pi, p) in el.paras.iter().enumerate() {
        for (k, l) in p.lines.iter().enumerate() {
            if std::ptr::eq(l, line) {
                let first_para = el.paras.iter().position(|q| !q.lines.is_empty());
                let last_para = el.paras.iter().rposition(|q| !q.lines.is_empty());
                return Some(LineGeom {
                    para: u32_of(pi),
                    k,
                    n: p.lines.len(),
                    first_all: first_para == Some(pi) && k == 0,
                    last_all: last_para == Some(pi) && k + 1 == p.lines.len(),
                });
            }
        }
    }
    None
}

/// `hitLineX` on the first or last line of all the edited lines (`enterRowAt`'s target).
fn edge_line_offset(el: &EditLines, last: bool, x: f64) -> Option<DocPosition> {
    let (pi, line): (usize, &EditLine) = if last {
        let pi = el.paras.iter().rposition(|p| !p.lines.is_empty())?;
        (pi, el.paras.get(pi)?.lines.last()?)
    } else {
        let pi = el.paras.iter().position(|p| !p.lines.is_empty())?;
        (pi, el.paras.get(pi)?.lines.first()?)
    };
    Some(DocPosition::new(u32_of(pi), el.hit_line_x(line, x)))
}

/// `hitLineX` on the first or last line of paragraph `p` of the edited lines.
fn para_line_offset(el: &EditLines, p: u32, last: bool, x: f64) -> Option<u32> {
    let para = el.paras.get(p as usize)?;
    let line = if last { para.lines.last()? } else { para.lines.first()? };
    Some(el.hit_line_x(line, x))
}

fn caret_x(el: &EditLines, pos: EditPos) -> f64 {
    el.line_of(pos).map_or(0.0, |line| el.caret_x(pos.offset, line))
}

fn sel(p: DocPosition) -> Command {
    Command::Text(TextCmd::SetSelection { anchor: p, focus: p })
}

/// `enterCellAt(anchor, path, r, c, atEnd)` (19363) through `ContextCmd::EnterCell`.
fn enter_cell_at(steps: &mut Steps, s: &mut UoWriteSession, anchor: u32, path: &[CellHop], r: i64, c: i64, at_end: bool) -> bool {
    let cmd = {
        let doc = s.session.document();
        let Some(t) = table_at(doc, anchor, path) else { return false };
        if t.rows.is_empty() {
            return false;
        }
        let m = table_ops::map_table(t);
        let ri = r.clamp(0, i64::try_from(t.rows.len()).unwrap_or(1) - 1);
        let Some(g) = table_ops::cell_at(&m, ri, c).or_else(|| m.row(ri).last().copied()) else { return false };
        let Some(cell) = t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci)) else { return false };
        let last = cell.content.paras.len().saturating_sub(1);
        let caret =
            if at_end { cell.content.paras.get(last).map(|p| DocPosition::new(u32_of(last), u32_of(para_length(p)))) } else { None };
        let mut hops = path.to_vec();
        hops.push(CellHop { row: u32_of(g.ri), col: u32::try_from(g.col.max(0)).unwrap_or(0), block: None });
        Command::Context(ContextCmd::EnterCell { anchor, hops, caret })
    };
    steps.run(s, cmd)
}

/// `enterRowAt(anchor, path, r, x, fromBelow)` (21178): the cell of row `r` under `x`, then the caret on
/// its first or last line at `x`.
fn enter_row_at(
    steps: &mut Steps,
    s: &mut UoWriteSession,
    l: &UoLayout,
    (anchor, path): (u32, &[CellHop]),
    r: i64,
    x: f64,
    from_below: bool,
) {
    let (r, c) = {
        let doc = s.session.document();
        let mut c = 0i64;
        for b in table_boxes(doc, layout_geom(l), anchor, path) {
            if i64::from(b.r) == r && x >= b.x && x <= b.x + b.w {
                c = i64::from(b.c);
                break;
            }
        }
        let mut r = r;
        if let Some(t) = table_at(doc, anchor, path) {
            let m = table_ops::map_table(t);
            let cont = table_ops::cell_at(&m, r, c)
                .and_then(|g| t.rows.get(g.ri).and_then(|row| row.cells.get(g.ci)))
                .is_some_and(|cell| table_ops::v_merge_of(cell) == VMerge::Continue);
            if cont {
                r = table_ops::v_extent(t, &m, r, c).0;
            }
        }
        (r, c)
    };
    if !enter_cell_at(steps, s, anchor, path, r, c, false) {
        return;
    }
    let target = s.lines(l).and_then(|el| edge_line_offset(el, from_below, x));
    if let Some(p) = target {
        steps.run(s, sel(p));
    }
}

/// `leaveTable(anchor, path, where, x?)` (21195).
fn leave_table(steps: &mut Steps, s: &mut UoWriteSession, l: &UoLayout, anchor: u32, path: &[CellHop], after: bool, x: Option<f64>) {
    if let Some((h, outer)) = path.split_last() {
        // nested: back to the cell outside
        let outer = outer.to_vec();
        enter_cell_at(steps, s, anchor, &outer, i64::from(h.row), i64::from(h.col), after);
        return;
    }
    let (before_para, nb, rows_nb, last_cell) = {
        let doc = s.session.document();
        let Some(a) = doc.body.tables.get(anchor as usize) else { return };
        let bp = a.before_para;
        let tables: Vec<u32> = doc.body.tables.iter().map(|t| t.before_para).collect();
        let nb = if after {
            tables.iter().enumerate().find(|(i, b)| u32_of(*i) > anchor && **b == bp).map(|(i, _)| u32_of(i))
        } else {
            tables.iter().enumerate().rev().find(|(i, b)| u32_of(*i) < anchor && **b == bp).map(|(i, _)| u32_of(i))
        };
        let (rows_nb, last_cell) = nb
            .and_then(|n| doc.body.tables.get(n as usize))
            .map(|t| {
                let m = table_ops::map_table(&t.table);
                let lc = m.rows.last().and_then(|r| r.last()).map(|g| (i64::try_from(g.ri).unwrap_or(0), g.col));
                (i64::try_from(t.table.rows.len()).unwrap_or(0), lc)
            })
            .unwrap_or((0, None));
        (bp, nb, rows_nb, last_cell)
    };
    if let Some(nb) = nb {
        match (x, after) {
            (Some(x), _) => enter_row_at(steps, s, l, (nb, &[]), if after { 0 } else { rows_nb - 1 }, x, !after),
            (None, true) => {
                enter_cell_at(steps, s, nb, &[], 0, 0, false);
            }
            (None, false) => {
                if let Some((r, c)) = last_cell {
                    enter_cell_at(steps, s, nb, &[], r, c, true);
                }
            }
        }
        return;
    }
    let para = if after { i64::from(before_para) } else { i64::from(before_para) - 1 };
    if para < 0 || !matches!(s.session.context(), EditContext::Cell { .. }) {
        return;
    }
    steps.run(s, Command::Context(ContextCmd::ExitCell));
    let Some(n) = s.session.paras().map(|p| p.len()) else { return };
    let p = u32::try_from(para.min(i64::try_from(n).unwrap_or(0) - 1).max(0)).unwrap_or(0);
    let len = s.session.paras().and_then(|ps| ps.get(p as usize)).map_or(0, para_length);
    let mut offset = if after { 0 } else { u32_of(len) };
    if let Some(x) = x
        && let Some(o) = s.lines(l).and_then(|el| para_line_offset(el, p, !after, x))
    {
        offset = o;
    }
    steps.run(s, sel(DocPosition::new(p, offset)));
}

/// `tableNavigate(dir, extend)` (21100; with `enterRowAt` and `leaveTable`): the arrows between cells and
/// between the body and its tables. `dir` 0 left, 1 right, 2 up, 3 down. An `ExecResult` when the TS
/// handles the key (its `true`), status 1 when the default caret move runs (its `false`).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_navigate(h: *mut UoWriteSession, layout: *const UoLayout, dir: u32, extend: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let extend = extend != 0;
    if dir > 3 || !matches!(s.session.context(), EditContext::Body | EditContext::Cell { .. }) || s.session.paras().is_none() {
        return buf(NONE, Vec::new());
    }
    let (left, right, up, down) = (dir == 0, dir == 1, dir == 2, dir == 3);
    let focus = s.session.selection().focus;
    let pos = EditPos { para: focus.para, offset: focus.offset };
    let (info, x) = match s.lines(l) {
        Some(el) => (line_info(el, pos), caret_x(el, pos)),
        None => (None, 0.0),
    };
    let ps_len = s.session.paras().map_or(0, |p| p.len());
    let para_len = |s: &UoWriteSession, i: u32| s.session.paras().and_then(|p| p.get(i as usize)).map_or(0, para_length);
    let mut steps = Steps::new();
    if let EditContext::Cell { anchor, hops, .. } = s.session.context().clone() {
        let at_start = focus.para == 0 && focus.offset == 0;
        let at_end = ps_len > 0 && focus.para as usize == ps_len - 1 && focus.offset as usize == para_len(s, focus.para);
        let leaving = (left && at_start)
            || (right && at_end)
            || (up && info.as_ref().is_none_or(|i| i.first_all))
            || (down && info.as_ref().is_none_or(|i| i.last_all));
        if !leaving {
            return buf(NONE, Vec::new());
        }
        let Some((me, path)) = hops.split_last() else { return buf(NONE, Vec::new()) };
        let path = path.to_vec();
        let me = *me;
        let (mr, mc) = (i64::from(me.row), i64::from(me.col));
        if extend {
            // Shift+arrow leaving the cell: the selection of cells begins
            let cmd = {
                let doc = s.session.document();
                let Some(t) = table_at(doc, anchor, &path) else { return buf(NONE, Vec::new()) };
                let m = table_ops::map_table(t);
                let g = table_ops::cell_at(&m, mr, mc);
                let (mut r, mut c) = (mr, mc);
                let rows = i64::try_from(t.rows.len()).unwrap_or(0);
                if up {
                    r = (table_ops::v_extent(t, &m, mr, mc).0 - 1).max(0);
                } else if down {
                    r = (table_ops::v_extent(t, &m, mr, mc).1 + 1).min(rows - 1);
                } else if left {
                    c = (mc - 1).max(0);
                } else {
                    c = (m.n_cols - 1).min(g.map_or(mc + 1, |g| g.col + g.span));
                }
                let rect = table_ops::norm_rect(t, &m, CellRect { r1: mr.min(r), c1: mc.min(c), r2: mr.max(r), c2: mc.max(c) });
                let n = table_ops::cells_in(&m, rect)
                    .iter()
                    .filter(|x| {
                        t.rows.get(x.ri).and_then(|row| row.cells.get(x.ci)).is_some_and(|c| table_ops::v_merge_of(c) != VMerge::Continue)
                    })
                    .count();
                (n > 1).then(|| Command::Table(TableCmd::SetTableSelection { anchor, path: path.clone(), rect, origin: (mr, mc) }))
            };
            if let Some(cmd) = cmd {
                steps.run(s, cmd);
            }
            return steps.answer(s);
        }
        if left || right {
            let next = {
                let doc = s.session.document();
                let Some(t) = table_at(doc, anchor, &path) else { return buf(NONE, Vec::new()) };
                let m = table_ops::map_table(t);
                let order: Vec<(i64, i64)> = m
                    .rows
                    .iter()
                    .flatten()
                    .filter(|g| {
                        t.rows.get(g.ri).and_then(|row| row.cells.get(g.ci)).is_some_and(|c| table_ops::v_merge_of(c) != VMerge::Continue)
                    })
                    .map(|g| (i64::try_from(g.ri).unwrap_or(0), g.col))
                    .collect();
                let i = order.iter().position(|&(r, c)| r == mr && c == mc);
                let j = match (i, right) {
                    (Some(i), true) => i.checked_add(1),
                    (Some(i), false) => i.checked_sub(1),
                    // `order.findIndex(...)` = −1: `order[0]` to the right, `order[-2]` (none) to the left
                    (None, true) => Some(0),
                    (None, false) => None,
                };
                j.and_then(|j| order.get(j).copied())
            };
            match next {
                Some((r, c)) => {
                    enter_cell_at(&mut steps, s, anchor, &path, r, c, left);
                }
                None => leave_table(&mut steps, s, l, anchor, &path, right, None),
            }
            return steps.answer(s);
        }
        let (a, b, rows) = {
            let doc = s.session.document();
            let Some(t) = table_at(doc, anchor, &path) else { return buf(NONE, Vec::new()) };
            let m = table_ops::map_table(t);
            let (a, b) = table_ops::v_extent(t, &m, mr, mc);
            (a, b, i64::try_from(t.rows.len()).unwrap_or(0))
        };
        let r = if down { b + 1 } else { a - 1 };
        if r >= 0 && r < rows {
            enter_row_at(&mut steps, s, l, (anchor, &path), r, x, up);
        } else {
            leave_table(&mut steps, s, l, anchor, &path, down, Some(x));
        }
        return steps.answer(s);
    }
    if extend {
        return buf(NONE, Vec::new());
    }
    // body → the table next to the caret's paragraph
    let p = focus.para;
    let last_line = info.as_ref().is_none_or(|i| i.para != p || i.k + 1 == i.n);
    let first_line = info.as_ref().is_none_or(|i| i.para != p || i.k == 0);
    let plen = u32_of(para_len(s, p));
    let anchors: Vec<(u32, i64)> =
        s.session.document().body.tables.iter().map(|a| (a.before_para, i64::try_from(a.table.rows.len()).unwrap_or(0))).collect();
    if (down && last_line) || (right && focus.offset == plen) {
        let Some(ai) = anchors.iter().position(|(b, _)| Some(*b) == p.checked_add(1)) else { return buf(NONE, Vec::new()) };
        let ai = u32_of(ai);
        if down {
            enter_row_at(&mut steps, s, l, (ai, &[]), 0, x, false);
        } else {
            enter_cell_at(&mut steps, s, ai, &[], 0, 0, false);
        }
        return steps.answer(s);
    }
    if (up && first_line) || (left && focus.offset == 0) {
        let Some(ai) = anchors.iter().rposition(|(b, _)| *b == p) else { return buf(NONE, Vec::new()) };
        let rows = anchors.get(ai).map_or(0, |a| a.1);
        let ai = u32_of(ai);
        if up {
            enter_row_at(&mut steps, s, l, (ai, &[]), rows - 1, x, true);
        } else {
            let last = {
                let doc = s.session.document();
                doc.body.tables.get(ai as usize).and_then(|t| {
                    let m = table_ops::map_table(&t.table);
                    m.rows.last().and_then(|r| r.last()).map(|g| (i64::try_from(g.ri).unwrap_or(0), g.col))
                })
            };
            if let Some((r, c)) = last {
                enter_cell_at(&mut steps, s, ai, &[], r, c, true);
            }
        }
        return steps.answer(s);
    }
    buf(NONE, Vec::new())
}

/// `moveCell(dir)` (19272): Tab / Shift+Tab in a cell. The cells of the same table in the order of the
/// layout's lines; the next one is entered with its whole content selected; Tab in the last cell adds a
/// row (`tabNewRow`) when editing is not locked. `dir` 1 or −1 (as `i32`). An `ExecResult`, status 1
/// when nothing moves (the TS `false`).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_move_cell(h: *mut UoWriteSession, layout: *const UoLayout, dir: i32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let EditContext::Cell { anchor, hops, .. } = s.session.context().clone() else { return buf(NONE, Vec::new()) };
    let Some((me, prefix)) = hops.split_last() else { return buf(NONE, Vec::new()) };
    let prefix = prefix.to_vec();
    let mut seen: Vec<(u32, u32)> = Vec::new();
    {
        let doc = s.session.document();
        for p in &layout_geom(l).pages {
            for line in &p.lines {
                if line.at.story != LineStory::Body {
                    continue;
                }
                let Some((a, lh, _)) = cell_ref(doc, &line.at.path.0) else { continue };
                if a != anchor || lh.len() != hops.len() {
                    continue;
                }
                let Some((last, lp)) = lh.split_last() else { continue };
                if !same_hops(lp, &prefix) {
                    continue;
                }
                if !seen.contains(&(last.row, last.col)) {
                    seen.push((last.row, last.col));
                }
            }
        }
    }
    seen.sort_unstable();
    let idx = seen.iter().position(|&(r, c)| r == me.row && c == me.col);
    let next = idx.and_then(|i| if dir >= 0 { i.checked_add(1) } else { i.checked_sub(1) }).and_then(|j| seen.get(j).copied());
    let mut steps = Steps::new();
    if idx.is_some() && next.is_none() && dir == 1 && !s.session.locked() {
        steps.run(s, Command::Table(TableCmd::TabNewRow));
        return steps.answer(s);
    }
    let Some((r, c)) = next else { return buf(NONE, Vec::new()) };
    let mut nh = prefix;
    nh.push(CellHop { row: r, col: c, block: None });
    if !steps.run(s, Command::Context(ContextCmd::EnterCell { anchor, hops: nh, caret: None })) {
        return buf(NONE, Vec::new());
    }
    // the cell's whole content selected (Word selects on entering by Tab)
    let end = s.session.paras().and_then(|ps| {
        let last = ps.len().checked_sub(1)?;
        Some(DocPosition::new(u32_of(last), u32_of(para_length(ps.get(last)?))))
    });
    if let Some(end) = end {
        steps.run(s, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: end }));
    }
    steps.answer(s)
}

// ───────────────────────── headers and footers (`hfPartOfPage` 18686, `hfBand` 18708) ─────────────────────────

/// `hfTypeOfPage(pg)` (11222): the variant the Word rule gives page `pg` (first page of a section with
/// Different First Page, an even page with Different Odd & Even, else the default).
fn rule_slot(s: &UoWriteSession, geom: &Layout, pg: usize) -> HfSlot {
    let doc = s.session.document();
    let Some(page) = geom.pages.get(pg) else { return HfSlot::Default };
    let sp = doc.sections.get(page.section as usize).or_else(|| doc.sections.first()).map(|x| &x.sect_pr);
    let mut sect_page = 0usize;
    let mut i = pg;
    while i > 0 && geom.pages.get(i - 1).is_some_and(|p| p.section == page.section) {
        sect_page += 1;
        i -= 1;
    }
    if sect_page == 0 && sp.and_then(|p| p.title_pg).unwrap_or(false) {
        HfSlot::First
    } else if s.session.even_odd_headers() && uo_jsnum::math::rem(page.number, 2.0) == 0.0 {
        HfSlot::Even
    } else {
        HfSlot::Default
    }
}

fn write_hf_side(j: &mut Js, s: &UoWriteSession, geom: &Layout, pg: usize, which: HfWhich) {
    let session = &s.session;
    let doc = session.document();
    let Some(page) = geom.pages.get(pg) else { return };
    let header = which == HfWhich::Header;
    // the variant: the page's lines, else the Word rule (`hfPartOfPage`)
    let mut slot: Option<HfSlot> = None;
    let mut part: Option<uo_docx::ids::HfPartId> = None;
    for l in &page.lines {
        if let LineStory::HeaderFooter { which: w2, slot: sl, part: pt } = l.at.story
            && w2 == which
        {
            slot = Some(sl);
            part = Some(pt);
            break;
        }
    }
    let sect = page.section as usize;
    let sp = doc.sections.get(sect).or_else(|| doc.sections.first()).map(|x| &x.sect_pr);
    let slot = slot.unwrap_or_else(|| rule_slot(s, geom, pg));
    // `hfBlocksFor(full, page.section, which, type)`: the section that DEFINES the slot (inheritance)
    let key = slot_str(slot);
    let mut definer: Option<usize> = None;
    let top = sect.min(doc.sections.len().saturating_sub(1));
    for i in (0..=top).rev() {
        let Some(sec) = doc.sections.get(i) else { continue };
        let set = if header { &sec.headers } else { &sec.footers };
        if let Some(pt) = set.get(key) {
            definer = Some(i);
            if part.is_none() {
                part = Some(*pt);
            }
            break;
        }
    }
    // the band (`hfBand`): the margin or what the part's lines take, the larger
    // the page height of the section (the layout handle keeps the page sizes in its render view; the TS
    // `page.heightTw ?? sp.pageHeightTwips`: equal for a laid-out page)
    let page_h = sp.map_or(15840.0, |p| p.page_height_twips.to_number());
    let (mt, mb) = sp.map_or((1440.0, 1440.0), |p| (p.margin_top_twips.to_number(), p.margin_bottom_twips.to_number()));
    let mut top_y = if header { 0.0 } else { page_h - mb };
    let mut bottom = if header { mt } else { page_h };
    for l in &page.lines {
        if let LineStory::HeaderFooter { which: w2, .. } = l.at.story
            && w2 == which
        {
            if header {
                bottom = uo_jsnum::math::max(bottom, l.y + l.height);
            } else {
                top_y = uo_jsnum::math::min(top_y, l.y);
            }
        }
    }
    // "Same as Previous" (`hfBandLabels`): a later section without its own reference of the variant
    let sec_count = doc.sections.len();
    let label_sec = sect.min(sec_count.saturating_sub(1));
    let linked = label_sec > 0
        && doc.sections.get(label_sec).is_some_and(|sec| {
            let refs = if header { &sec.header_refs } else { &sec.footer_refs };
            !refs.iter().any(|r| &*r.r#type == key)
        });
    j.open('{');
    j.key("slot");
    j.string(key);
    j.key("section");
    j.num(f(definer.unwrap_or(sect)));
    j.key("part");
    match part {
        Some(p) => j.int(p.0),
        None => j.null(),
    }
    j.key("top");
    j.num(top_y);
    j.key("bottom");
    j.num(bottom);
    j.key("linked");
    j.boolean(linked);
    j.close('}');
}

/// Where the headers and footers of page `page` are (one answer for the TS `hfPartOfPage`, `hfBand`,
/// `hfZoneAt`, `hfRegionAtClick`, `hfBandLabels`, `hfTypeOfPage` and `hfContext`), as JSON: `{ page,
/// section, sections, number, evenOdd, width, height, marginLeft, marginRight, marginTop, marginBottom,
/// header: { slot, section, part | null, top, bottom, linked }, footer: {…} }`. `section` of a side is
/// the section that defines its part (or the page's when none does: the part is created there). Status 1
/// for a page out of range.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_hf_page(h: *const UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let geom = layout_geom(l);
    let pg = page as usize;
    let Some(p) = geom.pages.get(pg) else { return buf(NONE, Vec::new()) };
    let doc = s.session.document();
    let sect = (p.section as usize).min(doc.sections.len().saturating_sub(1));
    let sp = doc.sections.get(sect).map(|x| &x.sect_pr);
    let mut j = Js::new();
    j.open('{');
    j.key("page");
    j.int(page);
    j.key("section");
    j.num(f(sect));
    j.key("sections");
    j.num(f(doc.sections.len()));
    j.key("number");
    j.num(p.number);
    j.key("evenOdd");
    j.boolean(s.session.even_odd_headers());
    j.key("type");
    j.string(slot_str(rule_slot(s, geom, pg)));
    j.key("width");
    j.num(sp.map_or(12240.0, |p| p.page_width_twips.to_number()));
    j.key("height");
    j.num(sp.map_or(15840.0, |p| p.page_height_twips.to_number()));
    if let Some(sp) = sp {
        for (k, v) in [
            ("marginLeft", sp.margin_left_twips),
            ("marginRight", sp.margin_right_twips),
            ("marginTop", sp.margin_top_twips),
            ("marginBottom", sp.margin_bottom_twips),
        ] {
            j.key(k);
            j.num(v.to_number());
        }
    }
    j.key("header");
    write_hf_side(&mut j, s, geom, pg, HfWhich::Header);
    j.key("footer");
    write_hf_side(&mut j, s, geom, pg, HfWhich::Footer);
    j.close('}');
    j.done()
}

// ───────────────────────── notes (`noteRunAt` 18934 + `isNoteMark`) ─────────────────────────

/// The note run at `(para, offset)` of the edited story (`noteRunAt`: a reference, or the note's number
/// mark), as JSON `{ ref: { footnote, id } | null, mark }`; status 1 when no such run is there.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_note_at(h: *const UoWriteSession, para: u32, offset: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(p) = s.session.paras().and_then(|ps| ps.get(para as usize)) else { return buf(NONE, Vec::new()) };
    let mut acc = 0usize;
    let off = offset as usize;
    for r in &p.runs {
        let end = acc + r.text.len();
        let ext = r.ext();
        let note_ref = ext.and_then(|e| e.note_ref.as_ref());
        let mark = ext.and_then(|e| e.field.as_deref()).is_some_and(uo_docx::session::settle::is_note_mark_field);
        if (note_ref.is_some() || mark) && off >= acc && off <= end {
            let mut j = Js::new();
            j.open('{');
            j.key("ref");
            match note_ref {
                Some(n) => {
                    j.open('{');
                    j.key("footnote");
                    j.boolean(&*n.kind != "endnote");
                    j.key("id");
                    j.num(n.id.to_number());
                    j.close('}');
                }
                None => j.null(),
            }
            j.key("mark");
            j.boolean(mark);
            j.close('}');
            return j.done();
        }
        acc = end;
    }
    buf(NONE, Vec::new())
}

// ───────────────────────── the text box scope (WS-D6) ─────────────────────────

/// The chain of the text box being edited (`textboxChain(tb)`, universal-write.ts 2810, over the story that holds
/// the box — `storyBlocksOf`: the body or the header/footer part — in `forEachTextbox` order: boxes in cells, group
/// items and boxes included): the head's drawing (the session edits the head's content), then the boxes linked to
/// its story in `seq` order (a stable sort, as the JS one); the head alone when the TS answers no chain (no head of
/// that story or no linked box). And the header/footer page when the box is in a header or footer. `None` outside
/// a text box context. Wave 6 (C): the first cut gathered the linked boxes of every story but not of group items
/// (f7d__7__10: 4 of the TS's 5 members).
#[must_use]
pub fn textbox_chain(s: &uo_docx::session::Session) -> Option<(Vec<uo_docx::ids::DrawingId>, Option<u32>)> {
    use uo_docx::js::JsNumber;
    let ctx = s.context();
    let EditContext::Textbox { outer, page, .. } = ctx else { return None };
    let doc = s.document();
    let tb = uo_docx::session::ops::context::textbox_of(doc, ctx)?;
    let mut chain = vec![tb.drawing];
    if (tb.story.is_some() || tb.link.is_some())
        && let Some(story) = uo_docx::session::ops::context::story_of(doc, outer)
    {
        let mut order = Vec::new();
        textbox_order(story, &mut order, 0);
        let id = match &tb.link {
            Some(l) => l.id.to_number(),
            None => tb.story.map_or(f64::NAN, JsNumber::to_number),
        };
        // `heads`: the first box of each story id that links nowhere
        let head = order.iter().find(|t| t.link.is_none() && t.story.is_some_and(|v| v.to_number() == id));
        let mut ms: Vec<&uo_docx::model::run::DrawingTextbox> =
            order.iter().copied().filter(|t| t.link.as_ref().is_some_and(|l| l.id.to_number() == id)).collect();
        ms.sort_by(|a, b| {
            let (x, y) = (a.link.as_ref().map_or(f64::NAN, |l| l.seq.to_number()), b.link.as_ref().map_or(f64::NAN, |l| l.seq.to_number()));
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        });
        if head.is_some() && !ms.is_empty() {
            chain.extend(ms.into_iter().map(|t| t.drawing));
        }
    }
    let hf_page = matches!(**outer, EditContext::HeaderFooter { .. }).then_some(*page);
    Some((chain, hf_page))
}

/// The textbox runs of `story` in `forEachTextbox` order (fulldoc.ts 441: each box, then its content; cells;
/// group items) — the TS `contextTextboxRuns`, uo-docx `loc::textbox_paths` order.
fn textbox_order<'a>(story: &'a uo_docx::model::story::Story, out: &mut Vec<&'a uo_docx::model::run::DrawingTextbox>, depth: u32) {
    if depth > 64 {
        return;
    }
    fn runs<'a>(rs: &'a [uo_docx::model::run::Run], out: &mut Vec<&'a uo_docx::model::run::DrawingTextbox>, depth: u32) {
        for r in rs {
            let Some(e) = r.ext() else { continue };
            if let Some(tb) = e.textbox.as_deref() {
                out.push(tb);
                textbox_order(&tb.content, out, depth + 1);
            }
            if let Some(g) = e.group.as_deref() {
                for it in &g.items {
                    runs(std::slice::from_ref(&it.run), out, depth + 1);
                }
            }
        }
    }
    for b in story.blocks() {
        match b {
            Block::Paragraph(p) => runs(&p.runs, out, depth),
            Block::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        textbox_order(&c.content, out, depth + 1);
                    }
                }
            }
        }
    }
}

/// `(para, run)` of the top-level paragraph run of `story` carrying the box `id` (or the head of story id
/// `head_story`).
fn top_run_of(story: &uo_docx::model::story::Story, want: impl Fn(&uo_docx::model::run::DrawingTextbox) -> bool) -> Option<(u32, u32)> {
    for (pi, p) in story.paras.iter().enumerate() {
        for (ri, r) in p.runs.iter().enumerate() {
            if r.ext().and_then(|e| e.textbox.as_deref()).is_some_and(&want) {
                return Some((u32_of(pi), u32_of(ri)));
            }
        }
    }
    None
}

/// `hitTextbox(e)` (13338) on `page`: the text box (with content) under `(x, y)` among the boxes of the
/// story outside the box (the body, or the header/footer part in edit: the TS `mine` =
/// `contextTextboxRuns`, boxes in cells, groups and boxes included), turned with its shape, `tol` twips
/// of border outside (5 px of the screen: the shell passes it), the top one winning (front layer first,
/// then `relativeHeight`). JSON `{ para, run, host, focus, page, rect: { x, y, w, h }, rot, onBorder }`:
/// `host`/`focus` the order of the chain's head and of the hit box among the story's boxes (the session's
/// `enterTextboxAt { focus }`), `para`/`run` the head when it is a top-level run of the outer story (null
/// otherwise; the old `enterTextbox`), `onBorder` whether the point is in the band of `tol` on both sides
/// of the outline (`onObjectBorder` 13673: the TS selects the shape there, frontier O's selection; the shell
/// does not enter the box). Status 1 when no box is there. Wave 6 (C): ED-102 closed (the first cut kept the
/// boxes of the outer story's top-level paragraphs only).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_textbox_at(
    h: *const UoWriteSession,
    layout: *const UoLayout,
    page: u32,
    x: f64,
    y: f64,
    tol: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let session = &s.session;
    let doc = session.document();
    let view = session.view();
    let ctx = session.context();
    let outer: &EditContext = match ctx {
        EditContext::Textbox { outer, .. } => outer,
        c => c,
    };
    let (in_hf, hf_page) = match outer {
        EditContext::HeaderFooter { page, .. } => (true, Some(*page)),
        _ => (false, None),
    };
    if hf_page.is_some_and(|p| p != page) || !x.is_finite() || !y.is_finite() {
        return buf(NONE, Vec::new());
    }
    let tol = if tol.is_finite() { tol } else { 0.0 };
    let Some(outer_story) = (match outer {
        EditContext::HeaderFooter { part, .. } => doc.hf.get(part.index()).map(|p| &p.story),
        _ => Some(&doc.body),
    }) else {
        return buf(NONE, Vec::new());
    };
    let geom = layout_geom(l);
    let Some(p) = geom.pages.get(page as usize) else { return buf(NONE, Vec::new()) };
    // the TS `mine`: the boxes of the outer story (`contextTextboxRuns`), by drawing
    let mut order = Vec::new();
    textbox_order(outer_story, &mut order, 0);
    let mut best: Option<(&uo_docx_layout::output::PlacedObject, &uo_docx::model::run::DrawingTextbox, f64, bool)> = None;
    let mut best_z = f64::NEG_INFINITY;
    for o in p.objects.as_deref().unwrap_or(&[]) {
        let Some(rr) = o.run.as_ref() else { continue };
        if o.content.is_none() || o.hf != in_hf {
            continue;
        }
        let Some(tb) = view.run(rr).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()) else { continue };
        if !order.iter().any(|t| t.drawing == tb.drawing) {
            continue;
        }
        let rot = tb.rot.map_or(0.0, uo_docx::js::JsNumber::to_number) * std::f64::consts::PI / 180.0;
        let (cx, cy) = (o.x + o.w / 2.0, o.y + o.h / 2.0);
        let (dx, dy) = (x - cx, y - cy);
        let (c, sn) = (uo_jsnum::math::cos(rot), uo_jsnum::math::sin(rot));
        let lx = dx * c + dy * sn;
        let ly = -dx * sn + dy * c;
        let (ax, ay) = (uo_jsnum::math::abs(lx), uo_jsnum::math::abs(ly));
        if !(ax <= o.w / 2.0 + tol && ay <= o.h / 2.0 + tol) {
            continue;
        }
        let z = (if o.layer == uo_docx_layout::output::Layer::Front { 1_099_511_627_776.0 } else { 0.0 })
            + tb.anchor.as_deref().and_then(|a| a.relative_height).map_or(0.0, uo_docx::js::JsNumber::to_number);
        if z >= best_z {
            best_z = z;
            // `onObjectBorder` (13673): the band of `tol` on BOTH sides of the outline (inside it too)
            best = Some((o, tb, rot, ax >= o.w / 2.0 - tol || ay >= o.h / 2.0 - tol));
        }
    }
    let Some((o, tb, rot, on_border)) = best else { return buf(NONE, Vec::new()) };
    // the head of the chain (`textboxChain(tb)[0]`: the box whose story the link names; the session edits its
    // content) and the orders of `contextTextboxRuns`
    let focus = order.iter().position(|t| t.drawing == tb.drawing).unwrap_or(0);
    let head = tb
        .link
        .as_ref()
        .map(|l| l.id.to_number())
        .and_then(|sid| order.iter().position(|t| t.link.is_none() && t.story.is_some_and(|v| v.to_number() == sid)))
        .unwrap_or(focus);
    let head_drawing = order.get(head).map(|t| t.drawing);
    let top = head_drawing.and_then(|d| top_run_of(outer_story, |t| t.drawing == d));
    let mut j = Js::new();
    j.open('{');
    for (k, v) in [("para", top.map(|t| t.0)), ("run", top.map(|t| t.1))] {
        j.key(k);
        match v {
            Some(v) => j.int(v),
            None => j.null(),
        }
    }
    for (k, v) in [("host", u32_of(head)), ("focus", u32_of(focus)), ("page", page)] {
        j.key(k);
        j.int(v);
    }
    j.key("rect");
    j.open('{');
    for (k, v) in [("x", o.x), ("y", o.y), ("w", o.w), ("h", o.h)] {
        j.key(k);
        j.num(v);
    }
    j.close('}');
    j.key("rot");
    j.num(rot * 180.0 / std::f64::consts::PI);
    j.key("onBorder");
    j.boolean(on_border);
    j.key("hf");
    j.boolean(in_hf);
    j.close('}');
    j.done()
}

// ───────────────────────── Sort and Convert Text to Table (`sortInfo` 19765, `textToTableGuess` 21257) ─────────────────────────

/// `guessSortType(texts)` (universal-write.ts 795).
fn guess_sort_type(texts: &[Vec<u16>]) -> &'static str {
    use uo_docx::session::ops::tables::sort::{parse_sort_date, parse_sort_number};
    let xs: Vec<Vec<u16>> = texts
        .iter()
        .map(|t| uo_jsnum::text::trim(&String::from_utf16_lossy(t)).encode_utf16().collect::<Vec<u16>>())
        .filter(|t| !t.is_empty())
        .collect();
    if xs.is_empty() {
        return "text";
    }
    if xs.iter().all(|t| parse_sort_date(t).is_some()) {
        return "date";
    }
    if xs.iter().all(|t| parse_sort_number(t).is_some()) {
        return "number";
    }
    "text"
}

/// `s.split(ch)` on UTF-16 units.
fn split_units(s: &[u16], ch: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if !ch.is_empty() && s.get(i..i + ch.len()) == Some(ch) {
            out.push(std::mem::take(&mut cur));
            i += ch.len();
        } else {
            cur.push(s.get(i).copied().unwrap_or(0));
            i += 1;
        }
    }
    out.push(cur);
    out
}

/// The selection's range, start first.
fn ordered(s: &uo_docx::session::Session) -> (DocPosition, DocPosition) {
    let r = s.range();
    if (r.start.para, r.start.offset) <= (r.end.para, r.end.offset) { (r.start, r.end) } else { (r.end, r.start) }
}

/// `sortInfo(separator)` (19765) as JSON `{ kind: 'table' | 'paragraphs', columns: [{ text, guess }], header,
/// mergeError }`; `sep` the field separator of paragraphs (UTF-8, consumed; empty = tab). Status 1 = null
/// (locked or no paragraphs).
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_sort_info(h: *const UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let sep = unsafe { crate::render::take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let session = &s.session;
    let Some(paras) = session.paras() else { return buf(NONE, Vec::new()) };
    if session.locked() {
        return buf(NONE, Vec::new());
    }
    let mut j = Js::new();
    j.open('{');
    if let Some(tg) = table_target(s) {
        use uo_docx::session::ops::tables::formula::cell_text;
        let t = tg.table;
        let m = table_ops::map_table(t);
        let (r0, r1) = if tg.cells { (tg.rect.r1, tg.rect.r2) } else { (0, i64::try_from(t.rows.len()).unwrap_or(0) - 1) };
        let header = usize::try_from(r0)
            .ok()
            .and_then(|r| t.rows.get(r))
            .and_then(|r| r.tr_pr.as_deref())
            .and_then(|p| p.tbl_header)
            .unwrap_or(false);
        let text_of = |g: Option<table_ops::GridCell>| -> Vec<u16> {
            g.and_then(|g| t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci))).map(cell_text).unwrap_or_default()
        };
        j.key("kind");
        j.string("table");
        j.key("columns");
        j.open('[');
        let mut c = 0i64;
        while c < m.n_cols {
            let g = table_ops::cell_at(&m, r0, c);
            let text = match g {
                Some(g) if g.col == c => uo_jsnum::text::trim(&String::from_utf16_lossy(&text_of(Some(g)))).to_owned(),
                _ => String::new(),
            };
            let mut below = Vec::new();
            let mut r = r0 + 1;
            while r <= r1 {
                below.push(text_of(table_ops::cell_at(&m, r, c)));
                r += 1;
            }
            j.open('{');
            j.key("text");
            j.string(&text);
            j.key("guess");
            j.string(guess_sort_type(&below));
            j.close('}');
            c += 1;
        }
        j.close(']');
        j.key("header");
        j.boolean(header);
        j.key("mergeError");
        j.boolean(!uo_docx::session::ops::tables::sort::can_sort_table(t));
    } else {
        let (a, b) = ordered(session);
        let has_sel = a != b;
        let texts: Vec<Vec<u16>> = paras
            .iter()
            .enumerate()
            .filter(|(i, _)| !has_sel || (*i >= a.para as usize && *i <= b.para as usize))
            .map(|(_, p)| uo_docx::session::text::para_text(p))
            .collect();
        let ch: Vec<u16> = if sep.is_empty() { vec![9] } else { String::from_utf8_lossy(&sep).encode_utf16().collect() };
        let fields: Vec<Vec<Vec<u16>>> = texts.iter().map(|t| split_units(t, &ch)).collect();
        let nf = fields.iter().map(Vec::len).fold(1, usize::max);
        j.key("kind");
        j.string("paragraphs");
        j.key("header");
        j.boolean(false);
        j.key("mergeError");
        j.boolean(false);
        j.key("columns");
        j.open('[');
        if nf > 1 {
            for k in 0..nf {
                let col: Vec<Vec<u16>> = fields.iter().skip(1).map(|f| f.get(k).cloned().unwrap_or_default()).collect();
                j.open('{');
                j.key("text");
                j.string("");
                j.key("guess");
                j.string(guess_sort_type(&col));
                j.close('}');
            }
        } else {
            j.open('{');
            j.key("text");
            j.string("");
            j.key("guess");
            j.string(guess_sort_type(&texts));
            j.close('}');
        }
        j.close(']');
    }
    j.close('}');
    j.done()
}

/// `textToTableGuess(separator?, cols?)` (21257) as JSON `{ separator, cols, rows }` (`separator` the TS
/// `TextSeparator`). `sep` (UTF-8, consumed): empty = guess, else `paragraph`, `tab`, `semicolon`, `comma`
/// or `char:` and the text; `cols` below 1 or NaN = none. Status 1 = null (a cell, header/footer or text box
/// context, no paragraphs).
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_text_to_table_guess(h: *const UoWriteSession, ptr: *mut u8, len: usize, cols: f64) -> *mut UoBuf {
    use table_ops::TextSeparator as Ts;
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let sep = unsafe { crate::render::take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let session = &s.session;
    if !matches!(session.context(), EditContext::Body) {
        return buf(NONE, Vec::new());
    }
    let Some(paras) = session.paras() else { return buf(NONE, Vec::new()) };
    let (a, b) = ordered(session);
    let p2 = if b.offset == 0 && b.para > a.para { b.para - 1 } else { b.para };
    let list: Vec<&uo_docx::model::para::Paragraph> =
        paras.iter().enumerate().filter(|(i, _)| *i >= a.para as usize && *i <= p2 as usize).map(|(_, p)| p).collect();
    if list.is_empty() {
        return buf(NONE, Vec::new());
    }
    let given = String::from_utf8_lossy(&sep).into_owned();
    let sep = match given.as_str() {
        "" => table_ops::guess_separator(&list),
        "paragraph" => Ts::Paragraph,
        "tab" => Ts::Tab,
        "semicolon" => Ts::Semicolon,
        "comma" => Ts::Comma,
        other => Ts::Char(other.strip_prefix("char:").unwrap_or(other).encode_utf16().collect()),
    };
    let n = f(list.len());
    let c = match sep {
        Ts::Paragraph => uo_jsnum::math::max(1.0, if cols.is_nan() || cols < 1.0 { 1.0 } else { cols }),
        _ => table_ops::columns_for(&list, &sep),
    };
    let mut j = Js::new();
    j.open('{');
    j.key("separator");
    match &sep {
        Ts::Paragraph => j.string("paragraph"),
        Ts::Tab => j.string("tab"),
        Ts::Semicolon => j.string("semicolon"),
        Ts::Comma => j.string("comma"),
        Ts::Char(u) => {
            j.open('{');
            j.key("char");
            j.string(&String::from_utf16_lossy(u));
            j.close('}');
        }
    }
    j.key("cols");
    j.num(c);
    j.key("rows");
    j.num(if matches!(sep, Ts::Paragraph) { uo_jsnum::math::ceil(n / c) } else { n });
    j.close('}');
    j.done()
}

/// `dragCellSel(start, ev)` (21013) with the cell being edited as the start: the point over another cell of
/// the same table selects the rectangle of cells (normalized over the merges, the start as its origin);
/// back over the start cell, the selection of cells goes. An `ExecResult` when the selection of cells
/// changed, status 1 when nothing did (the shell answers the TS boolean from the context:
/// `!!tableSel`).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_drag_cell_sel(h: *mut UoWriteSession, layout: *const UoLayout, page: u32, x: f64, y: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let EditContext::Cell { anchor, hops, .. } = s.session.context().clone() else { return buf(NONE, Vec::new()) };
    let Some((me, path)) = hops.split_last() else { return buf(NONE, Vec::new()) };
    let has_sel = s.session.table_selection().is_some();
    let cmd = {
        let doc = s.session.document();
        let geom = layout_geom(l);
        let Some(p) = geom.pages.get(page as usize) else { return buf(NONE, Vec::new()) };
        let mut best: Option<(u32, Vec<CellHop>)> = None;
        for b in p.cells.as_deref().unwrap_or(&[]) {
            if x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h || b.at.story != LineStory::Body {
                continue;
            }
            let mut segs: Vec<u32> = b.at.path.0.to_vec();
            segs.push(0);
            let Some((a, hh, _)) = cell_ref(doc, &segs) else { continue };
            if best.as_ref().is_none_or(|(_, bh)| hh.len() > bh.len()) {
                best = Some((a, hh));
            }
        }
        let Some((ha, hh)) = best else { return buf(NONE, Vec::new()) };
        let Some((hl, hp)) = hh.split_last() else { return buf(NONE, Vec::new()) };
        // `sameHops([...hit.path, {0,0}], [...path, {0,0}])`: the paths, every block compared
        let same_path = hp.len() == path.len() && hp.iter().zip(path).all(|(a, b)| a.row == b.row && a.col == b.col && a.block == b.block);
        if ha != anchor || !same_path {
            return buf(NONE, Vec::new());
        }
        if hl.row == me.row && hl.col == me.col {
            if !has_sel {
                return buf(NONE, Vec::new());
            }
            Command::Table(TableCmd::ClearTableSelection)
        } else {
            let Some(t) = table_at(doc, anchor, path) else { return buf(NONE, Vec::new()) };
            let m = table_ops::map_table(t);
            let (mr, mc, hr, hc) = (i64::from(me.row), i64::from(me.col), i64::from(hl.row), i64::from(hl.col));
            let rect = table_ops::norm_rect(t, &m, CellRect { r1: mr.min(hr), c1: mc.min(hc), r2: mr.max(hr), c2: mc.max(hc) });
            Command::Table(TableCmd::SetTableSelection { anchor, path: path.to_vec(), rect, origin: (mr, mc) })
        }
    };
    let mut steps = Steps::new();
    steps.run(s, cmd);
    steps.answer(s)
}

// ───────────────────────── wave 7 (A2): the cells of a page, the rows of a table ─────────────────────────

/// Every located body cell box of `page` (JSON `[{ anchor, path, r, c, x, y, w, h, span }]`): what the TS
/// `tableEdgeAt` and `cellBoxAtPoint` (universal-write.ts 20667, 20945) loop over (`locateRef(box.ref + '/b0')`).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_page_cells(h: *const UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let geom = layout_geom(l);
    let doc = s.session.document();
    let mut j = Js::new();
    j.open('[');
    if let Some(p) = geom.pages.get(page as usize) {
        for b in p.cells.as_deref().unwrap_or(&[]) {
            if b.at.story != LineStory::Body {
                continue;
            }
            let mut segs: Vec<u32> = b.at.path.0.to_vec();
            segs.push(0);
            let Some((anchor, hops, _)) = cell_ref(doc, &segs) else { continue };
            let Some((last, path)) = hops.split_last() else { continue };
            j.open('{');
            j.key("anchor");
            j.int(anchor);
            j.key("path");
            write_hops(&mut j, path);
            j.key("r");
            j.int(last.row);
            j.key("c");
            j.int(last.col);
            for (k, v) in [("x", b.x), ("y", b.y), ("w", b.w), ("h", b.h), ("span", b.span)] {
                j.key(k);
                j.num(v);
            }
            j.close('}');
        }
    }
    j.close(']');
    j.done()
}

/// The placed boxes and the displayed row heights of the table at `anchor` and the hops `path_json`
/// (`[[row, col, block|-1]…]`): JSON `{ rowHeights: [...], boxes: [{ pg, r, c, x, y, w, h }] }` (`tableBoxes`,
/// `rowHeights` of the TS; every row of the table).
///
/// # Safety
/// `h`, `layout` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_rows(
    h: *const UoWriteSession,
    layout: *const UoLayout,
    anchor: u32,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { crate::render::take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout_ref(layout) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(path) = parse_hops(&text) else { return buf(super::handle::BAD_ARGUMENT, b"path".to_vec()) };
    let doc = s.session.document();
    let Some(t) = table_at(doc, anchor, &path) else { return buf(NONE, Vec::new()) };
    let boxes = table_boxes(doc, layout_geom(l), anchor, &path);
    let n = i64::try_from(t.rows.len()).unwrap_or(0);
    let mut j = Js::new();
    j.open('{');
    j.key("rowHeights");
    j.open('[');
    for v in row_heights(&boxes, 0, n - 1) {
        j.num(v);
    }
    j.close(']');
    j.key("boxes");
    j.open('[');
    for b in &boxes {
        j.open('{');
        j.key("pg");
        j.int(b.pg);
        j.key("r");
        j.int(b.r);
        j.key("c");
        j.int(b.c);
        for (k, v) in [("x", b.x), ("y", b.y), ("w", b.w), ("h", b.h)] {
            j.key(k);
            j.num(v);
        }
        j.close('}');
    }
    j.close(']');
    j.close('}');
    j.done()
}

/// `[[row, col, block|-1]…]` (the hops of the wave-7 exports).
fn parse_hops(text: &str) -> Option<Vec<CellHop>> {
    let v = uo_docx::read::json_parse::parse(text)?;
    let uo_docx::js::JsonValue::Array(a) = v else { return None };
    let u = |x: &uo_docx::js::JsonValue| -> Option<Option<u32>> {
        match x {
            uo_docx::js::JsonValue::Num(n) if n.is_finite() && n.fract() == 0.0 && *n < 0.0 => Some(None),
            uo_docx::js::JsonValue::Num(n) if n.is_finite() && n.fract() == 0.0 && *n <= 4_294_967_295.0 => {
                n.to_string().parse::<u32>().ok().map(Some)
            }
            _ => None,
        }
    };
    a.iter()
        .map(|h| {
            let uo_docx::js::JsonValue::Array(t) = h else { return None };
            let row = u(t.first()?)??;
            let col = u(t.get(1)?)??;
            let block = match t.get(2) {
                Some(b) => u(b)?,
                None => None,
            };
            Some(CellHop { row, col, block })
        })
        .collect()
}
