//! Parity of D2a's text operations (record "D2a — session (architecture)", "Parity"): replays
//! `tests/parity/d2a-ops.capture`, written by `js/capture-d2a-ops.mjs` from the frozen TS — the
//! docx-core model ops through the editor's guard (`session::guard::guarded`), the settle's
//! `syncRunRPr` after every op (`session::settle::sync_paras`) and the JSON round trip of an undo
//! step (`session::normalize`), over the body paragraphs of corpus documents, with seeded op
//! scripts (typing incl. non-BMP text and one-unit deletes that leave lone surrogates, deletes
//! across paragraphs, Enter, joins, formatting with explicit-`undefined` keys, clear formatting,
//! the tracked twins with two authors, accept/reject one or all, and undo round trips).
//!
//! Facets per op: `ret` (what the op returned), `state` (the canonical digest of the paragraph
//! list), `json` (the digest of its JSON round trip). Accepted by name: R1's A3 (a document whose
//! text holds lone surrogates from character references reads U+FFFD in uo-opc): none in the
//! sample, the replay skips such records.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2a-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;
use std::sync::Arc;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::ids::IdAlloc;
use uo_docx::js::{JsNumber, JsValue};
use uo_docx::model::media::MediaStore;
use uo_docx::model::para::Paragraph;
use uo_docx::model::props::RevisionInfo;
use uo_docx::model::run::RunProps;
use uo_docx::pvec::PVec;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::guard::{guard_runs, guarded};
use uo_docx::session::normalize::normalize;
use uo_docx::session::revisions::{RevisionScope, accept_revisions, collect_revisions, reject_revisions, span_scope};
use uo_docx::session::settle::sync_paras;
use uo_docx::session::text::{ClearOptions, DocPosition, DocRange, clear_para_formatting, clear_run_formatting};
use uo_docx::word::obj;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, Stream, repo_root};

fn digest_traced(paras: &PVec<Paragraph>, media: &MediaStore, trace: Option<&str>) -> String {
    let mut s = Sink(if trace.is_some() { Stream::traced() } else { Stream::new() });
    for (i, p) in paras.iter().enumerate() {
        s.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut s).with_media(media);
        p.canon(&mut w);
    }
    if let Some(name) = trace {
        let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".to_owned());
        std::fs::write(Path::new(&dir).join(name), s.0.trace()).unwrap();
    }
    s.0.finish().short()
}

fn digest(paras: &PVec<Paragraph>, media: &MediaStore) -> String {
    digest_traced(paras, media, None)
}

fn json_digest(paras: &PVec<Paragraph>, media: &MediaStore, ids: &mut IdAlloc) -> String {
    let mut copy = paras.clone();
    normalize(&mut copy, ids);
    digest(&copy, media)
}

fn num(s: &str) -> u32 {
    s.parse().unwrap()
}

fn pos(a: &str, b: &str) -> DocPosition {
    DocPosition::new(num(a), num(b))
}

