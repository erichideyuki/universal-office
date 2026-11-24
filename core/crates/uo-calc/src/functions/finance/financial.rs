//! `functions/financial.ts`: time value of money (PV, FV, PMT, IPMT, PPMT, NPER, RATE…), cash flows (NPV, IRR,
//! MIRR, XNPV, XIRR, FVSCHEDULE), depreciation (SLN, SYD, DB, DDB, VDB, AMOR*), bonds (PRICE, YIELD, DURATION,
//! COUP*, discount securities, ACCRINT*, T-bills, ODDF*/ODDL*). The day counts and coupon dates are [`super::daycount`].
//!
//! Every body is the TS one in the same order of floating-point operations. Where the TS loop has no bound of its
//! own (a TS hang or minutes per cell), the loop carries a step budget and answers `#NUM!` past it (decision A4c-2;
//! the record lists each one); where the remaining iterations of a TS loop are provably identical (DB, AMORDEGRC),
//! the loop stops at the fixed point with the TS result.
#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::conv::truthy;
use uo_jsnum::math::{abs, ceil, exp, expm1, floor, log, log1p, log10, max, min, pow, round, sign, sqrt, trunc};

use super::daycount::{
    Bond, CDate, coup_day_bs, coup_days, coup_days_nc, coup_ncd, coup_num, coup_pcd, days_in_year, diff_date, year_frac, year_frac_parts,
};
use super::{F64Buf, LOOP_BUDGET, PRICE_BUDGET, Steps, type_error};
use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::functions::shared::mathu::{MAX_NUM_ARGS, NumSpec, Nums, Raw, atp_fn, atp_num, atp_raw, each_number_ref, lift_scalars, num_fn};
use crate::functions::shared::{EMPTY, R, Stop, arg, bodies, err, len, num, num_or_err, scalar, to_num};
use crate::math_util::{Num, RoundMode, decimal_round, excel_power, fin};
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `financial.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "FV" => fv,
    "PV" => pv,
    "PMT" => pmt,
    "IPMT" => ipmt,
    "PPMT" => ppmt,
    "NPER" => nper,
    "RATE" => rate,
    "ISPMT" => ispmt,
    "RRI" => rri,
    "PDURATION" => pduration,
    "NPV" => npv,
    "IRR" => irr,
    "MIRR" => mirr,
    "XNPV" => xnpv,
    "XIRR" => xirr,
    "FVSCHEDULE" => fvschedule,
    "CUMIPMT" => cumipmt,
    "CUMPRINC" => cumprinc,
    "EFFECT" => effect,
    "NOMINAL" => nominal,
    "SLN" => sln,
    "SYD" => syd,
    "DB" => db,
    "DDB" => ddb,
    "VDB" => vdb,
    "AMORDEGRC" => amordegrc,
    "AMORLINC" => amorlinc,
    "DOLLARDE" => dollarde,
    "DOLLARFR" => dollarfr,
    "COUPDAYBS" => coupdaybs,
    "COUPDAYS" => coupdays,
    "COUPDAYSNC" => coupdaysnc,
    "COUPNCD" => coupncd,
    "COUPNUM" => coupnum,
    "COUPPCD" => couppcd,
    "PRICE" => price,
    "YIELD" => yield_fn,
    "DURATION" => duration,
    "MDURATION" => mduration,
    "DISC" => disc,
    "INTRATE" => intrate,
    "RECEIVED" => received,
    "PRICEDISC" => pricedisc,
    "YIELDDISC" => yielddisc,
    "PRICEMAT" => pricemat,
    "YIELDMAT" => yieldmat,
    "ACCRINTM" => accrintm,
    "ACCRINT" => accrint,
    "TBILLPRICE" => tbillprice,
    "TBILLYIELD" => tbillyield,
    "TBILLEQ" => tbilleq,
    "ODDLPRICE" => oddlprice,
    "ODDLYIELD" => oddlyield,
    "ODDFPRICE" => oddfprice,
    "ODDFYIELD" => oddfyield,
];

/// `pw(x, y)`: `excelPower`, an Excel error as NaN.
#[inline]
fn pw(x: f64, y: f64) -> f64 {
    excel_power(x, y).unwrap_or(f64::NAN)
}

/// A number or an error as a body result (`fin` is applied by the factory).
#[inline]
fn val(r: Num) -> R<Value> {
    Ok(num_or_err(r))
}

/// An error as a body result.
#[inline]
fn e(c: ErrorCode) -> R<Value> {
    Ok(err(c))
}

/// `fin(x)` as a body result (the bodies that are not factories).
#[inline]
fn finv(x: f64) -> R<Value> {
    Ok(num_or_err(fin(x)))
}

/// An Excel error of a helper as the body's value (a host throw goes on).
fn err_value(r: R<()>) -> R<Option<Value>> {
    match r {
        Ok(()) => Ok(None),
        Err(Stop::Err(c)) => Ok(Some(err(c))),
        Err(t) => Err(t),
    }
}

// ---------------------------------------------------------------- time value of money

/// `fvOf(r, n, pmt, pv, t)`.
pub(super) fn fv_of(r: f64, n: f64, pmt: f64, pv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + pmt * n);
    }
    let q = pw(1.0 + r, n);
    -(pv * q + annuity(r, q, pmt, t))
}

/// `pvOf(r, n, pmt, fv, t)`.
fn pv_of(r: f64, n: f64, pmt: f64, fv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(fv + pmt * n);
    }
    let q = pw(1.0 + r, n);
    if q == 0.0 {
        return f64::NAN;
    }
    if !q.is_finite() {
        return f64::INFINITY;
    }
    (-fv - annuity(r, q, pmt, t)) / q
}

/// `annuity(r, q, pmt, t)`: pmt·(1+r·t)·(q−1)/r in Excel's order.
#[inline]
fn annuity(r: f64, q: f64, pmt: f64, t: f64) -> f64 {
    let k = (q - 1.0) / r;
    if truthy(t) { pmt * (k * (1.0 + r)) } else { pmt * k }
}

/// `pmtOf(r, n, pv, fv, t)`.
pub(super) fn pmt_of(r: f64, n: f64, pv: f64, fv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + fv) / n;
    }
    let l = log1p(r);
    let v = exp(-n * l);
    -((pv + fv * v) * r) / ((1.0 + r * t) * -expm1(-n * l))
}

/// `ipmtOf(r, per, n, pv, fv, t)`.
fn ipmt_of(r: f64, per: f64, n: f64, pv: f64, fv: f64, t: f64) -> f64 {
    let pmt = pmt_of(r, n, pv, fv, t);
    let ip = if per == 1.0 {
        if truthy(t) { 0.0 } else { -pv }
    } else if truthy(t) {
        fv_of(r, per - 2.0, pmt, pv, 1.0) - pmt
    } else {
        fv_of(r, per - 1.0, pmt, pv, 0.0)
    };
    ip * r
}

/// `typeArg(t)`.
#[inline]
fn type_arg(t: f64) -> f64 {
    if t != 0.0 { 1.0 } else { 0.0 }
}

const TVM: NumSpec = NumSpec::new(3, 5).defaults(&[0.0, 0.0]);
const TVM_PER: NumSpec = NumSpec::new(4, 6).defaults(&[0.0, 0.0]);

fn fv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM, |x, _| Ok(num(fv_of(x.at(0), x.at(1), x.at(2), x.at(3), type_arg(x.at(4))))))
}

fn pv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM, |x, _| {
        let v = pv_of(x.at(0), x.at(1), x.at(2), x.at(3), type_arg(x.at(4)));
        // measured by the TS: PV(−1;10;−100) = #DIV/0!
        if v.is_nan() { e(ErrorCode::Div0) } else { Ok(num(v)) }
    })
}

fn pmt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM, |x, _| {
        if x.at(1) == 0.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(pmt_of(x.at(0), x.at(1), x.at(2), x.at(3), type_arg(x.at(4)))))
    })
}

fn ipmt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM_PER, |x, _| {
        let (per, n) = (x.at(1), x.at(2));
        if per < 1.0 || per >= n + 1.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(ipmt_of(x.at(0), per, n, x.at(3), x.at(4), type_arg(x.at(5)))))
    })
}

fn ppmt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM_PER, |x, _| {
        let (r, per, n, p, f) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4));
        if per < 1.0 || per >= n + 1.0 {
            return e(ErrorCode::Num);
        }
        let tt = type_arg(x.at(5));
        Ok(num(pmt_of(r, n, p, f, tt) - ipmt_of(r, per, n, p, f, tt)))
    })
}

fn nper(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &TVM, |x, _| {
        let (r, p, v, f) = (x.at(0), x.at(1), x.at(2), x.at(3));
        let tt = type_arg(x.at(4));
        if r == 0.0 {
            return if p == 0.0 { e(ErrorCode::Div0) } else { Ok(num(-(v + f) / p)) };
        }
        let a = p * (1.0 + r * tt) - f * r;
        let b = p * (1.0 + r * tt) + v * r;
        if b == 0.0 {
            return e(ErrorCode::Num);
        }
        let q = a / b;
        if !(q > 0.0) || r <= -1.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(log(q) / log(1.0 + r)))
    })
}

/// `rateOf(n, p, v, f, t, guess)`: Newton with the analytic derivative from the guess; 20 iterations, |Δ| < 1E-7.
fn rate_of(n: f64, p: f64, v: f64, f: f64, t: f64, guess: f64) -> Num {
    let ff = |r: f64| -> f64 {
        if abs(r) < 1e-12 {
            return v + p * n + f;
        }
        let q = pw(1.0 + r, n);
        v * q + (p * (1.0 + r * t) * (q - 1.0)) / r + f
    };
    let dd = |r: f64| -> f64 {
        if abs(r) < 1e-12 {
            return v * n + p * (n * (n - 1.0) / 2.0 + t * n);
        }
        let q = pw(1.0 + r, n);
        let q1 = pw(1.0 + r, n - 1.0);
        v * n * q1 + (p * t * (q - 1.0)) / r + (p * (1.0 + r * t) * (n * q1 * r - (q - 1.0))) / (r * r)
    };
    let mut r = guess;
    for _ in 0..20 {
        let d = dd(r);
        if !d.is_finite() || d == 0.0 {
            return Err(ErrorCode::Num);
        }
        let rn = r - ff(r) / d;
        if !rn.is_finite() || rn <= -1.0 {
            return Err(ErrorCode::Num);
        }
        if abs(rn - r) < 1e-7 {
            // one more Newton step leaves the result at machine precision
            let d2 = dd(rn);
            return Ok(if truthy(d2) && d2.is_finite() { rn - ff(rn) / d2 } else { rn });
        }
        r = rn;
    }
    Err(ErrorCode::Num)
}

fn rate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    const S: NumSpec = NumSpec::new(3, 6).defaults(&[0.0, 0.0, 0.1]);
    num_fn(a, ctx, &S, |x, _| val(rate_of(x.at(0), x.at(1), x.at(2), x.at(3), type_arg(x.at(4)), x.at(5))))
}

fn ispmt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(4, 4), |x, _| {
        let (r, per, n, v) = (x.at(0), x.at(1), x.at(2), x.at(3));
        if n == 0.0 { e(ErrorCode::Div0) } else { Ok(num(v * r * (per / n - 1.0))) }
    })
}

fn rri(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let (n, v, f) = (x.at(0), x.at(1), x.at(2));
        if n <= 0.0 || v == 0.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(pw(f / v, 1.0 / n) - 1.0))
    })
}

fn pduration(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let (r, v, f) = (x.at(0), x.at(1), x.at(2));
        if r <= 0.0 || v <= 0.0 || f <= 0.0 {
            return e(ErrorCode::Num);
        }
        Ok(num((log(f) - log(v)) / log(1.0 + r)))
    })
}

// ---------------------------------------------------------------- cash flows

/// `eachNumber([v], ctx, f)` for one argument: a reference or an array visits its numbers (an error stops), a direct
/// scalar converts (empty skipped), a LAMBDA is `#CALC!`.
fn each_flow(v: &Value, ctx: &mut dyn FnContext, f: &mut dyn FnMut(f64)) -> R<()> {
    match v {
        Value::Ref(r) => each_number_ref(r, ctx, f),
        Value::Array(x) => {
            for &s in x.data.iter() {
                match s {
                    Scalar::Num(n) => f(n),
                    Scalar::Err(c) => return Err(Stop::Err(c)),
                    _ => {}
                }
            }
            Ok(())
        }
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Calc)),
        Value::Scalar(Scalar::Empty) => Ok(()),
        Value::Scalar(s) => {
            f(to_num(ctx, *s)?);
            Ok(())
        }
    }
}

