//! D2b family F10: the layout family (record `core/docs/modules/uo-docx-d2b/F10.md`): fields
//! (`insertField`, `updateFields`, `refreshFieldCache`/`setFieldResult` over the layout oracle),
//! the date/time, document-info and alignment-tab insertions, cross-references, the table of
//! contents and the table of figures, captions, and Manual Hyphenation. The pure rules of
//! `references.ts`, `toc.ts` and `text-layout.ts` are in the submodules [`pure`], [`toc`],
//! [`hyphen`], [`datetime`] and [`rx`] (every regex a hand matcher).
//!
//! The layout enters through the [`crate::view::LayoutOracle`] carried by `ExecCx` and nowhere
//! else (record, "The oracle use"): `field_values` (the TS `onField` of `refreshFieldCache`),
//! `shown_page_numbers` (the TS `pageLabelOf`, asked once at the start of a command that builds
//! TOC sources) and `next_hyphenation` (the TS forced-hyphenation layout).

pub mod datetime;
pub mod hyphen;
pub mod pure;
pub mod rx;
pub mod toc;

mod captions;
mod crossref;
mod fields;
mod manual_hyph;
mod toc_cmds;

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::{ExecCx, Session};
use super::styles::UiLang;
use crate::ids::ParaId;
use crate::js::Str;
use uo_fonts::hyphen::HyphenationRegistry;

pub use captions::{AutoCaption, AutoCaptionCfg, CaptionLabelInfo, CaptionOptions, CaptionPosition, CaptionSettings, CaptionTarget};
pub use crossref::{CrossRefItem, CrossRefOptions};
pub use hyphen::ManualHyphenation;
pub use manual_hyph::{ManualHyphState, ManualHyphWord};
pub use pure::{CaptionFormat, CaptionNumbering, ChapterSeparator, CrossRefKind, CrossRefType, TocOptions, TofFormat};
pub use toc::{TableOfContentsOptions, TocFormat, TocTemplateId};
pub use toc_cmds::{TableOfFiguresOptions, TocPreview};

/// `insertField(kind)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Page,
    NumPages,
    Date,
}

/// `insertDocumentInfo(kind, …)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocInfoKind {
    Author,
    Title,
    FileName,
    FilePath,
}

/// `updateFields(scope, …)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldScopeSel {
    Selection,
    All,
}

/// How to update the tables of contents (`'pages'` or `'all'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocMode {
    Pages,
    All,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceCmd {
    /// `insertField(kind)`
    InsertField { kind: FieldKind },
    /// `insertDateTime(pattern, update, lang?)` (`lang` `None`: the session's)
    InsertDateTime { pattern: Vec<u16>, update: bool, lang: Option<UiLang> },
    /// `insertDocumentInfo(kind, fileName)`
    InsertDocumentInfo { kind: DocInfoKind, file_name: String },
    /// `insertAlignmentTab(alignment, relativeTo, leader)`
    InsertAlignmentTab { alignment: String, relative_to: String, leader: String },
    /// `updateFields(scope, toc)`
    UpdateFields { scope: FieldScopeSel, toc: TocMode },
    /// `insertCrossReference(opts)`
    InsertCrossReference(CrossRefOptions),
    /// `insertTableOfFigures(opts)`
    InsertTableOfFigures(TableOfFiguresOptions),
    /// `updateTableOfFigures()`
    UpdateTableOfFigures,
    /// `updateToc(mode)`
    UpdateToc { mode: TocMode },
    /// `insertTocTemplate(id)`
    InsertTocTemplate { id: TocTemplateId },
    /// `insertTableOfContents(opts, replace?)`
    InsertTableOfContents { opts: TableOfContentsOptions, replace: Option<bool> },
    /// `removeTableOfContents()`
    RemoveTableOfContents,
    /// `setTocLevel(level)`
    SetTocLevel { level: f64 },
    /// `markTocEntry({ text, identifier, level })`
    MarkTocEntry { text: String, identifier: Option<String>, level: Option<f64> },
    /// `captionSettings = v`
    SetCaptionSettings(CaptionSettings),
    /// `addCaptionLabel(name)`
    AddCaptionLabel { name: String },
    /// `deleteCaptionLabel(name)`
    DeleteCaptionLabel { name: String },
    /// `setCaptionNumbering(label, n)`
    SetCaptionNumbering { label: String, numbering: CaptionNumbering },
    /// `setAutoCaption(auto)`
    SetAutoCaption { auto: Option<(String, CaptionPosition)> },
    /// `insertCaption(opts)`
    InsertCaption(CaptionOptions),
    /// `nextManualHyphenation()` (the answer is [`Session::references_result`])
    NextManualHyphenation,
    /// `applyManualHyphenation(letters)`
    ApplyManualHyphenation { letters: f64 },
    /// `skipManualHyphenation()`
    SkipManualHyphenation,
    /// `endManualHyphenation()`
    EndManualHyphenation,
}

