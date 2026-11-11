//! D2b family: the clipboard (frontier D2b-F8, record `core/docs/modules/uo-docx-d2b/F8.md`): `copy`,
//! `cut`, `paste` (two-phase: the shell reads the system clipboard and sends the text, the neutral
//! paste tree of the HTML and the image, G15), `pasteData`, `pasteText`, `pastePlain`, `repaste`,
//! `pasteCells`, Paste Special (with Paste Link), Paste as Picture (two-phase: the shell lays out
//! and paints the clip, `renderClipPng`), the Office Clipboard (the ring of D2a), the paste under
//! the formatting policy and the rules of `paste-html.ts` (`html.rs`, over the paste tree of
//! `tree.rs`: the Opus boundary).
//!
//! The TS (`universal-write.ts` 4757–5757, 20430–20597, `paste-html.ts`, `numbering-store.ts`
//! `importAbstract`) is ported 1:1 for its model half; the `ClipboardEvent`, `navigator.clipboard`,
//! the layout and the canvas of `renderClipPng`, the paste button and the panel listeners stay in
//! the shell.

pub mod cells;
pub mod clone;
pub mod copy;
pub mod html;
pub mod image;
pub mod paste;
pub mod rx;
pub mod sanitize;
pub mod tree;

use std::sync::Arc;

use super::super::clipboard::{Clip, ClipPara};
use super::super::command::{ChangeSet, ClipImage, CommandError, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::text::DocPosition;
use super::super::{ExecCx, Session};
use crate::js::Str;
use crate::model::Story;
use crate::model::section::SectPr;
use crate::text::Utf16;

pub use tree::{PasteHtml, PasteLimits};

/// A paste option (`PasteMode`): Keep Source Formatting, Merge Formatting, Keep Text Only, Picture;
/// `Default` = Word's default for HTML of another program (the target's styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    Keep,
    Merge,
    Text,
    Picture,
    Default,
}

/// A list paste option (`ListPasteMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListPasteMode {
    Continue,
    New,
    Merge,
    NoMerge,
}

/// A Paste Special format (`PasteSpecialFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteSpecialFormat {
    Rtf,
    Text,
    Picture,
    Html,
    Unicode,
    Image,
}

/// How copied cells are pasted (`pasteCells(mode)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellsPasteMode {
    Overwrite,
    Rows,
    Nest,
    Text,
}

/// Whether the shell's data takes the rich copy (`withRich(d)`, the `paste` event handler) or not
/// (`pasteData(d)` as given).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RichPick {
    WithRich,
    None,
}

/// What the shell knows of the layout for a paste (`contentWidthTwips()`, F8-D3): `None` = the
/// print width of the body section.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PasteLayoutIn {
    pub content_width_twips: Option<f64>,
}

/// What is on the clipboard (`PasteData` without `rich`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteInput {
    pub text: Utf16,
    /// `d.html` (an empty or undecodable payload is no HTML)
    pub html: Option<Arc<PasteHtml>>,
    pub image: Option<ClipImage>,
}

/// `PasteData`: the input and the rich copy that goes with it. Its identity (`d === lastPaste.data`)
/// is the `Arc`'s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteData {
    pub input: Arc<PasteInput>,
    pub rich: Option<Arc<Clip>>,
}

/// `PasteRange`: from the caret to the end; `marked` = the paragraphs that took the copied mark.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteRange {
    pub start: DocPosition,
    pub end: DocPosition,
    pub first: u32,
    pub last: u32,
    pub marked: Vec<u32>,
}

/// `lastPaste`.
#[derive(Debug, Clone, PartialEq)]
pub struct LastPaste {
    pub data: Arc<PasteData>,
    pub mode: PasteMode,
    pub list: Option<ListPasteMode>,
    pub depth: usize,
    pub range: PasteRange,
}

/// `pasteDefaults`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteDefaults {
    pub within: PasteMode,
    pub between: PasteMode,
    pub other: PasteMode,
}

impl Default for PasteDefaults {
    fn default() -> Self {
        PasteDefaults { within: PasteMode::Keep, between: PasteMode::Keep, other: PasteMode::Default }
    }
}

