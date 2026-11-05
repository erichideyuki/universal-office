//! Memory gate (M1) of frontier W5 (`write::emit::background`): every public function runs and its
//! outputs are dropped; the live bytes must return exactly. Includes the error paths of
//! `write_page_background` (a document with a malformed relationships part, a package without the
//! main part) and a package edit with an image.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_docx::model::media::{ImageData, MediaRef, MediaStore};
use uo_docx::read::DOCX;
use uo_docx::read::page_background::read_page_background;
use uo_docx::write::emit::background::{
    BackgroundAdded, WORD_PATTERNS, WatermarkBox, WatermarkIds, WatermarkLayout, WatermarkPictureSpec, WatermarkSpec, WatermarkTextSpec,
    add_default_extension, auto_text_box, explicit_text_box, one_color_gradient_color2, page_background_xml, pattern_gif, picture_box,
    resolve_color2, vml_color_attr, watermark_gallery, watermark_run_xml, watermark_texts, write_page_background,
};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::xml::parse;

const BG: &str = "<w:background xmlns:w=\"w\" xmlns:v=\"v\" xmlns:o=\"o\" xmlns:r=\"r\" w:color=\"FF0000\"><v:background id=\"_x0000_s1025\" fillcolor=\"red\"><v:fill type=\"frame\" r:id=\"rId2\" o:title=\"a&amp;b.png\"/></v:background></w:background>";
const GRAD: &str = "<w:background xmlns:w=\"w\" xmlns:v=\"v\" xmlns:o=\"o\" xmlns:r=\"r\"><v:background fillcolor=\"#abc\"><v:fill type=\"gradient\" color2=\"fill darken(128)\" angle=\"-90\" focus=\"50%\" focusposition=\".5,1\" colors=\"0 red;1 blue\" method=\"linear sigma\"/></v:background></w:background>";

fn package(doc: &str, rels: Option<&str>) -> Package {
    const CT: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";
    let mut parts: Vec<(String, Vec<u8>)> =
        vec![("[Content_Types].xml".into(), CT.as_bytes().to_vec()), ("word/document.xml".into(), doc.as_bytes().to_vec())];
    if let Some(r) = rels {
        parts.push(("word/_rels/document.xml.rels".into(), r.as_bytes().to_vec()));
    }
    Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default())
}

fn model(xml: &str, media: &mut MediaStore) -> uo_docx::model::background::PageBackground {
    let doc = parse(xml, &DOCX).unwrap();
    read_page_background(doc.root(), &mut |_| {
        Some(ImageData { data: MediaRef(media.add(b"IMAGE")), ext: "jpg".into(), ..ImageData::default() })
    })
    .unwrap()
}

#[test]
fn m1_pure_functions_return_the_memory() {
    for hex in ["FF0000", "#aabbcc", "", "ÀÀÀÀÀÀ", "abc"] {
        assert_no_leak(|| black_box(vml_color_attr(hex)));
    }
    for c2 in [None, Some("fill"), Some("FILL darken(5)"), Some("blue"), Some("x")] {
        assert_no_leak(|| black_box(resolve_color2(c2, "FF8800")));
    }
    for d in [0.0, 0.25, 0.5, 0.75, 1.0, f64::NAN] {
        assert_no_leak(|| black_box(one_color_gradient_color2(d)));
    }
    assert_no_leak(|| black_box(pattern_gif(&[0.0, 255.0, 170.0, 85.0, 1.0, 2.0, 4.0, 8.0])));
    assert_no_leak(|| black_box(pattern_gif(&[])));
    assert_no_leak(|| black_box(add_default_extension("<Types xmlns=\"x\"></Types>", "a$&b")));
    assert_no_leak(|| black_box(watermark_gallery()));
    assert_no_leak(|| black_box(watermark_texts("pt")));
    assert_no_leak(|| black_box(WORD_PATTERNS.len()));
    assert_no_leak(|| black_box(auto_text_box("DO NOT COPY", WatermarkLayout::Diagonal, 451.3)));
    assert_no_leak(|| black_box(explicit_text_box(300.2, 40.0, 1.2207)));
    assert_no_leak(|| black_box(picture_box(100.0, 50.0, Some(50.0), 468.0)));
}

