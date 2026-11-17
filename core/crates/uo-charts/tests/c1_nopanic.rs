//! No-panic of frontier C-1 over mutated chart XML (bounded: a few seconds): attribute values replaced by hostile
//! numbers and strings, elements deleted, duplicated and swapped, deep nesting of the chart drawing's groups; every
//! entry point must return, and the model keeps its caps (100,000 points).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use std::time::{Duration, Instant};

use uo_charts::host::FileFormat;
use uo_charts::{Scheme, dml_color, parse_chart_space, parse_paint, parse_paint_node, summarize};
use uo_opc::xml::{ParseOptions, parse};
use uo_testkit::generate::Rng;

mod c1_support;
use c1_support::corpus::{PlainHost, capture_xmls, corpus, doc};

const HOSTILE: [&str; 26] = [
    "",
    "0",
    "-0",
    "1",
    "-1",
    "2",
    "100",
    "99999",
    "100000",
    "100001",
    "4294967296",
    "99999999999999999999",
    "1e308",
    "-1e308",
    "1e-320",
    "NaN",
    "Infinity",
    "-Infinity",
    "0x10",
    "1.5e",
    "abc",
    " 7 ",
    "true",
    "false",
    "\u{1F600}",
    "+5",
];

/// The offset of a random `needle` of `s` (a start of a tag, an attribute value…).
fn pick(s: &str, needle: &str, skip: usize, rng: &mut Rng) -> Option<usize> {
    let marks: Vec<usize> = s.match_indices(needle).map(|(i, _)| i + skip).collect();
    marks.get(rng.below(marks.len().max(1))).copied()
}

/// The end (exclusive) of the self-closing element that starts at `at`, when it has no child.
fn leaf_end(s: &str, at: usize) -> Option<usize> {
    let end = s.get(at..)?.find("/>")?;
    (!s.get(at..at + end)?.contains('>')).then_some(at + end + 2)
}

fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_owned();
    for _ in 0..1 + rng.below(6) {
        match rng.below(6) {
            // replace an attribute value by a hostile one
            0..=2 => {
                if let Some(at) = pick(&s, "=\"", 2, rng)
                    && let Some(end) = s.get(at..).and_then(|t| t.find('"'))
                {
                    s.replace_range(at..at + end, HOSTILE[rng.below(HOSTILE.len())]);
                }
            }
            // delete a self-closing element
            3 => {
                if let Some(at) = pick(&s, "<", 0, rng)
                    && let Some(end) = leaf_end(&s, at)
                {
                    s.replace_range(at..end, "");
                }
            }
            // duplicate a self-closing element
            4 => {
                if let Some(at) = pick(&s, "<", 0, rng)
                    && let Some(end) = leaf_end(&s, at)
                {
                    let piece = s.get(at..end).unwrap_or("").to_owned();
                    s.insert_str(at, &piece);
                }
            }
            // rename an element to another one of the model
            _ => {
                let names =
                    ["c:ser", "c:pt", "c:val", "c:cat", "c:axId", "c:dLbls", "c:spPr", "c:ln", "a:solidFill", "c:legend", "c:title"];
                let a = names[rng.below(names.len())];
                let b = names[rng.below(names.len())];
                s = s.replacen(&format!("<{a}"), &format!("<{b}"), 1);
            }
        }
    }
    s
}

fn exercise(xml: &str, hosts: &[PlainHost]) -> bool {
    let Ok(d) = parse(xml.to_owned(), &ParseOptions::LENIENT) else { return false };
    for h in hosts {
        let cs = parse_chart_space(d.root(), h);
        let sm = summarize(&cs);
        assert!(sm.series.len() <= 100_000 * 4);
        for g in &cs.groups {
            for s in &g.series {
                assert!(s.values.len() <= 100_000);
                assert!(s.cats.as_ref().is_none_or(|c| c.len() <= 100_000));
            }
        }
        // every element as a paint parent and fill node, as a colour with a Sheet scheme
        let sc = Scheme::from_pairs(&[("dk1", "#000000"), ("lt1", "#FFFFFF"), ("accent1", "#156082")]);
        let col = |s: uo_charts::host::ColorSource<'_>| uo_charts::dml_color_source(s, &sc);
        for e in d.root().descendants().take(400) {
            let _ = parse_paint(e, &col);
            let _ = parse_paint_node(e, &col);
            let _ = dml_color(e, &sc);
        }
    }
    true
}

