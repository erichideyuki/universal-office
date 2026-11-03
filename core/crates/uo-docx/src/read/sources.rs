//! Frontier R6 (D1a reader, record "Milestone 1 — D1a foundation"): `source.ts` (reader side) and
//! `document.ts` `captureParagraph`: building the `SourceStore` during the read (signatures
//! excluded: the session takes them at install, G23).
//!
//! The TS slices the part's XML string by the parser's `s/cs/ce/e` positions; here the same
//! pieces are [`SrcText::Span`]s of UTF-8 offsets (`Element::positions`) into the part text the
//! XML reader parsed, registered in the store with `ReadCtx::source_part` (a shared handle, not
//! a copy). `part` below is that registration's index. Text the TS builds (`openOf`'s `/>` →
//! `>`, the `raw` of a field that joins several top-level children) is [`SrcText::Owned`]; two
//! adjacent spans join into one span.
//!
//! What the callers (R4c `readParagraph`, R4b objects, R5 `fulldoc`) use:
//!
//! - [`SourceStore::add_para`], [`SourceStore::add_table`], [`SourceStore::add_row`],
//!   [`SourceStore::add_cell`] (give the object its `src`), [`SourceStore::add_textbox_groups`],
//!   [`SourceStore::at_of`], [`SourceStore::record_comments`];
//! - paragraphs: [`PendingSeg`] (one per top-level child of the `w:p`), [`inline_wrapper`],
//!   [`SourceStore::concat`] (the `raw +=` of a field that joins children), [`RunSide`] (the
//!   TS symbol keys `RPR_EL`/`OBJ_EL` of a run) and [`capture_paragraph`];
//! - stories: [`story_of`], [`group_of`] (iterative), [`note_story_of`], [`notes_part_of`],
//!   [`note_item`];
//! - tables: [`table_source`], [`row_source`], [`cell_source`], [`open_of`].
//!
//! Positions are UTF-8 here and UTF-16 in the TS; every `at` and span is compared in UTF-8 by the
//! digest (F11). Elements without positions (the reader asked for none, or a synthetic element)
//! never reach these functions in the TS (`pEl.s !== undefined` guards the capture); here they
//! give an empty text.

use std::borrow::Cow;

use uo_opc::xml::Element;

use crate::ids::{SourceKey, TxbxSrc};
use crate::js::{JsNumber, JsObject, JsValue, JsonWriter, NumKey, Str};
use crate::model::sources::{
    CellSource, NoteItemSource, NotesPartSource, NumSet, ParaSource, RowSource, RunSource, SegSource, SourceStore, SrcText, StoryGroup,
    StorySource, TableSource, WrapSeg,
};
use crate::model::table::{Cell, Row, Table};
use crate::model::{Comment, Paragraph};

use super::matchers::self_closing_to_open;

/// `INLINE_WRAPPERS` of `document.ts`: the top-level children of a `w:p` that wrap runs.
pub const INLINE_WRAPPERS: [&str; 6] = ["w:sdt", "w:hyperlink", "w:smartTag", "w:customXml", "w:dir", "w:bdo"];

/// Whether `name` is in [`INLINE_WRAPPERS`].
#[must_use]
pub fn is_inline_wrapper(name: &str) -> bool {
    INLINE_WRAPPERS.contains(&name)
}

/// A position of the XML reader as a store offset.
///
/// Saturating: positions come from the parser as `u32` (a part is limited far below 4 GiB), so
/// this never saturates; a saturated offset resolves to the empty text.
fn off(x: usize) -> u32 {
    u32::try_from(x).unwrap_or(u32::MAX)
}

/// `xml.slice(start, end)` of part `part`.
#[must_use]
pub fn span(part: u32, start: usize, end: usize) -> SrcText {
    SrcText::Span { part, start: off(start), end: off(end) }
}

/// `xml.slice(el.s, el.e)`: the whole element.
#[must_use]
pub fn el_span(part: u32, el: Element<'_>) -> SrcText {
    match el.positions() {
        Some(p) => span(part, p.start, p.end),
        None => SrcText::default(),
    }
}

