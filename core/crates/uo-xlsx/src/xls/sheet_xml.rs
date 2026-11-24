//! A worksheet of the `.xls` (`XlsSheet`) → `xl/worksheets/sheetN.xml` (+ the comments) (`sheet-xml.ts`). COLINFO and
//! STANDARDWIDTH come in 1/256 of a character WITH the padding — the unit of `<col width>`; ROW/DEFAULTROWHEIGHT in
//! twips (÷20 = points).

use std::collections::HashMap;

use super::cf::rule_xml;
use super::drawing::{MediaState, drawing_parts};
use super::globals::XlsGlobals;
use super::sheet::{NoteInfo, XlsSheet};
use super::styles_xml::fixed_num;
use super::xmlw::{NS_MAIN, NS_R, Parts, Rel, XML_HEAD, cell_ref, cell_ref_s, push_u, range_ref, xml_text};

pub struct SheetParts {
    pub xml: String,
    pub rels: Vec<Rel>,
    /// Extra parts of the package (drawing, media, charts).
    pub parts: Parts,
    pub overrides: Vec<(String, String)>,
    pub defaults: Vec<(String, String)>,
}

/// `fmtNum`: `String(+v.toFixed(6))`.
pub fn fmt_num(v: f64) -> String {
    fixed_num(v, 6)
}

/// `rowHeightPt`: a height in twips → points ADJUSTED TO THE PIXEL as Excel shows it at 96 dpi: px = floor(tw/15 + 0.1)
/// (measured: 764/764 custom rows of the corpus — 282 tw = 18.8 px → 18 px; 224 tw = 14.93 px → 15 px).
pub fn row_height_pt(tw: f64) -> f64 {
    uo_jsnum::math::floor(tw / 15.0 + 0.1) * 0.75
}

