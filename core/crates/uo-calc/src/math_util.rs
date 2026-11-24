//! Numeric support of the math, engineering and financial functions (`functions/math-util.ts`): the 15-digit view,
//! Excel's cancellation in sums, decimal rounding, `POWER`, the ATP argument rules and the x87 trigonometry.
//!
//! The function-definition factories of the TS (`numFn`, `atpFn`, `def`) and the argument walkers that need the
//! evaluation context (`eachNumber`, `asMatrix`, `atpRaw`, `atpList`, `liftScalars`) are wired by A2/A4 on top of
//! the rules here ([`EachNumber`], [`atp_num`], [`crate::value::broadcast_at`], [`crate::value::lift_shape`]).

use uo_jsnum::math::{abs, floor, pow, sqrt};

use crate::coerce::to_number;
use crate::libm::{cr_log, xl_cos, xl_exp, xl_sin, xl_tan};
use crate::locale::ValueLocale;
use crate::value::{ErrorCode, ScalarView};

/// A numeric result or an Excel error (`Num` in the TS).
pub type Num = Result<f64, ErrorCode>;

/// `fin(x)`: NaN/±∞ → `#NUM!`; `-0` → `0`.
pub fn fin(x: f64) -> Num {
    if !x.is_finite() {
        return Err(ErrorCode::Num);
    }
    Ok(if x == 0.0 { 0.0 } else { x })
}

/// `finFlush(x)`: NaN/±∞ → `#NUM!`; subnormal results (and `-0`) become `+0` (Excel's `POWER`).
pub fn fin_flush(x: f64) -> Num {
    if !x.is_finite() {
        return Err(ErrorCode::Num);
    }
    Ok(if abs(x) < 2.225_073_858_507_201_4e-308 { 0.0 } else { x })
}

/// `approx15(x)`: the number with 15 significant digits (Excel's view before truncating/rounding).
#[must_use]
pub fn approx15(x: f64) -> f64 {
    crate::collate::round15(x)
}

/// `exponentOf(x)`: the exact binary exponent ⌊log2|x|⌋ (also next to the powers of 2); `-1075` for 0.
#[must_use]
pub fn exponent_of(x: f64) -> f64 {
    let e = ((x.to_bits() >> 52) & 0x7ff) as i32;
    if e == 0 {
        return if x == 0.0 { -1075.0 } else { floor(uo_jsnum::math::log2(abs(x))) };
    }
    f64::from(e - 1023)
}

/// `snapAdd(a, b)`: `a + b` with Excel's cancellation: opposite signs and a result below `2^(e−49)`, e = the
/// binary exponent of the LEFT operand (the accumulator), become an exact 0.
#[must_use]
pub fn snap_add(a: f64, b: f64) -> f64 {
    let r = a + b;
    if r == 0.0 || !r.is_finite() || (a > 0.0) == (b > 0.0) || a == 0.0 || b == 0.0 {
        return r;
    }
    if abs(r) < pow(2.0, exponent_of(a) - 49.0) { 0.0 } else { r }
}

/// Decimal rounding mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundMode {
    /// Half away from zero (`ROUND`).
    Half,
    /// Away from zero (`ROUNDUP`).
    Up,
    /// Toward zero (`ROUNDDOWN`).
    Down,
}

/// `str.slice(start, end)` index of a JS number argument (ToIntegerOrInfinity, negatives from the end, clamped).
fn js_slice_index(x: f64, len: usize) -> usize {
    let t = if x.is_nan() { 0.0 } else { uo_jsnum::math::trunc(x) };
    let len_f = len as f64;
    if t < 0.0 {
        let r = len_f + t;
        if r > 0.0 { crate::cast::trunc_usize(r) } else { 0 }
    } else if t < len_f {
        crate::cast::trunc_usize(t)
    } else {
        len
    }
}

