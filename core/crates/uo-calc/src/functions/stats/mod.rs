//! Family `stats` (frontier A4b): `functions/statistical.ts` (this file: AVERAGE…AGGREGATE), `distributions.ts`
//! ([`dist`]), `regression.ts` ([`regress`]) and `ets.ts` ([`ets`]). The numeric kernels are A1's
//! ([`crate::stats`], [`crate::special`]); the walkers are [`super::shared::statw`] plus [`walk`] here (the
//! `collectNumbers` rules with the store's lane fast path for plain references).
//!
//! Every body keeps the TS order of floating-point operations and of the context reads (a `NeedCell` throw comes
//! from the same walk as in the TS). Declared choices (record `uo-calc.md` "A4b — as built", A4b-n): order
//! statistics by selection instead of a full sort (A4b-2: the same element of the same total order), streaming
//! aggregates (A4b-1), the bounded loops of `ets.ts` and the regression (A4b-5/6).

mod dist;
mod ets;
mod regress;

use core::cmp::Ordering;
use std::collections::HashMap;

use uo_jsnum::conv::{canonicalize_nan, truthy};
use uo_jsnum::math::{abs, ceil, exp, floor, log, log2, pow, trunc};

use super::shared::agg;
use super::shared::statw::{collect_into_skipping, numeric_pairs};
use super::shared::{EMPTY, R, Stop, arg, bodies, err, len, num, num_or_err, one, scalar, view};
use crate::coerce::parse_number_text;
use crate::collate::round15;
use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::key::SheetIx;
use crate::math_util::snap_add;
use crate::stats::{BIG_AREA, Collect, DirectText, Take, area_size, arg_int, arg_number, devsq, kurt, mean, skew, skew_p, stat_num};
use crate::value::{ArrayValue, ErrorCode, Scalar, ScalarView, Value};

/// The bodies of `statistical.ts`.
const STATISTICAL: &[(&str, FnBody)] = bodies![
    "AVERAGE" => average,
    "AVERAGEA" => averagea,
    "MAX" => max,
    "MAXA" => maxa,
    "MIN" => min,
    "MINA" => mina,
    "MEDIAN" => median,
    "MODE" => mode_sngl,
    "MODE.SNGL" => mode_sngl,
    "MODE.MULT" => mode_mult,
    "STDEV" => stdev_s,
    "STDEV.S" => stdev_s,
    "STDEVA" => stdeva,
    "STDEVP" => stdev_p,
    "STDEV.P" => stdev_p,
    "STDEVPA" => stdevpa,
    "VAR" => var_s,
    "VAR.S" => var_s,
    "VARA" => vara,
    "VARP" => var_p,
    "VAR.P" => var_p,
    "VARPA" => varpa,
    "DEVSQ" => devsq_fn,
    "AVEDEV" => avedev,
    "KURT" => kurt_fn,
    "SKEW" => skew_fn,
    "SKEW.P" => skew_p_fn,
    "GEOMEAN" => geomean,
    "HARMEAN" => harmean,
    "COUNT" => count,
    "COUNTA" => counta,
    "COUNTBLANK" => countblank,
    "LARGE" => large,
    "SMALL" => small,
    "PERCENTILE" => percentile_inc,
    "PERCENTILE.INC" => percentile_inc,
    "PERCENTILE.EXC" => percentile_exc,
    "QUARTILE" => quartile_inc,
    "QUARTILE.INC" => quartile_inc,
    "QUARTILE.EXC" => quartile_exc,
    "PERCENTRANK" => percentrank_inc,
    "PERCENTRANK.INC" => percentrank_inc,
    "PERCENTRANK.EXC" => percentrank_exc,
    "RANK" => rank_eq,
    "RANK.EQ" => rank_eq,
    "RANK.AVG" => rank_avg,
    "TRIMMEAN" => trimmean,
    "FREQUENCY" => frequency,
    "STANDARDIZE" => standardize,
    "FISHER" => fisher,
    "FISHERINV" => fisherinv,
    "PERMUT" => permut,
    "PERMUTATIONA" => permutationa,
    "PROB" => prob,
    "SUBTOTAL" => subtotal,
    "AGGREGATE" => aggregate,
];

/// The bodies of the family: `statistical.ts`, `distributions.ts`, `regression.ts`, `ets.ts`.
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[STATISTICAL, dist::BODIES, regress::BODIES, ets::BODIES];

// ---------------------------------------------------------------- support shared by the family's files

/// `num(x)` of `stats.ts` as a value: ∞/NaN → `#NUM!`, a subnormal → 0.
#[inline]
pub(super) fn sn(x: f64) -> Value {
    num_or_err(stat_num(x))
}

/// `argNumber(v ?? null, ctx)` (`stats.ts`): a non-finite number is `#NUM!`, text converts in the locale.
#[inline]
pub(super) fn an(ctx: &dyn FnContext, v: Option<Scalar>) -> Result<f64, ErrorCode> {
    arg_number(view(ctx, v.unwrap_or(Scalar::Empty)), ctx.locale(), ctx.year())
}

/// `argInt(v ?? null, ctx)`.
#[inline]
fn ai(ctx: &dyn FnContext, v: Option<Scalar>) -> Result<f64, ErrorCode> {
    arg_int(view(ctx, v.unwrap_or(Scalar::Empty)), ctx.locale(), ctx.year())
}

/// `args[i] === undefined || args[i] === null` for a `value` argument.
#[inline]
pub(super) fn absent(v: Option<Scalar>) -> bool {
    matches!(v, None | Some(Scalar::Empty))
}

