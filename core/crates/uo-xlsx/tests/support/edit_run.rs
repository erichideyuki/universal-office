//! The replay of `sheet-edit.capture` through the session (shared by the parity test and the hostile/memory tests).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::{Clock, RandomSource};
use uo_testkit::Capture;
use uo_xlsx::grid::{
    CellCtx, CellText, CellView, FontDesc, IdentityEnv, Laid, MeasureTable, Measurer, PaintBook, SheetPaint, cf, layout_cell,
    layout_cell_layers,
};
use uo_xlsx::save::{AlignChange, FontChange, StyleChange};
use uo_xlsx::session::{BorderKind, ClearKind, FindOptions, MergeKind, NumFmtArg, Session, SessionOpts};

pub type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

pub const CF_CLOCK: cf::CfClock = cf::CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

pub fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

pub fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap();
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap();
    String::from_utf8(raw).unwrap()
}

pub fn sidecar(path: &Path) -> Side {
    let mut out: Side = BTreeMap::new();
    for line in gunzip(path).lines() {
        let mut it = line.splitn(3, '\t');
        if let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next()) {
            out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
        }
    }
    out
}

pub fn sha_hex(b: &[u8]) -> String {
    uo_testkit::stream::hex(&uo_testkit::sha256::sha256(b))
}

/// `dig(s)` of capture-edit: `byteLength:sha256[0..16]`.
pub fn dg(s: &str) -> String {
    format!("{}:{}", s.len(), &sha_hex(s.as_bytes())[..16])
}

/// `listDig(lines)`: `n:sha256[0..32]` of the lines joined by `\n`.
pub fn list_dig(lines: &[String]) -> String {
    format!("{}:{}", lines.len(), &sha_hex(lines.join("\n").as_bytes())[..32])
}

/// `${n}:${sha16(join)}`.
pub fn n_sha16(lines: &[String]) -> String {
    format!("{}:{}", lines.len(), &sha_hex(lines.join("\n").as_bytes())[..16])
}

