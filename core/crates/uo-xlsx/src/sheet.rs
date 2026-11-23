//! What a caller says about a sheet, a row and a cell, the validators, and the worksheet XML
//! around `<sheetData>` (head before the first row, tail after the last).

use uo_opc::xml::write::{Escape, escape_into};

use crate::cell::{DateTime, push_number, push_ref, push_u32};
use crate::error::{Error, Result};
use crate::style::StyleId;
use crate::{MAX_COLS, MAX_ROWS};

/// The value of a cell. Text is always a text cell (never a formula, whatever it starts with).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellValue<'a> {
    /// no value (the cell is written only when it has a style or a link)
    Empty,
    Text(&'a str),
    /// a finite number; NaN and infinities are written as empty cells (no `<v>` can hold them)
    Number(f64),
    Bool(bool),
    /// a wall-clock date and time, written as its serial. Without a number format in its style,
    /// the cell gets the reader's short date (built-in 14), or short date and time (22) when the
    /// time is not midnight. A date Excel cannot store (before 1900-01-01, after 9999-12-31) is
    /// written as ISO 8601 text.
    Date(DateTime),
}

/// A hyperlink of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link<'a> {
    /// an external target (`http:`, `https:`, `mailto:`, `ftp:`, `ftps:`, `tel:`); other schemes
    /// (`javascript:`, `file:`, `data:`…) and targets longer than [`crate::MAX_LINK_LEN`] are
    /// dropped and counted ([`crate::XlsxWriter::dropped_links`])
    Url(&'a str),
    /// a place in the workbook (`'Sheet 2'!A1`, a defined name)
    Location(&'a str),
}

/// One cell of a row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell<'a> {
    pub value: CellValue<'a>,
    pub style: StyleId,
    pub link: Option<Link<'a>>,
}

impl<'a> Cell<'a> {
    pub fn new(value: CellValue<'a>) -> Self {
        Cell { value, style: StyleId::DEFAULT, link: None }
    }
    pub fn text(s: &'a str) -> Self {
        Cell::new(CellValue::Text(s))
    }
    pub fn number(x: f64) -> Self {
        Cell::new(CellValue::Number(x))
    }
    pub fn empty() -> Self {
        Cell::new(CellValue::Empty)
    }
    pub fn with_style(mut self, style: StyleId) -> Self {
        self.style = style;
        self
    }
    pub fn with_link(mut self, link: Link<'a>) -> Self {
        self.link = Some(link);
        self
    }
}

/// Row properties.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RowOptions {
    /// outline (group) level 0-7, at most [`SheetOptions::outline_levels`]
    pub outline_level: u8,
    /// height in points (0-409.5); `None` = automatic
    pub height: Option<f64>,
    pub hidden: bool,
}

/// One column of a sheet (index 0 = column A).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Column {
    /// width in characters of the Normal font's widest digit (Excel's unit, 0-255); `None` =
    /// the default width. [`column_width_from_px`] converts from pixels.
    pub width: Option<f64>,
    pub hidden: bool,
}

/// A rectangle of cells, 1-based and inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRange {
    pub first_row: u32,
    pub first_col: u32,
    pub last_row: u32,
    pub last_col: u32,
}

impl CellRange {
    pub fn new(first_row: u32, first_col: u32, last_row: u32, last_col: u32) -> Self {
        CellRange { first_row, first_col, last_row, last_col }
    }
    /// One cell.
    pub fn cell(row: u32, col: u32) -> Self {
        CellRange::new(row, col, row, col)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let ok = (1..=MAX_ROWS).contains(&self.first_row)
            && (1..=MAX_COLS).contains(&self.first_col)
            && (self.first_row..=MAX_ROWS).contains(&self.last_row)
            && (self.first_col..=MAX_COLS).contains(&self.last_col);
        if ok { Ok(()) } else { Err(Error::InvalidRange) }
    }

    pub(crate) fn overlaps(&self, o: &CellRange) -> bool {
        self.first_row <= o.last_row && o.first_row <= self.last_row && self.first_col <= o.last_col && o.first_col <= self.last_col
    }

    /// `A1:B2` (or `A1` for one cell).
    pub(crate) fn push_a1(&self, out: &mut String) {
        push_ref(out, self.first_row, self.first_col);
        if self.first_row != self.last_row || self.first_col != self.last_col {
            out.push(':');
            push_ref(out, self.last_row, self.last_col);
        }
    }

    /// `$A$1:$B$2` (or `$A$1`).
    pub(crate) fn push_absolute(&self, out: &mut String) {
        let one = |out: &mut String, r: u32, c: u32| {
            out.push('$');
            crate::cell::push_column(out, c);
            out.push('$');
            push_u32(out, r);
        };
        one(out, self.first_row, self.first_col);
        if self.first_row != self.last_row || self.first_col != self.last_col {
            out.push(':');
            one(out, self.last_row, self.last_col);
        }
    }
}

