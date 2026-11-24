//! C3c parity: the paint layers against the frozen TS (`sheet-paint` capture from `js/capture-paint.mjs`): table and pivot
//! styles by element, the format each cell gets from them, the final cell style under table + conditional format, the
//! border pass (the real `paintGridBorders` over a recording canvas), fills (`paintCellFill`), diagonals, the spill border,
//! the grid-layout helpers and `layoutRichLines`.
//! Run: `cargo test -p uo-xlsx --features grid,facet --test paint_parity`; the local big run with `UO_PAINT_BIG=1`.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::key::{Area, SheetIx};
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::paint_rules::border::{BStyle, CornerInfo, Edge, NO_CORNER, PaintRect, emit_edge, rank, winning_edge};
use uo_xlsx::grid::paint_rules::fill::{FillPaint, PaintFill, Stop, fill_paint, path_gradient_rgba, pattern_masks};
use uo_xlsx::grid::paint_rules::{
    Color, FillRef, Palette, boundary_at, frozen_extent, index_screen_pos, layout_rich_lines, px_to_col_width_chars, px_to_index,
    px_to_row_height_pt, spill_border_rect, table_handle_rects,
};
use uo_xlsx::grid::table_style::{El, Spec, TableCellFormat, resolve_table_style};
use uo_xlsx::grid::*;
use uo_xlsx::read::model::Styles;

type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

const CF_CLOCK: cf::CfClock = cf::CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };
const FILL_CFG: (f64, f64, f64) = (1.1, 29.25, 13.5);
const CFG: [(f64, f64, f64); 2] = [(1.0, 0.0, 0.0), (1.75, 41.5, 17.25)];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn samples_dir() -> PathBuf {
    match std::env::var_os("UO_SHEET_SAMPLES") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sheet/samples"),
    }
}

fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap();
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap();
    String::from_utf8(raw).unwrap()
}

fn sidecar(path: &Path) -> Side {
    let mut out: Side = BTreeMap::new();
    for line in gunzip(path).lines() {
        let mut it = line.splitn(3, '\t');
        if let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next()) {
            out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
        }
    }
    out
}

fn dig(text: &str) -> String {
    let h = uo_testkit::sha256::sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_testkit::stream::hex(&h)[..32])
}

fn list_dig(lines: &[String]) -> String {
    dig(&lines.join("\n"))
}

/// `fh`: the IEEE bits as 16 hex digits, `nan`.
fn fh(x: f64) -> String {
    if x.is_nan() { "nan".to_owned() } else { format!("{:016x}", x.to_bits()) }
}

fn js(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn tri(v: Option<bool>) -> &'static str {
    match v {
        None => "-",
        Some(true) => "1",
        Some(false) => "0",
    }
}

fn col_letters(col: u32) -> String {
    let mut s = Vec::new();
    let mut c = col;
    while c > 0 {
        s.push(b'A' + ((c - 1) % 26) as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

fn ref_of(c: u32, r: u32) -> String {
    format!("{}{r}", col_letters(c))
}

/// `JSON.stringify` of a string given as UTF-16 units.
fn json_units(u: &[u16]) -> String {
    let mut o = String::from("\"");
    let mut i = 0;
    while i < u.len() {
        let x = u[i];
        match x {
            0x22 => o.push_str("\\\""),
            0x5c => o.push_str("\\\\"),
            0x0a => o.push_str("\\n"),
            0x0d => o.push_str("\\r"),
            0x09 => o.push_str("\\t"),
            0x08 => o.push_str("\\b"),
            0x0c => o.push_str("\\f"),
            x if x < 0x20 => o.push_str(&format!("\\u{x:04x}")),
            0xD800..=0xDBFF if u.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l)) => {
                o.push(char::decode_utf16([x, u[i + 1]]).next().unwrap().unwrap());
                i += 1;
            }
            0xD800..=0xDFFF => o.push_str(&format!("\\u{x:04x}")),
            x => o.push(char::from_u32(u32::from(x)).unwrap()),
        }
        i += 1;
    }
    o.push('"');
    o
}

fn json_str(s: &str) -> String {
    json_units(&s.encode_utf16().collect::<Vec<_>>())
}

// ---------------------------------------------------------------- canonical texts

struct Canon<'a> {
    book: &'a PaintBook,
}

