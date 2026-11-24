//! The parts the CALC touches when the `.xlsx` is written (`save-calc.ts`, record there): the calc chain goes
//! (a stale one makes Excel repair the file), `<calcPr>` attributes, the dynamic-array metadata (`metadata.xml`,
//! XLDAPR) and the defined names. Surgical text edits on the original parts; the regular expressions are the
//! TS's, run on `regress` (see `rx.rs`).

use uo_opc::Package;

use super::SaveError;
use super::rx::Rx;
use super::text::{esc_attr, esc_text, put_num};
use crate::read::model::DefinedName;

const WB: &str = "xl/workbook.xml";
const WB_RELS: &str = "xl/_rels/workbook.xml.rels";
const CT: &str = "[Content_Types].xml";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const METADATA_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml";

/// `getPartText(path)` (case-insensitive name fallback, as `XlsxPackage.caseless`).
fn text(pkg: &Package, name: &str) -> Result<Option<String>, SaveError> {
    Ok(pkg.find_text(name)?.map(std::borrow::Cow::into_owned))
}

/// `setPartText` (the exact name; a new one goes last).
fn set(pkg: &mut Package, name: &str, xml: String) {
    pkg.set_part_text(name, xml);
}

/// `deletePart`: the exact name, else the case-insensitive match.
fn delete_part(pkg: &mut Package, name: &str) -> bool {
    let real = if pkg.contains(name) { Some(name.to_owned()) } else { pkg.find_name(name).map(str::to_owned) };
    real.is_some_and(|r| pkg.remove_part(&r))
}

/// `resolveRelTarget` (`package.ts`).
fn resolve_rel_target(from: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let mut dir = from.rfind('/').map_or("", |i| &from[..i]).to_owned();
    let mut t = target;
    while let Some(rest) = t.strip_prefix("../") {
        t = rest;
        dir = dir.rfind('/').map_or(String::new(), |i| dir[..i].to_owned());
    }
    if dir.is_empty() { t.to_owned() } else { format!("{dir}/{t}") }
}

