//! A5 robustness and memory (record `uo-calc.md` "A5 — as built"): hostile formula texts through the translator both
//! ways and the legacy rewrite (deep operator chains, deep parentheses, deeply nested operator functions, long
//! texts, random UTF-16 with lone surrogates) — no panic, bounded time, no recursion; M1 for every A5 operation
//! (translate, rewrite, file form, open-recalc decision and apply, save preparation): live bytes back exactly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_calc::ast::Ast;
use uo_calc::cell::FileCell;
use uo_calc::engine::{Engine, EngineInput, EngineOptions};
use uo_calc::file_form::to_file_formula;
use uo_calc::i18n::{LangSpec, SyntaxOptions, Translator};
use uo_calc::implicit::{NoNames, legacy_to_dynamic};
use uo_calc::locale::{Clock, RandomSource, ValueLocale};
use uo_calc::open_recalc::{CalcPrFlags, OpenRecalcWorkbook, apply_open_recalc, should_recalc_on_open};
use uo_calc::parse::{FormulaParser, ParseOptions};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::save::{SaveOptions, prepare_for_save};
use uo_calc::{JsStr, JsString, Store};

const PT: SyntaxOptions<'static> = SyntaxOptions {
    lang: LangSpec::Id("pt-BR"),
    decimal: None,
    list: None,
    array_col: None,
    array_row: None,
    r1c1: false,
    file_prefixes: false,
    in_table: None,
    names: None,
    regional_decimal: None,
};

/// xorshift32 (deterministic inputs).
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn both_ways(tr: &mut Translator, u: &[u16]) -> f64 {
    let t = Instant::now();
    let mut opts = PT;
    let a = tr.to_local(JsStr::Utf16(u), &opts);
    let b = tr.from_local(a.as_js(), &opts);
    opts.file_prefixes = true;
    let _ = tr.from_local(b.as_js(), &opts);
    opts.r1c1 = true;
    let _ = tr.to_local(JsStr::Utf16(u), &opts);
    t.elapsed().as_secs_f64()
}

#[test]
fn hostile_texts_translate() {
    let mut tr = Translator::new();
    let n = 100_000;
    let cases: Vec<Vec<u16>> = vec![
        units(&format!("={}1{}", "(".repeat(n), ")".repeat(n))),
        units(&format!("={}A1", "-".repeat(n))),
        units(&format!("=1{}", "+1".repeat(n))),
        units(&format!("={}A1{}", "_xlfn.SINGLE(".repeat(20_000), ")".repeat(20_000))),
        units(&format!("={}A1:B2{}", "_xlfn._TRO_ALL(".repeat(2_000), ")".repeat(2_000))),
        units(&format!("=LET({}1)", "x,1,".repeat(20_000))),
        units(&format!("=SUM({}1)", "A1,".repeat(n))),
        units(&format!("=T[{}]", "[".repeat(n))),
        units(&format!("=\"{}", "a".repeat(n))),
        units(&format!("='{}", "a".repeat(n))),
        units(&format!("={{{}}}", "1,".repeat(n))),
        units(&"#N/A".repeat(n / 4)),
    ];
    for c in &cases {
        let s = both_ways(&mut tr, c);
        assert!(s < 20.0, "{s} s for a {}-unit text", c.len());
    }
    // random UTF-16 with formula tokens and lone surrogates
    let mut rng = Rng(0xa5a5);
    let toks: [&str; 24] = [
        "SOMA(",
        "SUM(",
        ")",
        ";",
        ",",
        "\\",
        "{",
        "}",
        "\"",
        "'",
        "[",
        "]",
        "#N/D",
        "#",
        "@",
        "A1",
        "L1C1",
        "R[1]C",
        "_xlfn.",
        "_xlpm.x",
        "LET(",
        "VERDADEIRO",
        " ",
        "\u{301}",
    ];
    let mut worst = 0.0f64;
    for _ in 0..20_000 {
        let mut u: Vec<u16> = vec![u16::from(b'=')];
        for _ in 0..rng.below(24) {
            if rng.below(5) == 0 {
                u.push(0xD800 + (rng.below(0x800) as u16));
            } else {
                u.extend(toks[rng.below(toks.len() as u32) as usize].encode_utf16());
            }
        }
        worst = uo_jsnum::math::max(worst, both_ways(&mut tr, &u));
    }
    assert!(worst < 0.05, "worst {worst}");
}