fn npv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let r = match to_num(ctx, scalar(a, 0).unwrap_or(Scalar::Empty)) {
        Ok(r) => r,
        Err(c) => return e(c),
    };
    if r == -1.0 {
        return e(ErrorCode::Div0);
    }
    // the flows are summed as they are visited (the TS collects them first: same terms in the same order)
    let (mut s, mut i) = (0.0, 0.0);
    let mut add = |x: f64| {
        i += 1.0;
        s += x / pw(1.0 + r, i);
    };
    for k in 1..len(a) {
        // measured by the TS: an omitted argument counts as a period of value 0 (NPV(10%;;2) = 2/1,1²)
        let v = match arg(a, k) {
            None | Some(Value::Scalar(Scalar::Empty)) => &Value::Scalar(Scalar::Num(0.0)),
            Some(v) => v,
        };
        if let Some(x) = err_value(each_flow(v, ctx, &mut add))? {
            return Ok(x);
        }
    }
    finv(s)
}

/// `arrayFlows(v, ctx)`: the numbers of a matrix/reference (IRR, MIRR: text and empty skipped; an error inside a
/// matrix of flows = `#VALUE!`).
fn array_flows(v: Option<&Value>, ctx: &mut dyn FnContext, out: &mut F64Buf) -> R<()> {
    let v = v.unwrap_or(&EMPTY);
    if let Value::Scalar(Scalar::Err(c)) = v {
        return Err(Stop::Err(*c));
    }
    match each_flow(v, ctx, &mut |x| out.push(x)) {
        Err(Stop::Err(_)) if matches!(v, Value::Array(_) | Value::Ref(_)) => Err(Stop::Err(ErrorCode::Value)),
        r => r,
    }
}

/// `irrOf(vs, guess)`.
fn irr_of(vs: &[f64], guess: f64) -> Num {
    if !vs.iter().any(|&x| x > 0.0) || !vs.iter().any(|&x| x < 0.0) {
        return Err(ErrorCode::Num);
    }
    let mut r = guess;
    for _ in 0..20 {
        let (mut f, mut d) = (0.0, 0.0);
        let mut i = 0.0;
        for &v in vs {
            let q = pw(1.0 + r, i);
            f += v / q;
            d -= (i * v) / (q * (1.0 + r));
            i += 1.0;
        }
        if d == 0.0 || !d.is_finite() {
            return Err(ErrorCode::Num);
        }
        let mut rn = r - f / d;
        if !rn.is_finite() {
            return Err(ErrorCode::Num);
        }
        if rn <= -1.0 {
            // does not cross −100%: half way
            rn = (r - 1.0) / 2.0;
        }
        if abs(rn - r) < 1e-10 {
            return Ok(rn);
        }
        r = rn;
    }
    Err(ErrorCode::Num)
}

fn irr(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut vs = F64Buf::new();
    if let Some(x) = err_value(array_flows(arg(a, 0), ctx, &mut vs))? {
        return Ok(x);
    }
    let mut g = 0.1;
    if len(a) > 1 {
        match scalar(a, 1) {
            None | Some(Scalar::Empty) => {}
            Some(s) => match to_num(ctx, s) {
                Ok(n) => g = n,
                Err(c) => return e(c),
            },
        }
    }
    val(irr_of(vs.as_slice(), g))
}

fn mirr(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut vs = F64Buf::new();
    if let Some(x) = err_value(array_flows(arg(a, 0), ctx, &mut vs))? {
        return Ok(x);
    }
    let fr = to_num(ctx, scalar(a, 1).unwrap_or(Scalar::Empty));
    let rr = to_num(ctx, scalar(a, 2).unwrap_or(Scalar::Empty));
    let fr = match fr {
        Ok(x) => x,
        Err(c) => return e(c),
    };
    let rr = match rr {
        Ok(x) => x,
        Err(c) => return e(c),
    };
    let vs = vs.as_slice();
    let n = vs.len() as f64;
    let (mut pos, mut neg) = (0.0, 0.0);
    let mut i = 0.0;
    for &v in vs {
        if v > 0.0 {
            pos += v * pow(1.0 + rr, n - 1.0 - i);
        } else if v < 0.0 {
            neg += v / pow(1.0 + fr, i);
        }
        i += 1.0;
    }
    // measured by the TS: no positive flow = −100%; no negative = #DIV/0!
    if neg == 0.0 || n < 2.0 {
        return e(ErrorCode::Div0);
    }
    finv(pow(-pos / neg, 1.0 / (n - 1.0)) - 1.0)
}

