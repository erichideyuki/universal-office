//! The editing session (D2a, record `core/docs/modules/uo-docx.md` "D2a — session
//! (architecture)"): one [`Session`] per open document owns the package, the model, the edit
//! context, the selection, the undo history and the per-document state the TS keeps on the
//! `<universal-write>` element; commands come in as values ([`Command`]) and go out as a
//! [`ChangeSet`] with the model-derived events in TS order.
//!
//! The protocol every command family follows (D2b frontiers included):
//!
//! 1. `self.locked()` refuses an edit in protected mode / read view ([`CommandError::Locked`]);
//! 2. [`Session::begin_step`] takes the undo step **before** the change (`snapshot()`), with
//!    `deep` and the header/footer parts the step changes;
//! 3. the change goes through the story of the edit context ([`Session::paras_mut`],
//!    [`Session::story_mut`]), each op keeping or renewing identities by the rule of its TS
//!    construction site (G22), and records what it touched ([`Session::touch_para`],
//!    [`Session::touch_structure`]);
//! 4. a step that changed nothing is dropped with [`Session::discard_step`] (`discardSnapshot`);
//! 5. [`Session::finish`] runs the settle step (G21a), prices the history (U19) and collects the
//!    version stores (U12), and builds the [`ChangeSet`].
//!
//! Commands that need the shell (browser codecs, the system clipboard: U9, G25) return
//! [`Outcome::NeedsJobs`] and finish in [`Session::complete`].

pub mod clipboard;
pub mod command;
pub mod env;
pub mod font_usage;
pub mod guard;
pub mod history;
mod io;
pub mod normalize;
pub mod ops;
pub mod policy;
pub mod revisions;
mod save_parts;
pub mod settle;
pub mod state;
pub mod text;
pub mod versions;

use std::collections::HashSet;
use std::sync::Arc;

use uo_opc::Package;

use crate::ids::{HfPartId, ParaId};
use crate::js::{NumKey, Str};
use crate::model::Document;
use crate::model::para::Paragraph;
use crate::model::story::Story;
use crate::pvec::PVec;
use crate::view::{ContextView, DocView, LayoutOracle};
use crate::write::ctx::SaveState;

pub use clipboard::{Clip, ClipboardRing};
pub use command::{ChangeSet, Command, CommandError, Event, Job, JobId, JobResult, Outcome, StoryChange, StoryKey};
pub use env::{DateParts, Env, FixedEnv, LocalNow};
pub use history::{History, HistoryLimits};
pub use io::{OpenError, OpenOptions};
pub use ops::context::EditContext;
pub use policy::FormattingPolicy;
pub use state::{CellHop, ContextStep, EditState, Selection, SettingsStep, TableSel, VersionId};
pub use text::{DocPosition, DocRange};

use history::DirtyChange;
use settle::SectionState;
use text::CarriedProps;
use versions::VersionStore;

/// What a command needs from outside the session for the call (A4, G15): the layout (the
/// layout-dependent family, G21b) and the clipboard ring of the wasm instance (G7).
pub struct ExecCx<'a> {
    pub layout: &'a mut dyn LayoutOracle,
    pub clipboard: &'a mut ClipboardRing,
}

/// The markup view of the revisions (`markupMode`): which runs the caret skips as hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkupMode {
    /// "All Markup" (the default): nothing hidden
    #[default]
    All,
    /// "Simple Markup": the final text (deleted text hidden)
    Simple,
    /// "No Markup": the final text
    None,
    /// "Original": inserted text hidden
    Original,
}

/// The document options the session keeps (`tracking`, `author`, `protectedMode`, the read view,
/// the markup mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    pub author: Str,
    pub tracking: bool,
    pub protected: bool,
    pub read_view: bool,
    pub markup: MarkupMode,
    /// "Show Markup" (`markup`: `MarkupOptions`, D2b-F6)
    pub markup_options: ops::revision_cmds::MarkupOptions,
    /// WordCraft study (fields, B25): the frozen TS's table-formula error texts and ROUND (`OpenOptions::ts_formula_errors`)
    pub ts_formula_errors: bool,
    /// WordCraft study (fields, B26/B27): the frozen TS's list words and isLgl in the outline, the TOC sources and the
    /// cross-reference items (`OpenOptions::ts_numbering`)
    pub ts_numbering: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        // the TS default without an `author` attribute or property: "Author" in the UI language
        // (the shell passes the name; English is the UI default)
        SessionOptions {
            author: "Author".into(),
            tracking: false,
            protected: false,
            read_view: false,
            markup: MarkupMode::All,
            markup_options: ops::revision_cmds::MarkupOptions::default(),
            ts_formula_errors: false,
            ts_numbering: false,
        }
    }
}

/// What one `exec`/`undo`/`redo` accumulated for its [`ChangeSet`].
#[derive(Debug, Default)]
struct Tx {
    events: Vec<Event>,
    changed: Vec<ParaId>,
    structure: bool,
    all: bool,
    formatting: bool,
    /// the body's paragraph list or a section break changed: `syncSections` runs at the settle
    sections: bool,
}

