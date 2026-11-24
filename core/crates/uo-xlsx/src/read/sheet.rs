//! `read.ts` `readWorksheet` minus the rows: everything of a worksheet part that is not `<sheetData>` (merges,
//! columns, pane, flags, conditional formats, comments, drawing, hyperlinks, tables, pivots, validations,
//! autofilter) read from the frame (the part with its `<sheetData>` taken out).

use super::cf::read_conditional_formats;
use super::drawing::{read_controls, read_drawing, read_ole_objects};
use super::model::{
    AutoFilterInfo, ChartsheetView, ColDef, ColorContext, Comment, Hyperlink, PaneInfo, PivotInfo, PivotItem, SheetFormatPr,
    TableColumnInfo, TableInfo, Worksheet,
};
use super::theme::to_xel;
use super::workbook::{doc, read_rels, rel_get, rels_path_for, resolve_rel_target};
use super::{El, off, on, owned, pf, pi, trim};
use crate::book::ReadError;
use uo_opc::Package;

/// The relationship targets of a rels part whose Type ends with `suffix` (`findRelByType`/`findRelsByType`):
/// every one, in order, with a non-empty Target (the first-only caller takes the first).
fn rels_by_type(pkg: &Package, rels_part: &str, suffix: &str) -> Result<Vec<Option<String>>, ReadError> {
    let Some(d) = doc(pkg, rels_part)? else { return Ok(Vec::new()) };
    Ok(d.root()
        .children_named("Relationship")
        .filter(|r| r.attr("Type").unwrap_or("").ends_with(suffix))
        .map(|r| owned(r.attr("Target")))
        .collect())
}

