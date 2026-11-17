//! The Slides presentation model of the UniversalOffice Rust core: the byte-part package session
//! ([`package`]), the deck with its parse caches and the slide/layout/master graph ([`deck`]), the
//! slide model read over it ([`model`]), timing and animation ([`timing`], [`anim`]), and with
//! features the editing session (`edit`), the `.ppt` converter (`ppt`) and the exporters
//! (`export`). Shapes, themes, the text cascade, text layout and the display-list emission are the
//! shared `uo-dml-render` (used by Sheet's drawings too).
//!
//! Architecture: `core/docs/modules/uo-pptx.md` (data model P1–P7, frontiers B1–B8, parity, traps,
//! memory, decisions T1/T2/P1). Behaviour source: the frozen TS `slides/packages/slides-core/src`
//! (0.9.2, `b92b60fa`) and the editor model `slides/packages/slides/src/{deck-edit,xml-edit,
//! history,blank-deck}.ts`.
//!
//! **Status**: B1b ported (package session, parse/media/slide caches, `readPresentation`, `readSlide`,
//! notes, embedded fonts); timing, `.ppt` and the editor are later frontiers (skeleton types).
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

pub mod anim;
#[cfg(feature = "canon")]
pub mod canon;
pub mod deck;
#[cfg(feature = "edit")]
pub mod edit;
pub mod error;
pub mod fonts;
pub mod limits;
pub mod model;
pub mod package;
#[cfg(feature = "ppt")]
pub mod ppt;
pub mod read;
pub mod timing;

pub use deck::Deck;
pub use error::PptxError;
pub use limits::Limits;
pub use model::{Presentation, SlideModel};
pub use package::{PartGen, PartStore};
pub use read::{read_presentation, read_slide, read_slide_notes};

/// The deck-level caches live in [`Deck`] (`Deck::clear_caches`); nothing global yet.
pub fn clear_caches() {}
