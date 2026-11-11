//! The bind emitter (milestone 2, the session in wasm): conversions between the generated wire types of
//! a schema and existing Rust types of the engine, so a schema can carry a type that already exists
//! (the session's `Command` and its argument types) without a second hand-written mapping.
//!
//! It reads a **bind map** (a text file next to the schema, the manifest key `bind=`) and emits, for
//! every wire type the map names, `impl FromWire<w::T> for Native` and `impl IntoWire<w::T> for Native`,
//! against traits the consumer provides (the `runtime` line). Every conversion goes through the traits
//! (`FromWire::from_wire(x)?`, `IntoWire::into_wire(&x)?`), so type inference picks the leaf impls the
//! runtime has (primitives, strings, lists, options, boxes, ids); every match is exhaustive in both
//! directions, so a new variant or field of a bound type fails to compile until the schema has it.
//!
//! # The bind map
//! One declaration per line; `#` starts a comment.
//!
//! - `runtime <rust path>`: the module of `FromWire`, `IntoWire` and `BindError` (required, once).
//! - `<Wire> struct <path> [skip=a,b] [empty] [into_only]`: field by field, same names. `skip=` names the
//!   Rust-only fields (a field the schema leaves out; raw identifiers as `r#type`; the modifier may repeat):
//!   `FromWire` sets each to `Default::default()`, and `IntoWire` destructures the native struct with NO
//!   rest pattern, naming every wire field and every skipped one, so a new real field of the native struct
//!   fails to compile until the schema or the `skip=` list has it. (The old `default` modifier is gone.)
//!   `empty`: the wire struct only carries a placeholder (`<path>::default()` back, the placeholder
//!   `false` out).
//! - `<Wire> enum <path> [native_only=A,B] [into_only]`: variant by variant, same names. A variant whose
//!   payload is a struct without a bind line is a Rust struct variant (`V { a, b }`); a payload struct
//!   bound `tuple` is a Rust tuple variant spread over its fields (`V(a, b)`); any other payload is a
//!   one-field tuple variant (`V(x)`); no payload is a unit variant. `native_only` variants exist only
//!   in Rust: `IntoWire` answers `BindError::NativeOnly`.
//! - `<Wire> tuple [into_only]`: the fields in order are a Rust tuple (`(A, B)`; generic over the
//!   element types).
//! - `<Wire> option [into_only]`: a struct of one optional field is a Rust `Option<T>` (where the IDL
//!   has no optional: a list element, a payload, an option of an option).
//! - `<Wire> newtype <path> [into_only]`: a struct of one field is a Rust newtype (`Path(x)`).
//! - `<Wire> manual`: the consumer writes the impls.
//!
//! `into_only` emits `IntoWire` only (outputs of the engine). Generic Rust paths (`Nullable<PicColor>`)
//! are written as they are; their patterns use the path without the arguments.

use std::collections::BTreeSet;

use crate::model::{Def, EnumDef, Schema, StructDef, Ty};
use crate::out::Out;
use crate::rust::rust_ty;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Struct { path: String, skip: Vec<String>, empty: bool },
    Enum { path: String, native_only: Vec<String> },
    Tuple,
    Option,
    Newtype { path: String },
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub wire: String,
    pub kind: Kind,
    pub into_only: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMap {
    pub runtime: String,
    pub binds: Vec<Bind>,
}

