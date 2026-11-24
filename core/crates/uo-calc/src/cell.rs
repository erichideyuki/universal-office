//! A cell as the file and the calc see it: the sheet-core `Cell` model (`model.ts:124-143`) ↔ the store's
//! lanes, `cellValue`/`errorOf` (`from-sheet-core.ts:67-101`), `numberLiteral` (`print.ts:24-27`),
//! `fileCellValue`/`numberText` (`to-sheet-core.ts:38-55`) and the product write path `writeScalar` +
//! `writeBack`'s create/remove rule (`sheet-calc.ts:230-262, 461-471`).
//!
//! Canonical forms live in the lanes, anything else in [`CellExtra`]: a number whose `<v>` is
//! `numberLiteral(Number(v))`, a boolean `1`/`0`, an error code as written, a shared-string index as
//! `String(i)`, an inline string `<is>{text}</is>`, an `s` that is a `u32`, an `r` equal to the A1 of the
//! position.

use crate::error::{CalcError, Result};
use crate::formula::{FileFormula, FormulaAttrs};
use crate::key::SheetIx;
use crate::store::{CellExtra, CellKind, InlineExtra, NeedSst, RawCell, RichRun, Store, ValueTag, ts_key};
use crate::strings::{JsStr, JsString, StrId, StrPool};
use crate::value::{ErrorCode, Scalar, StrRef};

/// The `<is>` of a cell (`SharedString`, `model.ts:116-121`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineText {
    /// `text`.
    pub text: Option<JsString>,
    /// `runs`.
    pub runs: Option<Vec<RichRun>>,
}

impl InlineText {
    /// `textOf` (`from-sheet-core.ts:65`): `text ?? runs.join('') ?? ''`.
    #[must_use]
    pub fn text_of(&self) -> JsString {
        if let Some(t) = &self.text {
            return t.clone();
        }
        match &self.runs {
            Some(runs) => {
                let units: Vec<u16> = runs.iter().flat_map(|r| r.text.as_js().units()).collect();
                JsString::from_utf16(&units)
            }
            None => JsString::default(),
        }
    }
}

/// One `<c>` as the sheet-core model holds it (owned): what a reader hands to the store and what the save and
/// the facet read back. Numbers are the JS numbers `parseInt` produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileCell {
    /// `ref` (the `r` attribute, or the reader's canonical one).
    pub r#ref: Option<JsString>,
    /// `type` (`t`).
    pub t: Option<JsString>,
    /// `value` (`<v>`).
    pub v: Option<JsString>,
    /// `style` (`s`).
    pub s: Option<f64>,
    /// `formula` (`<f>` text).
    pub f: Option<JsString>,
    /// `formulaAttrs`.
    pub fa: Option<FormulaAttrs>,
    /// `cm`.
    pub cm: Option<f64>,
    /// `vm`.
    pub vm: Option<f64>,
    /// `inlineString` (`<is>`).
    pub is: Option<InlineText>,
}

// ---------------------------------------------------------------- numbers and errors as the TS prints them

/// `numberLiteral` (`print.ts:24-27`): `String(v)` with `E` and a `+` after it before a digit.
#[must_use]
pub fn number_literal(x: f64) -> String {
    let s = uo_jsnum::format::to_js_string(x);
    if !s.contains('e') {
        return s;
    }
    let s = s.replacen('e', "E", 1);
    // `.replace(/E(\d)/, 'E+$1')`: the first `E` followed by a digit
    match s.find('E') {
        Some(i) if s.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit) => format!("{}E+{}", &s[..i], &s[i + 1..]),
        _ => s,
    }
}

/// The number of a `<v>` text when the text is its canonical print (`numberLiteral(Number(v)) === v`): such a
/// cell needs no extra (the reader's fast path, [`crate::load::SheetLoader::cell_raw`]).
#[must_use]
pub fn canonical_number(v: &str) -> Option<f64> {
    let b = v.as_bytes();
    // plain integers: no sign trouble, no leading zero, exact below 2^53
    let digits = b.strip_prefix(b"-").unwrap_or(b);
    if !digits.is_empty()
        && digits.len() <= 15
        && digits.iter().all(u8::is_ascii_digit)
        && (digits[0] != b'0' || digits.len() == 1)
        && v != "-0"
    {
        let n = digits.iter().fold(0i64, |a, &d| a * 10 + i64::from(d - b'0'));
        return Some(if b[0] == b'-' { -(n as f64) } else { n as f64 });
    }
    if v.is_empty() {
        return None;
    }
    let x = uo_jsnum::parse::to_number(v);
    (number_literal(x) == v).then_some(x)
}

