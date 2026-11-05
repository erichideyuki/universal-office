//! No-panic loops of frontier R5 (`read_document` and what it is made of): the synthetic whole-document
//! packages (`tests/parity/full.inputs`, `sources.inputs`) read with and without sources and with
//! small limits; seeded mutations of their parts and of the parts of oracle documents (a character
//! replaced, removed, duplicated, a junk string inserted, a tag cut, a stretch removed); deep nesting
//! (wrappers around blocks, rows and cells, tables and text boxes at and past the limit of 64 stories,
//! on the 1 MiB stack of wasm32 where the structure allows it); and hostile shapes (a hundred
//! thousand paragraphs, bookmarks, notes and comments; thousands of sections and VML boxes). A panic
//! (or a stack overflow) fails the test; the result is not looked at (the parity tests look at it)
//! except for the limits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::error::DocxLimitKind;
use uo_docx::limits::DocxLimits;
use uo_docx::model::Document;
use uo_docx::read::inspect::{document_custom_inlines, document_is_empty, document_links};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::{DocView, DocxError};
use uo_opc::Package;
use uo_testkit::repo_root;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\"";

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

/// A sink that counts what the digest writes (the walk must not panic or overflow).
struct Count(u64);

impl CanonSink for Count {
    fn tag(&mut self, _: u8) {
        self.0 += 1;
    }
    fn u32(&mut self, _: u32) {
        self.0 += 1;
    }
    fn f64(&mut self, _: f64) {
        self.0 += 1;
    }
    fn bool(&mut self, _: bool) {
        self.0 += 1;
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0 += b.len() as u64;
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0 += b.len() as u64;
    }
}

fn package(parts: Vec<(String, Vec<u8>)>) -> Package {
    Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

/// Reads, digests and queries a document; the number of paragraphs of the body (0 on an error).
fn read_all(pkg: &Package, sources: bool, limits: &DocxLimits) -> Result<usize, DocxError> {
    let opts = ReadOptions { sources, limits: limits.clone() };
    let doc: Document = read_document(pkg, &opts)?;
    let mut sink = Count(0);
    {
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        doc.canon_full(&mut w);
    }
    assert!(sink.0 > 0);
    let _ = (document_is_empty(&doc.body), document_links(&doc.body).len(), document_custom_inlines(&doc.body, None).len());
    Ok(doc.body.paras.len())
}

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

type Input = (String, Vec<(String, String)>);

fn inputs(name: &str) -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity").join(name)).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t') {
            out.last_mut().unwrap().1.push((path.to_owned(), unescape(data)));
        }
    }
    out
}

fn all_inputs() -> Vec<Input> {
    let mut v = inputs("full.inputs");
    v.extend(inputs("sources.inputs"));
    v
}

fn pkg_of(parts: &[(String, String)]) -> Package {
    package(parts.iter().map(|(n, t)| (n.clone(), t.as_bytes().to_vec())).collect())
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(7) {
            0 => chars[at] = ['"', '<', '>', '/', '=', ';', ':', ' ', '\u{a0}', '\u{0}', '😀', 'é', '&', '\''][rng.below(14)],
            1 => {
                chars.remove(at);
            }
            2 => chars.insert(at, chars[at]),
            3 => {
                let junk =
                    ["#😀", "1e999", "--", "99999999999999999999", "-0", "NaN", "&#xD800;", "\u{feff}", "px", "%", "f", ".", ",", "\\"]
                        [rng.below(14)];
                for (k, c) in junk.chars().enumerate() {
                    chars.insert(at + k, c);
                }
            }
            4 => chars.truncate(at),
            5 => {
                let to = (at + 1 + rng.below(40)).min(chars.len());
                chars.drain(at..to);
            }
            _ => {
                let other = rng.below(chars.len());
                chars.swap(at, other);
            }
        }
    }
    chars.into_iter().collect()
}

fn small_limits() -> DocxLimits {
    DocxLimits {
        max_nesting: 2,
        max_field_depth: 3,
        max_fields_per_story: 6,
        max_runs_per_part: 40,
        max_paragraphs_per_part: 3,
        ..DocxLimits::DEFAULT
    }
}

