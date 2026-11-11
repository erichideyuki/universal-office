//! The op codes of `js/capture-d2b-f3-session.mjs` decoded into session commands (the argument
//! tables by index, the same as the harness).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use uo_docx::model::props::{BorderSpec, WidthSpec};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::tables::sort::SortType;
use uo_docx::session::ops::tables::table_ops::{self as t_ops, AutoFit, AutoFitMode};
use uo_docx::session::ops::tables::{
    ApplyTo, CellTab, DeleteCellsShift, InsertCellsShift, MarginsIn, RowTab, SelKey, ShadeColor, ShadePattern, SortRequest, StyleOptions,
    TableCmd, TableLayoutIn, TablePropertiesInput, TableTab,
};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, ChangeSet, Command, DocPosition, Outcome, Session};
use uo_docx::text::Utf16;
use uo_jsnum::parse::to_number;

fn units(h: &str) -> Vec<u16> {
    h.split('.').filter(|x| !x.is_empty()).map(|x| u16::from_str_radix(x, 16).unwrap()).collect()
}

/// `lay` of the capture: `<width>|<h0>,<h1>,…`.
pub fn layout(lay: Option<&str>) -> TableLayoutIn {
    let Some(lay) = lay else { return TableLayoutIn::default() };
    let (w, hs) = lay.split_once('|').unwrap();
    TableLayoutIn { content_width_twips: Some(to_number(w)), row_heights: hs.split(',').filter(|x| !x.is_empty()).map(to_number).collect() }
}

fn wspec(w_: f64, ty: &str) -> WidthSpec {
    w(w_, ty)
}

const SPLITS: [(f64, f64, bool); 6] =
    [(2.0, 1.0, true), (1.0, 2.0, true), (3.0, 1.0, false), (2.0, 2.0, true), (1.0, 1.0, true), (2.0, 1.0, false)];
const ROWH: [(f64, bool); 3] = [(0.0, false), (400.0, false), (600.0, true)];
const COLW: [f64; 3] = [500.0, 1440.0, 3000.0];
const AH: [&str; 4] = ["left", "center", "right", "both"];
const AV: [&str; 3] = ["top", "center", "bottom"];
const DIR: [&str; 3] = ["lrTb", "tbRl", "btLr"];
const STYLES: [Option<&str>; 6] = [
    Some("Table Grid"),
    Some("Grid Table 4 - Accent 1"),
    Some("Plain Table 2"),
    Some("List Table 3 - Accent 2"),
    Some("No Such Style"),
    None,
];

fn margins(i: usize) -> (MarginsIn, Option<f64>, Option<bool>) {
    match i {
        0 => (MarginsIn { top: Some(50.0), left: Some(70.0), ..MarginsIn::default() }, None, None),
        1 => (MarginsIn::default(), Some(40.0), Some(true)),
        2 => (MarginsIn { left: Some(100.0), right: Some(100.0), top: Some(0.0), bottom: Some(20.0) }, Some(0.0), Some(false)),
        _ => (MarginsIn::default(), None, Some(true)),
    }
}

fn sort_req(i: usize) -> SortRequest {
    let k = |c: i64, t: SortType, d: bool| (c, t, d);
    match i {
        0 => SortRequest { keys: vec![k(0, SortType::Text, false)], ..SortRequest::default() },
        1 => SortRequest { keys: vec![k(1, SortType::Number, true)], header: true, ..SortRequest::default() },
        2 => SortRequest {
            keys: vec![k(0, SortType::Date, false), k(1, SortType::Text, true)],
            case_sensitive: true,
            ..SortRequest::default()
        },
        _ => SortRequest { keys: vec![k(0, SortType::Number, false)], header: true, case_sensitive: false, separator: None },
    }
}

