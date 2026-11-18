//! The PowerPoint-over-TS chart rules of Slides wave 5 (`uo_charts::rules::ChartRules`, record `uo-charts.md` "PowerPoint
//! rules (wave 5)") against the values PowerPoint 365 reports through its object model (`PlotArea.Inside*`, `Legend`,
//! `ChartTitle`) for the same charts (lab `c-pie`/`c-r2`: an 800 × 420 pt frame, the plot positions here are relative to the
//! frame = PowerPoint's chart coordinates + 4 pt). `ts_strict()` keeps the TS numbers.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

mod c1_support;

use uo_charts::emit::{ChartSel, DataSel, EmitParams, HostProfile, Rect};
use uo_charts::host::{ChartImageInfo, FileFormat};

use uo_charts::measure::{ChartFace, ChartMeasure, FontBox};
use uo_charts::model::BlipKey;
use uo_charts::rules::ChartRules;
use uo_charts::{LayoutCall, NullCanvas, parse_chart_space, part_bounds, trace_layout};

use c1_support::corpus::{PlainHost, doc};

/// Widths of 0.55 em per character, the font box 0.9 / 0.2 em.
struct Synth;

impl ChartMeasure for Synth {
    fn width(&self, _: &ChartFace, size_px: f64, text: &str) -> Option<f64> {
        Some(text.chars().count() as f64 * size_px * 0.55)
    }
    fn font_box(&self, _: &ChartFace, size_px: f64) -> Option<FontBox> {
        Some(FontBox { ascent: Some(size_px * 0.9), descent: Some(size_px * 0.2) })
    }
    fn has_font_bounding_box(&self) -> bool {
        true
    }
    fn image(&self, _: BlipKey) -> Option<Option<ChartImageInfo>> {
        Some(None)
    }
}

const FRAME: Rect = Rect { x: 0.0, y: 0.0, w: 800.0, h: 420.0 };

fn params(rect: Rect, rules: ChartRules, sel: ChartSel) -> EmitParams<'static> {
    EmitParams {
        rect,
        px_per_pt: 1.0,
        profile: HostProfile { area_fill_bg1: false, area_line: None, excel_axis_density: false },
        locale: uo_numfmt::pt_br(),
        sel,
        images: false,
        rules,
    }
}

const TX: &str = "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"1197\"/></a:pPr><a:endParaRPr lang=\"pt-BR\"/></a:p></c:txPr>";

/// A chart in the form PowerPoint 365 writes it (c14:style 102), one series "Vendas" of four points.
fn chart(kind: &str, atd: Option<bool>, legend: Option<&str>, dlbls: &str, extra_group: &str) -> String {
    let dpts: String = (0..4)
        .map(|k| {
            format!(
                "<c:dPt><c:idx val=\"{k}\"/><c:bubble3D val=\"0\"/><c:spPr><a:solidFill><a:schemeClr val=\"accent{}\"/></a:solidFill><a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"lt1\"/></a:solidFill></a:ln></c:spPr></c:dPt>",
                k + 1
            )
        })
        .collect();
    let ser = format!(
        "<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx><c:strRef><c:f>P!$B$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>Vendas</c:v></c:pt></c:strCache></c:strRef></c:tx>{}<c:cat><c:strRef><c:f>P!$A$2:$A$5</c:f><c:strCache><c:ptCount val=\"4\"/><c:pt idx=\"0\"><c:v>Cat A</c:v></c:pt><c:pt idx=\"1\"><c:v>Cat B</c:v></c:pt><c:pt idx=\"2\"><c:v>Cat C</c:v></c:pt><c:pt idx=\"3\"><c:v>Cat D</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>P!$B$2:$B$5</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"4\"/><c:pt idx=\"0\"><c:v>8.2</c:v></c:pt><c:pt idx=\"1\"><c:v>3.2</c:v></c:pt><c:pt idx=\"2\"><c:v>1.4</c:v></c:pt><c:pt idx=\"3\"><c:v>1.2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>",
        if kind == "bar" { "" } else { &dpts }
    );
    let group = match kind {
        "pie" => format!("<c:pieChart><c:varyColors val=\"1\"/>{ser}{dlbls}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
        "doughnut" => format!(
            "<c:doughnutChart><c:varyColors val=\"1\"/>{ser}{dlbls}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>"
        ),
        _ => format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>{ser}<c:gapWidth val=\"219\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart><c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:catAx><c:valAx><c:axId val=\"2\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"l\"/><c:crossAx val=\"1\"/></c:valAx>"
        ),
    };
    let atd = atd.map_or(String::new(), |v| format!("<c:autoTitleDeleted val=\"{}\"/>", u8::from(v)));
    let legend = legend.map_or(String::new(), |p| format!("<c:legend><c:legendPos val=\"{p}\"/><c:overlay val=\"0\"/>{TX}</c:legend>"));
    format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><c:roundedCorners val=\"0\"/><mc:AlternateContent><mc:Choice Requires=\"c14\" xmlns:c14=\"http://schemas.microsoft.com/office/drawing/2007/8/2/chart\"><c14:style val=\"102\"/></mc:Choice><mc:Fallback><c:style val=\"2\"/></mc:Fallback></mc:AlternateContent><c:chart>{atd}<c:plotArea><c:layout/>{group}{extra_group}</c:plotArea>{legend}<c:plotVisOnly val=\"1\"/></c:chart></c:chartSpace>"
    )
}

