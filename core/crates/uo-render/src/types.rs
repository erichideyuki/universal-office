//! The item vocabulary (`tree.ts` of the frozen TS engine, minus the Write geometry, which stays
//! in the producer's app section: RA2, RQ2, K14).
//!
//! Field by field the TS shape, with these representation rules (uo-render RA1, "Types (T)"):
//! strings are [`Sym`]s, optional numbers [`OptF64`], optional booleans `bool` (every painter
//! tests them by truthiness), model objects become ids ([`FaceId`], [`ImageId`], [`CustId`]),
//! and variable-length payloads live in the [`ItemList`] columns or behind a `Box`, so an
//! [`Item`] is at most 48 bytes (`tests/size.rs`).

use crate::ids::{CustId, FaceId, ImageId, PageFillId, Sym};
use crate::list::{ItemList, Span};
use crate::opt::OptF64;

/// One page of a layout: the TS `LayoutPage` minus the Write section (`section`, `number`,
/// `lines`, `hfItems`, `objects`, `cells`).
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// `widthTw`, `heightTw` (the producer's unit).
    pub w: f64,
    pub h: f64,
    /// `items` in paint order, with their columns.
    pub list: ItemList,
    /// `background` (`w:background` colour as written; the painters parse it differently, §4.2).
    pub background: Option<Sym>,
    /// `backgroundFill`.
    pub background_fill: Option<PageFillId>,
    /// `backgroundShow`.
    pub background_show: bool,
}

/// One display item (TS `PageItem`). Small variants are inline (the tag packs into their
/// padding); the two big ones are boxed.
#[derive(Clone, Debug)]
pub enum Item {
    /// `GlyphRunItem`: glyphs `list.glyphs[glyphs]`, the rest of the run in `list.runs[run]`.
    Glyphs {
        face: FaceId,
        /// `color` (RRGGBB as resolved, or whatever string the producer wrote).
        color: Sym,
        size_pt: f64,
        /// baseline.
        y: f64,
        glyphs: Span,
        run: u32,
        flags: RunFlags,
    },
    /// `RectItem`.
    Rect { x: f64, y: f64, w: f64, h: f64, fill: Sym, alpha: OptF64 },
    /// `LineItem`.
    Line { x1: f64, y1: f64, x2: f64, y2: f64, color: Sym, width_pt: f64, dash: Option<LineDash> },
    /// `ImageItem`.
    Image(Box<ImageItem>),
    /// `ShapeItem`.
    Shape(Box<ShapeItem>),
    /// `PathItem`: commands `list.path_cmds[cmds]`.
    Path { cmds: Span, fill: Option<Sym>, stroke: Option<Stroke>, even_odd: bool },
    /// `ClipItem` with `op: 'push'` (the box fields are optional in the TS; `shiftItem` leaves a
    /// push without `y` untouched).
    ClipPush { x: OptF64, y: OptF64, w: OptF64, h: OptF64 },
    /// `ClipItem` with `op: 'pop'`.
    ClipPop,
    /// `XformItem` with `op: 'push'`; a push without `m` acts as a pop in both painters
    /// (canvas.ts:245, pdf.ts:469).
    XformPush { m: Option<Box<[f64; 6]>> },
    /// `XformItem` with `op: 'pop'`.
    XformPop,
}

/// The optional booleans of a glyph run (absent = false).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct RunFlags(pub u8);

impl RunFlags {
    /// `synthBold`
    pub const SYNTH_BOLD: u8 = 1;
    /// `synthItalic`
    pub const SYNTH_ITALIC: u8 = 2;
    /// `asPath`: outlines in the PDF (WordArt, translucent text, rotated boxes)
    pub const AS_PATH: u8 = 4;
    /// `colr`: colour glyphs drawn as layered outlines
    pub const COLR: u8 = 8;

    #[must_use]
    pub fn has(self, f: u8) -> bool {
        self.0 & f != 0
    }

    #[must_use]
    pub fn with(self, f: u8, on: bool) -> RunFlags {
        if on { RunFlags(self.0 | f) } else { RunFlags(self.0 & !f) }
    }
}

/// `LineItem.dash` (RQ13): the nine kinds; both painters expand them with the same arithmetic
/// (`BORDER_DASHES`, then x20 on the canvas).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LineDash {
    Dotted,
    Dashed,
    DashDot,
    DashDotDot,
    BdrDot,
    BdrDash,
    BdrDashDot,
    BdrDashDotDot,
    BdrDashSmall,
}

