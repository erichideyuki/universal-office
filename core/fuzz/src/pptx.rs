//! Fuzz targets of the Slides read path (`uo-pptx`, `uo-dml-render`; records `core/docs/modules/uo-pptx.md`
//! and `uo-dml-render.md`, frontiers B1a/B1b).
//!
//! - `pptx_open`: the input is a `.pptx` (ZIP): open it with small limits and read the presentation, every
//!   slide (and its notes), the embedded fonts, the deck typefaces and a save. The seeds are the committed
//!   generated decks, so the mutation loop reaches the XML through the inflate step as well as the zip
//!   structure.
//! - `dml_shapes`: structure-aware: the input (UTF-8, lossy) is the CONTENT of a `<p:spTree>` read by the
//!   shared shape reader over an in-memory package: groups 10^4 deep, huge `avLst`, NaN/infinity attributes,
//!   tables at the limit, merges pointing outside, custom geometry with hostile guides. Byte 0 selects the
//!   theme and the limits.
//!
//! Invariants beyond "no panic": a read that succeeds respects the caps (shapes per list), the group depth
//! limit is an error and never a stack overflow, and reading the same input twice gives the same model.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use uo_dml_render::canon::{Canon, Json};
use uo_dml_render::ids::MediaId;
use uo_dml_render::read::read_shapes;
use uo_dml_render::read_dml::Rd;
use uo_dml_render::source::{Limits as DmlLimits, PartSource, R, RelMap, parse_xml};
use uo_dml_render::theme::{Theme, read_theme};
use uo_opc::xml::Document;
use uo_pptx::{Deck, Limits};

/// Limits small enough that an execution stays fast.
fn small_limits() -> Limits {
    Limits {
        max_slides: 64,
        max_shapes_per_slide: 2_000,
        max_group_depth: 24,
        max_table_cells: 10_000,
        max_xml_cache_bytes: 8 << 20,
        max_cached_slides: 4,
        ..Limits::default()
    }
}

/// `pptx_open`: a `.pptx` read end to end.
pub fn pptx_open(data: &[u8]) {
    let opc = uo_opc::Limits {
        max_xml_part_bytes: 4 << 20,
        max_binary_part_bytes: 8 << 20,
        max_total_bytes: 32 << 20,
        max_entries: 512,
        max_xml_depth: 256,
        max_xml_elements: 400_000,
    };
    let Ok(mut deck) = Deck::open_with(data, small_limits(), &opc) else { return };
    let Ok(pres) = deck.presentation().cloned() else { return };
    let n = pres.slides.len().min(8);
    for i in 0..n {
        let i = u32::try_from(i).unwrap_or(0);
        if let Ok(a) = deck.slide(i) {
            // the same read twice: deterministic, equal through the cache and a fresh read (canonical
            // text: `==` is false for NaN and the Debug of a parsed tree lists a hash map)
            let text = |d: &Deck, m: &uo_pptx::SlideModel| {
                let src = d.source();
                uo_pptx::canon::slide_record(&Canon { src: &src }, "", m)
            };
            let ta = text(&deck, &a);
            deck.clear_caches();
            if let Ok(b) = deck.slide(i) {
                assert!(ta == text(&deck, &b), "slide {i}: two reads of the same deck differ");
            }
            assert!(a.shapes.len() <= small_limits().max_shapes_per_slide as usize + 1);
        }
        let _ = deck.slide_notes(i);
    }
    let _ = deck.embedded_fonts();
    let _ = deck.deck_typefaces();
    if let Ok(saved) = deck.save() {
        assert!(Deck::open_with(&saved, small_limits(), &opc).is_ok(), "a saved package must reopen");
    }
}

/// A package of a few in-memory parts for `dml_shapes`.
struct Mem {
    parts: HashMap<&'static str, Arc<[u8]>>,
}

