//! Unit tests of frontier R1 (`read::props`, `read::text_effects`): the cases of
//! `tests/props_unit.cases` (written by `js/unit-cases.mjs` from the frozen TS: the reader's
//! `JSON.stringify` output, `measureTwips`, `elementXml`) and the behaviours the cases cannot
//! show (absent input, raw capture, the iterative chains on a small stack).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::js::{JsObject, JsValue, JsonWriter, Slot};
use uo_docx::read::DOCX;
use uo_docx::read::props::{
    int, measure_twips, on_off_val, read_border, read_ppr, read_revision_info, read_rpr, read_shading, read_tbl_pr, read_tc_pr, read_tr_pr,
    split_measure,
};
use uo_docx::read::text_effects::{element_xml, read_color, read_fill, read_text_effects};
use uo_opc::xml::{Document, parse};

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" xmlns:w16du=\"x\"";

fn doc(inner: &str) -> Document {
    parse(format!("<w:document {NS}>{inner}</w:document>"), &DOCX).unwrap()
}

fn json(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap()
}

fn cases() -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/props_unit.cases")).unwrap();
    text.lines().filter(|l| !l.starts_with('#')).map(|l| l.split('\t').map(str::to_owned).collect()).collect()
}

#[test]
fn properties_match_the_ts_json() {
    let mut n = 0;
    for c in cases().iter().filter(|c| c[0] == "prop") {
        let d = doc(&format!("<w:{k}>{inner}</w:{k}>", k = c[1], inner = c[2]));
        let el = d.root().first_child(&format!("w:{}", c[1]));
        let got = match c[1].as_str() {
            "rPr" => json(&read_rpr(el).unwrap()),
            "pPr" => json(&read_ppr(el).unwrap()),
            "tblPr" => json(&read_tbl_pr(el).unwrap()),
            "trPr" => json(&read_tr_pr(el).unwrap()),
            _ => json(&read_tc_pr(el).unwrap()),
        };
        assert_eq!(got, c[3], "{} {}", c[1], c[2]);
        n += 1;
    }
    assert_eq!(n, 16);
}

#[test]
fn measure_twips_matches_the_ts() {
    let mut n = 0;
    for c in cases().iter().filter(|c| c[0] == "measure") {
        let got = match measure_twips(Some(&c[1])) {
            None => "undefined".to_owned(),
            Some(x) if x == 0.0 && x.is_sign_negative() => "-0".to_owned(),
            Some(x) => uo_jsnum::format::to_js_string(x),
        };
        assert_eq!(got, c[2], "measure {:?}", c[1]);
        n += 1;
    }
    assert_eq!(n, 22);
    assert_eq!(measure_twips(None), None);
}

#[test]
fn element_xml_matches_the_ts() {
    let mut n = 0;
    for c in cases().iter().filter(|c| c[0] == "xml") {
        let d = doc(&format!("<w14:scene3d a=\"1\" 1=\"x\" b=\"&amp;&quot;\">{}</w14:scene3d>", c[1]));
        assert_eq!(element_xml(d.root().first_child("w14:scene3d").unwrap()), c[2], "elementXml {:?}", c[1]);
        n += 1;
    }
    assert_eq!(n, 5);
}

#[test]
fn element_xml_uses_the_raw_capture() {
    let d = doc("<w:drawing a=\"1\"><x/>  text</w:drawing><w:p><w:drawing/></w:p>");
    assert_eq!(element_xml(d.root().first_child("w:drawing").unwrap()), "<w:drawing a=\"1\"><x/>  text</w:drawing>");
    let p = d.root().first_child("w:p").unwrap();
    assert_eq!(element_xml(p), "<w:p><w:drawing/></w:p>");
}

#[test]
fn absent_input_is_absent() {
    assert!(read_rpr(None).is_none());
    assert!(read_ppr(None).is_none());
    assert!(read_tbl_pr(None).is_none());
    assert!(read_tr_pr(None).is_none());
    assert!(read_tc_pr(None).is_none());
    assert!(read_border(None).is_none());
    assert!(read_shading(None).is_none());
    assert!(read_color(None).is_none());
    assert!(read_fill(None).is_none());
    assert_eq!(int(None), None);
    assert_eq!(int(Some("")), None);
}

#[test]
fn int_and_measure_helpers() {
    assert_eq!(int(Some(" 7 ")), Some(7.0));
    assert_eq!(int(Some("7.9")), Some(7.0));
    assert_eq!(int(Some("abc")), None);
    assert_eq!(int(Some("1e1000")), Some(1.0));
    assert_eq!(int(Some("-0")).map(f64::is_sign_negative), Some(true));
    assert_eq!(split_measure("-12.5pt"), Some(("-12.5", Some("pt"))));
    assert_eq!(split_measure("12"), Some(("12", None)));
    assert_eq!(split_measure("12 pt"), None);
    assert_eq!(split_measure(""), None);
    assert!(measure_twips(Some(".")).is_some_and(f64::is_nan));
    assert_eq!(measure_twips(Some("")), None);
}

