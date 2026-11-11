//! No panic in the D2b-F4 families (sections, page setup, headers/footers, page colour, text layout
//! options; record `core/docs/modules/uo-docx-d2b/F4.md`) on any document and any command sequence:
//! corpus documents and byte-mutated ones, with hostile arguments — margins and widths NaN/±∞/1e300,
//! column counts NaN, section and part indices at `u32::MAX`, a header/footer part to return to that
//! does not exist, line numbering restarts, a locked editor in the middle — then undo/redo storms and
//! a save that reopens. Every command answers a value (`Ok` or a `CommandError`), never a panic.
//! Bounded: at most 30 documents of 60 steps by default (`UO_NOPANIC_BIG=1` for the full sweep).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f4_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::{Cx, open, sample};
use d2b_f4_support::{doc, doc_with_props, doc_with_watermark, hostile_cmd, hostile_remainder_cmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, Session};
use uo_testkit::generate::Rng;

/// Sections, columns and parts a script may build before the commands stop (the size guard).
const MAX_SECTIONS: usize = 400;

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    for k in 0..steps {
        if s.document().sections.len() > MAX_SECTIONS || s.document().hf.len() > 400 {
            break;
        }
        let c = if rng.below(4) == 0 { hostile_remainder_cmd(&mut rng, &s) } else { hostile_cmd(&mut rng, &s) };
        let shown = format!("{c:?}");
        let _ = s.exec(c, &mut cx.cx());
        // the selection always lies in the story
        if let Some(paras) = s.paras() {
            let sel = s.selection();
            for p in [sel.anchor, sel.focus] {
                assert!((p.para as usize) < paras.len().max(1), "selection outside the story after {}", &shown[..shown.len().min(300)]);
            }
        }
        if rng.below(12) == 0 {
            s.undo(&mut cx.cx());
        }
        if k == steps / 2 {
            let _ = s.exec(Command::Settings(SettingsCmd::SetProtected { on: rng.below(2) == 0 }), &mut cx.cx());
        }
    }
    let _ = s.exec(Command::Settings(SettingsCmd::SetProtected { on: false }), &mut cx.cx());
    for _ in 0..300 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..300 {
        s.redo(&mut cx.cx());
    }
    // the queries on whatever is left
    let _ = (s.hyphenation(), s.even_odd_headers(), s.document().sections.len());
    match s.save_docx() {
        Ok(bytes) => open(bytes).is_some(),
        Err(_) => true,
    }
}

fn big() -> bool {
    std::env::var("UO_NOPANIC_BIG").is_ok()
}

#[test]
fn hostile_f4_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    let step = if big() { 11 } else { 90 };
    for (i, f) in sample(step).iter().enumerate().take(if big() { usize::MAX } else { 30 }) {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf4ed + u32::try_from(i).unwrap(), if big() { 150 } else { 60 }) {
            reopened += 1;
        }
        n += 1;
    }
    for (i, bytes) in [doc(4, 2), doc(3, 1), doc(8, 2), doc_with_props(), doc_with_watermark()].iter().enumerate() {
        let s = open(bytes.clone()).unwrap();
        if hammer(s, 0xf4f0 + u32::try_from(i).unwrap(), 150) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2b-f4: {n} documents, {reopened} saves reopened");
    assert!(n >= 10);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f4_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xbad4);
    let mut opened = 0;
    let mut refused = 0;
    for f in sample(if big() { 37 } else { 190 }) {
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
                hammer(s, rng.next_u32(), 40);
                opened += 1;
            }
            None => refused += 1,
        }
    }
    println!("nopanic d2b-f4 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused >= 5);
}
