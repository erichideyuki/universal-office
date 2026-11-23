//! Byte-driven sequences of writer calls: the fuzz target `xlsx_write` (fuzz/src/xlsx.rs
//! includes this file) and the seeded no-panic test. Beyond "no panic": every call either
//! succeeds or returns an error, and a finished workbook is a valid package (every entry
//! inflates to its CRC, every part parses in the strict XML mode, the overrides name parts).
#![allow(dead_code)]

use uo_xlsx::{
    BorderEdge, BorderStyle, Borders, Cell, CellRange, CellValue, Column, DateTime, DefinedName, HAlign, Link, Rgb, RowOptions, SheetOptions, Style,
    StyleId, WorkbookOptions, XlsxWriter,
};

/// Reads bytes as small integers (zeros past the end).
pub struct Input<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Input<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Input { data, at: 0 }
    }
    pub fn done(&self) -> bool {
        self.at >= self.data.len()
    }
    pub fn u8(&mut self) -> u8 {
        let b = self.data.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        b
    }
    pub fn u16(&mut self) -> u16 {
        u16::from(self.u8()) | (u16::from(self.u8()) << 8)
    }
    pub fn u32(&mut self) -> u32 {
        u32::from(self.u16()) | (u32::from(self.u16()) << 16)
    }
    pub fn f64(&mut self) -> f64 {
        let lo = u64::from(self.u32());
        let hi = u64::from(self.u32());
        f64::from_bits(lo | (hi << 32))
    }
    pub fn bool(&mut self) -> bool {
        self.u8() & 1 == 1
    }
    /// A short string: ASCII, the characters that need escapes, controls, astral, `_xHHHH_`.
    pub fn text(&mut self) -> String {
        const PIECES: [&str; 24] = [
            "a", "Z", "0", " ", "&", "<", ">", "\"", "'", "\u{1}", "\t", "\n", "_x0041_", "ç", "😀", "\u{FFFE}", "[", "]", "/", "?", "!", ":", "=SUM(A1)", "History",
        ];
        let n = self.u8() % 12;
        (0..n).map(|_| PIECES[usize::from(self.u8()) % PIECES.len()]).collect()
    }
}

fn edge(inp: &mut Input<'_>) -> Option<BorderEdge> {
    match inp.u8() % 4 {
        0 | 1 => None,
        2 => Some(BorderEdge { style: BorderStyle::Thin, color: None }),
        _ => Some(BorderEdge { style: BorderStyle::Medium, color: Some(Rgb(inp.u32() & 0xFF_FFFF)) }),
    }
}

fn style(inp: &mut Input<'_>) -> Style {
    let codes = ["0.00", "General", "dd/mm/yyyy", "[Red]0;-0", "", "@", "0%", "\"x\"#", "yyyy-mm-dd hh:mm"];
    let flags = inp.u8();
    Style {
        num_fmt: (flags & 1 != 0).then(|| {
            let k = usize::from(inp.u8());
            if k >= codes.len() { inp.text() } else { codes[k].to_owned() }
        }),
        bold: flags & 2 != 0,
        italic: flags & 4 != 0,
        color: (flags & 8 != 0).then(|| Rgb(inp.u32() & 0xFF_FFFF)),
        fill: (flags & 16 != 0).then(|| Rgb(u32::from(inp.u8()))),
        border: if flags & 32 != 0 { Borders { left: edge(inp), right: edge(inp), top: edge(inp), bottom: edge(inp) } } else { Borders::default() },
        align: match inp.u8() % 8 {
            0 => Some(HAlign::Left),
            1 => Some(HAlign::Center),
            2 => Some(HAlign::Right),
            3 => Some(HAlign::Start),
            4 => Some(HAlign::End),
            5 => Some(HAlign::CenterContinuous),
            _ => None,
        },
        wrap: flags & 64 != 0,
    }
}

fn range(inp: &mut Input<'_>) -> CellRange {
    let r = u32::from(inp.u8() % 30);
    let c = u32::from(inp.u8() % 30);
    CellRange::new(r, c, r + u32::from(inp.u8() % 4), c + u32::from(inp.u8() % 4))
}

