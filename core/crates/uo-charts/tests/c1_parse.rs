//! Unit tests of frontier C-1 (model, parser, colours): the expected values of `charts.test.ts`
//! (cases that read the model or the colours), the rules measured in the Office that the parser encodes,
//! and the JS traps of the port. The parity replay against the TS captures is `parity.rs` (C-0).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used, clippy::too_many_lines)]

use uo_charts::color::{chart_shade_tint, tint_linear};
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{
    AxisKind, BlipTile, ChartColor, ChartSpace, FillRect, GradPath, GroupKind, LegendPos, Paint, TextProps, TickMark, TileFlip,
};
use uo_charts::{ChartType, Scheme, dml_color, parse_chart_space, parse_paint, parse_paint_node, summarize};
use uo_opc::xml::{Document, Element, ParseOptions, parse};

fn xml(src: &str) -> Document {
    parse(src.to_owned(), &ParseOptions::LENIENT).expect("test xml parses")
}

const THEME: [(&str, &str); 8] = [
    ("accent1", "#156082"),
    ("accent2", "#E97132"),
    ("accent3", "#196B24"),
    ("accent4", "#0F9ED5"),
    ("accent5", "#A02B93"),
    ("accent6", "#4EA72E"),
    ("tx1", "#000000"),
    ("bg1", "#FFFFFF"),
];

/// The host of the TS tests: `a:srgbClr` → `#val`, scheme colours from the Office theme, minor font Aptos.
#[derive(Default)]
struct Host {
    format: Option<FileFormat>,
    shapes: Option<Document>,
    widths: Vec<f64>,
    fill_style: bool,
    blips: bool,
}

impl ChartHost for Host {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        let theme = |v: &str| THEME.iter().find(|(k, _)| *k == v).map_or("#000000", |(_, c)| *c);
        Some(ChartColor::new(&match src {
            ColorSource::Scheme(v) => theme(v).to_owned(),
            ColorSource::Element(e) if e.name() == "a:srgbClr" => format!("#{}", e.attr("val").unwrap_or("")),
            ColorSource::Element(e) => theme(e.attr("val").unwrap_or("")).to_owned(),
        }))
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: None, minor: Some("Aptos") }
    }
    fn has_fill_style(&self) -> bool {
        self.fill_style
    }
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        Some(Paint::Solid { color: ChartColor::new(&format!("{ph_clr}{idx}")), alpha: None })
    }
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        (self.blips && blip.attr("r:embed") == Some("rId3")).then_some(BlipKey(7))
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

fn cache(v: &[&str], str_cache: bool) -> String {
    let tag = if str_cache { "str" } else { "num" };
    let pts: String = v.iter().enumerate().map(|(i, x)| format!("<c:pt idx=\"{i}\"><c:v>{x}</c:v></c:pt>")).collect();
    format!("<c:{tag}Cache><c:ptCount val=\"{}\"/>{pts}</c:{tag}Cache>", v.len())
}

fn ser(i: usize, v: &[f64], extra: &str) -> String {
    let nums: Vec<String> = v.iter().map(f64::to_string).collect();
    let nums: Vec<&str> = nums.iter().map(String::as_str).collect();
    format!(
        "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:v>S{}</c:v></c:tx>{extra}<c:cat><c:strRef>{}</c:strRef></c:cat><c:val><c:numRef>{}</c:numRef></c:val></c:ser>",
        i + 1,
        cache(&["A", "B", "C", "D"], true),
        cache(&nums, false)
    )
}

const AX: &str = "<c:catAx><c:axId val=\"1\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:catAx><c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/><c:majorGridlines/><c:crossAx val=\"1\"/></c:valAx>";

fn space(plot: &str, extra: &str) -> Document {
    xml(&format!("<c:chartSpace><c:chart><c:plotArea>{plot}{AX}</c:plotArea>{extra}</c:chart></c:chartSpace>"))
}

fn parsed(doc: &Document, host: &Host) -> ChartSpace {
    parse_chart_space(doc.root(), host)
}

fn doc(inner: &str, plot: &str) -> Document {
    xml(&format!("<c:chartSpace>{inner}<c:chart><c:plotArea>{plot}{AX}</c:plotArea></c:chart></c:chartSpace>"))
}

fn bar(body: &str, extra: &str) -> String {
    format!("<c:barChart><c:barDir val=\"col\"/>{extra}{body}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>")
}

fn hex(c: &ChartColor) -> &str {
    c.as_str()
}

#[test]
fn automatic_colours_in_linear_rgb() {
    assert_eq!(chart_shade_tint("#156082", -0.3), "#0E4965");
    assert_eq!(chart_shade_tint("#156082", 0.3), "#AFBAC4");
    assert_eq!(tint_linear("#000000", 0.75), "#898989");
}

#[test]
fn dml_color_resolver_of_a_host_without_one() {
    let sc = Scheme::from_pairs(&[("dk1", "#000000"), ("lt1", "#FFFFFF"), ("accent1", "#156082")]);
    let d = xml("<a:schemeClr val=\"tx1\"><a:lumMod val=\"65000\"/><a:lumOff val=\"35000\"/></a:schemeClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#595959");
    let d = xml("<a:schemeClr val=\"tx1\"><a:tint val=\"75000\"/></a:schemeClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#898989");
    // plain colours: srgb keeps the last six characters, prst/sys have their tables, alpha does not "touch"
    let d = xml("<a:srgbClr val=\"ff8000\"><a:alpha val=\"50000\"/></a:srgbClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#FF8000");
    let d = xml("<a:srgbClr val=\"0x4472C4\"/>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#4472C4");
    let d = xml("<a:prstClr val=\"lightGray\"/>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#D3D3D3");
    let d = xml("<a:sysClr val=\"window\"/>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#FFFFFF");
    let d = xml("<a:schemeClr val=\"accent9\"/>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#000000");
    // not a colour element
    let d = xml("<a:noFill/>");
    assert!(dml_color(d.root(), &sc).is_none());
    // measured: shade 15% of 04617B is 012430; satMod without ceiling; a hostile modifier gives NaN text
    let d = xml("<a:srgbClr val=\"04617B\"><a:shade val=\"15000\"/></a:srgbClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#012430");
    let d = xml("<a:srgbClr val=\"04617B\"><a:satMod val=\"320000\"/></a:srgbClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#00ABFE");
    let d = xml("<a:srgbClr val=\"04617B\"><a:shade val=\"x\"/></a:srgbClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#NANNANNAN");
    // gamma mode: shade in sRGB
    let d = xml("<a:srgbClr val=\"808080\"><a:gamma/><a:shade val=\"50000\"/><a:invGamma/></a:srgbClr>");
    assert_eq!(dml_color(d.root(), &sc).unwrap().as_str(), "#404040");
}

