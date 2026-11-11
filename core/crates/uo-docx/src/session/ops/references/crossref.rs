//! Cross-references: `insertCrossReference`, `insertRefFields`, `refBookmark`, `newHiddenName`,
//! `getCrossReferenceItems`, `nextCaptionNumber` (universal-write.ts 6103–6455).

use std::collections::HashSet;

use super::super::super::command::{CommandError, Outcome};
use super::super::super::settle::block_of_para;
use super::super::super::state::Selection;
use super::super::super::text::{DocPosition, para_length, para_text, props_at};
use super::super::super::{ExecCx, Session};
use super::super::bookmarks::{add_bookmark_marks, bookmarks_in};
use super::super::context::{EditContext, cell_index, resolve_cell};
use super::super::revision_cmds::{CellStep, StoryLoc, story_at, story_at_mut};
use super::pure::{CrossRefKind, CrossRefType, caption_range, cross_ref_instr, hidden_bookmark_name, seq_label};
use super::rx;
use crate::model::run::Run;
use crate::text::Utf16;
use crate::view::{RunRef, RunSub};
use crate::word::numbering::{TextCap, format_number};
use uo_jsnum::format::to_js_string;

/// `CrossRefOptions`.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossRefOptions {
    pub ty: CrossRefType,
    /// the caption label (type `Caption`)
    pub label: Option<String>,
    /// the id of the item (`getCrossReferenceItems`)
    pub item: String,
    pub kind: CrossRefKind,
    /// Insert as hyperlink (`\h`; the default is yes)
    pub hyperlink: Option<bool>,
    /// Include above/below: a REF `\p` after the field
    pub position: Option<bool>,
    /// `\d`, in the full-context number only
    pub separator: Option<String>,
}

/// `CrossRefItem`: an item of the Cross-reference dialog (`p:<paragraph>`, a bookmark name,
/// `n:<note>`).
#[derive(Debug, Clone, PartialEq)]
pub struct CrossRefItem {
    pub id: String,
    pub text: Utf16,
}

