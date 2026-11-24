//! The streaming worksheet reader: a part is inflated 64 KiB at a time (`Package::stream_part`), tokenized by
//! `uo_opc::xml::pull` and its `<sheetData>` goes straight into the `uo-calc` store row by row, never as one text
//! or one tree (the TS decodes the part to a string and parses it into a DOM, `read.ts:285-305`). The bytes before
//! and after `<sheetData>` are the frame: parsed as a DOM for the sheet model (`read_worksheet_frame`) and kept as
//! text for the surgical save.
//!
//! Rules of `readRow`/`readCell` (`read.ts:573-621`): `r` is optional on `<row>` and `<c>` (the position follows
//! the previous one), a `<c r>` whose reference does not parse gets the position-derived reference, `t` is
//! taken as written, `s` through `parseInt`, `cm`/`vm` only when all digits, `<f>` attributes through
//! `readFormulaAttrs`, `<is>` through `readSharedStringEntry`. Only the FIRST `<v>`, `<f>`, `<is>` of a cell
//! count. A canonical cell (plain number, boolean, error text, shared-string index, no formula, no raw `r`)
//! takes the store's fast path (`SheetLoader::cell_raw`: lanes only); anything else goes through `FileCell`.

use super::model::{ColorContext, Worksheet};
use super::sheet::read_worksheet_frame;
use super::sst::{MEl, MiniBuilder, RunPropsTable, local_name, main_prefixes, shared_string_entry};
use super::{decode_xstring, pi, ref_to_row_col};
use crate::book::{ReadError, ReadLimits, SheetFrame};
use uo_calc::cell::{FileCell, InlineText, a1_ref, canonical_number, error_of, error_text};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::key::{MAX_COL, MAX_ROW, SheetIx};
use uo_calc::load::SheetLoader;
use uo_calc::store::{CellKind, RawCell, RowMeta, Store, ValueTag};
use uo_calc::strings::{JsStr, JsString, StrId, StrPool};
use uo_opc::Package;
use uo_opc::xml::pull::{self, FeedEnd, Flow, Pull, StartTag, Token};

/// What reading one worksheet produced besides the cells.
#[derive(Debug)]
pub struct SheetOutcome {
    /// The part outside `<sheetData>`.
    pub frame: SheetFrame,
    /// The sheet model without rows.
    pub worksheet: Worksheet,
    /// Cells put in the store.
    pub cells: u64,
    /// Cells and rows that fall outside A1:XFD1048576 (dropped: the store has no place for them).
    pub dropped: u64,
}

fn xml_err(e: uo_opc::Error) -> ReadError {
    ReadError::Opc(e)
}

fn calc_err(e: &uo_calc::CalcError) -> ReadError {
    ReadError::Calc(format!("{e:?}"))
}

/// An attribute value decoded to text (references, normalization); lone surrogates become U+FFFD.
fn text_of(raw: &[u8], attr: bool) -> Result<std::borrow::Cow<'_, str>, ReadError> {
    if let Some(s) = pull::is_plain(raw, attr) {
        return Ok(std::borrow::Cow::Borrowed(s));
    }
    let mut u = Vec::new();
    if attr { pull::decode_attr(raw, &mut u) } else { pull::decode_text(raw, &mut u) }.map_err(xml_err)?;
    Ok(std::borrow::Cow::Owned(String::from_utf16_lossy(&u)))
}

fn js_of(raw: &[u8], attr: bool) -> Result<JsString, ReadError> {
    if let Some(s) = pull::is_plain(raw, attr) {
        return Ok(JsString::from_str_js(s));
    }
    let mut u = Vec::new();
    if attr { pull::decode_attr(raw, &mut u) } else { pull::decode_text(raw, &mut u) }.map_err(xml_err)?;
    Ok(JsString::from_utf16(&u))
}

