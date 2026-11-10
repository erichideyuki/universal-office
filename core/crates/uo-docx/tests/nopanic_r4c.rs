//! No-panic loops of frontier R4c: `read_paragraph` over the paragraph quirk corpus and seeded
//! mutations of it (a character replaced, removed, duplicated, a junk string or a surrogate-pair
//! half inserted, a tag cut, a stretch removed), with and without sources and with small limits;
//! deep nesting (wrappers, ruby, fractions, delimiters, textboxes past the limit of 64 stories, in
//! a thread with the 1 MiB stack of wasm32); hostile field codes (nested to the limit and past it,
//! thousands in a row, huge instructions, stray `separate`/`end`, crossing a thousand paragraphs);
//! and the tag parsers over random strings and bytes. A panic (or a stack overflow) fails the
//! test; the result is not looked at (the parity tests look at it) except for the limits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::DocxError;
use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::error::DocxLimitKind;
use uo_docx::js::JsValue;
use uo_docx::limits::DocxLimits;
use uo_docx::model::Paragraph;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::json_parse;
use uo_docx::read::paragraph::{ReadContext, parse_custom_inline_tag};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::Package;
use uo_opc::xml::parse;

#[allow(unused_imports)]
use crate::nested_stories;

use nested_stories::NESTED_STACK_KIB;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

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

fn package() -> Package {
    Package::from_parts(
        [(
            "word/_rels/document.xml.rels".to_owned(),
            b"<Relationships><Relationship Id=\"rId1\" Type=\"x\" Target=\"https://e\" TargetMode=\"External\"/><Relationship Id=\"rId2\" Type=\"x\" Target=\"media/a.png\"/></Relationships>"
                .to_vec(),
        )],
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    )
}

/// Reads every top-level paragraph of a parsed part (errors are values), digests them, returns
/// the count of runs.
fn read_part(text: &str, sources: bool, limits: &DocxLimits) -> usize {
    let Ok(d) = parse(text.to_owned(), if sources { &DOCX_POS } else { &DOCX }) else { return 0 };
    let pkg = package();
    let opts = ReadOptions { sources, limits: limits.clone() };
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let part = if sources { ctx.source_part("word/document.xml", &d) } else { None };
    let mut paras: Vec<Paragraph> = Vec::new();
    {
        let Ok(mut rc) = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels") else { return 0 };
        rc.set_source_part(part);
        rc.collect_bookmarks = true;
        let mut stack = vec![d.root()];
        while let Some(e) = stack.pop() {
            if e.name() == "w:p" {
                match rc.read_paragraph(e) {
                    Ok(p) => paras.push(p),
                    Err(DocxError::Limit { .. }) => break,
                    Err(_) => break,
                }
                continue;
            }
            let kids: Vec<_> = e.child_elements().collect();
            stack.extend(kids.into_iter().rev());
        }
    }
    let mut sink = Count(0);
    {
        let mut w = CanonWriter::new(&mut sink).with_media(&ctx.media);
        for p in &paras {
            p.canon(&mut w);
        }
    }
    assert!(sink.0 > 0 || paras.is_empty());
    paras.iter().map(|p| p.runs.len()).sum()
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

fn quirk_texts() -> Vec<String> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/paragraph.quirks.txt")).unwrap();
    text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| unescape(l.split_once('\t').unwrap().1)).collect()
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

#[test]
fn quirk_corpus_does_not_panic() {
    let mut total = 0;
    for t in quirk_texts() {
        total += read_part(&t, false, &DocxLimits::DEFAULT);
        total += read_part(&t, true, &DocxLimits::DEFAULT);
    }
    assert!(total > 1000);
}

