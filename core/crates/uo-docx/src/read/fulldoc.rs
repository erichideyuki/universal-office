//! Frontier R5 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `fulldoc.ts`: tables,
//! rows, cells (`unwrapRaw`, `readTable`), blocks, header/footer parts, notes, comments, sections,
//! `linkVmlTextboxes`, `readFullDocument` ([`read_document`]), `readCoreProps`, `readSectionProps`
//! and `readBodyFragment`. The declarations (the nine kinds) are in
//! `core/docs/modules/uo-docx-d1a/R5.md`.
//!
//! Shape of the port:
//!
//! - the TS `Block[]` of a container is a [`Story`] (A2): blocks are pushed in document order
//!   through a [`BlockSink`] (a story, or a list of [`OwnedBlock`]s when the caller needs the blocks
//!   one child produced, for the source groups);
//! - the TS closures of the full-document read (`onBookmark`, `readHfSet`, `closeSection`) are the
//!   local state of [`read_document`] (the bookmark marks are queued by the paragraph reader and
//!   drained after each body child, before its blocks are pushed: `blocks.length` then is the
//!   TS value); a header/footer part read in the middle of the body has its own [`ReadContext`]
//!   (`buildReadContext` per part) over the same [`ReadCtx`], with the part counts put aside;
//! - `readBlocksOf`, `unwrap`, `unwrapRaw`, `readBlocksWithSections`, `forEachTextbox` are
//!   loops over explicit stacks; the recursion across stories (cell → table → cell, text box
//!   content) is bounded by `ReadCtx::enter_nested` (G20): a cell or text box past `max_nesting`
//!   is read as plain text paragraphs in place;
//! - TS reads the part texts with `getPartText` (`undefined` when absent, `''` when empty: both
//!   make `readHeaderFooterPart`, notes, comments and core props give nothing); a part that is
//!   absent or empty is `None` here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uo_jsnum::{conv, parse, text as jstext};
use uo_opc::Package;
use uo_opc::xml::{Document as XmlDocument, Element};

use crate::error::DocxError;
use crate::ids::{DrawingId, HfPartId, SourceKey, TxbxSrc};
use crate::js::{JsMap, JsNumber, JsObject, JsRecord, NumKey, Str};
use crate::model::background::PageBackground;
use crate::model::document::{BookmarkEnd, BookmarkStart, Bookmarks, Comment, CoreProps, NoteSpecialIds, Notes};
use crate::model::media::{ImageData, MediaRef};
use crate::model::para::{BlockSdt, BorderEdge, SdtKey};
use crate::model::run::{DrawingTextbox, Run, TextboxLink, TextboxView};
use crate::model::section::{ColDef, DocGrid, HfPart, HfRef, LnNumType, PageBorders, Section, SectionProps};
use crate::model::sources::{SourceStore, SrcText, StoryGroup, StorySource, WrapId, WrapSeg};
use crate::model::story::{Blocks, OwnedBlock};
use crate::model::table::{Cell, CellView, Row, Table, TableBorders};
use crate::model::{Block, Document, Paragraph, Story};
use crate::pvec::PVec;
use crate::text::Utf16;

use super::ReadOptions;
use super::ctx::ReadCtx;
use super::matchers::element_text;
use super::page_background::read_page_background;
use super::paragraph::{BlocksMode, ReadContext};
use super::parts::{part_doc, read_doc_formatting_facts};
use super::plain::{default_run_props, plain_paragraphs_with};
use super::props::{read_border, read_tbl_pr, read_tbl_pr_word, read_tc_pr, read_tr_pr};
use super::props_map::{read_border_edge, read_sect_pr};
use super::sources::{cell_source, el_span, group_of, note_item, note_story_of, notes_part_of, row_source, span, story_of, table_source};
use super::styles::read_styles;

type Result<T> = std::result::Result<T, DocxError>;

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `x || 0`.
fn or0(x: f64) -> f64 {
    if conv::truthy(x) { x } else { 0.0 }
}

/// `v.attr` as a non-empty string (the TS truthiness of an optional attribute).
fn truthy(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty())
}

/// `x` when the key is already recorded, else the key goes last (see [`append_key`]).
fn append_key<T: JsObject>(o: &mut T, key: &str) {
    let names: Vec<&'static str> = o.slots_in_order().into_iter().map(|(k, _)| k).collect();
    for k in names {
        o.mark(k);
    }
    o.mark(key);
}

/// The key goes last when the object's insertion order is recorded (an object in declaration order
/// already has it there).
fn mark_if_recorded<T: JsObject>(o: &mut T, key: &str) {
    if !o.key_order().is_empty() {
        o.mark(key);
    }
}

// ───────────────────────── blocks ─────────────────────────

/// Where the blocks of a container go, in document order.
pub trait BlockSink {
    fn push_paragraph(&mut self, p: Paragraph);
    fn push_table(&mut self, t: Arc<Table>);
}

impl BlockSink for Story {
    fn push_paragraph(&mut self, p: Paragraph) {
        Story::push_paragraph(self, p);
    }
    fn push_table(&mut self, t: Arc<Table>) {
        Story::push_table(self, t);
    }
}

impl BlockSink for Vec<OwnedBlock> {
    fn push_paragraph(&mut self, p: Paragraph) {
        self.push(OwnedBlock::Paragraph(p));
    }
    fn push_table(&mut self, t: Arc<Table>) {
        self.push(OwnedBlock::Table(t));
    }
}

/// `blockSrc(b) ?? 0`: the `src` of a block (paragraph: `props.src`; table: `src`).
#[must_use]
pub fn block_src(b: &OwnedBlock) -> SourceKey {
    match b {
        OwnedBlock::Paragraph(p) => p.props.src,
        OwnedBlock::Table(t) => t.src,
    }
    .unwrap_or(SourceKey(0))
}

fn kids_of<'d>(el: Element<'d>) -> std::vec::IntoIter<Element<'d>> {
    el.child_elements().collect::<Vec<_>>().into_iter()
}

/// The blocks of the given children, in order, through `w:sdt`/`w:customXml` (the body of the
/// loop of `readBlocksOf`). Iterative: the wrappers nest to the XML depth.
fn read_children_into<'d, S: BlockSink>(rc: &mut ReadContext<'_, '_>, kids: Vec<Element<'d>>, out: &mut S) -> Result<()> {
    let mut stack = vec![kids.into_iter()];
    while let Some(top) = stack.last_mut() {
        let Some(c) = top.next() else {
            stack.pop();
            continue;
        };
        match c.name() {
            "w:p" => out.push_paragraph(rc.read_paragraph(c)?),
            "w:tbl" => out.push_table(Arc::new(read_table(rc, c)?)),
            "w:sdt" => {
                if let Some(content) = c.first_child("w:sdtContent") {
                    stack.push(kids_of(content));
                }
            }
            "w:customXml" => stack.push(kids_of(c)),
            _ => {}
        }
    }
    Ok(())
}

/// `readBlocksOf(container, ctx)`: the blocks (`w:p` / `w:tbl`) of a container, in order, through
/// `w:sdt`/`w:customXml`.
pub fn read_blocks_of<S: BlockSink>(rc: &mut ReadContext<'_, '_>, container: Element<'_>, out: &mut S) -> Result<()> {
    read_children_into(rc, container.child_elements().collect(), out)
}

