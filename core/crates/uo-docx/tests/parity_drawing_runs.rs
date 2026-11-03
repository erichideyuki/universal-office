//! Parity of frontier R4b (facet `drawing-runs`): replays `tests/parity/drawing-runs.capture`,
//! written by `js/capture-drawing-runs.mjs` from the frozen TS (`readParagraph` over a run holding
//! a `w:drawing`, `w:pict`, `w:object` or `mc:AlternateContent`, `readPictureFormat`,
//! `syncImageFromXml`, `syncShapeFromXml`), and compares exactly: per target element
//! `<container>#<part>#<ordinal>` one digest per facet (`runs`, `fmt`, `fmtany`, `sync`) of the
//! canonical stream of its result and one of the key-order facet (`<facet>.o`), and per container
//! the number of targets. Corpus: the 2,767 git-tracked docx of the repository and the seeded
//! quirk corpus (`tests/parity/drawing-runs.quirks.txt`, `js/quirk-objects.mjs`).
//!
//! The host is the context the capture used: the relationships of the part (the TS
//! `buildReadContext` rules; D6 path resolution) for documents, the quirk rules (even-length id =
//! an image, a part id = the part of the case) for fragments, and the stub block reader (one
//! paragraph per `w:p` child). The group items' runs are compared with the model's key order of
//! `Run` on both sides (the capture moves `props` after the text).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-drawing-runs.mjs`.
//! Trace one record: `UO_PARITY_TRACE=<key>` here and `--trace <key>` there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use support::Sink;
use uo_docx::canon::{CanonWriter, canon_boxed, canon_entries};
use uo_docx::error::DocxError;
use uo_docx::ids::{IdAlloc, MediaId};
use uo_docx::js::JsValue;
use uo_docx::model::para::ParaProps;
use uo_docx::model::run::{RunExt, RunProps};
use uo_docx::model::{Paragraph, Run, Story};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::inline_objects::{ObjectHost, ResolvedImage, TextboxRead, read_object_runs, sync_image_from_xml, sync_shape_from_xml};
use uo_docx::read::picture_format::{dml_color_spec, read_dml_color, read_picture_format};
use uo_docx::read::{DOCX, ReadOptions};
use uo_docx::text::Utf16;
use uo_opc::Package;
use uo_opc::xml::{Document as XmlDocument, Element, parse};
use uo_testkit::{Capture, Replay, Stream, repo_root};

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

enum Mode {
    Real { source: String, rels: HashMap<String, (String, bool)> },
    Quirk { parts: HashMap<String, String> },
}

struct Host<'p> {
    ctx: ReadCtx<'p>,
    mode: Mode,
}

impl ObjectHost for Host<'_> {
    fn ids(&mut self) -> &mut IdAlloc {
        &mut self.ctx.ids
    }

    fn resolve_image(&mut self, rid: &str) -> Result<Option<ResolvedImage>, DocxError> {
        match &self.mode {
            Mode::Quirk { .. } => {
                if rid.encode_utf16().count().is_multiple_of(2) {
                    let id = self.ctx.media.add(format!("img:{rid}").as_bytes());
                    Ok(Some(ResolvedImage { data: id, ext: "png".into() }))
                } else {
                    Ok(None)
                }
            }
            Mode::Real { source, rels } => {
                let Some((target, external)) = rels.get(rid) else { return Ok(None) };
                if *external {
                    return Ok(None);
                }
                let Some(path) = self.ctx.resolve_target(source, target) else { return Ok(None) };
                let path = path.to_owned();
                let Some(id) = self.ctx.media_part(&path)? else { return Ok(None) };
                let ext = uo_jsnum::text::to_lowercase(path.rsplit('.').next().unwrap_or(""));
                Ok(Some(ResolvedImage { data: id, ext: ext.into() }))
            }
        }
    }

    fn add_media(&mut self, bytes: &[u8]) -> MediaId {
        self.ctx.media.add(bytes)
    }

    fn resolve_part(&mut self, rid: &str) -> Result<Option<XmlDocument>, DocxError> {
        match &self.mode {
            Mode::Quirk { parts } => match parts.get(rid) {
                Some(t) => Ok(Some(parse(t.as_str(), &DOCX).map_err(DocxError::from)?)),
                None => Ok(None),
            },
            Mode::Real { source, rels } => {
                let Some((target, external)) = rels.get(rid) else { return Ok(None) };
                if *external {
                    return Ok(None);
                }
                let Some(path) = self.ctx.resolve_target(source, target) else { return Ok(None) };
                self.ctx.xml_part_plain(path)
            }
        }
    }

    fn enter_nested(&mut self) -> bool {
        self.ctx.enter_nested()
    }

    fn leave_nested(&mut self) {
        self.ctx.leave_nested();
    }

    fn read_textbox_content(&mut self, content: Element<'_>) -> Result<TextboxRead, DocxError> {
        let mut story = Story::new();
        for p in content.children_named("w:p") {
            let mut para = Paragraph::empty(self.ctx.ids.para());
            para.runs = vec![Run { text: Utf16::from(p.text_content()), props: Arc::new(RunProps::default()), ..Run::default() }];
            para.props = Arc::new(ParaProps::default());
            story.push_paragraph(para);
        }
        Ok(TextboxRead::Blocks { story, txbx_src: None })
    }
}

