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
    let texts: &[&str] = &[
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
        // A4a: the math family (lifts, ATP rules, complex texts, units, hostile loops and shapes)
        "ROUND(2.675,2)+MOD(-7,3)+POWER(-8,1/3)+CEILING.MATH(-2.5,,1)+FLOOR(1E300,3)",
        "SUM(A:A)+SUMPRODUCT({1,2;3,4},{5,6;7,8})+SUMX2MY2(A1:A3,{1;2;3})",
        "BESSELJ(1,1E300)+BESSELY(1E300,2^53)+BESSELI(1,2^60)+BESSELK(2,1E21)",
        "IMSUM(\"3+4i\",\"1-2j\",A1:B2)&IMPOWER(\"-4\",0.5)&COMPLEX(1,2,\"j\")",
        "CONVERT(1,\"kconstructor\",\"constructor\")+CONVERT(1,\"Gibyte\",\"bit\")",
        "MMULT(MUNIT(300),MUNIT(300))+MINVERSE({1,2;3,4})+MDETERM({1,2;3,4})",
        "DECIMAL(\"\u{fb00}\",36)+ARABIC(\"-MCMXCIV\")&ROMAN(3999,4)&BASE(255,16,10)",
        "HEX2DEC(\"FFFFFFFFFF\")+BITLSHIFT(1,53)+GCD(A1:A3,12)+LCM(4,6)+COMBINA(10,3)+FACTDOUBLE(301)",
        "SIN(A1:A1048576+0*A1:XFD1)+RANDBETWEEN({1,2},6)+SERIESSUM(2,1,2,{1,2,3})",
        // A4b: the statistics family (order statistics, AGGREGATE/SUBTOTAL, iterative inverses and discrete loops at
        // their caps, the ETS and regression shapes, the tests over whole columns)
        "MEDIAN(A1:A10,{1;2;3})+PERCENTILE.EXC(A:A,0.3)+QUARTILE(A1:C3,1)+PERCENTRANK.INC({1,2,3},2.5,20)+RANK.AVG(A1,A1:A10,1)+TRIMMEAN(A1:A9,0.4)+GEOMEAN(1E300,1E300,1E-300)",
        "AGGREGATE(14,6,A1:A10/(A1:A10>0),{1;2})+SUBTOTAL(109,A1:A10)+AGGREGATE(3,7,A1:B5)+SUM(MODE.MULT(1,1,2,2))+SUM(FREQUENCY({1;2;3},{2;1}))+COUNTBLANK(A1:C3)+COUNTA(A:A,,\"x\")",
        "T.INV(1E-300,1)+BETA.INV(0.5,1E-300,1E300)+GAMMA.INV(0.999999,1E10,1E-10)+CHISQ.INV.RT(1E-300,1E10)+F.INV.RT(0.5,1,1E9)+BINOM.INV(2^31-2,0.5,0.5)+T.INV.2T(1E-300,1E10)",
        "HYPGEOM.DIST(1E6,2E6,2E6,2^31-2,TRUE)+BINOM.DIST.RANGE(2^30,0.5,0,2^30)+NEGBINOM.DIST(1E9,1,0.5,TRUE)+PERMUT(1E300,5)+POISSON.DIST(1E9,1E9,TRUE)+PERMUTATIONA(2^53,2^53)",
        "FORECAST.ETS.CONFINT(1E21,{1;2;3;4;5;6},{1;2;3;4;5;6})+FORECAST.ETS(10,A1:A100,B1:B100,8760,0,7)+FORECAST.ETS.STAT({1;2;3},{1;2;1E9},4)+FORECAST.ETS.SEASONALITY(A:A,B:B)",
        "SUM(LINEST(A1:A20,B1:E20,FALSE,TRUE))+SUM(TREND({1;2;3},{1;2;3},{4,5;6,7}))+SUM(LOGEST({1,2,4},{1,2,3},TRUE,TRUE))+SUM(GROWTH(A:A,B:B))+CORREL(A:A,B:B)+T.TEST(A1:A5,B1:B5,2,3)+CHISQ.TEST(A1:B3,C1:D3)",
        // A4c: the finance/date family (solvers, unbounded TS loops at their budgets, coupon walks over NaN fields,
        // the ATP lifts and holidays)
        "RATE(1E300,-1,1E-300)+IRR({-1,1E300,-1E300})+XIRR({-1,1},{1,2E9},{1})+MIRR(A:A,0.1,0.1)+NPV(-1,A1:A3,,{1,2})+FVSCHEDULE(1,A1:B2)",
        "CUMIPMT(0.01,1E21,1,1,1E21,0)+VDB(1,0,1E21,0,1E21,2,TRUE)+DB(1,2,1E300,1E300)+AMORDEGRC(1,1,2,0,1E15,1E-300,1)+DDB(1,0,1E300,1E300)",
        "COUPNCD(1,1E21,2)+COUPDAYSNC(1E300,2E300,4,1)+YIELD(1,2958465,1,1,1E300,4)+ODDFYIELD(1,1E8,-1E300,2,0,1,1,4)+ODDLPRICE(2,1E8,1,1,1,1,4)+ACCRINT(1,1E8,1E8+1,1,1,4)",
        "WORKDAY(1,1E300,A1:A3)+WORKDAY.INTL(1,-1E300,\"0101010\",{1;2})+NETWORKDAYS.INTL({1,2},A1:A3,17,B:B)+EDATE({1;60},{1,2})+EOMONTH(2958465,1)+YEARFRAC(1,2958465,1)",
        "DATEDIF(1,2958465,\"yd\")+DATEVALUE(\"29/02/1900 25:00\")+TIME(-40000,1E9,-1)+WEEKNUM(60,21)+DAYS360(59,60)+TODAY()-NOW()+DATE(1E300,1,1)",
        // A4d: the text family (UTF-16 positions, wildcards, the format-code translation, splits over long texts,
        // catastrophic regexes at their budget, hostile XML/XPath, the bounded TS hangs)
        "LEFT(\"a\u{1F600}b\",2)&MID(\"\u{e9}\u{1F600}z\",2,1)&RIGHTB(\"\u{4e2d}a\",1)&MIDB(\"\u{e9}\u{1F600}z\",1,2)&LEN(REPT(\"ab\",16383))&FIND(\"b\",\"ab\",0/0)&SEARCH(\"b*~?\",\"ab*?c\")&SUBSTITUTE(\"aaaa\",\"aa\",\"b\",2)&REPLACE(\"abc\",2,1E21,\"x\")&REPT(\"\",1E300)",
        "TEXT(45000.5,\"dd/mm/aaaa hh:mm:ss,00\")&TEXT(-1,\"[Vermelho]#.##0,00;[Cor10](0)\")&TEXT(1E21,\"0,00E+00\")&TEXT(1,\"[$-constructor]0\")&FIXED(1E300,-300)&DOLLAR(-0.001,127)&NUMBERVALUE(\" 1.234,5 %%\",\",\",\".\")&VALUE(\"1 1/2\")",
        "TEXTSPLIT(REPT(\"a,b;\",8000),\",\",\";\",TRUE)&TEXTBEFORE(\"a-b-c\",\"-\",-1)&TEXTAFTER(\"aXbxc\",\"x\",2,1)&TEXTJOIN(\",\",FALSE,A:A,{1,2})&CONCAT(A1:C3)&ARRAYTOTEXT({1,\"a\";TRUE,#N/A},1)&VALUETOTEXT(\"q\"\"\",1)",
        "REGEXTEST(REPT(\"a\",40),\"(a+)+b\")&REGEXREPLACE(\"abc\",\"(?<n>b)\",\"[${n}$0\\1]\")&REGEXEXTRACT(\"x1y22\",\"\\d+\",1)&REGEXEXTRACT(\"ab\",\"(a)|(x)\",2)&REGEXTEST(\"a\",\"\\p{L\")&REGEXTEST(REPT(\"a\",30000),\"(.*a){20}\")&REGEXTEST(\"aB\",\"a(?i)b\")",
        "FILTERXML(\"<r><a k='1'>x<c>y</c>z</a><a>2</a></r>\",\"//a[@k='1']|//a[last()]/@k|/r/*[position()>=1 and . != '']\")&FILTERXML(REPT(\"<a>\",3000)&REPT(\"</a>\",3000),\"//*//*//*//*\")&FILTERXML(\"<a>&#1114112;</a>\",\"/a\")",
        "BAHTTEXT(1.7976931348623157E308)&BAHTTEXT(-1234.56)&UNICHAR(65535)&UNICODE(\"\u{1F600}\")&CHAR(128)&CODE(\"\u{20ac}\")&ENCODEURL(\"\u{e7}\u{e3}o ~\")&PROPER(\"o'neil 2nd\")&UPPER(\"stra\u{df}e\")&LOWER(\"\u{130}\u{3a3}\u{391}\u{3a3}\")&TRIM(\"  a  b  \")&THAIDIGIT(123)&NUMBERSTRING(1E21,1)&DATESTRING(45000)",
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

/// Seeds of `calc_workbook`: short and long byte streams (every choice reads one byte, zeros past the end).
pub fn calc_workbook_seeds() -> Vec<Vec<u8>> {
    let mut out = vec![vec![], vec![1, 2, 3, 4, 5, 6, 7, 8]];
    for k in 0u8..8 {
        out.push((0..512u32).map(|i| (i as u8).wrapping_mul(31).wrapping_add(k.wrapping_mul(17))).collect());
    }
    out.push((0..=255u8).cycle().take(4096).collect());
    out
}

/// The byte stream of `calc_workbook`: one byte per choice, zeros past the end.
struct Bytes<'a> {
    data: &'a [u8],
    i: usize,
}