/// `decimalRound(x, d, mode)`: rounds to `d` decimals (d < 0 = left of the point) as `ROUND`/`ROUNDUP`/`ROUNDDOWN`:
/// first the number with 15 significant digits, then the decimal digits (half away from zero). `ROUND(2,675;2)` =
/// 2,68. Returns ±∞ on overflow (the caller gives `#NUM!`).
#[must_use]
pub fn decimal_round(x: f64, d: f64, mode: RoundMode) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let neg = x < 0.0;
    let s = uo_jsnum::format::to_exponential(abs(x), 14).unwrap_or_default(); // "d.dddddddddddddde±X"
    let ei = s.find('e').unwrap_or(s.len());
    let mut digits = String::with_capacity(15);
    digits.push_str(&s[..1]);
    digits.push_str(s.get(2..ei).unwrap_or(""));
    let e = uo_jsnum::parse::parse_int(s.get(ei + 1..).unwrap_or(""), 10);
    // value = 0.d1d2…d15 × 10^(e+1); keep `keep` digits
    let keep = e + 1.0 + d;
    let neg_d = uo_jsnum::format::to_js_string(-d);
    let out = if keep >= 15.0 {
        let mut t = String::with_capacity(24);
        t.push_str(&digits[..1]);
        t.push('.');
        t.push_str(&digits[1..]);
        t.push('e');
        t.push_str(&uo_jsnum::format::to_js_string(e));
        uo_jsnum::parse::to_number(&t)
    } else if keep < 0.0 {
        if mode == RoundMode::Up { uo_jsnum::parse::to_number(&format!("1e{neg_d}")) } else { 0.0 }
    } else {
        // `keep` may be fractional or NaN (a fractional `d`): `slice` truncates it
        let k = js_slice_index(keep, digits.len());
        let (kept, rest) = digits.split_at(k);
        let up = match mode {
            RoundMode::Up => rest.bytes().any(|b| (b'1'..=b'9').contains(&b)),
            RoundMode::Half => rest.as_bytes().first().is_some_and(|&b| b >= b'5'),
            RoundMode::Down => false,
        };
        let mut kd: Vec<u8> = kept.as_bytes().to_vec();
        if up {
            // add 1 to the last kept digit (with carry)
            let mut i = kd.len();
            let mut carried_out = true;
            while i > 0 {
                i -= 1;
                if kd[i] == b'9' {
                    kd[i] = b'0';
                } else {
                    kd[i] += 1;
                    carried_out = false;
                    break;
                }
            }
            if carried_out {
                kd.insert(0, b'1');
            }
        }
        if kd.is_empty() {
            0.0
        } else {
            let mut t = String::from_utf8(kd).unwrap_or_default();
            t.push('e');
            t.push_str(&neg_d);
            uo_jsnum::parse::to_number(&t)
        }
    };
    if neg { -out } else { out }
}

/// `TRIG_LIMIT` = 2^27: `SIN`/`COS`/`TAN` of a larger |x| are `#NUM!`.
pub const TRIG_LIMIT: f64 = 134_217_728.0;
/// `cosExt`.
pub use crate::libm::xl_cos as cos_ext;
/// `sinExt`: Excel's `SIN` (x87 π reduction).
pub use crate::libm::xl_sin as sin_ext;
/// `tanExt`.
pub use crate::libm::xl_tan as tan_ext;

const _: fn(f64) -> f64 = xl_sin;
const _: fn(f64) -> f64 = xl_cos;
const _: fn(f64) -> f64 = xl_tan;

/// `x^n` by successive squaring, `n` an integer `0 ≤ n < 2^32`.
fn pow_int(x: f64, n: f64) -> f64 {
    let mut n = crate::cast::trunc_u64(n);
    let mut r = 1.0;
    let mut b = x;
    while n > 0 {
        if n % 2 == 1 {
            r *= b;
        }
        n /= 2;
        if n != 0 {
            b *= b;
        }
    }
    r
}

