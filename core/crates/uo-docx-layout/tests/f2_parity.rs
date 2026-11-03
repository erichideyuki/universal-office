//! Frontier F2 parity (record "Wave 1 — F2"): the P3 facets of the Word semantics in
//! `uo_docx::word`, replayed from the inputs `tools/capture-layout.mjs` dumps
//! (`<key>.f2.tsv`, `tools/lib/facets-f2.mjs`) and compared with the capture's digests:
//!
//! - `p3.datefmt` (`dateFieldValue`), `p3.formula` (`evaluateFieldFormula`, with the transcript of
//!   its nested-field callback) — recomputed from the dumped strings;
//! - `p3.numbering` (every `NumberingState.next` of the engine's states, with their clones) and
//!   `p3.style` (`StyleResolver` queries) — over the formatting the D1a reader (R2) gives;
//! - `p3.hyphlangs` (`documentHyphenationLanguages`) needs the whole document (R5): reported
//!   `pending` until `uo_docx::read::read_document` reads.
//!
//! And the São Paulo date facet (LQ10): `datefmt-America-Sao-Paulo.*` from
//! `uo-docx/js/capture-word.mjs --tz America/Sao_Paulo`, replayed through the layout's `Clock` with
//! a `TimeZoneRule::Table` built from the zone's transitions the capture recorded.
//!
//! Always runs on the committed fixture `tests/captures/f2` (four `tools/gen-f2-docs.mjs`
//! documents: lists, styles, DATE/TIME/CREATEDATE and formula fields); `UO_F2_CAPTURE=<capture>
//! UO_F2_DUMP=<dump dir> UO_F2_DOCS=<dir the keys are relative to>` replays another run (the Word
//! oracle: `--mode oracle`, docs = `write/packages/docx-layout/test/oracle`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_docx::canon::{CanonSink, CanonWriter, EntryRef, canon_entries};
use uo_docx::js::{JsNumber, JsValue};
use uo_docx::model::formatting::DocFormatting;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::read_doc_formatting;
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::word::{self, LocalDateTime, NumberingState, StyleResolver, TextCap};
use uo_opc::Package;
use uo_testkit::{Capture, Replay, Stream};

/// The facet stream of the TS `Sink` (`tools/lib/sink.mjs`).
struct S(Stream);

impl S {
    fn new() -> S {
        S(Stream::new())
    }
    fn tag(&mut self, c: u8) -> &mut Self {
        self.0.tag(c);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.u32(v);
        self
    }
    fn num(&mut self, v: f64) -> &mut Self {
        self.0.f64(uo_jsnum::conv::canonicalize_nan(v));
        self
    }
    fn u16s(&mut self, s: &[u16]) -> &mut Self {
        let b: Vec<u8> = s.iter().flat_map(|u| u.to_le_bytes()).collect();
        self.0.bytes(&b);
        self
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.u16s(&s.encode_utf16().collect::<Vec<u16>>())
    }
    fn opt_u16s(&mut self, s: Option<&[u16]>) -> &mut Self {
        self.0.bool(s.is_some());
        if let Some(s) = s {
            self.u16s(s);
        }
        self
    }
    fn value(self) -> String {
        let n = self.0.items();
        format!("{n}:{}", self.0.finish().short())
    }
}

/// A canonical sink over a testkit stream (the uo-docx `Canon` of the marker and style values).
struct CS(Stream);

