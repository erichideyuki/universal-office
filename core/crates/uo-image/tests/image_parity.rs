//! Parity replay of the codecs against `tests/captures/image.capture`, written by
//! `tools/capture-image.mjs` from the frozen TS (`write/packages/docx-layout/dist/images.js`, Node 24.15.0):
//! `jpegInfo`, `imagePixelSize`, `decodePng`, `decodeGif`, `decodeBmp` and `decodeTiff` over
//!  - the oracle images (every distinct image of the tracked OOXML packages and image files),
//!  - the generated images (`tests/corpus/generated.pack`: every PNG colour type and depth, Adam7, GIF
//!    frames, BMP variants, TIFF strips and tiles, JPEG headers),
//!  - five deterministic mutations of each (the mutator below mirrors the script's).
//!
//! A decoder result is `N` (TS `null`: `Unsupported`), `E` (the TS threw, or asked for more than 64 MiB of
//! array: `Malformed` or a limit here, whose thresholds are the same numbers, see `LIMITS`) or `K:<w>x<h>:<digest
//! of the RGBA bytes>`. `decodeRaster` is checked against the `??` chain of the captured facets.
//!
//! Deviations are named: each is a decision listed in `core/docs/modules/uo-image.md`, counted here, and an
//! unlisted difference fails the test.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use uo_image::{
    ImageError, ImageLimits, Rgba, decode_bmp, decode_gif, decode_png, decode_raster, decode_tiff, image_pixel_size, jpeg_info,
};
use uo_opc::Package;
use uo_opc::limits::Limits;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Digest, Replay};

const CAPTURE: &str = include_str!("captures/image.capture");
mod support;

use support::{mutate, pack};

/// The script's `Uint8Array` allocation limit is 64 MiB (4 MiB for the mutations): `width * height * 4 <= 64 MiB` is
/// 16 Mi pixels (1 Mi), and every other intermediate buffer is capped at the same bytes. Sides and tag values are
/// not limited there.
const LIMITS: ImageLimits = ImageLimits { max_px: 16 << 20, max_dim: u32::MAX, max_inflated: 64 << 20, max_tag_values: u64::MAX };
const MUT_LIMITS: ImageLimits = ImageLimits { max_px: 1 << 20, max_dim: u32::MAX, max_inflated: 4 << 20, max_tag_values: u64::MAX };

fn capture() -> Capture {
    Capture::parse(CAPTURE).unwrap()
}

