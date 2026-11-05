//! The text operations of the editing model (docx-core `model-ops.ts`, ported 1:1): over the
//! paragraphs of one story (the TS `DocumentModel.paragraphs`), positions in UTF-16 code units
//! (G6, A1.4).
//!
//! - formatting part of a run splits it at the exact limits;
//! - Enter splits the paragraph; Backspace at the boundary merges (the result takes the props of
//!   the SECOND, identity at the final ¶ mark);
//! - adjacent runs with identical props are normalised (merged), as Word does on save.
//!
//! **Identity** (G22, A3 "Identity rule per op"): a paragraph mutated in place keeps its
//! [`ParaId`] (`paras.make_mut(i)`); a new TS paragraph object (the `tail` literal of
//! [`split_paragraph`]) takes a new one from the document's [`IdAlloc`]. Runs and props are values
//! (props aliasing is never pointer identity, P2): the TS symbol keys on the props travel as
//! [`Run::rpr_link`] wherever the TS spreads the props.
//!
//! **Errors**: where the TS would throw (`model.paragraphs[i]!` undefined), the operation returns
//! [`OpError::Position`] **before** changing anything (the TS can throw after a partial change;
//! declared D2a-T1). An offset past the end of a paragraph is not an error (the TS clamps by its
//! own loops), and the same here.

use std::collections::HashSet;
use std::sync::Arc;

use crate::ids::{IdAlloc, ParaId};
use crate::js::{JsAssign, JsObject, JsValue, JsonWriter, Slot};
use crate::model::para::{ParaProps, Paragraph};
use crate::model::props::RPr;
use crate::model::run::{RprLink, Run, RunExt, RunProps};
use crate::pvec::PVec;
use crate::read::plain::default_run_props;
use crate::read::props_map::{edit_base_props, para_props_from_ppr};
use crate::text::Utf16;
use crate::word::obj;
use crate::write::emit::run::is_mark_run;
use crate::write::lossless::{cleared_ppr, cleared_rpr};

/// A position in a story: paragraph index and offset in its concatenated text (UTF-16 units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DocPosition {
    pub para: u32,
    pub offset: u32,
}

impl DocPosition {
    #[must_use]
    pub const fn new(para: u32, offset: u32) -> DocPosition {
        DocPosition { para, offset }
    }
}

/// A range (`DocRange`); `start` may come after `end` (see [`normalize_range`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DocRange {
    pub start: DocPosition,
    pub end: DocPosition,
}

impl DocRange {
    #[must_use]
    pub const fn new(start: DocPosition, end: DocPosition) -> DocRange {
        DocRange { start, end }
    }
}

/// Why an operation did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpError {
    /// a paragraph index outside the story (the TS throws a `TypeError`)
    Position,
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::Position => f.write_str("a position outside the story"),
        }
    }
}

impl std::error::Error for OpError {}

pub type OpResult<T> = Result<T, OpError>;

/// Run props as a TS object carries them: the props and the symbol keys the spread copies with
/// them ([`Run::rpr_link`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CarriedProps {
    pub props: RunProps,
    pub link: Option<Box<RprLink>>,
}

impl CarriedProps {
    /// Props without symbol keys (a TS object literal).
    #[must_use]
    pub fn plain(props: RunProps) -> CarriedProps {
        CarriedProps { props, link: None }
    }

    /// The props of a run, as `{ ...run.props }` copies them.
    #[must_use]
    pub fn of_run(run: &Run) -> CarriedProps {
        CarriedProps { props: (*run.props).clone(), link: run.rpr_link.clone() }
    }
}

/// `paraText(p)`.
#[must_use]
pub fn para_text(p: &Paragraph) -> Vec<u16> {
    let mut out = Vec::with_capacity(para_length(p));
    for r in &p.runs {
        out.extend_from_slice(r.text.units());
    }
    out
}