impl CanonSink for CS {
    fn tag(&mut self, c: u8) {
        self.0.tag(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.u32(v);
    }
    fn f64(&mut self, v: f64) {
        self.0.f64(v);
    }
    fn bool(&mut self, v: bool) {
        self.0.bool(v);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.bytes(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.digest_of(b);
    }
}

fn canon_digest(f: impl FnOnce(&mut CanonWriter<'_>)) -> String {
    let mut s = CS(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s);
        f(&mut w);
    }
    let n = s.0.items();
    format!("{n}:{}", s.0.finish().short())
}

/// A dumped string: `-` undefined, `~` null, `:` + UTF-16 hex.
fn field(f: &str) -> Option<Vec<u16>> {
    let h = f.strip_prefix(':')?;
    Some((0..h.len() / 4).map(|i| u16::from_str_radix(&h[4 * i..4 * i + 4], 16).unwrap()).collect())
}

fn number(f: &str) -> Option<JsNumber> {
    match f {
        "-" => None,
        "~" => Some(JsNumber::Null),
        "-0" => Some(JsNumber::Num(-0.0)),
        _ => Some(JsNumber::Num(uo_jsnum::parse::to_number(f))),
    }
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/f2")
}

fn formatting_of(docx: &Path) -> DocFormatting {
    let bytes = std::fs::read(docx).unwrap_or_else(|e| panic!("{}: {e}", docx.display()));
    let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let opts = ReadOptions::default();
    let mut ctx = ReadCtx::new(&pkg, &opts);
    read_doc_formatting(&mut ctx).unwrap()
}

/// The marker digest of `facets-f2.mjs` (`digest(m)`: the marker object through `canon.mjs`).
fn marker_digest(fmt: &DocFormatting, m: &word::NumberMarker) -> String {
    let lvl = m.level_in(fmt).unwrap();
    canon_digest(|w| {
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
    })
}

/// The style query digests of `facets-f2.mjs`.
fn style_digest(fmt: &DocFormatting, sr: &mut StyleResolver, method: &str, id: Option<&str>) -> String {
    match method {
        "paraStyle" => {
            let ps = sr.para_style(fmt, id);
            canon_digest(|w| {
                let a = |w: &mut CanonWriter<'_>| ps.p_pr.canon(w);
                let b = |w: &mut CanonWriter<'_>| ps.r_pr.canon(w);
                canon_entries(w, &mut [("pPr", &a), ("rPr", &b)]);
            })
        }
        "charStyle" => {
            let cs = sr.char_style(fmt, id);
            canon_digest(|w| cs.canon(w))
        }
        _ => {
            let ts = sr.table_style(fmt, id);
            canon_digest(|w| {
                let a = |w: &mut CanonWriter<'_>| ts.tbl_pr.canon(w);
                let b = |w: &mut CanonWriter<'_>| ts.tr_pr.canon(w);
                let c = |w: &mut CanonWriter<'_>| ts.tc_pr.canon(w);
                let d = |w: &mut CanonWriter<'_>| ts.p_pr.canon(w);
                let e = |w: &mut CanonWriter<'_>| ts.r_pr.canon(w);
                let f = |w: &mut CanonWriter<'_>| ts.cond.canon(w);
                let h = |w: &mut CanonWriter<'_>| w.bool(true);
                let mut v: Vec<EntryRef<'_>> = vec![("tblPr", &a), ("trPr", &b), ("tcPr", &c), ("pPr", &d), ("rPr", &e), ("cond", &f)];
                if ts.has_style {
                    v.push(("hasStyle", &h));
                }
                canon_entries(w, &mut v);
            })
        }
    }
}

/// One answer of a formula's nested-field callback: name, instruction, value.
type Answer = (Vec<u16>, Vec<u16>, Option<Vec<u16>>);

/// One document's F2 facets from its dump; `fmt` = the formatting read from its docx (`None`: the
/// docx is not at hand, numbering and style reported pending).
fn replay_doc(r: &mut Replay<'_>, key: &str, dump: &str, fmt: Option<&DocFormatting>, counts: &mut [usize; 4]) {
    let mut date = S::new();
    let mut formula = S::new();
    let mut numbering = S::new();
    let mut style = S::new();
    let mut states: Vec<Option<NumberingState>> = Vec::new();
    let mut sr = StyleResolver::new();
    let mut cap = TextCap::default();
    for line in dump.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "D" => {
                let instr = field(f[1]).unwrap();
                let t = number(f[2]).unwrap().to_number();
                let off = number(f[3]).unwrap().to_number();
                let lang = field(f[4]).map_or_else(|| "pt-BR".to_owned(), |u| String::from_utf16(&u).unwrap());
                // the layout's clock at the capture's zone (UTC: the fixed offset the dump shows)
                let clock = Clock { now_ms: t, tz: TimeZoneRule::Fixed { offset_min: uo_jsnum::conv::to_int32(off) } };
                let d = clock.local(t);
                let v = word::date_field_value(&instr, &d, &lang, &mut cap);
                date.tag(b'd').u16s(&instr).num(t).num(off).str(&lang).u16s(v.units());
                counts[0] += 1;
            }
            "F" => {
                let code = field(f[1]).unwrap();
                let lang = field(f[2]).map(|u| String::from_utf16(&u).unwrap());
                let n: usize = f[3].parse().unwrap();
                let answers: Vec<Answer> =
                    (0..n).map(|i| (field(f[4 + 3 * i]).unwrap(), field(f[5 + 3 * i]).unwrap(), field(f[6 + 3 * i]))).collect();
                let mut at = 0usize;
                let mut asked: Vec<Answer> = Vec::new();
                let mut value = |name: &str, instr: &[u16]| -> Option<Vec<u16>> {
                    let a = answers.get(at).and_then(|x| x.2.clone());
                    asked.push((name.encode_utf16().collect(), instr.to_vec(), a.clone()));
                    at += 1;
                    a
                };
                let res = word::evaluate_field_formula(&code, &mut value, lang.as_deref(), 256, &mut cap).unwrap();
                let s = formula.tag(b'f').u16s(&code).opt_u16s(lang.as_ref().map(|l| l.encode_utf16().collect::<Vec<u16>>()).as_deref());
                s.u32(u32::try_from(asked.len()).unwrap());
                for (nm, i, a) in &asked {
                    s.u16s(nm).u16s(i).opt_u16s(a.as_deref());
                }
                s.opt_u16s(res.as_ref().map(uo_docx::text::Utf16::units));
                counts[1] += 1;
            }
            "N" | "C" | "X" if fmt.is_none() => {}
            "N" => {
                let l: usize = f[1].parse().unwrap();
                let lang = String::from_utf16(&field(f[2]).unwrap()).unwrap();
                if states.len() <= l {
                    states.resize(l + 1, None);
                }
                states[l] = Some(word::numbering_state(&lang, &uo_docx_layout::options::LayoutLimits::default()));
                numbering.tag(b'n').u32(u32::try_from(l).unwrap()).str(&lang);
            }
            "C" => {
                let (src, dst): (usize, usize) = (f[1].parse().unwrap(), f[2].parse().unwrap());
                if states.len() <= dst {
                    states.resize(dst + 1, None);
                }
                states[dst] = states[src].clone();
                numbering.tag(b'c').u32(u32::try_from(src).unwrap()).u32(u32::try_from(dst).unwrap());
            }
            "X" => {
                let fmt = fmt.unwrap();
                let l: usize = f[1].parse().unwrap();
                let (num_id, ilvl) = (number(f[2]), number(f[3]));
                let st = states[l].as_mut().unwrap();
                let m = st.next(fmt, num_id.unwrap_or(JsNumber::Num(f64::NAN)), ilvl.unwrap_or(JsNumber::Num(f64::NAN)));
                let s = numbering.tag(b'x').u32(u32::try_from(l).unwrap());
                for a in [num_id, ilvl] {
                    match a {
                        None => s.tag(b'u'),
                        Some(JsNumber::Null) => s.tag(b'z'),
                        Some(JsNumber::Num(x)) => s.tag(b'n').num(x),
                    };
                }
                s.str(&m.as_ref().map_or_else(|| "null".to_owned(), |m| marker_digest(fmt, m)));
                counts[2] += 1;
            }
            "S" if fmt.is_some() => {
                let fmt = fmt.unwrap();
                let id = field(f[2]).map(|u| String::from_utf16(&u).unwrap());
                let d = style_digest(fmt, &mut sr, f[1], id.as_deref());
                style.tag(b's').str(f[1]).opt_u16s(id.as_ref().map(|s| s.encode_utf16().collect::<Vec<u16>>()).as_deref()).str(&d);
                counts[3] += 1;
            }
            _ => {}
        }
    }
    r.check(key, "p3.datefmt", &date.value());
    r.check(key, "p3.formula", &formula.value());
    if fmt.is_some() {
        r.check(key, "p3.numbering", &numbering.value());
        r.check(key, "p3.style", &style.value());
    }
    // p3.hyphlangs: the whole document is R5's (pending until read_document reads)
}