/// `⌊log2|x|⌋` through `Math.log2` (`exponentOf` of `stats.ts`) for a finite `x ≠ 0`, read from the exponent bits
/// where `Math.log2` cannot reach the next integer (decision A4b-7): with a mantissa `1 + f < 2 − 2^−20`, fdlibm's
/// log2 is `k + log2(1 + f)` (exactly `k` for a power of two) within an ulp of a result at least 6.8·10^−7 below
/// `k + 1`, so its floor is `k`; the top 2^−20 of each binade and the subnormals call `log2`.
#[inline]
fn floor_log2(x: f64) -> f64 {
    let bits = x.to_bits();
    let e = (bits >> 52) & 0x7ff;
    let m = bits & ((1u64 << 52) - 1);
    if e != 0 && e != 0x7ff && m < (1u64 << 52) - (1u64 << 32) {
        // e ≤ 2046: exact in an i32 and in an f64
        return f64::from(i32::try_from(e).unwrap_or(0) - 1023);
    }
    floor(log2(abs(x)))
}

/// `xlAdd(a, b)` of `stats.ts` ([`crate::stats::xl_add`]: the same value for every input) with [`floor_log2`].
#[inline]
pub(super) fn xadd(a: f64, b: f64) -> f64 {
    let r = a + b;
    if r != 0.0 && a != 0.0 && (a > 0.0) != (b > 0.0) && r.is_finite() && floor_log2(r) <= floor_log2(a) - 50.0 {
        return 0.0;
    }
    r
}

/// The plain options (`{}`): only numbers of references and arrays; direct text converts.
pub(super) const PLAIN: Collect = Collect {
    ref_text: Take::Skip,
    ref_bool: Take::Skip,
    arr_text: Take::Skip,
    arr_bool: Take::Skip,
    direct_text: DirectText::Convert,
    direct_bool_skip: false,
    skip_errors: false,
};

/// `A_OPTS` (the `*A` versions): text 0 and boolean 1/0 in a reference.
const A_OPTS: Collect = Collect { ref_text: Take::Count, ref_bool: Take::Count, ..PLAIN };

/// `collectInto(·, v, ctx, o)` as a stream: `f` gets every number the TS would push, in the same order; the first
/// error stops (`Err`). A reference with the plain reference rules (numbers only, errors propagate) reads the store's
/// column lanes through [`FnContext::each_number`] (decision A4a-8); the other rules walk the cells.
pub(super) fn walk(v: &Value, ctx: &mut dyn FnContext, o: &Collect, f: &mut dyn FnMut(f64)) -> R<()> {
    match v {
        Value::Ref(r) => {
            let plain = o.ref_text == Take::Skip && o.ref_bool == Take::Skip && !o.skip_errors;
            for &area in r.areas() {
                if plain {
                    if let Some(e) = ctx.each_number(area, &mut *f)? {
                        return Err(Stop::Err(e));
                    }
                    continue;
                }
                let mut e: Option<ErrorCode> = None;
                ctx.for_each_cell(area, &mut |s, _, _| {
                    if e.is_some() {
                        return;
                    }
                    match s {
                        Scalar::Num(n) => f(n),
                        Scalar::Err(x) => {
                            if !o.skip_errors {
                                e = Some(x);
                            }
                        }
                        Scalar::Str(_) => {
                            if o.ref_text == Take::Count {
                                f(0.0);
                            }
                        }
                        Scalar::Bool(b) => {
                            if o.ref_bool == Take::Count {
                                f(if b { 1.0 } else { 0.0 });
                            }
                        }
                        Scalar::Empty => {}
                    }
                })?;
                if let Some(e) = e {
                    return Err(Stop::Err(e));
                }
            }
            Ok(())
        }
        Value::Array(x) => {
            for &s in x.data.iter() {
                match s {
                    Scalar::Num(n) => f(n),
                    Scalar::Err(e) => {
                        if !o.skip_errors {
                            return Err(Stop::Err(e));
                        }
                    }
                    Scalar::Str(_) => {
                        if o.arr_text == Take::Count {
                            f(0.0);
                        }
                    }
                    Scalar::Bool(b) => {
                        if o.arr_bool == Take::Count {
                            f(if b { 1.0 } else { 0.0 });
                        }
                    }
                    Scalar::Empty => {}
                }
            }
            Ok(())
        }
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Value)),
        Value::Scalar(s) => {
            match *s {
                Scalar::Empty => f(0.0),
                Scalar::Num(n) => f(n),
                Scalar::Bool(b) => {
                    if !o.direct_bool_skip {
                        f(if b { 1.0 } else { 0.0 });
                    }
                }
                Scalar::Err(e) => {
                    if !o.skip_errors {
                        return Err(Stop::Err(e));
                    }
                }
                Scalar::Str(_) => {
                    if o.direct_text == DirectText::Ignore {
                        return Ok(());
                    }
                    let c: &dyn FnContext = ctx;
                    let ScalarView::Str(t) = view(c, *s) else { return Ok(()) };
                    match parse_number_text(t, c.locale(), false, c.year()) {
                        Some(n) => f(n),
                        None if o.direct_text != DirectText::Skip => return Err(Stop::Err(ErrorCode::Value)),
                        None => {}
                    }
                }
            }
            Ok(())
        }
    }
}

/// [`walk`] over every argument (`collectNumbers(args, ctx, o)` as a stream).
fn walk_args(a: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect, f: &mut dyn FnMut(f64)) -> R<()> {
    for i in 0..len(a) {
        walk(arg(a, i).unwrap_or(&EMPTY), ctx, o, f)?;
    }
    Ok(())
}

/// `collectNumbers([v], ctx, o)`.
pub(super) fn collect(v: &Value, ctx: &mut dyn FnContext, o: &Collect) -> R<Vec<f64>> {
    let mut out = Vec::new();
    walk(v, ctx, o, &mut |x| out.push(x))?;
    Ok(out)
}

/// `collectNumbers(args, ctx, o)`.
fn collect_args(a: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect) -> R<Vec<f64>> {
    let mut out = Vec::new();
    walk_args(a, ctx, o, &mut |x| out.push(x))?;
    Ok(out)
}

