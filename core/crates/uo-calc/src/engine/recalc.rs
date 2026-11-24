//! The calculation (`engine.ts:628-1066`): edits, dirty marking through the reverse index, the explicit-stack
//! evaluation of dirty formulas with their static precedents (`computeRoot`), `NeedCell` as a `Result`, spills and
//! CSE areas, cycles with and without iteration. No recursion anywhere in the scheduler: a 1,000,000-formula chain
//! runs on two vectors.

use super::{CellInput, Graph, InternalError, NULL_VIEW, RevSheet, fl};
use crate::a1::{a1_to_area, area_contains, areas_overlap};
use crate::ast::{Ast, NodeId};
use crate::cell::{CellWrite, WriteKind};
use crate::error::CalcError;
use crate::eval::{EvalState, Evaluator, Scope};
use crate::fnreg::Throw;
use crate::formula::{FileFormula, FormulaAttrs, FormulaType};
use crate::key::{Area, EXTERNAL_BASE, MAX_COL, MAX_ROW, SheetIx};
use crate::locale::{Clock, RandomSource};
use crate::store::{CellKind, Store, ts_key};
use crate::strings::{JsStr, JsString};
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, StrRef, Value};

use super::host::Host;

/// One call's borrow of everything: the arena, the evaluator state, the graph, the store and the host inputs.
pub(crate) struct Run<'a> {
    pub ast: &'a mut Ast,
    pub st: &'a mut EvalState,
    pub g: &'a mut Graph,
    pub store: &'a mut Store,
    pub rnd: &'a mut RandomSource,
    pub clock: Clock,
    /// `clock.current_year()`.
    pub year: f64,
    /// A store failure (a limit) met while writing: reported by the public call.
    pub err: Option<CalcError>,
}

/// Buffers of the scheduler, reused across formulas.
#[derive(Default)]
pub(crate) struct Scratch {
    stack: Vec<(u32, bool)>,
    out: Vec<u32>,
    deps: Vec<Area>,
}

/// A value to put in a cell or an array to place.
enum Placed {
    S(Scalar),
    A(ArrayValue),
}

macro_rules! eval {
    ($run:expr, |$ev:ident| $body:expr) => {{
        let mut host = Host { g: &mut *$run.g, store: &mut *$run.store, rnd: &mut *$run.rnd, clock: $run.clock, year: $run.year };
        #[allow(unused_mut)]
        let mut $ev = Evaluator { ast: &mut *$run.ast, host: &mut host, st: &mut *$run.st };
        $body
    }};
}