#[test]
fn style_axes_and_smooth() {
    let d = xml(&format!(
        "<c:chartSpace><mc:AlternateContent><mc:Choice><c14:style val=\"118\"/></mc:Choice></mc:AlternateContent><c:chart><c:plotArea><c:lineChart>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>{AX}</c:plotArea></c:chart></c:chartSpace>",
        ser(0, &[1.0, 2.0, 3.0, 4.0], "")
    ));
    let cs = parsed(&d, &Host::default());
    assert_eq!(cs.style, 18.0);
    assert!(cs.office2010);
    assert_eq!(cs.groups[0].series[0].smooth, None);
    assert_eq!(cs.axes.iter().map(|a| a.kind).collect::<Vec<_>>(), [AxisKind::Cat, AxisKind::Val]);
    assert_eq!(cs.accents[0].as_str(), "#156082");
    assert_eq!(cs.tx1.as_str(), "#000000");
    assert_eq!(cs.minor_font.as_deref(), Some("Aptos"));
    assert_eq!(cs.major_font, None);
    assert!(!cs.has_theme_fill);
    // eight series of one bar group
    let d = space(&bar(&(0..8).map(|i| ser(i, &[1.0, 2.0, 3.0, 4.0], "")).collect::<String>(), ""), "");
    assert_eq!(parsed(&d, &Host::default()).groups[0].series.len(), 8);
    // a missing c14:style value and a plain c:style
    let d = xml("<c:chartSpace><c:style val=\"41\"/><c:chart/></c:chartSpace>");
    assert_eq!(parsed(&d, &Host::default()).style, 41.0);
    let d = xml("<c:chartSpace/>");
    assert_eq!(parsed(&d, &Host::default()).style, 2.0);
    let d = xml("<c:chartSpace><mc:AlternateContent><c14:style val=\"x\"/></mc:AlternateContent></c:chartSpace>");
    assert!(parsed(&d, &Host::default()).style.is_nan());
}

#[test]
fn a_group_without_axes_gets_the_default_pair() {
    let d = xml(&format!(
        "<c:chartSpace><c:chart><c:plotArea><c:barChart>{}</c:barChart></c:plotArea></c:chart></c:chartSpace>",
        ser(0, &[1.0, 2.0], "")
    ));
    let cs = parsed(&d, &Host::default());
    assert_eq!(cs.groups[0].ax_ids.len(), 2);
    assert_eq!(cs.axes.len(), 2);
    assert_eq!(&*cs.axes[0].id, "__x0");
    assert_eq!(&*cs.axes[1].id, "__y0");
    assert_eq!(cs.axes[0].cross_ax.as_deref(), Some("__y0"));
    assert!(cs.axes[1].major_grid.is_some());
    // a horizontal bar flips the positions
    let d =
        xml("<c:chartSpace><c:chart><c:plotArea><c:barChart><c:barDir val=\"bar\"/></c:barChart></c:plotArea></c:chart></c:chartSpace>");
    let cs = parsed(&d, &Host::default());
    assert_eq!(format!("{:?}", cs.axes[0].pos), "L");
    assert_eq!(format!("{:?}", cs.axes[1].pos), "B");
    // pies need no axes; a plot with one unmatched axis id links to the existing pair
    let d = xml("<c:chartSpace><c:chart><c:plotArea><c:pieChart/></c:plotArea></c:chart></c:chartSpace>");
    assert!(parsed(&d, &Host::default()).axes.is_empty());
    let d = xml(
        "<c:chartSpace><c:chart><c:plotArea><c:lineChart><c:axId val=\"9\"/></c:lineChart><c:catAx><c:axId val=\"\"/></c:catAx></c:plotArea></c:chart></c:chartSpace>",
    );
    let cs = parsed(&d, &Host::default());
    assert_eq!(cs.axes.len(), 2);
    assert_eq!(&*cs.axes[0].id, "_x0");
    assert_eq!(&*cs.axes[1].id, "__y0");
    assert_eq!(cs.axes[0].cross_ax.as_deref(), Some("__y0"));
    assert_eq!(&*cs.groups[0].ax_ids[0], "_x0");
}

#[test]
fn office_2007_files() {
    let h = Host { format: Some(FileFormat::Mso2007), ..Host::default() };
    let d = doc("", &bar(&ser(0, &[1.0, -2.0, 3.0, 4.0], "<c:invertIfNegative/>"), "<c:varyColors/>"));
    let cs = parsed(&d, &h);
    assert!(!cs.groups[0].vary_colors);
    assert_eq!(cs.groups[0].series[0].invert_if_negative, Some(false));
    assert!(cs.axes.iter().all(|a| a.major_tick == TickMark::Out && a.minor_tick == TickMark::None));
    assert!(!cs.rounded_corners);
    assert!(cs.office2010);
    assert!(cs.title.is_some());
    let d = doc("", &format!("<c:lineChart>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>", ser(0, &[1.0, 2.0], "")));
    let ln = parsed(&d, &h);
    assert_eq!(ln.groups[0].series[0].smooth, Some(false));
    assert!(ln.groups[0].show_marker);
}

#[test]
fn modern_files() {
    let h = Host { format: Some(FileFormat::Modern), ..Host::default() };
    let d = doc("", &bar(&ser(0, &[1.0, 2.0], "<c:dLbls><c:showVal val=\"1\"/></c:dLbls>"), ""));
    let cs = parsed(&d, &h);
    assert!(cs.axes.iter().all(|a| a.deleted));
    assert!(cs.groups[0].vary_colors);
    let dl = cs.groups[0].series[0].d_lbls.as_ref().unwrap();
    assert_eq!((dl.common.show_val, dl.common.show_cat, dl.common.show_ser), (Some(true), Some(true), Some(true)));
    assert!(cs.rounded_corners);
    assert!(cs.title.is_none());
    // bars without invertIfNegative invert; a legend without overlay overlaps; trendlines show eq and R²
    let d = doc("", &bar(&ser(0, &[1.0, -2.0], "<c:trendline/>"), ""));
    let cs = parsed(&d, &h);
    assert_eq!(cs.groups[0].series[0].invert_if_negative, Some(true));
    assert!(cs.groups[0].series[0].trendlines[0].disp_eq && cs.groups[0].series[0].trendlines[0].disp_r_sqr);
    let d = xml("<c:chartSpace><c:chart><c:legend><c:legendPos val=\"t\"/></c:legend></c:chart></c:chartSpace>");
    let lg = parsed(&d, &h).legend.unwrap();
    assert!(lg.overlay);
    assert_eq!(lg.pos, LegendPos::T);
    // a host without file format (the Sheet): the earlier behaviour
    let d = doc("", &bar(&ser(0, &[1.0, 2.0], ""), "<c:varyColors/>"));
    let cs = parsed(&d, &Host::default());
    assert!(cs.groups[0].vary_colors);
    assert!(cs.axes.iter().all(|a| !a.deleted));
    assert_eq!(cs.axes[0].major_tick, TickMark::Cross);
}

