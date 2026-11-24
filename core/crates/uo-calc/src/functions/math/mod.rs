//! Family `math` (frontier A4a): `functions/math.ts` (aggregations SUM/PRODUCT/SUMSQ/SUMPRODUCT, rounding, MOD,
//! POWER, logarithms, trigonometry, factorials and combinations, GCD/LCM, bases and Roman numerals, matrices,
//! RAND/RANDBETWEEN), `complex.ts` ([`complex`]: COMPLEX, IM*) and `engineering.ts` ([`engineering`]: bases,
//! bits, CONVERT, DELTA/GESTEP, ERF*, BESSEL*). The numeric support is [`crate::math_util`] and [`crate::libm`]
//! (A1) and the walkers [`super::shared::mathu`].
//!
//! Every body is the TS one in the same order of floating-point operations; the record (`uo-calc.md` "A4a — as
//! built") lists where a TS loop is bounded (decision A4a-3) or a JS exception is reproduced as a throw.

mod complex;
mod engineering;

use uo_jsnum::math::{abs, ceil, floor, rem, sqrt, trunc};

use super::shared::mathu::{NumSpec, Raw, as_matrix, atp_fn, atp_list, atp_num, atp_raw, each_number, num_fn};
use super::shared::{R, Stop, arg, ascii, bodies, err, len, num, num_or_err, scalar, to_num, view};
use crate::coerce::to_text;
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::libm::{cr_atan, cr_expm1, cr_log, cr_log10, xl_exp};
use crate::math_util::{RoundMode, TRIG_LIMIT, approx15, cos_ext, decimal_round, excel_power, fin, sin_ext, snap_add, tan_ext};
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

use core::f64::consts::PI;

/// `MAX_SAFE` of `math.ts` (2^53).
const MAX_SAFE: f64 = 9_007_199_254_740_992.0;

/// The bodies of `math.ts`.
const MATH: &[(&str, FnBody)] = bodies![
    "ROUNDBAHTDOWN" => baht,
    "ROUNDBAHTUP" => baht,
    "SUM" => sum,
    "PRODUCT" => product,
    "SUMSQ" => sumsq,
    "SUMPRODUCT" => sumproduct,
    "SUMX2MY2" => sumx2my2,
    "SUMX2PY2" => sumx2py2,
    "SUMXMY2" => sumxmy2,
    "ROUND" => round,
    "ROUNDUP" => roundup,
    "ROUNDDOWN" => rounddown,
    "TRUNC" => trunc_fn,
    "INT" => int,
    "CEILING" => ceiling,
    "ECMA.CEILING" => ceiling,
    "FLOOR" => floor_fn,
    "CEILING.MATH" => ceiling_math,
    "FLOOR.MATH" => floor_math,
    "CEILING.PRECISE" => ceiling_precise,
    "ISO.CEILING" => ceiling_precise,
    "FLOOR.PRECISE" => floor_precise,
    "MROUND" => mround,
    "EVEN" => even,
    "ODD" => odd,
    "MOD" => mod_fn,
    "POWER" => power,
    "QUOTIENT" => quotient,
    "SQRT" => sqrt_fn,
    "SQRTPI" => sqrtpi,
    "EXP" => exp,
    "LN" => ln,
    "LOG10" => log10,
    "LOG" => log,
    "ABS" => abs_fn,
    "SIGN" => sign,
    "FACT" => fact,
    "FACTDOUBLE" => factdouble,
    "COMBIN" => combin,
    "COMBINA" => combina,
    "GCD" => gcd,
    "LCM" => lcm,
    "MULTINOMIAL" => multinomial,
    "SERIESSUM" => seriessum,
    "PI" => pi,
    "RAND" => rand,
    "RANDBETWEEN" => randbetween,
    "SIN" => sin,
    "COS" => cos,
    "TAN" => tan,
    "COT" => cot,
    "SEC" => sec,
    "CSC" => csc,
    "ASIN" => asin,
    "ACOS" => acos,
    "ATAN" => atan,
    "ATAN2" => atan2,
    "ACOT" => acot,
    "SINH" => sinh,
    "COSH" => cosh,
    "TANH" => tanh,
    "COTH" => coth,
    "SECH" => sech,
    "CSCH" => csch,
    "ASINH" => asinh,
    "ACOSH" => acosh,
    "ATANH" => atanh,
    "ACOTH" => acoth,
    "RADIANS" => radians,
    "DEGREES" => degrees,
    "BASE" => base,
    "DECIMAL" => decimal,
    "ROMAN" => roman,
    "ARABIC" => arabic,
    "MMULT" => mmult,
    "MDETERM" => mdeterm,
    "MINVERSE" => minverse,
    "MUNIT" => munit,
];

/// The bodies of the family: `math.ts`, `complex.ts`, `engineering.ts`.
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[MATH, complex::BODIES, engineering::BODIES];

/// A numeric result as a value (`Num` → value, no `fin`).
#[inline]
fn n(r: Result<f64, ErrorCode>) -> R<Value> {
    Ok(num_or_err(r))
}

