//! `distributions.ts`: the probability distributions, their inverses and the hypothesis tests, with the legacy
//! names. Every body is the TS one with `Math.*` as V8 computes it ([`uo_jsnum::math`]) and the special functions of
//! A1 ([`crate::special`]); the inverses use the TS root finder ([`solve`], ≤ 500 steps) and bracket search
//! (≤ 3,000 doublings), so they stop where the TS stops.

use uo_jsnum::math::{abs, exp, expm1, floor, log, log1p, max, min, pow, sqrt, trunc};

use super::{PLAIN, absent, an, collect, nf, pairs};
use crate::coerce::bool_name;
use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::functions::shared::statw::zip_cells;
use crate::functions::shared::{EMPTY, R, Stop, arg, bodies, num_or_err, scalar, view};
use crate::jstext::{js_trim, upper, with_u16};
use crate::special::{bd0, beta_inc, dbinom_raw, dnorm, dpois_raw, gamma, lbeta, lgamma, norm_cdf_xl, pgamma, qnorm, stirlerr};
use crate::stats::{mean, stat_num, variance};
use crate::value::{ErrorCode, Scalar, ScalarView, Value};

use core::f64::consts::PI;

/// The bodies of `distributions.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "NORM.DIST" => norm_dist,
    "NORMDIST" => norm_dist,
    "NORM.S.DIST" => norm_s_dist,
    "NORMSDIST" => normsdist,
    "NORM.INV" => norm_inv,
    "NORMINV" => norm_inv,
    "NORM.S.INV" => norm_s_inv,
    "NORMSINV" => norm_s_inv,
    "LOGNORM.DIST" => lognorm_dist,
    "LOGNORMDIST" => lognormdist,
    "LOGNORM.INV" => lognorm_inv,
    "LOGINV" => lognorm_inv,
    "GAUSS" => gauss,
    "PHI" => phi,
    "EXPON.DIST" => expon_dist,
    "EXPONDIST" => expon_dist,
    "WEIBULL.DIST" => weibull_dist,
    "WEIBULL" => weibull_dist,
    "GAMMA.DIST" => gamma_dist,
    "GAMMADIST" => gamma_dist,
    "GAMMA.INV" => gamma_inv,
    "GAMMAINV" => gamma_inv,
    "GAMMALN" => gammaln,
    "GAMMALN.PRECISE" => gammaln,
    "GAMMA" => gamma_fn,
    "CHISQ.DIST" => chisq_dist,
    "CHISQ.DIST.RT" => chisq_rt,
    "CHIDIST" => chisq_rt,
    "CHISQ.INV" => chisq_inv,
    "CHISQ.INV.RT" => chisq_inv_rt,
    "CHIINV" => chisq_inv_rt,
    "BETA.DIST" => beta_dist,
    "BETADIST" => betadist,
    "BETA.INV" => beta_inv,
    "BETAINV" => beta_inv,
    "T.DIST" => t_dist,
    "T.DIST.RT" => t_dist_rt,
    "T.DIST.2T" => t_dist_2t,
    "TDIST" => tdist,
    "T.INV" => t_inv,
    "T.INV.2T" => t_inv_2t,
    "TINV" => t_inv_2t,
    "F.DIST" => f_dist,
    "F.DIST.RT" => f_dist_rt,
    "FDIST" => f_dist_rt,
    "F.INV" => f_inv,
    "F.INV.RT" => f_inv_rt,
    "FINV" => f_inv_rt,
    "BINOM.DIST" => binom_dist,
    "BINOMDIST" => binom_dist,
    "BINOM.DIST.RANGE" => binom_dist_range,
    "BINOM.INV" => binom_inv,
    "CRITBINOM" => binom_inv,
    "NEGBINOM.DIST" => negbinom_dist,
    "NEGBINOMDIST" => negbinomdist,
    "HYPGEOM.DIST" => hypgeom_dist,
    "HYPGEOMDIST" => hypgeomdist,
    "POISSON.DIST" => poisson_dist,
    "POISSON" => poisson_dist,
    "CONFIDENCE.NORM" => confidence_norm,
    "CONFIDENCE" => confidence_norm,
    "CONFIDENCE.T" => confidence_t,
    "Z.TEST" => z_test,
    "ZTEST" => z_test,
    "T.TEST" => t_test,
    "TTEST" => t_test,
    "F.TEST" => f_test,
    "FTEST" => f_test,
    "CHISQ.TEST" => chisq_test,
    "CHITEST" => chisq_test,
];

/// A body's numeric answer (`number | CalcError`, the number as is) as a value.
type N = Result<f64, ErrorCode>;

/// `N` → the evaluator's result.
#[inline]
fn out(r: N) -> R<Value> {
    Ok(num_or_err(r))
}

/// `num(x)` of `stats.ts` as an `N`.
#[inline]
fn fin(x: f64) -> N {
    stat_num(x)
}

// ---------------------------------------------------------------- support