/// `numericPairs(xv, yv, ctx)` with a fast path (decision A4b-3): two one-area references of the same size (not a
/// big area) are first read through the store's lanes ([`FnContext::each_number`]); when every cell of both is a
/// number (as many numbers as cells, no error) the pairs are the two sequences as they are. Anything else (a gap,
/// text, an error, other shapes) goes through `zipCells` ([`numeric_pairs`]): the same pairs, the same first
/// error, and the same reads in the same order (x before y) for a `NeedCell`.
pub(super) fn pairs(xv: &Value, yv: &Value, ctx: &mut dyn FnContext, xs: &mut Vec<f64>, ys: &mut Vec<f64>) -> R<()> {
    if let (Value::Ref(rx), Value::Ref(ry)) = (xv, yv)
        && let (Some(ax), Some(ay)) = (rx.single(), ry.single())
    {
        let n = area_size(&ax);
        if n == area_size(&ay) && n <= BIG_AREA {
            let ex = ctx.each_number(ax, &mut |x| xs.push(x))?;
            let ey = ctx.each_number(ay, &mut |y| ys.push(y))?;
            if ex.is_none() && ey.is_none() && xs.len() as u64 == n && ys.len() as u64 == n {
                return Ok(());
            }
            xs.clear();
            ys.clear();
        }
    }
    numeric_pairs(xv, yv, ctx, xs, ys)
}

/// The order of `Float64Array.prototype.sort` (`sorted` of `stats.ts`): numeric, `-0` before `+0`, NaN last.
fn typed_cmp(a: &f64, b: &f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.total_cmp(b),
    }
}

/// `sorted(xs)[i]` without sorting (A4b-2): the element at position `i` of the typed order. `xs` is reordered (the
/// multiset is kept, so later selections on the same buffer stay right).
fn select(xs: &mut [f64], i: usize) -> f64 {
    if i >= xs.len() {
        return f64::NAN;
    }
    *xs.select_nth_unstable_by(i, typed_cmp).1
}

/// The first element of `xs` in the typed order (`sorted(xs)[0]`; NaN = `undefined` when empty).
fn typed_min(xs: &[f64]) -> f64 {
    let mut it = xs.iter();
    let Some(&first) = it.next() else { return f64::NAN };
    it.fold(first, |m, &x| if typed_cmp(&x, &m) == Ordering::Less { x } else { m })
}

/// The last element of `xs` in the typed order (`sorted(xs)[n − 1]`).
fn typed_max(xs: &[f64]) -> f64 {
    let mut it = xs.iter();
    let Some(&first) = it.next() else { return f64::NAN };
    it.fold(first, |m, &x| if typed_cmp(&x, &m) == Ordering::Greater { x } else { m })
}

/// A JS `Map` key of a number (SameValueZero: `-0` is `+0`, every NaN is one key).
fn map_key(x: f64) -> u64 {
    if x == 0.0 { 0 } else { canonicalize_nan(x).to_bits() }
}

/// An array index from a double the caller computed as a non-negative integer below the length.
#[inline]
fn ix(x: f64) -> usize {
    crate::cast::trunc_usize(x)
}

/// `xs.length` as a number.
#[inline]
#[allow(clippy::cast_precision_loss)]
fn nf(n: usize) -> f64 {
    n as f64
}

// ---------------------------------------------------------------- the aggregations

/// `aggregate(name, aggAverage, o)` streamed (A4b-1): `xlSum` = the plain sum of all but the last, then `snapAdd`.
fn average_with(a: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect) -> R<Value> {
    let (mut s, mut prev, mut last, mut n) = (0.0, 0.0, 0.0, 0usize);
    walk_args(a, ctx, o, &mut |x| {
        prev = s;
        s += x;
        last = x;
        n += 1;
    })?;
    if n == 0 {
        return Ok(err(ErrorCode::Div0));
    }
    Ok(sn(snap_add(prev, last) / nf(n)))
}

fn average(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    average_with(a, ctx, &PLAIN)
}

fn averagea(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    average_with(a, ctx, &Collect { arr_text: Take::Count, ..A_OPTS })
}

/// `aggMax`/`aggMin` streamed (strict comparison from ∓∞; 0 when empty).
fn extreme(a: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect, is_max: bool) -> R<Value> {
    let (mut m, mut n) = (if is_max { f64::NEG_INFINITY } else { f64::INFINITY }, 0usize);
    walk_args(a, ctx, o, &mut |x| {
        let better = if is_max { x > m } else { x < m };
        if better {
            m = x;
        }
        n += 1;
    })?;
    Ok(num(if n == 0 { 0.0 } else { m }))
}

fn max(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    extreme(a, ctx, &PLAIN, true)
}

fn maxa(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    extreme(a, ctx, &A_OPTS, true)
}

fn min(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    extreme(a, ctx, &PLAIN, false)
}

fn mina(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    extreme(a, ctx, &A_OPTS, false)
}

/// `aggMedian` (raw, no `num`): the middle element(s) by selection.
fn median_of(xs: &mut [f64]) -> Result<f64, ErrorCode> {
    let n = xs.len();
    if n == 0 {
        return Err(ErrorCode::Num);
    }
    if n % 2 == 1 {
        return Ok(select(xs, (n - 1) / 2));
    }
    let hi = select(xs, n / 2);
    let lo = typed_max(&xs[..n / 2]);
    Ok((lo + hi) / 2.0)
}

fn median(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = collect_args(a, ctx, &PLAIN)?;
    Ok(num_or_err(median_of(&mut xs)))
}

/// An `Agg` over collected numbers (`aggregate(name, agg, o)`).
fn agg_with(a: &CallArgs<'_>, ctx: &mut dyn FnContext, o: &Collect, f: agg::Agg) -> R<Value> {
    let xs = collect_args(a, ctx, o)?;
    Ok(num_or_err(f(&xs)))
}

fn stdev_s(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, agg::stdev_s)
}

fn stdeva(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &A_OPTS, agg::stdev_s)
}

fn stdev_p(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, agg::stdev_p)
}

fn stdevpa(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &A_OPTS, agg::stdev_p)
}

fn var_s(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, agg::var_s)
}

fn vara(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &A_OPTS, agg::var_s)
}

fn var_p(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, agg::var_p)
}

fn varpa(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &A_OPTS, agg::var_p)
}

