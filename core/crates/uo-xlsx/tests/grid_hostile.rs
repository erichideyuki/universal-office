//! Hostile input of the grid model (`--features grid`): no panics and bounded work for typed text, widths that
//! are NaN/infinite/huge, damaged workbooks and absurd column definitions.
#![cfg(feature = "grid")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::grid::*;

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

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }
}

fn x1_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks/x1"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

#[test]
fn typed_text_never_panics() {
    // pieces that make entries interesting: signs, separators, currency, exponents, dates, formulas, surrogates
    let alphabet: Vec<Vec<u16>> = [
        "0",
        "1",
        "2",
        "9",
        "00",
        "12",
        "999",
        "-",
        "+",
        "(",
        ")",
        "%",
        ".",
        ",",
        ";",
        " ",
        "  ",
        "/",
        ":",
        "E",
        "e",
        "R$",
        "$",
        "€",
        "#",
        "'",
        "=",
        "\"",
        "[",
        "]",
        "{",
        "}",
        "@",
        "SUM(",
        "SOMA(",
        "A1",
        "A1:B2",
        "TRUE",
        "VERDADEIRO",
        "#N/A",
        "#DIV/0!",
        "jan",
        "AM",
        "PM",
        "Taxa",
        "\u{0301}",
        "ção",
        "\u{20ac}",
        "٣",
    ]
    .iter()
    .map(|s| s.encode_utf16().collect())
    .chain([vec![0xD800], vec![0xDC00], vec![0xD83D, 0xDE00], vec![0], vec![0x2028]])
    .collect();
    let mut rng = Rng(0x2545_F491);
    let langs = [FormulaLocale::pt_br(), FormulaLocale::en_us(), FormulaLocale::for_tag("de-DE", Some(","), None)];
    let cats = [
        FmtCategory::General,
        FmtCategory::Number,
        FmtCategory::Percent,
        FmtCategory::Date,
        FmtCategory::Text,
        FmtCategory::Fraction,
        FmtCategory::Time,
    ];
    let names = ["Taxa", "Total"];
    for k in 0..40_000 {
        let n = rng.below(10);
        let mut s: Vec<u16> = Vec::new();
        for _ in 0..n {
            s.extend_from_slice(&alphabet[rng.below(alphabet.len())]);
        }
        let ctx = EntryContext {
            locale: &langs[k % langs.len()],
            current: cats[rng.below(cats.len())],
            row: rng.below(2_000_000) as u32,
            col: rng.below(20_000) as u32,
            date1904: k % 5 == 0,
            year: 2025.0,
            names: &names,
            in_table: if k % 7 == 0 { Some("Tabela1") } else { None },
        };
        let e = parse_entry(&s, &ctx, &CanonicalSyntax);
        if let Entry::Value { value: EntryValue::Number(v), .. } = &e {
            // the edit text of what was just parsed
            let _ = edit_text(&EditValue::Number(*v), ctx.current, ctx.locale, ctx.date1904, false);
        }
        let _ = auto_close(&s);
    }
}

#[test]
fn layout_with_absurd_widths_and_values() {
    let mut table = MeasureTable::new();
    let id =
        table.font(&FontRequest { name: "Calibri", alt_name: None, size_pt: 11.0, bold: false, italic: false, zoom: 1.0 }, &IdentityEnv);
    let widths = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -5.0, 0.0, 1e300, 1e9, 5.0, 64.0];
    let nums = [f64::NAN, f64::INFINITY, -0.0, 1e308, 5e-324, -1e-300, 123456789012345678.0, 0.1, 1e21];
    let codes: Vec<Option<Vec<u16>>> = [
        None,
        Some("General"),
        Some("0.00"),
        Some("*x@"),
        Some("@*."),
        Some("[Red]0;;;"),
        Some("0.0E+00"),
        Some("# ?/?"),
        Some("dd/mm/yyyy"),
        Some("["),
    ]
    .iter()
    .map(|c| c.map(|s| s.encode_utf16().collect()))
    .collect();
    let mut out = CellText::default();
    for &w in &widths {
        for &n in &nums {
            for c in &codes {
                let inp = CellTextInput {
                    kind: CellKind::Number,
                    num: n,
                    code: c.as_deref(),
                    show_zeros: n != 5e-324,
                    indent_px: w,
                    edge_px: 1.0,
                    ..CellTextInput::default()
                };
                for _ in 0..3 {
                    layout_cell_text(&inp, &table.metrics(id), w, &mut out);
                    if !table.has_needs() {
                        break;
                    }
                    table.satisfy(&Fake);
                }
            }
        }
        let text: Vec<u16> = "x".repeat(40).encode_utf16().collect();
        for c in &codes {
            let inp = CellTextInput { kind: CellKind::Text, str: &text, code: c.as_deref(), ..CellTextInput::default() };
            layout_cell_text(&inp, &table.metrics(id), w, &mut out);
        }
        let mut ranges = Vec::new();
        wrap_lines(&table.metrics(id), &text, w, usize::MAX, &mut ranges);
        wrap_lines(&table.metrics(id), &[], w, 3, &mut ranges);
    }
    // a hash column cannot allocate what the width says
    layout_cell_text(
        &CellTextInput { kind: CellKind::Number, num: 1e300, code: None, ..CellTextInput::default() },
        &table.metrics(id),
        1e12,
        &mut out,
    );
    assert!(out.text.len() <= 65_536);
}

#[test]
fn geometry_with_absurd_sizes() {
    let g = Geometry::build(1_048_576, 20.0, 1.0, &[(1, 1_048_577, f64::NAN)]);
    let _ = (g.bisect_le(1e9), g.bisect_le(f64::NAN), g.edge(g.len() - 1), g.total());
    let g = Geometry::build(10, f64::INFINITY, 3.0, &[(2, 3, f64::NEG_INFINITY), (4, u32::MAX, -1.0)]);
    let _ = (g.bisect_le(0.0), g.bisect_le(-1.0), g.total());
    let mut buf = [0.0; 4];
    g.fill_edges(u32::MAX - 1, &mut buf);
    let _ = Geometry::build(0, 20.0, 1.0, &[]);
}

#[test]
fn damaged_workbooks_do_not_panic_the_grid() {
    let files = x1_files();
    let mut rng = Rng(0x9E37_79B9);
    let mut opened = 0;
    for round in 0..120 {
        let mut bytes = std::fs::read(&files[round % files.len()]).unwrap();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(bytes.len());
            bytes[at] ^= 1 << rng.below(8);
        }
        let Ok(mut book) = Book::open(&bytes, ReadLimits::default()) else { continue };
        let _ = book.load_all();
        opened += 1;
        let styles = StyleTable::build(&book.styles);
        let mut table = MeasureTable::new();
        for si in 0..book.workbook.sheets.len() {
            if book.sheets[si].is_none() {
                continue;
            }
            let mut grid = SheetGrid::open(&book, si, &styles);
            let ctx = CellCtx { date1904: book.workbook.date1904, show_zeros: true, lang: FormulaLang::PtBr, locale: None };
            let mut env = FitEnv { fonts: &mut table, env: &IdentityEnv, ctx };
            let fit = fit_row_heights(&mut book, si, &styles, &grid, &mut env);
            grid.set_row_fit(fit);
            let (c, r) = (grid.col_geometry(1.0), grid.row_geometry(1.0));
            let _ = (c.total(), r.total());
        }
    }
    assert!(opened > 10, "{opened} damaged workbooks still opened");
}
