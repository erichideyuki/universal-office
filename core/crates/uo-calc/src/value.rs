//! Calc values (`values.ts`): scalars, arrays, references, lambdas. Error codes are a `u8` enum (the TS
//! interns one `CalcError` object per code, `values.ts:17-26`); strings are [`StrRef`]s, never owned heap
//! strings per cell.

use crate::key::Area;
use crate::strings::StrId;

/// Canonical error codes in `ERROR.TYPE` order (`values.ts:44-48`; the discriminant IS the number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum ErrorCode {
    /// `#NULL!`
    Null = 1,
    /// `#DIV/0!`
    Div0 = 2,
    /// `#VALUE!`
    Value = 3,
    /// `#REF!`
    Ref = 4,
    /// `#NAME?`
    Name = 5,
    /// `#NUM!`
    Num = 6,
    /// `#N/A`
    NA = 7,
    /// `#GETTING_DATA`
    GettingData = 8,
    /// `#SPILL!`
    Spill = 9,
    /// `#CONNECT!`
    Connect = 10,
    /// `#BLOCKED!`
    Blocked = 11,
    /// `#UNKNOWN!`
    Unknown = 12,
    /// `#FIELD!`
    Field = 13,
    /// `#CALC!`
    Calc = 14,
    /// `#BUSY!`
    Busy = 15,
    /// `#PYTHON!`
    Python = 16,
}

/// A string inside a value: interned (a cell's, free to copy) or a temporary of the running recalc (an
/// index into the engine's per-pass arena, cleared at the end of `run`). No reference counting per read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrRef {
    /// Interned in the store's pool.
    Pooled(StrId),
    /// Built by the evaluation; valid until the end of the pass that made it.
    Temp(u32),
}

/// A scalar (`values.ts:51`): `Empty` is the TS `null` (empty cell / omitted argument). 16 bytes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scalar {
    /// `null`.
    Empty,
    /// An IEEE-754 number (`-0` and every NaN payload kept as the TS keeps them).
    Num(f64),
    /// Text.
    Str(StrRef),
    /// `TRUE`/`FALSE`.
    Bool(bool),
    /// An Excel error.
    Err(ErrorCode),
}

/// A rectangular array, row by row (`values.ts:71-87`). Owned by the evaluation.
#[derive(Clone, Debug)]
pub struct ArrayValue {
    /// Rows.
    pub rows: u32,
    /// Columns.
    pub cols: u32,
    /// `rows * cols` scalars.
    pub data: Box<[Scalar]>,
}

/// A reference: one or more areas (`values.ts:65-68`).
#[derive(Clone, Debug)]
pub struct RefValue {
    /// The areas, in the order of the union.
    pub areas: Vec<Area>,
}

/// Any value an expression produces (`values.ts:95`); a LAMBDA is an index into the evaluator's closures.
#[derive(Clone, Debug)]
pub enum Value {
    /// A scalar.
    Scalar(Scalar),
    /// An array.
    Array(ArrayValue),
    /// A reference.
    Ref(RefValue),
    /// A LAMBDA (opaque outside the evaluator).
    Lambda(u32),
}

/// A scalar with its text resolved: what the A1 helpers (coercion, comparison, criteria) read. The evaluator turns a
/// [`Scalar`] into a view by resolving its [`StrRef`] (pool or pass arena); no string is copied.
#[derive(Clone, Copy, Debug)]
pub enum ScalarView<'a> {
    /// `null`.
    Empty,
    /// A number.
    Num(f64),
    /// Text.
    Str(crate::strings::JsStr<'a>),
    /// A boolean.
    Bool(bool),
    /// An error.
    Err(ErrorCode),
}

impl ScalarView<'_> {
    /// `v === null`.
    #[must_use]
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }
}

/// `ERROR_TYPE` of a code (`values.ts:44-48`).
#[must_use]
pub const fn error_type(e: ErrorCode) -> u8 {
    e as u8
}

impl ArrayValue {
    /// `get(r, c)` (`?? null` outside the data).
    #[must_use]
    pub fn get(&self, r: u32, c: u32) -> Scalar {
        if r >= self.rows || c >= self.cols {
            return Scalar::Empty;
        }
        let i = r as usize * self.cols as usize + c as usize;
        self.data.get(i).copied().unwrap_or(Scalar::Empty)
    }

    /// `ArrayValue.fill(rows, cols, f)`.
    pub fn fill(rows: u32, cols: u32, mut f: impl FnMut(u32, u32) -> Scalar) -> Self {
        let mut data = Vec::with_capacity(rows as usize * cols as usize);
        for r in 0..rows {
            for c in 0..cols {
                data.push(f(r, c));
            }
        }
        Self { rows, cols, data: data.into_boxed_slice() }
    }

    /// `ArrayValue.of(rows)`: ragged rows padded with `#N/A` to the widest one. The TS takes
    /// `Math.max(...rows.map(x => x.length))`, which throws a `RangeError` past ~10^5 rows; this loop does not
    /// (declared in the record, "JS semantics that are traps").
    #[must_use]
    pub fn of(rows: &[Vec<Scalar>]) -> Self {
        let r = rows.len();
        let c = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut data = Vec::with_capacity(r * c);
        for row in rows {
            for j in 0..c {
                data.push(row.get(j).copied().unwrap_or(Scalar::Err(ErrorCode::NA)));
            }
        }
        Self { rows: r as u32, cols: c as u32, data: data.into_boxed_slice() }
    }
}

/// `broadcastAt(a, r, c)` (`math-util.ts:306-310`): a dimension of 1 repeats, outside → `#N/A`.
#[must_use]
pub fn broadcast_at(a: &ArrayValue, r: u32, c: u32) -> Scalar {
    let rr = if a.rows == 1 { 0 } else { r };
    let cc = if a.cols == 1 { 0 } else { c };
    if rr >= a.rows || cc >= a.cols {
        return Scalar::Err(ErrorCode::NA);
    }
    a.get(rr, cc)
}

/// The result shape of `liftScalars` (`math-util.ts:316-322`): the largest rows and columns among the array
/// arguments, `None` when every argument is a scalar (the function is called once, unlifted).
#[must_use]
pub fn lift_shape<'a>(arrays: impl IntoIterator<Item = &'a ArrayValue>) -> Option<(u32, u32)> {
    let mut rows = 0;
    let mut cols = 0;
    for a in arrays {
        rows = rows.max(a.rows);
        cols = cols.max(a.cols);
    }
    if rows == 0 { None } else { Some((rows, cols)) }
}
