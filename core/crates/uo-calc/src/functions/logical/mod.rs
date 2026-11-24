//! Family `logical` (frontier A4f): `logical.ts` (this file: IF, IFS, IFERROR, IFNA, SWITCH, CHOOSE, AND, OR, XOR,
//! NOT, TRUE, FALSE, the catalogue bodies of LET/LAMBDA/ISOMITTED, MAP, REDUCE, SCAN, BYROW, BYCOL, MAKEARRAY),
//! `info.ts` ([`info`]) and `cube.ts` ([`cube`]). The family contract is in [`super`].
//!
//! The lazy functions force their thunks in the TS order, and a cache the TS keeps with `??=` (a branch, a key or a
//! value that evaluated to `null` is evaluated again at its next use) re-evaluates exactly there ([`Memo`]): the
//! evaluations a thunk makes (random values, dirty precedents) happen as often as in the TS.

mod cube;
mod info;

use crate::apply::{implicit_intersection, top_left};
use crate::coerce::compare_scalars;
use crate::fnreg::{Arg, CallArgs, FnBody, FnContext, Throw};
use crate::jstext::{eq_upper, upper, with_u16};
use crate::key::Area;
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, RefValue, Scalar, StrRef, Value, array_too_big, broadcast_at};

use super::shared::{R, Stop, arg, bodies, boolean, err, is_int, num, scalar, view};

const LOGICAL: &[(&str, FnBody)] = bodies![
    "IF" => if_,
    "IFS" => ifs,
    "IFERROR" => iferror,
    "IFNA" => ifna,
    "SWITCH" => switch,
    "CHOOSE" => choose,
    "AND" => and,
    "OR" => or,
    "XOR" => xor,
    "NOT" => not,
    "TRUE" => true_,
    "FALSE" => false_,
    "LET" => calc_err,
    "LAMBDA" => calc_err,
    "ISOMITTED" => false_,
    "MAP" => map,
    "REDUCE" => reduce,
    "SCAN" => scan,
    "BYROW" => byrow,
    "BYCOL" => bycol,
    "MAKEARRAY" => makearray,
];

/// The bodies of the family: `logical.ts`, `info.ts`, `cube.ts`.
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[LOGICAL, info::INFO, cube::CUBE];

// ---------------------------------------------------------------- values

/// `Scalar | ArrayValue`: what `scalarOrArray` gives.
#[derive(Clone, Debug)]
enum Sa {
    S(Scalar),
    A(ArrayValue),
}

impl Sa {
    /// `at(x, r, c)`: a scalar repeats; an array broadcasts (a dimension of 1 repeats, outside → `#N/A`).
    fn at(&self, r: u32, c: u32) -> Scalar {
        match self {
            Self::S(s) => *s,
            Self::A(a) => broadcast_at(a, r, c),
        }
    }
}

/// `scalarOrArray(v, ctx)`: a one-cell reference → the cell; a larger one → the implicit intersection in a legacy
/// formula, its values otherwise (a union → `#VALUE!`); a LAMBDA → `#CALC!`.
fn sa(v: Value, ctx: &mut dyn FnContext) -> Result<Sa, Throw> {
    Ok(match v {
        Value::Scalar(s) => Sa::S(s),
        Value::Array(a) => Sa::A(a),
        Value::Lambda(_) => Sa::S(Scalar::Err(ErrorCode::Calc)),
        Value::Ref(r) => {
            let a = r.single();
            if let Some(a) = a
                && a.r1 == a.r2
                && a.c1 == a.c2
            {
                return Ok(Sa::S(ctx.cell(a.sheet, a.r1, a.c1)?));
            }
            if ctx.array_context() == Some(false) {
                return Ok(Sa::S(implicit_intersection(&r, ctx)?));
            }
            if a.is_none() {
                return Ok(Sa::S(Scalar::Err(ErrorCode::Value)));
            }
            Sa::A(ctx.values(&r, 0)?)
        }
    })
}

