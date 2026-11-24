//! `ets.ts`: FORECAST.ETS, FORECAST.ETS.CONFINT, FORECAST.ETS.SEASONALITY, FORECAST.ETS.STAT — additive triple
//! exponential smoothing (Holt-Winters AAA) over a regular timeline, as the TS builds it (step = the most frequent
//! difference, gaps completed or zeroed, duplicates aggregated, seasonality by autocorrelation, α/β/γ by a grid
//! search).
//!
//! Bounded where the TS is not (decision A4b-5): the series length that would make `new Array(n)` throw throws the
//! same `RangeError` before any allocation, and a series with too many gaps answers `#NUM!` before it is built;
//! the seasonality detection (quadratic in n), the grid search and CONFINT's horizon loop stop with a `RangeError`
//! past [`BUDGET`] steps — the TS runs for minutes (or never returns: `CONFINT` with a target date of 1e21).

use uo_jsnum::math::{abs, ceil, floor, max, max_n, min, min_n, pow, rem, round, sqrt, trunc};

use super::{absent, an, nf, sn};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::statw::zip_cells;
use crate::functions::shared::{EMPTY, R, Stop, arg, bodies, err, num, scalar};
use crate::special::qnorm;
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `ets.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "FORECAST.ETS" => forecast_ets,
    "FORECAST.ETS.CONFINT" => forecast_ets_confint,
    "FORECAST.ETS.SEASONALITY" => forecast_ets_seasonality,
    "FORECAST.ETS.STAT" => forecast_ets_stat,
];

/// The most steps of one quadratic or grid loop (multiply-adds): past it a `RangeError` (`#NUM!` in the engine).
const BUDGET: f64 = 2_147_483_648.0;

/// The largest JS array length (`new Array(n)` throws past it).
const MAX_JS_ARRAY: f64 = 4_294_967_295.0;

fn range() -> Stop {
    Stop::Throw(Throw::Range)
}

/// A regular series (`Series`).
struct Series {
    y: Vec<f64>,
    t0: f64,
    step: f64,
}

