//! Parity of the Slides text layout (frontier B3): replays the text-layout captures of the frozen TS
//! (`js/capture-textlayout.mjs`: every text body of the samples, of `decks/b1a` and of the text-heavy `decks/b3a`, laid
//! out by the TS `layoutTextBody` with the TS measurers over the real advance widths of the open fonts). The
//! measurement log (`*.measures.tsv.gz`) becomes the `MeasureTable`; each call compares its inputs (`.in`), the set of
//! measurements asked (`.ask`, against the TS `measureText` calls) and the canonical JSON of the result, exactly.
//! The two-phase protocol is replayed too: from an empty table, `need_measures` rounds answered from the log reach
//! the same result within `MAX_ROUNDS`. Set `UO_PPTX_DUMP=<dir>` to write the Rust result texts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uo_dml_render::canon::Canon;
use uo_dml_render::model::{Shape, TextBody};
use uo_dml_render::text::{
    Bits, FaceKey, Kerning, LayoutError, LayoutTextOptions, MAX_ROUNDS, MeasureReq, MeasureTable, Measurer, StringKey, cell_body,
    layout_text_body, layout_text_body_traced, need_measures,
};
use uo_jsnum::format::to_js_string;
use uo_pptx::canon::{bytes_digest, text_digest};
use uo_pptx::{Deck, Limits};
use uo_testkit::sha256::sha256;
use uo_testkit::{Capture, Replay};

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn deck_path(key: &str) -> PathBuf {
    if let Some(rest) = key.strip_prefix("gen/") {
        let (group, n) = rest.split_once('/').unwrap();
        return captures().join("decks").join(group).join(format!("{n}.pptx"));
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples").join(key.strip_prefix("samples/").unwrap())
}

// ---------------------------------------------------------------- the measurement log

/// One line of the log: the ask key (everything but the width) and the width.
struct Entry {
    key: String,
    face: FaceKey,
    px: f64,
    kerning: Kerning,
    text: String,
    width: f64,
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap()
}

/// `JSON.parse` of a JSON string literal (the log's text field; no lone surrogates: they were written as U+FFFD).
fn json_string(s: &str) -> String {
    let inner = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap();
    let mut units: Vec<u16> = Vec::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut b = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut b));
            continue;
        }
        let e = it.next().unwrap();
        let u = match e {
            '"' => 0x22,
            '\\' => 0x5c,
            '/' => 0x2f,
            'b' => 8,
            'f' => 12,
            'n' => 10,
            'r' => 13,
            't' => 9,
            'u' => u16::from_str_radix(&it.by_ref().take(4).collect::<String>(), 16).unwrap(),
            _ => panic!("bad escape {e}"),
        };
        units.push(u);
    }
    String::from_utf16_lossy(&units)
}

fn kerning_name(k: Kerning) -> &'static str {
    match k {
        Kerning::Current => "auto",
        Kerning::None => "none",
        Kerning::Normal => "normal",
    }
}

fn load_log(file: &str) -> Vec<Entry> {
    let text = String::from_utf8(gunzip(&std::fs::read(captures().join(file)).unwrap())).unwrap();
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 7, "{line}");
        let kerning = match f[4] {
            "auto" => Kerning::Current,
            "none" => Kerning::None,
            "normal" => Kerning::Normal,
            k => panic!("kerning {k}"),
        };
        let key = f[..6].join("\t");
        out.push(Entry {
            key,
            face: FaceKey { family: f[0].into(), bold: f[1] == "1", italic: f[2] == "1" },
            px: uo_jsnum::parse::to_number(f[3]),
            kerning,
            text: json_string(f[5]),
            width: uo_jsnum::parse::to_number(f[6]),
        });
    }
    out
}

/// (family, bold, italic, text) of a 1000 px measurement.
type KernKey = (String, bool, bool, String);

