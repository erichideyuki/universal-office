//! No panic in the D2b-F1 commands (record `docs/modules/uo-docx-d2b/F1.md`, "Security"): hostile
//! arguments (NaN, infinities, -0, 1e300, empty and 300-character strings, control characters,
//! bad indexes) in every command, over corpus documents and byte-mutated ones, mixed with the D2a
//! commands (selections, every context entered), undo/redo storms, the policy on and tracking
//! toggled — and a save that reopens. Every command answers a value (`Ok` or a `CommandError`),
//! never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f1_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use d2b_f1_support::hostile::hostile_f1;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{Command, Session};
use uo_testkit::generate::Rng;

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    for i in 0..steps {
        match rng.below(10) {
            0..=5 => run_step(&mut s, Step::Cmd(hostile_f1(&mut rng)), &mut cx),
            6 => {
                let st = next_step(&mut rng, &s);
                run_step(&mut s, st, &mut cx);
            }
            7 => run_step(&mut s, Step::Undo, &mut cx),
            8 if i % 7 == 0 => {
                let on = rng.below(2) == 0;
                run_step(&mut s, Step::Cmd(Command::Settings(SettingsCmd::SetTracking { on })), &mut cx);
            }
            8 if i % 11 == 0 => {
                let policy = (rng.below(2) == 0).then(FormattingPolicy::template_document);
                run_step(&mut s, Step::Cmd(Command::Settings(SettingsCmd::SetPolicy { policy })), &mut cx);
            }
            _ => run_step(&mut s, Step::Redo, &mut cx),
        }
        if let Some(paras) = s.paras() {
            let sel = s.selection();
            for p in [sel.anchor, sel.focus] {
                assert!((p.para as usize) < paras.len().max(1), "selection outside the story");
            }
        }
    }
    for _ in 0..300 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..300 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    match s.save_docx() {
        Ok(bytes) => open(bytes).is_some(),
        Err(_) => true,
    }
}

#[test]
fn hostile_f1_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    for (i, f) in sample(11).iter().enumerate() {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf1f1 + u32::try_from(i).unwrap(), 150) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2b-f1: {n} documents, {reopened} saves reopened");
    assert!(n > 200);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f1_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xf1bad);
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
    println!("nopanic d2b-f1 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused > 50);
}
