//! Memory and no-panic gates of frontier C-2 (record `uo-charts.md` "Memory", "Fuzz targets"): M1 every entry point
//! (`auto_scale`, `tick_values`, `frac`, `nice_step`, `fit_trend`, `trend_num`, `histogram_bins`, `image_dpi`, the chartex
//! parse) returns live bytes exactly after its outputs drop; M2 a soak is flat; bounded no-panic over extreme arguments and
//! seeded random numbers (a few seconds).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use uo_alloc::{assert_flat, assert_no_leak, soak, thread_stats};
use uo_charts::chartex::histogram_bins;
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::image::image_dpi;
use uo_charts::layout::Scale;
use uo_charts::model::{ChartColor, Paint};
use uo_charts::parse_chart_space;
use uo_charts::scale::{Fixed, MAX_TICKS, auto_scale, frac, nice_step, tick_values};
use uo_charts::trend::{TrendOpts, fit_trend, trend_num};
use uo_opc::xml::{Document, Element, ParseOptions, parse};
use uo_testkit::generate::Rng;

const SPECIAL: [f64; 21] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    1e-300,
    1e300,
    1e-9,
    1e12,
    0.1,
    0.2,
    5.0,
    10.0,
    100.0,
    1000.0,
    999.9999,
    1e15,
    9_007_199_254_740_992.0,
    f64::MAX,
];

fn any(r: &mut Rng) -> f64 {
    match r.below(4) {
        0 => SPECIAL[r.below(SPECIAL.len())],
        1 => uo_jsnum::math::pow(10.0, f64::from(i32::try_from(r.below(25)).unwrap() - 12)) * if r.below(2) == 0 { 1.0 } else { -1.0 },
        2 => (f64::from(u32::try_from(r.below(200_000)).unwrap()) - 100_000.0) / 1000.0,
        _ => f64::from(u32::try_from(r.below(10_000)).unwrap()) / 100.0,
    }
}

struct Host;

impl ChartHost for Host {
    fn color(&self, _: ColorSource<'_>) -> Option<ChartColor> {
        Some(ChartColor::new("#123456"))
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts::default()
    }
    fn has_fill_style(&self) -> bool {
        false
    }
    fn fill_style(&self, _: u32, _: &str) -> Option<Paint> {
        None
    }
    fn blip(&self, _: Element<'_>) -> Option<BlipKey> {
        None
    }
    fn file_format(&self) -> Option<FileFormat> {
        None
    }
    fn user_shapes(&self) -> Option<Element<'_>> {
        None
    }
    fn theme_line_widths(&self) -> &[f64] {
        &[]
    }
}

fn cx_doc() -> Document {
    let cats = "<cx:strDim type=\"cat\"><cx:lvl ptCount=\"6\"><cx:pt idx=\"0\">a</cx:pt><cx:pt idx=\"1\">b</cx:pt><cx:pt idx=\"2\">a</cx:pt><cx:pt idx=\"5\">z</cx:pt></cx:lvl></cx:strDim>";
    let pts: String = (0..60).map(|i| format!("<cx:pt idx=\"{i}\">{}</cx:pt>", (i * 7) % 23)).collect();
    let src = format!(
        "<cx:chartSpace xmlns:cx=\"cx\"><cx:chartData><cx:data id=\"0\">{cats}<cx:numDim type=\"val\"><cx:lvl ptCount=\"60\" formatCode=\"0.0\">{pts}</cx:lvl></cx:numDim></cx:data></cx:chartData><cx:chart><cx:title><cx:tx><cx:txData><cx:v>t</cx:v></cx:txData></cx:tx></cx:title><cx:plotArea><cx:plotAreaRegion><cx:series layoutId=\"waterfall\"><cx:dataId val=\"0\"/><cx:dataLabels/></cx:series></cx:plotAreaRegion><cx:axis id=\"0\"><cx:catScaling gapWidth=\"0.5\"/><cx:tickLabels/></cx:axis></cx:plotArea><cx:legend/></cx:chart></cx:chartSpace>"
    );
    parse(src, &ParseOptions::LENIENT).unwrap()
}

