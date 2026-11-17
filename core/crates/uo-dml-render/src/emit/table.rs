//! Tables (`render.ts` 2152-2420): rows that grow to fit their text (the layout runs in EMU, scale-free), the
//! shadow, the background and cell fills, the border grid and the cell text.

use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_render::slides::{Cap, CtmAxis, CtmOperand, PathOp, Rect4, SItem};

use super::cx::{Cx, E, K, rgba_of};
use super::paint::{dash_pattern, image_tile, inset_rect, is_gradient, make_linear_gradient, preset_pattern};
use super::shape::{Fit, SHADOW_BLUR_K};
use super::text::{DEFAULT_BODY_PT, layout, paint_text};
use crate::model::{Fill, FillKind, ImageRef, LineGrid, OuterShadow, Shape, Table, TableBgPaint, TableCell, TableLine};
use crate::text::{LayoutTextOptions, Measurer, cell_body};

/// `TablePaint` (resolved fill of a table or cell).
struct TablePaint {
    fill: Option<Fill>,
    alpha: Option<f64>,
    image: Option<ImageRef>,
    tile: bool,
}

struct CellAt<'a> {
    cell: &'a TableCell,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn cell_paint(cell: &TableCell) -> Option<TablePaint> {
    if let Some(c) = cell.fill.as_ref().filter(|c| !c.as_str().is_empty()) {
        let mut f = Fill::of_kind(FillKind::Solid);
        f.color = Some(c.clone());
        return Some(TablePaint { fill: Some(f), alpha: cell.fill_alpha, image: None, tile: false });
    }
    if let Some(f) = cell.fill_paint.as_deref() {
        return Some(TablePaint { fill: Some(f.clone()), alpha: None, image: None, tile: false });
    }
    cell.fill_image.as_deref().map(|i| TablePaint {
        fill: None,
        alpha: None,
        image: Some(i.clone()),
        tile: cell.fill_tile.unwrap_or(false),
    })
}

fn bg_paint(t: &Table) -> Option<TablePaint> {
    if let Some(b) = t.bg_paint.as_deref() {
        let TableBgPaint { fill, alpha, image, tile } = b;
        return Some(TablePaint { fill: fill.clone(), alpha: *alpha, image: image.as_deref().cloned(), tile: tile.unwrap_or(false) });
    }
    t.bg.as_ref().map(|(c, a)| {
        let mut f = Fill::of_kind(FillKind::Solid);
        f.color = Some(c.clone());
        TablePaint { fill: Some(f), alpha: *a, image: None, tile: false }
    })
}

fn sum(v: &[f64]) -> f64 {
    v.iter().fold(0.0, |a, b| a + b)
}