/// `nums(args, ctx, n)`: the first `N` arguments as numbers (`args[i] ?? null`); the first failure decides.
fn nums<const K: usize>(a: &CallArgs<'_>, ctx: &dyn FnContext) -> Result<[f64; K], ErrorCode> {
    let mut v = [0.0; K];
    for (i, d) in v.iter_mut().enumerate() {
        *d = an(ctx, scalar(a, i))?;
    }
    Ok(v)
}

/// `logical(v, ctx)`: number ≠ 0, boolean, empty = FALSE; text only as a boolean name ("1" → `#VALUE!`).
fn logical(ctx: &dyn FnContext, v: Option<Scalar>) -> Result<bool, ErrorCode> {
    match v {
        None | Some(Scalar::Empty) => Ok(false),
        Some(Scalar::Bool(b)) => Ok(b),
        Some(Scalar::Num(n)) => Ok(n != 0.0),
        Some(Scalar::Err(e)) => Err(e),
        Some(s @ Scalar::Str(_)) => {
            let ScalarView::Str(t) = view(ctx, s) else { return Err(ErrorCode::Value) };
            with_u16(t, |u| bool_name(&upper(js_trim(u)), ctx.locale())).ok_or(ErrorCode::Value)
        }
    }
}

/// `optNum(v, ctx, dflt)`.
fn opt_num(ctx: &dyn FnContext, v: Option<Scalar>, d: f64) -> N {
    if absent(v) { Ok(d) } else { an(ctx, v) }
}

/// `solve(f, target, lo, hi)`: Brent's root of `f(x) = target` in `[lo, hi]`, at most 500 steps.
pub(super) fn solve(f: &dyn Fn(f64) -> f64, target: f64, lo: f64, hi: f64) -> f64 {
    let (mut a, mut b) = (lo, hi);
    let mut fa = f(a) - target;
    let mut fb = f(b) - target;
    if fa == 0.0 {
        return a;
    }
    if fb == 0.0 {
        return b;
    }
    if fa * fb > 0.0 {
        return f64::NAN;
    }
    let (mut c, mut fc, mut d) = (a, fa, b - a);
    let mut e = d;
    for _ in 0..500 {
        if fb * fc > 0.0 {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if abs(fc) < abs(fb) {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol = 2.0 * f64::EPSILON * abs(b) + 1e-300;
        let m = 0.5 * (c - b);
        if abs(m) <= tol || fb == 0.0 {
            return b;
        }
        if abs(e) >= tol && abs(fa) > abs(fb) {
            let (mut p, mut q);
            let s = fb / fa;
            if a == c {
                p = 2.0 * m * s;
                q = 1.0 - s;
            } else {
                let qq = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * m * qq * (qq - r) - (b - a) * (r - 1.0));
                q = (qq - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            if 2.0 * p < min(3.0 * m * q - abs(tol * q), abs(e * q)) {
                e = d;
                d = p / q;
            } else {
                d = m;
                e = d;
            }
        } else {
            d = m;
            e = d;
        }
        a = b;
        fa = fb;
        b += if abs(d) > tol {
            d
        } else if m > 0.0 {
            tol
        } else {
            -tol
        };
        fb = f(b) - target;
    }
    b
}

/// `quantilePos(cdf, p, x0, upperP)`: the quantile of a distribution on `[0, ∞)`, solved in the smaller tail.
fn quantile_pos(cdf: &dyn Fn(f64, bool) -> f64, p: f64, x0: f64, upper_p: Option<f64>) -> f64 {
    let lower = p <= 0.5 && upper_p.is_none();
    let t = if lower { p } else { upper_p.unwrap_or(1.0 - p) };
    let f = |x: f64| if lower { cdf(x, true) } else { -cdf(x, false) };
    let target = if lower { t } else { -t };
    let mut hi = max(x0, 1e-300);
    let mut i = 0;
    while i < 3000 && f(hi) < target {
        hi *= 2.0;
        i += 1;
    }
    let mut lo = min(x0, hi);
    let mut i = 0;
    while i < 3000 && lo > 0.0 && f(lo) > target {
        lo /= 2.0;
        i += 1;
    }
    if f(lo) > target {
        lo = 0.0;
    }
    solve(&f, target, lo, hi)
}

// ---------------------------------------------------------------- normal and lognormal

fn normal(a: &CallArgs<'_>, ctx: &dyn FnContext, x: f64, m: f64, s: f64, cum_at: usize) -> N {
    let cum = logical(ctx, scalar(a, cum_at))?;
    if s <= 0.0 {
        return Err(ErrorCode::Num);
    }
    let z = (x - m) / s;
    fin(if cum { norm_cdf_xl(z) } else { dnorm(z) / s })
}

fn norm_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[x, m, s]| normal(a, ctx, x, m, s, 3)))
}

fn norm_s_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[x]| normal(a, ctx, x, 0.0, 1.0, 1)))
}

