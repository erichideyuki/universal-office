//! Stage-4 parity of the container layer: replays `tests/parity/opc.cap`, the capture of the
//! frozen TS (0.9.x) written by `tests/parity_support/capture_opc.mjs`, and compares exactly.
//!
//! Corpus: every git-tracked OOXML package of the repository (the 2,742 oracle documents of
//! docx-layout, sheet/samples, slides/samples and the other fixtures), every legacy .doc/.ppt,
//! the CFB parts embedded in the packages, and the generated quirk corpus
//! (`parity_support/quirks.rs`). Per input and app (Write `w`, Sheet `s`, Slides `p`, each with
//! its own policy, see `parity_support/mod.rs`):
//! - `in`: the input bytes (a changed fixture or generator means "recapture", not a mismatch);
//! - `load`: ordered part names and part bytes, or the error class (eager, as the TS load);
//! - `xml`: every XML-like part parsed with the app's presets, canonical trees with positions
//!   (UTF-8 offsets) where Write uses them;
//! - `rels`: every relationship in the app's scope, the app's target rule and lookup outcome;
//! - `save`: the package saved, listed by an independent central directory reader and read back
//!   by uo-opc (names, order, UTF-8 flag, method, decompressed bytes) — the TS side saved with
//!   `zipSync` and read back with fflate;
//! - `cfb.doc/.xls/.ppt`: the three TS CFB readers (entries, `has`, every stream).
//!
//! Accepted differences, by decision (listed by name, counted in the output):
//! - G4: the Rust package copies untouched entries raw on save (since D5 only the verified ones;
//!   a malformed entry is written again, keeping a stored entry stored), so a part stored in the
//!   source stays stored where `zipSync` deflates everything; the save is compared on names,
//!   flags and bytes, and the fully rewritten save (every part set again) on the method too;
//! - G5: sheet-core hangs on some unclosed constructs (the capture lists them, `g5.*`); uo-opc
//!   reports an unclosed-construct error there;
//! - D1: a deflated entry with an empty stream and a declared size is `Corrupt` in uo-opc where
//!   fflate returns that many zeros, so the eager load fails; accepted when replacing exactly
//!   those entries by fflate's zeros gives the captured package, and the other facets are then
//!   compared on that package;
//! - D4: after an entry named `__proto__` fflate drops canonical numeric names that are not
//!   indices of its bytes; uo-opc keeps them (accepted when that drop is the whole difference).
//!
//! D5 (an entry is copied raw on save only when verified) changes no facet: a rewritten entry
//! holds the bytes the reader produced, which is what the save facets compare.
//!
//! Recapture (needs a built checkout of main and node ≥ 22):
//! `UO_TS_ROOT=<checkout> node core/crates/uo-opc/tests/parity_support/capture_opc.mjs`.
//! Trace one input: `UO_PARITY_TRACE=<key> cargo test -p uo-opc --test opc_parity -- --nocapture`
//! and compare with `capture_opc.mjs --only <key> --trace <key>`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod parity_support;
mod xml_support;

use std::collections::HashMap;
use std::path::Path;

use parity_support::{
    App, G5, Tally, cfb_value, edit, input_digest, js_proto_dropped, load, load_as_fflate_d1, load_value, quirks, rels_value, save_raw,
    save_rewritten, save_value, xml_value,
};
use uo_opc::cfb::is_cfb;
use uo_testkit::{Capture, Replay, repo_root};

const G4: &str = "G4: raw copy of untouched entries keeps the source method";
const D1: &str = "D1: an empty deflate stream with a declared size is Corrupt (fflate: that many zeros)";
const D4: &str = "D4: after a `__proto__` entry fflate drops canonical numeric names that are not indices of its bytes; uo-opc keeps them";

fn capture() -> Capture {
    Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/opc.cap")).unwrap()
}

#[derive(Default)]
struct Counts {
    packages: u64,
    parts: u64,
    saves: u64,
    cfb: u64,
    g4_saves: u64,
    d1: u64,
    d4: u64,
    edits: u64,
}

fn container(key: &str) -> &str {
    key.split_once('!').map_or(key, |(c, _)| c)
}

fn bucket(key: &str, n: u32) -> u32 {
    container(key).bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % n
}

