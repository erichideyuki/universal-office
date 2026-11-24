//! Applying a function to its evaluated arguments (`apply.ts`): argument modes, implicit intersection, the
//! element-wise lifting with its memo, and error propagation. The evaluator calls [`apply_fn`]; the A4 harness
//! (`callFn` over a mock context) does too.

use std::collections::HashMap;

use crate::fnreg::{Arg, ArgMode, CallArgs, EvalResult, FnBody, FnContext, FnIx, FnMeta, Throw};
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, Value, broadcast_at};

/// `ApplyOptions`.
#[derive(Clone, Copy, Debug)]
pub struct ApplyOptions<'a> {
    /// Dynamic/CSE formula (a multi-cell reference in a `value` argument becomes an array, lifted) or legacy
    /// (implicit intersection).
    pub array_context: bool,
    /// Positions omitted in the formula.
    pub omitted: Option<&'a [u32]>,
}

/// A function to apply: its metadata, its body (`None` until frontier A4 ports it) and its row for reports.
#[derive(Clone, Copy, Debug)]
pub struct FnDef<'a> {
    /// Metadata.
    pub meta: &'a FnMeta,
    /// Body.
    pub body: Option<FnBody>,
    /// Row of [`crate::fnreg::FN_TABLE`].
    pub ix: FnIx,
}

/// `implicitIntersection(ref, ctx)`: the cell of a reference on the formula's row/column (`#VALUE!` when it does
/// not cross).
pub fn implicit_intersection(r: &RefValue, ctx: &mut dyn FnContext) -> Result<Scalar, Throw> {
    let Some(a) = r.single() else { return Ok(Scalar::Err(ErrorCode::Value)) };
    let (row, col) = (ctx.row(), ctx.col());
    if a.r1 == a.r2 && a.c1 == a.c2 {
        return ctx.cell(a.sheet, a.r1, a.c1);
    }
    if a.c1 == a.c2 && row >= a.r1 && row <= a.r2 {
        return ctx.cell(a.sheet, row, a.c1);
    }
    if a.r1 == a.r2 && col >= a.c1 && col <= a.c2 {
        return ctx.cell(a.sheet, a.r1, col);
    }
    // a 2D area (usually on another sheet): the cell on the formula's row AND column (measured)
    if row >= a.r1 && row <= a.r2 && col >= a.c1 && col <= a.c2 {
        return ctx.cell(a.sheet, row, col);
    }
    Ok(Scalar::Err(ErrorCode::Value))
}

/// `topLeft(v, ctx)`: the first scalar of a value.
pub fn top_left(v: &Value, ctx: &mut dyn FnContext) -> Result<Scalar, Throw> {
    Ok(match v {
        Value::Array(a) => {
            if a.rows > 0 && a.cols > 0 {
                a.get(0, 0)
            } else {
                Scalar::Err(ErrorCode::Value)
            }
        }
        Value::Ref(r) => return implicit_intersection(r, ctx),
        Value::Lambda(_) => Scalar::Err(ErrorCode::Calc),
        Value::Scalar(s) => *s,
    })
}

/// The value of an argument for the non-lazy modes (`typeof a === 'function' ? a() : a`).
fn forced(a: &Arg, ctx: &mut dyn FnContext) -> EvalResult {
    match a {
        Arg::Val(v) => Ok(v.clone()),
        Arg::Thunk(_) => ctx.force(a),
    }
}

