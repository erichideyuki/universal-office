//! `regression.ts`: the pair functions (CORREL, PEARSON, RSQ, SLOPE, INTERCEPT, STEYX, COVAR, COVARIANCE.P/.S,
//! FORECAST/FORECAST.LINEAR) and the least squares of LINEST/LOGEST/TREND/GROWTH (Householder with column pivoting
//! over the centred data, as the TS). The work of one fit is bounded (decision A4b-6).

use uo_jsnum::math::{exp, log, max, sqrt};

use super::{absent, an, nf, pairs, sn, xadd};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::statw::grid_of;
use crate::functions::shared::{EMPTY, R, Stop, arg, bodies, err, num_or_err, scalar};
use crate::stats::{BIG_AREA, area_size, stat_num};
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

/// The bodies of `regression.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "CORREL" => correl,
    "PEARSON" => correl,
    "RSQ" => rsq,
    "SLOPE" => slope,
    "INTERCEPT" => intercept,
    "STEYX" => steyx,
    "COVAR" => covar,
    "COVARIANCE.P" => covar,
    "COVARIANCE.S" => covariance_s,
    "FORECAST" => forecast,
    "FORECAST.LINEAR" => forecast,
    "LINEST" => linest,
    "LOGEST" => logest,
    "TREND" => trend,
    "GROWTH" => growth,
];

/// `Moments`: means and the two-pass sums of squares and products (Excel's sum).
struct Moments {
    n: f64,
    mx: f64,
    my: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

fn moments(xs: &[f64], ys: &[f64]) -> Moments {
    let n = nf(xs.len());
    let (mut sx, mut sy) = (0.0, 0.0);
    for (&x, &y) in xs.iter().zip(ys) {
        sx = xadd(sx, x);
        sy = xadd(sy, y);
    }
    let (mx, my) = (sx / n, sy / n);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (&x, &y) in xs.iter().zip(ys) {
        let (dx, dy) = (x - mx, y - my);
        sxx = xadd(sxx, dx * dx);
        syy = xadd(syy, dy * dy);
        sxy = xadd(sxy, dx * dy);
    }
    Moments { n, mx, my, sxx, syy, sxy }
}

/// `pairFn`'s call: the numeric pairs (x from the second argument when `y_first`), at least one, then `f`.
fn pair_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, y_first: bool, f: fn(&Moments) -> Result<f64, ErrorCode>) -> R<Value> {
    let (va, vb) = (arg(a, 0).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY));
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    if y_first {
        pairs(vb, va, ctx, &mut xs, &mut ys)?;
    } else {
        pairs(va, vb, ctx, &mut xs, &mut ys)?;
    }
    if xs.is_empty() {
        return Ok(err(ErrorCode::Div0));
    }
    Ok(num_or_err(f(&moments(&xs, &ys))))
}

/// CORREL/PEARSON: cov.P / (σx·σy) with the roots taken apart.
fn correl(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, false, |m| {
        if m.sxx == 0.0 || m.syy == 0.0 {
            return Err(ErrorCode::Div0);
        }
        stat_num((m.sxy / m.n) / (sqrt(m.sxx / m.n) * sqrt(m.syy / m.n)))
    })
}

fn rsq(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, true, |m| {
        if m.sxx == 0.0 || m.syy == 0.0 {
            return Err(ErrorCode::Div0);
        }
        let d = m.n - 1.0;
        let r = (m.sxy / d) / (sqrt(m.sxx / d) * sqrt(m.syy / d));
        stat_num(r * r)
    })
}

fn slope(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, true, |m| if m.sxx == 0.0 { Err(ErrorCode::Div0) } else { stat_num(m.sxy / m.sxx) })
}

fn intercept(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, true, |m| if m.sxx == 0.0 { Err(ErrorCode::Div0) } else { stat_num(m.my - m.sxy / m.sxx * m.mx) })
}

fn steyx(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, true, |m| {
        if m.n < 3.0 || m.sxx == 0.0 {
            return Err(ErrorCode::Div0);
        }
        stat_num(sqrt((m.syy - m.sxy * m.sxy / m.sxx) / (m.n - 2.0)))
    })
}

fn covar(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, false, |m| stat_num(m.sxy / m.n))
}

fn covariance_s(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_fn(a, ctx, false, |m| if m.n < 2.0 { Err(ErrorCode::Div0) } else { stat_num(m.sxy / (m.n - 1.0)) })
}