#[test]
fn point_format_sparse_pts_and_levels() {
    let pts = "<c:val><c:numRef><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"2\"/><c:pt idx=\"0\" formatCode=\"0%\"><c:v>0.29</c:v></c:pt><c:pt idx=\"1\"><c:v>0.5</c:v></c:pt></c:numCache></c:numRef></c:val>";
    let s = format!("<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:dLbls><c:showVal val=\"1\"/></c:dLbls>{pts}</c:ser>");
    let cs = parsed(&space(&bar(&s, ""), ""), &Host::default());
    let s0 = &cs.groups[0].series[0];
    assert_eq!(s0.values, [Some(0.29), Some(0.5)]);
    assert_eq!(s0.fmt.as_ref().unwrap().units(), "General".encode_utf16().collect::<Vec<_>>());
    let pf = s0.pt_fmts.as_ref().unwrap();
    assert_eq!(pf.len(), 1);
    assert_eq!(pf[0].as_ref().unwrap().units(), "0%".encode_utf16().collect::<Vec<_>>());
    // sparse points: holes are null, text is NaN → null, ptCount pads, idx at the cap is skipped
    let s = "<c:ser><c:val><c:numLit><c:ptCount val=\"6\"/><c:pt idx=\"3\"><c:v>7</c:v></c:pt><c:pt idx=\"1\"><c:v>x</c:v></c:pt><c:pt idx=\"-1\"><c:v>9</c:v></c:pt><c:pt idx=\"100000\"><c:v>9</c:v></c:pt><c:pt idx=\"3\"><c:v>8</c:v></c:pt></c:numLit></c:val></c:ser>";
    let cs = parsed(&space(&bar(s, ""), ""), &Host::default());
    assert_eq!(cs.groups[0].series[0].values, [None, None, None, Some(8.0), None, None]);
    // multi-level categories: the first level are the labels, the rest the levels; a level borrows ptCount
    let s = "<c:ser><c:cat><c:multiLvlStrRef><c:multiLvlStrCache><c:ptCount val=\"3\"/><c:lvl><c:pt idx=\"0\"><c:v>a</c:v></c:pt><c:pt idx=\"2\"><c:v>c</c:v></c:pt></c:lvl><c:lvl><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>G</c:v></c:pt></c:lvl></c:multiLvlStrCache></c:multiLvlStrRef></c:cat></c:ser>";
    let cs = parsed(&space(&bar(s, ""), ""), &Host::default());
    let s0 = &cs.groups[0].series[0];
    assert_eq!(s0.cats.as_ref().unwrap().iter().map(|c| &**c).collect::<Vec<_>>(), ["a", "", "c"]);
    assert_eq!(s0.cat_levels.as_ref().unwrap()[0].iter().map(|c| &**c).collect::<Vec<_>>(), ["G"]);
    // numeric categories, scatter x values
    let s = "<c:ser><c:xVal><c:numRef><c:numCache><c:formatCode>m/d/yyyy</c:formatCode><c:ptCount val=\"2\"/><c:pt idx=\"0\"><c:v>43101</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numLit><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>2</c:v></c:pt></c:numLit></c:yVal></c:ser>";
    let d = xml(&format!("<c:chartSpace><c:chart><c:plotArea><c:scatterChart>{s}</c:scatterChart></c:plotArea></c:chart></c:chartSpace>"));
    let cs = parsed(&d, &Host::default());
    let s0 = &cs.groups[0].series[0];
    assert_eq!(s0.x_vals, Some(vec![Some(43101.0), None]));
    assert_eq!(s0.cat_nums, Some(vec![Some(43101.0), None]));
    assert!(s0.cat_fmt.is_some());
    assert_eq!(s0.values, [Some(2.0)]);
    assert!(cs.groups[0].show_marker);
}

#[test]
fn view_3d_and_data_table() {
    let d = xml(&format!(
        "<c:chartSpace><c:chart><c:view3D><c:rotX val=\"30\"/><c:hPercent val=\"50\"/><c:rAngAx val=\"0\"/></c:view3D><c:plotArea><c:bar3DChart><c:barDir val=\"col\"/>{}<c:gapDepth val=\"300\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:bar3DChart>{AX}</c:plotArea></c:chart></c:chartSpace>",
        ser(0, &[1.0, 2.0], "")
    ));
    let cs = parsed(&d, &Host::default());
    let v = cs.view_3d.unwrap();
    assert_eq!(
        (v.rot_x, v.h_percent, v.r_ang_ax, v.rot_y, v.perspective, v.depth_percent),
        (Some(30.0), Some(50.0), false, None, 30.0, 100.0)
    );
    assert_eq!(cs.groups[0].gap_depth, Some(300.0));
    assert!(cs.groups[0].three_d);
    let dt =
        "<c:dTable><c:showHorzBorder val=\"1\"/><c:showVertBorder val=\"1\"/><c:showOutline val=\"1\"/><c:showKeys val=\"1\"/></c:dTable>";
    let cs = parsed(
        &space(&format!("<c:lineChart>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>{dt}", ser(0, &[1.5, 2.0, 3.0, 4.0], "")), ""),
        &Host::default(),
    );
    let t = cs.d_table.unwrap();
    assert!(t.horz && t.vert && t.outline && t.keys);
}

