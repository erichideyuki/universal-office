//! Parity of D2b-F5 (footnotes and endnotes) against the frozen `<universal-write>` element in
//! Chromium (G17): replays `tests/parity/d2b-f5-session.capture`, written by
//! `js/capture-d2b-f5-session.mjs`. Seeded scripts over corpus documents (those with notes or
//! several sections first) mix typing, selections and tracking with the note commands: insert a
//! footnote or endnote (with and without a custom mark), the options at the document or the
//! section (defaults, restart rules, rounding, the place, columns), convert (all, swap, one),
//! open/go to/next/show, leaving a note, entering and leaving a cell, undo and redo. Facets per
//! step: the D2a ones (`paras`, `sel`, `anc`, `ev`, `flags`) and F5's: `ctx` (body, note or cell),
//! `no` (the note options of both kinds and the counts), `np` (the note settings JSON), `ref` (the
//! references of the body), `nts` (the content of every note), `sect` (the note properties of
//! every section's `sectPr`) and, with each save, `docxml` and `parts` (`footnotes.xml`,
//! `endnotes.xml`, `settings.xml`, `styles.xml`, the content types and the rels).
//!
//! Accepted by name: the events of the install (step 0), and — from the step on — the list rules of
//! D2b-F2 (Enter on an empty list item, Backspace at the start of one: D2a-T4), as in the D2a
//! replay.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f5-session.mjs`.
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
use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::session::ops::context::{ContextCmd, EditContext};
use uo_docx::session::ops::notes::{ConvertMode, NoteCmd, NoteOptionsPatch, NoteScope};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

const MARKS: [Option<&str>; 4] = [None, Some("*"), Some("  "), Some(" † ")];
const MODES: [ConvertMode; 3] = [ConvertMode::ToEndnotes, ConvertMode::ToFootnotes, ConvertMode::Swap];

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

fn sel_of(sel: uo_docx::session::Selection) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(sel.anchor), pos_json(sel.focus))
}

/// `state.s`: the body's selection (in a cell or a note, the one saved on entering).
fn sel_json(s: &Session) -> String {
    sel_of(uo_docx::session::ops::context::saved_selection(s.context()).unwrap_or(s.selection()))
}

/// `(state.ce ?? state.nt)?.s`: the selection inside a cell or a note.
fn csel_json(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { .. } | EditContext::Note { .. } => sel_of(s.selection()),
        _ => "null".to_owned(),
    }
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
            Event::StylesChange => "uw-styles-change:".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn saved_parts(s: &mut Session) -> Option<uo_opc::Package> {
    let bytes = s.save_docx().ok()?;
    uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).ok()
}

fn part_digest(pkg: &uo_opc::Package, name: &str) -> String {
    let t = pkg.part_text(name).unwrap().map(|t| t.into_owned());
    Digest::of(t.as_deref().unwrap_or("-").as_bytes()).short()
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn n(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn patch(i: usize) -> NoteOptionsPatch {
    let mut p = NoteOptionsPatch::default();
    let s = |x: &str| Some(x.to_owned());
    match i {
        0 => p.number_format = s("lowerLetter"),
        1 => p.start_at = Some(5.0),
        2 => p.restart = s("eachSect"),
        3 => p.restart = s("eachPage"),
        4 => p.position = s("beneathText"),
        5 => p.position = s("sectEnd"),
        6 => p.columns = Some(2.0),
        7 => p.columns = Some(0.0),
        8 => {
            p.number_format = s("decimal");
            p.start_at = Some(1.0);
            p.restart = s("continuous");
            p.position = s("pageBottom");
        }
        9 => p.start_at = Some(0.4),
        10 => p.start_at = Some(-3.0),
        11 => {
            p.number_format = s("upperRoman");
            p.restart = s("eachSect");
            p.start_at = Some(7.0);
        }
        12 => p.position = s("docEnd"),
        13 => {
            p.number_format = s("chicago");
            p.start_at = Some(2.5);
        }
        _ => panic!("options {i}"),
    }
    p
}

fn step(s: &mut Session, code: &str, cx: &mut Cx) -> Result<ChangeSet, String> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let ni = |i: usize| f[i].parse::<i64>().unwrap();
    let refs = s.note_refs(None);
    let ref_at = |i: i64| usize::try_from(i).ok().and_then(|i| refs.get(i)).map(|r| (r.footnote, r.id));
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "nf" => Command::Note(NoteCmd::Insert { footnote: nn(1) == 0, mark: MARKS[nn(2) as usize].map(Utf16::from) }),
        "no" => Command::Note(NoteCmd::SetOptions {
            footnote: nn(1) == 0,
            patch: patch(nn(2) as usize),
            scope: if f[3] == "1" { NoteScope::Section } else { NoteScope::Document },
        }),
        "cv" => Command::Note(NoteCmd::Convert { mode: MODES[nn(1) as usize], only: ref_at(ni(2)) }),
        "on" => {
            let (footnote, id) = ref_at(ni(1)).unwrap_or((true, 99999.0));
            Command::Note(NoteCmd::Open { footnote, id })
        }
        "gr" => {
            let (footnote, id) = ref_at(ni(1)).unwrap_or((true, 99999.0));
            Command::Note(NoteCmd::GoToRef { footnote, id })
        }
        "nn" => Command::Note(NoteCmd::Next { footnote: nn(1) == 0, forward: nn(2) == 1 }),
        "sh" => Command::Note(NoteCmd::Show { footnote: if nn(1) == 2 { None } else { Some(nn(1) == 0) } }),
        "xn" => Command::Context(ContextCmd::ExitNote),
        "ec" => Command::Context(ContextCmd::EnterCell { anchor: nn(1), hops: vec![CellHop { row: 0, col: 0, block: None }], caret: None }),
        "xc" => Command::Context(ContextCmd::ExitCell),
        "un" => return Ok(s.undo(&mut cx.cx())),
        "re" => return Ok(s.redo(&mut cx.cx())),
        _ => panic!("step {code}"),
    };
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => Ok(cs),
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F5 scripts"),
        Err(e) => Err(format!("{code}: {e}")),
    }
}

