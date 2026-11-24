//! Converts a `.xls` with `uo_xlsx::xls::convert` and writes the parts into a folder (or prints their sizes): the tool to diff
//! against `js/dump-xls-parts.mjs` (the frozen TS converter).
//!
//!   cargo run -p uo-xlsx --features xls --example xls_dump -- <file.xls> [<out folder>]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: xls_dump <file.xls> [out folder]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(path).unwrap();
    let t0 = std::time::Instant::now();
    let parts = match uo_xlsx::xls::convert(&bytes) {
        Ok(p) => p,
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    match args.get(1) {
        Some(out) => {
            for (name, data) in &parts.items {
                let p = std::path::Path::new(out).join(name.replace('/', "__"));
                std::fs::create_dir_all(out).unwrap();
                std::fs::write(p, data).unwrap();
            }
            println!("{} parts in {ms:.1} ms -> {out}", parts.items.len());
        }
        None => {
            for (name, data) in &parts.items {
                println!("{name}\t{}", data.len());
            }
            println!("{ms:.1} ms");
        }
    }
}
