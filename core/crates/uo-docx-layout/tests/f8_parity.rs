//! The P4 replay of frontier F8 (record "Stage 3 — F8"): for every document of a capture written
//! by `tools/capture-layout.mjs --f8`, read the `.docx` with the uo-docx reader, lay it out
//! through the real Rust engine with the transcript policy and the capture's options (plus
//! `collect_fields`), and compute every `p4.*` facet of `tools/lib/facets-f8.mjs` with the Rust
//! queries — the same samples, the same stream encoding. P1, `pages` and `p2first` of the same
//! records are compared too (a P4 difference over an unequal layout is reported as such).
//!
//! The committed fixture (`tests/captures/f8`, `--gen f8` seed 7, open fonts) runs in
//! `cargo test`; `UO_F8_PARITY=1 UO_LAYOUT_CAPTURE=… UO_LAYOUT_DUMP=… UO_LAYOUT_DOCS=…` replays
//! any capture (the oracle run of the record).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{FontIndex, StreamSink, Transcript, TranscriptProvider, load_answers};
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hit::{CharsView, EditLines, EditPos, EditScope, LineIndex, SelRect, caret_x_in, cell_at, offset_at_x};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::model::{
    Block, BlockPath, DocView, DrawingId, FieldScope, HfPartId, HfWhich, HyphenSkips, NoteKind, NumKey, RunRef, RunSub, StoryRef,
};
use uo_docx_layout::options::{Clock, Hyphenation, RevisionMode, RevisionView, TimeZoneRule};
use uo_docx_layout::refs::{LocKind, Locator, ParaLoc, para_length};
use uo_docx_layout::run_boxes::{RunFilter, run_boxes_with};
use uo_docx_layout::{Layout, LayoutOptions};
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

// ───────────────────────── the stream (tools/lib/sink.mjs) ─────────────────────────

struct S(Stream);

impl S {
    fn new() -> S {
        S(Stream::new())
    }
    fn traced() -> S {
        S(Stream::traced())
    }
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
    /// The value, and with `UO_F8_TRACE=<dir>` the item trace as `<key>.rust.trace`.
    fn value_traced(self, key: &str) -> String {
        if let Ok(d) = std::env::var("UO_F8_TRACE") {
            let _ = std::fs::write(Path::new(&d).join(format!("{}.rust.trace", safe(key))), self.0.trace());
        }
        self.value()
    }
    fn value(self) -> String {
        let n = self.0.items();
        format!("{n}:{}", self.0.finish().short())
    }
}

// ───────────────────────── the layout ─────────────────────────

struct Recording<'p> {
    inner: &'p TranscriptProvider,
    labels: HashMap<(String, bool, bool), u32>,
    calls: Mutex<Vec<(String, bool, bool, u32)>>,
}

impl FontProvider for Recording<'_> {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let label = self.labels.get(&(family.to_owned(), bold, italic)).copied().unwrap_or(0);
        if let Ok(mut c) = self.calls.lock() {
            c.push((family.to_owned(), bold, italic, label));
        }
        self.inner.face(family, bold, italic)
    }
}

fn read_doc(docx: &[u8]) -> uo_docx::model::Document {
    let pkg = uo_opc::Package::open(docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).expect("package");
    uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).expect("document")
}

fn lay(view: DocView<'_>, provider: &dyn FontProvider, o: &LayoutOptions) -> Result<(Layout, RunNames), LayoutError> {
    let ldoc = uo_docx_layout::derive::build(view, o)?;
    let mut hyph = HyphenSessions::new(hyph_registry());
    let names = run_names(&ldoc);
    uo_docx_layout::engine::layout_document(&ldoc, o, provider, &mut hyph).map(|l| (l, names))
}

/// `runRefs(full)` of tools/lib/canon.mjs (the P1 names of `PlacedObject.run`; as layout_parity.rs).
type RunNames = HashMap<RunRef, Vec<u16>>;

