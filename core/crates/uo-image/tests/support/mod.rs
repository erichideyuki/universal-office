//! Shared by the test binaries: the pack of generated images and the deterministic mutator (the one of
//! `tools/capture-image.mjs`).
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;

use uo_testkit::generate::Rng;

const PACK: &[u8] = include_bytes!("../corpus/generated.pack");

/// The generated images in the order of the pack.
pub fn pack_list() -> Vec<(String, Vec<u8>)> {
    assert_eq!(PACK.get(..8), Some(&b"UOIMGPK1"[..]));
    let u32_at = |i: usize| u32::from_le_bytes(PACK[i..i + 4].try_into().unwrap()) as usize;
    let count = u32_at(8);
    let mut at = 12;
    let mut out = Vec::new();
    for _ in 0..count {
        let nl = usize::from(u16::from_le_bytes(PACK[at..at + 2].try_into().unwrap()));
        let name = String::from_utf8(PACK[at + 2..at + 2 + nl].to_vec()).unwrap();
        at += 2 + nl;
        let len = u32_at(at);
        out.push((name, PACK[at + 4..at + 4 + len].to_vec()));
        at += 4 + len;
    }
    assert_eq!(at, PACK.len());
    out
}

pub fn pack() -> HashMap<String, Vec<u8>> {
    pack_list().into_iter().collect()
}

// ── the mutator of the script ───────────────────────────────────────────────────────────────────

const HOSTILE: [u32; 9] = [0, 0xffff_ffff, 0x7fff_ffff, 0x8000_0000, 1, 0x10000, 0xffff, 0x100, 0x4000_0000];

pub fn fnv(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, c| (h ^ u32::from(c)).wrapping_mul(0x0100_0193))
}

pub fn mutate(b: &[u8], name: &str, m: u32) -> Vec<u8> {
    let mut rng = Rng::new(fnv(name) ^ (m + 1).wrapping_mul(0x9e37_79b1));
    let n = b.len();
    let mut a = b.to_vec();
    if n == 0 {
        return a;
    }
    match m % 5 {
        0 => {
            let k = rng.below(n);
            a.truncate(k);
        }
        1 => {
            for _ in 0..1 + rng.below(3) {
                let p = rng.below(n.min(64));
                let bit = rng.below(8);
                a[p] ^= 1 << bit;
            }
        }
        2 => {
            for _ in 0..1 + rng.below(3) {
                let p = rng.below(n);
                let bit = rng.below(8);
                a[p] ^= 1 << bit;
            }
        }
        3 => {
            for _ in 0..1 + rng.below(3) {
                let p = rng.below(n);
                a[p] = rng.below(256) as u8;
            }
        }
        _ => {
            for _ in 0..1 + rng.below(2) {
                let p = rng.below(n.min(128));
                let v = HOSTILE[rng.below(HOSTILE.len())];
                for j in 0..4 {
                    if p + j < n {
                        a[p + j] = (v >> (8 * j)) as u8;
                    }
                }
            }
        }
    }
    a
}

// ── builders for the large images of the memory and limit tests ─────────────────────────────────

fn content(rng: &mut Rng, n: usize) -> Vec<u8> {
    let mut v = rng.below(256) as u8;
    (0..n)
        .map(|_| {
            if rng.below(16) == 0 {
                v = rng.below(256) as u8;
            } else {
                v = v.wrapping_add(rng.below(7) as u8).wrapping_sub(3);
            }
            v
        })
        .collect()
}

fn png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = ty.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&uo_opc::flate::crc32(&body).to_be_bytes());
}

/// A non-interlaced PNG of `w` x `h` with the given depth and colour type (random filtered rows).
pub fn png(w: u32, h: u32, depth: u8, ctype: u8, seed: u32) -> Vec<u8> {
    let channels = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let stride = (w as usize * channels * usize::from(depth)).div_ceil(8);
    let mut rng = Rng::new(seed);
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for y in 0..h {
        raw.push((y % 5) as u8);
        raw.extend(content(&mut rng, stride));
    }
    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, ctype, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    if ctype == 3 {
        png_chunk(&mut out, b"PLTE", &rng.bytes(3 << depth.min(8)));
    }
    png_chunk(&mut out, b"IDAT", &uo_opc::flate::zlib(&raw, 1));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// A 24-bit BMP.
pub fn bmp24(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut rng = Rng::new(seed);
    let pix = content(&mut rng, stride * h as usize);
    let mut out = vec![0x42, 0x4d];
    out.extend_from_slice(&(54 + pix.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&[1, 0, 24, 0]);
    out.extend_from_slice(&[0; 24]);
    out.extend_from_slice(&pix);
    out
}

/// A GIF of 8-bit indices coded as literals with a clear code every 200 codes (a valid, uncompressed stream).
pub fn gif(w: u16, h: u16, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&[0xf7, 0, 0]);
    out.extend_from_slice(&rng.bytes(768));
    out.extend_from_slice(&[0x2c, 0, 0, 0, 0]);
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&[0, 8]);
    let (mut bits, mut nbits) = (0u32, 0u32);
    let mut data = Vec::new();
    let mut put = |code: u32, data: &mut Vec<u8>| {
        bits |= code << nbits;
        nbits += 9;
        while nbits >= 8 {
            data.push(bits as u8);
            bits >>= 8;
            nbits -= 8;
        }
    };
    put(256, &mut data);
    for i in 0..usize::from(w) * usize::from(h) {
        if i % 200 == 199 {
            put(256, &mut data);
        }
        put(rng.below(256) as u32, &mut data);
    }
    put(257, &mut data);
    if nbits > 0 {
        data.push(bits as u8);
    }
    for block in data.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&[0, 0x3b]);
    out
}

/// An uncompressed little-endian RGB TIFF with one strip.
pub fn tiff_rgb(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let pix = content(&mut rng, w as usize * h as usize * 3);
    let entries: [(u16, u16, u32, u32); 9] = [
        (256, 4, 1, w),
        (257, 4, 1, h),
        (258, 3, 3, 0),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 8),
        (277, 3, 1, 3),
        (278, 4, 1, h),
        (279, 4, 1, pix.len() as u32),
    ];
    let ifd = 8 + pix.len() as u32;
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&ifd.to_le_bytes());
    out.extend_from_slice(&pix);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let heap = ifd + 2 + 12 * entries.len() as u32 + 4;
    for (tag, ty, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        if tag == 258 {
            out.extend_from_slice(&heap.to_le_bytes());
        } else if ty == 3 {
            out.extend_from_slice(&(value as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    for _ in 0..3 {
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    out
}
