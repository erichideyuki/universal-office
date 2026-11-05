//! Shared helpers of the D2b-F5 memory and no-panic tests: seeded command generators for the notes
//! family (plausible and hostile).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::ids::HfPartId;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::notes::{ConvertMode, NoteCmd, NoteOptionsPatch, NoteScope};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, Command, DocPosition, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

const MARKS: [Option<&str>; 4] = [None, Some("*"), Some("  "), Some(" † ")];

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let para = rng.below(paras.len());
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

fn patch(rng: &mut Rng) -> NoteOptionsPatch {
    let mut p = NoteOptionsPatch::default();
    let s = |x: &str| Some(x.to_owned());
    match rng.below(10) {
        0 => p.number_format = s("lowerLetter"),
        1 => p.start_at = Some(5.0),
        2 => p.restart = s("eachSect"),
        3 => p.restart = s("eachPage"),
        4 => p.position = s("beneathText"),
        5 => p.position = s("sectEnd"),
        6 => p.columns = Some(2.0),
        7 => p.start_at = Some(0.4),
        8 => p.start_at = Some(-3.0),
        _ => {
            p.number_format = s("decimal");
            p.start_at = Some(1.0);
            p.restart = s("continuous");
            p.position = s("pageBottom");
        }
    }
    p
}

fn ref_of(rng: &mut Rng, s: &Session) -> (bool, f64) {
    let refs = s.note_refs(None);
    refs.get(rng.below(refs.len() + 1)).map_or((true, 99999.0), |r| (r.footnote, r.id))
}

/// A command of the notes family a user could give now (and the typing around it).
pub fn f5_cmd(rng: &mut Rng, s: &Session) -> Command {
    let k = rng.below(100);
    match k {
        0..=11 => Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", "😀", "x y"][rng.below(4)]) }),
        12..=14 => Command::Text(TextCmd::Enter),
        15..=19 => Command::Text(TextCmd::Backspace),
        20..=29 => {
            let a = pos(rng, s);
            let f = if rng.below(2) == 0 { a } else { pos(rng, s) };
            Command::Text(TextCmd::SetSelection { anchor: a, focus: f })
        }
        30..=32 => Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 }),
        33..=44 => Command::Note(NoteCmd::Insert { footnote: rng.below(2) == 0, mark: MARKS[rng.below(4)].map(Utf16::from) }),
        45..=57 => Command::Note(NoteCmd::SetOptions {
            footnote: rng.below(2) == 0,
            patch: patch(rng),
            scope: if rng.below(3) == 0 { NoteScope::Section } else { NoteScope::Document },
        }),
        58..=63 => {
            let (f, id) = ref_of(rng, s);
            Command::Note(NoteCmd::Convert {
                mode: [ConvertMode::ToEndnotes, ConvertMode::ToFootnotes, ConvertMode::Swap][rng.below(3)],
                only: (rng.below(2) == 0).then_some((f, id)),
            })
        }
        64..=68 => {
            let (footnote, id) = ref_of(rng, s);
            Command::Note(NoteCmd::Open { footnote, id })
        }
        69..=72 => {
            let (footnote, id) = ref_of(rng, s);
            Command::Note(NoteCmd::GoToRef { footnote, id })
        }
        73..=78 => Command::Note(NoteCmd::Next { footnote: rng.below(2) == 0, forward: rng.below(2) == 0 }),
        79..=81 => Command::Note(NoteCmd::Show { footnote: [None, Some(true), Some(false)][rng.below(3)] }),
        _ => ctx_cmd(rng, s),
    }
}

/// Enter or leave a cell, a note or a header/footer part (the note commands work from any context).
fn ctx_cmd(rng: &mut Rng, s: &Session) -> Command {
    match rng.below(5) {
        0 => {
            let n = s.document().body.tables.len();
            if n > 0 {
                let anchor = u32::try_from(rng.below(n)).unwrap();
                Command::Context(ContextCmd::EnterCell { anchor, hops: vec![CellHop { row: 0, col: 0, block: None }], caret: None })
            } else {
                Command::Context(ContextCmd::ExitCell)
            }
        }
        1 => {
            let (footnote, id) = ref_of(rng, s);
            Command::Context(ContextCmd::EnterNote { footnote, id, caret: None, at_end: rng.below(2) == 0 })
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

fn wild_f(rng: &mut Rng) -> f64 {
    [f64::NAN, -0.0, 1e300, -1.0, f64::INFINITY, f64::NEG_INFINITY, 0.5, 4_294_967_296.0][rng.below(8)]
}

fn wild_s(rng: &mut Rng) -> String {
    match rng.below(5) {
        0 => String::new(),
        1 => "x".repeat(40 + rng.below(3)),
        2 => "\u{0}\u{fffd}".to_owned(),
        3 => "𝒜b".to_owned(),
        _ => ["decimal", "lowerRoman", "eachSect", "pageBottom", "docEnd"][rng.below(5)].to_owned(),
    }
}

/// A command with hostile arguments (ids NaN/−0/1e300, options NaN/∞/huge/odd strings, marks of
/// lone surrogates and NUL, selections far outside the story, carets outside a note, the editor
/// locked in the middle).
pub fn hostile_cmd(rng: &mut Rng, s: &Session) -> Command {
    let k = rng.below(100);
    if k < 45 {
        return f5_cmd(rng, s);
    }
    match k {
        45..=52 => Command::Note(NoteCmd::Open { footnote: rng.below(2) == 0, id: wild_f(rng) }),
        53..=58 => Command::Note(NoteCmd::GoToRef { footnote: rng.below(2) == 0, id: wild_f(rng) }),
        59..=64 => Command::Note(NoteCmd::Convert {
            mode: [ConvertMode::ToEndnotes, ConvertMode::ToFootnotes, ConvertMode::Swap][rng.below(3)],
            only: Some((rng.below(2) == 0, wild_f(rng))),
        }),
        65..=76 => Command::Note(NoteCmd::SetOptions {
            footnote: rng.below(2) == 0,
            patch: NoteOptionsPatch {
                position: (rng.below(2) == 0).then(|| wild_s(rng)),
                number_format: (rng.below(2) == 0).then(|| wild_s(rng)),
                start_at: (rng.below(2) == 0).then(|| wild_f(rng)),
                restart: (rng.below(2) == 0).then(|| wild_s(rng)),
                columns: (rng.below(2) == 0).then(|| wild_f(rng)),
            },
            scope: if rng.below(2) == 0 { NoteScope::Section } else { NoteScope::Document },
        }),
        77..=82 => {
            Command::Note(NoteCmd::Insert { footnote: rng.below(2) == 0, mark: Some(Utf16::from_units(vec![0xdc00, 0x20, 0x61, 0])) })
        }
        83..=88 => Command::Text(TextCmd::SetSelection {
            anchor: DocPosition::new(wild(rng), wild(rng)),
            focus: DocPosition::new(wild(rng), wild(rng)),
        }),
        89..=92 => Command::Context(ContextCmd::EnterNote {
            footnote: rng.below(2) == 0,
            id: wild_f(rng),
            caret: Some(DocPosition::new(wild(rng), wild(rng))),
            at_end: false,
        }),
        _ => Command::Settings(SettingsCmd::SetProtected { on: rng.below(4) == 0 }),
    }
}
