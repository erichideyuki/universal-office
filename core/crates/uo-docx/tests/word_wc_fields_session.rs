//! WordCraft study (fields; record `core/docs/modules/uo-docx-layout.md` "WordCraft study — fields"): the session halves
//! of B25 (the table formula texts and ROUND, `OpenOptions::ts_formula_errors`) and of B26/B27 (the list words and
//! `w:isLgl` in the session's outline, TOC sources and cross-reference items, `OpenOptions::ts_numbering`). Word's rules
//! by default; the switches give the frozen TS's back.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use uo_docx::ids::IdAlloc;
use uo_docx::session::ops::tables::formula::{FormulaEnv, formula_instr, formula_result_text};
use uo_docx::session::ops::tables::table_ops::{NewTableOptions, new_table};
use uo_docx::session::{FixedEnv, OpenOptions, Session};

/// The result text of `formula` in an empty 2 × 2 table's first cell, with Word's rules (`word`) or the TS's.
fn formula_text(formula: &str, word: bool) -> String {
    let mut ids = IdAlloc::new();
    let t = new_table(2.0, 2.0, &mut NewTableOptions::new(9000.0), &mut ids);
    let f: Vec<u16> = formula.encode_utf16().collect();
    let instr = formula_instr(&f, None);
    formula_result_text(&instr, &FormulaEnv { table: &t, r: 0, c: 0, bookmarks: None, word }).unwrap()
}

/// The measured `hf-pt-BR` rows of `core/target/wordcraft-study/fields/fixtures/word-fields-dump.tsv` (Word 365 pt-BR,
/// the formula in a header: no table, no bookmark — the same as an empty table's cell here), each with Word's result and
/// the frozen TS's. Out: `= 5 \* CardText` (Word: "Cinco"; the table formula's instruction takes a `\#` picture only and
/// the session does not know the field run's language the words follow — a syntax error under both rules).
const HF_ROWS: [(&str, &str, &str); 51] = [
    ("= SUM(1;2;3)", "6", "6"),
    ("= AVERAGE(1;2)", "1,5", "1,5"),
    ("= MAX(1;5;3)", "5", "5"),
    ("= MIN(4;2)", "2", "2"),
    ("= PRODUCT(2;3)", "6", "6"),
    ("= SIGN(-2)", "-1", "-1"),
    ("= SIGN(0)", "0", "0"),
    ("= IF(1>0;5;6)", "5", "5"),
    ("= IF(1<0;5;6)", "6", "6"),
    ("= AND(1;0)", "0", "0"),
    ("= OR(0;1)", "1", "1"),
    ("= NOT(1)", "0", "0"),
    ("= TRUE", "1", "1"),
    ("= FALSE", "0", "0"),
    ("= COUNT(1;2;3)", "3", "3"),
    ("= DEFINED(nada)", "0", "!Erro de sintaxe"),
    ("= 2^3", "8", "8"),
    ("= 1=1", "1", "1"),
    ("= 3<>3", "0", "0"),
    ("= 50%", "0,5", "0,5"),
    ("= 200*10%", "20", "20"),
    ("= INT(7,9)", "7", "7"),
    ("= ROUND(2,345;2)", "2,35", "2,35"),
    ("= ROUND(-2,345;2)", "-2,35", "-2,35"),
    ("= ROUND(1234,5;-2)", "1200", "1200"),
    ("= MOD(7;-3)", "1", "1"),
    ("= MOD(7,5;2)", "1,5", "1,5"),
    ("= ABS(-3)", "3", "3"),
    ("= 1/0", "!Divisão por zero", "!Divisão por zero"),
    ("= SUM(", "!Fim imprevisto da fórmula", "!Fim imprevisto da fórmula"),
    ("= 2 3", "!Está faltando operador", "!Operador ausente"),
    ("= FOO(1)", "!Erro de sintaxe, 1", "!Erro de sintaxe"),
    ("= nada", "!Indicador não definido, NADA", "!Erro de sintaxe"),
    ("= (1+2", "!Fim imprevisto da fórmula", "!Fim imprevisto da fórmula"),
    ("= 1,2,3", "!Está faltando operador", "!Operador ausente"),
    ("= 1.000,5", "1.000,5", "1000,5"),
    ("= 1.5", "!Erro de sintaxe, .", "15"),
    ("= 3 \\# \"0,00\"", "3,00", "3,00"),
    ("= INT(-2.5)", "!Erro de sintaxe, .", "-25"),
    ("= SUM(1,2)", "!Erro de sintaxe, 1,2", "1,2"),
    ("= 1;2", "!Erro de sintaxe, ;", "!Operador ausente"),
    ("= int(2,5)", "2", "2"),
    ("= Round(2,5;0)", "3", "3"),
    ("= -2,5 \\# \"0,0\"", "-2,5", "-2,5"),
    ("= 1234,5 \\# \"#.##0,00\"", "1.234,50", "1.234,50"),
    ("= 0,5 \\# \"0%\"", "1%", "1%"),
    ("= 7 \\# \"00%\"", "07%", "07%"),
    ("= MAX()", "!Erro de sintaxe, )", "0"),
    ("= 1+", "!Fim imprevisto da fórmula", "!Fim imprevisto da fórmula"),
    ("= )", "!Erro de sintaxe, )", "!Erro de sintaxe"),
    ("= 2*(3", "!Fim imprevisto da fórmula", "!Fim imprevisto da fórmula"),
];

