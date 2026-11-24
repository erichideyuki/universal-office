//! Cell fills as Excel paints them (`fills.ts`, measured in `t-fills`): the background covers the WHOLE cell including
//! the grid lines of its four edges (between two filled cells the one below/right paints last); patterns are 8x8 tiles
//! anchored at the sheet origin (A1 = 0,0: the same pattern in neighbouring columns stays in phase), `fg` = the "on"
//! pixels over `bg` (no colours: black on WHITE); the linear gradient's angle holds in the unit square and stretches
//! with the cell (45 degrees runs corner to corner), sRGB interpolation; the `path` gradient is concentric rectangles
//! from the inner rectangle (left/right/top/bottom) to the cell's edge.
//!
//! The model holds a `Fill` per `fills`/dxf entry; the paint model is a [`FillRef`]: `Copy`, with the rare non-solid
//! fills in a table ([`PaintFill`]) so a frame resolves a cell's fill without allocating.

use super::{Color, Palette};
use crate::read::model::Fill;
use uo_jsnum::math;

/// The 17 legacy pattern masks of `PATTERN_MASKS` (`grid-layout.ts`): 8 rows, the most significant bit = x 0.
const MASKS: [(&str, [u8; 8]); 17] = [
    ("gray0625", [0x80, 0x00, 0x08, 0x00, 0x80, 0x00, 0x08, 0x00]),
    ("gray125", [0x88, 0x00, 0x22, 0x00, 0x88, 0x00, 0x22, 0x00]),
    ("lightGray", [0x88, 0x22, 0x88, 0x22, 0x88, 0x22, 0x88, 0x22]),
    ("mediumGray", [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55]),
    ("darkGray", [0xee, 0xbb, 0xee, 0xbb, 0xee, 0xbb, 0xee, 0xbb]),
    ("darkHorizontal", [0xff, 0xff, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00]),
    ("lightHorizontal", [0xff, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00]),
    ("darkVertical", [0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc]),
    ("lightVertical", [0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88]),
    ("darkDown", [0xcc, 0x66, 0x33, 0x99, 0xcc, 0x66, 0x33, 0x99]),
    ("lightDown", [0x88, 0x44, 0x22, 0x11, 0x88, 0x44, 0x22, 0x11]),
    ("darkUp", [0x33, 0x66, 0xcc, 0x99, 0x33, 0x66, 0xcc, 0x99]),
    ("lightUp", [0x11, 0x22, 0x44, 0x88, 0x11, 0x22, 0x44, 0x88]),
    ("darkGrid", [0x99, 0x66, 0x66, 0x99, 0x99, 0x66, 0x66, 0x99]),
    ("lightGrid", [0xff, 0x88, 0x88, 0x88, 0xff, 0x88, 0x88, 0x88]),
    ("darkTrellis", [0xff, 0x66, 0xff, 0x99, 0xff, 0x66, 0xff, 0x99]),
    ("lightTrellis", [0x88, 0x55, 0x22, 0x55, 0x88, 0x55, 0x22, 0x55]),
];

/// A pattern mask: 8 rows of 8 pixels (bit 7 = x 0).
pub type PatternMask = [u8; 8];

/// The mask of a legacy pattern name (`PATTERN_MASKS[name]`), `None` for a name the TS has no mask for (such a
/// pattern paints its `bg`).
#[must_use]
pub fn pattern_rows(name: &str) -> Option<&'static PatternMask> {
    MASKS.iter().find(|(n, _)| *n == name).map(|(_, m)| m)
}

/// All the masks (name, rows), in the TS order.
#[must_use]
pub fn pattern_masks() -> &'static [(&'static str, PatternMask)] {
    &MASKS
}

/// A gradient stop.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stop {
    /// `position`.
    pub pos: f64,
    /// Colour.
    pub color: Color,
    /// The channels `hexRgb` reads from the colour text (the path gradient interpolates them).
    pub rgb: [u8; 3],
}

/// A non-solid fill.
#[derive(Clone, PartialEq, Debug)]
pub enum PaintFill {
    /// A legacy pattern: `rows` = its mask (`None` = a name without a mask: paints `bg`).
    Pattern {
        /// The 8x8 mask.
        rows: Option<PatternMask>,
        /// "On" pixels.
        fg: Color,
        /// Background.
        bg: Color,
    },
    /// A linear gradient of at least two stops.
    Linear {
        /// `degree`.
        degree: f64,
        /// Stops in file order.
        stops: Box<[Stop]>,
    },
    /// A `path` gradient of at least two stops (sorted by position, stable).
    Path {
        /// Inner rectangle fractions: left.
        left: f64,
        /// Right.
        right: f64,
        /// Top.
        top: f64,
        /// Bottom.
        bottom: f64,
        /// Stops sorted by position.
        stops: Box<[Stop]>,
    },
}

