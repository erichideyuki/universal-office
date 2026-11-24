//! Dense-border workbooks for the C3c benchmark (included next to `paint_pkg.rs`).

/// A styles part with `13 x 4` xfs: one border per style (all four sides, distinct colours) x fills none/solid/gray125/
/// darkTrellis (xf = border * 4 + fill + 1; xf 0 is plain).
pub fn styles_dense() -> String {
    const ST: [&str; 13] = [
        "hair",
        "dashDotDot",
        "dashDot",
        "dotted",
        "dashed",
        "thin",
        "mediumDashDotDot",
        "slantDashDot",
        "mediumDashDot",
        "mediumDashed",
        "medium",
        "thick",
        "double",
    ];
    let mut borders = String::from("<border><left/><right/><top/><bottom/><diagonal/></border>");
    for (k, s) in ST.iter().enumerate() {
        let c = |t: &str| format!(r#"<{t} style="{s}"><color rgb="FF{:02X}{:02X}{:02X}"/></{t}>"#, k * 19 % 256, 255 - k * 13, k * 7 % 256);
        borders.push_str(&format!("<border>{}{}{}{}<diagonal/></border>", c("left"), c("right"), c("top"), c("bottom")));
    }
    let mut xfs = String::from(r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>"#);
    for b in 0..13 {
        for f in 0..4 {
            xfs.push_str(&format!(r#"<xf numFmtId="0" fontId="0" fillId="{f}" borderId="{}"/>"#, b + 1));
        }
    }
    let ns = super::paint_pkg::NS;
    format!(
        r#"<?xml version="1.0"?><styleSheet xmlns="{ns}"><fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/></font></fonts><fills count="4"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="darkTrellis"><fgColor rgb="FF0000FF"/><bgColor rgb="FFFFFFFF"/></patternFill></fill></fills><borders count="14">{borders}</borders><cellXfs count="53">{xfs}</cellXfs><dxfs count="1"><dxf><font><b/></font></dxf></dxfs></styleSheet>"#
    )
}

/// A worksheet of `rows` x `cols` numbers with xf `(r * 7 + c * 13) % 53` (dense borders and fills).
pub fn sheet_dense(rows: u32, cols: u32) -> String {
    let ns = super::paint_pkg::NS;
    let mut s = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{ns}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>"#
    );
    for r in 1..=rows {
        s.push_str(&format!("<row r=\"{r}\">"));
        for c in 0..cols {
            let name = if c < 26 {
                char::from(b'A' + u8::try_from(c).unwrap_or(0)).to_string()
            } else {
                format!("A{}", char::from(b'A' + u8::try_from(c - 26).unwrap_or(0)))
            };
            s.push_str(&format!("<c r=\"{name}{r}\" s=\"{}\"><v>{}</v></c>", (r * 7 + c * 13) % 53, (r * 31 + c) % 100));
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData></worksheet>");
    s
}