/// The box of what the data of the chart draws (the pie: its square, plus half the 1.5 pt white slice border).
fn data_box(xml: &str, rect: Rect, rules: ChartRules) -> Rect {
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let cs = parse_chart_space(doc(xml).root(), &host);
    part_bounds(&cs, &host, &params(rect, rules, ChartSel { bg: false, data: DataSel::All }), &Synth).unwrap().unwrap()
}

fn assert_square(b: Rect, x: f64, y: f64, side: f64, tol: f64) {
    let pad = 0.75; // half the slice border
    for (got, want, what) in [(b.x + pad, x, "x"), (b.y + pad, y, "y"), (b.w - 2.0 * pad, side, "w"), (b.h - 2.0 * pad, side, "h")] {
        assert!((got - want).abs() <= tol, "{what}: {got} vs PowerPoint {want} (box {b:?})");
    }
}

#[test]
fn pie_fills_the_plot_square_11_pt_from_the_frame() {
    // c-pie #4: inside 197;7;398;398 → frame 201, 11, 398
    let xml = chart("pie", Some(true), None, "", "");
    assert_square(data_box(&xml, FRAME, ChartRules::default()), 201.0, 11.0, 398.0, 0.01);
    // c-pie #5 (400 × 300): inside 57;7;278;278
    assert_square(data_box(&xml, Rect { w: 400.0, h: 300.0, ..FRAME }, ChartRules::default()), 61.0, 11.0, 278.0, 0.01);
    // the TS: 97 % of the square inside its own pads
    let ts = data_box(&xml, FRAME, ChartRules::ts_strict());
    assert!((ts.w - 1.5 - 395.566).abs() < 0.01, "TS diameter {}", ts.w - 1.5);
}

#[test]
fn outside_labels_shrink_the_pie_by_a_fixed_rule() {
    // c-pie #16/#17, c-r2 #5–#9: bestFit/outEnd/no position at 8–24 pt, all inside 223.25;33.25;345.5 = 0.875 × 398 − 2.75
    for (pos, sz) in
        [("<c:dLblPos val=\"bestFit\"/>", 1197), ("<c:dLblPos val=\"outEnd\"/>", 800), ("<c:dLblPos val=\"outEnd\"/>", 2400), ("", 1200)]
    {
        let dl = format!(
            "<c:dLbls><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"{sz}\"/></a:pPr><a:endParaRPr lang=\"pt-BR\"/></a:p></c:txPr>{pos}<c:showLegendKey val=\"0\"/><c:showVal val=\"0\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"1\"/><c:showBubbleSize val=\"0\"/></c:dLbls>"
        );
        let b = data_box(&chart("pie", Some(true), None, &dl, ""), FRAME, ChartRules::default());
        assert_square(b, 227.25, 37.25, 345.5, 0.01);
        // a doughnut's labels count too (c-r2 #10/#11)
        let d = data_box(&chart("doughnut", Some(true), None, &dl, ""), FRAME, ChartRules::default());
        assert_square(d, 227.25, 37.25, 345.5, 0.01);
    }
    // inEnd / ctr: no inset (c-pie #18/#19)
    let dl = "<c:dLbls><c:dLblPos val=\"ctr\"/><c:showLegendKey val=\"0\"/><c:showVal val=\"0\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"1\"/><c:showBubbleSize val=\"0\"/></c:dLbls>";
    assert_square(data_box(&chart("pie", Some(true), None, dl, ""), FRAME, ChartRules::default()), 201.0, 11.0, 398.0, 0.01);
}

