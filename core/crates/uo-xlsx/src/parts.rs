//! The parts written by `finish`: workbook, relationships, content types, document properties.

use uo_opc::xml::write::{Escape, XML_DECL_BARE, escape_into};

use crate::cell::push_u32;
use crate::sheet::{CellRange, push_quoted_sheet};

const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

/// A sheet as `finish` needs it.
pub(crate) struct SheetMeta {
    pub name: String,
    pub print_titles: Option<(u32, u32)>,
    pub names: Vec<crate::sheet::DefinedName>,
}

pub(crate) fn workbook_xml(sheets: &[SheetMeta]) -> String {
    let mut o = String::with_capacity(512 + 128 * sheets.len());
    o.push_str(XML_DECL_BARE);
    o.push_str("<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"");
    o.push_str(NS_REL);
    o.push_str("\"><bookViews><workbookView activeTab=\"0\"/></bookViews><sheets>");
    for (i, s) in sheets.iter().enumerate() {
        o.push_str("<sheet name=\"");
        escape_into(&mut o, &s.name, Escape::ATTR);
        o.push_str("\" sheetId=\"");
        push_u32(&mut o, i as u32 + 1);
        o.push_str("\" r:id=\"rId");
        push_u32(&mut o, i as u32 + 1);
        o.push_str("\"/>");
    }
    o.push_str("</sheets>");
    // Excel keeps defined names sorted by name, then scope; any order opens, sorted is what it writes
    let mut names: Vec<(String, Option<u32>, String)> = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        if let Some((a, b)) = s.print_titles {
            let mut v = String::new();
            push_quoted_sheet(&mut v, &s.name);
            v.push_str("!$");
            push_u32(&mut v, a);
            v.push_str(":$");
            push_u32(&mut v, b);
            names.push(("_xlnm.Print_Titles".to_owned(), Some(i as u32), v));
        }
        for n in &s.names {
            let mut v = String::new();
            push_quoted_sheet(&mut v, &s.name);
            v.push('!');
            CellRange::push_absolute(&n.range, &mut v);
            names.push((n.name.clone(), n.sheet_scope.then_some(i as u32), v));
        }
    }
    if !names.is_empty() {
        names.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then(a.1.cmp(&b.1)));
        o.push_str("<definedNames>");
        for (name, scope, value) in &names {
            o.push_str("<definedName name=\"");
            escape_into(&mut o, name, Escape::ATTR);
            o.push('"');
            if let Some(id) = scope {
                o.push_str(" localSheetId=\"");
                push_u32(&mut o, *id);
                o.push('"');
            }
            o.push('>');
            o.push_str(value);
            o.push_str("</definedName>");
        }
        o.push_str("</definedNames>");
    }
    // calcId 191029 (Excel 365): an older id makes Excel recalculate and ask to save on close
    o.push_str("<calcPr calcId=\"191029\"/></workbook>");
    o
}

pub(crate) fn workbook_rels(sheets: usize, sst: bool) -> String {
    let mut o = String::with_capacity(256 + 128 * sheets);
    o.push_str(XML_DECL_BARE);
    o.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    let rel = |o: &mut String, id: u32, ty: &str, target: &str| {
        o.push_str("<Relationship Id=\"rId");
        push_u32(o, id);
        o.push_str("\" Type=\"");
        o.push_str(REL_TYPE);
        o.push_str(ty);
        o.push_str("\" Target=\"");
        o.push_str(target);
        o.push_str("\"/>");
    };
    for i in 1..=sheets as u32 {
        let mut t = String::from("worksheets/sheet");
        push_u32(&mut t, i);
        t.push_str(".xml");
        rel(&mut o, i, "worksheet", &t);
    }
    let n = sheets as u32;
    rel(&mut o, n + 1, "styles", "styles.xml");
    if sst {
        rel(&mut o, n + 2, "sharedStrings", "sharedStrings.xml");
    }
    o.push_str("</Relationships>");
    o
}

/// The relationships of a sheet's external hyperlinks (`rId1`… in link order).
pub(crate) fn push_link_rel(o: &mut String, id: u32, target: &str) {
    o.push_str("<Relationship Id=\"rId");
    push_u32(o, id);
    o.push_str("\" Type=\"");
    o.push_str(REL_TYPE);
    o.push_str("hyperlink\" Target=\"");
    escape_into(o, target, Escape::ATTR_VALID);
    o.push_str("\" TargetMode=\"External\"/>");
}

pub(crate) const ROOT_RELS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>",
    "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>",
    "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>",
    "</Relationships>"
);

pub(crate) fn content_types(sheets: usize, sst: bool) -> String {
    let mut o = String::with_capacity(1024 + 128 * sheets);
    o.push_str(XML_DECL_BARE);
    o.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    o.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    o.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    o.push_str("<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>");
    for i in 1..=sheets as u32 {
        o.push_str("<Override PartName=\"/xl/worksheets/sheet");
        push_u32(&mut o, i);
        o.push_str(".xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>");
    }
    o.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
    if sst {
        o.push_str("<Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>");
    }
    o.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    o.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>");
    o.push_str("</Types>");
    o
}

/// `docProps/core.xml`: title, creator, language and the creation time (W3CDTF, given by the
/// caller: the writer has no clock).
pub(crate) fn core_xml(title: Option<&str>, creator: Option<&str>, language: Option<&str>, created: Option<&str>) -> String {
    let mut o = String::with_capacity(512);
    o.push_str(XML_DECL_BARE);
    o.push_str(
        "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
    );
    let el = |o: &mut String, tag: &str, v: Option<&str>| {
        if let Some(v) = v {
            o.push('<');
            o.push_str(tag);
            o.push('>');
            escape_into(o, v, Escape::TEXT_VALID);
            o.push_str("</");
            o.push_str(tag);
            o.push('>');
        }
    };
    el(&mut o, "dc:title", title);
    el(&mut o, "dc:creator", creator);
    el(&mut o, "dc:language", language);
    if let Some(c) = created {
        o.push_str("<dcterms:created xsi:type=\"dcterms:W3CDTF\">");
        escape_into(&mut o, c, Escape::TEXT_VALID);
        o.push_str("</dcterms:created>");
    }
    o.push_str("</cp:coreProperties>");
    o
}

pub(crate) fn app_xml(application: &str) -> String {
    let mut o = String::with_capacity(256);
    o.push_str(XML_DECL_BARE);
    o.push_str("<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>");
    escape_into(&mut o, application, Escape::TEXT_VALID);
    o.push_str("</Application></Properties>");
    o
}
