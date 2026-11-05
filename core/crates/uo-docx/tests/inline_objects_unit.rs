//! Unit tests of frontier R4b: `read::inline_objects` and `read::picture_format` on small inputs
//! whose results were produced by the frozen TS (`tests/inline_objects_unit.cases`, written by
//! `js/unit-cases-r4b.mjs`: `JSON.stringify` of the object runs, the picture formats, the
//! sync results and `blipFxImageEffects`, against `JsonWriter` of the model: values and key
//! insertion order), plus the behaviours that have no TS counterpart (the G20 fallback, the
//! bound of nested groups, the sync of a different kind, the host of the document reads) and the
//! error and `None` paths of every public function.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/r4b_host.rs"]
mod r4b_host;

use r4b_host::{QuirkHost, empty_package};
use std::collections::HashMap;

use uo_docx::DocxLimits;
use uo_docx::js::{JsNumber, JsValue, JsonWriter};
use uo_docx::model::picture::{BlipLum, PicBlipFx};
use uo_docx::model::run::{DrawingTextbox, RunExt, TextboxView};
use uo_docx::model::{MediaStore, Paragraph};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::inline_objects::{
    blip_fx_image_effects, read_drawing_run, read_object_runs, read_vml_runs, sync_image_from_xml, sync_shape_from_xml,
};
use uo_docx::read::picture_format::{dml_color_spec, read_blip, read_dml_color, read_dml_color_element, read_effects, read_picture_format};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::Package;
use uo_opc::xml::{Document as XmlDocument, parse};

type Host<'p> = QuirkHost<'p>;

impl<'p> Host<'p> {
    fn with_len4(pkg: &'p Package, parts: HashMap<String, String>) -> Host<'p> {
        let mut h = QuirkHost::new(pkg, parts);
        h.image_len = Some(4);
        h
    }
}

fn json_of(v: &dyn JsValue, media: &MediaStore) -> String {
    JsonWriter::stringify(v, Some(media)).unwrap_or_else(|| "undefined".into())
}

/// `JSON.stringify(objsOf(runs))`.
fn json_objs(runs: &[RunExt], media: &MediaStore) -> String {
    let mut out: Vec<String> = Vec::new();
    for r in runs {
        let mut s = String::from("{\"text\":\"\u{fffc}\"");
        let fields: [(&str, Option<&dyn JsValue>); 4] = [
            ("image", r.image.as_deref().map(|x| x as &dyn JsValue)),
            ("textbox", r.textbox.as_deref().map(|x| x as &dyn JsValue)),
            ("shape", r.shape.as_deref().map(|x| x as &dyn JsValue)),
            ("group", r.group.as_deref().map(|x| x as &dyn JsValue)),
        ];
        for (k, v) in fields {
            if let Some(v) = v {
                s.push_str(&format!(",\"{k}\":{}", json_of(v, media)));
            }
        }
        s.push('}');
        out.push(s);
    }
    format!("[{}]", out.join(","))
}

struct Case {
    kind: String,
    name: String,
    xml: String,
    want: String,
}

fn cases() -> (Vec<Case>, HashMap<String, HashMap<String, String>>) {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/inline_objects_unit.cases")).unwrap();
    let mut cases = Vec::new();
    let mut parts: HashMap<String, HashMap<String, String>> = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        assert_eq!(f.len(), 4, "{line}");
        if f[0] == "part" {
            parts.entry(f[1].to_owned()).or_default().insert(f[2].to_owned(), f[3].to_owned());
        } else {
            cases.push(Case { kind: f[0].into(), name: f[1].into(), xml: f[2].into(), want: f[3].into() });
        }
    }
    (cases, parts)
}

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

