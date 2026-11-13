//! The derived document of one layout call (LA2, K1, K3): what `layoutDocument` synthesizes before
//! and during laying out — frames lifted into textboxes anchored in a copy of the next paragraph
//! (`liftFrames`), textbox chains (`indexChains`), continuation slices of chained paragraphs
//! (`sliceParagraphFrom`), deleted paragraphs joined in the final view (`joinDeleted`), the
//! revision-view copies — as **shallow** copies: a derived paragraph is a list of run keys with
//! optional overrides, never a cloned model `Paragraph`, and a model run keeps its identity (its
//! `RunRef`) through every copy. Nothing of the model is cloned; no text is copied (a slice keeps
//! an offset into the model run's UTF-16 text). Built per call by `derive.rs` (F7a), dropped with
//! it (LQ18: the TS cross-call frame caches are not reproduced).

use std::sync::Arc;

use crate::keys::{ChainId, LParaId, RunKey, TbKey};
use crate::model::{BlockPath, DocView, DrawingId, PPr, ParaId, ParaProps, Paragraph, Run, RunRef, StoryRef};

/// The derived document.
pub struct LDoc<'a> {
    pub view: DocView<'a>,
    /// every paragraph the passes touch, model or copy ([`LParaId`])
    pub paras: Vec<LPara<'a>>,
    /// every run the passes touch ([`RunKey`])
    pub runs: Vec<RunSrc<'a>>,
    /// body, each header/footer part per slot use, each note, each textbox content
    pub stories: Vec<LStory>,
    /// model textboxes and synthetic frame boxes ([`TbKey`])
    pub textboxes: Vec<LTextbox>,
    /// chains of linked textboxes ([`ChainId`])
    pub chains: Vec<Chain<'a>>,
    /// `revisionAuthors(full)` order (the default `RevisionView.order`)
    pub revision_order: Vec<&'a str>,
    /// the key of the `j`-th field-result run of a model run (`RunRef` with
    /// `RunSub::FieldResult(j)`): the identity of an object `prepareParagraph` shows from a field's
    /// cached result (added by F4)
    pub field_results: rustc_hash::FxHashMap<(RunKey, u32), RunKey>,
    /// the derived paragraph of each model paragraph (cells and textbox contents included; added
    /// by F7a: the engine reaches cell paragraphs by their model id)
    pub by_para: rustc_hash::FxHashMap<crate::model::ParaId, LParaId>,
    /// the story of each header/footer part (F7a)
    pub hf_stories: rustc_hash::FxHashMap<crate::model::HfPartId, u32>,
    /// the story of each note (F7a)
    pub note_stories: rustc_hash::FxHashMap<(crate::model::NoteKind, crate::model::NumKey), u32>,
    /// the textbox of each model drawing (F7a)
    pub tb_of_drawing: rustc_hash::FxHashMap<DrawingId, TbKey>,
    /// the endnote separator paragraph (`{ runs: [], props: paraPropsFromPPr(sepPPr), ppr:
    /// sepPPr }` of `placeEndnotes`, engine.ts:2461): a copy with both overrides and no runs, so
    /// no model field of its base is read (added by F7c)
    pub en_sep: Option<LParaId>,
    /// wave 7 (B, gate-O3): the theme every colour resolves with ([`crate::theme_word::theme_of`]): the document's,
    /// or Word's Office 2023 colours for a package without a theme part
    pub theme: &'a uo_docx::model::formatting::Theme,
    /// wave 7c (W, item 2; oracle nosettings): Word's compatibility mode for a package WITHOUT `word/settings.xml` (12,
    /// measured: COM `CompatibilityMode`); `None` = the model's (the frozen TS reads 15, `ts_missing_settings_mode15`)
    pub compat_word: Option<f64>,
    /// wave 7c (W, item 2): Word's default tab stop when the settings have no `w:defaultTabStop` (708 tw, measured);
    /// `None` = the model's (the frozen TS's 720)
    pub default_tab_word: Option<f64>,
    /// wave 7c (W, item 4): the document carries bookmark marks in its runs (a lossless read, `sources`: the editor's session), so
    /// a reference can find a bookmark outside the body index
    pub bookmark_marks: bool,
    /// wave 7c (W, item 4, fix round 1): the bookmark marks of every derived paragraph by name, built once per call on
    /// the first reference that needs it (`Engine::bookmark_where`; its building charges the budget)
    pub bookmark_mark_index: std::cell::OnceCell<BookmarkMarkIndex>,
}