fn forecast(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x0 = an(ctx, scalar(a, 0))?;
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    pairs(arg(a, 2).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY), ctx, &mut xs, &mut ys)?;
    if xs.is_empty() {
        return Ok(err(ErrorCode::Div0));
    }
    let m = moments(&xs, &ys);
    if m.sxx == 0.0 {
        return Ok(err(ErrorCode::Div0));
    }
    let b = m.sxy / m.sxx;
    Ok(sn(m.my - b * m.mx + b * x0))
}

// ---------------------------------------------------------------- LINEST / LOGEST / TREND / GROWTH

/// A numeric matrix (`numericGrid`).
struct Grid {
    rows: u32,
    cols: u32,
    data: Vec<f64>,
}

/// `numericGrid(v, ctx)`: every value a number (text, empty, boolean and errors → `#VALUE!`); a whole column has
/// empty cells unless every cell exists.
fn numeric_grid(v: &Value, ctx: &mut dyn FnContext) -> R<Grid> {
    if let Value::Ref(r) = v
        && let Some(x) = r.single()
    {
        let size = area_size(&x);
        if size > BIG_AREA {
            let mut k = 0u64;
            ctx.for_each_cell(x, &mut |s, _, _| {
                if s != Scalar::Empty {
                    k += 1;
                }
            })?;
            if k < size {
                return Err(Stop::Err(ErrorCode::Value));
            }
        }
    }
    let g = grid_of(v, ctx)?;
    let mut data = Vec::with_capacity(g.data.len());
    for &x in g.data.iter() {
        let Scalar::Num(x) = x else { return Err(Stop::Err(ErrorCode::Value)) };
        data.push(x);
    }
    if data.is_empty() {
        return Err(Stop::Err(ErrorCode::Value));
    }
    Ok(Grid { rows: g.rows, cols: g.cols, data })
}

/// The result of `leastSquares` the callers read.
struct Fit {
    coef: Vec<f64>,
    b: f64,
    se: Vec<f64>,
    se_b: f64,
    r2: f64,
    sey: f64,
    f: f64,
    df: f64,
    ssreg: f64,
    ssresid: f64,
}

/// The most work one least-squares fit may take (multiply-adds, `k·p·n + p³`); past it a `RangeError` (`#NUM!` in
/// the engine), decision A4b-6. The TS allocates `k × k` and runs for minutes on such shapes.
const FIT_BUDGET: f64 = 4_294_967_296.0;

