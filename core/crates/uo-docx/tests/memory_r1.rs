//! Memory gate M1 of frontier R1: every public reader of `read::props` and `read::text_effects`
//! returns the live bytes exactly to their value before the call, output dropped, on the inputs
//! of the quirk corpus (including the ones that read nothing: absent input, empty elements,
//! unknown children, deep chains).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::assert_no_leak;
use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::read::DOCX;
use uo_docx::read::props::{
    int, measure_twips, on_off_val, read_border, read_ppr, read_revision_info, read_rpr, read_shading, read_tbl_pr, read_tc_pr, read_tr_pr,
    split_measure,
};
use uo_docx::read::text_effects::{element_xml, read_color, read_fill, read_text_effects};
use uo_opc::xml::{Document, parse};

fn quirks() -> Vec<Document> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/props.quirks.txt")).unwrap();
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (_, t) = l.split_once('\t').unwrap();
            let t = t.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\\\", "\\");
            parse(&t, &DOCX).ok()
        })
        .collect()
}

#[test]
fn m1_readers_over_the_quirk_corpus() {
    let docs = quirks();
    assert!(docs.len() > 500);
    for d in &docs {
        let root = d.root();
        for el in std::iter::once(root).chain(root.descendants()) {
            match el.name() {
                "w:rPr" => {
                    assert_no_leak(|| black_box(read_rpr(Some(el))));
                    assert_no_leak(|| black_box(read_text_effects(el)));
                    assert_no_leak(|| JsonWriter::stringify(&read_rpr(Some(el)).unwrap(), None));
                }
                "w:pPr" => {
                    assert_no_leak(|| black_box(read_ppr(Some(el))));
                }
                "w:tblPr" | "w:tblPrEx" => {
                    assert_no_leak(|| black_box(read_tbl_pr(Some(el))));
                }
                "w:trPr" => {
                    assert_no_leak(|| black_box(read_tr_pr(Some(el))));
                }
                "w:tcPr" => {
                    assert_no_leak(|| black_box(read_tc_pr(Some(el))));
                }
                "w:top" | "w:bdr" | "w:left" => {
                    assert_no_leak(|| black_box(read_border(Some(el))));
                }
                "w:shd" => {
                    assert_no_leak(|| black_box(read_shading(Some(el))));
                }
                "w:rPrChange" | "w:pPrChange" | "w:ins" | "w:del" => {
                    assert_no_leak(|| black_box(read_revision_info(el)));
                }
                "w14:scene3d" | "w14:props3d" => {
                    assert_no_leak(|| black_box(element_xml(el)));
                }
                "w14:solidFill" | "w14:srgbClr" | "w14:glow" => {
                    assert_no_leak(|| black_box(read_color(Some(el))));
                }
                "w14:gradFill" | "w14:textFill" | "w14:textOutline" => {
                    assert_no_leak(|| black_box(read_fill(Some(el))));
                }
                _ => {
                    assert_no_leak(|| black_box(on_off_val(el)));
                }
            }
        }
    }
}

#[test]
fn m1_absent_and_numeric_helpers() {
    assert_no_leak(|| black_box((read_rpr(None), read_ppr(None), read_tbl_pr(None), read_tr_pr(None), read_tc_pr(None))));
    assert_no_leak(|| black_box((read_border(None), read_shading(None), read_color(None), read_fill(None))));
    for v in ["", "12", "12pt", "1.5.2mm", ".", " 7 ", "abc", "1e3", "99999999999999999999pt"] {
        assert_no_leak(|| black_box((int(Some(v)), measure_twips(Some(v)), split_measure(v))));
    }
}

#[test]
fn m1_values_json() {
    let d = parse(
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:rPr><w:b/><w:sz w:val=\"2\"/></w:rPr></w:document>",
        &DOCX,
    )
    .unwrap();
    let r = read_rpr(d.root().first_child("w:rPr")).unwrap();
    assert_no_leak(|| {
        let mut w = JsonWriter::new(None);
        r.json(&mut w);
        black_box(w.finish())
    });
    assert_no_leak(|| black_box(r.clone()));
}
