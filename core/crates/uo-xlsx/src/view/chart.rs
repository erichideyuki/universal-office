//! The Sheet's charts (frontier C4b): `sheet-core/src/chart.ts` (the host bridge: Office 2023 theme for a workbook without a
//! theme part, `chartFileFormat`, the chart drawing, `paintChart`'s Excel profile) and `chart-data.ts` (the caches of the chart
//! rewritten from the CELLS of the workbook, as Excel rereads them at open), ported 1:1 (C1a-12 had left `chart-data.ts` for here).
//!
//! The TS rewrites the parsed tree in place and hands it to `parseChartSpace`; here the canonical tree is written back as XML
//! text with the rewritten caches and parsed again (the same elements, attributes and texts: C4b-4), then `uo_charts` parses it
//! through [`SheetHost`]. Models are kept per chart part ([`MAX_CHART_MODELS`], least recently used dropped: the TS `chartCache`
//! is unbounded and keyed by the XML text, which is the same model).

use std::sync::Arc;

use uo_calc::key::SheetIx;
use uo_charts::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use uo_charts::model::{ChartColor, ChartSpace, Paint};
use uo_charts::{CHART_PREFIXES, Scheme, dml_color_source, parse_chart_space, parse_paint_node};
use uo_opc::xml::{CanonicalPrefixes, Document, Element, Node, ParseOptions, parse};

use crate::book::Book;
use crate::read::model::{Theme, XChild, XEl};

/// Parsed chart models a viewer keeps (M5; the TS `chartCache` is unbounded).
pub const MAX_CHART_MODELS: usize = 32;
/// Cells one reference area may cover before the cache is kept (`n > 100000` in `cellsOf`).
const MAX_AREA_CELLS: f64 = 100_000.0;

/// The Sheet profile of the chart parse: canonical prefixes (`canonicalNs`), line ends normalised as `parseXml`.
#[must_use]
pub fn xl_options() -> ParseOptions<'static> {
    ParseOptions {
        canonical: Some(CanonicalPrefixes { table: CHART_PREFIXES, default_namespace: true }),
        normalize_eol: true,
        ..ParseOptions::LENIENT
    }
}

/// `FALLBACK` of chart.ts: the Office 2013–2022 palette under the theme's colours.
const FALLBACK: [(&str, &str); 12] = [
    ("dk1", "#000000"),
    ("lt1", "#FFFFFF"),
    ("dk2", "#44546A"),
    ("lt2", "#E7E6E6"),
    ("accent1", "#4472C4"),
    ("accent2", "#ED7D31"),
    ("accent3", "#A5A5A5"),
    ("accent4", "#FFC000"),
    ("accent5", "#5B9BD5"),
    ("accent6", "#70AD47"),
    ("hlink", "#0563C1"),
    ("folHlink", "#954F72"),
];

/// `OFFICE_2023.byName` (a workbook without a theme part: Excel uses the Office 2023 theme, measured).
const OFFICE_2023: [(&str, &str); 12] = [
    ("dk1", "#000000"),
    ("lt1", "#FFFFFF"),
    ("dk2", "#0E2841"),
    ("lt2", "#E8E8E8"),
    ("accent1", "#156082"),
    ("accent2", "#E97132"),
    ("accent3", "#196B24"),
    ("accent4", "#0F9ED5"),
    ("accent5", "#A02B93"),
    ("accent6", "#4EA72E"),
    ("hlink", "#467886"),
    ("folHlink", "#96607D"),
];

/// The Excel profile of `paintChart`: the chart area without `spPr` white (`bg1`) with the 2010 automatic frame, denser axes.
#[must_use]
pub fn sheet_profile() -> uo_charts::emit::HostProfile {
    uo_charts::emit::HostProfile { area_fill_bg1: true, area_line: Some(uo_charts::emit::AreaLine::Auto2010), excel_axis_density: true }
}

// ------------------------------------------------------------------------------------------------ the host

/// The host of `parseChart` (chart.ts:46-73) over the workbook theme.
pub struct SheetHost {
    scheme: Scheme,
    minor: Box<str>,
    major: Option<Box<str>>,
    /// `theme.fillStyles` as one parsed document (its root's children, in order)
    fills: Option<Document>,
    format: FileFormat,
    user_shapes: Option<Document>,
    widths: Vec<f64>,
}

