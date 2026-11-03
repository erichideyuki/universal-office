//! Writes the corpus of the G16 capture (`tests/parity_support/legacy_kern.cs`) to a folder:
//! `manifest.tsv` (`key<TAB>file`) and one `.bin` per font.
//!
//!   cargo run --release -p uo-fonts --example legacy_kern_corpus -- <empty output folder>
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../tests/b_support/mod.rs"]
mod b_support;
#[path = "../tests/fonts_parity_support/mod.rs"]
mod fonts_parity_support;
#[path = "../tests/frontier_d_support/mod.rs"]
mod frontier_d_support;
#[path = "../tests/gpos_support/mod.rs"]
mod gpos_support;

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: legacy_kern_corpus <output folder>");
        std::process::exit(2);
    };
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest = String::new();
    for (i, (key, make)) in fonts_parity_support::legacy_corpus::legacy_entries(&root).into_iter().enumerate() {
        let file = format!("{i:05}.bin");
        std::fs::write(std::path::Path::new(&dir).join(&file), make()).unwrap();
        manifest.push_str(&format!("{key}\t{file}\n"));
    }
    std::fs::write(std::path::Path::new(&dir).join("manifest.tsv"), manifest).unwrap();
    println!("corpus written to {dir}");
}
