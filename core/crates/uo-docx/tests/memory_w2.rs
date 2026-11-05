//! Memory gate of the D1b frontier W2 (`write::emit::object`): M1 — every public function returns the
//! live bytes exactly, outputs dropped — over the corpus shapes (the pure text functions, ok and error
//! paths), over a `Writer` (new picture, OLE object, rebuilt text box/shape/group, chains, fresh ids,
//! the depth bound), and over whole saves with the named edits of `js/capture-object-edits.mjs`; M3
//! numbers of a save with every shape edited (printed with `--nocapture`, budget asserted).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::{Path, PathBuf};

use uo_alloc::{assert_no_leak, measure};
use uo_docx::error::DocxError;
use uo_docx::ids::DrawingId;
use uo_docx::js::{JsNumber, Str};
use uo_docx::model::media::MediaRef;
use uo_docx::model::picture::PicColor;
use uo_docx::model::run::{DrawingGroup, DrawingShape, DrawingTextbox, ImageLayer, InlineImage, OleData, Run};
use uo_docx::model::story::Block;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::ctx::{DEFAULT_SHAPE_THEME, WriteCtx, Writer};
use uo_docx::write::emit::object::{
    MAX_REBUILT_BOX_DEPTH, NewTextboxOptions, ShapeChange, ShapeFill, StoryChange, TextboxBodyPatch, VmlOptions, begin_shape_context,
    body_pr_xml, default_textbox_body, doc_pr_name, drawing_xml, edited_group_text, edited_group_xml, edited_shape_xml, fresh_drawing_ids,
    fresh_ids_text, hidden_line_xml, new_textbox_drawing_xml, new_textbox_fill, new_textbox_line, ole_object_xml, once_shape,
    patch_drawing_geometry, patch_shape, picture_xml, read_shape_format, shape_color, shape_fill_xml, shape_out, style_refs_xml,
    textbox_alternate_content, txbx_content_of, vml_color_of, vml_fallback, vml_pt,
};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_opc::{Limits, Package};
use uo_testkit::repo_root;

mod w2_support;
use w2_support::{EDITS, apply};

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "docx") {
                out.push(p);
            }
        }
    }
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("write/packages/docx-layout/test/oracle"), &mut out);
    walk(&root.join("write/packages/docx-core/test/fixtures"), &mut out);
    out
}

/// The drawings of the parity inputs (a text box, a shape, a picture-less group…): the first `n`.
fn drawings(n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/shape-xml.inputs")).unwrap();
    text.lines().filter(|l| !l.starts_with('#')).step_by(7).take(n).map(unescape).collect()
}

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

#[test]
fn m1_text_functions_return_the_memory() {
    let all = drawings(40);
    let theme: indexmap::IndexMap<Str, Str> = DEFAULT_SHAPE_THEME.iter().map(|&(k, v)| (Str::from(k), Str::from(v))).collect();
    let color = |c: &PicColor| shape_color(&theme, c);
    for raw in &all {
        assert_no_leak(|| {
            let f = read_shape_format(raw);
            black_box(
                f.as_ref().map(|f| (shape_fill_xml(&f.fill), f.style.as_ref().map(style_refs_xml), f.body.as_ref().map(body_pr_xml))),
            );
            let o = VmlOptions { color: &color, next: Some("Text Box 2"), spid: 4097, outline: None };
            black_box(vml_fallback(raw, "<w:p/>", &o).ok());
            black_box(textbox_alternate_content(raw, "<w:p/>", &o).ok());
            black_box(txbx_content_of(raw));
            black_box(doc_pr_name(Some(raw)));
            black_box(
                patch_drawing_geometry(
                    raw,
                    &uo_docx::write::emit::object::Geometry {
                        width_twips: n(2000.0),
                        height_twips: n(900.0),
                        offset_x_twips: Some(n(5.0)),
                        offset_y_twips: Some(n(-5.0)),
                    },
                )
                .ok(),
            );
            black_box(fresh_ids_text(raw, &mut 0.0));
            black_box(edited_group_text(raw, raw));
            black_box(hidden_line_xml(f.as_ref().and_then(|f| f.base.line.as_ref().and_then(|l| l.as_val())).map(|l| &**l)));
            let ch = ShapeChange {
                fill: Some(ShapeFill::None),
                outline: Some(None),
                body: Some(TextboxBodyPatch { l_ins: Some(1.0), ..TextboxBodyPatch::default() }),
                story: Some(StoryChange::Txbx { id: Some(3.0), content: Some("<w:p/>".into()) }),
                ..ShapeChange::default()
            };
            black_box(patch_shape(raw, &ch).ok());
        });
    }
    // error paths: text that is not a drawing
    for bad in ["", "<w:drawing>", "<w:drawing><x/></w:drawing>", "<w:drawing><wp:inline/></w:drawing>"] {
        assert_no_leak(|| {
            let r = patch_shape(bad, &ShapeChange { fill: Some(ShapeFill::None), ..ShapeChange::default() });
            black_box(r.is_err());
            black_box(read_shape_format(bad));
        });
    }
    assert_no_leak(|| {
        black_box(new_textbox_drawing_xml(&NewTextboxOptions { content: "x".into(), ..NewTextboxOptions::default() }));
        black_box((new_textbox_line(), new_textbox_fill(), default_textbox_body()));
        black_box((vml_pt(12700.0), vml_pt(f64::NAN), vml_color_of("aabbcc"), vml_color_of("")));
        black_box(shape_color(&theme, &PicColor { kind: "schemeClr".into(), val: "accent1".into(), ..PicColor::default() }).ok());
        black_box(shape_color(&theme, &PicColor { kind: "prstClr".into(), val: "constructor".into(), ..PicColor::default() }).is_err());
    });
}

