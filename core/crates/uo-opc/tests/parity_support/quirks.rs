//! The generated quirk corpus of the package parity (stage 4): archives that exercise what the
//! real Word, Excel and PowerPoint files never do — names without the UTF-8 flag (and with it
//! on invalid UTF-8), duplicates, integer-like and `__proto__` names, lying sizes, backslashes,
//! case differences, UTF-16 and BOM-prefixed parts, percent-encoded and odd relationship
//! targets, corrupt deflate streams, broken central directories — plus seeded random
//! combinations of them. Every archive holds the main parts of the three apps, so each app's
//! load, XML, relationship and save paths run on it.
//!
//! The same function feeds the capture (through the `opc_quirk_corpus` example, which writes the
//! files the Node side reads) and the replay (in memory); the capture keeps the digest of every
//! archive, so a change here is caught as "recapture needed", never as a silent mismatch.
#![allow(dead_code)]

use uo_opc::flate::deflate_raw;
use uo_testkit::generate::{RawZipEntry, RawZipOptions, Rng, build_zip};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const PML: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A part to write: name bytes, UTF-8 flag, content, stored or deflated.
#[derive(Debug, Clone)]
pub struct QPart {
    pub name: Vec<u8>,
    pub flags: u16,
    pub data: Vec<u8>,
    pub stored: bool,
}

impl QPart {
    fn new(name: &str, data: impl Into<Vec<u8>>) -> Self {
        let flags = if name.is_ascii() { 0 } else { 0x800 };
        QPart { name: name.as_bytes().to_vec(), flags, data: data.into(), stored: false }
    }
    fn raw(name: &[u8], flags: u16, data: impl Into<Vec<u8>>) -> Self {
        QPart { name: name.to_vec(), flags, data: data.into(), stored: false }
    }
}

fn entry(p: &QPart) -> RawZipEntry {
    let e = if p.stored {
        RawZipEntry::stored(p.name.clone(), p.data.clone())
    } else {
        RawZipEntry::deflated(p.name.clone(), deflate_raw(&p.data, 6), p.data.len() as u32)
    };
    e.flags(p.flags)
}

fn zip(parts: &[QPart]) -> Vec<u8> {
    build_zip(&parts.iter().map(entry).collect::<Vec<_>>(), &RawZipOptions::default())
}

fn rel(id: &str, ty: &str, target: &str) -> String {
    format!("<Relationship Id=\"{id}\" Type=\"{REL}/{ty}\" Target=\"{target}\"/>")
}

fn rels(body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"{RELS}\">{body}</Relationships>")
}

/// The odd targets every document relationship part carries (each app resolves them its way).
fn odd_targets() -> String {
    [
        rel("rId1", "styles", "styles.xml"),
        rel("rId2", "image", "media/image1.png"),
        rel("rId3", "image", "./media/image1.png"),
        rel("rId4", "image", "/word/media/image1.png"),
        rel("rId5", "customXml", "../customXml/item1.xml"),
        rel("rId6", "image", "media/image%201.png"),
        rel("rId7", "image", "media\\image2.png"),
        rel("rId8", "image", "media/IMAGE1.PNG"),
        rel("rId9", "image", "../../outside.png"),
        rel("rId10", "image", "media/image1.png#frag"),
        rel("rId11", "image", "media/image1.png?q=1"),
        rel("rId12", "image", "%2e%2e/customXml/item1.xml"),
        rel("rId13", "image", "media/%FF.png"),
        rel("rId14", "image", "media/imag\u{e9}.png"),
        rel("rId15", "image", "media/imag%C3%A9.png"),
        rel("rId16", "image", "worksheets/sheet1.xml"),
        rel("rId17", "image", "slides/slide1.xml"),
        rel("rId18", "image", "../word/media/image1.png"),
        rel("rId2", "image", "media/dup.png"),
        format!("<Relationship Id=\"rId19\" Type=\"{REL}/hyperlink\" Target=\"http://example.com/a b\" TargetMode=\"External\"/>"),
        format!("<Relationship Id=\"rId20\" Type=\"{REL}/image\"/>"),
        format!("<Relationship Type=\"{REL}/image\" Target=\"media/image1.png\"/>"),
        format!("<Relationship Id=\"\" Type=\"{REL}/image\" Target=\"media/image1.png\"/>"),
        format!("<Relationship Id=\"rId21\" Type=\"{REL}/image\" Target=\"\"/>"),
        rel("rId22", "image", "&#x6d;edia/image1.png"),
        rel("rId23", "image", "/"),
        rel("rId24", "image", "."),
    ]
    .concat()
}

