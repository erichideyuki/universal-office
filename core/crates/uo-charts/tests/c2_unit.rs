//! Unit tests of frontier C-2 (scale, trendlines, `imageDpi`, chartex): the expected values of `charts.test.ts`
//! (escala automática, resolução gravada no arquivo, chartex, linhas de tendência) and the JS traps of the port.
//! The parity replay against the TS captures is `parity.rs` (the 2,760 differential cases) and `c1_replay.rs` (the
//! 117 `cx:` charts of `charts-c1a`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::image::image_dpi;
use uo_charts::model::{ChartColor, GroupKind, Paint};
use uo_charts::scale::{Fixed, auto_scale, frac, nice_step, tick_values};
use uo_charts::trend::{TrendOpts, fit_trend, trend_num};
use uo_charts::{ChartSpace, parse_chart_space};
use uo_opc::xml::{Document, Element, ParseOptions, parse};

#[test]
fn scale_zero_on_the_axis_five_percent_slack_smallest_step() {
    let f = Fixed::default();
    let s = auto_scale(1.8, 5.0, 10.0, &f, 0.05);
    assert_eq!((s.lo, s.hi, s.major), (0.0, 6.0, 1.0));
    let s = auto_scale(2.5, 4.5, 10.0, &f, 0.05);
    assert_eq!((s.lo, s.hi, s.major), (0.0, 5.0, 0.5));
    let s = auto_scale(0.18, 0.5, 10.0, &f, 0.05);
    assert_eq!((s.lo, s.hi), (0.0, 0.6));
    // data far from zero (min > 5/6 max) drop the zero
    let s = auto_scale(100.0, 110.0, 10.0, &f, 0.05);
    assert!(s.lo > 0.0, "{s:?}");
    // equal values are not a narrow range: 5,5,5 -> 0..6 (measured)
    let s = auto_scale(5.0, 5.0, 10.0, &f, 0.05);
    assert_eq!((s.lo, s.hi), (0.0, 6.0));
}

#[test]
fn scale_absurd_unit_does_not_make_millions_of_ticks() {
    let f = Fixed { min: Some(0.0), max: Some(3000.0), major: Some(1e-6), ..Fixed::default() };
    let s = auto_scale(0.0, 3000.0, 10.0, &f, 0.05);
    assert!(tick_values(&s, None).len() < 20);
}

#[test]
fn scale_log_and_ticks() {
    let f = Fixed { log: Some(10.0), ..Fixed::default() };
    let s = auto_scale(3.0, 8000.0, 10.0, &f, 0.05);
    assert_eq!((s.lo, s.hi, s.log), (1.0, 10_000.0, Some(10.0)));
    assert_eq!(tick_values(&s, None), vec![1.0, 10.0, 100.0, 1000.0, 10_000.0]);
    assert_eq!(frac(&s, 100.0), 0.5);
    assert_eq!(frac(&s, -4.0), 0.0);
    // 6 * 0.1 has no binary residue
    let lin = auto_scale(0.0, 0.6, 6.0, &Fixed::default(), 0.0);
    assert_eq!(tick_values(&lin, None).last().copied(), Some(lin.hi));
    assert_eq!(nice_step(0.0, 10.0), 1.0);
    assert_eq!(nice_step(f64::NAN, 10.0), 1.0);
}

#[test]
fn scale_ticks_stop_at_one_thousand() {
    let s = uo_charts::layout::Scale { lo: 0.0, hi: 1e9, major: 1.0, minor: 0.0, log: None };
    assert_eq!(tick_values(&s, None).len(), 1001);
    let l = uo_charts::layout::Scale { lo: 1.0, hi: 1e300, major: 0.0, minor: 0.0, log: Some(1.0) };
    assert_eq!(tick_values(&l, None).len(), 1000);
}

const X: [f64; 7] = [1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 13.0];
const Y: [f64; 7] = [2.1, 3.9, 6.2, 9.8, 17.5, 21.0, 26.9];

fn eq(kind: &str, o: &TrendOpts) -> String {
    fit_trend(kind, &X, &Y, o).unwrap().unwrap().eq.unwrap().iter().map(|p| &*p.text).collect::<Vec<_>>().join("|")
}

