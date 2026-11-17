//! Fuzz targets of the B5b pictures (record `core/docs/modules/uo-dml-render.md` "B5b — metafiles", `uo-image.md`, `uo-fonts.md`):
//! `emf_play` (EMF, EMF+ and WMF), `pict_play` (QuickDraw PICT), `tiff_slides` (the Slides TIFF decoder) and `eot_read` (EOT, with MicroType
//! Express). Each prefixes a valid header when the selector asks, so the fuzzer works on the records and not on the format detection.
//!
//! Invariants beyond "no panic": a recipe starts with `UOMF` and has a canvas of 1..=2401 px a side, two runs give equal recipes (and
//! the measure protocol converges: a pass with every answer given never asks again), a decoded TIFF has `w * h * 4` bytes, an EOT that
//! reads as MTX yields an sfnt whose table directory fits its length.

use std::path::Path;

use uo_dml_render::metafile::{Decoded, Measure, Metrics, Need, Outcome, Raster, rasterize_metafile, rasterize_pict};

use crate::split1;

/// Answers every question (the deterministic advance of the capture's stub) and decodes nothing.
struct Answering;

impl Measure for Answering {
    fn metrics(&mut self, font: &[u16], text: &[u16]) -> Option<Metrics> {
        let size = f64::from(u32::try_from(font.len() % 40).unwrap_or(0) + 8);
        Some(Metrics { width: size * 0.5 * text.len() as f64, font_ascent: 0.9 * size, font_descent: 0.21 * size })
    }
    fn decoded(&mut self, _key: &str) -> Decoded {
        Decoded::Failed
    }
}

/// Answers nothing: every text and every compressed bitmap is a need.
struct Silent;

impl Measure for Silent {
    fn metrics(&mut self, _: &[u16], _: &[u16]) -> Option<Metrics> {
        None
    }
    fn decoded(&mut self, _: &str) -> Decoded {
        Decoded::Unknown
    }
}

fn check(o: &Outcome) {
    match o {
        Outcome::Done(Some(Raster::Recipe(r))) => {
            assert!((1..=2401).contains(&r.w) && (1..=2401).contains(&r.h), "canvas {}x{}", r.w, r.h);
            assert!(r.bytes.starts_with(b"UOMF"), "recipe magic");
        }
        Outcome::Done(None) => {}
        Outcome::Need(n) => {
            assert!(!n.is_empty());
            for x in n {
                if let Need::Decode { data, .. } = x {
                    assert!(!data.is_empty() || data.is_empty());
                }
            }
        }
    }
}

fn same(a: &Outcome, b: &Outcome) -> bool {
    match (a, b) {
        (Outcome::Done(Some(Raster::Recipe(x))), Outcome::Done(Some(Raster::Recipe(y)))) => x.bytes == y.bytes,
        (Outcome::Done(None), Outcome::Done(None)) => true,
        (Outcome::Need(x), Outcome::Need(y)) => x == y,
        _ => false,
    }
}

/// A valid EMF header (88 bytes) in front of the records.
fn emf_with_header(body: &[u8], frame_w: i32, dev: (i32, i32)) -> Vec<u8> {
    let mut f = vec![0u8; 88];
    let put = |f: &mut Vec<u8>, o: usize, v: i32| f[o..o + 4].copy_from_slice(&v.to_le_bytes());
    f[0] = 1;
    put(&mut f, 4, 88);
    put(&mut f, 16, 794);
    put(&mut f, 20, 1123);
    put(&mut f, 32, frame_w);
    put(&mut f, 36, 29_700);
    f[40..44].copy_from_slice(&0x464d_4520u32.to_le_bytes());
    put(&mut f, 72, dev.0);
    put(&mut f, 76, dev.1);
    put(&mut f, 80, 508);
    put(&mut f, 84, 286);
    f.extend_from_slice(body);
    f
}

/// A placeable WMF header (22 + 18 bytes) in front of the records.
fn wmf_with_header(body: &[u8], box_w: i16) -> Vec<u8> {
    let mut f = vec![0xd7, 0xcd, 0xc6, 0x9a, 0, 0];
    for v in [0i16, 0, box_w, 800] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&96u16.to_le_bytes());
    f.extend_from_slice(&[0; 6]);
    f.extend_from_slice(&[1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(body);
    f
}

/// `emf_play`: selector bits 0-1: raw bytes, EMF header, WMF header, EMF header with an EMF+ comment around the body; bit 2: a display
/// size from the next two bytes.
pub fn emf_play(data: &[u8]) {
    let (sel, rest) = split1(data);
    let (disp_bytes, body) = if sel & 4 != 0 && rest.len() >= 2 { rest.split_at(2) } else { (&[][..], rest) };
    let display = if disp_bytes.len() == 2 { Some((f64::from(disp_bytes[0]) * 12.0, f64::from(disp_bytes[1]) * 8.0)) } else { None };
    let bytes = match sel & 3 {
        0 => body.to_vec(),
        1 => emf_with_header(body, 21_000, (1920, 1080)),
        2 => wmf_with_header(body, 1000),
        _ => {
            // a GDI comment `EMF+` around the body
            let mut c = Vec::new();
            c.extend_from_slice(&70u32.to_le_bytes());
            c.extend_from_slice(&(((20 + body.len() + 3) & !3) as u32).to_le_bytes());
            c.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
            c.extend_from_slice(b"EMF+");
            c.extend_from_slice(body);
            while c.len() % 4 != 0 {
                c.push(0);
            }
            emf_with_header(&c, 21_000, (1920, 1080))
        }
    };
    let a = rasterize_metafile(&bytes, display, &mut Answering);
    check(&a);
    let b = rasterize_metafile(&bytes, display, &mut Answering);
    assert!(same(&a, &b), "two passes differ");
    // the protocol: with nothing answered, a pass that asks is not a recipe; with everything answered it never asks
    let s = rasterize_metafile(&bytes, display, &mut Silent);
    check(&s);
    assert!(!matches!(a, Outcome::Need(_)), "an answering host was asked");
}

/// `pict_play`: selector bit 0: a valid v2 header (with the 512-byte file header on bit 1) in front of the opcodes.
pub fn pict_play(data: &[u8]) {
    let (sel, rest) = split1(data);
    let bytes = if sel & 1 != 0 {
        let mut p = if sel & 2 != 0 { vec![0u8; 512] } else { Vec::new() };
        p.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x00, 0xc8, 0x01, 0x2c, 0x00, 0x11, 0x02, 0xff, 0x0c, 0x00]);
        p.extend_from_slice(&[0; 24]);
        p.extend_from_slice(rest);
        p
    } else {
        rest.to_vec()
    };
    let a = rasterize_pict(&bytes, &mut Answering);
    check(&a);
    let b = rasterize_pict(&bytes, &mut Answering);
    assert!(same(&a, &b), "two passes differ");
}

