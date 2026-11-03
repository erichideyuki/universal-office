//! Unit tests of frontier R3: the readers of `read::drawing`, `read::vml_shape`,
//! `read::page_background` and `read::watermark` on small inputs whose results were produced by
//! the frozen TS (`JSON.stringify` of its objects against `JsonWriter` of the model: the values
//! and the insertion order of the keys), including the error paths. The parity tests cover the
//! corpus; these pin readable cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::model::MediaStore;
use uo_docx::model::media::{ImageData, MediaRef};
use uo_docx::read::DOCX_POS;
use uo_docx::read::drawing::{
    parse_vml_style, read_cust_geom, read_sp_pr, read_wp_anchor, vml_anchor, vml_color, vml_len_twips, vml_shape_style,
};
use uo_docx::read::page_background::read_page_background;
use uo_docx::read::vml_shape::{VmlLang, vml_single_textbox, vml_textbox_to_drawing};
use uo_docx::read::watermark::{WatermarkInfo, is_watermark_xml, read_watermark_xml};
use uo_opc::xml::{Document, parse};

fn doc(xml: &str) -> Document {
    parse(xml, &DOCX_POS).unwrap()
}

fn json(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap()
}

#[test]
fn anchor_of_a_tight_wrap() {
    let d = doc(
        "<wp:anchor behindDoc=\"1\" distT=\"635\" distL=\"1270\" simplePos=\"1\" relativeHeight=\"251659264\"><wp:simplePos x=\"635\" y=\"1270\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>6350</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"paragraph\"><wp:align>top</wp:align></wp:positionV><wp:wrapTight wrapText=\"bothSides\"><wp:wrapPolygon><wp:start x=\"0\" y=\"0\"/><wp:lineTo x=\"0\" y=\"21600\"/><wp:lineTo x=\"10800\" y=\"21600\"/></wp:wrapPolygon></wp:wrapTight><wp:effectExtent l=\"635\" t=\"0\" r=\"0\" b=\"1270\"/></wp:anchor>",
    );
    assert_eq!(
        json(&read_wp_anchor(Some(d.root()))),
        r#"{"wrap":"tight","posH":{"relativeFrom":"page","offset":10},"posV":{"relativeFrom":"paragraph","align":"top"},"wrapText":"bothSides","wrapBox":{"x0":0,"y0":0,"x1":0.5,"y1":1},"wrapPoly":[[0,0],[0,1],[0.5,1]],"behindDoc":true,"distT":1,"distL":2,"simplePos":{"x":1,"y":2},"relativeHeight":251659264,"effectExtent":{"l":1,"t":0,"r":0,"b":2}}"#
    );
}

#[test]
fn anchor_defaults_and_the_rectangle_polygon() {
    let d = doc("<wp:anchor><wp:wrapSquare/></wp:anchor>");
    assert_eq!(
        json(&read_wp_anchor(Some(d.root()))),
        r#"{"wrap":"square","posH":{"relativeFrom":"column"},"posV":{"relativeFrom":"paragraph"}}"#
    );
    // a rectangle gives the box and no polygon; a later wrapNone keeps the box (the TS variables outlive the loop)
    let d = doc(
        "<wp:anchor><wp:wrapTight><wp:wrapPolygon><wp:start x=\"0\" y=\"0\"/><wp:lineTo x=\"0\" y=\"21600\"/><wp:lineTo x=\"21600\" y=\"21600\"/><wp:lineTo x=\"21600\" y=\"0\"/></wp:wrapPolygon></wp:wrapTight><wp:wrapNone/></wp:anchor>",
    );
    assert_eq!(
        json(&read_wp_anchor(Some(d.root()))),
        r#"{"wrap":"none","posH":{"relativeFrom":"column"},"posV":{"relativeFrom":"paragraph"},"wrapBox":{"x0":0,"y0":0,"x1":1,"y1":1}}"#
    );
}

