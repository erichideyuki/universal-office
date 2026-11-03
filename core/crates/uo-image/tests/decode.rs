//! Behaviour of the decoders on small hand-made images: the exact pixels, the error classes, the limits and
//! the `decodeRaster` chain. (The parity with the TS over thousands of images is `image_parity.rs`.)
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::{bmp24, gif, png, tiff_rgb};
use uo_image::{
    ImageError, ImageLimits, LimitKind, decode_bmp, decode_gif, decode_png, decode_raster, decode_tiff, image_pixel_size, jpeg_info,
};
use uo_opc::flate;

const L: ImageLimits = ImageLimits::NATIVE;

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = ty.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&flate::crc32(&body).to_be_bytes());
}

/// A PNG with the given header fields, extra chunks (before the data) and raw scanline data.
fn make_png(w: u32, h: u32, depth: u8, ctype: u8, interlace: u8, extra: &[(&[u8; 4], Vec<u8>)], raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, ctype, 0, 0, interlace]);
    chunk(&mut out, b"IHDR", &ihdr);
    for (ty, data) in extra {
        chunk(&mut out, ty, data);
    }
    chunk(&mut out, b"IDAT", &flate::zlib(raw, 6));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn png_rgba8_with_every_filter() {
    // 2x5 RGBA: filter types 0..4 on rows that are easy to predict
    let raw = [
        0, 10, 20, 30, 40, 50, 60, 70, 80, // none
        1, 1, 1, 1, 1, 2, 2, 2, 2, // sub: second pixel = first + (2,2,2,2)... after the first (1,1,1,1)
        2, 5, 5, 5, 5, 6, 6, 6, 6, // up
        3, 1, 1, 1, 1, 1, 1, 1, 1, // average
        4, 1, 1, 1, 1, 1, 1, 1, 1, // paeth
    ];
    let img = decode_png(&make_png(2, 5, 8, 6, 0, &[], &raw), &L).unwrap();
    assert_eq!((img.width, img.height), (2, 5));
    assert_eq!(&img.data[..8], &[10, 20, 30, 40, 50, 60, 70, 80]);
    assert_eq!(&img.data[8..16], &[1, 1, 1, 1, 3, 3, 3, 3]);
    assert_eq!(&img.data[16..24], &[6, 6, 6, 6, 9, 9, 9, 9]);
    // average: left 0, up (6,6,6,6) -> 1 + 3; then 1 + ((4 + 9) >> 1)
    assert_eq!(&img.data[24..32], &[4, 4, 4, 4, 7, 7, 7, 7]);
    // paeth: `up` at the first pixel (4 + 1), then up again for the second (left 5, up 7, upper-left 4: 8)
    assert_eq!(&img.data[32..40], &[5, 5, 5, 5, 8, 8, 8, 8]);
}

