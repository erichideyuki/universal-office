//! The shape model: `model.ts` of the frozen TS field by field (lines cited per type). The object
//! graph is **owned trees** (a body owns its paragraphs, a paragraph its runs, a shape its body):
//! the reader B1a clones shapes across lists (inherited decorations, the placeholder `spPr` merge)
//! and copies style images into paragraphs, which an arena with indices would have to remap at
//! every crossing. The recorded decision is in `uo-dml-render.md` "B1a" (the skeleton of the
//! architecture pass used arenas); shared bytes stay shared through the media table (`MediaId`).
//!
//! Rules every frontier keeps:
//! - every TS optional field (`x?: T`) is an `Option<T>`: the model-JSON capture tells "absent"
//!   from `false`/`0`, and `readSlide` copies with `!== undefined` tests;
//! - units are the TS's: EMU for geometry, points for sizes, fractions 0..1 for alphas and crops,
//!   1/60000 degree for `rot`;
//! - colours are [`CssColor`] strings; free text is `Box<str>` (valid UTF-8: `uo-opc` replaces a lone
//!   surrogate reference by U+FFFD, its declared decision);
//! - raw attribute vocabularies (`align`, `anchor`, `geom`, `vert`, …) stay strings: the TS compares
//!   the strings and keeps unknown values.

use std::sync::Arc;

use crate::color::CssColor;
use crate::ids::MediaId;
use crate::text_style::StyleLevels;
use crate::theme::Theme;

/// EMU per inch (`model.ts:10`).
pub const EMU_PER_INCH: f64 = 914_400.0;
/// EMU per point (`model.ts:11`).
pub const EMU_PER_POINT: f64 = 12_700.0;
/// EMU per px at 96 dpi (`model.ts:13`).
pub const EMU_PER_PX: f64 = 9_525.0;

/// `<a:xfrm>` in EMU (`model.ts:135`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Xfrm {
    pub x: f64,
    pub y: f64,
    pub cx: f64,
    pub cy: f64,
    /// 1/60000 degree.
    pub rot: Option<f64>,
    pub flip_h: Option<bool>,
    pub flip_v: Option<bool>,
}

/// Insets as fractions of each edge (`fillToRect`, `fillRect`, `srcRect`, a custGeom text rect).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Inset {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// `<a:gs>` (`model.ts:48`).
#[derive(Clone, PartialEq, Debug)]
pub struct GradientStop {
    pub pos: f64,
    pub color: CssColor,
    pub alpha: Option<f64>,
}

/// `Fill.type` (`model.ts:61`). A pattern fill is `Solid` with [`Fill::pattern`] set and the
/// average colour in [`Fill::color`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillKind {
    Solid,
    None,
    Gradient,
}

/// `<a:pattFill>` (`model.ts:75`).
#[derive(Clone, PartialEq, Debug)]
pub struct PatternFill {
    pub prst: Box<str>,
    pub fg: CssColor,
    pub bg: CssColor,
    pub fg_a: Option<f64>,
    pub bg_a: Option<f64>,
}

/// A fill (`model.ts:60`): solid, none, linear or path gradient, pattern.
#[derive(Clone, PartialEq, Debug)]
pub struct Fill {
    pub kind: FillKind,
    pub color: Option<CssColor>,
    pub stops: Option<Box<[GradientStop]>>,
    pub angle_deg: Option<f64>,
    pub scaled: Option<bool>,
    /// `<a:path path>` of a path gradient (`circle`, `rect`, `shape`).
    pub path: Option<Box<str>>,
    pub fill_to: Option<Inset>,
    pub pattern: Option<Box<PatternFill>>,
}

impl Fill {
    /// A fill of the given kind with every optional field absent.
    pub fn of_kind(kind: FillKind) -> Fill {
        Fill { kind, color: None, stops: None, angle_deg: None, scaled: None, path: None, fill_to: None, pattern: None }
    }
}

/// `<a:clrChange>` of a blip (`model.ts:114`).
#[derive(Clone, PartialEq, Debug)]
pub struct ClrChange {
    pub from: CssColor,
    pub to: CssColor,
    pub to_alpha: f64,
}

