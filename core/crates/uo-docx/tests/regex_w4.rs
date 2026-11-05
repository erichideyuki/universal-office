//! The regex sites of the D1b frontier W4/W4b against V8: the rows of `js/regex-sites/w4.mjs` (run on its
//! own, not by `js/regex-diff.mjs`, which rewrites the other frontiers' files) over
//! `tests/regex/w4.inputs.txt`, expected results in `tests/regex/w4.expected`. Each row runs the function
//! the writer uses for that site (`write::emit::notes`, `write::emit::settings`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use uo_docx::js::{JsNumber, Str};
use uo_docx::model::formatting::NoteProps;
use uo_docx::write::emit::notes::{NotePropsArg, free_rid_number, has_note_relationship, patch_settings_notes, settings_props_element};
use uo_docx::write::emit::settings::{
    copy_hf_part_xml, drop_hf_text, hf_target_path, is_hf_relationship, is_page_number_gallery, quoted_attr, remove_override,
    self_closing_matches, val_is_off, val_pattern_test,
};
use uo_docx::write::matchers::element_matches;
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

fn opt_str(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), json::string)
}

fn first_range(x: &str, r: Option<&(usize, usize)>) -> String {
    opt_str(r.and_then(|&(a, b)| x.get(a..b)))
}

fn n(x: f64) -> Option<JsNumber> {
    Some(JsNumber::Num(x))
}

fn run(id: &str, x: &str) -> String {
    match id {
        "tab.cur" => first_range(x, self_closing_matches(x, "w:defaultTabStop").first()),
        "tab.val" => {
            let mut it = x.split('§');
            let v = it.next().unwrap_or("");
            let hay = it.next().unwrap_or("");
            let val = uo_jsnum::format::to_js_string(uo_jsnum::math::round(uo_jsnum::parse::to_number(v)));
            val_pattern_test(hay, &val).to_string()
        }
        "hy.cur" => first_range(x, element_matches(x, "w:autoHyphenation").first()),
        "flag.cur" => first_range(x, element_matches(x, "w:evenAndOddHeaders").first()),
        "flag.off" => val_is_off(x).to_string(),
        "notes.props" => first_range(x, settings_props_element(x, "w:footnotePr").as_ref()),
        "notes.refs" => {
            let refs: String = self_closing_matches(x, "w:footnote").iter().filter_map(|&(a, b)| x.get(a..b)).collect();
            json::string(&refs)
        }
        "notes.norefs" => {
            let mut out = String::new();
            let mut last = 0;
            for (a, b) in self_closing_matches(x, "w:footnote") {
                out.push_str(&x[last..a]);
                last = b;
            }
            out.push_str(&x[last..]);
            json::string(&out)
        }
        "notes.keep" => json::string(&patch_settings_notes(x, NotePropsArg::Unchanged, NotePropsArg::Unchanged)),
        "notes.set" => {
            let fp = NoteProps {
                pos: Some(Str::from("pageBottom")),
                num_fmt: Some(Str::from("lowerRoman")),
                num_start: n(3.0),
                num_restart: Some(Str::from("eachPage")),
                ..NoteProps::default()
            };
            json::string(&patch_settings_notes(x, NotePropsArg::Set(&fp), NotePropsArg::Remove))
        }
        "notes.set2" => {
            let ep = NoteProps { pos: Some(Str::from("sectEnd")), num_start: n(0.0), ..NoteProps::default() };
            json::string(&patch_settings_notes(x, NotePropsArg::Remove, NotePropsArg::Set(&ep)))
        }
        "notes.relfn" => has_note_relationship(x, "footnote").to_string(),
        "notes.relen" => has_note_relationship(x, "endnote").to_string(),
        "notes.free" => free_rid_number(x).to_string(),
        "gallery" => is_page_number_gallery(x).to_string(),
        "attr.id" => opt_str(quoted_attr(x, "Id")),
        "attr.target" => opt_str(quoted_attr(x, "Target")),
        "hf.rel" => is_hf_relationship(x).to_string(),
        "hf.target" => json::string(&hf_target_path(x)),
        "hf.override" => {
            let mut it = x.split('§');
            let path: String = it.next().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || "._/-".contains(*c)).collect();
            json::string(&remove_override(it.next().unwrap_or(""), &path))
        }
        "hf.copy" => {
            let mut k = 100.0;
            json::string(&copy_hf_part_xml(x, &mut || {
                k += 1.0;
                k
            }))
        }
        "hf.drop" => {
            let mut it = x.split('§');
            let rels = it.next().unwrap_or("");
            let ct = it.next().unwrap_or("");
            let ids: HashSet<String> = it.next().unwrap_or("").split(',').map(str::to_owned).collect();
            let (rels, ct, removed) = drop_hf_text(rels, ct, &ids);
            let removed: Vec<String> = removed.iter().map(|p| json::string(p)).collect();
            format!("{{\"rels\":{},\"ct\":{},\"removed\":[{}]}}", json::string(&rels), json::string(&ct), removed.join(","))
        }
        other => panic!("unknown site {other}"),
    }
}

#[test]
fn w4_regex_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("w4.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("w4.expected")).unwrap();
    let mut bad = Vec::new();
    let mut count = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, i, want) = (it.next().unwrap(), it.next().unwrap().parse::<usize>().unwrap(), it.next().unwrap());
        let got = run(id, &inputs[i]);
        count += 1;
        if got != want {
            bad.push(format!("{id} #{i} {:?}\n  v8:   {want}\n  rust: {got}", inputs[i]));
        }
    }
    assert!(bad.is_empty(), "{} of {count} differ:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(count > 19_000);
}