fn replay(name: &str, keep: impl Fn(&str) -> bool) {
    let full = capture();
    let cap = full.subset(|k| keep(k));
    assert!(!cap.is_empty(), "{name}: nothing to replay");
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let quirk_inputs: HashMap<String, Vec<u8>> = if cap.records().iter().any(|r| r.key.starts_with("quirk/")) {
        quirks::quirk_corpus().into_iter().map(|(l, b)| (format!("quirk/{l}"), b)).collect()
    } else {
        HashMap::new()
    };
    let trace_key = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    let mut tally = Tally::default();
    let mut counts = Counts::default();
    let mut embedded: HashMap<String, Vec<u8>> = HashMap::new();

    for record in cap.records() {
        let key = record.key.as_str();
        let traced = trace_key.as_deref() == Some(key);
        let show = |facet: &str, st: &uo_testkit::Stream| {
            if traced {
                println!("--- {key} {facet}\n{}", st.trace());
            }
        };
        let bytes: Vec<u8> = if let Some(b) = quirk_inputs.get(key) {
            b.clone()
        } else if let Some(b) = embedded.remove(key) {
            b
        } else {
            match std::fs::read(root.join(key)) {
                Ok(b) => b,
                Err(e) => panic!("{key}: {e}"),
            }
        };
        if !replay.check(key, "in", &input_digest(&bytes)) {
            // a different input: the rest of the record is meaningless until it is recaptured
            for (f, _) in &record.facets {
                replay.skip(key, f);
            }
            continue;
        }
        let apps: Vec<App> = record.facets.iter().filter_map(|(f, _)| f.strip_prefix("load.").and_then(App::from_id)).collect();
        for (i, &app) in apps.iter().enumerate() {
            let id = app.id();
            let lf = format!("load.{id}");
            let (mut pkg, d1_class) = match load(app, &bytes) {
                Ok(p) => (p, None),
                Err(class) => match load_as_fflate_d1(app, &bytes) {
                    Some(p) if cap.get(key, &lf).is_some_and(|v| v.starts_with("ok")) => (p, Some(class)),
                    _ => {
                        replay.check(key, &lf, &class);
                        continue;
                    }
                },
            };
            counts.packages += 1;
            counts.parts += pkg.len() as u64;
            let (v, st) = load_value(&pkg, traced);
            show(&lf, &st);
            let dropped = js_proto_dropped(&bytes);
            if let Some(class) = d1_class {
                if cap.get(key, &lf) == Some(v.as_str()) {
                    counts.d1 += 1;
                    replay.accept(key, &lf, &class, D1);
                } else {
                    replay.check(key, &lf, &class);
                }
            } else if cap.get(key, &lf) != Some(v.as_str()) && !dropped.is_empty() {
                // the rest of the record is compared on the package as fflate's object holds it
                for d in &dropped {
                    pkg.remove_part(d);
                }
                let (emulated, _) = load_value(&pkg, false);
                if cap.get(key, &lf) == Some(emulated.as_str()) {
                    counts.d4 += 1;
                    replay.accept(key, &lf, &v, D4);
                } else {
                    replay.check(key, &lf, &v);
                }
            } else {
                replay.check(key, &lf, &v);
            }
            let g5 = G5::parse(record.get(&format!("g5.{id}")));
            if record.get(&format!("g5.{id}")).is_some() {
                replay.skip(key, &format!("g5.{id}"));
            }
            let (v, st) = xml_value(app, &pkg, &g5, &mut tally, traced);
            show(&format!("xml.{id}"), &st);
            replay.check(key, &format!("xml.{id}"), &v);
            let (v, st) = rels_value(app, &pkg, &g5, &mut tally, traced);
            show(&format!("rels.{id}"), &st);
            replay.check(key, &format!("rels.{id}"), &v);

            let mut saves = |pkg: &uo_opc::Package, facet: &str, replay: &mut Replay<'_>| {
                let (rewritten, st) = save_value(&save_rewritten(pkg), traced);
                show(facet, &st);
                replay.check(key, facet, &rewritten);
                let (raw, _) = save_value(&save_raw(pkg), false);
                let expected = cap.get(key, facet).unwrap_or("");
                let no_method = |v: &str| v.rsplit(':').next().map(str::to_owned);
                let count = |v: &str| v.split(':').next().map(str::to_owned);
                if raw != expected && no_method(&raw) == no_method(expected) && count(&raw) == count(expected) {
                    counts.g4_saves += 1;
                    replay.accept(key, facet, &raw, G4);
                } else {
                    replay.check(key, facet, &raw);
                }
                counts.saves += 2;
            };
            saves(&pkg, &format!("save.{id}"), &mut replay);
            let ef = format!("edit.{id}");
            if edit(app, &mut pkg) {
                saves(&pkg, &ef, &mut replay);
                counts.edits += 1;
            } else {
                replay.check(key, &ef, "skip");
            }

            if i == 0 {
                for n in pkg.part_names() {
                    let b = pkg.part(n).unwrap().unwrap();
                    if is_cfb(b) {
                        embedded.insert(format!("{key}!{n}"), b.to_vec());
                    }
                }
            }
        }
        if record.get("cfb.doc").is_some() {
            counts.cfb += 1;
            let (v, st) = cfb_value(&bytes, traced);
            show("cfb", &st);
            for copy in ["doc", "xls", "ppt"] {
                replay.check(key, &format!("cfb.{copy}"), &v);
            }
        }
    }

    let report = replay.finish();
    println!(
        "{name}: {} inputs; {} packages loaded, {} parts, {} XML trees, {} parse errors on both sides (G5: {} sheet-core hangs \
         and {} garbage trees where uo-opc reports an unclosed construct), {} relationship parts, {} relationships, \
         {} saves read back ({} raw saves with G4 methods, {} edited packages), {} CFB files, {} packages with the D1 difference, \
         {} with the D4 difference",
        cap.len(),
        counts.packages,
        counts.parts,
        tally.trees,
        tally.xml_errors,
        tally.g5_hangs,
        tally.g5_garbage,
        tally.rels_parts,
        tally.rels,
        counts.saves,
        counts.g4_saves,
        counts.edits,
        counts.cfb,
        counts.d1,
        counts.d4
    );
    println!("{name}: {}", report.summary());
    println!(
        "{name}: {} relationships kept by the app rule have another outcome under the C# rule (stage-5 evidence, not a parity target)",
        tally.csharp_rule_differs
    );
    for e in &tally.csharp_examples {
        println!("  {e}");
    }
    report.assert_clean();
}