#[test]
fn sp_pr_gradient_shadow_and_line() {
    let d = doc(
        "<wps:wsp><wps:spPr><a:xfrm rot=\"5400000\" flipH=\"1\"><a:ext cx=\"914400\" cy=\"457200\"/></a:xfrm><a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 16667\"/></a:avLst></a:prstGeom><a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"100000\"><a:srgbClr val=\"00ff00\"/></a:gs><a:gs pos=\"0\"><a:schemeClr val=\"accent1\"><a:lumMod val=\"75000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill><a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FF0000\"><a:alpha val=\"50000\"/></a:srgbClr></a:solidFill><a:prstDash val=\"dash\"/></a:ln><a:effectLst><a:outerShdw blurRad=\"25400\" dist=\"38100\" dir=\"2700000\"><a:srgbClr val=\"000000\"/></a:outerShdw></a:effectLst></wps:spPr><wps:style><a:lnRef idx=\"1\"><a:srgbClr val=\"111111\"/></a:lnRef></wps:style></wps:wsp>",
    );
    let wsp = d.root();
    let st = read_sp_pr(wsp.first_child("wps:spPr"), wsp.first_child("wps:style"));
    assert_eq!(
        json(&st),
        r#"{"geom":"roundRect","adj":{"adj":16667},"rot":90,"flipH":true,"gradient":{"from":"00FF00","to":"@accent1|lumMod=75000","angleDeg":90,"stops":[{"pos":0,"color":"@accent1|lumMod=75000"},{"pos":1,"color":"00FF00"}],"scaled":false,"rotWithShape":true},"fill":"00FF00","shadow":{"dx":42.42640687119285,"dy":42.426406871192846,"color":"000000","blur":40},"lineInset":{"w":1},"line":{"color":"FF0000","widthPt":1,"alpha":0.5,"dash":"dash"}}"#
    );
}

