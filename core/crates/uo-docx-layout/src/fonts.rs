//! Faces of a layout call (LA6; TS `fonts.ts`, frontier F1): the provider answers and their
//! identity, the per-call `FaceResolver` cache (K4, LQ19), the per-layout face table and the
//! DirectWrite metrics.
//!
//! The provider is a **named policy of the uo-fonts registry** (frontier P, LQ8): `viewer`,
//! `server-substitute`, `oracle` (tests), `editor` (milestone 2), plus the P2 transcript replay of
//! the parity tests. Each reproduces its TS cache keying, because the identity of the answer is
//! observable: kerning, ligatures and run merging apply only across the **same** face object
//! (K2a). This crate sees them through the registry's [`FontProvider`].

use std::sync::Arc;

use indexmap::IndexMap;

use crate::error::{Frontier, LResult, unported};
use uo_render::FaceId;

/// The provider answers come from the uo-fonts registry (frontier P, LQ8): a `FontAnswer` with
/// its [`AnswerId`] (equal ids = the same TS `FontFace` object), the parsed face pinned while the
/// answer lives, and the PDF's advance table (PQ1).
pub use uo_fonts::registry::{AnswerId, FontAnswer, FontProvider};

/// `FaceMetrics` (fonts.ts:41), computed once per face (`faceMetrics`, the DirectWrite rules).
#[derive(Clone, Copy, Debug, Default)]
pub struct FaceMetrics {
    pub ascent: f64,
    pub descent: f64,
    pub line_height: f64,
    pub underline_pos: f64,
    pub underline_thick: f64,
    pub bare_ascent: f64,
    /// `supOffset`
    pub sup_offset: Option<f64>,
}

/// One face of the layout: the answer, its id in `Resources.faces` and its metrics.
#[derive(Clone, Debug)]
pub struct FaceEntry {
    pub answer: Arc<FontAnswer>,
    pub id: FaceId,
    pub metrics: FaceMetrics,
}

/// The faces of one `Layout`, in first-use order (`FaceId` = first use of an answer identity).
#[derive(Debug, Default)]
pub struct FaceTable {
    pub entries: Vec<FaceEntry>,
    pub by_answer: IndexMap<AnswerId, FaceId>,
}

/// The `FaceResolver` (fonts.ts:224) cache: per **layout call** (K4), keyed as the TS
/// (`${family}|${bold?'b':''}${italic?'i':''}`), the resolved face after the Word substitution
/// chain (`None` = no face).
#[derive(Debug, Default)]
pub struct FaceResolverCache {
    pub resolved: IndexMap<Box<str>, Option<FaceId>>,
}

/// `deviceEmPt` (typography/lineheight.ts): the em of a size at 600 DPI.
#[must_use]
pub fn device_em_pt(size_pt: f64) -> f64 {
    (uo_jsnum::math::round((size_pt * 600.0) / 72.0) * 72.0) / 600.0
}

/// `faceMetrics(font)` (fonts.ts:55) — F1.
pub fn face_metrics(_face: &uo_fonts::Face) -> LResult<FaceMetrics> {
    unported(Frontier::F1, "fonts::face_metrics")
}