impl Run<'_> {
    fn fail(&mut self, e: CalcError) {
        if self.err.is_none() {
            self.err = Some(e);
        }
    }

    // ------------------------------------------------------------ texts and writes

    fn text(&self, s: StrRef) -> JsStr<'_> {
        match s {
            StrRef::Pooled(id) => self.store.strings.get(id),
            StrRef::Temp(i) => self.st.temps.get(i),
            StrRef::Lit(i) => self.ast.str(crate::ast::StrIx(i)),
        }
    }

    /// `a !== b` of two cell values (JS: `0 === -0`, `NaN !== NaN`, texts by content, error singletons by code).
    fn same(&self, a: Scalar, b: Scalar) -> bool {
        match (a, b) {
            (Scalar::Empty, Scalar::Empty) => true,
            (Scalar::Num(x), Scalar::Num(y)) => x == y,
            (Scalar::Bool(x), Scalar::Bool(y)) => x == y,
            (Scalar::Err(x), Scalar::Err(y)) => x == y,
            (Scalar::Str(x), Scalar::Str(y)) => x == y || self.text(x).eq_units(self.text(y)),
            _ => false,
        }
    }

    /// Writes a value into the store with the bridge's kind (formula / spill / constant) and clears the
    /// `ignoreCache` views of the cell.
    fn store_write(&mut self, s: u32, r: u32, c: u32, v: Scalar) {
        let sheet = SheetIx(s);
        let raw = self.store.get(sheet, r, c).map_or(0, |x| x.kind.0);
        let cur = if raw & CellKind::HAS_FORMULA != 0 { self.g.current(s, r, c) } else { None };
        let k = ts_key(r, c);
        let owned = if raw & CellKind::OWNED != 0 { self.g.sheets[s as usize].owners.get(&k).copied() } else { None };
        let kind = if cur.is_some() {
            WriteKind::Formula
        } else if owned.is_some() {
            WriteKind::Spill
        } else {
            WriteKind::Constant
        };
        let res = match v {
            Scalar::Empty => self.store.write_value(sheet, r, c, CellWrite::Empty, kind),
            Scalar::Num(x) => self.store.write_value(sheet, r, c, CellWrite::Num(x), kind),
            Scalar::Bool(b) => self.store.write_value(sheet, r, c, CellWrite::Bool(b), kind),
            Scalar::Err(e) => self.store.write_value(sheet, r, c, CellWrite::Err(e), kind),
            Scalar::Str(StrRef::Pooled(id)) => self.store.write_value(sheet, r, c, CellWrite::Pooled(id), kind),
            Scalar::Str(StrRef::Temp(i)) => self.store.write_value(sheet, r, c, CellWrite::Str(self.st.temps.get(i)), kind),
            Scalar::Str(StrRef::Lit(i)) => self.store.write_value(sheet, r, c, CellWrite::Str(self.ast.str(crate::ast::StrIx(i))), kind),
        };
        if let Err(e) = res {
            self.fail(e);
        }
        if let Some(f) = cur {
            self.g.recs[f as usize].set(fl::ZERO, false);
            if self.g.recs[f as usize].has(fl::DYNAMIC) {
                self.set_dynamic_attrs(f);
            }
        }
        if let Some(o) = owned
            && o & NULL_VIEW != 0
            && let Some(x) = self.g.sheets[s as usize].owners.get_mut(&k)
        {
            *x &= !NULL_VIEW;
        }
        self.g.note_row(s, r);
    }

    /// `touch` of a dynamic anchor whose area changed (the bridge rewrites its `<f>` attributes).
    fn touch_anchor(&mut self, f: u32) {
        let r = self.g.recs[f as usize];
        self.g.touch(r.sheet, r.row, u32::from(r.col));
        if self.g.current(r.sheet, r.row, u32::from(r.col)) == Some(f) {
            self.set_dynamic_attrs(f);
        }
    }

    /// `write(st, r, c, rec, v)`: only when the value changed.
    fn write(&mut self, s: u32, r: u32, c: u32, v: Scalar) {
        let Some((cur, _)) = self.g.view(self.store, SheetIx(s), r, c) else { return };
        if !self.same(cur, v) {
            self.store_write(s, r, c, v);
            self.g.touch(s, r, c);
        }
    }

    /// `put(st, f, v)`: the formula's own cell, if it still exists.
    fn put(&mut self, f: u32, v: Scalar) {
        let rec = self.g.recs[f as usize];
        self.write(rec.sheet, rec.row, u32::from(rec.col), v);
    }

    /// `st.set(r, c, { v, owner: f })` (always a write).
    fn set_owned(&mut self, f: u32, s: u32, r: u32, c: u32, v: Scalar) {
        if self.g.current(s, r, c).is_some() {
            // a formula cell replaced by the area (a CSE typed over formulas): the formula is orphaned
            self.g.unset_current(s, r, c);
            if let Err(e) = self.store.clear_content(SheetIx(s), r, c) {
                self.fail(e);
            }
        }
        let had = self.store.get(SheetIx(s), r, c).is_some();
        self.g.sheets[s as usize].owners.insert(ts_key(r, c), f);
        if let Err(e) = self.store.claim_owned(SheetIx(s), r, c) {
            self.fail(e);
        }
        if !had {
            self.g.grew = true;
        }
        self.store_write(s, r, c, v);
        self.g.touch(s, r, c);
    }

    /// `st.delete(r, c)` of an owned cell (the bridge then strips it).
    fn delete_owned(&mut self, s: u32, r: u32, c: u32) {
        self.g.sheets[s as usize].owners.remove(&ts_key(r, c));
        if let Err(e) = self.store.clear_content(SheetIx(s), r, c) {
            self.fail(e);
        }
        self.g.touch(s, r, c);
    }

    /// Cells of `a` owned by `f`.
    fn owned_by(&self, f: u32, a: Area) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let Some(sg) = self.g.sheets.get(a.sheet.0 as usize) else { return out };
        self.store.each(a, |r, c, raw| {
            if raw.kind.has(CellKind::OWNED) && sg.owners.get(&ts_key(r, c)).is_some_and(|&o| o & !NULL_VIEW == f) {
                out.push((r, c));
            }
        });
        out
    }

    /// `clearArea(f)`.
    fn clear_area(&mut self, f: u32) {
        let Some(&a) = self.g.areas.get(&f) else { return };
        for (r, c) in self.owned_by(f, a) {
            self.delete_owned(a.sheet.0, r, c);
        }
        if self.g.recs[f as usize].has(fl::DYNAMIC) {
            self.g.areas.remove(&f);
        }
    }

    /// `setDynamicAttrs` (`sheet-calc.ts:479-492`): the `<f>` attributes of a dynamic anchor for the save.
    fn set_dynamic_attrs(&mut self, f: u32) {
        let rec = self.g.recs[f as usize];
        let (s, r, c) = (SheetIx(rec.sheet), rec.row, u32::from(rec.col));
        let area = self.g.areas.get(&f).copied();
        let multi = area.is_some_and(|a| a.r1 != a.r2 || a.c1 != a.c2);
        let cm = self.store.extra(s, r, c).and_then(|x| x.cm);
        let dyn_cm = self.g.dyn_cm.first().copied();
        let volatile = rec.has(fl::VOLATILE) && !rec.has(fl::INPUT_CA);
        let Some(attrs) = self.store.formula_attrs_mut(s, r, c) else { return };
        if !multi && cm.is_none() {
            if let Some(a) = attrs.as_mut() {
                a.t = None;
                a.r#ref = None;
            }
        } else {
            let a = attrs.get_or_insert_with(Box::default);
            a.t = Some(FormulaType::Array);
            let text = area.map_or_else(|| crate::cell::a1_ref(r, c), crate::a1::area_to_a1);
            a.r#ref = Some(JsString::from_str_js(&text));
        }
        if volatile {
            attrs.get_or_insert_with(Box::default).flags |= FormulaAttrs::CA;
        }
        if attrs.as_ref().is_some_and(|a| **a == FormulaAttrs::default()) {
            *attrs = None;
        }
        if cm.is_none()
            && (multi || cm.is_some())
            && let Some(d) = dyn_cm
        {
            self.store.edit_extra(s, r, c, |x| x.cm = Some(d));
        }
    }

    // ------------------------------------------------------------ static precedents

    /// The reference nodes of a tree and its names (`depsOf`'s walk, `engine.ts:826-848`), built once per (tree,
    /// sheet) and kept on the record.
    fn template(&mut self, f: u32) -> (u32, u32) {
        let rec = self.g.recs[f as usize];
        if rec.tpl != super::NONE {
            return self.g.tpls[rec.tpl as usize];
        }
        let key = (rec.node, rec.sheet);
        let ix = match self.g.tpl.get(&key) {
            Some(&i) => i,
            None => {
                let i = self.build_template(rec.node, rec.sheet);
                self.g.tpl.insert(key, i);
                i
            }
        };
        self.g.recs[f as usize].tpl = ix;
        self.g.tpls[ix as usize]
    }

    fn build_template(&mut self, node: u32, sheet: u32) -> u32 {
        use crate::ast::Node as N;
        let start = self.g.tpl_refs.len() as u32;
        let mut seen: Vec<NodeId> = Vec::new();
        // pre-order with names expanded in place, before the node's own children (the TS recursion inside `walk`)
        let mut stack: Vec<(NodeId, u32)> = vec![(NodeId(node), 0)];
        let part = |ast: &Ast, p: crate::ast::Part| -> (u8, f64) {
            if !p.is_some() {
                return (0, 0.0);
            }
            match ast.part(p) {
                Some(r) if r.abs => (1, r.n),
                Some(r) => (2, r.n),
                None => (1, f64::NAN),
            }
        };
        while let Some((id, depth)) = stack.pop() {
            let n = *self.ast.node(id);
            let mut expand: Option<NodeId> = None;
            match n {
                N::Cell { sheet: 0, row, col } => {
                    let (r, c) = (part(self.ast, row), part(self.ast, col));
                    self.g.tpl_refs.push(super::TplRef::Simple([r, c, r, c]));
                }
                N::Area { sheet: 0, r1, c1, r2, c2 } => {
                    let p = [part(self.ast, r1), part(self.ast, c1), part(self.ast, r2), part(self.ast, c2)];
                    self.g.tpl_refs.push(super::TplRef::Simple(p));
                }
                N::Cell { .. } | N::Area { .. } | N::Table { .. } => self.g.tpl_refs.push(super::TplRef::Node(id)),
                N::Name { name, .. } if depth < 20 => {
                    let t = self.ast.string(name);
                    expand = self.g.defined_name(t.as_js(), Some(SheetIx(sheet)));
                }
                N::Fn { name, args, .. } => {
                    let def = match name {
                        crate::ast::FnName::Known(ix) => ix.entry().def.is_some(),
                        crate::ast::FnName::Other(_) => false,
                    };
                    stack.extend(self.ast.kids(args).iter().rev().map(|&k| (k, depth)));
                    if !def && depth < 20 {
                        let t = self.ast.fn_name(name).to_owned_js();
                        expand = self.g.defined_name(t.as_js(), Some(SheetIx(sheet)));
                    }
                }
                N::Call { f, args } => {
                    stack.extend(self.ast.kids(args).iter().rev().map(|&k| (k, depth)));
                    stack.push((f, depth));
                }
                N::Unary { e, .. } | N::Paren(e) => stack.push((e, depth)),
                N::Bin { a, b, .. } => {
                    stack.push((b, depth));
                    stack.push((a, depth));
                }
                _ => {}
            }
            if let Some(d) = expand
                && !seen.contains(&d)
            {
                seen.push(d);
                stack.push((d, depth + 1));
            }
        }
        self.g.tpls.push((start, self.g.tpl_refs.len() as u32 - start));
        self.g.tpls.len() as u32 - 1
    }

    /// `depsOf(f)`: the template instantiated at the formula's cell.
    pub(crate) fn deps_into(&mut self, f: u32, out: &mut Vec<Area>) {
        out.clear();
        let rec = self.g.recs[f as usize];
        if rec.node == super::NONE {
            return;
        }
        let (start, len) = self.template(f);
        let (row, col) = (rec.row, u32::from(rec.col));
        for i in start..start + len {
            match self.g.tpl_refs[i as usize] {
                super::TplRef::Simple(p) => {
                    let side = |(k, n): (u8, f64), base: u32, is_row: bool, dflt: u32| match k {
                        0 => f64::from(dflt),
                        1 => n,
                        _ => {
                            let x = f64::from(base) + n;
                            if is_row { crate::a1::wrap_row(x) } else { crate::a1::wrap_col(x) }
                        }
                    };
                    let (r1, r2) = (side(p[0], row, true, 0), side(p[2], row, true, MAX_ROW));
                    let (c1, c2) = (side(p[1], col, false, 0), side(p[3], col, false, MAX_COL));
                    if r1.is_nan() || r2.is_nan() || c1.is_nan() || c2.is_nan() {
                        continue;
                    }
                    let (lo, hi) = (uo_jsnum::math::min, uo_jsnum::math::max);
                    out.push(Area {
                        sheet: SheetIx(rec.sheet),
                        r1: crate::cast::to_u32(lo(r1, r2)),
                        c1: crate::cast::to_u32(lo(c1, c2)),
                        r2: crate::cast::to_u32(hi(r1, r2)),
                        c2: crate::cast::to_u32(hi(c1, c2)),
                    });
                }
                super::TplRef::Node(x) => {
                    let s = Scope { sheet: SheetIx(rec.sheet), row, col, arr: true, env: None };
                    if let Ok(Value::Ref(r)) = eval!(self, |ev| ev.eval_node(x, s)) {
                        out.extend_from_slice(r.areas());
                    }
                }
            }
        }
    }

    fn ensure_rev(&mut self) {
        if self.g.rev.is_some() {
            return;
        }
        let mut rev: Vec<RevSheet> = (0..self.g.sheets.len()).map(|_| RevSheet::default()).collect();
        let mut deps = Vec::new();
        for f in 0..self.g.recs.len() as u32 {
            if !self.g.recs[f as usize].has(fl::LIVE) {
                continue;
            }
            self.deps_into(f, &mut deps);
            for d in &deps {
                if let Some(ix) = rev.get_mut(d.sheet.0 as usize) {
                    ix.insert(*d, f, false);
                }
            }
        }
        for r in &mut rev {
            r.sort();
        }
        self.g.rev = Some(rev);
    }

    fn rev_edit(&mut self, f: u32, add: bool) {
        if self.g.rev.is_none() {
            return;
        }
        let mut deps = Vec::new();
        self.deps_into(f, &mut deps);
        if let Some(rev) = self.g.rev.as_mut() {
            for d in deps {
                if let Some(ix) = rev.get_mut(d.sheet.0 as usize) {
                    if add {
                        ix.insert(d, f, true);
                    } else {
                        ix.remove(d, f);
                    }
                }
            }
        }
    }

    /// `markDependents(changed)`.
    pub(crate) fn mark_dependents(&mut self, mut queue: Vec<Area>) {
        self.ensure_rev();
        self.g.epoch = self.g.epoch.wrapping_add(1);
        let epoch = self.g.epoch;
        let mut hits = Vec::new();
        while let Some(a) = queue.pop() {
            hits.clear();
            if let Some(ix) = self.g.rev.as_ref().and_then(|r| r.get(a.sheet.0 as usize)) {
                ix.query(a, &mut hits);
            }
            for &f in &hits {
                let r = &mut self.g.recs[f as usize];
                if r.seen == epoch {
                    continue;
                }
                r.seen = epoch;
                if r.has(fl::DATATABLE) {
                    continue;
                }
                self.g.set_dirty(f, true);
                queue.push(self.g.area_of(f));
            }
        }
    }

    /// `dependentsOf` with areas as seeds.
    pub(crate) fn dependents_of(&mut self, mut queue: Vec<Area>) -> Vec<(SheetIx, u32, u32)> {
        self.ensure_rev();
        self.g.epoch = self.g.epoch.wrapping_add(1);
        let epoch = self.g.epoch;
        let mut out = Vec::new();
        let mut hits = Vec::new();
        while let Some(a) = queue.pop() {
            hits.clear();
            if let Some(ix) = self.g.rev.as_ref().and_then(|r| r.get(a.sheet.0 as usize)) {
                ix.query(a, &mut hits);
            }
            for &f in &hits {
                let r = &mut self.g.recs[f as usize];
                if r.seen == epoch {
                    continue;
                }
                r.seen = epoch;
                let r = *r;
                out.push((SheetIx(r.sheet), r.row, u32::from(r.col)));
                queue.push(self.g.area_of(f));
            }
        }
        out
    }

    /// `pendingPrecedents(f)` (`engine.ts:799-823`), in the TS push order.
    fn pending(&mut self, f: u32, out: &mut Vec<u32>, deps: &mut Vec<Area>) {
        out.clear();
        self.deps_into(f, deps);
        let g = &*self.g;
        let ok = |x: u32| {
            let r = &g.recs[x as usize];
            r.has(fl::DIRTY) && !r.has(fl::STALE)
        };
        for a in deps.iter() {
            if a.sheet.0 >= EXTERNAL_BASE {
                continue;
            }
            let Some(sg) = g.sheets.get(a.sheet.0 as usize) else { continue };
            let visit = |fc: &super::graph::FCol, out: &mut Vec<u32>| {
                if fc.dirty == 0 {
                    return;
                }
                let mut i = super::graph::lower(&fc.rows, a.r1);
                while i < fc.rows.len() && fc.rows[i] <= a.r2 {
                    let x = fc.fids[i];
                    if ok(x) {
                        out.push(x);
                    }
                    i += 1;
                }
            };
            if u64::from(a.c2 - a.c1) < sg.fcols.len() as u64 {
                for c in a.c1..=a.c2 {
                    if let Some(&slot) = sg.col_slot.get(c as usize)
                        && slot != super::NONE
                    {
                        visit(&sg.fcols[slot as usize], out);
                    }
                }
            } else {
                for fc in &sg.fcols {
                    if fc.col >= a.c1 && fc.col <= a.c2 {
                        visit(fc, out);
                    }
                }
            }
            for &m in &g.masters {
                if ok(m)
                    && let Some(ma) = g.areas.get(&m)
                    && areas_overlap(*ma, *a)
                {
                    out.push(m);
                }
            }
        }
    }

    // ------------------------------------------------------------ evaluation

    /// `evaluateRec(f)`: `Err` only for `NeedCell`.
    fn evaluate(&mut self, f: u32) -> Result<(), u32> {
        let rec = self.g.recs[f as usize];
        if rec.has(fl::DATATABLE) {
            return Ok(());
        }
        if let Some(t) = self.g.trace.as_mut() {
            t.push((rec.sheet, rec.row, u32::from(rec.col)));
        }
        let (sheet, row, col) = (SheetIx(rec.sheet), rec.row, u32::from(rec.col));
        let v = if rec.node == super::NONE {
            Value::Scalar(Scalar::Err(ErrorCode::Name))
        } else {
            match eval!(self, |ev| ev.eval_formula(NodeId(rec.node), sheet, row, col, true)) {
                Ok(v) => v,
                Err(Throw::NeedCell(p)) => return Err(p as u32),
                Err(Throw::Range) => Value::Scalar(Scalar::Err(ErrorCode::Num)),
                Err(Throw::Internal(what)) => {
                    self.g.internal_errors.push(InternalError { sheet, row, col, what });
                    Value::Scalar(Scalar::Err(ErrorCode::Value))
                }
                Err(Throw::NotPorted(ix)) => {
                    self.g.not_ported.push((sheet, row, col, ix));
                    Value::Scalar(Scalar::Err(ErrorCode::Value))
                }
            }
        };
        let s = Scope { sheet, row, col, arr: true, env: None };
        let r = if !rec.has(fl::ARRAY) {
            match self.top_scalar(v, s) {
                Ok(x) => {
                    self.put(f, x);
                    Ok(())
                }
                Err(p) => Err(p),
            }
        } else if rec.has(fl::DYNAMIC) {
            self.spill(f, v)
        } else {
            self.fill_cse(f, v)
        };
        self.st.clear_pass();
        r
    }

    fn need(t: Throw) -> u32 {
        match t {
            Throw::NeedCell(p) => p as u32,
            // reads of the host throw nothing else
            _ => u32::MAX,
        }
    }

    /// `topScalar(v, ctx)`.
    fn top_scalar(&mut self, v: Value, s: Scope) -> Result<Scalar, u32> {
        let x = match v {
            Value::Ref(r) if r.one_sheet().is_none() && !r.areas().is_empty() => Scalar::Err(ErrorCode::Ref),
            Value::Ref(r) => eval!(self, |ev| ev.intersect(&r, s)).map_err(Self::need)?,
            Value::Array(a) => {
                if a.rows > 0 && a.cols > 0 {
                    a.get(0, 0)
                } else {
                    Scalar::Err(ErrorCode::Value)
                }
            }
            Value::Lambda(_) => Scalar::Err(ErrorCode::Calc),
            Value::Scalar(x) => x,
        };
        Ok(if matches!(x, Scalar::Empty) { Scalar::Num(0.0) } else { x })
    }

    fn read_ref(&mut self, r: &RefValue, single_cell: bool) -> Result<Placed, u32> {
        let Some(a) = r.single() else {
            let multi = r.areas().iter().any(|x| Some(x.sheet) != r.areas().first().map(|y| y.sheet));
            return Ok(Placed::S(Scalar::Err(if multi && single_cell { ErrorCode::Ref } else { ErrorCode::Value })));
        };
        if single_cell && a.r1 == a.r2 && a.c1 == a.c2 {
            let v = eval!(self, |ev| crate::eval::EvalHost::cell(&mut *ev.host, a.sheet, a.r1, a.c1)).map_err(Self::need)?;
            return Ok(Placed::S(v));
        }
        let arr = eval!(self, |ev| crate::eval::EvalHost::values(&mut *ev.host, a));
        match arr {
            Ok(x) => Ok(Placed::A(x)),
            Err(Throw::NeedCell(p)) => Err(p as u32),
            Err(_) => Ok(Placed::S(Scalar::Err(ErrorCode::Num))),
        }
    }

    /// `spill(st, f, v)` (`engine.ts:954-1011`).
    fn spill(&mut self, f: u32, v: Value) -> Result<(), u32> {
        let rec = self.g.recs[f as usize];
        let (s, row, col) = (rec.sheet, rec.row, u32::from(rec.col));
        let mut arr = match v {
            Value::Ref(r) => self.read_ref(&r, true)?,
            Value::Lambda(_) => Placed::S(Scalar::Err(ErrorCode::Calc)),
            Value::Array(a) => Placed::A(a),
            Value::Scalar(x) => Placed::S(x),
        };
        if let Placed::A(a) = &arr {
            if a.rows == 0 || a.cols == 0 {
                arr = Placed::S(Scalar::Err(ErrorCode::Calc));
            } else if a.rows == 1 && a.cols == 1 {
                arr = Placed::S(a.get(0, 0));
            }
        }
        let old = self.g.areas.get(&f).copied();
        let a = match arr {
            Placed::S(x) => {
                self.clear_area(f);
                self.put(f, if matches!(x, Scalar::Empty) { Scalar::Num(0.0) } else { x });
                if let Some(o) = old {
                    self.g.spill_changed.push(o);
                    self.touch_anchor(f);
                }
                return Ok(());
            }
            Placed::A(a) => a,
        };
        let (r2, c2) = (u64::from(row) + u64::from(a.rows) - 1, u64::from(col) + u64::from(a.cols) - 1);
        let mut blocked = r2 > u64::from(MAX_ROW) || c2 > u64::from(MAX_COL);
        let target =
            Area { sheet: SheetIx(s), r1: row, c1: col, r2: r2.min(u64::from(MAX_ROW)) as u32, c2: c2.min(u64::from(MAX_COL)) as u32 };
        if !blocked {
            let g = &*self.g;
            let store = &*self.store;
            store.each(target, |r, c, _| {
                if blocked || (r == row && c == col) {
                    return;
                }
                if g.current(s, r, c).is_some() {
                    blocked = true;
                    return;
                }
                match g.sheets[s as usize].owners.get(&ts_key(r, c)) {
                    Some(&o) => blocked |= o & !NULL_VIEW != f,
                    None => blocked |= g.view(store, SheetIx(s), r, c).is_some_and(|(v, _)| !matches!(v, Scalar::Empty)),
                }
            });
            if !blocked {
                blocked = self.store.sheet(SheetIx(s)).is_some_and(|sh| sh.merges.iter().any(|m| areas_overlap(*m, target)));
            }
            if !blocked {
                blocked = self.g.tables.iter().any(|t| areas_overlap(t.area, target));
            }
        }
        if blocked {
            self.clear_area(f);
            self.put(f, Scalar::Err(ErrorCode::Spill));
            if let Some(o) = old {
                self.g.spill_changed.push(o);
                self.touch_anchor(f);
            }
            return Ok(());
        }
        if let Some(o) = old {
            for (r, c) in self.owned_by(f, o) {
                if !area_contains(target, SheetIx(s), r, c) {
                    self.delete_owned(s, r, c);
                }
            }
        }
        for dr in 0..a.rows {
            for dc in 0..a.cols {
                let x = a.get(dr, dc);
                let val = if matches!(x, Scalar::Empty) { Scalar::Num(0.0) } else { x };
                if dr == 0 && dc == 0 {
                    self.put(f, val);
                    continue;
                }
                let (r, c) = (row + dr, col + dc);
                let mine = self.g.sheets[s as usize].owners.get(&ts_key(r, c)).is_some_and(|&o| o & !NULL_VIEW == f)
                    && self.g.view(self.store, SheetIx(s), r, c).is_some();
                if mine {
                    self.write(s, r, c, val);
                } else {
                    self.set_owned(f, s, r, c, val);
                }
            }
        }
        self.g.areas.insert(f, target);
        if old.is_none_or(|o| o.r2 != target.r2 || o.c2 != target.c2) {
            self.g.spill_changed.push(target);
            if let Some(o) = old {
                self.g.spill_changed.push(o);
            }
            self.touch_anchor(f);
        }
        Ok(())
    }

    /// The CSE fill (`engine.ts:914-926`).
    fn fill_cse(&mut self, f: u32, v: Value) -> Result<(), u32> {
        let rec = self.g.recs[f as usize];
        let (s, row, col) = (rec.sheet, rec.row, u32::from(rec.col));
        let arr = match v {
            Value::Ref(r) => match r.single() {
                None => Placed::S(Scalar::Err(ErrorCode::Value)),
                Some(_) => self.read_ref(&r, false)?,
            },
            Value::Lambda(_) => Placed::S(Scalar::Err(ErrorCode::Calc)),
            Value::Array(a) => Placed::A(a),
            Value::Scalar(x) => Placed::S(x),
        };
        let a = self.g.area_of(f);
        for r in a.r1..=a.r2 {
            for c in a.c1..=a.c2 {
                let x = match &arr {
                    Placed::A(m) => {
                        let (rr, cc) = (if m.rows == 1 { 0 } else { r - a.r1 }, if m.cols == 1 { 0 } else { c - a.c1 });
                        if rr >= m.rows || cc >= m.cols { Scalar::Err(ErrorCode::NA) } else { m.get(rr, cc) }
                    }
                    Placed::S(x) => *x,
                };
                let val = if matches!(x, Scalar::Empty) { Scalar::Num(0.0) } else { x };
                if r == row && c == col {
                    self.put(f, val);
                    continue;
                }
                let exists = self.g.view(self.store, SheetIx(s), r, c).is_some();
                let mine = exists && self.g.sheets[s as usize].owners.get(&ts_key(r, c)).is_some_and(|&o| o & !NULL_VIEW == f);
                if mine {
                    self.write(s, r, c, val);
                } else if self.g.current(s, r, c).is_none() {
                    self.set_owned(f, s, r, c, val);
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ the scheduler

    /// `computeRoot(root)` (`engine.ts:749-796`): an explicit stack; soft frames (static precedents) vs hard ones
    /// (the evaluation read the cell); a cycle of hard edges only is circular.
    fn compute_root(&mut self, root: u32, scratch: &mut Scratch) {
        let mut stack = core::mem::take(&mut scratch.stack);
        stack.clear();
        stack.push((root, true));
        self.g.recs[root as usize].set(fl::ON_STACK, true);
        while let Some(&(f, _)) = stack.last() {
            if !self.g.recs[f as usize].has(fl::DIRTY) {
                self.pop(&mut stack);
                continue;
            }
            if self.g.recs[f as usize].scan != self.g.pass {
                self.g.recs[f as usize].scan = self.g.pass;
                self.pending(f, &mut scratch.out, &mut scratch.deps);
                let mut pushed = false;
                for &g in &scratch.out {
                    let r = &mut self.g.recs[g as usize];
                    if g == f || r.has(fl::ON_STACK) {
                        continue;
                    }
                    r.set(fl::ON_STACK, true);
                    stack.push((g, false));
                    pushed = true;
                }
                if pushed {
                    continue;
                }
            }
            match self.evaluate(f) {
                Ok(()) => {
                    self.g.recs[f as usize].set(fl::COMPUTED, true);
                    self.g.set_dirty(f, false);
                    self.pop(&mut stack);
                }
                Err(g) => {
                    if g as usize >= self.g.recs.len() {
                        // not a NeedCell (cannot happen): drop the frame
                        self.g.set_dirty(f, false);
                        self.pop(&mut stack);
                        continue;
                    }
                    if !self.g.recs[g as usize].has(fl::ON_STACK) {
                        self.g.recs[g as usize].set(fl::ON_STACK, true);
                        stack.push((g, true));
                        continue;
                    }
                    let i = stack.iter().rposition(|&(x, _)| x == g).unwrap_or(0);
                    if let Some(k) = (i + 1..stack.len()).find(|&j| !stack[j].1) {
                        while stack.len() > k {
                            self.pop(&mut stack);
                        }
                        continue;
                    }
                    // a real cycle g … f: the previous values stay
                    let members: Vec<u32> = stack[i..].iter().map(|&(x, _)| x).collect();
                    for &m in &members {
                        let r = &mut self.g.recs[m as usize];
                        if !r.has(fl::CIRC) {
                            r.set(fl::CIRC, true);
                            self.g.circular.push(m);
                        }
                        let r = *r;
                        if !r.has(fl::COMPUTED)
                            && let Some(&v) = self.g.cached.get(&m)
                        {
                            // not calculated yet: the "previous value" is the file's (`rec.v = m.cached`)
                            let c = u32::from(r.col);
                            if self.g.view(self.store, SheetIx(r.sheet), r.row, c).is_some() {
                                self.store_write(r.sheet, r.row, c, v);
                            }
                        }
                    }
                    for &m in &members {
                        self.g.set_dirty(m, false);
                    }
                    while stack.len() > i {
                        self.pop(&mut stack);
                    }
                }
            }
        }
        scratch.stack = stack;
    }

    fn pop(&mut self, stack: &mut Vec<(u32, bool)>) {
        if let Some((f, _)) = stack.pop() {
            self.g.recs[f as usize].set(fl::ON_STACK, false);
        }
    }

    /// `computeDirty()`: every dirty formula in sheet order — through the dirty list when it is small (only dirty
    /// formulas are touched), else through `allFormulas`.
    fn compute_dirty(&mut self) {
        self.g.computing = true;
        self.g.pass = self.g.pass.wrapping_add(1);
        if self.g.pass == u32::MAX {
            // `scan` starts at u32::MAX: skip it
            self.g.pass = 0;
        }
        let mut scratch = Scratch::default();
        let use_ordered = self.g.dirty_total.saturating_mul(8) > u64::from(self.g.live);
        let list: Vec<u32> = if use_ordered {
            self.g.ensure_ordered();
            core::mem::take(&mut self.g.ordered)
        } else {
            let mut l = core::mem::take(&mut self.g.dirty_list);
            l.retain(|&f| self.g.recs[f as usize].has(fl::DIRTY));
            l.sort_unstable_by_key(|&f| self.g.order_key(f));
            l.dedup();
            l
        };
        self.g.dirty_list.clear();
        for &f in &list {
            if self.g.recs[f as usize].has(fl::DIRTY) {
                self.compute_root(f, &mut scratch);
            }
        }
        if use_ordered && self.g.ordered_ok {
            self.g.ordered = list;
        }
        self.g.dirty_list.clear();
        self.g.computing = false;
        for f in core::mem::take(&mut self.g.stale) {
            self.g.recs[f as usize].set(fl::STALE, false);
        }
    }

    /// `run()` (`engine.ts:713-728`).
    pub(crate) fn run(&mut self) {
        for f in core::mem::take(&mut self.g.circular) {
            self.g.recs[f as usize].set(fl::CIRC, false);
        }
        for _ in 0..10 {
            self.g.spill_changed.clear();
            self.compute_dirty();
            if self.g.spill_changed.is_empty() {
                break;
            }
            let changed = core::mem::take(&mut self.g.spill_changed);
            self.mark_dependents(changed);
            if self.g.dirty_total == 0 {
                break;
            }
        }
        if self.g.iterate.is_some() && !self.g.circular.is_empty() {
            self.iterate_cycles();
        }
        self.st.clear_pass();
    }

    fn cell_value(&self, f: u32) -> Scalar {
        let r = self.g.recs[f as usize];
        self.g.view(self.store, SheetIx(r.sheet), r.row, u32::from(r.col)).map_or(Scalar::Empty, |x| x.0)
    }

    /// `iterateCycles()` (`engine.ts:1030-1066`): Gauss-Seidel sweeps over the members in sheet order.
    fn iterate_cycles(&mut self) {
        let Some((count, delta)) = self.g.iterate else { return };
        let mut members = self.g.circular.clone();
        members.sort_unstable_by_key(|&f| self.g.order_key(f));
        self.g.computing = true;
        self.g.pass = self.g.pass.wrapping_add(1);
        for &m in &members {
            if !self.g.recs[m as usize].has(fl::STALE) {
                self.g.recs[m as usize].set(fl::STALE, true);
                self.g.stale.push(m);
            }
        }
        let mut scratch = Scratch::default();
        for sweep in 0..count {
            let mut max_delta = 0.0f64;
            self.st.clear_pass();
            for &m in &members {
                let v0 = self.cell_value(m);
                for _ in 0..10_000 {
                    match self.evaluate(m) {
                        Ok(()) => {
                            self.g.recs[m as usize].set(fl::COMPUTED, true);
                            break;
                        }
                        Err(g) if (g as usize) < self.g.recs.len() => self.compute_root(g, &mut scratch),
                        Err(_) => break,
                    }
                }
                let v1 = self.cell_value(m);
                match (v0, v1) {
                    (Scalar::Num(a), Scalar::Num(b)) => max_delta = uo_jsnum::math::max(max_delta, (b - a).abs()),
                    (a, b) => {
                        if !self.same(a, b) {
                            max_delta = f64::INFINITY;
                        }
                    }
                }
            }
            if sweep > 0 && max_delta < delta {
                break;
            }
        }
        self.g.computing = false;
        for f in core::mem::take(&mut self.g.stale) {
            self.g.recs[f as usize].set(fl::STALE, false);
        }
        // dependents of the cycles (calculated with the values from before)
        let deps: Vec<Area> = members.iter().map(|&f| self.g.area_of(f)).collect();
        self.mark_dependents(deps);
        for &m in &members {
            self.g.set_dirty(m, false);
        }
        let others = self.g.dirty_total > 0;
        if others {
            let circ = self.g.circular.clone();
            self.compute_dirty();
            for c in circ {
                if !self.g.recs[c as usize].has(fl::CIRC) {
                    self.g.recs[c as usize].set(fl::CIRC, true);
                    self.g.circular.push(c);
                }
            }
        }
    }

    // ------------------------------------------------------------ edits

    /// `removeFormula(rec)` with the reverse index kept.
    fn remove_formula(&mut self, f: u32) {
        if !self.g.recs[f as usize].has(fl::LIVE) {
            return;
        }
        self.rev_edit(f, false);
        self.g.free_formula(f);
    }

    /// `setCell(sheet, row, col, inp)` (`engine.ts:628-677`); `node`/`flags` were prepared by the caller.
    pub(crate) fn set_cell(
        &mut self,
        s: u32,
        row: u32,
        col: u32,
        input: Option<CellInput>,
        node: Option<NodeId>,
        flags: u16,
    ) -> crate::error::Result<()> {
        let sheet = SheetIx(s);
        let mut changed = vec![Area { sheet, r1: row, c1: col, r2: row, c2: col }];
        if let Some(f) = self.g.current(s, row, col) {
            if let Some(&a) = self.g.areas.get(&f) {
                changed.push(a);
                self.clear_area(f);
            }
            self.remove_formula(f);
        }
        let owner = self.g.sheets[s as usize].owners.remove(&ts_key(row, col)).map(|o| o & !NULL_VIEW);
        self.store.clear_content(sheet, row, col)?;
        self.g.touch(s, row, col);
        match input {
            Some(CellInput::Formula(fi)) => {
                let mut attrs = FormulaAttrs::default();
                if fi.array {
                    attrs.t = Some(FormulaType::Array);
                    attrs.r#ref.clone_from(&fi.r#ref);
                }
                if fi.ca {
                    attrs.flags |= FormulaAttrs::CA;
                }
                let attrs = (attrs != FormulaAttrs::default()).then(|| Box::new(attrs));
                self.store.set_formula(sheet, row, col, FileFormula { text: Some(fi.text.clone()), attrs })?;
                let fid = self.g.add_formula(s, row, col, node.map_or(super::NONE, |n| n.0), flags);
                self.rev_edit(fid, true);
                if flags & fl::DYNAMIC != 0 {
                    self.g.add_master(fid);
                }
                if fi.array
                    && !fi.dynamic
                    && let Some(a) = fi.r#ref.as_ref().and_then(|t| a1_to_area(t.as_js(), sheet))
                {
                    self.g.areas.insert(fid, a);
                    self.g.add_master(fid);
                    for rr in a.r1..=a.r2 {
                        for cc in a.c1..=a.c2 {
                            if rr != row || cc != col {
                                self.set_owned(fid, s, rr, cc, Scalar::Empty);
                                // `{ v: null, owner }`: the write above is a no-op for null on a fresh cell
                            }
                        }
                    }
                    changed.push(a);
                }
                self.g.set_dirty(fid, true);
            }
            Some(CellInput::Value(v)) if !matches!(v, Scalar::Empty) => {
                self.store_write(s, row, col, v);
            }
            Some(CellInput::Text(t)) => {
                self.store.write_value(sheet, row, col, CellWrite::Str(t.as_js()), WriteKind::Constant)?;
                self.g.note_row(s, row);
            }
            _ => {}
        }
        if let Some(o) = owner
            && self.g.recs[o as usize].has(fl::DYNAMIC)
            && self.g.recs[o as usize].has(fl::LIVE)
        {
            self.g.set_dirty(o, true);
        }
        let masters = self.g.masters.clone();
        for m in masters {
            let r = self.g.recs[m as usize];
            if !r.has(fl::DYNAMIC) || r.sheet != s {
                continue;
            }
            if self.g.areas.get(&m).is_some_and(|a| area_contains(*a, sheet, row, col)) {
                self.g.set_dirty(m, true);
            }
            if r.row <= row
                && u32::from(r.col) <= col
                && matches!(self.g.view(self.store, sheet, r.row, u32::from(r.col)), Some((Scalar::Err(ErrorCode::Spill), _)))
            {
                self.g.set_dirty(m, true);
            }
        }
        self.mark_dependents(changed);
        Ok(())
    }
}
