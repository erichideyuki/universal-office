//! Parity of D2b-F5's pure model rules: replays `tests/parity/d2b-f5-ops.capture`, written by
//! `js/capture-d2b-f5-ops.mjs` from the frozen editor `notes.ts` (`effectiveNoteOptions`,
//! `notePropsOf`, `ensureNoteStyles`, `nextNoteId`, `convertNoteContent`).
//!
//! Facets: per document `nid` (the next note ids), `eff` (the effective options from the settings
//! and the last section), `sty` (`ensureNoteStyles` of both kinds, the second on the sheet the first
//! changed); per note `cv:<kind>:<id>` the canonical digest of its top-level paragraphs after
//! `convertNoteContent`; and `props#<i>` (`notePropsOf` over generated options).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f5-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::model::story::Story;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::ops::notes::{NoteOptions, testing};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn n(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn digest(s: &Story, doc: &uo_docx::model::Document) -> String {
    let mut sink = Sink(Stream::new());
    for (i, p) in s.paras.iter().enumerate() {
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        p.canon(&mut w);
    }
    sink.0.finish().short()
}

fn parse_num(s: &str) -> f64 {
    match s {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        _ => uo_jsnum::parse::to_number(s),
    }
}

#[test]
fn d2b_f5_model_rules_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f5-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut replay = Replay::new(&cap);
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.starts_with("props#")) {
        let code = rec.get("op").unwrap();
        replay.check(&rec.key, "op", code);
        let f: Vec<&str> = code.split('|').collect();
        let footnote = f[0] == "footnote";
        let o = NoteOptions {
            position: f[1].to_owned(),
            number_format: f[2].to_owned(),
            start_at: parse_num(f[3]),
            restart: f[4].to_owned(),
            columns: None,
        };
        replay.check(&rec.key, "ret", &testing::props_of(footnote, &o, f[5] == "1"));
    }
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#init")) {
        let rel = rec.key.trim_end_matches("#init");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        replay.check(&rec.key, "nid", &format!("{},{}", n(testing::next_id(&doc, true)), n(testing::next_id(&doc, false))));
        let s = &doc.formatting.settings;
        let sp = doc.sections.last().map(|sec| &*sec.sect_pr);
        let eff_f = testing::effective(
            true,
            s.footnote_pr.as_ref(),
            sp.and_then(|p| p.footnote_pr.as_ref()),
            sp.and_then(|p| p.footnote_columns).map(|c| c.to_number()),
        );
        let eff_e = testing::effective(false, s.endnote_pr.as_ref(), sp.and_then(|p| p.endnote_pr.as_ref()), None);
        replay.check(&rec.key, "eff", &format!("{eff_f}#{eff_e}"));
        let xml0 = pkg.part_text("word/styles.xml").unwrap().map(|t| t.into_owned());
        let xml0 = xml0.unwrap_or_else(|| "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:styles>".to_owned());
        let (x1, ft, fr) = testing::ensure_note_styles(&xml0, true);
        let (x2, et, er) = testing::ensure_note_styles(&x1, false);
        replay.check(&rec.key, "sty", &format!("{},{ft},{fr}#{},{et},{er}", sha(&x1), sha(&x2)));
        let from_to = |kind_foot: bool| -> ((String, String), (String, String)) {
            let (ft, fr) = (ft.clone(), fr.clone());
            let (et, er) = (et.clone(), er.clone());
            if kind_foot { ((ft, fr), (et, er)) } else { ((et, er), (ft, fr)) }
        };
        for (footnote, map) in [(true, &doc.notes.footnotes), (false, &doc.notes.endnotes)] {
            let kind = if footnote { "footnote" } else { "endnote" };
            for (id, story) in map.iter().filter(|(k, _)| k.get() > 0.0).take(4) {
                let key = format!("{rel}#cv:{kind}:{}", n(id.get()));
                let (from, to) = from_to(footnote);
                let conv = testing::convert_content(story, !footnote, (&from.0, &from.1), (&to.0, &to.1));
                replay.check(&key, "digest", &digest(&conv, &doc));
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f5-ops: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
    assert!(docs > 50);
}
