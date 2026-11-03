//! Parity of the native system font index and of the policies over it (feature `system`):
//!
//! - `tests/parity/provider_oracle.cap`: `createNodeFontProvider` of write/tools/corpus-lab/node-fonts.mjs
//!   (`parity_support/capture_system.mjs`) against the `oracle` policy;
//! - `tests/parity/provider_csharp.cap`: the C# `SystemFontIndex`, `SystemFontProvider`,
//!   `SubstituteFontProvider` and `FontCatalog` (`parity_support/provider_capture.cs`, run on .NET)
//!   against the index, `SystemFontProvider`, the `server-substitute` policy and
//!   `csharp_metric_substitute`.
//!
//! The roots are exactly the generated corpus (written to a temp folder from `provider_support`) and
//! `shared/fonts/files`; the paths of the keys are reduced to file names. `UO_PROVIDER_TRACE=<file>`
//! writes the readable lines of the `server` replay to diff with the capture's `--trace` file.
#![cfg(feature = "system")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use provider_support::{TempDir, answer_line, repo_root, scenarios, sha_hex, write_corpus};
use uo_fonts::FontLimits;
use uo_fonts::policy::{OracleProvider, SubstituteFontProvider, SystemFontProvider, csharp_metric_substitute};
use uo_fonts::registry::{FaceCache, FaceSource, FontBudget, FontProvider};
use uo_fonts::system::SystemFontIndex;
use uo_testkit::{Capture, Replay, Stream};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

fn index_over(corpus: &Path) -> Arc<SystemFontIndex> {
    let cache = Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits::NATIVE));
    Arc::new(SystemFontIndex::with_roots(vec![corpus.to_path_buf(), repo_root().join("shared/fonts/files")], cache))
}

fn digest(lines: &[String]) -> String {
    let mut s = Stream::new();
    for l in lines {
        s.tag(b'q').str(l);
    }
    let items = s.items();
    format!("{items}:{}", s.finish().short())
}

fn list_digest(items: &[String]) -> String {
    format!("{}:{}", items.len(), &sha_hex(items.join("\u{1}").as_bytes())[..32])
}

/// One layout call: the answers are held until the end of the scenario, as the layout holds the
/// faces it resolved. (An answer nobody holds may be given a new identity the next time: nobody
/// can compare it with another.)
fn run_scenario(provider: &dyn FontProvider, asks: &[Vec<String>], trace: &mut Vec<String>, label: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut lines = Vec::new();
    let mut held = Vec::new();
    for ask in asks {
        let answer = provider.face(&ask[1], ask[2] == "1", ask[3] == "1");
        let line = answer_line(answer.as_ref(), &mut seen);
        trace.push(format!("{label} {} {}{} {line}", ask[1], ask[2], ask[3]));
        lines.push(line);
        held.push(answer);
    }
    drop(held);
    lines
}

#[test]
fn oracle_policy_matches_node_fonts() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/provider_oracle.cap")).unwrap();
    let dir = TempDir::new("oracle");
    write_corpus(&dir.0);
    let index = index_over(&dir.0);
    let mut replay = Replay::new(&cap);
    let mut families: Vec<String> = index.families();
    families.sort();
    replay.check("oracle/families", "list", &list_digest(&families));
    let (mut asks, mut answered) = (0usize, 0usize);
    for (id, ops) in scenarios("S") {
        let provider = OracleProvider::new(Arc::clone(&index) as Arc<dyn FaceSource>, None, &[]);
        let lines = run_scenario(&provider, &ops, &mut Vec::new(), "oracle");
        asks += lines.len();
        answered += lines.iter().filter(|l| *l != "-").count();
        replay.check(&format!("oracle/{id}"), "stream", &digest(&lines));
    }
    let report = replay.finish();
    eprintln!(
        "oracle policy: {} scenarios, {asks} asks ({answered} answered), {} families: {}",
        cap.len() - 1,
        families.len(),
        report.summary()
    );
    report.assert_clean();
    assert!(answered > 800);
}

#[test]
fn system_provider_and_substitute_match_the_csharp() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/provider_csharp.cap")).unwrap();
    let dir = TempDir::new("csharp");
    write_corpus(&dir.0);
    let index = index_over(&dir.0);
    let mut replay = Replay::new(&cap);

    // the index: every face it read, sorted, and the families
    let mut entries: Vec<String> = index
        .entries()
        .iter()
        .map(|e| {
            format!(
                "{}|{}|{}{}|{}|{}#{}",
                e.family,
                e.typo.as_deref().unwrap_or("-"),
                if e.bold { "b" } else { "" },
                if e.italic { "i" } else { "" },
                e.weight,
                e.path.file_name().unwrap().to_string_lossy(),
                e.index
            )
        })
        .collect();
    entries.sort();
    replay.check("csharp-index/entries", "list", &list_digest(&entries));
    let mut families = index.families();
    families.sort();
    replay.check("csharp-index/families", "list", &list_digest(&families));

    let mut trace = Vec::new();
    let mut asked: BTreeSet<String> = BTreeSet::new();
    let (mut asks, mut stand_ins, mut direct) = (0usize, 0usize, 0usize);
    for (id, ops) in scenarios("S") {
        asked.extend(ops.iter().map(|o| o[1].clone()));
        trace.push(format!("## {id}"));
        let source = Arc::clone(&index) as Arc<dyn FaceSource>;
        let system = SystemFontProvider::new(Arc::clone(&source), None);
        let server = SubstituteFontProvider::new(Arc::new(SystemFontProvider::new(source, None)));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut seen_a = Vec::new();
        let mut seen_b = Vec::new();
        let mut held = Vec::new();
        for ask in &ops {
            let (fam, bold, italic) = (&ask[1], ask[2] == "1", ask[3] == "1");
            let ans = system.face(fam, bold, italic);
            let la = answer_line(ans.as_ref(), &mut seen_a);
            let sub = server.face(fam, bold, italic);
            let lb = answer_line(sub.as_ref(), &mut seen_b);
            held.push((ans.clone(), sub.clone()));
            trace.push(format!("system {fam} {}{} {la}", ask[2], ask[3]));
            trace.push(format!("server {fam} {}{} {lb}", ask[2], ask[3]));
            asks += 1;
            stand_ins += usize::from(sub.as_ref().is_some_and(|s| s.stand_in));
            direct += usize::from(sub.as_ref().is_some_and(|s| !s.stand_in));
            a.push(la);
            b.push(lb);
        }
        replay.check(&format!("csharp-system/{id}"), "stream", &digest(&a));
        replay.check(&format!("csharp-server/{id}"), "stream", &digest(&b));
    }
    // the catalog lookup of the C# (Trim, ToLowerInvariant) over every family asked
    let catalog: Vec<String> = asked.iter().map(|f| format!("{f}=>{}", csharp_metric_substitute(f).unwrap_or("-"))).collect();
    replay.check("csharp-catalog", "list", &list_digest(&catalog));
    if let Ok(path) = std::env::var("UO_PROVIDER_TRACE") {
        std::fs::write(path, trace.join("\n") + "\n").unwrap();
    }
    let report = replay.finish();
    eprintln!(
        "C# providers: {} faces in the index, {asks} asks ({direct} answered by the system, {stand_ins} by a metric substitute), {} catalog lookups: {}",
        entries.len(),
        catalog.len(),
        report.summary()
    );
    report.assert_clean();
    assert!(stand_ins > 50 && direct > 400, "the corpus must exercise both paths");
}