/// A document, its package and a media payload, as `Writer::new` needs them.
fn open_doc() -> (Package, uo_docx::model::Document) {
    let files = corpus();
    let bytes = std::fs::read(&files[3]).unwrap();
    let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
    (pkg, doc)
}

fn image_run(data: MediaRef, raw: Option<&str>) -> Run {
    let mut r = Run { text: "\u{fffc}".into(), ..Run::default() };
    r.ext_mut().image = Some(Box::new(InlineImage {
        data,
        ext: "png".into(),
        width_twips: n(1500.0),
        height_twips: n(700.0),
        raw_drawing_xml: raw.map(Str::from),
        ..InlineImage::default()
    }));
    r
}

const PIC_RAW: &str = "<w:drawing><wp:inline><wp:extent cx=\"1\" cy=\"2\"/><wp:docPr id=\"7\" name=\"a\"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed=\"uw:media\"/><a:blip r:embed=\"uw:layer\"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx=\"1\" cy=\"2\"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>";

#[test]
fn m1_writer_functions_return_the_memory() {
    let shapes = drawings(60);
    let tb_raw = shapes.iter().find(|r| r.contains("txBox=\"1\"")).cloned().unwrap();
    let sh_raw = shapes.iter().find(|r| !r.contains("txBox=\"1\"")).cloned().unwrap();
    assert_no_leak(|| {
        let (mut pkg, mut doc) = open_doc();
        let data = MediaRef(doc.media.add(b"\x89PNG-payload"));
        let layer = MediaRef(doc.media.add(b"\x89PNG-layer"));
        let ole_data = MediaRef(doc.media.add(b"PK-ole"));
        let mut fresh = 0.0;
        let mut w = Writer {
            doc: &doc,
            pkg: &mut pkg,
            cx: WriteCtx::new(Limits::DEFAULT, SigMode::Canonical),
            fresh_id: &mut fresh,
            lossless: None,
        };
        // a new picture and an OLE object, queued in `mediaOut`
        let plain = image_run(data, None);
        black_box(drawing_xml(&mut w, &plain).unwrap());
        let mut ole = image_run(data, None);
        if let Some(i) = ole.ext_mut().image.as_deref_mut() {
            i.ole = Some(Box::new(OleData { prog_id: "Excel.Sheet.12".into(), data: ole_data, ext: "xlsx".into(), ..OleData::default() }));
        }
        black_box(ole_object_xml(&mut w, &ole).unwrap());
        // a picture with raw XML, the markers resolved (without sources: `mediaOut`)
        let mut with_raw = image_run(data, Some(PIC_RAW));
        if let Some(i) = with_raw.ext_mut().image.as_deref_mut() {
            i.layer = Some(ImageLayer { data: layer, ext: "wdp".into(), ..ImageLayer::default() });
        }
        let img = with_raw.ext().and_then(|e| e.image.as_deref()).unwrap();
        black_box(picture_xml(&mut w, img, None).unwrap());
        black_box(picture_xml(&mut w, img, Some(PIC_RAW)).unwrap());
        // rebuilt text box, shape and group
        let mut tb = Run { text: "\u{fffc}".into(), ..Run::default() };
        tb.ext_mut().textbox = Some(Box::new(DrawingTextbox {
            raw_drawing_xml: Some(Str::from(tb_raw.as_str())),
            width_twips: n(3000.0),
            height_twips: n(1000.0),
            edited: Some(true),
            drawing: DrawingId(1),
            ..DrawingTextbox::default()
        }));
        black_box(edited_shape_xml(&mut w, &tb).unwrap());
        let mut vml_tb = tb.clone();
        if let Some(t) = vml_tb.ext_mut().textbox.as_deref_mut() {
            t.vml = Some(true);
            t.drawing = DrawingId(2);
        }
        black_box(edited_shape_xml(&mut w, &vml_tb).unwrap());
        w.cx.vml_only = true;
        black_box(edited_shape_xml(&mut w, &tb).unwrap());
        w.cx.vml_only = false;
        let mut sh = Run { text: "\u{fffc}".into(), ..Run::default() };
        sh.ext_mut().shape = Some(Box::new(DrawingShape {
            raw_drawing_xml: Some(Str::from(sh_raw.as_str())),
            width_twips: n(100.0),
            height_twips: n(100.0),
            edited: Some(true),
            drawing: DrawingId(3),
            ..DrawingShape::default()
        }));
        let shape_xml = edited_shape_xml(&mut w, &sh).unwrap();
        black_box(once_shape(&mut w, DrawingId(3), shape_xml.clone()).unwrap());
        black_box(once_shape(&mut w, DrawingId(3), shape_xml).unwrap());
        black_box(fresh_drawing_ids(&mut w, &tb_raw).unwrap());
        black_box(edited_group_xml(&mut w, "<mc:AlternateContent><w:drawing/><v:group/></mc:AlternateContent>", &sh_raw).unwrap());
        let _ = DrawingGroup::default();
        // chains over the blocks of the document
        let blocks: Vec<Block<'_>> = doc.body.blocks().collect();
        begin_shape_context(&mut w, &blocks).unwrap();
        // shape_out with the VML only
        let theme = w.cx.shape_theme.clone();
        let color = |c: &PicColor| shape_color(&theme, c);
        let o = VmlOptions { color: &color, next: None, spid: 4100, outline: None };
        black_box(shape_out(&w, &tb_raw, "<w:p/>", &o, true).unwrap());
        // error paths: a run without its object, a picture without raw XML, the depth bound
        let bare = Run::default();
        assert!(drawing_xml(&mut w, &bare).is_err());
        assert!(ole_object_xml(&mut w, &plain).is_err());
        let norw = image_run(data, None);
        assert!(picture_xml(&mut w, norw.ext().and_then(|e| e.image.as_deref()).unwrap(), None).is_err());
        w.cx.shape_depth = MAX_REBUILT_BOX_DEPTH;
        assert!(matches!(edited_shape_xml(&mut w, &tb), Err(DocxError::Write(_))));
        assert_eq!(w.cx.shape_depth, MAX_REBUILT_BOX_DEPTH);
    });
}

