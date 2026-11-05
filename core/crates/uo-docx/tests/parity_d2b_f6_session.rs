//! Parity of D2b-F6 (review, comments, bookmarks, links) against the frozen `<universal-write>`
//! element in Chromium (G17): replays `tests/parity/d2b-f6-session.capture`, written by
//! `js/capture-d2b-f6-session.mjs`. Seeded scripts over corpus documents (those with revisions,
//! comments, bookmarks and notes first) mix typing with tracking on, two authors, the review
//! commands (accept/reject at an index, at the caret or selection with or without moving on, all
//! or the shown ones with or without stopping tracking, next/previous), the markup options and
//! modes, comments, bookmarks, hyperlinks and undo/redo. Facets per step: the D2a ones (`paras`,
//! `sel`, `anc`, `ev`, `flags`, `docxml`) and F6's: `rev` (the revision entries of every story),
//! `ra` (authors with colours), `txt` (the runs of every story of `markStories` with their marks),
//! `cm` (comments), `bk` (bookmarks), `mo` (markup mode and options).
//!
//! Accepted by name: the events of the install (step 0), and — from the step on — the list rules of
//! D2b-F2 (Enter on an empty list item, Backspace at the start of one: D2a-T4), as in the D2a
//! replay.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f6-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod support;

use std::path::Path;

use d2a_support::Cx;
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions};
use uo_docx::session::ops::comments::CommentCmd;
use uo_docx::session::ops::revision_cmds::{Balloons, MarkupPatch, RevisionCmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, MarkupMode, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

const NAMES: [&str; 9] = ["Alfa", "alfa", "Beta", "_oculto", "Gama1", "1ruim", "é2", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "a b"];
const URLS: [&str; 3] = ["https://example.com/a", " https://example.org ", "mailto:a@b.c"];

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
            Event::RevisionsChanged => "uw-revisions-changed:".to_owned(),
            Event::CommentsChanged => "uw-comments-changed:".to_owned(),
            Event::BookmarksChange => "uw-bookmarks-change:".to_owned(),
            Event::Markup => "uw-markup:".to_owned(),
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
            Digest::of(t.as_bytes()).short()
        }
        Err(_) => "err".to_owned(),
    }
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

fn n(x: uo_docx::js::JsNumber) -> String {
    uo_jsnum::format::to_js_string(x.to_number())
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn markup_patch(i: usize) -> MarkupPatch {
    let mut p = MarkupPatch::default();
    match i {
        0 => p.ins_del = Some(false),
        1 => p.ins_del = Some(true),
        2 => p.authors = Some(Some(vec!["A".into()])),
        3 => p.authors = Some(Some(vec!["B".into()])),
        4 => p.authors = Some(None),
        5 => p.formatting = Some(false),
        6 => p.comments = Some(false),
        7 => p.balloons = Some(Balloons::Inline),
        _ => panic!("markup {i}"),
    }
    p
}

fn step(s: &mut Session, code: &str, cx: &mut Cx) -> Result<ChangeSet, String> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "au" => Command::Settings(SettingsCmd::SetAuthor { name: f[1].into() }),
        "ac" => Command::Revision(RevisionCmd::AcceptAt { index: nn(1) }),
        "rj" => Command::Revision(RevisionCmd::RejectAt { index: nn(1) }),
        "ach" => Command::Revision(RevisionCmd::Accept { move_next: f[1] == "1" }),
        "rch" => Command::Revision(RevisionCmd::Reject { move_next: f[1] == "1" }),
        "aal" => Command::Revision(RevisionCmd::AcceptAll { shown: f[1] == "1", stop_tracking: f[2] == "1" }),
        "ral" => Command::Revision(RevisionCmd::RejectAll { shown: f[1] == "1", stop_tracking: f[2] == "1" }),
        "nx" => Command::Revision(RevisionCmd::NextChange),
        "pv" => Command::Revision(RevisionCmd::PreviousChange),
        "sr" => Command::Revision(RevisionCmd::SelectRevision { index: nn(1) }),
        "mo" => Command::Revision(RevisionCmd::SetMarkupOptions(markup_patch(nn(1) as usize))),
        "mm" => Command::Settings(SettingsCmd::SetMarkup {
            mode: match f[1] {
                "all" => MarkupMode::All,
                "simple" => MarkupMode::Simple,
                "none" => MarkupMode::None,
                _ => MarkupMode::Original,
            },
        }),
        "cm" => Command::Comment(CommentCmd::Add { text: Utf16::from_units(units(f[1])), author: None }),
        "cr" => Command::Comment(CommentCmd::Remove { id: f64::from(nn(1)) }),
        "bm" => Command::Bookmark(BookmarkCmd::Insert { name: NAMES[nn(1) as usize].into() }),
        "bd" => Command::Bookmark(BookmarkCmd::Delete { name: NAMES[nn(1) as usize].into() }),
        "sl" => Command::Bookmark(BookmarkCmd::SetLink { url: URLS.get(nn(1) as usize).map(|u| (*u).into()) }),
        "hl" => Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions {
            address: Some(URLS[nn(1) as usize].into()),
            tip: (f[2] == "1").then(|| "tip".into()),
            text: match f[3] {
                "1" => Some(Utf16::from_units("texto".encode_utf16().collect())),
                "2" => Some(Utf16::from_units(Vec::new())),
                _ => None,
            },
            ..HyperlinkOptions::default()
        })),
        "rl" => Command::Bookmark(BookmarkCmd::RemoveLink),
        "un" => return Ok(s.undo(&mut cx.cx())),
        "re" => return Ok(s.redo(&mut cx.cx())),
        _ => panic!("step {code}"),
    };
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => Ok(cs),
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F6 scripts"),
        Err(e) => Err(format!("{code}: {e}")),
    }
}

