//! No panic in the D1b frontier W1 (`write::emit::run`) on hostile input: the synthetic packages and
//! corpus documents with their main part mutated (bytes flipped, cut, tags duplicated, `>` removed,
//! attributes doubled), read with and without sources and saved in the generating variants (`gen`,
//! `actr`, `actp` of `tests/parity_runs_w1.rs`); every save that succeeds must reopen. The emitters
//! finish in linear time on hostile texts and models (a long text, a field code of many groups or
//! deep nesting, two hundred thousand runs after a hyperlink, as many revision wrappers).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use uo_docx::js::Str;
use uo_docx::model::Document;
use uo_docx::model::props::RevisionInfo;
use uo_docx::model::run::{BookmarkMark, Run, RunProps};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::ctx::{DEFAULT_NUM_IDS, Lossless, WriteCtx, Writer};
use uo_docx::write::emit::run::{GenItem, field_code_xml, gen_runs, revision_runs, text_xml, with_no_proof};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::{Baseline, Pristine, SigMode};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::generate::Rng;
use uo_testkit::repo_root;

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

type Input = (String, Vec<(String, Vec<u8>)>);

fn synthetic_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/runs.inputs")).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t') {
            out.last_mut().unwrap().1.push((path.to_owned(), unescape(data).into_bytes()));
        }
    }
    out
}

fn from_parts(parts: &[(String, Vec<u8>)]) -> Package {
    const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>";
    let base: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("_rels/.rels".into(), RELS.as_bytes().to_vec()),
        ("word/document.xml".into(), DOC.as_bytes().to_vec()),
    ];
    let mut pkg = Package::from_parts(base, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    for (p, d) in parts {
        pkg.set_part(p.clone(), d.clone());
    }
    pkg
}

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "docx") {
                out.push(p);
            }
        }
    }
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("write/packages/docx-layout/test/oracle"), &mut out);
    walk(&root.join("write/packages/docx-core/test/fixtures"), &mut out);
    out
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut s: Vec<u8> = text.as_bytes().to_vec();
    for _ in 0..1 + rng.below(4) {
        if s.is_empty() {
            break;
        }
        let at = rng.below(s.len());
        match rng.below(8) {
            0 => s[at] = b"<>/\"='&w: x"[rng.below(11)],
            1 => s.truncate(at),
            2 => {
                let end = (at + 1 + rng.below(200)).min(s.len());
                let dup = s[at..end].to_vec();
                s.splice(at..at, dup);
            }
            3 => {
                if let Some(k) = s[at..].iter().position(|&b| b == b'>') {
                    s.remove(at + k);
                }
            }
            4 => {
                let ins: &[u8] = [
                    &b" w:id=\"1\""[..],
                    b"<w:fldChar w:fldCharType=\"begin\"/>",
                    b"<w:instrText> { { } } </w:instrText>",
                    b"<w:ins w:id=\"1\" w:author=\"a\">",
                    b"</w:ins>",
                    b"$&",
                    b"\xF0\x9F\x98\x80",
                ][rng.below(7)];
                s.splice(at..at, ins.iter().copied());
            }
            5 => {
                let end = (at + rng.below(64)).min(s.len());
                s.drain(at..end);
            }
            6 => {
                let ins: &[u8] =
                    [&b"<w:hyperlink r:id=\"x\">"[..], b"</w:hyperlink>", b"<w:sdt>", b"<w:tag w:val=\"wordnext:inline:a:%E0%A4%A\"/>"]
                        [rng.below(4)];
                s.splice(at..at, ins.iter().copied());
            }
            _ => s[at] = s[at].wrapping_add(1),
        }
    }
    String::from_utf8_lossy(&s).into_owned()
}

fn round_trip(mut pkg: Package, variant: &str, mode: SigMode) -> Option<Result<(), String>> {
    let sources = variant != "gen";
    let mut doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() }).ok()?;
    let mut state = install(&mut doc, mode);
    if sources {
        for pi in 0..doc.body.paras.len() {
            if let Some(p) = doc.body.paras.make_mut(pi) {
                for r in &mut p.runs {
                    Arc::make_mut(&mut r.props).src = None;
                }
                if variant == "actp" {
                    Arc::make_mut(&mut p.props).src = None;
                }
            }
        }
    }
    if let Err(e) = save(&mut pkg, &doc, &mut state, DirtyHf::None) {
        return Some(Err(e.to_string()));
    }
    let out = pkg.save(&SaveOptions::default()).unwrap();
    Package::open(&out, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
    Some(Ok(()))
}

#[test]
fn hostile_generated_saves_do_not_panic() {
    let synthetic = synthetic_inputs();
    let files = corpus();
    let mut rng = Rng::new(0x57a1_bad5);
    let (mut saved, mut refused) = (0, 0);
    let mut errors = std::collections::BTreeMap::new();
    for i in 0..1500 {
        let pkg = if i % 3 == 0 {
            let bytes = std::fs::read(&files[rng.below(files.len())]).unwrap();
            let Ok(mut pkg) = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()) else { continue };
            if let Ok(Some(t)) = pkg.part_text("word/document.xml").map(|t| t.map(std::borrow::Cow::into_owned)) {
                let m = mutate(&mut rng, &t);
                pkg.set_part_text("word/document.xml", m);
            }
            pkg
        } else {
            let (_, parts) = &synthetic[rng.below(synthetic.len())];
            let mut parts = parts.clone();
            for (path, data) in &mut parts {
                if path == "word/document.xml" && rng.below(2) == 0 {
                    *data = mutate(&mut rng, &String::from_utf8_lossy(data)).into_bytes();
                }
            }
            from_parts(&parts)
        };
        let variant = ["gen", "actr", "actp"][rng.below(3)];
        match round_trip(pkg, variant, if i % 2 == 0 { SigMode::Canonical } else { SigMode::Ts }) {
            Some(Ok(())) => saved += 1,
            Some(Err(e)) => {
                *errors.entry(e).or_insert(0usize) += 1;
                refused += 1;
            }
            None => refused += 1,
        }
    }
    println!("no panic: {saved} hostile saves reopened, {refused} refused (read or write error)");
    println!("write errors: {errors:?}");
    assert!(saved > 400);
}

