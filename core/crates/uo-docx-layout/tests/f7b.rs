//! F7b (engine: tables and floats): memory (M1 per call, M2 soak open → layout → drop, M3
//! numbers), the limits and the no-panic loop over hostile tables, floats, frames and linked
//! textboxes, all through the real path (uo-docx reader → `derive::build` →
//! `engine::layout_document`) with the repository's open fonts.

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
const NS: &str = r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape""#;

/// A 2 × 2 RGB PNG (generated here: no third-party data).
fn png() -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, ty: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]);
    // raw rows (filter 0) stored in a zlib stream without compression
    let raw: Vec<u8> = [0u8, 255, 0, 0, 0, 255, 0, 0u8, 0, 0, 255, 255, 255, 255].to_vec();
    let mut z = vec![0x78, 0x01, 1];
    z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    z.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

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

/// A docx with `body`, a default table style, `settings` and one image part (`rImg`).
fn docx(body: &str, settings: &str) -> Vec<u8> {
    let sect = r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/></w:sectPr>"#;
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" {NS}><w:body>{body}{sect}</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="table" w:default="1" w:styleId="TN"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>"#
    );
    let settings = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{settings}</w:settings>"#);
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in [
        ("[Content_Types].xml", ct.as_bytes().to_vec()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("word/document.xml", doc.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes().to_vec()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/settings.xml", settings.into_bytes()),
        ("word/media/image1.png", png()),
    ] {
        z.add(name, &data).unwrap();
    }
    z.finish()
}

fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "tabelas", "celulas", "informação", "flutuante", "página", "responsabilidade"];
    (0..n).map(|i| WS[(i * 7 + seed * 3) % WS.len()]).collect::<Vec<_>>().join(" ")
}

fn run(n: usize, seed: usize) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{} </w:t></w:r>"#, words(n, seed))
}