/// Parses a bind map (see the module documentation).
pub fn parse(text: &str) -> Result<BindMap, String> {
    let mut runtime: Option<String> = None;
    let mut binds: Vec<Bind> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line_no = n + 1;
        // a comment starts at a `#` that begins a word (`r#type` is a raw identifier, not a comment)
        let cut = raw
            .char_indices()
            .find(|(i, c)| *c == '#' && raw[..*i].chars().next_back().is_none_or(char::is_whitespace))
            .map_or(raw.len(), |(i, _)| i);
        let line = raw[..cut].trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let err = |m: &str| format!("bind map line {line_no}: {m}");
        if words.first() == Some(&"runtime") {
            if runtime.is_some() {
                return Err(err("`runtime` twice"));
            }
            let Some(p) = words.get(1) else { return Err(err("`runtime` needs a path")) };
            runtime = Some((*p).to_string());
            continue;
        }
        let (Some(wire), Some(kind)) = (words.first(), words.get(1)) else { return Err(err("expected `<Wire> <kind> ...`")) };
        let mut rest: Vec<&str> = words.iter().skip(2).copied().collect();
        let into_only = rest.last() == Some(&"into_only");
        if into_only {
            rest.pop();
        }
        let kind = match *kind {
            "struct" => {
                let Some(path) = rest.first() else { return Err(err("`struct` needs a path")) };
                let mut skip: Vec<String> = Vec::new();
                let mut empty = false;
                for m in rest.iter().skip(1) {
                    if *m == "empty" {
                        empty = true;
                    } else if *m == "default" {
                        return Err(err("`default` is gone: name the Rust-only fields with skip=a,b"));
                    } else if let Some(list) = m.strip_prefix("skip=") {
                        for name in list.split(',').filter(|x| !x.is_empty()) {
                            if skip.iter().any(|x| x == name) {
                                return Err(err(&format!("skip field `{name}` twice")));
                            }
                            skip.push(name.to_string());
                        }
                    } else {
                        return Err(err(&format!("unknown struct modifier `{m}`")));
                    }
                }
                Kind::Struct { path: (*path).to_string(), skip, empty }
            }
            "enum" => {
                let Some(path) = rest.first() else { return Err(err("`enum` needs a path")) };
                let mut native_only = Vec::new();
                for m in rest.iter().skip(1) {
                    match m.strip_prefix("native_only=") {
                        Some(list) => native_only.extend(list.split(',').filter(|s| !s.is_empty()).map(str::to_string)),
                        None => return Err(err(&format!("unknown enum modifier `{m}`"))),
                    }
                }
                Kind::Enum { path: (*path).to_string(), native_only }
            }
            "tuple" | "option" | "manual" if !rest.is_empty() => return Err(err(&format!("`{kind}` takes no path"))),
            "tuple" => Kind::Tuple,
            "option" => Kind::Option,
            "manual" => Kind::Manual,
            "newtype" => match rest.as_slice() {
                [path] => Kind::Newtype { path: (*path).to_string() },
                _ => return Err(err("`newtype` needs exactly a path")),
            },
            other => return Err(err(&format!("unknown kind `{other}`"))),
        };
        if binds.iter().any(|b| b.wire == *wire) {
            return Err(err(&format!("`{wire}` bound twice")));
        }
        binds.push(Bind { wire: (*wire).to_string(), kind, into_only, line: line_no });
    }
    let Some(runtime) = runtime else { return Err("bind map: no `runtime` line".to_string()) };
    Ok(BindMap { runtime, binds })
}

fn def_index(schema: &Schema, name: &str) -> Option<usize> {
    schema.defs.iter().position(|d| d.name() == name)
}

/// The path without generic arguments (for patterns and constructors of enum variants).
fn bare(path: &str) -> &str {
    path.split('<').next().unwrap_or(path)
}

/// A struct-literal path: `a::B<C>` → `a::B::<C>`.
fn literal_path(path: &str) -> String {
    match path.find('<') {
        Some(i) => format!("{}::{}", &path[..i], &path[i..]),
        None => path.to_string(),
    }
}

struct Ctx<'a> {
    schema: &'a Schema,
    map: &'a BindMap,
}

impl Ctx<'_> {
    fn bind_of(&self, name: &str) -> Option<&Bind> {
        self.map.binds.iter().find(|b| b.wire == name)
    }

    fn payload_struct<'s>(&'s self, t: &Ty) -> Option<&'s StructDef> {
        match t {
            Ty::Named(i) => match self.schema.def(*i) {
                Def::Struct(s) => Some(s),
                Def::Enum(_) => None,
            },
            _ => None,
        }
    }
}

