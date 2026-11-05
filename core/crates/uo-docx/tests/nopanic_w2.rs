//! No panic and linear time of the D1b frontier W2 (`write::emit::object`) on hostile input: the corpus
//! shapes with seeded mutations (truncations, deleted and swapped characters, injected tag soup) through
//! every text function; 2–3.5 MB hostile texts through the matchers (each under a few seconds); saves of
//! documents with the named edits whose raw drawings were then mangled; a `Writer` with models that
//! hold garbage (raw drawings that are not XML, objects without their payload).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use uo_docx::ids::DrawingId;
use uo_docx::js::{JsNumber, Str};
use uo_docx::model::picture::PicColor;
use uo_docx::model::run::{DrawingShape, DrawingTextbox, Run};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::ctx::{DEFAULT_SHAPE_THEME, WriteCtx, Writer};
use uo_docx::write::emit::object::{
    Geometry, ShapeChange, ShapeFill, StoryChange, StylePreset, TextboxBodyPatch, VmlOptions, doc_pr_name, edited_group_text,
    edited_group_xml, edited_shape_xml, fresh_drawing_ids, fresh_ids_text, patch_drawing_geometry, patch_shape, read_shape_format,
    shape_color, textbox_alternate_content, txbx_content_of, vml_fallback,
};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::package::OpenOptions;
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

fn drawings() -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/shape-xml.inputs")).unwrap();
    text.lines().filter(|l| !l.starts_with('#')).map(unescape).collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const SOUP: &[&str] = &[
    "<wp:docPr ",
    "<a:ext ",
    "cx=\"1\" ",
    "/>",
    ">",
    "\"",
    "<wps:wsp>",
    "</wps:wsp>",
    "<wp:anchor ",
    "</wp:anchor>",
    "<a:ln ",
    "<a:solidFill>",
    "</a:solidFill>",
    "<wps:bodyPr ",
    "<wps:txbx>",
    "<w:txbxContent>",
    "</w:txbxContent>",
    "<a:extLst>",
    "<a:ext uri=\"{91240B29-F687-4F45-9708-019B960494DF}\">",
    "name=\"",
    "id=\"",
    "<v:group ",
    "style=\"",
    "é",
    "😀",
    "\u{0}",
    "&amp;",
    "&",
    "<",
];

/// A seeded mutation of `s` at char boundaries.
fn mutate(s: &str, rng: &mut Rng) -> String {
    let cuts: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    if cuts.is_empty() {
        return SOUP[rng.below(SOUP.len())].to_owned();
    }
    let at = cuts[rng.below(cuts.len())];
    match rng.below(5) {
        0 => s[..at].to_owned(),
        1 => {
            let end = cuts.get(cuts.iter().position(|&c| c == at).unwrap() + 1 + rng.below(40)).copied().unwrap_or(s.len());
            format!("{}{}", &s[..at], &s[end..])
        }
        2 => format!("{}{}{}", &s[..at], SOUP[rng.below(SOUP.len())], &s[at..]),
        3 => s.replace('>', if rng.below(2) == 0 { "" } else { "<" }),
        _ => format!("{}{}", SOUP[rng.below(SOUP.len())], &s[at..]),
    }
}

fn change_sets() -> Vec<ShapeChange> {
    vec![
        ShapeChange { fill: Some(ShapeFill::None), outline: Some(None), ..ShapeChange::default() },
        ShapeChange {
            body: Some(TextboxBodyPatch {
                l_ins: Some(5.0),
                upright: Some(false),
                vert: Some("vert".into()),
                ..TextboxBodyPatch::default()
            }),
            story: Some(StoryChange::Txbx { id: Some(2.0), content: None }),
            ..ShapeChange::default()
        },
        ShapeChange {
            style_preset: Some(StylePreset { sp_pr: "<a:noFill/>".into(), style: "<wps:style/>".into() }),
            story: Some(StoryChange::Linked { id: 1.0, seq: 2.0 }),
            style: Some(None),
            ..ShapeChange::default()
        },
    ]
}