fn blip_input(name: &str) -> PicBlipFx {
    let lum = |bright: Option<f64>, contrast: Option<f64>| BlipLum {
        bright: bright.map(JsNumber::Num),
        contrast: contrast.map(JsNumber::Num),
        ..BlipLum::default()
    };
    match name {
        "lum-pos" => PicBlipFx { lum: Some(lum(Some(10000.0), Some(20000.0))), ..PicBlipFx::default() },
        "lum-neg" => PicBlipFx { lum: Some(lum(Some(-20000.0), Some(-50000.0))), ..PicBlipFx::default() },
        "lum-big" => PicBlipFx { lum: Some(lum(None, Some(250_000.0))), ..PicBlipFx::default() },
        "lum-empty" => PicBlipFx { lum: Some(lum(None, None)), ..PicBlipFx::default() },
        "gray" => PicBlipFx { grayscl: Some(true), ..PicBlipFx::default() },
        "bilevel" => PicBlipFx { bi_level: Some(JsNumber::Num(25000.0)), ..PicBlipFx::default() },
        "all" => PicBlipFx {
            lum: Some(lum(Some(5.0), Some(5.0))),
            grayscl: Some(true),
            bi_level: Some(JsNumber::Num(0.0)),
            ..PicBlipFx::default()
        },
        "nan" => PicBlipFx { lum: Some(lum(Some(f64::NAN), Some(f64::NAN))), ..PicBlipFx::default() },
        _ => PicBlipFx::default(),
    }
}

#[test]
fn cases_of_the_frozen_ts() {
    let (cases, part_sets) = cases();
    assert!(cases.len() > 50);
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut n = 0;
    for c in &cases {
        let parts = match c.name.as_str() {
            "smartart-throws" => part_sets["bad"].clone(),
            "smartart-no-parts" => HashMap::new(),
            _ => part_sets["good"].clone(),
        };
        let mut host = Host::with_len4(&pkg, parts);
        let got = match c.kind.as_str() {
            "fx" => {
                let media = MediaStore::new();
                blip_fx_image_effects(&blip_input(&c.name)).map_or_else(String::new, |fx| json_of(&fx, &media))
            }
            "obj" => {
                let doc = parse(format!("<w:root {NS}>{}</w:root>", c.xml), &DOCX).unwrap();
                let el = doc.root().child_elements().next().unwrap();
                match read_object_runs(el, &mut host) {
                    Ok(runs) => json_objs(&runs, &host.ctx.media),
                    Err(_) => "!throw".into(),
                }
            }
            "fmt" | "fmtany" => {
                let doc = parse(format!("<w:root {NS}>{}</w:root>", c.xml), &DOCX).unwrap();
                let drawing = std::iter::once(doc.root()).chain(doc.root().descendants()).find(|e| e.name() == "w:drawing").unwrap();
                let media = MediaStore::new();
                read_picture_format(drawing.raw().unwrap(), c.kind == "fmtany").map_or_else(|| "undefined".into(), |f| json_of(&f, &media))
            }
            "sync" => {
                let (xml, edits) = c.xml.split_once('\u{1}').unwrap();
                let doc = parse(format!("<w:root {NS}>{xml}</w:root>"), &DOCX).unwrap();
                let el = doc.root().child_elements().next().unwrap();
                let mut raw = el.raw().unwrap().to_owned();
                for e in edits.split('\u{3}') {
                    let (a, b) = e.split_once('\u{2}').unwrap_or_else(|| panic!("{}: {e:?}", c.name));
                    raw = raw.replacen(a, b, 1);
                }
                let mut runs = read_object_runs(el, &mut host).unwrap();
                let run = runs.first_mut().unwrap();
                if let Some(i) = run.image.as_mut() {
                    i.raw_drawing_xml = Some(raw.as_str().into());
                    sync_image_from_xml(i);
                } else {
                    if let Some(o) = run.textbox.as_mut() {
                        o.raw_drawing_xml = Some(raw.as_str().into());
                    } else if let Some(o) = run.shape.as_mut() {
                        o.raw_drawing_xml = Some(raw.as_str().into());
                    } else if let Some(o) = run.group.as_mut() {
                        o.raw_drawing_xml = Some(raw.as_str().into());
                    }
                    sync_shape_from_xml(run);
                }
                json_objs(&runs, &host.ctx.media)
            }
            other => panic!("unknown kind {other}"),
        };
        assert_eq!(got, c.want, "{} {}", c.kind, c.name);
        n += 1;
    }
    assert_eq!(n, cases.len());
}

fn drawing_doc(inner: &str) -> XmlDocument {
    parse(format!("<w:drawing {NS}><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/>{inner}</wp:inline></w:drawing>"), &DOCX).unwrap()
}

const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";

fn textbox_of(inner: &str, host: &mut Host<'_>) -> Box<DrawingTextbox> {
    let doc = drawing_doc(&format!(
        "<a:graphic><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic>"
    ));
    let run = read_drawing_run(doc.root(), doc.root().raw(), host).unwrap().unwrap();
    run.textbox.unwrap()
}