/// [`el_span`] of an element that may be absent (`slice(el)` of `readTable`).
#[must_use]
pub fn opt_span(part: u32, el: Option<Element<'_>>) -> Option<SrcText> {
    el.map(|e| el_span(part, e))
}

/// The text of `part` as a cheap handle (the XML reader's own buffer).
fn part_text(store: &SourceStore, part: u32) -> Option<uo_opc::xml::Source> {
    store.parts.get(usize::try_from(part).ok()?).map(|p| p.text.clone())
}

/// `openOf` of `readTable`: `xml.slice(el.s, el.cs === el.e ? el.e : el.cs).replace(/\s*\/>$/, '>')`,
/// the start tag of the element made an open tag.
#[must_use]
pub fn open_of(store: &SourceStore, part: u32, el: Element<'_>) -> SrcText {
    let Some(p) = el.positions() else { return SrcText::default() };
    let end = if p.content_start == p.end { p.end } else { p.content_start };
    let Some(text) = part_text(store, part) else { return SrcText::default() };
    match text.get(p.start..end) {
        Some(tag) => match self_closing_to_open(tag) {
            Cow::Borrowed(_) => span(part, p.start, end),
            Cow::Owned(s) => SrcText::Owned(s.into()),
        },
        None => SrcText::default(),
    }
}

/// The wrapper pieces of a container child: `wrapOpen = xml.slice(child.s, start)` and
/// `wrapClose = xml.slice(content.ce, child.e)`, where `start` is the start of `content` (the
/// `w:sdtContent`, or the child itself, in which case the leading `*Pr` children stay in the
/// wrapper). `None` when `content` is empty (`cs === e`) or has no positions.
#[must_use]
pub fn wrapper_spans(part: u32, child: Element<'_>, content: Element<'_>) -> Option<(SrcText, SrcText)> {
    let c = content.positions()?;
    if c.content_start == c.end {
        return None;
    }
    let ch = child.positions()?;
    let mut start = c.content_start;
    if content == child {
        for k in child.child_elements() {
            if k.name().ends_with("Pr") {
                start = k.positions()?.end;
            } else {
                break;
            }
        }
    }
    Some((span(part, ch.start, start), span(part, c.content_end, ch.end)))
}

/// The wrapper of a top-level child of a `w:p` (`readParagraph`): `child` is in
/// [`INLINE_WRAPPERS`], not empty (`child.cs !== child.e`) and its content (the `w:sdtContent` of
/// a `w:sdt`, else the child) is not empty. The caller adds the `!openHere()` condition (no field
/// open across this child).
#[must_use]
pub fn inline_wrapper(part: u32, child: Element<'_>) -> Option<(SrcText, SrcText)> {
    if !is_inline_wrapper(child.name()) {
        return None;
    }
    let p = child.positions()?;
    if p.content_start == p.end {
        return None;
    }
    let content = if child.name() == "w:sdt" { child.first_child("w:sdtContent")? } else { child };
    wrapper_spans(part, child, content)
}

/// A top-level child of a `w:p` being captured: its [`SegSource`] and the model runs
/// `from..to` it produced (the `{ seg, from, to }` of `readParagraph`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingSeg {
    pub seg: SegSource,
    pub from: usize,
    pub to: usize,
}

impl PendingSeg {
    /// `{ seg: { raw, sids: [] }, from, to }` for `child`.
    #[must_use]
    pub fn new(part: u32, child: Element<'_>, from: usize, to: usize) -> PendingSeg {
        PendingSeg { seg: SegSource { raw: el_span(part, child), ..SegSource::default() }, from, to }
    }
}

/// The element a run keeps under the TS symbol `OBJ_EL`: the object element on the first run of
/// the object (`Head`), `true` on the other runs of the same object (`Follow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjSide {
    #[default]
    None,
    Head(ElSpan),
    Follow,
}

/// The `[s, e)` of an element (UTF-8 offsets into its part).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElSpan {
    pub start: u32,
    pub end: u32,
}

