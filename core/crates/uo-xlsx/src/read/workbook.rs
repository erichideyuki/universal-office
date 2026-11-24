//! `read.ts` `readWorkbook` and its helpers: `xl/workbook.xml`, the relationships, external links and the
//! dynamic-array metadata, plus the package path helpers of `package.ts`.

use super::model::{CalcPr, DefinedName, ExternalCell, ExternalLink, ExternalName, ExternalSheetData, SheetRef, Workbook};
use super::{El, SHEET_XML, on, owned, pf, pi};
use crate::book::ReadError;
use uo_opc::Package;
use uo_opc::xml::Document;

/// `resolveRelTarget` (`package.ts:148-158`).
pub fn resolve_rel_target(from_part: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let up = |d: &str| d.rfind('/').map_or(String::new(), |i| d[..i].to_owned());
    let mut dir = if from_part.contains('/') { up(from_part) } else { String::new() };
    let mut t = target;
    while let Some(rest) = t.strip_prefix("../") {
        t = rest;
        dir = if dir.contains('/') { up(&dir) } else { String::new() };
    }
    if dir.is_empty() { t.to_owned() } else { format!("{dir}/{t}") }
}

/// `relsPathFor` / the rels of a part: `dir/_rels/file.rels`.
pub fn rels_path_for(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
        None => format!("/_rels/{part}.rels"),
    }
}

/// A part parsed with the sheet-core grammar: `Ok(None)` when it does not exist, `Err` for a damaged part.
pub fn doc(pkg: &Package, name: &str) -> Result<Option<Document>, ReadError> {
    match pkg.xml_part(name, &SHEET_XML) {
        Ok(d) => Ok(Some(d)),
        Err(uo_opc::Error::MissingPart(_)) => Ok(None),
        Err(e) => Err(ReadError::Opc(e)),
    }
}

/// `readRels`: Id -> Target, a repeated Id keeping the last Target (a JS `Map`).
pub fn read_rels(pkg: &Package, rels_part: &str) -> Result<Vec<(String, String)>, ReadError> {
    let mut out: Vec<(String, String)> = Vec::new();
    let Some(d) = doc(pkg, rels_part)? else { return Ok(out) };
    for rel in d.root().children_named("Relationship") {
        if let (Some(id), Some(t)) = (rel.attr("Id"), rel.attr("Target")) {
            match out.iter_mut().find(|(k, _)| k == id) {
                Some(slot) => slot.1 = t.to_owned(),
                None => out.push((id.to_owned(), t.to_owned())),
            }
        }
    }
    Ok(out)
}

/// A lookup in a rels list.
pub fn rel_get<'a>(rels: &'a [(String, String)], id: &str) -> Option<&'a str> {
    rels.iter().find(|(k, _)| k == id).map(|(_, v)| v.as_str())
}

fn decode_str(s: &str) -> String {
    super::decode_xstring_str(s)
}

/// `readWorkbook`.
pub fn read_workbook(pkg: &Package) -> Result<Workbook, ReadError> {
    let d = doc(pkg, "xl/workbook.xml")?.ok_or(ReadError::NotWorkbook)?;
    let root = d.root();
    let rels = read_rels(pkg, "xl/_rels/workbook.xml.rels")?;
    let mut wb = Workbook::default();
    if let Some(sheets) = root.first_child("sheets") {
        for s in sheets.children_named("sheet") {
            let r_id = s.attr("r:id");
            let target = r_id.and_then(|r| rel_get(&rels, r)).map(|t| resolve_rel_target("xl/workbook.xml", t));
            wb.sheets.push(SheetRef {
                name: s.attr("name").unwrap_or("").to_owned(),
                sheet_id: s.attr("sheetId").unwrap_or("").to_owned(),
                r_id: r_id.unwrap_or("").to_owned(),
                state: owned(s.attr("state")),
                target,
            });
        }
    }
    let wb_view = root.first_child("bookViews").and_then(|b| b.first_child("workbookView"));
    wb.active_tab = wb_view.and_then(|v| v.attr("activeTab")).or_else(|| root.attr("activeTab")).map(pi);
    wb.date1904 = matches!(root.first_child("workbookPr").and_then(|p| p.attr("date1904")), Some("1" | "true"));
    if let Some(dns) = root.first_child("definedNames") {
        for dn in dns.children_named("definedName") {
            let name = dn.attr("name").unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let ls = dn.attr("localSheetId");
            wb.defined_names.push(DefinedName {
                name: name.to_owned(),
                formula: dn.text_content(),
                local_sheet_id: ls.filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit())).map(pi),
                hidden: on(dn.attr("hidden")),
            });
        }
    }
    wb.app_name = owned(root.first_child("fileVersion").and_then(|f| f.attr("appName")));
    if let Some(cp) = root.first_child("calcPr") {
        let num = |k: &str| cp.attr(k).map(pf).filter(|n| n.is_finite());
        wb.calc_pr = Some(CalcPr {
            calc_id: num("calcId"),
            calc_mode: owned(cp.attr("calcMode")).filter(|s| !s.is_empty()),
            full_calc_on_load: on(cp.attr("fullCalcOnLoad")),
            force_full_calc: on(cp.attr("forceFullCalc")),
            iterate: on(cp.attr("iterate")),
            iterate_count: num("iterateCount"),
            iterate_delta: num("iterateDelta"),
            ref_mode: owned(cp.attr("refMode")).filter(|s| !s.is_empty()),
        });
    }
    if let Some(ers) = root.first_child("externalReferences") {
        for er in ers.children_named("externalReference") {
            let target = er.attr("r:id").and_then(|r| rel_get(&rels, r));
            let part = target.map(|t| resolve_rel_target("xl/workbook.xml", t)).filter(|p| !p.is_empty());
            let link = match part {
                Some(p) => read_external_link(pkg, &p)?,
                None => None,
            };
            wb.external_links.push(link.unwrap_or_default());
        }
    }
    // dynamic arrays: xl/metadata.xml unless the rels name another sheetMetadata part
    let mut meta_part = "xl/metadata.xml".to_owned();
    if let Some(rd) = doc(pkg, "xl/_rels/workbook.xml.rels")? {
        for rel in rd.root().children_named("Relationship") {
            if rel.attr("Type").unwrap_or("").ends_with("/sheetMetadata")
                && let Some(t) = rel.attr("Target").filter(|t| !t.is_empty())
            {
                meta_part = resolve_rel_target("xl/workbook.xml", t);
            }
        }
    }
    wb.dynamic_array_cm = read_dynamic_array_cm(pkg, &meta_part);
    Ok(wb)
}