/// The part of the source store the element belongs to when the read keeps sources (`ctx.sources`
/// and `el.s !== undefined`, then `ctx.sourceXml`).
fn source_part_of(rc: &ReadContext<'_, '_>, el: Element<'_>) -> Option<u32> {
    rc.source_part().filter(|_| rc.ctx.sources.is_some() && el.positions().is_some())
}

fn ids_of(blocks: &[OwnedBlock]) -> Vec<SourceKey> {
    blocks.iter().map(block_src).collect()
}

fn push_all<S: BlockSink>(out: &mut S, blocks: Vec<OwnedBlock>) {
    for b in blocks {
        match b {
            OwnedBlock::Paragraph(p) => out.push_paragraph(p),
            OwnedBlock::Table(t) => out.push_table(t),
        }
    }
}

/// `readBlocksGrouped(container, ctx)`: the blocks of a container and, without loss, its children as
/// groups when there is something besides `w:p`/`w:tbl` (block `w:sdt`/`w:customXml`) and the read
/// keeps sources.
pub fn read_blocks_grouped(rc: &mut ReadContext<'_, '_>, container: Element<'_>) -> Result<(Story, Option<Vec<StoryGroup>>)> {
    let mut story = Story::new();
    let part = source_part_of(rc, container);
    let kids: Vec<Element<'_>> = if part.is_some() { container.child_elements().collect() } else { Vec::new() };
    let Some(part) = part.filter(|_| kids.iter().any(|k| k.name() != "w:p" && k.name() != "w:tbl")) else {
        read_blocks_of(rc, container, &mut story)?;
        return Ok((story, None));
    };
    let mut groups = Vec::with_capacity(kids.len());
    for k in kids {
        let mut produced: Vec<OwnedBlock> = Vec::new();
        read_children_into(rc, vec![k], &mut produced)?;
        if let Some(store) = rc.ctx.sources.as_ref() {
            groups.push(group_of(store, part, k, &ids_of(&produced)));
        }
        push_all(&mut story, produced);
    }
    Ok((story, Some(groups)))
}

/// `textboxBlocks` over the block readers of the context: the content of a `w:txbxContent` as a
/// story and, with sources and groups, the key of the groups in the store
/// (`ctx.sources.addTextboxGroups`). `grouped`: `readBlocksGrouped` is set (else only `readBlocks`).
pub fn read_textbox_content(rc: &mut ReadContext<'_, '_>, content: Element<'_>, grouped: bool) -> Result<(Story, Option<TxbxSrc>)> {
    if !grouped {
        let mut story = Story::new();
        read_blocks_of(rc, content, &mut story)?;
        return Ok((story, None));
    }
    let (story, groups) = read_blocks_grouped(rc, content)?;
    let txbx_src = match (groups, rc.ctx.sources.as_mut()) {
        (Some(g), Some(store)) => Some(store.add_textbox_groups(g)),
        _ => None,
    };
    Ok((story, txbx_src))
}

// ───────────────────────── tables ─────────────────────────

/// `readBorderSet(el, inside)`: `w:tblBorders`/`w:tcBorders` in editing form, `None` without any edge.
fn read_border_set(el: Option<Element<'_>>, inside: bool) -> Option<Box<TableBorders>> {
    let el = el?;
    let mut out = TableBorders::default();
    let mut any = false;
    let mut edge = |name: &str, slot: &mut Option<BorderEdge>| {
        if let Some(e) = read_border_edge(el.first_child(name)) {
            *slot = Some(e);
            any = true;
        }
    };
    edge("w:top", &mut out.top);
    edge("w:left", &mut out.left);
    edge("w:bottom", &mut out.bottom);
    edge("w:right", &mut out.right);
    if inside {
        edge("w:insideH", &mut out.inside_h);
        edge("w:insideV", &mut out.inside_v);
    }
    any.then_some(Box::new(out))
}

/// `unwrap(el, name)`: the rows or cells below an element, through `w:sdt`/`w:customXml`.
fn unwrap_plain<'d>(el: Element<'d>, name: &str) -> Vec<Element<'d>> {
    let mut out = Vec::new();
    let mut stack = vec![kids_of(el)];
    while let Some(top) = stack.last_mut() {
        let Some(c) = top.next() else {
            stack.pop();
            continue;
        };
        if c.name() == name {
            out.push(c);
        } else if c.name() == "w:sdt" {
            if let Some(content) = c.first_child("w:sdtContent") {
                stack.push(kids_of(content));
            }
        } else if c.name() == "w:customXml" {
            stack.push(kids_of(c));
        }
    }
    out
}

/// A row or cell found through `w:sdt`/`w:customXml` and the raw pieces around it (`Wrapped`).
struct Wrapped<'d> {
    el: Element<'d>,
    pre: Vec<WrapSeg>,
    post: Vec<WrapSeg>,
}

fn wrap_seg(raw: SrcText) -> WrapSeg {
    WrapSeg { raw, ..WrapSeg::default() }
}

/// One `walk` of `unwrapRaw` being run.
struct WrapFrame<'d> {
    kids: std::vec::IntoIter<Element<'d>>,
    /// the container is a `w:customXml` (its `*Pr` children are the wrapper's)
    custom: bool,
    top: bool,
    /// what the wrapper this frame walks the content of does when the frame is done
    resume: Option<WrapResume>,
}

struct WrapResume {
    id: WrapId,
    before: Vec<WrapSeg>,
    n0: usize,
    whole: SrcText,
    close: SrcText,
}

/// `unwrapRaw(el, name, skip, xml)`: as [`unwrap_plain`], keeping the raw text around each element:
/// the loose marks before it and the openings and closings of the wrappers (`WrapSeg`), whose
/// ids come from the per-read counter (`wrapIds`, G7). Iterative.
fn unwrap_raw<'d>(rc: &mut ReadContext<'_, '_>, part: u32, el: Element<'d>, name: &str, skip: &[&str]) -> Vec<Wrapped<'d>> {
    let mut out: Vec<Wrapped<'d>> = Vec::new();
    let mut pending: Vec<WrapSeg> = Vec::new();
    let mut stack = vec![WrapFrame { kids: kids_of(el), custom: el.name() == "w:customXml", top: true, resume: None }];
    while let Some((top, custom)) = stack.last().map(|f| (f.top, f.custom)) {
        let next = stack.last_mut().and_then(|f| f.kids.next());
        let Some(c) = next else {
            let Some(done) = stack.pop() else { break };
            if let Some(r) = done.resume {
                if out.len() > r.n0 {
                    let mut tail = std::mem::take(&mut pending);
                    tail.push(WrapSeg { close: Some(r.id), ..wrap_seg(r.close) });
                    if let Some(last) = out.last_mut() {
                        last.post.extend(tail);
                    }
                } else {
                    pending = r.before;
                    pending.push(wrap_seg(r.whole));
                }
            }
            continue;
        };
        if c.name() == name {
            out.push(Wrapped { el: c, pre: std::mem::take(&mut pending), post: Vec::new() });
            continue;
        }
        let skipped = if top { skip.contains(&c.name()) } else { custom && c.name().ends_with("Pr") };
        if skipped {
            continue;
        }
        let content = match c.name() {
            "w:sdt" => c.first_child("w:sdtContent"),
            "w:customXml" => Some(c),
            _ => None,
        };
        let wrapper = match (content, content.and_then(|x| x.positions()), c.positions()) {
            (Some(content), Some(cp), Some(cs)) if cp.content_start != cp.end => Some((content, cp, cs)),
            _ => None,
        };
        if let Some((content, cp, cs)) = wrapper {
            let mut start = cp.content_start;
            if content == c {
                for k in c.child_elements() {
                    if k.name().ends_with("Pr") {
                        start = k.positions().map_or(start, |p| p.end);
                    } else {
                        break;
                    }
                }
            }
            let id = rc.ctx.next_wrap_id();
            let before = std::mem::take(&mut pending);
            let n0 = out.len();
            pending = before.clone();
            pending.push(WrapSeg { open: Some(id), ..wrap_seg(span(part, cs.start, start)) });
            stack.push(WrapFrame {
                kids: kids_of(content),
                custom: content.name() == "w:customXml",
                top: false,
                resume: Some(WrapResume { id, before, n0, whole: span(part, cs.start, cs.end), close: span(part, cp.content_end, cs.end) }),
            });
            continue;
        }
        pending.push(wrap_seg(el_span(part, c)));
    }
    if !pending.is_empty()
        && let Some(last) = out.last_mut()
    {
        last.post.extend(pending);
    }
    out
}