/// `leastSquares(y, X, withConst)`: Householder QR with column pivoting (largest residual norm first; a tie keeps
/// the rightmost column); a column whose residual norm vanishes is collinear (β = 0).
#[allow(clippy::too_many_lines, clippy::needless_range_loop)]
fn least_squares(y: &[f64], xcols: &[Vec<f64>], with_const: bool) -> R<Fit> {
    let (n, k) = (y.len(), xcols.len());
    let pm = k.min(n);
    if nf(k) * nf(k) > 16_777_216.0 || nf(k) * nf(pm) * nf(n) + nf(pm) * nf(pm) * nf(pm) > FIT_BUDGET {
        return Err(Stop::Throw(Throw::Range));
    }
    let nn = nf(n);
    let means: Vec<f64> = xcols
        .iter()
        .map(|c| {
            if with_const {
                let mut s = 0.0;
                for &v in c {
                    s += v;
                }
                s / nn
            } else {
                0.0
            }
        })
        .collect();
    let ym = if with_const {
        let mut s = 0.0;
        for &v in y {
            s += v;
        }
        s / nn
    } else {
        0.0
    };
    let mut aa: Vec<Vec<f64>> = xcols.iter().zip(&means).map(|(c, &m)| c.iter().map(|&v| v - m).collect()).collect();
    let mut r: Vec<f64> = y.iter().map(|&v| v - ym).collect();
    let mut rm: Vec<Vec<f64>> = vec![vec![0.0; k]; k];
    let mut order: Vec<usize> = Vec::with_capacity(pm);
    let orig_norm: Vec<f64> = aa
        .iter()
        .map(|c| {
            let mut t = 0.0;
            for &v in c {
                t += v * v;
            }
            sqrt(t)
        })
        .collect();
    let mut left = vec![true; k];
    let mut n_left = k;
    let mut v = vec![0.0; n];
    let mut row = 0;
    while n_left > 0 && row < n {
        let mut best: Option<usize> = None;
        let mut best_norm = -1.0;
        for j in 0..k {
            if !left[j] {
                continue;
            }
            let mut t = 0.0;
            for i in row..n {
                t += aa[j][i] * aa[j][i];
            }
            if t > best_norm * (1.0 + 1e-9) || (best.is_some_and(|b| j > b) && t >= best_norm * (1.0 - 1e-9)) {
                best_norm = max(t, best_norm);
                best = Some(j);
            }
        }
        let norm = sqrt(best_norm);
        let Some(best) = best else { break };
        if norm.partial_cmp(&(1e-12 * orig_norm[best])) != Some(core::cmp::Ordering::Greater) || norm == 0.0 {
            break;
        }
        left[best] = false;
        n_left -= 1;
        let alpha = if aa[best][row] > 0.0 { -norm } else { norm };
        v[row..n].copy_from_slice(&aa[best][row..n]);
        v[row] -= alpha;
        let mut vv = 0.0;
        for i in row..n {
            vv += v[i] * v[i];
        }
        let apply = |c: &mut [f64]| {
            if vv == 0.0 {
                return;
            }
            let mut d = 0.0;
            for i in row..n {
                d += v[i] * c[i];
            }
            let f = 2.0 * d / vv;
            for i in row..n {
                c[i] -= f * v[i];
            }
        };
        apply(&mut aa[best]);
        for j in 0..k {
            if left[j] {
                apply(&mut aa[j]);
            }
        }
        apply(&mut r);
        order.push(best);
        let at = order.len() - 1;
        for jj in 0..k {
            rm[at][jj] = aa[jj][row];
        }
        row += 1;
    }
    let p = order.len();
    // R β = Qᵀy (triangular, the kept columns)
    let mut coef = vec![0.0; k];
    for ii in (0..p).rev() {
        let j = order[ii];
        let mut s = r[ii];
        for t in ii + 1..p {
            s -= rm[ii][order[t]] * coef[order[t]];
        }
        coef[j] = s / rm[ii][j];
    }
    let mut sstot = 0.0;
    for &yv in y {
        sstot += (yv - ym) * (yv - ym);
    }
    let mut ssresid = 0.0;
    for i in p..n {
        ssresid += r[i] * r[i];
    }
    if ssresid <= sstot * 1e-30 {
        ssresid = 0.0;
    }
    let ssreg = sstot - ssresid;
    let b = if with_const {
        let mut s = 0.0;
        for (j, &c) in coef.iter().enumerate() {
            s += c * means[j];
        }
        ym - s
    } else {
        0.0
    };
    let pf = nf(p);
    let df = nn - pf - if with_const { 1.0 } else { 0.0 };
    let sey = if df > 0.0 { sqrt(ssresid / df) } else { 0.0 };
    let r2 = if sstot == 0.0 { f64::NAN } else { ssreg / sstot };
    let f = if df > 0.0 && p > 0 && ssresid > 0.0 { (ssreg / pf) / (ssresid / df) } else { f64::NAN };
    // (RᵀR)⁻¹ of the kept columns through the inverse of R
    let mut rinv: Vec<Vec<f64>> = vec![vec![0.0; p]; p];
    for i in (0..p).rev() {
        rinv[i][i] = 1.0 / rm[i][order[i]];
        for j2 in i + 1..p {
            let mut s = 0.0;
            for t in i + 1..=j2 {
                s += rm[i][order[t]] * rinv[t][j2];
            }
            rinv[i][j2] = -s / rm[i][order[i]];
        }
    }
    let cov = |a: usize, c: usize| -> f64 {
        let mut s = 0.0;
        for t in 0..p {
            s += rinv[a][t] * rinv[c][t];
        }
        s
    };
    let mut se = vec![0.0; k];
    for ii in 0..p {
        se[order[ii]] = sey * sqrt(cov(ii, ii));
    }
    let mut se_b = 0.0;
    if with_const {
        let mut q = 1.0 / nn;
        for a in 0..p {
            for c in 0..p {
                q += means[order[a]] * means[order[c]] * cov(a, c);
            }
        }
        se_b = sey * sqrt(q);
    }
    Ok(Fit { coef, b, se, se_b, r2, sey, f, df, ssreg, ssresid })
}