/// `paintTable`.
pub fn paint_table(cx: &mut Cx<'_>, t: &Table, x: f64, y: f64, w: f64, h: f64, fit: &Fit) -> E {
    let scale = fit.scale;
    let all_zero = t.rows.iter().all(|r| !truthy(r.height_emu));
    let tot = sum(&t.col_widths_emu);
    let total_col_emu = if truthy(tot) { tot } else { 1.0 };
    let sx = w / total_col_emu;
    let opts = LayoutTextOptions { default_sz_pt: Some(DEFAULT_BODY_PT), default_anchor: Some("t".into()), ..LayoutTextOptions::default() };
    let need_of = |cx: &mut Cx<'_>, cell: &TableCell, ci: usize| -> E<f64> {
        let span = cell.grid_span.unwrap_or(1.0);
        let mut cw_emu = 0.0;
        // columns past the grid add 0 (`?? 0`): stop at the grid (a hostile span of 1e300 or infinity must not loop)
        let mut k = ci as f64;
        let end = math::min(ci as f64 + span, t.col_widths_emu.len() as f64);
        while k < end {
            cw_emu += usize_of(k).and_then(|i| t.col_widths_emu.get(i)).copied().unwrap_or(0.0);
            k += 1.0;
        }
        let Some(body) = cell_body(cell) else { return Ok(0.0) };
        let Some(lay) = layout(cx, &body, cw_emu, 1e12, Measurer::Ppt, &opts)? else { return Ok(0.0) };
        Ok(lay.total_h + body.ins_t.unwrap_or(0.0) + body.ins_b.unwrap_or(0.0))
    };
    let skip_cell = |cell: &TableCell| -> bool {
        cell.h_merge.unwrap_or(false)
            || cell.v_merge.unwrap_or(false)
            || cell.tx_body.is_none()
            || cell.vert.as_ref().is_some_and(|v| !v.is_empty())
    };
    let mut row_hs: Vec<f64> = Vec::with_capacity(t.rows.len());
    for r in &t.rows {
        if all_zero {
            row_hs.push(1.0);
            continue;
        }
        let mut need = math::max(r.height_emu, 1.0);
        for (ci, cell) in r.cells.iter().enumerate() {
            if skip_cell(cell) || cell.row_span.unwrap_or(1.0) > 1.0 {
                continue;
            }
            need = math::max(need, need_of(cx, cell, ci)?);
        }
        row_hs.push(need);
    }
    if !all_zero {
        for (ri, r) in t.rows.iter().enumerate() {
            for (ci, cell) in r.cells.iter().enumerate() {
                let rs = math::min(cell.row_span.unwrap_or(1.0), (t.rows.len() - ri) as f64);
                if skip_cell(cell) || rs <= 1.0 {
                    continue;
                }
                let mut have = 0.0;
                let mut k = ri as f64;
                while k < ri as f64 + rs {
                    have += usize_of(k).and_then(|i| row_hs.get(i)).copied().unwrap_or(0.0);
                    k += 1.0;
                }
                let need = need_of(cx, cell, ci)?;
                if need > have
                    && let Some(i) = usize_of(ri as f64 + rs - 1.0)
                    && let Some(v) = row_hs.get_mut(i)
                {
                    *v += need - have;
                }
            }
        }
    }
    let rt = sum(&row_hs);
    let total_row_emu = if truthy(rt) { rt } else { 1.0 };
    let sy = if all_zero { h / total_row_emu } else { scale };
    let mut col_x = vec![0.0];
    for cw in &t.col_widths_emu {
        col_x.push(col_x.last().copied().unwrap_or(0.0) + cw * sx);
    }
    let mut row_y = vec![0.0];
    for rh in &row_hs {
        row_y.push(row_y.last().copied().unwrap_or(0.0) + rh * sy);
    }
    let tw = col_x.last().copied().unwrap_or(0.0);
    let th = row_y.last().copied().unwrap_or(0.0);
    let mut cells_at: Vec<CellAt<'_>> = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate() {
            if cell.h_merge.unwrap_or(false) || cell.v_merge.unwrap_or(false) || ci + 1 >= col_x.len() {
                continue;
            }
            let span = cell.grid_span.unwrap_or(1.0);
            let rspan = cell.row_span.unwrap_or(1.0);
            let cx0 = x + col_x[ci];
            let cy0 = y + row_y[ri];
            let ce =
                usize_of(math::min(ci as f64 + span, (col_x.len() - 1) as f64)).and_then(|i| col_x.get(i)).copied().unwrap_or(f64::NAN);
            let re =
                usize_of(math::min(ri as f64 + rspan, (row_y.len() - 1) as f64)).and_then(|i| row_y.get(i)).copied().unwrap_or(f64::NAN);
            cells_at.push(CellAt { cell, x: cx0, y: cy0, w: ce - col_x[ci], h: re - row_y[ri] });
        }
    }
    let bg = bg_paint(t);
    let xs: Vec<f64> = col_x.iter().map(|v| x + v).collect();
    let ys: Vec<f64> = row_y.iter().map(|v| y + v).collect();
    if let Some(sd) = t.shadow.as_ref() {
        let mut rects: Vec<[f64; 4]> = if bg.is_some() {
            vec![[x, y, tw, th]]
        } else {
            cells_at.iter().filter(|e| cell_paint(e.cell).is_some()).map(|e| [e.x, e.y, e.w, e.h]).collect()
        };
        if let (Some(hl), Some(vl)) = (t.h_lines.as_ref(), t.v_lines.as_ref()) {
            for s in table_line_segs(hl, vl, &xs, &ys, scale) {
                rects.push(if s.vertical {
                    [s.x1 - s.w / 2.0, s.y1, s.w, s.y2 - s.y1]
                } else {
                    [s.x1, s.y1 - s.w / 2.0, s.x2 - s.x1, s.w]
                });
            }
        }
        if !rects.is_empty() {
            paint_rects_shadow(cx, &rects, sd, x, y, tw, th, scale);
        }
    }
    if let Some(p) = bg.as_ref() {
        paint_table_paint(cx, p, [x, y, tw, th], [x, y, tw, th], scale, fit);
    }
    for e in &cells_at {
        if let Some(p) = cell_paint(e.cell) {
            paint_table_paint(cx, &p, [e.x, e.y, e.w, e.h], [x, y, tw, th], scale, fit);
        }
    }
    if let (Some(hl), Some(vl)) = (t.h_lines.as_ref(), t.v_lines.as_ref()) {
        for s in table_line_segs(hl, vl, &xs, &ys, scale) {
            stroke_table_line(cx, &s);
        }
    }
    let neutral = Shape { id: "".into(), name: "tc".into(), ..Shape::default() };
    for e in &cells_at {
        if e.cell.tx_body.is_some()
            && let Some(body) = cell_body(e.cell)
        {
            paint_text(cx, &neutral, &body, e.x, e.y, e.w, e.h, scale, false)?;
        }
    }
    Ok(())
}