/// `AGG[aggregation](xs)` (1 mean, 2/3 count, 4 max, 5 median, 6 min, 7 sum).
fn agg(aggregation: f64, xs: &[f64]) -> f64 {
    let sum = || xs.iter().fold(0.0, |a, &b| a + b);
    match crate::cast::trunc_usize(aggregation) {
        1 => sum() / nf(xs.len()),
        2 | 3 => nf(xs.len()),
        4 => max_n(xs),
        5 => {
            let mut s = xs.to_vec();
            uo_jsnum::sort::js_sort_by(&mut s, |a, b| a - b);
            let n = s.len();
            if n % 2 == 1 { s[(n - 1) / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 }
        }
        6 => min_n(xs),
        _ => sum(),
    }
}

/// `buildSeries(values, timeline, completion, aggregation, ctx)`.
fn build_series(values_v: &Value, timeline_v: &Value, completion: f64, aggregation: f64, ctx: &mut dyn FnContext) -> R<Series> {
    // the points (t, v) in the walk's order; `Map` keys read `-0` as `+0`
    let mut pts: Vec<(f64, f64)> = Vec::new();
    zip_cells(timeline_v, values_v, ctx, &mut |_, t, v| {
        if let Scalar::Err(e) = t {
            return Err(Stop::Err(e));
        }
        if let Scalar::Err(e) = v {
            return Err(Stop::Err(e));
        }
        if let (Scalar::Num(t), Scalar::Num(v)) = (t, v) {
            pts.push((if t == 0.0 { 0.0 } else { t }, v));
        }
        Ok(())
    })?;
    // the distinct keys ascending (`[...pts.keys()].sort((a, b) => a − b)`), each key's values in walk order: a
    // stable sort of the points by key
    uo_jsnum::sort::js_sort_by(&mut pts, |a, b| a.0 - b.0);
    let mut keys: Vec<f64> = Vec::new();
    for &(t, _) in &pts {
        if keys.last() != Some(&t) {
            keys.push(t);
        }
    }
    if keys.len() < 3 {
        return Err(Stop::Err(ErrorCode::Num));
    }
    // step = the most frequent difference (first seen wins; a tie takes the smaller)
    let mut freq: Vec<(f64, usize)> = Vec::new();
    let mut at: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for w in keys.windows(2) {
        let d = w[1] - w[0];
        let k = if d == 0.0 { 0 } else { uo_jsnum::conv::canonicalize_nan(d).to_bits() };
        match at.get(&k) {
            Some(&i) => freq[i].1 += 1,
            None => {
                at.insert(k, freq.len());
                freq.push((d, 1));
            }
        }
    }
    let (mut step, mut best_f) = (0.0, 0usize);
    for &(d, f) in &freq {
        if f > best_f || (f == best_f && d < step) {
            best_f = f;
            step = d;
        }
    }
    if step.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(Stop::Err(ErrorCode::Num));
    }
    let t0 = keys[0];
    let n = round((keys[keys.len() - 1] - t0) / step) + 1.0;
    // `new Array(n)`: past the JS limit a RangeError
    if !matches!(n.partial_cmp(&MAX_JS_ARRAY), Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)) {
        return Err(range());
    }
    // the indices filled (ascending: the keys are), and the gaps decided before anything of size n is built
    let mut distinct = 0usize;
    let mut last_idx = f64::NAN;
    for &k in &keys {
        let idx = round((k - t0) / step);
        if idx != last_idx {
            distinct += 1;
            last_idx = idx;
        }
    }
    let missing = n - nf(distinct);
    if missing > 0.3 * n {
        return Err(Stop::Err(ErrorCode::Num));
    }
    let nu = crate::cast::trunc_usize(n);
    let mut y: Vec<f64> = vec![0.0; nu];
    let mut set: Vec<bool> = vec![false; nu];
    // each index's values: the keys' lists concatenated in key order (the indices ascend with the keys)
    let mut list: Vec<f64> = Vec::new();
    let mut g = 0;
    while g < pts.len() {
        let idx = round((pts[g].0 - t0) / step);
        list.clear();
        list.push(pts[g].1);
        let mut j = g + 1;
        while j < pts.len() && round((pts[j].0 - t0) / step) == idx {
            list.push(pts[j].1);
            j += 1;
        }
        if let Some(at) = uo_jsnum::conv::js_index(idx, nu) {
            y[at] = agg(aggregation, &list);
            set[at] = true;
        }
        g = j;
    }
    // the gaps: zero, or interpolated from the previous value (already completed) and the next known one
    let mut next = vec![nu; nu];
    let mut nx = nu;
    for j in (0..nu).rev() {
        next[j] = nx;
        if set[j] {
            nx = j;
        }
    }
    for i in 0..nu {
        if set[i] {
            continue;
        }
        if completion == 0.0 {
            y[i] = 0.0;
            continue;
        }
        // `a` = i − 1: every index before i holds a value by now (index 0 is the first key)
        let b = next[i];
        let has_a = i > 0;
        let ya = if has_a { y[i - 1] } else { y.get(b).copied().unwrap_or(f64::NAN) };
        let yb = if b < nu { y[b] } else { ya };
        y[i] = if has_a && b < nu { ya + (yb - ya) * (nf(i) - nf(i - 1)) / (nf(b) - nf(i - 1)) } else { ya };
    }
    Ok(Series { y, t0, step })
}

/// `detectSeason(y)`: the lag with the highest autocorrelation of the detrended series above 0.5 (0 = none).
fn detect_season(y: &[f64]) -> R<f64> {
    let n = y.len();
    let nn = nf(n);
    let half = n / 2;
    // Σ_{m=2}^{n/2} (n − m) multiply-adds
    if half >= 2 && (nf(half) - 1.0) * (nn - (nf(half) + 2.0) / 2.0) > BUDGET {
        return Err(range());
    }
    let tm = (nn - 1.0) / 2.0;
    let ym = y.iter().fold(0.0, |a, &b| a + b) / nn;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, &v) in y.iter().enumerate() {
        sxy += (nf(i) - tm) * (v - ym);
        sxx += pow(nf(i) - tm, 2.0);
    }
    let b = if uo_jsnum::conv::truthy(sxx) { sxy / sxx } else { 0.0 };
    let r: Vec<f64> = y.iter().enumerate().map(|(i, &v)| v - ym - b * (nf(i) - tm)).collect();
    let var0 = r.iter().fold(0.0, |s, &v| s + v * v);
    if var0 <= 1e-12 * (ym * ym * nn + 1e-300) {
        return Ok(0.0);
    }
    let (mut best, mut best_acf) = (0usize, 0.5);
    for m in 2..=half {
        let mut c = 0.0;
        for i in m..n {
            c += r[i] * r[i - m];
        }
        let acf = c / var0;
        if acf > best_acf + 1e-12 {
            best_acf = acf;
            best = m;
        }
    }
    Ok(nf(best))
}