fn normsdist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // `normDist([a[0] ?? null, true], c, true)`
    out(nums::<1>(a, ctx).and_then(|[x]| fin(norm_cdf_xl(x))))
}

fn norm_inv_of(p: f64, m: f64, s: f64) -> N {
    if p <= 0.0 || p >= 1.0 || s <= 0.0 {
        return Err(ErrorCode::Num);
    }
    fin(m + s * qnorm(p))
}

fn norm_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[p, m, s]| norm_inv_of(p, m, s)))
}

fn norm_s_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[p]| norm_inv_of(p, 0.0, 1.0)))
}

fn lognormal(a: &CallArgs<'_>, ctx: &dyn FnContext, legacy: bool) -> N {
    let [x, m, s] = nums::<3>(a, ctx)?;
    let cum = if legacy { true } else { logical(ctx, scalar(a, 3))? };
    if x <= 0.0 || s <= 0.0 {
        return Err(ErrorCode::Num);
    }
    let z = (log(x) - m) / s;
    fin(if cum { norm_cdf_xl(z) } else { dnorm(z) / (x * s) })
}

fn lognorm_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(lognormal(a, ctx, false))
}

fn lognormdist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(lognormal(a, ctx, true))
}

fn lognorm_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[p, m, s]| {
        if p <= 0.0 || p >= 1.0 || s <= 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(exp(m + s * qnorm(p)))
    }))
}

fn gauss(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[x]| fin(norm_cdf_xl(x) - 0.5)))
}

fn phi(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[x]| fin(dnorm(x))))
}

// ---------------------------------------------------------------- exponential and Weibull

fn expon_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, l] = nums::<2>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 2))?;
        if x < 0.0 || l <= 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(if cum { -expm1(-l * x) } else { l * exp(-l * x) })
    };
    out(f())
}

fn weibull_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, al, be] = nums::<3>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 3))?;
        if x < 0.0 || al <= 0.0 || be <= 0.0 {
            return Err(ErrorCode::Num);
        }
        let t = pow(x / be, al);
        if cum {
            return fin(-expm1(-t));
        }
        if x == 0.0 {
            return Ok(0.0);
        }
        fin((al / pow(be, al)) * pow(x, al - 1.0) * exp(-t))
    };
    out(f())
}

// ---------------------------------------------------------------- gamma and chi-squared

/// `gammaPdf(x, a, b)`: direct, else Loader's form.
fn gamma_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x == 0.0 {
        return if a <= 1.0 { f64::NAN } else { 0.0 };
    }
    let direct = pow(x, a - 1.0) * exp(-x / b) / (pow(b, a) * exp(lgamma(a)));
    if direct.is_finite() && direct > 1e-300 {
        return direct;
    }
    let y = x / b;
    if a >= 1.0 {
        return dpois_raw(a - 1.0, y) / b;
    }
    dpois_raw(a, y) * a / x
}

fn gamma_cdf(x: f64, a: f64, b: f64, lower: bool) -> f64 {
    pgamma(x / b, a, lower)
}

fn gamma_quantile(p: f64, a: f64, b: f64) -> f64 {
    quantile_pos(&|x, lo| gamma_cdf(x, a, b, lo), p, a * b, None)
}

fn gamma_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, al, be] = nums::<3>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 3))?;
        if x < 0.0 || al <= 0.0 || be <= 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(if cum { gamma_cdf(x, al, be, true) } else { gamma_pdf(x, al, be) })
    };
    out(f())
}

fn gamma_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[p, al, be]| {
        if p < 0.0 || p >= 1.0 || al <= 0.0 || be <= 0.0 {
            return Err(ErrorCode::Num);
        }
        if p == 0.0 {
            return Ok(0.0);
        }
        fin(gamma_quantile(p, al, be))
    }))
}

fn gammaln(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[x]| if x <= 0.0 { Err(ErrorCode::Num) } else { fin(lgamma(x)) }))
}

fn gamma_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<1>(a, ctx).and_then(|[x]| if x <= 0.0 && x == floor(x) { Err(ErrorCode::Num) } else { fin(gamma(x)) }))
}

/// `dfOf(x)`: truncated degrees of freedom in `[1, 10^10]`.
fn df_of(x: f64) -> N {
    let d = trunc(x);
    if !(1.0..=1e10).contains(&d) { Err(ErrorCode::Num) } else { Ok(d) }
}

fn chisq_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, d] = nums::<2>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 2))?;
        let df = df_of(d)?;
        if x < 0.0 {
            return Err(ErrorCode::Num);
        }
        if !cum && x == 0.0 {
            return if df == 2.0 {
                Ok(0.5)
            } else if df == 1.0 {
                Err(ErrorCode::Num)
            } else {
                Ok(0.0)
            };
        }
        fin(if cum { gamma_cdf(x, df / 2.0, 2.0, true) } else { gamma_pdf(x, df / 2.0, 2.0) })
    };
    out(f())
}