/// `wrapped(parent, name, skip)` of `readTable`.
fn wrapped_of<'d>(rc: &mut ReadContext<'_, '_>, part: Option<u32>, parent: Element<'d>, name: &str, skip: &[&str]) -> Vec<Wrapped<'d>> {
    match part {
        Some(part) => unwrap_raw(rc, part, parent, name, skip),
        None => unwrap_plain(parent, name).into_iter().map(|el| Wrapped { el, pre: Vec::new(), post: Vec::new() }).collect(),
    }
}

/// The content of a cell (`readBlocksOf(tcEl)`, or the children one by one as groups when there are
/// block wrappers or loose marks between the paragraphs and the read keeps sources), and the groups.
fn read_cell_content(rc: &mut ReadContext<'_, '_>, part: Option<u32>, tc_el: Element<'_>) -> Result<(Story, Option<Vec<StoryGroup>>)> {
    let mut story = Story::new();
    let kids: Vec<Element<'_>> =
        if part.is_some() { tc_el.child_elements().filter(|k| k.name() != "w:tcPr").collect() } else { Vec::new() };
    let mut groups = None;
    match part.filter(|_| kids.iter().any(|k| k.name() != "w:p" && k.name() != "w:tbl")) {
        Some(part) => {
            let mut g: Vec<StoryGroup> = Vec::with_capacity(kids.len());
            for k in kids {
                let mut produced: Vec<OwnedBlock> = Vec::new();
                read_children_into(rc, vec![k], &mut produced)?;
                if let Some(store) = rc.ctx.sources.as_ref() {
                    g.push(group_of(store, part, k, &ids_of(&produced)));
                }
                push_all(&mut story, produced);
            }
            groups = Some(g);
        }
        None => read_blocks_of(rc, tc_el, &mut story)?,
    }
    if !story.has_paragraph() {
        story.push_paragraph(Paragraph::empty(rc.ctx.ids.para()));
    }
    Ok((story, groups))
}

/// The content of a cell past `max_nesting` (G20): plain text paragraphs in place, counted against
/// the part's `max_paragraphs_per_part` and `max_runs_per_part` like the paragraphs the paragraph
/// reader reads (the paragraphs are counted before they are built), so that content buried below
/// the nesting limit cannot pass the per-part limits.
#[inline(never)]
fn plain_cell_content(ctx: &mut ReadCtx<'_>, tc_el: Element<'_>) -> Result<Story> {
    for _ in std::iter::once(tc_el).chain(tc_el.descendants()).filter(|e| e.name() == "w:p") {
        ctx.count_paragraph()?;
    }
    let mut story = Story::new();
    let props = ctx.intern(default_run_props());
    for p in plain_paragraphs_with(tc_el, &mut ctx.ids, &props) {
        if !p.runs.is_empty() {
            ctx.count_run()?;
        }
        story.push_paragraph(p);
    }
    if !story.has_paragraph() {
        story.push_paragraph(Paragraph::empty(ctx.ids.para()));
    }
    Ok(story)
}

/// One cell of `readTable`.
#[inline(never)]
fn read_cell(rc: &mut ReadContext<'_, '_>, part: Option<u32>, tc_wr: Wrapped<'_>) -> Result<Cell> {
    let tc_el = tc_wr.el;
    let tc_pr_el = tc_el.first_child("w:tcPr");
    let tc_pr = read_tc_pr(tc_pr_el);
    let shd = tc_pr_el.and_then(|e| e.first_child("w:shd")).and_then(|e| e.attr("w:fill"));
    let tc_w = tc_pr_el.and_then(|e| e.first_child("w:tcW"));
    let cell_borders = read_border_set(tc_pr_el.and_then(|e| e.first_child("w:tcBorders")), true);
    let (content, groups) = if rc.ctx.enter_nested() {
        let read = read_cell_content(rc, part, tc_el);
        rc.ctx.leave_nested();
        read?
    } else {
        (plain_cell_content(rc.ctx, tc_el)?, None)
    };
    let grid_span = match tc_pr.as_ref().and_then(|t| t.grid_span) {
        Some(JsNumber::Num(x)) => JsNumber::Num(x),
        _ => JsNumber::Num(1.0),
    };
    let mut cell = Cell {
        grid_span,
        width_twips: tc_w
            .filter(|w| w.attr("w:type") == Some("dxa"))
            .map(|w| JsNumber::Num(parse::parse_int(w.attr("w:w").unwrap_or("0"), 10))),
        shading_hex: shd.filter(|s| !s.is_empty() && *s != "auto").map(Str::from),
        borders: cell_borders,
        tc_pr: tc_pr.map(|t| rc.ctx.intern(t)),
        content,
        view: CellView::Synced,
        ..Cell::default()
    };
    for (key, absent) in
        [("widthTwips", cell.width_twips.is_none()), ("shadingHex", cell.shading_hex.is_none()), ("borders", cell.borders.is_none())]
    {
        if absent {
            cell.mark_undefined(key);
        }
    }
    if let (Some(part), Some(store)) = (part, rc.ctx.sources.as_mut()) {
        let src = cell_source(store, part, tc_el, tc_pr_el, groups, tc_wr.pre, tc_wr.post);
        store.add_cell(&mut cell, src);
    }
    Ok(cell)
}