fn run_names(ldoc: &uo_docx_layout::ldoc::LDoc<'_>) -> RunNames {
    use uo_docx_layout::ldoc::RunSrc;
    let view = ldoc.view;
    let mut hf: HashMap<HfPartId, usize> = HashMap::new();
    for sec in view.sections() {
        for set in [&sec.headers, &sec.footers] {
            for k in ["default", "first", "even"] {
                if let Some(&p) = set.get(k) {
                    let n = hf.len();
                    hf.entry(p).or_insert(n);
                }
            }
        }
    }
    let mut tb_owner: HashMap<DrawingId, (RunRef, Option<usize>)> = HashMap::new();
    for r in &ldoc.runs {
        let RunSrc::Model { run, at } = r else { continue };
        let Some(e) = run.ext() else { continue };
        if let Some(tb) = &e.textbox {
            tb_owner.entry(tb.drawing).or_insert((at.clone(), None));
        }
        if let Some(g) = &e.group {
            for (j, it) in g.items.iter().enumerate() {
                if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_ref()) {
                    tb_owner.entry(tb.drawing).or_insert((at.clone(), Some(j)));
                }
            }
        }
    }
    fn name(
        at: &RunRef,
        hf: &HashMap<HfPartId, usize>,
        tb_owner: &HashMap<DrawingId, (RunRef, Option<usize>)>,
        depth: u32,
    ) -> Option<String> {
        if depth > 16 {
            return None;
        }
        let prefix = match at.story {
            StoryRef::Body => String::new(),
            StoryRef::HeaderFooter(p) => format!("hf{}", hf.get(&p)?),
            StoryRef::Note(kind, id) => {
                format!("{}{}", if kind == NoteKind::Footnote { "fn" } else { "en" }, uo_jsnum::format::to_js_string(id.get()))
            }
            StoryRef::Textbox(d) => {
                let (owner, j) = tb_owner.get(&d)?;
                let base = name(owner, hf, tb_owner, depth + 1)?;
                match j {
                    Some(j) => format!("{base}/g{j}#0/tx"),
                    None => format!("{base}/tx"),
                }
            }
            StoryRef::Context => return None,
        };
        let mut path = String::new();
        for (i, seg) in at.path.steps().iter().enumerate() {
            if i > 0 || !prefix.is_empty() {
                path.push('/');
            }
            path.push(match i % 3 {
                0 => 'b',
                1 => 'r',
                _ => 'c',
            });
            path.push_str(&seg.to_string());
        }
        let base = format!("{prefix}{path}#{}", at.run);
        match at.sub {
            RunSub::None => Some(base),
            RunSub::GroupItem(j) => Some(format!("{base}/g{j}#0")),
            RunSub::FieldResult(_) => None,
        }
    }
    let mut out = RunNames::new();
    for r in &ldoc.runs {
        let RunSrc::Model { at, .. } = r else { continue };
        if let Some(n) = name(at, &hf, &tb_owner, 0) {
            out.entry(at.clone()).or_insert_with(|| n.encode_utf16().collect());
        }
    }
    out
}

// ───────────────────────── names (facets-f8.mjs `names`) ─────────────────────────

struct Names {
    hf: HashMap<HfPartId, String>,
    hf_order: Vec<(HfPartId, HfWhich)>,
    /// a textbox's content story: `<owner run>/tx`
    tb: HashMap<DrawingId, String>,
}

