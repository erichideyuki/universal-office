//! `trend.ts`: trendlines (`c:trendline`), least squares as Excel does (frontier C-2): linear (optional fixed
//! intercept), polynomial (order 2-6), exponential and power (in log space), logarithmic, and the moving
//! average. The R² of the log-linearised kinds is the one of the transformed fit (as Excel). The equation text
//! has 4 decimals, decimal comma (pt-BR) and superscript exponents.
//!
//! The TS `TrendFit.f` closure is [`TrendFit::eval`] over the stored model; every operation keeps the TS order.

use uo_jsnum::format::to_fixed;
use uo_jsnum::math::{abs, exp, log, max, min, pow, round};

use crate::error::JsErrorKind;
pub use crate::layout::{EqPart, TrendFit, TrendModel};

/// The options of `fitTrend` (`order`, `period`, `intercept`); `None` is `undefined`.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct TrendOpts {
    pub order: Option<f64>,
    pub period: Option<f64>,
    pub intercept: Option<f64>,
}

impl TrendFit {
    /// `f(x)`: the curve (`None` = `undefined`, outside the domain, e.g. the log of x <= 0).
    pub fn eval(&self, x: f64) -> Option<f64> {
        match &self.model {
            TrendModel::MovingAvg => None,
            TrendModel::Exp { a, b } => Some(a * exp(b * x)),
            TrendModel::Power { a, b } => (x > 0.0).then(|| a * pow(x, *b)),
            TrendModel::Log { c0, c1 } => (x > 0.0).then(|| c0 + c1 * log(x)),
            TrendModel::Poly(c) => {
                let mut acc = 0.0;
                for (i, k) in c.iter().enumerate() {
                    acc += k * pow(x, i as f64);
                }
                Some(acc)
            }
        }
    }
}

/// `solve(A, b)`: A·c = b by Gauss with partial pivoting.
fn solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.push(b.get(i).copied().unwrap_or(f64::NAN));
            row
        })
        .collect();
    let at = |m: &[Vec<f64>], r: usize, c: usize| m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(f64::NAN);
    for c in 0..n {
        let mut p = c;
        for r in (c + 1)..n {
            if abs(at(&m, r, c)) > abs(at(&m, p, c)) {
                p = r;
            }
        }
        if abs(at(&m, p, c)) < 1e-12 {
            return None;
        }
        m.swap(c, p);
        let pivot_row = m.get(c).cloned().unwrap_or_default();
        let pc = pivot_row.get(c).copied().unwrap_or(f64::NAN);
        for r in 0..n {
            if r == c {
                continue;
            }
            let Some(row) = m.get_mut(r) else { continue };
            let k = row.get(c).copied().unwrap_or(f64::NAN) / pc;
            for j in c..=n {
                if let (Some(x), Some(pv)) = (row.get_mut(j), pivot_row.get(j)) {
                    *x -= k * pv;
                }
            }
        }
    }
    Some((0..n).map(|i| at(&m, i, n) / at(&m, i, i)).collect())
}

/// `polyFit(xs, ys, deg, b0)`: coefficients from the constant term up, with an optional fixed intercept.
/// `deg` NaN is the TS `new Array(NaN)` RangeError.
fn poly_fit(xs: &[f64], ys: &[f64], deg: f64, b0: Option<f64>) -> Result<Option<Vec<f64>>, JsErrorKind> {
    let start = if b0.is_some() { 1.0 } else { 0.0 };
    let m = deg + 1.0 - start;
    // `new Array(m)`: a length that is not an integer in 0..2^32-1 throws (deg is NaN, or 1..6 otherwise)
    if !(m >= 0.0 && m == m.floor() && m < 4_294_967_296.0) {
        return Err(JsErrorKind::RangeError);
    }
    let mu = m_to_usize(m);
    let mut a = vec![vec![0.0; mu]; mu];
    let mut b = vec![0.0; mu];
    for (i, x) in xs.iter().enumerate() {
        let y = ys.get(i).copied().unwrap_or(f64::NAN) - b0.unwrap_or(0.0);
        for r in 0..mu {
            let rr = r as f64;
            if let Some(slot) = b.get_mut(r) {
                *slot += y * pow(*x, rr + start);
            }
            for c in 0..mu {
                if let Some(slot) = a.get_mut(r).and_then(|row| row.get_mut(c)) {
                    *slot += pow(*x, rr + start + c as f64 + start);
                }
            }
        }
    }
    let Some(c) = solve(&a, &b) else { return Ok(None) };
    Ok(Some(match b0 {
        Some(b0) => {
            let mut v = Vec::with_capacity(c.len() + 1);
            v.push(b0);
            v.extend(c);
            v
        }
        None => c,
    }))
}