/// `design(y, x, ctx, logY)`: y, the columns of X and the orientation.
struct Design {
    y: Vec<f64>,
    cols: Vec<Vec<f64>>,
    by_row: bool,
}

fn design(y_arg: &Value, x_arg: Option<&Value>, ctx: &mut dyn FnContext, log_y: bool) -> R<Design> {
    let yg = numeric_grid(y_arg, ctx)?;
    let mut y = yg.data;
    if log_y {
        if y.iter().any(|&v| v <= 0.0) {
            return Err(Stop::Err(ErrorCode::Num));
        }
        for v in &mut y {
            *v = log(*v);
        }
    }
    let n = y.len();
    let by_row = yg.rows == 1 && yg.cols > 1;
    let Some(x_arg) = x_arg.filter(|x| !matches!(x, Value::Scalar(Scalar::Empty))) else {
        let cols = vec![(1..=n).map(nf).collect()];
        return Ok(Design { y, cols, by_row });
    };
    let xg = numeric_grid(x_arg, ctx)?;
    let (xr, xc) = (xg.rows as usize, xg.cols as usize);
    let mut cols: Vec<Vec<f64>> = Vec::new();
    if yg.rows == 1 || yg.cols == 1 {
        if xg.data.len() == n {
            cols.push(xg.data);
        } else if !by_row && xr == n {
            for c in 0..xc {
                cols.push((0..n).map(|r| xg.data.get(r * xc + c).copied().unwrap_or(f64::NAN)).collect());
            }
        } else if by_row && xc == n {
            for r in 0..xr {
                cols.push(xg.data[r * xc..(r + 1) * xc].to_vec());
            }
        } else {
            return Err(Stop::Err(ErrorCode::Ref));
        }
    } else {
        if xg.data.len() != n {
            return Err(Stop::Err(ErrorCode::Ref));
        }
        cols.push(xg.data);
    }
    if cols.is_empty() || cols.iter().any(|c| c.len() != n) {
        return Err(Stop::Err(ErrorCode::Ref));
    }
    Ok(Design { y, cols, by_row })
}

/// `constArg(v, ctx)`: omitted = TRUE.
fn const_arg(ctx: &dyn FnContext, v: Option<Scalar>) -> Result<bool, ErrorCode> {
    match v {
        None | Some(Scalar::Empty) => Ok(true),
        Some(Scalar::Bool(b)) => Ok(b),
        v => an(ctx, v).map(|n| n != 0.0),
    }
}

/// `num(v)` of a fit statistic as a scalar.
fn ns(x: f64) -> Scalar {
    match stat_num(x) {
        Ok(v) => Scalar::Num(v),
        Err(e) => Scalar::Err(e),
    }
}

fn linest_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, log_y: bool) -> R<Value> {
    let with_const = match const_arg(ctx, scalar(a, 2)) {
        Ok(b) => b,
        Err(e) => return Ok(err(e)),
    };
    let s3 = scalar(a, 3);
    let stats = if absent(s3) {
        false
    } else {
        match const_arg(ctx, s3) {
            Ok(b) => b,
            Err(e) => return Ok(err(e)),
        }
    };
    let d = design(arg(a, 0).unwrap_or(&EMPTY), arg(a, 1), ctx, log_y)?;
    let fit = least_squares(&d.y, &d.cols, with_const)?;
    let k = d.cols.len();
    let tr = |x: f64| if log_y { exp(x) } else { x };
    let mut first: Vec<Scalar> = Vec::with_capacity(k + 1);
    for j in (0..k).rev() {
        first.push(ns(tr(fit.coef[j])));
    }
    first.push(ns(if with_const {
        tr(fit.b)
    } else if log_y {
        1.0
    } else {
        0.0
    }));
    let w = u32::try_from(k + 1).map_err(|_| Stop::Throw(Throw::Range))?;
    if !stats {
        return Ok(Value::Array(ArrayValue::new(1, w, first)));
    }
    let na = Scalar::Err(ErrorCode::NA);
    let mut se_row: Vec<Scalar> = (0..k).rev().map(|j| ns(fit.se[j])).collect();
    se_row.push(if with_const { ns(fit.se_b) } else { na });
    let pairs = [(fit.r2, fit.sey), (fit.f, fit.df), (fit.ssreg, fit.ssresid)];
    Ok(Value::Array(ArrayValue::fill(5, w, |r, c| {
        let c = c as usize;
        match r {
            0 => first.get(c).copied().unwrap_or(na),
            1 => se_row.get(c).copied().unwrap_or(na),
            _ => {
                let (x, y) = pairs[(r - 2) as usize];
                match c {
                    0 => ns(x),
                    1 => ns(y),
                    _ => na,
                }
            }
        }
    })))
}

