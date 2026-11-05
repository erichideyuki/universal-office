//! Frontier R4c (D1a reader, record "Milestone 1 — D1a foundation"): `document.ts` lines
//! 1583–2213: `ReadContext` (here [`ReadContext`], a struct that borrows the [`ReadCtx`] of the
//! read), `buildReadContext`, `readRunsInto`, simple and complex (nested) fields, custom inlines,
//! OMML math, `readSingleRun`, `readParagraph` and `collectInstrText`. The declarations (the nine
//! kinds) are in `core/docs/modules/uo-docx-d1a/R4c.md`.
//!
//! Shape of the port:
//!
//! - the TS `ReadContext` object (`resolveImage`, `resolveLink`, `resolvePart`, `activeComments`,
//!   `bookmarkIds`, `fieldStack`, `onBookmark`/`onBookmarkEnd`, `readBlocks`, `sources`,
//!   `sourceXml`) is the struct [`ReadContext`]: the resolvers are methods, the bookmark
//!   callbacks are [`BookmarkEvent`]s the caller drains, the block readers are the flag
//!   [`BlocksMode`] (the reader of blocks is R5's `fulldoc` function), the source store is the
//!   `ReadCtx`'s;
//! - `readRunsInto` and `readSingleRun` (mutually recursive in the TS: a container reads runs, a
//!   run's `w:ruby` reads a container) are one loop over an explicit stack of frames, so no
//!   nesting of the XML (up to its depth limit of 1,000) uses the native stack; the OMML walk
//!   (`readMathNodes`, `collectMathText`) and `collectInstrText` are loops too;
//! - the TS symbol keys of a run (`RPR_EL`, `OBJ_EL`) are a [`RunSide`] kept next to every run
//!   in a [`RunList`], and handed to R6's `capture_paragraph` with the segments;
//! - a limit hit is an error value (field depth, fields per story, runs and paragraphs per part).

use std::collections::HashMap;
use std::sync::Arc;
use std::vec::IntoIter;

use indexmap::IndexSet;
use uo_jsnum::{conv, parse, text as jstext};
use uo_opc::xml::{Document as XmlDocument, Element};

use crate::error::{DocxError, DocxLimitKind};
use crate::ids::MediaId;
use crate::js::{JsNumber, JsObject, JsonValue, NumKey, Str};
use crate::model::Paragraph;
use crate::model::para::ParaProps;
use crate::model::props::{PPr, RPr, RevisionInfo};
use crate::model::run::{BookmarkMark, CustomInline, FormField, MathNode, MathZone, NoteRef, Ptab, Run, RunExt, RunProps};
use crate::model::sources::SegSource;
use crate::text::Utf16;

use super::ctx::ReadCtx;
use super::inline_objects::object_runs;
use super::inline_objects::with_context as object;
use super::json_parse;
use super::matchers::{Cur, find_word, newlines_to_space, replace_char};
use super::plain::default_run_props;
use super::props::{read_ppr, read_revision_info, read_rpr};
use super::props_map::{para_props_from_ppr, run_props_from_rpr};
use super::sources::{ElSpan, ObjSide, PendingSeg, RunSide, capture_paragraph, inline_wrapper};

/// `CUSTOM_INLINE_TAG_PREFIX`.
pub const CUSTOM_INLINE_TAG_PREFIX: &str = "wordnext:inline:";

/// The most comment ranges that may be open at once (item 9 of the record): every run takes the
/// ids of the open ranges (`props.commentIds = [...ctx.activeComments]`), so a part of n unclosed
/// ranges and n runs holds n²/2 ids. Real files have a handful.
const MAX_OPEN_COMMENT_RANGES: usize = 64;

/// `OBJ`: the text of an object run.
const OBJ_TEXT: [u16; 1] = [0xFFFC];

/// How the context reads the content of a textbox (`readBlocks` / `readBlocksGrouped` of the TS
/// `ReadContext`: the reader of blocks is injected by the full-document reader, R5). R4b asks
/// this when it reads a `w:txbxContent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlocksMode {
    /// no block reader (`readBlocks` undefined): a textbox keeps its paragraphs only
    #[default]
    None,
    /// `readBlocks` set, `readBlocksGrouped` not
    Blocks,
    /// both set
    Grouped,
}

/// A bookmark mark seen in a paragraph: the TS `onBookmark(name, offset)` (start) and
/// `onBookmarkEnd(name, offset)` (end) callbacks, queued. `offset` is the position in the text of
/// the run list being read (UTF-16 units). The consumer (the full-document reader) drains them
/// after each block, before it pushes the block (its callbacks read `blocks.length`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkEvent {
    pub name: Str,
    pub offset: usize,
    pub end: bool,
}

/// A media part a relationship resolved to (`resolveImage`: `{ data, ext, path }`; the bytes are
/// in the document's media store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMedia {
    pub id: MediaId,
    pub ext: Str,
    pub path: Str,
}

#[derive(Debug, Clone)]
struct Rel {
    target: Str,
    external: bool,
}

/// What a wrapper (`w:hyperlink`, revisions) passes to the runs inside (`RunInherit`; the TS
/// `field` member is never set by the reader and is not ported).
#[derive(Debug, Default)]
struct Inherit {
    link: Option<Str>,
    tip: Option<Str>,
    ins: Option<RevisionInfo>,
    del: Option<RevisionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Instr,
    Result,
}

/// The runs of a list and the read-time side data of each (`RPR_EL`, `OBJ_EL`), and the length
/// of their text (the TS recomputes it with a `reduce` per bookmark).
#[derive(Debug, Default)]
pub struct RunList {
    runs: Vec<Run>,
    sides: Vec<RunSide>,
    text_len: usize,
}

impl RunList {
    fn push(&mut self, run: Run, side: RunSide) {
        self.text_len = self.text_len.saturating_add(run.text.len());
        self.runs.push(run);
        self.sides.push(side);
    }

    fn len(&self) -> usize {
        self.runs.len()
    }
}

/// An open complex field (`FieldFrame`).
#[derive(Debug)]
struct FieldFrame {
    /// identity of the frame (the TS identity of its `runs` array)
    id: u32,
    instr: String,
    code: String,
    nested: bool,
    ff: Option<FormField>,
    phase: Phase,
    /// the instruction is a table of contents (`/^\s*TOC\b/i`, fixed when the phase changes)
    toc: bool,
    runs: RunList,
    crossed: bool,
    props: RunProps,
    rpr: Option<Arc<RPr>>,
    rpr_el: Option<ElSpan>,
}

/// Where the runs of the moment go (`runTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// the list being read
    List,
    /// the result of the field with this id
    Frame(u32),
    /// the instruction of a field: dropped
    Discard,
}

/// The reader context of one part (the TS `ReadContext` made by `buildReadContext`).
pub struct ReadContext<'c, 'p> {
    /// the context of the read (limits, ids, media, the source store, nesting)
    pub ctx: &'c mut ReadCtx<'p>,
    source: Box<str>,
    rels: HashMap<Box<str>, Rel>,
    active_comments: IndexSet<NumKey>,
    comments_ver: u64,
    bookmark_ids: HashMap<NumKey, Str>,
    field_stack: Vec<FieldFrame>,
    next_frame: u32,
    fields_opened: u32,
    default_props: Arc<RunProps>,
    /// queue the bookmark marks (`ctx.onBookmark` set)
    pub collect_bookmarks: bool,
    /// the queued bookmark marks (see [`BookmarkEvent`])
    pub bookmark_events: Vec<BookmarkEvent>,
    /// the block reader of the context (see [`BlocksMode`])
    pub blocks: BlocksMode,
    /// the index (in the source store) of the part being read with positions: the TS `sourceXml`
    source_part: Option<u32>,
}

/// The part a `.rels` part belongs to (`word/_rels/document.xml.rels` → `word/document.xml`).
fn source_of_rels(rels_part: &str) -> String {
    let name = rels_part.strip_suffix(".rels").unwrap_or(rels_part);
    match name.rsplit_once('/') {
        Some((dir, file)) => match dir.strip_suffix("_rels") {
            Some(parent) => format!("{parent}{file}"),
            None => name.to_owned(),
        },
        None => name.to_owned(),
    }
}

