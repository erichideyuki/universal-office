//! Parity of D2b-F6's pure model rules: replays `tests/parity/d2b-f6-ops.capture`, written by
//! `js/capture-d2b-f6-ops.mjs` from the frozen editor `references.ts` (`bookmarksIn`,
//! `findBookmark`, `removeBookmarkMarks`, `addBookmarkMarks`, `textBetween`, `unlinkRuns`) and the
//! guarded `applyRunProps` that sets a link, with the settle's `syncRunRPr` after every op and the
//! JSON round trip of an undo step, over the body paragraphs of corpus documents.
//!
//! Facets per op: `ret` (what the op returned), `state` (the canonical digest of the paragraph
//! list), `json` (the digest of its JSON round trip). Documents whose text holds lone surrogates
//! from character references (R1's A3) are skipped from the init record on, as in D2a.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f6-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::ids::IdAlloc;
use uo_docx::js::JsValue;
use uo_docx::model::media::MediaStore;
use uo_docx::model::para::Paragraph;
use uo_docx::model::run::RunProps;
use uo_docx::pvec::PVec;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::guard::{guard_runs, guarded};
use uo_docx::session::normalize::normalize;
use uo_docx::session::ops::bookmarks::{
    BookmarkLoc, add_bookmark_marks, bookmarks_in, find_bookmark, remove_bookmark_marks, text_between, unlink_runs,
};
use uo_docx::session::settle::sync_paras;
use uo_docx::session::text::{DocPosition, DocRange, normalize_range, para_length};
use uo_docx::word::obj;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, Stream, repo_root};

const NAMES: [&str; 6] = ["Alfa", "alfa", "Beta", "_oculto", "Gama1", "é2"];
const URLS: [&str; 2] = ["https://example.com/a", "mailto:a@b.c"];

fn digest(paras: &PVec<Paragraph>, media: &MediaStore) -> String {
    let mut s = Sink(Stream::new());
    for (i, p) in paras.iter().enumerate() {
        s.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut s).with_media(media);
        p.canon(&mut w);
    }
    s.0.finish().short()
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

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn enc(list: &[BookmarkLoc]) -> String {
    let v: Vec<String> = list
        .iter()
        .map(|b| {
            let name: Vec<u16> = b.name.encode_utf16().collect();
            format!(
                "{}:{}:{}:{}:{}:{}",
                hx(&name),
                uo_jsnum::format::to_js_string(b.id.to_number()),
                b.start.para,
                b.start.offset,
                b.end.para,
                b.end.offset
            )
        })
        .collect();
    if v.is_empty() { "-".to_owned() } else { v.join("|") }
}

fn run(code: &str, paras: &mut PVec<Paragraph>, ids: &mut IdAlloc, next: &mut f64) -> String {
    let f: Vec<&str> = code.split(':').collect();
    let a = &f[1..];
    match f[0] {
        "ab" => {
            let name = NAMES[num(a[4]) as usize];
            remove_bookmark_marks(paras, name);
            let r = normalize_range(range(a));
            let id = *next;
            *next += 1.0;
            add_bookmark_marks(paras, name, id, r.start, r.end).map_or("err".to_owned(), |()| "t".to_owned())
        }
        "rb" => if remove_bookmark_marks(paras, NAMES[num(a[0]) as usize]) { "t" } else { "f" }.to_owned(),
        "bi" => enc(&bookmarks_in(paras.iter())),
        "fb" => find_bookmark(paras.iter(), NAMES[num(a[0]) as usize]).map_or("u".to_owned(), |b| enc(&[b])),
        "tb" => {
            let r = normalize_range(range(a));
            let t = text_between(paras, r.start, r.end);
            let h = hx(t.units());
            if h.is_empty() { "-".to_owned() } else { h }
        }
        "lk" => {
            let r = normalize_range(range(a));
            let mut patch = RunProps::default();
            obj::set(&mut patch, "link", |p| p.link = Some(URLS[num(a[4]) as usize].into()));
            obj::set(&mut patch, "charStyle", |p| p.char_style = Some("Hyperlink".into()));
            guarded::apply_run_props(paras, DocRange::new(r.start, r.end), &patch).map_or("err".to_owned(), |()| "u".to_owned())
        }
        "ul" => {
            let r = normalize_range(range(a));
            guard_runs(paras, r.start.para, r.end.para, |m| {
                for pi in r.start.para..=r.end.para {
                    let Some(p) = m.make_mut(pi as usize) else { return };
                    let from = if pi == r.start.para { r.start.offset as usize } else { 0 };
                    let to = if pi == r.end.para { r.end.offset as usize } else { para_length(p) };
                    unlink_runs(p, from, to, "Hyperlink");
                }
            });
            "u".to_owned()
        }
        "rt" => {
            normalize(paras, ids);
            "t".to_owned()
        }
        _ => panic!("op {code}"),
    }
}

fn max_id(paras: &PVec<Paragraph>) -> f64 {
    let mut max = -1.0_f64;
    for p in paras.iter() {
        for r in &p.runs {
            if let Some(b) = r.ext().and_then(|e| e.bookmark.as_ref()) {
                max = uo_jsnum::math::max(max, b.id.to_number());
            }
        }
    }
    max
}

#[test]
fn d2b_f6_model_rules_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f6-ops.capture")).unwrap();
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
        replay.check(&rec.key, "state", &digest(&paras, &media));
        replay.check(&rec.key, "json", &json_digest(&paras, &media, &mut ids));
        let mut next = max_id(&paras) + 1.0;
        replay.check(&rec.key, "next", &uo_jsnum::format::to_js_string(next));
        for j in 0..ops {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            let ret = run(code, &mut paras, &mut ids, &mut next);
            sync_paras(&mut paras);
            replay.check(&key, "ret", &ret);
            replay.check(&key, "state", &digest(&paras, &media));
            replay.check(&key, "json", &json_digest(&paras, &media, &mut ids));
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f6-ops: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
    assert!(docs > 50);
}
