//! Fuzz target `chart_parse` of `uo-charts` (record `core/docs/modules/uo-charts.md`, "C-1 — model, parser, colours"):
//! the chart part (`c:chartSpace` / `cx:chartSpace`, with the canonical prefixes or without), the chart drawing and
//! the three host profiles through `parse_chart_space`, every element as a paint parent, a fill node and a colour.
//!
//! Input: byte 0 = flags (bits 0-1 file format 0 none / 1 modern / 2 Office 2007, bit 2 canonical prefixes
//! (the Sheet), bit 3 the host has no fill style), the rest split on a 0 byte into the chart XML and the chart
//! drawing's XML. Beyond "no panic": the model keeps its caps (100,000 points per series), the series of a group
//! are sorted by `order`, every group's axes exist, and the same XML parsed twice gives the same model.

use uo_charts::canon::Canon;
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{ChartColor, GroupKind, Paint};
use uo_charts::{CHART_PREFIXES, Scheme, dml_color, dml_color_source, parse_chart_space, parse_paint, parse_paint_node, summarize};
use uo_opc::xml::{CanonicalPrefixes, Document, Element, ParseOptions, parse};

struct FuzzHost {
    format: Option<FileFormat>,
    fill: bool,
    shapes: Option<Document>,
}

impl ChartHost for FuzzHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        let v = match src {
            ColorSource::Scheme(v) => v,
            ColorSource::Element(e) => e.attr("val").unwrap_or(""),
        };
        // a resolver that sometimes refuses (the TS host returns undefined for colours it cannot read)
        if v.len() % 5 == 4 { None } else { Some(ChartColor::new(&format!("#{v}"))) }
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: Some("Major"), minor: if self.fill { Some("Minor") } else { None } }
    }
    fn has_fill_style(&self) -> bool {
        self.fill
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
        if self.fill { &[] } else { &[0.5, 1.0, 1.5] }
    }
}

/// Seeds: (flags, chart XML, drawing XML).
pub fn chart_parse_seeds() -> Vec<Vec<u8>> {
    let pts = |n: usize, f: &dyn Fn(usize) -> String| -> String {
        (0..n).map(|i| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", f(i))).collect()
    };
    let ser = |i: usize, extra: &str| {
        format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:v>S{i}</c:v></c:tx><c:spPr><a:solidFill><a:schemeClr val=\"accent{}\"><a:lumMod val=\"75000\"/></a:schemeClr></a:solidFill><a:ln w=\"19050\"><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></a:ln></c:spPr>{extra}<c:cat><c:strLit><c:ptCount val=\"4\"/>{}</c:strLit></c:cat><c:val><c:numLit><c:formatCode>0.0</c:formatCode><c:ptCount val=\"4\"/>{}</c:numLit></c:val></c:ser>",
            1 + i % 6,
            pts(4, &|k| format!("C{k}")),
            pts(4, &|k| (k * 3 + i).to_string())
        )
    };
    let ax = "<c:catAx><c:axId val=\"1\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:catAx><c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/><c:majorGridlines/><c:crossAx val=\"1\"/><c:dispUnits><c:builtInUnit val=\"millions\"/></c:dispUnits></c:valAx>";
    let dl = "<c:dLbls><c:dLbl><c:idx val=\"1\"/><c:tx><c:rich><a:bodyPr/><a:p><a:fld id=\"{1}\" type=\"VALUE\"><a:rPr sz=\"1000\"/><a:t>[VALUE]</a:t></a:fld></a:p></c:rich></c:tx><c:showVal val=\"1\"/><c:extLst><c:ext><c15:layout><c:manualLayout><c:w val=\"0.2\"/><c:h val=\"0.1\"/></c:manualLayout></c15:layout></c:ext></c:extLst></c:dLbl><c:showVal val=\"1\"/><c:extLst><c:ext><c15:showLeaderLines val=\"1\"/></c:ext></c:extLst></c:dLbls>";
    let tl = "<c:trendline><c:trendlineType val=\"poly\"/><c:order val=\"3\"/><c:dispEq val=\"1\"/><c:trendlineLbl><c:numFmt formatCode=\"0.0\" sourceLinked=\"0\"/></c:trendlineLbl></c:trendline>";
    let bar = format!(
        "<c:chartSpace><mc:AlternateContent><mc:Choice><c14:style val=\"102\"/></mc:Choice></mc:AlternateContent><c:chart><c:title><c:tx><c:rich><a:bodyPr rot=\"-5400000\"/><a:p><a:pPr><a:defRPr sz=\"1400\" b=\"1\"/></a:pPr><a:r><a:t>T</a:t></a:r></a:p></c:rich></c:tx></c:title><c:view3D><c:rotX val=\"15\"/></c:view3D><c:plotArea><c:barChart><c:barDir val=\"col\"/>{}{}<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>{ax}<c:spPr><a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs><a:gs pos=\"100000\"><a:srgbClr val=\"EEEEEE\"/></a:gs></a:gsLst><a:lin ang=\"5400000\"/></a:gradFill></c:spPr></c:plotArea><c:legend><c:legendPos val=\"r\"/><c:legendEntry><c:idx val=\"0\"/><c:delete val=\"1\"/></c:legendEntry></c:legend><c:dispBlanksAs val=\"gap\"/></c:chart><c:spPr><a:blipFill><a:blip r:embed=\"rId3\"/><a:tile tx=\"12700\" flip=\"xy\"/></a:blipFill></c:spPr></c:chartSpace>",
        ser(0, &format!("{dl}{tl}<c:dPt><c:idx val=\"0\"/><c:spPr><a:noFill/></c:spPr></c:dPt>")),
        ser(1, "<c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/></c:marker>")
    );
    let pie = format!(
        "<c:chartSpace><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val=\"pie\"/><c:varyColors val=\"1\"/>{}<c:splitType val=\"cust\"/><c:custSplit><c:secondPiePt val=\"2\"/></c:custSplit></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>",
        ser(0, "<c:explosion val=\"5\"/>")
    );
    let scatter = "<c:chartSpace><c:chart><c:plotArea><c:scatterChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:xVal><c:numLit><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt></c:numLit></c:xVal><c:yVal><c:numLit><c:ptCount val=\"3\"/><c:pt idx=\"2\"><c:v>5</c:v></c:pt></c:numLit></c:yVal></c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>";
    let multi = "<c:chartSpace><c:chart><c:plotArea><c:barChart><c:ser><c:cat><c:multiLvlStrCache><c:ptCount val=\"3\"/><c:lvl><c:pt idx=\"0\"><c:v>a</c:v></c:pt></c:lvl><c:lvl><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>G</c:v></c:pt></c:lvl></c:multiLvlStrCache></c:cat></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>";
    let drawing = "<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0.9</cdr:y></cdr:from><cdr:to><cdr:x>1</cdr:x><cdr:y>1</cdr:y></cdr:to><cdr:sp><cdr:spPr><a:prstGeom prst=\"rect\"/></cdr:spPr><cdr:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"1\"><a:schemeClr val=\"accent2\"/></a:fillRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></a:fontRef></cdr:style><cdr:txBody><a:bodyPr anchor=\"ctr\" vert=\"vert270\"/><a:p><a:r><a:t>Fonte</a:t></a:r></a:p></cdr:txBody></cdr:sp><cdr:grpSp><cdr:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"100\" cy=\"100\"/></a:xfrm></cdr:grpSpPr><cdr:sp><cdr:spPr><a:xfrm><a:off x=\"50\" y=\"0\"/><a:ext cx=\"50\" cy=\"25\"/></a:xfrm></cdr:spPr></cdr:sp></cdr:grpSp></cdr:relSizeAnchor></c:userShapes>";
    let cx = "<cx:chartSpace><cx:chart><cx:plotArea/></cx:chart></cx:chartSpace>";
    let mut out = Vec::new();
    for (i, xml) in [bar.as_str(), pie.as_str(), scatter, multi, cx].into_iter().enumerate() {
        for flags in [0u8, 1, 2 | 4, 1 | 8] {
            let mut b = vec![flags];
            b.extend_from_slice(xml.as_bytes());
            b.push(0);
            if i == 0 {
                b.extend_from_slice(drawing.as_bytes());
            }
            out.push(b);
        }
    }
    out
}

