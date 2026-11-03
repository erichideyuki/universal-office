//! The model as this crate reads it: the uo-docx D1a types (architecture A2, A4; layout
//! "Requirements on uo-docx"). The only module of the crate that names uo-docx: every other module
//! goes through these re-exports ("Milestone 1 — F0", decision F0-1).

pub use uo_docx::ids::{DrawingId, HfPartId, MediaId, ParaId, TableId};
pub use uo_docx::js::NumKey;
pub use uo_docx::model::props::PPr;
pub use uo_docx::model::{ParaProps, Paragraph, Run};
pub use uo_docx::text::Utf16;
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