#[test]
fn the_g20_fallback_reads_the_content_as_plain_paragraphs() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut host = Host::with_len4(&pkg, HashMap::new());
    host.nested_left = Some(0);
    let tb = textbox_of("<w:p><w:r><w:t>um</w:t><w:tab/><w:t>dois</w:t></w:r></w:p><w:p/>", &mut host);
    assert_eq!(tb.view, TextboxView::Synced);
    let paras: Vec<&Paragraph> = tb.content.flat_paragraphs();
    assert_eq!(paras.len(), 2);
    assert!(paras.first().unwrap().runs.first().unwrap().text.eq_str("um\tdois"));
    assert!(paras.get(1).unwrap().runs.is_empty());
    // the fallback did not enter: nothing to leave
    assert_eq!(host.nested_left, Some(0));
    // with room it is the host's reader, and the guard is left again
    host.nested_left = Some(1);
    let tb = textbox_of("<w:p><w:r><w:t>um</w:t></w:r></w:p>", &mut host);
    assert_eq!(tb.content.flat_paragraphs().len(), 1);
    assert_eq!(host.nested_left, Some(1));
}

#[test]
fn an_empty_text_box_content_is_the_phantom_view() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut host = Host::with_len4(&pkg, HashMap::new());
    let tb = textbox_of("", &mut host);
    assert_eq!(tb.view, TextboxView::Phantom);
    assert!(tb.content.flat_paragraphs().is_empty());
}

fn group_xml(depth: usize) -> String {
    let leaf = "<wps:wsp><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"635\" cy=\"635\"/></a:xfrm><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></wps:spPr></wps:wsp>";
    let mut s = leaf.to_owned();
    for _ in 0..depth {
        s = format!(
            "<wpg:grpSp><wpg:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"635\" cy=\"635\"/></a:xfrm></wpg:grpSpPr>{leaf}{s}</wpg:grpSp>"
        );
    }
    format!("<a:graphic><a:graphicData uri=\"x\"><wpg:wgp><wpg:grpSpPr/>{s}</wpg:wgp></a:graphicData></a:graphic>")
}

fn depth_of(g: &uo_docx::model::run::DrawingGroup) -> usize {
    let mut d = 1;
    let mut cur = g;
    while let Some(next) = cur.items.iter().find_map(|i| i.run.ext().and_then(|e| e.group.as_deref())) {
        d += 1;
        cur = next;
    }
    d
}

#[test]
fn nested_groups_are_bounded_by_max_nesting() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    // deep nesting does not use the native stack: the 64th nested group and the rest are dropped
    let doc = drawing_doc(&group_xml(200));
    let mut host = Host::with_len4(&pkg, HashMap::new());
    let run = read_drawing_run(doc.root(), None, &mut host).unwrap().unwrap();
    assert_eq!(depth_of(run.group.as_deref().unwrap()), 65);
    // a shallow one is read whole: 3 nested groups under the first
    let doc = drawing_doc(&group_xml(3));
    let run = read_drawing_run(doc.root(), None, &mut host).unwrap().unwrap();
    assert_eq!(depth_of(run.group.as_deref().unwrap()), 4);
    // the guard is balanced
    assert_eq!(host.ctx.nesting(), 0);
    let limits = DocxLimits { max_nesting: 2, ..DocxLimits::DEFAULT };
    let mut host = Host::with_len4(&pkg, HashMap::new());
    host.ctx = ReadCtx::new(&pkg, &ReadOptions { limits, ..ReadOptions::default() });
    let doc = drawing_doc(&group_xml(5));
    let run = read_drawing_run(doc.root(), None, &mut host).unwrap().unwrap();
    assert_eq!(depth_of(run.group.as_deref().unwrap()), 3);
}

#[test]
fn raw_is_the_callers_and_empty_raw_is_none() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut host = Host::with_len4(&pkg, HashMap::new());
    let doc = drawing_doc(&format!(
        "<a:graphic><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:spPr><a:prstGeom prst=\"rect\"/><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></wps:spPr></wps:wsp></a:graphicData></a:graphic>"
    ));
    let with = read_drawing_run(doc.root(), Some("<raw/>"), &mut host).unwrap().unwrap();
    assert_eq!(with.shape.unwrap().raw_drawing_xml.as_deref(), Some("<raw/>"));
    for raw in [None, Some("")] {
        let without = read_drawing_run(doc.root(), raw, &mut host).unwrap().unwrap();
        assert!(without.shape.unwrap().raw_drawing_xml.is_none());
    }
}

