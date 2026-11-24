//! `drawing.ts`: the worksheet drawing (`xl/drawings/drawingN.xml`): anchors, charts (anchor + raw chart XML),
//! other objects as owned DrawingML subtrees, OLE previews and form controls.

use super::model::{AnchorKind, AnchorPos, DrawingAnchor, DrawingChart, DrawingControl, DrawingObject, XChild, XEl};
use super::theme::to_xel_with;
use super::workbook::{doc, rels_path_for, resolve_rel_target};
use super::{pi, trim};
use uo_opc::Package;

/// Charts and the other objects of a drawing.
#[derive(Debug, Default)]
pub struct DrawingContent {
    /// Charts.
    pub charts: Vec<DrawingChart>,
    /// Other objects.
    pub objects: Vec<DrawingObject>,
}

/// `\w+` prefix of `^(?:(\w+):)?wsDr$`: `Some(prefix)` (empty = none), `None` = not a drawing root.
fn wsdr_prefix(name: &str) -> Option<&str> {
    if name == "wsDr" {
        return Some("");
    }
    let p = name.strip_suffix(":wsDr")?;
    (!p.is_empty() && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')).then_some(p)
}

fn requires_of(ch: &XEl) -> Vec<String> {
    uo_jsnum::text::split_js_space(ch.attr("Requires").unwrap_or("")).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// `^(sle15|tsle)$`.
fn unrendered(r: &str) -> bool {
    r == "sle15" || r == "tsle"
}

/// `^(a14|a15|a16|x14|x15|cx\d*)$`.
fn known(r: &str) -> bool {
    matches!(r, "a14" | "a15" | "a16" | "x14" | "x15") || r.strip_prefix("cx").is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit()))
}

/// The effective branch of an `mc:AlternateContent`.
fn alternate(alt: &XEl) -> Option<&XEl> {
    for ch in alt.elements(Some("mc:Choice")) {
        let req = requires_of(ch);
        if req.iter().any(|r| unrendered(r)) {
            return None;
        }
        if req.iter().all(|r| known(r)) {
            return Some(ch);
        }
    }
    alt.first("mc:Fallback")
}

/// Children with `mc:AlternateContent` flattened in place.
pub fn resolved_children(el: &XEl) -> Vec<&XEl> {
    let mut out = Vec::new();
    for c in el.elements(None) {
        if c.name != "mc:AlternateContent" {
            out.push(c);
            continue;
        }
        if let Some(b) = alternate(c) {
            out.extend(resolved_children(b));
        }
    }
    out
}

fn finite_or_zero(x: f64) -> f64 {
    if x.is_finite() { x } else { 0.0 }
}

fn num(el: Option<&XEl>, name: &str) -> f64 {
    let c = el.and_then(|e| e.first(name));
    finite_or_zero(c.map_or(0.0, |c| pi(trim(&text_of(c)))))
}

fn text_of(e: &XEl) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c {
            XChild::Text(t) => s.push_str(t),
            XChild::El(x) => s.push_str(&text_of(x)),
        }
    }
    s
}

fn cell_pos(el: Option<&XEl>) -> AnchorPos {
    AnchorPos { col: num(el, "xdr:col"), row: num(el, "xdr:row"), col_off: num(el, "xdr:colOff"), row_off: num(el, "xdr:rowOff") }
}

fn attr_num(el: Option<&XEl>, name: &str) -> f64 {
    finite_or_zero(el.map_or(0.0, |e| pi(e.attr(name).unwrap_or("0"))))
}

/// `readAnchor`: `None` for an element that is not an anchor.
pub fn read_anchor(el: &XEl) -> Option<DrawingAnchor> {
    match el.name.as_str() {
        "xdr:twoCellAnchor" => Some(DrawingAnchor {
            from: cell_pos(el.first("xdr:from")),
            to: cell_pos(el.first("xdr:to")),
            edit_as: el.attr("editAs").filter(|s| !s.is_empty()).map(str::to_owned),
            ..DrawingAnchor::default()
        }),
        "xdr:oneCellAnchor" => {
            let from = cell_pos(el.first("xdr:from"));
            let ext = el.first("xdr:ext");
            Some(DrawingAnchor {
                kind: Some(AnchorKind::OneCell),
                from,
                to: from,
                ext: Some((attr_num(ext, "cx"), attr_num(ext, "cy"))),
                ..DrawingAnchor::default()
            })
        }
        "xdr:absoluteAnchor" => {
            let pos = el.first("xdr:pos");
            let ext = el.first("xdr:ext");
            Some(DrawingAnchor {
                kind: Some(AnchorKind::Absolute),
                pos: Some((attr_num(pos, "x"), attr_num(pos, "y"))),
                ext: Some((attr_num(ext, "cx"), attr_num(ext, "cy"))),
                ..DrawingAnchor::default()
            })
        }
        _ => None,
    }
}

fn has_spid(e: &XEl, spid: &str) -> bool {
    if e.name.ends_with(":compatExt") && e.attr("spid") == Some(spid) {
        return true;
    }
    e.elements(None).any(|c| has_spid(c, spid))
}