/// What a copy puts on the system clipboard (`writeClipboard(text, html)`; the shell writes it).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CopyOut {
    pub text: Utf16,
    /// `text/html` (needs the `export` feature, F8-D5)
    pub html: Option<String>,
}

/// `pasteOptions()`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteOptions {
    pub modes: Vec<PasteMode>,
    pub list: Vec<ListPasteMode>,
    /// `current` and `currentList` (present only for the last paste's data)
    pub current: Option<(PasteMode, Option<ListPasteMode>)>,
}

/// `pasteSpecialFormats()`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpecialFormats {
    pub formats: Vec<PasteSpecialFormat>,
    pub link: bool,
    pub source: Str,
}

/// The kind of an Office Clipboard item (`clipboardItems()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Table,
    Drawing,
    Image,
}

/// One item of `clipboardItems()` (newest first).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardItem {
    pub id: u32,
    pub text: Utf16,
    pub kind: ItemKind,
    pub at: super::super::env::LocalNow,
}

/// What the shell lays out and paints for a Paste as Picture (`renderClipPng(paras)`): the clip's
/// paragraphs as a story (their pictures refer to `media`), on the cursor section's page without
/// page borders and columns, at least 31,680 twips high.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderRequest {
    pub story: Story,
    pub sect_pr: SectPr,
    pub media: Vec<Arc<[u8]>>,
}

/// A Paste as Picture waiting for its PNG: the data and the paragraphs `pastePicture` built (their
/// pictures in the clip's media when `rich_media`, else in the document's).
#[derive(Debug, Clone, PartialEq)]
pub struct PicturePaste {
    pub data: Arc<PasteData>,
    pub paras: Arc<Vec<ClipPara>>,
    pub rich_media: bool,
}

/// The family's own state (D2b-F8).
#[derive(Debug, Clone, Default)]
pub struct F8State {
    pub(crate) defaults: PasteDefaults,
    pub(crate) last_paste: Option<LastPaste>,
    /// `specialData` (Paste Special after its formats were read)
    pub(crate) special: Option<Arc<PasteData>>,
    pub(crate) special_formats: Option<SpecialFormats>,
    /// `documentPath`
    pub(crate) document_path: Option<Str>,
    /// what the last copy wrote (`None`: nothing written)
    pub(crate) copied: Option<CopyOut>,
    /// the name `addLinkSourceBookmark` gave (Paste Link between sessions, F8-D4)
    pub(crate) link_name: Option<Str>,
    /// the registered inline renderer types (`inlineRenderers` keys: the shell's plugins)
    pub(crate) inline_types: Vec<Str>,
    /// the undo's change set of a `repaste` whose paste went pending as a command of another
    /// family (F7's picture insert), was cancelled or failed: it goes in front of the session's
    /// next `Done` outcome (F8-D18)
    pub(crate) carried: Option<ChangeSet>,
}

impl F8State {
    /// `installFullDoc`: `this.lastPaste = null` (the paste defaults, the path and the plugins are
    /// the element's).
    pub(crate) fn reset_for_install(&mut self) {
        self.last_paste = None;
    }
}

