//! Parity of uo-docx-layout frontier F2 (the Word semantics in `uo_docx::word`; record
//! `core/docs/modules/uo-docx-layout.md`, "Wave 1 — F2"): replays `tests/parity/word.capture`,
//! written by `js/capture-word.mjs` from the frozen TS over seeded generated inputs
//! (`tests/parity/word.inputs`), and compares exactly: canonical digests of the values and of
//! their key order (A6, K9/LQ20) and the JSON text of strings.
//!
//! Inputs are OOXML read by the D1a readers (R1, R2) on both sides; the facet `in` is the digest
//! of what was read, so a reader difference shows there and the cases built on it are reported
//! as the reader's, not this module's.
//!
//! TS throws accepted by name (K11, LQ14): `formatNumber` of a fraction or NaN in `chicago`
//! (TypeError) and in `decimalEnclosedCircle` (RangeError), and of ±Infinity or a counter past the
//! string limit in the alphabetic and chicago formats (`repeat`, RangeError): Rust returns a text
//! (capped by `TextCap`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of the frozen TS> node core/crates/uo-docx/js/capture-word.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::collections::HashMap;
use std::sync::Arc;

use support::Sink;
use uo_docx::canon::{CanonWriter, EntryRef, canon_entries};
use uo_docx::js::{JsNumber, JsObject, JsRecord, JsValue};
use uo_docx::model::Paragraph;
use uo_docx::model::effects::{FxColor, FxFill, FxOutline, FxShadow, OpenTypeProps, TextEffects};
use uo_docx::model::formatting::DocFormatting;
use uo_docx::model::para::{BorderEdge, NumberingProps, ParaFormatChange, ParaProps, ParaTabStop, ParagraphBorders};
use uo_docx::model::props::{PPr, RPr, RevisionInfo};
use uo_docx::model::run::{FormatChange, NoteRef, Run, RunProps, ThemeColorAttrs};
use uo_docx::read::DOCX;
use uo_docx::read::parts::{read_numbering_text, read_settings, read_style_sheet_text, read_theme_text};
use uo_docx::read::props::{read_ppr, read_rpr, read_tbl_pr, read_tc_pr, read_tr_pr};
use uo_docx::read::props_map::{para_props_from_ppr, run_props_from_rpr};
use uo_docx::text::Utf16;
use uo_docx::word::obj::{set, set_undefined};
use uo_docx::word::{self, LocalDateTime, RevisionDisplay, RevisionMode, StyleResolver, TextCap};
use uo_docx::{DocxLimits, read::plain::default_run_props};
use uo_opc::xml::parse;
use uo_testkit::{Capture, Replay, Stream};

// ───────────── inputs ─────────────

fn dec(f: &str) -> Option<Vec<u16>> {
    let s = f.strip_prefix('s')?;
    let mut out: Vec<u16> = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if b[i] == b'%' {
            if b.get(i + 1) == Some(&b'u') {
                out.push(u16::from_str_radix(&s[i + 2..i + 6], 16).unwrap());
                i += 6;
            } else {
                out.push(u16::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            }
        } else {
            let ch = s[i..].chars().next().unwrap();
            let mut buf = [0u16; 2];
            out.extend_from_slice(ch.encode_utf16(&mut buf));
            i += ch.len_utf8();
        }
    }
    Some(out)
}

fn dec_str(f: &str) -> Option<String> {
    dec(f).map(|u| String::from_utf16(&u).expect("well-formed"))
}

fn num(f: &str) -> Option<JsNumber> {
    match f {
        "-" => None,
        "null" => Some(JsNumber::Null),
        "NaN" => Some(JsNumber::Num(f64::NAN)),
        "Infinity" => Some(JsNumber::Num(f64::INFINITY)),
        "-Infinity" => Some(JsNumber::Num(f64::NEG_INFINITY)),
        "-0" => Some(JsNumber::Num(-0.0)),
        _ => Some(JsNumber::Num(uo_jsnum::parse::to_number(f))),
    }
}

/// `tests/parity/word` (`.capture`, `.inputs`), or `UO_WORD_CAPTURE=<path without extension>` (a
/// larger local capture: `capture-word.mjs --count 3000 --fmt-count 300 --seed 11 --out <path>`).
fn base() -> std::path::PathBuf {
    std::env::var_os("UO_WORD_CAPTURE")
        .map_or_else(|| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/word")), std::path::PathBuf::from)
}

fn inputs() -> HashMap<String, Vec<String>> {
    let text = std::fs::read_to_string(base().with_extension("inputs")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let k = it.next().unwrap().to_owned();
            (k, it.map(str::to_owned).collect())
        })
        .collect()
}

// a minimal JSON reader for the edit scripts ({"$u":1} = undefined, {"$n":"NaN"} = NaN)
#[derive(Debug, Clone)]
enum J {
    Null,
    Undef,
    Bool(bool),
    Num(f64),
    Str(Vec<u16>),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

fn json(s: &str) -> J {
    let b = s.as_bytes();
    let mut i = 0;
    parse_j(b, &mut i)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | 9 | 10 | 13) {
        *i += 1;
    }
}

