//! The regex sites of frontier R3 against V8: the rows of `js/regex-sites/r3.mjs` (run by
//! `js/regex-diff.mjs`) over `tests/regex/r3.inputs.txt`, expected results in
//! `tests/regex/r3.expected`. The Rust side is the matcher or the helper each site became.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::drawing::{ends_with_ascii_lower, split_vml_len, val_int_group};
use uo_docx::read::watermark::{attr, find_open_tag, find_shape, has_word_prefixed, is_watermark_xml, strip_quotes, style_prop};
use uo_jsnum::json;
use uo_jsnum::text::{split_js_space, trim};

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

/// The pieces of `s.trim().split(/\s+/)`.
fn pieces(s: &str) -> Vec<&str> {
    split_js_space(trim(s)).collect()
}

/// The Rust of each site.
fn run(id: &str, s: &str) -> String {
    match id {
        "d1.val_int" => group(val_int_group(s)),
        "d2.len_test" => split_vml_len(s).is_some().to_string(),
        "d2.len_num" => group(split_vml_len(s).map(|(n, _)| n)),
        "d2.len_unit" => group(split_vml_len(s).map(|(_, u)| u)),
        "d3.color_token" | "d5.gd_op" => json::string(pieces(s).first().copied().unwrap_or("")),
        "d4.ends_lower" => ends_with_ascii_lower(s).to_string(),
        "d5.gd_arg1" => {
            let p = pieces(s);
            group(if p.len() > 1 { p.get(1).copied() } else { None })
        }
        "v1.x0000" => s.starts_with("_x0000_").to_string(),
        "p1.colors_first" => {
            let p = pieces(s);
            group(if p.len() >= 2 { p.first().copied() } else { None })
        }
        "p1.colors_second" => {
            let p = pieces(s);
            group(if p.len() >= 2 { p.get(1).copied() } else { None })
        }
        "w1.is_wm" => is_watermark_xml(Some(s)).to_string(),
        "w2.attr_style" => group(attr(s, "style").as_deref()),
        "w2.attr_rid" => group(attr(s, "r:id").as_deref()),
        "w2.attr_title" => group(attr(s, "o:title").as_deref()),
        "w2.attr_string" => group(attr(s, "string").as_deref()),
        "w3.prop_width" => group(style_prop(s, "width")),
        "w3.prop_font_family" => group(style_prop(s, "font-family")),
        "w3.prop_font_size" => group(style_prop(s, "font-size")),
        "w3.prop_rotation" => group(style_prop(s, "rotation")),
        "w4.shape_match" => group(find_shape(s).map(|m| m.text)),
        "w4.shape_kind" => group(find_shape(s).map(|m| m.kind)),
        "w5.tag_shape" => group(find_open_tag(s, "v:shape")),
        "w5.tag_imagedata" => group(find_open_tag(s, "v:imagedata")),
        "w5.tag_textpath" => group(find_open_tag(s, "v:textpath")),
        "w5.tag_fill" => group(find_open_tag(s, "v:fill")),
        "w6.gain" => has_word_prefixed(s, "gain=").to_string(),
        "w6.blacklevel" => has_word_prefixed(s, "blacklevel=").to_string(),
        "w7.quotes" => json::string(strip_quotes(s)),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn r3_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}r3.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}r3.expected")).unwrap();
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
    assert_eq!(sites.len(), 29, "every site of r3.mjs is replayed");
    assert_eq!(n, 29 * inputs.len());
}