/// Runs the calls the bytes describe; returns the finished file when `finish` succeeded.
pub fn run(data: &[u8]) -> Option<Vec<u8>> {
    let mut inp = Input::new(data);
    let head = inp.u8();
    let opts = WorkbookOptions {
        title: (head & 1 != 0).then(|| inp.text()),
        language: (head & 2 != 0).then(|| inp.text()),
        shared_strings: head & 4 == 0,
        level: [0, 1, 4][usize::from(head >> 3) % 3],
        ..WorkbookOptions::default()
    };
    let mut w = XlsxWriter::new(opts).ok()?;
    let mut styles: Vec<StyleId> = vec![StyleId::DEFAULT];
    let mut texts: Vec<String> = Vec::new();
    let mut steps = 0;
    while !inp.done() && steps < 400 {
        steps += 1;
        match inp.u8() % 8 {
            0 => {
                let s = style(&mut inp);
                if let Ok(id) = w.add_style(&s) {
                    styles.push(id);
                }
            }
            1 => {
                let name = if inp.bool() { inp.text() } else { format!("S{}", inp.u8() % 4) };
                let cols = (0..inp.u8() % 5).map(|_| Column { width: inp.bool().then(|| f64::from(inp.u16()) / 100.0), hidden: inp.bool() }).collect();
                let names = (0..inp.u8() % 3)
                    .map(|_| DefinedName { name: if inp.bool() { "ColumnTitle".into() } else { inp.text() }, range: range(&mut inp), sheet_scope: inp.bool() })
                    .collect();
                let _ = w.begin_sheet(SheetOptions {
                    name,
                    right_to_left: inp.bool(),
                    columns: cols,
                    freeze_rows: u32::from(inp.u8() % 4),
                    freeze_cols: u32::from(inp.u8() % 3),
                    outline_levels: inp.u8() % 9,
                    summary_above: inp.bool(),
                    print_title_rows: inp.bool().then(|| (u32::from(inp.u8() % 3), u32::from(inp.u8() % 4))),
                    names,
                });
            }
            2..=4 => {
                let row = RowOptions {
                    outline_level: inp.u8() % 9,
                    height: inp.bool().then(|| f64::from(inp.u16()) / 100.0),
                    hidden: inp.bool(),
                };
                let n = usize::from(inp.u8() % 8);
                texts.clear();
                for _ in 0..n {
                    texts.push(inp.text());
                }
                let mut cells = Vec::with_capacity(n);
                for t in &texts {
                    let value = match inp.u8() % 6 {
                        0 => CellValue::Empty,
                        1 => CellValue::Text(t),
                        2 => CellValue::Number(inp.f64()),
                        3 => CellValue::Bool(inp.bool()),
                        _ => CellValue::Date(DateTime {
                            year: 1890 + i32::from(inp.u8()),
                            month: inp.u8() % 14,
                            day: inp.u8() % 33,
                            hour: inp.u8() % 25,
                            minute: inp.u8() % 61,
                            second: inp.u8() % 61,
                            millisecond: inp.u16() % 1001,
                        }),
                    };
                    let style = styles[usize::from(inp.u8()) % styles.len()];
                    let style = if inp.u8() == 255 { StyleId(9999) } else { style };
                    let link = match inp.u8() % 5 {
                        0 => Some(Link::Url(["https://e.com/a?b&c", "javascript:x", "mailto:a@b", t][usize::from(inp.u8() % 4)])),
                        1 => Some(Link::Location(t)),
                        _ => None,
                    };
                    cells.push(Cell { value, style, link });
                }
                let _ = w.append_row(&row, &cells);
            }
            5 => {
                let _ = w.merge(range(&mut inp));
            }
            6 => {
                let _ = w.end_sheet();
            }
            _ => {
                let _ = (w.next_row(), w.rows_written(), w.bytes_written(), w.dropped_links(), w.cut_texts());
            }
        }
    }
    let bytes = w.finish().ok()?;
    check_package(&bytes);
    Some(bytes)
}

/// The finished file is a valid package.
#[allow(clippy::panic)]
pub fn check_package(bytes: &[u8]) {
    let entries = uo_opc::zip::read(bytes, 10_000).unwrap_or_else(|e| panic!("zip: {e:?}"));
    let mut names = Vec::new();
    let mut content_types = String::new();
    for e in &entries {
        let raw = e.compressed(bytes);
        let data = if e.method == 0 { raw.to_vec() } else { uo_opc::flate::inflate_raw(raw, Some(e.size), 1 << 30).unwrap_or_else(|x| panic!("inflate {}: {x:?}", e.name)) };
        assert_eq!(uo_opc::flate::crc32(&data), e.crc32, "{}", e.name);
        let text = String::from_utf8(data).unwrap_or_else(|_| panic!("{} is not UTF-8", e.name));
        if let Err(x) = uo_opc::xml::parse(text.clone(), &uo_opc::xml::ParseOptions::STRICT) {
            panic!("{}: {x:?}\n{text:.2000}", e.name);
        }
        if e.name == "[Content_Types].xml" {
            content_types = text;
        }
        names.push(e.name.clone());
    }
    for n in &names {
        if !n.ends_with(".rels") && n != "[Content_Types].xml" {
            assert!(content_types.contains(&format!("PartName=\"/{n}\"")), "no override for {n}");
        }
    }
}