/// `dims(xs)`: the common size of a broadcast (at least 1 × 1).
fn dims<'a>(xs: impl IntoIterator<Item = &'a Sa>) -> (u32, u32) {
    let (mut rows, mut cols) = (1, 1);
    for x in xs {
        if let Sa::A(a) = x {
            rows = rows.max(a.rows);
            cols = cols.max(a.cols);
        }
    }
    (rows, cols)
}

/// `ArrayValue.fill(rows, cols, f)` of a result whose cells can throw; past the engine's array cap a `RangeError`
/// (decision A4a-4: the TS allocates up to 2^32 − 1 cells).
fn fill(rows: u32, cols: u32, f: impl FnMut(u32, u32) -> Result<Scalar, Throw>) -> R<Value> {
    if array_too_big(rows, cols) {
        return Err(Stop::Throw(Throw::Range));
    }
    Ok(Value::Array(ArrayValue::try_fill(rows, cols, f)?))
}

/// A lazily computed value the TS caches with `x ??= f()`: a `null` result is not kept (the next use evaluates again).
struct Memo(Option<Sa>);

impl Memo {
    const NONE: Self = Self(None);

    fn get(&mut self, f: impl FnOnce() -> Result<Sa, Throw>) -> Result<&Sa, Throw> {
        if matches!(self.0, None | Some(Sa::S(Scalar::Empty))) {
            self.0 = Some(f()?);
        }
        Ok(self.0.get_or_insert(Sa::S(Scalar::Empty)))
    }
}

// ---------------------------------------------------------------- thunks

/// `t()` of a lazy argument (an omitted one is `null`; a value where a thunk is expected is the TS `TypeError`).
fn force(a: &CallArgs<'_>, i: usize, ctx: &mut dyn FnContext) -> Result<Value, Throw> {
    match a.args.get(i) {
        Some(x) => ctx.force(x),
        None => Err(Throw::Internal("TypeError: t is not a function")),
    }
}

/// `ofThunk(t, missing)`: absent → `missing`; omitted → 0; else `t()`.
fn of_thunk(a: &CallArgs<'_>, i: usize, missing: Scalar, ctx: &mut dyn FnContext) -> Result<Value, Throw> {
    match a.args.get(i) {
        None => Ok(Value::Scalar(missing)),
        Some(Arg::Thunk(t)) if t.omitted => Ok(num(0.0)),
        Some(x) => ctx.force(x),
    }
}

// ---------------------------------------------------------------- booleans

/// `textToBool(s, ctx)`: only the name of the boolean in the locale, case-insensitive, untrimmed.
fn text_to_bool(ctx: &dyn FnContext, s: StrRef) -> Option<bool> {
    let loc = ctx.locale();
    with_u16(ctx.text(s), |u| {
        let up = upper(u);
        if eq_upper(&up, &loc.true_text) {
            Some(true)
        } else if eq_upper(&up, &loc.false_text) {
            Some(false)
        } else {
            None
        }
    })
}

/// `boolOf(v, ctx)` (the condition of IF/IFS, NOT).
fn bool_of(ctx: &dyn FnContext, v: Scalar) -> Result<bool, ErrorCode> {
    match v {
        Scalar::Err(e) => Err(e),
        Scalar::Str(s) => text_to_bool(ctx, s).ok_or(ErrorCode::Value),
        Scalar::Num(n) => Ok(n != 0.0),
        Scalar::Bool(b) => Ok(b),
        Scalar::Empty => Ok(false),
    }
}

// ---------------------------------------------------------------- IF, IFS

fn if_(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let c = force(a, 0, ctx)?;
    let then = |ctx: &mut dyn FnContext| of_thunk(a, 1, Scalar::Empty, ctx);
    let other = |ctx: &mut dyn FnContext| of_thunk(a, 2, Scalar::Bool(false), ctx);
    match sa(c, ctx)? {
        Sa::A(cv) => pick_array(&cv, a, ctx),
        Sa::S(cv) => {
            let b = bool_of(ctx, cv)?;
            Ok(if b { then(ctx)? } else { other(ctx)? })
        }
    }
}

