//! `.xls` (BIFF8, and BIFF5/7 inside a Compound File) read BY CONVERSION (frontier C2, feature `xls`): the binary folder becomes, in
//! memory, the equivalent OOXML package (workbook, sheets, styles, strings, comments, drawings, charts) and follows the SAME path
//! as the `.xlsx` — one model, one renderer. The part texts are byte for byte those of the frozen TS converter
//! (`sheet-core/src/xls/*`, `ts-ref-092`); record `core/docs/modules/uo-xlsx.md`, "C2 — as built".
//!
//! ```ignore
//! let parts = uo_xlsx::xls::convert(&bytes)?;          // the package parts, in the order of the TS converter
//! let book = uo_xlsx::book::Book::open_xls(&bytes, ReadLimits::default())?; // convert + the normal reader
//! ```
//!
//! Reused instead of ported again: the Compound File reader (`uo_opc::cfb`), the OfficeArt records, properties, BLIP store and
//! preset table (`uo_drawingml::odraw`, flavor Sheet), the zip reader/inflate (`uo_opc`), the JS number printing (`uo_jsnum`) and the
//! built-in number formats (`uo_numfmt`).

mod biff;
mod cf;
mod chart;
mod data;
mod drawing;
mod dv;
mod formula;
mod geom;
mod globals;
mod metrics;
mod officeart;
mod shapes;
mod sheet;
mod sheet_xml;
mod styles_xml;
mod theme_xml;
mod xmlw;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use uo_opc::cfb::{self, CfbFile};
use uo_opc::package::{CaseFold, OpenOptions};
use uo_opc::{Limits, Package};

use self::biff::{OLD_BOFS, Records, rt};
use self::drawing::MediaState;
use self::formula::{FormulaContext, decompile};
use self::globals::{XlsGlobals, read_globals};
use self::sheet::{Rect, StringTable, XlsSheet, read_sheet};
use self::xmlw::{NS_MAIN, NS_R, Parts, Rel, XML_HEAD, rels_xml, xml_text};
use crate::book::{Book, ReadError, ReadLimits};

pub use self::xmlw::Parts as XlsParts;

/// Why a `.xls` could not be converted (typed, never a panic). The TS throws `XlsFormatError` / `XlsEncryptedError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsError {
    /// Not a readable Excel folder (the message says why).
    Format(String),
    /// Password-protected (encrypted) workbook.
    Encrypted,
    /// A bound was reached (the converted sheet would be larger than the XML part limit, …).
    Limit(&'static str),
}

impl std::fmt::Display for XlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XlsError::Format(m) => write!(f, "{m}"),
            XlsError::Encrypted => {
                write!(f, ".xls workbook protected by a password (encrypted): open it in Excel, remove the password and save again")
            }
            XlsError::Limit(w) => write!(f, "limit reached: {w}"),
        }
    }
}

impl std::error::Error for XlsError {}

/// Why [`Book::open_xls`] failed: the conversion, or the normal reader on the converted package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsOpenError {
    Xls(XlsError),
    Read(ReadError),
}

impl std::fmt::Display for XlsOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XlsOpenError::Xls(e) => write!(f, "{e}"),
            XlsOpenError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for XlsOpenError {}

impl From<XlsError> for XlsOpenError {
    fn from(e: XlsError) -> Self {
        XlsOpenError::Xls(e)
    }
}

impl From<ReadError> for XlsOpenError {
    fn from(e: ReadError) -> Self {
        XlsOpenError::Read(e)
    }
}

/// Bounds of the conversion (the TS has none: a hostile file could make it allocate without end).
#[derive(Clone, Copy, Debug)]
pub struct XlsLimits {
    /// Most bytes of the XML of one converted sheet (default: the XML part cap of `uo_opc::Limits`, so the sheet stays readable).
    pub max_sheet_bytes: u64,
    /// Most bytes of all the converted parts together (default: `uo_opc::Limits::max_total_bytes`).
    pub max_total_bytes: u64,
}

