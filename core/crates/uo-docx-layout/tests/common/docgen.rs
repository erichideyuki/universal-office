//! Generated test documents (moved from `g4_cancel.rs`): a minimal package with a style sheet, a
//! numbering and settings, paragraphs of words, a section, and the big documents of the cancel
//! and incremental-relayout (LA7) tests and benches.

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A docx with `body` (paragraph XML) and the final `sect` (sectPr XML), plus a style sheet with a
/// heading, a contextual style and the line-number style, and a two-level numbering.
#[must_use]
pub fn docx(body: &str, sect: &str, settings: &str) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}{sect}</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/><w:pPr><w:keepNext/><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="C"><w:name w:val="Corpo"/><w:pPr><w:contextualSpacing/></w:pPr></w:style><w:style w:type="character" w:styleId="LN"><w:name w:val="line number"/></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let settings = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{settings}</w:settings>"#);
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [
        ("[Content_Types].xml", ct.to_owned()),
        ("_rels/.rels", rels.to_owned()),
        ("word/document.xml", doc),
        ("word/_rels/document.xml.rels", doc_rels.to_owned()),
        ("word/styles.xml", styles),
        ("word/numbering.xml", numbering),
        ("word/settings.xml", settings),
    ] {
        if z.add(name, data.as_bytes()).is_err() {
            return Vec::new();
        }
    }
    z.finish()
}

/// `n` Portuguese words, the sequence picked by `seed`.
#[must_use]
pub fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "colunas", "secoes", "informação", "parágrafo", "página", "responsabilidade"];
    (0..n).map(|i| WS.get((i * 7 + seed * 3) % WS.len()).copied().unwrap_or("o")).collect::<Vec<_>>().join(" ")
}

/// A paragraph with properties `ppr` and `n` words.
#[must_use]
pub fn para(ppr: &str, n: usize, seed: usize) -> String {
    format!(r#"<w:p><w:pPr>{ppr}</w:pPr><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, words(n, seed))
}

/// A section: A4, `cols` columns, `extra` sectPr children, the break `kind` element.
#[must_use]
pub fn sect(cols: u32, extra: &str, kind: &str) -> String {
    format!(
        r#"<w:sectPr>{kind}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="{cols}" w:space="708" w:sep="1"/>{extra}</w:sectPr>"#
    )
}

/// A document that takes a release layout several hundred ms: `paragraphs` paragraphs of 80-120
/// words (every 10th keepNext), a PAGE/NUMPAGES field (two or more passes). (`g4_cancel.rs`)
#[must_use]
pub fn big_doc(paragraphs: usize) -> Vec<u8> {
    big_doc_pages(paragraphs, true)
}

/// [`big_doc`] with (`numpages`) or without a NUMPAGES field: without it the document lays out in
/// one pass (a PAGE field only). 310 paragraphs ≈ 50 pages, 3,130 ≈ 500 pages (A4, Liberation
/// Serif 11 pt).
#[must_use]
pub fn big_doc_pages(paragraphs: usize, numpages: bool) -> Vec<u8> {
    let mut b = String::new();
    for i in 0..paragraphs {
        let ppr = if i % 10 == 0 { "<w:keepNext/>" } else { "" };
        b.push_str(&para(ppr, 80 + (i * 7) % 41, i));
        if i == 3 {
            b.push_str(r#"<w:p><w:r><w:t>p. </w:t></w:r><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple>"#);
            if numpages {
                b.push_str(r#"<w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>1</w:t></w:r></w:fldSimple>"#);
            }
            b.push_str("</w:p>");
        }
    }
    docx(&b, &sect(1, "", ""), "")
}
