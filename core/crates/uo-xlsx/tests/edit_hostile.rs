//! C5b hostile and memory tests of the editor session: seeded random operation sequences (every operation family, undo,
//! redo, save, reopen) over the committed workbooks — no panic, the store's and the engine's invariants after every step,
//! a save reopens to the same store facet —; M1 (a script and its undo/redo leave no live byte after drop), M2 (1,000
//! steady edits are flat once the history is full) and M5 (the history byte cap holds).
//!
//! Run: `cargo test -p uo-xlsx --features edit,facet --release --test edit_hostile -- --nocapture`
//! (`UO_EDIT_SEEDS=<n>` random scripts per workbook, default 3).
#![cfg(all(feature = "edit", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation, clippy::too_many_lines)]

#[path = "support/edit_run.rs"]
mod edit_run;

use edit_run::*;
use uo_calc::{Clock, RandomSource};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::save::{AlignChange, FontChange, StyleChange};
use uo_xlsx::session::{BorderKind, ClearKind, FindOptions, HistoryLimits, MergeKind, NumFmtArg, Session, SessionOpts};

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u32) as usize]
    }
}

const TEXTS: &[&str] = &[
    "42",
    "-0,5",
    "1,5",
    "50%",
    "12/03/2024",
    "R$ 10,00",
    "texto ção",
    "😀 x",
    "  esp  ",
    "'007",
    "1E+21",
    "VERDADEIRO",
    "=SOMA(A1:A5)*2",
    "=SE(A2>1;\"s\";\"n\")",
    "=SEQUÊNCIA(3;2)",
    "=A1&\"x\"",
    "=1/0",
    "=SOMA(",
    "=B2+1",
    "",
    "\u{FFFF}\u{10FFFF}",
    "=LET(x;1;x+1)",
    "=ÚNICO(A1:A6)",
    "#N/D",
    "=A1:A3",
    "1/2",
    "\t\r\n",
];

fn cell(r: &mut Rng) -> String {
    format!("{}{}", uo_calc::a1::col_to_letters(r.below(12)), 1 + r.below(40))
}

fn range(r: &mut Rng) -> String {
    let (c, row) = (r.below(10), 1 + r.below(30));
    match r.below(10) {
        0 => format!("{}:{}", uo_calc::a1::col_to_letters(c), uo_calc::a1::col_to_letters(c + r.below(3))),
        1 => format!("{row}:{}", row + r.below(3)),
        _ => format!("{}{row}:{}{}", uo_calc::a1::col_to_letters(c), uo_calc::a1::col_to_letters(c + r.below(4)), row + r.below(5)),
    }
}

