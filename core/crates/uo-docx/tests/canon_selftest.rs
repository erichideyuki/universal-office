//! The canonical digest's self-test (A6): the Rust `Canon` of a hand-built model equals the TS
//! walker (`js/canon.mjs`) over the equivalent TS document (`js/canon-selftest.mjs`, digests in
//! `tests/canon/selftest.expected`). `UO_CANON_TRACE=<dir>` writes the Rust traces next to the
//! ones `node js/canon-selftest.mjs --trace <dir>` writes, to find the first diverging item.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;

#[test]
fn canon_matches_the_ts_walker() {
    let expected = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/canon/selftest.expected")).unwrap();
    let get = |k: &str| expected.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap().to_string();
    let trace_dir = std::env::var("UO_CANON_TRACE").ok();
    let doc = support::sample_document();
    let sources = support::sample_sources();
    let (d, traces) = support::digests(&doc, &sources, trace_dir.is_some());
    if let Some(dir) = &trace_dir {
        for (name, t) in ["rust-full.trace", "rust-order.trace", "rust-sources.trace"].iter().zip(&traces) {
            std::fs::write(Path::new(dir).join(name), t).unwrap();
        }
    }
    assert_eq!(d[0], get("full"), "full stream");
    assert_eq!(d[2], get("sources"), "sources facet");
    assert_eq!(d[1], get("order"), "key-order facet");
}

#[test]
fn canon_is_deterministic_and_sensitive() {
    let doc = support::sample_document();
    let sources = support::sample_sources();
    let (a, _) = support::digests(&doc, &sources, false);
    let (b, _) = support::digests(&doc.clone(), &sources, false);
    assert_eq!(a, b);
    // one explicit undefined less changes the stream
    let mut doc2 = support::sample_document();
    let mut c = (*doc2.comments).to_vec();
    c[0].cleared = Default::default();
    doc2.comments = c.into();
    let (e, _) = support::digests(&doc2, &sources, false);
    assert_ne!(a[0], e[0]);
}