/// `numberText` (`to-sheet-core.ts:47-52`): non-finite and `-0` → `"0"`, else `String(v)` with `E`.
#[must_use]
pub fn number_text(x: f64) -> String {
    if !x.is_finite() || (x == 0.0 && x.is_sign_negative()) {
        return "0".to_owned();
    }
    uo_jsnum::format::to_js_string(x).replacen('e', "E", 1)
}

/// The text of an error code (`#DIV/0!`…).
#[must_use]
pub const fn error_text(e: ErrorCode) -> &'static str {
    match e {
        ErrorCode::Null => "#NULL!",
        ErrorCode::Div0 => "#DIV/0!",
        ErrorCode::Value => "#VALUE!",
        ErrorCode::Ref => "#REF!",
        ErrorCode::Name => "#NAME?",
        ErrorCode::Num => "#NUM!",
        ErrorCode::NA => "#N/A",
        ErrorCode::GettingData => "#GETTING_DATA",
        ErrorCode::Spill => "#SPILL!",
        ErrorCode::Connect => "#CONNECT!",
        ErrorCode::Blocked => "#BLOCKED!",
        ErrorCode::Unknown => "#UNKNOWN!",
        ErrorCode::Field => "#FIELD!",
        ErrorCode::Calc => "#CALC!",
        ErrorCode::Busy => "#BUSY!",
        ErrorCode::Python => "#PYTHON!",
    }
}

/// Every error code in `ERROR.TYPE` order.
pub const ERROR_CODES: [ErrorCode; 16] = [
    ErrorCode::Null,
    ErrorCode::Div0,
    ErrorCode::Value,
    ErrorCode::Ref,
    ErrorCode::Name,
    ErrorCode::Num,
    ErrorCode::NA,
    ErrorCode::GettingData,
    ErrorCode::Spill,
    ErrorCode::Connect,
    ErrorCode::Blocked,
    ErrorCode::Unknown,
    ErrorCode::Field,
    ErrorCode::Calc,
    ErrorCode::Busy,
    ErrorCode::Python,
];

/// The code from its exact text.
#[must_use]
pub fn error_from_text(s: JsStr<'_>) -> Option<ErrorCode> {
    ERROR_CODES.into_iter().find(|&e| s.eq_str(error_text(e)))
}

fn error_from_u8(n: u8) -> ErrorCode {
    ERROR_CODES.get(usize::from(n).wrapping_sub(1)).copied().unwrap_or(ErrorCode::Value)
}

/// `errorOf` (`from-sheet-core.ts:68-71`): `code.trim().toUpperCase()` in the table, else `#VALUE!`.
#[must_use]
pub fn error_of(s: JsStr<'_>) -> ErrorCode {
    let units = s.to_utf16();
    let a = units.iter().position(|&u| !uo_jsnum::text::is_js_space_unit(u)).unwrap_or(units.len());
    let b = units.iter().rposition(|&u| !uo_jsnum::text::is_js_space_unit(u)).map_or(a, |i| i + 1);
    let upper = uo_text::case::to_uppercase(units.get(a..b).unwrap_or(&[]));
    error_from_text(JsStr::Utf16(&upper)).unwrap_or(ErrorCode::Value)
}

/// `Number(s)` of a JS string.
#[must_use]
pub fn js_number(s: JsStr<'_>) -> f64 {
    match s {
        JsStr::Latin1(b) if b.is_ascii() => uo_jsnum::parse::to_number(core::str::from_utf8(b).unwrap_or("x")),
        _ => uo_jsnum::parse::to_number(&s.to_string_lossy()),
    }
}

/// The value of `fileCellValue` (`to-sheet-core.ts:38-44`): `t` and the `<v>` text.
#[derive(Clone, Debug, PartialEq)]
pub struct FileValue {
    /// `type`: `str`, `b`, `e` or none (number).
    pub t: Option<&'static str>,
    /// `value`.
    pub v: JsString,
}

/// `fileCellValue`: an engine value → `t` and `<v>` as Excel writes a formula/spill cell (`writeValues` path).
/// `text` resolves the strings that are not in the pool (the evaluator's temporaries and formula literals).
#[must_use]
pub fn file_cell_value(v: &Scalar, pool: &StrPool, text: &dyn Fn(StrRef) -> JsString) -> FileValue {
    match v {
        Scalar::Num(x) => FileValue { t: None, v: JsString::from_str_js(&number_text(*x)) },
        Scalar::Str(StrRef::Pooled(id)) => FileValue { t: Some("str"), v: pool.get(*id).to_owned_js() },
        Scalar::Str(r @ (StrRef::Temp(_) | StrRef::Lit(_))) => FileValue { t: Some("str"), v: text(*r) },
        Scalar::Bool(b) => FileValue { t: Some("b"), v: JsString::from_str_js(if *b { "1" } else { "0" }) },
        Scalar::Err(e) => FileValue { t: Some("e"), v: JsString::from_str_js(error_text(*e)) },
        Scalar::Empty => FileValue { t: None, v: JsString::from_str_js("0") },
    }
}

