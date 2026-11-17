//! A plain host and a built-in corpus of chart XML for the memory and no-panic tests of C-1.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{ChartColor, Paint};
use uo_opc::xml::{Document, Element, ParseOptions, parse};

use super::{gunzip, json};

/// The host of the TS tests (`charts.test.ts`): `a:srgbClr` → `#val`, the Office theme for the rest.
pub struct PlainHost {
    pub format: Option<FileFormat>,
    pub shapes: Option<Document>,
}

impl ChartHost for PlainHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        let theme = |v: &str| match v {
            "accent1" => "#156082",
            "accent2" => "#E97132",
            "accent3" => "#196B24",
            "accent4" => "#0F9ED5",
            "accent5" => "#A02B93",
            "accent6" => "#4EA72E",
            "bg1" => "#FFFFFF",
            _ => "#000000",
        };
        Some(ChartColor::new(&match src {
            ColorSource::Scheme(v) => theme(v).to_owned(),
            ColorSource::Element(e) if e.name() == "a:srgbClr" => format!("#{}", e.attr("val").unwrap_or("")),
            ColorSource::Element(e) => theme(e.attr("val").unwrap_or("")).to_owned(),
        }))
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: Some("Aptos Display"), minor: Some("Aptos") }
    }
    fn has_fill_style(&self) -> bool {
        true
    }
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        Some(Paint::Solid { color: ChartColor::new(&format!("{ph_clr}{idx}")), alpha: None })
    }
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        blip.attr("r:embed").map(|_| BlipKey(1))
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

fn pts(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|i| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", f(i))).collect()
}

/// One series with `n` points (caches of text categories and numbers), a colour with modifiers and a line.
pub fn series_xml(i: usize, n: usize, extra: &str) -> String {
    format!(
        "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:strRef><c:f>S!$A$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>Series {i}</c:v></c:pt></c:strCache></c:strRef></c:tx>\
         <c:spPr><a:solidFill><a:schemeClr val=\"accent{}\"><a:lumMod val=\"75000\"/></a:schemeClr></a:solidFill><a:ln w=\"19050\"><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></a:ln></c:spPr>{extra}\
         <c:cat><c:strRef><c:f>S!$A$2</c:f><c:strCache><c:ptCount val=\"{n}\"/>{}</c:strCache></c:strRef></c:cat>\
         <c:val><c:numRef><c:f>S!$B$2</c:f><c:numCache><c:formatCode>0.00</c:formatCode><c:ptCount val=\"{n}\"/>{}</c:numCache></c:numRef></c:val></c:ser>",
        1 + i % 6,
        pts(n, |k| format!("Category {k}")),
        pts(n, |k| format!("{}", (k * 7 + i * 3) % 50))
    )
}