pub fn wb_bytes(wb: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = wb.strip_prefix("x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else {
        samples_dir().join(wb.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

/// The TS's `Math.random` of one script: xorshift32 seeded from `sha256(key)[0..8]`, `/ 2^32`.
pub fn random_for(key: &str) -> RandomSource {
    let h = uo_testkit::sha256::sha256(key.as_bytes());
    let seed = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
    let mut g = uo_testkit::generate::Rng::new(if seed == 0 { 1 } else { seed });
    RandomSource::sequence((0..4096).map(|_| f64::from(g.next_u32()) / 4_294_967_296.0).collect())
}

/// The fake canvas of the harness: 6 px per UTF-16 unit, the `'Hg'` box 9/3.
pub struct Fake;

impl Measurer for Fake {
    fn advance(&self, _f: &FontDesc, cp: u32) -> i32 {
        if cp > 0xFFFF { 12 } else { 6 }
    }
    fn text_width(&self, _f: &FontDesc, s: &[u16]) -> i32 {
        6 * i32::try_from(s.len()).unwrap()
    }
    fn font_box(&self, _f: &FontDesc) -> (Option<f64>, Option<f64>) {
        (Some(9.0), Some(3.0))
    }
}

pub fn opts(lang: &str) -> SessionOpts {
    SessionOpts { locale: lang.to_owned(), clock: Clock { now_serial: 45672.5 }, year: 2025.0, ..SessionOpts::default() }
}

pub fn open(bytes: Vec<u8>, key: &str, lang: &str) -> Session {
    Session::open_with(bytes, opts(lang), random_for(key), Some(Box::new(Fake))).unwrap()
}

/// The texts of a language (`T` of the script).
pub struct Texts {
    pub num: &'static str,
    pub neg: &'static str,
    pub pct: &'static str,
    pub date: &'static str,
    pub money: &'static str,
    pub big: &'static str,
    pub text: &'static str,
    pub quoted: &'static str,
    pub bool_: &'static str,
    pub sum: &'static str,
    pub iff: &'static str,
    pub cat: &'static str,
    pub seq: &'static str,
    pub bad: &'static str,
    pub today: &'static str,
    pub refr: &'static str,
    pub rnd: &'static str,
    pub find: &'static str,
    pub find_num: &'static str,
    pub sum_name: &'static str,
}

pub fn texts(lang: &str) -> Texts {
    if lang == "pt-BR" {
        Texts {
            num: "1,5",
            neg: "-0,5",
            pct: "50%",
            date: "12/03/2024",
            money: "R$ 10,00",
            big: "1.234,5",
            text: "texto ção",
            quoted: "'007",
            bool_: "VERDADEIRO",
            sum: "=SOMA(A1:A5)*2",
            iff: "=SE(A2>1;\"s\";\"n\")",
            cat: "=A1&\"x\"",
            seq: "=SEQUÊNCIA(3;2)",
            bad: "=SOMA(",
            today: "=HOJE()",
            refr: "=B2+1",
            rnd: "=ALEATÓRIO()",
            find: "ção",
            find_num: "1",
            sum_name: "SOMA",
        }
    } else {
        Texts {
            num: "1.5",
            neg: "-0.5",
            pct: "50%",
            date: "3/12/2024",
            money: "$10.00",
            big: "1,234.5",
            text: "texto ção",
            quoted: "'007",
            bool_: "TRUE",
            sum: "=SUM(A1:A5)*2",
            iff: "=IF(A2>1,\"s\",\"n\")",
            cat: "=A1&\"x\"",
            seq: "=SEQUENCE(3,2)",
            bad: "=SUM(",
            today: "=TODAY()",
            refr: "=B2+1",
            rnd: "=RAND()",
            find: "ção",
            find_num: "1",
            sum_name: "SUM",
        }
    }
}

pub fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn js(s: &[u16]) -> String {
    let mut o = String::new();
    uo_jsnum::json::push_string_utf16(&mut o, s);
    o
}

fn num(x: f64) -> String {
    uo_jsnum::json::number(x)
}

/// Runs one step of a script; returns the result as the capture writes it (`-` for undefined, JSON otherwise),
/// `None` when the step is not known to the replay.
pub fn run_step(s: &mut Session, op: &str, desc: &str, t: &Texts) -> Option<String> {
    let sel = |s: &mut Session, r: &str| s.select_range(r).unwrap();
    let txt = |s: &mut Session, cell: &str, text: &str| -> String {
        s.select_range(cell).unwrap();
        s.set_active_cell_text(&u(text)).to_json()
    };
    let font = |b: Option<bool>, i: Option<bool>| FontChange { bold: b, italic: i, ..FontChange::default() };
    let fill = |c: Option<&str>| StyleChange { fill: Some(c.map(str::to_owned)), ..StyleChange::default() };
    let dash = "-".to_owned();
    if op == "undo" {
        return Some(s.undo().to_string());
    }
    if op == "redo" {
        return Some(s.redo().to_string());
    }
    let r = match desc {
        // entry-const
        "A1 number" => txt(s, "A1", t.num),
        "B2 text with accents" => txt(s, "B2", t.text),
        "C3 percent" => txt(s, "C3", t.pct),
        "D4 date" => txt(s, "D4", t.date),
        "E5 currency" => txt(s, "E5", t.money),
        "F6 quoted text" => txt(s, "F6", t.quoted),
        "G7 boolean" => txt(s, "G7", t.bool_),
        "B30 far below the used range (grows)" => txt(s, "B30", t.big),
        "A1 cleared by empty text" => txt(s, "A1", ""),
        // entry-formula
        "D1 sum" => txt(s, "D1", t.sum),
        "D2 concat" => txt(s, "D2", t.cat),
        "D3 unbalanced formula refused" => txt(s, "D3", t.bad),
        "E1 spill" => txt(s, "E1", t.seq),
        "D4 date function (format hint)" => txt(s, "D4", t.today),
        "D5 if" => txt(s, "D5", t.iff),
        "E1 overwrite the spill anchor with a constant" => txt(s, "E1", t.num),
        "recalculate" => {
            s.recalculate();
            dash
        }
        // entry-editor
        "beginEdit A2, editor value, commit" => {
            s.begin_edit(1, 2, Some(&u(t.refr)), false);
            s.set_editor_text(&u(t.refr));
            s.commit_edit().to_string()
        }
        "beginEdit B3 then cancel" => {
            s.begin_edit(2, 3, Some(&u("abc")), false);
            s.cancel_edit();
            "\"cancelled\"".to_owned()
        }
        "typing a character (keydown) then commit" => {
            sel(s, "C2");
            let (c, r) = (s.selection().col, s.selection().row);
            s.begin_edit(c, r, Some(&u("x")), false);
            s.set_editor_text(&u(t.num));
            s.commit_edit().to_string()
        }
        "F2 on a formula cell, commit unchanged" => {
            sel(s, "A2");
            let (c, r) = (s.selection().col, s.selection().row);
            s.begin_edit(c, r, None, true);
            s.commit_edit().to_string()
        }
        "refused entry keeps editing (false)" => {
            s.begin_edit(3, 3, Some(&u(t.bad)), false);
            let r = s.commit_edit();
            s.cancel_edit();
            r.to_string()
        }
        "Delete on one cell" => {
            sel(s, "C2");
            s.delete_key();
            dash
        }
        "Backspace on a range (clear contents)" => {
            sel(s, "A1:B2");
            s.delete_key();
            dash
        }
        "setCellValue (no undo record)" => s.set_cell_value(4, 4, &u(t.neg)).to_json(),
        // fmt-font
        "B2:C4 bold" => {
            sel(s, "B2:C4");
            s.set_font(font(Some(true), None));
            dash
        }
        "B2:C4 size 14, colour, italic" => {
            s.set_font(FontChange {
                size: Some(14.0),
                color: Some(Some("#C00000".to_owned())),
                italic: Some(true),
                ..FontChange::default()
            });
            dash
        }
        "A1 font name, underline, strike" => {
            sel(s, "A1");
            s.set_font(FontChange { name: Some("Arial".to_owned()), underline: Some(true), strike: Some(true), ..FontChange::default() });
            dash
        }
        "D2:E3 fill" => {
            sel(s, "D2:E3");
            s.set_format(&fill(Some("#FFFF00")));
            dash
        }
        "D2 fill removed" => {
            sel(s, "D2");
            s.set_format(&fill(None));
            dash
        }
        "B2:C4 all" => {
            sel(s, "B2:C4");
            s.set_borders(BorderKind::All, "#000000");
            dash
        }
        "B2:C4 thick outside" => {
            s.set_borders(BorderKind::ThickOutside, "#0000FF");
            dash
        }
        "B2:C4 bottom" => {
            s.set_borders(BorderKind::Bottom, "#000000");
            dash
        }
        "B2:C4 none" => {
            s.set_borders(BorderKind::None, "#000000");
            dash
        }
        "F1:F2 font+fill+border+align in one action" => {
            sel(s, "F1:F2");
            let b = uo_xlsx::read::model::Border {
                bottom: Some(uo_xlsx::read::model::BorderEdge { style: "thin".to_owned(), color: "#000000".to_owned() }),
                ..uo_xlsx::read::model::Border::default()
            };
            s.set_format(&StyleChange {
                font: Some(font(Some(true), None)),
                fill: Some(Some("#C6EFCE".to_owned())),
                align: Some(AlignChange { horizontal: Some(Some("center".to_owned())), ..AlignChange::default() }),
                border: Some(Some(b)),
                ..StyleChange::default()
            });
            dash
        }
        // fmt-number-align
        "B2:B4 percent" => {
            sel(s, "B2:B4");
            s.set_number_format(&NumFmtArg::Kind(uo_xlsx::grid::NumberFormatKind::Percent));
            dash
        }
        "B2:B4 currency" => {
            s.set_number_format(&NumFmtArg::Kind(uo_xlsx::grid::NumberFormatKind::Currency));
            dash
        }
        "B2:B4 short date" => {
            s.set_number_format(&NumFmtArg::Kind(uo_xlsx::grid::NumberFormatKind::ShortDate));
            dash
        }
        "B2:B4 custom code" => {
            s.set_number_format(&NumFmtArg::Code("#,##0.000".to_owned()));
            dash
        }
        "B2:B4 +1 decimal" => {
            s.adjust_decimals(1);
            dash
        }
        "B2:B4 -1 decimal" => {
            s.adjust_decimals(-1);
            dash
        }
        "C2:C3 centre + wrap" => {
            sel(s, "C2:C3");
            s.set_alignment(AlignChange { horizontal: Some(Some("center".to_owned())), wrap_text: Some(true), ..AlignChange::default() });
            dash
        }
        "C2:C3 indent 2 left, vertical top" => {
            s.set_alignment(AlignChange {
                horizontal: Some(Some("left".to_owned())),
                indent: Some(2.0),
                vertical: Some(Some("top".to_owned())),
                ..AlignChange::default()
            });
            dash
        }
        "C2:C3 alignment reset" => {
            s.set_alignment(AlignChange { horizontal: Some(None), vertical: Some(None), ..AlignChange::default() });
            dash
        }
        // fmt-lines
        "column B fill (whole column)" => {
            sel(s, "B:B");
            s.set_format(&fill(Some("#DDEBF7")));
            dash
        }
        "rows 2:3 bold (whole rows)" => {
            sel(s, "2:3");
            s.set_font(font(Some(true), None));
            dash
        }
        "select all, number format" => {
            s.select_all();
            s.set_number_format(&NumFmtArg::Code("0.00".to_owned()));
            dash
        }
        "columns C:D borders" => {
            sel(s, "C:D");
            s.set_borders(BorderKind::All, "#000000");
            dash
        }
        "column B clear formats (whole)" => {
            sel(s, "B:B");
            s.clear(ClearKind::Formats);
            dash
        }
        // merge
        "B2:C3 merge and centre" => {
            sel(s, "B2:C3");
            let l = s.merge_loses_data();
            s.merge(MergeKind::Center);
            format!("[{l},null]")
        }
        "D2:F4 merge across" => {
            sel(s, "D2:F4");
            s.merge(MergeKind::Across);
            dash
        }
        "A6:B7 merge cells" => {
            sel(s, "A6:B7");
            s.merge(MergeKind::Cells);
            dash
        }
        "B2:D3 overlapping an existing merge" => {
            sel(s, "B2:D3");
            s.merge(MergeKind::Cells);
            dash
        }
        "unmerge A6" => {
            sel(s, "A6");
            s.unmerge();
            dash
        }
        "unmerge a selection with no merge (no-op)" => {
            sel(s, "H20");
            s.unmerge();
            dash
        }
        // fill-clipboard
        "seed A1" => txt(s, "A1", t.sum),
        "A1:A5 down (formula shifts)" => {
            sel(s, "A1:A5");
            s.fill(true);
            dash
        }
        "A1:D2 right" => {
            sel(s, "A1:D2");
            s.fill(false);
            dash
        }
        "A3 alone (copies the row above)" => {
            sel(s, "A3");
            s.fill(true);
            dash
        }
        "copy A1:B2 then paste at D6" => {
            sel(s, "A1:B2");
            let x = s.copy(false);
            sel(s, "D6");
            s.paste(None, false);
            js(&x)
        }
        "cut A1:A2 then paste at F8" => {
            sel(s, "A1:A2");
            let x = s.copy(true);
            sel(s, "F8");
            s.paste(None, false);
            js(&x)
        }
        "copy D6:E7 then paste values at H2" => {
            sel(s, "D6:E7");
            s.copy(false);
            sel(s, "H2");
            s.paste(None, true);
            dash
        }
        "external TSV text pasted at A9" => {
            sel(s, "A9");
            let x = format!("1\t{}\r\n{}\t{}\r\n", t.text, t.date, t.sum);
            s.paste(Some(&u(&x)), false);
            dash
        }
        // clear
        "B2:C3 bold (so formats exist)" => {
            sel(s, "B2:C3");
            s.set_font(font(Some(true), None));
            dash
        }
        "A1:C3 contents" => {
            sel(s, "A1:C3");
            s.clear(ClearKind::Contents);
            dash
        }
        "A1:C3 formats" => {
            s.clear(ClearKind::Formats);
            dash
        }
        "D1:F4 all" => {
            sel(s, "D1:F4");
            s.clear(ClearKind::All);
            dash
        }
        "rows 1:2 all (whole rows)" => {
            sel(s, "1:2");
            s.clear(ClearKind::All);
            dash
        }
        "column A contents (whole column)" => {
            sel(s, "A:A");
            s.clear(ClearKind::Contents);
            dash
        }
        // find-replace
        "seed B2" => txt(s, "B2", t.text),
        "seed C3" => txt(s, "C3", t.text),
        "find the accent word from A1" => {
            sel(s, "A1");
            hit(s.find(&u(t.find), FindOptions::default()))
        }
        "find backwards, values" => {
            hit(s.find(&u(t.find), FindOptions { backwards: true, look_in_values: true, ..FindOptions::default() }))
        }
        "find all sheets, whole cell, match case" => {
            hit(s.find(&u(t.text), FindOptions { all_sheets: true, whole_cell: true, match_case: true, ..FindOptions::default() }))
        }
        "replace on the active cell" => {
            sel(s, "B2");
            hit(s.replace(&u("texto"), &u("TXT"), FindOptions::default()))
        }
        "replace all \"ção\"" => s.replace_all(&u(t.find), &u("cao"), FindOptions::default()).to_string(),
        "replace all digit 1 -> 7 on every sheet" => {
            s.replace_all(&u(t.find_num), &u("7"), FindOptions { all_sheets: true, ..FindOptions::default() }).to_string()
        }
        "replace all whole-cell match case" => {
            s.replace_all(&u("TXT"), &u("x"), FindOptions { whole_cell: true, match_case: true, ..FindOptions::default() }).to_string()
        }
        // view-freeze
        "freeze 1 col, 1 row" => {
            s.freeze_panes(Some(1.0), Some(1.0));
            panes(s)
        }
        "freeze at the active cell C3" => {
            sel(s, "C3");
            s.freeze_panes(None, None);
            panes(s)
        }
        "freeze top row only" => {
            s.freeze_panes(Some(0.0), Some(1.0));
            panes(s)
        }
        "unfreeze" | "unfreeze again (no-op)" => {
            s.freeze_panes(Some(0.0), Some(0.0));
            panes(s)
        }
        "gridlines off" => {
            s.set_show_gridlines(false);
            s.show_gridlines().to_string()
        }
        "gridlines on" => {
            s.set_show_gridlines(true);
            s.show_gridlines().to_string()
        }
        // size
        "B:C width 20" => {
            sel(s, "B:C");
            s.set_column_width(20.0, None);
            format!("[{},{}]", num(s.column_width(2)), num(s.column_width(3)))
        }
        "D width 3.5 (< 1 char path 0)" => {
            sel(s, "D1");
            s.set_column_width(0.5, None);
            num(s.column_width(4))
        }
        "rows 2:3 height 30" => {
            sel(s, "2:3");
            s.set_row_height(30.0, None);
            format!("[{},{}]", num(s.row_height_pt(2)), num(s.row_height_pt(3)))
        }
        "autofit columns B:C" => {
            sel(s, "B:C");
            s.autofit_columns(None);
            format!("[{},{}]", num(s.column_width(2)), num(s.column_width(3)))
        }
        "autofit rows 2:3" => {
            sel(s, "2:3");
            s.autofit_rows(None);
            format!("[{},{}]", num(s.row_height_pt(2)), num(s.row_height_pt(3)))
        }
        "hide rows 4:5" => {
            sel(s, "4:5");
            s.set_hidden(false, true);
            dash
        }
        "hide column C" => {
            sel(s, "C:C");
            s.set_hidden(true, true);
            dash
        }
        "unhide columns B:D (neighbours)" => {
            sel(s, "B:D");
            s.set_hidden(true, false);
            dash
        }
        "unhide rows 3:6" => {
            sel(s, "3:6");
            s.set_hidden(false, false);
            dash
        }
        "unhide with nothing hidden (no-op)" => {
            sel(s, "B:B");
            s.set_hidden(true, false);
            dash
        }
        // size-drag
        "drag column B +40 px" => drag(s, true, 2, 40.0),
        "drag row 2 +12 px" => drag(s, false, 2, 12.0),
        "drag column C to 0 (hides)" => drag(s, true, 3, -2000.0),
        "drag row 3 to 0 (hides)" => drag(s, false, 3, -2000.0),
        // sheets
        "A1 on the first sheet (unsaved edit travels through addSheet)" => txt(s, "A1", t.num),
        "addSheet() default name" => match s.add_sheet(None) {
            Ok(n) => js(&u(&n)),
            Err(e) => format!("throw:{e}"),
        },
        "A1 on the new sheet" => txt(s, "A1", t.pct),
        "addSheet(\"Resumo\")" => match s.add_sheet(Some("Resumo")) {
            Ok(n) => js(&u(&n)),
            Err(e) => format!("throw:{e}"),
        },
        "cross-sheet formula" => {
            let f = t.sum.replace("A1:A5", "'Resumo'!A1:A5");
            txt(s, "B1", &f)
        }
        "select sheet 0" => {
            s.select_sheet(0);
            s.selection().sheet.to_string()
        }
        "undo the cross-sheet formula (the stack was restarted by the addSheet before it)" => s.undo().to_string(),
        "newWorkbook()" => {
            s.new_workbook().unwrap();
            dash
        }
        // autosum
        "AutoSum on a range A1:C4 (one result per column below)" => {
            sel(s, "A1:C4");
            s.auto_sum("SUM");
            dash
        }
        "AutoSum AVERAGE on B1:B3" => {
            sel(s, "B1:B3");
            s.auto_sum("AVERAGE");
            dash
        }
        "AutoSum single cell: edit state, then commit" => {
            sel(s, "A8");
            s.auto_sum("SUM");
            let text = s.editing.as_ref().map(|e| e.text.clone()).unwrap_or_default();
            s.commit_edit();
            js(&text)
        }
        "AutoSum MAX single cell next to numbers" => {
            sel(s, "E3");
            s.auto_sum("MAX");
            let text = s.editing.as_ref().map(|e| e.text.clone()).unwrap_or_default();
            s.cancel_edit();
            js(&text)
        }
        "insertFunction then complete and commit" => {
            sel(s, "H1");
            s.insert_function(t.sum_name);
            let text = s.editing.as_ref().map(|e| e.text.clone()).unwrap_or_default();
            let mut full = text.clone();
            full.extend(u("A1:A3)"));
            s.set_editor_text(&full);
            s.commit_edit();
            js(&text)
        }
        "getFunctionNames (count)" => s.function_names().len().to_string(),
        // host-names
        "upsert a workbook-level name" => {
            let n = uo_xlsx::read::model::DefinedName {
                name: "Capt_One".into(),
                formula: "'Sheet1'!$A$1:$A$3".into(),
                local_sheet_id: None,
                hidden: false,
            };
            uo_xlsx::save::calc::update_defined_names(&mut s.book.pkg, &[n], &[]).unwrap();
            "\"names 1+0\"".to_owned()
        }
        "upsert a sheet-local name (localSheetId 0) and hidden" => {
            let a = uo_xlsx::read::model::DefinedName {
                name: "Capt_Local".into(),
                formula: "1+2".into(),
                local_sheet_id: Some(0.0),
                hidden: false,
            };
            let b = uo_xlsx::read::model::DefinedName { name: "Capt_Hid".into(), formula: "42".into(), local_sheet_id: None, hidden: true };
            uo_xlsx::save::calc::update_defined_names(&mut s.book.pkg, &[a, b], &[]).unwrap();
            "\"names 2+0\"".to_owned()
        }
        "a formula that uses the name" => txt(s, "A1", &format!("={}(Capt_One)", t.sum_name)),
        "change one name and remove another" => {
            let a = uo_xlsx::read::model::DefinedName {
                name: "Capt_One".into(),
                formula: "'Sheet1'!$B$1:$B$3".into(),
                local_sheet_id: None,
                hidden: false,
            };
            uo_xlsx::save::calc::update_defined_names(&mut s.book.pkg, &[a], &[("Capt_Hid".to_owned(), None)]).unwrap();
            "\"names 1+1\"".to_owned()
        }
        _ => return None,
    };
    Some(r)
}

fn hit(h: Option<(usize, String)>) -> String {
    match h {
        Some((s, r)) => format!("{{\"sheet\":{s},\"ref\":{}}}", uo_jsnum::json::string(&r)),
        None => "null".to_owned(),
    }
}

fn panes(s: &Session) -> String {
    let (c, r) = s.frozen_panes();
    format!("{{\"cols\":{c},\"rows\":{r}}}")
}

fn drag(s: &mut Session, cols: bool, index: u32, delta: f64) -> String {
    let px = s.line_px(cols, index);
    s.resize_drag(cols, index, px + delta);
    format!("[{},{index},{}]", if cols { "\"col\"" } else { "\"row\"" }, num(px))
}

// ---------------------------------------------------------------- facets

/// Inserts `"key":value` into the top level of a canonical JSON object (keys sorted by UTF-16 units; ASCII here).
pub fn insert_key(obj: &str, key: &str, value: &str) -> String {
    let b = obj.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    let mut i = 0;
    let entry = format!("\"{key}\":{value}");
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    let sep = if obj[..i].ends_with('{') { "" } else { "," };
                    return format!("{}{sep}{entry}{}", &obj[..i], &obj[i..]);
                }
            }
            b'"' => {
                if depth == 1 && (b[i - 1] == b'{' || b[i - 1] == b',') {
                    // a key: compare
                    let end = obj[i + 1..].find('"').map_or(b.len(), |k| i + 1 + k);
                    let k = &obj[i + 1..end];
                    if k > key {
                        return format!("{}{entry},{}", &obj[..i], &obj[i..]);
                    }
                }
                in_str = true;
            }
            _ => {}
        }
        i += 1;
    }
    obj.to_owned()
}

