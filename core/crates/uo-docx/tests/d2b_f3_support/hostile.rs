//! Hostile F3 commands: every table command with the arguments a careless or malicious caller may
//! pass (NaN, infinities, huge counts and spans, rectangles outside the table, hops past the
//! tables, merged cells, texts with control characters).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::tables::formula::{FormulaEnv, formula_result_text};
use uo_docx::session::ops::tables::table_ops::{AutoFit, AutoFitMode, CellRect};
use uo_docx::session::ops::tables::{
    ApplyTo, DeleteCellsShift, InsertCellsShift, MarginsIn, SelKey, SortRequest, StyleOptions, TableCmd, TableLayoutIn,
    TablePropertiesInput,
};
use uo_docx::session::{CellHop, Command, Session};
use uo_testkit::generate::Rng;

pub fn wild_f(rng: &mut Rng) -> f64 {
    [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        0.0,
        1e300,
        -1e300,
        0.5,
        2_147_483_648.0,
        -2_147_483_649.0,
        9_007_199_254_740_993.0,
        720.0,
        -720.0,
        3.0,
        63.0,
        64.0,
    ][rng.below(16)]
}

fn wild_i(rng: &mut Rng) -> i64 {
    [0, 1, 2, 3, -1, -2, i64::MAX, i64::MIN, 1 << 40, -(1 << 40), 4_294_967_295, 4_294_967_296, 63, 64, 100_000][rng.below(15)]
}

fn small(rng: &mut Rng) -> i64 {
    i64::try_from(rng.below(5)).unwrap()
}

fn rect(rng: &mut Rng) -> CellRect {
    if rng.below(3) == 0 {
        CellRect::new(wild_i(rng), wild_i(rng), wild_i(rng), wild_i(rng))
    } else {
        let (r1, c1) = (small(rng), small(rng));
        CellRect::new(r1, c1, r1 + small(rng), c1 + small(rng))
    }
}

fn hops(rng: &mut Rng) -> Vec<CellHop> {
    (0..1 + rng.below(3))
        .map(|_| CellHop {
            row: [0, 1, 2, u32::MAX, 7][rng.below(5)],
            col: [0, 1, 2, u32::MAX, 9][rng.below(5)],
            block: (rng.below(2) == 0).then(|| [0, 1, u32::MAX][rng.below(3)]),
        })
        .collect()
}

fn layout(rng: &mut Rng) -> TableLayoutIn {
    TableLayoutIn {
        content_width_twips: (rng.below(2) == 0).then(|| wild_f(rng)),
        row_heights: (0..rng.below(6)).map(|_| wild_f(rng)).collect(),
    }
}

fn wild_s(rng: &mut Rng) -> String {
    let long = "x".repeat(300);
    ["", "#", "=SUM(ABOVE)", "tab", "\u{0}", "😀", "a:b", long.as_str(), "lrTb", "tbRl", "center", "FFFFFF", "accent1"][rng.below(13)]
        .to_owned()
}

fn border(rng: &mut Rng) -> Option<uo_docx::model::props::BorderSpec> {
    match rng.below(4) {
        0 => None,
        1 => Some(bs("single", Some(wild_f(rng)), Some(wild_f(rng)), Some("auto"))),
        2 => Some(bs("nil", None, None, None)),
        _ => Some(bs(&wild_s(rng), Some(4.0), Some(0.0), Some(&wild_s(rng)))),
    }
}