/// The table the log answers: every line as a whole-string width; the 1000 px single characters with kerning off as
/// A1; the 1000 px strings measured with kerning off and on as A2.
fn table_from(log: &[Entry], font_kerning: bool, segmenter: bool) -> MeasureTable {
    let mut t = MeasureTable::with_cap(usize::MAX, usize::MAX);
    t.caps.font_kerning = font_kerning;
    t.caps.segmenter = segmenter;
    let mut kern: BTreeMap<KernKey, (Option<f64>, Option<f64>)> = BTreeMap::new();
    for e in log {
        t.set_width(
            &StringKey {
                face: e.face.clone(),
                size_px: Bits::of(e.px),
                kerning: e.kerning,
                letter_spacing_px: Bits::of(0.0),
                text: e.text.as_str().into(),
            },
            e.width,
        );
        // A1 is measured with kerning off where the context has `fontKerning`, else in its current state
        let a1 = if font_kerning { Kerning::None } else { Kerning::Current };
        if e.px == 1000.0 && e.kerning == a1 {
            let mut chars = e.text.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                t.set_char_at_1000(&e.face, c, e.width);
            }
        }
        if e.px == 1000.0 && e.kerning != Kerning::Current {
            let slot = kern.entry((e.face.family.to_string(), e.face.bold, e.face.italic, e.text.clone())).or_default();
            if e.kerning == Kerning::None {
                slot.0 = Some(e.width);
            } else {
                slot.1 = Some(e.width);
            }
        }
    }
    for ((family, bold, italic, text), (w0, w1)) in kern {
        if let (Some(w0), Some(w1)) = (w0, w1) {
            t.set_kern_at_1000(&FaceKey { family: family.into(), bold, italic }, &text, w0, w1);
        }
    }
    t
}

/// The raw `measureText` keys a request stands for (the capture's ask keys).
fn raw_keys(r: &MeasureReq, font_kerning: bool, out: &mut Vec<String>) {
    let key = |face: &FaceKey, px: f64, k: &str, text: &str| {
        format!(
            "{}\t{}\t{}\t{}\t{k}\t{}",
            face.family,
            u8::from(face.bold),
            u8::from(face.italic),
            to_js_string(px),
            uo_jsnum::json::string(text)
        )
    };
    match r {
        MeasureReq::CharAt1000 { face, ch } => {
            out.push(key(face, 1000.0, if font_kerning { "none" } else { "auto" }, ch.encode_utf8(&mut [0; 4])))
        }
        MeasureReq::KernAt1000 { face, text } => {
            out.push(key(face, 1000.0, "none", text));
            out.push(key(face, 1000.0, "normal", text));
        }
        MeasureReq::Width(k) => out.push(key(&k.face, k.size_px.value(), kerning_name(k.kerning), &k.text)),
        other => panic!("unexpected request {other:?}"),
    }
}

fn asks_digest(asked: &[MeasureReq], font_kerning: bool) -> String {
    let mut keys = Vec::new();
    for r in asked {
        raw_keys(r, font_kerning, &mut keys);
    }
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    keys.dedup();
    let d = sha256(keys.join("\n").as_bytes());
    format!("{}:{}", keys.len(), &uo_dml_render::canon::hex(&d)[..16])
}

// ---------------------------------------------------------------- the walk (capture-textlayout.mjs `slideCalls`)

struct Call {
    id: String,
    body: TextBody,
    w: f64,
    h: f64,
    measurer: Measurer,
    opts: LayoutTextOptions,
}

fn in_facet(c: &Call) -> String {
    format!(
        "w={},h={},m={},sz={},a={},s={}",
        to_js_string(c.w),
        to_js_string(c.h),
        if c.measurer == Measurer::Plain { "plain" } else { "ppt" },
        to_js_string(c.opts.default_sz_pt.unwrap()),
        c.opts.default_anchor.as_deref().unwrap(),
        c.opts.size_scale.map_or("-".to_string(), to_js_string)
    )
}