/// Emits the conversions of `map` over `schema`; `source` names the inputs in the header, `stem` is the
/// sibling module of the generated types (`use super::<stem> as w;`).
pub fn emit(schema: &Schema, map: &BindMap, source: &str, stem: &str) -> Result<String, String> {
    let cx = Ctx { schema, map };
    // check the map against the schema
    for b in &map.binds {
        let Some(i) = def_index(schema, &b.wire) else {
            return Err(format!("bind map line {}: no type `{}` in the schema", b.line, b.wire));
        };
        let at = |m: &str| format!("bind map line {}: `{}`: {m}", b.line, b.wire);
        match (&b.kind, schema.def(i)) {
            (Kind::Enum { native_only, .. }, Def::Enum(e)) => {
                for v in native_only {
                    if e.variants.iter().any(|x| x.name == *v) {
                        return Err(at(&format!("native_only variant `{v}` is also on the wire")));
                    }
                }
            }
            (Kind::Enum { .. }, Def::Struct(_)) => return Err(at("is a struct, bound as an enum")),
            (Kind::Manual, _) => {}
            (_, Def::Enum(_)) => return Err(at("is an enum, bound as a struct")),
            (Kind::Option, Def::Struct(s)) if s.fields.len() != 1 || !s.fields.iter().all(|f| f.optional) => {
                return Err(at("an `option` binding needs a struct of one optional field"));
            }
            (Kind::Newtype { .. }, Def::Struct(s)) if s.fields.len() != 1 => {
                return Err(at("a `newtype` binding needs a struct of one field"));
            }
            (Kind::Struct { skip, empty, .. }, Def::Struct(s)) if *empty && !skip.is_empty() => {
                let _ = s;
                return Err(at("`skip` together with `empty`"));
            }
            (Kind::Struct { skip, .. }, Def::Struct(s)) if skip.iter().any(|k| s.fields.iter().any(|f| f.rname == *k)) => {
                return Err(at("a `skip` field is on the wire"));
            }
            (Kind::Struct { empty: true, .. }, Def::Struct(s)) if s.fields.len() != 1 || s.fields.iter().any(|f| f.ty != Ty::Bool) => {
                return Err(at("an `empty` binding needs a struct of one bool placeholder"));
            }
            _ => {}
        }
    }
    let mut o = Out::new("    ");
    o.line(&format!(
        "// @generated by core/tools/uo-schema (rust_bind) from {source} (schema `{}`, hash {}). Do not edit: change the bind map or the .uos file and run `cargo run -p uo-schema -- generate`.",
        schema.name,
        schema.hash_hex()
    ));
    o.line("#![cfg_attr(rustfmt, rustfmt::skip)]");
    o.line("#![allow(clippy::redundant_field_names, clippy::needless_borrow, clippy::match_single_binding, clippy::too_many_lines, clippy::unnecessary_wraps, clippy::clone_on_copy, clippy::useless_conversion, clippy::default_trait_access, clippy::needless_update, clippy::unneeded_struct_pattern, unused_variables)]");
    o.blank();
    o.line(&format!("use super::{stem} as w;"));
    o.line(&format!("use {}::{{BindError, FromWire, IntoWire}};", map.runtime));
    o.blank();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for b in &map.binds {
        let Some(i) = def_index(schema, &b.wire) else { continue };
        if !done.insert(b.wire.as_str()) {
            continue;
        }
        match (&b.kind, schema.def(i)) {
            (Kind::Manual, _) => {}
            (Kind::Struct { path, skip, empty }, Def::Struct(s)) => emit_struct(&mut o, s, path, skip, *empty, b.into_only),
            (Kind::Tuple, Def::Struct(s)) => emit_tuple(&cx, &mut o, s, b.into_only),
            (Kind::Option, Def::Struct(s)) => emit_option(&cx, &mut o, s, b.into_only),
            (Kind::Newtype { path }, Def::Struct(s)) => emit_newtype(&mut o, s, path, b.into_only),
            (Kind::Enum { path, native_only }, Def::Enum(e)) => emit_enum(&cx, &mut o, e, path, native_only, b.into_only),
            _ => {}
        }
    }
    Ok(o.finish())
}

fn emit_struct(o: &mut Out, s: &StructDef, path: &str, skip: &[String], empty: bool, into_only: bool) {
    let n = &s.name;
    if !into_only {
        o.open(&format!("impl FromWire<w::{n}> for {path} {{"));
        o.open(&format!("fn from_wire(v: w::{n}) -> Result<Self, BindError> {{"));
        if empty {
            o.line("Ok(Default::default())");
        } else {
            o.open(&format!("Ok({} {{", literal_path(path)));
            for f in &s.fields {
                o.line(&format!("{r}: FromWire::from_wire(v.{r})?,", r = f.rname));
            }
            for k in skip {
                o.line(&format!("{k}: Default::default(),"));
            }
            o.close("})");
        }
        o.close("}");
        o.close("}");
        o.blank();
    }
    o.open(&format!("impl IntoWire<w::{n}> for {path} {{"));
    o.open(&format!("fn into_wire(&self) -> Result<w::{n}, BindError> {{"));
    if empty {
        let f = s.fields.first().map_or("unused".to_string(), |f| f.rname.clone());
        o.line(&format!("Ok(w::{n} {{ {f}: false }})"));
    } else {
        // every field of the native struct is named (no `..`): a new real field fails to compile
        let bind = |r: &str| format!("x_{}", r.trim_start_matches("r#"));
        let mut pat: Vec<String> = s.fields.iter().map(|f| format!("{r}: {b}", r = f.rname, b = bind(&f.rname))).collect();
        pat.extend(skip.iter().map(|k| format!("{k}: _")));
        o.line(&format!("let {} {{ {} }} = self;", bare(path), pat.join(", ")));
        o.open(&format!("Ok(w::{n} {{"));
        for f in &s.fields {
            o.line(&format!("{r}: IntoWire::into_wire({b})?,", r = f.rname, b = bind(&f.rname)));
        }
        o.close("})");
    }
    o.close("}");
    o.close("}");
    o.blank();
}