#[test]
fn mutated_drawings_do_not_panic() {
    let all = drawings();
    let theme: indexmap::IndexMap<Str, Str> = DEFAULT_SHAPE_THEME.iter().map(|&(k, v)| (Str::from(k), Str::from(v))).collect();
    let color = |c: &PicColor| shape_color(&theme, c);
    let changes = change_sets();
    let mut rng = Rng(0x2d1b_57a9);
    let (mut ok, mut err) = (0, 0);
    for raw in all.iter().step_by(3) {
        for _ in 0..12 {
            let m = mutate(raw, &mut rng);
            let o = VmlOptions { color: &color, next: Some("x"), spid: 1, outline: None };
            for r in [vml_fallback(&m, "<w:p/>", &o).map(drop), textbox_alternate_content(&m, "", &o).map(drop)] {
                if r.is_ok() { ok += 1 } else { err += 1 }
            }
            std::hint::black_box(read_shape_format(&m));
            std::hint::black_box(txbx_content_of(&m));
            std::hint::black_box(doc_pr_name(Some(&m)));
            let geo = Geometry {
                width_twips: JsNumber::Num(1.0),
                height_twips: JsNumber::Null,
                offset_x_twips: Some(JsNumber::Num(f64::NAN)),
                offset_y_twips: Some(JsNumber::Num(-1e300)),
            };
            patch_drawing_geometry(&m, &geo).unwrap();
            std::hint::black_box(fresh_ids_text(&m, &mut 0.0));
            std::hint::black_box(edited_group_text(&m, &m));
            std::hint::black_box(edited_group_text(raw, &m));
            for ch in &changes {
                if patch_shape(&m, ch).is_ok() { ok += 1 } else { err += 1 }
            }
        }
    }
    println!("mutated drawings: {ok} ok, {err} refused, no panic");
    assert!(ok > 100 && err > 100);
}

