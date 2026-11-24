//! Rich text cells: `paintCellContent`'s rich branch, `richRunStyle` (`universal-sheet.ts:3935-3980`) and `paintRichText`
//! (`cell-paint.ts`), each run with its own font, colour, decorations and script shift.

use uo_calc::store::{RawCell, ValueTag};
use uo_jsnum::math::{abs, floor, max, min, round};
use uo_render::slides::{Baseline, FillRule, SItem};

use super::region::{Dc, Rg};
use super::text::{Fnt, HA, TextScratch, Underline, draw_text, effective_halign};
use crate::grid::paint_rules::layout_rich_lines;
use crate::grid::style::{HAlign, VAlign};
use crate::grid::text::{CellKind, FontMetrics, cell_margin_px};
use crate::grid::{CellPaint, FontRequest, display_text, excel_row_px, text_input};
use crate::read::model::{Font, RunProps};

/// One run of a rich string: its text and style.
struct RunSt<'a> {
    m: Fnt<'a>,
    color: u32,
    underline: Option<Underline>,
    strike: bool,
    shift: f64,
    line_h: f64,
}

fn runs_of(g: &Rg<'_>, cell: RawCell) -> Option<Vec<(Vec<u16>, Option<u32>)>> {
    if cell.kind.tag() != ValueTag::Sst {
        return None;
    }
    let i = u32::try_from(cell.payload).ok()?;
    let rich = g.book.store.sst.get(i as usize)?.as_ref()?.rich?;
    let runs = g.book.store.rich_runs(rich)?;
    if runs.is_empty() {
        return None;
    }
    Some(runs.iter().map(|r| (r.text.as_js().units().collect::<Vec<u16>>(), r.props)).collect())
}

fn script_size(size: f64, va: bool) -> f64 {
    if va { max(1.0, round(size * 2.0 / 3.0 * 2.0) / 2.0) } else { size }
}

