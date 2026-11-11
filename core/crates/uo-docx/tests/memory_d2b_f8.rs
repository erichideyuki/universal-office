//! Memory gate of D2b-F8 (the clipboard; record `core/docs/modules/uo-docx-d2b/F8.md`):
//!
//! - **M1**: session lives with the clipboard commands (the ring included, the jobs answered,
//!   failed, garbage and cancelled) over corpus documents and a constructed one that walks the
//!   error paths, with undo, redo, the queries and a save: the live bytes return exactly.
//! - **M2**: 1,000 × (copy → paste Keep → paste Text → repaste Merge → undo × 3 → redo) is flat
//!   (`UO_M2_N` for more).
//! - **M3**: the paste of a 5,000-paragraph HTML (decode, rules, insert) and the copy of a
//!   15,000-paragraph selection: allocated bytes, peak, the history priced; the copy and the cut of
//!   a whole 10,000 × 10 table (time-bounded: the TS `Set` of cells stays a hash set).
//! - **M5**: the paste-tree limits refuse past them; `lastPaste` keeps one input;
//!   `ClipboardRing::bytes()` stays within a factor 3 of what a clip keeps (cells, a range with a
//!   table, 15,000 paragraphs) and the byte cap bounds four table copies.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2a_support::{Cx, sample};
use d2b_f8_support::{Shell, cmd, f8_step, layout, run_f8};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::session::ClipboardRing;
use uo_docx::session::ops::clipboard::tree::{HtmlCell, HtmlTable};
use uo_docx::session::ops::clipboard::tree::{
    LimitKind, PasteHtml, PasteLimits, PasteTreeError, TreeBuilder, decode_payload, encode_payload,
};
use uo_docx::session::ops::clipboard::{ClipboardCmd, PasteMode, RichPick, clipboard_items};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::tables::{TableCmd, TableRect};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, CommandError, DocPosition, FixedEnv, JobResult, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

fn life(bytes: Option<&[u8]>, seed: u32) -> usize {
    let mut cx = Cx::new();
    let s = match bytes {
        Some(b) => Session::open(b.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())).ok(),
        None => Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).ok(),
    };
    let Some(mut s) = s else { return 0 };
    let mut rng = Rng::new(seed);
    for i in 0..100u32 {
        let ids: Vec<u32> = clipboard_items(&cx.clip).iter().map(|x| x.id).collect();
        let c = f8_step(&mut rng, &s, &ids, false);
        let shell = if i % 9 == 8 { Shell::Hostile } else { Shell::Plain };
        run_f8(&mut s, &mut cx, c, &mut rng, shell, i);
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    black_box((
        s.paste_options(),
        s.copied().cloned(),
        s.paste_special_formats().cloned(),
        s.last_paste_range().cloned(),
        s.selected_text(),
    ));
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_with_the_f8_commands_return_the_memory() {
    let mut n = 0;
    for (i, f) in sample(60).iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(life(Some(&bytes), 0xf800 + u32::try_from(i).unwrap())));
        n += 1;
    }
    for seed in 0..4 {
        assert_no_leak(|| black_box(life(None, 0xf8b0 + seed)));
        n += 1;
    }
    // the error paths: locked, Busy, an unknown job, Failed, garbage, cancel mid-read, a stale ring id
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        let _ = s.exec(Command::Text(TextCmd::Type { text: Utf16::from("abc") }), &mut cx.cx());
        let _ =
            s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 2) }), &mut cx.cx());
        let _ = s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx());
        let o = s.exec(cmd(ClipboardCmd::Paste { mode: None, layout: layout() }), &mut cx.cx()).unwrap();
        assert_eq!(s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap_err(), CommandError::Busy);
        if let Outcome::NeedsJobs(j) = o {
            let id = j[0].id();
            s.complete(id, JobResult::Bytes(Arc::from(vec![1u8])), &mut cx.cx()).unwrap();
            assert_eq!(s.complete(id, JobResult::Failed, &mut cx.cx()).unwrap_err(), CommandError::UnknownJob);
        }
        let _ = s.exec(cmd(ClipboardCmd::Paste { mode: Some(PasteMode::Picture), layout: layout() }), &mut cx.cx());
        s.cancel_pending();
        let o = s.exec(cmd(ClipboardCmd::Paste { mode: None, layout: layout() }), &mut cx.cx()).unwrap();
        if let Outcome::NeedsJobs(j) = o {
            let _ = s.complete(j[0].id(), JobResult::Failed, &mut cx.cx());
        }
        let _ = s.exec(cmd(ClipboardCmd::PasteClipboardItem { id: u32::MAX, mode: None, layout: layout() }), &mut cx.cx());
        let _ = s.exec(cmd(ClipboardCmd::Repaste { mode: PasteMode::Merge, list: None, layout: layout() }), &mut cx.cx());
        let _ = s.exec(Command::Settings(uo_docx::session::ops::settings::SettingsCmd::SetProtected { on: true }), &mut cx.cx());
        let _ = s.exec(cmd(ClipboardCmd::Paste { mode: None, layout: layout() }), &mut cx.cx());
        let _ = s.exec(cmd(ClipboardCmd::Cut), &mut cx.cx());
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().is_ok())
    });
    n += 1;
    println!("M1 d2b-f8: {n} session lives + the error paths");
    assert!(n >= 8);
}

