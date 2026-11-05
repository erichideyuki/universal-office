//! F7d (linked textbox chains, endnote-flow tables): memory (M1 per call, M2 soak open → layout →
//! drop, M3 numbers), the limits and the no-panic loop over hostile chains (a box of the chain in
//! its own content, chains in each other's content, links to missing or hostile ids, duplicated
//! and hostile `seq`s, empty and huge boxes, very long chains) and hostile endnote tables, all
//! through the real path (uo-docx reader → `derive::build` → `engine::layout_document`) with the
//! repository's open fonts.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::{LayoutLimits, LayoutOptions};
use uo_docx_layout::{Layout, LimitKind};
use uo_fonts::FontLimits;
use uo_fonts::registry::{AnswerParts, FaceCache, FontBudget};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS: &str = r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The repository's open fonts behind a fixed family map (one answer per request key).
struct OpenFonts {
    cache: Arc<FaceCache>,
    files: HashMap<String, Arc<[u8]>>,
    answers: Mutex<HashMap<(String, bool, bool), Arc<FontAnswer>>>,
}

impl OpenFonts {
    fn new() -> OpenFonts {
        let mut files = HashMap::new();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            let Ok(rd) = std::fs::read_dir(common::repo().join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "ttf") {
                    let name = p.file_stem().unwrap().to_string_lossy().into_owned();
                    files.insert(name, Arc::from(std::fs::read(&p).unwrap()));
                }
            }
        }
        OpenFonts {
            cache: Arc::new(FaceCache::new(FontBudget::of(uo_fonts::registry::FontProfile::Desktop), FontLimits::default())),
            files,
            answers: Mutex::new(HashMap::new()),
        }
    }
}

impl FontProvider for OpenFonts {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let fam = family.to_lowercase();
        let k = (fam.clone(), bold, italic);
        if let Some(a) = self.answers.lock().unwrap().get(&k) {
            return Some(Arc::clone(a));
        }
        let base = match fam.as_str() {
            "carlito" | "calibri" => "carlito",
            "caladea" | "cambria" => "caladea",
            "liberation serif" | "times new roman" => "liberationserif",
            "liberation mono" | "courier new" => "liberationmono",
            _ => "liberationsans",
        };
        let v = match (bold, italic) {
            (false, false) => "regular",
            (true, false) => "bold",
            (false, true) => "italic",
            (true, true) => "bolditalic",
        };
        let bytes = self.files.get(&format!("{base}-{v}"))?.clone();
        let a = FontAnswer::new(AnswerParts {
            key: format!("{family}|{}{}", u8::from(bold), u8::from(italic)),
            family: family.to_owned(),
            face: self.cache.get_or_parse(bytes).ok()?,
            bold,
            italic,
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            advances: None,
            ticket: None,
        });
        self.answers.lock().unwrap().insert(k, Arc::clone(&a));
        Some(a)
    }
}

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