fn separator(i: usize) -> t_ops::TextSeparator {
    match i {
        0 => t_ops::TextSeparator::Tab,
        1 => t_ops::TextSeparator::Paragraph,
        2 => t_ops::TextSeparator::Semicolon,
        3 => t_ops::TextSeparator::Comma,
        _ => t_ops::TextSeparator::Char(vec![u16::from(b'|')]),
    }
}

fn style_options(i: usize) -> StyleOptions {
    match i {
        0 => StyleOptions { header_row: Some(false), ..StyleOptions::default() },
        1 => StyleOptions { total_row: Some(true), banded_rows: Some(false), ..StyleOptions::default() },
        2 => StyleOptions { first_column: Some(false), last_column: Some(true), banded_columns: Some(true), ..StyleOptions::default() },
        _ => StyleOptions { header_row: Some(true), banded_columns: Some(false), ..StyleOptions::default() },
    }
}

fn shade(i: usize) -> TableCmd {
    let theme = |name: &str, tint: Option<&str>, shade: Option<&str>| ShadeColor::Theme {
        theme_color: name.into(),
        tint: tint.map(Into::into),
        shade: shade.map(Into::into),
    };
    let pat = |val: &str, color: Option<&str>| ShadePattern { val: val.into(), color: color.map(Into::into) };
    match i {
        0 => TableCmd::SetCellShading { color: Some(ShadeColor::Hex("FF0000".into())), pattern: None, apply_to: None },
        1 => TableCmd::SetCellShading {
            color: Some(ShadeColor::Hex("#00ff00".into())),
            pattern: Some(pat("pct25", Some("0000FF"))),
            apply_to: None,
        },
        2 => TableCmd::SetCellShading { color: Some(theme("accent1", Some("33"), None)), pattern: None, apply_to: None },
        3 => TableCmd::SetCellShading { color: Some(theme("accent2", None, Some("BF"))), pattern: None, apply_to: Some(ApplyTo::Table) },
        4 => TableCmd::SetCellShading { color: None, pattern: None, apply_to: None },
        5 => TableCmd::SetCellShading { color: None, pattern: Some(pat("horzStripe", None)), apply_to: Some(ApplyTo::Cell) },
        _ => TableCmd::SetCellShading {
            color: Some(theme("text2", None, None)),
            pattern: Some(pat("clear", None)),
            apply_to: Some(ApplyTo::Cell),
        },
    }
}

fn border_sides(i: usize) -> TableCmd {
    let b = |val: &str, sz: f64, c: &str| Some(bs(val, Some(sz), Some(0.0), Some(c)));
    match i {
        0 => TableCmd::SetTableBorderSides {
            sides: vec![("top".into(), b("single", 8.0, "FF0000")), ("bottom".into(), None)],
            apply_to: ApplyTo::Cell,
        },
        1 => TableCmd::SetTableBorderSides {
            sides: vec![("left".into(), b("double", 4.0, "auto")), ("insideH".into(), b("single", 4.0, "auto"))],
            apply_to: ApplyTo::Table,
        },
        2 => TableCmd::SetTableBorderSides {
            sides: ["top", "left", "bottom", "right", "insideH", "insideV"].iter().map(|s| ((*s).to_owned(), None)).collect(),
            apply_to: ApplyTo::Table,
        },
        _ => TableCmd::SetTableBorderSides {
            sides: vec![("right".into(), b("dashed", 12.0, "00FF00")), ("insideV".into(), None)],
            apply_to: ApplyTo::Cell,
        },
    }
}

/// `PEN[i]` resolved: the default pen of the element is `single`, 4, 0, `auto`.
fn pen(i: usize) -> Option<BorderSpec> {
    match i {
        0 => Some(bs("single", Some(4.0), Some(0.0), Some("auto"))),
        1 => None,
        _ => Some(bs("dashed", Some(8.0), Some(0.0), Some("0000FF"))),
    }
}