/// `paraLength(p)`.
#[must_use]
pub fn para_length(p: &Paragraph) -> usize {
    p.runs.iter().map(|r| r.text.len()).sum()
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `comparePos(a, b)` (the sign).
#[must_use]
pub fn compare_pos(a: DocPosition, b: DocPosition) -> std::cmp::Ordering {
    (a.para, a.offset).cmp(&(b.para, b.offset))
}

/// `normalizeRange(r)`: `start` before `end`.
#[must_use]
pub fn normalize_range(r: DocRange) -> DocRange {
    if compare_pos(r.start, r.end).is_le() { r } else { DocRange { start: r.end, end: r.start } }
}

/// `JSON.stringify(v ?? null)`.
pub(crate) fn json_or_null(v: Option<&dyn JsValue>) -> String {
    v.and_then(|v| JsonWriter::stringify(v, None)).unwrap_or_else(|| "null".to_owned())
}

/// `sameJson(a, b)` / revisions' `same(a, b)`: `a === b || JSON.stringify(a ?? null) ===
/// JSON.stringify(b ?? null)` (equal values have equal JSON: the identity half adds nothing).
pub(crate) fn same_json(a: Option<&dyn JsValue>, b: Option<&dyn JsValue>) -> bool {
    json_or_null(a) == json_or_null(b)
}

fn dynv<T: JsValue>(v: Option<&T>) -> Option<&dyn JsValue> {
    v.map(|x| x as &dyn JsValue)
}

/// `sameRPr(a, b)`: the typed original formatting (`rpr`) is equal (runs with a different `rpr`
/// never merge).
fn same_rpr(a: &Run, b: &Run) -> bool {
    match (&a.rpr, &b.rpr) {
        (Some(x), Some(y)) if Arc::ptr_eq(x, y) => true,
        (None, None) => true,
        _ => same_json(dynv(a.rpr.as_deref()), dynv(b.rpr.as_deref())),
    }
}

/// `x === y` on two optional TS numbers (`undefined`, `null`, a number; NaN never equal).
fn num_eq(a: Option<crate::js::JsNumber>, b: Option<crate::js::JsNumber>) -> bool {
    use crate::js::JsNumber::{Null, Num};
    match (a, b) {
        (None, None) | (Some(Null), Some(Null)) => true,
        (Some(Num(x)), Some(Num(y))) => x == y,
        _ => false,
    }
}

/// `(x ?? d) === (y ?? d)` (`null ?? d` is `d`).
fn num_or_eq(a: Option<crate::js::JsNumber>, b: Option<crate::js::JsNumber>, d: f64) -> bool {
    let v = |x: Option<crate::js::JsNumber>| match x {
        Some(crate::js::JsNumber::Num(n)) => n,
        _ => d,
    };
    v(a) == v(b)
}

/// `sameComments(a, b)`: element-wise `===`, except that two `NaN` entries are equal (declared
/// D2a-T2: the TS answers `true` for the same array object, which every split of a run shares).
fn same_comments(a: Option<&Vec<crate::js::JsNumber>>, b: Option<&Vec<crate::js::JsNumber>>) -> bool {
    use crate::js::JsNumber::{Null, Num};
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| match (x, y) {
                    (Null, Null) => true,
                    (Num(x), Num(y)) => x == y || (x.is_nan() && y.is_nan()),
                    _ => false,
                })
        }
        _ => false,
    }
}

/// `sameProps(a, b)` of model-ops.ts, field by field.
#[must_use]
pub fn same_props(a: &RunProps, b: &RunProps) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.underline_style == b.underline_style
        && a.underline_color == b.underline_color
        && same_json(dynv(a.underline_theme.as_ref()), dynv(b.underline_theme.as_ref()))
        && same_json(dynv(a.color_theme.as_ref()), dynv(b.color_theme.as_ref()))
        && same_json(dynv(a.text_fx.as_deref()), dynv(b.text_fx.as_deref()))
        && same_json(dynv(a.open_type.as_deref()), dynv(b.open_type.as_deref()))
        && a.hidden.unwrap_or(false) == b.hidden.unwrap_or(false)
        && num_or_eq(a.scale_pct, b.scale_pct, 100.0)
        && num_or_eq(a.position_half_pt, b.position_half_pt, 0.0)
        && num_or_eq(a.kern_half_pt, b.kern_half_pt, 0.0)
        && a.strike == b.strike
        && a.double_strike == b.double_strike
        && a.vert_align == b.vert_align
        && num_eq(a.sz_half_pt, b.sz_half_pt)
        && a.color == b.color
        && a.highlight == b.highlight
        && a.font_ascii == b.font_ascii
        && a.all_caps == b.all_caps
        && a.small_caps == b.small_caps
        && a.link == b.link
        && a.link_tip == b.link_tip
        && a.char_style == b.char_style
        && a.no_proof.unwrap_or(false) == b.no_proof.unwrap_or(false)
        && a.web_hidden.unwrap_or(false) == b.web_hidden.unwrap_or(false)
        && a.src == b.src
        && same_comments(a.comment_ids.as_ref(), b.comment_ids.as_ref())
        && same_json(dynv(a.ins.as_deref()), dynv(b.ins.as_deref()))
        && same_json(dynv(a.del.as_deref()), dynv(b.del.as_deref()))
        && same_json(dynv(a.fmt.as_deref()), dynv(b.fmt.as_deref()))
}

fn truthy_str(s: Option<&crate::js::Str>) -> Option<&crate::js::Str> {
    s.filter(|s| !s.is_empty())
}

