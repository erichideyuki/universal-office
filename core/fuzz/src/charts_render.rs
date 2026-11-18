//! Fuzz targets `chart_scale` and `chart_emit` of `uo-charts` (record `core/docs/modules/uo-charts.md`, "C-2" and "C-3").
//!
//! `chart_scale`: byte 0 = mode, the rest as little-endian `f64`s (every bit pattern: NaN, infinities, subnormals, 1e308):
//! 0 = `auto_scale` + `tick_values` + `frac` + `nice_step` (the ticks never pass 1,000), 1 = `fit_trend` of every kind over
//! points from the bytes + `eval` + `trend_num`, 2 = `histogram_bins` over values from the bytes with a `cx:binning` node
//! built from them (a bin count that is not an array length is the RangeError, never an allocation), 3 = `image_dpi` over the
//! raw bytes (PNG / JPEG / EXIF / BMP readers on truncated and hostile input).
//!
//! `chart_emit`: byte 0 = flags (bits 0-1 file format, bit 2 canonical prefixes, bit 3 images on, bit 4 the TS chart rules), bytes 1..9 = hostile
//! `px_per_pt` and bytes 9..41 the rectangle (4 × `f64`), byte 41 = measure mode (answers NaN / 0 / 1e308 / negative /
//! missing by hash), then the chart XML and the chart drawing split on a 0 byte. The chart is parsed and emitted through
//! `emit_chart`, `need_measures`, `part_bounds` and `trace_layout` (every family: axes, XY, pies, radar, chartex): no panic, the number of canvas calls is
//! bounded by the size of the chart (points + series + shapes), the questions of a round are bounded.

use uo_charts::chartex::histogram_bins;
use uo_charts::emit::{
    Canvas, CanvasFont, CanvasPaint, ChartSel, DataSel, EmitParams, HostProfile, LineCap, LineJoin, Rect, TextAlign, TextBaseline,
};
use uo_charts::host::{BlipKey, ChartHost, ChartImageInfo, ColorSource, FileFormat, HostFonts};
use uo_charts::image::image_dpi;
use uo_charts::layout::Scale;
use uo_charts::measure::{ChartFace, ChartMeasure, FontBox};
use uo_charts::model::{ChartColor, Paint};
use uo_charts::scale::{Fixed, MAX_TICKS, auto_scale, frac, nice_step, tick_values};
use uo_charts::trend::{TrendOpts, fit_trend, trend_num};
use uo_charts::{CHART_PREFIXES, ChartError, emit_chart, need_measures, parse_chart_space, part_bounds, trace_layout};
use uo_opc::xml::{CanonicalPrefixes, Document, Element, ParseOptions, parse};

struct Floats<'a> {
    b: &'a [u8],
    i: usize,
}

impl Floats<'_> {
    fn f(&mut self) -> f64 {
        let Some(c) = self.b.get(self.i..self.i + 8) else { return 0.0 };
        self.i += 8;
        let mut a = [0u8; 8];
        a.copy_from_slice(c);
        f64::from_le_bytes(a)
    }

    /// A float, or `None` for a missing optional (a flag bit).
    fn opt(&mut self, on: bool) -> Option<f64> {
        let v = self.f();
        on.then_some(v)
    }
}

