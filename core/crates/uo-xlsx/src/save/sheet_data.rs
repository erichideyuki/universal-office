//! `<sheetData>` streamed from the store (`serializeSheetData`/`rowXml`/`cellXml`/`formulaXml`,
//! `serialize.ts:30-109`): rows in ascending order (a `FileOrder` sheet replays the TS order: the file's row
//! elements stable-sorted by row, each element's cells in file order, shadowed duplicates included), 64 KiB
//! buffers into the zip stream, no string of the sheet ever exists.

use std::collections::{BTreeMap, HashSet};

use uo_calc::cell::{FileCell, error_text};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::key::{Area, MAX_COL, MAX_ROW, SheetIx};
use uo_calc::load::FileRow;
use uo_calc::store::{CellKind, RawCell, RowMeta, Store, ValueTag};
use uo_calc::strings::JsStr;
use uo_calc::value::ErrorCode;
use uo_opc::zip::ZipWriter;

use super::text::{ColLetters, Esc, edge_js_space, edge_xml_space, put_js, put_num, put_u64};

const FLUSH: usize = 64 << 10;

/// The errors the file's `<v>` accepts; the 365 ones Excel writes as `#VALUE!` + a rich value.
fn file_error(e: ErrorCode) -> bool {
    matches!(
        e,
        ErrorCode::Null
            | ErrorCode::Div0
            | ErrorCode::Value
            | ErrorCode::Ref
            | ErrorCode::Name
            | ErrorCode::Num
            | ErrorCode::NA
            | ErrorCode::GettingData
    )
}

fn error_of_payload(p: u64) -> ErrorCode {
    uo_calc::cell::ERROR_CODES.get((p as usize).wrapping_sub(1)).copied().unwrap_or(ErrorCode::Value)
}

/// `FILE_ERRORS.has(value.trim().toUpperCase())`.
fn file_error_text(v: JsStr<'_>) -> bool {
    let units = v.to_utf16();
    let a = units.iter().position(|&u| !uo_jsnum::text::is_js_space_unit(u)).unwrap_or(units.len());
    let b = units.iter().rposition(|&u| !uo_jsnum::text::is_js_space_unit(u)).map_or(a, |i| i + 1);
    let upper = uo_text::case::to_uppercase(units.get(a..b).unwrap_or(&[]));
    uo_calc::cell::error_from_text(JsStr::Utf16(&upper)).is_some_and(file_error)
}

/// Where the XML goes: a zip stream, flushed in 64 KiB pieces (or a plain buffer in tests).
pub(crate) trait Out {
    fn write(&mut self, data: &[u8]) -> Result<(), uo_opc::Error>;
}

impl Out for ZipWriter {
    fn write(&mut self, data: &[u8]) -> Result<(), uo_opc::Error> {
        self.stream_write(data)
    }
}

impl Out for Vec<u8> {
    fn write(&mut self, data: &[u8]) -> Result<(), uo_opc::Error> {
        self.extend_from_slice(data);
        Ok(())
    }
}

struct Writer<'a, O: Out> {
    out: &'a mut O,
    buf: Vec<u8>,
    /// the current row's cells (the row element's `spans` come before them)
    row: Vec<u8>,
    letters: ColLetters,
    started: bool,
    err: Option<uo_opc::Error>,
    min: u32,
    max: u32,
    count: u32,
}