fn repo() -> PathBuf {
    std::env::var_os("UO_REPO_ROOT").map_or_else(|| uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR")), PathBuf::from)
}

/// The bytes of an oracle key: `oracle/<path>!<part>` (a part of an OOXML package) or `oracle/<path>`.
fn oracle_bytes(key: &str, packages: &mut HashMap<String, Package>) -> Vec<u8> {
    let rest = key.strip_prefix("oracle/").unwrap();
    let root = repo();
    match rest.split_once('!') {
        None => std::fs::read(root.join(rest)).unwrap(),
        Some((file, part)) => {
            let pkg = packages.entry(file.to_owned()).or_insert_with(|| {
                let bytes = std::fs::read(Path::new(&root).join(file)).unwrap();
                Package::open_owned(bytes, &Limits::NATIVE, &OpenOptions::default()).unwrap()
            });
            pkg.part(part).unwrap().unwrap().to_vec()
        }
    }
}

// ── the facets ──────────────────────────────────────────────────────────────────────────────────

fn out(r: &Result<Rgba, ImageError>) -> String {
    match r {
        Ok(img) => format!("K:{}x{}:{}", img.width, img.height, Digest::of(&img.data).short()),
        Err(ImageError::Unsupported) => "N".into(),
        Err(_) => "E".into(),
    }
}

fn jpeg_out(b: &[u8]) -> String {
    jpeg_info(b).map_or("N".into(), |j| format!("{}x{}x{}{}", j.width, j.height, j.components, if j.adobe { "a" } else { "" }))
}

fn size_out(b: &[u8]) -> String {
    image_pixel_size(b).map_or("N".into(), |s| format!("{}x{}", s.width, s.height))
}

const D_FRACTION: &str =
    "D-FRACTION: a RATIONAL (fractional) value in a TIFF size or count tag is Unsupported (the TS computes with the fraction)";

/// Does the TIFF hold a fractional value (type RATIONAL, numerator not a multiple of the denominator, a zero
/// denominator counting as 1) in the first value of a tag whose value the decoder uses as a whole number? An
/// independent reader of the IFD, so it shares nothing with the decoder.
fn tiff_has_fractional_tag(b: &[u8]) -> bool {
    let le = b.starts_with(b"II");
    let u16_at = |o: usize| b.get(o..o + 2).map(|x| if le { u16::from_le_bytes([x[0], x[1]]) } else { u16::from_be_bytes([x[0], x[1]]) });
    let u32_at = |o: usize| {
        b.get(o..o + 4)
            .map(|x| if le { u32::from_le_bytes([x[0], x[1], x[2], x[3]]) } else { u32::from_be_bytes([x[0], x[1], x[2], x[3]]) })
    };
    let Some(ifd) = u32_at(4) else { return false };
    let Some(n) = u16_at(ifd as usize) else { return false };
    for i in 0..usize::from(n) {
        let e = ifd as usize + 2 + i * 12;
        let (Some(tag), Some(ty), Some(count)) = (u16_at(e), u16_at(e + 2), u32_at(e + 4)) else { return false };
        if ty != 5 || count == 0 || ![256, 257, 258, 259, 262, 277, 278, 284, 317, 322, 323, 530].contains(&tag) {
            continue;
        }
        let Some(base) = u32_at(e + 8) else { return false };
        let values = if tag == 530 { 2.min(count) } else { 1 };
        for k in 0..values as usize {
            let (Some(num), Some(den)) = (u32_at(base as usize + k * 8), u32_at(base as usize + k * 8 + 4)) else { return false };
            if num % den.max(1) != 0 {
                return true;
            }
        }
    }
    false
}

/// Why a result may differ from the TS (a decision of the module record); `None` is a mismatch.
fn deviation(bytes: &[u8], facet: &str, expected: &str, actual: &str) -> Option<&'static str> {
    if facet == "tiff" && actual == "N" && expected.starts_with('K') && tiff_has_fractional_tag(bytes) {
        return Some(D_FRACTION);
    }
    None
}

#[derive(Default)]
struct Tally {
    equal: u64,
    /// per outcome class of the TS (K, N, E), the images where the Rust agrees
    by_class: BTreeMap<String, u64>,
    accepted: BTreeMap<&'static str, u64>,
    mismatches: Vec<String>,
}

fn check(t: &mut Tally, replay: &mut Replay<'_>, key: &str, bytes: &[u8], facet: &str, actual: &str) {
    check_with(t, replay, key, bytes, facet, actual, "");
}

fn check_with(t: &mut Tally, replay: &mut Replay<'_>, key: &str, bytes: &[u8], facet: &str, actual: &str, detail: &str) {
    let expected = replay.capture().get(key, facet).unwrap_or("").to_owned();
    let reason = if expected == actual { None } else { deviation(bytes, facet, &expected, actual) };
    if let Some(reason) = reason {
        *t.accepted.entry(reason).or_default() += 1;
        replay.accept(key, facet, actual, reason);
    } else if replay.check(key, facet, actual) {
        t.equal += 1;
        *t.by_class.entry(format!("{facet}:{}", &expected[..1])).or_default() += 1;
    } else if t.mismatches.len() < 400 {
        t.mismatches.push(format!("{key} [{facet}]: TS {expected}, Rust {actual} {detail}"));
    }
}