/// `readTable(tblEl, ctx)`.
#[inline(never)]
pub fn read_table(rc: &mut ReadContext<'_, '_>, tbl_el: Element<'_>) -> Result<Table> {
    let part = source_part_of(rc, tbl_el);
    let grid = tbl_el.first_child("w:tblGrid");
    let grid_cols_twips: Vec<JsNumber> = grid
        .map(|g| g.children_named("w:gridCol").map(|c| JsNumber::Num(or0(parse::parse_int(c.attr("w:w").unwrap_or("0"), 10)))).collect())
        .unwrap_or_default();
    let tbl_pr_el = tbl_el.first_child("w:tblPr");
    let tbl_borders = read_border_set(tbl_pr_el.and_then(|e| e.first_child("w:tblBorders")), true);
    let mut rows: Vec<Row> = Vec::new();
    for tr_w in wrapped_of(rc, part, tbl_el, "w:tr", &["w:tblPr", "w:tblGrid"]) {
        let tr_el = tr_w.el;
        let mut cells: Vec<Cell> = Vec::new();
        for tc_wr in wrapped_of(rc, part, tr_el, "w:tc", &["w:trPr", "w:tblPrEx"]) {
            cells.push(read_cell(rc, part, tc_wr)?);
        }
        if cells.is_empty() {
            continue;
        }
        cells.shrink_to_fit();
        let tr_pr_el = tr_el.first_child("w:trPr");
        let ex_el = tr_el.first_child("w:tblPrEx");
        let mut row = Row {
            cells,
            tr_pr: read_tr_pr(tr_pr_el).map(|t| rc.ctx.intern(t)),
            tbl_pr_ex: read_tbl_pr(ex_el).map(|t| rc.ctx.intern(t)),
            ..Row::default()
        };
        if let (Some(part), Some(store)) = (part, rc.ctx.sources.as_mut()) {
            let src = row_source(store, part, tr_el, tr_pr_el, ex_el, tr_w.pre, tr_w.post);
            store.add_row(&mut row, src);
        }
        rows.push(row);
    }
    rows.shrink_to_fit();
    let mut table = Table {
        rows,
        grid_cols_twips,
        borders: tbl_borders,
        tbl_pr: read_tbl_pr(tbl_pr_el).map(|t| rc.ctx.intern(t)),
        id: rc.ctx.ids.table(),
        // WordCraft study (tables, the TCond aside): Word's view of an out-of-order tblStyle
        word_tbl_pr: read_tbl_pr_word(tbl_pr_el).map(std::sync::Arc::new),
        ..Table::default()
    };
    if table.borders.is_none() {
        table.mark_undefined("borders");
    }
    if let (Some(part), Some(store)) = (part, rc.ctx.sources.as_mut()) {
        let src = table_source(store, part, tbl_el, tbl_pr_el, grid);
        store.add_table(&mut table, src);
    }
    Ok(table)
}

// ───────────────────────── block content controls ─────────────────────────

/// `docPartGallery(sdt)`: the `w:docPartGallery` of a block `w:sdt` with `w:docPartObj` (`""`
/// without a gallery), `None` when it is not a `docPartObj`.
fn doc_part_gallery(sdt: Element<'_>) -> Option<&str> {
    let obj = sdt.first_child("w:sdtPr")?.first_child("w:docPartObj")?;
    Some(obj.first_child("w:docPartGallery").and_then(|g| g.attr("w:val")).unwrap_or(""))
}

/// `markBlockSdt(child, g, produced)`: the `ParaProps.blockSdt` mark on the paragraphs of a block
/// `w:sdt` with `w:docPartObj` and, on the editor path, the key in its source group.
fn mark_block_sdt(rc: &mut ReadContext<'_, '_>, child: Element<'_>, g: Option<&mut StoryGroup>, produced: &mut [OwnedBlock]) {
    let Some(gallery) = (child.name() == "w:sdt").then(|| doc_part_gallery(child)).flatten() else { return };
    let key: SdtKey = rc.ctx.next_block_sdt_key();
    let sdt = Arc::new(BlockSdt { key: key.clone(), gallery: (!gallery.is_empty()).then(|| gallery.into()), ..BlockSdt::default() });
    if let Some(g) = g {
        g.sdt_key = Some(key);
    }
    for b in produced {
        if let OwnedBlock::Paragraph(p) = b {
            let props = Arc::make_mut(&mut p.props);
            props.block_sdt = Some(Arc::clone(&sdt));
            mark_if_recorded(props, "blockSdt");
            rc.ctx.intern_shared(&mut p.props);
        }
    }
}

// ───────────────────────── VML text box chains ─────────────────────────

/// A text box of the blocks in `forEachTextbox` order, with what the chain linking reads.
struct TbEntry {
    drawing: DrawingId,
    vml_id: Option<Str>,
    vml_next: Option<Str>,
    story: Option<f64>,
    /// the index after the last entry inside the content of this text box
    content_end: usize,
}

/// One level of the walk of [`collect_textboxes`].
enum TbFrame<'a> {
    Blocks(Blocks<'a>),
    Runs(std::slice::Iter<'a, Run>),
    Items(std::slice::Iter<'a, crate::model::run::GroupItem>),
    Close(usize),
}

/// `forEachTextbox(blocks, fn)`: every text box of the blocks (top-level paragraphs, cells, group
/// items and boxes in boxes), in document order, each with the extent of its content. Iterative.
fn collect_textboxes(story: &Story) -> Vec<TbEntry> {
    let mut out: Vec<TbEntry> = Vec::new();
    let mut stack = vec![TbFrame::Blocks(story.blocks())];
    while let Some(top) = stack.last_mut() {
        match top {
            TbFrame::Blocks(it) => match it.next() {
                None => {
                    stack.pop();
                }
                Some(Block::Paragraph(p)) => stack.push(TbFrame::Runs(p.runs.iter())),
                Some(Block::Table(t)) => {
                    for row in t.rows.iter().rev() {
                        for cell in row.cells.iter().rev() {
                            stack.push(TbFrame::Blocks(cell.content.blocks()));
                        }
                    }
                }
            },
            TbFrame::Items(it) => match it.next() {
                None => {
                    stack.pop();
                }
                Some(item) => stack.push(TbFrame::Runs(std::slice::from_ref(&item.run).iter())),
            },
            TbFrame::Close(i) => {
                let i = *i;
                stack.pop();
                let end = out.len();
                if let Some(e) = out.get_mut(i) {
                    e.content_end = end;
                }
            }
            TbFrame::Runs(it) => {
                let Some(r) = it.next() else {
                    stack.pop();
                    continue;
                };
                let Some(ext) = r.ext() else { continue };
                if let Some(group) = ext.group.as_deref() {
                    stack.push(TbFrame::Items(group.items.iter()));
                }
                if let Some(tb) = ext.textbox.as_deref() {
                    let i = out.len();
                    out.push(TbEntry {
                        drawing: tb.drawing,
                        vml_id: tb.vml_id.clone().filter(|s| !s.is_empty()),
                        vml_next: tb.vml_next.clone().filter(|s| !s.is_empty()),
                        story: tb.story.and_then(|s| match s {
                            JsNumber::Num(x) => Some(x),
                            JsNumber::Null => None,
                        }),
                        content_end: i + 1,
                    });
                    stack.push(TbFrame::Close(i));
                    stack.push(TbFrame::Blocks(tb.content.blocks()));
                }
            }
        }
    }
    out
}

/// What `linkVmlTextboxes` does to one text box.
#[derive(Default)]
struct TbMutation {
    story: Option<f64>,
    link: Option<(f64, f64)>,
    clear: bool,
}

fn apply_to_textbox(tb: &mut DrawingTextbox, muts: &HashMap<DrawingId, TbMutation>) {
    if let Some(m) = muts.get(&tb.drawing) {
        if let Some(id) = m.story {
            tb.story = Some(JsNumber::Num(id));
            append_key(tb, "story");
        }
        if let Some((id, seq)) = m.link {
            tb.link = Some(TextboxLink { id: JsNumber::Num(id), seq: JsNumber::Num(seq), ..TextboxLink::default() });
            append_key(tb, "link");
        }
        if m.clear {
            tb.content = Story::new();
            tb.view = TextboxView::Synced;
        }
    }
    apply_to_story(&mut tb.content, muts);
}

fn apply_to_runs(runs: &mut [Run], muts: &HashMap<DrawingId, TbMutation>) {
    for r in runs {
        let Some(ext) = r.ext.as_mut() else { continue };
        if let Some(tb) = ext.textbox.as_mut() {
            apply_to_textbox(tb, muts);
        }
        if let Some(g) = ext.group.as_mut() {
            for it in &mut g.items {
                apply_to_runs(std::slice::from_mut(&mut it.run), muts);
            }
        }
    }
}

/// Walks a story mutably (recursion through cells and text boxes, bounded by `max_nesting`: the
/// reader reads nothing deeper as a story).
fn apply_to_story(story: &mut Story, muts: &HashMap<DrawingId, TbMutation>) {
    for i in 0..story.paras.len() {
        if let Some(p) = story.paras.make_mut(i) {
            apply_to_runs(&mut p.runs, muts);
        }
    }
    for i in 0..story.tables.len() {
        if let Some(a) = story.tables.make_mut(i) {
            let table = Arc::make_mut(&mut a.table);
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    apply_to_story(&mut cell.content, muts);
                }
            }
        }
    }
}