/// One run of Holt-Winters (`run`): the sum of squared one-step errors and the final states; `trace` gets each
/// (fitted, error). `s` is the seasonal buffer (reused).
struct Run {
    sse: f64,
    l: f64,
    b: f64,
}

fn run(y: &[f64], m: usize, alpha: f64, beta: f64, gamma: f64, s: &mut Vec<f64>, mut trace: impl FnMut(f64, f64)) -> Run {
    let n = y.len();
    let (mut l, mut b);
    s.clear();
    if m >= 2 && n >= 2 * m {
        let mean = |a: usize, c: usize| -> f64 {
            let mut t = 0.0;
            for &v in &y[a..c] {
                t += v;
            }
            t / (nf(c) - nf(a))
        };
        let (m1, m2) = (mean(0, m), mean(m, 2 * m));
        let mf = nf(m);
        b = (m2 - m1) / mf;
        l = m1 - b * (mf - 1.0) / 2.0;
        for (i, &v) in y.iter().take(m).enumerate() {
            s.push(v - (l + b * nf(i)));
        }
        l -= b;
    } else {
        l = y[0];
        b = if n > 1 { y[1] - y[0] } else { 0.0 };
        l -= b;
    }
    let seasonal = m >= 2 && !s.is_empty();
    let mut sse = 0.0;
    for (i, &yi) in y.iter().enumerate() {
        let si = if seasonal { s[i % m] } else { 0.0 };
        let f = l + b + si;
        let e = yi - f;
        trace(f, e);
        sse += e * e;
        let ln = l + b + alpha * e;
        b += alpha * beta * e;
        if seasonal {
            s[i % m] = si + gamma * e;
        }
        l = ln;
    }
    Run { sse, l, b }
}

/// The fitted model (`Fit`).
struct Model {
    alpha: f64,
    beta: f64,
    gamma: f64,
    sse: f64,
    errs: Vec<f64>,
    fitted: Vec<f64>,
    l: f64,
    b: f64,
    /// The seasonal states in the order of the next index.
    s: Vec<f64>,
}

/// `grid(c, w, k)`: `c + i·w` clamped to `[0.001, 0.999]`, duplicates removed (first kept).
fn grid(c: f64, w: f64, out: &mut Vec<f64>) {
    out.clear();
    for i in -5i32..=5 {
        let v = min(0.999, max(0.001, c + f64::from(i) * w));
        if !out.iter().any(|&x| x == v || (x.is_nan() && v.is_nan())) {
            out.push(v);
        }
    }
}

