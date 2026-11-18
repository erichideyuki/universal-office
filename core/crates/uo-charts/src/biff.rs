//! BIFF8 chart → `c:chartSpace` (Slides wave 5, `.ppt` charts; reusable by the Sheet's `.xls` reader).
//!
//! PowerPoint opens the chart of a `.ppt` (an embedded `Excel.Chart.8` OLE object: a `Workbook` stream whose chart sheet is a
//! BIFF8 chart substream, `BOF dt=0x20 … EOF`) as a NATIVE chart (measured: `Shape.Type` = msoChart, `HasChart`), not as the
//! fallback picture the `.ppt` keeps beside it. This module converts that substream into the chart part the engine reads.
//!
//! Port of `sheet-core/src/xls/chart.ts` (ts-ref-092; the Sheet's `.xls` charts) with the Office 2007+ extension records that
//! PowerPoint/Excel 2007+ write into the substream and that the TS does not read: `ShapePropsStream` (0x08A4, the element's
//! `a:spPr`), `TextPropsStream` (0x08A5, its `a:txPr`) and `CrtLayout12A` (0x08A7, the automatic/manual plot layout). Where an
//! element carries its XML the XML wins (PowerPoint draws the 2007 formatting, not the Excel 97 palette approximation of the
//! BIFF records). Values come from the chart's own cache (`SIINDEX` + `NUMBER`/`LABEL`), which an embedded chart always
//! carries; the cells of the workbook are not read (the Sheet port will add them through [`ChartCells`]).
//!
//! Not converted (counted by the caller as the fallback picture): BIFF5, a substream without a chart group. `GELFRAME`
//! (Office 2007 OfficeArt fills) is used only through the `ShapePropsStream` that accompanies it.

use std::collections::HashMap;
use std::fmt::Write as _;

use uo_jsnum::format::to_js_string;

const BOF: u16 = 0x0809;
const EOF: u16 = 0x000A;
const CONTINUE: u16 = 0x003C;
const CONTINUE_FRT12: u16 = 0x087F;
const FONT: u16 = 0x0031;
const FORMAT: u16 = 0x041E;
const XF: u16 = 0x00E0;
const PALETTE: u16 = 0x0092;
const NUMBER: u16 = 0x0203;
const LABEL: u16 = 0x0204;
const SHAPE_PROPS: u16 = 0x08A4;
const TEXT_PROPS: u16 = 0x08A5;
const CRT_LAYOUT12A: u16 = 0x08A7;
const CRT_LAYOUT12: u16 = 0x089D;
const BOUNDSHEET: u16 = 0x0085;
const EXTERNSHEET: u16 = 0x0017;
const SERAUXTREND: u16 = 0x104B;
const SERAUXERRBAR: u16 = 0x105B;

const CHART: u16 = 0x1002;
const SERIES: u16 = 0x1003;
const DATAFORMAT: u16 = 0x1006;
const LINEFORMAT: u16 = 0x1007;
const MARKERFORMAT: u16 = 0x1009;
const AREAFORMAT: u16 = 0x100A;
const PIEFORMAT: u16 = 0x100B;
const ATTACHEDLABEL: u16 = 0x100C;
const SERIESTEXT: u16 = 0x100D;
const CHARTFORMAT: u16 = 0x1014;
const LEGEND: u16 = 0x1015;
const BAR: u16 = 0x1017;
const LINE: u16 = 0x1018;
const PIE: u16 = 0x1019;
const AREA: u16 = 0x101A;
const SCATTER: u16 = 0x101B;
const AXIS: u16 = 0x101D;
const TICK: u16 = 0x101E;
const VALUERANGE: u16 = 0x101F;
const CATSERRANGE: u16 = 0x1020;
const AXISLINEFORMAT: u16 = 0x1021;
const DEFAULTTEXT: u16 = 0x1024;
const TEXT: u16 = 0x1025;
const FONTX: u16 = 0x1026;
const OBJECTLINK: u16 = 0x1027;
const FRAME: u16 = 0x1032;
const BEGIN: u16 = 0x1033;
const END: u16 = 0x1034;
const PLOTAREA: u16 = 0x1035;
const D3: u16 = 0x103A;
const RADAR: u16 = 0x103E;
const RADARAREA: u16 = 0x1040;
const AXISPARENT: u16 = 0x1041;
const SHTPROPS: u16 = 0x1044;
const SERTOCRT: u16 = 0x1045;
const SERPARENT: u16 = 0x104A;
const IFMT: u16 = 0x104E;
const AI: u16 = 0x1051;
const SERFMT: u16 = 0x105D;
const BOPPOP: u16 = 0x1061;
const SIINDEX: u16 = 0x1065;
const GELFRAME: u16 = 0x1066;

/// Most records a substream may hold before the conversion gives up (a real chart has a few thousand).
const MAX_RECORDS: usize = 200_000;
/// Deepest BEGIN/END nesting followed.
const MAX_DEPTH: usize = 64;
/// Most points per series (Excel 97–2003: 32 000).
const MAX_POINTS: usize = 32_000;

/// The default indexed palette of Excel (indices 0–63, `INDEXED_COLORS_DEFAULT` of the TS).
const PALETTE_DEFAULT: [&str; 64] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "000000", "FFFFFF", "FF0000", "00FF00", "0000FF",
    "FFFF00", "FF00FF", "00FFFF", "800000", "008000", "000080", "808000", "800080", "008080", "C0C0C0", "808080", "9999FF", "993366",
    "FFFFCC", "CCFFFF", "660066", "FF8080", "0066CC", "CCCCFF", "000080", "FF00FF", "FFFF00", "00FFFF", "800080", "800000", "008080",
    "0000FF", "00CCFF", "CCFFFF", "CCFFCC", "FFFF99", "99CCFF", "FF99CC", "CC99FF", "FFCC99", "3366FF", "33CCCC", "99CC00", "FFCC00",
    "FF9900", "FF6600", "666699", "969696", "003366", "339966", "003300", "333300", "993300", "993366", "333399", "333333",
];

/// One BIFF record with its CONTINUE data appended (`conts`: the offsets where each CONTINUE began).
#[derive(Clone, Debug)]
struct Rec {
    t: u16,
    data: Vec<u8>,
    conts: Vec<usize>,
}

fn records(stream: &[u8]) -> Vec<Rec> {
    let mut out: Vec<Rec> = Vec::new();
    let mut off = 0usize;
    while off + 4 <= stream.len() && out.len() < MAX_RECORDS {
        let t = u16::from_le_bytes([stream[off], stream[off + 1]]);
        let len = usize::from(u16::from_le_bytes([stream[off + 2], stream[off + 3]]));
        let end = (off + 4 + len).min(stream.len());
        let data = &stream[off + 4..end];
        match (t, out.last_mut()) {
            (CONTINUE, Some(last)) => {
                last.conts.push(last.data.len());
                last.data.extend_from_slice(data);
            }
            // the XML of a ShapePropsStream/TextPropsStream goes on in ContinueFrt12 records (after their 12-byte header)
            (CONTINUE_FRT12, Some(last)) if matches!(last.t, SHAPE_PROPS | TEXT_PROPS) => {
                last.data.extend_from_slice(data.get(12..).unwrap_or(&[]));
            }
            _ => out.push(Rec { t, data: data.to_vec(), conts: Vec::new() }),
        }
        off = end;
    }
    out
}

/// Bounded little-endian reads (0 past the end, as the TS `Bytes`).
trait Le {
    fn u8_at(&self, o: usize) -> u8;
    fn u16_at(&self, o: usize) -> u16;
    fn i16_at(&self, o: usize) -> i16;
    fn u32_at(&self, o: usize) -> u32;
    fn i32_at(&self, o: usize) -> i32;
    fn f64_at(&self, o: usize) -> f64;
}

impl Le for [u8] {
    fn u8_at(&self, o: usize) -> u8 {
        self.get(o).copied().unwrap_or(0)
    }
    fn u16_at(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.u8_at(o), self.u8_at(o + 1)])
    }
    fn i16_at(&self, o: usize) -> i16 {
        i16::from_le_bytes([self.u8_at(o), self.u8_at(o + 1)])
    }
    fn u32_at(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.u8_at(o), self.u8_at(o + 1), self.u8_at(o + 2), self.u8_at(o + 3)])
    }
    fn i32_at(&self, o: usize) -> i32 {
        i32::from_le_bytes([self.u8_at(o), self.u8_at(o + 1), self.u8_at(o + 2), self.u8_at(o + 3)])
    }
    fn f64_at(&self, o: usize) -> f64 {
        let mut b = [0u8; 8];
        for (k, x) in b.iter_mut().enumerate() {
            *x = self.u8_at(o + k);
        }
        f64::from_le_bytes(b)
    }
}

/// `readChars`: `cch` characters at `off`, 8-bit or UTF-16, crossing CONTINUE boundaries (each starts with an options byte).
fn read_chars(d: &[u8], off: usize, cch: usize, mut high: bool, conts: &[usize]) -> (String, usize) {
    let mut units: Vec<u16> = Vec::new();
    let mut left = cch;
    let mut p = off;
    let mut ci = conts.iter().position(|&c| c >= p).unwrap_or(conts.len());
    while left > 0 && p < d.len() {
        let bound = conts.get(ci).copied().unwrap_or(d.len());
        let avail = if high { bound.saturating_sub(p) >> 1 } else { bound.saturating_sub(p) };
        let n = left.min(avail);
        for k in 0..n {
            units.push(if high { d.u16_at(p + 2 * k) } else { u16::from(d.u8_at(p + k)) });
        }
        p += if high { n * 2 } else { n };
        left -= n;
        if left > 0 {
            if p >= d.len() {
                break;
            }
            if p >= bound && ci < conts.len() {
                high = d.u8_at(p) & 1 != 0;
                p += 1;
                ci += 1;
            } else if n == 0 {
                p += 1;
            }
        }
    }
    (String::from_utf16_lossy(&units), p)
}

/// `readXLString` (BIFF8): the count (1 or 2 bytes), the options byte, the characters.
fn read_xl_string(d: &[u8], off: usize, cch_size: usize, conts: &[usize]) -> String {
    let cch = if cch_size == 1 { usize::from(d.u8_at(off)) } else { usize::from(d.u16_at(off)) };
    let p = off + cch_size;
    read_chars(d, p + 1, cch, d.u8_at(p) & 1 != 0, conts).0
}

/// What the chart needs of the workbook's globals.
#[derive(Clone, Debug)]
struct Globals {
    palette: Vec<String>,
    fonts: Vec<Font>,
    formats: HashMap<u16, String>,
    /// the number format of each XF
    xf_fmt: Vec<u16>,
    /// the sheet names (BOUNDSHEET order) and the `itabFirst` of each EXTERNSHEET entry (the references of the series)
    sheets: Vec<String>,
    xti: Vec<i16>,
}