#[test]
fn trend_equations_have_four_decimals_and_a_comma() {
    let o = TrendOpts::default();
    assert_eq!(eq("linear", &o), "y = 2,1067x - 0,1543");
    let fit = fit_trend("linear", &X, &Y, &o).unwrap().unwrap();
    assert_eq!(format!("{:.4}", fit.r2.unwrap()), "0.9979");
    assert_eq!(eq("log", &o), "y = 9,6097ln(x) - 1,7204");
    // the exponent is superscript
    assert_eq!(eq("exp", &o), "y = 2,7165e|0,2003x");
    assert_eq!(trend_num(-0.00004), "0");
    assert_eq!(trend_num(1.5), "1,5");
    assert_eq!(trend_num(100.0), "100");
    assert_eq!(trend_num(f64::NAN), "NaN");
}

#[test]
fn trend_moving_average_from_the_period_on() {
    let fit =
        fit_trend("movingAvg", &[1.0, 2.0, 3.0, 4.0], &[2.0, 4.0, 6.0, 8.0], &TrendOpts { period: Some(2.0), ..TrendOpts::default() })
            .unwrap()
            .unwrap();
    assert_eq!(fit.points.clone().unwrap(), vec![(2.0, 3.0), (3.0, 5.0), (4.0, 7.0)]);
    assert_eq!(fit.eval(1.0), None);
}

#[test]
fn trend_traps() {
    // fewer than two finite points, a degenerate system, a log of x <= 0
    assert!(fit_trend("linear", &[1.0], &[1.0], &TrendOpts::default()).unwrap().is_none());
    assert!(fit_trend("linear", &[3.0, 3.0, 3.0], &[1.0, 2.0, 3.0], &TrendOpts::default()).unwrap().is_none());
    assert!(fit_trend("log", &[-1.0, 0.0, 2.0], &[1.0, 2.0, 3.0], &TrendOpts::default()).unwrap().is_none());
    // `new Array(NaN)`: a polynomial order of NaN throws a RangeError
    assert!(fit_trend("poly", &X, &Y, &TrendOpts { order: Some(f64::NAN), ..TrendOpts::default() }).is_err());
    // order 9 is clamped to 6
    let p = fit_trend("poly", &X, &Y, &TrendOpts { order: Some(9.0), ..TrendOpts::default() });
    assert!(p.is_ok());
    // an unknown kind is linear
    assert_eq!(eq("bogus", &TrendOpts::default()), eq("linear", &TrendOpts::default()));
}

fn png(phys: Option<[u8; 9]>) -> Vec<u8> {
    let mut v = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 60, 0, 0, 0, 40, 8, 6, 0, 0, 0, 0, 0,
        0, 0,
    ];
    if let Some(p) = phys {
        v.extend([0, 0, 0, 9, 0x70, 0x48, 0x59, 0x73]);
        v.extend(p);
        v.extend([0, 0, 0, 0]);
    }
    v.extend([0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0, 0, 0, 0]);
    v
}

#[test]
fn image_resolution_from_png_and_jpeg() {
    // 5669 px/m = 144 dpi
    let (x, y) = image_dpi(&png(Some([0, 0, 0x16, 0x25, 0, 0, 0x16, 0x25, 1]))).unwrap();
    assert!((x - 144.0).abs() < 0.5 && (y - 144.0).abs() < 0.5);
    assert_eq!(image_dpi(&png(None)), None);
    // unit 0 (unknown): none
    assert_eq!(image_dpi(&png(Some([0, 0, 0x16, 0x25, 0, 0, 0x16, 0x25, 0]))), None);
    let jfif = [0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 1, 0, 75, 0, 75, 0, 0, 0xff, 0xda, 0, 2];
    assert_eq!(image_dpi(&jfif), Some((75.0, 75.0)));
    // truncated and garbage input never panic
    for n in 0..jfif.len() {
        let _ = image_dpi(&jfif[..n]);
    }
    assert_eq!(image_dpi(&[]), None);
    assert_eq!(image_dpi(&[0x42, 0x4d]), None);
}

