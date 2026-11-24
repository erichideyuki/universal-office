//! `spill-border.ts`, `paintTableHandle` (`table-styles.ts`) and the pure helpers of `grid-layout.ts` that C3a's
//! geometry does not hold: hit tests with frozen panes, the header resize grip, the inverse of the column-width
//! formula. (`bisectLE` is [`Geometry::bisect_le`]; the merge index is [`crate::grid::MergeIndex`].)
//!
//! Geometry convention (the TS one): `edges[i]` = the accumulated left/top edge of column/row `i + 1`; the first
//! `frozen_count` columns/rows stay on screen (no scroll), the rest shift by `scroll`, cut at `edges[frozen_count]`.

use crate::grid::geometry::Geometry;
use uo_jsnum::math;

/// `SPILL_BORDER_COLOR`: the Office highlight blue (not measured: the border is interface, not part of the oracle's
/// camera image).
pub const SPILL_BORDER_COLOR: &str = "#4472C4";

/// The colour of the table's resize handle (measured: a 4x4 px step).
pub const TABLE_HANDLE_COLOR: &str = "#485DB1";

/// `paintSpillBorder`: the `strokeRect` rectangle `[x, y, w, h]` (1 px line, so +0.5 and w/h - 1) of the outline of the
/// rectangle (x1, y1)-(x2, y2) in screen px; `None` when it has no area.
#[must_use]
pub fn spill_border_rect(x1: f64, y1: f64, x2: f64, y2: f64) -> Option<[f64; 4]> {
    let (rx1, ry1) = (math::round(x1), math::round(y1));
    let (w, h) = (math::round(x2) - rx1, math::round(y2) - ry1);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some([rx1 + 0.5, ry1 + 0.5, w - 1.0, h - 1.0])
}

/// `paintTableHandle`: the two `fillRect`s of the resize handle at the bottom-right corner (a 4x4 step: 2x2 at the
/// top right + 4x2 below), `[x, y, w, h]` each, in [`TABLE_HANDLE_COLOR`].
#[must_use]
pub fn table_handle_rects(right: f64, bottom: f64) -> [[f64; 4]; 2] {
    let (r, b) = (math::round(right), math::round(bottom));
    [[r - 2.0, b - 4.0, 2.0, 2.0], [r - 4.0, b - 2.0, 4.0, 2.0]]
}

/// `frozenExtent`: the extent of the frozen region = the edge after the last frozen entry.
#[must_use]
pub fn frozen_extent(edges: &Geometry, frozen_count: u32) -> f64 {
    if frozen_count > 0 { edges.edge0(frozen_count) } else { 0.0 }
}

/// `pxToIndex`: the 1-based index of the column/row at the content coordinate `content_px` (screen px without the
/// header), the frozen region not scrolling; `None` outside `1..=max_index` or before the origin.
#[must_use]
pub fn px_to_index(edges: &Geometry, content_px: f64, frozen_count: u32, scroll: f64, max_index: u32) -> Option<u32> {
    if content_px < 0.0 {
        return None;
    }
    let fpx = frozen_extent(edges, frozen_count);
    let x = if content_px < fpx { content_px } else { content_px + scroll };
    let idx = edges.bisect_le(x).checked_add(1)?;
    (idx >= 1 && idx <= max_index).then_some(idx)
}

/// `indexScreenPos`: the screen position (content coordinates) of the left/top edge of the 1-based `index`.
#[must_use]
pub fn index_screen_pos(edges: &Geometry, index: u32, frozen_count: u32, scroll: f64) -> f64 {
    let pos = edges.edge0(index.wrapping_sub(1));
    if index <= frozen_count { pos } else { pos - scroll }
}

