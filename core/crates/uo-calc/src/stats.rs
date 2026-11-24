//! Base of the statistical and database functions (`stats.ts`): collecting numbers from arguments (Excel's rules for
//! reference × array × direct argument), Excel's sum with its cancellation to zero, and the algorithms measured in the
//! oracle (variance, moments).
//!
//! The walkers over references that need the evaluation context (`collectNumbers`/`collectInto`, `scalarsOf`,
//! `gridOf`, `zipCells`, `numericPairs`) are wired by A2/A4; their per-value rules are [`Collect`], the ordering of
//! the sparse pairing of `zipCells` is [`zip_sparse`], and the scalar argument rules are [`arg_number`],
//! [`arg_int`], [`arg_bool`].

use std::cmp::Ordering;

use uo_jsnum::math::{abs, floor, log2, pow, sqrt, trunc};

use crate::coerce::{bool_name, parse_number_text};
use crate::jstext::{js_trim, upper, with_u16};
use crate::key::Area;
use crate::locale::ValueLocale;
use crate::math_util::snap_add;
use crate::value::{ErrorCode, ScalarView};

/// `exponentOf` of `stats.ts`: `⌊log2|x|⌋` through `Math.log2` (not the bit-exact one of `math-util.ts`).
fn exponent_log2(x: f64) -> f64 {
    floor(log2(abs(x)))
}

/// `xlAdd(a, b)`: `a + b` as the `+` operator and `SUM`/`AVERAGE` add (measured): opposite signs and a result that
/// lost 50 bits or more against the FIRST operand (the accumulator) → 0.
#[must_use]
pub fn xl_add(a: f64, b: f64) -> f64 {
    let r = a + b;
    if r != 0.0 && a != 0.0 && (a > 0.0) != (b > 0.0) && r.is_finite() && exponent_log2(r) <= exponent_log2(a) - 50.0 {
        return 0.0;
    }
    r
}

/// `xlSum(xs)`: left to right, the cancellation only on the LAST addition ([`snap_add`]).
#[must_use]
pub fn xl_sum(xs: &[f64]) -> f64 {
    let Some((&last, head)) = xs.split_last() else { return 0.0 };
    let mut s = 0.0;
    for &x in head {
        s += x;
    }
    snap_add(s, last)
}

/// `num(x)` of `stats.ts`: ∞/NaN → `#NUM!`; a subnormal (|x| < 2.2e−308) becomes 0.
pub fn stat_num(x: f64) -> Result<f64, ErrorCode> {
    if !x.is_finite() {
        return Err(ErrorCode::Num);
    }
    Ok(if abs(x) < 2.225_073_858_507_201_4e-308 { 0.0 } else { x })
}

/// `mean(xs)`: Excel's sum / n.
#[must_use]
pub fn mean(xs: &[f64]) -> f64 {
    xl_sum(xs) / xs.len() as f64
}

/// `devsq(xs)`: sum of squared deviations in two passes.
#[must_use]
pub fn devsq(xs: &[f64]) -> f64 {
    let m = mean(xs);
    let mut s = 0.0;
    for &x in xs {
        let d = x - m;
        s = xl_add(s, d * d);
    }
    s
}

/// `variance(xs, sample)`: the "textbook" formula unless it loses precision, then two passes (measured on ~900
/// sets).
#[must_use]
pub fn variance(xs: &[f64], sample: bool) -> f64 {
    let n = xs.len() as f64;
    let mut s1 = 0.0;
    let mut s2 = 0.0;
    for &x in xs {
        s1 = xl_add(s1, x);
        s2 = xl_add(s2, x * x);
    }
    let m = s1 / n;
    let numer = s2 - s1 * s1 / n;
    let tb = if sample { numer / (n - 1.0) } else { (n * s2 - s1 * s1) / (n * n) };
    if numer / s2 > 0.01 && tb / m >= 1e-3 {
        return tb;
    }
    let mut ss = 0.0;
    for &x in xs {
        ss = xl_add(ss, pow(x - m, 2.0));
    }
    ss / if sample { n - 1.0 } else { n }
}

/// Two-pass standard deviation (base of SKEW/KURT).
fn sd_two_pass(xs: &[f64], m: f64, ddof: f64) -> f64 {
    let mut ss = 0.0;
    for &x in xs {
        ss = xl_add(ss, pow(x - m, 2.0));
    }
    sqrt(ss / (xs.len() as f64 - ddof))
}

/// `skew(xs)` (sample): `n/((n−1)(n−2)) · Σz³`, `z·z·z`.
pub fn skew(xs: &[f64]) -> Result<f64, ErrorCode> {
    let n = xs.len() as f64;
    if n < 3.0 {
        return Err(ErrorCode::Div0);
    }
    let m = mean(xs);
    let s = sd_two_pass(xs, m, 1.0);
    if s == 0.0 {
        return Err(ErrorCode::Div0);
    }
    let mut t = 0.0;
    for &x in xs {
        let z = (x - m) / s;
        t = xl_add(t, z * z * z);
    }
    stat_num(n / ((n - 1.0) * (n - 2.0)) * t)
}

