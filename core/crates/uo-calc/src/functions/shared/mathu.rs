//! The context walkers and `FnDef` factories of `functions/math-util.ts` (the pure rules are in
//! [`crate::math_util`]): `eachNumber`, `asMatrix`, `atpRaw`, `atpList`, `liftScalars`, `numFn`, `atpFn`.
//!
//! A factory of the TS (`numFn(name, min, max, body, opts)`) is a spec ([`NumSpec`]) plus a body closure: the
//! function of the family calls [`num_fn`]/[`atp_fn`] with both. Arguments are converted into a stack buffer
//! ([`Nums`]): no allocation on the scalar path.

use crate::fnreg::{CallArgs, FnContext};
use crate::stats::area_size;
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, Value, broadcast_at};

use super::{R, Stop, arg, fin_value, to_num, top};

/// The most arguments a `numFn`/`atpFn` body receives (the largest `maxArgs` of the TS factories is 8).
pub const MAX_NUM_ARGS: usize = 16;

/// The converted numbers of a `numFn`/`atpFn` call (`xs`), on the stack.
#[derive(Clone, Copy, Debug)]
pub struct Nums {
    xs: [f64; MAX_NUM_ARGS],
    n: usize,
}

impl Nums {
    /// No number.
    #[must_use]
    pub const fn new() -> Self {
        Self { xs: [0.0; MAX_NUM_ARGS], n: 0 }
    }
    /// `xs.push(x)` (past [`MAX_NUM_ARGS`] the value is dropped: no factory declares that many).
    pub fn push(&mut self, x: f64) {
        if let Some(d) = self.xs.get_mut(self.n) {
            *d = x;
            self.n += 1;
        }
    }
    /// The numbers.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        &self.xs[..self.n]
    }
    /// `xs.length`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.n
    }
    /// `xs.length === 0`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// `xs[i]` (`undefined` → `None`).
    #[must_use]
    pub fn get(&self, i: usize) -> Option<f64> {
        self.as_slice().get(i).copied()
    }
    /// `xs[i]` where the factory guarantees it exists (min/max arguments, defaults); NaN otherwise (the TS
    /// `undefined` in arithmetic).
    #[must_use]
    pub fn at(&self, i: usize) -> f64 {
        self.get(i).unwrap_or(f64::NAN)
    }
}

impl Default for Nums {
    fn default() -> Self {
        Self::new()
    }
}

/// The options of `numFn`/`atpFn` (`NumFnOpts`) with the arity.
#[derive(Clone, Copy, Debug)]
pub struct NumSpec {
    /// `minArgs`.
    pub min: usize,
    /// `maxArgs`.
    pub max: usize,
    /// Values of the missing optional arguments, by position after `min`.
    pub defaults: &'static [f64],
    /// An empty argument (`=F(1;)`) also takes its default.
    pub empty_is_default: bool,
}

impl NumSpec {
    /// `min`..`max` arguments, no defaults.
    #[must_use]
    pub const fn new(min: usize, max: usize) -> Self {
        Self { min, max, defaults: &[], empty_is_default: false }
    }
    /// With defaults.
    #[must_use]
    pub const fn defaults(mut self, d: &'static [f64]) -> Self {
        self.defaults = d;
        self
    }
    /// `emptyIsDefault`.
    #[must_use]
    pub const fn empty_default(mut self) -> Self {
        self.empty_is_default = true;
        self
    }
    /// `defaults?.[i - min]`.
    #[must_use]
    pub fn default_at(&self, i: usize) -> Option<f64> {
        i.checked_sub(self.min).and_then(|k| self.defaults.get(k).copied())
    }
}

/// The argument conversion of `numFn` (arguments `value`, lifted by the evaluator): converted in order, the first
/// failure decides the error; a missing optional argument takes its default (or ends the list).
pub fn num_args(a: &CallArgs<'_>, ctx: &dyn FnContext, s: &NumSpec) -> Result<Nums, ErrorCode> {
    let mut xs = Nums::new();
    for i in 0..s.max {
        let mut v = super::scalar(a, i);
        if matches!(v, Some(Scalar::Empty)) && s.empty_is_default && s.default_at(i).is_some() {
            v = None;
        }
        match v {
            None => match s.default_at(i) {
                Some(d) => xs.push(d),
                None => break,
            },
            Some(v) => xs.push(to_num(ctx, v)?),
        }
    }
    Ok(xs)
}