/// `applyFn(def, raw, ctx, opts)`.
/// `raw` is converted in place (the caller recycles the vector).
pub fn apply_fn(def: FnDef<'_>, raw: &mut Vec<Arg>, ctx: &mut dyn FnContext, opts: ApplyOptions<'_>) -> EvalResult {
    let m = def.meta;
    if raw.len() < usize::from(m.min_args) || raw.len() > usize::from(m.max_args) {
        return Ok(Value::Scalar(Scalar::Err(ErrorCode::Value)));
    }
    let args = raw;
    let mut lift_idx: Vec<usize> = Vec::new();
    for (i, slot) in args.iter_mut().enumerate() {
        let mode = m.arg_mode(i);
        if mode == ArgMode::Lazy {
            continue;
        }
        let v = forced(slot, ctx)?;
        let v = match mode {
            ArgMode::Any | ArgMode::Ref | ArgMode::Lazy => v,
            ArgMode::Array => match v {
                Value::Ref(r) => match r.single() {
                    Some(_) => Value::Array(ctx.values(&r, 0)?),
                    None => Value::Scalar(Scalar::Err(ErrorCode::Value)),
                },
                v => v,
            },
            ArgMode::Value => {
                let mut x = v;
                if let Value::Ref(r) = &x {
                    let s = r.single();
                    x = match s {
                        Some(s) if s.r1 == s.r2 && s.c1 == s.c2 => Value::Scalar(ctx.cell(s.sheet, s.r1, s.c1)?),
                        Some(_) if opts.array_context && m.lift() => Value::Array(ctx.values(r, 0)?),
                        _ => Value::Scalar(implicit_intersection(r, ctx)?),
                    };
                }
                if let Value::Lambda(_) = x {
                    x = Value::Scalar(Scalar::Err(ErrorCode::Calc));
                }
                if let Value::Array(a) = &x {
                    if m.lift() {
                        lift_idx.push(i);
                    } else {
                        x = Value::Scalar(if a.rows > 0 && a.cols > 0 { a.get(0, 0) } else { Scalar::Err(ErrorCode::Value) });
                    }
                }
                x
            }
        };
        *slot = Arg::Val(v);
    }
    if lift_idx.is_empty() {
        return call(def, args, ctx, opts);
    }
    // lifting: the largest rows × columns among the arrays
    let (mut rows, mut cols) = (1u32, 1u32);
    for &i in &lift_idx {
        if let Arg::Val(Value::Array(a)) = &args[i] {
            rows = rows.max(a.rows);
            cols = cols.max(a.cols);
        }
    }
    if crate::value::array_too_big(rows, cols) {
        return Err(Throw::Range);
    }
    let arrays: Vec<ArrayValue> = lift_idx
        .iter()
        .map(|&i| match &args[i] {
            Arg::Val(Value::Array(a)) => a.clone(),
            _ => ArrayValue::empty(),
        })
        .collect();
    // repeated elements of a deterministic function give the same result: computed once
    let mut memo: Option<HashMap<Vec<u16>, Scalar>> = (!m.volatile() && u64::from(rows) * u64::from(cols) > 64).then(HashMap::new);
    let mut xs = args.clone();
    let mut key: Vec<u16> = Vec::new();
    let out = ArrayValue::try_fill(rows, cols, |r, c| -> Result<Scalar, Throw> {
        xs.clone_from(args);
        key.clear();
        for (k, &i) in lift_idx.iter().enumerate() {
            let x = broadcast_at(&arrays[k], r, c);
            if memo.is_some() {
                memo_key(&mut key, x, ctx);
            }
            xs[i] = Arg::Val(Value::Scalar(x));
        }
        if let Some(memo) = memo.as_mut() {
            if let Some(hit) = memo.get(&key) {
                return Ok(*hit);
            }
            let v = call(def, &xs, ctx, opts)?;
            let v = top_left(&v, ctx)?;
            memo.insert(key.clone(), v);
            return Ok(v);
        }
        let v = call(def, &xs, ctx, opts)?;
        top_left(&v, ctx)
    })?;
    Ok(Value::Array(out))
}

/// The memo key of one element: `s<text>`, `e<code>`, `n<String(x)>`, `b<true|false>` or `z`, then a space.
fn memo_key(key: &mut Vec<u16>, x: Scalar, ctx: &mut dyn FnContext) {
    let ascii = |key: &mut Vec<u16>, s: &str| key.extend(s.bytes().map(u16::from));
    match x {
        Scalar::Str(r) => {
            key.push(u16::from(b's'));
            key.extend(ctx.text(r).units());
        }
        Scalar::Err(e) => {
            key.push(u16::from(b'e'));
            ascii(key, crate::cell::error_text(e));
        }
        Scalar::Num(n) => {
            key.push(u16::from(b'n'));
            ascii(key, &uo_jsnum::format::to_js_string(n));
        }
        Scalar::Bool(b) => ascii(key, if b { "btrue" } else { "bfalse" }),
        Scalar::Empty => key.push(u16::from(b'z')),
    }
    key.push(0x20);
}

/// `call(xs)`: the first error of a `value` argument unless the function accepts errors, then the body.
fn call(def: FnDef<'_>, xs: &[Arg], ctx: &mut dyn FnContext, opts: ApplyOptions<'_>) -> EvalResult {
    if !def.meta.accepts_errors() {
        for (i, x) in xs.iter().enumerate() {
            if def.meta.arg_mode(i) == ArgMode::Value
                && let Arg::Val(Value::Scalar(Scalar::Err(e))) = x
            {
                return Ok(Value::Scalar(Scalar::Err(*e)));
            }
        }
    }
    let omitted = opts.omitted.filter(|o| !o.is_empty());
    match def.body {
        Some(body) => body(&CallArgs { args: xs, omitted }, ctx),
        None => Err(Throw::NotPorted(def.ix)),
    }
}
