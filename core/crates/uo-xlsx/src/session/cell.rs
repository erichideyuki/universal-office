//! Cell entry and the texts of a cell (`SheetCalc.edit`, `barText`, `copyText`, `valueText`, `ensureCell`,
//! `dropIfEmpty` of `sheet-calc.ts`; `setCellValue`, `setActiveCellText`, the cell editor of the element).

use uo_calc::engine::{CellInput, FormulaInfo, FormulaInput};
use uo_calc::store::{CellKind, NeedSst, ValueTag};
use uo_calc::{JsStr, JsString, Scalar, SheetIx, StrRef};

use super::syntax::{LocaleSyntax, names_of};
use super::{Editing, Session};
use crate::grid::{
    EditValue, Entry, EntryContext, EntryValue, FormulaLang, edit_text, error_text, format_category, infer_formula_format, parse_entry,
    should_apply_format,
};
use crate::save::NumFmtChange;

/// `EditResult` of an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditResult {
    /// Written: the views whose values moved and whether a used range grew.
    Ok {
        /// Views (sorted).
        views: Vec<usize>,
        /// A used range grew.
        grew: bool,
    },
    /// Refused (Excel's message in the formula language).
    Refused {
        /// The message.
        message: String,
    },
}

impl EditResult {
    /// `ok`.
    #[must_use]
    pub fn ok(&self) -> bool {
        matches!(self, EditResult::Ok { .. })
    }

    /// The JSON the element's result serializes to (`JSON.stringify`, Sets as sorted arrays).
    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            EditResult::Ok { views, grew } => {
                let v: Vec<String> = views.iter().map(ToString::to_string).collect();
                format!("{{\"ok\":true,\"views\":[{}],\"grew\":{grew}}}", v.join(","))
            }
            EditResult::Refused { message } => format!("{{\"ok\":false,\"message\":{}}}", uo_jsnum::json::string(message)),
        }
    }
}

/// The formula bar of a cell (`BarText`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarText {
    /// The formula in the language (`=SOMA(A1:A3)`, `{=…}` for CSE) or empty for a constant.
    pub formula: Vec<u16>,
    /// What the bar shows / F2 edits.
    pub edit: Vec<u16>,
    /// A spilled cell (not the anchor): the formula is the anchor's.
    pub ghost: bool,
}

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `formatOf(xf)`: the number format id and code of an xf index.
pub(crate) fn format_of(styles: &crate::read::model::Styles, xf: f64) -> (f64, String) {
    let id = xf_index(xf).and_then(|i| styles.cell_xfs.get(i)).map_or(0.0, |x| x.num_fmt_id);
    if let Some((_, c)) = styles.num_fmts.iter().rev().find(|(k, _)| *k == id) {
        return (id, c.clone());
    }
    let builtin = if id >= 0.0 && id.fract() == 0.0 && id < 4_294_967_296.0 {
        uo_numfmt::builtin_format_code(uo_jsnum::conv::to_uint32(id), uo_numfmt::pt_br())
    } else {
        None
    };
    (id, builtin.unwrap_or("General").to_owned())
}

/// An xf number as an index (`cellXfs[xf]` of a non-integer or negative is `undefined`).
pub(crate) fn xf_index(xf: f64) -> Option<usize> {
    (xf >= 0.0 && xf.fract() == 0.0 && xf < 4_294_967_296.0).then(|| uo_jsnum::conv::to_uint32(xf) as usize)
}

impl Session {
    // ---------------------------------------------------------------- the model cell

    /// The cell exists in the model.
    pub(crate) fn cell_exists(&self, vi: usize, col: u32, row: u32) -> bool {
        col >= 1 && row >= 1 && self.book.store.get(self.ix(vi), row - 1, col - 1).is_some()
    }

    /// `cell.formula !== undefined`.
    pub(crate) fn has_formula_cell(&self, vi: usize, col: u32, row: u32) -> bool {
        col >= 1 && row >= 1 && self.book.store.get(self.ix(vi), row - 1, col - 1).is_some_and(|c| c.kind.has(CellKind::HAS_FORMULA))
    }

