//! A cell of the store as the text layout reads it: the TS `cellTextInput` (`universal-sheet.ts:4274-4310`) over
//! the lanes of the store, and `displayText`/`formatCell` (the text in a "wide" column: no `####`, `*x` once).
//!
//! The value is read as the FILE MODEL says it (type + `<v>` text), not as the calc sees it: an unrecognised
//! `<v>` of a numeric cell (`abc`, `Infinity`, `0x10`) is text/number by `Number(v.trim())` exactly as the TS does.
//! A shared string must be decoded in the store's table first ([`uo_calc::Store::resolve_sst`]); until it is the
//! cell reads as [`CellRead::NeedSst`].

use super::entry::{FormulaLang, error_text};
use super::text::{CellKind, CellTextInput, MAX_CELL_CHARS, iso_to_serial, localize_bool};
use uo_calc::cell::FileCell;
use uo_calc::key::SheetIx;
use uo_calc::store::{CellKind as LaneKind, NeedSst, RawCell, Store, ValueTag};
use uo_calc::strings::JsStr;
use uo_calc::value::Scalar;
use uo_numfmt::{FormatLocale, FormatOptions, Value, format_value};

/// The workbook/sheet settings the display of a cell depends on.
#[derive(Clone, Copy, Debug)]
pub struct CellCtx<'a> {
    /// 1904 date system.
    pub date1904: bool,
    /// `showZeros` (the sheet view shows zeros).
    pub show_zeros: bool,
    /// Language of the booleans and errors (the formula language of the element).
    pub lang: FormulaLang,
    /// Format locale (`None` = pt-BR, as the TS).
    pub locale: Option<&'a FormatLocale>,
}

/// The outcome of reading a cell for display.
#[derive(Debug)]
pub enum CellRead<'a> {
    /// No displayed value (empty cell, `""`).
    Empty,
    /// The input of the layout.
    Input(CellTextInput<'a>),
    /// A shared string is not decoded yet.
    NeedSst(u32),
}

fn fill(scratch: &mut Vec<u16>, s: JsStr<'_>) {
    scratch.clear();
    scratch.extend(s.units());
}