#[test]
fn rich_titles() {
    let mk = |p: &str| {
        xml(&format!(
            "<c:chartSpace><c:chart><c:title><c:tx><c:rich><a:bodyPr rot=\"-5400000\"/>{p}</c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:plotArea><c:barChart>{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>{AX}</c:plotArea></c:chart></c:chartSpace>",
            ser(0, &[1.0, 2.0], "")
        ))
    };
    let d = mk("<a:p><a:r><a:t>T</a:t></a:r></a:p>");
    let cs = parsed(&d, &Host::default());
    let t = cs.title.unwrap();
    assert!(t.paras.as_ref().unwrap()[0].no_def_rpr);
    assert_eq!(t.tx_pr.rot, Some(-90.0));
    let d = mk(
        "<a:p><a:pPr algn=\"ctr\"><a:defRPr sz=\"1400\" b=\"1\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:pPr><a:r><a:rPr u=\"sng\"/><a:t>T</a:t></a:r><a:br/><a:r><a:t>U</a:t></a:r></a:p>",
    );
    let cs = parsed(&d, &Host::default());
    let t = cs.title.unwrap();
    let p = &t.paras.unwrap()[0];
    assert!(!p.no_def_rpr);
    assert_eq!(p.align.as_deref(), Some("ctr"));
    assert_eq!(p.props.sz, Some(14.0));
    assert_eq!(p.props.b, Some(true));
    assert_eq!(p.props.color.as_ref().map(hex), Some("#FF0000"));
    assert_eq!(p.props.font.as_deref(), Some("Aptos"));
    assert_eq!(p.runs.iter().map(|r| &*r.text).collect::<Vec<_>>(), ["T", "\n", "U"]);
    assert_eq!(p.runs[0].props.u, Some(true));
    // a title with a cell reference
    let d = xml(
        "<c:chartSpace><c:chart><c:title><c:tx><c:strRef><c:strCache><c:pt idx=\"0\"><c:v>Cell</c:v></c:pt></c:strCache></c:strRef></c:tx><c:overlay/></c:title></c:chart></c:chartSpace>",
    );
    let t = parsed(&d, &Host::default()).title.unwrap();
    assert_eq!(t.text.as_deref(), Some("Cell"));
    assert!(t.overlay);
    assert!(t.paras.is_none());
}

#[test]
fn picture_fills() {
    let area = |fill: &str| xml(&format!("<c:chartSpace><c:chart><c:plotArea/></c:chart><c:spPr>{fill}</c:spPr></c:chartSpace>"));
    let h = Host { blips: true, ..Host::default() };
    let tile = parsed(
        &area(
            "<a:blipFill><a:blip r:embed=\"rId3\"/><a:tile tx=\"12700\" ty=\"0\" sx=\"50000\" sy=\"100000\" flip=\"xy\" algn=\"ctr\"/></a:blipFill>",
        ),
        &h,
    );
    assert_eq!(
        tile.sp.fill,
        Some(Paint::Blip {
            key: BlipKey(7),
            tile: Some(Box::new(BlipTile { sx: 0.5, sy: 1.0, tx: 1.0, ty: 0.0, algn: "ctr".into(), flip: Some(TileFlip::Xy) })),
            fill_rect: None,
            alpha: None
        })
    );
    let st = parsed(
        &area(
            "<a:blipFill><a:blip r:embed=\"rId3\"><a:alphaModFix amt=\"50000\"/></a:blip><a:stretch><a:fillRect l=\"10000\"/></a:stretch></a:blipFill>",
        ),
        &h,
    );
    assert_eq!(
        st.sp.fill,
        Some(Paint::Blip {
            key: BlipKey(7),
            tile: None,
            fill_rect: Some(Box::new(FillRect { l: 0.1, t: 0.0, r: 0.0, b: 0.0 })),
            alpha: Some(0.5)
        })
    );
    // neither tile nor stretch: tiled at natural size
    let nt = parsed(&area("<a:blipFill><a:blip r:embed=\"rId3\"/></a:blipFill>"), &h);
    assert!(matches!(nt.sp.fill, Some(Paint::Blip { tile: Some(t), .. }) if t.sx == 1.0 && &*t.algn == "tl"));
    // an empty fillRect and an unknown relationship ignore the picture
    let st = parsed(&area("<a:blipFill><a:blip r:embed=\"rId3\"/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"), &h);
    assert_eq!(st.sp.fill, Some(Paint::Blip { key: BlipKey(7), tile: None, fill_rect: None, alpha: None }));
    let unknown = parsed(&area("<a:blipFill><a:blip r:embed=\"rId9\"/></a:blipFill>"), &h);
    assert_eq!(unknown.sp.fill, None);
    assert_eq!(parsed(&area("<a:blipFill><a:blip r:embed=\"rId3\"/></a:blipFill>"), &Host::default()).sp.fill, None);
}

#[test]
fn paints_lines_and_shadows() {
    let cs = parsed(
        &xml(
            "<c:chartSpace><c:chart/><c:spPr><a:gradFill><a:gsLst><a:gs pos=\"100000\"><a:srgbClr val=\"0000FF\"><a:alpha val=\"50000\"/></a:srgbClr></a:gs><a:gs pos=\"0\"><a:srgbClr val=\"FF0000\"/></a:gs><a:gs pos=\"x\"><a:srgbClr val=\"00FF00\"/></a:gs></a:gsLst><a:lin ang=\"5400000\"/><a:path path=\"circle\"/></a:gradFill>\
             <a:ln w=\"25400\" cap=\"rnd\"><a:solidFill><a:srgbClr val=\"00FF00\"><a:alpha val=\"20000\"/><a:alpha val=\"80000\"/></a:srgbClr></a:solidFill><a:prstDash val=\"dash\"/></a:ln>\
             <a:effectLst><a:outerShdw blurRad=\"38100\" dist=\"25400\" dir=\"5400000\"><a:srgbClr val=\"000000\"><a:alpha val=\"40000\"/></a:srgbClr></a:outerShdw></a:effectLst></c:spPr></c:chartSpace>",
        ),
        &Host::default(),
    );
    let Some(Paint::Grad { stops, angle, path }) = &cs.sp.fill else { panic!("gradient expected") };
    // the stops sorted by position: NaN breaks the order as the V8 sort leaves it
    assert_eq!(stops.len(), 3);
    assert_eq!(*angle, 90.0);
    assert_eq!(*path, Some(GradPath::Circle));
    assert!(stops.iter().any(|s| s.pos == 0.0 && s.color.as_str() == "#FF0000"));
    assert!(stops.iter().any(|s| s.pos == 1.0 && s.alpha == Some(0.5)));
    let ln = cs.sp.line.as_ref().unwrap();
    assert_eq!(ln.w, Some(2.0));
    assert_eq!(ln.dash.as_deref(), Some("dash"));
    assert_eq!(ln.cap.as_deref(), Some("rnd"));
    // the LAST alpha wins (measured)
    assert_eq!(ln.paint, Some(Paint::Solid { color: ChartColor::new("#00FF00"), alpha: Some(0.8) }));
    let sh = cs.sp.shadow.as_ref().unwrap();
    assert_eq!((sh.blur, sh.alpha), (3.0, 0.4));
    assert!((sh.dy - 2.0).abs() < 1e-12 && sh.dx.abs() < 1e-12);
}