impl PartSource for Mem {
    fn part(&self, name: &str) -> Option<Arc<[u8]>> {
        self.parts.get(name).cloned()
    }

    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        match self.parts.get(name) {
            Some(b) => Ok(Some(Arc::new(parse_xml(String::from_utf8_lossy(b).into_owned())?))),
            None => Ok(None),
        }
    }

    fn media(&self, part: &str) -> Option<MediaId> {
        self.parts.contains_key(part).then_some(MediaId(0))
    }

    fn media_entry(&self, _id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

const THEME: &str = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:themeElements><a:clrScheme name=\"x\">\
<a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"44546A\"/></a:dk2>\
<a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2><a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1><a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink></a:clrScheme>\
<a:fontScheme name=\"f\"><a:majorFont><a:latin typeface=\"Calibri Light\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"m\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:gradFill><a:gsLst><a:gs pos=\"0\"><a:schemeClr val=\"phClr\"/></a:gs>\
<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:shade val=\"50000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\"/></a:gradFill></a:fillStyleLst>\
<a:lnStyleLst><a:ln w=\"9525\"/><a:ln w=\"25400\"/></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst><a:outerShdw blurRad=\"40000\" dist=\"23000\" dir=\"5400000\">\
<a:srgbClr val=\"000000\"><a:alpha val=\"35000\"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst></a:fmtScheme></a:themeElements></a:theme>";

/// `dml_shapes`: a shape tree read by the shared reader.
pub fn dml_shapes(data: &[u8]) {
    let Some((&sel, body)) = data.split_first() else { return };
    let mem = Mem {
        parts: HashMap::from([
            ("ppt/theme/theme1.xml", Arc::from(THEME.as_bytes())),
            ("ppt/media/image1.png", Arc::from(&b"\x89PNG"[..])),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                Arc::from(
                    &b"<Relationships><Relationship Id=\"rId1\" Type=\"x/image\" Target=\"../media/image1.png\"/></Relationships>"[..],
                ),
            ),
        ]),
    };
    let Ok(theme) = read_theme(&mem, if sel & 1 == 0 { "ppt/theme/theme1.xml" } else { "ppt/theme/none.xml" }) else { return };
    let mut rels = RelMap::new();
    rels.set("rId1", "ppt/media/image1.png");
    let limits = DmlLimits { max_group_depth: 4 + u32::from(sel >> 4), max_shapes: 1_000, max_table_cells: 4_096 };
    let rd = Rd { src: &mem, limits };
    let xml = format!("<p:spTree xmlns:p=\"p\" xmlns:a=\"a\" xmlns:r=\"r\">{}</p:spTree>", String::from_utf8_lossy(body));
    let Ok(doc) = parse_xml(xml) else { return };
    let first = read_shapes(&rd, doc.root(), &theme, &rels);
    let second: R<_> = read_shapes(&rd, doc.root(), &Theme::clone(&theme), &rels);
    // compared as canonical text: NaN != NaN under `==` and the model carries NaN from hostile attributes
    let canon = Canon { src: &mem };
    let text = |r: &R<Vec<uo_dml_render::Shape>>| match r {
        Ok(v) => Json::Arr(v.iter().map(|s| canon.shape(s)).collect()).to_text(),
        Err(e) => format!("err {e}"),
    };
    assert!(text(&first) == text(&second), "two reads of the same tree differ");
    if let Ok(shapes) = first {
        assert!(shapes.len() <= 1_001);
    }
}