fn parse_j(b: &[u8], i: &mut usize) -> J {
    ws(b, i);
    match b[*i] {
        b'{' => {
            *i += 1;
            let mut kv = Vec::new();
            loop {
                ws(b, i);
                if b[*i] == b'}' {
                    *i += 1;
                    break;
                }
                let J::Str(k) = parse_j(b, i) else { panic!() };
                let k = String::from_utf16_lossy(&k);
                ws(b, i);
                *i += 1; // :
                let v = parse_j(b, i);
                kv.push((k, v));
                ws(b, i);
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            match kv.as_slice() {
                [(k, _)] if k == "$u" => J::Undef,
                [(k, J::Str(s))] if k == "$n" => J::Num(uo_jsnum::parse::to_number(&String::from_utf16_lossy(s))),
                _ => J::Obj(kv),
            }
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(b, i);
                if b[*i] == b']' {
                    *i += 1;
                    break;
                }
                v.push(parse_j(b, i));
                ws(b, i);
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            J::Arr(v)
        }
        b'"' => {
            *i += 1;
            let mut u: Vec<u16> = Vec::new();
            loop {
                let c = b[*i];
                if c == b'"' {
                    *i += 1;
                    break;
                }
                if c == b'\\' {
                    let e = b[*i + 1];
                    *i += 2;
                    match e {
                        b'u' => {
                            u.push(u16::from_str_radix(std::str::from_utf8(&b[*i..*i + 4]).unwrap(), 16).unwrap());
                            *i += 4;
                        }
                        b'n' => u.push(10),
                        b't' => u.push(9),
                        b'r' => u.push(13),
                        b'b' => u.push(8),
                        b'f' => u.push(12),
                        other => u.push(u16::from(other)),
                    }
                } else {
                    let s = std::str::from_utf8(&b[*i..]).unwrap();
                    let ch = s.chars().next().unwrap();
                    let mut buf = [0u16; 2];
                    u.extend_from_slice(ch.encode_utf16(&mut buf));
                    *i += ch.len_utf8();
                }
            }
            J::Str(u)
        }
        b't' => {
            *i += 4;
            J::Bool(true)
        }
        b'f' => {
            *i += 5;
            J::Bool(false)
        }
        b'n' => {
            *i += 4;
            J::Null
        }
        _ => {
            let st = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *i += 1;
            }
            J::Num(uo_jsnum::parse::to_number(std::str::from_utf8(&b[st..*i]).unwrap()))
        }
    }
}

impl J {
    fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(kv) => kv.iter().find(|(a, _)| a == k).map(|(_, v)| v),
            _ => None,
        }
    }
    fn s(&self) -> Option<Box<str>> {
        match self {
            J::Str(s) => Some(String::from_utf16_lossy(s).into()),
            _ => None,
        }
    }
    fn n(&self) -> Option<JsNumber> {
        match self {
            J::Num(x) => Some(JsNumber::Num(*x)),
            J::Null => Some(JsNumber::Null),
            _ => None,
        }
    }
    fn b(&self) -> Option<bool> {
        match self {
            J::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => &[],
        }
    }
}

// ───────────── digests ─────────────

fn digest(f: impl FnOnce(&mut CanonWriter<'_>)) -> (String, String) {
    let (mut s, mut o) = (Sink(Stream::new()), Sink(Stream::new()));
    {
        let mut w = CanonWriter::new(&mut s).with_order(&mut o);
        f(&mut w);
    }
    let v = |x: Sink| {
        let n = x.0.items();
        format!("{n}:{}", x.0.finish().short())
    };
    (v(s), v(o))
}

fn check_value(r: &mut Replay<'_>, key: &str, facet: &str, f: impl FnOnce(&mut CanonWriter<'_>)) -> bool {
    let (d, o) = digest(f);
    let a = r.check(key, facet, &d);
    let b = r.check(key, &format!("{facet}.ord"), &o);
    a && b
}

const READER: &str = "the inputs read differently by the D1a readers (R1/R2/R4a): their parity, not F2's";

/// The inputs as read (`in`): a difference is the reader's; the record's other facets are then
/// not compared (marked replayed).
fn check_in(r: &mut Replay<'_>, cap: &Capture, key: &str, f: impl FnOnce(&mut CanonWriter<'_>)) -> bool {
    let (d, o) = digest(f);
    let a = r.accept(key, "in", &d, READER);
    let b = r.accept(key, "in.ord", &o, READER);
    if !(a && b) {
        skip_all(r, cap, key);
    }
    a && b
}

/// Marks every facet of `key` replayed.
fn skip_all(r: &mut Replay<'_>, cap: &Capture, key: &str) {
    if let Some(rec) = cap.record(key) {
        for (f, _) in &rec.facets {
            r.skip(key, f);
        }
    }
}

fn js_str(u: &[u16]) -> String {
    let mut s = String::new();
    uo_jsnum::json::push_string_utf16(&mut s, u);
    s
}

fn opt_str(u: Option<&[u16]>) -> String {
    u.map_or_else(|| "undefined".to_owned(), js_str)
}

fn json_num(x: f64) -> String {
    let mut s = String::new();
    uo_jsnum::json::push_number(&mut s, x);
    s
}

// ───────────── readers ─────────────

fn read_el<T>(xml: Option<&str>, f: impl Fn(Option<uo_opc::xml::Element<'_>>) -> Option<T>) -> Option<T> {
    let xml = xml?;
    let d = parse(xml.to_owned(), &DOCX).unwrap();
    f(Some(d.root()))
}

fn rpr(x: Option<&str>) -> Option<RPr> {
    read_el(x, read_rpr)
}
fn ppr(x: Option<&str>) -> Option<PPr> {
    read_el(x, read_ppr)
}

fn formatting(f: &[String]) -> DocFormatting {
    let styles = dec_str(&f[0]).map_or_else(Default::default, |x| read_style_sheet_text(&x).unwrap());
    let numbering = dec_str(&f[1]).map_or_else(Default::default, |x| read_numbering_text(&x, &DocxLimits::default()).unwrap());
    let theme = dec_str(&f[2]).map_or_else(Default::default, |x| read_theme_text(&x).unwrap());
    DocFormatting { styles, numbering, theme, settings: read_settings(None), font_table: JsRecord::new(), ..DocFormatting::default() }
}

// ───────────── edit scripts ─────────────

fn theme_attrs(j: &J) -> ThemeColorAttrs {
    let mut t = ThemeColorAttrs::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "themeColor" => t.theme_color = v.s().unwrap(),
                "tint" => t.tint = v.s(),
                "shade" => t.shade = v.s(),
                _ => panic!("{k}"),
            }
            t.mark(k);
        }
    }
    t
}

