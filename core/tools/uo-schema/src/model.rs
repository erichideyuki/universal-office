//! The checked model of a schema: names resolved, rules enforced (every rule that keeps the
//! emitted Rust and TypeScript valid and the decoder bounded is checked here, so the emitters can
//! assume them), the hash computed.

use std::collections::HashSet;

use crate::parse::{Ast, AstItem, AstTy, Error, err};
use crate::sha256::sha256;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Bool,
    U8,
    U16,
    U32,
    I32,
    F64,
    Utf16,
    /// An id into the implicit symbol table (table 0); a string in TypeScript.
    Sym,
    /// Inline bytes (`bytes`, `app_section`).
    Bytes,
    /// A handle to bytes the shell pulls separately: a varint id.
    BytesRef,
    /// An id into a declared table (the index into `Schema::tables`).
    Ref(usize),
    /// A struct or enum (the index into `Schema::defs`).
    Named(usize),
    List(Box<Ty>),
    /// A list of a struct stored column by column (the struct's index in `Schema::defs`).
    Columns(usize),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    /// The Rust identifier (`r#ref` for a keyword).
    pub rname: String,
    pub ts_name: String,
    pub ty: Ty,
    pub optional: bool,
    pub doc: Vec<String>,
}

impl Field {
    /// A field that lives in the struct's bitmap.
    pub fn is_bit(&self) -> bool {
        self.optional || self.ty == Ty::Bool
    }
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub message: bool,
    pub doc: Vec<String>,
    /// Reachable from itself (through lists): the decoder counts depth.
    pub recursive: bool,
    /// Used by a `columns<>` type: a `<Name>Columns` struct is emitted.
    pub columns_used: bool,
    /// Least bits an element takes in a list.
    pub min_bits: u64,
    /// Least bits an element takes in a `columns` value (bool and optional fields take one bit).
    pub columns_min_bits: u64,
}

impl StructDef {
    pub fn bit_fields(&self) -> usize {
        self.fields.iter().filter(|f| f.is_bit()).count()
    }
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub ts_name: String,
    pub payload: Option<Ty>,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub tag: String,
    pub variants: Vec<Variant>,
    pub doc: Vec<String>,
    /// No variant has a payload: a string union in TypeScript, a fieldless enum in Rust.
    pub unit_only: bool,
    pub recursive: bool,
    pub min_bits: u64,
}

#[derive(Debug, Clone)]
pub enum Def {
    Struct(StructDef),
    Enum(EnumDef),
}

impl Def {
    pub fn name(&self) -> &str {
        match self {
            Def::Struct(s) => &s.name,
            Def::Enum(e) => &e.name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    /// `None` for the implicit symbol table (UTF-16 strings).
    pub entry: Option<usize>,
    pub min_bits: u64,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub name: String,
    pub magic: [u8; 4],
    pub hash: [u8; 8],
    pub canonical: String,
    pub tables: Vec<Table>,
    pub defs: Vec<Def>,
    /// Indices into `defs`, in declaration order (the message id is the position here).
    pub messages: Vec<usize>,
}

impl Schema {
    pub fn def(&self, i: usize) -> &Def {
        &self.defs[i]
    }

    pub fn struct_def(&self, i: usize) -> &StructDef {
        match &self.defs[i] {
            Def::Struct(s) => s,
            Def::Enum(_) => unreachable!("checked: {i} is a struct"),
        }
    }

    pub fn uses_sym(&self) -> bool {
        self.tables.first().is_some_and(|t| t.entry.is_none())
    }

    pub fn hash_hex(&self) -> String {
        self.hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Least bits a value of `ty` takes where it stands alone (a payload, a list element, a field).
    pub fn min_bits(&self, ty: &Ty) -> u64 {
        match ty {
            Ty::F64 => 64,
            Ty::Named(i) => match &self.defs[*i] {
                Def::Struct(s) => s.min_bits,
                Def::Enum(e) => e.min_bits,
            },
            _ => 8,
        }
    }

    /// Least bits of an element of `list<ty>`: a `bool` is one bit there.
    pub fn list_elem_min_bits(&self, ty: &Ty) -> u64 {
        if *ty == Ty::Bool { 1 } else { self.min_bits(ty) }
    }
}

/// The runtime keeps one id bound per table (`uo_codec::MAX_TABLES`, a test keeps them equal).
pub const MAX_TABLES: usize = 16;

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield", "try", "gen",
];

/// The Rust identifier of a field name: a keyword becomes a raw identifier (`ref` → `r#ref`).
pub fn rust_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_string() }
}

