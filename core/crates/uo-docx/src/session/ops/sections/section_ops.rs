//! docx-core `section-ops.ts` (D2b-F4): section ranges, columns and orientation, with the rules Word
//! follows (measured in `write/tools/sections-lab`):
//!
//! - section `k` runs from the paragraph after break `k-1` to the paragraph carrying break `k`; the
//!   last ends at the end (its `sectPr` is the body's);
//! - a new break SPLITS the paragraph; the first half carries the new `sectPr` = a full copy of the
//!   section (the old type included) with the header/footer references, which LEAVE the section
//!   after it (it becomes linked to the previous); the break's type goes to the section AFTER;
//! - columns: default spacing 708 tw, minimum width 720 tw (the spacing shrinks);
//! - orientation rotates the margins (portrait to landscape: T'=L, R'=T, B'=R, L'=B).
//!
//! The functions that change the paragraph list (`insertSectionBreak`, `insertPageBreakParagraph`)
//! run in the session (`sections.rs`), which owns the guarded split they take as `opts.split`.

use uo_jsnum::math::{floor, max, min, round};

use crate::js::JsNumber;
use crate::model::para::Paragraph;
use crate::model::section::{ColWidth, Cols, SectPr};
use crate::pvec::PVec;

/// Default spacing between columns of Word pt-BR (1.25 cm).
pub const COLUMN_SPACE_DEFAULT: f64 = 708.0;
/// Minimum column width (1.27 cm).
pub const COLUMN_MIN_WIDTH: f64 = 720.0;

/// `x ?? d` over a model number (`null` is nullish too).
fn nn(x: Option<JsNumber>, d: f64) -> f64 {
    x.and_then(JsNumber::as_f64).unwrap_or(d)
}

/// One section of the body paragraphs (`SectionRange`): the first and last paragraph and the break
/// paragraph that ends it (`None` for the last section).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionRange {
    pub start: usize,
    pub end: usize,
    /// the `end` paragraph carries a section break (the last section has none)
    pub has_break: bool,
}

/// `sectionRanges(model)`.
#[must_use]
pub fn section_ranges(paras: &PVec<Paragraph>) -> Vec<SectionRange> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, p) in paras.iter().enumerate() {
        if p.props.section_break.is_none() {
            continue;
        }
        out.push(SectionRange { start, end: i, has_break: true });
        start = i + 1;
    }
    out.push(SectionRange { start, end: start.max(paras.len().saturating_sub(1)), has_break: false });
    out
}

/// `sectionIndexAt(model, para)`: the section of a paragraph (`para` past the end is the last).
#[must_use]
pub fn section_index_at(paras: &PVec<Paragraph>, para: usize) -> usize {
    let rs = section_ranges(paras);
    rs.iter().position(|r| para <= r.end).unwrap_or(rs.len() - 1)
}

/// The width between the margins (`contentWidth`).
#[must_use]
pub fn content_width(sp: &SectPr) -> f64 {
    sp.page_width_twips.to_number() - sp.margin_left_twips.to_number() - sp.margin_right_twips.to_number()
}

/// The most columns that fit (`maxColumns`: minimum width 720 tw, no spacing).
#[must_use]
pub fn max_columns(sp: &SectPr) -> f64 {
    max(1.0, floor(content_width(sp) / COLUMN_MIN_WIDTH))
}

/// `ColumnSpec`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnSpec {
    pub count: f64,
    /// spacing between equal columns (twips; default 708)
    pub space_twips: Option<f64>,
    /// own widths (`w:equalWidth="0"`): width and the space after each column
    pub widths: Option<Vec<(f64, f64)>>,
    /// the line between columns
    pub sep: bool,
}

/// `columnsFor(sp, spec)`: the `w:cols` of a specification, fitted as Word does: equal columns that
/// do not fit with the requested spacing get 720 tw and the spacing shrinks to
/// `floor((W - n*720)/(n-1))`; more columns than `floor(W/720)` do not fit. One column = no
/// `w:cols`.
#[must_use]
pub fn columns_for(sp: &SectPr, spec: &ColumnSpec) -> Option<Cols> {
    let w = content_width(sp);
    let n = max(1.0, min(round(spec.count), max_columns(sp)));
    if n < 2.0 {
        return None;
    }
    let cur_space = sp.cols.as_ref().and_then(|c| c.space_twips.as_f64());
    let mut space = max(0.0, round(spec.space_twips.or(cur_space).unwrap_or(COLUMN_SPACE_DEFAULT)));
    if (w - space * (n - 1.0)) / n < COLUMN_MIN_WIDTH {
        space = max(0.0, floor((w - n * COLUMN_MIN_WIDTH) / (n - 1.0)));
    }
    let mut cols = Cols { count: JsNumber::Num(n), space_twips: JsNumber::Num(space), sep: spec.sep, ..Cols::default() };
    if let Some(ws) = &spec.widths
        && ws.len() as f64 >= n
    {
        let mut out = Vec::new();
        for (i, (width, sp_after)) in ws.iter().enumerate() {
            if i as f64 >= n {
                break;
            }
            out.push(ColWidth {
                width_twips: JsNumber::Num(max(COLUMN_MIN_WIDTH, round(*width))),
                // Word writes the `w:space` of the last column with the general spacing
                space_twips: JsNumber::Num(if i as f64 == n - 1.0 { space } else { max(0.0, round(*sp_after)) }),
                ..ColWidth::default()
            });
        }
        crate::word::obj::set(&mut cols, "widths", |c| c.widths = Some(out));
    }
    Some(cols)
}