/// The `CellTextInput` of the cell at (0-based `row`, `col`) whose lanes are `cell` (`cellTextInput`). `code` is the
/// number-format code of the cell (`numCode ?? cellFormatCode`), `scratch` holds the text (reused across cells).
#[allow(clippy::too_many_arguments)]
pub fn text_input<'a>(
    store: &Store,
    sheet: SheetIx,
    row: u32,
    col: u32,
    cell: RawCell,
    code: &'a [u16],
    ctx: &CellCtx<'a>,
    scratch: &'a mut Vec<u16>,
) -> CellRead<'a> {
    let has_v = cell.kind.has(LaneKind::HAS_VALUE);
    let cap = |s: &mut Vec<u16>| s.truncate(MAX_CELL_CHARS);
    let base = CellTextInput { date1904: ctx.date1904, show_zeros: ctx.show_zeros, locale: ctx.locale, ..CellTextInput::default() };
    let number = |n: f64| CellTextInput { kind: CellKind::Number, num: n, code: Some(code), ..base };
    match cell.kind.tag() {
        ValueTag::Sst => {
            let Ok(v) = store.cell_value(sheet, row, col) else {
                // `cell_value` says which index it needs
                return match store.cell_value(sheet, row, col) {
                    Err(NeedSst(i)) => CellRead::NeedSst(i),
                    _ => CellRead::Empty,
                };
            };
            match v {
                Scalar::Str(r) => text_of(store, r, scratch, |s| CellTextInput { kind: CellKind::Text, str: s, code: Some(code), ..base }),
                _ => CellRead::Empty,
            }
        }
        ValueTag::Inline => {
            let id = cell.str_id();
            match id {
                Some(id) => fill(scratch, store.strings.get(id)),
                None => scratch.clear(),
            }
            if scratch.is_empty()
                && cell.kind.has(LaneKind::HAS_EXTRA)
                && let Some(raw) = store.extra(sheet, row, col).and_then(|e| e.raw_value.as_ref())
            {
                fill(scratch, raw.as_js());
            }
            if scratch.is_empty() {
                return CellRead::Empty;
            }
            cap(scratch);
            CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, code: Some(code), ..base })
        }
        ValueTag::Str => {
            if !has_v {
                return CellRead::Empty;
            }
            match cell.str_id() {
                Some(id) => fill(scratch, store.strings.get(id)),
                None => scratch.clear(),
            }
            if scratch.is_empty() {
                return CellRead::Empty;
            }
            cap(scratch);
            CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, code: Some(code), ..base })
        }
        ValueTag::Bool => {
            if !has_v {
                return CellRead::Empty;
            }
            let t = if ctx.lang == FormulaLang::PtBr {
                localize_bool(cell.payload == 1)
            } else if cell.payload == 1 {
                "TRUE"
            } else {
                "FALSE"
            };
            scratch.clear();
            scratch.extend(t.encode_utf16());
            CellRead::Input(CellTextInput { kind: CellKind::Bool, str: scratch, ..base })
        }
        ValueTag::Err => {
            if !has_v {
                return CellRead::Empty;
            }
            let Ok(Scalar::Err(c)) = store.cell_value(sheet, row, col) else { return CellRead::Empty };
            scratch.clear();
            scratch.extend(error_text(c, ctx.lang).encode_utf16());
            CellRead::Input(CellTextInput { kind: CellKind::Error, str: scratch, ..base })
        }
        ValueTag::Date => {
            let id = if has_v { cell.str_id() } else { None };
            let Some(id) = id else { return CellRead::Empty };
            fill(scratch, store.strings.get(id));
            if scratch.is_empty() {
                return CellRead::Empty;
            }
            match iso_to_serial(Some(scratch), ctx.date1904) {
                Some(n) => CellRead::Input(number(n)),
                None => CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, ..base }),
            }
        }
        ValueTag::None | ValueTag::N | ValueTag::Other => {
            if !has_v {
                return CellRead::Empty;
            }
            // a non-canonical `<v>` is kept as text in the extra: `Number(v.trim())` decides
            if cell.kind.has(LaneKind::HAS_EXTRA)
                && let Some(raw) = store.extra(sheet, row, col).and_then(|e| e.raw_value.as_ref())
            {
                fill(scratch, raw.as_js());
                let t = uo_text::space::trim(scratch);
                if t.is_empty() {
                    return CellRead::Empty;
                }
                let n = uo_jsnum::parse::to_number(&String::from_utf16_lossy(t));
                if n.is_finite() {
                    return CellRead::Input(number(n));
                }
                let (a, b) = (scratch.len() - uo_text::space::trim_start(scratch).len(), t.len());
                scratch.drain(..a);
                scratch.truncate(b);
                cap(scratch);
                return CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, ..base });
            }
            let n = cell.number();
            if n.is_finite() {
                CellRead::Input(number(n))
            } else {
                // `Number(v)` is not finite (`Infinity`): the cell shows the `<v>` text, which for a canonical value is
                // the JS print of the number
                scratch.clear();
                scratch.extend(uo_jsnum::format::to_js_string(n).encode_utf16());
                CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, ..base })
            }
        }
    }
}

fn text_of<'a>(
    store: &Store,
    r: uo_calc::value::StrRef,
    scratch: &'a mut Vec<u16>,
    mk: impl FnOnce(&'a [u16]) -> CellTextInput<'a>,
) -> CellRead<'a> {
    let uo_calc::value::StrRef::Pooled(id) = r else { return CellRead::Empty };
    fill(scratch, store.strings.get(id));
    if scratch.is_empty() {
        return CellRead::Empty;
    }
    scratch.truncate(MAX_CELL_CHARS);
    CellRead::Input(mk(scratch))
}