const RESERVED_TYPES: &[&str] = &[
    "Tables",
    "Delta",
    "Reader",
    "Writer",
    "Limits",
    "DecodeError",
    "EncodeError",
    "TableDelta",
    "Header",
    "Vec",
    "Option",
    "Result",
    "Some",
    "None",
    "Ok",
    "Err",
    "String",
    "Self",
    "Uint8Array",
    "Array",
    "Object",
    "Math",
    "Number",
    "Map",
    "Error",
    "SymTable",
    "Bounds",
    "Rng",
    "EncodeState",
];

fn is_type_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_snake_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.ends_with('_')
        && !s.contains("__")
}

/// A property name: an identifier, or any printable ASCII without quotes or backslashes (`"0"`; reached with `["0"]`).
fn is_ts_prop(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '\'' | '`' | '$'))
}

/// `.name` or `["name"]`: how a property is reached.
pub fn ts_acc(name: &str) -> String {
    if is_js_ident(name) { format!(".{name}") } else { format!("[\"{name}\"]") }
}

/// `name` or `"name"`: how a property is written in an object literal or an interface.
pub fn ts_key(name: &str) -> String {
    if is_js_ident(name) { name.to_string() } else { format!("\"{name}\"") }
}

pub fn is_js_ident(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// `flip_h` → `flipH`.
pub fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `DashDot` → `dashDot`.
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_lowercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

struct Resolver {
    names: Vec<(String, usize)>,
    tables: Vec<(String, usize)>,
}

impl Resolver {
    fn def_index(&self, name: &str) -> Option<usize> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
    }

    fn ty(&self, t: &AstTy, line: usize, sym_table: &mut bool, columns_used: &mut Vec<usize>) -> Result<Ty, Error> {
        Ok(match t {
            AstTy::Prim(p) => match p.as_str() {
                "bool" => Ty::Bool,
                "u8" => Ty::U8,
                "u16" => Ty::U16,
                "u32" => Ty::U32,
                "i32" => Ty::I32,
                "f64" => Ty::F64,
                "utf16" => Ty::Utf16,
                "sym" => {
                    *sym_table = true;
                    Ty::Sym
                }
                "bytes" | "app_section" => Ty::Bytes,
                _ => Ty::BytesRef,
            },
            AstTy::Named(n) => match self.def_index(n) {
                Some(i) => Ty::Named(i),
                None => return err(line, format!("unknown type `{n}`")),
            },
            AstTy::Ref(t) => match self.tables.iter().find(|(n, _)| n == t) {
                Some((_, i)) => Ty::Ref(*i),
                None => return err(line, format!("`ref<{t}>`: no table `{t}`")),
            },
            AstTy::List(inner) => Ty::List(Box::new(self.ty(inner, line, sym_table, columns_used)?)),
            AstTy::Columns(n) => match self.def_index(n) {
                Some(i) => {
                    columns_used.push(i);
                    Ty::Columns(i)
                }
                None => return err(line, format!("`columns<{n}>`: unknown struct")),
            },
        })
    }
}

