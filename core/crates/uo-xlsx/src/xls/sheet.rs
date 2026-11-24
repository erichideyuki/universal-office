//! The substream of a worksheet (`sheet.ts`, [MS-XLS] 2.1.7.20.5): cells (NUMBER, RK, MULRK, LABELSST, LABEL, RSTRING,
//! BOOLERR, FORMULA + STRING, BLANK, MULBLANK), rows/columns, merges, the window (grid, panes, zoom), hyperlinks,
//! notes (NOTE + OBJ + TXO) and the drawing (MSODRAWING concatenated + OBJ).
//!
//! The cells come out as XML fragments (`<c>`) in ONE arena string with a 16-byte entry each — a sheet of 15 MB has
//! hundreds of thousands of them; the TS keeps an array of strings per row. Entries keep the file order; the sheet XML
//! writer sorts them only when the file was not in order.

use std::collections::{BTreeMap, HashMap};

use uo_jsnum::conv::to_int32;

use super::XlsError;
use super::biff::{Codepage, FormatRun, Rd, Records, RichString, read_chars, read_xl_string, rt};
use super::cf::{CondFormatOut, apply_cf_ex, read_cf, read_cf12, read_cond_fmt};
use super::dv::read_dv;
use super::formula::{FormulaContext, decompile_into};
use super::globals::XlsGlobals;
use super::xmlw::{cell_ref, col_name, push_i, push_u, xml_text};

/// Last row/column of the .xlsx grid (references past them are dropped).
pub const MAX_ROW: u32 = 1_048_576;
pub const MAX_COL: u32 = 16_384;
const TABLE_RT: u16 = 0x0236;
const CALCCOUNT_RT: u16 = 0x000c;
const ITERATION_RT: u16 = 0x0011;
const DELTA_RT: u16 = 0x0010;
/// `'../'` repeats of a file hyperlink (`cAnti`): Excel stops far below this; the cap bounds the output of a hostile record.
const MAX_ANTI: usize = 1024;

/// A rectangle 0-based inclusive `[r1, c1, r2, c2]`.
pub type Rect = [u32; 4];

/// The value of a cell kept for the charts (the `.xls` rarely carries the series cache).
#[derive(Clone, Debug)]
pub enum CellVal {
    Num(f64),
    Str(String),
}

#[derive(Clone, Debug)]
pub struct CellValue {
    pub v: CellVal,
    pub xf: u16,
}

/// The shared string table: the SST + the strings of LABEL/RSTRING/formula results (BIFF5).
pub struct StringTable {
    pub items: Vec<RichString>,
    plain: HashMap<String, usize>,
}

impl StringTable {
    pub fn new(sst: Vec<RichString>) -> Self {
        StringTable { items: sst, plain: HashMap::new() }
    }

    /// `add`: the index of a string; plain ones (no runs) are shared.
    pub fn add(&mut self, s: RichString) -> usize {
        if s.runs.is_empty() {
            if let Some(&k) = self.plain.get(&s.text) {
                return k;
            }
            self.plain.insert(s.text.clone(), self.items.len());
        }
        self.items.push(s);
        self.items.len() - 1
    }
}

/// One cell fragment in the arena.
#[derive(Clone, Copy, Debug)]
pub struct CellOut {
    pub row: u16,
    pub col: u16,
    pub xf: u16,
    pub off: u32,
    pub len: u32,
}

/// The cells of a sheet: the arena of XML fragments and one entry per `put`.
#[derive(Default)]
pub struct Cells {
    pub arena: String,
    pub list: Vec<CellOut>,
    /// `put` entries whose `<f>` is inserted later (the first cell of a shared/array formula): index → the `<f>` XML.
    pub patches: HashMap<u32, String>,
    sorted: bool,
    last_key: i64,
    /// entry index of the LAST cell at (row, col), kept only for the cells waiting for their SHRFMLA/ARRAY/TABLE
    last_idx: HashMap<(u32, u32), u32>,
    /// the order of the entries by (row, col) (stable) when the file was not in order
    pub order: Option<Vec<u32>>,
}

#[allow(clippy::len_without_is_empty)]
impl Cells {
    fn new() -> Self {
        Cells { sorted: true, last_key: -1, ..Cells::default() }
    }

    fn begin(&self) -> usize {
        self.arena.len()
    }

    fn end(&mut self, start: usize, row: u32, col: u32, xf: u32) -> Result<(), XlsError> {
        let idx = u32::try_from(self.list.len()).map_err(|_| XlsError::Limit("cells"))?;
        let len = self.arena.len() - start;
        let off = u32::try_from(start).map_err(|_| XlsError::Limit("sheet size"))?;
        let key = (i64::from(row) << 16) | i64::from(col);
        if key <= self.last_key {
            self.sorted = false;
        }
        self.last_key = key;
        if !self.last_idx.is_empty()
            && let Some(e) = self.last_idx.get_mut(&(row, col))
        {
            *e = idx;
        }
        self.list.push(CellOut {
            row: u16::try_from(row).unwrap_or(u16::MAX),
            col: u16::try_from(col).unwrap_or(u16::MAX),
            xf: u16::try_from(xf).unwrap_or(u16::MAX),
            off,
            len: u32::try_from(len).map_err(|_| XlsError::Limit("cell size"))?,
        });
        Ok(())
    }

    /// Frees the arena and the entries (the sheet XML has been written).
    pub fn release(&mut self) {
        *self = Cells::new();
    }

    /// The XML of an entry with its patch applied.
    pub fn write_cell(&self, i: usize, out: &mut String) {
        let Some(c) = self.list.get(i) else { return };
        let (a, b) = (c.off as usize, (c.off + c.len) as usize);
        let xml = self.arena.get(a..b).unwrap_or("");
        if let Some(fx) = self.patches.get(&u32::try_from(i).unwrap_or(u32::MAX)) {
            // `acc.xml[i].replace('><v>', `>${fx}<v>`)` when the fragment has no `<f` yet
            if !xml.contains("<f")
                && let Some(k) = xml.find("><v>")
            {
                out.push_str(&xml[..=k]);
                out.push_str(fx);
                out.push_str(&xml[k + 1..]);
                return;
            }
        }
        out.push_str(xml);
    }

    /// Orders the entries by (row, column), stable (a no-op for a file in order).
    fn finish(&mut self) {
        if !self.sorted {
            let mut order: Vec<u32> = (0..u32::try_from(self.list.len()).unwrap_or(u32::MAX)).collect();
            order.sort_by_key(|&i| {
                let c = &self.list[i as usize];
                (c.row, c.col)
            });
            self.order = Some(order);
        }
    }

