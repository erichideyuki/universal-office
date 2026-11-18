//! Raster image codecs of the UniversalOffice Rust core, shared by Write, Sheet, Slides and the
//! converter: what the PDF writer and the read path need to turn the pictures of a document into
//! pixels. A port of `write/packages/docx-layout/src/images.ts` (frozen 0.9.x): the header reads
//! [`jpeg_info`] and [`image_pixel_size`], and the decoders [`decode_png`] (every depth and colour
//! type, Adam7), [`decode_gif`] (first frame), [`decode_bmp`], [`decode_tiff`] (baseline: strips
//! and tiles, none/LZW/PackBits/deflate, predictor, palette, CMYK, YCbCr with subsampling) and
//! [`decode_raster`], which tries them in that order. JPEG is embedded as it is (DCTDecode), so only its header is
//! read — except `decode_jpeg` (feature `jpeg`, the PDF's colour effects on a JPEG). Plan and decisions: `core/docs/modules/uo-image.md`.
//!
//! The decoders keep the TS behaviour on damaged files (a short file decodes to a partly zero
//! image, an unsupported variant is [`ImageError::Unsupported`], what makes the TS throw is
//! [`ImageError::Malformed`]), and add [`ImageLimits`]: nothing is allocated from a size declared
//! in a file before it is checked against the limits, and every intermediate buffer is bounded by
//! the bytes actually produced or by a limit. Nothing here panics on image data.
//!
//! Out of scope: EMF/WMF (`uo-docx-layout::metafile`, layout LQ7), the picture adjustments
//! (`uo-drawingml::blip`) and the PDF embedding (`uo-pdf`).
#![forbid(unsafe_code)]
#![deny(clippy::indexing_slicing)]

mod bmp;
mod gif;
mod header;
#[cfg(feature = "jpeg")]
mod jpeg;
mod limits;
mod lzw;
mod png;
mod raster;
mod tiff;
mod tiff_slides;

pub use bmp::decode_bmp;
pub use gif::decode_gif;
pub use header::{JpegInfo, PixelSize, image_pixel_size, jpeg_info};
#[cfg(feature = "jpeg")]
pub use jpeg::decode_jpeg;
pub use limits::{ImageError, ImageLimits, LimitKind, Result};
pub use png::decode_png;
pub use raster::decode_raster;
pub use tiff::decode_tiff;
pub use tiff_slides::{decode_tiff_slides, is_tiff_slides};

/// A decoded image: 8-bit RGBA, row by row, `width * height * 4` bytes (the TS `Rgba`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Nothing in this crate caches across calls; kept for the memory-gate convention (M1).
pub fn clear_caches() {}

/// `b[i]` of a `Uint8Array`: `None` is `undefined`.
#[inline]
pub(crate) fn at(b: &[u8], i: u64) -> Option<u8> {
    usize::try_from(i).ok().and_then(|i| b.get(i)).copied()
}

/// `b[i]` where only bit operations read it (`undefined | 0` is 0, `undefined << 8` is 0).
#[inline]
pub(crate) fn at0(b: &[u8], i: u64) -> u32 {
    at(b, i).map_or(0, u32::from)
}

/// `b.subarray(start, end)` for non-negative integer bounds: the range clamped to the array.
#[inline]
pub(crate) fn sub(b: &[u8], start: u64, end: u64) -> &[u8] {
    let len = b.len() as u64;
    let s = start.min(len);
    let e = end.min(len).max(s);
    match (usize::try_from(s), usize::try_from(e)) {
        (Ok(s), Ok(e)) => b.get(s..e).unwrap_or(&[]),
        _ => &[],
    }
}

/// `unzlibSync(z)` for the front of the result: at least `need` bytes of it when it has them (the
/// output is cut there, never above `max`). `Ok(None)` is the stream the TS refuses (it throws).
///
/// The cheap way is to inflate with `need` as the size of the output, which does not allocate more
/// and still reads the whole stream; it refuses a stored block that does not fit, which the TS
/// accepts, so a refusal is tried again without the size, up to `max`.
pub(crate) fn unzlib(z: &[u8], need: u64, max: u64) -> Result<Option<Vec<u8>>> {
    use uo_opc::Error as FlateError;
    let limit = |e: FlateError| if matches!(e, FlateError::Limit { .. }) { Some(ImageError::Limit(LimitKind::Inflated)) } else { None };
    // an empty body (zlib data of at most 6 bytes) inflates to nothing
    let declared = if z.len() <= 6 { None } else { Some(need) };
    match uo_opc::flate::inflate_zlib(z, declared, max) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            if let Some(l) = limit(e) {
                return Err(l);
            }
            if declared.is_none() {
                return Ok(None);
            }
            match uo_opc::flate::inflate_zlib(z, None, max) {
                Ok(v) => Ok(Some(v)),
                Err(e) => limit(e).map_or(Ok(None), Err),
            }
        }
    }
}

/// A zero-filled buffer of `n` bytes, or the limit error when the allocator refuses it.
pub(crate) fn zeroed(n: u64) -> Result<Vec<u8>> {
    let n = usize::try_from(n).map_err(|_| ImageError::Limit(LimitKind::Allocation))?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| ImageError::Limit(LimitKind::Allocation))?;
    v.resize(n, 0);
    Ok(v)
}