/// A resolved fill: nothing visible, a solid colour, or an entry of the book's table.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FillRef {
    /// No visible fill (`hasFill` false).
    #[default]
    None,
    /// A solid colour.
    Solid(Color),
    /// A pattern or gradient: index into [`crate::grid::paint::PaintBook::fill`].
    Id(u32),
}

/// `hasFill` of a resolved fill.
#[must_use]
pub fn has_fill_ref(f: FillRef) -> bool {
    f != FillRef::None
}

impl FillRef {
    /// Resolves a model `Fill` (`hasFill` + the kind `paintCellFill` draws) into the book's table. A fill that paints
    /// nothing is [`FillRef::None`]. A gradient with a single stop is that stop's colour.
    pub fn resolve(fill: &Fill, pal: &mut Palette, arena: &mut Vec<PaintFill>) -> FillRef {
        if let Some(g) = &fill.gradient
            && !g.stops.is_empty()
        {
            let stops: Vec<Stop> = g
                .stops
                .iter()
                .map(|s| {
                    let color = pal.intern(&s.color);
                    Stop { pos: s.position, color, rgb: pal.hex_rgb(color) }
                })
                .collect();
            if stops.len() == 1 {
                return FillRef::Solid(stops[0].color);
            }
            let pf = match g.path {
                Some([left, right, top, bottom]) => {
                    let mut sorted = stops;
                    // `[...stops].sort((a, b) => a.position - b.position)`: stable; a NaN position keeps its place
                    sorted.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
                    PaintFill::Path { left, right, top, bottom, stops: sorted.into() }
                }
                None => PaintFill::Linear { degree: g.degree, stops: stops.into() },
            };
            return push(arena, pf);
        }
        match fill.pattern.as_deref() {
            None | Some("none") => FillRef::None,
            Some("solid") => FillRef::Solid(pal.intern(fill.fg_color.as_deref().unwrap_or("#000000"))),
            Some(name) => {
                let fg = pal.intern(fill.fg_color.as_deref().unwrap_or("#000000"));
                let bg = pal.intern(fill.bg_color.as_deref().unwrap_or("#FFFFFF"));
                push(arena, PaintFill::Pattern { rows: pattern_rows(name).copied(), fg, bg })
            }
        }
    }
}

fn push(arena: &mut Vec<PaintFill>, f: PaintFill) -> FillRef {
    match u32::try_from(arena.len()) {
        Ok(i) => {
            arena.push(f);
            FillRef::Id(i)
        }
        Err(_) => FillRef::None,
    }
}

/// What `paintCellFill` draws for a rectangle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FillPaint<'a> {
    /// Nothing (no fill, or a rectangle with `w <= 0` or `h <= 0`).
    Nothing,
    /// `fillStyle = color; fillRect`.
    Solid(Color),
    /// A pattern: tile it (`rows` = the mask, `None` = the unknown-name fallback is [`FillPaint::Solid`] of `bg`),
    /// `fg` on `bg`, with the pattern transform `phase` (`DOMMatrix([1, 0, 0, 1, mx, my])`, only when `mx || my`).
    Pattern {
        /// The mask.
        rows: &'a PatternMask,
        /// "On" pixels.
        fg: Color,
        /// Background.
        bg: Color,
        /// The translation of the pattern (`None` = identity).
        phase: Option<(f64, f64)>,
    },
    /// `createLinearGradient(p0, p1)` with the stops (positions clamped to `[0, 1]` when added).
    Linear {
        /// Start point.
        p0: (f64, f64),
        /// End point.
        p1: (f64, f64),
        /// The stops.
        stops: &'a [Stop],
    },
    /// The `path` gradient, rasterised at `w` x `h` px by [`path_gradient_rgba`] and drawn into the rectangle.
    Path {
        /// Inner rectangle fractions.
        inner: [f64; 4],
        /// Stops sorted by position.
        stops: &'a [Stop],
        /// Pixel width of the raster (`max(1, round(w))`).
        w: u32,
        /// Pixel height.
        h: u32,
    },
}

/// The pixels above which the TS gives up on the path gradient and paints the first stop.
pub const PATH_GRADIENT_MAX_PIXELS: u64 = 4_000_000;

