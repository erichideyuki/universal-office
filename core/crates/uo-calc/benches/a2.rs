//! A2 benchmarks (record `uo-calc.md` "A2 — as built", PERF rows), native:
//!
//!   cargo bench -p uo-calc --bench a2
//!
//! The same inputs as `js/bench-a2.mjs` (the frozen TS in Node, the baseline), ns per formula, medians of 7 rounds:
//! - parse, print (file form) and parse + print of the oracle bank's formulas and the generated workbooks' (calc-order
//!   sidecar), each at its cell; one arena per round (the engine keeps every tree in one arena);
//! - evaluate 10,000 operator formulas (`js/bench-a2.mjs` `evalFormula(i)`) over a 100 × 10 grid of numbers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout)]

#[path = "../tests/support/mod.rs"]
mod support;

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use support::{J, parse};
use uo_calc::ast::{Ast, NodeId};
use uo_calc::eval::{EvalHost, EvalState, Evaluator, TableDef};
use uo_calc::fnreg::Throw;
use uo_calc::key::{Area, SheetIx};
use uo_calc::locale::ValueLocale;
use uo_calc::parse::{FormulaParser, ParseOptions};
use uo_calc::print::{PrintOptions, print_into};
use uo_calc::value::{ArrayValue, Scalar, Value};
use uo_calc::{JsStr, JsString, StrId, StrPool};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn ns_per<R>(n: usize, mut f: impl FnMut() -> R) -> f64 {
    black_box(f());
    median(
        (0..7)
            .map(|_| {
                let t = Instant::now();
                black_box(f());
                t.elapsed().as_secs_f64() * 1e9 / n as f64
            })
            .collect(),
    )
}

/// `A1` → (row, col) as `bench-a2.mjs` reads a cell key.
fn a1(k: &str) -> (u32, u32) {
    let letters: String = k.chars().take_while(char::is_ascii_uppercase).collect();
    let digits = &k[letters.len()..];
    if letters.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return (0, 0);
    }
    let c = letters.bytes().fold(0u32, |c, b| c * 26 + u32::from(b - 64));
    (digits.parse::<u32>().unwrap() - 1, c - 1)
}

/// The formula list of `bench-a2.mjs` `formulaList()` (texts as UTF-16, with their cell).
fn formula_list() -> Vec<(Vec<u16>, u32, u32)> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle = crate_dir.join("../../../sheet/packages/calc/test/oracle");
    let mut files: Vec<_> =
        std::fs::read_dir(&oracle).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(".cases.json")).collect();
    files.sort();
    let strip = |u: Vec<u16>| if u.first() == Some(&u16::from(b'=')) { u[1..].to_vec() } else { u };
    let mut list = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        // a BOM and the final line break of the file (ASCII white space only)
        let j = parse(text.trim_start_matches('\u{feff}').trim_end_matches(['\r', '\n', ' ']));
        let cases = match &j {
            J::Arr(a) => a.clone(),
            o => o.get("cases").unwrap().arr().to_vec(),
        };
        for c in &cases {
            if let Some(f) = c.get("f").and_then(J::js) {
                list.push((strip(f.as_js().to_utf16()), 0, 0));
            }
            if let Some(J::Obj(cells)) = c.get("cells") {
                for (k, v) in cells {
                    if let Some(f) = v.get("f") {
                        let (r, col) = a1(k);
                        list.push((strip(f.js().unwrap().as_js().to_utf16()), r, col));
                    }
                }
            }
        }
    }
    let side = support::gunzip(&support::captures_dir().join("calc-order.side.gz"));
    for line in side.lines() {
        let mut p = line.splitn(3, '\t');
        let (_, facet, rest) = (p.next(), p.next(), p.next());
        if facet != Some("wb") {
            continue;
        }
        let wb = parse(rest.unwrap());
        for sh in wb.get("sheets").unwrap().arr() {
            for cell in sh.get("cells").unwrap().arr() {
                let c = cell.arr();
                if let Some(t) = c[2].get("f").and_then(|f| f.get("text")).and_then(J::js) {
                    list.push((t.as_js().to_utf16(), c[0].num().unwrap() as u32, c[1].num().unwrap() as u32));
                }
            }
        }
    }
    let mut parser = FormulaParser::new();
    let mut ast = Ast::new();
    list.retain(|(f, r, c)| parser.parse(&mut ast, JsStr::Utf16(f), &ParseOptions { row: *r, col: *c, ..Default::default() }).is_ok());
    list
}

fn eval_formula(i: usize) -> String {
    let (r, s) = (1 + i % 100, 1 + (i * 7) % 99);
    match i % 8 {
        0 => format!("A{r}+B{r}*2-C{r}/3"),
        1 => format!("(A{r}>B{s})*1+(A{r}=C{r})"),
        2 => format!("A{r}&\"-\"&B{s}"),
        3 => format!("-A{r}%+2^3"),
        4 => format!("LET(x,A{r},y,x*x,y+x)"),
        5 => format!("A{r}:B{s} B{r}:C{s}"),
        6 => format!("{{1,2,3}}*A{r}"),
        _ => format!("((A{r}+1)*(B{s}+2))^0.5"),
    }
}

/// A 100 × 10 grid of numbers (`gridValue`).
struct Grid {
    locale: ValueLocale,
    v: Vec<f64>,
    pool: StrPool,
    name: JsString,
}

