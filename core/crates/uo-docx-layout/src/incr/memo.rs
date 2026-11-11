//! LA7 level 1 — the paragraph memo of the incremental relayout (record `uo-docx-layout.md`
//! "Stage M2 — incremental relayout (LA7)", "Level 1"). `prepare_paragraph` of a body paragraph
//! is kept per model paragraph (`ParaId`) with everything it read outside its paragraph node and
//! everything it did outside its result, and served again — the result's run keys moved into
//! the new derived document — when every read gives the same answer:
//!
//! - **the key** (checked before anything runs): the same paragraph node (`Arc::ptr_eq`, kept
//!   alive by the entry), the derived paragraph a plain body model paragraph with the same runs
//!   (each the same model run object, the same field-result keys), the same `ParaEnv` (`Debug`),
//!   the same recursion depth; per call, the same options (`Debug`, without the cancellation),
//!   the same `formatting` and `styles` objects. Every face key the paragraph asked the call's
//!   resolver for is cached in the running call with the same `FaceId` and the same provider
//!   answer.
//! - **the recorded reads and effects** (the effect log, replayed in order): every call of the
//!   `PrepFlow` — field values (`PAGE`, `REF`, `STYLEREF`, `NOTEREF`… — the other paragraphs
//!   they read are read again), `number_next` (the numbering marker), `note_mark`, `on_field`,
//!   `warn` — runs again on the live pass state; each answer must equal the recorded one. The
//!   paragraph's own budget charge is charged once at the end.
//! - **what makes a paragraph not memoizable**: a prepare that added an entry to any call
//!   table (a new face, symbol, text effect, image…, or moved the line counter: then a hit
//!   would have to add it too), a result holding a call-local id other than a face (an image,
//!   a text effect), a run key that is not one of its own runs or their field results.
//!
//! When an answer differs at step `i` the paragraph is prepared again with steps `0..=i` served
//! from what the replay already did on the live state (the same calls with the same arguments:
//! a deterministic prefix), so no effect runs twice; the new run is recorded in place of the
//! entry. Exactness rests on `prepare_paragraph` being a function of the key, the answers and the
//! cached faces (declarations D-LA7-18…27).

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::record::CallLens;
use crate::context::{DocContext, NoteKindFlow};
use crate::engine::{Cx, Engine};
use crate::error::{LResult, LayoutError};
use crate::fonts::AnswerId;
use crate::keys::{LParaId, RunKey};
use crate::ldoc::{LDoc, LPara, ParaSrc, RunSrc};
use crate::math::{MathBox, MathItem};
use crate::model::{DocFormatting, Document, JsNumber, ParaId, Paragraph, Run, Story, StoryRef};
use crate::options::LayoutOptions;
use crate::para::{
    Ch, FieldRun, FieldSource, Glyph, GlyphFlags, GlyphKind, ParaEnv, ParaMarker, PrepFlow, PreparedPara, RunStyle, StyleIx,
    field_model_ref, prepare_paragraph,
};
use crate::word::{NumberMarker, TextCap};
use uo_render::FaceId;
use uo_render::OptF64;

/// The internal error of a failed memo check (a served call that is not the one the prepare
/// made, a key that does not map): never seen; the caller lays out again without the memo.
const MEMO_ERR: &str = "incr: memo check";

/// Whether `e` is a failed memo check (D-LA7-21).
pub(crate) fn failed(e: &LayoutError) -> bool {
    matches!(e, LayoutError::Internal(m) if *m == MEMO_ERR)
}

/// The caps of the memo (M5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoCaps {
    /// `false`: no paragraph is memoized
    pub enabled: bool,
    /// entries kept at most (paragraphs × up to two environments)
    pub entries: usize,
    /// estimated heap bytes of the kept entries at most; past it new paragraphs are not kept
    /// (the first ones in document order stay: a full layout walks the document in order, so
    /// evicting the oldest would evict exactly the ones the next full layout needs first)
    pub bytes: usize,
}

/// What the memo did in one relayout (in [`super::RelayoutReport::memo`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoReport {
    /// prepares served from the memo (key and every answer the same)
    pub hits: u32,
    /// prepares whose key held but an answer differed (prepared again, served prefix)
    pub replayed: u32,
    /// prepares without a usable entry (prepared and recorded)
    pub misses: u32,
    /// prepares outside the memo's scope (not a plain body model paragraph)
    pub skipped: u32,
    /// prepares that could not be kept (they added to a call table, or hold a call-local id)
    pub unmemoizable: u32,
    /// prepares not kept because the memo was full
    pub over_cap: u32,
    /// entries and their estimated heap bytes after the relayout
    pub entries: usize,
    pub bytes: usize,
}