/// `fin(x)` as a value.
#[inline]
fn f(x: f64) -> R<Value> {
    Ok(num_or_err(fin(x)))
}

// ---------------------------------------------------------------- aggregations

fn baht(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(err(ErrorCode::Value))
}

fn sum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut s, mut prev, mut last) = (0.0, 0.0, 0.0);
    each_number(a, ctx, &mut |x| {
        prev = s;
        last = x;
        s += x;
    })?;
    f(snap_add(prev, last))
}

fn product(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut p, mut any) = (1.0, false);
    each_number(a, ctx, &mut |x| {
        p *= x;
        any = true;
    })?;
    if any { f(p) } else { Ok(num(0.0)) }
}

fn sumsq(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut s = 0.0;
    each_number(a, ctx, &mut |x| s += x * x)?;
    f(s)
}

fn sumproduct(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // an error in any matrix propagates before the size check (matrix by matrix)
    let mut ms: Vec<ArrayValue> = Vec::with_capacity(len(a));
    for i in 0..len(a) {
        let v = arg(a, i).unwrap_or(&super::shared::EMPTY);
        if let Value::Scalar(Scalar::Err(e)) = v {
            return Err(Stop::Err(*e));
        }
        let m = as_matrix(v, ctx)?;
        if let Some(Scalar::Err(e)) = m.data.iter().find(|x| matches!(x, Scalar::Err(_))) {
            return Err(Stop::Err(*e));
        }
        ms.push(m);
    }
    let (r, c) = (ms[0].rows, ms[0].cols);
    if ms.iter().any(|m| m.rows != r || m.cols != c) {
        return Ok(err(ErrorCode::Value));
    }
    let n = ms[0].data.len();
    let mut s = 0.0;
    for i in 0..n {
        let mut p = 1.0;
        for m in &ms {
            match m.data.get(i) {
                Some(Scalar::Num(v)) => p *= v,
                _ => {
                    p = 0.0;
                    break;
                }
            }
        }
        s += p;
    }
    f(s)
}

/// `pairSum`: pairs where both sides are numbers; no numeric pair = `#DIV/0!`.
fn pair_sum(a: &CallArgs<'_>, ctx: &mut dyn FnContext, g: fn(f64, f64) -> f64) -> R<Value> {
    let va = arg(a, 0).unwrap_or(&super::shared::EMPTY);
    let vb = arg(a, 1).unwrap_or(&super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = va {
        return Ok(err(*e));
    }
    if let Value::Scalar(Scalar::Err(e)) = vb {
        return Ok(err(*e));
    }
    let ma = as_matrix(va, ctx);
    let mb = as_matrix(vb, ctx);
    let ma = ma?;
    let mb = mb?;
    if ma.data.len() != mb.data.len() {
        return Ok(err(ErrorCode::NA));
    }
    for v in ma.data.iter().chain(mb.data.iter()) {
        if let Scalar::Err(e) = v {
            return Ok(err(*e));
        }
    }
    let (mut s, mut cnt) = (0.0, 0u32);
    for (x, y) in ma.data.iter().zip(mb.data.iter()) {
        if let (Scalar::Num(x), Scalar::Num(y)) = (x, y) {
            s += g(*x, *y);
            cnt += 1;
        }
    }
    if cnt > 0 { f(s) } else { Ok(err(ErrorCode::Div0)) }
}

fn sumx2my2(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_sum(a, ctx, |x, y| x * x - y * y)
}

fn sumx2py2(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_sum(a, ctx, |x, y| x * x + y * y)
}

fn sumxmy2(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    pair_sum(a, ctx, |x, y| (x - y) * (x - y))
}

// ---------------------------------------------------------------- rounding

/// `roundFn(name, mode)`.
fn round_by(a: &CallArgs<'_>, ctx: &mut dyn FnContext, mode: RoundMode, s: &NumSpec) -> R<Value> {
    num_fn(a, ctx, s, |xs, _| {
        let dd = trunc(xs.at(1));
        if mode == RoundMode::Half && dd >= 2_147_483_647.0 {
            return Ok(num(0.0)); // measured: ROUND(1,5;2^31−1) = 0
        }
        f(decimal_round(xs.at(0), uo_jsnum::math::max(-400.0, uo_jsnum::math::min(400.0, dd)), mode))
    })
}

fn round(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    round_by(a, ctx, RoundMode::Half, &NumSpec::new(2, 2))
}

fn roundup(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    round_by(a, ctx, RoundMode::Up, &NumSpec::new(2, 2))
}

fn rounddown(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    round_by(a, ctx, RoundMode::Down, &NumSpec::new(2, 2))
}

fn trunc_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    round_by(a, ctx, RoundMode::Down, &NumSpec::new(1, 2).defaults(&[0.0]))
}

/// `excelInt(x)`: ⌊x⌋ of the 15-digit number (up to 1E15; above, ⌊x⌋ itself).
#[must_use]
pub fn excel_int(x: f64) -> f64 {
    floor(if abs(x) < 1e15 { approx15(x) } else { x })
}

fn int(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(excel_int(xs.at(0)))))
}

