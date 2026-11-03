//! The regex site of frontier R1 (`measureTwips` of `ooxml/props.ts`) against V8: the rows of
//! `js/regex-sites/r1.mjs` (run by `js/regex-diff.mjs`) over `tests/regex/r1.inputs.txt`, expected
//! results in `tests/regex/r1.expected`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::props::split_measure;
use uo_docx::read::text_effects::esc_x;
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

fn run(id: &str, s: &str) -> String {
    match id {
        "r1.measure_test" => split_measure(s).is_some().to_string(),
        "r1.measure_number" => group(split_measure(s).map(|(n, _)| n)),
        "r1.measure_unit" => group(split_measure(s).and_then(|(_, u)| u)),
        "r1.esc_x" => json::string(&esc_x(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn r1_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r1.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r1.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on {input:?}");
        n += 1;
    }
    assert_eq!(n, 4 * inputs.len());
}
