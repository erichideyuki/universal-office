//! The regex sites of frontier R4b (`document.ts` inline objects, `picture-xml.ts`) against V8:
//! the rows of `js/regex-sites/r4b.mjs` (run by `js/regex-diff.mjs`) over
//! `tests/regex/r4b.inputs.txt`, expected results in `tests/regex/r4b.expected`. The Rust side is
//! the matcher or the helper each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::drawing::val_int_group;
use uo_docx::read::inline_objects::{has_bold_weight, is_diagram_uri, strip_leading_hash, strip_vml_prefix};
use uo_docx::read::matchers::{find_lit_then_boundary, is_one_of, keep_ascii_digits};
use uo_docx::read::watermark::strip_quotes;
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

fn group(g: Option<&str>) -> String {
    g.map_or_else(|| "null".into(), json::string)
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "r1.diagram_uri" => is_diagram_uri(s).to_string(),
        "r2.one_of" => is_one_of(s, &["1", "true"]).to_string(),
        "r3.bold" => has_bold_weight(s).to_string(),
        "r3.italic" => s.contains("italic").to_string(),
        "r4.quotes" => json::string(strip_quotes(s)),
        "r4.hash" => json::string(strip_leading_hash(s)),
        "r4.digits" => json::string(&keep_ascii_digits(s)),
        "r4.vtag" => json::string(strip_vml_prefix(s)),
        "r5.blipfill" => {
            // the UTF-16 index of the match (the inputs hold astral characters before it only in the quoted case)
            find_lit_then_boundary(s, "<a:blipFill")
                .map_or_else(|| "null".into(), |b| uo_jsnum::text::utf16_len(s.get(..b).unwrap_or("")).to_string())
        }
        "r6.val_int" => group(val_int_group(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn r4b_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r4b.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r4b.expected")).unwrap();
    let mut n = 0;
    let mut sites = std::collections::BTreeSet::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on {input:?}");
        sites.insert(id.to_string());
        n += 1;
    }
    assert_eq!(sites.len(), 10, "every site of r4b.mjs is replayed");
    assert_eq!(n, 10 * inputs.len());
}
