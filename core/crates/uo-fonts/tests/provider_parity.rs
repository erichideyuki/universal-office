//! Parity of the provider policies (K6, LQ8): each policy against the provider it ports, over the
//! scenarios of `tests/parity/provider_scenarios.txt`.
//!
//! - `viewer_policy_matches_the_ts`: `tests/parity/provider_viewer.cap`, the real `ViewerFonts` of
//!   write/packages/viewer over a stub resolver (`parity_support/capture_viewer.mjs`).
//! - `oracle_policy_matches_node_fonts`, `system_provider_and_substitute_match_the_csharp`,
//!   `system_index_matches_the_references`: see below (feature `system`).
//!
//! Set `UO_PROVIDER_TRACE=<file>` to write the readable lines of a replay (the captures take
//! `--trace <file>` and print the same lines) and diff the two.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use provider_support::{answer_line, scenarios, viewer_pool};
use uo_fonts::policy::ViewerProvider;
use uo_fonts::registry::{AdvanceTable, FaceCache, FontBudget, FontProvider, FontRegistry, FontSource, HostFace};
use uo_fonts::{FontLimits, Variant};
use uo_testkit::{Capture, Replay, Stream};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

fn variant(c: &str) -> Variant {
    match c {
        "r" => Variant::Regular,
        "b" => Variant::Bold,
        "i" => Variant::Italic,
        _ => Variant::BoldItalic,
    }
}

fn source(name: &str) -> FontSource {
    match name {
        "host" => FontSource::Host,
        "local-access" => FontSource::LocalAccess,
        "bundled" => FontSource::Bundled,
        "page" => FontSource::Page,
        "installed" => FontSource::Installed,
        "substitute" => FontSource::Substitute,
        _ => FontSource::Default,
    }
}

fn trace_out(lines: &[String]) {
    if let Ok(path) = std::env::var("UO_PROVIDER_TRACE") {
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }
}

#[test]
fn viewer_policy_matches_the_ts() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/provider_viewer.cap")).unwrap();
    let pool = viewer_pool();
    let bytes_of = |name: &str| pool.iter().find(|(n, _)| *n == name).map(|(_, b)| Arc::clone(b)).unwrap();
    let advances = AdvanceTable::parse_json(r#"{"11":{"a":123.5,"b":7},"12":{"a":9}}"#).unwrap();
    let advances = Arc::new(advances);
    let mut replay = Replay::new(&cap);
    let (mut asks, mut answered, mut stand_ins, mut dumps) = (0usize, 0usize, 0usize, 0usize);
    let mut trace: Vec<String> = Vec::new();
    for (id, ops) in scenarios("V") {
        let cache = Arc::new(FaceCache::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes: u64::MAX }, FontLimits::NATIVE));
        let registry = Arc::new(FontRegistry::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes: u64::MAX }, cache));
        for op in ops.iter().filter(|o| o[0] == "a") {
            let mut face = HostFace::new(variant(&op[2]), bytes_of(&op[3]));
            if op[4] == "1" {
                face.advances = Some(Arc::clone(&advances));
            }
            let _ = registry.push_asset(&op[1], face);
        }
        let mut s = Stream::new();
        let mut seen: Vec<u64> = Vec::new();
        let mut provider = ViewerProvider::new(Arc::clone(&registry));
        trace.push(format!("## {id}"));
        for op in &ops {
            match op[0].as_str() {
                "f" => {
                    let faces: Vec<HostFace> = op[4]
                        .split(',')
                        .map(|pair| {
                            let (v, pool) = pair.split_once('=').unwrap();
                            HostFace::new(variant(v), bytes_of(pool))
                        })
                        .collect();
                    registry.push_family(&op[1], source(&op[2]), op[3] == "1", faces).unwrap();
                }
                "c" => provider = ViewerProvider::new(Arc::clone(&registry)),
                "q" => {
                    let answer = provider.face(&op[1], op[2] == "1", op[3] == "1");
                    let mut line = answer_line(answer.as_ref(), &mut seen);
                    if let Some(a) = &answer {
                        let adv = a.advances.as_ref().and_then(|t| t.get("11", u32::from('a')));
                        line = format!("{line}|adv={}", adv.map_or("-".to_owned(), uo_jsnum::format::to_js_string));
                        answered += 1;
                        stand_ins += usize::from(a.stand_in);
                    }
                    asks += 1;
                    s.tag(b'q').str(&line);
                    trace.push(format!("q {} {}{} {line}", op[1], op[2], op[3]));
                }
                "m" => {
                    let dump: Vec<String> = registry
                        .missing()
                        .iter()
                        .map(|m| format!("{}:{}", m.family, m.variants.iter().map(|v| v.name()).collect::<Vec<_>>().join("+")))
                        .collect();
                    let want = registry.take_missing();
                    let loads: Vec<String> =
                        want.iter().flat_map(|m| m.variants.iter().map(|v| format!("{}:{}", m.family, v.name()))).collect();
                    let line = format!("{}|{}|{}", dump.join(","), u8::from(!want.is_empty()), loads.join(";"));
                    dumps += 1;
                    s.tag(b'm').str(&line);
                    trace.push(format!("m {line}"));
                }
                _ => {}
            }
        }
        let items = s.items();
        let value = format!("{items}:{}", s.finish().short());
        replay.check(&format!("viewer/{id}"), "stream", &value);
    }
    trace_out(&trace);
    let report = replay.finish();
    let mut summary = String::new();
    write!(
        summary,
        "viewer policy: {} scenarios, {asks} asks ({answered} answered, {stand_ins} stand-ins), {dumps} missing dumps: {}",
        cap.len(),
        report.summary()
    )
    .unwrap();
    eprintln!("{summary}");
    report.assert_clean();
    assert!(answered > 1000 && stand_ins > 50 && dumps > 600, "the corpus must exercise the policy");
}