#[derive(Clone, Debug)]
struct Font {
    height: u16,
    italic: bool,
    underline: u8,
    weight: u16,
    color: u16,
    name: String,
}

fn read_globals(recs: &[Rec]) -> Globals {
    let mut g = Globals {
        palette: PALETTE_DEFAULT.iter().map(|s| (*s).to_owned()).collect(),
        fonts: Vec::new(),
        formats: HashMap::new(),
        xf_fmt: Vec::new(),
        sheets: Vec::new(),
        xti: Vec::new(),
    };
    for r in recs.iter().skip(1) {
        let d = r.data.as_slice();
        match r.t {
            EOF | BOF => break,
            FONT => g.fonts.push(Font {
                height: d.u16_at(0),
                italic: d.u16_at(2) & 2 != 0,
                color: d.u16_at(4),
                weight: d.u16_at(6),
                underline: d.u8_at(10),
                name: read_xl_string(d, 14, 1, &[]),
            }),
            FORMAT => {
                g.formats.insert(d.u16_at(0), read_xl_string(d, 2, 2, &r.conts));
            }
            XF => g.xf_fmt.push(d.u16_at(2)),
            BOUNDSHEET => g.sheets.push(read_xl_string(d, 6, 1, &r.conts)),
            EXTERNSHEET => {
                let n = usize::from(d.u16_at(0));
                for k in 0..n.min(d.len() / 6) {
                    g.xti.push(d.i16_at(2 + k * 6 + 2));
                }
            }
            PALETTE => {
                let n = usize::from(d.u16_at(0)).min(56);
                for i in 0..n {
                    let o = 2 + i * 4;
                    if o + 3 > d.len() {
                        break;
                    }
                    if let Some(p) = g.palette.get_mut(8 + i) {
                        *p = format!("{:02X}{:02X}{:02X}", d.u8_at(o), d.u8_at(o + 1), d.u8_at(o + 2));
                    }
                }
            }
            _ => {}
        }
    }
    g
}

/// Cell values of the workbook for the series references (the Sheet port); `None` = use the chart's cache.
pub trait ChartCells {
    /// The value of a cell of sheet `sheet` (BOUNDSHEET order), 0-based row/column.
    fn number(&self, sheet: u32, row: u32, col: u32) -> Option<f64>;
}

// ───────────────────────────── tree ─────────────────────────────

#[derive(Clone, Debug)]
struct Node {
    t: u16,
    d: Vec<u8>,
    conts: Vec<usize>,
    kids: Vec<Node>,
}

impl Node {
    fn kid(&self, t: u16) -> Option<&Node> {
        self.kids.iter().find(|k| k.t == t)
    }
    fn kids_of(&self, t: u16) -> impl Iterator<Item = &Node> {
        self.kids.iter().filter(move |k| k.t == t)
    }
    /// The XML of the element's `ShapePropsStream` with context `ctx`, as a `c:spPr`.
    fn shape_props(&self, ctx: u16) -> Option<String> {
        self.kids_of(SHAPE_PROPS).find(|k| k.d.u16_at(12) == ctx).and_then(|k| fragment(&k.d, 24, "spPr"))
    }
    /// The XML of the element's `TextPropsStream`, as a `c:txPr`.
    fn text_props(&self) -> Option<String> {
        self.kid(TEXT_PROPS).and_then(|k| fragment(&k.d, 20, "txPr"))
    }
}

/// The records after the BOF as a tree by BEGIN/END (the record before a BEGIN is the parent).
fn tree(recs: &[Rec]) -> Vec<Node> {
    let mut stack: Vec<Vec<Node>> = vec![Vec::new()];
    for (i, r) in recs.iter().enumerate() {
        if i == 0 && r.t == BOF {
            continue;
        }
        match r.t {
            EOF => break,
            BEGIN => {
                if stack.len() > MAX_DEPTH {
                    break;
                }
                stack.push(Vec::new());
            }
            END => {
                if stack.len() > 1 {
                    let kids = stack.pop().unwrap_or_default();
                    if let Some(parent) = stack.last_mut().and_then(|l| l.last_mut()) {
                        parent.kids = kids;
                    }
                }
            }
            _ => {
                if let Some(level) = stack.last_mut() {
                    let node = Node { t: r.t, d: r.data.clone(), conts: r.conts.clone(), kids: Vec::new() };
                    // GELFRAME (the Office 2007 fill as OfficeArt properties) completes the AREAFORMAT before it (the TS `gel`)
                    if r.t == GELFRAME
                        && let Some(area) = level.iter_mut().rev().find(|k| k.t == AREAFORMAT)
                    {
                        area.kids.push(node.clone());
                    }
                    level.push(node);
                }
            }
        }
    }
    while stack.len() > 1 {
        let kids = stack.pop().unwrap_or_default();
        if let Some(parent) = stack.last_mut().and_then(|l| l.last_mut()) {
            parent.kids = kids;
        }
    }
    stack.pop().unwrap_or_default()
}

/// An `a:spPr`/`a:txPr` fragment of an extension record (`cb` at `at - 4`, the UTF-8 XML at `at`) renamed into the chart
/// namespace (`c:spPr`/`c:txPr`); `None` when it is not that element.
fn fragment(d: &[u8], at: usize, local: &str) -> Option<String> {
    let cb = usize::try_from(d.u32_at(at - 4)).ok()?;
    let bytes = d.get(at..at.checked_add(cb)?.min(d.len()))?;
    let s = std::str::from_utf8(bytes).ok()?;
    let s = match s.find("?>") {
        Some(k) if uo_jsnum::text::trim_start(s).starts_with("<?xml") => &s[k + 2..],
        _ => s,
    };
    let s = uo_jsnum::text::trim(s);
    let open = format!("<a:{local}");
    let rest = s.strip_prefix(&open)?;
    if !rest.starts_with([' ', '>', '/']) {
        return None;
    }
    let close = format!("</a:{local}>");
    let body = match rest.strip_suffix(&close) {
        Some(b) => format!("<c:{local}{b}</c:{local}>"),
        None if rest.ends_with("/>") => format!("<c:{local}{rest}"),
        None => return None,
    };
    Some(body)
}

// ───────────────────────────── colours, lines, fills ─────────────────────────────

fn icv_color(g: &Globals, icv: u16) -> String {
    if icv < 64 {
        return g.palette.get(usize::from(icv)).cloned().unwrap_or_else(|| "000000".into());
    }
    match icv {
        0x41 | 0x4E => "FFFFFF".into(),
        0x50 => "FFFFE1".into(),
        _ => "000000".into(),
    }
}

fn auto_fill(g: &Globals, n: usize) -> String {
    g.palette.get(24 + n % 8).cloned().unwrap_or_else(|| "9999FF".into())
}

fn auto_line(g: &Globals, n: usize) -> String {
    g.palette.get(32 + n % 8).cloned().unwrap_or_else(|| "000080".into())
}

const AUTO_MARKERS: [&str; 9] = ["diamond", "square", "triangle", "x", "star", "circle", "plus", "dash", "dot"];
const MARKERS: [&str; 10] = ["none", "square", "diamond", "triangle", "x", "star", "dash", "dash", "circle", "plus"];
const DASH: [&str; 5] = ["", "dash", "sysDot", "dashDot", "lgDashDotDot"];
const TICK_MARK: [&str; 4] = ["none", "in", "out", "cross"];
const TICK_LBL: [&str; 4] = ["none", "low", "high", "nextTo"];

fn line_w(we: i16) -> u32 {
    match we {
        w if w < 0 => 3175,
        0 => 9525,
        1 => 19050,
        _ => 28575,
    }
}

fn solid(c: &str) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill>")
}

fn ln_xml(g: &Globals, n: Option<&Node>, auto: &str, auto_w: u32) -> String {
    let Some(n) = n else { return format!("<a:ln w=\"{auto_w}\">{}</a:ln>", solid(auto)) };
    let d = n.d.as_slice();
    let (lns, we, grbit, icv) = (d.u16_at(4), d.i16_at(6), d.u16_at(8), d.u16_at(10));
    if lns == 5 {
        return "<a:ln><a:noFill/></a:ln>".into();
    }
    let is_auto = grbit & 1 != 0;
    let color = if is_auto { auto.to_owned() } else { icv_color(g, icv) };
    let w = if is_auto { auto_w } else { line_w(we) };
    let dash = DASH.get(usize::from(lns)).filter(|d| !d.is_empty()).map(|d| format!("<a:prstDash val=\"{d}\"/>")).unwrap_or_default();
    format!("<a:ln w=\"{w}\">{}{dash}</a:ln>", solid(&color))
}

fn fill_xml(g: &Globals, n: Option<&Node>, auto: Option<&str>) -> String {
    let no_auto = || auto.map_or_else(|| "<a:noFill/>".to_owned(), solid);
    let Some(n) = n else { return no_auto() };
    let d = n.d.as_slice();
    let (fls, grbit, icv_fore) = (d.u16_at(8), d.u16_at(10), d.u16_at(12));
    if grbit & 1 != 0 {
        return no_auto();
    }
    if fls == 0 {
        return "<a:noFill/>".into();
    }
    // Office 2007+: the GELFRAME holds the real fill (RGB, gradients); the AREAFORMAT only the palette approximation
    if let Some(x) = n.kid(GELFRAME).and_then(gel_fill) {
        return x;
    }
    solid(&icv_color(g, icv_fore))
}

/// An OfficeArt COLORREF as `RRGGBB`; `None` for a scheme, palette or system index (the caller keeps the palette colour).
fn colorref(v: u32) -> Option<String> {
    let [r, gr, b, flags] = v.to_le_bytes();
    (flags & !0x02 == 0).then(|| format!("{r:02X}{gr:02X}{b:02X}"))
}