/// The target `chart_scale`.
pub fn chart_scale(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else { return };
    let mut r = Floats { b: rest, i: 0 };
    match mode % 4 {
        0 => {
            let flags = rest.first().copied().unwrap_or(0);
            r.i = 1;
            let (a, b, d, pad) = (r.f(), r.f(), r.f(), r.f());
            let fixed = Fixed {
                min: r.opt(flags & 1 != 0),
                max: r.opt(flags & 2 != 0),
                major: r.opt(flags & 4 != 0),
                minor: r.opt(flags & 8 != 0),
                log: r.opt(flags & 16 != 0),
            };
            let s = auto_scale(a, b, d, &fixed, if flags & 32 != 0 { pad } else { 0.05 });
            let unit = r.opt(flags & 64 != 0);
            let ticks = tick_values(&s, unit);
            assert!(ticks.len() as f64 <= MAX_TICKS + 1.0, "{} ticks", ticks.len());
            let _ = frac(&s, r.f());
            let _ = nice_step(a, d);
            let raw = Scale { lo: r.f(), hi: r.f(), major: r.f(), minor: r.f(), log: r.opt(flags & 128 != 0) };
            assert!(tick_values(&raw, unit).len() as f64 <= MAX_TICKS + 1.0);
            let _ = frac(&raw, a);
        }
        1 => {
            let kind = ["linear", "poly", "exp", "log", "power", "movingAvg", "bogus"][usize::from(rest.first().copied().unwrap_or(0)) % 7];
            r.i = 1;
            let o = TrendOpts { order: Some(r.f()), period: Some(r.f()), intercept: Some(r.f()) };
            let o = TrendOpts {
                order: o.order.filter(|_| rest.get(1).is_some_and(|b| b & 1 != 0)),
                period: o.period.filter(|_| rest.get(1).is_some_and(|b| b & 2 != 0)),
                intercept: o.intercept.filter(|_| rest.get(1).is_some_and(|b| b & 4 != 0)),
            };
            let n = (rest.len().saturating_sub(40) / 16).min(200);
            let xs: Vec<f64> = (0..n).map(|_| r.f()).collect();
            let ys: Vec<f64> = (0..n).map(|_| r.f()).collect();
            if let Ok(Some(fit)) = fit_trend(kind, &xs, &ys, &o) {
                for x in xs.iter().take(20) {
                    let _ = fit.eval(*x);
                }
                for e in fit.eq.iter().flatten() {
                    assert!(e.text.len() < 4096);
                }
            }
            let _ = trend_num(r.f());
        }
        2 => {
            let sel = rest.first().copied().unwrap_or(0);
            r.i = 1;
            let attr = |name: &str, on: bool, r: &mut Floats<'_>| -> String {
                let v = r.f();
                if on { format!(" {name}=\"{v}\"") } else { String::new() }
            };
            let bin = format!(
                "<cx:layoutPr><cx:binning intervalClosed=\"{}\"{}{}><cx:binSize{}/><cx:binCount{}/></cx:binning></cx:layoutPr>",
                if sel & 1 == 0 { "r" } else { "l" },
                attr("underflow", sel & 2 != 0, &mut r),
                attr("overflow", sel & 4 != 0, &mut r),
                attr("val", sel & 8 != 0, &mut r),
                attr("val", sel & 16 != 0, &mut r),
            );
            let Ok(doc) = parse(bin, &ParseOptions::LENIENT) else { return };
            let n = (rest.len().saturating_sub(40) / 8).min(400);
            let vals: Vec<f64> = (0..n).map(|_| r.f()).collect();
            if let Ok(h) = histogram_bins(&vals, Some(doc.root()), if sel & 32 != 0 { Some("0.0") } else { None }) {
                assert_eq!(h.labels.len(), h.counts.len());
                assert!(h.labels.len() <= 100_002);
            }
        }
        _ => {
            let _ = image_dpi(rest);
        }
    }
}

/// Seeds of `chart_scale`: one per mode, with ordinary and hostile numbers.
pub fn chart_scale_seeds() -> Vec<Vec<u8>> {
    let fl = |v: &[f64]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let mut out = Vec::new();
    for hostile in [false, true] {
        let vals: Vec<f64> = if hostile {
            vec![f64::NAN, f64::INFINITY, 1e308, -1e308, 1e-320, 0.0, -0.0, 5.0]
        } else {
            vec![0.0, 12.0, 8.0, 0.05, 2.0, 100.0, 1.0, 1.0]
        };
        let mut a = vec![0u8, 0xff];
        a.extend(fl(&vals));
        a.extend(fl(&vals));
        out.push(a);
        let mut t = vec![1u8, 1, 7];
        t.extend(fl(&vals));
        t.extend(fl(&vals));
        t.extend(fl(&vals));
        out.push(t);
        let mut h = vec![2u8, 0xff];
        h.extend(fl(&vals));
        h.extend(fl(&vals));
        out.push(h);
    }
    out.push(
        [
            &[3u8][..],
            &[
                0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 9, 0x70, 0x48, 0x59, 0x73, 0, 0, 0x16, 0x25, 0, 0, 0x16, 0x25, 1,
                0, 0, 0, 0, 0, 0, 0, 0,
            ][..],
        ]
        .concat(),
    );
    out.push(
        [&[3u8][..], &[0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 1, 0, 75, 0, 75, 0, 0, 0xff, 0xda, 0, 2][..]]
            .concat(),
    );
    out
}

