//! EMF/WMF records → display items (`emf.ts`, `wmf.ts`, frontier F3; LQ7: here now, moving to a
//! shared crate with Slides' EMF path later). EMF bitmap records become BMP bytes and EMF+ images
//! are copied: derived images (K12), one `ImageId` per creation (RQ3). Bounded by
//! `max_metafile_records` and `max_metafile_items` (truncated drawing + warning, §7).

use crate::error::{Frontier, LResult, unported};
use crate::items::PassItems;
use uo_render::FaceId;

/// The only thing the metafile players need from the engine: a face for a font record (LQ7) and a
/// place for warnings (the TS passes `DocContext` for those two only).
pub trait FaceLookup {
    /// the face of a metafile font record (family as written, weight, italic)
    fn face(&mut self, family: &[u16], bold: bool, italic: bool) -> Option<FaceId>;
    fn warn(&mut self, msg: &str);
}

/// Where a metafile is drawn: the frame in page twips and the crop.
#[derive(Clone, Copy, Debug)]
pub struct MetafileFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `renderEmf(bytes, …)` (emf.ts) — F3.
pub fn render_emf(_bytes: &[u8], _at: MetafileFrame, _faces: &mut dyn FaceLookup, _out: &mut PassItems) -> LResult<bool> {
    unported(Frontier::F3, "metafile::render_emf")
}

/// `renderWmf(bytes, …)` (wmf.ts) — F3.
pub fn render_wmf(_bytes: &[u8], _at: MetafileFrame, _faces: &mut dyn FaceLookup, _out: &mut PassItems) -> LResult<bool> {
    unported(Frontier::F3, "metafile::render_wmf")
}
