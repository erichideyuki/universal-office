//! Fuzz target of the `uo-calc` cell store (record `core/docs/modules/uo-calc.md` "Security and fuzz targets",
//! `store_ops`): the bytes are a sequence of store operations (file cells of every type and hostile text, removes,
//! engine writes, styles, owned bits, dense runs across chunk boundaries, gathers and column runs, compaction)
//! interpreted by `crates/uo-calc/tests/support/ops.rs`, shared with the seeded test, and checked differentially
//! against a `BTreeMap` model: same file cells, same `cellValue`, same iteration; invariants (sorted non-empty
//! chunks, lane lengths, side-table bits, pool reference counts = holders) every 64 operations.

#[path = "../../crates/uo-calc/tests/support/ops.rs"]
mod ops;

/// Seeds: empty, each operation once, a dense run over a chunk boundary, and every byte.
pub fn calc_seeds() -> Vec<Vec<u8>> {
    let mut out = vec![vec![], vec![0, 0, 0, 1, 2, 3]];
    for op in 0u8..10 {
        out.push(vec![op, 4, 1, 3, 5, 7, 9, op, 2, 4]);
    }
    out.push(vec![7, 3, 0, 200, 8, 3, 0, 9, 0, 0, 0]);
    out.push((0..=255u8).cycle().take(2048).collect());
    out
}

pub fn store_ops(data: &[u8]) {
    let _ = ops::run(data);
}

#[path = "../../crates/uo-calc/examples/stack_common/mod.rs"]
mod stack_common;

/// Seeds of `formula_parse`: formulas of every syntax (record "A2 — as built").
pub fn formula_seeds() -> Vec<Vec<u8>> {
    let texts: [&str; 16] = [
        "1+2*3^-4%",
        "SUM(A1:B2,Sheet1!$C$3,'My Sheet'!D4:E5)",
        "_xlfn.LET(_xlpm.x,1,_xlpm.x+A1#)",
        "LAMBDA(x,[y],ISOMITTED(y))(1)",
        "Tbl[[#Headers],[Id]:[Name]]&[@Val]",
        "{1,-2;\"a\",#N/A}",
        "R[-1]C[2]+RC1",
        "A1:.A10 B:B (A1,B2)",
        "'[1]Ext'!A1+[1]!Name+!A1",
        "\"a\"\"b\"&TRUE&#ſpill!",
        "1e309+12345678901234567890",
        "((((1))))",
        "@A1:A3+_xleta.SUM",
        "Sheet1:Data!A1",
        "=1+(2",
        "LET(f,LAMBDA(g,g(g)),f(f))",
    ];
    let mut out: Vec<Vec<u8>> = Vec::new();
    for (i, t) in texts.iter().enumerate() {
        let mut v = vec![(i as u8) & 0x0f];
        v.extend_from_slice(t.as_bytes());
        out.push(v);
    }
    out
}

/// Fuzz target `formula_parse` (record `uo-calc.md` "Security and fuzz targets"): byte 0 chooses the options (bit 0
/// R1C1, bit 1 the pt-BR letters, bit 2 a far host cell, bit 3 UTF-16 text), the rest is the formula text (Latin-1
/// bytes or UTF-16LE units, lone surrogates included). Parses, prints the four forms, re-parses the file form and
/// evaluates the tree over an empty workbook in both array contexts: no panic, bounded by the depth limits.
pub fn formula_parse(data: &[u8]) {
    use uo_calc::JsStr;
    use uo_calc::ast::Ast;
    use uo_calc::eval::{EvalState, Evaluator};
    use uo_calc::parse::{FormulaParser, ParseOptions};
    use uo_calc::print::{PrintOptions, print_formula};
    let Some((&flags, rest)) = data.split_first() else { return };
    let units: Vec<u16> = if flags & 8 != 0 {
        rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
    } else {
        rest.iter().map(|&b| u16::from(b)).collect()
    };
    let (l, c) = (JsStr::Latin1(b"L"), JsStr::Latin1(b"C"));
    let (row, col) = if flags & 4 != 0 { (1_048_575, 16_383) } else { (3, 2) };
    let o = ParseOptions { r1c1: flags & 1 != 0, letters: (flags & 2 != 0).then_some((l, c)), row, col };
    let mut ast = Ast::new();
    let mut parser = FormulaParser::new();
    let Ok(n) = parser.parse(&mut ast, JsStr::Utf16(&units), &o) else { return };
    let mut file = None;
    for (r1c1, ui) in [(false, false), (false, true), (true, false), (true, true)] {
        let t = print_formula(&ast, n, &PrintOptions { row, col, r1c1, ui });
        if !r1c1 && !ui {
            file = Some(t);
        }
    }
    if let Some(t) = file {
        let mut a2 = Ast::new();
        let _ = parser.parse(&mut a2, t.as_js(), &ParseOptions { row, col, ..Default::default() });
    }
    let mut host = stack_common::EmptyHost::default();
    let mut st = EvalState::new();
    for arr in [true, false] {
        let _ = Evaluator { ast: &mut ast, host: &mut host, st: &mut st }.eval_formula(n, uo_calc::SheetIx(0), row, col, arr);
        st.clear_pass();
    }
}
