//! Replay of the C-1 facets (model JSON digest, host call log digest) of the TS capture
//! (`js/capture-charts.mjs`, files `tests/captures/charts-<group>.{capture,data.jsonl.gz}`): every chart's XML is
//! parsed by `uo-opc` (Slides profile: as is; Sheet profile: canonical prefixes), the host answers from the
//! TS call log (so no theme resolver is ported here), and the canonical JSON of the model and the ordered
//! digest of the calls must equal the capture's `model` and `host` facets. Other facets belong to later
//! frontiers and are skipped (the `cx:` charts are replayed since C-2). The host's theme fonts and line widths are the data
//! line's `fonts` and `themeLn`. `UO_CHARTS_C1_STEM` replays another capture (`<stem>.capture` + `<stem>.data.jsonl.gz`,
//! absolute or relative to `tests/captures`); `UO_CHARTS_C1_DUMP` writes the model JSON of every chart to
//! `core/target/c1-cap/dump/` for a diff against the TS text (`capture-charts.mjs --detail`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use uo_charts::canon::{Canon, Json, element_json};
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{BlipTile, ChartColor, FillRect, GradPath, GradStop, Paint, TileFlip};
use uo_charts::{CHART_PREFIXES, parse_chart_space};
use uo_opc::xml::{CanonicalPrefixes, Document, Element, ParseOptions, parse};
use uo_testkit::capture::{Capture, Replay};
use uo_testkit::sha256::sha256;

mod c1_support;
use c1_support::{J, gunzip, hex, json};

fn captures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("captures")
}

// ------------------------------------------------------------------------------------------ the host
struct RHost {
    ppt: bool,
    format: Option<FileFormat>,
    minor: Option<&'static str>,
    major: Option<&'static str>,
    widths: Vec<f64>,
    shapes: Option<Document>,
    answers: HashMap<String, String>,
    calls: RefCell<Vec<String>>,
    misses: RefCell<Vec<String>>,
    blips: RefCell<Vec<String>>,
}

impl RHost {
    fn answer(&self, kind: &str, key: &str) -> String {
        let k = format!("{kind}\t{key}");
        match self.answers.get(&k) {
            Some(a) => {
                self.calls.borrow_mut().push(format!("{k}\t{a}"));
                a.clone()
            }
            None => {
                self.misses.borrow_mut().push(k);
                "null".to_owned()
            }
        }
    }
    fn color_of(&self, key: &str) -> Option<ChartColor> {
        let a = self.answer("color", key);
        match json(&a) {
            J::Null => None,
            J::Str(s) => Some(ChartColor::new(&s)),
            other => panic!("colour answer {other:?}"),
        }
    }
    fn blip_name(&self, k: BlipKey) -> String {
        self.blips.borrow().get(k.0 as usize).cloned().unwrap_or_default()
    }
}

fn color_j(j: &J) -> ChartColor {
    ChartColor::new(j.s())
}

fn paint_of(j: &J, h: &RHost) -> Paint {
    match j.get("kind").unwrap().s() {
        "none" => Paint::None,
        "solid" => Paint::Solid { color: color_j(j.get("color").unwrap()), alpha: j.get("alpha").map(J::n) },
        "grad" => Paint::Grad {
            stops: j
                .get("stops")
                .unwrap()
                .arr()
                .iter()
                .map(|s| GradStop {
                    pos: s.get("pos").unwrap().n(),
                    color: color_j(s.get("color").unwrap()),
                    alpha: s.get("alpha").map(J::n),
                })
                .collect(),
            angle: j.get("angle").unwrap().n(),
            path: j.get("path").map(|p| match p.s() {
                "circle" => GradPath::Circle,
                "rect" => GradPath::Rect,
                "shape" => GradPath::Shape,
                o => GradPath::Other(o.into()),
            }),
        },
        "patt" => {
            Paint::Patt { fg: color_j(j.get("fg").unwrap()), bg: color_j(j.get("bg").unwrap()), prst: j.get("prst").unwrap().s().into() }
        }
        "blip" => {
            let name = j.get("ref").unwrap().s().to_owned();
            let mut b = h.blips.borrow_mut();
            let ix = b.iter().position(|x| *x == name).unwrap_or_else(|| {
                b.push(name);
                b.len() - 1
            });
            Paint::Blip {
                key: BlipKey(u32::try_from(ix).unwrap()),
                tile: j.get("tile").map(|t| {
                    Box::new(BlipTile {
                        sx: t.get("sx").unwrap().n(),
                        sy: t.get("sy").unwrap().n(),
                        tx: t.get("tx").unwrap().n(),
                        ty: t.get("ty").unwrap().n(),
                        algn: t.get("algn").unwrap().s().into(),
                        flip: t.get("flip").map(|f| match f.s() {
                            "x" => TileFlip::X,
                            "y" => TileFlip::Y,
                            _ => TileFlip::Xy,
                        }),
                    })
                }),
                fill_rect: j.get("fillRect").map(|r| {
                    Box::new(FillRect {
                        l: r.get("l").unwrap().n(),
                        t: r.get("t").unwrap().n(),
                        r: r.get("r").unwrap().n(),
                        b: r.get("b").unwrap().n(),
                    })
                }),
                alpha: j.get("alpha").map(J::n),
            }
        }
        other => panic!("paint kind {other}"),
    }
}

