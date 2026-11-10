//! Parity of the D2b-F3 commands (tables) against the frozen `<universal-write>` element in
//! Chromium (G17, with the uo-jsnum math override): replays `tests/parity/d2b-f3-session.capture`,
//! written by `js/capture-d2b-f3-session.mjs` (Playwright, the element's public methods and the
//! private entries the shell reaches, seeded scripts that mix the table commands with typing,
//! Enter, Backspace, Delete, selections, undo, redo and track changes over corpus documents that
//! have tables). Per step: the body as an undo would restore it (`paras`), the tables of that state
//! (`tbl`), the selection (`sel`), the table anchors and bookmark paragraphs (`anc`), the cell
//! being edited (`ctx`), the selection of cells (`ts`), the model-derived events (`ev`), dirty /
//! can undo / can redo / edits since load (`flags`) and, every 8 steps, the saved
//! `word/document.xml`. What the TS reads from the layout (`lay`: the content width and the
//! displayed row heights) is handed to the command from the capture.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f3-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f3_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use d2a_support::Cx;
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::context::{EditContext, saved_selection};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Session};
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

/// The tables of the state an undo restores (`state.tb`): the JSON copy of each.
fn tables_digest(s: &Session) -> String {
    let doc = s.document();
    let mut ids = doc.ids.clone();
    let mut sink = Sink(Stream::new());
    for (i, a) in doc.body.tables.iter().enumerate() {
        let mut t = (*a.table).clone();
        uo_docx::session::normalize::normalize(&mut t, &mut ids);
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        t.canon(&mut w);
    }
    sink.0.finish().short()
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn sel_text(sel: uo_docx::session::Selection) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(sel.anchor), pos_json(sel.focus))
}

/// `state.s`: the body's selection (the saved one while a cell is edited).
fn sel_json(s: &Session) -> String {
    sel_text(saved_selection(s.context()).unwrap_or_else(|| s.selection()))
}

fn anc_json(s: &Session) -> String {
    let t: Vec<String> = s.document().body.tables.iter().map(|a| a.before_para.to_string()).collect();
    let b: Vec<String> = s.bookmark_paras().iter().map(|(k, v)| format!("[{},{v}]", uo_jsnum::json::string(k))).collect();
    format!("{{\"t\":[{}],\"b\":[{}]}}", t.join(","), b.join(","))
}

fn hop_json(h: &uo_docx::session::CellHop) -> String {
    match h.block {
        Some(b) => format!("{{\"r\":{},\"c\":{},\"b\":{b}}}", h.row, h.col),
        None => format!("{{\"r\":{},\"c\":{}}}", h.row, h.col),
    }
}

/// `JSON.stringify(state.ce)`: `{ a, h: [{ r, c, b? }], s }`.
fn ctx_json(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> = hops.iter().map(hop_json).collect();
            format!("{{\"a\":{anchor},\"h\":[{}],\"s\":{}}}", h.join(","), sel_text(s.selection()))
        }
        _ => "null".to_owned(),
    }
}

/// `JSON.stringify(state.ts)`: `{ anchor, path, rect, origin }`.
fn ts_json(s: &Session) -> String {
    let Some(ts) = s.table_selection() else { return "null".to_owned() };
    let h: Vec<String> = ts.path.iter().map(hop_json).collect();
    format!(
        "{{\"anchor\":{},\"path\":[{}],\"rect\":{{\"r1\":{},\"c1\":{},\"r2\":{},\"c2\":{}}},\"origin\":{{\"r\":{},\"c\":{}}}}}",
        ts.anchor,
        h.join(","),
        ts.row0,
        ts.col0,
        ts.row1,
        ts.col1,
        ts.origin_row,
        ts.origin_col
    )
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
            Event::SortError { .. } => "uw-sort-error:".to_owned(),
            Event::StylesChange => "uw-styles-change:".to_owned(),
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

#[test]
fn d2b_f3_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f3-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let sig_mode = if std::env::var("UO_D2A_SIG").is_ok_and(|v| v == "canon") { SigMode::Canonical } else { SigMode::Ts };
        let mut s = Session::open(bytes, OpenOptions { sig_mode, ..OpenOptions::default() }, Box::new(FixedEnv::default())).unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if let Some(why) = cap.get(&key, "thr") {
                // the TS threw on this step (a table the editor cannot handle): the document ends here
                replay.skip(&key, "thr");
                println!("{key}: the TS threw: {why}");
                break;
            }
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let cs = d2b_f3_support::session_codes::step(&mut s, code, cap.get(&key, "lay"), &mut cx);
                replay.check(&key, "ev", &ev_text(&cs));
            }
            if let Ok(d) = std::env::var("UO_F3_DUMP")
                && d == key
            {
                let tables: Vec<String> = s
                    .document()
                    .body
                    .tables
                    .iter()
                    .map(|a| {
                        let mut t = (*a.table).clone();
                        let mut ids = s.document().ids.clone();
                        uo_docx::session::normalize::normalize(&mut t, &mut ids);
                        uo_docx::js::JsonWriter::stringify(&t, Some(&s.document().media)).unwrap()
                    })
                    .collect();
                println!("RUST [{}]", tables.join(","));
            }
            if let Ok(d) = std::env::var("UO_F3_DUMP")
                && let Some(para) = d.strip_prefix(&format!("{key}:p")).and_then(|p| p.parse::<usize>().ok())
            {
                let mut paras = s.document().body.paras.clone();
                let mut ids = s.document().ids.clone();
                uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                println!("RUST {}", uo_docx::js::JsonWriter::stringify(paras.get(para).unwrap(), Some(&s.document().media)).unwrap());
            }
            replay.check(&key, "paras", &paras_digest(&s));
            replay.check(&key, "tbl", &tables_digest(&s));
            replay.check(&key, "sel", &sel_json(&s));
            replay.check(&key, "anc", &anc_json(&s));
            replay.check(&key, "ctx", &ctx_json(&s));
            replay.check(&key, "ts", &ts_json(&s));
            replay.check(&key, "flags", &flags(&s));
            if cap.get(&key, "docxml").is_some() {
                let dump = std::env::var("UO_D2A_DUMP").is_ok_and(|d| d == format!("{key}:xml"));
                let d = docxml(&mut s, dump);
                replay.check(&key, "docxml", &d);
            }
            if cap.get(&key, "lay").is_some() {
                replay.skip(&key, "lay");
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f3-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
