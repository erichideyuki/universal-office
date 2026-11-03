//! An optional `f64` in 8 bytes.

/// An optional number that keeps "absent" (the TS `undefined`) apart from every value the TS can
/// hold, NaN included, in 8 bytes instead of the 16 of `Option<f64>` (the item size budget,
/// RA1.3).
///
/// "Absent" is a signalling-NaN payload that JavaScript can never produce: every NaN stored as a
/// value is canonicalized first ([`uo_jsnum::conv::canonicalize_nan`]), exactly what the codec
/// does to every `f64` it writes (uo-render §3.1), so no value collides with the marker.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptF64(u64);

const ABSENT: u64 = 0x7ff4_0000_0000_0001;

impl OptF64 {
    /// `undefined`.
    pub const NONE: OptF64 = OptF64(ABSENT);

    /// A present value (NaN canonicalized).
    #[must_use]
    pub fn some(v: f64) -> OptF64 {
        OptF64(uo_jsnum::conv::canonicalize_nan(v).to_bits())
    }

    #[must_use]
    pub fn get(self) -> Option<f64> {
        if self.0 == ABSENT { None } else { Some(f64::from_bits(self.0)) }
    }

    #[must_use]
    pub fn is_some(self) -> bool {
        self.0 != ABSENT
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
