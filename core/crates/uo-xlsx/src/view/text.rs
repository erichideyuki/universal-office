//! The text of one cell as the Excel painter draws it (`cell-paint.ts`: `paintCellText`, `paintWrapped`, `paintRotated`,
//! `decorate`, `paintErrorMark`) emitted as `SheetFont` + `GlyphRun` items. Every arithmetic expression is the TS's,
//! with `uo_jsnum::math` for the JS functions.

use uo_jsnum::math::{abs, ceil, cos, floor, max, min, round, sin};
use uo_render::slides::Baseline;

use super::out::Out;
use crate::grid::style::{HAlign, VAlign};
use crate::grid::text::{CellKind, CellText, FontMetrics, cell_margin_px, text_width, wrap_lines};
use crate::grid::{CellFont, is_complex};

/// The underline of a run (`UnderlineStyle` of the model).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Underline {
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

impl Underline {
    pub fn of(style: Option<&str>) -> Underline {
        match style {
            Some("double") => Underline::Double,
            Some("singleAccounting") => Underline::SingleAccounting,
            Some("doubleAccounting") => Underline::DoubleAccounting,
            _ => Underline::Single,
        }
    }
    fn accounting(self) -> bool {
        matches!(self, Underline::SingleAccounting | Underline::DoubleAccounting)
    }
    fn double(self) -> bool {
        matches!(self, Underline::Double | Underline::DoubleAccounting)
    }
}

/// A font of the painter: the measurements and what the `SheetFont` item names.
#[derive(Clone, Copy)]
pub struct Fnt<'a> {
    pub m: CellFont<'a>,
    pub face: u32,
    pub ppem: i32,
    pub italic: bool,
    /// `f.ascent`/`f.descent`: the rounded `'Hg'` box.
    pub ascent: i32,
    pub descent: i32,
    pub xl_ascent: i32,
    pub xl_descent: i32,
}

/// `PaintTextArgs` of `cell-paint.ts`.
pub struct TextArgs<'a, 'b> {
    pub f: Fnt<'a>,
    pub ct: &'b CellText,
    pub kind: CellKind,
    pub left: f64,
    pub top: f64,
    pub w: f64,
    pub h: f64,
    pub halign: HAlign,
    pub valign: VAlign,
    pub wrap: bool,
    pub indent: f64,
    pub indent_px: f64,
    pub rotation: f64,
    pub color: u32,
    pub underline: Option<Underline>,
    pub strike: bool,
    pub line_h: f64,
    pub merged: bool,
    pub base_shift: f64,
    pub row_desc: f64,
    pub mirrored: bool,
}

/// Reused buffers of the text pass.
#[derive(Default)]
pub struct TextScratch {
    adv: Vec<i32>,
    lines: Vec<(u32, u32)>,
}

/// `drawCellText`: the font item and the glyph run of `units` at `x` on the baseline `y`.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    o: &mut Out<'_>,
    sc: &mut TextScratch,
    f: &Fnt<'_>,
    units: &[u16],
    x: f64,
    y: f64,
    min_x: f64,
    max_x: f64,
    mirrored: bool,
) {
    o.font_cell(f.face, f.ppem, f.italic);
    let complex = is_complex(units);
    sc.adv.clear();
    let width = if complex {
        f.m.width(units, f64::INFINITY)
    } else {
        let mut w = 0.0;
        let mut i = 0;
        while let Some(&u) = units.get(i) {
            let two = (0xD800..0xDC00).contains(&u) && units.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l));
            let cp = if two { 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(units[i + 1]) - 0xDC00) } else { u32::from(u) };
            let a = f.m.adv(cp);
            sc.adv.push(a);
            w += f64::from(a);
            i += if two { 2 } else { 1 };
        }
        w
    };
    o.glyph_run(units, &sc.adv, x, y, min_x, max_x, width, complex, mirrored);
}

/// `paintErrorMark`: the green triangle of "number stored as text".
pub fn paint_error_mark(o: &mut Out<'_>, left: f64, top: f64, zoom: f64) {
    let n = max(2.0, round(6.0 * zoom));
    let x = round(left) + 1.0;
    let y = round(top) + 1.0;
    o.fill_style("#008000");
    let mut k = 0.0;
    while k < n {
        o.fill_rect(x, y + k, n - k, 1.0);
        k += 1.0;
    }
}

