//! F7c (notes, headers/footers, fields): memory (M1 per call, M2 soak, M3 numbers), the limits and
//! the no-panic loop over hostile notes, headers/footers and field nesting, through the real path
//! (uo-docx reader → `derive::build` → `engine::layout_document`) with the repository's open fonts.

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
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

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

/// The parts of a test document beyond the body.
#[derive(Default)]
struct Parts {
    /// (file, `hdr`|`ftr`, paragraphs XML)
    hf: Vec<(String, &'static str, String)>,
    footnotes: String,
    endnotes: String,
    settings: String,
}

/// A docx with `body` (block XML incl. the final sectPr), the header/footer parts (referenced as
/// `r:id` = the file stem), footnotes and endnotes.
fn docx(body: &str, parts: &Parts) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/><w:pPr><w:keepNext/><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="H2"><w:name w:val="heading 2"/><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="FT"><w:name w:val="footnote text"/><w:pPr><w:spacing w:after="0"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr></w:style><w:style w:type="character" w:styleId="FR"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let settings =
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{}</w:settings>"#, parts.settings);
    let sep = |k: &str| {
        format!(
            r#"<w:{k} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{k}><w:{k} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{k}>"#
        )
    };
    let mut files: Vec<(String, String)> = vec![
        ("word/document.xml".into(), doc),
        ("word/styles.xml".into(), styles),
        ("word/numbering.xml".into(), numbering),
        ("word/settings.xml".into(), settings),
    ];
    let over = |p: &str, t: &str| {
        format!(r#"<Override PartName="/{p}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{t}+xml"/>"#)
    };
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    );
    ct += &over("word/document.xml", "document.main");
    ct += &over("word/styles.xml", "styles");
    ct += &over("word/numbering.xml", "numbering");
    ct += &over("word/settings.xml", "settings");
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>"#,
    );
    for (file, tag, paras) in &parts.hf {
        let (kind, root) = if *tag == "hdr" { ("header", "hdr") } else { ("footer", "ftr") };
        files.push((
            format!("word/{file}.xml"),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} xmlns:w="{W}" xmlns:r="{R}">{paras}</w:{root}>"#),
        ));
        ct += &over(&format!("word/{file}.xml"), kind);
        rels += &format!(r#"<Relationship Id="{file}" Type="{R}/{kind}" Target="{file}.xml"/>"#);
    }
    for (name, kind, body) in [("footnotes", "footnote", &parts.footnotes), ("endnotes", "endnote", &parts.endnotes)] {
        if body.is_empty() {
            continue;
        }
        files.push((
            format!("word/{name}.xml"),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{name} xmlns:w="{W}">{}{body}</w:{name}>"#, sep(kind)),
        ));
        ct += &over(&format!("word/{name}.xml"), name);
        rels += &format!(r#"<Relationship Id="r{name}" Type="{R}/{name}" Target="{name}.xml"/>"#);
    }
    ct += "</Types>";
    rels += "</Relationships>";
    let pkg_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    z.add("[Content_Types].xml", ct.as_bytes()).unwrap();
    z.add("_rels/.rels", pkg_rels.as_bytes()).unwrap();
    z.add("word/_rels/document.xml.rels", rels.as_bytes()).unwrap();
    for (name, data) in &files {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

fn words(n: usize, seed: usize) -> String {
    const WS: [&str; 12] =
        ["o", "motor", "de", "layout", "quebra", "linhas", "notas", "rodape", "informação", "parágrafo", "página", "responsabilidade"];
    (0..n).map(|i| WS[(i * 7 + seed * 3) % WS.len()]).collect::<Vec<_>>().join(" ")
}

fn run(t: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{t}</w:t></w:r>"#)
}

fn fld(instr: &str) -> String {
    format!(r#"<w:fldSimple w:instr=" {} "><w:r><w:t>1</w:t></w:r></w:fldSimple>"#, instr.replace('"', "&quot;"))
}

fn note_ref(kind: &str, id: &str) -> String {
    format!(r#"<w:r><w:rPr><w:rStyle w:val="FR"/></w:rPr><w:{kind}Reference w:id="{id}"/></w:r>"#)
}

fn note(kind: &str, id: &str, paras: usize, len: usize) -> String {
    let mut ps = String::new();
    for k in 0..paras {
        let mark = if k == 0 { format!(r#"<w:r><w:rPr><w:rStyle w:val="FR"/></w:rPr><w:{kind}Ref/></w:r>"#) } else { String::new() };
        ps += &format!(r#"<w:p><w:pPr><w:pStyle w:val="FT"/></w:pPr>{mark}{}</w:p>"#, run(&words(len, k)));
    }
    format!(r#"<w:{kind} w:id="{id}">{ps}</w:{kind}>"#)
}

/// A document of every F7c path: headers and footers in three slots (with fields, a list and a
/// border), footnotes (one long enough to split, restart each page in section 2, beneath text),
/// endnotes (a two-column section end), SEQ/REF/PAGEREF/NOTEREF/STYLEREF over bookmarks and
/// numbered headings.
fn every_path() -> Vec<u8> {
    let mut parts =
        Parts { settings: r#"<w:evenAndOddHeaders/><w:endnotePr><w:pos w:val="sectEnd"/></w:endnotePr>"#.into(), ..Parts::default() };
    let hf_para = |t: &str| {
        format!(
            r#"<w:p><w:pPr><w:jc w:val="center"/><w:pBdr><w:bottom w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr></w:pPr>{}{}{}{}</w:p><w:p><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr>{}</w:p>"#,
            run(t),
            fld("PAGE"),
            run(" / "),
            fld("NUMPAGES"),
            run("item")
        )
    };
    for (f, tag, t) in
        [("h1", "hdr", "Cabeçalho"), ("h2", "hdr", "Primeira"), ("h3", "hdr", "Par"), ("f1", "ftr", "Rodapé"), ("f2", "ftr", "Seção")]
    {
        parts.hf.push((f.into(), tag, hf_para(t)));
    }
    let mut b = String::new();
    let mut fid = 1;
    for i in 0..44 {
        let style = match i % 11 {
            0 => r#"<w:pStyle w:val="H1"/>"#,
            5 => r#"<w:pStyle w:val="H2"/>"#,
            _ => "",
        };
        let mut content = String::new();
        if i % 7 == 1 {
            content += &format!(r#"<w:bookmarkStart w:id="{i}" w:name="bm{i}"/>{}<w:bookmarkEnd w:id="{i}"/>"#, run(&words(6, i)));
        }
        content += &run(&words(20 + (i * 17) % 70, i));
        if i % 3 == 0 {
            fid += 1;
            content += &note_ref("footnote", &fid.to_string());
            parts.footnotes += &note("footnote", &fid.to_string(), 1 + i % 2, if i % 9 == 0 { 400 } else { 20 });
        }
        if i % 5 == 2 {
            fid += 1;
            content += &note_ref("endnote", &fid.to_string());
            parts.endnotes += &note("endnote", &fid.to_string(), 1 + i % 3, 30);
        }
        if i % 4 == 3 {
            content +=
                &fld(["SEQ Figura", "SEQ Figura \\s 1", "SEQ Tabela \\* ROMAN", "SEQ Figura \\r 7", "SEQ Figura \\c", "SEQ Figura \\h"]
                    [i % 6]);
        }
        if i > 8 && i % 6 == 4 {
            let bm = format!("bm{}", (i / 7) * 7 + 1);
            content += &fld(&[
                format!("REF {bm} \\h"),
                format!("PAGEREF {bm} \\h"),
                format!("NOTEREF {bm}"),
                format!("REF {bm} \\p"),
                format!("REF {bm} \\w \\d \"-\""),
            ][i % 5]);
        }
        if i % 13 == 6 {
            content += &fld(["STYLEREF 1", "STYLEREF \"1\" \\n", "STYLEREF 2 \\s"][i % 3]);
        }
        b += &format!(r#"<w:p><w:pPr>{style}</w:pPr>{content}</w:p>"#);
        if i == 21 {
            b += r#"<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="h1"/><w:headerReference w:type="first" r:id="h2"/><w:headerReference w:type="even" r:id="h3"/><w:footerReference w:type="default" r:id="f1"/><w:footnotePr><w:numFmt w:val="lowerRoman"/></w:footnotePr><w:type w:val="nextPage"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:titlePg/></w:sectPr></w:pPr></w:p>"#;
        }
    }
    b += r#"<w:sectPr><w:footerReference w:type="default" r:id="f2"/><w:footnotePr><w:pos w:val="beneathText"/><w:numRestart w:val="eachPage"/></w:footnotePr><w:type w:val="continuous"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:pgNumType w:start="3" w:fmt="lowerRoman"/><w:cols w:num="2" w:space="708"/></w:sectPr>"#;
    docx(&b, &parts)
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

/// Every index the output holds is valid (lines → chars and pfmts, item metadata → items).
fn check_indices(l: &Layout) {
    for p in &l.pages {
        for line in &p.lines {
            if let Some(s) = line.chars {
                assert!((s.start + s.len) as usize <= p.chars.o.len());
            }
            if let Some(f) = line.pfmt {
                assert!((f as usize) < p.pfmts.len());
            }
            assert_ne!(line.at.to_ts(), "ensep", "the endnote separator's line leaves the page");
        }
        for (i, _) in &p.item_meta {
            assert!((*i as usize) < p.page.list.items.len());
        }
        assert!(p.hf_items.is_some_and(|h| h as usize <= p.page.list.items.len()));
    }
}

#[test]
fn the_every_path_document_lays_out() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let l = layout_bytes(&every_path(), &fonts, &reg, &opts()).unwrap();
    assert!(l.pages.len() >= 4, "{} pages", l.pages.len());
    check_indices(&l);
    let refs: Vec<String> = l.pages.iter().flat_map(|p| p.lines.iter().map(|x| x.at.to_ts())).collect();
    for want in ["hdr:first/b0", "hdr:default/b0", "ftr:default/b0", "fn2/b0", "en3/b0"] {
        assert!(
            refs.iter().any(|r| r == want),
            "no line {want}: {:?}",
            refs.iter().filter(|r| !r.starts_with('b')).collect::<std::collections::BTreeSet<_>>()
        );
    }
    assert!(l.pages.iter().all(|p| p.hf_items.unwrap_or(0) > 0), "a page without its header/footer");
}

#[test]
fn m1_a_layout_call_returns_every_byte() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    let u = assert_no_leak(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
    eprintln!("M1 every-path document: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    // a call that ends in a limit mid-pass (inside a header, a note or the endnote flow) too
    for max_pages in [1, 2, 3] {
        let tight = LayoutOptions { limits: LayoutLimits { max_pages, ..LayoutLimits::default() }, ..opts() };
        assert_no_leak(|| assert_eq!(layout_bytes(&bytes, &fonts, &reg, &tight).unwrap_err(), LayoutError::Limit(LimitKind::Pages)));
    }
}

#[test]
fn m2_soak_open_layout_drop() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let one = std::fs::read(common::manifest().join("tests/captures/f7c/docs/f7c__7__0.docx")).unwrap();
    let every = every_path();
    let o = opts();
    let (n1, n2) = if cfg!(debug_assertions) { (60, 6) } else { (1_000, 50) };
    for (bytes, n) in [(&one, n1), (&every, n2)] {
        drop(layout_bytes(bytes, &fonts, &reg, &o).unwrap());
        let series = soak(n, (n / 20).max(1), |_| drop(layout_bytes(bytes, &fonts, &reg, &o).unwrap()));
        eprintln!(
            "M2 soak ({n} × open → layout → drop): first {} last {} min {} max {}",
            series[0],
            series[series.len() - 1],
            series.iter().min().unwrap(),
            series.iter().max().unwrap()
        );
        assert_flat(&series, 1, 0);
    }
}

#[test]
fn m3_numbers_of_the_fixture_documents() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let dir = common::manifest().join("tests/captures/f7c/docs");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut docs: Vec<(String, Vec<u8>)> =
        rd.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap())).collect();
    docs.push(("every-path".into(), every_path()));
    for (name, bytes) in docs {
        drop(layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let (l, u) = measure(|| layout_bytes(&bytes, &fonts, &reg, &opts()).unwrap());
        let chars: usize = l.pages.iter().map(|p| p.chars.o.len()).sum();
        eprintln!(
            "M3 {name}: {} pages, {chars} chars, {} B allocated, {} B peak, {} B retained ({:.1} B/char)",
            l.pages.len(),
            u.allocated,
            u.peak_above_start,
            u.live_delta,
            u.live_delta as f64 / chars.max(1) as f64
        );
    }
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

/// A complex field `{ code }` with `nested` complex fields inside its code (field nesting).
fn complex_field(code: &str, nested: usize) -> String {
    let begin = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#;
    let sep = r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#;
    let end = r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#;
    let mut s = String::new();
    for _ in 0..nested {
        s += begin;
        s += &format!(r#"<w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r>"#);
    }
    s += begin;
    s += &format!(r#"<w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r>"#);
    s += sep;
    s += &run("9");
    s += end;
    for _ in 0..nested {
        s += sep;
        s += &run("8");
        s += end;
    }
    s
}

#[test]
fn no_panic_on_hostile_notes_headers_and_fields() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let ids = ["1", "2", "3", "-1", "0", "NaN", "1e9", "99999999999", "abc", "", "2.5", "-0"];
    let nums = ["0", "-1", "1", "7", "720", "-720", "1e9", "NaN", "abc", "", "0.5", "45"];
    let codes = [
        "PAGE",
        "NUMPAGES",
        "SEQ a \\s 9",
        "SEQ a \\r 1e400",
        "SEQ \\h",
        "SEQ",
        "SEQ a \\* constructor",
        "REF",
        "REF nada \\n",
        "REF bm \\w \\d \"",
        "REF bm \\p",
        "PAGEREF",
        "PAGEREF bm",
        "NOTEREF bm",
        "NOTEREF bm \\p",
        "STYLEREF",
        "STYLEREF 9",
        "STYLEREF \"1\" \\s \\n",
        "STYLEREF 1 \\s",
        "= 1 + 2",
        "SAVEDATE",
        "DATE \\@ \"dddd, d 'de' MMMM\"",
        "FOOTNOTEREF_MARK",
        "ENDNOTEREF_MARK",
    ];
    let mut rng = Lcg(11);
    let (mut ok, mut errs) = (0, Vec::new());
    let o = LayoutOptions { limits: LayoutLimits { max_pages: 60, ..LayoutLimits::default() }, ..opts() };
    let cases = if cfg!(debug_assertions) { 30 } else { 200 };
    for case in 0..cases {
        let mut parts = Parts {
            settings: rng
                .pick(&[
                    "",
                    "<w:evenAndOddHeaders/>",
                    r#"<w:endnotePr><w:pos w:val="sectEnd"/></w:endnotePr>"#,
                    r#"<w:footnotePr><w:pos w:val="beneathText"/><w:numRestart w:val="eachPage"/></w:footnotePr>"#,
                ])
                .into(),
            ..Parts::default()
        };
        // headers/footers: tall ones, empty ones, with note references, fields and nested fields
        let mut hf_refs = String::new();
        for (k, (tag, slot)) in [("hdr", "default"), ("ftr", "default"), ("hdr", "first"), ("ftr", "even")].into_iter().enumerate() {
            if rng.next().is_multiple_of(3) {
                continue;
            }
            let n = (rng.next() % 4) as usize * if rng.next().is_multiple_of(10) { 8 } else { 1 };
            let mut ps = String::new();
            for i in 0..n {
                let code = rng.pick(&codes);
                ps += &format!(
                    r#"<w:p><w:pPr><w:spacing w:before="{}" w:after="{}"/>{}</w:pPr>{}{}{}</w:p>"#,
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&[
                        "",
                        r#"<w:numPr><w:numId w:val="1"/></w:numPr>"#,
                        r#"<w:rPr><w:vanish/></w:rPr>"#,
                        r#"<w:pBdr><w:bottom w:val="single" w:sz="1e9"/></w:pBdr>"#
                    ]),
                    run(&words((rng.next() % 30) as usize, i)),
                    if rng.next().is_multiple_of(2) { fld(code) } else { complex_field(code, (rng.next() % 4) as usize) },
                    if rng.next().is_multiple_of(6) { note_ref("footnote", rng.pick(&ids)) } else { String::new() },
                );
            }
            let file = format!("p{k}");
            parts.hf.push((file.clone(), tag, ps));
            let elem = if tag == "hdr" { "headerReference" } else { "footerReference" };
            hf_refs += &format!(r#"<w:{elem} w:type="{slot}" r:id="{file}"/>"#);
        }
        // notes: missing, duplicated, self-referencing, long, with fields
        let mut body = String::new();
        let n = 1 + rng.next() % 10;
        for i in 0..n {
            let mut content = String::new();
            if rng.next().is_multiple_of(4) {
                content += r#"<w:bookmarkStart w:id="1" w:name="bm"/>"#;
            }
            content += &run(&words((rng.next() % 80) as usize, i as usize));
            for _ in 0..rng.next() % 3 {
                let kind = rng.pick(&["footnote", "endnote"]);
                let id = rng.pick(&ids);
                content += &note_ref(kind, id);
                if !rng.next().is_multiple_of(3) {
                    let mut nb = note(kind, id, 1 + (rng.next() % 3) as usize, (rng.next() % 400) as usize);
                    if rng.next().is_multiple_of(4) {
                        // a note that references itself and holds fields
                        nb = nb.replace("</w:p>", &format!("{}{}</w:p>", note_ref(kind, id), fld(rng.pick(&codes))));
                    }
                    if kind == "footnote" { parts.footnotes += &nb } else { parts.endnotes += &nb }
                }
            }
            if rng.next().is_multiple_of(3) {
                content += &complex_field(rng.pick(&codes), (rng.next() % 5) as usize);
            }
            if rng.next().is_multiple_of(5) {
                content += r#"<w:bookmarkEnd w:id="1"/>"#;
            }
            let style = rng.pick(&["", r#"<w:pStyle w:val="H1"/>"#, r#"<w:pStyle w:val="H2"/>"#]);
            body += &format!(r#"<w:p><w:pPr>{style}</w:pPr>{content}</w:p>"#);
            if rng.next().is_multiple_of(5) {
                body += &format!(
                    r#"<w:p><w:pPr><w:sectPr>{hf_refs}<w:footnotePr><w:numFmt w:val="{}"/><w:numStart w:val="{}"/></w:footnotePr><w:type w:val="{}"/><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="1701" w:bottom="{}" w:left="1701" w:header="{}" w:footer="{}"/><w:cols w:num="{}"/><w:pgNumType w:chapStyle="{}" w:chapSep="{}"/>{}</w:sectPr></w:pPr></w:p>"#,
                    rng.pick(&["chicago", "upperRoman", "decimal", "x"]),
                    rng.pick(&nums),
                    rng.pick(&["continuous", "nextPage", "evenPage", "nextColumn"]),
                    rng.pick(&["11906", "1000", "1e9", "0"]),
                    rng.pick(&["16838", "1000", "1e9", "NaN"]),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&nums),
                    rng.pick(&["1", "2", "3", "0"]),
                    rng.pick(&["1", "9", "0", "abc"]),
                    rng.pick(&["hyphen", "constructor", "emDash", ""]),
                    rng.pick(&["", "<w:titlePg/>", "<w:noEndnote/>"]),
                );
            }
        }
        body += &format!(
            r#"<w:sectPr>{hf_refs}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="{}" w:footer="{}"/><w:cols w:num="{}"/></w:sectPr>"#,
            rng.pick(&nums),
            rng.pick(&nums),
            1 + rng.next() % 2
        );
        match layout_bytes(&docx(&body, &parts), &fonts, &reg, &o) {
            Ok(l) => {
                check_indices(&l);
                ok += 1;
            }
            Err(e) => errs.push((case, e)),
        }
    }
    eprintln!("no-panic: {ok} laid out, {} errors: {:?}", errs.len(), &errs[..errs.len().min(12)]);
    for (_, e) in &errs {
        assert!(matches!(e, LayoutError::Limit(_) | LayoutError::Internal(_) | LayoutError::Unported(..)), "{e:?}");
    }
}

#[test]
fn limits_end_the_call() {
    let (fonts, reg) = (OpenFonts::new(), hyph_registry());
    let bytes = every_path();
    let work = LayoutOptions { limits: LayoutLimits { work_floor: 2_000, work_per_glyph: 0, ..LayoutLimits::default() }, ..opts() };
    assert_eq!(layout_bytes(&bytes, &fonts, &reg, &work).unwrap_err(), LayoutError::Limit(LimitKind::Work));
    // a header taller than every page: each page is laid out with its body pushed down; the call
    // ends (the pages are bounded by max_pages)
    let tall: String = (0..200).map(|i| format!("<w:p>{}</w:p>", run(&words(10, i)))).collect();
    let parts = Parts { hf: vec![("h".into(), "hdr", tall)], ..Parts::default() };
    let body = format!(
        r#"<w:p>{}</w:p><w:sectPr><w:headerReference w:type="default" r:id="h"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708"/></w:sectPr>"#,
        run(&words(3000, 1))
    );
    let few = LayoutOptions { limits: LayoutLimits { max_pages: 50, ..LayoutLimits::default() }, ..opts() };
    match layout_bytes(&docx(&body, &parts), &fonts, &reg, &few) {
        Ok(l) => check_indices(&l),
        Err(e) => assert!(matches!(e, LayoutError::Limit(_)), "{e:?}"),
    }
}