/// `isSpecial(r)`: image, field, note, plugin inline, object, `ptab` or a zero-width mark — runs
/// that never merge and keep their extras.
#[must_use]
pub fn is_special(r: &Run) -> bool {
    let Some(e) = r.ext() else { return false };
    e.image.is_some()
        || truthy_str(e.field.as_ref()).is_some()
        || e.note_ref.is_some()
        || e.custom.is_some()
        || e.textbox.is_some()
        || e.shape.is_some()
        || e.group.is_some()
        || e.ptab.is_some()
        || is_mark_run(r)
}

fn field_of(r: &Run) -> Option<&str> {
    r.ext().and_then(|e| e.field.as_deref())
}

fn has_field_result(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.field_result.is_some())
}

fn concat(a: &Utf16, b: &Utf16) -> Utf16 {
    let mut v = Vec::with_capacity(a.len() + b.len());
    v.extend_from_slice(a.units());
    v.extend_from_slice(b.units());
    Utf16::from_units(v)
}

/// `s.slice(a, b)` with `a ≤ b` already clamped by the caller's arithmetic (`min` to the length).
fn slice(s: &Utf16, a: usize, b: usize) -> Utf16 {
    let u = s.units();
    let b = b.min(u.len());
    let a = a.min(b);
    Utf16::from_units(u.get(a..b).map(<[u16]>::to_vec).unwrap_or_default())
}

/// `normalizeParagraph(p)`: merges identical adjacent runs and drops the empty ones (special runs
/// never merge and keep their extras). Every surviving run is the TS `{ ...run, props: {
/// ...run.props } }`: the same values here.
pub fn normalize_paragraph(p: &mut Paragraph) {
    let runs = std::mem::take(&mut p.runs);
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.text.is_empty() && !is_special(&run) {
            continue;
        }
        let merge = out.last().is_some_and(|last| {
            !is_special(last)
                && !is_special(&run)
                && same_props(&last.props, &run.props)
                && same_rpr(last, &run)
                && !has_field_result(last)
                && !has_field_result(&run)
                && field_of(last) == field_of(&run)
        });
        match out.last_mut() {
            Some(last) if merge => last.text = concat(&last.text, &run.text),
            _ => out.push(run),
        }
    }
    p.runs = out;
}

/// `textExtras(run)`: the TEXT extras each piece of a split run keeps (`field`, `fieldOpen`,
/// `rpr`, `symFont`, `inToc`, each only when truthy); with the props given.
fn text_piece(run: &Run, text: Utf16, props: Arc<RunProps>) -> Run {
    let e = run.ext();
    let field = e.and_then(|e| truthy_str(e.field.as_ref()).cloned());
    let field_open = e.is_some_and(|e| e.field_open == Some(true));
    let sym_font = e.and_then(|e| truthy_str(e.sym_font.as_ref()).cloned());
    let in_toc = e.is_some_and(|e| e.in_toc == Some(true));
    let ext = (field.is_some() || field_open || sym_font.is_some() || in_toc).then(|| {
        Box::new(RunExt { field, field_open: field_open.then_some(true), sym_font, in_toc: in_toc.then_some(true), ..RunExt::default() })
    });
    let mut out = Run { text, props, rpr: run.rpr.clone(), ext, rpr_link: run.rpr_link.clone(), ..Run::default() };
    // `{ ...textExtras(run), text, props }`: the TS key order of the new object is `rpr` (an
    // extra) before `text` and `props` — what the order-sensitive TS run signature sees (U6, G23;
    // the extras of `RunExt` keep their own order)
    if out.rpr.is_some() {
        let k = |n: &str| <Run as crate::js::JsObject>::key_index(n);
        if let (Some(r), Some(t), Some(p)) = (k("rpr"), k("text"), k("props")) {
            obj::set_key_order(&mut out, &[r, t, p]);
        }
    }
    out
}

/// `splitAt(p, offset)`: splits the run that contains `offset`; the index of the run that starts
/// there.
pub fn split_at(p: &mut Paragraph, offset: usize) -> usize {
    let mut acc = 0usize;
    for i in 0..p.runs.len() {
        let Some(run) = p.runs.get(i) else { break };
        if offset == acc {
            return i;
        }
        let len = run.text.len();
        if offset < acc + len {
            let k = offset - acc;
            let head = text_piece(run, slice(&run.text, 0, k), Arc::clone(&run.props));
            let tail = text_piece(run, slice(&run.text, k, len), Arc::clone(&run.props));
            p.runs.splice(i..=i, [head, tail]);
            return i + 1;
        }
        acc += len;
    }
    p.runs.len()
}

