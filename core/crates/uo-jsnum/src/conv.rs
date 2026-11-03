//! ECMA-262 integer conversions (ToInt32, ToUint32, ...), the typed-array element stores and
//! the numeric predicates the engines use. All pure and allocation free.
//!
//! Float → integer goes through these functions only: `x as i32` saturates and maps NaN to 0,
//! JS wraps modulo 2^32 (`|0`, `>>> 0`, shifts, `Int32Array`, `DataView.setInt32`).

/// ToInt32: truncate towards zero, wrap modulo 2^32, NaN and ±∞ give 0 (`x | 0`, `~~x`, the
/// operand of shifts, `Int32Array` stores).
#[must_use]
pub fn to_int32(x: f64) -> i32 {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7FF) as i32 - 1075;
    if exponent <= -53 || exponent >= 32 {
        return 0;
    }
    let significand = (bits & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
    let magnitude = if exponent < 0 { significand >> -exponent } else { significand << exponent };
    let low = magnitude as u32;
    (if (bits >> 63) != 0 { low.wrapping_neg() } else { low }) as i32
}

/// ToUint32 (`x >>> 0`, `Uint32Array` stores).
#[must_use]
pub fn to_uint32(x: f64) -> u32 {
    to_int32(x) as u32
}

/// ToInt16 (`DataView.setInt16`, `Int16Array` stores).
#[must_use]
pub fn to_int16(x: f64) -> i16 {
    to_int32(x) as i16
}

/// ToUint16 (`DataView.setUint16`, `Uint16Array` stores).
#[must_use]
pub fn to_uint16(x: f64) -> u16 {
    to_int32(x) as u16
}

/// ToInt8 (`DataView.setInt8`, `Int8Array` stores).
#[must_use]
pub fn to_int8(x: f64) -> i8 {
    to_int32(x) as i8
}

/// ToUint8: wraps (`Uint8Array` stores, `DataView.setUint8`); `x as u8` would saturate.
#[must_use]
pub fn to_uint8(x: f64) -> u8 {
    to_int32(x) as u8
}

/// ToUint8Clamp (`Uint8ClampedArray` stores): clamps to 0..=255 and rounds half to even;
/// NaN gives 0.
#[must_use]
pub fn to_uint8_clamp(x: f64) -> u8 {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    if x >= 255.0 {
        return 255;
    }
    let f = x.floor();
    let v = if f + 0.5 < x {
        f + 1.0
    } else if x < f + 0.5 {
        f
    } else if f % 2.0 == 1.0 {
        f + 1.0
    } else {
        f
    };
    v as u8
}

/// The element index a JS number addresses in a typed array of `len` elements, `ta[i]` with a
/// number `i`: `None` for NaN, a fraction, a negative number (−0 is index 0, since
/// `String(-0)` is `"0"`) and anything at or beyond `len`. A read of `None` gives `NaN` (JS
/// `undefined` coerced), a write of `None` does nothing.
#[must_use]
pub fn js_index(i: f64, len: usize) -> Option<usize> {
    if i >= 0.0 && i.trunc() == i && i < len as f64 { Some(i as usize) } else { None }
}

/// Any NaN as the quiet NaN `0x7ff8000000000000`. Rust code must canonicalize before any byte
/// serialization or bit inspection: the NaN payload of a JS number is observable
/// (`DataView.setFloat64(Math.log(-1))` writes `0x7ff8000000000001`) and never part of parity.
#[must_use]
pub fn canonicalize_nan(x: f64) -> f64 {
    if x.is_nan() { f64::from_bits(0x7FF8_0000_0000_0000) } else { x }
}

/// JS truthiness of a number: false for ±0 and NaN (`x || d`, `if (x)`).
#[must_use]
pub fn truthy(x: f64) -> bool {
    !(x == 0.0 || x.is_nan())
}

/// `Number.isInteger`.
#[must_use]
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

/// `Number.isSafeInteger`.
#[must_use]
pub fn is_safe_integer(x: f64) -> bool {
    is_integer(x) && x.abs() <= 9_007_199_254_740_991.0
}
