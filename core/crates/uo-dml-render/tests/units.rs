//! Behaviour tests of the pieces the parity corpora (generated decks + 53 samples) do not reach:
//! measured colour results, the cascade merge rules, table styles, relationship helpers, the sheet
//! drawing path (`xdr:` prefix, hidden shapes), the OLE preview from VML, the chart theme override and
//! the symbol fonts. Numbers marked "measured" come from the TS comments (oracle: PowerPoint).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use uo_dml_render::ids::MediaId;
use uo_dml_render::model::*;
use uo_dml_render::read::{read_drawing_elements, read_shapes};
use uo_dml_render::read_dml::Rd;
use uo_dml_render::source::{
    ElementRef, Limits, PartSource, R, RelMap, ShapePrefix, parse_xml, read_rels, rels_path_for, resolve_rel_target,
};
use uo_dml_render::text_style::{TextStyleLevel, empty_levels, merge_style_levels};
use uo_dml_render::theme::{Theme, color_from_dml, read_theme};
use uo_opc::xml::Document;

struct Mem {
    parts: HashMap<String, Arc<[u8]>>,
}

impl Mem {
    fn new(parts: &[(&str, &str)]) -> Mem {
        Mem { parts: parts.iter().map(|(k, v)| ((*k).to_string(), Arc::from(v.as_bytes()))).collect() }
    }
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

const NS: &str = "xmlns:a=\"a\" xmlns:p=\"p\" xmlns:r=\"r\" xmlns:xdr=\"x\" xmlns:mc=\"mc\"";

fn theme_xml(extra: &str) -> String {
    format!(
        "<a:theme xmlns:a=\"a\"><a:themeElements><a:clrScheme name=\"t\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"44546A\"/></a:dk2><a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"04617B\"/></a:accent1><a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink></a:clrScheme>{extra}</a:themeElements></a:theme>"
    )
}

fn theme() -> Theme {
    read_theme(&Mem::new(&[("t.xml", &theme_xml(""))]), "t.xml").unwrap()
}

fn color(inner: &str, th: &Theme) -> Option<String> {
    let doc = parse_xml(format!("<a:c {NS}>{inner}</a:c>")).unwrap();
    color_from_dml(doc.root().child_elements().next(), th).map(|c| c.as_str().to_string())
}

#[test]
fn colour_profile_matches_the_measured_values() {
    let th = theme();
    // measured in PowerPoint: 04617B with shade 15% → 012430; satMod 320% → 00ABFE (lower case in the TS)
    assert_eq!(color("<a:schemeClr val=\"accent1\"><a:shade val=\"15000\"/></a:schemeClr>", &th).as_deref(), Some("#012430"));
    assert_eq!(color("<a:schemeClr val=\"accent1\"><a:satMod val=\"320000\"/></a:schemeClr>", &th).as_deref(), Some("#00abfe"));
    // untouched colours keep the file's spelling; alpha does not touch
    assert_eq!(color("<a:srgbClr val=\"abc\"/>", &th).as_deref(), Some("#abc"));
    assert_eq!(color("<a:srgbClr val=\"AbCdEf\"><a:alpha val=\"50000\"/></a:srgbClr>", &th).as_deref(), Some("#AbCdEf"));
    assert_eq!(color("<a:srgbClr/>", &th).as_deref(), Some("#000000"));
    assert_eq!(color("<a:sysClr lastClr=\"123456\"/>", &th).as_deref(), Some("#123456"));
    assert_eq!(color("<a:prstClr val=\"red\"/>", &th).as_deref(), Some("#FF0000"));
    assert_eq!(color("<a:prstClr val=\"nope\"/>", &th).as_deref(), Some("#000000"));
    // aliases: tx1/bg1 through the standard map; an unknown scheme colour is black
    assert_eq!(color("<a:schemeClr val=\"tx1\"/>", &th).as_deref(), Some("#000000"));
    assert_eq!(color("<a:schemeClr val=\"bg1\"/>", &th).as_deref(), Some("#FFFFFF"));
    assert_eq!(color("<a:schemeClr val=\"zzz\"/>", &th).as_deref(), Some("#000000"));
    // a non-colour element resolves to nothing
    assert_eq!(color("<a:ln/>", &th), None);
    // garbage values flow like in the JS: NaN channels print `NaN`
    assert_eq!(color("<a:srgbClr val=\"000000\"><a:lumMod val=\"x\"/></a:srgbClr>", &th).as_deref(), Some("#NaNNaNNaN"));
    // the sRGB mode between gamma and invGamma
    let lin = color("<a:srgbClr val=\"808080\"><a:shade val=\"50000\"/></a:srgbClr>", &th).unwrap();
    let srgb = color("<a:srgbClr val=\"808080\"><a:gamma/><a:shade val=\"50000\"/><a:invGamma/></a:srgbClr>", &th).unwrap();
    assert_eq!(srgb, "#404040");
    assert_ne!(lin, srgb);
}

#[test]
fn theme_reading_and_clr_map() {
    let th = theme();
    assert_eq!(th.colors.get("tx1").map(|c| c.as_str()), Some("#000000"));
    assert_eq!(th.scheme.as_ref().unwrap().get("tx1"), None); // the raw scheme has no aliases
    assert_eq!(th.part.as_deref(), Some("t.xml"));
    let dark = parse_xml("<p:clrMap xmlns:p=\"p\" bg1=\"dk1\" tx1=\"lt1\" bg2=\"dk2\" tx2=\"lt2\" empty=\"\"/>".to_string()).unwrap();
    let map = uo_dml_render::theme::clr_map_attrs(Some(dark.root())).unwrap();
    assert!(map.get("empty").is_none() && map.len() == 4);
    let d = uo_dml_render::theme::apply_clr_map(th.clone(), Some(&map));
    assert_eq!(d.colors.get("bg1").map(|c| c.as_str()), Some("#000000"));
    assert_eq!(d.colors.get("tx1").map(|c| c.as_str()), Some("#FFFFFF"));
    assert_eq!(th.colors.get("bg1").map(|c| c.as_str()), Some("#FFFFFF")); // the master's theme is intact
    // without a theme part: an empty colour table and no scheme
    let none = read_theme(&Mem::new(&[]), "missing.xml").unwrap();
    assert!(none.colors.is_empty() && none.scheme.is_none() && none.part.is_none());
}

#[test]
fn level_merge_rules() {
    let mut a = empty_levels();
    a[0] = TextStyleLevel {
        sz: Some(10.0),
        line_spacing: Some(1.5),
        bu_char: Some("x".into()),
        space_before_pt: Some(3.0),
        ..TextStyleLevel::default()
    };
    let mut b = empty_levels();
    b[0] = TextStyleLevel {
        line_spacing_pt: Some(20.0),
        bu_none: Some(true),
        space_before_pct: Some(0.2),
        bold: Some(true),
        ..TextStyleLevel::default()
    };
    let m = merge_style_levels(&[&a, &b]);
    // points × percent: the later erases the earlier; a bullet layer replaces the whole bullet
    assert_eq!((m[0].line_spacing, m[0].line_spacing_pt), (None, Some(20.0)));
    assert_eq!((m[0].space_before_pt, m[0].space_before_pct), (None, Some(0.2)));
    assert_eq!((m[0].bu_char.as_deref(), m[0].bu_none), (None, Some(true)));
    assert_eq!((m[0].sz, m[0].bold), (Some(10.0), Some(true)));
}

#[test]
fn relationship_helpers() {
    assert_eq!(resolve_rel_target("ppt/slides/slide1.xml", "../slideLayouts/slideLayout1.xml"), "ppt/slideLayouts/slideLayout1.xml");
    assert_eq!(resolve_rel_target("ppt/slides/slide1.xml", "/ppt/media/a.png"), "ppt/media/a.png");
    assert_eq!(resolve_rel_target("ppt/slides/slide1.xml", "../../../x.xml"), "x.xml");
    assert_eq!(resolve_rel_target("slide1.xml", "a/b.xml"), "a/b.xml");
    assert_eq!(rels_path_for("ppt/slides/slide1.xml"), "ppt/slides/_rels/slide1.xml.rels");
    let mem = Mem::new(&[(
        "ppt/slides/_rels/s.xml.rels",
        "<Relationships><Relationship Id=\"a\" Target=\"../x.xml\"/><Relationship Id=\"b\" Target=\"https://e.com\" TargetMode=\"External\"/><Relationship Id=\"a\" Target=\"../y.xml\"/><Relationship Target=\"no-id\"/></Relationships>",
    )]);
    let rels: RelMap = read_rels(&mem, "ppt/slides/_rels/s.xml.rels", "ppt/slides/s.xml").unwrap();
    // Map.set semantics: a repeated id keeps its first position and the last target
    assert_eq!(rels.iter().collect::<Vec<_>>(), vec![("a", "ppt/y.xml"), ("b", "https://e.com")]);
}

#[test]
fn sheet_drawing_path_prunes_hidden_shapes_and_renames_the_prefix() {
    let drawing = format!(
        "<xdr:wsDr {NS}><xdr:twoCellAnchor><xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"2\" name=\"visible\"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x=\"1\" y=\"2\"/><a:ext cx=\"3\" cy=\"4\"/></a:xfrm><a:prstGeom prst=\"rect\"/></xdr:spPr></xdr:sp></xdr:twoCellAnchor>\
<xdr:twoCellAnchor><xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"3\" name=\"hidden\" hidden=\"1\"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr/></xdr:sp></xdr:twoCellAnchor>\
<xdr:twoCellAnchor><xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id=\"4\" name=\"g\"/></xdr:nvGrpSpPr><xdr:grpSpPr/><xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"5\" name=\"in\"/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9\" cy=\"9\"/></a:xfrm></xdr:spPr></xdr:sp>\
<xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"6\" name=\"gone\" hidden=\"true\"/></xdr:nvSpPr><xdr:spPr/></xdr:sp></xdr:grpSp></xdr:twoCellAnchor></xdr:wsDr>"
    );
    let mem = Mem::new(&[]);
    let doc = Arc::new(parse_xml(drawing).unwrap());
    let anchors: Vec<ElementRef> = doc
        .root()
        .child_elements()
        .map(|a| ElementRef { doc: Arc::clone(&doc), element: a.child_elements().next().unwrap().id() })
        .collect();
    let out = read_drawing_elements(&mem, Limits::default(), "xl/drawings/drawing1.xml", &anchors, &theme(), ShapePrefix::Xdr).unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].iter().map(|s| &*s.name).collect::<Vec<_>>(), ["visible"]);
    assert_eq!(out[0][0].xfrm.map(|x| (x.x, x.y, x.cx, x.cy)), Some((1.0, 2.0, 3.0, 4.0)));
    assert!(out[1].is_empty(), "a hidden shape is not drawn");
    assert_eq!(out[2].iter().map(|s| &*s.name).collect::<Vec<_>>(), ["in"], "hidden shapes inside a group are pruned too");
    assert_eq!(out[2][0].extras.as_ref().unwrap().group_ids.as_deref().unwrap(), [Box::<str>::from("4")]);
}

