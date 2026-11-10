//! Parity of frontier R4a (props mapping, facet `props-map`): replays
//! `tests/parity/props-map.capture`, written by `js/capture-props-map.mjs` from the frozen TS
//! (`runPropsFromRPr`, `editBaseProps`, `paraPropsFromPPr`, `readSectPr`, `readBorderEdge`), and
//! compares exactly (values `props-map`, key order `order`, element count `n`). Corpus: every
//! git-tracked .docx of the repository (the word/*.xml parts) and the generated attribute-quirk
//! corpus (`tests/parity/props-map.quirks.txt`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-props-map.mjs`.
//! Trace one part: `UO_PARITY_TRACE=<key> cargo test -p uo-docx --test parity_props_map --
//! --nocapture` against `capture-props-map.mjs --trace <key>`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;
use std::sync::Arc;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::model::Run;
use uo_docx::model::RunProps;
use uo_docx::model::run::{NoteRef, RunExt};
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{edit_base_props, para_props_from_ppr, read_border_edge, read_sect_pr, run_props_from_rpr};
use uo_docx::read::{DOCX, WORD_PREFIXES};
use uo_opc::xml::{Document, Element, parse};
use uo_testkit::{Capture, Replay, Stream, repo_root};

const EDGES: [&str; 6] = ["w:top", "w:left", "w:bottom", "w:right", "w:between", "w:bar"];

fn base_runs() -> Vec<Run> {
    let link = || Arc::new(RunProps { link: Some("https://x".into()), ..RunProps::default() });
    let note = || Some(NoteRef { kind: "footnote".into(), id: 1.0.into(), ..NoteRef::default() });
    let ext = |note_ref, sym: bool| Some(Box::new(RunExt { note_ref, sym_font: sym.then(|| "Wingdings".into()), ..RunExt::default() }));
    vec![
        Run { props: link(), ..Run::default() },
        Run { ext: ext(note(), false), ..Run::default() },
        Run { ext: ext(None, true), ..Run::default() },
        Run { props: link(), ext: ext(note(), true), ..Run::default() },
    ]
}

/// The elements in pre-order with the name of their parent.
fn elements<'d>(root: Element<'d>) -> Vec<(Element<'d>, &'d str)> {
    let mut out = Vec::new();
    let mut stack = vec![(root.name(), root.child_elements().collect::<Vec<_>>().into_iter())];
    out.push((root, ""));
    while let Some((name, it)) = stack.last_mut() {
        let name = *name;
        match it.next() {
            None => {
                stack.pop();
            }
            Some(c) => {
                out.push((c, name));
                stack.push((c.name(), c.child_elements().collect::<Vec<_>>().into_iter()));
            }
        }
    }
    out
}

struct Digests {
    n: usize,
    main: String,
    order: String,
    traces: Option<(String, String)>,
}

fn digest_part(doc: &Document, traced: bool) -> Digests {
    let mk = || Sink(if traced { Stream::traced() } else { Stream::new() });
    let (mut s, mut o) = (mk(), mk());
    let runs = base_runs();
    let mut n = 0u32;
    for (el, parent) in elements(doc.root()) {
        let name = el.name();
        let is_edge = EDGES.contains(&name) && parent == "w:pBdr";
        if !(name == "w:rPr" || name == "w:pPr" || name == "w:sectPr" || is_edge) {
            continue;
        }
        s.0.tag(b'E').u32(n);
        o.0.tag(b'E').u32(n);
        n += 1;
        let mut w = CanonWriter::new(&mut s).with_order(&mut o);
        match name {
            "w:rPr" => {
                let rpr = read_rpr(Some(el));
                run_props_from_rpr(rpr.as_ref()).canon(&mut w);
                for run in &runs {
                    edit_base_props(run, rpr.as_ref()).canon(&mut w);
                }
            }
            "w:pPr" => para_props_from_ppr(read_ppr(Some(el)).as_ref()).canon(&mut w),
            "w:sectPr" => read_sect_pr(Some(el)).canon(&mut w),
            _ => read_border_edge(Some(el)).canon(&mut w),
        }
    }
    let traces = traced.then(|| (s.0.trace().to_owned(), o.0.trace().to_owned()));
    Digests { n: n as usize, main: s.0.finish().short(), order: o.0.finish().short(), traces }
}

fn replay_part(replay: &mut Replay<'_>, key: &str, doc: &Document, trace: Option<&str>) {
    let traced = trace == Some(key);
    let d = digest_part(doc, traced);
    if let Some((m, o)) = &d.traces {
        let name = key.replace(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'), "_");
        std::fs::write(format!("{name}.rust.trace"), m).unwrap();
        std::fs::write(format!("{name}.rust.order.trace"), o).unwrap();
    }
    replay.check(key, "n", &d.n.to_string());
    replay.check(key, "props-map", &d.main);
    replay.check(key, "order", &d.order);
}

#[test]
fn props_map_matches_the_frozen_ts() {
    assert!(!WORD_PREFIXES.is_empty());
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/props-map.capture")).unwrap();
    let quirks = std::fs::read_to_string(Path::new(manifest).join("tests/parity/props-map.quirks.txt")).unwrap();
    let quirks: Vec<&str> = quirks.lines().collect();
    let root = repo_root(manifest);
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    let mut current: Option<(String, uo_opc::Package)> = None;
    let mut parts = 0u64;
    for record in cap.records() {
        let key = record.key.as_str();
        parts += 1;
        let (container, part) = key.rsplit_once('#').unwrap();
        match container {
            "synthetic" => {
                assert_eq!(part, "sectPr-undefined");
                let (mut s, mut o) = (Sink(Stream::new()), Sink(Stream::new()));
                let mut w = CanonWriter::new(&mut s).with_order(&mut o);
                read_sect_pr(None).canon(&mut w);
                replay.check(key, "n", "1");
                replay.check(key, "props-map", &s.0.finish().short());
                replay.check(key, "order", &o.0.finish().short());
            }
            "quirk" => {
                let doc = parse(quirks[part.parse::<usize>().unwrap()].to_owned(), &DOCX).unwrap();
                replay_part(&mut replay, key, &doc, trace.as_deref());
            }
            _ => {
                if current.as_ref().is_none_or(|(c, _)| c != container) {
                    let bytes = std::fs::read(root.join(container)).unwrap();
                    let pkg =
                        uo_opc::Package::open_owned(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
                    current = Some((container.to_owned(), pkg));
                }
                let pkg = &current.as_ref().unwrap().1;
                match pkg.xml_part(part, &DOCX) {
                    Ok(doc) => replay_part(&mut replay, key, &doc, trace.as_deref()),
                    Err(e) => {
                        replay.check(key, "n", &format!("parse error: {e}"));
                        replay.check(key, "props-map", "parse error");
                        replay.check(key, "order", "parse error");
                    }
                }
            }
        }
    }
    println!("replayed {parts} parts");
    replay.finish().assert_clean();
}