#[derive(Clone, Debug, PartialEq)]
enum TKind {
    None,
    N,
    B,
    E,
    S,
    Inline,
    Str,
    D,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Child {
    None,
    V,
    F,
    Is,
}

/// A piece of the text of `<v>` or `<f>`: raw text or a CDATA section.
#[derive(Clone, Copy)]
struct Piece {
    end: usize,
    cdata: bool,
}

#[derive(Default)]
struct Pieces {
    bytes: Vec<u8>,
    pieces: Vec<Piece>,
}

impl Pieces {
    fn clear(&mut self) {
        self.bytes.clear();
        self.pieces.clear();
    }
    fn push(&mut self, raw: &[u8], cdata: bool) {
        self.bytes.extend_from_slice(raw);
        self.pieces.push(Piece { end: self.bytes.len(), cdata });
    }
    /// `textContent` as a JS string.
    fn to_js(&self) -> Result<JsString, ReadError> {
        if let [one] = self.pieces.as_slice()
            && !one.cdata
        {
            return js_of(&self.bytes, false);
        }
        Ok(JsString::from_utf16(&self.units()?))
    }
    fn units(&self) -> Result<Vec<u16>, ReadError> {
        let mut u = Vec::new();
        let mut at = 0;
        for p in &self.pieces {
            let raw = self.bytes.get(at..p.end).unwrap_or(&[]);
            if p.cdata { pull::decode_cdata(raw, &mut u) } else { pull::decode_text(raw, &mut u) }.map_err(xml_err)?;
            at = p.end;
        }
        Ok(u)
    }
    /// The plain text when the content is one non-CDATA piece without references (the fast path).
    fn plain(&self) -> Option<&str> {
        match self.pieces.as_slice() {
            [one] if !one.cdata => pull::is_plain(&self.bytes, false),
            _ => None,
        }
    }
}

/// The `<is><t>text</t></is>` shape read without building an element tree.
#[derive(Default)]
struct FastIs {
    /// 0: `<is>` open, 1: inside `<t>`, 2: `<t>` closed.
    state: u8,
    is_attrs: Vec<u8>,
    t_attrs: Vec<u8>,
    preserve: bool,
    text: Pieces,
}

enum RefState {
    /// No `r`, or one that equals the position: nothing to keep.
    Canonical,
    /// A reference that differs (`$A$1`, another row): kept raw.
    Custom(JsString),
}

struct CellSt {
    col0: Option<u32>,
    t_present: bool,
    t: TKind,
    t_other: Option<JsString>,
    s: Option<f64>,
    cm: Option<f64>,
    vm: Option<f64>,
    r: RefState,
    v: bool,
    v_preserve: bool,
    f: bool,
    f_attrs: Option<FormulaAttrs>,
    is: Option<InlineText>,
    seen_is: bool,
}

struct Sink<'a, 's> {
    loader: SheetLoader<'s>,
    cx: &'a ColorContext,
    props: &'a mut RunPropsTable,
    prefixes: Vec<Vec<u8>>,
    d: u32,
    skip: Option<u32>,
    in_sd: bool,
    in_row: bool,
    row0: Option<u32>,
    next_row: f64,
    next_col: f64,
    child: Child,
    cell: Option<CellSt>,
    vtext: Pieces,
    ftext: Pieces,
    mini: Option<MiniBuilder>,
    fast: FastIs,
    mini_bytes: usize,
    max_cell_text: usize,
    rows: u32,
    spans: Vec<(u32, u32, u32)>,
    uniq: Vec<(Vec<u8>, JsString)>,
    sheetdata_start: Option<u64>,
    sheetdata_end: u64,
    data_name: Vec<u8>,
    data_attrs: Vec<u8>,
    data_self_closing: bool,
    done: bool,
    cells: u64,
    dropped: u64,
    err: Option<ReadError>,
}

impl Sink<'_, '_> {
    fn local<'b>(&self, name: &'b [u8]) -> &'b [u8] {
        local_name(name, &self.prefixes)
    }

    fn fail(&mut self, e: ReadError) -> Flow {
        self.err.get_or_insert(e);
        Flow::Stop
    }

    fn token(&mut self, tok: Token<'_>) -> Flow {
        match self.token_inner(tok) {
            Ok(f) => f,
            Err(e) => self.fail(e),
        }
    }

    fn token_inner(&mut self, tok: Token<'_>) -> Result<Flow, ReadError> {
        match tok {
            Token::Start(t) => self.start(&t),
            Token::End { end, .. } => self.end(end),
            Token::Text(raw) => self.text(raw, false),
            Token::CData(raw) => self.text(raw, true),
        }
    }

    /// Text or CDATA of the open child; the bytes one cell collects are capped (`ReadLimits::max_cell_text`).
    fn text(&mut self, raw: &[u8], cdata: bool) -> Result<Flow, ReadError> {
        let used = match self.child {
            Child::V => {
                self.vtext.push(raw, cdata);
                self.vtext.bytes.len()
            }
            Child::F => {
                self.ftext.push(raw, cdata);
                self.ftext.bytes.len()
            }
            Child::Is => {
                self.mini_bytes += raw.len();
                if let Some(m) = self.mini.as_mut() {
                    if cdata { m.cdata(raw)? } else { m.text(raw)? }
                } else if self.fast.state == 1 {
                    self.fast.text.push(raw, cdata);
                }
                self.mini_bytes
            }
            _ => 0,
        };
        if used > self.max_cell_text {
            return Err(ReadError::Limit("cell text"));
        }
        Ok(Flow::Continue)
    }

    fn start(&mut self, t: &StartTag<'_>) -> Result<Flow, ReadError> {
        if !t.self_closing {
            self.d = t.depth;
        }
        if self.skip.is_some() {
            return Ok(Flow::Continue);
        }
        let local = self.local(t.name);
        match t.depth {
            1 => {
                self.prefixes = main_prefixes(t.attr_bytes);
            }
            2 if !self.in_sd && self.sheetdata_start.is_none() && local == b"sheetData" => {
                self.sheetdata_start = Some(t.start);
                self.sheetdata_end = t.end;
                self.data_name = t.name.to_vec();
                self.data_attrs = t.attr_bytes.to_vec();
                self.data_self_closing = t.self_closing;
                if t.self_closing {
                    self.done = true;
                    return Ok(Flow::Stop);
                }
                self.in_sd = true;
            }
            3 if self.in_sd => {
                if local == b"row" {
                    self.start_row(t)?;
                    if t.self_closing {
                        self.in_row = false;
                    }
                } else if !t.self_closing {
                    self.skip = Some(3);
                }
            }
            4 if self.in_row => {
                if local == b"c" {
                    self.start_cell(t)?;
                    if t.self_closing {
                        self.end_cell()?;
                    }
                } else if !t.self_closing {
                    self.skip = Some(4);
                }
            }
            5 if self.cell.is_some() => self.start_child(t, local == b"v", local == b"f", local == b"is")?,
            d if d > 5 && self.cell.is_some() => {
                if self.child == Child::Is {
                    if let Some(m) = self.mini.as_mut() {
                        m.start(t, &self.prefixes)?;
                    } else if d == 6 && self.fast.state == 0 && local == b"t" {
                        // the common shape `<is><t>text</t></is>`: no element tree
                        self.fast.t_attrs = t.attr_bytes.to_vec();
                        self.fast.preserve = t.attr(b"xml:space").is_some_and(|v| v == b"preserve");
                        self.fast.state = if t.self_closing { 2 } else { 1 };
                        self.fast.text.clear();
                    } else {
                        self.leave_fast_shape()?;
                        if let Some(m) = self.mini.as_mut() {
                            m.start(t, &self.prefixes)?;
                        }
                    }
                }
            }
            _ => {
                if self.in_sd && !t.self_closing {
                    self.skip = Some(t.depth);
                }
            }
        }
        Ok(Flow::Continue)
    }

    fn start_row(&mut self, t: &StartTag<'_>) -> Result<(), ReadError> {
        let mut r_attr = f64::NAN;
        let mut meta = RowMeta::default();
        let mut spans_raw: Option<&[u8]> = None;
        for (n, v) in t.attrs() {
            match n {
                b"r" => r_attr = pi(&text_of(v, true)?),
                b"spans" => spans_raw = Some(v),
                b"ht" => meta.height = Some(uo_jsnum::parse::parse_float(&text_of(v, true)?)),
                b"customHeight" => set_flag(&mut meta.flags, RowMeta::CUSTOM_HEIGHT, &text_of(v, true)?),
                b"hidden" => set_flag(&mut meta.flags, RowMeta::HIDDEN, &text_of(v, true)?),
                b"s" => meta.style = Some(pi(&text_of(v, true)?)),
                b"customFormat" => set_flag(&mut meta.flags, RowMeta::CUSTOM_FORMAT, &text_of(v, true)?),
                b"thickTop" => set_flag(&mut meta.flags, RowMeta::THICK_TOP, &text_of(v, true)?),
                b"thickBot" => set_flag(&mut meta.flags, RowMeta::THICK_BOT, &text_of(v, true)?),
                _ => {}
            }
        }
        let row_r = if r_attr.is_finite() && r_attr > 0.0 { r_attr } else { self.next_row };
        self.next_row = row_r + 1.0;
        self.next_col = 1.0;
        self.in_row = true;
        if row_r > f64::from(MAX_ROW) + 1.0 {
            self.row0 = None;
            self.dropped += 1;
            return Ok(());
        }
        let row0 = uo_jsnum::conv::to_uint32(row_r - 1.0);
        if let Some(raw) = spans_raw {
            let known = if self.uniq.last().is_some_and(|(r, _)| r == raw) {
                Some(self.uniq.len() - 1)
            } else if self.uniq.len() <= 32 {
                self.uniq.iter().position(|(r, _)| r == raw)
            } else {
                None
            };
            let id = match known {
                Some(i) => i,
                None => {
                    self.uniq.push((raw.to_vec(), js_of(raw, true)?));
                    self.uniq.len() - 1
                }
            };
            self.spans.push((self.rows, row0, u32::try_from(id).unwrap_or(u32::MAX)));
            meta.spans = Some(StrPool::EMPTY);
        }
        self.row0 = Some(row0);
        self.rows += 1;
        self.loader.row(row0, meta).map_err(|e| calc_err(&e))
    }

    fn start_cell(&mut self, t: &StartTag<'_>) -> Result<(), ReadError> {
        let mut t_present = false;
        let mut c = CellSt {
            col0: None,
            t_present: false,
            t: TKind::None,
            t_other: None,
            s: None,
            cm: None,
            vm: None,
            r: RefState::Canonical,
            v: false,
            v_preserve: false,
            f: false,
            f_attrs: None,
            is: None,
            seen_is: false,
        };
        let mut r_raw: Option<&[u8]> = None;
        for (n, v) in t.attrs() {
            match n {
                b"r" => r_raw = Some(v),
                b"t" => {
                    c.t = match v {
                        b"n" => TKind::N,
                        b"b" => TKind::B,
                        b"e" => TKind::E,
                        b"s" => TKind::S,
                        b"inlineStr" => TKind::Inline,
                        b"str" => TKind::Str,
                        b"d" => TKind::D,
                        _ => {
                            // spelled with references or another value: decode and compare as JS strings
                            let js = js_of(v, true)?;
                            let tk = match js.as_js() {
                                x if x.eq_str("n") => TKind::N,
                                x if x.eq_str("b") => TKind::B,
                                x if x.eq_str("e") => TKind::E,
                                x if x.eq_str("s") => TKind::S,
                                x if x.eq_str("inlineStr") => TKind::Inline,
                                x if x.eq_str("str") => TKind::Str,
                                x if x.eq_str("d") => TKind::D,
                                _ => TKind::Other,
                            };
                            if tk == TKind::Other {
                                c.t_other = Some(js);
                            }
                            tk
                        }
                    };
                    t_present = true;
                }
                b"s" => c.s = Some(parse_s(v)?),
                b"cm" => c.cm = digits_only(v)?,
                b"vm" => c.vm = digits_only(v)?,
                _ => {}
            }
        }
        c.t_present = t_present;
        // the column: from the reference when it parses, else the one after the previous cell
        let mut col1 = self.next_col;
        if let Some(raw) = r_raw.filter(|r| !r.is_empty()) {
            match (self.row0, quick_ref(raw)) {
                (Some(row0), Some((cc, rr))) if rr == f64::from(row0) + 1.0 => col1 = cc,
                _ => {
                    let txt = text_of(raw, true)?;
                    if let Some((cc, _)) = ref_to_row_col(txt.as_bytes()) {
                        col1 = cc;
                        let canon = match self.row0 {
                            Some(row0) if cc >= 1.0 && cc <= f64::from(MAX_COL) + 1.0 => {
                                Some(a1_ref(row0, uo_jsnum::conv::to_uint32(cc - 1.0)))
                            }
                            _ => None,
                        };
                        if canon.as_deref() != Some(&*txt) {
                            c.r = RefState::Custom(js_of(raw, true)?);
                        }
                    }
                }
            }
        }
        self.next_col = col1 + 1.0;
        c.col0 = (self.row0.is_some() && col1 >= 1.0 && col1 <= f64::from(MAX_COL) + 1.0).then(|| uo_jsnum::conv::to_uint32(col1 - 1.0));
        if c.col0.is_none() {
            self.dropped += 1;
        }
        self.vtext.clear();
        self.ftext.clear();
        self.child = Child::None;
        self.cell = Some(c);
        Ok(())
    }

    fn start_child(&mut self, t: &StartTag<'_>, is_v: bool, is_f: bool, is_is: bool) -> Result<(), ReadError> {
        let Some(c) = self.cell.as_mut() else { return Ok(()) };
        if is_v && !c.v {
            c.v = true;
            c.v_preserve = last_attr(t, b"xml:space").is_ok_and(|v| v == "preserve");
            self.child = Child::V;
        } else if is_f && !c.f {
            c.f = true;
            let mut fa = FormulaAttrs::default();
            let mut any = false;
            let mut seen: Vec<&[u8]> = Vec::new();
            // the TS object keeps the LAST value of a repeated attribute
            for (n, _) in t.attrs() {
                if !seen.contains(&n) {
                    seen.push(n);
                }
            }
            for n in seen {
                let Some(v) = t.attrs().filter(|(k, _)| *k == n).last().map(|(_, v)| v) else { continue };
                let val = text_of(v, true)?;
                match n {
                    b"t" => match val.as_ref() {
                        "normal" => fa.t = Some(FormulaType::Normal),
                        "array" => fa.t = Some(FormulaType::Array),
                        "dataTable" => fa.t = Some(FormulaType::DataTable),
                        "shared" => fa.t = Some(FormulaType::Shared),
                        _ => {}
                    },
                    b"ref" => fa.r#ref = Some(js_of(v, true)?),
                    b"si" => {
                        if !val.is_empty() && val.bytes().all(|b| b.is_ascii_digit()) {
                            fa.si = Some(pi(&val));
                        }
                    }
                    b"aca" => flag(&mut fa.flags, FormulaAttrs::ACA, &val),
                    b"ca" => flag(&mut fa.flags, FormulaAttrs::CA, &val),
                    b"bx" => flag(&mut fa.flags, FormulaAttrs::BX, &val),
                    b"dt2D" => flag(&mut fa.flags, FormulaAttrs::DT2D, &val),
                    b"dtr" => flag(&mut fa.flags, FormulaAttrs::DTR, &val),
                    b"del1" => flag(&mut fa.flags, FormulaAttrs::DEL1, &val),
                    b"del2" => flag(&mut fa.flags, FormulaAttrs::DEL2, &val),
                    b"r1" => fa.r1 = Some(js_of(v, true)?),
                    b"r2" => fa.r2 = Some(js_of(v, true)?),
                    _ => {}
                }
                any = true;
            }
            let _ = any;
            c.f_attrs = (fa != FormulaAttrs::default()).then_some(fa);
            self.child = Child::F;
        } else if is_is && !c.seen_is {
            c.seen_is = true;
            if t.self_closing {
                // `<is/>`: readSharedStringEntry of an empty element is `{text: ''}`
                c.is = Some(InlineText { text: Some(JsString::default()), runs: None });
                return Ok(());
            }
            self.child = Child::Is;
            self.mini_bytes = 0;
            self.mini = None;
            self.fast.state = 0;
            self.fast.is_attrs = t.attr_bytes.to_vec();
            self.fast.text.clear();
            return Ok(());
        } else {
            if !t.self_closing {
                self.skip = Some(5);
            }
            return Ok(());
        }
        if t.self_closing {
            self.child = Child::None;
        }
        Ok(())
    }

    fn end(&mut self, end: u64) -> Result<Flow, ReadError> {
        let closing = self.d;
        self.d = self.d.saturating_sub(1);
        if let Some(sd) = self.skip {
            if sd == closing {
                self.skip = None;
            }
            return Ok(Flow::Continue);
        }
        match closing {
            2 if self.in_sd => {
                self.in_sd = false;
                self.done = true;
                self.sheetdata_end = end;
                return Ok(Flow::Stop);
            }
            3 if self.in_row => {
                self.in_row = false;
            }
            4 if self.cell.is_some() => self.end_cell()?,
            5 if self.cell.is_some() => {
                if self.child == Child::Is {
                    let entry = if let Some(mut b) = self.mini.take() {
                        b.end().map(|el| shared_string_entry(&el, self.cx, self.props))
                    } else {
                        Some(InlineText { text: Some(self.fast_text()?), runs: None })
                    };
                    if let (Some(e), Some(c)) = (entry, self.cell.as_mut()) {
                        c.is = Some(e);
                    }
                }
                self.child = Child::None;
            }
            d if d > 5 && self.child == Child::Is => {
                if let Some(m) = self.mini.as_mut() {
                    m.end();
                } else if self.fast.state == 1 {
                    self.fast.state = 2;
                }
            }
            _ => {}
        }
        Ok(Flow::Continue)
    }

    /// Leaves the `<is><t>` shape: builds the element tree from what the fast path already saw.
    fn leave_fast_shape(&mut self) -> Result<(), ReadError> {
        let mut m = MiniBuilder::new();
        let p = &self.prefixes;
        fn tag<'a>(name: &'a [u8], attrs: &'a [u8], depth: u32) -> StartTag<'a> {
            StartTag { name, attr_bytes: attrs, self_closing: false, depth, start: 0, end: 0 }
        }
        m.start(&tag(b"is", &self.fast.is_attrs, 5), p)?;
        if self.fast.state >= 1 {
            m.start(&tag(b"t", &self.fast.t_attrs, 6), p)?;
            let mut at = 0;
            for pc in &self.fast.text.pieces {
                let raw = self.fast.text.bytes.get(at..pc.end).unwrap_or(&[]);
                if pc.cdata {
                    m.cdata(raw)?
                } else {
                    m.text(raw)?
                }
                at = pc.end;
            }
            if self.fast.state == 2 {
                m.end();
            }
        }
        self.mini = Some(m);
        Ok(())
    }

    /// `tText` of the `<t>` the fast path collected.
    fn fast_text(&self) -> Result<JsString, ReadError> {
        let f = &self.fast;
        if f.state == 0 || f.text.pieces.is_empty() {
            return Ok(JsString::default());
        }
        if let Some(p) = f.text.plain() {
            let p = if f.preserve { p } else { p.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) };
            if !p.contains("_x") {
                return Ok(JsString::from_str_js(p));
            }
        }
        let u = f.text.units()?;
        let u = if f.preserve { u } else { trim_xml_ws(&u) };
        Ok(JsString::from_utf16(&decode_xstring(&u)))
    }

    fn end_cell(&mut self) -> Result<(), ReadError> {
        let Some(c) = self.cell.take() else { return Ok(()) };
        let Some(col0) = c.col0 else { return Ok(()) };
        self.cells += 1;
        // fast path: lanes only
        if !c.f && c.is.is_none() && c.cm.is_none() && c.vm.is_none() && matches!(c.r, RefState::Canonical) {
            let style = match c.s {
                None => Some(0u32),
                Some(s) if uo_jsnum::conv::is_integer(s) && (0.0..4_294_967_295.0).contains(&s) && !(s == 0.0 && s.is_sign_negative()) => {
                    Some(uo_jsnum::conv::to_uint32(s) + 1)
                }
                _ => None,
            };
            if let Some(style) = style
                && let Some(raw) = self.fast_cell(&c, style)
            {
                return self.loader.cell_raw(col0, raw).map_err(|e| calc_err(&e));
            }
        }
        let t_text = match c.t {
            TKind::None => None,
            TKind::N => Some("n"),
            TKind::B => Some("b"),
            TKind::E => Some("e"),
            TKind::S => Some("s"),
            TKind::Inline => Some("inlineStr"),
            TKind::Str => Some("str"),
            TKind::D => Some("d"),
            TKind::Other => None,
        };
        let t = match (&c.t_other, t_text) {
            (Some(o), _) => Some(o.clone()),
            (None, Some(x)) => Some(JsString::from_str_js(x)),
            (None, None) => None,
        };
        let _ = c.t_present;
        let mut cell = FileCell { t, s: c.s, cm: c.cm, vm: c.vm, ..FileCell::default() };
        if let RefState::Custom(r) = c.r {
            cell.r#ref = Some(r);
        }
        if c.v {
            let units;
            cell.v = Some(if c.t == TKind::Str {
                // tText: trimmed at both ends unless xml:space="preserve", then `_xHHHH_`
                let u = self.vtext.units()?;
                units = if c.v_preserve { u } else { trim_xml_ws(&u) };
                JsString::from_utf16(&decode_xstring(&units))
            } else {
                self.vtext.to_js()?
            });
        }
        if c.f {
            cell.f = Some(self.ftext.to_js()?);
            cell.fa = c.f_attrs;
        }
        cell.is = c.is;
        self.loader.cell(col0, &cell).map_err(|e| calc_err(&e))
    }

    /// The lanes of a canonical cell, or `None` when it needs the model path.
    fn fast_cell(&self, c: &CellSt, style: u32) -> Option<RawCell> {
        let v: Option<&str> = if c.v { Some(self.vtext.plain().unwrap_or("\u{0}\u{0}")) } else { None };
        // `plain()` is None for empty, split or non-plain text: the sentinel never parses as canonical
        if c.v && self.vtext.pieces.is_empty() {
            return None;
        }
        let (tag, payload, has_v) = match (&c.t, v) {
            (TKind::None | TKind::N, None) => (if c.t == TKind::N { ValueTag::N } else { ValueTag::None }, 0u64, false),
            (TKind::None | TKind::N, Some(v)) => {
                let x = canonical_number(v)?;
                (if c.t == TKind::N { ValueTag::N } else { ValueTag::None }, x.to_bits(), true)
            }
            (TKind::B, Some("1")) => (ValueTag::Bool, 1, true),
            (TKind::B, Some("0")) => (ValueTag::Bool, 0, true),
            (TKind::E, Some(v)) => {
                let code = error_of(JsStr::Latin1(v.as_bytes()));
                if !v.is_ascii() || v != error_text(code) {
                    return None;
                }
                (ValueTag::Err, u64::from(code as u8), true)
            }
            (TKind::S, Some(v)) => {
                let b = v.as_bytes();
                if b.is_empty() || b.len() > 9 || !b.iter().all(u8::is_ascii_digit) || (b[0] == b'0' && b.len() > 1) {
                    return None;
                }
                (ValueTag::Sst, v.parse::<u64>().ok()?, true)
            }
            _ => return None,
        };
        let flags = if has_v { CellKind::HAS_VALUE } else { 0 };
        Some(RawCell { kind: CellKind::new(tag, flags), payload, style })
    }
}

