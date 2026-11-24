//! Calc values (`values.ts`): scalars, arrays, references, lambdas. Error codes are a `u8` enum (the TS
//! interns one `CalcError` object per code, `values.ts:17-26`); strings are [`StrRef`]s, never owned heap
//! strings per cell. Arrays share their data (`Arc`: the TS passes them by reference, a `LET` binding used
//! twice does not copy) and a one-area reference is held inline (no allocation per reference).

use std::sync::Arc;

use crate::key::{Area, SheetIx};
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
    /// A text literal of a parsed formula (`"abc"`, `{"a"}`): an index into the AST arena's strings
    /// ([`crate::ast::Ast`]), valid as long as the arena (frontier A2: evaluating a literal copies nothing).
    Lit(u32),
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

/// A rectangular array, row by row (`values.ts:71-87`). Cloning shares the data.
#[derive(Clone, Debug)]
pub struct ArrayValue {
    /// Rows.
    pub rows: u32,
    /// Columns.
    pub cols: u32,
    /// `rows * cols` scalars.
    pub data: Arc<[Scalar]>,
}

/// A reference: one or more areas (`values.ts:65-68`). One area (the common case) is held inline.
#[derive(Clone, Debug, PartialEq)]
pub enum RefValue {
    /// One area.
    One(Area),
    /// A union of areas, in the order of the union (also the empty list, which no parse produces).
    Many(Arc<RefList>),
}

/// The areas of a union and whether they all lie on one sheet (kept as the union grows, so `(A1,A1,…)` checks
/// and appends in constant time per area instead of copying the list at each `,`).
#[derive(Clone, Debug, PartialEq)]
pub struct RefList {
    areas: Vec<Area>,
    sheet: Option<SheetIx>,
}

impl RefValue {
    /// A reference from a list of areas (one area is held inline).
    #[must_use]
    pub fn from_areas(areas: Vec<Area>) -> Self {
        if areas.len() == 1 {
            return Self::One(areas[0]);
        }
        let sheet = areas.first().map(|a| a.sheet).filter(|&s| areas.iter().all(|a| a.sheet == s));
        Self::Many(Arc::new(RefList { areas, sheet }))
    }

    /// `areas`.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        match self {
            Self::One(a) => core::slice::from_ref(a),
            Self::Many(v) => &v.areas,
        }
    }

    /// `single`: the area when there is exactly one.
    #[must_use]
    pub fn single(&self) -> Option<Area> {
        match self {
            Self::One(a) => Some(*a),
            Self::Many(v) if v.areas.len() == 1 => Some(v.areas[0]),
            Self::Many(_) => None,
        }
    }

    /// The sheet of every area (`None` when they differ or there is none).
    #[must_use]
    pub fn one_sheet(&self) -> Option<SheetIx> {
        match self {
            Self::One(a) => Some(a.sheet),
            Self::Many(v) => v.sheet,
        }
    }

    /// `[...a.areas, ...b.areas]`: appends in place when this list is not shared.
    #[must_use]
    pub fn union(self, b: &Self) -> Self {
        let sheet = match (self.one_sheet(), b.one_sheet()) {
            (Some(x), Some(y)) if x == y => Some(x),
            _ => None,
        };
        match self {
            Self::One(a) => {
                let mut areas = Vec::with_capacity(1 + b.areas().len());
                areas.push(a);
                areas.extend_from_slice(b.areas());
                Self::Many(Arc::new(RefList { areas, sheet }))
            }
            Self::Many(mut v) => {
                let l = Arc::make_mut(&mut v);
                l.areas.extend_from_slice(b.areas());
                l.sheet = sheet;
                Self::Many(v)
            }
        }
    }
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
        let n = rows as usize * cols as usize;
        let c = cols.max(1) as usize;
        // a `Range` mapped is `TrustedLen`: one allocation of exactly `n` scalars, filled row by row
        let data: Arc<[Scalar]> = (0..n).map(|i| f((i / c) as u32, (i % c) as u32)).collect();
        Self { rows, cols, data }
    }

    /// `ArrayValue.fill` whose cell function can fail (a host read that throws stops the fill, as the TS
    /// exception leaves `fill`).
    pub fn try_fill<E>(rows: u32, cols: u32, mut f: impl FnMut(u32, u32) -> Result<Scalar, E>) -> Result<Self, E> {
        let n = rows as usize * cols as usize;
        let c = cols.max(1) as usize;
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            data.push(f((i / c) as u32, (i % c) as u32)?);
        }
        Ok(Self { rows, cols, data: data.into() })
    }

    /// An array of `rows × cols` from its row-major data (`new ArrayValue(rows, cols, data)`).
    #[must_use]
    pub fn new(rows: u32, cols: u32, data: Vec<Scalar>) -> Self {
        debug_assert_eq!(data.len(), rows as usize * cols as usize);
        Self { rows, cols, data: data.into() }
    }

    /// The `0 × 0` array.
    #[must_use]
    pub fn empty() -> Self {
        Self { rows: 0, cols: 0, data: Arc::new([]) }
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
        Self { rows: r as u32, cols: c as u32, data: data.into() }
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
