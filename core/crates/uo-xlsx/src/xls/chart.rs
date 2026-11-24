//! The chart substream of BIFF8 (BOF dt=0x20 … EOF, [MS-XLS] 2.4 "Chart") → `c:chartSpace` for the single chart engine
//! (`chart.ts`).
//!
//! The substream is a tree (BEGIN/END): CHART { FRAME, SERIES { AI, SERIESTEXT, DATAFORMAT { LINEFORMAT, AREAFORMAT,
//! MARKERFORMAT… }, SERTOCRT }, TEXT (titles), AXISPARENT { AXIS { VALUERANGE/CATSERRANGE, TICK, AXISLINEFORMAT }, PLOTAREA,
//! FRAME, CHARTFORMAT { BAR/LINE/PIE…, LEGEND } } }.
//!
//! Values: the `.xls` almost never carries the cache of the series (SIINDEX empty — not even Excel writes it); the series are
//! resolved through their REFERENCES (AI) against the cells of the workbook, with the cache as a fallback. "Automatic"
//! formatting follows Excel 97–2003 (which current Excel keeps on opening an .xls): fills from palette 24–31, lines 32–39,
//! grey plot area, markers in a cycle.
//!
//! This is a port of `sheet-core/src/xls/chart.ts` as it is: `uo_charts::biff` (the `.ppt` charts) started from it but reads the
//! Office 2007 extension records and the trendlines, so it does not give the bytes of this converter (record C2, decision C2-4).

use std::collections::HashMap;

use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{floor, max, min, round};

use super::biff::{Codepage, Rd, Records, read_xl_string, rt};
use super::formula::{Area, formula_areas};
use super::globals::XlsGlobals;
use super::shapes::foptfill_xml;
use super::sheet::{CellVal, CellValue, Rect};
use super::styles_xml::{font_at, normalize_format};
use super::xmlw::{XML_HEAD, col_name_s, xml_text_s};
use uo_drawingml::odraw::{Flavor, PropTable, read_fopt, recs_in, rt as oa_rt};

/// Cells of the workbook for the series references.
pub struct ChartContext<'a> {
    pub g: &'a XlsGlobals,
    /// Sheet names in BOUNDSHEET order.
    pub sheet_names: Vec<String>,
    /// The values kept per sheet (BOUNDSHEET index) by `readSheet`.
    pub values: Vec<Option<&'a HashMap<u64, CellValue>>>,
}

impl ChartContext<'_> {
    fn cell(&self, sheet: usize, row: u32, col: u32) -> Option<&CellValue> {
        self.values.get(sheet).copied().flatten().and_then(|m| m.get(&(u64::from(row) * 16384 + u64::from(col))))
    }
}

mod ct {
    pub const CHART: u16 = 0x1002;
    pub const SERIES: u16 = 0x1003;
    pub const DATAFORMAT: u16 = 0x1006;
    pub const LINEFORMAT: u16 = 0x1007;
    pub const MARKERFORMAT: u16 = 0x1009;
    pub const AREAFORMAT: u16 = 0x100a;
    pub const PIEFORMAT: u16 = 0x100b;
    pub const ATTACHEDLABEL: u16 = 0x100c;
    pub const SERIESTEXT: u16 = 0x100d;
    pub const CHARTFORMAT: u16 = 0x1014;
    pub const LEGEND: u16 = 0x1015;
    pub const BAR: u16 = 0x1017;
    pub const LINE: u16 = 0x1018;
    pub const PIE: u16 = 0x1019;
    pub const AREA: u16 = 0x101a;
    pub const SCATTER: u16 = 0x101b;
    pub const AXIS: u16 = 0x101d;
    pub const TICK: u16 = 0x101e;
    pub const VALUERANGE: u16 = 0x101f;
    pub const CATSERRANGE: u16 = 0x1020;
    pub const AXISLINEFORMAT: u16 = 0x1021;
    pub const DEFAULTTEXT: u16 = 0x1024;
    pub const TEXT: u16 = 0x1025;
    pub const FONTX: u16 = 0x1026;
    pub const OBJECTLINK: u16 = 0x1027;
    pub const FRAME: u16 = 0x1032;
    pub const BEGIN: u16 = 0x1033;
    pub const END: u16 = 0x1034;
    pub const PLOTAREA: u16 = 0x1035;
    pub const D3: u16 = 0x103a;
    pub const RADAR: u16 = 0x103e;
    pub const RADARAREA: u16 = 0x1040;
    pub const AXISPARENT: u16 = 0x1041;
    pub const SHTPROPS: u16 = 0x1044;
    pub const SERTOCRT: u16 = 0x1045;
    pub const SERPARENT: u16 = 0x104a;
    pub const IFMT: u16 = 0x104e;
    pub const SERFMT: u16 = 0x105d;
    pub const AXCEXT: u16 = 0x1062;
    pub const BOPPOP: u16 = 0x1061;
    pub const SIINDEX: u16 = 0x1065;
    pub const GELFRAME: u16 = 0x1066;
}

/// Nesting of BEGIN/END followed (`stack.length > 64`).
const MAX_DEPTH: usize = 64;
/// Records of a chart substream read (a real chart has a few thousand): the TS has no bound.
const MAX_RECORDS: usize = 500_000;
/// Areas of chart series kept per sheet (record C2, decision C2-8).
const MAX_WANTED: usize = 512;
/// Values read for the series of ONE chart in all (a real chart: thousands); the TS has no bound.
const CHART_VALUE_BUDGET: usize = 2_000_000;
/// Values of one series (an area of a series is cut at 32,001 × 32,001 cells in the TS; Excel 97–2003 holds 32,000 points).
const MAX_VALUES: usize = 1_000_000;

type NodeId = usize;

struct NodeRec {
    t: u16,
    data: Vec<u8>,
    conts: Vec<usize>,
    kids: Vec<NodeId>,
    /// The GELFRAME (Office 2007 fill in OfficeArt) that completes this AREAFORMAT.
    gel: Option<Vec<u8>>,
}

struct Tree {
    nodes: Vec<NodeRec>,
    root: Vec<NodeId>,
}

#[derive(Clone, Copy)]
enum Level {
    Root,
    Node(NodeId),
    /// the children of a BEGIN without a record before it: built and dropped
    Dead(usize),
}

/// Records → tree by the BEGIN/END (the record before the BEGIN is the parent).
fn tree(recs: Vec<ChartRec>) -> Tree {
    let mut t = Tree { nodes: Vec::new(), root: Vec::new() };
    let mut dead: Vec<Vec<NodeId>> = Vec::new();
    let mut stack: Vec<Level> = vec![Level::Root];
    let mut last: Option<NodeId> = None;
    let mut first = true;
    for rec in recs {
        if first {
            first = false;
            if rec.ty == rt::BOF {
                continue;
            }
        }
        if rec.ty == rt::EOF {
            break;
        }
        if rec.ty == ct::BEGIN {
            stack.push(match last {
                Some(id) => Level::Node(id),
                None => {
                    dead.push(Vec::new());
                    Level::Dead(dead.len() - 1)
                }
            });
            last = None;
            if stack.len() > MAX_DEPTH {
                break;
            }
            continue;
        }
        if rec.ty == ct::END {
            if stack.len() > 1 {
                stack.pop();
            }
            last = None;
            continue;
        }
        if t.nodes.len() >= MAX_RECORDS {
            break;
        }
        let id = t.nodes.len();
        // the GELFRAME is also kept by the AREAFORMAT it completes (below)
        let gel_copy = if rec.ty == ct::GELFRAME { Some(rec.data.clone()) } else { None };
        t.nodes.push(NodeRec { t: rec.ty, data: rec.data, conts: rec.conts, kids: Vec::new(), gel: None });
        last = Some(id);
        let level = *stack.last().unwrap_or(&Level::Root);
        // GELFRAME completes the AREAFORMAT before it
        if let Some(data) = gel_copy {
            let list: &[NodeId] = match level {
                Level::Root => &t.root,
                Level::Node(p) => &t.nodes[p].kids,
                Level::Dead(d) => &dead[d],
            };
            if let Some(&area) = list.iter().rev().find(|&&k| t.nodes[k].t == ct::AREAFORMAT) {
                t.nodes[area].gel = Some(data);
            }
        }
        match level {
            Level::Root => t.root.push(id),
            Level::Node(p) => t.nodes[p].kids.push(id),
            Level::Dead(d) => dead[d].push(id),
        }
    }
    t
}

/// A chart record (owned: the chart is converted after the sheets were read).
pub struct ChartRec {
    pub ty: u16,
    pub data: Vec<u8>,
    pub conts: Vec<usize>,
}

/// The records of a range of the workbook stream.
pub fn chart_records(buf: &[u8], range: (usize, usize)) -> Vec<ChartRec> {
    Records::new(buf, range.0, range.1)
        .take(MAX_RECORDS)
        .map(|r| ChartRec { ty: r.ty, data: r.data.into_owned(), conts: r.conts })
        .collect()
}