fn el(name: &str, attrs: &[(&str, &str)], children: Vec<XEl>) -> XEl {
    XEl::new(name, attrs, children)
}

fn ole_frame(host: Option<&DrawingObject>) -> Vec<XEl> {
    let own: Vec<XEl> = host
        .and_then(|h| h.el.first("xdr:spPr"))
        .map(|sp| sp.elements(None).filter(|c| c.name == "a:solidFill" || c.name == "a:noFill" || c.name == "a:ln").cloned().collect())
        .unwrap_or_default();
    if !own.is_empty() {
        return own;
    }
    let rgb = |v: &str| el("a:solidFill", &[], vec![el("a:srgbClr", &[("val", v)], vec![])]);
    vec![rgb("FFFFFF"), el("a:ln", &[("w", "9525")], vec![rgb("000000")])]
}

/// `readOleObjects`.
pub fn read_ole_objects(sheet_root: &XEl, sheet_path: &str, drawing_objects: &[DrawingObject]) -> Vec<DrawingObject> {
    let Some(list) = resolved_children(sheet_root).into_iter().find(|c| c.name == "oleObjects") else { return Vec::new() };
    let mut out: Vec<DrawingObject> = Vec::new();
    for ole in resolved_children(list) {
        if ole.name != "oleObject" {
            continue;
        }
        let pr = ole.first("objectPr");
        let rid = pr.and_then(|p| p.attr("r:id")).filter(|s| !s.is_empty());
        let anchor_el = pr.and_then(|p| p.first("anchor"));
        let (Some(rid), Some(anchor_el)) = (rid, anchor_el) else { continue };
        let anchor =
            DrawingAnchor { from: cell_pos(anchor_el.first("from")), to: cell_pos(anchor_el.first("to")), ..DrawingAnchor::default() };
        let shape_id = ole.attr("shapeId").unwrap_or("");
        let host = if shape_id.is_empty() {
            None
        } else {
            let spid = format!("_x0000_s{shape_id}");
            drawing_objects.iter().find(|o| has_spid(&o.el, &spid))
        };
        let pic = el(
            "xdr:pic",
            &[],
            vec![
                el(
                    "xdr:nvPicPr",
                    &[],
                    vec![
                        el("xdr:cNvPr", &[("id", shape_id), ("name", ole.attr("progId").unwrap_or("OLE"))], vec![]),
                        el("xdr:cNvPicPr", &[], vec![]),
                    ],
                ),
                el(
                    "xdr:blipFill",
                    &[],
                    vec![el("a:blip", &[("r:embed", rid)], vec![]), el("a:stretch", &[], vec![el("a:fillRect", &[], vec![])])],
                ),
                el("xdr:spPr", &[], {
                    let mut v = vec![el("a:prstGeom", &[("prst", "rect")], vec![])];
                    v.extend(ole_frame(host));
                    v
                }),
            ],
        );
        let order = host.map_or(1e6 + out.len() as f64, |h| h.order + 0.5);
        out.push(DrawingObject { anchor, order, el: pic, part: sheet_path.to_owned(), control: None });
    }
    out
}

fn unhide(e: &XEl) -> XEl {
    XEl {
        name: e.name.clone(),
        attrs: if e.name.ends_with(":cNvPr") { e.attrs.iter().filter(|(k, _)| k != "hidden").cloned().collect() } else { e.attrs.clone() },
        children: e
            .children
            .iter()
            .map(|c| match c {
                XChild::El(x) => XChild::El(unhide(x)),
                XChild::Text(t) => XChild::Text(t.clone()),
            })
            .collect(),
    }
}

