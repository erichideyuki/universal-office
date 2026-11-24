//! The store facet (record `uo-calc.md` "Parity"): the canonical JSON of every cell, byte-equal to what the TS
//! adapter (`js/capture-store.mjs`, the `storeOf` of uo-xlsx `capture-sheet.mjs`) prints from the sheet-core
//! model: keys sorted by UTF-16 code unit, `String(n)` numbers with `-0` and `{"$num":"NaN"}`, `JSON.stringify`
//! strings (lone surrogates as `\udXXX`), strings over 4096 units as `{"$text":n,"sha256":hex}`; line keys
//! `{c, cm, f, fa, is, r, ref, s, t, v, vm}` with 1-based `c`/`r` and `ref` only when not the canonical A1.
//! Feature `facet` (tests, the parity harness of C1a/C1b/A3).

use std::fmt::Write as _;

use crate::cell::{FileCell, InlineText};
use crate::formula::FormulaAttrs;
use crate::key::{Area, MAX_COL, MAX_ROW, SheetIx};
use crate::load::FileRow;
use crate::store::{RowMeta, Store};
use crate::strings::{JsStr, JsString, StrPool};
use crate::value::{Scalar, StrRef};

const LONG_TEXT: usize = 4096;

/// Appends the canonical JSON of a number.
pub fn push_num(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str(r#"{"$num":"NaN"}"#);
    } else if x == f64::INFINITY {
        out.push_str(r#"{"$num":"Infinity"}"#);
    } else if x == f64::NEG_INFINITY {
        out.push_str(r#"{"$num":"-Infinity"}"#);
    } else if x == 0.0 && x.is_sign_negative() {
        out.push_str("-0");
    } else {
        out.push_str(&uo_jsnum::format::to_js_string(x));
    }
}

/// Appends the canonical JSON of a JS string.
pub fn push_str(out: &mut String, s: JsStr<'_>) {
    if s.len() > LONG_TEXT {
        let digest = uo_testkit::sha256::sha256(s.to_string_lossy().as_bytes());
        let _ = write!(out, r#"{{"$text":{},"sha256":"{}"}}"#, s.len(), uo_testkit::stream::hex(&digest));
        return;
    }
    match s {
        JsStr::Latin1(b) if b.is_ascii() => uo_jsnum::json::push_string(out, core::str::from_utf8(b).unwrap_or("")),
        _ => uo_jsnum::json::push_string_utf16(out, &s.to_utf16()),
    }
}

struct Obj<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> Obj<'a> {
    fn new(out: &'a mut String) -> Self {
        out.push('{');
        Self { out, first: true }
    }
    fn key(&mut self, k: &str) -> &mut String {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        uo_jsnum::json::push_string(self.out, k);
        self.out.push(':');
        self.out
    }
    fn num(&mut self, k: &str, x: Option<f64>) {
        if let Some(x) = x {
            push_num(self.key(k), x);
        }
    }
    fn js(&mut self, k: &str, s: Option<&JsString>) {
        if let Some(s) = s {
            push_str(self.key(k), s.as_js());
        }
    }
    fn bool(&mut self, k: &str, on: bool) {
        if on {
            self.key(k).push_str("true");
        }
    }
    fn end(self) {
        self.out.push('}');
    }
}

fn push_fa(out: &mut String, fa: &FormulaAttrs) {
    let mut o = Obj::new(out);
    for (bit, name) in FormulaAttrs::FLAG_NAMES {
        o.bool(name, fa.flags & bit != 0);
    }
    o.js("r1", fa.r1.as_ref());
    o.js("r2", fa.r2.as_ref());
    o.js("ref", fa.r#ref.as_ref());
    o.num("si", fa.si);
    if let Some(t) = fa.t {
        uo_jsnum::json::push_string(o.key("t"), t.text());
    }
    o.end();
}

fn push_is(out: &mut String, is: &InlineText, props: &dyn Fn(u32) -> String) {
    let mut o = Obj::new(out);
    if let Some(runs) = &is.runs {
        let out = o.key("runs");
        out.push('[');
        for (i, r) in runs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let mut ro = Obj::new(out);
            if let Some(p) = r.props {
                ro.key("props").push_str(&props(p));
            }
            if let Some(rpr) = &r.rpr {
                let mut attrs: Vec<&(JsString, JsString)> = rpr.iter().collect();
                attrs.sort_by(|a, b| a.0.as_js().units().cmp(b.0.as_js().units()));
                let out = ro.key("rPr");
                out.push('{');
                for (j, (k, v)) in attrs.into_iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    // keys are JSON.stringify'd as is (never long)
                    uo_jsnum::json::push_string_utf16(out, &k.as_js().to_utf16());
                    out.push(':');
                    push_str(out, v.as_js());
                }
                out.push('}');
            }
            push_str(ro.key("text"), r.text.as_js());
            ro.end();
        }
        out.push(']');
    }
    o.js("text", is.text.as_ref());
    o.end();
}

/// The facet line of a model cell at 0-based `(row, col)`.
pub fn cell_line(out: &mut String, row: u32, col: u32, cell: &FileCell, props: &dyn Fn(u32) -> String) {
    let mut o = Obj::new(out);
    push_num(o.key("c"), f64::from(col + 1));
    o.num("cm", cell.cm);
    o.js("f", cell.f.as_ref());
    if let Some(fa) = &cell.fa {
        push_fa(o.key("fa"), fa);
    }
    if let Some(is) = &cell.is {
        push_is(o.key("is"), is, props);
    }
    push_num(o.key("r"), f64::from(row) + 1.0);
    if let Some(r) = &cell.r#ref
        && !r.as_js().eq_str(&crate::cell::a1_ref(row, col))
    {
        push_str(o.key("ref"), r.as_js());
    }
    o.num("s", cell.s);
    o.js("t", cell.t.as_ref());
    o.js("v", cell.v.as_ref());
    o.num("vm", cell.vm);
    o.end();
}

fn whole(sheet: SheetIx) -> Area {
    Area { sheet, r1: 0, c1: 0, r2: MAX_ROW, c2: MAX_COL }
}

/// The store facet of a sheet: one line per cell, sorted by (row, col).
#[must_use]
pub fn store_facet(store: &Store, sheet: SheetIx, props: &dyn Fn(u32) -> String) -> Vec<String> {
    let mut keys = Vec::new();
    store.each(whole(sheet), |r, c, _| keys.push((r, c)));
    keys.into_iter()
        .filter_map(|(r, c)| {
            let cell = store.file_cell(sheet, r, c)?;
            let mut s = String::new();
            cell_line(&mut s, r, c, &cell, props);
            Some(s)
        })
        .collect()
}

/// The `<c>` sequence in file order as 1-based `(row, col)` (the `fo` facet: the TS writes `sorted` when the
/// keys strictly increase, else the digest of `row:col` lines).
#[must_use]
pub fn file_order(store: &Store, sheet: SheetIx) -> Vec<(u32, u32)> {
    let Some(sh) = store.sheet(sheet) else { return Vec::new() };
    match &sh.file_order {
        Some(fo) => fo
            .rows
            .iter()
            .flat_map(|fr| {
                fr.cells.iter().map(move |&item| {
                    let col = if item & FileRow::SHADOW != 0 {
                        fo.shadowed.get((item & !FileRow::SHADOW) as usize).map_or(0, |s| s.0)
                    } else {
                        item
                    };
                    (fr.row + 1, col + 1)
                })
            })
            .collect(),
        None => {
            let mut v = Vec::new();
            store.each(whole(sheet), |r, c, _| v.push((r + 1, c + 1)));
            v
        }
    }
}

/// The `rw` facet lines: row attributes in file order (`{customFormat, customHeight, hidden, ht, r, s, spans,
/// thickBot, thickTop}`), rows without attributes skipped.
#[must_use]
pub fn row_meta_lines(store: &Store, sheet: SheetIx) -> Vec<String> {
    let Some(sh) = store.sheet(sheet) else { return Vec::new() };
    let metas: Vec<(u32, RowMeta)> = match &sh.file_order {
        Some(fo) => fo.rows.iter().filter_map(|fr| fr.meta.map(|m| (fr.row, m))).collect(),
        None => sh.rows.clone(),
    };
    metas
        .into_iter()
        .filter(|(_, m)| m.has_attrs())
        .map(|(row, m)| {
            let mut s = String::new();
            let mut o = Obj::new(&mut s);
            o.bool("customFormat", m.flags & RowMeta::CUSTOM_FORMAT != 0);
            o.bool("customHeight", m.flags & RowMeta::CUSTOM_HEIGHT != 0);
            o.bool("hidden", m.flags & RowMeta::HIDDEN != 0);
            o.num("ht", m.height);
            push_num(o.key("r"), f64::from(row) + 1.0);
            o.num("s", m.style);
            if let Some(id) = m.spans {
                push_str(o.key("spans"), store.strings.get(id));
            }
            o.bool("thickBot", m.flags & RowMeta::THICK_BOT != 0);
            o.bool("thickTop", m.flags & RowMeta::THICK_TOP != 0);
            o.end();
            s
        })
        .collect()
}

/// The canonical value text of the calc captures (`makeValueText`, `js/lib/common.mjs`): `_`, `n<f64 hex>`,
/// `s<JSON>`, `b1`/`b0`, `e<code>`.
#[must_use]
pub fn value_text(pool: &StrPool, v: &Scalar) -> String {
    match v {
        Scalar::Empty => "_".to_owned(),
        Scalar::Num(x) => format!("n{:016x}", x.to_bits()),
        Scalar::Str(StrRef::Pooled(id)) => {
            let mut s = "s".to_owned();
            uo_jsnum::json::push_string_utf16(&mut s, &pool.get(*id).to_utf16());
            s
        }
        Scalar::Str(StrRef::Temp(i)) => format!("?temp{i}"),
        Scalar::Str(StrRef::Lit(i)) => format!("?lit{i}"),
        Scalar::Bool(b) => (if *b { "b1" } else { "b0" }).to_owned(),
        Scalar::Err(e) => format!("e{}", crate::cell::error_text(*e)),
    }
}