/// Wave 7c (W, item 4, fix round 1): for each bookmark name, what the FIRST derived paragraph that opens it holds:
/// `Some(text)` when its end mark (the same id) is in that paragraph (the range's text, objects skipped), `None` when it is
/// not (Word's error: an unterminated bookmark).
pub type BookmarkMarkIndex = rustc_hash::FxHashMap<Box<str>, Option<Vec<u16>>>;

/// Where a derived paragraph comes from.
#[derive(Clone, Copy, Debug)]
pub enum ParaSrc<'a> {
    /// the model object itself (the TS hands it to the engine): keyed by its `ParaId`
    Model { para: &'a Paragraph, id: ParaId },
    /// a TS copy (`{ ...p, … }`): **no** `ParaId`, so what the TS keys by paragraph object misses
    /// it, in the TS and here (a manual hyphenation skip never matches a copy)
    Copy { base: LParaId },
}

bitflags_lite! {
    /// The TS `WeakSet<Paragraph>` memberships (K3), per derived paragraph.
    pub struct ParaFlags: u8 {
        /// `CONTINUATION` (chain.ts:15): no marker, no first-line indent, no space before
        const CONTINUATION = 1;
        /// `FRAME_SKIP` (frames.ts): the frame paragraph left the flow
        const FRAME_SKIP = 2;
        /// `INNER` (frames.ts): the copy inside a lifted frame
        const INNER = 4;
    }
}

/// One derived paragraph (the TS object the engine holds).
#[derive(Debug)]
pub struct LPara<'a> {
    pub src: ParaSrc<'a>,
    /// overrides of a copy (revision view `{ ...para, ppr, props }`, frame inner copy `{ ...p, ppr }`)
    pub ppr: Option<Arc<PPr>>,
    pub props: Option<Arc<ParaProps>>,
    /// the TS `runs` array of this object, in order
    pub runs: Vec<RunKey>,
    /// `REV_JOINS` (revisions.ts:151, a symbol key copied by later spreads)
    pub rev_joins: Vec<u32>,
    pub flags: ParaFlags,
    /// `FRAME_CARRY` (frames.ts:25): the frame run this anchor copy carries
    pub frame_carry: Option<RunKey>,
    /// where the paragraph sits (`BlockPath` = the TS ref path, `Story::blocks()` order)
    pub story: StoryRef,
    pub path: BlockPath,
}

/// Where a run comes from.
#[derive(Debug)]
pub enum RunSrc<'a> {
    /// a model run (also when reached through a copied paragraph, K1): reported by
    /// `field_values`, the `PlacedObject.run` of floats anchored in copies
    Model { run: &'a Run, at: RunRef },
    /// a slice `{ ...r, text: r.text.slice(from) }` of a model run (chain.ts:57): the text is the
    /// model run's from `from`, never copied
    Slice { base: RunKey, from: u32 },
    /// a run the TS builds (the frame run of a lifted frame)
    Synth(Box<Run>),
}

/// A story of the derived document: its blocks in `Story::blocks()` order.
#[derive(Debug, Default)]
pub struct LStory {
    pub blocks: Vec<LBlock>,
    /// the model story (its tables are read through the view; added by F7c)
    pub sref: Option<StoryRef>,
}

/// A block of a derived story.
#[derive(Debug, Clone, Copy)]
pub enum LBlock {
    Paragraph(LParaId),
    /// a table of the model, by its block index in the story (the layout reads it through the view)
    Table {
        block: u32,
    },
}

/// `FrameInfo` (frames.ts:20) of a lifted `w:framePr`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameHRule {
    Auto,
    Exact,
    AtLeast,
}

/// `FRAME_INFO` (frames.ts): the frame data of a synthetic textbox.
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    pub auto_width: bool,
    pub h_rule: FrameHRule,
    pub h: f64,
}

/// A textbox of the derived document.
#[derive(Debug)]
pub struct LTextbox {
    /// the model drawing (`None` for a frame lifted by `liftFrames`)
    pub drawing: Option<DrawingId>,
    /// the content story (index into `LDoc::stories`)
    pub story: u32,
    pub frame: Option<FrameInfo>,
    /// the chain it belongs to
    pub chain: Option<ChainId>,
}

