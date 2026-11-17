//! The replay host (C-3 onwards): a [`ChartHost`] that answers from the TS call logs of a capture (the parse log and the
//! paint-phase `themeFill` log), so the emit is replayed without a theme resolver. `BlipKey(i)` is the index of the
//! chart's image in `ChartData::images` (the recorder's source strings), the host's own blips (any other part) follow.
//! (`tests/c1_replay.rs` keeps its own copy for the model replay: the facet it checks there does not need the images.)

use std::cell::RefCell;
use std::collections::HashMap;

use uo_charts::CHART_PREFIXES;
use uo_charts::canon::{Json, element_json};
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{BlipTile, ChartColor, FillRect, GradPath, GradStop, Paint, TileFlip};
use uo_opc::xml::{CanonicalPrefixes, Document, Element, ParseOptions, parse};

use super::data::{ChartData, Host};
use super::json::{self, J};

pub struct RHost {
    ppt: bool,
    format: Option<FileFormat>,
    minor: Option<String>,
    major: Option<String>,
    widths: Vec<f64>,
    shapes: Option<Document>,
    answers: HashMap<String, String>,
    blips: RefCell<Vec<String>>,
    pub misses: RefCell<Vec<String>>,
}

/// The Sheet profile: canonical prefixes (the TS `canonicalNs`).
pub fn xl_options() -> ParseOptions<'static> {
    ParseOptions {
        canonical: Some(CanonicalPrefixes { table: CHART_PREFIXES, default_namespace: true }),
        normalize_eol: true,
        ..ParseOptions::LENIENT
    }
}

/// The chart part parsed as its host bridge does (Slides: as is; Sheet: canonical prefixes).
pub fn parse_xml(chart: &ChartData) -> Option<Document> {
    match chart.host {
        Host::Ppt => parse(chart.xml.clone(), &ParseOptions::LENIENT).ok(),
        Host::Xl => parse(chart.xml.clone(), &xl_options()).ok(),
    }
}

impl RHost {
    pub fn new(chart: &ChartData) -> RHost {
        let mut answers = HashMap::new();
        for e in chart.host_log.iter().chain(&chart.host_paint) {
            let k = match e.kind {
                super::data::Kind::Color => "color",
                super::data::Kind::Blip => "blip",
                super::data::Kind::Fill => "fill",
            };
            answers.entry(format!("{k}\t{}", e.key)).or_insert_with(|| e.ans.clone());
        }
        let shapes = match (&chart.user_shapes, chart.host) {
            (Some(s), Host::Xl) => parse(s.clone(), &xl_options()).ok(),
            _ => None,
        };
        RHost {
            ppt: chart.host == Host::Ppt,
            format: chart.file_format,
            minor: chart.minor_font.clone(),
            major: chart.major_font.clone(),
            widths: chart.theme_ln.clone(),
            shapes,
            answers,
            blips: RefCell::new(chart.images.iter().map(|i| i.part.clone()).collect()),
            misses: RefCell::new(Vec::new()),
        }
    }

    fn answer(&self, kind: &str, key: &str) -> String {
        let k = format!("{kind}\t{key}");
        match self.answers.get(&k) {
            Some(a) => a.clone(),
            None => {
                self.misses.borrow_mut().push(k);
                "null".to_owned()
            }
        }
    }

    fn blip_ix(&self, name: &str) -> BlipKey {
        let mut b = self.blips.borrow_mut();
        let ix = b.iter().position(|x| x == name).unwrap_or_else(|| {
            b.push(name.to_owned());
            b.len() - 1
        });
        BlipKey(u32::try_from(ix).unwrap_or(u32::MAX))
    }
}

fn color_j(j: &J) -> ChartColor {
    ChartColor::new(j.str().unwrap_or(""))
}

fn n(j: &J, k: &str) -> f64 {
    j.get(k).and_then(J::num).unwrap_or(f64::NAN)
}

fn paint_of(j: &J, h: &RHost) -> Paint {
    match j.get("kind").and_then(J::str).unwrap_or("none") {
        "solid" => Paint::Solid { color: color_j(j.get("color").unwrap_or(&J::Null)), alpha: j.get("alpha").and_then(J::num) },
        "grad" => Paint::Grad {
            stops: j
                .get("stops")
                .map(J::arr)
                .unwrap_or(&[])
                .iter()
                .map(|s| GradStop {
                    pos: n(s, "pos"),
                    color: color_j(s.get("color").unwrap_or(&J::Null)),
                    alpha: s.get("alpha").and_then(J::num),
                })
                .collect(),
            angle: n(j, "angle"),
            path: j.get("path").and_then(J::str).map(|p| match p {
                "circle" => GradPath::Circle,
                "rect" => GradPath::Rect,
                "shape" => GradPath::Shape,
                o => GradPath::Other(o.into()),
            }),
        },
        "patt" => Paint::Patt {
            fg: color_j(j.get("fg").unwrap_or(&J::Null)),
            bg: color_j(j.get("bg").unwrap_or(&J::Null)),
            prst: j.get("prst").and_then(J::str).unwrap_or("").into(),
        },
        "blip" => Paint::Blip {
            key: h.blip_ix(j.get("ref").and_then(J::str).unwrap_or("")),
            tile: j.get("tile").map(|t| {
                Box::new(BlipTile {
                    sx: n(t, "sx"),
                    sy: n(t, "sy"),
                    tx: n(t, "tx"),
                    ty: n(t, "ty"),
                    algn: t.get("algn").and_then(J::str).unwrap_or("tl").into(),
                    flip: t.get("flip").and_then(J::str).map(|f| match f {
                        "x" => TileFlip::X,
                        "y" => TileFlip::Y,
                        _ => TileFlip::Xy,
                    }),
                })
            }),
            fill_rect: j.get("fillRect").map(|r| Box::new(FillRect { l: n(r, "l"), t: n(r, "t"), r: n(r, "r"), b: n(r, "b") })),
            alpha: j.get("alpha").and_then(J::num),
        },
        _ => Paint::None,
    }
}

impl ChartHost for RHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        let key = match src {
            ColorSource::Element(e) => element_json(e).to_text(),
            ColorSource::Scheme(v) => Json::Obj(vec![
                ("name".to_owned(), Json::s("a:schemeClr")),
                ("attrs".to_owned(), Json::Obj(vec![("val".to_owned(), Json::s(v))])),
                ("children".to_owned(), Json::Arr(Vec::new())),
            ])
            .to_text(),
        };
        match json::parse(&self.answer("color", &key)) {
            Ok(J::Str(s)) => Some(ChartColor::new(&s)),
            _ => None,
        }
    }

    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: self.major.as_deref(), minor: self.minor.as_deref() }
    }

    fn has_fill_style(&self) -> bool {
        true
    }

    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        match json::parse(&self.answer("fill", &format!("{idx}|{ph_clr}"))) {
            Ok(J::Null) | Err(_) => None,
            Ok(j) => Some(paint_of(&j, self)),
        }
    }

    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        if !self.ppt {
            return None;
        }
        match json::parse(&self.answer("blip", &element_json(blip).to_text())) {
            Ok(J::Str(name)) => Some(self.blip_ix(&name)),
            _ => None,
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