impl Default for XlsLimits {
    fn default() -> Self {
        XlsLimits { max_sheet_bytes: Limits::DEFAULT.max_xml_part_bytes, max_total_bytes: Limits::DEFAULT.max_total_bytes }
    }
}

/// The file is a Compound File (signature D0 CF 11 E0 — .xls/.doc/.ppt)? (`isXlsBytes`)
pub fn is_xls_bytes(bytes: &[u8]) -> bool {
    cfb::is_cfb(bytes)
}

/// A level-0 substream (BOF … EOF).
struct Substream {
    start: usize,
    end: usize,
    dt: u16,
}

/// `scanSubstreams`: walks the headers (no copy) and finds the level-0 substreams; `charts` = there is some chart (BOF dt=0x20,
/// also embedded in a sheet).
fn scan_substreams(buf: &[u8]) -> (Vec<Substream>, bool) {
    let mut out: Vec<Substream> = Vec::new();
    let (mut off, mut depth, mut charts) = (0usize, 0usize, false);
    let mut cur: Option<usize> = None;
    let le16 = |o: usize| usize::from(buf.get(o).copied().unwrap_or(0)) | (usize::from(buf.get(o + 1).copied().unwrap_or(0)) << 8);
    while off + 4 <= buf.len() {
        let ty = le16(off);
        let len = le16(off + 2);
        if ty == usize::from(rt::BOF) {
            let dt = u16::try_from(le16(off + 6)).unwrap_or(0);
            if dt == 0x20 {
                charts = true;
            }
            if depth == 0 {
                out.push(Substream { start: off, end: buf.len(), dt });
                cur = Some(out.len() - 1);
            }
            depth += 1;
        } else if ty == usize::from(rt::EOF) && depth > 0 {
            depth -= 1;
            if depth == 0
                && let Some(c) = cur.take()
            {
                out[c].end = off + 4 + len;
            }
        }
        off += 4 + len;
    }
    (out, charts)
}

/// The stream of the workbook: `Workbook` (BIFF8) or `Book` (BIFF5/7).
fn workbook_stream(cfb: &CfbFile<'_>) -> Result<Vec<u8>, XlsError> {
    let read = |name: &str| cfb.stream(name).map_err(|e| XlsError::Format(format!("corrupt or unsupported .xls file ({e})")));
    if let Some(wb) = read("Workbook")? {
        return Ok(wb);
    }
    if let Some(wb) = read("Book")? {
        return Ok(wb);
    }
    if cfb.has("EncryptedPackage") || cfb.has("EncryptionInfo") {
        return Err(XlsError::Encrypted);
    }
    if cfb.has("WordDocument") {
        return Err(XlsError::Format("the file is a Word document (.doc), not a workbook".to_owned()));
    }
    if cfb.has("PowerPoint Document") {
        return Err(XlsError::Format("the file is a PowerPoint presentation (.ppt), not a workbook".to_owned()));
    }
    Err(XlsError::Format("`.xls` file without the \"Workbook\" stream: not an Excel workbook".to_owned()))
}

/// `s.slice(0, n)` over UTF-16 units (a cut pair becomes U+FFFD, which `xmlText` would write for the lone surrogate).
fn slice_units(s: &str, n: usize) -> String {
    if s.is_ascii() {
        return s.get(..n.min(s.len())).unwrap_or("").to_owned();
    }
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&u[..n.min(u.len())])
}

/// A sheet of the output (BOUNDSHEET order without the VBA modules).
struct OutSheet {
    name: String,
    state: u8,
    sheet: XlsSheet,
    /// index in the BOUNDSHEET list
    bidx: usize,
}

/// Converts the bytes of a `.xls` into the parts of the equivalent OOXML package (`xlsToXlsxPackage`), in the order of the TS: the
/// content types and the package relationships first, then workbook, its relationships, each sheet with its comments,
/// drawings, charts and media, styles, shared strings and theme.
pub fn convert(bytes: &[u8]) -> Result<Parts, XlsError> {
    convert_with(bytes, &XlsLimits::default())
}