/// `linkVmlTextboxes(blocks)`: the chains of VML text boxes (`v:textbox style="mso-next-textbox:#id"`)
/// become the DrawingML shape: the first box of a chain gets `story` and the next ones `link`
/// `{ id, seq }` (without content). Story ids are free from the largest `wps:txbx@id` of the part.
pub fn link_vml_textboxes(story: &mut Story) {
    let entries = collect_textboxes(story);
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut targets: HashSet<&str> = HashSet::new();
    let mut max_story = 0.0_f64;
    for (i, e) in entries.iter().enumerate() {
        if let Some(id) = e.vml_id.as_deref() {
            by_id.insert(id, i);
        }
        if let Some(next) = e.vml_next.as_deref() {
            targets.insert(next);
        }
        if let Some(s) = e.story
            && s > max_story
        {
            max_story = s;
        }
    }
    if targets.is_empty() {
        return;
    }
    let mut muts: HashMap<DrawingId, TbMutation> = HashMap::new();
    let mut cleared: HashSet<DrawingId> = HashSet::new();
    let mut i = 0;
    while let Some(e) = entries.get(i) {
        i += 1;
        let head = e.vml_next.as_deref().is_some_and(|_| !e.vml_id.as_deref().is_some_and(|id| targets.contains(id)) && e.story.is_none());
        if head && let Some(next) = e.vml_next.as_deref() {
            max_story += 1.0;
            muts.entry(e.drawing).or_default().story = Some(max_story);
            let mut seq = 0.0_f64;
            let mut seen: HashSet<DrawingId> = HashSet::from([e.drawing]);
            let mut cur = by_id.get(next).and_then(|&k| entries.get(k));
            while let Some(c) = cur
                && !seen.contains(&c.drawing)
            {
                seen.insert(c.drawing);
                seq += 1.0;
                let m = muts.entry(c.drawing).or_default();
                m.link = Some((max_story, seq));
                m.clear = true;
                cleared.insert(c.drawing);
                cur = c.vml_next.as_deref().and_then(|n| by_id.get(n)).and_then(|&k| entries.get(k));
            }
        }
        if cleared.contains(&e.drawing) {
            i = e.content_end.max(i);
        }
    }
    apply_to_story(story, &muts);
}

// ───────────────────────── header and footer parts ─────────────────────────

/// A part that is absent or empty is no part (`if (!xml) return undefined`).
fn part_doc_pos(ctx: &ReadCtx<'_>, name: &str) -> Result<Option<XmlDocument>> {
    if ctx.pkg.entry(name).is_none_or(|e| e.size <= 3)
        && let Some(text) = ctx.pkg.part_text(name)?
        && text.is_empty()
    {
        return Ok(None);
    }
    ctx.xml_part(name)
}

/// The name of the part's relationships (`word/_rels/${partName}.rels`).
fn rels_of(path: &str) -> String {
    format!("word/_rels/{}.rels", path.rsplit('/').next().unwrap_or(path))
}

/// `readHeaderFooterPart(pkg, path, store, xml0)`: the story of a header/footer part, read as a
/// part of the document: with sources (`ctx.sources`) its pieces and story are kept. `xml0`: the
/// content in place of the part's (a new part of the editor, the gallery's template), read as if it
/// came from the file; the relationships are those of `path`. `None` for an absent or empty part.
pub fn read_header_footer_part(ctx: &mut ReadCtx<'_>, path: &str, xml0: Option<&str>) -> Result<Option<Story>> {
    let doc = match xml0 {
        Some("") => None,
        Some(text) => Some(ctx.xml_text(text)?),
        None => part_doc_pos(ctx, path)?,
    };
    let Some(doc) = doc else { return Ok(None) };
    let saved = ctx.suspend_part();
    ctx.begin_part(path);
    let read = read_part_story(ctx, path, &doc);
    ctx.resume_part(saved);
    let mut story = read?;
    link_vml_textboxes(&mut story);
    Ok(Some(story))
}

fn read_part_story(ctx: &mut ReadCtx<'_>, path: &str, doc: &XmlDocument) -> Result<Story> {
    let part = ctx.source_part(path, doc);
    let mut rc = ReadContext::new(ctx, &rels_of(path))?;
    rc.blocks = BlocksMode::Grouped;
    let root = doc.root();
    let mut story = Story::new();
    let Some(part) = part else {
        read_blocks_of(&mut rc, root, &mut story)?;
        return Ok(story);
    };
    rc.set_source_part(Some(part));
    let mut src = match rc.ctx.sources.as_ref() {
        Some(store) => story_of(store, part, root),
        None => StorySource::default(),
    };
    for c in root.child_elements() {
        let mut produced: Vec<OwnedBlock> = Vec::new();
        read_children_into(&mut rc, vec![c], &mut produced)?;
        if let Some(store) = rc.ctx.sources.as_ref() {
            src.groups.push(group_of(store, part, c, &ids_of(&produced)));
        }
        push_all(&mut story, produced);
    }
    if let Some(store) = rc.ctx.sources.as_mut() {
        store.stories.insert(path.into(), src);
    }
    Ok(story)
}

/// `readBodyFragment(pkg, store, inner, ns)`: a fragment of the body (the children of `w:body`) read as
/// the body is: with sources (`ctx.sources`), its blocks and groups (the `w:sdt` of a cover page) go to
/// the story of `document.xml` and come out raw at the save. `ns`: the namespace declarations of the
/// fragment. `None` when the fragment has no `w:body`.
pub fn read_body_fragment(ctx: &mut ReadCtx<'_>, inner: &str, ns: &str) -> Result<Option<(Story, Vec<StoryGroup>)>> {
    let xml =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:document {ns}><w:body>{inner}</w:body></w:document>");
    let doc = ctx.xml_text(&xml)?;
    let Some(body) = doc.root().first_child("w:body") else { return Ok(None) };
    let part = ctx.source_part("word/document.xml", &doc);
    let saved = ctx.suspend_part();
    ctx.begin_part("word/document.xml");
    let read = read_fragment_blocks(ctx, body, part);
    ctx.resume_part(saved);
    read.map(Some)
}

