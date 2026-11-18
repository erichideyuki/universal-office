//! `.ppt` charts (Slides wave 5, record `uo-pptx.md` "`.ppt` charts"): PowerPoint 365 opens an embedded `Excel.Chart.8` OLE
//! object as a NATIVE chart when PowerPoint 2007+ wrote it from one (the shape's round-trip blob says
//! `<a:downRevStg … convChart="1"/>`), and keeps the OLE picture otherwise (measured through the object model: `Shape.Type`
//! msoChart 3 for `pb-graficos-partes.ppt`, msoEmbeddedOLEObject 7 for `pb-graficos.ppt`, whose OLE storages are the same).
//! The converter follows it by default; `ConvertOptions::ts_strict()` keeps the TS's fallback picture.
#![cfg(feature = "ppt")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_pptx::ppt::{ConvertOptions, convert_ppt_to_pptx_parts};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples/playback").join(name)).unwrap()
}

fn parts(name: &str, o: &ConvertOptions) -> Vec<(String, Vec<u8>)> {
    let o = ConvertOptions { today: Some((2026, 11, 18)), ..*o };
    convert_ppt_to_pptx_parts(&sample(name), &o).unwrap()
}

fn text(p: &[(String, Vec<u8>)], name: &str) -> String {
    String::from_utf8(p.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name}")).1.clone()).unwrap()
}

#[test]
fn downgraded_charts_come_back_as_native_charts() {
    let p = parts("pb-graficos-partes.ppt", &ConvertOptions::default());
    let charts: Vec<&String> = p.iter().map(|(n, _)| n).filter(|n| n.starts_with("ppt/charts/chart")).collect();
    // six chart slides (bars by category/series, stacked, pie, line), one chart each
    assert_eq!(charts.len(), 6, "{charts:?}");
    let slide = text(&p, "ppt/slides/slide1.xml");
    assert!(slide.contains("<p:graphicFrame>") && slide.contains("drawingml/2006/chart") && !slide.contains("<p:pic>"), "{slide}");
    // the frame keeps the OLE anchor (808 × 448 pt, PowerPoint's chart frame of this deck)
    assert!(slide.contains("<a:ext cx=\"10261600\" cy=\"5689600\"/>"), "{slide}");
    let rels = text(&p, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("relationships/chart\" Target=\"../charts/chart1.xml\""), "{rels}");
    let ct = text(&p, "[Content_Types].xml");
    assert!(ct.contains("/ppt/charts/chart1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\""));
    let c1 = text(&p, "ppt/charts/chart1.xml");
    // the BIFF8 chart with its Office 2007 XML: three column series in theme colours, the bottom legend of CrtLayout12,
    // the overlap of the .pptx twin (−27; the BAR record holds 27), the light gridlines and grey labels of the XML streams
    for want in [
        "<c:barDir val=\"col\"/>",
        "<c:overlap val=\"-27\"/>",
        "<c:gapWidth val=\"219\"/>",
        "<a:schemeClr val=\"accent1\"/>",
        "<a:schemeClr val=\"accent3\"/>",
        "<c:legendPos val=\"b\"/>",
        "<c:plotArea><c:layout/>",
        "<a:lumMod val=\"15000\"/><a:lumOff val=\"85000\"/>",
        "<c:v>Categoria 4</c:v>",
        "<c:v>4.3</c:v>",
    ] {
        assert!(c1.contains(want), "{want} missing in {c1}");
    }
    // the pie of slide 5 has its title "Vendas"
    let pie =
        p.iter().map(|(n, d)| (n, String::from_utf8_lossy(d))).find(|(n, d)| n.starts_with("ppt/charts/") && d.contains("<c:pieChart>"));
    let (_, pie) = pie.expect("a pie chart");
    assert!(pie.contains("<a:t>Vendas</a:t>") && pie.contains("<c:autoTitleDeleted val=\"0\"/>"), "{pie}");
}

#[test]
fn charts_without_the_round_trip_mark_stay_pictures() {
    let p = parts("pb-graficos.ppt", &ConvertOptions::default());
    assert!(!p.iter().any(|(n, _)| n.starts_with("ppt/charts/")));
    assert!(text(&p, "ppt/slides/slide1.xml").contains("<p:pic>"));
}

#[test]
fn ts_strict_keeps_the_fallback_picture() {
    let p = parts("pb-graficos-partes.ppt", &ConvertOptions::ts_strict());
    assert!(!p.iter().any(|(n, _)| n.starts_with("ppt/charts/")));
    assert!(text(&p, "ppt/slides/slide1.xml").contains("<p:pic>"));
}