impl Tree {
    fn n(&self, id: NodeId) -> &NodeRec {
        &self.nodes[id]
    }
    fn rd(&self, id: NodeId) -> Rd<'_> {
        Rd(&self.nodes[id].data)
    }
    fn kid(&self, id: Option<NodeId>, ty: u16) -> Option<NodeId> {
        id.and_then(|i| self.nodes[i].kids.iter().copied().find(|&k| self.nodes[k].t == ty))
    }
    fn kids_of(&self, id: Option<NodeId>, ty: u16) -> Vec<NodeId> {
        id.map_or_else(Vec::new, |i| self.nodes[i].kids.iter().copied().filter(|&k| self.nodes[k].t == ty).collect())
    }
}

// ───────────────────────────── references ─────────────────────────────

/// ixti → the sheet index in the workbook itself (`None` = external/invalid).
fn sheet_of(g: &XlsGlobals, ixti: i32) -> Option<usize> {
    let x = g.xti.get(usize::try_from(ixti).ok()?)?;
    if !g.supbooks.get(usize::from(x.book)).copied().unwrap_or(false) || x.tab < 0 {
        return None;
    }
    let tab = usize::try_from(x.tab).ok()?;
    if tab >= g.sheets.len() { None } else { Some(tab) }
}

struct Ai {
    id: u8,
    rt: u8,
    unlinked: bool,
    ifmt: u16,
    areas: Vec<Area>,
}

fn ai_of(r: &Rd<'_>) -> Ai {
    Ai { id: r.u8(0), rt: r.u8(1), unlinked: r.u16(2) & 1 != 0, ifmt: r.u16(4), areas: formula_areas(r, 8, usize::from(r.u16(6))) }
}

/// `chartRefs`: the areas the series (AI) of ALL the charts reference, by sheet — `readSheet` keeps only the values of those cells.
pub fn chart_refs(buf: &[u8], g: &XlsGlobals) -> HashMap<usize, Vec<Rect>> {
    let mut out: HashMap<usize, Vec<Rect>> = HashMap::new();
    if !g.biff8 {
        return out;
    }
    let mut off = 0usize;
    while off + 4 <= buf.len() {
        let t = usize::from(buf[off]) | (usize::from(buf[off + 1]) << 8);
        let len = usize::from(buf[off + 2]) | (usize::from(buf[off + 3]) << 8);
        if t == usize::from(rt::AI) && buf.get(off + 5) == Some(&2) {
            let r = Rd(buf.get(off + 4..(off + 4 + len).min(buf.len())).unwrap_or(&[]));
            for a in formula_areas(&r, 8, usize::from(r.u16(6))) {
                let Some(s) = sheet_of(g, a.ixti) else { continue };
                let list = out.entry(s).or_default();
                // a series does not pass 32,000 points in Excel 97–2003; the TS keeps 4,096 areas per sheet, here 512 (`readSheet` tests
                // every cell against them: a hostile file would make that quadratic)
                if list.len() < MAX_WANTED {
                    list.push([a.r1, a.c1, a.r2.min(a.r1 + 32000), a.c2.min(a.c1 + 32000)]);
                }
            }
        }
        off += 4 + len;
    }
    out
}

// ───────────────────────────── colours and formats ─────────────────────────────

fn hex(c: &str) -> String {
    c.replace('#', "").to_uppercase()
}

fn icv_color(g: &XlsGlobals, icv: u16) -> String {
    if icv < 64 {
        return g.palette.get(usize::from(icv)).cloned().unwrap_or_else(|| "#000000".to_owned());
    }
    if icv == 0x41 || icv == 0x4e {
        return "#FFFFFF".to_owned();
    }
    if icv == 0x50 {
        return "#FFFFE1".to_owned();
    }
    "#000000".to_owned()
}

/// The automatic FILL colour of series/point n (palette 24–31 of Excel 97).
fn auto_fill(g: &XlsGlobals, n: usize) -> String {
    g.palette.get(24 + (n % 8)).cloned().unwrap_or_else(|| "#9999FF".to_owned())
}

/// The automatic LINE colour of series n (palette 32–39).
fn auto_line(g: &XlsGlobals, n: usize) -> String {
    g.palette.get(32 + (n % 8)).cloned().unwrap_or_else(|| "#000080".to_owned())
}

const AUTO_MARKERS: [&str; 9] = ["diamond", "square", "triangle", "x", "star", "circle", "plus", "dash", "dot"];
const MARKERS: [&str; 10] = ["none", "square", "diamond", "triangle", "x", "star", "dash", "dash", "circle", "plus"];

fn line_w(we: i16) -> u32 {
    if we < 0 {
        3175
    } else if we == 0 {
        9525
    } else if we == 1 {
        19050
    } else {
        28575
    }
}

const DASH: [&str; 5] = ["", "dash", "sysDot", "dashDot", "lgDashDotDot"];

fn solid(c: &str) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex(c))
}

/// LINEFORMAT → `<a:ln>`; `auto` = the automatic colour.
fn ln_xml(g: &XlsGlobals, t: &Tree, n: Option<NodeId>, auto: &str, auto_w: u32) -> String {
    let Some(n) = n else { return format!("<a:ln w=\"{auto_w}\">{}</a:ln>", solid(auto)) };
    let r = t.rd(n);
    let (lns, we, grbit, icv) = (r.u16(4), r.i16(6), r.u16(8), r.u16(10));
    if lns == 5 {
        return "<a:ln><a:noFill/></a:ln>".to_owned();
    }
    let is_auto = grbit & 1 != 0;
    let color = if is_auto { auto.to_owned() } else { icv_color(g, icv) };
    let w = if is_auto { auto_w } else { line_w(we) };
    let dash = DASH.get(usize::from(lns)).filter(|d| !d.is_empty()).map(|d| format!("<a:prstDash val=\"{d}\"/>")).unwrap_or_default();
    format!("<a:ln w=\"{w}\">{}{dash}</a:ln>", solid(&color))
}

/// AREAFORMAT → fill; `auto` = the automatic colour (`None` = no fill).
fn fill_xml(g: &XlsGlobals, t: &Tree, n: Option<NodeId>, auto: Option<&str>) -> String {
    let auto_fill = || auto.map_or_else(|| "<a:noFill/>".to_owned(), solid);
    let Some(n) = n else { return auto_fill() };
    let r = t.rd(n);
    let (fls, grbit, icv_fore) = (r.u16(8), r.u16(10), r.u16(12));
    if grbit & 1 != 0 {
        return auto_fill();
    }
    if fls == 0 {
        return "<a:noFill/>".to_owned();
    }
    // Excel 2007+: the GELFRAME holds the true fill (RGB, gradient with fillShadeColors) — the AREAFORMAT only the palette approximation
    if let Some(gel) = &t.n(n).gel {
        let mut props = PropTable::new();
        for c in recs_in(gel, 0, gel.len() as u64) {
            if c.rec_type == oa_rt::FOPT || c.rec_type == oa_rt::SECONDARY_FOPT || c.rec_type == oa_rt::TERTIARY_FOPT {
                read_fopt(&c, &mut props, Flavor::Sheet);
            }
        }
        if let Some(x) = foptfill_xml(g, &props).filter(|x| !x.is_empty()) {
            return x;
        }
    }
    solid(&icv_color(g, icv_fore))
}

/// The format code of an XF of the workbook (the "linked to source" of the series).
fn xf_format(g: &XlsGlobals, xf: Option<u16>) -> String {
    let id = xf.and_then(|x| g.xfs.get(usize::from(x))).map_or(0, |x| x.fmt);
    ifmt_code(g, id)
}

fn ifmt_code(g: &XlsGlobals, id: u16) -> String {
    let own = g.format_of(id);
    if let Some(o) = own
        && (id >= 50 || (5..=8).contains(&id) || (41..=44).contains(&id))
    {
        return normalize_format(o);
    }
    match uo_numfmt::builtin_format_code(u32::from(id), uo_numfmt::pt_br()) {
        Some(c) => c.to_owned(),
        None => own.map_or_else(|| "General".to_owned(), normalize_format),
    }
}

// ───────────────────────────── text ─────────────────────────────

#[derive(Clone)]
struct TextProps {
    size: f64,
    bold: bool,
    italic: bool,
    underline: bool,
    color: String,
    face: String,
}

fn font_props(g: &XlsGlobals, ifnt: u16) -> TextProps {
    let f = font_at(g, ifnt);
    TextProps {
        size: f64::from(f.height) / 20.0,
        bold: f.weight >= 600,
        italic: f.italic,
        underline: f.underline != 0,
        color: if f.color == 0x7fff { "#000000".to_owned() } else { icv_color(g, f.color) },
        face: f.name.clone(),
    }
}

fn rpr_attrs(p: &TextProps) -> String {
    format!(
        "sz=\"{}\" b=\"{}\" i=\"{}\"{}",
        to_js_string(round(p.size * 100.0)),
        i32::from(p.bold),
        i32::from(p.italic),
        if p.underline { " u=\"sng\"" } else { "" }
    )
}

fn rpr_body(p: &TextProps) -> String {
    format!("{}<a:latin typeface=\"{}\"/>", solid(&p.color), xml_text_s(&p.face))
}

fn def_rpr(p: &TextProps) -> String {
    format!("<a:defRPr {}>{}</a:defRPr>", rpr_attrs(p), rpr_body(p))
}