/// `pickArray(c, [then, else], ctx)`: each branch evaluated once (in the order the condition first uses them), the
/// result chosen element by element.
fn pick_array(c: &ArrayValue, a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut vals = [Memo::NONE, Memo::NONE];
    let mut get = |i: usize, ctx: &mut dyn FnContext| -> Result<Sa, Throw> {
        let v = vals[i].get(|| {
            let v = if i == 0 { of_thunk(a, 1, Scalar::Empty, ctx)? } else { of_thunk(a, 2, Scalar::Bool(false), ctx)? };
            sa(v, ctx)
        })?;
        Ok(v.clone())
    };
    // the size: the condition broadcast with the branches it uses (in the order of first use)
    let mut used: [Option<usize>; 2] = [None, None];
    for &v in c.data.iter() {
        if let Ok(b) = bool_of(ctx, v) {
            let k = usize::from(!b);
            if !used.contains(&Some(k)) {
                let slot = usize::from(used[0].is_some());
                used[slot] = Some(k);
                if slot == 1 {
                    break;
                }
            }
        }
    }
    let mut got: [Option<Sa>; 2] = [None, None];
    for (slot, k) in used.iter().enumerate() {
        if let Some(k) = *k {
            got[slot] = Some(get(k, ctx)?);
        }
    }
    let cs = Sa::A(c.clone());
    let (rows, cols) = dims(core::iter::once(&cs).chain(got.iter().flatten()));
    drop(got);
    fill(rows, cols, |r, cc| {
        let b = match bool_of(ctx, broadcast_at(c, r, cc)) {
            Ok(b) => b,
            Err(e) => return Ok(Scalar::Err(e)),
        };
        Ok(get(usize::from(!b), ctx)?.at(r, cc))
    })
}

fn ifs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = a.args.len();
    if n % 2 == 1 {
        return Err(ErrorCode::NA.into());
    }
    let cond_at = |i: usize, ctx: &mut dyn FnContext| -> Result<Sa, Throw> {
        match a.args.get(i) {
            Some(Arg::Thunk(t)) if t.omitted => Ok(Sa::S(Scalar::Empty)),
            _ => sa(force(a, i, ctx)?, ctx),
        }
    };
    let mut i = 0;
    while i < n {
        match cond_at(i, ctx)? {
            Sa::A(first) => {
                // an array condition: decided element by element with the remaining conditions
                let mut cs: Vec<Sa> = Vec::with_capacity((n - i) / 2);
                cs.push(Sa::A(first));
                let mut j = i + 2;
                while j < n {
                    cs.push(cond_at(j, ctx)?);
                    j += 2;
                }
                let mut cache: Vec<Memo> = (0..cs.len()).map(|_| Memo::NONE).collect();
                let (rows, cols) = dims(&cs);
                return fill(rows, cols, |r, c| {
                    for (k, ck) in cs.iter().enumerate() {
                        match bool_of(ctx, ck.at(r, c)) {
                            Err(e) => return Ok(Scalar::Err(e)),
                            Ok(true) => {
                                let at = i + 2 * k + 1;
                                let v = cache[k].get(|| sa(of_thunk(a, at, Scalar::Empty, ctx)?, ctx))?;
                                return Ok(v.at(r, c));
                            }
                            Ok(false) => {}
                        }
                    }
                    Ok(Scalar::Err(ErrorCode::NA))
                });
            }
            Sa::S(cv) => {
                if bool_of(ctx, cv)? {
                    return Ok(of_thunk(a, i + 1, Scalar::Empty, ctx)?);
                }
            }
        }
        i += 2;
    }
    Err(ErrorCode::NA.into())
}

// ---------------------------------------------------------------- IFERROR, IFNA

fn iferror(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    replace_errors(a, ctx, |_| true)
}

fn ifna(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    replace_errors(a, ctx, |e| e == ErrorCode::NA)
}