/// `paintCellFill(ctx, fill, x, y, w, h, ox, oy)` as paint data: `(ox, oy)` is the screen position of A1 (the
/// pattern anchor). `arena` is the book's table of non-solid fills.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn fill_paint<'a>(arena: &'a [PaintFill], f: FillRef, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64) -> FillPaint<'a> {
    if w <= 0.0 || h <= 0.0 {
        return FillPaint::Nothing;
    }
    let id = match f {
        FillRef::None => return FillPaint::Nothing,
        FillRef::Solid(c) => return FillPaint::Solid(c),
        FillRef::Id(i) => i,
    };
    let Some(pf) = arena.get(id as usize) else { return FillPaint::Nothing };
    match pf {
        PaintFill::Linear { degree, stops } => {
            let rad = (math::rem(*degree, 360.0) * std::f64::consts::PI) / 180.0;
            let (dx, dy) = (math::cos(rad), math::sin(rad));
            let half = (math::abs(dx) + math::abs(dy)) / 2.0;
            FillPaint::Linear {
                p0: (x + (0.5 - dx * half) * w, y + (0.5 - dy * half) * h),
                p1: (x + (0.5 + dx * half) * w, y + (0.5 + dy * half) * h),
                stops,
            }
        }
        PaintFill::Path { left, right, top, bottom, stops } => {
            let (pw, ph) = (math::max(1.0, math::round(w)), math::max(1.0, math::round(h)));
            if pw * ph > 4_000_000.0 {
                return stops.first().map_or(FillPaint::Nothing, |s| FillPaint::Solid(s.color));
            }
            FillPaint::Path {
                inner: [*left, *right, *top, *bottom],
                stops,
                w: uo_jsnum::conv::to_uint32(pw),
                h: uo_jsnum::conv::to_uint32(ph),
            }
        }
        PaintFill::Pattern { rows, fg, bg } => match rows {
            None => FillPaint::Solid(*bg),
            Some(r) => {
                let mx = math::rem(math::rem(ox, 8.0) + 8.0, 8.0);
                let my = math::rem(math::rem(oy, 8.0) + 8.0, 8.0);
                let phase = (uo_jsnum::conv::truthy(mx) || uo_jsnum::conv::truthy(my)).then_some((mx, my));
                FillPaint::Pattern { rows: r, fg: *fg, bg: *bg, phase }
            }
        },
    }
}

/// `paintPathGradient`'s pixels: `w` x `h` RGBA bytes (alpha 255) written into `out` (cleared first). Each pixel is the
/// normalised distance `t` from the inner rectangle to the edge, mapped through the stops with `Math.round` per
/// channel.
pub fn path_gradient_rgba(inner: [f64; 4], stops: &[Stop], w: u32, h: u32, out: &mut Vec<u8>) {
    out.clear();
    let n = u64::from(w) * u64::from(h);
    if n > PATH_GRADIENT_MAX_PIXELS || stops.is_empty() {
        return;
    }
    out.reserve(usize::try_from(n * 4).unwrap_or(0));
    let (wf, hf) = (f64::from(w), f64::from(h));
    let (l, r, t, b) = (inner[0] * wf, inner[1] * wf, inner[2] * hf, inner[3] * hf);
    for j in 0..h {
        let py = f64::from(j) + 0.5;
        let ty = if py < t {
            (t - py) / math::max(t, 1e-9)
        } else if py > b {
            (py - b) / math::max(hf - b, 1e-9)
        } else {
            0.0
        };
        for i in 0..w {
            let px = f64::from(i) + 0.5;
            let tx = if px < l {
                (l - px) / math::max(l, 1e-9)
            } else if px > r {
                (px - r) / math::max(wf - r, 1e-9)
            } else {
                0.0
            };
            let tt = math::min(1.0, math::max(tx, ty));
            let mut k = 0usize;
            while k + 1 < stops.len() && stops.get(k + 1).is_some_and(|s| s.pos < tt) {
                k += 1;
            }
            let (Some(a), Some(bs)) = (stops.get(k), stops.get((k + 1).min(stops.len() - 1))) else { continue };
            let f = if bs.pos > a.pos { math::min(1.0, math::max(0.0, (tt - a.pos) / (bs.pos - a.pos))) } else { 0.0 };
            for c in 0..3 {
                let (ca, cb) = (f64::from(a.rgb[c]), f64::from(bs.rgb[c]));
                out.push(uo_jsnum::conv::to_uint8_clamp(math::round(ca + (cb - ca) * f)));
            }
            out.push(255);
        }
    }
}

