//! Parity of the D2a session against the frozen `<universal-write>` element in Chromium (G17):
//! replays `tests/parity/d2a-session.capture`, written by `js/capture-d2a-session.mjs` (Playwright,
//! the element's public and TS-private entry points, seeded scripts of typing, Enter, Backspace,
//! Delete, selections, undo, redo and track changes over corpus documents). Per step: the body as
//! an undo would restore it (`paras`: the canonical digest of `parseState(stringifyState()).m.
//! paragraphs` against the session's body normalised), the selection, the table anchors and the
//! bookmark paragraphs (`anc`), the model-derived events in order (`ev`), dirty / can undo / can
//! redo / edits since load (`flags`), and every 8 steps the saved `word/document.xml`.
//!
//! Accepted by name: the events of the install (`loadDocx` dispatches `uw-protected`; the session
//! opens without a change set): the `ev` facet of step 0 is not compared (D2a-E1).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2a-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use d2a_support::Cx;
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

fn paras_digest(s: &Session) -> String {
    let doc = s.document();
    let mut paras = doc.body.paras.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
    let mut sink = Sink(Stream::new());
    for (i, p) in paras.iter().enumerate() {
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        p.canon(&mut w);
    }
    sink.0.finish().short()
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn sel_json(s: &Session) -> String {
    let sel = s.selection();
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(sel.anchor), pos_json(sel.focus))
}

fn anc_json(s: &Session) -> String {
    let t: Vec<String> = s.document().body.tables.iter().map(|a| a.before_para.to_string()).collect();
    let b: Vec<String> = s.bookmark_paras().iter().map(|(k, v)| format!("[{},{v}]", uo_jsnum::json::string(k))).collect();
    format!("{{\"t\":[{}],\"b\":[{}]}}", t.join(","), b.join(","))
}

fn ev_text(cs: &ChangeSet) -> String {
    let e: Vec<String> = cs
        .events
        .iter()
        .map(|e| match e {
            Event::Change => "uw-change:".to_owned(),
            Event::DirtyChange { dirty } => format!("uw-dirty-change:{dirty}"),
            Event::Input { .. } => "uw-input:".to_owned(),
            Event::TrackChanges { on } => format!("uw-track-changes:{on}"),
            Event::CellMode { active } => format!("uw-cellmode:{active}"),
            Event::NoteMode { active, .. } => format!("uw-notemode:{active}"),
            Event::HfMode { active } => format!("uw-hfmode:{active}"),
            Event::TbMode { active } => format!("uw-tbmode:{active}"),
            Event::Protected { active, .. } => format!("uw-protected:{active}"),
            Event::Blocked { .. } => "uw-blocked:".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn docxml(s: &mut Session, dump: bool) -> String {
    match s.save_docx() {
        Ok(bytes) => {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
            if dump {
                println!("RUSTXML {t}");
            }
            Digest::of(t.as_bytes()).short()
        }
        Err(e) => {
            if dump {
                println!("RUSTXML error {e}");
            }
            "err".to_owned()
        }
    }
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn step(s: &mut Session, code: &str, cx: &mut Cx) -> ChangeSet {
    let f: Vec<&str> = code.split(':').collect();
    let n = |i: usize| f[i].parse::<u32>().unwrap();
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(n(1), n(2)), focus: DocPosition::new(n(3), n(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "un" => return s.undo(&mut cx.cx()),
        "re" => return s.redo(&mut cx.cx()),
        _ => panic!("step {code}"),
    };
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => cs,
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in D2a scripts"),
        Err(e) => panic!("{code}: {e}"),
    }
}

/// A step that reaches a rule of a D2b family (the hooks of `session::ops::text`): from there on the
/// document's facets were accepted by name until the family landed. D2b-F2 ported the list rules
/// (`listEnterOnEmpty`, `listBackspaceAtStart`) and `applyNextStyle`, so nothing is accepted any more
/// (D2a-T4 resolved: the 264 facets it accepted are equalities now).
fn d2b_hook(_s: &Session, _code: &str) -> Option<&'static str> {
    None
}

#[test]
fn d2a_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2a-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        // `UO_D2A_SIG=canon`: the canonical signatures of G23 (the U6 count); default `Ts` (D2a-U6)
        let sig_mode = if std::env::var("UO_D2A_SIG").is_ok_and(|v| v == "canon") { SigMode::Canonical } else { SigMode::Ts };
        let mut s = Session::open(
            bytes,
            OpenOptions { sig_mode, ts_formula_errors: true, ts_numbering: true, ..OpenOptions::default() },
            Box::new(FixedEnv::default()),
        )
        .unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        let mut pending: Option<&str> = None;
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if pending.is_none() {
                pending = d2b_hook(&s, code);
            }
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let cs = step(&mut s, code, &mut cx);
                if let Some(why) = pending {
                    let mut facets = vec![
                        ("ev", ev_text(&cs)),
                        ("paras", paras_digest(&s)),
                        ("sel", sel_json(&s)),
                        ("anc", anc_json(&s)),
                        ("flags", flags(&s)),
                    ];
                    if cap.get(&key, "docxml").is_some() {
                        facets.push(("docxml", docxml(&mut s, false)));
                    }
                    for (f, actual) in facets {
                        replay.accept(&key, f, &actual, why);
                    }
                    continue;
                }
                replay.check(&key, "ev", &ev_text(&cs));
            }
            if let Ok(d) = std::env::var("UO_D2A_DUMP")
                && let Some(para) = d.strip_prefix(&format!("{key}:")).and_then(|p| p.parse::<usize>().ok())
            {
                let mut paras = s.document().body.paras.clone();
                let mut ids = s.document().ids.clone();
                uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                let p = paras.get(para).unwrap();
                println!("RUST {}", uo_docx::js::JsonWriter::stringify(p, Some(&s.document().media)).unwrap());
            }
            replay.check(&key, "paras", &paras_digest(&s));
            replay.check(&key, "sel", &sel_json(&s));
            replay.check(&key, "anc", &anc_json(&s));
            replay.check(&key, "flags", &flags(&s));
            if cap.get(&key, "docxml").is_some() {
                let dump = std::env::var("UO_D2A_DUMP").is_ok_and(|d| d == format!("{key}:xml"));
                let d = docxml(&mut s, dump);
                replay.check(&key, "docxml", &d);
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2a-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