fn range(f: &[&str]) -> DocRange {
    DocRange::new(pos(f[0], f[1]), pos(f[2], f[3]))
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn patch(k: &str) -> RunProps {
    let mut p = RunProps::default();
    match k {
        "b" => obj::set(&mut p, "bold", |x| x.bold = Some(true)),
        "i" => obj::set(&mut p, "italic", |x| x.italic = Some(true)),
        "s" => obj::set(&mut p, "szHalfPt", |x| x.sz_half_pt = Some(JsNumber::Num(28.0))),
        "u" => obj::set_undefined(&mut p, "underline", |x| x.underline = None),
        "c" => obj::set(&mut p, "color", |x| x.color = Some("FF0000".into())),
        "n" => {
            obj::set(&mut p, "bold", |x| x.bold = Some(false));
            obj::set_undefined(&mut p, "italic", |x| x.italic = None);
        }
        _ => panic!("patch {k}"),
    }
    p
}

fn rev(i: &str) -> RevisionInfo {
    let (author, date, utc) = if i == "0" {
        ("A", "2026-01-02T03:04:00Z", "2026-01-02T03:04:00Z")
    } else {
        ("B", "2026-01-02T05:06:00Z", "2026-01-02T08:06:00Z")
    };
    RevisionInfo { author: author.into(), date: Some(date.into()), date_utc: Some(utc.into()), ..RevisionInfo::default() }
}

fn p_enc(p: DocPosition) -> String {
    format!("{}:{}", p.para, p.offset)
}

/// Runs one encoded op; the `ret` facet.
fn run(code: &str, paras: &mut PVec<Paragraph>, ids: &mut IdAlloc) -> String {
    let f: Vec<&str> = code.split(':').collect();
    let op = f[0];
    let a = &f[1..];
    let r = |x: Result<DocPosition, _>| x.map_or_else(|_: uo_docx::session::text::OpError| "err".to_owned(), p_enc);
    match op {
        "it" => r(guarded::insert_text(paras, pos(a[0], a[1]), &units(a[2]), None)),
        "dr" => r(guarded::delete_range(paras, range(a))),
        "sp" => r(guarded::split_paragraph(paras, ids, pos(a[0], a[1]))),
        "mg" => r(guarded::merge_with_previous(paras, num(a[0]))),
        "rp" => guarded::apply_run_props(paras, range(a), &patch(a[4])).map_or("err".to_owned(), |()| "u".to_owned()),
        "cr" => clear_run_formatting(paras, range(a), &ClearOptions::default()).map_or("err".to_owned(), |()| "u".to_owned()),
        "cp" => clear_para_formatting(paras, range(a)).map_or("err".to_owned(), |()| "u".to_owned()),
        "ti" => r(guarded::track_insert_text(paras, pos(a[0], a[1]), &units(a[2]), &rev(a[3]), None)),
        "td" => guarded::track_delete(paras, range(a), &rev(a[4])).map_or("err".to_owned(), |t| {
            format!("{}:{}:{}", p_enc(t.start), p_enc(t.end), t.merged.iter().map(u32::to_string).collect::<Vec<_>>().join("."))
        }),
        "ts" => r(guarded::track_split_paragraph(paras, ids, pos(a[0], a[1]), &rev(a[2]))),
        "tr" => guarded::track_apply_run_props(paras, range(a), &patch(a[4]), &rev(a[5])).map_or("err".to_owned(), |()| "u".to_owned()),
        "aa" | "ra" | "ac" | "rj" => {
            let scope = if op == "ac" || op == "rj" {
                let spans = collect_revisions(paras.iter());
                span_scope(&spans[num(a[0]) as usize])
            } else {
                RevisionScope::default()
            };
            let accept = op == "aa" || op == "ac";
            let hi = u32::try_from(paras.len().saturating_sub(1)).unwrap();
            let changed = guard_runs(paras, 0, hi, |p| {
                if accept { accept_revisions(p, &scope, &mut |_| {}) } else { reject_revisions(p, &scope, &mut |_| {}) }
            });
            if changed { "t" } else { "f" }.to_owned()
        }
        "rt" => {
            normalize(paras, ids);
            "t".to_owned()
        }
        _ => panic!("op {code}"),
    }
}

#[test]
fn d2a_text_ops_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2a-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let ops: usize = cap.meta("ops").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#init")) {
        let rel = rec.key.trim_end_matches("#init");
        if rec.get("a3").is_some() {
            replay.skip(&rec.key, "state");
            continue;
        }
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let mut doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        if doc.body.paras.is_empty() {
            let id = doc.ids.para();
            doc.body.paras.push(Paragraph { id, ..Paragraph::default() });
        }
        let media = doc.media.clone();
        let mut ids = std::mem::take(&mut doc.ids);
        let mut paras = std::mem::take(&mut doc.body.paras);
        sync_paras(&mut paras);
        if std::env::var("UO_PARITY_TRACE").is_ok_and(|t| t == rel) {
            digest_traced(&paras, &media, Some("rust-d2a-state.trace"));
            let mut copy = paras.clone();
            normalize(&mut copy, &mut ids);
            digest_traced(&copy, &media, Some("rust-d2a-json.trace"));
        }
        replay.check(&rec.key, "state", &digest(&paras, &media));
        replay.check(&rec.key, "json", &json_digest(&paras, &media, &mut ids));
        for j in 0..ops {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            let ret = run(code, &mut paras, &mut ids);
            sync_paras(&mut paras);
            replay.check(&key, "ret", &ret);
            replay.check(&key, "state", &digest(&paras, &media));
            replay.check(&key, "json", &json_digest(&paras, &media, &mut ids));
        }
        docs += 1;
        drop(Arc::new(()));
    }
    let report = replay.finish();
    println!("d2a-ops: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
    assert!(docs > 100);
}