fn fx_color(j: &J) -> FxColor {
    let mut c = FxColor::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "rgb" => c.rgb = v.s(),
                "scheme" => c.scheme = v.s(),
                "mods" => c.mods = Some(v.arr().iter().map(|m| (m.arr()[0].s().unwrap(), m.arr()[1].n().unwrap())).collect()),
                _ => panic!("{k}"),
            }
            c.mark(k);
        }
    }
    c
}

fn fx_fill(j: &J) -> FxFill {
    let mut f = FxFill::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "type" => f.r#type = v.s().unwrap(),
                "color" => f.color = Some(fx_color(v)),
                _ => panic!("{k}"),
            }
            f.mark(k);
        }
    }
    f
}

fn text_fx(j: &J) -> TextEffects {
    let mut t = TextEffects::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "fill" => t.fill = Some(Box::new(fx_fill(v))),
                "outline" => {
                    let mut o = FxOutline::default();
                    if let J::Obj(okv) = v {
                        for (ok, ov) in okv {
                            match ok.as_str() {
                                "w" => o.w = ov.n(),
                                "fill" => o.fill = Some(Box::new(fx_fill(ov))),
                                _ => panic!("{ok}"),
                            }
                            o.mark(ok);
                        }
                    }
                    t.outline = Some(Box::new(o));
                }
                "shadow" => {
                    let g = |n: &str| v.get(n).and_then(J::n).unwrap();
                    t.shadow = Some(Box::new(FxShadow {
                        blur_rad: g("blurRad"),
                        dist: g("dist"),
                        dir: g("dir"),
                        sx: g("sx"),
                        sy: g("sy"),
                        kx: g("kx"),
                        ky: g("ky"),
                        algn: v.get("algn").and_then(J::s).unwrap(),
                        color: fx_color(v.get("color").unwrap()),
                        ..FxShadow::default()
                    }));
                }
                _ => panic!("{k}"),
            }
            t.mark(k);
        }
    }
    t
}

/// `p = { ...p, [k]: v }` for each edit (a new key goes last).
fn edit_run_props(p: &mut RunProps, edits: &J) {
    for e in edits.arr() {
        let k = e.arr()[0].s().unwrap();
        let v = &e.arr()[1];
        let k: &str = &k;
        if matches!(v, J::Undef) {
            set_undefined(p, k, |p| match k {
                "bold" => p.bold = None,
                "italic" => p.italic = None,
                "hidden" => p.hidden = None,
                "doubleStrike" => p.double_strike = None,
                "underlineStyle" => p.underline_style = None,
                "underlineColor" => p.underline_color = None,
                "color" => p.color = None,
                "colorTheme" => p.color_theme = None,
                "underlineTheme" => p.underline_theme = None,
                "highlight" => p.highlight = None,
                "fontAscii" => p.font_ascii = None,
                "szHalfPt" => p.sz_half_pt = None,
                "scalePct" => p.scale_pct = None,
                "positionHalfPt" => p.position_half_pt = None,
                "kernHalfPt" => p.kern_half_pt = None,
                "charSpacingTwips" => p.char_spacing_twips = None,
                "charStyle" => p.char_style = None,
                "vertAlign" => p.vert_align = None,
                "link" => p.link = None,
                "textFx" => p.text_fx = None,
                "openType" => p.open_type = None,
                _ => panic!("{k}"),
            });
            continue;
        }
        set(p, k, |p| match k {
            "bold" => p.bold = v.b(),
            "italic" => p.italic = v.b(),
            "underline" => p.underline = v.b(),
            "strike" => p.strike = v.b(),
            "allCaps" => p.all_caps = v.b(),
            "smallCaps" => p.small_caps = v.b(),
            "hidden" => p.hidden = v.b(),
            "doubleStrike" => p.double_strike = v.b(),
            "underlineStyle" => p.underline_style = v.s(),
            "underlineColor" => p.underline_color = v.s(),
            "color" => p.color = v.s(),
            "colorTheme" => p.color_theme = Some(theme_attrs(v)),
            "underlineTheme" => p.underline_theme = Some(theme_attrs(v)),
            "highlight" => p.highlight = v.s(),
            "fontAscii" => p.font_ascii = v.s(),
            "szHalfPt" => p.sz_half_pt = v.n(),
            "scalePct" => p.scale_pct = v.n(),
            "positionHalfPt" => p.position_half_pt = v.n(),
            "kernHalfPt" => p.kern_half_pt = v.n(),
            "charSpacingTwips" => p.char_spacing_twips = v.n(),
            "charStyle" => p.char_style = v.s(),
            "vertAlign" => p.vert_align = v.s(),
            "link" => p.link = v.s(),
            "textFx" => p.text_fx = Some(Box::new(text_fx(v))),
            "openType" => {
                let mut o = OpenTypeProps::default();
                if let J::Obj(kv) = v {
                    for (ok, ov) in kv {
                        match ok.as_str() {
                            "ligatures" => o.ligatures = ov.s(),
                            "numForm" => o.num_form = ov.s(),
                            _ => panic!("{ok}"),
                        }
                        o.mark(ok);
                    }
                }
                p.open_type = Some(Box::new(o));
            }
            _ => panic!("{k}"),
        });
    }
}

fn border_edge(j: &J) -> BorderEdge {
    let mut e = BorderEdge::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "style" => e.style = v.s().unwrap(),
                "sizeEighthPt" => e.size_eighth_pt = v.n().unwrap(),
                "spacePt" => e.space_pt = v.n().unwrap(),
                "color" => {
                    if matches!(v, J::Undef) {
                        e.mark_undefined("color");
                    } else {
                        e.color = v.s();
                    }
                }
                _ => panic!("{k}"),
            }
            e.mark(k);
        }
    }
    e
}

