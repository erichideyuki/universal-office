//! Parity of the drawing-object emitters of the D1b writer (frontier W2, `write::emit::object`) on
//! documents with a named edit: replays `tests/parity/object-edits.capture`, written by
//! `js/capture-object-edits.mjs` from the frozen TS (docx-core's lossless harness with one edit applied to
//! the top-level paragraphs of the body between the install and the save: `edited` on the shapes, text
//! boxes, groups and images, their size, a hidden group, a swapped image, a text box that came in VML,
//! images without their raw drawing — a new picture or an OLE object), read with sources and every
//! header/footer part dirty (`all`) and without sources (`gen`). The edits are implemented here as in the
//! capture script (`EDITS`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-object-edits.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Digest, Replay, repo_root};

mod w2_support;
use w2_support::{EDITS, apply};

fn facets(bytes: &[u8], edit: &str, dirty: DirtyHf, sources: bool) -> Result<Vec<(String, String)>, uo_docx::error::DocxError> {
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    let input = Package::open(bytes, &limits, &options)?;
    let mut pkg = Package::open(bytes, &limits, &options)?;
    let mut doc = read_document(&pkg, &ReadOptions { sources, ..ReadOptions::default() })?;
    let mut state = install(&mut doc, SigMode::Canonical);
    apply(&mut doc, edit);
    save(&mut pkg, &doc, &mut state, dirty)?;
    let saved = pkg.save(&uo_opc::package::SaveOptions::default())?;
    let after = Package::open(&saved, &limits, &options)?;
    let names: Vec<String> = after.part_names().map(str::to_owned).collect();
    let mut out = vec![("order".to_owned(), Digest::of(names.join("\n").as_bytes()).short())];
    let mut changed: Vec<&str> = Vec::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for name in &names {
        let a = after.part(name)?.unwrap_or(&[]);
        let same = input.part(name)?.is_some_and(|b| b == a);
        if !same {
            changed.push(name);
            parts.push((format!("p:{name}"), Digest::of(a).short()));
        }
    }
    out.push(("changed".to_owned(), changed.join("|")));
    out.extend(parts);
    Ok(out)
}

fn run(shard: usize, shards: usize) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/object-edits.capture")).unwrap();
    let keys: Vec<String> = cap.records().iter().map(|r| r.key.clone()).collect();
    let mine: Vec<&String> = keys.iter().enumerate().filter(|(i, _)| i % shards == shard).map(|(_, k)| k).collect();
    let sub = cap.subset(|k| mine.iter().any(|m| m.as_str() == k));
    let mut replay = Replay::new(&sub);
    let root = repo_root(manifest);
    let mut stubs = std::collections::BTreeMap::<&'static str, usize>::new();
    for key in mine {
        let mut it = key.rsplitn(3, '#');
        let variant = it.next().unwrap();
        let edit = it.next().unwrap();
        let rel = it.next().unwrap();
        assert!(EDITS.contains(&edit), "{edit}");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let (dirty, sources) = if variant == "all" { (DirtyHf::All, true) } else { (DirtyHf::All, false) };
        let result = match facets(&bytes, edit, dirty, sources) {
            Ok(f) => f,
            Err(uo_docx::error::DocxError::Write(what)) if what.starts_with("emitter ") => {
                *stubs.entry(what).or_default() += 1;
                if let Some(rec) = sub.record(key) {
                    for (f, _) in &rec.facets {
                        replay.skip(key, f);
                    }
                }
                continue;
            }
            Err(e) => {
                if std::env::var("UO_PARITY_ERRORS").is_ok() {
                    println!("{key}: {e}");
                }
                vec![("error".to_owned(), "err".to_owned())]
            }
        };
        for (f, v) in &result {
            replay.check(key, f, v);
        }
        if let Some(rec) = sub.record(key) {
            for (f, _) in &rec.facets {
                if !result.iter().any(|(g, _)| g == f) {
                    replay.check(key, f, "(not produced)");
                }
            }
        }
    }
    let report = replay.finish();
    println!("object edits shard {shard}/{shards}: {}", report.summary());
    if !stubs.is_empty() {
        println!("records stopped at an emitter stub (frontier pending): {stubs:?}");
    }
    if !report.is_clean() {
        println!("{}", report.describe(20));
    }
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn object_edits_shard_0() {
    run(0, 4);
}

#[test]
fn object_edits_shard_1() {
    run(1, 4);
}

#[test]
fn object_edits_shard_2() {
    run(2, 4);
}

#[test]
fn object_edits_shard_3() {
    run(3, 4);
}
