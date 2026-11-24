//! The decorations of the paint: fills (`paintCellFill`), data bars and icons, diagonals, markers (comment, filter
//! arrow, table handle), the selection (`paintSelection`) and the headers (`paintHeaders`).

use rustc_hash::FxHashMap;
use uo_jsnum::math::{floor, max, min, round};
use uo_render::slides::{
    DrawImageItem, FillRule, Gradient, ImageSrc, Mat6, Paint, Pair, PathOp, PatternRes, PatternSrc, Rect4, Repeat, SItem, Span, Stop,
};

use super::out::Out;
use crate::grid::cf::{BarFill, CanvasOp};
use crate::grid::paint_rules::fill::{FillPaint, path_gradient_rgba};
use crate::grid::paint_rules::{Color, DiagonalStrokes, Palette, SPILL_BORDER_COLOR, spill_border_rect, table_handle_rects};

pub const HEADER_W: f64 = 46.0;
pub const HEADER_H: f64 = 22.0;
pub const GRID_LINE: &str = "#e0e0e0";
pub const COL_HEADER_BG: &str = "#f6f6f6";
pub const ROW_HEADER_BG: &str = "#f6f6f6";
pub const CORNER_BG: &str = "#ececec";
pub const HEADER_LINE: &str = "#d3d3d3";
pub const TEXT: &str = "#1f1f1f";

/// Caches of resources the fills create (pattern tiles and path-gradient rasters).
#[derive(Default)]
pub struct FillCache {
    tiles: FxHashMap<(u64, u32, u32), u32>,
    rasters: FxHashMap<(u32, u32, u64), u32>,
    pub raster: Vec<u8>,
}

fn clamp01(p: f64) -> f64 {
    min(1.0, max(0.0, p))
}

fn linear_gradient(o: &mut Out<'_>, p0: (f64, f64), p1: (f64, f64), stops: &[(f64, u32)]) -> Paint {
    let start = u32::try_from(o.list.stops.len()).unwrap_or(u32::MAX);
    for &(offset, color) in stops {
        o.list.stops.push(Stop { offset, color });
    }
    let g = Gradient {
        g0: p0.0,
        g1: p0.1,
        g2: p1.0,
        g3: p1.1,
        g4: 0.0,
        g5: 0.0,
        stops: Span { start, len: u32::try_from(stops.len()).unwrap_or(u32::MAX) },
    };
    let i = u32::try_from(o.list.gradients.len()).unwrap_or(u32::MAX);
    o.list.gradients.push(g);
    Paint::Linear(i)
}