/// One flow call of a prepare, with its answer (the run keys of the recorded log are relative:
/// indices into [`Entry::refs`]).
#[derive(Clone, Debug)]
enum FlowOp {
    Field { src: FieldSource, name: Box<[u16]>, instr: Box<[u16]>, at: f64, ans: Option<Box<[u16]>> },
    Number { num_id: JsNumber, ilvl: JsNumber, ans: Option<NumberMarker> },
    NoteMark { kind: NoteKindFlow, id: f64, custom: bool, lang: Box<str>, cap0: TextCap, cap1: TextCap, ans: Box<[u16]> },
    OnField { run: FieldRun, v: Box<[u16]> },
    Warn { msg: Box<str> },
}

/// `Debug` equality (a JS number: `-0`/`+0` and NaN stay apart as the printed value does).
fn dbg_eq(a: &impl std::fmt::Debug, b: &impl std::fmt::Debug) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

impl FlowOp {
    /// The same call (kind and arguments, the answer aside).
    fn same_call(&self, o: &FlowOp) -> bool {
        match (self, o) {
            (FlowOp::Field { src, name, instr, at, .. }, FlowOp::Field { src: s, name: n, instr: i, at: a, .. }) => {
                src == s && name == n && instr == i && at.to_bits() == a.to_bits()
            }
            (FlowOp::Number { num_id, ilvl, .. }, FlowOp::Number { num_id: n, ilvl: l, .. }) => dbg_eq(num_id, n) && dbg_eq(ilvl, l),
            (
                FlowOp::NoteMark { kind, id, custom, lang, cap0, .. },
                FlowOp::NoteMark { kind: k, id: i, custom: c, lang: l, cap0: c0, .. },
            ) => kind == k && id.to_bits() == i.to_bits() && custom == c && lang == l && cap0 == c0,
            (FlowOp::OnField { run, v }, FlowOp::OnField { run: r, v: w }) => run == r && v == w,
            (FlowOp::Warn { msg }, FlowOp::Warn { msg: m }) => msg == m,
            _ => false,
        }
    }

    /// The run keys of the op mapped by `f` (`None` when one does not map).
    fn map_keys(&self, f: &mut impl FnMut(RunKey) -> Option<RunKey>) -> Option<FlowOp> {
        let mut op = self.clone();
        if let FlowOp::OnField { run, .. } = &mut op
            && let Some(k) = run.run.as_mut()
        {
            *k = f(*k)?;
        }
        Some(op)
    }

    fn bytes(&self) -> usize {
        std::mem::size_of::<FlowOp>()
            + match self {
                FlowOp::Field { name, instr, ans, .. } => 2 * (name.len() + instr.len() + ans.as_ref().map_or(0, |a| a.len())),
                FlowOp::Number { ans, .. } => ans.as_ref().map_or(0, |a| 2 * a.text.len() + 64),
                FlowOp::NoteMark { lang, ans, .. } => lang.len() + 2 * ans.len(),
                FlowOp::OnField { run, v } => 4 * run.sub.len() + 2 * v.len(),
                FlowOp::Warn { msg } => msg.len(),
            }
    }
}

/// The address of a model object (an identity: the entry keeps the paragraph node alive, so the
/// address is never reused while the entry lives).
fn addr<T>(x: &T) -> usize {
    std::ptr::from_ref(x) as usize
}

/// A run key of a recorded result, relative to its paragraph: the `run`-th run of the derived
/// paragraph, or field result `sub` of it.
#[derive(Clone, Copy, Debug)]
struct KeyRef {
    run: u32,
    sub: Option<u32>,
    /// the model run the key names (an identity)
    obj: usize,
}

/// One memoized prepare.
struct Entry {
    node: Arc<Paragraph>,
    env: String,
    depth: u32,
    /// the model run of each run of the derived paragraph, and how many field results each has
    runs: Vec<(usize, u32)>,
    refs: Vec<KeyRef>,
    /// every face key asked, its face and the provider answer behind it
    faces: Vec<(Box<str>, FaceId, AnswerId)>,
    ops: Vec<FlowOp>,
    /// the paragraph's own work units (without those of the flow calls)
    charge: u64,
    /// the result, its run keys relative ([`Entry::refs`]) and `para` unset
    prep: Packed,
    bytes: usize,
    /// the call that last used it
    epoch: u64,
}

/// The per-call key of the memo: entries recorded under another one are dropped.
struct CallKey {
    options: String,
    parts: Document,
}

/// The paragraph memo (in the [`super::IncrState`]).
pub struct ParaMemo {
    caps: MemoCaps,
    key: Option<CallKey>,
    entries: FxHashMap<ParaId, Vec<Entry>>,
    count: usize,
    bytes: usize,
    epoch: u64,
    /// the body paragraph nodes of the running call, by id (built at the first record)
    nodes: Option<FxHashMap<ParaId, Arc<Paragraph>>>,
    active: bool,
    report: MemoReport,
}

