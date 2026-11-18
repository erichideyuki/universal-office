//! JPEG to RGBA (feature `jpeg`): only for the PDF paths that must touch the pixels of a JPEG — the colour effects of a
//! picture (grayscale, duotone, bi-level, colour change) and of a metafile recipe's bitmap. Everything else still embeds
//! JPEG as it is (DCTDecode) and the browser decodes it itself, so no wasm build enables this feature.
//!
//! The decoder is `zune-jpeg` (pinned, `default-features = false`: the scalar path, no SIMD dispatch, no unsafe of
//! its own on that path; single-threaded). Chosen over `jpeg-decoder` 0.3.2 (closer to libjpeg-turbo on 4:2:0 chroma:
//! mean 0.14 against 0.36 levels, both at most 3–4) because `jpeg-decoder` spawns worker threads for images over
//! 128×128 px, with no switch to stop it, which a library inside a PDF writer and the per-thread memory gate of the core
//! should not do; record `docs/modules/uo-image.md` "JPEG decoder (Slides wave 5)". Measured against libjpeg-turbo (via
//! Pillow) on 13 files (4:4:4, 4:2:2, 4:2:0, progressive, restart markers, grayscale, Adobe CMYK, 6 corpus JPEGs): at
//! most 4 levels, mean ≤ 0.36.
//!
//! Limits: the declared size is checked against [`ImageLimits`] before anything is decoded (and given to the decoder as
//! its own maximum); a decoder error is [`ImageError::Malformed`].

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::limits::{ImageError, ImageLimits, Result};
use crate::{Rgba, jpeg_info};

/// Decodes a JPEG (baseline or progressive, grayscale, YCbCr, Adobe CMYK/YCCK) to 8-bit RGBA.
pub fn decode_jpeg(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    let info = jpeg_info(b).ok_or(ImageError::Unsupported)?;
    limits.check_size(u64::from(info.width), u64::from(info.height))?;
    let max_side = usize::try_from(limits.max_dim).unwrap_or(usize::MAX);
    let opts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(max_side)
        .set_max_height(max_side)
        .set_use_unsafe(false)
        .set_strict_mode(false);
    let mut d = JpegDecoder::new_with_options(ZCursor::new(b), opts);
    d.decode_headers().map_err(|_| ImageError::Malformed("jpeg header"))?;
    let (w, h) = d.dimensions().ok_or(ImageError::Malformed("jpeg size"))?;
    let (w, h) = (
        u32::try_from(w).map_err(|_| ImageError::Malformed("jpeg size"))?,
        u32::try_from(h).map_err(|_| ImageError::Malformed("jpeg size"))?,
    );
    let px = limits.check_size(u64::from(w), u64::from(h))?;
    if px.saturating_mul(4) > limits.max_inflated {
        return Err(ImageError::Limit(crate::LimitKind::Inflated));
    }
    // RGB out (the decoder's RGBA of a CMYK file leaves the alpha at 0), made opaque RGBA here
    let rgb = d.decode().map_err(|_| ImageError::Malformed("jpeg data"))?;
    let n = usize::try_from(px).map_err(|_| ImageError::Malformed("jpeg size"))?;
    if rgb.len() != n.checked_mul(3).ok_or(ImageError::Malformed("jpeg size"))? {
        return Err(ImageError::Malformed("jpeg data"));
    }
    let mut data = Vec::with_capacity(n * 4);
    for p in rgb.as_chunks::<3>().0 {
        data.extend_from_slice(p);
        data.push(255);
    }
    Ok(Rgba { width: w, height: h, data })
}