/// The count of `'  '.repeat(n)` for an indent level (`n` below 0 is 0; capped at 64 levels).
fn repeat_count(n: f64) -> usize {
    if n >= 64.0 { 64 } else { usize::try_from(uo_jsnum::conv::to_uint32(uo_jsnum::math::max(0.0, n))).unwrap_or(0) }
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn concat(parts: &[&[u16]]) -> Utf16 {
    let mut v: Vec<u16> = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    Utf16::from_units(v)
}

impl Session {
    // ── names and bookmarks ────────────────────────────────────────────────────────────────

    /// `refSeed.n`, drawn at the first use (F10-D2).
    fn ref_seed_value(&mut self) -> f64 {
        if let Some(n) = self.f10.ref_seed {
            return n;
        }
        // `243_000_000 + Math.floor(Math.random() * 400_000)`
        let r = f64::from(self.env.random_u32()) / 4_294_967_296.0;
        let n = 243_000_000.0 + uo_jsnum::math::floor(r * 400_000.0);
        self.f10.ref_seed = Some(n);
        n
    }

    /// `newHiddenName(prefix)`: a `_Ref`/`_Toc` name the document does not use yet.
    pub(crate) fn new_hidden_name(&mut self, prefix: &str) -> String {
        let mut seed = self.ref_seed_value();
        let name = {
            let this: &Session = self;
            hidden_bookmark_name(prefix, &mut |n| this.has_bookmark(n), &mut seed)
        };
        self.f10.ref_seed = Some(seed);
        name
    }

    /// `refBookmark(paras, para, from, to, prefix)` over the story at `loc`: the `_Ref`/`_Toc`
    /// bookmark that covers exactly `[from, to]` of the paragraph (reused, as Word does at the
    /// second reference to the same target) or a new one.
    pub(crate) fn ref_bookmark_in(&mut self, loc: &StoryLoc, para: u32, from: u32, to: u32, prefix: &str) -> Result<String, CommandError> {
        let Some(st) = story_at(&self.doc, loc) else { return Err(CommandError::Position) };
        let hit = bookmarks_in(st.paras.iter()).into_iter().find(|b| {
            b.name.starts_with(prefix) && b.start.para == para && b.end.para == para && b.start.offset == from && b.end.offset == to
        });
        if let Some(b) = hit {
            return Ok(b.name.to_string());
        }
        let name = self.new_hidden_name(prefix);
        let id = self.new_bookmark_id();
        let st = story_at_mut(&mut self.doc, loc).ok_or(CommandError::Position)?;
        add_bookmark_marks(&mut st.paras, &name, id, DocPosition::new(para, from), DocPosition::new(para, to))?;
        Ok(name)
    }

    // ── insertCrossReference ───────────────────────────────────────────────────────────────

    /// `insertCrossReference(opts)`.
    pub(crate) fn insert_cross_reference(&mut self, cx: &mut ExecCx<'_>, opts: &CrossRefOptions) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none()
            || self.locked()
            || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. })
            || !self.can_insert_field()
        {
            return self.done_nothing();
        }
        let target: String;
        match opts.ty {
            CrossRefType::Bookmark => {
                if !self.has_bookmark(&opts.item) {
                    return self.done_nothing();
                }
                target = opts.item.clone();
                self.begin_step(false, &[]);
            }
            CrossRefType::Footnote | CrossRefType::Endnote => {
                let footnote = opts.ty == CrossRefType::Footnote;
                let id = rx::note_item(&opts.item).map_or(f64::NAN, uo_jsnum::parse::to_number);
                let Some(r) = self.note_refs(Some(footnote)).into_iter().find(|r| r.id == id) else { return self.done_nothing() };
                self.begin_step(false, &[]);
                let loc_target: Result<String, CommandError> = if opts.kind == CrossRefKind::Page {
                    // the page of a note: the `_Ref` goes IN the note, around its first paragraph
                    let nk = crate::js::NumKey::new(id);
                    let loc = StoryLoc::Note { footnote, id: nk };
                    let Some(len) = story_at(&self.doc, &loc).and_then(|s| s.paras.get(0)).map(para_length) else {
                        // (the step stays: the TS takes it first)
                        return self.done_nothing();
                    };
                    self.ref_bookmark_in(&loc, 0, 0, u32::try_from(len).unwrap_or(u32::MAX), "_Ref")
                } else {
                    let host_loc: Option<StoryLoc> = match r.cell {
                        None => Some(StoryLoc::Body),
                        Some((anchor, hop)) => self.doc.body.tables.get(anchor as usize).and_then(|a| {
                            resolve_cell(&a.table, &[hop])?;
                            let (_, k) = cell_index(&a.table, &hop)?;
                            Some(StoryLoc::Cell(vec![CellStep { table: anchor, row: hop.row, cell: u32::try_from(k).unwrap_or(u32::MAX) }]))
                        }),
                    };
                    // (a cell that is not there returns with the step taken, as the TS)
                    let Some(host_loc) = host_loc else { return self.done_nothing() };
                    let mut len = 1usize;
                    let mut acc = 0usize;
                    if let Some(p) = story_at(&self.doc, &host_loc).and_then(|s| s.paras.get(r.para as usize)) {
                        for run in &p.runs {
                            if acc == r.offset as usize && run.ext().is_some_and(|e| e.note_ref.is_some()) {
                                len = run.text.len();
                                break;
                            }
                            acc += run.text.len();
                        }
                    }
                    self.ref_bookmark_in(&host_loc, r.para, r.offset, r.offset + u32::try_from(len).unwrap_or(u32::MAX), "_Ref")
                };
                target = match loc_target {
                    Ok(t) => t,
                    Err(e) => {
                        self.discard_step();
                        return Err(e);
                    }
                };
            }
            _ => {
                let Some(para) = rx::para_item(&opts.item).map(uo_jsnum::parse::to_number) else { return self.done_nothing() };
                let body_n = self.doc.body.paras.len();
                if !(para >= 0.0 && para < body_n as f64) {
                    return self.done_nothing();
                }
                let pi = uo_jsnum::conv::to_uint32(para);
                self.begin_step(false, &[]);
                let (len, seq_end) = {
                    let Some(p) = self.doc.body.paras.get(pi as usize) else {
                        self.discard_step();
                        return self.done_nothing();
                    };
                    let len = para_length(p);
                    let mut acc = 0usize;
                    let mut seq_end: Option<usize> = None;
                    for r in &p.runs {
                        acc += r.text.len();
                        if seq_end.is_none() && !seq_label(r.ext().and_then(|e| e.field.as_deref())).is_empty() {
                            seq_end = Some(acc);
                        }
                    }
                    (len, seq_end)
                };
                let (mut from, mut to) = (0usize, len);
                if opts.ty == CrossRefType::Caption {
                    (from, to) = caption_range(opts.kind, len, seq_end.unwrap_or(len));
                }
                match self.ref_bookmark_in(
                    &StoryLoc::Body,
                    pi,
                    u32::try_from(from).unwrap_or(u32::MAX),
                    u32::try_from(to).unwrap_or(u32::MAX),
                    "_Ref",
                ) {
                    Ok(t) => target = t,
                    Err(e) => {
                        self.discard_step();
                        return Err(e);
                    }
                }
            }
        }
        self.insert_ref_fields(cx, &target, opts)
    }

    /// `insertRefFields(name, opts, true)`: the fields of the reference at the caret (the step is
    /// already taken).
    fn insert_ref_fields(&mut self, cx: &mut ExecCx<'_>, name: &str, opts: &CrossRefOptions) -> Result<Outcome, CommandError> {
        if self.has_selection() {
            match self.delete_selection() {
                Ok(pos) => self.selection = Selection::caret(pos),
                Err(e) => {
                    self.discard_step();
                    return Err(e);
                }
            }
        }
        let hyper = Some(opts.hyperlink != Some(false));
        let mut instrs = vec![cross_ref_instr(name, opts.kind, hyper, opts.separator.as_deref())];
        if opts.position == Some(true) && opts.kind != CrossRefKind::Position {
            instrs.push(cross_ref_instr(name, CrossRefKind::Position, hyper, None));
        }
        let mut added: Vec<RunRef> = Vec::new();
        let mut last: Option<(u32, usize)> = None;
        let mut pos = self.selection.focus;
        for (i, instr) in instrs.iter().enumerate() {
            let step: Result<(), CommandError> = (|| {
                if i > 0 {
                    pos = self.insert_typed(pos, &units(" "), None)?;
                }
                let carried = self.paras().map(|ps| props_at(ps, pos)).unwrap_or_default();
                let props = self.new_run_props_from(&carried.props);
                let mut run = Run { text: Utf16::from("?"), props: std::sync::Arc::new(props), ..Run::default() };
                run.rpr_link = carried.link.clone();
                crate::word::obj::set(run.ext_mut(), "field", |e| e.field = Some(instr.as_str().into()));
                pos = self.insert_whole_run(pos, run)?;
                if let Some(p) = self.paras().and_then(|ps| ps.get(pos.para as usize)) {
                    let mut acc = 0usize;
                    for (ri, r) in p.runs.iter().enumerate() {
                        acc += r.text.len();
                        if acc == pos.offset as usize && r.ext().and_then(|e| e.field.as_deref()) == Some(instr.as_str()) {
                            if let Some((story, path)) = self.para_ref(pos.para) {
                                let rr = RunRef { story, path, run: u32::try_from(ri).unwrap_or(u32::MAX), sub: RunSub::None };
                                if !added.contains(&rr) {
                                    added.push(rr);
                                }
                            }
                            last = Some((pos.para, ri));
                            break;
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = step {
                self.discard_step();
                return Err(e);
            }
        }
        self.selection = Selection::caret(pos);
        let only: HashSet<RunRef> = added.iter().cloned().collect();
        // (the TS `[...added].pop()`: the last distinct run added)
        let last_added = added.last().and(last);
        if let Err(e) = self.refresh_field_cache(cx, Some(only)) {
            self.rollback_step();
            return Err(e);
        }
        // the result may differ in size from the "?": the caret goes after the fields
        if let Some((para, ri)) = last_added
            && let Some(p) = self.paras().and_then(|ps| ps.get(para as usize))
        {
            let acc: usize = p.runs.iter().take(ri + 1).map(|r| r.text.len()).sum();
            self.selection = Selection::caret(DocPosition::new(pos.para, u32::try_from(acc).unwrap_or(u32::MAX)));
        }
        self.answer_bool(true);
        self.touch_para(pos.para as usize);
        let cs = self.finish();
        Ok(Outcome::Done(cs))
    }

    // ── queries ────────────────────────────────────────────────────────────────────────────

    /// `crossReferenceKinds(type)`.
    #[must_use]
    pub fn cross_reference_kinds(&self, ty: CrossRefType) -> Vec<CrossRefKind> {
        super::pure::cross_ref_kinds(ty)
    }

    /// `getCrossReferenceItems(type, label)`: the items of the Cross-reference dialog as Word
    /// lists them: headings indented by level, numbered items with the number, visible bookmarks
    /// (in discovery order: the by-name collation is the shell's, F6-D3), notes "mark  text",
    /// the captions of the label.
    #[must_use]
    pub fn cross_reference_items(&self, ty: CrossRefType, label: Option<&str>) -> Vec<CrossRefItem> {
        let body = &self.doc.body;
        match ty {
            CrossRefType::Heading => self
                .headings()
                .into_iter()
                .map(|h| {
                    let pad = vec![32u16; 2 * repeat_count(h.level - 1.0)];
                    CrossRefItem { id: format!("p:{}", h.para), text: concat(&[&pad, h.text.units()]) }
                })
                .collect(),
            CrossRefType::Numbered => {
                let view = crate::view::DocView::new(&self.doc);
                crate::word::outline::numbered_items(view, None)
                    .into_iter()
                    .filter_map(|n| {
                        let pi = super::fields::para_of_block(body, n.block as usize)?;
                        let pad = vec![32u16; 2 * repeat_count(n.ilvl)];
                        Some(CrossRefItem { id: format!("p:{pi}"), text: concat(&[&pad, n.number.units(), &[32], n.text.units()]) })
                    })
                    .collect()
            }
            CrossRefType::Bookmark => self
                .bookmarks(false, false)
                .into_iter()
                .map(|b| CrossRefItem { id: b.name.to_string(), text: Utf16::from(&*b.name) })
                .collect(),
            CrossRefType::Footnote | CrossRefType::Endnote => {
                let footnote = ty == CrossRefType::Footnote;
                let opt = self.note_options(footnote);
                let mut n = opt.start_at - 1.0;
                let mut seen: HashSet<crate::js::NumKey> = HashSet::new();
                let mut out = Vec::new();
                for r in self.note_refs(Some(footnote)) {
                    if !seen.insert(crate::js::NumKey::new(r.id)) {
                        continue;
                    }
                    let story = {
                        let notes = self.doc.notes.as_ref();
                        let map = if footnote { &notes.footnotes } else { &notes.endnotes };
                        map.get(&crate::js::NumKey::new(r.id))
                    };
                    let mut joined: Vec<u16> = Vec::new();
                    for (k, p) in story.iter().flat_map(|s| s.paras.iter()).enumerate() {
                        if k > 0 {
                            joined.push(32);
                        }
                        let mut t = para_text(p);
                        // `String.replace(NOTE_REF_TEXT, '')`: the first one only
                        if let Some(i) = t.iter().position(|&u| u == 0xFFFC) {
                            t.remove(i);
                        }
                        joined.extend(t);
                    }
                    for u in &mut joined {
                        if matches!(*u, 9 | 10) {
                            *u = 32;
                        }
                    }
                    let text = rx::trim_units(&joined).to_vec();
                    n += 1.0;
                    let mut cap = TextCap::default();
                    let num = format_number(n, Some(&opt.number_format), "pt-BR", &mut cap);
                    out.push(CrossRefItem { id: format!("n:{}", to_js_string(r.id)), text: concat(&[num.units(), &[32, 32], &text]) });
                }
                out
            }
            CrossRefType::Caption => {
                let want = uo_jsnum::text::to_lowercase(label.unwrap_or(""));
                let mut out = Vec::new();
                if want.is_empty() {
                    return out;
                }
                for (pi, p) in body.paras.iter().enumerate() {
                    let hit =
                        p.runs.iter().any(|r| uo_jsnum::text::to_lowercase(&seq_label(r.ext().and_then(|e| e.field.as_deref()))) == want);
                    if hit {
                        let mut t = para_text(p);
                        for u in &mut t {
                            if matches!(*u, 9 | 10 | 12) {
                                *u = 32;
                            }
                        }
                        out.push(CrossRefItem { id: format!("p:{pi}"), text: Utf16::from_units(rx::trim_units(&t).to_vec()) });
                    }
                }
                out
            }
        }
    }

    /// `nextCaptionNumber(label)`: the number the next caption of the label gets.
    #[must_use]
    pub fn next_caption_number(&self, label: &str) -> Utf16 {
        let want = uo_jsnum::text::to_lowercase(label);
        let mut n = 0.0_f64;
        for p in self.doc.body.paras.iter() {
            for r in &p.runs {
                if uo_jsnum::text::to_lowercase(&seq_label(r.ext().and_then(|e| e.field.as_deref()))) == want {
                    n += 1.0;
                }
            }
        }
        let f = self.caption_numbering(label).format;
        let mut cap = TextCap::default();
        format_number(n + 1.0, Some(f.number_format()), "pt-BR", &mut cap)
    }

    /// Whether the TS `blockOf` of a paragraph equals itself (kept for the callers' asserts).
    #[allow(dead_code)]
    pub(crate) fn block_index_of_body_para(&self, p: usize) -> usize {
        block_of_para(&self.doc.body, p)
    }
}
