//! Parity of the `.ppt` converter (frontier B2): replays `tests/captures/ppt-samples.capture` (the
//! parts the frozen TS `convertPptToPptx` builds for the 25 tracked `.ppt` samples, one fresh module
//! per deck) and compares part names, order and every part's SHA-256 exactly. For the parts of at most
//! 8 KiB the capture keeps the bytes (`z:` + base64 of a raw-deflate stream): a mismatch prints the
//! first differing position with its context.
#![cfg(feature = "ppt")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_pptx::ppt::{ConvertOptions, convert_ppt_to_pptx_parts};
use uo_testkit::sha256::sha256;
use uo_testkit::{Capture, Replay};

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `len:sha256[0..n]` of the bytes.
pub fn digest(b: &[u8], n: usize) -> String {
    format!("{}:{}", b.len(), &hex(&sha256(b))[..n])
}

fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap());
        }
    }
    out
}

/// The bytes the capture stored for part `i` of a record (small parts only).
fn stored(cap: &Capture, key: &str, i: usize) -> Option<Vec<u8>> {
    let z = cap.get(key, &format!("b{i}"))?.strip_prefix("z:")?;
    Some(uo_opc::flate::inflate_raw(&b64(z), None, 1 << 30).unwrap())
}

fn show_diff(name: &str, want: &[u8], got: &[u8]) {
    let at = want.iter().zip(got).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
    let ctx = |b: &[u8]| String::from_utf8_lossy(&b[at.saturating_sub(120)..(at + 160).min(b.len())]).into_owned();
    eprintln!(
        "DIFF {name}: first difference at byte {at} (want {} B, got {} B)\n  want: …{}\n  got:  …{}",
        want.len(),
        got.len(),
        ctx(want),
        ctx(got)
    );
}

/// The date the TS ran with: the automatic date field (`new Date()`, local time) of 30 parts carries
/// the day the capture was taken (2026-11-17). `UO_PPT_TODAY=YYYY-M-D` overrides it (it must then
/// fail: those parts change with the day).
fn today() -> Option<(i32, u32, u32)> {
    let v = std::env::var("UO_PPT_TODAY").unwrap_or_else(|_| "2026-11-17".to_owned());
    let mut it = v.split('-').map(|x| x.parse::<i64>().unwrap());
    Some((i32::try_from(it.next()?).unwrap(), u32::try_from(it.next()?).unwrap(), u32::try_from(it.next()?).unwrap()))
}

/// What the 25 decks exercise, for the record: the parts that contain each tag.
const COVERAGE_TAGS: [&str; 20] = [
    "<a:custGeom>",
    "<a:prstTxWarp",
    "<a:blipFill",
    "<a:duotone",
    "<a:pattFill",
    "<a:gradFill",
    "<p:timing>",
    "<p:bldLst>",
    "<a:fld ",
    "<p:graphicFrame",
    "<p:grpSp>",
    "<a:scene3d>",
    "<a:outerShdw",
    "<a:buAutoNum",
    "<a:hlinkClick",
    "<a:buBlip",
    "<p:cxnSp>",
    "<p:pic>",
    "<p:transition",
    "<a:clrChange",
];

#[test]
fn ppt_samples_parts_are_byte_equal() {
    let cap = Capture::read(captures().join("ppt-samples.capture")).unwrap();
    let mut rp = Replay::new(&cap);
    let (mut decks, mut parts_total, mut parts_equal, mut with_date) = (0usize, 0usize, 0usize, 0usize);
    let mut coverage = [0usize; COVERAGE_TAGS.len() + 1];
    for rec in cap.records() {
        let key = rec.key.as_str();
        let path = repo_root().join("slides/samples").join(key.strip_prefix("samples/").unwrap());
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{key}: {e}"));
        rp.check(key, "input", &digest(&bytes, 16));
        decks += 1;
        let opts = ConvertOptions { today: today(), ..ConvertOptions::default() };
        let parts = match convert_ppt_to_pptx_parts(&bytes, &opts) {
            Ok(p) => p,
            Err(e) => {
                rp.check(key, "convert", &format!("err:{e}"));
                continue;
            }
        };
        rp.check(key, "convert", "ok");
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        rp.check(key, "parts", &format!("{}|{}", parts.len(), names.join("|")));
        for (i, (name, data)) in parts.iter().enumerate() {
            parts_total += 1;
            let got = digest(data, 32);
            let ok = rp.check(key, &format!("p{i}"), &got);
            if ok {
                parts_equal += 1;
            }
            let text = String::from_utf8_lossy(data);
            if text.contains("type=\"datetime1\"") {
                with_date += 1;
            }
            for (k, tag) in COVERAGE_TAGS.iter().enumerate() {
                if text.contains(tag) {
                    coverage[k] += 1;
                }
            }
            if name.starts_with("ppt/media/") || name.starts_with("ppt/diagrams/") || name.starts_with("ppt/notesSlides/") {
                coverage[COVERAGE_TAGS.len()] += 1;
            }
            // the bytes themselves, for the parts the capture kept
            if let Some(want) = stored(&cap, key, i) {
                let facet = format!("b{i}");
                match cap.get(key, &facet) {
                    Some(z) if want == *data => {
                        rp.check(key, &facet, z);
                    }
                    _ => {
                        show_diff(&format!("{key} {name}"), &want, data);
                        rp.check(key, &facet, "bytes differ");
                    }
                }
            }
        }
    }
    let cov: Vec<String> = COVERAGE_TAGS.iter().zip(&coverage).map(|(t, c)| format!("{t}: {c}")).collect();
    eprintln!("coverage (parts containing the tag): {}; media/diagram/notes parts: {}", cov.join(", "), coverage[COVERAGE_TAGS.len()]);
    eprintln!("ppt parity: {decks} decks, {parts_equal}/{parts_total} parts byte-equal ({with_date} parts carry the automatic date field)");
    rp.finish().assert_clean();
}
