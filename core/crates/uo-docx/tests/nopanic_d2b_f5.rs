//! No panic in the D2b-F5 family (notes; record `core/docs/modules/uo-docx-d2b/F5.md`) on any
//! document and any command sequence: corpus documents and byte-mutated ones, with hostile arguments
//! — note ids NaN/−0/1e300/∞, options NaN/∞/huge/odd strings and columns, marks of lone surrogates
//! and NUL, selections far outside the story, carets outside a note, the editor locked in the
//! middle, notes inserted, converted and navigated from every context (body, cell, note,
//! header/footer) — then undo/redo storms and a save that reopens. Every command answers a value
//! (`Ok` or a `CommandError`), never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f5_support;
mod support;

use d2a_support::{Cx, open, sample};
use d2b_f5_support::hostile_cmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, Session};
use uo_testkit::generate::Rng;

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    for _ in 0..steps {
        let c = hostile_cmd(&mut rng, &s);
        let shown = format!("{c:?}");
        let _ = s.exec(c, &mut cx.cx());
        // the selection always lies in the story
        if let Some(paras) = s.paras() {
            let sel = s.selection();
            for p in [sel.anchor, sel.focus] {
                assert!((p.para as usize) < paras.len().max(1), "selection outside the story after {shown} in {:?}", s.context());
            }
        }
        if rng.below(12) == 0 {
            s.undo(&mut cx.cx());
        }
        if rng.below(40) == 0 {
            s.exec(Command::Settings(SettingsCmd::SetProtected { on: false }), &mut cx.cx()).unwrap();
        }
    }
    for _ in 0..300 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..300 {
        s.redo(&mut cx.cx());
    }
    // the queries on whatever is left
    let _ =
        (s.note_options(true), s.note_options(false), s.note_count(true), s.note_refs(None), s.can_insert_note(), s.note_settings().json());
    match s.save_docx() {
        Ok(bytes) => open(bytes).is_some(),
        Err(_) => true,
    }
}

#[test]
fn hostile_f5_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    for (i, f) in sample(11).iter().enumerate() {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf5ed + u32::try_from(i).unwrap(), 150) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2b-f5: {n} documents, {reopened} saves reopened");
    assert!(n > 200);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f5_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xbad5);
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
    println!("nopanic d2b-f5 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused > 50);
}