/// A two-phase command waiting for its shell jobs (G25).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub jobs: Vec<JobId>,
    pub command: Command,
}

/// One open document being edited (see the module comment).
pub struct Session {
    pkg: Package,
    doc: Document,
    ctx: EditContext,
    selection: Selection,
    table_sel: Option<TableSel>,
    /// the selected drawing object (its order among the context's drawing runs, `floatSel`)
    float_sel: Option<u32>,
    /// bookmark → body paragraph (`bookmarkParas`), shifted by Enter/Backspace
    bookmark_paras: state::BookmarkParas,
    styles: VersionStore,
    numbering: VersionStore,
    /// the lists/styles family's own state (`numTouched`, `lastBullet`…; D2b-F2)
    f2: ops::lists::F2State,
    /// the find and navigation family's own state (extra ranges, the last search; D2b-F9)
    f9: ops::navigation::F9State,
    /// the layout family's own state (the last answer, the reference seed, the caption settings, the manual hyphenation; D2b-F10)
    f10: ops::references::F10State,
    /// the notes family's own state (the loaded note settings, the last conversion; D2b-F5)
    f5: ops::notes::F5State,
    /// the sections / header-footer / page family's own state (D2b-F4)
    f4: ops::sections::F4State,
    /// the drawing objects / pictures family's own state (the inline selection, the multi-selection, the pending job results; D2b-F7)
    f7: ops::drawing::F7State,
    /// the clipboard family's own state (the last paste, the Paste Special data, the paste defaults; D2b-F8)
    f8: ops::clipboard::F8State,
    settings: Arc<SettingsStep>,
    sections: SectionState,
    save: SaveState,
    history: History<EditState>,
    env: Box<dyn Env>,
    opts: SessionOptions,
    /// the revision authors in colour order (`revAuthors`)
    rev_authors: Vec<Str>,
    /// the next free `w:id` of a bookmark (`nextBmId`, -1 = not computed yet; D2b-F6)
    next_bm_id: f64,
    /// the props the next typed text takes (`pendingProps`, the format family)
    pending_props: Option<CarriedProps>,
    pending: Option<Pending>,
    next_job: u32,
    /// header/footer parts edited since the install (`dirtyHf`, by part)
    dirty_hf: HashSet<HfPartId>,
    /// wave 7 (B): the textbox context whose box carries a flat editing view (`ops/hf_flat.rs`)
    flat_box: Option<ops::context::EditContext>,
    policy: Option<(FormattingPolicy, policy::ResolvedPolicy)>,
    /// the clipboard identity of this session (U3, G7: session id + generation)
    session_id: u32,
    generation: u32,
    quiet: bool,
    tx: Tx,
    /// `loadedFormat === 'odt'` (universal-write.ts 2129; wave 7, A1): set by [`Session::open_odt`]; the style sheet is
    /// not editable (`stylesEditable()` 4582)
    loaded_odt: bool,
    /// wave 7c (S): `this.pkg === null` of the TS for an `.odt` (`loadOdt` 2240): every save writes into a fresh
    /// empty package (`this.pkg ?? DocxPackage.empty()`, 8552) until the watermark or the cover page makes one (11877,
    /// 12041); the paths the TS guards with `this.pkg ?` / `if (this.pkg)` treat it as no package
    pkg_scratch: bool,
    /// wave 7c (S): `pendingTableStyles` (19268): the built-in table styles a table took where the style sheet is not
    /// editable (an `.odt`, a `.docx` without a styles part), written by the save (`addStylesXml`, 8641). A step of the
    /// history holds it (Word manda sobre o TS: an undone table takes its style out of the file;
    /// `OpenOptions::ts_table_style_survives_undo` keeps the TS map outside the history)
    pending_tables: Arc<indexmap::IndexMap<String, String>>,
    /// wave 7c (S): what the saves wrote outside the writer (`save_parts.rs`)
    save_parts: save_parts::SaveParts,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("ctx", &self.ctx)
            .field("selection", &self.selection)
            .field("paragraphs", &self.doc.body.paras.len())
            .field("undo", &self.history.undo_len())
            .field("redo", &self.history.redo_len())
            .field("dirty", &self.history.is_dirty())
            .finish_non_exhaustive()
    }
}

impl policy::PolicyGuardHost for Session {
    /// `styleKeyOf(id)`: the key of the style's name (`w:name`), or of the id when the document
    /// has no such style.
    fn style_key_of(&self, id: &str) -> String {
        let name = self.doc.formatting.styles.styles.get(id).and_then(|s| s.name.as_deref());
        policy::style_key(name.unwrap_or(id))
    }
}

impl Session {
    // ── reading the state ──────────────────────────────────────────────────────────────────