fn shapes_of(mem: &Mem, xml: &str, th: &Theme, rels: &RelMap) -> Vec<Shape> {
    let doc = parse_xml(format!("<p:spTree {NS}>{xml}</p:spTree>")).unwrap();
    read_shapes(&Rd { src: mem, limits: Limits::default() }, doc.root(), th, rels).unwrap()
}

#[test]
fn ole_preview_from_the_vml_drawing() {
    let vml = "<xml><v:shape id=\"_x0000_s1026\" type=\"#t\" filled=\"t\" fillcolor=\"#3cf\" stroked=\"t\" strokecolor=\"red [3]\" strokeweight=\"2pt\"><v:imagedata o:relid=\"rId9\" o:title=\"\"/></v:shape></xml>";
    let mem = Mem::new(&[
        ("ppt/drawings/vmlDrawing1.vml", vml),
        (
            "ppt/drawings/_rels/vmlDrawing1.vml.rels",
            "<Relationships><Relationship Id=\"rId9\" Target=\"../media/image1.emf\"/></Relationships>",
        ),
        ("ppt/media/image1.emf", "EMF"),
    ]);
    let mut rels = RelMap::new();
    rels.set("rId1", "ppt/drawings/vmlDrawing1.vml");
    let gf = "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"7\" name=\"ole\"/></p:nvGraphicFramePr><p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"5\" cy=\"5\"/></p:xfrm><a:graphic><a:graphicData><p:oleObj spid=\"_x0000_s1026\"/></a:graphicData></a:graphic></p:graphicFrame>";
    let s = shapes_of(&mem, gf, &theme(), &rels);
    assert_eq!(s.len(), 1);
    let sh = &s[0];
    assert_eq!(sh.image.as_ref().map(|i| &*i.mime), Some("image/x-emf"));
    assert_eq!(sh.fill.as_ref().and_then(|f| f.color.as_ref()).map(|c| c.as_str()), Some("#33CCFF"));
    assert_eq!(sh.line_color.as_ref().map(|c| c.as_str()), Some("#FF0000"));
    assert_eq!(sh.line_width, Some(25400.0));
}

