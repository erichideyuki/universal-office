//! TS differential of the D1b font-embedding step (`write::fonts`, from uo-fonts G2): the replay of
//! `tests/parity/font-embedding.capture` (the frozen TS `readDocxEmbedding` / `writeDocxEmbedding`,
//! `js/capture-font-embedding.mjs`, Node 24.15 x64) over the corpus docx and the 500 synthetic
//! packages of `tests/parity/font-embedding.inputs`, each with its seeded plan of open fonts and its
//! seeded GUID source. Every part the step sets is compared byte for byte after decompression
//! (an `.odttf` part by its length, its 32 obfuscated bytes and the equality of its tail with the
//! font file; any other part by its digest), plus the part order, the set of parts written, the
//! content types and relationships, and `read_docx_embedding` before and after.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-font-embedding.mjs`.
//! Trace one record: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir>` writes the parts the step set
//! as `rust-<name>` (the TS side: `capture-font-embedding.mjs --dump <key>`, `ts-<name>`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::fonts_support;

use std::collections::HashMap;
use std::sync::Arc;

use fonts_support::{Inputs, PlanSpec, fonts, guid_source, inputs, open_parts, plan_of};
use uo_docx::DocxError;
use uo_docx::write::fonts::{TargetRule, read_docx_embedding_with, write_docx_embedding_with};
use uo_jsnum::json;
use uo_jsnum::text::to_lowercase;
use uo_opc::Package;
use uo_testkit::{Capture, Replay, Stream, repo_root};

fn read_facet(pkg: &Package, rule: TargetRule) -> String {
    let r = read_docx_embedding_with(pkg, rule).unwrap();
    let opt = |v: Option<bool>| v.map_or_else(|| "null".to_owned(), |b| b.to_string());
    let names: Vec<String> = r.embedded.iter().map(|n| json::string(n)).collect();
    format!("[{},{},{},[{}]]", r.settings.embed, opt(r.settings.subset), opt(r.settings.skip_common_fonts), names.join(","))
}

fn json_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<String> = names.map(json::string).collect();
    format!("[{}]", v.join(","))
}

fn digest_text(s: &str) -> String {
    let mut st = Stream::new();
    st.str(s);
    st.finish().short()
}

fn part_digest(pkg: &Package, name: &str) -> String {
    match pkg.part(name).unwrap() {
        Some(b) => {
            let mut st = Stream::new();
            st.digest_of(b);
            st.finish().short()
        }
        None => "-".to_owned(),
    }
}

/// The facets of one record, as `js/capture-font-embedding.mjs` computes them.
fn facets(
    mut pkg: Package,
    spec: &PlanSpec,
    fonts: &HashMap<String, Arc<[u8]>>,
    dump: bool,
    rule: TargetRule,
) -> Vec<(&'static str, String)> {
    let mut out = vec![("before", read_facet(&pkg, rule))];
    let plan = plan_of(spec, fonts);
    let mut guid = guid_source(spec.seed);
    match write_docx_embedding_with(&mut pkg, &plan, &spec.settings, &mut guid, rule) {
        Ok(()) => {}
        Err(DocxError::Corrupt(_)) => {
            out.push(("error", "TypeError".to_owned()));
            return out;
        }
        Err(e) => {
            out.push(("error", format!("{e:?}")));
            return out;
        }
    }
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    out.push(("order", digest_text(&json_names(names.iter().map(String::as_str)))));
    let set: Vec<&str> = names.iter().map(String::as_str).filter(|n| pkg.entry(n).is_none()).collect();
    out.push(("set", digest_text(&json_names(set.iter().copied()))));
    if dump {
        let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
        for n in set.iter().filter(|n| !to_lowercase(n).ends_with(".odttf")) {
            let file: String = n.chars().map(|c| if matches!(c, '/' | '[' | ']') { '_' } else { c }).collect();
            std::fs::write(std::path::Path::new(&dir).join(format!("rust-{file}")), pkg.part(n).unwrap().unwrap()).unwrap();
        }
        std::fs::write(
            std::path::Path::new(&dir).join("rust-names.json"),
            format!("{{\"names\": {}, \"set\": {}}}", json_names(names.iter().map(String::as_str)), json_names(set.iter().copied())),
        )
        .unwrap();
    }
    let mut st = Stream::new();
    for n in &set {
        let data = pkg.part(n).unwrap().unwrap();
        st.str(n);
        if to_lowercase(n).ends_with(".odttf") {
            let src = spec.faces.iter().find(|f| {
                let b = &fonts[&f.file];
                b.len() == data.len() && b.get(32..) == data.get(32..)
            });
            st.u64(data.len() as u64).bytes(&data[..data.len().min(32)]).str(src.map_or("?", |f| f.file.as_str()));
            if src.is_none() {
                st.digest_of(data);
            }
        } else {
            st.digest_of(data);
        }
    }
    out.push(("parts", st.finish().short()));
    out.push(("settings", part_digest(&pkg, "word/settings.xml")));
    out.push(("fontTable", part_digest(&pkg, "word/fontTable.xml")));
    out.push(("ftRels", part_digest(&pkg, "word/_rels/fontTable.xml.rels")));
    out.push(("docRels", part_digest(&pkg, "word/_rels/document.xml.rels")));
    out.push(("types", part_digest(&pkg, "[Content_Types].xml")));
    out.push(("after", read_facet(&pkg, rule)));
    out
}