/// A boolean attribute: the last value of a repeated attribute decides.
fn set_flag(flags: &mut u8, bit: u8, v: &str) {
    if v == "1" || v == "true" {
        *flags |= bit;
    } else {
        *flags &= !bit;
    }
}

fn flag(flags: &mut u8, bit: u8, v: &str) {
    set_flag(flags, bit, v);
}

fn last_attr(t: &StartTag<'_>, name: &[u8]) -> Result<String, ReadError> {
    match t.attr(name) {
        Some(v) => Ok(text_of(v, true)?.into_owned()),
        None => Ok(String::new()),
    }
}

/// `parseInt` of an `s` attribute, with a fast path for plain digits.
fn parse_s(v: &[u8]) -> Result<f64, ReadError> {
    if !v.is_empty() && v.len() <= 9 && v.iter().all(u8::is_ascii_digit) {
        return Ok(f64::from(v.iter().fold(0u32, |a, &d| a * 10 + u32::from(d - b'0'))));
    }
    Ok(pi(&text_of(v, true)?))
}

/// A reference that is exactly `[A-Z]+[1-9][0-9]*`: its column and row (no decoding, no allocation).
fn quick_ref(raw: &[u8]) -> Option<(f64, f64)> {
    let l = raw.iter().take_while(|c| c.is_ascii_uppercase()).count();
    let d = raw.get(l..)?;
    if l == 0 || d.is_empty() || d.len() > 9 || d[0] == b'0' || !d.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let col = raw[..l].iter().fold(0.0f64, |acc, &c| acc * 26.0 + f64::from(c - 64));
    let row = d.iter().fold(0u32, |a, &x| a * 10 + u32::from(x - b'0'));
    Some((col, f64::from(row)))
}