#[test]
fn chart_with_a_theme_override_and_images() {
    let override_xml = "<a:themeOverride xmlns:a=\"a\"><a:clrScheme name=\"o\"><a:accent1><a:srgbClr val=\"FF0000\"/></a:accent1></a:clrScheme><a:fontScheme name=\"f\"><a:majorFont><a:latin typeface=\"Over\"/></a:majorFont></a:fontScheme></a:themeOverride>";
    let chart = "<c:chartSpace xmlns:c=\"c\"><a:blip r:embed=\"rId2\" r:embed2=\"x\"/></c:chartSpace>";
    let mem = Mem::new(&[
        ("ppt/charts/chart1.xml", chart),
        (
            "ppt/charts/_rels/chart1.xml.rels",
            "<Relationships><Relationship Id=\"rId1\" Target=\"themeOverride1.xml\"/><Relationship Id=\"rId2\" Target=\"../media/p.png\"/><Relationship Id=\"rId3\" Target=\"../media/q.png\"/></Relationships>",
        ),
        ("ppt/charts/themeOverride1.xml", override_xml),
        ("ppt/media/p.png", "PNG"),
        ("ppt/media/q.png", "PNG"),
    ]);
    let mut rels = RelMap::new();
    rels.set("rId5", "ppt/charts/chart1.xml");
    let mut th = read_theme(&Mem::new(&[("t.xml", &theme_xml("<a:fontScheme name=\"b\"><a:majorFont><a:latin typeface=\"Base\"/></a:majorFont><a:minorFont><a:latin typeface=\"BaseMinor\"/></a:minorFont></a:fontScheme>"))]), "t.xml").unwrap();
    assert_eq!(th.fonts.as_ref().unwrap().major.as_deref(), Some("Base"));
    th.clr_map = None;
    let gf = "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"8\" name=\"c\"/></p:nvGraphicFramePr><p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"5\" cy=\"5\"/></p:xfrm><a:graphic><a:graphicData><c:chart xmlns:c=\"c\" r:id=\"rId5\"/></a:graphicData></a:graphic></p:graphicFrame>";
    let s = shapes_of(&mem, gf, &th, &rels);
    let c = s[0].extras.as_ref().unwrap().chart.as_ref().unwrap();
    assert_eq!(&*c.part, "ppt/charts/chart1.xml");
    assert_eq!(c.theme.colors.get("accent1").map(|x| x.as_str()), Some("#FF0000"));
    let f = c.theme.fonts.as_ref().unwrap();
    assert_eq!((f.major.as_deref(), f.minor.as_deref()), (Some("Over"), Some("BaseMinor")));
    // only the image the chart XML cites by `embed="rIdN"` is kept
    let ids: Vec<&str> = c.images.as_ref().unwrap().iter().map(|i| &*i.r_id).collect();
    assert_eq!(ids, ["rId2"]);
    assert_eq!(c.format, Some(ChartFormat::Modern));
    let app = Mem::new(&[(
        "docProps/app.xml",
        "<Properties><Application> Microsoft Office PowerPoint </Application><AppVersion>12.0000</AppVersion></Properties>",
    )]);
    assert_eq!(uo_dml_render::read::chart_file_format(&app), ChartFormat::Mso2007);
}

