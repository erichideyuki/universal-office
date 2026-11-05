//! The regex sites of frontier W5 against V8: the rows of `js/regex-sites/w5.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/w5.inputs.txt`, expected results in
//! `tests/regex/w5.expected`. The Rust side is the matcher or the helper each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::write::emit::background::{
    add_default_extension, esc_pb, fill_color2_match, first_open_tag, first_r_id, frac_attr, letters_len, pct_attr, rels_path_of,
    remove_relationship, strip_trailing_digits,
};
use uo_docx::write::matchers::element_matches;
use uo_jsnum::json;
use uo_jsnum::parse::to_number;

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

fn opt(g: Option<&str>) -> String {
    g.map_or_else(|| "null".into(), json::string)
}

fn tag_match(s: &str, name: &str) -> String {
    opt(first_open_tag(s, name).and_then(|(a, b)| s.get(a..b)))
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "w5.rels_path" => json::string(&rels_path_of(s)),
        "w5.first_rid" => opt(first_r_id(s)),
        "w5.types_open" => tag_match(s, "Types"),
        "w5.document_open" => tag_match(s, "w:document"),
        "w5.background_el" => opt(element_matches(s, "w:background").first().and_then(|&(a, b)| s.get(a..b))),
        "w5.add_ext_png" => json::string(&add_default_extension(s, "png")),
        "w5.add_ext_amp" => json::string(&add_default_extension(s, "a$&b")),
        "w5.add_ext_back" => json::string(&add_default_extension(s, "a$`b")),
        "w5.add_ext_quote" => json::string(&add_default_extension(s, "a$'b")),
        "w5.add_ext_group" => json::string(&add_default_extension(s, "a$1b$2c$10d$01e$0")),
        "w5.add_ext_dollar" => json::string(&add_default_extension(s, "$$")),
        "w5.color2" => match fill_color2_match(s) {
            None => "null".to_owned(),
            Some(None) => "[null,null]".to_owned(),
            Some(Some((w, d))) => format!("[{},{}]", json::string(w), json::string(d)),
        },
        "w5.trailing_digits" => json::string(strip_trailing_digits(s)),
        "w5.letters_len" | "w5.letters_len_s" => uo_jsnum::format::to_js_string(letters_len(s)),
        "w5.frac_attr" => json::string(&frac_attr(to_number(s))),
        "w5.pct_attr" => json::string(&pct_attr(to_number(s))),
        "w5.esc_attr" => json::string(&esc_pb(s)),
        "w5.remove_rel_5" => json::string(&remove_relationship(s, "rId5")),
        "w5.remove_rel_12" => json::string(&remove_relationship(s, "rId12")),
        "w5.contains_id" => s.contains("Id=\"rId5\"").to_string(),
        "w5.contains_quoted" => s.contains("\"rId5\"").to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn w5_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("w5.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("w5.expected")).unwrap();
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
    println!("w5 regex: {equal} equal, {} different", bad.len());
    assert!(bad.is_empty(), "{}", bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(equal > 2_000);
}