pub fn check(ast: &Ast) -> Result<Schema, Error> {
    if !is_snake_name(&ast.name) {
        return err(0, format!("schema name `{}` must be lower_snake_case", ast.name));
    }
    let magic: [u8; 4] = match ast.magic.as_bytes().try_into() {
        Ok(m) if ast.magic.chars().all(|c| c.is_ascii_graphic()) => m,
        _ => return err(0, "the magic must be exactly 4 printable ASCII characters"),
    };

    let mut names: Vec<(String, usize)> = Vec::new();
    let mut def_items: Vec<&AstItem> = Vec::new();
    let mut seen = HashSet::new();
    let mut table_decls: Vec<(&String, &String, usize)> = Vec::new();
    for item in &ast.items {
        match item {
            AstItem::Table { name, entry, line } => {
                if !is_snake_name(name) {
                    return err(*line, format!("table name `{name}` must be lower_snake_case"));
                }
                if name == "sym" || name == "symbols" {
                    return err(*line, "`sym` and `symbols` are the implicit symbol table");
                }
                if table_decls.iter().any(|(n, _, _)| *n == name) {
                    return err(*line, format!("table `{name}` declared twice"));
                }
                table_decls.push((name, entry, *line));
            }
            AstItem::Struct { name, line, .. } | AstItem::Enum { name, line, .. } => {
                if !is_type_name(name) {
                    return err(*line, format!("type name `{name}` must be UpperCamelCase letters and digits"));
                }
                if RESERVED_TYPES.contains(&name.as_str()) {
                    return err(*line, format!("`{name}` is reserved by the generated code"));
                }
                if !seen.insert(name.clone()) {
                    return err(*line, format!("type `{name}` declared twice"));
                }
                names.push((name.clone(), def_items.len()));
                def_items.push(item);
            }
        }
    }

    // table 0 is the symbol table if any `sym` is used: decided after the types are resolved, so
    // user tables are numbered from 1 first and shifted back if no symbol is used
    let user_tables: Vec<(String, usize)> = table_decls.iter().enumerate().map(|(i, (n, _, _))| ((*n).clone(), i + 1)).collect();
    let resolver = Resolver { names, tables: user_tables };

    let mut uses_sym = false;
    let mut columns_used: Vec<usize> = Vec::new();
    let mut defs: Vec<Def> = Vec::new();
    for item in &def_items {
        match item {
            AstItem::Struct { name, fields, message, doc, line } => {
                if fields.is_empty() {
                    return err(*line, format!("struct `{name}` has no fields"));
                }
                let mut out_fields = Vec::new();
                let mut names_seen = HashSet::new();
                let mut ts_seen = HashSet::new();
                for f in fields {
                    if !is_snake_name(&f.name) {
                        return err(f.line, format!("field name `{}` must be lower_snake_case", f.name));
                    }
                    if ["self", "super", "crate", "Self"].contains(&f.name.as_str()) {
                        return err(f.line, format!("field name `{}` cannot be a raw identifier", f.name));
                    }
                    if !names_seen.insert(f.name.clone()) {
                        return err(f.line, format!("field `{}` declared twice in `{name}`", f.name));
                    }
                    let ts_name = f.ts.clone().unwrap_or_else(|| camel(&f.name));
                    if !is_ts_prop(&ts_name) {
                        return err(f.line, format!("`{ts_name}` is not a usable TypeScript property name"));
                    }
                    if !ts_seen.insert(ts_name.clone()) {
                        return err(f.line, format!("two fields of `{name}` have the TypeScript name `{ts_name}`"));
                    }
                    let ty = resolver.ty(&f.ty, f.line, &mut uses_sym, &mut columns_used)?;
                    out_fields.push(Field {
                        name: f.name.clone(),
                        rname: rust_ident(&f.name),
                        ts_name,
                        ty,
                        optional: f.optional,
                        doc: f.doc.clone(),
                    });
                }
                let bits = out_fields.iter().filter(|f| f.is_bit()).count();
                if bits > 64 {
                    return err(*line, format!("struct `{name}` has {bits} bool and optional fields; the bitmap holds 64"));
                }
                defs.push(Def::Struct(StructDef {
                    name: name.clone(),
                    fields: out_fields,
                    message: *message,
                    doc: doc.clone(),
                    recursive: false,
                    columns_used: false,
                    min_bits: 0,
                    columns_min_bits: 0,
                }));
            }
            AstItem::Enum { name, tag, variants, doc, line } => {
                if variants.is_empty() {
                    return err(*line, format!("enum `{name}` has no variants"));
                }
                let any_payload = variants.iter().any(|v| v.payload.is_some());
                if tag.is_some() && !any_payload {
                    return err(*line, format!("enum `{name}` has no payloads: `tag` applies to enums with payloads"));
                }
                let tag = tag.clone().unwrap_or_else(|| "t".to_string());
                if !is_js_ident(&tag) {
                    return err(*line, format!("`{tag}` is not a valid TypeScript property name"));
                }
                let mut out_variants = Vec::new();
                let mut seen_v = HashSet::new();
                let mut seen_ts = HashSet::new();
                for v in variants {
                    if !is_type_name(&v.name) {
                        return err(v.line, format!("variant `{}` must be UpperCamelCase", v.name));
                    }
                    if !seen_v.insert(v.name.clone()) {
                        return err(v.line, format!("variant `{}` declared twice in `{name}`", v.name));
                    }
                    let ts_name = v.ts.clone().unwrap_or_else(|| lower_first(&v.name));
                    if !is_ts_prop(&ts_name) {
                        return err(v.line, format!("`{ts_name}` is not a usable TypeScript name"));
                    }
                    if !seen_ts.insert(ts_name.clone()) {
                        return err(v.line, format!("two variants of `{name}` have the TypeScript name \"{ts_name}\""));
                    }
                    let payload = match &v.payload {
                        Some(t) => Some(resolver.ty(t, v.line, &mut uses_sym, &mut columns_used)?),
                        None => None,
                    };
                    out_variants.push(Variant { name: v.name.clone(), ts_name, payload, doc: v.doc.clone() });
                }
                defs.push(Def::Enum(EnumDef {
                    name: name.clone(),
                    tag,
                    variants: out_variants,
                    doc: doc.clone(),
                    unit_only: !any_payload,
                    recursive: false,
                    min_bits: 0,
                }));
            }
            AstItem::Table { .. } => {}
        }
    }

    // tables: the symbol table first when used; the numbering of `Ty::Ref` assumed `i + 1`
    let mut tables = Vec::new();
    let mut shift = 0usize;
    if !uses_sym {
        shift = 1;
    }
    // entries of tables may use `sym` too: resolve them now (they are structs of `defs`)
    for (name, entry, line) in &table_decls {
        let Some(idx) = resolver.def_index(entry) else {
            return err(*line, format!("table `{name}`: unknown struct `{entry}`"));
        };
        match &defs[idx] {
            Def::Struct(s) if !s.message => {}
            _ => return err(*line, format!("table `{name}`: `{entry}` must be a plain struct")),
        }
        tables.push(Table { name: (*name).clone(), entry: Some(idx), min_bits: 0 });
    }
    if uses_sym {
        tables.insert(0, Table { name: "sym".to_string(), entry: None, min_bits: 8 });
    }
    if tables.len() > MAX_TABLES {
        return err(0, format!("{} tables; the runtime keeps {} bounds", tables.len(), MAX_TABLES));
    }
    if shift == 1 {
        for d in &mut defs {
            match d {
                Def::Struct(s) => {
                    for f in &mut s.fields {
                        shift_refs(&mut f.ty);
                    }
                }
                Def::Enum(e) => {
                    for v in &mut e.variants {
                        if let Some(t) = &mut v.payload {
                            shift_refs(t);
                        }
                    }
                }
            }
        }
    }

    // structural checks that need every type resolved
    for d in &defs {
        match d {
            Def::Struct(s) => {
                for f in &s.fields {
                    check_ty(&defs, &f.ty, &format!("field `{}` of `{}`", f.name, s.name))?;
                }
            }
            Def::Enum(e) => {
                for v in &e.variants {
                    if let Some(t) = &v.payload {
                        check_ty(&defs, t, &format!("variant `{}` of `{}`", v.name, e.name))?;
                        if let Ty::Named(i) = t
                            && let Def::Struct(s) = &defs[*i]
                            && s.fields.iter().any(|f| f.ts_name == e.tag)
                            && !e.unit_only
                        {
                            return err(
                                0,
                                format!(
                                    "enum `{}`: the tag property `{}` is also a field of `{}`, which the TypeScript form flattens",
                                    e.name, e.tag, s.name
                                ),
                            );
                        }
                    }
                }
            }
        }
    }
    for i in columns_used {
        match &mut defs[i] {
            Def::Struct(s) if !s.message => s.columns_used = true,
            _ => return err(0, "`columns<>` needs a plain struct"),
        }
    }
    let used_columns: Vec<String> = defs
        .iter()
        .filter_map(|d| if let Def::Struct(s) = d { s.columns_used.then(|| format!("{}Columns", s.name)) } else { None })
        .collect();
    for u in &used_columns {
        if defs.iter().any(|d| d.name() == u) {
            return err(0, format!("`{u}` clashes with the columns type generated for `{}`", u.trim_end_matches("Columns")));
        }
    }

    cycles(&defs)?;
    recursion_flags(&mut defs);
    let mut schema = Schema { name: ast.name.clone(), magic, hash: [0; 8], canonical: String::new(), tables, defs, messages: Vec::new() };
    schema.messages = (0..schema.defs.len()).filter(|i| matches!(&schema.defs[*i], Def::Struct(s) if s.message)).collect();
    compute_min_bits(&mut schema);
    schema.canonical = canonical(&schema);
    let digest = sha256(schema.canonical.as_bytes());
    schema.hash.copy_from_slice(&digest[..8]);
    Ok(schema)
}