#[test]
fn png_gray_depths_and_transparency_key() {
    // 1-bit gray, 8 pixels in one byte 0b1010_0000 (5 columns used), key = 0 -> the 0 pixels are transparent
    let img = decode_png(&make_png(4, 1, 1, 0, 0, &[(b"tRNS", vec![0, 0])], &[0, 0b1010_0000]), &L).unwrap();
    assert_eq!(img.data, [255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
    // 2-bit gray scales by 255/3
    let img = decode_png(&make_png(4, 1, 2, 0, 0, &[], &[0, 0b00_01_10_11]), &L).unwrap();
    assert_eq!(img.data.chunks(4).map(|p| p[0]).collect::<Vec<_>>(), [0, 85, 170, 255]);
    // 16-bit gray keeps the high byte; the key compares the whole sample
    let img = decode_png(&make_png(2, 1, 16, 0, 0, &[(b"tRNS", vec![0x12, 0x34])], &[0, 0x12, 0x34, 0xab, 0xcd]), &L).unwrap();
    assert_eq!(img.data, [0x12, 0x12, 0x12, 0, 0xab, 0xab, 0xab, 255]);
}

#[test]
fn png_palette_with_a_short_table() {
    let img =
        decode_png(&make_png(3, 1, 8, 3, 0, &[(b"PLTE", vec![255, 0, 0, 0, 255, 0]), (b"tRNS", vec![128])], &[0, 0, 1, 7]), &L).unwrap();
    // index 0: red, half transparent; 1: green; 7: outside the table: black (undefined reads as 0), opaque
    assert_eq!(img.data, [255, 0, 0, 128, 0, 255, 0, 255, 0, 0, 0, 255]);
    // no palette at all: black
    let img = decode_png(&make_png(1, 1, 8, 3, 0, &[], &[0, 3]), &L).unwrap();
    assert_eq!(img.data, [0, 0, 0, 255]);
}

#[test]
fn png_adam7_places_the_passes() {
    // 3x3 gray 8-bit: the pixels are numbered in the order the seven passes store them
    let (w, h) = (3u32, 3u32);
    let mut raw = Vec::new();
    let mut n = 0u8;
    for (ox, oy, dx, dy) in [(0u32, 0u32, 8u32, 8u32), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)] {
        let pw = (w + dx - 1).saturating_sub(ox) / dx;
        let ph = (h + dy - 1).saturating_sub(oy) / dy;
        if w <= ox || h <= oy {
            continue;
        }
        for _ in 0..ph {
            raw.extend_from_slice(&[0]);
            for _ in 0..pw {
                n += 1;
                raw.push(n * 10);
            }
        }
    }
    let img = decode_png(&make_png(w, h, 8, 0, 1, &[], &raw), &L).unwrap();
    let gray: Vec<u8> = img.data.chunks(4).map(|p| p[0]).collect();
    // pass 1: (0,0) = 10; pass 4: (2,0) = 20; pass 5: (0,2) = 30 and (2,2) = 40; pass 6: (1,0) = 50 and (1,2) = 60;
    // pass 7: row 1 = 70 80 90
    assert_eq!(gray, [10, 50, 20, 70, 80, 90, 30, 60, 40]);
}

#[test]
fn png_short_data_decodes_what_it_has() {
    // 2x3 RGB8 with data for one and a half rows: the rest reads as undefined, which stores as 0 (opaque)
    let img = decode_png(&make_png(2, 3, 8, 2, 0, &[], &[0, 1, 2, 3, 4, 5, 6, 0, 7, 8, 9]), &L).unwrap();
    assert_eq!(&img.data[..8], &[1, 2, 3, 255, 4, 5, 6, 255]);
    assert_eq!(&img.data[8..12], &[7, 8, 9, 255]);
    assert_eq!(&img.data[12..], &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
}

#[test]
fn png_error_classes() {
    // not a PNG
    assert_eq!(decode_png(b"GIF89a", &L).unwrap_err(), ImageError::Unsupported);
    assert_eq!(decode_png(&[], &L).unwrap_err(), ImageError::Unsupported);
    // no size
    assert_eq!(decode_png(&make_png(0, 4, 8, 2, 0, &[], &[]), &L).unwrap_err(), ImageError::Unsupported);
    // a damaged zlib stream is null, however large the header says the image is
    let mut bad = make_png(100_000, 100_000, 8, 6, 0, &[], &[0; 16]);
    let n = bad.len();
    bad[n - 20] ^= 0xff;
    let r = decode_png(&bad, &L);
    assert!(matches!(r, Err(ImageError::Limit(LimitKind::Pixels) | ImageError::Unsupported)), "{r:?}");
    // an IHDR cut off by the end of the file throws in the TS
    let mut cut = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0];
    assert!(matches!(decode_png(&cut, &L), Err(ImageError::Malformed(_))));
    cut.truncate(20);
    assert!(matches!(decode_png(&cut, &L), Err(ImageError::Malformed(_))));
}

#[test]
fn gif_two_by_two_with_transparency() {
    // 2x2 screen, global table of 4 colours, transparent index 1, indices 0 1 / 2 3 coded with min code size 2
    let mut g = b"GIF89a".to_vec();
    g.extend_from_slice(&[2, 0, 2, 0, 0x81, 0, 0]);
    g.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]);
    g.extend_from_slice(&[0x21, 0xf9, 4, 1, 0, 0, 1, 0]);
    g.extend_from_slice(&[0x2c, 0, 0, 0, 0, 2, 0, 2, 0, 0, 2]);
    // codes: clear 4, 0, 1, 2 (3 bits each), then the dictionary has 8 entries and the width is 4 bits: 3, eoi 5
    let codes = [(4u32, 3), (0, 3), (1, 3), (2, 3), (3, 4), (5, 4)];
    let (mut acc, mut n, mut data) = (0u32, 0, Vec::new());
    for (c, width) in codes {
        acc |= c << n;
        n += width;
        while n >= 8 {
            data.push(acc as u8);
            acc >>= 8;
            n -= 8;
        }
    }
    if n > 0 {
        data.push(acc as u8);
    }
    g.push(data.len() as u8);
    g.extend_from_slice(&data);
    g.extend_from_slice(&[0, 0x3b]);
    let img = decode_gif(&g, &L).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.data, [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 255, 9, 9, 9, 255]);
    // the same file with an invalid minimum code size is null
    let at = g.iter().position(|&b| b == 0x2c).unwrap() + 10;
    g[at] = 12;
    assert_eq!(decode_gif(&g, &L).unwrap_err(), ImageError::Unsupported);
    // a file with no frame is null
    assert_eq!(decode_gif(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;", &L).unwrap_err(), ImageError::Unsupported);
}

#[test]
fn bmp_bottom_up_24_bit_and_1_bit() {
    // 2x2, 24 bpp, rows padded to 8 bytes; the first row in the file is the bottom row
    let mut b = bmp24(2, 2, 1);
    let off = 54;
    b[off..off + 12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10]);
    b[off + 12..off + 16].copy_from_slice(&[11, 12, 0, 0]);
    let img = decode_bmp(&b, &L).unwrap();
    // BGR -> RGB, bottom row first
    assert_eq!(&img.data[8..16], &[3, 2, 1, 255, 6, 5, 4, 255]);
    // a negative height is top-down
    let mut td = b.clone();
    td[22..26].copy_from_slice(&(-2i32).to_le_bytes());
    let img = decode_bmp(&td, &L).unwrap();
    assert_eq!(&img.data[..8], &[3, 2, 1, 255, 6, 5, 4, 255]);
    // compression other than none and BITFIELDS, and a zero size, are null
    let mut rle = b.clone();
    rle[30] = 1;
    assert_eq!(decode_bmp(&rle, &L).unwrap_err(), ImageError::Unsupported);
    let mut zero = b.clone();
    zero[18..22].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(decode_bmp(&zero, &L).unwrap_err(), ImageError::Unsupported);
    // a header cut off throws
    assert!(matches!(decode_bmp(&b[..20], &L), Err(ImageError::Malformed(_))));
}

