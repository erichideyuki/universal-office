//! PP0 of the painter (record "Milestone 1 — frontier P"): replays `tests/parity/painter.cap`, captured from
//! the frozen TS `renderPdf` by `tests/parity_support/capture_painter.mjs`.
//!
//! - `raw`: the file made with `compress: false` is compared byte for byte (length and SHA-256);
//! - `norm`: the file made with `compress: true` is compared structurally (same object numbers, dictionaries equal
//!   apart from `/Filter` and `/Length`, inflated streams equal), and its xref and trailer are validated; the streams
//!   that were deflated on one side and stored on the other (the "only if smaller" rule near its edge) are counted.
//!
//! With `UO_PAINTER_CAPTURE=<file> UO_PAINTER_DUMP=<dir>` the same replay runs over a capture of real documents
//! (`capture_painter.mjs --docs`), whose specs and faces are in the dump (never committed: the faces may not be open).
//! With `UO_PAINTER_TS=<dir>` (the `--dump` of a generated capture) a mismatch prints the first differing byte.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::path::{Path, PathBuf};

use support::pages::{Loaded, Sources, load_text};
use support::pdf_parse::normalize;
use uo_pdf::{PdfOptions, render_pdf};
use uo_testkit::stream::Digest;
use uo_testkit::{Capture, Replay};

fn first_diff(a: &[u8], b: &[u8]) -> String {
    let at = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let ctx = |s: &[u8]| String::from_utf8_lossy(&s[at.saturating_sub(120)..(at + 80).min(s.len())]).into_owned();
    format!(
        "first difference at byte {at} (rust {} bytes, ts {} bytes)\n--- rust ---\n{}\n--- ts ---\n{}",
        a.len(),
        b.len(),
        ctx(a),
        ctx(b)
    )
}

struct Totals {
    records: u32,
    objects: u64,
    ts_deflated: u64,
    rust_deflated: u64,
    flips: u32,
    throws: u32,
}

fn replay_capture(cap: &Capture, spec_of: &dyn Fn(&str) -> (String, Sources), min_records: u32) -> Totals {
    let mut replay = Replay::new(cap);
    let mut t = Totals { records: 0, objects: 0, ts_deflated: 0, rust_deflated: 0, flips: 0, throws: 0 };
    let ts_dir = std::env::var("UO_PAINTER_TS").ok().map(PathBuf::from);
    for rec in cap.records() {
        let key = rec.key.as_str();
        if cap.get(key, "error").is_some() {
            replay.skip(key, "error");
            continue;
        }
        if cap.get(key, "raw") == Some("throw") {
            t.throws += 1;
            replay.skip(key, "raw");
            continue;
        }
        let (text, src) = spec_of(key);
        let Loaded { pages, res, nav } = load_text(&text, &src);
        t.records += 1;
        replay.skip(key, "spec");
        let raw = render_pdf(&pages, &res, nav.as_ref(), &PdfOptions { compress: false, ..PdfOptions::default() })
            .unwrap_or_else(|e| panic!("{key}: {e}"));
        let want = cap.get(key, "raw").unwrap();
        let got = format!("{}:{}", raw.len(), Digest::of(&raw).short());
        if got != want
            && let Some(dir) = &ts_dir
            && let Ok(ts) = std::fs::read(dir.join(format!("{key}.raw.pdf")))
        {
            eprintln!("{key}: {}", first_diff(&raw, &ts));
        }
        replay.check(key, "raw", &got);
        let comp = render_pdf(&pages, &res, nav.as_ref(), &PdfOptions::default()).unwrap();
        let n = normalize(&comp).unwrap_or_else(|e| panic!("{key}: the compressed file is not valid: {e}"));
        let r = normalize(&raw).unwrap_or_else(|e| panic!("{key}: the stored file is not valid: {e}"));
        assert_eq!((n.objects, &n.digest), (r.objects, &r.digest), "{key}: the file differs between modes");
        replay.check(key, "norm", &format!("{}:{}", n.objects, n.digest));
        t.objects += u64::from(n.objects);
        let ts_def: u32 = cap.get(key, "deflated").unwrap().parse().unwrap();
        t.ts_deflated += u64::from(ts_def);
        t.rust_deflated += u64::from(n.deflated);
        if ts_def != n.deflated {
            t.flips += 1;
        }
        replay.skip(key, "deflated");
    }
    assert!(t.records >= min_records, "the capture holds {} records", t.records);
    replay.finish().assert_clean();
    t
}

#[test]
fn generated_pages_match_the_ts() {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/painter.cap")).unwrap();
    let t = replay_capture(&cap, &|key| (cap.get(key, "spec").unwrap().to_owned(), Sources::open_fonts()), 40);
    println!(
        "painter parity: {} specs, {} objects; deflated streams TS {}, Rust {}; specs where the count differs: {}",
        t.records, t.objects, t.ts_deflated, t.rust_deflated, t.flips
    );
}

/// The real documents (capture of `--docs`): runs only when the capture and its dump are given.
#[test]
fn real_documents_match_the_ts() {
    let (Ok(cap_path), Ok(dump)) = (std::env::var("UO_PAINTER_CAPTURE"), std::env::var("UO_PAINTER_DUMP")) else { return };
    let cap = Capture::read(Path::new(&cap_path)).unwrap();
    let dump = PathBuf::from(dump);
    let t = replay_capture(
        &cap,
        &|key| {
            let text = std::fs::read_to_string(dump.join(format!("{key}.spec.json"))).unwrap();
            let mut s = Sources::open_fonts();
            s.dump = Some(dump.clone());
            (text, s)
        },
        1,
    );
    println!(
        "real documents: {} ({} where the TS throws), {} objects; deflated streams TS {}, Rust {}; documents where the count differs: {}",
        t.records, t.throws, t.objects, t.ts_deflated, t.rust_deflated, t.flips
    );
}

/// Documents laid out by the frozen TS engine (open fonts only, `--docs … --open-fonts --inline-spec`): the real shape of
/// a LayoutResult - word spaces between runs, headers and footers, tables, page numbers.
#[test]
fn laid_out_documents_match_the_ts() {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/painter-docs.cap")).unwrap();
    let t = replay_capture(&cap, &|key| (cap.get(key, "spec").unwrap().to_owned(), Sources::open_fonts()), 4);
    println!(
        "laid-out documents: {} documents, {} objects; deflated streams TS {}, Rust {}",
        t.records, t.objects, t.ts_deflated, t.rust_deflated
    );
}