/// Blip colour effects (`ImageFx`, `model.ts:101`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ImageFx {
    pub duotone: Option<[CssColor; 2]>,
    pub alpha: Option<f64>,
    pub gray: Option<bool>,
    pub bi_level: Option<f64>,
    pub bright: Option<f64>,
    pub contrast: Option<f64>,
    pub clr_change: Option<ClrChange>,
}

/// `<a:tile>` (`ImageRef.tile`, `model.ts:95`).
#[derive(Clone, PartialEq, Debug)]
pub struct Tile {
    pub sx: f64,
    pub sy: f64,
    pub tx: f64,
    pub ty: f64,
    pub algn: Option<Box<str>>,
    /// `x`, `y` or `xy`.
    pub flip: Option<Box<str>>,
}

/// One use of an image (`ImageRef`, `model.ts:82`). The TS keeps the part name and the package's
/// bytes; here the media table entry holds both (the same `Arc` as the part store).
#[derive(Clone, PartialEq, Debug)]
pub struct ImageRef {
    pub media: MediaId,
    pub mime: Box<str>,
    pub fx: Option<Box<ImageFx>>,
    pub tile: Option<Box<Tile>>,
    pub fill_rect: Option<Inset>,
}

/// `video` or `audio` (`MediaRef.kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Video,
    Audio,
}

/// A bookmark of a media file (`p14:bmkLst`).
#[derive(Clone, PartialEq, Debug)]
pub struct MediaBookmark {
    pub name: Box<str>,
    pub time_ms: f64,
}

/// Embedded or linked media of a `<p:pic>` (`MediaRef`, `model.ts:118`).
#[derive(Clone, PartialEq, Debug)]
pub struct MediaRef {
    pub kind: MediaKind,
    pub part: Option<Box<str>>,
    pub url: Option<Box<str>>,
    pub mime: Box<str>,
    /// `p14:trim` (start, end) in ms.
    pub trim: Option<(f64, f64)>,
    /// `p14:fade` (in, out) in ms.
    pub fade: Option<(f64, f64)>,
    pub bookmarks: Option<Box<[MediaBookmark]>>,
}

/// An outer shadow (`Shape.shadow`, `Run.shadow`, `Table.shadow`; `model.ts:342`).
#[derive(Clone, PartialEq, Debug)]
pub struct OuterShadow {
    pub color: CssColor,
    pub blur_rad: f64,
    pub dist: f64,
    pub dir_deg: f64,
    pub alpha: f64,
    pub sx: Option<f64>,
    pub sy: Option<f64>,
    pub algn: Option<Box<str>>,
}

/// The run's text effects and rare formatting (`Run`, `model.ts:146`), boxed because most runs
/// have none.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RunExtras {
    pub field: Option<Box<str>>,
    /// `<a:rPr><a:ln>`: colour and width in EMU.
    pub outline: Option<(CssColor, f64)>,
    pub patt_fill: Option<PatternFill>,
    pub grad_fill: Option<Box<Fill>>,
    pub fill_image: Option<ImageRef>,
    pub fill_tile: Option<bool>,
    pub alpha: Option<f64>,
    pub sym_font: Option<Box<str>>,
    pub hyperlink: Option<Box<str>>,
    pub shadow: Option<OuterShadow>,
    pub baseline: Option<f64>,
    /// `all`, `small` or `none`.
    pub cap: Option<Box<str>>,
    pub strike: Option<bool>,
    pub spc: Option<f64>,
    pub kern: Option<f64>,
    pub link_color: Option<CssColor>,
}

/// `<a:r>` (`model.ts:146`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Run {
    pub text: Box<str>,
    pub sz: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<CssColor>,
    pub font: Option<Box<str>>,
    pub lang: Option<Box<str>>,
    pub extras: Option<Box<RunExtras>>,
}

/// `<a:tab>` (`Paragraph.tabs`).
#[derive(Clone, PartialEq, Debug)]
pub struct TabStop {
    pub pos: f64,
    pub algn: Box<str>,
}