struct FuzzHost {
    format: Option<FileFormat>,
    shapes: Option<Document>,
}

impl ChartHost for FuzzHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        let v = match src {
            ColorSource::Scheme(v) => v,
            ColorSource::Element(e) => e.attr("val").unwrap_or(""),
        };
        if v.len() % 7 == 6 { None } else { Some(ChartColor::new(&format!("#{v}"))) }
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: Some("Major"), minor: Some("Minor") }
    }
    fn has_fill_style(&self) -> bool {
        true
    }
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        (!idx.is_multiple_of(3)).then(|| Paint::Solid { color: ChartColor::new(ph_clr), alpha: None })
    }
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        blip.attr("r:embed").map(|v| BlipKey(u32::try_from(v.len()).unwrap_or(0)))
    }
    fn file_format(&self) -> Option<FileFormat> {
        self.format
    }
    fn user_shapes(&self) -> Option<Element<'_>> {
        self.shapes.as_ref().map(Document::root)
    }
    fn theme_line_widths(&self) -> &[f64] {
        &[0.5, 1.0, 1.5]
    }
}

/// Hostile measure answers (the mode picks which texts get NaN, 0, 1e308, a negative width or no answer).
struct FuzzMeasure(u8);

impl ChartMeasure for FuzzMeasure {
    fn width(&self, _: &ChartFace, size_px: f64, text: &str) -> Option<f64> {
        let h = text.bytes().fold(u64::from(self.0), |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b)));
        if self.0 != 0 {
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
        Some(FontBox { ascent: Some(size_px), descent: if self.0.is_multiple_of(3) { None } else { Some(size_px * 0.2) } })
    }
    fn has_font_bounding_box(&self) -> bool {
        self.0.is_multiple_of(2)
    }
    fn image(&self, key: BlipKey) -> Option<Option<ChartImageInfo>> {
        match key.0 % 3 {
            0 => None,
            1 => Some(None),
            _ => Some(Some(ChartImageInfo { media: key.0, w: 32.0, h: 0.0 + f64::from(key.0), w_pt: None, h_pt: Some(5.0) })),
        }
    }
}

/// A canvas that counts its calls.
#[derive(Default)]
struct Count(u64);

macro_rules! count_calls {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(fn $name(&mut self, $(_: $ty),*) { self.0 += 1; })*
    };
}

impl Canvas for Count {
    count_calls! {
        save();
        restore();
        translate(a: f64, b: f64);
        rotate(a: f64);
        stroke_style(a: &str);
        line_width(a: f64);
        line_dash(a: &[f64]);
        line_cap(a: LineCap);
        line_join(a: LineJoin);
        shadow_color(a: &str);
        shadow_blur(a: f64);
        shadow_offset_x(a: f64);
        shadow_offset_y(a: f64);
        global_alpha_mul(a: f64);
        text_align(a: TextAlign);
        text_baseline(a: TextBaseline);
        begin_path();
        move_to(a: f64, b: f64);
        line_to(a: f64, b: f64);
        bezier_curve_to(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64);
        rect(a: f64, b: f64, c: f64, d: f64);
        round_rect(a: f64, b: f64, c: f64, d: f64, e: f64);
        arc(a: f64, b: f64, c: f64, d: f64, e: f64, f: bool);
        close_path();
        fill();
        stroke();
        clip();
        fill_rect(a: f64, b: f64, c: f64, d: f64);
        stroke_rect(a: f64, b: f64, c: f64, d: f64);
        fill_text(a: &str, b: f64, c: f64);
        draw_image(a: u32, b: f64, c: f64, d: f64, e: f64);
    }
    fn fill_style(&mut self, _: &CanvasPaint<'_>) {
        self.0 += 1;
    }
    fn font(&mut self, _: &CanvasFont<'_>) {
        self.0 += 1;
    }
    fn ellipse(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: bool) {
        self.0 += 1;
    }
}

