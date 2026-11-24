//! Preparing the store to WRITE the `.xlsx` after calculating, as Excel 365 writes it (`save.ts` +
//! `file-form.ts` `saveFlags`, frontier A5): `ca`/`aca` of every formula, `cm` of the dynamic arrays (and only
//! theirs), a shared child whose anchor vanished becomes its own formula (Excel "repairs" an orphaned
//! `<f t="shared" si>`), a formula typed in this session gets the file form ([`crate::file_form`]), and the
//! TRUST in the computed values — what decides `<calcPr fullCalcOnLoad>`: a function the engine does not calculate
//! as Excel (environment/server functions, unknown names/UDFs, data tables, unparsed formulas, internal errors)
//! makes the save ask Excel to recalculate on open.
//!
//! The TS edits the sheet-core model; here the store IS the model (decision S5): the rules edit the store's formula
//! table and cell extras in place. uo-xlsx C1b (the surgical save) calls [`prepare_for_save`] before writing the
//! touched sheets.

use std::collections::HashSet;

use crate::a1::{area_to_a1, cell_to_a1};
use crate::ast::{Ast, Node, NodeId, UnaryOp, name_flags};
use crate::engine::{Engine, Graph, fl};
use crate::file_form::to_file_formula;
use crate::formula::{FormulaAttrs, FormulaType};
use crate::implicit::LegacyHost;
use crate::key::{Area, SheetIx};
use crate::print::{PrintOptions, print_formula};
use crate::store::Store;
use crate::strings::{JsStr, JsString};
use crate::value::{ErrorCode, Scalar};

/// The errors the file's `<v>` accepts (the 365 ones Excel writes as `#VALUE!` + a rich value).
const FILE_ERRORS: [ErrorCode; 8] = [
    ErrorCode::Null,
    ErrorCode::Div0,
    ErrorCode::Value,
    ErrorCode::Ref,
    ErrorCode::Name,
    ErrorCode::Num,
    ErrorCode::NA,
    ErrorCode::GettingData,
];

/// Functions whose result comes from outside (server, cube, pivot): our value is not Excel's.
pub const ENVIRONMENT_FUNCTIONS: [&str; 17] = [
    "WEBSERVICE",
    "RTD",
    "CUBEVALUE",
    "CUBEMEMBER",
    "CUBEMEMBERPROPERTY",
    "CUBERANKEDMEMBER",
    "CUBEKPIMEMBER",
    "CUBESET",
    "CUBESETCOUNT",
    "STOCKHISTORY",
    "GETPIVOTDATA",
    "IMAGE",
    "FILTERXML",
    "TRANSLATE",
    "DETECTLANGUAGE",
    "COPILOT",
    "PY",
];

/// The engine's defined names as the legacy rewrite asks them (`nameIsMulti`/`isName` of the engine, `save.ts`).
pub(crate) struct EngineNames<'g> {
    pub g: &'g Graph,
    pub sheet: u32,
    /// `isName` excludes table names (`save.ts`: `definedName(n) && !table(n)`); the engine's own rewrite does not.
    pub exclude_tables: bool,
}

impl LegacyHost for EngineNames<'_> {
    fn name_is_multi(&self, ast: &Ast, name: JsStr<'_>) -> bool {
        let Some(mut d) = self.g.defined_name(name, Some(SheetIx(self.sheet))) else { return true };
        // a scalar constant, also with a sign/parentheses (irr_guess = -0.05 → no @; .xls corpus)
        loop {
            match *ast.node(d) {
                Node::Paren(e) => d = e,
                Node::Unary { op, e } if op != UnaryOp::Percent && !matches!(ast.node(e), Node::Cell { .. }) => d = e,
                _ => break,
            }
        }
        !matches!(ast.node(d), Node::Cell { .. } | Node::Num { .. } | Node::Str(_) | Node::Bool(_) | Node::Err(_))
    }
    fn is_name(&self, _ast: &Ast, name: JsStr<'_>) -> bool {
        self.g.defined_name(name, Some(SheetIx(self.sheet))).is_some()
            && !(self.exclude_tables && self.g.table_by_name.contains_key(&uo_text::case::to_uppercase(&name.to_utf16()).into_owned()))
    }
}

/// `ca`/`aca` Excel would write on a formula now (`SaveFlags`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveFlags {
    /// `ca="1"`.
    pub ca: bool,
    /// `aca="1"`.
    pub aca: bool,
}

