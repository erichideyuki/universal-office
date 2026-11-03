//! The regex sites of the D1b font-embedding step against V8: the rows of
//! `js/regex-sites/d1b-fonts.mjs` (run by `js/regex-diff.mjs`) over `tests/regex/d1b-fonts.inputs.txt`,
//! expected results in `tests/regex/d1b-fonts.expected`. Each row runs the hand matcher (or the
//! uo-opc edit) the step uses for that site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::write::fonts::docx::{
    find_font_element_ci, font_entries, fonts_add_xmlns_r, fonts_has_xmlns_r, fonts_self_closing, has_embed_tag, setting_on,
    strip_embed_tags,
};
use uo_docx::write::fonts::ooxml::{
    attr_value, esc_attr, find_element, find_start_tag, has_default_extension, last_close_trimmed, read_rels_text, remove_elements,
    replace_range, unesc_attr,
};
use uo_jsnum::json;
use uo_opc::Limits;
use uo_opc::xml::raw::{Insert, expand_self_closing, insert_child};

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

fn b(v: bool) -> String {
    v.to_string()
}

fn family_of(id: &str) -> String {
    let hex = id.strip_prefix("family.").unwrap();
    if hex == "empty" {
        return String::new();
    }
    hex.split('-').map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()).collect()
}

fn run(id: &str, x: &str) -> String {
    let lim = &Limits::DEFAULT;
    let s = |v: String| json::string(&v);
    match id {
        "rels.all" => {
            let mut out = String::new();
            let mut last = 0;
            for t in read_rels_text(x) {
                out.push_str(&x[last..t.start]);
                out.push('[');
                out.push_str(&x[t.start..t.end]);
                out.push(']');
                last = t.end;
            }
            out.push_str(&x[last..]);
            s(out)
        }
        "rels.id" => opt(attr_value(x, "Id")),
        "rels.type" => opt(attr_value(x, "Type")),
        "rels.target" => opt(attr_value(x, "Target")),
        "rels.mode" => opt(attr_value(x, "TargetMode")),
        "attr.unesc" => s(unesc_attr(x)),
        "attr.esc" => s(esc_attr(x)),
        "rels.expand" => s(expand_self_closing(x, "Relationships")),
        "rels.insert" => {
            let text = "<Relationship Id=\"rId9\" Type=\"t\" Target=\"fonts/font1.odttf\"/>";
            s(insert_child(x, "Relationships", text, Insert::BeforeLastCloseTrimmed, lim).unwrap().unwrap_or_else(|| x.to_owned()))
        }
        "ct.has_odttf" => b(has_default_extension(x, "odttf")),
        "ct.after_types" => s(uo_opc::content_types::add_default(
            x,
            "odttf",
            "application/vnd.openxmlformats-officedocument.obfuscatedFont",
            Insert::AfterRootOpen,
            lim,
        )
        .unwrap()),
        "ct.before_types_close" => s(uo_opc::content_types::add_override(
            x,
            "word/fontTable.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml",
            Insert::BeforeLastCloseTrimmed,
            lim,
        )
        .unwrap()),
        "el.find.ttf" | "el.find.zoom" => {
            let q = if id.ends_with("ttf") { "w:embedTrueTypeFonts" } else { "w:zoom" };
            s(match find_element(x, q) {
                Some((a, e)) => format!("[{},{}]", json::string(&x[..a]), json::string(&x[a..e])),
                None => "null".to_owned(),
            })
        }
        "el.remove.ttf" => s(remove_elements(x, "w:embedTrueTypeFonts")),
        "el.remove.sys" => s(remove_elements(x, "w:embedSystemFonts")),
        "el.remove.a" => s(remove_elements(x, "a")),
        "settings.on.ttf" => b(setting_on(x, "embedTrueTypeFonts")),
        "settings.on.subset" => b(setting_on(x, "saveSubsetFonts")),
        "settings.on.sys" => b(setting_on(x, "embedSystemFonts")),
        "settings.root" => opt(find_start_tag(x, "w:settings").map(|(a, e)| &x[a..e])),
        "settings.root_edit" => s(match find_start_tag(x, "w:settings") {
            None => "THROW".to_owned(),
            Some((a, e)) => {
                let root = &x[a..e];
                let xml = match root.strip_suffix("/>") {
                    Some(open) => replace_range(x, a, e, &format!("{open}></w:settings>"), u64::MAX).unwrap(),
                    None => x.to_owned(),
                };
                let (_, e2) = find_start_tag(&xml, "w:settings").unwrap();
                format!("[{},{}]", json::string(&xml[..e2]), json::string(&xml[e2..]))
            }
        }),
        "fonttable.entries" => {
            let mut out = String::new();
            let mut last = 0;
            for e in font_entries(x) {
                out.push_str(&x[last..e.start]);
                out.push_str(&format!("[[{},{}]]", json::string(&x[e.value.0..e.value.1]), json::string(&x[e.content.0..e.content.1])));
                last = e.end;
            }
            out.push_str(&x[last..]);
            s(out)
        }
        "fonttable.embed_tag" => b(has_embed_tag(x)),
        "fonttable.strip" => s(strip_embed_tags(x)),
        "fonts.has_r" => b(fonts_has_xmlns_r(x)),
        "fonts.add_r" => s(fonts_add_xmlns_r(x)),
        "fonts.self_closing" => b(fonts_self_closing(x).is_some()),
        "fonts.expand" => s(match fonts_self_closing(x) {
            Some((slash, gt)) => format!("{}></w:fonts>{}", &x[..slash], &x[gt + 1..]),
            None => x.to_owned(),
        }),
        "font.close_tail" => s(match x.strip_suffix("</w:font>") {
            Some(body) => format!("{body}<w:embedBold r:id=\"rId1\" w:fontKey=\"{{K}}\"/></w:font>"),
            None => x.to_owned(),
        }),
        "fonts.close_subst" => s(match last_close_trimmed(x, "w:fonts") {
            Some(p) => {
                replace_range(x, p, x.len(), "<w:font w:name=\"$&|$`|$'|$$|$1|$0|$01|$<x>|$\">X</w:font></w:fonts>", u64::MAX).unwrap()
            }
            None => x.to_owned(),
        }),
        "str.subst" => s(match x.find("<w:font") {
            Some(p) => replace_range(x, p, p + 7, "A$&B$`C$'D$$E$1F$<G$", u64::MAX).unwrap(),
            None => x.to_owned(),
        }),
        _ if id.starts_with("family.") => opt(find_font_element_ci(x, &esc_attr(&family_of(id))).map(|(a, e)| &x[a..e])),
        _ => panic!("no Rust matcher for site {id}"),
    }
}

#[test]
fn d1b_fonts_sites_match_v8() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/regex/");
    let inputs: Vec<String> = std::fs::read_to_string(format!("{dir}d1b-fonts.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(format!("{dir}d1b-fonts.expected")).unwrap();
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
    assert!(sites.len() >= 70, "every site of d1b-fonts.mjs is replayed ({})", sites.len());
    println!("{n} results over {} sites, all equal to V8", sites.len());
}