/// `atpNum(atpRaw(v))` of `math-util.ts` for a scalar position: an array reaches `toNumber` as an object (the TS
/// `TypeError`).
fn atp_scalar(v: Option<&Value>, ctx: &mut dyn FnContext) -> R<Num> {
    match atp_raw(v, ctx)? {
        Raw::S(s) => Ok(atp_num(ctx, s)),
        Raw::A(_) => Err(type_error()),
    }
}

/// `atpArray(v, ctx)`: the values/dates of XNPV/XIRR (measured by the TS: text, boolean or empty inside a list =
/// `#NUM!`).
fn atp_array(v: Option<&Value>, ctx: &mut dyn FnContext, out: &mut Vec<f64>) -> R<()> {
    let one = |x: Scalar, multi: bool, ctx: &dyn FnContext, out: &mut Vec<f64>| -> R<()> {
        match x {
            Scalar::Err(c) => Err(Stop::Err(c)),
            Scalar::Num(n) => {
                out.push(n);
                Ok(())
            }
            _ if multi => Err(Stop::Err(ErrorCode::Num)),
            Scalar::Empty | Scalar::Bool(_) => Err(Stop::Err(ErrorCode::Value)),
            Scalar::Str(_) => {
                out.push(atp_num(ctx, x)?);
                Ok(())
            }
        }
    };
    match v {
        None | Some(Value::Scalar(Scalar::Empty)) => Err(Stop::Err(ErrorCode::NA)),
        Some(Value::Scalar(s)) => one(*s, false, ctx, out),
        // a LAMBDA in a 1 × 1 matrix reaches `toNumber` (the TS `TypeError`)
        Some(Value::Lambda(_)) => Err(type_error()),
        Some(Value::Array(m)) => {
            let multi = m.data.len() > 1;
            m.data.iter().try_for_each(|&x| one(x, multi, ctx, out))
        }
        Some(Value::Ref(r)) => {
            if r.single().is_none() {
                return Err(Stop::Err(ErrorCode::Value));
            }
            let m = ctx.values(r, 0)?;
            let multi = m.data.len() > 1;
            m.data.iter().try_for_each(|&x| one(x, multi, ctx, out))
        }
    }
}

/// `xArgs(args, ctx, vi)`: values and truncated dates (same length, every date ≥ the first and ≥ 0).
fn x_args(a: &CallArgs<'_>, ctx: &mut dyn FnContext, vi: usize, vs: &mut Vec<f64>, ds: &mut Vec<f64>) -> R<()> {
    atp_array(arg(a, vi), ctx, vs)?;
    atp_array(arg(a, vi + 1), ctx, ds)?;
    if vs.len() != ds.len() {
        return Err(Stop::Err(ErrorCode::Num));
    }
    let d0 = trunc(ds.first().copied().unwrap_or(f64::NAN));
    for d in ds.iter_mut() {
        *d = trunc(*d);
        if *d < d0 || *d < 0.0 {
            return Err(Stop::Err(ErrorCode::Num));
        }
    }
    Ok(())
}

fn xnpv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let r = match atp_scalar(arg(a, 0), ctx)? {
        Ok(r) => r,
        Err(c) => return e(c),
    };
    let (mut vs, mut ds) = (Vec::new(), Vec::new());
    if let Some(x) = err_value(x_args(a, ctx, 1, &mut vs, &mut ds))? {
        return Ok(x);
    }
    // measured by the TS: XNPV(0;…) and a negative rate = #NUM!
    if r <= 0.0 {
        return e(ErrorCode::Num);
    }
    let mut s = 0.0;
    for (v, d) in vs.iter().zip(&ds) {
        s += v / pw(1.0 + r, (d - ds[0]) / 365.0);
    }
    finv(s)
}

/// `xirrOf(vs, ds, guess)`.
fn xirr_of(vs: &[f64], ds: &[f64], guess: f64) -> Num {
    if !vs.iter().any(|&x| x > 0.0) || !vs.iter().any(|&x| x < 0.0) {
        return Err(ErrorCode::Num);
    }
    let mut r = guess;
    for _ in 0..100 {
        let (mut f, mut d) = (0.0, 0.0);
        for (v, dt) in vs.iter().zip(ds) {
            let ex = (dt - ds[0]) / 365.0;
            let q = pw(1.0 + r, ex);
            f += v / q;
            d -= (ex * v) / (q * (1.0 + r));
        }
        if d == 0.0 || !d.is_finite() {
            return Err(ErrorCode::Num);
        }
        let rn = r - f / d;
        if !rn.is_finite() {
            return Err(ErrorCode::Num);
        }
        if abs(rn - r) < 1e-10 {
            return Ok(rn);
        }
        r = if rn <= -1.0 { (r - 1.0) / 2.0 } else { rn };
    }
    Err(ErrorCode::Num)
}

fn xirr(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut vs, mut ds) = (Vec::new(), Vec::new());
    if let Some(x) = err_value(x_args(a, ctx, 0, &mut vs, &mut ds))? {
        return Ok(x);
    }
    let mut g = 0.1;
    if len(a) > 2 && !matches!(arg(a, 2), None | Some(Value::Scalar(Scalar::Empty))) {
        match atp_scalar(arg(a, 2), ctx)? {
            Ok(n) => g = n,
            Err(c) => return e(c),
        }
    }
    val(xirr_of(&vs, &ds, g))
}

fn fvschedule(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let p = match atp_scalar(arg(a, 0), ctx)? {
        Ok(p) => p,
        Err(c) => return e(c),
    };
    let mut v = p;
    match arg(a, 1).unwrap_or(&EMPTY) {
        s @ (Value::Ref(_) | Value::Array(_)) => {
            let m = match s {
                Value::Array(m) => m.clone(),
                Value::Ref(r) => match r.single() {
                    Some(_) => ctx.values(r, 0)?,
                    None => return e(ErrorCode::Value),
                },
                _ => return e(ErrorCode::Value),
            };
            // every rate is checked before the product (an error after a valid rate wins)
            for &x in m.data.iter() {
                match x {
                    Scalar::Err(c) => return e(c),
                    Scalar::Empty | Scalar::Num(_) => {}
                    _ => return e(ErrorCode::Value),
                }
            }
            for &x in m.data.iter() {
                let r = if let Scalar::Num(r) = x { r } else { 0.0 };
                v *= 1.0 + r;
            }
        }
        Value::Lambda(_) => return e(ErrorCode::Value),
        s => match atp_scalar(Some(s), ctx)? {
            Ok(r) => v *= 1.0 + r,
            Err(c) => return e(c),
        },
    }
    finv(v)
}

/// `cumArgs(a)`: rate, periods and value positive, 1 ≤ start ≤ end ≤ periods, type 0 or 1.
fn cum_args(x: &Nums) -> Option<ErrorCode> {
    let (r, n, v, s, e, t) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4), x.at(5));
    if r <= 0.0 || n <= 0.0 || v <= 0.0 || s < 1.0 || e < 1.0 || s > e || e > n || (t != 0.0 && t != 1.0) {
        return Some(ErrorCode::Num);
    }
    None
}

/// The periods `s..=e` of CUMIPMT/CUMPRINC, each passed to `f` (`#NUM!` past [`LOOP_BUDGET`] periods: the TS loop
/// never ends for an infinite end and takes seconds past a few million).
fn cum_periods(s: f64, e: f64, mut f: impl FnMut(f64)) -> Result<(), ErrorCode> {
    if e - s >= f64::from(LOOP_BUDGET) {
        return Err(ErrorCode::Num);
    }
    let mut steps = Steps::new(LOOP_BUDGET);
    let mut i = s;
    while i <= e {
        steps.tick()?;
        f(i);
        i += 1.0;
    }
    Ok(())
}

fn cumipmt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(6, 6), |x, _| {
        if let Some(c) = cum_args(x) {
            return e(c);
        }
        let (r, n, v, t) = (x.at(0), x.at(1), x.at(2), x.at(5));
        // measured by the TS: CUMIPMT(…;1,5;3,7;…) = periods 2 and 3
        let mut s = ceil(x.at(3));
        let en = trunc(x.at(4));
        let p = pmt_of(r, n, v, 0.0, t);
        let mut z = 0.0;
        if s == 1.0 {
            if t <= 0.0 {
                z = -v;
            }
            s += 1.0;
        }
        if let Err(c) = cum_periods(s, en, |i| z += if t > 0.0 { fv_of(r, i - 2.0, p, v, 1.0) - p } else { fv_of(r, i - 1.0, p, v, 0.0) }) {
            return e(c);
        }
        Ok(num(z * r))
    })
}