#[test]
fn image_resolution_from_exif_and_bmp() {
    // JPEG with an EXIF APP1 (little endian): X/YResolution 300/1, unit 2 (inch)
    let mut t = vec![0x49, 0x49, 0x2a, 0, 8, 0, 0, 0, 3, 0];
    let ent = |tag: u16, ty: u16, val: u32| {
        let mut e = Vec::new();
        e.extend(tag.to_le_bytes());
        e.extend(ty.to_le_bytes());
        e.extend(1u32.to_le_bytes());
        e.extend(val.to_le_bytes());
        e
    };
    t.extend(ent(0x011a, 5, 8 + 2 + 36 + 4));
    t.extend(ent(0x011b, 5, 8 + 2 + 36 + 4 + 8));
    t.extend(ent(0x0128, 3, 2));
    t.extend([0, 0, 0, 0]);
    t.extend(300u32.to_le_bytes());
    t.extend(1u32.to_le_bytes());
    t.extend(300u32.to_le_bytes());
    t.extend(1u32.to_le_bytes());
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(&t);
    let len = u16::try_from(app1.len() + 2).unwrap();
    let mut j = vec![0xff, 0xd8, 0xff, 0xe1];
    j.extend(len.to_be_bytes());
    j.extend(app1);
    j.extend([0xff, 0xda, 0, 2]);
    assert_eq!(image_dpi(&j), Some((300.0, 300.0)));
    // BMP: 3780 px/m = 96.012 dpi
    let mut b = vec![0x42, 0x4d];
    b.resize(38, 0);
    b.extend(3780i32.to_le_bytes());
    b.extend(3780i32.to_le_bytes());
    b.extend([0; 8]);
    let (x, _) = image_dpi(&b).unwrap();
    assert!((x - 96.012).abs() < 1e-9);
}

struct Host;

