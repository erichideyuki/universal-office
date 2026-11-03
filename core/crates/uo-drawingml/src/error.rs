//! Errors of the DrawingML layer. Messages are English; parity with the TS compares the error
//! CLASS (the variant), never the text. Which condition gives which variant is the table of the
//! module record (`core/docs/modules/uo-drawingml.md`, "Error variant per operation and
//! condition"): the checks run in the TS order and before the first allocation of the call.

use std::fmt;

/// Which limit was exceeded (mapped to `OfficeLimitExceededException` on .NET).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// pixels of a mask or of a padded working plane ([`crate::Limits::max_mask_pixels`])
    MaskPixels,
    /// the radius of a blur kernel ([`crate::Limits::max_kernel_radius`])
    KernelRadius,
    /// points one `flatten_path` call produces ([`crate::Limits::max_path_points`])
    PathPoints,
    /// rows times edges of one `fill_polygons` call ([`crate::Limits::max_fill_work`])
    FillWork,
    /// pixels of an image an adjustment copies ([`crate::Limits::max_image_pixels`])
    ImagePixels,
}

/// Why an operation did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// a limit of [`crate::Limits`] was exceeded (the TS throws a `RangeError`, exhausts memory
    /// or hangs)
    Limit(LimitKind),
    /// a pixel buffer whose length is not what its width and height say
    BufferSize,
    /// an argument no result can be built from (the TS throws a `RangeError` on it)
    InvalidArgument,
    /// the function is declared with its final signature but its port is not written yet
    /// (stage 3, phase 1): the phase-2 port of the module removes every use of it
    NotPorted,
}

/// The result of a fallible operation of this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Limit(kind) => write!(f, "limit exceeded ({kind:?})"),
            Error::BufferSize => write!(f, "the buffer length does not match the dimensions"),
            Error::InvalidArgument => write!(f, "invalid argument"),
            Error::NotPorted => write!(f, "not ported yet"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_every_class() {
        let all = [
            Error::Limit(LimitKind::MaskPixels),
            Error::Limit(LimitKind::KernelRadius),
            Error::Limit(LimitKind::PathPoints),
            Error::Limit(LimitKind::FillWork),
            Error::Limit(LimitKind::ImagePixels),
            Error::BufferSize,
            Error::InvalidArgument,
            Error::NotPorted,
        ];
        let texts: Vec<String> = all.iter().map(|e| e.to_string()).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            for b in &texts[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(texts[0], "limit exceeded (MaskPixels)");
    }
}
