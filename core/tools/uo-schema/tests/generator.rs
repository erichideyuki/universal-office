//! The generator itself: the committed outputs are current (the CI regeneration check, also run as
//! `uo-schema check`), the grammar and its rules, the schema hash, the manifest.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use uo_schema::{Target, generate_all, generate_target, load, normalized, parse_manifest, relative_import};

fn core() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file of `core/schema/manifest.txt` is what the generator writes from its schema today.
#[test]
fn committed_generated_files_are_current() {
    let root = core();
    let files = generate_all(&root, &root.join("schema/manifest.txt")).unwrap();
    assert!(files.len() >= 12, "{} files", files.len());
    for f in &files {
        let on_disk = std::fs::read_to_string(root.join(&f.path)).unwrap_or_else(|e| panic!("{}: {e}", f.path.display()));
        assert!(
            normalized(&on_disk) == f.text,
            "{} is out of date: run `cargo run -p uo-schema -- generate` and commit the result",
            f.path.display()
        );
        assert!(!f.text.contains('\r'), "{}: LF only", f.path.display());
        assert!(f.text.ends_with('\n'));
    }
}

#[test]
fn generation_is_deterministic() {
    let root = core();
    let manifest = root.join("schema/manifest.txt");
    assert_eq!(generate_all(&root, &manifest).unwrap(), generate_all(&root, &manifest).unwrap());
}

#[test]
fn the_runtime_and_the_generator_agree_on_the_table_limit() {
    assert_eq!(uo_schema::model::MAX_TABLES, uo_codec::MAX_TABLES);
}

#[test]
fn manifest_lines_parse() {
    let t = parse_manifest("# c\nschema/a.uos rust=x/a.rs ts=y/a.ts rust_support=x/a_s.rs ts_support=y/a.s.ts # tail\n\nb.uos rust=b.rs\n")
        .unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].schema, PathBuf::from("schema/a.uos"));
    assert_eq!(t[0].rust, Some(PathBuf::from("x/a.rs")));
    assert_eq!(t[0].ts_support, Some(PathBuf::from("y/a.s.ts")));
    assert_eq!(t[1], Target { schema: PathBuf::from("b.uos"), rust: Some(PathBuf::from("b.rs")), ..Target::default() });
    assert!(parse_manifest("a.uos nonsense").is_err());
    assert!(parse_manifest("a.uos wat=1").is_err());
}

#[test]
fn relative_imports() {
    let p = Path::new;
    assert_eq!(
        relative_import(p("crates/uo-wasm/js/gen/render.ts"), p("crates/uo-codec/js/runtime.ts")),
        "../../../uo-codec/js/runtime.ts"
    );
    assert_eq!(relative_import(p("a/x.support.ts"), p("a/x.ts")), "./x.ts");
    assert_eq!(relative_import(p("x.ts"), p("lib/runtime.ts")), "./lib/runtime.ts");
}

const BASE: &str = "schema t magic \"TEST\"; struct A { x: f64, y: sym? }";

#[test]
fn the_hash_covers_the_wire_not_the_source_text() {
    let base = load(BASE).unwrap();
    assert_eq!(
        base.canonical,
        "uos1 schema t magic TEST
table sym utf16
struct A { x:f64; y:sym?; }
",
        "the normalized form the hash is taken over (changing it changes every schema hash: a wire version bump)"
    );
    assert_eq!(base.hash_hex(), "ebe79f884b66eb70");
    // comments, docs, spacing and TypeScript names are not on the wire
    let dressed = "/// doc\nschema t magic \"TEST\";\n\n// c\n/// a struct\nstruct A {\n  /// the x\n  x: f64 ts \"ex\",\n  y: sym?,\n}\n";
    assert_eq!(load(dressed).unwrap().hash, base.hash);
    // names, types, order, optionality and the magic are
    for changed in [
        "schema t magic \"TEST\"; struct A { x: f64, z: sym? }",
        "schema t magic \"TEST\"; struct A { x: u32, y: sym? }",
        "schema t magic \"TEST\"; struct A { y: sym?, x: f64 }",
        "schema t magic \"TEST\"; struct A { x: f64, y: sym }",
        "schema t magic \"TESX\"; struct A { x: f64, y: sym? }",
        "schema u magic \"TEST\"; struct A { x: f64, y: sym? }",
        "schema t magic \"TEST\"; struct B { x: f64, y: sym? }",
    ] {
        assert_ne!(load(changed).unwrap().hash, base.hash, "{changed}");
    }
}