impl Names {
    fn new(view: DocView<'_>) -> Names {
        let mut hf = HashMap::new();
        let mut hf_order = Vec::new();
        for sec in view.sections() {
            for (set, which) in [(&sec.headers, HfWhich::Header), (&sec.footers, HfWhich::Footer)] {
                for k in ["default", "first", "even"] {
                    if let Some(&p) = set.get(k)
                        && !hf.contains_key(&p)
                    {
                        hf.insert(p, format!("hf{}", hf.len()));
                        hf_order.push((p, which));
                    }
                }
            }
        }
        let mut n = Names { hf, hf_order, tb: HashMap::new() };
        // textbox owners, in the TS walk order (body, then parts, then notes; depth first)
        fn walk(n: &mut Names, s: &uo_docx::model::Story, story: &str, base: Option<(&BlockPath, u32, u32)>) {
            for (i, b) in s.blocks().enumerate() {
                let path = match base {
                    None => BlockPath::root(i as u32),
                    Some((p, r, c)) => p.cell_block(r, c, i as u32),
                };
                match b {
                    Block::Paragraph(p) => {
                        for (k, r) in p.runs.iter().enumerate() {
                            let rf = format!("{story}:{path}#{k}");
                            let Some(e) = r.ext() else { continue };
                            if let Some(t) = e.textbox.as_deref() {
                                let st = format!("{rf}/tx");
                                n.tb.entry(t.drawing).or_insert_with(|| st.clone());
                                walk(n, &t.content, &st, None);
                            }
                            for (j, it) in e.group.iter().flat_map(|g| g.items.iter()).enumerate() {
                                if let Some(t) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                                    let st = format!("{rf}/g{j}/tx");
                                    n.tb.entry(t.drawing).or_insert_with(|| st.clone());
                                    walk(n, &t.content, &st, None);
                                }
                            }
                        }
                    }
                    Block::Table(t) => {
                        for (r, row) in t.rows.iter().enumerate() {
                            for (c, cell) in row.cells.iter().enumerate() {
                                walk(n, &cell.content, story, Some((&path, r as u32, c as u32)));
                            }
                        }
                    }
                }
            }
        }
        walk(&mut n, view.body(), "b", None);
        let parts: Vec<(HfPartId, String)> = n.hf_order.iter().map(|(p, _)| (*p, n.hf[p].clone())).collect();
        for (p, name) in parts {
            if let Some(s) = view.hf_story(p) {
                walk(&mut n, s, &name, None);
            }
        }
        for (kind, map) in [("fn", &view.notes().footnotes), ("en", &view.notes().endnotes)] {
            for (id, s) in map {
                walk(&mut n, s, &format!("{kind}{}", uo_jsnum::format::to_js_string(id.get())), None);
            }
        }
        n
    }

    fn story(&self, s: &StoryRef) -> Option<String> {
        match s {
            StoryRef::Body => Some("b".into()),
            StoryRef::HeaderFooter(p) => self.hf.get(p).cloned(),
            StoryRef::Note(k, id) => {
                Some(format!("{}{}", if *k == NoteKind::Footnote { "fn" } else { "en" }, uo_jsnum::format::to_js_string(id.get())))
            }
            StoryRef::Textbox(d) => self.tb.get(d).cloned(),
            StoryRef::Context => None,
        }
    }

    fn para(&self, p: &ParaLoc) -> String {
        match self.story(&p.story) {
            Some(s) => format!("{s}:{}", p.path),
            None => "?".into(),
        }
    }

    /// `<para>#<run>` (+ `/f<j>` for a field result run); `None` = not a named model run.
    fn run(&self, r: &RunRef) -> Option<String> {
        let s = self.story(&r.story)?;
        let base = format!("{s}:{}#{}", r.path, r.run);
        match r.sub {
            RunSub::None => Some(base),
            RunSub::FieldResult(j) => Some(format!("{base}/f{j}")),
            RunSub::GroupItem(j) => Some(format!("{base}/g{j}")),
        }
    }
}

// ───────────────────────── samples (facets-f8.mjs) ─────────────────────────

fn sample_pages(n: usize) -> Vec<usize> {
    if n <= 6 { (0..n).collect() } else { vec![0, 1, 2, n - 3, n - 2, n - 1] }
}

fn sample_points(p: &uo_docx_layout::LayoutPage) -> Vec<(f64, f64)> {
    let (w, h) = (p.page.w, p.page.h);
    let mut out = Vec::new();
    for y in [0.0, h * 0.05, h * 0.2, h * 0.35, h * 0.5, h * 0.65, h * 0.8, h * 0.95, h] {
        for x in [-100.0, w * 0.13, w * 0.37, w * 0.61, w * 0.89, w + 100.0] {
            out.push((x, y));
        }
    }
    for l in p.lines.iter().take(40) {
        out.push((l.x - 10.0, l.y + l.height / 2.0));
        out.push((l.x + l.width * 0.31, l.y + l.height / 2.0));
        out.push((l.x + l.width + 10.0, l.y - 5.0));
    }
    out.retain(|&(x, y)| x >= 0.0 && x <= w && y >= 0.0 && y <= h);
    out
}

fn sample_offsets(len: u32, lines: &[uo_docx_layout::hit::EditLine]) -> Vec<u32> {
    let mut s: std::collections::BTreeSet<u32> = (0..=len.min(12)).collect();
    s.insert(len);
    for l in lines {
        s.insert(l.start);
        s.insert(l.end);
    }
    s.into_iter().collect()
}

fn sample_ranges(lens: &[u32]) -> Vec<(EditPos, EditPos)> {
    let n = lens.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    let p = |para: usize, offset: u32| EditPos { para: para as u32, offset };
    out.push((p(0, 0), p(n - 1, lens[n - 1])));
    let mut i = 0;
    while i + 1 < n && i < 25 {
        out.push((p(i, lens[i].min(1)), p(i + 1, lens[i + 1].min(2))));
        i += 1;
    }
    for (i, &len) in lens.iter().enumerate().take(25) {
        out.push((p(i, 0), p(i, len)));
    }
    out
}

