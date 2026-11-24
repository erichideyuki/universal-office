//! A5 benchmarks (record `uo-calc.md` "A5 — as built", PERF), native:
//!
//!   cargo bench -p uo-calc --bench a5
//!
//! The same inputs as `js/bench-a5.mjs` (the frozen TS in Node, the baseline), medians of 7 rounds:
//! - `toLocal`/`fromLocal` (pt-BR) of 100,000 formulas `formula(k)` (15 templates), one [`Translator`] reused, the
//!   output buffer reused (the translation allocates nothing once warm: checked here);
//! - the legacy `@` rewrite + screen print of the 15 template trees, 100,000 calls (the engine runs it once per
//!   hash-consed tree);
//! - the open-recalc decision on giant-1m (no formula), 100,000 formulas, and the same + one `NOW()` without `ca`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout)]

use std::hint::black_box;
use std::time::Instant;

use uo_alloc as _;
use uo_calc::ast::Ast;
use uo_calc::cell::FileCell;
use uo_calc::i18n::{LangSpec, SyntaxOptions, Translator};
use uo_calc::implicit::{LegacyBufs, NoNames, legacy_to_dynamic_in};
use uo_calc::open_recalc::{CalcPrFlags, OpenRecalcWorkbook, should_recalc_on_open};
use uo_calc::parse::{FormulaParser, ParseOptions};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::store::{CellKind, RawCell, RowMeta, ValueTag};
use uo_calc::{JsStr, JsString, Store};

fn median(mut f: impl FnMut()) -> f64 {
    f();
    let mut v: Vec<f64> = (0..7)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e9
        })
        .collect();
    v.sort_by(f64::total_cmp);
    v[3]
}

const TEMPLATES: [&str; 15] = [
    "SUM(A{i}:B{j})*2",
    "IF(A{i}>0,\"yes\",\"no\")",
    "VLOOKUP(A{i},Sheet2!$A$1:$C$100,3,FALSE)",
    "AVERAGE(A{i}:A{j})+MAX(B{i}:B{j})",
    "IFERROR(INDEX(C:C,MATCH(A{i},B:B,0)),#N/A)",
    "ROUND(A{i}*1.5,2)",
    "TEXT(A{i},\"0.00\")&\" items\"",
    "LET(x,A{i},y,x*2,x+y)",
    "SUMIFS(C:C,A:A,\">=\"&A{i},B:B,\"x\")",
    "COUNTIF(A1:A{j},A{i})>1",
    "{1.5,2;3,4}*A{i}",
    "Tabela1[[#This Row],[Qtd]]*A{i}",
    "NOW()-A{i}",
    "CONCATENATE(A{i},\" \",B{i})",
    "AND(A{i}>0,B{i}<10,OR(C{i}=1,D{i}=2))",
];

fn formula(k: usize) -> String {
    let i = 1 + (k % 1000);
    format!("={}", TEMPLATES[k % TEMPLATES.len()].replace("{i}", &i.to_string()).replace("{j}", &(i + 10).to_string()))
}

const N: usize = 100_000;

fn num(x: f64) -> RawCell {
    RawCell { kind: CellKind::new(ValueTag::None, CellKind::HAS_VALUE), payload: x.to_bits(), style: 0 }
}