fn para(ppr: &str, inner: &str) -> String {
    format!(r#"<w:p><w:pPr>{ppr}</w:pPr>{inner}</w:p>"#)
}

/// An anchored drawing: `kind` = `pic` (the image), `shape` or `tb` (a textbox with `content`).
#[allow(clippy::too_many_arguments, reason = "the anchor's attributes")]
fn anchor(
    id: u32,
    kind: &str,
    wrap: &str,
    h_rel: &str,
    v_rel: &str,
    off: (i64, i64),
    size: (u32, u32),
    behind: bool,
    content: &str,
) -> String {
    let (cx, cy) = (u64::from(size.0) * 635, u64::from(size.1) * 635);
    let graphic = match kind {
        "pic" => format!(
            r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="p"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rImg"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData>"#
        ),
        _ => format!(
            r#"<a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvSpPr{}/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></wps:spPr>{}</wps:wsp></a:graphicData>"#,
            if kind == "tb" { r#" txBox="1""# } else { "" },
            if kind == "tb" {
                format!(
                    r#"<wps:txbx><w:txbxContent>{content}</w:txbxContent></wps:txbx><wps:bodyPr anchor="ctr"><a:spAutoFit/></wps:bodyPr>"#
                )
            } else {
                "<wps:bodyPr/>".to_owned()
            }
        ),
    };
    format!(
        r#"<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="{}" behindDoc="{}" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="{h_rel}"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="{v_rel}"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>{wrap}<wp:docPr id="{id}" name="d{id}"/><wp:cNvGraphicFramePr/><a:graphic>{graphic}</a:graphic></wp:anchor></w:drawing></w:r>"#,
        251_658_240 + id,
        u8::from(behind),
        off.0 * 635,
        off.1 * 635
    )
}

fn inline_pic(id: u32) -> String {
    format!(
        r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="457200" cy="304800"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="{id}" name="i{id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="i"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rImg"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="457200" cy="304800"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

fn cell(w: u32, tcpr: &str, content: &str) -> String {
    format!(r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/>{tcpr}</w:tcPr>{content}</w:tc>"#)
}

fn table(tblpr: &str, grid: &[u32], rows: &[String]) -> String {
    let g: String = grid.iter().map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect();
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="double" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="dotted" w:sz="4" w:color="auto"/></w:tblBorders>{tblpr}</w:tblPr><w:tblGrid>{g}</w:tblGrid>{}</w:tbl>"#,
        rows.join("")
    )
}

/// A document of every F7b path: tables (header rows repeated over a page split, vertical merges
/// with fills crossing a page, nested tables, an exact-height row, a cantSplit row, keepNext rows),
/// a floating table split over two pages, a keepNext heading before a table (the preview), floats
/// (square both sides and largest, topAndBottom, behind, a page-relative late float, a picture,
/// an auto-fit textbox, a float in a cell), an inline picture, and a frame.
fn every_path() -> Vec<u8> {
    let mut b = String::new();
    b.push_str(&para("<w:keepNext/>", &run(6, 1)));
    let mut rows = vec![format!(
        r#"<w:tr><w:trPr><w:tblHeader/></w:trPr>{}{}</w:tr>"#,
        cell(3000, "", &para("", &run(3, 2))),
        cell(5000, r#"<w:shd w:val="clear" w:fill="D9E2F3"/>"#, &para("", &run(3, 3)))
    )];
    for r in 0..14 {
        let merge = match r % 5 {
            0 => r#"<w:vMerge w:val="restart"/><w:shd w:val="clear" w:fill="FFF2CC"/>"#,
            1..=3 => "<w:vMerge/>",
            _ => "",
        };
        let first = if r % 5 == 0 || r % 5 == 4 { para("", &run(8, r)) } else { "<w:p/>".to_owned() };
        let second = if r == 6 {
            format!(
                "{}<w:p/>",
                table(
                    "",
                    &[2000, 2000],
                    &[format!("<w:tr>{}{}</w:tr>", cell(2000, "", &para("", &run(9, 4))), cell(2000, "", &para("", &run(4, 5))))]
                )
            )
        } else if r == 8 {
            para(
                "",
                &format!(
                    "{}{}",
                    run(10, r),
                    anchor(
                        40 + r as u32,
                        "shape",
                        r#"<wp:wrapSquare wrapText="bothSides"/>"#,
                        "column",
                        "paragraph",
                        (100, 0),
                        (900, 500),
                        false,
                        ""
                    )
                ),
            )
        } else {
            para("", &run(20 + r * 13 % 60, r))
        };
        let trpr = match r {
            3 => r#"<w:trPr><w:trHeight w:val="900" w:hRule="exact"/></w:trPr>"#,
            5 => "<w:trPr><w:cantSplit/></w:trPr>",
            _ => "",
        };
        rows.push(format!("<w:tr>{trpr}{}{}</w:tr>", cell(3000, merge, &first), cell(5000, "", &second)));
    }
    b.push_str(&table("", &[3000, 5000], &rows));
    b.push_str(&para("", &format!("{}{}", run(30, 7), inline_pic(2))));
    // floats in the flow
    b.push_str(&para(
        "",
        &format!(
            "{}{}",
            anchor(3, "pic", r#"<wp:wrapSquare wrapText="bothSides"/>"#, "column", "paragraph", (0, 0), (2000, 1500), false, ""),
            run(90, 8)
        ),
    ));
    b.push_str(&para(
        "",
        &format!(
            "{}{}",
            anchor(4, "shape", r#"<wp:wrapSquare wrapText="largest"/>"#, "margin", "paragraph", (2500, 200), (2000, 900), false, ""),
            run(70, 9)
        ),
    ));
    b.push_str(&para(
        "",
        &format!(
            "{}{}",
            anchor(5, "shape", "<wp:wrapTopAndBottom/>", "page", "paragraph", (3000, 100), (3000, 600), false, ""),
            run(40, 10)
        ),
    ));
    b.push_str(&para(
        "",
        &format!("{}{}", anchor(6, "shape", "<wp:wrapNone/>", "page", "page", (100, 100), (1000, 1000), true, ""), run(20, 11)),
    ));
    let tb_content = format!("{}{}", para("", &run(12, 12)), para("", &run(6, 13)));
    b.push_str(&para(
        "",
        &format!(
            "{}{}",
            anchor(7, "tb", r#"<wp:wrapSquare wrapText="bothSides"/>"#, "column", "paragraph", (5000, 0), (2600, 800), false, &tb_content),
            run(80, 14)
        ),
    ));
    // a page-relative float covering lines already laid out: a late float of the next pass
    for k in 0..6 {
        b.push_str(&para("", &run(30, 20 + k)));
    }
    b.push_str(&para(
        "",
        &format!(
            "{}{}",
            run(10, 30),
            anchor(8, "shape", r#"<wp:wrapSquare wrapText="bothSides"/>"#, "margin", "margin", (0, 400), (3000, 1200), false, "")
        ),
    ));
    // a frame and a floating table that splits over the page foot
    b.push_str(&para(
        r#"<w:framePr w:w="2600" w:hSpace="180" w:wrap="around" w:vAnchor="text" w:hAnchor="margin" w:xAlign="right" w:y="1"/>"#,
        &run(15, 31),
    ));
    b.push_str(&para("", &run(60, 32)));
    let frows: Vec<String> = (0..30).map(|r| format!("<w:tr>{}</w:tr>", cell(2400, "", &para("", &run(6, r))))).collect();
    b.push_str(&table(r#"<w:tblpPr w:leftFromText="180" w:rightFromText="180" w:vertAnchor="text" w:horzAnchor="margin" w:tblpXSpec="right" w:tblpY="120"/>"#, &[2400], &frows));
    for k in 0..8 {
        b.push_str(&para("", &run(50, 40 + k)));
    }
    docx(
        &b,
        r#"<w:defaultTabStop w:val="708"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>"#,
    )
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
/// content → content lines, images → resources).
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
        for it in &p.page.list.items {
            if let uo_render::types::Item::Image(im) = it {
                assert!(l.resources.image(im.image).is_some());
            }
        }
        assert!(p.hf_items.is_some_and(|h| h as usize <= p.page.list.items.len()));
    }
    assert_eq!(l.image_origins.len(), l.resources.images.len());
}

#[test]
fn the_every_path_document_lays_out() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let l = layout_bytes(&every_path(), &fonts, &reg, &opts()).unwrap();
    assert!(l.pages.len() >= 3, "{} pages", l.pages.len());
    check_indices(&l);
    // the floats and the textbox's content lines are there, and the cells' boxes
    assert!(l.pages.iter().any(|p| p.objects.as_ref().is_some_and(|o| o.len() >= 2)));
    assert!(l.pages.iter().any(|p| !p.content_lines.is_empty()));
    assert!(l.pages.iter().any(|p| p.cells.as_ref().is_some_and(|c| !c.is_empty())));
    assert!(l.pages.iter().any(|p| p.page.list.items.iter().any(|i| matches!(i, uo_render::types::Item::Image(_)))));
}

#[test]
fn m1_a_layout_call_returns_every_byte() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    let u = assert_no_leak(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    eprintln!("M1 every-path document: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    // a call that ends in a limit mid-pass (inside a table row split, say) returns every byte too
    let tight = LayoutOptions { limits: LayoutLimits { max_pages: 2, ..LayoutLimits::default() }, ..opts() };
    assert_no_leak(|| assert_eq!(layout_bytes(&bytes, &fonts, &reg, &tight).unwrap_err(), LayoutError::Limit(LimitKind::Pages)));
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
    let dir = common::manifest().join("tests/captures/f7b/docs");
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
fn m5_the_image_table_holds_one_id_per_model_picture() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    // the same picture inline on 400 lines of a table: one image object per drawing, not per page
    // or per pass (RQ3, F7b-1)
    let rows: Vec<String> = (0..200)
        .map(|r| format!("<w:tr>{}</w:tr>", cell(4000, "", &para("", &format!("{}{}", run(4, r), inline_pic(100 + r as u32))))))
        .collect();
    let bytes = docx(&table("", &[4000], &rows), "");
    let l = layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap();
    check_indices(&l);
    assert!(l.resources.images.len() >= 200, "{}", l.resources.images.len());
    // two passes at most share every id: no more than one image per drawing and pass
    assert!(l.resources.images.len() <= 200 * 5, "{}", l.resources.images.len());
}

#[test]
fn limits_end_the_call() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    let pages = LayoutOptions { limits: LayoutLimits { max_pages: 1, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&bytes, &fonts, &reg, &pages).unwrap_err(), LayoutError::Limit(LimitKind::Pages));
    let work = LayoutOptions { limits: LayoutLimits { work_floor: 1_000, work_per_glyph: 0, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&bytes, &fonts, &reg, &work).unwrap_err(), LayoutError::Limit(LimitKind::Work));
    // nested tables past `max_depth` (the TS recurses until V8's stack throws; derive and the cell
    // flow both count the levels)
    let mut t = para("", &run(2, 1));
    for _ in 0..12 {
        t = format!("{}<w:p/>", table("", &[3000], &[format!("<w:tr>{}</w:tr>", cell(3000, "", &t))]));
    }
    let deep = docx(&t, "");
    let shallow = LayoutOptions { limits: LayoutLimits { max_depth: 8, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&deep, &fonts, &reg, &shallow).unwrap_err(), LayoutError::Limit(LimitKind::Depth));
    // (under the default bound of 192 the same nesting lays out)
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
fn no_panic_on_hostile_tables_floats_frames_and_chains() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let nums = ["0", "-1", "1", "7", "720", "-720", "1e9", "99999999", "NaN", "abc", "", "-0", "4097", "-99999999"];
    let wraps = [
        r#"<wp:wrapSquare wrapText="bothSides"/>"#,
        r#"<wp:wrapSquare wrapText="largest"/>"#,
        "<wp:wrapTopAndBottom/>",
        "<wp:wrapNone/>",
        r#"<wp:wrapTight wrapText="bothSides"><wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="10800" y="21600"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon></wp:wrapTight>"#,
        r#"<wp:wrapThrough wrapText="bothSides"><wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="0" y="21600"/></wp:wrapPolygon></wp:wrapThrough>"#,
    ];
    let rels = ["column", "margin", "page", "paragraph", "line", "character", "leftMargin", "insideMargin", "x"];
    let mut rng = Lcg(11);
    let (mut ok, mut errs) = (0, Vec::new());
    let o = LayoutOptions { limits: LayoutLimits { max_pages: 600, ..LayoutLimits::default() }, ..opts() };
    let cases = if cfg!(debug_assertions) { 40 } else { 200 };
    for case in 0..cases {
        let mut body = String::new();
        let n = 1 + rng.next() % 6;
        for i in 0..n {
            match rng.next() % 4 {
                0 => {
                    let k = (rng.next() % 3) as u32;
                    let off = uo_jsnum::math::max(
                        -1e7,
                        uo_jsnum::math::min(1e7, {
                            let v = uo_jsnum::parse::to_number(rng.pick(&nums));
                            if v.is_nan() { 0.0 } else { v }
                        }),
                    ) as i64;
                    let sz = uo_jsnum::math::max(
                        0.0,
                        uo_jsnum::math::min(1e7, {
                            let v = uo_jsnum::parse::to_number(rng.pick(&nums));
                            if v.is_nan() { 0.0 } else { v }
                        }),
                    ) as u32;
                    let kind = ["pic", "shape", "tb"][k as usize];
                    let content = para("", &run(5, i as usize));
                    let a = anchor(
                        10 + i as u32,
                        kind,
                        rng.pick(&wraps),
                        rng.pick(&rels),
                        rng.pick(&rels),
                        (off, -off),
                        (sz, sz / 2 + 1),
                        rng.next().is_multiple_of(3),
                        &content,
                    );
                    body.push_str(&para("", &format!("{a}{}", run((rng.next() % 60) as usize, i as usize))));
                }
                1 => {
                    let rows: Vec<String> = (0..1 + rng.next() % 5)
                        .map(|r| {
                            format!(
                                r#"<w:tr><w:trPr><w:trHeight w:val="{}" w:hRule="{}"/>{}</w:trPr>{}{}</w:tr>"#,
                                rng.pick(&nums),
                                rng.pick(&["exact", "atLeast", "auto"]),
                                rng.pick(&["", "<w:cantSplit/>", "<w:tblHeader/>"]),
                                cell(
                                    1000,
                                    &format!(
                                        r#"<w:gridSpan w:val="{}"/>{}"#,
                                        rng.pick(&["1", "2", "0", "-1"]),
                                        rng.pick(&["", "<w:vMerge/>", r#"<w:vMerge w:val="restart"/>"#])
                                    ),
                                    &para("", &run((rng.next() % 40) as usize, r as usize))
                                ),
                                cell(
                                    2000,
                                    rng.pick(&["", r#"<w:textDirection w:val="btLr"/>"#, r#"<w:vAlign w:val="bottom"/>"#]),
                                    &para("", &run((rng.next() % 30) as usize, r as usize + 1))
                                )
                            )
                        })
                        .collect();
                    let tp = rng.pick(&[
                        "",
                        r#"<w:tblpPr w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="NaN" w:tblpY="-720"/>"#,
                        r#"<w:tblpPr w:vertAnchor="page" w:horzAnchor="page" w:tblpXSpec="center" w:tblpYSpec="bottom"/>"#,
                    ]);
                    body.push_str(&table(tp, &[1000, 2000], &rows));
                }
                2 => {
                    let f = format!(
                        r#"<w:framePr w:w="{}" w:h="{}" w:hRule="{}" w:wrap="{}" w:vAnchor="{}" w:hAnchor="{}" w:x="{}" w:y="{}"/>"#,
                        rng.pick(&nums),
                        rng.pick(&nums),
                        rng.pick(&["exact", "atLeast", "auto"]),
                        rng.pick(&["around", "notBeside", "none", "auto"]),
                        rng.pick(&["text", "margin", "page"]),
                        rng.pick(&["text", "margin", "page"]),
                        rng.pick(&nums),
                        rng.pick(&nums)
                    );
                    body.push_str(&para(&f, &run(8, i as usize)));
                }
                _ => {
                    // a break with w:clear, and a linked textbox chain (laid out since F7d; the
                    // hostile chains are `tests/f7d.rs`'s)
                    body.push_str(&para(
                        "",
                        &format!(
                            r#"{}<w:r><w:br w:type="{}" w:clear="{}"/></w:r>{}"#,
                            run(10, i as usize),
                            rng.pick(&["textWrapping", "page"]),
                            rng.pick(&["all", "left", "none"]),
                            run(10, i as usize + 1)
                        ),
                    ));
                    if rng.next().is_multiple_of(2) {
                        let head = anchor(
                            60 + i as u32,
                            "tb",
                            "<wp:wrapNone/>",
                            "column",
                            "paragraph",
                            (0, 0),
                            (2000, 600),
                            false,
                            &para("", &run(40, i as usize)),
                        )
                        .replace("<wps:txbx>", r#"<wps:txbx id="7">"#);
                        let link = anchor(70 + i as u32, "tb", "<wp:wrapNone/>", "column", "paragraph", (2500, 0), (2000, 600), false, "")
                            .replace("<wps:txbx><w:txbxContent></w:txbxContent></wps:txbx>", r#"<wps:linkedTxbx id="7" seq="1"/>"#);
                        body.push_str(&para("", &format!("{head}{link}{}", run(5, i as usize))));
                    }
                }
            }
        }
        let bytes = docx(&body, rng.pick(&["", r#"<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="14"/><w:doNotBreakWrappedTables/></w:compat>"#]));
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
        assert!(matches!(e, LayoutError::Limit(_) | LayoutError::Internal(_) | LayoutError::Unported(..)), "{e:?}");
    }
}
