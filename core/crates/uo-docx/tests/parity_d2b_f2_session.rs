//! Parity of the D2b-F2 session (lists and styles) against the frozen `<universal-write>` element
//! in Chromium: replays `tests/parity/d2b-f2-session.capture`, written by
//! `js/capture-d2b-f2-session.mjs` (the D2a harness plus seeded scripts of the lists and styles
//! commands). Per step the facets of the D2a capture (`paras`, `sel`, `anc`, `ev`, `flags`) plus
//! `sv`/`nv` (the style sheet and numbering text of now) and, every 8 steps, the saved
//! `word/document.xml`, `word/numbering.xml` and `word/styles.xml` (the stores' saves).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f2-session.mjs`.
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
use uo_docx::session::ops::lists::{ListCmd, ListLevelFormat, ListLevelSpec};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::{Ch, CreateStyleSpec, DefaultFont, ModifyStyleSpec, StyleCmd, StyleFormat, UiLang};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

const NAMES: [&str; 4] = ["Meu Estilo", "Destaque X", "Citação", "Nota 1"];

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
            Event::StylesChange => "uw-styles-change:".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn lvl(i: u32, num_fmt: &str, text: Option<&str>) -> ListLevelSpec {
    ListLevelSpec {
        ilvl: f64::from(i),
        num_fmt: num_fmt.to_owned(),
        lvl_text: text.map_or_else(|| format!("%{}.", i + 1), str::to_owned),
        ind_left: 720.0 * f64::from(i + 1),
        hanging: 360.0,
        ..ListLevelSpec::default()
    }
}

fn text_digest(t: Option<&str>) -> String {
    t.map_or_else(|| "-".to_owned(), |t| Digest::of(t.as_bytes()).short())
}

fn part_digest(pkg: &uo_opc::Package, name: &str) -> String {
    let t = pkg.part_text(name).unwrap();
    text_digest(t.as_deref())
}

fn command(code: &str) -> Option<Command> {
    let f: Vec<&str> = code.splitn(5, ':').collect();
    let n = |i: usize| f[i].parse::<u32>().unwrap();
    let idx = |i: usize| (f[i] != "-").then(|| f[i].parse::<usize>().unwrap());
    Some(match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(n(1), n(2)), focus: DocPosition::new(n(3), n(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "un" | "re" | "init" => return None,
        "tl" => Command::List(ListCmd::ToggleList { bullet: f[1] == "b" }),
        "al" => Command::List(ListCmd::ApplyListLibrary { bullet: f[1] == "b", index: idx(2) }),
        "rl" => Command::List(ListCmd::RemoveList),
        "ml" => Command::List(ListCmd::ApplyMultilevelLibrary { index: idx(1) }),
        "sl" => Command::List(ListCmd::SetListLevel { ilvl: f64::from(n(1)) }),
        "ci" => Command::List(ListCmd::ChangeListLevel { delta: f[1].parse::<i8>().unwrap() }),
        "rs" => Command::List(ListCmd::RestartNumbering { start: f.get(1).map(|v| uo_jsnum::parse::parse_int(v, 10)) }),
        "cn" => Command::List(ListCmd::ContinueNumbering),
        "nv" => Command::List(ListCmd::SetNumberingValue {
            new_list: f[1] == "n",
            value: (f[2] != "-").then(|| uo_jsnum::parse::parse_int(f[2], 10)),
        }),
        "dl" => Command::List(ListCmd::DefineListLevel {
            format: Box::new(if f[1] == "b" {
                ListLevelFormat {
                    bullet: Some(char::from_u32(u32::from_str_radix(f[2], 16).unwrap()).unwrap().to_string()),
                    font: Some("Arial".to_owned()),
                    ..ListLevelFormat::default()
                }
            } else {
                ListLevelFormat {
                    num_fmt: Some(f[2].to_owned()),
                    lvl_text: Some(f[3].to_owned()),
                    bold: true,
                    ..ListLevelFormat::default()
                }
            }),
        }),
        "as" => Command::Style(StyleCmd::ApplyStyle { style_id: (f[1] != "-").then(|| f[1].to_owned()) }),
        "cs" => Command::Style(StyleCmd::CreateStyle {
            spec: Box::new(CreateStyleSpec {
                name: NAMES[n(1) as usize].to_owned(),
                kind: Some(f[2].to_owned()),
                ..CreateStyleSpec::default()
            }),
        }),
        "ms" => {
            let spec = match n(2) {
                0 => {
                    ModifyStyleSpec { format: StyleFormat { bold: Ch::Val(true), ..StyleFormat::default() }, ..ModifyStyleSpec::default() }
                }
                1 => ModifyStyleSpec {
                    format: StyleFormat { sz_half_pt: Ch::Val(28.0), alignment: Ch::Val("center".to_owned()), ..StyleFormat::default() },
                    ..ModifyStyleSpec::default()
                },
                2 => ModifyStyleSpec { quick: Some(false), auto_update: Some(true), ..ModifyStyleSpec::default() },
                _ => ModifyStyleSpec {
                    next: Ch::Null,
                    format: StyleFormat { italic: Ch::Val(true), color: Ch::Val("FF0000".to_owned()), ..StyleFormat::default() },
                    ..ModifyStyleSpec::default()
                },
            };
            Command::Style(StyleCmd::ModifyStyle { id: f[1].to_owned(), spec: Box::new(spec) })
        }
        "rn" => Command::Style(StyleCmd::RenameStyle { id: f[1].to_owned(), name: NAMES[n(2) as usize].to_owned() }),
        "dd" => Command::Style(StyleCmd::DeleteStyle { id: f[1].to_owned() }),
        "sg" => Command::Style(StyleCmd::SetStyleInGallery { id: f[1].to_owned(), on: f[2] == "1" }),
        "us" => Command::Style(StyleCmd::UpdateStyleFromSelection { id: None }),
        "cp" => Command::Style(StyleCmd::ClearParagraphStyle),
        "cc" => Command::Style(StyleCmd::ClearCharacterStyle),
        "pf" => Command::Style(StyleCmd::ClearParagraphFormatting),
        "cf" => Command::Style(StyleCmd::ClearCharacterFormatting),
        "sf" => Command::Style(StyleCmd::SetDefaultFont {
            font: Box::new(match n(1) {
                0 => DefaultFont { size_pt: Some(12.0), ..DefaultFont::default() },
                1 => DefaultFont { font_family: Some("Arial".to_owned()), bold: Some(true), ..DefaultFont::default() },
                2 => DefaultFont { italic: Some(true), color: Ch::Val("00FF00".to_owned()), ..DefaultFont::default() },
                _ => DefaultFont { spacing_twips: Some(0.0), ..DefaultFont::default() },
            }),
        }),
        "ls" => Command::List(ListCmd::CreateListStyle {
            name: NAMES[n(1) as usize].to_owned(),
            levels: if f[1] == "0" { vec![] } else { vec![lvl(0, "upperRoman", None), lvl(1, "lowerLetter", Some("%2)"))] },
            apply: true,
        }),
        "la" => Command::List(ListCmd::ApplyListStyle { style_id: f[1].to_owned() }),
        "dm" => Command::List(ListCmd::DefineMultilevelList {
            levels: vec![lvl(0, "upperLetter", None), lvl(1, "decimal", Some("%1.%2."))],
            forward: f[1] == "1",
        }),
        _ => panic!("step {code}"),
    })
}

fn step(s: &mut Session, code: &str, cx: &mut Cx) -> ChangeSet {
    match code.split(':').next().unwrap() {
        "un" => return s.undo(&mut cx.cx()),
        "re" => return s.redo(&mut cx.cx()),
        _ => {}
    }
    let cmd = command(code).unwrap();
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => cs,
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in the F2 scripts"),
        Err(e) => panic!("{code}: {e}"),
    }
}