fn slide_calls(shapes: &[Shape], slide_w: f64) -> Vec<Call> {
    let s = 1280.0 / slide_w;
    let mut calls = Vec::new();
    for (k, sh) in shapes.iter().enumerate() {
        let (cx, cy) = sh.xfrm.as_ref().map_or((0.0, 0.0), |x| (x.cx, x.cy));
        let (w, h) = ((cx * s) / s, (cy * s) / s);
        let is_title = matches!(sh.ph_type.as_deref(), Some("title" | "ctrTitle"));
        let dsz = if is_title { 40.0 } else { 18.0 };
        let anchor: Box<str> = if is_title { "ctr".into() } else { "t".into() };
        let opts = |scale: Option<f64>| LayoutTextOptions {
            default_sz_pt: Some(dsz),
            default_anchor: Some(anchor.clone()),
            size_scale: scale,
            ..Default::default()
        };
        if let Some(tb) = sh.tx_body.as_deref() {
            if let Some(warp) = &tb.x().warp {
                if !matches!(tb.x().vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl")) {
                    let single = matches!(&*warp.prst, "textArchUp" | "textArchDown" | "textCircle" | "textButton");
                    let mut body = tb.clone();
                    if single {
                        body.wrap = Some("none".into());
                    }
                    calls.push(Call { id: format!("{k}"), body, w, h, measurer: Measurer::Plain, opts: opts(None) });
                }
            } else {
                let turn = matches!(tb.x().vert.as_deref(), Some("vert" | "eaVert" | "vert270"));
                let mut body = tb.clone();
                if turn {
                    body.x_mut().vert = None;
                }
                let (bw, bh) = if turn { ((cy * s) / s, (cx * s) / s) } else { (w, h) };
                calls.push(Call { id: format!("{k}"), body: body.clone(), w: bw, h: bh, measurer: Measurer::Ppt, opts: opts(Some(1.0)) });
                if k % 5 == 0 {
                    calls.push(Call { id: format!("{k}.x"), body, w: bw, h: bh, measurer: Measurer::Ppt, opts: opts(Some(1.5)) });
                }
            }
        }
        if let Some(t) = sh.extras.as_ref().and_then(|e| e.table.as_deref()) {
            for (ri, row) in t.rows.iter().enumerate() {
                for (ci, cell) in row.cells.iter().enumerate() {
                    if cell.h_merge == Some(true) || cell.v_merge == Some(true) {
                        continue;
                    }
                    let Some(body) = cell_body(cell) else { continue };
                    let span = cell.grid_span.unwrap_or(1.0);
                    let mut cw = 0.0;
                    let mut q = ci as f64;
                    while q < ci as f64 + span {
                        cw += t.col_widths_emu.get(q as usize).copied().unwrap_or(0.0);
                        q += 1.0;
                    }
                    let o = LayoutTextOptions { default_sz_pt: Some(18.0), default_anchor: Some("t".into()), ..Default::default() };
                    calls.push(Call { id: format!("{k}.c{ri}.{ci}"), body, w: cw, h: 1e12, measurer: Measurer::Ppt, opts: o });
                }
            }
        }
    }
    calls
}

// ---------------------------------------------------------------- replay

struct Totals {
    calls: usize,
    two_phase: usize,
    max_rounds: u32,
}

