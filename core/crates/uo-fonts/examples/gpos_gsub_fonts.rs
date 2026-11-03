//! Writes the generated fonts of the kerning and ligature differential check to a directory (one
//! `<name>.ttf` each) for `tests/gpos_support/capture_gpos_gsub.mjs`, which reads them with the
//! frozen TS and writes the expected digests.
//!
//!   cargo run -p uo-fonts --example gpos_gsub_fonts -- <output directory>
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../tests/gpos_support/mod.rs"]
mod gpos_support;

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: gpos_gsub_fonts <output directory>");
        std::process::exit(2);
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut n = 0;
    for case in gpos_support::all_named() {
        std::fs::write(std::path::Path::new(&dir).join(format!("{}.ttf", case.name)), &case.bytes).unwrap();
        n += 1;
    }
    for (name, bytes) in [("big_exact", gpos_support::big_font(4096)), ("big_over", gpos_support::big_font(4097))] {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ttf")), bytes).unwrap();
        n += 1;
    }
    println!("{n} fonts written to {dir}");
}