#[test]
fn hostile_texts_are_linear() {
    let n = 100_000;
    let cases: Vec<(&str, String)> = vec![
        ("docpr without >", "<wp:docPr name=\"a\" ".repeat(n)),
        ("docpr with names, no closing quote", "<wp:docPr name=\"".repeat(n)),
        ("docpr names without >", format!("<wp:docPr {}", "name=\"x\" ".repeat(n))),
        ("a:ext without />", "<a:ext cx=\"1\" ".repeat(n)),
        ("a:ext with >", "<a:ext cx=\"1\">".repeat(n)),
        ("wp:extent without />", "<wp:extent cx=\"1\">".repeat(n)),
        ("positionH unclosed", "<wp:positionH><wp:posOffset>1</wp:posOffset>".repeat(n)),
        ("positionH no >", "<wp:positionH ".repeat(n)),
        ("v:group no >", "<mc:AlternateContent><v:group style=\"".repeat(n)),
        ("style quotes", format!("<mc:AlternateContent><v:group style=\"{}\">", "visibility:a;".repeat(n))),
        ("anchor attrs no quote", " wp14:anchorId=\"".repeat(n)),
        ("txbxContent unclosed", "<w:txbxContent>".repeat(n)),
        ("txbxContent no >", "<w:txbxContent ".repeat(n)),
        ("drawing unclosed", "<w:drawing>".repeat(n)),
        ("fallback", "<mc:Fallback>".repeat(n)),
        ("ids without id", "<wp:docPr a=\"1\">".repeat(n)),
        ("ids no >", "<wp:docPr id=\"x\" ".repeat(n)),
    ];
    for (name, text) in cases {
        let t = Instant::now();
        let geo = Geometry {
            width_twips: JsNumber::Num(10.0),
            height_twips: JsNumber::Num(20.0),
            offset_x_twips: Some(JsNumber::Num(5.0)),
            offset_y_twips: Some(JsNumber::Num(6.0)),
        };
        patch_drawing_geometry(&text, &geo).unwrap();
        std::hint::black_box(doc_pr_name(Some(&text)));
        std::hint::black_box(fresh_ids_text(&text, &mut 0.0));
        std::hint::black_box(edited_group_text(&text, &text));
        std::hint::black_box(edited_group_text(&format!("<mc:AlternateContent>{text}"), &text));
        std::hint::black_box(txbx_content_of(&text));
        let theme: indexmap::IndexMap<Str, Str> = DEFAULT_SHAPE_THEME.iter().map(|&(k, v)| (Str::from(k), Str::from(v))).collect();
        let color = |c: &PicColor| shape_color(&theme, c);
        let o = VmlOptions { color: &color, next: None, spid: 1, outline: None };
        let _ = vml_fallback(&text, "", &o);
        let _ = patch_shape(&text, &ShapeChange { fill: Some(ShapeFill::None), ..ShapeChange::default() });
        let took = t.elapsed();
        println!("{name}: {} KB in {took:?}", text.len() / 1024);
        assert!(took.as_secs() < 10, "{name} took {took:?}");
    }
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

/// Saves with a named edit and the raw drawings of the edited objects then mangled: an error or a
/// document, never a panic.
#[test]
fn edited_saves_with_mangled_drawings_do_not_panic() {
    let files = corpus();
    let mut rng = Rng(0x51a7_e0d3);
    let (mut saved, mut refused) = (0, 0);
    for edit in EDITS {
        let mut done = 0;
        for f in files.iter().step_by(5) {
            let bytes = std::fs::read(f).unwrap();
            let mut pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
            let Ok(mut doc) = read_document(&pkg, &ReadOptions { sources: rng.below(2) == 0, ..ReadOptions::default() }) else { continue };
            let mut state = install(&mut doc, SigMode::Canonical);
            if apply(&mut doc, edit) == 0 {
                continue;
            }
            mangle_raw_drawings(&mut doc, &mut rng);
            match save(&mut pkg, &doc, &mut state, DirtyHf::All) {
                Ok(()) => saved += 1,
                Err(_) => refused += 1,
            }
            done += 1;
            if done == 12 {
                break;
            }
        }
    }
    println!("edited saves with mangled drawings: {saved} saved, {refused} refused, no panic");
    assert!(saved + refused > 30);
}

fn mangle_raw_drawings(doc: &mut uo_docx::model::Document, rng: &mut Rng) {
    for pi in 0..doc.body.paras.len() {
        let has = doc.body.paras.get(pi).is_some_and(|p| p.runs.iter().any(|r| r.ext().is_some()));
        if !has {
            continue;
        }
        let Some(p) = doc.body.paras.make_mut(pi) else { continue };
        for r in &mut p.runs {
            let e = r.ext_mut();
            let mut touch = |raw: &mut Option<Str>| {
                if let Some(s) = raw.as_deref()
                    && rng.below(2) == 0
                {
                    *raw = Some(Str::from(mutate(s, rng)));
                }
            };
            if let Some(t) = e.textbox.as_deref_mut() {
                touch(&mut t.raw_drawing_xml);
            }
            if let Some(s) = e.shape.as_deref_mut() {
                touch(&mut s.raw_drawing_xml);
            }
            if let Some(g) = e.group.as_deref_mut() {
                touch(&mut g.raw_drawing_xml);
            }
            if let Some(i) = e.image.as_deref_mut() {
                touch(&mut i.raw_drawing_xml);
            }
        }
    }
}

/// A `Writer` over models that hold garbage.
#[test]
fn writer_functions_refuse_garbage_without_panic() {
    let files = corpus();
    let bytes = std::fs::read(&files[3]).unwrap();
    let mut pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
    let garbage = ["", "not xml", "<w:drawing/>", "<w:drawing><wp:anchor/></w:drawing>", "<<<<", "\u{0}<wps:wsp>"];
    let runs: Vec<(Run, Run)> = garbage
        .iter()
        .map(|raw| {
            let mut tb = Run::default();
            tb.ext_mut().textbox = Some(Box::new(DrawingTextbox {
                raw_drawing_xml: Some((*raw).into()),
                drawing: DrawingId(1),
                edited: Some(true),
                ..DrawingTextbox::default()
            }));
            let mut sh = Run::default();
            sh.ext_mut().shape =
                Some(Box::new(DrawingShape { raw_drawing_xml: Some((*raw).into()), drawing: DrawingId(2), ..DrawingShape::default() }));
            (tb, sh)
        })
        .collect();
    let bare = Run::default();
    let mut fresh = 0.0;
    let mut w =
        Writer { doc: &doc, pkg: &mut pkg, cx: WriteCtx::new(Limits::DEFAULT, SigMode::Canonical), fresh_id: &mut fresh, lossless: None };
    for (raw, (tb, sh)) in garbage.iter().zip(&runs) {
        let _ = edited_shape_xml(&mut w, tb);
        let _ = edited_shape_xml(&mut w, sh);
        let _ = edited_group_xml(&mut w, raw, raw);
        let _ = fresh_drawing_ids(&mut w, raw);
    }
    // an object run without its object
    assert!(edited_shape_xml(&mut w, &bare).unwrap().is_empty());
    assert_eq!(w.cx.shape_depth, 0);
}