impl Session {
    /// Puts the carried change set of a `repaste` (F8-D18) in front of a `Done` outcome; kept
    /// while the outcome is pending or an error.
    pub(crate) fn with_f8_carried(&mut self, out: Res) -> Res {
        match out {
            Ok(Outcome::Done(cs)) if self.f8.carried.is_some() => Ok(paste::prepend(self.f8.carried.take(), Outcome::Done(cs))),
            o => o,
        }
    }
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardCmd {
    /// `copy()`: the rich copy goes into the ring; [`Session::copied`] is what the shell writes
    Copy,
    /// `cut()`
    Cut,
    /// `paste(mode?)`: reads the clipboard (job `ReadClipboard`) then `pasteData(withRich(d))`
    Paste { mode: Option<PasteMode>, layout: PasteLayoutIn },
    /// `pasteData(d, mode?, list?)`: the shell's data (`html` = a paste-tree payload)
    PasteData {
        text: Utf16,
        html: Option<Arc<[u8]>>,
        image: Option<ClipImage>,
        rich: RichPick,
        mode: Option<PasteMode>,
        list: Option<ListPasteMode>,
        layout: PasteLayoutIn,
    },
    /// `pasteText(text)`
    PasteText { text: Utf16 },
    /// `repaste(mode, list?)`
    Repaste { mode: PasteMode, list: Option<ListPasteMode>, layout: PasteLayoutIn },
    /// `pasteCells(mode)` with the current copy
    PasteCells { mode: CellsPasteMode, layout: PasteLayoutIn },
    /// `pasteSpecialFormats()` (reads the clipboard; [`Session::paste_special_formats`])
    PasteSpecialFormats,
    /// `pasteSpecial(format, { link })`
    PasteSpecial { format: PasteSpecialFormat, link: bool, layout: PasteLayoutIn },
    /// `pasteClipboardItem(id, mode?)`
    PasteClipboardItem { id: u32, mode: Option<PasteMode>, layout: PasteLayoutIn },
    /// `pasteAllClipboardItems()`
    PasteAllClipboardItems { layout: PasteLayoutIn },
    /// `removeClipboardItem(id)`
    RemoveClipboardItem { id: u32 },
    /// `clearClipboardItems()`
    ClearClipboardItems,
    /// `documentPath = path`
    SetDocumentPath { path: Option<Str> },
    /// `pasteDefaults = …`
    SetPasteDefaults { defaults: PasteDefaults },
    /// the types of the registered inline renderers (`inlineRenderers.keys()`)
    SetInlineTypes { types: Vec<Str> },
    /// `addLinkSourceBookmark(clip)` on THIS session for the ring's current copy (`item: None`) or
    /// an item: the source half of a Paste Link between two sessions (F8-D4);
    /// [`Session::link_source_name`] is the answer
    AddLinkSource { item: Option<u32> },
    /// The target half of a Paste Link between two sessions: the `LINK` field with the source's
    /// bookmark `name` and the clip of the ring (`text`: `\t`, else `\r`)
    PasteLinkWith { name: Str, item: Option<u32>, text: bool, layout: PasteLayoutIn },
    /// (internal) Paste as Picture waiting for the shell's PNGs, in order
    RenderPictures { queue: Vec<PicturePaste>, carried: Option<Box<ChangeSet>>, layout: PasteLayoutIn },
}

impl ClipboardCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads: no
    /// clipboard method is guarded (the paste applies the policy itself).
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}

pub(crate) type Res = Result<Outcome, CommandError>;

impl Session {
    pub(crate) fn done_f8(&mut self) -> Res {
        Ok(Outcome::Done(self.finish_if_stepped()))
    }

    pub(crate) fn exec_clipboard(&mut self, cmd: ClipboardCmd, cx: &mut ExecCx<'_>) -> Res {
        let out = self.exec_clipboard_cmd(cmd, cx);
        if matches!(out, Ok(Outcome::NeedsJobs(_))) {
            // what the command did before it went pending (a picture paste builds its paragraphs
            // first: the built-in styles they define fire `uw-styles-change` then, as the TS before
            // its await) comes back with the completion (F8-D18)
            let early = self.change_set();
            if !early.events.is_empty() || !early.stories.is_empty() || early.formatting {
                let mut acc = self.f8.carried.take();
                paste::merge_cs(&mut acc, early);
                self.f8.carried = acc;
            }
        }
        out
    }