#[test]
fn group_transform_flip_and_rotation() {
    let mem = Mem::new(&[]);
    let g = "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"g\"/></p:nvGrpSpPr><p:grpSpPr><a:xfrm flipH=\"1\"><a:off x=\"100\" y=\"100\"/><a:ext cx=\"200\" cy=\"100\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"100\" cy=\"100\"/></a:xfrm><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></p:grpSpPr>\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"c\"/></p:nvSpPr><p:spPr><a:xfrm rot=\"1800000\"><a:off x=\"10\" y=\"10\"/><a:ext cx=\"20\" cy=\"20\"/></a:xfrm><a:grpFill/></p:spPr></p:sp></p:grpSp>";
    let s = shapes_of(&mem, g, &theme(), &RelMap::new());
    let x = s[0].xfrm.unwrap();
    // x' = 100 + 10*2 = 120, w = 40; flipH mirrors in the group rect (100..300): 2*200 - (120+40) = 240
    assert_eq!((x.x, x.y, x.cx, x.cy), (240.0, 110.0, 40.0, 20.0));
    assert_eq!(x.flip_h, Some(true));
    assert_eq!(x.rot, Some(-1800000.0));
    assert_eq!(s[0].fill.as_ref().and_then(|f| f.color.as_ref()).map(|c| c.as_str()), Some("#FF0000"));
    assert_eq!(s[0].grp_fill, None);
}

