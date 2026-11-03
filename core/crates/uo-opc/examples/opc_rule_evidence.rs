//! Evidence for the result-changing proposals of uo-opc stage 5, decided at gate 3 (R1, R2, R3,
//! R5): over the parity corpus (every git-tracked OOXML package, as `capture_opc.mjs` reads
//! them, and the generated quirk corpus), each app's 0.9.x relationship rule and lookup (the TS,
//! reproduced by `tests/parity_support::legacy`, which the parity replay checks against the
//! capture) against the one resolution path of gate 2 D6 (`Package::resolve_target`: the OPC
//! rule of the C# `OpcPartName` and ASCII case folding), now the rule of the three apps; the
//! Sheet lookup with Unicode against ASCII folding; and the depth of every XML part against
//! `Limits::max_xml_depth`, which `Package::xml_part` applies to Sheet and Slides too.
//!
//!   cargo run --release -p uo-opc --example opc_rule_evidence [-- --all]
//!
//! Prints every difference on real files (with `--all`, the quirk corpus too) and the counts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../tests/parity_support/mod.rs"]
mod parity_support;
#[path = "../tests/xml_support/mod.rs"]
mod xml_support;

use std::collections::BTreeMap;
use std::process::Command;

use parity_support::{App, is_rels_part, legacy, load, load_as_fflate_d1, quirks, rels_owner};
use uo_opc::Limits;
use uo_opc::limits::is_xml_like;
use uo_opc::package::Package;
use uo_opc::part_name;
use uo_opc::xml::{Element, parse};
use uo_testkit::repo_root;
use xml_support::preset;

/// The part found for one relationship by the 0.9.x rule and by the D6 path.
fn outcome(app: App, pkg: &Package, owner: &str, target: &str) -> (Option<String>, Option<String>) {
    (legacy::resolve(app, pkg, owner, target).found, app.resolve(pkg, owner, target).found)
}

/// The shape of a target that explains a difference.
fn shape(app: App, owner: &str, target: &str) -> &'static str {
    let by_app = legacy::resolve_name(app, owner, target);
    let by_rule = part_name::resolve_relationship_target(owner, target);
    if by_rule.as_deref().is_some_and(|r| r == by_app) {
        "case (exact lookup)"
    } else if by_rule.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(&by_app)) {
        "case (exact lookup), same resolution ignoring case"
    } else if target.contains('#') {
        "fragment"
    } else if target.contains('%') {
        "percent-encoded"
    } else if target.starts_with("./") || target.contains("/./") {
        "dot segment"
    } else if target.contains("../") {
        if app == App::Write { "parent segment (Write joins word/ without resolving it)" } else { "parent segment" }
    } else if target.contains('\\') {
        "backslash"
    } else {
        "other"
    }
}

fn max_depth(root: Element<'_>) -> usize {
    let mut max = 0;
    let mut stack = vec![(root, 1usize)];
    while let Some((e, d)) = stack.pop() {
        max = max.max(d);
        stack.extend(e.child_elements().map(|c| (c, d + 1)));
    }
    max
}

#[derive(Default)]
struct Totals {
    packages: u64,
    rels: u64,
    differs: BTreeMap<String, u64>,
    /// relationships whose resolved name or lookup differs (what the replay accepts per app)
    rule_items: BTreeMap<&'static str, u64>,
    unicode_vs_ascii: u64,
    xml_parts: u64,
    deepest: usize,
    deeper_than_limit: u64,
}

