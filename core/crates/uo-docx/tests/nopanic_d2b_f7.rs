//! No panic in the D2b-F7 families (drawing objects, text boxes, pictures; record
//! `core/docs/modules/uo-docx-d2b/F7.md`) on any document and any command sequence: corpus
//! documents, constructed ones with pictures and boxes, and byte-mutated ones, with hostile
//! arguments (NaN/inf/-0/1e300 sizes, rotations, crops, ppi and style numbers, indices at
//! `u32::MAX`, unknown ids and keys, garbage image bytes, decode answers that disagree with
//! themselves, jobs completed twice or never issued, commands cancelled midway, the editor locked
//! in the middle), then undo/redo storms, the queries and a save that reopens. Every command
//! answers a value (`Ok` or a `CommandError`), never a panic.
//!
//! Bounded by default (a few seconds); `UO_NOPANIC_BIG=1` runs the full sweep.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f7_support;
mod support;

use d2a_support::{Cx, open, sample};
use d2b_f7_support::{Shell, doc_with_objects, hostile_step, run_step};
use uo_docx::session::Session;
use uo_testkit::generate::Rng;

fn big() -> bool {
    std::env::var("UO_NOPANIC_BIG").is_ok()
}

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    let mut counter = 0u32;
    for i in 0..steps {
        let step = hostile_step(&mut rng, &s);
        run_step(&mut s, &mut cx, step, &mut rng, Shell::Hostile, &mut counter);
        // the selection always lies in the story
        if let Some(paras) = s.paras() {
            let sel = s.selection();
            for p in [sel.anchor, sel.focus] {
                assert!((p.para as usize) < paras.len().max(1), "selection outside the story");
            }
        }
        if i % 9 == 0 {
            let _ = (
                s.float_selection(),
                s.selected_object_count(),
                s.selection_pane_items(),
                s.selected_object_state(false),
                s.selected_object_state(true),
            );
        }
        if rng.below(12) == 0 {
            s.undo(&mut cx.cx());
        }
        if rng.below(40) == 0 {
            s.cancel_pending();
        }
    }
    for _ in 0..300 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..300 {
        s.redo(&mut cx.cx());
    }
    // the queries on whatever is left
    let _ = (s.float_selection(), s.selected_object_count(), s.selection_pane_items(), s.pane_keys(), s.compress_result());
    match s.save_docx() {
        Ok(bytes) => {
            let ok = open(bytes).is_some();
            if !ok {
                println!("save did not reopen: seed {seed:#x}");
            }
            ok
        }
        Err(_) => true,
    }
}

#[test]
fn hostile_f7_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    let step = if big() { 11 } else { 90 };
    for (i, f) in sample(step).iter().enumerate().take(if big() { usize::MAX } else { 30 }) {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf7ed + u32::try_from(i).unwrap(), if big() { 150 } else { 60 }) {
            reopened += 1;
        }
        n += 1;
    }
    // documents that have pictures and boxes to work on
    for (i, bytes) in [doc_with_objects(3, 2, 12), doc_with_objects(5, 1, 9), doc_with_objects(0, 3, 4)].iter().enumerate() {
        for k in 0..(if big() { 6 } else { 2 }) {
            let s = open(bytes.clone()).unwrap();
            if hammer(s, 0xf7f0 + u32::try_from(i * 10 + k).unwrap(), if big() { 150 } else { 80 }) {
                reopened += 1;
            }
            n += 1;
        }
    }
    println!("nopanic d2b-f7: {n} documents, {reopened} saves reopened");
    assert!(n >= 10);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f7_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xbad7);
    let mut opened = 0;
    let mut refused = 0;
    let mut sources: Vec<Vec<u8>> = vec![doc_with_objects(3, 2, 12), doc_with_objects(2, 1, 6)];
    for f in sample(if big() { 37 } else { 190 }) {
        sources.push(std::fs::read(&f).unwrap());
    }
    for mut bytes in sources {
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
                hammer(s, rng.next_u32(), 40);
                opened += 1;
            }
            None => refused += 1,
        }
    }
    println!("nopanic d2b-f7 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused >= 5);
}
