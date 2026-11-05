//! Shared helpers of the D2b-F6 memory and no-panic tests: seeded command generators for the
//! review, comment and bookmark families (plausible and hostile).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::ids::HfPartId;
use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions};
use uo_docx::session::ops::comments::CommentCmd;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::revision_cmds::{Balloons, MarkupPatch, RevisionCmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, Command, DocPosition, MarkupMode, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

const NAMES: [&str; 8] = ["Alfa", "alfa", "Beta", "_oculto", "Gama1", "1ruim", "é2", "a b"];
const URLS: [&str; 3] = ["https://example.com/a", " https://example.org ", "mailto:a@b.c"];

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let para = rng.below(paras.len());
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

fn markup(rng: &mut Rng) -> MarkupPatch {
    let mut p = MarkupPatch::default();
    match rng.below(8) {
        0 => p.ins_del = Some(false),
        1 => p.ins_del = Some(true),
        2 => p.authors = Some(Some(vec!["A".into()])),
        3 => p.authors = Some(Some(vec!["B".into()])),
        4 => p.authors = Some(None),
        5 => p.formatting = Some(false),
        6 => p.comments = Some(false),
        _ => p.balloons = Some(Balloons::Inline),
    }
    p
}

/// A command of the F6 families a user could give now (and the typing that makes revisions).
pub fn f6_cmd(rng: &mut Rng, s: &Session) -> Command {
    let n = u32::try_from(s.revision_count()).unwrap();
    let idx = u32::try_from(rng.below(usize::try_from(n).unwrap() + 2)).unwrap();
    let k = rng.below(100);
    let flag = |rng: &mut Rng| rng.below(2) == 0;
    match k {
        0..=17 => Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", "😀", "x y"][rng.below(4)]) }),
        18..=21 => Command::Text(TextCmd::Enter),
        22..=27 => Command::Text(TextCmd::Backspace),
        28..=35 => {
            let a = pos(rng, s);
            let f = if rng.below(2) == 0 { a } else { pos(rng, s) };
            Command::Text(TextCmd::SetSelection { anchor: a, focus: f })
        }
        36..=40 => Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 }),
        41..=43 => Command::Settings(SettingsCmd::SetAuthor { name: if rng.below(2) == 0 { "A" } else { "B" }.into() }),
        44..=49 => Command::Revision(if flag(rng) { RevisionCmd::AcceptAt { index: idx } } else { RevisionCmd::RejectAt { index: idx } }),
        50..=55 => Command::Revision(if flag(rng) {
            RevisionCmd::Accept { move_next: flag(rng) }
        } else {
            RevisionCmd::Reject { move_next: flag(rng) }
        }),
        56..=59 => Command::Revision(if flag(rng) {
            RevisionCmd::AcceptAll { shown: flag(rng), stop_tracking: flag(rng) }
        } else {
            RevisionCmd::RejectAll { shown: flag(rng), stop_tracking: flag(rng) }
        }),
        60..=62 => Command::Revision(if flag(rng) { RevisionCmd::NextChange } else { RevisionCmd::PreviousChange }),
        63..=64 => Command::Revision(RevisionCmd::SelectRevision { index: idx }),
        65..=67 => Command::Revision(RevisionCmd::SetMarkupOptions(markup(rng))),
        68..=69 => Command::Settings(SettingsCmd::SetMarkup {
            mode: [MarkupMode::All, MarkupMode::Simple, MarkupMode::None, MarkupMode::Original][rng.below(4)],
        }),
        70..=75 => Command::Comment(CommentCmd::Add { text: Utf16::from(["ok", "  ", "ção 😀"][rng.below(3)]), author: None }),
        76..=78 => {
            let id = s.comments().get(rng.below(s.comments().len() + 1)).map_or(0.0, |c| c.id.to_number());
            Command::Comment(CommentCmd::Remove { id })
        }
        79..=85 => Command::Bookmark(BookmarkCmd::Insert { name: NAMES[rng.below(NAMES.len())].into() }),
        86..=89 => Command::Bookmark(BookmarkCmd::Delete { name: NAMES[rng.below(NAMES.len())].into() }),
        90..=92 => Command::Bookmark(BookmarkCmd::SetLink { url: URLS.get(rng.below(4)).map(|u| (*u).into()) }),
        93..=95 => Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions {
            address: Some(URLS[rng.below(3)].into()),
            tip: (rng.below(2) == 0).then(|| "tip".into()),
            text: (rng.below(2) == 0).then(|| Utf16::from("texto")),
            ..HyperlinkOptions::default()
        })),
        96 => Command::Bookmark(BookmarkCmd::RemoveLink),
        _ => ctx_cmd(rng, s),
    }
}