#[test]
fn d2b_f2_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f2-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let only = std::env::var("UO_D2B_DOC").ok();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        if only.as_deref().is_some_and(|o| !rel.contains(o)) {
            continue;
        }
        let bytes = std::fs::read(root.join(rel)).unwrap();
        // the element draws one `Math.random()` when it is constructed (`refSeed`, TS line 6103): the
        // session's source starts one value on
        let mut env = FixedEnv::default();
        uo_docx::session::Env::random_u32(&mut env);
        let mut s = Session::open(
            bytes,
            OpenOptions { sig_mode: SigMode::Ts, ts_formula_errors: true, ts_numbering: true, ..OpenOptions::default() },
            Box::new(env),
        )
        .unwrap();
        // the page locale of the capture is pt-BR
        s.set_ui_lang(UiLang::Pt);
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let cs = step(&mut s, code, &mut cx);
                let ev = ev_text(&cs);
                match cap.get(&key, "ev") {
                    // the TS throws "style name in use" after the step was taken (the step stays);
                    // Rust answers false: the events before the throw are the same
                    Some(want) if want.contains(" THROW:") && want.split(" THROW:").next() == Some(ev.as_str()) => {
                        replay.accept(&key, "ev", &ev, "D2b-F2: the TS throws after the step; Rust answers false with the step kept");
                    }
                    _ => {
                        replay.check(&key, "ev", &ev);
                    }
                }
            }
            if let Ok(d) = std::env::var("UO_D2B_DUMP")
                && let Some(para) = d.strip_prefix(&format!("{key}:")).and_then(|p| p.parse::<usize>().ok())
            {
                let mut paras = s.document().body.paras.clone();
                let mut ids = s.document().ids.clone();
                uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                let p = paras.get(para).unwrap();
                println!("RUST {}", uo_docx::js::JsonWriter::stringify(p, Some(&s.document().media)).unwrap());
            }
            if let Ok(d) = std::env::var("UO_D2B_TEXT") {
                if d == format!("{key}:nv") {
                    std::fs::write(std::env::temp_dir().join("rusttext.txt"), s.numbering_store().xml().map_or("-", |x| &**x)).unwrap();
                } else if d == format!("{key}:sv") {
                    std::fs::write(std::env::temp_dir().join("rusttext.txt"), s.styles_store().xml().map_or("-", |x| &**x)).unwrap();
                }
            }
            replay.check(&key, "paras", &paras_digest(&s));
            replay.check(&key, "sel", &sel_json(&s));
            replay.check(&key, "anc", &anc_json(&s));
            replay.check(&key, "flags", &flags(&s));
            replay.check(&key, "sv", &text_digest(s.styles_store().xml().map(|x| &**x)));
            replay.check(&key, "nv", &text_digest(s.numbering_store().xml().map(|x| &**x)));
            if cap.get(&key, "docxml").is_some() {
                match s.save_docx() {
                    Ok(bytes) => {
                        let pkg =
                            uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
                        replay.check(&key, "docxml", &part_digest(&pkg, "word/document.xml"));
                        replay.check(&key, "numxml", &part_digest(&pkg, "word/numbering.xml"));
                        replay.check(&key, "stylexml", &part_digest(&pkg, "word/styles.xml"));
                    }
                    Err(e) => {
                        println!("save {key}: {e}");
                        for f in ["docxml", "numxml", "stylexml"] {
                            replay.check(&key, f, "err");
                        }
                    }
                }
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f2-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2B_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