/// `numFn(name, min, max, body, opts).call`: converts the arguments ([`num_args`]) and applies `fin` to a numeric
/// result.
pub fn num_fn(
    a: &CallArgs<'_>,
    ctx: &mut dyn FnContext,
    s: &NumSpec,
    body: impl FnOnce(&Nums, &mut dyn FnContext) -> R<Value>,
) -> R<Value> {
    let xs = num_args(a, ctx, s)?;
    Ok(fin_value(body(&xs, ctx)?))
}

/// A raw argument of an ATP function after [`atp_raw`]: a scalar or an array (lifted).
#[derive(Clone, Debug)]
pub enum Raw {
    /// A scalar.
    S(Scalar),
    /// An array.
    A(ArrayValue),
}

/// `atpRaw(v, ctx)`: missing or empty → `#N/A`; a one-cell reference → its value (empty = 0); any other reference
/// or a LAMBDA → `#VALUE!`.
pub fn atp_raw(v: Option<&Value>, ctx: &mut dyn FnContext) -> R<Raw> {
    Ok(match v {
        None | Some(Value::Scalar(Scalar::Empty)) => Raw::S(Scalar::Err(ErrorCode::NA)),
        Some(Value::Ref(r)) => match r.single() {
            Some(x) if x.r1 == x.r2 && x.c1 == x.c2 => match ctx.cell(x.sheet, x.r1, x.c1)? {
                Scalar::Empty => Raw::S(Scalar::Num(0.0)),
                c => Raw::S(c),
            },
            _ => Raw::S(Scalar::Err(ErrorCode::Value)),
        },
        Some(Value::Lambda(_)) => Raw::S(Scalar::Err(ErrorCode::Value)),
        Some(Value::Array(x)) => Raw::A(x.clone()),
        Some(Value::Scalar(s)) => Raw::S(*s),
    })
}

/// `atpNum(v, ctx)` of `math-util.ts`: boolean → `#VALUE!`; empty → 0.
#[inline]
pub fn atp_num(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    if let Scalar::Bool(_) = v {
        return Err(ErrorCode::Value);
    }
    to_num(ctx, v)
}

/// The most raw arguments [`lift_scalars`] lifts at once (the ATP functions take at most 8).
const MAX_LIFT: usize = 16;

/// `liftScalars(xs, f)`: `f` over the scalars of each position, broadcast over the arrays (Excel's rule: a 1-wide
/// dimension repeats, outside → `#N/A`); no array → one call. The result of each element is its top-left scalar.
pub fn lift_scalars(xs: &[Raw], ctx: &mut dyn FnContext, mut f: impl FnMut(&[Scalar], &mut dyn FnContext) -> R<Value>) -> R<Value> {
    let (mut rows, mut cols) = (0u32, 0u32);
    for x in xs {
        if let Raw::A(a) = x {
            rows = rows.max(a.rows);
            cols = cols.max(a.cols);
        }
    }
    let n = xs.len().min(MAX_LIFT);
    let mut buf = [Scalar::Empty; MAX_LIFT];
    if rows == 0 {
        for (d, x) in buf.iter_mut().zip(xs) {
            *d = match x {
                Raw::S(s) => *s,
                // a 0-row array reaches `f` as an object the TS cannot convert: `#VALUE!` (declared, A4a-5)
                Raw::A(_) => Scalar::Err(ErrorCode::Value),
            };
        }
        return f(&buf[..n], ctx);
    }
    if crate::value::array_too_big(rows, cols) {
        return Err(Stop::Throw(crate::fnreg::Throw::Range));
    }
    let out = ArrayValue::try_fill(rows, cols, |r, c| -> R<Scalar> {
        for (d, x) in buf.iter_mut().zip(xs) {
            *d = match x {
                Raw::S(s) => *s,
                Raw::A(a) => broadcast_at(a, r, c),
            };
        }
        Ok(top(&f(&buf[..n], ctx)?))
    })?;
    Ok(Value::Array(out))
}

/// `atpFn(name, min, max, body, opts).call`: arguments `any`, the ATP rules (omitted = `#N/A`, boolean =
/// `#VALUE!`, multi-cell reference = `#VALUE!`), lifted over arrays; an omitted optional argument takes its default
/// (appended after the present ones, as the TS).
pub fn atp_fn(
    a: &CallArgs<'_>,
    ctx: &mut dyn FnContext,
    s: &NumSpec,
    mut body: impl FnMut(&Nums, &mut dyn FnContext) -> R<Value>,
) -> R<Value> {
    let mut raw: [Raw; MAX_LIFT] = core::array::from_fn(|_| Raw::S(Scalar::Empty));
    let mut nr = 0;
    let mut n_def = Nums::new();
    for i in 0..s.max {
        let dv = s.default_at(i);
        let v = arg(a, i);
        if v.is_some() && !(matches!(v, Some(Value::Scalar(Scalar::Empty))) && dv.is_some()) {
            if nr < MAX_LIFT {
                raw[nr] = atp_raw(v, ctx)?;
                nr += 1;
            }
        } else if let Some(d) = dv {
            n_def.push(d);
        } else {
            break;
        }
    }
    lift_scalars(&raw[..nr], ctx, |sc, ctx| {
        let mut xs = Nums::new();
        for &v in sc {
            match atp_num(ctx, v) {
                Ok(x) => xs.push(x),
                Err(e) => return Ok(super::err(e)),
            }
        }
        for &d in n_def.as_slice() {
            xs.push(d);
        }
        Ok(fin_value(body(&xs, ctx)?))
    })
}