fn package(label: &str, app: App, bytes: &[u8], t: &mut Totals, show: bool) {
    // the packages the replay compares: those D1 stops at load as fflate holds them
    let Some(pkg) = load(app, bytes).ok().or_else(|| load_as_fflate_d1(app, bytes)) else { return };
    t.packages += 1;
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    for p in &names {
        if is_xml_like(p) {
            // the app's preset without a depth limit (Write's has docx-core's 1000)
            let opts = uo_opc::xml::ParseOptions { max_depth: None, ..preset(app.presets(p)[0]) };
            if let Ok(doc) = parse(pkg.part_text(p).unwrap().unwrap().as_ref(), &opts) {
                t.xml_parts += 1;
                let d = max_depth(doc.root());
                t.deepest = t.deepest.max(d);
                if d > Limits::DEFAULT.max_xml_depth as usize {
                    t.deeper_than_limit += 1;
                    if show {
                        println!("  {label} [{}] {p}: depth {d} > {}", app.id(), Limits::DEFAULT.max_xml_depth);
                    }
                }
            }
        }
        if !is_rels_part(p) || !app.rels_in_scope(p) {
            continue;
        }
        let Ok(doc) = parse(pkg.part_text(p).unwrap().unwrap().as_ref(), &preset(app.rels_preset())) else { continue };
        let owner = rels_owner(p);
        for rel in doc.root().child_elements().filter(|e| e.name() == "Relationship") {
            let (id, target) = (rel.attr("Id"), rel.attr("Target"));
            if !app.selected(id, target, rel.attr("TargetMode") == Some("External")) {
                continue;
            }
            let target = target.unwrap_or("");
            t.rels += 1;
            if legacy::resolve(app, &pkg, &owner, target) != app.resolve(&pkg, &owner, target) {
                *t.rule_items.entry(app.id()).or_default() += 1;
            }
            let (by_app, by_d6) = outcome(app, &pkg, &owner, target);
            if app == App::Sheet {
                let name = legacy::resolve_name(app, &owner, target);
                let unicode = legacy::lookup(app, &pkg, &name);
                let ascii = pkg.find_name(&name);
                if unicode.as_deref() != ascii {
                    t.unicode_vs_ascii += 1;
                    if show {
                        println!("  {label} [s] {p} {target:?}: Unicode lookup {unicode:?}, ASCII lookup {ascii:?}");
                    }
                }
            }
            if by_app == by_d6 {
                continue;
            }
            let kind = match (&by_app, &by_d6) {
                (Some(_), None) => "found → missing",
                (None, Some(_)) => "missing → found",
                _ => "another part",
            };
            *t.differs.entry(format!("{} {kind}: {}", app.id(), shape(app, &owner, target))).or_default() += 1;
            if show {
                println!("  {label} [{}] {p} Target={target:?}: app {by_app:?}, D6 {by_d6:?}", app.id());
            }
        }
    }
}

fn report(name: &str, t: &Totals) {
    println!(
        "{name}: {} packages, {} relationships kept by the app rule, {} XML parts (deepest {}, {} deeper than {})",
        t.packages,
        t.rels,
        t.xml_parts,
        t.deepest,
        t.deeper_than_limit,
        Limits::DEFAULT.max_xml_depth
    );
    println!("  Sheet lookups where Unicode and ASCII folding disagree: {}", t.unicode_vs_ascii);
    for (app, n) in &t.rule_items {
        println!("  {app}: relationships whose resolved name or part differs (accepted by the replay): {n}");
    }
    for (k, n) in &t.differs {
        println!("  {k}: {n}");
    }
}

fn main() {
    let all = std::env::args().any(|a| a == "--all");
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("git").arg("-C").arg(&root).args(["ls-files", "-z"]).output().unwrap();
    let mut files: Vec<String> = String::from_utf8(out.stdout).unwrap().split('\0').filter(|s| !s.is_empty()).map(str::to_owned).collect();
    files.sort();
    let mut real = Totals::default();
    println!("real files (each with its app):");
    for rel in &files {
        let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let Some(app) = App::for_extension(&ext) else { continue };
        package(rel, app, &std::fs::read(root.join(rel)).unwrap(), &mut real, true);
    }
    let mut quirk = Totals::default();
    if all {
        println!("quirk corpus (each archive with the three apps):");
    }
    for (label, bytes) in quirks::quirk_corpus() {
        for app in App::ALL {
            package(&format!("quirk/{label}"), app, &bytes, &mut quirk, all);
        }
    }
    report("real files", &real);
    report("quirk corpus", &quirk);
}
