//! PP0 of the writer layer (record "Milestone 1 — frontier W"): replays `tests/parity/writer.cap`,
//! captured from the frozen TS pdf-writer by `tests/parity_support/capture_writer.mjs`.
//!
//! - `raw`: the file made with `compress: false` is compared byte for byte (length and SHA-256);
//! - `norm`: the file made with `compress: true` is compared structurally (same object numbers,
//!   dictionaries equal apart from `/Filter` and `/Length`, inflated streams equal), and its xref
//!   and trailer are validated; the streams that were deflated on one side and stored on the
//!   other (the "only if smaller" rule near its edge, fflate against miniz_oxide) are counted;
//! - `scalars`: `fmt`, `hexColor`, `safePdfUri` and `utf16beHex` over generated inputs.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::path::Path;

use support::inputs::{fmt_inputs, hex_inputs, uri_inputs};
use support::json::parse;
use support::pdf_parse::normalize;
use support::script::run_script;
use uo_jsnum::conv::canonicalize_nan;
use uo_pdf::{PdfError, fmt, hex_color, safe_pdf_uri, utf16be_hex};
use uo_testkit::stream::{Digest, Stream};
use uo_testkit::{Capture, Replay};

fn capture() -> Capture {
    Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/writer.cap")).unwrap()
}

#[test]
fn files_match_the_ts() {
    let cap = capture().subset(|k| k.starts_with("s0"));
    let mut replay = Replay::new(&cap);
    let (mut scripts, mut throws, mut flips, mut streams_ts, mut streams_rust, mut objects) = (0, 0, 0u32, 0u32, 0u32, 0u64);
    for rec in cap.records() {
        let key = rec.key.as_str();
        let script = parse(cap.get(key, "script").unwrap());
        scripts += 1;
        replay.skip(key, "script");
        let want_raw = cap.get(key, "raw").unwrap();
        let raw = run_script(&script, false);
        let comp = run_script(&script, true);
        if want_raw == "throw" {
            throws += 1;
            assert!(matches!(raw, Err(PdfError::Invalid(_))), "{key}: the TS throws, Rust gave {:?}", raw.as_ref().map(Vec::len));
            assert!(matches!(comp, Err(PdfError::Invalid(_))), "{key}");
            replay.check(key, "raw", "throw");
            replay.check(key, "norm", "throw");
            replay.skip(key, "deflated");
            continue;
        }
        let raw = raw.unwrap();
        replay.check(key, "raw", &format!("{}:{}", raw.len(), Digest::of(&raw).short()));
        let comp = comp.unwrap();
        let n = normalize(&comp).unwrap_or_else(|e| panic!("{key}: the compressed file is not valid: {e}"));
        let r = normalize(&raw).unwrap_or_else(|e| panic!("{key}: the stored file is not valid: {e}"));
        assert_eq!((n.objects, &n.digest), (r.objects, &r.digest), "{key}: the file differs between modes");
        replay.check(key, "norm", &format!("{}:{}", n.objects, n.digest));
        objects += u64::from(n.objects);
        let ts_deflated: u32 = cap.get(key, "deflated").unwrap().parse().unwrap();
        streams_ts += ts_deflated;
        streams_rust += n.deflated;
        if ts_deflated != n.deflated {
            flips += 1;
        }
        replay.skip(key, "deflated");
    }
    println!(
        "writer parity: {scripts} scripts ({throws} where the TS throws), {objects} objects; deflated streams TS {streams_ts}, Rust {streams_rust}; \
         scripts where the count differs: {flips}"
    );
    assert!(scripts >= 100, "the capture holds {scripts} scripts");
    replay.finish().assert_clean();
}

#[test]
fn scalars_match_the_ts() {
    let cap = capture().subset(|k| k == "scalars");
    let mut replay = Replay::new(&cap);
    let mut s = Stream::new();
    for v in fmt_inputs() {
        s.str(&fmt(v));
    }
    replay.check("scalars", "fmt", &s.finish().short());
    let mut s = Stream::new();
    for v in hex_inputs() {
        for x in hex_color(&v) {
            s.f64(canonicalize_nan(x));
        }
    }
    replay.check("scalars", "hexColor", &s.finish().short());
    let mut s = Stream::new();
    for v in uri_inputs() {
        match safe_pdf_uri(Some(&v)) {
            None => {
                s.tag(b'n');
            }
            Some(u) => {
                s.tag(b's').str(&u);
            }
        }
    }
    replay.check("scalars", "safePdfUri", &s.finish().short());
    let mut s = Stream::new();
    for v in uri_inputs() {
        s.str(&utf16be_hex(&v));
    }
    replay.check("scalars", "utf16beHex", &s.finish().short());
    replay.finish().assert_clean();
}

/// Writes the Rust files of the capture's scripts (compressed) to `UO_PDF_DUMP`, for
/// `tests/parity_support/check_pdfjs.mjs` (pdf.js opens each and extracts the text).
#[test]
fn dump_files_for_pdfjs() {
    let Ok(dir) = std::env::var("UO_PDF_DUMP") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let cap = capture().subset(|k| k.starts_with("s0"));
    for rec in cap.records() {
        let script = parse(cap.get(&rec.key, "script").unwrap());
        if let Ok(bytes) = run_script(&script, true) {
            std::fs::write(Path::new(&dir).join(format!("{}.pdf", rec.key)), bytes).unwrap();
        }
    }
}