impl ParaMemo {
    pub(crate) fn new(caps: MemoCaps) -> ParaMemo {
        ParaMemo {
            caps,
            key: None,
            entries: FxHashMap::default(),
            count: 0,
            bytes: 0,
            epoch: 0,
            nodes: None,
            active: false,
            report: MemoReport::default(),
        }
    }

    /// Drops every entry (M1).
    pub fn clear(&mut self) {
        self.key = None;
        self.entries = FxHashMap::default();
        self.count = 0;
        self.bytes = 0;
        self.nodes = None;
        self.active = false;
    }

    /// Entries kept and their estimated heap bytes.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (self.count, self.bytes)
    }

    /// The start of a layout call over `ldoc` with the call's effective options.
    pub(crate) fn begin_call(&mut self, ldoc: &LDoc<'_>, opts: &LayoutOptions) {
        self.epoch = self.epoch.wrapping_add(1);
        self.report = MemoReport::default();
        self.nodes = None;
        self.active = false;
        let Some(options) = super::keys::options_key(opts).filter(|_| self.caps.enabled) else {
            self.clear();
            return;
        };
        let doc = ldoc.view.document();
        let same = self.key.as_ref().is_some_and(|k| {
            k.options == options && Arc::ptr_eq(&k.parts.formatting, &doc.formatting) && Arc::ptr_eq(&k.parts.styles, &doc.styles)
        });
        if !same {
            self.clear();
            self.key = Some(CallKey { options, parts: Document { body: Story::default(), ..doc.clone() } });
        }
        self.active = true;
    }

    /// The end of a call; after a full layout (every paragraph prepared) the entries it did not
    /// use go (removed paragraphs).
    pub(crate) fn end_call(&mut self, full: bool) -> MemoReport {
        self.nodes = None;
        self.active = false;
        if full {
            let epoch = self.epoch;
            for v in self.entries.values_mut() {
                v.retain(|e| e.epoch == epoch);
            }
            self.entries.retain(|_, v| !v.is_empty());
            self.count = self.entries.values().map(Vec::len).sum();
            self.bytes = self.entries.values().flatten().map(|e| e.bytes).sum();
        }
        self.report.entries = self.count;
        self.report.bytes = self.bytes;
        self.report
    }

    fn node(&mut self, ldoc: &LDoc<'_>, id: ParaId) -> Option<Arc<Paragraph>> {
        let nodes = self.nodes.get_or_insert_with(|| ldoc.view.document().body.paras.iter_arcs().map(|a| (a.id, Arc::clone(a))).collect());
        nodes.get(&id).cloned()
    }

    fn keep(&mut self, id: ParaId, e: Entry) {
        let list = self.entries.entry(id).or_default();
        let same = list.iter().position(|x| x.env == e.env && x.depth == e.depth);
        let freed = same.and_then(|i| list.get(i)).map_or(0, |x| x.bytes);
        let full_now = self.bytes - freed + e.bytes > self.caps.bytes || (same.is_none() && self.count >= self.caps.entries);
        if full_now {
            self.report.over_cap += 1;
            if list.is_empty() {
                self.entries.remove(&id);
            }
            return;
        }
        self.bytes = self.bytes - freed + e.bytes;
        match same {
            Some(i) => {
                if let Some(slot) = list.get_mut(i) {
                    *slot = e;
                }
            }
            None if list.len() >= 2 => {
                // two environments already: the one used longest ago goes
                let old = list.iter().enumerate().min_by_key(|(_, x)| x.epoch).map(|(i, _)| i).unwrap_or(0);
                if let Some(slot) = list.get_mut(old) {
                    self.bytes -= slot.bytes;
                    *slot = e;
                }
            }
            None => {
                list.push(e);
                self.count += 1;
            }
        }
    }
}

