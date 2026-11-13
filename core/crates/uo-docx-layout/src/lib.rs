//! The Write layout engine of the UniversalOffice Rust core: a port of the frozen TS
//! `docx-layout` (`write/packages/docx-layout/src` on main 0.9.x) — pages, sections, columns and
//! their balancing, paragraphs, lines, tables, floats, textboxes and chains, frames, notes,
//! headers and footers, fields — producing the uo-render display list plus the Write geometry
//! (lines, characters, objects, cells). Plan, decisions and identity rules:
//! `core/docs/modules/uo-docx-layout.md` (LA1–LA7, K1–K20, LQ1–LQ22; "Milestone 1 — F0").
//!
//! Module map (TS file → module; frontier in brackets):
//! - `engine.ts` pass loop and `class Engine` → [`engine`] (`page`, `flow`, `tables`, `floats`,
//!   `notes`, `hf`, `fields`) [F7a–c], `chain` [F7d]; `frames.ts`, `chain.ts` `indexChains` →
//!   [`derive`] [F7a, F7b]; `chain.ts` `sliceParagraphFrom`/`remapLines` → `engine::chain` [F7d]
//! - `para.ts`, `hyphenate.ts`, `math.ts` → [`para`], [`hyphenate`], [`math`] [F4; the 600-DPI
//!   width model F1]
//! - `lines.ts`, `underline.ts`, `paint.ts` → [`lines`], [`underline`], [`paint`] [F5]
//! - `table.ts` → [`table`] [F6]
//! - `fonts.ts`, `context.ts`, the `SfntFont` kerning scale → [`fonts`], [`context`] [F1]
//! - Word semantics (`resolve.ts`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `outline.ts`,
//!   `numpages.ts`, `revisions.ts`, `paraformat.ts`) → `uo_docx::word`, reached via [`word`] [F2]
//! - `geometry.ts` layout half, `picture.ts`, `page-fill.ts`, `textfx.ts` → [`drawing`]; `emf.ts`,
//!   `wmf.ts` → [`metafile`] [F3]
//! - `refs.ts`, `run-boxes.ts`, `pdf-navigation.ts`, the editor queries → [`refs`],
//!   [`run_boxes`], [`navigation`], [`hit`], gathered in [`query`]; the uo-docx `LayoutOracle` →
//!   `oracle`; the views besides Print Layout → [`view_transform`] [F8]
//! - `tree.ts` → uo-render (items) + [`output`] (the Write section); its page encoding (the
//!   `write.uos` message carried as the render message's `app`, G8) → [`section`]
//! - new: [`ldoc`] (the derived document, LA2), [`keys`] (LA3), [`items`] (pass-scope item and
//!   line containers), [`options`], [`error`], [`json`] (K9), [`canon`] (the P1 stream), [`model`]
//!   (the uo-docx facade).
//!
//! Rules (LA1, LQ9): floats are the TS `f64`s in the TS order of operations, every `Math.*`
//! through uo-jsnum; no `unsafe`, no indexing that can panic, no float → integer `as` casts; a TS
//! `x[i]!` that would throw is `Err(LayoutError::Internal)`, a TS `undefined` read is `get()` with
//! the TS fallback. No `Rc`, `RefCell`, closures stored in data or global state: every TS identity
//! is an explicit key (LA3).
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]