/// `withoutRevisions(props)`: a copy without the revision marks (`ins`, `del`, `fmt`).
#[must_use]
pub fn without_revisions(props: &RunProps) -> RunProps {
    let mut out = props.clone();
    obj::delete(&mut out, "ins");
    obj::delete(&mut out, "del");
    obj::delete(&mut out, "fmt");
    out
}

/// `propsAt(model, pos)`: the props at an insertion point (the previous character's, as Word;
/// revisions do not pass on; zero-width marks have no formatting).
#[must_use]
pub fn props_at(paras: &PVec<Paragraph>, pos: DocPosition) -> CarriedProps {
    let runs: Vec<&Run> = paras.get(pos.para as usize).map(|p| p.runs.iter().filter(|r| !is_mark_run(r)).collect()).unwrap_or_default();
    let Some(last) = runs.last() else { return CarriedProps::plain(default_run_props()) };
    let offset = pos.offset as usize;
    let mut acc = 0usize;
    for run in &runs {
        if offset <= acc + run.text.len() {
            return CarriedProps { props: without_revisions(&run.props), link: run.rpr_link.clone() };
        }
        acc += run.text.len();
    }
    CarriedProps { props: without_revisions(&last.props), link: last.rpr_link.clone() }
}

/// `insertIndex(p, offset)`: where a new run enters at `offset`: after the bookmark marks and the
/// field ends there, before a field begin.
pub fn insert_index(p: &mut Paragraph, offset: usize) -> usize {
    let mut idx = split_at(p, offset);
    while p.runs.get(idx).and_then(Run::ext).is_some_and(|e| e.bookmark.is_some() || e.field_end == Some(true)) {
        idx += 1;
    }
    idx
}

/// `rprAt(model, pos)`: the `rpr` of the run the text enters (the previous one, as `propsAt`).
fn rpr_at(paras: &PVec<Paragraph>, pos: DocPosition) -> Option<Arc<RPr>> {
    let p = paras.get(pos.para as usize)?;
    if p.runs.is_empty() {
        return None;
    }
    let offset = pos.offset as usize;
    let mut acc = 0usize;
    for run in &p.runs {
        if offset <= acc + run.text.len() && !is_special(run) {
            return run.rpr.clone();
        }
        acc += run.text.len();
    }
    p.runs.iter().rev().find(|r| !is_special(r)).and_then(|r| r.rpr.clone())
}

fn check(paras: &PVec<Paragraph>, para: u32) -> OpResult<usize> {
    let i = para as usize;
    if i < paras.len() { Ok(i) } else { Err(OpError::Position) }
}

fn para_mut(paras: &mut PVec<Paragraph>, i: usize) -> OpResult<&mut Paragraph> {
    paras.make_mut(i).ok_or(OpError::Position)
}

fn advanced(pos: DocPosition, by: usize) -> DocPosition {
    DocPosition { para: pos.para, offset: u32_of((pos.offset as usize).saturating_add(by)) }
}

/// `insertText(model, pos, text, props?)`: the new text inherits the props (and the typed `rpr`)
/// of the neighbouring run.
pub fn insert_text(paras: &mut PVec<Paragraph>, pos: DocPosition, text: &[u16], props: Option<&CarriedProps>) -> OpResult<DocPosition> {
    let i = check(paras, pos.para)?;
    let effective = props.cloned().unwrap_or_else(|| props_at(paras, pos));
    let rpr = rpr_at(paras, pos);
    let p = para_mut(paras, i)?;
    let idx = insert_index(p, pos.offset as usize);
    let run =
        Run { text: Utf16::from_units(text.to_vec()), props: Arc::new(effective.props), rpr, rpr_link: effective.link, ..Run::default() };
    p.runs.insert(idx.min(p.runs.len()), run);
    normalize_paragraph(p);
    Ok(advanced(pos, text.len()))
}

/// `insertRun(model, pos, run)`: a whole run (image, field, plugin inline) without losing its
/// special data.
pub fn insert_run(paras: &mut PVec<Paragraph>, pos: DocPosition, run: Run) -> OpResult<DocPosition> {
    let i = check(paras, pos.para)?;
    let len = run.text.len();
    let p = para_mut(paras, i)?;
    let idx = insert_index(p, pos.offset as usize);
    p.runs.insert(idx.min(p.runs.len()), run);
    normalize_paragraph(p);
    Ok(advanced(pos, len))
}

