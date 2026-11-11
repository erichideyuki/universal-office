//! Parity of D2b-F8's pure model files (record `core/docs/modules/uo-docx-d2b/F8.md`): replays
//! `tests/parity/d2b-f8-ops.capture`, written by `js/capture-d2b-f8-ops.mjs` from the frozen
//! editor's `paste-html.ts` (cssColor, cssSizeHalfPt, cssFamily, fragmentOf, wordListFormats,
//! wordListOf, looksOrdered) and `numbering-store.ts` `importAbstract` over corpus numbering parts.
//!
//! Accepted by name: F8-D9 (a family name with a lone surrogate: attribute and CSS strings of the
//! paste tree are UTF-8, so the surrogate is U+FFFD here).
//!
//! Recapture: `UO_TS_ROOT=<frozen built checkout> node --experimental-transform-types --no-warnings
//! core/crates/uo-docx/js/capture-d2b-f8-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2b_f8_support::{hx, sha, units};
use uo_docx::session::env::FixedEnv;
use uo_docx::session::ops::clipboard::html;
use uo_docx::session::ops::clipboard::testing;
use uo_docx::session::versions::VersionStore;
use uo_jsnum::format::to_js_string;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, repo_root};

fn val_s(v: Option<String>) -> String {
    match v {
        None => "u".to_owned(),
        Some(s) => format!("s{}", hx(&s.encode_utf16().collect::<Vec<_>>())),
    }
}

fn has_lone_surrogate(u: &[u16]) -> bool {
    String::from_utf16(u).is_err()
}

fn abstracts(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(p) = xml[at..].find("<w:abstractNum") {
        let s = at + p;
        let after = xml.as_bytes().get(s + "<w:abstractNum".len()).copied().unwrap_or(b' ');
        if after.is_ascii_alphanumeric() || after == b'_' {
            at = s + 1;
            continue;
        }
        match xml[s..].find("</w:abstractNum>") {
            Some(e) => {
                out.push(xml[s..s + e + "</w:abstractNum>".len()].to_owned());
                at = s + e + 1;
            }
            None => break,
        }
    }
    out
}

#[test]
fn d2b_f8_ops_match_the_frozen_ts() {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let cap = Capture::read(root.join("core/crates/uo-docx/tests/parity/d2b-f8-ops.capture")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut accepted = 0usize;
    for j in 0.. {
        let key = format!("css#{j}");
        let Some(input) = cap.get(&key, "in") else { break };
        replay.check(&key, "in", input);
        let parts: Vec<Vec<u16>> = input.split('|').map(units).collect();
        let s = |u: &[u16]| String::from_utf16_lossy(u);
        replay.check(&key, "color", &val_s(html::css_color(&s(&parts[0]))));
        let size = match html::css_size_half_pt(&s(&parts[1])) {
            None => "u".to_owned(),
            Some(n) => format!("n{}", to_js_string(n)),
        };
        replay.check(&key, "size", &size);
        let fam = val_s(html::css_family(&s(&parts[2])));
        // the TS answer with its lone surrogates as U+FFFD must be the Rust answer
        let fffd = |v: &str| match v.strip_prefix('s') {
            Some(h) => format!("s{}", hx(&String::from_utf16_lossy(&units(h)).encode_utf16().collect::<Vec<_>>())),
            None => v.to_owned(),
        };
        if cap.get(&key, "family") != Some(fam.as_str())
            && has_lone_surrogate(&parts[2])
            && cap.get(&key, "family").map(fffd).as_deref() == Some(fam.as_str())
        {
            replay.accept(&key, "family", &fam, "F8-D9");
            accepted += 1;
        } else {
            replay.check(&key, "family", &fam);
        }
    }
    for j in 0.. {
        let key = format!("frag#{j}");
        let Some(input) = cap.get(&key, "in") else { break };
        replay.check(&key, "in", input);
        replay.check(&key, "out", &hx(html::fragment_of(&units(input))));
    }
    for j in 0.. {
        let key = format!("wl#{j}");
        let Some(input) = cap.get(&key, "in") else { break };
        replay.check(&key, "in", input);
        let parts: Vec<Vec<u16>> = input.split('|').map(units).collect();
        let fmts = html::word_list_formats(&parts[0])
            .iter()
            .map(|(k, b)| format!("{}={}", String::from_utf16_lossy(k), u8::from(*b)))
            .collect::<Vec<_>>()
            .join(" ");
        replay.check(&key, "fmts", if fmts.is_empty() { "-" } else { &fmts });
        let style: Vec<u16> = if j % 7 == 0 { Vec::new() } else { parts[1].clone() };
        let of = match html::word_list_of(&style) {
            Some((a, b, c)) => format!("{},{},{}", to_js_string(a), to_js_string(b), to_js_string(c)),
            None => "-".to_owned(),
        };
        replay.check(&key, "of", &of);
        replay.check(&key, "ord", if html::looks_ordered(&parts[2]) { "1" } else { "0" });
    }
    // importAbstract: the documents in capture order
    let mut docs: Vec<(String, Option<String>)> = Vec::new();
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#ia0")) {
        let rel = rec.key.trim_end_matches("#ia0").to_owned();
        if docs.iter().any(|(r, _)| *r == rel) {
            continue;
        }
        let bytes = std::fs::read(root.join(&rel)).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let xml = pkg.part_text("word/numbering.xml").unwrap().map(|t| t.to_string());
        docs.push((rel, xml));
    }
    let mut n = 0usize;
    for (rel, xml) in &docs {
        let mut st = VersionStore::new(xml.as_deref().map(Arc::from));
        let mut env = FixedEnv::default();
        for j in 0..4 {
            let key = format!("{rel}#ia{j}");
            let Some(raw) = cap.get(&key, "raw") else { break };
            replay.skip(&key, "doc");
            replay.check(&key, "raw", raw);
            let (src_rel, ai) = raw.rsplit_once('|').unwrap();
            let src_bytes = std::fs::read(root.join(src_rel)).unwrap();
            let src_pkg = Package::open(&src_bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
            let src_xml = src_pkg.part_text("word/numbering.xml").unwrap().unwrap().to_string();
            let abs = abstracts(&src_xml);
            let raw_abs = &abs[ai.parse::<usize>().unwrap()];
            let id = testing::import_abstract(&mut st, &mut env, raw_abs);
            replay.check(&key, "id", &to_js_string(id));
            let x = st.xml().map(|x| x.to_string()).unwrap_or_default();
            replay.check(&key, "xml", &sha(&hx(&x.encode_utf16().collect::<Vec<_>>())));
            n += 1;
        }
    }
    let report = replay.finish();
    println!("d2b-f8-ops: {n} imports, {accepted} accepted (F8-D9); {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
}