/// The base package: the main parts of Write, Sheet and Slides plus media and odd relationships.
pub fn base_parts() -> Vec<QPart> {
    let ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>",
    );
    let root_rels = rels(
        &[
            rel("rId1", "officeDocument", "word/document.xml"),
            rel("rId2", "officeDocument", "/xl/workbook.xml"),
            rel("rId3", "officeDocument", "ppt/presentation.xml"),
            rel("rId4", "image", "word/media/image1.png"),
        ]
        .concat(),
    );
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:ve=\"{MC_NS}\">\
         <w:body><w:p><w:r><w:t xml:space=\"preserve\"> Ol\u{e1} \u{1F600} &amp; &#xD83D;&#xDE00; &lt;x&gt; </w:t></w:r></w:p>\
         <ve:AlternateContent><ve:Choice Requires=\"w14\"><w:p/></ve:Choice><ve:Fallback><w:p/></ve:Fallback></ve:AlternateContent>\
         <w:p><w:r><w:drawing><wp:inline xmlns:wp=\"x\"><a:graphic xmlns:a=\"{A_NS}\">\u{e9}</a:graphic></wp:inline></w:drawing></w:r></w:p>\r\n\
         <w:p w:rsidR=\"00A1\" 2=\"two\" 1=\"one\"><w:r><w:t>a\r\nb&#13;</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"
    );
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<x:workbook xmlns:x=\"{SML}\" xmlns:r=\"{R_NS}\">\
         <x:sheets><x:sheet name=\"S&#x110000;1\" sheetId=\"1\" r:id=\"rId1\"/></x:sheets></x:workbook>"
    );
    let wb_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><pr:Relationships xmlns:pr=\"{RELS}\">\
         <pr:Relationship Id=\"rId1\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
         <pr:Relationship Id=\"rId2\" Type=\"{REL}/styles\" Target=\"/xl/Styles.xml\"/>\
         <pr:Relationship Id=\"rId3\" Type=\"{REL}/chart\" Target=\"charts/chart1.xml\"/></pr:Relationships>"
    );
    let sheet1 = format!(
        "<worksheet xmlns=\"{SML}\"><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>x\r\ny\rz</t></is></c></row></sheetData>\
         <drawing r:id=\"rId1\" xmlns:r=\"{R_NS}\"/></worksheet>"
    );
    let chart = format!(
        "<chartSpace xmlns=\"{CHART}\" xmlns:a=\"{A_NS}\"><chart><plotArea a:x=\"1\" y=\"2\"/></chart><c:extra xmlns:c=\"{CHART}\"/></chartSpace>"
    );
    let drawing = format!(
        "<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"{A_NS}\"><a:t>d</a:t></xdr:wsDr>"
    );
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:presentation xmlns:p=\"{PML}\" xmlns:r=\"{R_NS}\">\
         <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/></p:sldIdLst></p:presentation>"
    );
    let pres_rels = rels(
        &[rel("rId1", "slideMaster", "slideMasters/slideMaster1.xml"), rel("rId2", "slide", "slides/slide1.xml"), odd_targets()].concat(),
    );
    let slide = format!(
        "<p:sld xmlns:p=\"{PML}\" xmlns:a=\"{A_NS}\"><p:cSld><?pi in content?><a:t>&#x110000;&#xD800;\u{2028}</a:t></p:cSld></p:sld>"
    );
    let slide_rels =
        rels(&[rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), rel("rId2", "image", "../media/image1.png")].concat());
    let png: Vec<u8> = [&[0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..], &[7u8; 300][..]].concat();
    vec![
        QPart::new("[Content_Types].xml", ct),
        QPart::new("_rels/.rels", root_rels),
        QPart::new("word/document.xml", document),
        QPart::new("word/_rels/document.xml.rels", rels(&odd_targets())),
        QPart::new("word/styles.xml", format!("<w:styles xmlns:w=\"{W_NS}\"><w:style w:styleId=\"a\"/></w:styles>")),
        QPart::new("word/media/image1.png", png.clone()),
        QPart::new("word/media/image 1.png", png[..100].to_vec()),
        QPart::new("customXml/item1.xml", "<root>\u{FEFF}<!-- c --><v>1</v></root>"),
        QPart::new("xl/workbook.xml", workbook),
        QPart::new("xl/_rels/workbook.xml.rels", wb_rels),
        QPart::new("xl/worksheets/Sheet1.xml", sheet1),
        QPart::new("xl/worksheets/_rels/sheet1.xml.rels", rels(&rel("rId1", "drawing", "../drawings/drawing1.xml"))),
        QPart::new("xl/styles.xml", format!("<styleSheet xmlns=\"{SML}\"/>")),
        QPart::new("xl/charts/chart1.xml", chart),
        QPart::new("xl/drawings/drawing1.xml", drawing),
        QPart::new("ppt/presentation.xml", presentation),
        QPart::new("ppt/_rels/presentation.xml.rels", pres_rels),
        QPart::new("ppt/slides/slide1.xml", slide),
        QPart::new("ppt/slides/_rels/slide1.xml.rels", slide_rels),
        QPart::new("ppt/media/image1.png", png),
    ]
}