impl ChartHost for RHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        match src {
            ColorSource::Element(e) => self.color_of(&element_json(e).to_text()),
            ColorSource::Scheme(v) => {
                let node = Json::Obj(vec![
                    ("name".to_owned(), Json::s("a:schemeClr")),
                    ("attrs".to_owned(), Json::Obj(vec![("val".to_owned(), Json::s(v))])),
                    ("children".to_owned(), Json::Arr(Vec::new())),
                ]);
                self.color_of(&node.to_text())
            }
        }
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: self.major, minor: self.minor }
    }
    fn has_fill_style(&self) -> bool {
        true
    }
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        let a = self.answer("fill", &format!("{idx}|{ph_clr}"));
        match json(&a) {
            J::Null => None,
            j => Some(paint_of(&j, self)),
        }
    }
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        if !self.ppt {
            return None;
        }
        let a = self.answer("blip", &element_json(blip).to_text());
        match json(&a) {
            J::Null => None,
            J::Str(name) => {
                let mut b = self.blips.borrow_mut();
                let ix = b.iter().position(|x| *x == name).unwrap_or_else(|| {
                    b.push(name);
                    b.len() - 1
                });
                Some(BlipKey(u32::try_from(ix).unwrap()))
            }
            other => panic!("blip answer {other:?}"),
        }
    }
    fn file_format(&self) -> Option<FileFormat> {
        self.format
    }
    fn user_shapes(&self) -> Option<Element<'_>> {
        self.shapes.as_ref().map(Document::root)
    }
    fn theme_line_widths(&self) -> &[f64] {
        &self.widths
    }
}

fn xl_options() -> ParseOptions<'static> {
    ParseOptions {
        canonical: Some(CanonicalPrefixes { table: CHART_PREFIXES, default_namespace: true }),
        normalize_eol: true,
        ..ParseOptions::LENIENT
    }
}

/// The result of one capture replay.
struct Outcome {
    charts: usize,
    chartex: usize,
    report: uo_testkit::capture::Report,
}

