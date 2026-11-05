//! The regex sites of the D1b frontier W2 (`write::emit::object`: `patchDrawingGeometry`, `docPrName`,
//! `freshDrawingIds`, `editedGroupXml`, `shapeOut`, `txbxContentOf`) against V8: the rows of
//! `js/regex-sites/w2.mjs` over `tests/regex/w2.inputs.txt`, expected results in `tests/regex/w2.expected`
//! (regenerate: `node core/crates/uo-docx/js/regex-diff.mjs`). Each row runs the matcher the writer uses
//! for that site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::js::JsNumber;
use uo_docx::write::emit::object::{
    Geometry, doc_pr_name, edited_group_text, fresh_ids_text, has_alternate_content, has_behind_doc, has_doc_pr_hidden,
    patch_drawing_geometry, relative_height_of, replace_first_drawing, strip_fallback, txbx_content_of,
};
use uo_jsnum::json;

const RAW: &str = "<w:drawing><wp:anchor behindDoc=\"1\" relativeHeight=\"251659264\"><wp:docPr id=\"2\" name=\"Grupo &amp; 1\" hidden=\"1\"/></wp:anchor></w:drawing>";
const RAW2: &str =
    "<w:drawing><wp:anchor behindDoc=\"0\" relativeHeight=\"0007\"><wp:docPr id=\"2\" hidden=\"0\"/></wp:anchor></w:drawing>";
const SRC: &str = "<mc:AlternateContent><mc:Choice Requires=\"wpg\"><w:drawing><wp:anchor/></w:drawing></mc:Choice><mc:Fallback><w:pict><v:group id=\"old\" style=\"position:absolute;visibility:visible;z-index:5\"></v:group></w:pict></mc:Fallback></mc:AlternateContent>";

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

fn n(x: f64) -> Option<JsNumber> {
    Some(JsNumber::Num(x))
}

fn geo(w: f64, h: f64, ox: Option<f64>, oy: Option<f64>) -> Geometry {
    Geometry {
        width_twips: JsNumber::Num(w),
        height_twips: JsNumber::Num(h),
        offset_x_twips: ox.and_then(n),
        offset_y_twips: oy.and_then(n),
    }
}

fn s(x: String) -> String {
    json::string(&x)
}

fn b(x: bool) -> String {
    x.to_string()
}

fn run(id: &str, x: &str) -> String {
    match id {
        "geo.1" => s(patch_drawing_geometry(x, &geo(1234.0, 567.0, Some(10.0), Some(-5.0))).unwrap()),
        "geo.2" => s(patch_drawing_geometry(x, &geo(100.5, 0.0, None, None)).unwrap()),
        "geo.3" => s(patch_drawing_geometry(x, &geo(1e15, -3.0, Some(12_345_678_901.0), Some(0.0))).unwrap()),
        "docpr.name" => s(doc_pr_name(Some(x)).unwrap_or_else(|| "undefined".to_owned())),
        "docpr.hidden" => b(has_doc_pr_hidden(x)),
        "rh" => relative_height_of(x).map_or_else(|| "null".to_owned(), json::string),
        "behind" => b(has_behind_doc(x)),
        "fresh" => {
            let mut counter = 0.0;
            let out = fresh_ids_text(x, &mut counter);
            s(format!("{out}|{}", uo_jsnum::format::to_js_string(counter)))
        }
        "group.src" => s(edited_group_text(x, RAW)),
        "group.src2" => s(edited_group_text(x, RAW2)),
        "group.raw" => s(edited_group_text(SRC, x)),
        "ac" => b(has_alternate_content(x)),
        "strip" => s(strip_fallback(x)),
        "txbx.of" => s(txbx_content_of(x).unwrap_or_else(|| "undefined".to_owned())),
        "drawing.first" => s(replace_first_drawing(x, "RAW")),
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
fn w2_regex_sites_match_v8() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regex");
    let inputs: Vec<String> = std::fs::read_to_string(dir.join("w2.inputs.txt")).unwrap().lines().map(unescape).collect();
    let expected = std::fs::read_to_string(dir.join("w2.expected")).unwrap();
    let mut bad = Vec::new();
    let mut count = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let mut it = line.splitn(3, '\t');
        let (id, i, want) = (it.next().unwrap(), it.next().unwrap().parse::<usize>().unwrap(), it.next().unwrap());
        let got = run(id, &inputs[i]);
        // V8 writes a lone surrogate as `\udxxx`; the TS saves it as U+FFFD (`TextEncoder`)
        let want = &lone_to_fffd(want);
        count += 1;
        if got != *want {
            bad.push(format!("{id} #{i} {:?}\n  v8:   {want}\n  rust: {got}", inputs[i]));
        }
    }
    assert!(bad.is_empty(), "{} of {count} differ:\n{}", bad.len(), bad.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    assert!(count > 9_000);
}