#[test]
fn mutated_quirk_corpus_does_not_panic() {
    let texts = quirk_texts();
    let mut rng = Rng(0x4c0d_e4c1);
    let small = DocxLimits {
        max_field_depth: 3,
        max_fields_per_story: 6,
        max_runs_per_part: 40,
        max_paragraphs_per_part: 3,
        ..DocxLimits::DEFAULT
    };
    for i in 0..3000 {
        let base = &texts[rng.below(texts.len())];
        let t = mutate(&mut rng, base);
        read_part(&t, i % 2 == 0, if i % 5 == 0 { &small } else { &DocxLimits::DEFAULT });
    }
}

#[test]
fn small_limits_over_the_corpus_are_errors_not_panics() {
    let limits = DocxLimits {
        max_field_depth: 2,
        max_fields_per_story: 3,
        max_runs_per_part: 12,
        max_paragraphs_per_part: 2,
        ..DocxLimits::DEFAULT
    };
    for t in quirk_texts() {
        read_part(&t, false, &limits);
        read_part(&t, true, &limits);
    }
}

fn doc(body: &str) -> String {
    format!("<w:document {NS}><w:body>{body}</w:body></w:document>")
}

/// Runs `f` on a thread with `kb` KiB of stack (1,024 is the stack of wasm32).
fn on_stack<T: Send + 'static>(kb: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(kb * 1024).spawn(f).unwrap().join().unwrap()
}

fn on_wasm_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    on_stack(1024, f)
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let chain = |open: &str, close: &str, n: usize, leaf: &str| format!("{}{leaf}{}", open.repeat(n), close.repeat(n));
    let cases = vec![
        chain("<w:hyperlink r:id=\"rId1\">", "</w:hyperlink>", 480, "<w:r><w:t>x</w:t></w:r>"),
        chain("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>", 240, "<w:r><w:t>x</w:t></w:r>"),
        chain(
            "<w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:a:1\"/></w:sdtPr><w:sdtContent>",
            "</w:sdtContent></w:sdt>",
            240,
            "<w:r><w:t>x</w:t></w:r>",
        ),
        chain("<w:fldSimple w:instr=\"a\">", "</w:fldSimple>", 480, "<w:r><w:t>x</w:t></w:r>"),
        chain("<w:r><w:ruby><w:rubyBase>", "</w:rubyBase></w:ruby></w:r>", 160, "<w:r><w:t>x</w:t></w:r>"),
        chain("<w:ins w:author=\"a\"><w:del w:author=\"b\"><w:smartTag>", "</w:smartTag></w:del></w:ins>", 160, "<w:r><w:t>x</w:t></w:r>"),
        format!(
            "<m:oMathPara><m:oMath>{}</m:oMath></m:oMathPara>",
            chain("<m:f><m:num>", "</m:num><m:den><m:r><m:t>d</m:t></m:r></m:den></m:f>", 480, "<m:r><m:t>x</m:t></m:r>")
        ),
        format!(
            "<m:oMathPara><m:oMath>{}</m:oMath></m:oMathPara>",
            chain("<m:d><m:e>", "</m:e><m:e><m:r><m:t>s</m:t></m:r></m:e></m:d>", 480, "<m:r><m:t>x</m:t></m:r>")
        ),
        format!("<m:oMath>{}</m:oMath>", chain("<m:box><m:e>", "</m:e></m:box>", 480, "<m:r><m:t>x</m:t></m:r>")),
        chain("<w:smartTag>", "</w:smartTag>", 980, "<w:r><w:instrText> TOC </w:instrText></w:r>"),
    ];
    for body in cases {
        let text = doc(&format!("<w:p>{body}</w:p>"));
        for sources in [false, true] {
            let text = text.clone();
            on_wasm_stack(move || read_part(&text, sources, &DocxLimits::DEFAULT));
        }
    }
}

/// A textbox inside a textbox, `n` levels (VML, content read as paragraphs).
fn nested_textboxes(n: usize) -> String {
    let open = "<w:p><w:r><w:pict><v:shape style=\"position:absolute;width:100pt;height:50pt\"><v:textbox><w:txbxContent>";
    let close = "</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>";
    format!("{}<w:p><w:r><w:t>core</w:t></w:r></w:p>{}", open.repeat(n), close.repeat(n))
}