#[test]
fn parse_paint_standalone() {
    let color = |s: ColorSource<'_>| match s {
        ColorSource::Element(e) => Some(ChartColor::new(&format!("#{}", e.attr("val").unwrap_or("")))),
        ColorSource::Scheme(_) => None,
    };
    let d = xml("<a:spPr><a:ln/><a:solidFill><a:srgbClr val=\"123456\"/></a:solidFill></a:spPr>");
    assert_eq!(parse_paint(d.root(), &color), Some(Paint::Solid { color: ChartColor::new("#123456"), alpha: None }));
    let d = xml("<a:noFill/>");
    assert_eq!(parse_paint(d.root(), &color), None);
    assert_eq!(parse_paint_node(d.root(), &color), Some(Paint::None));
    let d = xml("<a:pattFill prst=\"ltUpDiag\"><a:fgClr><a:srgbClr val=\"AA0000\"/></a:fgClr></a:pattFill>");
    assert_eq!(
        parse_paint_node(d.root(), &color),
        Some(Paint::Patt { fg: ChartColor::new("#AA0000"), bg: ChartColor::new("#FFFFFF"), prst: "ltUpDiag".into() })
    );
    // a solid fill whose colour the resolver does not know decides: no paint, and the loop stops
    let d = xml("<a:spPr><a:solidFill><a:schemeClr/></a:solidFill></a:spPr>");
    let none = |_: ColorSource<'_>| None;
    assert_eq!(parse_paint(d.root(), &none), None);
}

fn user_shapes_doc() -> Document {
    xml(concat!(
        "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0.9</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to>",
        "<cdr:sp><cdr:nvSpPr><cdr:cNvPr id=\"2\" name=\"T\"/></cdr:nvSpPr><cdr:spPr><a:prstGeom prst=\"rect\"/></cdr:spPr><cdr:txBody><a:bodyPr anchor=\"ctr\" lIns=\"0\"/><a:p><a:pPr algn=\"r\"/><a:r><a:rPr sz=\"900\" i=\"1\"/><a:t>Fonte: X</a:t></a:r></a:p></cdr:txBody></cdr:sp></cdr:relSizeAnchor>",
        "<cdr:relSizeAnchor><cdr:from><cdr:x>0.5</cdr:x><cdr:y>0</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>0.5</cdr:y></cdr:to><cdr:grpSp><cdr:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"100\" cy=\"100\"/></a:xfrm></cdr:grpSpPr>",
        "<cdr:sp><cdr:spPr><a:xfrm><a:off x=\"50\" y=\"0\"/><a:ext cx=\"50\" cy=\"25\"/></a:xfrm><a:prstGeom prst=\"ellipse\"/><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></cdr:spPr></cdr:sp></cdr:grpSp></cdr:relSizeAnchor>",
        "<cdr:absSizeAnchor><cdr:from><cdr:x>0.1</cdr:x><cdr:y>0.1</cdr:y></cdr:from><cdr:ext cx=\"127000\" cy=\"254000\"/><cdr:cxnSp><cdr:spPr><a:prstGeom prst=\"straightConnector1\"/></cdr:spPr></cdr:cxnSp></cdr:absSizeAnchor></c:userShapes>"
    ))
}

#[test]
fn chart_drawing_and_theme_widths() {
    let base = xml("<c:chartSpace xmlns:c=\"c\" xmlns:a=\"a\"><c:chart><c:plotArea/></c:chart></c:chartSpace>");
    let h = Host { shapes: Some(user_shapes_doc()), ..Host::default() };
    let u = parsed(&base, &h).user_shapes.unwrap();
    assert_eq!(u.len(), 3);
    assert_eq!((u[0].rect.x, u[0].rect.y, u[0].rect.w, &*u[0].geom), (0.0, 0.9, 1.0, "rect"));
    assert!((u[0].rect.h - 0.1).abs() < 1e-9);
    let t = u[0].text.as_ref().unwrap();
    assert_eq!((t.ins.l, t.ins.t), (0.0, 3.6));
    assert_eq!(format!("{:?}", t.anchor), "Ctr");
    assert_eq!(t.paras[0].align.as_deref(), Some("r"));
    assert_eq!(&*t.paras[0].runs[0].text, "Fonte: X");
    assert_eq!((t.paras[0].runs[0].props.sz, t.paras[0].runs[0].props.i), (Some(9.0), Some(true)));
    assert_eq!((u[1].rect.x, u[1].rect.y, u[1].rect.w, u[1].rect.h, &*u[1].geom), (0.5, 0.0, 0.5, 0.5, "ellipse"));
    let s = u[1].sub.unwrap();
    assert_eq!((s.x, s.y, s.w, s.h), (0.5, 0.0, 0.5, 0.25));
    assert_eq!(u[1].sp.fill, Some(Paint::Solid { color: ChartColor::new("#FF0000"), alpha: None }));
    assert!(u[2].abs);
    assert_eq!((u[2].rect.w, u[2].rect.h, &*u[2].geom), (10.0, 20.0, "straightConnector1"));
}

#[test]
fn theme_line_widths_reach_the_model() {
    let base = xml("<c:chartSpace><c:chart><c:plotArea/></c:chart></c:chartSpace>");
    let h = Host { widths: vec![0.5, 1.0, 1.5], ..Host::default() };
    assert_eq!(parsed(&base, &h).theme_ln.as_deref(), Some(&[0.5, 1.0, 1.5][..]));
    assert!(parsed(&base, &Host::default()).theme_ln.is_none());
    let h = Host { fill_style: true, ..Host::default() };
    assert!(parsed(&base, &h).has_theme_fill);
}