/// Enter or leave a cell, a note or a header/footer part (the review commands work in any context).
fn ctx_cmd(rng: &mut Rng, s: &Session) -> Command {
    match rng.below(4) {
        0 => {
            let n = s.document().body.tables.len();
            if n > 0 {
                let anchor = u32::try_from(rng.below(n)).unwrap();
                let hops = vec![CellHop { row: 0, col: 0, block: None }];
                Command::Context(ContextCmd::EnterCell { anchor, hops, caret: None })
            } else {
                Command::Context(ContextCmd::ExitCell)
            }
        }
        1 => {
            let n = s.document().notes.footnotes.len();
            if n > 0 {
                let id = s.document().notes.footnotes.keys().nth(rng.below(n)).map_or(0.0, |k| k.get());
                Command::Context(ContextCmd::EnterNote { footnote: true, id, caret: None, at_end: rng.below(2) == 0 })
            } else {
                Command::Context(ContextCmd::ExitNote)
            }
        }
        2 => {
            let n = s.document().hf.len();
            if n > 0 {
                let part = HfPartId(u32::try_from(rng.below(n)).unwrap());
                Command::Context(ContextCmd::EnterHeaderFooter { part, page: 0 })
            } else {
                Command::Context(ContextCmd::ExitHeaderFooter)
            }
        }
        _ => Command::Context([ContextCmd::ExitCell, ContextCmd::ExitNote, ContextCmd::ExitHeaderFooter][rng.below(3)].clone()),
    }
}

fn wild(rng: &mut Rng) -> u32 {
    match rng.below(4) {
        0 => rng.next_u32(),
        1 => u32::MAX - u32::try_from(rng.below(3)).unwrap(),
        _ => u32::try_from(rng.below(40)).unwrap(),
    }
}

/// A command with hostile arguments (indices at `u32::MAX`, ids NaN/−0/1e300, huge or odd names,
/// markup filters with unknown authors, selections far outside the story).
pub fn hostile_cmd(rng: &mut Rng, s: &Session) -> Command {
    let k = rng.below(100);
    if k < 40 {
        return f6_cmd(rng, s);
    }
    let name = |rng: &mut Rng| -> String {
        match rng.below(6) {
            0 => String::new(),
            1 => "x".repeat(40 + rng.below(3)),
            2 => "\u{0}\u{fffd}".to_owned(),
            3 => "𝒜b".repeat(10),
            4 => "Ça_1".to_owned(),
            _ => NAMES[rng.below(NAMES.len())].to_owned(),
        }
    };
    match k {
        40..=47 => Command::Revision(RevisionCmd::AcceptAt { index: wild(rng) }),
        48..=53 => Command::Revision(RevisionCmd::RejectAt { index: wild(rng) }),
        54..=57 => Command::Revision(RevisionCmd::SelectRevision { index: wild(rng) }),
        58..=62 => Command::Text(TextCmd::SetSelection {
            anchor: DocPosition::new(wild(rng), wild(rng)),
            focus: DocPosition::new(wild(rng), wild(rng)),
        }),
        63..=66 => Command::Revision(RevisionCmd::SetMarkupOptions(MarkupPatch {
            authors: Some(Some(vec!["nobody".into(), String::new().into()])),
            ins_del: Some(rng.below(2) == 0),
            ..MarkupPatch::default()
        })),
        67..=72 => Command::Comment(CommentCmd::Remove { id: [f64::NAN, -0.0, 1e300, -1.0, f64::INFINITY][rng.below(5)] }),
        73..=77 => Command::Bookmark(BookmarkCmd::Insert { name: name(rng).into() }),
        78..=81 => Command::Bookmark(BookmarkCmd::Delete { name: name(rng).into() }),
        82..=86 => Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions {
            address: (rng.below(3) != 0).then(|| name(rng).into()),
            anchor: Some(["p:99999999999999999999", "p:0", "p:", "x", "p:4294967296", "_top"][rng.below(6)].into()),
            tip: Some(name(rng).into()),
            text: (rng.below(2) == 0).then(|| Utf16::from_units(vec![0xd800, 0x61])),
        })),
        87..=90 => Command::Bookmark(BookmarkCmd::SetLink { url: Some(name(rng).into()) }),
        91..=93 => Command::Comment(CommentCmd::Add { text: Utf16::from_units(vec![0xdc00, 0x20, 0x61]), author: Some(name(rng).into()) }),
        _ => Command::Settings(SettingsCmd::SetProtected { on: rng.below(4) == 0 }),
    }
}