fn read_external_link(pkg: &Package, part: &str) -> Result<Option<ExternalLink>, ReadError> {
    let Ok(Some(d)) = doc(pkg, part) else { return Ok(None) };
    let root = d.root();
    let mut link = ExternalLink::default();
    let Some(book) = root.first_child("externalBook") else { return Ok(Some(link)) };
    if let Some(rid) = book.attr("r:id")
        && let Some(t) = rel_get(&read_rels(pkg, &rels_path_for(part))?, rid)
    {
        link.target = Some(t.to_owned());
    }
    let under = |name: &str| book.first_child(name).unwrap_or(book);
    for sn in under("sheetNames").children_named("sheetName") {
        link.sheet_names.push(sn.attr("val").unwrap_or("").to_owned());
    }
    for dn in under("definedNames").children_named("definedName") {
        link.defined_names.push(ExternalName {
            name: dn.attr("name").unwrap_or("").to_owned(),
            refers_to: owned(dn.attr("refersTo")),
            sheet_id: dn.attr("sheetId").map(pi),
        });
    }
    for sd in under("sheetDataSet").children_named("sheetData") {
        let mut cells = Vec::new();
        for row in sd.children_named("row") {
            for c in row.children_named("cell") {
                let r = c.attr("r").unwrap_or("");
                if r.is_empty() {
                    continue;
                }
                cells.push(ExternalCell {
                    r#ref: r.to_owned(),
                    t: owned(c.attr("t")),
                    v: c.first_child("v").map(|v| decode_str(&v.text_content())),
                });
            }
        }
        link.sheet_data.push(ExternalSheetData { sheet_id: pi(sd.attr("sheetId").unwrap_or("0")), cells });
    }
    Ok(Some(link))
}

/// `serializeLoose`: the attributes of an element and of every descendant as text (to find `fDynamic`).
fn serialize_loose(el: El<'_>) -> String {
    let mut s = el.attrs().map(|a| format!("{}=\"{}\"", a.name, a.value)).collect::<Vec<_>>().join(" ");
    for c in el.child_elements() {
        s.push(' ');
        s.push_str(&serialize_loose(c));
    }
    s
}

fn read_dynamic_array_cm(pkg: &Package, part: &str) -> Vec<f64> {
    let Ok(Some(d)) = doc(pkg, part) else { return Vec::new() };
    let root = d.root();
    let types: Vec<String> = root
        .first_child("metadataTypes")
        .unwrap_or(root)
        .children_named("metadataType")
        .map(|t| t.attr("name").unwrap_or("").to_owned())
        .collect();
    let mut future: Vec<(String, Vec<bool>)> = Vec::new();
    for fm in root.children_named("futureMetadata") {
        let flags: Vec<bool> = fm
            .children_named("bk")
            .map(|bk| {
                let s = serialize_loose(bk);
                s.contains("fDynamic=\"1\"") || s.contains("fDynamic=\"true\"")
            })
            .collect();
        let name = fm.attr("name").unwrap_or("").to_owned();
        match future.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = flags,
            None => future.push((name, flags)),
        }
    }
    let mut out = Vec::new();
    for (i, bk) in root.first_child("cellMetadata").unwrap_or(root).children_named("bk").enumerate() {
        for rc in bk.children_named("rc") {
            let t = pi(rc.attr("t").unwrap_or("0")) - 1.0;
            if types.get(uo_jsnum::conv::js_index(t, types.len()).unwrap_or(usize::MAX)).map(String::as_str) != Some("XLDAPR") {
                continue;
            }
            let flags = future.iter().find(|(k, _)| k == "XLDAPR").map(|(_, f)| f);
            let v = pi(rc.attr("v").unwrap_or("0"));
            let flag = flags.and_then(|f| uo_jsnum::conv::js_index(v, f.len()).and_then(|k| f.get(k))).copied().unwrap_or(true);
            if flag {
                out.push(f64::from(u32::try_from(i + 1).unwrap_or(u32::MAX)));
                break;
            }
        }
    }
    out
}
