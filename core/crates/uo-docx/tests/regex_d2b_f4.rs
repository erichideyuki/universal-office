//! The regex sites of the D2b-F4 remainder (watermark, cover pages, header/footer templates) against
//! V8: the rows of `js/regex-sites/d2b-f4.mjs` (run by `js/regex-diff.mjs`) over
//! `tests/regex/d2b-f4.inputs.txt`, expected results in `tests/regex/d2b-f4.expected`. The Rust side is
//! the matcher each site became (`session::ops::page::rx`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::page::rx;
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

fn opt(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), js)
}

/// `{ let k = 1; while (new RegExp(`Id="rId${k}"`).test(rels)) k++; return k }`.
fn next_rid(rels: &str) -> u64 {
    let mut k = 1;
    while rels.contains(&format!("Id=\"rId{k}\"")) {
        k += 1;
    }
    k
}

fn run(id: &str, s: &str) -> String {
    match id {
        "d2b-f4.pict_span" => opt(rx::pict_span(s)),
        "d2b-f4.hf_part_name" => rx::is_hf_part_name(s).to_string(),
        "d2b-f4.next_spid" => {
            let v = rx::next_spid(s);
            js(&if v.is_finite() { uo_jsnum::format::to_js_string(v) } else { "inf".to_owned() })
        }
        "d2b-f4.cover_gallery" => rx::has_cover_gallery(s).to_string(),
        "d2b-f4.prop_title" => js(&rx::xml_prop_trimmed(s, "dc:title")),
        "d2b-f4.prop_company" => js(&rx::xml_prop_trimmed(s, "Company")),
        "d2b-f4.prop_raw_creator" => opt(rx::xml_prop(s, "dc:creator").as_deref()),
        "d2b-f4.bracketed" => rx::is_bracketed(s).to_string(),
        "d2b-f4.strip_hash" => js(rx::strip_hash(s)),
        "d2b-f4.numpages_ci" => rx::contains_ci_ascii(s, "NUMPAGES").to_string(),
        "d2b-f4.page_ci" => rx::contains_ci_ascii(s, "PAGE").to_string(),
        "d2b-f4.page_cs" => s.contains("PAGE").to_string(),
        "d2b-f4.next_rid" => next_rid(s).to_string(),
        "d2b-f4.default_png" => uo_docx::write::matchers::find_literal_ci(s, "<Default Extension=\"png\"").to_string(),
        "d2b-f4.default_jpeg" => uo_docx::write::matchers::find_literal_ci(s, "<Default Extension=\"jpeg\"").to_string(),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d2b_f4_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f4.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f4.expected")).unwrap();
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
    assert!(bad.is_empty(), "{} mismatches of {n}:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(n > 1000, "{n}");
}
