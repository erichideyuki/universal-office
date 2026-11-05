//! The column-width model of `table.ts` (Word's autofit, oracles tblw/tblpct/tblpctmin/tblmincol/
//! nowrap/tblsqueeze): `widthTw` 266, `GeoCell` 274, `boundaryWidths` 293, `prefOf` 321,
//! `fitPctMins` 327, `fixedMins` 352, `columnWidths`/`columnWidthsExact` 396–517, and
//! `paraContentWidths` 576 (the min/max line widths a cell paragraph asks for). Pure over numbers
//! except the measures, which `layout.rs` answers in the TS call order.
//!
//! Arrays are read the JS way: an index that is negative, fractional, NaN or past the end reads
//! `undefined` (NaN in arithmetic, [`at`]), a write there is invisible to the array (ignored); the
//! `slice` arguments follow `Array.prototype.slice` ([`slice`]).

use uo_jsnum::math as jm;

use super::{at, slice};
use crate::error::{LResult, LayoutError, LimitKind};
use crate::model::{JsNumber, WidthSpec};
use crate::output::TabAlign;
use crate::para::{GlyphFlags, GlyphKind, MarkerSuffix, PreparedPara};
use crate::word::obj::js_sort;

/// The `type` of a `WidthSpec` as `prefOf` and `widthTw` read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WType {
    Dxa,
    Pct,
    /// `auto`, `nil` or any other string
    Other,
}

/// A `WidthSpec` as numbers: `w` with `null` as 0 (`ToNumber`, the arithmetic and comparisons of
/// every site), and whether it was `null` (`widthTw` of a `dxa` returns the `null`, which every
/// caller coalesces with `??`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wspec {
    pub w: f64,
    pub ty: WType,
    pub null: bool,
}

impl Wspec {
    pub(crate) fn of(s: &WidthSpec) -> Wspec {
        let ty = match &*s.r#type {
            "dxa" => WType::Dxa,
            "pct" => WType::Pct,
            _ => WType::Other,
        };
        Wspec { w: s.w.to_number(), ty, null: matches!(s.w, JsNumber::Null) }
    }
}

/// `widthTw(w, base)` (table.ts:266): twips of a `dxa`, the fraction of `base` of a `pct`
/// (fiftieths of a percent); `undefined` otherwise (and for a `dxa` of `null`, which every caller
/// coalesces with `??`/`||` like `undefined`).
pub(crate) fn width_tw(w: Option<&WidthSpec>, base: f64) -> Option<f64> {
    let w = Wspec::of(w?);
    match w.ty {
        WType::Dxa if w.null => None,
        WType::Dxa => Some(w.w),
        WType::Pct => Some((base * w.w) / 5000.0),
        WType::Other => None,
    }
}

/// `prefOf(w)` (table.ts:321): `dxa`/`pct` when the width is positive, else `auto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pref {
    Dxa,
    Pct,
    Auto,
}

pub(crate) fn pref_of(w: Option<&Wspec>) -> Pref {
    match w {
        Some(w) if w.w > 0.0 && w.ty == WType::Dxa => Pref::Dxa,
        Some(w) if w.w > 0.0 && w.ty == WType::Pct => Pref::Pct,
        _ => Pref::Auto,
    }
}

/// What a `GeoCell`'s `measure` closure measures (`layout.rs` answers it).
#[derive(Clone, Copy, Debug)]
pub(crate) enum GeoSrc {
    /// slot `j` of row `ri`
    Slot { ri: usize, j: usize },
    /// a `gridBefore`/`gridAfter` gap: `{ min: 0, max: 0 }`
    Gap,
}

/// `GeoCell` (table.ts:274).
#[derive(Clone, Debug)]
pub(crate) struct GeoCell {
    /// the cell's position in the flat list (`cells.flat()`): the key of the `measure` memo
    pub ix: usize,
    pub col: f64,
    pub span: f64,
    pub tc_w: Option<Wspec>,
    pub src: GeoSrc,
    pub floor: bool,
    /// `pad` (`undefined` for a grid gap)
    pub pad: Option<f64>,
}

