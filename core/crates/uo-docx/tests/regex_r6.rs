//! The regex sites of frontier R6 against V8: the rows of `js/regex-sites/r6.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/r6.inputs.txt`, expected results in
//! `tests/regex/r6.expected`. Both sites are `.replace(/\s*\/>$/, '>')` over the text of a tag:
//! `openOf` of `readTable` (`read::sources::open_of`) and the paragraph's `open` of
//! `captureParagraph`. Where the input is exactly one self-closing element the real function runs
//! on it (the element parsed with positions, the result resolved against the store); the other
//! inputs (tags cut from the source, text no parser accepts) run the matcher the functions use,
//! `matchers::self_closing_to_open`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::model::Paragraph;
use uo_docx::model::sources::SourceStore;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::matchers::self_closing_to_open;
use uo_docx::read::sources::{capture_paragraph, open_of, text_of};
use uo_docx::read::{DOCX_POS, ReadOptions};
use uo_jsnum::json;
use uo_opc::xml::{Document, parse};

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

/// The input as one self-closing element, parsed and registered in a store, when it is one.
fn as_element(s: &str) -> Option<(Document, SourceStore)> {
    let doc = parse(s.to_owned(), &DOCX_POS).ok()?;
    let p = doc.root().positions()?;
    if p.start != 0 || p.end != s.len() || p.content_start != p.end {
        return None;
    }
    let pkg = uo_opc::Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
    ctx.source_part("word/document.xml", &doc)?;
    let store = ctx.sources.take()?;
    Some((doc, store))
}

fn run(id: &str, s: &str) -> (String, bool) {
    let element = as_element(s);
    let real = element.is_some();
    let out = match (id, element) {
        ("s1.open_of", Some((doc, store))) => text_of(&store, &open_of(&store, 0, doc.root())).to_string(),
        ("s2.para_open", Some((doc, mut store))) => {
            capture_paragraph(&mut store, 0, &mut Paragraph::default(), doc.root(), None, Vec::new(), &[]);
            let open = store.paras.values().next().unwrap().open.clone();
            text_of(&store, &open).to_string()
        }
        ("s1.open_of" | "s2.para_open", None) => self_closing_to_open(s).into_owned(),
        _ => panic!("no Rust matcher for site {id}"),
    };
    (json::string(&out), real)
}

#[test]
fn r6_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r6.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r6.expected")).unwrap();
    let (mut n, mut through_the_function) = (0, 0);
    let mut sites = std::collections::BTreeSet::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        let (got, real) = run(id, input);
        assert_eq!(got, want, "site {id} on {input:?}");
        through_the_function += usize::from(real);
        sites.insert(id.to_string());
        n += 1;
    }
    assert_eq!(sites.len(), 2, "every site of r6.mjs is replayed");
    assert_eq!(n, 2 * inputs.len());
    println!("{n} results, {through_the_function} through the real function");
    assert!(through_the_function >= 20, "enough inputs are parsed as an element to exercise the functions");
}