/// Runs `f` on a thread with `kb` KiB of stack (1,024 is the stack of wasm32).
fn on_stack<T: Send + 'static>(kb: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(kb * 1024).spawn(f).unwrap().join().unwrap()
}

/// `documentHyphenationLanguages` visits a text box's `paras` and `blocks` views both, at every level:
/// 2^depth visits (the TS does the same; finding for F2 and the security review), so the deep inputs
/// are left out of this walk.
#[test]
fn hyphenation_languages_over_the_inputs_do_not_panic() {
    for (key, parts) in all_inputs().iter().filter(|(k, _)| !k.contains("/nest-")) {
        let _ = key;
        if let Ok(doc) = read_document(&pkg_of(parts), &ReadOptions::default()) {
            let _ = uo_docx::word::document_hyphenation_languages(DocView::new(&doc), "pt-BR", true);
        }
    }
}

#[test]
fn synthetic_inputs_do_not_panic() {
    let mut total = 0;
    for (_, parts) in all_inputs() {
        let pkg = pkg_of(&parts);
        for sources in [false, true] {
            total += read_all(&pkg, sources, &DocxLimits::DEFAULT).unwrap_or(0);
        }
    }
    assert!(total > 1000);
}

#[test]
fn small_limits_over_the_inputs_are_errors_not_panics() {
    let limits = small_limits();
    let mut errors = 0;
    for (_, parts) in all_inputs() {
        let pkg = pkg_of(&parts);
        for sources in [false, true] {
            if let Err(DocxError::Limit { .. }) = read_all(&pkg, sources, &limits) {
                errors += 1;
            }
        }
    }
    assert!(errors > 20, "the small limits are hit: {errors}");
}

#[test]
fn mutated_inputs_do_not_panic() {
    let inputs = all_inputs();
    let mut rng = Rng(0x5eed_0005);
    let small = small_limits();
    for i in 0..3000 {
        let (_, parts) = &inputs[rng.below(inputs.len())];
        let mut parts = parts.clone();
        for _ in 0..1 + rng.below(2) {
            let k = rng.below(parts.len());
            parts[k].1 = mutate(&mut rng, &parts[k].1);
        }
        let pkg = pkg_of(&parts);
        let _ = read_all(&pkg, i % 2 == 0, if i % 5 == 0 { &small } else { &DocxLimits::DEFAULT });
    }
}

#[test]
fn mutated_oracle_documents_do_not_panic() {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("write/packages/docx-layout/test/oracle");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    let mut sub: Vec<std::path::PathBuf> = Vec::new();
    for d in files.iter().filter(|p| p.is_dir()).take(40) {
        let mut inner: Vec<_> = std::fs::read_dir(d).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        inner.sort();
        sub.extend(inner.into_iter().filter(|p| p.extension().is_some_and(|e| e == "docx")).take(1));
    }
    assert!(sub.len() > 20);
    let mut rng = Rng(0x0dd5_0005);
    let small = small_limits();
    for (n, file) in sub.iter().enumerate() {
        let bytes = std::fs::read(file).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
        let names: Vec<String> = pkg.part_names().filter(|n| n.ends_with(".xml") || n.ends_with(".rels")).map(str::to_owned).collect();
        let texts: Vec<(String, String)> =
            names.iter().filter_map(|n| pkg.part_text(n).ok().flatten().map(|t| (n.clone(), t.into_owned()))).collect();
        for i in 0..40 {
            let mut parts = texts.clone();
            let k = rng.below(parts.len());
            parts[k].1 = mutate(&mut rng, &parts[k].1);
            let _ = read_all(&pkg_of(&parts), (i + n) % 2 == 0, if i % 4 == 0 { &small } else { &DocxLimits::DEFAULT });
        }
    }
}

// ───────────────────────── depth ─────────────────────────

fn doc_pkg(body: &str, extra: &[(&str, String)]) -> Package {
    let mut parts = vec![("word/document.xml".to_owned(), format!("<w:document {NS}><w:body>{body}</w:body></w:document>").into_bytes())];
    for (n, t) in extra {
        parts.push(((*n).to_owned(), t.clone().into_bytes()));
    }
    package(parts)
}

fn chain(open: &str, close: &str, n: usize, leaf: &str) -> String {
    format!("{}{leaf}{}", open.repeat(n), close.repeat(n))
}