impl<O: Out> Writer<'_, O> {
    fn flush(&mut self, force: bool) {
        if (force || self.buf.len() >= FLUSH) && !self.buf.is_empty() {
            if self.err.is_none()
                && let Err(e) = self.out.write(&self.buf)
            {
                self.err = Some(e);
            }
            self.buf.clear();
        }
    }

    fn begin_row_cells(&mut self) {
        self.row.clear();
        self.min = u32::MAX;
        self.max = 0;
        self.count = 0;
    }

    fn note_col(&mut self, col: u32) {
        self.min = self.min.min(col);
        self.max = self.max.max(col);
        self.count += 1;
    }

    /// `rowXml` + the filter of `serializeSheetData` (cells, or `ht`, `hidden` or `s`).
    fn end_row(&mut self, row0: u32, meta: Option<&RowMeta>) {
        let m = meta.copied().unwrap_or_default();
        let keep = self.count > 0 || m.height.is_some() || m.flags & RowMeta::HIDDEN != 0 || m.style.is_some();
        if !keep {
            return;
        }
        if !self.started {
            self.buf.extend_from_slice(b"<sheetData>");
            self.started = true;
        }
        let b = &mut self.buf;
        b.extend_from_slice(b"<row r=\"");
        put_u64(b, u64::from(row0) + 1);
        b.push(b'"');
        if self.count > 0 {
            b.extend_from_slice(b" spans=\"");
            put_u64(b, u64::from(self.min) + 1);
            b.push(b':');
            put_u64(b, u64::from(self.max) + 1);
            b.push(b'"');
        }
        if let Some(s) = m.style {
            b.extend_from_slice(b" s=\"");
            put_num(b, s);
            b.extend_from_slice(b"\" customFormat=\"1\"");
        }
        if let Some(h) = m.height {
            b.extend_from_slice(b" ht=\"");
            put_num(b, h);
            b.push(b'"');
        }
        if m.flags & RowMeta::HIDDEN != 0 {
            b.extend_from_slice(b" hidden=\"1\"");
        }
        if m.flags & RowMeta::CUSTOM_HEIGHT != 0 {
            b.extend_from_slice(b" customHeight=\"1\"");
        }
        if self.count > 0 {
            b.push(b'>');
            b.extend_from_slice(&self.row);
            b.extend_from_slice(b"</row>");
        } else {
            b.extend_from_slice(b"/>");
        }
        self.flush(false);
    }

    fn cell_ref(&mut self, row0: u32, col: u32) {
        let (l, n) = self.letters.get(col);
        let r = &mut self.row;
        r.extend_from_slice(b"<c r=\"");
        r.extend_from_slice(&l[..n]);
        put_u64(r, u64::from(row0) + 1);
        r.push(b'"');
    }

    /// One live cell of the store.
    fn cell(&mut self, store: &Store, sheet: SheetIx, row0: u32, col: u32, raw: RawCell) {
        if raw.kind.0 & (CellKind::HAS_EXTRA | CellKind::HAS_FORMULA) == 0 && raw.kind.tag() != ValueTag::Other {
            self.note_col(col);
            self.cell_fast(store, row0, col, raw);
        } else if let Some(fc) = store.file_cell(sheet, row0, col) {
            self.note_col(col);
            self.cell_file(&fc);
        }
    }

    /// The canonical cell, straight from the lanes (no allocation).
    fn cell_fast(&mut self, store: &Store, row0: u32, col: u32, raw: RawCell) {
        self.cell_ref(row0, col);
        let r = &mut self.row;
        if raw.style != 0 {
            r.extend_from_slice(b" s=\"");
            put_u64(r, u64::from(raw.style - 1));
            r.push(b'"');
        }
        let has_v = raw.kind.has(CellKind::HAS_VALUE);
        let tag = raw.kind.tag();
        r.extend_from_slice(
            match tag {
                ValueTag::None | ValueTag::Other => "",
                ValueTag::N => " t=\"n\"",
                ValueTag::Bool => " t=\"b\"",
                ValueTag::Err => " t=\"e\"",
                ValueTag::Sst => " t=\"s\"",
                ValueTag::Inline => " t=\"inlineStr\"",
                ValueTag::Str => " t=\"str\"",
                ValueTag::Date => " t=\"d\"",
            }
            .as_bytes(),
        );
        match tag {
            ValueTag::None | ValueTag::N | ValueTag::Other if has_v => {
                r.extend_from_slice(b"><v>");
                let x = raw.number();
                if x.fract() == 0.0 && x.abs() < 1e15 && x != 0.0 {
                    put_num(r, x);
                } else if x == 0.0 {
                    r.push(b'0');
                } else {
                    r.extend_from_slice(uo_calc::cell::number_literal(x).as_bytes());
                }
                r.extend_from_slice(b"</v></c>");
            }
            ValueTag::Bool if has_v => {
                r.extend_from_slice(if raw.payload == 1 { b"><v>1</v></c>" } else { b"><v>0</v></c>" });
            }
            ValueTag::Err if has_v => {
                let e = error_of_payload(raw.payload);
                r.extend_from_slice(b"><v>");
                r.extend_from_slice(if file_error(e) { error_text(e).as_bytes() } else { b"#VALUE!" });
                r.extend_from_slice(b"</v></c>");
            }
            ValueTag::Sst if has_v => {
                r.extend_from_slice(b"><v>");
                put_u64(r, raw.payload);
                r.extend_from_slice(b"</v></c>");
            }
            ValueTag::Inline => {
                let text = raw.str_id().map_or(JsStr::EMPTY, |id| store.strings.get(id));
                r.extend_from_slice(b"><is><t");
                if edge_js_space(text) {
                    r.extend_from_slice(b" xml:space=\"preserve\"");
                }
                r.push(b'>');
                put_js(r, text, Esc::Xstring);
                r.extend_from_slice(b"</t></is></c>");
            }
            ValueTag::Str if has_v => {
                let text = raw.str_id().map_or(JsStr::EMPTY, |id| store.strings.get(id));
                r.extend_from_slice(b"><v");
                if edge_xml_space(text) {
                    r.extend_from_slice(b" xml:space=\"preserve\"");
                }
                r.push(b'>');
                put_js(r, text, Esc::Xstring);
                r.extend_from_slice(b"</v></c>");
            }
            ValueTag::Date if has_v => {
                let text = raw.str_id().map_or(JsStr::EMPTY, |id| store.strings.get(id));
                r.extend_from_slice(b"><v>");
                put_js(r, text, Esc::Text);
                r.extend_from_slice(b"</v></c>");
            }
            _ => r.extend_from_slice(b"/>"),
        }
    }

    /// `cellXml` of a model cell (the general case).
    fn cell_file(&mut self, c: &FileCell) {
        let r = &mut self.row;
        r.extend_from_slice(b"<c r=\"");
        if let Some(x) = &c.r#ref {
            put_js(r, x.as_js(), Esc::Attr);
        }
        r.push(b'"');
        if let Some(s) = c.s {
            r.extend_from_slice(b" s=\"");
            put_num(r, s);
            r.push(b'"');
        }
        if let Some(t) = &c.t
            && !t.as_js().is_empty()
        {
            r.extend_from_slice(b" t=\"");
            // `t="${c.type}"`: the TS does not escape it
            put_js(r, t.as_js(), Esc::Raw);
            r.push(b'"');
        }
        if let Some(cm) = c.cm {
            r.extend_from_slice(b" cm=\"");
            put_num(r, cm);
            r.push(b'"');
        }
        if let Some(vm) = c.vm {
            r.extend_from_slice(b" vm=\"");
            put_num(r, vm);
            r.push(b'"');
        }
        r.push(b'>');
        let start = r.len();
        if let Some(ftext) = &c.f {
            formula_xml(r, ftext.as_js(), c.fa.as_ref());
        }
        let tstr = c.t.as_ref().is_some_and(|t| t.as_js().eq_str("str"));
        let terr = c.t.as_ref().is_some_and(|t| t.as_js().eq_str("e"));
        if let Some(is) = &c.is {
            let text = is.text_of();
            r.extend_from_slice(b"<is><t");
            if edge_js_space(text.as_js()) {
                r.extend_from_slice(b" xml:space=\"preserve\"");
            }
            r.push(b'>');
            put_js(r, text.as_js(), Esc::Xstring);
            r.extend_from_slice(b"</t></is>");
        } else if tstr && let Some(v) = &c.v {
            r.extend_from_slice(b"<v");
            if edge_xml_space(v.as_js()) {
                r.extend_from_slice(b" xml:space=\"preserve\"");
            }
            r.push(b'>');
            put_js(r, v.as_js(), Esc::Xstring);
            r.extend_from_slice(b"</v>");
        } else if terr
            && let Some(v) = &c.v
            && !file_error_text(v.as_js())
        {
            r.extend_from_slice(b"<v>#VALUE!</v>");
        } else if let Some(v) = &c.v
            && (!v.as_js().is_empty() || c.f.is_none())
        {
            r.extend_from_slice(b"<v>");
            put_js(r, v.as_js(), Esc::Text);
            r.extend_from_slice(b"</v>");
        }
        if r.len() > start {
            r.extend_from_slice(b"</c>");
        } else {
            // no content: `<c .../>`
            r.pop();
            r.extend_from_slice(b"/>");
        }
    }
}