/// `objsOf`: `[{ text, image?, textbox?, shape?, group? }]` (keys in this order).
fn canon_objs(w: &mut CanonWriter<'_>, runs: &[RunExt]) {
    w.array(runs.len());
    for r in runs {
        let mut entries: Vec<(&str, Box<uo_docx::canon::ValueFn<'_>>)> =
            vec![("text", Box::new(|w: &mut CanonWriter<'_>| w.str("\u{fffc}")))];
        if let Some(i) = &r.image {
            entries.push(("image", Box::new(move |w: &mut CanonWriter<'_>| i.canon(w))));
        }
        if let Some(i) = &r.textbox {
            entries.push(("textbox", Box::new(move |w: &mut CanonWriter<'_>| i.canon(w))));
        }
        if let Some(i) = &r.shape {
            entries.push(("shape", Box::new(move |w: &mut CanonWriter<'_>| i.canon(w))));
        }
        if let Some(i) = &r.group {
            entries.push(("group", Box::new(move |w: &mut CanonWriter<'_>| i.canon(w))));
        }
        canon_boxed(w, entries);
    }
}

/// The nine edits of a raw `w:drawing` of the sync facet (`variants` of the capture script).
fn variants(raw: &str) -> Vec<String> {
    let r = |s: &str, a: &str, b: &str| s.replacen(a, b, 1);
    vec![
        raw.to_owned(),
        r(&r(raw, "<wp:inline", "<wp:anchor"), "</wp:inline>", "</wp:anchor>"),
        r(&r(raw, "<wp:anchor", "<wp:inline"), "</wp:anchor>", "</wp:inline>"),
        r(raw, "<a:xfrm>", "<a:xfrm rot=\"2700000\" flipH=\"1\" flipV=\"1\">"),
        r(raw, "<wp:docPr ", "<wp:docPr hidden=\"1\" "),
        r(raw, "<a:prstGeom prst=\"rect\"", "<a:prstGeom prst=\"ellipse\""),
        r(raw, "<a:ext cx=\"", "<a:ext cx=\"9"),
        r(raw, "<wp:extent cx=\"", "<wp:extent cx=\"3"),
        r(raw, "behindDoc=\"0\"", "behindDoc=\"1\""),
    ]
}

struct D {
    m: String,
    o: String,
    traces: (String, String),
}

/// The digests of what `f` writes (main stream, key-order stream); `Err` is the TS exception.
fn digest(trace: bool, f: impl FnOnce(&mut Sink, &mut Sink) -> Result<(), DocxError>) -> D {
    let mk = || Sink(if trace { Stream::traced() } else { Stream::new() });
    let (mut s, mut o) = (mk(), mk());
    if f(&mut s, &mut o).is_err() {
        let (mut s, mut o) = (mk(), mk());
        CanonWriter::new(&mut s).with_order(&mut o).str("!throw");
        return D { m: s.0.finish().short(), o: o.0.finish().short(), traces: (String::new(), String::new()) };
    }
    let traces = (s.0.trace().to_owned(), o.0.trace().to_owned());
    D { m: s.0.finish().short(), o: o.0.finish().short(), traces }
}

fn facets(el: Element<'_>, host: &mut Host<'_>, trace: bool) -> Vec<(&'static str, D)> {
    let mut out = Vec::new();
    out.push((
        "runs",
        digest(trace, |s, o| {
            let runs = read_object_runs(el, host)?;
            let mut w = CanonWriter::new(s).with_order(o).with_media(&host.ctx.media);
            canon_objs(&mut w, &runs);
            Ok(())
        }),
    ));
    if el.name() == "w:drawing" {
        let raw = el.raw().unwrap_or("");
        for (name, any) in [("fmt", false), ("fmtany", true)] {
            out.push((
                name,
                digest(trace, |s, o| {
                    let mut w = CanonWriter::new(s).with_order(o);
                    match read_picture_format(raw, any) {
                        Some(f) => f.canon(&mut w),
                        None => w.undefined(),
                    }
                    Ok(())
                }),
            ));
        }
        out.push((
            "spec",
            digest(trace, |s, o| {
                for e in std::iter::once(el).chain(el.descendants()) {
                    match read_dml_color(Some(e)) {
                        Some(col) => {
                            let (spec, alpha) = dml_color_spec(&col);
                            // the named difference of R4b.md item 9: without a modifier the TS spec of a preset colour named
                            // like an `Object.prototype` member is that member itself (a function or the prototype: an object
                            // with no own keys), Rust has the text `String(member)`
                            let proto_member = &*col.kind == "prstClr"
                                && PROTO_NAMES.contains(&&*col.val)
                                && col.mods.iter().flatten().all(|(n, _)| &**n == "alpha");
                            let mut w = CanonWriter::new(s).with_order(o);
                            canon_entries(
                                &mut w,
                                &mut [
                                    ("spec", &move |w: &mut CanonWriter<'_>| {
                                        if proto_member {
                                            w.order_facet(|_| {});
                                            w.object(0);
                                        } else {
                                            w.str(&spec);
                                        }
                                    }),
                                    ("alpha", &move |w: &mut CanonWriter<'_>| w.num(alpha)),
                                ],
                            );
                        }
                        None => {
                            s.0.tag(b'U');
                        }
                    }
                }
                Ok(())
            }),
        ));
        out.push((
            "sync",
            digest(trace, |s, o| {
                for (i, v) in variants(raw).iter().enumerate() {
                    s.0.tag(b'V').u32(u32::try_from(i).unwrap_or(0));
                    let mut runs = read_object_runs(el, host)?;
                    for run in &mut runs {
                        if let Some(img) = run.image.as_mut().filter(|i| i.raw_drawing_xml.as_deref().is_some_and(|r| !r.is_empty())) {
                            img.raw_drawing_xml = Some(v.as_str().into());
                            sync_image_from_xml(img);
                        } else if run.textbox.as_ref().is_some_and(|t| t.raw_drawing_xml.as_deref().is_some_and(|r| !r.is_empty())) {
                            if let Some(t) = run.textbox.as_mut() {
                                t.raw_drawing_xml = Some(v.as_str().into());
                            }
                            sync_shape_from_xml(run);
                        } else if run.shape.as_ref().is_some_and(|t| t.raw_drawing_xml.as_deref().is_some_and(|r| !r.is_empty())) {
                            if let Some(t) = run.shape.as_mut() {
                                t.raw_drawing_xml = Some(v.as_str().into());
                            }
                            sync_shape_from_xml(run);
                        } else if run.group.as_ref().is_some_and(|t| t.raw_drawing_xml.as_deref().is_some_and(|r| !r.is_empty())) {
                            if let Some(t) = run.group.as_mut() {
                                t.raw_drawing_xml = Some(v.as_str().into());
                            }
                            sync_shape_from_xml(run);
                        }
                    }
                    let mut w = CanonWriter::new(s).with_order(o).with_media(&host.ctx.media);
                    canon_objs(&mut w, &runs);
                }
                Ok(())
            }),
        ));
    }
    out
}

const PROTO_NAMES: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

const TARGETS: [&str; 4] = ["w:drawing", "w:pict", "w:object", "mc:AlternateContent"];

fn replay_part(
    replay: &mut Replay<'_>,
    container: &str,
    part: &str,
    doc: &XmlDocument,
    host: &mut Host<'_>,
    ordinal: &mut u32,
    trace_key: Option<&str>,
) {
    let root = doc.root();
    for el in std::iter::once(root).chain(root.descendants()) {
        if !TARGETS.contains(&el.name()) {
            continue;
        }
        let key = format!("{container}#{part}#{ordinal}");
        let traced = trace_key == Some(key.as_str());
        for (facet, d) in facets(el, host, traced) {
            replay.check(&key, facet, &d.m);
            replay.check(&key, &format!("{facet}.o"), &d.o);
            if traced {
                println!("--- {key} {facet}\n{}\n--- order\n{}", d.traces.0, d.traces.1);
            }
        }
        *ordinal += 1;
    }
}

fn shard_of(container: &str, shards: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in container.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h % shards
}

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity")
}

fn rels_of(ctx: &ReadCtx<'_>, part: &str) -> HashMap<String, (String, bool)> {
    let rels_path = format!("word/_rels/{}.rels", part.strip_prefix("word/").unwrap_or(part));
    let mut out = HashMap::new();
    if let Ok(Some(doc)) = ctx.xml_part_plain(&rels_path) {
        for r in uo_opc::rels::from_document(&doc) {
            if r.id.is_empty() || r.target.is_empty() {
                continue;
            }
            let target = r.target.strip_prefix("./").unwrap_or(&r.target).to_owned();
            out.insert(r.id, (target, r.external));
        }
    }
    out
}

fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(parity_dir().join("drawing-runs.capture")).unwrap();
    let container_of = |key: &str| key.split('#').next().unwrap_or("").to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(&c, shards) == shard }
    };
    let cap = cap.subset(mine);
    assert!(!cap.is_empty());
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        let quirks = std::fs::read_to_string(parity_dir().join("drawing-runs.quirks.txt")).unwrap();
        let mut n = 0u32;
        let empty = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
        for line in quirks.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (key, text) = line.split_once('\t').unwrap();
            let text = unescape(text);
            let mut pieces = text.split('\u{1}');
            let frag = pieces.next().unwrap();
            let parts: HashMap<String, String> =
                pieces.filter_map(|p| p.split_once('\u{2}')).map(|(rid, t)| (rid.to_owned(), t.to_owned())).collect();
            let mut host = Host { ctx: ReadCtx::new(&empty, &ReadOptions::default()), mode: Mode::Quirk { parts } };
            let mut ordinal = 0u32;
            if let Ok(doc) = parse(frag, &DOCX) {
                replay_part(&mut replay, key, "fragment", &doc, &mut host, &mut ordinal, trace.as_deref());
            }
            replay.check(key, "targets", &ordinal.to_string());
            n += 1;
        }
        assert_eq!(n, 2214, "the quirk corpus changed: recapture");
    } else {
        let root = repo_root(manifest);
        let mut containers: Vec<String> = cap.records().iter().filter(|r| !r.key.contains('#')).map(|r| r.key.clone()).collect();
        containers.sort();
        for rel in containers {
            let bytes = std::fs::read(root.join(&rel)).unwrap();
            let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let mut ordinal = 0u32;
            for name in pkg.part_names().filter(|n| n.strip_prefix("word/").is_some_and(|r| !r.contains('/') && r.ends_with(".xml"))) {
                let Ok(doc) = pkg.xml_part(name, &DOCX) else { continue };
                let ctx = ReadCtx::new(&pkg, &ReadOptions::default());
                let rels = rels_of(&ctx, name);
                let mut host = Host { ctx, mode: Mode::Real { source: name.to_owned(), rels } };
                replay_part(&mut replay, &rel, name, &doc, &mut host, &mut ordinal, trace.as_deref());
            }
            replay.check(&rel, "targets", &ordinal.to_string());
        }
    }
    let report = replay.finish();
    println!("drawing-runs parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn drawing_runs_parity_shard_0() {
    run(0, 4);
}

#[test]
fn drawing_runs_parity_shard_1() {
    run(1, 4);
}

#[test]
fn drawing_runs_parity_shard_2() {
    run(2, 4);
}

#[test]
fn drawing_runs_parity_shard_3() {
    run(3, 4);
}

#[test]
fn drawing_runs_parity_quirks() {
    run(0, 0);
}