/// `stepRound(x, sig, up)`: the multiple of `sig` by ⌈q⌉ or ⌊q⌋ of the 15-digit quotient.
fn step_round(x: f64, sig: f64, up: bool) -> R<Value> {
    let q = x / sig;
    if !q.is_finite() {
        return Ok(err(ErrorCode::Num));
    }
    let a = approx15(q);
    f((if up { ceil(a) } else { floor(a) }) * sig)
}

fn ceiling(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, s) = (xs.at(0), xs.at(1));
        if s == 0.0 || x == 0.0 {
            return Ok(num(0.0));
        }
        if x > 0.0 && s < 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        step_round(x, s, true)
    })
}

fn floor_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, s) = (xs.at(0), xs.at(1));
        if x == 0.0 {
            return Ok(num(0.0));
        }
        if s == 0.0 {
            return Ok(err(ErrorCode::Div0));
        }
        if x > 0.0 && s < 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        step_round(x, s, false)
    })
}

const MATH_SPEC: NumSpec = NumSpec::new(1, 3).defaults(&[1.0, 0.0]).empty_default();

fn ceiling_math(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &MATH_SPEC, |xs, _| {
        let (x, sig, mode) = (xs.at(0), abs(xs.at(1)), xs.at(2));
        if sig == 0.0 || x == 0.0 {
            return Ok(num(0.0));
        }
        step_round(x, sig, !(x < 0.0 && mode != 0.0))
    })
}

fn floor_math(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &MATH_SPEC, |xs, _| {
        let (x, sig, mode) = (xs.at(0), abs(xs.at(1)), xs.at(2));
        if sig == 0.0 || x == 0.0 {
            return Ok(num(0.0));
        }
        step_round(x, sig, x < 0.0 && mode != 0.0)
    })
}

/// `precise(name, up)`.
fn precise(a: &CallArgs<'_>, ctx: &mut dyn FnContext, up: bool) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 2).defaults(&[1.0]).empty_default(), |xs, _| {
        let (x, sig) = (xs.at(0), abs(xs.at(1)));
        if sig == 0.0 || x == 0.0 {
            return Ok(num(0.0));
        }
        step_round(x, sig, up)
    })
}

fn ceiling_precise(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    precise(a, ctx, true)
}

fn floor_precise(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    precise(a, ctx, false)
}

fn mround(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, m) = (xs.at(0), xs.at(1));
        if m == 0.0 || x == 0.0 {
            return Ok(num(0.0));
        }
        if (x > 0.0) != (m > 0.0) {
            return Ok(err(ErrorCode::Num));
        }
        let q = x / m;
        if !q.is_finite() {
            return Ok(err(ErrorCode::Num));
        }
        let mut k = floor(q);
        if q - k >= 0.5 - 5e-15 {
            k += 1.0;
        }
        f(k * m)
    })
}

fn even(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        let mut k = ceil(abs(x));
        if truthy(rem(k, 2.0)) {
            k += 1.0;
        }
        Ok(num(if x < 0.0 { -k } else { k }))
    })
}

fn odd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        let mut k = ceil(abs(x));
        if rem(k, 2.0) == 0.0 {
            k += 1.0;
        }
        Ok(num(if x < 0.0 { -k } else { k }))
    })
}

/// JS truthiness of a number (`0`, `-0` and NaN are false).
#[inline]
fn truthy(x: f64) -> bool {
    uo_jsnum::conv::truthy(x)
}

// ---------------------------------------------------------------- arithmetic

fn mod_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, d) = (xs.at(0), xs.at(1));
        if d == 0.0 {
            return Ok(err(ErrorCode::Div0));
        }
        if abs(x / d) >= 1.1259e12 {
            return Ok(err(ErrorCode::Num));
        }
        let mut r = rem(x, d);
        if r != 0.0 && (r < 0.0) != (d < 0.0) {
            r += d;
            if r == d {
                r = 0.0;
            }
        }
        Ok(num(r))
    })
}

fn power(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| n(excel_power(xs.at(0), xs.at(1))))
}

fn quotient(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        if xs.at(1) == 0.0 {
            return Ok(err(ErrorCode::Div0));
        }
        f(trunc(xs.at(0) / xs.at(1)))
    })
}

fn sqrt_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if x < 0.0 { err(ErrorCode::Num) } else { num(sqrt(x)) })
    })
}

fn sqrtpi(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if x < 0.0 { err(ErrorCode::Num) } else { num(sqrt(x * PI)) })
    })
}

fn exp(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| f(xl_exp(xs.at(0))))
}

fn ln(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if x <= 0.0 { err(ErrorCode::Num) } else { num(cr_log(x)) })
    })
}

fn log10(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if x <= 0.0 { err(ErrorCode::Num) } else { num(cr_log10(x)) })
    })
}

fn log(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 2), |xs, _| {
        let x = xs.at(0);
        if x <= 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        let Some(b) = xs.get(1) else { return Ok(num(cr_log10(x))) };
        if b <= 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        if b == 1.0 {
            return Ok(err(ErrorCode::Div0));
        }
        Ok(num(cr_log(x) / cr_log(b)))
    })
}

fn abs_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(abs(xs.at(0)))))
}

