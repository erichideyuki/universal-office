//! A minimal consumer of uo-xlsx as the grid's `grid-xlsx` would be: one writer per instance,
//! styles and rows passed as byte buffers. Only for the wasm size measurement; the encoding is
//! deliberately small, not an API.
//!
//! Row buffer: outline level u8, then per cell `kind u8` (0 empty, 1 text, 2 number, 3 bool,
//! 4 date; | 0x80 = a URL follows the value), `style u32`, the value (text: `len u32` + UTF-8;
//! number: f64; bool: u8; date: year u32, month, day, hour, minute, second u8, ms u16), then the
//! URL (`len u32` + UTF-8). Style buffer: flags u8 (1 bold, 2 italic, 4 colour, 8 fill, 16
//! border, 32 wrap), colour u32, fill u32, align u8 (0 none, 1-6), format `len u32` + UTF-8.

use std::cell::RefCell;

use uo_xlsx::{
    BorderEdge, BorderStyle, Borders, Cell, CellRange, CellValue, Column, DateTime, DefinedName, HAlign, Link, Rgb, RowOptions, SheetOptions, Style,
    StyleId, WorkbookOptions, XlsxWriter,
};

thread_local! {
    static W: RefCell<Option<XlsxWriter>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn take(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // SAFETY: ptr/len come from `alloc(len)` and the host wrote only inside them
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

struct R<'a>(&'a [u8], usize);

impl<'a> R<'a> {
    fn bytes(&mut self, n: usize) -> &'a [u8] {
        let s = self.0.get(self.1..self.1.saturating_add(n)).unwrap_or(&[]);
        self.1 = self.1.saturating_add(n);
        s
    }
    fn u8(&mut self) -> u8 {
        self.bytes(1).first().copied().unwrap_or(0)
    }
    fn u32(&mut self) -> u32 {
        self.bytes(4).try_into().map(u32::from_le_bytes).unwrap_or(0)
    }
    fn f64(&mut self) -> f64 {
        self.bytes(8).try_into().map(f64::from_le_bytes).unwrap_or(0.0)
    }
    fn text(&mut self) -> &'a str {
        let n = self.u32() as usize;
        std::str::from_utf8(self.bytes(n)).unwrap_or("")
    }
    fn done(&self) -> bool {
        self.1 >= self.0.len()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut v = vec![0u8; len];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Starts a workbook: title, then `dc:language`.
#[unsafe(no_mangle)]
pub extern "C" fn create(ptr: *mut u8, len: usize) -> u32 {
    let b = take(ptr, len);
    let mut r = R(&b, 0);
    let (title, lang) = (r.text().to_owned(), r.text().to_owned());
    let w = XlsxWriter::new(WorkbookOptions { title: Some(title), language: Some(lang), ..WorkbookOptions::default() });
    W.with(|c| *c.borrow_mut() = w.ok());
    W.with(|c| u32::from(c.borrow().is_some()))
}

#[unsafe(no_mangle)]
pub extern "C" fn add_style(ptr: *mut u8, len: usize) -> i32 {
    let b = take(ptr, len);
    let mut r = R(&b, 0);
    let flags = r.u8();
    let (color, fill) = (r.u32(), r.u32());
    let align = match r.u8() {
        1 => Some(HAlign::Left),
        2 => Some(HAlign::Center),
        3 => Some(HAlign::Right),
        4 => Some(HAlign::Start),
        5 => Some(HAlign::End),
        6 => Some(HAlign::CenterContinuous),
        _ => None,
    };
    let fmt = r.text();
    let edge = BorderEdge { style: BorderStyle::Thin, color: None };
    let s = Style {
        num_fmt: (!fmt.is_empty()).then(|| fmt.to_owned()),
        bold: flags & 1 != 0,
        italic: flags & 2 != 0,
        color: (flags & 4 != 0).then_some(Rgb(color)),
        fill: (flags & 8 != 0).then_some(Rgb(fill)),
        border: if flags & 16 != 0 { Borders::all(edge) } else { Borders::default() },
        align,
        wrap: flags & 32 != 0,
    };
    W.with(|c| c.borrow_mut().as_mut().and_then(|w| w.add_style(&s).ok()).map_or(-1, |id| id.0 as i32))
}

/// Starts a sheet: name, rtl u8, header rows u32 (frozen, print titles, `ColumnTitle` on the
/// first cell of the last one), outline levels u8, then the column widths (f64 each).
#[unsafe(no_mangle)]
pub extern "C" fn begin_sheet(ptr: *mut u8, len: usize) -> u32 {
    let b = take(ptr, len);
    let mut r = R(&b, 0);
    let name = uo_xlsx::sanitize_sheet_name(r.text(), "Data");
    let rtl = r.u8() != 0;
    let headers = r.u32();
    let outline = r.u8();
    let mut columns = Vec::new();
    while !r.done() {
        columns.push(Column { width: Some(r.f64()), hidden: false });
    }
    let names = if headers > 0 { vec![DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(headers, 1), sheet_scope: true }] } else { Vec::new() };
    let opts = SheetOptions {
        name,
        right_to_left: rtl,
        columns,
        freeze_rows: headers,
        outline_levels: outline,
        print_title_rows: (headers > 0).then_some((1, headers)),
        names,
        ..SheetOptions::default()
    };
    W.with(|c| u32::from(c.borrow_mut().as_mut().is_some_and(|w| w.begin_sheet(opts).is_ok())))
}

/// Appends one row; returns its number (0 on error).
#[unsafe(no_mangle)]
pub extern "C" fn append_row(ptr: *mut u8, len: usize) -> u32 {
    let b = take(ptr, len);
    let mut r = R(&b, 0);
    let level = r.u8();
    let mut cells = Vec::new();
    while !r.done() {
        let kind = r.u8();
        let style = StyleId(r.u32());
        let value = match kind & 0x7F {
            1 => CellValue::Text(r.text()),
            2 => CellValue::Number(r.f64()),
            3 => CellValue::Bool(r.u8() != 0),
            4 => CellValue::Date(DateTime {
                year: r.u32() as i32,
                month: r.u8(),
                day: r.u8(),
                hour: r.u8(),
                minute: r.u8(),
                second: r.u8(),
                millisecond: u16::from(r.u8()) | (u16::from(r.u8()) << 8),
            }),
            _ => CellValue::Empty,
        };
        let link = (kind & 0x80 != 0).then(|| Link::Url(r.text()));
        cells.push(Cell { value, style, link });
    }
    let row = RowOptions { outline_level: level, ..RowOptions::default() };
    W.with(|c| c.borrow_mut().as_mut().and_then(|w| w.append_row(&row, &cells).ok()).unwrap_or(0))
}

/// Finishes the workbook; the file is at `out_ptr()` / `out_len()`.
#[unsafe(no_mangle)]
pub extern "C" fn finish() -> u32 {
    let Some(w) = W.with(|c| c.borrow_mut().take()) else { return 0 };
    match w.finish() {
        Ok(bytes) => {
            OUT.with(|o| *o.borrow_mut() = bytes);
            1
        }
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}
