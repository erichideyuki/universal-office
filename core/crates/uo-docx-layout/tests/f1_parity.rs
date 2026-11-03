//! Frontier F1 parity (record "Wave 1 — F1"): the P3 facets `p3.metrics` (`faceMetrics`),
//! `p3.advance` (`advanceTwips`), `p3.kern` (`SfntFont.kernGlyphsTwips`, K7) and `p3.faces`
//! (`FaceResolver.get`, the Word substitution chain), and the P2 facet `p2first` (the first
//! provider request per lower-case key and the answer identity, LQ19), replayed from the inputs
//! `tools/capture-layout.mjs` dumps (`<key>.f1.tsv`, `tools/lib/facets-f1.mjs`) and the provider
//! transcript (`<key>.transcript.tsv`). Every value is recomputed by the Rust port and compared
//! bit for bit with the TS value, and the four streams plus `p2first` are rebuilt and compared
//! with the capture's digests.
//!
//! Always runs on the committed fixture `tests/captures/f1` (CI-mode documents, open fonts);
//! `UO_LAYOUT_CAPTURE=<capture> UO_LAYOUT_DUMP=<dump dir>` replays a capture of the same run
//! (the CI job's 200 generated documents, or the Word oracle on a machine with the Office fonts,
//! LQ21: a face whose digest is not found fails).
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

mod common;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{FontIndex, Transcript, load_answers, uri_decode};
use uo_docx_layout::fonts::{
    FaceEnv, FaceResolverCache, FaceTable, FontAnswer, FontProvider, FontTable, face_metrics, sfnt_kern_glyphs_twips,
};
use uo_docx_layout::model::FontInfo;
use uo_docx_layout::para::advance_twips_font;
use uo_fonts::registry::{AnswerParts, FaceCache};
use uo_render::{FaceId, Resources};
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
    fn opt_num(&mut self, v: Option<f64>) -> &mut Self {
        self.0.bool(v.is_some());
        if let Some(v) = v {
            self.num(v);
        }
        self
    }
    fn bool(&mut self, b: bool) -> &mut Self {
        self.0.bool(b);
        self
    }
    fn u16s(&mut self, s: &str) -> &mut Self {
        let b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
        self.0.bytes(&b);
        self
    }
    fn opt_u16s(&mut self, s: Option<&str>) -> &mut Self {
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

fn bits(h: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(h, 16).unwrap_or_else(|e| panic!("bad f64 bits {h}: {e}")))
}

fn opt_bits(h: &str) -> Option<f64> {
    (h != "-").then(|| bits(h))
}

/// `-` = absent, `:` + encodeURIComponent.
fn opt_str(s: &str) -> Option<String> {
    s.strip_prefix(':').map(uri_decode)
}

fn same(a: f64, b: f64) -> bool {
    uo_jsnum::conv::canonicalize_nan(a).to_bits() == uo_jsnum::conv::canonicalize_nan(b).to_bits()
}

/// A JS number that is a code point / glyph id: exact integer, else the replay fails.
fn int_of(x: f64) -> u32 {
    assert!(x >= 0.0 && x.trunc() == x, "not an integer input: {x}");
    // ids above u32 behave as any id above 0xFFFF
    if x > f64::from(u32::MAX) { u32::MAX } else { uo_jsnum::conv::to_uint32(x) }
}

/// The transcript replay with call recording: answers every `face(family, bold, italic)` with the
/// TS answer of that request (its identity, K2a) and logs the call with the TS label.
struct Recorder {
    by_request: HashMap<(String, bool, bool), u32>,
    answers: Vec<Option<Arc<FontAnswer>>>,
    calls: Mutex<Vec<(String, bool, bool, u32)>>,
    unexpected: Mutex<Vec<(String, bool, bool)>>,
}