    /// The entry at position `k` of the (row, col) order.
    pub fn at(&self, k: usize) -> Option<&CellOut> {
        match &self.order {
            Some(o) => o.get(k).and_then(|&i| self.list.get(i as usize)),
            None => self.list.get(k),
        }
    }

    pub fn index_at(&self, k: usize) -> usize {
        match &self.order {
            Some(o) => o.get(k).map_or(0, |&i| i as usize),
            None => k,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The XFs of every entry of a row (duplicates included), in the (row, col) order.
    pub fn row_xfs(&self, row: u32) -> impl Iterator<Item = u16> + '_ {
        let n = self.list.len();
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.at(mid).is_some_and(|c| u32::from(c.row) < row) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (lo..n).map_while(move |k| self.at(k).filter(|c| u32::from(c.row) == row).map(|c| c.xf))
    }
}

#[derive(Clone, Debug, Default)]
pub struct RowInfo {
    /// Height in twips.
    pub height: Option<u32>,
    pub custom: bool,
    pub hidden: bool,
    /// The row's default XF (fGhostDirty).
    pub xf: Option<u32>,
    /// fExAsc / fExDes: thick border on top / bottom.
    pub thick_top: bool,
    pub thick_bot: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ColInfo {
    pub first: u32,
    pub last: u32,
    pub width: u32,
    pub xf: u32,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HyperlinkInfo {
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
    pub url: Option<String>,
    pub location: Option<String>,
    pub display: Option<String>,
    pub tooltip: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NoteInfo {
    pub row: u32,
    pub col: u32,
    pub obj_id: i32,
    pub author: String,
    pub text: Option<String>,
}

/// An object (OBJ) with the offset of the drawing up to it (links it to the OfficeArtClientData).
#[derive(Clone, Debug)]
pub struct ObjInfo {
    pub ot: u16,
    pub id: u16,
    /// Size of the concatenated drawing when the OBJ appeared.
    pub drawing_at: usize,
    /// The text of the TXO that follows the object (text boxes/notes).
    pub text: Option<RichString>,
    pub txo_align: Option<u16>,
    pub txo_rot: Option<u16>,
    /// The records of the embedded chart substream (BOF dt=0x20 … EOF) after the OBJ: a range of the stream.
    pub chart: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Frozen {
    pub x: u32,
    pub y: u32,
    pub top: u32,
    pub left: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CalcOpts {
    pub mode: Option<i16>,
    pub count: Option<u16>,
    pub iterate: Option<bool>,
    pub delta: Option<f64>,
}

pub struct XlsSheet {
    pub cells: Cells,
    pub rows: BTreeMap<u32, RowInfo>,
    pub cols: Vec<ColInfo>,
    pub def_col_width: Option<u32>,
    pub std_width: Option<u32>,
    pub def_row_height: Option<u32>,
    pub def_row_hidden: bool,
    pub def_row_custom: bool,
    pub merges: Vec<[u32; 4]>,
    pub show_grid: bool,
    pub show_zeros: bool,
    pub show_formulas: bool,
    pub right_to_left: bool,
    pub selected: bool,
    pub zoom: Option<u32>,
    pub frozen: Option<Frozen>,
    pub hyperlinks: Vec<HyperlinkInfo>,
    pub notes: Vec<NoteInfo>,
    pub objs: Vec<ObjInfo>,
    pub drawing: Option<Vec<u8>>,
    /// The sheet's own records when it is a chart sheet: a range of the stream.
    pub chart_records: Option<(usize, usize)>,
    pub has_auto_filter: bool,
    pub auto_filter: Option<Rect>,
    pub data_validations: Vec<String>,
    pub cond_formats: Vec<CondFormatOut>,
    pub calc: Option<CalcOpts>,
}

/// `ERRORS` of the cells (`#GETTING_DATA` included).
fn cell_error(code: u8) -> &'static str {
    match code {
        0x00 => "#NULL!",
        0x07 => "#DIV/0!",
        0x0f => "#VALUE!",
        0x17 => "#REF!",
        0x1d => "#NAME?",
        0x24 => "#NUM!",
        0x2b => "#GETTING_DATA",
        _ => "#N/A",
    }
}

/// `rkValue` ([MS-XLS] 2.5.217): a 30-bit integer or the high 30 bits of a double, optionally ÷100.
pub fn rk_value(rk: u32) -> f64 {
    let v =
        if rk & 2 != 0 { f64::from(i32::from_le_bytes(rk.to_le_bytes()) >> 2) } else { f64::from_bits(u64::from(rk & 0xffff_fffc) << 32) };
    if rk & 1 != 0 { v / 100.0 } else { v }
}

/// `num`: the text of a `<v>` (exact round trip of the double).
fn push_num(out: &mut String, v: f64) {
    if !v.is_finite() {
        out.push('0');
    } else if v.fract() == 0.0 && v.abs() < 2_147_483_648.0 {
        push_i(out, i64::from(to_int32(v)));
    } else {
        out.push_str(&uo_jsnum::format::to_js_string(v));
    }
}

fn is_cell_record(t: u16) -> bool {
    matches!(
        t,
        rt::NUMBER
            | rt::RK
            | rt::MULRK
            | rt::LABELSST
            | rt::LABEL
            | rt::RSTRING
            | rt::BOOLERR
            | rt::FORMULA
            | rt::BLANK
            | rt::MULBLANK
            | rt::ROW
    )
}

/// A shared/array/table formula definition.
struct SharedF {
    r1: u32,
    r2: u32,
    c1: u32,
    c2: u32,
    data: Vec<u8>,
    off: usize,
    cce: usize,
    array: bool,
    table: Option<String>,
}

struct Pending {
    row: u32,
    col: u32,
    xf: u32,
    fx: String,
    exp_key: Option<(u32, u32)>,
}

/// The state of the reading of one sheet.
struct Reader<'g, 'a> {
    g: &'g XlsGlobals,
    b8: bool,
    sh: XlsSheet,
    fctx: FormulaContext<'g>,
    shared: HashMap<(u32, u32), SharedF>,
    waiting: HashMap<(u32, u32), Vec<(u32, u32)>>,
    wanted: Option<&'a [Rect]>,
    wanted_box: Rect,
    drawing: Vec<u8>,
    has_drawing: bool,
    /// the values of the cells referenced by charts (key row * 16384 + column)
    values: HashMap<u64, CellValue>,
    /// scratch: the `<f>` being built and its text
    fx: String,
    ftext: String,
}

impl<'g, 'a> Reader<'g, 'a> {
    fn keep(&mut self, row: u32, col: u32, v: CellVal, xf: u16) {
        let Some(w) = self.wanted else { return };
        let bb = self.wanted_box;
        if row < bb[0] || row > bb[2] || col < bb[1] || col > bb[3] {
            return;
        }
        for &[r1, c1, r2, c2] in w {
            if row >= r1 && row <= r2 && col >= c1 && col <= c2 {
                self.values.insert(u64::from(row) * 16384 + u64::from(col), CellValue { v, xf });
                return;
            }
        }
    }

    /// The ` s="…"` attribute of an XF.
    fn s_attr(&self, out: &mut String, xf: u32) {
        if xf > 0 && usize::try_from(xf).unwrap_or(usize::MAX) < self.g.xfs.len() {
            out.push_str(" s=\"");
            push_u(out, u64::from(xf));
            out.push('"');
        }
    }

    /// `<c r="A1" s="n"` (the start of a cell, not closed) into `h`.
    fn cell_head(&self, h: &mut String, row: u32, col: u32, xf: u32) {
        h.push_str("<c r=\"");
        cell_ref(h, row, col);
        h.push('"');
        self.s_attr(h, xf);
    }

    fn cell_num(&mut self, row: u32, col: u32, xf: u32, v: f64) -> Result<(), XlsError> {
        if row >= MAX_ROW || col >= MAX_COL {
            return Ok(());
        }
        let st = self.sh.cells.begin();
        let mut h = std::mem::take(&mut self.sh.cells.arena);
        h.push_str("<c r=\"");
        cell_ref(&mut h, row, col);
        h.push('"');
        self.s_attr(&mut h, xf);
        h.push_str("><v>");
        push_num(&mut h, v);
        h.push_str("</v></c>");
        self.sh.cells.arena = h;
        self.sh.cells.end(st, row, col, xf)?;
        self.keep(row, col, CellVal::Num(v), u16::try_from(xf).unwrap_or(u16::MAX));
        Ok(())
    }

    fn cell_str(&mut self, strings: &mut StringTable, row: u32, col: u32, xf: u32, s: RichString) -> Result<(), XlsError> {
        if row >= MAX_ROW || col >= MAX_COL {
            return Ok(());
        }
        let text = s.text.clone();
        let idx = strings.add(s);
        self.put_shared_ref(row, col, xf, idx)?;
        self.keep(row, col, CellVal::Str(text), u16::try_from(xf).unwrap_or(u16::MAX));
        Ok(())
    }

    /// `<c r s t="s"><v>idx</v></c>`.
    fn put_shared_ref(&mut self, row: u32, col: u32, xf: u32, idx: usize) -> Result<(), XlsError> {
        let st = self.sh.cells.begin();
        let mut h = std::mem::take(&mut self.sh.cells.arena);
        h.push_str("<c r=\"");
        cell_ref(&mut h, row, col);
        h.push('"');
        self.s_attr(&mut h, xf);
        h.push_str(" t=\"s\"><v>");
        push_u(&mut h, idx as u64);
        h.push_str("</v></c>");
        self.sh.cells.arena = h;
        self.sh.cells.end(st, row, col, xf)
    }

    fn cell_blank(&mut self, row: u32, col: u32, xf: u32) -> Result<(), XlsError> {
        if row >= MAX_ROW || col >= MAX_COL {
            return Ok(());
        }
        let st = self.sh.cells.begin();
        let mut h = std::mem::take(&mut self.sh.cells.arena);
        h.push_str("<c r=\"");
        cell_ref(&mut h, row, col);
        h.push('"');
        self.s_attr(&mut h, xf);
        h.push_str("/>");
        self.sh.cells.arena = h;
        self.sh.cells.end(st, row, col, xf)
    }

    /// `fXml`: the `<f>` of a cell — direct, shared (ptgExp → SHRFMLA) or array (ARRAY) — written into `self.fx` (cleared first);
    /// `false` = `""`.
    fn f_xml(&mut self, row: u32, col: u32, exp_key: Option<(u32, u32)>, r: Option<&Rd<'_>>) -> bool {
        self.fx.clear();
        if !self.b8 {
            return false;
        }
        let ok: bool;
        if let Some(key) = exp_key {
            let Some(s) = self.shared.get(&key) else { return false };
            if row < s.r1 || row > s.r2 || col < s.c1 || col > s.c2 {
                return false;
            }
            // data table: the <f> goes only in the first cell of the interior
            if let Some(t) = &s.table {
                if row == s.r1 && col == s.c1 {
                    self.fx.push_str(t);
                    return true;
                }
                return false;
            }
            self.fctx.row = row;
            self.fctx.col = col;
            // shared: relative ptgRef3d/ptgArea3d hold an offset
            self.fctx.rel3d = !s.array;
            ok = decompile_into(&Rd(&s.data), s.off, s.cce, &self.fctx, &mut self.ftext);
            self.fctx.rel3d = false;
            if s.array {
                if ok && row == s.r1 && col == s.c1 {
                    let x = &mut self.fx;
                    x.push_str("<f t=\"array\" ref=\"");
                    col_name(x, s.c1);
                    push_u(x, u64::from(s.r1) + 1);
                    x.push(':');
                    col_name(x, s.c2);
                    push_u(x, u64::from(s.r2) + 1);
                    x.push_str("\">");
                    xml_text(x, &self.ftext);
                    x.push_str("</f>");
                    return true;
                }
                return false;
            }
        } else if let Some(r) = r {
            self.fctx.row = row;
            self.fctx.col = col;
            ok = decompile_into(r, 22, usize::from(r.u16(20)), &self.fctx, &mut self.ftext);
        } else {
            return false;
        }
        if ok {
            self.fx.push_str("<f>");
            xml_text(&mut self.fx, &self.ftext);
            self.fx.push_str("</f>");
        }
        ok
    }

    /// `patchF`: inserts the `<f>` into a cell already written (the first of a shared/array formula).
    fn patch_f(&mut self, row: u32, col: u32, fx: String) {
        if fx.is_empty() {
            return;
        }
        if let Some(&idx) = self.sh.cells.last_idx.get(&(row, col)) {
            // the first patch wins: a second one finds the `<f` of the first (`includes('<f')`)
            self.sh.cells.patches.entry(idx).or_insert(fx);
        }
    }

    /// Registers the last written cell as waiting for the definition of `key`.
    fn wait_for(&mut self, key: (u32, u32), row: u32, col: u32) {
        let idx = u32::try_from(self.sh.cells.list.len().saturating_sub(1)).unwrap_or(0);
        self.sh.cells.last_idx.insert((row, col), idx);
        self.waiting.entry(key).or_default().push((row, col));
    }

    fn flush_formula(&mut self, p: Pending, text: Option<&str>) -> Result<(), XlsError> {
        let Pending { row, col, xf, exp_key, .. } = p;
        // shared: the SHRFMLA may have arrived between the FORMULA and the STRING
        let mut fx = p.fx;
        if fx.is_empty()
            && let Some(k) = exp_key
            && self.f_xml(row, col, Some(k), None)
        {
            fx = std::mem::take(&mut self.fx);
        }
        if row >= MAX_ROW || col >= MAX_COL {
            return Ok(());
        }
        let st = self.sh.cells.begin();
        let mut h = std::mem::take(&mut self.sh.cells.arena);
        self.cell_head(&mut h, row, col, xf);
        h.push_str(" t=\"str\">");
        h.push_str(&fx);
        h.push_str("<v>");
        xml_text(&mut h, text.unwrap_or(""));
        h.push_str("</v></c>");
        self.sh.cells.arena = h;
        self.sh.cells.end(st, row, col, xf)?;
        if fx.is_empty()
            && let Some(k) = exp_key
            && !self.shared.contains_key(&k)
        {
            self.wait_for(k, row, col);
        }
        if let Some(t) = text
            && !t.is_empty()
        {
            self.keep(row, col, CellVal::Str(t.to_owned()), u16::try_from(xf).unwrap_or(u16::MAX));
        }
        Ok(())
    }

    fn add_drawing(&mut self, d: &[u8]) {
        self.drawing.extend_from_slice(d);
        self.has_drawing = true;
    }
}

/// `readSheet`: reads the substream of a sheet (from the record AFTER the BOF). `is_chart`: a chart sheet (its records
/// go raw to the chart converter). `wanted`: areas referenced by chart series — the values of those cells are kept and come back
/// beside the sheet.
pub fn read_sheet(
    recs: &mut Records<'_>,
    g: &XlsGlobals,
    strings: &mut StringTable,
    dxfs: &mut Vec<String>,
    is_chart: bool,
    wanted: Option<&[Rect]>,
    max_arena: usize,
) -> Result<(XlsSheet, HashMap<u64, CellValue>), XlsError> {
    let b8 = g.biff8;
    let chart_start = recs.offset();
    let wanted = wanted.filter(|w| !w.is_empty());
    let wanted_box = wanted.map_or([0; 4], |w| {
        w.iter().fold([u32::MAX, u32::MAX, 0, 0], |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])])
    });
    let sh = XlsSheet {
        cells: Cells::new(),
        rows: BTreeMap::new(),
        cols: Vec::new(),
        def_col_width: None,
        std_width: None,
        def_row_height: None,
        def_row_hidden: false,
        def_row_custom: false,
        merges: Vec::new(),
        show_grid: true,
        show_zeros: true,
        show_formulas: false,
        right_to_left: false,
        selected: false,
        zoom: None,
        frozen: None,
        hyperlinks: Vec::new(),
        notes: Vec::new(),
        objs: Vec::new(),
        drawing: None,
        chart_records: None,
        has_auto_filter: false,
        auto_filter: None,
        data_validations: Vec::new(),
        cond_formats: Vec::new(),
        calc: None,
    };
    let mut rd = Reader {
        g,
        b8,
        sh,
        fctx: FormulaContext::new(g),
        shared: HashMap::new(),
        waiting: HashMap::new(),
        wanted,
        wanted_box,
        drawing: Vec::new(),
        has_drawing: false,
        values: HashMap::new(),
        fx: String::new(),
        ftext: String::new(),
    };
    // the cell of the last FORMULA: the SHRFMLA/ARRAY/TABLE follows the cell that owns it, and the ptgExp of the others
    // points at IT — not at the corner of the area (measured on the corpus: area B42:L50 with owner J42)
    let mut last_formula: Option<(u32, u32)> = None;
    let mut pending: Option<Pending> = None;
    let mut last_obj: Option<usize> = None;
    let mut cur_cf: Option<usize> = None;
    let mut cf_by_nid: Vec<(u32, usize)> = Vec::new();
    let mut cf_base = (0u32, 0u32);
    let mut cf_priority = 0u32;
    let mut last_link: Option<usize> = None;
    let mut last_note5: Option<usize> = None;
    let mut depth = 0u32;
    let mut chart_open: Option<usize> = None; // the OBJ whose nested chart is being read
    let mut chart_end = chart_start;
    let cp: Codepage = g.cp;

    while let Some(rec) = recs.next() {
        if is_chart {
            chart_end = recs.offset();
        }
        // nested substream (embedded chart): its records belong to the previous OBJ. The TS fills the array as the records
        // come, so a substream whose EOF never arrives still gives its records up to the end of the stream.
        if rec.ty == rt::BOF {
            depth += 1;
            if depth == 1 {
                if let Some(oi) = last_obj
                    && rd.sh.objs.get(oi).is_some_and(|o| o.ot == 5 && o.chart.is_none())
                {
                    chart_open = Some(oi);
                    if let Some(o) = rd.sh.objs.get_mut(oi) {
                        o.chart = Some((rec.pos, recs.offset()));
                    }
                }
            } else if let Some(o) = chart_open.and_then(|oi| rd.sh.objs.get_mut(oi)) {
                o.chart = o.chart.map(|(s, _)| (s, recs.offset()));
            }
            continue;
        }
        if depth > 0 {
            if let Some(o) = chart_open.and_then(|oi| rd.sh.objs.get_mut(oi)) {
                o.chart = o.chart.map(|(s, _)| (s, recs.offset()));
            }
            if rec.ty == rt::EOF {
                depth -= 1;
                if depth == 0 {
                    chart_open = None;
                }
            }
            continue;
        }
        if rec.ty == rt::EOF {
            break;
        }
        if rec.ty != rt::STRING
            && is_cell_record(rec.ty)
            && let Some(p) = pending.take()
        {
            rd.flush_formula(p, Some(""))?;
        }
        if rd.sh.cells.arena.len() > max_arena {
            return Err(XlsError::Limit("sheet size"));
        }
        let r = rec.rd();
        match rec.ty {
            rt::NUMBER => rd.cell_num(u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)), r.f64(6))?,
            rt::RK => rd.cell_num(u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)), rk_value(r.u32(6)))?,
            rt::MULRK => {
                let row = u32::from(r.u16(0));
                let mut col = u32::from(r.u16(2));
                let mut o = 4usize;
                while o + 8 <= r.len() {
                    rd.cell_num(row, col, u32::from(r.u16(o)), rk_value(r.u32(o + 2)))?;
                    o += 6;
                    col += 1;
                }
            }
            rt::LABELSST => {
                let (row, col, xf) = (u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)));
                let i = usize::try_from(r.u32(6)).unwrap_or(usize::MAX);
                if row >= MAX_ROW || col >= MAX_COL {
                    continue;
                }
                let idx = if i < strings.items.len() { i } else { strings.add(RichString::default()) };
                rd.put_shared_ref(row, col, xf, idx)?;
                let text = strings.items.get(i).map(|s| s.text.clone()).unwrap_or_default();
                rd.keep(row, col, CellVal::Str(text), u16::try_from(xf).unwrap_or(u16::MAX));
            }
            rt::LABEL => {
                let (text, _) = read_xl_string(&r, 6, b8, &cp, 2, &rec.conts);
                rd.cell_str(strings, u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)), RichString { text, runs: Vec::new() })?;
            }
            rt::RSTRING => {
                // BIFF5/7: cch u16 + bytes + cRuns u8 + (ich u8, ifnt u8)…; BIFF8: XLUnicodeString + cRuns u16 + (ich u16, ifnt u16)…
                let (text, next) = read_xl_string(&r, 6, b8, &cp, 2, &rec.conts);
                let mut runs: Vec<FormatRun> = Vec::new();
                let mut p = next;
                if b8 {
                    let n = usize::from(r.u16(p));
                    p += 2;
                    let mut i = 0;
                    while i < n && p + 4 <= r.len() {
                        runs.push(FormatRun { ich: r.u16(p), ifnt: r.u16(p + 2) });
                        i += 1;
                        p += 4;
                    }
                } else {
                    let n = usize::from(r.u8(p));
                    p += 1;
                    let mut i = 0;
                    while i < n && p + 2 <= r.len() {
                        runs.push(FormatRun { ich: u16::from(r.u8(p)), ifnt: u16::from(r.u8(p + 1)) });
                        i += 1;
                        p += 2;
                    }
                }
                rd.cell_str(strings, u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)), RichString { text, runs })?;
            }
            rt::BOOLERR => {
                let (row, col, xf) = (u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)));
                let v = r.u8(6);
                if row >= MAX_ROW || col >= MAX_COL {
                    continue;
                }
                let st = rd.sh.cells.begin();
                let mut h = std::mem::take(&mut rd.sh.cells.arena);
                rd.cell_head(&mut h, row, col, xf);
                if r.u8(7) != 0 {
                    h.push_str(" t=\"e\"><v>");
                    h.push_str(cell_error(v));
                } else {
                    h.push_str(" t=\"b\"><v>");
                    h.push(if v != 0 { '1' } else { '0' });
                }
                h.push_str("</v></c>");
                rd.sh.cells.arena = h;
                rd.sh.cells.end(st, row, col, xf)?;
            }
            rt::FORMULA => {
                let (row, col, xf) = (u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)));
                if row >= MAX_ROW || col >= MAX_COL {
                    continue;
                }
                // ptgExp (shared/array) or ptgTbl (data table) → the master cell
                let exp_key =
                    if b8 && (r.u8(22) == 0x01 || r.u8(22) == 0x02) { Some((u32::from(r.u16(23)), u32::from(r.u16(25)))) } else { None };
                last_formula = Some((row, col));
                let has_fx = rd.f_xml(row, col, exp_key, Some(&r));
                // fAlwaysCalc (ca): Excel recalculates the cell on open
                if has_fx && r.u16(14) & 1 != 0 && !rd.fx.contains("t=\"dataTable\"") {
                    rd.fx.insert_str(2, " ca=\"1\"");
                }
                if r.u16(12) == 0xffff && r.u8(6) == 0 {
                    pending = Some(Pending { row, col, xf, fx: std::mem::take(&mut rd.fx), exp_key });
                    continue;
                }
                let st = rd.sh.cells.begin();
                let mut h = std::mem::take(&mut rd.sh.cells.arena);
                rd.cell_head(&mut h, row, col, xf);
                let mut num: Option<f64> = None;
                if r.u16(12) == 0xffff {
                    let kind = r.u8(6);
                    if kind == 1 {
                        h.push_str(" t=\"b\">");
                        h.push_str(&rd.fx);
                        h.push_str("<v>");
                        h.push(if r.u8(8) != 0 { '1' } else { '0' });
                        h.push_str("</v></c>");
                    } else if kind == 2 {
                        h.push_str(" t=\"e\">");
                        h.push_str(&rd.fx);
                        h.push_str("<v>");
                        h.push_str(cell_error(r.u8(8)));
                        h.push_str("</v></c>");
                    } else {
                        h.push_str(" t=\"str\">");
                        h.push_str(&rd.fx);
                        h.push_str("<v></v></c>");
                    }
                } else {
                    let v = r.f64(6);
                    h.push('>');
                    h.push_str(&rd.fx);
                    h.push_str("<v>");
                    push_num(&mut h, v);
                    h.push_str("</v></c>");
                    num = Some(v);
                }
                rd.sh.cells.arena = h;
                rd.sh.cells.end(st, row, col, xf)?;
                if let Some(v) = num {
                    rd.keep(row, col, CellVal::Num(v), u16::try_from(xf).unwrap_or(u16::MAX));
                }
                if !has_fx
                    && let Some(k) = exp_key
                    && !rd.shared.contains_key(&k)
                {
                    rd.wait_for(k, row, col);
                }
            }
            rt::CALCMODE => rd.sh.calc.get_or_insert_with(CalcOpts::default).mode = Some(r.i16(0)),
            CALCCOUNT_RT => rd.sh.calc.get_or_insert_with(CalcOpts::default).count = Some(r.u16(0)),
            ITERATION_RT => rd.sh.calc.get_or_insert_with(CalcOpts::default).iterate = Some(r.u16(0) != 0),
            DELTA_RT => rd.sh.calc.get_or_insert_with(CalcOpts::default).delta = Some(r.f64(0)),
            TABLE_RT => {
                if !b8 {
                    continue;
                }
                // RefU of the interior + flags + input cells ([MS-XLS] Table)
                let (r1, r2, c1, c2) = (u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u8(4)), u32::from(r.u8(5)));
                let fl = r.u16(6);
                let cell = |a: &mut String, rw: u32, cl: u32| cell_ref(a, rw, cl);
                let tbl2 = fl & 0x08 != 0;
                let mut a = String::from(" t=\"dataTable\" ref=\"");
                cell(&mut a, r1, c1);
                a.push(':');
                cell(&mut a, r2, c2);
                a.push('"');
                if tbl2 {
                    a.push_str(" dt2D=\"1\"");
                }
                if fl & 0x04 != 0 {
                    a.push_str(" dtr=\"1\"");
                }
                if fl & 0x10 != 0 {
                    a.push_str(" del1=\"1\"");
                }
                if fl & 0x20 != 0 {
                    a.push_str(" del2=\"1\"");
                }
                a.push_str(" r1=\"");
                cell(&mut a, u32::from(r.u16(8)), u32::from(r.u16(10)));
                a.push('"');
                if tbl2 {
                    a.push_str(" r2=\"");
                    cell(&mut a, u32::from(r.u16(12)), u32::from(r.u16(14)));
                    a.push('"');
                }
                let key = last_formula.unwrap_or((r1, c1));
                rd.shared.insert(
                    key,
                    SharedF { r1, r2, c1, c2, data: Vec::new(), off: 0, cce: 0, array: false, table: Some(format!("<f{a}/>")) },
                );
                for (wr, wc) in rd.waiting.remove(&key).unwrap_or_default() {
                    if rd.f_xml(wr, wc, Some(key), None) {
                        let fx = std::mem::take(&mut rd.fx);
                        rd.patch_f(wr, wc, fx);
                    }
                }
            }
            rt::SHRFMLA | rt::ARRAY => {
                if !b8 {
                    continue;
                }
                // RefU (rwFirst, rwLast u16; colFirst, colLast u8) + … + cce + rgce
                let (r1, r2, c1, c2) = (u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u8(4)), u32::from(r.u8(5)));
                let array = rec.ty == rt::ARRAY;
                let cce_off = if array { 12 } else { 8 };
                let key = last_formula.unwrap_or((r1, c1));
                rd.shared.insert(
                    key,
                    SharedF { r1, r2, c1, c2, data: r.0.to_vec(), off: cce_off + 2, cce: usize::from(r.u16(cce_off)), array, table: None },
                );
                for (wr, wc) in rd.waiting.remove(&key).unwrap_or_default() {
                    if rd.f_xml(wr, wc, Some(key), None) {
                        let fx = std::mem::take(&mut rd.fx);
                        rd.patch_f(wr, wc, fx);
                    }
                }
            }
            rt::STRING => {
                if pending.is_none() {
                    continue;
                }
                let (text, _) =
                    if b8 { read_xl_string(&r, 0, true, &cp, 2, &rec.conts) } else { read_xl_string(&r, 0, false, &cp, 2, &[]) };
                if let Some(p) = pending.take() {
                    rd.flush_formula(p, Some(&text))?;
                }
            }
            rt::BLANK => rd.cell_blank(u32::from(r.u16(0)), u32::from(r.u16(2)), u32::from(r.u16(4)))?,
            rt::MULBLANK => {
                let row = u32::from(r.u16(0));
                let mut col = u32::from(r.u16(2));
                let mut o = 4usize;
                while o + 4 <= r.len() {
                    rd.cell_blank(row, col, u32::from(r.u16(o)))?;
                    o += 2;
                    col += 1;
                }
            }
            rt::ROW => {
                let rw = u32::from(r.u16(0));
                let miy = r.u16(6);
                let fl = r.u32(12);
                let mut info = RowInfo { custom: fl & 0x40 != 0, hidden: fl & 0x20 != 0, ..RowInfo::default() };
                let h = u32::from(miy & 0x7fff);
                if miy & 0x8000 == 0 || h > 0 {
                    info.height = Some(h);
                }
                if fl & 0x80 != 0 {
                    info.xf = Some((fl >> 16) & 0xfff);
                }
                if fl & 0x1000_0000 != 0 {
                    info.thick_top = true;
                }
                if fl & 0x2000_0000 != 0 {
                    info.thick_bot = true;
                }
                if rw < MAX_ROW {
                    rd.sh.rows.insert(rw, info);
                }
            }
            rt::DEFAULTROWHEIGHT => {
                if r.len() >= 4 {
                    let fl = r.u16(0);
                    rd.sh.def_row_height = Some(u32::from(r.u16(2)));
                    rd.sh.def_row_hidden = fl & 2 != 0;
                    rd.sh.def_row_custom = fl & 1 != 0;
                } else {
                    rd.sh.def_row_height = Some(u32::from(r.u16(0) & 0x7fff));
                }
            }
            rt::DEFCOLWIDTH => rd.sh.def_col_width = Some(u32::from(r.u16(0))),
            rt::STANDARDWIDTH => rd.sh.std_width = Some(u32::from(r.u16(0))),
            rt::COLINFO => {
                let (first, last) = (u32::from(r.u16(0)), u32::from(r.u16(2)));
                if first > last || first >= MAX_COL {
                    continue;
                }
                rd.sh.cols.push(ColInfo {
                    first,
                    last: last.min(MAX_COL - 1),
                    width: u32::from(r.u16(4)),
                    xf: u32::from(r.u16(6)),
                    hidden: r.u16(8) & 1 != 0,
                });
            }
            rt::MERGECELLS => {
                let n = usize::from(r.u16(0));
                let mut i = 0;
                let mut o = 2usize;
                while i < n && o + 8 <= r.len() {
                    let (r1, r2, c1, c2) = (u32::from(r.u16(o)), u32::from(r.u16(o + 2)), u32::from(r.u16(o + 4)), u32::from(r.u16(o + 6)));
                    i += 1;
                    o += 8;
                    if r2 < r1 || c2 < c1 || (r1 == r2 && c1 == c2) {
                        continue;
                    }
                    rd.sh.merges.push([r1, c1, r2.min(MAX_ROW - 1), c2.min(MAX_COL - 1)]);
                }
            }
            rt::WINDOW2 => {
                let fl = r.u16(0);
                rd.sh.show_formulas = fl & 0x01 != 0;
                rd.sh.show_grid = fl & 0x02 != 0;
                rd.sh.show_zeros = fl & 0x10 != 0;
                rd.sh.right_to_left = fl & 0x40 != 0;
                rd.sh.selected = fl & 0x200 != 0;
                if fl & 0x08 != 0 {
                    rd.sh.frozen = Some(Frozen { x: 0, y: 0, top: u32::from(r.u16(2)), left: u32::from(r.u16(4)) });
                }
                // BIFF8: wScaleNormal (the zoom of the normal view) — the SCL, if it comes, wins
                if b8 && r.len() >= 14 {
                    let z = u32::from(r.u16(12));
                    if (10..=400).contains(&z) {
                        rd.sh.zoom = Some(z);
                    }
                }
            }
            rt::SCL => {
                let (n, d) = (r.i16(0), r.i16(2));
                if n > 0 && d > 0 {
                    let z = uo_jsnum::math::round(f64::from(i32::from(n) * 100) / f64::from(d));
                    if (10.0..=400.0).contains(&z) {
                        rd.sh.zoom = Some(u32::try_from(to_int32(z)).unwrap_or(100));
                    }
                }
            }
            rt::PANE => {
                if rd.sh.frozen.is_some() {
                    rd.sh.frozen = Some(Frozen {
                        x: u32::from(r.u16(0)),
                        y: u32::from(r.u16(2)),
                        top: u32::from(r.u16(4)),
                        left: u32::from(r.u16(6)),
                    });
                }
            }
            rt::HLINK => {
                if let Some(link) = read_hlink(&r) {
                    rd.sh.hyperlinks.push(link);
                    last_link = Some(rd.sh.hyperlinks.len() - 1);
                }
            }
            rt::HLINKTOOLTIP => {
                // FrtRefHeaderNoGrbit (10 bytes) + wzTooltip (UTF-16, ends in 0)
                if let Some(li) = last_link {
                    let mut units: Vec<u16> = Vec::new();
                    let mut o = 10usize;
                    while o + 1 < r.len() {
                        let c = r.u16(o);
                        if c == 0 {
                            break;
                        }
                        units.push(c);
                        o += 2;
                    }
                    if !units.is_empty()
                        && let Some(l) = rd.sh.hyperlinks.get_mut(li)
                    {
                        l.tooltip = Some(String::from_utf16_lossy(&units));
                    }
                }
            }
            rt::NOTE => {
                if b8 {
                    let (author, _) = read_xl_string(&r, 8, true, &cp, 2, &rec.conts);
                    rd.sh.notes.push(NoteInfo {
                        row: u32::from(r.u16(0)),
                        col: u32::from(r.u16(2)),
                        obj_id: i32::from(r.u16(6)),
                        author,
                        text: None,
                    });
                } else {
                    // BIFF5: the text is in the NOTE itself; continuations have rw = 0xFFFF
                    let row = u32::from(r.u16(0));
                    let cch = usize::from(r.u16(4));
                    let text = cp.decode(r.sub(6, 6 + cch));
                    if row == 0xffff && last_note5.is_some() {
                        if let Some(n) = last_note5.and_then(|i| rd.sh.notes.get_mut(i)) {
                            n.text.get_or_insert_with(String::new).push_str(&text);
                        }
                    } else {
                        rd.sh.notes.push(NoteInfo { row, col: u32::from(r.u16(2)), obj_id: -1, author: String::new(), text: Some(text) });
                        last_note5 = Some(rd.sh.notes.len() - 1);
                    }
                }
            }
            rt::OBJ => {
                if !b8 {
                    last_obj = None;
                    continue;
                }
                // ftCmo: ft u16 (0x15), cb u16, ot u16, id u16, …
                if r.u16(0) == 0x15 {
                    rd.sh.objs.push(ObjInfo {
                        ot: r.u16(4),
                        id: r.u16(6),
                        drawing_at: rd.drawing.len(),
                        text: None,
                        txo_align: None,
                        txo_rot: None,
                        chart: None,
                    });
                    last_obj = Some(rd.sh.objs.len() - 1);
                }
                // a CONTINUE after the OBJ = a continuation of the DRAWING (Excel writes the next MSODRAWING — e.g. the
                // ClientTextbox — as CONTINUE)
                if let Some(&c0) = rec.conts.first() {
                    rd.add_drawing(&rec.data[c0.min(rec.data.len())..]);
                }
            }
            rt::TXO => {
                if !b8 {
                    continue;
                }
                let cch = usize::from(r.u16(10));
                let cb_runs = usize::from(r.u16(12));
                let mut end = rec.conts.first().copied().unwrap_or(r.len()); // the end of what is the TXO's (text + formats)
                if cch > 0 && !rec.conts.is_empty() {
                    let c0 = rec.conts[0];
                    let (text, next) = read_chars(&r, c0 + 1, cch, r.u8(c0) & 1 != 0, &rec.conts);
                    end = next;
                    let mut rs = RichString { text, runs: Vec::new() };
                    // runs (8 bytes: ich, ifnt, 4 reserved) in the CONTINUE after the text
                    let run_start = rec.conts.iter().copied().find(|&c| c >= next);
                    if let Some(run_start) = run_start
                        && cb_runs > 0
                    {
                        let run_end = r.len().min(run_start + cb_runs);
                        end = run_end;
                        let mut o = run_start;
                        while o + 8 <= run_end {
                            let ich = r.u16(o);
                            if usize::from(ich) >= cch {
                                break;
                            }
                            rs.runs.push(FormatRun { ich, ifnt: r.u16(o + 2) });
                            o += 8;
                        }
                    }
                    if let Some(o) = last_obj.and_then(|i| rd.sh.objs.get_mut(i)) {
                        o.text = Some(rs);
                    }
                }
                if let Some(o) = last_obj.and_then(|i| rd.sh.objs.get_mut(i)) {
                    o.txo_align = Some(r.u16(0));
                    o.txo_rot = Some(r.u16(2));
                }
                // a CONTINUE beyond the text and the formats = a continuation of the DRAWING (the next shape)
                if let Some(tail) = rec.conts.iter().copied().find(|&c| c >= end) {
                    rd.add_drawing(&rec.data[tail.min(rec.data.len())..]);
                }
            }
            rt::CONDFMT | rt::CONDFMT12 => {
                if !b8 {
                    continue;
                }
                let cf = read_cond_fmt(&r, if rec.ty == rt::CONDFMT12 { 12 } else { 0 });
                cur_cf = if cf.sqref.is_empty() {
                    None
                } else {
                    rd.sh.cond_formats.push(CondFormatOut { sqref: cf.sqref, rules: Vec::new(), by_icf: Vec::new() });
                    cf_base = (cf.top, cf.left);
                    let idx = rd.sh.cond_formats.len() - 1;
                    if rec.ty == rt::CONDFMT {
                        cf_by_nid.push((cf.nid, idx));
                    }
                    Some(idx)
                };
            }
            rt::CF => {
                let Some(ci) = cur_cf.filter(|_| b8) else { continue };
                rd.fctx.row = cf_base.0;
                rd.fctx.col = cf_base.1;
                cf_priority += 1;
                let rule = read_cf(g, &r, dxfs, &rd.fctx, cf_priority);
                if let Some(cf) = rd.sh.cond_formats.get_mut(ci) {
                    match rule {
                        Some(rule) => {
                            cf.rules.push(rule);
                            cf.by_icf.push(Some(cf.rules.len() - 1));
                        }
                        None => cf.by_icf.push(None),
                    }
                }
            }
            rt::CF12 => {
                let Some(ci) = cur_cf.filter(|_| b8) else { continue };
                rd.fctx.row = cf_base.0;
                rd.fctx.col = cf_base.1;
                let rule = read_cf12(g, &r, dxfs, &rd.fctx);
                if let (Some(rule), Some(cf)) = (rule, rd.sh.cond_formats.get_mut(ci)) {
                    cf.rules.push(rule);
                }
            }
            rt::CFEX => {
                if b8 {
                    apply_cf_ex(g, &r, dxfs, &mut rd.sh.cond_formats, &cf_by_nid);
                }
            }
            rt::DV => {
                if !b8 {
                    continue;
                }
                let dv = read_dv(&r, &cp, &mut rd.fctx);
                rd.fctx.rel3d = false;
                if let Some(dv) = dv {
                    rd.sh.data_validations.push(dv);
                }
            }
            rt::AUTOFILTERINFO => rd.sh.has_auto_filter = true,
            rt::MSODRAWING => rd.add_drawing(&rec.data),
            _ => {}
        }
    }
    if let Some(p) = pending.take() {
        rd.flush_formula(p, Some(""))?;
    }
    // ptgExp whose master formula (SHRFMLA/ARRAY/TABLE) never came: Excel shows =#REF! (measured: lo-tdf100582)
    let leftovers: Vec<(u32, u32)> = rd.waiting.values().flatten().copied().collect();
    for (wr, wc) in leftovers {
        rd.patch_f(wr, wc, "<f>#REF!</f>".to_owned());
    }
    if rd.has_drawing {
        rd.sh.drawing = Some(std::mem::take(&mut rd.drawing));
    }
    if is_chart {
        rd.sh.chart_records = Some((chart_start, chart_end));
    }
    // notes of BIFF8: the text comes from the TXO of the OBJ (ot = 0x19) with the same id
    let mut by_id: HashMap<u16, usize> = HashMap::new();
    for (i, o) in rd.sh.objs.iter().enumerate() {
        by_id.insert(o.id, i);
    }
    let texts: Vec<Option<String>> = rd
        .sh
        .notes
        .iter()
        .map(|n| {
            if n.text.is_some() {
                None
            } else {
                Some(
                    u16::try_from(n.obj_id)
                        .ok()
                        .and_then(|id| by_id.get(&id))
                        .and_then(|&i| rd.sh.objs.get(i))
                        .and_then(|o| o.text.as_ref())
                        .map(|t| t.text.clone())
                        .unwrap_or_default(),
                )
            }
        })
        .collect();
    for (n, t) in rd.sh.notes.iter_mut().zip(texts) {
        if let Some(t) = t {
            n.text = Some(t);
        }
    }
    rd.sh.cells.finish();
    Ok((rd.sh, rd.values))
}