fn sign(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(num(if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }))
    })
}

// ---------------------------------------------------------------- factorials and combinations

/// `factorial(n)`: a decreasing product (bit-exact with Excel); > 170 = `#NUM!`.
pub fn factorial(n: f64) -> Result<f64, ErrorCode> {
    if n < 0.0 {
        return Err(ErrorCode::Num);
    }
    let n = trunc(n);
    if n > 170.0 {
        return Err(ErrorCode::Num);
    }
    let mut p = 1.0;
    let mut i = n;
    while i > 1.0 {
        p *= i;
        i -= 1.0;
    }
    Ok(p)
}

fn fact(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| n(factorial(xs.at(0))))
}

fn factdouble(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if x < -1.0 {
            return Ok(err(ErrorCode::Num));
        }
        let k = trunc(x);
        if k > 400.0 {
            return Ok(err(ErrorCode::Num));
        }
        let mut p = 1.0;
        let mut i = k;
        while i > 1.0 {
            p *= i;
            i -= 2.0;
        }
        f(p)
    })
}

/// `binomial(n, k)` with `n`, `k` valid integers: the TS product. The loop stops once the product is not finite:
/// every factor is ≥ 1 (k ≤ n − k after the swap), so ∞ and NaN are absorbing and the result is the one the full
/// loop gives (decision A4a-3; `COMBINA(n; ∞)` never returns in the TS).
#[must_use]
pub fn binomial(n: f64, k: f64) -> f64 {
    let k = if k > n - k { n - k } else { k };
    let mut r = 1.0;
    let mut i = 1.0;
    while i <= k {
        r *= (n - k + i) / i;
        if !r.is_finite() {
            break;
        }
        i += 1.0;
    }
    r
}

fn combin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (nn, k) = (trunc(xs.at(0)), trunc(xs.at(1)));
        if nn < 0.0 || k < 0.0 || k > nn {
            return Ok(err(ErrorCode::Num));
        }
        if nn >= 2_147_483_647.0 {
            return Ok(err(ErrorCode::Num));
        }
        f(binomial(nn, k))
    })
}

fn combina(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (nn, k) = (trunc(xs.at(0)), trunc(xs.at(1)));
        if nn < 0.0 || k < 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        if nn == 0.0 && k == 0.0 {
            return Ok(num(1.0));
        }
        if k > nn + k - 1.0 {
            return Ok(err(ErrorCode::Num));
        }
        f(binomial(nn + k - 1.0, k))
    })
}

/// Euclid on doubles (`while (b)`: NaN and 0 stop).
fn gcd2(mut a: f64, mut b: f64) -> f64 {
    while truthy(b) {
        let t = rem(a, b);
        a = b;
        b = t;
    }
    a
}

fn gcd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = Vec::new();
    atp_list(a, ctx, &mut xs)?;
    let mut g = 0.0;
    for &x in &xs {
        if x < 0.0 || x > MAX_SAFE {
            return Ok(err(ErrorCode::Num));
        }
        g = gcd2(g, trunc(x));
    }
    Ok(num(g))
}

fn lcm(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = Vec::new();
    atp_list(a, ctx, &mut xs)?;
    let mut l = 1.0;
    for &x in &xs {
        if x < 0.0 || x > MAX_SAFE {
            return Ok(err(ErrorCode::Num));
        }
        let k = trunc(x);
        if k == 0.0 {
            return Ok(num(0.0));
        }
        l = (l / gcd2(l, k)) * k;
        if l > MAX_SAFE {
            return Ok(err(ErrorCode::Num));
        }
    }
    Ok(num(l))
}

fn multinomial(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = Vec::new();
    atp_list(a, ctx, &mut xs)?;
    let (mut s, mut r) = (0.0, 1.0);
    for &x in &xs {
        if x < 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        let k = trunc(x);
        if s + k > 170.0 {
            return Ok(err(ErrorCode::Num));
        }
        let mut i = 1.0;
        while i <= k {
            s += 1.0;
            r = (r * s) / i;
            i += 1.0;
        }
    }
    if s > 170.0 {
        return Ok(err(ErrorCode::Num));
    }
    f(r)
}