/// `splitParagraph(model, pos)` (Enter): the NEW paragraph (a new id) takes the second half; the
/// original keeps the first. The paragraph props are copied; the section break and the mark's
/// revision belong to the second half.
pub fn split_paragraph(paras: &mut PVec<Paragraph>, ids: &mut IdAlloc, pos: DocPosition) -> OpResult<DocPosition> {
    let i = check(paras, pos.para)?;
    let tail = {
        let p = para_mut(paras, i)?;
        let idx = insert_index(p, pos.offset as usize);
        let tail_runs = p.runs.split_off(idx.min(p.runs.len()));
        let mut tail = Paragraph { id: ids.para(), runs: tail_runs, props: Arc::clone(&p.props), ..Paragraph::default() };
        if let Some(ppr) = &p.ppr {
            obj::set(&mut tail, "ppr", |t| t.ppr = Some(Arc::clone(ppr)));
        }
        if p.ppr_edited == Some(true) {
            obj::set(&mut tail, "pprEdited", |t| t.ppr_edited = Some(true));
        }
        if p.props.section_break.is_some() {
            let mut props = (*p.props).clone();
            obj::delete(&mut props, "sectionBreak");
            p.props = Arc::new(props);
        }
        if p.props.mark_ins.is_some() || p.props.mark_del.is_some() {
            let mut props = (*p.props).clone();
            obj::delete(&mut props, "markIns");
            obj::delete(&mut props, "markDel");
            p.props = Arc::new(props);
        }
        normalize_paragraph(p);
        tail
    };
    let mut tail = tail;
    normalize_paragraph(&mut tail);
    paras.insert(i + 1, Arc::new(tail));
    Ok(DocPosition { para: pos.para.saturating_add(1), offset: 0 })
}

/// A run of the story by its place before the operation (the TS `Set<Run>` of object identities:
/// runs are values here, and no run object is shared between two places of a story).
type RunAt = (usize, usize);

/// `doomedMarks(model, start, end)`: the marks that go with `[start, end]` — a bookmark wholly
/// inside (a point bookmark only strictly inside), and a field whose begin goes, with its end.
///
/// The TS walks the whole story; this walks only what can change the answer (the same result,
/// D2a-T5; the per-keystroke budget of U19 on a document with thousands of bookmarks and fields):
/// - a bookmark goes only when its start and end both lie in `[start, end]`, so only the
///   paragraphs of the range are walked for bookmarks (the latest start of an id before an end in
///   the range is in the range, or the pair does not go);
/// - fields pair by a stack: a pair whose begin lies before `start` never goes, and such outer
///   begins only ever sit under the begins of the range, so the walk starts at `start.para` (an
///   end that finds the stack empty is an outer pair) and stops past `end.para` once no begin of
///   the range is left on the stack.
fn doomed_marks(paras: &PVec<Paragraph>, start: DocPosition, end: DocPosition) -> HashSet<RunAt> {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum BmKey {
        Num(crate::js::NumKey),
        Null,
    }
    let mut out = HashSet::new();
    if compare_pos(start, end).is_ge() {
        return out;
    }
    let mut starts: std::collections::HashMap<BmKey, (RunAt, DocPosition)> = std::collections::HashMap::new();
    // (run, at, text length, begin at or after `start`)
    let mut stack: Vec<(RunAt, DocPosition, usize, bool)> = Vec::new();
    let mut in_range_begins = 0usize;
    let first = start.para as usize;
    let last = end.para as usize;
    for pi in first..paras.len() {
        if pi > last && in_range_begins == 0 {
            break;
        }
        let Some(p) = paras.get(pi) else { break };
        let mut offset = 0usize;
        for (ri, r) in p.runs.iter().enumerate() {
            let at = DocPosition { para: u32_of(pi), offset: u32_of(offset) };
            offset += r.text.len();
            let Some(e) = r.ext() else { continue };
            if let Some(bm) = e.bookmark.as_ref() {
                if pi > last {
                    continue;
                }
                let key = match bm.id {
                    crate::js::JsNumber::Num(x) => BmKey::Num(crate::js::NumKey::new(x)),
                    crate::js::JsNumber::Null => BmKey::Null,
                };
                if bm.end != Some(true) {
                    starts.insert(key, ((pi, ri), at));
                } else if let Some(&(s_run, s_at)) = starts.get(&key) {
                    let inside = |x: DocPosition| compare_pos(x, start).is_ge() && compare_pos(x, end).is_le();
                    let point = compare_pos(s_at, at).is_eq();
                    let gone =
                        if point { compare_pos(at, start).is_gt() && compare_pos(at, end).is_lt() } else { inside(s_at) && inside(at) };
                    if gone {
                        out.insert(s_run);
                        out.insert((pi, ri));
                    }
                }
            } else if e.field_begin.is_some() || e.field_open == Some(true) {
                let mine = compare_pos(at, start).is_ge();
                in_range_begins += usize::from(mine);
                stack.push(((pi, ri), at, r.text.len(), mine));
            } else if e.field_end == Some(true) {
                let Some((b_run, b_at, b_len, mine)) = stack.pop() else { continue };
                in_range_begins -= usize::from(mine);
                if !mine {
                    continue;
                }
                let gone = if b_len > 0 { compare_pos(advanced(b_at, b_len), end).is_le() } else { compare_pos(at, end).is_le() };
                if gone {
                    out.insert(b_run);
                    out.insert((pi, ri));
                }
            }
        }
    }
    out
}

