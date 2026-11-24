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
