//! Fields: `insertField`, `insertFieldRun`, `updateFields`, `refreshFieldCache` and
//! `setFieldResult` (the oracle's `field_values`), `fieldRunsInSelection`, `formulaWork`, and the
//! insertions of `universal-write.ts` 11585–11640 (`insertDateTime`, `insertDocumentInfo`,
//! `insertAlignmentTab`).
//!
//! A TS `Run` object that the layout reports (`onField(run, value)`) is a [`RunRef`] here: the
//! story, the path of the paragraph block and the run index (K1). [`Session::para_ref`] gives the
//! reference of a paragraph of the edit context and [`Session::field_run_mut`] resolves one back
//! to the model run, copy-on-write down to it.

use std::collections::HashSet;
use std::sync::Arc;

use super::super::super::command::CommandError;
use super::super::super::command::Outcome;
use super::super::super::settle::{block_of_para, sync_paras};
use super::super::super::state::Selection;
use super::super::super::text::{self, CarriedProps, DocPosition, DocRange, json_or_null, normalize_range, para_length, without_revisions};
use super::super::super::{ExecCx, Session};
use super::super::context::{EditContext, cell_index, table_index_of_block};
use super::super::drawing;
use super::super::tables::table_ops::map_table;
use super::datetime::format_now;
use super::pure::{ref_field_target, runs_between};
use super::rx;
use super::{DocInfoKind, F10Result, FieldKind, FieldScopeSel, TocMode, toc_cmds::TocField};
use crate::js::{JsValue, NumKey, Str};
use crate::model::para::Paragraph;
use crate::model::run::{Ptab, Run, RunProps};
use crate::model::story::{Block, Story};
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::view::{BlockPath, FieldScope, NoteKind, RunRef, RunSub, StoryRef};
use crate::word::obj;

/// The depth the table walks stop at (the reader never builds deeper; a hostile hand-built model
/// must not overflow the stack).
const MAX_DEPTH: usize = 64;

/// Wave 7c (W, item 4): Word's result texts of a REF/PAGEREF/NOTEREF to an undefined bookmark (pt-BR measured; en/es Word's
/// UI strings), the ones the layout writes (uo-docx-layout `engine/field_refs.rs`).
fn is_word_ref_error(v: &Utf16) -> bool {
    const ALL: [&str; 6] = [
        "Erro! Fonte de referência não encontrada.",
        "Erro! Indicador não definido.",
        "Error! Reference source not found.",
        "Error! Bookmark not defined.",
        "¡Error! No se encuentra el origen de la referencia.",
        "¡Error! Marcador no definido.",
    ];
    ALL.iter().any(|s| s.encode_utf16().eq(v.units().iter().copied()))
}

/// The paragraph index of block `bi` of a story (`None` for a table block), by binary search over
/// `block_of_para` (strictly increasing).
pub(super) fn para_of_block(s: &Story, bi: usize) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, s.paras.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match block_of_para(s, mid).cmp(&bi) {
            std::cmp::Ordering::Equal => return Some(mid),
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The paragraph a block path names, for a change.
fn para_by_path_mut<'a>(story: &'a mut Story, steps: &[u32]) -> Option<&'a mut Paragraph> {
    let bi = *steps.first()? as usize;
    if steps.len() == 1 {
        let pi = para_of_block(story, bi)?;
        return story.paras.make_mut(pi);
    }
    let ti = table_index_of_block(story, bi)?;
    let (r, c) = (*steps.get(1)? as usize, *steps.get(2)? as usize);
    let a = story.tables.make_mut(ti)?;
    let t = Arc::make_mut(&mut a.table);
    let cell = t.rows.get_mut(r)?.cells.get_mut(c)?;
    para_by_path_mut(&mut cell.content, steps.get(3..)?)
}

/// `Math.random()` as the session's generator gives it.
fn unit_random(s: &mut Session) -> f64 {
    f64::from(s.env.random_u32()) / 4_294_967_296.0
}

impl Session {
    // ── RunRef ↔ the model ─────────────────────────────────────────────────────────────────

