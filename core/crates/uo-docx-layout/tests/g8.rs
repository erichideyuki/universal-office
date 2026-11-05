//! G8 — the Write section of the page message (record "Write page section (G8)").
//!
//! - **Parity**: for every document of a capture written by `tools/capture-layout.mjs --g8`, lay
//!   it out through the real Rust engine with the transcript policy (as `f8_parity.rs`), encode
//!   every page with `Layout::encode_page`, decode the render message (uo-render's generated
//!   codec) and its `app` with the generated `write.uos` decoder, and write from the DECODED
//!   values the stream `tools/lib/facets-g8.mjs` writes from the frozen TS LayoutPage and the
//!   viewer's `para(ref)` / `levelResolver` (`g8.section`). P1 and the page count are compared too,
//!   so a section mismatch over an unequal layout is reported as such.
//! - **Round trip**: the decoded `WritePage` re-encodes to the very `app` bytes, equals
//!   `Layout::write_page`, and the render part equals uo-render's encoder without a section.
//!
//! Committed fixtures: `tests/captures/g8/<f7a|f7b|f7d|f8>.capture` over the documents and
//! transcripts of `tests/captures/<same>/`. `UO_G8_PARITY=1 UO_LAYOUT_CAPTURE=… UO_LAYOUT_DUMP=…
//! UO_LAYOUT_DOCS=…` replays any capture (the oracle slice of the record).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{FontIndex, StreamSink, Transcript, TranscriptProvider, load_answers};
use uo_codec::Limits;
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::codec as wc;
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::model::DocView;
use uo_docx_layout::options::{Clock, RevisionMode, RevisionView, TimeZoneRule};
use uo_docx_layout::section::{RunNames, SectionModel};
use uo_docx_layout::{Layout, LayoutOptions};
use uo_render::DeltaMark;
use uo_testkit::{Capture, Replay, Stream};

const NOW_MS: f64 = 1_768_478_400_000.0;

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

fn options_for(key: &str) -> LayoutOptions {
    let base = key.rsplit('/').next().unwrap_or(key);
    LayoutOptions {
        clock: Clock { now_ms: NOW_MS, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        revisions: base.starts_with("mk-").then(|| RevisionView { mode: RevisionMode::All, ..RevisionView::default() }),
        ..LayoutOptions::default()
    }
}

fn read_doc(docx: &[u8]) -> uo_docx::model::Document {
    let pkg = uo_opc::Package::open(docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).expect("package");
    uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).expect("document")
}

fn lay(view: DocView<'_>, provider: &dyn FontProvider, o: &LayoutOptions) -> Result<Layout, LayoutError> {
    let ldoc = uo_docx_layout::derive::build(view, o)?;
    let mut hyph = HyphenSessions::new(hyph_registry());
    uo_docx_layout::engine::layout_document(&ldoc, o, provider, &mut hyph)
}

// ───────────────────────── the stream (tools/lib/sink.mjs) ─────────────────────────

struct S(Stream);

impl S {
    fn tag(&mut self, c: u8) -> &mut S {
        self.0.tag(c);
        self
    }
    fn u32(&mut self, v: u32) -> &mut S {
        self.0.u32(v);
        self
    }
    fn num(&mut self, v: f64) -> &mut S {
        self.0.f64(if v.is_nan() { f64::NAN } else { v });
        self
    }
    fn bool(&mut self, v: bool) -> &mut S {
        self.0.bool(v);
        self
    }
    fn u16s(&mut self, u: &[u16]) -> &mut S {
        let b: Vec<u8> = u.iter().flat_map(|c| c.to_le_bytes()).collect();
        self.0.bytes(&b);
        self
    }
    fn str(&mut self, s: &str) -> &mut S {
        let u: Vec<u16> = s.encode_utf16().collect();
        self.u16s(&u)
    }
    fn opt_num(&mut self, v: Option<f64>) -> &mut S {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.num(v);
        }
        self
    }
    fn opt_u16s(&mut self, v: Option<&[u16]>) -> &mut S {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.u16s(v);
        }
        self
    }
    fn value(self) -> String {
        let n = self.0.items();
        format!("{n}:{}", self.0.finish().short())
    }
}

fn align_name(a: wc::TabAlign) -> &'static str {
    match a {
        wc::TabAlign::Left => "left",
        wc::TabAlign::Center => "center",
        wc::TabAlign::Right => "right",
        wc::TabAlign::Decimal => "decimal",
        wc::TabAlign::Bar => "bar",
        wc::TabAlign::Clear => "clear",
        wc::TabAlign::Num => "num",
    }
}