/// `paintCellFill` for a fill as paint data.
#[allow(clippy::too_many_arguments)]
pub fn emit_fill(o: &mut Out<'_>, pal: &Palette, cache: &mut FillCache, fp: FillPaint<'_>, x: f64, y: f64, w: f64, h: f64) {
    let csym = |o: &mut Out<'_>, c: Color| o.color_sym(c, || pal.text(c));
    match fp {
        FillPaint::Nothing => {}
        FillPaint::Solid(c) => {
            let s = csym(o, c);
            o.fill_style_sym(s);
            o.fill_rect(x, y, w, h);
        }
        FillPaint::Pattern { rows, fg, bg, phase } => {
            let key = (u64::from_le_bytes(*rows), fg.0, bg.0);
            let id = match cache.tiles.get(&key) {
                Some(&i) => i,
                None => {
                    let (f, b) = (pal.hex_rgb(fg), pal.hex_rgb(bg));
                    let mut px = Vec::with_capacity(8 * 8 * 4);
                    for r in 0..8usize {
                        for c in 0..8usize {
                            let on = rows.get(r).is_some_and(|m| m & (1 << (7 - c)) != 0);
                            let rgb = if on { f } else { b };
                            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                        }
                    }
                    let i = o.res.bitmap(8, 8, px.into()).unwrap_or(0);
                    cache.tiles.insert(key, i);
                    i
                }
            };
            let pi = u32::try_from(o.list.patterns.len()).unwrap_or(u32::MAX);
            o.list.patterns.push(PatternRes {
                src: PatternSrc::Bitmap(id),
                repeat: Repeat::Repeat,
                transform: phase.map(|(e, f)| Mat6 { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e, f }),
                tile: None,
            });
            o.push(SItem::FillStyle(Paint::Pattern(pi)));
            o.st.fill = None;
            o.fill_rect(x, y, w, h);
        }
        FillPaint::Linear { p0, p1, stops } => {
            let st: Vec<(f64, u32)> = stops.iter().map(|s| (clamp01(s.pos), csym(o, s.color))).collect();
            let g = linear_gradient(o, p0, p1, &st);
            o.push(SItem::FillStyle(g));
            o.st.fill = None;
            o.fill_rect(x, y, w, h);
        }
        FillPaint::Path { inner, stops, w: pw, h: ph } => {
            let key = (pw, ph, hash_path(inner, stops));
            let id = match cache.rasters.get(&key) {
                Some(&i) => i,
                None => {
                    path_gradient_rgba(inner, stops, pw, ph, &mut cache.raster);
                    let i = o.res.bitmap(pw, ph, cache.raster.clone().into()).unwrap_or(0);
                    cache.rasters.insert(key, i);
                    i
                }
            };
            o.push(SItem::DrawImage(DrawImageItem { src: ImageSrc::Bitmap(id), s: None, d: Rect4 { x, y, w, h } }));
        }
    }
}

fn hash_path(inner: [f64; 4], stops: &[crate::grid::paint_rules::fill::Stop]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for v in inner {
        mix(v.to_bits());
    }
    for s in stops {
        mix(s.pos.to_bits());
        mix(u64::from(s.rgb[0]) << 16 | u64::from(s.rgb[1]) << 8 | u64::from(s.rgb[2]));
    }
    h
}

/// `paintDataBar`: the rectangles of C3b's `bar_rects`.
pub fn emit_bar(o: &mut Out<'_>, rects: &[crate::grid::cf::BarRect]) {
    for r in rects {
        match &r.fill {
            BarFill::Solid(c) => o.fill_style(c),
            BarFill::Linear { x0, x1, c0, c1 } => {
                let (a, b) = (o.sym(c0), o.sym(c1));
                let g = linear_gradient(o, (*x0, 0.0), (*x1, 0.0), &[(0.0, a), (1.0, b)]);
                o.push(SItem::FillStyle(g));
                o.st.fill = None;
            }
        }
        o.fill_rect(r.x, r.y, r.w, r.h);
    }
}

