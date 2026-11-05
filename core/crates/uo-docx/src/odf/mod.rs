//! docx-core `odf/**` (feature `odf`): OpenDocument Text (`.odt`) read and write for the Write
//! model. `zip.ts` is not here: the container is uo-opc's package (zip read and write, limits).
//!
//! | TS | module |
//! |---|---|
//! | `odf/index.ts` (`isOdt`, `readOdt`, the re-exports) | [`reader`] |
//! | `odf/units.ts` | [`units`] |
//! | `odf/props.ts` | [`props`] |
//! | `odf/styles.ts` | [`styles`] |
//! | `odf/write-odt.ts` | [`write_odt`] |
//!
//! Declarations of the port (the nine kinds of the frontier brief): `core/docs/modules/uo-docx-d5/D5b.md`.

pub mod props;
pub mod reader;
pub mod styles;
pub mod units;
pub mod write_odt;

pub use reader::{ODT_MIMETYPE, is_odt, read_odt, read_package};
pub use units::{length_to_half_pt, length_to_twips, length_to_twips_or};
pub use write_odt::{OdtImages, OdtOptions, write_odt};