fn replay(capture: &str, log_file: &str, font_kerning: bool, segmenter: bool, two_phase_every: usize) -> (Totals, uo_testkit::Report) {
    let cap = Capture::read(captures().join(capture)).unwrap();
    let log = load_log(log_file);
    let table = table_from(&log, font_kerning, segmenter);
    let by_key: std::collections::HashMap<&str, &Entry> = log.iter().map(|e| (e.key.as_str(), e)).collect();
    let mut rp = Replay::new(&cap);
    let mut tot = Totals { calls: 0, two_phase: 0, max_rounds: 0 };
    for rec in cap.records() {
        let key = rec.key.as_str();
        let bytes = std::fs::read(deck_path(key)).unwrap_or_else(|e| panic!("{key}: {e}"));
        let _ = bytes_digest(&bytes);
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{key}: {e}"));
        let pres = deck.presentation().unwrap().clone();
        rp.check(key, "read", "ok");
        rp.check(key, "slides", &pres.slides.len().to_string());
        for i in 0..pres.slides.len() {
            let model = deck.slide(u32::try_from(i).unwrap()).unwrap();
            let src = deck.source();
            let canon = Canon { src: &src };
            let calls = slide_calls(&model.shapes, pres.slide_w);
            rp.check(key, &format!("s{i}.calls"), &calls.len().to_string());
            for c in &calls {
                let id = format!("t{i}.{}", c.id);
                rp.check(key, &format!("{id}.in"), &in_facet(c));
                let (res, asked) = layout_text_body_traced(&c.body, c.w, c.h, c.measurer, &table, &c.opts);
                tot.calls += 1;
                let out = match &res {
                    Ok(r) => {
                        let text = canon.layout(r).to_text();
                        if let Ok(dir) = std::env::var("UO_PPTX_DUMP") {
                            std::fs::create_dir_all(&dir).unwrap();
                            std::fs::write(Path::new(&dir).join(format!("{}__{id}.json", key.replace('/', "__"))), &text).unwrap();
                        }
                        text_digest(&text)
                    }
                    Err(LayoutError::Throws(name)) => format!("err:{name}"),
                    Err(LayoutError::NeedMeasures(n)) => format!("need:{} first {:?}", n.0.len(), n.0.first()),
                };
                // a throwing layout asks nothing the TS logged before throwing only when it throws first (displayRuns)
                rp.check(key, &format!("{id}.ask"), &asks_digest(&asked, font_kerning));
                rp.check(key, &id, &out);
                // the two-phase protocol from an empty table, the shell answering from the log (questions the true
                // path never asks are answered with 0: they only steer a discarded pass)
                if two_phase_every > 0 && tot.calls.is_multiple_of(two_phase_every) && res.is_ok() {
                    let mut t = MeasureTable::with_cap(usize::MAX, usize::MAX);
                    t.caps = table.caps;
                    let mut rounds = 0;
                    loop {
                        let need = need_measures(&c.body, c.w, c.h, c.measurer, &t, &c.opts);
                        if need.is_empty() {
                            break;
                        }
                        rounds += 1;
                        assert!(rounds <= MAX_ROUNDS, "{key} {id}: more than {MAX_ROUNDS} rounds");
                        for r in &need {
                            answer(&mut t, r, font_kerning, &by_key);
                        }
                    }
                    let again = layout_text_body(&c.body, c.w, c.h, c.measurer, &t, &c.opts).unwrap();
                    assert_eq!(text_digest(&canon.layout(&again).to_text()), out, "{key} {id}: two-phase result");
                    tot.two_phase += 1;
                    tot.max_rounds = tot.max_rounds.max(rounds);
                }
            }
        }
    }
    (tot, rp.finish())
}

fn answer(t: &mut MeasureTable, r: &MeasureReq, font_kerning: bool, log: &std::collections::HashMap<&str, &Entry>) {
    let mut keys = Vec::new();
    raw_keys(r, font_kerning, &mut keys);
    let w: Vec<f64> = keys.iter().map(|k| log.get(k.as_str()).map_or(0.0, |e| e.width)).collect();
    match r {
        MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, w[0]),
        MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, w[0], w[1]),
        MeasureReq::Width(k) => t.set_width(k, w[0]),
        other => panic!("unexpected request {other:?}"),
    }
}

fn run(capture: &str, log: &str, kerning: bool, seg: bool, every: usize, want_calls: usize) {
    let t0 = std::time::Instant::now();
    let (tot, report) = replay(capture, log, kerning, seg, every);
    eprintln!(
        "{capture}: {} layout calls, {} two-phase replays (max {} rounds), {:.1} s, {}",
        tot.calls,
        tot.two_phase,
        tot.max_rounds,
        t0.elapsed().as_secs_f64(),
        report.summary()
    );
    assert!(report.mismatches.is_empty() && report.missing.is_empty(), "{}\n{}", report.summary(), report.describe(20));
    assert_eq!(tot.calls, want_calls, "the capture shrank or grew");
}

#[test]
fn samples_match_the_ts() {
    run("textlayout-samples.capture", "textlayout-samples.measures.tsv.gz", true, true, 7, 1682);
}

#[test]
fn b1a_generated_decks_match_the_ts() {
    run("textlayout-b1a.capture", "textlayout-b1a.measures.tsv.gz", true, true, 13, 5100);
}

#[test]
fn b3a_text_heavy_decks_match_the_ts() {
    run("textlayout-b3a.capture", "textlayout-b3a.measures.tsv.gz", true, true, 11, 2807);
}

#[test]
fn b3a_without_kerning_and_segmenter_match_the_ts() {
    run("textlayout-b3a-nokerning-noseg.capture", "textlayout-b3a-nokerning-noseg.measures.tsv.gz", false, false, 17, 2807);
}