fn edit_para_props(p: &mut ParaProps, edits: &J) {
    for e in edits.arr() {
        let k = e.arr()[0].s().unwrap();
        let v = &e.arr()[1];
        let k: &str = &k;
        if matches!(v, J::Undef) {
            set_undefined(p, k, |p| match k {
                "alignment" => p.alignment = None,
                "styleId" => p.style_id = None,
                "numbering" => p.numbering = None,
                "indentLeftTwips" => p.indent_left_twips = None,
                "indentRightTwips" => p.indent_right_twips = None,
                "indentFirstLineTwips" => p.indent_first_line_twips = None,
                "lineTwentieths" => p.line_twentieths = None,
                "spaceAfterTwips" => p.space_after_twips = None,
                "spaceBeforeTwips" => p.space_before_twips = None,
                "spaceBeforeAuto" => p.space_before_auto = None,
                "spaceAfterAuto" => p.space_after_auto = None,
                "shadingHex" => p.shading_hex = None,
                "tabStops" => p.tab_stops = None,
                "borders" => p.borders = None,
                "keepNext" => p.keep_next = None,
                "pageBreakBefore" => p.page_break_before = None,
                "widowControl" => p.widow_control = None,
                "textboxTightWrap" => p.textbox_tight_wrap = None,
                _ => panic!("{k}"),
            });
            continue;
        }
        set(p, k, |p| match k {
            "alignment" => p.alignment = v.s(),
            "styleId" => p.style_id = v.s(),
            "numbering" => {
                let mut n = NumberingProps::default();
                if let J::Obj(kv) = v {
                    for (nk, nv) in kv {
                        match nk.as_str() {
                            "kind" => n.kind = nv.s().unwrap(),
                            "ilvl" => n.ilvl = nv.n().unwrap(),
                            "numId" => n.num_id = nv.n(),
                            _ => panic!("{nk}"),
                        }
                        n.mark(nk);
                    }
                }
                p.numbering = Some(n);
            }
            "indentLeftTwips" => p.indent_left_twips = v.n(),
            "indentRightTwips" => p.indent_right_twips = v.n(),
            "indentFirstLineTwips" => p.indent_first_line_twips = v.n(),
            "lineTwentieths" => p.line_twentieths = v.n(),
            "spaceAfterTwips" => p.space_after_twips = v.n(),
            "spaceBeforeTwips" => p.space_before_twips = v.n(),
            "spaceBeforeAuto" => p.space_before_auto = v.b(),
            "spaceAfterAuto" => p.space_after_auto = v.b(),
            "shadingHex" => p.shading_hex = v.s(),
            "tabStops" => {
                p.tab_stops = Some(
                    v.arr()
                        .iter()
                        .map(|t| {
                            let mut s = ParaTabStop::default();
                            if let J::Obj(kv) = t {
                                for (tk, tv) in kv {
                                    match tk.as_str() {
                                        "posTwips" => s.pos_twips = tv.n().unwrap(),
                                        "align" => s.align = tv.s().unwrap(),
                                        "leader" => s.leader = tv.s(),
                                        _ => panic!("{tk}"),
                                    }
                                    s.mark(tk);
                                }
                            }
                            s
                        })
                        .collect(),
                );
            }
            "borders" => {
                let mut b = ParagraphBorders::default();
                if let J::Obj(kv) = v {
                    for (bk, bv) in kv {
                        let e = Some(border_edge(bv));
                        match bk.as_str() {
                            "top" => b.top = e,
                            "left" => b.left = e,
                            "bottom" => b.bottom = e,
                            "right" => b.right = e,
                            "between" => b.between = e,
                            "bar" => b.bar = e,
                            _ => panic!("{bk}"),
                        }
                        b.mark(bk);
                    }
                }
                p.borders = Some(Box::new(b));
            }
            "keepNext" => p.keep_next = v.b(),
            "pageBreakBefore" => p.page_break_before = v.b(),
            "widowControl" => p.widow_control = v.b(),
            "textboxTightWrap" => p.textbox_tight_wrap = v.s(),
            _ => panic!("{k}"),
        });
    }
}

fn rev_info(j: &J) -> RevisionInfo {
    let mut r = RevisionInfo::default();
    if let J::Obj(kv) = j {
        for (k, v) in kv {
            match k.as_str() {
                "author" => r.author = v.s().unwrap(),
                "date" => r.date = v.s(),
                "move" => r.r#move = v.b(),
                "old" => {}
                _ => panic!("{k}"),
            }
            r.mark(k);
        }
    }
    r
}

// ───────────── the replay ─────────────

/// The TS throws accepted by name (K11, LQ14): Rust returns a value where the TS throws.
fn accepted_throw(key: &str, f: &[String]) -> Option<&'static str> {
    if key.starts_with("fmtnum/") {
        return Some("formatNumber: TS throws (chicago/enclosed fraction or NaN, alphabetic/chicago repeat past the limit; K11)");
    }
    if key.contains("/num/") {
        return Some("NumberingState.next: TS throws inside formatNumber (chicago of NaN; K11)");
    }
    let _ = f;
    None
}

struct Counts {
    reader: usize,
    checked: usize,
    accepted: usize,
}

#[test]
fn word_semantics_match_the_ts() {
    let cap = Capture::read(base().with_extension("capture")).unwrap();
    let inp = inputs();
    let mut r = Replay::new(&cap);
    let mut c = Counts { reader: 0, checked: 0, accepted: 0 };
    // the fmt records: each fmt/<i> input drives its sub-keys
    let mut keys: Vec<&str> = inp.keys().map(String::as_str).collect();
    keys.sort_unstable();
    for key in keys {
        let f = &inp[key];
        let rec = cap.record(key);
        if let Some(rec) = rec
            && rec.get("throw").is_some()
            && let Some(why) = accepted_throw(key, f)
        {
            // the value Rust gives must still be computed without a panic
            replay_one(&mut r, key, f, &cap, &mut c, true);
            r.accept(key, "throw", "(a value)", why);
            c.accepted += 1;
            continue;
        }
        replay_one(&mut r, key, f, &cap, &mut c, false);
    }
    let report = r.finish();
    eprintln!(
        "word unit: {} cases checked, {} reader differences (facet `in`), {} TS throws accepted by name; {}",
        c.checked,
        c.reader,
        c.accepted,
        report.summary()
    );
    report.assert_clean();
}

