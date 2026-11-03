//! The tables and the captures are generated: running the generators again on Node 24.15 must give the committed
//! files byte for byte (the same check CI runs). Node is part of the dev environment; without Node 24.15 the
//! test fails with the reason instead of passing silently (set `UO_TEXT_SKIP_REGEN=1` to skip it knowingly).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

fn node_version() -> Option<String> {
    let out = Command::new("node").arg("--version").output().ok()?;
    out.status.success().then(|| uo_jsnum::text::trim(&String::from_utf8_lossy(&out.stdout)).to_owned())
}

fn run_check(script: &str) {
    if std::env::var_os("UO_TEXT_SKIP_REGEN").is_some() {
        eprintln!("UO_TEXT_SKIP_REGEN set: {script} not checked");
        return;
    }
    let version = node_version().unwrap_or_else(|| panic!("node is not on PATH: install Node 24.15 or set UO_TEXT_SKIP_REGEN=1"));
    assert!(version.starts_with("v24.15."), "the tables are pinned to Node 24.15, found {version}: use it or set UO_TEXT_SKIP_REGEN=1");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("node").arg(root.join("js").join(script)).arg("--check").output().unwrap();
    assert!(
        out.status.success(),
        "{script} --check failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn committed_tables_equal_a_fresh_generation() {
    run_check("gen-tables.mjs");
}

#[test]
fn committed_captures_equal_a_fresh_generation() {
    run_check("capture-oracle.mjs");
}