/// Seeds of `pptx_open` (the smallest committed decks) and `dml_shapes` (shape snippets).
pub fn pptx_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let dir = repo.join("core/crates/uo-pptx/tests/captures/decks/b1a");
    for n in ["0003", "0004", "0006", "0010"] {
        if let Ok(b) = std::fs::read(dir.join(format!("{n}.pptx"))) {
            out.push(("pptx_open", b));
        }
    }
    let snippets: [&str; 6] = [
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"T\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot=\"5400000\" flipH=\"1\"><a:off x=\"1\" y=\"2\"/><a:ext cx=\"3\" cy=\"4\"/></a:xfrm><a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 16667\"/></a:avLst></a:prstGeom><a:solidFill><a:schemeClr val=\"accent1\"><a:lumMod val=\"50000\"/><a:alpha val=\"50000\"/></a:schemeClr></a:solidFill><a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:prstDash val=\"dash\"/></a:ln></p:spPr><p:txBody><a:bodyPr anchor=\"ctr\"><a:normAutofit fontScale=\"62500\"/></a:bodyPr><a:p><a:pPr lvl=\"1\" algn=\"ctr\"><a:buChar char=\"x\"/></a:pPr><a:r><a:rPr sz=\"1800\" b=\"1\"><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></a:rPr><a:t>Hello</a:t></a:r></a:p></p:txBody></p:sp>",
        "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"3\" name=\"G\"/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"50\" cy=\"50\"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id=\"4\" name=\"c\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"5\" y=\"5\"/><a:ext cx=\"10\" cy=\"10\"/></a:xfrm><a:grpFill/></p:spPr></p:sp></p:grpSp>",
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"5\" name=\"t\"/></p:nvGraphicFramePr><p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9\" cy=\"9\"/></p:xfrm><a:graphic><a:graphicData><a:tbl><a:tblPr firstRow=\"1\" bandRow=\"1\"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr><a:tblGrid><a:gridCol w=\"10\"/><a:gridCol w=\"10\"/></a:tblGrid><a:tr h=\"5\"><a:tc gridSpan=\"2\"><a:txBody><a:bodyPr/><a:p><a:r><a:t>a</a:t></a:r></a:p></a:txBody></a:tc><a:tc hMerge=\"1\"/></a:tr><a:tr h=\"5\"><a:tc rowSpan=\"9\"/></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        "<p:pic><p:nvPicPr><p:cNvPr id=\"6\" name=\"p\"/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId1\"><a:duotone><a:srgbClr val=\"000000\"/><a:srgbClr val=\"FFFFFF\"/></a:duotone></a:blip><a:srcRect l=\"-1\" t=\"1e9\"/><a:tile sx=\"1\"/></p:blipFill><p:spPr/></p:pic>",
        "<p:sp><p:spPr><a:custGeom><a:avLst/><a:gdLst><a:gd name=\"x\" fmla=\"*/ w 1 0\"/></a:gdLst><a:pathLst><a:path w=\"10\" h=\"10\"><a:moveTo><a:pt x=\"x\" y=\"0\"/></a:moveTo><a:arcTo wR=\"5\" hR=\"5\" stAng=\"0\" swAng=\"21600000\"/><a:cubicBezTo><a:pt x=\"1\" y=\"1\"/><a:pt x=\"2\" y=\"2\"/><a:pt x=\"3\" y=\"3\"/></a:cubicBezTo><a:close/></a:path></a:pathLst></a:custGeom></p:spPr></p:sp>",
        "<mc:AlternateContent xmlns:mc=\"mc\"><mc:Fallback><p:sp><p:spPr><a:gradFill><a:gsLst><a:gs pos=\"NaN\"><a:srgbClr val=\"zz\"><a:hueOff val=\"99999999999999999999\"/></a:srgbClr></a:gs><a:gs pos=\"1e5\"><a:sysClr lastClr=\"1\"/></a:gs></a:gsLst><a:path path=\"rect\"><a:fillToRect l=\"x\"/></a:path></a:gradFill></p:spPr></p:sp></mc:Fallback></mc:AlternateContent>",
    ];
    for (i, s) in snippets.iter().enumerate() {
        let mut v = vec![u8::try_from(i * 16 % 256).unwrap_or(0)];
        v.extend_from_slice(s.as_bytes());
        out.push(("dml_shapes", v));
    }
    out
}