/// `fit(y, m)`: α, β, γ minimizing the squared errors (a grid of 11 values per parameter around the best, 4 rounds,
/// the step divided by 10). The search keeps only the parameters and the sum (A4b-4); the model of the winner is run
/// once more with its trace (`run` is deterministic: the same numbers).
fn fit(y: &[f64], m: usize) -> R<Model> {
    let seasonal = m >= 2;
    let runs = if seasonal { 4.0 * 1331.0 + 1.0 } else { 4.0 * 121.0 + 1.0 };
    if runs * nf(y.len()) > BUDGET {
        return Err(range());
    }
    let mut s = Vec::with_capacity(m);
    let g0 = if seasonal { 0.1 } else { 0.0 };
    let mut best = (0.5, 0.1, g0, run(y, m, 0.5, 0.1, g0, &mut s, |_, _| {}).sse);
    let (mut as_, mut bs, mut gs) = (Vec::new(), Vec::new(), Vec::new());
    let mut w = 0.1;
    for _ in 0..4 {
        grid(best.0, w, &mut as_);
        grid(best.1, w, &mut bs);
        if seasonal {
            grid(best.2, w, &mut gs);
        } else {
            gs.clear();
            gs.push(0.0);
        }
        for &a in &as_ {
            for &b in &bs {
                for &g in &gs {
                    let sse = run(y, m, a, b, g, &mut s, |_, _| {}).sse;
                    if sse < best.3 - 1e-12 * best.3 {
                        best = (a, b, g, sse);
                    }
                }
            }
        }
        w /= 10.0;
    }
    let (alpha, beta, gamma, _) = best;
    let (mut errs, mut fitted) = (Vec::with_capacity(y.len()), Vec::with_capacity(y.len()));
    let r = run(y, m, alpha, beta, gamma, &mut s, |f, e| {
        fitted.push(f);
        errs.push(e);
    });
    let n = y.len();
    let s_out: Vec<f64> = if s.is_empty() { Vec::new() } else { (0..s.len()).map(|k| s[(n + k) % m]).collect() };
    Ok(Model { alpha, beta, gamma, sse: r.sse, errs, fitted, l: r.l, b: r.b, s: s_out })
}

/// `Prepared`.
struct Prepared {
    series: Series,
    m: f64,
    model: Model,
}

/// `prepare(...)` with the arguments already placed: values, timeline, seasonality, data completion, aggregation.
fn prepare(
    values_v: &Value,
    timeline_v: &Value,
    season: Option<Scalar>,
    completion: Option<Scalar>,
    aggregation: Option<Scalar>,
    ctx: &mut dyn FnContext,
) -> R<Prepared> {
    let opt = |v: Option<Scalar>, d: f64| -> Result<f64, ErrorCode> { if absent(v) { Ok(d) } else { an(ctx, v).map(trunc) } };
    let season = opt(season, 1.0)?;
    let completion = opt(completion, 1.0)?;
    let aggregation = opt(aggregation, 1.0)?;
    if !(0.0..=8760.0).contains(&season) || (completion != 0.0 && completion != 1.0) || !(1.0..=7.0).contains(&aggregation) {
        return Err(Stop::Err(ErrorCode::Num));
    }
    let series = build_series(values_v, timeline_v, completion, aggregation, ctx)?;
    let m = if season == 1.0 { detect_season(&series.y)? } else { season };
    if m >= nf(series.y.len()) {
        return Err(Stop::Err(ErrorCode::Num));
    }
    let model = fit(&series.y, crate::cast::trunc_usize(m))?;
    Ok(Prepared { series, m, model })
}

/// `forecastAt(p, h)`: h steps ahead (a fraction interpolates).
fn forecast_at(p: &Prepared, h: f64) -> f64 {
    let at = |k: f64| -> f64 {
        let md = &p.model;
        let si = if md.s.is_empty() { 0.0 } else { uo_jsnum::conv::js_index(rem(k - 1.0, p.m), md.s.len()).map_or(f64::NAN, |i| md.s[i]) };
        md.l + k * md.b + si
    };
    let k = floor(h);
    if k == h || k < 1.0 {
        return at(max(1.0, ceil(h)));
    }
    at(k) + (at(k + 1.0) - at(k)) * (h - k)
}

/// `stepsAhead(p, target)`.
fn steps_ahead(p: &Prepared, target: f64) -> f64 {
    let last = p.series.t0 + (nf(p.series.y.len()) - 1.0) * p.series.step;
    (target - last) / p.series.step
}

/// `valueAt(p, h)`: ahead the forecast; inside the history the one-step fit.
fn value_at(p: &Prepared, h: f64) -> Result<f64, ErrorCode> {
    if h > 0.0 {
        return Ok(forecast_at(p, h));
    }
    let idx = round(nf(p.series.y.len()) - 1.0 + h);
    if idx < 0.0 {
        return Err(ErrorCode::Num);
    }
    Ok(uo_jsnum::conv::js_index(idx, p.model.fitted.len()).map_or(f64::NAN, |i| p.model.fitted[i]))
}

