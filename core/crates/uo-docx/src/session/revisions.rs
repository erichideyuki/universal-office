//! Tracked changes in the editing model (docx-core `revisions.ts`, ported 1:1): list, accept and
//! reject the revisions (`w:ins`, `w:del`, `w:moveFrom/To`, `w:rPrChange`, `w:pPrChange` and the
//! paragraph marks) and edit with tracking ON. The rules are Word's, measured through COM:
//!
//! - typed text becomes an insertion; next to an insertion of the same author it extends it;
//!   inside another author's insertion it splits it;
//! - deleting one's OWN insertion deletes for real; the rest becomes a deletion (another author's
//!   insertion stays inserted AND deleted);
//! - Enter: the NEW mark is the first paragraph's;
//! - joining paragraphs deletes the first one's mark; if it has text, the second receives its
//!   formatting (`w:pPrChange` keeps the one before);
//! - formatting keeps the props from BEFORE (the first); returning to them undoes the revision;
//! - accepting a deleted mark / rejecting an inserted one joins the paragraphs with the second's
//!   formatting.
//!
//! Object identity, ids and errors follow [`super::text`]. The JSON comparisons of the TS
//! (`same`) are order-sensitive (`JSON.stringify` with the insertion order of the keys), as here.

use std::sync::Arc;

use crate::ids::IdAlloc;
use crate::js::{JsNumber, JsObject, JsValue, JsonWriter, Slot, Str};
use crate::js_object;
use crate::model::para::{ParaFormatChange, ParaProps, Paragraph};
use crate::model::props::RevisionInfo;
use crate::model::run::{FormatChange, Run, RunProps};
use crate::model::story::{Block, Story};
use crate::pvec::PVec;
use crate::read::props_map::{edit_base_props, para_props_from_ppr, rev_info, run_props_from_rpr};
use crate::text::Utf16;
use crate::word::obj;

use super::env::LocalNow;
use super::text::{
    CarriedProps, DocPosition, DocRange, OpError, OpResult, copy_key, insert_run, insert_text, normalize_paragraph, normalize_range,
    para_length, props_at, split_at, split_paragraph, without_revisions,
};

/// `newRevision(author, now)`: the authorship of a revision made NOW: `w:date` in Word's
/// convention (local time, seconds zeroed, a `Z` suffix) and `w16du:dateUtc` (UTC).
#[must_use]
pub fn new_revision(author: &str, now: &LocalNow) -> RevisionInfo {
    RevisionInfo {
        author: author.into(),
        date: Some(now.local.word_date().into()),
        date_utc: Some(now.utc.word_date().into()),
        ..RevisionInfo::default()
    }
}

/// `RUN_META`: the RunProps fields that are not formatting.
pub const RUN_META: [&str; 7] = ["src", "link", "commentIds", "isMath", "ins", "del", "fmt"];
/// `PARA_META`: the ParaProps fields that are not formatting.
pub const PARA_META: [&str; 9] = ["src", "fmtSrc", "sectionBreak", "markIns", "markDel", "pfmt", "tocInstr", "tocEntry", "blockSdt"];

/// `runFormatting(p)`: only the formatting of a run (what a `w:rPrChange` keeps).
#[must_use]
pub fn run_formatting(p: &RunProps) -> RunProps {
    let mut out = p.clone();
    for k in RUN_META {
        obj::delete(&mut out, k);
    }
    out
}

/// `paraFormatting(p)`: only the formatting of a paragraph (what a `w:pPrChange` keeps).
#[must_use]
pub fn para_formatting(p: &ParaProps) -> ParaProps {
    let mut out = p.clone();
    for k in PARA_META {
        obj::delete(&mut out, k);
    }
    out
}

fn has_val<T: JsObject>(o: &T, k: &str) -> bool {
    T::key_index(k).is_some_and(|i| matches!(o.slot(usize::from(i)), Slot::Val(_)))
}

/// `runMeta(p)`: the meta keys that hold a value, in `RUN_META` order.
fn run_meta(p: &RunProps) -> RunProps {
    let mut out = RunProps::default();
    for k in RUN_META {
        if has_val(p, k) {
            copy_key(&mut out, p, k);
        }
    }
    out
}

/// `paraMeta(p)`.
fn para_meta(p: &ParaProps) -> ParaProps {
    let mut out = ParaProps::default();
    for k in PARA_META {
        if has_val(p, k) {
            copy_key(&mut out, p, k);
        }
    }
    out
}

fn json(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap_or_else(|| "null".to_owned())
}

/// `same(a, b)` over two objects (`JSON.stringify`, insertion order).
fn same<T: JsValue>(a: &T, b: &T) -> bool {
    json(a) == json(b)
}

/// The authorship of anything that has one (`revInfo` reads `author`, `date`, `dateUtc`, `move`).
trait Authored {
    fn info(&self) -> RevisionInfo;
    fn author(&self) -> &str;
}

macro_rules! authored {
    ($($t:ty),*) => {
        $(impl Authored for $t {
            fn info(&self) -> RevisionInfo {
                rev_info(&RevisionInfo {
                    author: self.author.clone(),
                    date: self.date.clone(),
                    date_utc: self.date_utc.clone(),
                    r#move: self.r#move,
                    ..RevisionInfo::default()
                })
            }
            fn author(&self) -> &str {
                &self.author
            }
        })*
    };
}
authored!(RevisionInfo, FormatChange, ParaFormatChange, crate::model::props::RPrChange, crate::model::props::PPrChange);