/// `boundaryAt`: the 1-based index whose END edge is within `tol` px of `content_px` (the header's resize grip). With
/// coincident edges (hidden columns) the highest index wins; on the freeze line the frozen entry is preferred.
#[must_use]
pub fn boundary_at(edges: &Geometry, content_px: f64, frozen_count: u32, scroll: f64, max_index: u32, tol: f64) -> Option<u32> {
    if content_px < 0.0 {
        return None;
    }
    let fpx = frozen_extent(edges, frozen_count);
    if frozen_count > 0 && content_px <= fpx + tol {
        let k = edges.bisect_le(content_px + tol);
        if k >= 1 && k <= frozen_count.min(max_index) && math::abs(edges.edge0(k) - content_px) <= tol {
            return Some(k);
        }
    }
    if content_px >= fpx || frozen_count == 0 {
        let x = content_px + scroll;
        let k = edges.bisect_le(x + tol);
        if k >= 1 && k <= max_index && k > frozen_count && math::abs(edges.edge0(k) - x) <= tol {
            return Some(k);
        }
    }
    None
}

/// `pxToColWidthChars(px, mdw)`: the smallest column width (character units, step 1/256) whose pixel width is `px`.
#[must_use]
pub fn px_to_col_width_chars(px: f64, mdw: f64) -> f64 {
    if px <= 0.0 {
        return 0.0;
    }
    let u = math::ceil((256.0 * px) / mdw - math::floor(128.0 / mdw) - 1e-9);
    math::max(0.0, u) / 256.0
}

/// `pxToRowHeightPt`: px to points of row height (`round(px * 0.75 * 100) / 100`).
#[must_use]
pub fn px_to_row_height_pt(px: f64) -> f64 {
    math::round(px * 0.75 * 100.0) / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(n: u32) -> Geometry {
        Geometry::build(n, 64.0, 1.0, &[])
    }

    #[test]
    fn hit_tests_with_frozen_panes() {
        let g = geo(100);
        assert_eq!(px_to_index(&g, 0.0, 0, 0.0, 100), Some(1));
        assert_eq!(px_to_index(&g, 63.9, 0, 0.0, 100), Some(1));
        assert_eq!(px_to_index(&g, 64.0, 0, 0.0, 100), Some(2));
        assert_eq!(px_to_index(&g, -1.0, 0, 0.0, 100), None);
        // two frozen columns: the first 128 px do not scroll
        assert_eq!(px_to_index(&g, 100.0, 2, 640.0, 100), Some(2));
        assert_eq!(px_to_index(&g, 130.0, 2, 640.0, 100), Some(13));
        assert_eq!(index_screen_pos(&g, 2, 2, 640.0), 64.0);
        assert_eq!(index_screen_pos(&g, 13, 2, 640.0), 12.0 * 64.0 - 640.0);
        assert_eq!(index_screen_pos(&g, 0, 0, 0.0), 0.0);
    }

    #[test]
    fn grips_and_inverses() {
        let g = geo(100);
        assert_eq!(boundary_at(&g, 129.0, 0, 0.0, 100, 3.0), Some(2));
        assert_eq!(boundary_at(&g, 120.0, 0, 0.0, 100, 3.0), None);
        assert_eq!(boundary_at(&g, 127.0, 2, 640.0, 100, 3.0), Some(2));
        assert_eq!(px_to_col_width_chars(64.0, 7.0), (256.0f64 * 64.0 / 7.0 - 18.0 - 1e-9).ceil() / 256.0);
        assert_eq!(px_to_col_width_chars(0.0, 7.0), 0.0);
        assert_eq!(px_to_row_height_pt(20.0), 15.0);
    }

    #[test]
    fn spill_and_handle() {
        assert_eq!(spill_border_rect(10.4, 20.6, 50.2, 40.5), Some([10.5, 21.5, 39.0, 19.0]));
        assert_eq!(spill_border_rect(10.0, 10.0, 10.0, 20.0), None);
        assert_eq!(table_handle_rects(100.4, 50.5), [[98.0, 47.0, 2.0, 2.0], [96.0, 49.0, 4.0, 2.0]]);
    }
}