fn cumprinc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(6, 6), |x, _| {
        if let Some(c) = cum_args(x) {
            return e(c);
        }
        let (r, n, v, t) = (x.at(0), x.at(1), x.at(2), x.at(5));
        let mut s = ceil(x.at(3));
        let en = trunc(x.at(4));
        let p = pmt_of(r, n, v, 0.0, t);
        let mut k = 0.0;
        if s == 1.0 {
            k = if t <= 0.0 { p + v * r } else { p };
            s += 1.0;
        }
        let step = |i: f64| if t > 0.0 { p - (fv_of(r, i - 2.0, p, v, 1.0) - p) * r } else { p - fv_of(r, i - 1.0, p, v, 0.0) * r };
        if let Err(c) = cum_periods(s, en, |i| k += step(i)) {
            return e(c);
        }
        Ok(num(k))
    })
}

fn effect(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |x, _| {
        let (nom, n) = (x.at(0), trunc(x.at(1)));
        if nom <= 0.0 || n < 1.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(pw(1.0 + nom / n, n) - 1.0))
    })
}

fn nominal(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |x, _| {
        let (eff, n) = (x.at(0), trunc(x.at(1)));
        if eff <= 0.0 || n < 1.0 {
            return e(ErrorCode::Num);
        }
        Ok(num((pw(1.0 + eff, 1.0 / n) - 1.0) * n))
    })
}

// ---------------------------------------------------------------- depreciation

fn sln(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let (c, s, l) = (x.at(0), x.at(1), x.at(2));
        if l == 0.0 { e(ErrorCode::Div0) } else { Ok(num((c - s) / l)) }
    })
}

fn syd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(4, 4), |x, _| {
        let (c, s, l, p) = (x.at(0), x.at(1), x.at(2), x.at(3));
        if l <= 0.0 || p <= 0.0 || p > l {
            return e(ErrorCode::Num);
        }
        Ok(num(((c - s) * (l - p + 1.0) * 2.0) / (l * (l + 1.0))))
    })
}

/// The same double, or NaN for NaN (a fixed point of an iteration).
#[inline]
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn db(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(4, 5).defaults(&[12.0]), |x, _| {
        let (c, s, l, p) = (x.at(0), x.at(1), x.at(2), x.at(3));
        let months = trunc(x.at(4));
        // a NaN month passes, as the TS comparisons
        if months < 1.0 || months > 12.0 || s < 0.0 || c < 0.0 || l < 1.0 || p <= 0.0 {
            return e(ErrorCode::Num);
        }
        let per = trunc(p);
        let extra = if months < 12.0 { 1.0 } else { 0.0 };
        if per > l + extra || p > l + 1.0 {
            return e(ErrorCode::Num);
        }
        if c == 0.0 {
            return Ok(num(0.0));
        }
        let mut rate_d = 1.0 - pw(s / c, 1.0 / l);
        rate_d = round(rate_d * 1000.0) / 1000.0;
        let first = (c * rate_d * months) / 12.0;
        if per == 1.0 {
            return Ok(num(first));
        }
        let mut book = c - first;
        let mut d = 0.0;
        let imax = trunc(min(l, per));
        // `book` decays (or grows) geometrically: it reaches a fixed point (0, a subnormal, ±∞, NaN) within ≈ 1.5M
        // steps, after which every step repeats — the loop stops there with the TS result
        let mut steps = Steps::new(LOOP_BUDGET);
        let mut i = 2.0;
        while i <= imax {
            if let Err(c) = steps.tick() {
                return e(c);
            }
            let before = book;
            d = book * rate_d;
            book -= d;
            if same(before, book) {
                break;
            }
            i += 1.0;
        }
        if per > l {
            d = (book * rate_d * (12.0 - months)) / 12.0;
        }
        Ok(num(d))
    })
}

/// `ddbOf(c, s, l, p, f)`: min(book·rate; book − salvage), book = cost·(1−rate)^(period−1).
fn ddb_of(c: f64, s: f64, l: f64, p: f64, f: f64) -> f64 {
    let rr = min(f / l, 1.0);
    let book = if p <= 1.0 { c } else { c * pw(1.0 - rr, p - 1.0) };
    max(0.0, min(book * rr, book - s))
}

fn ddb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(4, 5).defaults(&[2.0]), |x, _| {
        let (c, s, l, p, f) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4));
        if c < 0.0 || s < 0.0 || f <= 0.0 || l <= 0.0 || p <= 0.0 || p > l {
            return e(ErrorCode::Num);
        }
        Ok(num(ddb_of(c, s, l, p, f)))
    })
}

/// `interVdb(c, s, l, l1, per, f)` (`#NUM!` past the step budget).
fn inter_vdb(c: f64, s: f64, l: f64, l1: f64, per: f64, f: f64, steps: &mut Steps) -> Num {
    let (mut vdb, mut sln, mut now) = (0.0, 0.0, false);
    let int_end = ceil(per);
    if !steps.fits(int_end) {
        return Err(ErrorCode::Num);
    }
    let mut rest = c - s;
    let mut i = 1.0;
    while i <= int_end {
        steps.tick()?;
        let mut term;
        if now {
            term = sln;
        } else {
            let d = ddb_of(c, s, l, i, f);
            sln = rest / (l1 - (i - 1.0));
            if sln > d {
                term = sln;
                now = true;
            } else {
                term = d;
                rest -= d;
            }
        }
        if i == int_end {
            term *= per + 1.0 - int_end;
        }
        vdb += term;
        i += 1.0;
    }
    Ok(vdb)
}

fn vdb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = [0.0; 7];
    for (i, x) in xs.iter_mut().enumerate() {
        if i >= len(a) {
            *x = if i == 5 { 2.0 } else { 0.0 };
            continue;
        }
        let v = scalar(a, i).unwrap_or(Scalar::Empty);
        if i == 6 && matches!(v, Scalar::Str(_)) {
            return e(ErrorCode::Value);
        }
        *x = match to_num(ctx, v) {
            Ok(n) => n,
            Err(c) => return e(c),
        };
    }
    let [mut c, s, l, mut st, mut en, f, ns] = xs;
    if st < 0.0 || en < st || en > l || c < 0.0 || s < 0.0 || f < 0.0 || l <= 0.0 {
        return e(ErrorCode::Num);
    }
    let int_start = floor(st);
    let int_end = ceil(en);
    // the periods: a step budget (the TS takes seconds past a few million periods, forever for an infinite life)
    let mut steps = Steps::new(LOOP_BUDGET);
    if truthy(ns) {
        if !steps.fits(int_end - int_start) {
            return e(ErrorCode::Num);
        }
        let mut res = 0.0;
        let mut i = int_start + 1.0;
        while i <= int_end {
            if let Err(c) = steps.tick() {
                return e(c);
            }
            let mut term = ddb_of(c, s, l, i, f);
            if i == int_start + 1.0 {
                term *= min(en, int_start + 1.0) - st;
            } else if i == int_end {
                term *= en + 1.0 - int_end;
            }
            res += term;
            i += 1.0;
        }
        return finv(res);
    }
    let mut l1 = l;
    if st != floor(st) && f > 1.0 && st >= l / 2.0 {
        let part = st - l / 2.0;
        st = l / 2.0;
        en -= part;
        l1 += 1.0;
    }
    match inter_vdb(c, s, l, l1, st, f, &mut steps) {
        Ok(x) => c -= x,
        Err(c) => return e(c),
    }
    match inter_vdb(c, s, l, l - st, en - st, f, &mut steps) {
        Ok(x) => finv(x),
        Err(c) => e(c),
    }
}

/// `basisArg(b)`: the truncated basis 0–4 (NaN passes, as the TS).
fn basis_arg(b: f64) -> Num {
    let x = trunc(b);
    if x < 0.0 || x > 4.0 { Err(ErrorCode::Num) } else { Ok(x) }
}

/// `ROUND(x;0)` of Excel (15 digits: 327,49999999999994 → 328).
fn round_half(x: f64) -> f64 {
    decimal_round(x, 0.0, RoundMode::Half)
}

