//! The Sheet EDITOR exports (`uo-sheet-editor.wasm` = features `sheet` + `sheet-edit`; frontier C5b, record
//! `core/docs/modules/uo-xlsx.md` "C5b — as built"): the editing session of `<universal-sheet engine="rust">`
//! ([`uo_xlsx::session::Session`]) with its own viewer, so the frame the shell paints after an edit shows it.
//!
//! One handle `UoSheetEditor` owns the session and a [`Viewer`] over the session's book. Commands are one UTF-8 line,
//! `op` then its arguments separated by tabs; the answer is JSON: `{"ok":true,"result":…,"state":{…}}` or
//! `{"ok":false,"error":"…","state":{…}}`. The state is what the shell shows: the selection, the sheets, undo/redo, the
//! dirty flag, panes, grid lines, the clipboard flag, the formula bar of the active cell and the editor in progress.
//!
//! The viewport, its needs/font/provide protocol and the bitmaps have the layouts of `uo_sheet_*` (src/sheet.rs). After a
//! command that changed the model the viewer is rebuilt (its measurement table is kept) and the active sheet activated
//! again at the next frame: correct, not incremental (record, open item).

use std::cell::RefCell;

use uo_calc::{Clock, RandomSource};
use uo_xlsx::grid::cf::CfClock;
use uo_xlsx::grid::{FontId, IdentityEnv, MeasureTable, NumberFormatKind, ViewportRequest};
use uo_xlsx::read::model::{Border, BorderEdge};
use uo_xlsx::save::{AlignChange, FontChange, StyleChange};
use uo_xlsx::session::{AutoSum, BorderKind, ClearKind, EditResult, FindOptions, MergeKind, NumFmtArg, Session, SessionOpts};
use uo_xlsx::view::{ViewError, ViewParams, Viewer};

use crate::buf::{NULL_HANDLE, OK, Out, UoBuf, buf, take_bytes};

/// A frame that needs the shell's measurements.
pub const NEED: i32 = 12;
const OPEN: i32 = 3;
const BAD_ARG: i32 = 5;
const NO_SHEET: i32 = 6;
const ENCODE: i32 = 10;
const SAVE: i32 = 13;

/// The session, its viewer and the state of the frames.
pub struct UoSheetEditor {
    session: Session,
    viewer: Viewer,
    clock: CfClock,
    /// Model generation (bumped by every command that may change the model) and the one the viewer shows.
    model_gen: u64,
    viewer_gen: u64,
    accent: String,
}

thread_local! {
    static LAST: RefCell<(i32, String)> = const { RefCell::new((0, String::new())) };
}

fn set_last(code: i32, msg: String) {
    LAST.with(|l| *l.borrow_mut() = (code, msg));
}