/// The two stops of a stop list the way `addColorStop(Math.min(1, Math.max(0, position)), color)` receives them.
#[must_use]
pub fn clamp_stop(pos: f64) -> f64 {
    math::min(1.0, math::max(0.0, pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::model::{Gradient, GradientStop};

    fn solid(c: &str) -> Fill {
        Fill { pattern: Some("solid".into()), fg_color: Some(c.into()), ..Fill::default() }
    }

    #[test]
    fn resolve_kinds() {
        let (mut pal, mut arena) = (Palette::default(), Vec::new());
        assert_eq!(FillRef::resolve(&Fill::default(), &mut pal, &mut arena), FillRef::None);
        assert_eq!(FillRef::resolve(&Fill { pattern: Some("none".into()), ..Fill::default() }, &mut pal, &mut arena), FillRef::None);
        assert_eq!(FillRef::resolve(&solid("#FF0000"), &mut pal, &mut arena), FillRef::Solid(Color::from_rgb(255, 0, 0)));
        let p = Fill { pattern: Some("gray125".into()), ..Fill::default() };
        let FillRef::Id(i) = FillRef::resolve(&p, &mut pal, &mut arena) else { panic!("pattern") };
        assert_eq!(arena[i as usize], PaintFill::Pattern { rows: pattern_rows("gray125").copied(), fg: Color::BLACK, bg: Color::WHITE });
        // a name without a mask keeps its fg/bg and paints bg
        let q = Fill { pattern: Some("foo".into()), ..Fill::default() };
        let FillRef::Id(j) = FillRef::resolve(&q, &mut pal, &mut arena) else { panic!("unknown") };
        assert!(matches!(arena[j as usize], PaintFill::Pattern { rows: None, .. }));
        let g1 = Fill {
            gradient: Some(Gradient { degree: 0.0, stops: vec![GradientStop { position: 0.0, color: "#102030".into() }], path: None }),
            ..Fill::default()
        };
        assert_eq!(FillRef::resolve(&g1, &mut pal, &mut arena), FillRef::Solid(Color::from_rgb(0x10, 0x20, 0x30)));
        let g0 = Fill { gradient: Some(Gradient { degree: 0.0, stops: vec![], path: None }), ..Fill::default() };
        assert_eq!(FillRef::resolve(&g0, &mut pal, &mut arena), FillRef::None);
    }

    #[test]
    fn paint_geometry() {
        let (mut pal, mut arena) = (Palette::default(), Vec::new());
        let g = Fill {
            gradient: Some(Gradient {
                degree: 45.0,
                stops: vec![
                    GradientStop { position: 0.0, color: "#000000".into() },
                    GradientStop { position: 1.0, color: "#ffffff".into() },
                ],
                path: None,
            }),
            ..Fill::default()
        };
        let f = FillRef::resolve(&g, &mut pal, &mut arena);
        let FillPaint::Linear { p0, p1, .. } = fill_paint(&arena, f, 0.0, 0.0, 100.0, 50.0, 0.0, 0.0) else { panic!("linear") };
        // 45 degrees runs corner to corner of the stretched cell
        assert!((p0.0 - 0.0).abs() < 1e-9 && (p0.1 - 0.0).abs() < 1e-9 && (p1.0 - 100.0).abs() < 1e-9 && (p1.1 - 50.0).abs() < 1e-9);
        assert_eq!(fill_paint(&arena, f, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0), FillPaint::Nothing);
        let pat = FillRef::resolve(&Fill { pattern: Some("darkUp".into()), ..Fill::default() }, &mut pal, &mut arena);
        let FillPaint::Pattern { phase, .. } = fill_paint(&arena, pat, 0.0, 0.0, 9.0, 9.0, -13.5, 0.0) else { panic!("pattern") };
        assert_eq!(phase, Some((2.5, 0.0)));
        let FillPaint::Pattern { phase, .. } = fill_paint(&arena, pat, 0.0, 0.0, 9.0, 9.0, 0.0, 8.0) else { panic!("pattern") };
        assert_eq!(phase, None);
    }

    #[test]
    fn path_gradient_edges() {
        let stops = [Stop { pos: 0.0, color: Color::BLACK, rgb: [0, 0, 0] }, Stop { pos: 1.0, color: Color::WHITE, rgb: [255, 255, 255] }];
        let mut px = Vec::new();
        path_gradient_rgba([0.5, 0.5, 0.5, 0.5], &stops, 4, 4, &mut px);
        assert_eq!(px.len(), 64);
        assert!(px.chunks(4).all(|p| p[3] == 255));
        // the outermost pixels are lighter than the centre ones
        assert!(px[0] > px[(4 + 1) * 4]);
        path_gradient_rgba([0.0; 4], &stops, 3000, 3000, &mut px);
        assert!(px.is_empty(), "past 4,000,000 pixels the TS paints the first stop instead");
    }
}