/// The target.
pub fn chart_parse(data: &[u8]) {
    let Some((&flags, rest)) = data.split_first() else { return };
    let mut parts = rest.splitn(2, |&b| b == 0);
    let chart = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
    let drawing = parts.next().map(|d| String::from_utf8_lossy(d).into_owned());
    let canonical = flags & 4 != 0;
    let opts = if canonical {
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
    let shapes = drawing.and_then(|d| parse(d, &opts).ok());
    let host = FuzzHost { format, fill: flags & 8 == 0, shapes };
    let cs = parse_chart_space(doc.root(), &host);
    let again = parse_chart_space(doc.root(), &host);
    // the canonical JSON compares NaN fields (a derived `==` would not)
    let canon = Canon { blip: &|k| k.0.to_string() };
    assert_eq!(canon.chart_space(&cs).to_text(), canon.chart_space(&again).to_text(), "the parse is deterministic");
    for g in &cs.groups {
        for s in &g.series {
            assert!(s.values.len() <= 100_000);
            assert!(s.cats.as_ref().is_none_or(|c| c.len() <= 100_000));
            assert!(s.pt_fmts.as_ref().is_none_or(|c| c.len() <= 100_000));
        }
        assert!(g.series.windows(2).all(|w| w[0].order <= w[1].order), "series sorted by order");
        // `fixAxes`: a group that is not a pie has its two axes (found, or created by the default pair)
        let pie = matches!(g.kind, GroupKind::Pie | GroupKind::Doughnut | GroupKind::OfPie);
        if !pie && doc.root().name() != "cx:chartSpace" {
            assert!(g.ax_ids.iter().filter(|id| cs.axes.iter().any(|a| a.id == **id)).count() >= 2, "a group's axes exist");
        }
    }
    let _ = summarize(&cs);
    let sc = Scheme::from_pairs(&[("dk1", "#000000"), ("lt1", "#FFFFFF"), ("accent1", "#156082")]);
    let col = |s: ColorSource<'_>| dml_color_source(s, &sc);
    for e in doc.root().descendants().take(300) {
        let _ = parse_paint(e, &col);
        let _ = parse_paint_node(e, &col);
        let _ = dml_color(e, &sc);
    }
}
