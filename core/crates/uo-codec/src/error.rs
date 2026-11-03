use std::fmt;

/// Why a message did not decode. Typed, so the shell can tell a stale wasm/decoder pair
/// ([`DecodeError::VersionMismatch`]) from a damaged message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer does not start with the schema's magic.
    BadMagic,
    /// Same schema name, different schema hash: the two ends were built from different schemas.
    VersionMismatch { expected: [u8; 8], found: [u8; 8] },
    /// The message id is not the one the decoder was asked for.
    WrongMessage { expected: u32, found: u32 },
    /// The bytes ended inside a value, or a count promises more elements than the bytes can hold.
    Truncated,
    /// A varint longer than 5 bytes, over `u32::MAX`, over the range of its type, or not in the shortest form.
    BadVarint,
    /// An enum tag with no variant.
    BadTag { ty: &'static str, tag: u32 },
    /// A `bool` byte other than 0 or 1.
    BadBool,
    /// An id outside the table it refers to (the entries sent so far plus this message's).
    BadRef { table: &'static str, index: u32, bound: u32 },
    /// The message's entries of a table do not continue the decoder's table.
    TableGap { table: &'static str, expected: u32, found: u32 },
    /// A NaN that is not the canonical quiet NaN.
    NonCanonicalNaN,
    /// The unused bits of a bitmap byte are not zero.
    NonZeroPadding,
    /// A [`crate::Limits`] limit was exceeded.
    Limit(&'static str),
    /// Bytes after the end of the message.
    TrailingBytes,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "bad magic"),
            Self::VersionMismatch { expected, found } => {
                write!(f, "schema hash mismatch: expected {expected:02x?}, found {found:02x?}")
            }
            Self::WrongMessage { expected, found } => write!(f, "message id {found}, expected {expected}"),
            Self::Truncated => write!(f, "truncated message"),
            Self::BadVarint => write!(f, "bad varint"),
            Self::BadTag { ty, tag } => write!(f, "tag {tag} is not a variant of {ty}"),
            Self::BadBool => write!(f, "bool byte is not 0 or 1"),
            Self::BadRef { table, index, bound } => write!(f, "{table} id {index} out of range (bound {bound})"),
            Self::TableGap { table, expected, found } => write!(f, "{table} entries start at {found}, expected {expected}"),
            Self::NonCanonicalNaN => write!(f, "NaN is not the canonical quiet NaN"),
            Self::NonZeroPadding => write!(f, "bitmap padding bits are not zero"),
            Self::Limit(what) => write!(f, "limit exceeded: {what}"),
            Self::TrailingBytes => write!(f, "bytes after the end of the message"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Why a value did not encode (a caller bug or a size no 32-bit count can express).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A list, string or table is longer than `u32::MAX`.
    TooLarge,
    /// The columns of a `columns` value do not all have the same length.
    ColumnLengths(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "a count does not fit in 32 bits"),
            Self::ColumnLengths(ty) => write!(f, "columns of {ty} have different lengths"),
        }
    }
}

impl std::error::Error for EncodeError {}