/// A defined name over a range of the sheet that declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedName {
    /// the name (Excel's rules: a letter, `_` or `\` first, then letters, digits, `_`, `.`, `\`;
    /// not a cell reference; at most 255 characters). The screen-reader convention for column
    /// headers is a name starting with `ColumnTitle` on the first header cell.
    pub name: String,
    pub range: CellRange,
    /// scoped to its sheet (`localSheetId`) rather than to the workbook
    pub sheet_scope: bool,
}

/// Options of a sheet, given when it starts (the worksheet XML puts them before the rows).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetOptions {
    /// tab name ([`validate_sheet_name`]; [`sanitize_sheet_name`] makes any text valid)
    pub name: String,
    /// right-to-left sheet (column A on the right)
    pub right_to_left: bool,
    /// widths of columns A, B, … (`Column::default()` keeps the default)
    pub columns: Vec<Column>,
    /// rows frozen at the top (0 = none)
    pub freeze_rows: u32,
    /// columns frozen at the start (0 = none)
    pub freeze_cols: u32,
    /// deepest row outline level of the sheet (0-7): rows may not exceed it
    pub outline_levels: u8,
    /// outline summary rows ABOVE their details (Excel's default, and this default, is below)
    pub summary_above: bool,
    /// rows repeated at the top of every printed page (1-based, inclusive)
    pub print_title_rows: Option<(u32, u32)>,
    /// defined names over ranges of this sheet
    pub names: Vec<DefinedName>,
}

/// Why Excel would refuse `name` as a sheet name, if it would.
pub fn validate_sheet_name(name: &str) -> Result<()> {
    let units = name.encode_utf16().count();
    if units == 0 {
        return Err(Error::SheetName("empty"));
    }
    if units > 31 {
        return Err(Error::SheetName("longer than 31 characters"));
    }
    if name.contains(['[', ']', ':', '*', '?', '/', '\\']) {
        return Err(Error::SheetName("contains one of [ ] : * ? / \\"));
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Err(Error::SheetName("starts or ends with an apostrophe"));
    }
    if name.eq_ignore_ascii_case("history") {
        return Err(Error::SheetName("History is reserved"));
    }
    if name.chars().any(|c| (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}') {
        return Err(Error::SheetName("contains a control character"));
    }
    Ok(())
}

/// `name` made valid: the characters Excel refuses (`[ ] : * ? / \` and control characters)
/// removed, apostrophes trimmed at the ends, cut to 31 characters (never inside a surrogate
/// pair); `fallback` when nothing is left or the name is `History`.
pub fn sanitize_sheet_name(name: &str, fallback: &str) -> String {
    let cleaned: String =
        name.chars().filter(|&c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') && (c as u32) >= 0x20 && c != '\u{FFFE}' && c != '\u{FFFF}').collect();
    let trimmed = cleaned.trim_matches('\'');
    let cut = crate::cell::cut_text(trimmed, 31).trim_end_matches('\'');
    if validate_sheet_name(cut).is_ok() {
        return cut.to_owned();
    }
    let fb = crate::cell::cut_text(fallback, 31).trim_matches('\'');
    if validate_sheet_name(fb).is_ok() { fb.to_owned() } else { "Sheet".to_owned() }
}

/// A column width in Excel's unit for a width in pixels, given the widest digit of the Normal
/// font in pixels (`mdw`: 7 for Calibri 11 and Aptos Narrow 11 at 96 dpi). Excel shows the
/// result as exactly `px` pixels (it draws ⌊(256·w + ⌊128/mdw⌋)/256 · mdw⌋, measured in
/// sheet-core `grid-metrics.ts`); the width is capped at 255.
pub fn column_width_from_px(px: u32, mdw: u32) -> f64 {
    let mdw = mdw.max(1);
    let k = 128 / mdw;
    // smallest n (w = n/256) with ⌊(n + k)·mdw / 256⌋ >= px
    let n = (256 * u64::from(px)).div_ceil(u64::from(mdw)).saturating_sub(u64::from(k));
    let n = n.min(255 * 256);
    n as f64 / 256.0
}

/// Whether `name` is a defined name Excel accepts.
pub(crate) fn validate_defined_name(name: &str) -> Result<()> {
    let bad = || Err(Error::InvalidName(name.to_owned()));
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return bad() };
    if name.encode_utf16().count() > 255 {
        return bad();
    }
    if !(first.is_alphabetic() || first == '_' || first == '\\') {
        return bad();
    }
    if !chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?')) {
        return bad();
    }
    if name.starts_with("_xlnm.") || looks_like_reference(name) {
        return bad();
    }
    Ok(())
}

