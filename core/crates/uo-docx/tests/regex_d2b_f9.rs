//! The regex sites of frontier D2b-F9 against V8: the rows of `js/regex-sites/d2b-f9.mjs` (run by
//! `js/regex-diff.mjs --only d2b-f9`) over `tests/regex/d2b-f9.inputs.txt`, expected results in
//! `tests/regex/d2b-f9.expected`. The Rust side is the hand matcher each site became
//! (`session::ops::find::regex_site`). The inputs file is UTF-8, so an input with a lone surrogate
//! reads U+FFFD: those rows are skipped here (the lone surrogates of the find engine are compared by
//! the ops capture, whose queries carry UTF-16 in hex).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::find::regex_site;

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

#[test]
fn d2b_f9_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("d2b-f9.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("d2b-f9.expected")).unwrap();
    let (mut equal, mut skipped, mut bad) = (0usize, 0usize, Vec::new());
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, idx, want) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        let input = &inputs[idx.parse::<usize>().unwrap()];
        if input.contains('\u{FFFD}') {
            skipped += 1;
            continue;
        }
        let units: Vec<u16> = input.encode_utf16().collect();
        let got = uo_jsnum::json::string(&regex_site(id, &units).unwrap_or_else(|| panic!("no Rust matcher for site {id}")));
        if got == want {
            equal += 1;
        } else {
            bad.push(format!("{id} #{idx} {input:?}\n  v8:   {want}\n  rust: {got}"));
        }
    }
    println!("d2b-f9 regex: {equal} equal, {} different, {skipped} skipped (lone surrogates)", bad.len());
    assert!(bad.is_empty(), "{}", bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(equal > 3000);
}
