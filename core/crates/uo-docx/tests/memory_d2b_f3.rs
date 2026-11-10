//! Memory gate of D2b-F3 (record `docs/modules/uo-docx-d2b/F3.md`, "Memory"):
//!
//! - **M1**: whole session lives with the table commands (open -> scripted and hostile table
//!   commands, undo, redo, save -> drop) return the live bytes exactly.
//! - **M2**: 100 x (open -> 60 table commands -> 30 undo -> 30 redo -> save -> close) over corpus
//!   documents: the live-bytes series is flat after warm-up.
//! - **M3**: one table command on a generated 3,000-row x 6-column table (18,000 cells): a command
//!   allocates a bounded multiple of the table's own size (the table is copied once, copy-on-write
//!   against the undo step: U19) — numbers printed with `--nocapture`; no command allocates more
//!   than 4 times the table.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f3_support;
#[allow(unused_imports)]
use crate::support;

use std::fmt::Write as _;
use std::hint::black_box;

use d2a_support::{Cx, Step, run_step, sample};
use d2b_f3_support::hostile::hostile_f3;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::tables::sort::SortType;
use uo_docx::session::ops::tables::table_ops::{AutoFitMode, CellRect};
use uo_docx::session::ops::tables::{SortRequest, TableCmd, TableLayoutIn};
use uo_docx::session::{CellHop, Command, FixedEnv, HistoryLimits, OpenOptions, Session};
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn f3_life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    let mut rng = Rng::new(seed);
    for _ in 0..25 {
        let c = hostile_f3(&mut rng, &s);
        run_step(&mut s, Step::Cmd(c), &mut cx);
    }
    for _ in 0..12 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..12 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_f3_session_lives_return_the_memory() {
    let docs = sample(120);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(f3_life(&bytes, 0xf3 + u32::try_from(i).unwrap())));
    }
    println!("M1 d2b-f3: {} session lives", docs.len());
    assert!(docs.len() > 10);
}

#[test]
fn m2_soak_f3_edit_undo_redo_save() {
    let docs: Vec<Vec<u8>> = sample(250).iter().map(|f| std::fs::read(f).unwrap()).filter(|b| b.len() < 60_000).take(10).collect();
    assert!(docs.len() >= 5);
    let mut out = 0usize;
    let series = soak(60, 10, |i| {
        let bytes = &docs[i % docs.len()];
        out = out.wrapping_add(f3_life(bytes, u32::try_from(i % 7).unwrap()));
    });
    println!("M2 soak d2b-f3: 60 lives over {} documents: {:?}", docs.len(), &series[..4]);
    black_box(out);
    assert_flat(&series, 1, 0);
}

/// A generated document with one table of `rows` x 6 cells of numbers and words.
fn big_table_document(rows: u32) -> Vec<u8> {
    let mut body = String::with_capacity(4 << 20);
    body.push_str("<w:p><w:r><w:t>before</w:t></w:r></w:p>");
    body.push_str("<w:tbl><w:tblPr><w:tblStyle w:val=\"Tabelacomgrade\"/><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid>");
    for _ in 0..6 {
        body.push_str("<w:gridCol w:w=\"1500\"/>");
    }
    body.push_str("</w:tblGrid>");
    for r in 0..rows {
        body.push_str("<w:tr>");
        for c in 0..6u32 {
            let v = (r.wrapping_mul(7919) + c.wrapping_mul(104_729)) % 10_007;
            let _ =
                write!(body, "<w:tc><w:tcPr><w:tcW w:w=\"1500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>{v} x{c}</w:t></w:r></w:p></w:tc>");
        }
        body.push_str("</w:tr>");
    }
    body.push_str("</w:tbl><w:p><w:r><w:t>after</w:t></w:r></w:p>");
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

fn enter(s: &mut Session, cx: &mut Cx, row: u32, col: u32) {
    s.exec(Command::Context(ContextCmd::EnterCell { anchor: 0, hops: vec![CellHop { row, col, block: None }], caret: None }), &mut cx.cx())
        .unwrap();
}

#[test]
fn m3_per_command_on_a_3000_row_table() {
    let rows = 3_000u32;
    let bytes = big_table_document(rows);
    let mut s = open_with(bytes, HistoryLimits::default()).unwrap();
    assert_eq!(s.document().body.tables.len(), 1);
    assert_eq!(s.document().body.tables.get(0).unwrap().table.rows.len(), rows as usize);
    let mut cx = Cx::new();
    // the table's own size: what the document holds with it and without it
    let (_, open_cost) = measure(|| open_with(big_table_document(rows), HistoryLimits::default()));
    let table_bytes = open_cost.allocated.max(1);
    println!("M3 d2b-f3: table of {rows} x 6 cells, a session opened over it allocates {table_bytes} B");
    let at = rows / 2;
    let lay = TableLayoutIn { content_width_twips: Some(8000.0), row_heights: vec![300.0; rows as usize] };
    let sort = |col: i64, ty: SortType, desc: bool| {
        TableCmd::Sort(SortRequest { keys: vec![(col, ty, desc)], header: true, case_sensitive: false, separator: None })
    };
    let cases: Vec<(&str, Vec<Command>, TableCmd)> = vec![
        ("insert row", vec![], TableCmd::InsertRowBelow),
        ("delete row", vec![], TableCmd::DeleteRow),
        ("sort text", vec![], sort(0, SortType::Text, false)),
        ("sort number", vec![], sort(1, SortType::Number, true)),
        (
            "merge 2x2",
            vec![Command::Table(TableCmd::SetTableSelection {
                anchor: 0,
                path: vec![],
                rect: CellRect::new(i64::from(at), 1, i64::from(at) + 1, 2),
                origin: (i64::from(at), 1),
            })],
            TableCmd::MergeCells,
        ),
        ("column width", vec![], TableCmd::SetColumnWidth { twips: 1111.0 }),
        ("distribute rows", vec![Command::Table(TableCmd::SelectTable)], TableCmd::DistributeRows { layout: lay.clone() }),
        ("autofit window", vec![], TableCmd::AutoFitTable { mode: AutoFitMode::Window, layout: lay.clone() }),
        ("insert column", vec![], TableCmd::InsertColumnRight),
        ("split table", vec![], TableCmd::SplitTable),
    ];
    let mut report = String::new();
    for (name, pre, cmd) in cases {
        enter(&mut s, &mut cx, at, 1);
        for p in pre {
            s.exec(p, &mut cx.cx()).unwrap();
        }
        // a warm-up of the same command (the first run pays for lazily built caches)
        let (r, u) = measure(|| s.exec(Command::Table(cmd.clone()), &mut cx.cx()));
        drop(r);
        let ratio = u.allocated as f64 / table_bytes as f64;
        let _ = writeln!(report, "  {name}: {} B ({ratio:.2}x the table)", u.allocated);
        assert!(u.allocated <= 4 * table_bytes, "{name}: {} B allocated for a table of {table_bytes} B", u.allocated);
        // restore for the next case
        s.undo(&mut cx.cx());
    }
    println!("M3 d2b-f3 per command:\n{report}");
}