fn save_with_edit(bytes: &[u8], edit: &str, sources: bool) -> Result<usize, DocxError> {
    let mut pkg = Package::open(bytes, &Limits::DEFAULT, &OpenOptions::default())?;
    let mut doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() })?;
    let mut state = install(&mut doc, SigMode::Canonical);
    let hit = apply(&mut doc, edit);
    save(&mut pkg, &doc, &mut state, DirtyHf::All)?;
    Ok(hit + pkg.save(&SaveOptions::default())?.len())
}

#[test]
fn m1_edited_saves_return_the_memory() {
    let files = corpus();
    // documents with objects: the first of each edit that touches something
    let mut n_runs = 0;
    for edit in EDITS {
        let mut found = 0;
        for f in files.iter().step_by(3) {
            let bytes = std::fs::read(f).unwrap();
            let touched = {
                let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
                let Ok(mut doc) = read_document(&pkg, &ReadOptions::default()) else { continue };
                apply(&mut doc, edit) > 0
            };
            if !touched {
                continue;
            }
            for sources in [true, false] {
                assert_no_leak(|| black_box(save_with_edit(&bytes, edit, sources).is_ok()));
                n_runs += 1;
            }
            found += 1;
            if found == 2 {
                break;
            }
        }
    }
    println!("M1: {n_runs} saves with an edit, 0 B left each");
    assert!(n_runs > 20);
}