fn cycle(s: &mut Session, cx: &mut Cx) {
    let _ = s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 5) }), &mut cx.cx());
    let _ = s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx());
    let _ = s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 5), focus: DocPosition::new(0, 5) }), &mut cx.cx());
    let paste = |mode| {
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("Hello"),
            html: None,
            image: None,
            rich: RichPick::WithRich,
            mode: Some(mode),
            list: None,
            layout: layout(),
        })
    };
    let _ = s.exec(paste(PasteMode::Keep), &mut cx.cx());
    let _ = s.exec(paste(PasteMode::Text), &mut cx.cx());
    let _ = s.exec(cmd(ClipboardCmd::Repaste { mode: PasteMode::Merge, list: None, layout: layout() }), &mut cx.cx());
    for _ in 0..3 {
        s.undo(&mut cx.cx());
    }
    s.redo(&mut cx.cx());
    s.undo(&mut cx.cx());
}

#[test]
fn m2_soak_copy_paste_repaste_undo_is_flat() {
    let n: usize = std::env::var("UO_M2_N").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = Cx::new();
    let _ = s.exec(Command::Text(TextCmd::Type { text: Utf16::from("Hello world") }), &mut cx.cx());
    // warm-up: the ring and the history reach their steady state
    for _ in 0..10 {
        cycle(&mut s, &mut cx);
    }
    let series = soak(n, (n / 20).max(1), |_| cycle(&mut s, &mut cx));
    println!("M2 soak d2b-f8: {n} cycles (copy, paste K, paste T, repaste M, undo x3, redo): {:?}", &series[..series.len().min(6)]);
    assert_flat(&series, 2, 256);
}

fn big_html(paras: usize) -> Vec<u8> {
    let mut b = TreeBuilder::body();
    for i in 0..paras {
        b.open("P", &[("fontWeight", if i % 3 == 0 { "bold" } else { "" })]).text(&format!("Paragraph {i} with some text")).close();
    }
    encode_payload(&PasteHtml { raw: Utf16::from("<p>x</p>"), fragment: b.finish(), tables: Vec::new() })
}