fn chisq_rt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<2>(a, ctx).and_then(|[x, d]| {
        let df = df_of(d)?;
        if x < 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(gamma_cdf(x, df / 2.0, 2.0, false))
    }))
}

fn chisq_inv_of(a: &CallArgs<'_>, ctx: &dyn FnContext, rt: bool) -> N {
    let [p, d] = nums::<2>(a, ctx)?;
    let df = df_of(d)?;
    if !(0.0..=1.0).contains(&p) {
        return Err(ErrorCode::Num);
    }
    if rt {
        if p == 0.0 {
            return Err(ErrorCode::Num);
        }
        if p == 1.0 {
            return Ok(0.0);
        }
        if df == 1.0 {
            return fin(gamma_quantile(1.0 - p, 0.5, 2.0));
        }
        return fin(if p > 0.5 {
            gamma_quantile(1.0 - p, df / 2.0, 2.0)
        } else {
            quantile_pos(&|x, lo| gamma_cdf(x, df / 2.0, 2.0, lo), 1.0 - p, df, Some(p))
        });
    }
    if p == 1.0 {
        return Err(ErrorCode::Num);
    }
    if p == 0.0 {
        return Ok(0.0);
    }
    fin(gamma_quantile(p, df / 2.0, 2.0))
}

fn chisq_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(chisq_inv_of(a, ctx, false))
}

fn chisq_inv_rt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(chisq_inv_of(a, ctx, true))
}

// ---------------------------------------------------------------- beta

fn beta_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x == 0.0 {
        return if a <= 1.0 { f64::NAN } else { 0.0 };
    }
    if x == 1.0 {
        return if b < 1.0 {
            f64::NAN
        } else if b == 1.0 {
            a
        } else {
            0.0
        };
    }
    if a <= 2.0 || b <= 2.0 {
        return exp((a - 1.0) * log(x) + (b - 1.0) * log1p(-x) - lbeta(a, b));
    }
    (a + b - 1.0) * dbinom_raw(a - 1.0, a + b - 2.0, x, 1.0 - x)
}

/// `betaBounds(a, i, ctx)`: the optional A and B.
fn beta_bounds(a: &CallArgs<'_>, i: usize, ctx: &dyn FnContext) -> Result<(f64, f64), ErrorCode> {
    let lo = opt_num(ctx, scalar(a, i), 0.0)?;
    let hi = opt_num(ctx, scalar(a, i + 1), 1.0)?;
    Ok((lo, hi))
}

fn beta_dist_of(a: &CallArgs<'_>, ctx: &dyn FnContext, legacy: bool) -> N {
    let [x, al, be] = nums::<3>(a, ctx)?;
    let cum = if legacy { true } else { logical(ctx, scalar(a, 3))? };
    let (lo, hi) = beta_bounds(a, if legacy { 3 } else { 4 }, ctx)?;
    if al <= 0.0 || be <= 0.0 || x < lo || x > hi || lo == hi {
        return Err(ErrorCode::Num);
    }
    let y = (x - lo) / (hi - lo);
    fin(if cum { beta_inc(y, al, be).0 } else { beta_pdf(y, al, be) / (hi - lo) })
}

fn beta_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(beta_dist_of(a, ctx, false))
}

fn betadist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(beta_dist_of(a, ctx, true))
}

fn beta_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [p, al, be] = nums::<3>(a, ctx)?;
        let (lo, hi) = beta_bounds(a, 3, ctx)?;
        if p <= 0.0 || p >= 1.0 || al <= 0.0 || be <= 0.0 || lo >= hi {
            return Err(ErrorCode::Num);
        }
        let y = if p <= 0.5 {
            solve(&|t| beta_inc(t, al, be).0, p, 0.0, 1.0)
        } else {
            solve(&|t| -beta_inc(t, al, be).1, -(1.0 - p), 0.0, 1.0)
        };
        fin(lo + y * (hi - lo))
    };
    out(f())
}

// ---------------------------------------------------------------- Student's t

/// `tUpper(t, df)`: `P(T > t)` for `t ≥ 0`.
pub(super) fn t_upper(t: f64, df: f64) -> f64 {
    if t == 0.0 {
        return 0.5;
    }
    if !t.is_finite() {
        return 0.0;
    }
    let t2 = t * t;
    if t2 > df {
        return 0.5 * beta_inc(df / (df + t2), df / 2.0, 0.5).0;
    }
    0.5 * beta_inc(t2 / (df + t2), 0.5, df / 2.0).1
}

fn t_cdf(t: f64, df: f64, lower: bool) -> f64 {
    let u = t_upper(abs(t), df);
    if (t >= 0.0) == lower { 1.0 - u } else { u }
}

