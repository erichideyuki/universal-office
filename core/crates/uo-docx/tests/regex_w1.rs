//! The regex sites of the D1b frontier W1 against V8: the rows of `js/regex-sites/w1.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/w1.inputs.txt`, expected results in `tests/regex/w1.expected`.
//! Each row runs the function the writer uses for that site (`write::emit::run`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::write::emit::run::{
    encode_uri_component, field_code_xml, has_h_switch, is_formula, is_marker_field, is_note_ref_mark, text_xml, with_no_proof,
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

/// `/^[a-zA-Z0-9._-]+$/.test(type)` as the writer checks it.
fn custom_type_ok(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn run(id: &str, x: &str) -> String {
    let s = |v: String| json::string(&v);
    match id {
        "note.mark" => is_note_ref_mark(x).to_string(),
        "formula" => is_formula(x).to_string(),
        "marker" => is_marker_field(x).to_string(),
        "hswitch" => has_h_switch(x).to_string(),
        "custom.type" => custom_type_ok(x).to_string(),
        "text.xml" => s(text_xml(x, false)),
        "text.del" => s(text_xml(x, true)),
        "no.proof" => s(with_no_proof(x)),
        "field.code" => {
            let (code, flat) = match x.split_once('§') {
                // `x.split('§')` keeps the first two pieces: a second `§` ends the flat text
                Some((code, rest)) => (code, Some(rest.split('§').next().unwrap_or(""))),
                None => (x, None),
            };
            s(field_code_xml(code, flat, "<w:rPr/>", "w:instrText").unwrap())
        }
        "uri" => s(encode_uri_component(x)),
        other => panic!("unknown site {other}"),
    }
}

/// The lone-surrogate escapes of a JSON text as U+FFFD (the bytes the TS writes for them: `TextEncoder`).
fn lone_to_fffd(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\ud") {
        out.push_str(&rest[..i]);
        let unit = rest.get(i + 2..i + 6).and_then(|h| u16::from_str_radix(h, 16).ok());
        if unit.is_some_and(|u| (0xD800..=0xDFFF).contains(&u)) {
            out.push('\u{FFFD}');
            rest = &rest[i + 6..];
        } else {
            out.push('\\');
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn w1_regex_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("w1.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("w1.expected")).unwrap();
    let mut bad = Vec::new();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, i, want) = (it.next().unwrap(), it.next().unwrap().parse::<usize>().unwrap(), it.next().unwrap());
        let got = run(id, &inputs[i]);
        // V8 writes a lone surrogate as `\udxxx` (`slice(0, -1)` cuts a final non-BMP character); the TS
        // saves it as U+FFFD (`TextEncoder`)
        let want = &lone_to_fffd(want);
        n += 1;
        if got != *want {
            bad.push(format!("{id} #{i} {:?}\n  v8:   {want}\n  rust: {got}", inputs[i]));
        }
    }
    assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(n > 5_000);
}