fn seriessum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = [0.0; 3];
    for (i, slot) in xs.iter_mut().enumerate() {
        let r = atp_raw(arg(a, i), ctx)?;
        let Raw::S(v) = r else { return Ok(err(ErrorCode::Value)) };
        *slot = atp_num(ctx, v)?;
    }
    let mut cs: Vec<f64> = Vec::new();
    match arg(a, 3) {
        None | Some(Value::Scalar(Scalar::Empty)) => return Ok(err(ErrorCode::NA)),
        Some(Value::Ref(r)) => {
            for &area in r.areas() {
                let mut e: Option<ErrorCode> = None;
                ctx.for_each_cell(area, &mut |v, _, _| {
                    if e.is_some() || matches!(v, Scalar::Empty) {
                        return;
                    }
                    match v {
                        Scalar::Num(x) => cs.push(x),
                        Scalar::Err(x) => e = Some(x),
                        _ => e = Some(ErrorCode::Value),
                    }
                })?;
                if let Some(e) = e {
                    return Ok(err(e));
                }
            }
        }
        Some(Value::Array(x)) => {
            for v in x.data.iter() {
                match v {
                    Scalar::Err(e) => return Ok(err(*e)),
                    Scalar::Num(x) => cs.push(*x),
                    _ => return Ok(err(ErrorCode::Value)),
                }
            }
        }
        // a LAMBDA cannot be converted (a JS object): `#VALUE!` (declared, A4a-5)
        Some(Value::Lambda(_)) => return Ok(err(ErrorCode::Value)),
        Some(Value::Scalar(s)) => cs.push(atp_num(ctx, *s)?),
    }
    let [x, nn, step] = xs;
    let mut s = 0.0;
    for (i, &c) in cs.iter().enumerate() {
        let e = nn + i as f64 * step;
        if x == 0.0 && e <= 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        let p = uo_jsnum::math::pow(x, e);
        if p.is_nan() {
            continue;
        }
        s += c * p;
    }
    f(s)
}

// ---------------------------------------------------------------- constants and random numbers

fn pi(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(num(PI))
}

fn rand(_: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    Ok(num(ctx.random()))
}

fn randbetween(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |xs, ctx| {
        let (lo, hi) = (ceil(xs.at(0)), ceil(xs.at(1)));
        if lo > hi {
            return Ok(err(ErrorCode::Num));
        }
        Ok(num(lo + floor(ctx.random() * (hi - lo + 1.0))))
    })
}

// ---------------------------------------------------------------- trigonometry

/// `trig(name, f)`: |x| ≥ 2^27 = `#NUM!`.
fn trig(a: &CallArgs<'_>, ctx: &mut dyn FnContext, g: fn(f64) -> R<Value>) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if abs(x) >= TRIG_LIMIT {
            return Ok(err(ErrorCode::Num));
        }
        g(x)
    })
}

fn sin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| Ok(num(sin_ext(x))))
}

fn cos(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| Ok(num(cos_ext(x))))
}

fn tan(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| Ok(num(tan_ext(x))))
}

fn cot(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| if x == 0.0 { Ok(err(ErrorCode::Div0)) } else { f(1.0 / tan_ext(x)) })
}

fn sec(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| f(1.0 / cos_ext(x)))
}

fn csc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    trig(a, ctx, |x| if x == 0.0 { Ok(err(ErrorCode::Div0)) } else { f(1.0 / sin_ext(x)) })
}

/// `asinX(x)` = `atan(x / √((1−x)(1+x)))` with the correct `atan`.
fn asin_x(x: f64) -> f64 {
    cr_atan(x / sqrt((1.0 - x) * (1.0 + x)))
}

fn asin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if abs(x) > 1.0 { err(ErrorCode::Num) } else { num(asin_x(x)) })
    })
}

fn acos(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(if abs(x) > 1.0 { err(ErrorCode::Num) } else { num(PI / 2.0 - asin_x(x)) })
    })
}

fn atan(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(cr_atan(xs.at(0)))))
}

fn atan2(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, y) = (xs.at(0), xs.at(1));
        if x == 0.0 {
            return Ok(if y == 0.0 {
                err(ErrorCode::Div0)
            } else if y > 0.0 {
                num(PI / 2.0)
            } else {
                num(-PI / 2.0)
            });
        }
        let q = y / x;
        if !q.is_finite() {
            return Ok(err(ErrorCode::Num));
        }
        let t = cr_atan(q);
        Ok(num(if x > 0.0 {
            t
        } else if y >= 0.0 {
            t + PI
        } else {
            t - PI
        }))
    })
}

fn acot(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        Ok(num(if x == 0.0 {
            PI / 2.0
        } else if x > 0.0 {
            cr_atan(1.0 / x)
        } else {
            PI + cr_atan(1.0 / x)
        }))
    })
}

/// `EXP_MAX` of `math.ts`.
const EXP_MAX: f64 = 709.782_712_893_384;

fn cosh_x(x: f64) -> f64 {
    (xl_exp(x) + xl_exp(-x)) / 2.0
}

fn sinh_x(x: f64) -> f64 {
    if abs(x) >= 1.0 { (xl_exp(x) - xl_exp(-x)) / 2.0 } else { (cr_expm1(x) - cr_expm1(-x)) / 2.0 }
}

fn tanh_x(x: f64) -> f64 {
    if abs(x) > EXP_MAX {
        return if x > 0.0 { 1.0 } else { -1.0 };
    }
    if abs(x) >= 1.0 {
        let (p, q) = (xl_exp(x), xl_exp(-x));
        return (p - q) / (p + q);
    }
    sinh_x(x) / cosh_x(x)
}

fn sinh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if abs(x) > EXP_MAX { Ok(err(ErrorCode::Num)) } else { f(sinh_x(x)) }
    })
}

fn cosh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if abs(x) > EXP_MAX { Ok(err(ErrorCode::Num)) } else { f(cosh_x(x)) }
    })
}

fn tanh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(tanh_x(xs.at(0)))))
}