/// `tPdf(t, n)`: Loader's density.
fn t_pdf(t: f64, n: f64) -> f64 {
    let tt = -bd0(n / 2.0, (n + 1.0) / 2.0) + stirlerr((n + 1.0) / 2.0) - stirlerr(n / 2.0);
    let x2n = t * t / n;
    let u = if t * t > 0.2 * n { log(1.0 + x2n) * n / 2.0 } else { -bd0(n / 2.0, (n + t * t) / 2.0) + t * t / 2.0 };
    exp(tt - u) / sqrt(2.0 * PI * (1.0 + x2n))
}

/// `tUpperQuantile(q, df)`: `t ≥ 0` with `P(T > t) = q`.
fn t_upper_quantile(q: f64, df: f64) -> f64 {
    if q >= 0.5 {
        return 0.0;
    }
    let f = |t: f64| -t_upper(t, df);
    let mut hi = 1.0;
    let mut i = 0;
    while i < 3000 && f(hi) < -q {
        hi *= 2.0;
        i += 1;
    }
    solve(&f, -q, 0.0, hi)
}

fn t_quantile(p: f64, df: f64) -> f64 {
    if p < 0.5 { -t_upper_quantile(p, df) } else { t_upper_quantile(1.0 - p, df) }
}

fn t_df(x: f64) -> N {
    let df = trunc(x);
    if df < 1.0 { Err(ErrorCode::Num) } else { Ok(df) }
}

fn t_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, d] = nums::<2>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 2))?;
        let df = t_df(d)?;
        fin(if cum { t_cdf(x, df, true) } else { t_pdf(x, df) })
    };
    out(f())
}

fn t_dist_rt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<2>(a, ctx).and_then(|[x, d]| fin(t_cdf(x, t_df(d)?, false))))
}

fn t_dist_2t(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<2>(a, ctx).and_then(|[x, d]| {
        let df = t_df(d)?;
        if x < 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(2.0 * t_upper(x, df))
    }))
}

fn tdist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[x, d, tl]| {
        let df = t_df(d)?;
        let tails = trunc(tl);
        if x < 0.0 || (tails != 1.0 && tails != 2.0) {
            return Err(ErrorCode::Num);
        }
        fin(tails * t_upper(x, df))
    }))
}

fn t_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<2>(a, ctx).and_then(|[p, d]| {
        let df = t_df(d)?;
        if p == 0.0 && df == 1.0 {
            return Err(ErrorCode::Div0);
        }
        if p <= 0.0 || p >= 1.0 {
            return Err(ErrorCode::Num);
        }
        fin(t_quantile(p, df))
    }))
}

fn t_inv_2t(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<2>(a, ctx).and_then(|[p, d]| {
        let df = t_df(d)?;
        if p == 0.0 && df == 1.0 {
            return Err(ErrorCode::Div0);
        }
        if p <= 0.0 || p > 1.0 {
            return Err(ErrorCode::Num);
        }
        fin(t_upper_quantile(p / 2.0, df))
    }))
}

// ---------------------------------------------------------------- Snedecor's F

fn f_cdf(x: f64, d1: f64, d2: f64, lower: bool) -> f64 {
    if x <= 0.0 {
        return if lower { 0.0 } else { 1.0 };
    }
    if !x.is_finite() {
        return if lower { 1.0 } else { 0.0 };
    }
    if lower { beta_inc(d1 * x / (d1 * x + d2), d1 / 2.0, d2 / 2.0).0 } else { beta_inc(d2 / (d2 + d1 * x), d2 / 2.0, d1 / 2.0).0 }
}

fn f_pdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x == 0.0 {
        return if d1 < 2.0 {
            f64::NAN
        } else if d1 == 2.0 {
            1.0
        } else {
            0.0
        };
    }
    exp(0.5 * d1 * log(d1 * x) + 0.5 * d2 * log(d2) - 0.5 * (d1 + d2) * log(d1 * x + d2) - log(x) - lbeta(d1 / 2.0, d2 / 2.0))
}

/// `fArgs(a, ctx)`: x and the truncated degrees of freedom in `[1, 10^10)`.
fn f_args(a: &CallArgs<'_>, ctx: &dyn FnContext) -> Result<(f64, f64, f64), ErrorCode> {
    let [x, a1, a2] = nums::<3>(a, ctx)?;
    let (d1, d2) = (trunc(a1), trunc(a2));
    if d1 < 1.0 || d2 < 1.0 || d1 >= 1e10 || d2 >= 1e10 {
        return Err(ErrorCode::Num);
    }
    Ok((x, d1, d2))
}

fn f_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let (x, d1, d2) = f_args(a, ctx)?;
        let cum = logical(ctx, scalar(a, 3))?;
        if x < 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(if cum { f_cdf(x, d1, d2, true) } else { f_pdf(x, d1, d2) })
    };
    out(f())
}

fn f_dist_rt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(f_args(a, ctx).and_then(|(x, d1, d2)| if x < 0.0 { Err(ErrorCode::Num) } else { fin(f_cdf(x, d1, d2, false)) }))
}

