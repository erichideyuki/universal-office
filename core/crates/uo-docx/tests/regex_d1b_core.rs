//! The regex sites of the D1b writer core against V8: the rows of `js/regex-sites/d1b-core.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/d1b-core.inputs.txt`, expected results in
//! `tests/regex/d1b-core.expected`. Each row runs the matcher the writer uses for that site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::write::ctx::GenNumIds;
use uo_docx::write::matchers::{
    TagPattern, Tail, code_parts, comment_range_pattern, comment_reference_runs, count_txbx_contents, drop_comment_marks,
    field_code_groups, find_literal_ci, free_num_ids, grid_of, has_prefixed_attr, has_xmlns, is_empty_element, is_word_rels_path, max_w_id,
    rel_id_of, relationship_tags, remove_elements_lazy, remove_tags, replace_tags, replace_txbx_contents, strip_pg_num_attrs,
};
use uo_docx::write::parts::{
    comment_elements, durable_id, merge_numbering, numbering_abstracts, numbering_nums, para_ids, with_namespaces,
};
use uo_docx::write::rev::{W16DU_NS, ensure_revision_namespaces, track_revisions_on};
use uo_jsnum::json;

const IDS: &[&str] = &["1", "12", "-3"];
const PARA: &[&str] = &["0A1B", "FF", "12"];
const DUR: &[&str] = &["d1", "D2", "x-y"];

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

fn b(v: bool) -> String {
    v.to_string()
}

/// The full namespace URIs the writer adds, shortened as the JS rows write them.
fn shorten(mut s: String) -> String {
    for (uri, short) in [
        ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "W"),
        ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "R"),
        ("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "WP"),
        ("http://schemas.microsoft.com/office/word/2012/wordml", "W15"),
        ("urn:schemas-microsoft-com:vml", "V"),
        ("urn:schemas-microsoft-com:office:office", "O"),
        ("urn:schemas-microsoft-com:office:word", "W10"),
        ("http://schemas.microsoft.com/office/word/2010/wordml", "W14"),
        ("http://schemas.openxmlformats.org/markup-compatibility/2006", "MC"),
        ("http://schemas.microsoft.com/office/word/2010/wordprocessingShape", "WPS"),
        ("http://schemas.openxmlformats.org/drawingml/2006/main", "A"),
        ("http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing", "WP14"),
    ] {
        s = s.replace(&format!("\"{uri}\""), &format!("\"{short}\""));
    }
    s
}

