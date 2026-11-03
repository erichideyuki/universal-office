//! The values the document API takes and gives (`document.ts`).

use crate::color::PdfColor;

/// A clickable area of a page (`/Link` annotation), in points with the origin at the TOP of the
/// page. The target is `uri` (only http, https and mailto: anything else drops the link) or, with
/// no `uri`, a place in the document (`page_index`, top `dest_top_pt`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PdfLink {
    pub x_pt: f64,
    pub y_top_pt: f64,
    pub w_pt: f64,
    pub h_pt: f64,
    pub uri: Option<String>,
    /// the page of an internal destination; clamped to the pages of the document (the TS takes any
    /// number: a fraction or NaN, which it turns into `undefined 0 R`, is not supported)
    pub page_index: Option<i64>,
    pub dest_top_pt: Option<f64>,
}

/// An entry of the navigation panel (`/Outlines`): title, level (1 = root), page and top (pt).
#[derive(Debug, Clone, PartialEq)]
pub struct PdfOutlineItem {
    pub title: String,
    pub level: u32,
    pub page_index: i64,
    pub top_pt: f64,
}

/// A glyph already placed: the glyph id (= CID), the code point (for ToUnicode, registered by
/// [`crate::PageWriter::encode`]) and the position in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    pub gid: u32,
    pub code_point: u32,
    /// absolute x on the page, in points (origin at the top left)
    pub x_pt: f64,
    /// y of the baseline from the TOP of the page, in points
    pub baseline_from_top_pt: f64,
}

/// A path command (points, y from the TOP).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    M { x: f64, y: f64 },
    L { x: f64, y: f64 },
    C { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Z,
}

/// The stroke of a path or line; an empty `dash` is a solid line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke<'a> {
    pub color: PdfColor,
    pub width_pt: f64,
    pub dash: &'a [f64],
}

/// Options of [`crate::PageWriter::glyphs`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GlyphOptions {
    /// horizontal scale of the text matrix (default 1)
    pub scale_x: Option<f64>,
    /// fill and stroke (text rendering mode 2) with a line proportional to the size
    pub synth_bold: bool,
    /// the 0.21 skew of the text matrix
    pub synth_italic: bool,
    /// the Word style: one `Tm` and one `TJ` per same-baseline, increasing-x run
    pub runs: bool,
}

/// An embedded font (the TS `EmbeddedFont` handle): the document owns the font, the handle names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub(crate) u32);

impl FontId {
    /// The resource name used in the content stream, `F<index>`.
    pub fn resource_name(self) -> String {
        format!("F{}", self.0)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

/// An embedded image (XObject) ready to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PdfImage {
    pub(crate) index: u32,
    pub(crate) obj_num: u32,
}

impl PdfImage {
    /// The resource name used in the content stream, `Im<index>`.
    pub fn resource_name(self) -> String {
        format!("Im{}", self.index)
    }

    pub fn obj_num(self) -> u32 {
        self.obj_num
    }
}

/// An embedded shading (a `/Shading` resource) ready to paint inside a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PdfShading {
    pub(crate) index: u32,
    pub(crate) obj_num: u32,
}

impl PdfShading {
    /// The resource name used in the content stream, `Sh<index>`.
    pub fn resource_name(self) -> String {
        format!("Sh{}", self.index)
    }

    pub fn obj_num(self) -> u32 {
        self.obj_num
    }
}

/// The geometry of a shading, in points with y from the TOP of the page: axial from (x0, y0) to
/// (x1, y1), or radial with concentric circles centred on (x0, y0) of radius 0 to `r`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShadingGeom {
    Axial { x0: f64, y0: f64, x1: f64, y1: f64 },
    Radial { x0: f64, y0: f64, r: f64 },
}

/// A soft mask (8-bit alpha at any resolution; it covers the whole image).
#[derive(Debug, Clone, Copy)]
pub struct SoftMask<'a> {
    pub alpha: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// A baseline JPEG embedded as it is (`DCTDecode`).
#[derive(Debug, Clone, Copy)]
pub struct JpegImage<'a> {
    pub bytes: &'a [u8],
    pub width_px: u32,
    pub height_px: u32,
    /// 1 (gray), 3 (RGB) or 4 (CMYK); anything else is RGB (as the TS)
    pub components: u32,
    pub adobe_inverted: bool,
    /// `/Decode`: a line per component (brightness and contrast without decoding the JPEG)
    pub decode: &'a [f64],
    pub smask: Option<SoftMask<'a>>,
}

impl<'a> JpegImage<'a> {
    /// An RGB JPEG with no decode array or mask (the TS defaults).
    pub fn new(bytes: &'a [u8], width_px: u32, height_px: u32) -> Self {
        JpegImage { bytes, width_px, height_px, components: 3, adobe_inverted: false, decode: &[], smask: None }
    }
}