/// Replays the records of `keep`; `source` gives the bytes of a key.
fn run(name: &str, limits: &ImageLimits, keep: impl Fn(&str) -> bool, mut source: impl FnMut(&str) -> Vec<u8>) {
    let full = capture();
    let cap = full.subset(|k| keep(k));
    assert!(!cap.is_empty(), "{name}: nothing to replay");
    let mut replay = Replay::new(&cap);
    let mut tally = Tally::default();
    for record in cap.records() {
        let key = record.key.as_str();
        let bytes = source(key);
        if !replay.check(key, "in", &Digest::of(&bytes).short()) {
            for (f, _) in &record.facets {
                replay.skip(key, f);
            }
            tally.mismatches.push(format!("{key}: the input changed (recapture)"));
            continue;
        }
        check(&mut tally, &mut replay, key, &bytes, "jpeg", &jpeg_out(&bytes));
        check(&mut tally, &mut replay, key, &bytes, "size", &size_out(&bytes));
        let results = [
            ("png", decode_png(&bytes, limits)),
            ("gif", decode_gif(&bytes, limits)),
            ("bmp", decode_bmp(&bytes, limits)),
            ("tiff", decode_tiff(&bytes, limits)),
        ];
        for (facet, r) in &results {
            check_with(&mut tally, &mut replay, key, &bytes, facet, &out(r), &r.as_ref().err().map_or(String::new(), |e| format!("{e:?}")));
        }
        // `decodeRaster` is the `??` chain: the first facet of the TS that is not null decides (a throw ends it)
        let chain = ["png", "gif", "bmp", "tiff"].iter().map(|f| cap.get(key, f).unwrap()).find(|v| *v != "N").unwrap_or("N").to_owned();
        let raster = out(&decode_raster(&bytes, limits));
        // the chain of the captured facets is the TS result; where a facet deviates the chain is compared on the Rust facets
        let rust_chain = results.iter().map(|(_, r)| out(r)).find(|v| v != "N").unwrap_or_else(|| "N".into());
        if raster != rust_chain {
            tally.mismatches.push(format!("{key} [raster]: chain of the Rust decoders {rust_chain}, decode_raster {raster}"));
        } else if raster == chain {
            tally.equal += 1;
        }
    }
    let report = replay.finish();
    println!("{name}: {} records, {} facets equal, accepted {:?}", cap.len(), tally.equal, tally.accepted);
    println!("{name}: agreeing outcomes {:?}", tally.by_class);
    for m in &tally.mismatches {
        println!("MISMATCH {m}");
    }
    assert!(tally.mismatches.is_empty(), "{name}: {} mismatches (listed above; recapture needs UO_TS_ROOT)", tally.mismatches.len());
    assert!(report.mismatches.is_empty(), "{name}: {}", report.describe(20));
    let _ = &report.accepted;
    assert!(report.missing.is_empty(), "{name}: captured facets never replayed: {:?}", report.missing.iter().take(5).collect::<Vec<_>>());
}

#[test]
fn generated_images() {
    let pack = pack();
    run("generated", &LIMITS, |k| k.starts_with("gen/"), |k| pack.get(k).unwrap_or_else(|| panic!("{k} missing from the pack")).clone());
}

#[test]
fn oracle_images() {
    let mut packages = HashMap::new();
    run("oracle", &LIMITS, |k| k.starts_with("oracle/"), |k| oracle_bytes(k, &mut packages));
}

#[test]
fn mutated_images() {
    let pack = pack();
    let mut packages = HashMap::new();
    run(
        "mutated",
        &MUT_LIMITS,
        |k| k.starts_with("mut/"),
        |k| {
            let rest = k.strip_prefix("mut/").unwrap();
            let (orig, m) = rest.rsplit_once('#').unwrap();
            let src = if orig.starts_with("oracle/") { oracle_bytes(orig, &mut packages) } else { pack.get(orig).unwrap().clone() };
            mutate(&src, orig, m.parse().unwrap())
        },
    );
}