/// Opens a workbook for editing. `locale` is the `formula-locale` (`pt-BR`, `en-US`…); the clock is the host's (the date of
/// the conditional formats and `now_serial` for `NOW()`/`TODAY()`); `seed` seeds `RAND()`. Null on failure (`uo_sheet_edit_error`).
///
/// # Safety
/// `ptr`/`len` and `lptr`/`llen` must each come from one call to `uo_alloc_bytes(len)`, filled and not consumed yet.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_sheet_edit_open(
    ptr: *mut u8,
    len: usize,
    lptr: *mut u8,
    llen: usize,
    year: f64,
    month: f64,
    day: f64,
    seconds: f64,
    now_serial: f64,
    seed: u32,
) -> *mut UoSheetEditor {
    // SAFETY: the caller guarantees both buffers came from uo_alloc_bytes and are filled.
    let (bytes, locale) = unsafe { (take_bytes(ptr, len), take_bytes(lptr, llen)) };
    let locale = String::from_utf8(locale).unwrap_or_default();
    let opts = SessionOpts {
        locale: if locale.is_empty() { "pt-BR".to_owned() } else { locale },
        clock: Clock { now_serial },
        year,
        ..SessionOpts::default()
    };
    let random = RandomSource::seeded(u64::from(seed.max(1)));
    let session = if bytes.is_empty() { Session::new_blank(opts, random) } else { Session::open_with(bytes, opts, random, None) };
    match session {
        Ok(session) => {
            let clock = CfClock { year, month, day, seconds };
            let viewer = Viewer::new(&session.book, 1, clock);
            Box::into_raw(Box::new(UoSheetEditor { session, viewer, clock, model_gen: 0, viewer_gen: 0, accent: "#006d35".to_owned() }))
        }
        Err(e) => {
            set_last(OPEN, e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// The status and message of the last failed open (and clears them).
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_edit_error() -> *mut UoBuf {
    let (c, m) = LAST.with(|l| std::mem::take(&mut *l.borrow_mut()));
    buf(c, m.into_bytes())
}

/// # Safety
/// `h` must be null or a live handle, released once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_free(h: *mut UoSheetEditor) {
    if !h.is_null() {
        // SAFETY: the caller guarantees `h` came from uo_sheet_edit_open and is released once.
        drop(unsafe { Box::from_raw(h) });
    }
}

/// # Safety
/// `h` must be null or a live handle.
unsafe fn ed<'a>(h: *mut UoSheetEditor) -> Option<&'a mut UoSheetEditor> {
    // SAFETY: the caller guarantees `h` is null or a live handle with no other reference in use.
    unsafe { h.as_mut() }
}

// ---------------------------------------------------------------- JSON

fn js(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn js16(u: &[u16]) -> String {
    let mut o = String::with_capacity(u.len() + 2);
    o.push('"');
    for r in char::decode_utf16(u.iter().copied()) {
        match r {
            Ok(c) => match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\r' => o.push_str("\\r"),
                '\t' => o.push_str("\\t"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            },
            Err(e) => o.push_str(&format!("\\u{:04x}", e.unpaired_surrogate())),
        }
    }
    o.push('"');
    o
}

fn num(x: f64) -> String {
    if x.is_finite() { format!("{x}") } else { "null".to_owned() }
}

fn state(s: &mut Session) -> String {
    let sel = s.selection();
    let names: Vec<String> = s.sheet_names().iter().map(|n| js(n)).collect();
    let (fc, fr) = s.frozen_panes();
    let bar = if sel.sheet < s.views.len() { s.bar_text(sel.sheet, sel.col, sel.row) } else { Default::default() };
    let editing = s
        .editing
        .as_ref()
        .map_or_else(|| "null".to_owned(), |e| format!("{{\"col\":{},\"row\":{},\"text\":{}}}", e.col, e.row, js16(&e.text)));
    let label = |l: Option<&str>| l.map_or_else(|| "null".to_owned(), js);
    format!(
        "{{\"sheet\":{},\"col\":{},\"row\":{},\"range\":[{},{},{},{}],\"sheets\":[{}],\"canUndo\":{},\"canRedo\":{},\"undoLabel\":{},\"redoLabel\":{},\"dirty\":{},\"frozen\":[{fc},{fr}],\"gridlines\":{},\"clip\":{},\"bar\":{{\"formula\":{},\"edit\":{},\"ghost\":{}}},\"editing\":{editing}}}",
        sel.sheet,
        sel.col,
        sel.row,
        sel.range.c1,
        sel.range.r1,
        sel.range.c2,
        sel.range.r2,
        names.join(","),
        s.can_undo(),
        s.can_redo(),
        label(s.undo_label()),
        label(s.redo_label()),
        s.is_dirty(),
        s.show_gridlines(),
        s.has_clipboard(),
        js16(&bar.formula),
        js16(&bar.edit),
        bar.ghost
    )
}

fn hit(h: Option<(usize, String)>) -> String {
    h.map_or_else(|| "null".to_owned(), |(s, r)| format!("{{\"sheet\":{s},\"ref\":{}}}", js(&r)))
}

fn edit_json(r: &EditResult) -> String {
    r.to_json()
}

// ---------------------------------------------------------------- commands

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn n(a: Option<&&str>) -> Option<f64> {
    a.and_then(|x| num_arg(x))
}

/// A number argument as JS `Number(text)` reads it (empty and NaN = absent).
fn num_arg(x: &str) -> Option<f64> {
    if x.is_empty() {
        return None;
    }
    let v = uo_jsnum::parse::to_number(x);
    (!v.is_nan()).then_some(v)
}

fn u(a: Option<&&str>) -> Option<u32> {
    a.and_then(|x| x.trim_matches(' ').parse::<u32>().ok())
}

fn flag(v: &str) -> bool {
    matches!(v, "1" | "true")
}

fn num_fmt_kind(k: &str) -> Option<NumberFormatKind> {
    Some(match k {
        "general" => NumberFormatKind::General,
        "number" => NumberFormatKind::Number,
        "comma" => NumberFormatKind::Comma,
        "currency" => NumberFormatKind::Currency,
        "accounting" => NumberFormatKind::Accounting,
        "shortDate" => NumberFormatKind::ShortDate,
        "longDate" => NumberFormatKind::LongDate,
        "time" => NumberFormatKind::Time,
        "percent" => NumberFormatKind::Percent,
        "percent0" => NumberFormatKind::Percent0,
        "fraction" => NumberFormatKind::Fraction,
        "scientific" => NumberFormatKind::Scientific,
        "text" => NumberFormatKind::Text,
        _ => return None,
    })
}

/// `kind:<name>` | `code:<code>` | `id:<n>[:<code>]`.
fn num_fmt_arg(v: &str) -> Option<NumFmtArg> {
    let (k, rest) = v.split_once(':')?;
    match k {
        "kind" => num_fmt_kind(rest).map(NumFmtArg::Kind),
        "code" => Some(NumFmtArg::Code(rest.to_owned())),
        "id" => {
            let (id, code) = match rest.split_once(':') {
                Some((i, c)) => (i, Some(c.to_owned())),
                None => (rest, None),
            };
            num_arg(id).map(|i| NumFmtArg::Id(i, code))
        }
        _ => None,
    }
}

fn opt_text(v: &str) -> Option<String> {
    (v != "null").then(|| v.to_owned())
}

/// `style:color` of a border edge.
fn edge(v: &str) -> Option<BorderEdge> {
    let (style, color) = v.split_once(':').unwrap_or((v, "#000000"));
    (!style.is_empty()).then(|| BorderEdge { style: style.to_owned(), color: color.to_owned() })
}

/// The key=value arguments of `format` (`font`, `fill`, `align` are the same command): `bold italic underline strike size
/// name color` (font, `color=null` automatic), `fill` (`#RRGGBB` | `null`), `h`/`v` (alignment or `null`), `wrap`, `indent`,
/// `numfmt` (see [`num_fmt_arg`]), `border=null` or `border.left|right|top|bottom|diagonal=style:color`.
fn style_change(s: &mut Session, args: &[&str]) -> Result<StyleChange, String> {
    let mut c = StyleChange::default();
    let mut font = FontChange::default();
    let mut has_font = false;
    let mut align = AlignChange::default();
    let mut has_align = false;
    let mut border: Option<Border> = None;
    for a in args {
        let (k, v) = a.split_once('=').ok_or_else(|| format!("bad argument {a}"))?;
        match k {
            "bold" => (font.bold, has_font) = (Some(flag(v)), true),
            "italic" => (font.italic, has_font) = (Some(flag(v)), true),
            "underline" => (font.underline, has_font) = (Some(flag(v)), true),
            "strike" => (font.strike, has_font) = (Some(flag(v)), true),
            "size" => (font.size, has_font) = (num_arg(v), true),
            "name" => (font.name, has_font) = (Some(v.to_owned()), true),
            "color" => (font.color, has_font) = (Some(opt_text(v)), true),
            "fill" => c.fill = Some(opt_text(v)),
            "h" => (align.horizontal, has_align) = (Some(opt_text(v)), true),
            "v" => (align.vertical, has_align) = (Some(opt_text(v)), true),
            "wrap" => (align.wrap_text, has_align) = (Some(flag(v)), true),
            "indent" => (align.indent, has_align) = (num_arg(v), true),
            "numfmt" => {
                let f = num_fmt_arg(v).ok_or_else(|| format!("bad number format {v}"))?;
                c.num_fmt = Some(s.resolve_num_fmt(&f));
            }
            "border" if v == "null" => c.border = Some(None),
            k if k.starts_with("border.") => {
                let b = border.get_or_insert_with(Border::default);
                let e = edge(v);
                match &k[7..] {
                    "left" => b.left = e,
                    "right" => b.right = e,
                    "top" => b.top = e,
                    "bottom" => b.bottom = e,
                    "diagonal" => b.diagonal = e,
                    other => return Err(format!("bad border edge {other}")),
                }
            }
            _ => return Err(format!("unknown format key {k}")),
        }
    }
    if has_font {
        c.font = Some(font);
    }
    if has_align {
        c.align = Some(align);
    }
    if let Some(b) = border {
        c.border = Some(Some(b));
    }
    Ok(c)
}

fn find_opts(f: Option<&&str>) -> FindOptions {
    let f = f.copied().unwrap_or("");
    FindOptions {
        all_sheets: f.contains('a'),
        backwards: f.contains('b'),
        look_in_values: f.contains('v'),
        whole_cell: f.contains('w'),
        match_case: f.contains('c'),
    }
}

fn lines_arg(a: Option<&&str>) -> Option<Vec<u32>> {
    let a = a?;
    if a.is_empty() || *a == "-" {
        return None;
    }
    Some(a.split(',').filter_map(|x| x.trim_matches(' ').parse().ok()).collect())
}

/// An argument as the shell wrote it: `\\` is a backslash, `\t` a tab (the separator of the line).
fn unescape(a: &str) -> String {
    if !a.contains('\\') {
        return a.to_owned();
    }
    let mut o = String::with_capacity(a.len());
    let mut it = a.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('t') => o.push('\t'),
            Some(x) => o.push(x),
            None => o.push('\\'),
        }
    }
    o
}

/// Runs one command; `Ok((result JSON, changed the model))`.
fn run(e: &mut UoSheetEditor, line: &str) -> Result<(String, bool), String> {
    let mut it = line.split('\t');
    let op = it.next().unwrap_or("");
    let owned: Vec<String> = it.map(unescape).collect();
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let s = &mut e.session;
    let a0 = args.first().copied().unwrap_or("");
    let dash = "null".to_owned();
    Ok(match op {
        "state" => (dash, false),
        "select" => {
            s.select_range(a0).map_err(|x| x.to_string())?;
            (dash, false)
        }
        "selection" => {
            // selection sheet col row c1 r1 c2 r2: the shell's selection as it is (the active cell anywhere in the range); the
            // formula bar of the active cell is read as `emitSelection` does (it builds the engine where the TS does)
            let v: Vec<u32> = (0..7).filter_map(|i| u(args.get(i))).collect();
            let [sheet, col, row, c1, r1, c2, r2] = v[..] else { return Err("bad selection".to_owned()) };
            let max = |x: u32, m: u32| x >= 1 && x <= m;
            if sheet as usize >= s.views.len()
                || !(max(col, 16_384)
                    && max(c1, 16_384)
                    && max(c2, 16_384)
                    && max(row, 1_048_576)
                    && max(r1, 1_048_576)
                    && max(r2, 1_048_576))
                || c1 > c2
                || r1 > r2
            {
                return Err("bad selection".to_owned());
            }
            if sheet as usize != s.selection().sheet {
                s.select_sheet(sheet as usize);
            }
            s.sel.col = col;
            s.sel.row = row;
            s.sel.range = uo_xlsx::session::Rect { c1, r1, c2, r2 };
            s.sel.corner = (c2, r2);
            let _ = s.bar_text(sheet as usize, col, row);
            (dash, false)
        }
        "select_sheet" => {
            s.select_sheet(u(args.first()).ok_or("bad sheet")? as usize);
            (dash, false)
        }
        "select_all" => {
            s.select_all();
            (dash, false)
        }
        "enter" => (edit_json(&s.set_active_cell_text(&u16s(a0))), true),
        "set_cell" => {
            let (c, r) = (u(args.first()).ok_or("bad col")?, u(args.get(1)).ok_or("bad row")?);
            (edit_json(&s.set_cell_value(c, r, &u16s(args.get(2).copied().unwrap_or("")))), true)
        }
        "begin_edit" => {
            // begin_edit col row mode [text]: mode `text` (the initial text), `keep` (F2: the bar text), `empty`
            let (c, r) = (u(args.first()).ok_or("bad col")?, u(args.get(1)).ok_or("bad row")?);
            match args.get(2).copied().unwrap_or("empty") {
                "text" => s.begin_edit(c, r, Some(&u16s(args.get(3).copied().unwrap_or(""))), false),
                "keep" => s.begin_edit(c, r, None, true),
                _ => s.begin_edit(c, r, None, false),
            }
            (dash, false)
        }
        "editor_text" => {
            s.set_editor_text(&u16s(a0));
            (dash, false)
        }
        "commit" => (s.commit_edit().to_string(), true),
        "cancel" => {
            s.cancel_edit();
            (dash, false)
        }
        "delete" => (edit_json(&s.delete_key()), true),
        "format" | "font" | "fill" | "align" => {
            let c = style_change(s, &args)?;
            s.set_format(&c);
            (dash, true)
        }
        "numfmt" => {
            let f = num_fmt_arg(a0).ok_or("bad number format")?;
            s.set_number_format(&f);
            (dash, true)
        }
        "decimals" => {
            s.adjust_decimals(if a0.starts_with('-') { -1 } else { 1 });
            (dash, true)
        }
        "border" => {
            let kind = match a0 {
                "all" => BorderKind::All,
                "outside" => BorderKind::Outside,
                "thickOutside" => BorderKind::ThickOutside,
                "top" => BorderKind::Top,
                "bottom" => BorderKind::Bottom,
                "left" => BorderKind::Left,
                "right" => BorderKind::Right,
                "none" => BorderKind::None,
                _ => return Err(format!("bad border kind {a0}")),
            };
            s.set_borders(kind, args.get(1).copied().unwrap_or("#000000"));
            (dash, true)
        }
        "merge" => {
            let kind = match a0 {
                "across" => MergeKind::Across,
                "cells" => MergeKind::Cells,
                _ => MergeKind::Center,
            };
            s.merge(kind);
            (dash, true)
        }
        "unmerge" => {
            s.unmerge();
            (dash, true)
        }
        "merge_loses_data" => (s.merge_loses_data().to_string(), false),
        "fill_down" => {
            s.fill(true);
            (dash, true)
        }
        "fill_right" => {
            s.fill(false);
            (dash, true)
        }
        "copy" => (js16(&s.copy(false)), false),
        "cut" => (js16(&s.copy(true)), false),
        "paste" => {
            // paste [values] [text]: values `1` = Paste Values; text absent = the internal clip
            let values = flag(a0);
            let text = args.get(1).map(|t| u16s(t));
            s.paste(text.as_deref(), values);
            (dash, true)
        }
        "clear" => {
            let k = match a0 {
                "formats" => ClearKind::Formats,
                "all" => ClearKind::All,
                _ => ClearKind::Contents,
            };
            s.clear(k);
            (dash, true)
        }
        "find" => (hit(s.find(&u16s(a0), find_opts(args.get(1)))), false),
        "replace" => (hit(s.replace(&u16s(a0), &u16s(args.get(1).copied().unwrap_or("")), find_opts(args.get(2)))), true),
        "replace_all" => (s.replace_all(&u16s(a0), &u16s(args.get(1).copied().unwrap_or("")), find_opts(args.get(2))).to_string(), true),
        "freeze" => {
            s.freeze_panes(n(args.first()), n(args.get(1)));
            let (c, r) = s.frozen_panes();
            (format!("{{\"cols\":{c},\"rows\":{r}}}"), true)
        }
        "gridlines" => {
            s.set_show_gridlines(flag(a0));
            (s.show_gridlines().to_string(), true)
        }
        "col_width" => {
            s.set_column_width(n(args.first()).ok_or("bad width")?, lines_arg(args.get(1)));
            (dash, true)
        }
        "row_height" => {
            s.set_row_height(n(args.first()).ok_or("bad height")?, lines_arg(args.get(1)));
            (dash, true)
        }
        "autofit_cols" => {
            s.autofit_columns(lines_arg(args.first()));
            (dash, true)
        }
        "autofit_rows" => {
            s.autofit_rows(lines_arg(args.first()));
            (dash, true)
        }
        "hide" | "unhide" => {
            s.set_hidden(a0 == "cols", op == "hide");
            (dash, true)
        }
        "resize_drag" => {
            let (i, px) = (u(args.get(1)).ok_or("bad index")?, n(args.get(2)).ok_or("bad px")?);
            s.resize_drag(a0 == "col", i, px);
            (dash, true)
        }
        "column_width" => (num(s.column_width(u(args.first()).ok_or("bad col")?)), false),
        "row_height_pt" => (num(s.row_height_pt(u(args.first()).ok_or("bad row")?)), false),
        "line_px" => (num(s.line_px(a0 == "col", u(args.get(1)).ok_or("bad index")?)), false),
        "autosum" => match s.auto_sum(if a0.is_empty() { "SUM" } else { a0 }) {
            AutoSum::Editing(t) => (js16(&t), false),
            AutoSum::Written => ("true".to_owned(), true),
            AutoSum::None => (dash, false),
        },
        "insert_function" => {
            s.insert_function(a0);
            (dash, false)
        }
        "function_names" => (format!("[{}]", s.function_names().iter().map(|x| js(x)).collect::<Vec<_>>().join(",")), false),
        "add_sheet" => {
            let name = s.add_sheet(args.first().copied().filter(|x| !x.is_empty())).map_err(|x| x.to_string())?;
            (js(&name), true)
        }
        "new_workbook" => {
            s.new_workbook().map_err(|x| x.to_string())?;
            (dash, true)
        }
        "recalc" => {
            s.recalculate();
            (dash, true)
        }
        "undo" => (s.undo().to_string(), true),
        "redo" => (s.redo().to_string(), true),
        "set_dirty" => {
            s.set_dirty(flag(a0));
            (dash, false)
        }
        "bar_text" => {
            let sel = s.selection();
            let (c, r) = (u(args.first()).unwrap_or(sel.col), u(args.get(1)).unwrap_or(sel.row));
            let b = s.bar_text(sel.sheet, c, r);
            (format!("{{\"formula\":{},\"edit\":{},\"ghost\":{}}}", js16(&b.formula), js16(&b.edit), b.ghost), false)
        }
        _ => return Err(format!("unknown command {op}")),
    })
}

/// One command (a UTF-8 line `op\targ…`); the answer is JSON (status 0) — `ok:false` carries the error.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` from `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_cmd(h: *mut UoSheetEditor, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(line) = String::from_utf8(bytes) else { return buf(BAD_ARG, Vec::new()) };
    let out = match run(e, &line) {
        Ok((result, changed)) => {
            if changed {
                e.model_gen += 1;
            }
            format!("{{\"ok\":true,\"result\":{result},\"state\":{}}}", state(&mut e.session))
        }
        Err(msg) => format!("{{\"ok\":false,\"error\":{},\"state\":{}}}", js(&msg), state(&mut e.session)),
    };
    buf(OK, out.into_bytes())
}

/// The `.xlsx` of the session (the surgical save; status 13 + the message on failure).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_save(h: *mut UoSheetEditor) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match e.session.save() {
        Ok(b) => buf(OK, b),
        Err(x) => buf(SAVE, x.to_string().into_bytes()),
    }
}