fn forecast_ets(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = an(ctx, scalar(a, 0))?;
    let p = prepare(arg(a, 1).unwrap_or(&EMPTY), arg(a, 2).unwrap_or(&EMPTY), scalar(a, 3), scalar(a, 4), scalar(a, 5), ctx)?;
    Ok(match value_at(&p, steps_ahead(&p, t)) {
        Ok(v) => sn(v),
        Err(e) => err(e),
    })
}

fn forecast_ets_confint(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = an(ctx, scalar(a, 0))?;
    let mut conf = 0.95;
    let cr = scalar(a, 3);
    if !absent(cr) {
        let c = an(ctx, cr)?;
        if c <= 0.0 || c >= 1.0 {
            return Ok(err(ErrorCode::Num));
        }
        conf = c;
    }
    let p = prepare(arg(a, 1).unwrap_or(&EMPTY), arg(a, 2).unwrap_or(&EMPTY), scalar(a, 4), scalar(a, 5), scalar(a, 6), ctx)?;
    let h = max(1.0, steps_ahead(&p, t));
    let n = nf(p.series.y.len());
    let sigma2 = p.model.sse / n;
    let (alpha, beta, gamma) = (p.model.alpha, p.model.beta, p.model.gamma);
    let end = ceil(h);
    // the horizon loop runs ⌈h⌉ − 1 times: a target date of 1e21 never returns in the TS (decision A4b-5)
    if end - 1.0 > BUDGET {
        return Err(range());
    }
    let mut v = 1.0;
    let mut j = 1.0;
    while j < end {
        let c = alpha * (1.0 + j * beta) + if p.m >= 2.0 && rem(j, p.m) == 0.0 { gamma } else { 0.0 };
        v += c * c;
        j += 1.0;
    }
    Ok(sn(qnorm(0.5 + conf / 2.0) * sqrt(sigma2 * v)))
}

fn forecast_ets_seasonality(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let p = prepare(arg(a, 0).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY), None, scalar(a, 2), scalar(a, 3), ctx)?;
    Ok(num(p.m))
}

fn forecast_ets_stat(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let st = an(ctx, scalar(a, 2))?;
    let k = trunc(st);
    if !(1.0..=8.0).contains(&k) {
        return Ok(err(ErrorCode::Num));
    }
    let p = prepare(arg(a, 0).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY), scalar(a, 3), scalar(a, 4), scalar(a, 5), ctx)?;
    let (errs, fitted) = (&p.model.errs, &p.model.fitted);
    let y = &p.series.y;
    let n = nf(y.len());
    let abs_sum = || errs.iter().fold(0.0, |s, &e| s + abs(e));
    Ok(match crate::cast::trunc_usize(k) {
        1 => num(p.model.alpha),
        2 => num(p.model.beta),
        3 => num(if p.m >= 2.0 { p.model.gamma } else { f64::EPSILON }),
        4 => {
            let m = if p.m >= 2.0 { crate::cast::trunc_usize(p.m) } else { 1 };
            let mut d = 0.0;
            for i in m..y.len() {
                d += abs(y[i] - y[i - m]);
            }
            d /= n - nf(m);
            let mae = abs_sum() / n;
            if uo_jsnum::conv::truthy(d) { sn(mae / d) } else { num(0.0) }
        }
        5 => {
            let mut s = 0.0;
            for (i, &e) in errs.iter().enumerate() {
                let den = (abs(y[i]) + abs(fitted[i])) / 2.0;
                s += if uo_jsnum::conv::truthy(den) { abs(e) / den } else { 0.0 };
            }
            sn(s / n)
        }
        6 => sn(abs_sum() / n),
        7 => sn(sqrt(errs.iter().fold(0.0, |s, &e| s + e * e) / n)),
        _ => num(p.series.step),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quadratic seasonality detection and the grid search stop before their loops past the budget.
    #[test]
    fn budgets() {
        let y = vec![1.0; 120_000];
        assert!(matches!(detect_season(&y), Err(Stop::Throw(Throw::Range))));
        let y = vec![1.0; 500_000];
        assert!(matches!(fit(&y, 12), Err(Stop::Throw(Throw::Range))));
        // under the budget: computed
        let y: Vec<f64> = (0..48).map(|i| f64::from(i % 12) + f64::from(i) * 0.5).collect();
        assert_eq!(detect_season(&y).ok(), Some(12.0));
    }
}