#[test]
fn sync_of_a_different_kind_and_without_raw_changes_nothing() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut host = Host::with_len4(&pkg, HashMap::new());
    let doc = drawing_doc(&format!(
        "<a:graphic><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent><w:p/></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic>"
    ));
    let mut run = read_drawing_run(doc.root(), doc.root().raw(), &mut host).unwrap().unwrap();
    // a raw text that now reads as a plain shape: the TS mixes the keys of two kinds, kept as it is here
    let shape_raw = format!(
        "<w:drawing {NS}><wp:inline><wp:extent cx=\"1\" cy=\"1\"/><a:graphic><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></wps:spPr></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>"
    );
    run.textbox.as_mut().unwrap().raw_drawing_xml = Some(shape_raw.as_str().into());
    let before = json_objs(std::slice::from_ref(&run), &host.ctx.media);
    sync_shape_from_xml(&mut run);
    assert_eq!(json_objs(std::slice::from_ref(&run), &host.ctx.media), before);
    // no raw, an empty raw, text that does not parse: nothing
    let mut run = read_drawing_run(doc.root(), None, &mut host).unwrap().unwrap();
    let before = json_objs(std::slice::from_ref(&run), &host.ctx.media);
    sync_shape_from_xml(&mut run);
    assert_eq!(json_objs(std::slice::from_ref(&run), &host.ctx.media), before);
    run.textbox.as_mut().unwrap().raw_drawing_xml = Some("<w:drawing><".into());
    sync_shape_from_xml(&mut run);
    assert_eq!(run.textbox.as_ref().unwrap().raw_drawing_xml.as_deref(), Some("<w:drawing><"));
    let mut image = uo_docx::model::run::InlineImage::default();
    sync_image_from_xml(&mut image);
    image.raw_drawing_xml = Some("<not xml".into());
    sync_image_from_xml(&mut image);
    assert_eq!(image.raw_drawing_xml.as_deref(), Some("<not xml"));
}

#[test]
fn the_dispatch_gives_nothing_for_other_elements() {
    let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut host = Host::with_len4(&pkg, HashMap::new());
    for xml in
        ["<w:t>x</w:t>", "<mc:AlternateContent/>", "<mc:AlternateContent><mc:Fallback/></mc:AlternateContent>", "<w:pict/>", "<w:object/>"]
    {
        let doc = parse(xml, &DOCX).unwrap();
        assert!(read_object_runs(doc.root(), &mut host).unwrap().is_empty(), "{xml}");
    }
    let doc = parse("<w:pict><o:lock/><w10:anchorlock/><v:shapetype/></w:pict>", &DOCX).unwrap();
    assert!(read_vml_runs(doc.root(), false, &mut host).unwrap().is_empty());
}