/// `aggDevsq`.
fn devsq_agg(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() { Err(ErrorCode::Num) } else { stat_num(devsq(xs)) }
}

fn devsq_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, devsq_agg)
}

/// `aggAvedev`.
fn avedev_agg(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() {
        return Err(ErrorCode::Num);
    }
    let m = mean(xs);
    let mut s = 0.0;
    for &x in xs {
        s = xadd(s, abs(x - m));
    }
    stat_num(s / nf(xs.len()))
}

fn avedev(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, avedev_agg)
}

fn kurt_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, kurt)
}

fn skew_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, skew)
}

fn skew_p_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, skew_p)
}

/// `frexp(x)`: `x = m·2^e`, `m` in `[0.5, 1)` (through `Math.log2` and `2 ** e`, as the TS).
fn frexp(x: f64) -> (f64, f64) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0.0);
    }
    let mut e = ceil(log2(abs(x)));
    let mut m = x / pow(2.0, e);
    if abs(m) >= 1.0 {
        m /= 2.0;
        e += 1.0;
    }
    if abs(m) < 0.5 {
        m *= 2.0;
        e -= 1.0;
    }
    (m, e)
}

/// `aggGeomean`: the product of the mantissas and the sum of the exponents.
fn geomean_agg(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() {
        return Err(ErrorCode::Num);
    }
    let (mut m, mut e) = (1.0, 0.0);
    for &x in xs {
        if x <= 0.0 {
            return Err(ErrorCode::Num);
        }
        let (mm, ee) = frexp(x);
        m *= mm;
        e += ee;
        if m < 1e-150 {
            let (m2, e2) = frexp(m);
            m = m2;
            e += e2;
        }
    }
    let n = nf(xs.len());
    let q = floor(e / n);
    let r = e - q * n;
    if r > 600.0 || m < 1e-150 {
        return stat_num(pow(m, 1.0 / n) * pow(2.0, e / n));
    }
    stat_num(pow(m * pow(2.0, r), 1.0 / n) * pow(2.0, q))
}

fn geomean(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, geomean_agg)
}

/// `aggHarmean`.
fn harmean_agg(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() {
        return Err(ErrorCode::NA);
    }
    let mut s = 0.0;
    for &x in xs {
        if x <= 0.0 {
            return Err(ErrorCode::Num);
        }
        s = xadd(s, 1.0 / x);
    }
    stat_num(1.0 / (s / nf(xs.len())))
}

fn harmean(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    agg_with(a, ctx, &PLAIN, harmean_agg)
}

fn count(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut n = 0usize;
    walk_args(a, ctx, &Collect { skip_errors: true, direct_text: DirectText::Skip, ..PLAIN }, &mut |_| n += 1)?;
    Ok(num(nf(n)))
}

fn counta(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut n = 0usize;
    for i in 0..len(a) {
        match arg(a, i) {
            Some(Value::Ref(r)) => {
                for &area in r.areas() {
                    ctx.for_each_cell(area, &mut |v, _, _| {
                        if v != Scalar::Empty {
                            n += 1;
                        }
                    })?;
                }
            }
            Some(Value::Array(x)) => n += x.data.len(),
            _ => n += 1,
        }
    }
    Ok(num(nf(n)))
}

fn countblank(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(Value::Ref(r)) = arg(a, 0) else { return Ok(err(ErrorCode::Value)) };
    let mut n = 0.0;
    let mut texts = Vec::new();
    for &area in r.areas() {
        let mut filled = 0usize;
        texts.clear();
        ctx.for_each_cell(area, &mut |v, _, _| match v {
            Scalar::Empty => {}
            Scalar::Str(s) => texts.push(s),
            _ => filled += 1,
        })?;
        filled += texts.iter().filter(|&&s| !ctx.text(s).is_empty()).count();
        n += crate::cast::u53_to_f64(area_size(&area)) - nf(filled);
    }
    Ok(num(n))
}

/// `modes(xs)`: the most repeated values in the order of their first occurrence (`Map` keys: `-0` reads `+0`).
fn modes(xs: &[f64]) -> Vec<f64> {
    let mut at: HashMap<u64, usize> = HashMap::new();
    let mut cnt: Vec<(f64, usize)> = Vec::new();
    for &x in xs {
        let k = map_key(x);
        match at.get(&k) {
            Some(&i) => cnt[i].1 += 1,
            None => {
                at.insert(k, cnt.len());
                cnt.push((if x == 0.0 { 0.0 } else { x }, 1));
            }
        }
    }
    let best = cnt.iter().fold(1, |b, &(_, c)| if c > b { c } else { b });
    if best < 2 {
        return Vec::new();
    }
    cnt.into_iter().filter(|&(_, c)| c == best).map(|(v, _)| v).collect()
}

/// `aggModeSngl`.
fn mode_sngl_agg(xs: &[f64]) -> Result<f64, ErrorCode> {
    modes(xs).first().copied().ok_or(ErrorCode::NA)
}

