//! A2 robustness and memory (seeded, seconds): hostile formula text (deep nesting, huge literals, long chains,
//! unterminated quotes and brackets) and random UTF-16 text through the parser (A1 and R1C1), the printer (four
//! forms), the round trip and the evaluator: no panic, bounded time, the depth limit as declared (A2-2). Then M1
//! (parse, print and evaluate leave nothing behind) and the zero-allocation guarantee of scalar evaluation (PERF).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../examples/stack_common/mod.rs"]
mod stack_common;

use std::time::{Duration, Instant};

use stack_common::EmptyHost;
use uo_calc::ast::{Ast, MAX_AST_DEPTH};
use uo_calc::eval::{EvalState, Evaluator};
use uo_calc::parse::{FormulaParser, ParseOptions, SyntaxKind};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::{JsStr, SheetIx};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Parses, prints in four forms, re-parses the file form and evaluates (A1 options only), on a big stack thread
/// (debug frames); returns whether the text parsed.
fn exercise(units: &[u16], r1c1: bool, parser: &mut FormulaParser, st: &mut EvalState) -> bool {
    let mut ast = Ast::new();
    let o = ParseOptions { r1c1, row: 4, col: 2, ..Default::default() };
    let Ok(n) = parser.parse(&mut ast, JsStr::Utf16(units), &o) else { return false };
    for po in [
        PrintOptions { row: 4, col: 2, ..Default::default() },
        PrintOptions { row: 4, col: 2, ui: true, ..Default::default() },
        PrintOptions { r1c1: true, ..Default::default() },
        PrintOptions { r1c1: true, ui: true, ..Default::default() },
    ] {
        let t = print_formula(&ast, n, &po);
        if !po.r1c1 && !po.ui {
            let mut a2 = Ast::new();
            let _ = parser.parse(&mut a2, t.as_js(), &ParseOptions { row: 4, col: 2, ..Default::default() });
        }
    }
    if !r1c1 {
        let mut host = EmptyHost::default();
        let mut ev = Evaluator { ast: &mut ast, host: &mut host, st };
        let _ = ev.eval_formula(n, SheetIx(0), 4, 2, true);
        let _ = ev.eval_formula(n, SheetIx(0), 4, 2, false);
        st.clear_pass();
    }
    true
}

fn on_big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(64 << 20).spawn(f).unwrap().join().unwrap();
}

#[test]
fn hostile_formulas_bounded() {
    on_big_stack(|| {
        let rep = |s: &str, n: usize| s.repeat(n);
        let cases: Vec<(String, &str)> = vec![
            (format!("{}1{}", rep("(", 1_000_000), rep(")", 1_000_000)), "1M parentheses"),
            (format!("1{}", rep("+1", 300_000)), "300k-term chain"),
            (format!("{}1", rep("-", 1_000_000)), "1M unary minus"),
            (format!("{}1{}", rep("1^(", 200_000), rep(")", 200_000)), "200k right-nested powers"),
            (format!("{}1{}", rep("F(", 100_000), rep(")", 100_000)), "100k nested functions"),
            (format!("{}x{}", rep("LET(x,1,", 600), rep(")", 600)), "600 nested LET"),
            (rep("9", 1_000_000), "1M-digit number"),
            (format!("0.{}1e-5", rep("0", 500_000)), "long fraction"),
            (format!("\"{}\"", rep("a", 1_000_000)), "1M-unit text"),
            (format!("\"{}", rep("a", 1_000_000)), "unterminated text"),
            (format!("'{}", rep("b", 1_000_000)), "unterminated sheet"),
            (rep("a", 1_000_000), "1M-unit name"),
            (format!("T[{}", rep("[", 100_000)), "unclosed structured reference"),
            (format!("{{{}1}}", rep("1,", 200_000)), "200k-element array"),
            (format!("{}A1", rep("A1:", 100_000)), "100k-range chain"),
            (rep("#", 200_000), "200k hashes"),
            (format!("SUM({})", rep("A1,", 100_000)), "100k arguments"),
            (format!("({})", rep("A1,", 50_000) + "A1"), "50k-area union"),
            (rep("R[1]C[1]", 100_000), "R1C1 run"),
            (format!("{}x{}", rep("LAMBDA(x,", 300), rep(")", 300)), "300 nested LAMBDA"),
        ];
        let mut parser = FormulaParser::new();
        let mut st = EvalState::new();
        let mut worst = Duration::ZERO;
        for (text, what) in &cases {
            let u: Vec<u16> = text.encode_utf16().collect();
            for r1c1 in [false, true] {
                let t = Instant::now();
                let ok = exercise(&u, r1c1, &mut parser, &mut st);
                let e = t.elapsed();
                eprintln!("{what} (r1c1 {r1c1}): parsed {ok} in {e:?}");
                worst = worst.max(e);
            }
        }
        assert!(worst < Duration::from_secs(10), "{worst:?}");
    });
}

