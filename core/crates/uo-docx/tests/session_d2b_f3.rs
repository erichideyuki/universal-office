//! D2b-F3 behaviour (record `docs/modules/uo-docx-d2b/F3.md`): one test per command (M1) with the
//! error and no-op paths — a table is inserted at the caret and the cursor goes to its first cell,
//! rows/columns/cells are inserted and deleted, merge and split follow Word's rules, sort, the
//! converters, the formulas, undo and redo give back the state, the lock and the policy answer
//! silently, hostile positions change nothing.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod support;

use d2a_support::Cx;
use uo_docx::model::table::Table;
use uo_docx::session::ops::tables::sort::SortType;
use uo_docx::session::ops::tables::table_ops::{AutoFit, AutoFitMode, BorderKind, CellRect, TextSeparator};
use uo_docx::session::ops::tables::{
    ApplyTo, DeleteCellsShift, InsertCellsShift, SortRequest, TableCmd, TableLayoutIn, TablePropertiesInput,
};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn blank() -> Session {
    Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> ChangeSet {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(cs) => cs,
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn tbl(s: &mut Session, cx: &mut Cx, c: TableCmd) -> ChangeSet {
    run(s, cx, Command::Table(c))
}

fn typed(s: &mut Session, cx: &mut Cx, t: &str) {
    run(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(t) }));
}

fn insert(s: &mut Session, cx: &mut Cx, rows: f64, cols: f64) {
    tbl(s, cx, TableCmd::InsertTable { rows, cols, autofit: None, col_width_twips: None, layout: TableLayoutIn::default() });
}

fn table(s: &Session, i: usize) -> Table {
    (*s.document().body.tables.get(i).unwrap().table).clone()
}

fn cell_text(t: &Table, r: usize, c: usize) -> String {
    t.rows
        .get(r)
        .unwrap()
        .cells
        .get(c)
        .unwrap()
        .content
        .paras
        .iter()
        .map(|p| p.runs.iter().map(|x| x.text.to_string_lossy()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Types `text` into the cell (row, grid col) of body table `anchor`.
fn fill(s: &mut Session, cx: &mut Cx, anchor: u32, r: u32, c: u32, text: &str) {
    run(
        s,
        cx,
        Command::Context(uo_docx::session::ops::context::ContextCmd::EnterCell {
            anchor,
            hops: vec![uo_docx::session::CellHop { row: r, col: c, block: None }],
            caret: None,
        }),
    );
    if !text.is_empty() {
        typed(s, cx, text);
    }
}

fn grid(s: &mut Session, cx: &mut Cx, texts: &[&[&str]]) {
    insert(s, cx, texts.len() as f64, texts.first().map_or(1, |r| r.len()) as f64);
    for (r, row) in texts.iter().enumerate() {
        for (c, t) in row.iter().enumerate() {
            if !t.is_empty() {
                fill(s, cx, 0, r as u32, c as u32, t);
            }
        }
    }
}

#[test]
fn insert_table_goes_to_the_first_cell_with_one_step() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "ab");
    let undo0 = s.history().undo_len();
    let cs = tbl(
        &mut s,
        &mut cx,
        TableCmd::InsertTable { rows: 2.0, cols: 3.0, autofit: None, col_width_twips: None, layout: TableLayoutIn::default() },
    );
    assert!(cs.dirty);
    assert_eq!(s.history().undo_len(), undo0 + 1);
    let t = table(&s, 0);
    assert_eq!((t.rows.len(), t.rows[0].cells.len()), (2, 3));
    assert_eq!(t.grid_cols_twips.len(), 3);
    assert!(matches!(s.context(), uo_docx::session::ops::context::EditContext::Cell { .. }));
    // the table is anchored before the paragraph the caret was in (at its end: the tail paragraph)
    assert_eq!(s.document().body.tables.get(0).unwrap().before_para, 1);
    // undo gives back the document without the table
    s.undo(&mut cx.cx());
    assert_eq!(s.document().body.tables.len(), 0);
    s.redo(&mut cx.cx());
    assert_eq!(s.document().body.tables.len(), 1);
}

#[test]
fn insert_table_refuses_when_locked_and_clamps_the_size() {
    let mut cx = Cx::new();
    let mut s = blank();
    run(&mut s, &mut cx, Command::Settings(uo_docx::session::ops::settings::SettingsCmd::SetProtected { on: true }));
    insert(&mut s, &mut cx, 2.0, 2.0);
    assert_eq!(s.document().body.tables.len(), 0);
    let mut s = blank();
    insert(&mut s, &mut cx, f64::NAN, 1000.0);
    let t = table(&s, 0);
    assert_eq!((t.rows.len(), t.rows[0].cells.len()), (1, 63));
}

#[test]
fn rows_columns_and_cells() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a", "b"], &["c", "d"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::InsertRowBelow);
    let t = table(&s, 0);
    assert_eq!(t.rows.len(), 3);
    assert_eq!(cell_text(&t, 0, 0), "a");
    assert_eq!(cell_text(&t, 1, 0), "");
    tbl(&mut s, &mut cx, TableCmd::InsertColumnRight);
    // the new row is selected, so as many columns as it has are inserted (Word)
    let t = table(&s, 0);
    assert_eq!(t.rows[0].cells.len(), 4);
    assert_eq!(t.grid_cols_twips.len(), 4);
    tbl(&mut s, &mut cx, TableCmd::DeleteColumn);
    assert_eq!(table(&s, 0).rows[0].cells.len(), 2);
    tbl(&mut s, &mut cx, TableCmd::DeleteRow);
    assert_eq!(table(&s, 0).rows.len(), 2);
    // delete cells shifting left leaves gridAfter
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::DeleteCells { shift: DeleteCellsShift::Left });
    let t = table(&s, 0);
    assert_eq!(t.rows[0].cells.len(), 1);
    assert!(t.rows[0].tr_pr.is_some());
    // undo three times returns the first state of the table
    s.undo(&mut cx.cx());
    assert_eq!(table(&s, 0).rows[0].cells.len(), 2);
    tbl(&mut s, &mut cx, TableCmd::InsertCells { shift: InsertCellsShift::Down });
    assert_eq!(table(&s, 0).rows.len(), 3);
    tbl(&mut s, &mut cx, TableCmd::DeleteTable);
    assert_eq!(s.document().body.tables.len(), 0);
}