/// `prepareParagraph` through the memo (`Engine::prepare` of a model paragraph while an
/// incremental relayout runs).
pub(crate) fn prepare(m: &mut ParaMemo, eng: &mut Engine, cx: &mut Cx<'_, '_>, k: LParaId, env: &ParaEnv) -> LResult<PreparedPara> {
    let ldoc = cx.ldoc;
    let Some(lp) = ldoc.para(k).filter(|_| m.active) else { return prepare_paragraph(cx, eng, k, env) };
    let ParaSrc::Model { para, id } = lp.src else {
        m.report.skipped += 1;
        return prepare_paragraph(cx, eng, k, env);
    };
    if lp.story != StoryRef::Body
        || lp.ppr.is_some()
        || lp.props.is_some()
        || lp.flags.0 != 0
        || lp.frame_carry.is_some()
        || !lp.rev_joins.is_empty()
    {
        m.report.skipped += 1;
        return prepare_paragraph(cx, eng, k, env);
    }
    let env_key = format!("{env:?}");
    let mut served = VecDeque::new();
    let mut lens0 = None;
    let found = m.entries.get(&id).and_then(|v| v.iter().position(|e| e.env == env_key && e.depth == cx.depth));
    let entry = found.and_then(|i| m.entries.get(&id).and_then(|v| v.get(i)));
    match entry.filter(|e| std::ptr::eq(para, Arc::as_ptr(&e.node))) {
        Some(e) => match e.keys_in(ldoc, lp).filter(|_| e.faces_ok(cx)) {
            Some(keys) => {
                lens0 = Some(CallLens::of_cx(cx));
                match replay(e, eng, cx, &keys)? {
                    None => {
                        cx.budget.charge(e.charge)?;
                        let p = e.output(k, &keys)?;
                        let epoch = m.epoch;
                        if let Some(e) = found.and_then(|i| m.entries.get_mut(&id).and_then(|v| v.get_mut(i))) {
                            e.epoch = epoch;
                        }
                        m.report.hits += 1;
                        return Ok(p);
                    }
                    Some(done) => {
                        served = done;
                        m.report.replayed += 1;
                    }
                }
            }
            None => m.report.misses += 1,
        },
        None => m.report.misses += 1,
    }
    record(m, eng, cx, (k, lp, id, para), env, env_key, served, lens0)
}

impl Entry {
    /// The keys of the recorded result in the running call's derived document, when its
    /// paragraph has the same runs (`None` otherwise).
    fn keys_in(&self, ldoc: &LDoc<'_>, lp: &LPara<'_>) -> Option<Vec<RunKey>> {
        if lp.runs.len() != self.runs.len() {
            return None;
        }
        for (&rk, &(obj, n)) in lp.runs.iter().zip(&self.runs) {
            if model_run_addr(ldoc, rk) != Some(obj) || field_results(ldoc, rk) != n {
                return None;
            }
        }
        let mut out = Vec::with_capacity(self.refs.len());
        for r in &self.refs {
            let base = *lp.runs.get(r.run as usize)?;
            let key = match r.sub {
                None => base,
                Some(j) => ldoc.field_result_key(base, j)?,
            };
            if model_run_addr(ldoc, key) != Some(r.obj) {
                return None;
            }
            out.push(key);
        }
        Some(out)
    }

    /// Every face key is cached in the running call with the same face and answer.
    fn faces_ok(&self, cx: &Cx<'_, '_>) -> bool {
        self.faces
            .iter()
            .all(|(key, id, aid)| cx.resolver.cached(key) == Some(*id) && cx.fonts.entry(*id).map(|e| e.answer.id()) == Some(*aid))
    }

    /// The recorded result in the running call: its paragraph and run keys.
    fn output(&self, k: LParaId, keys: &[RunKey]) -> LResult<PreparedPara> {
        let mut p = self.prep.unpack();
        p.para = k;
        let mut f = |r: RunKey| keys.get(r.ix()).copied();
        if !map_prepared(&mut p, &mut f) {
            return Err(LayoutError::Internal(MEMO_ERR));
        }
        Ok(p)
    }
}

/// The model run a key names (its address), `None` for a slice or a synthetic run.
fn model_run_addr(ldoc: &LDoc<'_>, k: RunKey) -> Option<usize> {
    match ldoc.run(k)? {
        RunSrc::Model { run, .. } => Some(addr::<Run>(run)),
        RunSrc::Slice { .. } | RunSrc::Synth(_) => None,
    }
}

/// How many field results (`field_result_key(k, 0..)`) a run has.
fn field_results(ldoc: &LDoc<'_>, k: RunKey) -> u32 {
    let mut n = 0u32;
    while ldoc.field_result_key(k, n).is_some() {
        n += 1;
    }
    n
}

/// Replays the recorded flow calls on the live state: `None` when every answer is the recorded
/// one, else the calls done so far (the last one the first with another answer), with their
/// live answers and current keys, to be served to the prepare that follows.
fn replay(e: &Entry, eng: &mut Engine, cx: &mut Cx<'_, '_>, keys: &[RunKey]) -> LResult<Option<VecDeque<FlowOp>>> {
    let mut done = VecDeque::with_capacity(e.ops.len());
    for op in &e.ops {
        let op = op.map_keys(&mut |r| keys.get(r.ix()).copied()).ok_or(LayoutError::Internal(MEMO_ERR))?;
        let (live, same) = match op {
            FlowOp::Field { src, name, instr, at, ans } => {
                let live = PrepFlow::field(eng, cx, src, &name, &instr, at)?;
                let same = live == ans;
                (FlowOp::Field { src, name, instr, at, ans: live }, same)
            }
            FlowOp::Number { num_id, ilvl, ans } => {
                let live = PrepFlow::number_next(eng, cx.ldoc.view.formatting(), num_id, ilvl);
                let same = dbg_eq(&live, &ans);
                (FlowOp::Number { num_id, ilvl, ans: live }, same)
            }
            FlowOp::NoteMark { kind, id, custom, lang, cap0, cap1, ans } => {
                let mut cap = cap0;
                let live = PrepFlow::note_mark(eng, kind, id, custom, &lang, &mut cap);
                let same = live == ans && cap == cap1;
                (FlowOp::NoteMark { kind, id, custom, lang, cap0, cap1: cap, ans: live }, same)
            }
            FlowOp::OnField { run, v } => {
                let model = field_model_ref(cx.ldoc, cx.opts.collect_fields.is_some(), &run);
                PrepFlow::on_field(eng, &run, model, &v);
                (FlowOp::OnField { run, v }, true)
            }
            FlowOp::Warn { msg } => {
                PrepFlow::warn(eng, &msg);
                (FlowOp::Warn { msg }, true)
            }
        };
        done.push_back(live);
        if !same {
            return Ok(Some(done));
        }
    }
    Ok(None)
}

