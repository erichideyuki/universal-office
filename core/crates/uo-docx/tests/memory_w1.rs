//! Memory gate of the D1b frontier W1 (`write::emit::run`): M1 — every public function returns the
//! live bytes exactly, outputs dropped — over the paragraphs and runs of synthetic packages
//! (`tests/parity/runs.inputs`) and corpus documents, read with and without sources (the writer
//! inactive and active), and over the error paths: a custom inline with an invalid type and a field
//! code nested past the bound. M3: the generated XML of a large paragraph set against its size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::path::{Path, PathBuf};

use uo_alloc::{assert_no_leak, measure};
use uo_docx::error::DocxError;
use uo_docx::model::Document;
use uo_docx::model::Paragraph;
use uo_docx::model::run::{CustomInline, Run};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::ctx::{DEFAULT_NUM_IDS, Lossless, WriteCtx, Writer};
use uo_docx::write::emit::run::{
    GenItem, comment_marks, encode_uri_component, field_code_xml, field_or_run_xml, gen_runs, has_h_switch, is_formula, is_mark_run,
    is_marker_field, is_note_ref_mark, para_xml_inner, revision_runs, run_xml, sect_pr_inner, text_xml, with_no_proof,
};
use uo_docx::write::sig::{Baseline, Pristine, SigMode};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
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

/// The state a writer borrows: a fresh package, the `freshId` counter and the baseline.
struct Rig {
    pkg: Package,
    fresh: f64,
    baseline: Baseline,
}

impl Rig {
    fn new() -> Rig {
        Rig { pkg: from_parts(&[]), fresh: 0.0, baseline: Baseline::default() }
    }

    /// A writer over `doc`, lossless when the document was read with sources and `active` is asked for.
    fn writer<'a>(&'a mut self, doc: &'a Document, active: bool) -> Writer<'a> {
        let lossless = if active {
            doc.sources.as_deref().map(|store| Lossless {
                store,
                pristine: Pristine::new(&self.baseline, Some(&doc.media)),
                rels_path: "word/_rels/document.xml.rels".to_owned(),
                claimed: HashSet::new(),
                gen_numbering: false,
                num_ids: DEFAULT_NUM_IDS,
                claimed_segs: HashSet::new(),
                remaining: HashMap::new(),
                para_runs: None,
                emitted_obj: HashSet::new(),
            })
        } else {
            None
        };
        Writer {
            doc,
            pkg: &mut self.pkg,
            cx: WriteCtx::new(uo_opc::Limits::DEFAULT, SigMode::Canonical),
            fresh_id: &mut self.fresh,
            lossless,
        }
    }
}

/// Every public emitter over one paragraph (results dropped; stubs of other frontiers are errors).
fn exercise<'a>(w: &mut Writer<'a>, p: &'a Paragraph) {
    let (before, after) = comment_marks(&p.runs, None);
    let items: Vec<GenItem<'a>> =
        p.runs.iter().zip(before).zip(after).map(|((run, before), after)| GenItem { run, before, after }).collect();
    black_box(revision_runs(w, &items).is_ok());
    black_box(gen_runs(w, &items, false).is_ok());
    black_box(gen_runs(w, &items, true).is_ok());
    black_box(para_xml_inner(w, &p.props, "<w:r/>").is_ok());
    for r in &p.runs {
        black_box(is_mark_run(r));
        black_box(run_xml(w, r).is_ok());
        black_box(field_or_run_xml(w, r).is_ok());
    }
}

fn docs(sources: bool, step: usize) -> Vec<Document> {
    let mut out = Vec::new();
    for (_, parts) in synthetic_inputs().iter().step_by(step) {
        let pkg = from_parts(parts);
        out.push(read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() }).unwrap());
    }
    for f in corpus().iter().step_by(step * 15) {
        let bytes = std::fs::read(f).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        if let Ok(d) = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() }) {
            out.push(d);
        }
    }
    out
}

#[test]
fn m1_emitters_return_the_memory() {
    let mut n = 0;
    for (sources, active) in [(false, false), (true, true), (true, false)] {
        for doc in docs(sources, 6) {
            assert_no_leak(|| {
                let mut rig = Rig::new();
                let mut w = rig.writer(&doc, active);
                for p in doc.body.paras.iter() {
                    exercise(&mut w, p);
                }
                for s in doc.sections.iter() {
                    black_box(sect_pr_inner(&w.cx, &s.sect_pr, &[]).is_ok());
                }
            });
            n += 1;
        }
    }
    println!("M1: {n} documents, 0 B left each");
    assert!(n > 60);
}

