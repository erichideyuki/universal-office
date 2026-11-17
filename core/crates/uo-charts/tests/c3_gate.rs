//! Gates of frontier C-3 (record `uo-charts.md` "Memory", "Fuzz targets", rule CH-1): M1 every emit entry point returns live
//! bytes exactly; M2 a soak of emit cycles is flat; the retained/peak numbers of one emit; bounded no-panic over mutated charts,
//! hostile rectangles and hostile measure answers; and the measured round counts of the CH-1 wrap prefetch over `charts-c1a`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

mod c1_support;
mod support;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_charts::emit::{ChartSel, DataSel, EmitParams, HostProfile, Rect};
use uo_charts::host::{ChartImageInfo, FileFormat};
use uo_charts::measure::{ChartFace, ChartMeasure, ChartMeasureReq, FontBox};
use uo_charts::model::{BlipKey, ChartSpace};
use uo_charts::{ChartError, NullCanvas, emit_chart, need_measures, parse_chart_space, part_bounds, set_wrap_prefetch, trace_layout};
use uo_testkit::generate::Rng;

use c1_support::corpus::{PlainHost, capture_xmls, corpus, doc};

/// A deterministic synthetic measurer: width = characters × size × 0.55; `hostile` answers NaN, 0, huge, negative or nothing.
struct Synth {
    hostile: u64,
}

impl ChartMeasure for Synth {
    fn width(&self, _: &ChartFace, size_px: f64, text: &str) -> Option<f64> {
        let h = text.bytes().fold(self.hostile, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b)));
        if self.hostile != 0 {
            match h % 9 {
                0 => return Some(f64::NAN),
                1 => return Some(0.0),
                2 => return Some(1e308),
                3 => return Some(-5.0),
                4 => return None,
                _ => {}
            }
        }
        Some(text.chars().count() as f64 * size_px * 0.55)
    }
    fn font_box(&self, _: &ChartFace, size_px: f64) -> Option<FontBox> {
        Some(FontBox { ascent: Some(size_px * 0.9), descent: Some(size_px * 0.2) })
    }
    fn has_font_bounding_box(&self) -> bool {
        true
    }
    fn image(&self, key: BlipKey) -> Option<Option<ChartImageInfo>> {
        Some(Some(ChartImageInfo { media: key.0, w: 64.0, h: 48.0, w_pt: Some(48.0), h_pt: None }))
    }
}

fn params(rect: Rect, ppt: f64, sel: ChartSel, images: bool) -> EmitParams<'static> {
    EmitParams {
        rect,
        px_per_pt: ppt,
        profile: HostProfile { area_fill_bg1: false, area_line: None, excel_axis_density: false },
        locale: uo_numfmt::pt_br(),
        sel,
        images,
    }
}

fn full() -> ChartSel {
    ChartSel { bg: true, data: DataSel::All }
}

const R: Rect = Rect { x: 0.0, y: 0.0, w: 480.0, h: 300.0 };

fn parsed(xmls: &[String]) -> Vec<ChartSpace> {
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    xmls.iter().map(|x| parse_chart_space(doc(x).root(), &host)).collect()
}