/// B25 (`fields/wcfld3/hf-*`, pt-BR Word): Word's error texts name the offending token or the bookmark (any name that is
/// no function or cell), a left-over operand is "!Está faltando operador", a left-over separator the syntax error at it,
/// ROUND rounds half away at the decimal digits; with `word: false`
/// (`OpenOptions::ts_formula_errors`) the frozen TS's texts and ROUND stay.
#[test]
fn b25_table_formula_texts_and_round_behind_the_ts_switch() {
    let cases = [
        ("= 2 3", "!Está faltando operador", "!Operador ausente"),
        ("= 1 +", "!Fim imprevisto da fórmula", "!Fim imprevisto da fórmula"),
        ("= NADA + 1", "!Indicador não definido, NADA", "!Erro de sintaxe"),
        ("= 1;2", "!Erro de sintaxe, ;", "!Operador ausente"),
        ("= )", "!Erro de sintaxe, )", "!Erro de sintaxe"),
        ("= nada", "!Indicador não definido, NADA", "!Erro de sintaxe"),
        // (2,345 × 100 is 234,5 in binary: the TS rounds it up too, measured Word 2,35; 1,005 × 100 is 100,4999…: Word's
        // rule — half away at the decimal digits — gives 1,01, the TS's binary product 1)
        ("=ROUND(2,345;2)", "2,35", "2,35"),
        ("=ROUND(-2,345;2)", "-2,35", "-2,35"),
        ("=ROUND(1234,5;-2)", "1200", "1200"),
        ("=ROUND(1,005;2)", "1,01", "1"),
        ("=SUM(1;2;3)", "6", "6"),
    ];
    let cases = cases.into_iter().chain(HF_ROWS);
    let mut bad = Vec::new();
    for (f, word, ts) in cases {
        let (w, t) = (formula_text(f, true), formula_text(f, false));
        if w != word || t != ts {
            bad.push(format!("{f}: Word's rules [{w}] want [{word}]; TS [{t}] want [{ts}]"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// B25: the table names under Word's grammar (the header/footer parser with the session's cells, ranges and directions):
/// the TS's sums, ranges and currency where Word's rows say nothing else; a list where one number is due, a range
/// whose second end is no cell and `DEFINED` of a cell (no bookmark) follow Word's parser. (Not measured in a table:
/// the `hf-*` rows have none.)
#[test]
fn b25_table_names_under_word_rules() {
    use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
    use uo_docx::session::{CellHop, Command, Outcome};
    use uo_docx::text::Utf16;
    let preview = |ts_formula_errors: bool, formulas: &[&str]| -> Vec<String> {
        let mut cx = crate::d2a_support::Cx::new();
        let o = OpenOptions { ts_formula_errors, ..OpenOptions::default() };
        let mut s = Session::new_blank(o, Box::new(FixedEnv::default())).unwrap();
        let mut run = |s: &mut Session, c: Command| assert!(matches!(s.exec(c, &mut cx.cx()).unwrap(), Outcome::Done(_)));
        let layout = TableLayoutIn::default();
        run(&mut s, Command::Table(TableCmd::InsertTable { rows: 3.0, cols: 2.0, autofit: None, col_width_twips: None, layout }));
        let enter = |r: u32, c: u32| {
            Command::Context(uo_docx::session::ops::context::ContextCmd::EnterCell {
                anchor: 0,
                hops: vec![CellHop { row: r, col: c, block: None }],
                caret: None,
            })
        };
        for (r, c, t) in [(0, 0, "10"), (0, 1, "20"), (1, 0, "5,5"), (1, 1, "R$ 3")] {
            run(&mut s, enter(r, c));
            run(&mut s, Command::Text(uo_docx::session::ops::text::TextCmd::Type { text: Utf16::from(t) }));
        }
        run(&mut s, enter(2, 0));
        formulas.iter().map(|f| s.preview_formula(f, None).unwrap_or_else(|| "<none>".into())).collect()
    };
    let cases = [
        ("=SUM(ABOVE)", "15,5", "15,5"),
        ("=ABOVE", "15,5", "15,5"),
        ("=SUM(A1:B2)", "R$ 38,5", "R$ 38,5"),
        ("=sum(a1:b1)+A2", "35,5", "35,5"),
        ("=A1:B1", "30", "30"),
        ("=MAX(A1:B2;1)", "R$ 20,0", "R$ 20,0"),
        ("=COUNT(ABOVE;A1)", "3", "3"),
        ("=AVERAGE(LEFT)", "!Divisão por zero", "!Divisão por zero"),
        ("=ABOVE*2", "!Erro de sintaxe, ABOVE", "!Erro de sintaxe"),
        ("=A1:5", "!Erro de sintaxe, 5", "!Erro de sintaxe"),
        ("=DEFINED(A1)", "0", "1"),
        ("=B2+1", "R$ 4", "R$ 4"),
    ];
    let fs: Vec<&str> = cases.iter().map(|c| c.0).collect();
    let (w, t) = (preview(false, &fs), preview(true, &fs));
    let mut bad = Vec::new();
    for (((f, word, ts), w), t) in cases.iter().zip(&w).zip(&t) {
        if w != word || t != ts {
            bad.push(format!("{f}: Word's rules [{w}] want [{word}]; TS [{t}] want [{ts}]"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

fn oracle_doc(group: &str, name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../uo-docx-layout/tests/oracle_wc").join(group).join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The numbered cross-reference items (number + text) and the headings of a document, Word's rules or the TS's.
fn items(bytes: &[u8], ts_numbering: bool) -> (Vec<String>, Vec<String>) {
    let o = OpenOptions { ts_numbering, ..OpenOptions::default() };
    let s = Session::open(bytes.to_vec(), o, Box::new(FixedEnv::default())).unwrap();
    let numbered = s
        .cross_reference_items(uo_docx::session::ops::references::CrossRefType::Numbered, None)
        .into_iter()
        .map(|i| uo_jsnum::text::trim(&String::from_utf16_lossy(i.text.units())).to_owned())
        .collect();
    let headings = s.headings().into_iter().map(|h| String::from_utf16_lossy(h.text.units())).collect();
    (numbered, headings)
}

/// Word's line texts of an oracle case (`.word.json`: every line of these documents is one list paragraph).
fn word_lines(group: &str, stem: &str) -> Vec<String> {
    let json = String::from_utf8(oracle_doc(group, &format!("{stem}.word.json"))).unwrap();
    json.split("\"text\": \"").skip(1).map(|t| t.split('"').next().unwrap_or("").to_owned()).collect()
}

/// B26/B27 in the session: the numbered cross-reference items (number + text, spaces dropped) of a cardinalText /
/// ordinalText list in an en-US document and of isLgl lists equal Word's list strings (`ListString`, the lines of the
/// oracle cases) under Word's rules; `ts_numbering` gives the TS's (pt-BR words up to 99, the level's own format under
/// isLgl) back, which Word does not write.
#[test]
fn b26_b27_session_items_follow_word_unless_ts_numbering() {
    let squash = |v: Vec<String>| {
        v.into_iter().map(|s| s.chars().filter(|c| !uo_jsnum::text::is_js_space(*c)).collect::<String>()).collect::<Vec<_>>()
    };
    let mut report = Vec::new();
    for (group, stem) in [("lists26", "b26-en-US"), ("lists27", "b27-lgl-own"), ("lists27", "b27-lgl-letter")] {
        let bytes = oracle_doc(group, &format!("{stem}.docx"));
        let word = word_lines(group, stem);
        let (w, _) = items(&bytes, false);
        let (t, _) = items(&bytes, true);
        let (w, t) = (squash(w), squash(t));
        if std::env::var_os("UO_WC_VERBOSE").is_some() {
            println!("{stem}: Word {word:?}\n{stem}: Word's rules {w:?}\n{stem}: ts_numbering {t:?}");
        }
        if w != word {
            report.push(format!("{stem}: Word's rules {w:?}, Word {word:?}"));
        }
        if t == word {
            report.push(format!("{stem}: ts_numbering equals Word ({t:?}): the switch gives nothing back"));
        }
    }
    assert!(report.is_empty(), "{}", report.join("\n"));
}