/// A checked small count (`m` is an integer in 0..=7 for every real call; the check above bounds it).
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "m is a non-negative integer below 2^32")]
fn m_to_usize(m: f64) -> usize {
    m as usize
}

/// `r2(ys, fit)`.
fn r2(ys: &[f64], fit: &[f64]) -> f64 {
    let mut sum = 0.0;
    for y in ys {
        sum += y;
    }
    let mean = sum / ys.len() as f64;
    let mut ssr = 0.0;
    let mut sst = 0.0;
    for (i, y) in ys.iter().enumerate() {
        ssr += pow(y - fit.get(i).copied().unwrap_or(f64::NAN), 2.0);
        sst += pow(y - mean, 2.0);
    }
    if sst > 0.0 { 1.0 - ssr / sst } else { 1.0 }
}

/// `trendNum(v)`: the number of a label, 4 decimals, no trailing zeros, decimal comma.
pub fn trend_num(v: f64) -> String {
    let fixed = to_fixed(round(v * 1e4) / 1e4, 4).unwrap_or_default();
    // `.replace(/\.?0+$/, '')`: the trailing zeros and the point before them
    let s = if fixed.ends_with('0') {
        let t = fixed.trim_end_matches('0');
        t.strip_suffix('.').unwrap_or(t)
    } else {
        fixed.as_str()
    };
    let s = if s == "-0" { "0" } else { s };
    s.replacen('.', ",", 1)
}

const SUP: [&str; 7] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶"];

/// `signed(v, first)`: the term with its sign as the equation writes it.
fn signed(v: f64, first: bool) -> String {
    let sign = if first {
        if v < 0.0 { "-" } else { "" }
    } else if v < 0.0 {
        " - "
    } else {
        " + "
    };
    format!("{sign}{}", trend_num(abs(v)))
}

fn part(text: String, sup: bool) -> EqPart {
    EqPart { text: text.into(), sup }
}

