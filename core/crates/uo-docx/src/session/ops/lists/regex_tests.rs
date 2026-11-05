//! The regex sites of D2b-F2 against V8: the rows of `js/regex-sites/d2b-f2.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d2b-f2.inputs.txt`, expected results in
//! `tests/regex/d2b-f2.expected`. Each row runs the hand matcher the lists and styles code uses for
//! that site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use super::super::styles::{sheet, xform};
use super::{numbered_raw, put_num_pr, rx, set_level_start, shift_ind_left, store, tmpl_of};
use crate::session::versions::VersionStore;
use uo_jsnum::{format::to_js_string, json};

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
    s.map_or_else(|| "null".to_owned(), json::string)
}

fn slice(x: &str, r: Option<(usize, usize)>) -> String {
    opt(r.and_then(|(s, e)| x.get(s..e)))
}

fn run(id: &str, x: &str) -> String {
    let s = |v: String| json::string(&v);
    let abs = |n: &str| slice(x, rx::element_range(x, "w:abstractNum", "w:abstractNumId", n, "</w:abstractNum>"));
    match id {
        "abs.find" => abs("1"),
        "abs.find12" => abs("12"),
        "num.find" => slice(x, rx::num_range(x, "1")),
        "num.find2" => slice(x, rx::num_range(x, "2")),
        "lvl.find" => slice(x, rx::element_range(x, "w:lvl", "w:ilvl", "1", "</w:lvl>")),
        "nextid.abs" => to_js_string(store::next_id(&VersionStore::new(Some(Arc::from(x))), true)),
        "nextid.num" => to_js_string(store::next_id(&VersionStore::new(Some(Arc::from(x))), false)),
        "search.abs" => store::search_anchor(x, true).map_or_else(|| "null".to_owned(), |p| p.to_string()),
        "search.num" => store::search_anchor(x, false).map_or_else(|| "null".to_owned(), |p| p.to_string()),
        "copy.tplc" => s(store::blank_tplc(x)),
        "copy.absid" => s(store::replace_abstract_id(x, 7.0)),
        "copy.nsid" => s(store::replace_hex_element(x, "<w:nsid w:val=\"", "<w:nsid w:val=\"AAAAAAAA\"/>")),
        "setlevels" => s(store::replace_levels(x, &["LVLS".to_owned()])),
        "mlt" => s(store::replace_multi_level_type(x, "multilevel")),
        "tmpl.set" => s(store::replace_hex_element(x, "<w:tmpl w:val=\"", "<w:tmpl w:val=\"01234567\"/>")),
        "tentative" => s(store::tentative_levels(x, &[0.0])),
        "ind.find" => opt(store::ind_element(x)),
        "ind.replace" => s(store::replace_ind(x, "<w:ind/>")),
        "ilvl.replace" => s(store::replace_ilvl(x, 3.0)),
        "indleft" => s(shift_ind_left(x, 360.0)),
        "start.test" => rx::next_open(x, "w:start", 0).is_some().to_string(),
        "start.set" => s(set_level_start(x, "<w:start w:val=\"5\"/>")),
        "tmpl8" => opt(tmpl_of(x).as_deref()),
        "numpr.test" => numbered_raw(x).to_string(),
        "put.numpr" => s(put_num_pr(x, "<w:numPr><w:numId w:val=\"9\"/></w:numPr>")),
        "lvl.all" => format!("[{}]", super::all_level_elements(x).iter().map(|l| json::string(l)).collect::<Vec<_>>().join(",")),
        "lsd.q" => s(sheet::set_latent_q_format(x, "heading 1", true)),
        "lsd.noq" => s(sheet::set_latent_q_format(x, "heading 1", false)),
        "refs.in" => format!(
            "[{}]",
            xform::ref_tags(x, &["w:basedOn", "w:next", "w:link"]).iter().map(|t| json::string(&t.3)).collect::<Vec<_>>().join(",")
        ),
        "retarget" => s(xform::retarget(x, &[("X".to_owned(), "Y".to_owned())])),
        "qformat.remove" => s(sheet::remove_q_format(x)),
        other => panic!("site {other}"),
    }
}

#[test]
fn d2b_f2_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d2b-f2.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d2b-f2.expected")).unwrap();
    let (mut n, mut bad) = (0, Vec::new());
    let mut sites = std::collections::BTreeSet::new();
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(3, '\t');
        let (id, i, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = &inputs[i.parse::<usize>().unwrap()];
        let got = run(id, input);
        if got != want && bad.len() < 20 {
            bad.push(format!("site {id} on {input:?}:\n  rust {got}\n  v8   {want}"));
        }
        sites.insert(id.to_string());
        n += 1;
    }
    assert!(bad.is_empty(), "{} mismatches (first 20):\n{}", bad.len(), bad.join("\n"));
    assert_eq!(n, sites.len() * inputs.len(), "every site runs on every input");
    assert!(sites.len() >= 30, "every site of d2b-f2.mjs is replayed ({})", sites.len());
    println!("d2b-f2 regex: {n} results over {} sites, all equal to V8", sites.len());
}