/// The usual rounding of AMORDEGRC (`sign(x)·floor(|x| + 0.5)`).
fn rnd(x: f64) -> f64 {
    sign(x) * floor(abs(x) + 0.5)
}

const AMOR: NumSpec = NumSpec::new(6, 7).defaults(&[0.0]);

fn amordegrc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &AMOR, |x, ctx| {
        let (cost, date, first, salv, per, r) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4), x.at(5));
        let basis = match basis_arg(x.at(6)) {
            Ok(b) if b != 2.0 => b,
            _ => return e(ErrorCode::Num),
        };
        if r <= 0.0 || per < 0.0 || salv < 0.0 || cost < 0.0 || salv > cost {
            return e(ErrorCode::Num);
        }
        let use_per = 1.0 / r;
        let coeff = if use_per < 3.0 {
            1.0
        } else if use_per < 5.0 {
            1.5
        } else if use_per <= 6.0 {
            2.0
        } else {
            2.5
        };
        let rr = r * coeff;
        // measured by the TS: at the second-to-last period of the life (⌈1/rate⌉) the rate becomes 50%; at the last,
        // the rest
        let life = ceil(use_per);
        let mut c = cost;
        let nr0 = year_frac(trunc(date), trunc(first), basis, ctx.date1904()) * rr * c;
        let n = trunc(per);
        if n == 0.0 {
            return Ok(num(round_half(nr0)));
        }
        let mut nr = rnd(nr0);
        c -= nr;
        let mut rest = c - salv;
        let mut steps = Steps::new(LOOP_BUDGET);
        let mut i = 0.0;
        while i < n {
            if let Err(c) = steps.tick() {
                return e(c);
            }
            let p = i + 1.0;
            let (c0, rest0) = (c, rest);
            nr = if p == life - 2.0 {
                rnd(c * 0.5)
            } else if p == life - 1.0 {
                c
            } else if p >= life {
                0.0
            } else {
                rnd(rr * c)
            };
            rest -= nr;
            if rest < 0.0 {
                return Ok(num(if n - i <= 1.0 { rnd(c * 0.5) } else { 0.0 }));
            }
            c -= nr;
            // the remaining steps repeat this one: past the life every step is 0 and changes nothing (`rest` was
            // checked); before it, a step that changed neither the book value nor the rest repeats until the
            // second-to-last period
            if p >= life {
                break;
            }
            if same(c0, c) && same(rest0, rest) {
                let special = life - 3.0;
                if !(special < 9_007_199_254_740_992.0) || special >= n {
                    // never reached (a NaN or infinite life, or past the last period): this step repeats to the end
                    break;
                }
                if special > i {
                    i = special;
                    continue;
                }
            }
            i += 1.0;
        }
        Ok(num(nr))
    })
}

fn amorlinc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &AMOR, |x, ctx| {
        let (cost, date, first, salv, per, r) = (x.at(0), x.at(1), x.at(2), x.at(3), x.at(4), x.at(5));
        let basis = match basis_arg(x.at(6)) {
            Ok(b) if b != 2.0 => b,
            _ => return e(ErrorCode::Num),
        };
        if r <= 0.0 || per < 0.0 || salv < 0.0 || cost < 0.0 || salv > cost {
            return e(ErrorCode::Num);
        }
        let n = trunc(per);
        let one = cost * r;
        let delta = cost - salv;
        let r0 = year_frac(trunc(date), trunc(first), basis, ctx.date1904()) * r * cost;
        let full = trunc((cost - salv - r0) / one);
        let res = if n == 0.0 {
            r0
        } else if n <= full {
            one
        } else if n == full + 1.0 {
            delta - one * full - r0
        } else {
            0.0
        };
        Ok(num(if res > 0.0 { res } else { 0.0 }))
    })
}

fn dollarde(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |x, _| {
        let (d, fr) = (x.at(0), trunc(x.at(1)));
        if fr < 0.0 {
            return e(ErrorCode::Num);
        }
        if fr == 0.0 {
            return e(ErrorCode::Div0);
        }
        let i = trunc(d);
        let frac = d - i;
        Ok(num(i + (frac * pw(10.0, ceil(log10(fr)))) / fr))
    })
}

fn dollarfr(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |x, _| {
        let (d, fr) = (x.at(0), trunc(x.at(1)));
        if fr < 0.0 {
            return e(ErrorCode::Num);
        }
        if fr == 0.0 {
            return e(ErrorCode::Div0);
        }
        let i = trunc(d);
        let frac = d - i;
        Ok(num(i + (frac * fr) / pw(10.0, ceil(log10(fr)))))
    })
}

// ---------------------------------------------------------------- bonds

/// `bondOf(settle, mat, freq, basis)`: settlement < maturity, frequency 1/2/4, basis 0–4.
fn bond_of(settle: f64, mat: f64, freq: f64, basis: f64, d1904: bool) -> Result<Bond, ErrorCode> {
    let b = basis_arg(basis)?;
    let f = trunc(freq);
    let (s, m) = (trunc(settle), trunc(mat));
    if f != 1.0 && f != 2.0 && f != 4.0 {
        return Err(ErrorCode::Num);
    }
    if s < 0.0 || m < 0.0 || s >= m {
        return Err(ErrorCode::Num);
    }
    Ok(Bond { settle: s, mat: m, freq: f, basis: b, d1904 })
}

/// `bondOf1`: PRICE/YIELD/DURATION — settlement at serial 0 = `#NUM!` (the COUP* accept it).
fn bond_of1(settle: f64, mat: f64, freq: f64, basis: f64, d1904: bool) -> Result<Bond, ErrorCode> {
    if trunc(settle) <= 0.0 {
        return Err(ErrorCode::Num);
    }
    bond_of(settle, mat, freq, basis, d1904)
}

/// `coupFn(name, f)`: a COUP* body (`None` from a coupon walk = a TS hang: `#NUM!`).
fn coup_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, f: fn(&Bond) -> Option<f64>) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(3, 4).defaults(&[0.0]), |x, ctx| match bond_of(x.at(0), x.at(1), x.at(2), x.at(3), ctx.date1904()) {
        Ok(b) => val(f(&b).ok_or(ErrorCode::Num)),
        Err(c) => e(c),
    })
}

fn coupdaybs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_day_bs)
}
fn coupdays(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_days)
}
fn coupdaysnc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_days_nc)
}
fn coupncd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_ncd)
}
fn coupnum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_num)
}
fn couppcd(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    coup_fn(a, ctx, coup_pcd)
}

/// The coupon data of `priceOf`/`durationOf`: E (`coupDays`), N (`coupNum`), A (`coupDayBs`), computed once per call
/// (they do not depend on the yield; the TS recomputes the same values at each price).
#[derive(Clone, Copy)]
struct Coupons {
    e: f64,
    n: f64,
    a: f64,
}

impl Coupons {
    fn of(b: &Bond) -> Option<Self> {
        Some(Self { e: coup_days(b)?, n: coup_num(b)?, a: coup_day_bs(b)? })
    }
}

/// `priceOf(b, rate, yld, red)` (Excel's help; measured by the TS: DSC = E − A). `pws` counts the powers computed.
fn price_of(f: f64, c: &Coupons, rate: f64, yld: f64, red: f64, pws: &mut f64) -> f64 {
    let (e, n, a) = (c.e, c.n, c.a);
    let dsc = e - a;
    let t1 = (100.0 * rate) / f;
    if n == 1.0 {
        return (red + t1) / (1.0 + (dsc / e) * yld / f) - (t1 * a) / e;
    }
    let t2 = 1.0 + yld / f;
    let mut p = red / pw(t2, n - 1.0 + dsc / e);
    let mut k = 1.0;
    // N coupons: bounded by the JS `Date` range (≈ 1.1M)
    while k <= n {
        p += t1 / pw(t2, k - 1.0 + dsc / e);
        k += 1.0;
    }
    *pws += n + 1.0;
    p - (t1 * a) / e
}