impl ElSpan {
    /// The element's span (`None` without positions).
    #[must_use]
    pub fn of(el: Element<'_>) -> Option<ElSpan> {
        let p = el.positions()?;
        Some(ElSpan { start: off(p.start), end: off(p.end) })
    }
}

/// What the TS keeps on a run under the symbol keys `RPR_EL` (the `w:rPr` it was read from) and
/// `OBJ_EL` (the object element): read-time only, not part of the model, so the reader carries
/// one per run next to the runs (index = run index) and hands the slice to [`capture_paragraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunSide {
    pub rpr: Option<ElSpan>,
    pub obj: ObjSide,
}

/// `key_order` bookkeeping of a key the TS appends to an existing object (`p.props.src = id`):
/// when the object's insertion order is recorded (data-dependent), the key goes last; an object
/// in declaration order already has `src` after every key assigned before it (F4).
fn mark_appended<T: JsObject>(obj: &mut T, key: &str) {
    if !obj.key_order().is_empty() {
        obj.mark(key);
    }
}

impl SourceStore {
    /// `addPara(p, src)`: stores the source and gives the paragraph its `props.src`.
    pub fn add_para(&mut self, para: &mut Paragraph, src: ParaSource) -> SourceKey {
        let id = self.id();
        self.paras.insert(id, src);
        let props = std::sync::Arc::make_mut(&mut para.props);
        props.src = Some(id);
        mark_appended(props, "src");
        id
    }

    /// `addTable(t, src)`.
    pub fn add_table(&mut self, table: &mut Table, src: TableSource) -> SourceKey {
        let id = self.id();
        self.tables.insert(id, src);
        table.src = Some(id);
        mark_appended(table, "src");
        id
    }

    /// `addRow(row, src)`.
    pub fn add_row(&mut self, row: &mut Row, src: RowSource) -> SourceKey {
        let id = self.id();
        self.trs.insert(id, src);
        row.src = Some(id);
        mark_appended(row, "src");
        id
    }

    /// `addCell(cell, src)`.
    pub fn add_cell(&mut self, cell: &mut Cell, src: CellSource) -> SourceKey {
        let id = self.id();
        self.tcs.insert(id, src);
        cell.src = Some(id);
        mark_appended(cell, "src");
        id
    }

    /// `addTextboxGroups(tb, groups)`: the groups of a textbox content, keyed by the returned
    /// [`TxbxSrc`] (which the textbox carries, in place of the TS `WeakMap` keyed by the object).
    pub fn add_textbox_groups(&mut self, groups: Vec<StoryGroup>) -> TxbxSrc {
        self.txbx_groups.push(groups);
        TxbxSrc(off(self.txbx_groups.len().saturating_sub(1)))
    }

    /// `atOf(id)`: the source position of a block (paragraph or table) by its `src`.
    #[must_use]
    pub fn at_of(&self, id: SourceKey) -> Option<u32> {
        self.paras.get(&id).and_then(|p| p.at).or_else(|| self.tables.get(&id).and_then(|t| t.at))
    }

    /// The `raw +=` of a field that joins several top-level children: one span when `a` and `b`
    /// are adjacent in the same part, else the built string.
    #[must_use]
    pub fn concat(&self, a: &SrcText, b: &SrcText) -> SrcText {
        if let (SrcText::Span { part: pa, start: sa, end: ea }, SrcText::Span { part: pb, start: sb, end: eb }) = (a, b)
            && pa == pb
            && ea == sb
        {
            return SrcText::Span { part: *pa, start: *sa, end: *eb };
        }
        let mut s = String::from(a.resolve(&self.parts));
        s.push_str(b.resolve(&self.parts));
        SrcText::Owned(s.into())
    }

    /// `for (const c of comments) store.commentIds.add(c.id); store.commentsSig =
    /// JSON.stringify(comments)` of `readFullDocument`.
    pub fn record_comments(&mut self, comments: &[Comment]) {
        let mut ids = NumSet::default();
        let mut w = JsonWriter::new(None);
        w.raw("[");
        for (i, c) in comments.iter().enumerate() {
            ids.0.insert(NumKey::new(c.id.to_number()));
            if i > 0 {
                w.raw(",");
            }
            c.json(&mut w);
        }
        w.raw("]");
        self.comment_ids = ids;
        self.comments_sig = Some(w.finish().into());
    }
}

