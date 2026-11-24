//! Test support of the S0 store: the capture sidecars (gzip), a small JSON reader that keeps JS strings as
//! UTF-16 (lone surrogates included), the test-only loader that feeds the store from the sheet-core model
//! lines of `js/capture-store.mjs`, and the digests of the capture facets.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod calcwb;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_calc::cell::{FileCell, InlineText};
use uo_calc::formula::{FormulaAttrs, FormulaType};
use uo_calc::store::{RichRun, RowMeta, SstDecoded, SstSource};
use uo_calc::{JsString, SheetIx, Store};

pub fn captures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

pub fn xlsx_captures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../uo-xlsx/tests/captures")
}

pub fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib");
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    String::from_utf8(raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Sidecar lines `<key>\t<facet>\t<text>` grouped by key then facet, in file order.
pub fn sidecar(path: &Path) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let text = gunzip(path);
    let mut out: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for line in text.lines() {
        let mut it = line.splitn(3, '\t');
        let (Some(k), Some(f), Some(t)) = (it.next(), it.next(), it.next()) else { continue };
        out.entry(k.to_owned()).or_default().entry(f.to_owned()).or_default().push(t.to_owned());
    }
    out
}

/// `count:sha256[0..32]` of lines joined by `\n` (the `listDig` of the capture scripts).
pub fn list_digest(lines: &[String]) -> String {
    let mut h = uo_testkit::sha256::Sha256::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            h.update(b"\n");
        }
        h.update(l.as_bytes());
    }
    format!("{}:{}", lines.len(), &uo_testkit::stream::hex(&h.finish())[..32])
}

// ---------------------------------------------------------------- JSON with JS strings

#[derive(Clone, PartialEq, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(Vec<u16>),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            J::Num(n) => Some(*n),
            J::Obj(_) => match self.get("$num").and_then(J::js) {
                Some(s) if s.as_js().eq_str("NaN") => Some(f64::NAN),
                Some(s) if s.as_js().eq_str("Infinity") => Some(f64::INFINITY),
                Some(s) if s.as_js().eq_str("-Infinity") => Some(f64::NEG_INFINITY),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn js(&self) -> Option<JsString> {
        match self {
            J::Str(u) => Some(JsString::from_utf16(u)),
            _ => None,
        }
    }
    pub fn bool(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => &[],
        }
    }
}

pub fn parse(text: &str) -> J {
    let mut p = P { b: text.as_bytes(), i: 0 };
    let v = p.value();
    assert_eq!(p.i, p.b.len(), "trailing JSON in {text}");
    v
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut o = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(o);
                }
                loop {
                    self.ws();
                    let k = String::from_utf16_lossy(&self.string());
                    self.ws();
                    assert_eq!(self.b[self.i], b':');
                    self.i += 1;
                    let v = self.value();
                    o.push((k, v));
                    self.ws();
                    match self.b[self.i] {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return J::Obj(o);
                        }
                        c => panic!("bad JSON object at {} ({})", self.i, c as char),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(a);
                }
                loop {
                    a.push(self.value());
                    self.ws();
                    match self.b[self.i] {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return J::Arr(a);
                        }
                        c => panic!("bad JSON array at {} ({})", self.i, c as char),
                    }
                }
            }
            b'"' => J::Str(self.string()),
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let s = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[s..self.i]).unwrap();
                J::Num(uo_jsnum::parse::to_number(t))
            }
        }
    }
    fn string(&mut self) -> Vec<u16> {
        assert_eq!(self.b[self.i], b'"');
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = self.b[self.i];
            match c {
                b'"' => {
                    self.i += 1;
                    return out;
                }
                b'\\' => {
                    let e = self.b[self.i + 1];
                    self.i += 2;
                    match e {
                        b'"' => out.push(0x22),
                        b'\\' => out.push(0x5C),
                        b'/' => out.push(0x2F),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                            out.push(u16::from_str_radix(h, 16).unwrap());
                            self.i += 4;
                        }
                        _ => panic!("bad escape"),
                    }
                }
                _ => {
                    // one UTF-8 character
                    let len = match c {
                        0..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let s = std::str::from_utf8(&self.b[self.i..self.i + len]).unwrap();
                    out.extend(s.encode_utf16());
                    self.i += len;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the test-only loader

/// Rich-run `props` blobs: the store keeps a host id, the test keeps the canonical JSON text.
#[derive(Default)]
pub struct Props(pub Vec<String>);

impl Props {
    pub fn text(&self, id: u32) -> String {
        self.0[id as usize].clone()
    }
}

fn canon(j: &J) -> String {
    let mut s = String::new();
    canon_into(&mut s, j);
    s
}

fn canon_into(out: &mut String, j: &J) {
    match j {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(x) => uo_calc::facet::push_num(out, *x),
        J::Str(u) => uo_jsnum::json::push_string_utf16(out, u),
        J::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canon_into(out, x);
            }
            out.push(']');
        }
        J::Obj(o) => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                uo_jsnum::json::push_string(out, k);
                out.push(':');
                canon_into(out, v);
            }
            out.push('}');
        }
    }
}