#[allow(clippy::too_many_lines)]
fn replay_one(r: &mut Replay<'_>, key: &str, f: &[String], cap: &Capture, c: &mut Counts, threw: bool) {
    c.checked += 1;
    let suite = key.split('/').next().unwrap();
    let mut tcap = TextCap::default();
    // a TS throw accepted by name: compute, compare nothing
    let chk = |r: &mut Replay<'_>, facet: &str, v: &str| {
        if !threw {
            r.check(key, facet, v);
        }
    };
    match suite {
        "date" => {
            let t = num(&f[1]).unwrap().to_number();
            let off = num(&f[2]).unwrap().to_number();
            let lang = dec_str(&f[3]).unwrap();
            let s = dec(&f[4]).unwrap();
            let d = if off.is_nan() { LocalDateTime::INVALID } else { LocalDateTime::at(t, off * 60_000.0) };
            let v = if f[0] == "pic" {
                word::format_word_date(&d, &s, &lang)
            } else {
                word::date_field_value(&s, &d, &lang, &mut tcap).units().to_vec()
            };
            chk(r, "s", &js_str(&v));
        }
        "formula" => {
            let code = dec(&f[0]).unwrap();
            let table: [(&str, Option<&str>); 9] = [
                ("PAGE", Some("3")),
                ("NUMPAGES", Some("12")),
                ("SECTIONPAGES", Some("1.234")),
                ("SEQ", Some("2,5")),
                ("DATE", Some("hoje")),
                ("REF", None),
                ("NEG", Some("-7")),
                ("BIG", Some("999.999.999")),
                ("ZERO", Some("0")),
            ];
            let mut value = |name: &str, _i: &[u16]| -> Option<Vec<u16>> {
                table.iter().find(|(n, _)| *n == name).and_then(|(_, v)| v.map(|s| s.encode_utf16().collect()))
            };
            let v = word::evaluate_field_formula(&code, &mut value, Some("pt-BR"), 256, &mut tcap).unwrap();
            chk(r, "s", &opt_str(v.as_ref().map(Utf16::units)));
        }
        "picture" => {
            let v = num(&f[0]).unwrap().to_number();
            let p = dec_str(&f[1]).unwrap();
            let s = word::format_picture(v, &p).unwrap();
            chk(r, "s", &js_str(&s.encode_utf16().collect::<Vec<u16>>()));
        }
        "result" => {
            let v = num(&f[0]).unwrap().to_number();
            let s = word::format_formula_result(
                v,
                &word::CellNumber { n: v, decimals: f[1].parse().unwrap(), thousands: f[2] == "1", prefix: None, suffix: None },
            )
            .unwrap();
            chk(r, "s", &js_str(&s.encode_utf16().collect::<Vec<u16>>()));
        }
        "fmtnum" => {
            let n = num(&f[0]).unwrap().to_number();
            let fmt = dec_str(&f[1]);
            let lang = dec_str(&f[2]).unwrap();
            let s = word::format_number(n, fmt.as_deref(), &lang, &mut tcap);
            let out = if s.len() > 200 { format!("\"len:{}\"", s.len()) } else { js_str(s.units()) };
            chk(r, "s", &out);
        }
        "merge" => {
            let (a, b) = (dec_str(&f[1]), dec_str(&f[2]));
            macro_rules! merge {
                ($read:expr, $merge:path) => {{
                    let (ra, rb) = (read_el(a.as_deref(), $read), read_el(b.as_deref(), $read));
                    if !check_in(r, cap, key, |w| [ra.clone(), rb.clone()].canon(w)) {
                        c.reader += 1;
                        return;
                    }
                    let m = $merge(ra.as_ref(), rb.as_ref());
                    check_value(r, key, "out", |w| m.canon(w));
                    (ra, rb)
                }};
            }
            match f[0].as_str() {
                "rPr" => {
                    let (ra, rb) = merge!(read_rpr, word::merge_rpr);
                    let fonts =
                        word::merge_rfonts(ra.as_ref().and_then(|x| x.r_fonts.as_deref()), rb.as_ref().and_then(|x| x.r_fonts.as_deref()));
                    check_value(r, key, "fonts", |w| fonts.canon(w));
                }
                "pPr" => {
                    merge!(read_ppr, word::merge_ppr);
                }
                "tblPr" => {
                    merge!(read_tbl_pr, word::merge_tbl_pr);
                }
                "trPr" => {
                    merge!(read_tr_pr, word::merge_tr_pr);
                }
                _ => {
                    merge!(read_tc_pr, word::merge_tc_pr);
                }
            }
        }
        "cascade" => {
            let dd = rpr(dec_str(&f[0]).as_deref());
            let direct = rpr(dec_str(&f[1]).as_deref());
            let n: usize = f[2].parse().unwrap();
            let levels: Vec<Option<RPr>> = (0..n).map(|i| rpr(dec_str(&f[3 + i]).as_deref())).collect();
            if !check_in(r, cap, key, |w| {
                w.array(3);
                dd.canon(w);
                levels.canon(w);
                direct.canon(w);
            }) {
                c.reader += 1;
                return;
            }
            let lv: Vec<Option<&RPr>> = levels.iter().map(Option::as_ref).collect();
            let out = word::cascade_rpr(dd.as_ref(), &lv, direct.as_ref());
            check_value(r, key, "out", |w| out.canon(w));
        }
        "color" => {
            let theme = read_theme_text(&dec_str(&f[0]).unwrap()).unwrap();
            let rp = rpr(dec_str(&f[1]).as_deref());
            if !check_in(r, cap, key, |w| {
                w.array(2);
                theme.canon(w);
                rp.canon(w);
            }) {
                c.reader += 1;
                return;
            }
            let Some(rp) = rp else {
                // `rp.color` of undefined: the TS throws TypeError (the facets after `in` were not written)
                r.check(key, "throw", "\"TypeError\"");
                return;
            };
            let s = |x: Option<String>| x.map_or_else(|| "undefined".to_owned(), |v| js_str(&v.encode_utf16().collect::<Vec<u16>>()));
            chk(r, "color", &s(word::resolve_color(rp.color.as_deref().map(word::ColorRef::of_color), &theme)));
            chk(r, "ucolor", &s(word::resolve_color(rp.u.as_deref().map(word::ColorRef::of_underline), &theme)));
            chk(r, "shd", &s(word::shading_fill(rp.shd.as_deref(), &theme)));
            let hex = dec_str(&f[2]).unwrap();
            chk(r, "tint", &s(Some(word::apply_tint_shade(&hex, dec_str(&f[3]).as_deref(), dec_str(&f[4]).as_deref()))));
            chk(r, "hl", &s(word::highlight_hex(dec_str(&f[5]).as_deref()).map(str::to_owned)));
            let slot = match f[6].as_str() {
                "ascii" => word::FontSlot::Ascii,
                "hAnsi" => word::FontSlot::HAnsi,
                "eastAsia" => word::FontSlot::EastAsia,
                _ => word::FontSlot::Cs,
            };
            chk(r, "font", &s(word::font_of_slot(rp.r_fonts.as_deref(), slot, &theme).map(str::to_owned)));
        }
        "fmt" => replay_fmt(r, key, f, cap, c),
        "rev" => replay_rev(r, key, f),
        "heading" => {
            let runs = json(&dec_str(&f[0]).unwrap());
            let mut p = Paragraph::default();
            let mut caps = Vec::new();
            for x in runs.arr() {
                let a = x.arr();
                let J::Str(t) = &a[0] else { panic!() };
                let mut props = RunProps::default();
                if a[1].n().map(JsNumber::to_number) == Some(1.0) {
                    props.del = Some(Box::new(RevisionInfo { author: "A".into(), ..RevisionInfo::default() }));
                }
                caps.push(a[2].n().map(JsNumber::to_number) == Some(1.0));
                p.runs.push(Run { text: Utf16::from_units(t.clone()), props: Arc::new(props), ..Run::default() });
            }
            let ptrs: Vec<*const Run> = p.runs.iter().map(|r| r as *const Run).collect();
            let mut f = |run: &Run| -> bool {
                let j = ptrs.iter().position(|&q| std::ptr::eq(q, run)).unwrap();
                caps[j]
            };
            chk(r, "s", &js_str(&word::heading_text(&p, Some(&mut f))));
            chk(r, "plain", &js_str(&word::heading_text(&p, None)));
        }
        _ => {}
    }
}