/// `formulaXml`: `<f>` with the attributes in the TS order; a child of a shared formula without text is `<f …/>`.
fn formula_xml(r: &mut Vec<u8>, text: JsStr<'_>, fa: Option<&FormulaAttrs>) {
    let mut a: Vec<u8> = Vec::new();
    if let Some(fa) = fa {
        if let Some(t) = fa.t
            && t != FormulaType::Normal
        {
            a.extend_from_slice(b" t=\"");
            a.extend_from_slice(t.text().as_bytes());
            a.push(b'"');
        }
        if let Some(x) = &fa.r#ref {
            a.extend_from_slice(b" ref=\"");
            put_js(&mut a, x.as_js(), Esc::Attr);
            a.push(b'"');
        }
        for (flag, name) in [
            (FormulaAttrs::DT2D, " dt2D=\"1\""),
            (FormulaAttrs::DTR, " dtr=\"1\""),
            (FormulaAttrs::DEL1, " del1=\"1\""),
            (FormulaAttrs::DEL2, " del2=\"1\""),
        ] {
            if fa.flags & flag != 0 {
                a.extend_from_slice(name.as_bytes());
            }
        }
        for (name, v) in [(" r1=\"", &fa.r1), (" r2=\"", &fa.r2)] {
            if let Some(x) = v {
                a.extend_from_slice(name.as_bytes());
                put_js(&mut a, x.as_js(), Esc::Attr);
                a.push(b'"');
            }
        }
        if fa.flags & FormulaAttrs::CA != 0 {
            a.extend_from_slice(b" ca=\"1\"");
        }
        if let Some(si) = fa.si {
            a.extend_from_slice(b" si=\"");
            put_num(&mut a, si);
            a.push(b'"');
        }
        for (flag, name) in [(FormulaAttrs::ACA, " aca=\"1\""), (FormulaAttrs::BX, " bx=\"1\"")] {
            if fa.flags & flag != 0 {
                a.extend_from_slice(name.as_bytes());
            }
        }
    }
    if !text.is_empty() {
        r.extend_from_slice(b"<f");
        r.extend_from_slice(&a);
        r.push(b'>');
        put_js(r, text, Esc::Text);
        r.extend_from_slice(b"</f>");
    } else if !a.is_empty() {
        r.extend_from_slice(b"<f");
        r.extend_from_slice(&a);
        r.extend_from_slice(b"/>");
    } else {
        r.extend_from_slice(b"<f></f>");
    }
}

