//! The paint rules of the grid (frontier C3c): colours, border edges and their conflicts (`borders.ts`), fills
//! (`fills.ts`), the spill border (`spill-border.ts`) and the helpers of `grid-layout.ts` the geometry of C3a does
//! not hold (hit tests with frozen panes, the rich-text line splitter, the pattern masks).
//!
//! Everything here is plain `Copy` data and pure functions: no canvas, no allocation per cell. The viewport emission
//! (C4) turns [`border::PaintRect`]s, [`fill::FillPaint`]s and [`misc`] rectangles into display-list items.
//!
//! | Module | TS | What |
//! |---|---|---|
//! | [`border`] | `borders.ts` | styles and ranks, the winning edge of a boundary, corners, the 1-px runs of a segment, diagonals |
//! | [`fill`] | `fills.ts`, `PATTERN_MASKS` | fills as paint data: solid, 8x8 pattern, linear/path gradient, `hasFill` |
//! | [`misc`] | `spill-border.ts`, `paintTableHandle`, `grid-layout.ts` | spill rectangle, table handle, `pxToIndex`/`boundaryAt`/… |
//! | [`rich`] | `layoutRichLines` | word wrap of rich-text runs |

pub mod border;
pub mod fill;
pub mod misc;
pub mod rich;

pub use border::{
    BStyle, CornerInfo, DiagonalStrokes, Edge, NO_CORNER, PaintRect, corner, diagonal_strokes, edge_extent, emit_h_edge, emit_v_edge, rank,
    winning_edge,
};
pub use fill::{
    FillPaint, FillRef, PATH_GRADIENT_MAX_PIXELS, PaintFill, PatternMask, Stop, clamp_stop, fill_paint, has_fill_ref, path_gradient_rgba,
    pattern_masks, pattern_rows,
};
pub use misc::{
    SPILL_BORDER_COLOR, TABLE_HANDLE_COLOR, boundary_at, frozen_extent, index_screen_pos, px_to_col_width_chars, px_to_index,
    px_to_row_height_pt, spill_border_rect, table_handle_rects,
};
pub use rich::{RichSeg, layout_rich_lines};

use std::collections::HashMap;

/// A colour as the TS holds it (a CSS string): `0x00RRGGBB` for the exact `#rrggbb` form (any case: the case is not
/// observable on a canvas and [`Palette`] compares as the TS `sameColor` does), or `0x8000_0000 | n` for any other
/// text (`#abc`, `#NaNNaNNaN`, `#red`…), whose lower-cased text lives in the [`Palette`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Color(pub u32);

impl Color {
    /// `#000000`.
    pub const BLACK: Color = Color(0);
    /// `#ffffff`.
    pub const WHITE: Color = Color(0x00ff_ffff);
    const RAW: u32 = 0x8000_0000;

    /// The packed colour of the exact `#rrggbb` form.
    #[must_use]
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Color {
        Color((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    }

    /// The channels of a `#rrggbb` colour (`None` for a text kept in the palette).
    #[must_use]
    pub fn rgb(self) -> Option<[u8; 3]> {
        (self.0 & Self::RAW == 0).then_some([(self.0 >> 16 & 255) as u8, (self.0 >> 8 & 255) as u8, (self.0 & 255) as u8])
    }

    /// The colour is a text kept in the palette (anything but `#rrggbb`).
    #[must_use]
    pub fn is_raw(self) -> bool {
        self.0 & Self::RAW != 0
    }
}

/// The interned colours of a workbook: only the texts that are not `#rrggbb` take a slot (built with the styles, the
/// dxfs and the table-style specs; a frame never adds to it).
#[derive(Clone, Debug, Default)]
pub struct Palette {
    raw: Vec<Box<str>>,
    map: HashMap<Box<str>, u32>,
}

fn hex_pair(b: &[u8]) -> Option<u8> {
    let d = |c: u8| char::from(c).to_digit(16);
    Some((d(*b.first()?)? * 16 + d(*b.get(1)?)?) as u8)
}

impl Palette {
    /// The colour of a TS colour string.
    pub fn intern(&mut self, s: &str) -> Color {
        let b = s.as_bytes();
        if b.len() == 7
            && b[0] == b'#'
            && let (Some(r), Some(g), Some(bl)) = (hex_pair(&b[1..3]), hex_pair(&b[3..5]), hex_pair(&b[5..7]))
        {
            return Color::from_rgb(r, g, bl);
        }
        let lower = s.to_lowercase();
        if let Some(&i) = self.map.get(lower.as_str()) {
            return Color(Color::RAW | i);
        }
        let i = u32::try_from(self.raw.len()).unwrap_or(u32::MAX >> 1) & !Color::RAW;
        let key: Box<str> = lower.into();
        self.raw.push(key.clone());
        self.map.insert(key, i);
        Color(Color::RAW | i)
    }

    /// The lower-cased text of a colour (`#rrggbb` for a packed one).
    #[must_use]
    pub fn text(&self, c: Color) -> String {
        match c.rgb() {
            Some([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
            None => self.raw.get((c.0 & !Color::RAW) as usize).map(|s| s.to_string()).unwrap_or_default(),
        }
    }

    /// `hexRgb` of `fills.ts` (`/^#?([0-9a-f]{6})/i`; black when it does not match).
    #[must_use]
    pub fn hex_rgb(&self, c: Color) -> [u8; 3] {
        if let Some(rgb) = c.rgb() {
            return rgb;
        }
        let t = self.text(c);
        let b = t.strip_prefix('#').unwrap_or(&t).as_bytes();
        match (b.get(0..2).and_then(hex_pair), b.get(2..4).and_then(hex_pair), b.get(4..6).and_then(hex_pair)) {
            (Some(r), Some(g), Some(bl)) => [r, g, bl],
            _ => [0, 0, 0],
        }
    }

    /// Slots in use.
    #[must_use]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// No raw colour.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.raw.iter().map(|s| s.len() * 2 + 48).sum()
    }
}