/// `paintCfIcon` from the C3b ops.
pub fn emit_icon(o: &mut Out<'_>, ops: &[CanvasOp]) {
    o.st.stroke = None;
    // C3b folds `strokeStyle = c` into `Stroke(c)`; the TS sets it BEFORE `lineWidth`/`lineCap` of the same stroke
    let mut run: Option<usize> = None;
    let mut cap = false;
    let mut prev_lw: Option<usize> = None;
    for op in ops {
        // `strokeStyle = c; lineWidth = 1; strokeRect(..)`: the style comes before a width that directly precedes the rect
        let lw_before = prev_lw.take();
        if let CanvasOp::LineWidth(_) = op {
            prev_lw = Some(o.list.items.len());
        }
        let pending = if cap { run } else { None };
        // the path ops between the assignments and the stroke are `Path2D` calls in the TS: no canvas event
        if !matches!(
            op,
            CanvasOp::LineWidth(_)
                | CanvasOp::LineCapButt
                | CanvasOp::Stroke(_)
                | CanvasOp::BeginPath
                | CanvasOp::MoveTo(..)
                | CanvasOp::LineTo(..)
                | CanvasOp::Arc(..)
                | CanvasOp::Rect(..)
                | CanvasOp::RoundRect(..)
                | CanvasOp::ClosePath
        ) {
            run = None;
            cap = false;
        }
        match op {
            CanvasOp::Save => o.save(),
            CanvasOp::Restore => o.restore(),
            CanvasOp::Translate(x, y) => o.translate(*x, *y),
            CanvasOp::Scale(x, y) => o.scale(*x, *y),
            CanvasOp::LineJoinMiter => o.push(SItem::LineJoin(uo_render::slides::Join::Miter)),
            CanvasOp::LineCapButt => {
                cap = true;
                o.push(SItem::LineCap(uo_render::slides::Cap::Butt));
            }
            CanvasOp::LineWidth(w) => {
                run.get_or_insert(o.list.items.len());
                o.line_width(*w);
            }
            CanvasOp::BeginPath => o.begin_path(),
            CanvasOp::MoveTo(x, y) => o.path(&[PathOp::MoveTo(Pair { x: *x, y: *y })]),
            CanvasOp::LineTo(x, y) => o.path(&[PathOp::LineTo(Pair { x: *x, y: *y })]),
            CanvasOp::Arc(cx, cy, r, a0, a1) => {
                o.path(&[PathOp::Arc(uo_render::slides::ArcOp { cx: *cx, cy: *cy, r: *r, a0: *a0, a1: *a1, ccw: false })]);
            }
            CanvasOp::Rect(x, y, w, h) => o.rect(*x, *y, *w, *h),
            CanvasOp::RoundRect(x, y, w, h, r) => {
                o.path(&[PathOp::RoundRect(uo_render::slides::RoundRectOp { x: *x, y: *y, w: *w, h: *h, r: *r })]);
            }
            CanvasOp::ClosePath => o.path(&[PathOp::Close]),
            CanvasOp::Fill(c) => {
                o.fill_style(c);
                o.fill();
            }
            CanvasOp::Stroke(c) => {
                let sym = o.sym(c);
                let it = SItem::StrokeStyle(Paint::Css(sym));
                match run.take() {
                    Some(i) => {
                        o.list.items.insert(i, it);
                        o.st.stroke = None;
                    }
                    None => o.push(it),
                }
                o.stroke();
            }
            CanvasOp::Clip => o.push(SItem::Clip(FillRule::NonZero)),
            CanvasOp::FillRect(c, x, y, w, h) => {
                // the `!` symbol sets `strokeStyle` to the colour of its rectangles, then never strokes (the op list drops it)
                if let Some(i) = pending {
                    let sym = o.sym(c);
                    {
                        o.list.items.insert(i, SItem::StrokeStyle(Paint::Css(sym)));
                        o.st.stroke = None;
                    }
                }
                o.fill_style(c);
                o.fill_rect(*x, *y, *w, *h);
            }
            CanvasOp::StrokeRect(c, x, y, w, h) => {
                if let Some(i) = lw_before {
                    let sym = o.sym(c);
                    {
                        o.list.items.insert(i, SItem::StrokeStyle(Paint::Css(sym)));
                        o.st.stroke = None;
                    }
                } else {
                    o.st.stroke = None;
                    o.stroke_style(c);
                }
                o.stroke_rect(*x, *y, *w, *h);
            }
        }
    }
}

/// `paintDiagonals`.
pub fn emit_diagonals(o: &mut Out<'_>, pal: &Palette, d: &DiagonalStrokes) {
    o.save();
    o.begin_path();
    o.rect(d.clip[0], d.clip[1], d.clip[2], d.clip[3]);
    o.push(SItem::Clip(FillRule::NonZero));
    let c = o.color_sym(d.color, || pal.text(d.color));
    o.stroke_style_sym(c);
    o.line_width(d.line_width);
    let start = u32::try_from(o.list.dashes.len()).unwrap_or(u32::MAX);
    o.list.dashes.extend_from_slice(d.dash);
    o.push(SItem::LineDash(Span { start, len: u32::try_from(d.dash.len()).unwrap_or(u32::MAX) }));
    for l in d.lines.iter().take(d.n) {
        o.begin_path();
        o.path(&[PathOp::MoveTo(Pair { x: l[0], y: l[1] }), PathOp::LineTo(Pair { x: l[2], y: l[3] })]);
        o.stroke();
    }
    o.restore();
}