impl LineDash {
    /// The TS string.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LineDash::Dotted => "dotted",
            LineDash::Dashed => "dashed",
            LineDash::DashDot => "dashDot",
            LineDash::DashDotDot => "dashDotDot",
            LineDash::BdrDot => "bdrDot",
            LineDash::BdrDash => "bdrDash",
            LineDash::BdrDashDot => "bdrDashDot",
            LineDash::BdrDashDotDot => "bdrDashDotDot",
            LineDash::BdrDashSmall => "bdrDashSmall",
        }
    }

    /// The kind of a TS string (`None` for anything else).
    #[must_use]
    pub fn from_name(s: &str) -> Option<LineDash> {
        Some(match s {
            "dotted" => LineDash::Dotted,
            "dashed" => LineDash::Dashed,
            "dashDot" => LineDash::DashDot,
            "dashDotDot" => LineDash::DashDotDot,
            "bdrDot" => LineDash::BdrDot,
            "bdrDash" => LineDash::BdrDash,
            "bdrDashDot" => LineDash::BdrDashDot,
            "bdrDashDotDot" => LineDash::BdrDashDotDot,
            "bdrDashSmall" => LineDash::BdrDashSmall,
            _ => return None,
        })
    }

    /// `BORDER_DASHES[name](w)` (tree.ts:271-274) for the border kinds, in the TS arithmetic;
    /// `None` for the line kinds (each painter has its own pattern for them).
    #[must_use]
    pub fn border_dashes(self, w: f64) -> Option<Vec<f64>> {
        Some(match self {
            LineDash::BdrDot => vec![w, w],
            LineDash::BdrDash => vec![3.0 * w, w],
            LineDash::BdrDashDot => vec![3.0 * w, w, w, w],
            LineDash::BdrDashDotDot => vec![3.0 * w, w, w, w, w, w],
            LineDash::BdrDashSmall => vec![(25.0 * w) / 6.0, (5.0 * w) / 6.0],
            _ => return None,
        })
    }
}

/// `{ color, widthPt, dash? }` of a shape or path outline.
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub color: Sym,
    pub width_pt: f64,
    pub dash: Option<Sym>,
}