/// `A1`, `XFD1048576`, `R`, `C`, `R1C1`, `r2`… (names Excel reads as references).
fn looks_like_reference(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    if up == "R" || up == "C" {
        return true;
    }
    let b = up.as_bytes();
    // A1 style: 1-3 letters then digits, column <= XFD, row <= 1048576
    let letters = b.iter().take_while(|c| c.is_ascii_uppercase()).count();
    if (1..=3).contains(&letters) && letters < b.len() && b[letters..].iter().all(u8::is_ascii_digit) {
        let col = b[..letters].iter().fold(0u32, |a, &c| a * 26 + u32::from(c - b'A' + 1));
        let row = std::str::from_utf8(&b[letters..]).ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(u64::MAX);
        if col <= MAX_COLS && (1..=u64::from(MAX_ROWS)).contains(&row) {
            return true;
        }
    }
    // R1C1 style: R[digits][C[digits]] or C[digits]
    let rc = |s: &[u8]| -> bool {
        let mut i = 0;
        if s.first() == Some(&b'R') {
            i = 1;
            while s.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        if s.get(i) == Some(&b'C') {
            i += 1;
            while s.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        i > 0 && i == s.len()
    };
    rc(b)
}

/// `'name'` with apostrophes doubled (a sheet in a formula or defined name).
pub(crate) fn push_quoted_sheet(out: &mut String, name: &str) {
    out.push('\'');
    for c in name.chars() {
        if c == '\'' {
            out.push('\'');
        }
        if c == '&' || c == '<' || c == '>' {
            escape_into(out, c.encode_utf8(&mut [0; 4]), Escape::TEXT);
        } else {
            out.push(c);
        }
    }
    out.push('\'');
}

/// The worksheet XML up to and including `<sheetData>`.
pub(crate) fn push_head(o: &mut String, opts: &SheetOptions, tab_selected: bool) {
    o.push_str(uo_opc::xml::write::XML_DECL_BARE);
    o.push_str(
        "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
    );
    if opts.outline_levels > 0 || opts.summary_above {
        o.push_str(if opts.summary_above { "<sheetPr><outlinePr summaryBelow=\"0\"/></sheetPr>" } else { "<sheetPr><outlinePr summaryBelow=\"1\"/></sheetPr>" });
    }
    o.push_str("<sheetViews><sheetView");
    if opts.right_to_left {
        o.push_str(" rightToLeft=\"1\"");
    }
    if tab_selected {
        o.push_str(" tabSelected=\"1\"");
    }
    o.push_str(" workbookViewId=\"0\"");
    let (x, y) = (opts.freeze_cols, opts.freeze_rows);
    if x == 0 && y == 0 {
        o.push_str("/></sheetViews>");
    } else {
        let pane = if x > 0 && y > 0 {
            "bottomRight"
        } else if x > 0 {
            "topRight"
        } else {
            "bottomLeft"
        };
        o.push_str("><pane");
        if x > 0 {
            o.push_str(" xSplit=\"");
            push_u32(o, x);
            o.push('"');
        }
        if y > 0 {
            o.push_str(" ySplit=\"");
            push_u32(o, y);
            o.push('"');
        }
        o.push_str(" topLeftCell=\"");
        push_ref(o, y + 1, x + 1);
        o.push_str("\" activePane=\"");
        o.push_str(pane);
        o.push_str("\" state=\"frozen\"/>");
        // Excel writes a selection per pane; the active one is enough
        o.push_str("<selection pane=\"");
        o.push_str(pane);
        o.push_str("\" activeCell=\"");
        push_ref(o, y + 1, x + 1);
        o.push_str("\" sqref=\"");
        push_ref(o, y + 1, x + 1);
        o.push_str("\"/></sheetView></sheetViews>");
    }
    o.push_str("<sheetFormatPr defaultRowHeight=\"15\"");
    if opts.outline_levels > 0 {
        o.push_str(" outlineLevelRow=\"");
        push_u32(o, u32::from(opts.outline_levels));
        o.push('"');
    }
    o.push_str("/>");
    let mut any_col = false;
    for (i, c) in opts.columns.iter().enumerate() {
        if c.width.is_none() && !c.hidden {
            continue;
        }
        if !any_col {
            o.push_str("<cols>");
            any_col = true;
        }
        let n = i as u32 + 1;
        o.push_str("<col min=\"");
        push_u32(o, n);
        o.push_str("\" max=\"");
        push_u32(o, n);
        o.push('"');
        // hidden without a width: Excel needs a width attribute, the default one (8.43)
        o.push_str(" width=\"");
        push_number(o, c.width.unwrap_or(8.43));
        o.push('"');
        if c.width.is_some() {
            o.push_str(" customWidth=\"1\"");
        }
        if c.hidden {
            o.push_str(" hidden=\"1\"");
        }
        o.push_str("/>");
    }
    if any_col {
        o.push_str("</cols>");
    }
    o.push_str("<sheetData>");
}

/// Checks the options of a sheet (everything but the name, checked by the writer).
pub(crate) fn validate_options(opts: &SheetOptions) -> Result<()> {
    if opts.outline_levels > 7 {
        return Err(Error::OutlineLevel { level: opts.outline_levels, declared: 7 });
    }
    if opts.columns.len() > MAX_COLS as usize {
        return Err(Error::TooManyColumns);
    }
    for c in &opts.columns {
        if let Some(w) = c.width
            && !(w.is_finite() && (0.0..=255.0).contains(&w))
        {
            return Err(Error::InvalidSize);
        }
    }
    if opts.freeze_rows >= MAX_ROWS || opts.freeze_cols >= MAX_COLS {
        return Err(Error::InvalidRange);
    }
    if let Some((a, b)) = opts.print_title_rows
        && (a == 0 || b < a || b > MAX_ROWS)
    {
        return Err(Error::InvalidRange);
    }
    for (i, n) in opts.names.iter().enumerate() {
        validate_defined_name(&n.name)?;
        n.range.validate()?;
        if opts.names[..i].iter().any(|m| m.sheet_scope == n.sheet_scope && m.name.to_lowercase() == n.name.to_lowercase()) {
            return Err(Error::DuplicateName(n.name.clone()));
        }
    }
    Ok(())
}

/// `<row …>` attributes for row `r`.
pub(crate) fn push_row_open(o: &mut String, r: u32, row: &RowOptions) {
    o.push_str("<row r=\"");
    push_u32(o, r);
    o.push('"');
    if let Some(h) = row.height {
        o.push_str(" ht=\"");
        push_number(o, h);
        o.push_str("\" customHeight=\"1\"");
    }
    if row.hidden {
        o.push_str(" hidden=\"1\"");
    }
    if row.outline_level > 0 {
        o.push_str(" outlineLevel=\"");
        push_u32(o, u32::from(row.outline_level));
        o.push('"');
    }
    o.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names() {
        assert!(validate_sheet_name("Relatório 2026").is_ok());
        assert!(validate_sheet_name(&"x".repeat(31)).is_ok());
        assert!(validate_sheet_name(&"x".repeat(32)).is_err());
        for bad in ["", "a/b", "a[b", "'a", "a'", "History", "HISTORY", "a\u{1}"] {
            assert!(validate_sheet_name(bad).is_err(), "{bad:?}");
        }
        assert!(validate_sheet_name("a'b").is_ok());
        assert_eq!(sanitize_sheet_name("Q1: [sales]/*?", "S"), "Q1 sales");
        assert_eq!(sanitize_sheet_name("'''", "Data"), "Data");
        assert_eq!(sanitize_sheet_name("history", "Data"), "Data");
        assert_eq!(sanitize_sheet_name(&"😀".repeat(20), "S").encode_utf16().count(), 30);
        assert_eq!(sanitize_sheet_name("", "[]"), "Sheet");
    }

    #[test]
    fn names() {
        for ok in ["ColumnTitle", "ColumnTitle1", "_x", "\\a", "Título", "a.b", "AAAA1", "XFE1", "A0"] {
            assert!(validate_defined_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "1a", "A1", "xfd1048576", "R", "c", "R1C1", "R2", "C3", "a b", "a-b", "_xlnm.Print_Titles"] {
            assert!(validate_defined_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn widths() {
        let px = |w: f64, mdw: u32| ((256.0 * w + f64::from(128 / mdw)) / 256.0 * f64::from(mdw)).floor() as u32;
        for mdw in [5, 7, 8, 9, 11] {
            for p in 0..400 {
                let w = column_width_from_px(p, mdw);
                if w < 255.0 {
                    assert_eq!(px(w, mdw), p.max(px(0.0, mdw)), "px {p} mdw {mdw} w {w}");
                }
            }
        }
        assert_eq!(column_width_from_px(100_000, 7), 255.0);
    }

    #[test]
    fn ranges() {
        let mut s = String::new();
        CellRange::new(1, 1, 2, 3).push_a1(&mut s);
        s.push(' ');
        CellRange::cell(5, 28).push_absolute(&mut s);
        assert_eq!(s, "A1:C2 $AB$5");
        assert!(CellRange::new(2, 1, 1, 1).validate().is_err());
        assert!(CellRange::new(1, 1, MAX_ROWS, MAX_COLS).validate().is_ok());
        assert!(CellRange::new(0, 1, 1, 1).validate().is_err());
        assert!(CellRange::new(1, 1, 2, 2).overlaps(&CellRange::cell(2, 2)));
        assert!(!CellRange::new(1, 1, 2, 2).overlaps(&CellRange::cell(3, 2)));
    }
}