fn read_fragment_blocks(ctx: &mut ReadCtx<'_>, body: Element<'_>, part: Option<u32>) -> Result<(Story, Vec<StoryGroup>)> {
    let mut rc = ReadContext::new(ctx, "word/_rels/document.xml.rels")?;
    rc.blocks = BlocksMode::Blocks;
    let mut story = Story::new();
    let Some(part) = part else {
        read_blocks_of(&mut rc, body, &mut story)?;
        return Ok((story, Vec::new()));
    };
    rc.set_source_part(Some(part));
    let mut groups = Vec::new();
    for c in body.child_elements() {
        let mut produced: Vec<OwnedBlock> = Vec::new();
        read_children_into(&mut rc, vec![c], &mut produced)?;
        let mut g = match rc.ctx.sources.as_ref() {
            Some(store) => group_of(store, part, c, &ids_of(&produced)),
            None => StoryGroup::default(),
        };
        mark_block_sdt(&mut rc, c, Some(&mut g), &mut produced);
        groups.push(g);
        push_all(&mut story, produced);
    }
    Ok((story, groups))
}

// ───────────────────────── notes, comments, core props ─────────────────────────

/// What a notes part gives: one story per note id (TS `Map` order) and the ids of the special notes
/// (separators, continuation notice).
struct NotesRead {
    notes: JsMap<NumKey, Story>,
    special: Vec<JsNumber>,
}

/// `readNotesPart(pkg, path, elementName, blocksOut, store, special)`.
fn read_notes_part(ctx: &mut ReadCtx<'_>, path: &str, element_name: &str) -> Result<NotesRead> {
    let mut out = NotesRead { notes: JsMap::default(), special: Vec::new() };
    let Some(doc) = part_doc_pos(ctx, path)? else { return Ok(out) };
    ctx.begin_part(path);
    let part = ctx.source_part(path, &doc);
    let mut rc = ReadContext::new(ctx, &rels_of(path))?;
    rc.blocks = BlocksMode::Grouped;
    rc.set_source_part(part);
    let root = doc.root();
    let mut notes_src = match (part, rc.ctx.sources.as_ref()) {
        (Some(part), Some(store)) => Some(notes_part_of(store, part, root)),
        _ => None,
    };
    for note in root.children_named(element_name) {
        let id = parse::parse_int(note.attr("w:id").unwrap_or("0"), 10);
        if matches!(note.attr("w:type"), Some("separator" | "continuationSeparator" | "continuationNotice")) {
            out.special.push(JsNumber::Num(id));
            if let (Some(part), Some(src)) = (part, notes_src.as_mut()) {
                src.items.push(note_item(part, note, JsNumber::Num(id), None));
            }
            continue;
        }
        let mut story = Story::new();
        match part {
            None => read_blocks_of(&mut rc, note, &mut story)?,
            Some(part) => {
                let mut ss = note_story_of(part, note);
                for c in note.child_elements() {
                    let mut produced: Vec<OwnedBlock> = Vec::new();
                    read_children_into(&mut rc, vec![c], &mut produced)?;
                    if let Some(store) = rc.ctx.sources.as_ref() {
                        ss.groups.push(group_of(store, part, c, &ids_of(&produced)));
                    }
                    push_all(&mut story, produced);
                }
                if let Some(src) = notes_src.as_mut() {
                    src.items.push(note_item(part, note, JsNumber::Num(id), Some(ss)));
                }
            }
        }
        out.notes.insert(NumKey::new(id), story);
    }
    if let (Some(src), Some(store)) = (notes_src, rc.ctx.sources.as_mut()) {
        store.note_parts.insert(path.into(), src);
    }
    Ok(out)
}

/// `noteRefIds(blocks, kind)`: the ids of the notes referenced in the blocks (body and cells, in
/// document order, without repeats).
#[must_use]
pub fn note_ref_ids(story: &Story, kind: &str) -> Vec<NumKey> {
    let mut out: indexmap::IndexSet<NumKey> = indexmap::IndexSet::new();
    for p in story.flat_paragraphs() {
        for r in &p.runs {
            if let Some(n) = r.ext().and_then(|e| e.note_ref.as_ref())
                && &*n.kind == kind
                && let JsNumber::Num(id) = n.id
            {
                out.insert(NumKey::new(id));
            }
        }
    }
    out.into_iter().collect()
}

/// `readComments(pkg)`: the text of each `w:comment` (its paragraphs' run texts, joined with `\n`).
fn read_comments(ctx: &mut ReadCtx<'_>) -> Result<Vec<Comment>> {
    let Some(doc) = part_doc(ctx, "word/comments.xml")? else { return Ok(Vec::new()) };
    ctx.begin_part("word/comments.xml");
    let mut rc = ReadContext::new(ctx, "word/_rels/comments.xml.rels")?;
    let mut out = Vec::new();
    for c in doc.root().children_named("w:comment") {
        let mut text: Vec<u16> = Vec::new();
        for (i, p) in c.children_named("w:p").enumerate() {
            if i > 0 {
                text.push(10);
            }
            for r in &rc.read_paragraph(p)?.runs {
                text.extend_from_slice(r.text.units());
            }
        }
        let author = c.attr("w:author").map(Str::from);
        let mut comment = Comment {
            id: JsNumber::Num(parse::parse_int(c.attr("w:id").unwrap_or("0"), 10)),
            author,
            text: Utf16::from_units(text),
            ..Comment::default()
        };
        if comment.author.is_none() {
            comment.mark_undefined("author");
        }
        out.push(comment);
    }
    Ok(out)
}

/// `readCoreProps(pkg)`: the dates of `docProps/core.xml` (creation, last save, last print);
/// `None` without the part (`{}`), `Some` with no key for a part that has none of them.
pub fn read_core_props(ctx: &ReadCtx<'_>) -> Result<Option<CoreProps>> {
    let Some(text) = ctx.pkg.part_text("docProps/core.xml")? else { return Ok(None) };
    if text.is_empty() {
        return Ok(None);
    }
    let get = |tag: &str| element_text(&text, tag).map(jstext::trim).filter(|s| !s.is_empty()).map(Str::from);
    Ok(Some(CoreProps {
        created: get("dcterms:created"),
        modified: get("dcterms:modified"),
        last_printed: get("cp:lastPrinted"),
        ..CoreProps::default()
    }))
}

// ───────────────────────── sections ─────────────────────────