/// `excelPower(x, y)`: `POWER(x; y)` and `x^y`.
pub fn excel_power(x: f64, y: f64) -> Num {
    if x == 0.0 {
        if y == 0.0 {
            return Err(ErrorCode::Num);
        }
        if y < 0.0 {
            return Err(ErrorCode::Div0);
        }
        return Ok(0.0);
    }
    if uo_jsnum::conv::is_integer(y) && abs(y) < 4_294_967_296.0 {
        // base 10: the exact power of the table (POWER(10;300) = 1E300; 10^−n = 1/10^n)
        let pos = if x == 10.0 { uo_jsnum::parse::to_number(&format!("1e{}", crate::cast::trunc_u64(abs(y)))) } else { pow_int(x, abs(y)) };
        return fin_flush(if y < 0.0 { 1.0 / pos } else { pos });
    }
    if x < 0.0 {
        let t = approx15(1.0 / y);
        if !uo_jsnum::conv::is_integer(t) || uo_jsnum::math::rem(abs(t), 2.0) != 1.0 {
            return Err(ErrorCode::Num);
        }
        return fin_flush(-pow_real(-x, y));
    }
    fin_flush(pow_real(x, y))
}

/// `x^y` for a non-integer y, x > 0: `exp(y·ln x)` in doubles (Excel's x87 `EXP`, correct `ln`), except y = 0.5 (the
/// exact root); y < 0 → `1/x^(−y)`.
fn pow_real(x: f64, y: f64) -> f64 {
    if y < 0.0 {
        return 1.0 / pow_real(x, -y);
    }
    if y == 0.5 { sqrt(x) } else { xl_exp(y * cr_log(x)) }
}

/// `atpNum(v)`: the number of an ATP scalar (boolean = `#VALUE!`; empty = 0).
pub fn atp_num(v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Num {
    if matches!(v, ScalarView::Bool(_)) {
        return Err(ErrorCode::Value);
    }
    to_number(v, locale, false, year)
}

/// `nums(xs)`: converts in order; the first failure decides the error.
pub fn nums<'a>(
    xs: impl IntoIterator<Item = ScalarView<'a>>,
    locale: &ValueLocale,
    year: f64,
    out: &mut Vec<f64>,
) -> Result<(), ErrorCode> {
    for x in xs {
        out.push(to_number(x, locale, false, year)?);
    }
    Ok(())
}

/// The per-value rules of `eachNumber` (`SUM`, `PRODUCT`, `SUMSQ`…): a direct argument converts (text-number,
/// boolean; non-numeric text = `#VALUE!`; omitted = skipped); references and arrays count only numbers; an error
/// anywhere propagates (the first in argument order, row by row inside a range). A lambda argument is `#CALC!`.
#[derive(Clone, Copy, Debug)]
pub struct EachNumber;

impl EachNumber {
    /// A cell of a reference argument: `Some(n)` to visit, `None` to skip, `Err` to stop.
    pub fn ref_cell(v: ScalarView<'_>) -> Result<Option<f64>, ErrorCode> {
        match v {
            ScalarView::Num(n) => Ok(Some(n)),
            ScalarView::Err(e) => Err(e),
            _ => Ok(None),
        }
    }

    /// An element of an array argument (same rule as a cell).
    pub fn array_elem(v: ScalarView<'_>) -> Result<Option<f64>, ErrorCode> {
        Self::ref_cell(v)
    }

    /// A direct scalar argument (`null` = omitted → skipped).
    pub fn direct(v: ScalarView<'_>, locale: &ValueLocale, year: f64) -> Result<Option<f64>, ErrorCode> {
        if v.is_empty() {
            return Ok(None);
        }
        to_number(v, locale, false, year).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured() {
        assert_eq!(snap_add(1.0, -0.999_999_999_999_998_3), 0.0);
        assert_eq!(snap_add(1.0, -0.999_999_999_999_998_2), 1.776_356_839_400_250_5e-15);
        assert_eq!(excel_power(1.1, 700.0), Ok(9.437_992_127_672_861e28));
        assert_eq!(decimal_round(2.675, 2.0, RoundMode::Half), 2.68);
        assert_eq!(excel_power(8.0, 1.0 / 3.0), Ok(1.999_999_999_999_999_8));
    }
}