impl Bytes<'_> {
    fn b(&mut self) -> u8 {
        let x = self.data.get(self.i).copied().unwrap_or(0);
        self.i += 1;
        x
    }
}

fn wb_col(c: u8) -> char {
    char::from(b'A' + c % 8)
}

fn wb_formula(b: &mut Bytes<'_>, h: u8, w: u8) -> String {
    let r = |b: &mut Bytes<'_>| format!("{}{}", wb_col(b.b() % w), 1 + u32::from(b.b() % h));
    let (x, y) = (r(b), r(b));
    match b.b() % 12 {
        0 => format!("{x}+{y}"),
        1 => format!("{x}*{y}-1"),
        2 => format!("{x}&\"a\""),
        3 => format!("-{x}%"),
        4 => format!("{x}:{y}*1"),
        5 => format!("{x}:{y} {y}"),
        6 => format!("_xlfn.ANCHORARRAY({x})+1"),
        7 => "{1,2;3,4}".to_owned(),
        8 => format!("Nm+{x}"),
        9 => format!("S1!{x}/{y}"),
        10 => format!("({x},{y})"),
        _ => format!("{x}^{y}"),
    }
}

/// Fuzz target `calc_workbook` (record `uo-calc.md` "Security and fuzz targets", frontier A3): the bytes choose a
/// workbook (1–3 sheets of operator formulas over random cells — cycles, CSE areas, dynamic spills into each other and
/// into merges, ANCHORARRAY, names, duplicates, iteration on/off, ignoreCache) and a script of edits; after every call
/// the engine's and the store's invariants must hold. No panic, bounded time (cells ≤ 24 × 8 per sheet).
pub fn calc_workbook(data: &[u8]) {
    use uo_calc::cell::FileCell;
    use uo_calc::engine::{CalcOptions, CellInput, Engine, EngineInput, EngineOptions, FormulaInput, NameInput};
    use uo_calc::formula::{FormulaAttrs, FormulaType};
    use uo_calc::store::RowMeta;
    use uo_calc::{Clock, JsString, RandomSource, Scalar, SheetIx, Store, ValueLocale};
    let mut b = Bytes { data, i: 0 };
    let js = |s: &str| JsString::from_str_js(s);
    let ns = 1 + b.b() % 3;
    let (h, w) = (1 + b.b() % 24, 1 + b.b() % 8);
    let mut st = Store::new();
    for _ in 0..ns {
        let Ok(s) = st.add_sheet() else { return };
        let Ok(mut l) = st.load_sheet(s) else { return };
        let n = usize::from(b.b()) % 64;
        let mut row = None;
        for _ in 0..n {
            let (r, c) = (u32::from(b.b() % h), b.b() % w);
            if row != Some(r) {
                let _ = l.row(r, RowMeta::default());
                row = Some(r);
            }
            let fc = match b.b() % 8 {
                0..=2 => FileCell { v: Some(js(&format!("{}", b.b() % 9))), ..FileCell::default() },
                3 => FileCell { t: Some(js("str")), v: Some(js("x")), ..FileCell::default() },
                4 | 5 => FileCell { f: Some(js(&wb_formula(&mut b, h, w))), ..FileCell::default() },
                6 => {
                    let rf = format!("{}{}:{}{}", wb_col(c), r + 1, wb_col(c + b.b() % 2), r + 1 + u32::from(b.b() % 3));
                    FileCell {
                        f: Some(js(&wb_formula(&mut b, h, w))),
                        fa: Some(FormulaAttrs { t: Some(FormulaType::Array), r#ref: Some(js(&rf)), ..FormulaAttrs::default() }),
                        ..FileCell::default()
                    }
                }
                _ => FileCell {
                    f: Some(js(&wb_formula(&mut b, h, w))),
                    fa: Some(FormulaAttrs { t: Some(FormulaType::Array), ..FormulaAttrs::default() }),
                    cm: Some(1.0),
                    ..FileCell::default()
                },
            };
            let _ = l.cell(u32::from(c), &fc);
        }
        let _ = l.finish();
    }
    if b.b() % 3 == 0
        && let Some(sh) = st.sheets.first_mut()
    {
        sh.merges.push(uo_calc::Area { sheet: SheetIx(0), r1: 1, c1: 1, r2: 2, c2: 1 });
    }
    let input = EngineInput {
        sheet_names: (0..ns).map(|i| js(&format!("S{}", i + 1))).collect(),
        names: vec![NameInput { name: js("Nm"), formula: js("S1!$A$1"), sheet: None }],
        calc: CalcOptions { iterate: b.b() % 2 == 0, iterate_count: Some(f64::from(b.b() % 40)), iterate_delta: Some(0.01) },
        dynamic_cm: vec![1.0],
        ..EngineInput::default()
    };
    let mut o = EngineOptions::new(ValueLocale::en_us());
    o.ignore_cache = b.b() % 4 == 0;
    let Ok(mut e) = Engine::new(&mut st, &input, &o, None) else { return };
    let clock = Clock { now_serial: 45658.5 };
    let mut rnd = RandomSource::seeded(7);
    let check = |e: &Engine, st: &Store| {
        if let Err(m) = e.check_invariants(st) {
            panic!("engine invariant: {m}");
        }
        if let Err(m) = st.check_invariants() {
            panic!("store invariant: {m}");
        }
    };
    let _ = e.recalc(&mut st, clock, &mut rnd);
    check(&e, &st);
    for _ in 0..usize::from(b.b() % 16) {
        let (s, r, c) = (u32::from(b.b() % ns), u32::from(b.b() % (h + 2)), u32::from(b.b() % (w + 2)));
        let x = match b.b() % 6 {
            0 => None,
            1 => Some(CellInput::Value(Scalar::Num(f64::from(b.b())))),
            2 => Some(CellInput::Text(js("t"))),
            3 => Some(CellInput::Formula(FormulaInput { text: js(&wb_formula(&mut b, h, w)), ..FormulaInput::default() })),
            4 => Some(CellInput::Formula(FormulaInput {
                text: js(&wb_formula(&mut b, h, w)),
                array: true,
                dynamic: true,
                ..FormulaInput::default()
            })),
            _ => Some(CellInput::Formula(FormulaInput { text: js(&wb_formula(&mut b, h, w)), ca: true, ..FormulaInput::default() })),
        };
        let _ = e.set_cell(&mut st, SheetIx(s), r, c, x);
        check(&e, &st);
        let _ = e.calculate(&mut st, clock, &mut rnd);
        check(&e, &st);
    }
}