fn ctx_text(s: &Session) -> String {
    match s.context() {
        EditContext::Note { footnote, id, .. } => format!("note:{}:{}", if *footnote { "footnote" } else { "endnote" }, n(*id)),
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> = hops.iter().map(|h| format!("[{},{}]", h.row, h.col)).collect();
            format!("cell:{anchor}:[{}]", h.join(","))
        }
        _ => "body".to_owned(),
    }
}

fn no_text(s: &Session) -> String {
    let o = |footnote: bool| {
        let o = s.note_options(footnote);
        format!("{},{},{},{},{}", o.position, o.number_format, n(o.start_at), o.restart, o.columns.map_or("-".to_owned(), n))
    };
    format!("{}#{}#{}#{}", o(true), o(false), s.note_count(true), s.note_count(false))
}

fn ref_text(s: &Session) -> String {
    s.note_refs(None)
        .iter()
        .map(|r| {
            let cell = r.cell.map_or("-".to_owned(), |(a, h)| format!("{a}.{}.{}", h.row, h.col));
            format!("{}:{}:{}:{cell}:{}", if r.footnote { "footnote" } else { "endnote" }, n(r.id), r.para, r.offset)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn run_sig(r: &uo_docx::model::run::Run) -> String {
    let p = &r.props;
    let field = r
        .ext()
        .and_then(|e| e.field.as_deref())
        .filter(|f| !f.is_empty())
        .map_or(String::new(), |f| hx(&f.encode_utf16().collect::<Vec<_>>()));
    let note = r
        .ext()
        .and_then(|e| e.note_ref.as_ref())
        .map_or(String::new(), |nr| format!("{}:{}:{}", nr.kind, n(nr.id.to_number()), u8::from(nr.custom == Some(true))));
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        hx(r.text.units()),
        p.char_style.as_deref().unwrap_or(""),
        p.vert_align.as_deref().unwrap_or(""),
        p.ins.as_deref().map_or("", |i| &i.author),
        p.del.as_deref().map_or("", |i| &i.author),
        field,
        note
    )
}

fn nts_text(s: &Session) -> String {
    let side = |footnote: bool| -> String {
        let doc = s.document();
        let map = if footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
        let mut items: Vec<_> = map.iter().collect();
        items.sort_by(|a, b| a.0.get().total_cmp(&b.0.get()));
        items
            .iter()
            .map(|(id, st)| {
                let paras: Vec<String> = st
                    .paras
                    .iter()
                    .map(|p| {
                        format!(
                            "{}:{}",
                            p.props.style_id.as_deref().unwrap_or(""),
                            p.runs.iter().map(run_sig).collect::<Vec<_>>().join(",")
                        )
                    })
                    .collect();
                format!("{}={}", n(id.get()), paras.join(";"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{}\n#\n{}", side(true), side(false))
}

fn sect_text(s: &Session) -> String {
    let of = |sp: &uo_docx::model::section::SectPr| {
        let j = |p: &Option<uo_docx::model::formatting::NoteProps>| {
            p.as_ref().and_then(|p| JsonWriter::stringify(p, None)).unwrap_or_else(|| "null".to_owned())
        };
        format!("{}|{}|{}", j(&sp.footnote_pr), j(&sp.endnote_pr), sp.footnote_columns.map_or(String::new(), |c| n(c.to_number())))
    };
    let mut out: Vec<String> = Vec::new();
    for p in s.document().body.paras.iter() {
        if let Some(sb) = p.props.section_break.as_deref() {
            out.push(of(&sb.sect_pr));
        }
    }
    out.push(of(&s.section_state().body_sect_pr));
    out.join("\n")
}

/// A step that reaches a rule of a D2b family that has not landed (the hooks of `session::ops::text`).
fn d2b_hook(s: &Session, code: &str) -> Option<&'static str> {
    let sel = s.selection();
    if sel.anchor != sel.focus {
        return None;
    }
    let p = s.paras()?.get(sel.focus.para as usize)?;
    let len = uo_docx::session::text::para_length(p);
    let style = p.props.style_id.as_deref().and_then(|id| s.document().styles.get(id));
    let sheet = &s.document().formatting.styles.styles;
    let mut id = p.props.style_id.clone();
    let mut style_num = false;
    for _ in 0..32 {
        let Some(def) = id.as_deref().and_then(|i| sheet.get(i)) else { break };
        if def.p_pr.as_deref().is_some_and(|pp| pp.num_pr.is_some()) {
            style_num = true;
            break;
        }
        id = def.based_on.clone();
    }
    let numbered = p.props.numbering.is_some() || style_num;
    let list_like = numbered
        || style.and_then(|st| st.name.as_deref()).is_some_and(|n| n.eq_ignore_ascii_case("list paragraph"))
        || p.props.indent_left_twips == Some(uo_docx::js::JsNumber::Num(360.0));
    match code {
        "en" if numbered && len == 0 => Some("D2b-F2 list rule: listEnterOnEmpty (D2a-T4)"),
        "bs" if list_like && sel.focus.offset == 0 => Some("D2b-F2 list rule: listBackspaceAtStart (D2a-T4)"),
        _ => None,
    }
}

#[test]
fn d2b_f5_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f5-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let mut s =
            Session::open(bytes, OpenOptions { sig_mode: SigMode::Ts, ..OpenOptions::default() }, Box::new(FixedEnv::default())).unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        let mut pending: Option<&str> = None;
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if pending.is_none() {
                pending = d2b_hook(&s, code);
            }
            let mut ev = None;
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let cs = match step(&mut s, code, &mut cx) {
                    Ok(cs) => cs,
                    Err(e) => panic!("{rel}#{j}: {e}"),
                };
                ev = Some(ev_text(&cs));
            }
            let mut facets = vec![
                ("paras", paras_digest(&s)),
                ("sel", sel_json(&s)),
                ("csel", csel_json(&s)),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("ctx", ctx_text(&s)),
                ("no", no_text(&s)),
                ("np", s.note_settings().json()),
                ("ref", sha(&ref_text(&s))),
                ("nts", sha(&nts_text(&s))),
                ("sect", sha(&sect_text(&s))),
            ];
            if let Some(ev) = ev {
                facets.push(("ev", ev));
            }
            if cap.get(&key, "docxml").is_some() {
                match saved_parts(&mut s) {
                    Some(pkg) => {
                        facets.push(("docxml", part_digest(&pkg, "word/document.xml")));
                        let parts: Vec<String> = [
                            "word/footnotes.xml",
                            "word/endnotes.xml",
                            "word/settings.xml",
                            "word/styles.xml",
                            "[Content_Types].xml",
                            "word/_rels/document.xml.rels",
                        ]
                        .iter()
                        .map(|p| format!("{p}={}", part_digest(&pkg, p)))
                        .collect();
                        facets.push(("parts", parts.join(",")));
                        if let Ok(dir) = std::env::var("UO_F5_DUMP")
                            && std::env::var("UO_F5_KEY").is_ok_and(|k| k == key)
                        {
                            for p in
                                ["word/document.xml", "word/footnotes.xml", "word/endnotes.xml", "word/settings.xml", "word/styles.xml"]
                            {
                                let t = pkg.part_text(p).unwrap().map(|t| t.into_owned()).unwrap_or_default();
                                std::fs::write(Path::new(&dir).join(format!("rust-{j}-{}", p.rsplit('/').next().unwrap())), t).unwrap();
                            }
                        }
                    }
                    None => {
                        facets.push(("docxml", "err".to_owned()));
                        facets.push(("parts", "err".to_owned()));
                    }
                }
            }
            if std::env::var("UO_F5_DUMP").is_ok_and(|d| d == key) {
                println!("RUST ref\n{}\nRUST nts\n{}\nRUST sect\n{}", ref_text(&s), nts_text(&s), sect_text(&s));
            }
            for (f, actual) in facets {
                match pending {
                    Some(why) if j > 0 => {
                        replay.accept(&key, f, &actual, why);
                    }
                    _ => {
                        replay.check(&key, f, &actual);
                    }
                }
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f5-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