/// `{ min, max }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MinMax {
    pub min: f64,
    pub max: f64,
}

/// The `measure` of every cell, answered by the caller in the order the TS calls it.
pub(crate) type Measure<'m> = dyn FnMut(&GeoCell) -> LResult<MinMax> + 'm;

fn sum(v: &[f64]) -> f64 {
    v.iter().fold(0.0, |a, x| a + x)
}

fn bad(site: &'static str) -> LayoutError {
    LayoutError::Internal(site)
}

/// `boundaryWidths(cells, nCols, v, spread?)` (table.ts:293).
fn boundary_widths(
    cells: &[GeoCell],
    n_cols: usize,
    v: &mut dyn FnMut(&GeoCell) -> f64,
    spread: Option<&dyn Fn(&GeoCell) -> bool>,
) -> LResult<Vec<f64>> {
    let nc = n_cols as f64;
    let mut by_end: Vec<Vec<(f64, f64)>> = vec![Vec::new(); n_cols + 1];
    let mut late: Vec<&GeoCell> = Vec::new();
    for c in cells {
        let s = jm::min(c.col, nc);
        let e = jm::min(nc, c.col + c.span);
        if e <= s {
            continue;
        }
        if e - s > 1.0 && spread.is_some_and(|f| f(c)) {
            late.push(c);
        } else {
            // `byEnd[e]!.push(…)`: an `e` that is no index of `byEnd` throws (TypeError)
            let k = uo_jsnum::conv::js_index(e, by_end.len()).ok_or_else(|| bad("table.ts:301 byEnd[e]"))?;
            let val = v(c);
            by_end.get_mut(k).ok_or_else(|| bad("table.ts:301 byEnd[e]"))?.push((s, val));
        }
    }
    let mut b = vec![0.0];
    for k in 1..=n_cols {
        let mut m = at(&b, (k - 1) as f64);
        for &(start, val) in by_end.get(k).map_or(&[][..], Vec::as_slice) {
            m = jm::max(m, at(&b, start) + val);
        }
        b.push(m);
    }
    let mut w: Vec<f64> = b.iter().skip(1).enumerate().map(|(k, x)| x - at(&b, k as f64)).collect();
    js_sort(&mut late, |a, z| a.span - z.span);
    for c in late {
        let s = jm::min(c.col, nc);
        let e = jm::min(nc, c.col + c.span);
        let have = sum(slice(&w, s, Some(e)));
        let need = v(c) - have;
        if need <= 0.0 {
            continue;
        }
        let mut k = s;
        while k < e {
            if let Some(i) = uo_jsnum::conv::js_index(k, w.len())
                && let Some(x) = w.get_mut(i)
            {
                let old = *x;
                *x = old + (if have > 0.0 { (need * old) / have } else { need / (e - s) });
            }
            k += 1.0;
        }
    }
    Ok(w)
}

/// `fitPctMins(p, cells, minOf, colMin, pct)` (table.ts:327).
fn fit_pct_mins(p: &mut [f64], cells: &[(&GeoCell, MinMax)], col_min: &[f64], pct: &dyn Fn(f64) -> bool) {
    let n = p.len();
    for _ in 0..(2 * n + 2) {
        let mut worst: Option<&GeoCell> = None;
        let mut need = 1e-6;
        for (c, mm) in cells {
            if !pct(c.col) {
                continue;
            }
            let d = mm.min - sum(slice(p, c.col, Some(c.col + c.span)));
            if d > need {
                need = d;
                worst = Some(c);
            }
        }
        let Some(worst) = worst else { break };
        let s = worst.col;
        let e = jm::min(n as f64, worst.col + worst.span);
        let others: Vec<usize> = (0..n)
            .filter(|&k| {
                let kf = k as f64;
                (kf < s || kf >= e) && pct(kf) && at(p, kf) > at(col_min, kf) + 1e-6
            })
            .collect();
        let have = sum(slice(p, s, Some(e)));
        let mut k = s;
        while k < e {
            if let Some(i) = uo_jsnum::conv::js_index(k, n)
                && let Some(x) = p.get_mut(i)
            {
                let old = *x;
                *x = old + (if have > 0.0 { (need * old) / have } else { need / (e - s) });
            }
            k += 1.0;
        }
        // no other pct column to give way: the table grows past what it asked for (tblpctmin)
        if others.is_empty() {
            continue;
        }
        let share = need / others.len() as f64;
        for k in others {
            if let Some(x) = p.get_mut(k) {
                *x -= share;
            }
        }
    }
}