fn replay(stem: &str) -> Option<Outcome> {
    let cap_path = captures().join(format!("{stem}.capture"));
    let data_path = captures().join(format!("{stem}.data.jsonl.gz"));
    if !cap_path.exists() || !data_path.exists() {
        eprintln!("c1_replay: {} not found (run js/capture-charts.mjs --gen c1a)", cap_path.display());
        return None;
    }
    let cap = Capture::read(&cap_path).unwrap();
    let data = String::from_utf8(gunzip(&std::fs::read(&data_path).unwrap())).unwrap();
    let mut detail: HashMap<String, String> = HashMap::new();
    if let Ok(p) = std::env::var("UO_CHARTS_C1_DETAIL") {
        // the TS model texts (`capture-charts.mjs --detail`): the first difference of a mismatch is printed
        let t = String::from_utf8(gunzip(&std::fs::read(p).unwrap())).unwrap();
        if let J::Obj(v) = json(&t) {
            detail = v.into_iter().map(|(k, x)| (k, x.s().to_owned())).collect();
        }
    }
    let mut rp = Replay::new(&cap);
    let mut charts = 0;
    let mut chartex = 0;
    for line in data.lines().filter(|l| !l.is_empty()) {
        let rec = json(line);
        let key = rec.get("key").unwrap().s().to_owned();
        let Some(record) = cap.record(&key) else { panic!("{key}: not in the capture") };
        let xml = rec.get("xml").unwrap().s();
        let host_name = rec.get("host").unwrap().s();
        let fmt = match rec.get("fileFormat") {
            Some(J::Str(s)) if s == "mso2007" => Some(FileFormat::Mso2007),
            Some(J::Str(s)) if s == "modern" => Some(FileFormat::Modern),
            _ => None,
        };
        rp.skip(&key, "input");
        if xml.contains("<cx:chartSpace") {
            chartex += 1;
        }
        charts += 1;
        let ppt = host_name == "ppt";
        // the theme's fonts (leaked: a test, a few names) and line widths, as the capture's bridge saw them
        let font = |k: &str| rec.get("fonts").and_then(|f| f.get(k)).map(|j| &*Box::leak(j.s().to_owned().into_boxed_str()));
        let (minor, major) = (font("minor"), font("major"));
        let widths: Vec<f64> = match rec.get("themeLn") {
            Some(J::Arr(a)) => a.iter().map(J::n).collect(),
            _ => Vec::new(),
        };
        let mut answers = HashMap::new();
        let mut expected_calls = Vec::new();
        for e in rec.get("hostLog").unwrap().arr() {
            let (k, kk, a) = (e.get("k").unwrap().s(), e.get("key").unwrap().s(), e.get("ans").unwrap().s());
            answers.insert(format!("{k}\t{kk}"), a.to_owned());
            expected_calls.push(format!("{k}\t{kk}\t{a}"));
        }
        let shapes = match rec.get("userShapes") {
            Some(J::Str(s)) if !ppt => Some(parse(s.clone(), &xl_options()).unwrap()),
            _ => None,
        };
        let host = RHost {
            ppt,
            format: fmt,
            minor,
            major,
            widths,
            shapes,
            answers,
            calls: RefCell::new(Vec::new()),
            misses: RefCell::new(Vec::new()),
            blips: RefCell::new(Vec::new()),
        };
        let doc = if ppt { parse(xml.to_owned(), &ParseOptions::LENIENT) } else { parse(xml.to_owned(), &xl_options()) };
        let Ok(doc) = doc else {
            rp.check(&key, "parse", "err:xml");
            continue;
        };
        rp.check(&key, "parse", "ok");
        let cs = parse_chart_space(doc.root(), &host);
        let model = Canon { blip: &|k| host.blip_name(k) }.chart_space(&cs).to_text();
        let mlen = model.len();
        if let Some(want) = detail.get(&key)
            && *want != model
        {
            let at = want.bytes().zip(model.bytes()).position(|(a, b)| a != b).unwrap_or(want.len().min(model.len()));
            let ctx = |t: &str| t.get(at.saturating_sub(80)..(at + 120).min(t.len())).unwrap_or("").to_owned();
            eprintln!(
                "{key}: model JSON differs at byte {at}
  ts:   {}
  rust: {}",
                ctx(want),
                ctx(&model)
            );
        }
        rp.check(&key, "model", &format!("{mlen}:{}", &hex(&sha256(model.as_bytes()))[..32]));
        let calls = host.calls.borrow();
        assert!(host.misses.borrow().is_empty(), "{key}: calls the TS did not make: {:?}", host.misses.borrow());
        if *calls != expected_calls {
            let at = calls.iter().zip(&expected_calls).position(|(a, b)| a != b).unwrap_or(calls.len().min(expected_calls.len()));
            eprintln!("{key}: host calls differ at #{at}\n  rust: {:?}\n  ts:   {:?}", calls.get(at), expected_calls.get(at));
        }
        rp.check(&key, "host", &format!("{}:{}", calls.len(), &hex(&sha256(calls.join("\n").as_bytes()))[..16]));
        for (f, _) in &record.facets {
            if !matches!(f.as_str(), "parse" | "model" | "host") {
                rp.skip(&key, f);
            }
        }
        if std::env::var("UO_CHARTS_C1_DUMP").is_ok() {
            let dir = captures().join("..").join("..").join("..").join("..").join("target").join("c1-cap").join("dump");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{}.json", key.replace('/', "_"))), &model).unwrap();
        }
    }
    Some(Outcome { charts, chartex, report: rp.finish() })
}

fn run(default_stem: &str) {
    let stem = std::env::var("UO_CHARTS_C1_STEM").unwrap_or_else(|_| default_stem.to_owned());
    let Some(o) = replay(&stem) else { return };
    eprintln!("c1_replay {stem}: {} charts replayed, {} chartex (C-2); {}", o.charts, o.chartex, o.report.summary());
    if !o.report.mismatches.is_empty() {
        eprintln!("{}", o.report.describe(8));
    }
    assert!(o.report.mismatches.is_empty(), "{}", o.report.summary());
}

#[test]
fn c1a_model_and_host_log_match_the_ts() {
    run("charts-c1a");
}

#[test]
fn samples_model_and_host_log_match_the_ts() {
    run("charts-samples");
}