/// `replaceErrors(v, alt, ctx, hit)`.
fn replace_errors(a: &CallArgs<'_>, ctx: &mut dyn FnContext, hit: impl Fn(ErrorCode) -> bool) -> R<Value> {
    let v = of_thunk(a, 0, Scalar::Empty, ctx)?;
    match sa(v, ctx)? {
        Sa::A(x) => {
            if !x.data.iter().any(|e| matches!(*e, Scalar::Err(e) if hit(e))) {
                return Ok(Value::Array(x));
            }
            let alt = sa(of_thunk(a, 1, Scalar::Empty, ctx)?, ctx)?;
            let (rows, cols) = match &alt {
                Sa::A(_) => dims([&Sa::A(x.clone()), &alt]),
                Sa::S(_) => (x.rows, x.cols),
            };
            fill(rows, cols, |r, c| {
                Ok(match broadcast_at(&x, r, c) {
                    Scalar::Err(e) if hit(e) => alt.at(r, c),
                    e => e,
                })
            })
        }
        Sa::S(Scalar::Err(e)) if hit(e) => Ok(of_thunk(a, 1, Scalar::Empty, ctx)?),
        Sa::S(x) => Ok(Value::Scalar(x)),
    }
}

// ---------------------------------------------------------------- SWITCH, CHOOSE

fn switch(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ev = sa(of_thunk(a, 0, Scalar::Empty, ctx)?, ctx)?;
    let pairs = a.args.len() - 1;
    let n = pairs / 2;
    match ev {
        // one value: each key is used at most once, no cache needed
        Sa::S(x) => Ok(switch_one(a, ctx, x, n, &mut |i, ctx| key_of(a, i, ctx))?),
        Sa::A(ev) => {
            let mut keys: Vec<Memo> = (0..n).map(|_| Memo::NONE).collect();
            fill(ev.rows, ev.cols, |r, c| {
                let v = switch_one(a, ctx, ev.get(r, c), n, &mut |i, ctx| Ok(keys[i].get(|| key_of(a, i, ctx))?.clone()))?;
                top_left(&v, ctx)
            })
        }
    }
}

/// The key `i` of SWITCH (`scalarOrArray(ofThunk(args[1 + 2i], null))`).
fn key_of(a: &CallArgs<'_>, i: usize, ctx: &mut dyn FnContext) -> Result<Sa, Throw> {
    sa(of_thunk(a, 1 + 2 * i, Scalar::Empty, ctx)?, ctx)
}

/// `one(x)` of SWITCH.
fn switch_one(
    a: &CallArgs<'_>,
    ctx: &mut dyn FnContext,
    x: Scalar,
    n: usize,
    key: &mut dyn FnMut(usize, &mut dyn FnContext) -> Result<Sa, Throw>,
) -> Result<Value, Throw> {
    if let Scalar::Err(e) = x {
        return Ok(err(e));
    }
    for i in 0..n {
        let kv = match key(i, ctx)? {
            Sa::A(k) => {
                if k.rows > 0 && k.cols > 0 {
                    k.get(0, 0)
                } else {
                    Scalar::Err(ErrorCode::Value)
                }
            }
            Sa::S(k) => k,
        };
        if let Scalar::Err(e) = kv {
            return Ok(err(e));
        }
        if same_value(ctx, x, kv) {
            return of_thunk(a, 2 + 2 * i, Scalar::Empty, ctx);
        }
    }
    let has_default = (a.args.len() - 1) % 2 == 1;
    if has_default { of_thunk(a, a.args.len() - 1, Scalar::Empty, ctx) } else { Ok(err(ErrorCode::NA)) }
}

/// `sameValue(a, b)` of SWITCH: empty = 0, different types never equal, texts by the collation.
fn same_value(ctx: &dyn FnContext, a: Scalar, b: Scalar) -> bool {
    let z = |s: Scalar| if let Scalar::Empty = s { Scalar::Num(0.0) } else { s };
    let (a, b) = (z(a), z(b));
    if core::mem::discriminant(&a) != core::mem::discriminant(&b) {
        return false;
    }
    compare_scalars(view(ctx, a), view(ctx, b)) == 0
}

