//! The regex sites of frontier R4c (`document.ts` `readSingleRun`, `readParagraph`,
//! `parseCustomInlineTag`) against V8: the rows of `js/regex-sites/r4c.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/r4c.inputs.txt`, expected results in
//! `tests/regex/r4c.expected`. The Rust side is the matcher each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::matchers::{find_word, newlines_to_space, replace_char};
use uo_docx::read::paragraph::{is_custom_inline_type, is_toc_instr};
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

/// The UTF-16 index of a byte offset of `s`.
fn utf16_index(s: &str, byte: usize) -> usize {
    s.get(..byte).map_or(0, |h| h.encode_utf16().count())
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "c1.toc_instr" => is_toc_instr(s).to_string(),
        "c2.toc_word" => find_word(s, "TOC", false).is_some().to_string(),
        "c2.toc_word_at" => find_word(s, "TOC", false).map_or_else(|| "null".into(), |b| utf16_index(s, b).to_string()),
        "c3.shy_to_nb" => json::string(&replace_char(s, '\u{AD}', '\u{2011}')),
        "c4.newlines" => json::string(&newlines_to_space(s)),
        "c5.inline_type" => is_custom_inline_type(s).to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn r4c_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r4c.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r4c.expected")).unwrap();
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
    assert_eq!(sites.len(), 6, "every site of r4c.mjs is replayed");
    assert_eq!(n, 6 * inputs.len());
}