#[test]
fn hostile_trees_rewrite() {
    let mut parser = FormulaParser::new();
    let n = 100_000;
    for f in [
        format!("{}A1:A3{}", "(".repeat(n), ")".repeat(n)),
        format!("A1:A3{}", "+A1:A3".repeat(n)),
        format!("{}A1:A3", "-".repeat(n)),
        format!("{}A1:A3{}", "SUM(".repeat(500), ")".repeat(500)),
        format!("{}A1:A3{}", "IF(1,".repeat(500), ")".repeat(500)),
        format!("INDEX({}A1:A3{},1)", "(".repeat(n), ")".repeat(n)),
        format!("XLOOKUP(1,A1:A3,{}B1:B3{})", "(".repeat(n), ")".repeat(n)),
    ] {
        let mut ast = Ast::new();
        let u = units(&f);
        let Ok(root) = parser.parse(&mut ast, JsStr::Utf16(&u), &ParseOptions::default()) else { continue };
        let t = Instant::now();
        let l = legacy_to_dynamic(&mut ast, root, &NoNames);
        let _ = print_formula(&ast, l.root, &PrintOptions { row: 0, col: 0, r1c1: false, ui: true });
        let ff = to_file_formula(&mut ast, root, 0, 0, &NoNames);
        assert!(!ff.text.as_js().is_empty());
        assert!(t.elapsed().as_secs_f64() < 20.0);
    }
}

fn small_book() -> Store {
    let mut st = Store::new();
    let sh = st.add_sheet().unwrap();
    for r in 0..50u32 {
        st.set_file_cell(sh, r, 0, &FileCell { v: Some(JsString::from_str_js(&format!("{r}"))), ..FileCell::default() }).unwrap();
        let f = match r % 4 {
            0 => format!("A{}:A{}*2", r + 1, r + 3),
            1 => "SUM(A1:A10)".to_owned(),
            2 => format!("A{}+1", r + 1),
            _ => "IF(A1:A3>1,1,2)".to_owned(),
        };
        st.set_file_cell(
            sh,
            r,
            1,
            &FileCell { f: Some(JsString::from_str_js(&f)), v: Some(JsString::from_str_js("1")), ..FileCell::default() },
        )
        .unwrap();
    }
    st
}

/// M1 after a warm-up run (lazily built static tables — case mapping, the dispatch table — are not leaks).
fn m1(mut op: impl FnMut()) {
    op();
    uo_alloc::assert_no_leak(op);
}

#[test]
fn memory_m1() {
    assert!(uo_alloc::is_counting());
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    let canon = units("=SUM(A1:B2;1,5)+LET(x;A1;x*2)&Tabela1[[#This Row];[Qtd]]&IFERROR(1/0;#N/A)&{1,2;3,4}");
    m1(|| {
        let mut tr = Translator::new();
        let a = tr.to_local(JsStr::Utf16(&canon), &PT);
        let b = tr.from_local(a.as_js(), &PT);
        drop((a, b, tr));
    });
    m1(|| {
        let mut ast = Ast::new();
        let mut p = FormulaParser::new();
        let u = units("IF(A1:A3>1,A1:A3,INDEX(B1:B3,0))+@C1:C3");
        let root = p.parse(&mut ast, JsStr::Utf16(&u), &ParseOptions::default()).unwrap();
        let l = legacy_to_dynamic(&mut ast, root, &NoNames);
        let ff = to_file_formula(&mut ast, l.root, 0, 0, &NoNames);
        drop((l, ff, ast, p));
    });
    let book = OpenRecalcWorkbook { app_name_xl: true, calc_pr: Some(CalcPrFlags { calc_id: Some(191_029.0), ..CalcPrFlags::default() }) };
    let mut st = small_book();
    m1(|| drop(should_recalc_on_open(&book, &st)));
    let input = EngineInput { sheet_names: vec![JsString::from_str_js("S")], ..EngineInput::default() };
    m1(|| {
        let mut e = Engine::new(&mut st, &input, &EngineOptions::new(ValueLocale::en_us()), None).unwrap();
        let plan = should_recalc_on_open(&OpenRecalcWorkbook { app_name_xl: false, calc_pr: None }, &st);
        let mut rnd = RandomSource::sequence(Vec::new());
        let _ = apply_open_recalc(&mut e, &mut st, &plan, Clock { now_serial: 45658.5 }, &mut rnd).unwrap();
        // the formula bar texts of the legacy cells
        for r in 0..50 {
            drop(e.formula_text(&st, uo_calc::SheetIx(0), r, 1));
            drop(e.formula_display(&st, uo_calc::SheetIx(0), r, 1));
        }
        // a second save is a no-op on the store's attributes: the first one's growth is measured apart
        drop(e);
    });
    let mut e = Engine::new(&mut st, &input, &EngineOptions::new(ValueLocale::en_us()), None).unwrap();
    let mut rnd = RandomSource::sequence(Vec::new());
    e.recalc(&mut st, Clock { now_serial: 45658.5 }, &mut rnd).unwrap();
    let _ = prepare_for_save(&mut e, &mut st, &mut SaveOptions::default());
    // idempotent: a second preparation changes nothing and keeps nothing
    uo_alloc::assert_no_leak(|| {
        let p = prepare_for_save(&mut e, &mut st, &mut SaveOptions::default());
        assert!(p.touched.is_empty(), "{p:?}");
        drop(p);
    });
}