/// The accent colour (CSS, UTF-8) of the selection and the headers.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` a UTF-8 text of `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_set_accent(h: *mut UoSheetEditor, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return NULL_HANDLE };
    match String::from_utf8(bytes) {
        Ok(t) => {
            e.accent = t;
            OK
        }
        Err(_) => BAD_ARG,
    }
}

/// The viewer shows the current model: rebuilt after a change (the measurement table is kept), the sheet activated.
fn sync_viewer(e: &mut UoSheetEditor, vi: usize) -> Result<usize, i32> {
    if e.viewer_gen != e.model_gen {
        let measure = std::mem::replace(&mut e.viewer.measure, MeasureTable::new());
        e.viewer = Viewer::new(&e.session.book, 1, e.clock);
        e.viewer.measure = measure;
        e.viewer_gen = e.model_gen;
    }
    let si = e.session.views.get(vi).map(|v| v.sheet).ok_or(NO_SHEET)?;
    match e.viewer.activate(&mut e.session.book, si, &IdentityEnv) {
        Ok(()) => Ok(si),
        Err(ViewError::NeedMeasures) => Err(NEED),
        Err(_) => Err(NO_SHEET),
    }
}

/// One frame of view `sheet` (the session's sheet index): the message bytes (status 0) or [`NEED`] (answer through
/// `uo_sheet_edit_needs`/`uo_sheet_edit_font`/`uo_sheet_edit_provide` and call again). The selection is six scalars `col, row,
/// c1, r1, c2, r2` (1-based; `uo_sheet_viewport` takes a pointer to them, which the module never releases: no allocation
/// here); `flags` bit 0 = capture; `edit_col`/`edit_row` the cell under the editor (0 = none).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_sheet_edit_viewport(
    h: *mut UoSheetEditor,
    sheet: u32,
    width: f64,
    height: f64,
    dpr: f64,
    scroll_x: f64,
    scroll_y: f64,
    zoom: f64,
    col: u32,
    row: u32,
    c1: u32,
    r1: u32,
    c2: u32,
    r2: u32,
    flags: u32,
    edit_col: u32,
    edit_row: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let selection = [col, row, c1, r1, c2, r2];
    let si = match sync_viewer(e, sheet as usize) {
        Ok(si) => si,
        Err(code) => return buf(code, Vec::new()),
    };
    // the spill range of the selection's formula, from the engine
    let spill = e.session.book.engine.as_ref().and_then(|eng| {
        let ix = uo_calc::SheetIx(si as u32);
        let (r0, c0) = (selection[1].checked_sub(1)?, selection[0].checked_sub(1)?);
        let anchor = if eng.has_formula(ix, r0, c0) { (ix, r0, c0) } else { eng.owner(ix, r0, c0)? };
        let a = eng.spill_of(anchor.0, anchor.1, anchor.2)?;
        Some([a.c1 + 1, a.r1 + 1, a.c2 + 1, a.r2 + 1])
    });
    let p = ViewParams {
        req: ViewportRequest { sheet: si as u32, width, height, dpr, scroll_x, scroll_y, zoom, selection },
        accent: e.accent.clone(),
        capture: flags & 1 != 0,
        editing: (edit_col > 0 && edit_row > 0).then_some((edit_col, edit_row)),
        spill,
        mirrored: false,
    };
    let mut out = Vec::new();
    match e.viewer.emit(&e.session.book, &p, &IdentityEnv, &mut out) {
        Ok(()) => buf(OK, out),
        Err(ViewError::NeedMeasures) => buf(NEED, Vec::new()),
        Err(ViewError::NoSheet) => buf(NO_SHEET, Vec::new()),
        Err(ViewError::Encode(_)) => buf(ENCODE, Vec::new()),
    }
}