/// `skewP(xs)`: `Σz³/n` with the population deviation.
pub fn skew_p(xs: &[f64]) -> Result<f64, ErrorCode> {
    let n = xs.len() as f64;
    if n < 3.0 {
        return Err(ErrorCode::Div0);
    }
    let m = mean(xs);
    let s = sd_two_pass(xs, m, 0.0);
    if s == 0.0 {
        return Err(ErrorCode::Div0);
    }
    let mut t = 0.0;
    for &x in xs {
        let z = (x - m) / s;
        t = xl_add(t, z * z * z);
    }
    stat_num(t / n)
}

/// `kurt(xs)`: `n(n+1)/((n−1)(n−2)(n−3)) · Σz⁴ − 3(n−1)²/((n−2)(n−3))`, `z⁴ = (z·z)·(z·z)`.
pub fn kurt(xs: &[f64]) -> Result<f64, ErrorCode> {
    let n = xs.len() as f64;
    if n < 4.0 {
        return Err(ErrorCode::Div0);
    }
    let m = mean(xs);
    let s = sd_two_pass(xs, m, 1.0);
    if s == 0.0 {
        return Err(ErrorCode::Div0);
    }
    let mut t = 0.0;
    for &x in xs {
        let z = (x - m) / s;
        let z2 = z * z;
        t = xl_add(t, z2 * z2);
    }
    stat_num(n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * t - 3.0 * pow(n - 1.0, 2.0) / ((n - 2.0) * (n - 3.0)))
}

/// `Float64Array.prototype.sort` order: numeric, `-0` before `+0`, NaN last.
fn typed_cmp(a: &f64, b: &f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.total_cmp(b),
    }
}

/// `sorted(xs)`: an ascending copy (`Float64Array.from(xs).sort()`).
#[must_use]
pub fn sorted(xs: &[f64]) -> Vec<f64> {
    let mut v = xs.to_vec();
    v.sort_by(typed_cmp);
    v
}

/// Sorts in place with the `sorted` order (no copy, for callers that own the vector).
pub fn sort_in_place(xs: &mut [f64]) {
    xs.sort_by(typed_cmp);
}

// ---------------------------------------------------------------- collecting numbers

/// What a text/boolean value counts as in a collection (`CollectOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Take {
    /// Ignored (the default).
    #[default]
    Skip,
    /// Text as 0, boolean as 1/0 (the `*A` versions).
    Count,
}

/// A direct text argument (`directText`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DirectText {
    /// Converts; non-numeric text → `#VALUE!` (default).
    #[default]
    Convert,
    /// Converts; non-numeric text is skipped (COUNT).
    Skip,
    /// Any text is ignored, even one that looks like a number (MODE).
    Ignore,
}

/// `CollectOptions` (`stats.ts:65-85`) without `skipCell`, which the walker applies before calling the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Collect {
    /// Text in a reference.
    pub ref_text: Take,
    /// Boolean in a reference.
    pub ref_bool: Take,
    /// Text in an array.
    pub arr_text: Take,
    /// Boolean in an array.
    pub arr_bool: Take,
    /// Direct text.
    pub direct_text: DirectText,
    /// Direct boolean: counted 1/0 (default) or ignored (`Take::Skip` here means ignored).
    pub direct_bool_skip: bool,
    /// Errors are skipped instead of propagated (COUNT, AGGREGATE 2/3/6/7).
    pub skip_errors: bool,
}

impl Collect {
    /// A cell of a reference argument (`collectInto`, reference branch). `Err` stops the collection.
    pub fn ref_cell(&self, out: &mut Vec<f64>, v: ScalarView<'_>) -> Result<(), ErrorCode> {
        match v {
            ScalarView::Num(n) => out.push(n),
            ScalarView::Err(e) if !self.skip_errors => return Err(e),
            ScalarView::Str(_) if self.ref_text == Take::Count => out.push(0.0),
            ScalarView::Bool(b) if self.ref_bool == Take::Count => out.push(if b { 1.0 } else { 0.0 }),
            _ => {}
        }
        Ok(())
    }

    /// An element of an array argument.
    pub fn array_elem(&self, out: &mut Vec<f64>, v: ScalarView<'_>) -> Result<(), ErrorCode> {
        match v {
            ScalarView::Num(n) => out.push(n),
            ScalarView::Err(e) if !self.skip_errors => return Err(e),
            ScalarView::Str(_) if self.arr_text == Take::Count => out.push(0.0),
            ScalarView::Bool(b) if self.arr_bool == Take::Count => out.push(if b { 1.0 } else { 0.0 }),
            _ => {}
        }
        Ok(())
    }