impl ChartHost for Host {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        Some(ChartColor::new(match src {
            ColorSource::Scheme("bg1") => "#FFFFFF",
            ColorSource::Scheme("tx1") => "#000000",
            ColorSource::Scheme(_) => "#156082",
            ColorSource::Element(_) => "#123456",
        }))
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

fn num_dim(id: &str, vals: &[f64], ty: &str) -> String {
    let pts: String = vals.iter().enumerate().map(|(i, v)| format!("<cx:pt idx=\"{i}\">{v}</cx:pt>")).collect();
    format!(
        "<cx:data id=\"{id}\"><cx:numDim type=\"{ty}\"><cx:f>x</cx:f><cx:lvl ptCount=\"{}\" formatCode=\"General\">{pts}</cx:lvl></cx:numDim></cx:data>",
        vals.len()
    )
}

fn ex(series: &str, data: &str, axes: &str) -> ChartSpace {
    let src = format!(
        "<cx:chartSpace xmlns:cx=\"cx\" xmlns:a=\"a\"><cx:chartData>{data}</cx:chartData><cx:chart><cx:plotArea><cx:plotAreaRegion>{series}</cx:plotAreaRegion>{axes}</cx:plotArea><cx:legend pos=\"t\"/></cx:chart></cx:chartSpace>"
    );
    let doc: Document = parse(src, &ParseOptions::LENIENT).unwrap();
    parse_chart_space(doc.root(), &Host)
}

fn vals(s: &uo_charts::model::Series) -> Vec<Option<f64>> {
    s.values.clone()
}

#[test]
fn chartex_waterfall_stack_with_an_invisible_base() {
    let cs = ex(
        "<cx:series layoutId=\"waterfall\"><cx:dataId val=\"0\"/><cx:layoutPr><cx:subtotals><cx:idx val=\"3\"/></cx:subtotals></cx:layoutPr></cx:series>",
        &num_dim("0", &[5.0, 3.0, -2.0, 6.0, -1.0], "val"),
        "",
    );
    let g = cs.groups.first().unwrap();
    assert_eq!(g.kind, GroupKind::Bar);
    assert_eq!(g.series.iter().map(|s| &*s.name).collect::<Vec<_>>(), ["", "Aumento", "Diminuição", "Total"]);
    let some = |v: &[Option<f64>]| v.to_vec();
    assert_eq!(vals(&g.series[0]), some(&[Some(0.0), Some(5.0), Some(6.0), None, Some(5.0)]));
    assert_eq!(vals(&g.series[1]), some(&[Some(5.0), Some(3.0), None, None, None]));
    assert_eq!(vals(&g.series[2]), some(&[None, None, Some(2.0), None, Some(1.0)]));
    assert_eq!(vals(&g.series[3]), some(&[None, None, None, Some(6.0), None]));
    assert!(cs.legend.as_ref().unwrap().deleted.has(0.0));
}

#[test]
fn chartex_histogram_uses_scott_with_two_significant_digits() {
    let cs = ex(
        "<cx:series layoutId=\"clusteredColumn\"><cx:dataId val=\"0\"/><cx:layoutPr><cx:binning intervalClosed=\"r\"/></cx:layoutPr></cx:series>",
        &num_dim("0", &[12.0, 10.0, 11.0, 13.0, 15.0], "val"),
        "<cx:axis id=\"0\"><cx:catScaling gapWidth=\"0\"/><cx:tickLabels/></cx:axis><cx:axis id=\"1\"><cx:valScaling/><cx:tickLabels/></cx:axis>",
    );
    let g = cs.groups.first().unwrap();
    let s = g.series.first().unwrap();
    assert_eq!(s.cats.as_ref().unwrap().iter().map(|c| &**c).collect::<Vec<_>>(), ["[10, 13,9]", "(13,9, 17,8]"]);
    assert_eq!(vals(s), vec![Some(4.0), Some(1.0)]);
    assert_eq!(g.gap_width, 0.0);
}

#[test]
fn chartex_treemap_parent_levels_inherit_the_cell_above() {
    let cats = "<cx:strDim type=\"cat\"><cx:lvl ptCount=\"3\"><cx:pt idx=\"0\">a</cx:pt><cx:pt idx=\"1\">b</cx:pt><cx:pt idx=\"2\">c</cx:pt></cx:lvl><cx:lvl ptCount=\"3\"><cx:pt idx=\"0\">P</cx:pt><cx:pt idx=\"2\">Q</cx:pt></cx:lvl></cx:strDim>";
    let data = format!(
        "<cx:data id=\"0\">{cats}<cx:numDim type=\"size\"><cx:lvl ptCount=\"3\"><cx:pt idx=\"0\">1</cx:pt><cx:pt idx=\"1\">2</cx:pt><cx:pt idx=\"2\">3</cx:pt></cx:lvl></cx:numDim></cx:data>"
    );
    let cs = ex("<cx:series layoutId=\"treemap\"><cx:dataId val=\"0\"/></cx:series>", &data, "");
    let g = cs.groups.first().unwrap();
    assert_eq!(g.kind, GroupKind::Treemap);
    let s = g.series.first().unwrap();
    assert_eq!(s.cats.as_ref().unwrap().iter().map(|c| &**c).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(
        s.cat_levels.as_ref().unwrap().iter().map(|l| l.iter().map(|c| &**c).collect::<Vec<_>>()).collect::<Vec<_>>(),
        vec![vec!["P", "P", "Q"]]
    );
}

#[test]
fn chartex_pareto_sorts_and_accumulates() {
    let cats = "<cx:strDim type=\"cat\"><cx:lvl ptCount=\"4\"><cx:pt idx=\"0\">x</cx:pt><cx:pt idx=\"1\">y</cx:pt><cx:pt idx=\"2\">x</cx:pt><cx:pt idx=\"3\">z</cx:pt></cx:lvl></cx:strDim>";
    let data = format!(
        "<cx:data id=\"0\">{cats}<cx:numDim type=\"val\"><cx:lvl ptCount=\"4\"><cx:pt idx=\"0\">1</cx:pt><cx:pt idx=\"1\">6</cx:pt><cx:pt idx=\"2\">1</cx:pt><cx:pt idx=\"3\">1</cx:pt></cx:lvl></cx:numDim></cx:data>"
    );
    let cs = ex(
        "<cx:series layoutId=\"clusteredColumn\"><cx:dataId val=\"0\"/></cx:series><cx:series layoutId=\"paretoLine\"><cx:dataId val=\"0\"/></cx:series>",
        &data,
        "",
    );
    assert_eq!(cs.groups.len(), 2);
    let bars = cs.groups[0].series.first().unwrap();
    assert_eq!(bars.cats.as_ref().unwrap().iter().map(|c| &**c).collect::<Vec<_>>(), ["y", "x", "z"]);
    assert_eq!(vals(bars), vec![Some(6.0), Some(2.0), Some(1.0)]);
    let line = cs.groups[1].series.first().unwrap();
    assert_eq!(vals(line), vec![Some(6.0 / 9.0), Some(8.0 / 9.0), Some(1.0)]);
    assert_eq!(cs.axes.len(), 4);
    assert!(cs.legend.as_ref().unwrap().deleted.has(1.0));
}

#[test]
fn chartex_hostile_counts_and_extremes_return() {
    // a negative ptCount, an idx past ptCount and 1e9 bins: no panic, the chart still converts
    for (count, idx) in [("-5", "0"), ("2", "5000"), ("0", "99999"), ("1000000000", "1")] {
        let data = format!(
            "<cx:data id=\"0\"><cx:numDim type=\"val\"><cx:lvl ptCount=\"{count}\"><cx:pt idx=\"{idx}\">3</cx:pt></cx:lvl></cx:numDim></cx:data>"
        );
        let cs = ex(
            "<cx:series layoutId=\"clusteredColumn\"><cx:dataId val=\"0\"/><cx:layoutPr><cx:binning><cx:binSize val=\"1e-300\"/></cx:binning></cx:layoutPr></cx:series>",
            &data,
            "",
        );
        assert_eq!(cs.groups.len(), 1);
    }
}