#[test]
fn picture_format_pieces() {
    let doc = parse(
        "<a:effectLst><a:glow rad=\"5\"><a:srgbClr val=\"ff0000\"><a:alpha val=\"50000\"/></a:srgbClr></a:glow><a:foo/></a:effectLst>",
        &DOCX_POS,
    )
    .unwrap();
    let media = MediaStore::new();
    assert_eq!(
        json_of(&read_effects(Some(doc.root())), &media),
        r#"{"glow":{"rad":5,"color":{"kind":"srgbClr","val":"ff0000","mods":[["alpha",50000]]}},"otherRaw":["<a:foo/>"]}"#
    );
    assert_eq!(json_of(&read_effects(None), &media), "{}");
    let doc =
        parse("<a:blip><a:alphaModFix/><a:biLevel thresh=\"x\"/><a:grayscl/><a:duotone><a:scrgbClr/></a:duotone></a:blip>", &DOCX).unwrap();
    assert_eq!(
        json_of(&read_blip(Some(doc.root())), &media),
        r#"{"alphaModFix":100000,"biLevel":50000,"grayscl":true,"duotone":[{"kind":"srgbClr","val":"000000"},{"kind":"srgbClr","val":"000000"}]}"#
    );
    assert_eq!(json_of(&read_blip(None), &media), "{}");
    // the first colour child; a kind the model does not know gives black
    let doc = parse("<a:solidFill><a:foo/><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/><a:hslClr/></a:solidFill>", &DOCX).unwrap();
    assert_eq!(json_of(&read_dml_color(Some(doc.root())).unwrap(), &media), r#"{"kind":"sysClr","val":"window","lastClr":"FFFFFF"}"#);
    let doc = parse("<a:solidFill><a:hslClr/></a:solidFill>", &DOCX).unwrap();
    assert_eq!(json_of(&read_dml_color(Some(doc.root())).unwrap(), &media), r#"{"kind":"srgbClr","val":"000000"}"#);
    assert!(read_dml_color(None).is_none());
    assert!(read_dml_color(Some(parse("<a:solidFill/>", &DOCX).unwrap().root())).is_none());
    // not a drawing
    assert!(read_picture_format("", false).is_none());
    assert!(read_picture_format("<w:drawing/>", true).is_none());
}

#[test]
fn the_host_of_the_document_reads_is_the_read_context() {
    use uo_docx::read::inline_objects::with_context;
    use uo_docx::read::paragraph::{BlocksMode, ReadContext};
    let pkg = empty_package();
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions::default());
    let doc = drawing_doc(&format!(
        "<a:graphic><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:spPr/><wps:txbx><w:txbxContent><w:p><w:r><w:t>Olá</w:t></w:r></w:p><w:p/></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic>"
    ));
    {
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").unwrap();
        // without a block reader (the comments): the `w:p` children are read as paragraphs
        let run = with_context::read_drawing_run(doc.root(), &mut rc).unwrap().unwrap();
        assert!(run.text.eq_str("\u{fffc}"));
        let tb = run.ext().unwrap().textbox.as_deref().unwrap();
        assert_eq!(tb.view, TextboxView::ParasOnly);
        let paras = tb.content.flat_paragraphs();
        assert_eq!(paras.len(), 2);
        assert!(paras.first().unwrap().runs.first().unwrap().text.eq_str("Olá"));
        // a VML picture whose relationship does not resolve is dropped
        let pict =
            parse("<w:pict><v:shape style=\"width:10pt;height:5pt\"><v:imagedata r:id=\"rId9\"/></v:shape></w:pict>", &DOCX).unwrap();
        assert!(with_context::read_vml_runs(pict.root(), &mut rc, false).unwrap().is_empty());
        // with the block reader of the full-document read (`fulldoc::read_textbox_content`): the content is a
        // story of blocks, and the guard is balanced
        rc.blocks = BlocksMode::Blocks;
        let run = with_context::read_drawing_run(doc.root(), &mut rc).unwrap().unwrap();
        let tb = run.ext().unwrap().textbox.as_deref().unwrap();
        assert_eq!(tb.view, TextboxView::Synced);
        assert_eq!(tb.content.paras.len(), 2);
    }
    assert_eq!(ctx.nesting(), 0);
}

#[test]
fn colour_spec_of_a_picture_colour() {
    let spec = |xml: &str| {
        let doc = parse(xml, &DOCX).unwrap();
        dml_color_spec(&read_dml_color_element(doc.root()))
    };
    assert_eq!(
        spec("<a:srgbClr val=\"ab12cd\"><a:alpha val=\"50000\"/><a:lumMod val=\"75000\"/><a:gray/></a:srgbClr>"),
        ("AB12CD|lumMod=75000,gray=0".to_owned(), 0.5)
    );
    assert_eq!(spec("<a:schemeClr val=\"tx1\"/>"), ("@dk1".to_owned(), 1.0));
    assert_eq!(spec("<a:schemeClr val=\"accent2\"><a:tint val=\"60000\"/></a:schemeClr>"), ("@accent2|tint=60000".to_owned(), 1.0));
    assert_eq!(spec("<a:prstClr val=\"white\"/>"), ("FFFFFF".to_owned(), 1.0));
    assert_eq!(spec("<a:prstClr val=\"nope\"/>"), ("000000".to_owned(), 1.0));
    assert_eq!(spec("<a:sysClr val=\"window\"/>"), ("FFFFFF".to_owned(), 1.0));
    assert_eq!(spec("<a:sysClr val=\"windowText\" lastClr=\"112233\"/>"), ("112233".to_owned(), 1.0));
    // a name Object.prototype has prints the member (the TS keeps the member itself when no modifier follows)
    assert_eq!(spec("<a:schemeClr val=\"toString\"/>").0, "@function toString() { [native code] }");
    assert_eq!(spec("<a:prstClr val=\"constructor\"><a:lumMod val=\"1\"/></a:prstClr>").0, "function Object() { [native code] }|lumMod=1");
}
