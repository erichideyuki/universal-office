//! The bind emitter's map parser and checks (src/rust_bind.rs): the grammar, the refusals, a target with
//! a bind map in the manifest, determinism.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use uo_schema::rust_bind::{Kind, emit, parse};
use uo_schema::{Target, generate_target, generate_target_with_bind, load, parse_manifest};

const SCHEMA: &str = "schema t magic \"TTTT\";\nstruct P { x: f64, n: u32? }\nstruct E { unused: bool }\nenum C { A, B }\nenum S { Unit, One(P), Many(P) }\nmessage M { p: P, c: C }\n";

#[test]
fn the_grammar() {
    let m = parse("# c\nruntime crate::rt\nP struct a::P default\nC enum a::C native_only=X,Y into_only\nE struct a::E empty\nM manual\n")
        .unwrap();
    assert_eq!(m.runtime, "crate::rt");
    assert_eq!(m.binds.len(), 4);
    assert_eq!(m.binds[0].kind, Kind::Struct { path: "a::P".into(), default: true, empty: false });
    assert_eq!(m.binds[1].kind, Kind::Enum { path: "a::C".into(), native_only: vec!["X".into(), "Y".into()] });
    assert!(m.binds[1].into_only);
    assert_eq!(m.binds[3].kind, Kind::Manual);
}

#[test]
fn refusals() {
    for (text, why) in [
        ("P struct a::P\n", "no `runtime`"),
        ("runtime a\nruntime b\n", "`runtime` twice"),
        ("runtime a\nP strukt a::P\n", "unknown kind"),
        ("runtime a\nP struct a::P\nP struct a::Q\n", "bound twice"),
        ("runtime a\nP struct a::P wat\n", "unknown struct modifier"),
        ("runtime a\nP tuple a::P\n", "takes no path"),
        ("runtime a\nP newtype\n", "needs exactly a path"),
    ] {
        let e = parse(text).unwrap_err();
        assert!(e.contains(why), "{text:?}: {e}");
    }
    let schema = load(SCHEMA).unwrap();
    for (text, why) in [
        ("runtime a\nQ struct a::Q\n", "no type `Q`"),
        ("runtime a\nC struct a::C\n", "is an enum"),
        ("runtime a\nP enum a::P\n", "is a struct"),
        ("runtime a\nP option\n", "one optional field"),
        ("runtime a\nP newtype a::P\n", "one field"),
        ("runtime a\nP struct a::P empty\n", "placeholder"),
        ("runtime a\nC enum a::C native_only=A\n", "also on the wire"),
    ] {
        let e = emit(&schema, &parse(text).unwrap(), "t", "g").unwrap_err();
        assert!(e.contains(why), "{text:?}: {e}");
    }
}

#[test]
fn the_shapes_of_the_emitted_code() {
    let schema = load(SCHEMA).unwrap();
    let map = parse("runtime crate::rt\nP struct a::P default\nC enum a::C\nS enum a::S\nE struct a::E empty\nM struct a::M into_only\n")
        .unwrap();
    let out = emit(&schema, &map, "t.uos", "generated").unwrap();
    assert!(out.starts_with("// @generated"));
    assert!(out.contains("use crate::rt::{BindError, FromWire, IntoWire};"));
    assert!(out.contains("use super::generated as w;"));
    assert!(out.contains("..Default::default()"));
    // P is bound: `S::One(x)`, not struct fields
    assert!(out.contains("w::S::One(x) => a::S::One(FromWire::from_wire(x)?)"));
    assert!(out.contains("a::S::Unit { .. } => w::S::Unit"));
    assert!(out.contains("Ok(w::E { unused: false })"));
    // into_only: no FromWire for M
    assert!(!out.contains("impl FromWire<w::M>"));
    assert!(out.contains("impl IntoWire<w::M> for a::M"));
    assert_eq!(out, emit(&schema, &map, "t.uos", "generated").unwrap());
}

#[test]
fn a_target_with_a_bind_map() {
    let t = parse_manifest("s.uos rust=x/g.rs rust_bind=x/b.rs bind=s.bind\n").unwrap();
    assert_eq!(t[0].rust_bind.as_deref(), Some(std::path::Path::new("x/b.rs")));
    let files = generate_target_with_bind(&t[0], SCHEMA, Some("runtime crate::rt\nP struct a::P\n")).unwrap();
    assert_eq!(files.len(), 2);
    assert!(generate_target(&t[0], SCHEMA).unwrap_err().contains("was not read"));
    let lone = Target { bind: None, ..t[0].clone() };
    assert!(generate_target_with_bind(&lone, SCHEMA, None).unwrap_err().contains("go together"));
    let elsewhere = Target { rust_bind: Some("y/b.rs".into()), ..t[0].clone() };
    assert!(generate_target_with_bind(&elsewhere, SCHEMA, Some("runtime a\n")).unwrap_err().contains("same directory"));
}
