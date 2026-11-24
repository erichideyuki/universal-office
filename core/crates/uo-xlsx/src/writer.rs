//! The workbook writer: one sheet open at a time, rows streamed into the archive.

use std::collections::HashMap;

use uo_opc::flate::{self, Deflater};
use uo_opc::zip::{RawEntry, WriteOptions, ZipWriter};

use crate::cell::{cut_text, date_serial, date_text, needs_preserve, push_number, push_ref, push_u32, push_xstring};
use crate::error::{Error, Result};
use crate::parts::{self, SheetMeta};
use crate::sheet::{self, AutoFilter, Cell, CellRange, CellValue, Link, RowOptions, SheetOptions};
use crate::style::{BUILTIN_DATE_TIME, BUILTIN_SHORT_DATE, Style, StyleId, StylePool};
use crate::{MAX_CELL_TEXT, MAX_COLS, MAX_LINK_LEN, MAX_LINKS_PER_SHEET, MAX_ROWS};

/// Distinct strings the shared-string table deduplicates; later new strings are appended
/// without lookup (a valid table may repeat strings), so the index stops growing.
pub const SST_DEDUP_CAP: usize = 1 << 20;
/// Sheet XML buffered before it is deflated into the archive.
const FLUSH_AT: usize = 64 * 1024;

/// The Normal font of the workbook (`styles.xml` font 0: every style uses its name and size).
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    pub name: String,
    /// points (1-409)
    pub size: f64,
}

impl Default for FontSpec {
    /// Aptos Narrow 11, the Normal font of Excel 365 (sheet-core `blank.ts`). The file stores
    /// only the name: a reader without the font substitutes it.
    fn default() -> Self {
        FontSpec { name: "Aptos Narrow".to_owned(), size: 11.0 }
    }
}

/// Options of a workbook.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkbookOptions {
    /// `dc:title`
    pub title: Option<String>,
    /// `dc:creator`
    pub creator: Option<String>,
    /// `dc:language` (BCP 47, `pt-BR`)
    pub language: Option<String>,
    /// `dcterms:created`, W3CDTF (`2026-11-23T10:00:00Z`); the writer has no clock
    pub created: Option<String>,
    /// `Application` of `docProps/app.xml`
    pub application: String,
    pub font: FontSpec,
    /// text in the shared-string table (Excel's way; repeated texts stored once). `false`
    /// writes inline strings: nothing grows with the text but the archive.
    pub shared_strings: bool,
    /// deflate level 0-9 (0 = stored)
    pub level: u8,
}

impl Default for WorkbookOptions {
    fn default() -> Self {
        WorkbookOptions {
            title: None,
            creator: None,
            language: None,
            created: None,
            application: "UniversalOffice".to_owned(),
            font: FontSpec::default(),
            shared_strings: true,
            level: uo_opc::zip::DEFAULT_LEVEL,
        }
    }
}

/// The shared-string table, deflated as it grows.
struct Sst {
    index: HashMap<Box<str>, u32>,
    count: u32,
    pending: String,
    deflater: Option<Deflater>,
    out: Vec<u8>,
    crc: u32,
    size: u64,
}

const SST_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">";
const SST_TAIL: &str = "</sst>";

impl Sst {
    fn new(level: u8) -> Self {
        let deflater = (level > 0).then(|| {
            let mut d = Deflater::new(level);
            d.stream_begin();
            d
        });
        let mut s = Sst { index: HashMap::new(), count: 0, pending: String::new(), deflater, out: Vec::new(), crc: 0, size: 0 };
        s.pending.push_str(SST_HEAD);
        s
    }

    fn get(&mut self, text: &str) -> Result<u32> {
        if let Some(&i) = self.index.get(text) {
            return Ok(i);
        }
        let i = self.count;
        self.count += 1;
        if self.index.len() < SST_DEDUP_CAP {
            self.index.insert(text.into(), i);
        }
        self.pending.push_str(if needs_preserve(text) { "<si><t xml:space=\"preserve\">" } else { "<si><t>" });
        push_xstring(&mut self.pending, text);
        self.pending.push_str("</t></si>");
        if self.pending.len() >= FLUSH_AT {
            self.flush()?;
        }
        Ok(i)
    }

