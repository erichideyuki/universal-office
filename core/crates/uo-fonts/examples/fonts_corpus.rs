//! Writes the font corpus of the stage-4 parity capture to a folder: `manifest.tsv` and one
//! `NNNN.bin` per generated entry (generated fonts, mutants, collections), for
//! `tests/parity_support/capture_fonts.mjs`.
//!
//!   cargo run --release -p uo-fonts --example fonts_corpus -- <empty output folder>
//!
//! Manifest columns (tab separated): key, depth, facets (`all` or `ttc`), the `ttc` probes
//! (`|` separated), kind and its arguments: `file <repo path>`, `embedded <docx> <family>
//! <variant>`, `gen <file>`.
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../tests/b_support/mod.rs"]
mod b_support;
#[path = "../tests/fonts_parity_support/mod.rs"]
mod fonts_parity_support;
#[path = "../tests/frontier_d_support/mod.rs"]
mod frontier_d_support;
#[path = "../tests/gpos_support/mod.rs"]
mod gpos_support;

use fonts_parity_support::corpus::{Source, bytes_of, entries};

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: fonts_corpus <output folder>");
        std::process::exit(2);
    };
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest = String::new();
    let all = entries(&root);
    for (i, e) in all.iter().enumerate() {
        let facets = if e.ttc_only { "ttc" } else { "all" };
        let probes = e.probes.join("|");
        let kind = match &e.source {
            Source::File(rel) => format!("file\t{rel}"),
            Source::Embedded { docx, family, variant } => format!("embedded\t{docx}\t{family}\t{variant}"),
            Source::Generated(_) => {
                let file = format!("{i:05}.bin");
                std::fs::write(std::path::Path::new(&dir).join(&file), &*bytes_of(e, &root)).unwrap();
                format!("gen\t{file}")
            }
        };
        manifest.push_str(&format!("{}\t{}\t{facets}\t{probes}\t{kind}\n", e.key, e.depth));
    }
    std::fs::write(std::path::Path::new(&dir).join("manifest.tsv"), manifest).unwrap();
    let pool_dir = std::path::Path::new(&dir).join("pool");
    std::fs::create_dir_all(&pool_dir).unwrap();
    for (name, bytes) in fonts_parity_support::corpus::embed_pool(&root) {
        std::fs::write(pool_dir.join(format!("{name}.bin")), bytes).unwrap();
    }
    println!("{} entries written to {dir}", all.len());
}
