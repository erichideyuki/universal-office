//! The regex sites of D2b-F3 against V8: the rows of `js/regex-sites/d2b-f3.mjs` over
//! `tests/regex/d2b-f3.inputs.txt`, expected results in `tests/regex/d2b-f3.expected`. The Rust
//! side is the matcher each site became (`session::ops::tables::{formula, sort}`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::tables::formula::{cell_ref, parse_cell_number, parse_formula_instr, tokenize};
use uo_docx::session::ops::tables::sort::parse_sort_date;
use uo_jsnum::format::to_js_string;
use uo_jsnum::json::{number, string};

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

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The result of a site as the JS expression prints it (`JSON.stringify` of a string).
fn run(id: &str, s: &str) -> String {
    let r: String = match id {
        "d2b-f3.cell_number" => match parse_cell_number(&u16s(s)) {
            None => "null".to_owned(),
            Some(x) => format!(
                "[{},{},{},{},{}]",
                number(x.n),
                to_js_string(x.decimals),
                x.thousands,
                x.prefix.as_deref().map_or("null".to_owned(), string),
                x.suffix.as_deref().map_or("null".to_owned(), string)
            ),
        },
        "d2b-f3.tokenize" => format!("[{}]", tokenize(&u16s(s)).iter().map(|t| string(t)).collect::<Vec<_>>().join(",")),
        "d2b-f3.cell_ref" => cell_ref(s).map_or("null".to_owned(), |(a, b)| format!("[{},{}]", string(a), string(b))),
        "d2b-f3.formula_instr" => match parse_formula_instr(&u16s(s)) {
            None => "null".to_owned(),
            Some((f, p)) => format!(
                "[{},{}]",
                string(&String::from_utf16_lossy(&f)),
                p.map_or("null".to_owned(), |p| string(&String::from_utf16_lossy(&p)))
            ),
        },
        "d2b-f3.sort_date" => parse_sort_date(&u16s(s)).map_or("null".to_owned(), to_js_string),
        _ => panic!("no Rust matcher for site {id}"),
    };
    string(&r)
}

#[test]
fn d2b_f3_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f3.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f3.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on input #{i} {input:?}");
        n += 1;
    }
    println!("regex d2b-f3: {n} results");
    assert!(n > 1000);
}
