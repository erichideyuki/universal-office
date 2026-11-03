//! Parity of frontier R3 (facet `drawing`): replays `tests/parity/drawing.capture`, written by
//! `js/capture-drawing.mjs` from the frozen TS (`readWpAnchor`, `readSpPr`, `readCustGeom`,
//! `parseVmlStyle`, `vmlShapeStyle`, `vmlAnchor`, `vmlTextboxToDrawing`, `vmlSingleTextbox`,
//! `readPageBackground`, `isWatermarkXml`, `readWatermarkXml`, `vmlColor`) and compares exactly:
//! per target element `<container>#<part>#<ordinal>` one digest per function of the canonical
//! stream of its result (`js/canon.mjs` against the model's `JsValue::canon`), and per container
//! the number of targets. Corpus: the 2,767 git-tracked docx of the repository (the 2,742
//! oracle documents of docx-layout and the fixtures) and the seeded quirk corpus
//! (`tests/parity/drawing.quirks.txt`: attribute quirks of anchors, spPr, custom geometry, VML
//! shapes, page backgrounds and watermarks), plus `vmlColor` over `tests/parity/drawing.vmlcolors.txt`.
//!
//! The one named difference, the lone surrogate of `vmlColor` (`h.slice(0, 6)` or a doubled
//! unit of a non-BMP character): the TS string holds the surrogate, `Str` holds U+FFFD. The
//! `vmlcolor/<n>` records compare the Rust result with the TS result after `fffd` (the same
//! mapping), so the difference is pinned exactly; the main corpus has no such colour.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-drawing.mjs`.
//! Trace one record: `UO_PARITY_TRACE=<key> cargo test -p uo-docx --test parity_drawing -- --nocapture`
//! with `capture-drawing.mjs --trace <key>`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::{CanonWriter, canon_entries};
use uo_docx::ids::MediaId;
use uo_docx::js::JsValue;
use uo_docx::model::MediaStore;
use uo_docx::model::media::{ImageData, MediaRef};
use uo_docx::read::DOCX_POS;
use uo_docx::read::drawing::{parse_vml_style, read_cust_geom, read_sp_pr, read_wp_anchor, vml_anchor, vml_color, vml_shape_style};
use uo_docx::read::matchers::keep_ascii_digits;
use uo_docx::read::page_background::read_page_background;
use uo_docx::read::vml_shape::{VmlLang, vml_single_textbox, vml_textbox_to_drawing};
use uo_docx::read::watermark::{is_watermark_xml, read_watermark_xml};
use uo_jsnum::parse::parse_int;
use uo_opc::Package;
use uo_opc::xml::{Document, Element, parse};
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