/// A bar chart of `s` series × `n` points with every kind of element the parser reads.
pub fn rich_chart(s: usize, n: usize) -> String {
    let dl = "<c:dLbls><c:dLbl><c:idx val=\"1\"/><c:tx><c:rich><a:bodyPr/><a:p><a:fld id=\"{1}\" type=\"VALUE\"><a:t>[VALUE]</a:t></a:fld></a:p></c:rich></c:tx><c:showVal val=\"1\"/></c:dLbl><c:numFmt formatCode=\"0.0\" sourceLinked=\"0\"/><c:showVal val=\"1\"/><c:showLeaderLines val=\"1\"/></c:dLbls>";
    let tl = "<c:trendline><c:spPr><a:ln w=\"12700\"><a:prstDash val=\"dash\"/></a:ln></c:spPr><c:trendlineType val=\"poly\"/><c:order val=\"3\"/><c:dispEq val=\"1\"/></c:trendline>";
    let sers: String =
        (0..s).map(|i| series_xml(i, n, &format!("{dl}{tl}<c:dPt><c:idx val=\"0\"/><c:spPr><a:noFill/></c:spPr></c:dPt>"))).collect();
    format!(
        "<c:chartSpace><c:style val=\"2\"/><c:roundedCorners val=\"0\"/><c:chart><c:title><c:tx><c:rich><a:bodyPr rot=\"0\"/><a:p><a:pPr><a:defRPr sz=\"1800\"/></a:pPr><a:r><a:rPr lang=\"en-US\"/><a:t>Title</a:t></a:r></a:p></c:rich></c:tx></c:title>\
         <c:view3D><c:rotX val=\"15\"/></c:view3D><c:plotArea><c:layout><c:manualLayout><c:layoutTarget val=\"inner\"/><c:x val=\"0.1\"/><c:y val=\"0.2\"/><c:w val=\"0.8\"/><c:h val=\"0.6\"/></c:manualLayout></c:layout>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{sers}<c:gapWidth val=\"80\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:crossAx val=\"2\"/></c:catAx>\
         <c:valAx><c:axId val=\"2\"/><c:scaling><c:max val=\"60\"/></c:scaling><c:axPos val=\"l\"/><c:majorGridlines><c:spPr><a:ln w=\"9525\"><a:solidFill><a:srgbClr val=\"D9D9D9\"/></a:solidFill></a:ln></c:spPr></c:majorGridlines><c:crossAx val=\"1\"/><c:dispUnits><c:builtInUnit val=\"thousands\"/></c:dispUnits></c:valAx>\
         <c:spPr><a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs><a:gs pos=\"100000\"><a:srgbClr val=\"EEEEEE\"/></a:gs></a:gsLst><a:lin ang=\"5400000\"/></a:gradFill></c:spPr></c:plotArea>\
         <c:legend><c:legendPos val=\"r\"/><c:legendEntry><c:idx val=\"0\"/><c:delete val=\"1\"/></c:legendEntry></c:legend><c:dispBlanksAs val=\"gap\"/></c:chart>\
         <c:spPr><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill></c:spPr><c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz=\"1000\"><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:pPr></a:p></c:txPr></c:chartSpace>"
    )
}

/// A pie, a scatter, a line chart, an empty chart and a `cx:` chart.
pub fn other_charts() -> Vec<String> {
    let wrap = |plot: &str| {
        format!(
            "<c:chartSpace><c:chart><c:plotArea>{plot}</c:plotArea><c:legend><c:legendPos val=\"b\"/></c:legend></c:chart></c:chartSpace>"
        )
    };
    vec![
        wrap(&format!(
            "<c:pieChart><c:varyColors val=\"1\"/>{}<c:firstSliceAng val=\"30\"/></c:pieChart>",
            series_xml(0, 8, "<c:explosion val=\"5\"/>")
        )),
        wrap(&format!(
            "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:xVal><c:numLit><c:ptCount val=\"20\"/>{}</c:numLit></c:xVal><c:yVal><c:numLit><c:ptCount val=\"20\"/>{}</c:numLit></c:yVal></c:ser><c:axId val=\"1\"/><c:axId val=\"2\"/></c:scatterChart>\
             <c:valAx><c:axId val=\"1\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:valAx><c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/><c:crossAx val=\"1\"/></c:valAx>",
            pts(20, |k| k.to_string()),
            pts(20, |k| (k * k).to_string())
        )),
        wrap(&format!(
            "<c:lineChart><c:grouping val=\"standard\"/>{}{}<c:marker val=\"1\"/></c:lineChart>",
            series_xml(0, 30, ""),
            series_xml(1, 30, "")
        )),
        "<c:chartSpace><c:chart/></c:chartSpace>".to_owned(),
        "<cx:chartSpace><cx:chart/></cx:chartSpace>".to_owned(),
    ]
}

/// The built-in corpus of the memory and no-panic tests.
pub fn corpus() -> Vec<String> {
    let mut v = vec![rich_chart(1, 4), rich_chart(3, 40), rich_chart(2, 400)];
    v.extend(other_charts());
    v
}

/// The chart XML of a capture (`tests/captures/<stem>.data.jsonl.gz`, or the stem of `UO_CHARTS_C1_STEM`), when it exists.
pub fn capture_xmls(stem: &str) -> Vec<String> {
    let stem = std::env::var("UO_CHARTS_C1_STEM").unwrap_or_else(|_| stem.to_owned());
    let stem = stem.as_str();
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("captures");
    let path = base.join(format!("{stem}.data.jsonl.gz"));
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    let text = String::from_utf8(gunzip(&bytes)).unwrap();
    text.lines().filter(|l| !l.is_empty()).map(|l| json(l).get("xml").unwrap().s().to_owned()).collect()
}

pub fn doc(xml: &str) -> Document {
    parse(xml.to_owned(), &ParseOptions::LENIENT).unwrap()
}