    /// The story reference and the block path of paragraph `para` of the edit context (a header or
    /// footer part and a note included); `None` in a textbox context.
    pub(crate) fn para_ref(&self, para: u32) -> Option<(StoryRef, BlockPath)> {
        let p = para as usize;
        match &self.ctx {
            EditContext::Body => Some((StoryRef::Body, BlockPath::root(u32::try_from(block_of_para(&self.doc.body, p)).ok()?))),
            EditContext::Note { footnote, id, .. } => {
                let kind = if *footnote { NoteKind::Footnote } else { NoteKind::Endnote };
                let s = self.story()?;
                Some((StoryRef::Note(kind, NumKey::new(*id)), BlockPath::root(u32::try_from(block_of_para(s, p)).ok()?)))
            }
            EditContext::HeaderFooter { part, .. } => {
                let s = self.story()?;
                Some((StoryRef::HeaderFooter(*part), BlockPath::root(u32::try_from(block_of_para(s, p)).ok()?)))
            }
            EditContext::Cell { anchor, hops, .. } => {
                let body = &self.doc.body;
                let ai = *anchor as usize;
                let a = body.tables.get(ai)?;
                let block = (a.before_para as usize).min(body.paras.len()) + ai;
                let mut path = BlockPath::root(u32::try_from(block).ok()?);
                let mut table = &*a.table;
                for (i, h) in hops.iter().enumerate() {
                    let (r, k) = cell_index(table, h)?;
                    let cell = table.rows.get(r)?.cells.get(k)?;
                    let (r32, k32) = (u32::try_from(r).ok()?, u32::try_from(k).ok()?);
                    if i + 1 == hops.len() {
                        path = path.cell_block(r32, k32, u32::try_from(block_of_para(&cell.content, p)).ok()?);
                        break;
                    }
                    let bi = h.block?;
                    path = path.cell_block(r32, k32, bi);
                    match cell.content.block_at(bi as usize)? {
                        Block::Table(t) => table = t,
                        Block::Paragraph(_) => return None,
                    }
                }
                Some((StoryRef::Body, path))
            }
            EditContext::Textbox { .. } => None,
        }
    }

