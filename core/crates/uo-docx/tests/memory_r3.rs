//! Memory gate M1 of frontier R3: every public function of `read::drawing`, `read::vml_shape`,
//! `read::page_background` and `read::watermark` returns the live bytes exactly to their value
//! before the call, output dropped, on the elements of the quirk corpus (including the ones that
//! read nothing and the ones whose generated text does not parse: the error path of
//! `vml_textbox_to_drawing`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::assert_no_leak;
use uo_docx::js::JsonWriter;
use uo_docx::model::MediaStore;
use uo_docx::model::media::{ImageData, MediaRef};
use uo_docx::read::DOCX_POS;
use uo_docx::read::drawing::{
    anchor_child, ends_with_ascii_lower, find_deep_drawing, parse_vml_style, read_cust_geom, read_sp_pr, read_wp_anchor, split_vml_len,
    srgb_of, text_of_drawing, val_int_group, vml_anchor, vml_color, vml_len_twips, vml_shape_style,
};
use uo_docx::read::page_background::read_page_background;
use uo_docx::read::vml_shape::{VmlLang, vml_single_textbox, vml_textbox_to_drawing};
use uo_docx::read::watermark::{
    attr, find_open_tag, find_shape, has_word_prefixed, is_watermark_xml, read_watermark_xml, strip_quotes, style_prop,
};
use uo_opc::xml::{Document, parse};

fn quirks() -> Vec<Document> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/drawing.quirks.txt")).unwrap();
    text.lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let t = l.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\\\", "\\");
            parse(&t, &DOCX_POS).ok()
        })
        .collect()
}

#[test]
fn m1_readers_over_the_quirk_corpus() {
    let docs = quirks();
    assert!(docs.len() > 1500);
    let (mut anchors, mut shapes, mut boxes, mut errors, mut bgs, mut picts) = (0, 0, 0, 0, 0, 0);
    for d in &docs {
        let root = d.root();
        for el in std::iter::once(root).chain(root.descendants()) {
            match el.name() {
                "wp:anchor" => {
                    anchors += 1;
                    assert_no_leak(|| black_box(read_wp_anchor(Some(el))));
                    assert_no_leak(|| black_box(anchor_child(el, "wp:positionH")));
                    assert_no_leak(|| JsonWriter::stringify(&read_wp_anchor(Some(el)), None));
                }
                "wps:wsp" => {
                    shapes += 1;
                    assert_no_leak(|| black_box(read_sp_pr(el.first_child("wps:spPr"), el.first_child("wps:style"))));
                    assert_no_leak(|| black_box(read_sp_pr(el.first_child("wps:spPr"), None)));
                    assert_no_leak(|| JsonWriter::stringify(&read_sp_pr(el.first_child("wps:spPr"), el.first_child("wps:style")), None));
                }
                "pic:pic" => {
                    assert_no_leak(|| black_box(read_sp_pr(el.first_child("pic:spPr"), None)));
                }
                "a:custGeom" => {
                    assert_no_leak(|| black_box(read_cust_geom(el, 1_905_000.0, 952_500.0)));
                    assert_no_leak(|| black_box(read_cust_geom(el, 0.0, f64::NAN)));
                }
                "v:shape" | "v:rect" | "v:roundrect" | "v:oval" | "v:line" => {
                    assert_no_leak(|| black_box(parse_vml_style(el.attr("style"))));
                    assert_no_leak(|| black_box(vml_shape_style(el, "rect")));
                    assert_no_leak(|| black_box(vml_anchor(&parse_vml_style(el.attr("style")), el.descend("w10:wrap"))));
                    for lang in [VmlLang::Pt, VmlLang::En, VmlLang::Es] {
                        let r = assert_no_leak(|| black_box(vml_textbox_to_drawing(el, 7.0, lang)));
                        let _ = r;
                    }
                    match vml_textbox_to_drawing(el, 7.0, VmlLang::Pt) {
                        Ok(Some(_)) => boxes += 1,
                        Err(_) => errors += 1,
                        Ok(None) => {}
                    }
                    shapes += 1;
                }
                "w:pict" => {
                    picts += 1;
                    assert_no_leak(|| black_box(vml_single_textbox(el)));
                    let xml = el.source_slice().unwrap_or("");
                    assert_no_leak(|| black_box(is_watermark_xml(Some(xml))));
                    assert_no_leak(|| black_box(read_watermark_xml(xml, &|v| vml_color(v))));
                    assert_no_leak(|| black_box(find_shape(xml)));
                }
                "w:background" => {
                    bgs += 1;
                    assert_no_leak(|| {
                        let mut media = MediaStore::new();
                        let bg = read_page_background(el, &mut |rid| {
                            (rid.len() % 2 == 0).then(|| ImageData {
                                data: MediaRef(media.add(rid.as_bytes())),
                                ext: "png".into(),
                                ..ImageData::default()
                            })
                        });
                        black_box((bg, media))
                    });
                    assert_no_leak(|| black_box(read_page_background(el, &mut |_| None)));
                }
                _ => {}
            }
            assert_no_leak(|| black_box(srgb_of(Some(el))));
            assert_no_leak(|| black_box(find_deep_drawing(el, "a:ext").is_some()));
            assert_no_leak(|| black_box(text_of_drawing(el)));
        }
    }
    assert!(
        anchors > 100 && shapes > 100 && boxes > 50 && errors > 10 && bgs > 50 && picts > 100,
        "{anchors} {shapes} {boxes} {errors} {bgs} {picts}"
    );
}

#[test]
fn m1_string_functions() {
    let samples = [
        "",
        "12pt",
        " 5 cm ",
        "-.5in",
        "val -12",
        "#abc",
        "#12345😀",
        "silver",
        "position:absolute;width:10pt;__proto__:x;flip:xy",
        "font-family:&quot;A&quot;;font-size:12pt",
        "<v:shape id=\"PowerPlusWaterMarkObject1\" style=\"width:1in;height:2in;rotation:315\"><v:textpath string=\"X\" style=\"font-size:20pt\"/></v:shape>",
        "<v:shape id=\"WordPictureWatermark2\"><v:imagedata r:id=\"rId1\" gain=\"1\"/></v:shape>",
        "\"'\"",
    ];
    for s in samples {
        assert_no_leak(|| black_box(vml_len_twips(Some(s))));
        assert_no_leak(|| black_box(split_vml_len(s)));
        assert_no_leak(|| black_box(vml_color(Some(s))));
        assert_no_leak(|| black_box(parse_vml_style(Some(s))));
        assert_no_leak(|| black_box(val_int_group(s)));
        assert_no_leak(|| black_box(ends_with_ascii_lower(s)));
        assert_no_leak(|| black_box(attr(s, "style")));
        assert_no_leak(|| black_box(style_prop(s, "width")));
        assert_no_leak(|| black_box(find_shape(s)));
        assert_no_leak(|| black_box(find_open_tag(s, "v:shape")));
        assert_no_leak(|| black_box(has_word_prefixed(s, "gain=")));
        assert_no_leak(|| black_box(strip_quotes(s)));
        assert_no_leak(|| black_box(is_watermark_xml(Some(s))));
        assert_no_leak(|| black_box(read_watermark_xml(s, &|v| vml_color(v))));
    }
}