fn replay_fmt(r: &mut Replay<'_>, key: &str, f: &[String], cap: &Capture, c: &mut Counts) {
    let parts: Vec<&str> = key.split('/').collect();
    if parts.len() != 2 && !(parts.len() == 3 && parts[2] == "num") && parts.len() != 4 {
        return;
    }
    if parts.len() == 3 {
        return; // the numbering ops are replayed with their fmt record
    }
    if parts.len() == 4 {
        return; // run/para cases are replayed with their fmt record
    }
    let fmt = formatting(f);
    if !check_in(r, cap, key, |w| fmt.canon(w)) {
        c.reader += 1;
        for k in cap.records().iter().map(|x| x.key.clone()).filter(|k| k.starts_with(&format!("{key}/"))) {
            skip_all(r, cap, &k);
        }
        return;
    }
    // the style resolver
    let mut sr = StyleResolver::new();
    for id in [
        "Normal",
        "Heading1",
        "Heading2",
        "Title",
        "ListPara",
        "2",
        "10",
        "1",
        "Char1",
        "Char2",
        "TableGrid",
        "TblB",
        "NumSt",
        "constructor",
        "-",
        "",
    ] {
        let k = format!("{key}/style/{id}");
        let id = if id == "-" { None } else { Some(id) };
        r.check(
            &k,
            "id",
            &StyleResolver::para_style_id(&fmt, id).map_or_else(|| "null".to_owned(), |s| js_str(&s.encode_utf16().collect::<Vec<u16>>())),
        );
        let chain: Vec<String> =
            StyleResolver::para_chain(&fmt, id).iter().map(|s| js_str(&s.style_id.encode_utf16().collect::<Vec<u16>>())).collect();
        r.check(&k, "chain", &format!("[{}]", chain.join(",")));
        let ps = sr.para_style(&fmt, id);
        check_value(r, &k, "para", |w| {
            let e: [EntryRef<'_>; 2] = [("pPr", &|w| ps.p_pr.canon(w)), ("rPr", &|w| ps.r_pr.canon(w))];
            canon_entries(w, &mut e.clone());
        });
        let cs = sr.char_style(&fmt, id);
        check_value(r, &k, "char", |w| cs.canon(w));
        let ts = sr.table_style(&fmt, id);
        check_value(r, &k, "table", |w| {
            let has = |w: &mut CanonWriter<'_>| w.bool(true);
            let a = |w: &mut CanonWriter<'_>| ts.tbl_pr.canon(w);
            let b = |w: &mut CanonWriter<'_>| ts.tr_pr.canon(w);
            let c2 = |w: &mut CanonWriter<'_>| ts.tc_pr.canon(w);
            let d = |w: &mut CanonWriter<'_>| ts.p_pr.canon(w);
            let e2 = |w: &mut CanonWriter<'_>| ts.r_pr.canon(w);
            let f2 = |w: &mut CanonWriter<'_>| ts.cond.canon(w);
            let mut e: Vec<EntryRef<'_>> = vec![("tblPr", &a), ("trPr", &b), ("tcPr", &c2), ("pPr", &d), ("rPr", &e2), ("cond", &f2)];
            if ts.has_style {
                e.push(("hasStyle", &has));
            }
            canon_entries(w, &mut e);
        });
    }
    // the numbering ops
    let nkey = format!("{key}/num");
    let inp = inputs_cache();
    if let Some(nf) = inp.get(&nkey) {
        let lang = dec_str(&nf[0]).unwrap();
        let mut ns = word::NumberingState::new(&lang, TextCap::default());
        for (j, op) in nf[1..].iter().enumerate() {
            let k = format!("{nkey}/{j}");
            let (a, b) = op.split_once(',').unwrap();
            let num_id = num(a).unwrap_or(JsNumber::Num(f64::NAN));
            // `ilvl ?? 0`
            let ilvl = match num(b) {
                None | Some(JsNumber::Null) => JsNumber::Num(0.0),
                Some(x) => x,
            };
            let threw = cap.get(&k, "throw").is_some();
            let m = ns.next(&fmt, num_id, ilvl);
            if threw {
                r.accept(&k, "throw", "(a value)", "NumberingState.next: TS throws inside formatNumber (chicago of NaN; K11)");
                c.accepted += 1;
                continue;
            }
            r.check(
                &k,
                "listKey",
                &ns.list_key(&fmt, num_id).map_or_else(|| "null".to_owned(), |s| js_str(&s.encode_utf16().collect::<Vec<u16>>())),
            );
            if let Some(l) = ns.level(&fmt, num_id, ilvl) {
                check_value(r, &k, "level", |w| l.canon(w));
            }
            match m {
                None => {
                    r.check(&k, "m", "null");
                }
                Some(m) => {
                    let lvl = m.level_in(&fmt).unwrap();
                    check_value(r, &k, "m", |w| {
                        let text = |w: &mut CanonWriter<'_>| w.str16(m.text.units());
                        let level = |w: &mut CanonWriter<'_>| lvl.canon(w);
                        let nid = |w: &mut CanonWriter<'_>| w.num(m.num_id);
                        let il = |w: &mut CanonWriter<'_>| w.num(m.ilvl);
                        let val = |w: &mut CanonWriter<'_>| w.num(m.value);
                        let lk = |w: &mut CanonWriter<'_>| w.str(&m.list_key);
                        let ch = |w: &mut CanonWriter<'_>| w.str16(m.chapter.as_ref().unwrap().units());
                        let pa = |w: &mut CanonWriter<'_>| {
                            let p = m.parts.as_ref().unwrap();
                            w.array(p.len());
                            for x in p {
                                w.str16(x.units());
                            }
                        };
                        let mut e: Vec<EntryRef<'_>> =
                            vec![("text", &text), ("level", &level), ("numId", &nid), ("ilvl", &il), ("value", &val), ("listKey", &lk)];
                        if m.chapter.is_some() {
                            e.push(("chapter", &ch));
                        }
                        if m.parts.is_some() {
                            e.push(("parts", &pa));
                        }
                        canon_entries(w, &mut e);
                    });
                }
            }
        }
    }
    // runs: directRPr and the Original view
    for j in 0..6 {
        let k = format!("{key}/run/{j}");
        let Some(rf) = inp.get(&k) else { continue };
        if cap.get(&k, "throw").is_some() {
            r.skip(&k, "throw");
            continue;
        }
        let rp = rpr(dec_str(&rf[0]).as_deref());
        let mut props = match &rp {
            Some(x) => run_props_from_rpr(Some(x)),
            None => default_run_props(),
        };
        edit_run_props(&mut props, &json(&dec_str(&rf[1]).unwrap()));
        let mut run = Run { text: Utf16::from("x"), props: Arc::new(props.clone()), rpr: rp.map(Arc::new), ..Run::default() };
        match rf[2].as_str() {
            "note" => run.ext_mut().note_ref = Some(NoteRef { kind: "footnote".into(), id: JsNumber::Num(1.0), ..NoteRef::default() }),
            "sym" => run.ext_mut().sym_font = Some("Symbol".into()),
            _ => {}
        }
        if !check_in(r, cap, &k, |w| props.canon(w)) {
            c.reader += 1;
            continue;
        }
        let d = word::direct_rpr(&run);
        check_value(r, &k, "out", |w| d.canon(w));
        let rd = RevisionDisplay::new(RevisionMode::Original, None, None, None, None, None, &[]);
        let o = rd.direct_rpr(&run);
        check_value(r, &k, "orig", |w| o.canon(w));
    }
    // paragraphs: directPPr, resolvePPr, inheritedPPr, effectivePPr, outlineLevelOf
    let mut resolver = StyleResolver::new();
    for j in 0..6 {
        let k = format!("{key}/para/{j}");
        let Some(pf) = inp.get(&k) else { continue };
        if cap.get(&k, "throw").is_some() {
            r.skip(&k, "throw");
            continue;
        }
        let p = ppr(dec_str(&pf[0]).as_deref());
        let mut props = para_props_from_ppr(p.as_ref());
        edit_para_props(&mut props, &json(&dec_str(&pf[1]).unwrap()));
        let table = ppr(dec_str(&pf[2]).as_deref());
        if !check_in(r, cap, &k, |w| props.canon(w)) {
            c.reader += 1;
            continue;
        }
        let d = word::direct_ppr(p.as_ref(), &props);
        check_value(r, &k, "out", |w| d.canon(w));
        let res = word::resolve_ppr(&fmt, &mut resolver, p.as_ref(), &props, table.as_ref());
        check_value(r, &k, "ppr", |w| res.ppr.canon(w));
        r.check(
            &k,
            "styleId",
            &res.style_id.as_deref().map_or_else(|| "null".to_owned(), |s| js_str(&s.encode_utf16().collect::<Vec<u16>>())),
        );
        check_value(r, &k, "styleRPr", |w| res.style_rpr.canon(w));
        let mut fresh = StyleResolver::new();
        let inh = word::inherited_ppr(&fmt, &mut fresh, p.as_ref(), &props);
        check_value(r, &k, "inherited", |w| inh.canon(w));
        let eff = word::effective_ppr(&fmt, &mut fresh, p.as_ref(), &props);
        check_value(r, &k, "effective", |w| eff.canon(w));
        let para = Paragraph { props: Arc::new(props.clone()), ppr: p.clone().map(Arc::new), ..Paragraph::default() };
        r.check(&k, "outline", &json_num(word::outline_level_of(&fmt, &para, &res.ppr)));
        let rd = RevisionDisplay::new(RevisionMode::Original, None, None, None, None, None, &[]);
        let q = rd.paragraph(&para);
        check_value(r, &k, "orig", |w| match &q {
            None => w.null(),
            Some((qp, qprops)) => {
                let e: [EntryRef<'_>; 2] = [("ppr", &|w| qp.canon(w)), ("props", &|w| qprops.canon(w))];
                canon_entries(w, &mut e.clone());
            }
        });
    }
}

fn inputs_cache() -> &'static HashMap<String, Vec<String>> {
    static CELL: std::sync::OnceLock<HashMap<String, Vec<String>>> = std::sync::OnceLock::new();
    CELL.get_or_init(inputs)
}

