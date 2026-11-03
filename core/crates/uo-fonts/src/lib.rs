//! The font layer of the UniversalOffice Rust core, shared by Write, Sheet, Slides and the
//! converter: sfnt files (table directory, cmap, metrics, hmtx, VDMX, COLR, outlines), the
//! Office kerning emulation over GPOS and the legacy `kern` table, GSUB ligatures, the converter's
//! PDF font program, TrueType subsetting, `.ttc` extraction, EOT writing, odttf obfuscation, the
//! catalog of bundled and substitute fonts, the embedding plan and Liang hyphenation. Plan and
//! decisions: `core/docs/modules/uo-fonts.md`.
//!
//! Reads follow the TS: every table is read through one view over the whole input, in the same
//! order, and a failure is caught where the TS catches it. Nothing here panics on font data and
//! nothing is allocated from a size declared in a file ([`FontLimits`]).
#![forbid(unsafe_code)]

pub mod catalog;
pub mod cmap;
pub mod colr;
pub mod embed;
pub mod eot;
pub mod error;
pub mod face;
pub mod gpos;
pub mod gsub;
pub mod hyphen;
pub mod info;
#[allow(dead_code)] // consumed by the hyphenation, variant, catalog, odttf and embedding ports
pub(crate) mod js;
pub mod limits;
pub mod odttf;
pub mod outline;
pub mod program;
pub mod read;
pub mod sfnt;
pub mod subset;
pub mod ttc;
pub mod variant;
pub mod vdmx;

pub use catalog::{BundledFamily, DEFAULT_FAMILY, METRIC_SUBSTITUTES, PROBE_FAMILIES, bundled, bundled_file_name, metric_substitute};
pub use cmap::glyph_ids_for;
pub use colr::ColorLayer;
pub use embed::{
    EmbeddingPlan, EmbeddingPolicy, FaceRequest, FaceToEmbed, FontEmbeddingSettings, FontUsage, POWERPOINT_COMMON_FONTS,
    POWERPOINT_DEFAULT_EMBEDDING, SkipReason, WORD_COMMON_FONTS, WORD_DEFAULT_EMBEDDING, embedding_requests, plan_font_embedding,
};
pub use eot::sfnt_to_eot;
pub use error::{FontError, LimitKind};
pub use face::{Face, FontMetrics};
pub use gsub::LigGroup;
#[cfg(feature = "hyphen-data")]
pub use hyphen::embedded_patterns;
pub use hyphen::{
    HYPHENATION_PATTERN_IDS, HyphenSession, HyphenationPatterns, HyphenationRegistry, Hyphenator, hyphenation_pattern_id,
    pattern_ids_to_load,
};
pub use info::{FaceInfo, FaceNames, face_info, has_true_type_outlines, is_embeddable};
pub use limits::{FontLimits, WorkBudget};
pub use outline::OutlineCmd;
pub use program::FontProgram;
pub use read::{OutOfBounds, Tag, View};
pub use rustc_hash::{FxHashMap, FxHashSet};
pub use sfnt::{TableDir, TableRecord, is_font_file, table_of};
pub use subset::subset_true_type;
pub use ttc::standalone_face;
pub use variant::{VARIANTS, Variant, is_bold_variant, is_italic_variant, variant_of, variant_of_style};
pub use vdmx::VdmxEntry;

/// No cache lives beyond its owner (a `Face` holds its own, a `HyphenSession` its own), so there
/// is nothing process-wide to clear; kept for the memory-gate convention (M1).
pub fn clear_caches() {}