/// A `u8` flag set without a dependency: `struct Name: u8 { const A = 1; … }`.
macro_rules! bitflags_lite {
    ($(#[$m:meta])* pub struct $name:ident: u8 { $($(#[$fm:meta])* const $f:ident = $v:expr;)* }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
        pub struct $name(pub u8);

        impl $name {
            $($(#[$fm])* pub const $f: $name = $name($v);)*

            #[must_use]
            pub fn has(self, f: $name) -> bool {
                self.0 & f.0 != 0
            }

            #[must_use]
            pub fn with(self, f: $name, on: bool) -> $name {
                if on { $name(self.0 | f.0) } else { $name(self.0 & !f.0) }
            }
        }
    };
}

pub mod annotations;
pub mod canon;
pub mod codec;
pub mod context;
pub mod derive;
pub mod drawing;
pub mod engine;
pub mod error;
pub mod fonts;
pub mod hit;
pub mod hyphenate;
pub mod incr;
pub mod items;
pub mod json;
pub mod keys;
pub mod ldoc;
pub mod lines;
pub mod math;
pub mod metafile;
pub mod model;
pub mod navigation;
pub mod options;
#[cfg(feature = "oracle")]
pub mod oracle;
pub mod output;
pub mod paint;
pub mod para;
pub mod query;
pub mod refs;
pub mod run_boxes;
pub mod section;
pub mod table;
pub mod theme_word;
pub mod underline;
pub mod view_transform;
pub mod word;

pub use error::{LayoutError, LimitKind};
pub use options::{LayoutLimits, LayoutOptions};
pub use output::{Layout, LayoutPage};

use fonts::FontProvider;
use hyphenate::HyphenSessions;
use model::{DocView, StoryChange};

/// The layout engine of one document (plan §3): the hyphenation sessions, the last `Layout` and,
/// in milestone 2, the paragraph memo and page checkpoints (LA7). The face resolver cache is per
/// call (K4). The provider is the registry policy the binding chose (LQ8).
pub struct LayoutEngine {
    hyph: HyphenSessions,
    /// the last result of [`LayoutEngine::layout`] (`None` once a relayout ran after it)
    last: Option<Layout>,
    /// the incremental state of [`LayoutEngine::relayout`] (LA7)
    incr: incr::IncrState,
    /// whether the newest attempt was a relayout (then [`LayoutEngine::last`] is its result)
    newest_relayout: bool,
}

impl Default for LayoutEngine {
    fn default() -> LayoutEngine {
        LayoutEngine::new()
    }
}

impl LayoutEngine {
    #[must_use]
    pub fn new() -> LayoutEngine {
        LayoutEngine { hyph: HyphenSessions::default(), last: None, incr: incr::IncrState::default(), newest_relayout: false }
    }

    /// An engine whose hyphenation draws on `registry` (the pattern sets the host registered;
    /// `LayoutOptions::hyphenation_loaded` still says which ones count, K5). Added by F7a.
    #[must_use]
    pub fn with_hyphenation(registry: std::sync::Arc<uo_fonts::hyphen::HyphenationRegistry>) -> LayoutEngine {
        LayoutEngine { hyph: HyphenSessions::new(registry), last: None, incr: incr::IncrState::default(), newest_relayout: false }
    }

    /// A full layout of `view` in the view `o.view` (Web and Draft lay out a transformed copy
    /// of the document, F8).
    pub fn layout(&mut self, view: DocView<'_>, provider: &dyn FontProvider, o: &LayoutOptions) -> Result<&Layout, LayoutError> {
        self.last = None;
        self.newest_relayout = false;
        let shown = view_transform::transformed(view.document(), o.view)?;
        let view = shown.as_ref().map_or(view, DocView::new);
        let ldoc = derive::build(view, o)?;
        let layout = engine::layout_document(&ldoc, o, provider, &mut self.hyph)?;
        Ok(self.last.insert(layout))
    }

    /// The incremental relayout with the caps of `cfg` (LA7; the default is on, with the caps of
    /// [`incr::IncrConfig::default`]).
    #[must_use]
    pub fn with_incremental(mut self, cfg: incr::IncrConfig) -> LayoutEngine {
        self.incr = incr::IncrState::new(cfg);
        self
    }

    /// A relayout after an edit: the same `Layout` as [`LayoutEngine::layout`] of `view`, bit for
    /// bit (LA7), computed on the previous relayout's run when only body blocks changed in place
    /// (record "Stage M2 — incremental relayout"). `changes` is advisory (D-LA7-2): what changed is
    /// found by structural sharing. On an error the incremental state is dropped (the next
    /// relayout is a full one).
    pub fn relayout(
        &mut self,
        view: DocView<'_>,
        _changes: &[StoryChange],
        provider: &dyn FontProvider,
        o: &LayoutOptions,
    ) -> Result<&Layout, LayoutError> {
        self.last = None;
        self.newest_relayout = true;
        if o.view != options::ViewTransform::Print {
            // the other views lay out a transformed copy of the document: no incremental state
            self.incr.clear();
            self.incr.full_report(incr::FallbackReason::View);
            return self.layout(view, provider, o);
        }
        let ldoc = match derive::build(view, o) {
            Ok(l) => l,
            Err(e) => {
                self.incr.fail();
                return Err(e);
            }
        };
        incr::relayout(&mut self.incr, &ldoc, provider, o, &mut self.hyph)?;
        self.incr.layout().ok_or(LayoutError::Internal("relayout: no layout"))
    }

    /// What the last relayout did (path, fallback reason, the pages laid out again, the bytes
    /// kept): the shell repaints `relaid_pages`, or every page when the path is `Full`.
    #[must_use]
    pub fn last_relayout(&self) -> Option<&incr::RelayoutReport> {
        self.incr.report()
    }

    /// Drops the incremental state (the next relayout is a full one).
    pub fn clear_incremental(&mut self) {
        self.incr.clear();
    }

    /// Lends the last relayout's result out without copying it (wave 6, L; additive): the editor
    /// shell's layout handle owns it until [`LayoutEngine::give_back`]. While it is out
    /// [`LayoutEngine::last`] answers `None`, and a relayout before the give-back is a full one
    /// (reason [`incr::FallbackReason::Lent`]). `None` when the newest result is not a relayout's
    /// (a `layout()`, another view) or it is lent already.
    pub fn lend_last(&mut self) -> Option<(incr::LendTicket, Layout)> {
        if !self.newest_relayout || self.last.is_some() {
            return None;
        }
        self.incr.lend()
    }

    /// The lend in course ([`LayoutEngine::lend_last`]), if any.
    #[must_use]
    pub fn lent(&self) -> Option<incr::LendTicket> {
        self.incr.lent()
    }

    /// Takes a lent result back, unchanged (the next relayout reuses it).
    ///
    /// # Errors
    /// The layout itself (boxed) when `ticket` is not the lend in course (another engine's, an older one, a
    /// cleared state) or its page count differs: the caller keeps it.
    pub fn give_back(&mut self, ticket: incr::LendTicket, layout: Layout) -> Result<(), Box<Layout>> {
        self.incr.give_back(ticket, layout)
    }

    /// After a failed relayout: the given-back result it did not consume (the caller lends it to
    /// its previous owner again; the engine keeps nothing of it).
    pub fn take_spare(&mut self) -> Option<Layout> {
        self.incr.take_spare()
    }

    /// The newer of the last [`LayoutEngine::layout`] and [`LayoutEngine::relayout`] results;
    /// `None` after either one failed (each drops the previous result before it runs, as
    /// `layout()` did in milestone 1; a failed relayout also drops the incremental state, and a
    /// failed `layout()` hides an older relayout result without dropping the state).
    #[must_use]
    pub fn last(&self) -> Option<&Layout> {
        if self.newest_relayout { self.last.as_ref().or_else(|| self.incr.layout()) } else { self.last.as_ref() }
    }

    /// Drops every cache (M1: live bytes return to the start).
    pub fn clear_caches(&mut self) {
        // the sessions' word caches go; the registry stays (F7a)
        self.hyph.clear();
        self.last = None;
        self.incr.clear();
    }
}

/// Nothing outside [`LayoutEngine`] caches; kept for the memory-gate convention.
pub fn clear_caches() {}
