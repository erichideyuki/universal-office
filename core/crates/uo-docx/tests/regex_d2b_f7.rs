//! The regex sites of D2b-F7 against V8: the rows of `js/regex-sites/d2b-f7.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d2b-f7.inputs.txt`, expected results in
//! `tests/regex/d2b-f7.expected`. The Rust side is the matcher each site became
//! (`session::ops::drawing::rx`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::drawing::rx;
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

fn run(id: &str, s: &str) -> String {
    match id {
        "d2b-f7.sp_in" => js(&rx::sp_pr_inner(s)),
        "d2b-f7.strip_xfrm" => js(&rx::strip_xfrm(s)),
        "d2b-f7.strip_geometry" => js(&rx::strip_geometry(s)),
        "d2b-f7.doc_pr_ids" => js(&format!("[{}]", rx::doc_pr_ids(s).iter().map(|d| js(d)).collect::<Vec<_>>().join(","))),
        "d2b-f7.user_id" => js(&rx::replace_doc_pr_id(s, "77")),
        "d2b-f7.user_name" => js(&rx::replace_doc_pr_name(s, "Text Box 77")),
        "d2b-f7.user_z" => js(&rx::replace_relative_height(s, "251659264")),
        "d2b-f7.theme_path" => opt(if s.is_empty() { None } else { rx::theme_part_path(s) }.as_deref()),
        "d2b-f7.tx_defaults" => match rx::tx_def(s).filter(|t| !t.is_empty()) {
            None => "null".to_owned(),
            Some(td) => {
                let bp = rx::tx_def_body_pr(td);
                let at = |n: &str| opt(rx::body_pr_attr(bp, n));
                let a = [at("lIns"), at("tIns"), at("rIns"), at("bIns"), at("vert"), at("wrap"), at("anchor")].join(",");
                js(&format!("[{},{},{a},{}]", js(rx::tx_def_sp_pr(td)), js(bp), td.contains("<a:spAutoFit/>")))
            }
        },
        "d2b-f7.tx_def_span" => match rx::tx_def_span(s) {
            Some((i, j)) => js(&format!("{}[TX]{}", &s[..i], &s[j..])),
            None => "null".to_owned(),
        },
        "d2b-f7.squash" => js(&rx::squash(s)),
        "d2b-f7.style_parts" => {
            rx::shape_style_parts(s).map_or_else(|| "null".to_owned(), |(a, b)| js(&format!("[{},{}]", js(&a), js(&b))))
        }
        "d2b-f7.svg_ext" => js(&rx::strip_svg_blip_ext(s)),
        "d2b-f7.glow" => match rx::parse_glow(s) {
            Some(_) => {
                let rest = s.strip_prefix("glow-").unwrap();
                let (a, b) = rest.split_once('-').unwrap();
                js(&format!("[{},{}]", js(a), js(b)))
            }
            None => "null".to_owned(),
        },
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d2b_f7_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f7.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f7.expected")).unwrap();
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
    assert!(n > 2000, "{n}");
}