/// The Rust type of a wire value as the bind file names it (the generated types under `w::`).
fn wire_ty(schema: &Schema, ty: &Ty) -> String {
    match ty {
        Ty::Named(i) => format!("w::{}", schema.def(*i).name()),
        Ty::Columns(i) => format!("w::{}Columns", schema.def(*i).name()),
        Ty::List(inner) => match **inner {
            Ty::Bool | Ty::U8 => rust_ty(schema, ty),
            _ => format!("Vec<{}>", wire_ty(schema, inner)),
        },
        _ => rust_ty(schema, ty),
    }
}

fn field_wire_ty(schema: &Schema, f: &crate::model::Field) -> String {
    let t = wire_ty(schema, &f.ty);
    if f.optional { format!("Option<{t}>") } else { t }
}

fn emit_tuple(cx: &Ctx<'_>, o: &mut Out, s: &StructDef, into_only: bool) {
    let n = &s.name;
    let params: Vec<String> = (0..s.fields.len()).map(|i| format!("T{i}")).collect();
    let tuple = if params.len() == 1 { format!("({},)", params[0]) } else { format!("({})", params.join(", ")) };
    if !into_only {
        let bounds: Vec<String> =
            s.fields.iter().zip(&params).map(|(f, p)| format!("{p}: FromWire<{}>", field_wire_ty(cx.schema, f))).collect();
        o.open(&format!("impl<{}> FromWire<w::{n}> for {tuple} {{", bounds.join(", ")));
        o.open(&format!("fn from_wire(v: w::{n}) -> Result<Self, BindError> {{"));
        let parts: Vec<String> = s.fields.iter().map(|f| format!("FromWire::from_wire(v.{})?", f.rname)).collect();
        o.line(&format!("Ok(({}{}))", parts.join(", "), if parts.len() == 1 { "," } else { "" }));
        o.close("}");
        o.close("}");
        o.blank();
    }
    let bounds: Vec<String> =
        s.fields.iter().zip(&params).map(|(f, p)| format!("{p}: IntoWire<{}>", field_wire_ty(cx.schema, f))).collect();
    o.open(&format!("impl<{}> IntoWire<w::{n}> for {tuple} {{", bounds.join(", ")));
    o.open(&format!("fn into_wire(&self) -> Result<w::{n}, BindError> {{"));
    let parts: Vec<String> = s.fields.iter().enumerate().map(|(i, f)| format!("{}: IntoWire::into_wire(&self.{i})?", f.rname)).collect();
    o.line(&format!("Ok(w::{n} {{ {} }})", parts.join(", ")));
    o.close("}");
    o.close("}");
    o.blank();
}

fn emit_option(cx: &Ctx<'_>, o: &mut Out, s: &StructDef, into_only: bool) {
    let n = &s.name;
    let Some(f) = s.fields.first() else { return };
    let inner = wire_ty(cx.schema, &f.ty);
    if !into_only {
        o.open(&format!("impl<T: FromWire<{inner}>> FromWire<w::{n}> for Option<T> {{"));
        o.open(&format!("fn from_wire(v: w::{n}) -> Result<Self, BindError> {{"));
        o.line(&format!("FromWire::from_wire(v.{})", f.rname));
        o.close("}");
        o.close("}");
        o.blank();
    }
    o.open(&format!("impl<T: IntoWire<{inner}>> IntoWire<w::{n}> for Option<T> {{"));
    o.open(&format!("fn into_wire(&self) -> Result<w::{n}, BindError> {{"));
    o.line(&format!("Ok(w::{n} {{ {}: IntoWire::into_wire(self)? }})", f.rname));
    o.close("}");
    o.close("}");
    o.blank();
}

fn emit_newtype(o: &mut Out, s: &StructDef, path: &str, into_only: bool) {
    let n = &s.name;
    let Some(f) = s.fields.first() else { return };
    if !into_only {
        o.open(&format!("impl FromWire<w::{n}> for {path} {{"));
        o.open(&format!("fn from_wire(v: w::{n}) -> Result<Self, BindError> {{"));
        o.line(&format!("Ok({}(FromWire::from_wire(v.{})?))", bare(path), f.rname));
        o.close("}");
        o.close("}");
        o.blank();
    }
    o.open(&format!("impl IntoWire<w::{n}> for {path} {{"));
    o.open(&format!("fn into_wire(&self) -> Result<w::{n}, BindError> {{"));
    o.line(&format!("Ok(w::{n} {{ {}: IntoWire::into_wire(&self.0)? }})", f.rname));
    o.close("}");
    o.close("}");
    o.blank();
}

