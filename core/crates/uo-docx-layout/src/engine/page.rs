//! Page geometry, sections and columns (frontier F7a): `SectGeom`, `PageState`, the z-ordered
//! float layers and the wrap frames (engine.ts:35–151).

use crate::context::NoteNumbering;
use crate::items::{PassItems, PassLine};
use crate::output::{PlacedCell, PlacedObject};
use uo_render::{OptF64, PageFillId, Sym};

/// A column of the text area: `{ x, w }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col {
    pub x: f64,
    pub w: f64,
}

/// `chap`: chapter numbers before the page number (`w:chapStyle`/`w:chapSep`).
#[derive(Clone, Debug)]
pub struct ChapStyle {
    pub level: f64,
    pub sep: Box<str>,
}

/// `w:lnNumType`.
#[derive(Clone, Debug)]
pub struct LnNum {
    pub count_by: f64,
    pub start: f64,
    pub restart: Box<str>,
    pub distance: f64,
}

/// `notes` of a section: effective numbering, footnote position, note-area columns.
#[derive(Clone, Debug)]
pub struct SectNotes {
    pub footnote: NoteNumbering,
    pub endnote: NoteNumbering,
    pub fn_pos: Box<str>,
    pub fn_cols: OptF64,
}

/// `SectGeom` (engine.ts:35): the geometry of a section, derived once per pass (cache).
#[derive(Clone, Debug)]
pub struct SectGeom {
    pub index: u32,
    pub page_w: f64,
    pub page_h: f64,
    pub mar_l: f64,
    pub mar_r: f64,
    pub mar_t: f64,
    pub mar_b: f64,
    pub hdr: f64,
    pub ftr: f64,
    pub cols: Vec<Col>,
    pub col_sep: bool,
    pub title_pg: bool,
    pub kind: Box<str>,
    pub v_align: Option<Box<str>>,
    pub pg_start: OptF64,
    pub pg_fmt: Option<Box<str>>,
    pub chap: Option<ChapStyle>,
    /// header/footer stories per slot `[default, first, even]` (index into `LDoc::stories`; K13:
    /// one part can serve several slots)
    pub headers: [Option<u32>; 3],
    pub footers: [Option<u32>; 3],
    /// `blocks`: `[start, end)` in the body
    pub blocks: (u32, u32),
    pub ln_num: Option<LnNum>,
    pub line_pitch: OptF64,
    pub notes: SectNotes,
    /// `balKey` (default: the section index; the endnote area is balanced apart)
    pub bal_key: Option<i64>,
}

/// `FloatBox` (engine.ts:71): a float's wrap box.
#[derive(Clone, Debug)]
pub struct FloatBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub wrap: Box<str>,
    pub side: Option<Box<str>>,
    pub dist_l: f64,
    pub dist_r: f64,
    pub dist_t: f64,
    pub dist_b: f64,
    pub body_only: bool,
    /// `poly`: the wrap polygon (page twips)
    pub poly: Option<Vec<(f64, f64)>>,
}

/// `WrapFrame` (engine.ts:121): a line's space around floats.
#[derive(Clone, Debug)]
pub struct WrapFrame {
    pub left: f64,
    pub right: f64,
    pub skip_to: OptF64,
    pub segs: Option<Vec<(f64, f64)>>,
    pub gap: bool,
}

/// `ZObj` (engine.ts:151): a float with its z order (`relativeHeight` / VML z-index).
#[derive(Clone, Debug, Default)]
pub struct ZObj {
    pub z: f64,
    pub seq: f64,
    pub items: PassItems,
}

/// The `LayoutPage` being built inside a `PageState` (its items are assembled from the layers at
/// the end of the pass).
#[derive(Clone, Debug, Default)]
pub struct PageOut {
    pub w: f64,
    pub h: f64,
    pub section: u32,
    pub number: f64,
    pub lines: Vec<PassLine>,
    pub objects: Option<Vec<PlacedObject>>,
    pub cells: Option<Vec<PlacedCell>>,
    pub hf_items: Option<u32>,
    pub background: Option<Sym>,
    pub background_fill: Option<PageFillId>,
    pub background_show: bool,
}

/// A note on a page: its id and its laid-out content.
#[derive(Clone, Debug)]
pub struct PageNote {
    pub id: i32,
    pub content: crate::table::CellContent,
}

/// `PageState` (engine.ts:123). Output (`page`, the layers) and per-page working state (floats,
/// notes, note area) — the level-2 checkpoints of milestone 2 keep it whole.
#[derive(Clone, Debug, Default)]
pub struct PageState {
    pub page: PageOut,
    /// index into `Engine::geoms`
    pub geom: u32,
    pub body_top: f64,
    pub body_bottom: f64,
    pub behind: Vec<ZObj>,
    pub hf: PassItems,
    pub body: PassItems,
    pub front: Vec<ZObj>,
    pub hf_behind: Vec<ZObj>,
    pub hf_front: Vec<ZObj>,
    pub floats: Vec<FloatBox>,
    pub sect_page: u32,
    pub notes: Vec<PageNote>,
    pub note_area: f64,
    pub body_bottom_base: f64,
    pub notes_full: bool,
    pub cols: Option<Vec<Col>>,
}