/// `rowColToRef` (`model.ts:541-550`) of a 0-based position: `A1`.
#[must_use]
pub fn a1_ref(row: u32, col: u32) -> String {
    let mut letters = [0u8; 8];
    let mut n = 0;
    let mut c = col + 1;
    while c > 0 {
        let rem = (c - 1) % 26;
        letters[n] = b'A' + rem as u8;
        n += 1;
        c = (c - 1) / 26;
    }
    let mut s = String::with_capacity(n + 7);
    for &l in letters[..n].iter().rev() {
        s.push(char::from(l));
    }
    s.push_str(&(u64::from(row) + 1).to_string());
    s
}

// ---------------------------------------------------------------- dates (`t="d"`)

/// Days from 1970-01-01 of a proleptic Gregorian date (`Date.UTC`, exact for any year).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `ymdToSerial` (`dates.ts:35-46`) for the integer fields `cellValue` passes (4-digit year, 2-digit month and day).
/// Port kept local to S0 (A1 owns `dates`; both are the same arithmetic).
fn ymd_to_serial(y: i64, m: i64, d: i64, date1904: bool) -> f64 {
    let yy = y + (m - 1).div_euclid(12);
    let mm = (m - 1).rem_euclid(12);
    // `Date.UTC(yy, mm, 1)` (years 0–99 corrected back by `setUTCFullYear`): whole days since the epoch
    let days = days_from_civil(yy, mm + 1, 1);
    if date1904 {
        return (days - days_from_civil(1904, 1, 1) + d - 1) as f64;
    }
    let mut s = days - days_from_civil(1899, 12, 31);
    if s >= 60 {
        s += 1;
    }
    (s + d - 1) as f64
}

/// `cellValue` of `t="d"`: `/^(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2})(?::(\d{2}(?:\.\d+)?))?)?/`.
fn date_value(s: JsStr<'_>, date1904: bool) -> Option<f64> {
    let u: Vec<u16> = s.units().collect();
    let digit = |i: usize| u.get(i).filter(|&&c| (0x30..=0x39).contains(&c)).map(|&c| i64::from(c - 0x30));
    let num = |a: usize, n: usize| -> Option<i64> { (a..a + n).try_fold(0i64, |acc, i| Some(acc * 10 + digit(i)?)) };
    let y = num(0, 4)?;
    (u.get(4) == Some(&0x2D)).then_some(())?;
    let m = num(5, 2)?;
    (u.get(7) == Some(&0x2D)).then_some(())?;
    let d = num(8, 2)?;
    let mut secs = 0.0;
    // the optional time group: all of `Thh:mm` or nothing
    if u.get(10) == Some(&0x54)
        && let (Some(h), Some(mi), true) = (num(11, 2), num(14, 2), u.get(13) == Some(&0x3A))
    {
        let mut sec = 0.0;
        if u.get(16) == Some(&0x3A) && num(17, 2).is_some() {
            // `\d{2}(?:\.\d+)?`, read by unary `+`
            let mut end = 19;
            if u.get(19) == Some(&0x2E) && digit(20).is_some() {
                end = 21;
                while digit(end).is_some() {
                    end += 1;
                }
            }
            let text: String = u[17..end].iter().map(|&c| char::from(c as u8)).collect();
            sec = uo_jsnum::parse::to_number(&text);
        }
        secs = (h * 3600 + mi * 60) as f64 + sec;
    }
    Some(ymd_to_serial(y, m, d, date1904) + secs / 86400.0)
}

// ---------------------------------------------------------------- encode / decode

fn intern(pool: &mut StrPool, s: JsStr<'_>) -> Result<StrId> {
    pool.intern(s)
}

