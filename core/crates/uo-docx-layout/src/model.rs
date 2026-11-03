//! The model as this crate reads it: the uo-docx D1a types (architecture A2, A4; layout
//! "Requirements on uo-docx"). The only module of the crate that names uo-docx: every other module
//! goes through these re-exports ("Milestone 1 — F0", decision F0-1).

pub use uo_docx::ids::{DrawingId, HfPartId, MediaId, ParaId, TableId};
pub use uo_docx::js::NumKey;
pub use uo_docx::model::props::PPr;
pub use uo_docx::model::{ParaProps, Paragraph, Run};
pub use uo_docx::text::Utf16;
// F5 (lines and paint): run properties, borders, shading, settings and inline objects
pub use uo_docx::js::JsNumber;
pub use uo_docx::model::formatting::Settings;
pub use uo_docx::model::props::{BorderSpec, PBdr, RFonts, RPr, ShadingSpec, TextShadow};
pub use uo_docx::model::run::{DrawingShape, InlineImage, Ptab};
pub use uo_docx::model::section::PageBorders;
pub use uo_docx::word::resolve::{ColorRef, FontSlot, font_of_slot, resolve_color, shading_fill};
// F1 (fonts): the font table the face resolver reads and the embedded fonts of `withEmbeddedFonts`
pub use uo_docx::js::JsRecord;
pub use uo_docx::model::formatting::FontInfo;
pub use uo_docx::read::EmbeddedFont;
pub use uo_docx::view::{
    BlockPath, CellKey, DocRef, DocView, FieldScope, HfWhich, HyphenCandidate, HyphenSkips, NoteKind, RunRef, RunSub, StoryRef,
};

/// What changed in a story since the previous layout (A4 `ChangeSet.stories`; milestone 2). Kept
/// here until the uo-docx session (D2a) defines it; `relayout` takes it already.
#[derive(Clone, Debug)]
pub struct StoryChange {
    pub story: StoryRef,
    pub changed: Vec<ParaId>,
    /// insert, delete, split or merge
    pub structure: bool,
}
// F3 (drawing values and metafiles): shape styles, groups, pictures, the page background, text
// effects, the theme and the media they read
pub use uo_docx::js::{Nullable, Str};
pub use uo_docx::model::MediaStore;
pub use uo_docx::model::background::PageBackground;
pub use uo_docx::model::color::DrawingColor;
pub use uo_docx::model::drawing::ShapeStyle;
pub use uo_docx::model::effects::{FxColor, FxFill, TextEffects};
pub use uo_docx::model::formatting::Theme;
pub use uo_docx::model::media::ImageData;
pub use uo_docx::model::picture::{PicBlipFx, PicColor};
pub use uo_docx::model::run::{DrawingGroup, GroupItem};