/// `readControls`.
pub fn read_controls(pkg: &Package, sheet_root: &XEl, sheet_path: &str, drawing_objects: &[DrawingObject]) -> Vec<DrawingObject> {
    let Some(list) = resolved_children(sheet_root).into_iter().find(|c| c.name == "controls") else { return Vec::new() };
    let mut rels: Vec<(String, String)> = Vec::new();
    if let Ok(Some(d)) = doc(pkg, &rels_path_for(sheet_path)) {
        for r in d.root().children_named("Relationship") {
            if let (Some(id), Some(t)) = (r.attr("Id").filter(|s| !s.is_empty()), r.attr("Target").filter(|s| !s.is_empty())) {
                let t = resolve_rel_target(sheet_path, t);
                match rels.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = t,
                    None => rels.push((id.to_owned(), t)),
                }
            }
        }
    }
    let mut out: Vec<DrawingObject> = Vec::new();
    for c in resolved_children(list) {
        if c.name != "control" {
            continue;
        }
        let Some(anchor_el) = c.first("controlPr").unwrap_or(c).first("anchor") else { continue };
        let anchor =
            DrawingAnchor { from: cell_pos(anchor_el.first("from")), to: cell_pos(anchor_el.first("to")), ..DrawingAnchor::default() };
        let props = c.attr("r:id").and_then(|r| rels.iter().find(|(k, _)| k == r)).map(|(_, p)| p.clone());
        let (mut ty, mut checked) = (String::new(), None);
        if let Some(p) = props
            && let Ok(Some(d)) = doc(pkg, &p)
        {
            ty = d.root().attr("objectType").unwrap_or("").to_owned();
            checked = d.root().attr("checked").filter(|s| !s.is_empty()).map(str::to_owned);
        }
        let shape_id = c.attr("shapeId").unwrap_or("");
        let host = if shape_id.is_empty() {
            None
        } else {
            let spid = format!("_x0000_s{shape_id}");
            drawing_objects.iter().find(|o| has_spid(&o.el, &spid))
        };
        let sp = match host {
            Some(h) => unhide(&h.el),
            None => el(
                "xdr:sp",
                &[],
                vec![
                    el(
                        "xdr:nvSpPr",
                        &[],
                        vec![
                            el("xdr:cNvPr", &[("id", shape_id), ("name", c.attr("name").unwrap_or(""))], vec![]),
                            el("xdr:cNvSpPr", &[], vec![]),
                        ],
                    ),
                    el("xdr:spPr", &[], vec![]),
                ],
            ),
        };
        let order = host.map_or(1e6 + out.len() as f64, |h| h.order + 0.5);
        out.push(DrawingObject {
            anchor,
            order,
            el: sp,
            part: host.map_or_else(|| sheet_path.to_owned(), |h| h.part.clone()),
            control: Some(DrawingControl { r#type: ty, checked }),
        });
    }
    out
}

/// `readDrawing`: a missing or unreadable part is an empty drawing.
pub fn read_drawing(pkg: &Package, drawing_path: &str) -> DrawingContent {
    let mut out = DrawingContent::default();
    let Ok(Some(d)) = doc(pkg, drawing_path) else { return out };
    let root_el = d.root();
    let Some(prefix) = wsdr_prefix(root_el.name()) else { return out };
    let root = if prefix == "xdr" {
        to_xel_with(root_el, &|n| n.to_owned())
    } else {
        let from = prefix.to_owned();
        to_xel_with(root_el, &move |n| {
            if from.is_empty() {
                if n.contains(':') { n.to_owned() } else { format!("xdr:{n}") }
            } else {
                match n.strip_prefix(&format!("{from}:")) {
                    Some(rest) => format!("xdr:{rest}"),
                    None => n.to_owned(),
                }
            }
        })
    };
    // rels of the drawing: rId -> part (charts here; pictures are resolved by the DrawingML reader)
    let mut rels: Vec<(String, String)> = Vec::new();
    if let Ok(Some(rd)) = doc(pkg, &rels_path_for(drawing_path)) {
        for rel in rd.root().children_named("Relationship") {
            let (id, target) = (rel.attr("Id").unwrap_or(""), rel.attr("Target").unwrap_or(""));
            if !id.is_empty() && !target.is_empty() && rel.attr("TargetMode") != Some("External") {
                let t = resolve_rel_target(drawing_path, target);
                match rels.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = t,
                    None => rels.push((id.to_owned(), t)),
                }
            }
        }
    }
    let mut order = 0u32;
    for anchor_el in resolved_children(&root) {
        let Some(anchor) = read_anchor(anchor_el) else { continue };
        let Some(obj) = resolved_children(anchor_el)
            .into_iter()
            .find(|c| matches!(c.name.as_str(), "xdr:sp" | "xdr:pic" | "xdr:cxnSp" | "xdr:grpSp" | "xdr:graphicFrame"))
        else {
            continue;
        };
        let idx = f64::from(order);
        order += 1;
        if obj.name == "xdr:graphicFrame" {
            let data = obj.first("a:graphic").unwrap_or(obj).first("a:graphicData");
            let chart_el = data.and_then(|d| d.first("c:chart").or_else(|| d.first("cx:chart")));
            if let Some(chart_el) = chart_el {
                let key = chart_el.attr("r:id").or_else(|| chart_el.attr("id")).unwrap_or("");
                let path = rels.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone());
                let chart_xml = path.as_ref().and_then(|p| pkg.find_text(p).ok().flatten()).map(|t| t.into_owned());
                if let (Some(path), Some(chart_xml)) = (path, chart_xml)
                    && !chart_xml.is_empty()
                {
                    let name = obj
                        .first("xdr:nvGraphicFramePr")
                        .unwrap_or(obj)
                        .first("xdr:cNvPr")
                        .and_then(|n| n.attr("name"))
                        .filter(|s| !s.is_empty());
                    out.charts.push(DrawingChart {
                        anchor,
                        chart_xml,
                        order: Some(idx),
                        name: name.map(str::to_owned),
                        part: Some(path),
                        chartex: chart_el.name == "cx:chart",
                    });
                }
                continue;
            }
        }
        out.objects.push(DrawingObject { anchor, order: idx, el: obj.clone(), part: drawing_path.to_owned(), control: None });
    }
    out
}