/// The rotation of the TEXT/TICK (trot) → the attributes of `a:bodyPr`.
fn body_pr(trot: u16) -> String {
    if trot == 255 {
        "<a:bodyPr vert=\"wordArtVert\"/>".to_owned()
    } else if trot > 0 && trot <= 90 {
        format!("<a:bodyPr rot=\"{}\" vert=\"horz\"/>", -i64::from(trot) * 60000)
    } else if trot > 90 && trot <= 180 {
        format!("<a:bodyPr rot=\"{}\" vert=\"horz\"/>", (i64::from(trot) - 90) * 60000)
    } else {
        "<a:bodyPr/>".to_owned()
    }
}

fn tx_pr(p: &TextProps, trot: u16) -> String {
    format!("<c:txPr>{}<a:lstStyle/><a:p><a:pPr>{}</a:pPr><a:endParaRPr lang=\"pt-BR\"/></a:p></c:txPr>", body_pr(trot), def_rpr(p))
}

/// TEXT → properties (the font of the FONTX, the colour of the TEXT when not automatic).
fn text_node_props(g: &XlsGlobals, t: &Tree, n: Option<NodeId>, base: &TextProps) -> TextProps {
    let Some(n) = n else { return base.clone() };
    let mut p = match t.kid(Some(n), ct::FONTX) {
        Some(fx) => font_props(g, t.rd(fx).u16(0)),
        None => base.clone(),
    };
    let r = t.rd(n);
    if r.u16(24) & 1 == 0 {
        p.color = icv_color(g, r.u16(26));
    }
    p
}

