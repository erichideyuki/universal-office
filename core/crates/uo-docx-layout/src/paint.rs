//! Painting lines, boxes and borders into display items (`paint.ts`, frontier F5): glyph runs
//! (merged across the same face object only, K2a), underlines and strikes, highlights and
//! shading, paragraph and table borders (`paintBorder`, `borderWidth`), revision bars, line
//! numbers. Emits into [`crate::items::PassItems`] with the `lineBox`/`lineNum` side data (RQ2)
//! and the per-call `lineSeq` ([`crate::keys::LineSeq`]).

use crate::engine::Cx;
use crate::error::{Frontier, LResult, unported};
use crate::items::PassItems;
use crate::lines::Line;
use crate::para::PreparedPara;

/// Where a line is painted: the frame's left edge, the line's top and the page context.
#[derive(Clone, Copy, Debug)]
pub struct LinePlacement {
    pub frame_x: f64,
    pub top: f64,
    /// the line is in a header/footer
    pub hf: bool,
}

/// `paintLine(…)` (paint.ts) — F5.
pub fn paint_line(_cx: &mut Cx<'_, '_>, _p: &PreparedPara, _line: &Line, _at: LinePlacement, _out: &mut PassItems) -> LResult<()> {
    unported(Frontier::F5, "paint::paint_line")
}