/// `readSectionProps(sectEl)`: the `w:sectPr` beyond the basic geometry (`None` reads as an empty
/// `w:sectPr`).
#[must_use]
pub fn read_section_props(sect_el: Option<Element<'_>>) -> SectionProps {
    let child = |n: &str| sect_el.and_then(|e| e.first_child(n));
    let val = |n: &str| child(n).and_then(|c| c.attr("w:val"));
    let mut out = SectionProps { r#type: val("w:type").unwrap_or("nextPage").into(), ..SectionProps::default() };
    if let Some(orient @ ("landscape" | "portrait")) = child("w:pgSz").and_then(|e| e.attr("w:orient")) {
        out.orient = Some(orient.into());
    }
    if let Some(gutter) = truthy(child("w:pgMar").and_then(|e| e.attr("w:gutter"))) {
        out.gutter = Some(JsNumber::Num(or0(parse::parse_int(gutter, 10))));
    }
    if let Some(cols) = child("w:cols") {
        if let Some(eq) = cols.attr("w:equalWidth") {
            out.equal_width = Some(!(eq == "0" || eq == "false"));
        }
        let defs: Vec<ColDef> = cols
            .children_named("w:col")
            .map(|c| ColDef {
                w: JsNumber::Num(or0(parse::parse_int(c.attr("w:w").unwrap_or("0"), 10))),
                space: JsNumber::Num(or0(parse::parse_int(c.attr("w:space").unwrap_or("0"), 10))),
                ..ColDef::default()
            })
            .collect();
        if !defs.is_empty() {
            out.col_defs = Some(defs);
        }
    }
    if let Some(va) = truthy(val("w:vAlign")) {
        out.v_align = Some(va.into());
    }
    if let Some(dg) = child("w:docGrid") {
        let mut g = DocGrid::default();
        if let Some(t) = truthy(dg.attr("w:type")) {
            g.r#type = Some(t.into());
        }
        if let Some(v) = truthy(dg.attr("w:linePitch")) {
            g.line_pitch = Some(JsNumber::Num(parse::parse_int(v, 10)));
        }
        if let Some(v) = truthy(dg.attr("w:charSpace")) {
            g.char_space = Some(JsNumber::Num(parse::parse_int(v, 10)));
        }
        out.doc_grid = Some(g);
    }
    if let Some(pb) = child("w:pgBorders") {
        let mut b = PageBorders::default();
        if let Some(v) = truthy(pb.attr("w:offsetFrom")) {
            b.offset_from = Some(v.into());
        }
        if let Some(v) = truthy(pb.attr("w:display")) {
            b.display = Some(v.into());
        }
        if let Some(v) = truthy(pb.attr("w:zOrder")) {
            b.z_order = Some(v.into());
        }
        b.top = read_border(pb.first_child("w:top"));
        b.left = read_border(pb.first_child("w:left"));
        b.bottom = read_border(pb.first_child("w:bottom"));
        b.right = read_border(pb.first_child("w:right"));
        out.pg_borders = Some(Box::new(b));
    }
    if let Some(fmt) = truthy(child("w:pgNumType").and_then(|e| e.attr("w:fmt"))) {
        out.pg_num_fmt = Some(fmt.into());
    }
    if let Some(ln) = child("w:lnNumType") {
        let mut l = LnNumType::default();
        if let Some(v) = truthy(ln.attr("w:countBy")) {
            l.count_by = Some(JsNumber::Num(parse::parse_int(v, 10)));
        }
        if let Some(v) = truthy(ln.attr("w:start")) {
            l.start = Some(JsNumber::Num(parse::parse_int(v, 10)));
        }
        if let Some(v) = truthy(ln.attr("w:restart")) {
            l.restart = Some(v.into());
        }
        if let Some(v) = truthy(ln.attr("w:distance")) {
            l.distance = Some(JsNumber::Num(parse::parse_int(v, 10)));
        }
        out.ln_num_type = Some(l);
    }
    out.bidi = child("w:bidi").map(|_| true);
    out.rtl_gutter = child("w:rtlGutter").map(|_| true);
    out.form_prot = child("w:formProt").map(|_| true);
    out.no_endnote = child("w:noEndnote").map(|_| true);
    out
}

/// The header/footer state of the full-document read: the relationship targets of `document.xml`
/// by id, and each part once (`hfCache`).
struct HfState {
    /// rId → the part the relationship names (`None`: it names none)
    targets: HashMap<String, Option<Box<str>>>,
    /// the part path → what was read (`None`: absent or empty, read once)
    cache: HashMap<Box<str>, Option<HfPartId>>,
    parts: PVec<HfPart>,
}

impl HfState {
    fn new(ctx: &ReadCtx<'_>) -> Result<HfState> {
        let mut targets = HashMap::new();
        for rel in ctx.rels("word/_rels/document.xml.rels")? {
            if !rel.id.is_empty() && !rel.target.is_empty() {
                let part = ctx.resolve_target("word/document.xml", &rel.target).map(Box::from);
                targets.insert(rel.id, part);
            }
        }
        Ok(HfState { targets, cache: HashMap::new(), parts: PVec::new() })
    }

    /// `readHfSet(sectEl, kind)`: the slot → part map and the references of a section.
    fn read_set(&mut self, ctx: &mut ReadCtx<'_>, sect_el: Option<Element<'_>>, kind: &str) -> Result<(JsRecord<HfPartId>, Vec<HfRef>)> {
        let mut set = JsRecord::new();
        let mut refs = Vec::new();
        let Some(sect_el) = sect_el else { return Ok((set, refs)) };
        for r in sect_el.children_named(kind) {
            let Some(rid) = truthy(r.attr("r:id")) else { continue };
            let ty = r.attr("w:type").unwrap_or("default");
            let Some(Some(target)) = self.targets.get(rid).cloned() else { continue };
            if !self.cache.contains_key(&target) {
                let id = match read_header_footer_part(ctx, &target, None)? {
                    Some(story) => {
                        let id = ctx.ids.hf_part();
                        self.parts.push(HfPart { path: target.as_ref().into(), story, ..HfPart::default() });
                        Some(id)
                    }
                    None => None,
                };
                self.cache.insert(target.clone(), id);
            }
            if let Some(Some(id)) = self.cache.get(&target) {
                // `set['__proto__'] = part.paras` sets the prototype of the object: no own key
                if ty != "__proto__" {
                    set.insert(ty, *id);
                }
                refs.push(HfRef { r#type: ty.into(), rid: rid.into(), path: target.as_ref().into(), part: *id, ..HfRef::default() });
            }
        }
        Ok((set, refs))
    }
}

/// The sections of the body being read (`sections`, `sectionStart`, `closeSection`).
struct Sections {
    list: Vec<Section>,
    start: f64,
    hf: HfState,
}

impl Sections {
    /// `closeSection(sectEl, endBlock)`.
    fn close(&mut self, ctx: &mut ReadCtx<'_>, sect_el: Option<Element<'_>>, end_block: f64) -> Result<()> {
        let (headers, header_refs) = self.hf.read_set(ctx, sect_el, "w:headerReference")?;
        let (footers, footer_refs) = self.hf.read_set(ctx, sect_el, "w:footerReference")?;
        self.list.push(Section {
            sect_pr: Arc::new(read_sect_pr(sect_el)),
            sect: Some(Arc::new(read_section_props(sect_el))),
            block_start: JsNumber::Num(self.start),
            block_end: JsNumber::Num(end_block),
            header_refs,
            footer_refs,
            headers,
            footers,
            ..Section::default()
        });
        self.start = end_block + 1.0;
        Ok(())
    }
}

// ───────────────────────── the body ─────────────────────────

/// A block of the body and the `w:sectPr` of its paragraph (`readBlocksWithSections`).
type WithSection<'d> = (OwnedBlock, Option<Element<'d>>);

/// `readBlocksWithSections(container)`: the blocks of the content of a body-level `w:sdt` or
/// `w:customXml`, each with the section its paragraph closes. Iterative.
fn read_blocks_with_sections<'d>(rc: &mut ReadContext<'_, '_>, container: Element<'d>, out: &mut Vec<WithSection<'d>>) -> Result<()> {
    let mut stack = vec![kids_of(container)];
    while let Some(top) = stack.last_mut() {
        let Some(c) = top.next() else {
            stack.pop();
            continue;
        };
        match c.name() {
            "w:p" => {
                let sect = c.first_child("w:pPr").and_then(|p| p.first_child("w:sectPr"));
                out.push((OwnedBlock::Paragraph(rc.read_paragraph(c)?), sect));
            }
            "w:tbl" => out.push((OwnedBlock::Table(Arc::new(read_table(rc, c)?)), None)),
            "w:sdt" => {
                if let Some(content) = c.first_child("w:sdtContent") {
                    stack.push(kids_of(content));
                }
            }
            "w:customXml" => stack.push(kids_of(c)),
            _ => {}
        }
    }
    Ok(())
}

/// Resolves the images of the page background through the relationships of the body.
fn read_background(rc: &mut ReadContext<'_, '_>, root: Element<'_>) -> Result<Option<Arc<PageBackground>>> {
    let Some(bg) = root.first_child("w:background") else { return Ok(None) };
    let mut failed: Option<DocxError> = None;
    let out = read_page_background(bg, &mut |rid| match rc.resolve_image(rid) {
        Ok(Some(m)) => Some(ImageData { data: MediaRef(m.id), ext: m.ext, ..ImageData::default() }),
        Ok(None) => None,
        Err(e) => {
            failed.get_or_insert(e);
            None
        }
    });
    match failed {
        Some(e) => Err(e),
        None => Ok(out.map(Arc::new)),
    }
}

/// `readFullDocument(pkg, { sources })`: reads a docx package into a [`Document`] (A5).
pub fn read_document(pkg: &Package, opts: &ReadOptions) -> Result<Document> {
    let mut ctx = ReadCtx::new(pkg, opts);
    ctx.begin_part("word/document.xml");
    let doc = ctx.xml_part("word/document.xml")?.ok_or(DocxError::NotWordprocessing)?;
    let root = doc.root();
    let body = root.first_child("w:body").ok_or(DocxError::NotWordprocessing)?;
    let part = ctx.source_part("word/document.xml", &doc);
    let hf = HfState::new(&ctx)?;
    if let (Some(part), Some(store)) = (part, ctx.sources.as_mut()) {
        store.stories.insert("word/document.xml".into(), story_of_body(store, part, body));
    }
    let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels")?;
    rc.blocks = BlocksMode::Grouped;
    rc.collect_bookmarks = true;
    rc.set_source_part(part);

    let mut story = Story::new();
    let mut bookmarks = Bookmarks::default();
    let mut sections = Sections { list: Vec::new(), start: 0.0, hf };
    let mut body_src = part.and_then(|p| rc.ctx.sources.as_ref().map(|s| story_of_body(s, p, body)));
    let kids: Vec<Element<'_>> = body.child_elements().collect();
    for (index, child) in kids.iter().copied().enumerate() {
        let mut local: Vec<WithSection<'_>> = Vec::new();
        match child.name() {
            "w:p" => {
                let sect = child.first_child("w:pPr").and_then(|p| p.first_child("w:sectPr"));
                local.push((OwnedBlock::Paragraph(rc.read_paragraph(child)?), sect));
            }
            "w:tbl" => local.push((OwnedBlock::Table(Arc::new(read_table(&mut rc, child)?)), None)),
            "w:sdt" | "w:customXml" => {
                let content = if child.name() == "w:sdt" { child.first_child("w:sdtContent") } else { Some(child) };
                if let Some(content) = content {
                    read_blocks_with_sections(&mut rc, content, &mut local)?;
                }
            }
            _ => {}
        }
        let block_no = as_f64(story.block_count());
        for ev in std::mem::take(&mut rc.bookmark_events) {
            let offset = JsNumber::Num(as_f64(ev.offset));
            if ev.end {
                bookmarks.ends.entry(ev.name).or_insert(BookmarkEnd { block: JsNumber::Num(block_no), offset, ..BookmarkEnd::default() });
            } else {
                bookmarks.starts.entry(ev.name).or_insert(BookmarkStart { block: JsNumber::Num(block_no), offset });
            }
        }
        let (mut blocks, sects): (Vec<OwnedBlock>, Vec<Option<Element<'_>>>) = local.into_iter().unzip();
        let ids = if part.is_some() { ids_of(&blocks) } else { Vec::new() };
        let mut group = None;
        if let (Some(part), Some(store)) = (part, rc.ctx.sources.as_ref()) {
            group = Some(group_of(store, part, child, &ids));
        }
        mark_block_sdt(&mut rc, child, group.as_mut(), &mut blocks);
        for (b, sect) in blocks.into_iter().zip(sects) {
            match b {
                OwnedBlock::Paragraph(p) => story.push_paragraph(p),
                OwnedBlock::Table(t) => story.push_table(t),
            }
            if let Some(sect) = sect {
                sections.close(rc.ctx, Some(sect), as_f64(story.block_count()) - 1.0)?;
            }
        }
        if child.name() == "w:sectPr" {
            sections.close(rc.ctx, Some(child), as_f64(story.block_count()) - 1.0)?;
        }
        if let (Some(src), Some(part)) = (body_src.as_mut(), part) {
            if child.name() == "w:sectPr" && index + 1 == kids.len() {
                src.sect_pr = Some(el_span(part, child));
            } else if let Some(g) = group {
                src.groups.push(g);
            }
        }
    }
    if sections.list.is_empty() {
        sections.close(rc.ctx, body.first_child("w:sectPr"), as_f64(story.block_count()) - 1.0)?;
    }
    if story.block_count() == 0 {
        story.push_paragraph(Paragraph::empty(rc.ctx.ids.para()));
    }
    if let (Some(src), Some(store)) = (body_src, rc.ctx.sources.as_mut()) {
        store.stories.insert("word/document.xml".into(), src);
    }
    link_vml_textboxes(&mut story);

    let comments = read_comments(rc.ctx)?;
    if let Some(store) = rc.ctx.sources.as_mut() {
        store.record_comments(&comments);
    }
    let footnotes = read_notes_part(rc.ctx, "word/footnotes.xml", "w:footnote")?;
    let endnotes = read_notes_part(rc.ctx, "word/endnotes.xml", "w:endnote")?;
    if let Some(store) = rc.ctx.sources.as_mut() {
        for kind in ["footnote", "endnote"] {
            let ids = note_ref_ids(&story, kind);
            if let Some(p) = store.note_parts.get_mut(format!("word/{kind}s.xml").as_str()) {
                p.refs_at_load.0.extend(ids);
            }
        }
    }
    let styles = read_styles(rc.ctx)?;
    let (formatting, read_facts) = read_doc_formatting_facts(rc.ctx)?; // wave 7c (W, item 2)
    let core_props = read_core_props(rc.ctx)?;
    let background = read_background(&mut rc, root)?;
    let ctx = rc.ctx;
    Ok(Document {
        body: story,
        sections: sections.list.into(),
        hf: sections.hf.parts,
        notes: Arc::new(Notes {
            footnotes: footnotes.notes,
            endnotes: endnotes.notes,
            special_ids: NoteSpecialIds { footnote: footnotes.special, endnote: endnotes.special, ..NoteSpecialIds::default() },
            ..Notes::default()
        }),
        comments: comments.into(),
        styles: Arc::new(styles),
        bookmarks: Arc::new(bookmarks),
        formatting: Arc::new(formatting),
        core_props,
        background,
        media: std::mem::take(&mut ctx.media),
        sources: ctx.sources.take().map(Arc::new),
        ids: std::mem::take(&mut ctx.ids),
        formatting_version: 0,
        read_facts,
    })
}

/// `storyOf(xml, body)`.
fn story_of_body(store: &SourceStore, part: u32, body: Element<'_>) -> StorySource {
    story_of(store, part, body)
}