/// `modeFn`'s checks and collection: an empty argument, a union, or a reference to ONE empty cell → `#VALUE!`;
/// direct text is ignored.
fn mode_data(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Vec<f64>> {
    for i in 0..len(a) {
        match arg(a, i) {
            None | Some(Value::Scalar(Scalar::Empty)) => return Err(Stop::Err(ErrorCode::Value)),
            Some(Value::Ref(r)) => {
                if r.areas().len() > 1 {
                    return Err(Stop::Err(ErrorCode::Value));
                }
                if let Some(x) = r.areas().first()
                    && x.r1 == x.r2
                    && x.c1 == x.c2
                    && ctx.cell(x.sheet, x.r1, x.c1)? == Scalar::Empty
                {
                    return Err(Stop::Err(ErrorCode::Value));
                }
            }
            _ => {}
        }
    }
    collect_args(a, ctx, &Collect { direct_text: DirectText::Ignore, ..PLAIN })
}

fn mode_sngl(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xs = mode_data(a, ctx)?;
    Ok(num_or_err(mode_sngl_agg(&xs)))
}

fn mode_mult(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xs = mode_data(a, ctx)?;
    let m = modes(&xs);
    if m.is_empty() {
        return Ok(err(ErrorCode::NA));
    }
    let rows = u32::try_from(m.len()).map_err(|_| Stop::Throw(crate::fnreg::Throw::Range))?;
    Ok(Value::Array(ArrayValue::new(rows, 1, m.into_iter().map(Scalar::Num).collect())))
}

// ---------------------------------------------------------------- order: LARGE/SMALL, percentiles, ranks

/// `dataOf(a, ctx)` of the first argument.
fn data_of(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Vec<f64>> {
    collect(arg(a, 0).unwrap_or(&EMPTY), ctx, &PLAIN)
}

/// `kth(xs, k, ctx, largest)`: SMALL truncates k; LARGE(k) = SMALL(n + 1 − k) truncated.
fn kth(xs: &mut [f64], k_raw: Option<Scalar>, ctx: &dyn FnContext, largest: bool) -> Result<f64, ErrorCode> {
    let kn = an(ctx, k_raw)?;
    let n = nf(xs.len());
    if xs.is_empty() || kn < 1.0 || kn > n {
        return Err(ErrorCode::Num);
    }
    let k = floor(if largest { n + 1.0 - kn } else { kn });
    if k < 1.0 || k > n {
        return Err(ErrorCode::Num);
    }
    Ok(select(xs, ix(k - 1.0)))
}

fn large(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(kth(&mut xs, scalar(a, 1), ctx, true)))
}

fn small(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(kth(&mut xs, scalar(a, 1), ctx, false)))
}

/// `s[i − 1] + f·(s[i] − s[i − 1])` for `1 ≤ i < n` by selection.
fn interpolate(xs: &mut [f64], i: usize, f: f64) -> f64 {
    let a = select(xs, i - 1);
    let b = typed_min(&xs[i..]);
    a + f * (b - a)
}

/// `percentileInc(sorted(xs), p)` (`n ≥ 1`, `0 ≤ p ≤ 1`).
fn pct_inc(xs: &mut [f64], p: f64) -> f64 {
    let n = xs.len();
    let pos = (nf(n) - 1.0) * p + 1.0;
    let i = floor(pos);
    let f = pos - i;
    if i < nf(n) { interpolate(xs, ix(i), f) } else { typed_max(xs) }
}

/// `percentileExc(sorted(xs), p)`.
fn pct_exc(xs: &mut [f64], p: f64) -> Result<f64, ErrorCode> {
    let n = nf(xs.len());
    let pos = p * (n + 1.0);
    if pos < 1.0 || pos > n {
        return Err(ErrorCode::Num);
    }
    let i = floor(pos);
    let f = pos - i;
    Ok(if i < n { interpolate(xs, ix(i), f) } else { select(xs, ix(i - 1.0)) })
}

/// `percentileFn(xs, p, ctx, exc)`.
fn percentile_of(xs: &mut [f64], p_raw: Option<Scalar>, ctx: &dyn FnContext, exc: bool) -> Result<f64, ErrorCode> {
    let p = an(ctx, p_raw)?;
    if xs.is_empty() {
        return Err(ErrorCode::Num);
    }
    let out = if exc { p <= 0.0 || p >= 1.0 } else { p < 0.0 || p > 1.0 };
    if out {
        return Err(ErrorCode::Num);
    }
    if exc { pct_exc(xs, p) } else { Ok(pct_inc(xs, p)) }
}

/// `quartileFn(xs, q, ctx, exc)`.
fn quartile_of(xs: &mut [f64], q_raw: Option<Scalar>, ctx: &dyn FnContext, exc: bool) -> Result<f64, ErrorCode> {
    let q = trunc(an(ctx, q_raw)?);
    if xs.is_empty() {
        return Err(ErrorCode::Num);
    }
    let out = if exc { q <= 0.0 || q >= 4.0 } else { q < 0.0 || q > 4.0 };
    if out {
        return Err(ErrorCode::Num);
    }
    if exc { pct_exc(xs, q / 4.0) } else { Ok(pct_inc(xs, q / 4.0)) }
}

fn percentile_inc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(percentile_of(&mut xs, scalar(a, 1), ctx, false)))
}

fn percentile_exc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(percentile_of(&mut xs, scalar(a, 1), ctx, true)))
}

fn quartile_inc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(quartile_of(&mut xs, scalar(a, 1), ctx, false)))
}

fn quartile_exc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = data_of(a, ctx)?;
    Ok(num_or_err(quartile_of(&mut xs, scalar(a, 1), ctx, true)))
}

/// `truncDigits(x, digits)`: the 15-digit view truncated to `digits` decimals.
fn trunc_digits(x: f64, digits: f64) -> f64 {
    let t = round15(x);
    let f = pow(10.0, digits);
    let mut y = floor(t * f);
    if (y + 1.0) / f <= t {
        y += 1.0;
    }
    y / f
}

/// `percentRankFn`: the position of x among the sorted numbers in one pass (A4b-2): `lo` = how many are smaller,
/// `s[lo]` the first not smaller, `s[lo − 1]` the last smaller.
fn percentrank(a: &CallArgs<'_>, ctx: &mut dyn FnContext, exc: bool) -> R<Value> {
    let xs = data_of(a, ctx)?;
    let x = an(ctx, scalar(a, 1))?;
    let mut sig = 3.0;
    let sr = scalar(a, 2);
    if !absent(sr) {
        sig = trunc(an(ctx, sr)?);
    }
    if sig < 1.0 || xs.is_empty() {
        return Ok(err(ErrorCode::Num));
    }
    let n = xs.len();
    if x < typed_min(&xs) || x > typed_max(&xs) {
        return Ok(err(ErrorCode::NA));
    }
    let lo = xs.iter().filter(|&&v| v < x).count();
    // `s[lo]`: the first in the typed order among those not smaller; `s[lo − 1]`: the last among the smaller ones
    let s_lo = if lo < n { typed_min_where(&xs, |v| v.partial_cmp(&x) != Some(Ordering::Less)) } else { f64::NAN };
    let rank = if lo < n && s_lo == x {
        nf(lo)
    } else {
        let a0 = if lo > 0 { typed_max_where(&xs, |v| v < x) } else { f64::NAN };
        nf(lo) - 1.0 + (x - a0) / (s_lo - a0)
    };
    let nn = nf(n);
    let r = if exc {
        if n == 1 { 1.0 } else { (rank + 1.0) / (nn + 1.0) }
    } else if n == 1 {
        1.0
    } else {
        rank / (nn - 1.0)
    };
    Ok(num(trunc_digits(r, sig)))
}