/// The depth bound (W-D15) returns the memory too: a package of 63 nested VML text boxes saved without
/// sources is refused with an error, not written in 2^63 steps.
#[test]
fn m1_nested_boxes_bound() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/full.inputs")).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut on = false;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            on = key.contains("nest-vml-63");
        } else if on && let Some((path, data)) = line.split_once('\t') {
            parts.push((path.to_owned(), unescape(data).into_bytes()));
        }
    }
    assert!(!parts.is_empty());
    let build = || {
        let ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
        let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
        let base = vec![("[Content_Types].xml".to_owned(), ct.as_bytes().to_vec()), ("_rels/.rels".to_owned(), rels.as_bytes().to_vec())];
        let mut pkg = Package::from_parts(base, &Limits::DEFAULT, &OpenOptions::default());
        for (p, d) in &parts {
            pkg.set_part(p.clone(), d.clone());
        }
        pkg
    };
    assert_no_leak(|| {
        let mut pkg = build();
        let mut doc = read_document(&pkg, &ReadOptions { sources: false, ..ReadOptions::default() }).unwrap();
        let mut state = install(&mut doc, SigMode::Canonical);
        let t = std::time::Instant::now();
        let r = save(&mut pkg, &doc, &mut state, DirtyHf::All);
        assert!(matches!(r, Err(DocxError::Write(m)) if m.contains("nested deeper than 16")), "{r:?}");
        assert!(t.elapsed().as_secs() < 20);
    });
}

/// M3: a save of the document with the most shapes with every shape edited: allocated bytes per byte
/// of the XML written (budget: 3 × the XML + 256 KiB per rebuilt shape + 64 KiB: a rebuilt shape parses its
/// drawing three times and writes the content twice, the VML fallback beside the wps).
#[test]
fn m3_edited_shapes_budget() {
    let files = corpus();
    let mut best: Option<(usize, PathBuf)> = None;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let Ok(mut doc) = read_document(&pkg, &ReadOptions::default()) else { continue };
        let hit = apply(&mut doc, "shapes");
        if best.as_ref().is_none_or(|b| hit > b.0) {
            best = Some((hit, f.clone()));
        }
    }
    let (hit, f) = best.unwrap();
    let bytes = std::fs::read(&f).unwrap();
    let mut pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
    let mut state = install(&mut doc, SigMode::Canonical);
    {
        let mut pkg0 = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let mut doc0 = read_document(&pkg0, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        let mut st0 = install(&mut doc0, SigMode::Canonical);
        let ((), u0) = measure(|| save(&mut pkg0, &doc0, &mut st0, DirtyHf::All).unwrap());
        println!("M3 same save without the edit: allocated {} B", u0.allocated);
    }
    apply(&mut doc, "shapes");
    let ((), u) = measure(|| save(&mut pkg, &doc, &mut state, DirtyHf::All).unwrap());
    let written = pkg.part("word/document.xml").unwrap().map_or(0, <[u8]>::len);
    println!(
        "M3 {} ({hit} shapes edited): document.xml {written} B, allocated {} B ({:.2}x), peak {} B",
        f.file_name().unwrap().to_string_lossy(),
        u.allocated,
        u.allocated as f64 / written.max(1) as f64,
        u.peak_above_start
    );
    assert!(
        u.allocated <= 3 * written as u64 + hit as u64 * (256 << 10) + (64 << 10),
        "{} B allocated for {written} B, {hit} shapes",
        u.allocated
    );
}