pub fn worksheet_xml<'a>(g: &'a XlsGlobals, sh: &XlsSheet, index: usize, media: &mut MediaState<'a>) -> SheetParts {
    let mut out = SheetParts { xml: String::new(), rels: Vec::new(), parts: Parts::default(), overrides: Vec::new(), defaults: Vec::new() };
    let cells = &sh.cells;

    // ── dimension: rows/columns with cells ──
    let (mut max_r, mut max_c, mut min_r, mut min_c) = (-1i64, -1i64, i64::MAX, i64::MAX);
    for c in &cells.list {
        let (r, cc) = (i64::from(c.row), i64::from(c.col));
        max_r = max_r.max(r);
        min_r = min_r.min(r);
        max_c = max_c.max(cc);
        min_c = min_c.min(cc);
    }

    // the cells' XML + the `<row>` tags (about 40 bytes each) + the rest of the sheet
    let n_rows = sh.rows.len().max(usize::try_from(max_r + 1).unwrap_or(0));
    let mut x = String::with_capacity(cells.arena.len() + n_rows * 40 + 8192);
    x.push_str(XML_HEAD);
    x.push_str(&format!("<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">"));
    x.push_str("<dimension ref=\"");
    if max_r >= 0 {
        let (a, b, c, d) = (
            u32::try_from(min_r).unwrap_or(0),
            u32::try_from(min_c).unwrap_or(0),
            u32::try_from(max_r).unwrap_or(0),
            u32::try_from(max_c).unwrap_or(0),
        );
        range_ref(&mut x, a, b, c, d);
    } else {
        x.push_str("A1");
    }
    x.push_str("\"/>");

    // ── view ──
    let mut view = String::from("<sheetView");
    if sh.show_formulas {
        view.push_str(" showFormulas=\"1\"");
    }
    if !sh.show_grid {
        view.push_str(" showGridLines=\"0\"");
    }
    if !sh.show_zeros {
        view.push_str(" showZeros=\"0\"");
    }
    if sh.right_to_left {
        view.push_str(" rightToLeft=\"1\"");
    }
    if sh.selected {
        view.push_str(" tabSelected=\"1\"");
    }
    if let Some(z) = sh.zoom.filter(|&z| z != 0 && z != 100) {
        view.push_str(&format!(" zoomScale=\"{z}\" zoomScaleNormal=\"{z}\""));
    }
    view.push_str(" workbookViewId=\"0\"");
    match sh.frozen.filter(|fz| fz.x > 0 || fz.y > 0) {
        Some(fz) => {
            let pane = if fz.x != 0 && fz.y != 0 {
                "bottomRight"
            } else if fz.y != 0 {
                "bottomLeft"
            } else {
                "topRight"
            };
            view.push_str("><pane");
            if fz.x != 0 {
                view.push_str(&format!(" xSplit=\"{}\"", fz.x));
            }
            if fz.y != 0 {
                view.push_str(&format!(" ySplit=\"{}\"", fz.y));
            }
            view.push_str(&format!(
                " topLeftCell=\"{}\" activePane=\"{pane}\" state=\"frozen\"/></sheetView>",
                cell_ref_s(fz.top.max(fz.y), fz.left.max(fz.x))
            ));
        }
        None => view.push_str("/>"),
    }
    x.push_str("<sheetViews>");
    x.push_str(&view);
    x.push_str("</sheetViews>");

    // ── grid defaults ──
    // without STANDARDWIDTH the default width comes from baseColWidth (the reader applies Excel's rule: base × MDW + 5 px,
    // rounded to a multiple of 8 px)
    let base = sh.def_col_width.unwrap_or(8);
    x.push_str(&format!("<sheetFormatPr baseColWidth=\"{base}\""));
    if let Some(w) = sh.std_width {
        x.push_str(&format!(" defaultColWidth=\"{}\"", fmt_num(f64::from(w) / 256.0)));
    }
    x.push_str(&format!(" defaultRowHeight=\"{}\"", fmt_num(row_height_pt(f64::from(sh.def_row_height.unwrap_or(255))))));
    if sh.def_row_custom {
        x.push_str(" customHeight=\"1\"");
    }
    if sh.def_row_hidden {
        x.push_str(" zeroHeight=\"1\"");
    }
    x.push_str("/>");

    // ── columns ──
    if !sh.cols.is_empty() {
        let mut cols: Vec<_> = sh.cols.iter().collect();
        cols.sort_by_key(|c| c.first);
        let mut cx = String::new();
        let mut last_max = -1i64;
        for c in cols {
            let first = i64::from(c.first).max(last_max + 1);
            if first > i64::from(c.last) {
                continue;
            }
            last_max = i64::from(c.last);
            // up to the last BIFF column (IV = 255, or 256 in the "rest of the row" record): it holds for the whole row —
            // a .xls opened in Excel has only 256 columns, so no "unformatted" column is left (the auto-fit does not use the
            // Normal font as a floor)
            let max = if c.last >= 255 { 16384 } else { i64::from(c.last) + 1 };
            cx.push_str(&format!(
                "<col min=\"{}\" max=\"{max}\" width=\"{}\" customWidth=\"1\"",
                first + 1,
                fmt_num(f64::from(c.width) / 256.0)
            ));
            if c.hidden {
                cx.push_str(" hidden=\"1\"");
            }
            if c.xf > 0 && usize::try_from(c.xf).unwrap_or(usize::MAX) < g.xfs.len() {
                cx.push_str(&format!(" style=\"{}\"", c.xf));
            }
            cx.push_str("/>");
        }
        if !cx.is_empty() {
            x.push_str("<cols>");
            x.push_str(&cx);
            x.push_str("</cols>");
        }
    }

    // ── rows: the union of the rows with a ROW and the rows with cells ──
    let n = cells.len();
    if n == 0 && sh.rows.is_empty() {
        x.push_str("<sheetData/>");
    } else {
        x.push_str("<sheetData>");
        let mut k = 0usize; // position in the (row, col) order
        let mut infos = sh.rows.iter().peekable();
        loop {
            let next_cell_row = cells.at(k).map(|c| u32::from(c.row));
            let next_info_row = infos.peek().map(|(r, _)| **r);
            let r = match (next_cell_row, next_info_row) {
                (None, None) => break,
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (Some(a), Some(b)) => a.min(b),
            };
            let info = if next_info_row == Some(r) { infos.next().map(|(_, i)| i) } else { None };
            x.push_str("<row r=\"");
            push_u(&mut x, u64::from(r) + 1);
            x.push('"');
            if let Some(info) = info {
                // the stored height adjusted to the pixel; customHeight = fUnsynced (without it the renderer redoes the
                // auto-fit by the font, as Excel does on open)
                if let Some(h) = info.height {
                    x.push_str(&format!(" ht=\"{}\"", fmt_num(row_height_pt(f64::from(h)))));
                }
                if info.custom {
                    x.push_str(" customHeight=\"1\"");
                }
                if info.thick_top {
                    x.push_str(" thickTop=\"1\"");
                }
                if info.thick_bot {
                    x.push_str(" thickBot=\"1\"");
                }
                if info.hidden {
                    x.push_str(" hidden=\"1\"");
                }
                if let Some(xf) = info.xf.filter(|&xf| xf > 0 && usize::try_from(xf).unwrap_or(usize::MAX) < g.xfs.len()) {
                    x.push_str(&format!(" s=\"{xf}\" customFormat=\"1\""));
                }
            }
            if next_cell_row == Some(r) {
                x.push('>');
                // the last write of a column wins
                while let Some(c) = cells.at(k) {
                    if u32::from(c.row) != r {
                        break;
                    }
                    let dup = cells.at(k + 1).is_some_and(|d| d.row == c.row && d.col == c.col);
                    if !dup {
                        cells.write_cell(cells.index_at(k), &mut x);
                    }
                    k += 1;
                }
                x.push_str("</row>");
            } else {
                x.push_str("/>");
            }
        }
        x.push_str("</sheetData>");
    }

    if let Some([r1, c1, r2, c2]) = sh.auto_filter {
        x.push_str("<autoFilter ref=\"");
        range_ref(&mut x, r1, c1, r2.max(r1), c2.max(c1));
        x.push_str("\"/>");
    }

    if !sh.merges.is_empty() {
        x.push_str(&format!("<mergeCells count=\"{}\">", sh.merges.len()));
        for &[r1, c1, r2, c2] in &sh.merges {
            x.push_str("<mergeCell ref=\"");
            range_ref(&mut x, r1, c1, r2, c2);
            x.push_str("\"/>");
        }
        x.push_str("</mergeCells>");
    }

    for cf in &sh.cond_formats {
        // CFEX with fActive = 0 turns the rule off
        let rules: Vec<_> = cf.rules.iter().filter(|r| !r.ty.is_empty()).collect();
        if !rules.is_empty() {
            x.push_str(&format!("<conditionalFormatting sqref=\"{}\">", cf.sqref));
            for r in rules {
                rule_xml(&mut x, r);
            }
            x.push_str("</conditionalFormatting>");
        }
    }

    if !sh.data_validations.is_empty() {
        x.push_str(&format!("<dataValidations count=\"{}\">", sh.data_validations.len()));
        for d in &sh.data_validations {
            x.push_str(d);
        }
        x.push_str("</dataValidations>");
    }

    if !sh.hyperlinks.is_empty() {
        let mut hx = String::new();
        for l in &sh.hyperlinks {
            hx.push_str("<hyperlink ref=\"");
            range_ref(&mut hx, l.r1, l.c1, l.r2.max(l.r1), l.c2.max(l.c1));
            hx.push('"');
            if let Some(url) = l.url.as_ref().filter(|u| !u.is_empty()) {
                let id = format!("rId{}", out.rels.len() + 1);
                out.rels.push(Rel { id: id.clone(), kind: "hyperlink", target: url.clone(), external: true });
                hx.push_str(&format!(" r:id=\"{id}\""));
            }
            if let Some(loc) = l.location.as_ref().filter(|u| !u.is_empty()) {
                hx.push_str(" location=\"");
                xml_text(&mut hx, loc.strip_prefix('#').unwrap_or(loc));
                hx.push('"');
            }
            if let Some(d) = l.display.as_ref().filter(|u| !u.is_empty()) {
                hx.push_str(" display=\"");
                xml_text(&mut hx, d);
                hx.push('"');
            }
            if let Some(t) = l.tooltip.as_ref().filter(|u| !u.is_empty()) {
                hx.push_str(" tooltip=\"");
                xml_text(&mut hx, t);
                hx.push('"');
            }
            hx.push_str("/>");
        }
        x.push_str("<hyperlinks>");
        x.push_str(&hx);
        x.push_str("</hyperlinks>");
    }

    if let Some(dr) = drawing_parts(g, sh, index, media) {
        let id = format!("rId{}", out.rels.len() + 1);
        out.rels.push(Rel { id: id.clone(), kind: "drawing", target: format!("../drawings/drawing{index}.xml"), external: false });
        x.push_str(&format!("<drawing r:id=\"{id}\"/>"));
        for (p, d) in dr.parts.items {
            out.parts.set(&p, d);
        }
        out.overrides.extend(dr.overrides);
        out.defaults.extend(dr.defaults);
    }

    x.push_str("</worksheet>");
    // the part is kept for the life of the package: give back what the estimate over-reserved
    x.shrink_to_fit();
    out.xml = x;
    out
}

/// `commentsXml`: the notes of the sheet → `xl/commentsN.xml`.
pub fn comments_xml(notes: &[NoteInfo]) -> String {
    let mut authors: Vec<&str> = Vec::new();
    let mut a_idx: HashMap<&str, usize> = HashMap::new();
    let mut list = String::new();
    for n in notes {
        let ai = *a_idx.entry(n.author.as_str()).or_insert_with(|| {
            authors.push(n.author.as_str());
            authors.len() - 1
        });
        list.push_str("<comment ref=\"");
        cell_ref(&mut list, n.row, n.col);
        list.push_str(&format!("\" authorId=\"{ai}\"><text><t xml:space=\"preserve\">"));
        xml_text(&mut list, n.text.as_deref().unwrap_or(""));
        list.push_str("</t></text></comment>");
    }
    let mut x = String::new();
    x.push_str(XML_HEAD);
    x.push_str(&format!("<comments xmlns=\"{NS_MAIN}\"><authors>"));
    for a in authors {
        x.push_str("<author>");
        xml_text(&mut x, a);
        x.push_str("</author>");
    }
    x.push_str("</authors><commentList>");
    x.push_str(&list);
    x.push_str("</commentList></comments>");
    x
}