    /// `cell.style` (`None` = the attribute is absent or the cell does not exist).
    pub(crate) fn cell_style(&self, vi: usize, col: u32, row: u32) -> Option<f64> {
        if col == 0 || row == 0 {
            return None;
        }
        let ix = self.ix(vi);
        let c = self.book.store.get(ix, row - 1, col - 1)?;
        if c.kind.has(CellKind::HAS_EXTRA)
            && let Some(s) = self.book.store.extra(ix, row - 1, col - 1).and_then(|e| e.raw_style)
        {
            return Some(s);
        }
        (c.style != 0).then(|| f64::from(c.style - 1))
    }

    /// `xfIndexAt(v, cell, col, row)`.
    pub(crate) fn xf_at(&self, vi: usize, col: u32, row: u32) -> f64 {
        if col == 0 || row == 0 {
            return 0.0;
        }
        self.book.store.style_at(self.ix(vi), row - 1, col - 1)
    }

    /// The cell has content (value, formula, inline string): `filled` / `mergeLosesData`.
    pub(crate) fn cell_filled(&self, vi: usize, col: u32, row: u32) -> bool {
        if col == 0 || row == 0 {
            return false;
        }
        let ix = self.ix(vi);
        let Some(c) = self.book.store.get(ix, row - 1, col - 1) else { return false };
        c.kind.has(CellKind::HAS_VALUE) || c.kind.has(CellKind::HAS_FORMULA) || has_inline(&self.book.store, ix, row - 1, col - 1, c)
    }

    /// Sets `cell.style` (`None` deletes the attribute) of an existing cell.
    pub(crate) fn put_style(&mut self, vi: usize, col: u32, row: u32, style: Option<f64>) {
        let ix = self.ix(vi);
        let s = style.and_then(xf_index).map(|i| u32::try_from(i).unwrap_or(u32::MAX - 1));
        let _ = self.book.store.set_style(ix, row - 1, col - 1, s);
    }

    /// `setStyle(cell, xf)`: 0 deletes the attribute.
    pub(crate) fn set_style_xf(&mut self, vi: usize, col: u32, row: u32, xf: usize) {
        self.put_style(vi, col, row, if xf == 0 { None } else { Some(xf as f64) });
    }

    /// `ensureCell`: the model cell, created empty with the row/column style when missing.
    pub(crate) fn ensure_cell(&mut self, vi: usize, col: u32, row: u32) {
        if self.cell_exists(vi, col, row) {
            return;
        }
        let ix = self.ix(vi);
        let xf = self.book.store.default_style(ix, row - 1, col - 1);
        let fc = uo_calc::FileCell { s: (xf != 0).then(|| f64::from(xf)), ..uo_calc::FileCell::default() };
        let _ = self.book.store.set_file_cell(ix, row - 1, col - 1, &fc);
    }

    /// `dropIfEmpty`: the cell without value, formula, inline string and style leaves the model.
    pub(crate) fn drop_if_empty(&mut self, vi: usize, col: u32, row: u32) {
        let ix = self.ix(vi);
        let Some(c) = self.book.store.get(ix, row - 1, col - 1) else { return };
        if self.cell_style(vi, col, row).is_none()
            && !c.kind.has(CellKind::HAS_VALUE)
            && !c.kind.has(CellKind::HAS_FORMULA)
            && !has_inline(&self.book.store, ix, row - 1, col - 1, c)
        {
            self.book.store.remove(ix, row - 1, col - 1);
            self.prune_file_order(ix.0 as usize);
        }
    }