#[test]
fn emitters_are_linear_on_hostile_texts() {
    let big = |unit: &str, n: usize| unit.repeat(n);
    let t = Instant::now();
    let tabs = big("a\tb\n  ", 400_000);
    assert!(text_xml(&tabs, false).len() > tabs.len());
    assert!(t.elapsed().as_secs() < 5, "text_xml took {:?}", t.elapsed());
    let t = Instant::now();
    for soup in [big("<w:", 300_000), big("<w:rPr><w:b/>", 200_000), big("<w:noProofX", 200_000) + "</w:rPr>"] {
        let _ = with_no_proof(&soup);
    }
    assert!(t.elapsed().as_secs() < 5, "with_no_proof took {:?}", t.elapsed());
    // a field code of many groups against a long flat instruction (V8 is O(n^k) here), and deep nesting
    let t = Instant::now();
    let code = big("a{x}", 2_000) + "b";
    let flat = big("a1", 2_000) + "b";
    let x = field_code_xml(&code, Some(&flat), "<w:rPr/>", "w:instrText").unwrap();
    assert!(x.contains("w:fldCharType=\"separate\""));
    let deep = format!("{}x{}", "{".repeat(100_000), "}".repeat(100_000));
    assert!(field_code_xml(&deep, None, "<w:rPr/>", "w:instrText").is_err());
    assert!(t.elapsed().as_secs() < 10, "field_code_xml took {:?}", t.elapsed());
}

fn rev(author: &str) -> Box<RevisionInfo> {
    Box::new(RevisionInfo { author: Str::from(author), ..RevisionInfo::default() })
}

#[test]
fn run_lists_are_linear() {
    let doc = Document::default();
    let mut pkg = from_parts(&[]);
    let mut fresh = 0.0;
    let mut w = Writer {
        doc: &doc,
        pkg: &mut pkg,
        cx: WriteCtx::new(uo_opc::Limits::DEFAULT, SigMode::Canonical),
        fresh_id: &mut fresh,
        lossless: None,
    };
    let n = 200_000;
    // inserted runs, all by one author: one wrapper; alternating authors: n wrappers
    let ins = |author: &str| Arc::new(RunProps { ins: Some(rev(author)), ..RunProps::default() });
    let one = ins("a");
    let two = ins("b");
    let same: Vec<Run> = (0..n).map(|_| Run { text: "x".into(), props: Arc::clone(&one), ..Run::default() }).collect();
    let alternating: Vec<Run> =
        (0..n).map(|i| Run { text: "x".into(), props: Arc::clone(if i % 2 == 0 { &one } else { &two }), ..Run::default() }).collect();
    for runs in [&same, &alternating] {
        let items: Vec<GenItem<'_>> = runs.iter().map(|run| GenItem { run, before: String::new(), after: String::new() }).collect();
        let t = Instant::now();
        let x = revision_runs(&mut w, &items).unwrap();
        assert!(x.contains("<w:ins "));
        assert!(t.elapsed().as_secs() < 10, "revision_runs took {:?}", t.elapsed());
    }
    // a hyperlink followed by many bookmark marks and then text: the look-ahead of the group is one scan
    let link_props = Arc::new(RunProps { link: Some(Str::from("#a")), ..RunProps::default() });
    let mut marks: Vec<Run> = vec![Run { text: "l".into(), props: Arc::clone(&link_props), ..Run::default() }];
    for i in 0..n {
        let mut m = Run::default();
        m.ext_mut().bookmark =
            Some(BookmarkMark { id: uo_docx::js::JsNumber::Num(f64::from(i)), name: Str::from("b"), ..BookmarkMark::default() });
        marks.push(m);
    }
    marks.push(Run { text: "t".into(), ..Run::default() });
    let items: Vec<GenItem<'_>> = marks.iter().map(|run| GenItem { run, before: String::new(), after: String::new() }).collect();
    // active: the hyperlinks are grouped only in the lossless save
    let baseline = Baseline::default();
    let store = uo_docx::model::sources::SourceStore::default();
    let mut w2 = Writer {
        doc: &doc,
        pkg: &mut pkg,
        cx: WriteCtx::new(uo_opc::Limits::DEFAULT, SigMode::Canonical),
        fresh_id: &mut fresh,
        lossless: Some(Lossless {
            store: &store,
            pristine: Pristine::new(&baseline, Some(&doc.media)),
            rels_path: "word/_rels/document.xml.rels".to_owned(),
            claimed: Default::default(),
            gen_numbering: false,
            num_ids: DEFAULT_NUM_IDS,
            claimed_segs: Default::default(),
            remaining: Default::default(),
            para_runs: None,
            emitted_obj: Default::default(),
        }),
    };
    let t = Instant::now();
    let x = gen_runs(&mut w2, &items, false).unwrap();
    assert!(x.contains("<w:hyperlink"));
    assert!(t.elapsed().as_secs() < 10, "gen_runs took {:?}", t.elapsed());
}