#[test]
fn style_references_of_a_drawing_shape() {
    let us = xml(concat!(
        "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to>",
        "<cdr:sp><cdr:spPr><a:ln w=\"6350\"/></cdr:spPr><cdr:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"1\"><a:schemeClr val=\"accent2\"/></a:fillRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"accent3\"/></a:fontRef></cdr:style>",
        "<cdr:txBody><a:bodyPr vert=\"vert270\" rot=\"60000\"/><a:p><a:r><a:t>hi</a:t></a:r></a:p></cdr:txBody></cdr:sp>",
        "<cdr:pic><cdr:blipFill><a:blip r:embed=\"rId3\"/></cdr:blipFill><cdr:spPr/></cdr:pic>",
        "<cdr:sp><cdr:nvSpPr><cdr:cNvPr hidden=\"1\"/></cdr:nvSpPr></cdr:sp></cdr:relSizeAnchor></c:userShapes>"
    ));
    let base = xml("<c:chartSpace><c:chart/></c:chartSpace>");
    // without a theme fill style: a solid colour
    let h = Host { shapes: Some(us), blips: true, ..Host::default() };
    let u = parsed(&base, &h).user_shapes.unwrap();
    assert_eq!(u.len(), 2);
    assert_eq!(u[0].sp.fill, Some(Paint::Solid { color: ChartColor::new("#E97132"), alpha: None }));
    let ln = u[0].sp.line.as_ref().unwrap();
    // the shape's own width wins; the paint comes from lnRef
    assert_eq!(ln.w, Some(0.5));
    assert_eq!(ln.paint, Some(Paint::Solid { color: ChartColor::new("#156082"), alpha: None }));
    let t = u[0].text.as_ref().unwrap();
    assert_eq!(t.def.color.as_ref().map(hex), Some("#196B24"));
    assert_eq!(t.rot, Some(-89.0));
    assert_eq!(u[1].blip, Some(BlipKey(7)));
    // with the theme's fill style
    let us2 = xml(
        "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to><cdr:sp><cdr:spPr/><cdr:style><a:fillRef idx=\"3\"><a:schemeClr val=\"accent2\"/></a:fillRef></cdr:style></cdr:sp></cdr:relSizeAnchor></c:userShapes>",
    );
    let h = Host { shapes: Some(us2), fill_style: true, ..Host::default() };
    let u = parsed(&base, &h).user_shapes.unwrap();
    assert_eq!(u[0].sp.fill, Some(Paint::Solid { color: ChartColor::new("#E971323"), alpha: None }));
}

#[test]
fn point_labels_with_fields_offsets_and_boxes() {
    let dl = concat!(
        "<c:dLbls><c:dLbl><c:idx val=\"0\"/><c:layout><c:manualLayout><c:x val=\"0.1\"/><c:y val=\"-0.05\"/></c:manualLayout></c:layout>",
        "<c:tx><c:rich><a:bodyPr/><a:p><a:fld id=\"{1}\" type=\"CATEGORYNAME\"><a:rPr sz=\"2000\"/><a:t>[CATEGORY NAME]</a:t></a:fld><a:r><a:t>: </a:t></a:r><a:fld id=\"{2}\" type=\"VALUE\"><a:t>[VALUE]</a:t></a:fld></a:p></c:rich></c:tx>",
        "<c:showCatName val=\"1\"/><c:extLst><c:ext uri=\"x\"><c15:layout><c:manualLayout><c:w val=\"0.2\"/><c:h val=\"0.1\"/></c:manualLayout></c15:layout></c:ext></c:extLst></c:dLbl>",
        "<c:dLbl><c:idx val=\"0\"/><c:showVal val=\"0\"/></c:dLbl><c:dLbl><c:showVal val=\"1\"/></c:dLbl>",
        "<c:showVal val=\"1\"/><c:separator>; </c:separator><c:numFmt formatCode=\"0.0\" sourceLinked=\"0\"/><c:extLst><c:ext uri=\"y\"><c15:showLeaderLines val=\"1\"/><c15:leaderLines><c:spPr><a:ln w=\"38100\"><a:solidFill><a:srgbClr val=\"7030A0\"/></a:solidFill></a:ln></c:spPr></c15:leaderLines></c:ext></c:extLst></c:dLbls>"
    );
    let d = xml(&format!(
        "<c:chartSpace><c:chart><c:plotArea><c:pieChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{dl}</c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"
    ));
    let cs = parsed(&d, &Host::default());
    let dl = cs.groups[0].series[0].d_lbls.as_ref().unwrap();
    // the second dLbl idx 0 REPLACES the first (Map.set) keeping its position; the one without idx is skipped
    let pts = dl.points.as_ref().unwrap();
    assert_eq!(pts.len(), 1);
    let pd = pts.get(0.0).unwrap();
    assert_eq!(pd.common.show_val, Some(false));
    assert_eq!(dl.common.sep.as_deref(), Some("; "));
    let nf = dl.common.num_fmt.as_ref().unwrap();
    assert_eq!((nf.code.units().len(), nf.linked), (3, false));
    let ll = dl.common.leader_lines.as_ref().unwrap().line.as_ref().unwrap();
    assert_eq!(ll.w, Some(3.0));
    assert_eq!(ll.paint, Some(Paint::Solid { color: ChartColor::new("#7030A0"), alpha: None }));
    // the first dLbl alone (no replacement)
    let d = xml(&format!(
        "<c:chartSpace><c:chart><c:plotArea><c:pieChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>{}</c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>",
        dl_first(dl_src())
    ));
    let cs = parsed(&d, &Host::default());
    let pd = cs.groups[0].series[0].d_lbls.as_ref().unwrap().points.as_ref().unwrap().get(0.0).unwrap();
    let parts = pd.parts.as_ref().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!((&*parts[0].text, parts[0].fld.as_deref()), ("[CATEGORY NAME]", Some("CATEGORYNAME")));
    assert_eq!((&*parts[1].text, parts[1].fld.as_deref()), (": ", None));
    assert_eq!(&*pd.text.clone().unwrap(), "[CATEGORY NAME]: [VALUE]");
    assert_eq!(pd.run_props.as_ref().unwrap().sz, Some(20.0));
    assert_eq!(pd.layout.as_ref().unwrap().x, Some(0.1));
    assert_eq!(pd.layout.as_ref().unwrap().y, Some(-0.05));
    assert_eq!(pd.size, Some((0.2, 0.1)));
}

fn dl_src() -> &'static str {
    concat!(
        "<c:dLbls><c:dLbl><c:idx val=\"0\"/><c:layout><c:manualLayout><c:x val=\"0.1\"/><c:y val=\"-0.05\"/></c:manualLayout></c:layout>",
        "<c:tx><c:rich><a:bodyPr/><a:p><a:fld id=\"{1}\" type=\"CATEGORYNAME\"><a:rPr sz=\"2000\"/><a:t>[CATEGORY NAME]</a:t></a:fld><a:r><a:t>: </a:t></a:r><a:fld id=\"{2}\" type=\"VALUE\"><a:t>[VALUE]</a:t></a:fld></a:p></c:rich></c:tx>",
        "<c:showCatName val=\"1\"/><c:extLst><c:ext uri=\"x\"><c15:layout><c:manualLayout><c:w val=\"0.2\"/><c:h val=\"0.1\"/></c:manualLayout></c15:layout></c:ext></c:extLst></c:dLbl></c:dLbls>"
    )
}

fn dl_first(s: &str) -> String {
    s.to_owned()
}