impl Canon<'_> {
    fn color(&self, c: Color) -> String {
        self.book.palette().text(c)
    }

    fn opt_color(&self, c: Option<Color>) -> String {
        c.map_or_else(|| "-".to_owned(), |c| self.color(c))
    }

    fn stops(&self, s: &[Stop]) -> String {
        s.iter().map(|s| format!("{}={}", fh(s.pos), self.color(s.color))).collect::<Vec<_>>().join(",")
    }

    fn fill(&self, f: FillRef) -> String {
        match f {
            FillRef::None => "-".to_owned(),
            FillRef::Solid(c) => format!("S:{}", self.color(c)),
            FillRef::Id(i) => match self.book.arena().get(i as usize) {
                Some(PaintFill::Pattern { rows, fg, bg }) => {
                    let name = rows.and_then(|r| pattern_masks().iter().find(|(_, m)| *m == r).map(|(n, _)| *n));
                    format!("{}:{}:{}", name.map_or_else(|| "P?".to_owned(), |n| format!("P:{n}")), self.color(*fg), self.color(*bg))
                }
                Some(PaintFill::Linear { degree, stops }) => format!("GL:{}:{}", fh(*degree), self.stops(stops)),
                Some(PaintFill::Path { left, right, top, bottom, stops }) => {
                    format!("GP:{}:{}", [left, right, top, bottom].iter().map(|x| fh(**x)).collect::<Vec<_>>().join(","), self.stops(stops))
                }
                None => "?".to_owned(),
            },
        }
    }

    fn fill_obj(&self, f: Option<FillRef>) -> String {
        match f {
            None => "u".to_owned(),
            Some(FillRef::None) => "0".to_owned(),
            Some(x) => self.fill(x),
        }
    }

    fn edge(&self, e: Edge) -> String {
        if e.is_some() { format!("{}:{}", e.style.name(), self.color(e.color)) } else { "-".to_owned() }
    }

    fn edges(&self, e: &[Edge]) -> String {
        e.iter().map(|e| self.edge(*e)).collect::<Vec<_>>().join("|")
    }

    fn font(&self, f: &FontPaint) -> String {
        format!("{}|{}{}{}{}", self.opt_color(f.color), u8::from(f.bold), u8::from(f.italic), u8::from(f.underline), u8::from(f.strike))
    }

    fn tf(&self, t: &TableCellFormat) -> String {
        format!(
            "F={};C={};f={}{}{}{};B={}",
            self.fill_obj(t.fill),
            self.opt_color(t.font_color),
            tri(t.bold),
            tri(t.italic),
            tri(t.underline),
            tri(t.strike),
            self.edges(&t.border)
        )
    }

    fn spec(&self, s: Option<&Spec>) -> String {
        const KNOWN: [&str; 25] = [
            "wholeTable",
            "firstColumnStripe",
            "secondColumnStripe",
            "firstRowStripe",
            "secondRowStripe",
            "lastColumn",
            "firstColumn",
            "headerRow",
            "totalRow",
            "firstHeaderCell",
            "lastHeaderCell",
            "firstTotalCell",
            "lastTotalCell",
            "firstSubtotalColumn",
            "secondSubtotalColumn",
            "thirdSubtotalColumn",
            "firstSubtotalRow",
            "secondSubtotalRow",
            "thirdSubtotalRow",
            "firstRowSubheading",
            "secondRowSubheading",
            "thirdRowSubheading",
            "blankRow",
            "pageFieldLabels",
            "pageFieldValues",
        ];
        let Some(s) = s else { return "null".to_owned() };
        KNOWN
            .iter()
            .filter_map(|k| {
                let f = s.el(El::parse(k)?)?;
                Some(format!(
                    "{k}{{F={};C={};f={}{}{}{};sz={};B={}}}",
                    self.fill_obj(f.fill),
                    self.opt_color(f.font_color),
                    tri(f.bold),
                    tri(f.italic),
                    tri(f.underline),
                    tri(f.strike),
                    f.size.map_or_else(|| "-".to_owned(), fh),
                    self.edges(&f.border)
                ))
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_ops(&self, f: FillRef, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64) -> String {
        let r = format!("{} {} {} {}", fh(x), fh(y), fh(w), fh(h));
        match self.book.fill_paint(f, x, y, w, h, ox, oy) {
            FillPaint::Nothing => String::new(),
            FillPaint::Solid(c) => format!("FR {} {r}", self.color(c)),
            FillPaint::Pattern { rows, fg, bg, phase } => format!(
                "FR PAT({}/{}/{}/{}) {r}",
                self.color(bg),
                self.color(fg),
                rows.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                phase.map_or_else(|| "-".to_owned(), |(a, b)| format!("{},{}", fh(a), fh(b)))
            ),
            FillPaint::Linear { p0, p1, stops } => format!(
                "FR L({},{},{},{};{}) {r}",
                fh(p0.0),
                fh(p0.1),
                fh(p1.0),
                fh(p1.1),
                stops
                    .iter()
                    .map(|s| format!("{}={}", fh(uo_xlsx::grid::paint_rules::fill::clamp_stop(s.pos)), self.color(s.color)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            FillPaint::Path { inner, stops, w: pw, h: ph } => {
                let mut px = Vec::new();
                path_gradient_rgba(inner, stops, pw, ph, &mut px);
                let d = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(&px));
                format!("DI {} {r}", &d[..16])
            }
        }
    }
}

// ---------------------------------------------------------------- run plumbing

struct Run {
    rep: Replay<'static>,
    cap: &'static Capture,
    side: Side,
    lines: u64,
    lines_equal: u64,
    /// The sheet being compared has a cell at a position named by another `r`: differences are accepted and counted apart.
    odd: bool,
    odd_lines: u64,
    odd_equal: u64,
}

const ODD: &str = "a cell at a position named by another ref (the TS model follows the name, the store the file; C3a-9)";

impl Run {
    fn open(cap: &Path, side: &Path) -> Run {
        let cap: &'static Capture = Box::leak(Box::new(Capture::read(cap).unwrap()));
        Run { rep: Replay::new(cap), cap, side: sidecar(side), lines: 0, lines_equal: 0, odd: false, odd_lines: 0, odd_equal: 0 }
    }

    fn count(&mut self, total: u64, equal: u64) {
        if self.odd {
            self.odd_lines += total;
            self.odd_equal += equal;
        } else {
            self.lines += total;
            self.lines_equal += equal;
        }
    }

    fn check(&mut self, key: &str, facet: &str, digest: &str) -> bool {
        if self.odd { self.rep.accept(key, facet, digest, ODD) } else { self.rep.check(key, facet, digest) }
    }

    /// A facet with sidecar lines: the digest and the lines against the TS's.
    fn facet(&mut self, key: &str, facet: &str, lines: &[String]) {
        let want = self.side.get(key).and_then(|m| m.get(facet)).cloned().unwrap_or_default();
        let mut diff = lines.len().abs_diff(want.len()) as u64;
        let mut shown = 0;
        for (k, (a, w)) in lines.iter().zip(want.iter()).enumerate() {
            if a != w {
                diff += 1;
                if shown < 5 && !self.odd {
                    shown += 1;
                    eprintln!(
                        "{key} {facet} line {k}:
  ts:   {w}
  rust: {a}"
                    );
                }
            }
        }
        let total = lines.len().max(want.len()) as u64;
        self.count(total, total.saturating_sub(diff));
        self.check(key, facet, &list_dig(lines));
    }

    /// A digest-only facet (the capture keeps the lines with `--lines`).
    fn digest(&mut self, key: &str, facet: &str, lines: &[String]) {
        let ok = self.check(key, facet, &list_dig(lines));
        let n = lines.len() as u64;
        self.count(n, if ok { n } else { 0 });
        if !ok && !self.odd && std::env::var_os("UO_PAINT_DUMP").is_some() {
            let p = std::env::temp_dir().join(format!("paint-{}-{facet}.txt", key.replace('/', "_")));
            std::fs::write(
                &p,
                lines.join(
                    "
",
                ),
            )
            .unwrap();
            eprintln!("{key} {facet}: lines written to {}", p.display());
        }
    }
}

#[derive(Default)]
struct Totals {
    workbooks: u64,
    sheets: u64,
    layer_sheets: u64,
    cells: u64,
    displays: u64,
    /// Sheets skipped: a cell sits at a position named by another `r` (the TS model follows the name, the store the file).
    odd_ref_sheets: u64,
}

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/paint1/") {
        dir().join(format!("workbooks/paint1/paint1-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

/// The fake canvas of the harness (6 px per UTF-16 unit, the `'Hg'` box 9/3).
struct Fake;

impl Measurer for Fake {
    fn advance(&self, _f: &FontDesc, cp: u32) -> i32 {
        if cp > 0xFFFF { 12 } else { 6 }
    }
    fn text_width(&self, _f: &FontDesc, s: &[u16]) -> i32 {
        6 * i32::try_from(s.len()).unwrap()
    }
    fn font_box(&self, _f: &FontDesc) -> (Option<f64>, Option<f64>) {
        (Some(9.0), Some(3.0))
    }
}

fn open_grid(book: &mut Book, si: usize, styles: &StyleTable, table: &mut MeasureTable) -> SheetGrid {
    let mut grid = SheetGrid::open(book, si, styles);
    for _ in 0..5 {
        let ctx = CellCtx {
            date1904: book.workbook.date1904,
            show_zeros: !book.sheets[si].as_ref().unwrap().show_zeros_off,
            lang: FormulaLang::PtBr,
            locale: None,
        };
        let mut env = FitEnv { fonts: &mut *table, env: &IdentityEnv, ctx };
        let f = fit_row_heights(book, si, styles, &grid, &mut env);
        if !f.needs_measure {
            grid.set_row_fit(f);
            return grid;
        }
        table.satisfy(&Fake);
    }
    panic!("fit settles");
}

fn replay_workbooks(r: &mut Run, big: bool, t: &mut Totals, bytes: &dyn Fn(&str) -> Option<Vec<u8>>) {
    let keys: Vec<String> =
        r.cap.records().iter().map(|rec| rec.key.clone()).filter(|k| k.starts_with("gen/") || k.starts_with("samples/")).collect();
    for key in keys {
        if r.cap.get(&key, "open") != Some("ok") {
            continue;
        }
        let Some(raw) = bytes(&key) else { continue };
        t.workbooks += 1;
        r.rep.skip(&key, "open");
        let mut book = Book::open(&raw, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let styles = StyleTable::build(&book.styles);
        let mut mt = MeasureTable::new();
        let sheets: Vec<usize> = (0..book.workbook.sheets.len()).filter(|&i| book.workbook.sheets[i].target.is_some()).collect();
        for (vi, &si) in sheets.iter().enumerate() {
            t.sheets += 1;
            let grid = open_grid(&mut book, si, &styles, &mut mt);
            sheet_facets(r, &mut book, &key, vi, si, &styles, &grid, &mut mt, t, big);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sheet_facets(
    r: &mut Run,
    book: &mut Book,
    key: &str,
    vi: usize,
    si: usize,
    styles: &StyleTable,
    grid: &SheetGrid,
    mt: &mut MeasureTable,
    t: &mut Totals,
    _big: bool,
) {
    let region = r.cap.get(key, &format!("b{vi}")).unwrap_or("8 8").to_owned();
    r.rep.skip(key, &format!("b{vi}"));
    let (c2, r2): (u32, u32) = {
        let mut it = region.split(' ').map(|x| x.parse::<u32>().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    let ix0 = SheetIx(u32::try_from(si).unwrap());
    let mut odd = false;
    book.store.each(Area { sheet: ix0, r1: 0, c1: 0, r2: 1_048_575, c2: 16_383 }, |row, col, cell| {
        use uo_calc::store::CellKind as LaneKind;
        if !odd
            && cell.kind.has(LaneKind::HAS_EXTRA)
            && let Some(rr) = book.store.extra(ix0, row, col).and_then(|e| e.raw_ref.as_ref())
            && rr.as_js().to_string_lossy() != ref_of(col + 1, row + 1)
        {
            odd = true;
        }
    });
    r.odd = odd;
    if odd {
        t.odd_ref_sheets += 1;
    }
    let ws = book.sheets[si].as_ref().unwrap().clone();
    let has_cf = !ws.cond_formats.is_empty();
    let mut eng = if has_cf { cf::CfEngine::for_book(book, si, CF_CLOCK, cf::CfLimits::default()) } else { None };
    if let Some(e) = &eng {
        cf::prepare_book(book, e).unwrap();
    }
    let mut pbook = PaintBook::build(&book.styles);
    let cfp = eng.as_ref().map(|e| pbook.attach_cf(e));
    let sheet = SheetPaint::build(&mut pbook, &ws, &book.styles, &book.cx);
    if !sheet.is_empty() || has_cf {
        t.layer_sheets += 1;
    }
    let ix = SheetIx(u32::try_from(si).unwrap());
    let cn = Canon { book: &pbook };

    // the styles used
    let mut names: Vec<String> = ws
        .tables
        .iter()
        .filter_map(|x| x.style_name.clone())
        .chain(ws.pivots.iter().filter_map(|p| p.style_name.clone()))
        .filter(|n| !n.is_empty())
        .collect();
    names.sort();
    names.dedup();
    let sp: Vec<String> = names
        .iter()
        .map(|n| {
            let (mut pal, mut arena) = (Palette::default(), Vec::new());
            let spec = resolve_table_style(Some(n), &book.styles, &book.cx, &mut pal, &mut arena);
            // the spec's colours live in a private palette: print through a book built on it
            let tmp = spec_canon(spec.as_ref(), &pal, &arena);
            format!("{}\t{tmp}", json_str(n))
        })
        .collect();
    r.facet(key, &format!("sp{vi}"), &sp);

    // per cell: table layer, filters, final style
    let mut frame = PaintFrame::new();
    {
        let inp = PaintInputs { book: &pbook, sheet: &sheet, styles, store: &book.store, ix };
        let cfa = match (eng.as_mut(), cfp.as_ref()) {
            (Some(e), Some(p)) => Some((e, p)),
            _ => None,
        };
        frame.resolve(&inp, cfa, 1, c2, 1, r2);
    }
    let (mut tl, mut fl, mut sl, mut pl, mut gl) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (zf, sxf, syf) = FILL_CFG;
    let (xs, ys) = (grid.col_geometry(zf), grid.row_geometry(zf));
    let inp = PaintInputs { book: &pbook, sheet: &sheet, styles, store: &book.store, ix };
    for row in 1..=r2 {
        for col in 1..=c2 {
            let at = ref_of(col, row);
            let tf = sheet.format_at(col, row);
            if let Some(f) = &tf {
                tl.push(format!("{at}\t{}", cn.tf(f)));
            }
            let fa = frame.filter_at(&inp, col, row);
            let end = sheet.table_end_at(col, row);
            if fa.is_some() || end {
                fl.push(format!("{at}\t{};{}", fa.map_or("-", |a| if a { "1" } else { "0" }), u8::from(end)));
            }
            let Some(cp) = frame.cell(col, row) else { continue };
            let diag = diag_desc(&cn, &pbook, cp);
            let any = cp.filled
                || cp.border.iter().chain(cp.table_border.iter()).any(|e| e.is_some())
                || diag != "-"
                || tf.is_some()
                || cp.has_cf;
            if any {
                sl.push(format!(
                    "{at}\tF={};N={};B={};T={};D={diag};K={}{}",
                    cn.fill(cp.fill),
                    cn.font(&cp.font),
                    cn.edges(&cp.border),
                    cn.edges(&cp.table_border),
                    u8::from(tf.is_some()),
                    u8::from(cp.has_cf)
                ));
            }
            // a Float64Array read past its end is `undefined` (NaN in arithmetic): cells beyond maxCol/maxRow
            let e = |g: &Geometry, i: u32| g.edge(i).unwrap_or(f64::NAN);
            let (left, top) = (e(&xs, col - 1) - sxf, e(&ys, row - 1) - syf);
            let (w, h) = (e(&xs, col) - e(&xs, col - 1), e(&ys, row) - e(&ys, row - 1));
            if cp.filled {
                pl.push(format!("{at}\t{}", cn.fill_ops(cp.fill, left, top, w + 1.0, h + 1.0, -sxf, -syf)));
            }
            if diag != "-"
                && let Some(b) = pbook.border(cp.border_ix)
                && let Some(d) = b.diagonal_strokes(left, top, w, h)
            {
                let head = format!(
                    "ST {} {} [{}] {}",
                    cn.color(d.color),
                    fh(d.line_width),
                    d.dash.iter().map(|x| fh(*x)).collect::<Vec<_>>().join(","),
                    d.clip.iter().map(|x| fh(*x)).collect::<Vec<_>>().join(",")
                );
                let lines = d.lines[..d.n]
                    .iter()
                    .map(|l| format!("L {}", l.iter().map(|x| fh(*x)).collect::<Vec<_>>().join(",")))
                    .collect::<Vec<_>>()
                    .join(" ");
                gl.push(format!("{at}\t{head} {lines}"));
            }
        }
    }
    r.facet(key, &format!("t{vi}"), &tl);
    r.facet(key, &format!("f{vi}"), &fl);
    r.facet(key, &format!("s{vi}"), &sl);
    r.facet(key, &format!("p{vi}"), &pl);
    r.facet(key, &format!("dg{vi}"), &gl);
    t.cells += u64::from(c2) * u64::from(r2);

    // the border pass
    for (k, &(z, sx, sy)) in CFG.iter().enumerate() {
        let (xs, ys) = (grid.col_geometry(z), grid.row_geometry(z));
        let (mut ex, mut ey) = (PxEdges::default(), PxEdges::default());
        ex.fill(&xs, 1, c2 + 2, sx);
        ey.fill(&ys, 1, r2 + 2, sy);
        let (px, py) = (|c: u32| ex.at(c), |rr: u32| ey.at(rr));
        let (ax, ay) = (uo_jsnum::conv::to_int32(uo_jsnum::math::round(sx)), uo_jsnum::conv::to_int32(uo_jsnum::math::round(sy)));
        let mut out = Vec::new();
        frame.border_segments(&grid.merges, !ws.show_grid_lines_off, 1, c2, 1, r2, &px, &py, ax, ay, &mut |seg| {
            seg.rects(None, &mut |br| {
                out.push(format!("FR {} {} {} {} {}", cn.color(br.color), br.rect.x, br.rect.y, br.rect.w, br.rect.h))
            });
        });
        r.digest(key, &format!("bd{vi}@{k}"), &out);
    }

    // the displayed text under the layers (the generated groups: clean row-major sheets)
    if key.contains("paint") {
        let lines = display_lines(book, si, grid, styles, mt, &sheet, eng.as_mut(), ix);
        t.displays += lines.len() as u64;
        r.digest(key, &format!("d{vi}"), &lines);
    }
}

fn diag_desc(cn: &Canon<'_>, book: &PaintBook, cp: &CellPaint) -> String {
    match book.border(cp.border_ix) {
        Some(b) if b.diagonal.is_some() && (b.up || b.down) => format!("{}:{}{}", cn.edge(b.diagonal), u8::from(b.up), u8::from(b.down)),
        _ => "-".to_owned(),
    }
}

/// A spec printed against the palette and arena it was resolved with.
fn spec_canon(spec: Option<&Spec>, pal: &Palette, arena: &[PaintFill]) -> String {
    let b = PaintBook::with_tables(pal.clone(), arena.to_vec());
    Canon { book: &b }.spec(spec)
}

#[allow(clippy::too_many_arguments)]
fn display_lines(
    book: &Book,
    si: usize,
    grid: &SheetGrid,
    styles: &StyleTable,
    table: &mut MeasureTable,
    sheet: &SheetPaint,
    mut eng: Option<&mut cf::CfEngine>,
    ix: SheetIx,
) -> Vec<String> {
    use uo_calc::store::CellKind as LaneKind;
    let ws = book.sheets[si].as_ref().unwrap();
    let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros: !ws.show_zeros_off, lang: FormulaLang::PtBr, locale: None };
    let view = CellView { store: &book.store, fonts: &book.styles.fonts, styles, ctx };
    let mut cells: Vec<(u32, u32, uo_calc::store::RawCell)> = Vec::new();
    book.store.each(Area { sheet: ix, r1: 0, c1: 0, r2: 1_048_575, c2: 16_383 }, |row, col, cell| cells.push((row, col, cell)));
    let mut scratch: Vec<u16> = Vec::new();
    let mut out = CellText::default();
    let mut lines = Vec::new();
    for (row, col, cell) in cells {
        let _ = LaneKind::HAS_EXTRA;
        let mm = grid.merges.merge_at(col + 1, row + 1);
        let (c0, c1) = match mm.filter(|m| m.is_origin) {
            Some(m) => (m.first_col, m.last_col),
            None => (col + 1, col + 1),
        };
        let mut w = 0.0;
        for k in c0..=c1 {
            if grid.hidden_cols.get(k).is_none() {
                w += grid.col_widths.get(k).unwrap_or(grid.default_col_w);
            }
        }
        let tf = sheet.format_at(col + 1, row + 1);
        let layer = eng.as_deref_mut().and_then(|e| e.at(&book.store, row, col));
        let text = loop {
            let cfp = match (eng.as_deref(), layer.as_ref()) {
                (Some(e), Some(l)) => Some((e, l)),
                _ => None,
            };
            let laid = layout_cell_layers(&view, table, &IdentityEnv, ix, row, col, cell, w, 1.0, &mut scratch, &mut out, tf.as_ref(), cfp);
            match laid {
                Laid::Empty => break String::new(),
                Laid::NeedSst(_) => panic!("shared string not decoded"),
                Laid::Done(_) => {
                    if table.has_needs() {
                        table.satisfy(&Fake);
                        continue;
                    }
                    break String::from_utf16_lossy(&out.text);
                }
            }
        };
        lines.push(json_str(&format!("{}\t{text}", ref_of(col + 1, row + 1))));
    }
    lines
}

#[test]
fn ci_paint() {
    let mut r = Run::open(&dir().join("sheet-paint.capture"), &dir().join("sheet-paint.side.gz"));
    let mut t = Totals::default();
    replay_workbooks(&mut r, false, &mut t, &bytes_of);
    misc(&mut r);
    let lines = (r.lines, r.lines_equal);
    let odd = (r.odd_lines, r.odd_equal);
    let rep = r.rep.finish();
    eprintln!(
        "paint: {} workbooks, {} sheets ({} with a table/pivot/CF layer; {} with a cell at a position named by another ref: {} lines, {} equal), {} cells resolved, {} displays; {} lines compared, {} equal; {}",
        t.workbooks,
        t.sheets,
        t.layer_sheets,
        t.odd_ref_sheets,
        odd.0,
        odd.1,
        t.cells,
        t.displays,
        lines.0,
        lines.1,
        rep.summary()
    );
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
    assert_eq!(lines.0, lines.1, "sidecar lines differ");
    assert!(t.workbooks >= 49 && t.layer_sheets >= 20);
}

/// The local run (`UO_PAINT_BIG=1`; the capture is made by `capture-paint.mjs --big` into `core/target/sheet-cap`).
#[test]
fn big_paint() {
    if !uo_testkit::opt_in("UO_PAINT_BIG", "the paint-big + x1-big parity (local capture files)") {
        return;
    }
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap");
    let mut r = Run::open(&base.join("paint-big.capture"), &base.join("paint-big.side.gz"));
    let bytes = |key: &str| {
        if let Some(n) = key.strip_prefix("gen/paint-big/") {
            std::fs::read(base.join(format!("workbooks/paint-big/paint-big-{n}.xlsx"))).ok()
        } else {
            key.strip_prefix("gen/x1-big/").and_then(|n| std::fs::read(base.join(format!("workbooks/x1-big/x1-big-{n}.xlsx"))).ok())
        }
    };
    let mut t = Totals::default();
    replay_workbooks(&mut r, true, &mut t, &bytes);
    misc(&mut r);
    let lines = (r.lines, r.lines_equal);
    let odd = (r.odd_lines, r.odd_equal);
    let rep = r.rep.finish();
    eprintln!(
        "paint big: {} workbooks, {} sheets ({} with a layer; {} with a cell at a position named by another ref: {} lines, {} equal), {} cells resolved, {} displays; {} lines compared, {} equal; {}",
        t.workbooks,
        t.sheets,
        t.layer_sheets,
        t.odd_ref_sheets,
        odd.0,
        odd.1,
        t.cells,
        t.displays,
        lines.0,
        lines.1,
        rep.summary()
    );
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
    assert_eq!(lines.0, lines.1);
}

// ---------------------------------------------------------------- the pure helpers (`misc/*`)

fn corners() -> [CornerInfo; 4] {
    [
        NO_CORNER,
        CornerInfo { before: 1, after: 1, double_before: false, double_after: false },
        CornerInfo { before: 0, after: 1, double_before: true, double_after: true },
        CornerInfo { before: 1, after: 0, double_before: true, double_after: false },
    ]
}

fn misc(r: &mut Run) {
    r.odd = false;
    // masks
    let masks: Vec<String> =
        pattern_masks().iter().map(|(n, m)| format!("{n} {}", m.iter().map(|b| format!("{b:02x}")).collect::<String>())).collect();
    r.facet("misc", "masks", &masks);
    // edges
    let sts: Vec<Option<&str>> =
        std::iter::once(None).chain(BSTYLES.iter().map(|s| Some(*s))).chain(std::iter::once(Some("zigzag"))).collect();
    let bs = |s: Option<&str>| s.map_or(BStyle::None, BStyle::parse);
    let mut lines = Vec::new();
    for s in &sts {
        let e = uo_xlsx::grid::paint_rules::edge_extent(bs(*s));
        lines.push(format!("ext {} {},{} {}", s.unwrap_or("-"), e[0], e[1], rank(bs(*s))));
    }
    for p in &sts {
        for q in &sts {
            let (a, b) = (Edge::new(bs(*p), Color(0x11)), Edge::new(bs(*q), Color(0x22)));
            let w = winning_edge(a, b);
            lines.push(format!(
                "win {} {} {}",
                p.unwrap_or("-"),
                q.unwrap_or("-"),
                if !w.is_some() {
                    "-"
                } else if w.color == a.color {
                    "a"
                } else {
                    "b"
                }
            ));
        }
    }
    r.facet("misc", "edges", &lines);
    // runs
    let mut ops = Vec::new();
    let styles: Vec<&str> = BSTYLES.iter().copied().chain(std::iter::once("zigzag")).collect();
    for s in &styles {
        for (ci, c0) in corners().iter().enumerate() {
            for anchor in [0, 7, -5, 1_000_003] {
                for erase in [false, true] {
                    for horiz in [true, false] {
                        let c1 = corners()[(ci + 1) % 4];
                        let e = Edge::new(BStyle::parse(s), Color(0x00ab_12cd));
                        let mut out: Vec<String> = Vec::new();
                        emit_edge(horiz, e, 10, 70, 20, *c0, c1, anchor, erase, None, &mut |p: PaintRect| {
                            out.push(format!("FR {} {} {} {} {}", if p.erase { "#ffffff" } else { "#ab12cd" }, p.x, p.y, p.w, p.h));
                        });
                        ops.push(format!("{s} {ci} {anchor} {} {}\t{}", u8::from(erase), if horiz { "h" } else { "v" }, out.join(" | ")));
                    }
                }
            }
        }
    }
    r.facet("misc", "hv", &ops);
    // grid layout
    let runs = [(1u32, 5u32, 30.0f64), (7, 7, 0.0), (10, 40, 17.5), (55, 60, 0.0)];
    let mut gl = Vec::new();
    for zoom in [1.0, 1.75] {
        let xs = Geometry::build(80, 64.0, zoom, &runs);
        for frozen in [0u32, 2, 9] {
            for scroll in [0.0, 123.5, 700.0] {
                gl.push(format!("ext {} {frozen} {}", js(zoom), fh(frozen_extent(&xs, frozen))));
                for px in [-1.0, 0.0, 0.5, 29.0, 30.0, 100.0, 127.9, 128.0, 129.0, 400.0, 1000.0, 5000.0, 1e6, f64::NAN] {
                    let n = |o: Option<u32>| o.map_or_else(|| "null".to_owned(), |v| v.to_string());
                    gl.push(format!(
                        "idx {} {frozen} {} {} {} {} {}",
                        js(zoom),
                        js(scroll),
                        fh(px),
                        n(px_to_index(&xs, px, frozen, scroll, 80)),
                        n(boundary_at(&xs, px, frozen, scroll, 80, 3.0)),
                        n(boundary_at(&xs, px, frozen, scroll, 80, 0.5))
                    ));
                }
                for k in [0u32, 1, 2, 3, 10, 60, 81, 82, 99] {
                    gl.push(format!("pos {} {frozen} {} {k} {}", js(zoom), js(scroll), fh(index_screen_pos(&xs, k, frozen, scroll))));
                }
            }
        }
        for bi in [0, 1, 2, 5, 8] {
            gl.push(format!(
                "bis {} {bi} {}",
                js(zoom),
                [0.0, 29.9, 30.0, 150.0, 9999.0].iter().map(|x| xs.bisect_le(*x).to_string()).collect::<Vec<_>>().join(",")
            ));
        }
    }
    for mdw in [7.0, 8.0, 9.0, 6.0] {
        for px in (0..=300).step_by(7) {
            let px = f64::from(px);
            gl.push(format!("cw {} {} {} {}", js(mdw), js(px), fh(px_to_col_width_chars(px, mdw)), fh(px_to_row_height_pt(px))));
        }
    }
    r.facet("misc", "layout", &gl);
    // spill and handle
    let mut sl = Vec::new();
    for [x1, y1, x2, y2] in [
        [10.4, 20.6, 50.2, 40.5],
        [10.0, 10.0, 10.0, 20.0],
        [0.0, 0.0, 1.0, 1.0],
        [5.5, 5.5, 6.5, 6.5],
        [-3.5, 2.5, 99.5, 9.5],
        [10.0, 10.0, 5.0, 5.0],
    ] {
        let (call, color) = match spill_border_rect(x1, y1, x2, y2) {
            Some(q) => (q.iter().map(|x| fh(*x)).collect::<Vec<_>>().join(","), "#4472c4"),
            None => ("-".to_owned(), "-"),
        };
        sl.push(format!("spill {} {call} {color}", [x1, y1, x2, y2].iter().map(|x| fh(*x)).collect::<Vec<_>>().join(",")));
    }
    for [rx, by] in [[100.4, 50.5], [0.0, 0.0], [-1.5, 2.5], [33.5, 17.49]] {
        let q = table_handle_rects(rx, by);
        sl.push(format!(
            "handle {} {} {}",
            fh(rx),
            fh(by),
            q.iter().map(|b| format!("FR #485db1 {} {} {} {}", js(b[0]), js(b[1]), js(b[2]), js(b[3]))).collect::<Vec<_>>().join(" | ")
        ));
    }
    r.facet("misc", "spill", &sl);
    // rich lines
    let samples: [&[&str]; 9] = [
        &["aaa bbb ccc", "ddd eee"],
        &["a\nb", " c d "],
        &["", ""],
        &["abcdefghij"],
        &["  x  y  "],
        &["ab\n\ncd"],
        &["\u{1F600}\u{1F600}\u{1F600} x"],
        &[""], // replaced below: the lone surrogate sample is built from units
        &["a b", "c d", "e f g"],
    ];
    let mut rl = Vec::new();
    for (si, t) in samples.iter().enumerate() {
        let runs: Vec<Vec<u16>> = if si == 7 {
            vec!["lone".encode_utf16().chain([0xD800]).chain("surrogate word".encode_utf16()).collect()]
        } else {
            t.iter().map(|s| s.encode_utf16().collect()).collect()
        };
        for mw in [0.0, 10.0, 25.0, 61.0, 120.0, 1000.0, f64::NAN] {
            let refs: Vec<&[u16]> = runs.iter().map(Vec::as_slice).collect();
            let lines = layout_rich_lines(&refs, mw, &mut |s, run| s.len() as f64 * (6.0 + run as f64));
            let arr = format!("[{}]", runs.iter().map(|u| json_units(u)).collect::<Vec<_>>().join(","));
            let body = lines
                .iter()
                .map(|l| l.iter().map(|sg| format!("{}#{}", json_units(&sg.text), sg.run)).collect::<Vec<_>>().join("+"))
                .collect::<Vec<_>>()
                .join(" / ");
            rl.push(format!("{arr} {}\t{body}", fh(mw)));
        }
    }
    r.facet("misc", "rich", &rl);
    // the built-in styles under three themes
    let mut specs = Vec::new();
    for th in 0..3 {
        let raw = std::fs::read(dir().join(format!("workbooks/paint1/paint1-{th:04}.xlsx"))).unwrap();
        let book = Book::open(&raw, ReadLimits::default()).unwrap();
        let st: &Styles = &book.styles;
        let names: Vec<String> = (1..=21)
            .map(|i| format!("TableStyleLight{i}"))
            .chain((1..=28).map(|i| format!("TableStyleMedium{i}")))
            .chain((1..=11).map(|i| format!("TableStyleDark{i}")))
            .chain(["Light", "Medium", "Dark"].iter().flat_map(|f| (1..=28).map(move |i| format!("PivotStyle{f}{i}"))))
            .collect();
        for n in &names {
            let (mut pal, mut arena) = (Palette::default(), Vec::new());
            let spec = resolve_table_style(Some(n), st, &book.cx, &mut pal, &mut arena);
            specs.push(format!("{th}\t{n}\t{}", spec_canon(spec.as_ref(), &pal, &arena)));
        }
        for n in ["", "constructor", "toString", "NoSuchStyle", "tablestylemedium2"] {
            let (mut pal, mut arena) = (Palette::default(), Vec::new());
            let spec = resolve_table_style(Some(n), st, &book.cx, &mut pal, &mut arena);
            specs.push(format!("{th}\t{}\t{}", json_str(n), spec_canon(spec.as_ref(), &pal, &arena)));
        }
    }
    r.facet("misc", "specs", &specs);
    // keep the unused helpers honest
    let _ = fill_paint(&[], FillRef::None, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0);
}

const BSTYLES: [&str; 13] = [
    "hair",
    "dashDotDot",
    "dashDot",
    "dotted",
    "dashed",
    "thin",
    "mediumDashDotDot",
    "slantDashDot",
    "mediumDashDot",
    "mediumDashed",
    "medium",
    "thick",
    "double",
];
