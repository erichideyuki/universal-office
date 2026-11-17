//! The PLUG POINTS of the frontiers C-2…C-5. Each function answers one kind of capture facet for one chart; `None` means "this
//! stage is not ported yet": the driver (`facets.rs`) skips the facet and counts it per stage, it never fails it. A frontier
//! plugs in by replacing the body of its function (the signature stays) and the facets turn from skipped to compared.
//!
//! | function | facets | frontier | what to call |
//! |---|---|---|---|
//! | [`ops`] | `ops.<view>.<sel>`, `ask.<view>.<sel>` | C-3 (title, legend, area, shapes), C-4 (axes families), C-5 (pies, hierarchy, radar, labels) | `uo_charts::emit_chart` over a [`RecCanvas`] with the host of `c1_replay.rs` (`RHost`, answers from the TS call log) and [`MapMeasure`]; loop on `ChartError::NeedMeasures` until it completes |
//! | [`bounds`] | `bounds.<view>.<sel>` (Slides profile) | C-3 | `uo_charts::part_bounds` with [`MapMeasure`]; the box as `canonJson` (`{"h":…,"w":…,"x":…,"y":…}`) or `null` |
//! | [`layout`] | `lay.<view>`, `lay.<view>.<method>` | C-3 (layoutTitle, layoutLegend, legendEntries), C-4 (prepValueAxis, prepCatAxis, measureAxis, plotRect, dateLabels, valueLabels), C-2/C-4 (trendCurve), C-5 (hierarchy) | the layout structs of `uo_charts::layout` printed as the capture's `plain()` JSON (see `js/capture-charts.mjs`) |
//! | [`diff`] | `diff/<fn>` facets of `charts-diff` | C-2 | `scale::{nice_step, auto_scale, tick_values, frac}`, `trend::{fit_trend, trend_num}`, `chartex::histogram_bins` over the argument array |
//!
//! The facets of C-1 (`parse`, `model`, `dropped`, `host`, `hostx`) are replayed by `tests/c1_replay.rs`; this driver checks
//! the capture files themselves for them (`host`/`hostx` recomputed from the sidecar log).

use std::cell::RefCell;
use std::collections::HashMap;

use uo_charts::BlipKey;
use uo_charts::emit::{AreaLine, ChartSel, DataSel, EmitParams, HostProfile, Rect};
use uo_charts::error::ChartError;
use uo_charts::host::ChartImageInfo;
use uo_charts::measure::{ChartFace, ChartMeasure, FontBox};
use uo_charts::model::ChartSpace;
use uo_charts::{emit_chart, parse_chart_space, part_bounds, trace_layout};
use uo_jsnum::format::to_fixed;

use super::data::{ChartData, DiffCase, Host, Measure};
use super::host::{RHost, parse_xml};
use super::layout_json::call_line;
use super::rec::RecCanvas;

/// A view of the capture: the chart rectangle in px and px per pt.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub name: &'static str,
    pub rect: Rect,
    pub px_per_pt: f64,
}

pub fn view(name: &str) -> View {
    match name {
        "a" => View { name: "a", rect: Rect { x: 0.0, y: 0.0, w: 480.0, h: 300.0 }, px_per_pt: 4.0 / 3.0 },
        _ => View { name: "b", rect: Rect { x: 17.5, y: 9.25, w: 360.5, h: 240.25 }, px_per_pt: 0.8833 },
    }
}

/// The selection of a capture name: `full`, `bg`, `none`, `s0`, `c1`, `p02`, `not-s0` (the closures of `capture-charts.mjs`).
pub fn sel_of(name: &str) -> ChartSel {
    match name {
        "bg" => ChartSel { bg: true, data: DataSel::None },
        "none" => ChartSel { bg: false, data: DataSel::None },
        "s0" => ChartSel { bg: false, data: DataSel::Series(0) },
        "c1" => ChartSel { bg: false, data: DataSel::Category(1) },
        "p02" => ChartSel { bg: false, data: DataSel::Point { series: 0, category: 2 } },
        "not-s0" => ChartSel { bg: false, data: DataSel::NotCovered(Box::new([DataSel::Series(0)])) },
        _ => ChartSel { bg: true, data: DataSel::All },
    }
}

/// The `RenderOptions` the capture used per host (`paintOptions` of `capture-charts.mjs`).
pub fn profile_of(host: Host) -> HostProfile {
    match host {
        Host::Ppt => HostProfile { area_fill_bg1: false, area_line: None, excel_axis_density: false },
        Host::Xl => HostProfile { area_fill_bg1: true, area_line: Some(AreaLine::Auto2010), excel_axis_density: true },
    }
}

