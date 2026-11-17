//! The display list of the UniversalOffice Rust core: the contract between every producer of
//! pages (Write's layout now, Slides and Sheet later) and every painter (the TS canvas painter of
//! the shell, uo-pdf, later XPS and raster). Plan and decisions: `core/docs/modules/uo-render.md`
//! (RA1-RA3, RQ1-RQ13; the "Types (T)" section records what frontier T fixed).
//!
//! - [`types`]: the item vocabulary (no Word concepts: no refs, runs, paragraphs, line numbers or
//!   header/footer flags; those travel in the producer's app section), the per-layout
//!   [`Resources`] (faces, images, text effects, page fills, custom paths, interned strings) and
//!   the PDF [`Navigation`] value.
//! - [`geom`]: shape geometry evaluated by each painter in its own unit (RQ1) — frontier G.
//! - [`paint`]: the raster and fill helpers both painters need — frontier G.
//! - [`encode`]: the canvas encoder (frontier E): a [`Page`] and its [`Resources`] to the per-page
//!   message, with the canvas hints of §3.3, the app-section hook and the inverse for tests.
//! - [`view`]: the view the shell holds: pages, resources, the encoder's delta mark and the
//!   item-addressed paint operations behind the wasm exports (frontier E).
//! - [`codec`]: the schema-driven per-page encoding (G15, RQ4) — frontier C, generated from
//!   `core/schema/render.uos` (and, separately, `slides.uos`/`slots.uos` for Slides).
//! - [`slides`]: the Slides display list (a canvas call sequence per scale), its resources and codec (R-S1/R-S2).
//!
//! Numbers are the TS values bit for bit: every coordinate is an `f64` in the producer's unit
//! (twips from the page top-left for Write), optional numbers keep "absent" apart from every
//! value ([`OptF64`]), colours are the strings the document wrote (interned, RQ6) and code points
//! are `u32`, never `char` (lone surrogates occur, RQ9).
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]

pub mod codec;
pub mod encode;
pub mod geom;
mod ids;
mod list;
mod nav;
mod opt;
pub mod paint;
mod resources;
pub mod slides;
mod sym;
pub mod types;
pub mod view;

pub use encode::{AppSection, CanvasOptions, DeltaMark, NoApp, encode_page_for_canvas, encode_page_for_canvas_with};
pub use ids::{CustId, FaceId, ImageId, PageFillId, Sym, TextFxId};
pub use list::{CapacityError, GlyphColumns, GlyphRunHead, ItemList, NO_GID, RunSide, Span};
pub use nav::{LinkTarget, Navigation, OutlineEntry, PageLink, PdfLink};
pub use opt::OptF64;
pub use resources::{FaceRes, ImageRes, Resources};
pub use sym::SymbolTable;
pub use types::{Item, Page};
pub use view::{RenderView, ViewError};

/// The crate keeps no cache; kept for the M1 harness (`clear_caches` before measuring).
pub fn clear_caches() {}