/// A hostile command over the family (and the cell contexts it needs).
pub fn hostile_f3(rng: &mut Rng, s: &Session) -> Command {
    let n_tables = u32::try_from(s.document().body.tables.len()).unwrap();
    let t = |c: TableCmd| Command::Table(c);
    match rng.below(46) {
        0..=2 => {
            // into a cell (valid or not)
            let anchor = if n_tables > 0 && rng.below(4) != 0 { u32::try_from(rng.below(n_tables as usize)).unwrap() } else { u32::MAX };
            Command::Context(ContextCmd::EnterCell { anchor, hops: hops(rng), caret: None })
        }
        3 => t(TableCmd::InsertTable {
            // the 1,000,000-cell cut is tested by itself (`a_huge_table_is_bounded`); here the sizes stay small
            rows: uo_jsnum::math::max(-5.0, uo_jsnum::math::min(wild_f(rng), 40.0)),
            cols: uo_jsnum::math::max(-5.0, uo_jsnum::math::min(wild_f(rng), 20.0)),
            autofit: [None, Some(AutoFit::Fixed), Some(AutoFit::Content), Some(AutoFit::Window)][rng.below(4)],
            col_width_twips: (rng.below(2) == 0).then(|| wild_f(rng)),
            layout: layout(rng),
        }),
        4 => t(TableCmd::InsertRowAbove),
        5 => t(TableCmd::InsertRowBelow),
        6 => t(TableCmd::InsertColumnLeft),
        7 => t(TableCmd::InsertColumnRight),
        8 => t(TableCmd::InsertCells {
            shift: [InsertCellsShift::Right, InsertCellsShift::Down, InsertCellsShift::Row, InsertCellsShift::Column][rng.below(4)],
        }),
        9 => t(TableCmd::DeleteRow),
        10 => t(TableCmd::DeleteColumn),
        11 => t(TableCmd::DeleteTable),
        12 => t(TableCmd::DeleteCells {
            shift: [DeleteCellsShift::Left, DeleteCellsShift::Up, DeleteCellsShift::Row, DeleteCellsShift::Column][rng.below(4)],
        }),
        13 | 14 => t(TableCmd::MergeCells),
        15 | 16 => t(TableCmd::SplitCell {
            cols: uo_jsnum::math::max(-5.0, uo_jsnum::math::min(wild_f(rng), 8.0)),
            rows: uo_jsnum::math::max(-5.0, uo_jsnum::math::min(wild_f(rng), 8.0)),
            merge_first: rng.below(2) == 0,
            layout: layout(rng),
        }),
        17 => t(TableCmd::SplitTable),
        18 | 19 => t([TableCmd::SelectCell, TableCmd::SelectRow, TableCmd::SelectColumn, TableCmd::SelectTable][rng.below(4)].clone()),
        20 | 21 => t(TableCmd::SetTableSelection {
            anchor: if n_tables > 0 { u32::try_from(rng.below(n_tables as usize)).unwrap() } else { 0 },
            path: if rng.below(4) == 0 { hops(rng) } else { Vec::new() },
            rect: rect(rng),
            origin: (wild_i(rng), wild_i(rng)),
        }),
        22 => t(TableCmd::ClearTableSelection),
        23 => t(TableCmd::TableSelArrow {
            key: [SelKey::Left, SelKey::Right, SelKey::Up, SelKey::Down][rng.below(4)],
            shift: rng.below(2) == 0,
        }),
        24 => t(TableCmd::TableSelDelete { backspace: rng.below(2) == 0 }),
        25 => t(TableCmd::ClearSelectedCells),
        26 => t(TableCmd::DistributeRows { layout: layout(rng) }),
        27 => t(TableCmd::DistributeColumns),
        28 => t(TableCmd::SetRowHeight { twips: wild_f(rng), exact: rng.below(2) == 0 }),
        29 => t(TableCmd::SetColumnWidth { twips: wild_f(rng) }),
        30 => t(TableCmd::AutoFitTable {
            mode: [AutoFitMode::Content, AutoFitMode::Window, AutoFitMode::Fixed][rng.below(3)],
            layout: layout(rng),
        }),
        31 => t(TableCmd::SetCellAlignment { horizontal: wild_s(rng), vertical: wild_s(rng) }),
        32 => t(if rng.below(2) == 0 { TableCmd::SetTextDirection { dir: wild_s(rng) } } else { TableCmd::CycleTextDirection }),
        33 => t(TableCmd::ToggleRepeatHeaderRows),
        34 => t(TableCmd::SetTableCellMargins {
            margins: MarginsIn {
                top: Some(wild_f(rng)),
                left: (rng.below(2) == 0).then(|| wild_f(rng)),
                bottom: None,
                right: Some(wild_f(rng)),
            },
            spacing_twips: (rng.below(2) == 0).then(|| wild_f(rng)),
            auto_resize: (rng.below(2) == 0).then(|| rng.below(2) == 0),
        }),
        35 => t(TableCmd::Sort(SortRequest {
            keys: (0..rng.below(3))
                .map(|_| (wild_i(rng), [SortType::Text, SortType::Number, SortType::Date][rng.below(3)], rng.below(2) == 0))
                .collect(),
            header: rng.below(2) == 0,
            case_sensitive: rng.below(2) == 0,
            separator: None,
        })),
        36 => t(TableCmd::ConvertTableToText { separator: sep(rng.below(N_SEPS)) }),
        37 => t(TableCmd::ConvertTextToTable {
            separator: (rng.below(2) == 0).then(|| sep(rng.below(N_SEPS))),
            cols: (rng.below(2) == 0).then(|| wild_f(rng)),
            autofit: None,
            col_width_twips: (rng.below(2) == 0).then(|| wild_f(rng)),
            layout: layout(rng),
        }),
        38 => t(TableCmd::SetTableStyle {
            name: (rng.below(5) != 0)
                .then(|| ["Table Grid", "Grid Table 4 - Accent 1", "Plain Table 1", "No Such", "Normal Table"][rng.below(5)].to_owned()),
        }),
        39 => t(TableCmd::SetTableStyleOptions(StyleOptions {
            header_row: Some(rng.below(2) == 0),
            banded_rows: Some(rng.below(2) == 0),
            ..StyleOptions::default()
        })),
        40 => t(TableCmd::SetCellShading {
            color: Some(uo_docx::session::ops::tables::ShadeColor::Hex(wild_s(rng))),
            pattern: None,
            apply_to: [None, Some(ApplyTo::Cell), Some(ApplyTo::Table)][rng.below(3)],
        }),
        41 => t(TableCmd::SetCellBorders { kind: kind(KINDS[rng.below(KINDS.len())]), spec: border(rng) }),
        42 => t(TableCmd::SetTableBorderSides {
            sides: vec![(wild_s(rng), border(rng)), ("top".to_owned(), border(rng))],
            apply_to: if rng.below(2) == 0 { ApplyTo::Cell } else { ApplyTo::Table },
        }),
        43 => t(TableCmd::InsertFormula {
            formula: (rng.below(2) == 0).then(|| wild_s(rng)),
            number_format: (rng.below(2) == 0).then(|| wild_s(rng)),
        }),
        44 => match rng.below(5) {
            0 => t(TableCmd::TabNewRow),
            1 => t(TableCmd::MoveTableRow { dir: i32::try_from(wild_i(rng).clamp(-5, 5)).unwrap() }),
            2 => t(TableCmd::ResizeTableColumn { boundary: wild_i(rng), delta_twips: wild_f(rng) }),
            3 => t(TableCmd::ResizeTableRow { row: wild_i(rng), delta_twips: wild_f(rng), layout: layout(rng) }),
            _ => t(TableCmd::SetTableProperties { input: Box::new(TablePropertiesInput::default()), layout: layout(rng) }),
        },
        _ => {
            let p = TablePropertiesInput {
                column_width: Some(Some(w(wild_f(rng), ["dxa", "pct", "auto"][rng.below(3)]))),
                cell: Some(uo_docx::session::ops::tables::CellTab { width: Some(Some(w(wild_f(rng), "dxa"))), ..Default::default() }),
                ..TablePropertiesInput::default()
            };
            t(TableCmd::SetTableProperties { input: Box::new(p), layout: layout(rng) })
        }
    }
}

/// The pure formulas with hostile texts (no panic, any answer).
pub fn hostile_formula(rng: &mut Rng, t: &uo_docx::model::table::Table) -> Option<String> {
    let texts = [
        "=",
        "=(((((((((((1",
        "=1+",
        "=SUM(",
        "=A1:ZZ999999999",
        "=2^",
        "=-----1",
        "=\u{0}",
        "= 1 2",
        "=ROUND(1;1e300)",
        "=1/0",
        "=MOD(1;0)",
        "=((((((((((((((((((((((((((((1))))))))))))))))))))))))))))",
    ];
    let f: Vec<u16> = texts[rng.below(texts.len())].encode_utf16().collect();
    let instr = uo_docx::session::ops::tables::formula::formula_instr(&f, None);
    formula_result_text(
        &instr,
        &FormulaEnv { table: t, r: i64::try_from(rng.below(5)).unwrap(), c: i64::try_from(rng.below(5)).unwrap(), bookmarks: None },
    )
}