/// `centerX`.
fn center_x(left: f64, w: f64, big_w: f64) -> f64 {
    left + 1.0 + floor((w - big_w) / 2.0)
}

/// `effectiveHAlign`: the horizontal alignment as the painter sees it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HA {
    Left,
    Right,
    Center,
    CenterContinuous,
    Fill,
    Justify,
    Distributed,
    Other,
}

pub fn effective_halign(h: HAlign, kind: CellKind) -> HA {
    match h {
        HAlign::None | HAlign::General => match kind {
            CellKind::Number => HA::Right,
            CellKind::Text => HA::Left,
            _ => HA::Center,
        },
        HAlign::Left => HA::Left,
        HAlign::Right => HA::Right,
        HAlign::Center => HA::Center,
        HAlign::CenterContinuous => HA::CenterContinuous,
        HAlign::Fill => HA::Fill,
        HAlign::Justify => HA::Justify,
        HAlign::Distributed => HA::Distributed,
        HAlign::Other => HA::Other,
    }
}

/// `baselineFor`.
pub fn baseline_for(f: &Fnt<'_>, valign: VAlign, top: f64, h: f64, row_desc: f64) -> f64 {
    let (a, d) = (f64::from(f.xl_ascent), f64::from(f.xl_descent));
    match valign {
        VAlign::Top => top + a,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => top + floor((h - (a + d)) / 2.0) + a,
        _ => top + h - max(d, row_desc),
    }
}

/// `decorate`: the underline and the strike of one line drawn in `[x0, x0 + W]`.
#[allow(clippy::too_many_arguments)]
fn decorate(o: &mut Out<'_>, a: &TextArgs<'_, '_>, x0: f64, big_w: f64, y: f64, box_l: f64, box_r: f64) {
    if a.underline.is_none() && !a.strike {
        return;
    }
    let t = max(1.0, round(f64::from(a.f.ppem) / 16.0));
    o.fill_style_sym(a.color);
    let line = |o: &mut Out<'_>, yy: f64, xa: f64, xb: f64| o.fill_rect(round(xa), round(yy), round(xb - xa), t);
    if let Some(u) = a.underline {
        let acc = u.accounting();
        let (xa, xb) = if acc { (box_l, box_r) } else { (x0, x0 + big_w) };
        let u1 = y + max(1.0, round(f64::from(a.f.descent) * if acc { 0.6 } else { 0.35 }));
        line(o, u1, xa, xb);
        if u.double() {
            line(o, u1 + t + 1.0, xa, xb);
        }
    }
    if a.strike {
        line(o, y - round(f64::from(a.f.ascent) * 0.33), x0, x0 + big_w);
    }
}

fn piece(ct: &CellText, i: usize) -> &[u16] {
    ct.piece_text(i)
}