fn leader_name(l: wc::TabLeader) -> &'static str {
    match l {
        wc::TabLeader::None => "none",
        wc::TabLeader::Dot => "dot",
        wc::TabLeader::Hyphen => "hyphen",
        wc::TabLeader::Underscore => "underscore",
        wc::TabLeader::Heavy => "heavy",
        wc::TabLeader::MiddleDot => "middleDot",
    }
}

fn line(s: &mut S, l: &wc::LaidOutLine, pfmts: &[wc::LineParaFormat]) {
    s.tag(b'n').u16s(&l.r#ref).num(f64::from(l.start)).num(f64::from(l.end));
    for v in [l.x, l.y, l.height, l.baseline, l.width] {
        s.num(v);
    }
    s.bool(l.chars.is_some());
    if let Some(c) = &l.chars {
        s.u32(u32::try_from(c.o.len()).unwrap());
        for k in 0..c.o.len() {
            s.num(f64::from(c.o[k])).num(c.x[k]).num(c.w[k]);
        }
    }
    s.opt_num(l.hyph.map(f64::from));
    s.bool(l.rev.is_some());
    if let Some(r) = &l.rev {
        s.num(r.before).num(r.after);
    }
    s.bool(l.frame.is_some());
    if let Some(f) = &l.frame {
        s.num(f.l).num(f.r);
    }
    // `pfmt` is by value in the TS stream (an index into the page's formats on the wire)
    let pf = l.pfmt.map(|k| &pfmts[k as usize]);
    s.bool(pf.is_some());
    if let Some(pf) = pf {
        s.num(pf.left).num(pf.right).num(pf.first_line).u32(u32::try_from(pf.tabs.len()).unwrap());
        for t in &pf.tabs {
            s.str(align_name(t.val)).num(t.pos).bool(t.leader.is_some());
            if let Some(ld) = t.leader {
                s.str(leader_name(ld));
            }
        }
    }
}

/// The `g8.section` stream of tools/lib/facets-g8.mjs, from decoded sections.
fn section_stream(pages: &[wc::WritePage]) -> String {
    let mut s = S(Stream::new());
    s.tag(b'W').u32(u32::try_from(pages.len()).unwrap());
    for p in pages {
        s.tag(b'P').u32(p.section).num(p.number).opt_num(p.hf_items.map(f64::from));
        s.u32(u32::try_from(p.lines.len()).unwrap());
        let mut first: HashMap<u32, usize> = HashMap::new();
        for (i, l) in p.lines.iter().enumerate() {
            line(&mut s, l, &p.pfmts);
            s.bool(l.para.is_some());
            let Some(k) = l.para else { continue };
            match first.get(&k) {
                Some(&f) => {
                    s.bool(false).u32(u32::try_from(f).unwrap());
                }
                None => {
                    first.insert(k, i);
                    let tp = &p.paras[k as usize];
                    s.bool(true).u16s(&tp.text).opt_num(tp.level);
                }
            }
        }
        s.bool(p.objects.is_some());
        if let Some(os) = &p.objects {
            s.u32(u32::try_from(os.len()).unwrap());
            for o in os {
                s.tag(b'o').opt_u16s(o.run.as_deref());
                for v in [o.x, o.y, o.w, o.h] {
                    s.num(v);
                }
                s.str(if o.layer == wc::Layer::Behind { "behind" } else { "front" }).bool(o.hf);
                s.bool(o.frame.is_some());
                if let Some(f) = &o.frame {
                    for v in [f.col_x, f.col_w, f.para_top, f.page_w, f.page_h, f.mar_l, f.mar_r, f.mar_t, f.mar_b, f.body_top] {
                        s.num(v);
                    }
                }
                s.bool(o.content.is_some());
                if let Some(c) = &o.content {
                    s.u32(u32::try_from(c.lines.len()).unwrap());
                    for l in &c.lines {
                        assert!(l.para.is_none(), "textbox content lines have no text-layer paragraph");
                        line(&mut s, l, &p.pfmts);
                    }
                    for v in &c.xf {
                        s.num(*v);
                    }
                }
            }
        }
        s.bool(p.cells.is_some());
        if let Some(cs) = &p.cells {
            s.u32(u32::try_from(cs.len()).unwrap());
            for c in cs {
                s.tag(b'k').u16s(&c.r#ref).num(c.x).num(c.y).num(c.w).num(c.h).num(c.span);
            }
        }
        let mut clips: Vec<u32> = p.clip_src.iter().map(|c| c.item).collect();
        clips.sort_unstable();
        s.u32(u32::try_from(clips.len()).unwrap());
        for i in clips {
            s.u32(i);
        }
    }
    s.value()
}

/// Every page through `Layout::encode_page` → render decode → Write decode, with the round-trip
/// checks of the module doc. Returns the decoded sections.
pub fn encode_decode_all(layout: &Layout, m: &SectionModel, generation: u32) -> Vec<wc::WritePage> {
    let limits = Limits::default();
    let mut mark = DeltaMark::new(generation);
    let mut plain = DeltaMark::new(generation);
    let mut rt = uo_render::codec::Tables::new();
    let mut wt = wc::Tables::new();
    let mut got = Vec::new();
    for i in 0..layout.pages.len() {
        let mut out = Vec::new();
        layout.encode_page(i, m, &mut mark, &mut out).unwrap();
        let page = uo_render::codec::decode_page(&out, &mut rt, &limits).unwrap();
        // the render part is uo-render's own encoding of the page (the section rides as `app` only)
        let mut bare = Vec::new();
        uo_render::encode_page_for_canvas(&layout.pages[i].page, &layout.resources, &page.app, &mut plain, &mut bare).unwrap();
        assert_eq!(bare, out, "page {i}: the message is uo-render's with the section as app");
        let w = wc::decode_write_page(&page.app, &mut wt, &limits).unwrap();
        assert_eq!(wt.generation, Some(generation));
        let mut again = Vec::new();
        wc::encode_write_page(&mut again, &wc::Delta { generation }, &w).unwrap();
        assert_eq!(again, page.app, "page {i}: the Write section re-encodes to its bytes");
        assert_eq!(Some(&w), layout.write_page(i, m).as_ref(), "page {i}: decoded = the wire value");
        got.push(w);
    }
    // past the end: refused, nothing written, the mark kept
    let mut out = Vec::new();
    let before = mark.sent();
    assert!(layout.encode_page(layout.pages.len(), m, &mut mark, &mut out).is_err());
    assert!(out.is_empty() && mark.sent() == before);
    got
}

#[derive(Default, Debug)]
struct Tally {
    docs: usize,
    equal: usize,
    p1_differs: Vec<String>,
    section_differs: Vec<String>,
    annotations_differs: Vec<String>,
    pages: usize,
    paras: usize,
    lines: usize,
    section_bytes: usize,
    render_bytes: usize,
}

fn replay(capture: &Path, dump: &Path, docs: &Path) -> (Tally, uo_testkit::Report) {
    let cap = Capture::read(capture).unwrap();
    let mut fonts = FontIndex::open_fonts();
    let mut replay = Replay::new(&cap);
    let mut t = Tally::default();
    for rec in cap.records() {
        let skip_others = |replay: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                if !matches!(f.as_str(), "p1" | "pages" | "g8.section" | "g8.annotations") {
                    replay.skip(&rec.key, f);
                }
            }
        };
        skip_others(&mut replay);
        if rec.get("g8.section").is_none() || rec.get("error").is_some() {
            for f in ["p1", "pages", "g8.section", "g8.annotations", "error"] {
                if rec.get(f).is_some() {
                    replay.skip(&rec.key, f);
                }
            }
            continue;
        }
        let path = [docs.join(format!("{}.docx", safe(&rec.key))), docs.join(format!("{}.docx", rec.key))].into_iter().find(|p| p.exists());
        let docx = std::fs::read(path.unwrap_or_else(|| panic!("{}: docx not in {}", rec.key, docs.display()))).unwrap();
        let tr = Transcript::read(&dump.join(format!("{}.transcript.tsv", safe(&rec.key)))).unwrap();
        fonts.add_transcript_sources(&tr);
        if let Ok(pkg) = uo_opc::Package::open(&docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
            && let Ok(list) = uo_docx::read::read_embedded_fonts(&pkg)
        {
            for f in list {
                fonts.add_bytes(f.data.clone());
            }
        }
        let loaded = load_answers(&tr, &fonts);
        assert!(loaded.missing.is_empty(), "{}: faces not found by digest: {:?}", rec.key, loaded.missing);
        let provider = TranscriptProvider::new(&tr, loaded);
        let doc = read_doc(&docx);
        let view = DocView::new(&doc);
        t.docs += 1;
        let layout = lay(view, &provider, &options_for(&rec.key)).unwrap_or_else(|e| panic!("{}: {e}", rec.key));
        let names = RunNames::new(view);
        let mut sink = StreamSink(Stream::new());
        canon_layout(&layout, &|r| names.name(r).map(|s| s.encode_utf16().collect()), &mut sink);
        let p1 = replay.check(&rec.key, "p1", &sink.value()) & replay.check(&rec.key, "pages", &layout.pages.len().to_string());
        if !p1 {
            t.p1_differs.push(rec.key.clone());
        }
        let m = SectionModel::new(view, &layout);
        let pages = encode_decode_all(&layout, &m, 1);
        t.pages += pages.len();
        t.paras += pages.iter().map(|p| p.paras.len()).sum::<usize>();
        t.lines += pages.iter().map(|p| p.lines.len()).sum::<usize>();
        for i in 0..layout.pages.len() {
            t.section_bytes += layout.write_section(i, &m, 1).unwrap().0.len();
            let mut bare = Vec::new();
            uo_render::encode_page_for_canvas(&layout.pages[i].page, &layout.resources, &uo_render::NoApp, &mut DeltaMark::new(1), &mut bare)
                .unwrap();
            t.render_bytes += bare.len();
        }
        let sec = replay.check(&rec.key, "g8.section", &section_stream(&pages));
        if !sec {
            t.section_differs.push(rec.key.clone());
        }
        let ann = match rec.get("g8.annotations") {
            Some(_) => {
                let ok = replay.check(&rec.key, "g8.annotations", &annotations_stream(view, &layout));
                if !ok {
                    t.annotations_differs.push(rec.key.clone());
                }
                ok
            }
            None => true,
        };
        if p1 && sec && ann {
            t.equal += 1;
        }
    }
    (t, replay.finish())
}

/// The `g8.annotations` stream of tools/lib/facets-g8.mjs from the Rust `annotations` query.
fn annotations_stream(view: DocView<'_>, layout: &Layout) -> String {
    let a = layout.annotations(view);
    let mut s = S(Stream::new());
    s.tag(b'A').u32(u32::try_from(a.comments.len()).unwrap());
    for c in &a.comments {
        s.tag(b'c').num(c.id).opt_u16s(c.author.as_deref()).u16s(&c.text).u32(u32::try_from(c.rects.len()).unwrap());
        for r in &c.rects {
            s.u32(r.page).num(r.x).num(r.y).num(r.w).num(r.h);
        }
    }
    s.u32(u32::try_from(a.headings.len()).unwrap());
    for h in &a.headings {
        s.tag(b'h').num(h.level).u16s(&h.text).u32(h.page).num(h.y);
    }
    s.value()
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn g8_committed_fixtures() {
    let mut total = Tally::default();
    for name in ["f7a", "f7b", "f7d", "f8"] {
        let cap = manifest().join(format!("tests/captures/g8/{name}.capture"));
        if !cap.exists() {
            eprintln!("g8 fixture {name} absent");
            continue;
        }
        let dir = manifest().join(format!("tests/captures/{name}"));
        let (t, report) = replay(&cap, &dir, &dir.join("docs"));
        eprintln!(
            "g8 {name}: {}; docs {}, equal {}, pages {}, lines {}, paras {}, section {} B, render {} B; P1 differs {:?}, section differs {:?}, annotations differ {:?}",
            report.summary(),
            t.docs,
            t.equal,
            t.pages,
            t.lines,
            t.paras,
            t.section_bytes,
            t.render_bytes,
            t.p1_differs,
            t.section_differs,
            t.annotations_differs
        );
        report.assert_clean();
        assert_eq!(t.equal, t.docs, "{name}: every document equal (P1, section, annotations)");
        total.docs += t.docs;
        total.pages += t.pages;
    }
    assert!(total.docs > 0);
}

#[test]
fn g8_parity_against_the_frozen_ts() {
    if !uo_testkit::opt_in("UO_G8_PARITY", "G8 replay (needs a --g8 capture, its dump and docs)") {
        return;
    }
    let var = |n: &str| PathBuf::from(std::env::var(n).unwrap_or_else(|_| panic!("{n} is required")));
    let (t, report) = replay(&var("UO_LAYOUT_CAPTURE"), &var("UO_LAYOUT_DUMP"), &var("UO_LAYOUT_DOCS"));
    eprintln!(
        "g8 parity: {}; docs {}, equal {}, pages {}, lines {}, paras {}, section {} B, render {} B",
        report.summary(),
        t.docs,
        t.equal,
        t.pages,
        t.lines,
        t.paras,
        t.section_bytes,
        t.render_bytes
    );
    eprintln!("P1 differs ({}): {:?}", t.p1_differs.len(), t.p1_differs);
    let p1: std::collections::HashSet<&String> = t.p1_differs.iter().collect();
    let own: Vec<&String> = t.section_differs.iter().filter(|k| !p1.contains(k)).collect();
    eprintln!("section differs ({}; {} over an equal P1): {:?}", t.section_differs.len(), own.len(), own);
    let own_a: Vec<&String> = t.annotations_differs.iter().filter(|k| !p1.contains(k)).collect();
    eprintln!("annotations differ ({}; {} over an equal P1): {:?}", t.annotations_differs.len(), own_a.len(), own_a);
    assert!(own.is_empty() && own_a.is_empty(), "a section or annotations mismatch over an equal layout");
}