    fn flush(&mut self) -> Result<()> {
        let b = self.pending.as_bytes();
        self.crc = flate::crc32_update(self.crc, b);
        self.size += b.len() as u64;
        match &mut self.deflater {
            Some(d) => d.stream_feed(b, &mut self.out)?,
            None => self.out.extend_from_slice(b),
        }
        self.pending.clear();
        Ok(())
    }

    /// Ends the table and adds it to the archive.
    fn write(mut self, zip: &mut ZipWriter) -> Result<()> {
        self.pending.push_str(SST_TAIL);
        self.flush()?;
        let method = match &mut self.deflater {
            Some(d) => {
                d.stream_finish(&mut self.out)?;
                8
            }
            None => 0,
        };
        zip.add_raw("xl/sharedStrings.xml", RawEntry { method, crc32: self.crc, size: self.size, data: &self.out })?;
        Ok(())
    }
}

enum LinkTarget {
    External(String),
    Location(String),
}

/// The sheet being written.
struct OpenSheet {
    right_to_left: bool,
    outline_levels: u8,
    next_row: u32,
    merges: Vec<CellRange>,
    links: Vec<(u32, u32, LinkTarget)>,
    auto_filter: Option<AutoFilter>,
}

/// The xlsx writer. Calls: [`XlsxWriter::add_style`] (any time), then per sheet
/// [`XlsxWriter::begin_sheet`], [`XlsxWriter::append_row`]… (and [`XlsxWriter::merge`]),
/// [`XlsxWriter::end_sheet`]; finally [`XlsxWriter::finish`] returns the file.
pub struct XlsxWriter {
    opts: WorkbookOptions,
    zip: ZipWriter,
    styles: StylePool,
    sst: Option<Sst>,
    sheets: Vec<SheetMeta>,
    open: Option<OpenSheet>,
    buf: String,
    dropped_links: u64,
    cut_texts: u64,
    rows: u64,
}

const LINK_SCHEMES: [&str; 6] = ["http:", "https:", "mailto:", "ftp:", "ftps:", "tel:"];

fn link_allowed(url: &str) -> bool {
    if url.encode_utf16().count() > MAX_LINK_LEN {
        return false;
    }
    let head = url.get(..url.len().min(8)).unwrap_or("").to_ascii_lowercase();
    LINK_SCHEMES.iter().any(|s| head.starts_with(s))
}

impl XlsxWriter {
    pub fn new(opts: WorkbookOptions) -> Result<Self> {
        let f = &opts.font;
        if !(f.size.is_finite() && (1.0..=409.0).contains(&f.size)) {
            return Err(Error::InvalidSize);
        }
        let units = f.name.encode_utf16().count();
        if units == 0 || units > 31 {
            return Err(Error::InvalidName(f.name.clone()));
        }
        let zip = ZipWriter::new(WriteOptions { level: opts.level, ..WriteOptions::default() });
        let sst = opts.shared_strings.then(|| Sst::new(opts.level));
        Ok(XlsxWriter {
            opts,
            zip,
            styles: StylePool::new(),
            sst,
            sheets: Vec::new(),
            open: None,
            buf: String::with_capacity(FLUSH_AT + 4096),
            dropped_links: 0,
            cut_texts: 0,
            rows: 0,
        })
    }

    /// Interns a style (equal styles get the same id).
    pub fn add_style(&mut self, style: &Style) -> Result<StyleId> {
        self.styles.intern(style)
    }

