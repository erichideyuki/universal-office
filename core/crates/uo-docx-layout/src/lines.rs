//! Line breaking (`lines.ts`, frontier F5; seed: `experiments/rust-spike/src/linebreak.rs`, 0
//! mismatches on 13,353 lines): Word's rules for tabs (relative to the frame's left edge; default
//! grid from 0; the hanging indent as an implicit stop), justification and its space compression,
//! hanging punctuation, hyphenation, line height by the spacing rule.

use crate::engine::Cx;
use crate::error::{Frontier, LResult, unported};
use crate::para::PreparedPara;

/// Which glyph a placed glyph draws (`PlacedGlyph.g`, a TS object that is a paragraph glyph, a
/// marker glyph or the automatic hyphen the breaker builds, `{ ...kg, kind: 'shy', … }`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphSrc {
    Para(u32),
    Marker(u32),
    /// the automatic hyphen after paragraph glyph `k`
    AutoHyphen(u32),
}

/// `PlacedGlyph` (lines.ts:25).
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub g: GlyphSrc,
    /// x relative to the frame's left edge
    pub x: f64,
    /// effective width (tabs, justification)
    pub w: f64,
    /// `leader` of a tab (the TabSpec leader name)
    pub leader: Option<crate::output::TabLeader>,
}

/// `'page' | 'column'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakKind {
    Page,
    Column,
}

/// `'break' | 'start'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GapTab {
    Break,
    Start,
}

/// `Line` (lines.ts:35).
#[derive(Clone, Debug)]
pub struct Line {
    /// `[start, end)` in `para.glyphs`
    pub start: u32,
    pub end: u32,
    pub placed: Vec<PlacedGlyph>,
    pub marker: Option<Vec<PlacedGlyph>>,
    pub width: f64,
    pub x_start: f64,
    pub asc: f64,
    pub desc: f64,
    pub height: f64,
    pub baseline: f64,
    pub break_after: Option<BreakKind>,
    pub clear: bool,
    pub last: bool,
    pub gap_tab: Option<GapTab>,
    pub spanned: bool,
    /// `hrFrame`: `{ left, right }`
    pub hr_frame: Option<(f64, f64)>,
}

/// `LineFrame` (lines.ts:66): the space of one line, relative to the frame (indents applied).
#[derive(Clone, Copy, Debug)]
pub struct LineFrame {
    pub left: f64,
    pub right: f64,
    pub gap: bool,
    /// `next`: the next gap of the same line (after a float)
    pub next: Option<(f64, f64)>,
}

/// A breaker position to go back to (`reset(m)`).
#[derive(Clone, Copy, Debug, Default)]
pub struct BreakMark {
    pub pos: u32,
    pub line_idx: u32,
    pub trailing_break: bool,
    pub hyphen_run: f64,
}

/// `LineBreaker` (lines.ts:140): breaks one prepared paragraph line by line, each line in the
/// frame the engine gives it (floats change the frame per line).
#[derive(Debug)]
pub struct LineBreaker {
    pub mark: BreakMark,
    /// `grid`: the document grid pitch of the section (docGrid lines)
    pub grid: Option<f64>,
}

impl LineBreaker {
    #[must_use]
    pub fn new(_p: &PreparedPara, grid: Option<f64>) -> LineBreaker {
        LineBreaker { mark: BreakMark::default(), grid }
    }

    /// `next(frame)` — F5.
    pub fn next(&mut self, _cx: &mut Cx<'_, '_>, _p: &PreparedPara, _frame: LineFrame) -> LResult<Option<Line>> {
        unported(Frontier::F5, "lines::LineBreaker::next")
    }

    /// `reset(mark)`.
    pub fn reset(&mut self, m: BreakMark) {
        self.mark = m;
    }
}

/// `lineBox(asc, desc, p, textNat?, grid?)` (lines.ts:107): height and baseline by the spacing
/// rule — F5.
pub fn line_box(_asc: f64, _desc: f64, _p: &PreparedPara, _text_nat: Option<f64>, _grid: Option<f64>) -> LResult<(f64, f64)> {
    unported(Frontier::F5, "lines::line_box")
}
