//! The shared DrawingML shape renderer of the UniversalOffice Rust core, used by Slides
//! (`uo-pptx`) and by Sheet's drawings: the shape model read from `p:`/`xdr:` shape trees, the
//! theme with the PowerPoint HSL colour-modifier profile, the text-style cascade, table styles,
//! the text layout fed by advance tables (decision T1), and the emission of shapes, pictures,
//! tables, SmartArt and metafiles as a display list (the Slides list of `uo-render`).
//!
//! Architecture and boundary: `core/docs/modules/uo-dml-render.md`; the presentation side and the
//! decisions T1/T2/P1: `core/docs/modules/uo-pptx.md`; the display-list items:
//! `core/docs/modules/uo-render.md` "Slides extensions (plan §4.2) — design".
//!
//! Behaviour source: the frozen TS `slides/packages/slides-core/src` (0.9.2, `b92b60fa`):
//! `model.ts`, the shape half of `read.ts`, `theme.ts`, `text-style.ts`, `table-style*.ts`,
//! `symbol-fonts.ts`, `text-layout.ts`, `render.ts` (minus the pixel-reading effects, which stay in
//! the shell painter), `raster.ts`, `chart-engine.ts`, `diagram-*.ts`, `metafile.ts`, `emfplus.ts`,
//! `pict.ts`.
//!
//! **Status**: B1a ported (shape model as owned trees, the shape reader, the theme with the PowerPoint
//! colour profile, the text-style cascade, table styles, symbol fonts, the canonical JSON of the parity
//! captures under the feature `canon`) and B3 (the text layout and its measurement protocol, `text`; the SmartArt
//! text metrics, `smartart::metrics`). Emission, SmartArt regeneration and metafiles (B4–B5) are still to come.
#![forbid(unsafe_code)]
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[cfg(feature = "canon")]
pub mod canon;
pub mod color;
pub mod emit;
pub mod ids;
pub mod metafile;
pub mod model;
pub mod read;
pub mod read_dml;
pub mod read_text;
pub mod smartart;
pub mod source;
pub mod symbol;
mod symbol_data;
pub mod table_style;
mod table_styles_data;
pub mod text;
pub mod text_style;
pub mod theme;

pub use color::CssColor;
pub use ids::{MediaId, Span, ThemeId};
pub use model::Shape;
pub use source::{Limits, PartSource, R, ReadError};
pub use theme::Theme;
/// `looksLikeEot` and `eotToSfnt` of `eot.ts`: the reader lives in `uo_fonts::eot`; the deck's font reader (`uo-pptx`) calls it here.
pub use uo_fonts::eot::{looks_like_eot, read as eot_to_sfnt};

/// Nothing caches yet; the generated-bitmap and chart-model caches of B4 will be emptied here
/// (memory gate M1, `uo-pptx.md` M5 table).
pub fn clear_caches() {}
