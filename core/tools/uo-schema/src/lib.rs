//! `uo-schema`: the schema-driven codec generator of the UniversalOffice Rust core (G15).
//!
//! It reads an IDL file (`core/schema/*.uos`, grammar in the record `core/docs/modules/uo-render.md`,
//! "Milestone 1 — frontier C") and emits, deterministically (no clock, no hash-map order, LF):
//!
//! - Rust types with `encode`/`decode` written against `uo-codec`, the table state and the message functions;
//! - a TypeScript encoder/decoder written against `core/crates/uo-codec/js/runtime.ts`;
//! - optionally the test support of both (canonical dump, random messages).
//!
//! The generated files are committed; `uo-schema check` regenerates in memory and fails on any
//! difference (the CI step), `uo-schema generate` rewrites them.
#![forbid(unsafe_code)]
// The workspace ban list (clippy.toml) keeps JavaScript text semantics in the engines; this is a build tool over
// ASCII schema files and manifests, where Rust's trim and split_whitespace are the right ones.
#![allow(clippy::disallowed_methods)]

pub mod model;
mod out;
pub mod parse;
pub mod rust;
pub mod rust_bind;
pub mod rust_support;
mod sha256;
pub mod ts;
pub mod ts_support;

use std::fs;
use std::path::{Component, Path, PathBuf};

pub use model::Schema;
pub use parse::Error;

/// Parses and checks one schema source.
pub fn load(src: &str) -> Result<Schema, Error> {
    model::check(&parse::parse(src)?)
}

/// One line of the manifest: a schema and the files generated from it (paths relative to the root).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub schema: PathBuf,
    pub rust: Option<PathBuf>,
    pub ts: Option<PathBuf>,
    pub rust_support: Option<PathBuf>,
    pub ts_support: Option<PathBuf>,
    /// Where the TypeScript runtime is, relative to the root (default `crates/uo-codec/js/runtime.ts`).
    pub ts_runtime: Option<PathBuf>,
    /// The conversions between the wire types and existing Rust types ([`rust_bind`]): the output file and
    /// the bind map it reads (both relative to the root, both or neither).
    pub rust_bind: Option<PathBuf>,
    pub bind: Option<PathBuf>,
}

pub const DEFAULT_TS_RUNTIME: &str = "crates/uo-codec/js/runtime.ts";

/// The manifest: one target per line, `schema.uos key=path ...` with the keys `rust`, `ts`,
/// `rust_support`, `ts_support`, `ts_runtime`, `rust_bind` and `bind`; `#` starts a comment. Every path is relative to the root
/// (`core/`), the schema's too (`schema/render.uos`).
pub fn parse_manifest(text: &str) -> Result<Vec<Target>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let schema = parts.next().unwrap_or_default();
        let mut t = Target { schema: PathBuf::from(schema), ..Target::default() };
        for kv in parts {
            let Some((k, v)) = kv.split_once('=') else {
                return Err(format!("manifest line {}: expected key=path, found `{kv}`", n + 1));
            };
            let p = Some(PathBuf::from(v));
            match k {
                "rust" => t.rust = p,
                "ts" => t.ts = p,
                "rust_support" => t.rust_support = p,
                "ts_support" => t.ts_support = p,
                "ts_runtime" => t.ts_runtime = p,
                "rust_bind" => t.rust_bind = p,
                "bind" => t.bind = p,
                _ => return Err(format!("manifest line {}: unknown key `{k}`", n + 1)),
            }
        }
        out.push(t);
    }
    Ok(out)
}

fn parts(p: &Path) -> Vec<String> {
    p.components().filter_map(|c| if let Component::Normal(s) = c { Some(s.to_string_lossy().into_owned()) } else { None }).collect()
}