fn coth(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if x == 0.0 { Ok(err(ErrorCode::Div0)) } else { f(1.0 / tanh_x(x)) }
    })
}

fn sech(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if abs(x) > EXP_MAX { Ok(num(0.0)) } else { f(1.0 / cosh_x(x)) }
    })
}

fn csch(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if x == 0.0 {
            Ok(err(ErrorCode::Div0))
        } else if abs(x) > EXP_MAX {
            Ok(num(0.0))
        } else {
            f(1.0 / sinh_x(x))
        }
    })
}

fn asinh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        let ax = abs(x);
        let v = cr_log(ax + sqrt(ax * ax + 1.0));
        f(if x < 0.0 { -v } else { v })
    })
}

fn acosh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        if x < 1.0 { Ok(err(ErrorCode::Num)) } else { f(cr_log(x + sqrt(x * x - 1.0))) }
    })
}

fn atanh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        let ax = abs(x);
        if ax >= 1.0 {
            return Ok(err(ErrorCode::Num));
        }
        Ok(num(if ax <= 1e-4 { uo_jsnum::math::atanh(x) } else { 0.5 * cr_log((1.0 + x) / (1.0 - x)) }))
    })
}

fn acoth(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let x = xs.at(0);
        let ax = abs(x);
        if ax <= 1.0 {
            return Ok(err(ErrorCode::Num));
        }
        let v = 0.5 * uo_jsnum::math::log1p(2.0 / (ax - 1.0));
        Ok(num(if x < 0.0 { -v } else { v }))
    })
}

fn radians(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| f(xs.at(0) * (PI / 180.0)))
}

fn degrees(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| f(xs.at(0) * (180.0 / PI)))
}

// ---------------------------------------------------------------- bases and Roman numerals

const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn base(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 3).defaults(&[0.0]), |xs, ctx| {
        let (nv, radix, min_len) = (xs.at(0), trunc(xs.at(1)), trunc(xs.at(2)));
        if nv < 0.0 || nv >= MAX_SAFE || radix < 2.0 || radix > 36.0 || min_len < 0.0 || min_len > 255.0 {
            return Ok(err(ErrorCode::Num));
        }
        // `do { s = DIGITS[v % radix] + s; v = floor(v / radix) } while (v > 0)`; a NaN digit index reads
        // `undefined` (BASE(NaN;2) = "undefined", as the TS)
        let mut v = trunc(nv);
        let mut rev: Vec<u8> = Vec::with_capacity(64);
        loop {
            let d = rem(v, radix);
            if d.is_nan() {
                rev.extend(b"undefined".iter().rev());
            } else {
                rev.push(DIGITS[crate::cast::trunc_usize(abs(d))]);
            }
            v = floor(v / radix);
            // `while (v > 0)`: NaN stops too
            if v.is_nan() || v <= 0.0 {
                break;
            }
        }
        let pad = if min_len.is_nan() { 0 } else { crate::cast::trunc_usize(min_len) };
        let mut s: Vec<u8> = Vec::with_capacity(pad.max(rev.len()));
        s.resize(pad.saturating_sub(rev.len()), b'0');
        s.extend(rev.iter().rev());
        ascii(ctx, &s)
    })
}

fn decimal(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = scalar(a, 0).unwrap_or(Scalar::Empty);
    let r = scalar(a, 1).unwrap_or(Scalar::Empty);
    let units: Vec<u16> = {
        let tx = to_text(view(ctx, t), ctx.locale())?;
        tx.as_js().to_utf16()
    };
    let rn = to_num(ctx, r)?;
    let radix = trunc(rn);
    if radix < 2.0 || radix > 36.0 || units.len() > 255 {
        return Ok(err(ErrorCode::Num));
    }
    // leading spaces are ignored (trailing ones are not); base 16 accepts a "0x"/"x" prefix
    let start = units.iter().take_while(|&&u| u == 0x20).count();
    let up = crate::jstext::upper(&units[start..]);
    let mut body: &[u16] = &up;
    if radix == 16.0 {
        if body.first() == Some(&u16::from(b'0')) && body.get(1) == Some(&u16::from(b'X')) {
            body = &body[2..];
        } else if body.first() == Some(&u16::from(b'X')) {
            body = &body[1..];
        }
    }
    let mut v = 0.0;
    for ch in char::decode_utf16(body.iter().copied()) {
        let d = match ch {
            Ok(c) if c.is_ascii() => DIGITS.iter().position(|&x| char::from(x) == c),
            _ => None,
        };
        let Some(d) = d else { return Ok(err(ErrorCode::Num)) };
        let d = d as f64;
        if d >= radix {
            return Ok(err(ErrorCode::Num));
        }
        v = v * radix + d;
    }
    Ok(num(v))
}

const RCH: [u8; 7] = *b"MDCLXVI";
const RVAL: [f64; 7] = [1000.0, 500.0, 100.0, 50.0, 10.0, 5.0, 1.0];

/// `RVAL[i]` with the JS out-of-range read (`undefined` → NaN in arithmetic).
fn rval(i: i64) -> f64 {
    usize::try_from(i).ok().and_then(|i| RVAL.get(i).copied()).unwrap_or(f64::NAN)
}