#[test]
fn tiff_rgb_strip_round_trip() {
    let t = tiff_rgb(5, 4, 9);
    let img = decode_tiff(&t, &L).unwrap();
    assert_eq!((img.width, img.height), (5, 4));
    let pix = &t[8..8 + 60];
    for (i, px) in img.data.chunks(4).enumerate() {
        assert_eq!(&px[..3], &pix[i * 3..i * 3 + 3]);
        assert_eq!(px[3], 255);
    }
    // not a TIFF; a wrong magic is null; a short header throws
    assert_eq!(decode_tiff(b"nope", &L).unwrap_err(), ImageError::Unsupported);
    assert_eq!(decode_tiff(b"II+\0\x08\0\0\0", &L).unwrap_err(), ImageError::Unsupported);
    assert!(matches!(decode_tiff(b"II*\0", &L), Err(ImageError::Malformed(_))));
}

#[test]
fn headers() {
    let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 20, 0, 33, 3, 0, 0, 0];
    let j = jpeg_info(&jpeg).unwrap();
    assert_eq!((j.width, j.height, j.components, j.adobe), (33, 20, 3, false));
    let size = image_pixel_size(&jpeg).unwrap();
    assert_eq!((size.width, size.height), (33, 20));
    assert!(jpeg_info(&jpeg[..jpeg.len() - 5]).is_none(), "the SOF is cut off");
    assert!(jpeg_info(&[0xff]).is_none());
    let p = png(7, 9, 8, 6, 1);
    let s = image_pixel_size(&p).unwrap();
    assert_eq!((s.width, s.height), (7, 9));
    let g = gif(11, 13, 2);
    let s = image_pixel_size(&g).unwrap();
    assert_eq!((s.width, s.height), (11, 13));
    let mut b = bmp24(3, 4, 3);
    b[22..26].copy_from_slice(&(-4i32).to_le_bytes());
    let s = image_pixel_size(&b).unwrap();
    assert_eq!((s.width, s.height), (3, 4), "a top-down bitmap has a negative height");
    b[22..26].copy_from_slice(&i32::MIN.to_le_bytes());
    assert_eq!(image_pixel_size(&b).unwrap().height, 2_147_483_648);
    assert!(image_pixel_size(&[1, 2, 3]).is_none());
}