/// What the last command of this family answered (the TS return value).
#[derive(Debug, Clone, PartialEq)]
pub enum F10Result {
    /// no value (`void`)
    Unit,
    Bool(bool),
    /// `updateFields`: how many fields changed
    Count(u32),
    /// `nextManualHyphenation`: the proposed word, or `None` when there is no more
    Hyphenation(Option<ManualHyphenation>),
}

/// The state of the layout family (D2b-F10).
#[derive(Default)]
pub struct F10State {
    /// the answer of the last command
    pub(crate) result: Option<F10Result>,
    /// `refSeed.n` (drawn at the first hidden name, F10-D2)
    pub(crate) ref_seed: Option<f64>,
    /// `captionCfg`
    pub(crate) caption: captions::CaptionState,
    /// `manualHyph`
    pub(crate) manual: Option<manual_hyph::ManualHyph>,
    /// the hyphenation patterns the Manual Hyphenation reads (F10-D3)
    pub(crate) hyphenation: Option<Arc<HyphenationRegistry>>,
    /// the page labels of the layout the shell last showed, by paragraph (`pageLabelOf`), asked
    /// once per command that builds TOC sources
    pub(crate) pages: HashMap<ParaId, Str>,
}

impl std::fmt::Debug for F10State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("F10State").field("result", &self.result).field("ref_seed", &self.ref_seed).finish_non_exhaustive()
    }
}

impl Clone for F10State {
    fn clone(&self) -> Self {
        F10State {
            result: self.result.clone(),
            ref_seed: self.ref_seed,
            caption: self.caption.clone(),
            manual: self.manual.clone(),
            hyphenation: self.hyphenation.clone(),
            pages: self.pages.clone(),
        }
    }
}

impl Session {
    /// What the last command of this family answered (`None` until one ran).
    #[must_use]
    pub fn references_result(&self) -> Option<&F10Result> {
        self.f10.result.as_ref()
    }

    /// Sets the seed of the hidden bookmark names (`refSeed.n`): the TS draws it at element
    /// construction; here it is drawn at the first `_Ref`/`_Toc` name unless the host sets it
    /// (F10-D2).
    pub fn set_ref_seed(&mut self, n: f64) {
        self.f10.ref_seed = Some(n);
    }

    /// `refSeed.n`: the seed of the next hidden bookmark name (`None` until one is drawn or set).
    #[must_use]
    pub fn ref_seed(&self) -> Option<f64> {
        self.f10.ref_seed
    }

    /// Gives the Manual Hyphenation its patterns (the same registry as the layout engine's, F10-D3);
    /// without one the points are only the soft hyphens a word already has.
    pub fn set_hyphenation_registry(&mut self, r: Option<Arc<HyphenationRegistry>>) {
        self.f10.hyphenation = r;
    }

    /// `uiLang(this)`.
    pub(crate) fn ui_lang(&self) -> UiLang {
        self.f2.lang
    }