impl Recorder {
    fn new(t: &Transcript, fonts: &FontIndex) -> Recorder {
        let mut by_request = HashMap::new();
        for c in &t.calls {
            by_request.entry((c.family.clone(), c.bold, c.italic)).or_insert(c.answer);
        }
        let cache = FaceCache::shared();
        let loaded = load_answers(t, fonts);
        let answers: Vec<Option<Arc<FontAnswer>>> = loaded
            .answers
            .into_iter()
            .map(|(a, bytes, _)| {
                let face = cache.get_or_parse(bytes?).ok()?;
                Some(FontAnswer::new(AnswerParts {
                    key: a.key,
                    family: a.family,
                    face,
                    bold: a.bold,
                    italic: a.italic,
                    synth_bold: a.synth_bold,
                    synth_italic: a.synth_italic,
                    stand_in: a.stand_in,
                    advances: None,
                    ticket: None,
                }))
            })
            .collect();
        Recorder { by_request, answers, calls: Mutex::new(Vec::new()), unexpected: Mutex::new(Vec::new()) }
    }
}

impl FontProvider for Recorder {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let Some(&label) = self.by_request.get(&(family.to_owned(), bold, italic)) else {
            self.unexpected.lock().unwrap().push((family.to_owned(), bold, italic));
            return None;
        };
        self.calls.lock().unwrap().push((family.to_owned(), bold, italic, label));
        self.answers.get((label as usize).checked_sub(1)?)?.clone()
    }
}