/// The target `chart_emit`.
pub fn chart_emit(data: &[u8]) {
    if data.len() < 42 {
        return;
    }
    let flags = data[0];
    let mut r = Floats { b: data, i: 1 };
    let ppt = r.f();
    let rect = Rect { x: r.f(), y: r.f(), w: r.f(), h: r.f() };
    let mode = data[41];
    let mut parts = data[42..].splitn(2, |&b| b == 0);
    let chart = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
    let drawing = parts.next().map(|d| String::from_utf8_lossy(d).into_owned());
    let opts = if flags & 4 != 0 {
        ParseOptions {
            canonical: Some(CanonicalPrefixes { table: CHART_PREFIXES, default_namespace: true }),
            normalize_eol: true,
            ..ParseOptions::LENIENT
        }
    } else {
        ParseOptions::LENIENT
    };
    let Ok(doc) = parse(chart, &opts) else { return };
    let format = match flags & 3 {
        1 => Some(FileFormat::Modern),
        2 => Some(FileFormat::Mso2007),
        _ => None,
    };
    let host = FuzzHost { format, shapes: drawing.and_then(|d| parse(d, &opts).ok()) };
    let cs = parse_chart_space(doc.root(), &host);
    let sel = match mode % 4 {
        0 => ChartSel { bg: true, data: DataSel::All },
        1 => ChartSel { bg: true, data: DataSel::None },
        2 => ChartSel { bg: false, data: DataSel::Point { series: 0, category: 2 } },
        _ => ChartSel { bg: false, data: DataSel::NotCovered(Box::new([DataSel::Series(0)])) },
    };
    let profile = HostProfile { area_fill_bg1: flags & 16 != 0, area_line: None, excel_axis_density: flags & 32 != 0 };
    let rules = if flags & 16 != 0 { uo_charts::rules::ChartRules::ts_strict() } else { uo_charts::rules::ChartRules::default() };
    let p = EmitParams { rect, px_per_pt: ppt, profile, locale: uo_numfmt::pt_br(), sel, images: flags & 8 != 0, rules };
    let m = FuzzMeasure(mode / 4);
    let points: usize = cs
        .groups
        .iter()
        .flat_map(|g| g.series.iter())
        .map(|s| s.values.len().max(s.cats.as_ref().map_or(0, Vec::len)).max(s.cat_nums.as_ref().map_or(0, Vec::len)))
        .sum();
    let series: usize = cs.groups.iter().map(|g| g.series.len()).sum();
    let shapes = cs.user_shapes.as_ref().map_or(0, Vec::len);
    // the calls of the frame, title, legend (one entry per point at most) and chart drawing: linear in what the chart holds
    // + the value axes: at most 1,000 ticks each (`MAX_TICKS`), each with a label (measured, so a `font` call, in every one of the
    // ~30 `measureAxis` passes of the plot layout), a gridline and a tick mark, and 2,000 minor ticks of a log axis
    let bound = 20_000 + 400 * (points + series + shapes) as u64 + 150_000 * cs.axes.len() as u64;
    for which in 0..2 {
        let mut c = Count::default();
        let res = match which {
            0 => emit_chart(&cs, &host, &p, &m, &mut c),
            _ => trace_layout(&cs, &host, &p, &m, &mut c).0,
        };
        assert!(c.0 <= bound, "{} canvas calls for {points} points, {series} series, {shapes} shapes", c.0);
        if let Err(ChartError::NeedMeasures(v)) = res {
            assert!(v.len() <= 4096 + 600 * (points + series + shapes), "{} questions", v.len());
        }
    }
    let _ = need_measures(&cs, &host, &p, &m);
    let _ = part_bounds(&cs, &host, &p, &m);
}