#[test]
fn merge_and_split() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a", "b", "c"], &["d", "e", "f"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SetTableSelection { anchor: 0, path: vec![], rect: CellRect::new(0, 0, 0, 1), origin: (0, 0) });
    tbl(&mut s, &mut cx, TableCmd::MergeCells);
    let t = table(&s, 0);
    assert_eq!(t.rows[0].cells.len(), 2);
    assert_eq!(cell_text(&t, 0, 0), "a\nb");
    assert!(table_ops_span(&t, 0, 0) == 2);
    // split it back in two
    tbl(&mut s, &mut cx, TableCmd::SplitCell { cols: 2.0, rows: 1.0, merge_first: false, layout: TableLayoutIn::default() });
    let t = table(&s, 0);
    assert_eq!(t.rows[0].cells.len(), 3);
    // splitting a cell into two rows makes vertical merges of its neighbours
    fill(&mut s, &mut cx, 0, 1, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SplitCell { cols: 1.0, rows: 2.0, merge_first: true, layout: TableLayoutIn::default() });
    let t = table(&s, 0);
    assert_eq!(t.rows.len(), 3);
    // merging nothing (one cell) changes nothing and takes no step
    let n = s.history().undo_len();
    fill(&mut s, &mut cx, 0, 0, 0, "");
    assert_eq!(s.history().undo_len(), n, "enterCell");
    tbl(&mut s, &mut cx, TableCmd::SetTableSelection { anchor: 0, path: vec![], rect: CellRect::new(0, 0, 0, 0), origin: (0, 0) });
    assert_eq!(s.history().undo_len(), n, "select");
    tbl(&mut s, &mut cx, TableCmd::MergeCells);
    assert_eq!(s.history().undo_len(), n, "merge");
}

fn table_ops_span(t: &Table, r: usize, c: usize) -> i64 {
    uo_docx::session::ops::tables::table_ops::span_of(t.rows.get(r).unwrap().cells.get(c).unwrap())
}

#[test]
fn split_table_between_the_rows() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a"], &["b"], &["c"]]);
    fill(&mut s, &mut cx, 0, 1, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SplitTable);
    assert_eq!(s.document().body.tables.len(), 2);
    assert_eq!(table(&s, 0).rows.len(), 1);
    assert_eq!(table(&s, 1).rows.len(), 2);
    // a paragraph between them
    let a = s.document().body.tables.get(0).unwrap().before_para;
    let b = s.document().body.tables.get(1).unwrap().before_para;
    assert_eq!(b, a + 1);
    s.undo(&mut cx.cx());
    assert_eq!(s.document().body.tables.len(), 1);
}