/// `captureParagraph` (document.ts): gives every run of a top-level segment an id and a
/// [`RunSource`], completes the segments and registers the paragraph's [`ParaSource`].
///
/// `segs` are the top-level children of `p_el` in order with the runs they produced (`readParagraph`
/// builds them), `sides[i]` is run `i`'s [`RunSide`] (missing = default). Run ids come first, in
/// order, then the paragraph's (as in the TS).
pub fn capture_paragraph(
    store: &mut SourceStore,
    part: u32,
    para: &mut Paragraph,
    p_el: Element<'_>,
    ppr_el: Option<Element<'_>>,
    mut segs: Vec<PendingSeg>,
    sides: &[RunSide],
) {
    let run_count = para.runs.len();
    if let Some(last) = segs.last_mut()
        && last.to < run_count
    {
        last.to = run_count;
        last.seg.open = Some(true);
    }
    let mut obj_head = 0.0_f64;
    for pending in &mut segs {
        let seg = &mut pending.seg;
        if seg.wrap_open.is_some() && seg.raw.resolve(&store.parts).starts_with("<w:hyperlink") {
            let first = para.runs.get(pending.from);
            seg.link = Some(first.and_then(|r| r.props.link.clone()).unwrap_or_default());
            seg.tip = Some(first.and_then(|r| r.props.link_tip.clone()).unwrap_or_default());
        }
        for i in pending.from..pending.to {
            let Some(run) = para.runs.get_mut(i) else { break };
            let sid = store.id();
            let side = sides.get(i).copied().unwrap_or_default();
            let mut rs = RunSource::default();
            if let Some(el) = side.rpr {
                rs.r_pr = Some(SrcText::Span { part, start: el.start, end: el.end });
            }
            match side.obj {
                ObjSide::Follow => rs.obj_head = Some(JsNumber::Num(obj_head)),
                ObjSide::Head(el) => rs.obj = Some(SrcText::Span { part, start: el.start, end: el.end }),
                ObjSide::None => {}
            }
            store.runs.insert(sid, rs);
            if matches!(side.obj, ObjSide::Head(_)) {
                obj_head = f64::from(sid.0);
            }
            let props = std::sync::Arc::make_mut(&mut run.props);
            props.src = Some(sid);
            mark_appended(props, "src");
            seg.sids.push(JsNumber::Num(f64::from(sid.0)));
        }
    }
    let self_closing = p_el.positions().is_some_and(|p| p.content_start == p.end);
    let open = match p_el.positions() {
        Some(p) if self_closing => {
            let raw = part_text(store, part).and_then(|t| t.get(p.start..p.end).map(str::to_owned)).unwrap_or_default();
            SrcText::Owned(self_closing_to_open(&raw).into_owned().into())
        }
        Some(p) => span(part, p.start, p.content_start),
        None => SrcText::default(),
    };
    let src = ParaSource {
        raw: el_span(part, p_el),
        open,
        at: p_el.positions().map(|p| off(p.start)),
        p_pr: opt_span(part, ppr_el),
        segs: segs.into_iter().map(|s| s.seg).collect(),
        ..ParaSource::default()
    };
    store.add_para(para, src);
}

/// `storyOf(xml, container)`: everything before the container's content (declaration, root,
/// `w:background`, `<w:body>`) and everything after it; the groups are added by the reader.
#[must_use]
pub fn story_of(store: &SourceStore, part: u32, container: Element<'_>) -> StorySource {
    let len = store.parts.get(usize::try_from(part).unwrap_or(usize::MAX)).map_or(0, |p| p.text.len());
    match container.positions() {
        Some(p) => StorySource {
            open: span(part, 0, p.content_start),
            close: span(part, p.content_end, len),
            groups: Vec::new(),
            ..StorySource::default()
        },
        None => StorySource::default(),
    }
}

