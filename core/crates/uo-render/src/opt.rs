//! An optional `f64` in 8 bytes.

/// An optional number that keeps "absent" (the TS `undefined`) apart from every value the TS can
/// hold, NaN included, in 8 bytes instead of the 16 of `Option<f64>` (the item size budget,
/// RA1.3).
///
/// Stored as an integer: 0 is "absent", a value is its bits plus 1, with every NaN canonicalized first
/// ([`uo_jsnum::conv::canonicalize_nan`], what the codec does to every `f64` it writes, uo-render
/// §3.1). The canonical bits are never `u64::MAX`, so the sum never wraps to 0. It is never a NaN
/// payload, because JavaScript does not keep them: at opt-level 3 LLVM materialised the former
/// signalling-NaN marker as an `f64.const`, which the wasm2js build prints as `NaN` (G22, uo-bindings.md
/// "No-WebAssembly fallback").
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptF64(u64);

impl OptF64 {
    /// `undefined`.
    pub const NONE: OptF64 = OptF64(0);

    /// A present value (NaN canonicalized).
    #[must_use]
    pub fn some(v: f64) -> OptF64 {
        OptF64(uo_jsnum::conv::canonicalize_nan(v).to_bits().wrapping_add(1))
    }

    #[must_use]
    pub fn get(self) -> Option<f64> {
        self.0.checked_sub(1).map(f64::from_bits)
    }

    #[must_use]
    pub fn is_some(self) -> bool {
        self.0 != 0
    }

    /// `v ?? d`.
    #[must_use]
    pub fn or(self, d: f64) -> f64 {
        self.get().unwrap_or(d)
    }
}

impl Default for OptF64 {
    fn default() -> OptF64 {
        OptF64::NONE
    }
}

impl From<Option<f64>> for OptF64 {
    fn from(v: Option<f64>) -> OptF64 {
        v.map_or(OptF64::NONE, OptF64::some)
    }
}

impl std::fmt::Debug for OptF64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.get() {
            Some(v) => write!(f, "{v:?}"),
            None => f.write_str("undefined"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OptF64;

    #[test]
    fn absent_is_zero_and_every_bit_pattern_is_a_value() {
        assert_eq!(OptF64::NONE.0, 0);
        assert_eq!(OptF64::default(), OptF64::NONE);
        assert!(OptF64::some(f64::from_bits(u64::MAX)).is_some());
    }
}