fn inline_of(j: &J, props: &mut Props) -> InlineText {
    InlineText {
        text: j.get("text").and_then(J::js),
        runs: j.get("runs").map(|runs| {
            runs.arr()
                .iter()
                .map(|r| RichRun {
                    text: r.get("text").and_then(J::js).unwrap_or_default(),
                    rpr: r.get("rPr").map(|a| match a {
                        J::Obj(o) => o.iter().map(|(k, v)| (JsString::from_str_js(k), v.js().unwrap_or_default())).collect(),
                        _ => Box::default(),
                    }),
                    props: r.get("props").map(|p| {
                        props.0.push(canon(p));
                        (props.0.len() - 1) as u32
                    }),
                })
                .collect()
        }),
    }
}

/// A model cell (`capture-store.mjs` `C` line JSON, or a store-facet line) → [`FileCell`].
pub fn file_cell(j: &J, props: &mut Props) -> FileCell {
    FileCell {
        r#ref: j.get("ref").and_then(J::js),
        t: j.get("t").and_then(J::js),
        v: j.get("v").and_then(J::js),
        s: j.get("s").and_then(J::num),
        f: j.get("f").and_then(J::js),
        fa: j.get("fa").map(|fa| {
            let mut flags = 0;
            for (bit, name) in FormulaAttrs::FLAG_NAMES {
                if fa.get(name).is_some_and(J::bool) {
                    flags |= bit;
                }
            }
            FormulaAttrs {
                t: fa.get("t").and_then(J::js).map(|t| match String::from_utf16_lossy(&t.as_js().to_utf16()).as_str() {
                    "normal" => FormulaType::Normal,
                    "shared" => FormulaType::Shared,
                    "array" => FormulaType::Array,
                    _ => FormulaType::DataTable,
                }),
                r#ref: fa.get("ref").and_then(J::js),
                si: fa.get("si").and_then(J::num),
                flags,
                r1: fa.get("r1").and_then(J::js),
                r2: fa.get("r2").and_then(J::js),
            }
        }),
        cm: j.get("cm").and_then(J::num),
        vm: j.get("vm").and_then(J::num),
        is: j.get("is").map(|is| inline_of(is, props)),
    }
}

/// A `R` line's JSON → row meta (interning `spans`).
pub fn row_meta(store: &mut Store, j: &J) -> RowMeta {
    let mut flags = 0;
    for (k, bit) in [
        ("customHeight", RowMeta::CUSTOM_HEIGHT),
        ("hidden", RowMeta::HIDDEN),
        ("customFormat", RowMeta::CUSTOM_FORMAT),
        ("thickTop", RowMeta::THICK_TOP),
        ("thickBot", RowMeta::THICK_BOT),
    ] {
        if j.get(k).is_some_and(J::bool) {
            flags |= bit;
        }
    }
    RowMeta {
        height: j.get("ht").and_then(J::num),
        style: j.get("s").and_then(J::num),
        spans: j.get("spans").and_then(J::js).map(|s| store.strings.intern(s.as_js()).unwrap()),
        flags,
    }
}

/// Streams the `in<i>` lines of one sheet into a new sheet of `store` (file order, as a reader would).
pub fn load_sheet(store: &mut Store, lines: &[String], props: &mut Props) -> SheetIx {
    let sheet = store.add_sheet().unwrap();
    let mut metas = Vec::new();
    let mut cells = Vec::new();
    for l in lines {
        if let Some(r) = l.strip_prefix('R') {
            metas.push(parse(r));
        } else if let Some(c) = l.strip_prefix('C') {
            let (col, json) = c.split_once(' ').unwrap();
            cells.push((metas.len() - 1, col.parse::<i64>().unwrap(), parse(json)));
        }
    }
    let mut row_metas = Vec::new();
    for m in &metas {
        row_metas.push(row_meta(store, m));
    }
    let mut loader = store.load_sheet(sheet).unwrap();
    let mut ci = 0;
    for (ri, (m, meta)) in metas.iter().zip(row_metas).enumerate() {
        let r = m.get("r").and_then(J::num).unwrap() as u32;
        loader.row(r - 1, meta).unwrap();
        while ci < cells.len() && cells[ci].0 == ri {
            let (_, col, j) = &cells[ci];
            loader.cell((*col - 1) as u32, &file_cell(j, props)).unwrap();
            ci += 1;
        }
    }
    loader.finish().unwrap();
    sheet
}

/// The shared strings of a workbook as a lazy source (each `sst` sidecar line is a JSON string).
pub struct Sst(pub Vec<JsString>);

impl SstSource for Sst {
    fn decode(&mut self, index: u32) -> Option<SstDecoded> {
        self.0.get(index as usize).map(|t| SstDecoded { text: t.clone(), runs: None })
    }
}

pub fn sst_of(lines: &[String]) -> Sst {
    Sst(lines.iter().map(|l| parse(l).js().unwrap()).collect())
}
