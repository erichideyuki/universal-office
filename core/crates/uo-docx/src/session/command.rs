//! The command protocol of the session (G15, U9, U16): one [`Command`] enum of family enums (each
//! family enum lives in its own `ops` module, owned by one D2b frontier), the result of a command
//! ([`Outcome`]: done, or the shell jobs a two-phase command needs), the change set the shell and
//! the layout read, and the model-derived events the shell re-dispatches in order.

use crate::ids::ParaId;
use crate::js::Str;
use crate::text::Utf16;

use super::ops;
use super::state::Selection;

/// A model-changing request (one per model-changing public method of `UniversalWriteElement`,
/// plus the context entries the TS reaches through private methods).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// typing, Enter, Backspace/Delete, the selection (D2a)
    Text(ops::text::TextCmd),
    /// entering/leaving a cell, note, header/footer, textbox (`enterCell`, `enterNote`,
    /// `enterHeaderFooter`, `enterTextbox`, their exits)
    Context(ops::context::ContextCmd),
    /// the document options the session keeps: tracking, author, markup, protection, font
    /// embedding (D2a)
    Settings(ops::settings::SettingsCmd),
    Format(ops::format::FormatCmd),
    Paragraph(ops::paragraph::ParagraphCmd),
    List(ops::lists::ListCmd),
    Style(ops::styles::StyleCmd),
    Table(ops::tables::TableCmd),
    Section(ops::sections::SectionCmd),
    HeaderFooter(ops::header_footer::HeaderFooterCmd),
    Note(ops::notes::NoteCmd),
    Revision(ops::revision_cmds::RevisionCmd),
    Comment(ops::comments::CommentCmd),
    Bookmark(ops::bookmarks::BookmarkCmd),
    Drawing(ops::drawing::DrawingCmd),
    Picture(ops::pictures::PictureCmd),
    Page(ops::page::PageCmd),
    TextLayout(ops::text_layout::TextLayoutCmd),
    Clipboard(ops::clipboard::ClipboardCmd),
    Find(ops::find::FindCmd),
    Navigation(ops::navigation::NavigationCmd),
    /// fields, TOC, cross-references, captions, galleries, hyphenation: the layout family (U1:
    /// after the uo-docx-layout gate; goes through the `LayoutOracle`)
    Reference(ops::references::ReferenceCmd),
}

impl Command {
    /// The TS method the formatting policy guards for this command (`POLICY_GUARDS`) and the
    /// argument the guard reads; `None`: never refused.
    #[must_use]
    pub fn guard(&self) -> Option<(&'static str, super::policy::GuardArg<'_>)> {
        match self {
            Command::Text(_) | Command::Settings(_) => None,
            Command::Context(c) => match c {
                ops::context::ContextCmd::EnterHeaderFooter { .. } => Some(("enterHeaderFooter", super::policy::GuardArg::None)),
                _ => None,
            },
            Command::Format(c) => c.guard(),
            Command::Paragraph(c) => c.guard(),
            Command::List(c) => c.guard(),
            Command::Style(c) => c.guard(),
            Command::Table(c) => c.guard(),
            Command::Section(c) => c.guard(),
            Command::HeaderFooter(c) => c.guard(),
            Command::Note(c) => c.guard(),
            Command::Revision(c) => c.guard(),
            Command::Comment(c) => c.guard(),
            Command::Bookmark(c) => c.guard(),
            Command::Drawing(c) => c.guard(),
            Command::Picture(c) => c.guard(),
            Command::Page(c) => c.guard(),
            Command::TextLayout(c) => c.guard(),
            Command::Clipboard(c) => c.guard(),
            Command::Find(c) => c.guard(),
            Command::Navigation(c) => c.guard(),
            Command::Reference(c) => c.guard(),
        }
    }
}

/// The handle of a shell job of a two-phase command (G25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u32);

/// Work the shell does for a command (browser codecs, the system clipboard): the command is
/// pending until [`super::Session::complete`] gets every job's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// decode an image (size, pixels for transparency/contour)
    DecodeImage { id: JobId, bytes: std::sync::Arc<[u8]>, ext: Str },
    /// encode pixels (compress, recolour, crop to bytes): JPEG/PNG through `OffscreenCanvas`
    EncodeImage { id: JobId, width: u32, height: u32, rgba: std::sync::Arc<[u8]>, mime: Str, quality: Option<f64> },
    /// read the system clipboard (`paste()`)
    ReadClipboard { id: JobId },
    /// lay out and paint a clip for the system clipboard (`renderClipPng`)
    RenderClip { id: JobId },
}