/// A story nested in a story costs native stack on every level: a paragraph, its runs, the
/// reader of the object (frontier R4b) and the paragraphs of its textbox. The levels are bounded
/// by `max_nesting` (64) and the content past it is read as plain text; the frames of the chain
/// are small (record "Nested-story stack — fixed": 64 levels ≈ 80 KB of the wasm32 shadow stack,
/// ≈ 0.21 MB native in a release build, ≈ 0.6 MB in a debug build), so the thread gets the
/// nested-story budget (`NESTED_STACK_KIB`, `tests/nested_stack.rs` covers every kind of object).
#[test]
fn textboxes_nested_past_the_limit_do_not_overflow_the_stack() {
    for n in [1, 8, 63, 64, 65, 100, 120] {
        let text = doc(&nested_textboxes(n));
        let runs = on_stack(NESTED_STACK_KIB, move || read_part(&text, false, &DocxLimits::DEFAULT));
        assert!(runs >= 1, "{n}");
    }
    let text = doc(&nested_textboxes(100));
    on_stack(NESTED_STACK_KIB, move || read_part(&text, true, &DocxLimits::DEFAULT));
    let text = doc(&nested_textboxes(8));
    on_wasm_stack(move || read_part(&text, false, &DocxLimits::DEFAULT));
}

fn read_err(body: &str, limits: &DocxLimits) -> Option<DocxError> {
    let pkg = package();
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions { limits: limits.clone(), ..ReadOptions::default() });
    let d = parse(doc(body), &DOCX).unwrap();
    let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
    for p in d.root().first_child("w:body").unwrap().children_named("w:p") {
        if let Err(e) = rc.read_paragraph(p) {
            return Some(e);
        }
    }
    None
}

