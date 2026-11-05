//! The regex sites of the exporters (D5a) against V8: the rows of `js/regex-sites/d5a-export.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d5a-export.inputs.txt`, expected results in
//! `tests/regex/d5a-export.expected`. The Rust side is the matcher each site became
//! (`export::matchers`).
#![cfg(feature = "export")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::export::matchers::{
    cell_text_escape, collapse_blank_lines, escape_md_into, heading_level_of, lead_space_len, trail_space_start,
};
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

fn units_json(u: &[u16]) -> String {
    json::string(&String::from_utf16_lossy(u))
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    match id {
        "d5a.heading_level" => heading_level_of(s).to_string(),
        "d5a.escape_md" => {
            let mut out = Vec::new();
            escape_md_into(&mut out, &u);
            units_json(&out)
        }
        "d5a.lead_trail" => {
            let lead = lead_space_len(&u);
            let trail = trail_space_start(&u);
            // `(t.match(/\s*$/))[0]`: the match starts at `trail`, but for an all-white string it starts at 0
            format!("[{},{}]", units_json(u.get(..lead).unwrap()), units_json(u.get(trail..).unwrap()))
        }
        "d5a.cell_text" => units_json(&cell_text_escape(&u)),
        "d5a.collapse" => units_json(&collapse_blank_lines(&u)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d5a_export_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d5a-export.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d5a-export.expected")).unwrap();
    let mut n = 0;
    let mut sites = std::collections::BTreeSet::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        let got = run(id, input);
        assert_eq!(got, want, "site {id} on input #{i} {input:?}");
        sites.insert(id.to_owned());
        n += 1;
    }
    assert_eq!(sites.len(), 5, "every site is replayed");
    assert!(n > 800);
}