fn linest(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    linest_fn(a, ctx, false)
}

fn logest(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    linest_fn(a, ctx, true)
}

/// The shape of `gridOf(v)` (a value [`design`] already read as a grid).
fn grid_dims(v: &Value) -> (u32, u32) {
    match v {
        Value::Ref(r) => r.single().map_or((0, 0), |x| (x.r2 - x.r1 + 1, x.c2 - x.c1 + 1)),
        Value::Array(x) => (x.rows, x.cols),
        _ => (1, 1),
    }
}

fn trend_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, log_y: bool) -> R<Value> {
    let with_const = match const_arg(ctx, scalar(a, 3)) {
        Ok(b) => b,
        Err(e) => return Ok(err(e)),
    };
    let y_arg = arg(a, 0).unwrap_or(&EMPTY);
    let d = design(y_arg, arg(a, 1), ctx, log_y)?;
    let fit = least_squares(&d.y, &d.cols, with_const)?;
    let k = d.cols.len();
    let new_arg = arg(a, 2).filter(|x| !matches!(x, Value::Scalar(Scalar::Empty)));
    let owned: Vec<Vec<f64>>;
    let (new_cols, out_rows, out_cols): (&[Vec<f64>], u32, u32) = match new_arg {
        None => {
            let (r, c) = grid_dims(y_arg);
            (&d.cols, r, c)
        }
        Some(v) => {
            let ng = numeric_grid(v, ctx)?;
            let (nr, nc) = (ng.rows as usize, ng.cols as usize);
            if k == 1 {
                let (r, c) = (ng.rows, ng.cols);
                owned = vec![ng.data];
                (&owned, r, c)
            } else if !d.by_row {
                if nc != k {
                    return Ok(err(ErrorCode::Ref));
                }
                owned = (0..k).map(|c| (0..nr).map(|r| ng.data[r * nc + c]).collect()).collect();
                (&owned, ng.rows, 1)
            } else {
                if nr != k {
                    return Ok(err(ErrorCode::Ref));
                }
                owned = (0..k).map(|r| ng.data[r * nc..(r + 1) * nc].to_vec()).collect();
                (&owned, 1, ng.cols)
            }
        }
    };
    let m = new_cols.first().map_or(0, Vec::len);
    let mut pred: Vec<f64> = Vec::with_capacity(m);
    for i in 0..m {
        let mut s = if with_const { fit.b } else { 0.0 };
        for j in 0..k {
            s += fit.coef[j] * new_cols[j].get(i).copied().unwrap_or(f64::NAN);
        }
        pred.push(if log_y { exp(s) } else { s });
    }
    let oc = out_cols as usize;
    Ok(Value::Array(ArrayValue::fill(out_rows, out_cols, |r, c| ns(pred.get(r as usize * oc + c as usize).copied().unwrap_or(f64::NAN)))))
}

fn trend(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trend_fn(a, ctx, false)
}

fn growth(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trend_fn(a, ctx, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fit past the work budget throws a `RangeError` before its `k × k` matrix exists (decision A4b-6).
    #[test]
    fn budget() {
        let y = vec![1.0; 1300];
        let cols: Vec<Vec<f64>> = (0..1300).map(|j| (0..1300).map(|i| f64::from((i * 7 + j) % 13)).collect()).collect();
        assert!(matches!(least_squares(&y, &cols, true), Err(Stop::Throw(Throw::Range))));
        let cols: Vec<Vec<f64>> = (0..4097).map(|_| vec![1.0]).collect();
        assert!(matches!(least_squares(&[1.0], &cols, true), Err(Stop::Throw(Throw::Range))));
        // y = 2x + 1: exact coefficients
        let fit = least_squares(&[3.0, 5.0, 7.0, 9.0], &[vec![1.0, 2.0, 3.0, 4.0]], true).ok().unwrap();
        assert!((fit.coef[0] - 2.0).abs() < 1e-12 && (fit.b - 1.0).abs() < 1e-12);
    }
}