#[test]
fn oracle_docx_0() {
    replay("oracle docx (1/4)", |k| k.starts_with("write/packages/docx-layout/test/oracle/") && bucket(k, 4) == 0);
}

#[test]
fn oracle_docx_1() {
    replay("oracle docx (2/4)", |k| k.starts_with("write/packages/docx-layout/test/oracle/") && bucket(k, 4) == 1);
}

#[test]
fn oracle_docx_2() {
    replay("oracle docx (3/4)", |k| k.starts_with("write/packages/docx-layout/test/oracle/") && bucket(k, 4) == 2);
}

#[test]
fn oracle_docx_3() {
    replay("oracle docx (4/4)", |k| k.starts_with("write/packages/docx-layout/test/oracle/") && bucket(k, 4) == 3);
}

#[test]
fn sheet_samples() {
    replay("sheet samples", |k| k.starts_with("sheet/"));
}

#[test]
fn slides_samples() {
    replay("slides samples and legacy files", |k| k.starts_with("slides/"));
}

#[test]
fn other_fixtures() {
    replay("other fixtures", |k| {
        !k.starts_with("write/packages/docx-layout/test/oracle/")
            && !k.starts_with("sheet/")
            && !k.starts_with("slides/")
            && !k.starts_with("quirk/")
    });
}

#[test]
fn quirk_corpus() {
    replay("quirk corpus", |k| k.starts_with("quirk/"));
}

#[test]
fn every_record_is_replayed_by_one_test() {
    let cap = capture();
    assert!(cap.len() > 3000, "capture too small: {}", cap.len());
    for r in cap.records() {
        let k = r.key.as_str();
        let n = [
            k.starts_with("write/packages/docx-layout/test/oracle/"),
            k.starts_with("sheet/"),
            k.starts_with("slides/"),
            k.starts_with("quirk/"),
        ]
        .iter()
        .filter(|&&b| b)
        .count();
        assert!(n <= 1, "{k}");
    }
}