/// `<a:p>` (`model.ts:190`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Paragraph {
    pub align: Option<Box<str>>,
    pub level: Option<f64>,
    pub runs: Vec<Run>,
    pub bullet: Option<bool>,
    pub bullet_char: Option<Box<str>>,
    pub bullet_auto_num: Option<Box<str>>,
    pub mar_l: Option<f64>,
    pub indent: Option<f64>,
    pub line_spacing: Option<f64>,
    pub line_spacing_pt: Option<f64>,
    pub tabs: Option<Box<[TabStop]>>,
    pub def_tab_sz: Option<f64>,
    pub space_before_pt: Option<f64>,
    pub space_after_pt: Option<f64>,
    pub space_before_pct: Option<f64>,
    pub space_after_pct: Option<f64>,
    pub bullet_font: Option<Box<str>>,
    pub bullet_color: Option<CssColor>,
    pub bullet_size_pct: Option<f64>,
    pub bullet_size_pt: Option<f64>,
    pub bullet_image: Option<Box<ImageRef>>,
    pub end_sz: Option<f64>,
    pub ea_ln_brk: Option<bool>,
    pub hanging_punct: Option<bool>,
    pub font_algn: Option<Box<str>>,
}

/// `<a:prstTxWarp>` (`TextBody.warp`).
#[derive(Clone, PartialEq, Debug)]
pub struct TextWarp {
    pub prst: Box<str>,
    pub adj: Option<Box<[f64]>>,
    pub from_word_art: Option<bool>,
}

/// `normAutofit`, `noAutofit`, `spAutoFit` (`TextBody.autofit`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autofit {
    Norm,
    None,
    Shape,
}

/// `<p:txBody>` (`model.ts:239`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextBody {
    pub anchor: Option<Box<str>>,
    pub paragraphs: Vec<Paragraph>,
    pub wrap: Option<Box<str>>,
    pub font_scale: Option<f64>,
    pub autofit: Option<Autofit>,
    pub ln_spc_reduction: Option<f64>,
    pub spc_first_last_para: Option<bool>,
    pub warp: Option<Box<TextWarp>>,
    pub ins_l: Option<f64>,
    pub ins_t: Option<f64>,
    pub ins_r: Option<f64>,
    pub ins_b: Option<f64>,
    pub vert: Option<Box<str>>,
    pub rot: Option<f64>,
    pub num_col: Option<f64>,
    pub spc_col: Option<f64>,
    pub vert_overflow: Option<Box<str>>,
    pub horz_overflow: Option<Box<str>>,
    /// The body's own `<a:lstStyle>`, parsed by level.
    pub lst_style: Option<Box<StyleLevels>>,
}

/// `<a:custGeom>` (`Shape.custGeom`, `model.ts:294`): subpaths in the geometry's own `w × h`.
#[derive(Clone, PartialEq, Debug)]
pub struct CustGeom {
    pub w: f64,
    pub h: f64,
    pub subs: uo_drawingml::Subpaths,
    pub no_fill: Option<Box<[bool]>>,
    pub no_stroke: Option<Box<[bool]>>,
    pub lum: Option<Box<[f64]>>,
    pub text_rect: Option<Inset>,
}

/// `<a:glow>` (`Shape.glow`).
#[derive(Clone, PartialEq, Debug)]
pub struct Glow {
    pub rad: f64,
    pub color: CssColor,
    pub alpha: f64,
}

/// `<a:reflection>` (`Shape.reflection`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Reflection {
    pub blur_rad: f64,
    pub st_a: f64,
    pub end_a: f64,
    pub st_pos: f64,
    pub end_pos: f64,
    pub dist: f64,
    pub dir_deg: f64,
    pub sy: f64,
    pub fade_dir_deg: f64,
}

/// `<a:sp3d><a:bevelT>` with the scene light and material (`Shape.bevel`).
#[derive(Clone, PartialEq, Debug)]
pub struct Bevel {
    pub w: f64,
    pub h: f64,
    pub prst: Option<Box<str>>,
    pub rig: Option<Box<str>>,
    pub dir: Option<Box<str>>,
    pub mat: Option<Box<str>>,
}