/// A preset of the Columns gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnPreset {
    One,
    Two,
    Three,
    Left,
    Right,
}

/// `presetColumns(sp, preset)`: the specification of a preset (keeps the section's line and spacing).
#[must_use]
pub fn preset_columns(sp: &SectPr, preset: ColumnPreset) -> ColumnSpec {
    let sep = sp.cols.as_ref().is_some_and(|c| c.sep);
    let space = nn(sp.cols.as_ref().map(|c| c.space_twips), COLUMN_SPACE_DEFAULT);
    match preset {
        ColumnPreset::One => ColumnSpec { count: 1.0, sep, ..ColumnSpec::default() },
        ColumnPreset::Two | ColumnPreset::Three => {
            ColumnSpec { count: if preset == ColumnPreset::Two { 2.0 } else { 3.0 }, space_twips: Some(space), sep, widths: None }
        }
        ColumnPreset::Left | ColumnPreset::Right => {
            // 3-column grid: the narrow one is one, the wide one two plus the gap
            let w = content_width(sp);
            let narrow = round((w - 2.0 * space) / 3.0);
            let wide = w - narrow - space;
            let ws = if preset == ColumnPreset::Left { [narrow, wide] } else { [wide, narrow] };
            ColumnSpec { count: 2.0, space_twips: Some(space), sep, widths: Some(ws.iter().map(|x| (*x, space)).collect()) }
        }
    }
}

/// `columnsPresetOf(sp)`: which preset the section's columns are (for the gallery); `None` = none.
#[must_use]
pub fn columns_preset_of(sp: &SectPr) -> Option<ColumnPreset> {
    let Some(c) = sp.cols.as_ref() else { return Some(ColumnPreset::One) };
    let count = c.count.to_number();
    if count < 2.0 {
        return Some(ColumnPreset::One);
    }
    let Some(widths) = c.widths.as_ref() else {
        return if count == 2.0 {
            Some(ColumnPreset::Two)
        } else if count == 3.0 {
            Some(ColumnPreset::Three)
        } else {
            None
        };
    };
    if count != 2.0 {
        return None;
    }
    for p in [ColumnPreset::Left, ColumnPreset::Right] {
        let spec = preset_columns(sp, p);
        let refw = spec.widths.unwrap_or_default();
        let ok = refw.iter().enumerate().all(|(i, w)| match widths.get(i) {
            Some(cw) => uo_jsnum::math::abs(w.0 - cw.width_twips.to_number()) <= 20.0,
            None => false,
        });
        if ok {
            return Some(p);
        }
    }
    None
}

/// `columnWidths(sp)`: the width of each column (equal or own) and the space after it.
#[must_use]
pub fn column_widths(sp: &SectPr) -> Vec<(f64, f64)> {
    let w = content_width(sp);
    let Some(c) = sp.cols.as_ref() else { return vec![(w, 0.0)] };
    let count = c.count.to_number();
    if count < 2.0 {
        return vec![(w, 0.0)];
    }
    if let Some(ws) = c.widths.as_ref() {
        return ws.iter().map(|x| (x.width_twips.to_number(), x.space_twips.to_number())).collect();
    }
    let space = c.space_twips.to_number();
    let each = round((w - space * (count - 1.0)) / count);
    let n =
        if count.is_finite() && count > 0.0 { usize::try_from(uo_jsnum::conv::to_uint32(min(count, 1_000_000.0))).unwrap_or(0) } else { 0 };
    (0..n).map(|i| (each, if (i as f64) < count - 1.0 { space } else { 0.0 })).collect()
}

/// Landscape or portrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// `setOrientation(sp, orientation)`: swaps the paper width/height and ROTATES the margins
/// (portrait to landscape: top <- left, right <- top, bottom <- right, left <- bottom; the way back
/// is the inverse).
pub fn set_orientation(sp: &mut SectPr, orientation: Orientation) {
    let land = sp.page_width_twips.to_number() > sp.page_height_twips.to_number();
    if (orientation == Orientation::Landscape) == land {
        return;
    }
    std::mem::swap(&mut sp.page_width_twips, &mut sp.page_height_twips);
    let (t, r, b, l) = (sp.margin_top_twips, sp.margin_right_twips, sp.margin_bottom_twips, sp.margin_left_twips);
    if orientation == Orientation::Landscape {
        sp.margin_top_twips = l;
        sp.margin_right_twips = t;
        sp.margin_bottom_twips = r;
        sp.margin_left_twips = b;
    } else {
        sp.margin_top_twips = r;
        sp.margin_right_twips = b;
        sp.margin_bottom_twips = l;
        sp.margin_left_twips = t;
    }
}