/// How a variant's payload maps onto the Rust variant.
enum Shape<'a> {
    Unit,
    /// a struct variant: the payload struct's fields
    Fields(&'a StructDef),
    /// a tuple variant spread over a `tuple` payload's fields
    Spread(&'a StructDef),
    /// `V(x)`
    One,
}

fn shape<'a>(cx: &'a Ctx<'_>, payload: Option<&Ty>) -> Shape<'a> {
    let Some(t) = payload else { return Shape::Unit };
    match cx.payload_struct(t) {
        Some(s) => match cx.bind_of(&s.name).map(|b| &b.kind) {
            None => Shape::Fields(s),
            Some(Kind::Tuple) => Shape::Spread(s),
            Some(_) => Shape::One,
        },
        None => Shape::One,
    }
}

fn emit_enum(cx: &Ctx<'_>, o: &mut Out, e: &EnumDef, path: &str, native_only: &[String], into_only: bool) {
    let n = &e.name;
    let p = bare(path);
    if !into_only {
        o.open(&format!("impl FromWire<w::{n}> for {path} {{"));
        o.open(&format!("fn from_wire(v: w::{n}) -> Result<Self, BindError> {{"));
        o.open("Ok(match v {");
        for v in &e.variants {
            let vn = &v.name;
            match shape(cx, v.payload.as_ref()) {
                Shape::Unit => o.line(&format!("w::{n}::{vn} => {p}::{vn} {{}},")),
                Shape::One => o.line(&format!("w::{n}::{vn}(x) => {p}::{vn}(FromWire::from_wire(x)?),")),
                Shape::Fields(s) => {
                    let parts: Vec<String> = s.fields.iter().map(|f| format!("{r}: FromWire::from_wire(x.{r})?", r = f.rname)).collect();
                    o.line(&format!("w::{n}::{vn}(x) => {p}::{vn} {{ {} }},", parts.join(", ")));
                }
                Shape::Spread(s) => {
                    let parts: Vec<String> = s.fields.iter().map(|f| format!("FromWire::from_wire(x.{})?", f.rname)).collect();
                    o.line(&format!("w::{n}::{vn}(x) => {p}::{vn}({}),", parts.join(", ")));
                }
            }
        }
        o.close("})");
        o.close("}");
        o.close("}");
        o.blank();
    }
    o.open(&format!("impl IntoWire<w::{n}> for {path} {{"));
    o.open(&format!("fn into_wire(&self) -> Result<w::{n}, BindError> {{"));
    o.open("Ok(match self {");
    for v in &e.variants {
        let vn = &v.name;
        match shape(cx, v.payload.as_ref()) {
            Shape::Unit => o.line(&format!("{p}::{vn} {{ .. }} => w::{n}::{vn},")),
            Shape::One => o.line(&format!("{p}::{vn}(x) => w::{n}::{vn}(IntoWire::into_wire(x)?),")),
            Shape::Fields(s) => {
                let binds: Vec<String> = s.fields.iter().map(|f| f.rname.clone()).collect();
                let parts: Vec<String> = s.fields.iter().map(|f| format!("{r}: IntoWire::into_wire({r})?", r = f.rname)).collect();
                o.line(&format!("{p}::{vn} {{ {} }} => w::{n}::{vn}(w::{} {{ {} }}),", binds.join(", "), s.name, parts.join(", ")));
            }
            Shape::Spread(s) => {
                let binds: Vec<String> = (0..s.fields.len()).map(|i| format!("x{i}")).collect();
                let parts: Vec<String> =
                    s.fields.iter().zip(&binds).map(|(f, b)| format!("{}: IntoWire::into_wire({b})?", f.rname)).collect();
                o.line(&format!("{p}::{vn}({}) => w::{n}::{vn}(w::{} {{ {} }}),", binds.join(", "), s.name, parts.join(", ")));
            }
        }
    }
    for v in native_only {
        o.line(&format!(
            "{p}::{v} {{ .. }} => return Err(BindError::NativeOnly(\"{}::{v}\")),",
            bare(path).rsplit("::").next().unwrap_or(n)
        ));
    }
    o.close("})");
    o.close("}");
    o.close("}");
    o.blank();
}