/// The extent of view `sheet` at zoom 1: `width f64, height f64` (activates it first; status [`NEED`] as the viewport).
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_extent(h: *mut UoSheetEditor, sheet: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    if let Err(code) = sync_viewer(e, sheet as usize) {
        return buf(code, Vec::new());
    }
    let Some((w, hh)) = e.viewer.extent() else { return buf(NO_SHEET, Vec::new()) };
    let mut o = Out::default();
    o.f64(w).f64(hh);
    buf(OK, o.0)
}

fn put_units(o: &mut Out, u: &[u16]) {
    o.u32(u.len());
    for x in u {
        o.0.extend_from_slice(&x.to_le_bytes());
    }
}

/// What the shell must measure (after a [`NEED`]); the layout of `uo_sheet_needs`.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_needs(h: *mut UoSheetEditor) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let n = e.viewer.measure.take_needs();
    let mut o = Out::default();
    o.u32(n.texts.len());
    for (f, t) in &n.texts {
        o.u32(*f as usize);
        put_units(&mut o, t);
    }
    o.u32(n.font_boxes.len());
    for f in &n.font_boxes {
        o.u32(*f as usize);
    }
    buf(OK, o.0)
}

/// A font of the measurement table; the layout of `uo_sheet_font`.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_font(h: *mut UoSheetEditor, id: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(d) = e.viewer.measure.desc(FontId(id)) else { return buf(BAD_ARG, Vec::new()) };
    let mut o = Out::default();
    o.f64(d.size_pt).f64(d.zoom).u32(d.ppem as u32 as usize).u8(u8::from(d.bold)).u8(u8::from(d.italic));
    o.u32(d.name.len()).bytes(d.name.as_bytes());
    match &d.alt_name {
        Some(a) => {
            o.u32(a.len()).bytes(a.as_bytes());
        }
        None => {
            o.0.extend_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    o.u32(d.metric.len()).bytes(d.metric.as_bytes());
    buf(OK, o.0)
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.i..self.i + N)?;
        self.i += N;
        s.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take::<2>().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take::<4>().map(i32::from_le_bytes)
    }
    fn f64(&mut self) -> Option<f64> {
        self.take::<8>().map(f64::from_le_bytes)
    }
}