fn digits_only(v: &[u8]) -> Result<Option<f64>, ReadError> {
    let s = text_of(v, true)?;
    Ok((!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| pi(&s)))
}

fn trim_xml_ws(u: &[u16]) -> Vec<u16> {
    let ws = |x: u16| matches!(x, 0x20 | 0x09 | 0x0A | 0x0D);
    let a = u.iter().position(|&x| !ws(x)).unwrap_or(u.len());
    let b = u.iter().rposition(|&x| !ws(x)).map_or(a, |i| i + 1);
    u[a..b].to_vec()
}

/// Reads one worksheet part into `store` as sheet `sheet` and returns its frame and model.
pub fn read_sheet(
    pkg: &Package,
    part: &str,
    store: &mut Store,
    sheet: SheetIx,
    cx: &ColorContext,
    props: &mut RunPropsTable,
    limits: &ReadLimits,
) -> Result<SheetOutcome, ReadError> {
    let mut ps = pkg.stream_part(part).map_err(xml_err)?.ok_or_else(|| ReadError::Opc(uo_opc::Error::MissingPart(part.to_owned())))?;
    let mut head: Vec<u8> = Vec::new();
    let mut tail: Vec<u8> = Vec::new();
    let mut head_done = false;
    let mut pull = Pull::new().with_max_depth(limits.max_depth).with_max_token(limits.max_cell_text as usize + (64 << 10));
    let mut abs: u64 = 0;
    let mut stopped: Option<u64> = None;
    let mut first = true;
    let (mut cells, mut dropped, spans, uniq, sd_start, sd_name, sd_attrs, sd_self);
    {
        let loader = store.load_sheet(sheet).map_err(|e| calc_err(&e))?;
        let mut sink = Sink {
            loader,
            cx,
            props,
            prefixes: Vec::new(),
            d: 0,
            skip: None,
            in_sd: false,
            in_row: false,
            row0: None,
            next_row: 1.0,
            next_col: 1.0,
            child: Child::None,
            cell: None,
            vtext: Pieces::default(),
            ftext: Pieces::default(),
            mini: None,
            fast: FastIs::default(),
            mini_bytes: 0,
            max_cell_text: limits.max_cell_text as usize,
            rows: 0,
            spans: Vec::new(),
            uniq: Vec::new(),
            sheetdata_start: None,
            sheetdata_end: 0,
            data_name: Vec::new(),
            data_attrs: Vec::new(),
            data_self_closing: false,
            done: false,
            cells: 0,
            dropped: 0,
            err: None,
        };
        while let Some(chunk) = ps.next_chunk().map_err(xml_err)? {
            let mut data = chunk;
            if first {
                first = false;
                if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
                    data = rest;
                }
            }
            let chunk_abs = abs;
            abs += data.len() as u64;
            if let Some(e) = stopped {
                // after </sheetData>: the rest of the part is the suffix, not tokenized
                tail.extend_from_slice(data);
                let _ = e;
                continue;
            }
            if !head_done {
                head.extend_from_slice(data);
            }
            let end = pull.feed(data, false, |t| Ok(sink.token(t))).map_err(xml_err)?;
            if let Some(e) = sink.err.take() {
                return Err(e);
            }
            if !head_done && let Some(s) = sink.sheetdata_start {
                head.truncate(usize::try_from(s).unwrap_or(usize::MAX).min(head.len()));
                head_done = true;
            }
            match end {
                FeedEnd::Stopped(e) if sink.done => {
                    stopped = Some(e);
                    let from = usize::try_from(e.saturating_sub(chunk_abs)).unwrap_or(0).min(data.len());
                    tail.extend_from_slice(data.get(from..).unwrap_or(&[]));
                }
                FeedEnd::RootClosed(_) => {
                    // no <sheetData> at all, or it never closed: the frame is whatever the root held
                    stopped = Some(abs);
                }
                _ => {}
            }
        }
        if stopped.is_none() {
            // the part ended: a document with no root or an unclosed element
            pull.feed(&[], true, |_| Ok(Flow::Continue)).map_err(xml_err)?;
        }
        if sink.in_sd && !sink.done {
            return Err(ReadError::Opc(uo_opc::Error::Xml {
                offset: 0,
                kind: uo_opc::error::XmlErrorKind::Unclosed,
                part: Some(part.to_owned()),
            }));
        }
        cells = sink.cells;
        dropped = sink.dropped;
        spans = std::mem::take(&mut sink.spans);
        uniq = std::mem::take(&mut sink.uniq);
        sd_start = sink.sheetdata_start;
        sd_name = std::mem::take(&mut sink.data_name);
        sd_attrs = std::mem::take(&mut sink.data_attrs);
        sd_self = sink.data_self_closing;
        sink.loader.finish().map_err(|e| calc_err(&e))?;
    }
    patch_spans(store, sheet, &spans, &uniq).map_err(|e| calc_err(&e))?;
    let _ = (&mut cells, &mut dropped);

    // the frame and the sheet model
    let prefix_text = String::from_utf8_lossy(&head).into_owned();
    let suffix_text = String::from_utf8_lossy(&tail).into_owned();
    let frame_text = if sd_start.is_some() {
        format!("{prefix_text}<{}{}/>{suffix_text}", String::from_utf8_lossy(&sd_name), String::from_utf8_lossy(&sd_attrs))
    } else {
        prefix_text.clone()
    };
    let opts = pkg.limits().bounded(&super::SHEET_XML);
    let dom = uo_opc::xml::parse(frame_text, &opts).map_err(|e| match e {
        uo_opc::Error::Xml { offset, kind, part: None } => ReadError::Opc(uo_opc::Error::Xml { offset, kind, part: Some(part.to_owned()) }),
        e => ReadError::Opc(e),
    })?;
    let worksheet = read_worksheet_frame(pkg, part, dom.root(), cx)?;
    let frame = SheetFrame {
        part: part.into(),
        prefix: prefix_text.into_boxed_str(),
        suffix: if sd_start.is_some() { suffix_text.into_boxed_str() } else { Box::default() },
        data_name: String::from_utf8_lossy(&sd_name).into_owned().into_boxed_str(),
        data_attrs: String::from_utf8_lossy(&sd_attrs).into_owned().into_boxed_str(),
        has_sheet_data: sd_start.is_some(),
        data_self_closing: sd_self,
    };
    Ok(SheetOutcome { frame, worksheet, cells, dropped })
}