fn layouts() -> Vec<String> {
    ["waterfall", "funnel", "histogram", "boxWhisker", "treemap", "sunburst", "regionMap", "clusteredColumn"]
        .iter()
        .map(|l| {
            let pts: String = (0..60).map(|i| format!("<cx:pt idx=\"{i}\">{}</cx:pt>", (i * 7) % 23)).collect();
            format!(
                "<cx:chartSpace xmlns:cx=\"cx\"><cx:chartData><cx:data id=\"0\"><cx:numDim type=\"val\"><cx:lvl ptCount=\"60\">{pts}</cx:lvl></cx:numDim></cx:data></cx:chartData><cx:chart><cx:plotArea><cx:plotAreaRegion><cx:series layoutId=\"{l}\"><cx:dataId val=\"0\"/></cx:series><cx:series layoutId=\"paretoLine\"><cx:dataId val=\"0\"/></cx:series></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>"
            )
        })
        .collect()
}

fn lp(binning: &str) -> Document {
    parse(format!("<cx:layoutPr><cx:binning {binning}/></cx:layoutPr>"), &ParseOptions::LENIENT).unwrap()
}

fn series(n: usize) -> Vec<f64> {
    (0..n).map(|i| f64::from(u32::try_from((i * 37) % 101).unwrap()) / 3.0).collect()
}

