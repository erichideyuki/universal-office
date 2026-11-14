//! Parity of D2b-F9 (find, replace, go to, headings, the Navigation pane, the non-contiguous
//! selection) against the frozen `<universal-write>` element in Chromium (G17): replays
//! `tests/parity/d2b-f9-session.capture`, written by `js/capture-d2b-f9-session.mjs`. Seeded scripts
//! over corpus documents (those with headings, tables, fields, bookmarks, notes first) mix typing,
//! Backspace/Delete (over multi-selections too), selections, tracking, the F9 commands
//! (`findNextMatch` in the three directions, `replaceMatch`, `replaceAllMatches` in the document or
//! the selection with replacement formats, `selectAllMatches`, `countMatches`, `highlightMatches`,
//! `selectAll`, `setSelectionRanges`, `toggleRunProp('bold')` over the extra ranges, the seven heading
//! commands, `goTo` with every target kind, `search`/`goToSearchResult`, `goToBookmark`, entering and
//! leaving a cell) and undo/redo. Facets per step: the D2a ones (`paras`, `sel`, `anc`, `ev`, `flags`,
//! `docxml`) and F9's: `cursel` (the selection of the edited story), `res` (what the op returned),
//! `ext` (`getSelectionRanges()`), `ctx` (the edit context), `hd` (`getHeadings()` without pages).
//!
//! A step where the TS threw (`thrown`: a stale position, a TS bug) ends the document's script in the
//! capture; the replay stops there too.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f9-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f9_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use d2a_support::Cx;
use d2b_f9_support::{REPLS, format, hx, query, units};
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::context::{ContextCmd, EditContext, saved_selection};
use uo_docx::session::ops::find::{Direction, FindCmd, GoToWhat, Within};
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::navigation::{F9Result, FindOptions, HeadingWhere, NavigationCmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, ChangeSet, Command, DocPosition, DocRange, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

const GOTO: [GoToWhat; 11] = [
    GoToWhat::Section,
    GoToWhat::Heading,
    GoToWhat::Bookmark,
    GoToWhat::Comment,
    GoToWhat::Footnote,
    GoToWhat::Endnote,
    GoToWhat::Field,
    GoToWhat::Table,
    GoToWhat::Graphic,
    GoToWhat::Equation,
    GoToWhat::Object,
];

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

fn sel_json(a: DocPosition, f: DocPosition) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(a), pos_json(f))
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

