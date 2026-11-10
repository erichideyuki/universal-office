//! Memory gate of D2b-F2 (lists and styles; record uo-docx-d2b/F2.md):
//!
//! - **M1**: whole session lives with seeded F2 commands (lists, styles, the hooks of Enter and
//!   Backspace, hostile arguments and the error paths), undo, redo and save return the live bytes
//!   exactly.
//! - **M2**: soak of the same life: flat series.
//! - **M5**: the stores stay inside the history cap: 3,000 list changes (`UO_M5_N=10000` for the 10,000 of the
//!   brief: measured once, 102 s in release) and 400 style edits keep
//!   the version stores at ≤ the history length + 2 versions and the history at its byte cap.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f2_support;
#[allow(unused_imports)]
use crate::support;

use std::hint::black_box;

use d2a_support::{Cx, sample};
use d2b_f2_support::{f2_cmd, some_selection, typed};
use uo_alloc::{assert_flat, assert_no_leak, soak};
use uo_docx::session::ops::lists::ListCmd;
use uo_docx::session::ops::styles::{Ch, ModifyStyleSpec, StyleCmd, StyleFormat, UiLang};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, FixedEnv, OpenOptions, Session};
use uo_testkit::generate::Rng;

fn life(bytes: &[u8], seed: u32, wild: bool) -> usize {
    let mut cx = Cx::new();
    let Ok(mut s) = Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())) else { return 0 };
    s.set_ui_lang(UiLang::Pt);
    let mut rng = Rng::new(seed);
    for i in 0..80 {
        let c = match rng.below(6) {
            0 => some_selection(&mut rng, &s),
            1 => typed(&mut rng),
            2 if i % 5 == 0 => Command::Text(TextCmd::Backspace),
            _ => f2_cmd(&mut rng, wild),
        };
        let _ = s.exec(c, &mut cx.cx());
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    s.clear_caches();
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_f2_session_lives_return_the_memory() {
    let docs = sample(300);
    let mut n = 0;
    // the process-wide caches (the built-in style table, the blank document's latent styles) are
    // built once: warm them up outside the measured lives
    black_box(life(&std::fs::read(&docs[0]).unwrap(), 1, true));
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let i = u32::try_from(i).unwrap();
        assert_no_leak(|| black_box(life(&bytes, 0xf2 + i, false)));
        assert_no_leak(|| black_box(life(&bytes, 0xe2 + i, true)));
        n += 1;
    }
    // a blank document, every error path once
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        for c in [
            Command::List(ListCmd::ApplyListLibrary { bullet: true, index: Some(99) }),
            Command::List(ListCmd::SetListLevel { ilvl: f64::NAN }),
            Command::Style(StyleCmd::DeleteStyle { id: "NoSuch".into() }),
            Command::Style(StyleCmd::ApplyStyle { style_id: Some("NoSuch".into()) }),
        ] {
            let _ = s.exec(c, &mut cx.cx());
        }
        black_box(s.save_docx().is_ok())
    });
    println!("M1 d2b-f2: {n} documents x 2 lives + error paths");
    assert!(n > 5);
}

#[test]
fn m2_f2_soak() {
    let docs: Vec<Vec<u8>> = sample(300).iter().map(|f| std::fs::read(f).unwrap()).filter(|b| b.len() < 60_000).take(6).collect();
    assert!(docs.len() >= 3);
    let mut out = 0usize;
    let series = soak(300, 50, |i| {
        out = out.wrapping_add(life(&docs[i % docs.len()], u32::try_from(i % 5).unwrap(), i % 2 == 0));
    });
    println!("M2 soak d2b-f2: 300 lives over {} documents: {:?}", docs.len(), &series[..series.len().min(4)]);
    black_box(out);
    assert_flat(&series, 1, 0);
}

#[test]
fn m5_the_stores_stay_inside_the_history_cap() {
    let mut cx = Cx::new();
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    s.exec(Command::Text(TextCmd::Type { text: "item".into() }), &mut cx.cx()).unwrap();
    // a list item whose level format is re-applied `UO_M5_N` (3,000, or 10,000) times: a new numbering version each time (the
    // part itself does not grow, the item stays in its list)
    s.exec(Command::List(ListCmd::ToggleList { bullet: true }), &mut cx.cx()).unwrap();
    let mut peak = s.numbering_store().len();
    let t0 = std::time::Instant::now();
    let n: u32 = std::env::var("UO_M5_N").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000);
    for i in 0..n {
        s.exec(Command::List(ListCmd::ApplyListLibrary { bullet: i % 2 == 0, index: Some((i % 6) as usize) }), &mut cx.cx()).unwrap();
        peak = peak.max(s.numbering_store().len());
    }
    println!("{n} applications took {:?}", t0.elapsed());
    let steps = s.history().undo_len();
    println!(
        "M5 d2b-f2 lists: {n} list changes: history {steps} steps, {} KiB, numbering versions kept: {} (peak {peak})",
        s.history_bytes() / 1024,
        s.numbering_store().len()
    );
    // toggling on and off adds a list each time (as the TS: the part only grows): 400 rounds
    for i in 0..400u32 {
        s.exec(Command::List(ListCmd::ToggleList { bullet: i % 3 != 0 }), &mut cx.cx()).unwrap();
        peak = peak.max(s.numbering_store().len());
    }
    assert!(steps <= 200);
    assert!(s.numbering_store().len() <= steps + 3, "kept {}", s.numbering_store().len());
    assert!(peak <= 210);
    // the style sheet: 400 modifications
    for i in 0..400u32 {
        let spec = ModifyStyleSpec {
            format: StyleFormat { sz_half_pt: Ch::Val(f64::from(20 + i % 40)), ..StyleFormat::default() },
            ..ModifyStyleSpec::default()
        };
        s.exec(Command::Style(StyleCmd::ModifyStyle { id: "Normal".into(), spec: Box::new(spec) }), &mut cx.cx()).unwrap();
    }
    println!("M5 d2b-f2 styles: 400 edits: {} style versions kept, {} KiB history", s.styles_store().len(), s.history_bytes() / 1024);
    assert!(s.styles_store().len() <= s.history().undo_len() + 3);
    // the 200 steps do not hold more than a few copies of the two parts
    let parts = s.styles_store().bytes() + s.numbering_store().bytes();
    assert!(parts < 250 * 200_000, "{parts}");
}
