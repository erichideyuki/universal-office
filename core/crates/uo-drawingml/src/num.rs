//! The one numeric helper the crate adds to `uo-jsnum`: the `Float32Array` store.

/// A `Float32Array` element store (`Math.fround`): the `f64` rounded to the nearest `f32`, ties
/// to even, `NaN` and the infinities kept. R2: `f32` is storage only; every store goes through
/// here and every read is `f64::from`.
#[allow(dead_code, reason = "used by the phase-2 ports of raster, effects and bevel")]
#[expect(clippy::cast_possible_truncation, reason = "the rounding to f32 is the store")]
pub(crate) fn f32_store(x: f64) -> f32 {
    x as f32
}

#[cfg(test)]
mod tests {
    use super::f32_store;

    #[test]
    fn rounds_like_fround() {
        // 2^24 + 1 is not an f32: ties to even gives 2^24
        assert_eq!(f64::from(f32_store(16_777_217.0)), 16_777_216.0);
        assert_eq!(f64::from(f32_store(16_777_219.0)), 16_777_220.0);
        assert!(f32_store(f64::NAN).is_nan());
        assert_eq!(f32_store(f64::INFINITY), f32::INFINITY);
        assert_eq!(f32_store(1e300), f32::INFINITY);
        assert_eq!(f32_store(-0.0).to_bits(), (-0.0f32).to_bits());
    }
}