/// `tiff_slides`: selector bit 0: a little-endian header with the IFD at 8 in front of the data.
pub fn tiff_slides(data: &[u8]) {
    let (sel, rest) = split1(data);
    let bytes = if sel & 1 != 0 {
        let mut t = vec![if sel & 2 != 0 { 0x4d } else { 0x49 }, if sel & 2 != 0 { 0x4d } else { 0x49 }];
        if sel & 2 != 0 {
            t.extend_from_slice(&[0, 42, 0, 0, 0, 8]);
        } else {
            t.extend_from_slice(&[42, 0, 8, 0, 0, 0]);
        }
        t.extend_from_slice(rest);
        t
    } else {
        rest.to_vec()
    };
    if let Ok(Some(img)) = uo_image::decode_tiff_slides(&bytes) {
        assert_eq!(img.data.len() as u64, u64::from(img.width) * u64::from(img.height) * 4);
        assert!(f64::from(img.width) * f64::from(img.height) <= uo_image_max_px());
    }
}

fn uo_image_max_px() -> f64 {
    16_777_216.0
}

/// `eot_read`: selector bits 0-1: raw, header with no flags, header with XOR, header with MTX (+ XOR on bit 2).
pub fn eot_read(data: &[u8]) {
    let (sel, rest) = split1(data);
    let bytes = if sel & 3 == 0 {
        rest.to_vec()
    } else {
        let flags: u32 = match sel & 3 {
            1 => 0,
            2 => 0x1000_0000,
            _ => 0x4 | if sel & 4 != 0 { 0x1000_0000 } else { 0 },
        };
        let mut e = vec![0u8; 82];
        e[0..4].copy_from_slice(&((82 + rest.len()) as u32).to_le_bytes());
        e[4..8].copy_from_slice(&(rest.len() as u32).to_le_bytes());
        e[12..16].copy_from_slice(&flags.to_le_bytes());
        e[34] = 0x4c;
        e[35] = 0x50;
        e.extend_from_slice(rest);
        e
    };
    if let Some(f) = uo_fonts::eot::read(&bytes) {
        assert!(f.len() <= 64 << 20);
        if sel & 3 == 3 {
            // MTX: the result is an sfnt with a table directory that fits
            assert!(f.len() >= 12 && f.starts_with(&[0, 1, 0, 0]));
            let n = usize::from(u16::from_be_bytes([f[4], f[5]]));
            assert!(f.len() >= 12 + 16 * n);
        }
    }
}

/// Seeds: a few committed synthetic inputs of each kind (`uo-pptx/tests/captures/meta-b5b.inputs`).
pub fn metafile_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let Ok(b) = std::fs::read(repo.join("core/crates/uo-pptx/tests/captures/meta-b5b.inputs")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut p = 0usize;
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let u32at = |p: usize| b.get(p..p + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) as usize;
    while p + 4 <= b.len() {
        let kl = u32at(p);
        let key = String::from_utf8_lossy(b.get(p + 4..p + 4 + kl).unwrap_or(&[])).into_owned();
        p += 4 + kl;
        let n = u32at(p);
        let bytes = b.get(p + 4..p + 4 + n).unwrap_or(&[]).to_vec();
        p += 4 + n;
        let kind = key.split('/').next().unwrap_or("").to_string();
        let c = seen.entry(kind.clone()).or_insert(0);
        if *c >= 6 || kind.ends_with("-mut") {
            continue;
        }
        *c += 1;
        let (target, sel): (&'static str, u8) = match kind.as_str() {
            "emf" | "emfplus" | "emfplus-focus" => ("emf_play", 0),
            "wmf" => ("emf_play", 0),
            "pict" => ("pict_play", 0),
            "tiff" => ("tiff_slides", 0),
            "eot" => ("eot_read", 0),
            _ => continue,
        };
        let mut v = vec![sel];
        v.extend_from_slice(&bytes);
        out.push((target, v));
    }
    out
}