#[test]
fn raster_chain() {
    let formats = [png(8, 8, 8, 6, 1), gif(8, 8, 2), bmp24(8, 8, 3), tiff_rgb(8, 8, 4)];
    for f in &formats {
        let img = decode_raster(f, &L).unwrap();
        assert_eq!((img.width, img.height), (8, 8));
    }
    assert_eq!(decode_raster(b"not an image", &L).unwrap_err(), ImageError::Unsupported);
    // a throw ends the chain: a short BMP is Malformed, not Unsupported
    assert!(matches!(decode_raster(b"BM\0\0", &L), Err(ImageError::Malformed(_))));
    // JPEG is not decoded
    assert_eq!(decode_raster(&[0xff, 0xd8, 0xff, 0xd9], &L).unwrap_err(), ImageError::Unsupported);
}

#[test]
fn limits() {
    let tight = ImageLimits { max_px: 100, max_dim: 20, max_inflated: 1 << 20, max_tag_values: 4 };
    // pixels: 11x10 = 110 > 100
    assert_eq!(decode_bmp(&bmp24(11, 10, 1), &tight).unwrap_err(), ImageError::Limit(LimitKind::Pixels));
    assert!(decode_bmp(&bmp24(10, 10, 1), &tight).is_ok());
    // a side
    assert_eq!(decode_bmp(&bmp24(21, 1, 1), &tight).unwrap_err(), ImageError::Limit(LimitKind::Dimension));
    // a GIF frame counts on its own, and so does the screen
    assert_eq!(decode_gif(&gif(11, 10, 1), &tight).unwrap_err(), ImageError::Limit(LimitKind::Pixels));
    // the PNG inflate stops at max_inflated
    let small = ImageLimits { max_inflated: 100, ..L };
    assert_eq!(decode_png(&png(32, 32, 8, 6, 2), &small).unwrap_err(), ImageError::Limit(LimitKind::Inflated));
    // TIFF raw samples
    assert_eq!(decode_tiff(&tiff_rgb(32, 32, 3), &small).unwrap_err(), ImageError::Limit(LimitKind::Inflated));
    // TIFF tag values: the IFD of tiff_rgb keeps 3 + 1 + 1... values, more than 4
    assert_eq!(decode_tiff(&tiff_rgb(2, 2, 3), &tight).unwrap_err(), ImageError::Limit(LimitKind::TagValues));
    // the defaults of the target are the wasm32 baseline there and the native limits here
    let d = ImageLimits::default();
    assert_eq!(d, if cfg!(target_arch = "wasm32") { ImageLimits::WASM32 } else { ImageLimits::NATIVE });
    const { assert!(ImageLimits::WASM32.max_px < ImageLimits::NATIVE.max_px) };
}