/// The `ws<i>` text: the worksheet model without rows, with the presence the TS keeps (`cols: []`, `mergedAreas`).
pub fn ws_text(s: &Session, vi: usize) -> String {
    let v = &s.views[vi];
    let ws = s.book.sheets[v.sheet].as_ref().unwrap();
    let mut t = uo_xlsx::read::facet::worksheet(ws);
    if ws.cols.is_empty() && v.cols_present {
        t = insert_key(&t, "cols", "[]");
    }
    if ws.merges.is_empty() && v.merged_areas_present {
        t = insert_key(&t, "mergedAreas", "{\"$map\":[]}");
    }
    t
}

pub fn store_lines(s: &Session, vi: usize) -> Vec<String> {
    let ix = uo_calc::SheetIx(s.views[vi].sheet as u32);
    let rp = &s.book.run_props;
    let props = |id: u32| rp.get(id).map(uo_xlsx::read::facet::run_props_json).unwrap_or_default();
    uo_calc::facet::store_facet(&s.book.store, ix, &props)
}

/// The displayed text of every cell of the active sheet (`debugCellDisplays`).
pub fn display_lines(s: &mut Session) -> Vec<String> {
    use uo_calc::store::CellKind as LaneKind;
    let vi = s.selection().sheet;
    if vi >= s.views.len() {
        return Vec::new();
    }
    let si = s.views[vi].sheet;
    let ix = uo_calc::SheetIx(si as u32);
    let has_cf = !s.book.sheets[si].as_ref().unwrap().cond_formats.is_empty();
    let has_tables = {
        let ws = s.book.sheets[si].as_ref().unwrap();
        !ws.tables.is_empty() || !ws.pivots.is_empty()
    };
    let mut eng = None;
    if has_cf && let Some(e) = cf::CfEngine::for_book(&s.book, si, CF_CLOCK, cf::CfLimits::default()) {
        cf::prepare_book(&mut s.book, &e).unwrap();
        eng = Some(e);
    }
    let tables = if has_cf && has_tables {
        let ws = s.book.sheets[si].as_ref().unwrap();
        let mut pb = PaintBook::build(&s.book.styles);
        Some(SheetPaint::build(&mut pb, ws, &s.book.styles, &s.book.cx))
    } else {
        None
    };
    // the model's cell order: row-major, or the file order (shadowed duplicates included) of a `FileOrder` sheet
    let cells = model_order(s, ix);
    for &(r, c, shadow) in &cells {
        let sst = if let Some(k) = shadow {
            let fo = s.book.store.sheet(ix).unwrap().file_order.as_deref().unwrap();
            let fc = &fo.shadowed[k].1;
            (fc.t.as_ref().is_some_and(|t| t.as_js().to_string_lossy() == "s"))
                .then(|| fc.v.as_ref().map(|v| uo_jsnum::parse::to_number(&v.as_js().to_string_lossy())))
                .flatten()
        } else {
            s.book.store.get(ix, r, c).filter(|cell| cell.kind.tag() == uo_calc::store::ValueTag::Sst).map(|cell| cell.payload as f64)
        };
        if let Some(i) = sst
            && i >= 0.0
            && i.fract() == 0.0
            && i < 4e9
        {
            let _ = s.book.resolve_sst(i as u32);
        }
    }
    let ctx: CellCtx<'_> = s.cell_ctx(vi);
    let styles = s.styles.clone();
    let view = CellView { store: &s.book.store, fonts: &s.book.styles.fonts, styles: &styles, ctx };
    let mut table = MeasureTable::new();
    let mut scratch = Vec::new();
    let mut out = CellText::default();
    let grid = &s.views[vi].grid;
    let mut lines = Vec::with_capacity(cells.len());
    for (row, col, shadow) in cells {
        if let Some(k) = shadow {
            let fo = s.book.store.sheet(ix).unwrap().file_order.as_deref().unwrap();
            let fc = fo.shadowed[k].1.clone();
            let rf = fc
                .r#ref
                .as_ref()
                .map_or_else(|| format!("{}{}", uo_calc::a1::col_to_letters(col), row + 1), |r| r.as_js().to_string_lossy());
            let w = grid.hidden_cols.get(col + 1).map_or_else(|| grid.col_widths.get(col + 1).unwrap_or(grid.default_col_w), |_| 0.0);
            let got = shadow_display(&view, &mut table, &fc, w, &mut scratch, &mut out);
            lines.push(format!("{rf}	{got}"));
            continue;
        }
        let cell = s.book.store.get(ix, row, col).unwrap();
        let rf = if cell.kind.has(LaneKind::HAS_EXTRA) {
            s.book.store.extra(ix, row, col).and_then(|e| e.raw_ref.as_ref()).map(|r| r.as_js().to_string_lossy())
        } else {
            None
        }
        .unwrap_or_else(|| format!("{}{}", uo_calc::a1::col_to_letters(col), row + 1));
        let mm = grid.merges.merge_at(col + 1, row + 1);
        let (c0, c1) = match mm.filter(|m| m.is_origin) {
            Some(m) => (m.first_col, m.last_col),
            None => (col + 1, col + 1),
        };
        let mut w = 0.0;
        for k in c0..=c1 {
            if grid.hidden_cols.get(k).is_none() {
                w += grid.col_widths.get(k).unwrap_or(grid.default_col_w);
            }
        }
        let layer = eng.as_mut().and_then(|e: &mut cf::CfEngine| e.at(&s.book.store, row, col));
        let tf = tables.as_ref().and_then(|sp| sp.format_at(col + 1, row + 1));
        let got = loop {
            let laid = match (tables.as_ref(), eng.as_ref()) {
                (Some(_), e) => layout_cell_layers(
                    &view,
                    &mut table,
                    &IdentityEnv,
                    ix,
                    row,
                    col,
                    cell,
                    w,
                    1.0,
                    &mut scratch,
                    &mut out,
                    tf.as_ref(),
                    e.zip(layer.as_ref()),
                ),
                (None, Some(e)) => cf::layout_cell_cf(
                    &view,
                    &mut table,
                    &IdentityEnv,
                    ix,
                    row,
                    col,
                    cell,
                    w,
                    1.0,
                    &mut scratch,
                    &mut out,
                    e,
                    layer.as_ref(),
                ),
                (None, None) => layout_cell(&view, &mut table, &IdentityEnv, ix, row, col, cell, w, 1.0, &mut scratch, &mut out),
            };
            match laid {
                Laid::Empty => break String::new(),
                Laid::NeedSst(_) => break String::new(),
                Laid::Done(_) => {
                    if table.has_needs() {
                        table.satisfy(&Fake);
                        continue;
                    }
                    break String::from_utf16_lossy(&out.text);
                }
            }
        };
        lines.push(format!("{rf}\t{got}"));
    }
    lines
}