    /// The story a reference points into, for a change.
    fn story_ref_mut(&mut self, r: &StoryRef) -> Option<&mut Story> {
        match *r {
            StoryRef::Body => Some(&mut self.doc.body),
            StoryRef::HeaderFooter(p) => self.doc.hf.make_mut(p.index()).map(|h| &mut h.story),
            StoryRef::Note(kind, id) => {
                let notes = Arc::make_mut(&mut self.doc.notes);
                let map = if kind == NoteKind::Footnote { &mut notes.footnotes } else { &mut notes.endnotes };
                map.get_mut(&id)
            }
            StoryRef::Context => super::super::context::story_of_mut(&mut self.doc, &self.ctx),
            StoryRef::Textbox(id) => {
                // F7's path walk (`textbox_paths`, `run_at`): the first box with this drawing id in the
                // body, the header/footer parts and the notes, in that order
                let mut found: Option<(usize, drawing::loc::Path)> = None;
                {
                    let mut roots: Vec<&Story> = vec![&self.doc.body];
                    roots.extend(self.doc.hf.iter().map(|h| &h.story));
                    roots.extend(self.doc.notes.footnotes.values());
                    roots.extend(self.doc.notes.endnotes.values());
                    'roots: for (k, s) in roots.into_iter().enumerate() {
                        for path in drawing::loc::textbox_paths(s) {
                            let hit = drawing::loc::run_at(s, &path)
                                .and_then(|r| r.ext())
                                .and_then(|e| e.textbox.as_deref())
                                .is_some_and(|t| t.drawing == id);
                            if hit {
                                found = Some((k, path));
                                break 'roots;
                            }
                        }
                    }
                }
                let (k, path) = found?;
                let nh = self.doc.hf.len();
                let nf = self.doc.notes.footnotes.len();
                let root: &mut Story = if k == 0 {
                    &mut self.doc.body
                } else if k <= nh {
                    &mut self.doc.hf.make_mut(k - 1)?.story
                } else if k <= nh + nf {
                    Arc::make_mut(&mut self.doc.notes).footnotes.values_mut().nth(k - 1 - nh)?
                } else {
                    Arc::make_mut(&mut self.doc.notes).endnotes.values_mut().nth(k - 1 - nh - nf)?
                };
                drawing::loc::run_at_mut(root, &path)?.ext.as_deref_mut()?.textbox.as_deref_mut().map(|t| &mut t.content)
            }
        }
    }

    /// The model run a [`RunRef`] names, for a change (`None` when it names nothing: the layout
    /// reported a run the model does not have; never a panic).
    pub(crate) fn field_run_mut(&mut self, r: &RunRef) -> Option<&mut Run> {
        let story = self.story_ref_mut(&r.story)?;
        let p = para_by_path_mut(story, r.path.steps())?;
        let run = p.runs.get_mut(r.run as usize)?;
        match r.sub {
            RunSub::None => Some(run),
            RunSub::FieldResult(i) => run.ext.as_deref_mut()?.field_result.as_mut()?.get_mut(i as usize),
            RunSub::GroupItem(i) => run.ext.as_deref_mut()?.group.as_deref_mut()?.items.get_mut(i as usize).map(|it| &mut it.run),
        }
    }

    // ── refreshFieldCache / setFieldResult ─────────────────────────────────────────────────

    /// `refreshFieldCache(only)`: the layout calculates each dynamic field's value; the model
    /// runs take it (the cache the file carries). `only`: just these runs. Returns how many
    /// changed. A layout failure leaves the step as it is: the caller rolls it back (F10-D1).
    pub(crate) fn refresh_field_cache(&mut self, cx: &mut ExecCx<'_>, only: Option<HashSet<RunRef>>) -> Result<u32, CommandError> {
        if self.paras().is_none() {
            return Ok(0);
        }
        // `syncRunRPr(this.bodyModel().paragraphs)`
        sync_paras(&mut self.doc.body.paras);
        // wave 7c (S): the TS lays the fields out through `buildEngineDoc`, which rebuilds the bookmark index from the marks,
        // so a bookmark the command just wrote (the `_Toc` names of a TOC built or rebuilt) has its page in this pass: the
        // refresh rewrites the entries' `String(page.number)` labels with the layout's PAGEREF, and `updateFields` counts
        // them as the TS does (without it `updateFields('all','all')` answered 1 where the TS answers 4: the F2 review). A
        // derived index: a failed layout puts the previous one back (the rolled-back step leaves the document as it was).
        let prev = Arc::clone(&self.doc.bookmarks);
        self.sync_bookmarks();
        // and the sections from the body (`syncSections`: the TS builds them at every layout, S-G1), so a command that moved
        // the body's blocks (a TOC inserted at the start) lays its fields out with the sections of now, not the last settle's
        let prev_sections = matches!(self.ctx, super::super::context::EditContext::Body).then(|| {
            let p = (Arc::clone(&self.doc.sections), self.doc.formatting_version);
            self.sections.sync(&mut self.doc);
            if !Arc::ptr_eq(&p.0, &self.doc.sections) {
                self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
                self.tx.formatting = true;
            }
            p
        });
        let scope = match only {
            Some(s) => FieldScope::Runs(s),
            None => FieldScope::All,
        };
        let values = match cx.layout.field_values(self.view(), &scope) {
            Ok(v) => v,
            Err(e) => {
                self.doc.bookmarks = prev;
                if let Some((sections, version)) = prev_sections {
                    self.doc.sections = sections;
                    self.doc.formatting_version = version;
                }
                return Err(CommandError::Layout(e));
            }
        };
        // a Map: the first insertion keeps its place, the last value wins
        let mut order: Vec<(RunRef, Utf16)> = Vec::with_capacity(values.len());
        let mut at: std::collections::HashMap<RunRef, usize> = std::collections::HashMap::new();
        for (r, v) in values {
            match at.get(&r) {
                Some(&i) => {
                    if let Some(slot) = order.get_mut(i) {
                        slot.1 = v;
                    }
                }
                None => {
                    at.insert(r.clone(), order.len());
                    order.push((r, v));
                }
            }
        }
        let mut n = 0u32;
        for (r, v) in &order {
            if self.set_field_result(r, v) {
                n += 1;
            }
        }
        if n > 0 {
            self.touch_all();
        }
        Ok(n)
    }

    /// `setFieldResult(run, v)`: the new result of a field: the text and the runs formatted as
    /// Word does (a REF copies the formatting of the stretch; PAGEREF/SEQ/STYLEREF carry
    /// `w:noProof`; NOTEREF `\f` the note reference's style). Whether it changed.
    pub(crate) fn set_field_result(&mut self, rr: &RunRef, v: &Utf16) -> bool {
        let Some(run) = self.view().run(rr) else { return false };
        let instr: String = run.ext().and_then(|e| e.field.as_deref()).unwrap_or("").to_owned();
        let name = rx::field_name(&instr);
        let src = run.props.src.is_some();
        let old_result = run.ext().and_then(|e| e.field_result.clone());
        let old_text = run.text.clone();
        let props = Arc::clone(&run.props);
        let link = run.rpr_link.clone();
        if src && old_result.is_none() {
            if old_text == *v {
                return false;
            }
            if let Some(r) = self.field_run_mut(rr) {
                r.text = v.clone();
            }
            return true;
        }
        let before_json = json_or_null(old_result.as_ref().map(|x| x as &dyn JsValue));
        let mut before: Vec<u16> = old_text.units().to_vec();
        before.extend(before_json.encode_utf16());
        let mut base = without_revisions(&props);
        obj::delete(&mut base, "src");
        let plain = |text: &Utf16, props: RunProps| -> Run {
            let mut r = Run { text: text.clone(), props: Arc::new(props), ..Run::default() };
            r.rpr_link = link.clone();
            r
        };
        let result: Vec<Run> = if name == "REF" && !rx::has_pnrw(&instr) {
            let target = ref_field_target(Some(&instr));
            let mut copied: Option<Vec<Run>> = None;
            for s in self.mark_stories() {
                let Some(st) = super::super::revision_cmds::story_at(&self.doc, &s.loc) else { continue };
                if let Some(b) = super::super::bookmarks::find_bookmark(st.paras.iter(), &target)
                    && b.start.para == b.end.para
                    && let Some(p) = st.paras.get(b.start.para as usize)
                {
                    copied = Some(runs_between(p, b.start.offset as usize, b.end.offset as usize));
                    break;
                }
            }
            match copied {
                Some(c) if !c.is_empty() && c.iter().flat_map(|r| r.text.units().iter().copied()).eq(v.units().iter().copied()) => c,
                _ => vec![plain(v, base.clone())],
            }
        } else if name == "PAGEREF" || name == "SEQ" || name == "STYLEREF" {
            let mut p = base.clone();
            obj::set(&mut p, "noProof", |x| x.no_proof = Some(true));
            vec![plain(v, p)]
        } else if name == "NOTEREF" && rx::has_f_word(&instr) {
            let mut p = base.clone();
            if let Some(s) = self.note_ref_style_id(&instr).filter(|s| !s.is_empty()) {
                obj::set(&mut p, "charStyle", |x| x.char_style = Some(s.clone()));
            }
            vec![plain(v, p)]
        } else {
            vec![plain(v, base.clone())]
        };
        // wave 7c (W, item 4): Word saves its error result of a REF/PAGEREF/NOTEREF to an undefined bookmark in bold
        // (measured: `<w:b/><w:bCs/>` on the result run after Fields.Update + SaveAs2; the layout gives the text, uo-docx-layout
        // `field_refs::ref_error`/`bookmark_error`); the frozen TS never produces these texts, so its replays are unchanged
        let result: Vec<Run> = if matches!(name.as_str(), "REF" | "PAGEREF" | "NOTEREF") && is_word_ref_error(v) {
            result
                .into_iter()
                .map(|mut r| {
                    let mut p = (*r.props).clone();
                    obj::set(&mut p, "bold", |x| x.bold = Some(true));
                    r.props = Arc::new(p);
                    r
                })
                .collect()
        } else {
            result
        };
        let has_src = props.src.is_some();
        let had_result = old_result.is_some();
        let Some(r) = self.field_run_mut(rr) else { return false };
        r.text = v.clone();
        if !has_src || had_result {
            obj::set(r.ext_mut(), "fieldResult", |e| e.field_result = Some(result));
        }
        let after_json = json_or_null(r.ext().and_then(|e| e.field_result.as_ref()).map(|x| x as &dyn JsValue));
        let mut after: Vec<u16> = r.text.units().to_vec();
        after.extend(after_json.encode_utf16());
        before != after
    }

    /// `noteRefStyleId(instr)`: the character style of the note reference of the bookmark.
    fn note_ref_style_id(&self, instr: &str) -> Option<Str> {
        let target = ref_field_target(Some(instr));
        for s in self.mark_stories() {
            let Some(st) = super::super::revision_cmds::story_at(&self.doc, &s.loc) else { continue };
            let Some(b) = super::super::bookmarks::find_bookmark(st.paras.iter(), &target) else { continue };
            let mut acc = 0usize;
            for r in st.paras.get(b.start.para as usize).map(|p| p.runs.as_slice()).unwrap_or(&[]) {
                if acc >= b.start.offset as usize && r.ext().is_some_and(|e| e.note_ref.is_some()) {
                    return r.props.char_style.clone().or_else(|| r.rpr.as_ref().and_then(|x| x.r_style.clone()));
                }
                acc += r.text.len();
            }
        }
        None
    }

    /// `fieldRunsInSelection()`: the fields of the edit context that cross the selection (the
    /// caret next to one counts), with the field runs inside their results.
    pub(crate) fn field_runs_in_selection(&self) -> HashSet<RunRef> {
        let mut out = HashSet::new();
        let Some(paras) = self.paras() else { return out };
        let DocRange { start, end } = normalize_range(self.range());
        for pi in start.para..=end.para {
            let Some(p) = paras.get(pi as usize) else { continue };
            let Some((story, path)) = self.para_ref(pi) else { continue };
            let mut acc = 0usize;
            for (ri, r) in p.runs.iter().enumerate() {
                let rs = acc;
                acc += r.text.len();
                let a = if pi == start.para { start.offset as usize } else { 0 };
                let z = if pi == end.para { end.offset as usize } else { usize::MAX };
                let Some(e) = r.ext() else { continue };
                if e.field.as_deref().is_none_or(str::is_empty) || rs > z || acc < a {
                    continue;
                }
                let at = u32::try_from(ri).unwrap_or(u32::MAX);
                out.insert(RunRef { story, path: path.clone(), run: at, sub: RunSub::None });
                for (j, x) in e.field_result.iter().flatten().enumerate() {
                    if x.ext().is_some_and(|e| e.field.as_deref().is_some_and(|f| !f.is_empty())) {
                        out.insert(RunRef {
                            story,
                            path: path.clone(),
                            run: at,
                            sub: RunSub::FieldResult(u32::try_from(j).unwrap_or(u32::MAX)),
                        });
                    }
                }
            }
        }
        out
    }

    /// `formulaWork(only)`: the table formulas (`= …`) whose cached text is out of date, of every
    /// body table or just the `only` runs.
    pub(crate) fn formula_work(&self, only: Option<&HashSet<RunRef>>) -> Vec<(RunRef, Utf16)> {
        let mut out = Vec::new();
        let body = &self.doc.body;
        for (ai, a) in body.tables.iter().enumerate() {
            let block = (a.before_para as usize).min(body.paras.len()) + ai;
            let Ok(block) = u32::try_from(block) else { continue };
            visit_formulas(&a.table, &BlockPath::root(block), only, &mut out, 0, !self.options().ts_formula_errors);
        }
        out
    }

    // ── updateFields ───────────────────────────────────────────────────────────────────────

    /// `updateFields(scope, toc)`.
    pub(crate) fn update_fields(&mut self, cx: &mut ExecCx<'_>, scope: FieldScopeSel, toc: TocMode) -> Result<Outcome, CommandError> {
        self.f10.result = Some(F10Result::Count(0));
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let sel = (scope == FieldScopeSel::Selection).then(|| self.field_runs_in_selection());
        let work = self.formula_work(sel.as_ref());
        let tocs: Vec<TocField> =
            self.toc_fields().into_iter().filter(|t| scope == FieldScopeSel::All || self.toc_in_selection(t)).collect();
        if toc == TocMode::All && !tocs.is_empty() {
            let opts: Vec<_> = tocs.iter().map(|t| (super::pure::parse_toc_instr(&t.instr), Some((t.begin, t.last())))).collect();
            self.prepare_pages(cx, &opts, super::toc_cmds::AllParas::No)?;
        }
        // a field update is not an edit (no autosave): a quiet step
        self.set_quiet(true);
        self.begin_step(false, &[]);
        self.set_quiet(false);
        let mut n = u32::try_from(work.len()).unwrap_or(u32::MAX);
        for (rr, text) in &work {
            if let Some(r) = self.field_run_mut(rr) {
                r.text = text.clone();
                if let Some(e) = r.ext.as_deref_mut() {
                    obj::delete(e, "fieldResult");
                }
            }
        }
        if toc == TocMode::All {
            for t in tocs.iter().rev() {
                match self.rebuild_toc(t, None) {
                    Ok(true) => n += 1,
                    Ok(false) => {}
                    Err(e) => {
                        self.f10.pages.clear();
                        self.rollback_step();
                        return Err(e);
                    }
                }
            }
        }
        let mut only =
            (scope == FieldScopeSel::Selection && (tocs.is_empty() || toc == TocMode::Pages)).then(|| self.field_runs_in_selection());
        if toc == TocMode::Pages
            && let Some(o) = only.as_mut()
        {
            for t in &tocs {
                for r in self.toc_page_runs(t) {
                    o.insert(r);
                }
            }
        }
        let refreshed = self.refresh_field_cache(cx, only);
        self.f10.pages.clear();
        match refreshed {
            Ok(k) => n += k,
            Err(e) => {
                self.rollback_step();
                return Err(e);
            }
        }
        if n == 0 {
            self.discard_step();
            return self.done_nothing();
        }
        // the text of the result changed size: the caret stays inside its paragraph
        self.fit_selection();
        self.touch_all();
        self.f10.result = Some(F10Result::Count(n));
        self.done_step()
    }

    /// The selection clamped to the paragraphs of the context (`fit` of `updateFields`).
    pub(crate) fn fit_selection(&mut self) {
        let Some(paras) = self.paras() else { return };
        let n = paras.len();
        if n == 0 {
            return;
        }
        let fit = |q: DocPosition| -> DocPosition {
            let pi = (q.para as usize).min(n - 1);
            let len = paras.get(pi).map_or(0, para_length);
            DocPosition::new(u32::try_from(pi).unwrap_or(u32::MAX), q.offset.min(u32::try_from(len).unwrap_or(u32::MAX)))
        };
        let s = self.selection;
        self.selection = Selection { anchor: fit(s.anchor), focus: fit(s.focus) };
    }

    // ── insertField, insertFieldRun ────────────────────────────────────────────────────────

    /// `insertField(kind)`.
    pub(crate) fn insert_field(&mut self, kind: FieldKind) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let (instr, display) = match kind {
            FieldKind::Page => (" PAGE ".to_owned(), "PAGE".to_owned()),
            FieldKind::NumPages => (" NUMPAGES ".to_owned(), "NUMPAGES".to_owned()),
            FieldKind::Date => (" DATE \\@ \"dd/MM/yyyy\" ".to_owned(), self.env.now().local.pt_br_date()),
        };
        let d: Vec<u16> = display.encode_utf16().collect();
        self.insert_field_run(&instr, &d, None)?;
        self.done_step()
    }

    /// `insertFieldRun(instr, display, extraProps)`: a field run at the caret (it replaces the
    /// selection), a run of its own (it does not merge with its neighbours). Takes the step.
    pub(crate) fn insert_field_run(&mut self, instr: &str, display: &[u16], extra: Option<&RunProps>) -> Result<bool, CommandError> {
        if self.paras().is_none() {
            return Ok(false);
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            match self.delete_selection() {
                Ok(pos) => self.selection = Selection::caret(pos),
                Err(e) => {
                    self.discard_step();
                    return Err(e);
                }
            }
        }
        let DocPosition { para, offset } = self.selection.focus;
        let off = offset as usize;
        let shown = Utf16::from_units(display.to_vec());
        let Some(p0) = self.paras().and_then(|ps| ps.get(para as usize)) else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        // the run the field goes in/after (`run?.props`) and the pieces around it
        let mut acc = 0usize;
        let mut hit: Option<(usize, usize, Option<Run>)> = None;
        let n = p0.runs.len();
        for ri in 0..=n {
            let run = p0.runs.get(ri);
            let re = acc + run.map_or(0, |r| r.text.len());
            if off <= re || ri == n {
                hit = Some((ri, acc, run.cloned()));
                break;
            }
            acc = re;
        }
        let Some((ri, racc, run)) = hit else {
            self.discard_step();
            return Ok(false);
        };
        // `newRunProps(run?.props)` then `...extraProps`
        let base_props: RunProps = run.as_ref().map_or_else(default_run_props, |r| (*r.props).clone());
        let mut props = self.new_run_props_from(&base_props);
        if let Some(x) = extra {
            props = obj::spread(&props, x);
        }
        let mut field_run = Run { text: shown.clone(), props: Arc::new(props), ..Run::default() };
        field_run.rpr_link = run.as_ref().and_then(|r| r.rpr_link.clone());
        obj::set(field_run.ext_mut(), "field", |e| e.field = Some(instr.into()));
        let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(para as usize)) else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        match run {
            None => p.runs.push(field_run),
            Some(r) => {
                let cut = off.saturating_sub(racc).min(r.text.len());
                let (head, tail) = r.text.units().split_at(cut);
                let piece = |units: &[u16]| {
                    let mut x = Run { text: Utf16::from_units(units.to_vec()), props: Arc::new((*r.props).clone()), ..Run::default() };
                    x.rpr_link = r.rpr_link.clone();
                    if let Some(f) = r.ext().and_then(|e| e.field.clone()).filter(|f| !f.is_empty()) {
                        obj::set(x.ext_mut(), "field", |e| e.field = Some(f));
                    }
                    x
                };
                let mut parts: Vec<Run> = Vec::new();
                if !head.is_empty() {
                    parts.push(piece(head));
                }
                parts.push(field_run);
                if !tail.is_empty() {
                    parts.push(piece(tail));
                }
                p.runs.splice(ri..=ri, parts);
            }
        }
        let np = DocPosition::new(para, u32::try_from(off + shown.len()).unwrap_or(u32::MAX));
        self.selection = Selection::caret(np);
        self.touch_para(para as usize);
        Ok(true)
    }

    // ── date, time, document info, alignment tab ───────────────────────────────────────────

    /// `dateTimeFormats(lang)`: the patterns of the Date and Time dialog in the language (default:
    /// the session's) with the sample for now.
    #[must_use]
    pub fn date_time_formats(&self, lang: Option<super::UiLang>) -> Vec<(&'static str, Utf16)> {
        let l = lang.unwrap_or_else(|| self.ui_lang());
        super::datetime::date_time_samples(l, &self.env.now()).into_iter().map(|(p, s)| (p, Utf16::from_units(s))).collect()
    }

    /// `insertDateTime(pattern, update, lang)`.
    pub(crate) fn insert_date_time(&mut self, pattern: &[u16], update: bool, lang: Option<super::UiLang>) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let l = lang.unwrap_or_else(|| self.ui_lang());
        let now = self.env.now();
        let text = format_now(&now, pattern, l);
        if update {
            let mut instr: Vec<u16> = " TIME \\@ \"".encode_utf16().collect();
            instr.extend_from_slice(pattern);
            instr.extend("\" ".encode_utf16());
            // the instruction is a model string: a lone surrogate of the pattern becomes U+FFFD
            let instr = String::from_utf16_lossy(&instr);
            self.insert_field_run(&instr, &text, None)?;
            self.done_step()
        } else {
            self.paste_text(&text)
        }
    }

    /// `pasteText(text)` for `insertDateTime`: F8's `paste_text_only` (the TS `pasteTextOnly`: the text
    /// with the destination's formatting), then the step's `finish`.
    fn paste_text(&mut self, text: &[u16]) -> Result<Outcome, CommandError> {
        match self.paste_text_only(text) {
            Ok(None) => self.done_nothing(),
            Ok(Some(_)) => self.done_step(),
            Err(e) => {
                self.discard_step();
                Err(e)
            }
        }
    }

    /// `insertDocumentInfo(kind, fileName)`.
    pub(crate) fn insert_document_info(&mut self, kind: DocInfoKind, file_name: &str) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let core = self.pkg.part_text("docProps/core.xml").ok().flatten().unwrap_or_default();
        let (instr, shown): (&str, String) = match kind {
            DocInfoKind::Author => (" AUTHOR  \\* MERGEFORMAT ", rx::core_prop(&core, "dc:creator")),
            DocInfoKind::Title => (" TITLE  \\* MERGEFORMAT ", rx::core_prop(&core, "dc:title")),
            DocInfoKind::FileName => (" FILENAME  \\* MERGEFORMAT ", file_name.to_owned()),
            DocInfoKind::FilePath => (" FILENAME \\p  \\* MERGEFORMAT ", file_name.to_owned()),
        };
        let shown = if shown.is_empty() {
            match self.ui_lang() {
                super::UiLang::Pt => "[sem valor]",
                super::UiLang::En => "[no value]",
                super::UiLang::Es => "[sin valor]",
            }
            .to_owned()
        } else {
            shown
        };
        let d: Vec<u16> = shown.encode_utf16().collect();
        self.insert_field_run(instr, &d, None)?;
        self.done_step()
    }

    /// `insertAlignmentTab(alignment, relativeTo, leader)`: a `w:ptab` run at the caret.
    pub(crate) fn insert_alignment_tab(&mut self, alignment: &str, relative_to: &str, leader: &str) -> Result<Outcome, CommandError> {
        // the TS types are unions; a value outside them (a JS caller's) would be written as an
        // attribute of `w:ptab`: refused (F10-D19)
        let valid = matches!(alignment, "left" | "center" | "right")
            && matches!(relative_to, "margin" | "indent")
            && matches!(leader, "none" | "dot" | "hyphen" | "underscore" | "middleDot");
        if !valid || self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            match self.delete_selection() {
                Ok(pos) => self.selection = Selection::caret(pos),
                Err(e) => {
                    self.discard_step();
                    return Err(e);
                }
            }
        }
        let carried: CarriedProps = self.pending_props.clone().unwrap_or_else(|| self.current_props());
        let props = self.new_run_props_from(&carried.props);
        let mut run = Run { text: Utf16::from("\t"), props: Arc::new(props), ..Run::default() };
        run.rpr_link = carried.link.clone();
        obj::set(run.ext_mut(), "ptab", |e| {
            e.ptab =
                Some(Ptab { alignment: alignment.into(), relative_to: relative_to.into(), leader: Some(leader.into()), ..Ptab::default() });
        });
        let focus = self.selection.focus;
        let Some(paras) = self.paras_mut() else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        match text::insert_run(paras, focus, run) {
            Ok(pos) => {
                self.touch_para(focus.para as usize);
                self.selection = Selection::caret(pos);
                self.done_step()
            }
            Err(e) => {
                self.discard_step();
                Err(e.into())
            }
        }
    }

    /// A `Math.random()` as the session's generator gives it.
    pub(crate) fn random_unit(&mut self) -> f64 {
        unit_random(self)
    }
}

