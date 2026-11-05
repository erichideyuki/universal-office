//! The regex sites of the diff (D5a) against V8: the rows of `js/regex-sites/d5a-compare.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d5a-compare.inputs.txt`, expected results in
//! `tests/regex/d5a-compare.expected`. The Rust side is the matcher each site became
//! (`compare::matchers`).
#![cfg(feature = "compare")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::compare::matchers::{has_non_space, word_tokens};

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

fn run(id: &str, s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    match id {
        "d5a.word_tokens" => {
            let items: Vec<String> = word_tokens(&u).iter().map(|(a, b)| format!("[{a},{b}]")).collect();
            format!("[{}]", items.join(","))
        }
        "d5a.has_word" => has_non_space(&u).to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d5a_compare_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d5a-compare.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d5a-compare.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on input #{i} {input:?}");
        n += 1;
    }
    assert!(n > 80);
}