#[test]
fn sort_rows_by_the_columns() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["Nome", "N"], &["b", "10"], &["a", "9"], &["c", "2"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(
        &mut s,
        &mut cx,
        TableCmd::Sort(SortRequest { keys: vec![(1, SortType::Number, false)], header: true, case_sensitive: false, separator: None }),
    );
    let t = table(&s, 0);
    assert_eq!(
        (cell_text(&t, 0, 0), cell_text(&t, 1, 0), cell_text(&t, 2, 0), cell_text(&t, 3, 0)),
        ("Nome".into(), "c".into(), "a".into(), "b".into())
    );
    tbl(
        &mut s,
        &mut cx,
        TableCmd::Sort(SortRequest { keys: vec![(0, SortType::Text, true)], header: true, case_sensitive: false, separator: None }),
    );
    let t = table(&s, 0);
    assert_eq!((cell_text(&t, 1, 0), cell_text(&t, 2, 0), cell_text(&t, 3, 0)), ("c".into(), "b".into(), "a".into()));
}

#[test]
fn sort_refuses_merged_cells_with_an_event() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a", "b"], &["c", "d"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SetTableSelection { anchor: 0, path: vec![], rect: CellRect::new(0, 0, 0, 1), origin: (0, 0) });
    tbl(&mut s, &mut cx, TableCmd::MergeCells);
    fill(&mut s, &mut cx, 0, 1, 0, "");
    let n = s.history().undo_len();
    let cs = tbl(&mut s, &mut cx, TableCmd::Sort(SortRequest { keys: vec![(0, SortType::Text, false)], ..SortRequest::default() }));
    assert!(cs.events.iter().any(|e| matches!(e, uo_docx::session::Event::SortError { .. })));
    assert_eq!(s.history().undo_len(), n);
}

#[test]
fn sort_paragraphs() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "b");
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    typed(&mut s, &mut cx, "a");
    run(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(1, 1) }));
    tbl(&mut s, &mut cx, TableCmd::Sort(SortRequest { keys: vec![(0, SortType::Text, false)], ..SortRequest::default() }));
    let text: Vec<String> = s.document().body.paras.iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect();
    assert_eq!(text, ["a", "b"]);
}

#[test]
fn convert_between_text_and_table() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "a\tb");
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    typed(&mut s, &mut cx, "c\td");
    run(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(1, 3) }));
    tbl(
        &mut s,
        &mut cx,
        TableCmd::ConvertTextToTable {
            separator: None,
            cols: None,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        },
    );
    let t = table(&s, 0);
    assert_eq!((t.rows.len(), t.rows[0].cells.len()), (2, 2));
    assert_eq!(cell_text(&t, 1, 1), "d");
    tbl(&mut s, &mut cx, TableCmd::SelectTable);
    tbl(&mut s, &mut cx, TableCmd::ConvertTableToText { separator: TextSeparator::Tab });
    assert_eq!(s.document().body.tables.len(), 0);
    let text: Vec<String> = s.document().body.paras.iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect();
    assert_eq!(&text[..2], ["a\tb", "c\td"]);
}

#[test]
fn formulas_sum_above_and_left() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["10", "20"], &["5,5", ""], &["", ""]]);
    fill(&mut s, &mut cx, 0, 2, 0, "");
    assert_eq!(s.suggest_formula(), "=SUM(ABOVE)");
    assert_eq!(s.preview_formula("=SUM(ABOVE)", None).as_deref(), Some("15,5"));
    assert_eq!(s.preview_formula("=2^10", None).as_deref(), Some("1024"));
    assert_eq!(s.preview_formula("=1/0", None).as_deref(), Some("!Divisão por zero"));
    tbl(&mut s, &mut cx, TableCmd::InsertFormula { formula: None, number_format: None });
    let t = table(&s, 0);
    let p = t.rows[2].cells[0].content.paras.first().unwrap();
    assert_eq!(p.runs.iter().map(|r| r.text.to_string_lossy()).collect::<String>(), "15,5");
    assert!(p.runs.iter().any(|r| r.ext().and_then(|e| e.field.as_deref()).is_some_and(|f| f.contains("SUM(ABOVE)"))));
}