/// `yieldOf(b, rate, pr, red)`: the ATP secant/bisection (stops when the price equals the asked one within 2^-48).
/// The powers computed are budgeted ([`PRICE_BUDGET`]: never reached by dates up to 9999).
fn yield_of(b: &Bond, rate: f64, pr: f64, red: f64) -> Num {
    let f = b.freq;
    let n = coup_num(b).ok_or(ErrorCode::Num)?;
    if n <= 1.0 {
        let e = coup_days(b).ok_or(ErrorCode::Num)?;
        let a = coup_day_bs(b).ok_or(ErrorCode::Num)?;
        let dsr = e - a;
        let t = (rate / f) * (a / e);
        return Ok(((red / 100.0 + rate / f - (pr / 100.0 + t)) / (pr / 100.0 + t)) * ((f * e) / dsr));
    }
    let c = Coupons::of(b).ok_or(ErrorCode::Num)?;
    let mut pws = 0.0;
    let mut price = |y: f64| -> Num {
        if pws > PRICE_BUDGET {
            return Err(ErrorCode::Num);
        }
        Ok(price_of(f, &c, rate, y, red, &mut pws))
    };
    let eq = |a: f64, c: f64| a == c || abs(a - c) < abs(a) * 3.552_713_678_800_501e-15;
    let (mut y1, mut y2) = (0.0, 1.0);
    let mut p1 = price(y1)?;
    let mut p2 = price(y2)?;
    let mut pn = 0.0;
    let mut yn = (y2 - y1) * 0.5;
    let mut it = 0;
    while it < 100 && !eq(pn, pr) {
        pn = price(yn)?;
        if eq(pr, p1) {
            return Ok(y1);
        }
        if eq(pr, p2) {
            return Ok(y2);
        }
        if eq(pr, pn) {
            return Ok(yn);
        }
        if pr < p2 {
            y2 *= 2.0;
            p2 = price(y2)?;
            yn = (y2 - y1) * 0.5;
        } else {
            if pr < pn {
                y1 = yn;
                p1 = pn;
            } else {
                y2 = yn;
                p2 = pn;
            }
            yn = y2 - (y2 - y1) * ((pr - p2) / (p1 - p2));
        }
        it += 1;
    }
    if abs(pr - pn) > pr / 100.0 {
        return Err(ErrorCode::Num);
    }
    Ok(yn)
}

const BOND7: NumSpec = NumSpec::new(6, 7).defaults(&[0.0]);
const BOND6: NumSpec = NumSpec::new(5, 6).defaults(&[0.0]);
const DISC5: NumSpec = NumSpec::new(4, 5).defaults(&[0.0]);

fn price(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND7, |x, ctx| {
        let b = match bond_of1(x.at(0), x.at(1), x.at(5), x.at(6), ctx.date1904()) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let (r, y, red) = (x.at(2), x.at(3), x.at(4));
        if r < 0.0 || y < 0.0 || red <= 0.0 {
            return e(ErrorCode::Num);
        }
        let Some(c) = Coupons::of(&b) else { return e(ErrorCode::Num) };
        Ok(num(price_of(b.freq, &c, r, y, red, &mut 0.0)))
    })
}

fn yield_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND7, |x, ctx| {
        let b = match bond_of1(x.at(0), x.at(1), x.at(5), x.at(6), ctx.date1904()) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let (r, pr, red) = (x.at(2), x.at(3), x.at(4));
        if r < 0.0 || pr <= 0.0 || red <= 0.0 {
            return e(ErrorCode::Num);
        }
        val(yield_of(&b, r, pr, red))
    })
}

/// `durationOf(b, coup, yld)`: Macaulay with t_k = k − 1 + DSC/E, DSC = E − A (measured by the TS).
fn duration_of(b: &Bond, coup: f64, yld: f64) -> Num {
    let c = Coupons::of(b).ok_or(ErrorCode::Num)?;
    let (e, a, n) = (c.e, c.a, c.n);
    let x = (e - a) / e;
    let cf = (coup * 100.0) / b.freq;
    let y = 1.0 + yld / b.freq;
    let (mut nu, mut den) = (0.0, 0.0);
    let mut k = 1.0;
    while k <= n {
        let t = k - 1.0 + x;
        let v = (if k == n { cf + 100.0 } else { cf }) / pow(y, t);
        nu += t * v;
        den += v;
        k += 1.0;
    }
    Ok(nu / den / b.freq)
}

fn duration(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND6, |x, ctx| {
        let b = match bond_of1(x.at(0), x.at(1), x.at(4), x.at(5), ctx.date1904()) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let (c, y) = (x.at(2), x.at(3));
        if c < 0.0 || y < 0.0 {
            return e(ErrorCode::Num);
        }
        val(duration_of(&b, c, y))
    })
}

fn mduration(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND6, |x, ctx| {
        let b = match bond_of1(x.at(0), x.at(1), x.at(4), x.at(5), ctx.date1904()) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let (c, y) = (x.at(2), x.at(3));
        if c < 0.0 || y < 0.0 {
            return e(ErrorCode::Num);
        }
        val(duration_of(&b, c, y).map(|d| d / (1.0 + y / b.freq)))
    })
}

/// `discArgs(s, m, b)`: settlement < maturity, valid basis → (settlement, maturity, basis).
fn disc_args(s: f64, m: f64, b: f64) -> Result<(f64, f64, f64), ErrorCode> {
    let basis = basis_arg(b)?;
    let (ss, mm) = (trunc(s), trunc(m));
    if ss < 0.0 || mm < 0.0 || ss >= mm {
        return Err(ErrorCode::Num);
    }
    Ok((ss, mm, basis))
}

/// The shape of DISC/INTRATE/YIELDDISC: `k(x2, x3) · (year/days)` with both positive.
fn disc_like(a: &CallArgs<'_>, ctx: &mut dyn FnContext, k: fn(f64, f64) -> f64) -> R<Value> {
    atp_fn(a, ctx, &DISC5, |x, ctx| {
        let (s, m, b) = match disc_args(x.at(0), x.at(1), x.at(4)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (p, q) = (x.at(2), x.at(3));
        if p <= 0.0 || q <= 0.0 {
            return e(ErrorCode::Num);
        }
        let (days, year) = year_frac_parts(s, m, b, ctx.date1904());
        Ok(num(k(p, q) * (year / days)))
    })
}

fn disc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    disc_like(a, ctx, |pr, red| 1.0 - pr / red)
}

fn intrate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    disc_like(a, ctx, |inv, red| (red - inv) / inv)
}

fn yielddisc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    disc_like(a, ctx, |pr, red| (red - pr) / pr)
}

fn received(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &DISC5, |x, ctx| {
        let (s, m, b) = match disc_args(x.at(0), x.at(1), x.at(4)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (inv, d) = (x.at(2), x.at(3));
        if inv <= 0.0 || d <= 0.0 {
            return e(ErrorCode::Num);
        }
        let den = 1.0 - d * year_frac(s, m, b, ctx.date1904());
        if den <= 0.0 { e(ErrorCode::Num) } else { Ok(num(inv / den)) }
    })
}

fn pricedisc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &DISC5, |x, ctx| {
        let (s, m, b) = match disc_args(x.at(0), x.at(1), x.at(4)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (d, red) = (x.at(2), x.at(3));
        if d <= 0.0 || red <= 0.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(red * (1.0 - d * year_frac(s, m, b, ctx.date1904()))))
    })
}

/// PRICEMAT/YIELDMAT: `(B, DIM, DSM, A)` (Excel's help: B = days in the year, basis 1 = the settlement's year).
fn mat_days(s: f64, m: f64, is: f64, b: f64, d1904: bool) -> (f64, f64, f64, f64) {
    let bb = days_in_year(s, b, d1904);
    let dc = |x: f64, z: f64| year_frac_parts(x, z, b, d1904).0;
    (bb, dc(is, m), dc(s, m), dc(is, s))
}

fn pricemat(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND6, |x, ctx| {
        let (s, m, b) = match disc_args(x.at(0), x.at(1), x.at(5)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let is = trunc(x.at(2));
        let (r, y) = (x.at(3), x.at(4));
        if r < 0.0 || y < 0.0 || is > s {
            return e(ErrorCode::Num);
        }
        let (bb, dim, dsm, aa) = mat_days(s, m, is, b, ctx.date1904());
        Ok(num((100.0 + (dim / bb) * r * 100.0) / (1.0 + (dsm / bb) * y) - (aa / bb) * r * 100.0))
    })
}

fn yieldmat(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &BOND6, |x, ctx| {
        let (s, m, b) = match disc_args(x.at(0), x.at(1), x.at(5)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let is = trunc(x.at(2));
        let (r, pr) = (x.at(3), x.at(4));
        if r < 0.0 || pr <= 0.0 || is > s {
            return e(ErrorCode::Num);
        }
        let (bb, dim, dsm, aa) = mat_days(s, m, is, b, ctx.date1904());
        let t = pr / 100.0 + (aa / bb) * r;
        Ok(num(((1.0 + (dim / bb) * r - t) / t) * (bb / dsm)))
    })
}

