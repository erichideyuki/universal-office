//! Writes the generated quirk corpus of the package parity (tests/parity_support/quirks.rs) to a
//! directory, one `<label>.zip` per archive, for the Node capture
//! (`tests/parity_support/capture_opc.mjs` runs this itself).
//!
//!   cargo run -q -p uo-opc --example opc_quirk_corpus -- <out dir>
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "../tests/parity_support/quirks.rs"]
mod quirks;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: opc_quirk_corpus <out dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let corpus = quirks::quirk_corpus();
    for (label, bytes) in &corpus {
        std::fs::write(std::path::Path::new(&dir).join(format!("{label}.zip")), bytes).unwrap();
    }
    println!("{} archives", corpus.len());
}