#[test]
fn m1_page_background_xml_returns_the_memory() {
    for xml in [BG, GRAD] {
        assert_no_leak(|| {
            let mut media = MediaStore::new();
            let bg = model(xml, &mut media);
            black_box(page_background_xml(&bg, Some("rId7")))
        });
    }
}

#[test]
fn m1_watermark_run_xml_returns_the_memory() {
    let ids = WatermarkIds { n: 1.0, spid: 2049.0, anchor_id: "5C1B2A3D".into() };
    let b = WatermarkBox { width_pt: 468.0, height_pt: 117.5 };
    let text = WatermarkSpec::Text(WatermarkTextSpec {
        text: "A & B".into(),
        font: "Calibri".into(),
        size_pt: Some(40.0),
        color: "C0C0C0".into(),
        semitransparent: true,
        layout: WatermarkLayout::Diagonal,
        lang: None,
    });
    let pic = WatermarkSpec::Picture(WatermarkPictureSpec {
        data: Arc::from(vec![1u8, 2, 3]),
        ext: "png".into(),
        title: "logo.png".into(),
        scale: Some(50.0),
        washout: true,
    });
    assert_no_leak(|| black_box(watermark_run_xml(&text, b, &ids, None, None)));
    assert_no_leak(|| black_box(watermark_run_xml(&pic, b, &ids, Some("rId3"), Some(&|h: &str| format!("vml:{h}")))));
}

#[test]
fn m1_write_page_background_returns_the_memory() {
    const DOC: &str = "<?xml version=\"1.0\"?><w:document xmlns:w=\"w\"><w:background w:color=\"00FF00\"><v:fill r:id=\"rId5\"/></w:background><w:body/></w:document>";
    const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/></Relationships>";
    // image in, then replaced, then removed with `prev` each time
    assert_no_leak(|| {
        let mut pkg = package(DOC, Some(RELS));
        let mut media = MediaStore::new();
        let bg = model(BG, &mut media);
        let first = write_page_background(&mut pkg, &media, Some(&bg), None).unwrap();
        assert!(first.is_some());
        let second = write_page_background(&mut pkg, &media, Some(&bg), first.as_ref()).unwrap();
        let none = write_page_background(&mut pkg, &media, None, second.as_ref()).unwrap();
        black_box(none)
    });
    // no image
    assert_no_leak(|| {
        let mut pkg = package(DOC, None);
        let mut media = MediaStore::new();
        let bg = model(GRAD, &mut media);
        black_box(write_page_background(&mut pkg, &media, Some(&bg), None).unwrap())
    });
    // error paths: a malformed relationships part (the main part cannot be located), and a package
    // without the main part (nothing to write)
    assert_no_leak(|| {
        let mut pkg = package(DOC, Some("<Relationships><Relationship Id=\"rId1\" Type=\"x\"/>"));
        let media = MediaStore::new();
        black_box(write_page_background(&mut pkg, &media, None, None).is_err())
    });
    assert_no_leak(|| {
        let mut pkg = Package::from_parts(Vec::<(String, Vec<u8>)>::new(), &uo_opc::Limits::DEFAULT, &OpenOptions::default());
        let media = MediaStore::new();
        let prev = BackgroundAdded { rid: "rId1".into(), media: "word/media/image1.png".into() };
        black_box(write_page_background(&mut pkg, &media, None, Some(&prev)).unwrap())
    });
}

#[test]
fn m1_helpers_return_the_memory() {
    use uo_docx::write::emit::background::{
        default_vml_color, esc_pb, fill_color2_match, first_open_tag, first_r_id, frac_attr, letters_len, pct_attr, rels_path_of,
        remove_relationship, strip_trailing_digits,
    };
    assert_no_leak(|| black_box(default_vml_color("FF0000")));
    assert_no_leak(|| black_box(esc_pb("a&b\"<c>")));
    assert_no_leak(|| black_box((frac_attr(0.5), pct_attr(33.333), letters_len("a b c"), strip_trailing_digits("x12"))));
    assert_no_leak(|| black_box(fill_color2_match("fill darken(5)").is_some()));
    assert_no_leak(|| black_box((rels_path_of("word/document.xml"), first_r_id("<a r:id=\"rId1\"/>").map(str::len))));
    assert_no_leak(|| black_box(first_open_tag("<Types a=\"1\">", "Types")));
    assert_no_leak(|| black_box(remove_relationship("<Relationships><Relationship Id=\"rId5\"/></Relationships>", "rId5")));
}