/// The first element in the typed order among those that pass `keep`.
fn typed_min_where(xs: &[f64], keep: impl Fn(f64) -> bool) -> f64 {
    let mut m: Option<f64> = None;
    for &v in xs {
        if keep(v) && m.is_none_or(|m| typed_cmp(&v, &m) == Ordering::Less) {
            m = Some(v);
        }
    }
    m.unwrap_or(f64::NAN)
}

/// The last element in the typed order among those that pass `keep`.
fn typed_max_where(xs: &[f64], keep: impl Fn(f64) -> bool) -> f64 {
    let mut m: Option<f64> = None;
    for &v in xs {
        if keep(v) && m.is_none_or(|m| typed_cmp(&v, &m) == Ordering::Greater) {
            m = Some(v);
        }
    }
    m.unwrap_or(f64::NAN)
}

fn percentrank_inc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    percentrank(a, ctx, false)
}

fn percentrank_exc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    percentrank(a, ctx, true)
}

/// `rankFn`: counted while the reference is walked (no list).
fn rank(a: &CallArgs<'_>, ctx: &mut dyn FnContext, avg: bool) -> R<Value> {
    let x = an(ctx, scalar(a, 0))?;
    let Some(rv @ Value::Ref(_)) = arg(a, 1) else { return Ok(err(ErrorCode::Value)) };
    // the order argument is read after the collection in the TS; reading it first only moves a pure conversion
    let or = scalar(a, 2);
    let desc = if absent(or) { Ok(true) } else { an(ctx, or).map(|o| o == 0.0) };
    let d = *desc.as_ref().unwrap_or(&true);
    let (mut better, mut same) = (0usize, 0usize);
    walk(rv, ctx, &PLAIN, &mut |v| {
        if v == x {
            same += 1;
        } else if (d && v > x) || (!d && v < x) {
            better += 1;
        }
    })?;
    desc?;
    if same == 0 {
        return Ok(err(ErrorCode::NA));
    }
    Ok(num(if avg { nf(better) + (nf(same) + 1.0) / 2.0 } else { nf(better) + 1.0 }))
}

fn rank_eq(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    rank(a, ctx, false)
}

fn rank_avg(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    rank(a, ctx, true)
}

/// `trimmean`: ⌊n·p/2⌋ dropped at each end; the rest summed in the ORIGINAL order, plain `+`.
fn trimmean(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xs = data_of(a, ctx)?;
    let p = an(ctx, scalar(a, 1))?;
    if p < 0.0 || p >= 1.0 || xs.is_empty() {
        return Ok(err(ErrorCode::Num));
    }
    let n = xs.len();
    let k = floor(nf(n) * p / 2.0);
    if k == 0.0 {
        let mut t = 0.0;
        for &x in &xs {
            t += x;
        }
        return Ok(sn(t / nf(n)));
    }
    let mut s = xs.clone();
    s.sort_by(typed_cmp);
    let k = ix(k);
    let mut drop: HashMap<u64, usize> = HashMap::new();
    for i in 0..k {
        *drop.entry(map_key(s[i])).or_default() += 1;
        *drop.entry(map_key(s[n - 1 - i])).or_default() += 1;
    }
    let mut sum = 0.0;
    for &x in &xs {
        if let Some(d) = drop.get_mut(&map_key(x))
            && *d > 0
        {
            *d -= 1;
            continue;
        }
        sum += x;
    }
    Ok(sn(sum / (nf(n) - 2.0 * nf(k))))
}

/// FREQUENCY's `asArr`: a scalar counts as a 1×1 array (only a number counts in it); an error stays direct.
fn freq_numbers(v: &Value, ctx: &mut dyn FnContext, o: &Collect) -> R<Vec<f64>> {
    match v {
        Value::Scalar(Scalar::Err(_)) | Value::Ref(_) | Value::Array(_) => collect(v, ctx, o),
        Value::Scalar(s) => collect(&Value::Array(one(*s)), ctx, o),
        // a LAMBDA inside a 1×1 array is neither a number, an error, a text nor a boolean: nothing counts
        Value::Lambda(_) => Ok(Vec::new()),
    }
}

fn frequency(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xs = freq_numbers(arg(a, 0).unwrap_or(&EMPTY), ctx, &PLAIN)?;
    let bs_raw = freq_numbers(arg(a, 1).unwrap_or(&EMPTY), ctx, &Collect { skip_errors: true, ..PLAIN })?;
    let bs = if bs_raw.is_empty() { vec![0.0] } else { bs_raw };
    let mut order: Vec<(f64, f64)> = bs.iter().enumerate().map(|(i, &b)| (b, nf(i))).collect();
    uo_jsnum::sort::js_sort_by(&mut order, |x, y| {
        let d = x.0 - y.0;
        if truthy(d) { d } else { x.1 - y.1 }
    });
    let mut counts = vec![0.0f64; bs.len() + 1];
    for &x in &xs {
        let (mut lo, mut hi) = (0usize, order.len());
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if x > order[mid].0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let at = if lo < order.len() { ix(order[lo].1) } else { bs.len() };
        counts[at] += 1.0;
    }
    let rows = u32::try_from(counts.len()).map_err(|_| Stop::Throw(crate::fnreg::Throw::Range))?;
    Ok(Value::Array(ArrayValue::new(rows, 1, counts.into_iter().map(Scalar::Num).collect())))
}