/// `attr(el, name)`.
fn attr(el: &str, name: &str) -> Result<Option<String>, SaveError> {
    let re = Rx::new(&format!(r#"\s{name}\s*=\s*("([^"]*)"|'([^']*)')"#))?;
    Ok(re.find(el, 0).and_then(|m| m.group(2).or_else(|| m.group(3))).map(|r| el[r].to_owned()))
}

struct Rel {
    target: String,
    el: String,
}

/// `wbRels`: the relationships of the workbook with `type`.
fn wb_rels(pkg: &Package, ty: &str) -> Result<Vec<Rel>, SaveError> {
    let xml = text(pkg, WB_RELS)?.unwrap_or_default();
    let re = Rx::new(r"<Relationship\b[^>]*/>|<Relationship\b[^>]*>[\s\S]*?</Relationship>")?;
    let mut out = Vec::new();
    for m in re.find_all(&xml) {
        let el = &xml[m.start()..m.end()];
        if attr(el, "Type")?.as_deref() != Some(ty) {
            continue;
        }
        out.push(Rel { target: attr(el, "Target")?.unwrap_or_default(), el: el.to_owned() });
    }
    Ok(out)
}

/// `s.replace(from, '')` for a string pattern: the first occurrence.
fn replace_first(s: &str, from: &str, to: &str) -> String {
    match s.find(from) {
        Some(i) => format!("{}{to}{}", &s[..i], &s[i + from.len()..]),
        None => s.to_owned(),
    }
}

/// `part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')`.
fn regex_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// `removeCalcChain`: the part, the relationship and the content-type override go. Returns whether there was one.
pub(crate) fn remove_calc_chain(pkg: &mut Package) -> Result<bool, SaveError> {
    let rels = wb_rels(pkg, &format!("{REL_NS}/calcChain"))?;
    let mut removed = false;
    for r in &rels {
        let part = resolve_rel_target(WB, &r.target);
        if delete_part(pkg, &part) {
            removed = true;
        }
        let cur = text(pkg, WB_RELS)?.unwrap_or_default();
        set(pkg, WB_RELS, replace_first(&cur, &r.el, ""));
        if let Some(ct) = text(pkg, CT)? {
            let re = Rx::new_i(&format!(r#"<Override\b[^>]*PartName="/{}"[^>]*/>"#, regex_escape(&part)))?;
            let new = match re.find(&ct, 0) {
                Some(m) => format!("{}{}", &ct[..m.start()], &ct[m.end()..]),
                None => ct,
            };
            set(pkg, CT, new);
        }
    }
    if rels.is_empty() && delete_part(pkg, "xl/calcChain.xml") {
        removed = true;
    }
    Ok(removed)
}

/// Elements of CT_Workbook after `<calcPr>`.
const AFTER_CALCPR: [&str; 9] = [
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

fn workbook_xml(pkg: &Package) -> Result<String, SaveError> {
    text(pkg, WB)?.ok_or(SaveError::Frame("package without xl/workbook.xml"))
}

fn workbook_prefix(xml: &str) -> Result<String, SaveError> {
    Ok(Rx::new(r"<(\w+:)?workbook\b")?.find(xml, 0).and_then(|m| m.group(1)).map(|r| xml[r].to_owned()).unwrap_or_default())
}

/// Position before the first of `names` (workbook level), else before `</workbook>`.
fn workbook_insertion(xml: &str, prefix: &str, names: &[&str]) -> Result<usize, SaveError> {
    let mut at: Option<usize> = None;
    for name in names {
        if let Some(m) = Rx::new(&format!(r"<{prefix}{name}[\s/>]"))?.find(xml, 0)
            && at.is_none_or(|a| m.start() < a)
        {
            at = Some(m.start());
        }
    }
    at.or_else(|| xml.rfind(&format!("</{prefix}workbook>"))).ok_or(SaveError::Frame("workbook without a closing tag"))
}

/// `updateCalcPr`: attributes of `<calcPr>` replaced (`None` removes the attribute), the element created at its
/// schema position when missing.
pub(crate) fn update_calc_pr(pkg: &mut Package, patch: &[(&str, Option<&str>)]) -> Result<(), SaveError> {
    let mut xml = workbook_xml(pkg)?;
    let re = Rx::new(r"<calcPr\b([^>]*?)(/>|>([\s\S]*?)</calcPr>)")?;
    if let Some(m) = re.find(&xml, 0) {
        let mut attrs = m.group(1).map(|r| xml[r].to_owned()).unwrap_or_default();
        for (k, v) in patch {
            let are = Rx::new(&format!(r#"\s{k}\s*=\s*("[^"]*"|'[^']*')"#))?;
            match (v, are.find(&attrs, 0)) {
                (None, Some(a)) => attrs = format!("{}{}", &attrs[..a.start()], &attrs[a.end()..]),
                (None, None) => {}
                (Some(v), Some(a)) => attrs = format!("{} {k}=\"{}\"{}", &attrs[..a.start()], esc_attr(v), &attrs[a.end()..]),
                (Some(v), None) => attrs.push_str(&format!(" {k}=\"{}\"", esc_attr(v))),
            }
        }
        let el = match m.group(3) {
            Some(r) => format!("<calcPr{attrs}>{}</calcPr>", &xml[r]),
            None => format!("<calcPr{attrs}/>"),
        };
        xml = format!("{}{el}{}", &xml[..m.start()], &xml[m.end()..]);
    } else {
        let attrs: String = patch.iter().filter_map(|(k, v)| v.map(|v| format!(" {k}=\"{}\"", esc_attr(v)))).collect();
        let prefix = workbook_prefix(&xml)?;
        let el = format!("<{prefix}calcPr{attrs}/>");
        let at = workbook_insertion(&xml, &prefix, &AFTER_CALCPR)?;
        xml = format!("{}{el}{}", &xml[..at], &xml[at..]);
    }
    set(pkg, WB, xml);
    Ok(())
}

const XLDAPR_TYPE: &str = "<metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/>";
const XLDAPR_BK: &str = "<bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk>";
const XDA_NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray";

/// `bump(attrs)`: `count="n"` becomes `n + 1`.
fn bump(attrs: &str) -> Result<String, SaveError> {
    let re = Rx::new(r#"\scount="(\d+)""#)?;
    Ok(match re.find(attrs, 0) {
        Some(m) => {
            let n = m.group(1).map_or(0.0, |r| uo_jsnum::parse::parse_float(&attrs[r]));
            let mut b = Vec::new();
            put_num(&mut b, n + 1.0);
            format!("{} count=\"{}\"{}", &attrs[..m.start()], String::from_utf8_lossy(&b), &attrs[m.end()..])
        }
        None => attrs.to_owned(),
    })
}

/// `addWbRel`.
fn add_wb_rel(pkg: &mut Package, ty: &str, target: &str) -> Result<String, SaveError> {
    let xml = text(pkg, WB_RELS)?.unwrap_or_else(|| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_owned()
    });
    let ids: Vec<String> =
        Rx::new(r#"\sId="([^"]*)""#)?.find_all(&xml).iter().filter_map(|m| m.group(1)).map(|r| xml[r].to_owned()).collect();
    let uniq: std::collections::BTreeSet<&String> = ids.iter().collect();
    let mut n = uniq.len() + 1;
    while ids.contains(&format!("rId{n}")) {
        n += 1;
    }
    let id = format!("rId{n}");
    let add = format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{}\"/></Relationships>", esc_attr(target));
    set(pkg, WB_RELS, replace_first(&xml, "</Relationships>", &add));
    Ok(id)
}

/// `ensureDynamicArrayCm`: the `cm` (1-based) of a dynamic array: what the metadata has or a new one (the part,
/// its relationship and its content type are created when missing).
pub(crate) fn ensure_dynamic_array_cm(pkg: &mut Package) -> Result<f64, SaveError> {
    let metadata_rel = format!("{REL_NS}/sheetMetadata");
    let rels = wb_rels(pkg, &metadata_rel)?;
    let rel = rels.first();
    let part = rel.map_or_else(|| "xl/metadata.xml".to_owned(), |r| resolve_rel_target(WB, &r.target));
    let Some(mut xml) = text(pkg, &part)? else {
        set(
            pkg,
            &part,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<metadata xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:xda=\"{XDA_NS}\"><metadataTypes count=\"1\">{XLDAPR_TYPE}</metadataTypes><futureMetadata name=\"XLDAPR\" count=\"1\">{XLDAPR_BK}</futureMetadata><cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata></metadata>"
            ),
        );
        if rel.is_none() {
            let target = part.strip_prefix("xl/").unwrap_or(&part).to_owned();
            add_wb_rel(pkg, &metadata_rel, &target)?;
        }
        if let Some(ct) = text(pkg, CT)?
            && !ct.contains(&format!("PartName=\"/{part}\""))
        {
            let add = format!("<Override PartName=\"/{part}\" ContentType=\"{METADATA_CT}\"/></Types>");
            set(pkg, CT, replace_first(&ct, "</Types>", &add));
        }
        return Ok(1.0);
    };
    // types (1-based) and the XLDAPR blocks of futureMetadata
    let types: Vec<String> = Rx::new(r#"<metadataType\b[^>]*?name="([^"]*)""#)?
        .find_all(&xml)
        .iter()
        .filter_map(|m| m.group(1))
        .map(|r| xml[r].to_owned())
        .collect();
    let mut t = types.iter().position(|x| x == "XLDAPR").map_or(0, |i| i + 1);
    let fm = Rx::new(r#"<futureMetadata\b[^>]*name="XLDAPR"[^>]*>([\s\S]*?)</futureMetadata>"#)?.find(&xml, 0);
    let bks: Vec<String> = match fm.as_ref().and_then(|m| m.group(1)) {
        Some(g) => {
            let inner = &xml[g];
            Rx::new(r"<bk>[\s\S]*?</bk>")?.find_all(inner).iter().map(|m| inner[m.start()..m.end()].to_owned()).collect()
        }
        None => Vec::new(),
    };
    let dyn_re = Rx::new(r#"fDynamic="(1|true)""#)?;
    let mut v: isize = bks.iter().position(|b| dyn_re.find(b, 0).is_some()).map_or(-1, |i| i as isize);
    let cm_block = Rx::new(r"<cellMetadata\b[^>]*>([\s\S]*?)</cellMetadata>")?.find(&xml, 0);
    if t > 0
        && v >= 0
        && let Some(cb) = cm_block.as_ref().and_then(|m| m.group(1))
    {
        let inner = &xml[cb];
        let rc = Rx::new(&format!(r#"<rc\s+t="{t}"\s+v="{v}"\s*/>"#))?;
        let cbks = Rx::new(r"<bk>([\s\S]*?)</bk>")?.find_all(inner);
        if let Some(i) = cbks.iter().position(|b| b.group(1).is_some_and(|g| rc.find(&inner[g], 0).is_some())) {
            return Ok((i + 1) as f64);
        }
    }
    let splice = |xml: &str, at: usize, end: usize, with: &str| format!("{}{with}{}", &xml[..at], &xml[end..]);
    if !xml.contains(&format!("xmlns:xda=\"{XDA_NS}\"")) {
        // `xml.replace(/<metadata\b/, '<metadata xmlns:xda="…"')`
        if let Some(m) = Rx::new(r"<metadata\b")?.find(&xml, 0) {
            xml = splice(&xml, m.start(), m.end(), &format!("<metadata xmlns:xda=\"{XDA_NS}\""));
        }
    }
    if t == 0 {
        if Rx::new(r"<metadataTypes\b")?.find(&xml, 0).is_some() {
            if let Some(m) = Rx::new(r"<metadataTypes\b([^>]*)>([\s\S]*?)</metadataTypes>")?.find(&xml, 0) {
                let a = bump(&xml[m.group(1).unwrap_or(0..0)])?;
                let body = xml[m.group(2).unwrap_or(0..0)].to_owned();
                xml = splice(&xml, m.start(), m.end(), &format!("<metadataTypes{a}>{body}{XLDAPR_TYPE}</metadataTypes>"));
            }
        } else if let Some(m) = Rx::new(r"(<metadata\b[^>]*>)")?.find(&xml, 0) {
            let ins = format!("<metadataTypes count=\"1\">{XLDAPR_TYPE}</metadataTypes>");
            xml = splice(&xml, m.end(), m.end(), &ins);
        }
        t = types.len() + 1;
    }
    if v < 0 {
        if fm.is_some() {
            if let Some(m) = Rx::new(r#"<futureMetadata\b([^>]*name="XLDAPR"[^>]*)>([\s\S]*?)</futureMetadata>"#)?.find(&xml, 0) {
                let a = bump(&xml[m.group(1).unwrap_or(0..0)])?;
                let body = xml[m.group(2).unwrap_or(0..0)].to_owned();
                xml = splice(&xml, m.start(), m.end(), &format!("<futureMetadata{a}>{body}{XLDAPR_BK}</futureMetadata>"));
            }
            v = bks.len() as isize;
        } else {
            let fut = format!("<futureMetadata name=\"XLDAPR\" count=\"1\">{XLDAPR_BK}</futureMetadata>");
            let all = Rx::new(r"</mdxMetadata>|</metadataStrings>|</metadataTypes>")?.find_all(&xml);
            if let Some(last) = all.last() {
                let at = last.end();
                xml = splice(&xml, at, at, &fut);
            } else if let Some(m) = Rx::new(r"(<metadata\b[^>]*>)")?.find(&xml, 0) {
                xml = splice(&xml, m.end(), m.end(), &fut);
            }
            v = 0;
        }
    }
    let cm: f64 = if Rx::new(r"<cellMetadata\b")?.find(&xml, 0).is_some() {
        let n = match Rx::new(r"<cellMetadata\b[^>]*>([\s\S]*?)</cellMetadata>")?.find(&xml, 0).and_then(|m| m.group(1)) {
            Some(g) => Rx::new(r"<bk>")?.find_all(&xml[g]).len(),
            None => 0,
        };
        if let Some(m) = Rx::new(r"<cellMetadata\b([^>]*)>([\s\S]*?)</cellMetadata>")?.find(&xml, 0) {
            let a = bump(&xml[m.group(1).unwrap_or(0..0)])?;
            let body = xml[m.group(2).unwrap_or(0..0)].to_owned();
            xml = splice(&xml, m.start(), m.end(), &format!("<cellMetadata{a}>{body}<bk><rc t=\"{t}\" v=\"{v}\"/></bk></cellMetadata>"));
        }
        (n + 1) as f64
    } else {
        let el = format!("<cellMetadata count=\"1\"><bk><rc t=\"{t}\" v=\"{v}\"/></bk></cellMetadata>");
        if let Some(m) = Rx::new(r"<valueMetadata\b")?.find(&xml, 0) {
            xml = splice(&xml, m.start(), m.start(), &el);
        } else if let Some(m) = Rx::new(r"</metadata>")?.find(&xml, 0) {
            xml = splice(&xml, m.start(), m.start(), &el);
        }
        1.0
    };
    set(pkg, &part, xml);
    Ok(cm)
}

/// `nameKey`.
fn name_key(name: &str, local: Option<f64>) -> String {
    let mut b = Vec::new();
    put_num(&mut b, local.unwrap_or(-1.0));
    format!("{}|{}", uo_text::case::to_uppercase_str(name), String::from_utf8_lossy(&b))
}

/// `updateDefinedNames`: creates/changes/removes defined names without touching the others (attributes the
/// model does not keep stay). `formula` is the canonical file text without `=`. The block is created before
/// `<calcPr>` and goes away when empty.
pub fn update_defined_names(pkg: &mut Package, upserts: &[DefinedName], removals: &[(String, Option<f64>)]) -> Result<(), SaveError> {
    let mut xml = workbook_xml(pkg)?;
    let prefix = workbook_prefix(&xml)?;
    let el = |d: &DefinedName, keep: &str| -> String {
        let mut s = format!("<{prefix}definedName name=\"{}\"", esc_attr(&d.name));
        if let Some(l) = d.local_sheet_id {
            let mut b = Vec::new();
            put_num(&mut b, l);
            s.push_str(&format!(" localSheetId=\"{}\"", String::from_utf8_lossy(&b)));
        }
        if d.hidden {
            s.push_str(" hidden=\"1\"");
        }
        s.push_str(&format!("{keep}>{}</{prefix}definedName>", esc_text(&d.formula)));
        s
    };
    let m = Rx::new(&format!(r"<{prefix}definedNames\b[^>]*?(/>|>([\s\S]*?)</{prefix}definedNames>)"))?.find(&xml, 0);
    let mut items: Vec<String> = Vec::new();
    if let Some(inner) = m.as_ref().and_then(|m| m.group(2)) {
        let body = &xml[inner];
        let re = Rx::new(&format!(r"<{prefix}definedName\b[^>]*?(/>|>[\s\S]*?</{prefix}definedName>)"))?;
        items = re.find_all(body).iter().map(|x| body[x.start()..x.end()].to_owned()).collect();
    }
    let key_of = |item: &str| -> Result<String, SaveError> {
        let ls = attr(item, "localSheetId")?;
        let local = ls.as_deref().filter(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit())).map(uo_jsnum::parse::parse_float);
        Ok(name_key(&attr(item, "name")?.unwrap_or_default(), local))
    };
    let drop: Vec<String> = removals.iter().map(|(n, l)| name_key(n, *l)).collect();
    // `new Map(upserts.map(...))`: later values win, the first position stays
    let mut set_: Vec<(String, &DefinedName)> = Vec::new();
    for d in upserts {
        let k = name_key(&d.name, d.local_sheet_id);
        match set_.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = d,
            None => set_.push((k, d)),
        }
    }
    let attr_re = Rx::new(r#"\s([\w:]+)\s*=\s*("[^"]*"|'[^']*')"#)?;
    let open_re = Rx::new(r"^<[^>]*")?;
    let mut out: Vec<String> = Vec::new();
    for item in &items {
        let k = key_of(item)?;
        if drop.contains(&k) {
            continue;
        }
        let Some(pos) = set_.iter().position(|(x, _)| *x == k) else {
            out.push(item.clone());
            continue;
        };
        let (_, d) = set_.remove(pos);
        let open = open_re.find(item, 0).map_or("", |m| &item[m.start()..m.end()]);
        let mut keep = String::new();
        for a in attr_re.find_all(open) {
            let (Some(n), Some(v)) = (a.group(1), a.group(2)) else { continue };
            let n = &open[n];
            if !["name", "localSheetId", "hidden"].contains(&n) {
                keep.push_str(&format!(" {n}={}", &open[v]));
            }
        }
        out.push(el(d, &keep));
    }
    for (_, d) in &set_ {
        out.push(el(d, ""));
    }
    let block = if out.is_empty() { String::new() } else { format!("<{prefix}definedNames>{}</{prefix}definedNames>", out.concat()) };
    if let Some(m) = m {
        xml = format!("{}{block}{}", &xml[..m.start()], &xml[m.end()..]);
    } else if !block.is_empty() {
        let mut names: Vec<&str> = vec!["calcPr"];
        names.extend(AFTER_CALCPR);
        let at = workbook_insertion(&xml, &prefix, &names)?;
        xml = format!("{}{block}{}", &xml[..at], &xml[at..]);
    }
    set(pkg, WB, xml);
    Ok(())
}