#[test]
fn m1_each_entry_point_returns_live_bytes_exactly() {
    let charts = parsed(&corpus());
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let m = Synth { hostile: 0 };
    let p = params(R, 4.0 / 3.0, full(), true);
    // warm the lazy tables
    for cs in &charts {
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
    }
    for (i, cs) in charts.iter().enumerate() {
        let u = assert_no_leak(|| emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        if i == 1 {
            println!("M1 emit_chart (3 series x 40 points, legend + title): allocated {} B, peak {} B", u.allocated, u.peak_above_start);
        }
        assert_no_leak(|| emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        assert_no_leak(|| need_measures(cs, &host, &p, &Synth { hostile: 7 }));
        assert_no_leak(|| part_bounds(cs, &host, &p, &m));
        assert_no_leak(|| {
            let mut c = NullCanvas;
            trace_layout(cs, &host, &p, &m, &mut c)
        });
        assert_no_leak(|| {
            emit_chart(
                cs,
                &host,
                &params(R, 0.5, ChartSel { bg: true, data: DataSel::None }, false),
                &Synth { hostile: 3 },
                &mut NullCanvas,
            )
        });
    }
    set_wrap_prefetch(false);
    for cs in &charts {
        assert_no_leak(|| emit_chart(cs, &host, &p, &Synth { hostile: 5 }, &mut NullCanvas));
    }
    set_wrap_prefetch(true);
}

#[test]
fn m2_soak_of_emit_cycles_is_flat() {
    let charts = parsed(&corpus());
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let m = Synth { hostile: 0 };
    let p = params(R, 4.0 / 3.0, full(), true);
    for cs in &charts {
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
    }
    let base = thread_stats().live;
    let s = soak(2000, 50, |i| {
        let cs = &charts[i % charts.len()];
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        black_box(need_measures(cs, &host, &p, &Synth { hostile: 1 }));
        let _ = black_box(part_bounds(cs, &host, &p, &m));
    });
    assert_flat(&s, 1, 0);
    drop(s);
    assert_eq!(thread_stats().live, base);
}

/// The peak of one emit over a counting canvas: the engine keeps no list (the host does), so the peak is the layout of
/// one title/legend/shape pass.
#[test]
fn peak_of_one_emit() {
    let charts = parsed(&corpus());
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let m = Synth { hostile: 0 };
    let p = params(R, 4.0 / 3.0, full(), true);
    for cs in &charts {
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
    }
    let mut worst = 0i64;
    for cs in &charts {
        let (r, u) = measure(|| emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        let _ = black_box(r);
        worst = worst.max(u.peak_above_start);
    }
    println!("peak of one emit_chart over the built-in corpus: at most {worst} B");
    assert!(worst < 256 * 1024, "{worst}");
}

const HOSTILE: [&str; 20] = [
    "",
    "0",
    "-1",
    "1",
    "100000",
    "1e308",
    "-1e308",
    "1e-320",
    "NaN",
    "Infinity",
    "-Infinity",
    "0x10",
    "abc",
    " 7 ",
    "true",
    "2",
    "9",
    "50000",
    "\u{1F600}",
    "+5",
];

fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_owned();
    for _ in 0..1 + rng.below(5) {
        let marks: Vec<usize> = s.match_indices("=\"").map(|(i, _)| i + 2).collect();
        if marks.is_empty() {
            break;
        }
        let at = marks[rng.below(marks.len())];
        if let Some(end) = s.get(at..).and_then(|t| t.find('"')) {
            s.replace_range(at..at + end, HOSTILE[rng.below(HOSTILE.len())]);
        }
    }
    s
}

fn rect_of(rng: &mut Rng) -> (Rect, f64) {
    let pick = |rng: &mut Rng| [0.0, -3.0, 1e-9, 2.0, 2.5, 17.5, 480.0, 1e5, f64::NAN, f64::INFINITY, -f64::INFINITY, 1e308][rng.below(12)];
    match rng.below(4) {
        0 => (R, 4.0 / 3.0),
        1 => (Rect { x: 17.5, y: 9.25, w: 360.5, h: 240.25 }, 0.8833),
        _ => (
            Rect { x: pick(rng), y: pick(rng), w: pick(rng), h: pick(rng) },
            [1.0, 0.0, -2.0, 1e-9, 50.0, f64::NAN, f64::INFINITY][rng.below(7)],
        ),
    }
}

#[test]
fn no_panic_over_mutated_charts_hostile_rects_and_measures() {
    let mut base = corpus();
    base.extend(capture_xmls("charts-c1a").into_iter().take(80));
    let hosts = [
        PlainHost { format: None, shapes: None },
        PlainHost { format: Some(FileFormat::Modern), shapes: None },
        PlainHost { format: Some(FileFormat::Mso2007), shapes: None },
    ];
    let mut rng = Rng::new(0x00c3_0001);
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut emitted, mut throws, mut need) = (0u32, 0u32, 0u32);
    while Instant::now() < deadline {
        let m = mutate(&base[rng.below(base.len())], &mut rng);
        let Ok(d) = uo_opc::xml::parse(m, &uo_opc::xml::ParseOptions::LENIENT) else { continue };
        let host = &hosts[rng.below(hosts.len())];
        let cs = parse_chart_space(d.root(), host);
        let (rect, ppt) = rect_of(&mut rng);
        let sel = match rng.below(4) {
            0 => full(),
            1 => ChartSel { bg: true, data: DataSel::None },
            2 => ChartSel { bg: false, data: DataSel::Point { series: 0, category: 2 } },
            _ => ChartSel { bg: false, data: DataSel::NotCovered(Box::new([DataSel::Series(0)])) },
        };
        let p = params(rect, ppt, sel, rng.below(2) == 0);
        let ms = Synth { hostile: if rng.below(2) == 0 { 0 } else { rng.below(1000) as u64 + 1 } };
        match emit_chart(&cs, host, &p, &ms, &mut NullCanvas) {
            Ok(()) => emitted += 1,
            Err(ChartError::Throws(..)) => throws += 1,
            Err(ChartError::NeedMeasures(v)) => {
                need += 1;
                assert!(v.len() < 20_000, "{} questions", v.len());
            }
        }
        let _ = black_box(part_bounds(&cs, host, &p, &ms));
        black_box(need_measures(&cs, host, &p, &ms));
    }
    println!("c3 no-panic: {emitted} emits ok, {throws} throws, {need} need-measures");
    // every emit paints a whole chart now (the families are in: C-4/C-5), so a 5 s debug run makes fewer of them
    assert!(emitted > 40, "{emitted} emits");
}

/// CH-1: the rounds of the two-phase protocol over the generated charts, with and without the wrap prefetch. A round is one
/// `NeedMeasures` the host answers; the table of the capture is the oracle (only the asked keys become known).
#[test]
fn ch1_round_counts() {
    use support::data::{load_data, load_measures};
    use support::stages::{MapMeasure, mm, params as sparams, setup, view};
    let data = load_data("c1a");
    let table = load_measures("c1a");
    let mut out = String::new();
    for on in [true, false] {
        set_wrap_prefetch(on);
        let mut hist: BTreeMap<u32, u32> = BTreeMap::new();
        let (mut asked, mut charts, mut max_rounds) = (0usize, 0u32, 0u32);
        for chart in &data {
            let Some((host, cs)) = setup(chart) else { continue };
            for v in ["a", "b"] {
                let vw = view(v);
                let p = sparams(chart, &vw, "full", false);
                let mut known: HashSet<(String, String)> = HashSet::new();
                let mut rounds = 0u32;
                loop {
                    let m = MapMeasure { known: Some(RefCell::new(known.clone())), ..mm(chart, &table) };
                    match emit_chart(&cs, &host, &p, &m, &mut NullCanvas) {
                        Err(ChartError::NeedMeasures(reqs)) => {
                            rounds += 1;
                            asked += reqs.len();
                            for r in reqs {
                                match r {
                                    ChartMeasureReq::Width { face, size_px, text } => {
                                        known.insert((m.font_string(&face, size_px), text.to_string()));
                                    }
                                    ChartMeasureReq::FontBox { face, size_px } => {
                                        known.insert((m.font_string(&face, size_px), "Hg".to_owned()));
                                    }
                                    ChartMeasureReq::Image { .. } => {}
                                }
                            }
                            assert!(rounds <= 40, "{}: more than 40 rounds", chart.key);
                        }
                        _ => break,
                    }
                }
                charts += 1;
                max_rounds = max_rounds.max(rounds);
                *hist.entry(rounds).or_default() += 1;
            }
        }
        out += &format!("prefetch {on}: {charts} paints, rounds histogram {hist:?}, max {max_rounds}, {asked} questions asked in total\n");
        if on {
            assert!(max_rounds <= 8, "MAX_ROUNDS (8) with the prefetch: {max_rounds}");
        }
    }
    set_wrap_prefetch(true);
    println!("{out}");
}

/// C-4/C-5 M1/M3: every entry point over generated charts of every family (every 7th chart of `charts-c1a`, 86 charts: axes,
/// 3D, XY, trendlines, pies, of-pie, radar, stock, surface, chartex) returns live bytes exactly; the peak of one emit stays small.
#[test]
fn m1_and_peak_over_every_family() {
    let xmls: Vec<String> = capture_xmls("charts-c1a").into_iter().step_by(7).collect();
    let charts = parsed(&xmls);
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let m = Synth { hostile: 0 };
    let p = params(R, 4.0 / 3.0, full(), true);
    for cs in &charts {
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
    }
    let mut worst = 0i64;
    for cs in &charts {
        let (r, u) = measure(|| emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        let _ = black_box(r);
        worst = worst.max(u.peak_above_start);
        assert_no_leak(|| emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        assert_no_leak(|| need_measures(cs, &host, &p, &Synth { hostile: 7 }));
        assert_no_leak(|| part_bounds(cs, &host, &p, &m));
        assert_no_leak(|| trace_layout(cs, &host, &p, &m, &mut NullCanvas));
        assert_no_leak(|| {
            emit_chart(
                cs,
                &host,
                &params(R, 0.5, ChartSel { bg: true, data: DataSel::None }, false),
                &Synth { hostile: 3 },
                &mut NullCanvas,
            )
        });
    }
    println!("C-4/C-5 M1 over {} charts of c1a: exact; peak of one emit_chart at most {worst} B", charts.len());
    assert!(worst < 4 * 1024 * 1024, "{worst}");
}

/// C-4/C-5 M2: a soak of 1,000 emit cycles over the same charts, flat.
#[test]
fn m2_soak_over_every_family() {
    let xmls: Vec<String> = capture_xmls("charts-c1a").into_iter().step_by(7).collect();
    let charts = parsed(&xmls);
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let m = Synth { hostile: 0 };
    let p = params(R, 4.0 / 3.0, full(), true);
    for cs in &charts {
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
    }
    let base = thread_stats().live;
    let s = soak(1000, 50, |i| {
        let cs = &charts[i % charts.len()];
        let _ = black_box(emit_chart(cs, &host, &p, &m, &mut NullCanvas));
        black_box(need_measures(cs, &host, &p, &Synth { hostile: 1 }));
        let _ = black_box(part_bounds(cs, &host, &p, &m));
    });
    assert_flat(&s, 1, 0);
    drop(s);
    assert_eq!(thread_stats().live, base);
}