/// `fitTrend(kind, xs, ys, o)`. `Err` is the TS RangeError (a polynomial `order` of NaN).
pub fn fit_trend(kind: &str, xs0: &[f64], ys0: &[f64], o: &TrendOpts) -> Result<Option<TrendFit>, JsErrorKind> {
    let pts: Vec<(f64, f64)> =
        xs0.iter().enumerate().filter_map(|(i, x)| ys0.get(i).map(|y| (*x, *y))).filter(|(x, y)| x.is_finite() && y.is_finite()).collect();
    if pts.len() < 2 {
        return Ok(None);
    }
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    if kind == "movingAvg" {
        let p = max(2.0, round(o.period.unwrap_or(2.0)));
        let mut out: Vec<(f64, f64)> = Vec::new();
        let len = ys.len() as f64;
        let mut i = p - 1.0;
        while i < len {
            let mut sum = 0.0;
            for k in f64_index(i - p + 1.0)..=f64_index(i) {
                sum += ys.get(k).copied().unwrap_or(0.0);
            }
            out.push((xs.get(f64_index(i)).copied().unwrap_or(f64::NAN), sum / p));
            i += 1.0;
        }
        return Ok(Some(TrendFit { model: TrendModel::MovingAvg, points: Some(out), r2: None, eq: None }));
    }
    if kind == "exp" || kind == "power" {
        let is_exp = kind == "exp";
        let sel: Vec<(f64, f64)> = pts.iter().copied().filter(|(x, y)| *y > 0.0 && (is_exp || *x > 0.0)).collect();
        if sel.len() < 2 {
            return Ok(None);
        }
        let lx: Vec<f64> = sel.iter().map(|(x, _)| if is_exp { *x } else { log(*x) }).collect();
        let ly: Vec<f64> = sel.iter().map(|(_, y)| log(*y)).collect();
        let b0 = match o.intercept {
            Some(i) if i > 0.0 => Some(log(i)),
            _ => None,
        };
        let Some(c) = poly_fit(&lx, &ly, 1.0, b0)? else { return Ok(None) };
        let (c0, c1) = (c.first().copied().unwrap_or(f64::NAN), c.get(1).copied().unwrap_or(f64::NAN));
        let a = exp(c0);
        let b = c1;
        let fit_l: Vec<f64> = lx.iter().map(|x| c0 + b * x).collect();
        let eq = if is_exp {
            vec![part(format!("y = {}e", trend_num(a)), false), part(format!("{}x", trend_num(b)), true)]
        } else {
            vec![part(format!("y = {}x", trend_num(a)), false), part(trend_num(b), true)]
        };
        let model = if is_exp { TrendModel::Exp { a, b } } else { TrendModel::Power { a, b } };
        return Ok(Some(TrendFit { model, points: None, r2: Some(r2(&ly, &fit_l)), eq: Some(eq) }));
    }
    if kind == "log" {
        let sel: Vec<(f64, f64)> = pts.iter().copied().filter(|(x, _)| *x > 0.0).collect();
        if sel.len() < 2 {
            return Ok(None);
        }
        let lx: Vec<f64> = sel.iter().map(|(x, _)| log(*x)).collect();
        let sy: Vec<f64> = sel.iter().map(|(_, y)| *y).collect();
        let Some(c) = poly_fit(&lx, &sy, 1.0, None)? else { return Ok(None) };
        let (c0, c1) = (c.first().copied().unwrap_or(f64::NAN), c.get(1).copied().unwrap_or(f64::NAN));
        let fit = TrendFit { model: TrendModel::Log { c0, c1 }, points: None, r2: None, eq: None };
        let fitted: Vec<f64> = sel.iter().map(|(x, _)| fit.eval(*x).unwrap_or(f64::NAN)).collect();
        let eq = vec![part(format!("y = {}ln(x){}", signed(c1, true), signed(c0, false)), false)];
        return Ok(Some(TrendFit { r2: Some(r2(&sy, &fitted)), eq: Some(eq), ..fit }));
    }
    let deg = if kind == "poly" { max(2.0, min(6.0, round(o.order.unwrap_or(2.0)))) } else { 1.0 };
    let Some(c) = poly_fit(&xs, &ys, deg, o.intercept)? else { return Ok(None) };
    let fit = TrendFit { model: TrendModel::Poly(c.clone().into_boxed_slice()), points: None, r2: None, eq: None };
    // the equation from the highest degree to the constant term; null terms disappear
    let mut text = String::from("y = ");
    let mut first = true;
    let d = f64_index(deg);
    for iu in (0..=d).rev() {
        let k = c.get(iu).copied().unwrap_or(f64::NAN);
        if abs(k) < 5e-5 && iu != d {
            continue;
        }
        let tail = match iu {
            0 => String::new(),
            1 => "x".to_owned(),
            _ => format!("x{}", SUP.get(iu).map_or_else(|| format!("^{iu}"), |s| (*s).to_owned())),
        };
        text.push_str(&signed(k, first));
        text.push_str(&tail);
        first = false;
    }
    let fitted: Vec<f64> = xs.iter().map(|x| fit.eval(*x).unwrap_or(f64::NAN)).collect();
    Ok(Some(TrendFit { r2: Some(r2(&ys, &fitted)), eq: Some(vec![part(text, false)]), ..fit }))
}

/// An index that the surrounding checks keep in range (a loop counter held as a float).
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a non-negative integer-valued float below the length")]
fn f64_index(i: f64) -> usize {
    if i >= 0.0 { i as usize } else { 0 }
}