/// `<a:sp3d extrusionH>` (`Shape.extrusion`).
#[derive(Clone, PartialEq, Debug)]
pub struct Extrusion {
    pub h: f64,
    pub color: Option<CssColor>,
    pub rig: Option<Box<str>>,
    pub dir: Option<Box<str>>,
    pub mat: Option<Box<str>>,
}

/// `<a:camera><a:rot lat lon rev>` in degrees (`Shape.camRot`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CamRot {
    pub lat: f64,
    pub lon: f64,
    pub rev: f64,
}

/// `<a:innerShdw>` (`Shape.innerShadow`).
#[derive(Clone, PartialEq, Debug)]
pub struct InnerShadow {
    pub color: CssColor,
    pub blur_rad: f64,
    pub dist: f64,
    pub dir_deg: f64,
    pub alpha: f64,
}

/// `<a:fillOverlay blend>` (`Shape.fillOverlay`).
#[derive(Clone, PartialEq, Debug)]
pub struct FillOverlay {
    pub blend: Box<str>,
    pub fill: Fill,
}

/// `'mso2007' | 'modern'` (`Shape.chartFormat`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartFormat {
    Mso2007,
    Modern,
}

/// A chart frame (`Shape.chartXml`, `chartPart`, `chartTheme`, `chartImages`, `chartFormat`). The TS
/// stores the chart part's XML **text** in every shape; here the part name is kept and the text is
/// read through the host's part source (its parse cache). The model-JSON capture serializes the
/// part's text hash for `chartXml`.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartRef {
    pub part: Box<str>,
    pub theme: Arc<Theme>,
    /// `chartImages`: the chart part's `a:blipFill` images by relationship id, in TS insertion order.
    pub images: Option<Box<[ChartImage]>>,
    pub format: Option<ChartFormat>,
}

/// One `chartImages` entry: relationship id and the image use.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartImage {
    pub r_id: Box<str>,
    pub image: ImageRef,
}

/// The rare payloads of a shape (`model.ts:275–391`), boxed so a plain text box stays small.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShapeExtras {
    pub descr: Option<Box<str>>,
    pub dgm_id: Option<Box<str>>,
    pub dgm_text_of: Option<Box<str>>,
    pub media: Option<MediaRef>,
    pub chart: Option<ChartRef>,
    pub table: Option<Box<Table>>,
    pub shadow: Option<OuterShadow>,
    pub glow: Option<Glow>,
    pub soft_edge: Option<f64>,
    pub reflection: Option<Reflection>,
    pub bevel: Option<Bevel>,
    pub extrusion: Option<Extrusion>,
    pub cam_prst: Option<Box<str>>,
    pub cam_rot: Option<CamRot>,
    pub inner_shadow: Option<InnerShadow>,
    pub fill_overlay: Option<FillOverlay>,
    pub head_arrow: Option<Box<str>>,
    pub tail_arrow: Option<Box<str>>,
    /// `cNvPr id`s of the enclosing groups, outermost first.
    pub group_ids: Option<Box<[Box<str>]>>,
}

/// A shape (`<p:sp>`, `<p:pic>`, `<p:cxnSp>`, a graphic frame, a flattened group member;
/// `model.ts:275`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Shape {
    pub id: Box<str>,
    pub name: Box<str>,
    pub ph_type: Option<Box<str>>,
    pub ph_idx: Option<Box<str>>,
    pub is_text_box: Option<bool>,
    pub xfrm: Option<Xfrm>,
    pub geom: Option<Box<str>>,
    pub grp_fill: Option<bool>,
    pub adj: Option<Box<[f64]>>,
    pub adj_names: Option<Box<[Box<str>]>>,
    pub cust_geom: Option<Box<CustGeom>>,
    pub fill: Option<Box<Fill>>,
    pub line_color: Option<CssColor>,
    pub line_alpha: Option<f64>,
    pub line_width: Option<f64>,
    pub line_none: Option<bool>,
    pub line_dash: Option<Box<str>>,
    pub tx_body: Option<Box<TextBody>>,
    pub image: Option<Box<ImageRef>>,
    pub src_rect: Option<Inset>,
    pub alpha: Option<f64>,
    pub use_bg_fill: Option<bool>,
    pub fill_alpha: Option<f64>,
    pub fill_image: Option<Box<ImageRef>>,
    pub fill_tile: Option<bool>,
    pub fill_no_rot: Option<bool>,
    pub style_font_color: Option<CssColor>,
    pub style_font_idx: Option<Box<str>>,
    /// Decoration inherited from the layout/master (`readSlide`).
    pub inherited: Option<bool>,
    pub extras: Option<Box<ShapeExtras>>,
}