    /// `removeCell` of the model on a sheet saved in FILE order (C1b open item 4): a removed cell leaves its `<row>`
    /// element, so a cell created there later goes where `createCell` puts it, not back to its old place.
    pub(crate) fn prune_file_order(&mut self, si: usize) {
        let ix = SheetIx(u32::try_from(si).unwrap_or(u32::MAX));
        let store = &self.book.store;
        let Some(fo) = store.sheet(ix).and_then(|sh| sh.file_order.as_deref()) else { return };
        let gone = |row: u32, item: u32| item & uo_calc::load::FileRow::SHADOW == 0 && store.get(ix, row, item).is_none();
        if !fo.rows.iter().any(|fr| fr.cells.iter().any(|&c| gone(fr.row, c))) {
            return;
        }
        let keep: Vec<Vec<bool>> = fo.rows.iter().map(|fr| fr.cells.iter().map(|&c| !gone(fr.row, c)).collect()).collect();
        if let Some(fo) = self.book.store.sheets.get_mut(si).and_then(|sh| sh.file_order.as_deref_mut()) {
            for (fr, k) in fo.rows.iter_mut().zip(keep) {
                let mut it = k.into_iter();
                fr.cells.retain(|_| it.next().unwrap_or(true));
            }
        }
    }

    /// `cellValue(cell, shared, date1904)` (decodes a shared string on demand).
    pub(crate) fn cell_value(&mut self, ix: SheetIx, row0: u32, col0: u32) -> Scalar {
        for _ in 0..2 {
            match self.book.store.cell_value(ix, row0, col0) {
                Ok(v) => return v,
                Err(NeedSst(i)) => {
                    if self.book.resolve_sst(i).is_err() {
                        break;
                    }
                }
            }
        }
        Scalar::Empty
    }

    pub(crate) fn scalar_units(&self, v: &Scalar) -> Vec<u16> {
        match v {
            Scalar::Str(StrRef::Pooled(id)) => self.book.store.strings.get(*id).to_utf16(),
            _ => Vec::new(),
        }
    }

    // ---------------------------------------------------------------- the formula bar

    /// The workbook's defined names without `_xlnm.` (`names()`).
    pub(crate) fn names(&self) -> Vec<String> {
        names_of(&self.book.workbook)
    }

    /// `localFormula(rec)`: `=` + the anchor's text in the formula language.
    fn local_formula(&mut self, info: FormulaInfo) -> Vec<u16> {
        let Some(e) = self.book.engine.as_ref() else { return Vec::new() };
        let canon = e.formula_text(&self.book.store, info.sheet, info.row, info.col).unwrap_or_default();
        let table = e.table_name_at(info.sheet, info.row, info.col).cloned();
        let mut text = vec![u16::from(b'=')];
        text.extend(canon.as_js().units());
        self.to_local(&text, table.as_ref())
    }

    /// `toLocal(text, { ...locale.syntax, names, inTable })`.
    pub(crate) fn to_local(&self, canonical: &[u16], in_table: Option<&JsString>) -> Vec<u16> {
        let names = self.names();
        let syn = LocaleSyntax::new(&self.locale, &self.translator);
        syn.to_local(canonical, &names, in_table.map(JsString::as_js))
    }

    /// `toLocalFormula(canonical)` (AutoSum).
    pub fn to_local_formula(&self, canonical: &str) -> Vec<u16> {
        self.to_local(&u16s(canonical), None)
    }

    /// `barText(view, col, row)`: the formula in the language or the edit text of the constant.
    pub fn bar_text(&mut self, vi: usize, col: u32, row: u32) -> BarText {
        if vi >= self.views.len() || col == 0 || row == 0 {
            return BarText::default();
        }
        let ix = self.ix(vi);
        let (r0, c0) = (row - 1, col - 1);
        if (self.has_formula_cell(vi, col, row) || self.has_arrays(vi) || self.book.engine.is_some())
            && self.ensure_engine().is_ok()
            && let Some(e) = self.book.engine.as_ref()
        {
            {
                let own = e.formula_info(ix, r0, c0);
                let owner = if own.is_some() { None } else { e.owner_info(ix, r0, c0) };
                if let Some(rec) = own.or(owner)
                    && !rec.data_table
                {
                    let text = self.local_formula(rec);
                    let cse = rec.array && !rec.dynamic;
                    let formula = if cse {
                        let mut f = vec![u16::from(b'{')];
                        f.extend_from_slice(&text);
                        f.push(u16::from(b'}'));
                        f
                    } else {
                        text
                    };
                    let ghost = owner.is_some_and(|o| o.dynamic);
                    return BarText { edit: if ghost { Vec::new() } else { formula.clone() }, formula, ghost };
                }
            }
        }
        if !self.cell_exists(vi, col, row) {
            return BarText::default();
        }
        let xf = self.xf_at(vi, col, row);
        let value = self.cell_value(ix, r0, c0);
        let (_, code) = format_of(&self.book.styles, xf);
        let quote = xf_index(xf).is_some_and(|i| self.pool.quote_prefix(i));
        let units = self.scalar_units(&value);
        let ev = edit_value(&value, &units);
        let edit = edit_text(&ev, format_category(Some(&code)), &self.locale, self.book.workbook.date1904, quote);
        BarText { formula: Vec::new(), edit, ghost: false }
    }

