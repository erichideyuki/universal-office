//! B5a SmartArt: unit tests of the pieces, M1 (memory returns to the start), M5 (the metrics tables are
//! static, no cache to cap) and bounded no-panic over hostile data models. Parity with the TS is in
//! `uo-pptx/tests/replay.rs` (`b5a_smartart_decks_match_the_ts`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use uo_dml_render::model::Xfrm;
use uo_dml_render::smartart::metrics::{DiagramFont, measure_approx};
use uo_dml_render::smartart::xt::{X, XC, from_element, to_xml};
use uo_dml_render::smartart::{diagram_fallback_shapes, diagram_styler, regenerate_diagram};
use uo_dml_render::source::parse_xml;
use uo_dml_render::theme::Theme;

const NS: &str = "xmlns:dgm=\"http://schemas.openxmlformats.org/drawingml/2006/diagram\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

fn x(text: &str) -> X {
    from_element(parse_xml(text.to_string()).unwrap().root()).unwrap()
}

fn layout() -> X {
    x(&format!(
        "<dgm:layoutDef {NS} uniqueId=\"urn:microsoft.com/office/officeart/2005/8/layout/default\"><dgm:layoutNode name=\"diagram\"><dgm:alg type=\"snake\"><dgm:param type=\"grDir\" val=\"tL\"/></dgm:alg><dgm:shape r:blip=\"\"><dgm:adjLst/></dgm:shape><dgm:presOf/><dgm:constrLst><dgm:constr type=\"w\" for=\"ch\" forName=\"node\" refType=\"w\"/><dgm:constr type=\"h\" for=\"ch\" forName=\"node\" refType=\"w\" refFor=\"ch\" refForName=\"node\" fact=\"0.6\"/><dgm:constr type=\"primFontSz\" for=\"ch\" forName=\"node\" op=\"equ\" val=\"65\"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name=\"n3\" axis=\"ch\" ptType=\"node\"><dgm:layoutNode name=\"node\"><dgm:alg type=\"tx\"/><dgm:shape type=\"rect\" r:blip=\"\"><dgm:adjLst/></dgm:shape><dgm:presOf axis=\"desOrSelf\" ptType=\"node\"/><dgm:constrLst><dgm:constr type=\"lMarg\" refType=\"primFontSz\" fact=\"0.3\"/></dgm:constrLst><dgm:ruleLst><dgm:rule type=\"primFontSz\" val=\"5\" fact=\"NaN\" max=\"NaN\"/></dgm:ruleLst></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"
    ))
}

fn data(n: usize) -> X {
    let mut pts = String::from("<dgm:pt modelId=\"{D}\" type=\"doc\"/>");
    let mut cx = String::new();
    for i in 0..n {
        pts.push_str(&format!(
            "<dgm:pt modelId=\"{{N{i}}}\"><dgm:t><a:bodyPr/><a:p><a:r><a:t>Item number {i}</a:t></a:r></a:p></dgm:t></dgm:pt>"
        ));
        cx.push_str(&format!("<dgm:cxn modelId=\"{{C{i}}}\" srcId=\"{{D}}\" destId=\"{{N{i}}}\" srcOrd=\"{i}\"/>"));
    }
    x(&format!("<dgm:dataModel {NS}><dgm:ptLst>{pts}</dgm:ptLst><dgm:cxnLst>{cx}</dgm:cxnLst></dgm:dataModel>"))
}

#[test]
fn metrics_follow_the_tables() {
    let f = DiagramFont { family: Some("Calibri"), ..DiagramFont::default() };
    let w = measure_approx("Hello world", 18.0, &f);
    assert!(w > 0.0 && w < 18.0 * 12700.0 * 11.0);
    assert!(measure_approx("Hello", 18.0, &DiagramFont { bold: true, ..f }) > measure_approx("Hello", 18.0, &f));
    assert_eq!(measure_approx("", 18.0, &f), 0.0);
    // CJK counts one em
    assert_eq!(measure_approx("\u{4e2d}", 10.0, &f), 10.0 * 12700.0);
}

#[test]
fn a_basic_list_regenerates_one_shape_per_node() {
    let th = Theme::default();
    let tree = regenerate_diagram(Some(&data(4)), Some(&layout()), None, None, 8_000_000.0, 5_000_000.0, &th).unwrap();
    let text = to_xml(&tree);
    assert_eq!(text.matches("<dsp:sp ").count(), 4, "{text}");
    assert!(text.contains("Item number 3"));
    let again = regenerate_diagram(Some(&data(4)), Some(&layout()), None, None, 8_000_000.0, 5_000_000.0, &th).unwrap();
    assert_eq!(tree, again);
}

#[test]
fn unsupported_layouts_and_empty_frames_regenerate_nothing() {
    let th = Theme::default();
    let mut l = layout();
    l.children.push(XC::E(x("<dgm:alg type=\"venn\"/>")));
    assert!(regenerate_diagram(Some(&data(2)), Some(&l), None, None, 1e6, 1e6, &th).is_none());
    assert!(regenerate_diagram(Some(&data(2)), Some(&layout()), None, None, 0.0, 1e6, &th).is_none());
    assert!(regenerate_diagram(None, Some(&layout()), None, None, 1e6, 1e6, &th).is_none());
}

#[test]
fn the_fallback_lays_out_known_families_only() {
    let frame = Xfrm { x: 0.0, y: 0.0, cx: 8e6, cy: 5e6, ..Xfrm::default() };
    let th = Theme::default();
    let st = diagram_styler(None, None, &th);
    let d = data(3);
    let shapes = diagram_fallback_shapes(&d, "urn:x/layout/default", &frame, &th, Some(&st)).unwrap();
    assert_eq!(shapes.len(), 3);
    assert!(diagram_fallback_shapes(&d, "urn:x/layout/unknown", &frame, &th, Some(&st)).unwrap().is_empty());
}

