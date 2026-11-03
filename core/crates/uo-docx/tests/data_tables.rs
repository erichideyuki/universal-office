//! The generated data tables (`src/data`, `tools/gen-data.mjs`): the TS sources they came from
//! are the ones in the repository (SHA-256 in each file's header), and, opt-in
//! (`UO_TS_PARITY=1`, `UO_TS_ROOT`), the generator reproduces them byte for byte.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::data::{style_names_es, table_styles_builtin, word_styles};

#[test]
fn data_sources_unchanged() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for f in ["word_styles.rs", "style_names_es.rs", "table_styles_builtin.rs"] {
        let text = std::fs::read_to_string(crate_dir.join("src/data").join(f)).unwrap();
        for line in text.lines().filter_map(|l| l.strip_prefix("// source: ")) {
            let (path, hash) = line.split_once(" sha256 ").unwrap();
            let bytes = std::fs::read(repo.join(path)).unwrap();
            let got = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(&bytes));
            assert_eq!(got, hash, "{path} changed since src/data/{f} was generated: run tools/gen-data.mjs");
            checked += 1;
        }
    }
    assert_eq!(checked, 3);
}

#[test]
fn data_regenerates_identically() {
    if !uo_testkit::opt_in("UO_TS_PARITY", "uo-docx data tables regeneration (node tools/gen-data.mjs --check)") {
        return;
    }
    let status = std::process::Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/gen-data.mjs"))
        .arg("--check")
        .status()
        .unwrap();
    assert!(status.success(), "src/data is stale against UO_TS_ROOT");
}

#[test]
fn tables_have_their_shapes() {
    assert!(word_styles::WORD_BLANK_STYLES.starts_with("<?xml"));
    assert!(word_styles::WORD_BLANK_SECTPR.starts_with("<w:sectPr>"));
    assert!(word_styles::WORD_BUILTIN_STYLES.iter().any(|s| s.0 == "heading 1"));
    assert!(style_names_es::BUILTIN_STYLE_NAMES_ES.iter().any(|(k, _)| *k == "normal"));
    let t = table_styles_builtin::BUILTIN_TABLE_STYLES;
    assert_eq!(t.first().map(|s| s.name), Some("Normal Table"));
    assert!(t.iter().any(|s| s.accents.is_some()));
}