/// `x || ''`-style truthiness of an optional string.
fn truthy(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty())
}

/// `parseInt(v ?? default, 10)`.
fn int_attr(el: Element<'_>, name: &str, default: &str) -> f64 {
    parse::parse_int(el.attr(name).unwrap_or(default), 10)
}

/// The string children of an element joined (`child.children.filter(string).join('')`).
fn direct_text(el: Element<'_>) -> String {
    let mut out = String::new();
    for n in el.children() {
        if let Some(t) = n.as_text() {
            out.push_str(t);
        }
    }
    out
}

/// `/^\s*TOC\b/i.test(instr)`: the instruction of a table of contents.
#[must_use]
pub fn is_toc_instr(instr: &str) -> bool {
    let mut c = Cur::new(instr);
    c.spaces();
    c.lit_ci("TOC") && c.boundary()
}

/// Records the current key order of `o` (every present key, in order) and then the key `key` as
/// assigned now: the TS appends a key to an object whose insertion order is not the declaration
/// order of the model.
fn append_key<T: JsObject>(o: &mut T, key: &str) {
    let names: Vec<&'static str> = o.slots_in_order().into_iter().map(|(k, _)| k).collect();
    for k in names {
        o.mark(k);
    }
    o.mark(key);
}

/// Records the assignment of `key` when the insertion order of the object is recorded (a key
/// assigned after the others goes last; an object in declaration order already has it there).
fn mark_if_recorded<T: JsObject>(o: &mut T, key: &str) {
    if !o.key_order().is_empty() {
        o.mark(key);
    }
}

/// Whether the object has the key `key` (a value or an explicit `undefined`).
fn has_key<T: JsObject>(o: &T, key: &str) -> bool {
    T::key_index(key).is_some_and(|i| !matches!(o.slot(usize::from(i)), crate::js::Slot::Absent))
}

fn is_object_run(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some())
}

fn concat_text<'a>(runs: impl Iterator<Item = &'a Run>) -> Utf16 {
    let mut units: Vec<u16> = Vec::new();
    for r in runs {
        units.extend_from_slice(r.text.units());
    }
    Utf16::from_units(units)
}

/// `/^[a-zA-Z0-9._-]+$/.test(type)`: the type of a custom-inline tag.
#[must_use]
pub fn is_custom_inline_type(ty: &str) -> bool {
    super::matchers::all_in(ty, |c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// `parseCustomInlineTag`: the type and the data of a `wordnext:inline:<type>:<uri-encoded JSON>`
/// content-control tag; `None` for anything else, including a payload `decodeURIComponent` or
/// `JSON.parse` rejects (the TS `catch`).
#[must_use]
pub fn parse_custom_inline_tag(tag: Option<&str>) -> Option<(Str, JsonValue)> {
    let payload = tag?.strip_prefix(CUSTOM_INLINE_TAG_PREFIX)?;
    let split = payload.find(':')?;
    if split == 0 {
        return None;
    }
    let ty = payload.get(..split)?;
    if !is_custom_inline_type(ty) {
        return None;
    }
    let decoded = json_parse::decode_uri_component(payload.get(split + 1..)?)?;
    let data = json_parse::parse(&decoded)?;
    Some((ty.into(), data))
}

/// The text of the `m:t` descendants of an element (`collectMathText`): an `m:t` is read as a
/// whole and not entered.
fn collect_math_text(el: Element<'_>) -> String {
    let mut out = String::new();
    let mut stack: Vec<IntoIter<Element<'_>>> = vec![el.child_elements().collect::<Vec<_>>().into_iter()];
    while let Some(top) = stack.last_mut() {
        match top.next() {
            None => {
                stack.pop();
            }
            Some(c) if c.name() == "m:t" => out.push_str(&c.text_content()),
            Some(c) => stack.push(c.child_elements().collect::<Vec<_>>().into_iter()),
        }
    }
    out
}

/// A frame of the OMML walk.
enum MathFrame<'d> {
    /// the children of an element, flattened into list `list`
    Walk { kids: IntoIter<Element<'d>>, list: usize },
    /// a node to add to list `list`
    Emit { node: MathNode, list: usize },
    /// closes a fraction: its denominator and numerator lists are the last two
    Frac { ty: Option<Str>, list: usize },
}

fn math_run_node(text: &mut String, sty: Option<&str>, rpr: Option<&RPr>, out: &mut Vec<MathNode>) {
    if text.is_empty() {
        return;
    }
    let mut n = MathNode { t: "r".into(), text: Some(Utf16::from(text.as_str())), ..MathNode::default() };
    if let Some(s) = truthy(sty) {
        n.sty = Some(s.into());
    }
    if let Some(r) = rpr {
        n.rpr = Some(Box::new(r.clone()));
    }
    out.push(n);
    text.clear();
}

fn math_text_node(text: &str, sty: Option<&str>) -> MathNode {
    MathNode { t: "r".into(), text: Some(Utf16::from(text)), sty: sty.map(Str::from), ..MathNode::default() }
}

/// `readMathNodes(el)`: the content of an OMML equation (`MathNode`s).
fn read_math_nodes(el: Element<'_>) -> Vec<MathNode> {
    let mut lists: Vec<Vec<MathNode>> = vec![Vec::new()];
    let mut stack: Vec<MathFrame<'_>> = vec![MathFrame::Walk { kids: el.child_elements().collect::<Vec<_>>().into_iter(), list: 0 }];
    while let Some(frame) = stack.pop() {
        match frame {
            MathFrame::Emit { node, list } => {
                if let Some(l) = lists.get_mut(list) {
                    l.push(node);
                }
            }
            MathFrame::Frac { ty, list } => {
                let den = lists.pop().unwrap_or_default();
                let num = lists.pop().unwrap_or_default();
                let mut f = MathNode { t: "f".into(), num: Some(num), den: Some(den), ..MathNode::default() };
                if let Some(t) = ty {
                    f.r#type = Some(t);
                }
                if let Some(l) = lists.get_mut(list) {
                    l.push(f);
                }
            }
            MathFrame::Walk { mut kids, list } => {
                let Some(c) = kids.next() else { continue };
                stack.push(MathFrame::Walk { kids, list });
                match c.name() {
                    "m:r" | "w:r" => {
                        let mr_pr = c.first_child("m:rPr");
                        let sty = mr_pr.and_then(|m| {
                            if m.first_child("m:nor").is_some() { Some("p") } else { m.first_child("m:sty").and_then(|s| s.attr("m:val")) }
                        });
                        let rpr = c.first_child("w:rPr").and_then(|w| read_rpr(Some(w)));
                        let mut text = String::new();
                        let mut out: Vec<MathNode> = Vec::new();
                        for k in c.child_elements() {
                            match k.name() {
                                "m:t" | "w:t" => text.push_str(&k.text_content()),
                                "w:br" => {
                                    math_run_node(&mut text, sty, rpr.as_ref(), &mut out);
                                    let mut br = MathNode { t: "br".into(), ..MathNode::default() };
                                    if let Some(r) = &rpr {
                                        br.rpr = Some(Box::new(r.clone()));
                                    }
                                    out.push(br);
                                }
                                _ => {}
                            }
                        }
                        math_run_node(&mut text, sty, rpr.as_ref(), &mut out);
                        if let Some(l) = lists.get_mut(list) {
                            l.extend(out);
                        }
                    }
                    "m:f" => {
                        let ty = c
                            .first_child("m:fPr")
                            .and_then(|p| p.first_child("m:type"))
                            .and_then(|t| t.attr("m:val"))
                            .filter(|v| !v.is_empty());
                        let num = c.first_child("m:num");
                        let den = c.first_child("m:den");
                        lists.push(Vec::new());
                        lists.push(Vec::new());
                        let base = lists.len() - 2;
                        stack.push(MathFrame::Frac { ty: ty.map(Str::from), list });
                        if let Some(d) = den {
                            stack.push(MathFrame::Walk { kids: d.child_elements().collect::<Vec<_>>().into_iter(), list: base + 1 });
                        }
                        if let Some(n) = num {
                            stack.push(MathFrame::Walk { kids: n.child_elements().collect::<Vec<_>>().into_iter(), list: base });
                        }
                    }
                    "m:d" => {
                        let d_pr = c.first_child("m:dPr");
                        let chr = |name: &str, def: &'static str| -> String {
                            match d_pr.and_then(|p| p.first_child(name)) {
                                Some(e) => e.attr("m:val").unwrap_or("").to_owned(),
                                None => def.to_owned(),
                            }
                        };
                        let beg = chr("m:begChr", "(");
                        let sep = chr("m:sepChr", "|");
                        let end = chr("m:endChr", ")");
                        if !beg.is_empty()
                            && let Some(l) = lists.get_mut(list)
                        {
                            l.push(math_text_node(&beg, Some("p")));
                        }
                        if !end.is_empty() {
                            stack.push(MathFrame::Emit { node: math_text_node(&end, Some("p")), list });
                        }
                        let es: Vec<Element<'_>> = c.children_named("m:e").collect();
                        for (i, e) in es.iter().enumerate().rev() {
                            stack.push(MathFrame::Walk { kids: e.child_elements().collect::<Vec<_>>().into_iter(), list });
                            if i > 0 && !sep.is_empty() {
                                stack.push(MathFrame::Emit { node: math_text_node(&sep, Some("p")), list });
                            }
                        }
                    }
                    name if !name.ends_with("Pr") => {
                        stack.push(MathFrame::Walk { kids: c.child_elements().collect::<Vec<_>>().into_iter(), list });
                    }
                    _ => {}
                }
            }
        }
    }
    lists.pop().unwrap_or_default()
}

/// What happens when a container frame of the run walk is done.
enum Finish {
    Plain,
    /// `w:fldSimple`: the runs read are its result
    FldSimple {
        instr: Str,
        parent: usize,
    },
    /// `w:sdt` with a custom-inline tag: the runs read are the fallback text
    Custom {
        ty: Str,
        data: JsonValue,
        parent: usize,
    },
}

/// The children of a `w:r` being read (`readSingleRun`).
struct RunFrame<'d> {
    kids: IntoIter<Element<'d>>,
    list: usize,
    inherit: Arc<Inherit>,
    rpr: Option<Arc<RPr>>,
    r_props: RunProps,
    rpr_el: Option<ElSpan>,
    text: String,
    base: Option<(u64, Arc<RunProps>)>,
}