/// `rev` of the TS harness: `revisionSpans()` as text.
fn rev_text(s: &Session) -> String {
    s.revision_entries()
        .iter()
        .map(|e| {
            let sp = &e.span;
            [
                e.story.as_str().to_owned(),
                sp.kind.to_string(),
                sp.author.to_string(),
                sp.date.as_deref().unwrap_or("").to_owned(),
                sp.date_utc.as_deref().unwrap_or("").to_owned(),
                u8::from(sp.r#move == Some(true)).to_string(),
                u8::from(sp.mark == Some(true)).to_string(),
                n(sp.para),
                sp.end_para.map_or(String::new(), n),
                n(sp.start),
                n(sp.end),
                hx(sp.text.units()),
            ]
            .join("|")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ra_text(s: &Session) -> String {
    let v: Vec<String> = s.revision_authors_now().iter().map(|a| format!("{}={}", hs(a), s.author_color_of(a))).collect();
    if v.is_empty() { "-".to_owned() } else { v.join(",") }
}

fn run_sig(r: &uo_docx::model::run::Run) -> String {
    let p = &r.props;
    let bm = r
        .ext()
        .and_then(|e| e.bookmark.as_ref())
        .map_or(String::new(), |b| format!("{}{}{}", if b.end == Some(true) { "e" } else { "s" }, n(b.id), hs(&b.name)));
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        hx(r.text.units()),
        p.ins.as_deref().map_or("", |i| &i.author),
        p.del.as_deref().map_or("", |i| &i.author),
        u8::from(p.fmt.is_some()),
        p.comment_ids.as_deref().unwrap_or(&[]).iter().map(|x| n(*x)).collect::<Vec<_>>().join("."),
        bm,
        hs(p.link.as_deref().unwrap_or("")),
        p.char_style.as_deref().unwrap_or(""),
        hs(p.link_tip.as_deref().unwrap_or(""))
    )
}

fn txt(s: &Session) -> String {
    let mut stories: Vec<String> = Vec::new();
    for ms in s.mark_stories() {
        let Some(st) = uo_docx::session::ops::revision_cmds::story_at(s.document(), &ms.loc) else { continue };
        let paras: Vec<String> = st
            .paras
            .iter()
            .map(|p| {
                format!(
                    "{}{}{}:{}",
                    if p.props.mark_ins.is_some() { "i" } else { "" },
                    if p.props.mark_del.is_some() { "d" } else { "" },
                    if p.props.pfmt.is_some() { "f" } else { "" },
                    p.runs.iter().map(run_sig).collect::<Vec<_>>().join(",")
                )
            })
            .collect();
        stories.push(paras.join(";"));
    }
    stories.join("\n#\n")
}

fn cm_text(s: &Session) -> String {
    s.comments()
        .iter()
        .map(|c| format!("{}|{}|{}", n(c.id), hs(c.author.as_deref().unwrap_or("")), hx(c.text.units())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn bk_text(s: &Session) -> String {
    s.bookmarks(true, true)
        .iter()
        .map(|b| format!("{}|{}|{}", hs(&b.name), u8::from(b.hidden), hx(b.text.units())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn mo_text(s: &Session) -> String {
    let o = s.options();
    let m = &o.markup_options;
    format!(
        "{}|{}{}{}|{}|{}",
        match o.markup {
            MarkupMode::All => "all",
            MarkupMode::Simple => "simple",
            MarkupMode::None => "none",
            MarkupMode::Original => "original",
        },
        u8::from(m.ins_del),
        u8::from(m.formatting),
        u8::from(m.comments),
        match m.balloons {
            Balloons::Revisions => "revisions",
            Balloons::Inline => "inline",
            Balloons::Mixed => "mixed",
        },
        m.authors.as_ref().map_or("null".to_owned(), |a| a.iter().map(|x| hs(x)).collect::<Vec<_>>().join(","))
    )
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
fn d2b_f6_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f6-session.capture")).unwrap();
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
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("rev", sha(&rev_text(&s))),
                ("ra", ra_text(&s)),
                ("txt", sha(&txt(&s))),
                ("cm", sha(&cm_text(&s))),
                ("bk", sha(&bk_text(&s))),
                ("mo", mo_text(&s)),
            ];
            if let Some(ev) = ev {
                facets.push(("ev", ev));
            }
            if cap.get(&key, "docxml").is_some() {
                facets.push(("docxml", docxml(&mut s)));
            }
            if std::env::var("UO_F6_DUMP").is_ok_and(|d| d == key) {
                println!("RUST rev\n{}\nRUST txt\n{}\nRUST cm\n{}\nRUST bk\n{}", rev_text(&s), txt(&s), cm_text(&s), bk_text(&s));
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
    println!("d2b-f6-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
