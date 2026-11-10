//! Parity of the custom-inline tag parser (frontier R4c, `parseCustomInlineTag`: the prefix and
//! type checks, `decodeURIComponent`, `JSON.parse`): replays `tests/parity/custom-tag.capture`,
//! written by `js/capture-custom-tag.mjs` from V8 over `tests/parity/custom-tag.inputs.txt`
//! (hand-written tags, generated JSON percent-encoded in several ways, and their mutations), and
//! compares the digest of `{ type, data }` or `undef` exactly.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-custom-tag.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::read::paragraph::parse_custom_inline_tag;
use uo_testkit::{Capture, Replay, Stream};

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

#[test]
fn custom_tag_matches_v8() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/custom-tag.capture")).unwrap();
    let inputs = std::fs::read_to_string(Path::new(manifest).join("tests/parity/custom-tag.inputs.txt")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut parsed = 0;
    let mut n = 0;
    for (i, line) in inputs.lines().enumerate() {
        let tag = unescape(line);
        let key = i.to_string();
        let value = match parse_custom_inline_tag(Some(&tag)) {
            None => "undef".to_owned(),
            Some((ty, data)) => {
                parsed += 1;
                let mut s = Sink(Stream::new());
                {
                    let mut w = CanonWriter::new(&mut s);
                    w.object(2);
                    w.key("data");
                    data.canon(&mut w);
                    w.key("type");
                    w.str(&ty);
                }
                s.0.finish().short()
            }
        };
        replay.check(&key, "tag", &value);
        n += 1;
    }
    assert_eq!(n, 3020, "the inputs changed: recapture");
    let report = replay.finish();
    println!("custom-tag parity: {} ({parsed} parsed)", report.summary());
    report.assert_clean();
    assert!(parsed > 500);
}