impl Store {
    /// Writes a cell as the reader found it (the sheet-core model), replacing any cell at `(row, col)`
    /// (0-based). Interns its strings, keeps every non-canonical detail in the extra.
    pub fn set_file_cell(&mut self, sheet: SheetIx, row: u32, col: u32, cell: &FileCell) -> Result<()> {
        if self.sheet(sheet).is_none() {
            return Err(CalcError::NoSheet(sheet.0));
        }
        if self.get(sheet, row, col).is_some() {
            self.remove(sheet, row, col);
        }
        let mut extra = CellExtra::default();
        let tag = match cell.t.as_ref().map(JsString::as_js) {
            None => ValueTag::None,
            Some(t) if t.eq_str("n") => ValueTag::N,
            Some(t) if t.eq_str("b") => ValueTag::Bool,
            Some(t) if t.eq_str("e") => ValueTag::Err,
            Some(t) if t.eq_str("s") => ValueTag::Sst,
            Some(t) if t.eq_str("inlineStr") => ValueTag::Inline,
            Some(t) if t.eq_str("str") => ValueTag::Str,
            Some(t) if t.eq_str("d") => ValueTag::Date,
            Some(t) => {
                extra.raw_type = Some(t.to_owned_js());
                ValueTag::Other
            }
        };
        let mut flags = 0u8;
        let v = cell.v.as_ref().map(JsString::as_js);
        if v.is_some() {
            flags |= CellKind::HAS_VALUE;
        }
        let mut payload = 0u64;
        match (tag, v) {
            (_, None) if tag != ValueTag::Inline => {}
            (ValueTag::None | ValueTag::N | ValueTag::Other, Some(v)) => {
                let x = if v.is_empty() { f64::NAN } else { js_number(v) };
                payload = x.to_bits();
                if v.is_empty() || !v.eq_str(&number_literal(x)) {
                    extra.raw_value = Some(v.to_owned_js());
                }
            }
            (ValueTag::Bool, Some(v)) => {
                payload = u64::from(v.eq_str("1") || v.eq_str("true"));
                if !v.eq_str("1") && !v.eq_str("0") {
                    extra.raw_value = Some(v.to_owned_js());
                }
            }
            (ValueTag::Err, Some(v)) => {
                let e = error_of(v);
                payload = u64::from(e as u8);
                if !v.eq_str(error_text(e)) {
                    extra.raw_value = Some(v.to_owned_js());
                }
            }
            (ValueTag::Sst, Some(v)) => {
                let x = js_number(v);
                let canon = uo_jsnum::conv::is_integer(x) && (0.0..4_294_967_296.0).contains(&x) && !(x == 0.0 && x.is_sign_negative());
                if canon && v.eq_str(&uo_jsnum::format::to_js_string(x)) {
                    payload = u64::from(uo_jsnum::conv::to_uint32(x));
                } else {
                    extra.raw_value = Some(v.to_owned_js());
                }
            }
            // an inline cell's `<v>` has no lane
            (ValueTag::Inline, Some(v)) => extra.raw_value = Some(v.to_owned_js()),
            (ValueTag::Str | ValueTag::Date, Some(v)) => {
                payload = u64::from(intern(&mut self.strings, v)?.raw());
            }
            _ => {}
        }
        // `<is>`
        let mut rich_runs: Option<Vec<RichRun>> = None;
        if tag == ValueTag::Inline {
            let text = cell.is.as_ref().map_or_else(JsString::default, InlineText::text_of);
            match intern(&mut self.strings, text.as_js()) {
                Ok(id) => payload = u64::from(id.raw()),
                Err(e) => {
                    self.release_payload(tag, flags, payload);
                    return Err(e);
                }
            }
            match &cell.is {
                None => extra.inline = Some(InlineExtra::Absent),
                Some(InlineText { text: Some(_), runs: None }) => {}
                Some(is) => {
                    extra.inline = Some(InlineExtra::Shape { text: is.text.clone(), rich: None });
                    rich_runs.clone_from(&is.runs);
                }
            }
        } else if let Some(is) = &cell.is {
            extra.inline = Some(InlineExtra::Shape { text: is.text.clone(), rich: None });
            rich_runs.clone_from(&is.runs);
        }
        if let Some(runs) = rich_runs {
            let r = self.add_rich(runs);
            if let Some(InlineExtra::Shape { rich, .. }) = &mut extra.inline {
                *rich = Some(r);
            }
        }
        // `s`
        let mut style = 0u32;
        if let Some(s) = cell.s {
            if uo_jsnum::conv::is_integer(s) && (0.0..4_294_967_295.0).contains(&s) && !(s == 0.0 && s.is_sign_negative()) {
                style = uo_jsnum::conv::to_uint32(s) + 1;
            } else {
                extra.raw_style = Some(s);
            }
        }
        // `r`
        if let Some(r) = &cell.r#ref
            && !r.as_js().eq_str(&a1_ref(row, col))
        {
            extra.raw_ref = Some(r.clone());
        }
        extra.cm = cell.cm;
        extra.vm = cell.vm;
        let formula =
            (cell.f.is_some() || cell.fa.is_some()).then(|| FileFormula { text: cell.f.clone(), attrs: cell.fa.clone().map(Box::new) });
        if formula.is_some() {
            flags |= CellKind::HAS_FORMULA;
        }
        let has_extra = !extra.is_empty();
        if has_extra {
            flags |= CellKind::HAS_EXTRA;
        }
        let raw = RawCell { kind: CellKind::new(tag, flags), payload, style };
        if let Err(e) = self.put_raw(sheet, row, col, raw) {
            self.drop_extra(extra);
            return Err(e);
        }
        let k = ts_key(row, col);
        let sh = self.sheet_mut(sheet)?;
        if has_extra {
            sh.extras.insert(k, extra);
        }
        if let Some(f) = formula {
            sh.formulas.set(row, col, f);
        }
        Ok(())
    }

