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
//! **B6** (timing, animation, playback, show, media): `timing`/`timing_parse`/`timing_eval` (the `p:timing` model),
//! `anim_engine`/`anim_formula` (the SMIL engine), `slots` (`evaluate_slots`), `playback` (the timeline),
//! `show` (the click slide show), `media`.
//!
//! **Status**: B1b ported (package session, parse/media/slide caches, `readPresentation`, `readSlide`,
//! notes, embedded fonts) and B6 (timing, animation, playback, slide show, media, slot evaluation); the editor is a later
//! frontier (skeleton types).
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

pub mod anim;
pub mod anim_engine;
pub mod anim_formula;
#[cfg(feature = "canon")]
pub mod canon;
pub mod deck;
#[cfg(feature = "edit")]
pub mod edit;
pub mod emit;
pub mod error;
pub mod fonts;
pub mod limits;
pub mod media;
pub mod model;
pub mod package;
#[cfg(feature = "pdf")]
pub mod pdf;
pub mod playback;
#[cfg(feature = "ppt")]
pub mod ppt;
pub mod read;
pub mod show;
pub mod slots;
pub mod timing;
pub mod timing_eval;
pub mod timing_parse;

pub use deck::Deck;
pub use emit::{EmitFail, View, emit_slide};
pub use error::PptxError;
pub use limits::Limits;
pub use model::{Presentation, SlideModel};
pub use package::{PartGen, PartStore};
#[cfg(feature = "pdf")]
pub use pdf::pdf;
pub use read::{read_presentation, read_slide, read_slide_notes};

/// The deck-level caches live in [`Deck`] (`Deck::clear_caches`); nothing global yet.
pub fn clear_caches() {}