/// The CSS font stack of the capture per host (the recorder prints it in `font="…"`).
pub fn stack_of(host: Host) -> fn(&str, bool, bool) -> String {
    match host {
        Host::Ppt => RecCanvas::ppt_stack,
        Host::Xl => RecCanvas::xl_stack,
    }
}

/// The recorder's source strings of the chart's pictures, indexed like `ChartData::images` (the media id the emit gets from
/// `ChartHost::blip` must be that index; the host of the replay assigns `BlipKey(i)`).
pub fn media_of(chart: &ChartData) -> Vec<String> {
    chart.images.iter().map(|i| format!("img({})", i.sha)).collect()
}

/// A fresh [`RecCanvas`] for a chart: its host's stack and its pictures.
pub fn rec_canvas(chart: &ChartData) -> RecCanvas {
    RecCanvas::new(stack_of(chart.host), media_of(chart))
}

/// `ChartMeasure` over the capture's measurement log: the TS font string is rebuilt from the face and the size (the canvas
/// parsed `toFixed(2)`, so `to_fixed(size, 2)` gives the same text back), the key is `(font, text)`. Missing answers are
/// recorded in `misses` (and answer `None`: the engine counts them as 0 and asks again).
pub struct MapMeasure<'a> {
    pub chart: &'a ChartData,
    pub table: &'a HashMap<(String, String), Measure>,
    pub misses: RefCell<Vec<(String, String)>>,
    /// every `(font, text)` looked up, hits and misses (the `ask` facet is the distinct set)
    pub asked: RefCell<Vec<(String, String)>>,
    /// two-phase simulation: only these keys are answered (the table is the oracle for what a round fetched)
    pub known: Option<RefCell<std::collections::HashSet<(String, String)>>>,
}

impl MapMeasure<'_> {
    pub fn font_string(&self, face: &ChartFace, size_px: f64) -> String {
        // no font was ever set on the canvas: its inherited default (CH-5)
        if face.family.is_empty() {
            return "10px sans-serif".to_owned();
        }
        let px = to_fixed(size_px, 2).unwrap_or_default();
        let stack = stack_of(self.chart.host)(&face.family, face.bold, face.italic);
        format!("{}{}{px}px {stack}", if face.italic { "italic " } else { "" }, if face.bold { "bold " } else { "" })
    }

    fn lookup(&self, face: &ChartFace, size_px: f64, text: &str) -> Option<Measure> {
        let key = (self.font_string(face, size_px), text.to_owned());
        self.asked.borrow_mut().push(key.clone());
        let r = if self.known.as_ref().is_some_and(|k| !k.borrow().contains(&key)) { None } else { self.table.get(&key).copied() };
        if r.is_none() {
            self.misses.borrow_mut().push(key);
        }
        r
    }
}

impl ChartMeasure for MapMeasure<'_> {
    fn width(&self, face: &ChartFace, size_px: f64, text: &str) -> Option<f64> {
        self.lookup(face, size_px, text).map(|m| m.width)
    }

    fn font_box(&self, face: &ChartFace, size_px: f64) -> Option<FontBox> {
        self.lookup(face, size_px, "Hg").map(|m| FontBox { ascent: Some(m.ascent), descent: Some(m.descent) })
    }

    fn has_font_bounding_box(&self) -> bool {
        true
    }

    fn image(&self, key: BlipKey) -> Option<Option<ChartImageInfo>> {
        let i = usize::try_from(key.0).ok()?;
        Some(self.chart.images.get(i).map(|m| ChartImageInfo { media: key.0, w: m.w, h: m.h, w_pt: m.w_pt, h_pt: m.h_pt }))
    }
}

/// The result of one emit: the draw-form lines, the distinct measureText keys asked (`<font>\t<JSON text>` as the capture
/// writes them), whether a call the [`RecCanvas`] cannot reproduce was made, and the TS error text when the emit throws.
pub struct OpsOut {
    pub lines: Vec<String>,
    pub asks: Vec<String>,
    pub unsupported: bool,
    pub error: Option<String>,
}

/// The parsed chart and its replay host (`None` when the part does not parse: the capture has no facets for it).
pub fn setup(chart: &ChartData) -> Option<(RHost, ChartSpace)> {
    let doc = parse_xml(chart)?;
    let host = RHost::new(chart);
    let cs = parse_chart_space(doc.root(), &host);
    Some((host, cs))
}

pub fn params<'a>(chart: &ChartData, view: &View, sel: &str, images: bool) -> EmitParams<'a> {
    EmitParams {
        rect: view.rect,
        px_per_pt: view.px_per_pt,
        profile: profile_of(chart.host),
        locale: uo_numfmt::pt_br(),
        sel: sel_of(sel),
        images,
    }
}

