//! The regex sites of frontier D2b-F1 against V8: the rows of `js/regex-sites/d2b-f1.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d2b-f1.inputs.txt`, expected results in
//! `tests/regex/d2b-f1.expected`. The Rust side is the hand matcher each site became
//! (`plainTextSdt` of source.ts, the per-character tests of `caseMapped`, `colorPick`'s `#`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::paragraph::{plain_text_sdt, sdt_kind_matches, sdt_open_matches};
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

fn run(id: &str, s: &str) -> String {
    match id {
        "d2b-f1.sdt_open" => sdt_open_matches(s).to_string(),
        "d2b-f1.sdt_kind" => sdt_kind_matches(s).to_string(),
        "d2b-f1.plain_text_sdt" => plain_text_sdt(Some(s)).to_string(),
        "d2b-f1.space" => s.encode_utf16().any(uo_text::space::is_space).to_string(),
        "d2b-f1.digit" => s.chars().any(|c| c.is_ascii_digit()).to_string(),
        "d2b-f1.sentence_end" => s.chars().any(|c| matches!(c, '.' | '!' | '?')).to_string(),
        "d2b-f1.strip_hash" => json::string(s.strip_prefix('#').unwrap_or(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d2b_f1_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("d2b-f1.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("d2b-f1.expected")).unwrap();
    let (mut equal, mut bad) = (0usize, Vec::new());
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, idx, want) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        let input = &inputs[idx.parse::<usize>().unwrap()];
        let got = run(id, input);
        if got == want {
            equal += 1;
        } else {
            bad.push(format!("{id} #{idx} {input:?}\n  v8:   {want}\n  rust: {got}"));
        }
    }
    println!("d2b-f1 regex: {equal} equal, {} different", bad.len());
    assert!(bad.is_empty(), "{}", bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(equal > 500);
}
