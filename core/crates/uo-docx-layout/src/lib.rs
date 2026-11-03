//! The Write layout engine of the UniversalOffice Rust core: a port of the frozen TS
//! `docx-layout` (`write/packages/docx-layout/src` on main 0.9.x) — pages, sections, columns and
//! their balancing, paragraphs, lines, tables, floats, textboxes and chains, frames, notes,
//! headers and footers, fields — producing the uo-render display list plus the Write geometry
//! (lines, characters, objects, cells). Plan, decisions and identity rules:
//! `core/docs/modules/uo-docx-layout.md` (LA1–LA7, K1–K20, LQ1–LQ22; "Milestone 1 — F0").
//!
//! Module map (TS file → module; frontier in brackets):
//! - `engine.ts` pass loop and `class Engine` → [`engine`] (`page`, `flow`, `tables`, `floats`,
//!   `notes`, `hf`, `fields`) [F7a–c]; `frames.ts`, `chain.ts` → [`derive`] [F7a]
//! - `para.ts`, `hyphenate.ts`, `math.ts` → [`para`], [`hyphenate`], [`math`] [F4; the 600-DPI
//!   width model F1]
//! - `lines.ts`, `underline.ts`, `paint.ts` → [`lines`], [`underline`], [`paint`] [F5]
//! - `table.ts` → [`table`] [F6]
//! - `fonts.ts`, `context.ts`, the `SfntFont` kerning scale → [`fonts`], [`context`] [F1]
//! - Word semantics (`resolve.ts`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `outline.ts`,
//!   `numpages.ts`, `revisions.ts`, `paraformat.ts`) → `uo_docx::word`, reached via [`word`] [F2]
//! - `geometry.ts` layout half, `picture.ts`, `page-fill.ts`, `textfx.ts` → [`drawing`]; `emf.ts`,
//!   `wmf.ts` → [`metafile`] [F3]
//! - `refs.ts`, `run-boxes.ts`, `pdf-navigation.ts`, the editor queries → [`query`] [F8]
//! - `tree.ts` → uo-render (items) + [`output`] (the Write section)
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

pub mod canon;
pub mod codec;
pub mod context;
pub mod derive;
pub mod drawing;
pub mod engine;
pub mod error;
pub mod fonts;
pub mod hyphenate;
pub mod items;
pub mod json;
pub mod keys;
pub mod ldoc;
pub mod lines;
pub mod math;
pub mod metafile;
pub mod model;
pub mod options;
pub mod output;
pub mod paint;
pub mod para;
pub mod query;
pub mod table;
pub mod underline;
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
    last: Option<Layout>,
}

impl Default for LayoutEngine {
    fn default() -> LayoutEngine {
        LayoutEngine::new()
    }
}

impl LayoutEngine {
    #[must_use]
    pub fn new() -> LayoutEngine {
        LayoutEngine { hyph: HyphenSessions::default(), last: None }
    }

    /// A full layout of `view`.
    pub fn layout(&mut self, view: DocView<'_>, provider: &dyn FontProvider, o: &LayoutOptions) -> Result<&Layout, LayoutError> {
        self.last = None;
        let ldoc = derive::build(view, o)?;
        let layout = engine::layout_document(&ldoc, o, provider, &mut self.hyph)?;
        Ok(self.last.insert(layout))
    }

    /// A relayout after `changes`. Milestone 1: a full layout (correct by definition, LA7); the
    /// incremental levels come in milestone 2 with the criterion "same bits as a full layout".
    pub fn relayout(
        &mut self,
        view: DocView<'_>,
        _changes: &[StoryChange],
        provider: &dyn FontProvider,
        o: &LayoutOptions,
    ) -> Result<&Layout, LayoutError> {
        self.layout(view, provider, o)
    }

    /// The last successful layout.
    #[must_use]
    pub fn last(&self) -> Option<&Layout> {
        self.last.as_ref()
    }

    /// Drops every cache (M1: live bytes return to the start).
    pub fn clear_caches(&mut self) {
        self.hyph = HyphenSessions::default();
        self.last = None;
    }
}

/// Nothing outside [`LayoutEngine`] caches; kept for the memory-gate convention.
pub fn clear_caches() {}