/// [`convert`] with explicit bounds.
pub fn convert_with(bytes: &[u8], limits: &XlsLimits) -> Result<Parts, XlsError> {
    let cfb = cfb::read_cfb(bytes).map_err(|e| XlsError::Format(format!("corrupt .xls file: {e}")))?;
    let buf = workbook_stream(&cfb)?;
    let has_workbook = cfb.has("Workbook");
    drop(cfb);
    if buf.len() < 8 {
        return Err(XlsError::Format("empty or truncated .xls file".to_owned()));
    }
    let t0 = u16::from_le_bytes([buf[0], buf[1]]);
    if OLD_BOFS.contains(&t0) {
        return Err(XlsError::Format("Excel 2.x-4.0 format (BIFF2-4) not supported: save as .xlsx in Excel".to_owned()));
    }
    if t0 != rt::BOF {
        return Err(XlsError::Format("stream \"Workbook\" without a BOF record: corrupt file".to_owned()));
    }
    let vers = u16::from_le_bytes([buf[4], buf[5]]);
    // `cfb.has('Workbook')` of the TS: the stream was read by that name or by "Book"; the version decides first
    let biff8 = match vers {
        0x0600 => true,
        0x0500 => false,
        _ => has_workbook,
    };
    convert_stream(&buf, biff8, limits)
}