/// A path command (`PathCmdTw`), in the producer's unit.
#[derive(Clone, Copy, Debug)]
pub enum PathCmd {
    M { x: f64, y: f64 },
    L { x: f64, y: f64 },
    C { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Z,
}

/// `{ l, t, r, b }` (crop fractions, `fillToRect`).
#[derive(Clone, Copy, Debug)]
pub struct Ltrb {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// `ImageItem`.
#[derive(Clone, Debug)]
pub struct ImageItem {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub image: ImageId,
    pub crop: Option<Ltrb>,
    pub rot: OptF64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub pic: Option<Box<PictureLook>>,
}

/// `PictureLook` (Picture Format, resolved by the layout).
#[derive(Clone, Debug, Default)]
pub struct PictureLook {
    pub geom: Option<PresetSpec>,
    pub line: Option<PicLine>,
    pub shadow: Option<PicShadow>,
    pub glow: Option<PicGlow>,
    pub soft_edge_pt: OptF64,
    pub bevel: Option<PicBevel>,
    pub reflection: Option<PicReflection>,
    pub alpha: OptF64,
    pub duotone: Option<[Sym; 2]>,
    pub clr_change: Option<ClrChange>,
}

/// `{ prst, adj? }`: a preset with its adjust values in the TS object's key order (integer-like
/// names first, then insertion order — uo-jsnum `object::js_object_order`).
#[derive(Clone, Debug)]
pub struct PresetSpec {
    pub prst: Sym,
    pub adj: Option<Vec<(Sym, f64)>>,
}

/// `PictureLook.line`.
#[derive(Clone, Copy, Debug)]
pub struct PicLine {
    pub color: Sym,
    pub alpha: f64,
    pub width_pt: f64,
    pub dash: Option<Sym>,
    pub cmpd: Option<Sym>,
    pub cap: Option<Sym>,
    pub join: Option<Sym>,
}

/// `'outer' | 'inner'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowKind {
    Outer,
    Inner,
}

/// `PictureLook.shadow` (also the shape effects' shadow).
#[derive(Clone, Copy, Debug)]
pub struct PicShadow {
    pub kind: ShadowKind,
    pub color: Sym,
    pub alpha: f64,
    pub blur_pt: f64,
    pub dist_pt: f64,
    pub dir_deg: f64,
    pub sx: OptF64,
    pub sy: OptF64,
    pub kx_deg: OptF64,
    pub ky_deg: OptF64,
    pub algn: Option<Sym>,
    pub rot_with_shape: bool,
}

/// `PictureLook.glow`.
#[derive(Clone, Copy, Debug)]
pub struct PicGlow {
    pub color: Sym,
    pub alpha: f64,
    pub rad_pt: f64,
}

/// `PictureLook.bevel`.
#[derive(Clone, Copy, Debug)]
pub struct PicBevel {
    pub w_pt: f64,
    pub h_pt: f64,
    pub prst: Option<Sym>,
    pub rig: Option<Sym>,
    pub dir: Option<Sym>,
    pub material: Option<Sym>,
}

/// `PictureLook.reflection`.
#[derive(Clone, Copy, Debug)]
pub struct PicReflection {
    pub blur_pt: f64,
    pub st_a: f64,
    pub st_pos: f64,
    pub end_a: f64,
    pub end_pos: f64,
    pub dist_pt: f64,
    pub dir_deg: f64,
    pub sy: f64,
    pub algn: Option<Sym>,
    pub rot_with_shape: bool,
}

/// `PictureLook.clrChange`.
#[derive(Clone, Copy, Debug)]
pub struct ClrChange {
    pub from: Sym,
    pub to: Sym,
    pub to_alpha: f64,
}

/// `ShapeItem`, with the shape **spec** instead of the docx-core `ShapeStyle` (RQ1): each painter
/// evaluates the geometry in its own unit through [`crate::geom`].
#[derive(Clone, Debug)]
pub struct ShapeItem {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub geom: ShapeGeom,
    pub fill: Option<Sym>,
    pub stroke: Option<Stroke>,
    /// `line` (geom `line`): points relative to the corner.
    pub line: Option<[f64; 4]>,
    pub grad: Option<GradFill>,
    pub shadow: Option<ShapeShadow>,
    pub pattern: Option<Pattern>,
    pub blip: Option<Blip>,
    pub fx: Option<ShapeFx>,
}

/// What the painters read of `ShapeItem.style` (canvas.ts:427-451, pdf.ts:370-400, the shape
/// effects): the preset or `custom`, its adjust values, the custom paths, flips, rotation and the
/// two alphas.
#[derive(Clone, Debug)]
pub struct ShapeGeom {
    /// `style.geom` (`custom` for `a:custGeom`).
    pub geom: Sym,
    /// `style.adj` in the TS key order.
    pub adj: Option<Vec<(Sym, f64)>>,
    /// `style.cust`.
    pub cust: Option<CustId>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub rot: OptF64,
    pub fill_alpha: OptF64,
    /// `style.line?.alpha`
    pub line_alpha: OptF64,
    pub eval: GeomEval,
}

/// Which preset evaluator a shape uses (RQ1): Write's compact subset (`presetPathLocal`, arcs as
/// cubic Béziers) or uo-drawingml's 187 presets (Slides, later).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GeomEval {
    #[default]
    Compact,
    Drawingml,
}

/// `GradFill`.
#[derive(Clone, Debug)]
pub struct GradFill {
    pub stops: Vec<GradStop>,
    pub angle_deg: f64,
    pub scaled: bool,
    pub rot_with_shape: bool,
    pub path: Option<Sym>,
    pub fill_to: Option<Ltrb>,
}

/// `{ pos, color }`.
#[derive(Clone, Copy, Debug)]
pub struct GradStop {
    pub pos: f64,
    pub color: Sym,
}

/// `ShapeItem.shadow` (offset in twips).
#[derive(Clone, Copy, Debug)]
pub struct ShapeShadow {
    pub dx: f64,
    pub dy: f64,
    pub color: Sym,
    pub alpha: OptF64,
    pub blur: OptF64,
}

/// `ShapeItem.pattern`.
#[derive(Clone, Copy, Debug)]
pub struct Pattern {
    pub prst: Sym,
    pub fg: Sym,
    pub bg: Sym,
}

/// `ShapeItem.blip`.
#[derive(Clone, Copy, Debug)]
pub struct Blip {
    pub image: ImageId,
    pub tile: Option<BlipTile>,
    pub alpha: OptF64,
}

/// `a:tile` (EMU and 1/1000 %).
#[derive(Clone, Copy, Debug)]
pub struct BlipTile {
    pub tx: f64,
    pub ty: f64,
    pub sx: f64,
    pub sy: f64,
    pub algn: Sym,
    pub flip: Sym,
}

