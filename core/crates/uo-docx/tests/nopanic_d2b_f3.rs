//! No panic in the D2b-F3 commands (record `docs/modules/uo-docx-d2b/F3.md`, "Security"): hostile
//! arguments (NaN, infinities, huge counts and spans, rectangles and hops past the tables, merged
//! cells, control characters) in every table command, over corpus documents with tables and
//! byte-mutated ones, mixed with the D2a commands (selections, every context entered), undo/redo
//! storms, tracking toggled — and a save that reopens. Every command answers a value (`Ok` or a
//! `CommandError`), never a panic.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f3_support;
mod support;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use d2b_f3_support::hostile::hostile_f3;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, Session};
use uo_testkit::generate::Rng;

fn hammer(mut s: Session, seed: u32, steps: usize) -> bool {
    let mut cx = Cx::new();
    let mut rng = Rng::new(seed);
    for i in 0..steps {
        // a table command can double a table (all rows selected, Insert Rows): keep the work bounded
        let cells: usize = s.document().body.tables.iter().map(|a| a.table.rows.iter().map(|r| r.cells.len()).sum::<usize>()).sum();
        if cells > 20_000 {
            run_step(&mut s, Step::Undo, &mut cx);
            continue;
        }
        match rng.below(10) {
            0..=6 => {
                let c = hostile_f3(&mut rng, &s);
                run_step(&mut s, Step::Cmd(c), &mut cx);
            }
            7 => {
                let st = next_step(&mut rng, &s);
                run_step(&mut s, st, &mut cx);
            }
            8 if i % 5 == 0 => {
                run_step(&mut s, Step::Undo, &mut cx);
            }
            8 if i % 11 == 0 => {
                let on = rng.below(2) == 0;
                run_step(&mut s, Step::Cmd(Command::Settings(SettingsCmd::SetTracking { on })), &mut cx);
            }
            _ => {
                run_step(&mut s, Step::Redo, &mut cx);
            }
        }
        // the body selection saved when a cell was entered may end past the paragraphs (the TS keeps it, F3-D13): no check
    }
    for _ in 0..60 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..60 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    match s.save_docx() {
        Ok(bytes) => open(bytes).is_some(),
        Err(_) => true,
    }
}

/// Documents with tables first (the others only get the tables the commands insert).
fn with_tables() -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = sample(5)
        .into_iter()
        .filter(|f| {
            let Ok(b) = std::fs::read(f) else { return false };
            open(b).is_some_and(|s| !s.document().body.tables.is_empty())
        })
        .collect();
    out.extend(sample(97));
    out
}

#[test]
fn hostile_f3_commands_on_the_corpus() {
    let mut n = 0;
    let mut reopened = 0;
    for (i, f) in with_tables().iter().take(25).enumerate() {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        if hammer(s, 0xf3f3 + u32::try_from(i).unwrap(), 40) {
            reopened += 1;
        }
        n += 1;
    }
    println!("nopanic d2b-f3: {n} documents, {reopened} saves reopened");
    assert!(n >= 20);
    assert_eq!(n, reopened);
}

#[test]
fn hostile_f3_commands_on_mutated_documents() {
    let mut rng = Rng::new(0xf3bad);
    let mut opened = 0;
    let mut refused = 0;
    for f in sample(97) {
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
                hammer(s, rng.next_u32(), 25);
                opened += 1;
            }
            None => refused += 1,
        }
    }
    println!("nopanic d2b-f3 mutated: {opened} opened, {refused} refused");
    assert!(opened + refused > 15);
}

#[test]
fn hostile_formulas_on_tables() {
    let mut rng = Rng::new(0xf3f0);
    let mut n = 0;
    for f in sample(25) {
        let Some(s) = open(std::fs::read(f).unwrap()) else { continue };
        for a in s.document().body.tables.iter() {
            for _ in 0..30 {
                let _ = d2b_f3_support::hostile::hostile_formula(&mut rng, &a.table);
                n += 1;
            }
        }
    }
    println!("nopanic d2b-f3 formulas: {n}");
    assert!(n > 100);
}

#[test]
fn a_huge_table_is_bounded() {
    let mut cx = Cx::new();
    let mut s = Session::new_blank(uo_docx::session::OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).unwrap();
    // 32767 x 63 is Word's limit; the session caps the cells it builds at once
    s.exec(
        Command::Table(uo_docx::session::ops::tables::TableCmd::InsertTable {
            rows: 32767.0,
            cols: 63.0,
            autofit: None,
            col_width_twips: None,
            layout: Default::default(),
        }),
        &mut cx.cx(),
    )
    .unwrap();
    let cells: usize = s.document().body.tables.iter().map(|a| a.table.rows.iter().map(|r| r.cells.len()).sum::<usize>()).sum();
    assert!(cells <= 1_000_000, "{cells} cells");
}
