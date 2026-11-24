//! The engine input of the calc captures: a `CalcWorkbook` (the TS engine input, `workbook.ts`, as the canonical
//! JSON of `capture-order.mjs`) → a store streamed in the workbook's cell order (duplicates and out-of-order rows go
//! through the S0 loader's `FileOrder`) + [`EngineInput`]; and the value text of the captures.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::J;
use uo_calc::a1::a1_to_area;
use uo_calc::cell::{FileCell, InlineText, error_from_text, error_text};
use uo_calc::engine::{CalcOptions, EngineInput, ExtValue, ExternalSheet, NameInput, TableInput};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::store::RowMeta;
use uo_calc::{JsString, Scalar, SheetIx, Store, StrRef};

/// The `cm` the loader gives dynamic arrays.
pub const DYN_CM: f64 = 1.0;

fn js(j: &J) -> JsString {
    j.js().unwrap_or_else(|| panic!("not a string: {j:?}"))
}

fn num_text(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() { "-0".into() } else { uo_jsnum::format::to_js_string(x) }
}

/// A cell value of the JSON (`v`): number, string, boolean, `{code}` error or null.
pub enum V {
    Null,
    Num(f64),
    Str(JsString),
    Bool(bool),
    Err(uo_calc::ErrorCode),
}

pub fn value_of(j: Option<&J>) -> V {
    match j {
        None | Some(J::Null) => V::Null,
        Some(J::Num(x)) => V::Num(*x),
        Some(J::Str(_)) => V::Str(js(j.unwrap())),
        Some(J::Bool(b)) => V::Bool(*b),
        Some(o) => match o.get("code") {
            Some(c) => V::Err(error_from_text(js(c).as_js()).unwrap()),
            None => V::Num(o.num().unwrap()),
        },
    }
}

