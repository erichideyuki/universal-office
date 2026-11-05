//! The regex sites of D2b-F6 against V8: the rows of `js/regex-sites/d2b-f6.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d2b-f6.inputs.txt`, expected results in
//! `tests/regex/d2b-f6.expected`. The Rust side is the matcher each site became
//! (`session::ops::bookmarks`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::bookmarks::{bookmark_start_ids, is_bookmark_part, para_of_item_number, plain_text_sdt, valid_bookmark_name};

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
        "d2b-f6.bookmark_name" => valid_bookmark_name(s).to_string(),
        "d2b-f6.bookmark_part" => is_bookmark_part(s).to_string(),
        "d2b-f6.bookmark_start_ids" => {
            let ids: Vec<String> = bookmark_start_ids(s).iter().map(|d| uo_jsnum::json::string(d)).collect();
            uo_jsnum::json::string(&format!("[{}]", ids.join(",")))
        }
        "d2b-f6.para_of_item" => uo_jsnum::json::string(&para_of_item_number(s).map_or("-1".to_owned(), uo_jsnum::format::to_js_string)),
        "d2b-f6.plain_text_sdt" => plain_text_sdt(s).to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d2b_f6_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f6.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f6.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on input #{i} {input:?}");
        n += 1;
    }
    assert!(n > 800);
}