/// `(row, col, shadow index)` of every model cell (0-based): the TS `ws.rows` order.
fn model_order(s: &Session, ix: uo_calc::SheetIx) -> Vec<(u32, u32, Option<usize>)> {
    let sh = s.book.store.sheet(ix).unwrap();
    let mut live: Vec<(u32, u32)> = Vec::new();
    s.book.store.each(uo_calc::Area { sheet: ix, r1: 0, c1: 0, r2: uo_calc::MAX_ROW, c2: uo_calc::MAX_COL }, |r, c, _| live.push((r, c)));
    let Some(fo) = sh.file_order.as_deref() else { return live.into_iter().map(|(r, c)| (r, c, None)).collect() };
    let mut rows: Vec<(u32, Vec<u32>)> = fo.rows.iter().map(|fr| (fr.row, fr.cells.clone())).collect();
    let listed: std::collections::HashSet<(u32, u32)> =
        rows.iter().flat_map(|(r, cs)| cs.iter().filter(|&&c| c & uo_calc::load::FileRow::SHADOW == 0).map(move |&c| (*r, c))).collect();
    let col_of = |it: u32| -> u32 {
        if it & uo_calc::load::FileRow::SHADOW != 0 {
            fo.shadowed.get((it & !uo_calc::load::FileRow::SHADOW) as usize).map_or(0, |x| x.0)
        } else {
            it
        }
    };
    for (r, c) in live {
        if listed.contains(&(r, c)) {
            continue;
        }
        // `createCell`: lowerRow (binary search), else the first element with that row, else a new row there
        let (mut lo, mut hi) = (0usize, rows.len());
        while lo < hi {
            let m = (lo + hi) >> 1;
            if rows[m].0 < r { lo = m + 1 } else { hi = m }
        }
        let at = if rows.get(lo).is_some_and(|x| x.0 == r) { Some(lo) } else { rows.iter().position(|x| x.0 == r) };
        let i = match at {
            Some(i) => i,
            None => {
                rows.insert(lo, (r, Vec::new()));
                lo
            }
        };
        let items = &mut rows[i].1;
        let (mut a, mut b) = (0usize, items.len());
        while a < b {
            let m = (a + b) >> 1;
            if col_of(items[m]) < c { a = m + 1 } else { b = m }
        }
        items.insert(a, c);
    }
    let mut out = Vec::new();
    for (r, items) in rows {
        for it in items {
            if it & uo_calc::load::FileRow::SHADOW != 0 {
                let k = (it & !uo_calc::load::FileRow::SHADOW) as usize;
                out.push((r, col_of(it), Some(k)));
            } else if s.book.store.get(ix, r, it).is_some() {
                out.push((r, it, None));
            }
        }
    }
    out
}

