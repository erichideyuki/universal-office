//! Writes the generated font folder of the provider captures (`tests/provider_support`) to a
//! folder, for the Node and C# captures to read:
//!
//!   cargo run --release -p uo-fonts --example provider_corpus -- <empty output folder>
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../tests/b_support/mod.rs"]
mod b_support;
#[path = "../tests/provider_support/mod.rs"]
mod provider_support;

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: provider_corpus <output folder>");
        std::process::exit(2);
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    provider_support::write_corpus(&dir);
    let mut manifest = String::new();
    for (rel, bytes) in provider_support::corpus_files() {
        manifest.push_str(&format!("{rel}\t{}\t{}\n", bytes.len(), provider_support::sha_hex(&bytes)));
    }
    std::fs::write(dir.join("manifest.tsv"), manifest).unwrap();
    println!("corpus written to {}", dir.display());
}
