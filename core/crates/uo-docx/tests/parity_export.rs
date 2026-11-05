//! Parity of frontier D5a, facet `export`: replays `tests/parity/export.capture`, written by
//! `js/capture-export.mjs` from the frozen TS (`toHtml` and `toMarkdown` of
//! `exportDocFromFullDocument(readFullDocument(pkg))`, each string as its digest, UTF-8 with lone
//! surrogates as U+FFFD). Facets per input: `html`, `html-frag` (fragment, images omitted),
//! `html-title`, `md`, `md-omit` (images omitted, with a title), `error-md` when the TS throws,
//! `error` when the read throws. Corpus: the 2,742 oracle documents of docx-layout, the docx-core
//! test fixtures, and the synthetic packages of `js/quirk-d5a.mjs` (`tests/parity/export.inputs`: lists
//! with odd levels, tracked changes, comments, notes, highlights named like `Object.prototype`
//! members, tables, images, whitespace around emphasis, seeded documents).
//!
//! Accepted difference (named in `uo-docx-d5/D5a.md`): the list level limit (`export-list-huge`, a
//! level of 70,000 that the TS indents with a 140,000-unit string and this port refuses past
//! `MAX_LIST_LEVEL`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-export.mjs`.
#![cfg(feature = "export")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d5a_support;

use d5a_support::{corpus_shard, inputs_file, open_file, package_of, read};
use uo_docx::export::{ExportDoc, HtmlOptions, ImagesMode, MarkdownOptions, to_html, to_markdown};
use uo_opc::Package;
use uo_testkit::{Capture, Replay, Stream};

fn digest(s: &str) -> String {
    let mut st = Stream::new();
    st.str(s);
    st.finish().short()
}

/// The facets of one package.
fn facets(pkg: &Package) -> Vec<(&'static str, String)> {
    let Some(doc) = read(pkg) else { return vec![("error", "err".to_owned())] };
    let ed = ExportDoc::from_document(&doc);
    let mut out = vec![
        ("html", digest(&to_html(&ed, &HtmlOptions::default()))),
        (
            "html-frag",
            digest(&to_html(&ed, &HtmlOptions { full_document: Some(false), images: Some(ImagesMode::Omit), ..HtmlOptions::default() })),
        ),
        ("html-title", digest(&to_html(&ed, &HtmlOptions { title: Some("T <&> \"x\""), ..HtmlOptions::default() }))),
    ];
    match to_markdown(&ed, &MarkdownOptions::default()) {
        Ok(md) => {
            out.push(("md", digest(&md)));
            match to_markdown(&ed, &MarkdownOptions { title: Some("Doc"), images: Some(ImagesMode::Omit) }) {
                Ok(md) => out.push(("md-omit", digest(&md))),
                Err(_) => out.push(("error-md", "err".to_owned())),
            }
        }
        Err(_) => out.push(("error-md", "err".to_owned())),
    }
    out
}

fn accepted_reason(key: &str) -> Option<&'static str> {
    (key == "quirk/d5a/export-list-huge.docx#export")
        .then_some("D5a limit: a list level past MAX_LIST_LEVEL (65,535) is refused, the TS indents it")
}

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package) {
    for (facet, value) in facets(pkg) {
        match accepted_reason(key) {
            Some(reason) if facet == "error-md" => {
                replay.accept(key, facet, &value, reason);
                replay.skip(key, "md");
                replay.skip(key, "md-omit");
            }
            _ => {
                replay.check(key, facet, &value);
            }
        }
    }
}

fn run(shard: u32, shards: u32) {
    let cap = Capture::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/export.capture")).unwrap();
    let mine = |key: &str| {
        let c = key.rsplit_once('#').unwrap().0;
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && d5a_support::shard_of(c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        let inputs = inputs_file("export.inputs");
        assert!(inputs.len() > 250, "the D5a export inputs are missing");
        for (key, parts) in inputs {
            replay_package(&mut replay, &format!("{key}#export"), &package_of(parts));
        }
    } else {
        for (rel, file) in corpus_shard(shard, shards) {
            let Some(pkg) = open_file(&file) else { continue };
            replay_package(&mut replay, &format!("{rel}#export"), &pkg);
        }
    }
    let report = replay.finish();
    println!("export parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn export_parity_shard_0() {
    run(0, 4);
}

#[test]
fn export_parity_shard_1() {
    run(1, 4);
}

#[test]
fn export_parity_shard_2() {
    run(2, 4);
}

#[test]
fn export_parity_shard_3() {
    run(3, 4);
}

#[test]
fn export_parity_quirks() {
    run(0, 0);
}
