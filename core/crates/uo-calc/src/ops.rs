//! Formula operators over scalars (`ops.ts`): arithmetic, comparison, `&`, `%`, negation, with Excel's rules,
//! and the element-wise broadcast over arrays (in the evaluator, [`crate::eval`]). References arrive already
//! converted by the evaluator. The scalar paths allocate nothing; `&` writes its text into the evaluator's
//! temporary arena ([`TempStrs`]).

use crate::coerce::{to_number, to_text};
use crate::collate::{compare_numbers, compare_text_js};
use crate::fnreg::Throw;
use crate::locale::ValueLocale;
use crate::math_util::{excel_power, snap_add};
use crate::strings::JsStr;
use crate::value::{ErrorCode, Scalar, ScalarView, StrRef};

/// Smallest normal double: Excel flushes subnormals to 0 (`=2^-1074` → 0, measured).
const MIN_NORMAL: f64 = 2.225_073_858_507_201_4e-308;

/// `numResult(x)`: ±0 and subnormals → 0; infinite/NaN → `#NUM!`.
#[must_use]
pub fn num_result(x: f64) -> Scalar {
    if !x.is_finite() {
        return Scalar::Err(ErrorCode::Num);
    }
    Scalar::Num(if x.abs() < MIN_NORMAL { 0.0 } else { x })
}

/// `addSub(a, b, sub)`.
#[must_use]
pub fn add_sub(a: f64, b: f64, sub: bool) -> Scalar {
    num_result(if sub { a - b } else { a + b })
}

/// `addSubTop(a, b, sub)`: the "near zero" adjustment of the formula's LAST operation (`snapAdd`).
#[must_use]
pub fn add_sub_top(a: f64, b: f64, sub: bool) -> Scalar {
    num_result(snap_add(a, if sub { -b } else { b }))
}

/// `power(a, b)` = Excel's `POWER`.
#[must_use]
pub fn power(a: f64, b: f64) -> Scalar {
    match excel_power(a, b) {
        Ok(r) => num_result(r),
        Err(e) => Scalar::Err(e),
    }
}

/// Arithmetic operators (`ArithOp`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `^`
    Pow,
}

/// Comparison operators (`CmpOp`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpOp {
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// The number conventions an operator needs (`locale`, `date1904` and the clock's year of `toNumber`).
#[derive(Clone, Copy, Debug)]
pub struct NumCtx<'a> {
    /// Value locale.
    pub locale: &'a ValueLocale,
    /// 1904 dates.
    pub date1904: bool,
    /// The clock's current year (A1-4).
    pub year: f64,
}

/// `arith(op, x, y, locale, top, date1904)`.
#[must_use]
pub fn arith(op: ArithOp, x: ScalarView<'_>, y: ScalarView<'_>, c: NumCtx<'_>, top: bool) -> Scalar {
    if let ScalarView::Err(e) = x {
        return Scalar::Err(e);
    }
    let a = match to_number(x, c.locale, c.date1904, c.year) {
        Ok(a) => a,
        Err(e) => return Scalar::Err(e),
    };
    if let ScalarView::Err(e) = y {
        return Scalar::Err(e);
    }
    let b = match to_number(y, c.locale, c.date1904, c.year) {
        Ok(b) => b,
        Err(e) => return Scalar::Err(e),
    };
    match op {
        ArithOp::Add => {
            if top {
                add_sub_top(a, b, false)
            } else {
                add_sub(a, b, false)
            }
        }
        ArithOp::Sub => {
            if top {
                add_sub_top(a, b, true)
            } else {
                add_sub(a, b, true)
            }
        }
        ArithOp::Mul => num_result(a * b),
        ArithOp::Div => {
            if b == 0.0 {
                Scalar::Err(ErrorCode::Div0)
            } else {
                num_result(a / b)
            }
        }
        ArithOp::Pow => power(a, b),
    }
}

/// `compareValues(a, b)`: numbers < texts < booleans; empty is 0 / "" / FALSE by the other side; numbers at 15
/// significant digits and texts in the linguistic order. Errors: the caller.
#[must_use]
pub fn compare_values(a: ScalarView<'_>, b: ScalarView<'_>) -> i32 {
    use ScalarView as V;
    let fill = |o: ScalarView<'_>| match o {
        V::Str(_) => V::Str(JsStr::EMPTY),
        V::Bool(_) => V::Bool(false),
        _ => V::Num(0.0),
    };
    let (a, b) = match (a, b) {
        (V::Empty, V::Empty) => return 0,
        (V::Empty, o) => (fill(o), o),
        (o, V::Empty) => (o, fill(o)),
        p => p,
    };
    let rank = |v: ScalarView<'_>| match v {
        V::Num(_) => 0,
        V::Str(_) => 1,
        V::Bool(_) => 2,
        _ => 3,
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return if ra < rb { -1 } else { 1 };
    }
    match (a, b) {
        (V::Num(x), V::Num(y)) => compare_numbers(x, y),
        (V::Str(x), V::Str(y)) => compare_text_js(x, y),
        (V::Bool(x), V::Bool(y)) => {
            if x == y {
                0
            } else if x {
                1
            } else {
                -1
            }
        }
        _ => 0,
    }
}

/// `compare(op, x, y)`.
#[must_use]
pub fn compare(op: CmpOp, x: ScalarView<'_>, y: ScalarView<'_>) -> Scalar {
    if let ScalarView::Err(e) = x {
        return Scalar::Err(e);
    }
    if let ScalarView::Err(e) = y {
        return Scalar::Err(e);
    }
    let c = compare_values(x, y);
    Scalar::Bool(match op {
        CmpOp::Eq => c == 0,
        CmpOp::Ne => c != 0,
        CmpOp::Lt => c < 0,
        CmpOp::Le => c <= 0,
        CmpOp::Gt => c > 0,
        CmpOp::Ge => c >= 0,
    })
}