    /// Starts a sheet (the previous one must be ended).
    pub fn begin_sheet(&mut self, opts: SheetOptions) -> Result<()> {
        if self.open.is_some() {
            return Err(Error::SheetOpen);
        }
        sheet::validate_sheet_name(&opts.name)?;
        let lower = opts.name.to_lowercase();
        if self.sheets.iter().any(|s| s.name.to_lowercase() == lower) {
            return Err(Error::DuplicateSheetName(opts.name));
        }
        sheet::validate_options(&opts)?;
        let n = self.sheets.len() as u32 + 1;
        let mut part = String::from("xl/worksheets/sheet");
        push_u32(&mut part, n);
        part.push_str(".xml");
        self.zip.stream_begin(&part)?;
        self.buf.clear();
        sheet::push_head(&mut self.buf, &opts, self.sheets.is_empty());
        self.open = Some(OpenSheet {
            right_to_left: opts.right_to_left,
            outline_levels: opts.outline_levels,
            next_row: 1,
            merges: Vec::new(),
            links: Vec::new(),
            auto_filter: None,
        });
        self.sheets.push(SheetMeta { name: opts.name, print_titles: opts.print_title_rows, names: opts.names, filter: None });
        Ok(())
    }

    /// Appends the next row (row 1 first); `cells[0]` is column A. Returns the row number.
    pub fn append_row(&mut self, row: &RowOptions, cells: &[Cell<'_>]) -> Result<u32> {
        let Some(sheet) = &mut self.open else { return Err(Error::NoSheetOpen) };
        let r = sheet.next_row;
        if r > MAX_ROWS {
            return Err(Error::TooManyRows);
        }
        if cells.len() > MAX_COLS as usize {
            return Err(Error::TooManyColumns);
        }
        if row.outline_level > sheet.outline_levels {
            return Err(Error::OutlineLevel { level: row.outline_level, declared: sheet.outline_levels });
        }
        if let Some(h) = row.height
            && !(h.is_finite() && (0.0..=409.5).contains(&h))
        {
            return Err(Error::InvalidSize);
        }
        for c in cells {
            if !self.styles.contains(c.style) {
                return Err(Error::UnknownStyle(c.style.0));
            }
            if let CellValue::Date(d) = c.value {
                d.validate()?;
            }
        }
        let rtl = sheet.right_to_left;
        let row_start = self.buf.len();
        let links_before = sheet.links.len();
        let any_attr = row.height.is_some() || row.hidden || row.outline_level > 0;
        sheet::push_row_open(&mut self.buf, r, row);
        let cells_start = self.buf.len();
        for (i, c) in cells.iter().enumerate() {
            let col = i as u32 + 1;
            // what the cell holds, after the conversions
            let mut text: Option<std::borrow::Cow<'_, str>> = None;
            let mut number: Option<f64> = None;
            let mut boolean: Option<bool> = None;
            let mut implied = 0;
            match c.value {
                CellValue::Empty => {}
                CellValue::Text(s) => text = Some(std::borrow::Cow::Borrowed(s)),
                CellValue::Number(x) => number = x.is_finite().then_some(x),
                CellValue::Bool(b) => boolean = Some(b),
                CellValue::Date(d) => match date_serial(&d) {
                    Some(serial) => {
                        number = Some(serial);
                        if !self.styles.has_num_fmt(c.style) {
                            implied = if d.is_midnight() { BUILTIN_SHORT_DATE } else { BUILTIN_DATE_TIME };
                        }
                    }
                    None => text = Some(std::borrow::Cow::Owned(date_text(&d))),
                },
            }
            let link = match c.link {
                None => None,
                Some(Link::Url(u)) if link_allowed(u) && sheet.links.len() < MAX_LINKS_PER_SHEET => Some(LinkTarget::External(u.to_owned())),
                Some(Link::Location(l)) if l.encode_utf16().count() <= MAX_LINK_LEN && !l.is_empty() && sheet.links.len() < MAX_LINKS_PER_SHEET => {
                    Some(LinkTarget::Location(l.to_owned()))
                }
                Some(_) => {
                    self.dropped_links += 1;
                    None
                }
            };
            let empty = text.is_none() && number.is_none() && boolean.is_none();
            if empty && c.style == StyleId::DEFAULT && link.is_none() {
                continue;
            }
            // the only error past the checks above: more than 64,000 cell formats; the row is undone
            let xf = match self.styles.xf(c.style, rtl, implied) {
                Ok(xf) => xf,
                Err(e) => {
                    self.buf.truncate(row_start);
                    sheet.links.truncate(links_before);
                    return Err(e);
                }
            };
            if let Some(l) = link {
                sheet.links.push((r, col, l));
            }
            let o = &mut self.buf;
            o.push_str("<c r=\"");
            push_ref(o, r, col);
            o.push('"');
            if xf != 0 {
                o.push_str(" s=\"");
                push_u32(o, xf);
                o.push('"');
            }
            if let Some(t) = text {
                let full = t.as_ref();
                let t = cut_text(full, MAX_CELL_TEXT);
                if t.len() != full.len() {
                    self.cut_texts += 1;
                }
                match &mut self.sst {
                    Some(sst) => {
                        let idx = sst.get(t)?;
                        o.push_str(" t=\"s\"><v>");
                        push_u32(o, idx);
                        o.push_str("</v></c>");
                    }
                    None => {
                        o.push_str(if needs_preserve(t) { " t=\"inlineStr\"><is><t xml:space=\"preserve\">" } else { " t=\"inlineStr\"><is><t>" });
                        push_xstring(o, t);
                        o.push_str("</t></is></c>");
                    }
                }
            } else if let Some(x) = number {
                o.push_str("><v>");
                push_number(o, x);
                o.push_str("</v></c>");
            } else if let Some(b) = boolean {
                o.push_str(if b { " t=\"b\"><v>1</v></c>" } else { " t=\"b\"><v>0</v></c>" });
            } else {
                o.push_str("/>");
            }
        }
        if self.buf.len() == cells_start {
            if any_attr {
                // `<row …>` → `<row …/>`
                self.buf.pop();
                self.buf.push_str("/>");
            } else {
                self.buf.truncate(row_start);
            }
        } else {
            self.buf.push_str("</row>");
        }
        sheet.next_row += 1;
        self.rows += 1;
        if self.buf.len() >= FLUSH_AT {
            self.zip.stream_write(self.buf.as_bytes())?;
            self.buf.clear();
        }
        Ok(r)
    }

    /// Merges a range of the open sheet (the value of its top-left cell is shown). Ranges may
    /// not overlap.
    pub fn merge(&mut self, range: CellRange) -> Result<()> {
        let Some(sheet) = &mut self.open else { return Err(Error::NoSheetOpen) };
        range.validate()?;
        if sheet.merges.iter().any(|m| m.overlaps(&range)) {
            return Err(Error::OverlappingMerge);
        }
        sheet.merges.push(range);
        Ok(())
    }

    /// Puts filter (and sort) buttons on a range of the open sheet. Call it any time before
    /// [`XlsxWriter::end_sheet`] (the last row is usually known only at the end); calling it again replaces the filter. The sheet also gets the
    /// hidden `_xlnm._FilterDatabase` name Excel and LibreOffice expect. A range outside the sheet, a sort column
    /// outside the range or more than [`crate::MAX_SORT_KEYS`] keys is [`Error::InvalidRange`].
    pub fn set_auto_filter(&mut self, filter: AutoFilter) -> Result<()> {
        let Some(sheet) = &mut self.open else { return Err(Error::NoSheetOpen) };
        filter.validate()?;
        if let Some(meta) = self.sheets.last_mut() {
            meta.filter = Some(filter.range);
        }
        sheet.auto_filter = Some(filter);
        Ok(())
    }

    /// The next row number of the open sheet (1 before the first row).
    pub fn next_row(&self) -> Option<u32> {
        self.open.as_ref().map(|s| s.next_row)
    }

    /// Rows appended so far, all sheets.
    pub fn rows_written(&self) -> u64 {
        self.rows
    }

    /// Bytes of the archive so far (compressed sheets; a progress measure).
    pub fn bytes_written(&self) -> usize {
        self.zip.written_len()
    }

    /// Bytes reserved by the archive buffer (memory accounting: the file grows in one buffer
    /// whose spare capacity is the only memory besides the writer state).
    pub fn archive_capacity(&self) -> usize {
        self.zip.buffer_capacity()
    }

    /// Hyperlinks not written (scheme not allowed, target too long, past the per-sheet limit).
    pub fn dropped_links(&self) -> u64 {
        self.dropped_links
    }

    /// Texts cut to [`MAX_CELL_TEXT`].
    pub fn cut_texts(&self) -> u64 {
        self.cut_texts
    }

    /// Ends the open sheet.
    pub fn end_sheet(&mut self) -> Result<()> {
        let Some(sheet) = self.open.take() else { return Err(Error::NoSheetOpen) };
        let o = &mut self.buf;
        o.push_str("</sheetData>");
        // CT_Worksheet order: autoFilter comes before mergeCells
        if let Some(f) = &sheet.auto_filter {
            f.push_xml(o);
        }
        if !sheet.merges.is_empty() {
            o.push_str("<mergeCells count=\"");
            push_u32(o, sheet.merges.len() as u32);
            o.push_str("\">");
            for m in &sheet.merges {
                o.push_str("<mergeCell ref=\"");
                m.push_a1(o);
                o.push_str("\"/>");
            }
            o.push_str("</mergeCells>");
        }
        let mut rels = String::new();
        if !sheet.links.is_empty() {
            o.push_str("<hyperlinks>");
            let mut rid = 0;
            for (r, c, l) in &sheet.links {
                o.push_str("<hyperlink ref=\"");
                push_ref(o, *r, *c);
                match l {
                    LinkTarget::External(u) => {
                        rid += 1;
                        o.push_str("\" r:id=\"rId");
                        push_u32(o, rid);
                        o.push_str("\"/>");
                        parts::push_link_rel(&mut rels, rid, u);
                    }
                    LinkTarget::Location(loc) => {
                        o.push_str("\" location=\"");
                        uo_opc::xml::write::escape_into(o, loc, uo_opc::xml::write::Escape::ATTR_VALID);
                        o.push_str("\"/>");
                    }
                }
            }
            o.push_str("</hyperlinks>");
        }
        o.push_str("<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>");
        self.zip.stream_write(self.buf.as_bytes())?;
        self.buf.clear();
        self.buf.shrink_to(FLUSH_AT + 4096);
        self.zip.stream_end()?;
        if !rels.is_empty() {
            let mut part = String::from("xl/worksheets/_rels/sheet");
            push_u32(&mut part, self.sheets.len() as u32);
            part.push_str(".xml.rels");
            let mut x = String::with_capacity(rels.len() + 200);
            x.push_str(uo_opc::xml::write::XML_DECL_BARE);
            x.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
            x.push_str(&rels);
            x.push_str("</Relationships>");
            self.zip.add(&part, x.as_bytes())?;
        }
        Ok(())
    }

    /// Ends the open sheet (if any) and returns the xlsx file.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        if self.open.is_some() {
            self.end_sheet()?;
        }
        if self.sheets.is_empty() {
            return Err(Error::NoSheets);
        }
        let sst = match self.sst.take() {
            Some(s) if s.count > 0 => {
                s.write(&mut self.zip)?;
                true
            }
            _ => false,
        };
        let mut size = String::new();
        push_number(&mut size, self.opts.font.size);
        let z = &mut self.zip;
        z.add("xl/styles.xml", self.styles.xml(&self.opts.font.name, &size).as_bytes())?;
        z.add("xl/workbook.xml", parts::workbook_xml(&self.sheets).as_bytes())?;
        z.add("xl/_rels/workbook.xml.rels", parts::workbook_rels(self.sheets.len(), sst).as_bytes())?;
        let o = &self.opts;
        z.add("docProps/core.xml", parts::core_xml(o.title.as_deref(), o.creator.as_deref(), o.language.as_deref(), o.created.as_deref()).as_bytes())?;
        z.add("docProps/app.xml", parts::app_xml(&o.application).as_bytes())?;
        z.add("_rels/.rels", parts::ROOT_RELS.as_bytes())?;
        z.add("[Content_Types].xml", parts::content_types(self.sheets.len(), sst).as_bytes())?;
        Ok(self.zip.finish())
    }
}