/// `eachNumber(args, ctx, f)` (`SUM`, `PRODUCT`, `SUMSQ`…): a direct argument converts (text-number, boolean;
/// non-numeric text = `#VALUE!`; empty/omitted = skipped); references and arrays visit only numbers; an error
/// anywhere stops (the first in argument order, row by row inside a range); a LAMBDA = `#CALC!`.
pub fn each_number(args: &CallArgs<'_>, ctx: &mut dyn FnContext, f: &mut dyn FnMut(f64)) -> R<()> {
    for i in 0..args.args.len() {
        match arg(args, i) {
            Some(Value::Ref(r)) => each_number_ref(r, ctx, f)?,
            Some(Value::Array(x)) => {
                for &v in x.data.iter() {
                    match v {
                        Scalar::Num(n) => f(n),
                        Scalar::Err(e) => return Err(Stop::Err(e)),
                        _ => {}
                    }
                }
            }
            Some(Value::Lambda(_)) => return Err(Stop::Err(ErrorCode::Calc)),
            Some(Value::Scalar(Scalar::Empty)) | None => {}
            Some(Value::Scalar(s)) => f(to_num(ctx, *s)?),
        }
    }
    Ok(())
}

/// The reference branch of [`each_number`]: the existing cells of each area, row by row.
pub fn each_number_ref(r: &RefValue, ctx: &mut dyn FnContext, f: &mut dyn FnMut(f64)) -> R<()> {
    for &area in r.areas() {
        if let Some(e) = ctx.each_number(area, f)? {
            return Err(Stop::Err(e));
        }
    }
    Ok(())
}

/// `asMatrix(v, ctx)`: the values of an `array` argument as a matrix (scalar → 1×1; reference of several areas →
/// `#VALUE!`; LAMBDA → `#CALC!`).
pub fn as_matrix(v: &Value, ctx: &mut dyn FnContext) -> R<ArrayValue> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::Ref(r) => match r.single() {
            Some(_) => Ok(ctx.values(r, 0)?),
            None => Err(Stop::Err(ErrorCode::Value)),
        },
        Value::Lambda(_) => Err(Stop::Err(ErrorCode::Calc)),
        Value::Scalar(s) => Ok(super::one(*s)),
    }
}

/// `atpList(args, ctx)` (GCD, LCM, MULTINOMIAL): text converts anywhere (non-numeric = `#VALUE!`), boolean =
/// `#VALUE!`, an empty cell = 0 (an area with missing cells adds ONE 0), an omitted argument = `#N/A` except the
/// last one.
pub fn atp_list(args: &CallArgs<'_>, ctx: &mut dyn FnContext, out: &mut Vec<f64>) -> R<()> {
    let n = args.args.len();
    let mut cells: Vec<Scalar> = Vec::new();
    for i in 0..n {
        match arg(args, i) {
            None | Some(Value::Scalar(Scalar::Empty)) => {
                if i == n - 1 && i > 0 {
                    continue;
                }
                return Err(Stop::Err(ErrorCode::NA));
            }
            Some(Value::Ref(r)) => {
                for &area in r.areas() {
                    // the existing cells in order (texts need the context, which the walk borrows)
                    cells.clear();
                    ctx.for_each_cell(area, &mut |v, _, _| cells.push(v))?;
                    for &v in &cells {
                        out.push(match v {
                            Scalar::Empty => 0.0,
                            v => atp_num(ctx, v)?,
                        });
                    }
                    if (cells.len() as u64) < area_size(&area) {
                        out.push(0.0);
                    }
                }
            }
            Some(Value::Array(x)) => {
                for &v in x.data.iter() {
                    out.push(match v {
                        Scalar::Empty => 0.0,
                        v => atp_num(ctx, v)?,
                    });
                }
            }
            Some(Value::Lambda(_)) => return Err(Stop::Err(ErrorCode::Value)),
            Some(Value::Scalar(v)) => out.push(atp_num(ctx, *v)?),
        }
    }
    Ok(())
}