/// The flow of a recorded prepare: serves the calls the replay already did, then calls the
/// engine and logs every call with its answer.
struct RecFlow<'e> {
    eng: &'e mut Engine,
    served: VecDeque<FlowOp>,
    /// a served call was not the one the prepare made (impossible for a deterministic prepare)
    diverged: bool,
    ops: Vec<FlowOp>,
    /// work units charged inside live flow calls
    flow_charge: u64,
}

impl RecFlow<'_> {
    /// The next served call when it is `want` (the answer is the served one's).
    fn serve(&mut self, want: &FlowOp) -> Option<FlowOp> {
        let op = self.served.pop_front()?;
        if !op.same_call(want) {
            self.diverged = true;
        }
        self.ops.push(op.clone());
        Some(op)
    }
}

impl PrepFlow for RecFlow<'_> {
    fn ctx(&mut self) -> &mut DocContext {
        &mut self.eng.ctx
    }

    fn field(&mut self, cx: &mut Cx<'_, '_>, src: FieldSource, name: &[u16], instr: &[u16], at: f64) -> LResult<Option<Box<[u16]>>> {
        let want = FlowOp::Field { src, name: name.into(), instr: instr.into(), at, ans: None };
        if let Some(op) = self.serve(&want) {
            return match op {
                FlowOp::Field { ans, .. } => Ok(ans),
                _ => Err(LayoutError::Internal(MEMO_ERR)),
            };
        }
        let s0 = cx.budget.spent();
        let r = PrepFlow::field(&mut *self.eng, cx, src, name, instr, at);
        self.flow_charge = self.flow_charge.saturating_add(cx.budget.spent().saturating_sub(s0));
        if let (Ok(ans), FlowOp::Field { src, name, instr, at, .. }) = (&r, want) {
            self.ops.push(FlowOp::Field { src, name, instr, at, ans: ans.clone() });
        }
        r
    }

    fn number_next(&mut self, fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<NumberMarker> {
        let want = FlowOp::Number { num_id, ilvl, ans: None };
        if let Some(op) = self.serve(&want) {
            return match op {
                FlowOp::Number { ans, .. } => ans,
                _ => None,
            };
        }
        let ans = PrepFlow::number_next(&mut *self.eng, fmt, num_id, ilvl);
        self.ops.push(FlowOp::Number { num_id, ilvl, ans: ans.clone() });
        ans
    }

    fn note_mark(&mut self, kind: NoteKindFlow, id: f64, custom: bool, lang: &str, cap: &mut TextCap) -> Box<[u16]> {
        let want = FlowOp::NoteMark { kind, id, custom, lang: lang.into(), cap0: *cap, cap1: *cap, ans: Box::default() };
        if let Some(op) = self.serve(&want) {
            return match op {
                FlowOp::NoteMark { cap1, ans, .. } => {
                    *cap = cap1;
                    ans
                }
                _ => Box::default(),
            };
        }
        let cap0 = *cap;
        let ans = PrepFlow::note_mark(&mut *self.eng, kind, id, custom, lang, cap);
        self.ops.push(FlowOp::NoteMark { kind, id, custom, lang: lang.into(), cap0, cap1: *cap, ans: ans.clone() });
        ans
    }

    fn on_field(&mut self, run: &FieldRun, at: Option<&crate::model::RunRef>, v: &[u16]) {
        let want = FlowOp::OnField { run: run.clone(), v: v.into() };
        if self.serve(&want).is_some() {
            return;
        }
        PrepFlow::on_field(&mut *self.eng, run, at, v);
        self.ops.push(want);
    }

    fn warn(&mut self, msg: &str) {
        let want = FlowOp::Warn { msg: msg.into() };
        if self.serve(&want).is_some() {
            return;
        }
        PrepFlow::warn(&mut *self.eng, msg);
        self.ops.push(want);
    }
}