fn tprops(i: usize) -> TablePropertiesInput {
    let mut p = TablePropertiesInput::default();
    match i {
        0 => p.table = Some(TableTab { width: Some(Some(wspec(5000.0, "pct"))), align: Some("center".into()), ..TableTab::default() }),
        1 => p.table = Some(TableTab { width: Some(None), indent_twips: Some(360.0), ..TableTab::default() }),
        2 => {
            p.table = Some(TableTab {
                wrap: Some("around".into()),
                overlap: Some(false),
                title: Some("T".into()),
                description: Some("D".into()),
                ..TableTab::default()
            });
        }
        3 => {
            let mut pos = uo_docx::model::props::TblpPr::default();
            uo_docx::word::obj::set(&mut pos, "leftFromText", |x| x.left_from_text = Some(n(100.0)));
            uo_docx::word::obj::set(&mut pos, "vertAnchor", |x| x.vert_anchor = Some("page".into()));
            uo_docx::word::obj::set(&mut pos, "tblpX", |x| x.tblp_x = Some(n(500.0)));
            uo_docx::word::obj::set(&mut pos, "tblpY", |x| x.tblp_y = Some(n(700.0)));
            p.table = Some(TableTab { wrap: Some("none".into()), position: Some(pos), ..TableTab::default() });
        }
        4 => p.row = Some(RowTab { height: Some(Some((500.0, "exact".into()))), allow_break: Some(false), header: Some(true) }),
        5 => p.row = Some(RowTab { height: Some(None), allow_break: Some(true), header: Some(false) }),
        6 => p.column_width = Some(Some(wspec(25.0, "pct"))),
        7 => p.column_width = Some(Some(wspec(2000.0, "dxa"))),
        8 => p.column_width = Some(None),
        9 => {
            p.cell = Some(CellTab {
                width: Some(Some(wspec(1500.0, "dxa"))),
                v_align: Some("center".into()),
                margins: Some(Some(MarginsIn { top: Some(10.0), left: Some(20.0), ..MarginsIn::default() })),
                wrap: Some(false),
                fit_text: Some(true),
            });
        }
        10 => {
            p.cell = Some(CellTab {
                width: Some(Some(wspec(3000.0, "pct"))),
                v_align: Some("top".into()),
                margins: Some(None),
                wrap: Some(true),
                fit_text: Some(false),
            });
        }
        _ => {
            p.table = Some(TableTab {
                align: Some("right".into()),
                indent_twips: Some(100.0),
                width: Some(Some(wspec(6000.0, "dxa"))),
                overlap: Some(true),
                ..TableTab::default()
            });
        }
    }
    p
}

const FORMULAS: [(Option<&str>, Option<&str>); 9] = [
    (None, None),
    (Some("=SUM(LEFT)"), None),
    (Some("=SUM(ABOVE)"), Some("0,00")),
    (Some("=2^3^2"), None),
    (Some("=ROUND(10/3;2)"), Some("#.##0,00")),
    (Some("=1+"), None),
    (Some("SUM"), None),
    (Some("=A1+B1"), Some("0%")),
    (Some("=10^-2"), None),
];
const SELKEYS: [(&str, bool); 9] = [
    ("ArrowLeft", false),
    ("ArrowRight", true),
    ("ArrowUp", false),
    ("ArrowDown", true),
    ("ArrowRight", false),
    ("ArrowDown", false),
    ("Delete", false),
    ("Backspace", false),
    ("Escape", false),
];

fn x2t(i: usize, layout: TableLayoutIn) -> TableCmd {
    let mk = |sep: Option<t_ops::TextSeparator>, cols: Option<f64>, af: Option<AutoFit>, cw: Option<f64>| TableCmd::ConvertTextToTable {
        separator: sep,
        cols,
        autofit: af,
        col_width_twips: cw,
        layout: layout.clone(),
    };
    match i {
        0 => mk(Some(t_ops::TextSeparator::Tab), None, None, None),
        1 => mk(None, None, None, None),
        2 => mk(Some(t_ops::TextSeparator::Paragraph), Some(2.0), None, None),
        3 => mk(Some(t_ops::TextSeparator::Comma), None, Some(AutoFit::Content), None),
        _ => mk(Some(t_ops::TextSeparator::Semicolon), None, Some(AutoFit::Window), Some(1200.0)),
    }
}