/// A paragraph whose runs are read, before `readParagraph` finishes it.
struct ParaRead<'d> {
    ppr_el: Option<Element<'d>>,
    ppr: Option<Arc<PPr>>,
    props: Arc<ParaProps>,
    list: RunList,
    segs: Vec<PendingSeg>,
    part: Option<u32>,
}

enum Frame<'d> {
    Container { kids: IntoIter<Element<'d>>, inherit: Arc<Inherit>, list: usize, finish: Finish },
    Run(Box<RunFrame<'d>>),
}

/// The children of a `w:r` that are objects (R4b): read by `run_object`, whose text boxes read
/// paragraphs.
fn is_object_element(name: &str) -> bool {
    matches!(name, "w:drawing" | "w:pict" | "w:object" | "mc:AlternateContent")
}

/// The text of a `w:t`/`w:delText` added to the run's text (soft hyphens as U+2011, line breaks
/// as spaces).
#[inline(never)]
fn run_text(text: &mut String, child: Element<'_>) {
    text.push_str(&newlines_to_space(&replace_char(&direct_text(child), '\u{AD}', '\u{2011}')));
}

/// The base of a `w:ruby` whose run frame is on top of `stack`: read next, into the list of the
/// run, with the run's inheritance.
#[inline(never)]
fn ruby_base<'d>(stack: &mut Vec<Frame<'d>>, ruby: Element<'d>) {
    let Some(Frame::Run(rf)) = stack.last() else { return };
    let Some(b) = ruby.first_child("w:rubyBase") else { return };
    let (inherit, list) = (Arc::clone(&rf.inherit), rf.list);
    stack.push(Frame::Container { kids: b.child_elements().collect::<Vec<_>>().into_iter(), inherit, list, finish: Finish::Plain });
}

/// The list with index `idx`: 0 is the list the caller passed, `n` the `n`-th list the walk
/// opened (fields and custom inlines read into a list of their own). The indices are pushed and
/// popped in step with the frames; the fallback to the base list cannot happen.
fn pick<'x>(base: &'x mut RunList, lists: &'x mut [RunList], idx: usize) -> &'x mut RunList {
    match idx.checked_sub(1).and_then(|i| lists.get_mut(i)) {
        Some(l) => l,
        None => base,
    }
}

enum Step {
    Continue,
    /// a `w:ruby` with a base: read its runs into the list of the run
    Ruby,
}

impl<'c, 'p> ReadContext<'c, 'p> {
    /// `buildReadContext(pkg, relsPath)`: the relationships of a part (`word/_rels/document.xml.rels`…).
    pub fn new(ctx: &'c mut ReadCtx<'p>, rels_path: &str) -> Result<ReadContext<'c, 'p>, DocxError> {
        let mut rels: HashMap<Box<str>, Rel> = HashMap::new();
        if let Some(doc) = super::parts::part_doc(ctx, rels_path)? {
            for rel in doc.root().children_named("Relationship") {
                if let (Some(id), Some(target)) = (truthy(rel.attr("Id")), truthy(rel.attr("Target"))) {
                    let target = target.strip_prefix("./").unwrap_or(target);
                    rels.insert(id.into(), Rel { target: target.into(), external: rel.attr("TargetMode") == Some("External") });
                }
            }
        }
        let default_props = ctx.intern(default_run_props());
        Ok(ReadContext {
            ctx,
            source: source_of_rels(rels_path).into(),
            rels,
            active_comments: IndexSet::new(),
            comments_ver: 0,
            bookmark_ids: HashMap::new(),
            field_stack: Vec::new(),
            next_frame: 0,
            fields_opened: 0,
            default_props,
            collect_bookmarks: false,
            bookmark_events: Vec::new(),
            blocks: BlocksMode::None,
            source_part: None,
        })
    }

    /// `DEFAULT_RUN_PROPS` of this read (interned: every run with the default props shares it).
    #[must_use]
    pub fn default_run_props(&self) -> Arc<RunProps> {
        Arc::clone(&self.default_props)
    }

    /// Sets the part being read with positions (the TS `sourceXml`): the index `ReadCtx::source_part`
    /// returned for its text. Paragraphs are captured into the source store from then on.
    pub fn set_source_part(&mut self, part: Option<u32>) {
        self.source_part = part;
    }

    /// The part being read with positions (see [`ReadContext::set_source_part`]).
    #[must_use]
    pub fn source_part(&self) -> Option<u32> {
        self.source_part
    }

    /// `ctx.resolveImage(rid)`: the media part a relationship names, `None` for an unknown or
    /// external one or a missing part. The path is resolved by uo-opc D6 (accepted difference
    /// from the TS `word/${target}` rule).
    pub fn resolve_image(&mut self, rid: &str) -> Result<Option<ResolvedMedia>, DocxError> {
        let Some(rel) = self.rels.get(rid) else { return Ok(None) };
        if rel.external {
            return Ok(None);
        }
        let Some(path) = self.ctx.resolve_target(&self.source, &rel.target) else { return Ok(None) };
        let path: Str = path.into();
        let Some(id) = self.ctx.media_part(&path)? else { return Ok(None) };
        let last = path.rsplit('.').next().unwrap_or("");
        Ok(Some(ResolvedMedia { id, ext: jstext::to_lowercase(last).into(), path }))
    }

    /// `ctx.resolveLink(rid)`: the target of an external relationship.
    #[must_use]
    pub fn resolve_link(&self, rid: &str) -> Option<&str> {
        let rel = self.rels.get(rid)?;
        rel.external.then_some(&*rel.target)
    }

    /// `ctx.resolvePart?.(rid)`: the XML of a part a relationship names (the SmartArt drawing).
    pub fn resolve_part(&self, rid: &str) -> Result<Option<XmlDocument>, DocxError> {
        let Some(rel) = self.rels.get(rid) else { return Ok(None) };
        if rel.external {
            return Ok(None);
        }
        let Some(path) = self.ctx.resolve_target(&self.source, &rel.target) else { return Ok(None) };
        self.ctx.xml_part_plain(path)
    }