/// The story of one note (`{ open: xml.slice(note.s, note.cs), close: xml.slice(note.ce, note.e),
/// groups: [] }` of `readNotesPart`).
#[must_use]
pub fn note_story_of(part: u32, note: Element<'_>) -> StorySource {
    match note.positions() {
        Some(p) => StorySource {
            open: span(part, p.start, p.content_start),
            close: span(part, p.content_end, p.end),
            groups: Vec::new(),
            ..StorySource::default()
        },
        None => StorySource::default(),
    }
}

/// The notes part source of `readNotesPart`: `{ open: xml.slice(0, root.cs), close:
/// xml.slice(root.ce), items: [], refsAtLoad: new Set() }`.
#[must_use]
pub fn notes_part_of(store: &SourceStore, part: u32, root: Element<'_>) -> NotesPartSource {
    let story = story_of(store, part, root);
    NotesPartSource {
        open: story.open,
        close: story.close,
        items: Vec::new(),
        refs_at_load: NumSet::default(),
        ..NotesPartSource::default()
    }
}

/// An item of a notes part: a separator (`story` = `None`, `separator: true`) or a note with its
/// story.
#[must_use]
pub fn note_item(part: u32, note: Element<'_>, id: JsNumber, story: Option<StorySource>) -> NoteItemSource {
    NoteItemSource { id, raw: el_span(part, note), separator: story.is_none().then_some(true), story, ..NoteItemSource::default() }
}

/// `groupOf(xml, child, produced, store)`: the top-level child's raw text, the blocks it produced
/// (`produced` = `blockSrc(b) ?? 0` of each, [`SourceKey`]`(0)` for none) and, for a
/// `w:sdt`/`w:customXml` with content, the wrapper and the groups of its children, each with the
/// blocks whose `at` falls inside it. Iterative (an explicit stack, depth of the XML tree).
#[must_use]
pub fn group_of(store: &SourceStore, part: u32, child: Element<'_>, produced: &[SourceKey]) -> StoryGroup {
    let mut stack = vec![Frame::open(store, part, child, produced.to_vec())];
    loop {
        let Some(top) = stack.last_mut() else { return StoryGroup::default() };
        if let Some(k) = top.kids.get(top.next).copied() {
            top.next += 1;
            let inside = top.inside(k);
            stack.push(Frame::open(store, part, k, inside));
            continue;
        }
        let Some(done) = stack.pop() else { return StoryGroup::default() };
        let mut g = done.g;
        if done.wrapped {
            g.inner = Some(done.inner);
        }
        match stack.last_mut() {
            Some(parent) => parent.inner.push(g),
            None => return g,
        }
    }
}

/// One `groupOf` call being built.
struct Frame<'d> {
    g: StoryGroup,
    ids: Vec<SourceKey>,
    /// `atOf` of each id, once
    ats: Vec<Option<u32>>,
    /// the `at`s that exist never decrease: the blocks inside each child are a run found by
    /// advancing `ptr` (identical to the TS filter, which is O(children × blocks))
    mono: bool,
    ptr: usize,
    last_end: usize,
    kids: Vec<Element<'d>>,
    next: usize,
    wrapped: bool,
    inner: Vec<StoryGroup>,
}