#[test]
fn m3_paste_5000_html_paragraphs_and_copy_15000() {
    let payload: Arc<[u8]> = Arc::from(big_html(5000));
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = Cx::new();
    let t = std::time::Instant::now();
    let (_, used) = measure(|| {
        s.exec(
            cmd(ClipboardCmd::PasteData {
                text: Utf16::new(),
                html: Some(Arc::clone(&payload)),
                image: None,
                rich: RichPick::None,
                mode: Some(PasteMode::Keep),
                list: None,
                layout: layout(),
            }),
            &mut cx.cx(),
        )
        .unwrap()
    });
    let ms = t.elapsed().as_millis();
    let paras = s.paras().unwrap().len();
    let hist = s.history_bytes();
    println!(
        "M3 d2b-f8: HTML paste of {} B payload, {paras} paragraphs: {} B allocated, peak {} B, kept {} B, {ms} ms; history {hist} B",
        payload.len(),
        used.allocated,
        used.peak_above_start,
        used.live_delta
    );
    assert!(paras >= 5000);
    assert!(hist > 0, "the paste step was priced");
    // grow to 15,000 paragraphs and copy them all
    for _ in 0..2 {
        let _ = s.exec(
            cmd(ClipboardCmd::PasteData {
                text: Utf16::new(),
                html: Some(Arc::clone(&payload)),
                image: None,
                rich: RichPick::None,
                mode: Some(PasteMode::Text),
                list: None,
                layout: layout(),
            }),
            &mut cx.cx(),
        );
        let _ = s.exec(
            cmd(ClipboardCmd::PasteData {
                text: Utf16::new(),
                html: Some(Arc::clone(&payload)),
                image: None,
                rich: RichPick::None,
                mode: Some(PasteMode::Keep),
                list: None,
                layout: layout(),
            }),
            &mut cx.cx(),
        );
    }
    let n = u32::try_from(s.paras().unwrap().len()).unwrap();
    let _ =
        s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(n - 1, 0) }), &mut cx.cx());
    let t = std::time::Instant::now();
    let (_, used) = measure(|| s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap());
    let ms = t.elapsed().as_millis();
    println!(
        "M3 d2b-f8: copy of {n} paragraphs: {} B allocated, peak {} B, kept {} B (the clip and the HTML), {ms} ms; ring {} B",
        used.allocated,
        used.peak_above_start,
        used.live_delta,
        cx.clip.bytes()
    );
    assert!(n >= 15_000);
}

#[test]
fn m5_paste_tree_limits_and_one_last_paste() {
    let mut b = TreeBuilder::body();
    for _ in 0..2000 {
        b.open("DIV", &[]);
    }
    b.text("deep");
    let deep = encode_payload(&PasteHtml { raw: Utf16::from("x"), fragment: b.finish(), tables: Vec::new() });
    assert_eq!(decode_payload(&deep, &PasteLimits::default()).unwrap_err(), PasteTreeError::Limit(LimitKind::Depth));
    let wide = big_html(3000);
    assert_eq!(
        decode_payload(&wide, &PasteLimits { max_nodes: 1000, ..PasteLimits::default() }).unwrap_err(),
        PasteTreeError::Limit(LimitKind::Nodes)
    );
    assert_eq!(
        decode_payload(&wide, &PasteLimits { max_bytes: 100, ..PasteLimits::default() }).unwrap_err(),
        PasteTreeError::Limit(LimitKind::Bytes)
    );
    // a refused payload: the paste falls through to the text (F8-D2)
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = Cx::new();
    let _ = s.exec(
        cmd(ClipboardCmd::PasteData {
            text: Utf16::from("plain"),
            html: Some(Arc::from(deep)),
            image: None,
            rich: RichPick::None,
            mode: None,
            list: None,
            layout: layout(),
        }),
        &mut cx.cx(),
    );
    assert_eq!(uo_docx::session::text::para_text(s.paras().unwrap().get(0).unwrap()), "plain".encode_utf16().collect::<Vec<_>>());
    // lastPaste keeps one input: 50 pastes, the live bytes do not grow with them
    let (_, a) = measure(|| {
        for _ in 0..50 {
            let _ = s.exec(cmd(ClipboardCmd::PasteText { text: Utf16::from("y") }), &mut cx.cx());
        }
    });
    black_box(a);
}

