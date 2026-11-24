//! C3b parity: the conditional formats against the frozen TS (`sheet-cf` capture from `js/capture-cf.mjs`): per
//! workbook and sheet the `CfResult` of every cell inside a CF range (the 30 `x1` workbooks, the 40 generated `cf1`
//! ones and the samples), `evalFormula` over per-parameter traps and seeded formulas, the icon and data-bar canvas
//! calls. Run: `cargo test -p uo-xlsx --features grid,facet --test cf_parity` (the local 700-workbook run with
//! `UO_CF_BIG=1`, release).
#![cfg(all(feature = "grid", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::key::SheetIx;
use uo_calc::strings::JsStr;
use uo_testkit::{Capture, Replay};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::cf::formula::{CfCells, FCtx, SheetNames, SheetSel, eval_formula, parse_formula};
use uo_xlsx::grid::cf::value::FScalar;
use uo_xlsx::grid::cf::*;
use uo_xlsx::read::facet::{Obj, fill, jnum, js_json};

type Side = BTreeMap<String, BTreeMap<String, Vec<String>>>;

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
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 31).unwrap();
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

/// The capture's clock: 2025-01-15 12:00 (TZ = UTC).
const CLOCK: CfClock = CfClock { year: 2025.0, month: 1.0, day: 15.0, seconds: 43_200.0 };

struct Run {
    rep: Replay<'static>,
    cap: &'static Capture,
    side: Side,
    compared: u64,
    equal: u64,
}

impl Run {
    fn open(cap: &Path, side: &Path) -> Run {
        let cap: &'static Capture = Box::leak(Box::new(Capture::read(cap).unwrap()));
        Run { rep: Replay::new(cap), cap, side: sidecar(side), compared: 0, equal: 0 }
    }

    fn facet(&mut self, key: &str, facet: &str, lines: &[String]) {
        let want = self.side.get(key).and_then(|m| m.get(facet)).cloned().unwrap_or_default();
        self.compared += lines.len().max(want.len()) as u64;
        let mut shown = 0;
        let mut diff = 0u64;
        for (k, (a, w)) in lines.iter().zip(want.iter()).enumerate() {
            if a != w {
                diff += 1;
                if shown < 6 {
                    shown += 1;
                    eprintln!("{key} {facet} line {k}:\n  ts:   {w}\n  rust: {a}");
                }
            }
        }
        if lines.len() != want.len() {
            eprintln!("{key} {facet}: {} lines here, {} in the sidecar", lines.len(), want.len());
            diff += lines.len().abs_diff(want.len()) as u64;
        }
        self.equal += (lines.len().max(want.len()) as u64).saturating_sub(diff);
        self.rep.check(key, facet, &list_dig(lines));
    }
}