/// Seeds of `chart_emit`: charts with title, legend, chart drawing and picture fills, ordinary and hostile numbers.
pub fn chart_emit_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (ppt, rect, mode, flags) in [
        (4.0 / 3.0, [0.0, 0.0, 480.0, 300.0], 0u8, 1u8 | 8),
        (0.8833, [17.5, 9.25, 360.5, 240.25], 4, 2 | 16),
        (f64::NAN, [0.0, 0.0, 480.0, 300.0], 8, 1),
        (1.0, [f64::NAN, 0.0, 1e308, -5.0], 12, 1 | 4),
        (-3.0, [0.0, 0.0, 30.0, 30.0], 17, 1),
    ] {
        for chart in uo_charts_seed_charts() {
            let mut b = vec![flags];
            b.extend(ppt.to_le_bytes());
            for v in rect {
                b.extend(f64::to_le_bytes(v));
            }
            b.push(mode);
            b.extend(chart.0.as_bytes());
            b.push(0);
            b.extend(chart.1.as_bytes());
            out.push(b);
        }
    }
    out
}

fn uo_charts_seed_charts() -> Vec<(String, String)> {
    let pts = |n: usize, f: &dyn Fn(usize) -> String| -> String {
        (0..n).map(|i| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", f(i))).collect()
    };
    let ser = |i: usize| {
        format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:v>Series number {i} with a long name to wrap in a narrow legend box</c:v></c:tx><c:trendline><c:trendlineType val=\"linear\"/></c:trendline><c:cat><c:strLit><c:ptCount val=\"4\"/>{}</c:strLit></c:cat><c:val><c:numLit><c:ptCount val=\"4\"/>{}</c:numLit></c:val></c:ser>",
            pts(4, &|k| format!("Category {k}")),
            pts(4, &|k| (k * 3 + i).to_string())
        )
    };
    let bar = format!(
        "<c:chartSpace><c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>A rather long chart title that has to wrap into two lines for sure</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val=\"col\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart></c:plotArea><c:legend><c:legendPos val=\"b\"/><c:layout><c:manualLayout><c:x val=\"0.1\"/><c:y val=\"0.8\"/><c:w val=\"0.2\"/><c:h val=\"0.1\"/></c:manualLayout></c:layout></c:legend></c:chart><c:spPr><a:blipFill><a:blip r:embed=\"rId3\"/><a:tile tx=\"12700\"/></a:blipFill><a:ln w=\"9525\"/></c:spPr></c:chartSpace>",
        ser(0),
        ser(1)
    );
    let pie = format!(
        "<c:chartSpace><c:chart><c:plotArea><c:pieChart><c:varyColors val=\"1\"/>{}</c:pieChart></c:plotArea><c:legend><c:legendPos val=\"r\"/></c:legend></c:chart></c:chartSpace>",
        ser(0)
    );
    let drawing = "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0.9</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to><cdr:sp><cdr:spPr><a:prstGeom prst=\"roundRect\"/><a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></a:ln></cdr:spPr><cdr:txBody><a:bodyPr anchor=\"ctr\" rot=\"5400000\"/><a:p><a:r><a:t>Source: the fuzz seeds</a:t></a:r></a:p></cdr:txBody></cdr:sp></cdr:relSizeAnchor></c:userShapes>";
    // C-4/C-5: the families with axes (explicit axes, a secondary pair, titles, a log scale, a data table, 3D), XY with
    // trendlines and labels, radar, bar of pie, bubbles
    let axes = |inner: &str| {
        format!(
            "<c:chartSpace><c:chart><c:plotArea>{inner}</c:plotArea><c:legend><c:legendPos val=\"r\"/></c:legend></c:chart></c:chartSpace>"
        )
    };
    let cat = |id: u32, cross: u32, pos: &str| {
        format!(
            "<c:catAx><c:axId val=\"{id}\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{pos}\"/><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Categories</a:t></a:r></a:p></c:rich></c:tx></c:title><c:majorGridlines/><c:majorTickMark val=\"out\"/><c:tickLblPos val=\"nextTo\"/><c:txPr><a:bodyPr rot=\"-2700000\"/><a:p><a:pPr><a:defRPr sz=\"900\"/></a:pPr></a:p></c:txPr><c:crossAx val=\"{cross}\"/><c:crosses val=\"autoZero\"/><c:lblOffset val=\"100\"/></c:catAx>"
        )
    };
    let val = |id: u32, cross: u32, pos: &str, log: bool| {
        format!(
            "<c:valAx><c:axId val=\"{id}\"/><c:scaling>{}<c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{pos}\"/><c:majorGridlines/><c:minorGridlines/><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/><c:majorTickMark val=\"cross\"/><c:minorTickMark val=\"out\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"{cross}\"/><c:crosses val=\"max\"/><c:crossBetween val=\"between\"/><c:dispUnits><c:builtInUnit val=\"thousands\"/></c:dispUnits></c:valAx>",
            if log { "<c:logBase val=\"10\"/>" } else { "" }
        )
    };
    let labels = "<c:dLbls><c:numFmt formatCode=\"0.00\" sourceLinked=\"0\"/><c:showLegendKey val=\"1\"/><c:showVal val=\"1\"/><c:showCatName val=\"1\"/><c:showPercent val=\"1\"/></c:dLbls>";
    let combo = axes(&format!(
        "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"percentStacked\"/>{}{labels}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:lineChart><c:grouping val=\"standard\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>{}{}{}{}<c:dTable><c:showHorzBorder val=\"1\"/><c:showVertBorder val=\"1\"/><c:showOutline val=\"1\"/><c:showKeys val=\"1\"/></c:dTable>",
        ser(0),
        ser(1),
        cat(1, 2, "b"),
        val(2, 1, "l", false),
        cat(3, 4, "t"),
        val(4, 3, "r", true)
    ));
    let area3d = axes(&format!(
        "<c:view3D><c:rotX val=\"15\"/><c:rotY val=\"20\"/><c:rAngAx val=\"1\"/></c:view3D><c:area3DChart><c:grouping val=\"stacked\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:area3DChart><c:bar3DChart><c:barDir val=\"bar\"/><c:grouping val=\"clustered\"/>{}<c:gapDepth val=\"300\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:bar3DChart>{}{}",
        ser(0),
        ser(1),
        ser(2),
        cat(1, 2, "b"),
        val(2, 1, "l", false)
    ));
    let xy = axes(&format!(
        "<c:scatterChart><c:scatterStyle val=\"smoothMarker\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:scatterChart><c:bubbleChart><c:bubbleScale val=\"150\"/>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:bubbleChart>{}{}",
        ser(0).replace("linear", "poly\"/><c:order val=\"3\"/><c:dispEq val=\"1\"/><c:dispRSqr val=\"1"),
        ser(1).replace("linear", "power"),
        ser(2),
        val(1, 2, "b", false),
        val(2, 1, "l", false)
    ));
    let radar = format!(
        "<c:chartSpace><c:chart><c:plotArea><c:radarChart><c:radarStyle val=\"filled\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:radarChart>{}{}</c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1),
        cat(1, 2, "b"),
        val(2, 1, "l", false)
    );
    let ofpie = format!(
        "<c:chartSpace><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val=\"bar\"/><c:varyColors val=\"1\"/>{}{labels}<c:splitType val=\"val\"/><c:splitPos val=\"5\"/><c:serLines/></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>",
        ser(0)
    );
    let doughnut = format!(
        "<c:chartSpace><c:chart><c:view3D><c:rotX val=\"40\"/></c:view3D><c:plotArea><c:doughnutChart><c:varyColors val=\"1\"/>{}{}{labels}<c:holeSize val=\"40\"/></c:doughnutChart></c:plotArea></c:chart></c:chartSpace>",
        ser(0),
        ser(1)
    );
    vec![
        (bar, drawing.to_owned()),
        (pie, String::new()),
        ("<c:chartSpace><c:chart/></c:chartSpace>".to_owned(), String::new()),
        (combo, String::new()),
        (area3d, String::new()),
        (xy, String::new()),
        (radar, String::new()),
        (ofpie, String::new()),
        (doughnut, String::new()),
    ]
}