/// `readAutoFilter`.
pub fn read_auto_filter(af: El<'_>, fallback_ref: &str) -> AutoFilterInfo {
    let mut info = AutoFilterInfo { r#ref: af.attr("ref").unwrap_or(fallback_ref).to_owned(), ..AutoFilterInfo::default() };
    for fc in af.children_named("filterColumn") {
        let id = pi(fc.attr("colId").unwrap_or(""));
        if !id.is_finite() {
            continue;
        }
        if on(fc.attr("hiddenButton")) || off(fc.attr("showButton")) {
            info.hidden.push(id);
        }
        if fc.child_elements().next().is_some() {
            info.active.push(id);
        }
    }
    info
}

fn read_table_part(pkg: &Package, path: &str) -> Option<TableInfo> {
    let d = doc(pkg, path).ok().flatten()?;
    let root = d.root();
    let r = root.attr("ref").filter(|s| !s.is_empty())?;
    let style_info = root.first_child("tableStyleInfo");
    let flag = |k: &str| matches!(style_info.and_then(|s| s.attr(k)), Some("1" | "true"));
    let af = root.first_child("autoFilter");
    let mut out = TableInfo {
        r#ref: r.to_owned(),
        header_row: root.attr("headerRowCount") != Some("0"),
        totals_row: root.attr("totalsRowCount") == Some("1"),
        stripes: flag("showRowStripes"),
        auto_filter: af.is_some(),
        name: owned(root.attr("name")),
        display_name: owned(root.attr("displayName")),
        header_row_count: root.attr("headerRowCount").map(pi),
        totals_row_count: root.attr("totalsRowCount").map(pi),
        ..TableInfo::default()
    };
    if let Some(tcs) = root.first_child("tableColumns") {
        out.columns = Some(
            tcs.children_named("tableColumn")
                .map(|tc| TableColumnInfo {
                    name: super::decode_xstring_str(tc.attr("name").unwrap_or("")),
                    calculated_column_formula: tc.first_child("calculatedColumnFormula").map(|f| f.text_content()),
                    totals_row_function: owned(tc.attr("totalsRowFunction")),
                    totals_row_label: owned(tc.attr("totalsRowLabel")),
                    totals_row_formula: tc.first_child("totalsRowFormula").map(|f| f.text_content()),
                })
                .collect(),
        );
    }
    out.style_name = style_info.and_then(|s| s.attr("name")).filter(|s| !s.is_empty()).map(str::to_owned);
    out.col_stripes = flag("showColumnStripes");
    out.first_column = flag("showFirstColumn");
    out.last_column = flag("showLastColumn");
    out.filter = af.map(|a| read_auto_filter(a, r));
    out.border_dxf_id = root.attr("tableBorderDxfId").map(pi);
    Some(out)
}

fn read_pivot_table(pkg: &Package, path: &str) -> Option<PivotInfo> {
    let d = doc(pkg, path).ok().flatten()?;
    let root = d.root();
    let loc = root.first_child("location")?;
    let r = loc.attr("ref").filter(|s| !s.is_empty())?;
    let int = |el: Option<El<'_>>, k: &str, dflt: f64| {
        let v = el.and_then(|e| e.attr(k)).map_or(f64::NAN, pi);
        if v.is_finite() { v } else { dflt }
    };
    let flag = |k: &str, dflt: bool| root.attr(k).map_or(dflt, |v| v == "1" || v == "true");
    let items = |tag: &str| -> Vec<PivotItem> {
        root.first_child(tag)
            .map(|el| {
                el.children_named("i")
                    .map(|i| PivotItem {
                        t: i.attr("t").filter(|s| !s.is_empty()).map(str::to_owned),
                        r: int(Some(i), "r", 0.0),
                        x: f64::from(u32::try_from(i.children_named("x").count()).unwrap_or(u32::MAX)),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let fields = |tag: &str| -> Vec<f64> {
        root.first_child(tag).map(|el| el.children_named("field").map(|f| int(Some(f), "x", 0.0)).collect()).unwrap_or_default()
    };
    let si = root.first_child("pivotTableStyleInfo");
    let son = |k: &str| matches!(si.and_then(|s| s.attr(k)), Some("1" | "true"));
    let page_fields = root.first_child("pageFields");
    let mut out = PivotInfo {
        r#ref: r.to_owned(),
        first_header_row: int(Some(loc), "firstHeaderRow", 1.0),
        first_data_row: int(Some(loc), "firstDataRow", 1.0),
        first_data_col: int(Some(loc), "firstDataCol", 1.0),
        row_page_count: int(Some(loc), "rowPageCount", 0.0),
        col_page_count: int(Some(loc), "colPageCount", 0.0),
        page_field_count: page_fields.map_or(0.0, |p| f64::from(u32::try_from(p.children_named("pageField").count()).unwrap_or(u32::MAX))),
        row_fields: fields("rowFields"),
        col_fields: fields("colFields"),
        row_items: items("rowItems"),
        col_items: items("colItems"),
        compact: flag("compact", true),
        outline: flag("outline", false),
        show_headers: flag("showHeaders", true),
        row_grand_totals: flag("rowGrandTotals", true),
        col_grand_totals: flag("colGrandTotals", true),
        show_row_headers: son("showRowHeaders"),
        show_col_headers: son("showColHeaders"),
        show_row_stripes: son("showRowStripes"),
        show_col_stripes: son("showColStripes"),
        show_last_column: son("showLastColumn"),
        ..PivotInfo::default()
    };
    if let Some(si) = si {
        out.style_name = Some(si.attr("name").unwrap_or("").to_owned());
    }
    if let Some(pf_el) = root.first_child("pivotFields") {
        let mut filtered: Vec<f64> = Vec::new();
        for (k, f) in pf_el.children_named("pivotField").enumerate() {
            if let Some(its) = f.first_child("items")
                && its.children_named("item").any(|it| matches!(it.attr("h"), Some("1" | "true")))
            {
                filtered.push(f64::from(u32::try_from(k).unwrap_or(u32::MAX)));
            }
        }
        if let Some(filters) = root.first_child("filters") {
            for f in filters.children_named("filter") {
                filtered.push(int(Some(f), "fld", -1.0));
            }
        }
        out.filtered_fields = filtered;
    }
    if let Some(pfs) = page_fields {
        out.page_field_active = Some(
            pfs.children_named("pageField")
                .map(|p| p.attr("item").is_some() || out.filtered_fields.contains(&int(Some(p), "fld", -1.0)))
                .collect(),
        );
    }
    Some(out)
}

fn read_comments(pkg: &Package, path: &str) -> Result<Vec<Comment>, ReadError> {
    let Some(d) = doc(pkg, path)? else { return Ok(Vec::new()) };
    let root = d.root();
    let authors: Vec<String> =
        root.first_child("authors").map(|a| a.children_named("author").map(|x| x.text_content()).collect()).unwrap_or_default();
    let mut out = Vec::new();
    let Some(list) = root.first_child("commentList") else { return Ok(out) };
    for cmt in list.children_named("comment") {
        let Some(r) = cmt.attr("ref").filter(|s| !s.is_empty()) else { continue };
        let text = cmt.first_child("text").map(|t| t.text_content()).unwrap_or_default();
        let ai = cmt.attr("authorId").map_or(-1.0, pi);
        let author = if ai >= 0.0 { uo_jsnum::conv::js_index(ai, authors.len()).and_then(|i| authors.get(i)).cloned() } else { None };
        out.push(Comment { r#ref: r.to_owned(), text, author });
    }
    Ok(out)
}

/// `readWorksheet` without `rows`, over the parsed frame (`root`: the worksheet element with an empty
/// `<sheetData/>`). `sheet_path` is the part name.
pub fn read_worksheet_frame(pkg: &Package, sheet_path: &str, root: El<'_>, cx: &ColorContext) -> Result<Worksheet, ReadError> {
    let rels_path = rels_path_for(sheet_path);
    let sheet_rels = read_rels(pkg, &rels_path)?;
    let mut ws = Worksheet::default();

    if let Some(mc) = root.first_child("mergeCells") {
        for m in mc.children_named("mergeCell") {
            if let Some(r) = m.attr("ref") {
                ws.merges.push(r.to_owned());
            }
        }
    }
    for m in &ws.merges {
        let tl = m.split(':').next().unwrap_or(m).to_owned();
        match ws.merged_areas.iter_mut().find(|(k, _)| *k == tl) {
            Some(slot) => slot.1.clone_from(m),
            None => ws.merged_areas.push((tl, m.clone())),
        }
    }
    if let Some(cols) = root.first_child("cols") {
        for c in cols.children_named("col") {
            ws.cols.push(ColDef {
                min: pi(c.attr("min").unwrap_or("1")),
                max: pi(c.attr("max").unwrap_or("1")),
                width: c.attr("width").map(pf),
                custom_width: c.attr("customWidth") == Some("1"),
                hidden: c.attr("hidden") == Some("1"),
                style: c.attr("style").map(pi),
            });
        }
    }
    if let Some(sfp) = root.first_child("sheetFormatPr") {
        ws.sheet_format_pr = Some(SheetFormatPr {
            default_col_width: sfp.attr("defaultColWidth").map(pf),
            base_col_width: sfp.attr("baseColWidth").map(pf),
            default_row_height: sfp.attr("defaultRowHeight").map(pf),
            custom_height: on(sfp.attr("customHeight")),
            zero_height: on(sfp.attr("zeroHeight")),
        });
    }
    if let Some(hls) = root.first_child("hyperlinks") {
        for hl in hls.children_named("hyperlink") {
            let Some(r) = hl.attr("ref") else { continue };
            let rid = hl.attr("r:id").or_else(|| hl.attr("id"));
            ws.hyperlinks.push(Hyperlink {
                r#ref: r.to_owned(),
                target: rid.and_then(|id| rel_get(&sheet_rels, id)).map(str::to_owned),
                location: owned(hl.attr("location")),
                display: owned(hl.attr("display")),
                tooltip: owned(hl.attr("tooltip")),
            });
        }
    }
    let sheet_view = root.first_child("sheetViews").unwrap_or(root).first_child("sheetView");
    ws.show_grid_lines_off = sheet_view.and_then(|s| s.attr("showGridLines")) == Some("0");
    ws.show_zeros_off = off(sheet_view.and_then(|s| s.attr("showZeros")));
    ws.right_to_left = on(sheet_view.and_then(|s| s.attr("rightToLeft")));
    if let Some(p) = sheet_view.and_then(|s| s.first_child("pane")) {
        ws.pane = Some(PaneInfo {
            x_split: pi(p.attr("xSplit").unwrap_or("0")),
            y_split: pi(p.attr("ySplit").unwrap_or("0")),
            top_left_cell: owned(p.attr("topLeftCell")),
            state: p.attr("state").unwrap_or("split").to_owned(),
        });
    }
    ws.cond_formats = read_conditional_formats(root, cx);

    let comments = rels_by_type(pkg, &rels_path, "/comments")?.into_iter().next().flatten().filter(|s| !s.is_empty());
    if let Some(c) = comments {
        ws.comments = read_comments(pkg, &resolve_rel_target(sheet_path, &c))?;
    }
    let drawing_target = rels_by_type(pkg, &rels_path, "/drawing")?.into_iter().next().flatten().filter(|s| !s.is_empty());
    let mut drawing = match drawing_target {
        Some(t) => read_drawing(pkg, &resolve_rel_target(sheet_path, &t)),
        None => super::drawing::DrawingContent::default(),
    };
    ws.charts = std::mem::take(&mut drawing.charts);
    let needs_root = root.child_elements().any(|c| matches!(c.name(), "oleObjects" | "controls" | "mc:AlternateContent"));
    if needs_root {
        let sheet_root = to_xel(root);
        let ole = read_ole_objects(&sheet_root, sheet_path, &drawing.objects);
        let ctl = read_controls(pkg, &sheet_root, sheet_path, &drawing.objects);
        drawing.objects.extend(ole);
        drawing.objects.extend(ctl);
    }
    ws.drawings = drawing.objects;

    if let Some(dvs) = root.first_child("dataValidations") {
        for dv in dvs.children_named("dataValidation") {
            if dv.attr("type") != Some("list") {
                continue;
            }
            if let Some(sq) = dv.attr("sqref").filter(|s| !s.is_empty()) {
                ws.list_validations.extend(sq.split(' ').map(str::to_owned));
            }
        }
    }
    for t in rels_by_type(pkg, &rels_path, "/table")?.into_iter().flatten().filter(|s| !s.is_empty()) {
        if let Some(t) = read_table_part(pkg, &resolve_rel_target(sheet_path, &t)) {
            ws.tables.push(t);
        }
    }
    for t in rels_by_type(pkg, &rels_path, "/pivotTable")?.into_iter().flatten().filter(|s| !s.is_empty()) {
        if let Some(p) = read_pivot_table(pkg, &resolve_rel_target(sheet_path, &t)) {
            ws.pivots.push(p);
        }
    }
    for ie in root.first_child("ignoredErrors").unwrap_or(root).children_named("ignoredError") {
        if on(ie.attr("numberStoredAsText")) {
            ws.ignore_number_as_text
                .extend(uo_jsnum::text::split_js_space(ie.attr("sqref").unwrap_or("")).filter(|s| !s.is_empty()).map(str::to_owned));
        }
    }
    if root.name() == "chartsheet" {
        let sv = root.first_child("sheetViews").unwrap_or(root).first_child("sheetView");
        let z = pi(sv.and_then(|s| s.attr("zoomScale")).unwrap_or("100"));
        ws.chartsheet = Some(ChartsheetView {
            zoom: if z.is_finite() && (10.0..=400.0).contains(&z) { z } else { 100.0 },
            zoom_to_fit: on(sv.and_then(|s| s.attr("zoomToFit"))),
        });
    }
    ws.filter_mode = on(root.first_child("sheetPr").and_then(|s| s.attr("filterMode")));
    if let Some(af) = root.first_child("autoFilter")
        && let Some(r) = af.attr("ref").filter(|s| !s.is_empty())
    {
        ws.auto_filter = Some(read_auto_filter(af, r));
    }
    let _ = trim;
    Ok(ws)
}