/// `accrArgs(args, ctx, n, parIdx, boolIdx)`: the ATP rules, but an omitted par = 1000 and the omitted positions after
/// it take their defaults (basis 0, calc_method TRUE).
fn accr_args(a: &CallArgs<'_>, ctx: &mut dyn FnContext, n: usize, par_idx: usize, bool_idx: Option<usize>) -> R<[Raw; 8]> {
    let mut raw: [Raw; 8] = core::array::from_fn(|_| Raw::S(Scalar::Empty));
    let after = par_idx + usize::from(bool_idx.is_some());
    for (i, slot) in raw.iter_mut().enumerate().take(n) {
        let v = arg(a, i);
        let missing = matches!(v, None | Some(Value::Scalar(Scalar::Empty)));
        *slot = if missing && i == par_idx {
            Raw::S(Scalar::Num(1000.0))
        } else if missing && i > after {
            Raw::S(if Some(i) == bool_idx { Scalar::Bool(true) } else { Scalar::Num(0.0) })
        } else {
            atp_raw(v, ctx)?
        };
    }
    Ok(raw)
}

/// `accrNums(s, ctx, boolIdx)`: the numbers (a boolean only at the calc_method position).
fn accr_nums(sc: &[Scalar], ctx: &dyn FnContext, bool_idx: Option<usize>) -> Result<[f64; MAX_NUM_ARGS], ErrorCode> {
    let mut out = [0.0; MAX_NUM_ARGS];
    for (i, (&v, o)) in sc.iter().zip(out.iter_mut()).enumerate() {
        *o = match v {
            Scalar::Bool(b) if Some(i) == bool_idx => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            v => atp_num(ctx, v)?,
        };
    }
    Ok(out)
}

fn accrintm(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let raw = accr_args(a, ctx, 5, 3, None)?;
    lift_scalars(&raw[..5], ctx, |sc, ctx| {
        let x = match accr_nums(sc, ctx, None) {
            Ok(x) => x,
            Err(c) => return e(c),
        };
        let [iss, s, r, par, b, ..] = x;
        let basis = match basis_arg(b) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let (is, ss) = (trunc(iss), trunc(s));
        if r <= 0.0 || par <= 0.0 || is >= ss {
            return e(ErrorCode::Num);
        }
        let (days, year) = diff_date(is, ss, basis, ctx.date1904());
        finv(par * r * (days / year))
    })
}

/// The quasi-coupon date `k` periods of `months` from `anchor` (`at(k)` of ACCRINT/ODDF*/ODDL*).
#[inline]
fn quasi_at(anchor: &CDate, k: f64, months: f64, d1904: bool) -> f64 {
    let mut c = *anchor;
    c.add_months(k * months);
    c.serial(d1904)
}

/// `dayCounter(basis, d1904)`: 30/360 by the `ScaDate` rule in bases 0 and 4, actual days otherwise.
#[inline]
fn day_count(basis: f64, d1904: bool, x: f64, z: f64) -> f64 {
    if basis == 0.0 || basis == 4.0 { CDate::diff(&CDate::of(x, basis, d1904), &CDate::of(z, basis, d1904)) } else { z - x }
}

/// Below every finite serial: the JS `Date` range starts at −8.64·10^15 ms = 100,000,000 days before 1970, serial
/// ≈ −99,974,431 (1900 system) or −99,975,893 (1904); a margin of a million days.
const MIN_DATE_SERIAL: f64 = -101_000_000.0;

/// The cap of the quasi-coupon walks (ACCRINT, ODDF*, ODDL*): the JS `Date` range bounds them to ≈ 2.4M periods.
const QUASI_CAP: u32 = 1 << 23;

fn accrint(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let raw = accr_args(a, ctx, 8, 4, Some(7))?;
    lift_scalars(&raw, ctx, |sc, ctx| {
        let x = match accr_nums(sc, ctx, Some(7)) {
            Ok(x) => x,
            Err(c) => return e(c),
        };
        let [iss, first, s, r, par, fr, b, ..] = x;
        let basis = match basis_arg(b) {
            Ok(b) => b,
            Err(c) => return e(c),
        };
        let f = trunc(fr);
        if f != 1.0 && f != 2.0 && f != 4.0 {
            return e(ErrorCode::Num);
        }
        let (is, fi, ss) = (trunc(iss), trunc(first), trunc(s));
        if r <= 0.0 || par <= 0.0 || is >= ss || is <= 0.0 || fi <= 0.0 {
            return e(ErrorCode::Num);
        }
        let d1904 = ctx.date1904();
        let months = 12.0 / f;
        // quasi-coupons: dates anchored on the first coupon (with the end-of-month rule)
        let anchor = CDate::of(fi, basis, d1904);
        let at = |k: f64| quasi_at(&anchor, k, months, d1904);
        let mut steps = Steps::new(QUASI_CAP);
        // `at(k)` is a pure function of k: each date is computed once (the TS computes it up to three times)
        let mut k = 0.0;
        let mut cur = at(k);
        while cur > is {
            if let Err(c) = steps.tick() {
                return e(c);
            }
            k -= 1.0;
            cur = at(k);
        }
        let mut sum = 0.0;
        while cur < ss {
            if let Err(c) = steps.tick() {
                return e(c);
            }
            let st = cur;
            let en = at(k + 1.0);
            cur = en;
            let nl = if basis == 1.0 { en - st } else { (if basis == 3.0 { 365.0 } else { 360.0 }) / f };
            let from = max(st, is);
            let to = min(en, ss);
            if to > from {
                // measured by the TS: 30/360 days with the DAYS360 rule (31 → 30)
                sum += day_count(basis, d1904, from, to) / nl;
            }
            k += 1.0;
        }
        finv(((par * r) / f) * sum)
    })
}

/// `tbillDays(s, m)`: actual days, more than a year = `#NUM!`.
fn tbill_days(s: f64, m: f64) -> Num {
    let (ss, mm) = (trunc(s), trunc(m));
    if ss >= mm || mm - ss > 365.0 {
        return Err(ErrorCode::Num);
    }
    Ok(mm - ss)
}

fn tbillprice(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let dsm = match tbill_days(x.at(0), x.at(1)) {
            Ok(d) => d,
            Err(c) => return e(c),
        };
        let d = x.at(2);
        if d <= 0.0 {
            return e(ErrorCode::Num);
        }
        let p = 100.0 * (1.0 - (d * dsm) / 360.0);
        if p <= 0.0 { e(ErrorCode::Num) } else { Ok(num(p)) }
    })
}

fn tbillyield(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let dsm = match tbill_days(x.at(0), x.at(1)) {
            Ok(d) => d,
            Err(c) => return e(c),
        };
        let pr = x.at(2);
        if pr <= 0.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(((100.0 - pr) / pr) * (360.0 / dsm)))
    })
}

fn tbilleq(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(3, 3), |x, _| {
        let dsm = match tbill_days(x.at(0), x.at(1)) {
            Ok(d) => d,
            Err(c) => return e(c),
        };
        let d = x.at(2);
        if d <= 0.0 {
            return e(ErrorCode::Num);
        }
        if dsm <= 182.0 {
            return Ok(num((365.0 * d) / (360.0 - d * dsm)));
        }
        // more than half a year: equivalent yield with semiannual compounding
        let p = 100.0 * (1.0 - (d * dsm) / 360.0);
        if p <= 0.0 {
            return e(ErrorCode::Num);
        }
        let bq = dsm / 365.0;
        let c = (p - 100.0) / p;
        Ok(num((-2.0 * bq + 2.0 * sqrt(bq * bq - (2.0 * bq - 1.0) * c)) / (2.0 * bq - 1.0)))
    })
}

/// The ODDF*/ODDL* checks of basis and frequency → (basis, frequency).
fn odd_basis_freq(b: f64, fr: f64) -> Result<(f64, f64), ErrorCode> {
    let basis = basis_arg(b)?;
    let f = trunc(fr);
    if f != 1.0 && f != 2.0 && f != 4.0 {
        return Err(ErrorCode::Num);
    }
    Ok((basis, f))
}

