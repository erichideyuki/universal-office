//! No panic in the D2b-F6 families (review, comments, bookmarks, links; record
//! `core/docs/modules/uo-docx-d2b/F6.md`) on any document and any command sequence: corpus
//! documents and byte-mutated ones, with hostile arguments — revision indices at `u32::MAX`, comment
//! ids NaN/−0/1e300/∞, bookmark names empty, 41 units long, non-BMP, with NUL, hyperlink addresses
//! and anchors (`p:` with huge numbers), markup filters with unknown authors, selections far outside
//! the story, the editor locked in the middle — then undo/redo storms and a save that reopens.
//! Every command answers a value (`Ok` or a `CommandError`), never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f6_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::{Cx, open, sample};
use d2b_f6_support::hostile_cmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, Session};
use uo_testkit::generate::Rng;

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    s.exec(Command::Settings(SettingsCmd::SetTracking { on: true }), &mut cx.cx()).unwrap();
    for _ in 0..steps {
        let c = hostile_cmd(&mut rng, &s);
        let _ = s.exec(c, &mut cx.cx());
        // the selection always lies in the story
        if let Some(paras) = s.paras() {
            let sel = s.selection();
            for p in [sel.anchor, sel.focus] {
                assert!((p.para as usize) < paras.len().max(1), "selection outside the story");
            }
        }
        if rng.below(12) == 0 {
            s.undo(&mut cx.cx());
        }
    }
    for _ in 0..300 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..300 {
        s.redo(&mut cx.cx());
    }
    // the queries on whatever is left
    let _ = (s.revision_count(), s.revision_authors_now(), s.bookmarks(true, true), s.comments().len(), s.can_insert_field());
    match s.save_docx() {
        Ok(bytes) => open(bytes).is_some(),
        Err(_) => true,
    }
}

#[test]
fn hostile_f6_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    for (i, f) in sample(11).iter().enumerate() {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf6ed + u32::try_from(i).unwrap(), 150) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2b-f6: {n} documents, {reopened} saves reopened");
    assert!(n > 200);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f6_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xbad6);
    let mut opened = 0;
    let mut refused = 0;
    for f in sample(37) {
        let mut bytes = std::fs::read(&f).unwrap();
        for _ in 0..1 + rng.below(4) {
            if bytes.is_empty() {
                break;
            }
            let at = rng.below(bytes.len());
            if rng.below(8) == 0 {
                bytes.truncate(at);
            } else if let Some(b) = bytes.get_mut(at) {
                *b ^= 1 << rng.below(8);
            }
        }
        match open(bytes) {
            Some(s) => {
                hammer(s, rng.next_u32(), 60);
                opened += 1;
            }
            None => refused += 1,
        }
    }
    println!("nopanic d2b-f6 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused > 50);
}