/// `saveFlags(engine)`: per formula (anchors in sheet order, `allFormulas`) its position and flags: volatile by the
/// text + dependents (transitive); a dynamic one with `ca` → `aca`; `#SPILL!` → `ca` + `aca`.
pub fn save_flags(engine: &mut Engine, store: &mut Store) -> Vec<((SheetIx, u32, u32), SaveFlags)> {
    engine.g.ensure_ordered();
    let all: Vec<u32> = engine.g.ordered.clone();
    let seeds: Vec<Area> = all.iter().filter(|&&f| engine.g.recs[f as usize].has(fl::VOLFN)).map(|&f| engine.g.area_of(f)).collect();
    let mut ca: HashSet<(SheetIx, u32, u32)> =
        all.iter().filter(|&&f| engine.g.recs[f as usize].has(fl::VOLFN)).map(|&f| pos_of(&engine.g, f)).collect();
    if !seeds.is_empty() {
        ca.extend(engine.dependents_of(store, &seeds));
    }
    all.iter()
        .map(|&f| {
            let r = engine.g.recs[f as usize];
            let p = pos_of(&engine.g, f);
            let dynamic = r.has(fl::DYNAMIC);
            let v = engine.g.view(store, p.0, p.1, p.2).map(|(v, _)| v);
            let blocked = dynamic && v == Some(Scalar::Err(ErrorCode::Spill));
            let c = ca.contains(&p) || blocked;
            (p, SaveFlags { ca: c, aca: dynamic && c })
        })
        .collect()
}

fn pos_of(g: &Graph, f: u32) -> (SheetIx, u32, u32) {
    let r = g.recs[f as usize];
    (SheetIx(r.sheet), r.row, u32::from(r.col))
}

/// `unsureReasons(engine)`: what the engine does not calculate as Excel in this workbook, in the TS order (the
/// diagnostic texts are the TS's).
#[must_use]
pub fn unsure_reasons(engine: &Engine) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let add = |out: &mut Vec<String>, s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    if !engine.g.internal_errors.is_empty() {
        add(&mut out, "erro interno".into());
    }
    let mut seen: HashSet<(u32, Vec<u16>)> = HashSet::new();
    let mut ordered = engine.g.ordered.clone();
    if !engine.g.ordered_ok {
        ordered = (0..engine.g.recs.len() as u32).filter(|&f| engine.g.recs[f as usize].has(fl::LIVE)).collect();
        ordered.sort_by_key(|&f| engine.g.order_key(f));
    }
    for f in ordered {
        let r = engine.g.recs[f as usize];
        if r.has(fl::DATATABLE) {
            add(&mut out, "tabela de dados".into());
            continue;
        }
        if r.node == crate::engine::NONE {
            add(&mut out, "fórmula não analisada".into());
            continue;
        }
        visit_unsure(engine, NodeId(r.node), r.sheet, &mut seen, &mut out);
    }
    out
}

/// The TS `visit` (a walk whose callback walks a used defined name's tree at once, depth < 20).
fn visit_unsure(engine: &Engine, root: NodeId, sheet: u32, seen: &mut HashSet<(u32, Vec<u16>)>, out: &mut Vec<String>) {
    // explicit stack of (node, depth): pre-order, a name's tree is visited right after the name
    let ast = &engine.ast;
    let g = &engine.g;
    let mut st: Vec<(NodeId, u32)> = vec![(root, 0)];
    while let Some((id, depth)) = st.pop() {
        let n = *ast.node(id);
        match n {
            Node::Name { name, flags, .. } if flags & name_flags::PARAM == 0 && depth < 20 => {
                let up = uo_text::case::to_uppercase(&ast.str(name).to_utf16()).into_owned();
                if seen.insert((sheet, up))
                    && let Some(d) = g.defined_name(ast.str(name), Some(SheetIx(sheet)))
                {
                    st.push((d, depth + 1));
                }
                continue;
            }
            Node::Fn { name, .. } => {
                let text = ast.fn_name(name);
                let s = text.to_string_lossy();
                if ENVIRONMENT_FUNCTIONS.contains(&s.as_str()) {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                } else if matches!(name, crate::ast::FnName::Other(_))
                    && !s.eq("ANCHORARRAY")
                    && !s.starts_with("_TRO_")
                    && g.defined_name(text, Some(SheetIx(sheet))).is_none()
                {
                    let m = format!("desconhecida {s}");
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
            _ => {}
        }
        // children in pre-order (the `walk` order of `ast.ts`)
        match n {
            Node::Fn { args, .. } => st.extend(ast.kids(args).iter().rev().map(|&k| (k, depth))),
            Node::Call { f, args } => {
                st.extend(ast.kids(args).iter().rev().map(|&k| (k, depth)));
                st.push((f, depth));
            }
            Node::Unary { e, .. } | Node::Paren(e) => st.push((e, depth)),
            Node::Bin { a, b, .. } => {
                st.push((b, depth));
                st.push((a, depth));
            }
            _ => {}
        }
    }
}

/// Options of [`prepare_for_save`] (`SaveOptions`).
#[derive(Default)]
pub struct SaveOptions<'a> {
    /// The `cm` values (1-based) that already mark a dynamic array in the file (`dynamicArrayCm`).
    pub dynamic_cms: Vec<f64>,
    /// The `cm` index of a dynamic array for new formulas (creates the metadata part only when called).
    pub dynamic_cm: Option<&'a mut dyn FnMut() -> f64>,
    /// Sheets to prepare (`None` = all).
    pub sheets: Option<&'a [SheetIx]>,
}

/// `SavePrep`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavePrep {
    /// Sheets whose cells changed here (their `<sheetData>` must be rewritten), ascending.
    pub touched: Vec<SheetIx>,
    /// The computed values are Excel's (else: `fullCalcOnLoad`).
    pub sure: bool,
    /// Why not (diagnostics).
    pub unsure: Vec<String>,
}