fn p(t: &str) -> String {
    format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>")
}

#[test]
fn deep_wrappers_do_not_overflow_the_wasm_stack() {
    on_stack(1024, || {
        let leaf = p("x");
        let cases = vec![
            chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, &leaf),
            chain("<w:customXml w:element=\"e\">", "</w:customXml>", 480, &leaf),
            format!(
                "<w:tbl><w:tr>{}</w:tr></w:tbl>",
                chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, "<w:tc><w:p/></w:tc>")
            ),
            format!(
                "<w:tbl>{}</w:tbl>",
                chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, "<w:tr><w:tc><w:p/></w:tc></w:tr>")
            ),
            format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, &leaf)),
            format!(
                "<w:tbl><w:tr>{}</w:tr></w:tbl>",
                chain("<w:customXml w:element=\"c\"><w:customXmlPr/>", "</w:customXml>", 400, "<w:tc/>")
            ),
            chain("<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, &leaf),
            format!(
                "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>",
                chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 200, "<w:p><w:pPr><w:sectPr/></w:pPr></w:p>")
            ),
        ];
        for body in cases {
            let pkg = doc_pkg(&body, &[]);
            for sources in [false, true] {
                let _ = read_all(&pkg, sources, &DocxLimits::DEFAULT);
            }
        }
    });
}

fn tables(n: usize) -> String {
    chain("<w:tbl><w:tr><w:tc>", "</w:tc></w:tr></w:tbl>", n, &p("leaf"))
}

fn boxes(n: usize) -> String {
    let open = "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/><wp:docPr id=\"1\" name=\"b\"/><a:graphic><a:graphicData uri=\"x\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>";
    let close = "</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    chain(open, close, n, &p("leaf"))
}

fn vboxes(n: usize) -> String {
    let open =
        "<w:p><w:r><w:pict><v:shape id=\"v\" type=\"#t\" style=\"position:absolute;width:10pt;height:10pt\"><v:textbox><w:txbxContent>";
    let close = "</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>";
    chain(open, close, n, &p("leaf"))
}

fn mixed(n: usize) -> String {
    let mut s = p("leaf");
    for i in 0..n {
        s = if i % 2 == 0 {
            format!("<w:tbl><w:tr><w:tc>{s}</w:tc></w:tr></w:tbl>")
        } else {
            format!(
                "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1\" cy=\"1\"/><wp:docPr id=\"1\" name=\"b\"/><a:graphic><a:graphicData uri=\"x\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>{s}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
            )
        };
    }
    s
}

/// The stories nest to the limit and past it, on the 1 MiB stack of wasm32 (the text box readers were
/// slimmed for it: the record of R4c, item 5).
#[test]
fn nesting_at_and_past_the_limit_of_64_stories_does_not_panic() {
    on_stack(1024, || {
        for n in [1, 8, 63, 64, 65, 66, 200] {
            for body in [tables(n), boxes(n), vboxes(n), mixed(n)] {
                let pkg = doc_pkg(&body, &[]);
                for sources in [false, true] {
                    let _ = read_all(&pkg, sources, &DocxLimits::DEFAULT);
                }
            }
        }
    });
}

/// A host that raises `max_nesting` past 64 pays the stack for it: 250 levels on a large stack.
#[test]
fn a_raised_nesting_limit_works_on_a_large_stack() {
    on_stack(64 * 1024, || {
        let high = DocxLimits { max_nesting: 300, ..DocxLimits::DEFAULT };
        for body in [tables(250), boxes(250), mixed(250)] {
            let _ = read_all(&doc_pkg(&body, &[]), true, &high);
        }
    });
}

#[test]
fn tables_nested_to_the_limit_fit_the_wasm_stack() {
    on_stack(1024, || {
        for n in [64, 65, 120] {
            for sources in [false, true] {
                read_all(&doc_pkg(&tables(n), &[]), sources, &DocxLimits::DEFAULT).unwrap();
            }
        }
    });
}

// ───────────────────────── hostile shapes ─────────────────────────

