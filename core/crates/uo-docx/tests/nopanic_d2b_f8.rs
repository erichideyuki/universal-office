//! No panic in the clipboard family (D2b-F8, record `core/docs/modules/uo-docx-d2b/F8.md`):
//! hostile paste-tree payloads (random bytes, every truncation, lengths 0xFFFFFFFF, sizes out of the
//! tree, unknown kinds, past the limits, deep trees at the limit), hostile table grids (spans
//! `1e309`/`-5`/`999999999`, a 10k × 10k grid), and hostile command scripts in every context with
//! failed and garbage jobs, 300 undo + 300 redo and a save that reopens, and hostile Word lists
//! (200,000 `@list` definitions, a million unterminated ones; time-bounded). Bounded to seconds;
//! `UO_NOPANIC_BIG=1` for more.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2a_support::{Cx, sample};
use d2b_f8_support::{Shell, cmd, f8_step, layout, run_f8, sample_html};
use uo_docx::ids::IdAlloc;
use uo_docx::session::ops::clipboard::cells::tables_from_html;
use uo_docx::session::ops::clipboard::html::html_to_paras;
use uo_docx::session::ops::clipboard::tree::{HtmlCell, HtmlTable, PasteHtml, PasteLimits, TreeBuilder, decode_payload, encode_payload};
use uo_docx::session::ops::clipboard::{ClipboardCmd, RichPick, clipboard_items};
use uo_docx::session::{FixedEnv, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn big() -> bool {
    std::env::var("UO_NOPANIC_BIG").is_ok()
}

#[test]
fn hostile_payloads_never_panic() {
    let lim = PasteLimits::default();
    let good = sample_html();
    for n in 0..good.len() {
        let _ = decode_payload(&good[..n], &lim);
    }
    let mut rng = Rng::new(0xf8f8);
    let rounds = if big() { 200_000 } else { 20_000 };
    for i in 0..rounds {
        let mut b = good.clone();
        match i % 4 {
            0 => {
                let k = rng.below(b.len());
                b[k] = u8::try_from(rng.below(256)).unwrap();
            }
            1 => {
                // a length or size of 0xFFFFFFFF
                let k = rng.below(b.len().saturating_sub(4));
                b[k..k + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            }
            2 => b = (0..rng.below(200)).map(|_| u8::try_from(rng.below(256)).unwrap()).collect(),
            _ => {
                let k = rng.below(b.len());
                b.insert(k, u8::try_from(rng.below(256)).unwrap());
            }
        }
        if let Ok(h) = decode_payload(&b, &lim) {
            let _ = html_to_paras(&h);
            let _ = tables_from_html(&h.tables, 9000.0, &mut IdAlloc::default());
        }
    }
    // a valid tree at the depth limit, and wide
    let mut t = TreeBuilder::body();
    for _ in 0..1000 {
        t.open(["DIV", "UL", "LI", "TR", "SPAN", "PRE"][rng.below(6)], &[("style", "mso-list:l0 level1 lfo1")]);
    }
    t.text("x");
    let h = PasteHtml { raw: Utf16::from("@list l0:level1 {mso-level-number-format:bullet}"), fragment: t.finish(), tables: Vec::new() };
    let d = decode_payload(&encode_payload(&h), &lim).unwrap();
    assert!(!html_to_paras(&d).is_empty());
}

#[test]
fn hostile_table_grids_never_panic() {
    let cell =
        |cs: &str, rs: &str| HtmlCell { th: false, colspan: Some(cs.into()), rowspan: Some(rs.into()), paras: vec![Utf16::from(" a ")] };
    let spans = ["1e309", "-5", "999999999", "0", "63", "64", "x", "2", "1e9"];
    let mut rows = Vec::new();
    for r in 0..12 {
        rows.push((0..8).map(|c| cell(spans[(r + c) % spans.len()], spans[(r * 3 + c) % spans.len()])).collect());
    }
    let t = tables_from_html(&[HtmlTable { rows }], 9000.0, &mut IdAlloc::default());
    assert_eq!(t.len(), 1);
    // a 10k x 10k grid stays bounded (F8-D12)
    let n = if big() { 10_000 } else { 2_000 };
    let wide = HtmlTable { rows: vec![vec![cell("63", &n.to_string()); n / 63 + 1]; n] };
    let t0 = std::time::Instant::now();
    let _ = tables_from_html(&[wide], 9000.0, &mut IdAlloc::default());
    println!("no-panic d2b-f8: a {n} x {} grid in {} ms", 63 * (n / 63 + 1), t0.elapsed().as_millis());
}

#[test]
fn hostile_clipboard_scripts_never_panic() {
    let docs = if big() { sample(1) } else { sample(40) };
    let steps = if big() { 200 } else { 60 };
    let (mut opened, mut saved) = (0, 0);
    let t0 = std::time::Instant::now();
    let mut cx = Cx::new();
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let Ok(mut s) = Session::open(
            bytes,
            OpenOptions { session_id: u32::try_from(i).unwrap(), ..OpenOptions::default() },
            Box::new(FixedEnv::default()),
        ) else {
            continue;
        };
        opened += 1;
        let mut rng = Rng::new(0xf8c0 + u32::try_from(i).unwrap());
        for j in 0..steps {
            let ids: Vec<u32> = clipboard_items(&cx.clip).iter().map(|x| x.id).collect();
            let c = f8_step(&mut rng, &s, &ids, true);
            run_f8(&mut s, &mut cx, c, &mut rng, if j % 3 == 0 { Shell::Hostile } else { Shell::Plain }, j);
        }
        // a Paste Link whose source range is stale, a garbage tree
        let _ = s.exec(cmd(ClipboardCmd::AddLinkSource { item: Some(u32::MAX) }), &mut cx.cx());
        let _ = s.exec(
            cmd(ClipboardCmd::PasteData {
                text: Utf16::new(),
                html: Some(Arc::from(vec![0u8; 3])),
                image: None,
                rich: RichPick::WithRich,
                mode: None,
                list: None,
                layout: layout(),
            }),
            &mut cx.cx(),
        );
        for _ in 0..300 {
            s.undo(&mut cx.cx());
        }
        for _ in 0..300 {
            s.redo(&mut cx.cx());
        }
        if let Ok(b) = s.save_docx() {
            assert!(Session::open(b, OpenOptions::default(), Box::new(FixedEnv::default())).is_ok(), "{f:?}: the save does not reopen");
            saved += 1;
        }
    }
    println!("no-panic d2b-f8: {opened} documents x {steps} hostile steps, {saved} saves reopened, {} ms", t0.elapsed().as_millis());
    assert_eq!(opened, saved);
}

/// Hostile Word lists: 200,000 distinct `@list` definitions with as many `mso-list` paragraphs
/// (the TS `Map` lookups: linear here too), and 1,000,000 unterminated `@list … {` starts.
#[test]
fn hostile_word_lists_are_linear() {
    use std::fmt::Write as _;
    let n = 200_000;
    let mut raw = String::with_capacity(n * 56);
    raw.push_str("<style>");
    for i in 0..n {
        let _ = writeln!(raw, "@list l{i}:level1 {{mso-level-number-format:{}}}", if i % 2 == 0 { "bullet" } else { "decimal" });
    }
    raw.push_str("</style>");
    let mut b = TreeBuilder::body();
    for i in 0..n {
        b.open("P", &[("style", &format!("mso-list:l{i} level1 lfo{i}"))]).close();
    }
    let h = PasteHtml { raw: Utf16::from(raw.as_str()), fragment: b.finish(), tables: Vec::new() };
    let t = std::time::Instant::now();
    let paras = html_to_paras(&h);
    let ms = t.elapsed().as_millis();
    assert_eq!(paras.len(), n);
    assert_eq!(paras.iter().filter(|p| p.block.list.is_some_and(|l| !l.ordered)).count(), n / 2);
    // the unterminated starts: no `}` after them, no rescan to the end from each
    let open = "@list l1:level1 {".repeat(1_000_000);
    let t2 = std::time::Instant::now();
    let defs = uo_docx::session::ops::clipboard::html::word_list_formats(Utf16::from(open.as_str()).units());
    let ms2 = t2.elapsed().as_millis();
    assert!(defs.is_empty());
    println!("no-panic d2b-f8: {n} Word lists in {ms} ms; 1,000,000 unterminated @list in {ms2} ms");
    // quadratic (the Vec lookups) took minutes; linear is well under a second in release
    assert!(ms < 20_000 && ms2 < 20_000, "{ms} ms / {ms2} ms");
}