/// A docx with `body`, an optional header (`hdr`: its paragraphs) and endnotes (`<w:endnote>`s).
fn docx(body: &str, hdr: Option<&str>, endnotes: Option<&str>, settings: &str) -> Vec<u8> {
    let hdr_ref = if hdr.is_some() { r#"<w:headerReference w:type="default" r:id="rH"/>"# } else { "" };
    let sect = format!(
        r#"<w:sectPr>{hdr_ref}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/></w:sectPr>"#
    );
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" {NS}><w:body>{body}{sect}</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="table" w:default="1" w:styleId="TN"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style><w:style w:type="paragraph" w:styleId="ET"><w:name w:val="endnote text"/><w:pPr><w:spacing w:after="0"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr></w:style><w:style w:type="character" w:styleId="ER"><w:name w:val="endnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:pPr><w:ind w:left="360" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let en_pr = if endnotes.is_some() { r#"<w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr>"# } else { "" };
    let settings =
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{en_pr}{settings}</w:settings>"#);
    let over = |p: &str, t: &str| {
        format!(r#"<Override PartName="/{p}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{t}+xml"/>"#)
    };
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    );
    for (p, t) in [
        ("word/document.xml", "document.main"),
        ("word/styles.xml", "styles"),
        ("word/numbering.xml", "numbering"),
        ("word/settings.xml", "settings"),
    ] {
        ct += &over(p, t);
    }
    let mut doc_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="{REL}/styles" Target="styles.xml"/><Relationship Id="rN" Type="{REL}/numbering" Target="numbering.xml"/><Relationship Id="rT" Type="{REL}/settings" Target="settings.xml"/>"#
    );
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("word/document.xml".into(), doc.into_bytes()),
        ("word/styles.xml".into(), styles.into_bytes()),
        ("word/numbering.xml".into(), numbering.into_bytes()),
        ("word/settings.xml".into(), settings.into_bytes()),
    ];
    if let Some(h) = hdr {
        files.push((
            "word/header1.xml".into(),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}" {NS}>{h}</w:hdr>"#).into_bytes(),
        ));
        ct += &over("word/header1.xml", "header");
        doc_rels += &format!(r#"<Relationship Id="rH" Type="{REL}/header" Target="header1.xml"/>"#);
    }
    if let Some(e) = endnotes {
        let sep = r#"<w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote><w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote>"#;
        files.push((
            "word/endnotes.xml".into(),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:endnotes xmlns:w="{W}" {NS}>{sep}{e}</w:endnotes>"#)
                .into_bytes(),
        ));
        ct += &over("word/endnotes.xml", "endnotes");
        doc_rels += &format!(r#"<Relationship Id="rE" Type="{REL}/endnotes" Target="endnotes.xml"/>"#);
    }
    ct += "</Types>";
    doc_rels += "</Relationships>";
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    z.add("[Content_Types].xml", ct.as_bytes()).unwrap();
    z.add("_rels/.rels", rels.as_bytes()).unwrap();
    z.add("word/_rels/document.xml.rels", doc_rels.as_bytes()).unwrap();
    for (name, data) in &files {
        z.add(name, data).unwrap();
    }
    z.finish()
}

fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "caixas", "vinculadas", "informação", "flutuante", "página", "responsabilidade"];
    (0..n).map(|i| WS[(i * 7 + seed * 3) % WS.len()]).collect::<Vec<_>>().join(" ")
}

fn run(n: usize, seed: usize) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{} </w:t></w:r>"#, words(n, seed))
}