fn standardize(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = an(ctx, scalar(a, 0));
    let m = an(ctx, scalar(a, 1));
    let s = an(ctx, scalar(a, 2));
    let (x, m, s) = (x?, m?, s?);
    if s <= 0.0 {
        return Ok(err(ErrorCode::Num));
    }
    Ok(sn((x - m) / s))
}

fn fisher(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = an(ctx, scalar(a, 0))?;
    if x <= -1.0 || x >= 1.0 {
        return Ok(err(ErrorCode::Num));
    }
    Ok(sn(0.5 * log((1.0 + x) / (1.0 - x))))
}

fn fisherinv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let y = an(ctx, scalar(a, 0))?;
    let e = exp(2.0 * y);
    if !e.is_finite() {
        return Ok(num(1.0));
    }
    Ok(sn((e - 1.0) / (e + 1.0)))
}

fn permut(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = ai(ctx, scalar(a, 0));
    let k = ai(ctx, scalar(a, 1));
    let (n, k) = (n?, k?);
    if n < 0.0 || k < 0.0 || n < k {
        return Ok(err(ErrorCode::Num));
    }
    // (n−k+1)·…·n; every factor is ≥ 1 and the product stops at ∞, so the loop ends within ~1,100 steps even when
    // `i++` no longer moves (i ≥ 2^53)
    let mut p = 1.0f64;
    let mut i = n - k + 1.0;
    while i <= n && p.is_finite() {
        p *= i;
        i += 1.0;
    }
    Ok(sn(p))
}

fn permutationa(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = ai(ctx, scalar(a, 0));
    let k = ai(ctx, scalar(a, 1));
    let (n, k) = (n?, k?);
    if n < 0.0 || k < 0.0 {
        return Ok(err(ErrorCode::Num));
    }
    Ok(sn(pow(n, k)))
}

fn prob(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut xs, mut ps) = (Vec::new(), Vec::new());
    pairs(arg(a, 0).unwrap_or(&EMPTY), arg(a, 1).unwrap_or(&EMPTY), ctx, &mut xs, &mut ps)?;
    let l = an(ctx, scalar(a, 2))?;
    let mut h = l;
    let hr = scalar(a, 3);
    if !absent(hr) {
        h = an(ctx, hr)?;
    }
    let (mut total, mut s) = (0.0, 0.0);
    for (&x, &p) in xs.iter().zip(ps.iter()) {
        total = xadd(total, p);
        if x >= l && x <= h {
            s = xadd(s, p);
        }
    }
    if round15(total) != 1.0 {
        return Ok(err(ErrorCode::Num));
    }
    Ok(num(s))
}

// ---------------------------------------------------------------- SUBTOTAL and AGGREGATE

/// `SUBTOTAL_FNS[f]` (1–13; 3 is COUNTA, answered before).
fn subtotal_agg(f: f64, xs: &mut [f64]) -> Result<f64, ErrorCode> {
    match ix(f) {
        1 => agg::average(xs),
        2 => agg::count(xs),
        4 => agg::max(xs),
        5 => agg::min(xs),
        6 => agg::product(xs),
        7 => agg::stdev_s(xs),
        8 => agg::stdev_p(xs),
        9 => agg::sum(xs),
        10 => agg::var_s(xs),
        11 => agg::var_p(xs),
        12 => median_of(xs),
        _ => mode_sngl_agg(xs),
    }
}

/// `countaRefs([r], ctx, skip, skipErrors)`: the existing non-empty cells that are not skipped.
fn counta_ref(
    r: &Value,
    ctx: &mut dyn FnContext,
    skip: &mut dyn FnMut(&mut dyn FnContext, SheetIx, u32, u32) -> bool,
    skip_errors: bool,
) -> R<usize> {
    let Value::Ref(r) = r else { return Ok(0) };
    let mut n = 0;
    let mut cells: Vec<(Scalar, u32, u32)> = Vec::new();
    for &area in r.areas() {
        cells.clear();
        ctx.for_each_cell(area, &mut |v, row, col| cells.push((v, row, col)))?;
        for &(v, row, col) in &cells {
            if v == Scalar::Empty || skip(ctx, area.sheet, row, col) {
                continue;
            }
            if skip_errors && matches!(v, Scalar::Err(_)) {
                continue;
            }
            n += 1;
        }
    }
    Ok(n)
}

fn subtotal(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = trunc(an(ctx, scalar(a, 0))?);
    let base = if f > 100.0 { f - 100.0 } else { f };
    if base < 1.0 || base > 11.0 || (f > 11.0 && f < 101.0) || f > 111.0 {
        return Ok(err(ErrorCode::Value));
    }
    for i in 1..len(a) {
        if !matches!(arg(a, i), Some(Value::Ref(_))) {
            return Ok(err(ErrorCode::Value));
        }
    }
    let hide_manual = f > 100.0;
    let mut skip = |c: &mut dyn FnContext, s: SheetIx, r: u32, col: u32| -> bool {
        c.row_filtered(s, r) || (hide_manual && c.row_hidden(s, r)) || c.is_subtotal_cell(s, r, col)
    };
    if base == 3.0 {
        let mut n = 0;
        for i in 1..len(a) {
            n += counta_ref(arg(a, i).unwrap_or(&EMPTY), ctx, &mut skip, false)?;
        }
        return Ok(num(nf(n)));
    }
    let o = Collect { skip_errors: base == 2.0, ..PLAIN };
    let mut xs = Vec::new();
    for i in 1..len(a) {
        collect_into_skipping(&mut xs, arg(a, i).unwrap_or(&EMPTY), ctx, &o, &mut skip)?;
    }
    Ok(num_or_err(subtotal_agg(base, &mut xs)))
}