fn shift_refs(t: &mut Ty) {
    match t {
        Ty::Ref(i) => *i -= 1,
        Ty::List(inner) => shift_refs(inner),
        _ => {}
    }
}

fn check_ty(defs: &[Def], ty: &Ty, what: &str) -> Result<(), Error> {
    match ty {
        Ty::Named(i) => match &defs[*i] {
            Def::Struct(s) if s.message => err(0, format!("{what}: `{}` is a message and cannot be nested", s.name)),
            _ => Ok(()),
        },
        Ty::List(inner) => check_ty(defs, inner, what),
        _ => Ok(()),
    }
}

/// Edges that put one type inside another without an indirection (a `Vec`): a cycle of them is a type of infinite size.
fn direct_edges(d: &Def) -> Vec<usize> {
    let mut out = Vec::new();
    let mut add = |t: &Ty| {
        if let Ty::Named(i) = t {
            out.push(*i);
        }
    };
    match d {
        Def::Struct(s) => s.fields.iter().for_each(|f| add(&f.ty)),
        Def::Enum(e) => e.variants.iter().filter_map(|v| v.payload.as_ref()).for_each(&mut add),
    }
    out
}

fn all_edges(d: &Def) -> Vec<usize> {
    fn walk(t: &Ty, out: &mut Vec<usize>) {
        match t {
            Ty::Named(i) | Ty::Columns(i) => out.push(*i),
            Ty::List(inner) => walk(inner, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    match d {
        Def::Struct(s) => s.fields.iter().for_each(|f| walk(&f.ty, &mut out)),
        Def::Enum(e) => e.variants.iter().filter_map(|v| v.payload.as_ref()).for_each(|t| walk(t, &mut out)),
    }
    out
}

fn reaches(defs: &[Def], from: usize, target: usize, edges: fn(&Def) -> Vec<usize>) -> bool {
    let mut stack = edges(&defs[from]);
    let mut seen = vec![false; defs.len()];
    while let Some(n) = stack.pop() {
        if n == target {
            return true;
        }
        if !seen[n] {
            seen[n] = true;
            stack.extend(edges(&defs[n]));
        }
    }
    false
}

fn cycles(defs: &[Def]) -> Result<(), Error> {
    for i in 0..defs.len() {
        if reaches(defs, i, i, direct_edges) {
            return err(0, format!("type `{}` contains itself without a list: recursion is only allowed through `list<>`", defs[i].name()));
        }
    }
    Ok(())
}

fn recursion_flags(defs: &mut [Def]) {
    let flags: Vec<bool> = (0..defs.len()).map(|i| reaches(defs, i, i, all_edges)).collect();
    for (d, f) in defs.iter_mut().zip(flags) {
        match d {
            Def::Struct(s) => s.recursive = f,
            Def::Enum(e) => e.recursive = f,
        }
    }
}

fn compute_min_bits(schema: &mut Schema) {
    // the direct-containment graph is acyclic: iterate to a fixpoint (at most `defs.len()` rounds)
    for _ in 0..=schema.defs.len() {
        for i in 0..schema.defs.len() {
            let (mb, cmb) = match &schema.defs[i] {
                Def::Struct(s) => {
                    let bitmap = (s.bit_fields() as u64).div_ceil(8) * 8;
                    let plain: u64 = s.fields.iter().filter(|f| !f.is_bit()).map(|f| schema.min_bits(&f.ty)).sum();
                    let cols: u64 = s.fields.iter().map(|f| if f.is_bit() { 1 } else { schema.min_bits(&f.ty) }).sum();
                    (bitmap + plain, cols)
                }
                Def::Enum(e) => {
                    let least = e.variants.iter().map(|v| v.payload.as_ref().map_or(0, |t| schema.min_bits(t))).min().unwrap_or(0);
                    (8 + least, 0)
                }
            };
            match &mut schema.defs[i] {
                Def::Struct(s) => {
                    s.min_bits = mb;
                    s.columns_min_bits = cmb;
                }
                Def::Enum(e) => e.min_bits = mb,
            }
        }
    }
    for t in 0..schema.tables.len() {
        if let Some(e) = schema.tables[t].entry {
            schema.tables[t].min_bits = schema.min_bits(&Ty::Named(e));
        }
    }
}

fn ty_text(schema: &Schema, t: &Ty) -> String {
    match t {
        Ty::Bool => "bool".into(),
        Ty::U8 => "u8".into(),
        Ty::U16 => "u16".into(),
        Ty::U32 => "u32".into(),
        Ty::I32 => "i32".into(),
        Ty::F64 => "f64".into(),
        Ty::Utf16 => "utf16".into(),
        Ty::Sym => "sym".into(),
        Ty::Bytes => "bytes".into(),
        Ty::BytesRef => "bytes_ref".into(),
        Ty::Ref(i) => format!("ref<{}>", schema.tables[*i].name),
        Ty::Named(i) => schema.defs[*i].name().to_string(),
        Ty::List(inner) => format!("list<{}>", ty_text(schema, inner)),
        Ty::Columns(i) => format!("columns<{}>", schema.defs[*i].name()),
    }
}

/// The text the hash is taken over: what changes the bytes on the wire (names, types, order),
/// not the comments, the TypeScript names or the formatting of the source.
fn canonical(schema: &Schema) -> String {
    let mut s = format!("uos1 schema {} magic {}\n", schema.name, String::from_utf8_lossy(&schema.magic));
    for t in &schema.tables {
        match t.entry {
            Some(e) => s.push_str(&format!("table {} {}\n", t.name, schema.defs[e].name())),
            None => s.push_str("table sym utf16\n"),
        }
    }
    for d in &schema.defs {
        match d {
            Def::Struct(st) => {
                s.push_str(if st.message { "message " } else { "struct " });
                s.push_str(&st.name);
                s.push_str(" {");
                for f in &st.fields {
                    s.push_str(&format!(" {}:{}{};", f.name, ty_text(schema, &f.ty), if f.optional { "?" } else { "" }));
                }
                s.push_str(" }\n");
            }
            Def::Enum(e) => {
                s.push_str(&format!("enum {} {{", e.name));
                for v in &e.variants {
                    match &v.payload {
                        Some(t) => s.push_str(&format!(" {}({});", v.name, ty_text(schema, t))),
                        None => s.push_str(&format!(" {};", v.name)),
                    }
                }
                s.push_str(" }\n");
            }
        }
    }
    s
}