impl<'d> Frame<'d> {
    fn open(store: &SourceStore, part: u32, child: Element<'d>, ids: Vec<SourceKey>) -> Frame<'d> {
        let mut g = StoryGroup {
            raw: el_span(part, child),
            blocks: ids.iter().map(|id| JsNumber::Num(f64::from(id.0))).collect(),
            ..StoryGroup::default()
        };
        let content = match child.name() {
            "w:sdt" => child.first_child("w:sdtContent"),
            "w:customXml" => Some(child),
            _ => None,
        };
        let mut kids = Vec::new();
        let mut wrapped = false;
        if let Some(content) = content
            && let Some((open, close)) = wrapper_spans(part, child, content)
        {
            g.wrap_open = Some(open);
            g.wrap_close = Some(close);
            kids = content.child_elements().filter(|k| !k.name().ends_with("Pr")).collect();
            wrapped = true;
        }
        let ats: Vec<Option<u32>> = if wrapped { ids.iter().map(|id| store.at_of(*id)).collect() } else { Vec::new() };
        let mut prev = 0;
        let mono = ats.iter().flatten().all(|a| {
            let ok = *a >= prev;
            prev = *a;
            ok
        });
        Frame { g, ids, ats, mono, ptr: 0, last_end: 0, kids, next: 0, wrapped, inner: Vec::new() }
    }

    /// `produced.filter((b, i) => { const at = atOf(ids[i]); return at !== undefined && at >= k.s && at < k.e; })`
    fn inside(&mut self, k: Element<'d>) -> Vec<SourceKey> {
        let Some(p) = k.positions() else { return Vec::new() };
        let (ks, ke) = (p.start, p.end);
        let in_range = |a: u32| usize::try_from(a).is_ok_and(|a| a >= ks && a < ke);
        let mut out = Vec::new();
        if self.mono && ks >= self.last_end {
            while let (Some(at), Some(id)) = (self.ats.get(self.ptr), self.ids.get(self.ptr)) {
                match *at {
                    Some(a) if in_range(a) => out.push(*id),
                    Some(a) if usize::try_from(a).is_ok_and(|a| a >= ke) => break,
                    _ => {}
                }
                self.ptr += 1;
            }
        } else {
            for (at, id) in self.ats.iter().zip(&self.ids) {
                if at.is_some_and(in_range) {
                    out.push(*id);
                }
            }
        }
        self.last_end = self.last_end.max(ke);
        out
    }
}

/// `wrapOf(w)`: `pre`/`post` only when not empty.
fn wrap_of(pre: Vec<WrapSeg>, post: Vec<WrapSeg>) -> (Option<Vec<WrapSeg>>, Option<Vec<WrapSeg>>) {
    ((!pre.is_empty()).then_some(pre), (!post.is_empty()).then_some(post))
}

/// The `TableSource` of `readTable`.
#[must_use]
pub fn table_source(
    store: &SourceStore,
    part: u32,
    tbl_el: Element<'_>,
    tbl_pr_el: Option<Element<'_>>,
    grid_el: Option<Element<'_>>,
) -> TableSource {
    TableSource {
        raw: el_span(part, tbl_el),
        at: tbl_el.positions().map(|p| off(p.start)),
        open: open_of(store, part, tbl_el),
        tbl_pr: opt_span(part, tbl_pr_el),
        tbl_grid: opt_span(part, grid_el),
        ..TableSource::default()
    }
}

/// The `RowSource` of `readTable` (`pre`/`post` are the `Wrapped` pieces of `unwrapRaw`).
#[must_use]
pub fn row_source(
    store: &SourceStore,
    part: u32,
    tr_el: Element<'_>,
    tr_pr_el: Option<Element<'_>>,
    tbl_pr_ex_el: Option<Element<'_>>,
    pre: Vec<WrapSeg>,
    post: Vec<WrapSeg>,
) -> RowSource {
    let (pre, post) = wrap_of(pre, post);
    RowSource {
        open: open_of(store, part, tr_el),
        tr_pr: opt_span(part, tr_pr_el),
        tbl_pr_ex: opt_span(part, tbl_pr_ex_el),
        pre,
        post,
        ..RowSource::default()
    }
}

/// The `CellSource` of `readTable`.
#[must_use]
pub fn cell_source(
    store: &SourceStore,
    part: u32,
    tc_el: Element<'_>,
    tc_pr_el: Option<Element<'_>>,
    groups: Option<Vec<StoryGroup>>,
    pre: Vec<WrapSeg>,
    post: Vec<WrapSeg>,
) -> CellSource {
    let (pre, post) = wrap_of(pre, post);
    CellSource { open: open_of(store, part, tc_el), tc_pr: opt_span(part, tc_pr_el), groups, pre, post, ..CellSource::default() }
}

/// The owned text of a source piece (tests and diagnostics).
#[must_use]
pub fn text_of(store: &SourceStore, text: &SrcText) -> Str {
    text.resolve(&store.parts).into()
}