/// The display of a shadowed duplicate (the model holds it; the store does not): `cellLayout` over its `type`/`<v>`.
fn shadow_display(
    v: &CellView<'_>,
    table: &mut MeasureTable,
    fc: &uo_calc::FileCell,
    col_px: f64,
    scratch: &mut Vec<u16>,
    out: &mut CellText,
) -> String {
    let xf = v.styles.xf(fc.s.unwrap_or(0.0));
    let code = xf.code16.clone();
    let inp = match uo_xlsx::grid::file_text_input(v.store, fc, &code, &v.ctx, scratch) {
        uo_xlsx::grid::CellRead::Input(i) => i,
        _ => return String::new(),
    };
    let font = xf.font.and_then(|f| v.fonts.get(f as usize));
    let nf = &v.styles.normal;
    loop {
        let id = table.font(
            &uo_xlsx::grid::FontRequest {
                name: font.and_then(|f| f.name.as_deref()).unwrap_or(&nf.name),
                alt_name: font.and_then(|f| f.alt_name.as_deref()),
                size_pt: font.and_then(|f| f.size).unwrap_or(11.0),
                bold: font.is_some_and(|f| f.bold),
                italic: font.is_some_and(|f| f.italic),
                zoom: 1.0,
            },
            &IdentityEnv,
        );
        uo_xlsx::grid::layout_cell_text(&inp, &table.metrics(id), col_px, out);
        if table.has_needs() {
            table.satisfy(&Fake);
            continue;
        }
        return String::from_utf16_lossy(&out.text);
    }
}

fn set_text(set: &std::collections::BTreeSet<usize>) -> String {
    if set.is_empty() { "-".to_owned() } else { set.iter().map(ToString::to_string).collect::<Vec<_>>().join(",") }
}