#[test]
fn hostile_field_codes_end_in_values() {
    let fb = "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>";
    let fs = "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>";
    let fe = "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>";
    let it = |s: &str| format!("<w:r><w:instrText>{s}</w:instrText></w:r>");
    let l = DocxLimits::DEFAULT;
    // nested to the limit: fine; one more: the error value
    assert!(read_err(&format!("<w:p>{}{}</w:p>", fb.repeat(64), fe.repeat(64)), &l).is_none());
    assert!(matches!(
        read_err(&format!("<w:p>{}</w:p>", fb.repeat(65)), &l),
        Some(DocxError::Limit { kind: DocxLimitKind::FieldDepth, max: 64, .. })
    ));
    // nested in the instruction at every level
    let nested = format!("{}{}{}", (fb.to_owned() + &it(" X ")).repeat(60), fs.to_owned() + "<w:r><w:t>v</w:t></w:r>", fe.repeat(60));
    assert!(read_err(&format!("<w:p>{nested}</w:p>"), &l).is_none());
    // thousands in a row, in one paragraph and across paragraphs
    let many = (fb.to_owned() + &it(" F ") + fs + "<w:r><w:t>1</w:t></w:r>" + fe).repeat(20_000);
    assert!(read_err(&format!("<w:p>{many}</w:p>"), &l).is_none());
    let across = format!("<w:p>{fb}{}{fs}</w:p>{}<w:p>{fe}</w:p>", it(" TOC "), "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(1000));
    assert!(read_err(&across, &l).is_none());
    // a huge instruction, stray separators and ends, fields that never close
    assert!(read_err(&format!("<w:p>{fb}{}{fs}{fe}</w:p>", it(&"TOC ".repeat(200_000))), &l).is_none());
    assert!(read_err(&format!("<w:p>{}{}</w:p>", fs.repeat(1000), fe.repeat(1000)), &l).is_none());
    assert!(read_err(&format!("<w:p>{}</w:p>", fb.repeat(64)), &l).is_none());
    assert!(
        read_err(&format!("<w:p>{}{}{}</w:p>", fb.repeat(30), (it("a") + fs).repeat(30), "<w:r><w:t>z</w:t></w:r>".repeat(100)), &l)
            .is_none()
    );
    // the per-story count
    let hits = DocxLimits { max_fields_per_story: 100, ..DocxLimits::DEFAULT };
    assert!(matches!(
        read_err(&format!("<w:p>{}</w:p>", (fb.to_owned() + fe).repeat(101)), &hits),
        Some(DocxError::Limit { kind: DocxLimitKind::FieldsPerStory, max: 100, .. })
    ));
}

#[test]
fn many_bookmarks_and_comment_ranges_stay_linear() {
    let marks: String = (0..30_000)
        .map(|i| format!("<w:bookmarkStart w:id=\"{i}\" w:name=\"b{i}\"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id=\"{i}\"/><w:commentRangeStart w:id=\"{i}\"/><w:commentRangeEnd w:id=\"{i}\"/>"))
        .collect();
    let started = std::time::Instant::now();
    assert!(read_err(&format!("<w:p>{marks}</w:p>"), &DocxLimits::DEFAULT).is_none());
    assert!(started.elapsed().as_secs() < 20, "bookmarks and comment ranges are not quadratic");
}

#[test]
fn unclosed_comment_ranges_are_capped() {
    let open = |n: usize| (0..n).map(|i| format!("<w:commentRangeStart w:id=\"{i}\"/><w:r><w:t>x</w:t></w:r>")).collect::<String>();
    assert!(read_err(&format!("<w:p>{}</w:p>", open(64)), &DocxLimits::DEFAULT).is_none());
    assert!(matches!(read_err(&format!("<w:p>{}</w:p>", open(65)), &DocxLimits::DEFAULT), Some(DocxError::Corrupt(_))));
    assert!(matches!(read_err(&format!("<w:p>{}</w:p>", open(30_000)), &DocxLimits::DEFAULT), Some(DocxError::Corrupt(_))));
    let same_id = "<w:commentRangeStart w:id=\"1\"/><w:r><w:t>x</w:t></w:r>".repeat(1000);
    assert!(read_err(&format!("<w:p>{same_id}</w:p>"), &DocxLimits::DEFAULT).is_none(), "a repeated id is one open range");
}

fn random_text(rng: &mut Rng, alphabet: &[&str], len: usize) -> String {
    (0..len).map(|_| alphabet[rng.below(alphabet.len())]).collect()
}

#[test]
fn tag_parsers_survive_random_input() {
    let mut rng = Rng(0x5eed_1234);
    let alphabet = [
        "%",
        "%7B",
        "%22",
        "%E2%82",
        "%C0",
        "%ED",
        "%F4",
        "{",
        "}",
        "[",
        "]",
        ":",
        ",",
        "\"",
        "\\",
        "\\u",
        "d800",
        "0",
        "1",
        "-",
        ".",
        "e",
        "E",
        "+",
        "true",
        "null",
        " ",
        "\n",
        "é",
        "😀",
        "a",
        "wordnext:inline:",
        "x:",
    ];
    for _ in 0..20_000 {
        let n = rng.below(24);
        let s = random_text(&mut rng, &alphabet, n);
        let _ = json_parse::parse(&s);
        let _ = json_parse::decode_uri_component(&s);
        let _ = parse_custom_inline_tag(Some(&s));
        let _ = parse_custom_inline_tag(Some(&format!("wordnext:inline:t:{s}")));
    }
    let big = format!("[{}]", "1,".repeat(500_000) + "1");
    assert!(json_parse::parse(&big).is_some());
    let long_str = format!("\"{}\"", "a\\u00e9".repeat(200_000));
    assert!(json_parse::parse(&long_str).is_some());
    assert!(json_parse::parse(&format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000))).is_none());
    assert!(json_parse::decode_uri_component(&"%E2%82%AC".repeat(100_000)).is_some());
}