#[test]
fn legends_sit_6_pt_from_the_edge_and_11_pt_from_the_plot() {
    // c-pie #2 (bottom legend of 20.464 pt in PowerPoint; the TS row is 20.364): inside 210.232;7;371.536
    let b = data_box(&chart("pie", Some(true), Some("b"), "", ""), FRAME, ChartRules::default());
    assert_square(b, 214.232 - 0.05, 11.0, 371.536 + 0.1, 0.06);
    // c-pie #8 (top legend): inside 210.232;33.464;371.536
    let t = data_box(&chart("pie", Some(true), Some("t"), "", ""), FRAME, ChartRules::default());
    assert_square(t, 214.232 - 0.05, 37.464 - 0.1, 371.536 + 0.1, 0.06);
}

fn title_of(xml: &str, rules: ChartRules) -> Option<(f64, f64)> {
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let cs = parse_chart_space(doc(xml).root(), &host);
    let (_, trace) = trace_layout(&cs, &host, &params(FRAME, rules, ChartSel { bg: true, data: DataSel::All }), &Synth, &mut NullCanvas);
    trace.calls.iter().find_map(|c| match c {
        LayoutCall::Title { res: Some(t), .. } => Some((t.y, t.lines.first()?.first()?.style.sz)),
        _ => None,
    })
}

#[test]
fn automatic_title_follows_powerpoint() {
    // c-pie #1, #21: one series, autoTitleDeleted=0 written, no c:title → the series name, 18 pt bold with c14 (the text
    // 1.5 pt below the title box at 6 pt)
    for kind in ["pie", "bar"] {
        let (y, sz) = title_of(&chart(kind, Some(false), Some("b"), "", ""), ChartRules::default()).expect("automatic title");
        assert_eq!(sz, 18.0);
        assert_eq!(y, 7.5);
        // the TS: no automatic title in a 2010+ file
        assert!(title_of(&chart(kind, Some(false), Some("b"), "", ""), ChartRules::ts_strict()).is_none());
    }
    // c-r2 #2: the element absent = no title
    assert!(title_of(&chart("bar", None, Some("b"), "", ""), ChartRules::default()).is_none());
    // deleted
    assert!(title_of(&chart("bar", Some(true), Some("b"), "", ""), ChartRules::default()).is_none());
    // c-r2 #1: without c14:style the base size (10 pt)
    let no_c14 =
        chart("bar", Some(false), Some("b"), "", "").replace("<c14:style val=\"102\"/>", "").replace("Requires=\"c14\"", "Requires=\"zz\"");
    let (_, sz) = title_of(&no_c14, ChartRules::default()).expect("automatic title");
    assert_eq!(sz, 10.0);
}

#[test]
fn sys_clr_uses_the_windows_table_unless_ts() {
    // the same table as the Slides theme's (uo_dml_render::theme::system_color, measured by the theme frontier)
    for (k, v) in uo_charts::color::SYSTEM_COLORS {
        assert_eq!(uo_dml_render::theme::system_color(k), Some(v), "{k}");
    }
    let d = doc("<a:sysClr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" val=\"highlight\" lastClr=\"123456\"/>");
    let sc = uo_charts::Scheme::default();
    assert_eq!(uo_charts::dml_color_with(d.root(), &sc, false).unwrap().as_str(), "#0078D7");
    assert_eq!(uo_charts::dml_color_with(d.root(), &sc, true).unwrap().as_str(), "#123456");
    assert_eq!(uo_charts::dml_color(d.root(), &sc).unwrap().as_str(), "#123456");
}

