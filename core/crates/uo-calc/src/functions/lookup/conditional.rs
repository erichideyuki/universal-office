//! `conditional.ts`: COUNTIF, COUNTIFS, SUMIF, SUMIFS, AVERAGEIF, AVERAGEIFS, MAXIFS, MINIFS.
//!
//! Measured rules (the TS's): the criteria ranges of the *IFS (and the sum range) must have the same shape
//! (`#VALUE!`); in SUMIF/AVERAGEIF the sum range is only its corner, resized to the criteria range's shape; only the
//! numbers of the sum range count, an error in a matching cell propagates; an array criterion is lifted. Whole
//! columns walk the existing cells only (the matching empties enter through the size).
//!
//! **Reads (decision A4e-3).** The TS walks one area (`forEachCell`) and reads the other conditions' cells at each
//! position (`ctx.cell`), so which dirty precedent a NeedCell names depends on that order. Here every area is first
//! gathered with one walk (its existing cells, row by row) and the TS algorithm then runs over the gathered cells:
//! when every walk completed, nothing can throw and the result is the TS's; when a walk stopped at a dirty cell,
//! the positions after the last cell it saw are read for real (`ctx.cell`) at the moment the TS reads them, and a
//! walk of the TS over that area throws the walk's own NeedCell when it reaches the end of what was seen — so the
//! first throw is the TS's too.

use crate::criteria::{Criterion, parse_criterion};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::key::Area;
use crate::value::{ErrorCode, Scalar, Value};

use super::super::shared::{R, Stop, arg, bodies, len, num, scalar};
use super::{fail, view};

pub(super) const CONDITIONAL: &[(&str, FnBody)] = bodies![
    "COUNTIF" => countif,
    "COUNTIFS" => countifs,
    "SUMIF" => sumif,
    "AVERAGEIF" => averageif,
    "SUMIFS" => sumifs,
    "AVERAGEIFS" => averageifs,
    "MAXIFS" => maxifs,
    "MINIFS" => minifs,
];

/// `singleArea(v)`: the area of a one-area reference.
#[inline]
fn single_area(v: Option<&Value>) -> Option<Area> {
    match v {
        Some(Value::Ref(r)) if r.areas().len() == 1 => Some(r.areas()[0]),
        _ => None,
    }
}

#[inline]
fn shape(a: Area) -> (u32, u32) {
    (a.r2 - a.r1 + 1, a.c2 - a.c1 + 1)
}

/// `criterionArg(raw, ctx)`'s check: a text longer than 255 units → `#VALUE!` (measured). The parse itself never
/// fails and is done once the areas are read.
fn check_criterion(ctx: &dyn FnContext, raw: Scalar) -> Result<(), ErrorCode> {
    match raw {
        Scalar::Str(s) if ctx.text(s).len() > 255 => Err(ErrorCode::Value),
        _ => Ok(()),
    }
}

/// The existing cells of one area, gathered with one walk (`key` = `dr · cols + dc`, ascending).
struct Gathered {
    area: Area,
    cols: u64,
    keys: Vec<u64>,
    vals: Vec<Scalar>,
    /// The walk stopped at a dirty cell after the cells gathered (its NeedCell); `None` = complete.
    stop: Option<Throw>,
    /// The cursor of the last lookup.
    cur: usize,
}

impl Gathered {
    fn walk(area: Area, ctx: &mut dyn FnContext) -> R<Self> {
        let cols = u64::from(area.c2 - area.c1 + 1);
        let (mut keys, mut vals) = (Vec::new(), Vec::new());
        let r = ctx.for_each_cell(area, &mut |v, r, c| {
            keys.push(u64::from(r - area.r1) * cols + u64::from(c - area.c1));
            vals.push(v);
        });
        let stop = match r {
            Ok(()) => None,
            Err(t @ Throw::NeedCell(_)) => Some(t),
            Err(t) => return Err(Stop::Throw(t)),
        };
        Ok(Self { area, cols, keys, vals, stop, cur: 0 })
    }

    /// The gathered value at `key`: `Some(v)` (empty when no cell), `None` = past what the stopped walk saw.
    fn get(&mut self, key: u64) -> Option<Scalar> {
        if self.stop.is_some() && self.keys.last().is_none_or(|&l| key > l) {
            return None;
        }
        // a forward cursor (the walks ask in ascending order), a binary search when it goes back
        if self.keys.get(self.cur).is_some_and(|&k| k > key) || self.cur > self.keys.len() {
            self.cur = self.keys.partition_point(|&k| k < key);
        } else {
            while self.keys.get(self.cur).is_some_and(|&k| k < key) {
                self.cur += 1;
            }
        }
        Some(match self.keys.get(self.cur) {
            Some(&k) if k == key => self.vals[self.cur],
            _ => Scalar::Empty,
        })
    }
}