/// `{ ...rev, old }`: `rev` is always a [`new_revision`] (`author`, `date`, `dateUtc`, the
/// declaration order).
fn format_change(rev: &RevisionInfo, old: RunProps, old_link: Option<Box<crate::model::run::RprLink>>) -> FormatChange {
    FormatChange {
        author: rev.author.clone(),
        date: rev.date.clone(),
        date_utc: rev.date_utc.clone(),
        r#move: rev.r#move,
        old: Box::new(old),
        old_link,
        ..FormatChange::default()
    }
}

fn para_format_change(rev: &RevisionInfo, old: ParaProps) -> ParaFormatChange {
    ParaFormatChange {
        author: rev.author.clone(),
        date: rev.date.clone(),
        date_utc: rev.date_utc.clone(),
        r#move: rev.r#move,
        old: Box::new(old),
        ..ParaFormatChange::default()
    }
}

/// `isDeletedRun(r)`.
#[must_use]
pub fn is_deleted_run(r: &Run) -> bool {
    r.props.del.is_some()
}

/// `finalRuns(p)`: the runs of the FINAL view (without the deleted text).
#[must_use]
pub fn final_runs(p: &Paragraph) -> Vec<Run> {
    p.runs.iter().filter(|r| r.props.del.is_none()).cloned().collect()
}

/// `finalParagraphs(paras)`: the FINAL view of a paragraph list (as if everything were accepted):
/// no deleted text, and the paragraph of a deleted mark joined to the next with the next's
/// formatting. `index[i]` is the place, in the output, of the paragraph that receives the text of
/// the i-th. The output paragraphs keep their ids (they are derived values, not model objects).
#[must_use]
pub fn final_paragraphs<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> (Vec<Paragraph>, Vec<u32>) {
    let paras: Vec<&Paragraph> = paras.into_iter().collect();
    let mut out: Vec<Paragraph> = Vec::new();
    let mut index: Vec<u32> = Vec::with_capacity(paras.len());
    let mut carry: Vec<Run> = Vec::new();
    let n = paras.len();
    for (i, p) in paras.into_iter().enumerate() {
        let own = final_runs(p);
        let runs = if carry.is_empty() { own } else { std::mem::take(&mut carry).into_iter().chain(own).collect() };
        index.push(u32::try_from(out.len()).unwrap_or(u32::MAX));
        if p.props.mark_del.is_some() && i + 1 < n {
            carry = runs;
            continue;
        }
        out.push(Paragraph { runs, ..p.clone() });
    }
    (out, index)
}

/// `finalBlocks(blocks)` over a story (paragraphs between tables, and inside the cells). A cell is
/// one story here: its `paragraphs` view is the paragraphs of its final blocks (declared D2a-R1:
/// the TS computes `finalParagraphs(cell.paragraphs)` across the cell's nested tables, a view that
/// differs only when a deleted mark is followed by a nested table).
#[must_use]
pub fn final_story(story: &Story) -> Story {
    let mut out = Story::new();
    let mut run: Vec<&Paragraph> = Vec::new();
    let flush = |run: &mut Vec<&Paragraph>, out: &mut Story| {
        if run.is_empty() {
            return;
        }
        for p in final_paragraphs(run.drain(..)).0 {
            out.push_paragraph(p);
        }
    };
    for b in story.blocks() {
        match b {
            Block::Paragraph(p) => run.push(p),
            Block::Table(t) => {
                flush(&mut run, &mut out);
                let mut t = t.clone();
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        cell.content = final_story(&cell.content);
                    }
                }
                out.push_table(Arc::new(t));
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// `paragraphHasRevisions(p)`.
#[must_use]
pub fn paragraph_has_revisions(p: &Paragraph) -> bool {
    p.props.mark_ins.is_some()
        || p.props.mark_del.is_some()
        || p.props.pfmt.is_some()
        || p.runs.iter().any(|r| r.props.ins.is_some() || r.props.del.is_some() || r.props.fmt.is_some())
}

/// `hasLiveText(p)`.
fn has_live_text(p: &Paragraph) -> bool {
    p.runs.iter().any(|r| r.props.del.is_none() && !r.text.is_empty())
}

// ───────────────────────── listing ─────────────────────────

/// `RevisionKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RevisionKind {
    Insert,
    Delete,
    Format,
    ParaFormat,
}

impl RevisionKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RevisionKind::Insert => "insert",
            RevisionKind::Delete => "delete",
            RevisionKind::Format => "format",
            RevisionKind::ParaFormat => "paraFormat",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<RevisionKind> {
        Some(match s {
            "insert" => RevisionKind::Insert,
            "delete" => RevisionKind::Delete,
            "format" => RevisionKind::Format,
            "paraFormat" => RevisionKind::ParaFormat,
            _ => return None,
        })
    }

    /// `KIND_ORDER`.
    fn order(self) -> u8 {
        match self {
            RevisionKind::ParaFormat => 0,
            RevisionKind::Delete => 1,
            RevisionKind::Insert => 2,
            RevisionKind::Format => 3,
        }
    }
}