#[test]
fn m1_each_entry_point_returns_live_bytes_exactly() {
    let doc = cx_doc();
    let docs: Vec<Document> = layouts().into_iter().map(|s| parse(s, &ParseOptions::LENIENT).unwrap()).collect();
    let lp1 = lp("intervalClosed=\"l\" underflow=\"3\" overflow=\"20\"");
    let (xs, ys) = (series(40), series(40).iter().map(|v| v * 2.0 + 1.0).collect::<Vec<_>>());
    // warm the lazy tables
    black_box(parse_chart_space(doc.root(), &Host));
    let _ = black_box(histogram_bins(&xs, Some(lp1.root()), Some("0.0")));
    black_box(trend_num(1.5));
    assert_no_leak(|| parse_chart_space(doc.root(), &Host));
    for d in &docs {
        assert_no_leak(|| parse_chart_space(d.root(), &Host));
    }
    assert_no_leak(|| histogram_bins(&xs, Some(lp1.root()), Some("0.0")));
    assert_no_leak(|| histogram_bins(&xs, None, None));
    assert_no_leak(|| histogram_bins(&xs, Some(lp("binCount=\"x\"").root()), None));
    for kind in ["linear", "poly", "exp", "log", "power", "movingAvg", "bogus"] {
        assert_no_leak(|| fit_trend(kind, &xs, &ys, &TrendOpts { order: Some(4.0), period: Some(3.0), intercept: Some(1.0) }));
    }
    assert_no_leak(|| {
        let s = auto_scale(0.0, 12345.0, 10.0, &Fixed::default(), 0.05);
        (tick_values(&s, None), frac(&s, 5.0), nice_step(7.0, 10.0), trend_num(-12.34567))
    });
    assert_no_leak(|| image_dpi(&[0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
}

#[test]
fn m2_soak_is_flat() {
    let docs: Vec<Document> = layouts().into_iter().map(|s| parse(s, &ParseOptions::LENIENT).unwrap()).collect();
    let lp1 = lp("binCount=\"7\"");
    let xs = series(200);
    for d in &docs {
        black_box(parse_chart_space(d.root(), &Host));
    }
    let _ = black_box(histogram_bins(&xs, Some(lp1.root()), None));
    let base = thread_stats().live;
    let s = soak(2000, 50, |i| {
        black_box(parse_chart_space(docs[i % docs.len()].root(), &Host));
        black_box(histogram_bins(&xs, Some(lp1.root()), None).unwrap());
        black_box(fit_trend("poly", &xs, &xs, &TrendOpts::default()).unwrap());
        let sc = auto_scale(-3.0, f64::from(u32::try_from(i).unwrap()), 8.0, &Fixed::default(), 0.05);
        black_box(tick_values(&sc, None));
    });
    assert_flat(&s, 1, 0);
    drop(s);
    assert_eq!(thread_stats().live, base);
}

#[test]
fn no_panic_over_extreme_arguments() {
    let t0 = Instant::now();
    let mut r = Rng::new(0xC2C2);
    for _ in 0..20_000 {
        let (a, b, d) = (any(&mut r), any(&mut r), any(&mut r));
        let mut f = Fixed::default();
        if r.below(3) == 0 {
            f.min = Some(any(&mut r));
        }
        if r.below(3) == 0 {
            f.max = Some(any(&mut r));
        }
        if r.below(3) == 0 {
            f.major = Some(any(&mut r));
        }
        if r.below(6) == 0 {
            f.log = Some(any(&mut r));
        }
        let pad = any(&mut r);
        let s = auto_scale(a, b, d, &f, pad);
        let u = if r.below(2) == 0 { None } else { Some(any(&mut r)) };
        let ticks = tick_values(&s, u);
        assert!(ticks.len() as f64 <= MAX_TICKS + 1.0, "{} ticks of {s:?} unit {u:?}", ticks.len());
        black_box(frac(&s, any(&mut r)));
        black_box(nice_step(a, d));
        black_box(tick_values(&Scale { lo: a, hi: b, major: d, minor: 0.0, log: u }, None));
        black_box(trend_num(a));
    }
    // trendlines over hostile points
    for _ in 0..2_000 {
        let n = r.below(30);
        let xs: Vec<f64> = (0..n).map(|_| any(&mut r)).collect();
        let ys: Vec<f64> = (0..n).map(|_| any(&mut r)).collect();
        let o = TrendOpts {
            order: if r.below(2) == 0 { None } else { Some(any(&mut r)) },
            period: if r.below(2) == 0 { None } else { Some(any(&mut r)) },
            intercept: if r.below(2) == 0 { None } else { Some(any(&mut r)) },
        };
        for kind in ["linear", "poly", "exp", "log", "power", "movingAvg"] {
            if let Ok(Some(fit)) = fit_trend(kind, &xs, &ys, &o) {
                black_box(fit.eval(any(&mut r)));
            }
        }
    }
    // histograms: bin counts and widths from the file, ends at the extremes
    for _ in 0..1_500 {
        let n = r.below(60);
        let v: Vec<f64> = (0..n).map(|_| any(&mut r)).collect();
        let pick = |r: &mut Rng, list: &[&str]| list[r.below(list.len())].to_owned();
        let attrs = format!(
            "intervalClosed=\"{}\" underflow=\"{}\" overflow=\"{}\"",
            pick(&mut r, &["l", "r", "x"]),
            pick(&mut r, &["0", "-5", "1e308", "NaN", "5"]),
            pick(&mut r, &["0", "50", "-1e308", "x", "100"])
        );
        let doc = parse(
            format!(
                "<cx:layoutPr><cx:binning {attrs}><cx:binSize val=\"{}\"/><cx:binCount val=\"{}\"/></cx:binning></cx:layoutPr>",
                pick(&mut r, &["0", "1", "1e-300", "-3", "1e308", "x", "0.25"]),
                pick(&mut r, &["0", "3", "1e9", "-3", "2.5", "1e308", "x", "1000000"])
            ),
            &ParseOptions::LENIENT,
        )
        .unwrap();
        if let Ok(h) = histogram_bins(&v, Some(doc.root()), None) {
            assert_eq!(h.labels.len(), h.counts.len());
        }
    }
    // image bytes: every prefix of a few crafted headers and random garbage
    for _ in 0..2_000 {
        let n = r.below(200);
        let mut b: Vec<u8> = (0..n).map(|_| u8::try_from(r.below(256)).unwrap()).collect();
        let head: &[u8] = match r.below(4) {
            0 => &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
            1 => &[0xff, 0xd8, 0xff, 0xe1, 0, 40, 0x45, 0x78, 0x69, 0x66, 0, 0, 0x49, 0x49, 0x2a, 0, 8, 0, 0, 0],
            2 => &[0x42, 0x4d],
            _ => &[],
        };
        b.splice(0..0, head.iter().copied());
        black_box(image_dpi(&b));
    }
    assert!(t0.elapsed() < Duration::from_secs(20), "bounded: {:?}", t0.elapsed());
}