fn replay_rev(r: &mut Replay<'_>, key: &str, f: &[String]) {
    let view = json(&dec_str(&f[0]).unwrap());
    let doc_order: Vec<Box<str>> = json(&dec_str(&f[1]).unwrap()).arr().iter().map(|x| x.s().unwrap()).collect();
    let runs_j = json(&dec_str(&f[2]).unwrap());
    let pprops_j = json(&dec_str(&f[3]).unwrap());
    let carried: i64 = f[4].parse().unwrap();
    let mode = match view.get("mode").and_then(J::s).as_deref() {
        Some("simple") => RevisionMode::Simple,
        Some("all") => RevisionMode::All,
        Some("original") => RevisionMode::Original,
        _ => RevisionMode::Final,
    };
    let list = |k: &str| -> Option<Vec<Box<str>>> { view.get(k).map(|v| v.arr().iter().map(|x| x.s().unwrap()).collect()) };
    let colors: Option<indexmap::IndexMap<Box<str>, Box<str>>> = view.get("colors").map(|c| match c {
        J::Obj(kv) => kv.iter().map(|(k, v)| (k.as_str().into(), v.s().unwrap())).collect(),
        _ => indexmap::IndexMap::new(),
    });
    let authors = list("authors");
    let order = list("order");
    let rd = RevisionDisplay::new(
        mode,
        view.get("insDel").and_then(J::b),
        view.get("formatting").and_then(J::b),
        authors.as_deref(),
        colors.as_ref(),
        order.as_deref(),
        &doc_order,
    );
    let mut runs: Vec<(RunProps, usize)> = Vec::new();
    for x in runs_j.arr() {
        let J::Str(t) = x.get("text").unwrap() else { panic!() };
        let mut p = RunProps::default();
        let pj = x.get("props").unwrap();
        if let J::Obj(kv) = pj {
            for (k, v) in kv {
                match k.as_str() {
                    "ins" => p.ins = Some(Box::new(rev_info(v))),
                    "del" => p.del = Some(Box::new(rev_info(v))),
                    "fmt" => {
                        let ri = rev_info(v);
                        p.fmt = Some(Box::new(FormatChange {
                            author: ri.author,
                            date: ri.date,
                            date_utc: None,
                            r#move: ri.r#move,
                            ..FormatChange::default()
                        }));
                    }
                    _ => panic!("{k}"),
                }
            }
        }
        runs.push((p, t.len()));
    }
    let mut pprops = ParaProps::default();
    if let J::Obj(kv) = &pprops_j {
        for (k, v) in kv {
            match k.as_str() {
                "pfmt" => {
                    let ri = rev_info(v);
                    pprops.pfmt = Some(Box::new(ParaFormatChange {
                        author: ri.author,
                        date: ri.date,
                        r#move: ri.r#move,
                        ..ParaFormatChange::default()
                    }));
                }
                "markIns" => pprops.mark_ins = Some(Box::new(rev_info(v))),
                "markDel" => pprops.mark_del = Some(Box::new(rev_info(v))),
                _ => panic!("{k}"),
            }
        }
    }
    let run_json = |x: Option<word::RunRevision>| -> String {
        match x {
            None => "null".into(),
            Some(word::RunRevision::Hide) => "\"hide\"".into(),
            Some(word::RunRevision::Marked { color, del, r#move }) => {
                format!(
                    "{{\"color\":{},\"kind\":\"{}\",\"move\":{}}}",
                    js_str(&color.encode_utf16().collect::<Vec<u16>>()),
                    if del { "del" } else { "ins" },
                    r#move
                )
            }
        }
    };
    let all: Vec<String> = runs.iter().map(|(p, _)| run_json(rd.run(p))).collect();
    r.check(key, "runs", &format!("[{}]", all.join(",")));
    r.check(key, "gone", if rd.mark_gone(&pprops) { "true" } else { "false" });
    r.check(key, "bar", &rd.bar_color().map_or_else(|| "null".to_owned(), |c| format!("\"{c}\"")));
    let colors: Vec<String> =
        ["Ana", "Bia", "Caio", "Dani"].iter().map(|a| js_str(&rd.color(a).encode_utf16().collect::<Vec<u16>>())).collect();
    r.check(key, "colors", &format!("[{}]", colors.join(",")));
    let joins: Vec<u32> = if carried >= 0 {
        let n: usize = runs.iter().take(usize::try_from(carried).unwrap()).map(|(_, l)| *l).sum();
        vec![u32::try_from(n).unwrap()]
    } else {
        Vec::new()
    };
    let rr = word::revision_ranges(&rd, runs.iter().map(|(p, l)| (p, *l)), &pprops, &joins);
    let s = match rr {
        None => "null".to_owned(),
        Some(x) => {
            let rs: Vec<String> = x.ranges.iter().map(|(a, b, h)| format!("[{},{},{}]", json_num(*a), json_num(*b), h)).collect();
            format!("{{\"ranges\":[{}],\"whole\":{},\"mark\":{}}}", rs.join(","), x.whole, x.mark)
        }
    };
    r.check(key, "ranges", &s);
}
