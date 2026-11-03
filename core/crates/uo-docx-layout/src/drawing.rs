//! Drawing values resolved into display-list values (frontier F3): shapes and groups
//! (`geometry.ts` 356–475: `substPhClr`, `withThemeStyles`, `shapeExtras`, `groupChild`, building
//! the `uo_render::types::ShapeGeom` spec — the path evaluation itself moved to `uo_render::geom`,
//! RQ1), pictures (`picture.ts` `pictureLook`), the page fill (`page-fill.ts` `resolvePageFill`,
//! `imagePixelInfo`, `patternRowsFromImage`: a derived image per pass, K12/RQ3) and text effects
//! (`textfx.ts` `fxColor`, `fillOf`, `resolveTextFx`). Colours through uo-drawingml; strings kept
//! as the document wrote them (RQ6).
//!
//! Signatures are fixed by F3 against the uo-docx model types (`ShapeStyle`, `PageBackground`,
//! `TextEffects`, `Theme`) once they land; they return `uo_render` values and register images,
//! text effects and page fills in the layout's `Resources` through `Cx`.
