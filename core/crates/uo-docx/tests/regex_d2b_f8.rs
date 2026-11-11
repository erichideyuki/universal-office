//! The regex sites of D2b-F8 against V8: the rows of `js/regex-sites/d2b-f8.mjs` (run by
//! `js/regex-diff.mjs --only d2b-f8`) over `tests/regex/d2b-f8.inputs.txt`, expected results in
//! `tests/regex/d2b-f8.expected`. The Rust side is the matcher each site became
//! (`session::ops::clipboard::rx`, and `importAbstract`'s in `lists::store`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::clipboard::{rx, testing};
use uo_jsnum::json::string as js;

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

fn st(u: &[u16]) -> String {
    js(&String::from_utf16_lossy(u))
}

fn b(x: bool) -> String {
    x.to_string()
}

fn opt(s: Option<String>) -> String {
    s.map_or_else(|| "null".to_owned(), |x| js(&x))
}

fn arr(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

fn js_trim(u: &[u16]) -> &[u16] {
    let sp = uo_jsnum::text::is_js_space_unit;
    let a = u.iter().position(|c| !sp(*c)).unwrap_or(u.len());
    let e = u.iter().rposition(|c| !sp(*c)).map_or(a, |k| k + 1);
    &u[a..e.max(a)]
}

fn run(id: &str, s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    if let Some(ilvl) = id.strip_prefix("d2b-f8.lvl_") {
        let ilvl = if ilvl == "-0" { "0" } else { ilvl };
        return opt(rx::level_num_fmt(s, ilvl));
    }
    match id {
        "d2b-f8.nl" => st(&rx::normalize_newlines(&u)),
        "d2b-f8.lines" => js(&arr(&rx::split_lines(&u).iter().map(|l| st(l)).collect::<Vec<_>>())),
        "d2b-f8.blank" => b(rx::is_blank(&u)),
        "d2b-f8.table_tag" => b(rx::has_table_tag(&u)),
        "d2b-f8.strip_tables" => st(&rx::strip_tables(&u)),
        "d2b-f8.block_tag" => b(rx::has_block_tag(&u)),
        "d2b-f8.starts_word" => b(rx::starts_word(&u)),
        "d2b-f8.ends_word" => b(rx::ends_word(&u)),
        "d2b-f8.ends_word_punct" => b(rx::ends_word_or_punct(&u)),
        "d2b-f8.doc_pr_name" => js(&rx::renumber_doc_pr_name(s, "77")),
        "d2b-f8.backslashes" => js(&rx::double_backslashes(s)),
        "d2b-f8.esc" => st(&rx::esc_html(&u)),
        "d2b-f8.nl_space" => st(&rx::newlines_to_spaces(&u)),
        "d2b-f8.squash" => st(&rx::squash_trim(&u)),
        "d2b-f8.hex3" => rx::hex3(&u).map_or_else(|| "null".to_owned(), st),
        "d2b-f8.hex6" => rx::hex6(&u).map_or_else(|| "null".to_owned(), st),
        "d2b-f8.rgba" => match rx::rgba(&u) {
            Some((n, a)) => js(&arr(&[st(&n[0]), st(&n[1]), st(&n[2]), a.map_or_else(|| "null".to_owned(), |a| st(&a))])),
            None => "null".to_owned(),
        },
        "d2b-f8.size_unit" => match rx::size_unit(&u) {
            Some((num, pt)) => js(&arr(&[st(num), js(if pt { "pt" } else { "px" })])),
            None => "null".to_owned(),
        },
        "d2b-f8.quotes" => st(rx::strip_quotes(js_trim(&u))),
        "d2b-f8.generic" => b(rx::generic_family(&u)),
        "d2b-f8.javascript" => b(rx::is_javascript(&u)),
        "d2b-f8.data_url" => match rx::data_url(&u) {
            Some((t, p)) => js(&arr(&[st(t), st(p)])),
            None => "null".to_owned(),
        },
        "d2b-f8.no_space" => st(&rx::remove_spaces(&u)),
        "d2b-f8.list_defs" => js(&arr(&rx::word_list_defs(&u)
            .iter()
            .map(|(l, v, f)| arr(&[st(l), st(v), f.as_ref().map_or_else(|| "null".to_owned(), |x| js(x))]))
            .collect::<Vec<_>>())),
        "d2b-f8.list_of" => match rx::word_list_of(&u) {
            Some([a, b2, c]) => js(&arr(&[st(&a), st(&b2), st(&c)])),
            None => "null".to_owned(),
        },
        "d2b-f8.ordered" => b(rx::looks_ordered_units(&u)),
        "d2b-f8.support_lists" => b(rx::is_support_lists(&u)),
        "d2b-f8.endif" => b(rx::is_endif(&u)),
        "d2b-f8.ignore" => b(rx::is_mso_list_ignore(&u)),
        "d2b-f8.collapse" => st(&rx::collapse_html_space(&u)),
        "d2b-f8.end_space" => b(rx::ends_space_or_newline(&u)),
        "d2b-f8.lead" => st(rx::trim_leading_spaces(&u)),
        "d2b-f8.h_tag" => opt(rx::heading_tag(&u).map(|d| d.to_string())),
        "d2b-f8.mso_h" => opt(rx::mso_heading(&u).map(|d| d.to_string())),
        "d2b-f8.mso_t" => b(rx::mso_title(&u)),
        "d2b-f8.mso_s" => b(rx::mso_subtitle(&u)),
        "d2b-f8.abs_open" => js(&testing::replace_abstract_open(s, "<w:abstractNum w:abstractNumId=\"7\">")),
        "d2b-f8.style_links" => js(&testing::strip_style_links(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d2b_f8_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f8.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f8.expected")).unwrap();
    let mut n = 0;
    let mut bad = Vec::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        let got = run(id, input);
        if got != want {
            bad.push(format!("site {id} on input #{i} {input:?}: want {want}, got {got}"));
        }
        n += 1;
    }
    println!("regex d2b-f8: {n} V8 results, {} differences", bad.len());
    assert!(bad.is_empty(), "{} mismatches of {n}:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(n > 2000, "{n}");
}