fn docxml(s: &mut Session) -> String {
    match s.save_docx() {
        Ok(bytes) => {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
            if let Ok(dir) = std::env::var("UO_F9_XML") {
                std::fs::write(Path::new(&dir).join(format!("rust-{}.xml", Digest::of(t.as_bytes()).short())), &t).unwrap();
            }
            Digest::of(t.as_bytes()).short()
        }
        Err(_) => "err".to_owned(),
    }
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn err_hex(e: Option<&uo_docx::session::ops::find::engine::QueryError>) -> String {
    e.map(|e| hx(&e.message(true))).unwrap_or_default()
}

/// What the op returned, as the TS harness writes it.
fn res_of(s: &Session, code: &str) -> String {
    let op = code.split(':').next().unwrap();
    let r = s.navigation_result();
    let b = |v: Option<&F9Result>| match v {
        Some(F9Result::Bool(x)) => x.to_string(),
        other => format!("{other:?}"),
    };
    match op {
        "fn" => match r {
            Some(F9Result::Find(o)) if !o.budget => format!("{}{}|{}", u8::from(o.found), u8::from(o.wrapped), err_hex(o.error.as_ref())),
            other => format!("{other:?}"),
        },
        "rm" => match r {
            Some(F9Result::Replace(o)) if !o.find.budget => {
                format!("{}{}{}|{}", u8::from(o.replaced), u8::from(o.find.found), u8::from(o.find.wrapped), err_hex(o.find.error.as_ref()))
            }
            other => format!("{other:?}"),
        },
        "ra" => match r {
            Some(F9Result::Replace(o)) if !o.find.budget => format!("{}|{}", o.count, err_hex(o.find.error.as_ref())),
            other => format!("{other:?}"),
        },
        "sm" | "hm" => match r {
            Some(F9Result::Count(n)) => n.to_string(),
            other => format!("{other:?}"),
        },
        "mh" | "ph" | "dh" | "ih" | "xh" | "go" | "nr" | "gb" => b(r),
        "ns" => {
            let v: Vec<String> = s
                .search_results()
                .iter()
                .map(|x| {
                    format!(
                        "{},{},{},{},{}|{}|{}",
                        x.start,
                        x.end,
                        x.heading,
                        u8::from(x.in_table),
                        hx(&x.snippet.before),
                        hx(&x.snippet.r#match),
                        hx(&x.snippet.after)
                    )
                })
                .collect();
            if v.is_empty() { "-".to_owned() } else { v.join(";") }
        }
        _ => "-".to_owned(),
    }
}

fn step(s: &mut Session, code: &str, cx: &mut Cx) -> Result<(ChangeSet, Option<String>), String> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let ii = |i: usize| f[i].parse::<i64>().unwrap();
    let q = || query(f[1], &units(f[2]), f[3].parse().unwrap());
    let within = |x: &str| if x == "1" { Within::Selection } else { Within::Document };
    let fmt = |x: &str| format(x.parse().unwrap()).map(Box::new);
    let nav = Command::Navigation;
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "fn" => Command::Find(FindCmd::FindNext {
            query: q(),
            direction: match f[4] {
                "down" => Direction::Down,
                "up" => Direction::Up,
                _ => Direction::All,
            },
        }),
        "rm" => Command::Find(FindCmd::ReplaceMatch {
            query: q(),
            replacement: REPLS[nn(4) as usize].encode_utf16().collect(),
            direction: Direction::All,
            replace_format: fmt(f[5]),
        }),
        "ra" => Command::Find(FindCmd::ReplaceAll {
            query: q(),
            replacement: REPLS[nn(4) as usize].encode_utf16().collect(),
            within: within(f[5]),
            replace_format: fmt(f[6]),
        }),
        "sm" => Command::Find(FindCmd::SelectAllMatches { query: q(), within: within(f[4]) }),
        "cm" => {
            let n = s.count_matches(&q(), within(f[4])).map_err(|e| format!("{code}: {e:?}"))?;
            return Ok((ChangeSet::default(), Some(n.to_string())));
        }
        "hm" => Command::Find(FindCmd::HighlightMatches { query: q(), within: Within::Document }),
        "sa" => nav(NavigationCmd::SelectAll),
        "sr" => {
            let ranges = f[1]
                .split(',')
                .map(|r| {
                    let v: Vec<u32> = r.split('.').map(|x| x.parse().unwrap()).collect();
                    DocRange::new(DocPosition::new(v[0], v[1]), DocPosition::new(v[2], v[3]))
                })
                .collect();
            nav(NavigationCmd::SetSelectionRanges { ranges })
        }
        "tb" => Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".to_owned() }),
        "mh" => nav(NavigationCmd::MoveHeading { index: ii(1), before: ii(2) }),
        "ph" => nav(NavigationCmd::PromoteHeading { index: ii(1) }),
        "dh" => nav(NavigationCmd::DemoteHeading { index: ii(1) }),
        "ih" => nav(NavigationCmd::InsertHeading {
            index: ii(1),
            at: match f[3] {
                "before" => HeadingWhere::Before,
                "after" => HeadingWhere::After,
                _ => HeadingWhere::Sub,
            },
        }),
        "xh" => nav(NavigationCmd::DeleteHeading { index: ii(1) }),
        "sh" => nav(NavigationCmd::SelectHeading { index: ii(1) }),
        "gh" => nav(NavigationCmd::GoToHeading { index: ii(1) }),
        "go" => Command::Find(FindCmd::GoTo { what: GOTO[nn(1) as usize], target: units(f[2]) }),
        "ns" => {
            let o = f[2].as_bytes();
            nav(NavigationCmd::Search { text: units(f[1]), opts: FindOptions { match_case: o[0] == b'1', whole_word: o[1] == b'1' } })
        }
        "nr" => nav(NavigationCmd::GoToSearchResult { index: ii(1) }),
        "gb" => Command::Find(FindCmd::GoToBookmark { name: String::from_utf16(&units(f[1])).unwrap() }),
        "ec" => {
            let anchor = nn(1);
            let gb = s
                .document()
                .body
                .tables
                .get(anchor as usize)
                .and_then(|a| a.table.rows.first())
                .and_then(|r| r.tr_pr.as_deref())
                .and_then(|p| p.grid_before)
                .map_or(0, |g| uo_jsnum::conv::to_uint32(g.to_number()));
            Command::Context(ContextCmd::EnterCell {
                anchor,
                hops: vec![CellHop { row: 0, col: gb, block: None }],
                caret: Some(DocPosition::new(0, 0)),
            })
        }
        "xc" => Command::Context(ContextCmd::ExitCell),
        "un" => return Ok((s.undo(&mut cx.cx()), None)),
        "re" => return Ok((s.redo(&mut cx.cx()), None)),
        _ => panic!("step {code}"),
    };
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => Ok((cs, None)),
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F9 scripts"),
        Err(e) => Err(format!("{code}: {e}")),
    }
}