/// `rebuildDelete(p, from, to, doomed)`: removes `[from, to)` rebuilding the runs; zero-width marks
/// in the range stay (at the point of the deletion), except the doomed ones. `pi` is the
/// paragraph's index before the operation.
fn rebuild_delete(runs: &[Run], pi: usize, from: usize, to: usize, doomed: &HashSet<RunAt>) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len());
    let mut acc = 0usize;
    for (ri, run) in runs.iter().enumerate() {
        let rs = acc;
        let re = acc + run.text.len();
        acc = re;
        if is_mark_run(run) || (run.text.is_empty() && is_special(run)) {
            if !doomed.contains(&(pi, ri)) {
                out.push(run.clone());
            }
            continue;
        }
        let head_end = from.min(re).saturating_sub(rs);
        let units = run.text.units();
        let head = units.get(..head_end.min(units.len())).unwrap_or(&[]);
        let tail: &[u16] = if re > to { units.get(to.saturating_sub(rs).min(units.len())..).unwrap_or(&[]) } else { &[] };
        if head.len() + tail.len() == 0 {
            continue;
        }
        let mut text = Vec::with_capacity(head.len() + tail.len());
        text.extend_from_slice(head);
        text.extend_from_slice(tail);
        let text = Utf16::from_units(text);
        if text.len() == run.text.len() {
            out.push(Run { text, ..run.clone() });
        } else {
            out.push(text_piece(run, text, Arc::clone(&run.props)));
        }
    }
    out
}

/// `dropDoomed(model, doomed)` for the paragraphs that were at `orig` before the operation and are
/// at `now` after it.
fn drop_doomed_in(p: &mut Paragraph, orig: usize, doomed: &HashSet<RunAt>) {
    let mut ri = 0usize;
    p.runs.retain(|_| {
        let keep = !doomed.contains(&(orig, ri));
        ri += 1;
        keep
    });
}

fn has_doomed(p: &Paragraph, orig: usize, doomed: &HashSet<RunAt>) -> bool {
    (0..p.runs.len()).any(|ri| doomed.contains(&(orig, ri)))
}

/// `deleteRange(model, range)`.
pub fn delete_range(paras: &mut PVec<Paragraph>, range: DocRange) -> OpResult<DocPosition> {
    let DocRange { start, end } = normalize_range(range);
    let s = check(paras, start.para)?;
    let e = check(paras, end.para)?;
    let doomed = doomed_marks(paras, start, end);
    let drop_after = |paras: &mut PVec<Paragraph>, from_orig: usize, shift: usize| {
        if doomed.is_empty() {
            return;
        }
        for orig in from_orig..paras.len() + shift {
            let now = orig - shift;
            if paras.get(now).is_some_and(|p| has_doomed(p, orig, &doomed))
                && let Some(p) = paras.make_mut(now)
            {
                drop_doomed_in(p, orig, &doomed);
            }
        }
    };
    if s == e {
        let p = para_mut(paras, s)?;
        p.runs = rebuild_delete(&p.runs, s, start.offset as usize, end.offset as usize, &doomed);
        drop_after(paras, s + 1, 0);
        let p = para_mut(paras, s)?;
        normalize_paragraph(p);
        return Ok(start);
    }
    let first_len = paras.get(s).map_or(0, para_length);
    let mut runs = paras.get(s).map(|p| rebuild_delete(&p.runs, s, start.offset as usize, first_len, &doomed)).unwrap_or_default();
    for pi in s + 1..e {
        if let Some(p) = paras.get(pi) {
            for (ri, r) in p.runs.iter().enumerate() {
                if (is_mark_run(r) || (r.text.is_empty() && is_special(r))) && !doomed.contains(&(pi, ri)) {
                    runs.push(r.clone());
                }
            }
        }
    }
    let (last_runs, last_props, last_ppr) = match paras.get(e) {
        Some(last) => (rebuild_delete(&last.runs, e, 0, end.offset as usize, &doomed), Arc::clone(&last.props), last.ppr.clone()),
        None => return Err(OpError::Position),
    };
    runs.extend(last_runs);
    {
        let first = para_mut(paras, s)?;
        first.runs = runs;
        first.props = last_props;
        match last_ppr {
            Some(ppr) => obj::set(first, "ppr", |f| f.ppr = Some(ppr)),
            None => obj::delete(first, "ppr"),
        }
    }
    for _ in s + 1..=e {
        paras.remove(s + 1);
    }
    drop_after(paras, e + 1, e - s);
    let first = para_mut(paras, s)?;
    normalize_paragraph(first);
    Ok(start)
}