/// The fill of a GELFRAME (an OfficeArtFOPT, then its complex data): solid (`fillType` 0) or a linear shade (7, from
/// `fillColor` to `fillBackColor` at `fillAngle`, `fillFocus` 100 reversed, 50 mirrored); other types take `fillColor`.
/// `None` when the colour is not an RGB.
fn gel_fill(n: &Node) -> Option<String> {
    let d = n.d.as_slice();
    let count = usize::from(d.u16_at(0) >> 4);
    let mut props: HashMap<u16, u32> = HashMap::new();
    for k in 0..count.min(512) {
        let o = 8 + k * 6;
        if o + 6 > d.len() {
            break;
        }
        props.insert(d.u16_at(o) & 0x3FFF, d.u32_at(o + 2));
    }
    let filled = props.get(&0x1BF).is_none_or(|v| v & 0x0010_0000 == 0 || v & 0x10 != 0);
    if !filled {
        return Some("<a:noFill/>".into());
    }
    let fore = colorref(props.get(&0x181).copied().unwrap_or(0x00FF_FFFF))?;
    let alpha = props.get(&0x182).map_or(1.0, |v| f64::from(*v) / 65536.0);
    let clr = |c: &str, a: f64| {
        if a < 1.0 {
            format!("<a:srgbClr val=\"{c}\"><a:alpha val=\"{}\"/></a:srgbClr>", to_js_string(uo_jsnum::math::round(a * 100_000.0)))
        } else {
            format!("<a:srgbClr val=\"{c}\"/>")
        }
    };
    if props.get(&0x180).copied().unwrap_or(0) == 7 {
        let back = props.get(&0x183).copied().and_then(colorref).unwrap_or_else(|| "FFFFFF".into());
        let back_a = props.get(&0x184).map_or(1.0, |v| f64::from(*v) / 65536.0);
        let ang = f64::from(i32::from_le_bytes(props.get(&0x18B).copied().unwrap_or(0).to_le_bytes())) / 65536.0;
        let focus = i32::from_le_bytes(props.get(&0x18C).copied().unwrap_or(0).to_le_bytes());
        let (a, b) = (clr(&fore, alpha), clr(&back, back_a));
        let stops = match focus {
            100 | -100 => format!("<a:gs pos=\"0\">{b}</a:gs><a:gs pos=\"100000\">{a}</a:gs>"),
            50 | -50 => format!("<a:gs pos=\"0\">{a}</a:gs><a:gs pos=\"50000\">{b}</a:gs><a:gs pos=\"100000\">{a}</a:gs>"),
            _ => format!("<a:gs pos=\"0\">{a}</a:gs><a:gs pos=\"100000\">{b}</a:gs>"),
        };
        // the DrawingML angle is 270° − the GELFRAME's (measured on .xls charts, memory xls-biff-drawing-cf-chart-facts)
        let lin = uo_jsnum::math::round(((270.0 - ang) % 360.0 + 360.0) % 360.0 * 60000.0);
        return Some(format!(
            "<a:gradFill rotWithShape=\"1\"><a:gsLst>{stops}</a:gsLst><a:lin ang=\"{}\" scaled=\"0\"/></a:gradFill>",
            to_js_string(lin)
        ));
    }
    Some(format!("<a:solidFill>{}</a:solidFill>", clr(&fore, alpha)))
}

/// `c:spPr` of an element: its 2007 XML when present, else from the BIFF formats.
fn sp_pr(node: Option<&Node>, ctx: u16, fallback: impl FnOnce() -> String) -> String {
    node.and_then(|n| n.shape_props(ctx)).unwrap_or_else(|| format!("<c:spPr>{}</c:spPr>", fallback()))
}

fn ifmt_code(g: &Globals, id: u16) -> String {
    let own = g.formats.get(&id);
    if let Some(o) = own
        && (id >= 50 || (5..=8).contains(&id) || (41..=44).contains(&id))
    {
        return normalize_format(o);
    }
    match uo_numfmt::builtin_format_code(u32::from(id), uo_numfmt::pt_br()) {
        Some(c) => c.to_owned(),
        None => own.map_or_else(|| "General".to_owned(), |o| normalize_format(o)),
    }
}

