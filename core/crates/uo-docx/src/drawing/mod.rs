//! Word drawing-object helpers the editing session feeds to the picture and shape XML
//! operations, moved in from `shared/drawingml` (uo-drawingml gate decision G3: Word-only code
//! does not live in the shared DrawingML crate). Record `core/docs/modules/uo-docx.md`,
//! "contour and shape styles (from uo-drawingml G3)".
//!
//! | TS (`shared/drawingml/src`) | module |
//! |---|---|
//! | `contour.ts` `wrapContour` (the `wp:wrapPolygon` of a picture with transparency, 21600 units) | [`contour`] |
//! | `shape-styles.ts` `SHAPE_STYLE_PRESETS` (the "Shape Styles" gallery: `wps:spPr` tail + `wps:style`) | [`shape_styles`] |
//!
//! This is not [`crate::word`] (Word semantics over the model that the layout and the session
//! share: resolution, numbering, fields) nor [`crate::read`] (the reader): these are inputs and
//! generators of the edit operations (`setImageWrap` and `setShapeStyle` of the editor), so they
//! sit next to the future `write`/`session` modules, in a module of their own that either can use.

pub mod contour;
pub mod shape_styles;

pub use contour::{ContourError, Vertex, wrap_contour};
pub use shape_styles::{SHAPE_STYLE_PRESETS, ShapeStylePreset, find_shape_style, shape_style_preset};