    pub(crate) fn exec_references(&mut self, cmd: ReferenceCmd, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        use ReferenceCmd as C;
        self.f10.result = Some(F10Result::Unit);
        match cmd {
            C::InsertField { kind } => self.insert_field(kind),
            C::InsertDateTime { pattern, update, lang } => self.insert_date_time(&pattern, update, lang),
            C::InsertDocumentInfo { kind, file_name } => self.insert_document_info(kind, &file_name),
            C::InsertAlignmentTab { alignment, relative_to, leader } => self.insert_alignment_tab(&alignment, &relative_to, &leader),
            C::UpdateFields { scope, toc } => self.update_fields(cx, scope, toc),
            C::InsertCrossReference(o) => self.insert_cross_reference(cx, &o),
            C::InsertTableOfFigures(o) => self.insert_table_of_figures(cx, &o),
            C::UpdateTableOfFigures => self.update_table_of_figures(cx),
            C::UpdateToc { mode } => self.update_toc(cx, mode),
            C::InsertTocTemplate { id } => self.insert_toc_template(cx, id),
            C::InsertTableOfContents { opts, replace } => self.insert_table_of_contents(cx, &opts, replace),
            C::RemoveTableOfContents => self.remove_table_of_contents(),
            C::SetTocLevel { level } => self.set_toc_level(level),
            C::MarkTocEntry { text, identifier, level } => self.mark_toc_entry(&text, identifier.as_deref(), level),
            C::SetCaptionSettings(v) => self.set_caption_settings(v),
            C::AddCaptionLabel { name } => self.add_caption_label(&name),
            C::DeleteCaptionLabel { name } => self.delete_caption_label(&name),
            C::SetCaptionNumbering { label, numbering } => self.set_caption_numbering(&label, numbering),
            C::SetAutoCaption { auto } => self.set_auto_caption(auto),
            C::InsertCaption(o) => self.insert_caption(cx, &o),
            C::NextManualHyphenation => self.next_manual_hyphenation(cx),
            C::ApplyManualHyphenation { letters } => self.apply_manual_hyphenation(letters),
            C::SkipManualHyphenation => self.skip_manual_hyphenation(),
            C::EndManualHyphenation => self.end_manual_hyphenation(),
        }
    }

    /// The step just taken goes and the model it changed goes back (F10-D1: a layout failure
    /// after the command changed the model; the TS throws and leaves the partial change).
    pub(crate) fn rollback_step(&mut self) {
        let (state, c) = self.history.discard();
        self.dirty_event(c);
        if let Some(s) = state {
            self.restore_state(s);
        }
    }

    /// [`Self::rollback_step`] for a command that took TWO steps (the table and its automatic
    /// caption): the inner `snapshot()` forgets which step made the document dirty, so a document
    /// that was clean before the command is made clean again here.
    pub(crate) fn rollback_nested_step(&mut self, was_dirty: bool) {
        self.rollback_step();
        if !was_dirty {
            let c = self.history.set_dirty(false);
            self.dirty_event(c);
        }
    }

    /// `Done` with the change set of a command that changed nothing.
    pub(crate) fn done_nothing(&mut self) -> Result<Outcome, CommandError> {
        Ok(Outcome::Done(self.change_set()))
    }

    /// `Done` after a step (`finish`).
    pub(crate) fn done_step(&mut self) -> Result<Outcome, CommandError> {
        Ok(Outcome::Done(self.finish()))
    }

    /// Answers a command with a boolean (the TS return value).
    pub(crate) fn answer_bool(&mut self, v: bool) {
        self.f10.result = Some(F10Result::Bool(v));
    }
}

/// The caption label map in insertion order.
pub(crate) type NumberingMap = IndexMap<String, CaptionNumbering>;

impl ReferenceCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads:
    /// none of these methods is guarded.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}

impl Session {
    /// `refBookmark(paras, para, from, to, prefix)` (hook declared by D2b-F6 for `insertHyperlink`
    /// to a heading): the hidden `_Ref`/`_Toc` bookmark that covers exactly `[from, to]` of the
    /// body paragraph, reused or new.
    pub(crate) fn ref_bookmark(&mut self, para: u32, from: u32, to: u32, prefix: &str) -> Result<crate::js::Str, CommandError> {
        let name = self.ref_bookmark_in(&super::revision_cmds::StoryLoc::Body, para, from, to, prefix)?;
        Ok(name.into())
    }
}