/// The hex digest (first 16 bytes) of what `f` writes to a fresh canonical stream.
fn digest(trace: Option<&mut String>, f: impl FnOnce(&mut CanonWriter<'_>)) -> String {
    let mut s = Sink(if trace.is_some() { Stream::traced() } else { Stream::new() });
    {
        let mut w = CanonWriter::new(&mut s);
        f(&mut w);
    }
    if let Some(t) = trace {
        t.push_str(s.0.trace());
    }
    s.0.finish().short()
}

fn lang_of(k: usize) -> VmlLang {
    [VmlLang::Pt, VmlLang::En, VmlLang::Es].get(k % 3).copied().unwrap_or_default()
}

/// The facets of one target element, as `(facet, digest)`.
fn element_facets(el: Element<'_>, k_in_part: usize, trace: bool, traces: &mut Vec<(String, String)>) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let mut add = |facet: &'static str, f: &dyn Fn(&mut CanonWriter<'_>)| {
        let mut t = String::new();
        let d = digest(trace.then_some(&mut t), |w| f(w));
        if trace {
            traces.push((facet.to_string(), t));
        }
        out.push((facet, d));
    };
    match el.name() {
        "wp:anchor" => add("anchor", &|w| read_wp_anchor(Some(el)).canon(w)),
        "wps:wsp" => add("sppr", &|w| read_sp_pr(el.first_child("wps:spPr"), el.first_child("wps:style")).canon(w)),
        "pic:pic" => add("sppr", &|w| read_sp_pr(el.first_child("pic:spPr"), None).canon(w)),
        "a:custGeom" => add("cust", &|w| read_cust_geom(el, 1_905_000.0, 952_500.0).canon(w)),
        "v:shape" | "v:rect" | "v:roundrect" | "v:oval" | "v:line" => {
            add("vmlstyle0", &|w| {
                let style = parse_vml_style(el.attr("style"));
                let mut entries: Vec<(&str, &str)> = style.entries().collect();
                entries.sort_by(|a, b| uo_docx::canon::utf16_cmp(a.0, b.0));
                w.object(entries.len());
                for (k, v) in entries {
                    w.key(k);
                    w.str(v);
                }
            });
            add("vmlstyle", &|w| vml_shape_style(el, "rect").canon(w));
            add("vmlanchor", &|w| vml_anchor(&parse_vml_style(el.attr("style")), el.descend("w10:wrap")).canon(w));
            add("vmltb", &|w| {
                let digits = keep_ascii_digits(el.attr("o:spid").unwrap_or(""));
                let n = parse_int(&digits, 10);
                let id = 1.0 + if uo_jsnum::conv::truthy(n) { n } else { 0.0 };
                match vml_textbox_to_drawing(el, id, lang_of(k_in_part)) {
                    Ok(Some(s)) => w.str(&s),
                    Ok(None) => w.undefined(),
                    Err(_) => w.str("!throw"),
                }
            });
        }
        "w:pict" => {
            add("single", &|w| {
                let idx = vml_single_textbox(el).and_then(|s| el.child_elements().position(|c| c == s));
                w.num(idx.map_or(-1.0, |i| f64::from(u32::try_from(i).unwrap_or(u32::MAX))));
            });
            let xml = el.source_slice().unwrap_or("");
            add("wm", &|w| {
                let is = is_watermark_xml(Some(xml));
                let info = read_watermark_xml(xml, &|v| vml_color(v));
                canon_entries(
                    w,
                    &mut [
                        ("is", &move |w: &mut CanonWriter<'_>| w.bool(is)),
                        ("info", &|w: &mut CanonWriter<'_>| match &info {
                            Some(i) => i.canon(w),
                            None => w.null(),
                        }),
                    ],
                );
            });
        }
        "w:background" => {
            let mut media = MediaStore::new();
            let bg = read_page_background(el, &mut |rid| {
                (rid.encode_utf16().count() % 2 == 0).then(|| {
                    let id: MediaId = media.add(format!("img:{rid}").as_bytes());
                    ImageData { data: MediaRef(id), ext: "png".into(), ..ImageData::default() }
                })
            });
            let mut t = String::new();
            let mut s = Sink(if trace { Stream::traced() } else { Stream::new() });
            {
                let mut w = CanonWriter::new(&mut s).with_media(&media);
                bg.canon(&mut w);
            }
            t.push_str(s.0.trace());
            if trace {
                traces.push(("bg".into(), t));
            }
            out.push(("bg", s.0.finish().short()));
        }
        _ => {}
    }
    out
}

/// Replays the targets of one parsed part; `ordinal` continues across the parts of a container.
fn replay_part(replay: &mut Replay<'_>, container: &str, part: &str, doc: &Document, ordinal: &mut u32, trace_key: Option<&str>) {
    let root = doc.root();
    let mut k = 0usize;
    for el in std::iter::once(root).chain(root.descendants()) {
        let key = format!("{container}#{part}#{ordinal}");
        let traced = trace_key == Some(key.as_str());
        let mut traces = Vec::new();
        let facets = element_facets(el, k, traced, &mut traces);
        if facets.is_empty() {
            continue;
        }
        for (facet, d) in &facets {
            replay.check(&key, facet, d);
        }
        for (facet, t) in traces {
            println!("--- {key} {facet}\n{t}");
        }
        *ordinal += 1;
        k += 1;
    }
}

fn is_target_part(name: &str) -> bool {
    let word = name.strip_prefix("word/").is_some_and(|r| !r.contains('/') && r.ends_with(".xml"));
    word || name.ends_with(".vml")
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

fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(parity_dir().join("drawing.capture")).unwrap();
    let container_of = |key: &str| key.split('#').next().unwrap_or("").to_owned();
    let mine = |key: &str| {
        let c = container_of(key);
        if shards == 0 {
            c.starts_with("quirk/") || c.starts_with("vmlcolor/")
        } else {
            !c.starts_with("quirk/") && !c.starts_with("vmlcolor/") && shard_of(&c, shards) == shard
        }
    };
    let cap = cap.subset(mine);
    assert!(!cap.is_empty());
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        let quirks = std::fs::read_to_string(parity_dir().join("drawing.quirks.txt")).unwrap();
        let mut n = 0u32;
        for (i, line) in quirks.lines().filter(|l| !l.is_empty()).enumerate() {
            let container = format!("quirk/{i}");
            let mut ordinal = 0u32;
            if let Ok(doc) = parse(unescape(line), &DOCX_POS) {
                replay_part(&mut replay, &container, "fragment", &doc, &mut ordinal, trace.as_deref());
            }
            replay.check(&container, "targets", &ordinal.to_string());
            n += 1;
        }
        assert_eq!(n, 1600, "the quirk corpus changed: recapture");
        let colors = std::fs::read_to_string(parity_dir().join("drawing.vmlcolors.txt")).unwrap();
        for (i, line) in colors.lines().enumerate() {
            let v = unescape(line);
            let d = digest(None, |w| match vml_color(Some(&v)) {
                Some(s) => w.str(&s),
                None => w.undefined(),
            });
            replay.check(&format!("vmlcolor/{i}"), "c", &d);
        }
    } else {
        let root = repo_root(manifest);
        let mut containers: Vec<String> = cap.records().iter().filter(|r| !r.key.contains('#')).map(|r| r.key.clone()).collect();
        containers.sort();
        for rel in containers {
            let bytes = std::fs::read(root.join(&rel)).unwrap();
            let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let mut ordinal = 0u32;
            for name in pkg.part_names().filter(|n| is_target_part(n)) {
                if let Ok(doc) = pkg.xml_part(name, &DOCX_POS) {
                    replay_part(&mut replay, &rel, name, &doc, &mut ordinal, trace.as_deref());
                }
            }
            replay.check(&rel, "targets", &ordinal.to_string());
        }
    }
    let report = replay.finish();
    println!("drawing parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn drawing_parity_shard_0() {
    run(0, 4);
}

#[test]
fn drawing_parity_shard_1() {
    run(1, 4);
}

#[test]
fn drawing_parity_shard_2() {
    run(2, 4);
}

#[test]
fn drawing_parity_shard_3() {
    run(3, 4);
}

#[test]
fn drawing_parity_quirks() {
    run(0, 0);
}