fn f_inv_of(a: &CallArgs<'_>, ctx: &dyn FnContext, rt: bool) -> N {
    let (p, d1, d2) = f_args(a, ctx)?;
    if !(0.0..=1.0).contains(&p) {
        return Err(ErrorCode::Num);
    }
    let cdf = |x: f64, lo: bool| f_cdf(x, d1, d2, lo);
    if rt {
        if p == 0.0 {
            return Err(ErrorCode::Num);
        }
        if p == 1.0 {
            return Ok(0.0);
        }
        return fin(if p >= 0.5 { quantile_pos(&cdf, 1.0 - p, 1.0, None) } else { quantile_pos(&cdf, 1.0 - p, 1.0, Some(p)) });
    }
    if p == 1.0 {
        return Err(ErrorCode::Num);
    }
    if p == 0.0 {
        return Ok(0.0);
    }
    fin(quantile_pos(&cdf, p, 1.0, None))
}

fn f_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(f_inv_of(a, ctx, false))
}

fn f_inv_rt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(f_inv_of(a, ctx, true))
}

// ---------------------------------------------------------------- discrete: binomial, negative binomial, hypergeometric, Poisson

fn binom_pmf(k: f64, n: f64, p: f64) -> f64 {
    dbinom_raw(k, n, p, 1.0 - p)
}

/// `MAX_N`: n (or the population) ≥ 2^31 − 1 → `#NUM!` (measured).
const MAX_N: f64 = 2_147_483_647.0;

/// `MAX_TERMS`: terms summed; past it, the incomplete beta.
const MAX_TERMS: f64 = 5000.0;

fn binom_cdf(k: f64, n: f64, p: f64) -> f64 {
    if k >= n {
        return 1.0;
    }
    if k > MAX_TERMS {
        return beta_inc(1.0 - p, n - k, k + 1.0).0;
    }
    let mut s = 0.0;
    let mut i = 0.0;
    while i <= k {
        s += binom_pmf(i, n, p);
        i += 1.0;
    }
    min(s, 1.0)
}

fn binom_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [k, n, p] = nums::<3>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 3))?;
        let (k, n) = (trunc(k), trunc(n));
        if k < 0.0 || k > n || !(0.0..=1.0).contains(&p) || n >= MAX_N {
            return Err(ErrorCode::Num);
        }
        if !cum && k == 0.0 && p == 1.0 && n > 0.0 {
            return Err(ErrorCode::Num);
        }
        fin(if cum { binom_cdf(k, n, p) } else { binom_pmf(k, n, p) })
    };
    out(f())
}

fn binom_dist_range(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [n, p, s] = nums::<3>(a, ctx)?;
        let (n, s) = (trunc(n), trunc(s));
        let s2 = trunc(opt_num(ctx, scalar(a, 3), s)?);
        if n < 0.0 || !(0.0..=1.0).contains(&p) || s < 0.0 || s > n || s2 < s || s2 > n || n >= MAX_N {
            return Err(ErrorCode::Num);
        }
        if s2 - s > MAX_TERMS {
            return fin(binom_cdf(s2, n, p) - if s > 0.0 { binom_cdf(s - 1.0, n, p) } else { 0.0 });
        }
        let mut t = 0.0;
        let mut k = s;
        while k <= s2 {
            t += binom_pmf(k, n, p);
            k += 1.0;
        }
        fin(t)
    };
    out(f())
}

/// `binomInv`: the smallest k with `P(X ≤ k) > α`.
fn binom_inv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[n, p, al]| {
        let n = trunc(n);
        if n < 0.0 || p <= 0.0 || p >= 1.0 || al <= 0.0 || al >= 1.0 || n >= MAX_N {
            return Err(ErrorCode::Num);
        }
        if n > MAX_TERMS {
            let (mut lo, mut hi) = (0.0, n);
            while lo < hi {
                let mid = floor((lo + hi) / 2.0);
                if binom_cdf(mid, n, p) > al {
                    hi = mid;
                } else {
                    lo = mid + 1.0;
                }
            }
            return Ok(lo);
        }
        let mut s = 0.0;
        let mut k = 0.0;
        while k <= n {
            s += binom_pmf(k, n, p);
            if s > al {
                return Ok(k);
            }
            k += 1.0;
        }
        Ok(n)
    }))
}

fn negbinom_pmf(f: f64, s: f64, p: f64) -> f64 {
    p * dbinom_raw(s - 1.0, f + s - 1.0, p, 1.0 - p)
}

fn negbinom_of(a: &CallArgs<'_>, ctx: &dyn FnContext, legacy: bool) -> N {
    let [f, s, p] = nums::<3>(a, ctx)?;
    let cum = if legacy { false } else { logical(ctx, scalar(a, 3))? };
    let (f, s) = (trunc(f), trunc(s));
    if f < 0.0 || s < 1.0 || p <= 0.0 || p >= 1.0 {
        return Err(ErrorCode::Num);
    }
    if !cum {
        return fin(negbinom_pmf(f, s, p));
    }
    if f > MAX_TERMS {
        return fin(beta_inc(p, s, f + 1.0).0);
    }
    let mut t = 0.0;
    let mut i = 0.0;
    while i <= f {
        t += negbinom_pmf(i, s, p);
        i += 1.0;
    }
    fin(min(t, 1.0))
}