/// `concat(x, y, locale)`: the texts joined into `temps`; a cell text holds at most 32,767 units.
pub fn concat(x: ScalarView<'_>, y: ScalarView<'_>, locale: &ValueLocale, temps: &mut TempStrs) -> Result<Scalar, Throw> {
    let a = match to_text(x, locale) {
        Ok(a) => a,
        Err(e) => return Ok(Scalar::Err(e)),
    };
    let b = match to_text(y, locale) {
        Ok(b) => b,
        Err(e) => return Ok(Scalar::Err(e)),
    };
    let (a, b) = (a.as_js(), b.as_js());
    if a.len() + b.len() > 32_767 {
        return Ok(Scalar::Err(ErrorCode::Value));
    }
    Ok(Scalar::Str(temps.push2(a, b)?))
}

/// `negate(x, locale, date1904)`: `-0` → 0.
#[must_use]
pub fn negate(x: ScalarView<'_>, c: NumCtx<'_>) -> Scalar {
    if let ScalarView::Err(e) = x {
        return Scalar::Err(e);
    }
    match to_number(x, c.locale, c.date1904, c.year) {
        Ok(a) => Scalar::Num(if a == 0.0 { 0.0 } else { -a }),
        Err(e) => Scalar::Err(e),
    }
}

/// `percent(x, locale, date1904)`.
#[must_use]
pub fn percent(x: ScalarView<'_>, c: NumCtx<'_>) -> Scalar {
    if let ScalarView::Err(e) = x {
        return Scalar::Err(e);
    }
    match to_number(x, c.locale, c.date1904, c.year) {
        Ok(a) => num_result(a / 100.0),
        Err(e) => Scalar::Err(e),
    }
}

/// Texts built while evaluating (`StrRef::Temp`): Latin-1 and UTF-16 storage, cleared at the end of a pass
/// (the engine's per-recalc arena, record "Memory plan"). Past [`TempStrs::cap_bytes`] a new text is refused with
/// `#VALUE!` (hostile only: the TS would keep allocating).
#[derive(Clone, Debug)]
pub struct TempStrs {
    entries: Vec<(u32, u32)>,
    narrow: Vec<u8>,
    wide: Vec<u16>,
    /// Byte cap of the arena (64 MiB on wasm32, 256 MiB native).
    pub cap_bytes: usize,
}

impl Default for TempStrs {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            narrow: Vec::new(),
            wide: Vec::new(),
            cap_bytes: if cfg!(target_pointer_width = "32") { 64 << 20 } else { 256 << 20 },
        }
    }
}

impl TempStrs {
    /// A temporary's text.
    #[must_use]
    pub fn get(&self, i: u32) -> JsStr<'_> {
        let (start, lw) = self.entries[i as usize];
        let (s, n) = (start as usize, (lw >> 1) as usize);
        if lw & 1 == 0 { JsStr::Latin1(&self.narrow[s..s + n]) } else { JsStr::Utf16(&self.wide[s..s + n]) }
    }

    fn bytes(&self) -> usize {
        self.narrow.len() + self.wide.len() * 2 + self.entries.len() * 8
    }

    /// A new temporary.
    pub fn push(&mut self, s: JsStr<'_>) -> Result<StrRef, Throw> {
        self.push2(s, JsStr::EMPTY)
    }

    /// A new temporary `a + b`.
    pub fn push2(&mut self, a: JsStr<'_>, b: JsStr<'_>) -> Result<StrRef, Throw> {
        let len = a.len() + b.len();
        if self.bytes() + len * 2 > self.cap_bytes {
            return Err(Throw::Internal("temporary string arena full"));
        }
        let narrow = matches!(a, JsStr::Latin1(_)) && matches!(b, JsStr::Latin1(_)) || (a.units().chain(b.units()).all(|u| u <= 0xff));
        let id = self.entries.len() as u32;
        if narrow {
            let start = self.narrow.len() as u32;
            match (a, b) {
                (JsStr::Latin1(x), JsStr::Latin1(y)) => {
                    self.narrow.extend_from_slice(x);
                    self.narrow.extend_from_slice(y);
                }
                _ => self.narrow.extend(a.units().chain(b.units()).map(|u| u as u8)),
            }
            self.entries.push((start, (len as u32) << 1));
        } else {
            let start = self.wide.len() as u32;
            self.wide.extend(a.units().chain(b.units()));
            self.entries.push((start, ((len as u32) << 1) | 1));
        }
        Ok(StrRef::Temp(id))
    }

    /// A new temporary from UTF-16 units (narrow when every unit fits a byte).
    pub fn push_units(&mut self, u: &[u16]) -> Result<StrRef, Throw> {
        if self.bytes() + u.len() * 2 > self.cap_bytes {
            return Err(Throw::Internal("temporary string arena full"));
        }
        let id = self.entries.len() as u32;
        if u.iter().all(|&x| x <= 0xff) {
            let start = self.narrow.len() as u32;
            self.narrow.extend(u.iter().map(|&x| x as u8));
            self.entries.push((start, (u.len() as u32) << 1));
        } else {
            let start = self.wide.len() as u32;
            self.wide.extend_from_slice(u);
            self.entries.push((start, ((u.len() as u32) << 1) | 1));
        }
        Ok(StrRef::Temp(id))
    }

    /// Number of temporaries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// None.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops every temporary (end of a pass); the capacity is kept.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.narrow.clear();
        self.wide.clear();
    }

    /// Releases the capacity too.
    pub fn release(&mut self) {
        *self = Self { cap_bytes: self.cap_bytes, ..Self::default() };
    }
}