/// `RCH[i]` (`undefined` concatenates as the text "undefined").
fn rch(out: &mut Vec<u8>, i: i64) {
    match usize::try_from(i).ok().and_then(|i| RCH.get(i)) {
        Some(&c) => out.push(c),
        None => out.extend_from_slice(b"undefined"),
    }
}

/// `toRoman(value, mode)`: forms 0 (classic) to 4 (simplified), as Excel.
#[must_use]
pub fn to_roman(value: f64, mode: f64) -> Vec<u8> {
    let mut val = value;
    let mut out = Vec::new();
    let max_index: i64 = 6;
    let mut i: i64 = 0;
    while (i as f64) <= max_index as f64 / 2.0 {
        let mut index = 2 * i;
        let digit = floor(val / rval(index));
        if rem(digit, 5.0) == 4.0 {
            let index2 = if digit == 4.0 { index - 1 } else { index - 2 };
            let mut steps = 0.0;
            while steps < mode && index < max_index {
                steps += 1.0;
                if rval(index2) - rval(index + 1) <= val {
                    index += 1;
                } else {
                    steps = mode;
                }
            }
            rch(&mut out, index);
            rch(&mut out, index2);
            val = val + rval(index) - rval(index2);
        } else {
            if digit > 4.0 {
                rch(&mut out, index - 1);
            }
            // `RCH[index].repeat(digit % 5)`: a NaN count repeats 0 times
            let reps = rem(digit, 5.0);
            if reps >= 1.0 {
                for _ in 0..crate::cast::trunc_usize(reps) {
                    rch(&mut out, index);
                }
            }
            val = rem(val, rval(index));
        }
        i += 1;
    }
    out
}