fn rejects(src: &str, needle: &str) {
    match load(src) {
        Ok(_) => panic!("accepted: {src}"),
        Err(e) => assert!(e.to_string().contains(needle), "`{e}` does not mention `{needle}` for: {src}"),
    }
}

#[test]
fn syntax_errors_name_the_line() {
    rejects("struct A { x: f64 }", "expected `schema`");
    rejects("schema t magic \"TEST\";\nstruct A { x f64 }", "line 2: expected `:`");
    rejects("schema t magic \"TEST\";\nstruct A { x: wat }", "unknown type `wat`");
    rejects("schema t magic \"TEST\";\nstruct A { x: f64 } }", "expected a declaration");
    rejects("schema t magic \"TEST\"; struct A { x: f64 ", "expected `}`");
    rejects("schema t magic \"TEST\"; struct A { x: \"s\" }", "expected a type");
    rejects("schema t magic \"TEST\"; struct A { x: f64 } $", "unexpected character");
    rejects("schema t magic \"TEST\"; struct A { x: f64 ts \"a\n\" }", "unterminated string");
    rejects("schema t magic \"TEST\"; widget A {}", "expected `table`, `struct`, `message` or `enum`");
}

#[test]
fn rules_that_keep_the_generated_code_valid() {
    rejects("schema t magic \"TES\"; struct A { x: f64 }", "4 printable ASCII");
    rejects("schema T magic \"TEST\"; struct A { x: f64 }", "lower_snake_case");
    rejects("schema t magic \"TEST\"; struct A { x: f64 } struct A { y: f64 }", "declared twice");
    rejects("schema t magic \"TEST\"; struct A { }", "no fields");
    rejects("schema t magic \"TEST\"; enum E { }", "no variants");
    rejects("schema t magic \"TEST\"; struct a { x: f64 }", "UpperCamelCase");
    rejects("schema t magic \"TEST\"; struct A { X: f64 }", "lower_snake_case");
    rejects("schema t magic \"TEST\"; struct A { x: f64, x: u32 }", "declared twice");
    rejects("schema t magic \"TEST\"; struct A { self: f64 }", "raw identifier");
    rejects("schema t magic \"TEST\"; struct Tables { x: f64 }", "reserved");
    rejects("schema t magic \"TEST\"; struct Vec { x: f64 }", "reserved");
    rejects("schema t magic \"TEST\"; struct A { x: B }", "unknown type `B`");
    rejects("schema t magic \"TEST\"; struct A { x: ref<nope> }", "no table `nope`");
    rejects("schema t magic \"TEST\"; table f: A; struct A { x: f64 } table f: A;", "declared twice");
    rejects("schema t magic \"TEST\"; table f: Nope;", "unknown struct");
    rejects("schema t magic \"TEST\"; enum E { A } table f: E;", "plain struct");
    rejects("schema t magic \"TEST\"; table sym: A; struct A { x: f64 }", "implicit symbol table");
    rejects("schema t magic \"TEST\"; enum E { A } struct S { x: columns<E> }", "needs a plain struct");
    rejects("schema t magic \"TEST\"; struct A { x: columns<Nope> }", "unknown struct");
    rejects("schema t magic \"TEST\"; message M { x: f64 } struct A { m: M }", "message");
    rejects("schema t magic \"TEST\"; struct A { a: A }", "recursion is only allowed through `list<>`");
    rejects("schema t magic \"TEST\"; struct A { a: A? }", "recursion is only allowed through `list<>`");
    rejects("schema t magic \"TEST\"; struct A { b: B } struct B { a: A }", "recursion is only allowed through `list<>`");
    rejects("schema t magic \"TEST\"; enum E { A(E) }", "recursion is only allowed through `list<>`");
    rejects("schema t magic \"TEST\"; enum E tag \"t\" { A, B }", "no payloads");
    rejects("schema t magic \"TEST\"; enum E { A, A }", "declared twice");
    rejects("schema t magic \"TEST\"; enum E { A = \"x\", B = \"x\" }", "TypeScript name");
    rejects("schema t magic \"TEST\"; struct A { x_y: f64, x: f64 ts \"xY\" }", "TypeScript name `xY`");
    rejects("schema t magic \"TEST\"; struct A { x: f64 ts \"q\", y: f64 ts \"q\" }", "TypeScript name `q`");
    rejects("schema t magic \"TEST\"; struct P { t: f64 } enum E tag \"t\" { A(P) }", "also a field of `P`");
    rejects("schema t magic \"TEST\"; struct A { x: f64 ts \"a\\b\" }", "usable TypeScript");
}

