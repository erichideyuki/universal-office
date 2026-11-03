//! The regex-site templates of `read::matchers` (G9) against V8 `RegExp`: the rows of
//! `js/regex-sites/templates.mjs` (run by `js/regex-diff.mjs`) over
//! `tests/regex/templates.inputs.txt`, expected results in `tests/regex/templates.expected`. A
//! frontier adds its own site file and test (`js/regex-sites/r1.mjs`, `tests/regex_r1.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::matchers::*;
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

/// A byte offset as the UTF-16 index JS reports.
fn idx(s: &str, at: Option<usize>) -> String {
    at.map_or_else(|| "null".into(), |a| s[..a].encode_utf16().count().to_string())
}

fn group(g: Option<&str>) -> String {
    g.map_or_else(|| "null".into(), json::string)
}

/// The Rust template of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "t1.one_of" => is_one_of(s, &["1", "true"]).to_string(),
        "t2.word" => idx(s, find_word(s, "TOC", false)),
        "t2.word_ci" => idx(s, find_word(s, "TOC", true)),
        "t2.lit_boundary" => idx(s, find_lit_then_boundary(s, "<a:blipFill")),
        "t4.non_digits" => json::string(&keep_ascii_digits(s)),
        "t4.all_in" => all_in(s, |c| c.is_ascii_alphanumeric() || "._-".contains(c)).to_string(),
        "t5.toc_prefix" => {
            let mut c = Cur::new(s);
            c.spaces();
            (c.lit_ci("TOC") && c.boundary()).to_string()
        }
        "t5.val_int" => {
            let mut c = Cur::new(s);
            let g = if c.lit("val") && c.spaces1() {
                let start = c.pos();
                c.opt('-');
                c.digits1().map(|_| &s[start..c.pos()])
            } else {
                None
            };
            group(g)
        }
        "t6.shy" => json::string(&replace_char(s, '\u{ad}', '\u{2011}')),
        "t6.newlines" => json::string(&newlines_to_space(s)),
        "t6.self_closing" => json::string(&self_closing_to_open(s)),
        "t7.element_text" => group(element_text(s, "dcterms:created")),
        _ => panic!("no Rust template for site {id}"),
    }
}

#[test]
fn templates_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}templates.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}templates.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on {input:?}");
        n += 1;
    }
    assert_eq!(n, 12 * inputs.len());
}