fn negbinom_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(negbinom_of(a, ctx, false))
}

fn negbinomdist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(negbinom_of(a, ctx, true))
}

#[allow(non_snake_case)]
fn hypgeom_pmf(x: f64, n: f64, M: f64, N: f64) -> f64 {
    if x < max(0.0, n - N + M) || x > min(n, M) {
        return 0.0;
    }
    if n == 0.0 || n == N {
        return 1.0;
    }
    let p = n / N;
    let q = (N - n) / N;
    dbinom_raw(x, M, p, q) * dbinom_raw(n - x, N - M, p, q) / dbinom_raw(n, N, p, q)
}

#[allow(non_snake_case)]
fn hypgeom_of(a: &CallArgs<'_>, ctx: &dyn FnContext, legacy: bool) -> N {
    let [x, n, m, nn] = nums::<4>(a, ctx)?;
    let cum = if legacy { false } else { logical(ctx, scalar(a, 4))? };
    let (x, n, M, N) = (trunc(x), trunc(n), trunc(m), trunc(nn));
    if x < 0.0 || n < 0.0 || M < 0.0 || N <= 0.0 || n > N || M > N || N >= MAX_N {
        return Err(ErrorCode::Num);
    }
    if !cum {
        return fin(hypgeom_pmf(x, n, M, N));
    }
    let lo = max(0.0, n - N + M);
    let hi = min(min(x, n), M);
    if hi - lo > 1e6 {
        return Err(ErrorCode::Num);
    }
    let mut t = 0.0;
    let mut i = lo;
    while i <= hi {
        t += hypgeom_pmf(i, n, M, N);
        i += 1.0;
    }
    fin(min(t, 1.0))
}

fn hypgeom_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(hypgeom_of(a, ctx, false))
}

fn hypgeomdist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(hypgeom_of(a, ctx, true))
}

fn poisson_dist(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = || -> N {
        let [x, l] = nums::<2>(a, ctx)?;
        let cum = logical(ctx, scalar(a, 2))?;
        let x = trunc(x);
        if x < 0.0 || l < 0.0 {
            return Err(ErrorCode::Num);
        }
        if !cum {
            return fin(dpois_raw(x, l));
        }
        fin(if l == 0.0 { 1.0 } else { pgamma(l, x + 1.0, false) })
    };
    out(f())
}

// ---------------------------------------------------------------- confidence intervals

fn confidence_norm(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[al, s, n]| {
        let n = trunc(n);
        if al <= 0.0 || al >= 1.0 || s <= 0.0 || n < 1.0 {
            return Err(ErrorCode::Num);
        }
        fin(qnorm(1.0 - al / 2.0) * s / sqrt(n))
    }))
}

fn confidence_t(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    out(nums::<3>(a, ctx).and_then(|[al, s, n]| {
        let n = trunc(n);
        if al <= 0.0 || al >= 1.0 || s <= 0.0 || n < 1.0 {
            return Err(ErrorCode::Num);
        }
        if n == 1.0 {
            return Err(ErrorCode::Div0);
        }
        fin(t_upper_quantile(al / 2.0, n - 1.0) * s / sqrt(n))
    }))
}

// ---------------------------------------------------------------- hypothesis tests

/// `dataNums(v, ctx)`.
fn data_nums(a: &CallArgs<'_>, i: usize, ctx: &mut dyn FnContext) -> R<Vec<f64>> {
    collect(arg(a, i).unwrap_or(&EMPTY), ctx, &PLAIN)
}

fn z_test(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xs = data_nums(a, 0, ctx)?;
    let x = an(ctx, scalar(a, 1))?;
    if xs.is_empty() {
        return out(Err(ErrorCode::NA));
    }
    let n = nf(xs.len());
    let s = if absent(scalar(a, 2)) {
        if xs.len() < 2 {
            return out(Err(ErrorCode::Div0));
        }
        let s = sqrt(variance(&xs, true));
        if s == 0.0 {
            return out(Err(ErrorCode::Div0));
        }
        s
    } else {
        let t = an(ctx, scalar(a, 2))?;
        if t <= 0.0 {
            return out(Err(ErrorCode::Num));
        }
        t
    };
    // 1 − Φ(z) as Φ(−z)
    out(fin(norm_cdf_xl(-(mean(&xs) - x) / (s / sqrt(n)))))
}

