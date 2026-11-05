//! Fuzz target of `uo-image` (record `core/docs/modules/uo-image.md`): `image_decode`.
//!
//! Layout: one selector byte (bit 7: tight limits, so that the limit paths run on small inputs;
//! `% 6`: raster, PNG, GIF, BMP, TIFF, every decoder), then the image bytes. The target fails by
//! panicking: any panic of a decoder, and the invariants below (a decoded image has exactly
//! `width * height * 4` bytes within the limits, `decode_raster` is the chain of the four decoders,
//! the header reads agree with the JPEG decoder's contract).

use std::path::Path;

use uo_image::{
    ImageError, ImageLimits, Rgba, decode_bmp, decode_gif, decode_png, decode_raster, decode_tiff, image_pixel_size, jpeg_info,
};

use crate::split1;

const NORMAL: ImageLimits = ImageLimits { max_px: 1 << 22, max_dim: 1 << 17, max_inflated: 16 << 20, max_tag_values: 1 << 18 };
type Decoder = fn(&[u8], &ImageLimits) -> Result<Rgba, ImageError>;

const TIGHT: ImageLimits = ImageLimits { max_px: 600, max_dim: 40, max_inflated: 4096, max_tag_values: 24 };

fn check(r: &Result<Rgba, ImageError>, l: &ImageLimits) {
    if let Ok(img) = r {
        let px = u64::from(img.width) * u64::from(img.height);
        assert!(px <= l.max_px, "{} x {} above max_px {}", img.width, img.height, l.max_px);
        assert!(u64::from(img.width) <= u64::from(l.max_dim) && u64::from(img.height) <= u64::from(l.max_dim));
        assert_eq!(img.data.len() as u64, px * 4, "RGBA buffer of {} x {}", img.width, img.height);
    }
}

pub fn image_decode(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let l = if sel & 0x80 != 0 { TIGHT } else { NORMAL };
    let decoders: [Decoder; 5] = [decode_raster, decode_png, decode_gif, decode_bmp, decode_tiff];
    match sel % 6 {
        5 => {
            let results: Vec<_> = decoders[1..].iter().map(|d| d(bytes, &l)).collect();
            for r in &results {
                check(r, &l);
            }
            // `decodeRaster` is the chain: the first result that is not `Unsupported` decides
            let chain = results.iter().find(|r| !matches!(r, Err(ImageError::Unsupported)));
            let raster = decode_raster(bytes, &l);
            check(&raster, &l);
            match chain {
                Some(r) => assert_eq!(&raster, r, "decode_raster differs from the chain"),
                None => assert_eq!(raster, Err(ImageError::Unsupported)),
            }
        }
        i => check(&decoders[usize::from(i)](bytes, &l), &l),
    }
    if let Some(j) = jpeg_info(bytes) {
        let s = image_pixel_size(bytes).expect("a JPEG header has a size");
        assert_eq!((s.width, s.height), (j.width, j.height));
    }
    let _ = image_pixel_size(bytes);
}

/// The images of the generated pack of `tests/corpus` as seeds of the all-decoders mode (the
/// pack: `"UOIMGPK1"`, `u32` count, then `u16` name length, name, `u32` length, bytes).
pub fn image_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let Ok(pack) = std::fs::read(repo.join("core/crates/uo-image/tests/corpus/generated.pack")) else { return Vec::new() };
    let u32_at = |i: usize| pack.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut seeds = Vec::new();
    let (Some(count), true) = (u32_at(8), pack.starts_with(b"UOIMGPK1")) else { return seeds };
    let mut at = 12;
    for _ in 0..count {
        let Some(nl) = pack.get(at..at + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]]))) else { break };
        at += 2 + nl;
        let Some(len) = u32_at(at) else { break };
        let Some(image) = pack.get(at + 4..at + 4 + len) else { break };
        at += 4 + len;
        // the small ones: a seed is a starting point, not a corpus of its own
        if image.len() <= 4096 {
            let mut seed = vec![5u8];
            seed.extend_from_slice(image);
            seeds.push(("image_decode", seed));
        }
    }
    seeds
}