/// Gives the rows their real `spans` strings (the loader got the empty-string placeholder: it keeps a row
/// meta only when it has attributes, and the loader borrows the store, so the pool was out of reach while
/// streaming).
fn patch_spans(store: &mut Store, sheet: SheetIx, spans: &[(u32, u32, u32)], uniq: &[(Vec<u8>, JsString)]) -> uo_calc::Result<()> {
    if spans.is_empty() {
        return Ok(());
    }
    let mut ids: Vec<StrId> = Vec::with_capacity(uniq.len());
    for (_, js) in uniq {
        ids.push(store.strings.intern(js.as_js())?);
    }
    let Store { sheets, strings, .. } = store;
    if let Some(sh) = sheets.get_mut(sheet.0 as usize) {
        for &(k, row0, u) in spans {
            let Some(&id) = ids.get(u as usize) else { continue };
            if let Some(fo) = sh.file_order.as_deref_mut()
                && let Some(fr) = fo.rows.get_mut(k as usize)
                && fr.row == row0
                && let Some(m) = fr.meta.as_mut()
            {
                m.spans = Some(id);
                strings.retain(id);
            }
            if let Ok(i) = sh.rows.binary_search_by_key(&row0, |&(r, _)| r) {
                let m = &mut sh.rows[i].1;
                if let Some(old) = m.spans.replace(id) {
                    strings.release(old);
                }
                strings.retain(id);
            }
        }
    }
    for id in ids {
        strings.release(id);
    }
    Ok(())
}

/// A mini element of the model, re-exported for tests.
pub type SubTree = MEl;