fn choose(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let idx = scalar(a, 0).unwrap_or(Scalar::Empty);
    let k = match idx {
        Scalar::Str(s) => number_text(ctx, ctx.text(s))?.trunc(),
        Scalar::Num(n) => n.trunc(),
        Scalar::Bool(b) => f64::from(u8::from(b)),
        Scalar::Empty => 0.0,
        Scalar::Err(_) => f64::NAN,
    };
    let opts = crate::cast::u53_to_f64(a.args.len() as u64 - 1);
    if !(k >= 1.0 && k <= opts) {
        return Err(ErrorCode::Value.into());
    }
    let i = crate::cast::trunc_usize(k);
    Ok(of_thunk(a, i, Scalar::Empty, ctx)?)
}

/// `toNumberText(s, ctx)`: `Number` of the trimmed text with the locale's group separators removed and its first
/// decimal separator turned into a point; empty or not finite → `#VALUE!`.
fn number_text(ctx: &dyn FnContext, s: JsStr<'_>) -> Result<f64, ErrorCode> {
    let n = with_u16(s, |u| {
        let t = crate::jstext::js_trim(u);
        if t.is_empty() {
            return None;
        }
        let loc = ctx.locale();
        let dec: Vec<u16> = loc.decimal.as_js().units().collect();
        if dec == [u16::from(b'.')] {
            return Some(crate::cell::js_number(JsStr::Utf16(t)));
        }
        let group: Vec<u16> = loc.group.as_js().units().collect();
        // t.split(group).join('')
        let mut v: Vec<u16> = Vec::with_capacity(t.len() + 1);
        let mut i = 0;
        while i < t.len() {
            if !group.is_empty() && t[i..].starts_with(&group) {
                i += group.len();
            } else {
                v.push(t[i]);
                i += 1;
            }
        }
        // .replace(decimal, '.'): the first occurrence (an empty decimal inserts the point in front)
        if dec.is_empty() {
            v.insert(0, u16::from(b'.'));
        } else if let Some(p) = v.windows(dec.len()).position(|w| w == dec.as_slice()) {
            v.splice(p..p + dec.len(), [u16::from(b'.')]);
        }
        Some(crate::cell::js_number(JsStr::Utf16(&v)))
    });
    match n {
        Some(x) if x.is_finite() => Ok(x),
        _ => Err(ErrorCode::Value),
    }
}

// ---------------------------------------------------------------- AND, OR, XOR, NOT, TRUE, FALSE

#[derive(Clone, Copy)]
enum Op {
    And,
    Or,
    Xor,
}

/// The state of `logicalFold`.
struct Fold {
    op: Op,
    acc: bool,
    any: bool,
    err: Option<ErrorCode>,
}

impl Fold {
    fn put(&mut self, b: bool) {
        self.acc = match self.op {
            Op::And => self.acc && b,
            Op::Or => self.acc || b,
            Op::Xor => self.acc != b,
        };
        self.any = true;
    }

    /// `take(v, false)`: a value of a reference or an array (text and empty ignored).
    fn take(&mut self, v: Scalar) {
        if self.err.is_some() {
            return;
        }
        match v {
            Scalar::Err(e) => self.err = Some(e),
            Scalar::Bool(b) => self.put(b),
            Scalar::Num(n) => self.put(n != 0.0),
            Scalar::Str(_) | Scalar::Empty => {}
        }
    }
}

fn and(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    logical_fold(a, ctx, Op::And, true)
}

fn or(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    logical_fold(a, ctx, Op::Or, false)
}

fn xor(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    logical_fold(a, ctx, Op::Xor, false)
}