/// `formulaWork`'s `visit(t)`.
fn visit_formulas(
    t: &crate::model::table::Table,
    path: &BlockPath,
    only: Option<&HashSet<RunRef>>,
    out: &mut Vec<(RunRef, Utf16)>,
    depth: usize,
    word: bool,
) {
    use super::super::tables::formula::{FormulaEnv, formula_result_text};
    if depth > MAX_DEPTH {
        return;
    }
    let m = map_table(t);
    for row in &m.rows {
        for g in row {
            let Some(cell) = t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci)) else { continue };
            let (Ok(r32), Ok(c32)) = (u32::try_from(g.ri), u32::try_from(g.ci)) else { continue };
            for (bi, b) in cell.content.blocks().enumerate() {
                let Ok(bi32) = u32::try_from(bi) else { continue };
                let bpath = path.cell_block(r32, c32, bi32);
                match b {
                    Block::Table(nt) => visit_formulas(nt, &bpath, only, out, depth + 1, word),
                    Block::Paragraph(p) => {
                        for (rj, run) in p.runs.iter().enumerate() {
                            let Some(field) = run.ext().and_then(|e| e.field.as_deref()) else { continue };
                            if field.is_empty() || !rx::starts_with_equals(field) {
                                continue;
                            }
                            let rr = RunRef {
                                story: StoryRef::Body,
                                path: bpath.clone(),
                                run: u32::try_from(rj).unwrap_or(u32::MAX),
                                sub: RunSub::None,
                            };
                            if only.is_some_and(|o| !o.contains(&rr)) {
                                continue;
                            }
                            let units: Vec<u16> = field.encode_utf16().collect();
                            let env = FormulaEnv { table: t, r: i64::try_from(g.ri).unwrap_or(0), c: g.col, bookmarks: None, word };
                            let Some(text) = formula_result_text(&units, &env) else { continue };
                            let new = Utf16::from(text.as_str());
                            if new != run.text {
                                out.push((rr, new));
                            }
                        }
                    }
                }
            }
        }
    }
}
