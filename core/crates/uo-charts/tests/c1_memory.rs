//! Memory gate of frontier C-1 (record `uo-charts.md` "Memory"): M1 every entry point returns live bytes exactly after
//! its outputs drop; M2 a soak of 2,000 parse cycles is flat; M3 the retained size of a parsed `ChartSpace` against the XML
//! it came from (median over the corpus, target <= 2x); M5 the engine keeps no cache (nothing to cap).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_charts::host::{ColorSource, FileFormat};
use uo_charts::model::ChartColor;
use uo_charts::{Scheme, clear_caches, dml_color, parse_chart_space, parse_paint, parse_paint_node, summarize};

mod c1_support;
use c1_support::corpus::{PlainHost, capture_xmls, corpus, doc};

fn hosts() -> Vec<PlainHost> {
    vec![
        PlainHost { format: None, shapes: None },
        PlainHost { format: Some(FileFormat::Modern), shapes: None },
        PlainHost { format: Some(FileFormat::Mso2007), shapes: None },
    ]
}

#[test]
fn m1_each_entry_point_returns_live_bytes_exactly() {
    let docs: Vec<_> = corpus().iter().map(|x| doc(x)).collect();
    // warm every lazy table (uo-jsnum/uo-text) before measuring
    for h in &hosts() {
        for d in &docs {
            black_box(parse_chart_space(d.root(), h));
        }
    }
    let color = |s: ColorSource<'_>| match s {
        ColorSource::Element(e) => Some(ChartColor::new(&format!("#{}", e.attr("val").unwrap_or("")))),
        ColorSource::Scheme(_) => None,
    };
    for h in &hosts() {
        for (i, d) in docs.iter().enumerate() {
            let u = assert_no_leak(|| parse_chart_space(d.root(), h));
            if i == 1 {
                println!("M1 parse_chart_space (3 series x 40 points): allocated {} B, peak {} B", u.allocated, u.peak_above_start);
            }
            let cs = parse_chart_space(d.root(), h);
            assert_no_leak(|| summarize(&cs));
            // every element of the document as a paint parent and as a fill node
            assert_no_leak(|| {
                d.root().descendants().for_each(|e| {
                    black_box((parse_paint(e, &color), parse_paint_node(e, &color)));
                });
            });
        }
    }
    let sc = Scheme::from_pairs(&[("dk1", "#000000"), ("lt1", "#FFFFFF"), ("accent1", "#156082")]);
    let colours = doc(
        "<r><a:schemeClr val=\"tx1\"><a:lumMod val=\"65000\"/><a:lumOff val=\"35000\"/></a:schemeClr><a:srgbClr val=\"04617B\"><a:satMod val=\"320000\"/><a:hueOff val=\"100\"/></a:srgbClr><a:prstClr val=\"red\"/></r>",
    );
    assert_no_leak(|| {
        colours.root().child_elements().for_each(|e| {
            black_box(dml_color(e, &sc));
        });
    });
    assert_no_leak(|| {
        let s = uo_charts::color::chart_shade_tint("#156082", -0.3);
        let t = uo_charts::color::tint_linear("#156082", 0.75);
        let u = uo_charts::color::adjust_lum("#156082", 0.4);
        let v = uo_charts::color::rgba("#156082", 0.5);
        (s, t, u, v)
    });
    assert_no_leak(clear_caches);
}

#[test]
fn m2_soak_of_parse_cycles_is_flat() {
    let docs: Vec<_> = corpus().iter().map(|x| doc(x)).collect();
    let hs = hosts();
    // warm-up pass
    for d in &docs {
        black_box(parse_chart_space(d.root(), &hs[1]));
    }
    let base = thread_stats().live;
    let series = soak(2000, 50, |i| {
        let d = &docs[i % docs.len()];
        let cs = parse_chart_space(d.root(), &hs[i % hs.len()]);
        black_box(summarize(&cs));
    });
    assert_flat(&series, 1, 0);
    let (n, last) = (series.len(), *series.last().unwrap());
    drop(series);
    assert_eq!(thread_stats().live, base, "live bytes after the soak are the initial ones");
    println!("M2 soak: {n} samples, live bytes {last}, initial {base}");
}

/// M3: the bytes a parsed model keeps against the chart XML (the corpus of `c1a` when its capture is present).
#[test]
fn m3_retained_size_of_the_model() {
    let mut xmls = corpus();
    let captured = capture_xmls("charts-c1a");
    let n_cap = captured.len();
    xmls.extend(captured);
    let h = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let mut ratios: Vec<f64> = Vec::new();
    for x in &xmls {
        let d = doc(x);
        black_box(parse_chart_space(d.root(), &h));
        let (cs, u) = measure(|| parse_chart_space(d.root(), &h));
        if cs.groups.is_empty() {
            continue;
        }
        ratios.push(u.live_delta as f64 / x.len() as f64);
        drop(cs);
    }
    ratios.sort_by(f64::total_cmp);
    let median = ratios[ratios.len() / 2];
    let max = ratios[ratios.len() - 1];
    println!("M3 retained/XML bytes over {} charts ({} from the capture): median {median:.3}, max {max:.3}", ratios.len(), n_cap);
    assert!(median <= 2.0, "median retained/XML {median} exceeds 2");
}