/// The shell's answers (the layout of `uo_sheet_provide`). 0 ok, 5 malformed.
///
/// # Safety
/// `h` must be null or a live handle; `ptr`/`len` from `uo_alloc_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_provide(h: *mut UoSheetEditor, ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: as documented.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return NULL_HANDLE };
    let m = &mut e.viewer.measure;
    let mut r = Reader { b: &bytes, i: 0 };
    while r.i < bytes.len() {
        let Some(kind) = r.u8() else { return BAD_ARG };
        let ok = match kind {
            0 => (|| {
                let (f, cp, px) = (r.u32()?, r.u32()?, r.i32()?);
                m.provide_advance(FontId(f), cp, px);
                Some(())
            })(),
            1 => (|| {
                let f = r.u32()?;
                let n = r.u32()? as usize;
                let mut u = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    u.push(r.u16()?);
                }
                let px = r.i32()?;
                m.provide_text(FontId(f), &u, px);
                Some(())
            })(),
            2 => (|| {
                let f = r.u32()?;
                let (a, d) = (r.f64()?, r.f64()?);
                m.provide_box(FontId(f), (!a.is_nan()).then_some(a), (!d.is_nan()).then_some(d));
                Some(())
            })(),
            _ => None,
        };
        if ok.is_none() {
            return BAD_ARG;
        }
    }
    OK
}

/// The RGBA bytes of a bitmap handle of the stream.
///
/// # Safety
/// `h` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_sheet_edit_bitmap(h: *mut UoSheetEditor, handle: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or a live handle.
    let Some(e) = (unsafe { ed(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match e.viewer.bitmap(handle) {
        Some(b) => buf(OK, b.to_vec()),
        None => buf(BAD_ARG, Vec::new()),
    }
}

/// Live bytes of the module's allocator (memory gates; the `count` feature; `global_stats().1`, the LIVE counter).
#[cfg(feature = "count")]
#[unsafe(no_mangle)]
pub extern "C" fn uo_sheet_edit_live_bytes() -> u64 {
    uo_alloc::global_stats().1
}