/// One side of a cell's explicit borders (`TableCell.borders`).
#[derive(Clone, PartialEq, Debug)]
pub struct CellBorder {
    pub color: CssColor,
    pub width: f64,
    pub dash: Option<Box<str>>,
}

/// An edge of the resolved border grid (`TableLine`, `model.ts:450`).
#[derive(Clone, PartialEq, Debug)]
pub struct TableLine {
    pub color: CssColor,
    pub width: f64,
    pub alpha: Option<f64>,
    pub dash: Option<Box<str>>,
    pub cmpd: Option<Box<str>>,
}

/// An explicit `tcPr` line: absent, `<a:noFill/>` (`null` in the TS) or a line.
#[derive(Clone, PartialEq, Debug)]
pub enum CellLine {
    NoFill,
    Line(TableLine),
}

/// Four optional sides (`l`, `r`, `t`, `b`).
#[derive(Clone, PartialEq, Debug)]
pub struct Sides<T> {
    pub l: Option<T>,
    pub r: Option<T>,
    pub t: Option<T>,
    pub b: Option<T>,
}

impl<T> Default for Sides<T> {
    fn default() -> Self {
        Sides { l: None, r: None, t: None, b: None }
    }
}

/// `<a:tc>` (`model.ts:395`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TableCell {
    pub grid_span: Option<f64>,
    pub row_span: Option<f64>,
    pub h_merge: Option<bool>,
    pub v_merge: Option<bool>,
    pub tx_body: Option<Box<TextBody>>,
    pub fill: Option<CssColor>,
    pub border_color: Option<CssColor>,
    pub border_width: Option<f64>,
    pub fill_alpha: Option<f64>,
    pub borders: Option<Box<Sides<CellBorder>>>,
    pub anchor: Option<Box<str>>,
    pub vert: Option<Box<str>>,
    pub no_fill: Option<bool>,
    pub border_none: Option<Sides<bool>>,
    pub mar_l: Option<f64>,
    pub mar_r: Option<f64>,
    pub mar_t: Option<f64>,
    pub mar_b: Option<f64>,
    pub tx_color: Option<CssColor>,
    pub tx_bold: Option<bool>,
    pub tx_italic: Option<bool>,
    pub fill_paint: Option<Box<Fill>>,
    pub fill_image: Option<Box<ImageRef>>,
    pub fill_tile: Option<bool>,
    pub lines: Option<Box<Sides<CellLine>>>,
}

/// `<a:tr>` (`model.ts:461`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TableRow {
    pub height_emu: f64,
    pub cells: Vec<TableCell>,
}

/// `Table.bgPaint` (`model.ts:474`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TableBgPaint {
    pub fill: Option<Fill>,
    pub alpha: Option<f64>,
    pub image: Option<Box<ImageRef>>,
    pub tile: Option<bool>,
}

/// A border grid (`hLines`/`vLines`) stored row-major in one vector: `None` = no edge (`null`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LineGrid {
    pub rows: u32,
    pub cols: u32,
    pub cells: Vec<Option<TableLine>>,
}

/// `<a:tbl>` (`model.ts:468`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Table {
    pub col_widths_emu: Vec<f64>,
    pub bg: Option<(CssColor, Option<f64>)>,
    pub bg_paint: Option<Box<TableBgPaint>>,
    pub shadow: Option<OuterShadow>,
    pub rows: Vec<TableRow>,
    /// `(rows + 1) × cols` horizontal edges.
    pub h_lines: Option<LineGrid>,
    /// `rows × (cols + 1)` vertical edges.
    pub v_lines: Option<LineGrid>,
}