fn convert_stream(buf: &[u8], biff8: bool, limits: &XlsLimits) -> Result<Parts, XlsError> {
    let (subs, charts) = scan_substreams(buf);
    let globals_sub = subs.first().ok_or_else(|| XlsError::Format("stream \"Workbook\" without a BOF record: corrupt file".to_owned()))?;
    let mut g = read_globals(Records::new(buf, globals_sub.start, globals_sub.end), biff8)?;
    let mut strings = StringTable::new(std::mem::take(&mut g.sst));
    // areas referenced by chart series (the values of those cells are kept)
    let wanted: HashMap<usize, Vec<Rect>> = if charts { chart::chart_refs(buf, &g) } else { HashMap::new() };
    let mut dxfs: Vec<String> = Vec::new();

    // substreams of the sheets: by the lbPlyPos of the BOUNDSHEET; if it does not match, by order
    let by_pos: HashMap<usize, &Substream> = subs.iter().map(|s| (s.start, s)).collect();
    let rest = &subs[1..];
    let mut sheets: Vec<OutSheet> = Vec::new();
    // the values of the cells charts reference, beside the sheets (so a sheet can be released once written)
    let mut sheet_values: Vec<HashMap<u64, sheet::CellValue>> = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();
    // BOUNDSHEET → position of the tab in the package (VBA modules are not tabs)
    let mut out_index: HashMap<usize, usize> = HashMap::new();
    let mut active_out = 0usize;
    let max_arena = usize::try_from(limits.max_sheet_bytes).unwrap_or(usize::MAX);
    for (i, info) in g.sheets.iter().enumerate() {
        let sub = usize::try_from(info.pos).ok().and_then(|p| by_pos.get(&p).copied()).or_else(|| rest.get(i));
        if info.ty == 6 {
            continue; // VBA module (BIFF5): not a tab
        }
        out_index.insert(i, sheets.len());
        let is_chart = info.ty == 2 || sub.is_some_and(|s| s.dt == 0x20);
        let (mut sheet, values) = match sub {
            Some(sub) => {
                let mut it = Records::new(buf, sub.start, sub.end);
                it.next(); // BOF
                read_sheet(&mut it, &g, &mut strings, &mut dxfs, is_chart, wanted.get(&i).map(Vec::as_slice), max_arena)?
            }
            None => {
                let mut it = Records::new(buf, 0, 0);
                read_sheet(&mut it, &g, &mut strings, &mut dxfs, is_chart, None, max_arena)?
            }
        };
        if sheet.has_auto_filter
            && let Some(fa) = i32::try_from(i).ok().and_then(|k| g.filter_db.get(&k))
        {
            sheet.auto_filter = Some([fa.r1, fa.c1, fa.r2, fa.c2]);
        }
        // Excel cuts the sheet name at 31 characters (third-party writers go past that)
        let base = slice_units(
            if uo_jsnum::text::trim(&info.name).is_empty() { format!("Planilha{}", sheets.len() + 1) } else { info.name.clone() }.as_str(),
            31,
        );
        let mut name = base.clone();
        let mut k = 2usize;
        while used_names.contains(&uo_jsnum::text::to_lowercase(&name)) {
            let keep = 31usize.saturating_sub(k.to_string().len() + 3);
            name = format!("{} ({k})", slice_units(&base, keep));
            k += 1;
        }
        used_names.insert(uo_jsnum::text::to_lowercase(&name));
        if i == usize::from(g.active_tab) {
            active_out = sheets.len();
        }
        sheets.push(OutSheet { name, state: info.state, sheet, bidx: i });
        sheet_values.push(values);
    }
    if sheets.is_empty() {
        return Err(XlsError::Format(".xls workbook without sheets".to_owned()));
    }

    let mut parts = Parts::default();
    let mut overrides: Vec<(String, String)> = Vec::new();
    let mut defaults: Vec<(String, String)> = vec![
        ("rels".to_owned(), "application/vnd.openxmlformats-package.relationships+xml".to_owned()),
        ("xml".to_owned(), "application/xml".to_owned()),
    ];

    // workbook
    let mut wb_rels: Vec<Rel> = Vec::new();
    let mut sheets_xml = String::new();
    for (i, s) in sheets.iter().enumerate() {
        let rid = format!("rId{}", i + 1);
        wb_rels.push(Rel { id: rid.clone(), kind: "worksheet", target: format!("worksheets/sheet{}.xml", i + 1), external: false });
        let st = match s.state {
            1 => " state=\"hidden\"",
            2 => " state=\"veryHidden\"",
            _ => "",
        };
        sheets_xml.push_str("<sheet name=\"");
        xml_text(&mut sheets_xml, &s.name);
        let _ = write!(sheets_xml, "\" sheetId=\"{}\"{st} r:id=\"{rid}\"/>", i + 1);
    }
    let mut n = sheets.len();
    for (kind, target) in [("styles", "styles.xml"), ("sharedStrings", "sharedStrings.xml"), ("theme", "theme/theme1.xml")] {
        n += 1;
        wb_rels.push(Rel { id: format!("rId{n}"), kind, target: target.to_owned(), external: false });
    }
    let mut wbx = String::new();
    wbx.push_str(XML_HEAD);
    let _ = write!(wbx, "<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">");
    // RECALCID = written by Excel (the calcId says whether it trusts the cache); without it, recalculate on open
    if g.recalc_id.is_some() {
        wbx.push_str("<fileVersion appName=\"xl\"/>");
    }
    wbx.push_str(if g.date1904 { "<workbookPr date1904=\"1\"/>" } else { "<workbookPr/>" });
    let _ = write!(wbx, "<bookViews><workbookView activeTab=\"{active_out}\"/></bookViews><sheets>{sheets_xml}</sheets>");
    wbx.push_str(&defined_names_xml(&g, &out_index));
    wbx.push_str(&calc_pr_xml(&g, &sheets));
    wbx.push_str("</workbook>");
    parts.set("xl/workbook.xml", wbx.into_bytes());
    parts.set("xl/_rels/workbook.xml.rels", rels_xml(&wb_rels).into_bytes());
    overrides
        .push(("/xl/workbook.xml".to_owned(), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml".to_owned()));

    // sheets (+ comments, hyperlinks, drawings)
    let mut values: Vec<Option<&HashMap<u64, sheet::CellValue>>> = vec![None; g.sheets.len()];
    for (s, v) in sheets.iter().zip(&sheet_values) {
        if let Some(slot) = values.get_mut(s.bidx) {
            *slot = Some(v);
        }
    }
    let mut media = MediaState {
        n: 0,
        charts: 0,
        by_pib: HashMap::new(),
        store: None,
        chart_ctx: chart::ChartContext { g: &g, sheet_names: g.sheets.iter().map(|s| s.name.clone()).collect(), values },
        buf,
        metro_left: 64 << 20,
    };
    let mut total = 0u64;
    for (i, s) in sheets.iter_mut().enumerate() {
        let idx = i + 1;
        let sp = sheet_xml::worksheet_xml(&g, &s.sheet, idx, &mut media);
        // the cells are in the XML now: free the arena before the next sheet is written
        s.sheet.cells.release();
        total += sp.xml.len() as u64;
        if total > limits.max_total_bytes {
            return Err(XlsError::Limit("converted size"));
        }
        parts.set(&format!("xl/worksheets/sheet{idx}.xml"), sp.xml.into_bytes());
        overrides.push((
            format!("/xl/worksheets/sheet{idx}.xml"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml".to_owned(),
        ));
        let mut rels = sp.rels;
        if !s.sheet.notes.is_empty() {
            let path = format!("xl/comments{idx}.xml");
            parts.set(&path, sheet_xml::comments_xml(&s.sheet.notes).into_bytes());
            overrides.push((format!("/{path}"), "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml".to_owned()));
            let id = format!("rId{}", rels.len() + 1);
            rels.push(Rel { id, kind: "comments", target: format!("../comments{idx}.xml"), external: false });
        }
        for (path, data) in sp.parts.items {
            total += data.len() as u64;
            parts.set(&path, data);
        }
        overrides.extend(sp.overrides);
        for (ext, ct) in sp.defaults {
            if let Some(slot) = defaults.iter_mut().find(|(e, _)| *e == ext) {
                slot.1 = ct;
            } else {
                defaults.push((ext, ct));
            }
        }
        if !rels.is_empty() {
            parts.set(&format!("xl/worksheets/_rels/sheet{idx}.xml.rels"), rels_xml(&rels).into_bytes());
        }
        if total > limits.max_total_bytes {
            return Err(XlsError::Limit("converted size"));
        }
    }
    drop(media);

    // styles, strings, theme
    parts.set("xl/styles.xml", styles_xml::styles_xml(&g, &dxfs).into_bytes());
    overrides.push(("/xl/styles.xml".to_owned(), "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml".to_owned()));
    let mut sst = String::with_capacity(strings.items.len() * 40 + 256);
    sst.push_str(XML_HEAD);
    let _ = write!(sst, "<sst xmlns=\"{NS_MAIN}\" count=\"{0}\" uniqueCount=\"{0}\">", strings.items.len());
    for s in &strings.items {
        sst.push_str("<si>");
        styles_xml::rich_xml(&g, &mut sst, s);
        sst.push_str("</si>");
    }
    sst.push_str("</sst>");
    sst.shrink_to_fit();
    total += sst.len() as u64;
    parts.set("xl/sharedStrings.xml", sst.into_bytes());
    overrides.push((
        "/xl/sharedStrings.xml".to_owned(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml".to_owned(),
    ));
    parts.set("xl/theme/theme1.xml", theme_xml::theme_xml(g.theme_zip.as_deref()).into_bytes());
    overrides.push(("/xl/theme/theme1.xml".to_owned(), "application/vnd.openxmlformats-officedocument.theme+xml".to_owned()));
    if total > limits.max_total_bytes {
        return Err(XlsError::Limit("converted size"));
    }

    // package
    parts.set(
        "_rels/.rels",
        rels_xml(&[Rel { id: "rId1".to_owned(), kind: "officeDocument", target: "xl/workbook.xml".to_owned(), external: false }])
            .into_bytes(),
    );
    let mut ct = String::new();
    ct.push_str(XML_HEAD);
    ct.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (ext, ty) in &defaults {
        let _ = write!(ct, "<Default Extension=\"{ext}\" ContentType=\"{ty}\"/>");
    }
    for (path, ty) in &overrides {
        let _ = write!(ct, "<Override PartName=\"{path}\" ContentType=\"{ty}\"/>");
    }
    ct.push_str("</Types>");
    // the zip order as Excel's: the types and the package rels first
    let mut ordered = Parts::default();
    ordered.set("[Content_Types].xml", ct.into_bytes());
    let rels_part = parts.get("_rels/.rels").map(<[u8]>::to_vec).unwrap_or_default();
    ordered.set("_rels/.rels", rels_part);
    for (p, d) in parts.items {
        if !ordered.contains(&p) {
            ordered.set(&p, d);
        }
    }
    Ok(ordered)
}

/// Defined names of BIFF8 → `<definedNames>` (decompiled formula; built-ins with `_xlnm.`; references relative to A1, as in the
/// .xlsx). Macro names (fFunc/fOB) and those that do not decompile are left out.
fn defined_names_xml(g: &XlsGlobals, out_index: &HashMap<usize, usize>) -> String {
    if !g.biff8 || g.name_defs.is_empty() {
        return String::new();
    }
    let mut ctx = FormulaContext::new(g);
    ctx.rel3d = true;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = String::new();
    for d in &g.name_defs {
        if d.macro_ || d.name.is_empty() {
            continue;
        }
        let local = if d.sheet >= 0 { usize::try_from(d.sheet).ok().and_then(|s| out_index.get(&s).copied()) } else { None };
        if d.sheet >= 0 && local.is_none() {
            continue;
        }
        let text = if d.cce > 0 { decompile(&biff::Rd(&d.data), d.off, d.cce, &ctx) } else { None };
        let Some(text) = text else { continue };
        let name = if d.builtin { format!("_xlnm.{}", d.name) } else { d.name.clone() };
        let key = format!("{}|{}", name.to_uppercase(), local.map_or(-1, |l| i64::try_from(l).unwrap_or(-1)));
        if !seen.insert(key) {
            continue;
        }
        out.push_str("<definedName name=\"");
        xml_text(&mut out, &name);
        out.push('"');
        if let Some(l) = local {
            let _ = write!(out, " localSheetId=\"{l}\"");
        }
        if d.hidden {
            out.push_str(" hidden=\"1\"");
        }
        out.push('>');
        xml_text(&mut out, &text);
        out.push_str("</definedName>");
    }
    if out.is_empty() { String::new() } else { format!("<definedNames>{out}</definedNames>") }
}

/// Calculation options → `<calcPr>`: calcId = RECALCID.dwBuild (the engine of the last full recalculation; without it, no
/// calcId — Excel recalculates on open), the mode and iteration of the 1st sheet that carries them (the BIFF repeats them in each sheet).
fn calc_pr_xml(g: &XlsGlobals, sheets: &[OutSheet]) -> String {
    let c = sheets.iter().find_map(|s| s.sheet.calc);
    let mut a = String::new();
    if let Some(id) = g.recalc_id.filter(|&i| i != 0) {
        let _ = write!(a, " calcId=\"{id}\"");
    }
    if let Some(c) = c {
        match c.mode {
            Some(0) => a.push_str(" calcMode=\"manual\""),
            Some(-1) => a.push_str(" calcMode=\"autoNoTable\""),
            _ => {}
        }
        if c.iterate == Some(true) {
            a.push_str(" iterate=\"1\"");
        }
        if let Some(n) = c.count.filter(|&n| n != 100) {
            let _ = write!(a, " iterateCount=\"{n}\"");
        }
        if let Some(d) = c.delta.filter(|d| d.is_finite() && *d != 0.001) {
            let _ = write!(a, " iterateDelta=\"{}\"", uo_jsnum::format::to_js_string(d));
        }
    }
    if a.is_empty() { String::new() } else { format!("<calcPr{a}/>") }
}

impl Book {
    /// Opens a `.xls` workbook: converts it to the OOXML package ([`convert`]) and reads that package with the normal reader
    /// ([`Book::from_package`]).
    pub fn open_xls(bytes: &[u8], limits: ReadLimits) -> Result<Book, XlsOpenError> {
        let parts = convert(bytes)?;
        let pkg = Package::from_parts(parts.items, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() });
        Ok(Book::from_package(pkg, limits)?)
    }
}
