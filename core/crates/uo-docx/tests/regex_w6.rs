//! The regex sites of frontier W6 against V8: the rows of `js/regex-sites/w6.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/w6.inputs.txt`, expected results in
//! `tests/regex/w6.expected`. The Rust side is the matcher or the helper each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::write::emit::fx::{esc_attr, find_attr_span, open_tag_name, replace_wp_prefix, set_attr_in_tag, tag_prefix};
use uo_docx::write::lossless::self_close_to_open_tail;
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

fn opt(g: Option<&str>) -> String {
    g.map_or_else(|| "null".into(), json::string)
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    let sc = s.ends_with("/>");
    match id {
        "w6.set_name" => json::string(&set_attr_in_tag(s, "name", Some("a\"b<c>&"), sc)),
        "w6.set_name_inv" => json::string(&set_attr_in_tag(s, "name", Some("x"), !sc)),
        "w6.set_embed" => json::string(&set_attr_in_tag(s, "r:embed", Some("rId9"), sc)),
        "w6.set_dist" => json::string(&set_attr_in_tag(s, "distT", Some("12"), sc)),
        "w6.set_dot" => json::string(&set_attr_in_tag(s, "a.b", Some("x"), sc)),
        "w6.remove_name" => json::string(&set_attr_in_tag(s, "name", None, sc)),
        "w6.remove_embed" => json::string(&set_attr_in_tag(s, "r:embed", None, sc)),
        "w6.attr_span" => opt(find_attr_span(s, "r:embed").and_then(|(a, b)| s.get(a..b))),
        "w6.open_name" => opt(open_tag_name(s)),
        "w6.tag_prefix" => opt(tag_prefix(s)),
        "w6.self_close_open" => json::string(&self_close_to_open_tail(s)),
        "w6.wp_xx" => json::string(&replace_wp_prefix(s, "xx").unwrap()),
        "w6.wp_dollar" => json::string(&replace_wp_prefix(s, "a$b").unwrap()),
        "w6.wp_amp" => json::string(&replace_wp_prefix(s, "p$&q").unwrap()),
        "w6.wp_back" => json::string(&replace_wp_prefix(s, "p$`q").unwrap()),
        "w6.wp_quote" => json::string(&replace_wp_prefix(s, "p$'q").unwrap()),
        "w6.wp_dd" => json::string(&replace_wp_prefix(s, "$$").unwrap()),
        "w6.wp_group" => json::string(&replace_wp_prefix(s, "p$1q$2").unwrap()),
        "w6.esc_attr" => json::string(&esc_attr(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn w6_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("w6.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("w6.expected")).unwrap();
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
    println!("w6 regex: {equal} equal, {} different", bad.len());
    assert!(bad.is_empty(), "{}", bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(equal > 2_000);
}