#[test]
fn axes_trendlines_groups_and_legends() {
    let ax = concat!(
        "<c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"maxMin\"/></c:scaling><c:delete val=\"1\"/><c:axPos val=\"t\"/><c:majorTickMark val=\"weird\"/><c:tickLblPos val=\"low\"/><c:crossAx val=\"2\"/><c:crosses val=\"max\"/><c:lblOffset val=\"50\"/><c:tickLblSkip val=\"0\"/><c:tickMarkSkip val=\"3\"/><c:noMultiLvlLbl/></c:catAx>",
        "<c:valAx><c:axId val=\"2\"/><c:scaling><c:logBase val=\"1\"/><c:min val=\"-5\"/><c:max val=\"x\"/></c:scaling><c:axPos val=\"r\"/><c:majorGridlines><c:spPr><a:ln w=\"9525\"/></c:spPr></c:majorGridlines><c:minorGridlines/><c:numFmt formatCode=\"0%\" sourceLinked=\"1\"/><c:crossAx val=\"1\"/><c:crossBetween val=\"midCat\"/><c:majorUnit val=\"0\"/><c:minorUnit val=\"2\"/>",
        "<c:dispUnits><c:builtInUnit val=\"millions\"/><c:custUnit val=\"5\"/></c:dispUnits></c:valAx>",
        "<c:dateAx><c:axId val=\"3\"/><c:baseTimeUnit val=\"days\"/><c:majorTimeUnit val=\"months\"/><c:dispUnits><c:builtInUnit val=\"nope\"/><c:custUnit val=\"5\"/></c:dispUnits></c:dateAx><c:serAx/>"
    );
    let d = xml(&format!(
        "<c:chartSpace><c:chart><c:plotArea><c:barChart><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>{ax}</c:plotArea></c:chart></c:chartSpace>"
    ));
    let cs = parsed(&d, &Host::default());
    assert_eq!(cs.axes.len(), 4);
    let (c, v, dt) = (&cs.axes[0], &cs.axes[1], &cs.axes[2]);
    assert!(c.reversed && c.deleted && c.no_multi_lvl_lbl);
    assert_eq!(format!("{:?} {:?} {:?} {:?}", c.pos, c.major_tick, c.tick_lbl_pos, c.crosses), "T Other(\"weird\") Low Max");
    assert_eq!((c.lbl_offset, c.tick_lbl_skip, c.tick_mark_skip), (50.0, None, Some(3.0)));
    assert_eq!((v.log_base, v.min, v.max, v.major_unit, v.minor_unit), (None, Some(-5.0), None, None, Some(2.0)));
    assert!(v.major_grid.as_ref().unwrap().line.is_some() && v.minor_grid.as_ref().unwrap().line.is_none());
    assert!(v.num_fmt.as_ref().unwrap().linked);
    assert_eq!(v.cross_between, Some(uo_charts::model::CrossBetween::MidCat));
    assert_eq!(v.disp_units, Some(1e6));
    assert_eq!((dt.base_time_unit.as_deref(), dt.major_time_unit.as_deref()), (Some("days"), Some("months")));
    // an unknown built-in unit does NOT fall back to custUnit (`b ? UNITS[b] : c`)
    assert_eq!(dt.disp_units, None);
    assert_eq!(cs.axes[3].kind, AxisKind::Ser);
    // log base in range, grouping and group kinds
    let d = xml(concat!(
        "<c:chartSpace><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val=\"bar\"/><c:ser><c:idx val=\"2\"/><c:order val=\"2\"/></c:ser><c:ser><c:idx val=\"1\"/><c:order val=\"1\"/></c:ser><c:ser><c:idx val=\"0\"/><c:order val=\"1\"/></c:ser>",
        "<c:gapWidth val=\"10\"/><c:splitPos val=\"4\"/><c:custSplit><c:secondPiePt val=\"2\"/><c:secondPiePt val=\"-1\"/><c:secondPiePt val=\"x\"/><c:secondPiePt val=\"7\"/></c:custSplit></c:ofPieChart>",
        "<c:stockChart><c:upDownBars><c:upBars><c:spPr/></c:upBars></c:upDownBars><c:hiLowLines/><c:dropLines><c:spPr><a:ln/></c:spPr></c:dropLines></c:stockChart>",
        "<c:bar3DChart><c:grouping val=\"percentStacked\"/><c:gapDepth val=\"-1\"/></c:bar3DChart><c:surface3DChart/><c:radarChart><c:radarStyle val=\"filled\"/></c:radarChart>",
        "<c:lineChart><c:grouping val=\"weird\"/><c:marker val=\"0\"/></c:lineChart></c:plotArea></c:chart></c:chartSpace>"
    ));
    let cs = parsed(&d, &Host::default());
    let kinds: Vec<_> = cs.groups.iter().map(|g| (g.kind, g.three_d, g.index)).collect();
    assert_eq!(
        kinds,
        [
            (GroupKind::OfPie, false, 0),
            (GroupKind::Stock, false, 1),
            (GroupKind::Bar, true, 2),
            (GroupKind::Surface, true, 3),
            (GroupKind::Radar, false, 4),
            (GroupKind::Line, false, 5)
        ]
    );
    let g = &cs.groups[0];
    // the JS sort is stable: orders 1,1,2 keep idx 1 before idx 0
    assert_eq!(g.series.iter().map(|s| s.idx).collect::<Vec<_>>(), [1.0, 0.0, 2.0]);
    let op = g.of_pie.as_ref().unwrap();
    assert_eq!((&*op.split_type, op.split_pos, op.second_pie_size, op.cust_split.clone()), ("auto", Some(4.0), 75.0, Some(vec![2.0, 7.0])));
    assert_eq!(g.of_pie_type.as_deref(), Some("bar"));
    assert!(g.vary_colors);
    assert_eq!(
        (cs.groups[1].up_down_bars.as_ref().unwrap().gap_width, cs.groups[1].hi_low_lines.is_some(), cs.groups[1].drop_lines.is_some()),
        (150.0, true, true)
    );
    let b = &cs.groups[2];
    assert_eq!((format!("{:?}", b.grouping), b.overlap, b.gap_depth, b.gap_width), ("PercentStacked".to_owned(), 100.0, None, 150.0));
    assert_eq!(cs.groups[4].radar_style.as_deref(), Some("filled"));
    assert!(cs.groups[4].show_marker);
    assert_eq!(format!("{:?}", cs.groups[5].grouping), "Other(\"weird\")");
    assert!(!cs.groups[5].show_marker);
}

