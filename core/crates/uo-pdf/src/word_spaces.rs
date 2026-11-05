//! `pdf-word-spaces.ts` (`withWordSpaces`) and the `advanceTwips` it reads (`typography/font/sfnt.ts`
//! 386-403) with the optional advance table of the face (record LQ8(e), PQ1).
//!
//! The layout does not paint the spaces (it only places the words): without them the text
//! extracted from the PDF comes out glued ("conflitodeinteresses"). The painter writes the space
//! glyph where the distance to the next glyph on the same baseline is more than half a space of
//! the font, inside a run and between runs. PDF only; the layout does not change.

use uo_fonts::Face;
use uo_fonts::registry::AdvanceTable;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math;

/// The end (twips) of the last glyph written on the page and its baseline: the space between runs
/// of different styles (`WordSpaceCursor`).
#[derive(Clone, Copy, Debug, Default)]
pub struct WordSpaceCursor {
    /// `(y, xEnd)`
    pub last: Option<(f64, f64)>,
}

/// `SfntFont.advanceTwips(codePoint, sizePt)`: the table of the reference application first (key
/// `String(sizePt)` and the character), else the linear scale with the em rounded to the 600 dpi
/// pixel (11 pt -> 92/600" = 11.04 pt). `size_key` is `to_js_string(size_pt)` when a table is set.
fn advance_with_key(face: &Face, table: Option<&AdvanceTable>, size_key: &str, cp: u32, size_pt: f64) -> f64 {
    if let Some(t) = table.and_then(|t| t.get(size_key, cp)) {
        return t;
    }
    let units = f64::from(face.advance_width(face.glyph_id(cp)));
    let device_em_pt = (math::round((size_pt * 600.0) / 72.0) * 72.0) / 600.0;
    (units * device_em_pt * 20.0) / f64::from(face.metrics().units_per_em)
}

/// `font.advanceTwips(cp, sizePt)` with the face's advance table, if the host loaded one.
#[must_use]
pub fn advance_twips(face: &Face, table: Option<&AdvanceTable>, cp: u32, size_pt: f64) -> f64 {
    let key = if table.is_some() { to_js_string(size_pt) } else { String::new() };
    advance_with_key(face, table, &key, cp, size_pt)
}

/// `withWordSpaces(glyphs, font, sizePt, scaleX, y, cursor)`: the glyphs `(cp, x)` with a space glyph
/// (`cp` 32) inserted at the end of the previous glyph wherever the gap to the next one passes half
/// a space. `scale_x` is the run's `scaleX || 1`.
pub fn with_word_spaces(
    glyphs: &[(u32, f64)],
    face: &Face,
    table: Option<&AdvanceTable>,
    size_pt: f64,
    scale_x: f64,
    y: f64,
    cursor: &mut WordSpaceCursor,
) -> Vec<(u32, f64)> {
    let Some(&(last_cp, last_x)) = glyphs.last() else { return Vec::new() };
    let key = if table.is_some() { to_js_string(size_pt) } else { String::new() };
    let adv = |cp: u32| advance_with_key(face, table, &key, cp, size_pt) * scale_x;
    let end = (y, last_x + adv(last_cp));
    if face.glyph_id(32) == 0 {
        cursor.last = Some(end);
        return glyphs.to_vec();
    }
    let min = adv(32) * 0.5;
    let mut out = Vec::with_capacity(glyphs.len() + glyphs.len() / 4);
    // the advance of the previous glyph is the one just computed for it: a pure function, so it is
    // computed once per glyph
    let mut prev: Option<(u32, f64, f64)> = None;
    for &(cp, x) in glyphs {
        let prev_end = match prev {
            Some((_, px, padv)) => Some(px + padv),
            None => match cursor.last {
                Some((ly, lx_end)) if math::abs(ly - y) < 1.0 => Some(lx_end),
                _ => None,
            },
        };
        if let Some(pe) = prev_end
            && cp != 32
            && prev.is_none_or(|(pcp, _, _)| pcp != 32)
            && x - pe > min
        {
            out.push((32, pe));
        }
        out.push((cp, x));
        prev = Some((cp, x, adv(cp)));
    }
    cursor.last = Some(end);
    out
}