/// `paintCellText`.
pub fn paint_cell_text(o: &mut Out<'_>, sc: &mut TextScratch, a: &TextArgs<'_, '_>, overflow: &mut dyn FnMut(i32, f64) -> f64) {
    let ct = a.ct;
    if ct.pieces.is_empty() && ct.text.is_empty() {
        return;
    }
    let f = &a.f;
    let margin = (cell_margin_px(f.ppem) - 1.0) / 2.0;
    let box_l = a.left + 1.0 + margin;
    let box_r = a.left + a.w - margin;
    let box_w = max(0.0, box_r - box_l);
    let halign = effective_halign(a.halign, a.kind);
    let indent = if a.indent > 0.0 && matches!(halign, HA::Left | HA::Right | HA::Distributed) { a.indent * a.indent_px } else { 0.0 };
    o.fill_style_sym(a.color);
    o.text_baseline(Baseline::Alphabetic);

    if a.rotation != 0.0 && a.kind == CellKind::Text {
        paint_rotated(o, sc, a, box_l, box_r, a.rotation);
        return;
    }
    let wrap_mode = a.kind == CellKind::Text
        && !ct.hashes
        && (a.wrap || matches!(halign, HA::Justify | HA::Distributed) || matches!(a.valign, VAlign::Justify | VAlign::Distributed));
    if wrap_mode {
        paint_wrapped(o, sc, a, box_l, box_w, halign, indent);
        return;
    }
    let big_w = if ct.width >= 0.0 { ct.width } else { text_width(&f.m, &ct.text, box_w + 4096.0) };
    let x0 = if ct.filled || (halign == HA::Fill && a.kind == CellKind::Text && big_w > 0.0) {
        box_l
    } else if halign == HA::Right {
        box_r - big_w - indent
    } else if matches!(halign, HA::Center | HA::CenterContinuous) {
        center_x(a.left, a.w, big_w)
    } else {
        box_l + indent
    };
    let mut clip_l = a.left;
    let mut clip_r = a.left + a.w;
    if ct.overflows && !a.merged && big_w > box_w - indent && halign != HA::Fill {
        let over = big_w - (box_w - indent);
        if halign == HA::Right {
            clip_l -= overflow(-1, over);
        } else if matches!(halign, HA::Center | HA::CenterContinuous) {
            clip_l -= overflow(-1, over / 2.0 + 1.0);
            clip_r += overflow(1, over / 2.0 + 1.0);
        } else {
            clip_r += overflow(1, over);
        }
    }
    let y = baseline_for(f, a.valign, a.top, a.h, a.row_desc) + a.base_shift;
    o.save();
    o.begin_path();
    o.rect(clip_l + 1.0, a.top + 1.0, max(0.0, clip_r - clip_l - 1.0), max(0.0, a.h - 1.0));
    o.push(uo_render::slides::SItem::Clip(uo_render::slides::FillRule::NonZero));
    if halign == HA::Fill && a.kind == CellKind::Text && !ct.filled && big_w > 0.0 {
        let n = max(1.0, floor(box_w / big_w));
        let mut i = 0.0;
        while i < n {
            for k in 0..ct.pieces.len() {
                let px = ct.pieces.get(k).map_or(0.0, |p| p.x);
                draw_text(o, sc, f, piece(ct, k), x0 + i * big_w + px, y, clip_l, clip_r, a.mirrored);
            }
            i += 1.0;
        }
    } else {
        for k in 0..ct.pieces.len() {
            let px = ct.pieces.get(k).map_or(0.0, |p| p.x);
            draw_text(o, sc, f, piece(ct, k), x0 + px, y, clip_l, clip_r, a.mirrored);
        }
    }
    if !ct.hashes {
        decorate(o, a, x0, big_w, y, box_l, box_r);
    }
    o.restore();
}

fn paint_wrapped(o: &mut Out<'_>, sc: &mut TextScratch, a: &TextArgs<'_, '_>, box_l: f64, box_w: f64, halign: HA, indent: f64) {
    let f = &a.f;
    let ct = a.ct;
    let max_lines = max(1.0, ceil(a.h / max(1.0, a.line_h)) + 1.0);
    let mut lines = std::mem::take(&mut sc.lines);
    lines.clear();
    wrap_lines(&f.m, &ct.text, max(1.0, box_w - indent), (max_lines + 64.0) as usize, &mut lines);
    let line_h = a.line_h;
    let n = lines.len();
    let block_h = (n as f64 - 1.0) * line_h + f64::from(f.xl_ascent) + f64::from(f.xl_descent);
    let y0 = match a.valign {
        VAlign::Top | VAlign::Justify | VAlign::Distributed => a.top,
        VAlign::Center => a.top + floor((a.h - block_h) / 2.0),
        _ => a.top + a.h - block_h - max(0.0, a.row_desc - f64::from(f.xl_descent)),
    };
    o.save();
    o.begin_path();
    o.rect(a.left + 1.0, a.top + 1.0, max(0.0, a.w - 1.0), max(0.0, a.h - 1.0));
    o.push(uo_render::slides::SItem::Clip(uo_render::slides::FillRule::NonZero));
    for (i, &(s, e)) in lines.iter().enumerate() {
        let y = y0 + f64::from(f.xl_ascent) + i as f64 * line_h;
        if y - f64::from(f.ascent) > a.top + a.h || y + f64::from(f.descent) < a.top {
            continue;
        }
        let line = ct.text.get(s as usize..e as usize).unwrap_or(&[]);
        let big_w = f.m.width(line, f64::INFINITY);
        let last = i + 1 == n;
        let x = if (halign == HA::Justify && !last) || halign == HA::Distributed {
            // words = line.split(/ +/)
            let mut words: Vec<&[u16]> = Vec::new();
            let mut st = 0;
            let mut k = 0;
            while k < line.len() {
                if line[k] == 0x20 {
                    words.push(&line[st..k]);
                    while k < line.len() && line[k] == 0x20 {
                        k += 1;
                    }
                    st = k;
                } else {
                    k += 1;
                }
            }
            words.push(&line[st..]);
            if words.len() > 1 {
                let ww: f64 = words.iter().fold(0.0, |s, w| s + f.m.width(w, f64::INFINITY));
                let gap = (box_w - indent - ww) / (words.len() as f64 - 1.0);
                let mut cx = box_l + indent;
                for w in &words {
                    draw_text(o, sc, f, w, round(cx), y, f64::NEG_INFINITY, f64::INFINITY, a.mirrored);
                    cx += f.m.width(w, f64::INFINITY) + gap;
                }
                continue;
            }
            if halign == HA::Distributed { box_l + floor((box_w - big_w) / 2.0) } else { box_l + indent }
        } else if halign == HA::Right {
            box_l + box_w - big_w - indent
        } else if matches!(halign, HA::Center | HA::CenterContinuous | HA::Distributed) {
            box_l + floor((box_w - big_w) / 2.0)
        } else {
            box_l + indent
        };
        draw_text(o, sc, f, line, x, y, f64::NEG_INFINITY, f64::INFINITY, a.mirrored);
        decorate(o, a, x, big_w, y, box_l, box_l + box_w);
    }
    o.restore();
    sc.lines = lines;
}