#[test]
fn on_off_and_revision_info() {
    let d = doc(
        "<w:a/><w:b w:val=\"none\"/><w:c w:val=\"off\"/><w:d w:val=\"1\"/><w:e w:val=\"\"/><w:moveTo w:author=\"x\" w:date=\"\" w16du:dateUtc=\"u\"/><w:ins/>",
    );
    let on: Vec<bool> = d.root().child_elements().take(5).map(on_off_val).collect();
    assert_eq!(on, [true, false, false, true, true]);
    let info = read_revision_info(d.root().first_child("w:moveTo").unwrap());
    assert_eq!(json(&info), "{\"author\":\"x\",\"dateUtc\":\"u\",\"move\":true}");
    assert_eq!(json(&read_revision_info(d.root().first_child("w:ins").unwrap())), "{\"author\":\"\"}");
}

#[test]
fn key_order_follows_the_xml_and_the_first_assignment() {
    let d = doc("<w:rPr><w:sz w:val=\"4\"/><w:b/><w:sz w:val=\"6\"/><w:color w:val=\"A\"/><w:b w:val=\"0\"/></w:rPr>");
    let r = read_rpr(d.root().first_child("w:rPr")).unwrap();
    assert_eq!(json(&r), "{\"sz\":6,\"b\":false,\"color\":{\"val\":\"A\"}}");
    assert_eq!(r.keys.recorded().len(), 3);
    let order: Vec<&str> = r.slots_in_order().iter().map(|(k, _)| *k).collect();
    assert_eq!(order, ["sz", "b", "color"]);
}

#[test]
fn no_key_is_stored_as_undefined() {
    let d = doc("<w:rPr><w:rStyle/><w:w w:val=\"x\"/><w:rPrChange/></w:rPr>");
    let r = read_rpr(d.root().first_child("w:rPr")).unwrap();
    assert!(r.cleared.is_empty());
    assert!(matches!(r.slot(0), Slot::Absent));
    assert!(r.change.is_some());
}

#[test]
fn text_effects_are_absent_without_keys() {
    let d = doc("<w:rPr><w:b/><w14:glow/><w14:shadow/></w:rPr>");
    let (fx, ot) = read_text_effects(d.root().first_child("w:rPr").unwrap());
    assert!(fx.is_none() && ot.is_none());
    let r = read_rpr(d.root().first_child("w:rPr")).unwrap();
    assert!(r.fx.is_none() && r.ot.is_none());
}

fn chain(levels: usize, kind: &str, leaf: &str) -> String {
    let mut x = String::from(leaf);
    for i in 0..levels {
        x = format!("{leaf}<w:{kind}Change w:author=\"a{i}\"><w:{kind}>{x}</w:{kind}></w:{kind}Change>");
    }
    format!("<w:{kind}>{x}</w:{kind}>")
}

#[test]
fn change_chains_are_iterative() {
    let handle = std::thread::Builder::new()
        .stack_size(192 * 1024)
        .spawn(|| {
            let d = doc(&chain(480, "rPr", "<w:b/>"));
            let mut depth = 0;
            let mut cur = read_rpr(d.root().first_child("w:rPr")).unwrap();
            while let Some(c) = cur.change.take() {
                depth += 1;
                cur = *c.r_pr;
            }
            assert_eq!(depth, 480);
            let d = doc(&chain(480, "pPr", "<w:keepNext/>"));
            let mut depth = 0;
            let mut cur = read_ppr(d.root().first_child("w:pPr")).unwrap();
            while let Some(c) = cur.change.take() {
                depth += 1;
                cur = *c.p_pr;
            }
            assert_eq!(depth, 480);
            let many = format!("<w:rPr>{}</w:rPr>", "<w:rPrChange w:author=\"x\"><w:rPr><w:i/></w:rPr></w:rPrChange>".repeat(2000));
            let d = doc(&many);
            let r = read_rpr(d.root().first_child("w:rPr")).unwrap();
            assert_eq!(r.change.unwrap().r_pr.i, Some(true));
            let deep = format!("{}text{}", "<w14:a>".repeat(900), "</w14:a>".repeat(900));
            let d = doc(&deep);
            let xml = element_xml(d.root().first_child("w14:a").unwrap());
            assert_eq!(xml.len(), 900 * ("<w14:a>".len() + "</w14:a>".len()) + 4);
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn colour_and_fill_helpers() {
    let d = doc("<w14:solidFill><w14:srgbClr w14:val=\"ab\"><w14:alpha w14:val=\"5\"/><x:y w14:val=\"7\"/></w14:srgbClr></w14:solidFill>");
    let solid = d.root().first_child("w14:solidFill");
    assert_eq!(json(&read_color(solid).unwrap()), "{\"rgb\":\"AB\",\"mods\":[[\"alpha\",5],[\"\",7]]}");
    let d = doc(
        "<w14:gradFill><w14:gsLst><w14:gs w14:pos=\"0\"><w14:schemeClr/></w14:gs><w14:gs/></w14:gsLst><w14:path w14:path=\"rect\"><w14:fillToRect w14:l=\"5\"/></w14:path></w14:gradFill>",
    );
    let fill = read_fill(d.root().first_child("w14:gradFill").map(|_| d.root())).unwrap();
    assert_eq!(
        json(&fill),
        "{\"type\":\"grad\",\"stops\":[{\"pos\":0,\"color\":{\"scheme\":\"tx1\"}}],\"path\":{\"kind\":\"rect\",\"l\":5,\"t\":0,\"r\":0,\"b\":0}}"
    );
    let d = doc("<w14:solidFill/>");
    assert!(read_fill(Some(d.root())).is_none());
}