// ───────────────────────── the facets ─────────────────────────

fn chars_of<'a>(p: &'a uo_docx_layout::LayoutPage, l: &uo_docx_layout::output::LineGeom) -> CharsView<'a> {
    CharsView::of(p, l)
}

fn kind_name(k: LocKind) -> &'static str {
    match k {
        LocKind::Body => "body",
        LocKind::Cell => "cell",
        LocKind::HeaderFooter => "hf",
        LocKind::Note => "note",
        LocKind::Other => "other",
    }
}

/// Every p4 facet of one layout but the extra layouts.
fn editor_facets(view: DocView<'_>, layout: &Layout, key: &str) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let names = Names::new(view);
    let loc = Locator::new(view);

    // p4.caret
    {
        let mut s = S::new();
        let mut n = 0;
        'pages: for p in &layout.pages {
            for l in &p.lines {
                if n >= 400 {
                    continue 'pages;
                }
                n += 1;
                let cs = chars_of(p, l);
                let (st, en) = (f64::from(l.start), f64::from(l.end));
                for o in [st - 1.0, st, st + 1.0, uo_jsnum::math::floor((st + en) / 2.0), en - 1.0, en, en + 1.0] {
                    s.num(caret_x_in(cs, l.x, o));
                }
                let mut xs = vec![l.x - 50.0, l.x, l.x + l.width / 2.0, l.x + l.width + 50.0];
                for k in 0..cs.len().min(30) {
                    let (cx, cw) = (cs.x[k], cs.w[k]);
                    xs.push(cx + cw / 2.0);
                    xs.push(cx + cw / 2.0 - 0.001);
                }
                for x in xs {
                    s.u32(offset_at_x(cs, l.start, x, l.end)).u32(offset_at_x(cs, l.start, x, l.end + 7));
                }
            }
        }
        out.insert("p4.caret", s.value());
    }

    // p4.runboxes, p4.inline
    {
        let mut s = S::new();
        let links = run_boxes_with(&loc, layout, RunFilter::Links);
        let customs = run_boxes_with(&loc, layout, RunFilter::CustomInlines);
        for (tag, list) in [(b'L', &links), (b'C', &customs)] {
            s.tag(tag).u32(list.len() as u32);
            for b in list.iter() {
                let rn = names.run(&b.run).unwrap_or_else(|| "?".into());
                s.u32(b.page).num(b.x).num(b.y).num(b.w).num(b.h).num(b.baseline).str(&rn).u16s(&b.value);
            }
        }
        out.insert("p4.runboxes", s.value());
        let mut si = S::new();
        let probe: Vec<u16> = "Wq😀\u{fffd}x".encode_utf16().collect();
        // the lone surrogate of the TS probe ('Wq😀\uD800x')
        let mut probe2 = probe.clone();
        probe2[4] = 0xd800;
        for b in &customs {
            let f = layout.custom_inline_face(b);
            let fb: Vec<u16> = view
                .run(&b.run)
                .and_then(|r| r.ext())
                .and_then(|e| e.custom.as_deref())
                .map(|c| c.fallback_text.units().to_vec())
                .unwrap_or_default();
            si.num(f.size_pt)
                .u16s(&f.color)
                .num(layout.measure_text(f.face, f.size_pt, &fb))
                .num(layout.measure_text(f.face, f.size_pt, &probe2))
                .num(layout.measure_text(f.face, f.size_pt, &[]));
        }
        out.insert("p4.inline", si.value());
    }

    // p4.nav
    {
        let nav = layout.navigation(view);
        let mut s = S::new();
        s.u32(nav.links.len() as u32);
        for pl in &nav.links {
            let l = &pl.link;
            s.u32(pl.page).num(l.x_pt).num(l.y_top_pt).num(l.w_pt).num(l.h_pt);
            match &l.target {
                uo_render::LinkTarget::Uri(u) => {
                    s.tag(b'U').u16s(u);
                }
                uo_render::LinkTarget::Dest { page_index, dest_top_pt } => {
                    s.tag(b'D').u32(*page_index).num(*dest_top_pt);
                }
            }
        }
        s.u32(nav.outline.len() as u32);
        for o in &nav.outline {
            s.u16s(&o.title).num(f64::from(o.level)).u32(o.page_index).num(o.top_pt);
        }
        out.insert("p4.nav", s.value());
    }

    // p4.fields
    {
        let mut s = S::new();
        let named: Vec<(String, &[u16])> =
            layout.fields.as_deref().unwrap_or(&[]).iter().filter_map(|(r, v)| names.run(r).map(|n| (n, v.units()))).collect();
        if std::env::var("UO_F8_DEBUG").is_ok() {
            for (n, v) in &named {
                eprintln!("FIELD {n} {:?}", String::from_utf16_lossy(v));
            }
        }
        s.u32(named.len() as u32);
        for (n, v) in named {
            s.str(&n).u16s(v);
        }
        out.insert("p4.fields", s.value());
    }

    let index = LineIndex::new(&loc, layout);

    // p4.locate
    {
        let mut s = S::new();
        for (pg, p) in layout.pages.iter().enumerate() {
            for k in 0..p.lines.len() {
                let at = index.located(pg as u32, k as u32).unwrap();
                s.u32(pg as u32).str(kind_name(at.kind)).str(&at.para.as_ref().map_or_else(|| "-".to_owned(), |q| names.para(q)));
            }
        }
        out.insert("p4.locate", s.value());
    }

    // p4.lineat, p4.cellat
    {
        let (mut sl, mut sc) = (S::new(), S::new());
        for pg in sample_pages(layout.pages.len()) {
            let p = &layout.pages[pg];
            let pts = sample_points(p);
            sl.u32(pg as u32).u32(pts.len() as u32);
            sc.u32(pg as u32).u32(pts.len() as u32);
            for (x, y) in pts {
                sl.opt_num(index.line_at(layout, pg as u32, x, y).map(f64::from));
                sc.opt_num(cell_at(&loc, layout, pg as u32, x, y).map(f64::from));
            }
        }
        out.insert("p4.lineat", sl.value());
        out.insert("p4.cellat", sc.value());
    }

    // p4.pageof
    {
        let mut s = S::new();
        fn visit(story: &uo_docx::model::Story, base: Option<(&BlockPath, u32, u32)>, f: &mut dyn FnMut(ParaLoc, u32)) {
            for (i, b) in story.blocks().enumerate() {
                let path = match base {
                    None => BlockPath::root(i as u32),
                    Some((p, r, c)) => p.cell_block(r, c, i as u32),
                };
                match b {
                    Block::Paragraph(p) => f(ParaLoc { story: StoryRef::Body, path }, para_length(p)),
                    Block::Table(t) => {
                        for (r, row) in t.rows.iter().enumerate() {
                            for (c, cell) in row.cells.iter().enumerate() {
                                visit(&cell.content, Some((&path, r as u32, c as u32)), f);
                            }
                        }
                    }
                }
            }
        }
        visit(view.body(), None, &mut |p, len| {
            for off in [0, len / 2, len] {
                s.num(index.page_of(layout, &p, f64::from(off)).map_or(-1.0, f64::from));
            }
        });
        out.insert("p4.pageof", s.value());
    }

    // p4.edit
    {
        let mut s = if std::env::var("UO_F8_TRACE").is_ok() { S::traced() } else { S::new() };
        let mut scope = |tag: &str, sc: EditScope<'_>, hf: bool| {
            let el = EditLines::new(&loc, layout, &index, &sc);
            s.tag(b'S').str(tag).u32(el.paras.len() as u32).u32(if hf { el.page.unwrap_or(0) } else { 0 });
            for p in &el.paras {
                s.u32(p.lines.len() as u32);
                for l in &p.lines {
                    s.u32(l.page)
                        .u32(l.start)
                        .u32(l.end)
                        .num(l.x)
                        .num(l.y)
                        .num(l.height)
                        .num(l.baseline)
                        .num(l.width)
                        .u32(l.line_start)
                        .bool(l.xf.is_some());
                    if let Some(m) = l.xf {
                        for v in m {
                            s.num(v);
                        }
                    }
                }
            }
            for pg in sample_pages(layout.pages.len()) {
                let pts = sample_points(&layout.pages[pg]);
                s.u32(pg as u32).u32(pts.len() as u32);
                for (x, y) in pts {
                    let h = el.hit(pg as u32, x, y);
                    s.u32(h.para).u32(h.offset);
                }
            }
            for (pi, p) in el.paras.iter().enumerate().take(60) {
                for off in sample_offsets(p.len, &p.lines) {
                    match el.caret(EditPos { para: pi as u32, offset: off }) {
                        None => {
                            s.bool(false);
                        }
                        Some(c) => {
                            s.bool(true).u32(c.page).num(c.x).num(c.y).num(c.h);
                        }
                    }
                }
            }
            let mut pages: Vec<u32> = el.paras.iter().flat_map(|p| p.lines.iter().map(|l| l.page)).collect();
            pages.sort_unstable();
            pages.dedup();
            let lens: Vec<u32> = el.paras.iter().map(|p| p.len).collect();
            let mut rects: Vec<SelRect> = Vec::new();
            for (a, b) in sample_ranges(&lens) {
                for &pg in &pages {
                    rects.clear();
                    el.selection_rects(pg, a, b, &mut rects);
                    s.u32(pg).u32(rects.len() as u32);
                    for r in &rects {
                        s.num(r.x).num(r.y).num(r.w).num(r.h).bool(r.xf.is_some());
                        if let Some(m) = r.xf {
                            for v in m {
                                s.num(v);
                            }
                        }
                    }
                }
            }
        };
        scope("body", EditScope::Body, false);
        // cells: the first six, depth first
        struct CellScope {
            tag: String,
            table: BlockPath,
            row: u32,
            cell: u32,
        }
        let mut cells: Vec<CellScope> = Vec::new();
        #[allow(clippy::too_many_arguments)]
        fn visit_table(t: &uo_docx::model::Table, table: &BlockPath, anchor: usize, hops: &mut Vec<String>, cells: &mut Vec<CellScope>) {
            for (r, row) in t.rows.iter().enumerate() {
                let mut col = match row.tr_pr.as_deref().and_then(|p| p.grid_before) {
                    Some(uo_docx_layout::model::JsNumber::Num(v)) => v,
                    _ => 0.0,
                };
                for (k, cell) in row.cells.iter().enumerate() {
                    let c = col;
                    let span = match cell.tc_pr.as_deref().and_then(|p| p.grid_span) {
                        Some(uo_docx_layout::model::JsNumber::Num(v)) => v,
                        _ => match cell.grid_span {
                            uo_docx_layout::model::JsNumber::Num(v) => v,
                            uo_docx_layout::model::JsNumber::Null => 1.0,
                        },
                    };
                    col += uo_jsnum::math::max(1.0, span);
                    let cs = uo_jsnum::format::to_js_string(c);
                    if cells.len() < 6 {
                        let mut h = hops.clone();
                        h.push(format!("{r}.{cs}."));
                        cells.push(CellScope {
                            tag: format!("cell {anchor} {}", h.join("/")),
                            table: table.clone(),
                            row: r as u32,
                            cell: k as u32,
                        });
                    }
                    for (bi, b) in cell.content.blocks().enumerate() {
                        if let Block::Table(nt) = b
                            && cells.len() < 6
                        {
                            hops.push(format!("{r}.{cs}.{bi}"));
                            visit_table(nt, &table.cell_block(r as u32, k as u32, bi as u32), anchor, hops, cells);
                            hops.pop();
                        }
                    }
                }
            }
        }
        let mut anchor = 0;
        for (bi, b) in view.body().blocks().enumerate() {
            if let Block::Table(t) = b {
                if cells.len() < 6 {
                    visit_table(t, &BlockPath::root(bi as u32), anchor, &mut Vec::new(), &mut cells);
                }
                anchor += 1;
            }
        }
        for c in &cells {
            scope(&c.tag, EditScope::Cell { table: c.table.clone(), row: c.row, cell: c.cell }, false);
        }
        // header/footer parts: the first four, first and last page
        let last = layout.pages.len().saturating_sub(1) as u32;
        for &(part, which) in names.hf_order.iter().take(4) {
            for page in [0, last] {
                let kind = if which == HfWhich::Header { "header" } else { "footer" };
                scope(&format!("{} {kind} {page}", names.hf[&part]), EditScope::HeaderFooter { which, part, page }, true);
            }
        }
        // notes: three of each kind
        for (kind, map, k) in
            [("footnote", &view.notes().footnotes, NoteKind::Footnote), ("endnote", &view.notes().endnotes, NoteKind::Endnote)]
        {
            for id in map.keys().take(3) {
                scope(
                    &format!("{kind} {}", uo_jsnum::format::to_js_string(id.get())),
                    EditScope::Note { kind: k, id: NumKey::new(id.get()) },
                    false,
                );
            }
        }
        // textboxes with content (first four), each alone in its chain
        let mut tbs: Vec<(DrawingId, u32, bool)> = Vec::new();
        for (pg, p) in layout.pages.iter().enumerate() {
            for o in p.objects.as_deref().unwrap_or(&[]) {
                // model textboxes the facet names (not a lifted frame's synthetic box)
                let Some(tb) = o
                    .run
                    .as_ref()
                    .filter(|r| names.run(r).is_some())
                    .and_then(|r| view.run(r))
                    .and_then(|r| r.ext())
                    .and_then(|e| e.textbox.as_deref())
                else {
                    continue;
                };
                if o.content.is_none() || tbs.iter().any(|x| x.0 == tb.drawing) || tbs.len() >= 4 {
                    continue;
                }
                tbs.push((tb.drawing, pg as u32, o.hf));
            }
        }
        for (d, pg, hf) in tbs {
            let chain = [d];
            scope(
                &format!("textbox {pg}{}", if hf { " hf" } else { "" }),
                EditScope::Textbox { chain: &chain, page: hf.then_some(pg) },
                false,
            );
        }
        out.insert("p4.edit", s.value_traced(key));
    }
    out
}