fn replay(which: &str, keep: impl Fn(&str) -> bool) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(std::path::Path::new(manifest).join("tests/parity/font-embedding.capture")).unwrap();
    let cap = cap.subset(|k| keep(k));
    let Inputs { packages, plans } = inputs();
    let fonts = fonts();
    let quirks: HashMap<String, &fonts_support::Parts> = packages.iter().map(|(k, p)| (format!("{k}#fonts"), p)).collect();
    let root = repo_root(manifest);
    let mut replay = Replay::new(&cap);
    let (mut records, mut faces, mut d6) = (0usize, 0usize, 0usize);
    for r in cap.records() {
        let spec = &plans[&r.key];
        let open = || match quirks.get(&r.key) {
            Some(parts) => open_parts(parts),
            None => {
                let file = root.join(r.key.strip_suffix("#fonts").unwrap());
                Package::open(&std::fs::read(file).unwrap(), &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap()
            }
        };
        let dump = std::env::var("UO_PARITY_TRACE").is_ok_and(|t| t == r.key);
        let got = facets(open(), spec, &fonts, dump, TargetRule::Opc);
        let same = |got: &[(&str, String)]| got.len() == r.facets.len() && got.iter().all(|(f, v)| r.get(f) == Some(v.as_str()));
        // a difference is accepted only when the 0.9 target rule gives exactly the captured record
        let legacy = !same(&got) && same(&facets(open(), spec, &fonts, false, TargetRule::Fonts09));
        const D6: &str = "D6: relationship targets resolved by uo-opc (case folding, fragments, %XX), not the 0.9 fonts rule";
        for (facet, value) in &got {
            if legacy && r.get(facet) != Some(value.as_str()) {
                replay.accept(&r.key, facet, value, D6);
            } else {
                replay.check(&r.key, facet, value);
            }
        }
        if legacy {
            // the step failed with D6 (a settings part without `w:settings` found by case folding)
            for (facet, _) in r.facets.iter().filter(|(f, _)| !got.iter().any(|(g, _)| g == f)) {
                replay.accept(&r.key, facet, "(absent: the step failed)", D6);
            }
        }
        d6 += usize::from(legacy);
        records += 1;
        faces += spec.faces.len();
    }
    let report = replay.finish();
    println!("font embedding parity ({which}): {records} records ({d6} differ by D6 only), {faces} plan faces: {}", report.summary());
    report.assert_clean();
}

fn shard_of(key: &str, n: u32) -> u32 {
    key.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % n
}

#[test]
fn font_embedding_matches_the_ts_on_the_synthetic_packages() {
    replay("synthetic", |k| k.starts_with("quirk/"));
}

#[test]
fn font_embedding_matches_the_ts_on_the_corpus_shard_0() {
    replay("corpus 0/3", |k| !k.starts_with("quirk/") && shard_of(k, 3) == 0);
}

#[test]
fn font_embedding_matches_the_ts_on_the_corpus_shard_1() {
    replay("corpus 1/3", |k| !k.starts_with("quirk/") && shard_of(k, 3) == 1);
}

#[test]
fn font_embedding_matches_the_ts_on_the_corpus_shard_2() {
    replay("corpus 2/3", |k| !k.starts_with("quirk/") && shard_of(k, 3) == 2);
}