/// `logicalFold(args, ctx, f, init)`: booleans and numbers; a text given directly counts when it names a boolean in
/// the locale; texts and empty cells of references and arrays are ignored; an omitted argument is FALSE.
fn logical_fold(a: &CallArgs<'_>, ctx: &mut dyn FnContext, op: Op, init: bool) -> R<Value> {
    let mut f = Fold { op, acc: init, any: false, err: None };
    for i in 0..a.args.len() {
        match arg(a, i).unwrap_or(&super::shared::EMPTY) {
            Value::Ref(r) => {
                for &area in r.areas() {
                    if area.r1 == area.r2 && area.c1 == area.c2 {
                        // one cell: what `forEachCell` visits (an absent cell reads empty, which is ignored)
                        let v = ctx.cell(area.sheet, area.r1, area.c1)?;
                        f.take(v);
                    } else {
                        ctx.for_each_cell(area, &mut |v, _, _| f.take(v))?;
                    }
                }
            }
            Value::Array(x) => {
                for &v in x.data.iter() {
                    f.take(v);
                }
            }
            Value::Lambda(_) => {
                if f.err.is_none() {
                    f.err = Some(ErrorCode::Value);
                }
            }
            // an omitted argument (=AND(TRUE;)) counts as FALSE
            Value::Scalar(Scalar::Empty) => {
                if f.err.is_none() {
                    f.put(false);
                }
            }
            Value::Scalar(Scalar::Str(s)) => {
                if f.err.is_none()
                    && let Some(b) = text_to_bool(ctx, *s)
                {
                    f.put(b);
                }
            }
            Value::Scalar(s) => f.take(*s),
        }
        if let Some(e) = f.err {
            return Err(e.into());
        }
    }
    if f.any { Ok(boolean(f.acc)) } else { Err(ErrorCode::Value.into()) }
}

fn not(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(!bool_of(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?))
}

fn true_(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(true))
}

fn false_(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Ok(boolean(false))
}

/// LET and LAMBDA are special forms of the evaluator; their registered bodies (reached only as a function used as a
/// value) answer `#CALC!`.
fn calc_err(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    Err(ErrorCode::Calc.into())
}

// ---------------------------------------------------------------- MAP, REDUCE, SCAN, BYROW, BYCOL, MAKEARRAY

/// The input of MAP/REDUCE/SCAN/BYROW/BYCOL (`lambdaInput`): from a range the LAMBDA receives REFERENCES.
enum Input {
    Ref(Area),
    Arr(ArrayValue),
}

impl Input {
    fn rows(&self) -> u32 {
        match self {
            Self::Ref(a) => a.r2 - a.r1 + 1,
            Self::Arr(m) => m.rows,
        }
    }

    fn cols(&self) -> u32 {
        match self {
            Self::Ref(a) => a.c2 - a.c1 + 1,
            Self::Arr(m) => m.cols,
        }
    }

    fn get(&self, r: u32, c: u32) -> Value {
        match self {
            Self::Ref(a) => {
                let (row, col) = (a.r1 + r, a.c1 + c);
                Value::Ref(RefValue::One(Area { sheet: a.sheet, r1: row, c1: col, r2: row, c2: col }))
            }
            Self::Arr(m) => Value::Scalar(m.get(r, c)),
        }
    }

    fn line(&self, i: u32, by_row: bool) -> Value {
        match self {
            Self::Ref(a) => Value::Ref(RefValue::One(if by_row {
                Area { r1: a.r1 + i, r2: a.r1 + i, ..*a }
            } else {
                Area { c1: a.c1 + i, c2: a.c1 + i, ..*a }
            })),
            Self::Arr(m) => Value::Array(if by_row {
                ArrayValue::fill(1, m.cols, |_, c| m.get(i, c))
            } else {
                ArrayValue::fill(m.rows, 1, |r, _| m.get(r, i))
            }),
        }
    }
}

fn lambda_input(v: &Value) -> Result<Input, ErrorCode> {
    match v {
        Value::Ref(r) => r.single().map(Input::Ref).ok_or(ErrorCode::Value),
        Value::Lambda(_) => Err(ErrorCode::Value),
        Value::Array(m) => Ok(Input::Arr(m.clone())),
        Value::Scalar(s) => Ok(Input::Arr(ArrayValue::new(1, 1, vec![*s]))),
    }
}