/// `paintCommentIndicator`.
pub fn emit_comment_indicator(o: &mut Out<'_>, left: f64, top: f64, w: f64, zoom: f64) {
    let side = 5.0 * zoom;
    let (x1, y1) = (left + w, top);
    o.save();
    o.fill_style("#D7373F");
    o.begin_path();
    o.path(&[
        PathOp::MoveTo(Pair { x: x1 - side, y: y1 }),
        PathOp::LineTo(Pair { x: x1, y: y1 }),
        PathOp::LineTo(Pair { x: x1, y: y1 + side }),
        PathOp::Close,
    ]);
    o.fill();
    o.restore();
}

/// `paintFilterArrow`.
pub fn emit_filter_arrow(o: &mut Out<'_>, left: f64, top: f64, w: f64, h: f64, active: bool, zoom: f64) {
    let s = max(9.0, round(17.0 * zoom));
    if h < 8.0 || w < 8.0 {
        return;
    }
    let size = min(min(s, round(h) - 2.0), round(w));
    let x0 = round(left + w) - size;
    let y0 = round(top + h) - 1.0 - size;
    o.save();
    let (a, b) = (o.sym("#FEFEFE"), o.sym("#EFF2F7"));
    let g = linear_gradient(o, (0.0, y0), (0.0, y0 + size - 3.0), &[(0.0, a), (1.0, b)]);
    o.push(SItem::FillStyle(g));
    o.st.fill = None;
    o.fill_rect(x0, y0, size, size - 2.0);
    o.fill_style("#A6ACB3");
    o.fill_rect(x0, y0, 1.0, size - 2.0);
    o.fill_rect(x0 + size - 1.0, y0, 1.0, size - 2.0);
    o.fill_rect(x0, y0 + size - 3.0, size, 1.0);
    o.fill_style("#91969D");
    o.fill_rect(x0, y0 + size - 2.0, size, 1.0);
    o.fill_rect(x0, y0 + size - 1.0, 1.0, 1.0);
    o.fill_rect(x0 + size - 1.0, y0 + size - 1.0, 1.0, 1.0);
    o.fill_style("#DDE1E7");
    o.fill_rect(x0 + 1.0, y0 + size - 1.0, size - 2.0, 1.0);
    let cx = x0 + floor(size / 2.0);
    let ty = y0 + round(5.0 * size / 17.0);
    o.fill_style("#6F7174");
    if active {
        let sc = size / 17.0;
        let fx = x0 + round(7.0 * sc);
        let fy = y0 + round(2.0 * sc);
        o.fill_rect(fx, fy, 9.0, 1.0);
        o.fill_rect(fx + 1.0, fy + 1.0, 7.0, 1.0);
        o.fill_rect(fx + 2.0, fy + 2.0, 5.0, 1.0);
        o.fill_rect(fx + 3.0, fy + 3.0, 3.0, 5.0);
        o.fill_rect(x0 + round(4.0 * sc), y0 + round(8.0 * sc), 3.0, 1.0);
        o.fill_rect(x0 + round(5.0 * sc), y0 + round(9.0 * sc), 1.0, 1.0);
    } else {
        for k in 0..4 {
            let kf = f64::from(k);
            o.fill_rect(cx - 3.0 + kf, ty + kf, 7.0 - 2.0 * kf, 1.0);
        }
    }
    o.restore();
}

/// `paintTableHandle`.
pub fn emit_table_handle(o: &mut Out<'_>, right: f64, bottom: f64) {
    o.fill_style("#485DB1");
    for r in table_handle_rects(right, bottom) {
        o.fill_rect(r[0], r[1], r[2], r[3]);
    }
}