fn run_capture(capture: &Path, dump_dir: &Path, docs: &Path) {
    let cap = Capture::read(capture).unwrap();
    let mut r = Replay::new(&cap);
    let mut counts = [0usize; 4];
    let mut pending = 0usize;
    for rec in cap.records() {
        let key = rec.key.as_str();
        let safe = key.replace(['/', '\\', ':'], "__");
        let Ok(dump) = std::fs::read_to_string(dump_dir.join(format!("{safe}.f2.tsv"))) else { continue };
        let docx = docs.join(format!("{key}.docx"));
        let fmt = docx.exists().then(|| formatting_of(&docx));
        replay_doc(&mut r, key, &dump, fmt.as_ref(), &mut counts);
        // every other facet belongs to other frontiers or to P1/P2
        for (f, _) in &rec.facets {
            if !f.starts_with("p3.datefmt") && !f.starts_with("p3.formula") && !f.starts_with("p3.numbering") && !f.starts_with("p3.style")
            {
                r.skip(key, f);
            }
            if f == "p3.hyphlangs" {
                pending += 1;
            }
            if fmt.is_none() && (f == "p3.numbering" || f == "p3.style") {
                r.skip(key, f);
                pending += 1;
            }
        }
    }
    let report = r.finish();
    eprintln!(
        "F2 facets: {} DATE/TIME calls, {} formulas, {} numbering markers, {} style queries; {} facets pending (p3.hyphlangs needs the R5 reader); {}",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        pending,
        report.summary()
    );
    report.assert_clean();
}