/// The LAMBDA of a body, with the TS check of its parameter count (`fn.params.length !== k && typeof fn.body !==
/// 'function'` → `#VALUE!`; `None` = no check).
fn lambda_of(ctx: &dyn FnContext, v: Option<&Value>, params: Option<u32>) -> Result<u32, ErrorCode> {
    let Some(&Value::Lambda(f)) = v else { return Err(ErrorCode::Value) };
    if let Some(k) = params {
        let (n, native) = ctx.lambda_arity(f);
        if n != k && !native {
            return Err(ErrorCode::Value);
        }
    }
    Ok(f)
}

/// `fillLambda(rows, cols, ctx, f)`: a nested array result makes the WHOLE result `#CALC!` (the cells after it are
/// not evaluated).
fn fill_lambda(
    rows: u32,
    cols: u32,
    ctx: &mut dyn FnContext,
    mut f: impl FnMut(u32, u32, &mut dyn FnContext) -> Result<Value, Throw>,
) -> R<Value> {
    if array_too_big(rows, cols) {
        return Err(Stop::Throw(Throw::Range));
    }
    let n = rows as usize * cols as usize;
    let mut data: Vec<Scalar> = Vec::with_capacity(n.min(1 << 16));
    for r in 0..rows {
        for c in 0..cols {
            let v = f(r, c, ctx)?;
            match lambda_scalar(v, ctx)? {
                Some(s) => data.push(s),
                None => return Err(ErrorCode::Calc.into()),
            }
        }
    }
    Ok(Value::Array(ArrayValue::new(rows, cols, data)))
}

/// `lambdaScalar(v, ctx)`: a LAMBDA call's result put in a cell; `None` = a nested array.
fn lambda_scalar(v: Value, ctx: &mut dyn FnContext) -> Result<Option<Scalar>, Throw> {
    let z = |s: Scalar| if let Scalar::Empty = s { Scalar::Num(0.0) } else { s };
    Ok(match v {
        Value::Array(a) => (a.rows == 1 && a.cols == 1).then(|| z(a.get(0, 0))),
        Value::Ref(r) => match r.single() {
            Some(a) if a.r1 == a.r2 && a.c1 == a.c2 => Some(z(ctx.cell(a.sheet, a.r1, a.c1)?)),
            _ => None,
        },
        Value::Lambda(_) => Some(Scalar::Err(ErrorCode::Calc)),
        Value::Scalar(s) => Some(z(s)),
    })
}

/// A call of `f` with the arguments as a [`Value`] slice.
fn call(ctx: &mut dyn FnContext, f: u32, args: &[Value]) -> Result<Value, Throw> {
    ctx.call_lambda(f, args)
}