#[test]
fn table_style_bands_and_merges() {
    let mem = Mem::new(&[]);
    let cell = |t: &str| format!("<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>{t}</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>");
    let tbl = format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"9\" name=\"t\"/></p:nvGraphicFramePr><p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"5\" cy=\"5\"/></p:xfrm><a:graphic><a:graphicData><a:tbl><a:tblPr firstRow=\"1\" bandRow=\"1\"><a:tableStyleId>{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}</a:tableStyleId></a:tblPr><a:tblGrid><a:gridCol w=\"10\"/><a:gridCol w=\"10\"/></a:tblGrid>\
<a:tr h=\"5\">{a}{b}</a:tr><a:tr h=\"5\">{c}<a:tc hMerge=\"1\"/></a:tr><a:tr h=\"5\">{d}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
        a = cell("h1"),
        b = cell("h2"),
        c = cell("r1"),
        d = cell("r2")
    )
    .replace("<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>r1</a:t>", "<a:tc gridSpan=\"2\"><a:txBody><a:bodyPr/><a:p><a:r><a:t>r1</a:t>");
    let s = shapes_of(&mem, &tbl, &theme(), &RelMap::new());
    let t = s[0].extras.as_ref().unwrap().table.as_ref().unwrap();
    assert_eq!(t.rows.len(), 3);
    assert_eq!(t.rows[2].cells.len(), 2, "a short row gets empty cells up to the grid");
    assert_eq!(t.rows[1].cells[0].grid_span, Some(2.0));
    // medium style 2 accent 1: first row filled accent1 with white bold text; band 1 below it tinted
    assert_eq!(t.rows[0].cells[0].fill.as_ref().map(|c| c.as_str()), Some("#04617B"));
    assert_eq!(t.rows[0].cells[0].tx_bold, Some(true));
    assert!(t.rows[1].cells[0].fill.is_some());
    assert_eq!(t.h_lines.as_ref().unwrap().rows, 4);
    assert_eq!(t.v_lines.as_ref().unwrap().cols, 3);
    // the inner edge of a merge is not drawn
    assert!(t.v_lines.as_ref().unwrap().get(1, 1).is_none());
}

#[test]
fn symbol_fonts() {
    use uo_dml_render::symbol::{is_symbol_font, symbol_text};
    assert_eq!(symbol_text(Some("Wingdings"), "\u{f0fc}x"), "\u{2713}\u{1f5e1}".replace('\u{1f5e1}', &symbol_text(Some("Wingdings"), "x")));
    assert!(is_symbol_font(Some("Wingdings 2")));
    assert_eq!(symbol_text(None, "abc"), "abc");
}

#[test]
fn m1_the_reader_returns_every_byte() {
    // M1: read_theme, colour resolution, read_shapes (shapes, tables, group, drawing path) drop to the start
    let mem = Mem::new(&[("t.xml", &theme_xml(""))]);
    uo_dml_render::clear_caches();
    uo_alloc::assert_no_leak(|| read_theme(&mem, "t.xml").unwrap());
    uo_alloc::assert_no_leak(|| read_theme(&mem, "missing.xml").unwrap());
    let th = theme();
    uo_alloc::assert_no_leak(|| color("<a:schemeClr val=\"accent1\"><a:lumMod val=\"50000\"/></a:schemeClr>", &th));
    let tree = format!(
        "<p:spTree {NS}><p:sp><p:nvSpPr><p:cNvPr id=\"1\" name=\"s\"/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"1\" y=\"1\"/><a:ext cx=\"2\" cy=\"2\"/></a:xfrm><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></p:spPr>\
<p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr><a:defRPr sz=\"1000\"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:t>x</a:t></a:r></a:p></p:txBody></p:sp>\
<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"2\" name=\"g\"/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"i\"/></p:nvSpPr><p:spPr/></p:sp></p:grpSp></p:spTree>"
    );
    let doc = parse_xml(tree).unwrap();
    let rd = Rd { src: &mem, limits: Limits::default() };
    let rels = RelMap::new();
    uo_alloc::assert_no_leak(|| read_shapes(&rd, doc.root(), &th, &rels).unwrap());
    // an error mid-read (depth limit) frees what was built
    let deep = Rd { src: &mem, limits: Limits { max_group_depth: 0, ..Limits::default() } };
    uo_alloc::assert_no_leak(|| read_shapes(&deep, doc.root(), &th, &rels).unwrap_err());
}