/// The selection is outside the edited story (a paragraph past the end, an offset past its length).
fn out_of_story(s: &Session) -> bool {
    let sel = s.selection();
    let Some(paras) = s.paras() else { return false };
    [sel.anchor, sel.focus]
        .iter()
        .any(|p| paras.get(p.para as usize).is_none_or(|x| p.offset as usize > uo_docx::session::text::para_length(x)))
}

fn ctx_text(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> =
                hops.iter().map(|h| format!("{}.{}.{}", h.row, h.col, h.block.map_or(String::new(), |b| b.to_string()))).collect();
            format!("c{anchor}:{}", h.join("/"))
        }
        EditContext::Body => "b".to_owned(),
        _ => "o".to_owned(),
    }
}

fn ext_json(s: &Session) -> String {
    let v: Vec<String> =
        s.selection_ranges().iter().map(|r| format!("{{\"start\":{},\"end\":{}}}", pos_json(r.start), pos_json(r.end))).collect();
    format!("[{}]", v.join(","))
}

fn hd_text(s: &Session) -> String {
    s.headings()
        .iter()
        .map(|h| format!("{}|{}|{}", uo_jsnum::format::to_js_string(h.level), hx(h.text.units()), h.para))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn d2b_f9_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f9-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let mut s = Session::open(
            bytes,
            OpenOptions { sig_mode: SigMode::Ts, ts_formula_errors: true, ts_numbering: true, ..OpenOptions::default() },
            Box::new(FixedEnv::default()),
        )
        .unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        // F9-A2: `goToSearchResult` of a stale result leaves the TS selection outside the story; the
        // D2a undo/redo clamps a restored selection (D2a-C4) where the TS keeps it: from there the
        // document is accepted by name
        let mut stale = false;
        let mut accepted_from: Option<usize> = None;
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if cap.get(&key, "thrown").is_some() {
                replay.skip(&key, "thrown");
                break;
            }
            let mut ev = None;
            let mut res = "-".to_owned();
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let (cs, direct) = match step(&mut s, code, &mut cx) {
                    Ok(x) => x,
                    Err(e) => panic!("{rel}#{j}: {e}"),
                };
                if direct.is_none() {
                    ev = Some(ev_text(&cs));
                }
                res = direct.unwrap_or_else(|| res_of(&s, code));
                if stale && accepted_from.is_none() && (code == "un" || code == "re") {
                    accepted_from = Some(j);
                }
                if code.starts_with("nr:") && out_of_story(&s) {
                    stale = true;
                }
            }
            if std::env::var("UO_F9_TRACE").is_ok_and(|t| rel.contains(&t)) {
                println!("{j} {code} sel={:?} undo={} redo={}", s.selection(), s.history().undo_len(), s.history().redo_len());
            }
            let sel = s.selection();
            let body_sel = saved_selection(s.context()).unwrap_or(sel);
            let res = if res.len() > 200 { sha(&res) } else { res };
            let mut facets = vec![
                ("paras", paras_digest(&s)),
                ("sel", sel_json(body_sel.anchor, body_sel.focus)),
                ("cursel", sel_json(sel.anchor, sel.focus)),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("res", res),
                ("ext", ext_json(&s)),
                ("ctx", ctx_text(&s)),
                ("hd", sha(&hd_text(&s))),
            ];
            match ev {
                Some(ev) => facets.push(("ev", ev)),
                None if j > 0 => facets.push(("ev", "-".to_owned())),
                None => {}
            }
            if cap.get(&key, "docxml").is_some() {
                facets.push(("docxml", docxml(&mut s)));
            }
            for (f, actual) in facets {
                if accepted_from.is_some() {
                    replay.accept(
                        &key,
                        f,
                        &actual,
                        "F9-A2: a stale search result put the selection outside the story; the D2a undo clamps it",
                    );
                } else {
                    replay.check(&key, f, &actual);
                }
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f9-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
