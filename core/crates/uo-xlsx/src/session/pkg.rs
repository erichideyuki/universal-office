//! Package edits of the element: the blank workbook (`blankWorkbook`, `sheet-core/src/blank.ts`, over
//! `XlsxPackage.empty()`) and Insert › Sheet (`addSheetPart`, `universal-sheet.ts:4559-4590`).

use uo_opc::package::{CaseFold, OpenOptions, SaveOptions};
use uo_opc::{Limits, Package};

use super::SessionError;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn opts() -> OpenOptions {
    OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }
}

const XML_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

/// `blankXlsx({ sheetName })`: the minimal package with a complete `styles.xml` (Normal font Aptos Narrow 11).
pub(crate) fn blank_xlsx(sheet_name: &str) -> Result<Vec<u8>, SessionError> {
    let name = String::from_utf16_lossy(&sheet_name.encode_utf16().take(31).collect::<Vec<u16>>());
    let ct = format!(
        "{XML_HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>"
    );
    let rels = format!(
        "{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    );
    let wb = format!(
        "{XML_HEAD}<workbook {NS}><bookViews><workbookView activeTab=\"0\"/></bookViews><sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets><calcPr calcId=\"191029\"/></workbook>",
        esc(&name)
    );
    let wb_rels = format!(
        "{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>"
    );
    let sheet = format!(
        "{XML_HEAD}<worksheet {NS}><dimension ref=\"A1\"/><sheetViews><sheetView tabSelected=\"1\" workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>"
    );
    let styles = format!(
        "{XML_HEAD}<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Aptos Narrow\"/><family val=\"2\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles><dxfs count=\"0\"/><tableStyles count=\"0\"/></styleSheet>"
    );
    // the order of `XlsxPackage.empty()` + `setPartText` (new parts are appended)
    let parts = vec![
        ("[Content_Types].xml".to_owned(), ct.into_bytes()),
        ("_rels/.rels".to_owned(), rels.into_bytes()),
        ("xl/workbook.xml".to_owned(), wb.into_bytes()),
        ("xl/_rels/workbook.xml.rels".to_owned(), wb_rels.into_bytes()),
        ("xl/worksheets/sheet1.xml".to_owned(), sheet.into_bytes()),
        ("xl/styles.xml".to_owned(), styles.into_bytes()),
    ];
    let pkg = Package::from_parts(parts, &Limits::DEFAULT, &opts());
    pkg.save(&SaveOptions::default()).map_err(|e| SessionError::Save(crate::save::SaveError::Opc(e)))
}

/// The numbers of `<pattern>"(\d+)"` matches (`[...s.matchAll(re)].map((m) => Number(m[1]))`).
fn numbers_after(s: &str, prefix: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find(prefix) {
        let at = from + i + prefix.len();
        let digits: String = s[at..].chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && s[at + digits.len()..].starts_with('"') {
            out.push(uo_jsnum::parse::to_number(&digits));
        }
        from = at;
    }
    out
}

/// `addSheetPart(pkg, name)`: a blank worksheet at the end (`workbook.xml`, its rels, content types, the part).
pub(crate) fn add_sheet_part(bytes: &[u8], name: &str) -> Result<Vec<u8>, SessionError> {
    let to_err = |e: uo_opc::Error| SessionError::Save(crate::save::SaveError::Opc(e));
    let mut pkg = Package::open(bytes, &Limits::DEFAULT, &opts()).map_err(to_err)?;
    let wb = pkg.find_text("xl/workbook.xml").map_err(to_err)?.map(std::borrow::Cow::into_owned).ok_or(SessionError::NoSheet)?;
    let rels_path = "xl/_rels/workbook.xml.rels";
    let rels = pkg.find_text(rels_path).map_err(to_err)?.map(std::borrow::Cow::into_owned).unwrap_or_else(|| {
        format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>")
    });
    let mut k = 1;
    while pkg.contains(&format!("xl/worksheets/sheet{k}.xml")) {
        k += 1;
    }
    // `\bId="rId(\d+)"`: an `Id` attribute (word boundary before it)
    let ids: Vec<f64> = {
        let mut v = Vec::new();
        let mut from = 0;
        while let Some(i) = rels[from..].find("Id=\"rId") {
            let at = from + i;
            let boundary = at == 0 || !rels.as_bytes()[at - 1].is_ascii_alphanumeric() && rels.as_bytes()[at - 1] != b'_';
            let start = at + "Id=\"rId".len();
            let digits: String = rels[start..].chars().take_while(char::is_ascii_digit).collect();
            if boundary && !digits.is_empty() && rels[start + digits.len()..].starts_with('"') {
                v.push(uo_jsnum::parse::to_number(&digits));
            }
            from = start;
        }
        v
    };
    let rid = format!("rId{}", uo_jsnum::format::to_js_string(ids.iter().copied().fold(0.0, uo_jsnum::math::max) + 1.0));
    let sheet_ids: Vec<f64> = {
        // `<(?:\w+:)?sheet\b[^>]*\bsheetId="(\d+)"`
        let mut v = Vec::new();
        for (i, _) in wb.match_indices("sheet") {
            if i == 0 || wb.as_bytes()[i - 1] != b'<' && wb.as_bytes()[i - 1] != b':' {
                continue;
            }
            let after = &wb[i + 5..];
            if after.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let end = after.find('>').unwrap_or(after.len());
            v.extend(numbers_after(&after[..end], " sheetId=\""));
        }
        v
    };
    let sheet_id = uo_jsnum::format::to_js_string(sheet_ids.iter().copied().fold(0.0, uo_jsnum::math::max) + 1.0);
    // `<((?:\w+:)?)sheet\b[^>]*\s(\w+):id="`: the prefix of the element and of the relationship attribute
    let (p, rp) = {
        let mut found = (String::new(), "r".to_owned());
        for (i, _) in wb.match_indices("sheet") {
            let bytes = wb.as_bytes();
            let mut s = i;
            while s > 0 && (bytes[s - 1].is_ascii_alphanumeric() || bytes[s - 1] == b'_' || bytes[s - 1] == b':') {
                s -= 1;
            }
            if s == 0 || bytes[s - 1] != b'<' {
                continue;
            }
            let pre = &wb[s..i];
            if !(pre.is_empty() || pre.ends_with(':') && pre[..pre.len() - 1].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
                continue;
            }
            let after = &wb[i + 5..];
            if after.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let end = after.find('>').unwrap_or(after.len());
            let tag = &after[..end];
            if let Some(j) = tag.find(":id=\"") {
                let head = &tag[..j];
                let name: String = head
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let before = &head[..head.len() - name.len()];
                if !name.is_empty() && before.ends_with(uo_jsnum::text::is_js_space) {
                    found = (pre.to_owned(), name);
                    break;
                }
            }
        }
        found
    };
    let close_tag = format!("</{p}sheets>");
    let Some(close) = wb.find(&close_tag) else { return Err(SessionError::BadRef("workbook.xml sem <sheets>".to_owned())) };
    let wb2 = format!("{}<{p}sheet name=\"{}\" sheetId=\"{sheet_id}\" {rp}:id=\"{rid}\"/>{}", &wb[..close], esc(name), &wb[close..]);
    pkg.set_part_text("xl/workbook.xml", wb2);
    let rels_close = rels.rfind("</Relationships>").unwrap_or(rels.len());
    let rels2 = format!(
        "{}<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{k}.xml\"/>{}",
        &rels[..rels_close],
        &rels[rels_close..]
    );
    pkg.set_part_text(rels_path, rels2);
    if let Some(ct) = pkg.find_text("[Content_Types].xml").map_err(to_err)?.map(std::borrow::Cow::into_owned)
        && !ct.contains(&format!("/xl/worksheets/sheet{k}.xml\""))
    {
        let at = ct.rfind("</Types>").unwrap_or(ct.len());
        let ct2 = format!(
            "{}<Override PartName=\"/xl/worksheets/sheet{k}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>{}",
            &ct[..at],
            &ct[at..]
        );
        pkg.set_part_text("[Content_Types].xml", ct2);
    }
    pkg.set_part_text(
        format!("xl/worksheets/sheet{k}.xml"),
        format!(
            "{XML_HEAD}<worksheet {NS}><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>"
        ),
    );
    pkg.save(&SaveOptions::default()).map_err(to_err)
}
