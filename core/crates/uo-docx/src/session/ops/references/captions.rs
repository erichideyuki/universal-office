//! Captions: the settings (labels, numbering per label, AutoCaption), the label list,
//! `insertCaption`, `captionRuns`, `keepTableWithNext` and `autoCaptionTable` (universal-write.ts
//! 7484–7664). The settings are element state, not document state: they take no undo step (each
//! change only reports `uw-caption-settings`).

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;

use super::super::super::command::{CommandError, Event, Outcome};
use super::super::super::state::Selection;
use super::super::super::text::{CarriedProps, DocPosition, DocRange, normalize_range, para_length};
use super::super::super::{ExecCx, Session};
use super::super::context::EditContext;
use super::super::styles::UiLang;
use super::NumberingMap;
use super::pure::{BUILTIN_LABELS, CaptionNumbering, caption_fields, chapter_separator_text, seq_label};
use crate::model::para::{ParaProps, Paragraph};
use crate::model::run::{Run, RunProps};
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::view::{RunRef, RunSub};
use crate::word::obj;

/// Where a caption goes relative to its object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionPosition {
    Above,
    Below,
}

/// What the caption would caption now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionTarget {
    Table,
    Image,
}

/// The AutoCaption of a table (`auto.table`).
#[derive(Debug, Clone, PartialEq)]
pub struct AutoCaption {
    pub label: String,
    pub position: CaptionPosition,
}

/// `CaptionSettings['auto']`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoCaptionCfg {
    pub table: Option<AutoCaption>,
}

/// `CaptionSettings`: the labels created, the numbering per label and the AutoCaption.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptionSettings {
    pub labels: Vec<String>,
    pub numbering: NumberingMap,
    pub auto: Option<AutoCaptionCfg>,
}

/// The element state (`captionCfg`).
pub type CaptionState = CaptionSettings;

/// `CaptionLabelInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionLabelInfo {
    pub name: String,
    pub built_in: bool,
    pub position: CaptionPosition,
}

/// `CaptionOptions`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptionOptions {
    pub label: String,
    /// the text after the number
    pub title: Option<String>,
    /// with an image or a table selected
    pub position: Option<CaptionPosition>,
    /// Exclude label from caption
    pub exclude_label: Option<bool>,
    /// the numbering (default: the label's)
    pub numbering: Option<CaptionNumbering>,
}

/// The most labels the settings keep (M5: the TS is unbounded; F10-D8).
pub const MAX_CAPTION_LABELS: usize = 10_000;

fn lower(s: &str) -> String {
    uo_jsnum::text::to_lowercase(s)
}

impl Session {
    // ── the settings ───────────────────────────────────────────────────────────────────────

    /// `captionSettings` (the getter: a deep copy).
    #[must_use]
    pub fn caption_settings(&self) -> CaptionSettings {
        self.f10.caption.clone()
    }

    /// `captionSettings = v` (no event, no step).
    pub(crate) fn set_caption_settings(&mut self, v: CaptionSettings) -> Result<Outcome, CommandError> {
        let mut labels = v.labels;
        labels.truncate(MAX_CAPTION_LABELS);
        self.f10.caption = CaptionSettings { labels, numbering: v.numbering, auto: v.auto };
        self.done_nothing()
    }