#[test]
fn trendlines_legends_blanks_and_floors() {
    let tl = "<c:trendline><c:name>T</c:name><c:spPr><a:ln w=\"12700\"/></c:spPr><c:trendlineType val=\"poly\"/><c:order val=\"3\"/><c:period val=\"x\"/><c:forward val=\"1.5\"/><c:dispRSqr val=\"1\"/><c:trendlineLbl><c:layout><c:manualLayout><c:x val=\"0.2\"/></c:manualLayout></c:layout><c:numFmt formatCode=\"0.0\" sourceLinked=\"0\"/></c:trendlineLbl></c:trendline>";
    let d = space(
        &bar(&ser(0, &[1.0, 2.0], tl), ""),
        "<c:legend><c:legendPos val=\"zz\"/><c:legendEntry><c:idx val=\"1\"/><c:delete val=\"1\"/></c:legendEntry><c:legendEntry><c:idx val=\"2\"/></c:legendEntry><c:legendEntry><c:idx val=\"3\"/><c:delete val=\"0\"/></c:legendEntry><c:legendEntry><c:idx val=\"1\"/><c:delete/></c:legendEntry><c:overlay val=\"1\"/></c:legend><c:dispBlanksAs val=\"span\"/>",
    );
    let cs = parsed(&d, &Host::default());
    let t = &cs.groups[0].series[0].trendlines[0];
    assert_eq!(
        (&*t.kind, t.order, t.period, t.forward, t.disp_eq, t.disp_r_sqr, t.name.as_deref()),
        ("poly", Some(3.0), None, Some(1.5), false, true, Some("T"))
    );
    let lb = t.label.as_ref().unwrap();
    assert_eq!(lb.layout.as_ref().unwrap().x, Some(0.2));
    assert!(lb.num_fmt.is_some());
    let lg = cs.legend.as_ref().unwrap();
    assert_eq!(lg.pos, LegendPos::Other("zz".into()));
    assert!(lg.overlay);
    // `<c:delete/>` without val is true; the Set keeps one entry per index
    assert_eq!(lg.deleted.keys.len(), 1);
    assert!(lg.deleted.has(1.0) && !lg.deleted.has(2.0) && !lg.deleted.has(3.0));
    assert_eq!(cs.disp_blanks_as, uo_charts::model::DispBlanksAs::Span);
    let d = xml(
        "<c:chartSpace><c:chart><c:floor><c:spPr><a:noFill/></c:spPr></c:floor><c:backWall/><c:plotArea><c:layout><c:manualLayout><c:layoutTarget val=\"inner\"/><c:xMode val=\"edge\"/><c:yMode val=\"zzz\"/><c:x val=\"0.1\"/></c:manualLayout></c:layout><c:spPr><a:solidFill><a:schemeClr val=\"bg1\"/></a:solidFill></c:spPr></c:plotArea></c:chart></c:chartSpace>",
    );
    let cs = parsed(&d, &Host::default());
    assert_eq!(cs.floor_sp.as_ref().unwrap().fill, Some(Paint::None));
    assert_eq!(cs.back_wall_sp, Some(Default::default()));
    assert!(cs.side_wall_sp.is_none());
    let l = cs.plot_layout.unwrap();
    assert_eq!(
        (l.target, l.x_mode, l.y_mode, l.x),
        (Some(uo_charts::model::LayoutTarget::Inner), Some(uo_charts::model::LayoutMode::Edge), None, Some(0.1))
    );
    assert_eq!(cs.plot_sp.fill, Some(Paint::Solid { color: ChartColor::new("#FFFFFF"), alpha: None }));
}

#[test]
fn text_props_rules() {
    let tx = xml(
        "<c:chartSpace><c:txPr><a:bodyPr rot=\"-60000\" vert=\"horz\"/><a:p><a:pPr><a:defRPr sz=\"12a\" b=\"false\" i=\"1\" u=\"none\"><a:latin typeface=\"+mj-lt\"/></a:defRPr></a:pPr></a:p></c:txPr></c:chartSpace>",
    );
    let p = parsed(&tx, &Host::default()).tx_pr;
    // "12a" is not digits only: no size; +mj-lt with no major font: no face; vert=horz dropped
    assert_eq!(
        p,
        TextProps {
            sz: None,
            b: Some(false),
            i: Some(true),
            u: Some(false),
            color: None,
            alpha: None,
            font: None,
            rot: Some(-1.0),
            vert: None
        }
    );
}

#[test]
fn summary_of_the_model() {
    let d = space(
        &bar(&ser(0, &[1.0, 2.0], "<c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:ln><a:noFill/></a:ln></c:spPr>"), ""),
        "<c:title><c:tx><c:rich><a:p><a:r><a:t>Hello</a:t></a:r><a:r><a:t> w</a:t></a:r></a:p><a:p><a:r><a:t>2</a:t></a:r></a:p></c:rich></c:tx></c:title><c:legend><c:legendPos val=\"b\"/></c:legend>",
    );
    let cs = parsed(&d, &Host::default());
    let s = summarize(&cs);
    assert_eq!(s.ty, ChartType::BarCol);
    assert_eq!(s.title.as_deref(), Some("Hello w\n2"));
    assert_eq!(s.series.len(), 1);
    assert_eq!(s.series[0].values, [1.0, 2.0]);
    assert_eq!(s.series[0].fill, Some(Some(ChartColor::new("#FF0000"))));
    assert_eq!(s.series[0].line, Some(None));
    assert_eq!(s.series[0].color.as_ref().map(hex), Some("#FF0000"));
    assert_eq!(s.categories, ["A", "B", "C", "D"]);
    assert_eq!(s.legend_pos, Some(LegendPos::B));
    assert!(s.val_ax.as_ref().unwrap().grid && !s.cat_ax.as_ref().unwrap().grid);
    // numeric categories print as JS numbers
    let sc = "<c:ser><c:xVal><c:numLit><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>1.5</c:v></c:pt><c:pt idx=\"2\"><c:v>1e21</c:v></c:pt></c:numLit></c:xVal></c:ser>";
    let d = xml(&format!("<c:chartSpace><c:chart><c:plotArea><c:scatterChart>{sc}</c:scatterChart></c:plotArea></c:chart></c:chartSpace>"));
    let s = summarize(&parsed(&d, &Host::default()));
    assert_eq!(s.ty, ChartType::Scatter);
    assert_eq!(s.categories, ["1.5", "", "1e+21"]);
    assert!(summarize(&parsed(&xml("<c:chartSpace/>"), &Host::default())).ty == ChartType::Unknown);
}

#[test]
fn cx_chart_space_is_a_hook_for_c2() {
    let d = xml("<cx:chartSpace><cx:chart/></cx:chartSpace>");
    let cs = parsed(&d, &Host::default());
    assert!(cs.groups.is_empty() && cs.axes.is_empty());
    assert_eq!(cs.accents[1].as_str(), "#E97132");
}