/// `fixedMins(cells, w)` (table.ts:352): no cell narrower than its margins + 20 twips.
fn fixed_mins(cells: &[GeoCell], w: &[f64], max_cols: u32) -> LResult<Vec<f64>> {
    let mut out = w.to_vec();
    let n = out.len() as f64;
    let mut locked: Vec<bool> = vec![false; out.len()];
    for _ in 0..4 {
        let mut changed = false;
        for c in cells {
            // (a gridBefore/gridAfter gap is no cell)
            let Some(pad) = c.pad else { continue };
            // `Array.from({ length: Math.max(0, Math.min(out.length, c.col + c.span) - c.col) }, …)`
            let len = jm::max(0.0, jm::min(n, c.col + c.span) - c.col);
            let len = if len.is_nan() { 0.0 } else { jm::trunc(len) };
            if len > f64::from(max_cols) {
                return Err(LayoutError::Limit(LimitKind::Table));
            }
            let cols: Vec<f64> = (0..uo_jsnum::conv::to_uint32(len)).map(|k| c.col + f64::from(k)).collect();
            if cols.is_empty() {
                continue;
            }
            let have = cols.iter().fold(0.0, |a, &k| a + at(&out, k));
            let need = pad + 20.0 - have;
            if need <= 0.5 {
                continue;
            }
            let in_cols = |k: usize| cols.contains(&(k as f64));
            let donors: Vec<usize> = (0..out.len()).filter(|&k| !in_cols(k) && !locked.get(k).copied().unwrap_or(false)).collect();
            let pool = donors.iter().fold(0.0, |a, &k| a + at(&out, k as f64));
            if pool <= need {
                continue;
            }
            let ncols = cols.len() as f64;
            for &k in &cols {
                if let Some(i) = uo_jsnum::conv::js_index(k, out.len()) {
                    if let Some(x) = out.get_mut(i) {
                        let old = *x;
                        *x = old + (if have > 0.0 { (need * old) / have } else { need / ncols });
                    }
                    if let Some(l) = locked.get_mut(i) {
                        *l = true;
                    }
                }
            }
            for k in donors {
                if let Some(x) = out.get_mut(k) {
                    let old = *x;
                    *x = old - (need * old) / pool;
                }
            }
            changed = true;
        }
        if !changed {
            break;
        }
    }
    Ok(out)
}

/// The inputs of `columnWidths` besides the cells.
pub(crate) struct WidthIn<'g> {
    pub n_cols: usize,
    pub doc_grid: &'g [f64],
    pub tbl_w: Option<Wspec>,
    pub fixed: bool,
    pub base: f64,
    pub avail: f64,
    /// `max_table_cells` (the `Array.from` lengths)
    pub max_cols: u32,
}

/// `columnWidths(…)` (table.ts:396): the exact widths rounded to whole twips (`Math.round`).
pub(crate) fn column_widths(cells: &[GeoCell], i: &WidthIn<'_>, measure: &mut Measure<'_>) -> LResult<Vec<f64>> {
    Ok(column_widths_exact(cells, i, measure)?.into_iter().map(jm::round).collect())
}

