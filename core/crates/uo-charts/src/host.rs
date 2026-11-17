//! The host interface (`ChartHost` of parse.ts:13–56, plus the host halves of `RenderOptions`,
//! render.ts:28–55). The repository rule: **the host hands the parsed XML tree and its theme colour
//! resolver**; the engine never parses XML, never sees a clrMap and never decodes an image.
//!
//! - Slides (`uo-dml-render` `emit::chart`): the tree is the chart part parsed by `uo-opc` with the
//!   chart `CanonicalPrefixes` (the TS `canonicalNs`), the resolver is `uo_dml_render::theme::color_from_dml`
//!   over the chart's theme (`ChartRef.theme`, with the TS `FALLBACK` palette merged as
//!   chart-engine.ts:65), `fill_style` re-enters [`crate::model::Paint`] parsing over `theme.fillStyles`
//!   with `phClr`, `blip` maps `r:embed` through `ChartRef.images` to a media id, `file_format` is
//!   `ChartRef.format`.
//! - Sheet (track C): the tree is the chart part after `refreshChartData` (the Sheet rewrites the
//!   caches from the cells, then parses), the resolver is the crate's own `dml_color` over the workbook
//!   scheme (`dml-color.ts`, port in C-1), Office 2023 theme when the package has none.

use uo_opc::xml::Element;

pub use crate::model::BlipKey;
use crate::model::{ChartColor, Paint};

/// What the parser asks the resolver for: an element of the tree (`a:srgbClr`, `a:schemeClr`… with
/// modifiers, as found), or the one synthetic node the TS builds, `<a:schemeClr val="…"/>` with no
/// modifiers (parse.ts:721: accent1–6, tx1, bg1).
#[derive(Clone, Copy, Debug)]
pub enum ColorSource<'d> {
    Element(Element<'d>),
    Scheme(&'d str),
}

/// Who wrote the file (`ChartHost.fileFormat`, parse.ts:41): changes the meaning of absent elements
/// and of booleans without `val`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileFormat {
    /// Office 2007 (AppVersion ≤ 12)
    Mso2007,
    /// 2010 and later
    Modern,
}

/// Theme fonts (`+mj-lt`/`+mn-lt`).
#[derive(Clone, Copy, Debug, Default)]
pub struct HostFonts<'h> {
    pub major: Option<&'h str>,
    pub minor: Option<&'h str>,
}

/// A decoded chart image as the emit needs it (`ChartImage`, render.ts:63, minus the canvas source):
/// the host's media id for the display list, the pixel size (a measurement, `uo-dml-render` A12) and
/// the natural size in pt from the file's resolution (`imageDpi`), when known.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ChartImageInfo {
    pub media: u32,
    pub w: f64,
    pub h: f64,
    pub w_pt: Option<f64>,
    pub h_pt: Option<f64>,
}

/// The host. One object serves the parse and the emit (the emit calls [`ChartHost::fill_style`]; the
/// decoded images come through [`crate::measure::ChartMeasure::image`], because their size is a
/// measurement); every method is pure for a given chart (the parity replay answers them from a log).
pub trait ChartHost {
    /// A DrawingML colour → the CSS string (`#RRGGBB` as the host's own rules give it).
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor>;
    /// Theme fonts.
    fn fonts(&self) -> HostFonts<'_>;
    /// Whether [`ChartHost::fill_style`] is provided (the TS `host.fillStyle` presence).
    fn has_fill_style(&self) -> bool;
    /// `fillStyleLst[idx − 1]` of the theme with `phClr` = `ph_clr` (styles 17–32 and 41–48 paint with
    /// the 3rd).
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint>;
    /// The `<a:blip>` of an `a:blipFill` → an opaque key (`None` = picture fill ignored).
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey>;
    /// Who wrote the file (`None` = a host that does not know: the earlier behaviour).
    fn file_format(&self) -> Option<FileFormat>;
    /// The chart drawing (`c:userShapes` of the chartDrawing part), canonical prefixes.
    fn user_shapes(&self) -> Option<Element<'_>>;
    /// `a:lnStyleLst` widths in pt (empty = none: the PowerPoint rule).
    fn theme_line_widths(&self) -> &[f64];
}