/// `ShapeItem.fx` (`Pick<PictureLook, 'shadow' | 'glow' | 'softEdgePt' | 'reflection'>`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeFx {
    pub shadow: Option<PicShadow>,
    pub glow: Option<PicGlow>,
    pub soft_edge_pt: OptF64,
    pub reflection: Option<PicReflection>,
}

/// One `a:custGeom` path (docx-core `CustPath`), in its own `w x h` space.
///
/// The record asks for one type in `uo_drawingml` shared with the uo-docx model (uo-render §12);
/// until it lands there this is the definition, and it becomes a re-export.
#[derive(Clone, Debug)]
pub struct CustPath {
    pub w: f64,
    pub h: f64,
    pub fill: bool,
    pub stroke: bool,
    pub cmds: Vec<CustCmd>,
}

/// docx-core `CustCmd`.
#[derive(Clone, Debug)]
pub enum CustCmd {
    M { x: f64, y: f64 },
    L { x: f64, y: f64 },
    C { pts: Vec<[f64; 2]> },
    Q { pts: Vec<[f64; 2]> },
    A { w_r: f64, h_r: f64, st: f64, sw: f64 },
    Z,
}

/// `ResolvedFill` (textfx.ts:14).
#[derive(Clone, Debug)]
pub enum ResolvedFill {
    None,
    Solid { hex: Sym, alpha: f64 },
    Grad { stops: Vec<FxStop>, angle_deg: f64, scaled: bool, path: Option<Sym>, fill_to: Option<Ltrb> },
}

/// A gradient stop of a text fill.
#[derive(Clone, Copy, Debug)]
pub struct FxStop {
    pub pos: f64,
    pub hex: Sym,
    pub alpha: f64,
}

/// Canvas `lineCap` of a text outline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// Canvas `lineJoin` of a text outline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// `ResolvedTextFx` (textfx.ts:19).
#[derive(Clone, Debug, Default)]
pub struct ResolvedTextFx {
    pub fill: Option<ResolvedFill>,
    pub outline: Option<FxOutline>,
    pub shadow: Option<FxShadow>,
    pub glow: Option<FxGlow>,
    pub reflection: Option<FxReflection>,
}

/// `ResolvedTextFx.outline`.
#[derive(Clone, Debug)]
pub struct FxOutline {
    pub width_pt: f64,
    pub fill: ResolvedFill,
    pub dash: Option<Sym>,
    pub cap: LineCap,
    pub join: LineJoin,
}

/// `ResolvedTextFx.shadow`.
#[derive(Clone, Copy, Debug)]
pub struct FxShadow {
    pub blur_pt: f64,
    pub dist_pt: f64,
    pub dir_deg: f64,
    pub sx: f64,
    pub sy: f64,
    pub kx_deg: f64,
    pub ky_deg: f64,
    pub algn: Sym,
    pub hex: Sym,
    pub alpha: f64,
}

/// `ResolvedTextFx.glow`.
#[derive(Clone, Copy, Debug)]
pub struct FxGlow {
    pub rad_pt: f64,
    pub hex: Sym,
    pub alpha: f64,
}

/// `ResolvedTextFx.reflection`.
#[derive(Clone, Copy, Debug)]
pub struct FxReflection {
    pub blur_pt: f64,
    pub st_a: f64,
    pub st_pos: f64,
    pub end_a: f64,
    pub end_pos: f64,
    pub dist_pt: f64,
    pub dir_deg: f64,
    pub algn: Sym,
}

/// `PageFillPaint.type`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageFillKind {
    Gradient,
    GradientRadial,
    Pattern,
    Frame,
    Tile,
}

/// `PageFillPaint` (page-fill.ts:44): the page background effect, colours resolved.
#[derive(Clone, Debug)]
pub struct PageFill {
    pub kind: PageFillKind,
    pub c1: Sym,
    pub c2: Sym,
    pub stops: Option<Vec<GradStop>>,
    pub angle: f64,
    pub focus: f64,
    pub focus_position: Option<[f64; 2]>,
    pub gradient_center: bool,
    /// the effect's image (pattern: the GIF; picture/texture: the file) — a derived image (K12).
    pub image: Option<ImageId>,
    /// `imagePx`: pixel size and DPI.
    pub image_px: Option<[f64; 3]>,
    /// `pattern`: 8 rows (bit 7 = column 0), the TS numbers.
    pub pattern: Option<Vec<f64>>,
}