    fn release_payload(&mut self, tag: ValueTag, flags: u8, payload: u64) {
        let raw = RawCell { kind: CellKind::new(tag, flags), payload, style: 0 };
        if let Some(id) = raw.str_id() {
            self.strings.release(id);
        }
    }

    /// The model of a cell (`None` if absent): what the save writes and the store facet prints. `ref` is the
    /// raw one or the canonical A1.
    #[must_use]
    pub fn file_cell(&self, sheet: SheetIx, row: u32, col: u32) -> Option<FileCell> {
        let raw = self.get(sheet, row, col)?;
        let empty = CellExtra::default();
        let extra = if raw.kind.has(CellKind::HAS_EXTRA) { self.extra(sheet, row, col).unwrap_or(&empty) } else { &empty };
        let tag = raw.kind.tag();
        let js = |s: &str| JsString::from_str_js(s);
        let t = match tag {
            ValueTag::None => None,
            ValueTag::N => Some(js("n")),
            ValueTag::Bool => Some(js("b")),
            ValueTag::Err => Some(js("e")),
            ValueTag::Sst => Some(js("s")),
            ValueTag::Inline => Some(js("inlineStr")),
            ValueTag::Str => Some(js("str")),
            ValueTag::Date => Some(js("d")),
            ValueTag::Other => Some(extra.raw_type.clone().unwrap_or_default()),
        };
        let v = if let Some(rv) = &extra.raw_value {
            Some(rv.clone())
        } else if !raw.kind.has(CellKind::HAS_VALUE) || tag == ValueTag::Inline {
            None
        } else {
            Some(match tag {
                ValueTag::None | ValueTag::N | ValueTag::Other => js(&number_literal(raw.number())),
                ValueTag::Bool => js(if raw.payload == 1 { "1" } else { "0" }),
                ValueTag::Err => js(error_text(error_from_u8(raw.payload as u8))),
                ValueTag::Sst => js(&raw.payload.to_string()),
                ValueTag::Inline | ValueTag::Str | ValueTag::Date => {
                    raw.str_id().map(|id| self.strings.get(id).to_owned_js()).unwrap_or_default()
                }
            })
        };
        let s = extra.raw_style.or_else(|| (raw.style != 0).then(|| f64::from(raw.style - 1)));
        let r#ref = Some(extra.raw_ref.clone().unwrap_or_else(|| js(&a1_ref(row, col))));
        let is = match (&extra.inline, tag) {
            (Some(InlineExtra::Absent), _) => None,
            (Some(InlineExtra::Shape { text, rich }), _) => {
                Some(InlineText { text: text.clone(), runs: rich.and_then(|r| self.rich_runs(r)).map(<[RichRun]>::to_vec) })
            }
            (None, ValueTag::Inline) => Some(InlineText { text: raw.str_id().map(|id| self.strings.get(id).to_owned_js()), runs: None }),
            (None, _) => None,
        };
        let (f, fa) = if raw.kind.has(CellKind::HAS_FORMULA) {
            match self.sheet(sheet).and_then(|sh| sh.formulas.get(row, col)) {
                Some((_, ff)) => (ff.text.clone(), ff.attrs.as_deref().cloned()),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        Some(FileCell { r#ref, t, v, s, f, fa, cm: extra.cm, vm: extra.vm, is })
    }

    /// The shared string `i` once decoded (`None`: not decoded yet).
    #[must_use]
    pub fn sst_text(&self, i: u32) -> Option<StrId> {
        self.sst.get(i as usize).copied().flatten().map(|e| e.text)
    }

    /// Declares the shared-string count (entries undecoded).
    pub fn set_sst_len(&mut self, n: u32) {
        for e in self.sst.drain(..).flatten().collect::<Vec<_>>() {
            self.strings.release(e.text);
            if let Some(r) = e.rich {
                self.free_rich(r);
            }
        }
        self.sst = vec![None; n as usize];
    }

    /// Stores a decoded shared string (growing the table if needed).
    pub fn set_sst(&mut self, i: u32, entry: SstDecodedRef<'_>) -> Result<()> {
        let text = self.strings.intern(entry.text)?;
        let rich = entry.runs.map(|r| self.add_rich(r.to_vec()));
        if self.sst.len() <= i as usize {
            self.sst.resize(i as usize + 1, None);
        }
        if let Some(old) = self.sst[i as usize].replace(crate::store::SstEntry { text, rich }) {
            self.strings.release(old.text);
            if let Some(r) = old.rich {
                self.free_rich(r);
            }
        }
        Ok(())
    }

    /// Decodes shared string `i` through the reader's source if it is not decoded yet.
    pub fn resolve_sst(&mut self, i: u32, src: &mut dyn crate::store::SstSource) -> Result<Option<StrId>> {
        if let Some(id) = self.sst_text(i) {
            return Ok(Some(id));
        }
        if i as usize >= self.sst.len() {
            return Ok(None);
        }
        let Some(d) = src.decode(i) else { return Ok(None) };
        self.set_sst(i, SstDecodedRef { text: d.text.as_js(), runs: d.runs.as_deref() })?;
        Ok(self.sst_text(i))
    }

    /// `cellValue` (`from-sheet-core.ts:73-101`) of the cell at `(row, col)`: [`Scalar::Empty`] for no cell,
    /// `Err(NeedSst)` when it reads a shared string that is not decoded yet.
    pub fn cell_value(&self, sheet: SheetIx, row: u32, col: u32) -> core::result::Result<Scalar, NeedSst> {
        let Some(raw) = self.get(sheet, row, col) else { return Ok(Scalar::Empty) };
        let has_v = raw.kind.has(CellKind::HAS_VALUE);
        let raw_value = || -> Option<&JsString> {
            if raw.kind.has(CellKind::HAS_EXTRA) { self.extra(sheet, row, col).and_then(|e| e.raw_value.as_ref()) } else { None }
        };
        let pooled = |id: Option<StrId>| Scalar::Str(StrRef::Pooled(id.unwrap_or(StrPool::EMPTY)));
        Ok(match raw.kind.tag() {
            ValueTag::Sst => {
                if !has_v {
                    return Ok(Scalar::Empty);
                }
                let i = match raw_value() {
                    Some(v) => {
                        let x = js_number(v.as_js());
                        if !uo_jsnum::conv::is_integer(x) {
                            return Ok(Scalar::Empty);
                        }
                        // sst[i] for an integer outside the table (negative, huge) is undefined → ''
                        if !(0.0..4_294_967_296.0).contains(&x) {
                            return Ok(pooled(None));
                        }
                        uo_jsnum::conv::to_uint32(x)
                    }
                    None => raw.payload as u32,
                };
                if i as usize >= self.sst.len() {
                    return Ok(pooled(None));
                }
                match self.sst_text(i) {
                    Some(id) => pooled(Some(id)),
                    None => return Err(NeedSst(i)),
                }
            }
            ValueTag::Str => {
                if has_v {
                    pooled(raw.str_id())
                } else if raw.kind.has(CellKind::HAS_FORMULA)
                    && self.sheet(sheet).and_then(|s| s.formulas.get(row, col)).is_some_and(|(_, f)| f.text.is_some())
                {
                    pooled(None)
                } else {
                    Scalar::Empty
                }
            }
            ValueTag::Inline => pooled(raw.str_id()),
            ValueTag::Bool => {
                if has_v {
                    Scalar::Bool(raw.payload == 1)
                } else {
                    Scalar::Empty
                }
            }
            ValueTag::Err => {
                if has_v {
                    Scalar::Err(error_from_u8(raw.payload as u8))
                } else {
                    Scalar::Empty
                }
            }
            ValueTag::Date => match raw.str_id().map(|id| self.strings.get(id)) {
                Some(s) if has_v && !s.is_empty() => date_value(s, self.date1904).map_or(Scalar::Empty, Scalar::Num),
                _ => Scalar::Empty,
            },
            ValueTag::None | ValueTag::N | ValueTag::Other => {
                let x = raw.number();
                if has_v && x.is_finite() { Scalar::Num(x) } else { Scalar::Empty }
            }
        })
    }

    /// [`Store::cell_value`] decoding a missing shared string through `src`.
    pub fn cell_value_decoding(&mut self, sheet: SheetIx, row: u32, col: u32, src: &mut dyn crate::store::SstSource) -> Result<Scalar> {
        match self.cell_value(sheet, row, col) {
            Ok(v) => Ok(v),
            Err(NeedSst(i)) => {
                self.resolve_sst(i, src)?;
                Ok(self.cell_value(sheet, row, col).unwrap_or(Scalar::Str(StrRef::Pooled(StrPool::EMPTY))))
            }
        }
    }

    /// Writes an engine value with the product semantics (`writeBack` + `writeScalar`, `sheet-calc.ts:230-262,
    /// 461-471`): a number → no `t` (`numberLiteral` at save); a constant text → `inlineStr`; a formula/spill text
    /// → `t="str"`; a boolean → `t="b"`; an error → `t="e"`; empty → no `t`/`<v>`. An empty value on a cell
    /// without formula and not owned strips the content and deletes the cell when it has no style; a missing
    /// cell is created with the row/column default style. Returns whether a cell exists afterwards.
    pub fn write_value(&mut self, sheet: SheetIx, row: u32, col: u32, value: CellWrite<'_>, kind: WriteKind) -> Result<bool> {
        if self.sheet(sheet).is_none() {
            return Err(CalcError::NoSheet(sheet.0));
        }
        let cur = self.get(sheet, row, col);
        if matches!(value, CellWrite::Empty) && cur.is_none_or(|c| !c.kind.has(CellKind::HAS_FORMULA) && !c.kind.has(CellKind::OWNED)) {
            let Some(c) = cur else { return Ok(false) };
            // stripContent: inlineString, value, type
            let keep_style = c.style != 0 || self.extra(sheet, row, col).is_some_and(|e| e.raw_style.is_some());
            if !keep_style {
                self.remove(sheet, row, col);
                return Ok(false);
            }
            self.write_lanes(sheet, row, col, c, ValueTag::None, false, 0)?;
            return Ok(true);
        }
        if let (Some(c), CellWrite::Num(x)) = (cur, value)
            && !c.kind.has(CellKind::HAS_EXTRA)
        {
            // the hot path of a recalc: a number into a cell without file details
            let flags = (c.kind.0 & (CellKind::HAS_FORMULA | CellKind::OWNED)) | CellKind::HAS_VALUE;
            self.put_raw(sheet, row, col, RawCell { kind: CellKind::new(ValueTag::None, flags), payload: x.to_bits(), style: c.style })?;
            return Ok(true);
        }
        let c = match cur {
            Some(c) => c,
            None => {
                let xf = self.default_style(sheet, row, col);
                let raw =
                    RawCell { kind: CellKind::new(ValueTag::None, 0), payload: 0, style: if xf == 0 { 0 } else { xf.saturating_add(1) } };
                self.put_raw(sheet, row, col, raw)?;
                raw
            }
        };
        let (tag, has_v, payload) = match value {
            CellWrite::Num(x) => (ValueTag::None, true, x.to_bits()),
            CellWrite::Pooled(id) => {
                self.strings.retain(id);
                (
                    if kind == WriteKind::Constant { ValueTag::Inline } else { ValueTag::Str },
                    kind != WriteKind::Constant,
                    u64::from(id.raw()),
                )
            }
            CellWrite::Str(s) => {
                let id = self.strings.intern(s)?;
                (
                    if kind == WriteKind::Constant { ValueTag::Inline } else { ValueTag::Str },
                    kind != WriteKind::Constant,
                    u64::from(id.raw()),
                )
            }
            CellWrite::Bool(b) => (ValueTag::Bool, true, u64::from(b)),
            CellWrite::Err(e) => (ValueTag::Err, true, u64::from(e as u8)),
            CellWrite::Empty => (ValueTag::None, false, 0),
        };
        self.write_lanes(sheet, row, col, c, tag, has_v, payload)?;
        Ok(true)
    }

    /// Marks a cell as filled by another cell's array (`CellRec.owner`), creating it with the row/column default
    /// style when absent (the bridge's `createCell(…, xfIndexAt(…))`, `sheet-calc.ts:253`).
    pub fn claim_owned(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<()> {
        if self.sheet(sheet).is_none() {
            return Err(CalcError::NoSheet(sheet.0));
        }
        match self.get(sheet, row, col) {
            Some(c) if c.kind.has(CellKind::OWNED) => Ok(()),
            Some(_) => {
                self.set_owned(sheet, row, col, true);
                Ok(())
            }
            None => {
                let xf = self.default_style(sheet, row, col);
                let style = if xf == 0 { 0 } else { xf.saturating_add(1) };
                self.put_raw(sheet, row, col, RawCell { kind: CellKind::new(ValueTag::None, CellKind::OWNED), payload: 0, style })?;
                Ok(())
            }
        }
    }

    /// Deletes the calc content of a cell (the TS `SheetStore.delete` followed by the bridge's write-back of an
    /// empty value): formula, owner and value go; the cell stays only when it has a style.
    pub fn clear_content(&mut self, sheet: SheetIx, row: u32, col: u32) -> Result<()> {
        let Some(c) = self.get(sheet, row, col) else { return Ok(()) };
        if c.kind.has(CellKind::HAS_FORMULA) {
            if let Some(sh) = self.sheets.get_mut(sheet.0 as usize) {
                sh.formulas.remove(row, col);
            }
            self.set_flags(sheet, row, col, CellKind::HAS_FORMULA, false);
        }
        if c.kind.has(CellKind::OWNED) {
            self.set_owned(sheet, row, col, false);
        }
        self.write_value(sheet, row, col, CellWrite::Empty, WriteKind::Constant)?;
        Ok(())
    }

    /// Gives a cell a formula (created with the default style when absent): `<f>` text and attributes, no `<v>`
    /// (the engine's `{ v: 0, f }`, `engine.ts:649`). The previous value is dropped.
    pub fn set_formula(&mut self, sheet: SheetIx, row: u32, col: u32, f: FileFormula) -> Result<()> {
        if self.sheet(sheet).is_none() {
            return Err(CalcError::NoSheet(sheet.0));
        }
        let cur = match self.get(sheet, row, col) {
            Some(c) => c,
            None => {
                let xf = self.default_style(sheet, row, col);
                let raw =
                    RawCell { kind: CellKind::new(ValueTag::None, 0), payload: 0, style: if xf == 0 { 0 } else { xf.saturating_add(1) } };
                self.put_raw(sheet, row, col, raw)?;
                raw
            }
        };
        self.write_lanes(sheet, row, col, cur, ValueTag::None, false, 0)?;
        self.sheet_mut(sheet)?.formulas.set(row, col, f);
        self.set_flags(sheet, row, col, CellKind::HAS_FORMULA, true);
        Ok(())
    }

    /// The formula attributes of a cell's `<f>`, for an in-place change (the engine's `setDynamicAttrs`).
    pub fn formula_attrs_mut(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<&mut Option<Box<FormulaAttrs>>> {
        let sh = self.sheets.get_mut(sheet.0 as usize)?;
        sh.formulas.get_mut(row, col).map(|f| &mut f.attrs)
    }

    /// Replaces tag/value of an existing cell, clearing `inlineString`/`value`/`type` details of the extra.
    #[allow(clippy::too_many_arguments)]
    fn write_lanes(&mut self, sheet: SheetIx, row: u32, col: u32, cur: RawCell, tag: ValueTag, has_v: bool, payload: u64) -> Result<()> {
        if cur.kind.has(CellKind::HAS_EXTRA) {
            let mut dropped = None;
            self.edit_extra(sheet, row, col, |e| {
                e.raw_value = None;
                e.raw_type = None;
                dropped = e.inline.take();
            });
            if let Some(InlineExtra::Shape { rich: Some(r), .. }) = dropped {
                self.free_rich(r);
            }
        }
        let now = self.get(sheet, row, col).unwrap_or(cur);
        let keep = now.kind.0 & (CellKind::HAS_FORMULA | CellKind::OWNED | CellKind::HAS_EXTRA);
        let flags = keep | if has_v { CellKind::HAS_VALUE } else { 0 };
        self.put_raw(sheet, row, col, RawCell { kind: CellKind::new(tag, flags), payload, style: now.style })?;
        Ok(())
    }
}

/// A borrowed decoded shared string for [`Store::set_sst`].
#[derive(Clone, Copy, Debug)]
pub struct SstDecodedRef<'a> {
    /// `textOf(entry)`.
    pub text: JsStr<'a>,
    /// Runs, when rich.
    pub runs: Option<&'a [RichRun]>,
}

/// A value written by the engine ([`Store::write_value`]).
#[derive(Clone, Copy, Debug)]
pub enum CellWrite<'a> {
    /// `null`.
    Empty,
    /// A number.
    Num(f64),
    /// Text.
    Str(JsStr<'a>),
    /// Text already in the pool (one more reference is taken).
    Pooled(StrId),
    /// A boolean.
    Bool(bool),
    /// An error.
    Err(ErrorCode),
}

/// Who writes ([`Store::write_value`]): the `kind` of `writeScalar`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteKind {
    /// A constant typed by the user (text → `inlineStr`).
    Constant,
    /// A formula cell's result (text → `t="str"`).
    Formula,
    /// A spill/CSE member (text → `t="str"`).
    Spill,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(number_literal(1e21), "1E+21");
        assert_eq!(number_literal(1e-7), "1E-7");
        assert_eq!(number_literal(-0.0), "0");
        assert_eq!(number_text(-0.0), "0");
        assert_eq!(number_text(f64::NAN), "0");
        assert_eq!(number_text(1e21), "1E+21");
        assert_eq!(a1_ref(0, 0), "A1");
        assert_eq!(a1_ref(9, 26), "AA10");
        assert_eq!(a1_ref(1_048_575, 16_383), "XFD1048576");
    }

    #[test]
    fn errors() {
        assert_eq!(error_of(JsStr::Latin1(b" #n/a ")), ErrorCode::NA);
        let dotless: Vec<u16> = "#dıv/0!".encode_utf16().collect();
        assert_eq!(error_of(JsStr::Utf16(&dotless)), ErrorCode::Div0);
        assert_eq!(error_of(JsStr::Latin1(b"x")), ErrorCode::Value);
    }

    #[test]
    fn dates() {
        assert_eq!(ymd_to_serial(1900, 1, 1, false), 1.0);
        assert_eq!(ymd_to_serial(1900, 3, 1, false), 61.0);
        assert_eq!(ymd_to_serial(2024, 1, 15, false), 45306.0);
        assert_eq!(ymd_to_serial(1904, 1, 1, true), 0.0);
    }
}