/// One random operation.
fn step(s: &mut Session, r: &mut Rng) -> String {
    let op = r.below(30);
    let sel = |s: &mut Session, x: &str| {
        let _ = s.select_range(x);
    };
    match op {
        0..=5 => {
            let (c, t) = (cell(r), *r.pick(TEXTS));
            sel(s, &c);
            s.set_active_cell_text(&u(t));
            format!("text {c} {t:?}")
        }
        6 => {
            sel(s, &range(r));
            s.set_font(FontChange {
                bold: Some(r.below(2) == 0),
                size: Some(*r.pick(&[9.0, 14.0, 409.0, 0.0, f64::NAN])),
                ..FontChange::default()
            });
            "font".into()
        }
        7 => {
            sel(s, &range(r));
            let all = [
                NumFmtArg::Kind(uo_xlsx::grid::NumberFormatKind::Percent),
                NumFmtArg::Code("0.000".into()),
                NumFmtArg::Code("\"x\"@".into()),
                NumFmtArg::Id(14.0, None),
            ];
            let k = r.pick(&all);
            s.set_number_format(k);
            "numfmt".into()
        }
        8 => {
            sel(s, &range(r));
            s.set_borders(*r.pick(&[BorderKind::All, BorderKind::ThickOutside, BorderKind::None, BorderKind::Left]), "#FF0000");
            "borders".into()
        }
        9 => {
            sel(s, &range(r));
            s.set_format(&StyleChange {
                fill: Some(Some("#FFFF00".into())),
                align: Some(AlignChange { wrap_text: Some(true), indent: Some(3.0), ..AlignChange::default() }),
                ..StyleChange::default()
            });
            "format".into()
        }
        10 => {
            sel(s, &range(r));
            s.merge(*r.pick(&[MergeKind::Center, MergeKind::Across, MergeKind::Cells]));
            "merge".into()
        }
        11 => {
            sel(s, &range(r));
            s.unmerge();
            "unmerge".into()
        }
        12 => {
            sel(s, &range(r));
            s.fill(r.below(2) == 0);
            "fill".into()
        }
        13 => {
            sel(s, &range(r));
            let cut = r.below(3) == 0;
            s.copy(cut);
            sel(s, &cell(r));
            s.paste(None, r.below(4) == 0);
            "copy/paste".into()
        }
        14 => {
            sel(s, &cell(r));
            s.paste(Some(&u("1\t\"a\"\"b\"\r\n=A1\t\t\r\n")), false);
            "paste text".into()
        }
        15 => {
            sel(s, &range(r));
            s.clear(*r.pick(&[ClearKind::Contents, ClearKind::Formats, ClearKind::All]));
            "clear".into()
        }
        16 => {
            let q = *r.pick(&["a", "1", "ção", "", "=", "TXT"]);
            s.replace_all(&u(q), &u("Z"), FindOptions { all_sheets: r.below(2) == 0, ..FindOptions::default() });
            "replaceAll".into()
        }
        17 => {
            s.find(&u("1"), FindOptions { backwards: r.below(2) == 0, look_in_values: r.below(2) == 0, ..FindOptions::default() });
            "find".into()
        }
        18 => {
            s.freeze_panes(Some(f64::from(r.below(4))), Some(f64::from(r.below(4))));
            s.set_show_gridlines(r.below(2) == 0);
            "freeze".into()
        }
        19 => {
            sel(s, &range(r));
            s.set_column_width(*r.pick(&[0.0, 0.5, 20.0, 300.0, -1.0, f64::NAN]), None);
            "width".into()
        }
        20 => {
            sel(s, &range(r));
            s.set_row_height(*r.pick(&[0.0, 30.0, 500.0]), None);
            "height".into()
        }
        21 => {
            sel(s, &range(r));
            s.set_hidden(r.below(2) == 0, r.below(2) == 0);
            "hide".into()
        }
        22 => {
            sel(s, &range(r));
            s.auto_sum(r.pick(&["SUM", "MAX", "AVERAGE"]));
            s.commit_edit();
            "autosum".into()
        }
        23 => {
            sel(s, &range(r));
            s.autofit_columns(None);
            s.autofit_rows(None);
            "autofit".into()
        }
        24 | 25 => {
            s.undo();
            "undo".into()
        }
        26 => {
            s.redo();
            "redo".into()
        }
        27 => {
            s.recalculate();
            "recalc".into()
        }
        28 => {
            let n = s.views.len();
            s.select_sheet(r.below(n as u32) as usize);
            "select sheet".into()
        }
        _ => {
            sel(s, &cell(r));
            s.delete_key();
            "delete".into()
        }
    }
}

fn invariants(s: &Session, what: &str) {
    s.book.store.check_invariants().unwrap_or_else(|e| panic!("{what}: store {e}"));
    if let Some(e) = s.book.engine.as_ref() {
        e.check_invariants(&s.book.store).unwrap_or_else(|x| panic!("{what}: engine {x}"));
    }
}

fn workbooks() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for i in 0..30 {
        let k = format!("x1/{i:04}");
        if let Some(b) = wb_bytes(&k) {
            out.push((k, b));
        }
    }
    for n in ["demo.xlsx", "showcase.xlsx", "smoke.xlsx", "merge-smoke.xlsx", "gaps-smoke.xlsx"] {
        let k = format!("samples/{n}");
        if let Some(b) = wb_bytes(&k) {
            out.push((k, b));
        }
    }
    out
}