fn t_test(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let tails_n = an(ctx, scalar(a, 2))?;
    let type_n = an(ctx, scalar(a, 3))?;
    let (tails, ty) = (trunc(tails_n), trunc(type_n));
    if (tails != 1.0 && tails != 2.0) || !(1.0..=3.0).contains(&ty) {
        return out(Err(ErrorCode::Num));
    }
    let (t, df);
    if ty == 1.0 {
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        pairs(arg(a, 0).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY), ctx, &mut xs, &mut ys)?;
        let d: Vec<f64> = xs.iter().zip(ys.iter()).map(|(x, y)| x - y).collect();
        if d.len() < 2 {
            return out(Err(ErrorCode::Div0));
        }
        let sd = sqrt(variance(&d, true));
        if sd == 0.0 {
            return out(Err(ErrorCode::Div0));
        }
        t = mean(&d) / (sd / sqrt(nf(d.len())));
        df = nf(d.len()) - 1.0;
    } else {
        // both collected before either error is answered (a throw of the second walk wins, as in the TS)
        let (xs, ys) = two_data(a, ctx)?;
        let (n1, n2) = (nf(xs.len()), nf(ys.len()));
        if xs.len() < 2 || ys.len() < 2 {
            return out(Err(ErrorCode::Div0));
        }
        let (v1, v2) = (variance(&xs, true), variance(&ys, true));
        let (m1, m2) = (mean(&xs), mean(&ys));
        if ty == 2.0 {
            df = n1 + n2 - 2.0;
            let sp = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / df;
            let se = sqrt(sp * (1.0 / n1 + 1.0 / n2));
            if se == 0.0 {
                return out(Err(ErrorCode::Div0));
            }
            t = (m1 - m2) / se;
        } else {
            let (x, y) = (v1 / n1, v2 / n2);
            if x + y == 0.0 {
                return out(Err(ErrorCode::Div0));
            }
            t = (m1 - m2) / sqrt(x + y);
            df = pow(x + y, 2.0) / (x * x / (n1 - 1.0) + y * y / (n2 - 1.0));
        }
    }
    out(fin(tails * t_upper(abs(t), df)))
}

/// Collects the two data arguments as the TS does (both walks first), then answers the first error.
fn two_data(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<(Vec<f64>, Vec<f64>)> {
    let xs = data_nums(a, 0, ctx);
    if let Err(Stop::Throw(t)) = xs {
        return Err(Stop::Throw(t));
    }
    let ys = data_nums(a, 1, ctx);
    if let Err(Stop::Throw(t)) = ys {
        return Err(Stop::Throw(t));
    }
    Ok((xs?, ys?))
}

fn f_test(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (xs, ys) = two_data(a, ctx)?;
    if xs.len() < 2 || ys.len() < 2 {
        return out(Err(ErrorCode::Div0));
    }
    let (v1, v2) = (variance(&xs, true), variance(&ys, true));
    if v1 == 0.0 || v2 == 0.0 {
        return out(Err(ErrorCode::Div0));
    }
    let f = v1 / v2;
    let (d1, d2) = (nf(xs.len()) - 1.0, nf(ys.len()) - 1.0);
    let p = f_cdf(f, d1, d2, f <= 1.0);
    out(fin(min(2.0 * p, 1.0)))
}

/// `dims(v)` of CHISQ.TEST.
fn dims(v: &Value) -> (u64, u64) {
    match v {
        Value::Ref(r) => match r.single() {
            Some(s) => (u64::from(s.r2 - s.r1) + 1, u64::from(s.c2 - s.c1) + 1),
            None => (1, 1),
        },
        Value::Array(x) => (u64::from(x.rows), u64::from(x.cols)),
        _ => (1, 1),
    }
}

fn chisq_test(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let av = arg(a, 0).unwrap_or(&EMPTY);
    let ev = arg(a, 1).unwrap_or(&EMPTY);
    let many = |v: &Value| matches!(v, Value::Ref(r) if r.areas().len() != 1);
    if many(av) || many(ev) {
        return out(Err(ErrorCode::Value));
    }
    let ((r, c), (r2, c2)) = (dims(av), dims(ev));
    if r != r2 || c != c2 {
        return out(Err(ErrorCode::NA));
    }
    let mut chi = 0.0;
    zip_cells(av, ev, ctx, &mut |_, x, y| {
        if let Scalar::Err(e) = x {
            return Err(Stop::Err(e));
        }
        if let Scalar::Err(e) = y {
            return Err(Stop::Err(e));
        }
        let (Scalar::Num(x), Scalar::Num(y)) = (x, y) else { return Ok(()) };
        if y == 0.0 {
            return Err(Stop::Err(ErrorCode::Div0));
        }
        chi += pow(x - y, 2.0) / y;
        Ok(())
    })?;
    let df = if r > 1 && c > 1 { (r - 1) * (c - 1) } else { r * c - 1 };
    if df < 1 {
        return out(Err(ErrorCode::NA));
    }
    out(fin(pgamma(chi / 2.0, crate::cast::u53_to_f64(df) / 2.0, false)))
}