fn stack_text<'a>(entries: impl Iterator<Item = &'a uo_xlsx::session::UndoEntry>) -> String {
    let e: Vec<String> = entries
        .map(|e| {
            let shape = |snaps: &[uo_xlsx::session::SheetSnap]| -> String {
                snaps
                    .iter()
                    .map(|s| {
                        format!(
                            "{}:{}:{}",
                            s.sheet,
                            s.cells.as_ref().map_or("-".to_owned(), |c| c.len().to_string()),
                            if s.structure.is_some() { "S" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!("{}[{}>{}]", e.label, shape(&e.before), shape(&e.after))
        })
        .collect();
    format!("{}:{}", e.len(), e.join(" "))
}

fn runs_map(r: &uo_xlsx::grid::Runs<f64>) -> String {
    let mut v = Vec::new();
    for &(a, b, x) in r.runs() {
        for k in a..=b {
            v.push(format!("[{k},{}]", num(x)));
        }
    }
    format!("{{\"$map\":[{}]}}", v.join(","))
}

fn runs_set(r: &uo_xlsx::grid::Runs<bool>) -> String {
    let mut v = Vec::new();
    for &(a, b, _) in r.runs() {
        for k in a..=b {
            v.push(k.to_string());
        }
    }
    format!("{{\"$set\":[{}]}}", v.join(","))
}

fn struct_json(st: &uo_xlsx::session::StructSnap) -> String {
    let cols: Vec<String> = st
        .cols
        .iter()
        .map(|c| {
            let mut x = vec![];
            if c.custom_width {
                x.push("\"customWidth\":true".to_owned());
            }
            if c.hidden {
                x.push("\"hidden\":true".to_owned());
            }
            x.push(format!("\"max\":{}", num(c.max)));
            x.push(format!("\"min\":{}", num(c.min)));
            if let Some(s) = c.style {
                x.push(format!("\"style\":{}", num(s)));
            }
            if let Some(w) = c.width {
                x.push(format!("\"width\":{}", num(w)));
            }
            format!("{{{}}}", x.join(","))
        })
        .collect();
    let rows: Vec<String> = st
        .rows
        .iter()
        .map(|(r, a)| {
            let mut x = vec![];
            if a.custom_format {
                x.push("\"customFormat\":true".to_owned());
            }
            if a.custom_height {
                x.push("\"customHeight\":true".to_owned());
            }
            if let Some(h) = a.height {
                x.push(format!("\"height\":{}", num(h)));
            }
            if a.hidden {
                x.push("\"hidden\":true".to_owned());
            }
            if let Some(s) = a.style {
                x.push(format!("\"style\":{}", num(s)));
            }
            format!("[{r},{{{}}}]", x.join(","))
        })
        .collect();
    let mut o = vec![
        format!("\"colWidths\":{}", runs_map(&st.col_widths)),
        format!("\"cols\":[{}]", cols.join(",")),
        format!("\"frozenCols\":{}", st.frozen_cols),
        format!("\"frozenRows\":{}", st.frozen_rows),
        format!("\"hiddenCols\":{}", runs_set(&st.hidden_cols)),
        format!("\"hiddenRows\":{}", runs_set(&st.hidden_rows)),
        format!("\"merges\":[{}]", st.merges.iter().map(|m| uo_jsnum::json::string(m)).collect::<Vec<_>>().join(",")),
    ];
    if let Some(p) = &st.pane {
        let mut x = vec![format!("\"state\":{}", uo_jsnum::json::string(&p.state))];
        if let Some(t) = &p.top_left_cell {
            x.push(format!("\"topLeftCell\":{}", uo_jsnum::json::string(t)));
        }
        x.push(format!("\"xSplit\":{}", num(p.x_split)));
        x.push(format!("\"ySplit\":{}", num(p.y_split)));
        o.push(format!("\"pane\":{{{}}}", x.join(",")));
    }
    o.push(format!("\"rowHeights\":{}", runs_map(&st.row_heights)));
    o.push(format!("\"rows\":{{\"$map\":[{}]}}", rows.join(",")));
    o.push(format!("\"showGridLines\":{}", st.show_grid_lines));
    format!("{{{}}}", o.join(","))
}

/// The canonical JSON of an undo entry (`entryText`); `with_view_maps` false drops the view's size maps (the TS keeps
/// them in Map insertion order, which the runs here do not record: compared without them, declared C5b-3).
pub fn entry_json(e: &uo_xlsx::session::UndoEntry, with_view_maps: bool) -> String {
    let snaps = |v: &[uo_xlsx::session::SheetSnap]| -> String {
        let items: Vec<String> = v
            .iter()
            .map(|s| {
                let mut o = vec![];
                if let Some(c) = &s.cells {
                    let cells: Vec<String> = c
                        .iter()
                        .map(|c| {
                            let mut x = format!("{{\"col\":{},\"edit\":{},\"exists\":{},\"row\":{}", c.col, js(&c.edit), c.exists, c.row);
                            if let Some(st) = c.style {
                                x.push_str(&format!(",\"style\":{}", num(st)));
                            }
                            x.push('}');
                            x
                        })
                        .collect();
                    o.push(format!("\"cells\":[{}]", cells.join(",")));
                }
                o.push(format!("\"sheet\":{}", s.sheet));
                if let Some(st) = &s.structure {
                    let mut j = struct_json(st);
                    if !with_view_maps {
                        j = strip_view_maps(&j);
                    }
                    o.push(format!("\"struct\":{j}"));
                }
                format!("{{{}}}", o.join(","))
            })
            .collect();
        format!("[{}]", items.join(","))
    };
    format!("{{\"after\":{},\"before\":{},\"label\":{}}}", snaps(&e.after), snaps(&e.before), uo_jsnum::json::string(e.label))
}

/// Drops `colWidths`, `rowHeights`, `hiddenCols`, `hiddenRows` from a struct JSON (top level of the object).
pub fn strip_view_maps(j: &str) -> String {
    let mut out = j.to_owned();
    for key in ["colWidths", "rowHeights", "hiddenCols", "hiddenRows"] {
        let pat = format!("\"{key}\":");
        if let Some(i) = out.find(&pat) {
            // the value is an object `{...}`: find its end
            let start = i + pat.len();
            let b = out.as_bytes();
            let mut depth = 0;
            let mut k = start;
            while k < b.len() {
                match b[k] {
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            let mut a = i;
            let mut e = k;
            if out.as_bytes().get(e) == Some(&b',') {
                e += 1;
            } else if a > 0 && out.as_bytes()[a - 1] == b',' {
                a -= 1;
            }
            out.replace_range(a..e, "");
        }
    }
    out
}

/// Parts of a package: `name len sha[..32]` in archive order, plus the data.
pub fn parts_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    names.into_iter().map(|n| (n.clone(), pkg.part(&n).unwrap().unwrap().to_vec())).collect()
}

/// The facets of the current state, keyed by facet name (`sel`, `undo`, …), in the capture's format.
pub struct Facets {
    pub map: BTreeMap<String, String>,
    pub save_parts: Vec<(String, Vec<u8>)>,
    pub utop_json: Option<String>,
    pub utop_json_nomaps: Option<String>,
    pub displays: Vec<String>,
    pub stores: Vec<Vec<String>>,
}

pub fn facets(s: &mut Session, base: &[(String, Vec<u8>)], prev: &[(String, Vec<u8>)]) -> Facets {
    let mut m = BTreeMap::new();
    let sel = s.selection();
    m.insert(
        "sel".to_owned(),
        format!("{}|{},{}|{},{},{},{}", sel.sheet, sel.col, sel.row, sel.range.c1, sel.range.r1, sel.range.c2, sel.range.r2),
    );
    m.insert("undo".to_owned(), stack_text(s.undo_entries()));
    m.insert("redo".to_owned(), stack_text(s.redo_entries()));
    let top = s.undo_entries().last().cloned();
    let (utop_json, utop_nomaps) = match &top {
        Some(e) => {
            let j = entry_json(e, true);
            m.insert("utop".to_owned(), dg(&j));
            (Some(j), Some(entry_json(e, false)))
        }
        None => {
            m.insert("utop".to_owned(), "-".to_owned());
            (None, None)
        }
    };
    m.insert(
        "dirty".to_owned(),
        format!(
            "dirty={} dirtySheets={} merge={} view={}",
            s.is_dirty(),
            set_text(&s.dirty_sheets),
            set_text(&s.merge_dirty),
            set_text(&s.view_dirty)
        ),
    );
    let (fc, fr) = s.frozen_panes();
    m.insert(
        "sheets".to_owned(),
        format!(
            "{} zoom=1 frozen={fc},{fr} grid={} clip={} undo={} redo={}",
            s.sheet_names().join("|"),
            s.show_gridlines(),
            s.has_clipboard(),
            s.can_undo(),
            s.can_redo()
        ),
    );
    let mut stores = Vec::new();
    for vi in 0..s.views.len() {
        let st = store_lines(s, vi);
        m.insert(format!("st{vi}"), n_sha16(&st));
        stores.push(st);
        m.insert(format!("ws{vi}"), dg(&ws_text(s, vi)));
        let ix = uo_calc::SheetIx(s.views[vi].sheet as u32);
        let rw = uo_calc::facet::row_meta_lines(&s.book.store, ix);
        m.insert(format!("rw{vi}"), list_dig(&rw).chars().take(20).collect());
    }
    let displays = display_lines(s);
    m.insert(format!("d{}", s.selection().sheet), n_sha16(&displays));
    let mut save_parts = Vec::new();
    match s.save() {
        Ok(bytes) => {
            let parts = parts_of(&bytes);
            let lines: Vec<String> = parts.iter().map(|(n, d)| format!("{n} {} {}", d.len(), &sha_hex(d)[..32])).collect();
            m.insert("save".to_owned(), list_dig(&lines).chars().take(24).collect());
            let diff = |r: &[(String, Vec<u8>)]| -> Vec<String> {
                let mut v: Vec<String> =
                    parts.iter().filter(|(n, d)| r.iter().find(|x| &x.0 == n).is_none_or(|x| &x.1 != d)).map(|(n, _)| n.clone()).collect();
                v.extend(r.iter().filter(|(n, _)| !parts.iter().any(|x| &x.0 == n)).map(|(n, _)| format!("-{n}")));
                v
            };
            let chg = diff(base);
            m.insert("chg".to_owned(), if chg.is_empty() { "-".to_owned() } else { chg.join(" ") });
            let chgp = diff(prev);
            m.insert("chgp".to_owned(), if chgp.is_empty() { "-".to_owned() } else { chgp.join(" ") });
            save_parts = parts;
        }
        Err(e) => {
            m.insert("save".to_owned(), format!("err:{e}"));
        }
    }
    m.insert("xf".to_owned(), format!("{}:{}", s.book.styles.cell_xfs.len(), s.book.styles.num_fmts.len()));
    Facets { map: m, save_parts, utop_json, utop_json_nomaps: utop_nomaps, displays, stores }
}

// ---------------------------------------------------------------- the replay

#[derive(Default)]
pub struct Totals {
    pub scripts: usize,
    pub steps: usize,
    pub facets_compared: usize,
    pub facets_equal: usize,
    /// per facet kind: (compared, equal)
    pub by_facet: BTreeMap<String, (usize, usize)>,
    /// per op: (steps, steps with every facet equal)
    pub by_op: BTreeMap<String, (usize, usize)>,
    pub mismatches: Vec<String>,
    pub results_compared: usize,
    pub results_equal: usize,
    pub unknown_steps: Vec<String>,
    /// explained classes: name -> count
    pub explained: BTreeMap<String, usize>,
    /// scripts with the duplicated-position class
    pub dup_scripts: std::collections::BTreeSet<String>,
}

impl Totals {
    pub fn unexplained(&self) -> usize {
        self.mismatches.len() + self.unknown_steps.len()
    }

    pub fn report(&self) {
        eprintln!(
            "sheet-edit: {} scripts, {} steps; facets {} compared, {} equal; results {} compared, {} equal; {} unknown steps",
            self.scripts,
            self.steps,
            self.facets_compared,
            self.facets_equal,
            self.results_compared,
            self.results_equal,
            self.unknown_steps.len()
        );
        for (k, (c, e)) in &self.by_facet {
            eprintln!("  facet {k:6} {c:5} compared {e:5} equal");
        }
        for (k, (c, e)) in &self.by_op {
            eprintln!("  op {k:18} {c:4} steps {e:4} all facets equal");
        }
        for (k, n) in &self.explained {
            eprintln!("  explained {k}: {n}");
        }
        for k in &self.dup_scripts {
            eprintln!("  C5b-5 script: {k}");
        }
        for m in self.mismatches.iter().take(2000) {
            eprintln!("  MISMATCH {m}");
        }
        for m in self.unknown_steps.iter().take(20) {
            eprintln!("  UNKNOWN {m}");
        }
    }
}

fn facet_kind(f: &str) -> String {
    f.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned()
}

pub fn replay_capture(only: Option<&str>, verbose: bool) -> Totals {
    let cap = Capture::read(dir().join("sheet-edit.capture")).unwrap();
    let side = sidecar(&dir().join("sheet-edit.side.gz"));
    let mut t = Totals::default();
    for rec in cap.records() {
        let key = rec.key.as_str();
        if only.is_some_and(|o| !key.contains(o)) {
            continue;
        }
        let parts: Vec<&str> = key.split('/').collect();
        // edit/<scenario>/<x1|samples>/<name>/<lang>
        let (wb, lang) = (format!("{}/{}", parts[2], parts[3]), parts[4]);
        let Some(bytes) = wb_bytes(&wb) else { continue };
        t.scripts += 1;
        let tx = texts(lang);
        let base = parts_of(&bytes);
        let mut s = open(bytes, key, lang);
        // SH1 (T16): the store of the open differs from the TS model only in a workbook with a `<c r>` on another
        // `<row r>`; decided after the open's facets
        let sh1_book = has_sh1(&s);
        let dup_book = sh1_book || s.book.store.sheets.iter().any(|sh| sh.file_order.as_ref().is_some_and(|fo| !fo.shadowed.is_empty()));
        let mut sh1 = false;
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        let mut prev_parts = base.clone();
        let mut ts_stores: Vec<Vec<String>> = Vec::new();
        let mut utop_jsons: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut steps = 0;
        while rec.get(&format!("e{steps}")).is_some() {
            steps += 1;
        }
        for k in 0..=steps {
            let p = if k == 0 { "o".to_owned() } else { format!("e{}", k - 1) };
            let mut op = "open".to_owned();
            if k > 0 {
                let step = rec.get(&p).unwrap_or("");
                let (o, d) = step.split_once(" | ").unwrap_or((step, ""));
                op = o.to_owned();
                t.steps += 1;
                match run_step(&mut s, o, d, &tx) {
                    Some(r) => {
                        let r = one_line(&r);
                        let wr = rec.get(&format!("{p}.r")).unwrap_or("-");
                        t.results_compared += 1;
                        if wr == r {
                            t.results_equal += 1;
                        } else {
                            t.mismatches.push(format!("{key} {p} ({step}) result: ts {wr} rust {r}"));
                        }
                    }
                    None => {
                        t.unknown_steps.push(format!("{key} {p}: {step}"));
                        break;
                    }
                }
            }
            // the capture's facets of this step (carried forward)
            for (f, v) in &rec.facets {
                if let Some(name) = f.strip_prefix(&format!("{p}.")) {
                    if name.contains(':') || name == "r" || name == "xmlfeat" {
                        continue;
                    }
                    want.insert(name.to_owned(), v.to_owned());
                }
            }
            if k > 0 {
                for (vi, st) in ts_stores.iter_mut().enumerate() {
                    if let Some(d) = side.get(key).and_then(|m| m.get(&format!("{p}.st{vi}"))) {
                        *st = apply_delta(st, d);
                    }
                }
            }
            if let Some(j) = side.get(key).and_then(|m| m.get(&format!("{p}.utop"))).and_then(|v| v.last()) {
                utop_jsons.insert(dg(j), j.clone());
            }
            let got = facets(&mut s, &base, &prev_parts);
            let mut all_ok = true;
            if k == 0 && sh1_book {
                sh1 = want.iter().any(|(n, w)| n.starts_with("st") && got.map.get(n).is_some_and(|g| g != w));
            }
            for (name, w) in &want {
                // per-step facets (not carried): undo/redo/utop/save/chg/chgp are written every step
                let Some(g) = got.map.get(name) else { continue };
                let kind = facet_kind(name);
                let e = t.by_facet.entry(kind.clone()).or_default();
                e.0 += 1;
                t.facets_compared += 1;
                if g == w {
                    e.1 += 1;
                    t.facets_equal += 1;
                    continue;
                }
                // utop: the TS keeps the view's size maps in insertion order (C5b-3): compare without them
                if kind == "utop"
                    && let Some(ts_json) = utop_jsons.get(w.as_str())
                    && got.utop_json_nomaps.as_deref() == Some(strip_struct_maps(ts_json).as_str())
                {
                    e.1 += 1;
                    t.facets_equal += 1;
                    *t.explained.entry("utop: view size maps in insertion order (C5b-3)".to_owned()).or_default() += 1;
                    continue;
                }
                // a structural entry whose TS JSON is not in the sidecar (> 4,000 characters): same byte length, the
                // size maps in another insertion order (C5b-3)
                if kind == "utop"
                    && !utop_jsons.contains_key(w.as_str())
                    && s.undo_entries().last().is_some_and(|e| e.before.iter().any(|x| x.structure.is_some()))
                    && w.split(':').next() == g.split(':').next()
                {
                    e.1 += 1;
                    t.facets_equal += 1;
                    *t.explained
                        .entry("utop: structural entry > 4,000 chars, equal length, size maps not comparable (C5b-3)".to_owned())
                        .or_default() += 1;
                    continue;
                }
                // duplicated positions (a shadowed `<c>`, an implicit `<row>` holding refs of another row): the TS
                // `removeCell` misses the cell of the other element (a stripped zombie stays) or the shadow surfaces in
                // its model once the live cell goes; the store keeps one cell per position (C5b-5, the SH1 family)
                if dup_book && matches!(kind.as_str(), "st" | "d" | "save") {
                    *t.explained
                        .entry("duplicated positions: TS zombie / surfacing shadow after a removal (C5b-5)".to_owned())
                        .or_default() += 1;
                    t.dup_scripts.insert(key.to_owned());
                    all_ok = false;
                    continue;
                }
                // replaceAll walks the TS cell index in Map insertion order (T11: order-insensitive, values only)
                if kind == "utop"
                    && s.undo_entries().last().is_some_and(|e| e.label == "replaceAll")
                    && match utop_jsons.get(w.as_str()) {
                        Some(j) => sorted_cells(j) == sorted_cells(got.utop_json.as_deref().unwrap_or("")),
                        None => w.split(':').next() == g.split(':').next(),
                    }
                {
                    e.1 += 1;
                    t.facets_equal += 1;
                    *t.explained.entry("utop: replaceAll hits in the TS index order (T11)".to_owned()).or_default() += 1;
                    continue;
                }
                if sh1 {
                    *t.explained.entry("SH1: a <c r> on another <row r> (the store places it by element)".to_owned()).or_default() += 1;
                    all_ok = false;
                    continue;
                }
                all_ok = false;
                let mut detail = if verbose { detail_of(&side, key, &p, name, &got, &s) } else { String::new() };
                if verbose && kind == "st" {
                    let vi: usize = name[2..].parse().unwrap_or(0);
                    if let (Some(ts), Some(ours)) = (ts_stores.get(vi), got.stores.get(vi)) {
                        for l in ts.iter().filter(|l| !ours.contains(l)).take(4) {
                            detail.push_str(&format!(
                                "
    only ts:   {l}"
                            ));
                        }
                        for l in ours.iter().filter(|l| !ts.contains(l)).take(4) {
                            detail.push_str(&format!(
                                "
    only rust: {l}"
                            ));
                        }
                    }
                }
                t.mismatches.push(format!("{key} {p} [{op}] {name}: ts {w} rust {g}{detail}"));
            }
            let e = t.by_op.entry(op).or_default();
            e.0 += 1;
            if all_ok {
                e.1 += 1;
            }
            if !got.save_parts.is_empty() {
                prev_parts = got.save_parts;
            }
            // the TS store lines: the open's (equal to ours when `o` matched) + every step's delta
            if k == 0 {
                ts_stores = got.stores.clone();
            }
        }
    }
    t
}

/// The cell objects of every `"cells":[…]` array sorted by their text (the order-insensitive form of an entry).
pub fn sorted_cells(j: &str) -> String {
    let mut out = String::new();
    let mut rest = j;
    while let Some(i) = rest.find("\"cells\":[") {
        let start = i + "\"cells\":[".len();
        out.push_str(&rest[..start]);
        let end = rest[start..].find(']').map_or(rest.len(), |k| start + k);
        let body = &rest[start..end];
        let mut items: Vec<String> = Vec::new();
        let mut depth = 0;
        let mut cur = String::new();
        let mut in_str = false;
        let mut esc = false;
        for ch in body.chars() {
            if in_str {
                cur.push(ch);
                if esc {
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => {
                    in_str = true;
                    cur.push(ch);
                }
                '{' => {
                    depth += 1;
                    cur.push(ch);
                }
                '}' => {
                    depth -= 1;
                    cur.push(ch);
                    if depth == 0 {
                        items.push(std::mem::take(&mut cur));
                    }
                }
                ',' if depth == 0 => {}
                _ => cur.push(ch),
            }
        }
        items.sort();
        out.push_str(&items.join(","));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `oneLine(s, 240)` of the capture: tabs/newlines to a space, `=` to `:`, 240 UTF-16 units.
pub fn one_line(s: &str) -> String {
    let mut out: Vec<u16> = Vec::new();
    let mut prev_ws = false;
    for u in s.encode_utf16() {
        if matches!(u, 9 | 10 | 13) {
            if !prev_ws {
                out.push(32);
            }
            prev_ws = true;
            continue;
        }
        prev_ws = false;
        out.push(if u == 61 { 58 } else { u });
    }
    out.truncate(240);
    String::from_utf16_lossy(&out)
}

/// A cell whose `r` names another row than its `<row>` (SH1 of `uo-calc.md` T16).
pub fn has_sh1(s: &Session) -> bool {
    s.book.store.sheets.iter().any(|sh| {
        sh.extras.iter().any(|(k, e)| {
            e.raw_ref.as_ref().is_some_and(|r| {
                let text = r.as_js().to_string_lossy();
                uo_xlsx::read::ref_to_row_col(text.as_bytes()).is_some_and(|(_, row)| row - 1.0 != (*k / 16384) as f64)
            })
        })
    })
}

/// The TS store lines of a step: the previous lines with the sidecar delta (`+line`, `-r,c`) applied.
pub fn apply_delta(prev: &[String], delta: &[String]) -> Vec<String> {
    let key = |t: &str| -> (u64, u64) {
        let num = |k: &str| {
            t.split(k).nth(1).and_then(|x| x.split(|ch: char| !ch.is_ascii_digit()).next()).and_then(|x| x.parse().ok()).unwrap_or(0)
        };
        (num("\"r\":"), num("{\"c\":"))
    };
    let mut map: BTreeMap<(u64, u64), String> = prev.iter().map(|l| (key(l), l.clone())).collect();
    for d in delta {
        if let Some(x) = d.strip_prefix('+') {
            map.insert(key(x), x.to_owned());
        } else if let Some(x) = d.strip_prefix('-') {
            let mut it = x.split(',');
            let r: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let c: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            map.remove(&(r, c));
        }
    }
    map.into_values().collect()
}

/// The struct maps of every snapshot of a TS entry JSON dropped (`"struct":{…}` objects).
pub fn strip_struct_maps(j: &str) -> String {
    let mut out = String::with_capacity(j.len());
    let mut rest = j;
    while let Some(i) = rest.find("\"struct\":{") {
        let start = i + "\"struct\":".len();
        out.push_str(&rest[..start]);
        let b = rest.as_bytes();
        let mut depth = 0;
        let mut k = start;
        let mut in_str = false;
        let mut esc = false;
        while k < b.len() {
            let c = b[k];
            if in_str {
                if esc {
                    esc = false;
                } else if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    in_str = false;
                }
            } else {
                match c {
                    b'"' => in_str = true,
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            k += 1;
        }
        out.push_str(&strip_view_maps(&rest[start..k]));
        rest = &rest[k..];
    }
    out.push_str(rest);
    out
}

fn detail_of(side: &Side, key: &str, p: &str, name: &str, got: &Facets, s: &Session) -> String {
    let kind = facet_kind(name);
    let ts = side.get(key);
    match kind.as_str() {
        "utop" => {
            let tj = ts.and_then(|m| m.get(&format!("{p}.utop"))).and_then(|v| v.last()).cloned().unwrap_or_default();
            let gj = got.utop_json.clone().unwrap_or_default();
            let at = tj.bytes().zip(gj.bytes()).position(|(a, b)| a != b).unwrap_or(tj.len().min(gj.len()));
            let from = at.saturating_sub(80);
            format!("\n    ts:   ...{}\n    rust: ...{}", cut(&tj, from, 260), cut(&gj, from, 260))
        }
        "st" => {
            let lines = ts.and_then(|m| m.get(&format!("{p}.{name}"))).cloned().unwrap_or_default();
            let vi: usize = name[2..].parse().unwrap_or(0);
            let ours = got.stores.get(vi).cloned().unwrap_or_default();
            let mut out = String::new();
            for l in lines.iter().take(8) {
                let present = l.strip_prefix('+').is_some_and(|x| ours.iter().any(|o| o == x));
                out.push_str(&format!("\n    ts delta {l}{}", if present { "  (present here)" } else { "  (NOT here)" }));
            }
            out.push_str(&format!("\n    rust lines: {}", ours.len()));
            out
        }
        "ws" => {
            let vi: usize = name[2..].parse().unwrap_or(0);
            let tj = ts.and_then(|m| m.get(&format!("{p}.{name}"))).and_then(|v| v.last()).cloned().unwrap_or_default();
            let gj = ws_text(s, vi);
            let at = tj.bytes().zip(gj.bytes()).position(|(a, b)| a != b).unwrap_or(tj.len().min(gj.len()));
            let from = at.saturating_sub(60);
            format!("\n    ts:   ...{}\n    rust: ...{}", cut(&tj, from, 200), cut(&gj, from, 200))
        }
        "d" => {
            let lines = ts.and_then(|m| m.get(&format!("{p}.{name}"))).cloned().unwrap_or_default();
            let mut out = String::new();
            for l in lines.iter().take(6) {
                out.push_str(&format!("\n    ts delta {l}"));
            }
            out
        }
        "rw" => {
            let lines = ts.and_then(|m| m.get(&format!("{p}.{name}"))).cloned().unwrap_or_default();
            let vi: usize = name[2..].parse().unwrap_or(0);
            let ix = uo_calc::SheetIx(s.views[vi].sheet as u32);
            let ours = uo_calc::facet::row_meta_lines(&s.book.store, ix);
            format!("\n    ts {:?}\n    rust {:?}", lines.iter().take(6).collect::<Vec<_>>(), ours.iter().take(6).collect::<Vec<_>>())
        }
        "save" | "chgp" | "chg" => {
            let mut out = String::new();
            for (part, data) in &got.save_parts {
                if let Some(ts_x) = ts.and_then(|m| m.get(&format!("{p}.save:{part}"))).and_then(|v| v.last()) {
                    out.push_str(&format!("\n    part {part} (ts changed it): {} bytes here; ts diff {}", data.len(), cut(ts_x, 0, 300)));
                }
            }
            out
        }
        _ => String::new(),
    }
}

pub fn cut(s: &str, from: usize, n: usize) -> String {
    let mut a = from.min(s.len());
    while !s.is_char_boundary(a) {
        a -= 1;
    }
    let mut b = (from + n).min(s.len());
    while !s.is_char_boundary(b) {
        b += 1;
    }
    s[a..b].to_owned()
}
