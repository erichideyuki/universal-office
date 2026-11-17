//! SmartArt regeneration and fallback (frontier B5a): `diagram-layout.ts` (the layout engine for
//! diagrams WITHOUT a cached drawing), `diagram-fallback.ts` (the approximate layout of the common layouts
//! by the data model alone) and `diagramStyler` of `read.ts`. See `core/docs/modules/uo-dml-render.md`
//! "B5a — SmartArt".

mod data;
mod emit;
mod engine;
mod fallback;
pub mod metrics;
mod metrics_data;
mod styler;
pub mod xt;

pub use emit::{DiagramInput, layout_diagram, supported};
pub use fallback::diagram_fallback_shapes;
pub use metrics::{ApproxMeasure, DiagramFont, DiagramMeasure};
pub use styler::{DgmStyle, diagram_styler};

use crate::theme::{Theme, color_from_dml};
use xt::X;

/// `colorFromDml` of a [`X`] element (the engines work on owned trees; the colour function reads a
/// parsed element: the element goes through its text form).
pub fn color_of_x(el: &X, theme: &Theme) -> Option<crate::CssColor> {
    let doc = crate::source::parse_xml(xt::to_xml(el)).ok()?;
    color_from_dml(Some(doc.root()), theme)
}

/// `regenerateDiagram` (`diagram-layout.ts:1488`): redoes the drawing of a SmartArt without a cached
/// drawing: the parts already read, the frame's size and the theme (colours of the `span` lists, the body
/// font to measure). `None` when nothing can be regenerated (the caller falls back to the plan B).
pub fn regenerate_diagram(
    data: Option<&X>,
    layout: Option<&X>,
    style: Option<&X>,
    colors: Option<&X>,
    cx: f64,
    cy: f64,
    theme: &Theme,
) -> Option<X> {
    let (data, layout) = (data?, layout?);
    if cx.is_nan() || cx <= 0.0 || cy.is_nan() || cy <= 0.0 {
        return None;
    }
    // only layouts whose algorithms the engine implements (the rest stays with plan B)
    if !supported(layout, 0) {
        return None;
    }
    let resolve = |e: &X| color_of_x(e, theme).map(|c| c.as_str().to_string());
    let minor = theme.fonts.as_ref().and_then(|f| f.minor.as_deref());
    layout_diagram(&DiagramInput { data, layout, style, colors, cx, cy, resolve_color: Some(&resolve), measure: None, minor_font: minor })
}