/// The areas of a call, gathered once each (a repeated area is walked once).
struct Areas {
    g: Vec<Gathered>,
}

impl Areas {
    fn new(list: &[Area], ctx: &mut dyn FnContext) -> R<(Self, Vec<usize>)> {
        let mut g: Vec<Gathered> = Vec::with_capacity(list.len());
        let mut ix = Vec::with_capacity(list.len());
        for &a in list {
            match g.iter().position(|x| x.area == a) {
                Some(i) => ix.push(i),
                None => {
                    ix.push(g.len());
                    g.push(Gathered::walk(a, ctx)?);
                }
            }
        }
        Ok((Self { g }, ix))
    }

    fn complete(&self) -> bool {
        self.g.iter().all(|x| x.stop.is_none())
    }
}

/// The context as the algorithm sees it: only read when every walk completed (the criteria borrow its locale),
/// read and written when a walk stopped (cells past it are read at the TS's moment).
enum Ctx<'a> {
    Ro(&'a dyn FnContext),
    Rw(&'a mut dyn FnContext),
}

/// What the TS algorithm reads through: the gathered cells, and `ctx.cell` past a stopped walk.
struct Src<'a> {
    areas: &'a mut Areas,
    ctx: Ctx<'a>,
}

impl Src<'_> {
    #[inline]
    fn c(&self) -> &dyn FnContext {
        match &self.ctx {
            Ctx::Ro(c) => *c,
            Ctx::Rw(c) => &**c,
        }
    }

    /// `ctx.cell(area.sheet, area.r1 + dr, area.c1 + dc)`.
    fn read(&mut self, k: usize, key: u64) -> R<Scalar> {
        let g = &mut self.areas.g[k];
        if let Some(v) = g.get(key) {
            return Ok(v);
        }
        let (dr, dc) = ((key / g.cols) as u32, (key % g.cols) as u32);
        let a = g.area;
        match &mut self.ctx {
            Ctx::Rw(c) => Ok(c.cell(a.sheet, a.r1 + dr, a.c1 + dc)?),
            // every walk completed: a lookup always answers
            Ctx::Ro(_) => Err(Stop::Throw(Throw::Internal("conditional: read past a complete walk"))),
        }
    }
}

/// A condition: its area (index in [`Areas`]) and its criterion.
struct Cond<'l> {
    k: usize,
    crit: Criterion<'l>,
}