/// `p4.hyph`: the body hyphenation points of a forced layout.
fn hyph_points(view: DocView<'_>, layout: &Layout, s: &mut S) {
    let loc = Locator::new(view);
    let mut list = Vec::new();
    for p in &layout.pages {
        for l in &p.lines {
            let Some(h) = l.hyph else { continue };
            let at = loc.locate(p.section, &l.at);
            if at.kind != LocKind::Body {
                continue;
            }
            list.push((at.para.map_or(0, |q| q.path.top()), h));
        }
    }
    s.u32(list.len() as u32);
    for (b, h) in list {
        s.u32(b).u32(h);
    }
}

// ───────────────────────── the replay ─────────────────────────

#[derive(Default, Debug)]
struct Tally {
    docs: usize,
    equal: usize,
    p1_differs: Vec<String>,
    mismatch: BTreeMap<String, Vec<String>>,
    pending: BTreeMap<String, usize>,
}

fn replay(capture: &Path, dump: &Path, docs: &Path) -> (Tally, uo_testkit::Report) {
    let cap = Capture::read(capture).unwrap();
    let mut fonts = FontIndex::open_fonts();
    let mut replay = Replay::new(&cap);
    let mut t = Tally::default();
    let only: Vec<String> = std::env::var("UO_LAYOUT_ONLY").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default();
    for rec in cap.records() {
        let skip_all = |replay: &mut Replay<'_>| {
            for (f, _) in &rec.facets {
                replay.skip(&rec.key, f);
            }
        };
        if (!only.is_empty() && !only.iter().any(|p| rec.key.starts_with(p.as_str())))
            || rec.get("p4.edit").is_none()
            || rec.get("error").is_some()
        {
            skip_all(&mut replay);
            continue;
        }
        let path = [docs.join(format!("{}.docx", safe(&rec.key))), docs.join(format!("{}.docx", rec.key))].into_iter().find(|p| p.exists());
        let Some(path) = path else {
            *t.pending.entry("docx not in the docs dir".into()).or_default() += 1;
            skip_all(&mut replay);
            continue;
        };
        let docx = std::fs::read(&path).unwrap();
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
        let inner = TranscriptProvider::new(&tr, loaded);
        let mut labels = HashMap::new();
        for c in &tr.calls {
            labels.entry((c.family.clone(), c.bold, c.italic)).or_insert(c.answer);
        }
        let provider = Recording { inner: &inner, labels, calls: Mutex::new(Vec::new()) };
        let doc = read_doc(&docx);
        let view = DocView::new(&doc);
        let mut o = options_for(&rec.key);
        o.collect_fields = Some(FieldScope::All);
        t.docs += 1;
        let (layout, rnames) = match lay(view, &provider, &o) {
            Ok(x) => x,
            Err(LayoutError::Unported(fr, what)) => {
                *t.pending.entry(format!("{fr:?} {what}")).or_default() += 1;
                skip_all(&mut replay);
                continue;
            }
            Err(e) => panic!("{}: {e}", rec.key),
        };
        let mut sink = StreamSink(Stream::new());
        canon_layout(&layout, &|r| rnames.get(r).cloned(), &mut sink);
        let p1 = replay.check(&rec.key, "p1", &sink.value()) & replay.check(&rec.key, "pages", &layout.pages.len().to_string());
        if !p1 {
            t.p1_differs.push(rec.key.clone());
        }
        let mut all_ok = p1;
        let mut check = |replay: &mut Replay<'_>, f: &str, v: &str, t: &mut Tally| {
            if !replay.check(&rec.key, f, v) {
                t.mismatch.entry(f.to_owned()).or_default().push(rec.key.clone());
                all_ok = false;
            }
        };
        for (f, v) in editor_facets(view, &layout, &rec.key) {
            check(&mut replay, f, &v, &mut t);
        }
        // forced hyphenation, twice
        {
            let mut s = S::new();
            let mut oh = options_for(&rec.key);
            oh.hyphenation = Hyphenation::Force;
            let (l1, _) = lay(view, &provider, &oh).unwrap();
            hyph_points(view, &l1, &mut s);
            let skip_text = rec.get("p4.hyphskip").unwrap_or("");
            let mut skips = HyphenSkips::default();
            for part in skip_text.split(',').filter(|x| !x.is_empty()) {
                let (b, off) = part.split_once(':').unwrap();
                let b: usize = b.parse().unwrap();
                if let Some(Block::Paragraph(p)) = view.body().block_at(b) {
                    skips.0.insert((p.id, off.parse().unwrap()));
                }
            }
            replay.skip(&rec.key, "p4.hyphskip");
            oh.hyphenation_skips = skips;
            let (l2, _) = lay(view, &provider, &oh).unwrap();
            hyph_points(view, &l2, &mut s);
            check(&mut replay, "p4.hyph", &s.value(), &mut t);
        }
        // the Web view
        {
            let web = uo_docx_layout::view_transform::web_document(&doc, 9000.0);
            let (lw, names) = lay(DocView::new(&web), &provider, &options_for(&rec.key)).unwrap();
            let mut sink = StreamSink(Stream::new());
            canon_layout(&lw, &|r| names.get(r).cloned(), &mut sink);
            check(&mut replay, "p4.web", &sink.value(), &mut t);
            check(&mut replay, "p4.web.pages", &lw.pages.len().to_string(), &mut t);
        }
        // the TS-only count of field runs without a model name (slices, frame copies)
        replay.skip(&rec.key, "p4.fields.unnamed");
        for (f, _) in &rec.facets {
            if f.starts_with("p3.") || f == "p2" || f == "p2first" {
                replay.skip(&rec.key, f);
            }
        }
        if all_ok {
            t.equal += 1;
        }
    }
    (t, replay.finish())
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn f8_committed_fixture() {
    let dir = manifest().join("tests/captures/f8");
    if !dir.join("layout.capture").exists() {
        eprintln!("f8 fixture absent");
        return;
    }
    let (t, report) = replay(&dir.join("layout.capture"), &dir, &dir.join("docs"));
    eprintln!("f8 fixture: {t:?}; {}", report.summary());
    report.assert_clean();
    assert!(t.pending.is_empty(), "{:?}", t.pending);
    assert_eq!(t.equal, t.docs, "every document of the fixture equal in every P4 facet");
    assert!(t.docs > 0);
}

#[test]
fn f8_parity_against_the_frozen_ts() {
    if !uo_testkit::opt_in("UO_F8_PARITY", "F8 P4 replay (needs a --f8 capture, its dump and docs)") {
        return;
    }
    let var = |n: &str| PathBuf::from(std::env::var(n).unwrap_or_else(|_| panic!("{n} is required")));
    let (t, report) = replay(&var("UO_LAYOUT_CAPTURE"), &var("UO_LAYOUT_DUMP"), &var("UO_LAYOUT_DOCS"));
    eprintln!("f8 parity: {}; documents {}, equal {}, pending {:?}", report.summary(), t.docs, t.equal, t.pending);
    eprintln!("P1 differs ({}): {:?}", t.p1_differs.len(), &t.p1_differs[..t.p1_differs.len().min(20)]);
    for (f, keys) in &t.mismatch {
        let p1: HashSet<&String> = t.p1_differs.iter().collect();
        let own: Vec<&String> = keys.iter().filter(|k| !p1.contains(k)).collect();
        eprintln!("{f}: {} mismatches ({} over an equal P1): {:?}", keys.len(), own.len(), &own[..own.len().min(12)]);
    }
    report.assert_clean();
}