#[test]
fn depth_limit_as_declared() {
    let mut parser = FormulaParser::new();
    let nest = |n: u32| format!("{}1{}", "F(".repeat(n as usize), ")".repeat(n as usize));
    let mut ast = Ast::new();
    assert!(parser.parse(&mut ast, JsStr::Latin1(nest(MAX_AST_DEPTH - 1).as_bytes()), &ParseOptions::default()).is_ok());
    let e = parser.parse(&mut ast, JsStr::Latin1(nest(MAX_AST_DEPTH).as_bytes()), &ParseOptions::default()).unwrap_err();
    assert_eq!(e.kind, SyntaxKind::TooDeep);
    // operators and parentheses cost no level
    for f in ["(".repeat(5000) + "1" + &")".repeat(5000), "1+".repeat(5000) + "1", "1^(".repeat(5000) + "1" + &")".repeat(5000)] {
        assert!(parser.parse(&mut ast, JsStr::Latin1(f.as_bytes()), &ParseOptions::default()).is_ok());
    }
    // a failed parse rolls the arena back
    let before = ast.len();
    assert!(parser.parse(&mut ast, JsStr::Latin1(b"1+(2*"), &ParseOptions::default()).is_err());
    assert_eq!(ast.len(), before);
}

#[test]
fn random_text_no_panic() {
    on_big_stack(|| {
        const ALPH: [&str; 60] = [
            "(",
            ")",
            "\"",
            "'",
            "[",
            "]",
            "{",
            "}",
            ",",
            ";",
            ":",
            "!",
            "#",
            "@",
            "$",
            ".",
            " ",
            "-",
            "+",
            "*",
            "/",
            "^",
            "&",
            "%",
            "=",
            "<",
            ">",
            "<>",
            "A1",
            "B2:C3",
            "R",
            "C",
            "R1C1",
            "R[-1]",
            "1",
            "0.5",
            "1e5",
            "E",
            "TRUE",
            "x",
            "_xlpm.",
            "_xlfn.",
            "LET(",
            "LAMBDA(",
            "SUM(",
            "Tbl[",
            "#N/A",
            "Sheet1!",
            "'a b'!",
            "[1]",
            "\u{a0}",
            "\t",
            "\u{3000}",
            "\u{e9}",
            "\u{1F600}",
            ":.",
            ".:",
            "\u{0}",
            "@[",
            "[#This Row]",
        ];
        let mut rng = Rng(0xa2a2_5eed_0001);
        let mut parser = FormulaParser::new();
        let mut st = EvalState::new();
        let (mut parsed, mut worst) = (0, Duration::ZERO);
        for i in 0..20_000 {
            let n = 1 + rng.below(24);
            let mut u: Vec<u16> = Vec::new();
            for _ in 0..n {
                if rng.below(10) == 0 {
                    // a raw unit, lone surrogates included
                    u.push((rng.next() & 0xffff) as u16);
                } else {
                    u.extend(ALPH[rng.below(ALPH.len())].encode_utf16());
                }
            }
            let t = Instant::now();
            if exercise(&u, i % 3 == 0, &mut parser, &mut st) {
                parsed += 1;
            }
            worst = worst.max(t.elapsed());
        }
        eprintln!("random formulas: {parsed} of 20000 parsed, worst {worst:?}");
        assert!(parsed > 300);
        assert!(worst < Duration::from_secs(1), "{worst:?}");
    });
}

/// M1: parse, print and evaluate drop everything (the arena, the state, the texts); and evaluating scalar
/// operator formulas allocates nothing once the state is warm (PERF: the pass arenas keep their capacity).
#[test]
fn memory_m1_and_zero_alloc_eval() {
    assert!(uo_alloc::is_counting());
    // the function dispatch table (A4) is built once per process, like the static collation tables
    let _ = uo_calc::functions::body(uo_calc::fnreg::FnIx(0));
    for f in ["1+2*3", "LET(x,A1,y,x&\"ab\",y&y)", "{1,2;3,4}+A1:B2", "LAMBDA(x,x*2)(3)", "SUM(1,2)", "(A1:B2,C3)", "Tbl[@x]"] {
        uo_alloc::assert_no_leak(|| {
            let mut ast = Ast::new();
            let n = FormulaParser::new().parse(&mut ast, JsStr::Latin1(f.as_bytes()), &ParseOptions::default()).unwrap();
            let t = print_formula(&ast, n, &PrintOptions { ui: true, ..Default::default() });
            let mut host = EmptyHost::default();
            let mut st = EvalState::new();
            let r = Evaluator { ast: &mut ast, host: &mut host, st: &mut st }.eval_formula(n, SheetIx(0), 0, 0, true);
            drop(r);
            drop(t);
        });
    }
    let mut ast = Ast::new();
    let mut parser = FormulaParser::new();
    let formulas: Vec<_> =
        ["1+2*3-A1/4^2", "-A1%+B2", "A1=B1", "\"abc\"<\"abd\"", "A1&\"x\"&\"y\"", "LET(x,2,y,x*3,x+y)", "((((A1))))+1", "1+1+1+1+1+1+1+1"]
            .iter()
            .map(|f| parser.parse(&mut ast, JsStr::Latin1(f.as_bytes()), &ParseOptions::default()).unwrap())
            .collect();
    let mut host = EmptyHost::default();
    let mut st = EvalState::new();
    let run = |ast: &mut Ast, host: &mut EmptyHost, st: &mut EvalState| {
        for &n in &formulas {
            let r = Evaluator { ast: &mut *ast, host: &mut *host, st: &mut *st }.eval_formula(n, SheetIx(0), 0, 0, true);
            assert!(r.is_ok());
        }
        st.clear_pass();
    };
    run(&mut ast, &mut host, &mut st); // warm-up: the pass arenas grow once
    let u = uo_alloc::assert_no_leak(|| run(&mut ast, &mut host, &mut st));
    assert_eq!(u.allocated, 0, "scalar evaluation allocated {} B", u.allocated);
}