/// `normalizeFormat`: "general" in any case outside quotes/brackets becomes `General`.
fn normalize_format(code: &str) -> String {
    if !code.to_ascii_lowercase().contains("general") {
        return code.to_owned();
    }
    let ch: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c == '"' || c == '[' {
            let close = if c == '"' { '"' } else { ']' };
            let e = ch.iter().skip(i + 1).position(|&x| x == close).map_or(ch.len(), |j| i + 1 + j + 1);
            out.extend(&ch[i..e]);
            i = e;
            continue;
        }
        if c == '\\' || c == '_' || c == '*' {
            out.extend(ch.iter().skip(i).take(2));
            i += 2;
            continue;
        }
        let w: String = ch.iter().skip(i).take(7).collect();
        if w.eq_ignore_ascii_case("general") {
            out.push_str("General");
            i += 7;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Column letters (0 → A).
fn col_letters(c: u32) -> String {
    let mut n = c + 1;
    let mut out = Vec::new();
    while n > 0 {
        let m = (n - 1) % 26;
        out.push(char::from(b'A' + u8::try_from(m).unwrap_or(0)));
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// The reference of an AI record (`rt` = 2: a formula of PtgRef3d/PtgArea3d) as the `c:f` text (`Sheet!$B$2:$B$5`,
/// several areas in parentheses); `None` without one.
fn ai_ref(g: &Globals, d: &[u8]) -> Option<String> {
    if d.u8_at(1) != 2 {
        return None;
    }
    let cce = usize::from(d.u16_at(6));
    let f = d.get(8..(8 + cce).min(d.len()))?;
    let mut areas: Vec<String> = Vec::new();
    let mut p = 0usize;
    let sheet = |ixti: u16| -> Option<String> {
        let tab = usize::try_from(*g.xti.get(usize::from(ixti))?).ok()?;
        let name = g.sheets.get(tab)?;
        let plain = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            && !name.starts_with(|c: char| c.is_ascii_digit());
        Some(if plain { name.clone() } else { format!("'{}'", name.replace('\'', "''")) })
    };
    let cell = |r: u16, c: u16| format!("${}${}", col_letters(u32::from(c & 0x3FFF)), u32::from(r) + 1);
    while p < f.len() {
        match f.u8_at(p) {
            0x3A | 0x5A | 0x7A => {
                let s = sheet(f.u16_at(p + 1))?;
                areas.push(format!("{s}!{}", cell(f.u16_at(p + 3), f.u16_at(p + 5))));
                p += 7;
            }
            0x3B | 0x5B | 0x7B => {
                let s = sheet(f.u16_at(p + 1))?;
                let (a, b) = (cell(f.u16_at(p + 3), f.u16_at(p + 7)), cell(f.u16_at(p + 5), f.u16_at(p + 9)));
                areas.push(if a == b { format!("{s}!{a}") } else { format!("{s}!{a}:{b}") });
                p += 11;
            }
            // the union operator and the parentheses of a multi-area reference
            0x10 | 0x15 => p += 1,
            // PtgMemFunc (cce u16) / PtgMemArea (4 reserved + cce u16): the areas follow
            0x29 | 0x49 | 0x69 => p += 3,
            0x26 | 0x46 | 0x66 => p += 7,
            _ => return None,
        }
    }
    match areas.len() {
        0 => None,
        1 => areas.pop(),
        _ => Some(format!("({})", areas.join(","))),
    }
}

// ───────────────────────────── text ─────────────────────────────

#[derive(Clone, Debug)]
struct TextProps {
    size: f64,
    bold: bool,
    italic: bool,
    underline: bool,
    color: String,
    face: String,
}

fn font_props(g: &Globals, ifnt: u16) -> TextProps {
    let ix = if ifnt < 4 { usize::from(ifnt) } else { usize::from(ifnt) - 1 };
    let f = g.fonts.get(ix).or_else(|| g.fonts.first());
    match f {
        Some(f) => TextProps {
            size: f64::from(f.height) / 20.0,
            bold: f.weight >= 600,
            italic: f.italic,
            underline: f.underline != 0,
            color: if f.color == 0x7FFF { "000000".into() } else { icv_color(g, f.color) },
            face: f.name.clone(),
        },
        None => TextProps { size: 10.0, bold: false, italic: false, underline: false, color: "000000".into(), face: "Arial".into() },
    }
}

fn xml_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

fn rpr_attrs(p: &TextProps) -> String {
    format!(
        "sz=\"{}\" b=\"{}\" i=\"{}\"{}",
        to_js_string(uo_jsnum::math::round(p.size * 100.0)),
        u8::from(p.bold),
        u8::from(p.italic),
        if p.underline { " u=\"sng\"" } else { "" }
    )
}

fn rpr_body(p: &TextProps) -> String {
    format!("{}<a:latin typeface=\"{}\"/>", solid(&p.color), xml_text(&p.face))
}

fn def_rpr(p: &TextProps) -> String {
    format!("<a:defRPr {}>{}</a:defRPr>", rpr_attrs(p), rpr_body(p))
}

fn body_pr(trot: u16) -> String {
    if trot == 255 {
        return "<a:bodyPr vert=\"wordArtVert\"/>".into();
    }
    if trot > 0 && trot <= 90 {
        return format!("<a:bodyPr rot=\"{}\" vert=\"horz\"/>", -i64::from(trot) * 60000);
    }
    if trot > 90 && trot <= 180 {
        return format!("<a:bodyPr rot=\"{}\" vert=\"horz\"/>", (i64::from(trot) - 90) * 60000);
    }
    "<a:bodyPr/>".into()
}

fn tx_pr(p: &TextProps, trot: u16) -> String {
    format!("<c:txPr>{}<a:lstStyle/><a:p><a:pPr>{}</a:pPr><a:endParaRPr lang=\"pt-BR\"/></a:p></c:txPr>", body_pr(trot), def_rpr(p))
}

fn text_node_props(g: &Globals, t: Option<&Node>, base: &TextProps) -> TextProps {
    let Some(t) = t else { return base.clone() };
    let mut p = t.kid(FONTX).map_or_else(|| base.clone(), |fx| font_props(g, fx.d.u16_at(0)));
    if t.d.u16_at(24) & 1 == 0 {
        p.color = icv_color(g, t.d.u16_at(26));
    }
    p
}

/// The rich title: the text in paragraphs with the run properties; the 2007 `txPr` (when present) gives the defaults.
fn rich_title(text: &str, p: &TextProps, trot: u16, own_tx: Option<&str>, own_sp: Option<String>) -> String {
    let paras: String = text
        .split("\r\n")
        .flat_map(|l| l.split(['\r', '\n']))
        .map(|line| {
            format!(
                "<a:p><a:pPr>{}</a:pPr><a:r><a:rPr lang=\"pt-BR\" {}>{}</a:rPr><a:t>{}</a:t></a:r></a:p>",
                def_rpr(p),
                rpr_attrs(p),
                rpr_body(p),
                xml_text(line)
            )
        })
        .collect();
    let sp = own_sp.unwrap_or_else(|| "<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>".into());
    match own_tx {
        // with the 2007 txPr the text keeps only its characters: the properties come from the element (as Excel writes it)
        Some(tx) => {
            let body = tx.strip_prefix("<c:txPr>").and_then(|b| b.strip_suffix("</c:txPr>"));
            let (bp, ls, ppr) = split_tx(body.unwrap_or(""));
            let paras: String = text
                .split("\r\n")
                .flat_map(|l| l.split(['\r', '\n']))
                .map(|line| format!("<a:p>{ppr}<a:r><a:rPr lang=\"pt-BR\"/><a:t>{}</a:t></a:r></a:p>", xml_text(line)))
                .collect();
            format!("<c:title><c:tx><c:rich>{bp}{ls}{paras}</c:rich></c:tx><c:overlay val=\"0\"/>{sp}</c:title>")
        }
        None => format!("<c:title><c:tx><c:rich>{}<a:lstStyle/>{paras}</c:rich></c:tx><c:overlay val=\"0\"/>{sp}</c:title>", body_pr(trot)),
    }
}

/// `bodyPr`, `lstStyle` and the first `pPr` of a `txPr` body (empty strings when absent).
fn split_tx(body: &str) -> (String, String, String) {
    let grab = |open: &str, close: &str| -> String {
        let Some(a) = body.find(open) else { return String::new() };
        let rest = &body[a..];
        if let Some(e) = rest.find(close) {
            return rest[..e + close.len()].to_owned();
        }
        // self-closing
        rest.find("/>").map_or_else(String::new, |e| rest[..e + 2].to_owned())
    };
    let bp = grab("<a:bodyPr", "</a:bodyPr>");
    let bp = if bp.ends_with("/>") || bp.ends_with("</a:bodyPr>") { bp } else { "<a:bodyPr/>".into() };
    let ls = grab("<a:lstStyle", "</a:lstStyle>");
    let ls = if ls.is_empty() { "<a:lstStyle/>".into() } else { ls };
    let ppr = grab("<a:pPr", "</a:pPr>");
    (bp, ls, ppr)
}

fn series_text(n: &Node) -> Option<String> {
    n.kid(SERIESTEXT).map(|st| read_xl_string(&st.d, 2, 1, &st.conts))
}

// ───────────────────────────── conversion ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Bar,
    Line,
    Pie,
    Area,
    Scatter,
    Radar,
    Bubble,
}

#[derive(Clone, Debug)]
enum Cat {
    Num(f64),
    Str(String),
}

#[derive(Clone, Debug)]
struct SeriesData<'n> {
    name: Option<String>,
    vals: Vec<Option<f64>>,
    val_fmt: String,
    cats: Option<Vec<Option<Cat>>>,
    cats_numeric: bool,
    cat_fmt: Option<String>,
    bubbles: Option<Vec<Option<f64>>>,
    node: &'n Node,
    group: usize,
    /// `c:f` of the name, the values, the categories and the bubble sizes (by AI id)
    refs: [Option<String>; 4],
    /// the trendline and error-bar series of this one (SERPARENT)
    aux: Vec<&'n Node>,
}

/// The chart's own cache: `[index][series][point]` (index 1 values, 2 categories, 3 bubble sizes) with each value's XF.
type Cache = HashMap<u16, HashMap<u16, HashMap<u16, (Cat, u16)>>>;

fn chart_cache(top: &[Node]) -> Cache {
    let mut out: Cache = HashMap::new();
    let mut cur: Option<u16> = None;
    for n in top {
        if n.t == SIINDEX {
            let k = n.d.u16_at(0);
            out.entry(k).or_default();
            cur = Some(k);
            continue;
        }
        let Some(k) = cur else { continue };
        let (p, s, xf) = (n.d.u16_at(0), n.d.u16_at(2), n.d.u16_at(4));
        let v = match n.t {
            NUMBER => Cat::Num(n.d.f64_at(6)),
            LABEL => Cat::Str(read_xl_string(&n.d, 6, 2, &n.conts)),
            _ => continue,
        };
        if usize::from(p) >= MAX_POINTS {
            continue;
        }
        out.entry(k).or_default().entry(s).or_default().insert(p, (v, xf));
    }
    out
}

fn num_xml(vals: &[Option<f64>], fmt: &str) -> String {
    let mut pts = String::new();
    for (i, v) in vals.iter().enumerate() {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            let _ = write!(pts, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", to_js_string(v));
        }
    }
    format!("<c:numCache><c:formatCode>{}</c:formatCode><c:ptCount val=\"{}\"/>{pts}</c:numCache>", xml_text(fmt), vals.len())
}

fn str_xml(vals: &[Option<Cat>]) -> String {
    let mut pts = String::new();
    for (i, v) in vals.iter().enumerate() {
        let s = match v {
            Some(Cat::Num(n)) => to_js_string(*n),
            Some(Cat::Str(s)) => s.clone(),
            None => continue,
        };
        if !s.is_empty() {
            let _ = write!(pts, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", xml_text(&s));
        }
    }
    format!("<c:strCache><c:ptCount val=\"{}\"/>{pts}</c:strCache>", vals.len())
}

struct GroupInfo<'n> {
    node: &'n Node,
    parent: &'n Node,
    kind: Kind,
    type_node: &'n Node,
    d3: bool,
    varied: bool,
}

/// The chart sheet of an `Excel.Chart.8` OLE `Workbook` stream as a `c:chartSpace` part (`None`: no BIFF8 chart in it).
#[must_use]
pub fn chart_space_from_workbook(workbook: &[u8]) -> Option<String> {
    let recs = records(workbook);
    let first = recs.first()?;
    if first.t != BOF || first.data.u16_at(0) != 0x0600 {
        return None;
    }
    let g = read_globals(&recs);
    // the chart substream: a BOF with dt = 0x20
    let start = recs.iter().position(|r| r.t == BOF && r.data.u16_at(2) == 0x20)?;
    chart_space_xml(&g, &recs[start..])
}

#[allow(clippy::too_many_lines)]
fn chart_space_xml(g: &Globals, recs: &[Rec]) -> Option<String> {
    let top = tree(recs);
    let chart = top.iter().find(|n| n.t == CHART)?;
    let cache = chart_cache(&top);

    // default fonts: DEFAULTTEXT 2 (all text) and 3 (axes)
    let mut base_text = font_props(g, 0);
    let mut base_tx: Option<String> = None;
    let mut axis_text: Option<TextProps> = None;
    for (i, n) in chart.kids.iter().enumerate() {
        if n.t != DEFAULTTEXT {
            continue;
        }
        let Some(t) = chart.kids.get(i + 1).filter(|t| t.t == TEXT) else { continue };
        let p = text_node_props(g, Some(t), &base_text);
        match n.d.u16_at(0) {
            2 => {
                base_text = p;
                base_tx = t.text_props();
            }
            3 => axis_text = Some(p),
            _ => {}
        }
    }
    let axis_text = axis_text.unwrap_or_else(|| base_text.clone());

    // chart groups (CHARTFORMAT) by AXISPARENT, in file order
    let parents: Vec<&Node> = chart.kids_of(AXISPARENT).collect();
    let mut groups: Vec<GroupInfo<'_>> = Vec::new();
    for ap in &parents {
        for cf in ap.kids_of(CHARTFORMAT) {
            let Some(tn) = cf.kids.iter().find(|k| matches!(k.t, BAR | LINE | PIE | AREA | SCATTER | RADAR | RADARAREA | BOPPOP)) else {
                continue;
            };
            let mut kind = match tn.t {
                BAR => Kind::Bar,
                LINE => Kind::Line,
                AREA => Kind::Area,
                SCATTER => Kind::Scatter,
                RADAR | RADARAREA => Kind::Radar,
                _ => Kind::Pie,
            };
            if kind == Kind::Scatter && tn.d.u16_at(4) & 1 != 0 {
                kind = Kind::Bubble;
            }
            groups.push(GroupInfo {
                node: cf,
                parent: ap,
                kind,
                type_node: tn,
                d3: cf.kid(D3).is_some(),
                varied: cf.d.u16_at(16) & 1 != 0,
            });
        }
    }
    if groups.is_empty() {
        return None;
    }

    // series
    let mut series: Vec<SeriesData<'_>> = Vec::new();
    // trendline and error-bar series: SERPARENT (1-based index of the parent among the SERIES records)
    let mut aux: Vec<(usize, &Node)> = Vec::new();
    let mut by_record: HashMap<usize, usize> = HashMap::new();
    for (s_idx, s) in chart.kids_of(SERIES).enumerate() {
        if let Some(sp) = s.kid(SERPARENT) {
            aux.push((usize::from(sp.d.u16_at(0)).saturating_sub(1), s));
            continue;
        }
        by_record.insert(s_idx, series.len());
        let s_key = u16::try_from(s_idx).unwrap_or(u16::MAX);
        let mut sd = SeriesData {
            name: None,
            vals: Vec::new(),
            val_fmt: "General".into(),
            cats: None,
            cats_numeric: false,
            cat_fmt: None,
            bubbles: None,
            node: s,
            group: usize::from(s.kid(SERTOCRT).map_or(0, |k| k.d.u16_at(0))),
            refs: [None, None, None, None],
            aux: Vec::new(),
        };
        let fmt_of = |xf: u16| ifmt_code(g, g.xf_fmt.get(usize::from(xf)).copied().unwrap_or(0));
        for ai in s.kids_of(AI) {
            let (id, unlinked, ifmt) = (ai.d.u8_at(0), ai.d.u16_at(2) & 1 != 0, ai.d.u16_at(4));
            if let Some(slot) = sd.refs.get_mut(usize::from(id)) {
                *slot = ai_ref(g, &ai.d);
            }
            match id {
                0 => sd.name = series_text(s),
                1 => {
                    let n = usize::from(s.d.u16_at(6));
                    if let Some(c) = cache.get(&1).and_then(|m| m.get(&s_key)) {
                        let len = if n > 0 { n } else { c.keys().map(|k| usize::from(*k) + 1).max().unwrap_or(0) }.min(MAX_POINTS);
                        sd.vals = (0..len)
                            .map(|i| match c.get(&u16::try_from(i).unwrap_or(u16::MAX)) {
                                Some((Cat::Num(v), _)) => Some(*v),
                                _ => None,
                            })
                            .collect();
                        let first_xf = (0..len).find_map(|i| c.get(&u16::try_from(i).unwrap_or(u16::MAX)).map(|(_, xf)| *xf));
                        sd.val_fmt = if unlinked { ifmt_code(g, ifmt) } else { first_xf.map_or_else(|| "General".into(), fmt_of) };
                    } else {
                        sd.vals = vec![None; n.min(MAX_POINTS)];
                    }
                }
                2 => {
                    if let Some(c) = cache.get(&2).and_then(|m| m.get(&s_key)).filter(|c| !c.is_empty()) {
                        let len = c.keys().map(|k| usize::from(*k) + 1).max().unwrap_or(0).min(MAX_POINTS);
                        let cats: Vec<Option<Cat>> =
                            (0..len).map(|i| c.get(&u16::try_from(i).unwrap_or(u16::MAX)).map(|(v, _)| v.clone())).collect();
                        sd.cats_numeric = cats.iter().all(|v| !matches!(v, Some(Cat::Str(_))));
                        let first_xf = cats
                            .iter()
                            .enumerate()
                            .find_map(|(i, v)| v.as_ref().and(c.get(&u16::try_from(i).unwrap_or(u16::MAX)).map(|(_, xf)| *xf)));
                        sd.cat_fmt = first_xf.map(fmt_of);
                        sd.cats = Some(cats);
                    }
                }
                3 => {
                    if let Some(c) = cache.get(&3).and_then(|m| m.get(&s_key)) {
                        let len = c.keys().map(|k| usize::from(*k) + 1).max().unwrap_or(0).min(MAX_POINTS);
                        sd.bubbles = Some(
                            (0..len)
                                .map(|i| match c.get(&u16::try_from(i).unwrap_or(u16::MAX)) {
                                    Some((Cat::Num(v), _)) => Some(*v),
                                    _ => None,
                                })
                                .collect(),
                        );
                    }
                }
                _ => {}
            }
        }
        if sd.name.is_none() {
            sd.name = series_text(s);
        }
        series.push(sd);
    }
    for (parent, node) in aux {
        if let Some(sd) = by_record.get(&parent).and_then(|&i| series.get_mut(i)) {
            sd.aux.push(node);
        }
    }

    // titles by OBJECTLINK (1 chart, 2 value axis, 3 category axis)
    let mut titles: HashMap<u16, &Node> = HashMap::new();
    for t in chart.kids_of(TEXT).chain(parents.iter().flat_map(|p| p.kids_of(TEXT))) {
        if let Some(ol) = t.kid(OBJECTLINK) {
            titles.insert(ol.d.u16_at(0), t);
        }
    }

    // axes (one pair per AXISPARENT)
    let mut ax_id = 100_000u32;
    let mut axes_xml = String::new();
    let mut parent_axes: Vec<(*const Node, (u32, u32))> = Vec::new();
    for ap in &parents {
        let Some((gi, gr)) = groups.iter().enumerate().find(|(_, x)| std::ptr::eq(x.parent, *ap)) else { continue };
        if gr.kind == Kind::Pie {
            continue;
        }
        let secondary = ap.d.u16_at(0) == 1;
        let ids = (ax_id, ax_id + 1);
        ax_id += 2;
        parent_axes.push((std::ptr::from_ref(*ap), ids));
        let x_axis = ap.kids_of(AXIS).find(|a| a.d.u16_at(0) == 0);
        let y_axis = ap.kids_of(AXIS).find(|a| a.d.u16_at(0) == 1);
        let xy = matches!(gr.kind, Kind::Scatter | Kind::Bubble);
        let horizontal = gr.kind == Kind::Bar && gr.type_node.d.u16_at(4) & 1 != 0;
        let cat_range = x_axis.and_then(|a| a.kid(CATSERRANGE));
        let between = cat_range.map_or(gr.kind == Kind::Bar, |c| c.d.u16_at(6) & 1 != 0);
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
        let cat_fmt = first.filter(|f| f.cats_numeric).and_then(|f| f.cat_fmt.clone());
        axes_xml.push_str(&axis_xml(
            g,
            x_axis,
            if xy { "val" } else { "cat" },
            ids,
            x_pos,
            secondary,
            &axis_text,
            if secondary { None } else { titles.get(&3).copied() },
            cat_fmt.as_deref(),
            between,
            xy,
        ));
        let val_fmt = first.map_or_else(|| "General".to_owned(), |f| f.val_fmt.clone());
        axes_xml.push_str(&axis_xml(
            g,
            y_axis,
            "val",
            (ids.1, ids.0),
            y_pos,
            secondary,
            &axis_text,
            if secondary { None } else { titles.get(&2).copied() },
            Some(&val_fmt),
            between,
            xy,
        ));
    }

    // groups → c:xxxChart
    let mut plot = String::new();
    let mut order = 0usize;
    for (gi, gr) in groups.iter().enumerate() {
        let sers: Vec<&SeriesData<'_>> = series.iter().filter(|s| s.group == gi).collect();
        if sers.is_empty() {
            continue;
        }
        let tr = gr.type_node.d.as_slice();
        let mut ser_xml = String::new();
        for s in &sers {
            let idx = series.iter().position(|x| std::ptr::eq(x, *s)).unwrap_or(0);
            ser_xml.push_str(&series_xml(g, s, gr.kind, gr.varied, order, idx));
            order += 1;
        }
        let (open, close) = match gr.kind {
            Kind::Bar => {
                let grbit = tr.u16_at(4);
                let grouping = if grbit & 4 != 0 {
                    "percentStacked"
                } else if grbit & 2 != 0 {
                    "stacked"
                } else {
                    "clustered"
                };
                let tag = if gr.d3 { "c:bar3DChart" } else { "c:barChart" };
                // BIFF's pcOverlap is the negative of c:overlap (measured: the .pptx twin of a .ppt chart writes −27 where the BAR
                // record holds 27; the TS writes it unchanged)
                let overlap = if gr.d3 { String::new() } else { format!("<c:overlap val=\"{}\"/>", -tr.i16_at(0).clamp(-100, 100)) };
                (
                    format!(
                        "<{tag}><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"{}\"/>{ser_xml}<c:gapWidth val=\"{}\"/>{overlap}",
                        if grbit & 1 != 0 { "bar" } else { "col" },
                        u8::from(gr.varied),
                        tr.u16_at(2)
                    ),
                    tag,
                )
            }
            Kind::Line | Kind::Area => {
                let grbit = tr.u16_at(0);
                let grouping = if grbit & 2 != 0 {
                    "percentStacked"
                } else if grbit & 1 != 0 {
                    "stacked"
                } else {
                    "standard"
                };
                let tag = match (gr.kind, gr.d3) {
                    (Kind::Line, false) => "c:lineChart",
                    (Kind::Line, true) => "c:line3DChart",
                    (_, false) => "c:areaChart",
                    _ => "c:area3DChart",
                };
                let marker = if gr.kind == Kind::Line && !gr.d3 { "<c:marker val=\"1\"/>" } else { "" };
                (format!("<{tag}><c:grouping val=\"{grouping}\"/><c:varyColors val=\"{}\"/>{ser_xml}{marker}", u8::from(gr.varied)), tag)
            }
            Kind::Pie => {
                let donut = tr.u16_at(2);
                if donut > 0 {
                    let _ = write!(
                        plot,
                        "<c:doughnutChart><c:varyColors val=\"1\"/>{ser_xml}<c:firstSliceAng val=\"{}\"/><c:holeSize val=\"{}\"/></c:doughnutChart>",
                        tr.u16_at(0) % 360,
                        donut.clamp(10, 90)
                    );
                } else {
                    let tag = if gr.d3 { "c:pie3DChart" } else { "c:pieChart" };
                    let ang = if gr.d3 { String::new() } else { format!("<c:firstSliceAng val=\"{}\"/>", tr.u16_at(0) % 360) };
                    let _ = write!(plot, "<{tag}><c:varyColors val=\"1\"/>{ser_xml}{ang}</{tag}>");
                }
                continue;
            }
            Kind::Scatter => {
                (format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{ser_xml}"), "c:scatterChart")
            }
            Kind::Bubble => {
                let scale = if tr.u16_at(0) == 0 { 100 } else { tr.u16_at(0) };
                (format!("<c:bubbleChart><c:varyColors val=\"0\"/>{ser_xml}<c:bubbleScale val=\"{scale}\"/>"), "c:bubbleChart")
            }
            Kind::Radar => {
                let style = if gr.type_node.t == RADARAREA { "filled" } else { "marker" };
                (format!("<c:radarChart><c:radarStyle val=\"{style}\"/><c:varyColors val=\"0\"/>{ser_xml}"), "c:radarChart")
            }
        };
        let ids = parent_axes.iter().find(|(p, _)| std::ptr::eq(*p, gr.parent)).map_or_else(
            || {
                let i = (ax_id, ax_id + 1);
                ax_id += 2;
                i
            },
            |(_, i)| *i,
        );
        let _ = write!(plot, "{open}<c:axId val=\"{}\"/><c:axId val=\"{}\"/></{close}>", ids.0, ids.1);
    }
    if plot.is_empty() {
        return None;
    }

    // plot area and chart area
    let ap0 = parents.first().copied();
    let plot_frame =
        ap0.and_then(|ap| ap.kids.iter().position(|k| k.t == PLOTAREA).and_then(|i| ap.kids.get(i + 1))).filter(|f| f.t == FRAME);
    let plot_sp = match plot_frame {
        Some(f) => sp_pr(Some(f), 0, || {
            format!("{}{}", fill_xml(g, f.kid(AREAFORMAT), Some("C0C0C0")), ln_xml(g, f.kid(LINEFORMAT), "808080", 9525))
        }),
        None => "<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>".into(),
    };
    // the plot layout: CrtLayout12A (Office 2007+) says automatic or manual; else the AXISPARENT rectangle (SPRC, 1/4000)
    let plot_layout = match chart.kid(CRT_LAYOUT12A) {
        Some(l) => layout12(&l.d, 26, true),
        None => ap0
            .map_or_else(|| "<c:layout/>".into(), |ap| spc_layout(ap.d.i32_at(2), ap.d.i32_at(6), ap.d.i32_at(10), ap.d.i32_at(14), true)),
    };
    let chart_frame = chart.kid(FRAME);
    let chart_sp = sp_pr(chart_frame, 0, || {
        format!(
            "{}{}",
            fill_xml(g, chart_frame.and_then(|f| f.kid(AREAFORMAT)), Some("FFFFFF")),
            ln_xml(g, chart_frame.and_then(|f| f.kid(LINEFORMAT)), "000000", 3175)
        )
    });

    // title
    let mut title = String::new();
    if let Some(tt) = titles.get(&1).filter(|t| t.d.u16_at(24) & 0x40 == 0) {
        let mut text = series_text(tt);
        if text.is_none() && series.len() == 1 {
            text = series.first().and_then(|s| s.name.clone());
        }
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            let base = TextProps { bold: true, size: base_text.size * 1.2, ..base_text.clone() };
            let p = text_node_props(g, Some(tt), &base);
            let own_sp = tt.kid(FRAME).and_then(|f| f.shape_props(0));
            title = rich_title(&text, &p, tt.d.u16_at(30), tt.text_props().as_deref(), own_sp);
        }
    }

    // legend
    let mut legend = String::new();
    if let Some(lg) = groups.iter().find_map(|x| x.node.kid(LEGEND)) {
        // Office 2007+: CrtLayout12's autolayouttype (bits 1–4 of its flags) is the docked position — the LEGEND record then
        // says "not docked" (7) with Excel's computed box (measured, pb-graficos-partes.ppt: bottom legend)
        let pos_ix = match lg.kid(CRT_LAYOUT12) {
            Some(l) => usize::from((l.d.u16_at(16) >> 1) & 0xF),
            None => usize::from(lg.d.u8_at(16)),
        };
        let pos = ["b", "tr", "t", "r", "l"].get(pos_ix).copied().unwrap_or("r");
        let lt = lg.kid(TEXT);
        let lf = lg.kid(FRAME);
        let lsp = match lf {
            Some(f) => sp_pr(Some(f), 0, || {
                format!("{}{}", fill_xml(g, f.kid(AREAFORMAT), Some("FFFFFF")), ln_xml(g, f.kid(LINEFORMAT), "000000", 3175))
            }),
            None => format!("<c:spPr>{}<a:ln w=\"3175\">{}</a:ln></c:spPr>", solid("FFFFFF"), solid("000000")),
        };
        let lay = match lg.kid(CRT_LAYOUT12) {
            Some(l) => layout12(&l.d, 18, false),
            None => spc_layout(lg.d.i32_at(0), lg.d.i32_at(4), lg.d.i32_at(8), lg.d.i32_at(12), false),
        };
        let lay = if lay == "<c:layout/>" { String::new() } else { lay };
        let ltx = lg.text_props().unwrap_or_else(|| tx_pr(&text_node_props(g, lt, &base_text), 0));
        legend = format!("<c:legend><c:legendPos val=\"{pos}\"/>{lay}<c:overlay val=\"0\"/>{lsp}{ltx}</c:legend>");
    }

    // 3D
    let mut view3d = String::new();
    if let Some(d3) = groups.iter().find_map(|x| x.node.kid(D3)) {
        let d = d3.d.as_slice();
        let persp = d.u16_at(12) & 1 != 0;
        let depth = if d.u16_at(8) == 0 { 100 } else { d.u16_at(8) };
        let _ = write!(
            view3d,
            "<c:view3D><c:rotX val=\"{}\"/><c:rotY val=\"{}\"/><c:depthPercent val=\"{}\"/><c:rAngAx val=\"{}\"/>{}</c:view3D>",
            d.i16_at(2).clamp(-90, 90),
            d.u16_at(0) % 360,
            depth.clamp(20, 2000),
            u8::from(!persp),
            if persp { format!("<c:perspective val=\"{}\"/>", (i32::from(d.i16_at(4)) * 2).clamp(0, 240)) } else { String::new() }
        );
    }

    let sp = chart.kid(SHTPROPS);
    let blanks = sp.map_or("gap", |s| ["gap", "zero", "span"].get(usize::from(s.d.u8_at(2))).copied().unwrap_or("gap"));
    let plot_vis_only = sp.is_none_or(|s| s.d.u16_at(0) & 2 != 0);
    let auto_deleted = title.is_empty();
    let chart_tx = chart.text_props().or(base_tx).unwrap_or_else(|| tx_pr(&base_text, 0));
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/><c:chart>{title}<c:autoTitleDeleted val=\"{}\"/>{view3d}<c:plotArea>{plot_layout}{plot}{axes_xml}{plot_sp}</c:plotArea>{legend}<c:plotVisOnly val=\"{}\"/><c:dispBlanksAs val=\"{blanks}\"/></c:chart>{chart_sp}{chart_tx}</c:chartSpace>",
        u8::from(auto_deleted),
        u8::from(plot_vis_only)
    ))
}