    /// The `lang` label table of `BUILTIN_LABELS`.
    fn builtin_label_name(b: &super::pure::BuiltinLabel, lang: UiLang) -> &'static str {
        match lang {
            UiLang::En => b.en,
            UiLang::Pt => b.pt,
            UiLang::Es => b.es,
        }
    }

    /// Every paragraph of the document in `allParagraphs()` order (the body with its textboxes,
    /// the body tables' cells, the header/footer parts, the notes), each visited once per path (the
    /// TS dedupes by identity; the callers here do not care).
    pub(crate) fn all_paragraphs(&self, f: &mut dyn FnMut(&Paragraph)) {
        fn visit(ps: &mut dyn Iterator<Item = &Paragraph>, f: &mut dyn FnMut(&Paragraph), depth: usize) {
            for p in ps {
                f(p);
                if depth < 32 {
                    for r in &p.runs {
                        if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                            let flat = tb.content.flat_paragraphs();
                            visit(&mut flat.into_iter(), f, depth + 1);
                        }
                    }
                }
            }
        }
        fn table(t: &crate::model::table::Table, f: &mut dyn FnMut(&Paragraph), depth: usize) {
            if depth > 64 {
                return;
            }
            for row in &t.rows {
                for c in &row.cells {
                    visit(&mut c.content.paras.iter(), f, 0);
                    for a in c.content.tables.iter() {
                        table(&a.table, f, depth + 1);
                    }
                }
            }
        }
        visit(&mut self.doc.body.paras.iter(), f, 0);
        for a in self.doc.body.tables.iter() {
            table(&a.table, f, 0);
        }
        for s in self.doc.sections.iter() {
            for set in [&s.headers, &s.footers] {
                for slot in ["default", "first", "even"] {
                    if let Some(part) = set.get(slot).copied()
                        && let Some(h) = self.doc.hf.get(part.index())
                    {
                        visit(&mut h.story.paras.iter(), f, 0);
                    }
                }
            }
        }
        for m in [&self.doc.notes.footnotes, &self.doc.notes.endnotes] {
            for st in m.values() {
                visit(&mut st.paras.iter(), f, 0);
            }
        }
    }

    /// `getCaptionLabels()`: the built-in labels (in the interface language), the created ones and
    /// those the document uses in SEQ fields.
    #[must_use]
    pub fn caption_labels(&self) -> Vec<CaptionLabelInfo> {
        let lang = self.ui_lang();
        let mut out: Vec<CaptionLabelInfo> = BUILTIN_LABELS
            .iter()
            .map(|b| CaptionLabelInfo {
                name: Self::builtin_label_name(b, lang).to_owned(),
                built_in: true,
                position: if b.above { CaptionPosition::Above } else { CaptionPosition::Below },
            })
            .collect();
        for n in &self.f10.caption.labels {
            if !out.iter().any(|x| lower(&x.name) == lower(n)) {
                out.push(CaptionLabelInfo { name: n.clone(), built_in: false, position: CaptionPosition::Below });
            }
        }
        let mut found: Vec<String> = Vec::new();
        self.all_paragraphs(&mut |p| {
            for r in &p.runs {
                let l = seq_label(r.ext().and_then(|e| e.field.as_deref()));
                if !l.is_empty() {
                    found.push(l);
                }
            }
        });
        for l in found {
            if !out.iter().any(|x| lower(&x.name) == lower(&l)) {
                out.push(CaptionLabelInfo { name: l, built_in: false, position: CaptionPosition::Below });
            }
        }
        out
    }

    /// `getCaptionLabels().some(x => x.name.toLowerCase() === n.toLowerCase())` without building
    /// the list (it is called once per label added).
    fn caption_label_taken(&self, n: &str) -> bool {
        let eq = |a: &str| -> bool { if a.is_ascii() && n.is_ascii() { a.eq_ignore_ascii_case(n) } else { lower(a) == lower(n) } };
        let lang = self.ui_lang();
        if BUILTIN_LABELS.iter().any(|b| eq(Self::builtin_label_name(b, lang))) || self.f10.caption.labels.iter().any(|l| eq(l)) {
            return true;
        }
        let mut hit = false;
        self.all_paragraphs(&mut |p| {
            if !hit {
                hit = p.runs.iter().any(|r| {
                    let l = seq_label(r.ext().and_then(|e| e.field.as_deref()));
                    !l.is_empty() && eq(&l)
                });
            }
        });
        hit
    }

    /// `addCaptionLabel(name)` (New Label).
    pub(crate) fn add_caption_label(&mut self, name: &str) -> Result<Outcome, CommandError> {
        let n = uo_jsnum::text::trim(name).to_owned();
        let taken = n.is_empty() || self.f10.caption.labels.len() >= MAX_CAPTION_LABELS || self.caption_label_taken(&n);
        if taken {
            self.answer_bool(false);
            return self.done_nothing();
        }
        self.f10.caption.labels.push(n);
        self.emit(Event::CaptionSettings);
        self.answer_bool(true);
        self.done_nothing()
    }

    /// `deleteCaptionLabel(name)` (only the created ones).
    pub(crate) fn delete_caption_label(&mut self, name: &str) -> Result<Outcome, CommandError> {
        let Some(i) = self.f10.caption.labels.iter().position(|x| lower(x) == lower(name)) else {
            self.answer_bool(false);
            return self.done_nothing();
        };
        self.f10.caption.labels.remove(i);
        self.f10.caption.numbering.shift_remove(name);
        self.emit(Event::CaptionSettings);
        self.answer_bool(true);
        self.done_nothing()
    }

    /// `getCaptionNumbering(label)`.
    #[must_use]
    pub fn caption_numbering(&self, label: &str) -> CaptionNumbering {
        self.f10.caption.numbering.get(label).copied().unwrap_or_default()
    }

    /// `setCaptionNumbering(label, n)`.
    pub(crate) fn set_caption_numbering(&mut self, label: &str, n: CaptionNumbering) -> Result<Outcome, CommandError> {
        self.f10.caption.numbering.insert(label.to_owned(), n);
        self.emit(Event::CaptionSettings);
        self.done_nothing()
    }

    /// `getAutoCaption()`.
    #[must_use]
    pub fn auto_caption(&self) -> Option<AutoCaptionCfg> {
        self.f10.caption.auto.clone()
    }

    /// `setAutoCaption(auto)`.
    pub(crate) fn set_auto_caption(&mut self, auto: Option<(String, CaptionPosition)>) -> Result<Outcome, CommandError> {
        self.f10.caption.auto = auto.map(|(label, position)| AutoCaptionCfg { table: Some(AutoCaption { label, position }) });
        self.emit(Event::CaptionSettings);
        self.done_nothing()
    }

    // ── the target ─────────────────────────────────────────────────────────────────────────

    /// `captionTarget()`: a table (the caret in it), the image selected, or nothing.
    #[must_use]
    pub fn caption_target(&self) -> Option<CaptionTarget> {
        if matches!(self.ctx, EditContext::Cell { .. }) || self.table_sel.is_some() {
            return Some(CaptionTarget::Table);
        }
        self.selected_inline_image().map(|_| CaptionTarget::Image)
    }

    /// `selectedInlineImage()`: an inline image alone in the selection (its paragraph and offset).
    fn selected_inline_image(&self) -> Option<DocPosition> {
        if !self.has_selection() {
            return None;
        }
        let DocRange { start, end } = normalize_range(self.range());
        if start.para != end.para || end.offset.wrapping_sub(start.offset) != 1 {
            return None;
        }
        let mut acc = 0usize;
        for r in &self.paras()?.get(start.para as usize)?.runs {
            if acc == start.offset as usize && r.ext().and_then(|e| e.image.as_deref()).is_some_and(|i| i.floating != Some(true)) {
                return Some(start);
            }
            acc += r.text.len();
        }
        None
    }

    // ── insertCaption ──────────────────────────────────────────────────────────────────────

    /// `captionRuns(label, n, opts)`: "Label " + [chapter + separator] + number + text.
    fn caption_runs(label: &str, n: &CaptionNumbering, opts: &CaptionOptions) -> Vec<Run> {
        let f = caption_fields(label, n);
        let plain = |text: &str| Run { text: Utf16::from(text), props: Arc::new(default_run_props()), ..Run::default() };
        let field_run = |text: &str, instr: &str| {
            let mut r = plain(text);
            let mut np: RunProps = default_run_props();
            obj::set(&mut np, "noProof", |x| x.no_proof = Some(true));
            let res = Run { text: Utf16::from(text), props: Arc::new(np), ..Run::default() };
            let e = r.ext_mut();
            e.field = Some(instr.into());
            e.field_result = Some(vec![res]);
            r
        };
        let mut out: Vec<Run> = Vec::new();
        if opts.exclude_label != Some(true) {
            out.push(plain(&format!("{label} ")));
        }
        if let (Some(ch), Some(c)) = (&f.chapter, &n.chapter) {
            out.push(field_run("0", ch));
            out.push(plain(chapter_separator_text(c.separator)));
        }
        out.push(field_run("1", &f.seq));
        if let Some(t) = opts.title.as_deref().filter(|t| !t.is_empty()) {
            out.push(plain(t));
        }
        out
    }

    /// `keepTableWithNext(t)`: a caption below the table: the last row stays with the next
    /// paragraph (`keepNext` on the paragraph of the last cell).
    fn keep_table_with_next(&mut self, anchor: usize) {
        let Some(a) = self.doc.body.tables.make_mut(anchor) else { return };
        let t = Arc::make_mut(&mut a.table);
        let Some(cell) = t.rows.last_mut().and_then(|r| r.cells.last_mut()) else { return };
        let Some(n) = cell.content.paras.len().checked_sub(1) else { return };
        if let Some(p) = cell.content.paras.make_mut(n) {
            let mut props = (*p.props).clone();
            obj::set(&mut props, "keepNext", |x| x.keep_next = Some(true));
            p.props = Arc::new(props);
        }
    }

    /// The RunRefs of the runs of `paras[para]` selected by `pick`.
    fn caption_run_refs(&self, para: u32, pick: &dyn Fn(&Run) -> bool, out: &mut Vec<RunRef>) {
        let Some(p) = self.paras().and_then(|ps| ps.get(para as usize)) else { return };
        let Some((story, path)) = self.para_ref(para) else { return };
        for (ri, r) in p.runs.iter().enumerate() {
            if pick(r) {
                let rr = RunRef { story, path: path.clone(), run: u32::try_from(ri).unwrap_or(u32::MAX), sub: RunSub::None };
                if !out.contains(&rr) {
                    out.push(rr);
                }
            }
        }
    }

    /// `insertCaption(opts)`.
    pub(crate) fn insert_caption(&mut self, cx: &mut ExecCx<'_>, opts: &CaptionOptions) -> Result<Outcome, CommandError> {
        let ok = self.insert_caption_inner(cx, opts)?;
        self.answer_bool(ok);
        if ok { self.done_step() } else { self.done_nothing() }
    }

    /// `insertCaption` without the final settle (`autoCaptionTable` runs inside `insertTable`).
    fn insert_caption_inner(&mut self, cx: &mut ExecCx<'_>, opts: &CaptionOptions) -> Result<bool, CommandError> {
        if self.paras().is_none()
            || self.locked()
            || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. })
        {
            return Ok(false);
        }
        let label = uo_jsnum::text::trim(&opts.label).to_owned();
        if label.is_empty() {
            return Ok(false);
        }
        let numbering = opts.numbering.unwrap_or_else(|| self.caption_numbering(&label));
        let target = self.caption_target();
        // without an object the caption goes at the caret: not in a plain-text content control
        if target.is_none() && !self.can_insert_field() {
            return Ok(false);
        }
        let position = opts
            .position
            .unwrap_or_else(|| self.caption_labels().iter().find(|l| l.name == label).map_or(CaptionPosition::Below, |l| l.position));
        self.begin_step(false, &[]);
        let style = match self.builtin_style_id("caption") {
            Ok(s) => s,
            Err(e) => {
                self.discard_step();
                return Err(e);
            }
        };
        let runs = Self::caption_runs(&label, &numbering, opts);
        let caption_para = |ids: &mut crate::ids::IdAlloc, runs: Vec<Run>| -> Paragraph {
            let props = ParaProps {
                style_id: Some(style.clone()),
                keep_next: (position == CaptionPosition::Above).then_some(true),
                ..ParaProps::default()
            };
            Paragraph { runs, props: Arc::new(props), id: ids.para(), ..Paragraph::default() }
        };
        let mut added: Vec<RunRef> = Vec::new();
        let caret: DocPosition;
        match target {
            Some(CaptionTarget::Table) => {
                let anchor: i64 = match &self.ctx {
                    EditContext::Cell { anchor, .. } => i64::from(*anchor),
                    _ => self.table_sel.as_ref().map_or(-1, |t| i64::from(t.anchor)),
                };
                let Some(ai) = usize::try_from(anchor).ok().filter(|i| self.doc.body.tables.get(*i).is_some()) else {
                    // (the step stays: the TS takes it first)
                    return Ok(false);
                };
                self.exit_cell();
                self.table_sel = None;
                let at = self.doc.body.tables.get(ai).map_or(0, |a| a.before_para);
                let mut ids = std::mem::take(&mut self.doc.ids);
                let para = caption_para(&mut ids, runs.clone());
                self.doc.ids = ids;
                let len = para_length(&para);
                if !self.doc.body.paras.insert(at as usize, Arc::new(para)) {
                    self.discard_step();
                    return Err(CommandError::Position);
                }
                // the other anchors move; the bookmarks do not (a TS quirk: no `shiftAnchors` here)
                for i in 0..self.doc.body.tables.len() {
                    if i != ai
                        && self.doc.body.tables.get(i).is_some_and(|x| x.before_para >= at)
                        && let Some(x) = self.doc.body.tables.make_mut(i)
                    {
                        x.before_para += 1;
                    }
                }
                if position == CaptionPosition::Above {
                    if let Some(a) = self.doc.body.tables.make_mut(ai) {
                        a.before_para += 1;
                    }
                } else {
                    self.keep_table_with_next(ai);
                }
                caret = DocPosition::new(at, u32::try_from(len).unwrap_or(u32::MAX));
                for (k, r) in runs.iter().enumerate() {
                    if r.ext().is_some_and(|e| e.field.is_some())
                        && let Some((story, path)) = self.para_ref(at)
                    {
                        added.push(RunRef { story, path, run: u32::try_from(k).unwrap_or(u32::MAX), sub: RunSub::None });
                    }
                }
            }
            Some(CaptionTarget::Image) => {
                let Some(img) = self.selected_inline_image() else {
                    self.discard_step();
                    return Ok(false);
                };
                let at = if position == CaptionPosition::Above { img.para } else { img.para + 1 };
                let mut ids = std::mem::take(&mut self.doc.ids);
                let para = caption_para(&mut ids, runs.clone());
                self.doc.ids = ids;
                let len = para_length(&para);
                if position == CaptionPosition::Below
                    && let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(img.para as usize))
                {
                    let mut props = (*p.props).clone();
                    obj::set(&mut props, "keepNext", |x| x.keep_next = Some(true));
                    p.props = Arc::new(props);
                }
                let Some(ps) = self.paras_mut() else {
                    self.discard_step();
                    return Err(CommandError::Position);
                };
                if !ps.insert(at as usize, Arc::new(para)) {
                    self.discard_step();
                    return Err(CommandError::Position);
                }
                // `shiftAnchors(at - 1, 1)` (from -1 when the caption goes above the first paragraph)
                self.shift_anchors_from(at);
                caret = DocPosition::new(at, u32::try_from(len).unwrap_or(u32::MAX));
                for (k, r) in runs.iter().enumerate() {
                    if r.ext().is_some_and(|e| e.field.is_some())
                        && let Some((story, path)) = self.para_ref(at)
                    {
                        added.push(RunRef { story, path, run: u32::try_from(k).unwrap_or(u32::MAX), sub: RunSub::None });
                    }
                }
            }
            None => {
                if self.has_selection() {
                    match self.delete_selection() {
                        Ok(p) => self.selection = Selection::caret(p),
                        Err(e) => {
                            self.discard_step();
                            return Err(e);
                        }
                    }
                }
                let mut pos = self.selection.focus;
                for r in &runs {
                    let step = if r.ext().is_some_and(|e| e.field.is_some()) {
                        self.insert_whole_run(pos, r.clone())
                    } else {
                        let c = CarriedProps::plain((*r.props).clone());
                        self.insert_typed(pos, r.text.units(), Some(&c))
                    };
                    match step {
                        Ok(p) => pos = p,
                        Err(e) => {
                            self.discard_step();
                            return Err(e);
                        }
                    }
                }
                let patch = ParaProps { style_id: Some(style.clone()), ..ParaProps::default() };
                if let Err(e) = self.apply_para(DocRange::new(pos, pos), &patch) {
                    self.discard_step();
                    return Err(e);
                }
                caret = pos;
                // the runs inserted are copies: the caption's fields by the paragraph
                self.caption_run_refs(pos.para, &|r| r.ext().is_some_and(|e| e.field.is_some()), &mut added);
            }
        }
        self.selection = Selection::caret(caret);
        // the captions of the label are renumbered (Word updates the SEQ that follow)
        let want = lower(&label);
        let n = self.paras().map_or(0, |p| p.len());
        for pi in 0..n {
            self.caption_run_refs(
                u32::try_from(pi).unwrap_or(u32::MAX),
                &|r| lower(&seq_label(r.ext().and_then(|e| e.field.as_deref()))) == want,
                &mut added,
            );
        }
        let only: HashSet<RunRef> = added.into_iter().collect();
        if let Err(e) = self.refresh_field_cache(cx, Some(only)) {
            self.rollback_step();
            return Err(e);
        }
        let fit_len = self.paras().and_then(|ps| ps.get(caret.para as usize)).map_or(0, para_length);
        let off = if target.is_some() { fit_len } else { (caret.offset as usize).min(fit_len) };
        self.selection = Selection::caret(DocPosition::new(caret.para, u32::try_from(off).unwrap_or(u32::MAX)));
        self.touch_structure();
        Ok(true)
    }

    /// `shiftAnchors(at - 1, 1)`: the anchors and bookmarks after paragraph `at - 1` move one
    /// paragraph (`at - 1` is -1 when `at` is 0: every anchor and bookmark moves).
    fn shift_anchors_from(&mut self, at: u32) {
        match at.checked_sub(1) {
            Some(from) => self.shift_anchors(from, 1),
            None => {
                self.shift_anchors(0, 1);
                // the ones exactly at paragraph 0 move too (`> -1`)
                let body = matches!(self.ctx, EditContext::Body);
                let cell = matches!(self.ctx, EditContext::Cell { .. });
                if body || cell {
                    if let Some(story) = self.story_mut() {
                        for i in 0..story.tables.len() {
                            if story.tables.get(i).is_some_and(|a| a.before_para == 0)
                                && let Some(a) = story.tables.make_mut(i)
                            {
                                a.before_para = 1;
                            }
                        }
                    }
                    if body {
                        let map = self.bookmark_paras.map_mut();
                        for (_, v) in map.iter_mut() {
                            if *v == 0 {
                                *v = 1;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `autoCaptionTable()`: after inserting a table, the automatic caption (the AutoCaption
    /// setting; the caret goes back to the first cell). The table and the caption are ONE undo
    /// step.
    pub(crate) fn auto_caption_table(&mut self, cx: &mut ExecCx<'_>) -> Result<(), CommandError> {
        let Some(a) = self.f10.caption.auto.as_ref().and_then(|a| a.table.clone()) else { return Ok(()) };
        let EditContext::Cell { anchor, hops, .. } = &self.ctx else { return Ok(()) };
        let (anchor, hops) = (*anchor, hops.to_vec());
        let before = self.history.undo_len();
        let opts = CaptionOptions { label: a.label, position: Some(a.position), ..CaptionOptions::default() };
        self.insert_caption_inner(cx, &opts)?;
        // one step in the undo (the table and the caption)
        if self.history.undo_len() > before {
            self.history.collapse_to(before);
        }
        self.enter_cell_hops(anchor, hops, None);
        Ok(())
    }
}

/// `IndexMap` re-export keeps the type of the numbering map in one place.
#[allow(dead_code)]
type _Map = IndexMap<String, CaptionNumbering>;