    /// The paragraph that is the TS `activeComments` size > 0 check.
    fn comment_ids(&self) -> Option<Vec<JsNumber>> {
        (!self.active_comments.is_empty()).then(|| self.active_comments.iter().map(|k| JsNumber::Num(k.get())).collect())
    }

    fn target(&self) -> Target {
        match self.field_stack.last() {
            None => Target::List,
            Some(f) if f.phase == Phase::Instr => Target::Discard,
            Some(f) if f.crossed => Target::List,
            Some(f) => Target::Frame(f.id),
        }
    }

    fn frame_mut(&mut self, id: u32) -> Option<&mut FieldFrame> {
        self.field_stack.iter_mut().rev().find(|f| f.id == id)
    }

    /// `runTarget(ctx, runs)?.push(run)`.
    fn push_target(&mut self, list: &mut RunList, run: Run, side: RunSide) -> Result<(), DocxError> {
        match self.target() {
            Target::List => {
                self.ctx.count_run()?;
                list.push(run, side);
            }
            Target::Frame(id) => {
                self.ctx.count_run()?;
                if let Some(f) = self.frame_mut(id) {
                    f.runs.push(run, side);
                }
            }
            Target::Discard => {}
        }
        Ok(())
    }

    /// `tgt?.length ?? 0` of a target.
    fn target_len(&mut self, t: Target, list: &RunList) -> Option<usize> {
        match t {
            Target::List => Some(list.len()),
            Target::Frame(id) => self.frame_mut(id).map(|f| f.runs.len()),
            Target::Discard => None,
        }
    }

    /// `tagObjectRuns(tgt, from, el)`.
    fn tag_object_runs(&mut self, t: Target, list: &mut RunList, from: usize, el: Element<'_>) {
        let Some(span) = ElSpan::of(el) else { return };
        let sides: &mut Vec<RunSide> = match t {
            Target::List => &mut list.sides,
            Target::Frame(id) => match self.frame_mut(id) {
                Some(f) => &mut f.runs.sides,
                None => return,
            },
            Target::Discard => return,
        };
        for (i, side) in sides.iter_mut().enumerate().skip(from) {
            side.obj = if i == from { ObjSide::Head(span) } else { ObjSide::Follow };
        }
    }

    fn any_toc_result(&self) -> bool {
        self.field_stack.iter().any(|f| f.phase == Phase::Result && f.toc)
    }

    /// The runs of a `w:p` (or any container): the TS `readRunsInto(runs, container, ctx)` over
    /// all child elements.
    pub fn read_runs_into(&mut self, list: &mut RunList, container: Element<'_>) -> Result<(), DocxError> {
        let kids: Vec<Element<'_>> = container.child_elements().collect();
        self.read_children(list, kids, Arc::new(Inherit::default()))
    }