/// `mergeWithPrevious(model, para)` (Backspace at the start): the identity of the SECOND survives.
pub fn merge_with_previous(paras: &mut PVec<Paragraph>, para: u32) -> OpResult<DocPosition> {
    let i = check(paras, para)?;
    if i == 0 {
        return Err(OpError::Position);
    }
    let Some(cur) = paras.get(i).cloned() else { return Err(OpError::Position) };
    let prev = para_mut(paras, i - 1)?;
    let offset = para_length(prev);
    prev.runs.extend(cur.runs);
    prev.props = cur.props;
    match cur.ppr {
        Some(ppr) => obj::set(prev, "ppr", |p| p.ppr = Some(ppr)),
        None => obj::delete(prev, "ppr"),
    }
    if cur.ppr_edited == Some(true) {
        obj::set(prev, "pprEdited", |p| p.ppr_edited = Some(true));
    } else {
        obj::delete(prev, "pprEdited");
    }
    paras.remove(i);
    let prev = para_mut(paras, i - 1)?;
    normalize_paragraph(prev);
    Ok(DocPosition { para: para - 1, offset: u32_of(offset) })
}

/// The `[from, to)` of paragraph `pi` inside a normalised range.
fn span_in(p: &Paragraph, pi: usize, start: DocPosition, end: DocPosition) -> (usize, usize) {
    let from = if pi == start.para as usize { start.offset as usize } else { 0 };
    let to = if pi == end.para as usize { end.offset as usize } else { para_length(p) };
    (from, to)
}

fn check_range(paras: &PVec<Paragraph>, start: DocPosition, end: DocPosition) -> OpResult<()> {
    check(paras, start.para)?;
    check(paras, end.para)?;
    Ok(())
}

/// The runs of `[from, to)` after the splits (`i1` and the walk of `applyRunProps`).
fn for_runs_in(p: &mut Paragraph, from: usize, to: usize, mut f: impl FnMut(&mut Run)) {
    split_at(p, to);
    let i1 = split_at(p, from);
    let mut acc = 0usize;
    for (ri, run) in p.runs.iter_mut().enumerate() {
        let rs = acc;
        acc += run.text.len();
        if ri < i1 {
            continue;
        }
        if rs >= to {
            break;
        }
        f(run);
    }
}

/// `applyRunProps(model, range, patch)`: `Object.assign` of the patch on the props of every run of
/// the range (split at its limits).
pub fn apply_run_props(paras: &mut PVec<Paragraph>, range: DocRange, patch: &RunProps) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    check_range(paras, start, end)?;
    for pi in start.para as usize..=end.para as usize {
        let Some((from, to)) = paras.get(pi).map(|p| span_in(p, pi, start, end)) else { return Err(OpError::Position) };
        if to <= from {
            continue;
        }
        let p = para_mut(paras, pi)?;
        for_runs_in(p, from, to, |run| obj::assign(Arc::make_mut(&mut run.props), patch));
        normalize_paragraph(p);
    }
    Ok(())
}

/// `applyParaProps(model, range, patch)`: `Object.assign` on the props of every paragraph.
pub fn apply_para_props(paras: &mut PVec<Paragraph>, range: DocRange, patch: &ParaProps) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    check_range(paras, start, end)?;
    for pi in start.para as usize..=end.para as usize {
        let p = para_mut(paras, pi)?;
        obj::assign(Arc::make_mut(&mut p.props), patch);
    }
    Ok(())
}

/// `selectionRunProps(model, range)`: the props after the first character (toolbar state).
#[must_use]
pub fn selection_run_props(paras: &PVec<Paragraph>, range: DocRange) -> CarriedProps {
    let start = normalize_range(range).start;
    props_at(paras, advanced(start, 1))
}

/// `charStyle` of [`clear_run_formatting`]: a new character style (`string`), none (`undefined`:
/// removes it) or the current one (`null`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CharStyleOpt {
    Set(crate::js::Str),
    #[default]
    Remove,
    Keep,
}

/// The options of `clearRunFormatting`.
#[derive(Debug, Clone, Default)]
pub struct ClearOptions {
    /// only these props (TS names); `None` = all the direct formatting
    pub fields: Option<Vec<&'static str>>,
    pub char_style: CharStyleOpt,
}