    /// The document as the layout and the save see it.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// What uo-docx-layout reads (A4): the document and, in a context, the edited story.
    #[must_use]
    pub fn view(&self) -> DocView<'_> {
        match &self.ctx {
            EditContext::Body => DocView::new(&self.doc),
            _ => match self.story() {
                Some(story) => DocView::with_context(&self.doc, ContextView { story }),
                None => DocView::new(&self.doc),
            },
        }
    }

    #[must_use]
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    #[must_use]
    pub fn context(&self) -> &EditContext {
        &self.ctx
    }

    #[must_use]
    pub fn selection(&self) -> Selection {
        self.selection
    }

    #[must_use]
    pub fn table_selection(&self) -> Option<TableSel> {
        self.table_sel.clone()
    }

    #[must_use]
    pub fn options(&self) -> &SessionOptions {
        &self.opts
    }

    /// WordCraft study (fields, B26/B27): the list rules of the session's outline, TOC sources and cross-reference items
    /// (Word's unless `OpenOptions::ts_numbering`).
    #[must_use]
    pub fn num_rules(&self) -> crate::word::NumRules {
        crate::word::NumRules { word_lang: !self.opts.ts_numbering, word_lgl: !self.opts.ts_numbering }
    }

    #[must_use]
    pub fn bookmark_paras(&self) -> &state::BookmarkParas {
        &self.bookmark_paras
    }

    /// The section state (`sectionProps`, `nextSectionId`, the body `sectPr` and references).
    #[must_use]
    pub fn section_state(&self) -> &SectionState {
        &self.sections
    }

    #[must_use]
    pub fn styles_store(&self) -> &VersionStore {
        &self.styles
    }

    #[must_use]
    pub fn numbering_store(&self) -> &VersionStore {
        &self.numbering
    }

    #[must_use]
    pub fn settings(&self) -> &SettingsStep {
        &self.settings
    }

    #[must_use]
    pub fn revision_authors(&self) -> &[Str] {
        &self.rev_authors
    }

    #[must_use]
    pub fn policy(&self) -> Option<&FormattingPolicy> {
        self.policy.as_ref().map(|p| &p.0)
    }

    /// The resolved policy (`ResolvedPolicy`) the paste family sanitizes with.
    #[must_use]
    pub fn resolved_policy(&self) -> Option<&policy::ResolvedPolicy> {
        self.policy.as_ref().map(|p| &p.1)
    }

    #[must_use]
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// `isDirty()`.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// `editsSinceLoad` (the PDF comes from the original bytes while it is 0).
    #[must_use]
    pub fn edits_since_load(&self) -> u32 {
        self.history.edits_since_load()
    }

    /// The bytes the history counts against its cap (U12, U19), the version stores included.
    #[must_use]
    pub fn history_bytes(&self) -> usize {
        self.history.bytes()
    }

    #[must_use]
    pub fn history(&self) -> &History<EditState> {
        &self.history
    }

    /// The clipboard identity of this session (`RichClip` document identity, U3).
    #[must_use]
    pub fn clip_identity(&self) -> (u32, u32) {
        (self.session_id, self.generation)
    }

    /// The story of the edit context (`this.model.paragraphs` and its tables).
    #[must_use]
    pub fn story(&self) -> Option<&Story> {
        ops::context::story_of(&self.doc, &self.ctx)
    }

    /// The paragraphs of the edit context.
    #[must_use]
    pub fn paras(&self) -> Option<&PVec<Paragraph>> {
        self.story().map(|s| &s.paras)
    }

    /// `setDirty(dirty)` (the host marks the document saved).
    pub fn set_dirty(&mut self, dirty: bool) -> ChangeSet {
        let c = self.history.set_dirty(dirty);
        self.dirty_event(c);
        self.change_set()
    }

    /// Drops what is only cache (M1 convention): nothing in the session is a cache today; the
    /// version stores are collected with the history at every step.
    pub fn clear_caches(&mut self) {
        let live: Vec<(Option<VersionId>, Option<VersionId>)> = self.history.states().map(|s| (s.styles, s.numbering)).collect();
        self.styles.collect(live.iter().filter_map(|l| l.0));
        self.numbering.collect(live.iter().filter_map(|l| l.1));
    }

    // ── the step protocol (D2a; every family uses it) ──────────────────────────────────────

    /// `locked()`: protected mode or the read view.
    #[must_use]
    pub fn locked(&self) -> bool {
        self.opts.protected || self.opts.read_view
    }

    /// The story of the edit context, for a change (copy-on-write down to it).
    pub fn story_mut(&mut self) -> Option<&mut Story> {
        ops::context::story_of_mut(&mut self.doc, &self.ctx)
    }

    /// The paragraphs of the edit context, for a change.
    pub fn paras_mut(&mut self) -> Option<&mut PVec<Paragraph>> {
        self.story_mut().map(|s| &mut s.paras)
    }

    /// `snapshot(deep, parts)`: the state before the step goes on the undo stack.
    pub fn begin_step(&mut self, deep: bool, parts: &[HfPartId]) {
        // `if (this.hfEdit) this.dirtyHf.add(path)` (a textbox in a header keeps `hfEdit`)
        let part = match &self.ctx {
            EditContext::HeaderFooter { part, .. } => Some(*part),
            EditContext::Textbox { outer, .. } => match **outer {
                EditContext::HeaderFooter { part, .. } => Some(part),
                _ => None,
            },
            _ => None,
        };
        if let Some(part) = part {
            self.dirty_hf.insert(part);
        }
        let before = self.take_state(deep, parts);
        let c = self.history.push(before);
        self.dirty_event(c);
        if !self.quiet {
            self.tx.events.push(Event::Change);
        }
    }

    /// `discardSnapshot()`: the step just taken changed nothing.
    pub fn discard_step(&mut self) {
        let (_, c) = self.history.discard();
        self.dirty_event(c);
    }

    /// `changed()`: the document changed outside a step.
    pub fn changed(&mut self) {
        let c = self.history.changed();
        self.dirty_event(c);
        self.tx.events.push(Event::Change);
    }

    /// Quiet steps (`quietChange`: field updates) take a step without `uw-change`.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// The paragraph at `i` of the context changed in place.
    pub fn touch_para(&mut self, i: usize) {
        if let Some(id) = self.paras().and_then(|p| p.get(i)).map(|p| p.id)
            && !self.tx.changed.contains(&id)
        {
            self.tx.changed.push(id);
        }
    }

    /// Paragraphs were inserted, removed, split or merged (or a table changed) in the context.
    pub fn touch_structure(&mut self) {
        self.tx.structure = true;
        if matches!(self.ctx, EditContext::Body) {
            self.tx.sections = true;
        }
    }

    /// Every story may have changed (undo, deep steps, style changes).
    pub fn touch_all(&mut self) {
        self.tx.all = true;
        self.tx.sections = true;
    }

    /// The style sheet, numbering, settings or sections changed.
    pub fn touch_formatting(&mut self) {
        self.tx.formatting = true;
    }

    /// An event for the shell, in TS order.
    pub fn emit(&mut self, e: Event) {
        self.tx.events.push(e);
    }

    pub fn set_selection_raw(&mut self, s: Selection) {
        self.selection = s;
    }

    pub fn set_pending_props(&mut self, p: Option<CarriedProps>) {
        self.pending_props = p;
    }

    #[must_use]
    pub fn pending_props(&self) -> Option<&CarriedProps> {
        self.pending_props.as_ref()
    }

    /// `newRev()`: the authorship of a revision made now (the author joins the colour order).
    pub fn new_rev(&mut self) -> crate::model::props::RevisionInfo {
        let r = revisions::new_revision(&self.opts.author, &self.env.now());
        if !self.rev_authors.iter().any(|a| **a == *r.author) {
            self.rev_authors.push(r.author.clone());
        }
        r
    }

    /// The environment (clock, random source).
    pub fn env_mut(&mut self) -> &mut dyn Env {
        &mut *self.env
    }

    /// `shiftAnchors(fromPara, delta)`: table anchors and bookmarks after `from` move with the
    /// paragraphs (body and header/footer contexts; not in a cell or a note).
    pub fn shift_anchors(&mut self, from: u32, delta: i64) {
        if matches!(self.ctx, EditContext::Cell { .. }) {
            // the tables nested in the cell keep their order with the paragraphs (the TS
            // `reconcileBlocks` after every edit of the cell's paragraph list: new paragraphs go
            // right after the one they come from, in front of a table that follows it; removed
            // ones leave the tables where they were: D2b-F3, found by its harness)
            let n = delta.unsigned_abs();
            if let Some(story) = self.story_mut() {
                for i in 0..story.tables.len() {
                    let Some(k) = story.tables.get(i).map(|a| a.before_para) else { continue };
                    let new = if delta > 0 {
                        if k > from { u32::try_from(i64::from(k) + delta).unwrap_or(u32::MAX) } else { k }
                    } else if u64::from(k) > u64::from(from) + 1 {
                        let gone = (u64::from(k) - u64::from(from) - 1).min(n);
                        u32::try_from(u64::from(k) - gone).unwrap_or(k)
                    } else {
                        k
                    };
                    if new != k
                        && let Some(a) = story.tables.make_mut(i)
                    {
                        a.before_para = new;
                    }
                }
            }
            return;
        }
        if matches!(self.ctx, EditContext::Note { .. }) {
            return;
        }
        let shift = |v: u32| -> u32 { if v > from { u32::try_from((i64::from(v) + delta).max(0)).unwrap_or(u32::MAX) } else { v } };
        if let Some(story) = self.story_mut() {
            for i in 0..story.tables.len() {
                if story.tables.get(i).is_some_and(|a| a.before_para > from)
                    && let Some(a) = story.tables.make_mut(i)
                {
                    a.before_para = shift(a.before_para);
                }
            }
        }
        if matches!(self.ctx, EditContext::Body) {
            self.bookmark_paras.shift(from, delta);
        }
    }

    // ── commands ───────────────────────────────────────────────────────────────────────────

    /// Runs a command (one per model-changing public method of the TS element).
    pub fn exec(&mut self, cmd: Command, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        if self.pending.is_some() {
            // a two-phase command is waiting for the shell: nothing else changes the model
            return Err(CommandError::Busy);
        }
        self.tx = Tx::default();
        if let Some((_, rp)) = &self.policy
            && let Some((method, arg)) = cmd.guard()
            && !policy::guard_allows(rp, method, arg, self)
        {
            // refused: no effect (the TS guarded method returns); the shell learns it
            self.tx.events.push(Event::Blocked { command: method.into() });
            return Ok(Outcome::Done(self.change_set()));
        }
        let out = match cmd {
            Command::Text(c) => self.exec_text(c, cx),
            Command::Context(c) => self.exec_context(c, cx),
            Command::Settings(c) => self.exec_settings(c, cx),
            Command::Format(c) => self.exec_format(c, cx),
            Command::Paragraph(c) => self.exec_paragraph(c, cx),
            Command::List(c) => self.exec_lists(c, cx),
            Command::Style(c) => self.exec_styles(c, cx),
            Command::Table(c) => self.exec_tables(c, cx),
            Command::Section(c) => self.exec_sections(c, cx),
            Command::HeaderFooter(c) => self.exec_header_footer(c, cx),
            Command::Note(c) => self.exec_notes(c, cx),
            Command::Revision(c) => self.exec_revision_cmds(c, cx),
            Command::Comment(c) => self.exec_comments(c, cx),
            Command::Bookmark(c) => self.exec_bookmarks(c, cx),
            Command::Drawing(c) => self.exec_drawing(c, cx),
            Command::Picture(c) => self.exec_pictures(c, cx),
            Command::Page(c) => self.exec_page(c, cx),
            Command::TextLayout(c) => self.exec_text_layout(c, cx),
            Command::Clipboard(c) => self.exec_clipboard(c, cx),
            Command::Find(c) => self.exec_find(c, cx),
            Command::Navigation(c) => self.exec_navigation(c, cx),
            Command::Reference(c) => self.exec_references(c, cx),
        };
        if out.is_err() {
            // an error after a step was taken: the step goes (nothing changed before the error,
            // every op checks its positions first)
            self.tx = Tx::default();
        }
        // D2b-F8: the undo of a repaste whose paste went pending, was cancelled or failed
        self.with_f8_carried(out)
    }

    /// Ends a command: the settle step, the history pricing and the change set.
    pub fn finish(&mut self) -> ChangeSet {
        self.settle();
        self.price_history();
        self.change_set()
    }

    /// The jobs of a two-phase command: registers the pending command and gives each job an id.
    pub fn defer(&mut self, command: Command, jobs: Vec<Job>) -> Outcome {
        self.pending = Some(Pending { jobs: jobs.iter().map(Job::id).collect(), command });
        Outcome::NeedsJobs(jobs)
    }

    /// A fresh job id.
    pub fn job_id(&mut self) -> JobId {
        let id = JobId(self.next_job);
        self.next_job = self.next_job.wrapping_add(1);
        id
    }

    /// The shell's answer to one job of the pending command (G25): when the last job is in, the
    /// owning family applies the results.
    pub fn complete(&mut self, job: JobId, result: JobResult, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        let Some(p) = self.pending.as_mut() else { return Err(CommandError::UnknownJob) };
        let Some(k) = p.jobs.iter().position(|j| *j == job) else { return Err(CommandError::UnknownJob) };
        p.jobs.remove(k);
        let last = p.jobs.is_empty();
        let command = p.command.clone();
        // D2b-F7: a command of the picture family needs every job's answer, not only the last
        if matches!(command, Command::Picture(_) | Command::Drawing(_)) {
            self.f7.stash(job, result.clone());
        }
        if !last {
            return Ok(Outcome::NeedsJobs(Vec::new()));
        }
        self.pending = None;
        self.tx = Tx::default();
        let out = match command {
            Command::Clipboard(c) => self.complete_clipboard(c, job, result, cx),
            Command::Picture(c) => self.complete_pictures(c, job, result, cx),
            Command::Drawing(c) => self.complete_drawing(c, job, result, cx),
            _ => Err(CommandError::UnknownJob),
        };
        // D2b-F8: the undo of a repaste whose paste went pending (F8-D18)
        self.with_f8_carried(out)
    }

    /// Drops a pending two-phase command (the shell failed or the user cancelled).
    pub fn cancel_pending(&mut self) {
        self.pending = None;
        self.f7.clear_pending();
    }

    // ── undo / redo ────────────────────────────────────────────────────────────────────────

    /// `undo()`.
    pub fn undo(&mut self, cx: &mut ExecCx<'_>) -> ChangeSet {
        self.pop_history(true, cx)
    }

    /// `redo()`.
    pub fn redo(&mut self, cx: &mut ExecCx<'_>) -> ChangeSet {
        self.pop_history(false, cx)
    }

    // ── internals ──────────────────────────────────────────────────────────────────────────

    fn dirty_event(&mut self, c: DirtyChange) {
        if let Some(dirty) = c.dirty {
            self.tx.events.push(Event::DirtyChange { dirty });
        }
    }

    /// Ends a command: the settle step and the history pricing when it took a step or reported
    /// anything (F7: the drawing and picture commands end here), the bare change set otherwise.
    pub(crate) fn finish_if_stepped(&mut self) -> ChangeSet {
        if self.tx.events.is_empty() && self.tx.changed.is_empty() && !self.tx.structure && !self.tx.all && !self.tx.sections {
            self.change_set()
        } else {
            self.finish()
        }
    }

    fn change_set(&mut self) -> ChangeSet {
        // wave 7 (B, HF-T1): the edits of the flat editing views (header/footer part, note, textbox)
        // into their blocks
        self.sync_flat_views();
        self.settle_float_sel();
        let tx = std::mem::take(&mut self.tx);
        let story = if tx.all {
            StoryKey::All
        } else if matches!(self.ctx, EditContext::Body) {
            StoryKey::Body
        } else {
            StoryKey::Context
        };
        let stories = if tx.all || tx.structure || !tx.changed.is_empty() {
            vec![StoryChange { story, changed: tx.changed, structure: tx.structure || tx.all }]
        } else {
            Vec::new()
        };
        ChangeSet {
            stories,
            selection: self.selection,
            events: tx.events,
            dirty: self.history.is_dirty(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            formatting: tx.formatting,
        }
    }

    /// The settle step (G21a): where the TS `refresh()` runs, the layout-independent model
    /// normalisations of its layout pass, by name (`settle.rs`).
    fn settle(&mut self) {
        // `syncSections()` (body context only; its result depends only on the body paragraphs,
        // so it runs when they changed in structure or in a section break: D2a-S2)
        let body = matches!(self.ctx, EditContext::Body);
        if !body && (self.tx.sections || self.tx.all) {
            self.sections.pending = true;
        }
        if body && (self.tx.sections || self.tx.all || self.sections.pending) {
            self.sections.pending = false;
            self.sections.sync(&mut self.doc);
            self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
            self.tx.formatting = true;
        }
        // `syncNoteSettings()` of the layout pass (D2b-F5)
        self.sync_note_settings();
        // `syncRunRPr(this.model.paragraphs)` of the layout pass (the context's paragraphs)
        if let Some(paras) = self.paras_mut() {
            settle::sync_paras(paras);
        }
        // wave 7 (B, gate-10): the bookmark index of the layout pass is rebuilt by the hosts before a layout
        // (`Session::sync_bookmarks`; per command it broke the M3 keystroke budget)
    }

    fn price_history(&mut self) {
        let live = self.take_state(false, &[]);
        self.history.price(&live);
        drop(live);
        let live: Vec<(Option<VersionId>, Option<VersionId>)> = self.history.states().map(|s| (s.styles, s.numbering)).collect();
        if self.styles.len() > live.len() + 2 {
            self.styles.collect(live.iter().filter_map(|l| l.0));
        }
        if self.numbering.len() > live.len() + 2 {
            self.numbering.collect(live.iter().filter_map(|l| l.1));
        }
        self.history.set_extra_bytes(self.styles.bytes() + self.numbering.bytes());
    }

    /// `stringifyState(deep, parts)` as shared roots (A3, U19).
    fn take_state(&self, deep: bool, parts: &[HfPartId]) -> EditState {
        let context = ops::context::context_step(&self.doc, &self.ctx, self.selection);
        let (body_sel, ts) = match &self.ctx {
            EditContext::Body => (self.selection, self.table_sel.clone()),
            _ => (self.ctx_saved_selection(), self.table_sel.clone()),
        };
        let mut hf_parts: Vec<(HfPartId, Story)> = parts
            .iter()
            .filter(|p| !matches!(self.ctx, EditContext::HeaderFooter { part, .. } if part == **p))
            .filter_map(|p| self.doc.hf.get(p.index()).map(|h| (*p, h.story.clone())))
            .collect();
        // a textbox edited inside a header/footer: the TS state is taken from outside the box,
        // in the header (`hp`): the part's content goes with the step
        if let EditContext::Textbox { outer, .. } = &self.ctx
            && let EditContext::HeaderFooter { part, .. } = **outer
            && !hf_parts.iter().any(|(p, _)| *p == part)
            && let Some(h) = self.doc.hf.get(part.index())
        {
            hf_parts.push((part, h.story.clone()));
        }
        let (deep_hf, deep_notes) = if deep {
            let hf: Vec<(HfPartId, Story)> = (0..self.doc.hf.len())
                .filter_map(|i| self.doc.hf.get(i).map(|h| (HfPartId(u32::try_from(i).unwrap_or(u32::MAX)), h.story.clone())))
                .collect();
            let mut notes: Vec<(bool, f64, Story)> = Vec::new();
            for (footnote, map) in [(true, &self.doc.notes.footnotes), (false, &self.doc.notes.endnotes)] {
                for (k, s) in map {
                    let editing =
                        matches!(&self.ctx, EditContext::Note { footnote: f, id, .. } if *f == footnote && NumKey::new(*id) == *k);
                    if !editing {
                        notes.push((footnote, k.get(), s.clone()));
                    }
                }
            }
            (Some(hf.into()), Some(notes.into()))
        } else {
            (None, None)
        };
        EditState {
            body: self.doc.body.clone(),
            selection: body_sel,
            bookmarks: self.bookmark_paras.clone(),
            context,
            table_sel: ts,
            float_sel: if matches!(self.ctx, EditContext::Cell { .. } | EditContext::Note { .. }) { None } else { self.float_sel },
            styles: Some(self.styles.version()),
            numbering: Some(self.numbering.version()),
            pending_tables: Arc::clone(&self.pending_tables),
            settings: Arc::clone(&self.settings),
            body_sect_pr: Arc::clone(&self.sections.body_sect_pr),
            body_refs: self.sections.body_refs.clone(),
            deep_hf,
            deep_notes,
            hf_parts: hf_parts.into(),
        }
    }

    /// The body selection kept while a context is edited (`cellEdit.saved.selection`…).
    fn ctx_saved_selection(&self) -> Selection {
        ops::context::saved_selection(&self.ctx).unwrap_or(self.selection)
    }

    /// `popHistory(from, to)`.
    fn pop_history(&mut self, undo: bool, cx: &mut ExecCx<'_>) -> ChangeSet {
        self.tx = Tx::default();
        let can = if undo { self.history.can_undo() } else { self.history.can_redo() };
        if !can {
            return self.change_set();
        }
        // the step's parts go back too: the current state is taken with them
        let peek_parts: Vec<HfPartId> = {
            let top = if undo { self.history.peek_undo() } else { self.history.peek_redo() };
            top.map(|s| {
                let mut v: Vec<HfPartId> = s.hf_parts.iter().map(|(p, _)| *p).collect();
                if let ContextStep::HeaderFooter { part, .. } = &s.context {
                    v.push(*part);
                }
                v
            })
            .unwrap_or_default()
        };
        let deep = (if undo { self.history.peek_undo() } else { self.history.peek_redo() }).is_some_and(|s| s.deep_hf.is_some());
        // a textbox being edited: the state of now (with the box: `te`) goes to the other stack, then the
        // box is left (`popHistory`: `to.push(stringifyState(...))` before `exitTextbox(true)`)
        let in_box = matches!(self.ctx, EditContext::Textbox { .. }).then(|| self.take_state(deep, &peek_parts));
        ops::context::leave_for_history(self, cx);
        // a step taken in a note brings the caret into it before the state of now is taken (the
        // redo keeps its text: notes-lab fn-undo)
        let note = (if undo { self.history.peek_undo() } else { self.history.peek_redo() }).and_then(|s| match &s.context {
            ContextStep::Note { footnote, id, .. } => Some((*footnote, *id)),
            _ => None,
        });
        if let Some((footnote, id)) = note
            && !matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. })
        {
            let same =
                matches!(self.ctx, EditContext::Note { footnote: f, id: i, .. } if f == footnote && NumKey::new(i) == NumKey::new(id));
            if !same {
                self.enter_note(footnote, id, None, true);
            }
        }
        let current = in_box.unwrap_or_else(|| self.take_state(deep, &peek_parts));
        let popped = if undo { self.history.undo(current) } else { self.history.redo(current) };
        let Some((state, c)) = popped else { return self.change_set() };
        self.dirty_event(c);
        self.restore_state(state);
        // a step taken inside a text box goes back into it (`reenterTextbox`, D2b-F7)
        if let Some((host, focus, page, sel)) = self.f7.reenter.take() {
            self.reenter_textbox(host, focus, page, sel);
        }
        self.touch_all();
        // in header/footer mode the TS leaves the edit before the restore and `refresh()` (so
        // `syncSections` runs with the body in context) before returning to the page (D2b-F4)
        if matches!(self.ctx, EditContext::HeaderFooter { .. }) {
            self.sections.pending = false;
            self.sections.sync(&mut self.doc);
            self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
            self.tx.formatting = true;
        }
        self.finish()
    }

    /// The restore half of `popHistory`: the state of the step, normalised once (U4, A3).
    fn restore_state(&mut self, mut state: EditState) {
        // the JSON round trip of the TS (fresh identities, no cleared keys, no NaN, no symbol keys)
        normalize::normalize(&mut state.body, &mut self.doc.ids);
        state.context.normalize_stories(&mut self.doc.ids);
        let prev = std::mem::take(&mut self.ctx);
        let was_cell = matches!(prev, EditContext::Cell { .. });
        let was_note = match &prev {
            EditContext::Note { footnote, id, .. } => Some((*footnote, *id)),
            _ => None,
        };
        self.float_sel = None;
        // deep steps: every header/footer part and every other note back in place
        if let Some(hf) = &state.deep_hf {
            for (part, story) in hf.iter() {
                let mut story = story.clone();
                normalize::normalize(&mut story, &mut self.doc.ids);
                if let Some(h) = self.doc.hf.make_mut(part.index()) {
                    h.story = story;
                }
                self.dirty_hf.insert(*part);
            }
        }
        // the sections' `sectPr` of the break paragraphs are new objects too
        for i in 0..state.body.paras.len() {
            if state.body.paras.get(i).is_some_and(|p| p.props.section_break.is_some())
                && let Some(p) = state.body.paras.make_mut(i)
                && let Some(sb) = Arc::make_mut(&mut p.props).section_break.as_deref_mut()
            {
                sb.sect_pr = Arc::new((*sb.sect_pr).clone());
            }
        }
        self.doc.body = state.body;
        self.selection = state.selection;
        // a restored selection is a new object: the extra ranges of a multi-selection go (D2b-F9)
        self.f9.drop_extras();
        self.table_sel = state.table_sel;
        self.bookmark_paras = state.bookmarks;
        ops::context::restore_context(self, state.context, &prev);
        if was_cell != matches!(self.ctx, EditContext::Cell { .. }) {
            self.tx.events.push(Event::CellMode { active: matches!(self.ctx, EditContext::Cell { .. }) });
        }
        if let Some(notes) = &state.deep_notes {
            for (footnote, id, story) in notes.iter() {
                let mut story = story.clone();
                normalize::normalize(&mut story, &mut self.doc.ids);
                ops::context::set_note_story(&mut self.doc, *footnote, *id, story);
            }
        }
        if let Some((footnote, id)) = was_note {
            let now = match &self.ctx {
                EditContext::Note { footnote: f, id: i, .. } => Some((*f, *i)),
                _ => None,
            };
            if now != Some((footnote, id)) {
                self.tx.events.push(match now {
                    Some((f, i)) => Event::NoteMode { active: true, footnote: f, id: i },
                    None => Event::NoteMode { active: false, footnote, id },
                });
            }
        }
        if let Some(v) = state.styles
            && self.styles.restore(v)
        {
            // `syncStyleSheet()` (D2b-F2)
            self.sync_style_sheet();
        }
        if let Some(v) = state.numbering
            && self.numbering.restore(v)
        {
            // `syncNumbering()` (D2b-F2)
            self.sync_numbering();
        }
        // wave 7c (S; Word manda sobre o TS): the table styles the step's tables took come and go with it
        if !self.save_parts.ts_table_style_survives_undo && !Arc::ptr_eq(&self.pending_tables, &state.pending_tables) {
            self.swap_pending_tables(state.pending_tables);
        }
        // `dt`, `hy`, `bg`/`bgd`, `eo`, `np`: a value of the step that differs from the current one
        // is applied (D2b-F4: `restore_settings`)
        self.restore_settings(state.settings);
        // `m.sectPr` of the body (the last section's `sectPr`; the others' are in `body`): the JSON
        // copy is a new object, which the sections the layout holds do not point to until
        // `syncSections` runs (not in a cell or a note); a `sectPr` is shared by value, so a new `Arc`
        let mut sp = state.body_sect_pr;
        normalize::normalize(&mut sp, &mut self.doc.ids);
        self.sections.body_sect_pr = Arc::new((*sp).clone());
        // `m.bodyRefs` comes back with the model; the memory keyed by the `sectPr` objects goes (D2b-F4)
        self.sections.body_refs = state.body_refs;
        self.f4.on_restore();
        self.f7.on_restore();
        for (part, story) in state.hf_parts.iter() {
            let mut story = story.clone();
            normalize::normalize(&mut story, &mut self.doc.ids);
            if let Some(h) = self.doc.hf.make_mut(part.index()) {
                h.story = story;
            }
            self.dirty_hf.insert(*part);
        }
        // tables of the body: the cells' rPr links again (`syncTableRPr`)
        settle::sync_story(&mut self.doc.body, true);
        if let Some(fs) = state.float_sel {
            self.float_sel = Some(fs);
        }
        if matches!(prev, EditContext::HeaderFooter { .. }) && !matches!(self.ctx, EditContext::HeaderFooter { .. }) {
            self.tx.events.push(Event::HfMode { active: false });
        }
    }
}
