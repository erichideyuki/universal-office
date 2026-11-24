//! Family `finance` (frontier A4c): `functions/financial.ts` ([`financial`]: time value of money, cash flows,
//! depreciation, bonds), `functions/daycount.ts` ([`daycount`]: the ATP day counts and coupon dates, no bodies of its
//! own) and `functions/datetime.ts` ([`datetime`]: DATE … YEARFRAC, NOW/TODAY through the host clock). The calendar is
//! [`crate::dates`] and the numeric support [`crate::math_util`] (A1); the factories and walkers are
//! [`super::shared`].
//!
//! Every body is the TS one in the same order of floating-point operations. The TS loops without a bound of their
//! own carry a step budget here (decision A4c-2 of the record `uo-calc.md` "A4c — as built"); this file holds the
//! family's support: the budget counter, a small-vector of cash flows, the TS `TypeError` of a JS object reaching
//! `toNumber`, and the allocation-free `liftAtp` of the date bodies.

mod datetime;
pub mod daycount;
mod financial;

use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::tdl::AtpArg;
use crate::functions::shared::{EMPTY, R, Stop, arg, top};
use crate::value::{ArrayValue, ErrorCode, Scalar, Value, broadcast_at};

/// The bodies of the family: `financial.ts`, `datetime.ts` (`daycount.ts` registers no function).
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[financial::BODIES, datetime::BODIES];

/// The step budget of the TS loops that have no bound of their own and run once per period (CUMIPMT/CUMPRINC, DB,
/// VDB, AMORDEGRC): 2^22 steps (≈ 10–100 ms); past it `#NUM!` (the TS takes seconds, or never returns).
pub(crate) const LOOP_BUDGET: u32 = 1 << 22;

/// The budget of powers computed by the iterative bond solvers (YIELD, ODDFYIELD): 2^23 — dates up to 31/12/9999
/// (≤ 32,400 coupons) never reach it even when every iteration runs.
pub(crate) const PRICE_BUDGET: f64 = 8_388_608.0;

/// A step counter with a cap (`tick` fails with `#NUM!` past it).
pub(crate) struct Steps {
    left: u32,
}

impl Steps {
    /// A counter of `cap` steps.
    pub(crate) const fn new(cap: u32) -> Self {
        Self { left: cap }
    }
    /// Whether `n` more steps fit (a loop whose count is known fails at once instead of at the cap; a NaN count is a
    /// loop that does not run).
    #[inline]
    pub(crate) fn fits(&self, n: f64) -> bool {
        n.is_nan() || n <= f64::from(self.left)
    }
    /// One step.
    #[inline]
    pub(crate) fn tick(&mut self) -> Result<(), ErrorCode> {
        match self.left.checked_sub(1) {
            Some(l) => {
                self.left = l;
                Ok(())
            }
            None => Err(ErrorCode::Num),
        }
    }
}

/// The TS `TypeError` of a JS object (an array or a LAMBDA) reaching `toNumber` (`s.slice is not a function`).
pub(crate) fn type_error() -> Stop {
    Stop::Throw(Throw::Internal("TypeError: s.slice is not a function"))
}

/// The cash flows of IRR/MIRR: on the stack up to 64 values, on the heap past them (no allocation per call for the
/// usual windows).
pub(crate) struct F64Buf {
    st: [f64; 64],
    n: usize,
    heap: Vec<f64>,
}

impl F64Buf {
    /// Empty.
    pub(crate) const fn new() -> Self {
        Self { st: [0.0; 64], n: 0, heap: Vec::new() }
    }
    /// `push`.
    pub(crate) fn push(&mut self, x: f64) {
        if self.heap.is_empty() {
            if let Some(d) = self.st.get_mut(self.n) {
                *d = x;
                self.n += 1;
                return;
            }
            self.heap.reserve(self.n * 2);
            self.heap.extend_from_slice(&self.st);
        }
        self.heap.push(x);
    }
    /// The values.
    pub(crate) fn as_slice(&self) -> &[f64] {
        if self.heap.is_empty() { &self.st[..self.n] } else { &self.heap }
    }
}

/// The most arguments of a date ATP body (NETWORKDAYS.INTL, WORKDAY.INTL).
const MAX_ATP: usize = 4;

/// `liftAtp(args, ctx, f, pass)` of `tdl-util.ts` ([`crate::functions::shared::tdl::lift_atp`]) without the per-call
/// vector: the arguments live on the stack (≤ 4); `pass` = the one position that arrives unlifted (the holidays).
/// Shared-layer candidate (the record lists it).
pub(crate) fn lift_atp(
    a: &CallArgs<'_>,
    ctx: &mut dyn FnContext,
    pass: Option<usize>,
    mut f: impl FnMut(&[AtpArg], &mut dyn FnContext) -> R<Value>,
) -> R<Value> {
    let n = a.args.len().min(MAX_ATP);
    let mut xs: [AtpArg; MAX_ATP] = core::array::from_fn(|_| AtpArg::Omitted);
    let mut arrs: [Option<ArrayValue>; MAX_ATP] = Default::default();
    let mut any_array = false;
    for i in 0..n {
        let v = arg(a, i).unwrap_or(&EMPTY);
        if pass == Some(i) {
            xs[i] = AtpArg::Raw(v.clone());
            continue;
        }
        xs[i] = match v {
            Value::Scalar(Scalar::Empty) => AtpArg::Omitted,
            Value::Scalar(s) => AtpArg::S(*s),
            Value::Ref(r) => match r.single() {
                Some(x) if x.r1 == x.r2 && x.c1 == x.c2 => AtpArg::S(ctx.cell(x.sheet, x.r1, x.c1)?),
                _ => return Err(Stop::Err(ErrorCode::Value)),
            },
            Value::Lambda(_) => AtpArg::S(Scalar::Err(ErrorCode::Calc)),
            Value::Array(x) => {
                arrs[i] = Some(x.clone());
                any_array = true;
                AtpArg::S(Scalar::Empty)
            }
        };
    }
    if !any_array {
        return f(&xs[..n], ctx);
    }
    let (mut rows, mut cols) = (1u32, 1u32);
    for x in arrs.iter().flatten() {
        rows = rows.max(x.rows);
        cols = cols.max(x.cols);
    }
    if crate::value::array_too_big(rows, cols) {
        return Err(Stop::Throw(Throw::Range));
    }
    let out = ArrayValue::try_fill(rows, cols, |r, c| -> R<Scalar> {
        let mut ys = xs.clone();
        for (y, x) in ys.iter_mut().zip(&arrs) {
            if let Some(x) = x {
                *y = AtpArg::S(broadcast_at(x, r, c));
            }
        }
        Ok(top(&f(&ys[..n], ctx)?))
    })?;
    Ok(Value::Array(out))
}