fn legend_of(xml: &str, rules: ChartRules) -> uo_charts::layout::LegendLayout {
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let cs = parse_chart_space(doc(xml).root(), &host);
    let (_, trace) = trace_layout(&cs, &host, &params(FRAME, rules, ChartSel { bg: true, data: DataSel::All }), &Synth, &mut NullCanvas);
    trace
        .calls
        .iter()
        .find_map(|c| match c {
            LayoutCall::Legend { res: Some(l), .. } => Some(l.clone()),
            _ => None,
        })
        .expect("a legend")
}

#[test]
fn horizontal_legend_follows_powerpoints_spread() {
    // lab c-leg (18 legends): box = Σ(1.2 × text + 1.318 em) centred on the frame; s = (box − Σ(0.797 em + text) + 0.305 em)
    // ÷ (n + 1); the first key s + 0.4 pt in; keys 0.549 em. "Cat A–D" at 11.97 pt with the synthetic 0.55-em characters:
    let em = 11.97;
    let adv = 5.0 * em * 0.55;
    let w = 4.0 * (1.2 * adv + 1.318 * em);
    let sp = (w - 4.0 * (adv + 0.797 * em) + 0.305 * em) / 5.0;
    let l = legend_of(&chart("pie", Some(true), Some("b"), "", ""), ChartRules::default());
    assert!((l.bx.w - w).abs() < 1e-9 && (l.bx.x - (800.0 - w) / 2.0).abs() < 1e-9, "{:?}", l.bx);
    for (i, r) in l.rows.iter().enumerate() {
        let want = l.bx.x + sp + 0.4 + i as f64 * (adv + 0.797 * em + sp);
        assert!((r.x - want).abs() < 1e-9, "row {i}: {} vs {want}", r.x);
    }
    // the TS: 1.2-em spacing from the inner box
    let ts = legend_of(&chart("pie", Some(true), Some("b"), "", ""), ChartRules::ts_strict());
    assert!((ts.rows[1].x - ts.rows[0].x - (0.77 * em + adv + 1.2 * em)).abs() < 1e-9);
}

#[test]
fn corner_legend_and_its_pie() {
    // lab c-leg #15–#17: the `tr` box at the top right (6 pt), width = widest text + 1.1 em + 6; the plot box below it and
    // 11 pt left of it, the square 6 pt above that box's top
    let em = 11.97;
    let adv = 5.0 * em * 0.55;
    let l = legend_of(&chart("pie", Some(true), Some("tr"), "", ""), ChartRules::default());
    let w = adv + 1.1 * em + 6.0;
    assert!((l.bx.x - (794.0 - w)).abs() < 1e-9 && (l.bx.y - 6.0).abs() < 1e-9, "{:?}", l.bx);
    let p_top = l.bx.y + l.bx.h + 11.0;
    let side = 409.0 - p_top;
    let p_right = l.bx.x - 11.0;
    let b = data_box(&chart("pie", Some(true), Some("tr"), "", ""), FRAME, ChartRules::default());
    assert_square(b, (11.0 + p_right) / 2.0 - side / 2.0, p_top - 6.0, side, 1e-6);
}

#[test]
fn title_line_is_the_font_line() {
    // PowerPoint's title box = the font's line (ascent + descent) + 3 pt; the synthetic font box is 0.9 + 0.2 em
    let host = PlainHost { format: Some(FileFormat::Modern), shapes: None };
    let xml = chart("pie", Some(false), None, "", "");
    let cs = parse_chart_space(doc(&xml).root(), &host);
    let (_, trace) =
        trace_layout(&cs, &host, &params(FRAME, ChartRules::default(), ChartSel { bg: true, data: DataSel::All }), &Synth, &mut NullCanvas);
    let t = trace.calls.iter().find_map(|c| if let LayoutCall::Title { res: Some(t), .. } = c { Some(t.clone()) } else { None }).unwrap();
    assert!((t.h - 18.0 * 1.1).abs() < 1e-9, "{}", t.h);
    // the pie under it: 11 pt below the box (7.5 + h + 1.5)
    let b = data_box(&xml, FRAME, ChartRules::default());
    assert!((b.y + 0.75 - (7.5 + t.h + 1.5 + 11.0)).abs() < 1e-9);
}