#[test]
fn f2_facets_replay() {
    match (std::env::var_os("UO_F2_CAPTURE"), std::env::var_os("UO_F2_DUMP")) {
        (Some(c), Some(d)) => {
            let docs = std::env::var_os("UO_F2_DOCS").map_or_else(|| fixture().join("docs"), PathBuf::from);
            run_capture(Path::new(&c), Path::new(&d), &docs);
        }
        _ => run_capture(&fixture().join("layout-f2.capture"), &fixture(), &fixture().join("docs")),
    }
}

/// The São Paulo facet (LQ10): every DATE picture and instruction at instants across the zone's
/// rules (DST eras until 2019, the 2018/2019 switches), through `Clock::local` with the zone's
/// `TimeZoneRule::Table`; the rule's offsets equal the TS process's at every instant.
#[test]
fn sao_paulo_dates() {
    let base = fixture().join("datefmt-America-Sao-Paulo");
    let cap = Capture::read(base.with_extension("capture")).unwrap();
    let table: Vec<(f64, i32)> = cap
        .meta("transitions")
        .unwrap()
        .split(',')
        .map(|e| {
            let (t, o) = e.split_once(':').unwrap();
            (uo_jsnum::parse::to_number(t), o.parse().unwrap())
        })
        .collect();
    let rule = TimeZoneRule::Table(Arc::from(table));
    let inputs = std::fs::read_to_string(base.with_extension("inputs")).unwrap();
    let mut r = Replay::new(&cap);
    let (mut n, mut offsets) = (0usize, 0usize);
    for line in inputs.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let t = uo_jsnum::parse::to_number(f[2]);
        let recorded = uo_jsnum::parse::to_number(f[3]);
        let lang = dec(f[4]).map(|u| String::from_utf16(&u).unwrap()).unwrap();
        let s = dec(f[5]).unwrap();
        let clock = Clock { now_ms: t, tz: rule.clone() };
        if !t.is_nan() {
            assert_eq!(f64::from(rule.offset_min_at(t)), recorded, "offset at {t}");
            offsets += 1;
        }
        let d: LocalDateTime = clock.local(t);
        let v = if f[1] == "pic" {
            word::format_word_date(&d, &s, &lang)
        } else {
            word::date_field_value(&s, &d, &lang, &mut word::TextCap::default()).units().to_vec()
        };
        let mut j = String::new();
        uo_jsnum::json::push_string_utf16(&mut j, &v);
        r.check(f[0], "s", &j);
        n += 1;
    }
    let report = r.finish();
    eprintln!("São Paulo dates: {n} cases, {offsets} offsets equal; {}", report.summary());
    report.assert_clean();
}

/// `capture-word.mjs` string fields (`s` + escapes).
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

/// The glue: the options' revision view becomes the TS display (`new RevisionDisplay(opts.revisions,
/// opts.revisions?.order ?? [])`), the limits' field-text cap reaches the numbering formats.
#[test]
fn glue() {
    use uo_docx_layout::options::{LayoutLimits, RevisionMode, RevisionView};
    let v = RevisionView { mode: RevisionMode::All, order: Some(vec!["Bia".into(), "Ana".into()]), ..RevisionView::default() };
    let rd = word::revision_display(Some(&v));
    assert_eq!(&*rd.color("Ana"), word::AUTHOR_PALETTE[1]);
    assert_eq!(rd.bar_color(), Some(word::BAR_COLOR));
    assert_eq!(word::revision_display(None).bar_color(), None);
    let limits = LayoutLimits { max_field_text: 8, ..LayoutLimits::default() };
    let mut cap = word::text_cap(&limits);
    let s = word::format_number(1e9, Some("upperRoman"), "pt-BR", &mut cap);
    assert_eq!(s.len(), 8);
    assert!(cap.take_truncated());
    let _ = word::numbering_state("en-US", &limits);
}