/// `allMatch(conds, ctx, dr, dc, skip)`.
fn all_match(conds: &[Cond<'_>], src: &mut Src<'_>, key: u64, skip: Option<usize>) -> R<bool> {
    for (i, c) in conds.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
        let v = src.read(c.k, key)?;
        if !c.crit.matches(view(src.c(), v)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `countMatches(conds, ctx)`: positions where every condition matches, without walking empty cells.
fn count_matches(conds: &[Cond<'_>], src: &mut Src<'_>, rows: u32, cols: u32) -> R<f64> {
    if let Some(p) = conds.iter().position(|c| !c.crit.blank) {
        // walk the existing cells of a criterion that does not match an empty cell
        let k = conds[p].k;
        let mut n = 0.0;
        let len = src.areas.g[k].keys.len();
        for j in 0..len {
            let (key, v) = (src.areas.g[k].keys[j], src.areas.g[k].vals[j]);
            if conds[p].crit.matches(view(src.c(), v)) && all_match(conds, src, key, Some(p))? {
                n += 1.0;
            }
        }
        if let Some(t) = src.areas.g[k].stop {
            return Err(Stop::Throw(t));
        }
        return Ok(n);
    }
    // every criterion matches empty: the total minus the positions (with some cell) that fail
    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut fails = 0.0;
    let mut walked: Vec<usize> = Vec::new();
    for c in conds {
        let k = c.k;
        // a repeated area: its walk sees only positions already seen (no read can differ)
        if walked.contains(&k) {
            if let Some(t) = src.areas.g[k].stop {
                return Err(Stop::Throw(t));
            }
            continue;
        }
        walked.push(k);
        let len = src.areas.g[k].keys.len();
        for j in 0..len {
            let key = src.areas.g[k].keys[j];
            if !seen.insert(key) {
                continue;
            }
            if !all_match(conds, src, key, None)? {
                fails += 1.0;
            }
        }
        if let Some(t) = src.areas.g[k].stop {
            return Err(Stop::Throw(t));
        }
    }
    Ok(f64::from(rows) * f64::from(cols) - fails)
}

/// The reduction of the matching numbers, streamed (`rSum`, `rAvg`, `rMax`, `rMin`: the same additions and
/// comparisons in the same order as over the TS list).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reduce {
    Sum,
    Avg,
    Max,
    Min,
}

/// `matchedValues(conds, sum, ctx)` folded by `reduce`: the numbers of the sum range at matching positions (row by
/// row), or the first error.
fn matched_reduce(conds: &[Cond<'_>], src: &mut Src<'_>, sum: usize, rows: u32, cols: u32, reduce: Reduce) -> R<Value> {
    let mut err: Option<ErrorCode> = None;
    let (mut s, mut n) = (0.0, 0usize);
    let mut m = match reduce {
        Reduce::Max => f64::NEG_INFINITY,
        _ => f64::INFINITY,
    };
    let len = src.areas.g[sum].keys.len();
    let gcols = src.areas.g[sum].cols;
    for j in 0..len {
        if err.is_some() {
            break;
        }
        let (key, v) = (src.areas.g[sum].keys[j], src.areas.g[sum].vals[j]);
        let (dr, dc) = (key / gcols, key % gcols);
        if dr >= u64::from(rows) || dc >= u64::from(cols) {
            continue;
        }
        if !matches!(v, Scalar::Num(_) | Scalar::Err(_)) {
            continue;
        }
        if !all_match(conds, src, key, None)? {
            continue;
        }
        match v {
            Scalar::Err(e) => err = Some(e),
            Scalar::Num(x) => {
                n += 1;
                match reduce {
                    Reduce::Sum | Reduce::Avg => s += x,
                    Reduce::Max => {
                        if x > m {
                            m = x;
                        }
                    }
                    Reduce::Min => {
                        if x < m {
                            m = x;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // the walk goes on to the end of the sum range (checking each cell) even after an error
    if let Some(t) = src.areas.g[sum].stop {
        return Err(Stop::Throw(t));
    }
    if let Some(e) = err {
        return fail(e);
    }
    Ok(match reduce {
        // plain sum, WITHOUT Excel's zero: =SUMIFS(…) of 0.1; 0.2; −0.3 gives 5.55E−17 (measured)
        Reduce::Sum => {
            if s.is_finite() {
                num(s)
            } else {
                return fail(ErrorCode::Num);
            }
        }
        // `rSum(xs) as number / xs.length`: a non-finite sum is the `#NUM!` object divided — NaN
        Reduce::Avg => {
            if n == 0 {
                return fail(ErrorCode::Div0);
            }
            num(if s.is_finite() { s / n as f64 } else { f64::NAN })
        }
        Reduce::Max | Reduce::Min => num(if n > 0 { m } else { 0.0 }),
    })
}

/// Runs `f` over the gathered areas with the criteria parsed: against the context's locale when every walk
/// completed (no cell is read again), against a copy when a walk stopped (cells are then read between matches).
fn with_conds<T>(
    ctx: &mut dyn FnContext,
    areas: &mut Areas,
    raw: &[(usize, Scalar)],
    f: &mut dyn FnMut(&[Cond<'_>], &mut Src<'_>) -> R<T>,
) -> R<T> {
    let year = ctx.year();
    if areas.complete() {
        let c: &dyn FnContext = ctx;
        let conds: Vec<Cond<'_>> =
            raw.iter().map(|&(k, v)| Cond { k, crit: parse_criterion(view(c, v), c.locale(), year, false) }).collect();
        let mut src = Src { areas, ctx: Ctx::Ro(c) };
        return f(&conds, &mut src);
    }
    let loc = ctx.locale().clone();
    let conds: Vec<Cond<'_>> = raw.iter().map(|&(k, v)| Cond { k, crit: parse_criterion(view(ctx, v), &loc, year, false) }).collect();
    let mut src = Src { areas, ctx: Ctx::Rw(ctx) };
    f(&conds, &mut src)
}

// ───────────────────────────── bodies ─────────────────────────────

fn countif(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(area) = single_area(arg(a, 0)) else { return fail(ErrorCode::Value) };
    let crit = scalar(a, 1).unwrap_or(Scalar::Empty);
    check_criterion(ctx, crit)?;
    let (rows, cols) = shape(area);
    let (mut areas, ix) = Areas::new(&[area], ctx)?;
    let n = with_conds(ctx, &mut areas, &[(ix[0], crit)], &mut |conds, src| count_matches(conds, src, rows, cols))?;
    Ok(num(n))
}

/// `buildConds(args, start, ctx)`'s checks: one-area references of one shape, criteria no longer than 255.
fn build_conds(a: &CallArgs<'_>, start: usize, ctx: &dyn FnContext) -> R<Vec<(Area, Scalar)>> {
    let mut out: Vec<(Area, Scalar)> = Vec::new();
    let mut i = start;
    while i < len(a) {
        let Some(area) = single_area(arg(a, i)) else { return fail(ErrorCode::Value) };
        if i + 1 >= len(a) {
            return fail(ErrorCode::Value);
        }
        if let Some(&(first, _)) = out.first()
            && shape(area) != shape(first)
        {
            return fail(ErrorCode::Value);
        }
        let crit = scalar(a, i + 1).unwrap_or(Scalar::Empty);
        check_criterion(ctx, crit)?;
        out.push((area, crit));
        i += 2;
    }
    Ok(out)
}

fn countifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !len(a).is_multiple_of(2) {
        return fail(ErrorCode::Value);
    }
    let conds = build_conds(a, 0, ctx)?;
    let Some(&(first, _)) = conds.first() else { return fail(ErrorCode::Value) };
    let (rows, cols) = shape(first);
    let list: Vec<Area> = conds.iter().map(|c| c.0).collect();
    let (mut areas, ix) = Areas::new(&list, ctx)?;
    let raw: Vec<(usize, Scalar)> = conds.iter().zip(&ix).map(|(c, &k)| (k, c.1)).collect();
    let n = with_conds(ctx, &mut areas, &raw, &mut |conds, src| count_matches(conds, src, rows, cols))?;
    Ok(num(n))
}

/// SUMIF/AVERAGEIF: (range, criterion, [sum]) — the sum range resized from its corner.
fn if_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, reduce: Reduce) -> R<Value> {
    let Some(area) = single_area(arg(a, 0)) else { return fail(ErrorCode::Value) };
    let (rows, cols) = shape(area);
    let mut sum = area;
    if !matches!(arg(a, 2), None | Some(Value::Scalar(Scalar::Empty))) {
        let Some(s) = single_area(arg(a, 2)) else { return fail(ErrorCode::Value) };
        sum = Area { sheet: s.sheet, r1: s.r1, c1: s.c1, r2: s.r1 + rows - 1, c2: s.c1 + cols - 1 };
    }
    let crit = scalar(a, 1).unwrap_or(Scalar::Empty);
    check_criterion(ctx, crit)?;
    let (mut areas, ix) = Areas::new(&[area, sum], ctx)?;
    with_conds(ctx, &mut areas, &[(ix[0], crit)], &mut |conds, src| matched_reduce(conds, src, ix[1], rows, cols, reduce))
}

/// SUMIFS/AVERAGEIFS/MAXIFS/MINIFS: (sum, range1, criterion1, …).
fn ifs_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, reduce: Reduce) -> R<Value> {
    if len(a).is_multiple_of(2) {
        return fail(ErrorCode::Value);
    }
    let Some(sum) = single_area(arg(a, 0)) else { return fail(ErrorCode::Value) };
    let conds = build_conds(a, 1, ctx)?;
    let Some(&(first, _)) = conds.first() else { return fail(ErrorCode::Value) };
    if shape(sum) != shape(first) {
        return fail(ErrorCode::Value);
    }
    let (rows, cols) = shape(first);
    let mut list: Vec<Area> = conds.iter().map(|c| c.0).collect();
    list.push(sum);
    let (mut areas, ix) = Areas::new(&list, ctx)?;
    let raw: Vec<(usize, Scalar)> = conds.iter().zip(&ix).map(|(c, &k)| (k, c.1)).collect();
    let s = ix[conds.len()];
    with_conds(ctx, &mut areas, &raw, &mut |conds, src| matched_reduce(conds, src, s, rows, cols, reduce))
}

fn sumif(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if_fn(a, ctx, Reduce::Sum)
}

fn averageif(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if_fn(a, ctx, Reduce::Avg)
}

fn sumifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ifs_fn(a, ctx, Reduce::Sum)
}

fn averageifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ifs_fn(a, ctx, Reduce::Avg)
}

fn maxifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ifs_fn(a, ctx, Reduce::Max)
}

fn minifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ifs_fn(a, ctx, Reduce::Min)
}