impl ChartHost for SheetHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        dml_color_source(src, &self.scheme)
    }

    fn fonts(&self) -> HostFonts<'_> {
        HostFonts { major: self.major.as_deref(), minor: Some(&self.minor) }
    }

    fn has_fill_style(&self) -> bool {
        true
    }

    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        let doc = self.fills.as_ref()?;
        let el = doc.root().child_elements().nth(usize::try_from(idx.checked_sub(1)?).ok()?)?;
        let mut sc = self.scheme.clone();
        sc.set("phClr", ph_clr);
        parse_paint_node(el, &|e| dml_color_source(e, &sc))
    }

    fn blip(&self, _blip: Element<'_>) -> Option<BlipKey> {
        None
    }

    fn file_format(&self) -> Option<FileFormat> {
        Some(self.format)
    }

    fn user_shapes(&self) -> Option<Element<'_>> {
        self.user_shapes.as_ref().map(Document::root)
    }

    fn theme_line_widths(&self) -> &[f64] {
        &self.widths
    }
}

/// An owned element (the theme's fill styles) as XML text.
fn xel_text(out: &mut String, e: &XEl) {
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        attr_escape(out, v);
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &e.children {
        match c {
            XChild::El(x) => xel_text(out, x),
            XChild::Text(t) => text_escape(out, t),
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

/// Text content escaped for a re-parse that gives the same characters back.
pub(super) fn text_escape(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// An attribute value escaped so that the re-parse keeps it exactly (white space as character references: no normalisation).
pub(super) fn attr_escape(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// The theme's `fillStyleLst` entries as one document (`<w>…</w>`).
fn fills_doc(t: &Theme) -> Option<Document> {
    let fills = t.fill_styles.as_ref()?;
    let mut s = String::from("<w>");
    for f in fills {
        xel_text(&mut s, f);
    }
    s.push_str("</w>");
    parse(s, &ParseOptions::LENIENT).ok()
}

// ------------------------------------------------------------------------------------------------ the cells (chart-data.ts)

/// `ChartCell`.
#[derive(Clone, Debug)]
struct ChartCell {
    num: Option<f64>,
    text: String,
    fmt: String,
    err: bool,
}

/// An area of a reference (`Area`; 1-based rows and columns as the TS numbers them).
#[derive(Clone, Debug)]
struct Area {
    sheet: String,
    r0: f64,
    c0: f64,
    r1: f64,
    c1: f64,
}

/// `pkgCellSource` over the store (every sheet is loaded when a chart is painted: `Viewer::activate`).
struct Cells<'a> {
    book: &'a Book,
    /// `"<scope lower>!<name lower>"` → formula text (later definitions replace, as `Map.set`)
    names: Vec<(String, String)>,
    props: std::cell::RefCell<crate::read::sst::RunPropsTable>,
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

impl<'a> Cells<'a> {
    fn new(book: &'a Book) -> Cells<'a> {
        let wb = &book.workbook;
        let mut names: Vec<(String, String)> = Vec::new();
        for d in &wb.defined_names {
            if d.name.is_empty() {
                continue;
            }
            let scope = match d.local_sheet_id {
                Some(li) => usize_of(li).and_then(|i| wb.sheets.get(i)).map_or("", |s| s.name.as_str()),
                None => "",
            };
            let key = format!("{}!{}", lower(scope), lower(&d.name));
            match names.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1.clone_from(&d.formula),
                None => names.push((key, d.formula.clone())),
            }
        }
        Cells { book, names, props: std::cell::RefCell::new(crate::read::sst::RunPropsTable::default()) }
    }

    /// `sheetOf`: the first sheet of that name (lower case); its data is the host's sheet of that exact name (`chartSheetMap`:
    /// the LAST view with it wins, so two sheets named alike read the later one), else its own part.
    fn sheet_of(&self, name: &str) -> Option<usize> {
        let key = lower(name);
        let sheets = &self.book.workbook.sheets;
        let i = sheets.iter().position(|s| lower(&s.name) == key)?;
        let exact = &sheets[i].name;
        let loaded = |j: usize| sheets[j].target.is_some() && self.book.sheets.get(j).is_some_and(Option::is_some);
        if let Some(j) = (0..sheets.len()).rev().find(|&j| sheets[j].name == *exact && loaded(j)) {
            return Some(j);
        }
        loaded(i).then_some(i)
    }

    fn has_sheet(&self, s: &str) -> bool {
        self.sheet_of(s).is_some()
    }

    fn hidden_row(&self, s: &str, r: f64) -> bool {
        let Some(si) = self.sheet_of(s) else { return false };
        let Some(row) = idx_of(r) else { return false };
        let ix = SheetIx(u32::try_from(si).unwrap_or(u32::MAX));
        self.book.store.sheet(ix).and_then(|sh| sh.row_meta(row)).is_some_and(|m| m.flags & uo_calc::store::RowMeta::HIDDEN != 0)
    }

    fn hidden_col(&self, s: &str, c: f64) -> bool {
        let Some(si) = self.sheet_of(s) else { return false };
        let Some(ws) = self.book.sheets.get(si).and_then(Option::as_ref) else { return false };
        // `for (k = c.min; k <= min(c.max, 16384); k++) hiddenCols.add(k)`
        ws.cols.iter().any(|cd| {
            cd.hidden && cd.min <= c && c <= uo_jsnum::math::min(cd.max, 16384.0) && (c - cd.min).fract() == 0.0 && cd.min.is_finite()
        })
    }

    fn name(&self, n: &str, sheet: Option<&str>) -> Option<&str> {
        let k = format!("{}!{}", lower(sheet.unwrap_or("")), lower(n));
        let g = format!("!{}", lower(n));
        self.names.iter().find(|(x, _)| *x == k).or_else(|| self.names.iter().find(|(x, _)| *x == g)).map(|(_, v)| v.as_str())
    }

    /// `cell(sheet, row, col)` (1-based).
    fn cell(&self, s: &str, row: f64, col: f64) -> Option<ChartCell> {
        let si = self.sheet_of(s)?;
        let (r, c) = (idx_of(row)?, idx_of(col)?);
        if r >= 1_048_576 || c >= 16_384 {
            return None;
        }
        let ix = SheetIx(u32::try_from(si).ok()?);
        let fc = self.book.store.file_cell(ix, r, c)?;
        let styles = &self.book.styles;
        let style = fc.s.unwrap_or(0.0);
        let id = match usize_of(style).and_then(|i| styles.cell_xfs.get(i)) {
            Some(xf) => xf.num_fmt_id,
            None => 0.0,
        };
        let fmt = styles
            .num_fmts
            .iter()
            .rev()
            .find(|(k, _)| *k == id || (k.is_nan() && id.is_nan()))
            .map(|(_, v)| v.clone())
            .or_else(|| {
                let u = ((0.0..4_294_967_296.0).contains(&id) && id.fract() == 0.0).then(|| uo_jsnum::conv::to_uint32(id))?;
                uo_numfmt::builtin_format_code(u, uo_numfmt::pt_br()).map(str::to_owned)
            })
            .unwrap_or_else(|| "General".to_owned());
        let t = fc.t.as_ref().map(|t| t.as_js().to_string_lossy());
        let v = fc.v.as_ref().map(|v| v.as_js().to_string_lossy());
        match t.as_deref() {
            Some("s") => {
                let i = uo_jsnum::parse::parse_int(v.as_deref().unwrap_or("-1"), 10);
                let text = self.shared(i).unwrap_or_default();
                return Some(ChartCell { num: None, text, fmt, err: false });
            }
            Some("inlineStr") => {
                let text = fc.is.as_ref().and_then(|i| i.text.as_ref()).map(|t| t.as_js().to_string_lossy()).unwrap_or_default();
                return Some(ChartCell { num: None, text, fmt, err: false });
            }
            Some("str") => return Some(ChartCell { num: None, text: v.unwrap_or_default(), fmt, err: false }),
            Some("e") => return Some(ChartCell { num: None, text: v.unwrap_or_default(), fmt, err: true }),
            _ => {}
        }
        let v = v.filter(|v| !v.is_empty())?;
        if t.as_deref() == Some("b") {
            let one = v == "1";
            return Some(ChartCell {
                num: Some(if one { 1.0 } else { 0.0 }),
                text: if one { "VERDADEIRO" } else { "FALSO" }.to_owned(),
                fmt,
                err: false,
            });
        }
        let n = uo_jsnum::parse::parse_float(&v);
        if !n.is_finite() {
            return Some(ChartCell { num: None, text: v, fmt, err: false });
        }
        let code: Vec<u16> = fmt.encode_utf16().collect();
        let opts = uo_numfmt::FormatOptions { date1904: self.book.workbook.date1904, ..uo_numfmt::FormatOptions::default() };
        let text = String::from_utf16_lossy(&uo_numfmt::format_value(uo_numfmt::Value::Num(n), Some(&code), &opts).text);
        Some(ChartCell { num: Some(n), text, fmt, err: false })
    }

    /// `shared[i]?.text`: the plain text of shared string `i`.
    fn shared(&self, i: f64) -> Option<String> {
        let u = ((0.0..4_294_967_296.0).contains(&i) && i.fract() == 0.0).then(|| uo_jsnum::conv::to_uint32(i))?;
        if let Some(id) = self.book.store.sst_text(u) {
            return Some(self.book.store.strings.get(id).to_string_lossy());
        }
        let index = self.book.sst.as_ref()?;
        let it = index.decode(&self.book.pkg, u, &self.book.cx, &mut self.props.borrow_mut()).ok()??;
        it.text.map(|t| t.as_js().to_string_lossy())
    }
}

/// A non-negative integer index from a JS number (0-based from a 1-based row or column).
fn idx_of(x: f64) -> Option<u32> {
    ((1.0..=4_294_967_296.0).contains(&x) && x.fract() == 0.0).then(|| uo_jsnum::conv::to_uint32(x - 1.0))
}

fn usize_of(x: f64) -> Option<usize> {
    ((0.0..4_294_967_296.0).contains(&x) && x.fract() == 0.0).then(|| uo_jsnum::conv::to_uint32(x) as usize)
}

/// `colOf`: the uppercase letters at the start of a reference.
fn col_of(r: &str) -> f64 {
    let mut n = 0.0;
    for b in r.bytes() {
        if !b.is_ascii_uppercase() {
            break;
        }
        n = n * 26.0 + f64::from(b - 64);
    }
    n
}

/// `^\$?([A-Za-z]{1,3})\$?(\d+)(?::\$?([A-Za-z]{1,3})\$?(\d+))?$`.
/// The letters and digits of a cell reference.
type CellParts = (String, String);

fn parse_addr(addr: &str) -> Option<(String, String, Option<CellParts>)> {
    fn cell(s: &str) -> Option<(String, String, &str)> {
        let s = s.strip_prefix('$').unwrap_or(s);
        let n = s.bytes().take_while(u8::is_ascii_alphabetic).count();
        if !(1..=3).contains(&n) {
            return None;
        }
        let (letters, rest) = s.split_at(n);
        let rest = rest.strip_prefix('$').unwrap_or(rest);
        let d = rest.bytes().take_while(u8::is_ascii_digit).count();
        if d == 0 {
            return None;
        }
        let (digits, tail) = rest.split_at(d);
        Some((letters.to_owned(), digits.to_owned(), tail))
    }
    let (l1, d1, tail) = cell(addr)?;
    if tail.is_empty() {
        return Some((l1, d1, None));
    }
    let (l2, d2, tail2) = cell(tail.strip_prefix(':')?)?;
    tail2.is_empty().then_some((l1, d1, Some((l2, d2))))
}

/// `resolveRef`: `Sheet!$A$1:$B$2`, `'Sheet x'!A1`, `(area,area)`, defined names → areas; `None` when external or unreadable.
fn resolve_ref(f: &str, src: &Cells<'_>, depth: u32) -> Option<Vec<Area>> {
    let t = uo_jsnum::text::trim(f);
    let mut s: &str = t.strip_prefix('=').unwrap_or(t);
    if s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
        s = &s[1..s.len() - 1];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for ch in s.chars() {
        if ch == '\'' {
            q = !q;
        }
        if ch == ',' && !q {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    parts.push(cur);
    let mut out: Vec<Area> = Vec::new();
    for p0 in &parts {
        let p = uo_jsnum::text::trim(p0);
        let (mut sheet, addr) = match p.rfind('!') {
            Some(b) => (p[..b].to_owned(), &p[b + 1..]),
            None => (String::new(), p),
        };
        if p.contains('!') {
            if sheet.starts_with('\'') && sheet.ends_with('\'') && sheet.len() >= 2 {
                sheet = sheet[1..sheet.len() - 1].replace("''", "'");
            }
            // `/^\[\d+\]/`: another workbook (the cache stays)
            if let Some(rest) = sheet.strip_prefix('[') {
                let d = rest.bytes().take_while(u8::is_ascii_digit).count();
                if d > 0 && rest.as_bytes().get(d) == Some(&b']') {
                    return None;
                }
            }
            // `.replace(/^\[[^\]]*\]/, '')`
            if let Some(rest) = sheet.strip_prefix('[')
                && let Some(end) = rest.find(']')
            {
                sheet = rest[end + 1..].to_owned();
            }
        }
        let Some((l1, d1, second)) = parse_addr(addr) else {
            // a defined name (the scope of the sheet named, else global)
            if depth > 3 {
                return None;
            }
            let def = src.name(addr, (!sheet.is_empty()).then_some(sheet.as_str()))?;
            let sub = resolve_ref(def, src, depth + 1)?;
            out.extend(sub);
            continue;
        };
        if sheet.is_empty() || !src.has_sheet(&sheet) {
            return None;
        }
        let c0 = col_of(&l1.to_ascii_uppercase());
        let r0 = uo_jsnum::parse::parse_int(&d1, 10);
        let (c1, r1) = match &second {
            Some((l2, d2)) => (col_of(&l2.to_ascii_uppercase()), uo_jsnum::parse::parse_int(d2, 10)),
            None => (c0, r0),
        };
        out.push(Area {
            sheet,
            r0: uo_jsnum::math::min(r0, r1),
            c0: uo_jsnum::math::min(c0, c1),
            r1: uo_jsnum::math::max(r0, r1),
            c1: uo_jsnum::math::max(c0, c1),
        });
    }
    (!out.is_empty()).then_some(out)
}

/// `cellsOf`: the cells of 1-D areas in range order; `vis_only` skips hidden rows/columns.
fn cells_of(areas: &[Area], src: &Cells<'_>, vis_only: bool) -> Option<Vec<Option<ChartCell>>> {
    let mut out: Vec<Option<ChartCell>> = Vec::new();
    for a in areas {
        let n = (a.r1 - a.r0 + 1.0) * (a.c1 - a.c0 + 1.0);
        if n > MAX_AREA_CELLS {
            return None;
        }
        if a.r0 != a.r1 && a.c0 != a.c1 {
            // a 2-D range outside multiLvlStrRef: points along the longer dimension, the texts of the other one joined
            let by_rows = a.r1 - a.r0 >= a.c1 - a.c0;
            let (p0, p1, q0, q1) = if by_rows { (a.r0, a.r1, a.c0, a.c1) } else { (a.c0, a.c1, a.r0, a.r1) };
            let mut p = p0;
            while p <= p1 {
                let hidden = if by_rows { src.hidden_row(&a.sheet, p) } else { src.hidden_col(&a.sheet, p) };
                if !(vis_only && hidden) {
                    let mut parts: Vec<ChartCell> = Vec::new();
                    let mut q = q0;
                    while q <= q1 {
                        let cl = if by_rows { src.cell(&a.sheet, p, q) } else { src.cell(&a.sheet, q, p) };
                        if let Some(cl) = cl.filter(|c| !c.text.is_empty()) {
                            parts.push(cl);
                        }
                        q += 1.0;
                    }
                    out.push(parts.last().cloned().map(|last| {
                        let text = parts.iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join(" ");
                        ChartCell { text, ..last }
                    }));
                }
                p += 1.0;
            }
            continue;
        }
        let mut r = a.r0;
        while r <= a.r1 {
            let mut c = a.c0;
            while c <= a.c1 {
                if !(vis_only && (src.hidden_row(&a.sheet, r) || src.hidden_col(&a.sheet, c))) {
                    out.push(src.cell(&a.sheet, r, c));
                }
                c += 1.0;
            }
            r += 1.0;
        }
    }
    Some(out)
}

/// The rewritten children of a reference element (`[f, cache, ...ext]`), as XML text.
type Rewrite = String;

fn num_cache(cells: &[Option<ChartCell>], tag: &str, as_values: bool) -> Rewrite {
    let first = cells.iter().flatten().find(|c| c.num.is_some() || !as_values);
    let fmt = first.map_or("General", |c| c.fmt.as_str());
    let mut s = format!("<{tag}><c:formatCode>");
    text_escape(&mut s, fmt);
    s.push_str("</c:formatCode><c:ptCount val=\"");
    s.push_str(&cells.len().to_string());
    s.push_str("\"/>");
    for (i, c) in cells.iter().enumerate() {
        let Some(c) = c else { continue };
        if c.err {
            continue;
        }
        // text in a values range: Excel draws it as 0
        let v = match c.num {
            Some(n) => n,
            None if as_values => 0.0,
            None => continue,
        };
        s.push_str("<c:pt idx=\"");
        s.push_str(&i.to_string());
        s.push('"');
        if c.fmt != fmt {
            s.push_str(" formatCode=\"");
            attr_escape(&mut s, &c.fmt);
            s.push('"');
        }
        s.push_str("><c:v>");
        text_escape(&mut s, &uo_jsnum::format::to_js_string(v));
        s.push_str("</c:v></c:pt>");
    }
    s.push_str("</");
    s.push_str(tag);
    s.push('>');
    s
}

fn str_cache(cells: &[Option<ChartCell>]) -> Rewrite {
    let mut s = format!("<c:strCache><c:ptCount val=\"{}\"/>", cells.len());
    for (i, c) in cells.iter().enumerate() {
        if let Some(c) = c
            && !c.text.is_empty()
        {
            s.push_str(&format!("<c:pt idx=\"{i}\"><c:v>"));
            text_escape(&mut s, &c.text);
            s.push_str("</c:v></c:pt>");
        }
    }
    s.push_str("</c:strCache>");
    s
}

fn text_content(e: Element<'_>) -> String {
    let mut s = String::new();
    fn go(e: Element<'_>, s: &mut String) {
        for n in e.children() {
            match n {
                Node::Element(c) => go(c, s),
                Node::Text(t) | Node::CData(t) => s.push_str(t),
                _ => {}
            }
        }
    }
    go(e, &mut s);
    s
}

/// What replaces an element's children in the written tree.
enum Repl {
    /// `[f, cache, ...ext]` / `[f, ...lvls]`: the first child named `f` (its whole element), the given text, then the other
    /// element children except the names listed.
    Ref { f: &'static str, cache: String, drop: &'static [&'static str] },
}

/// The rewrites of a chart part, keyed by element (identity in the parsed document).
struct Plan {
    at: Vec<(uo_opc::xml::ElementId, Repl)>,
}

/// `refreshChartData` decided over the parsed tree: which elements get new children.
fn plan(root: Element<'_>, src: &Cells<'_>) -> Plan {
    let mut p = Plan { at: Vec::new() };
    if root.name() == "cx:chartSpace" {
        plan_chartex(root, src, &mut p);
        return p;
    }
    let Some(chart) = root.first_child("c:chart") else { return p };
    // a pivot chart: Excel draws from the pivot table — the cache stays
    if root.first_child("c:pivotSource").is_some() {
        return p;
    }
    let vis_only = match chart.first_child("c:plotVisOnly") {
        None => true,
        Some(pv) => matches!(pv.attr("val"), None | Some("1" | "true")),
    };
    fn visit(e: Element<'_>, in_val: bool, src: &Cells<'_>, vis_only: bool, p: &mut Plan) {
        for c in e.child_elements() {
            let n = c.name();
            if n == "c:numRef" || n == "c:strRef" {
                let areas = c.first_child("c:f").and_then(|f| resolve_ref(&text_content(f), src, 0));
                let Some(cells) = areas.and_then(|a| cells_of(&a, src, vis_only)) else { continue };
                let cache = if n == "c:numRef" { num_cache(&cells, "c:numCache", in_val) } else { str_cache(&cells) };
                p.at.push((c.id(), Repl::Ref { f: "c:f", cache, drop: &["c:numCache", "c:strCache", "c:f"] }));
                continue;
            }
            if n == "c:multiLvlStrRef" {
                if let Some(cache) = multi_level(c, src, vis_only) {
                    p.at.push((c.id(), Repl::Ref { f: "c:f", cache, drop: &["c:multiLvlStrCache", "c:f"] }));
                }
                continue;
            }
            visit(c, in_val || n == "c:val" || n == "c:yVal" || n == "c:bubbleSize", src, vis_only, p);
        }
    }
    if let Some(plot) = chart.first_child("c:plotArea") {
        visit(plot, false, src, vis_only, &mut p);
    }
    p
}

/// `refreshMultiLevel`: the levels of a 2-D range (columns or rows), from the inner (last) to the outer.
fn multi_level(r: Element<'_>, src: &Cells<'_>, vis_only: bool) -> Option<String> {
    let areas = resolve_ref(&text_content(r.first_child("c:f")?), src, 0)?;
    if areas.len() != 1 {
        return None;
    }
    let a = &areas[0];
    let by_rows = a.r1 - a.r0 >= a.c1 - a.c0;
    let (n_pts, n_lvl) = if by_rows { (a.r1 - a.r0 + 1.0, a.c1 - a.c0 + 1.0) } else { (a.c1 - a.c0 + 1.0, a.r1 - a.r0 + 1.0) };
    if n_pts > MAX_AREA_CELLS || n_lvl > 10.0 {
        return None;
    }
    let mut idx: Vec<f64> = Vec::new();
    let mut k = 0.0;
    while k < n_pts {
        let (rr, cc) = if by_rows { (a.r0 + k, a.c0) } else { (a.r0, a.c0 + k) };
        let hidden = if by_rows { src.hidden_row(&a.sheet, rr) } else { src.hidden_col(&a.sheet, cc) };
        if !(vis_only && hidden) {
            idx.push(k);
        }
        k += 1.0;
    }
    let mut s = format!("<c:multiLvlStrCache><c:ptCount val=\"{}\"/>", idx.len());
    let mut l = n_lvl - 1.0;
    while l >= 0.0 {
        s.push_str("<c:lvl>");
        for (i, &k) in idx.iter().enumerate() {
            let cell = if by_rows { src.cell(&a.sheet, a.r0 + k, a.c0 + l) } else { src.cell(&a.sheet, a.r0 + l, a.c0 + k) };
            if let Some(c) = cell
                && !c.text.is_empty()
            {
                s.push_str(&format!("<c:pt idx=\"{i}\"><c:v>"));
                text_escape(&mut s, &c.text);
                s.push_str("</c:v></c:pt>");
            }
        }
        s.push_str("</c:lvl>");
        l -= 1.0;
    }
    s.push_str("</c:multiLvlStrCache>");
    Some(s)
}

/// `refreshChartEx`: each `cx:numDim`/`cx:strDim` with `cx:f` gets the `cx:lvl` levels of its cells.
fn plan_chartex(root: Element<'_>, src: &Cells<'_>, p: &mut Plan) {
    let Some(cd) = root.first_child("cx:chartData") else { return };
    for d in cd.child_elements().filter(|e| e.name() == "cx:data") {
        for dim in d.child_elements() {
            let dn = dim.name();
            if dn != "cx:numDim" && dn != "cx:strDim" {
                continue;
            }
            let Some(areas) = dim.first_child("cx:f").and_then(|f| resolve_ref(&text_content(f), src, 0)) else { continue };
            let mut lv = String::new();
            let a0 = &areas[0];
            if dn == "cx:strDim" && areas.len() == 1 && a0.r0 != a0.r1 && a0.c0 != a0.c1 {
                let by_rows = a0.r1 - a0.r0 >= a0.c1 - a0.c0;
                let (n_pts, n_lvl) =
                    if by_rows { (a0.r1 - a0.r0 + 1.0, a0.c1 - a0.c0 + 1.0) } else { (a0.c1 - a0.c0 + 1.0, a0.r1 - a0.r0 + 1.0) };
                if n_pts > MAX_AREA_CELLS || n_lvl > 10.0 {
                    continue;
                }
                let mut l = n_lvl - 1.0;
                while l >= 0.0 {
                    lv.push_str(&format!("<cx:lvl ptCount=\"{}\">", uo_jsnum::format::to_js_string(n_pts)));
                    let mut k = 0.0;
                    while k < n_pts {
                        let cell =
                            if by_rows { src.cell(&a0.sheet, a0.r0 + k, a0.c0 + l) } else { src.cell(&a0.sheet, a0.r0 + l, a0.c0 + k) };
                        if let Some(c) = cell
                            && !c.text.is_empty()
                        {
                            lv.push_str(&format!("<cx:pt idx=\"{}\">", uo_jsnum::format::to_js_string(k)));
                            text_escape(&mut lv, &c.text);
                            lv.push_str("</cx:pt>");
                        }
                        k += 1.0;
                    }
                    lv.push_str("</cx:lvl>");
                    l -= 1.0;
                }
            } else {
                let Some(cells) = cells_of(&areas, src, false) else { continue };
                if dn == "cx:numDim" {
                    let fmt = cells.iter().flatten().find(|c| c.num.is_some()).map_or("General", |c| c.fmt.as_str());
                    lv.push_str(&format!("<cx:lvl ptCount=\"{}\" formatCode=\"", cells.len()));
                    attr_escape(&mut lv, fmt);
                    lv.push_str("\">");
                    for (i, c) in cells.iter().enumerate() {
                        if let Some(n) = c.as_ref().and_then(|c| c.num) {
                            lv.push_str(&format!("<cx:pt idx=\"{i}\">"));
                            text_escape(&mut lv, &uo_jsnum::format::to_js_string(n));
                            lv.push_str("</cx:pt>");
                        }
                    }
                } else {
                    lv.push_str(&format!("<cx:lvl ptCount=\"{}\">", cells.len()));
                    for (i, c) in cells.iter().enumerate() {
                        if let Some(c) = c
                            && !c.text.is_empty()
                        {
                            lv.push_str(&format!("<cx:pt idx=\"{i}\">"));
                            text_escape(&mut lv, &c.text);
                            lv.push_str("</cx:pt>");
                        }
                    }
                }
                lv.push_str("</cx:lvl>");
            }
            // `dim.children = [f, ...lvls]`: every other child goes
            p.at.push((dim.id(), Repl::Ref { f: "cx:f", cache: lv, drop: &["*"] }));
        }
    }
}

/// Writes `e` with the rewrites of the plan.
fn write(out: &mut String, e: Element<'_>, plan: &Plan) {
    out.push('<');
    out.push_str(e.name());
    for a in e.attrs() {
        out.push(' ');
        out.push_str(a.name);
        out.push_str("=\"");
        attr_escape(out, a.value);
        out.push('"');
    }
    out.push('>');
    if let Some((_, Repl::Ref { f, cache, drop })) = plan.at.iter().find(|(id, _)| *id == e.id()) {
        // `[f, cache, ...ext]` (texts dropped); a missing `f` is `undefined` in the TS array: nothing
        if let Some(fe) = e.first_child(f) {
            write(out, fe, plan);
        }
        out.push_str(cache);
        if drop.first() != Some(&"*") {
            for c in e.child_elements() {
                if !drop.contains(&c.name()) {
                    write(out, c, plan);
                }
            }
        }
    } else {
        for n in e.children() {
            match n {
                Node::Element(c) => write(out, c, plan),
                Node::Text(t) | Node::CData(t) => text_escape(out, t),
                _ => {}
            }
        }
    }
    out.push_str("</");
    out.push_str(e.name());
    out.push('>');
}

/// The chart part as `parseChart` gives it to `parseChartSpace`: canonical, with the caches rewritten from the cells.
fn chart_root(xml: &str, cells: &Cells<'_>) -> Option<Document> {
    // an unreadable part is an empty `c:chartSpace` (the TS `catch`)
    let Ok(doc) = parse(xml.to_owned(), &xl_options()) else { return parse("<c:chartSpace/>", &ParseOptions::LENIENT).ok() };
    let p = plan(doc.root(), cells);
    if p.at.is_empty() {
        return Some(doc);
    }
    let mut s = String::with_capacity(xml.len() + 256);
    write(&mut s, doc.root(), &p);
    Some(parse(s, &ParseOptions { normalize_eol: false, ..ParseOptions::LENIENT }).unwrap_or(doc))
}

/// `chartFileFormat`: `docProps/app.xml` `AppVersion` ≤ 12 → Excel 2007 (`Mso2007`), else the modern meaning.
fn file_format(book: &Book) -> FileFormat {
    let app = book.pkg.find_text("docProps/app.xml").ok().flatten().map(std::borrow::Cow::into_owned).unwrap_or_default();
    let v = app.find("<AppVersion>").map(|i| &app[i + 12..]).and_then(|r| r.find('<').map(|j| &r[..j]));
    let major = v.map_or(f64::NAN, |s| uo_jsnum::parse::parse_int(uo_jsnum::text::trim(s), 10));
    if major.is_finite() && major <= 12.0 { FileFormat::Mso2007 } else { FileFormat::Modern }
}

/// `userShapesOf`: the chart drawing of the part (`chartUserShapes` in its rels), canonical.
fn user_shapes(book: &Book, part: &str) -> Option<Document> {
    let slash = part.rfind('/')?;
    let rels = format!("{}/_rels/{}.rels", &part[..slash], &part[slash + 1..]);
    let text = book.pkg.find_text(&rels).ok().flatten()?;
    let root = parse(text.into_owned(), &crate::read::SHEET_XML).ok()?;
    for rel in root.root().children_named("Relationship") {
        let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else { continue };
        if id.is_empty() || target.is_empty() || rel.attr("TargetMode") == Some("External") {
            continue;
        }
        if !rel.attr("Type").unwrap_or("").ends_with("/chartUserShapes") {
            continue;
        }
        let t = crate::read::workbook::resolve_rel_target(part, target);
        let Some(xml) = book.pkg.find_text(&t).ok().flatten() else { continue };
        return parse(xml.into_owned(), &xl_options()).ok();
    }
    None
}

/// One parsed chart (the host and the model).
pub struct ChartModel {
    pub host: SheetHost,
    pub space: ChartSpace,
}

/// `chartModelFor`: the model of a chart part of the workbook, with the data of the cells.
#[must_use]
pub fn chart_model(book: &Book, part: &str, xml: &str) -> Option<ChartModel> {
    let no_theme = !book.pkg.part_names().any(|p| {
        let l = p.to_ascii_lowercase();
        l.strip_prefix("xl/theme/").is_some_and(|r| r.ends_with(".xml") && !r.contains('/') && r.len() > 4)
    });
    let theme = &book.cx.theme;
    let mut scheme = Scheme::from_pairs(&FALLBACK);
    if no_theme {
        for (k, v) in OFFICE_2023 {
            scheme.set(k, v);
        }
    } else {
        for (k, v) in &theme.by_name {
            scheme.set(k, v);
        }
    }
    let (minor, major) = if no_theme {
        (Box::from("Aptos"), Some(Box::from("Aptos Display")))
    } else {
        (theme.minor.as_deref().unwrap_or("Calibri").into(), theme.major.as_deref().map(Box::from))
    };
    let widths = if no_theme {
        vec![1.0, 1.5, 2.0]
    } else {
        match &theme.line_widths {
            Some(w) if !w.is_empty() => w.clone(),
            _ => vec![0.5, 1.0, 1.5],
        }
    };
    let host = SheetHost {
        scheme,
        minor,
        major,
        fills: if no_theme { None } else { fills_doc(theme) },
        format: file_format(book),
        user_shapes: user_shapes(book, part),
        widths,
    };
    let cells = Cells::new(book);
    let root = chart_root(xml, &cells)?;
    let space = parse_chart_space(root.root(), &host);
    Some(ChartModel { host, space })
}

/// The chart models of a viewer, least recently used first (M5: [`MAX_CHART_MODELS`]).
#[derive(Default)]
pub struct ChartCache {
    entries: Vec<(Box<str>, Arc<ChartModel>)>,
}

impl ChartCache {
    /// The model of `part` (built on a miss).
    pub fn get(&mut self, book: &Book, part: &str, xml: impl FnOnce() -> Option<String>) -> Option<Arc<ChartModel>> {
        if let Some(i) = self.entries.iter().position(|(k, _)| &**k == part) {
            let e = self.entries.remove(i);
            let m = Arc::clone(&e.1);
            self.entries.push(e);
            return Some(m);
        }
        let x = xml()?;
        let m = Arc::new(chart_model(book, part, &x)?);
        if self.entries.len() >= MAX_CHART_MODELS {
            self.entries.remove(0);
        }
        self.entries.push((part.into(), Arc::clone(&m)));
        Some(m)
    }

    /// Models held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No model held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops every model (M1).
    pub fn clear(&mut self) {
        self.entries = Vec::new();
    }
}
