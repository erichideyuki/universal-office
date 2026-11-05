//! No panic on hostile input for D2b-F9 (record `docs/modules/uo-docx-d2b/F9.md`, "Security"):
//!
//! - every hostile pattern (exponential backtracking, the endless empty-match loop of V8 before an
//!   astral character, 3,000 nested groups, huge bounds, lone surrogates, broken specials) compiles
//!   or is refused, and its search over a long text ends inside the step budget (and in time);
//! - corpus documents × hostile F9 commands mixed with the D2a commands (selections at `u32::MAX`,
//!   indices at `i64::MIN/MAX`, stale search results, overlapping multi-selections, undo/redo
//!   storms, tracking on), then a save that reopens.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f9_support;
mod support;

use std::time::Instant;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use d2b_f9_support::hostile::{hostile_f9, hostile_patterns};
use uo_docx::model::para::Paragraph;
use uo_docx::model::run::Run;
use uo_docx::pvec::PVec;
use uo_docx::session::ops::find::engine::{FindFailure, FindQuery, NoEnv, compile_query, find_all, story_text};
use uo_docx::session::ops::find::regex::{Budget, Vm};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn long_story() -> PVec<Paragraph> {
    let mut paras = PVec::new();
    let line: Vec<u16> = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 😀 bbb ab-cd (x) [y] {z} \u{2019}s ".encode_utf16().collect();
    for i in 0..400u32 {
        let mut t = line.clone();
        t.extend(i.to_string().encode_utf16());
        let run = Run { text: Utf16::from_units(t), ..Run::default() };
        paras.push(Paragraph { runs: vec![run], ..Paragraph::default() });
    }
    paras
}

#[test]
fn hostile_patterns_stay_inside_the_budget() {
    let paras = long_story();
    let mut vm = Vm::default();
    let mut worst = (0u128, String::new());
    for (text, wild) in hostile_patterns() {
        for mc in [false, true] {
            let q = FindQuery { text: text.clone(), wildcards: wild, match_case: mc, ..FindQuery::default() };
            let Ok(c) = compile_query(&q) else { continue };
            let mut b = Budget::new(2_000_000);
            let st = story_text(&paras, 0, paras.len(), &mut b).unwrap();
            let t0 = Instant::now();
            let r = find_all(&st, &q, c.as_ref(), &NoEnv, &mut b, &mut vm);
            let ms = t0.elapsed().as_millis();
            assert!(b.used() <= 2_000_000);
            if ms > worst.0 {
                worst = (ms, String::from_utf16_lossy(&text).chars().take(40).collect());
            }
            if let Err(e) = r {
                assert_eq!(e, FindFailure::Budget);
            }
        }
    }
    println!("hostile patterns: slowest {} ms ({:?}) with a 2,000,000-step budget", worst.0, worst.1);
    assert!(worst.0 < 20_000);
}

#[test]
fn hostile_f9_commands_on_the_corpus() {
    let docs = sample(30);
    let mut cx = Cx::new();
    let (mut lives, mut reopened) = (0usize, 0usize);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let Some(mut s) = open(bytes) else { continue };
        s.set_find_budget(Some(200_000));
        let mut rng = Rng::new(0xf9 + u32::try_from(i).unwrap());
        for k in 0..120 {
            let step = match rng.below(10) {
                0..=5 => Step::Cmd(hostile_f9(&mut rng)),
                6 => Step::Cmd(Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 })),
                _ => next_step(&mut rng, &s),
            };
            run_step(&mut s, step, &mut cx);
            if k % 40 == 39 {
                for _ in 0..60 {
                    run_step(&mut s, Step::Undo, &mut cx);
                }
                for _ in 0..60 {
                    run_step(&mut s, Step::Redo, &mut cx);
                }
            }
            let _ = s.headings();
            let _ = s.selection_ranges();
            let _ = s.current_heading();
        }
        lives += 1;
        if let Ok(b) = s.save_docx()
            && Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_ok()
        {
            reopened += 1;
        }
    }
    println!("nopanic d2b-f9: {lives} documents x 120 hostile steps, {reopened} saves reopened");
    assert!(lives > 10);
    assert_eq!(lives, reopened);
}