fn run(id: &str, x: &str) -> String {
    let s = |v: String| json::string(&v);
    match id {
        "drop.range" => s(remove_tags(x, &comment_range_pattern(IDS))),
        "drop.ref" => {
            let mut out = String::new();
            let mut last = 0;
            for (a, e) in comment_reference_runs(x, IDS) {
                out.push_str(&x[last..a]);
                last = e;
            }
            out.push_str(&x[last..]);
            s(out)
        }
        "drop.all" => s(drop_comment_marks(x, IDS)),
        "cids" => {
            let pat = TagPattern {
                names: &["w16cid:commentId"],
                name_boundary: true,
                attr: "w16cid:paraId",
                attr_boundary: false,
                values: PARA,
                tail: Tail::SelfClose,
            };
            s(replace_tags(x, &pat, |m| format!("[{}]", durable_id(m).unwrap_or(""))))
        }
        "cex" => {
            let pat = TagPattern {
                names: &["w15:commentEx"],
                name_boundary: true,
                attr: "w15:paraId",
                attr_boundary: false,
                values: PARA,
                tail: Tail::SelfClose,
            };
            s(remove_tags(x, &pat))
        }
        "cexb" => {
            let pat = TagPattern {
                names: &["w16cex:commentExtensible"],
                name_boundary: true,
                attr: "w16cex:durableId",
                attr_boundary: false,
                values: DUR,
                tail: Tail::SelfCloseOrElement("w16cex:commentExtensible"),
            };
            s(remove_tags(x, &pat))
        }
        "comment.el" => {
            let mut out = String::new();
            let mut last = 0;
            for (a, e, id) in comment_elements(x) {
                out.push_str(&x[last..a]);
                out.push_str(&format!("[{id}]"));
                last = e;
            }
            out.push_str(&x[last..]);
            s(out)
        }
        "para.ids" => s(para_ids(x).join(",")),
        "track.rm" => s(remove_elements_lazy(x, "w:trackRevisions")),
        "track.on" => s(b(track_revisions_on(x))),
        "field.groups" => match x.split_once('§') {
            None => s("skip".into()),
            Some((code, flat)) => {
                // `flat === undefined` only without `§`; a second `§` stays in the flat part in JS
                // (`split` keeps the second piece only): mirror that
                let flat = flat.split('§').next().unwrap_or("");
                let parts = code_parts(uo_jsnum::text::trim(code));
                match field_code_groups(&parts, uo_jsnum::text::trim(flat)) {
                    Some(g) => s(format!("[{}]", g.iter().map(|x| json::string(x)).collect::<Vec<_>>().join(","))),
                    None => s("null".into()),
                }
            }
        },
        "ci.png" => b(find_literal_ci(x, "Extension=\"png\"")),
        "ci.svg" => b(find_literal_ci(x, "Extension=\"svg\"")),
        "xmlns.wp" => b(has_xmlns(x, "wp")),
        "attr.o" => b(has_prefixed_attr(x, "o:") || x.contains("<o:")),
        "attr.w14" => b(has_prefixed_attr(x, "w14:") || x.contains("<w14:")),
        "attr.wp14" => b(x.contains("<wp14:") || has_prefixed_attr(x, "wp14:")),
        "with.ns" => s(shorten(with_namespaces(x, "w:document", x))),
        "rev.ns" => s(ensure_revision_namespaces(x, "w:document")
            .replace(&format!("\"{W16DU_NS}\""), "\"U\"")
            .replace("\"http://schemas.openxmlformats.org/markup-compatibility/2006\"", "\"M\"")),
        "rev.max" => s(uo_jsnum::format::to_js_string(max_w_id(&[x]) + 1.0)),
        "num.free" => {
            let (a, c) = free_num_ids(Some(x));
            s(format!("{},{}", uo_jsnum::format::to_js_string(a), uo_jsnum::format::to_js_string(c)))
        }
        "num.merge" => {
            let ids = GenNumIds { bullet: 1001.0, decimal: 1002.0 };
            s(merge_numbering(x, ids).replace(&numbering_abstracts(ids), "[A]").replace(&numbering_nums(ids), "[N]"))
        }
        "grid" => s(grid_of(x)),
        "rels.ids" => {
            s(relationship_tags(x).iter().map(|&(a, e)| rel_id_of(&x[a..e]).unwrap_or("-").to_owned()).collect::<Vec<_>>().join(","))
        }
        "rels.path" => b(is_word_rels_path(x)),
        "empty.ind" => b(is_empty_element(x, "w:ind")),
        "empty.pgnum" => b(is_empty_element(x, "w:pgNumType")),
        "pgnum.strip" => s(strip_pg_num_attrs(x)),
        "txbx.replace" => s(replace_txbx_contents(x, |_| "[X]".to_owned())),
        "txbx.count" => s(count_txbx_contents(x).to_string()),
        other => panic!("unknown site {other}"),
    }
}

/// The lone-surrogate escapes of a JSON text as U+FFFD (the bytes the TS writes for them).
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
fn d1b_core_regex_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("d1b-core.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("d1b-core.expected")).unwrap();
    let mut bad = Vec::new();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, i, want) = (it.next().unwrap(), it.next().unwrap().parse::<usize>().unwrap(), it.next().unwrap());
        let got = run(id, &inputs[i]);
        // V8 writes a lone surrogate as `\udxxx`; the TS saves it as U+FFFD (`TextEncoder`)
        let want = &lone_to_fffd(want);
        n += 1;
        if got != *want {
            bad.push(format!("{id} #{i} {:?}\n  v8:   {want}\n  rust: {got}", inputs[i]));
        }
    }
    assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(n > 10_000);
}