/// Streams `<sheetData>` of `sheet` into `out` (`<sheetData/>` when no row has content).
pub(crate) fn write_sheet_data<O: Out>(store: &Store, sheet: SheetIx, out: &mut O) -> Result<(), uo_opc::Error> {
    let mut w = Writer {
        out,
        buf: Vec::with_capacity(FLUSH + 8192),
        row: Vec::with_capacity(8192),
        letters: ColLetters::default(),
        started: false,
        err: None,
        min: 0,
        max: 0,
        count: 0,
    };
    if let Some(sh) = store.sheet(sheet) {
        match sh.file_order.as_deref() {
            None => canonical(store, sheet, &mut w),
            Some(fo) => file_order(store, sheet, &fo.rows, &fo.shadowed, &mut w),
        }
    }
    if w.started {
        w.buf.extend_from_slice(b"</sheetData>");
    } else {
        w.buf.extend_from_slice(b"<sheetData/>");
    }
    w.flush(true);
    match w.err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn canonical<O: Out>(store: &Store, sheet: SheetIx, w: &mut Writer<'_, O>) {
    let Some(sh) = store.sheet(sheet) else { return };
    let mut metas = sh.rows.iter().peekable();
    let mut cur: Option<(u32, Option<RowMeta>)> = None;
    let area = Area { sheet, r1: 0, c1: 0, r2: MAX_ROW, c2: MAX_COL };
    store.each(area, |r, c, raw| {
        if cur.as_ref().is_some_and(|x| x.0 != r)
            && let Some((row, m)) = cur.take()
        {
            w.end_row(row, m.as_ref());
        }
        if cur.is_none() {
            // rows with attributes and no cell that come before this one
            while let Some(&&(mr, m)) = metas.peek() {
                if mr >= r {
                    break;
                }
                w.begin_row_cells();
                w.end_row(mr, Some(&m));
                metas.next();
            }
            let m = match metas.peek() {
                Some(&&(mr, m)) if mr == r => {
                    metas.next();
                    Some(m)
                }
                _ => None,
            };
            w.begin_row_cells();
            cur = Some((r, m));
        }
        w.cell(store, sheet, r, c, raw);
    });
    if let Some((row, m)) = cur.take() {
        w.end_row(row, m.as_ref());
    }
    for &(mr, m) in metas {
        w.begin_row_cells();
        w.end_row(mr, Some(&m));
    }
}

fn file_order<O: Out>(store: &Store, sheet: SheetIx, rows: &[FileRow], shadowed: &[(u32, FileCell)], w: &mut Writer<'_, O>) {
    // live cells the file order does not list (created after the open) go after the first element of their
    // row, or into a row element of their own
    let listed: HashSet<(u32, u32)> =
        rows.iter().flat_map(|fr| fr.cells.iter().filter(|&&c| c & FileRow::SHADOW == 0).map(move |&c| (fr.row, c))).collect();
    let mut extras: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let n_live = store.sheet(sheet).map_or(0, |s| s.cells);
    if n_live != listed.len() as u64 || listed.iter().any(|&(r, c)| store.get(sheet, r, c).is_none()) {
        let area = Area { sheet, r1: 0, c1: 0, r2: MAX_ROW, c2: MAX_COL };
        store.each(area, |r, c, _| {
            if !listed.contains(&(r, c)) {
                extras.entry(r).or_default().push(c);
            }
        });
    }
    // the model's rows, stable-sorted by `r` (`[...rows].sort((a, b) => a.r - b.r)`), plus the synthetic rows
    let element_rows: HashSet<u32> = rows.iter().map(|fr| fr.row).collect();
    let mut seq: Vec<(u32, Option<usize>)> = rows.iter().enumerate().map(|(i, fr)| (fr.row, Some(i))).collect();
    seq.extend(extras.keys().filter(|r| !element_rows.contains(r)).map(|&r| (r, None)));
    seq.sort_by_key(|x| x.0);
    let mut seen: HashSet<u32> = HashSet::new();
    for (row, el) in seq {
        w.begin_row_cells();
        let mut meta = None;
        // the element's cells: file order, shadows included
        let mut items: Vec<u32> = Vec::new();
        if let Some(i) = el {
            meta = rows[i].meta;
            items.extend_from_slice(&rows[i].cells);
        }
        // cells created after the open: `createCell` puts them in the first element of the row, at the place a
        // binary search over the columns finds (`sheet-calc.ts:202-222`)
        if (el.is_none() || seen.insert(row))
            && let Some(cols) = extras.get(&row)
        {
            let col_of = |it: u32| -> u32 {
                if it & FileRow::SHADOW != 0 { shadowed.get((it & !FileRow::SHADOW) as usize).map_or(0, |x| x.0) } else { it }
            };
            for &c in cols {
                let (mut lo, mut hi) = (0usize, items.len());
                while lo < hi {
                    let m = (lo + hi) >> 1;
                    if col_of(items[m]) < c {
                        lo = m + 1;
                    } else {
                        hi = m;
                    }
                }
                items.insert(lo, c);
            }
        }
        for item in items {
            if item & FileRow::SHADOW != 0 {
                if let Some((col, fc)) = shadowed.get((item & !FileRow::SHADOW) as usize) {
                    w.note_col(*col);
                    w.cell_file(fc);
                }
            } else if let Some(raw) = store.get(sheet, row, item) {
                w.cell(store, sheet, row, item, raw);
            }
        }
        w.end_row(row, meta.as_ref());
    }
}