    /// The walk of `readRunsInto`/`readSingleRun` over the given child elements.
    #[inline(never)]
    fn read_children<'d>(&mut self, base: &mut RunList, kids: Vec<Element<'d>>, inherit: Arc<Inherit>) -> Result<(), DocxError> {
        let mut lists: Vec<RunList> = Vec::new();
        let mut stack: Vec<Frame<'d>> = vec![Frame::Container { kids: kids.into_iter(), inherit, list: 0, finish: Finish::Plain }];
        // This loop is on the native stack once per nested story (a text box is read from inside
        // its run, through `run_object`): the frame on top stays in place while its child is
        // read, and every step that cannot reach a text box is a function of its own.
        while let Some(top) = stack.last_mut() {
            match top {
                Frame::Run(rf) => match rf.kids.next() {
                    Some(child) if is_object_element(child.name()) => {
                        let li = rf.list;
                        self.run_object(rf, child, pick(base, &mut lists, li))?;
                    }
                    Some(child) => {
                        if let Step::Ruby = self.run_child(rf, child, base, &mut lists)? {
                            ruby_base(&mut stack, child);
                        }
                    }
                    None => self.end_run(&mut stack, base, &mut lists)?,
                },
                Frame::Container { .. } => self.container_step(&mut stack, base, &mut lists)?,
            }
        }
        Ok(())
    }

    /// The end of the `w:r` on top of `stack`: its pending text becomes a run.
    #[inline(never)]
    fn end_run(&mut self, stack: &mut Vec<Frame<'_>>, base: &mut RunList, lists: &mut [RunList]) -> Result<(), DocxError> {
        if let Some(Frame::Run(mut rf)) = stack.pop() {
            let list = rf.list;
            self.flush(&mut rf, pick(base, lists, list))?;
        }
        Ok(())
    }

    /// The next child of the container on top of `stack`, or its end.
    #[inline(never)]
    fn container_step<'d>(&mut self, stack: &mut Vec<Frame<'d>>, base: &mut RunList, lists: &mut Vec<RunList>) -> Result<(), DocxError> {
        let Some(Frame::Container { kids, inherit, list, .. }) = stack.last_mut() else { return Ok(()) };
        match kids.next() {
            Some(child) => {
                let (inherit, list) = (Arc::clone(inherit), *list);
                self.container_child(child, &inherit, list, base, lists, stack)
            }
            None => match stack.pop() {
                Some(Frame::Container { finish, .. }) => self.finish_container(finish, base, lists),
                _ => Ok(()),
            },
        }
    }

    #[inline(never)]
    fn finish_container(&mut self, finish: Finish, base: &mut RunList, lists: &mut Vec<RunList>) -> Result<(), DocxError> {
        match finish {
            Finish::Plain => {}
            Finish::FldSimple { instr, parent } => {
                let inner = lists.pop().unwrap_or_default();
                let first = inner.runs.iter().position(|r| !is_object_run(r));
                let first_run = first.and_then(|i| inner.runs.get(i));
                let text = concat_text(inner.runs.iter().filter(|r| !is_object_run(r)));
                let props = first_run.map_or_else(|| Arc::clone(&self.default_props), |r| Arc::clone(&r.props));
                let mut run = Run { text, props, ..Run::default() };
                let ext = run.ext_mut();
                ext.field = Some(instr);
                ext.mark("field");
                let mut side = RunSide::default();
                if let Some(i) = first {
                    if let Some(rpr) = inner.runs.get(i).and_then(|r| r.rpr.clone()) {
                        run.rpr = Some(rpr);
                    }
                    side.rpr = inner.sides.get(i).and_then(|s| s.rpr);
                }
                if !inner.runs.is_empty() {
                    let ext = run.ext_mut();
                    ext.field_result = Some(inner.runs);
                    ext.mark("fieldResult");
                }
                self.push_target(pick(base, lists, parent), run, side)?;
            }
            Finish::Custom { ty, data, parent } => {
                let nested = lists.pop().unwrap_or_default();
                let fallback = concat_text(nested.runs.iter());
                let props = nested.runs.first().map_or_else(|| Arc::clone(&self.default_props), |r| Arc::clone(&r.props));
                let mut run = Run { text: Utf16::from_units(OBJ_TEXT.to_vec()), props, ..Run::default() };
                let ext = run.ext_mut();
                ext.custom = Some(Box::new(CustomInline { r#type: ty, data, fallback_text: fallback, ..CustomInline::default() }));
                ext.mark("custom");
                self.push_target(pick(base, lists, parent), run, RunSide::default())?;
            }
        }
        Ok(())
    }

    /// One child element of a container (the `switch` of `readRunsInto`).
    #[inline(never)]
    fn container_child<'d>(
        &mut self,
        child: Element<'d>,
        inherit: &Arc<Inherit>,
        list: usize,
        base: &mut RunList,
        lists: &mut Vec<RunList>,
        stack: &mut Vec<Frame<'d>>,
    ) -> Result<(), DocxError> {
        match child.name() {
            "w:commentRangeStart" => {
                let key = NumKey::new(int_attr(child, "w:id", "-1"));
                if self.active_comments.len() >= MAX_OPEN_COMMENT_RANGES && !self.active_comments.contains(&key) {
                    return Err(DocxError::Corrupt("more than 64 comment ranges open at once"));
                }
                if self.active_comments.insert(key) {
                    self.comments_ver += 1;
                }
            }
            "w:commentRangeEnd" => {
                if self.active_comments.shift_remove(&NumKey::new(int_attr(child, "w:id", "-1"))) {
                    self.comments_ver += 1;
                }
            }
            "w:bookmarkStart" | "w:bookmarkEnd" => {
                let end = child.name() == "w:bookmarkEnd";
                let id = int_attr(child, "w:id", "");
                let name: Option<Str> =
                    if end { self.bookmark_ids.get(&NumKey::new(id)).cloned() } else { child.attr("w:name").map(Str::from) };
                let Some(name) = name.filter(|n| !n.is_empty()) else { return Ok(()) };
                let l = pick(base, lists, list);
                let at = l.text_len;
                if end {
                    self.bookmark_ids.remove(&NumKey::new(id));
                } else if id.is_finite() {
                    self.bookmark_ids.insert(NumKey::new(id), name.clone());
                }
                if self.collect_bookmarks {
                    self.bookmark_events.push(BookmarkEvent { name: name.clone(), offset: at, end });
                }
                let top_open = self.field_stack.last().is_some_and(|t| !t.crossed);
                if self.ctx.sources.is_some() && id.is_finite() && !top_open && child.attr("w:colFirst").is_none() {
                    let mut props = default_run_props();
                    if let Some(link) = inherit.link.as_deref().filter(|l| !l.is_empty()) {
                        props.link = Some(link.into());
                        if let Some(tip) = inherit.tip.as_deref().filter(|t| !t.is_empty()) {
                            props.link_tip = Some(tip.into());
                        }
                    }
                    let mut run = Run { props: self.ctx.intern(props), ..Run::default() };
                    let ext = run.ext_mut();
                    ext.bookmark = Some(BookmarkMark { id: JsNumber::Num(id), name, end: end.then_some(true), ..BookmarkMark::default() });
                    ext.mark("bookmark");
                    self.ctx.count_run()?;
                    pick(base, lists, list).push(run, RunSide::default());
                }
            }
            "w:r" => {
                let r_pr_el = child.first_child("w:rPr");
                let rpr = read_rpr(r_pr_el);
                let mut r_props = run_props_from_rpr(rpr.as_ref());
                if let Some(link) = inherit.link.as_deref().filter(|l| !l.is_empty()) {
                    let had_color = r_props.color.is_some();
                    r_props.link = Some(link.into());
                    if let Some(tip) = inherit.tip.as_deref().filter(|t| !t.is_empty()) {
                        r_props.link_tip = Some(tip.into());
                    }
                    r_props.underline = Some(true);
                    if !had_color {
                        append_key(&mut r_props, "color");
                        r_props.color = Some("0563C1".into());
                    }
                }
                let rf = RunFrame {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    list,
                    inherit: Arc::clone(inherit),
                    rpr: rpr.map(|r| self.ctx.intern(r)),
                    r_props,
                    rpr_el: r_pr_el.and_then(ElSpan::of),
                    text: String::new(),
                    base: None,
                };
                stack.push(Frame::Run(Box::new(rf)));
            }
            "w:hyperlink" => {
                let rid = truthy(child.attr("r:id"));
                let anchor = truthy(child.attr("w:anchor"));
                let link: Option<Str> = rid
                    .and_then(|r| self.resolve_link(r))
                    .filter(|l| !l.is_empty())
                    .map(Str::from)
                    .or_else(|| anchor.map(|a| format!("#{a}").into()));
                let tip = truthy(child.attr("w:tooltip")).map(Str::from).or_else(|| inherit.tip.clone());
                let inh = Inherit { link, tip, ins: inherit.ins.clone(), del: inherit.del.clone() };
                stack.push(Frame::Container {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    inherit: Arc::new(inh),
                    list,
                    finish: Finish::Plain,
                });
            }
            "w:fldSimple" => {
                let instr: Str = child.attr("w:instr").unwrap_or("").into();
                lists.push(RunList::default());
                stack.push(Frame::Container {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    inherit: Arc::clone(inherit),
                    list: lists.len(),
                    finish: Finish::FldSimple { instr, parent: list },
                });
            }
            "m:oMath" | "m:oMathPara" => {
                let text = collect_math_text(child);
                if text.is_empty() {
                    return Ok(());
                }
                let mut props = default_run_props();
                props.italic = Some(true);
                props.is_math = Some(true);
                let mut run = Run { text: Utf16::from(text), props: self.ctx.intern(props), ..Run::default() };
                if child.name() == "m:oMathPara" {
                    let jc = child
                        .first_child("m:oMathParaPr")
                        .and_then(|p| p.first_child("m:jc"))
                        .and_then(|j| j.attr("m:val"))
                        .filter(|v| !v.is_empty());
                    let mut nodes: Vec<MathNode> = Vec::new();
                    for om in child.children_named("m:oMath") {
                        if !nodes.is_empty() {
                            nodes.push(MathNode { t: "br".into(), ..MathNode::default() });
                        }
                        nodes.extend(read_math_nodes(om));
                    }
                    let mut zone = MathZone { nodes, ..MathZone::default() };
                    zone.mark("nodes");
                    if let Some(j) = jc {
                        zone.jc = Some(j.into());
                        zone.mark("jc");
                    }
                    let ext = run.ext_mut();
                    ext.math = Some(Box::new(zone));
                    ext.mark("math");
                }
                self.push_target(pick(base, lists, list), run, RunSide::default())?;
            }
            "w:sdt" => {
                let content = child.descend("w:sdtContent");
                let tag = child.first_child("w:sdtPr").and_then(|p| p.first_child("w:tag")).and_then(|t| t.attr("w:val"));
                let custom = parse_custom_inline_tag(tag);
                if let Some(content) = content {
                    let kids = content.child_elements().collect::<Vec<_>>().into_iter();
                    match custom {
                        Some((ty, data)) => {
                            lists.push(RunList::default());
                            stack.push(Frame::Container {
                                kids,
                                inherit: Arc::clone(inherit),
                                list: lists.len(),
                                finish: Finish::Custom { ty, data, parent: list },
                            });
                        }
                        None => stack.push(Frame::Container { kids, inherit: Arc::clone(inherit), list, finish: Finish::Plain }),
                    }
                }
            }
            "w:smartTag" | "w:customXml" | "w:dir" | "w:bdo" => {
                stack.push(Frame::Container {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    inherit: Arc::clone(inherit),
                    list,
                    finish: Finish::Plain,
                });
            }
            "w:ins" | "w:moveTo" => {
                let inh = Inherit {
                    link: inherit.link.clone(),
                    tip: inherit.tip.clone(),
                    ins: Some(read_revision_info(child)),
                    del: inherit.del.clone(),
                };
                stack.push(Frame::Container {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    inherit: Arc::new(inh),
                    list,
                    finish: Finish::Plain,
                });
            }
            "w:del" | "w:moveFrom" => {
                let inh = Inherit {
                    link: inherit.link.clone(),
                    tip: inherit.tip.clone(),
                    ins: inherit.ins.clone(),
                    del: Some(read_revision_info(child)),
                };
                stack.push(Frame::Container {
                    kids: child.child_elements().collect::<Vec<_>>().into_iter(),
                    inherit: Arc::new(inh),
                    list,
                    finish: Finish::Plain,
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// `make(text, extra)` of `readSingleRun`: a run with the props of the `w:r` and what the
    /// context adds (comments, revisions, the `rPr`, `inToc`). `ext` is the rare fields the
    /// caller gives (`extra`), in their order.
    fn make(&mut self, rf: &mut RunFrame<'_>, text: Utf16, ext: Option<Box<RunExt>>) -> (Run, RunSide) {
        let ver = self.comments_ver;
        let props = match &rf.base {
            Some((v, p)) if *v == ver => Arc::clone(p),
            _ => {
                let mut p = rf.r_props.clone();
                if let Some(ids) = self.comment_ids() {
                    p.comment_ids = Some(ids);
                    mark_if_recorded(&mut p, "commentIds");
                }
                if let Some(i) = &rf.inherit.ins {
                    p.ins = Some(Box::new(i.clone()));
                    mark_if_recorded(&mut p, "ins");
                }
                if let Some(d) = &rf.inherit.del {
                    p.del = Some(Box::new(d.clone()));
                    mark_if_recorded(&mut p, "del");
                }
                let p = self.ctx.intern(p);
                rf.base = Some((ver, Arc::clone(&p)));
                p
            }
        };
        let mut run = Run { text, props, rpr: rf.rpr.clone(), ext, ..Run::default() };
        if self.any_toc_result() {
            let ext = run.ext_mut();
            append_key(ext, "inToc");
            ext.in_toc = Some(true);
        }
        (run, RunSide { rpr: rf.rpr_el, obj: ObjSide::None })
    }

    fn flush(&mut self, rf: &mut RunFrame<'_>, list: &mut RunList) -> Result<(), DocxError> {
        if rf.text.is_empty() {
            return Ok(());
        }
        let text = Utf16::from(rf.text.as_str());
        rf.text.clear();
        let (run, side) = self.make(rf, text, None);
        self.push_target(list, run, side)
    }

    fn push_run(&mut self, rf: &mut RunFrame<'_>, list: &mut RunList, text: Utf16, ext: Option<Box<RunExt>>) -> Result<(), DocxError> {
        self.flush(rf, list)?;
        let (run, side) = self.make(rf, text, ext);
        self.push_target(list, run, side)
    }

    /// One child of a `w:r` (the loop of `readSingleRun`) that is not an object (objects go to
    /// [`ReadContext::run_object`] from the walk).
    #[inline(never)]
    fn run_child(
        &mut self,
        rf: &mut RunFrame<'_>,
        child: Element<'_>,
        base: &mut RunList,
        lists: &mut [RunList],
    ) -> Result<Step, DocxError> {
        let name = child.name();
        let li = rf.list;
        match name {
            "w:rPr" => {}
            "w:fldChar" => self.run_fld_char(rf, child, pick(base, lists, li))?,
            "w:instrText" | "w:delInstrText" => self.run_instr_text(child),
            "w:t" | "w:delText" => run_text(&mut rf.text, child),
            "w:tab" => rf.text.push('\t'),
            "w:br" | "w:cr" => rf.text.push(break_char(child)),
            "w:noBreakHyphen" => rf.text.push('\u{2011}'),
            "w:softHyphen" => rf.text.push('\u{AD}'),
            "w:ptab" => self.run_ptab(rf, child, pick(base, lists, li))?,
            "w:sym" => self.run_sym(rf, child, pick(base, lists, li))?,
            "w:footnoteReference" | "w:endnoteReference" => self.run_note_ref(rf, child, pick(base, lists, li))?,
            "w:footnoteRef" | "w:endnoteRef" | "w:pgNum" => self.run_field_mark(rf, name, pick(base, lists, li))?,
            "w:ruby" if child.first_child("w:rubyBase").is_some() => {
                self.flush(rf, pick(base, lists, li))?;
                return Ok(Step::Ruby);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }

    /// `w:fldChar`: a field begins, changes phase or ends.
    #[inline(never)]
    fn run_fld_char(&mut self, rf: &mut RunFrame<'_>, child: Element<'_>, list: &mut RunList) -> Result<(), DocxError> {
        self.flush(rf, list)?;
        match child.attr("w:fldCharType") {
            Some("begin") => self.field_begin(rf, child)?,
            Some("separate") => {
                if let Some(top) = self.field_stack.last_mut() {
                    top.phase = Phase::Result;
                    top.toc = is_toc_instr(&top.instr);
                }
            }
            Some("end") if !self.field_stack.is_empty() => self.close_field(list)?,
            _ => {}
        }
        Ok(())
    }

    /// `w:instrText`: the text goes to the open field while it is in its instruction.
    #[inline(never)]
    fn run_instr_text(&mut self, child: Element<'_>) {
        if let Some(top) = self.field_stack.last_mut()
            && top.phase == Phase::Instr
        {
            let t = child.text_content();
            top.instr.push_str(&t);
            top.code.push_str(&t);
        }
    }

    /// `w:ptab`.
    #[inline(never)]
    fn run_ptab(&mut self, rf: &mut RunFrame<'_>, child: Element<'_>, list: &mut RunList) -> Result<(), DocxError> {
        let mut ptab = Ptab {
            alignment: child.attr("w:alignment").unwrap_or("left").into(),
            relative_to: child.attr("w:relativeTo").unwrap_or("margin").into(),
            ..Ptab::default()
        };
        if let Some(l) = truthy(child.attr("w:leader")) {
            ptab.leader = Some(l.into());
        }
        let mut ext = Box::new(RunExt::default());
        ext.ptab = Some(ptab);
        ext.mark("ptab");
        self.push_run(rf, list, Utf16::from("\t"), Some(ext))
    }

    /// `w:sym`: one character of a symbol font.
    #[inline(never)]
    fn run_sym(&mut self, rf: &mut RunFrame<'_>, child: Element<'_>, list: &mut RunList) -> Result<(), DocxError> {
        let hex = child.attr("w:char").unwrap_or("F0B7");
        let mut code = parse::parse_int(hex, 16);
        if code <= 255.0 {
            code = f64::from(conv::to_int32(code) | 0xf000);
        }
        let font = child.attr("w:font");
        self.flush(rf, list)?;
        let (mut run, side) = self.make(rf, Utf16::from_units(vec![conv::to_uint16(code)]), None);
        {
            let props = Arc::make_mut(&mut run.props);
            let existed = has_key(props, "fontAscii");
            if !existed {
                append_key(props, "fontAscii");
            }
            let value = font.map(Str::from).or_else(|| rf.r_props.font_ascii.clone());
            props.font_ascii = value;
            if let Some(i) = RunProps::key_index("fontAscii") {
                if props.font_ascii.is_none() {
                    props.mark_undefined("fontAscii");
                } else {
                    props.cleared.clear(u32::from(i));
                }
            }
        }
        self.ctx.intern_shared(&mut run.props);
        if let Some(f) = truthy(font) {
            let ext = run.ext_mut();
            append_key(ext, "symFont");
            ext.sym_font = Some(f.into());
        }
        self.push_target(list, run, side)
    }

    /// `w:footnoteReference` / `w:endnoteReference`.
    #[inline(never)]
    fn run_note_ref(&mut self, rf: &mut RunFrame<'_>, child: Element<'_>, list: &mut RunList) -> Result<(), DocxError> {
        let id = int_attr(child, "w:id", "0");
        self.flush(rf, list)?;
        let custom = matches!(child.attr("w:customMarkFollows"), Some("1" | "true"));
        let mut note = NoteRef {
            kind: if child.name() == "w:footnoteReference" { "footnote" } else { "endnote" }.into(),
            id: JsNumber::Num(id),
            ..NoteRef::default()
        };
        if custom {
            note.custom = Some(true);
        }
        let mut ext = Box::new(RunExt::default());
        ext.note_ref = Some(note);
        ext.mark("noteRef");
        let (mut run, side) = self.make(rf, Utf16::new(), Some(ext));
        {
            let props = Arc::make_mut(&mut run.props);
            if !has_key(props, "vertAlign") {
                append_key(props, "vertAlign");
            }
            props.vert_align = Some("superscript".into());
        }
        self.ctx.intern_shared(&mut run.props);
        self.push_target(list, run, side)
    }

    /// `w:footnoteRef`, `w:endnoteRef` and `w:pgNum`: an empty run with a field.
    #[inline(never)]
    fn run_field_mark(&mut self, rf: &mut RunFrame<'_>, name: &str, list: &mut RunList) -> Result<(), DocxError> {
        let mut ext = Box::new(RunExt::default());
        ext.field = Some(
            match name {
                "w:footnoteRef" => " FOOTNOTEREF_MARK ",
                "w:endnoteRef" => " ENDNOTEREF_MARK ",
                _ => " PAGE ",
            }
            .into(),
        );
        ext.mark("field");
        self.push_run(rf, list, Utf16::new(), Some(ext))
    }

    /// `w:drawing`, `w:pict`, `w:object` and `mc:AlternateContent`: the objects of R4b. Their
    /// textboxes read paragraphs, which read runs: this function is on the native stack once per
    /// nested story: a small frame, the runs are read first and pushed by functions that return
    /// before or start after the read (every object is read before the first push, as the TS).
    #[inline(never)]
    fn run_object(&mut self, rf: &mut RunFrame<'_>, child: Element<'_>, list: &mut RunList) -> Result<(), DocxError> {
        let (tgt, k) = self.object_start(rf, list)?;
        let runs = object_runs(child, self)?;
        self.object_finish(rf, list, runs, (tgt, k), child)
    }

    /// Before an object: the text read so far becomes a run, and where the object runs go.
    #[inline(never)]
    fn object_start(&mut self, rf: &mut RunFrame<'_>, list: &mut RunList) -> Result<(Target, usize), DocxError> {
        self.flush(rf, list)?;
        let tgt = self.target();
        Ok((tgt, self.target_len(tgt, list).unwrap_or(0)))
    }

    /// After an object: its runs pushed and tagged with the object element.
    #[expect(clippy::vec_box, reason = "each box becomes the `ext` of a run as it is, with no copy of the `RunExt`")]
    #[inline(never)]
    fn object_finish(
        &mut self,
        rf: &mut RunFrame<'_>,
        list: &mut RunList,
        runs: Vec<Box<RunExt>>,
        (tgt, k): (Target, usize),
        child: Element<'_>,
    ) -> Result<(), DocxError> {
        for r in runs {
            self.push_object(rf, list, object::object_run(r))?;
        }
        self.tag_object_runs(tgt, list, k, child);
        Ok(())
    }

    /// `pushRun(r)` for an object run of R4b (`Omit<Run, 'props'>`): its text and rare fields
    /// go through `make`.
    fn push_object(&mut self, rf: &mut RunFrame<'_>, list: &mut RunList, r: Run) -> Result<(), DocxError> {
        self.push_run(rf, list, r.text, r.ext)
    }

    /// `fldChar begin`: a new frame on the field stack.
    fn field_begin(&mut self, rf: &RunFrame<'_>, child: Element<'_>) -> Result<(), DocxError> {
        if self.field_stack.len() >= self.ctx.limits.max_field_depth as usize {
            return Err(self.ctx.limit(DocxLimitKind::FieldDepth, self.ctx.limits.max_field_depth));
        }
        self.fields_opened = self.fields_opened.saturating_add(1);
        if self.fields_opened > self.ctx.limits.max_fields_per_story {
            return Err(self.ctx.limit(DocxLimitKind::FieldsPerStory, self.ctx.limits.max_fields_per_story));
        }
        let mut props = rf.r_props.clone();
        if let Some(i) = &rf.inherit.ins {
            props.ins = Some(Box::new(i.clone()));
            mark_if_recorded(&mut props, "ins");
        }
        if let Some(d) = &rf.inherit.del {
            props.del = Some(Box::new(d.clone()));
            mark_if_recorded(&mut props, "del");
        }
        let ff = child.first_child("w:ffData").and_then(read_form_field);
        let id = self.next_frame;
        self.next_frame = self.next_frame.wrapping_add(1);
        self.field_stack.push(FieldFrame {
            id,
            instr: String::new(),
            code: String::new(),
            nested: false,
            ff,
            phase: Phase::Instr,
            toc: false,
            runs: RunList::default(),
            crossed: false,
            props,
            rpr: rf.rpr.clone(),
            rpr_el: rf.rpr_el,
        });
        Ok(())
    }

    /// `closeField(ctx, runs)`: pops the top field and emits its run.
    fn close_field(&mut self, list: &mut RunList) -> Result<(), DocxError> {
        let Some(f) = self.field_stack.pop() else { return Ok(()) };
        if f.crossed {
            if self.ctx.sources.is_some() {
                let mut run = Run { props: Arc::clone(&self.default_props), ..Run::default() };
                let ext = run.ext_mut();
                ext.field_end = Some(true);
                ext.mark("fieldEnd");
                self.push_target(list, run, RunSide::default())?;
            }
            return Ok(());
        }
        let first = f.runs.runs.iter().position(|r| !is_object_run(r)).or(if f.runs.runs.is_empty() { None } else { Some(0) });
        let text = concat_text(f.runs.runs.iter().filter(|r| !is_object_run(r)));
        if let Some(parent) = self.field_stack.last_mut()
            && parent.phase == Phase::Instr
        {
            parent.instr.push_str(&text.to_string_lossy());
            parent.code.push_str(&format!("{{ {} }}", jstext::trim(&f.code)));
            parent.nested = true;
            return Ok(());
        }
        let first_run = first.and_then(|i| f.runs.runs.get(i));
        let props = first_run.map_or_else(|| self.ctx.intern(f.props.clone()), |r| Arc::clone(&r.props));
        let mut run = Run { text, props, ..Run::default() };
        let ext = run.ext_mut();
        ext.field = Some(format!(" {} ", jstext::trim(&f.instr)).into());
        ext.mark("field");
        if f.nested {
            ext.field_code = Some(format!(" {} ", jstext::trim(&f.code)).into());
            ext.mark("fieldCode");
        }
        let is_ff = f.ff.is_some();
        let rpr = if is_ff { f.rpr.clone() } else { first_run.and_then(|r| r.rpr.clone()).or_else(|| f.rpr.clone()) };
        run.rpr = rpr;
        let first_side = first.and_then(|i| f.runs.sides.get(i)).and_then(|s| s.rpr);
        let side = RunSide { rpr: if is_ff { f.rpr_el } else { first_side.or(f.rpr_el) }, obj: ObjSide::None };
        if let Some(ff) = f.ff {
            let ext = run.ext_mut();
            ext.form_field = Some(Box::new(ff));
            ext.mark("formField");
        }
        if !f.runs.runs.is_empty() {
            let ext = run.ext_mut();
            let mut result = f.runs.runs;
            result.shrink_to_fit();
            ext.field_result = Some(result);
            ext.mark("fieldResult");
        }
        self.push_target(list, run, side)
    }

    /// `readParagraph(pEl, ctx)`. A small frame: a paragraph is on the native stack once per nested
    /// story, the props, the segments and the paragraph are built by functions of their own.
    #[inline(never)]
    pub fn read_paragraph(&mut self, p_el: Element<'_>) -> Result<Paragraph, DocxError> {
        self.ctx.count_paragraph()?;
        let ppr_el = p_el.first_child("w:pPr");
        let (ppr, props) = paragraph_props(self.ctx, ppr_el);
        let mut list = RunList::default();
        let part = self.source_part.filter(|_| self.ctx.sources.is_some() && p_el.positions().is_some());
        let mut segs: Vec<PendingSeg> = Vec::new();
        match part {
            None => self.read_children(&mut list, p_el.child_elements().collect(), Arc::new(Inherit::default()))?,
            Some(part) => self.read_with_segments(&mut list, &mut segs, part, p_el)?,
        }
        self.finish_paragraph(p_el, ParaRead { ppr_el, ppr, props, list, segs, part })
    }

    /// The end of `readParagraph` once its runs are read: the fields left open, the TOC
    /// instruction, the id and the sources.
    #[inline(never)]
    fn finish_paragraph(&mut self, p_el: Element<'_>, read: ParaRead<'_>) -> Result<Paragraph, DocxError> {
        let ParaRead { ppr_el, ppr, mut props, mut list, segs, part } = read;
        self.cross_open_fields(&mut list)?;
        let instr = collect_instr_text(p_el);
        if find_word(&instr, "TOC", false).is_some() {
            Arc::make_mut(&mut props).toc_instr = Some(jstext::trim(&instr).into());
            self.ctx.intern_shared(&mut props);
        }
        // exact capacity: the model keeps the vector for the paragraph's life (record, "Model size — P1")
        let mut runs = std::mem::take(&mut list.runs);
        runs.shrink_to_fit();
        let mut para = Paragraph { runs, props, ppr, ..Paragraph::default() };
        para.id = self.ctx.ids.para();
        if let (Some(part), Some(store)) = (part, self.ctx.sources.as_mut()) {
            capture_paragraph(store, part, &mut para, p_el, ppr_el, segs, &list.sides);
        }
        Ok(para)
    }

    /// The reading of a paragraph that keeps sources: one segment per top-level child, a field
    /// left open by a child joins the following ones into its segment.
    #[inline(never)]
    fn read_with_segments(
        &mut self,
        list: &mut RunList,
        segs: &mut Vec<PendingSeg>,
        part: u32,
        p_el: Element<'_>,
    ) -> Result<(), DocxError> {
        let mut cur: Option<usize> = None;
        for child in p_el.child_elements() {
            if child.name() == "w:pPr" {
                continue;
            }
            let before = list.len();
            let joining = cur.is_some() && self.field_stack.iter().any(|f| !f.crossed);
            self.read_children(list, vec![child], Arc::new(Inherit::default()))?;
            cur = self.segment_of_child(list, segs, part, child, (before, joining, cur));
        }
        Ok(())
    }

    /// The segment of one top-level child of a paragraph read with sources, once its runs are read:
    /// joined to the current one while a field is open, else a new one (returned as current).
    #[inline(never)]
    fn segment_of_child(
        &mut self,
        list: &RunList,
        segs: &mut Vec<PendingSeg>,
        part: u32,
        child: Element<'_>,
        (before, joining, cur): (usize, bool, Option<usize>),
    ) -> Option<usize> {
        match (joining, cur) {
            (true, Some(c)) => {
                if let (Some(seg), Some(store)) = (segs.get_mut(c), self.ctx.sources.as_ref()) {
                    let piece = super::sources::el_span(part, child);
                    seg.seg.raw = store.concat(&seg.seg.raw, &piece);
                    seg.to = list.len();
                    seg.seg.wrap_open = None;
                    seg.seg.wrap_close = None;
                    for k in ["wrapOpen", "wrapClose"] {
                        if let Some(i) = SegSource::key_index(k) {
                            seg.seg.keys.forget(i);
                        }
                    }
                }
                cur
            }
            _ => {
                let mut seg = PendingSeg::new(part, child, before, list.len());
                let open_here = self.field_stack.iter().any(|f| !f.crossed);
                if !open_here && let Some((open, close)) = inline_wrapper(part, child) {
                    seg.seg.mark("raw");
                    seg.seg.mark("sids");
                    seg.seg.wrap_open = Some(open);
                    seg.seg.mark("wrapOpen");
                    seg.seg.wrap_close = Some(close);
                    seg.seg.mark("wrapClose");
                }
                segs.push(seg);
                Some(segs.len() - 1)
            }
        }
    }

    /// The end-of-paragraph loop of `readParagraph`: a field whose result goes on in the next
    /// paragraph turns what this paragraph read of it into a run, and its frame is `crossed`.
    fn cross_open_fields(&mut self, list: &mut RunList) -> Result<(), DocxError> {
        let n = self.field_stack.len();
        let sources = self.ctx.sources.is_some();
        for i in (0..n).rev() {
            let Some(f) = self.field_stack.get_mut(i) else { continue };
            if f.phase != Phase::Result || f.crossed {
                continue;
            }
            if i + 1 == n && (!f.runs.runs.is_empty() || sources) {
                let first = f.runs.runs.iter().position(|r| !is_object_run(r)).or(if f.runs.runs.is_empty() { None } else { Some(0) });
                let text = concat_text(f.runs.runs.iter().filter(|r| !is_object_run(r)));
                let first_run = first.and_then(|k| f.runs.runs.get(k));
                let props = first_run.map_or_else(|| self.ctx.intern(f.props.clone()), |r| Arc::clone(&r.props));
                let rpr = first_run.and_then(|r| r.rpr.clone()).or_else(|| f.rpr.clone());
                let mut run = Run { text, props, rpr, ..Run::default() };
                let ext = run.ext_mut();
                ext.field = Some(format!(" {} ", jstext::trim(&f.instr)).into());
                ext.mark("field");
                ext.field_open = Some(true);
                ext.mark("fieldOpen");
                if !f.runs.runs.is_empty() {
                    let mut result = std::mem::take(&mut f.runs.runs);
                    result.shrink_to_fit();
                    ext.field_result = Some(result);
                    ext.mark("fieldResult");
                }
                self.ctx.count_run()?;
                list.push(run, RunSide::default());
            }
            let Some(f) = self.field_stack.get_mut(i) else { continue };
            f.crossed = true;
            f.runs = RunList::default();
        }
        Ok(())
    }
}

/// `w:ffData` of a `fldChar begin` (a legacy form field).
fn read_form_field(ff: Element<'_>) -> Option<FormField> {
    if let Some(cb) = ff.first_child("w:checkBox") {
        let def = cb.first_child("w:default").and_then(|d| d.attr("w:val"));
        let chk = cb.first_child("w:checked");
        let size = cb.first_child("w:size").and_then(|s| s.attr("w:val"));
        let checked = match chk {
            Some(c) => !matches!(c.attr("w:val"), Some("0" | "false")),
            None => matches!(def, Some("1" | "true")),
        };
        let mut f = FormField { r#type: "checkBox".into(), checked: Some(checked), ..FormField::default() };
        if let Some(s) = truthy(size) {
            f.size_half_pt = Some(JsNumber::Num(parse::parse_int(s, 10)));
        }
        if cb.first_child("w:sizeAuto").is_some() || truthy(size).is_none() {
            f.size_auto = Some(true);
        }
        return Some(f);
    }
    if let Some(dl) = ff.first_child("w:ddList") {
        let entries: Vec<Utf16> = dl.children_named("w:listEntry").map(|e| Utf16::from(e.attr("w:val").unwrap_or(""))).collect();
        let idx = |name: &str| -> Option<f64> {
            let k = parse::parse_int(dl.first_child(name).and_then(|e| e.attr("w:val")).unwrap_or(""), 10);
            (conv::is_integer(k) && k >= 0.0 && k < entries.len() as f64).then_some(k)
        };
        let selected = idx("w:result").or_else(|| idx("w:default")).unwrap_or(0.0);
        return Some(FormField {
            r#type: "dropDown".into(),
            entries: Some(entries),
            selected: Some(JsNumber::Num(selected)),
            ..FormField::default()
        });
    }
    if ff.first_child("w:textInput").is_some() {
        return Some(FormField { r#type: "textInput".into(), ..FormField::default() });
    }
    None
}

/// The `w:pPr` of a paragraph read and mapped (`readPPr`, `paraPropsFromPPr`), both behind an
/// `Arc`: the large structs are built here, off the frame of the paragraph.
#[inline(never)]
fn paragraph_props(ctx: &mut ReadCtx<'_>, ppr_el: Option<Element<'_>>) -> (Option<Arc<PPr>>, Arc<ParaProps>) {
    let ppr: Option<PPr> = read_ppr(ppr_el);
    let props = para_props_from_ppr(ppr.as_ref());
    (ppr.map(|p| ctx.intern(p)), ctx.intern(props))
}

/// The character a `w:br` or `w:cr` adds to the text: a page, a column or a line break (a line
/// break with `w:clear` is U+2028).
fn break_char(child: Element<'_>) -> char {
    let clear = truthy(child.attr("w:clear"));
    match child.attr("w:type") {
        Some("page") => '\u{c}',
        Some("column") => '\u{b}',
        _ if clear.is_some_and(|c| c != "none") => '\u{2028}',
        _ => '\n',
    }
}

/// `collectInstrText(el)`: the text of every `w:instrText` below the element (iterative).
fn collect_instr_text(el: Element<'_>) -> String {
    let mut out = String::new();
    let mut stack: Vec<IntoIter<Element<'_>>> = vec![el.child_elements().collect::<Vec<_>>().into_iter()];
    while let Some(top) = stack.last_mut() {
        match top.next() {
            None => {
                stack.pop();
            }
            Some(c) if c.name() == "w:instrText" => out.push_str(&c.text_content()),
            Some(c) => stack.push(c.child_elements().collect::<Vec<_>>().into_iter()),
        }
    }
    out
}