    /// A direct scalar argument (`null` = an empty argument → 0).
    pub fn direct(&self, out: &mut Vec<f64>, v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Result<(), ErrorCode> {
        match v {
            ScalarView::Empty => out.push(0.0),
            ScalarView::Num(n) => out.push(n),
            ScalarView::Bool(b) => {
                if !self.direct_bool_skip {
                    out.push(if b { 1.0 } else { 0.0 });
                }
            }
            ScalarView::Err(e) => {
                if !self.skip_errors {
                    return Err(e);
                }
            }
            ScalarView::Str(s) => {
                if self.direct_text == DirectText::Ignore {
                    return Ok(());
                }
                match parse_number_text(s, locale, false, year) {
                    Some(n) => out.push(n),
                    None if self.direct_text != DirectText::Skip => return Err(ErrorCode::Value),
                    None => {}
                }
            }
        }
        Ok(())
    }
}

/// `areaSize(a)`: cells of an area.
#[must_use]
pub fn area_size(a: &Area) -> u64 {
    (u64::from(a.r2) - u64::from(a.r1) + 1) * (u64::from(a.c2) - u64::from(a.c1) + 1)
}

/// `BIG_AREA`: a larger area (whole column/row) is walked over its existing cells only.
pub const BIG_AREA: u64 = 65_536;

/// The sparse pairing of `zipCells` for two big areas of the same size: the existing (non-empty) cells of each side
/// as `(offset in the area, value)`, merged by offset ascending (the TS sorts the union of the keys); a position
/// present on one side only pairs with `null`. `visit` returning `Err` stops.
pub fn zip_sparse<T: Copy, E>(
    xs: &mut [(u64, T)],
    ys: &mut [(u64, T)],
    empty: T,
    mut visit: impl FnMut(T, T) -> Result<(), E>,
) -> Result<(), E> {
    xs.sort_by_key(|p| p.0);
    ys.sort_by_key(|p| p.0);
    let (mut i, mut j) = (0, 0);
    while i < xs.len() || j < ys.len() {
        match (xs.get(i), ys.get(j)) {
            (Some(&(kx, vx)), Some(&(ky, vy))) => match kx.cmp(&ky) {
                Ordering::Less => {
                    visit(vx, empty)?;
                    i += 1;
                }
                Ordering::Greater => {
                    visit(empty, vy)?;
                    j += 1;
                }
                Ordering::Equal => {
                    visit(vx, vy)?;
                    i += 1;
                    j += 1;
                }
            },
            (Some(&(_, vx)), None) => {
                visit(vx, empty)?;
                i += 1;
            }
            (None, Some(&(_, vy))) => {
                visit(empty, vy)?;
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(())
}

/// `argNumber(v)`: the number of a `value` argument already free of errors upstream: text converted in the locale,
/// boolean 1/0, empty 0; a non-finite number is `#NUM!`.
pub fn arg_number(v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Result<f64, ErrorCode> {
    match v {
        ScalarView::Num(n) => {
            if n.is_finite() {
                Ok(n)
            } else {
                Err(ErrorCode::Num)
            }
        }
        ScalarView::Empty => Ok(0.0),
        ScalarView::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        ScalarView::Err(e) => Err(e),
        ScalarView::Str(s) => parse_number_text(s, locale, false, year).ok_or(ErrorCode::Value),
    }
}

/// `argInt(v)`: truncated integer argument.
pub fn arg_int(v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Result<f64, ErrorCode> {
    arg_number(v, locale, year).map(trunc)
}

/// `argBool(v)`: number ≠ 0, boolean, `TRUE`/`FALSE` text (canonical or the locale's), or a text-number ≠ 0.
pub fn arg_bool(v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Result<bool, ErrorCode> {
    match v {
        ScalarView::Bool(b) => Ok(b),
        ScalarView::Num(n) => Ok(n != 0.0),
        ScalarView::Empty => Ok(false),
        ScalarView::Err(e) => Err(e),
        ScalarView::Str(s) => {
            if let Some(b) = with_u16(s, |u| bool_name(&upper(js_trim(u)), locale)) {
                return Ok(b);
            }
            parse_number_text(s, locale, false, year).map(|n| n != 0.0).ok_or(ErrorCode::Value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums() {
        assert_eq!(xl_add(-1.0, 1.0 - 8.0 * f64::EPSILON / 2.0), 0.0);
        assert_eq!(xl_sum(&[0.1, 0.2, -0.3]), 0.0);
        assert_eq!(sorted(&[3.0, -0.0, 0.0, f64::NAN, -1.0])[..4], [-1.0, -0.0, 0.0, 3.0]);
    }
}