/// `spcLayout`: a rectangle in SPRC (1/4000 of the chart area) → a manual `c:layout`; out of the plausible range (automatic
/// position written as 0) = automatic.
fn spc_layout(x: i32, y: i32, w: i32, h: i32, inner: bool) -> String {
    if !(w > 40 && h > 40 && x >= 0 && y >= 0 && x.saturating_add(w) <= 4400 && y.saturating_add(h) <= 4400) {
        return "<c:layout/>".into();
    }
    let f = |v: i32| to_js_string(uo_jsnum::math::round(f64::from(v) / 4000.0 * 100_000.0) / 100_000.0);
    format!(
        "<c:layout><c:manualLayout>{}<c:xMode val=\"edge\"/><c:yMode val=\"edge\"/><c:x val=\"{}\"/><c:y val=\"{}\"/><c:w val=\"{}\"/><c:h val=\"{}\"/></c:manualLayout></c:layout>",
        if inner { "<c:layoutTarget val=\"inner\"/>" } else { "" },
        f(x),
        f(y),
        f(w),
        f(h)
    )
}

/// `CrtLayout12`/`CrtLayout12A` (Office 2007+): the four modes at `modes` (0 automatic, 1 factor, 2 edge) and the fractions
/// after them; all automatic = `<c:layout/>`.
fn layout12(d: &[u8], modes: usize, has_target: bool) -> String {
    let m: [u16; 4] = [d.u16_at(modes), d.u16_at(modes + 2), d.u16_at(modes + 4), d.u16_at(modes + 6)];
    if m.iter().all(|&x| x == 0) {
        return "<c:layout/>".into();
    }
    let v = |k: usize| d.f64_at(modes + 8 + 8 * k);
    let mode = |x: u16| if x == 1 { "factor" } else { "edge" };
    let inner = has_target && d.u16_at(16) & 1 != 0;
    let mut out = String::from("<c:layout><c:manualLayout>");
    if inner {
        out.push_str("<c:layoutTarget val=\"inner\"/>");
    }
    if m[0] != 0 {
        let _ = write!(out, "<c:xMode val=\"{}\"/>", mode(m[0]));
    }
    if m[1] != 0 {
        let _ = write!(out, "<c:yMode val=\"{}\"/>", mode(m[1]));
    }
    for (k, (tag, on)) in [("x", m[0]), ("y", m[1]), ("w", m[2]), ("h", m[3])].iter().enumerate() {
        if *on != 0 && v(k).is_finite() {
            let _ = write!(out, "<c:{tag} val=\"{}\"/>", to_js_string(v(k)));
        }
    }
    out.push_str("</c:manualLayout></c:layout>");
    out
}

