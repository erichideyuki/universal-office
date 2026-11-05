//! No panic in the D2b-F2 commands (lists and styles; record uo-docx-d2b/F2.md): corpus documents,
//! byte-mutated ones and documents with hostile `numbering.xml` / `styles.xml`, with hostile
//! arguments (NaN, infinities, huge levels, ids and names with quotes, markup and emoji, unknown
//! styles, empty names), the formatting policy on, tracking on, contexts entered, undo/redo past
//! both ends, and a save that reopens. Every command answers a value, never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f2_support;
mod support;

use d2a_support::{Cx, sample};
use d2b_f2_support::{doc_with_parts, f2_cmd, some_selection, typed};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::UiLang;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{Command, FixedEnv, OpenOptions, Session};
use uo_testkit::generate::Rng;

fn storm(mut s: Session, seed: u32, n: usize) -> usize {
    let mut cx = Cx::new();
    s.set_ui_lang([UiLang::Pt, UiLang::En, UiLang::Es][(seed % 3) as usize]);
    let mut rng = Rng::new(seed);
    let mut errors = 0;
    for i in 0..n {
        let c = match rng.below(10) {
            0 => some_selection(&mut rng, &s),
            1 => typed(&mut rng),
            2 => Command::Text([TextCmd::Enter, TextCmd::Backspace, TextCmd::Delete][rng.below(3)].clone()),
            3 if i % 7 == 0 => Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 }),
            4 if i % 11 == 0 => {
                Command::Settings(SettingsCmd::SetPolicy { policy: (rng.below(2) == 0).then(FormattingPolicy::template_document) })
            }
            5 if i % 13 == 0 => {
                s.undo(&mut cx.cx());
                continue;
            }
            6 if i % 13 == 6 => {
                s.redo(&mut cx.cx());
                continue;
            }
            _ => f2_cmd(&mut rng, true),
        };
        if s.exec(c, &mut cx.cx()).is_err() {
            errors += 1;
        }
    }
    // undo/redo storms past both ends, then a save that reopens
    for _ in 0..300 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..300 {
        s.redo(&mut cx.cx());
    }
    if let Ok(b) = s.save_docx() {
        let _ = Session::open(b, OpenOptions::default(), Box::new(FixedEnv::default()));
    }
    errors
}

#[test]
fn corpus_documents_survive_hostile_f2_scripts() {
    let docs = sample(60);
    assert!(docs.len() > 20);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let Ok(s) = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())) else { continue };
        storm(s, 0x2000 + u32::try_from(i).unwrap(), 70);
    }
}

#[test]
fn mutated_documents_survive_hostile_f2_scripts() {
    let docs = sample(120);
    let mut rng = Rng::new(99);
    for (i, f) in docs.iter().enumerate() {
        let mut bytes = std::fs::read(f).unwrap();
        for _ in 0..3 {
            if bytes.is_empty() {
                break;
            }
            let at = rng.below(bytes.len());
            bytes[at] ^= 1 << rng.below(8);
        }
        let Ok(s) = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())) else { continue };
        storm(s, 0x3000 + u32::try_from(i).unwrap(), 40);
    }
}

#[test]
fn hostile_parts_survive_hostile_f2_scripts() {
    let nums: [&str; 6] = [
        "<w:numbering/>",
        "<w:numbering><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"></w:abstractNum><w:num w:numId=\"x\"/></w:numbering>",
        "<w:numbering><w:num w:numId=\"1\"><w:abstractNumId w:val=\"9\"/></w:num></w:numbering>",
        "<w:numbering><w:abstractNum w:abstractNumId=\"1\"><w:numStyleLink w:val=\"A\"/></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>",
        "<w:numbering",
        "<w:numbering><w:abstractNum w:abstractNumId=\"99999999999999999999\"><w:lvl w:ilvl=\"0\"/></w:abstractNum></w:numbering>",
    ];
    let styles: [&str; 5] = [
        "<w:styles/>",
        "<w:styles><w:style w:styleId=\"A\"><w:name w:val=\"a\"/><w:basedOn w:val=\"A\"/><w:link w:val=\"A\"/><w:next w:val=\"A\"/></w:style></w:styles>",
        "<w:styles><w:style w:styleId=\"A\" w:type=\"numbering\"><w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style></w:styles>",
        "<w:styles><w:style w:styleId=\"\"/><w:style/></w:styles>",
        "garbage",
    ];
    let mut seed = 1u32;
    for n in nums {
        for st in styles {
            let bytes = doc_with_parts(Some(n), Some(st));
            let Ok(s) = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())) else { continue };
            seed += 1;
            storm(s, seed, 40);
        }
    }
}