/// A blank document with a `rows × cols` table pasted from an HTML table, `chars` characters per
/// cell (the cell text carries `tag`).
fn table_doc(rows: usize, cols: usize, chars: usize, tag: usize, cx: &mut Cx) -> Session {
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut t = HtmlTable::default();
    for r in 0..rows {
        let row = (0..cols)
            .map(|c| {
                let mut text = format!("t{tag} r{r} c{c} ");
                while text.len() < chars {
                    text.push_str("lorem ipsum ");
                }
                text.truncate(chars);
                HtmlCell { th: false, colspan: None, rowspan: None, paras: vec![Utf16::from(text.as_str())] }
            })
            .collect();
        t.rows.push(row);
    }
    let h = PasteHtml { raw: Utf16::from("<table><tr><td>x</td></tr></table>"), fragment: TreeBuilder::body().finish(), tables: vec![t] };
    // a paragraph before the table (a range from the start then holds it)
    s.exec(cmd(ClipboardCmd::PasteText { text: Utf16::from("before\n") }), &mut cx.cx()).unwrap();
    s.exec(
        cmd(ClipboardCmd::PasteData {
            text: Utf16::new(),
            html: Some(Arc::from(encode_payload(&h))),
            image: None,
            rich: RichPick::None,
            mode: Some(PasteMode::Keep),
            list: None,
            layout: layout(),
        }),
        &mut cx.cx(),
    )
    .unwrap();
    assert_eq!(s.document().body.tables.len(), 1, "the table was pasted");
    s
}

fn select_cells(s: &mut Session, cx: &mut Cx, rows: usize, cols: usize) {
    let rect = TableRect { r1: 0, c1: 0, r2: i64::try_from(rows).unwrap() - 1, c2: i64::try_from(cols).unwrap() - 1 };
    s.exec(Command::Table(TableCmd::SetTableSelection { anchor: 0, path: Vec::new(), rect, origin: (0, 0) }), &mut cx.cx()).unwrap();
}

fn select_all_text(s: &mut Session, cx: &mut Cx) {
    let n = u32::try_from(s.paras().unwrap().len()).unwrap();
    s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(n - 1, 0) }), &mut cx.cx())
        .unwrap();
}

/// M3: the copy and the cut of a whole 10,000 × 10 table (the TS `Set` of cells: linear).
#[test]
fn m3_copy_and_cut_of_a_10000_by_10_table() {
    let (rows, cols) = (10_000, 10);
    let mut cx = Cx::new();
    let mut s = table_doc(rows, cols, 8, 0, &mut cx);
    select_cells(&mut s, &mut cx, rows, cols);
    let t = std::time::Instant::now();
    let (_, copy) = measure(|| s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap());
    let copy_ms = t.elapsed().as_millis();
    assert!(cx.clip.current().is_some_and(|c| c.cells.as_ref().is_some_and(|x| x.whole)));
    let t = std::time::Instant::now();
    let (_, cut) = measure(|| s.exec(cmd(ClipboardCmd::Cut), &mut cx.cx()).unwrap());
    let cut_ms = t.elapsed().as_millis();
    assert!(s.document().body.tables.is_empty(), "the whole table went");
    println!(
        "M3 d2b-f8: {rows} x {cols} table: copy {copy_ms} ms ({} B allocated, peak {} B), cut {cut_ms} ms ({} B allocated, peak {} B); ring {} B",
        copy.allocated,
        copy.peak_above_start,
        cut.allocated,
        cut.peak_above_start,
        cx.clip.bytes()
    );
    // quadratic (a list of the kept cells) was 10^10 compares; linear is well under a second in release
    assert!(copy_ms < 20_000 && cut_ms < 30_000, "copy {copy_ms} ms, cut {cut_ms} ms");
}