/// `RevisionSpan.old` / `.now`: run props or paragraph props.
#[derive(Debug, Clone, PartialEq)]
pub enum SpanProps {
    Run(Box<RunProps>),
    Para(Box<ParaProps>),
}

impl JsValue for SpanProps {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match self {
            SpanProps::Run(p) => p.canon(w),
            SpanProps::Para(p) => p.canon(w),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match self {
            SpanProps::Run(p) => p.json(w),
            SpanProps::Para(p) => p.json(w),
        }
    }
}

impl super::normalize::JsNormalize for SpanProps {
    fn js_needs(&self) -> bool {
        match self {
            SpanProps::Run(p) => p.js_needs(),
            SpanProps::Para(p) => p.js_needs(),
        }
    }
    fn js_normalize(&mut self, cx: &mut super::normalize::NormCx<'_>) {
        match self {
            SpanProps::Run(p) => p.js_normalize(cx),
            SpanProps::Para(p) => p.js_normalize(cx),
        }
    }
}

js_object! {
    /// A revision of the text (`RevisionSpan`: what the Review pane lists and Previous/Next walk).
    pub struct RevisionSpan {
        req kind: Str = "kind",
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        /// a move (`w:moveFrom/To`)
        opt r#move: bool = "move",
        /// ends at an inserted/deleted paragraph mark (alone: start = end = the paragraph's end)
        opt mark: bool = "mark",
        req para: JsNumber = "para",
        opt end_para: JsNumber = "endPara",
        req start: JsNumber = "start",
        req end: JsNumber = "end",
        req text: Utf16 = "text",
        opt old: SpanProps = "old",
        opt now: SpanProps = "now",
    }
}

impl RevisionSpan {
    #[must_use]
    pub fn kind(&self) -> RevisionKind {
        RevisionKind::parse(&self.kind).unwrap_or(RevisionKind::Insert)
    }

    fn n(x: JsNumber) -> u32 {
        uo_jsnum::conv::to_uint32(x.to_number())
    }

    fn info(&self) -> RevisionInfo {
        rev_info(&RevisionInfo {
            author: self.author.clone(),
            date: self.date.clone(),
            date_utc: self.date_utc.clone(),
            r#move: self.r#move,
            ..RevisionInfo::default()
        })
    }

    fn with_info(kind: RevisionKind, info: &RevisionInfo, mark: bool) -> RevisionSpan {
        RevisionSpan {
            kind: kind.as_str().into(),
            author: info.author.clone(),
            date: info.date.clone(),
            date_utc: info.date_utc.clone(),
            r#move: info.r#move,
            mark: mark.then_some(true),
            ..RevisionSpan::default()
        }
    }
}

fn num(n: usize) -> JsNumber {
    JsNumber::Num(n as f64)
}

fn utf16_join(runs: &[Run]) -> Utf16 {
    let mut v = Vec::new();
    for r in runs {
        v.extend_from_slice(r.text.units());
    }
    Utf16::from_units(v)
}

fn append(a: &Utf16, b: &[u16]) -> Utf16 {
    let mut v = a.units().to_vec();
    v.extend_from_slice(b);
    Utf16::from_units(v)
}