/// `text.split(/\r\n|\r|\n/)`.
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let (mut start, mut i) = (0usize, 0usize);
    while i < b.len() {
        if b[i] == b'\r' || b[i] == b'\n' {
            out.push(&text[start..i]);
            if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

fn rich_title(text: &str, p: &TextProps, trot: u16) -> String {
    let paras: String = split_lines(text)
        .iter()
        .map(|line| {
            format!(
                "<a:p><a:pPr>{}</a:pPr><a:r><a:rPr lang=\"pt-BR\" {}>{}</a:rPr><a:t>{}</a:t></a:r></a:p>",
                def_rpr(p),
                rpr_attrs(p),
                rpr_body(p),
                xml_text_s(line)
            )
        })
        .collect();
    format!(
        "<c:title><c:tx><c:rich>{}<a:lstStyle/>{paras}</c:rich></c:tx><c:overlay val=\"0\"/><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr></c:title>",
        body_pr(trot)
    )
}

/// The literal text of the SERIESTEXT of a TEXT/SERIES.
fn series_text(t: &Tree, n: Option<NodeId>) -> Option<String> {
    let st = t.kid(n, ct::SERIESTEXT)?;
    Some(read_xl_string(&t.rd(st), 2, true, &Codepage::new(1252), 1, &t.n(st).conts).0)
}

// ───────────────────────────── conversion ─────────────────────────────

/// A category/value of a cell or of the chart's own cache.
#[derive(Clone, Debug)]
enum CV {
    Num(f64),
    Str(String),
}

impl CV {
    fn text(&self) -> String {
        match self {
            CV::Num(n) => to_js_string(*n),
            CV::Str(s) => s.clone(),
        }
    }
}

struct SeriesData {
    name: Option<String>,
    name_ref: Option<String>,
    vals: Vec<Option<f64>>,
    val_ref: Option<String>,
    val_fmt: String,
    cats: Option<Vec<Option<CV>>>,
    cat_ref: Option<String>,
    cat_fmt: Option<String>,
    cats_numeric: bool,
    bubbles: Option<Vec<Option<f64>>>,
    node: NodeId,
    group: usize,
}

/// The values of a list of areas (row by row) and the reference text.
fn area_values(ctx: &ChartContext<'_>, areas: &[Area], budget: &mut usize) -> (Vec<Option<CellValue>>, Option<String>) {
    let mut vals: Vec<Option<CellValue>> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    for a in areas {
        let Some(s) = sheet_of(ctx.g, a.ixti) else { return (Vec::new(), None) };
        let name = ctx.sheet_names.get(s).map_or("", String::as_str);
        let plain = {
            let mut it = name.chars();
            it.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && it.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        };
        let q = if plain { name.to_owned() } else { format!("'{}'", name.replace('\'', "''")) };
        let abs = |r: u32, c: u32| format!("${}${}", col_name_s(c), r + 1);
        refs.push(if a.r1 == a.r2 && a.c1 == a.c2 {
            format!("{q}!{}", abs(a.r1, a.c1))
        } else {
            format!("{q}!{}:{}", abs(a.r1, a.c1), abs(a.r2, a.c2))
        });
        let r2 = a.r2.min(a.r1 + 32000);
        let c2 = a.c2.min(a.c1 + 32000);
        'rows: for r in a.r1..=r2 {
            for c in a.c1..=c2 {
                if vals.len() >= MAX_VALUES || *budget == 0 {
                    break 'rows;
                }
                *budget -= 1;
                vals.push(ctx.cell(s, r, c).cloned());
            }
        }
    }
    let r = if refs.len() == 1 { refs.pop() } else { Some(format!("({})", refs.join(","))) };
    (vals, r)
}

type Cache = HashMap<u16, HashMap<u16, HashMap<u16, CV>>>;

/// The chart's own cache (SIINDEX + NUMBER/LABEL): `[index][series][point]`.
fn chart_cache(t: &Tree) -> Cache {
    let mut out: Cache = HashMap::new();
    let mut cur: Option<u16> = None;
    for &n in &t.root {
        let r = t.rd(n);
        let ty = t.n(n).t;
        if ty == ct::SIINDEX {
            let k = r.u16(0);
            out.insert(k, HashMap::new());
            cur = Some(k);
            continue;
        }
        let Some(k) = cur else { continue };
        let (s, p) = (r.u16(2), r.u16(0));
        let v = if ty == rt::NUMBER {
            CV::Num(r.f64(6))
        } else if ty == rt::LABEL {
            CV::Str(read_xl_string(&r, 6, true, &Codepage::new(1252), 2, &t.n(n).conts).0)
        } else {
            continue;
        };
        out.entry(k).or_default().entry(s).or_default().insert(p, v);
    }
    out
}

fn num_xml(vals: &[Option<f64>], fmt: &str) -> String {
    let mut pts = String::new();
    for (i, v) in vals.iter().enumerate() {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            pts.push_str(&format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", to_js_string(v)));
        }
    }
    format!("<c:numCache><c:formatCode>{}</c:formatCode><c:ptCount val=\"{}\"/>{pts}</c:numCache>", xml_text_s(fmt), vals.len())
}

fn str_xml(vals: &[Option<CV>]) -> String {
    let mut pts = String::new();
    for (i, v) in vals.iter().enumerate() {
        // `v !== undefined && v !== ''`
        let Some(v) = v else { continue };
        let s = v.text();
        if !s.is_empty() {
            pts.push_str(&format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", xml_text_s(&s)));
        }
    }
    format!("<c:strCache><c:ptCount val=\"{}\"/>{pts}</c:strCache>", vals.len())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bar,
    Line,
    Pie,
    Area,
    Scatter,
    Radar,
    Bubble,
}

struct GroupInfo {
    node: NodeId,
    parent: NodeId,
    kind: Kind,
    type_node: NodeId,
    d3: bool,
    varied: bool,
}

/// `chartSpaceXml`: converts a chart substream. `None` = empty/unsupported.
pub fn chart_space_xml(ctx: &ChartContext<'_>, records: Vec<ChartRec>) -> Option<String> {
    let g = ctx.g;
    if !g.biff8 {
        return None;
    }
    let t = tree(records);
    let chart = t.root.iter().copied().find(|&n| t.n(n).t == ct::CHART);
    let chart = chart?;
    let cache = chart_cache(&t);

    // default fonts: DEFAULTTEXT 2 (all the text) and 3 (axes)
    let mut base_text = font_props(g, 0);
    let mut axis_text: Option<TextProps> = None;
    let ck = &t.n(chart).kids;
    for (i, &n) in ck.iter().enumerate() {
        if t.n(n).t != ct::DEFAULTTEXT {
            continue;
        }
        let Some(&tx) = ck.get(i + 1) else { continue };
        if t.n(tx).t != ct::TEXT {
            continue;
        }
        let p = text_node_props(g, &t, Some(tx), &base_text);
        match t.rd(n).u16(0) {
            2 => base_text = p,
            3 => axis_text = Some(p),
            _ => {}
        }
    }
    let axis_text = axis_text.unwrap_or_else(|| base_text.clone());

    // the groups (CHARTFORMAT) by AXISPARENT, in file order
    let parents = t.kids_of(Some(chart), ct::AXISPARENT);
    let mut groups: Vec<GroupInfo> = Vec::new();
    for &ap in &parents {
        for cf in t.kids_of(Some(ap), ct::CHARTFORMAT) {
            let tn = t.n(cf).kids.iter().copied().find(|&k| {
                matches!(t.n(k).t, ct::BAR | ct::LINE | ct::PIE | ct::AREA | ct::SCATTER | ct::RADAR | ct::RADARAREA | ct::BOPPOP)
            });
            let Some(tn) = tn else { continue };
            let mut kind = match t.n(tn).t {
                ct::BAR => Kind::Bar,
                ct::LINE => Kind::Line,
                ct::AREA => Kind::Area,
                ct::SCATTER => Kind::Scatter,
                ct::RADAR | ct::RADARAREA => Kind::Radar,
                _ => Kind::Pie,
            };
            if kind == Kind::Scatter && t.rd(tn).u16(4) & 1 != 0 {
                kind = Kind::Bubble;
            }
            groups.push(GroupInfo {
                node: cf,
                parent: ap,
                kind,
                type_node: tn,
                d3: t.kid(Some(cf), ct::D3).is_some(),
                varied: t.rd(cf).u16(16) & 1 != 0,
            });
        }
    }
    if groups.is_empty() {
        return None;
    }

    // series
    let mut value_budget = CHART_VALUE_BUDGET;
    let mut series: Vec<SeriesData> = Vec::new();
    for (s_idx, s) in t.kids_of(Some(chart), ct::SERIES).into_iter().enumerate() {
        if t.kid(Some(s), ct::SERPARENT).is_some() {
            continue; // trendline / error bar
        }
        let sr = t.rd(s);
        let mut sd = SeriesData {
            name: None,
            name_ref: None,
            vals: Vec::new(),
            val_ref: None,
            val_fmt: "General".to_owned(),
            cats: None,
            cat_ref: None,
            cat_fmt: None,
            cats_numeric: false,
            bubbles: None,
            node: s,
            group: t.kid(Some(s), ct::SERTOCRT).map_or(0, |k| usize::from(t.rd(k).u16(0))),
        };
        let s_key = u16::try_from(s_idx).unwrap_or(u16::MAX);
        for ai_node in t.kids_of(Some(s), rt::AI) {
            let ai = ai_of(&t.rd(ai_node));
            let resolved = if ai.rt == 2 && !ai.areas.is_empty() { Some(area_values(ctx, &ai.areas, &mut value_budget)) } else { None };
            let has_vals = resolved.as_ref().is_some_and(|(v, _)| !v.is_empty());
            match ai.id {
                0 => {
                    if ai.rt == 1 {
                        sd.name = series_text(&t, Some(s));
                    } else if has_vals && let Some((vals, rf)) = resolved {
                        sd.name = Some(match vals.first().and_then(Option::as_ref) {
                            None => String::new(),
                            Some(c) => match &c.v {
                                CellVal::Num(n) => to_js_string(*n),
                                CellVal::Str(s) => s.clone(),
                            },
                        });
                        sd.name_ref = rf;
                    }
                }
                1 => {
                    if has_vals {
                        if let Some((vals, rf)) = resolved {
                            sd.vals = vals
                                .iter()
                                .map(|c| c.as_ref().and_then(|c| if let CellVal::Num(n) = c.v { Some(n) } else { None }))
                                .collect();
                            sd.val_ref = rf;
                            let first_xf = vals.iter().flatten().next().map(|c| c.xf);
                            sd.val_fmt = if ai.unlinked { ifmt_code(g, ai.ifmt) } else { xf_format(g, first_xf) };
                        }
                    } else if let Some(c) = cache.get(&1).and_then(|m| m.get(&s_key)) {
                        let n = usize::from(sr.u16(6));
                        let len = if n != 0 { n } else { c.len() };
                        sd.vals = (0..len.min(MAX_VALUES))
                            .map(|i| match c.get(&u16::try_from(i).unwrap_or(u16::MAX)) {
                                Some(CV::Num(v)) => Some(*v),
                                _ => None,
                            })
                            .collect();
                    }
                }
                2 => {
                    if has_vals {
                        if let Some((vals, rf)) = resolved {
                            sd.cats = Some(
                                vals.iter()
                                    .map(|c| {
                                        c.as_ref().map(|c| match &c.v {
                                            CellVal::Num(n) => CV::Num(*n),
                                            CellVal::Str(s) => CV::Str(s.clone()),
                                        })
                                    })
                                    .collect(),
                            );
                            sd.cat_ref = rf;
                            sd.cats_numeric = vals.iter().all(|c| c.as_ref().is_none_or(|c| matches!(c.v, CellVal::Num(_))));
                            sd.cat_fmt = Some(xf_format(g, vals.iter().flatten().next().map(|c| c.xf)));
                        }
                    } else if let Some(c) = cache.get(&2).and_then(|m| m.get(&s_key)).filter(|c| !c.is_empty()) {
                        let n = c.keys().map(|&k| usize::from(k)).max().unwrap_or(0) + 1;
                        let cats: Vec<Option<CV>> = (0..n).map(|i| c.get(&u16::try_from(i).unwrap_or(u16::MAX)).cloned()).collect();
                        sd.cats_numeric = cats.iter().all(|v| v.as_ref().is_none_or(|v| matches!(v, CV::Num(_))));
                        sd.cats = Some(cats);
                    }
                }
                3 if has_vals => {
                    if let Some((vals, _)) = resolved {
                        sd.bubbles = Some(
                            vals.iter().map(|c| c.as_ref().and_then(|c| if let CellVal::Num(n) = c.v { Some(n) } else { None })).collect(),
                        );
                    }
                }
                _ => {}
            }
        }
        if sd.name.is_none()
            && let Some(tx) = series_text(&t, Some(s))
        {
            sd.name = Some(tx);
        }
        series.push(sd);
    }

    // ── date axis (AXCEXT fDateAxis): one position per time unit ──
    let mut date_axes: HashMap<NodeId, DateAxis> = HashMap::new();
    for &ap in &parents {
        let x_axis = t.kids_of(Some(ap), ct::AXIS).into_iter().find(|&a| t.rd(a).u16(0) == 0);
        let ext = t.kid(x_axis, ct::AXCEXT);
        let gs: Vec<usize> = (0..groups.len()).filter(|&i| groups[i].parent == ap).collect();
        let Some(ext) = ext else { continue };
        if gs.is_empty() || gs.iter().any(|&i| matches!(groups[i].kind, Kind::Pie | Kind::Scatter | Kind::Bubble | Kind::Radar)) {
            continue;
        }
        let sers: Vec<usize> = (0..series.len()).filter(|&i| gs.contains(&series[i].group)).collect();
        if let Some(da) = date_axis(g, &t, ext, &mut series, &sers) {
            date_axes.insert(ap, da);
        }
    }

    // ── axes (one pair per AXISPARENT) ──
    let mut ax_id = 100_000u32;
    let mut axes_xml: Vec<String> = Vec::new();
    let mut parent_axes: HashMap<NodeId, [u32; 2]> = HashMap::new();
    let mut titles: HashMap<u16, NodeId> = HashMap::new(); // OBJECTLINK → TEXT
    let mut title_nodes = t.kids_of(Some(chart), ct::TEXT);
    for &p in &parents {
        title_nodes.extend(t.kids_of(Some(p), ct::TEXT));
    }
    for tn in title_nodes {
        if let Some(ol) = t.kid(Some(tn), ct::OBJECTLINK) {
            titles.insert(t.rd(ol).u16(0), tn);
        }
    }
    for &ap in &parents {
        let Some(gi) = groups.iter().position(|x| x.parent == ap) else { continue };
        let gr = &groups[gi];
        if gr.kind == Kind::Pie {
            continue;
        }
        let secondary = t.rd(ap).u16(0) == 1;
        let ids = [ax_id, ax_id + 1];
        ax_id += 2;
        parent_axes.insert(ap, ids);
        let axes = t.kids_of(Some(ap), ct::AXIS);
        let x_axis = axes.iter().copied().find(|&a| t.rd(a).u16(0) == 0);
        let y_axis = axes.iter().copied().find(|&a| t.rd(a).u16(0) == 1);
        let xy = gr.kind == Kind::Scatter || gr.kind == Kind::Bubble;
        let horizontal = gr.kind == Kind::Bar && t.rd(gr.type_node).u16(4) & 1 != 0;
        let cat_range = t.kid(x_axis, ct::CATSERRANGE);
        let between = match cat_range {
            Some(c) => t.rd(c).u16(6) & 1 != 0,
            None => gr.kind == Kind::Bar,
        };
        let first = series.iter().find(|s| s.group == gi);
        let x_pos = if secondary {
            if horizontal { "r" } else { "t" }
        } else if horizontal {
            "l"
        } else {
            "b"
        };
        let y_pos = if secondary {
            if horizontal { "t" } else { "r" }
        } else if horizontal {
            "b"
        } else {
            "l"
        };
        let x_fmt = first.filter(|f| f.cats_numeric).and_then(|f| f.cat_fmt.clone());
        axes_xml.push(axis_xml(
            g,
            &t,
            x_axis,
            if xy { AxKind::Val } else { AxKind::Cat },
            ids[0],
            ids[1],
            x_pos,
            secondary,
            &axis_text,
            if secondary { None } else { titles.get(&3).copied() },
            x_fmt.as_deref(),
            between,
            xy,
            date_axes.get(&ap),
        ));
        let y_fmt = first.map_or_else(|| "General".to_owned(), |f| f.val_fmt.clone());
        axes_xml.push(axis_xml(
            g,
            &t,
            y_axis,
            AxKind::Val,
            ids[1],
            ids[0],
            y_pos,
            secondary,
            &axis_text,
            if secondary { None } else { titles.get(&2).copied() },
            Some(&y_fmt),
            between,
            xy,
            None,
        ));
    }

    // ── groups → c:xxxChart ──
    let mut plot = String::new();
    let mut order = 0usize;
    for (gi, gr) in groups.iter().enumerate() {
        let sers: Vec<usize> = (0..series.len()).filter(|&i| series[i].group == gi).collect();
        if sers.is_empty() {
            continue;
        }
        let tr = t.rd(gr.type_node);
        let mut ser_xml = String::new();
        for &si in &sers {
            ser_xml.push_str(&series_xml(g, &t, &series[si], gr.kind, gr.varied, order, si));
            order += 1;
        }

        let xml: String = match gr.kind {
            Kind::Bar => {
                let grbit = tr.u16(4);
                let grouping = if grbit & 4 != 0 {
                    "percentStacked"
                } else if grbit & 2 != 0 {
                    "stacked"
                } else {
                    "clustered"
                };
                let tag = if gr.d3 { "c:bar3DChart" } else { "c:barChart" };
                format!(
                    "<{tag}><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"{}\"/>{ser_xml}<c:gapWidth val=\"{}\"/>{}",
                    if grbit & 1 != 0 { "bar" } else { "col" },
                    i32::from(gr.varied),
                    tr.u16(2),
                    if gr.d3 { String::new() } else { format!("<c:overlap val=\"{}\"/>", i32::from(tr.i16(0)).clamp(-100, 100)) }
                )
            }
            Kind::Line | Kind::Area => {
                let grbit = tr.u16(0);
                let grouping = if grbit & 2 != 0 {
                    "percentStacked"
                } else if grbit & 1 != 0 {
                    "stacked"
                } else {
                    "standard"
                };
                let tag = if gr.kind == Kind::Line {
                    if gr.d3 { "c:line3DChart" } else { "c:lineChart" }
                } else if gr.d3 {
                    "c:area3DChart"
                } else {
                    "c:areaChart"
                };
                format!(
                    "<{tag}><c:grouping val=\"{grouping}\"/><c:varyColors val=\"{}\"/>{ser_xml}{}",
                    i32::from(gr.varied),
                    if gr.kind == Kind::Line && !gr.d3 { "<c:marker val=\"1\"/>" } else { "" }
                )
            }
            Kind::Pie => {
                let donut = tr.u16(2);
                if donut > 0 {
                    plot.push_str(&format!(
                        "<c:doughnutChart><c:varyColors val=\"1\"/>{ser_xml}<c:firstSliceAng val=\"{}\"/><c:holeSize val=\"{}\"/></c:doughnutChart>",
                        tr.u16(0) % 360,
                        i32::from(donut).clamp(10, 90)
                    ));
                } else {
                    let tag = if gr.d3 { "c:pie3DChart" } else { "c:pieChart" };
                    plot.push_str(&format!(
                        "<{tag}><c:varyColors val=\"1\"/>{ser_xml}{}</{tag}>",
                        if gr.d3 { String::new() } else { format!("<c:firstSliceAng val=\"{}\"/>", tr.u16(0) % 360) }
                    ));
                }
                continue;
            }
            Kind::Scatter => format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{ser_xml}"),
            Kind::Bubble => {
                let scale = if tr.u16(0) != 0 { tr.u16(0) } else { 100 };
                format!("<c:bubbleChart><c:varyColors val=\"0\"/>{ser_xml}<c:bubbleScale val=\"{scale}\"/>")
            }
            Kind::Radar => {
                format!(
                    "<c:radarChart><c:radarStyle val=\"{}\"/><c:varyColors val=\"0\"/>{ser_xml}",
                    if t.n(gr.type_node).t == ct::RADARAREA { "filled" } else { "marker" }
                )
            }
        };
        let ids = match parent_axes.get(&gr.parent) {
            Some(i) => *i,
            None => {
                let i = [ax_id, ax_id + 1];
                ax_id += 2;
                i
            }
        };
        let close = match gr.kind {
            Kind::Bar => {
                if gr.d3 {
                    "c:bar3DChart"
                } else {
                    "c:barChart"
                }
            }
            Kind::Line => {
                if gr.d3 {
                    "c:line3DChart"
                } else {
                    "c:lineChart"
                }
            }
            Kind::Area => {
                if gr.d3 {
                    "c:area3DChart"
                } else {
                    "c:areaChart"
                }
            }
            Kind::Scatter => "c:scatterChart",
            Kind::Bubble => "c:bubbleChart",
            Kind::Radar => "c:radarChart",
            Kind::Pie => "",
        };
        plot.push_str(&format!("{xml}<c:axId val=\"{}\"/><c:axId val=\"{}\"/></{close}>", ids[0], ids[1]));
    }
    if plot.is_empty() {
        return None;
    }

    // ── plot area and chart area ──
    let ap0 = parents.first().copied();
    let pa_idx = ap0.and_then(|a| t.n(a).kids.iter().position(|&k| t.n(k).t == ct::PLOTAREA));
    let plot_frame = match (ap0, pa_idx) {
        (Some(a), Some(i)) => t.n(a).kids.get(i + 1).copied(),
        _ => None,
    };
    // without PLOTAREA Excel does not paint the plot area (charts of Excel 2007+ saved in .xls)
    let plot_sp = match plot_frame.filter(|&f| t.n(f).t == ct::FRAME) {
        Some(f) => format!(
            "<c:spPr>{}{}</c:spPr>",
            fill_xml(g, &t, t.kid(Some(f), ct::AREAFORMAT), Some("#C0C0C0")),
            ln_xml(g, &t, t.kid(Some(f), ct::LINEFORMAT), "#808080", 9525)
        ),
        None => "<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>".to_owned(),
    };
    // the position of the INNER plot area (without the axis labels): the rectangle of the AXISPARENT in SPRC (1/4000 of the chart)
    let plot_layout = match ap0 {
        Some(a) => {
            let r = t.rd(a);
            spc_layout(r.i32(2), r.i32(6), r.i32(10), r.i32(14), true)
        }
        None => "<c:layout/>".to_owned(),
    };
    let chart_frame = t.kid(Some(chart), ct::FRAME);
    let chart_sp = format!(
        "<c:spPr>{}{}</c:spPr>",
        fill_xml(g, &t, t.kid(chart_frame, ct::AREAFORMAT), Some("#FFFFFF")),
        ln_xml(g, &t, t.kid(chart_frame, ct::LINEFORMAT), "#000000", 3175)
    );

    // ── title ──
    let mut title = String::new();
    if let Some(&tt) = titles.get(&1)
        && t.rd(tt).u16(24) & 0x40 == 0
    {
        let mut text = series_text(&t, Some(tt));
        if text.is_none() && series.len() == 1 {
            text = series[0].name.clone();
        }
        if let Some(text) = text.filter(|x| !x.is_empty()) {
            let mut base = base_text.clone();
            base.bold = true;
            base.size = base_text.size * 1.2;
            title = rich_title(&text, &text_node_props(g, &t, Some(tt), &base), t.rd(tt).u16(30));
        }
    }

    // ── legend ──
    let mut legend = String::new();
    let lg_node = groups.iter().find_map(|x| t.kid(Some(x.node), ct::LEGEND));
    if let Some(lg) = lg_node {
        let lr = t.rd(lg);
        let pos = ["b", "tr", "t", "r", "l"].get(usize::from(lr.u8(16))).copied().unwrap_or("r");
        let lt = t.kid(Some(lg), ct::TEXT);
        let lf = t.kid(Some(lg), ct::FRAME);
        let lsp = match lf {
            Some(lf) => format!(
                "<c:spPr>{}{}</c:spPr>",
                fill_xml(g, &t, t.kid(Some(lf), ct::AREAFORMAT), Some("#FFFFFF")),
                ln_xml(g, &t, t.kid(Some(lf), ct::LINEFORMAT), "#000000", 3175)
            ),
            None => format!("<c:spPr>{}<a:ln w=\"3175\">{}</a:ln></c:spPr>", solid("#FFFFFF"), solid("#000000")),
        };
        let lay = spc_layout(lr.i32(0), lr.i32(4), lr.i32(8), lr.i32(12), false);
        legend = format!(
            "<c:legend><c:legendPos val=\"{pos}\"/>{}<c:overlay val=\"0\"/>{lsp}{}</c:legend>",
            if lay == "<c:layout/>" { "" } else { &lay },
            tx_pr(&text_node_props(g, &t, lt, &base_text), 0)
        );
    }

    // ── 3D ──
    let mut view3d = String::new();
    if let Some(d3) = groups.iter().find_map(|x| t.kid(Some(x.node), ct::D3)) {
        let r = t.rd(d3);
        let persp = r.u16(12) & 1 != 0;
        let depth = if r.u16(8) != 0 { r.u16(8) } else { 100 };
        view3d = format!(
            "<c:view3D><c:rotX val=\"{}\"/><c:rotY val=\"{}\"/><c:depthPercent val=\"{}\"/><c:rAngAx val=\"{}\"/>{}</c:view3D>",
            i32::from(r.i16(2)).clamp(-90, 90),
            r.u16(0) % 360,
            i32::from(depth).clamp(20, 2000),
            if persp { 0 } else { 1 },
            if persp { format!("<c:perspective val=\"{}\"/>", (i32::from(r.i16(4)) * 2).clamp(0, 240)) } else { String::new() }
        );
    }

    let sp = t.kid(Some(chart), ct::SHTPROPS);
    let blanks = match sp {
        Some(s) => ["gap", "zero", "span"].get(usize::from(t.rd(s).u8(2))).copied().unwrap_or("gap"),
        None => "gap",
    };
    let plot_vis_only = sp.is_none_or(|s| t.rd(s).u16(0) & 2 != 0);
    let auto_deleted = title.is_empty();
    Some(format!(
        "{XML_HEAD}<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <c:roundedCorners val=\"0\"/><c:chart>{title}<c:autoTitleDeleted val=\"{}\"/>{view3d}<c:plotArea>{plot_layout}{plot}{}{plot_sp}</c:plotArea>{legend}<c:plotVisOnly val=\"{}\"/><c:dispBlanksAs val=\"{blanks}\"/></c:chart>{chart_sp}{}</c:chartSpace>",
        i32::from(auto_deleted),
        axes_xml.join(""),
        i32::from(plot_vis_only),
        tx_pr(&base_text, 0)
    ))
}

/// A rectangle in SPRC (1/4000 of the chart area) → a manual `c:layout`; out of the plausible range (the automatic position
/// is written as 0) = automatic layout.
fn spc_layout(x: i32, y: i32, w: i32, h: i32, inner: bool) -> String {
    if !(w > 40 && h > 40 && x >= 0 && y >= 0 && i64::from(x) + i64::from(w) <= 4400 && i64::from(y) + i64::from(h) <= 4400) {
        return "<c:layout/>".to_owned();
    }
    let f = |v: i32| -> String { super::styles_xml::fixed_num(f64::from(v) / 4000.0, 5) };
    format!(
        "<c:layout><c:manualLayout>{}<c:xMode val=\"edge\"/><c:yMode val=\"edge\"/><c:x val=\"{}\"/><c:y val=\"{}\"/><c:w val=\"{}\"/><c:h val=\"{}\"/></c:manualLayout></c:layout>",
        if inner { "<c:layoutTarget val=\"inner\"/>" } else { "" },
        f(x),
        f(y),
        f(w),
        f(h)
    )
}

/// The formats of the series (DATAFORMAT xi = 0xFFFF) and of the points.
fn series_xml(g: &XlsGlobals, t: &Tree, s: &SeriesData, kind: Kind, varied: bool, order: usize, idx: usize) -> String {
    let dfs = t.kids_of(Some(s.node), ct::DATAFORMAT);
    let own = dfs.iter().copied().find(|&d| t.rd(d).u16(0) == 0xffff);
    // the index of the automatic formatting: the iss of the series' DATAFORMAT (the order of the colours)
    let n = own.map_or(idx, |o| usize::from(t.rd(o).u16(4)));
    let line_auto = auto_line(g, n);
    let fill_auto = auto_fill(g, n);
    let filled = matches!(kind, Kind::Bar | Kind::Area | Kind::Pie | Kind::Bubble);
    let sp_of = |df: Option<NodeId>, pt_index: Option<usize>| -> String {
        let f_auto = match pt_index {
            Some(p) => auto_fill(g, p),
            None => fill_auto.clone(),
        };
        if filled {
            return format!(
                "<c:spPr>{}{}</c:spPr>",
                fill_xml(g, t, t.kid(df, ct::AREAFORMAT), Some(&f_auto)),
                ln_xml(g, t, t.kid(df, ct::LINEFORMAT), "#000000", 3175)
            );
        }
        format!(
            "<c:spPr>{}</c:spPr>",
            ln_xml(g, t, t.kid(df, ct::LINEFORMAT), &line_auto, if kind == Kind::Scatter { 9525 } else { 19050 })
        )
    };
    let marker_of = |df: Option<NodeId>| -> String {
        if !matches!(kind, Kind::Line | Kind::Scatter | Kind::Radar) {
            return String::new();
        }
        let mf = t.kid(df, ct::MARKERFORMAT);
        let auto = mf.is_none_or(|m| t.rd(m).u16(10) & 1 != 0);
        if auto {
            let sym = AUTO_MARKERS[n % AUTO_MARKERS.len()];
            return format!(
                "<c:marker><c:symbol val=\"{sym}\"/><c:size val=\"5\"/><c:spPr>{}<a:ln w=\"9525\">{}</a:ln></c:spPr></c:marker>",
                solid(&line_auto),
                solid(&line_auto)
            );
        }
        let r = t.rd(mf.unwrap_or(0));
        let (imk, grbit) = (r.u16(8), r.u16(10));
        let sym = MARKERS.get(usize::from(imk)).copied().unwrap_or("none");
        if sym == "none" {
            return "<c:marker><c:symbol val=\"none\"/></c:marker>".to_owned();
        }
        let size = max(2.0, min(72.0, round(f64::from(r.u32(16)) / 20.0)));
        let fill = if grbit & 0x10 != 0 { "<a:noFill/>".to_owned() } else { solid(&icv_color(g, r.u16(14))) };
        let line = if grbit & 0x20 != 0 {
            "<a:ln><a:noFill/></a:ln>".to_owned()
        } else {
            format!("<a:ln w=\"9525\">{}</a:ln>", solid(&icv_color(g, r.u16(12))))
        };
        format!("<c:marker><c:symbol val=\"{sym}\"/><c:size val=\"{}\"/><c:spPr>{fill}{line}</c:spPr></c:marker>", to_js_string(size))
    };
    let mut x = format!("<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{order}\"/>");
    if let Some(name) = &s.name {
        match &s.name_ref {
            Some(r) => x.push_str(&format!(
                "<c:tx><c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>",
                xml_text_s(r),
                xml_text_s(name)
            )),
            None => x.push_str(&format!("<c:tx><c:v>{}</c:v></c:tx>", xml_text_s(name))),
        }
    }
    x.push_str(&sp_of(own, None));
    if matches!(kind, Kind::Line | Kind::Scatter | Kind::Radar) {
        x.push_str(&marker_of(own));
    }
    if kind == Kind::Bar || kind == Kind::Bubble {
        x.push_str("<c:invertIfNegative val=\"0\"/>");
    }
    // points: own formatting (DATAFORMAT xi) and varied colours (pie)
    let pt_count = s.vals.len();
    let mut pts: HashMap<u16, NodeId> = HashMap::new();
    for &d in &dfs {
        let k = t.rd(d).u16(0);
        if k != 0xffff {
            pts.insert(k, d);
        }
    }
    let vary_pts = (kind == Kind::Pie || varied) && filled;
    let mut p = 0usize;
    while p < pt_count && (vary_pts || !pts.is_empty()) {
        let df = u16::try_from(p).ok().and_then(|k| pts.get(&k).copied());
        if df.is_none() && !vary_pts {
            p += 1;
            continue;
        }
        if !filled {
            // line/scatter/radar: the point's own marker and line
            x.push_str(&format!(
                "<c:dPt><c:idx val=\"{p}\"/>{}<c:spPr>{}</c:spPr></c:dPt>",
                marker_of(df),
                ln_xml(
                    g,
                    t,
                    t.kid(df, ct::LINEFORMAT).or_else(|| t.kid(own, ct::LINEFORMAT)),
                    &line_auto,
                    if kind == Kind::Scatter { 9525 } else { 19050 }
                )
            ));
            p += 1;
            continue;
        }
        let own_area = t.kid(df, ct::AREAFORMAT).or_else(|| t.kid(own, ct::AREAFORMAT));
        let area_auto = own_area.is_none_or(|a| t.rd(a).u16(10) & 1 != 0);
        let fill_at = if vary_pts && area_auto { auto_fill(g, p) } else { fill_auto.clone() };
        let pie = t.kid(df, ct::PIEFORMAT).map(|k| t.rd(k).u16(0)).filter(|&v| v != 0);
        x.push_str(&format!(
            "<c:dPt><c:idx val=\"{p}\"/>{}{}<c:spPr>{}{}</c:spPr></c:dPt>",
            if kind == Kind::Bar { "<c:invertIfNegative val=\"0\"/>" } else { "" },
            pie.map_or_else(String::new, |v| format!("<c:explosion val=\"{v}\"/>")),
            fill_xml(g, t, t.kid(df, ct::AREAFORMAT).or_else(|| t.kid(own, ct::AREAFORMAT)), Some(&fill_at)),
            ln_xml(g, t, t.kid(df, ct::LINEFORMAT).or_else(|| t.kid(own, ct::LINEFORMAT)), "#000000", 3175)
        ));
        p += 1;
    }
    // data labels (ATTACHEDLABEL of the series)
    if let Some(al) = t.kid(own, ct::ATTACHEDLABEL) {
        let f = t.rd(al).u16(0);
        let b = |bit: u16| -> u32 { u32::from(f & bit != 0) };
        if f & 0x77 != 0 {
            // pie: labels outside, with leader lines and a line break between the name and the % (as Excel shows .xls)
            let pie = kind == Kind::Pie;
            x.push_str(&format!(
                "<c:dLbls>{}<c:showLegendKey val=\"0\"/><c:showVal val=\"{}\"/><c:showCatName val=\"{}\"/><c:showSerName val=\"{}\"/><c:showPercent val=\"{}\"/><c:showBubbleSize val=\"{}\"/>{}</c:dLbls>",
                if pie { "<c:dLblPos val=\"bestFit\"/>" } else { "" },
                b(1),
                b(0x10) | b(4),
                b(0x40),
                b(2) | b(4),
                b(0x20),
                if pie { "<c:separator>\n</c:separator><c:showLeaderLines val=\"1\"/>" } else { "" }
            ));
        }
    }
    let xy = kind == Kind::Scatter || kind == Kind::Bubble;
    if let Some(cats) = &s.cats {
        let tag = if xy { "c:xVal" } else { "c:cat" };
        let cat_ref = xml_text_s(s.cat_ref.as_deref().unwrap_or(""));
        if s.cats_numeric {
            let nums: Vec<Option<f64>> = cats.iter().map(|v| if let Some(CV::Num(n)) = v { Some(*n) } else { None }).collect();
            x.push_str(&format!(
                "<{tag}><c:numRef><c:f>{cat_ref}</c:f>{}</c:numRef></{tag}>",
                num_xml(&nums, s.cat_fmt.as_deref().unwrap_or("General"))
            ));
        } else {
            x.push_str(&format!("<{tag}><c:strRef><c:f>{cat_ref}</c:f>{}</c:strRef></{tag}>", str_xml(cats)));
        }
    }
    let vtag = if xy { "c:yVal" } else { "c:val" };
    x.push_str(&format!(
        "<{vtag}><c:numRef><c:f>{}</c:f>{}</c:numRef></{vtag}>",
        xml_text_s(s.val_ref.as_deref().unwrap_or("")),
        num_xml(&s.vals, &s.val_fmt)
    ));
    if kind == Kind::Bubble {
        let bubbles: Vec<Option<f64>> = s.bubbles.clone().unwrap_or_else(|| s.vals.iter().map(|_| Some(1.0)).collect());
        x.push_str(&format!("<c:bubbleSize><c:numRef><c:f/>{}</c:numRef></c:bubbleSize>", num_xml(&bubbles, "General")));
    }
    if matches!(kind, Kind::Line | Kind::Scatter) {
        let smooth = t.kid(own, ct::SERFMT).is_some_and(|k| t.rd(k).u16(0) & 1 != 0);
        x.push_str(&format!("<c:smooth val=\"{}\"/>", i32::from(smooth)));
    }
    x.push_str("</c:ser>");
    x
}

const TICK_MARK: [&str; 4] = ["none", "in", "out", "cross"];
const TICK_LBL: [&str; 4] = ["none", "low", "high", "nextTo"];

/// A date axis already resolved: the base unit and the major/minor units.
struct DateAxis {
    base: u16,
    major: Option<u16>,
    major_unit: u16,
    minor: Option<u16>,
    minor_unit: u16,
}

const TIME_UNIT: [&str; 3] = ["days", "months", "years"];

/// Days since 1970-01-01 of a proleptic Gregorian date (the month may be out of 0..12: it carries into the year).
fn days_from_civil(y: i64, m0: i64, d: i64) -> i64 {
    let y = y + m0.div_euclid(12);
    let m = m0.rem_euclid(12) + 1;
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + (d - 1)
}

/// The civil date (year, month 0-based, day) of days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m - 1, d)
}

/// `Date.UTC(y, m, d)` in days (a year 0..99 is 1900 + y).
fn date_utc_days(y: i64, m: i64, d: i64) -> i64 {
    let y = if (0..=99).contains(&y) { y + 1900 } else { y };
    days_from_civil(y, m, d)
}

/// The largest |days| a JS `Date` holds (8.64e15 ms).
const MAX_DATE_DAYS: f64 = 100_000_000.0;

/// `serialYmd`: serial → (year, month 0-based, day) (1900 system with the fictitious 29/02/1900, or 1904); `None` = Invalid Date.
fn serial_ymd(v: f64, d1904: bool) -> Option<(i64, i64, i64)> {
    let mut n = floor(v);
    if !d1904 && n < 61.0 {
        n += 1.0;
    }
    let base = if d1904 { date_utc_days(1904, 0, 1) } else { date_utc_days(1899, 11, 30) };
    let total = n + base as f64;
    if !total.is_finite() || total.abs() > MAX_DATE_DAYS {
        return None;
    }
    Some(civil_from_days(uo_jsnum::conv::to_int32(total) as i64))
}

fn ymd_serial(y: f64, m: f64, d: f64, d1904: bool) -> f64 {
    let (yi, mi) = (uo_jsnum::conv::to_int32(y) as i64, uo_jsnum::conv::to_int32(m) as i64);
    let base = if d1904 { date_utc_days(1904, 0, 1) } else { date_utc_days(1899, 11, 30) };
    let mut n = (date_utc_days(yi, mi, uo_jsnum::conv::to_int32(d) as i64) - base) as f64;
    if !d1904 && n < 61.0 {
        n -= 1.0;
    }
    n
}

/// `fmt.replace(/"[^"]*"|\[[^\]]*\]|\\./g, '')`.
fn strip_format_literals(fmt: &str) -> String {
    let c: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' => {
                if let Some(j) = c[i + 1..].iter().position(|&x| x == '"') {
                    i += j + 2;
                    continue;
                }
            }
            '[' => {
                if let Some(j) = c[i + 1..].iter().position(|&x| x == ']') {
                    i += j + 2;
                    continue;
                }
            }
            '\\' if i + 1 < c.len() && c[i + 1] != '\n' && c[i + 1] != '\r' => {
                i += 2;
                continue;
            }
            _ => {}
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// A category axis with dates (AXCEXT.fDateAxis, [MS-XLS] 2.4.9): Excel puts ONE position per base unit (days/months/years)
/// between the minimum and the maximum — empty categories vanish and repeated dates fall at the same position. The chart
/// engine places by index: the series are redistributed here at the date positions (and the axis is written as `c:dateAx`).
fn date_axis(g: &XlsGlobals, t: &Tree, ext: NodeId, series: &mut [SeriesData], sers: &[usize]) -> Option<DateAxis> {
    let r = t.rd(ext);
    let fl = r.u16(16);
    if fl & 0x10 == 0 {
        return None;
    }
    let ref_i = sers.iter().copied().find(|&i| {
        let s = &series[i];
        s.cats_numeric && s.cats.as_ref().is_some_and(|c| c.iter().any(|v| matches!(v, Some(CV::Num(_)))))
    })?;
    let ref_cats = series[ref_i].cats.clone().unwrap_or_default(); // for the keys below (before any change)
    let ref_fmt = series[ref_i].cat_fmt.clone();
    // automatic type (fAutoDate): dates only if the categories are formatted as a date
    let fmt = strip_format_literals(ref_fmt.as_deref().unwrap_or("General"));
    let has_dmy = fmt.chars().any(|c| matches!(c, 'd' | 'D' | 'm' | 'M' | 'y' | 'Y'));
    let is_general = uo_jsnum::text::trim(&fmt).eq_ignore_ascii_case("general");
    if fl & 0x80 != 0 && !(has_dmy && !is_general) {
        return None;
    }
    let base = r.u16(12).min(2);
    let key = |v: f64| -> f64 {
        if base == 0 {
            return floor(v);
        }
        match serial_ymd(v, g.date1904) {
            None => f64::NAN,
            Some((y, m, _)) => {
                if base == 1 {
                    (y * 12 + m) as f64
                } else {
                    y as f64
                }
            }
        }
    };
    let keys: Vec<f64> =
        ref_cats.iter().filter_map(|v| if let Some(CV::Num(n)) = v { Some(*n) } else { None }).filter(|n| n.is_finite()).map(key).collect();
    let mut lo = keys.iter().fold(f64::INFINITY, |m, &k| min(m, k));
    let mut hi = keys.iter().fold(f64::NEG_INFINITY, |m, &k| max(m, k));
    if fl & 1 == 0 {
        lo = key(f64::from(r.u16(0)));
    }
    if fl & 2 == 0 {
        hi = key(f64::from(r.u16(2)));
    }
    let n = hi - lo + 1.0;
    if !(n > 0.0 && n <= 4000.0) {
        return None;
    }
    let count = usize::try_from(uo_jsnum::conv::to_int32(n)).unwrap_or(0);
    let slot_serial = |k: usize| -> f64 {
        let v = lo + k as f64;
        if base == 0 {
            v
        } else if base == 1 {
            ymd_serial(floor(v / 12.0), uo_jsnum::math::rem(v, 12.0), 1.0, g.date1904)
        } else {
            ymd_serial(v, 0.0, 1.0, g.date1904)
        }
    };
    let cats: Vec<Option<CV>> = (0..count).map(|k| Some(CV::Num(slot_serial(k)))).collect();
    for &si in sers {
        // `ref.cats` is read at each step: once the reference series was redistributed it holds the slot serials
        let own: Vec<Option<CV>> = match (&series[si].cats, series[si].cats_numeric) {
            (Some(c), true) => c.clone(),
            _ => series[ref_i].cats.clone().unwrap_or_default(),
        };
        let mut vals: Vec<Option<f64>> = vec![None; count];
        for (i, v) in series[si].vals.iter().enumerate() {
            let (Some(CV::Num(d)), Some(v)) = (own.get(i).and_then(Option::as_ref), v) else { continue };
            let k = key(*d) - lo;
            if k >= 0.0
                && k < n
                && let Ok(ki) = usize::try_from(uo_jsnum::conv::to_int32(k))
                && let Some(slot) = vals.get_mut(ki)
            {
                *slot = Some(*v);
            }
        }
        let s = &mut series[si];
        s.vals = vals;
        s.cats = Some(cats.clone());
        s.cats_numeric = true;
        if s.cat_fmt.is_none() {
            s.cat_fmt = ref_fmt.clone();
        }
    }
    Some(DateAxis {
        base,
        major: if fl & 4 != 0 { None } else { Some(r.u16(4)) },
        major_unit: r.u16(6).min(2),
        minor: if fl & 8 != 0 { None } else { Some(r.u16(8)) },
        minor_unit: r.u16(10).min(2),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AxKind {
    Cat,
    Val,
}

#[allow(clippy::too_many_arguments)]
fn axis_xml(
    g: &XlsGlobals,
    t: &Tree,
    ax: Option<NodeId>,
    kind: AxKind,
    id: u32,
    cross: u32,
    pos: &str,
    secondary: bool,
    base: &TextProps,
    title_node: Option<NodeId>,
    fmt: Option<&str>,
    between: bool,
    xy: bool,
    date: Option<&DateAxis>,
) -> String {
    let tick = t.kid(ax, ct::TICK);
    let vr = t.kid(ax, ct::VALUERANGE);
    let cr = t.kid(ax, ct::CATSERRANGE);
    let tag = if kind == AxKind::Cat { if date.is_some() { "c:dateAx" } else { "c:catAx" } } else { "c:valAx" };
    let mut reversed = false;
    let mut scaling = String::new();
    let mut crosses = if secondary { "<c:crosses val=\"max\"/>".to_owned() } else { "<c:crosses val=\"autoZero\"/>".to_owned() };
    let mut units = String::new();
    if let Some(vr) = vr {
        let r = t.rd(vr);
        let fl = r.u16(40);
        reversed = fl & 0x40 != 0;
        if fl & 0x20 != 0 {
            scaling.push_str("<c:logBase val=\"10\"/>");
        }
        scaling.push_str(&format!("<c:orientation val=\"{}\"/>", if reversed { "maxMin" } else { "minMax" }));
        if fl & 2 == 0 {
            scaling.push_str(&format!("<c:max val=\"{}\"/>", to_js_string(r.f64(8))));
        }
        if fl & 1 == 0 {
            scaling.push_str(&format!("<c:min val=\"{}\"/>", to_js_string(r.f64(0))));
        }
        if fl & 0x10 == 0 {
            crosses = format!("<c:crossesAt val=\"{}\"/>", to_js_string(r.f64(32)));
        }
        if fl & 0x80 != 0 {
            crosses = "<c:crosses val=\"max\"/>".to_owned();
        }
        if fl & 4 == 0 {
            units.push_str(&format!("<c:majorUnit val=\"{}\"/>", to_js_string(r.f64(16))));
        }
        if fl & 8 == 0 {
            units.push_str(&format!("<c:minorUnit val=\"{}\"/>", to_js_string(r.f64(24))));
        }
    } else {
        if let Some(cr) = cr {
            let r = t.rd(cr);
            reversed = r.u16(6) & 4 != 0;
            if r.u16(6) & 2 != 0 {
                crosses = "<c:crosses val=\"max\"/>".to_owned();
            }
        }
        scaling = format!("<c:orientation val=\"{}\"/>", if reversed { "maxMin" } else { "minMax" });
    }
    // lines: the axis (AXISLINEFORMAT 0) and the grids (1 = major, 2 = minor)
    let mut axis_line = format!("<a:ln w=\"9525\">{}</a:ln>", solid("#000000"));
    let mut major = String::new();
    let mut minor = String::new();
    let kids: &[NodeId] = ax.map_or(&[], |a| &t.n(a).kids);
    for (i, &k) in kids.iter().enumerate() {
        if t.n(k).t != ct::AXISLINEFORMAT {
            continue;
        }
        let lf = kids.get(i + 1).copied().filter(|&n| t.n(n).t == ct::LINEFORMAT);
        match t.rd(k).u16(0) {
            0 => axis_line = ln_xml(g, t, lf, "#000000", 9525),
            1 => major = format!("<c:majorGridlines><c:spPr>{}</c:spPr></c:majorGridlines>", ln_xml(g, t, lf, "#000000", 9525)),
            2 => minor = format!("<c:minorGridlines><c:spPr>{}</c:spPr></c:minorGridlines>", ln_xml(g, t, lf, "#000000", 9525)),
            _ => {}
        }
    }
    let ifmt = t.kid(ax, ct::IFMT);
    let num_fmt = match (ifmt, fmt) {
        (Some(i), _) => format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", xml_text_s(&ifmt_code(g, t.rd(i).u16(0)))),
        (None, Some(f)) if !f.is_empty() => format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"1\"/>", xml_text_s(f)),
        _ => String::new(),
    };
    let tr = tick.map(|k| t.rd(k));
    let major_t = TICK_MARK.get(usize::from(tr.map_or(2, |r| r.u8(0)))).copied().unwrap_or("out");
    let minor_t = TICK_MARK.get(usize::from(tr.map_or(0, |r| r.u8(1)))).copied().unwrap_or("none");
    let lbl_pos = TICK_LBL.get(usize::from(tr.map_or(3, |r| r.u8(2)))).copied().unwrap_or("nextTo");
    let fx = t.kid(ax, ct::FONTX);
    let mut tp = match fx {
        Some(f) => font_props(g, t.rd(f).u16(0)),
        None => base.clone(),
    };
    if let Some(r) = tr
        && r.u16(24) & 1 == 0
    {
        tp.color = icv_color(g, r.u16(26));
    }
    let rot = tr.map_or(0, |r| r.u16(28));
    let mut title = String::new();
    if let Some(tn) = title_node
        && t.rd(tn).u16(24) & 0x40 == 0
        && let Some(text) = series_text(t, Some(tn)).filter(|x| !x.is_empty())
    {
        let mut b = base.clone();
        b.bold = true;
        title = rich_title(&text, &text_node_props(g, t, Some(tn), &b), t.rd(tn).u16(30));
    }
    let mut x = format!(
        "<{tag}><c:axId val=\"{id}\"/><c:scaling>{scaling}</c:scaling><c:delete val=\"{}\"/><c:axPos val=\"{pos}\"/>{major}{minor}{title}{num_fmt}<c:majorTickMark val=\"{major_t}\"/><c:minorTickMark val=\"{minor_t}\"/><c:tickLblPos val=\"{lbl_pos}\"/><c:spPr><a:noFill/>{axis_line}</c:spPr>{}<c:crossAx val=\"{cross}\"/>{crosses}",
        i32::from(ax.is_none()),
        tx_pr(&tp, rot)
    );
    if kind == AxKind::Cat && date.is_some() {
        if let Some(d) = date {
            x.push_str(&format!(
                "<c:auto val=\"0\"/><c:lblOffset val=\"100\"/><c:baseTimeUnit val=\"{}\"/>",
                TIME_UNIT[usize::from(d.base.min(2))]
            ));
            if let Some(m) = d.major.filter(|&m| m != 0) {
                x.push_str(&format!(
                    "<c:majorUnit val=\"{m}\"/><c:majorTimeUnit val=\"{}\"/>",
                    TIME_UNIT[usize::from(d.major_unit.min(2))]
                ));
            }
            if let Some(m) = d.minor.filter(|&m| m != 0) {
                x.push_str(&format!(
                    "<c:minorUnit val=\"{m}\"/><c:minorTimeUnit val=\"{}\"/>",
                    TIME_UNIT[usize::from(d.minor_unit.min(2))]
                ));
            }
        }
    } else if kind == AxKind::Cat {
        x.push_str("<c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/>");
        if let Some(cr) = cr {
            let r = t.rd(cr);
            if r.u16(2) > 1 {
                x.push_str(&format!("<c:tickLblSkip val=\"{}\"/>", r.u16(2)));
            }
            if r.u16(4) > 1 {
                x.push_str(&format!("<c:tickMarkSkip val=\"{}\"/>", r.u16(4)));
            }
        }
        x.push_str("<c:noMultiLvlLbl val=\"0\"/>");
    } else {
        x.push_str(&format!(
            "<c:crossBetween val=\"{}\"/>{units}",
            if xy {
                "midCat"
            } else if between {
                "between"
            } else {
                "midCat"
            }
        ));
    }
    x.push_str(&format!("</{tag}>"));
    x
}