/// `next[k] = from[k]` with the JS rules (an `undefined` value is still a key).
pub(crate) fn copy_key<T: JsAssign>(next: &mut T, from: &T, key: &str) {
    let Some(i) = T::key_index(key) else { return };
    let i = usize::from(i);
    match from.slot(i) {
        Slot::Val(_) => obj::set(next, key, |n| n.assign_slot(from, i)),
        Slot::Undefined | Slot::Absent => obj::set_undefined(next, key, |n| n.remove_slot(i)),
    }
}

/// `clearRunFormatting(model, range, opts)`: removes the DIRECT formatting of the runs of the range
/// (or only `opts.fields`), as Word: the typed `rpr` loses the elements and the props return to
/// what the rest of it implies.
pub fn clear_run_formatting(paras: &mut PVec<Paragraph>, range: DocRange, opts: &ClearOptions) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    check_range(paras, start, end)?;
    for pi in start.para as usize..=end.para as usize {
        let Some((from, to)) = paras.get(pi).map(|p| span_in(p, pi, start, end)) else { return Err(OpError::Position) };
        if to <= from {
            continue;
        }
        let p = para_mut(paras, pi)?;
        for_runs_in(p, from, to, |run| clear_run(run, opts));
        normalize_paragraph(p);
    }
    Ok(())
}

/// `clearRun(run, opts)`.
pub fn clear_run(run: &mut Run, opts: &ClearOptions) {
    let rpr = cleared_rpr(run.rpr.as_deref(), opts.fields.as_deref());
    let old = Arc::clone(&run.props);
    let base = edit_base_props(run, Some(&rpr));
    let mut next = if opts.fields.is_some() { (*old).clone() } else { base.clone() };
    if let Some(fields) = &opts.fields {
        for k in fields {
            copy_key(&mut next, &base, k);
        }
    }
    for k in ["src", "link", "isMath", "commentIds"] {
        if RunProps::key_index(k).is_some_and(|i| matches!(old.slot(usize::from(i)), Slot::Val(_))) {
            copy_key(&mut next, &old, k);
        }
    }
    match &opts.char_style {
        CharStyleOpt::Set(s) => obj::set(&mut next, "charStyle", |n| n.char_style = Some(s.clone())),
        CharStyleOpt::Remove if opts.fields.is_none() => obj::delete(&mut next, "charStyle"),
        CharStyleOpt::Remove => {}
        CharStyleOpt::Keep => match &old.char_style {
            Some(c) => obj::set(&mut next, "charStyle", |n| n.char_style = Some(c.clone())),
            None => obj::delete(&mut next, "charStyle"),
        },
    }
    // `{ ...base }` is a new object without the symbol keys; `{ ...old }` keeps them
    if opts.fields.is_none() {
        run.rpr_link = None;
    }
    obj::set(run, "rpr", |r| r.rpr = Some(Arc::new(rpr)));
    obj::set(run, "rprEdited", |r| r.rpr_edited = Some(true));
    run.props = Arc::new(next);
}

/// `clearParaFormatting(model, range)`: removes the direct PARAGRAPH formatting (the style, the
/// mark and the section stay).
pub fn clear_para_formatting(paras: &mut PVec<Paragraph>, range: DocRange) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    check_range(paras, start, end)?;
    for pi in start.para as usize..=end.para as usize {
        clear_para(para_mut(paras, pi)?);
    }
    Ok(())
}

/// `clearPara(p)`.
pub fn clear_para(p: &mut Paragraph) {
    let ppr = cleared_ppr(p.ppr.as_deref());
    let old = Arc::clone(&p.props);
    let mut next = para_props_from_ppr(Some(&ppr));
    match &old.style_id {
        Some(s) => obj::set(&mut next, "styleId", |n| n.style_id = Some(s.clone())),
        None => obj::delete(&mut next, "styleId"),
    }
    for k in ["src", "sectionBreak", "tocInstr", "tocEntry", "blockSdt"] {
        if ParaProps::key_index(k).is_some_and(|i| matches!(old.slot(usize::from(i)), Slot::Val(_))) {
            copy_key(&mut next, &old, k);
        }
    }
    obj::set(p, "ppr", |q| q.ppr = Some(Arc::new(ppr)));
    obj::set(p, "pprEdited", |q| q.ppr_edited = Some(true));
    p.props = Arc::new(next);
}

/// The id of the paragraph at `i` (tests and the session's change sets).
#[must_use]
pub fn para_id(paras: &PVec<Paragraph>, i: usize) -> Option<ParaId> {
    paras.get(i).map(|p| p.id)
}
