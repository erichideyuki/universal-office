//! `styles.ts`: `xl/styles.xml` as the model the renderer needs (fonts, fills, borders, cellXfs, number formats,
//! dxfs, custom table styles), colors resolved against the theme.

use super::model::{
    Border, BorderEdge, CellXf, ColorContext, Dxf, Fill, Font, Gradient, GradientStop, Styles, TableStyleDef, TableStyleElementDef,
};
use super::theme::resolve_color_el;
use super::{El, off, on, owned, pf, pi};

/// `ooxmlFlag` of an optional element.
pub(crate) fn ooxml_flag(el: Option<El<'_>>) -> Option<bool> {
    el.map(|e| !matches!(e.attr("val"), Some("0" | "false" | "none")))
}

/// `underlineOf`: the style when it is not plain `single`; `Some(None)` = single, `None` = no underline.
fn underline_of(el: Option<El<'_>>) -> Option<Option<&'static str>> {
    let e = el?;
    Some(match e.attr("val").unwrap_or("single") {
        "double" => Some("double"),
        "singleAccounting" => Some("singleAccounting"),
        "doubleAccounting" => Some("doubleAccounting"),
        "none" => return None,
        _ => None,
    })
}

fn bool_el(el: El<'_>, name: &str) -> bool {
    ooxml_flag(el.first_child(name)).unwrap_or(false)
}

fn color_of(el: El<'_>, cx: &ColorContext) -> Option<String> {
    el.first_child("color").and_then(|c| resolve_color_el(c, cx))
}

/// `readBorder` (cell borders and dxf borders).
pub fn read_border(bd: El<'_>, cx: &ColorContext) -> Border {
    let mut border = Border::default();
    for side in ["left", "right", "top", "bottom", "diagonal", "vertical", "horizontal"] {
        let e = bd.first_child(side).or_else(|| match side {
            "left" => bd.first_child("start"),
            "right" => bd.first_child("end"),
            _ => None,
        });
        let Some(e) = e else { continue };
        let style = e.attr("style").unwrap_or("");
        if style.is_empty() || style == "none" {
            continue;
        }
        let color = e.first_child("color").and_then(|c| resolve_color_el(c, cx)).unwrap_or_else(|| "#000000".to_owned());
        let edge = Some(BorderEdge { style: style.to_owned(), color });
        match side {
            "left" => border.left = edge,
            "right" => border.right = edge,
            "top" => border.top = edge,
            "bottom" => border.bottom = edge,
            "diagonal" => border.diagonal = edge,
            "vertical" => border.vertical = edge,
            _ => border.horizontal = edge,
        }
    }
    border.diagonal_up = on(bd.attr("diagonalUp"));
    border.diagonal_down = on(bd.attr("diagonalDown"));
    border
}

/// `readFill`.
pub fn read_fill(fl: El<'_>, cx: &ColorContext) -> Fill {
    let mut fill = Fill::default();
    if let Some(pf_el) = fl.first_child("patternFill") {
        let pt = pf_el.attr("patternType");
        fill.pattern = owned(pt);
        let fg = pf_el.first_child("fgColor").and_then(|c| resolve_color_el(c, cx));
        let bg = pf_el.first_child("bgColor").and_then(|c| resolve_color_el(c, cx));
        if pt == Some("solid") {
            fill.fg_color = Some(fg.unwrap_or_else(|| "#000000".to_owned()));
        } else if pt.is_some() && pt != Some("none") {
            fill.fg_color = Some(fg.unwrap_or_else(|| "#000000".to_owned()));
            fill.bg_color = Some(bg.unwrap_or_else(|| "#FFFFFF".to_owned()));
        }
        return fill;
    }
    if let Some(gf) = fl.first_child("gradientFill") {
        let stops = gf
            .children_named("stop")
            .map(|stop| GradientStop {
                position: pf(stop.attr("position").unwrap_or("0")),
                color: stop.first_child("color").and_then(|c| resolve_color_el(c, cx)).unwrap_or_else(|| "#FFFFFF".to_owned()),
            })
            .collect();
        let num = |k: &str| {
            let v = pf(gf.attr(k).unwrap_or("0"));
            if v.is_finite() { v } else { 0.0 }
        };
        let path = (gf.attr("type") == Some("path")).then(|| [num("left"), num("right"), num("top"), num("bottom")]);
        fill.gradient = Some(Gradient { degree: num("degree"), stops, path });
    }
    fill
}

/// `readDxf`.
pub fn read_dxf(d: El<'_>, cx: &ColorContext) -> Dxf {
    let mut dxf = Dxf::default();
    let fill = d.first_child("fill");
    let pf_el = fill.and_then(|f| f.first_child("patternFill"));
    if let Some(pf_el) = pf_el {
        let pt = pf_el.attr("patternType");
        let fg = pf_el.first_child("fgColor").and_then(|c| resolve_color_el(c, cx));
        let bg = pf_el.first_child("bgColor").and_then(|c| resolve_color_el(c, cx));
        if pt == Some("none") {
            dxf.fill = Some(Fill { pattern: Some("none".to_owned()), ..Fill::default() });
        } else if pt.is_none() || pt == Some("solid") {
            if let Some(c) = bg.or(fg) {
                dxf.fill = Some(Fill { pattern: Some("solid".to_owned()), fg_color: Some(c.clone()), ..Fill::default() });
                dxf.fill_color = Some(c);
            }
        } else {
            dxf.fill = Some(Fill { pattern: owned(pt), fg_color: fg, bg_color: bg, gradient: None });
        }
    } else if let Some(f) = fill
        && f.first_child("gradientFill").is_some()
    {
        dxf.fill = Some(read_fill(f, cx));
    }
    if let Some(font) = d.first_child("font") {
        if font.first_child("b").is_some() {
            dxf.bold = Some(bool_el(font, "b"));
        }
        if font.first_child("i").is_some() {
            dxf.italic = Some(bool_el(font, "i"));
        }
        if font.first_child("strike").is_some() {
            dxf.strike = Some(bool_el(font, "strike"));
        }
        if let Some(u) = font.first_child("u") {
            dxf.underline = Some(u.attr("val") != Some("none"));
        }
        if let Some(c) = font.first_child("color") {
            dxf.font_color = resolve_color_el(c, cx);
        }
    }
    if let Some(bd) = d.first_child("border") {
        let b = read_border(bd, cx);
        if !b.is_empty() {
            dxf.border = Some(b);
        }
    }
    if let Some(nf) = d.first_child("numFmt") {
        dxf.num_fmt_id = nf.attr("numFmtId").map(pi);
        dxf.num_fmt_code = owned(nf.attr("formatCode"));
    }
    dxf
}

/// `readStyles` over the parsed root of `xl/styles.xml` (`None` = the part is missing: an empty model).
pub fn read_styles(root: Option<El<'_>>, cx: &ColorContext) -> Styles {
    let Some(root) = root else { return Styles::default() };
    let mut fonts = Vec::new();
    if let Some(fonts_el) = root.first_child("fonts") {
        for f in fonts_el.children_named("font") {
            let mut font = Font { bold: bool_el(f, "b"), italic: bool_el(f, "i"), strike: bool_el(f, "strike"), ..Font::default() };
            if let Some(u) = underline_of(f.first_child("u")) {
                font.underline = true;
                font.underline_style = u;
            }
            font.size = f.first_child("sz").and_then(|s| s.attr("val")).map(pf);
            font.color = color_of(f, cx);
            font.name = owned(f.first_child("name").and_then(|n| n.attr("val")));
            font.vert_align = match f.first_child("vertAlign").and_then(|v| v.attr("val")) {
                Some("superscript") => Some("superscript"),
                Some("subscript") => Some("subscript"),
                _ => None,
            };
            let themed = match f.first_child("scheme").and_then(|s| s.attr("val")) {
                Some("minor") => cx.theme.minor.as_ref(),
                Some("major") => cx.theme.major.as_ref(),
                _ => None,
            };
            if let Some(t) = themed {
                if let Some(n) = &font.name
                    && !n.is_empty()
                    && n != t
                {
                    font.alt_name = Some(n.clone());
                }
                font.name = Some(t.clone());
            }
            fonts.push(font);
        }
    }
    let fills = root.first_child("fills").map(|e| e.children_named("fill").map(|fl| read_fill(fl, cx)).collect()).unwrap_or_default();
    let borders = root.first_child("borders").map(|e| e.children_named("border").map(|b| read_border(b, cx)).collect()).unwrap_or_default();
    let dxfs = root.first_child("dxfs").map(|e| e.children_named("dxf").map(|d| read_dxf(d, cx)).collect()).unwrap_or_default();

    let mut table_styles: Option<Vec<TableStyleDef>> = None;
    let (mut default_table_style, mut default_pivot_style) = (None, None);
    if let Some(ts) = root.first_child("tableStyles") {
        default_table_style = owned(ts.attr("defaultTableStyle")).filter(|s| !s.is_empty());
        default_pivot_style = owned(ts.attr("defaultPivotStyle")).filter(|s| !s.is_empty());
        for t in ts.children_named("tableStyle") {
            let name = t.attr("name").unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let mut elements = Vec::new();
            for e in t.children_named("tableStyleElement") {
                let ty = e.attr("type").unwrap_or("");
                if ty.is_empty() {
                    continue;
                }
                elements.push(TableStyleElementDef {
                    r#type: ty.to_owned(),
                    dxf_id: e.attr("dxfId").map(pi),
                    size: e.attr("size").map(pi),
                });
            }
            table_styles.get_or_insert_with(Vec::new).push(TableStyleDef {
                name: name.to_owned(),
                pivot: t.attr("pivot") != Some("0"),
                table: t.attr("table") != Some("0"),
                elements,
            });
        }
    }

    let mut num_fmts: Vec<(f64, String)> = Vec::new();
    if let Some(nfs) = root.first_child("numFmts") {
        for nf in nfs.children_named("numFmt") {
            if let (Some(id), Some(code)) = (nf.attr("numFmtId"), nf.attr("formatCode")) {
                let mut key = pi(id);
                if key == 0.0 {
                    key = 0.0;
                }
                match num_fmts.iter_mut().find(|(k, _)| *k == key || (k.is_nan() && key.is_nan())) {
                    Some(slot) => slot.1 = code.to_owned(),
                    None => num_fmts.push((key, code.to_owned())),
                }
            }
        }
    }

    let mut cell_xfs = Vec::new();
    if let Some(xfs) = root.first_child("cellXfs") {
        for xf in xfs.children_named("xf") {
            let at = |k: &str| xf.attr(k);
            let mut rec = CellXf {
                num_fmt_id: pi(at("numFmtId").unwrap_or("0")),
                font_id: pi(at("fontId").unwrap_or("0")),
                fill_id: pi(at("fillId").unwrap_or("0")),
                border_id: pi(at("borderId").unwrap_or("0")),
                xf_id: pi(at("xfId").unwrap_or("0")),
                apply_number_format: at("applyNumberFormat") == Some("1"),
                apply_font: at("applyFont") == Some("1"),
                apply_fill: at("applyFill") == Some("1"),
                apply_border: at("applyBorder") == Some("1"),
                apply_alignment: at("applyAlignment") == Some("1"),
                pivot_button: on(at("pivotButton")),
                ..CellXf::default()
            };
            if let Some(al) = xf.first_child("alignment") {
                rec.horizontal = owned(al.attr("horizontal"));
                rec.vertical = owned(al.attr("vertical"));
                rec.wrap_text = on(al.attr("wrapText"));
                rec.indent = al.attr("indent").map(pi);
                let rot = al.attr("textRotation").map_or(0.0, pi);
                if rot > 0.0 && (rot <= 180.0 || rot == 255.0) {
                    rec.text_rotation = Some(rot);
                }
                rec.shrink_to_fit = on(al.attr("shrinkToFit"));
                rec.reading_order = al.attr("readingOrder").map(|v| {
                    let n = pi(v);
                    if n.is_nan() || n == 0.0 { 0.0 } else { n }
                });
                rec.justify_last_line = on(al.attr("justifyLastLine"));
            }
            cell_xfs.push(rec);
        }
    }
    let _ = off;
    Styles { num_fmts, cell_xfs, fonts, fills, borders, dxfs, table_styles, default_table_style, default_pivot_style }
}
