//! A one-sheet package with tables and pivot tables for the C3c tests and benchmark.

/// The parts of a one-sheet package.
pub struct Parts {
    /// `<worksheet>` XML WITHOUT `<tableParts>` (added from `tables`).
    pub sheet: String,
    /// `xl/styles.xml`.
    pub styles: String,
    /// `xl/tables/tableN.xml` texts.
    pub tables: Vec<String>,
    /// `xl/pivotTables/pivotTableN.xml` texts.
    pub pivots: Vec<String>,
}

pub const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// A styles part: one font, fills `none` + `gray125` + `solid FFFF00`, a thin border and a 4-sided border, three xfs
/// (default; yellow fill; borders), one dxf.
pub fn styles() -> String {
    format!(
        r#"<?xml version="1.0"?><styleSheet xmlns="{NS}"><fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left style="thin"><color rgb="FFFF0000"/></left><right style="medium"><color rgb="FF00FF00"/></right><top style="dashed"><color rgb="FF0000FF"/></top><bottom style="double"><color rgb="FF000000"/></bottom><diagonal/></border></borders><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="2" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="1"/></cellXfs><dxfs count="1"><dxf><font><b/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs></styleSheet>"#
    )
}

/// A table part over `ref_` with the style `style`.
pub fn table(n: u32, ref_: &str, style: &str, totals: bool) -> String {
    format!(
        r#"<?xml version="1.0"?><table xmlns="{NS}" id="{n}" name="T{n}" displayName="T{n}" ref="{ref_}"{}><tableColumns count="1"><tableColumn id="1" name="a"/></tableColumns><tableStyleInfo name="{style}" showFirstColumn="1" showLastColumn="1" showRowStripes="1" showColumnStripes="0"/></table>"#,
        if totals { r#" totalsRowCount="1""# } else { "" }
    )
}

/// The package.
pub fn package(p: &Parts) -> Vec<u8> {
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let pr = "http://schemas.openxmlformats.org/package/2006/relationships";
    let mut rels = String::new();
    let mut ct = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut tp = String::new();
    for (i, t) in p.tables.iter().enumerate() {
        let n = i + 1;
        rels.push_str(&format!(r#"<Relationship Id="rIdT{n}" Type="{rel}/table" Target="../tables/table{n}.xml"/>"#));
        tp.push_str(&format!(r#"<tablePart r:id="rIdT{n}"/>"#));
        ct.push_str(&format!(r#"<Override PartName="/xl/tables/table{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>"#));
        parts.push((format!("xl/tables/table{n}.xml"), t.clone()));
    }
    for (i, t) in p.pivots.iter().enumerate() {
        let n = i + 1;
        rels.push_str(&format!(r#"<Relationship Id="rIdP{n}" Type="{rel}/pivotTable" Target="../pivotTables/pivotTable{n}.xml"/>"#));
        ct.push_str(&format!(r#"<Override PartName="/xl/pivotTables/pivotTable{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml"/>"#));
        parts.push((format!("xl/pivotTables/pivotTable{n}.xml"), t.clone()));
    }
    let sheet = if tp.is_empty() {
        p.sheet.clone()
    } else {
        p.sheet.replace("</worksheet>", &format!("<tableParts count=\"{}\">{tp}</tableParts></worksheet>", p.tables.len()))
    };
    let mut all: Vec<(String, String)> = vec![
        (
            "[Content_Types].xml".into(),
            format!(
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>{ct}</Types>"#
            ),
        ),
        (
            "_rels/.rels".into(),
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="{pr}"><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml".into(),
            format!(
                r#"<?xml version="1.0"?><workbook xmlns="{NS}" xmlns:r="{rel}"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels".into(),
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="{pr}"><Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{rel}/styles" Target="styles.xml"/></Relationships>"#
            ),
        ),
        ("xl/styles.xml".into(), p.styles.clone()),
        ("xl/worksheets/sheet1.xml".into(), sheet),
    ];
    if !rels.is_empty() {
        all.push((
            "xl/worksheets/_rels/sheet1.xml.rels".into(),
            format!(r#"<?xml version="1.0"?><Relationships xmlns="{pr}">{rels}</Relationships>"#),
        ));
    }
    all.extend(parts);
    uo_opc::zip::write(all.iter().map(|(n, b)| (n.as_str(), b.as_bytes())), uo_opc::zip::WriteOptions::default()).unwrap_or_default()
}

/// A worksheet of `rows` x `cols` numbers (style 1 on every 7th cell, style 2 on every 11th) around `extra` XML.
pub fn sheet(rows: u32, cols: u32, extra: &str) -> String {
    let mut s = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>"#
    );
    for r in 1..=rows {
        s.push_str(&format!("<row r=\"{r}\">"));
        for c in 0..cols {
            let st = if (r + c) % 11 == 0 {
                " s=\"2\""
            } else if (r + c) % 7 == 0 {
                " s=\"1\""
            } else {
                ""
            };
            let name = if c < 26 {
                char::from(b'A' + u8::try_from(c).unwrap_or(0)).to_string()
            } else {
                format!("A{}", char::from(b'A' + u8::try_from(c - 26).unwrap_or(0)))
            };
            s.push_str(&format!("<c r=\"{name}{r}\"{st}><v>{}</v></c>", (r * 31 + c) % 100));
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");
    s.push_str(extra);
    s.push_str("</worksheet>");
    s
}