fn paint_rotated(o: &mut Out<'_>, sc: &mut TextScratch, a: &TextArgs<'_, '_>, box_l: f64, box_r: f64, rot: f64) {
    let f = &a.f;
    let ct = a.ct;
    o.save();
    o.begin_path();
    o.rect(a.left + 1.0, a.top + 1.0, max(0.0, a.w - 1.0), max(0.0, a.h - 1.0));
    o.push(uo_render::slides::SItem::Clip(uo_render::slides::FillRule::NonZero));
    if rot == 255.0 {
        let line_h = a.line_h;
        // one code point per line
        let mut cps: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while let Some(&u) = ct.text.get(i) {
            let two = (0xD800..0xDC00).contains(&u) && ct.text.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l));
            cps.push((i, i + if two { 2 } else { 1 }));
            i += if two { 2 } else { 1 };
        }
        let total = (cps.len() as f64 - 1.0) * line_h + f64::from(f.xl_ascent) + f64::from(f.xl_descent);
        let mut y = match a.valign {
            VAlign::Top => a.top,
            VAlign::Center => a.top + floor((a.h - total) / 2.0),
            _ => a.top + a.h - total,
        };
        y += f64::from(f.xl_ascent);
        for &(s, e) in &cps {
            let ch = &ct.text[s..e];
            let cp =
                if e - s == 2 { 0x10000 + ((u32::from(ch[0]) - 0xD800) << 10) + (u32::from(ch[1]) - 0xDC00) } else { u32::from(ch[0]) };
            let cw = f64::from(f.m.adv(cp));
            draw_text(o, sc, f, ch, a.left + floor((a.w - cw) / 2.0), y, f64::NEG_INFINITY, f64::INFINITY, a.mirrored);
            y += line_h;
        }
        o.restore();
        return;
    }
    let deg = if rot <= 90.0 { rot } else { -(rot - 90.0) };
    let rad = (-deg * std::f64::consts::PI) / 180.0;
    let big_w = f.m.width(&ct.text, f64::INFINITY);
    let big_h = f64::from(f.ascent + f.descent);
    let (c, s) = (abs(cos(rad)), abs(sin(rad)));
    let bw = big_w * c + big_h * s;
    let bh = big_w * s + big_h * c;
    let halign = if (rot == 90.0 || rot == 180.0) && matches!(a.halign, HAlign::None | HAlign::General) {
        HA::Right
    } else {
        effective_halign(a.halign, a.kind)
    };
    let cx = if halign == HA::Right {
        box_r - bw / 2.0
    } else if halign == HA::Center {
        (box_l + box_r) / 2.0
    } else {
        box_l + bw / 2.0
    };
    let cy = match a.valign {
        VAlign::Top => a.top + 1.0 + bh / 2.0,
        VAlign::Center => a.top + a.h / 2.0,
        _ => a.top + a.h - 1.0 - bh / 2.0,
    };
    o.translate(cx, cy);
    o.rotate(rad);
    draw_text(o, sc, f, &ct.text, -big_w / 2.0, big_h / 2.0 - f64::from(f.descent), f64::NEG_INFINITY, f64::INFINITY, a.mirrored);
    o.restore();
    let _ = min(0.0, 0.0);
}