fn main() {
    let canon: Vec<JsString> = (0..N).map(|k| JsString::from_str_js(&formula(k))).collect();
    let o = SyntaxOptions { lang: LangSpec::Id("pt-BR"), ..SyntaxOptions::default() };
    let mut tr = Translator::new();
    let local: Vec<JsString> = canon.iter().map(|t| tr.to_local(t.as_js(), &o)).collect();
    let mut out: Vec<u16> = Vec::with_capacity(256);
    let mut rows: Vec<(String, f64, f64)> = Vec::new();
    let t = median(|| {
        for f in &canon {
            out.clear();
            tr.to_local_into(f.as_js(), &o, &mut out);
            black_box(&out);
        }
    });
    rows.push(("toLocal pt-BR, 100k formulas (ns/formula)".into(), t / N as f64, 3083.2));
    let t = median(|| {
        for f in &local {
            out.clear();
            tr.from_local_into(f.as_js(), &o, &mut out);
            black_box(&out);
        }
    });
    rows.push(("fromLocal pt-BR, 100k formulas (ns/formula)".into(), t / N as f64, 6089.9));
    // allocations of a warm translation: none beyond the output (which is reused here)
    let ((), usage) = uo_alloc::measure(|| {
        for f in canon.iter().take(1000) {
            out.clear();
            tr.to_local_into(f.as_js(), &o, &mut out);
        }
        for f in local.iter().take(1000) {
            out.clear();
            tr.from_local_into(f.as_js(), &o, &mut out);
        }
    });
    println!("bytes allocated by 2,000 warm translations (output reused): {}", usage.allocated);

    // the legacy rewrite per tree
    let mut parser = FormulaParser::new();
    let mut ast = Ast::new();
    let trees: Vec<_> = TEMPLATES
        .iter()
        .map(|t| {
            let s = t.replace("{i}", "5").replace("{j}", "15");
            let u: Vec<u16> = s.encode_utf16().collect();
            parser.parse(&mut ast, JsStr::Utf16(&u), &ParseOptions::default()).unwrap()
        })
        .collect();
    let mark = ast.mark();
    let mut bufs = LegacyBufs::default();
    let t = median(|| {
        for k in 0..N {
            let l = legacy_to_dynamic_in(&mut ast, trees[k % trees.len()], &NoNames, &mut bufs);
            black_box(print_formula(&ast, l.root, &PrintOptions { row: 0, col: 0, r1c1: false, ui: true }));
            if k % 1000 == 999 {
                ast.rollback(mark);
            }
        }
        ast.rollback(mark);
    });
    rows.push(("legacyToDynamic + print(ui), per tree (ns)".into(), t / N as f64, 1161.5));

    // open-recalc
    let book = OpenRecalcWorkbook { app_name_xl: true, calc_pr: Some(CalcPrFlags { calc_id: Some(191_029.0), ..CalcPrFlags::default() }) };
    let mut giant = Store::new();
    let sh = giant.add_sheet().unwrap();
    let mut l = giant.load_sheet(sh).unwrap();
    for r in 0..10_000u32 {
        l.row(r, RowMeta::default()).unwrap();
        for c in 0..100u32 {
            l.cell_raw(c, num(f64::from((r + 1) * 1000 + c + 1))).unwrap();
        }
    }
    l.finish().unwrap();
    let mk = |volatile: bool| {
        let mut s = Store::new();
        let sh = s.add_sheet().unwrap();
        for r in 0..N as u32 {
            let c = FileCell {
                f: Some(JsString::from_str_js(&format!("A{}+1", r + 1))),
                v: Some(JsString::from_str_js("1")),
                ..FileCell::default()
            };
            s.set_file_cell(sh, r, 1, &c).unwrap();
        }
        if volatile {
            let c = FileCell { f: Some(JsString::from_str_js("NOW()")), v: Some(JsString::from_str_js("1")), ..FileCell::default() };
            s.set_file_cell(sh, N as u32, 1, &c).unwrap();
        }
        s
    };
    let fbook = mk(false);
    let vbook = mk(true);
    for (name, s, ts) in
        [("giant-1m (no formula)", &giant, 10.8), ("100k formulas", &fbook, 17.5), ("100k formulas + NOW() at the end", &vbook, 16.6)]
    {
        let mut plan = None;
        let t = median(|| plan = Some(should_recalc_on_open(&book, s)));
        let p = plan.unwrap();
        rows.push((format!("shouldRecalcOnOpen {name} (ms) -> {}/{}", p.mode.text(), p.reason.text()), t / 1e6, ts));
    }
    println!("{:<75} {:>10} {:>10} {:>7}", "operation", "Rust", "TS", "Rust/TS");
    for (k, v, ts) in rows {
        println!("{k:<75} {v:>10.2} {ts:>10.1} {:>7.3}", v / ts);
    }
}