fn roman(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let nv = to_num(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?;
    let mut mode = 0.0;
    if len(a) > 1 {
        match scalar(a, 1).unwrap_or(Scalar::Empty) {
            Scalar::Bool(b) => mode = if b { 0.0 } else { 4.0 },
            Scalar::Empty => {}
            s => mode = trunc(to_num(ctx, s)?),
        }
    }
    let v = trunc(nv);
    if v < 0.0 || v > 3999.0 || mode < 0.0 || mode > 4.0 {
        return Ok(err(ErrorCode::Value));
    }
    let r = to_roman(v, mode);
    ascii(ctx, &r)
}

fn arabic(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = scalar(a, 0).unwrap_or(Scalar::Empty);
    let units: Vec<u16> = {
        let tx = to_text(view(ctx, t), ctx.locale())?;
        let u = tx.as_js().to_utf16();
        crate::jstext::upper(crate::jstext::js_trim(&u)).into_owned()
    };
    if units.len() > 255 {
        return Ok(err(ErrorCode::Value));
    }
    let mut s: &[u16] = &units;
    let mut neg = false;
    if s.first() == Some(&u16::from(b'-')) {
        neg = true;
        s = &s[1..];
    }
    // right to left, subtracting what is smaller than the largest seen so far (IIV = 3)
    let (mut total, mut max) = (0.0, 0.0);
    for &u in s.iter().rev() {
        let v = match u {
            0x49 => 1.0,
            0x56 => 5.0,
            0x58 => 10.0,
            0x4c => 50.0,
            0x43 => 100.0,
            0x44 => 500.0,
            0x4d => 1000.0,
            _ => return Ok(err(ErrorCode::Value)),
        };
        if v < max {
            total -= v;
        } else {
            total += v;
            max = v;
        }
    }
    Ok(num(if neg { -total } else { total }))
}

// ---------------------------------------------------------------- matrices

/// `numericMatrix(v, ctx)`: a numeric matrix (any non-number = `#VALUE!`; an error propagates).
fn numeric_matrix(v: &Value, ctx: &mut dyn FnContext) -> R<(u32, u32, Vec<f64>)> {
    let m = as_matrix(v, ctx)?;
    let mut out = Vec::with_capacity(m.data.len());
    for x in m.data.iter() {
        match x {
            Scalar::Err(e) => return Err(Stop::Err(*e)),
            Scalar::Num(n) => out.push(*n),
            _ => return Err(Stop::Err(ErrorCode::Value)),
        }
    }
    Ok((m.rows, m.cols, out))
}

fn mmult(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = arg(a, 0).unwrap_or(&super::shared::EMPTY);
    let y = arg(a, 1).unwrap_or(&super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = x {
        return Ok(err(*e));
    }
    if let Value::Scalar(Scalar::Err(e)) = y {
        return Ok(err(*e));
    }
    let ma = numeric_matrix(x, ctx);
    let mb = numeric_matrix(y, ctx);
    let (ar, ac, av) = ma?;
    let (br, bc, bv) = mb?;
    if ac != br {
        return Ok(err(ErrorCode::Value));
    }
    // the result past the array cap, or more than 2^30 multiply-adds (seconds of work): a RangeError (A4a-4)
    if crate::value::array_too_big(ar, bc) || u64::from(ar) * u64::from(ac) * u64::from(bc) > MAX_FLOPS {
        return Err(Stop::Throw(Throw::Range));
    }
    let (ac, bc) = (ac as usize, bc as usize);
    Ok(Value::Array(ArrayValue::fill(ar, bc as u32, |i, j| {
        let (i, j) = (i as usize, j as usize);
        let mut s = 0.0;
        for k in 0..ac {
            s += av[i * ac + k] * bv[k * bc + j];
        }
        match fin(s) {
            Ok(v) => Scalar::Num(v),
            Err(e) => Scalar::Err(e),
        }
    })))
}

/// The most multiply-adds a matrix function does (MMULT, MDETERM, MINVERSE: n^3 work on inputs the array cap
/// allows): past it a `RangeError` (`#NUM!`), where the TS computes for minutes (decision A4a-4).
const MAX_FLOPS: u64 = 1 << 30;

/// `luDecompose(a, n, perm)`: LU with partial pivoting in place; the sign of the permutation or 0 when singular.
fn lu_decompose(a: &mut [f64], n: usize, perm: &mut [usize]) -> f64 {
    let mut sgn = 1.0;
    for (i, p) in perm.iter_mut().enumerate() {
        *p = i;
    }
    for k in 0..n {
        let mut p = k;
        let mut best = abs(a[k * n + k]);
        for i in k + 1..n {
            let v = abs(a[i * n + k]);
            if v > best {
                best = v;
                p = i;
            }
        }
        if best == 0.0 {
            return 0.0;
        }
        if p != k {
            for j in 0..n {
                a.swap(k * n + j, p * n + j);
            }
            perm.swap(k, p);
            sgn = -sgn;
        }
        let piv = a[k * n + k];
        for i in k + 1..n {
            let fct = a[i * n + k] / piv;
            a[i * n + k] = fct;
            if fct != 0.0 {
                for j in k + 1..n {
                    a[i * n + j] -= fct * a[k * n + j];
                }
            }
        }
    }
    sgn
}

fn mdeterm(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = arg(a, 0).unwrap_or(&super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = x {
        return Ok(err(*e));
    }
    let (r, c, mut m) = numeric_matrix(x, ctx)?;
    if r != c {
        return Ok(err(ErrorCode::Value));
    }
    if u64::from(r).pow(3) > MAX_FLOPS {
        return Err(Stop::Throw(Throw::Range));
    }
    let n = r as usize;
    let mut perm = vec![0usize; n];
    let sgn = lu_decompose(&mut m, n, &mut perm);
    if sgn == 0.0 {
        return Ok(num(0.0));
    }
    let mut d = sgn;
    for i in 0..n {
        d *= m[i * n + i];
    }
    f(d)
}

fn minverse(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = arg(a, 0).unwrap_or(&super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = x {
        return Ok(err(*e));
    }
    let (r, c, mut m) = numeric_matrix(x, ctx)?;
    if r != c {
        return Ok(err(ErrorCode::Value));
    }
    if u64::from(r).pow(3) > MAX_FLOPS {
        return Err(Stop::Throw(Throw::Range));
    }
    let n = r as usize;
    let mut perm = vec![0usize; n];
    if lu_decompose(&mut m, n, &mut perm) == 0.0 {
        return Ok(err(ErrorCode::Num));
    }
    let mut inv = vec![0.0; n * n];
    let mut y = vec![0.0; n];
    for col in 0..n {
        // solves L·U·x = P·e_col
        for i in 0..n {
            let mut s = if perm[i] == col { 1.0 } else { 0.0 };
            for k in 0..i {
                s -= m[i * n + k] * y[k];
            }
            y[i] = s;
        }
        for i in (0..n).rev() {
            let mut s = y[i];
            for k in i + 1..n {
                s -= m[i * n + k] * inv[k * n + col];
            }
            inv[i * n + col] = s / m[i * n + i];
        }
    }
    if inv.iter().any(|v| !v.is_finite()) {
        return Ok(err(ErrorCode::Num));
    }
    Ok(Value::Array(ArrayValue::fill(r, r, |i, j| {
        let v = inv[i as usize * n + j as usize];
        Scalar::Num(if v == 0.0 { 0.0 } else { v })
    })))
}

/// The largest array a body builds (the engine's array cap, 2^24 cells): past it the TS either throws a
/// `RangeError` (more than 2^32 − 1 elements) or exhausts the heap; Rust throws the `RangeError` (`#NUM!` in the
/// engine) for all of them (decision A4a-4).
pub(crate) const MAX_CELLS: f64 = 16_777_216.0;

fn munit(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| {
        let k = trunc(xs.at(0));
        if k < 1.0 {
            return Ok(err(ErrorCode::Value));
        }
        if k > 1_048_576.0 {
            return Ok(err(ErrorCode::Value));
        }
        // `new Array(NaN)` is a RangeError; past the array cap too
        if k.is_nan() || k * k > MAX_CELLS {
            return Err(Stop::Throw(Throw::Range));
        }
        let k = crate::cast::to_u32(k);
        Ok(Value::Array(ArrayValue::fill(k, k, |i, j| Scalar::Num(if i == j { 1.0 } else { 0.0 }))))
    })
}