#[test]
fn a_hundred_thousand_paragraphs_is_linear_and_the_limit_is_an_error() {
    let body: String = (0..100_000).map(|i| format!("<w:p><w:r><w:t>{i}</w:t></w:r></w:p>")).collect();
    let pkg = doc_pkg(&body, &[]);
    assert_eq!(read_all(&pkg, false, &DocxLimits::DEFAULT).unwrap(), 100_000);
    let limits = DocxLimits { max_paragraphs_per_part: 99_999, ..DocxLimits::DEFAULT };
    assert!(matches!(read_all(&pkg, false, &limits), Err(DocxError::Limit { kind: DocxLimitKind::ParagraphsPerPart, .. })));
}

#[test]
fn many_bookmarks_sections_notes_and_comments_are_linear() {
    let hdr = format!("<w:hdr {NS}><w:p><w:r><w:t>h</w:t></w:r></w:p></w:hdr>");
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rH\" Type=\"x/header\" Target=\"header1.xml\"/></Relationships>".to_owned();
    let mut body = String::new();
    for i in 0..5000 {
        body.push_str(&format!(
            "<w:p><w:bookmarkStart w:id=\"{i}\" w:name=\"b{}\"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id=\"{i}\"/><w:pPr><w:sectPr><w:headerReference w:type=\"default\" r:id=\"rH\"/></w:sectPr></w:pPr></w:p>",
            i % 700
        ));
    }
    let notes: String = (0..10_000).map(|i| format!("<w:footnote w:id=\"{i}\"><w:p><w:r><w:t>n</w:t></w:r></w:p></w:footnote>")).collect();
    let comments: String = (0..10_000).map(|i| format!("<w:comment w:id=\"{i}\"><w:p><w:r><w:t>c</w:t></w:r></w:p></w:comment>")).collect();
    let pkg = doc_pkg(
        &body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", hdr),
            ("word/footnotes.xml", format!("<w:footnotes {NS}>{notes}</w:footnotes>")),
            ("word/comments.xml", format!("<w:comments {NS}>{comments}</w:comments>")),
        ],
    );
    for sources in [false, true] {
        let doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() }).unwrap();
        assert_eq!(doc.sections.len(), 5000);
        assert_eq!(doc.hf.len(), 1, "one part, read once");
        assert_eq!(doc.bookmarks.starts.len(), 700);
        assert_eq!(doc.notes.footnotes.len(), 10_000);
        assert_eq!(doc.comments.len(), 10_000);
    }
}

fn vbox(id: usize, next: Option<usize>) -> String {
    let style = next.map_or(String::new(), |n| format!(" style=\"mso-next-textbox:#b{n}\""));
    format!(
        "<w:r><w:pict><v:shape id=\"b{id}\" type=\"#t\" style=\"position:absolute\"><v:textbox{style}><w:txbxContent><w:p/></w:txbxContent></v:textbox></v:shape></w:pict></w:r>"
    )
}

#[test]
fn long_vml_chains_and_many_heads_are_bounded() {
    // one chain of 5,000 boxes, then 400 heads that all point into its tail (the TS walks each)
    let mut runs = String::new();
    for i in 0..5000 {
        runs.push_str(&vbox(i, (i + 1 < 5000).then_some(i + 1)));
    }
    for h in 0..400 {
        runs.push_str(&vbox(10_000 + h, Some(2500)));
    }
    let pkg = doc_pkg(&format!("<w:p>{runs}</w:p>"), &[]);
    let t = std::time::Instant::now();
    assert!(read_all(&pkg, false, &DocxLimits::DEFAULT).is_ok());
    assert!(t.elapsed().as_secs() < 60, "{:?}", t.elapsed());
}

/// Text boxes nested to the limit of 64 on the stack `UO_R5_STACK_KB` KiB (default 64 MiB): the record
/// measures the least stack that passes (`UO_R5_STACK_KB=1024` is the wasm32 stack).
#[test]
fn text_boxes_at_the_limit_on_the_configured_stack() {
    let kb: usize = std::env::var("UO_R5_STACK_KB").ok().and_then(|v| v.parse().ok()).unwrap_or(64 * 1024);
    on_stack(kb, || {
        for body in [boxes(64), vboxes(64), mixed(64)] {
            for sources in [false, true] {
                read_all(&doc_pkg(&body, &[]), sources, &DocxLimits::DEFAULT).unwrap();
            }
        }
    });
}
