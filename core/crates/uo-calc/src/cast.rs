//! The float → integer conversions of the A1 code, by name. Each is called on a value whose range (and, where it
//! matters, integrality) the caller has established, so the `as` truncation is the JS/C++ operation it stands for
//! (`FastD2I`, an index already checked, a `TimeClip`ped time); the float-casts gate (`tests/float-casts.sh`) sees
//! only these four.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

/// Truncation to `i32` of a value inside the `i32` range (V8 `FastD2I`).
pub(crate) fn trunc_i32(x: f64) -> i32 {
    x as i32
}

/// Truncation to `i64` of a value inside the `i64` range (an integral time or an exponent).
pub(crate) fn trunc_i64(x: f64) -> i64 {
    x as i64
}

/// Truncation to `u64` of a non-negative value below 2^64 (an integer exponent of `POWER`).
pub(crate) fn trunc_u64(x: f64) -> u64 {
    x as u64
}

/// Truncation to `usize` of a non-negative value inside the target range (a string position, a quadrant).
pub(crate) fn trunc_usize(x: f64) -> usize {
    x as usize
}