#[test]
fn m1_string_functions_return_the_memory() {
    assert_no_leak(|| black_box(text_xml("a\tb\nc\u{c}d\u{b}e\u{2011}f\u{ad}g\u{2028}h  ", true)));
    assert_no_leak(|| black_box(text_xml("", false)));
    assert_no_leak(|| black_box(encode_uri_component("{\"a\":\"é 😀\"}")));
    assert_no_leak(|| black_box(with_no_proof("<w:rPr><w:b/><w:sz w:val=\"2\"/></w:rPr>")));
    assert_no_leak(|| black_box(with_no_proof("<w:rPr/>")));
    assert_no_leak(|| black_box(with_no_proof("<w:rPr>😀")));
    assert_no_leak(|| {
        black_box(is_note_ref_mark(" FOOTNOTEREF_MARK "));
        black_box(is_formula(" = 1"));
        black_box(is_marker_field("TC \"a\""));
        black_box(has_h_switch("REF a \\h"));
    });
    assert_no_leak(|| {
        black_box(field_code_xml(" IF { PAGE } = { NUMPAGES } \"x\" ", Some(" IF 3 = 4 \"x\" "), "<w:rPr/>", "w:instrText").unwrap())
    });
    assert_no_leak(|| black_box(field_code_xml(" A { B { C } } ", None, "", "w:delInstrText").unwrap()));
}

#[test]
fn m1_error_paths_return_the_memory() {
    let doc = Document::default();
    // a custom inline with an invalid type
    let mut bad = Run { text: "x".into(), ..Run::default() };
    bad.ext_mut().custom = Some(Box::new(CustomInline { r#type: "bad type".into(), ..CustomInline::default() }));
    assert_no_leak(|| {
        let mut rig = Rig::new();
        let r = run_xml(&mut rig.writer(&doc, false), &bad);
        assert!(matches!(r, Err(DocxError::Write("invalid custom inline type"))), "{r:?}");
    });
    let mut empty = Run { text: "x".into(), ..Run::default() };
    empty.ext_mut().custom = Some(Box::new(CustomInline { r#type: String::new().into(), ..CustomInline::default() }));
    assert_no_leak(|| {
        let mut rig = Rig::new();
        black_box(run_xml(&mut rig.writer(&doc, false), &empty).is_err())
    });
    // a field code nested past the bound
    let deep = format!("{}x{}", "{".repeat(600), "}".repeat(600));
    assert_no_leak(|| {
        let r = field_code_xml(&deep, None, "<w:rPr/>", "w:instrText");
        assert!(matches!(r, Err(DocxError::Write("field code nested too deeply"))), "{r:?}");
    });
    let mut field = Run { text: "x".into(), ..Run::default() };
    field.ext_mut().field = Some(" IF ".into());
    field.ext_mut().field_code = Some(deep.clone().into());
    assert_no_leak(|| {
        let mut rig = Rig::new();
        black_box(field_or_run_xml(&mut rig.writer(&doc, false), &field).is_err())
    });
}

/// M3: the XML generated for the paragraphs of the synthetic and corpus documents against its size (the
/// packages are built outside the measure). Budget: allocated ≤ 12 × the XML generated + 64 KiB (measured 10.8 ×, of which the
/// core's `rpr_xml` is about a third).
#[test]
fn m3_generation_budget() {
    let all = docs(false, 3);
    let mut rigs: Vec<Rig> = all.iter().map(|_| Rig::new()).collect();
    let mut xml = 0usize;
    let ((), u) = measure(|| {
        for (doc, rig) in all.iter().zip(rigs.iter_mut()) {
            let mut w = rig.writer(doc, false);
            for p in doc.body.paras.iter() {
                let (before, after) = comment_marks(&p.runs, None);
                let items: Vec<GenItem<'_>> =
                    p.runs.iter().zip(before).zip(after).map(|((run, before), after)| GenItem { run, before, after }).collect();
                if let Ok(runs) = revision_runs(&mut w, &items)
                    && let Ok(x) = para_xml_inner(&mut w, &p.props, &runs)
                {
                    xml += x.len();
                }
            }
        }
    });
    println!(
        "M3 W1: {} documents, {xml} B of paragraph XML, allocated {} B ({:.2}x), peak {} B",
        all.len(),
        u.allocated,
        u.allocated as f64 / xml as f64,
        u.peak_above_start
    );
    assert!(xml > 10_000);
    assert!(u.allocated <= 12 * xml as u64 + (64 << 10), "{} B allocated for {xml} B", u.allocated);
}
