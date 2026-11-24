//! Rust-side failures of the store and the engine (never a panic). Excel error VALUES are
//! [`crate::value::ErrorCode`], not this.

/// A failure of a store or engine operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcError {
    /// Skeleton: the operation is declared by the architecture pass and not ported yet.
    NotPorted(&'static str),
    /// A limit of [`crate::store::Limits`] was reached (untrusted input); the store is unchanged.
    Limit(&'static str),
    /// A sheet index outside the store.
    NoSheet(u32),
    /// The operation was cancelled through the host's cancel check (as Write's G4).
    Cancelled,
}

/// Result of a store or engine operation.
pub type Result<T> = core::result::Result<T, CalcError>;

impl core::fmt::Display for CalcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotPorted(what) => write!(f, "not ported yet: {what}"),
            Self::Limit(what) => write!(f, "limit reached: {what}"),
            Self::NoSheet(i) => write!(f, "no sheet {i}"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for CalcError {}