#[test]
fn sp_pr_without_a_shape_properties_element_is_a_rectangle() {
    assert_eq!(json(&read_sp_pr(None, None)), r#"{"geom":"rect"}"#);
}

#[test]
fn custom_geometry_is_evaluated() {
    let d = doc(
        "<a:custGeom><a:avLst><a:gd name=\"adj\" fmla=\"val 50000\"/></a:avLst><a:gdLst><a:gd name=\"x1\" fmla=\"*/ w adj 100000\"/><a:gd name=\"y1\" fmla=\"sin h 5400000\"/></a:gdLst><a:pathLst><a:path w=\"100\" h=\"200\"><a:moveTo><a:pt x=\"0\" y=\"0\"/></a:moveTo><a:lnTo><a:pt x=\"x1\" y=\"y1\"/></a:lnTo><a:arcTo wR=\"10\" hR=\"20\" stAng=\"5400000\" swAng=\"10800000\"/><a:close/></a:path></a:pathLst></a:custGeom>",
    );
    assert_eq!(
        json(&read_cust_geom(d.root(), 914_400.0, 457_200.0)),
        r#"[{"w":100,"h":200,"fill":true,"stroke":true,"cmds":[{"op":"M","x":0,"y":0},{"op":"L","x":457200,"y":457200},{"op":"A","wR":10,"hR":20,"st":90,"sw":180},{"op":"Z"}]}]"#
    );
    // no path, no commands: nothing
    let d = doc("<a:custGeom><a:pathLst><a:path/></a:pathLst></a:custGeom>");
    assert!(read_cust_geom(d.root(), 0.0, 0.0).is_empty());
}

#[test]
fn vml_lengths() {
    let cases: [(&str, Option<f64>); 14] = [
        ("12pt", Some(240.0)),
        ("1in", Some(1440.0)),
        ("5cm", Some(2_834.645_669_291_339)),
        ("10mm", Some(566.929_133_858_267_7)),
        ("3pc", Some(720.0)),
        ("2em", Some(480.0)),
        ("16px", Some(240.0)),
        ("16", Some(240.0)),
        ("-.5in", Some(-720.0)),
        ("1.2.3pt", Some(24.0)),
        (".", Some(f64::NAN)),
        ("", None),
        (" 5 pt ", Some(100.0)),
        ("1e3pt", None),
    ];
    for (v, want) in cases {
        let got = vml_len_twips(Some(v));
        match (got, want) {
            (Some(g), Some(w)) => assert!(g == w || (g.is_nan() && w.is_nan()), "{v}: {g} != {w}"),
            (None, None) => {}
            (g, w) => panic!("{v}: {g:?} != {w:?}"),
        }
    }
    assert_eq!(vml_len_twips(None), None);
}

#[test]
fn vml_colours() {
    let cases = [
        ("#ff0000", Some("FF0000")),
        ("#F0a", Some("FF00AA")),
        ("Red", Some("FF0000")),
        ("window", Some("FFFFFF")),
        ("#1234567", Some("123456")),
        ("#12", None),
        ("fill darken(128)", None),
        ("  #abc def", Some("AABBCC")),
        ("nope", None),
        ("", None),
    ];
    for (v, want) in cases {
        assert_eq!(vml_color(Some(v)).as_deref(), want, "{v:?}");
    }
    assert_eq!(vml_color(None), None);
    // a cut surrogate pair: the TS holds a lone surrogate, Str holds U+FFFD (named in R3.md)
    assert_eq!(vml_color(Some("#12345😀")).as_deref(), Some("12345\u{fffd}"));
}

#[test]
fn vml_style_and_anchor() {
    let style = parse_vml_style(Some("Position:absolute; margin-left : 10pt ;z-index:-3;;bad;__proto__:x;Left:1in"));
    assert_eq!(style.get("position"), Some("absolute"));
    assert_eq!(style.get("margin-left"), Some("10pt"));
    assert_eq!(style.get("__proto__"), None);
    let anchor = vml_anchor(&style, None).unwrap();
    assert_eq!(
        json(&anchor),
        r#"{"wrap":"none","posH":{"relativeFrom":"column","offset":1640},"posV":{"relativeFrom":"paragraph","offset":0},"relativeHeight":-3,"behindDoc":true}"#
    );
    assert!(vml_anchor(&parse_vml_style(Some("position:relative")), None).is_none());
}

#[test]
fn vml_shape_style_fill_stroke_and_flip() {
    let d = doc(
        "<v:oval fillcolor=\"#f00\" strokeweight=\"2pt\" style=\"rotation:30;flip:xy\"><v:stroke color=\"blue\" dashstyle=\"dash\"/></v:oval>",
    );
    assert_eq!(
        json(&vml_shape_style(d.root(), "ellipse")),
        r##"{"geom":"ellipse","fill":"FF0000","line":{"color":"0000FF","widthPt":2,"dash":"dash"},"rot":30,"flipH":true,"flipV":true}"##
    );
    let d = doc("<v:rect filled=\"f\" stroked=\"f\"/>");
    assert_eq!(json(&vml_shape_style(d.root(), "rect")), r#"{"geom":"rect"}"#);
}

const BOX: &str = "<v:shape id=\"_x0000_s1027\" style=\"position:absolute;margin-left:10pt;margin-top:20pt;width:100pt;height:50pt;z-index:-3;mso-position-horizontal-relative:page;rotation:45;flip:x\" fillcolor=\"#ff0000\" strokecolor=\"blue\" strokeweight=\"2pt\" alt=\"Caixa &amp; alt\" visibility=\"hidden\"><v:textbox inset=\"1pt,2pt,3pt,4pt\" style=\"layout-flow:vertical;mso-layout-flow-alt:bottom-to-top\"><w:txbxContent/></v:textbox><w10:wrap type=\"square\"/></v:shape>";

#[test]
fn textbox_drawing_text() {
    let d = doc(BOX);
    let got = vml_textbox_to_drawing(d.root(), 1027.0, VmlLang::Pt).unwrap().unwrap();
    let want = "<w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"3\" behindDoc=\"1\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>127000</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>254000</wp:posOffset></wp:positionV><wp:extent cx=\"1270000\" cy=\"635000\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapSquare wrapText=\"bothSides\"/><wp:docPr id=\"1027\" name=\"Caixa de Texto 1027\" descr=\"Caixa &amp; alt\"/><wp:cNvGraphicFramePr/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\"><wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm rot=\"2700000\" flipH=\"1\"><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:ln w=\"25400\"><a:solidFill><a:srgbClr val=\"0000FF\"/></a:solidFill></a:ln></wps:spPr><wps:txbx><w:txbxContent><w:p/></w:txbxContent></wps:txbx><wps:bodyPr rot=\"0\" spcFirstLastPara=\"0\" vertOverflow=\"overflow\" horzOverflow=\"overflow\" vert=\"vert270\" wrap=\"square\" lIns=\"12700\" tIns=\"25400\" rIns=\"38100\" bIns=\"50800\" numCol=\"1\" spcCol=\"0\" rtlCol=\"0\" fromWordArt=\"0\" anchor=\"t\" anchorCtr=\"0\" forceAA=\"0\" compatLnSpc=\"1\"><a:prstTxWarp prst=\"textNoShape\"><a:avLst/></a:prstTxWarp><a:noAutofit/></wps:bodyPr></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing>";
    assert_eq!(got, want);
    let en = vml_textbox_to_drawing(d.root(), 1027.0, VmlLang::En).unwrap().unwrap();
    assert!(en.contains("name=\"Text Box 1027\""));
    let es = vml_textbox_to_drawing(d.root(), 1027.0, VmlLang::Es).unwrap().unwrap();
    assert!(es.contains("name=\"Cuadro de texto 1027\""));
}

#[test]
fn textbox_drawing_none_cases() {
    // not absolute, no textbox, no size
    for xml in [
        "<v:shape style=\"width:1pt\"><v:textbox/></v:shape>",
        "<v:shape style=\"position:absolute;width:10pt;height:10pt\"/>",
        "<v:shape style=\"position:absolute;width:0;height:10pt\"><v:textbox/></v:shape>",
        "<v:shape style=\"position:absolute;width:x;height:10pt\"><v:textbox/></v:shape>",
    ] {
        let d = doc(xml);
        assert_eq!(vml_textbox_to_drawing(d.root(), 1.0, VmlLang::Pt).unwrap(), None, "{xml}");
    }
}

#[test]
fn textbox_drawing_round_rect_and_no_line() {
    let d = doc(
        "<v:roundrect id=\"Minha\" arcsize=\"5461f\" style=\"position:absolute;width:10pt;height:10pt\" filled=\"f\" stroked=\"f\"><v:textbox/></v:roundrect>",
    );
    let got = vml_textbox_to_drawing(d.root(), 1027.0, VmlLang::Pt).unwrap().unwrap();
    assert!(got.contains("<wp:docPr id=\"1027\" name=\"Minha\"/>"));
    assert!(got.contains("<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 8333\"/></a:avLst></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln>"));
}

#[test]
fn textbox_drawing_error_path() {
    // a raw attribute value that breaks the generated text: the TS patch step throws
    let d = doc("<v:shape style=\"position:absolute;width:10pt;height:10pt\"><v:stroke dashstyle=\"a&quot;b\"/><v:textbox/></v:shape>");
    assert!(vml_textbox_to_drawing(d.root(), 1.0, VmlLang::Pt).is_err());
    // an injected close tag the lenient parser accepts
    let d = doc(
        "<v:shape style=\"position:absolute;width:10pt;height:10pt;mso-position-horizontal:&lt;/wp:align&gt;&lt;x/&gt;\"><v:textbox/></v:shape>",
    );
    let got = vml_textbox_to_drawing(d.root(), 1.0, VmlLang::Pt).unwrap().unwrap();
    assert!(got.contains("<wp:align></wp:align><x/></wp:align>"));
}

#[test]
fn textbox_drawing_prototype_lookups() {
    let d = doc(
        "<v:shape style=\"position:absolute;width:10pt;height:10pt;mso-position-horizontal-relative:constructor\"><v:stroke linestyle=\"toString\"/><v:textbox/></v:shape>",
    );
    let got = vml_textbox_to_drawing(d.root(), 1.0, VmlLang::Pt).unwrap().unwrap();
    assert!(got.contains("<wp:positionH relativeFrom=\"function Object() { [native code] }\">"));
    assert!(got.contains("cmpd=\"function toString() { [native code] }\""));
}

#[test]
fn single_textbox() {
    let d = doc("<w:pict><v:shapetype id=\"t\"/><v:shape><v:textbox/></v:shape></w:pict>");
    let s = vml_single_textbox(d.root()).unwrap();
    assert_eq!(s.name(), "v:shape");
    let d = doc("<w:pict><v:shape><v:textbox/></v:shape><v:rect><v:textbox/></v:rect></w:pict>");
    assert!(vml_single_textbox(d.root()).is_none());
    let d = doc("<w:pict><v:shape/></w:pict>");
    assert!(vml_single_textbox(d.root()).is_none());
}

#[test]
fn page_background_with_effect() {
    let d = doc(
        "<w:background w:color=\"FFFF00\" w:themeColor=\"accent1\"><v:background fillcolor=\"red\" o:targetscreensize=\"1024,768\"><v:fill type=\"gradient\" color2=\"blue\" angle=\"-90\" focus=\"50%\" focusposition=\".5,1\" colors=\"0 red;19661f #66008f\" method=\"linear sigma\" o:title=\"t\" r:id=\"rId9\"><o:fill type=\"gradientCenter\"/></v:fill></v:background></w:background>",
    );
    let mut media = MediaStore::new();
    let bg = read_page_background(d.root(), &mut |rid| {
        (rid == "rId9").then(|| ImageData { data: MediaRef(media.add(&[1, 2])), ext: "png".into(), ..ImageData::default() })
    })
    .unwrap();
    assert_eq!(
        JsonWriter::stringify(&bg, Some(&media)).unwrap(),
        r##"{"color":"FFFF00","themeColor":"accent1","fill":{"type":"gradient","color":"FF0000","color2":"blue","angle":-90,"focus":50,"focusPosition":[0.5,1],"gradientCenter":true,"colors":[{"pos":0,"color":"FF0000"},{"pos":0.3000030517578125,"color":"66008F"}],"method":"linear sigma","title":"t","image":{"data":{"0":1,"1":2},"ext":"png"},"targetScreen":true}}"##
    );
    // nothing: no attributes and no effect
    let d = doc("<w:background/>");
    assert!(read_page_background(d.root(), &mut |_| None).is_none());
    // an unknown fill type is not an effect
    let d = doc("<w:background><v:background><v:fill type=\"solid\"/></v:background></w:background>");
    assert!(read_page_background(d.root(), &mut |_| None).is_none());
}

#[test]
fn watermark_text_and_picture() {
    let wm = "<w:pict><v:shape id=\"PowerPlusWaterMarkObject1\" style=\"position:absolute;width:468pt;height:117pt;rotation:315\" fillcolor=\"silver\"><v:fill opacity=\".5\"/><v:textpath style=\"font-family:&quot;Calibri&quot;;font-size:1pt\" string=\"RASCUNHO\"/></v:shape></w:pict>";
    assert!(is_watermark_xml(Some(wm)));
    let info = read_watermark_xml(wm, &|v| vml_color(v)).unwrap();
    assert_eq!(
        json(&info),
        r#"{"kind":"text","text":"RASCUNHO","font":"Calibri","color":"C0C0C0","semitransparent":true,"layout":"diagonal","widthPt":468,"heightPt":117}"#
    );
    assert!(matches!(info, WatermarkInfo::Text(_)));
    let wp = "<w:pict><v:shape id=\"WordPictureWatermark2\" style=\"width:10in;height:5in\"><v:imagedata r:id=\"rId7\" o:title=\"a&amp;b.png\" gain=\"19661f\"/></v:shape></w:pict>";
    assert_eq!(
        json(&read_watermark_xml(wp, &|v| vml_color(v)).unwrap()),
        r#"{"kind":"picture","rid":"rId7","title":"a&b.png","washout":true,"widthPt":720,"heightPt":360}"#
    );
    // not a watermark
    assert!(!is_watermark_xml(None));
    assert!(!is_watermark_xml(Some("")));
    assert!(!is_watermark_xml(Some("<v:shape id=\"Other1\"/>")));
    assert!(read_watermark_xml("<v:shape id=\"Other1\"></v:shape>", &|v| vml_color(v)).is_none());
}
