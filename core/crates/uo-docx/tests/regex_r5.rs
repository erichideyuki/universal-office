//! The regex sites of frontier R5 (`fulldoc.ts` `readCoreProps`, `blank.ts` `LANG_TAG`, `inspect.ts`
//! `runHasContent`) against V8: the rows of `js/regex-sites/r5.mjs` (run by `js/regex-diff.mjs`) over
//! `tests/regex/r5.inputs.txt`, expected results in `tests/regex/r5.expected`. The Rust side is the
//! matcher each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::blank::is_language_tag;
use uo_docx::read::inspect::units_have_text;
use uo_docx::read::matchers::element_text;
use uo_jsnum::json;

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

fn group(s: &str, tag: &str) -> String {
    element_text(s, tag).map_or_else(|| "null".into(), json::string)
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "r1.core_created" => group(s, "dcterms:created"),
        "r1.core_modified" => group(s, "dcterms:modified"),
        "r1.core_printed" => group(s, "cp:lastPrinted"),
        "r2.lang_tag" => is_language_tag(Some(s)).to_string(),
        "r3.run_has_text" => units_have_text(&s.encode_utf16().collect::<Vec<u16>>()).to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn r5_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r5.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r5.expected")).unwrap();
    let mut n = 0;
    let mut sites = std::collections::BTreeSet::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on {input:?}");
        sites.insert(id.to_string());
        n += 1;
    }
    assert_eq!(sites.len(), 5, "every site of r5.mjs is replayed");
    assert_eq!(n, 5 * inputs.len());
}
