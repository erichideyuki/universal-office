//! The document model (architecture A2). One model for the viewer, the PDF path and the
//! editor: each story stored once in the editor's flat shape ([`story::Story`]), nodes immutable
//! behind `Arc` and changed by copy-on-write, cross references as typed ids, text in UTF-16, and
//! every TS-observable state represented (explicit `undefined` keys as cleared bits, stored NaN,
//! key order). The TS names and shapes are kept field by field (the canonical digest writes the
//! TS names); string-literal unions of the TS are `Str` (the TS casts attribute values with
//! `as`, so any string can be stored), unless a frontier narrows one (declared, Opus check).
//!
//! | TS source | module |
//! |---|---|
//! | `ooxml/props.ts` | [`props`] |
//! | `text-effects.ts` | [`effects`] |
//! | `drawing.ts` | [`drawing`], [`color`] |
//! | `picture-xml.ts` (read) | [`picture`] |
//! | `document.ts` | [`run`], [`para`] |
//! | `fulldoc.ts` | [`table`], [`section`], [`story`], [`document`] |
//! | `ooxml/parts.ts`, `styles.ts` | [`formatting`] |
//! | `page-background.ts` (read) | [`background`] |
//! | `source.ts` (read) | [`sources`] |

pub mod background;
pub mod color;
pub mod document;
pub mod drawing;
pub mod effects;
pub mod formatting;
pub mod media;
pub mod para;
pub mod picture;
pub mod props;
pub mod run;
pub mod section;
pub mod sources;
pub mod story;
pub mod table;

pub use document::{Bookmarks, Comment, CoreProps, Document, Notes, ReadFacts};
pub use media::{MediaRef, MediaStore};
pub use para::{ParaProps, Paragraph};
pub use run::{Run, RunExt, RunProps};
pub use section::{HfPart, Section};
pub use story::{Block, Story, TableAnchor};
pub use table::{Cell, Row, Table};