#[test]
fn a_struct_holds_at_most_64_bit_fields() {
    let fields = |n: usize| (0..n).map(|i| format!("f{i}: bool")).collect::<Vec<_>>().join(", ");
    assert!(load(&format!("schema t magic \"TEST\"; struct A {{ {} }}", fields(64))).is_ok());
    rejects(&format!("schema t magic \"TEST\"; struct A {{ {} }}", fields(65)), "the bitmap holds 64");
}

#[test]
fn at_most_16_tables() {
    let tables = |n: usize| (0..n).map(|i| format!("table t{i}: A;")).collect::<Vec<_>>().join(" ");
    let src = |n: usize| format!("schema t magic \"TEST\"; struct A {{ x: f64 }} {}", tables(n));
    assert!(load(&src(16)).is_ok());
    rejects(&src(17), "17 tables");
    // the symbol table counts when `sym` is used
    let with_sym = format!("schema t magic \"TEST\"; struct A {{ x: sym }} {}", tables(16));
    rejects(&with_sym, "17 tables");
}

#[test]
fn the_grammar_accepts_what_the_schemas_use() {
    let src = r#"
/// Schema doc.
schema demo magic "DEMO";
/// table doc
table faces: Face;
enum Kind { A, B = "bee" }            // unit enum, TypeScript string union
enum Op tag "op" { M = "M"(P), Z = "Z", L(list<P>), N(f64), S(sym) }
struct P { x: f64 ts "0", y: f64 ts "1", }          // a trailing comma, numeric TypeScript names
struct Face { key: sym, kind: Kind?, n: list<list<u16>>, raw: bytes?, a: app_section, h: bytes_ref }
struct Row { c: u32, flag: bool, o: f64? }
/// a root
message Doc { face: ref<faces>, rows: columns<Row>, ops: list<Op>, nested: list<Doc2>? }
struct Doc2 { inner: list<Doc2> }
"#;
    let s = load(src).unwrap();
    assert_eq!(s.messages.len(), 1);
    assert_eq!(s.tables.len(), 2, "the implicit symbol table and `faces`");
    assert!(s.uses_sym());
    let no_sym = load("schema t magic \"TEST\"; struct A { x: f64 }").unwrap();
    assert!(!no_sym.uses_sym());
    assert!(no_sym.tables.is_empty());
}

#[test]
fn emitted_rust_and_typescript_have_the_expected_shape() {
    let src = "schema t magic \"TEST\"; table faces: Face; struct Face { key: sym, ref: bool } message M { f: ref<faces>, p: columns<R> } struct R { a: u32 }";
    let target = Target { schema: PathBuf::from("t.uos"), rust: Some("o/t.rs".into()), ts: Some("o/t.ts".into()), ..Target::default() };
    let files = generate_target(&target, src).unwrap();
    let rust = &files[0].text;
    let ts = &files[1].text;
    assert!(rust.contains("pub r#ref: bool"), "a Rust keyword becomes a raw identifier");
    assert!(rust.contains("pub struct RColumns"));
    assert!(rust.contains("pub fn encode_m(out: &mut Vec<u8>, delta: &Delta, msg: &M)"));
    assert!(rust.contains("pub fn decode_m(buf: &[u8], tables: &mut Tables, limits: &Limits)"));
    assert!(rust.contains("#![cfg_attr(rustfmt, rustfmt::skip)]"));
    assert!(!rust.contains("unsafe"));
    assert!(ts.contains("export function decodeM(buf: Uint8Array, tables: Tables"));
    assert!(ts.contains("export function encodeM(msg: M, state: EncodeState): Uint8Array"));
    assert!(ts.contains("from \"./../uo-codec/js/runtime.ts\"") || ts.contains("runtime.ts\""));
    assert!(rust.contains(&format!("hash {}", load(src).unwrap().hash_hex())));
}