/// A chain of linked textboxes (`TextboxChain`): the first box (with the content) and the linked
/// ones in `seq` order.
#[derive(Debug)]
pub struct Chain<'a> {
    pub members: Vec<TbKey>,
    /// the model box of each member (`members[k]`'s; added by F7d: `chainSlice` reads each box's
    /// size, insets and colours)
    pub boxes: Vec<&'a crate::model::DrawingTextbox>,
}

impl<'a> LDoc<'a> {
    /// An empty derived document over `view` (the shape `derive.rs` fills).
    #[must_use]
    pub fn empty(view: DocView<'a>) -> LDoc<'a> {
        LDoc {
            view,
            paras: Vec::new(),
            runs: Vec::new(),
            stories: Vec::new(),
            textboxes: Vec::new(),
            chains: Vec::new(),
            revision_order: Vec::new(),
            field_results: rustc_hash::FxHashMap::default(),
            by_para: rustc_hash::FxHashMap::default(),
            hf_stories: rustc_hash::FxHashMap::default(),
            note_stories: rustc_hash::FxHashMap::default(),
            tb_of_drawing: rustc_hash::FxHashMap::default(),
            en_sep: None,
            theme: crate::theme_word::theme_of(view, false),
            compat_word: None,
            default_tab_word: None,
            bookmark_marks: false,
            bookmark_mark_index: std::cell::OnceCell::new(),
        }
    }

    /// The compatibility mode the layout follows (wave 7c, W, item 2): Word's for a package without settings, else the
    /// model's `settings.compatibilityMode`.
    #[must_use]
    pub fn compat_mode(&self) -> f64 {
        self.compat_word.unwrap_or_else(|| self.view.formatting().settings.compatibility_mode.to_number())
    }

    /// The default tab stop the layout follows (wave 7c, W, item 2): Word's 708 tw when the settings do not give one,
    /// else the model's `settings.defaultTabStop`.
    #[must_use]
    pub fn default_tab_stop(&self) -> f64 {
        self.default_tab_word.unwrap_or_else(|| self.view.formatting().settings.default_tab_stop.to_number())
    }

    #[must_use]
    pub fn para(&self, k: LParaId) -> Option<&LPara<'a>> {
        self.paras.get(k.ix())
    }

    #[must_use]
    pub fn run(&self, k: RunKey) -> Option<&RunSrc<'a>> {
        self.runs.get(k.ix())
    }

    /// The key of the `j`-th field-result run of `base` (F4).
    #[must_use]
    pub fn field_result_key(&self, base: RunKey, j: u32) -> Option<RunKey> {
        self.field_results.get(&(base, j)).copied()
    }

    /// The model `ParaId` of a derived paragraph: only the model object itself has one.
    #[must_use]
    pub fn para_id(&self, k: LParaId) -> Option<ParaId> {
        match self.para(k)?.src {
            ParaSrc::Model { id, .. } => Some(id),
            ParaSrc::Copy { .. } => None,
        }
    }

    /// The `RunRef` of a model run (absent for slices and synthetic runs, as they are absent from
    /// `field_values`).
    #[must_use]
    pub fn run_ref(&self, k: RunKey) -> Option<&RunRef> {
        match self.run(k)? {
            RunSrc::Model { at, .. } => Some(at),
            RunSrc::Slice { .. } | RunSrc::Synth(_) => None,
        }
    }

    /// The UTF-16 text of a run (a slice reads its model run's text from its offset).
    #[must_use]
    pub fn run_text(&self, k: RunKey) -> &[u16] {
        self.run_text_at(k, 0)
    }

    fn run_text_at(&self, k: RunKey, depth: u32) -> &[u16] {
        match self.run(k) {
            Some(RunSrc::Model { run, .. }) => run.text.units(),
            Some(RunSrc::Synth(run)) => run.text.units(),
            // a slice of a slice is a slice of the base; the depth bound keeps a malformed chain
            // of keys from recursing (the builder never makes one)
            Some(RunSrc::Slice { base, from }) if depth < 64 => {
                let t = self.run_text_at(*base, depth + 1);
                t.get(*from as usize..).unwrap_or(&[])
            }
            _ => &[],
        }
    }
}