fn map(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = a.args.len();
    let Some(&Value::Lambda(_)) = arg(a, n - 1) else { return Err(ErrorCode::Value.into()) };
    let mut xs: Vec<Input> = Vec::with_capacity(n - 1);
    let mut first = None;
    for i in 0..n - 1 {
        match lambda_input(arg(a, i).unwrap_or(&super::shared::EMPTY)) {
            Ok(x) => xs.push(x),
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    if let Some(e) = first {
        return Err(e.into());
    }
    let f = lambda_of(ctx, arg(a, n - 1), Some(u32::try_from(xs.len()).unwrap_or(u32::MAX)))?;
    let rows = xs.iter().map(Input::rows).max().unwrap_or(0);
    let cols = xs.iter().map(Input::cols).max().unwrap_or(0);
    let mut vals: Vec<Value> = Vec::with_capacity(xs.len());
    fill_lambda(rows, cols, ctx, |r, c, ctx| {
        vals.clear();
        for x in &xs {
            vals.push(if x.rows() == 1 && x.cols() == 1 {
                x.get(0, 0)
            } else if r < x.rows() && c < x.cols() {
                x.get(r, c)
            } else {
                err(ErrorCode::NA)
            });
        }
        call(ctx, f, &vals)
    })
}

/// `[init, arr, fn]` of REDUCE/SCAN, checked: the LAMBDA takes two parameters, the input is a range or an array.
fn fold_args(a: &CallArgs<'_>, ctx: &dyn FnContext) -> Result<(Value, Input, u32), ErrorCode> {
    let f = lambda_of(ctx, arg(a, 2), Some(2))?;
    let x = lambda_input(arg(a, 1).unwrap_or(&super::shared::EMPTY))?;
    Ok((arg(a, 0).cloned().unwrap_or(Value::Scalar(Scalar::Empty)), x, f))
}

fn reduce(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut acc, x, f) = fold_args(a, ctx)?;
    // the TS walks every cell (1.7·10^10 calls for a whole sheet): bounded as the arrays (decision A4f-3)
    if array_too_big(x.rows(), x.cols()) {
        return Err(Stop::Throw(Throw::Range));
    }
    for r in 0..x.rows() {
        for c in 0..x.cols() {
            let args = [acc, x.get(r, c)];
            acc = call(ctx, f, &args)?;
        }
    }
    Ok(acc)
}

fn scan(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (mut acc, x, f) = fold_args(a, ctx)?;
    fill_lambda(x.rows(), x.cols(), ctx, |r, c, ctx| {
        let args = [core::mem::replace(&mut acc, Value::Scalar(Scalar::Empty)), x.get(r, c)];
        acc = call(ctx, f, &args)?;
        Ok(acc.clone())
    })
}

fn byrow(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    by_line(a, ctx, true)
}

fn bycol(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    by_line(a, ctx, false)
}

/// `byLine(arr, fn, ctx, rows)`.
fn by_line(a: &CallArgs<'_>, ctx: &mut dyn FnContext, rows: bool) -> R<Value> {
    let f = lambda_of(ctx, arg(a, 1), Some(1))?;
    let x = lambda_input(arg(a, 0).unwrap_or(&super::shared::EMPTY))?;
    let n = if rows { x.rows() } else { x.cols() };
    let (h, w) = if rows { (n, 1) } else { (1, n) };
    fill_lambda(h, w, ctx, |r, c, ctx| {
        let args = [x.line(if rows { r } else { c }, rows)];
        call(ctx, f, &args)
    })
}

fn makearray(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let f = lambda_of(ctx, arg(a, 2), None)?;
    let n = to_int(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?;
    let m = to_int(ctx, scalar(a, 1).unwrap_or(Scalar::Empty))?;
    // not an integer or < 1 → #VALUE! (measured: MAKEARRAY(1,9;…) → #VALUE!)
    if n < 1.0 || m < 1.0 || !is_int(n) || !is_int(m) {
        return Err(ErrorCode::Value.into());
    }
    if n * m > 1_048_576.0 * 16_384.0 {
        return Err(ErrorCode::Num.into());
    }
    // past the array cap a RangeError (the TS `new Array(n·m)` throws past 2^32 − 1; decision A4a-4)
    if n * m > crate::cast::u53_to_f64(crate::value::MAX_ARRAY_CELLS) {
        return Err(Stop::Throw(Throw::Range));
    }
    let (n, m) = (crate::cast::to_u32(n), crate::cast::to_u32(m));
    fill_lambda(n, m, ctx, |r, c, ctx| {
        let args = [num(f64::from(r + 1)), num(f64::from(c + 1))];
        call(ctx, f, &args)
    })
}

/// `toInt(v, ctx)`: the number of an argument (not truncated).
fn to_int(ctx: &dyn FnContext, v: Scalar) -> Result<f64, ErrorCode> {
    match v {
        Scalar::Err(e) => Err(e),
        Scalar::Num(n) => Ok(n),
        Scalar::Bool(b) => Ok(f64::from(u8::from(b))),
        Scalar::Empty => Ok(0.0),
        Scalar::Str(s) => number_text(ctx, ctx.text(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_keeps_values_but_not_null() {
        let mut m = Memo::NONE;
        let mut calls = 0;
        for _ in 0..3 {
            let _ = m.get(|| {
                calls += 1;
                Ok(Sa::S(Scalar::Empty))
            });
        }
        assert_eq!(calls, 3);
        let mut m = Memo::NONE;
        let mut calls = 0;
        for _ in 0..3 {
            let _ = m.get(|| {
                calls += 1;
                Ok(Sa::S(Scalar::Num(1.0)))
            });
        }
        assert_eq!(calls, 1);
    }
}