    fn exec_clipboard_cmd(&mut self, cmd: ClipboardCmd, cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            ClipboardCmd::Copy => {
                self.copy(cx);
                self.done_f8()
            }
            ClipboardCmd::Cut => self.cut(cx),
            ClipboardCmd::Paste { mode, layout } => {
                if self.locked() {
                    return self.done_f8();
                }
                let id = self.job_id();
                Ok(self.defer(
                    super::super::Command::Clipboard(ClipboardCmd::Paste { mode, layout }),
                    vec![super::super::command::Job::ReadClipboard { id }],
                ))
            }
            ClipboardCmd::PasteData { text, html, image, rich, mode, list, layout } => {
                let input = paste::input_of(text, html.as_deref(), image);
                let d = match rich {
                    RichPick::WithRich => self.with_rich(input, cx),
                    RichPick::None => PasteData { input: Arc::new(input), rich: None },
                };
                self.paste_data(Arc::new(d), mode, list, layout, cx)
            }
            ClipboardCmd::PasteText { text } => {
                let r = self.paste_text_only(text.units())?;
                if r.is_none() {
                    return self.done_f8();
                }
                self.done_f8()
            }
            ClipboardCmd::Repaste { mode, list, layout } => self.repaste(mode, list, layout, cx),
            ClipboardCmd::PasteCells { mode, layout } => {
                let clip = cx.clipboard.current().and_then(|c| c.cells.clone());
                let media = cx.clipboard.current().map(|c| c.media.clone()).unwrap_or_default();
                match clip {
                    Some(cells) => self.paste_cells(mode, &cells, Some(&media), &layout).map(|(o, _)| o),
                    None => self.done_f8(),
                }
            }
            ClipboardCmd::PasteSpecialFormats => {
                let id = self.job_id();
                Ok(self.defer(
                    super::super::Command::Clipboard(ClipboardCmd::PasteSpecialFormats),
                    vec![super::super::command::Job::ReadClipboard { id }],
                ))
            }
            ClipboardCmd::PasteSpecial { format, link, layout } => {
                if self.locked() {
                    return self.done_f8();
                }
                // `this.specialData ?? await this.readClipboard()`
                match self.f8.special.take() {
                    Some(d) => self.paste_special_with(d, format, link, layout, cx),
                    None => {
                        let id = self.job_id();
                        Ok(self.defer(
                            super::super::Command::Clipboard(ClipboardCmd::PasteSpecial { format, link, layout }),
                            vec![super::super::command::Job::ReadClipboard { id }],
                        ))
                    }
                }
            }
            ClipboardCmd::PasteClipboardItem { id, mode, layout } => {
                let Some(clip) = cx.clipboard.get(id).map(|x| Arc::clone(&x.clip)) else { return self.done_f8() };
                let d = PasteData { input: Arc::new(PasteInput { text: clip.text.clone(), ..PasteInput::default() }), rich: Some(clip) };
                self.paste_data(Arc::new(d), mode, None, layout, cx)
            }
            ClipboardCmd::PasteAllClipboardItems { layout } => self.paste_all(layout, cx),
            ClipboardCmd::RemoveClipboardItem { id } => {
                cx.clipboard.remove(id);
                self.done_f8()
            }
            ClipboardCmd::ClearClipboardItems => {
                cx.clipboard.clear();
                self.done_f8()
            }
            ClipboardCmd::SetDocumentPath { path } => {
                self.f8.document_path = path;
                self.done_f8()
            }
            ClipboardCmd::SetPasteDefaults { defaults } => {
                self.f8.defaults = defaults;
                self.done_f8()
            }
            ClipboardCmd::SetInlineTypes { types } => {
                self.f8.inline_types = types;
                self.done_f8()
            }
            ClipboardCmd::AddLinkSource { item } => {
                let clip = match item {
                    Some(id) => cx.clipboard.get(id).map(|x| Arc::clone(&x.clip)),
                    None => cx.clipboard.current().cloned(),
                };
                self.f8.link_name = clip.and_then(|c| self.add_link_source_bookmark(&c));
                self.done_f8()
            }
            ClipboardCmd::PasteLinkWith { name, item, text, layout: _ } => {
                let clip = match item {
                    Some(id) => cx.clipboard.get(id).map(|x| Arc::clone(&x.clip)),
                    None => cx.clipboard.current().cloned(),
                };
                match clip {
                    Some(c) => self.paste_link_named(&c, &name, text, cx),
                    None => self.done_f8(),
                }
            }
            // a continuation is never a command of the shell
            ClipboardCmd::RenderPictures { .. } => self.done_f8(),
        }
    }

    /// The second phase of a two-phase command of this family (G25): the shell's results are in.
    pub(crate) fn complete_clipboard(&mut self, cmd: ClipboardCmd, _job: JobId, result: JobResult, cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            ClipboardCmd::Paste { mode, layout } => match self.read_result(result, cx) {
                Some(d) => self.paste_data(Arc::new(d), mode, None, layout, cx),
                None => self.done_f8(),
            },
            ClipboardCmd::PasteSpecialFormats => {
                let d = self.read_result(result, cx).map(Arc::new);
                self.f8.special = d.clone();
                self.f8.special_formats = Some(paste::special_formats(d.as_deref()));
                self.done_f8()
            }
            ClipboardCmd::PasteSpecial { format, link, layout } => match self.read_result(result, cx) {
                Some(d) => self.paste_special_with(Arc::new(d), format, link, layout, cx),
                None => self.done_f8(),
            },
            ClipboardCmd::RenderPictures { queue, carried, layout } => self.render_done(queue, carried, result, layout, cx),
            _ => self.done_f8(),
        }
    }

    // ── queries ────────────────────────────────────────────────────────────────────────────

    /// What the last copy wrote to the system clipboard (`None`: it wrote nothing).
    #[must_use]
    pub fn copied(&self) -> Option<&CopyOut> {
        self.f8.copied.as_ref()
    }

    /// `pasteOptions()` (of the last paste).
    #[must_use]
    pub fn paste_options(&self) -> PasteOptions {
        match self.f8.last_paste.as_ref() {
            Some(lp) => self.paste_options_of(Some(&lp.data)),
            None => PasteOptions::default(),
        }
    }

    /// The last `pasteSpecialFormats()` answer.
    #[must_use]
    pub fn paste_special_formats(&self) -> Option<&SpecialFormats> {
        self.f8.special_formats.as_ref()
    }

    /// The range of the last paste (`lastPaste`; `pasteButtonRect` is the shell's).
    #[must_use]
    pub fn last_paste_range(&self) -> Option<&PasteRange> {
        self.f8.last_paste.as_ref().map(|l| &l.range)
    }

    /// The bookmark the last [`ClipboardCmd::AddLinkSource`] made.
    #[must_use]
    pub fn link_source_name(&self) -> Option<&str> {
        self.f8.link_name.as_deref()
    }

    /// The paste defaults.
    #[must_use]
    pub fn paste_defaults(&self) -> PasteDefaults {
        self.f8.defaults
    }

    /// What a pending Paste as Picture asks the shell to paint (recomputed from the pending command;
    /// `None` when no picture paste waits).
    #[must_use]
    pub fn render_clip_request(&self) -> Option<RenderRequest> {
        let p = self.pending()?;
        let super::super::Command::Clipboard(ClipboardCmd::RenderPictures { queue, .. }) = &p.command else { return None };
        let head = queue.first()?;
        Some(self.render_request_of(head))
    }

    /// `selectedText()`.
    #[must_use]
    pub fn selected_text(&self) -> Utf16 {
        Utf16::from_units(self.selected_text_units())
    }
}