#[test]
fn sizes_alignment_borders_and_properties() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a", "b"], &["c", "d"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SetRowHeight { twips: 500.0, exact: false });
    assert!(table(&s, 0).rows[0].tr_pr.as_deref().and_then(|p| p.tr_height.as_ref()).is_some());
    tbl(&mut s, &mut cx, TableCmd::SetColumnWidth { twips: 3000.0 });
    assert_eq!(table(&s, 0).grid_cols_twips[0].to_number(), 3000.0);
    tbl(&mut s, &mut cx, TableCmd::AutoFitTable { mode: AutoFitMode::Fixed, layout: TableLayoutIn::default() });
    assert_eq!(table(&s, 0).tbl_pr.as_deref().and_then(|p| p.tbl_layout.as_deref()), Some("fixed"));
    tbl(&mut s, &mut cx, TableCmd::SetCellAlignment { horizontal: "center".into(), vertical: "center".into() });
    let t = table(&s, 0);
    assert_eq!(t.rows[0].cells[0].tc_pr.as_deref().and_then(|p| p.v_align.as_deref()), Some("center"));
    assert_eq!(t.rows[0].cells[0].content.paras.get(0).unwrap().props.alignment.as_deref(), Some("center"));
    tbl(&mut s, &mut cx, TableCmd::SetCellBorders { kind: BorderKind::All, spec: None });
    tbl(
        &mut s,
        &mut cx,
        TableCmd::SetTableProperties { input: Box::new(TablePropertiesInput::default()), layout: TableLayoutIn::default() },
    );
    tbl(
        &mut s,
        &mut cx,
        TableCmd::SetCellShading {
            color: Some(uo_docx::session::ops::tables::ShadeColor::Hex("ff0000".into())),
            pattern: None,
            apply_to: Some(ApplyTo::Cell),
        },
    );
    assert_eq!(table(&s, 0).rows[0].cells[0].shading_hex.as_deref(), Some("FF0000"));
    tbl(&mut s, &mut cx, TableCmd::SetTableStyle { name: Some("Grid Table 4 - Accent 1".into()) });
    assert!(table(&s, 0).tbl_pr.as_deref().and_then(|p| p.tbl_style.as_deref()).is_some());
    let _ = AutoFit::Fixed;
}

#[test]
fn selection_and_clear_cells() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a", "b"], &["c", "d"]]);
    fill(&mut s, &mut cx, 0, 0, 0, "");
    tbl(&mut s, &mut cx, TableCmd::SelectRow);
    assert!(s.table_selection().is_some());
    tbl(&mut s, &mut cx, TableCmd::ClearSelectedCells);
    let t = table(&s, 0);
    assert_eq!((cell_text(&t, 0, 0), cell_text(&t, 0, 1), cell_text(&t, 1, 0)), (String::new(), String::new(), "c".into()));
    tbl(&mut s, &mut cx, TableCmd::ClearTableSelection);
    assert!(s.table_selection().is_none());
    // typing over selected cells clears them and types in the first
    tbl(&mut s, &mut cx, TableCmd::SelectColumn);
    typed(&mut s, &mut cx, "z");
    let t = table(&s, 0);
    assert_eq!((cell_text(&t, 0, 0), cell_text(&t, 1, 0)), ("z".into(), String::new()));
}

#[test]
fn no_table_no_effect() {
    let mut cx = Cx::new();
    let mut s = blank();
    for c in [TableCmd::InsertRowAbove, TableCmd::DeleteTable, TableCmd::MergeCells, TableCmd::SelectTable, TableCmd::TabNewRow] {
        let n = s.history().undo_len();
        tbl(&mut s, &mut cx, c);
        assert_eq!(s.history().undo_len(), n);
    }
}

#[test]
fn repeat_header_rows_toggle() {
    let mut cx = Cx::new();
    let mut s = blank();
    grid(&mut s, &mut cx, &[&["a"], &["b"], &["c"]]);
    fill(&mut s, &mut cx, 0, 1, 0, "");
    tbl(&mut s, &mut cx, TableCmd::ToggleRepeatHeaderRows);
    let t = table(&s, 0);
    assert_eq!(t.rows[0].tr_pr.as_deref().and_then(|p| p.tbl_header), Some(true));
    assert_eq!(t.rows[1].tr_pr.as_deref().and_then(|p| p.tbl_header), Some(true));
    assert!(t.rows[2].tr_pr.is_none());
    // without a selection of cells the toggle can only turn the header on (a TS quirk, F3.md)
    tbl(&mut s, &mut cx, TableCmd::ToggleRepeatHeaderRows);
    assert_eq!(table(&s, 0).rows[0].tr_pr.as_deref().and_then(|p| p.tbl_header), Some(true));
    // with the rows selected it toggles
    tbl(&mut s, &mut cx, TableCmd::SelectRow);
    tbl(&mut s, &mut cx, TableCmd::ToggleRepeatHeaderRows);
    assert!(table(&s, 0).rows[0].tr_pr.is_none());
}