/// The commands of one op code (some are two: selecting then acting).
pub fn commands(s: &Session, code: &str, lay: Option<&str>) -> Vec<Command> {
    let f: Vec<&str> = code.split(':').collect();
    let i = |k: usize| f[k].parse::<usize>().unwrap();
    let pos = |k: usize| DocPosition::new(f[k].parse().unwrap(), f[k + 1].parse().unwrap());
    let layout = layout(lay);
    let t = |c: TableCmd| vec![Command::Table(c)];
    match f[0] {
        "ty" => vec![Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) })],
        "en" => vec![Command::Text(TextCmd::Enter)],
        "bs" => vec![Command::Text(TextCmd::Backspace)],
        "de" => vec![Command::Text(TextCmd::Delete)],
        "se" => vec![Command::Text(TextCmd::SetSelection { anchor: pos(1), focus: pos(3) })],
        "tk" => vec![Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" })],
        "ec" => {
            // `enterCellAt(anchor, [], r, c)`: the cell at the grid column, else the last of the row
            let anchor = f[1].parse::<u32>().unwrap();
            let Some(tb) = s.document().body.tables.get(anchor as usize) else { return Vec::new() };
            let table = &tb.table;
            let (r, c) = (to_number(f[2]) as i64, to_number(f[3]) as i64);
            if table.rows.is_empty() {
                return Vec::new();
            }
            let m = t_ops::map_table(table);
            let ri = r.clamp(0, table.rows.len() as i64 - 1);
            let Some(g) = t_ops::cell_at(&m, ri, c).or_else(|| m.row(ri).last().copied()) else { return Vec::new() };
            vec![Command::Context(ContextCmd::EnterCell {
                anchor,
                hops: vec![CellHop { row: g.ri as u32, col: g.col as u32, block: None }],
                caret: None,
            })]
        }
        "it" => t(TableCmd::InsertTable {
            rows: to_number(f[1]),
            cols: to_number(f[2]),
            autofit: [None, Some(AutoFit::Content), Some(AutoFit::Window)][i(3)],
            col_width_twips: (to_number(f[4]) != 0.0).then(|| to_number(f[4])),
            layout,
        }),
        "ra" => t(TableCmd::InsertRowAbove),
        "rb" => t(TableCmd::InsertRowBelow),
        "cl" => t(TableCmd::InsertColumnLeft),
        "cr" => t(TableCmd::InsertColumnRight),
        "ic" => t(TableCmd::InsertCells {
            shift: [InsertCellsShift::Right, InsertCellsShift::Down, InsertCellsShift::Row, InsertCellsShift::Column][i(1)],
        }),
        "dr" => t(TableCmd::DeleteRow),
        "dc" => t(TableCmd::DeleteColumn),
        "dt" => t(TableCmd::DeleteTable),
        "dl" => t(TableCmd::DeleteCells {
            shift: [DeleteCellsShift::Left, DeleteCellsShift::Up, DeleteCellsShift::Row, DeleteCellsShift::Column][i(1)],
        }),
        "mc" => t(TableCmd::MergeCells),
        "sc" => {
            let (cols, rows, merge_first) = SPLITS[i(1)];
            t(TableCmd::SplitCell { cols, rows, merge_first, layout })
        }
        "st" => t(TableCmd::SplitTable),
        "sx" => t([TableCmd::SelectCell, TableCmd::SelectRow, TableCmd::SelectColumn, TableCmd::SelectTable][i(1)].clone()),
        "dsr" => t(TableCmd::DistributeRows { layout }),
        "dsc" => t(TableCmd::DistributeColumns),
        "rh" => t(TableCmd::SetRowHeight { twips: ROWH[i(1)].0, exact: ROWH[i(1)].1 }),
        "cw" => t(TableCmd::SetColumnWidth { twips: COLW[i(1)] }),
        "af" => t(TableCmd::AutoFitTable { mode: [AutoFitMode::Content, AutoFitMode::Window, AutoFitMode::Fixed][i(1)], layout }),
        "ca" => t(TableCmd::SetCellAlignment { horizontal: AH[i(1)].into(), vertical: AV[i(2)].into() }),
        "td" => t(TableCmd::SetTextDirection { dir: DIR[i(1)].into() }),
        "ctd" => t(TableCmd::CycleTextDirection),
        "rp" => t(TableCmd::ToggleRepeatHeaderRows),
        "mg" => {
            let (m, sp, ar) = margins(i(1));
            t(TableCmd::SetTableCellMargins { margins: m, spacing_twips: sp, auto_resize: ar })
        }
        "so" => t(TableCmd::Sort(sort_req(i(1)))),
        "t2t" => t(TableCmd::ConvertTableToText { separator: separator(i(1)) }),
        "x2t" => t(x2t(i(1), layout)),
        "ts" => t(TableCmd::SetTableStyle { name: STYLES[i(1)].map(Into::into) }),
        "tso" => t(TableCmd::SetTableStyleOptions(style_options(i(1)))),
        "sh" => t(shade(i(1))),
        "bd" => t(border_sides(i(1))),
        "bk" => t(TableCmd::SetCellBorders { kind: kind(KINDS[i(1)]), spec: pen(i(2)) }),
        "tp" => t(TableCmd::SetTableProperties { input: Box::new(tprops(i(1))), layout }),
        "fm" => t(TableCmd::InsertFormula { formula: FORMULAS[i(1)].0.map(Into::into), number_format: FORMULAS[i(1)].1.map(Into::into) }),
        "tn" => t(TableCmd::TabNewRow),
        "mv" => t(TableCmd::MoveTableRow { dir: to_number(f[1]) as i32 }),
        "rc" => t(TableCmd::ResizeTableColumn { boundary: to_number(f[1]) as i64, delta_twips: to_number(f[2]) }),
        "rr" => t(TableCmd::ResizeTableRow { row: to_number(f[1]) as i64, delta_twips: to_number(f[2]), layout }),
        "cs" => t(TableCmd::ClearSelectedCells),
        "sk" => {
            // `if (el.tableSel) el.tableSelKey(key)`
            if s.table_selection().is_none() {
                return Vec::new();
            }
            let (k, shift) = SELKEYS[i(1)];
            t(match k {
                "ArrowLeft" => TableCmd::TableSelArrow { key: SelKey::Left, shift },
                "ArrowRight" => TableCmd::TableSelArrow { key: SelKey::Right, shift },
                "ArrowUp" => TableCmd::TableSelArrow { key: SelKey::Up, shift },
                "ArrowDown" => TableCmd::TableSelArrow { key: SelKey::Down, shift },
                "Delete" => TableCmd::TableSelDelete { backspace: false },
                "Backspace" => TableCmd::TableSelDelete { backspace: true },
                _ => TableCmd::ClearTableSelection,
            })
        }
        other => panic!("op {other}"),
    }
}

/// Runs one op code; `un`/`re` are the session's undo/redo.
pub fn step(s: &mut Session, code: &str, lay: Option<&str>, cx: &mut crate::d2a_support::Cx) -> ChangeSet {
    match code.split(':').next().unwrap() {
        "un" => return s.undo(&mut cx.cx()),
        "re" => return s.redo(&mut cx.cx()),
        _ => {}
    }
    let cmds = commands(s, code, lay);
    let mut last = None;
    for c in cmds {
        last = Some(match s.exec(c, &mut cx.cx()).unwrap() {
            Outcome::Done(cs) => cs,
            Outcome::NeedsJobs(_) => panic!("no jobs"),
        });
    }
    last.unwrap_or_default()
}