#[test]
fn mutated_charts_never_panic() {
    let hosts = vec![
        PlainHost { format: None, shapes: None },
        PlainHost { format: Some(FileFormat::Modern), shapes: None },
        PlainHost { format: Some(FileFormat::Mso2007), shapes: None },
    ];
    let mut base = corpus();
    base.extend(capture_xmls("charts-c1a").into_iter().take(60));
    let mut rng = Rng::new(0x00c1_0001);
    let deadline = Instant::now() + Duration::from_secs(4);
    let (mut parsed, mut rejected) = (0u32, 0u32);
    while Instant::now() < deadline {
        let src = &base[rng.below(base.len())];
        let m = mutate(src, &mut rng);
        if exercise(&m, &hosts) {
            parsed += 1;
        } else {
            rejected += 1;
        }
    }
    println!("c1 no-panic: {parsed} mutated charts parsed, {rejected} rejected by the XML reader");
    assert!(parsed > 200, "the mutator keeps most charts well formed ({parsed} parsed)");
}

#[test]
fn hostile_drawings_and_extremes() {
    let h = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    // groups nested 20,000 deep in the chart drawing: capped, no stack overflow
    let n = 20_000;
    let mut us = String::from(
        "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to>",
    );
    for _ in 0..n {
        us.push_str("<cdr:grpSp><cdr:grpSpPr><a:xfrm><a:off x=\"1\" y=\"1\"/><a:ext cx=\"0\" cy=\"1e400\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></cdr:grpSpPr>");
    }
    us.push_str("<cdr:sp><cdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"5\" cy=\"5\"/></a:xfrm></cdr:spPr></cdr:sp>");
    for _ in 0..n {
        us.push_str("</cdr:grpSp>");
    }
    us.push_str("</cdr:relSizeAnchor></c:userShapes>");
    let shapes = parse(us, &ParseOptions::LENIENT).unwrap();
    let hd = PlainHost { format: Some(FileFormat::Modern), shapes: Some(shapes) };
    let base = doc("<c:chartSpace><c:chart><c:plotArea/></c:chart></c:chartSpace>");
    let cs = parse_chart_space(base.root(), &hd);
    assert!(cs.user_shapes.as_ref().is_none_or(|u| u.len() <= 1));
    // ptCount and idx at the cap, huge texts, many stops
    let stops: String = (0..5000).map(|i| format!("<a:gs pos=\"{}\"><a:srgbClr val=\"00FF00\"/></a:gs>", (i * 7919) % 100_000)).collect();
    let xml = format!(
        "<c:chartSpace><c:spPr><a:gradFill><a:gsLst>{stops}</a:gsLst></a:gradFill></c:spPr><c:chart><c:plotArea><c:barChart><c:ser><c:val><c:numLit><c:ptCount val=\"1e9\"/><c:pt idx=\"99999\" formatCode=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:val><c:cat><c:strLit><c:ptCount val=\"99999999999\"/></c:strLit></c:cat></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"
    );
    let d = doc(&xml);
    let cs = parse_chart_space(d.root(), &h);
    assert_eq!(cs.groups[0].series[0].values.len(), 100_000);
    assert_eq!(cs.groups[0].series[0].cats.as_ref().unwrap().len(), 100_000);
    assert_eq!(cs.groups[0].series[0].pt_fmts.as_ref().unwrap().len(), 100_000);
    let Some(uo_charts::model::Paint::Grad { stops, .. }) = &cs.sp.fill else { panic!("gradient") };
    assert_eq!(stops.len(), 5000);
    assert!(stops.windows(2).all(|w| w[0].pos <= w[1].pos), "sorted");
}