/// The values of one document, recomputed.
#[derive(Default)]
struct Outcome {
    facets: Vec<(&'static str, String)>,
    /// value-level differences (first few), for the report
    diffs: Vec<String>,
    /// the document needs faces the index cannot supply (embedded fonts)
    skipped: Option<String>,
}

fn diff(o: &mut Outcome, what: String) {
    if o.diffs.len() < 8 {
        o.diffs.push(what);
    }
}

fn replay_doc(f1: &str, t: &Transcript, fonts: &FontIndex) -> Outcome {
    let mut o = Outcome::default();
    let cache = FaceCache::shared();
    let mut font_sha: HashMap<u32, String> = HashMap::new();
    let mut font_face: HashMap<u32, uo_fonts::Face> = HashMap::new();
    let (mut metrics, mut advance, mut kern, mut faces) = (S::new(), S::new(), S::new(), S::new());
    let (mut seen_m, mut seen_a, mut seen_k, mut seen_f) = (HashSet::new(), HashSet::new(), HashSet::new(), HashSet::new());
    let font_ref = |s: &mut S, seen: &mut HashSet<u32>, label: u32, sha: &str| {
        s.u32(label);
        if seen.insert(label) {
            s.u16s(sha);
        }
    };
    let mut table = FontTable::new();
    let recorder = Recorder::new(t, fonts);
    let mut resolver = FaceResolverCache::new();
    let mut ftable = FaceTable::new();
    let mut res = Resources::new();
    let mut rust_label: HashMap<FaceId, u32> = HashMap::new();
    let mut pending_desc: Option<(u32, FaceId)> = None;
    let face_of = |label: u32, font_sha: &HashMap<u32, String>, font_face: &mut HashMap<u32, uo_fonts::Face>, o: &mut Outcome| {
        if let Some(f) = font_face.get(&label) {
            return Some(f.clone());
        }
        let sha = font_sha.get(&label)?;
        let Some(bytes) = fonts.get(sha) else {
            o.skipped.get_or_insert_with(|| format!("font {sha} not in the index"));
            return None;
        };
        let f = cache.get_or_parse(bytes).ok()?.face().clone();
        font_face.insert(label, f.clone());
        Some(f)
    };
    for line in f1.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["F", l, sha] => {
                font_sha.insert(l.parse().unwrap(), (*sha).to_owned());
            }
            ["M", l, asc, desc, lh, ulp, ult, bare, sup] => {
                let label: u32 = l.parse().unwrap();
                let sha = font_sha[&label].clone();
                let Some(face) = face_of(label, &font_sha, &mut font_face, &mut o) else { continue };
                let m = face_metrics(&face);
                let ts = [bits(asc), bits(desc), bits(lh), bits(ulp), bits(ult), bits(bare)];
                let rs = [m.ascent, m.descent, m.line_height, m.underline_pos, m.underline_thick, m.bare_ascent];
                if ts.iter().zip(&rs).any(|(a, b)| !same(*a, *b)) || m.sup_offset.map(f64::to_bits) != opt_bits(sup).map(f64::to_bits) {
                    diff(&mut o, format!("faceMetrics font {label}: ts {ts:?} {sup}, rust {rs:?} {:?}", m.sup_offset));
                }
                let s = metrics.tag(b'm');
                font_ref(s, &mut seen_m, label, &sha);
                for v in rs {
                    s.num(v);
                }
                s.opt_num(m.sup_offset);
            }
            ["A", l, cp, size, w, r] => {
                let label: u32 = l.parse().unwrap();
                let sha = font_sha[&label].clone();
                let Some(face) = face_of(label, &font_sha, &mut font_face, &mut o) else { continue };
                let (cp, size, w) = (bits(cp), bits(size), opt_bits(w));
                let got = advance_twips_font(&face, int_of(cp), size, w.unwrap_or(100.0));
                if !same(got, bits(r)) {
                    diff(&mut o, format!("advanceTwips font {label} cp {cp} size {size} w {w:?}: ts {} rust {got}", bits(r)));
                }
                let s = advance.tag(b'a');
                font_ref(s, &mut seen_a, label, &sha);
                s.num(cp).num(size).opt_num(w).num(got);
            }
            ["K", l, g1, g2, size, r] => {
                let label: u32 = l.parse().unwrap();
                let sha = font_sha[&label].clone();
                let Some(face) = face_of(label, &font_sha, &mut font_face, &mut o) else { continue };
                let (g1n, g2n, size): (f64, f64, f64) = (uo_jsnum::parse::to_number(g1), uo_jsnum::parse::to_number(g2), bits(size));
                let got = sfnt_kern_glyphs_twips(&face, int_of(g1n), int_of(g2n), size);
                if !same(got, bits(r)) {
                    diff(&mut o, format!("kernGlyphsTwips font {label} {g1}/{g2} size {size}: ts {} rust {got}", bits(r)));
                }
                let s = kern.tag(b'k');
                font_ref(s, &mut seen_k, label, &sha);
                s.num(g1n).num(g2n).num(size).num(got);
            }
            ["T", name, alt, family, panose, charset, usb0] => {
                let name = opt_str(name).unwrap_or_default();
                let mut info = FontInfo { name: name.clone().into(), ..FontInfo::default() };
                info.alt_name = opt_str(alt).map(Into::into);
                info.family = opt_str(family).map(Into::into);
                info.panose1 = opt_str(panose).map(Into::into);
                info.charset = opt_str(charset).map(Into::into);
                if let Some(u) = opt_str(usb0) {
                    let mut sig = uo_docx_layout::model::JsRecord::new();
                    sig.insert("usb0", u.into());
                    info.sig = Some(sig);
                }
                table.insert(&name, info);
            }
            ["R", family, b, i, label] => {
                let family = opt_str(family);
                let ts_label: u32 = label.parse().unwrap();
                let mut env = FaceEnv { provider: &recorder, font_table: &table, table: &mut ftable, res: &mut res };
                let got = resolver.get(&mut env, family.as_deref(), *b == "1", *i == "1");
                let (label, fresh) = match got {
                    Ok(id) => {
                        let next = u32::try_from(rust_label.len()).unwrap() + 1;
                        let mut fresh = false;
                        let l = *rust_label.entry(id).or_insert_with(|| {
                            fresh = true;
                            next
                        });
                        (l, fresh.then_some(id))
                    }
                    Err(_) => (0, None),
                };
                if label != ts_label {
                    diff(&mut o, format!("FaceResolver.get({family:?}, {b}, {i}): ts face {ts_label}, rust {label}"));
                }
                faces.tag(b'r').opt_u16s(family.as_deref()).bool(*b == "1").bool(*i == "1").u32(label);
                pending_desc = fresh.map(|id| (label, id));
            }
            ["D", label, key, family, b, i, sb, si, st, font] => {
                let Some((rl, id)) = pending_desc.take() else {
                    diff(&mut o, format!("face {label}: described by the TS, not new in Rust"));
                    continue;
                };
                let a = &ftable.entry(id).unwrap().answer;
                let ts = (opt_str(key).unwrap_or_default(), opt_str(family).unwrap_or_default(), [*b, *i, *sb, *si, *st].map(|v| v == "1"));
                let rs = (a.key.clone(), a.family.clone(), [a.bold, a.italic, a.synth_bold, a.synth_italic, a.stand_in]);
                if ts != rs || label.parse::<u32>().ok() != Some(rl) {
                    diff(&mut o, format!("face {label}: ts {ts:?}, rust {rs:?}"));
                }
                let font_label: u32 = font.parse().unwrap();
                let rsha = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(a.bytes()));
                let label_ok = font_sha.get(&font_label) == Some(&rsha);
                if !label_ok {
                    diff(&mut o, format!("face {label}: font {font_label} is not the file of the Rust answer ({rsha})"));
                }
                let s = faces.u16s(&rs.0).u16s(&rs.1);
                for v in rs.2 {
                    s.bool(v);
                }
                font_ref(s, &mut seen_f, if label_ok { font_label } else { 0 }, &rsha);
            }
            [""] | [] => {}
            _ => panic!("bad f1.tsv line: {line}"),
        }
    }
    let unexpected = recorder.unexpected.lock().unwrap().clone();
    if !unexpected.is_empty() {
        diff(&mut o, format!("provider requests the TS never made: {unexpected:?}"));
    }
    // p2first (tools/lib/transcript.mjs): the first request per lower-case key, the answer label,
    // the answer described the first time
    let mut first = S::new();
    let (mut keys, mut described) = (HashSet::new(), HashSet::new());
    for (family, b, i, label) in recorder.calls.lock().unwrap().iter() {
        let k = format!("{}|{b}|{i}", uo_text::case::to_lowercase_str(family));
        if !keys.insert(k.clone()) {
            continue;
        }
        first.u16s(&k).u32(*label);
        if *label != 0 && described.insert(*label) {
            let a = &t.answers[*label as usize - 1];
            first.u16s(&a.key).u16s(&a.family).bool(a.bold).bool(a.italic).bool(a.synth_bold).bool(a.synth_italic).bool(a.stand_in);
            first.bool(a.sha256.is_some());
            if let Some(h) = &a.sha256 {
                first.u16s(h);
            }
        }
    }
    o.facets = vec![
        ("p3.metrics", metrics.value()),
        ("p3.advance", advance.value()),
        ("p3.kern", kern.value()),
        ("p3.faces", faces.value()),
        ("p2first", first.value()),
    ];
    o
}

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