/// Paints a rich cell; false when the cell has no runs.
pub(super) fn paint_rich(g: &mut Rg<'_>, dc: &Dc, st: CellPaint, cell: RawCell) -> bool {
    let Some(runs) = runs_of(g, cell) else { return false };
    let (col0, row0) = (dc.dcol - 1, dc.drow - 1);
    let xf = g.styles.xf(crate::grid::cell_xf_index(&g.book.store, g.ix, row0, col0, cell));
    // `formatCell(cell, numFmtCode).text === ''`
    let ctx = crate::grid::CellCtx {
        date1904: g.book.workbook.date1904,
        show_zeros: !g.ws.show_zeros_off,
        lang: crate::grid::FormulaLang::PtBr,
        locale: None,
    };
    let mut scratch = Vec::new();
    if let crate::grid::CellRead::Input(inp) = text_input(&g.book.store, g.ix, row0, col0, cell, &xf.code16, &ctx, &mut scratch) {
        if display_text(&inp).is_empty() {
            return true;
        }
    } else {
        return true;
    }
    let base: Option<&Font> = xf.font.and_then(|f| g.book.styles.fonts.get(f as usize));
    let nf = g.styles.normal.clone();
    let zoom = g.zoom;
    // the font of each run
    struct Spec {
        font_id: crate::grid::FontId,
        bold: bool,
        italic: bool,
        ppem: i32,
        name: String,
        alt: Option<String>,
        color: Option<crate::grid::paint_rules::Color>,
        ucol: Option<String>,
        underline: bool,
        strike: bool,
        shift: f64,
        line_h: f64,
    }
    let mut specs: Vec<Spec> = Vec::with_capacity(runs.len());
    for (_, props) in &runs {
        let p: Option<&RunProps> = props.and_then(|id| g.book.run_props.get(id));
        let name = p.and_then(|p| p.font.clone()).or_else(|| base.and_then(|b| b.name.clone())).unwrap_or_else(|| nf.name.clone());
        let alt = if p.is_some_and(|p| p.font.is_some()) { None } else { base.and_then(|b| b.alt_name.clone()) };
        let bold = p.and_then(|p| p.bold).unwrap_or(st.font.bold);
        let italic = p.and_then(|p| p.italic).unwrap_or(st.font.italic);
        let size = p.and_then(|p| p.size).or_else(|| base.and_then(|b| b.size)).unwrap_or(11.0);
        let va = p.and_then(|p| p.vert_align).or_else(|| base.and_then(|b| b.vert_align));
        let underline = p.and_then(|p| p.underline).unwrap_or(st.font.underline);
        let strike = p.and_then(|p| p.strike).unwrap_or(st.font.strike);
        let id = g.measure.font(
            &FontRequest { name: &name, alt_name: alt.as_deref(), size_pt: script_size(size, va.is_some()), bold, italic, zoom },
            g.env,
        );
        let m = g.measure.metrics(id);
        let ppem = m.ppem();
        let shift = match va {
            Some(v) => {
                let pp = size * 96.0 / 72.0 * zoom;
                round(if v == "superscript" { -pp * 0.33 } else { pp * 0.14 })
            }
            None => 0.0,
        };
        let line_h = f64::from(excel_row_px(Some(&name), size, bold)) * zoom;
        specs.push(Spec {
            font_id: id,
            bold,
            italic,
            ppem,
            name,
            alt,
            color: None,
            ucol: p.and_then(|p| p.color.clone()),
            underline,
            strike,
            shift,
            line_h,
        });
    }
    let base_color = st.font.color;
    let ustyle = Underline::of(base.and_then(|b| b.underline_style));
    // the style of each run for the painter
    let mut rs: Vec<RunSt<'_>> = Vec::with_capacity(specs.len());
    let mut colors: Vec<u32> = Vec::with_capacity(specs.len());
    for sp in &specs {
        let c = match (&sp.ucol, base_color) {
            (Some(c), _) => g.o.sym(c),
            (None, Some(c)) => g.o.color_sym(c, || g.pbook.palette().text(c)),
            (None, None) => g.o.sym(super::chrome::TEXT),
        };
        colors.push(c);
        let _ = sp.color;
    }
    // fonts need `&mut g.o` for faces; the metrics borrow `g.measure` immutably afterwards
    let mut faces: Vec<u32> = Vec::with_capacity(specs.len());
    for sp in &specs {
        faces.push(g.o.face(sp.font_id.0, &sp.name, sp.alt.as_deref(), sp.bold, sp.italic));
    }
    let indent_px = {
        let id = g
            .measure
            .font(&FontRequest { name: &nf.name, alt_name: None, size_pt: nf.size, bold: nf.bold, italic: false, zoom: 1.0 }, g.env);
        3.0 * f64::from(g.measure.metrics(id).adv(32)) * zoom
    };
    let row_desc = round(f64::from(g.grid.row_desc(dc.drow, g.styles)) * zoom);
    for (k, sp) in specs.iter().enumerate() {
        let m = g.measure.metrics(sp.font_id);
        let (asc, dsc) = m.hg_box();
        let (xa, xd) = m.xl_ascent_descent();
        rs.push(RunSt {
            m: Fnt { m, face: faces[k], ppem: sp.ppem, italic: sp.italic, ascent: asc, descent: dsc, xl_ascent: xa, xl_descent: xd },
            color: colors[k],
            underline: sp.underline.then_some(ustyle),
            strike: sp.strike,
            shift: sp.shift,
            line_h: sp.line_h,
        });
    }
    let texts: Vec<&[u16]> = runs.iter().map(|(t, _)| t.as_slice()).collect();
    let (c, r) = (dc.dcol, dc.drow);
    let (book, ws, grid, xs, styles, ix) = (g.book, g.ws, g.grid, g.xs, g.styles, g.ix);
    let cctx = crate::grid::CellCtx {
        date1904: book.workbook.date1904,
        show_zeros: !ws.show_zeros_off,
        lang: crate::grid::FormulaLang::PtBr,
        locale: None,
    };
    let mut ovf = |dir: i32, need: f64| super::region::overflow_extent(&book.store, ws, grid, xs, styles, cctx, ix, c, r, dir, need);
    let a = RichArgs {
        left: dc.left,
        top: dc.top,
        w: dc.w,
        h: dc.h,
        halign: xf.align,
        valign: xf.valign,
        wrap: xf.wrap,
        indent: xf.indent,
        indent_px,
        merged: dc.merged,
        row_desc,
    };
    paint_rich_text(&mut g.o, g.sc, &rs, &texts, &a, &mut ovf);
    true
}

struct RichArgs {
    left: f64,
    top: f64,
    w: f64,
    h: f64,
    halign: HAlign,
    valign: VAlign,
    wrap: bool,
    indent: f64,
    indent_px: f64,
    merged: bool,
    row_desc: f64,
}

fn center_x(left: f64, w: f64, big_w: f64) -> f64 {
    left + 1.0 + floor((w - big_w) / 2.0)
}