#[test]
fn random_sequences_hold_invariants_and_round_trip() {
    let seeds: u32 = std::env::var("UO_EDIT_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut steps = 0;
    let mut roundtrips = 0;
    for (k, bytes) in workbooks() {
        for seed in 1..=seeds {
            let mut r = Rng(seed.wrapping_mul(2_654_435_761) ^ (k.len() as u32) ^ 0x9E37);
            let mut s = Session::open_with(
                bytes.clone(),
                opts(if seed % 2 == 0 { "en-US" } else { "pt-BR" }),
                RandomSource::seeded(u64::from(seed)),
                Some(Box::new(Fake)),
            )
            .unwrap();
            // SH1 and duplicated positions (a shadowed `<c>`): the reopened file shows the element the TS model would
            // (C5b-5); not compared
            let sh1 = has_sh1(&s) || s.book.store.sheets.iter().any(|sh| sh.file_order.as_ref().is_some_and(|fo| !fo.shadowed.is_empty()));
            for i in 0..40 {
                let what = step(&mut s, &mut r);
                steps += 1;
                invariants(&s, &format!("{k} seed {seed} step {i} {what}"));
                // SH1 workbooks (a `<c r>` on another `<row r>`) place such cells by element on reopen: not compared
                if i % 13 == 12 && !sh1 {
                    // save and reopen: the store of a fresh open equals the session's
                    let saved = s.save().unwrap_or_else(|e| panic!("{k} seed {seed} step {i}: save {e}"));
                    let mut b = Book::open(&saved, ReadLimits::default()).unwrap_or_else(|e| panic!("{k}: reopen {e}"));
                    b.load_all().unwrap();
                    for vi in 0..s.views.len() {
                        let si = s.views[vi].sheet;
                        let ours = store_lines(&s, vi);
                        let ix = uo_calc::SheetIx(si as u32);
                        let rp = &b.run_props;
                        let props = |id: u32| rp.get(id).map(uo_xlsx::read::facet::run_props_json).unwrap_or_default();
                        let back = uo_calc::facet::store_facet(&b.store, ix, &props);
                        // the facet of a reopened file canonicalises what the session holds as pending (a typed
                        // inline string with its runs, shared-string numbering): compared by cell count and positions
                        let pos = |v: &[String]| -> Vec<String> {
                            v.iter()
                                .map(|l| {
                                    l.split(",\"").next().unwrap_or("").to_owned()
                                        + &l.split("\"r\":")
                                            .nth(1)
                                            .unwrap_or("")
                                            .chars()
                                            .take_while(char::is_ascii_digit)
                                            .collect::<String>()
                                })
                                .collect()
                        };
                        assert_eq!(pos(&ours), pos(&back), "{k} seed {seed} step {i}: reopened sheet {vi} differs");
                    }
                    roundtrips += 1;
                }
            }
            // undo everything, redo everything: nothing panics, invariants hold
            while s.undo() {}
            invariants(&s, &format!("{k} seed {seed} all undone"));
            while s.redo() {}
            invariants(&s, &format!("{k} seed {seed} all redone"));
            s.save().unwrap();
        }
    }
    eprintln!("hostile: {steps} random steps, {roundtrips} save/reopen round trips, invariants after every step");
    assert!(steps > 1000);
}

fn s_clear() {
    uo_numfmt::clear_caches();
    uo_text::clear_caches();
}

/// One fixed script over a workbook (every family + undo/redo + save), for M1.
fn script(s: &mut Session) {
    let mut r = Rng(7);
    for _ in 0..30 {
        step(s, &mut r);
    }
    while s.undo() {}
    while s.redo() {}
    s.save().unwrap();
}

#[test]
fn m1_session_script_leaves_nothing() {
    // warm-up: the lazily built static tables (catalogue, languages, locale data) are built once per process
    for (_, bytes) in workbooks().into_iter().take(3) {
        let mut s = Session::open_with(bytes, opts("pt-BR"), RandomSource::seeded(1), Some(Box::new(Fake))).unwrap();
        script(&mut s);
    }
    let mut n = 0;
    for (k, bytes) in workbooks() {
        let bytes2 = bytes.clone();
        // warm the workbook's own statics (first run), then the gate
        {
            let mut s = Session::open_with(bytes.clone(), opts("pt-BR"), RandomSource::seeded(1), Some(Box::new(Fake))).unwrap();
            script(&mut s);
        }
        s_clear();
        let r = uo_alloc::check_no_leak(|| {
            let mut s = Session::open_with(bytes2.clone(), opts("pt-BR"), RandomSource::seeded(1), Some(Box::new(Fake))).unwrap();
            script(&mut s);
            s.clear_caches();
            drop(s);
            uo_numfmt::clear_caches();
            uo_text::clear_caches();
        });
        assert!(r.is_ok(), "{k}: {r:?}");
        n += 1;
    }
    eprintln!("M1: {n} workbooks, open + 30 operations + undo/redo all + save + drop: live bytes back exactly");
}

#[test]
fn m2_steady_editing_is_flat() {
    let bytes = wb_bytes("samples/demo.xlsx").or_else(|| wb_bytes("x1/0000")).unwrap();
    let mut s = Session::open_with(bytes, opts("pt-BR"), RandomSource::seeded(3), Some(Box::new(Fake))).unwrap();
    let texts = ["1", "texto", "=A1*2", "=SEQUÊNCIA(5)", ""];
    let series = uo_alloc::soak(1_000, 50, |i| {
        let cell = format!("{}{}", uo_calc::a1::col_to_letters((i % 6) as u32), 1 + i % 20);
        s.select_range(&cell).unwrap();
        s.set_active_cell_text(&u(texts[i % texts.len()]));
        if i % 25 == 0 {
            s.select_range("A1:J10").unwrap();
            s.copy(false);
            s.select_range("L1").unwrap();
            s.paste(None, false);
        }
        if i % 10 == 9 {
            s.undo();
            s.redo();
        }
    });
    eprintln!("M2 series (live bytes every 50 edits): {series:?}");
    // the history fills its 100 entries in the first ~110 iterations: flat after a warm-up of 300
    uo_alloc::assert_flat(&series, 6, 256 * 1024);
}

#[test]
fn m5_history_byte_cap_holds() {
    let bytes = wb_bytes("samples/demo.xlsx").or_else(|| wb_bytes("x1/0000")).unwrap();
    let cap = 64 * 1024;
    let o = SessionOpts {
        history: HistoryLimits { max_steps: 100, max_bytes: cap },
        clock: Clock { now_serial: 45672.5 },
        ..SessionOpts::default()
    };
    let mut s = Session::open_with(bytes, o, RandomSource::seeded(1), None).unwrap();
    for i in 0..60 {
        s.select_range("A1:T60").unwrap();
        s.set_font(FontChange { bold: Some(i % 2 == 0), ..FontChange::default() });
        let n = s.undo_entries().count();
        assert!(s.history_bytes() <= cap || n == 1, "history {} B over the cap with {n} entries", s.history_bytes());
    }
    let n = s.undo_entries().count();
    assert!(n < 60, "the cap dropped the oldest entries ({n} kept)");
    // the steps cap
    let mut s2 = Session::open_with(wb_bytes("x1/0000").unwrap(), opts("pt-BR"), RandomSource::seeded(1), None).unwrap();
    for i in 0..150 {
        s2.select_range("B2").unwrap();
        s2.set_active_cell_text(&u(&i.to_string()));
    }
    assert_eq!(s2.undo_entries().count(), 100);
    eprintln!("M5: byte cap {cap} B kept {n} of 60 entries; the step cap keeps 100 of 150");
}
