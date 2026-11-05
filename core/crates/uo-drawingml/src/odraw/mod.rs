//! OfficeArt ([MS-ODRAW]), the drawing layer of the Office 97-2003 binaries, shared by the
//! `.doc`, `.xls` and `.ppt` readers (uo-docx plan G11, feature `legacy`): records, shape
//! properties, the shape tree, the BLIP store and the BLIPs, and the msospt → DrawingML preset
//! map. API brief and decisions: `core/docs/modules/uo-drawingml.md`, "odraw — OfficeArt".
//!
//! Behaviour source: Write's `docx-core/src/doc/officeart.ts`, `bytes.ts` (the record half) and
//! `preset-map.ts` (frozen `main` 0.9.x); Slides' `ppt/officeart.ts` and Sheet's
//! `xls/officeart.ts`/`odraw-geom.ts` are copies that drifted: where they differ the API takes a
//! [`Flavor`] or a [`msospt::PresetVariant`], so the Slides and Sheet ports plug in without a new
//! shape of the API.
//!
//! Everything borrows the input stream (`'a`): shapes keep their property data and records as
//! slices, BLIPs that need no rewrite are borrowed. Nothing panics or loops without progress on
//! any input: every record walk advances by at least 8 bytes, a FOPT has at most 4,095 entries,
//! group nesting stops at [`OdrawLimits::max_group_depth`], and inflating a metafile is capped
//! per picture and per document.

mod bstore;
mod fopt;
pub mod msospt;
mod record;
mod shape;
mod word;

pub use bstore::{Blip, BlipBudget, BlipKind, Bse, decode_blip, read_bstore, read_fbse};
pub use fopt::{Prop, PropTable, is_boolean_prop, merge_booleans, read_fopt};
pub use record::{Rec, Recs, read_rec, recs_in, rt};
pub use shape::{
    AnchorKind, PROP_HSP_MASTER, RectI32, Shape, ShapeMap, apply_masters, fsp, index_drawing, read_sp_container, read_spgr_container,
};
pub use word::{WordDrawings, read_word_drawings};

/// Which reader's rules apply where the three TS copies differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Write `.doc` (`docx-core/src/doc/officeart.ts`): the behaviour ported and verified here
    Word,
    /// Slides `.ppt` (`slides-core/src/ppt/officeart.ts`)
    Slides,
    /// Sheet `.xls` (`sheet-core/src/xls/officeart.ts`)
    Sheet,
}

/// Limits of the OfficeArt readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OdrawLimits {
    /// bytes one metafile BLIP may inflate to (`MAX_BLIP`, 64 MiB in the three TS readers)
    pub max_blip_bytes: u64,
    /// nesting of groups read (`MAX_GROUP_DEPTH`: 64 in Word and Slides, 32 in Sheet)
    pub max_group_depth: u32,
    /// `dgglbl` + `DgContainer` pairs Word reads after the `DggContainer` (`guard < 16`)
    pub max_word_drawings: u32,
    /// shapes one tree read builds ([`read_spgr_container`], [`index_drawing`], and all the
    /// drawings of one [`read_word_drawings`]); past it the remaining shape containers are
    /// skipped. The TS has no bound: an 8-byte empty `SpContainer` costs a `Shape` (about
    /// 400 bytes); a listed difference on hostile input only
    pub max_shapes: u32,
    /// properties [`apply_masters`] may add to the shapes of one map; past it the remaining
    /// shapes inherit nothing. The TS copies without bound (one master of 4,095 properties
    /// named by every shape multiplies the input); a listed difference on hostile input only
    pub max_master_props: u32,
}

impl OdrawLimits {
    /// The limits of the Word and Slides readers.
    pub const WORD: OdrawLimits = OdrawLimits {
        max_blip_bytes: 64 << 20,
        max_group_depth: 64,
        max_word_drawings: 16,
        max_shapes: 1 << 16,
        max_master_props: 1 << 20,
    };

    /// The limits of the Sheet reader (groups nest to 32).
    pub const SHEET: OdrawLimits = OdrawLimits { max_group_depth: 32, ..OdrawLimits::WORD };
}

impl Default for OdrawLimits {
    fn default() -> Self {
        OdrawLimits::WORD
    }
}