/// `collectRevisions(paras)`: the revisions of a paragraph list, in text order. Neighbouring runs
/// of the same authorship (and, for formatting, the same change) form one revision.
#[must_use]
pub fn collect_revisions<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> Vec<RevisionSpan> {
    let mut out: Vec<RevisionSpan> = Vec::new();
    for (pi, p) in paras.into_iter().enumerate() {
        let len = para_length(p);
        if let Some(f) = &p.props.pfmt {
            let mut s = RevisionSpan::with_info(RevisionKind::ParaFormat, &f.info(), false);
            s.para = num(pi);
            s.start = num(0);
            s.end = num(len);
            s.text = utf16_join(&p.runs);
            s.old = Some(SpanProps::Para(f.old.clone()));
            s.now = Some(SpanProps::Para(Box::new(para_formatting(&p.props))));
            out.push(s);
        }
        // open spans per aspect: insert, delete, format (with their keys)
        let mut open: [Option<(RevisionSpan, String)>; 3] = [None, None, None];
        let close = |slot: &mut Option<(RevisionSpan, String)>, out: &mut Vec<RevisionSpan>| {
            if let Some((s, _)) = slot.take() {
                out.push(s);
            }
        };
        let mut acc = 0usize;
        for r in &p.runs {
            let rs = acc;
            acc += r.text.len();
            let aspects: [(RevisionKind, Option<RevisionInfo>, String); 3] = [
                (
                    RevisionKind::Insert,
                    r.props.ins.as_deref().map(Authored::info),
                    r.props.ins.as_deref().map_or_else(|| "null".into(), |v| json(v)),
                ),
                (
                    RevisionKind::Delete,
                    r.props.del.as_deref().map(Authored::info),
                    r.props.del.as_deref().map_or_else(|| "null".into(), |v| json(v)),
                ),
                (
                    RevisionKind::Format,
                    r.props.fmt.as_deref().map(Authored::info),
                    r.props.fmt.as_deref().map_or_else(String::new, |f| {
                        format!("[{},{},{}]", json(&f.info()), json(&*f.old), json(&run_formatting(&r.props)))
                    }),
                ),
            ];
            for (k, (kind, rev, key)) in aspects.into_iter().enumerate() {
                let Some(slot) = open.get_mut(k) else { continue };
                let Some(rev) = rev else {
                    close(slot, &mut out);
                    continue;
                };
                if let Some((span, okey)) = slot.as_mut()
                    && *okey == key
                    && RevisionSpan::n(span.end) as usize == rs
                {
                    span.end = num(acc);
                    span.text = append(&span.text, r.text.units());
                    continue;
                }
                close(slot, &mut out);
                let mut span = RevisionSpan::with_info(kind, &rev, false);
                span.para = num(pi);
                span.start = num(rs);
                span.end = num(acc);
                span.text = r.text.clone();
                if kind == RevisionKind::Format
                    && let Some(f) = r.props.fmt.as_deref()
                {
                    span.old = Some(SpanProps::Run(f.old.clone()));
                    span.now = Some(SpanProps::Run(Box::new(run_formatting(&r.props))));
                }
                *slot = Some((span, key));
            }
        }
        // close('delete'); close('insert'); close('format')
        for k in [1usize, 0, 2] {
            if let Some(slot) = open.get_mut(k) {
                close(slot, &mut out);
            }
        }
        for (rev, kind) in [(&p.props.mark_ins, RevisionKind::Insert), (&p.props.mark_del, RevisionKind::Delete)] {
            if let Some(m) = rev {
                let mut s = RevisionSpan::with_info(kind, &m.info(), true);
                s.para = num(pi);
                s.start = num(len);
                s.end = num(len);
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| {
        RevisionSpan::n(a.para)
            .cmp(&RevisionSpan::n(b.para))
            .then(RevisionSpan::n(a.start).cmp(&RevisionSpan::n(b.start)))
            .then(a.kind().order().cmp(&b.kind().order()))
    });
    // an insertion/deletion across paragraph marks of the same authorship: one revision, as Word
    // lists it (text to the end + ¶ + text from the start of the next)
    let mut merged: Vec<RevisionSpan> = Vec::with_capacity(out.len());
    for s in out {
        if let Some(last) = merged.last_mut()
            && matches!(s.kind(), RevisionKind::Insert | RevisionKind::Delete)
            && last.kind == s.kind
            && same(&last.info(), &s.info())
        {
            let lp = last.end_para.map_or(RevisionSpan::n(last.para), RevisionSpan::n);
            let last_mark = last.mark == Some(true);
            let s_mark = s.mark == Some(true);
            let joins = if last_mark {
                RevisionSpan::n(s.para) == lp.wrapping_add(1) && RevisionSpan::n(s.start) == 0
            } else {
                s_mark && RevisionSpan::n(s.para) == lp && RevisionSpan::n(last.end) == RevisionSpan::n(s.start)
            };
            if joins {
                if last_mark {
                    last.text = append(&last.text, &[u16::from(b'\n')]);
                }
                last.text = append(&last.text, s.text.units());
                obj::set(last, "endPara", |l| l.end_para = Some(s.para));
                last.end = s.end;
                if s_mark {
                    obj::set(last, "mark", |l| l.mark = Some(true));
                } else {
                    obj::delete(last, "mark");
                }
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

/// `spanEnd(s)`: the end of a revision in the document (a mark: the offset past the text).
fn span_end(s: &RevisionSpan) -> DocPosition {
    let para = s.end_para.map_or(RevisionSpan::n(s.para), RevisionSpan::n);
    let end = RevisionSpan::n(s.end);
    DocPosition { para, offset: if s.mark == Some(true) { end.saturating_add(1) } else { end } }
}

/// `revisionAt(spans, pos)`: the revision at the caret (the one containing it; else the one ending
/// at it), as an index into `spans`.
#[must_use]
pub fn revision_at(spans: &[RevisionSpan], pos: DocPosition) -> Option<usize> {
    let cmp = super::text::compare_pos;
    let text = || spans.iter().enumerate().filter(|(_, s)| s.kind() != RevisionKind::ParaFormat);
    text()
        .find(|(_, s)| {
            cmp(DocPosition::new(RevisionSpan::n(s.para), RevisionSpan::n(s.start)), pos).is_le() && cmp(pos, span_end(s)).is_lt()
        })
        .or_else(|| {
            text().find(|(_, s)| {
                cmp(pos, DocPosition::new(s.end_para.map_or(RevisionSpan::n(s.para), RevisionSpan::n), RevisionSpan::n(s.end))).is_eq()
            })
        })
        .or_else(|| spans.iter().enumerate().find(|(_, s)| RevisionSpan::n(s.para) == pos.para && s.kind() == RevisionKind::ParaFormat))
        .map(|(i, _)| i)
}

/// `RevisionScope`: what to accept or reject.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RevisionScope {
    /// a range (absent = every paragraph). The mark of paragraph i is in it when the range passes
    /// it: `end.para > i`, or `end.offset` past the paragraph's length
    pub range: Option<DocRange>,
    /// only these kinds (default: all)
    pub kinds: Option<Vec<RevisionKind>>,
    /// only these authors (default: all)
    pub authors: Option<Vec<Str>>,
    /// the PARAGRAPH formatting stays out (a text revision chosen at the caret)
    pub runs_only: bool,
}

/// `spanScope(s)`: the scope of one revision (a mark: the ¶, the offset past the text).
#[must_use]
pub fn span_scope(s: &RevisionSpan) -> RevisionScope {
    let kind = s.kind();
    RevisionScope {
        range: Some(DocRange::new(DocPosition::new(RevisionSpan::n(s.para), RevisionSpan::n(s.start)), span_end(s))),
        kinds: Some(vec![kind]),
        authors: Some(vec![s.author.clone()]),
        runs_only: kind != RevisionKind::ParaFormat,
    }
}

// ───────────────────────── accept / reject ─────────────────────────

/// `acceptRevisions(paras, scope, onMerge)`: `on_merge(i)` = paragraph i was joined to i − 1 and
/// left the list (the session shifts its anchors). Whether anything changed.
pub fn accept_revisions(paras: &mut PVec<Paragraph>, scope: &RevisionScope, on_merge: &mut dyn FnMut(u32)) -> bool {
    apply_revisions(paras, scope, true, on_merge)
}

/// `rejectRevisions(paras, scope, onMerge)`.
pub fn reject_revisions(paras: &mut PVec<Paragraph>, scope: &RevisionScope, on_merge: &mut dyn FnMut(u32)) -> bool {
    apply_revisions(paras, scope, false, on_merge)
}

struct Allow<'s> {
    scope: &'s RevisionScope,
}

impl Allow<'_> {
    fn kind_ok(&self, k: RevisionKind) -> bool {
        self.scope.kinds.as_ref().is_none_or(|ks| ks.contains(&k))
    }
    fn ok(&self, k: RevisionKind, author: Option<&str>) -> bool {
        let Some(author) = author else { return false };
        self.kind_ok(k) && self.scope.authors.as_ref().is_none_or(|a| a.iter().any(|x| &**x == author))
    }
}

fn apply_revisions(paras: &mut PVec<Paragraph>, scope: &RevisionScope, accept: bool, on_merge: &mut dyn FnMut(u32)) -> bool {
    let okf = Allow { scope };
    let range = scope.range.map(normalize_range);
    let mut changed = false;
    let mut i = paras.len();
    while i > 0 {
        i -= 1;
        let Some(p) = paras.get(i) else { continue };
        let len = para_length(p);
        let (mut from, mut to, mut mark_in) = (0usize, len, true);
        if let Some(r) = range {
            if i < r.start.para as usize || i > r.end.para as usize {
                continue;
            }
            from = if i == r.start.para as usize { r.start.offset as usize } else { 0 };
            to = if i == r.end.para as usize { (r.end.offset as usize).min(len) } else { len };
            mark_in = r.end.para as usize > i || r.end.offset as usize > len;
        }
        if to > from
            && let Some(p) = paras.make_mut(i)
            && apply_runs(p, from, to, accept, &okf)
        {
            changed = true;
        }
        if let Some(p) = paras.get(i)
            && let Some(pf) = p.props.pfmt.as_deref()
            && !scope.runs_only
            && okf.ok(RevisionKind::ParaFormat, Some(pf.author()))
        {
            changed = true;
            if let Some(p) = paras.make_mut(i) {
                if accept {
                    let mut props = (*p.props).clone();
                    obj::delete(&mut props, "pfmt");
                    p.props = Arc::new(props);
                } else {
                    reject_para_format(p);
                }
            }
        }
        if !mark_in {
            continue;
        }
        let last = i + 1 == paras.len();
        let Some(p) = paras.get(i) else { continue };
        let (mi, md) = (p.props.mark_ins.clone(), p.props.mark_del.clone());
        if let Some(md) = md
            && okf.ok(RevisionKind::Delete, Some(&md.author))
        {
            changed = true;
            if accept && !last {
                merge_next(paras, i);
                on_merge(u32::try_from(i + 1).unwrap_or(u32::MAX));
                continue;
            }
            if let Some(p) = paras.make_mut(i) {
                let mut props = (*p.props).clone();
                obj::delete(&mut props, "markDel");
                p.props = Arc::new(props);
                if let Some(ppr) = &p.ppr
                    && ppr.mark_deleted == Some(true)
                {
                    let mut ppr = (**ppr).clone();
                    obj::delete(&mut ppr, "markDeleted");
                    obj::delete(&mut ppr, "markDel");
                    p.ppr = Some(Arc::new(ppr));
                }
            }
        }
        if let Some(mi) = mi
            && okf.ok(RevisionKind::Insert, Some(&mi.author))
        {
            changed = true;
            if !accept && !last {
                merge_next(paras, i);
                on_merge(u32::try_from(i + 1).unwrap_or(u32::MAX));
                continue;
            }
            if let Some(p) = paras.make_mut(i) {
                let mut props = (*p.props).clone();
                obj::delete(&mut props, "markIns");
                p.props = Arc::new(props);
            }
        }
    }
    changed
}

/// `mergeNext(paras, i)`: joins paragraph i + 1 to i with the formatting of the SECOND (the mark
/// that remains).
fn merge_next(paras: &mut PVec<Paragraph>, i: usize) {
    let Some(next) = paras.get(i + 1).cloned() else { return };
    let Some(p) = paras.make_mut(i) else { return };
    p.runs.extend(next.runs);
    p.props = next.props;
    match next.ppr {
        Some(ppr) => obj::set(p, "ppr", |q| q.ppr = Some(ppr)),
        None => obj::delete(p, "ppr"),
    }
    paras.remove(i + 1);
    if let Some(p) = paras.make_mut(i) {
        normalize_paragraph(p);
    }
}

/// `applyRuns(p, from, to, accept, ok)`: accepts/rejects the revisions of the runs in `[from,
/// to)`.
fn apply_runs(p: &mut Paragraph, from: usize, to: usize, accept: bool, okf: &Allow<'_>) -> bool {
    split_at(p, to);
    split_at(p, from);
    let mut changed = false;
    let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
    let mut acc = 0usize;
    for run in std::mem::take(&mut p.runs) {
        let rs = acc;
        acc += run.text.len();
        let inside = if run.text.is_empty() { rs >= from && rs < to } else { rs >= from && acc <= to };
        if !inside {
            out.push(run);
            continue;
        }
        let mut r = run;
        if okf.ok(RevisionKind::Format, r.props.fmt.as_deref().map(Authored::author)) {
            changed = true;
            if accept {
                let mut props = (*r.props).clone();
                obj::delete(&mut props, "fmt");
                r.props = Arc::new(props);
            } else {
                r = reject_run_format(r);
            }
        }
        if okf.ok(RevisionKind::Delete, r.props.del.as_deref().map(Authored::author)) {
            changed = true;
            if accept {
                continue;
            }
            let mut props = (*r.props).clone();
            obj::delete(&mut props, "del");
            r.props = Arc::new(props);
        }
        if okf.ok(RevisionKind::Insert, r.props.ins.as_deref().map(Authored::author)) {
            changed = true;
            if !accept {
                continue;
            }
            let mut props = (*r.props).clone();
            obj::delete(&mut props, "ins");
            r.props = Arc::new(props);
        }
        out.push(r);
    }
    p.runs = out;
    normalize_paragraph(p);
    changed
}

/// `fileFormatChange(run)`: the run's `w:rPrChange` is the one from the file (the typed `rpr` has
/// the exact old one).
fn file_format_change(run: &Run) -> bool {
    let (Some(c), Some(f)) = (run.rpr.as_deref().and_then(|r| r.change.as_deref()), run.props.fmt.as_deref()) else { return false };
    same(&c.info(), &f.info()) && same(&run_props_from_rpr(Some(&*c.r_pr)), &*f.old)
}

/// `rejectRunFormat(run)`: back to the props from before (from the file: the exact old `rPr`).
fn reject_run_format(run: Run) -> Run {
    let Some(fmt) = run.props.fmt.clone() else { return run };
    let mut meta = run_meta(&run.props);
    obj::delete(&mut meta, "fmt");
    if file_format_change(&run) {
        let Some(old_rpr) = run.rpr.as_deref().and_then(|r| r.change.as_deref()).map(|c| (*c.r_pr).clone()) else { return run };
        let mut out = Run { rpr: Some(Arc::new(old_rpr.clone())), ..run };
        let base = without_revisions(&edit_base_props(&out, Some(&old_rpr)));
        out.props = Arc::new(obj::spread(&base, &meta));
        // `{ ...withoutRevisions(base), ...meta }`: a new object without symbol keys
        out.rpr_link = None;
        return out;
    }
    Run { props: Arc::new(obj::spread(&*fmt.old, &meta)), rpr_link: fmt.old_link.clone(), ..run }
}

/// `rejectParaFormat(p)`: back to the props from before (from the file: the old `pPr`).
fn reject_para_format(p: &mut Paragraph) {
    let Some(pf) = p.props.pfmt.clone() else { return };
    let mut meta = para_meta(&p.props);
    obj::delete(&mut meta, "pfmt");
    if let Some(cur) = p.ppr.clone()
        && let Some(c) = cur.change.as_deref()
        && same(&c.info(), &pf.info())
        && same(&para_formatting(&para_props_from_ppr(Some(&*c.p_pr))), &*pf.old)
    {
        let mut ppr = (*c.p_pr).clone();
        if let Some(r) = &cur.r_pr {
            obj::set(&mut ppr, "rPr", |x| x.r_pr = Some(r.clone()));
        }
        if cur.mark_deleted == Some(true) {
            obj::set(&mut ppr, "markDeleted", |x| x.mark_deleted = Some(true));
        }
        if let Some(m) = &cur.mark_del {
            obj::set(&mut ppr, "markDel", |x| x.mark_del = Some(m.clone()));
        }
        if let Some(m) = &cur.mark_ins {
            obj::set(&mut ppr, "markIns", |x| x.mark_ins = Some(m.clone()));
        }
        p.ppr = Some(Arc::new(ppr));
    }
    p.props = Arc::new(obj::spread(&*pf.old, &meta));
}

// ───────────────────────── editing with tracking ON ─────────────────────────

/// `{ ...base, ins: rev }`.
fn with_ins(mut props: RunProps, rev: &RevisionInfo) -> RunProps {
    obj::set(&mut props, "ins", |p| p.ins = Some(Box::new(rev.clone())));
    props
}

/// `trackInsertText(model, pos, text, rev, props?)`: typed/pasted text enters as an insertion.
pub fn track_insert_text(
    paras: &mut PVec<Paragraph>,
    pos: DocPosition,
    text: &[u16],
    rev: &RevisionInfo,
    props: Option<&CarriedProps>,
) -> OpResult<DocPosition> {
    let src = props.cloned().unwrap_or_else(|| props_at(paras, pos));
    let base = CarriedProps { props: with_ins(without_revisions(&src.props), rev), link: src.link };
    insert_text(paras, pos, text, Some(&base))
}

/// `trackInsertRun(model, pos, run, rev)`.
pub fn track_insert_run(paras: &mut PVec<Paragraph>, pos: DocPosition, run: Run, rev: &RevisionInfo) -> OpResult<DocPosition> {
    let props = with_ins(without_revisions(&run.props), rev);
    insert_run(paras, pos, Run { props: Arc::new(props), ..run })
}

/// `trackSplitParagraph(model, pos, rev)`: the new mark (the first paragraph's) is an insertion.
pub fn track_split_paragraph(
    paras: &mut PVec<Paragraph>,
    ids: &mut IdAlloc,
    pos: DocPosition,
    rev: &RevisionInfo,
) -> OpResult<DocPosition> {
    let out = split_paragraph(paras, ids, pos)?;
    if let Some(first) = paras.make_mut(pos.para as usize) {
        let mut props = (*first.props).clone();
        obj::set(&mut props, "markIns", |p| p.mark_ins = Some(Box::new(rev.clone())));
        first.props = Arc::new(props);
    }
    Ok(out)
}

/// `TrackedDelete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedDelete {
    /// the start of the deleted range, after the operation
    pub start: DocPosition,
    /// its end, after the operation (the deleted text stays before it)
    pub end: DocPosition,
    /// the paragraphs that left (joined to the previous), in the order they left
    pub merged: Vec<u32>,
}

/// `trackDelete(model, range, rev)`: one's own insertion (and own inserted mark) goes for real;
/// the rest becomes a deletion of `rev`; what was already deleted stays.
pub fn track_delete(paras: &mut PVec<Paragraph>, range: DocRange, rev: &RevisionInfo) -> OpResult<TrackedDelete> {
    let DocRange { start, end } = normalize_range(range);
    if end.para as usize >= paras.len() {
        return Err(OpError::Position);
    }
    let mut merged: Vec<u32> = Vec::new();
    let mut end_para = end.para as usize;
    let mut end_offset = i64::from(end.offset);
    let mut i = end.para as usize + 1;
    while i > start.para as usize {
        i -= 1;
        let Some(p) = paras.get(i) else { return Err(OpError::Position) };
        let from = if i == start.para as usize { start.offset as usize } else { 0 };
        let to = if i == end.para as usize { end.offset as usize } else { para_length(p) };
        if to > from
            && let Some(p) = paras.make_mut(i)
        {
            let removed = delete_runs(p, from, to, rev);
            if i == end.para as usize {
                end_offset -= i64::try_from(removed).unwrap_or(i64::MAX);
            }
        }
        if i == end.para as usize || i + 1 >= paras.len() {
            continue;
        }
        let Some(p) = paras.get(i) else { continue };
        if p.props.mark_del.is_some() {
            continue;
        }
        if p.props.mark_ins.as_deref().is_some_and(|m| m.author == rev.author) {
            // a mark inserted by the same author: it goes (the paragraphs really join)
            let len = para_length(p);
            let Some(next) = paras.get(i + 1).cloned() else { continue };
            if let Some(p) = paras.make_mut(i) {
                p.runs.extend(next.runs);
                p.props = next.props;
                match next.ppr {
                    Some(ppr) => obj::set(p, "ppr", |q| q.ppr = Some(ppr)),
                    None => obj::delete(p, "ppr"),
                }
            }
            paras.remove(i + 1);
            if let Some(p) = paras.make_mut(i) {
                normalize_paragraph(p);
            }
            merged.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
            if end_para == i + 1 {
                end_para = i;
                end_offset += i64::try_from(len).unwrap_or(i64::MAX);
            } else if end_para > i + 1 {
                end_para -= 1;
            }
            continue;
        }
        let Some(p) = paras.make_mut(i) else { continue };
        let mut props = (*p.props).clone();
        obj::set(&mut props, "markDel", |x| x.mark_del = Some(Box::new(rev.clone())));
        p.props = Arc::new(props);
        // in the final view this paragraph's text goes to the next: the next takes this one's
        // formatting (Word writes `w:pPrChange` on it) — only when this one has text
        let (Some(p), Some(next)) = (paras.get(i), paras.get(i + 1)) else { continue };
        let pf = para_formatting(&p.props);
        if has_live_text(p) && !same(&para_formatting(&next.props), &pf) {
            let old = next.props.pfmt.as_deref().map_or_else(|| para_formatting(&next.props), |f| (*f.old).clone());
            let pfmt = next.props.pfmt.clone().unwrap_or_else(|| Box::new(para_format_change(rev, old)));
            let mut props = obj::spread(&para_meta(&next.props), &pf);
            obj::set(&mut props, "pfmt", |x| x.pfmt = Some(pfmt));
            let undo = props.pfmt.as_deref().is_some_and(|f| same(&para_formatting(&props), &*f.old));
            if undo {
                obj::delete(&mut props, "pfmt");
            }
            if let Some(next) = paras.make_mut(i + 1) {
                next.props = Arc::new(props);
            }
        }
    }
    let offset = u32::try_from(end_offset.max(0)).unwrap_or(u32::MAX);
    Ok(TrackedDelete { start, end: DocPosition { para: u32::try_from(end_para).unwrap_or(u32::MAX), offset }, merged })
}

/// `deleteRuns(p, from, to, rev)`: marks `[from, to)` as deleted; one's own insertion goes. How
/// many characters went.
fn delete_runs(p: &mut Paragraph, from: usize, to: usize, rev: &RevisionInfo) -> usize {
    split_at(p, to);
    split_at(p, from);
    let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
    let mut acc = 0usize;
    let mut removed = 0usize;
    for run in std::mem::take(&mut p.runs) {
        let rs = acc;
        acc += run.text.len();
        let inside = if run.text.is_empty() { rs >= from && rs < to } else { rs >= from && acc <= to };
        if !inside || run.props.del.is_some() {
            out.push(run);
            continue;
        }
        if run.props.ins.as_deref().is_some_and(|i| i.author == rev.author && i.r#move != Some(true)) {
            removed += run.text.len();
            continue;
        }
        let mut props = (*run.props).clone();
        obj::set(&mut props, "del", |x| x.del = Some(Box::new(rev.clone())));
        out.push(Run { props: Arc::new(props), ..run });
    }
    p.runs = out;
    normalize_paragraph(p);
    removed
}

/// `Object.keys(patch).some((k) => !META.includes(k))`.
fn has_formatting_key<T: JsObject>(patch: &T, meta: &[&str]) -> bool {
    let mut any = false;
    patch.for_each_in_order(&mut |k, _| {
        if !meta.contains(&k) {
            any = true;
        }
    });
    any
}

/// `trackApplyRunProps(model, range, patch, rev)`: a formatting-only patch becomes a
/// `w:rPrChange`.
pub fn track_apply_run_props(paras: &mut PVec<Paragraph>, range: DocRange, patch: &RunProps, rev: &RevisionInfo) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    if start.para as usize >= paras.len() || end.para as usize >= paras.len() {
        return Err(OpError::Position);
    }
    let formatting = has_formatting_key(patch, &RUN_META);
    for pi in start.para as usize..=end.para as usize {
        let Some(p) = paras.get(pi) else { return Err(OpError::Position) };
        let from = if pi == start.para as usize { start.offset as usize } else { 0 };
        let to = if pi == end.para as usize { end.offset as usize } else { para_length(p) };
        if to <= from {
            continue;
        }
        let Some(p) = paras.make_mut(pi) else { return Err(OpError::Position) };
        split_at(p, to);
        split_at(p, from);
        let mut acc = 0usize;
        for run in &mut p.runs {
            let rs = acc;
            acc += run.text.len();
            if rs < from || acc > to || run.text.is_empty() {
                continue;
            }
            let before = run_formatting(&run.props);
            let mut props = obj::spread(&*run.props, patch);
            if formatting {
                let after = run_formatting(&props);
                match props.fmt.as_deref() {
                    Some(f) => {
                        // the change keeps the formatting from BEFORE the first; returning to it undoes
                        if same(&after, &*f.old) {
                            obj::delete(&mut props, "fmt");
                        }
                    }
                    None => {
                        if !same(&before, &after) {
                            let f = format_change(rev, before, run.rpr_link.clone());
                            obj::set(&mut props, "fmt", |x| x.fmt = Some(Box::new(f)));
                        }
                    }
                }
            }
            run.props = Arc::new(props);
        }
        normalize_paragraph(p);
    }
    Ok(())
}

/// `trackApplyParaProps(model, range, patch, rev)`: becomes a `w:pPrChange`.
pub fn track_apply_para_props(paras: &mut PVec<Paragraph>, range: DocRange, patch: &ParaProps, rev: &RevisionInfo) -> OpResult<()> {
    let DocRange { start, end } = normalize_range(range);
    if start.para as usize >= paras.len() || end.para as usize >= paras.len() {
        return Err(OpError::Position);
    }
    let formatting = has_formatting_key(patch, &PARA_META);
    for pi in start.para as usize..=end.para as usize {
        let Some(p) = paras.make_mut(pi) else { return Err(OpError::Position) };
        let before = para_formatting(&p.props);
        let mut props = obj::spread(&*p.props, patch);
        if formatting {
            let after = para_formatting(&props);
            match props.pfmt.as_deref() {
                Some(f) => {
                    if same(&after, &*f.old) {
                        obj::delete(&mut props, "pfmt");
                    }
                }
                None => {
                    if !same(&before, &after) {
                        let f = para_format_change(rev, before);
                        obj::set(&mut props, "pfmt", |x| x.pfmt = Some(Box::new(f)));
                    }
                }
            }
        }
        p.props = Arc::new(props);
    }
    Ok(())
}

/// `revisionAuthors(full)`: the authors in reading order (body, headers/footers, notes).
#[must_use]
pub fn revision_authors(view: crate::view::DocView<'_>) -> Vec<&str> {
    view.revision_authors()
}