/// `{ k, w }` of `kind(c, T)` (table.ts:442).
fn kind(c: &GeoCell, t: f64, is_pct: &[bool]) -> (Pref, f64) {
    match pref_of(c.tc_w.as_ref()) {
        Pref::Pct => {
            let col_pct =
                c.span == 1.0 && uo_jsnum::conv::js_index(c.col, is_pct.len()).and_then(|j| is_pct.get(j).copied()).unwrap_or(false);
            if col_pct { (Pref::Pct, 0.0) } else { (Pref::Dxa, (t * c.tc_w.map_or(0.0, |w| w.w)) / 5000.0) }
        }
        Pref::Dxa => (Pref::Dxa, c.tc_w.map_or(0.0, |w| w.w)),
        Pref::Auto => (Pref::Auto, 0.0),
    }
}

fn sum_of(v: &[f64], pick: &dyn Fn(usize) -> bool) -> f64 {
    v.iter().enumerate().fold(0.0, |a, (j, x)| a + (if pick(j) { *x } else { 0.0 }))
}

/// `columnWidthsExact(…)` (table.ts:401).
#[expect(clippy::too_many_lines, reason = "one TS function, kept in its order")]
fn column_widths_exact(cells: &[GeoCell], i: &WidthIn<'_>, measure: &mut Measure<'_>) -> LResult<Vec<f64>> {
    let n_cols = i.n_cols;
    let nc = n_cols as f64;
    let doc_grid = i.doc_grid;
    let tw = pref_of(i.tbl_w.as_ref());
    let t0: Option<f64> = match (tw, i.tbl_w) {
        (Pref::Dxa, Some(w)) => Some(w.w),
        (Pref::Pct, Some(w)) => Some((i.base * w.w) / 5000.0),
        _ => None,
    };
    if i.fixed {
        let t_ref = t0.unwrap_or(i.base);
        let mut v = |c: &GeoCell| match pref_of(c.tc_w.as_ref()) {
            Pref::Dxa => c.tc_w.map_or(0.0, |w| w.w),
            Pref::Pct => (t_ref * c.tc_w.map_or(0.0, |w| w.w)) / 5000.0,
            Pref::Auto => sum(slice(doc_grid, c.col, Some(c.col + c.span))),
        };
        let w = boundary_widths(cells, n_cols, &mut v, None)?;
        let s = sum(&w);
        let scaled: Vec<f64> = match t0 {
            Some(t) if s > 0.0 => w.iter().map(|x| (x * t) / s).collect(),
            _ => w,
        };
        return fixed_mins(cells, &scaled, i.max_cols);
    }
    // `for (const c of all) m.set(c, c.measure())`: every cell, in order
    let mut m: Vec<MinMax> = Vec::with_capacity(cells.len());
    for c in cells {
        m.push(measure(c)?);
    }
    let mm = |k: usize| m.get(k).copied().unwrap_or(MinMax { min: f64::NAN, max: f64::NAN });
    let min_of = |c: &GeoCell| mm(c.ix).min;
    let max_of = |c: &GeoCell| mm(c.ix).max;
    let always = |_: &GeoCell| true;
    let g1 = boundary_widths(cells, n_cols, &mut |c| min_of(c), Some(&always))?;
    let paired: Vec<(&GeoCell, MinMax)> = cells.iter().enumerate().map(|(k, c)| (c, mm(k))).collect();
    if !cells.is_empty() && cells.iter().all(|c| pref_of(c.tc_w.as_ref()) == Pref::Pct) {
        // only pct (as Word writes a converted .doc): fractions per boundary, merged cells included
        let t = match t0 {
            Some(t) => t,
            None => {
                let wants: Vec<f64> = cells.iter().map(|c| (max_of(c) * 5000.0) / c.tc_w.map_or(0.0, |w| w.w)).collect();
                jm::min(jm::max_n(&wants), i.avail)
            }
        };
        let mut p = boundary_widths(cells, n_cols, &mut |c| (t * c.tc_w.map_or(0.0, |w| w.w)) / 5000.0, None)?;
        // past 100%: the last column keeps what is left (f2)
        let mut room = t;
        for x in p.iter_mut().take(n_cols) {
            *x = jm::max(0.0, jm::min(*x, room));
            room -= *x;
        }
        fit_pct_mins(&mut p, &paired, &g1, &|_| true);
        let sp = sum(&p);
        // the longest word widens the table past the request: up to the text area when the
        // request is ≤ 100%; above 100% without a limit (tblpctmin)
        let cap = match t0 {
            Some(t0) if t0 > i.base + 0.5 => f64::INFINITY,
            _ => jm::max(t, i.base),
        };
        if sp > cap + 0.5 {
            return Ok(p.iter().map(|x| (x * cap) / sp).collect());
        }
        return Ok(if sp > 0.0 && t > sp + 0.5 { p.iter().map(|x| (x * t) / sp).collect() } else { p });
    }
    // pct column: a cell without a merge in pct and none in dxa; the fraction is the largest
    let frac: Vec<f64> = (0..n_cols)
        .map(|j| {
            let jf = j as f64;
            let own: Vec<&GeoCell> = cells.iter().filter(|c| c.span == 1.0 && c.col == jf).collect();
            if !own.iter().any(|c| pref_of(c.tc_w.as_ref()) == Pref::Pct) || own.iter().any(|c| pref_of(c.tc_w.as_ref()) == Pref::Dxa) {
                return 0.0;
            }
            let fs: Vec<f64> =
                own.iter().filter(|c| pref_of(c.tc_w.as_ref()) == Pref::Pct).map(|c| c.tc_w.map_or(0.0, |w| w.w) / 5000.0).collect();
            jm::max_n(&fs)
        })
        .collect();
    let is_pct: Vec<bool> = frac.iter().map(|f| *f > 0.0).collect();
    let pct_at = |j: usize| is_pct.get(j).copied().unwrap_or(false);
    let g_max = boundary_widths(cells, n_cols, &mut |c| max_of(c), Some(&always))?;
    let is_pct_ref = &is_pct;
    let not_dxa = |t: f64| move |c: &GeoCell| kind(c, t, is_pct_ref).0 != Pref::Dxa;
    let g3_of = |t: f64| -> LResult<Vec<f64>> {
        let sp = not_dxa(t);
        boundary_widths(
            cells,
            n_cols,
            &mut |c| {
                let (k, w) = kind(c, t, &is_pct);
                if k == Pref::Dxa { jm::max(w, min_of(c)) } else { min_of(c) }
            },
            Some(&sp),
        )
    };
    let g4_of = |t: f64| -> LResult<Vec<f64>> {
        let sp = not_dxa(t);
        boundary_widths(
            cells,
            n_cols,
            &mut |c| {
                let (k, w) = kind(c, t, &is_pct);
                match k {
                    Pref::Dxa => jm::max(w, min_of(c)),
                    Pref::Auto => max_of(c),
                    Pref::Pct => min_of(c),
                }
            },
            Some(&sp),
        )
    };
    let mut t = match t0 {
        Some(t) => t,
        None => {
            let g4 = g4_of(0.0)?;
            let rest = sum_of(&g4, &|j| !pct_at(j));
            let fr = jm::min(1.0, sum(&frac));
            let mut tt = rest;
            if fr > 0.0 {
                // the narrowest width where every pct column fits its fraction, the rest the remainder
                for j in 0..n_cols {
                    if pct_at(j) {
                        tt = jm::max(tt, at(&g_max, j as f64) / at(&frac, j as f64));
                    }
                }
                if fr < 1.0 {
                    tt = jm::max(tt, rest / (1.0 - fr));
                }
            }
            jm::min(tt, i.avail)
        }
    };
    let explicit = t0.is_some();
    // the pct columns
    let mut p: Vec<f64> = vec![0.0; n_cols];
    if is_pct.iter().any(|x| *x) {
        let mut room = t - sum_of(&g1, &|j| !pct_at(j));
        for (j, x) in p.iter_mut().enumerate() {
            if pct_at(j) {
                *x = jm::max(0.0, jm::min(at(&frac, j as f64) * t, room));
                room -= *x;
            }
        }
        let singles: Vec<(&GeoCell, MinMax)> = paired.iter().filter(|(c, _)| c.span == 1.0).copied().collect();
        fit_pct_mins(&mut p, &singles, &g1, &|k| uo_jsnum::conv::js_index(k, is_pct.len()).is_some_and(pct_at));
    }
    let np = |j: usize| !pct_at(j);
    if !is_pct.iter().any(|x| !*x) {
        let sp = sum(&p);
        return Ok(if sp > 0.0 && t > sp + 0.5 { p.iter().map(|x| (x * t) / sp).collect() } else { p });
    }
    let mut r = t - sum(&p);
    let g3 = g3_of(t)?;
    let g4 = g4_of(t)?;
    let s1 = sum_of(&g1, &np);
    let s3 = sum_of(&g3, &np);
    let s4 = sum_of(&g4, &np);
    let lerp = |a: &[f64], b: &[f64], tt: f64| -> Vec<f64> {
        a.iter().enumerate().map(|(j, x)| if pct_at(j) { at(&p, j as f64) } else { x + (at(b, j as f64) - x) * tt }).collect()
    };
    if r >= s4 - 0.5 {
        // room left: the automatic columns grow (in proportion to their content); without them,
        // the dxa ones — a column no cell covers (gridBefore/gridAfter without a width) keeps 0
        let covers = |c: &GeoCell, j: f64| c.col <= j && c.col + c.span > j;
        let covered = |j: usize| cells.iter().any(|c| covers(c, j as f64));
        let auto: Vec<bool> = (0..n_cols)
            .map(|j| np(j) && covered(j) && !cells.iter().any(|c| covers(c, j as f64) && kind(c, t, &is_pct).0 != Pref::Auto))
            .collect();
        let mut grow = auto;
        if !grow.iter().any(|x| *x) {
            grow = is_pct.iter().enumerate().map(|(j, x)| !*x && covered(j)).collect();
        }
        let wsum = sum_of(&g4, &|j| grow.get(j).copied().unwrap_or(false));
        let n = grow.iter().filter(|x| **x).count() as f64;
        return Ok(g4
            .iter()
            .enumerate()
            .map(|(j, x)| {
                if pct_at(j) {
                    at(&p, j as f64)
                } else if grow.get(j).copied().unwrap_or(false) {
                    x + ((r - s4) * (if wsum > 0.0 { x / wsum } else { 1.0 / n }))
                } else {
                    *x
                }
            })
            .collect());
    }
    if r >= s3 {
        return Ok(lerp(&g3, &g4, (r - s3) / (s4 - s3)));
    }
    if r >= s1 {
        return Ok(lerp(&g1, &g3, if s3 > s1 { (r - s1) / (s3 - s1) } else { 0.0 }));
    }
    // noWrap floors (tcW dxa) that do not fit: the table passes its tblW up to the available
    // width (if the tblW is already past it, it stays at the sum of the floors); beyond that the
    // floors shrink in proportion to the text area, width − margins (oracle nowrap, nw-piso)
    if cells.iter().any(|c| c.floor) && !is_pct.iter().any(|x| *x) && !(explicit && t > i.avail) && s1 > i.avail + 0.5 {
        let pad: Vec<f64> = (0..n_cols)
            .map(|j| {
                let mut v = vec![0.0];
                v.extend(cells.iter().filter(|c| c.span == 1.0 && c.col == j as f64).map(|c| c.pad.unwrap_or(0.0)));
                jm::max_n(&v)
            })
            .collect();
        let inner: Vec<f64> = g1.iter().enumerate().map(|(j, x)| jm::max(0.0, x - at(&pad, j as f64))).collect();
        let si = sum(&inner);
        let cut = s1 - i.avail;
        return Ok(g1.iter().enumerate().map(|(j, x)| x - (if si > 0.0 { (cut * at(&inner, j as f64)) / si } else { cut / nc })).collect());
    }
    // not even the minimums fit: with a tblW the table grows (f6) — but when they pass the
    // available width and the request is pct up to 100% or twips below it, the table goes to the
    // available width and squeezes like an automatic one; twips ≥ available, it grows (oracle
    // tblsqueeze); automatic, it squeezes (d9) — if the stored grid already has that width, it is
    // the squeeze Word itself computed
    if explicit {
        let squeeze = s1 > i.avail + 0.5 && (if tw == Pref::Pct { t <= i.avail + 0.5 } else { t < i.avail - 0.5 });
        if !squeeze {
            return Ok(g3.iter().enumerate().map(|(j, x)| if pct_at(j) { at(&p, j as f64) } else { *x }).collect());
        }
        r += i.avail - t;
        t = i.avail;
    }
    let sg = sum(doc_grid);
    if !is_pct.iter().any(|x| *x) && jm::abs(sg - t) <= jm::max(20.0, 0.005 * t) {
        return Ok(doc_grid.iter().map(|x| (x * t) / sg).collect());
    }
    Ok(g1
        .iter()
        .enumerate()
        .map(|(j, x)| {
            if pct_at(j) {
                at(&p, j as f64)
            } else if s1 > 0.0 {
                (x * r) / s1
            } else {
                *x
            }
        })
        .collect())
}