impl Job {
    #[must_use]
    pub fn id(&self) -> JobId {
        match self {
            Job::DecodeImage { id, .. } | Job::EncodeImage { id, .. } | Job::ReadClipboard { id } | Job::RenderClip { id } => *id,
        }
    }
}

/// What the shell answers to a [`Job`].
#[derive(Debug, Clone, PartialEq)]
pub enum JobResult {
    Image {
        width: u32,
        height: u32,
        rgba: Option<std::sync::Arc<[u8]>>,
    },
    Bytes(std::sync::Arc<[u8]>),
    /// a pasted tree / text (the paste family's neutral tree, G15)
    Clipboard {
        text: Option<Utf16>,
        html_tree: Option<std::sync::Arc<[u8]>>,
        fragment: Option<std::sync::Arc<[u8]>>,
    },
    Failed,
}

/// What changed in one story (A4: the layout's incremental relayout).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoryChange {
    pub story: StoryKey,
    /// paragraphs changed in place (same id)
    pub changed: Vec<ParaId>,
    /// paragraphs inserted, removed, split or merged (or a table changed): relayout the story
    pub structure: bool,
}

/// Which story a change is in (`view::StoryRef` without the layout's paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoryKey {
    #[default]
    Body,
    /// the story edited in the current context (cell, note, header/footer, textbox)
    Context,
    /// every story (an undo, a deep step, a style change)
    All,
}

/// A model-derived event, in the order the TS dispatches it (U16). The shell turns each into its
/// `uw-*` `CustomEvent`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `uw-change` (a step; not for quiet changes)
    Change,
    /// `uw-dirty-change`
    DirtyChange { dirty: bool },
    /// `uw-input` (typed text)
    Input { text: Utf16 },
    /// `uw-protected`
    Protected { active: bool, has_fields: bool },
    /// `uw-cellmode`
    CellMode { active: bool },
    /// `uw-notemode`
    NoteMode { active: bool, footnote: bool, id: f64 },
    /// `uw-hfmode`
    HfMode { active: bool },
    /// `uw-tbmode`
    TbMode { active: bool },
    /// `uw-track-changes`
    TrackChanges { on: bool },
    /// `uw-revisions-changed`
    RevisionsChanged,
    /// `uw-comments-changed`
    CommentsChanged,
    /// `uw-bookmarks-change`
    BookmarksChange,
    /// `uw-styles-change`
    StylesChange,
    /// `uw-theme-change`
    ThemeChange,
    /// `uw-caption-settings`
    CaptionSettings,
    /// `uw-sort-error`
    SortError { message: Str },
    /// `uw-pasted`
    Pasted,
    /// `uw-custom-inline-inserted`
    CustomInlineInserted { r#type: Str },
    /// `uw-blocked` (the formatting policy refused a command)
    Blocked { command: Str },
    /// `uw-font-embedding`
    FontEmbedding { embedded: Vec<Str>, skipped: Vec<Str> },
    /// `uw-toc-update`
    TocUpdate,
    /// `uw-formatting-policy-change`
    FormattingPolicyChange,
    /// `uw-markup` (`setMarkupMode`)
    Markup,
    /// `uw-paste-options` (the options of the last paste, D2b-F8)
    PasteOptions,
}

/// The result of a command, an undo or a redo.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChangeSet {
    pub stories: Vec<StoryChange>,
    pub selection: Selection,
    pub events: Vec<Event>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// the style sheet, numbering, settings or sections changed (`formatting_version` bumped)
    pub formatting: bool,
}

/// What [`super::Session::exec`] returns.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done(ChangeSet),
    /// a two-phase command: the shell runs the jobs and calls `complete`
    NeedsJobs(Vec<Job>),
}

/// Why a command did nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// editing is locked (protected mode, read view: the TS `locked()` returns silently)
    Locked,
    /// a position outside the story
    Position,
    /// the command belongs to a family not ported yet (D2b)
    NotPorted(&'static str),
    /// the layout failed (a value, never the TS legacy fallback, G21)
    Layout(crate::view::LayoutError),
    /// a job id the session is not waiting for
    UnknownJob,
    /// a two-phase command is waiting for its jobs (G25): no other command runs meanwhile
    Busy,
}

impl From<super::text::OpError> for CommandError {
    fn from(_: super::text::OpError) -> Self {
        CommandError::Position
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Locked => f.write_str("editing is locked"),
            CommandError::Position => f.write_str("a position outside the story"),
            CommandError::NotPorted(fam) => write!(f, "the {fam} commands are not ported yet"),
            CommandError::Layout(e) => write!(f, "layout: {}", e.detail),
            CommandError::UnknownJob => f.write_str("unknown job"),
            CommandError::Busy => f.write_str("a two-phase command is pending"),
        }
    }
}

impl std::error::Error for CommandError {}