#[test]
fn m1_every_operation_returns_its_bytes() {
    let (d, l) = (data(6), layout());
    let th = Theme::default();
    // the width tables are built once and kept (`OnceLock`): warm them up, they are not a leak
    let _ = regenerate_diagram(Some(&d), Some(&l), None, None, 8e6, 5e6, &th);
    uo_alloc::assert_no_leak(|| regenerate_diagram(Some(&d), Some(&l), None, None, 8e6, 5e6, &th));
    uo_alloc::assert_no_leak(|| regenerate_diagram(Some(&d), None, None, None, 8e6, 5e6, &th));
    let frame = Xfrm { x: 0.0, y: 0.0, cx: 8e6, cy: 5e6, ..Xfrm::default() };
    uo_alloc::assert_no_leak(|| {
        let st = diagram_styler(None, None, &th);
        diagram_fallback_shapes(&d, "x/default", &frame, &th, Some(&st))
    });
}

#[test]
fn m5_the_metrics_tables_are_static() {
    // `diagram-metrics.ts` caches the decoded widths per face; here they are built once (`OnceLock`) from
    // constants: nothing grows with the input, so there is no cap to exercise. clear_caches stays a no-op.
    for fam in ["Calibri", "Arial", "Times New Roman", "Cambria", "Aptos", "Aptos Narrow", "Verdana", "Zzz"] {
        for t in ["a", "Wide WORDS", "\u{e9}\u{4e2d}\t"] {
            assert!(measure_approx(t, 12.0, &DiagramFont { family: Some(fam), ..DiagramFont::default() }).is_finite());
        }
    }
    uo_dml_render::clear_caches();
}

fn bounded(label: &str, f: impl FnOnce()) {
    let t = Instant::now();
    f();
    assert!(t.elapsed() < Duration::from_secs(30), "{label} took {:?}", t.elapsed());
}

#[test]
fn hostile_models_are_bounded() {
    let th = Theme::default();
    let l = layout();
    let cyc = x(&format!(
        "<dgm:dataModel {NS}><dgm:ptLst><dgm:pt modelId=\"d\" type=\"doc\"/><dgm:pt modelId=\"a\"/><dgm:pt modelId=\"b\"/></dgm:ptLst><dgm:cxnLst><dgm:cxn srcId=\"d\" destId=\"a\"/><dgm:cxn srcId=\"a\" destId=\"b\"/><dgm:cxn srcId=\"b\" destId=\"a\"/><dgm:cxn srcId=\"a\" destId=\"a\"/><dgm:cxn srcId=\"d\" destId=\"zz\"/></dgm:cxnLst></dgm:dataModel>"
    ));
    bounded("cycle", || {
        let _ = regenerate_diagram(Some(&cyc), Some(&l), None, None, 1e6, 1e6, &th);
        let frame = Xfrm { cx: 1e6, cy: 1e6, ..Xfrm::default() };
        let _ = diagram_fallback_shapes(&cyc, "x/orgChart1", &frame, &th, None);
    });
    bounded("many nodes", || {
        let big = data(5_000);
        let _ = regenerate_diagram(Some(&big), Some(&l), None, None, 1e7, 1e7, &th);
    });
    bounded("numbers", || {
        let d = data(3);
        for (cx, cy) in [(1e300, 1e300), (1e-300, 1e-300), (f64::INFINITY, 5.0), (f64::NAN, 5.0), (1.0, 1.0)] {
            let _ = regenerate_diagram(Some(&d), Some(&l), None, None, cx, cy, &th);
        }
        let inf = x(&format!(
            "<dgm:layoutDef {NS}><dgm:layoutNode name=\"a\"><dgm:alg type=\"lin\"/><dgm:constrLst><dgm:constr type=\"w\" val=\"INF\"/><dgm:constr type=\"h\" val=\"NaN\"/><dgm:constr type=\"primFontSz\" val=\"1e9\"/></dgm:constrLst><dgm:ruleLst><dgm:rule type=\"h\" val=\"INF\"/></dgm:ruleLst><dgm:forEach axis=\"ch\" ptType=\"node\" cnt=\"-5\" st=\"1e400\" step=\"-1\"><dgm:layoutNode name=\"n\"><dgm:alg type=\"tx\"/><dgm:shape type=\"rect\"/><dgm:presOf axis=\"self\"/></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"
        ));
        let _ = regenerate_diagram(Some(&d), Some(&inf), None, None, 1e6, 1e6, &th);
    });
    bounded("recursion", || {
        let rec = x(&format!(
            "<dgm:layoutDef {NS}><dgm:layoutNode name=\"a\"><dgm:alg type=\"composite\"/><dgm:forEach name=\"f\" axis=\"self\"><dgm:layoutNode name=\"b\"><dgm:alg type=\"tx\"/><dgm:shape type=\"rect\"/><dgm:presOf axis=\"self\"/><dgm:forEach ref=\"f\"/></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"
        ));
        let _ = regenerate_diagram(Some(&data(2)), Some(&rec), None, None, 1e6, 1e6, &th);
    });
    // deep XML is refused by the converter (the TS recursion would throw)
    let deep = format!("{}{}", "<a:x>".repeat(5000), "</a:x>".repeat(5000));
    assert!(from_element(parse_xml(deep).unwrap().root()).is_none());
}
