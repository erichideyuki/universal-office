//! `uo-schema generate | check | hash`. See the library documentation.
#![forbid(unsafe_code)]
#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use uo_schema::{generate_all, load, normalized};

fn usage() -> ExitCode {
    eprintln!("usage: uo-schema generate [--root DIR] [--manifest FILE]   write every generated file");
    eprintln!("       uo-schema check [--root DIR] [--manifest FILE]      fail if a generated file differs from its schema");
    eprintln!("       uo-schema hash FILE.uos                              print the schema hash");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return usage();
    }
    let command = args.remove(0);
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manifest: Option<PathBuf> = None;
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => match it.next() {
                Some(v) => root = PathBuf::from(v),
                None => return usage(),
            },
            "--manifest" => match it.next() {
                Some(v) => manifest = Some(PathBuf::from(v)),
                None => return usage(),
            },
            _ => rest.push(a),
        }
    }
    let manifest = manifest.unwrap_or_else(|| root.join("schema/manifest.txt"));
    match command.as_str() {
        "hash" => {
            let Some(file) = rest.first() else { return usage() };
            match fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|s| load(&s).map_err(|e| e.to_string())) {
                Ok(schema) => {
                    println!("{}", schema.hash_hex());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{file}: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "generate" | "check" => {
            let files = match generate_all(&root, &manifest) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let mut stale = Vec::new();
            for f in &files {
                let path = root.join(&f.path);
                let current = fs::read_to_string(&path).ok();
                let same = current.as_deref().is_some_and(|c| normalized(c) == f.text);
                if command == "check" {
                    if !same {
                        stale.push(f.path.display().to_string());
                    }
                } else if !same {
                    if let Some(dir) = path.parent()
                        && let Err(e) = fs::create_dir_all(dir)
                    {
                        eprintln!("error: {}: {e}", dir.display());
                        return ExitCode::FAILURE;
                    }
                    if let Err(e) = fs::write(&path, f.text.as_bytes()) {
                        eprintln!("error: {}: {e}", path.display());
                        return ExitCode::FAILURE;
                    }
                    println!("wrote {}", f.path.display());
                }
            }
            if stale.is_empty() {
                if command == "check" {
                    println!("{} generated files are up to date", files.len());
                }
                ExitCode::SUCCESS
            } else {
                eprintln!("generated files out of date (run `cargo run -p uo-schema -- generate` and commit):");
                for s in stale {
                    eprintln!("  {s}");
                }
                ExitCode::FAILURE
            }
        }
        _ => usage(),
    }
}