fn replay(capture: &Path, dump: &Path) -> (uo_testkit::Report, usize, Vec<String>) {
    let cap = Capture::read(capture).unwrap();
    let mut fonts = FontIndex::open_fonts();
    let mut r = Replay::new(&cap);
    let mut done = 0;
    let mut notes = Vec::new();
    for rec in cap.records() {
        let skip_all = |r: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                r.skip(&rec.key, f);
            }
        };
        let f1 = dump.join(format!("{}.f1.tsv", safe(&rec.key)));
        if rec.get("error").is_some() || !f1.exists() {
            skip_all(&mut r);
            continue;
        }
        let t = Transcript::read(&dump.join(format!("{}.transcript.tsv", safe(&rec.key)))).unwrap();
        fonts.add_transcript_sources(&t);
        let out = replay_doc(&std::fs::read_to_string(&f1).unwrap(), &t, &fonts);
        if let Some(why) = &out.skipped {
            let embedded = t.answers.iter().any(|a| a.embedded);
            assert!(embedded, "{}: {why} (LQ21: a face not found fails)", rec.key);
            notes.push(format!("{}: skipped, embedded faces ({why})", rec.key));
            skip_all(&mut r);
            continue;
        }
        check_present(&mut r, rec, &out, &mut notes);
        done += 1;
    }
    (r.finish(), done, notes)
}