/// Saves read back across engines (opt-in: `UO_TS_PARITY=1 UO_TS_ROOT=<checkout of main>`,
/// plus `UO_DOTNET_PARITY=1` for .NET): the Rust saves (raw and rewritten) read by fflate, the
/// TS saves (`zipSync`) read by uo-opc, and all of them by .NET `ZipArchive` (names and bytes).
#[test]
#[allow(clippy::needless_range_loop)] // the index names the files of the node and .NET runs
fn save_cross_readers() {
    use std::process::Command;
    use uo_testkit::env_flag;
    if !env_flag("UO_TS_PARITY") {
        eprintln!("skipped: set UO_TS_PARITY=1 and UO_TS_ROOT to read the saves back with fflate (and UO_DOTNET_PARITY=1 for .NET)");
        return;
    }
    let cap = capture();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let quirk_inputs: HashMap<String, Vec<u8>> = quirks::quirk_corpus().into_iter().map(|(l, b)| (format!("quirk/{l}"), b)).collect();
    let dir = std::env::temp_dir().join(format!("uo-opc-save-cross-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut lines = String::new();
    let mut n = 0usize;
    let mut keys: Vec<String> = Vec::new();
    for record in cap.records() {
        let key = record.key.as_str();
        let (bytes, path) = match quirk_inputs.get(key) {
            Some(b) => {
                let p = dir.join(format!("{n}.input"));
                std::fs::write(&p, b).unwrap();
                (b.clone(), p)
            }
            None if key.contains('!') => continue,
            None => (std::fs::read(root.join(key)).unwrap(), root.join(key)),
        };
        for (facet, value) in &record.facets {
            let Some(app) = facet.strip_prefix("load.").and_then(App::from_id) else { continue };
            // the packages D1 stops at load are saved as the replay compares them (fflate's zeros)
            let Some(pkg) = load(app, &bytes).ok().or_else(|| load_as_fflate_d1(app, &bytes)).filter(|_| value.starts_with("ok")) else {
                continue;
            };
            std::fs::write(dir.join(format!("{n}.raw.zip")), save_raw(&pkg)).unwrap();
            std::fs::write(dir.join(format!("{n}.rw.zip")), save_rewritten(&pkg)).unwrap();
            lines.push_str(&format!("{n}\t{}\t{}\n", app.id(), path.display()));
            keys.push(format!("{key} ({})", app.id()));
            n += 1;
        }
    }
    std::fs::write(dir.join("inputs.tsv"), &lines).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity_support/save_cross.mjs");
    let out = Command::new("node").arg(&script).arg(&dir).output().unwrap();
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    let fflate: HashMap<String, String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    let mut mismatches = Vec::new();
    let mut checked = 0;
    for i in 0..n {
        for kind in ["raw", "rw", "ts"] {
            let file = format!("{i}.{kind}.zip");
            let rust = save_value(&std::fs::read(dir.join(&file)).unwrap(), false).0;
            checked += 1;
            if fflate.get(&file) != Some(&rust) {
                mismatches.push(format!("{file} {}: fflate {:?}, uo-opc {rust}", keys[i], fflate.get(&file)));
            }
        }
    }
    println!("{checked} saved archives read back by fflate and uo-opc ({n} packages: Rust raw, Rust rewritten, TS zipSync)");
    if env_flag("UO_DOTNET_PARITY") {
        let cs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity_support/save_readback.cs");
        let out = Command::new("dotnet").arg("run").arg(&cs).arg("--").arg(&dir).output().unwrap();
        assert!(out.status.success(), "dotnet failed: {}", String::from_utf8_lossy(&out.stderr));
        let dotnet: HashMap<String, String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect();
        // no exception since D5 (gate 2): a malformed source entry is written again, not copied
        // raw, so the raw saves read the same in .NET too (before, 9 did not: quirk/006, 025, 038)
        let mut dn = 0;
        for i in 0..n {
            for kind in ["raw", "rw", "ts"] {
                let file = format!("{i}.{kind}.zip");
                let rust = parity_support::names_bytes_value(&std::fs::read(dir.join(&file)).unwrap());
                dn += 1;
                if dotnet.get(&file) != Some(&rust) {
                    mismatches.push(format!("{file} {}: .NET {:?}, uo-opc {rust}", keys[i], dotnet.get(&file)));
                }
            }
        }
        println!("{dn} saved archives read back by .NET ZipArchive and uo-opc (names and bytes)");
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
