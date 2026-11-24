//! The engine of an opened [`Book`] (`fromSheetCore` + `SheetCalc.ensure`/`reset`, `from-sheet-core.ts:105-190`,
//! `sheet-calc.ts:96-130`): the store holds the cells, this adds what the reader keeps outside them (names,
//! tables, external caches, calc options, merges, filtered rows) and decides the open-recalc
//! (`shouldRecalcOnOpen`). The save needs the engine for `prepareForSave`; the viewer and the editor build it the
//! same way.

use uo_calc::engine::{CalcOptions, Changes, Engine, EngineInput, EngineOptions, ExtValue, ExternalSheet, NameInput, TableInput};
use uo_calc::key::SheetIx;
use uo_calc::locale::{Clock, RandomSource};
use uo_calc::open_recalc::{CalcPrFlags, OpenRecalcPlan, OpenRecalcWorkbook, apply_open_recalc, should_recalc_on_open};
use uo_calc::strings::JsString;

use crate::book::{Book, ReadError};
use crate::read::model::{ExternalCell, Worksheet};
use crate::read::sst::SstDecoder;

fn js(s: &str) -> JsString {
    JsString::from_str_js(s)
}

/// `extValue(t, v)`.
fn ext_value(c: &ExternalCell) -> ExtValue {
    let Some(v) = c.v.as_deref() else { return ExtValue::Empty };
    match c.t.as_deref() {
        Some("str" | "s" | "inlineStr") => ExtValue::Str(js(v)),
        Some("b") => ExtValue::Bool(v == "1" || v == "true"),
        Some("e") => ExtValue::Err(uo_calc::cell::error_of(js(v).as_js())),
        _ => {
            let n = uo_jsnum::parse::to_number(v);
            if n.is_finite() { ExtValue::Num(n) } else { ExtValue::Str(js(v)) }
        }
    }
}

