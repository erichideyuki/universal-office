//! Errors and limits of the codecs.

use std::fmt;

/// Which limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// `width * height` of the image (or of a GIF frame) above [`ImageLimits::max_px`]
    Pixels,
    /// a side above [`ImageLimits::max_dim`]
    Dimension,
    /// an intermediate buffer (inflated PNG data, TIFF raw samples, YCbCr blocks) above
    /// [`ImageLimits::max_inflated`]
    Inflated,
    /// the values of the TIFF tags that are read above [`ImageLimits::max_tag_values`]
    TagValues,
    /// the allocator refused a buffer
    Allocation,
}

/// Why an image was not decoded. All three mean "the image is skipped" to the PDF writer
/// (`pdf.ts` catches whatever the decoder throws); they differ for [`crate::decode_raster`],
/// which goes on to the next format only after `Unsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// the TS returns `null`: not this format, or a variant it does not decode
    Unsupported,
    /// the TS throws (a `DataView` read past the end, a negative array length): what, for
    /// diagnostics
    Malformed(&'static str),
    /// a limit of [`ImageLimits`] (the TS has none: it would allocate, or throw a `RangeError`)
    Limit(LimitKind),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Unsupported => f.write_str("unsupported image"),
            ImageError::Malformed(what) => write!(f, "malformed image: {what}"),
            ImageError::Limit(kind) => write!(f, "image limit exceeded: {kind:?}"),
        }
    }
}

impl std::error::Error for ImageError {}

pub type Result<T> = std::result::Result<T, ImageError>;

/// Limits for untrusted images, counted on declared sizes before anything is allocated from
/// them and on the bytes actually produced afterwards.
///
/// The decoded RGBA buffer is the one allocation whose size comes from the file (it is the size
/// of the result, as in the TS) and `max_px` bounds it; every intermediate buffer is bounded by
/// the data that really exists or by `max_inflated`. The flate expansion needs no ratio limit of
/// its own: DEFLATE cannot expand more than 1032:1, and the inflate stops at the size the header
/// needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageLimits {
    /// `width * height` (a GIF frame counts on its own)
    pub max_px: u64,
    /// the longest side
    pub max_dim: u32,
    /// bytes of one intermediate buffer (inflated PNG data, TIFF raw samples)
    pub max_inflated: u64,
    /// values read from the tags of one TIFF IFD (the TS reads up to 2^20 per tag, without a total)
    pub max_tag_values: u64,
}

impl ImageLimits {
    /// Native defaults: 120 Mpx (the 480 MB of RGBA of the plan's example), 131,072 per side,
    /// 1 GiB of intermediate data, 2^21 TIFF tag values.
    pub const NATIVE: ImageLimits = ImageLimits { max_px: 120_000_000, max_dim: 131_072, max_inflated: 1 << 30, max_tag_values: 1 << 21 };

    /// The phone baseline (wasm32): 40 Mpx (160 MB of RGBA: a 600 dpi A4 scan is 35 Mpx, a phone
    /// photo 12), 256 MiB of intermediate data.
    pub const WASM32: ImageLimits = ImageLimits { max_px: 40_000_000, max_dim: 131_072, max_inflated: 256 << 20, max_tag_values: 1 << 21 };

    /// The defaults of the target: [`ImageLimits::WASM32`] on wasm32, [`ImageLimits::NATIVE`] elsewhere.
    #[cfg(target_arch = "wasm32")]
    pub const DEFAULT: ImageLimits = ImageLimits::WASM32;

    /// The defaults of the target: [`ImageLimits::WASM32`] on wasm32, [`ImageLimits::NATIVE`] elsewhere.
    #[cfg(not(target_arch = "wasm32"))]
    pub const DEFAULT: ImageLimits = ImageLimits::NATIVE;

    /// Checks declared dimensions; the product is computed wide, so no input can overflow it.
    pub(crate) fn check_size(&self, width: u64, height: u64) -> Result<u64> {
        if width > u64::from(self.max_dim) || height > u64::from(self.max_dim) {
            return Err(ImageError::Limit(LimitKind::Dimension));
        }
        let px = width * height;
        if px > self.max_px {
            return Err(ImageError::Limit(LimitKind::Pixels));
        }
        Ok(px)
    }
}

impl Default for ImageLimits {
    fn default() -> Self {
        ImageLimits::DEFAULT
    }
}