/// `prepareForSave(engine, worksheets, o)` over the store: adjusts the calc attributes of the formulas to write.
/// Call after the values are written (the engine's write path already wrote values and dynamic anchors' `ref`).
pub fn prepare_for_save(engine: &mut Engine, store: &mut Store, o: &mut SaveOptions<'_>) -> SavePrep {
    let flags = save_flags(engine, store);
    let mut dyn_cms: Vec<f64> = o.dynamic_cms.clone();
    let mut cm: Option<f64> = None;
    let mut touched: Vec<SheetIx> = Vec::new();
    engine.g.ensure_ordered();
    let all: Vec<u32> = engine.g.ordered.clone();
    // the shared anchors present in the sheet being prepared (si keys)
    let mut anchors: Option<(SheetIx, HashSet<u64>)> = None;
    for (fi, &f) in all.iter().enumerate() {
        let r = engine.g.recs[f as usize];
        let sheet = SheetIx(r.sheet);
        let (row, col) = (r.row, u32::from(r.col));
        // `flags.get(f)`: the same order as `all`
        let fl0 = flags.get(fi).map_or(SaveFlags::default(), |x| x.1);
        if o.sheets.is_some_and(|s| !s.contains(&sheet)) {
            continue;
        }
        let Some(sh) = store.sheet(sheet) else { continue };
        let Some((_, ff)) = sh.formulas.get(row, col) else { continue };
        let Some(text) = ff.text.clone() else { continue };
        let mut fa: FormulaAttrs = ff.attrs.as_deref().cloned().unwrap_or_default();
        let mut formula = text;
        let mut changed = false;
        let mut text_changed = false;
        // a shared child whose anchor vanished: its own formula
        if fa.t == Some(FormulaType::Shared) && formula.as_js().is_empty() && r.node != crate::engine::NONE {
            if anchors.as_ref().is_none_or(|(s, _)| *s != sheet) {
                anchors = Some((sheet, shared_anchors(store, sheet)));
            }
            let anchored = fa.si.is_some_and(|si| anchors.as_ref().is_some_and(|(_, a)| a.contains(&si_key(si))));
            if !anchored {
                formula = print_formula(&engine.ast, NodeId(r.node), &PrintOptions { row, col, r1c1: false, ui: false });
                fa.t = None;
                fa.si = None;
                fa.r#ref = None;
                changed = true;
                text_changed = true;
            }
        }
        let extra_cm = store.extra(sheet, row, col).and_then(|x| x.cm);
        let extra_vm = store.extra(sheet, row, col).and_then(|x| x.vm);
        if r.has(fl::DYNAMIC) && r.node != crate::engine::NONE {
            let file_dyn = extra_cm.is_some_and(|c| dyn_cms.contains(&c));
            let mut dynamic = true;
            if !file_dyn {
                // typed in this session (365 semantics): legacy when it gives the same, else a dynamic array
                let host = EngineNames { g: &engine.g, sheet: r.sheet, exclude_tables: true };
                let ff2 = to_file_formula(&mut engine.ast, NodeId(r.node), row, col, &host);
                if formula != ff2.text {
                    formula = ff2.text;
                    changed = true;
                    text_changed = true;
                }
                dynamic = ff2.dynamic;
            }
            if dynamic {
                let rf = match engine.g.areas.get(&f) {
                    Some(a) => area_to_a1(*a),
                    None => cell_to_a1(row, col, false, false),
                };
                let rf = JsString::from_str_js(&rf);
                if fa.t != Some(FormulaType::Array) || fa.r#ref.as_ref() != Some(&rf) {
                    fa.t = Some(FormulaType::Array);
                    fa.r#ref = Some(rf);
                    changed = true;
                }
            } else if fa.t == Some(FormulaType::Array) {
                fa.t = None;
                fa.r#ref = None;
                changed = true;
            }
        }
        let is_dyn = fa.t == Some(FormulaType::Array) && r.has(fl::DYNAMIC);
        // a 365 error (#CALC!, #FIELD!…) goes as #VALUE! in <v>: with ca Excel recalculates it on open
        let v = engine.g.view(store, sheet, row, col).map(|(v, _)| v);
        let err = match v {
            Some(Scalar::Err(e)) => Some(e),
            _ => None,
        };
        let fl1 = if err.is_some_and(|e| !FILE_ERRORS.contains(&e)) { SaveFlags { ca: true, aca: fl0.aca || is_dyn } } else { fl0 };
        let mut new_vm = extra_vm;
        // the vm of an ERROR rich value (#SPILL!/#CALC! cached) on a formula that now gives a plain value: dropped
        if extra_vm.is_some() && err.is_none() {
            new_vm = None;
            changed = true;
        }
        let has_ca = fa.flags & FormulaAttrs::CA != 0;
        if has_ca != fl1.ca {
            fa.flags = if fl1.ca { fa.flags | FormulaAttrs::CA } else { fa.flags & !FormulaAttrs::CA };
            changed = true;
        }
        let aca = fl1.aca && is_dyn;
        if (fa.flags & FormulaAttrs::ACA != 0) != aca {
            fa.flags = if aca { fa.flags | FormulaAttrs::ACA } else { fa.flags & !FormulaAttrs::ACA };
            changed = true;
        }
        let mut new_cm = extra_cm;
        if is_dyn {
            if extra_cm.is_none_or(|c| !dyn_cms.contains(&c))
                && let Some(next) = o.dynamic_cm.as_mut()
            {
                let c = *cm.get_or_insert_with(next);
                new_cm = Some(c);
                if !dyn_cms.contains(&c) {
                    dyn_cms.push(c);
                }
                changed = true;
            }
        } else if extra_cm.is_some_and(|c| dyn_cms.contains(&c)) {
            new_cm = None;
            changed = true;
        }
        if changed {
            let empty = fa == FormulaAttrs::default();
            if let Some(sh) = store.sheets.get_mut(sheet.0 as usize)
                && let Some(ffm) = sh.formulas.get_mut(row, col)
            {
                ffm.attrs = if empty { None } else { Some(Box::new(fa)) };
                if text_changed {
                    ffm.text = Some(formula);
                }
            }
            if new_cm != extra_cm || new_vm != extra_vm {
                store.edit_extra(sheet, row, col, |x| {
                    x.cm = new_cm;
                    x.vm = new_vm;
                });
            }
            if !touched.contains(&sheet) {
                touched.push(sheet);
            }
        }
    }
    touched.sort_unstable();
    let mut unsure = unsure_reasons(engine);
    // a data table (<f t="dataTable"/> without text): the engine does not calculate it
    if !unsure.iter().any(|s| s == "tabela de dados") && has_data_table(store) {
        unsure.push("tabela de dados".into());
    }
    SavePrep { touched, sure: unsure.is_empty(), unsure }
}