/// Prepares the paragraph (serving `served` first) and keeps the result when it can be.
#[expect(clippy::too_many_arguments, reason = "the memo, the pass, the paragraph and the replay's state")]
fn record(
    m: &mut ParaMemo,
    eng: &mut Engine,
    cx: &mut Cx<'_, '_>,
    (k, lp, id, para): (LParaId, &LPara<'_>, ParaId, &Paragraph),
    env: &ParaEnv,
    env_key: String,
    served: VecDeque<FlowOp>,
    lens0: Option<CallLens>,
) -> LResult<PreparedPara> {
    let lens_before = lens0.unwrap_or_else(|| CallLens::of_cx(cx));
    let spent0 = cx.budget.spent();
    let tap_outer = cx.resolver.tap.replace(indexmap::IndexMap::default());
    let mut rf = RecFlow { eng, served, diverged: false, ops: Vec::new(), flow_charge: 0 };
    let r = prepare_paragraph(cx, &mut rf, k, env);
    let tap = std::mem::replace(&mut cx.resolver.tap, tap_outer);
    let (diverged, ops, flow_charge, left) = (rf.diverged, rf.ops, rf.flow_charge, rf.served.len());
    if diverged || left > 0 {
        return Err(LayoutError::Internal(MEMO_ERR));
    }
    let p = r?;
    if CallLens::of_cx(cx) != lens_before {
        m.report.unmemoizable += 1;
        return Ok(p);
    }
    let charge = cx.budget.spent().saturating_sub(spent0).saturating_sub(flow_charge);
    let ldoc = cx.ldoc;
    let entry = (|| -> Option<Entry> {
        let node = m.node(ldoc, id).filter(|n| std::ptr::eq(para, Arc::as_ptr(n)))?;
        let mut runs = Vec::with_capacity(lp.runs.len());
        let mut rel: FxHashMap<RunKey, (u32, Option<u32>)> = FxHashMap::default();
        for (i, &rk) in lp.runs.iter().enumerate() {
            let i = u32::try_from(i).ok()?;
            let n = field_results(ldoc, rk);
            runs.push((model_run_addr(ldoc, rk)?, n));
            rel.insert(rk, (i, None));
            for j in 0..n {
                rel.insert(ldoc.field_result_key(rk, j)?, (i, Some(j)));
            }
        }
        let mut refs: Vec<KeyRef> = Vec::new();
        let mut ix: FxHashMap<RunKey, RunKey> = FxHashMap::default();
        let mut f = |key: RunKey| -> Option<RunKey> {
            if let Some(r) = ix.get(&key) {
                return Some(*r);
            }
            let (run, sub) = *rel.get(&key)?;
            let r = RunKey(u32::try_from(refs.len()).ok()?);
            refs.push(KeyRef { run, sub, obj: model_run_addr(ldoc, key)? });
            ix.insert(key, r);
            Some(r)
        };
        let mut prep = p.clone();
        if !map_prepared(&mut prep, &mut f) {
            return None;
        }
        let ops: Vec<FlowOp> = ops.iter().map(|o| o.map_keys(&mut f)).collect::<Option<_>>()?;
        // every face of the result was asked in this prepare (so the key check covers it)
        let tap = tap?;
        let asked: Vec<FaceId> = tap.values().copied().collect();
        if !faces_of(&prep).iter().all(|f| asked.contains(f)) {
            return None;
        }
        let packed = Packed::of(prep);
        let mut faces = Vec::with_capacity(tap.len());
        for (key, fid) in tap {
            let aid = cx.fonts.entry(fid)?.answer.id();
            faces.push((key, fid, aid));
        }
        let bytes = std::mem::size_of::<Entry>()
            + packed.bytes()
            + env_key.len()
            + runs.len() * 16
            + refs.len() * std::mem::size_of::<KeyRef>()
            + faces.iter().map(|(k, ..)| k.len() + 32).sum::<usize>()
            + ops.iter().map(FlowOp::bytes).sum::<usize>();
        Some(Entry { node, env: env_key, depth: cx.depth, runs, refs, faces, ops, charge, prep: packed, bytes, epoch: m.epoch })
    })();
    match entry {
        Some(e) => m.keep(id, e),
        None => m.report.unmemoizable += 1,
    }
    Ok(p)
}

/// Maps every run key of a result (`false` when one does not map, or the result holds a
/// call-local id other than a face: an image or a text effect). Exhaustive: a new field does
/// not compile until it is classed here.
fn map_prepared(p: &mut PreparedPara, f: &mut impl FnMut(RunKey) -> Option<RunKey>) -> bool {
    let PreparedPara {
        para: _,
        ppr: _,
        style_id: _,
        styles,
        mark_style: _,
        mark_face: _,
        glyphs,
        marker,
        ind_left: _,
        ind_right: _,
        first_line: _,
        jc: _,
        tabs: _,
        space_before: _,
        space_after: _,
        before_auto: _,
        after_auto: _,
        line_rule: _,
        line: _,
        contextual: _,
        keep_next: _,
        keep_lines: _,
        widow_control: _,
        page_break_before: _,
        empty: _,
        rev_bars: _,
        math: _,
    } = p;
    for s in styles.iter() {
        let RunStyle {
            rpr: _,
            size_pt: _,
            color: _,
            auto_color: _,
            bold: _,
            italic: _,
            underline: _,
            strike: _,
            dstrike: _,
            highlight: _,
            shading: _,
            shift: _,
            hidden: _,
            link: _,
            alpha: _,
            outline: _,
            fx,
            fx_key: _,
        } = s;
        if fx.is_some() {
            return false;
        }
    }
    if let Some(m) = marker {
        let ParaMarker { text: _, glyphs: mg, width: _, level: _, suff: _, jc: _, pic_bullet, chapter: _ } = m;
        if pic_bullet.is_some() {
            return false;
        }
        if !mg.iter_mut().all(|g| map_glyph(g, f)) {
            return false;
        }
    }
    glyphs.iter_mut().all(|g| map_glyph(g, f))
}

fn map_glyph(g: &mut Glyph, f: &mut impl FnMut(RunKey) -> Option<RunKey>) -> bool {
    let Glyph {
        kind: _,
        ch: _,
        cp: _,
        face: _,
        style: _,
        adv: _,
        asc: _,
        desc: _,
        n_asc: _,
        n_desc: _,
        offset: _,
        run_idx: _,
        flags: _,
        obj,
        hr: _,
        ptab,
        size_pt: _,
        check_box: _,
        note_id: _,
        fe_shrink: _,
        kern_adj: _,
        lig_start: _,
        lig_end: _,
        sub_gid: _,
        math: _,
        pic_bullet: _,
    } = g;
    for k in [obj, ptab].into_iter().flatten() {
        match f(*k) {
            Some(n) => *k = n,
            None => return false,
        }
    }
    true
}

/// Every face id a result holds.
fn faces_of(p: &PreparedPara) -> Vec<FaceId> {
    let mut v = vec![p.mark_face];
    let none: &[Glyph] = &[];
    let marker = p.marker.as_ref().map_or(none, |m| m.glyphs.as_slice());
    v.extend(p.glyphs.iter().chain(marker).map(|g| g.face));
    for b in &p.math {
        math_faces(b, &mut v);
    }
    v.sort_unstable_by_key(|f| f.index());
    v.dedup();
    v
}

fn math_faces(b: &MathBox, v: &mut Vec<FaceId>) {
    let MathBox { w: _, items, rules: _, top: _, bottom: _, line_top: _, line_bottom: _ } = b;
    for it in items {
        let MathItem { cp: _, x: _, y: _, face, size_pt: _, style: _ } = it;
        v.push(*face);
    }
}

/// A glyph without any optional field (most of them), in 48 bytes instead of a [`Glyph`]'s
/// 200: its ascent, descent and size go to a table of the paragraph (they repeat per run).
#[derive(Clone, Copy)]
struct Core {
    adv: f64,
    cp: u32,
    face: FaceId,
    style: StyleIx,
    offset: u32,
    run_idx: u32,
    metrics: u32,
    ch: Ch,
    flags: GlyphFlags,
    kind: GlyphKind,
}

/// A memoized result: the glyphs packed (the memo's memory, M5), the rest as is.
struct Packed {
    /// the result without its glyphs
    base: PreparedPara,
    core: Vec<Core>,
    /// (ascent, descent, size) as bits
    metrics: Vec<[u64; 3]>,
    /// the glyphs with an optional field, by index
    extras: Vec<(u32, Glyph)>,
}

impl Packed {
    fn of(mut p: PreparedPara) -> Packed {
        let glyphs = std::mem::take(&mut p.glyphs);
        let mut core = Vec::with_capacity(glyphs.len());
        let mut metrics: Vec<[u64; 3]> = Vec::new();
        let mut seen: FxHashMap<[u64; 3], u32> = FxHashMap::default();
        let mut extras = Vec::new();
        for (i, g) in glyphs.into_iter().enumerate() {
            match plain(&g) {
                Some(m) => {
                    let ix = *seen.entry(m).or_insert_with(|| {
                        metrics.push(m);
                        u32::try_from(metrics.len() - 1).unwrap_or(u32::MAX)
                    });
                    core.push(Core {
                        adv: g.adv,
                        cp: g.cp,
                        face: g.face,
                        style: g.style,
                        offset: g.offset,
                        run_idx: g.run_idx,
                        metrics: ix,
                        ch: g.ch,
                        flags: g.flags,
                        kind: g.kind,
                    });
                }
                None => {
                    // a placeholder keeps the indices; the full glyph replaces it
                    core.push(Core {
                        adv: 0.0,
                        cp: 0,
                        face: g.face,
                        style: g.style,
                        offset: 0,
                        run_idx: 0,
                        metrics: u32::MAX,
                        ch: Ch::default(),
                        flags: GlyphFlags::default(),
                        kind: g.kind,
                    });
                    extras.push((u32::try_from(i).unwrap_or(u32::MAX), g));
                }
            }
        }
        Packed { base: p, core, metrics, extras }
    }

    fn unpack(&self) -> PreparedPara {
        let mut p = self.base.clone();
        let mut glyphs: Vec<Glyph> = Vec::with_capacity(self.core.len());
        let mut extras = self.extras.iter().peekable();
        for (i, c) in self.core.iter().enumerate() {
            if let Some((_, g)) = extras.next_if(|(j, _)| *j as usize == i) {
                glyphs.push(g.clone());
                continue;
            }
            let [asc, desc, size] = self.metrics.get(c.metrics as usize).copied().unwrap_or_default();
            let mut g = Glyph::new(c.kind, c.ch, c.cp, c.face, c.style);
            g.adv = c.adv;
            g.asc = f64::from_bits(asc);
            g.desc = f64::from_bits(desc);
            g.size_pt = f64::from_bits(size);
            g.offset = c.offset;
            g.run_idx = c.run_idx;
            g.flags = c.flags;
            glyphs.push(g);
        }
        p.glyphs = glyphs;
        p
    }

    /// Estimated heap bytes.
    fn bytes(&self) -> usize {
        prep_bytes(&self.base)
            + self.core.len() * std::mem::size_of::<Core>()
            + self.metrics.len() * 24
            + self.extras.len() * (std::mem::size_of::<Glyph>() + 8)
    }
}

/// The (ascent, descent, size) bits of a glyph whose optional fields are all absent, i.e. that
/// [`Glyph::new`] plus the fields of [`Core`] rebuild exactly. Exhaustive: a new field does not
/// compile until it is classed here.
fn plain(g: &Glyph) -> Option<[u64; 3]> {
    let Glyph {
        kind: _,
        ch: _,
        cp: _,
        face: _,
        style: _,
        adv: _,
        asc,
        desc,
        n_asc,
        n_desc,
        offset: _,
        run_idx: _,
        flags: _,
        obj,
        hr,
        ptab,
        size_pt,
        check_box,
        note_id,
        fe_shrink,
        kern_adj,
        lig_start,
        lig_end,
        sub_gid,
        math,
        pic_bullet,
    } = g;
    let absent = |o: &OptF64| !o.is_some();
    let all_absent = absent(n_asc)
        && absent(n_desc)
        && obj.is_none()
        && absent(hr)
        && ptab.is_none()
        && check_box.is_none()
        && absent(note_id)
        && absent(fe_shrink)
        && absent(kern_adj)
        && lig_start.is_none()
        && lig_end.is_none()
        && sub_gid.is_none()
        && math.is_none()
        && pic_bullet.is_none();
    all_absent.then(|| [asc.to_bits(), desc.to_bits(), size_pt.to_bits()])
}

/// Estimated heap bytes of a result (the cap of [`MemoCaps::bytes`]).
fn prep_bytes(p: &PreparedPara) -> usize {
    let s16 = |s: &[u16]| s.len() * 2;
    let glyphs = |g: &[Glyph]| std::mem::size_of_val(g) + g.iter().filter(|x| x.pic_bullet.is_some()).count() * 24;
    std::mem::size_of::<PreparedPara>()
        + glyphs(&p.glyphs)
        + p.styles.len() * std::mem::size_of::<RunStyle>()
        + p.styles
            .iter()
            .map(|s| {
                s16(&s.color)
                    + s.underline.as_ref().map_or(0, |(a, b)| a.len() + b.as_ref().map_or(0, |b| b.len()))
                    + s.highlight.as_ref().map_or(0, |h| h.len())
                    + s.shading.as_ref().map_or(0, |h| h.len())
                    + s.link.as_ref().map_or(0, |l| s16(l))
                    + s.fx_key.as_ref().map_or(0, |k| k.len())
            })
            .sum::<usize>()
        + p.marker.as_ref().map_or(0, |m| std::mem::size_of::<ParaMarker>() + glyphs(&m.glyphs) + s16(&m.text) + m.jc.len())
        + p.tabs.len() * std::mem::size_of::<crate::output::TabStop>()
        + p.jc.len()
        + p.style_id.as_ref().map_or(0, |s| s.len())
        + p.rev_bars.as_ref().map_or(0, |r| 64 + r.ranges.len() * 24)
        + p.math
            .iter()
            .map(|b| std::mem::size_of::<MathBox>() + b.items.len() * std::mem::size_of::<MathItem>() + b.rules.len() * 64)
            .sum::<usize>()
}