fn aggregate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let fnum = an(ctx, scalar(a, 0))?;
    let opt = an(ctx, scalar(a, 1))?;
    let (f, o) = (trunc(fnum), trunc(opt));
    if !(1.0..=19.0).contains(&f) || !(0.0..=7.0).contains(&o) {
        return Ok(err(ErrorCode::Value));
    }
    let ignore_nested = o <= 3.0;
    let ignore_hidden = o == 1.0 || o == 3.0 || o == 5.0 || o == 7.0;
    let ignore_errors = o == 2.0 || o == 3.0 || o == 6.0 || o == 7.0;
    let mut skip = |c: &mut dyn FnContext, s: SheetIx, r: u32, col: u32| -> bool {
        (ignore_hidden && (c.row_hidden(s, r) || c.row_filtered(s, r))) || (ignore_nested && c.is_subtotal_cell(s, r, col))
    };
    let opts = Collect { skip_errors: ignore_errors || f == 2.0, ..PLAIN };
    if f <= 13.0 {
        // the reference form: an array (constant or computed) → #VALUE! (measured)
        for i in 2..len(a) {
            if matches!(arg(a, i), Some(Value::Array(_))) {
                return Ok(err(ErrorCode::Value));
            }
        }
        if f == 3.0 {
            let mut n = 0;
            for i in 2..len(a) {
                match arg(a, i).unwrap_or(&EMPTY) {
                    r @ Value::Ref(_) => n += counta_ref(r, ctx, &mut skip, ignore_errors)?,
                    Value::Array(x) => n += x.data.iter().filter(|v| !(ignore_errors && matches!(v, Scalar::Err(_)))).count(),
                    Value::Scalar(Scalar::Err(_)) if ignore_errors => {}
                    _ => n += 1,
                }
            }
            return Ok(num(nf(n)));
        }
        let mut xs = Vec::new();
        for i in 2..len(a) {
            collect_into_skipping(&mut xs, arg(a, i).unwrap_or(&EMPTY), ctx, &opts, &mut skip)?;
        }
        return Ok(num_or_err(subtotal_agg(f, &mut xs)));
    }
    // 14–19: the array form AGGREGATE(f; option; array; k)
    if len(a) != 4 {
        return Ok(err(ErrorCode::Value));
    }
    let mut xs = Vec::new();
    collect_into_skipping(&mut xs, arg(a, 2).unwrap_or(&EMPTY), ctx, &opts, &mut skip)?;
    let one_k = |k: Scalar, xs: &mut Vec<f64>, c: &dyn FnContext| -> Scalar {
        if let Scalar::Err(e) = k {
            return Scalar::Err(e);
        }
        let r = match ix(f) {
            14 => kth(xs, Some(k), c, true),
            15 => kth(xs, Some(k), c, false),
            16 => percentile_of(xs, Some(k), c, false),
            17 => quartile_of(xs, Some(k), c, false),
            18 => percentile_of(xs, Some(k), c, true),
            _ => quartile_of(xs, Some(k), c, true),
        };
        match r {
            Ok(x) => Scalar::Num(x),
            Err(e) => Scalar::Err(e),
        }
    };
    // k an array (or a range) → element by element (measured: spills)
    let kv = match arg(a, 3).unwrap_or(&EMPTY) {
        Value::Ref(r) => match r.single() {
            Some(_) => Value::Array(ctx.values(r, 0)?),
            None => Value::Scalar(Scalar::Err(ErrorCode::Value)),
        },
        v => v.clone(),
    };
    Ok(Value::Scalar(match kv {
        Value::Array(ka) => {
            if ka.rows == 1 && ka.cols == 1 {
                one_k(ka.get(0, 0), &mut xs, ctx)
            } else {
                let c: &dyn FnContext = ctx;
                return Ok(Value::Array(ArrayValue::fill(ka.rows, ka.cols, |r, col| one_k(ka.get(r, col), &mut xs, c))));
            }
        }
        Value::Scalar(s) => one_k(s, &mut xs, ctx),
        // a LAMBDA as k converts to no number
        Value::Lambda(_) | Value::Ref(_) => Scalar::Err(ErrorCode::Value),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::xl_add;

    /// [`floor_log2`] equals `⌊Math.log2|x|⌋` (the binade edges, powers of two, subnormals and seeded values) and
    /// [`xadd`] equals A1's `xl_add`.
    #[test]
    fn floor_log2_equals_log2() {
        let mut xs: Vec<f64> = Vec::new();
        for e in -1074..=1023 {
            let p = pow(2.0, f64::from(e));
            for d in [p, p * (1.0 + f64::EPSILON), p * (2.0 - f64::EPSILON), p * (2.0 - 1e-6), p * (2.0 - 2e-6), p * 1.5] {
                xs.push(d);
                xs.push(-d);
            }
        }
        let mut seed = 0x1234_5678_9abc_def1_u64;
        for _ in 0..200_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            xs.push(f64::from_bits(seed & !(0x7ff << 52) | ((seed % 2046 + 1) << 52)));
        }
        for &x in &xs {
            if x == 0.0 || !x.is_finite() {
                continue;
            }
            assert_eq!(floor_log2(x), floor(log2(abs(x))), "{x:e}");
        }
        for w in xs.windows(2) {
            assert_eq!(xadd(w[0], -w[1]).to_bits(), xl_add(w[0], -w[1]).to_bits());
            assert_eq!(xadd(w[0], w[1] * -0.999_999_999_999_9).to_bits(), xl_add(w[0], w[1] * -0.999_999_999_999_9).to_bits());
        }
    }

    /// Selection gives the element of the full typed sort, also with `-0`/`+0` and NaN.
    #[test]
    fn selection_equals_sort() {
        let mut seed = 0x9e37_79b9_u32;
        for n in 1..60usize {
            let mut xs: Vec<f64> = (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    match seed % 7 {
                        0 => -0.0,
                        1 => 0.0,
                        2 => f64::NAN,
                        _ => f64::from(seed % 11) - 5.0,
                    }
                })
                .collect();
            let mut s = xs.clone();
            s.sort_by(typed_cmp);
            for (i, e) in s.iter().enumerate() {
                let v = select(&mut xs, i);
                assert_eq!(v.to_bits(), e.to_bits(), "n {n} i {i}");
            }
            assert_eq!(typed_min(&xs).to_bits(), s[0].to_bits());
            assert_eq!(typed_max(&xs).to_bits(), s[n - 1].to_bits());
        }
    }
}