// ───────────────────────────── HLINK ─────────────────────────────

const CLSID_URL: &str = "e0c9ea79f9bace118c8200aa004ba90b";
const CLSID_FILE: &str = "0303000000000000c000000000000046";

fn guid_hex(r: &Rd<'_>, o: usize) -> String {
    (0..16).map(|i| format!("{:02x}", r.u8(o + i))).collect()
}

/// `hlString`: a HyperlinkString: u32 with the number of characters (the final 0 included) + UTF-16.
fn hl_string(r: &Rd<'_>, o: usize) -> (String, usize) {
    let n = usize::try_from(r.u32(o)).unwrap_or(usize::MAX);
    let room = r.len().saturating_sub(o).saturating_sub(4) >> 1;
    let len = n.min(room);
    let mut units: Vec<u16> = Vec::new();
    for i in 0..len {
        let c = r.u16(o + 4 + i * 2);
        if c == 0 {
            break;
        }
        units.push(c);
    }
    (String::from_utf16_lossy(&units), o + 4 + len * 2)
}

/// `readHlink` (HLINK, [MS-XLS] 2.4.140 + [MS-OSHARED] Hyperlink Object).
fn read_hlink(r: &Rd<'_>) -> Option<HyperlinkInfo> {
    let mut link = HyperlinkInfo {
        r1: u32::from(r.u16(0)),
        r2: u32::from(r.u16(2)),
        c1: u32::from(r.u16(4)),
        c2: u32::from(r.u16(6)),
        ..HyperlinkInfo::default()
    };
    if link.r1 >= MAX_ROW || link.c1 >= MAX_COL {
        return None;
    }
    let mut p = 8 + 16; // ref + hlinkClsid
    p += 4; // streamVersion
    let fl = r.u32(p);
    p += 4;
    if fl & 0x10 != 0 {
        let (s, next) = hl_string(r, p);
        link.display = Some(s);
        p = next;
    }
    if fl & 0x80 != 0 {
        p = hl_string(r, p).1;
    }
    if fl & 0x01 != 0 {
        if fl & 0x100 != 0 {
            let (s, next) = hl_string(r, p);
            link.url = Some(s);
            p = next;
        } else {
            let clsid = guid_hex(r, p);
            p += 16;
            if clsid == CLSID_URL {
                let len = usize::try_from(r.u32(p)).unwrap_or(usize::MAX);
                p += 4;
                let mut units: Vec<u16> = Vec::new();
                let mut i = 0usize;
                while i.saturating_add(1) < len && p.saturating_add(i).saturating_add(1) < r.len() {
                    let c = r.u16(p + i);
                    if c == 0 {
                        break;
                    }
                    units.push(c);
                    i += 2;
                }
                link.url = Some(String::from_utf16_lossy(&units));
                p = p.saturating_add(len);
            } else if clsid == CLSID_FILE {
                let c_anti = usize::from(r.u16(p)).min(MAX_ANTI);
                p += 2;
                let ansi_len = usize::try_from(r.u32(p)).unwrap_or(usize::MAX);
                p += 4;
                let mut ansi = String::new();
                let mut i = 0usize;
                while i < ansi_len && p.saturating_add(i) < r.len() {
                    let c = r.u8(p + i);
                    if c == 0 {
                        break;
                    }
                    ansi.push(char::from(c));
                    i += 1;
                }
                p = p.saturating_add(ansi_len);
                p += 2 + 2 + 20; // endServer, versionNumber, reserved
                let cb_uni = r.u32(p);
                p += 4;
                let mut path = ansi;
                if cb_uni > 0 {
                    let cb_bytes = usize::try_from(r.u32(p)).unwrap_or(usize::MAX);
                    p += 4 + 2; // cbUnicodePathBytes + usKeyValue
                    let mut units: Vec<u16> = Vec::new();
                    let mut i = 0usize;
                    while i.saturating_add(1) < cb_bytes && p.saturating_add(i).saturating_add(1) < r.len() {
                        units.push(r.u16(p + i));
                        i += 2;
                    }
                    p = p.saturating_add(cb_bytes);
                    let u = String::from_utf16_lossy(&units);
                    if !u.is_empty() {
                        path = u;
                    }
                }
                link.url = Some(format!("{}{}", "../".repeat(c_anti), path.replace('\\', "/")));
            } else {
                return if link.display.is_some() { finish_link(link) } else { None };
            }
        }
    }
    if fl & 0x08 != 0 {
        let (s, _) = hl_string(r, p);
        link.location = Some(s);
    }
    finish_link(link)
}

fn finish_link(l: HyperlinkInfo) -> Option<HyperlinkInfo> {
    if l.url.is_none() && l.location.is_none() { None } else { Some(l) }
}