/// `oddlQuasi(s, m, last, f, basis)` summed as the bodies sum it: (Σ DC/NL, Σ DSC/NL, Σ A/NL) over the quasi-coupons
/// from `last` to the maturity.
fn oddl_sums(s: f64, m: f64, last: f64, f: f64, basis: f64, d1904: bool) -> Result<(f64, f64, f64), ErrorCode> {
    let anchor = CDate::of(last, basis, d1904);
    let at = |k: f64| quasi_at(&anchor, (k * 12.0) / f, 1.0, d1904);
    let dc = |x: f64, z: f64| day_count(basis, d1904, x, z);
    let (mut sdc, mut sdsc, mut sa) = (0.0, 0.0, 0.0);
    let mut steps = Steps::new(QUASI_CAP);
    let mut k = 0.0;
    // each quasi-coupon date is computed once (a pure function of k; the TS computes it up to three times)
    let mut cur = at(k);
    while cur < m {
        steps.tick()?;
        let st = cur;
        let en = at(k + 1.0);
        cur = en;
        let nl = if basis == 1.0 || basis == 3.0 { en - st } else { 360.0 / f };
        let end_in = min(en, m);
        let qdc = dc(st, end_in);
        let qa = if s > st { dc(st, min(en, s)) } else { 0.0 };
        let qdsc = if s < end_in { dc(max(s, st), end_in) } else { 0.0 };
        sdc += qdc / nl;
        sdsc += qdsc / nl;
        sa += qa / nl;
        k += 1.0;
    }
    Ok((sdc, sdsc, sa))
}

const ODDL: NumSpec = NumSpec::new(7, 8).defaults(&[0.0]);

fn oddlprice(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &ODDL, |x, ctx| {
        let (basis, f) = match odd_basis_freq(x.at(7), x.at(6)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (ss, mm, ll) = (trunc(x.at(0)), trunc(x.at(1)), trunc(x.at(2)));
        let (r, y, red) = (x.at(3), x.at(4), x.at(5));
        if r < 0.0 || y < 0.0 || red <= 0.0 || !(ll < ss && ss < mm) {
            return e(ErrorCode::Num);
        }
        let (sdc, sdsc, sa) = match oddl_sums(ss, mm, ll, f, basis, ctx.date1904()) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let t = (100.0 * r) / f;
        Ok(num((red + sdc * t) / (1.0 + sdsc * (y / f)) - sa * t))
    })
}

fn oddlyield(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &ODDL, |x, ctx| {
        let (basis, f) = match odd_basis_freq(x.at(7), x.at(6)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (ss, mm, ll) = (trunc(x.at(0)), trunc(x.at(1)), trunc(x.at(2)));
        let (r, pr, red) = (x.at(3), x.at(4), x.at(5));
        if r < 0.0 || pr <= 0.0 || red <= 0.0 || !(ll < ss && ss < mm) {
            return e(ErrorCode::Num);
        }
        let (sdc, sdsc, sa) = match oddl_sums(ss, mm, ll, f, basis, ctx.date1904()) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let t = (100.0 * r) / f;
        let base = pr + sa * t;
        Ok(num(((red + sdc * t - base) / base) * (f / sdsc)))
    })
}

/// The yield-independent part of `oddfPrice`: the quasi-coupons of the irregular first period (Σ DC/NL, Σ A/NL, the
/// discount exponent Nq + DSC/NL) and the regular coupons up to the maturity. `Err` = the TS walk never ends (a date
/// before the JS `Date` range) or the cap.
struct OddF {
    sdc: f64,
    sa: f64,
    x: f64,
    n: f64,
}

fn oddf_setup(s: f64, m: f64, iss: f64, first: f64, f: f64, basis: f64, d1904: bool) -> Result<OddF, ErrorCode> {
    let anchor = CDate::of(first, basis, d1904);
    let at = |k: f64| quasi_at(&anchor, (k * 12.0) / f, 1.0, d1904);
    let dc = |x: f64, z: f64| day_count(basis, d1904, x, z);
    let nl_of = |st: f64, en: f64| if basis == 1.0 || basis == 3.0 { en - st } else { 360.0 / f };
    let (mut sdc, mut sa, mut nq, mut dsc, mut nl) = (0.0, 0.0, 0.0, 0.0, 0.0);
    // every finite quasi-coupon lies after the first day of the JS `Date` range (serial ≈ −99,974,000 in both systems):
    // an issue before it is never reached and the TS walk never ends
    if iss < MIN_DATE_SERIAL {
        return Err(ErrorCode::Num);
    }
    let mut steps = Steps::new(QUASI_CAP);
    let mut k = 0.0;
    let mut en = at(k);
    loop {
        steps.tick()?;
        let st = at(k - 1.0);
        let len = nl_of(st, en);
        if en <= iss {
            break;
        }
        let from = max(st, iss);
        sdc += dc(from, en) / len;
        if s > from {
            sa += dc(from, min(en, s)) / len;
        }
        if s >= st && s < en {
            dsc = dc(s, en);
            nl = len;
            nq = -k;
        }
        if st <= iss {
            break;
        }
        if st.is_nan() {
            // a quasi-coupon before the JS `Date` range: every later one is NaN and the TS loop never ends
            return Err(ErrorCode::Num);
        }
        k -= 1.0;
        en = st;
    }
    let mut n = 0.0;
    while at(n) < m {
        steps.tick()?;
        n += 1.0;
    }
    Ok(OddF { sdc, sa, x: nq + dsc / nl, n })
}

/// `oddfPrice(…)` over a prepared [`OddF`]; `pws` counts the powers computed.
fn oddf_price(o: &OddF, rate: f64, yld: f64, red: f64, f: f64, pws: &mut f64) -> f64 {
    let t = (100.0 * rate) / f;
    let q = 1.0 + yld / f;
    let mut p = (t * o.sdc) / pw(q, o.x);
    let mut k = 1.0;
    while k <= o.n {
        p += (if k == o.n { red + t } else { t }) / pw(q, o.x + k);
        k += 1.0;
    }
    if o.n == 0.0 {
        p += red / pw(q, o.x);
    }
    *pws += o.n + 2.0;
    p - t * o.sa
}

const ODDF: NumSpec = NumSpec::new(8, 9).defaults(&[0.0]);

/// The common checks of ODDFPRICE/ODDFYIELD → (settlement, maturity, issue, first coupon, basis, frequency).
fn oddf_args(x: &Nums, p: f64) -> Result<(f64, f64, f64, f64, f64, f64), ErrorCode> {
    let (basis, f) = odd_basis_freq(x.at(8), x.at(7))?;
    let (ss, mm, is, fc) = (trunc(x.at(0)), trunc(x.at(1)), trunc(x.at(2)), trunc(x.at(3)));
    let (r, red) = (x.at(4), x.at(6));
    if r < 0.0 || p < 0.0 || red <= 0.0 || !(is < ss && ss < fc && fc < mm) {
        return Err(ErrorCode::Num);
    }
    Ok((ss, mm, is, fc, basis, f))
}

fn oddfprice(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &ODDF, |x, ctx| {
        let (ss, mm, is, fc, basis, f) = match oddf_args(x, x.at(5)) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let o = match oddf_setup(ss, mm, is, fc, f, basis, ctx.date1904()) {
            Ok(o) => o,
            Err(c) => return e(c),
        };
        Ok(num(oddf_price(&o, x.at(4), x.at(5), x.at(6), f, &mut 0.0)))
    })
}

fn oddfyield(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &ODDF, |x, ctx| {
        let pr = x.at(5);
        // the price must be positive (`pr <= 0`): checked with the others, in the TS order
        let (ss, mm, is, fc, basis, f) = match oddf_args(x, if pr <= 0.0 { -1.0 } else { 0.0 }) {
            Ok(v) => v,
            Err(c) => return e(c),
        };
        let (r, red) = (x.at(4), x.at(6));
        let o = match oddf_setup(ss, mm, is, fc, f, basis, ctx.date1904()) {
            Ok(o) => o,
            Err(c) => return e(c),
        };
        let mut y = if truthy(r) { r } else { 0.1 };
        let mut pws = 0.0;
        for _ in 0..100 {
            if pws > PRICE_BUDGET {
                return e(ErrorCode::Num);
            }
            let p = oddf_price(&o, r, y, red, f, &mut pws) - pr;
            let h = 1e-7;
            let d = (oddf_price(&o, r, y + h, red, f, &mut pws) - (p + pr)) / h;
            if !truthy(d) || !d.is_finite() {
                return e(ErrorCode::Num);
            }
            let yn = y - p / d;
            if abs(yn - y) < 1e-12 {
                return Ok(num(yn));
            }
            y = yn;
        }
        e(ErrorCode::Num)
    })
}