fn col_letters(mut c: u32) -> String {
    let mut s = Vec::new();
    while c > 0 {
        s.push(b'A' + ((c - 1) % 26) as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// The canonical JSON of a cell's result as `canonRes` of the capture.
fn canon(eng: &CfEngine, r: &CfCell) -> String {
    let mut o = Obj::new();
    let dxf = |i: u16| &eng.dxf(i).unwrap().dxf;
    match r.fill {
        CfFill::None => {}
        CfFill::Dxf(d) => {
            o.raw("fill", fill(dxf(d).fill.as_ref().unwrap()));
        }
        f => {
            let mut x = Obj::new();
            x.s("pattern", "solid").s("fgColor", &eng.fill_color(f).unwrap());
            o.raw("fill", x.done());
        }
    }
    if let Some(d) = r.font_color() {
        o.s("fontColor", dxf(d).font_color.as_deref().unwrap());
    }
    o.ob("bold", r.bold()).ob("italic", r.italic()).ob("underline", r.underline()).ob("strike", r.strike());
    if r.border().iter().any(Option::is_some) {
        let mut x = Obj::new();
        for (k, name) in ["left", "right", "top", "bottom"].iter().enumerate() {
            if let Some(d) = r.border()[k] {
                let b = dxf(d).border.as_ref().unwrap();
                let e = [&b.left, &b.right, &b.top, &b.bottom][k].as_ref().unwrap();
                let mut y = Obj::new();
                y.s("style", &e.style).s("color", &e.color);
                x.raw(name, y.done());
            }
        }
        o.raw("border", x.done());
    }
    if let Some(d) = r.num_fmt() {
        o.os("numFmtCode", dxf(d).num_fmt_code.as_deref()).on("numFmtId", dxf(d).num_fmt_id);
    }
    if r.bar_rule().is_some() {
        let p = eng.bar_paint(r).unwrap();
        let mut x = Obj::new();
        x.n("from", p.from).n("to", p.to).s("color", p.color).b("gradient", p.gradient).os("border", p.border);
        x.on("axis", p.axis).os("axisColor", p.axis_color).b("legacy", p.legacy).b("negative", p.negative);
        o.raw("bar", x.done());
    }
    if let Some(i) = &r.icon() {
        let (set, index) = eng.icon_of(i).unwrap();
        let mut x = Obj::new();
        x.s("set", set).n("index", index);
        o.raw("icon", x.done());
    }
    o.t("hideValue", r.hide_value());
    o.done()
}

fn bytes_of(key: &str) -> Option<Vec<u8>> {
    let p = if let Some(n) = key.strip_prefix("gen/x1/") {
        dir().join(format!("workbooks/x1/x1-{n}.xlsx"))
    } else if let Some(n) = key.strip_prefix("gen/cf1/") {
        dir().join(format!("workbooks/cf1/cf1-{n}.xlsx"))
    } else {
        samples_dir().join(key.strip_prefix("samples/")?)
    };
    std::fs::read(p).ok()
}

#[derive(Default)]
struct Totals {
    workbooks: u64,
    sheets: u64,
    cells: u64,
    results: u64,
}

fn replay_workbooks(r: &mut Run, bytes_of: &dyn Fn(&str) -> Option<Vec<u8>>, t: &mut Totals) {
    let keys: Vec<String> =
        r.cap.records().iter().map(|rec| rec.key.clone()).filter(|k| k.starts_with("gen/") || k.starts_with("samples/")).collect();
    for key in keys {
        let Some(bytes) = bytes_of(&key) else {
            eprintln!("{key}: workbook not found, skipped");
            continue;
        };
        if r.cap.get(&key, "open") != Some("ok") {
            continue;
        }
        r.rep.check(&key, "open", "ok");
        t.workbooks += 1;
        let mut book = Book::open(&bytes, ReadLimits::default()).unwrap();
        book.load_all().unwrap();
        let views: Vec<usize> = (0..book.workbook.sheets.len()).filter(|&i| book.workbook.sheets[i].target.is_some()).collect();
        let any = views.iter().any(|&si| book.sheets[si].as_ref().is_some_and(|w| !w.cond_formats.is_empty()));
        r.rep.check(&key, "cf", if any { "yes" } else { "no" });
        for (vi, &si) in views.iter().enumerate() {
            let Some(bounds) = r.cap.get(&key, &format!("b{vi}")) else { continue };
            // the evaluated rectangle is an input of the comparison (the TS used area + 2)
            r.rep.check(&key, &format!("b{vi}"), bounds);
            t.sheets += 1;
            let b: Vec<f64> = bounds.split(' ').map(|x| x.parse::<i64>().unwrap() as f64).collect();
            let mut eng = CfEngine::for_book(&book, si, CLOCK, CfLimits::default()).expect("engine");
            prepare_book(&mut book, &eng).unwrap();
            let rects: Vec<Rect> = book.sheets[si].as_ref().unwrap().cond_formats.iter().flat_map(|f| parse_sqref(&f.sqref)).collect();
            let mut lines = Vec::new();
            let mut n = 0u64;
            let mut r1 = b[1];
            while r1 <= b[3] {
                let mut c1 = b[0];
                while c1 <= b[2] {
                    if rects.iter().any(|q| c1 >= q.c1 && c1 <= q.c2 && r1 >= q.r1 && r1 <= q.r2) {
                        n += 1;
                        if let Some(res) = eng.at(&book.store, r1 as u32 - 1, c1 as u32 - 1) {
                            lines.push(format!("{}{}\t{}", col_letters(c1 as u32), r1, canon(&eng, &res)));
                        }
                    }
                    c1 += 1.0;
                }
                r1 += 1.0;
            }
            assert_eq!(eng.missing_strings(), 0, "{key}: shared strings not decoded");
            r.rep.check(&key, &format!("n{vi}"), &n.to_string());
            t.cells += n;
            t.results += lines.len() as u64;
            r.facet(&key, &format!("cf{vi}"), &lines);
        }
    }
}

// ---------------------------------------------------------------- the formula traps

struct Table {
    cells: BTreeMap<(bool, u32, u32), FScalar>,
}

impl CfCells for Table {
    fn cell(&self, sheet: SheetSel, col: f64, row: f64) -> FScalar {
        let other = match sheet {
            SheetSel::Own => false,
            SheetSel::Other(Some(_)) => true,
            SheetSel::Other(None) => return FScalar::Err(uo_xlsx::grid::cf::value::REF),
        };
        if col.fract() != 0.0 || row.fract() != 0.0 || !(1.0..=16_384.0).contains(&col) || !(1.0..=1_048_576.0).contains(&row) {
            return FScalar::Null;
        }
        self.cells.get(&(other, row as u32, col as u32)).cloned().unwrap_or(FScalar::Null)
    }
    fn each(&self, sheet: SheetSel, c1: f64, r1: f64, c2: f64, r2: f64, f: &mut dyn FnMut(FScalar, f64, f64)) {
        let other = match sheet {
            SheetSel::Own => false,
            SheetSel::Other(Some(_)) => true,
            SheetSel::Other(None) => return,
        };
        for ((o, r, c), v) in &self.cells {
            let (cf, rf) = (f64::from(*c), f64::from(*r));
            if *o == other && cf >= c1 && cf <= c2 && rf >= r1 && rf <= r2 {
                f(v.clone(), cf, rf);
            }
        }
    }
}

struct Outra;
impl SheetNames for Outra {
    fn find(&self, name: &[u16]) -> Option<SheetIx> {
        (String::from_utf16_lossy(name).to_lowercase() == "outra").then_some(SheetIx(1))
    }
}

fn show(x: Option<&FScalar>) -> String {
    match x {
        None => "u".into(),
        Some(FScalar::Null) => "z".into(),
        Some(FScalar::Num(n)) => format!("n{}", jnum(*n)),
        Some(FScalar::Str(s)) => format!("s{}", js_json(JsStr::Utf16(s))),
        Some(FScalar::Bool(b)) => if *b { "b1" } else { "b0" }.into(),
        Some(FScalar::Err(e)) => if e.na { "eNA" } else { "ex" }.into(),
    }
}

fn unshow(s: &str) -> FScalar {
    let (k, rest) = s.split_at(1);
    match k {
        "z" => FScalar::Null,
        "n" => FScalar::Num(if rest == "-0" { -0.0 } else { uo_jsnum::parse::to_number(rest) }),
        "s" => FScalar::Str(json_units(rest)),
        "b" => FScalar::Bool(rest == "1"),
        "e" => FScalar::Err(uo_xlsx::grid::cf::value::FErr { na: rest == "NA" }),
        _ => panic!("value {s}"),
    }
}

/// UTF-16 units of a JSON string literal (lone surrogates as `\udXXX`).
fn json_units(s: &str) -> Vec<u16> {
    let s = s.strip_prefix('"').unwrap().strip_suffix('"').unwrap();
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut b = [0u16; 2];
            out.extend_from_slice(c.encode_utf16(&mut b));
            continue;
        }
        match it.next().unwrap() {
            'n' => out.push(10),
            'r' => out.push(13),
            't' => out.push(9),
            'b' => out.push(8),
            'f' => out.push(12),
            'u' => {
                let h: String = it.by_ref().take(4).collect();
                out.push(u16::from_str_radix(&h, 16).unwrap());
            }
            c => out.push(c as u16),
        }
    }
    out
}

fn formula_traps(r: &mut Run) -> (u64, u64) {
    let table_lines = r.side.get("formula").and_then(|m| m.get("table")).cloned().unwrap();
    let mut cells = BTreeMap::new();
    for l in &table_lines {
        let f: Vec<&str> = l.splitn(4, '\t').collect();
        cells.insert((f[0] == "Outra", f[2].parse().unwrap(), f[1].parse().unwrap()), unshow(f[3]));
    }
    r.facet("formula", "table", &table_lines);
    let table = Table { cells };
    let ctxs = [
        FCtx { col: 3.0, row: 4.0, dc: 2.0, dr: 3.0, today: 45_672.0, seconds: 43_200.0, date1904: false },
        FCtx { col: 1.0, row: 1.0, dc: 0.0, dr: 0.0, today: 45_672.0, seconds: 43_200.0, date1904: false },
        FCtx { col: 16_384.0, row: 1_048_576.0, dc: 16_383.0, dr: 1_048_575.0, today: 45_672.0, seconds: 43_200.0, date1904: true },
    ];
    let want = r.side.get("formula").and_then(|m| m.get("eval")).cloned().unwrap();
    let mut lines = Vec::new();
    let mut deep = 0u64;
    for w in &want {
        let (src, _) = w.split_once('\t').unwrap();
        let units = json_units(src);
        let text = String::from_utf16_lossy(&units);
        let f = parse_formula(&text, &Outra);
        let out: Vec<String> = ctxs.iter().map(|cx| show(f.as_ref().map(|f| eval_formula(f, &table, *cx)).as_ref())).collect();
        if units.len() > 300 {
            deep += 1;
        }
        lines.push(format!("{src}\t{}", out.join(" ")));
    }
    r.facet("formula", "eval", &lines);
    (want.len() as u64, deep)
}

// ---------------------------------------------------------------- icons and bars

fn n(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn ops_text(ops: &[CanvasOp]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    for op in ops {
        match op {
            CanvasOp::Save => out.push("save".into()),
            CanvasOp::Restore => out.push("restore".into()),
            CanvasOp::Translate(x, y) => out.push(format!("T {} {}", n(*x), n(*y))),
            CanvasOp::Scale(x, y) => out.push(format!("S {} {}", n(*x), n(*y))),
            CanvasOp::LineJoinMiter => out.push("LJ miter".into()),
            CanvasOp::LineCapButt => out.push("LC butt".into()),
            CanvasOp::LineWidth(w) => out.push(format!("LW {}", n(*w))),
            CanvasOp::BeginPath => path.clear(),
            CanvasOp::MoveTo(x, y) => path.push(format!("M {} {}", n(*x), n(*y))),
            CanvasOp::LineTo(x, y) => path.push(format!("L {} {}", n(*x), n(*y))),
            CanvasOp::Arc(x, y, r, a, b) => path.push(format!("A {} {} {} {} {}", n(*x), n(*y), n(*r), n(*a), n(*b))),
            CanvasOp::Rect(x, y, w, h) => path.push(format!("R {} {} {} {}", n(*x), n(*y), n(*w), n(*h))),
            CanvasOp::RoundRect(x, y, w, h, r) => path.push(format!("RR {} {} {} {} {}", n(*x), n(*y), n(*w), n(*h), n(*r))),
            CanvasOp::ClosePath => path.push("Z".into()),
            CanvasOp::Fill(c) => out.push(format!("F {c} [{}]", path.join(";"))),
            CanvasOp::Stroke(c) => out.push(format!("K {c} [{}]", path.join(";"))),
            CanvasOp::Clip => out.push(format!("CLIP [{}]", path.join(";"))),
            CanvasOp::FillRect(c, x, y, w, h) => out.push(format!("FR {c} {} {} {} {}", n(*x), n(*y), n(*w), n(*h))),
            CanvasOp::StrokeRect(c, x, y, w, h) => out.push(format!("KR {c} {} {} {} {}", n(*x), n(*y), n(*w), n(*h))),
        }
    }
    out.join(" | ")
}

fn icons_and_bars(r: &mut Run) -> u64 {
    let want = r.side.get("icons").and_then(|m| m.get("ops")).cloned().unwrap();
    let mut lines = Vec::new();
    for w in &want {
        let (head, _) = w.split_once('\t').unwrap();
        let f: Vec<&str> = head.split(' ').collect();
        let x = |i: usize| uo_jsnum::parse::to_number(f[i]);
        lines.push(format!("{head}\t{}", ops_text(&icon_ops(f[0], x(1), x(2), x(3), x(4)))));
    }
    r.facet("icons", "ops", &lines);
    let wantb = r.side.get("bars").and_then(|m| m.get("ops")).cloned().unwrap();
    let mut bl = Vec::new();
    for w in &wantb {
        let (head, _) = w.split_once('\t').unwrap();
        let (json, geo) = head.split_once("} ").unwrap();
        let get = |k: &str| -> Option<&str> {
            let at = json.find(&format!("\"{k}\":"))? + k.len() + 3;
            let rest = &json[at..];
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            Some(rest[..end].trim_matches('"'))
        };
        let num = |k: &str| get(k).map(uo_jsnum::parse::to_number);
        let color = get("color").unwrap().to_owned();
        let border = get("border").map(str::to_owned);
        let axis_color = get("axisColor").map(str::to_owned);
        let bp = BarPaint {
            from: num("from").unwrap(),
            to: num("to").unwrap(),
            color: &color,
            gradient: get("gradient") == Some("true"),
            border: border.as_deref(),
            axis: num("axis"),
            axis_color: axis_color.as_deref(),
            legacy: get("legacy") == Some("true"),
            negative: get("negative") == Some("true"),
        };
        let g: Vec<f64> = geo.split(' ').map(uo_jsnum::parse::to_number).collect();
        let rects = bar_rects(&bp, g[0], g[1], g[2], g[3]);
        let ops: Vec<String> = rects
            .iter()
            .map(|r| {
                let style = match &r.fill {
                    BarFill::Solid(c) => c.clone(),
                    BarFill::Linear { x0, x1, c0, c1 } => format!("G {} {} {c0} {c1}", n(*x0), n(*x1)),
                };
                format!("FR {style} {} {} {} {}", n(r.x), n(r.y), n(r.w), n(r.h))
            })
            .collect();
        bl.push(format!("{head}\t{}", ops.join(" | ")));
    }
    r.facet("bars", "ops", &bl);
    (want.len() + wantb.len()) as u64
}

#[test]
fn cf_ci() {
    let mut r = Run::open(&dir().join("sheet-cf.capture"), &dir().join("sheet-cf.side.gz"));
    let mut t = Totals::default();
    replay_workbooks(&mut r, &bytes_of, &mut t);
    let (forms, deep) = formula_traps(&mut r);
    let ops = icons_and_bars(&mut r);
    let rep = r.rep.finish();
    eprintln!(
        "cf: {} workbooks, {} sheets, {} cells evaluated, {} results; formulas {} × 3 contexts ({} over 300 units); {} icon/bar paints; lines compared {}, equal {}; {}",
        t.workbooks,
        t.sheets,
        t.cells,
        t.results,
        forms,
        deep,
        ops,
        r.compared,
        r.equal,
        rep.summary()
    );
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
    assert_eq!(r.compared, r.equal);
    assert!(t.workbooks >= 50 && t.cells > 2000);
}

/// The local run (`UO_CF_BIG=1`): 300 generated `cf-big` workbooks + the 400 `x1-big` ones (capture files made by
/// `capture-cf.mjs --big` under `core/target/sheet-cap`).
#[test]
fn cf_big() {
    if !uo_testkit::opt_in("UO_CF_BIG", "the local CF parity (700 workbooks)") {
        return;
    }
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap");
    let mut r = Run::open(&base.join("cf-big.capture"), &base.join("cf-big.side.gz"));
    let bytes = |key: &str| -> Option<Vec<u8>> {
        if let Some(n) = key.strip_prefix("gen/x1-big/") {
            return std::fs::read(base.join(format!("workbooks/x1-big/x1-big-{n}.xlsx"))).ok();
        }
        let i: usize = key.strip_prefix("gen/cf-big/")?.parse().ok()?;
        uo_xlsx_gen_cf(i)
    };
    let mut t = Totals::default();
    replay_workbooks(&mut r, &bytes, &mut t);
    let rep = r.rep.finish();
    eprintln!("cf-big: {} workbooks, {} sheets, {} cells, {} results; {}", t.workbooks, t.sheets, t.cells, t.results, rep.summary());
    assert!(rep.mismatches.is_empty(), "{}", rep.describe(10));
    assert_eq!(r.compared, r.equal);
}

/// The `cf-big` workbooks are written by `capture-cf.mjs --big --write-big` into `core/target/sheet-cap/workbooks/cf-big`.
fn uo_xlsx_gen_cf(i: usize) -> Option<Vec<u8>> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sheet-cap/workbooks/cf-big");
    std::fs::read(base.join(format!("cf-big-{i:04}.xlsx"))).ok()
}