impl EvalHost for Grid {
    fn locale(&self) -> &ValueLocale {
        &self.locale
    }
    fn date1904(&self) -> bool {
        false
    }
    fn year(&self) -> f64 {
        2025.0
    }
    fn sheet_count(&self) -> u32 {
        1
    }
    fn sheet_name(&self, _: u32) -> JsStr<'_> {
        self.name.as_js()
    }
    fn sheet_index(&self, _: JsStr<'_>) -> Option<SheetIx> {
        Some(SheetIx(0))
    }
    fn external_sheet(&self, _: f64, _: JsStr<'_>) -> Option<SheetIx> {
        None
    }
    fn cell(&mut self, _: SheetIx, r: u32, c: u32) -> Result<Scalar, Throw> {
        Ok(if r < 100 && c < 10 { Scalar::Num(self.v[(r * 10 + c) as usize]) } else { Scalar::Empty })
    }
    fn values(&mut self, a: Area) -> Result<ArrayValue, Throw> {
        let v = &self.v;
        Ok(ArrayValue::fill(a.r2 - a.r1 + 1, a.c2 - a.c1 + 1, |r, c| {
            let (r, c) = (a.r1 + r, a.c1 + c);
            if r < 100 && c < 10 { Scalar::Num(v[(r * 10 + c) as usize]) } else { Scalar::Empty }
        }))
    }
    fn for_each_cell(&mut self, _: Area, _: &mut dyn FnMut(Scalar, u32, u32)) -> Result<(), Throw> {
        Ok(())
    }
    fn formula_text(&mut self, _: &Ast, _: SheetIx, _: u32, _: u32) -> Option<JsString> {
        None
    }
    fn is_subtotal_cell(&mut self, _: SheetIx, _: u32, _: u32) -> bool {
        false
    }
    fn row_hidden(&mut self, _: SheetIx, _: u32) -> bool {
        false
    }
    fn row_filtered(&mut self, _: SheetIx, _: u32) -> bool {
        false
    }
    fn now(&mut self) -> f64 {
        45658.5
    }
    fn random(&mut self) -> f64 {
        0.5
    }
    fn defined_name(&mut self, _: JsStr<'_>, _: Option<SheetIx>, _: bool) -> Option<NodeId> {
        None
    }
    fn table(&self, _: JsStr<'_>) -> Option<&TableDef> {
        None
    }
    fn table_at(&self, _: SheetIx, _: u32, _: u32) -> Option<&TableDef> {
        None
    }
    fn spill_area(&mut self, _: SheetIx, _: u32, _: u32) -> Result<Option<Area>, Throw> {
        Ok(None)
    }
    fn cell_info(&mut self, _: SheetIx, _: u32, _: u32, _: JsStr<'_>) -> Option<Scalar> {
        None
    }
    fn pooled(&self, id: StrId) -> JsStr<'_> {
        self.pool.get(id)
    }
}

fn main() {
    let list = formula_list();
    let n = list.len();
    let mut parser = FormulaParser::new();
    let mut out = Vec::new();
    out.push((
        format!("parse ({n} bank + generated formulas)"),
        ns_per(n, || {
            let mut ast = Ast::new();
            let mut k = 0;
            for (f, r, c) in &list {
                k += parser.parse(&mut ast, JsStr::Utf16(f), &ParseOptions { row: *r, col: *c, ..Default::default() }).unwrap().0;
            }
            k
        }),
    ));
    let mut ast = Ast::new();
    let roots: Vec<NodeId> = list
        .iter()
        .map(|(f, r, c)| parser.parse(&mut ast, JsStr::Utf16(f), &ParseOptions { row: *r, col: *c, ..Default::default() }).unwrap())
        .collect();
    let mut buf: Vec<u16> = Vec::new();
    out.push((
        "print (file form)".to_owned(),
        ns_per(n, || {
            let mut k = 0;
            for (i, &root) in roots.iter().enumerate() {
                buf.clear();
                print_into(&ast, root, &PrintOptions { row: list[i].1, col: list[i].2, ..Default::default() }, &mut buf);
                k += buf.len();
            }
            k
        }),
    ));
    out.push((
        "parse + print".to_owned(),
        ns_per(n, || {
            let mut a = Ast::new();
            let mut k = 0;
            for (f, r, c) in &list {
                let root = parser.parse(&mut a, JsStr::Utf16(f), &ParseOptions { row: *r, col: *c, ..Default::default() }).unwrap();
                buf.clear();
                print_into(&a, root, &PrintOptions { row: *r, col: *c, ..Default::default() }, &mut buf);
                k += buf.len();
            }
            k
        }),
    ));
    let mut grid = Grid { locale: ValueLocale::en_us(), v: Vec::new(), pool: StrPool::new(), name: JsString::from_str_js("Sheet1") };
    for r in 0..100u32 {
        for c in 0..10u32 {
            grid.v.push(f64::from((r * 31 + c * 7) % 97) / 4.0);
        }
    }
    const N: usize = 10_000;
    let mut east = Ast::new();
    let evs: Vec<(NodeId, u32, u32)> = (0..N)
        .map(|i| {
            let (row, col) = (200 + (i % 50) as u32, 12);
            (
                parser
                    .parse(&mut east, JsStr::Latin1(eval_formula(i).as_bytes()), &ParseOptions { row, col, ..Default::default() })
                    .unwrap(),
                row,
                col,
            )
        })
        .collect();
    let mut st = EvalState::new();
    out.push((
        format!("evaluate {N} operator formulas"),
        ns_per(N, || {
            let mut k = 0;
            for &(root, r, c) in &evs {
                let v = Evaluator { ast: &mut east, host: &mut grid, st: &mut st }.eval_formula(root, SheetIx(0), r, c, true);
                if matches!(v, Ok(Value::Scalar(Scalar::Num(_)))) {
                    k += 1;
                }
            }
            st.clear_pass();
            k
        }),
    ));
    for (k, v) in out {
        println!("{k}: {v:.1} ns/formula");
    }
}
