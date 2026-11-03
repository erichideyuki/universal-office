//! Runs the TypeScript runtime's own tests (`tests/ts/runtime.test.ts`) under Node. Node 24 runs the
//! `.ts` files by stripping the types, which is why the generated and runtime TypeScript is erasable-only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::process::Command;

#[test]
fn typescript_runtime_tests_pass_under_node() {
    if common::node_for_typescript().is_none() {
        return;
    }
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ts/runtime.test.ts");
    let out = Command::new("node").arg("--test").arg("--test-reporter=tap").arg(&file).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
    let pass = text.lines().find(|l| l.contains("# pass")).unwrap_or("");
    eprintln!("node --test runtime.test.ts: {pass}");
    assert!(text.contains("# fail 0"), "{text}");
}