/// `paintSpillBorder`.
pub fn emit_spill_border(o: &mut Out<'_>, x1: f64, y1: f64, x2: f64, y2: f64) {
    let Some(r) = spill_border_rect(x1, y1, x2, y2) else { return };
    o.save();
    o.stroke_style(SPILL_BORDER_COLOR);
    o.line_width(1.0);
    o.push(SItem::LineDash(Span { start: 0, len: 0 }));
    o.stroke_rect(r[0], r[1], r[2], r[3]);
    o.restore();
}

/// The white space a CSS colour can carry (`\s` of the JS regex).
fn js_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{a0}' | '\u{feff}')
}

/// `hexRGB` of `universal-sheet.ts` (`rgb()`/`rgba()`, `#rrggbb`, `#rgb`; grey otherwise).
pub fn hex_rgb(c: &str) -> [f64; 3] {
    let b = c.as_bytes();
    // rgb(r,g,b) / rgba(r,g,b,a): /rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)/i
    if let Some(p) = c.to_ascii_lowercase().find("rgb") {
        let rest = &c[p + 3..];
        let rest = rest.strip_prefix(['a', 'A']).unwrap_or(rest);
        if let Some(rest) = rest.strip_prefix('(') {
            let mut nums = [0.0; 3];
            let mut it = rest.trim_start_matches(js_space);
            let mut ok = true;
            for (i, n) in nums.iter_mut().enumerate() {
                let d: String = it.chars().take_while(char::is_ascii_digit).collect();
                if d.is_empty() {
                    ok = false;
                    break;
                }
                *n = uo_jsnum::parse::parse_int(&d, 10);
                it = &it[d.len()..];
                if i < 2 {
                    let t = it.trim_start_matches(|ch: char| ch == ',' || js_space(ch));
                    if t.len() == it.len() {
                        ok = false;
                        break;
                    }
                    it = t;
                }
            }
            if ok {
                return nums;
            }
        }
    }
    // #?([0-9a-f]{6})
    let hex = |s: &[u8]| s.iter().all(u8::is_ascii_hexdigit);
    for i in 0..b.len() {
        let st = if b[i] == b'#' { i + 1 } else { i };
        if let Some(h) = b.get(st..st + 6)
            && hex(h)
        {
            let n = u32::from_str_radix(std::str::from_utf8(h).unwrap_or("0"), 16).unwrap_or(0);
            return [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)];
        }
    }
    for i in 0..b.len() {
        let st = if b[i] == b'#' { i + 1 } else { i };
        if let Some(h) = b.get(st..st + 3)
            && hex(h)
            && !b.get(st + 3).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            let d = |k: usize| f64::from(char::from(h[k]).to_digit(16).unwrap_or(0) * 17);
            return [d(0), d(1), d(2)];
        }
    }
    [128.0, 128.0, 128.0]
}

/// `mix(a, b, t)` → `rgb(r,g,b)`.
pub fn mix(a: &str, b: &str, t: f64) -> String {
    let (pa, pb) = (hex_rgb(a), hex_rgb(b));
    let ch = |i: usize| round(pa[i] + (pb[i] - pa[i]) * t);
    format!("rgb({},{},{})", ch(0), ch(1), ch(2))
}

/// `rgba(c, a)`.
pub fn rgba(c: &str, a: f64) -> String {
    let p = hex_rgb(c);
    format!("rgba({},{},{},{})", p[0], p[1], p[2], a)
}

/// The letters of a column number (`colLetters`).
pub fn col_letters(col: u32) -> Vec<u16> {
    let mut s: Vec<u16> = Vec::new();
    let mut c = col;
    while c > 0 {
        s.push(65 + u16::try_from((c - 1) % 26).unwrap_or(0));
        c = (c - 1) / 26;
    }
    s.reverse();
    s
}