/// The engine's input from the workbook model (every sheet must be loaded: tables come from the sheet models).
#[must_use]
pub fn engine_input(book: &Book) -> EngineInput {
    let wb = &book.workbook;
    let mut input = EngineInput { dynamic_cm: wb.dynamic_array_cm.clone(), ..EngineInput::default() };
    for (si, sh) in wb.sheets.iter().enumerate() {
        input.sheet_names.push(js(&sh.name));
        let Some(ws) = book.sheets.get(si).and_then(Option::as_ref) else { continue };
        for t in &ws.tables {
            let Some(area) = uo_calc::a1::a1_to_area(js(&t.r#ref).as_js(), SheetIx(si as u32)) else { continue };
            let header = t.header_row_count.unwrap_or(if t.header_row { 1.0 } else { 0.0 });
            let totals = t.totals_row_count.unwrap_or(if t.totals_row { 1.0 } else { 0.0 });
            let mut columns: Vec<JsString> = t.columns.as_deref().unwrap_or(&[]).iter().map(|c| js(&c.name)).collect();
            if columns.is_empty() {
                columns = (0..=area.c2.saturating_sub(area.c1)).map(|i| js(&format!("Column{}", i + 1))).collect();
            }
            let n = input.tables.len();
            input.tables.push(TableInput {
                name: js(t.display_name.as_deref().or(t.name.as_deref()).map_or_else(|| format!("Table{}", n + 1), str::to_owned).as_str()),
                sheet: si as u32,
                r#ref: js(&t.r#ref),
                header_rows: uo_jsnum::conv::to_uint32(header),
                totals_rows: uo_jsnum::conv::to_uint32(totals),
                columns,
            });
        }
    }
    for d in &wb.defined_names {
        input.names.push(NameInput { name: js(&d.name), formula: js(&d.formula), sheet: d.local_sheet_id.map(uo_jsnum::conv::to_uint32) });
    }
    for x in &wb.external_links {
        let mut book_sheets = Vec::new();
        for (i, name) in x.sheet_names.iter().enumerate() {
            let cells = x
                .sheet_data
                .iter()
                .find(|d| d.sheet_id == i as f64)
                .map(|d| {
                    d.cells
                        .iter()
                        .filter_map(|c| {
                            let a = uo_calc::a1::a1_to_area(js(&c.r#ref).as_js(), SheetIx(0))?;
                            Some((a.r1, a.c1, ext_value(c)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            book_sheets.push(ExternalSheet { name: js(name), cells });
        }
        input.externals.push(book_sheets);
    }
    if let Some(c) = &wb.calc_pr {
        input.calc = CalcOptions { iterate: c.iterate, iterate_count: c.iterate_count, iterate_delta: c.iterate_delta };
    }
    input
}

/// Merges and filtered rows of the store (the reader fills the cells and the hidden rows).
fn prepare_store(book: &mut Book) {
    for si in 0..book.workbook.sheets.len() {
        let Some(ws) = book.sheets.get(si).and_then(Option::as_ref) else { continue };
        let ix = SheetIx(si as u32);
        let merges: Vec<_> = ws.merges.iter().filter_map(|m| uo_calc::a1::a1_to_area(js(m).as_js(), ix)).collect();
        let filters: Vec<_> = filter_areas(ws, ix);
        let Some(sh) = book.store.sheets.get_mut(si) else { continue };
        sh.merges = merges;
        sh.filtered = Default::default();
        let ranges = sh.hidden.ranges.clone();
        for (a, b) in ranges {
            for r in a..=b {
                if ws.filter_mode || filters.iter().any(|x| r > x.r1 && r <= x.r2) {
                    sh.filtered.insert(r);
                }
            }
        }
    }
}

fn filter_areas(ws: &Worksheet, ix: SheetIx) -> Vec<uo_calc::key::Area> {
    let mut v = Vec::new();
    let mut add = |f: &crate::read::model::AutoFilterInfo| {
        if !f.active.is_empty()
            && let Some(a) = uo_calc::a1::a1_to_area(js(&f.r#ref).as_js(), ix)
        {
            v.push(a);
        }
    };
    if let Some(f) = &ws.auto_filter {
        add(f);
    }
    for t in &ws.tables {
        if let Some(f) = &t.filter {
            add(f);
        }
    }
    v
}

/// Builds the engine of `book` (loads every sheet first) and keeps it in `book.engine`. The cells keep the file's
/// cached values (no `ignore_cache`): [`open_recalc`] decides what to calculate.
pub fn build_engine(book: &mut Book, opts: &EngineOptions) -> Result<(), ReadError> {
    book.load_all()?;
    prepare_store(book);
    let input = engine_input(book);
    let Book { store, sst, pkg, cx, run_props, .. } = book;
    let engine = match sst.as_ref() {
        Some(index) => {
            let mut dec = SstDecoder { pkg, index, cx, props: run_props, error: None };
            let e = Engine::new(store, &input, opts, Some(&mut dec)).map_err(|e| ReadError::Calc(format!("{e:?}")))?;
            if let Some(err) = dec.error {
                return Err(err);
            }
            e
        }
        None => Engine::new(store, &input, opts, None).map_err(|e| ReadError::Calc(format!("{e:?}")))?,
    };
    book.engine = Some(engine);
    Ok(())
}

/// The open-recalc decision of the file (`shouldRecalcOnOpen`).
#[must_use]
pub fn open_recalc_plan(book: &Book) -> OpenRecalcPlan {
    let cp = book.workbook.calc_pr.as_ref();
    let flags = cp.map(|c| CalcPrFlags {
        calc_id: c.calc_id,
        manual: c.calc_mode.as_deref() == Some("manual"),
        full_calc_on_load: c.full_calc_on_load,
        force_full_calc: c.force_full_calc,
    });
    should_recalc_on_open(&OpenRecalcWorkbook { app_name_xl: book.workbook.app_name.as_deref() == Some("xl"), calc_pr: flags }, &book.store)
}

/// Applies the open-recalc (`SheetCalc.reset`): full, partial or nothing. Builds the engine when needed; returns
/// the plan and what was written (its sheets are the ones the save must rewrite).
pub fn open_recalc(
    book: &mut Book,
    opts: &EngineOptions,
    clock: Clock,
    random: &mut RandomSource,
) -> Result<(OpenRecalcPlan, Changes), ReadError> {
    book.load_all()?;
    let plan = open_recalc_plan(book);
    if plan.mode == uo_calc::open_recalc::OpenRecalcMode::None {
        return Ok((plan, Changes::default()));
    }
    if book.engine.is_none() {
        build_engine(book, opts)?;
    }
    let Book { store, engine, .. } = book;
    let Some(e) = engine.as_mut() else { return Ok((plan, Changes::default())) };
    let changes = apply_open_recalc(e, store, &plan, clock, random).map_err(|e| ReadError::Calc(format!("{e:?}")))?;
    Ok((plan, changes))
}