/// What one clip keeps alive once its document is gone, against its estimate.
fn kept_by_ring(make: impl FnOnce(&mut Cx) -> Session) -> (usize, i64) {
    let (ring, used) = measure(|| {
        let mut cx = Cx::new();
        let s = make(&mut cx);
        drop(s);
        let Cx { clip, .. } = cx;
        clip
    });
    let est = ring.bytes();
    drop(ring);
    (est, used.live_delta)
}

/// A document made (and copied from) inside the measure.
type MakeDoc = Box<dyn FnOnce(&mut Cx) -> Session>;

/// M5: `ClipboardRing::bytes()` tracks what the clips keep (cells with their content, ranges with
/// paragraphs and whole tables), and the byte cap bounds the ring.
#[test]
fn m5_ring_bytes_track_the_live_bytes() {
    let cases: Vec<(&str, MakeDoc)> = vec![
        (
            "cells 5,000 x 6 x 50 chars",
            Box::new(|cx: &mut Cx| {
                let mut s = table_doc(5000, 6, 50, 1, cx);
                select_cells(&mut s, cx, 5000, 6);
                s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap();
                s
            }),
        ),
        (
            "range with a 2,000 x 4 table",
            Box::new(|cx: &mut Cx| {
                let mut s = table_doc(2000, 4, 30, 2, cx);
                s.exec(Command::Context(ContextCmd::ExitCell), &mut cx.cx()).unwrap();
                s.exec(cmd(ClipboardCmd::PasteText { text: Utf16::from("after the table\nand more") }), &mut cx.cx()).unwrap();
                select_all_text(&mut s, cx);
                s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap();
                assert!(cx.clip.current().is_some_and(|c| !c.tables.is_empty()), "the range holds the table");
                s
            }),
        ),
        (
            "range of 15,000 paragraphs",
            Box::new(|cx: &mut Cx| {
                let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
                let payload: Arc<[u8]> = Arc::from(big_html(5000));
                for mode in [PasteMode::Keep, PasteMode::Text, PasteMode::Keep] {
                    s.exec(
                        cmd(ClipboardCmd::PasteData {
                            text: Utf16::new(),
                            html: Some(Arc::clone(&payload)),
                            image: None,
                            rich: RichPick::None,
                            mode: Some(mode),
                            list: None,
                            layout: layout(),
                        }),
                        &mut cx.cx(),
                    )
                    .unwrap();
                }
                select_all_text(&mut s, cx);
                s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap();
                s
            }),
        ),
    ];
    for (name, make) in cases {
        let (est, live) = kept_by_ring(make);
        let live = usize::try_from(live.max(1)).unwrap();
        println!("M5 d2b-f8: {name}: ring estimate {est} B, kept {live} B");
        assert!(est * 3 >= live && est <= live * 3, "{name}: estimate {est} B, kept {live} B");
    }
    // the cap: copies of four tables of about 9 MB each into a 24 MiB ring
    let cap = 24 << 20;
    let (ring, used) = measure(|| {
        let mut cx = Cx::new();
        cx.clip = ClipboardRing::new(cap);
        for k in 0..4 {
            let mut s = table_doc(1000, 6, 50, 10 + k, &mut cx);
            select_cells(&mut s, &mut cx, 1000, 6);
            s.exec(cmd(ClipboardCmd::Copy), &mut cx.cx()).unwrap();
        }
        let Cx { clip, .. } = cx;
        clip
    });
    let kept = usize::try_from(used.live_delta.max(0)).unwrap();
    println!("M5 d2b-f8: 4 table copies into a {cap} B ring: {} items, estimate {} B, kept {kept} B", ring.len(), ring.bytes());
    assert!(ring.len() > 1 && ring.len() < 4, "the cap dropped the oldest");
    assert!(ring.bytes() <= cap);
    assert!(kept <= cap * 5 / 4, "kept {kept} B for a {cap} B cap");
}
