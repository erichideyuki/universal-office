//! No panic in the D2a session on any document and any command sequence (record "D2a — session
//! (architecture)", "Security"): corpus documents and byte-mutated ones, with hostile commands —
//! selections far outside the story, cells by random hops (nested past the tables), notes that
//! do not exist, header/footer parts and textbox runs by random index, every context entered from
//! every other, tracking toggled, undo/redo storms past both ends, the policy on — and a save that
//! reopens. Every command answers a value (`Ok` or a `CommandError`), never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod support;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use uo_docx::ids::HfPartId;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{CellHop, Command, DocPosition, MarkupMode, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn wild(rng: &mut Rng) -> u32 {
    match rng.below(4) {
        0 => rng.next_u32(),
        1 => u32::MAX - u32::try_from(rng.below(3)).unwrap(),
        _ => u32::try_from(rng.below(40)).unwrap(),
    }
}

fn hostile(rng: &mut Rng, s: &Session) -> Step {
    let k = rng.below(100);
    if k < 50 {
        return next_step(rng, s);
    }
    let cmd = match k {
        50..=57 => Command::Text(TextCmd::SetSelection {
            anchor: DocPosition::new(wild(rng), wild(rng)),
            focus: DocPosition::new(wild(rng), wild(rng)),
        }),
        58..=63 => {
            let hops = (0..1 + rng.below(4))
                .map(|_| CellHop { row: wild(rng), col: wild(rng), block: (rng.below(2) == 0).then(|| wild(rng)) })
                .collect();
            Command::Context(ContextCmd::EnterCell {
                anchor: wild(rng),
                hops,
                caret: (rng.below(2) == 0).then(|| DocPosition::new(wild(rng), wild(rng))),
            })
        }
        64..=67 => Command::Context(ContextCmd::EnterNote {
            footnote: rng.below(2) == 0,
            id: [f64::NAN, -0.0, 1.0, 2.0, 1e300, -1.0][rng.below(6)],
            caret: (rng.below(2) == 0).then(|| DocPosition::new(wild(rng), wild(rng))),
            at_end: rng.below(2) == 0,
        }),
        68..=71 => Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(wild(rng)), page: wild(rng) }),
        72..=76 => Command::Context(ContextCmd::EnterTextbox {
            para: wild(rng),
            run: wild(rng),
            host: wild(rng),
            focus: wild(rng),
            page: wild(rng),
        }),
        77..=80 => Command::Context(match rng.below(4) {
            0 => ContextCmd::ExitCell,
            1 => ContextCmd::ExitNote,
            2 => ContextCmd::ExitHeaderFooter,
            _ => ContextCmd::ExitTextbox,
        }),
        81..=83 => Command::Settings(SettingsCmd::SetMarkup {
            mode: [MarkupMode::All, MarkupMode::Simple, MarkupMode::None, MarkupMode::Original][rng.below(4)],
        }),
        84..=85 => Command::Settings(SettingsCmd::SetPolicy { policy: (rng.below(2) == 0).then(FormattingPolicy::template_document) }),
        86..=87 => Command::Settings(SettingsCmd::SetReadView { on: rng.below(4) == 0 }),
        88..=91 => Command::Text(TextCmd::Type {
            text: Utf16::from_units((0..rng.below(6)).map(|_| [0xD83D, 0xDE00, 0xFFFC, 0x0, 0x9, 0xD, 0x41][rng.below(7)]).collect()),
        }),
        92..=95 => return if rng.below(2) == 0 { Step::Undo } else { Step::Redo },
        _ => Command::Text([TextCmd::Enter, TextCmd::Backspace, TextCmd::Delete][rng.below(3)].clone()),
    };
    Step::Cmd(cmd)
}

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    for _ in 0..steps {
        let st = hostile(&mut rng, &s);
        run_step(&mut s, st, &mut cx);
        // the selection always lies in the story
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
fn hostile_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    for (i, f) in sample(11).iter().enumerate() {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0x5eed + u32::try_from(i).unwrap(), 150) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2a: {n} documents, {reopened} saves reopened");
    assert!(n > 200);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xbad);
    let mut opened = 0;
    let mut refused = 0;
    for f in sample(37) {
        let mut bytes = std::fs::read(&f).unwrap();
        // flip bytes inside the zip (most stored parts are deflated: the reader sees garbage or a
        // corrupt stream), or truncate
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
    println!("nopanic d2a mutated: {opened} opened, {refused} refused");
    assert!(opened + refused > 50);
}