pub fn mm<'a>(chart: &'a ChartData, table: &'a HashMap<(String, String), Measure>) -> MapMeasure<'a> {
    MapMeasure { chart, table, misses: RefCell::new(Vec::new()), asked: RefCell::new(Vec::new()), known: None }
}

/// The error text of a TS paint that throws, as the capture's `err:` facet holds it; `None` = a family that is not ported yet.
fn error_text(e: &ChartError, m: &MapMeasure<'_>) -> Option<String> {
    match e {
        ChartError::Throws(_, msg) => Some((*msg).to_owned()),
        ChartError::NeedMeasures(_) => Some(format!("measure answers missing from the capture: {:?}", m.misses.borrow())),
    }
}

/// C-3…C-5: the op stream of `paintChartSpace` for the chart, view and selection. `None` = a family whose painter is not
/// ported yet, or a picture tile (the shell painter's, C-6).
pub fn ops(chart: &ChartData, table: &HashMap<(String, String), Measure>, view: &View, sel: &str) -> Option<OpsOut> {
    let (host, cs) = setup(chart)?;
    let m = mm(chart, table);
    let p = params(chart, view, sel, true);
    let mut canvas = rec_canvas(chart);
    let error = match emit_chart(&cs, &host, &p, &m, &mut canvas) {
        Ok(()) => None,
        Err(e) => Some(error_text(&e, &m)?),
    };
    let mut asks: Vec<String> =
        m.asked.borrow().iter().map(|(f, t)| format!("{}	{}", uo_jsnum::json::string(f), uo_jsnum::json::string(t))).collect();
    asks.sort();
    asks.dedup();
    let unsupported = canvas.unsupported;
    Some(OpsOut { lines: canvas.into_lines(), asks, unsupported, error })
}

/// C-3: `chartPartBounds` as canonical JSON text (`null` when nothing is drawn) or `err:<message>`.
pub fn bounds(chart: &ChartData, table: &HashMap<(String, String), Measure>, view: &View, sel: &str) -> Option<String> {
    let (host, cs) = setup(chart)?;
    let m = mm(chart, table);
    let p = params(chart, view, sel, false);
    match part_bounds(&cs, &host, &p, &m) {
        Ok(None) => Some("null".to_owned()),
        Ok(Some(r)) => {
            use uo_charts::canon::Json;
            let o = vec![("x", r.x), ("y", r.y), ("w", r.w), ("h", r.h)];
            Some(Json::Obj(o.into_iter().map(|(k, v)| (k.to_owned(), Json::Num(v))).collect()).to_text())
        }
        Err(e) => Some(format!("err:{}", error_text(&e, &m)?)),
    }
}

/// C-3…C-5: the lines of the layout calls of one `method`, in call order, as the capture writes them: `canonJson({args, res})`
/// per call. `None` = a method of a frontier not ported yet (and the all-calls digest `lay.<view>`).
pub fn layout(chart: &ChartData, table: &HashMap<(String, String), Measure>, view: &View, method: Option<&str>) -> Option<Vec<String>> {
    let (host, cs) = setup(chart)?;
    let m = mm(chart, table);
    let p = params(chart, view, "full", false);
    let (_, trace) = trace_layout(&cs, &host, &p, &m, &mut uo_charts::NullCanvas);
    let lines = trace.calls.iter().enumerate().filter_map(|(seq, c)| call_line(c).map(|(name, l)| (seq, name, l)));
    Some(match method {
        // `lay.<view>`: every call in call order, as `<seq>, <method>, <json>` separated by tabs
        None => lines.map(|(seq, name, l)| format!("{seq}\t{name}\t{l}")).collect(),
        Some(m) => lines.filter(|(_, name, _)| *name == m).map(|(_, _, l)| l).collect(),
    })
}

/// C-2: the canonical JSON text of the result of a differential case, from its argument text.
pub fn diff(case: &DiffCase) -> Option<String> {
    super::diff_cases::run(case)
}

/// The stage that owns a facet, for the per-stage tally.
pub fn stage_of(facet: &str) -> &'static str {
    if facet.starts_with("ops.") || facet.starts_with("ask.") || facet.starts_with("bounds.") {
        return "C-3..C-5 emit";
    }
    if let Some(rest) = facet.strip_prefix("lay.") {
        return match rest.split_once('.').map(|(_, m)| m) {
            Some("layoutTitle" | "layoutLegend" | "legendEntries") => "C-3 layout",
            Some("hierarchy") => "C-5 layout",
            Some("trendCurve") => "C-4 layout (trendlines, C-2 fit)",
            Some(_) => "C-4 layout",
            None => "C-3..C-5 layout",
        };
    }
    "C-1"
}