#[allow(clippy::too_many_lines)]
fn series_xml(g: &Globals, s: &SeriesData<'_>, kind: Kind, varied: bool, order: usize, idx: usize) -> String {
    let dfs: Vec<&Node> = s.node.kids_of(DATAFORMAT).collect();
    let own = dfs.iter().copied().find(|d| d.d.u16_at(0) == 0xFFFF);
    let n = own.map_or(idx, |o| usize::from(o.d.u16_at(4)));
    let line_auto = auto_line(g, n);
    let fill_auto = auto_fill(g, n);
    let filled = matches!(kind, Kind::Bar | Kind::Area | Kind::Pie | Kind::Bubble);
    let lw = if kind == Kind::Scatter { 9525 } else { 19050 };
    let marker_of = |df: Option<&Node>| -> String {
        if !matches!(kind, Kind::Line | Kind::Scatter | Kind::Radar) {
            return String::new();
        }
        let mf = df.and_then(|d| d.kid(MARKERFORMAT));
        let Some(mf) = mf.filter(|m| m.d.u16_at(10) & 1 == 0) else {
            let sym = AUTO_MARKERS[n % AUTO_MARKERS.len()];
            return format!(
                "<c:marker><c:symbol val=\"{sym}\"/><c:size val=\"5\"/><c:spPr>{}<a:ln w=\"9525\">{}</a:ln></c:spPr></c:marker>",
                solid(&line_auto),
                solid(&line_auto)
            );
        };
        let r = mf.d.as_slice();
        let (imk, grbit) = (r.u16_at(8), r.u16_at(10));
        let sym = MARKERS.get(usize::from(imk)).copied().unwrap_or("none");
        if sym == "none" {
            return "<c:marker><c:symbol val=\"none\"/></c:marker>".into();
        }
        let size = uo_jsnum::math::min(uo_jsnum::math::max(uo_jsnum::math::round(f64::from(r.u32_at(16)) / 20.0), 2.0), 72.0);
        let fill = if grbit & 0x10 != 0 { "<a:noFill/>".to_owned() } else { solid(&icv_color(g, r.u16_at(14))) };
        let line = if grbit & 0x20 != 0 {
            "<a:ln><a:noFill/></a:ln>".to_owned()
        } else {
            format!("<a:ln w=\"9525\">{}</a:ln>", solid(&icv_color(g, r.u16_at(12))))
        };
        format!("<c:marker><c:symbol val=\"{sym}\"/><c:size val=\"{}\"/><c:spPr>{fill}{line}</c:spPr></c:marker>", to_js_string(size))
    };
    let mut x = format!("<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{order}\"/>");
    if let Some(name) = &s.name {
        match &s.refs[0] {
            Some(f) => {
                let _ = write!(
                    x,
                    "<c:tx><c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>",
                    xml_text(f),
                    xml_text(name)
                );
            }
            None => {
                let _ = write!(x, "<c:tx><c:v>{}</c:v></c:tx>", xml_text(name));
            }
        }
    }
    x += &sp_pr(own, 0, || {
        if filled {
            format!(
                "{}{}",
                fill_xml(g, own.and_then(|o| o.kid(AREAFORMAT)), Some(&fill_auto)),
                ln_xml(g, own.and_then(|o| o.kid(LINEFORMAT)), "000000", 3175)
            )
        } else {
            ln_xml(g, own.and_then(|o| o.kid(LINEFORMAT)), &line_auto, lw)
        }
    });
    x += &marker_of(own);
    if matches!(kind, Kind::Bar | Kind::Bubble) {
        x += "<c:invertIfNegative val=\"0\"/>";
    }
    // points: own formatting (DATAFORMAT xi) and varied colours (pie)
    let pts: HashMap<u16, &Node> = dfs.iter().filter(|d| d.d.u16_at(0) != 0xFFFF).map(|d| (d.d.u16_at(0), *d)).collect();
    let vary_pts = (kind == Kind::Pie || varied) && filled;
    if vary_pts || !pts.is_empty() {
        for p in 0..s.vals.len() {
            let df = pts.get(&u16::try_from(p).unwrap_or(u16::MAX)).copied();
            if df.is_none() && !vary_pts {
                continue;
            }
            if !filled {
                let ln = df.and_then(|d| d.kid(LINEFORMAT)).or_else(|| own.and_then(|o| o.kid(LINEFORMAT)));
                let sp = sp_pr(df, 0, || ln_xml(g, ln, &line_auto, lw));
                let _ = write!(x, "<c:dPt><c:idx val=\"{p}\"/>{}{sp}</c:dPt>", marker_of(df));
                continue;
            }
            let own_area = df.and_then(|d| d.kid(AREAFORMAT)).or_else(|| own.and_then(|o| o.kid(AREAFORMAT)));
            let area_auto = own_area.is_none_or(|a| a.d.u16_at(10) & 1 != 0);
            let fill_at = if vary_pts && area_auto { auto_fill(g, p) } else { fill_auto.clone() };
            let pie = df.and_then(|d| d.kid(PIEFORMAT)).map_or(0, |k| k.d.u16_at(0));
            // a varied point without its own XML: the series XML is for the series colour, the point keeps the automatic one
            let sp = sp_pr(df, 0, || {
                let ln = df.and_then(|d| d.kid(LINEFORMAT)).or_else(|| own.and_then(|o| o.kid(LINEFORMAT)));
                format!("{}{}", fill_xml(g, own_area, Some(&fill_at)), ln_xml(g, ln, "000000", 3175))
            });
            let _ = write!(
                x,
                "<c:dPt><c:idx val=\"{p}\"/>{}{}{sp}</c:dPt>",
                if kind == Kind::Bar { "<c:invertIfNegative val=\"0\"/>" } else { "" },
                if pie > 0 { format!("<c:explosion val=\"{pie}\"/>") } else { String::new() }
            );
        }
    }
    // data labels (ATTACHEDLABEL of the series)
    if let Some(al) = own.and_then(|o| o.kid(ATTACHEDLABEL)) {
        let f = al.d.u16_at(0);
        let b = |bit: u16| u8::from(f & bit != 0);
        if f & 0x77 != 0 {
            let pie = kind == Kind::Pie;
            let _ = write!(
                x,
                "<c:dLbls>{}<c:showLegendKey val=\"0\"/><c:showVal val=\"{}\"/><c:showCatName val=\"{}\"/><c:showSerName val=\"{}\"/><c:showPercent val=\"{}\"/><c:showBubbleSize val=\"{}\"/>{}</c:dLbls>",
                if pie { "<c:dLblPos val=\"bestFit\"/>" } else { "" },
                b(1),
                b(0x10) | b(4),
                b(0x40),
                b(2) | b(4),
                b(0x20),
                if pie { "<c:separator>\n</c:separator><c:showLeaderLines val=\"1\"/>" } else { "" }
            );
        }
    }
    x += &aux_xml(g, s, kind);
    let xy = matches!(kind, Kind::Scatter | Kind::Bubble);
    let f_of = |k: usize| s.refs[k].as_deref().map(xml_text).unwrap_or_default();
    if let Some(cats) = &s.cats {
        let tag = if xy { "c:xVal" } else { "c:cat" };
        if s.cats_numeric {
            let nums: Vec<Option<f64>> = cats.iter().map(|v| if let Some(Cat::Num(n)) = v { Some(*n) } else { None }).collect();
            let _ = write!(
                x,
                "<{tag}><c:numRef><c:f>{}</c:f>{}</c:numRef></{tag}>",
                f_of(2),
                num_xml(&nums, s.cat_fmt.as_deref().unwrap_or("General"))
            );
        } else {
            let _ = write!(x, "<{tag}><c:strRef><c:f>{}</c:f>{}</c:strRef></{tag}>", f_of(2), str_xml(cats));
        }
    }
    let vtag = if xy { "c:yVal" } else { "c:val" };
    let _ = write!(x, "<{vtag}><c:numRef><c:f>{}</c:f>{}</c:numRef></{vtag}>", f_of(1), num_xml(&s.vals, &s.val_fmt));
    if kind == Kind::Bubble {
        let ones: Vec<Option<f64>> = s.vals.iter().map(|_| Some(1.0)).collect();
        let _ = write!(
            x,
            "<c:bubbleSize><c:numRef><c:f>{}</c:f>{}</c:numRef></c:bubbleSize>",
            f_of(3),
            num_xml(s.bubbles.as_deref().unwrap_or(&ones), "General")
        );
    }
    if matches!(kind, Kind::Line | Kind::Scatter) {
        let smooth = own.and_then(|o| o.kid(SERFMT)).is_some_and(|k| k.d.u16_at(0) & 1 != 0);
        let _ = write!(x, "<c:smooth val=\"{}\"/>", u8::from(smooth));
    }
    x + "</c:ser>"
}