/// The `CellTextInput` of a cell as the file MODEL holds it (a shadowed duplicate of the file order, which has no
/// lanes): `cellTextInput` over `type` and `<v>`.
pub fn file_text_input<'a>(store: &Store, fc: &FileCell, code: &'a [u16], ctx: &CellCtx<'a>, scratch: &'a mut Vec<u16>) -> CellRead<'a> {
    let base = CellTextInput { date1904: ctx.date1904, show_zeros: ctx.show_zeros, locale: ctx.locale, ..CellTextInput::default() };
    let v_units = |fc: &FileCell, out: &mut Vec<u16>| {
        out.clear();
        if let Some(v) = &fc.v {
            out.extend(v.as_js().units());
        }
    };
    let t = fc.t.as_ref().map(|t| t.as_js().to_string_lossy());
    let text = |scratch: &'a mut Vec<u16>| -> CellRead<'a> {
        if scratch.is_empty() {
            return CellRead::Empty;
        }
        scratch.truncate(MAX_CELL_CHARS);
        CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, code: Some(code), ..base })
    };
    match t.as_deref() {
        Some("s") => {
            let Some(v) = &fc.v else { return CellRead::Empty };
            let i = uo_jsnum::parse::parse_int(&v.as_js().to_string_lossy(), 10);
            let Some(i) = uo_jsnum::conv::js_index(i, store.sst.len()) else { return CellRead::Empty };
            match store.sst_text(u32::try_from(i).unwrap_or(0)) {
                Some(id) => fill(scratch, store.strings.get(id)),
                None => return CellRead::NeedSst(u32::try_from(i).unwrap_or(0)),
            }
            text(scratch)
        }
        Some("inlineStr") => {
            match fc.is.as_ref().and_then(|i| i.text.as_ref()) {
                Some(t) => fill(scratch, t.as_js()),
                None => v_units(fc, scratch),
            }
            text(scratch)
        }
        Some("str") => {
            v_units(fc, scratch);
            text(scratch)
        }
        Some("b") => {
            let Some(v) = &fc.v else { return CellRead::Empty };
            let on = v.as_js().eq_str("1") || v.as_js().eq_str("true");
            let t = if ctx.lang == FormulaLang::PtBr {
                localize_bool(on)
            } else if on {
                "TRUE"
            } else {
                "FALSE"
            };
            scratch.clear();
            scratch.extend(t.encode_utf16());
            CellRead::Input(CellTextInput { kind: CellKind::Bool, str: scratch, ..base })
        }
        Some("e") => {
            v_units(fc, scratch);
            if scratch.is_empty() {
                return CellRead::Empty;
            }
            let c = uo_calc::cell::error_of(JsStr::Utf16(scratch));
            scratch.clear();
            scratch.extend(error_text(c, ctx.lang).encode_utf16());
            CellRead::Input(CellTextInput { kind: CellKind::Error, str: scratch, ..base })
        }
        Some("d") => {
            v_units(fc, scratch);
            if scratch.is_empty() {
                return CellRead::Empty;
            }
            match iso_to_serial(Some(scratch), ctx.date1904) {
                Some(n) => CellRead::Input(CellTextInput { kind: CellKind::Number, num: n, code: Some(code), ..base }),
                None => CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, ..base }),
            }
        }
        _ => {
            v_units(fc, scratch);
            let tr = uo_text::space::trim(scratch);
            if tr.is_empty() {
                return CellRead::Empty;
            }
            let n = uo_jsnum::parse::to_number(&String::from_utf16_lossy(tr));
            if n.is_finite() {
                return CellRead::Input(CellTextInput { kind: CellKind::Number, num: n, code: Some(code), ..base });
            }
            let (a, b) = (scratch.len() - uo_text::space::trim_start(scratch).len(), tr.len());
            scratch.drain(..a);
            scratch.truncate(b);
            scratch.truncate(MAX_CELL_CHARS);
            CellRead::Input(CellTextInput { kind: CellKind::Text, str: scratch, ..base })
        }
    }
}

/// `displayText`/`formatCell`: the text of an input in a column wide enough (no `####`, `*x` once).
#[must_use]
pub fn display_text(inp: &CellTextInput<'_>) -> Vec<u16> {
    let opts = FormatOptions { locale: inp.locale, col_width_chars: None, date1904: inp.date1904, show_zeros: inp.show_zeros };
    match inp.kind {
        CellKind::Number => format_value(Value::Num(inp.num), inp.code, &opts).text,
        CellKind::Text => match inp.code {
            Some(c) if !c.is_empty() => {
                format_value(Value::Str(inp.str), Some(c), &FormatOptions { show_zeros: true, date1904: false, ..opts }).text
            }
            _ => inp.str.to_vec(),
        },
        CellKind::Bool | CellKind::Error => inp.str.to_vec(),
    }
}