/// The `si` of the shared anchors of a sheet (`t="shared"`, `si`, a non-empty text).
fn shared_anchors(store: &Store, sheet: SheetIx) -> HashSet<u64> {
    let mut out = HashSet::new();
    let Some(sh) = store.sheet(sheet) else { return out };
    for fc in &sh.formulas.columns {
        for &id in &fc.ids {
            if let Some(f) = sh.formulas.by_id(id)
                && let Some(a) = f.attrs.as_deref()
                && a.t == Some(FormulaType::Shared)
                && let Some(si) = a.si
                && f.text.as_ref().is_some_and(|t| !t.as_js().is_empty())
            {
                out.insert(si_key(si));
            }
        }
    }
    out
}

/// `Set<number>` membership of an `si` (`-0` is `0`, NaN equals NaN).
fn si_key(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

fn has_data_table(store: &Store) -> bool {
    store.sheets.iter().any(|sh| {
        sh.formulas.columns.iter().any(|fc| {
            fc.ids
                .iter()
                .any(|&id| sh.formulas.by_id(id).is_some_and(|f| f.attrs.as_deref().is_some_and(|a| a.t == Some(FormulaType::DataTable))))
        }) || sh
            .file_order
            .as_ref()
            .is_some_and(|fo| fo.shadowed.iter().any(|(_, c)| c.fa.as_ref().is_some_and(|a| a.t == Some(FormulaType::DataTable))))
    })
}