    /// `copyText`: what typed at (`dcol`, `drow`) reproduces the cell (relative references shifted; `shift` false
    /// = the formula as it is, Cut).
    pub(crate) fn copy_text(&mut self, vi: usize, col: u32, row: u32, dcol: u32, drow: u32, shift: bool) -> Vec<u16> {
        let bar = self.bar_text(vi, col, row);
        if bar.ghost {
            return Vec::new();
        }
        if bar.formula.is_empty() || !shift || bar.formula.first() == Some(&u16::from(b'{')) {
            return bar.edit;
        }
        let ix = self.ix(vi);
        let Some(canon) = self.book.engine.as_ref().and_then(|e| e.formula_text(&self.book.store, ix, row - 1, col - 1)) else {
            return bar.edit;
        };
        let mut ast = uo_calc::ast::Ast::new();
        let po = uo_calc::parse::ParseOptions { r1c1: false, letters: None, row: row - 1, col: col - 1 };
        let Ok(node) = uo_calc::parse::parse_formula(&mut ast, canon.as_js(), &po) else { return bar.edit };
        let pr = uo_calc::print::PrintOptions { row: drow.saturating_sub(1), col: dcol.saturating_sub(1), r1c1: false, ui: false };
        let moved = uo_calc::print::print_formula(&ast, node, &pr);
        let mut text = vec![u16::from(b'=')];
        text.extend(moved.as_js().units());
        self.to_local(&text, None)
    }

    /// `valueText`: the current VALUE as a typable constant (Paste Values).
    pub(crate) fn value_text(&mut self, vi: usize, col: u32, row: u32) -> Vec<u16> {
        if !self.cell_exists(vi, col, row) {
            return Vec::new();
        }
        let ix = self.ix(vi);
        let value = self.cell_value(ix, row - 1, col - 1);
        match &value {
            Scalar::Empty => Vec::new(),
            Scalar::Str(_) => {
                let t = self.scalar_units(&value);
                if t.first().is_some_and(|&u| matches!(u, 61 | 43 | 45 | 64 | 39)) {
                    let mut o = vec![39];
                    o.extend_from_slice(&t);
                    o
                } else {
                    t
                }
            }
            Scalar::Bool(b) => u16s(uo_calc::i18n::localize_bool(*b, self.lang_table())),
            Scalar::Err(e) => u16s(error_text(*e, self.locale.lang)),
            Scalar::Num(_) => {
                let (_, code) = format_of(&self.book.styles, self.xf_at(vi, col, row));
                edit_text(&edit_value(&value, &[]), format_category(Some(&code)), &self.locale, self.book.workbook.date1904, false)
            }
        }
    }