/// The `FileCell` of a `CellInput` (`{ v, f }`) at `(row, col)`.
pub fn file_cell(row: u32, col: u32, inp: &J) -> FileCell {
    let f = inp.get("f");
    let mut c = FileCell { r#ref: Some(JsString::from_str_js(&uo_calc::cell::a1_ref(row, col))), ..FileCell::default() };
    let formula = f.is_some();
    match value_of(inp.get("v")) {
        V::Null => {}
        V::Num(x) => c.v = Some(JsString::from_str_js(&num_text(x))),
        V::Str(s) if formula => {
            c.t = Some(JsString::from_str_js("str"));
            c.v = Some(s);
        }
        V::Str(s) => {
            c.t = Some(JsString::from_str_js("inlineStr"));
            c.is = Some(InlineText { text: Some(s), runs: None });
        }
        V::Bool(b) => {
            c.t = Some(JsString::from_str_js("b"));
            c.v = Some(JsString::from_str_js(if b { "1" } else { "0" }));
        }
        V::Err(e) => {
            c.t = Some(JsString::from_str_js("e"));
            c.v = Some(JsString::from_str_js(error_text(e)));
        }
    }
    if let Some(f) = f {
        c.f = Some(f.get("text").map_or_else(JsString::default, js));
        let mut a = FormulaAttrs::default();
        let mut any = false;
        if let Some(t) = f.get("t") {
            let t = js(t);
            a.t = Some(match t.as_js().to_string_lossy().as_str() {
                "shared" => FormulaType::Shared,
                "array" => FormulaType::Array,
                "dataTable" => FormulaType::DataTable,
                _ => FormulaType::Normal,
            });
            any = true;
        }
        if let Some(r) = f.get("ref") {
            a.r#ref = Some(js(r));
            any = true;
        }
        if let Some(si) = f.get("si") {
            a.si = si.num();
            any = true;
        }
        for (k, bit) in [("ca", FormulaAttrs::CA), ("dt2D", FormulaAttrs::DT2D), ("dtr", FormulaAttrs::DTR)] {
            if f.get(k).is_some_and(J::bool) {
                a.flags |= bit;
                any = true;
            }
        }
        if let Some(r) = f.get("r1") {
            a.r1 = Some(js(r));
            any = true;
        }
        if let Some(r) = f.get("r2") {
            a.r2 = Some(js(r));
            any = true;
        }
        if any {
            c.fa = Some(a);
        }
        if f.get("dynamic").is_some_and(J::bool) {
            c.cm = Some(DYN_CM);
        }
    }
    c
}

/// The `(row, col, CellInput)` list of a sheet.
pub type SheetCells = Vec<(u32, u32, J)>;

/// The store and the engine input of a `CalcWorkbook`; `cells` overrides the sheets' cell lists (the full recalc of
/// the final inputs).
pub fn build(wb: &J, cells: Option<&[SheetCells]>) -> (Store, EngineInput) {
    let mut store = Store::new();
    store.date1904 = wb.get("date1904").is_some_and(J::bool);
    let mut input = EngineInput { dynamic_cm: vec![DYN_CM], ..EngineInput::default() };
    for (si, sh) in wb.get("sheets").unwrap().arr().iter().enumerate() {
        let s = store.add_sheet().unwrap();
        input.sheet_names.push(js(sh.get("name").unwrap()));
        let list: Vec<(u32, u32, J)> = match cells {
            Some(c) => c[si].clone(),
            None => sh
                .get("cells")
                .unwrap()
                .arr()
                .iter()
                .map(|c| {
                    let a = c.arr();
                    (a[0].num().unwrap() as u32, a[1].num().unwrap() as u32, a[2].clone())
                })
                .collect(),
        };
        {
            let mut ld = store.load_sheet(s).unwrap();
            let mut cur: Option<u32> = None;
            for (r, c, inp) in &list {
                if cur != Some(*r) {
                    ld.row(*r, RowMeta::default()).unwrap();
                    cur = Some(*r);
                }
                ld.cell(*c, &file_cell(*r, *c, inp)).unwrap();
            }
            ld.finish().unwrap();
        }
        let shs = &mut store.sheets[si];
        for r in sh.get("hiddenRows").map(J::arr).unwrap_or(&[]) {
            shs.hidden.insert(r.num().unwrap() as u32);
        }
        for r in sh.get("filteredRows").map(J::arr).unwrap_or(&[]) {
            shs.filtered.insert(r.num().unwrap() as u32);
        }
        for m in sh.get("merges").map(J::arr).unwrap_or(&[]) {
            if let Some(a) = a1_to_area(js(m).as_js(), s) {
                shs.merges.push(a);
            }
        }
    }
    for n in wb.get("names").map(J::arr).unwrap_or(&[]) {
        input.names.push(NameInput {
            name: js(n.get("name").unwrap()),
            formula: js(n.get("formula").unwrap()),
            sheet: n.get("sheet").and_then(J::num).map(|x| x as u32),
        });
    }
    for t in wb.get("tables").map(J::arr).unwrap_or(&[]) {
        input.tables.push(TableInput {
            name: js(t.get("name").unwrap()),
            sheet: t.get("sheet").unwrap().num().unwrap() as u32,
            r#ref: js(t.get("ref").unwrap()),
            header_rows: t.get("headerRowCount").unwrap().num().unwrap() as u32,
            totals_rows: t.get("totalsRowCount").unwrap().num().unwrap() as u32,
            columns: t.get("columns").unwrap().arr().iter().map(js).collect(),
        });
    }
    for x in wb.get("externals").map(J::arr).unwrap_or(&[]) {
        let mut book = Vec::new();
        for s in x.get("sheets").unwrap().arr() {
            let cells = s
                .get("cells")
                .unwrap()
                .arr()
                .iter()
                .map(|c| {
                    let a = c.arr();
                    let v = match value_of(Some(&a[2])) {
                        V::Null => ExtValue::Empty,
                        V::Num(x) => ExtValue::Num(x),
                        V::Str(t) => ExtValue::Str(t),
                        V::Bool(b) => ExtValue::Bool(b),
                        V::Err(e) => ExtValue::Err(e),
                    };
                    (a[0].num().unwrap() as u32, a[1].num().unwrap() as u32, v)
                })
                .collect();
            book.push(ExternalSheet { name: js(s.get("name").unwrap()), cells });
        }
        input.externals.push(book);
    }
    if let Some(c) = wb.get("calc") {
        input.calc = CalcOptions {
            iterate: c.get("iterate").is_some_and(J::bool),
            iterate_count: c.get("iterateCount").and_then(J::num),
            iterate_delta: c.get("iterateDelta").and_then(J::num),
        };
    }
    (store, input)
}

/// `makeValueText` of a cell value read from the store.
pub fn value_text(store: &Store, v: Scalar) -> String {
    match v {
        Scalar::Empty => "_".into(),
        Scalar::Num(x) => format!("n{:016x}", x.to_bits()),
        Scalar::Bool(b) => (if b { "b1" } else { "b0" }).into(),
        Scalar::Err(e) => format!("e{}", error_text(e)),
        Scalar::Str(StrRef::Pooled(id)) => {
            let mut o = String::from("s");
            uo_jsnum::json::push_string_utf16(&mut o, &store.strings.get(id).to_utf16());
            o
        }
        Scalar::Str(r) => format!("?{r:?}"),
    }
}

/// Every engine cell as the capture's `sheet,row,col=<value>` lines, sorted.
pub fn values(engine: &uo_calc::engine::Engine, store: &Store) -> Vec<String> {
    let mut out = Vec::new();
    engine.for_each_value(store, |s: SheetIx, r, c, v| out.push(format!("{},{},{}={}", s.0, r, c, value_text(store, v))));
    out.sort();
    out
}