fn para(ppr: &str, inner: &str) -> String {
    format!(r#"<w:p><w:pPr>{ppr}</w:pPr>{inner}</w:p>"#)
}

fn cell(w: u32, content: &str) -> String {
    format!(r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/></w:tcPr>{content}</w:tc>"#)
}

fn table(grid: &[u32], rows: &[String]) -> String {
    let g: String = grid.iter().map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect();
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:color="auto"/></w:tblBorders></w:tblPr><w:tblGrid>{g}</w:tblGrid>{}</w:tbl>"#,
        rows.join("")
    )
}

/// The `wps:wsp` of a box: `Ok((story id, content))` for a first box, `Err((link id, seq))` for
/// a linked one (ids and seqs as strings: hostile values go in as written).
fn wsp(size: (u32, u32), story: Result<(&str, &str), (&str, &str)>, ins: &str) -> String {
    let (cx, cy) = (u64::from(size.0) * 635, u64::from(size.1) * 635);
    let inner = match story {
        Ok((id, content)) => format!(r#"<wps:txbx id="{id}"><w:txbxContent>{content}</w:txbxContent></wps:txbx>"#),
        Err((id, seq)) => format!(r#"<wps:linkedTxbx id="{id}" seq="{seq}"/>"#),
    };
    format!(
        r#"<wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="6350"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></wps:spPr>{inner}<wps:bodyPr vert="horz" wrap="square" {ins} anchor="t"><a:noAutofit/></wps:bodyPr></wps:wsp>"#
    )
}

/// A floating box (page-relative offsets in twips).
fn floating(id: u32, at: (i64, i64), size: (u32, u32), story: Result<(&str, &str), (&str, &str)>) -> String {
    let (cx, cy) = (u64::from(size.0) * 635, u64::from(size.1) * 635);
    format!(
        r#"<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="{}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="{id}" name="b{id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">{}</a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#,
        251_658_240 + id,
        at.0 * 635,
        at.1 * 635,
        wsp(size, story, r#"lIns="91440" tIns="45720" rIns="91440" bIns="45720""#)
    )
}

/// An inline box.
fn inline(id: u32, size: (u32, u32), story: Result<(&str, &str), (&str, &str)>) -> String {
    let (cx, cy) = (u64::from(size.0) * 635, u64::from(size.1) * 635);
    format!(
        r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="{id}" name="i{id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">{}</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
        wsp(size, story, "")
    )
}

/// A box inside a floating drawing group.
fn grouped(id: u32, at: (i64, i64), size: (u32, u32), story: Result<(&str, &str), (&str, &str)>) -> String {
    let (cx, cy) = (u64::from(size.0) * 635, u64::from(size.1) * 635);
    format!(
        r#"<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="{}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="{id}" name="g{id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup"><wpg:wgp><wpg:cNvGrpSpPr/><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/><a:chOff x="0" y="0"/><a:chExt cx="{cx}" cy="{cy}"/></a:xfrm></wpg:grpSpPr>{}</wpg:wgp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#,
        251_658_240 + id,
        at.0 * 635,
        at.1 * 635,
        wsp(size, story, "")
    )
}

fn endnote_ref(id: u32) -> String {
    format!(r#"<w:r><w:rPr><w:rStyle w:val="ER"/></w:rPr><w:endnoteReference w:id="{id}"/></w:r>"#)
}

fn layout_bytes(
    bytes: &[u8],
    fonts: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    o: &LayoutOptions,
) -> Result<Layout, LayoutError> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
        .map_err(|_| LayoutError::Internal("test: package"))?;
    let doc =
        uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).map_err(|_| LayoutError::Internal("test: read"))?;
    let ldoc = uo_docx_layout::derive::build(uo_docx::view::DocView::new(&doc), o)?;
    let mut hyph = HyphenSessions::new(Arc::clone(reg));
    uo_docx_layout::engine::layout_document(&ldoc, o, fonts, &mut hyph)
}

fn opts() -> LayoutOptions {
    LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
}

/// Every index the output holds is valid (lines → chars and pfmts, item metadata → items, object
/// content → content lines).
fn check_indices(l: &Layout) {
    for p in &l.pages {
        for line in p.lines.iter().chain(&p.content_lines) {
            if let Some(s) = line.chars {
                assert!((s.start + s.len) as usize <= p.chars.o.len());
            }
            if let Some(f) = line.pfmt {
                assert!((f as usize) < p.pfmts.len());
            }
        }
        for (i, _) in &p.item_meta {
            assert!((*i as usize) < p.page.list.items.len());
        }
        for o in p.objects.iter().flatten() {
            if let Some((r, _)) = &o.content {
                assert!(r.end as usize <= p.content_lines.len());
            }
        }
        assert!(p.hf_items.is_some_and(|h| h as usize <= p.page.list.items.len()));
    }
    assert_eq!(l.image_origins.len(), l.resources.images.len());
}

/// The content of a chain's first box: a numbered paragraph and paragraphs long enough to be cut
/// in the middle of a run, a field, a table of 12 rows cut between rows.
fn chain_content(seed: usize) -> String {
    let mut c = String::new();
    c.push_str(&para(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#, &format!("{}{}", run(30, seed), run(25, seed + 1))));
    c.push_str(&para(
        r#"<w:ind w:firstLine="567"/><w:spacing w:before="120" w:after="120"/>"#,
        &format!(r#"{}<w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple>{}"#, run(40, seed + 2), run(20, seed + 3)),
    ));
    let rows: Vec<String> =
        (0..12).map(|r| format!("<w:tr>{}{}</w:tr>", cell(1200, &para("", &run(2, r))), cell(1600, &para("", &run(5, r + 1))))).collect();
    c.push_str(&table(&[1200, 1600], &rows));
    c.push_str(&para(r#"<w:widowControl w:val="0"/>"#, &run(60, seed + 4)));
    c
}

/// A document of every F7d path: a body chain of four floating boxes (a paragraph cut mid-run, a
/// numbered paragraph continued without its marker, a table cut between rows, the last box
/// overflowing), a chain of an inline first box and a grouped linked box, a header chain, a box
/// linked to a missing id, and endnotes with keepNext paragraphs before tables (one table
/// crossing a page).
fn every_path() -> Vec<u8> {
    let c1 = chain_content(1);
    let mut b = String::new();
    b.push_str(&para(
        "",
        &format!(
            "{}{}{}",
            floating(1, (1440, 1440), (2880, 1440), Ok(("1", &c1))),
            run(20, 1),
            floating(2, (5000, 1440), (2000, 2160), Err(("1", "1")))
        ),
    ));
    b.push_str(&para(
        "",
        &format!(
            "{}{}{}{}",
            run(30, 2),
            floating(3, (1440, 5000), (3600, 1800), Err(("1", "2"))),
            floating(4, (5400, 5000), (2400, 900), Err(("1", "3"))),
            endnote_ref(2)
        ),
    ));
    let c2 = para("", &run(70, 9));
    b.push_str(&para("", &format!("{}{}{}", inline(5, (2400, 700), Ok(("2", &c2))), run(10, 3), endnote_ref(3))));
    b.push_str(&para("", &format!("{}{}", grouped(6, (1440, 9000), (2400, 900), Err(("2", "1"))), run(25, 4))));
    b.push_str(&para("", &format!("{}{}", floating(7, (6000, 9000), (2000, 600), Err(("99", "1"))), run(15, 5))));
    for k in 0..6 {
        b.push_str(&para("", &run(40, 10 + k)));
    }
    let hc = para("", &run(30, 20));
    let hdr = para(
        "",
        &format!(
            "{}{}{}",
            run(3, 21),
            floating(8, (1440, 300), (2000, 400), Ok(("1", &hc))),
            floating(9, (5000, 300), (2000, 400), Err(("1", "1")))
        ),
    );
    let rows: Vec<String> =
        (0..60).map(|r| format!("<w:tr>{}{}</w:tr>", cell(1500, &para("", &run(2, r))), cell(4000, &para("", &run(9, r))))).collect();
    let small: Vec<String> = (0..3).map(|r| format!("<w:tr>{}</w:tr>", cell(3000, &para("", &run(3, r))))).collect();
    let en = format!(
        r#"<w:endnote w:id="2">{}{}{}{}</w:endnote><w:endnote w:id="3">{}{}{}</w:endnote>"#,
        para(r#"<w:pStyle w:val="ET"/>"#, &format!(r#"<w:r><w:rPr><w:rStyle w:val="ER"/></w:rPr><w:endnoteRef/></w:r>{}"#, run(20, 30))),
        para(r#"<w:pStyle w:val="ET"/><w:keepNext/>"#, &run(8, 31)),
        table(&[1500, 4000], &rows),
        para(r#"<w:pStyle w:val="ET"/>"#, &run(10, 32)),
        para(
            r#"<w:pStyle w:val="ET"/><w:keepNext/>"#,
            &format!(r#"<w:r><w:rPr><w:rStyle w:val="ER"/></w:rPr><w:endnoteRef/></w:r>{}"#, run(12, 33))
        ),
        table(&[3000], &small),
        para(r#"<w:pStyle w:val="ET"/>"#, ""),
    );
    docx(
        &b,
        Some(&hdr),
        Some(&en),
        r#"<w:defaultTabStop w:val="708"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>"#,
    )
}

#[test]
fn the_every_path_document_lays_out() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let l = layout_bytes(&every_path(), &fonts, &reg, &opts()).unwrap();
    check_indices(&l);
    // the chain's content flows: several boxes on the first page have content lines, and the
    // endnote tables put cell boxes on the pages
    let p0 = &l.pages[0];
    let with_content = p0.objects.iter().flatten().filter(|o| o.content.as_ref().is_some_and(|(r, _)| r.end > r.start)).count();
    assert!(with_content >= 4, "{with_content} boxes with content");
    assert!(l.pages.len() >= 2, "{} pages", l.pages.len());
    assert!(l.pages.iter().skip(1).any(|p| p.cells.as_ref().is_some_and(|c| !c.is_empty())));
}

#[test]
fn m1_a_layout_call_returns_every_byte() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    let u = assert_no_leak(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    eprintln!("M1 every-path document: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    // a call that ends in a limit mid-pass (inside the endnote flow) returns every byte too
    let tight = LayoutOptions { limits: LayoutLimits { max_pages: 1, ..LayoutLimits::default() }, ..opts() };
    assert_no_leak(|| assert_eq!(layout_bytes(&bytes, &fonts, &reg, &tight).unwrap_err(), LayoutError::Limit(LimitKind::Pages)));
    // and one that ends inside a chain (the work budget)
    let work = LayoutOptions { limits: LayoutLimits { work_floor: 3_000, work_per_glyph: 0, ..LayoutLimits::default() }, ..opts() };
    assert_no_leak(|| assert_eq!(layout_bytes(&bytes, &fonts, &reg, &work).unwrap_err(), LayoutError::Limit(LimitKind::Work)));
}

#[test]
fn m2_soak_open_layout_drop() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let every = every_path();
    let o = opts();
    let n = if cfg!(debug_assertions) { 10 } else { 100 };
    drop(layout_bytes(&every, &fonts, &reg, &o).unwrap());
    let series = soak(n, (n / 10).max(1), |_| drop(layout_bytes(&every, &fonts, &reg, &o).unwrap()));
    eprintln!(
        "M2 soak ({n} × open → layout → drop): first {} last {} min {} max {}",
        series[0],
        series[series.len() - 1],
        series.iter().min().unwrap(),
        series.iter().max().unwrap()
    );
    assert_flat(&series, 1, 0);
}

#[test]
fn m3_numbers_of_the_fixture_documents() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let dir = common::manifest().join("tests/captures/f7d/docs");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let bytes = std::fs::read(e.path()).unwrap();
        drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let (l, u) = measure(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let chars: usize = l.pages.iter().map(|p| p.chars.o.len()).sum();
        eprintln!(
            "M3 {}: {} pages, {chars} chars, {} B allocated, {} B peak, {} B retained ({:.1} B/char)",
            e.file_name().to_string_lossy(),
            l.pages.len(),
            u.allocated,
            u.peak_above_start,
            u.live_delta,
            u.live_delta as f64 / chars.max(1) as f64
        );
    }
}

#[test]
fn limits_end_the_call() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    // a very long chain (2,000 linked boxes): laid out under the defaults, stopped by the work
    // budget when it is small
    let content: String = (0..40).map(|k| para("", &run(50, k))).collect();
    let mut b = String::new();
    b.push_str(&para("", &floating(1, (100, 100), (1500, 300), Ok(("1", &content)))));
    let n = if cfg!(debug_assertions) { 300 } else { 2_000 };
    let links: String = (0..n)
        .map(|k| {
            floating(2 + k, (100 + i64::from(k % 7) * 1500, 500 + i64::from(k % 40) * 350), (1500, 300), Err(("1", &(k + 1).to_string())))
        })
        .collect();
    b.push_str(&para("", &links));
    let long = docx(&b, None, None, "");
    check_indices(&layout_bytes(&long, &fonts, &reg, &opts()).unwrap());
    let work = LayoutOptions { limits: LayoutLimits { work_floor: 2_000, work_per_glyph: 0, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&long, &fonts, &reg, &work).unwrap_err(), LayoutError::Limit(LimitKind::Work));
    // chains nested in each other's content past `max_depth` (each first box's content holds the
    // next chain's first box inline)
    let mut inner = para("", &run(20, 1));
    for d in 0..12u32 {
        let id = (100 + d).to_string();
        inner = para("", &format!("{}{}", inline(200 + d, (2000, 600), Ok((&id, &inner))), inline(300 + d, (2000, 600), Err((&id, "1")))));
    }
    let deep = docx(&inner, None, None, "");
    let shallow = LayoutOptions { limits: LayoutLimits { max_depth: 8, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&deep, &fonts, &reg, &shallow).unwrap_err(), LayoutError::Limit(LimitKind::Depth));
    check_indices(&layout_bytes(&deep, &fonts, &reg, &opts()).unwrap());
}

/// A small deterministic generator (no dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() as usize) % xs.len()]
    }
}

#[test]
fn no_panic_on_hostile_chains_and_endnote_tables() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let ids = ["1", "1", "2", "0", "-1", "NaN", "abc", "", "1e9", "2.5", "99999999999"];
    let seqs = ["1", "2", "3", "1", "0", "-1", "NaN", "", "1e9", "2.5"];
    let sizes = [0u32, 1, 30, 300, 1500, 4000, 40_000];
    let mut rng = Lcg(17);
    let (mut ok, mut errs) = (0, Vec::new());
    let o = LayoutOptions { limits: LayoutLimits { max_pages: 300, ..LayoutLimits::default() }, ..opts() };
    let cases = if cfg!(debug_assertions) { 30 } else { 200 };
    for case in 0..cases {
        let mut body = String::new();
        let mut notes = String::new();
        let mut doc_pr = 1u32;
        let n = 1 + rng.next() % 6;
        for i in 0..n as usize {
            let mut parts = vec![run((rng.next() % 50) as usize, i)];
            for _ in 0..1 + rng.next() % 4 {
                doc_pr += 1;
                let size = (sizes[(rng.next() % 7) as usize], sizes[(rng.next() % 7) as usize]);
                let at = ((rng.next() % 9000) as i64 - 500, (rng.next() % 15000) as i64 - 500);
                let id = rng.pick(&ids);
                let head = rng.next().is_multiple_of(3);
                let content = if head {
                    // the content may hold boxes of its own chain or of another (reentrancy)
                    let mut c = para(
                        rng.pick(&[
                            "",
                            "<w:keepNext/>",
                            r#"<w:ind w:firstLine="720"/>"#,
                            r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#,
                        ]),
                        &run((rng.next() % 90) as usize, i + 1),
                    );
                    if rng.next().is_multiple_of(3) {
                        c.push_str(&para("", &inline(900 + doc_pr, (1200, 400), Err((rng.pick(&ids), rng.pick(&seqs))))));
                    }
                    if rng.next().is_multiple_of(3) {
                        let rows: Vec<String> = (0..1 + rng.next() % 10)
                            .map(|r| format!("<w:tr>{}</w:tr>", cell(1000, &para("", &run(4, r as usize)))))
                            .collect();
                        c.push_str(&table(&[1000], &rows));
                        c.push_str("<w:p/>");
                    }
                    c
                } else {
                    String::new()
                };
                let story = if head { Ok((id, content.as_str())) } else { Err((id, rng.pick(&seqs))) };
                parts.push(match rng.next() % 4 {
                    0 => inline(doc_pr, size, story),
                    1 => grouped(doc_pr, at, size, story),
                    _ => floating(doc_pr, at, size, story),
                });
            }
            if rng.next().is_multiple_of(3) {
                let nid = 2 + i;
                parts.push(endnote_ref(nid as u32));
                let rows: Vec<String> = (0..1 + rng.next() % 40)
                    .map(|r| {
                        format!(
                            r#"<w:tr><w:trPr>{}</w:trPr>{}</w:tr>"#,
                            rng.pick(&[
                                "",
                                "<w:cantSplit/>",
                                "<w:tblHeader/>",
                                r#"<w:trHeight w:val="1e9"/>"#,
                                r#"<w:trHeight w:val="NaN" w:hRule="exact"/>"#
                            ]),
                            cell(2000, &para("", &run((rng.next() % 30) as usize, r as usize)))
                        )
                    })
                    .collect();
                notes.push_str(&format!(
                    r#"<w:endnote w:id="{nid}">{}{}{}</w:endnote>"#,
                    para(r#"<w:pStyle w:val="ET"/><w:keepNext/>"#, &run(5, i)),
                    table(&[2000], &rows),
                    rng.pick(&["<w:p/>", "", r#"<w:p><w:pPr><w:keepNext/></w:pPr></w:p>"#])
                ));
            }
            body.push_str(&para("", &parts.concat()));
        }
        let hdr = rng.next().is_multiple_of(5).then(|| {
            para(
                "",
                &format!(
                    "{}{}",
                    floating(800, (100, 100), (2000, 300), Ok(("1", &para("", &run(30, 1))))),
                    floating(801, (3000, 100), (2000, 300), Err(("1", rng.pick(&seqs))))
                ),
            )
        });
        let bytes = docx(&body, hdr.as_deref(), (!notes.is_empty()).then_some(notes.as_str()), "");
        match layout_bytes(&bytes, &fonts, &reg, &o) {
            Ok(l) => {
                check_indices(&l);
                ok += 1;
            }
            Err(e) => errs.push((case, e)),
        }
    }
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (_, e) in &errs {
        *kinds.entry(format!("{e:?}")).or_default() += 1;
    }
    eprintln!("no-panic: {ok} laid out, {} errors: {kinds:?}", errs.len());
    for (_, e) in &errs {
        assert!(matches!(e, LayoutError::Limit(_)), "{e:?}");
    }
}