/// The path from the directory of `from_file` to `to_file` (both relative to the same root), with `/` separators.
pub fn relative_import(from_file: &Path, to_file: &Path) -> String {
    let from = parts(from_file.parent().unwrap_or(Path::new("")));
    let to = parts(to_file);
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut segs: Vec<String> = vec!["..".to_string(); from.len() - common];
    segs.extend(to[common..].iter().cloned());
    let joined = segs.join("/");
    if joined.starts_with("..") { joined } else { format!("./{joined}") }
}

/// A generated file and where it goes (relative to the root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub path: PathBuf,
    pub text: String,
}

fn slash(p: &Path) -> String {
    parts(p).join("/")
}

/// Generates every file of a target from its schema source (a target with `rust_bind` needs
/// [`generate_target_with_bind`]).
pub fn generate_target(target: &Target, source: &str) -> Result<Vec<Generated>, String> {
    generate_target_with_bind(target, source, None)
}

/// [`generate_target`] with the text of the target's bind map (`bind=`), when it has one.
pub fn generate_target_with_bind(target: &Target, source: &str, bind_map: Option<&str>) -> Result<Vec<Generated>, String> {
    let schema = load(source).map_err(|e| format!("{}: {e}", slash(&target.schema)))?;
    let label = slash(&target.schema);
    let mut files = Vec::new();
    let runtime = target.ts_runtime.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_TS_RUNTIME));
    if let Some(p) = &target.rust {
        files.push(Generated { path: p.clone(), text: rust::emit(&schema, &label) });
    }
    if let Some(p) = &target.ts {
        files.push(Generated { path: p.clone(), text: ts::emit(&schema, &label, &relative_import(p, &runtime)) });
    }
    if let Some(p) = &target.rust_support {
        let main = target.rust.as_ref().ok_or("`rust_support` needs `rust`")?;
        if main.parent() != p.parent() {
            return Err("`rust_support` must be in the same directory as `rust`".to_string());
        }
        let stem = main.file_stem().and_then(|s| s.to_str()).ok_or("bad `rust` path")?;
        files.push(Generated { path: p.clone(), text: rust_support::emit(&schema, &label, stem) });
    }
    if let Some(p) = &target.ts_support {
        let main = target.ts.as_ref().ok_or("`ts_support` needs `ts`")?;
        files.push(Generated { path: p.clone(), text: ts_support::emit(&schema, &label, &relative_import(p, main)) });
    }
    match (&target.rust_bind, &target.bind) {
        (Some(p), Some(b)) => {
            let main = target.rust.as_ref().ok_or("`rust_bind` needs `rust`")?;
            if main.parent() != p.parent() {
                return Err("`rust_bind` must be in the same directory as `rust`".to_string());
            }
            let text = bind_map.ok_or("`rust_bind`: the bind map was not read")?;
            let map = rust_bind::parse(text).map_err(|e| format!("{}: {e}", slash(b)))?;
            let source = format!("{label} and {}", slash(b));
            let stem = main.file_stem().and_then(|s| s.to_str()).ok_or("bad `rust` path")?;
            files.push(Generated {
                path: p.clone(),
                text: rust_bind::emit(&schema, &map, &source, stem).map_err(|e| format!("{}: {e}", slash(b)))?,
            });
        }
        (None, None) => {}
        _ => return Err("`rust_bind` and `bind` go together".to_string()),
    }
    Ok(files)
}

/// Reads the manifest under `root` and generates everything.
pub fn generate_all(root: &Path, manifest: &Path) -> Result<Vec<Generated>, String> {
    let text = fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut all = Vec::new();
    for t in parse_manifest(&text)? {
        let path = root.join(&t.schema);
        let src = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bind = match &t.bind {
            Some(b) => {
                let p = root.join(b);
                Some(normalized(&fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?))
            }
            None => None,
        };
        all.extend(generate_target_with_bind(&t, &src, bind.as_deref())?);
    }
    Ok(all)
}

/// The text with CR removed (a Windows checkout may have converted the line ends).
pub fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}