/// `paraContentWidths(p, defaultTab)` (table.ts:576): the longest unbreakable stretch and the
/// longest unbroken line of a prepared paragraph, indents included, tabs at their stops.
pub(crate) fn para_content_widths(p: &PreparedPara, default_tab: f64) -> MinMax {
    let mut tabs: Vec<f64> = p.tabs.iter().filter(|t| t.val != TabAlign::Bar).map(|t| t.pos).collect();
    js_sort(&mut tabs, |a, b| a - b);
    let dts = if default_tab > 0.0 { default_tab } else { 720.0 };
    let next_tab = |x: f64| -> f64 {
        let t = tabs.iter().copied().find(|pos| *pos > x + 0.5);
        if p.first_line < 0.0 && p.ind_left > x + 0.5 && t.is_none_or(|t| p.ind_left < t) {
            return p.ind_left;
        }
        if let Some(t) = t {
            return t;
        }
        let last = tabs.last().copied().unwrap_or(x);
        (jm::floor(jm::max(x, last) / dts + 1e-6) + 1.0) * dts
    };
    let mut x = p.ind_left + p.first_line;
    if let Some(mk) = &p.marker {
        x += mk.width;
        match mk.suff {
            MarkerSuffix::Tab => x = next_tab(x),
            MarkerSuffix::Space => x += p.styles.get(p.mark_style.0 as usize).map_or(f64::NAN, |s| s.size_pt) * 5.0,
            MarkerSuffix::Nothing => {}
        }
    }
    let mut end = x;
    let mut max = 0.0;
    let mut min = 0.0;
    let mut seg = 0.0;
    let mut first = true;
    // the first word starts at the first-line indent (with a hanging indent, before the left
    // one) — with a list number, after it and its tab: no break between the number and the first
    // word (oracle tblnum)
    let x0 = x;
    let flush = |seg: &mut f64, min: &mut f64, first: &mut bool| {
        if *seg > 0.0 {
            *min = jm::max(*min, *seg + jm::max(0.0, if *first { x0 } else { p.ind_left }) + p.ind_right);
        }
        if *seg > 0.0 {
            *first = false;
        }
        *seg = 0.0;
    };
    for g in &p.glyphs {
        match g.kind {
            GlyphKind::Pb | GlyphKind::Cb => continue,
            GlyphKind::Lb => {
                flush(&mut seg, &mut min, &mut first);
                max = jm::max(max, end + p.ind_right);
                x = p.ind_left;
                end = x;
                continue;
            }
            GlyphKind::Tab => {
                flush(&mut seg, &mut min, &mut first);
                x = next_tab(x);
                continue;
            }
            _ => {}
        }
        x += g.adv;
        if g.kind == GlyphKind::Space {
            flush(&mut seg, &mut min, &mut first);
            continue;
        }
        end = x;
        seg += g.adv;
        if g.flags.has(GlyphFlags::BREAK_AFTER) {
            flush(&mut seg, &mut min, &mut first);
        }
    }
    flush(&mut seg, &mut min, &mut first);
    max = jm::max(max, end + p.ind_right);
    MinMax { min, max }
}