/// `paintRichText`.
fn paint_rich_text(
    o: &mut super::out::Out<'_>,
    sc: &mut TextScratch,
    st: &[RunSt<'_>],
    texts: &[&[u16]],
    a: &RichArgs,
    overflow: &mut dyn FnMut(i32, f64) -> f64,
) {
    if st.is_empty() {
        return;
    }
    let f0 = &st[0].m;
    let margin = (cell_margin_px(f0.ppem) - 1.0) / 2.0;
    let box_l = a.left + 1.0 + margin;
    let box_r = a.left + a.w - margin;
    let box_w = max(0.0, box_r - box_l);
    let halign = effective_halign(a.halign, CellKind::Text);
    let indent = if a.indent > 0.0 && matches!(halign, HA::Left | HA::Right) { a.indent * a.indent_px } else { 0.0 };
    let measure = |t: &[u16], run: usize| -> f64 { st.get(run).map_or(0.0, |r| r.m.m.width(t, f64::INFINITY)) };
    o.text_baseline(Baseline::Alphabetic);
    let line_metrics = |segs: &[(&[u16], usize)]| -> (f64, f64, f64) {
        let (mut asc, mut desc, mut lh) = (0.0f64, 0.0f64, 0.0f64);
        for (_, run) in segs {
            let r = &st[*run];
            asc = max(asc, f64::from(r.m.xl_ascent) - min(0.0, r.shift));
            desc = max(desc, f64::from(r.m.xl_descent) + max(0.0, r.shift));
            lh = max(lh, r.line_h);
        }
        if segs.is_empty() {
            asc = f64::from(f0.xl_ascent);
            desc = f64::from(f0.xl_descent);
            lh = st[0].line_h;
        }
        (asc, desc, lh)
    };
    let x_for = |big_w: f64| -> f64 {
        match halign {
            HA::Right => box_r - big_w - indent,
            HA::Center | HA::CenterContinuous => center_x(a.left, a.w, big_w),
            _ => box_l + indent,
        }
    };
    let draw_line = |o: &mut super::out::Out<'_>, sc: &mut TextScratch, segs: &[(&[u16], usize)], baseline: f64, x_start: f64| {
        let mut x = x_start;
        for (text, run) in segs {
            let r = &st[*run];
            let sw = r.m.m.width(text, f64::INFINITY);
            o.fill_style_sym(r.color);
            draw_text(o, sc, &r.m, text, x, baseline + r.shift, f64::NEG_INFINITY, f64::INFINITY, false);
            if r.underline.is_some() || r.strike {
                let t = max(1.0, round(f64::from(r.m.ppem) / 16.0));
                if let Some(u) = r.underline {
                    let u1 = baseline + r.shift + max(1.0, round(f64::from(r.m.descent) * 0.35));
                    o.fill_rect(round(x), round(u1), round(sw), t);
                    if matches!(u, Underline::Double | Underline::DoubleAccounting) {
                        o.fill_rect(round(x), round(u1) + t + 1.0, round(sw), t);
                    }
                }
                if r.strike {
                    o.fill_rect(round(x), round(baseline + r.shift - f64::from(r.m.ascent) * 0.33), round(sw), t);
                }
            }
            x += sw;
        }
    };
    let va = a.valign;
    if a.wrap {
        let mut meas = |t: &[u16], run: usize| measure(t, run);
        let lines = layout_rich_lines(texts, max(1.0, box_w - indent), &mut meas);
        let lines_ref: Vec<Vec<(&[u16], usize)>> = lines.iter().map(|l| l.iter().map(|s| (s.text.as_slice(), s.run)).collect()).collect();
        let ms: Vec<(f64, f64, f64)> = lines_ref.iter().map(|l| line_metrics(l)).collect();
        let n = ms.len();
        let total: f64 = ms.iter().enumerate().fold(0.0, |s, (i, m)| s + if i + 1 < n { m.2 } else { m.0 + m.1 });
        let mut y = match va {
            VAlign::Top => a.top,
            VAlign::Center => a.top + floor((a.h - total) / 2.0),
            _ => a.top + a.h - total - max(0.0, a.row_desc - ms.last().map_or(0.0, |m| m.1)),
        };
        o.save();
        o.begin_path();
        o.rect(a.left + 1.0, a.top + 1.0, max(0.0, a.w - 1.0), max(0.0, a.h - 1.0));
        o.push(SItem::Clip(FillRule::NonZero));
        for (i, segs) in lines_ref.iter().enumerate() {
            let m = ms[i];
            let big_w: f64 = segs.iter().fold(0.0, |s, g| s + measure(g.0, g.1));
            draw_line(o, sc, segs, y + m.0, x_for(big_w));
            y += m.2;
        }
        o.restore();
        return;
    }
    let segs: Vec<(&[u16], usize)> = texts.iter().enumerate().filter(|(_, t)| !t.is_empty()).map(|(i, t)| (*t, i)).collect();
    if segs.is_empty() {
        return;
    }
    let big_w: f64 = segs.iter().fold(0.0, |s, g| s + measure(g.0, g.1));
    let m = line_metrics(&segs);
    let baseline = match va {
        VAlign::Top => a.top + m.0,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => a.top + floor((a.h - (m.0 + m.1)) / 2.0) + m.0,
        _ => a.top + a.h - max(m.1, a.row_desc),
    };
    let (mut clip_l, mut clip_r) = (a.left, a.left + a.w);
    if !a.merged && big_w > box_w - indent {
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
    o.save();
    o.begin_path();
    o.rect(clip_l + 1.0, a.top + 1.0, max(0.0, clip_r - clip_l - 1.0), max(0.0, a.h - 1.0));
    o.push(SItem::Clip(FillRule::NonZero));
    draw_line(o, sc, &segs, baseline, x_for(big_w));
    o.restore();
    let _ = abs(0.0);
}