/// The store operations the parity tests drive directly (hidden).
#[doc(hidden)]
pub mod testing {
    use super::super::super::env::Env;
    use super::super::super::versions::VersionStore;

    /// `DocNumbering.importAbstract(raw)`.
    pub fn import_abstract(st: &mut VersionStore, env: &mut dyn Env, raw: &str) -> f64 {
        super::super::lists::store::import_abstract(st, env, raw)
    }

    /// `raw.replace(/<w:abstractNum\b[^>]*>/, to)`.
    #[must_use]
    pub fn replace_abstract_open(raw: &str, to: &str) -> String {
        super::super::lists::store::replace_abstract_open(raw, to)
    }

    /// `raw.replace(/<w:(?:styleLink|numStyleLink) w:val="[^"]*"\/>/g, '')`.
    #[must_use]
    pub fn strip_style_links(raw: &str) -> String {
        super::super::lists::store::strip_style_links(raw)
    }
}

/// `hasCellsClip()`.
#[must_use]
pub fn has_cells_clip(ring: &super::super::ClipboardRing) -> bool {
    ring.current().is_some_and(|c| c.cells.is_some())
}

/// `clipboardItems()`: newest first, with the kind of each.
#[must_use]
pub fn clipboard_items(ring: &super::super::ClipboardRing) -> Vec<ClipboardItem> {
    ring.items()
        .map(|x| {
            let c = &x.clip;
            let kind = if c.drawing.is_some() {
                ItemKind::Drawing
            } else if c.cells.is_some() {
                ItemKind::Table
            } else if c.text.is_empty() && c.paras.iter().any(|p| p.runs.iter().any(|r| r.obj.is_some())) {
                ItemKind::Image
            } else {
                ItemKind::Text
            };
            ClipboardItem { id: x.id, text: c.text.clone(), kind, at: x.at }
        })
        .collect()
}