fn find(parts: &mut [QPart], name: &str) -> Option<usize> {
    parts.iter().position(|p| p.name == name.as_bytes())
}

fn utf16(s: &str, le: bool) -> Vec<u8> {
    let mut out = if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
    for u in s.encode_utf16() {
        out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
    }
    out
}

/// Tokens inserted by the random XML mutations: none is a `Object.prototype` entity name (G5)
/// and none nests deeply (the slides parser recurses).
const XML_TOKENS: &[&str] = &[
    "\r\n",
    "\r",
    "&#x41;",
    "&#1a;",
    "&#xD83D;",
    "&#xDE00;",
    "&bogus;",
    "&amp;",
    "<?pi x?>",
    "<!--c-->",
    "<![CDATA[c\r\nd]]>",
    "\u{FEFF}",
    "\u{3000}",
    "<",
    ">",
    "&",
    "\"",
    "'",
    "<!--",
    "]]>",
    "<![CDATA[",
    "</",
    "<x/>",
    " a=\"1\"",
    "&#x110000;",
    "\t",
    "é",
    "😀",
];

/// The corpus: `(label, archive)` in a fixed order.
pub fn quirk_corpus() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut push = |label: &str, bytes: Vec<u8>| {
        let n = out.len();
        out.push((format!("{n:03}-{label}"), bytes));
    };
    let base = base_parts();
    push("base", zip(&base));
    let mut stored = base.clone();
    stored.iter_mut().for_each(|p| p.stored = true);
    push("base-stored", zip(&stored));

    // names without the UTF-8 flag (UTF-8 bytes read as Latin-1), with it on invalid UTF-8 and
    // with a BOM; the relationships target both spellings
    let mut p = base.clone();
    p.push(QPart::raw("word/media/imagé.png".as_bytes(), 0, b"latin1".to_vec()));
    p.push(QPart::raw(b"word/media/\xffbad.png", 0x800, b"bad utf8".to_vec()));
    p.push(QPart::raw(b"\xef\xbb\xbfword/media/bom.png", 0x800, b"bom".to_vec()));
    p.push(QPart::raw(b"word/media/imag\xe9.png", 0, b"real latin1".to_vec()));
    push("names-flag", zip(&p));

    // duplicates: the first position keeps the last bytes
    let mut p = base.clone();
    p.push(QPart::new("word/document.xml", format!("<w:document xmlns:w=\"{W_NS}\"><w:body><w:p/></w:body></w:document>")));
    p.push(QPart::new("word/_rels/document.xml.rels", rels(&rel("rId2", "image", "media/image 1.png"))));
    p.push(QPart::new("ppt/media/image1.png", b"dup".to_vec()));
    push("duplicates", zip(&p));

    // integer-like names come first in JS object order; __proto__ is dropped
    let mut p = base.clone();
    for n in ["10", "2", "01", "1", "4294967294", "4294967295", "-1", "1.5", "__proto__"] {
        p.push(QPart::new(n, n.as_bytes().to_vec()));
    }
    push("js-object-names", zip(&p));
    // after a `__proto__` entry the fflate result object has a Uint8Array as prototype, whose
    // [[Set]] silently drops canonical numeric names that are not valid indices of that array
    let mut p = base.clone();
    for n in ["1", "__proto__", "-1", "1.5", "0", "8", "1", "word/after.xml", "1e3", "-0"] {
        p.push(QPart::new(n, n.as_bytes().to_vec()));
    }
    push("proto-then-numeric-names", zip(&p));

    // lying sizes
    let mut entries: Vec<RawZipEntry> = base.iter().map(entry).collect();
    let body = b"abcdefghij".repeat(50);
    entries.push(RawZipEntry::deflated("word/media/short.bin", deflate_raw(&body, 6), 7));
    entries.push(RawZipEntry::deflated("word/media/long.bin", deflate_raw(&body, 6), 4000));
    entries.push(RawZipEntry::stored("word/media/stored-su.bin", body.clone()).size(3));
    entries.push(RawZipEntry::deflated("word/media/empty-declared.bin", Vec::new(), 64));
    entries.push(RawZipEntry::stored("word/media/huge-declared.bin", b"tiny".to_vec()).size(10 << 20));
    entries.push(RawZipEntry::stored("word/media/beyond.bin", b"end".to_vec()).csize(5000));
    push("size-lies", build_zip(&entries, &RawZipOptions::default()));

    // backslashes and case
    let mut p = base.clone();
    p.push(QPart::new("word\\media\\image2.png", b"backslash".to_vec()));
    p.push(QPart::new("word/media/image2.png", b"slash".to_vec()));
    p.push(QPart::new("Word/Media/Upper.png", b"upper".to_vec()));
    push("backslash-case", zip(&p));
    let mut p = base.clone();
    if let Some(i) = find(&mut p, "xl/workbook.xml") {
        p[i].name = b"XL/Workbook.xml".to_vec();
    }
    push("sheet-main-case", zip(&p));

    // UTF-16 and BOM-prefixed parts (TextDecoder is UTF-8: the TS reads them as garbage)
    let mut p = base.clone();
    for (name, le) in [("word/document.xml", true), ("xl/workbook.xml", false)] {
        if let Some(i) = find(&mut p, name) {
            let text = String::from_utf8_lossy(&p[i].data).into_owned();
            p[i].data = utf16(&text, le);
        }
    }
    if let Some(i) = find(&mut p, "ppt/presentation.xml") {
        p[i].data = [&[0xEF, 0xBB, 0xBF][..], &p[i].data].concat();
    }
    p.push(QPart::new("word/settings.xml", [&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF][..], b"<a/>"].concat()));
    p.push(QPart::new("word/bad-utf8.xml", b"<a v=\"\xff\xfe\xc3\">\xed\xa0\x80\xf4\x90\x80\x80</a>".to_vec()));
    push("utf16-bom", zip(&p));

    // percent-encoded part names next to the decoded ones
    let mut p = base.clone();
    p.push(QPart::new("word/media/image%201.png", b"percent".to_vec()));
    p.push(QPart::new("word/media/%FF.png", b"ff".to_vec()));
    p.push(QPart::new("word/media/imagé.png", b"utf8 flagged".to_vec()));
    push("percent-names", zip(&p));

    // the main parts missing, one at a time
    for main in ["word/document.xml", "xl/workbook.xml", "ppt/presentation.xml"] {
        let p: Vec<QPart> = base.iter().filter(|q| q.name != main.as_bytes()).cloned().collect();
        push(&format!("missing-{}", main.split('/').next().unwrap_or("")), zip(&p));
    }

    // archive-level quirks
    let good: Vec<RawZipEntry> = base.iter().map(entry).collect();
    let mut m12 = good.clone();
    m12.push(RawZipEntry { method: 12, ..RawZipEntry::stored("word/media/m12.bin", b"x".to_vec()) });
    push("method12", build_zip(&m12, &RawZipOptions::default()));
    push("comment", build_zip(&good, &RawZipOptions { comment: vec![b'c'; 300], ..RawZipOptions::default() }));
    let mut z64 = good.clone();
    z64.push(RawZipEntry {
        size: u32::MAX,
        csize: Some(u32::MAX),
        zip64_extra: Some(vec![8, 8]),
        ..RawZipEntry::stored("word/media/z64.bin", b"z64 data".to_vec())
    });
    push("zip64", build_zip(&z64, &RawZipOptions { zip64: true, ..RawZipOptions::default() }));
    let mut z64m = good.clone();
    z64m.push(RawZipEntry { size: u32::MAX, ..RawZipEntry::stored("word/media/z64.bin", b"z64 data".to_vec()) });
    push("zip64-missing-extra", build_zip(&z64m, &RawZipOptions { zip64: true, ..RawZipOptions::default() }));
    let full = build_zip(&good, &RawZipOptions::default());
    push("truncated-end", full[..full.len() - 30].to_vec());
    push("truncated-half", full[..full.len() / 2].to_vec());
    push("no-eocd", vec![0; 200]);
    push("count0", build_zip(&[], &RawZipOptions::default()));
    push("count-plus-one", build_zip(&good, &RawZipOptions { count: Some(good.len() as u64 + 1), ..RawZipOptions::default() }));
    push("count-minus-five", build_zip(&good, &RawZipOptions { count: Some(good.len() as u64 - 5), ..RawZipOptions::default() }));
    let mut ratio = good.clone();
    ratio.push(RawZipEntry::deflated("ppt/media/ratio.bin", deflate_raw(&[0u8; 2000], 6), 50 << 20));
    push("slides-ratio-filter", build_zip(&ratio, &RawZipOptions::default()));
    let mut off = good.clone();
    if let Some(e) = off.get_mut(5) {
        e.offset = Some(3);
    }
    push("bad-offset", build_zip(&off, &RawZipOptions::default()));
    let mut local = good.clone();
    if let Some(e) = local.get_mut(5) {
        e.local_name = Some(b"other-local-name-longer".to_vec());
    }
    push("local-name-differs", build_zip(&local, &RawZipOptions::default()));

    // unclosed constructs on which sheet-core hangs or reads garbage (gate 1, G5)
    let mut p = base.clone();
    for (i, x) in
        ["<sst><si><t>x</t></si><!-- sem fim", "<a><![CDATA[ sem fim", "<a><b>x</b></a", "<a><b/></b", "<a>&#x41;</"].iter().enumerate()
    {
        p.push(QPart::new(&format!("xl/unclosed{i}.xml"), x.as_bytes().to_vec()));
    }
    p.push(QPart::new("xl/worksheets/_rels/sheet2.xml.rels", format!("<Relationships xmlns=\"{RELS}\"><!-- sem fim")));
    push("unclosed-constructs", zip(&p));

    // embedded compound files (OLE objects): real .doc files and seeded mutations of them in the
    // header, FAT and directory sectors, read by the three CFB readers
    let docs: [&[u8]; 3] = [
        include_bytes!("../../../../../write/packages/docx-core/test/fixtures/doc/tabelas.doc"),
        include_bytes!("../../../../../write/packages/docx-core/test/fixtures/doc/notas-campos.doc"),
        include_bytes!("../../../../../write/packages/docx-core/test/fixtures/doc/imagens.doc"),
    ];
    let mut crng = Rng::new(0x0c0f_be11);
    let mut p = base.clone();
    for k in 0..48 {
        let mut d = docs[k % docs.len()].to_vec();
        if k >= docs.len() {
            if crng.one_in(6) {
                let cut = 8 + crng.below(d.len() - 8);
                d.truncate(cut);
            } else {
                for _ in 0..1 + crng.below(4) {
                    let at = 8 + if crng.one_in(2) { crng.below(504) } else { crng.below(d.len().min(8192) - 8) };
                    d[at] = crng.next_u32() as u8;
                }
            }
        }
        p.push(QPart { stored: k % 2 == 0, ..QPart::new(&format!("word/embeddings/oleObject{k}.bin"), d) });
    }
    push("embedded-cfb", zip(&p));

    // corrupt deflate streams: bit flips and truncations in one part
    let mut rng = Rng::new(0x5eed_0bc5);
    for k in 0..60 {
        let mut entries: Vec<RawZipEntry> = base.iter().map(entry).collect();
        let i = rng.below(entries.len());
        if let Some(e) = entries.get_mut(i) {
            let d = &mut e.data;
            if !d.is_empty() {
                if rng.one_in(4) {
                    let cut = rng.below(d.len());
                    d.truncate(cut);
                } else {
                    for _ in 0..1 + rng.below(3) {
                        let at = rng.below(d.len());
                        d[at] ^= 1 << rng.below(8);
                    }
                }
            }
        }
        push(&format!("corrupt-deflate-{k}"), build_zip(&entries, &RawZipOptions::default()));
    }

    // random combinations
    let names = [
        "word/media/x.png",
        "xl/worksheets/sheet2.xml",
        "ppt/slides/slide2.xml",
        "1",
        "Word/Document.xml",
        "word\\x.xml",
        "a/../b.xml",
        "/abs.xml",
    ];
    for k in 0..150 {
        let mut p = base.clone();
        for _ in 0..1 + rng.below(4) {
            match rng.below(9) {
                0 => {
                    let i = rng.below(p.len());
                    p.remove(i);
                }
                1 => {
                    let i = rng.below(p.len());
                    let mut d = p[i].clone();
                    d.data.extend_from_slice(b" ");
                    p.push(d);
                }
                2 => {
                    let i = rng.below(p.len());
                    let j = rng.below(p.len());
                    p.swap(i, j);
                }
                3 => {
                    let i = rng.below(p.len());
                    p[i].stored = !p[i].stored;
                }
                4 => {
                    let name = names[rng.below(names.len())];
                    p.push(QPart::new(name, format!("<n k=\"{k}\"/>")));
                }
                5 => {
                    // a quirk token inside an XML part (sheet-core may hang on some: G5)
                    let xml: Vec<usize> = (0..p.len()).filter(|&i| p[i].name.ends_with(b".xml") || p[i].name.ends_with(b".rels")).collect();
                    if let Some(&i) = xml.get(rng.below(xml.len())) {
                        let d = &mut p[i].data;
                        let at = rng.below(d.len() + 1);
                        let tok = XML_TOKENS[rng.below(XML_TOKENS.len())];
                        d.splice(at..at, tok.bytes());
                    }
                }
                6 => {
                    let i = rng.below(p.len());
                    p[i].flags ^= 0x800;
                }
                7 => {
                    // truncate an XML part
                    let i = rng.below(p.len());
                    let n = p[i].data.len();
                    p[i].data.truncate(rng.below(n + 1));
                }
                _ => {
                    let i = rng.below(p.len());
                    let t = rng.below(4);
                    p[i].name = match t {
                        0 => p[i].name.to_ascii_uppercase(),
                        1 => [b"./".as_slice(), &p[i].name].concat(),
                        2 => p[i].name.iter().map(|&b| if b == b'/' { b'\\' } else { b }).collect(),
                        _ => [&p[i].name, b"/".as_slice()].concat(),
                    };
                }
            }
        }
        push(&format!("random-{k}"), zip(&p));
    }
    out
}
