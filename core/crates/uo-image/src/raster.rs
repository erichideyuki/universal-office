//! `decodeRaster` of `images.ts`.

use crate::{ImageError, ImageLimits, Result, Rgba, decode_bmp, decode_gif, decode_png, decode_tiff};

type Decoder = fn(&[u8], &ImageLimits) -> Result<Rgba>;

/// Decodes any supported raster format (JPEG is embedded as it is, not decoded here): PNG, GIF,
/// BMP, then TIFF, the order of the TS `??` chain. Only `Unsupported` goes on to the next
/// decoder; a `Malformed` or limit error ends the chain, as a throw does in the TS.
pub fn decode_raster(b: &[u8], limits: &ImageLimits) -> Result<Rgba> {
    let decoders: [Decoder; 4] = [decode_png, decode_gif, decode_bmp, decode_tiff];
    for decode in decoders {
        match decode(b, limits) {
            Err(ImageError::Unsupported) => {}
            other => return other,
        }
    }
    Err(ImageError::Unsupported)
}
