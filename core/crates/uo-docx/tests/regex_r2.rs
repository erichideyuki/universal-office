//! The regex sites of frontier R2 (`ooxml/parts.ts`, `table-styles.ts`, `style-names-es.ts`) against
//! V8: the rows of `js/regex-sites/r2.mjs` (run by `js/regex-diff.mjs`) over
//! `tests/regex/r2.inputs.txt`, expected results in `tests/regex/r2.expected`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::parts::{target_ext_group, vml_len_match};
use uo_docx::read::style_names_es::{FAMILY_RULES, family_replace, split_accent};
use uo_docx::read::table_styles::{attr_value, local_style_id, replace_marker};
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

/// The Rust matcher of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "parts.vml_width_num" => group(vml_len_match(s, "width").map(|m| m.0)),
        "parts.vml_width_unit" => group(vml_len_match(s, "width").and_then(|m| m.1)),
        "parts.vml_height_num" => group(vml_len_match(s, "height").map(|m| m.0)),
        "parts.vml_height_unit" => group(vml_len_match(s, "height").and_then(|m| m.1)),
        "parts.target_ext" => group(target_ext_group(s)),
        "parts.strip_a" => json::string(s.strip_prefix("a:").unwrap_or(s)),
        "parts.strip_w" => json::string(s.strip_prefix("w:").unwrap_or(s)),
        "table.local_id" => json::string(&local_style_id(s)),
        "table.accent_sub" => json::string(&s.replace("{A}", "7")),
        "table.id_sub" => json::string(&s.replacen("{ID}", "Q", 1)),
        "table.attr_themeColor" => group(attr_value(s, "w:themeColor")),
        "table.attr_themeFillTint" => group(attr_value(s, "w:themeFillTint")),
        "table.attr_themeFill" => group(attr_value(s, "w:themeFill")),
        "table.marker_color" => json::string(&replace_marker(s, "w:color=\"~\"", |rest| format!("[{rest}]"))),
        "table.marker_fill" => json::string(&replace_marker(s, "w:fill=\"~\"", |rest| format!("[{rest}]"))),
        "es.accent_base" => group(split_accent(s).map(|m| m.0)),
        "es.accent_digit" => {
            let digit = split_accent(s).and_then(|m| m.1).map(String::from);
            group(digit.as_deref())
        }
        _ => match id.strip_prefix("es.family_").and_then(|n| n.parse::<usize>().ok()) {
            Some(rule) => json::string(&family_replace(rule, s)),
            None => panic!("no Rust matcher for site {id}"),
        },
    }
}

#[test]
fn sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r2.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r2.expected")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        assert_eq!(run(id, input), want, "site {id} on {input:?}");
        n += 1;
    }
    assert_eq!(n, (17 + FAMILY_RULES) * inputs.len());
}