    pub(crate) fn lang_table(&self) -> &'static uo_calc::i18n::FormulaLanguage {
        if self.locale.lang == FormulaLang::PtBr { uo_calc::i18n::PT_BR } else { uo_calc::i18n::EN_US }
    }

    // ---------------------------------------------------------------- entry

    /// `SheetCalc.edit(view, col, row, text)`: the typed text through the entry rules into the store and the engine.
    pub(crate) fn calc_edit(&mut self, vi: usize, col: u32, row: u32, text: &[u16]) -> EditResult {
        if vi >= self.views.len() || col == 0 || row == 0 || col > super::MAX_COLS || row > super::MAX_ROWS {
            return EditResult::Refused { message: String::new() };
        }
        if let Err(e) = self.ensure_engine() {
            return EditResult::Refused { message: e.to_string() };
        }
        let ix = self.ix(vi);
        let (r0, c0) = (row - 1, col - 1);
        let lang = self.locale.lang;
        // a part of a multi-cell CSE array (or a data table) cannot be edited
        if let Some(e) = self.book.engine.as_ref() {
            let rec = e.formula_info(ix, r0, c0).or_else(|| e.owner_info(ix, r0, c0));
            if let Some(rec) = rec
                && !rec.dynamic
                && (rec.data_table || (rec.array && rec.area.is_some_and(|a| a.r1 != a.r2 || a.c1 != a.c2)))
            {
                let m = if lang == FormulaLang::PtBr {
                    "Não é possível alterar parte de uma matriz."
                } else {
                    "You can't change part of an array."
                };
                return EditResult::Refused { message: m.to_owned() };
            }
        }
        let existed = self.cell_exists(vi, col, row);
        let xf = self.xf_at(vi, col, row);
        let (_, code) = format_of(&self.book.styles, xf);
        let current = format_category(Some(&code));
        let names = self.names();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let table = self.book.engine.as_ref().and_then(|e| e.table_name_at(ix, r0, c0)).map(|t| t.as_js().to_string_lossy());
        let entry = {
            let ctx = EntryContext {
                locale: &self.locale,
                current,
                row: r0,
                col: c0,
                date1904: self.book.workbook.date1904,
                year: self.opts.year,
                names: &name_refs,
                in_table: table.as_deref(),
            };
            let syn = LocaleSyntax::new(&self.locale, &self.translator);
            parse_entry(text, &ctx, &syn)
        };
        let xfi = xf_index(xf).unwrap_or(0);
        let input = match &entry {
            Entry::Invalid { message } => return EditResult::Refused { message: (*message).to_owned() },
            Entry::Unsupported => {
                let m =
                    if lang == FormulaLang::PtBr { "Há um problema com esta fórmula." } else { "There's a problem with this formula." };
                return EditResult::Refused { message: m.to_owned() };
            }
            Entry::Clear => {
                if existed {
                    self.book.store.strip_meta(ix, r0, c0);
                }
                None
            }
            Entry::Formula { text, .. } => {
                if existed {
                    self.book.store.strip_meta(ix, r0, c0);
                }
                Some(CellInput::Formula(FormulaInput {
                    text: JsString::from_utf16(text),
                    array: true,
                    dynamic: true,
                    r#ref: None,
                    ca: false,
                }))
            }
            Entry::Value { value, .. } => {
                if existed {
                    self.book.store.strip_meta(ix, r0, c0);
                }
                Some(match value {
                    EntryValue::Number(n) => CellInput::Value(Scalar::Num(*n)),
                    EntryValue::Text(t) => CellInput::Text(JsString::from_utf16(t)),
                    EntryValue::Bool(b) => CellInput::Value(Scalar::Bool(*b)),
                    EntryValue::Error(c) => CellInput::Value(Scalar::Err(*c)),
                })
            }
        };
        if matches!(entry, Entry::Value { .. } | Entry::Formula { .. }) && !existed {
            // `createCell(view, row, col, xf)`
            self.ensure_cell(vi, col, row);
        }
        // the derived format of a constant (`setStyle` before the engine writes, as the TS)
        let mut style_after: Option<usize> = None;
        if let Entry::Value { fmt, quote_prefix, .. } = &entry {
            let fmt_change = fmt.as_ref().filter(|h| should_apply_format(current, h));
            if fmt_change.is_some() || *quote_prefix != self.pool.quote_prefix(xfi) {
                let nf = fmt_change.map(|h| NumFmtChange { id: f64::from(h.id), code: h.code.clone() });
                style_after = Some(self.pool.derive(&mut self.book.styles, xfi, nf, *quote_prefix));
            }
        }
        let changes_all;
        {
            let crate::book::Book { engine, store, .. } = &mut self.book;
            let Some(e) = engine.as_mut() else { return EditResult::Refused { message: String::new() } };
            if let Err(err) = e.set_cell(store, ix, r0, c0, input) {
                return EditResult::Refused { message: format!("{err:?}") };
            }
            match e.calculate(store, self.opts.clock, &mut self.random) {
                Ok(c) => changes_all = c,
                Err(err) => return EditResult::Refused { message: format!("{err:?}") },
            }
            // `writeBack` of the edited cell, which `setCell` touched even when its value did not move
            if matches!(entry, Entry::Formula { .. }) {
                let _ = e.write_back(store, ix, r0, c0);
            }
        }
        if let Some(nx) = style_after
            && self.cell_exists(vi, col, row)
        {
            self.set_style_xf(vi, col, row, nx);
        }
        // the format a formula inherits (`=HOJE()`, `=A1` of a date…)
        if let Entry::Formula { ast, root, .. } = &entry
            && self.cell_exists(vi, col, row)
        {
            let hint = {
                let me: &Session = self;
                let look = |name: Option<&[u16]>, r: f64, c: f64| -> Option<(u32, String)> {
                    let view = match name {
                        None => Some(vi),
                        Some(n) => {
                            me.book.engine.as_ref().and_then(|e| e.sheet_index(JsStr::Utf16(n))).and_then(|s| me.view_of_sheet(s as usize))
                        }
                    }?;
                    let (row1, col1) = (r + 1.0, c + 1.0);
                    let xf = if row1 >= 1.0
                        && col1 >= 1.0
                        && row1.fract() == 0.0
                        && col1.fract() == 0.0
                        && row1 <= 1_048_576.0
                        && col1 <= 16_384.0
                    {
                        me.xf_at(view, uo_jsnum::conv::to_uint32(col1), uo_jsnum::conv::to_uint32(row1))
                    } else {
                        0.0
                    };
                    let (id, code) = format_of(&me.book.styles, xf);
                    Some((uo_jsnum::conv::to_uint32(id), code))
                };
                infer_formula_format(ast, *root, r0, c0, &look)
            };
            match hint {
                Some(h) if should_apply_format(current, &h) => {
                    let nx = self.pool.derive(&mut self.book.styles, xfi, Some(NumFmtChange { id: f64::from(h.id), code: h.code }), false);
                    self.set_style_xf(vi, col, row, nx);
                }
                _ => {
                    if self.pool.quote_prefix(xfi) {
                        let nx = self.pool.derive(&mut self.book.styles, xfi, None, false);
                        self.set_style_xf(vi, col, row, nx);
                    }
                }
            }
        }
        self.sync_styles();
        // writeBack: the views whose values moved, the used ranges
        let mut views: Vec<usize> = Vec::new();
        let mut grew = false;
        for (s, area, _) in &changes_all.sheets {
            if let Some(v) = self.view_of_sheet(s.0 as usize) {
                if !views.contains(&v) {
                    views.push(v);
                }
                let g = &mut self.views[v].grid;
                if area.r2 + 1 > g.max_row && area.r2 < super::MAX_ROWS {
                    // only cells that hold something grow the range (`writeBack` skips deletions)
                    let any = self.book.store.sheet(*s).is_some_and(|sh| sh.max_row.is_some_and(|m| m >= area.r2));
                    if any {
                        let g = &mut self.views[v].grid;
                        g.max_row = area.r2 + 1;
                        grew = true;
                    }
                }
                let g = &mut self.views[v].grid;
                if area.c2 + 1 > g.max_col && area.c2 < super::MAX_COLS {
                    g.max_col = area.c2 + 1;
                    grew = true;
                }
            }
        }
        self.note_changes(&changes_all);
        for (sx, _, _) in &changes_all.sheets {
            self.prune_file_order(sx.0 as usize);
        }
        self.prune_file_order(ix.0 as usize);
        if !views.contains(&vi) {
            views.push(vi);
        }
        views.sort_unstable();
        self.dirty_sheets.insert(vi);
        EditResult::Ok { views, grew }
    }

    /// `setCellValue(col, row, text)`: the text as typed, in the formula language, in a cell of the ACTIVE sheet; no
    /// undo entry (the host API every writer uses).
    pub fn set_cell_value(&mut self, col: u32, row: u32, text: &[u16]) -> EditResult {
        let vi = self.sel.sheet;
        if vi >= self.views.len() {
            return EditResult::Refused { message: String::new() };
        }
        let res = self.calc_edit(vi, col, row, text);
        if !res.ok() {
            return res;
        }
        let g = &mut self.views[vi].grid;
        if col > g.max_col {
            g.max_col = col;
        }
        if row > g.max_row {
            g.max_row = row;
        }
        self.dirty = true;
        res
    }

    /// `setActiveCellText(text)`: the formula bar's commit (one `typing` undo entry).
    pub fn set_active_cell_text(&mut self, text: &[u16]) -> EditResult {
        let (col, row) = (self.sel.col, self.sel.row);
        let res = self.record_cells("typing", &[(col, row)], |s| s.set_cell_value(col, row, text));
        if res.ok() {
            self.after_selection();
        }
        res
    }

    // ---------------------------------------------------------------- the cell editor (`beginEdit`/`commitEdit`/`cancelEdit`)

    /// `beginEdit(col, row, initialText, keepCurrent)`: the editor opens on the merge origin.
    pub fn begin_edit(&mut self, col: u32, row: u32, initial: Option<&[u16]>, keep_current: bool) {
        if self.views.is_empty() {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit();
        }
        let (c, r) = match self.active_view().and_then(|v| v.grid.merges.merge_at(col, row)) {
            Some(m) => (m.first_col, m.first_row),
            None => (col, row),
        };
        self.activate(c, r);
        self.after_selection();
        let text = match initial {
            Some(t) => t.to_vec(),
            None if keep_current => {
                let vi = self.sel.sheet;
                self.bar_text(vi, c, r).edit
            }
            None => Vec::new(),
        };
        self.editing = Some(Editing { col: c, row: r, text });
    }

    /// The input's value changed (the shell's `input` event).
    pub fn set_editor_text(&mut self, text: &[u16]) {
        if let Some(e) = self.editing.as_mut() {
            e.text = text.to_vec();
        }
    }

    /// `commitEdit()`: the editor's text into the cell (`typing`); a refused entry keeps editing and answers false.
    pub fn commit_edit(&mut self) -> bool {
        let Some(ed) = self.editing.clone() else { return true };
        let (col, row) = (ed.col, ed.row);
        let res = self.record_cells("typing", &[(col, row)], |s| s.set_cell_value(col, row, &ed.text));
        if !res.ok() {
            return false;
        }
        self.editing = None;
        self.after_selection();
        true
    }

    /// `cancelEdit()`.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }
}

/// An inline-string cell (`inlineString !== undefined`).
fn has_inline(store: &uo_calc::Store, ix: SheetIx, r0: u32, c0: u32, c: uo_calc::RawCell) -> bool {
    if c.kind.tag() != ValueTag::Inline {
        return false;
    }
    // an inline cell holds `<is>` unless the extra says it had none
    !store.extra(ix, r0, c0).is_some_and(|e| matches!(e.inline, Some(uo_calc::store::InlineExtra::Absent)))
}

/// The engine's scalar as the entry's edit value.
fn edit_value<'a>(v: &Scalar, units: &'a [u16]) -> EditValue<'a> {
    match v {
        Scalar::Empty => EditValue::Empty,
        Scalar::Num(n) => EditValue::Number(*n),
        Scalar::Str(_) => EditValue::Text(units),
        Scalar::Bool(b) => EditValue::Bool(*b),
        Scalar::Err(e) => EditValue::Error(*e),
    }
}