fn usize_of(v: f64) -> Option<usize> {
    uo_jsnum::conv::js_index(v, usize::MAX)
}

/// `paintTablePaint`.
fn paint_table_paint(cx: &mut Cx<'_>, p: &TablePaint, r: [f64; 4], t: [f64; 4], scale: f64, fit: &Fit) {
    let [rx, ry, rw, rh] = r;
    if !(rw > 0.0) || !(rh > 0.0) {
        return;
    }
    cx.save();
    if let Some(a) = p.alpha
        && a < 1.0
    {
        cx.global_alpha_mul(a);
    }
    let f = p.fill.as_ref();
    if let Some(ir) = p.image.as_ref() {
        if let Some(img) = cx.image(ir) {
            if let Some(a) = ir.fx.as_deref().and_then(|f| f.alpha) {
                cx.global_alpha_mul(a);
            }
            if p.tile {
                if let Some(pat) = image_tile(cx, &img, ir, t[0], t[1], scale, t[2], t[3]) {
                    cx.fill_style(pat);
                    cx.fill_rect(rx, ry, rw, rh);
                }
            } else {
                cx.begin_path();
                cx.path(vec![PathOp::Rect(Rect4 { x: rx, y: ry, w: rw, h: rh })]);
                cx.clip(None);
                let q = inset_rect(ir.fill_rect.as_ref(), rx, ry, rw, rh);
                cx.draw_image(img.src, None, q);
            }
        }
    } else if let Some(pf) = f.and_then(|f| f.pattern.as_deref()) {
        match preset_pattern(cx, pf, fit.ox, fit.oy, scale) {
            Some(pat) => cx.fill_style(pat),
            None => cx.fill_css(f.and_then(|f| f.color.as_ref()).map_or("#000000", |c| c.as_str())),
        }
        cx.fill_rect(rx, ry, rw, rh);
    } else if let Some(f) = f.filter(|f| is_gradient(f)) {
        let g = make_linear_gradient(cx, f, rx, ry, rw, rh);
        cx.fill_style(g);
        cx.fill_rect(rx, ry, rw, rh);
    } else if let Some(f) = f.filter(|f| f.kind == FillKind::Solid && f.color.as_ref().is_some_and(|c| !c.as_str().is_empty())) {
        cx.fill_css(f.color.as_ref().map_or("", |c| c.as_str()));
        cx.fill_rect(rx, ry, rw, rh);
    }
    cx.restore();
}

/// `paintRectsShadow`: the union of rectangles as an off-screen copy whose shadow comes back.
#[allow(clippy::too_many_arguments)]
fn paint_rects_shadow(cx: &mut Cx<'_>, rects: &[[f64; 4]], sd: &OuterShadow, bx: f64, by: f64, bw: f64, bh: f64, scale: f64) {
    let a = sd.dir_deg * std::f64::consts::PI / 180.0;
    cx.save();
    cx.shadow_color(&rgba_of(sd.color.as_str(), sd.alpha));
    cx.shadow_blur(sd.blur_rad * scale * SHADOW_BLUR_K);
    cx.shadow_offset(
        CtmOperand { add: sd.dist * math::cos(a) * scale, k: K, axis: CtmAxis::A },
        CtmOperand { add: sd.dist * math::sin(a) * scale, k: K, axis: CtmAxis::B },
    );
    cx.translate(-K, 0.0);
    if sd.sx.is_some() || sd.sy.is_some() {
        let al = sd.algn.as_deref().unwrap_or("b");
        let ax = bx
            + if matches!(al, "tl" | "l" | "bl") {
                0.0
            } else if matches!(al, "tr" | "r" | "br") {
                bw
            } else {
                bw / 2.0
            };
        let ay = by
            + if al.starts_with('t') {
                0.0
            } else if al.starts_with('b') {
                bh
            } else {
                bh / 2.0
            };
        cx.translate(ax, ay);
        cx.scale(sd.sx.unwrap_or(1.0), sd.sy.unwrap_or(1.0));
        cx.translate(-ax, -ay);
    }
    cx.fill_css("#000000");
    cx.begin_path();
    cx.path(rects.iter().map(|r| PathOp::Rect(Rect4 { x: r[0], y: r[1], w: r[2], h: r[3] })).collect());
    cx.fill(None);
    cx.restore();
}

struct Seg<'a> {
    l: &'a TableLine,
    w: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    vertical: bool,
}

fn grid_get(g: &LineGrid, r: i64, c: i64) -> Option<&TableLine> {
    let r = usize::try_from(r).ok()?;
    let c = usize::try_from(c).ok()?;
    if r >= usize::try_from(g.rows).ok()? || c >= usize::try_from(g.cols).ok()? {
        return None;
    }
    g.cells.get(r * usize::try_from(g.cols).ok()? + c)?.as_ref()
}

fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

/// `tableLineSegs`: the straight pieces of the border grid, the ends advanced, the thickness in px.
fn table_line_segs<'a>(hl: &'a LineGrid, vl: &'a LineGrid, xs: &[f64], ys: &[f64], scale: f64) -> Vec<Seg<'a>> {
    let mut out = Vec::new();
    let n_r = ys.len().saturating_sub(1);
    let n_c = xs.len().saturating_sub(1);
    let px = |l: Option<&TableLine>| -> f64 {
        match l {
            Some(l) => math::max(1.0, if l.width * scale < 1.5 { 1.0 } else { l.width * scale }),
            None => 0.0,
        }
    };
    let same = |a: &TableLine, b: Option<&TableLine>| -> bool {
        b.is_some_and(|b| {
            a.color == b.color && a.width == b.width && a.alpha == b.alpha && a.dash.is_none() && b.dash.is_none() && a.cmpd == b.cmpd
        })
    };
    let half_h = |r: i64, c: i64| math::max(px(grid_get(hl, r, c - 1)), px(grid_get(hl, r, c))) / 2.0;
    let half_v = |r: i64, c: i64| math::max(px(grid_get(vl, r - 1, c)), px(grid_get(vl, r, c))) / 2.0;
    let i = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
    for c in 0..=n_c {
        let mut r = 0;
        while r < n_r {
            let Some(l) = grid_get(vl, i(r), i(c)) else {
                r += 1;
                continue;
            };
            let mut r2 = r + 1;
            while r2 < n_r && same(l, grid_get(vl, i(r2), i(c))) {
                r2 += 1;
            }
            out.push(Seg {
                l,
                w: px(Some(l)),
                x1: at(xs, c),
                y1: at(ys, r) - half_h(i(r), i(c)),
                x2: at(xs, c),
                y2: at(ys, r2) + half_h(i(r2), i(c)),
                vertical: true,
            });
            r = r2;
        }
    }
    for r in 0..=n_r {
        let mut c = 0;
        while c < n_c {
            let Some(l) = grid_get(hl, i(r), i(c)) else {
                c += 1;
                continue;
            };
            let mut c2 = c + 1;
            while c2 < n_c && same(l, grid_get(hl, i(r), i(c2))) {
                c2 += 1;
            }
            out.push(Seg {
                l,
                w: px(Some(l)),
                x1: at(xs, c) - half_v(i(r), i(c)),
                y1: at(ys, r),
                x2: at(xs, c2) + half_v(i(r), i(c2)),
                y2: at(ys, r),
                vertical: false,
            });
            c = c2;
        }
    }
    out
}

fn stripes(cmpd: &str) -> Option<&'static [(f64, f64)]> {
    match cmpd {
        "dbl" => Some(&[(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0)]),
        "thickThin" => Some(&[(0.0, 0.6), (0.8, 1.0)]),
        "thinThick" => Some(&[(0.0, 0.2), (0.4, 1.0)]),
        "tri" => Some(&[(0.0, 1.0 / 6.0), (1.0 / 3.0, 2.0 / 3.0), (5.0 / 6.0, 1.0)]),
        _ => None,
    }
}

/// `strokeTableLine`: dashes and stripes (compound lines) with butt ends.
fn stroke_table_line(cx: &mut Cx<'_>, s: &Seg<'_>) {
    let l = s.l;
    cx.save();
    match l.alpha {
        Some(a) => cx.stroke_css(&rgba_of(l.color.as_str(), a)),
        None => cx.stroke_css(l.color.as_str()),
    }
    cx.push(SItem::LineCap(Cap::Butt));
    let st = l.cmpd.as_deref().filter(|c| !c.is_empty()).and_then(stripes);
    if let Some(d) = l.dash.as_deref().filter(|d| !d.is_empty()) {
        cx.line_dash(&dash_pattern(d, s.w));
    }
    let one: &[(f64, f64)] = &[(0.0, 1.0)];
    for &(a, b) in st.unwrap_or(one) {
        let off = ((a + b) / 2.0 - 0.5) * s.w;
        cx.line_width((b - a) * s.w);
        cx.begin_path();
        let (p0, p1) = if s.vertical {
            (uo_render::slides::Pair { x: s.x1 + off, y: s.y1 }, uo_render::slides::Pair { x: s.x2 + off, y: s.y2 })
        } else {
            (uo_render::slides::Pair { x: s.x1, y: s.y1 + off }, uo_render::slides::Pair { x: s.x2, y: s.y2 + off })
        };
        cx.path(vec![PathOp::MoveTo(p0), PathOp::LineTo(p1)]);
        cx.stroke();
    }
    cx.restore();
}