/// Checks the recomputed facets the record has, skips the others.
fn check_present(r: &mut Replay<'_>, rec: &uo_testkit::capture::Record, out: &Outcome, notes: &mut Vec<String>) {
    for (facet, value) in &out.facets {
        if rec.get(facet).is_some() {
            r.check(&rec.key, facet, value);
        }
    }
    for (f, _) in &rec.facets {
        if !out.facets.iter().any(|(n, _)| n == f) {
            r.skip(&rec.key, f);
        }
    }
    if !out.diffs.is_empty() {
        notes.push(format!("{}: {}", rec.key, out.diffs.join("; ")));
    }
}

/// The blocks of a combined dump (`S <key>` opens each).
fn blocks(text: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if let Some(k) = line.strip_prefix("S\t") {
            cur = Some(k.to_owned());
            out.entry(k.to_owned()).or_default();
        } else if let Some(k) = &cur {
            let b = out.get_mut(k).unwrap();
            b.push_str(line);
            b.push('\n');
        }
    }
    out
}

/// The resolver sweep (`tools/capture-f1-resolver.mjs`): `p3.faces` and `p2first` per scenario.
fn replay_sweep(capture: &Path, dir: &Path) -> (uo_testkit::Report, usize, Vec<String>) {
    let cap = Capture::read(capture).unwrap();
    let f1 = blocks(&std::fs::read_to_string(dir.join("resolver.f1.tsv")).unwrap());
    let tr = blocks(&std::fs::read_to_string(dir.join("resolver.transcript.tsv")).unwrap());
    let mut fonts = FontIndex::open_fonts();
    let mut r = Replay::new(&cap);
    let (mut done, mut notes) = (0, Vec::new());
    for rec in cap.records() {
        let t = Transcript::parse(&tr[&rec.key]).unwrap();
        fonts.add_transcript_sources(&t);
        let out = replay_doc(&f1[&rec.key], &t, &fonts);
        assert!(out.skipped.is_none(), "{}: {:?}", rec.key, out.skipped);
        check_present(&mut r, rec, &out, &mut notes);
        done += 1;
    }
    (r.finish(), done, notes)
}

fn report(what: &str, capture: &Path, dump: &Path) {
    let (rep, done, notes) = if what.starts_with("resolver") { replay_sweep(capture, dump) } else { replay(capture, dump) };
    eprintln!("F1 parity ({what}): {done} documents replayed; {}", rep.summary());
    for n in notes.iter().take(20) {
        eprintln!("  {n}");
    }
    rep.assert_clean();
    assert!(notes.iter().all(|n| n.contains("skipped, embedded faces")), "value differences: {notes:?}");
    assert!(done > 0, "nothing replayed");
}

#[test]
fn f1_facets_match_the_committed_capture() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures/f1");
    report("committed fixture", &dir.join("layout-ci.capture"), &dir);
}

#[test]
fn the_resolver_sweep_matches_the_committed_capture() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures/f1");
    report("resolver sweep (committed)", &dir.join("resolver.capture"), &dir);
}

#[test]
fn the_resolver_sweep_matches_a_capture_of_this_run() {
    let (Ok(cap), Ok(dir)) = (std::env::var("UO_F1_RESOLVER_CAPTURE"), std::env::var("UO_F1_RESOLVER_DUMP")) else {
        eprintln!("skipped: set UO_F1_RESOLVER_CAPTURE and UO_F1_RESOLVER_DUMP to replay a resolver sweep of this run");
        return;
    };
    report("resolver sweep (this run)", Path::new(&cap), Path::new(&dir));
}

#[test]
fn f1_facets_match_a_capture_of_this_run() {
    let (Ok(cap), Ok(dump)) = (std::env::var("UO_LAYOUT_CAPTURE"), std::env::var("UO_LAYOUT_DUMP")) else {
        eprintln!("skipped: set UO_LAYOUT_CAPTURE and UO_LAYOUT_DUMP to replay a capture of this run");
        return;
    };
    report("capture of this run", Path::new(&cap), Path::new(&dump));
}