/// The trendlines (SERAUXTREND) and error bars (SERAUXERRBAR; a "both" pair is two series, plus and minus) of a series,
/// with the line of their own DATAFORMAT (its 2007 XML when present).
fn aux_xml(g: &Globals, s: &SeriesData<'_>, kind: Kind) -> String {
    let mut out = String::new();
    let line_of = |n: &Node| -> String {
        let df = n.kids_of(DATAFORMAT).find(|d| d.d.u16_at(0) == 0xFFFF);
        sp_pr(df, 0, || ln_xml(g, df.and_then(|d| d.kid(LINEFORMAT)), "000000", 9525))
    };
    for n in &s.aux {
        let Some(t) = n.kid(SERAUXTREND) else { continue };
        let d = t.d.as_slice();
        let (regt, ord) = (d.u8_at(0), d.u8_at(1));
        let kind_xml = match (regt, ord) {
            (0, 0 | 1) => "<c:trendlineType val=\"linear\"/>".to_owned(),
            (0, o) => format!("<c:trendlineType val=\"poly\"/><c:order val=\"{}\"/>", o.min(6)),
            (1, _) => "<c:trendlineType val=\"exp\"/>".to_owned(),
            (2, _) => "<c:trendlineType val=\"log\"/>".to_owned(),
            (3, _) => "<c:trendlineType val=\"power\"/>".to_owned(),
            (4, o) => format!("<c:trendlineType val=\"movingAvg\"/><c:period val=\"{}\"/>", o.max(2)),
            _ => continue,
        };
        // numIntercept (+2), fEquation (+10), fRSquared (+11), numForecast (+12), numBackcast (+20): an Xnum "not set" is a NaN
        let num = |o: usize| Some(d.f64_at(o)).filter(|v| v.is_finite());
        let mut extra = String::new();
        if let Some(f) = num(12).filter(|v| *v != 0.0) {
            let _ = write!(extra, "<c:forward val=\"{}\"/>", to_js_string(f));
        }
        if let Some(b) = num(20).filter(|v| *v != 0.0) {
            let _ = write!(extra, "<c:backward val=\"{}\"/>", to_js_string(b));
        }
        if let Some(i) = num(2) {
            let _ = write!(extra, "<c:intercept val=\"{}\"/>", to_js_string(i));
        }
        let _ = write!(
            out,
            "<c:trendline>{}{kind_xml}{extra}<c:dispRSqr val=\"{}\"/><c:dispEq val=\"{}\"/></c:trendline>",
            line_of(n),
            u8::from(d.u8_at(11) != 0),
            u8::from(d.u8_at(10) != 0)
        );
    }
    // error bars: sertm 1/2 = x plus/minus, 3/4 = y plus/minus; ebsrc 1 percentage, 2 fixed, 3 std dev, 4 custom, 5 std error
    for dir in ["y", "x"] {
        let parts: Vec<(&Node, u8, u8, bool, f64)> = s
            .aux
            .iter()
            .filter_map(|n| n.kid(SERAUXERRBAR).map(|e| (*n, e.d.u8_at(0), e.d.u8_at(1), e.d.u8_at(2) != 0, e.d.f64_at(4))))
            .filter(|(_, sertm, ..)| if dir == "y" { matches!(sertm, 3 | 4) } else { matches!(sertm, 1 | 2) })
            .collect();
        let Some(&(first, _, src, cap, val)) = parts.first() else { continue };
        let plus = parts.iter().any(|p| matches!(p.1, 1 | 3));
        let minus = parts.iter().any(|p| matches!(p.1, 2 | 4));
        let ty = match (plus, minus) {
            (true, true) => "both",
            (true, false) => "plus",
            _ => "minus",
        };
        let val_ty = match src {
            1 => "percentage",
            2 => "fixedVal",
            3 => "stdDev",
            5 => "stdErr",
            _ => "cust",
        };
        // the line: the half that carries the 2007 XML (Excel writes it on one of the two), else the first
        let with_xml = parts.iter().find(|p| p.0.kids_of(DATAFORMAT).any(|d| d.shape_props(0).is_some())).map_or(first, |p| p.0);
        let v = if val.is_finite() { format!("<c:val val=\"{}\"/>", to_js_string(val)) } else { String::new() };
        let dir_xml =
            if matches!(kind, Kind::Scatter | Kind::Bubble) || dir == "x" { format!("<c:errDir val=\"{dir}\"/>") } else { String::new() };
        let _ = write!(
            out,
            "<c:errBars>{dir_xml}<c:errBarType val=\"{ty}\"/><c:errValType val=\"{val_ty}\"/><c:noEndCap val=\"{}\"/>{v}{}</c:errBars>",
            u8::from(!cap),
            line_of(with_xml)
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn axis_xml(
    g: &Globals,
    ax: Option<&Node>,
    kind: &str,
    ids: (u32, u32),
    pos: &str,
    secondary: bool,
    base: &TextProps,
    title_node: Option<&Node>,
    fmt: Option<&str>,
    between: bool,
    xy: bool,
) -> String {
    let tick = ax.and_then(|a| a.kid(TICK));
    let vr = ax.and_then(|a| a.kid(VALUERANGE));
    let cr = ax.and_then(|a| a.kid(CATSERRANGE));
    let tag = if kind == "cat" { "c:catAx" } else { "c:valAx" };
    let mut crosses = if secondary { "<c:crosses val=\"max\"/>".to_owned() } else { "<c:crosses val=\"autoZero\"/>".to_owned() };
    let mut scaling = String::new();
    let mut units = String::new();
    if let Some(vr) = vr {
        let r = vr.d.as_slice();
        let fl = r.u16_at(40);
        let reversed = fl & 0x40 != 0;
        if fl & 0x20 != 0 {
            scaling += "<c:logBase val=\"10\"/>";
        }
        let _ = write!(scaling, "<c:orientation val=\"{}\"/>", if reversed { "maxMin" } else { "minMax" });
        if fl & 2 == 0 {
            let _ = write!(scaling, "<c:max val=\"{}\"/>", to_js_string(r.f64_at(8)));
        }
        if fl & 1 == 0 {
            let _ = write!(scaling, "<c:min val=\"{}\"/>", to_js_string(r.f64_at(0)));
        }
        if fl & 0x10 == 0 {
            crosses = format!("<c:crossesAt val=\"{}\"/>", to_js_string(r.f64_at(32)));
        }
        if fl & 0x80 != 0 {
            crosses = "<c:crosses val=\"max\"/>".into();
        }
        if fl & 4 == 0 {
            let _ = write!(units, "<c:majorUnit val=\"{}\"/>", to_js_string(r.f64_at(16)));
        }
        if fl & 8 == 0 {
            let _ = write!(units, "<c:minorUnit val=\"{}\"/>", to_js_string(r.f64_at(24)));
        }
    } else {
        let mut reversed = false;
        if let Some(cr) = cr {
            reversed = cr.d.u16_at(6) & 4 != 0;
            if cr.d.u16_at(6) & 2 != 0 {
                crosses = "<c:crosses val=\"max\"/>".into();
            }
        }
        scaling = format!("<c:orientation val=\"{}\"/>", if reversed { "maxMin" } else { "minMax" });
    }
    // lines: the axis (AXISLINEFORMAT 0) and the gridlines (1 major, 2 minor); the 2007 XML by the same context
    let mut axis_line = format!("<a:ln w=\"9525\">{}</a:ln>", solid("000000"));
    let mut major = String::new();
    let mut minor = String::new();
    if let Some(a) = ax {
        for (i, k) in a.kids.iter().enumerate() {
            if k.t != AXISLINEFORMAT {
                continue;
            }
            let lf = a.kids.get(i + 1).filter(|n| n.t == LINEFORMAT);
            if k.d.u16_at(0) == 0 {
                axis_line = ln_xml(g, lf, "000000", 9525);
            } else if k.d.u16_at(0) == 1 {
                major = format!("<c:majorGridlines>{}</c:majorGridlines>", sp_pr(Some(a), 1, || ln_xml(g, lf, "000000", 9525)));
            } else if k.d.u16_at(0) == 2 {
                minor = format!("<c:minorGridlines>{}</c:minorGridlines>", sp_pr(Some(a), 2, || ln_xml(g, lf, "000000", 9525)));
            }
        }
    }
    let num_fmt = match (ax.and_then(|a| a.kid(IFMT)), fmt) {
        (Some(i), _) => format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", xml_text(&ifmt_code(g, i.d.u16_at(0)))),
        (None, Some(f)) => format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"1\"/>", xml_text(f)),
        _ => String::new(),
    };
    let tr = tick.map(|t| t.d.as_slice());
    let major_t = TICK_MARK.get(usize::from(tr.map_or(2, |t| t.u8_at(0)))).copied().unwrap_or("out");
    let minor_t = TICK_MARK.get(usize::from(tr.map_or(0, |t| t.u8_at(1)))).copied().unwrap_or("none");
    let lbl_pos = TICK_LBL.get(usize::from(tr.map_or(3, |t| t.u8_at(2)))).copied().unwrap_or("nextTo");
    let mut tp = ax.and_then(|a| a.kid(FONTX)).map_or_else(|| base.clone(), |fx| font_props(g, fx.d.u16_at(0)));
    if let Some(t) = tr
        && t.u16_at(24) & 1 == 0
    {
        tp.color = icv_color(g, t.u16_at(26));
    }
    let rot = tr.map_or(0, |t| t.u16_at(28));
    let mut title = String::new();
    if let Some(tn) = title_node.filter(|t| t.d.u16_at(24) & 0x40 == 0)
        && let Some(text) = series_text(tn).filter(|t| !t.is_empty())
    {
        let p = text_node_props(g, Some(tn), &TextProps { bold: true, ..base.clone() });
        title = rich_title(&text, &p, tn.d.u16_at(30), tn.text_props().as_deref(), tn.kid(FRAME).and_then(|f| f.shape_props(0)));
    }
    let axis_sp = ax.and_then(|a| a.shape_props(0)).unwrap_or_else(|| format!("<c:spPr><a:noFill/>{axis_line}</c:spPr>"));
    let axis_tx = ax.and_then(Node::text_props).unwrap_or_else(|| tx_pr(&tp, rot));
    let mut x = format!(
        "<{tag}><c:axId val=\"{}\"/><c:scaling>{scaling}</c:scaling><c:delete val=\"{}\"/><c:axPos val=\"{pos}\"/>{major}{minor}{title}{num_fmt}<c:majorTickMark val=\"{major_t}\"/><c:minorTickMark val=\"{minor_t}\"/><c:tickLblPos val=\"{lbl_pos}\"/>{axis_sp}{axis_tx}<c:crossAx val=\"{}\"/>{crosses}",
        ids.0,
        u8::from(ax.is_none()),
        ids.1
    );
    if kind == "cat" {
        x += "<c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/>";
        if let Some(cr) = cr {
            if cr.d.u16_at(2) > 1 {
                let _ = write!(x, "<c:tickLblSkip val=\"{}\"/>", cr.d.u16_at(2));
            }
            if cr.d.u16_at(4) > 1 {
                let _ = write!(x, "<c:tickMarkSkip val=\"{}\"/>", cr.d.u16_at(4));
            }
        }
        x += "<c:noMultiLvlLbl val=\"0\"/>";
    } else {
        let _ = write!(x, "<c:crossBetween val=\"{}\"/>{units}", if xy || !between { "midCat" } else { "between" });
    }
    let _ = write!(x, "</{tag}>");
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_renames_the_element() {
        let xml = b"<?xml version=\"1.0\"?>\r\n<a:spPr xmlns:a=\"x\"><a:noFill/></a:spPr>";
        let mut d = vec![0u8; 24];
        d[20..24].copy_from_slice(&u32::try_from(xml.len()).unwrap().to_le_bytes());
        d.extend_from_slice(xml);
        assert_eq!(fragment(&d, 24, "spPr").as_deref(), Some("<c:spPr xmlns:a=\"x\"><a:noFill/></c:spPr>"));
        assert_eq!(fragment(&d, 24, "txPr"), None);
    }

    #[test]
    fn layout12_automatic() {
        assert_eq!(layout12(&[0u8; 68], 18, true), "<c:layout/>");
    }

    #[test]
    fn normalize_general() {
        assert_eq!(normalize_format("general"), "General");
        assert_eq!(normalize_format("\"general\" GENERAL"), "\"general\" General");
    }
}

#[cfg(test)]
mod tests_w5 {
    use super::*;

    fn node(t: u16, d: Vec<u8>, kids: Vec<Node>) -> Node {
        Node { t, d, conts: Vec::new(), kids }
    }

    #[test]
    fn ai_reference_to_c_f() {
        let g = Globals {
            palette: Vec::new(),
            fonts: Vec::new(),
            formats: HashMap::new(),
            xf_fmt: Vec::new(),
            sheets: vec!["Planilha1".into(), "Dados 2".into()],
            xti: vec![0, 1],
        };
        // id 1, rt 2, cce 11: PtgArea3d ixti 0, rows 1..4, cols 1..1 → Planilha1!$B$2:$B$5
        let mut d = vec![1u8, 2, 0, 0, 0, 0, 11, 0, 0x3B, 0, 0, 1, 0, 4, 0, 1, 0, 1, 0];
        assert_eq!(ai_ref(&g, &d).as_deref(), Some("Planilha1!$B$2:$B$5"));
        // PtgRef3d ixti 1 → a quoted sheet name
        d = vec![0u8, 2, 0, 0, 0, 0, 7, 0, 0x3A, 1, 0, 0, 0, 2, 0];
        assert_eq!(ai_ref(&g, &d).as_deref(), Some("'Dados 2'!$C$1"));
        // a literal (rt 1): no reference
        assert_eq!(ai_ref(&g, &[0u8, 1, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn gelframe_solid_fill_is_the_rgb() {
        // 2 properties: fillType 0 (solid), fillColor 0x02826015 (PowerPoint's accent1 of pb-graficos, COLORREF 15 60 82 + flags 2)
        let mut d = vec![0x23u8, 0x00, 0x0B, 0xF0, 12, 0, 0, 0];
        d.extend_from_slice(&[0x80, 0x01, 0, 0, 0, 0]);
        d.extend_from_slice(&[0x81, 0x01, 0x15, 0x60, 0x82, 0x02]);
        assert_eq!(gel_fill(&node(GELFRAME, d, vec![])).as_deref(), Some("<a:solidFill><a:srgbClr val=\"156082\"/></a:solidFill>"));
    }

    #[test]
    fn trendline_and_error_bar_series() {
        // SERAUXTREND: regt 0, order 2 (polynomial), intercept NaN, no equation/R², no forecasts
        let mut tr = vec![0u8, 2];
        tr.extend_from_slice(&f64::NAN.to_le_bytes());
        tr.extend_from_slice(&[0, 0]);
        tr.extend_from_slice(&0f64.to_le_bytes());
        tr.extend_from_slice(&0f64.to_le_bytes());
        let trend = node(SERIES, vec![0; 12], vec![node(SERAUXTREND, tr, vec![])]);
        // SERAUXERRBAR: y plus and y minus, percentage, end caps, 10
        let eb = |sertm: u8| {
            let mut d = vec![sertm, 1, 1, 1];
            d.extend_from_slice(&10f64.to_le_bytes());
            d.extend_from_slice(&[1, 0]);
            node(SERIES, vec![0; 12], vec![node(SERAUXERRBAR, d, vec![])])
        };
        let (plus, minus) = (eb(3), eb(4));
        let owner = node(SERIES, vec![0; 12], vec![]);
        let g = Globals {
            palette: Vec::new(),
            fonts: Vec::new(),
            formats: HashMap::new(),
            xf_fmt: Vec::new(),
            sheets: Vec::new(),
            xti: Vec::new(),
        };
        let s = SeriesData {
            name: None,
            vals: vec![],
            val_fmt: "General".into(),
            cats: None,
            cats_numeric: false,
            cat_fmt: None,
            bubbles: None,
            node: &owner,
            group: 0,
            refs: [None, None, None, None],
            aux: vec![&trend, &plus, &minus],
        };
        let x = aux_xml(&g, &s, Kind::Bar);
        assert!(x.contains("<c:trendlineType val=\"poly\"/><c:order val=\"2\"/>"), "{x}");
        assert!(!x.contains("c:intercept"), "{x}");
        assert!(
            x.contains(
                "<c:errBars><c:errBarType val=\"both\"/><c:errValType val=\"percentage\"/><c:noEndCap val=\"0\"/><c:val val=\"10\"/>"
            ),
            "{x}"
        );
    }
}
