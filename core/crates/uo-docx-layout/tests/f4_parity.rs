//! The F4 facet (record "Stage 3 — F4"): `p3.prepare` — every `prepareParagraph` call of the TS
//! layout, replayed from the inputs `tools/lib/facets-f4.mjs` dumped (`<key>.f4.bin`): the docx is
//! read by the uo-docx reader, the paragraph is found by its model position, the table-style env
//! is rebuilt from (style, conditions), the field callback, the numbering and the note marks
//! answer from the TS transcript, the faces from the P2 transcript (real font files); the Rust
//! `prepare_paragraph` result is encoded as the TS one and compared byte for byte, and the
//! per-document facet digest is rebuilt and compared with the capture.
//!
//! Runs the committed fixture (`tests/captures/f4`) always; a fresh capture with
//! `UO_F4_CAPTURE=<capture> UO_F4_DUMP=<dump dir> UO_F4_DOCS=<dir the keys are relative to>`.
//! Calls whose paragraph is not a model paragraph (copies the engine derives, LA2: F7a) are
//! counted as pending, never as equal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{FontIndex, Transcript, TranscriptProvider, load_answers};
use uo_docx::DocView;
use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::js::JsValue;
use uo_docx::model::Document;
use uo_docx::model::story::{Block, Story};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::view::{BlockPath, RunRef, RunSub, StoryRef};
use uo_docx_layout::context::{DocContext, NoteKindFlow};
use uo_docx_layout::engine::{Cx, PassInput};
use uo_docx_layout::error::LResult;
use uo_docx_layout::fonts::{FaceResolverCache, FaceTable};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::keys::{LParaId, RunKey};
use uo_docx_layout::ldoc::{LDoc, LPara, ParaFlags, ParaSrc, RunSrc};
use uo_docx_layout::model::{DocFormatting, JsNumber, PPr, RPr};
use uo_docx_layout::options::{Budget, Hyphenation, LayoutOptions, RevisionMode, RevisionView};
use uo_docx_layout::para::{
    FieldSource, Glyph, GlyphFlags, MARKER_IX, MarkerLevel, ParaEnv, PrepFlow, PreparedPara, RunStyle, prepare_paragraph,
};
use uo_docx_layout::word::{self, LevelSrc, NumberMarker};
use uo_render::types::{LineCap, LineJoin, ResolvedFill, ResolvedTextFx};
use uo_render::{FaceId, Resources};
use uo_testkit::{Capture, Replay, Stream};

// ── the raw Sink encoding (sink.mjs, facets-f5.mjs `Raw`) ──────────────────────────────────

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

type R<T> = Result<T, String>;

impl Rd<'_> {
    fn take(&mut self, n: usize) -> R<&[u8]> {
        let s = self.b.get(self.at..self.at + n).ok_or("truncated f4 dump")?;
        self.at += n;
        Ok(s)
    }
    fn tag(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn num(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bool(&mut self) -> R<bool> {
        Ok(self.take(1)?[0] != 0)
    }
    fn bytes(&mut self) -> R<Vec<u8>> {
        let n = u64::from_le_bytes(self.take(8)?.try_into().unwrap()) as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn u16s(&mut self) -> R<Vec<u16>> {
        Ok(self.bytes()?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
    fn string(&mut self) -> R<String> {
        Ok(String::from_utf16_lossy(&self.u16s()?))
    }
    fn opt_u16s(&mut self) -> R<Option<Vec<u16>>> {
        Ok(if self.bool()? { Some(self.u16s()?) } else { None })
    }
}

#[derive(Default)]
struct Raw(Vec<u8>);

impl Raw {
    fn tag(&mut self, c: u8) -> &mut Self {
        self.0.push(c);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn num(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&uo_jsnum::conv::canonicalize_nan(v).to_le_bytes());
        self
    }
    fn bool(&mut self, v: bool) -> &mut Self {
        self.0.push(u8::from(v));
        self
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(b);
        self
    }
    fn u16s(&mut self, s: &[u16]) -> &mut Self {
        let b: Vec<u8> = s.iter().flat_map(|u| u.to_le_bytes()).collect();
        self.bytes(&b)
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.u16s(&s.encode_utf16().collect::<Vec<u16>>())
    }
    fn opt_num(&mut self, v: Option<f64>) -> &mut Self {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.num(v);
        }
        self
    }
    fn opt_u16s(&mut self, s: Option<&[u16]>) -> &mut Self {
        self.bool(s.is_some());
        if let Some(s) = s {
            self.u16s(s);
        }
        self
    }
    fn opt_str(&mut self, s: Option<&str>) -> &mut Self {
        self.bool(s.is_some());
        if let Some(s) = s {
            self.str(s);
        }
        self
    }
}

/// A canonical sink over a testkit stream (the uo-docx `Canon` digests of `ppr`/`rpr`).
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

fn digest(v: &dyn JsValue) -> String {
    let mut s = CS(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s);
        v.canon(&mut w);
    }
    let n = s.0.items();
    format!("{n}:{}", s.0.finish().short())
}

// ── the model positions (facets-f4.mjs `modelRefs`) ────────────────────────────────────────

/// Paragraph ref → (story, path, paragraph); every run of those paragraphs (and their field
/// results, one level) in an LDoc, with the run refs the TS prints.
struct Model<'d> {
    ldoc: LDoc<'d>,
    paras: HashMap<String, LParaId>,
    run_names: HashMap<RunKey, String>,
}

fn build_model(doc: &Document) -> Model<'_> {
    let view = DocView::new(doc);
    let mut m = Model { ldoc: LDoc::empty(view), paras: HashMap::new(), run_names: HashMap::new() };
    walk(&mut m, &doc.body, StoryRef::Body, None, "");
    let mut seen = Vec::new();
    let mut n = 0usize;
    for (si, s) in doc.sections.iter().enumerate() {
        let _ = si;
        for rec in [&s.headers, &s.footers] {
            for k in ["default", "first", "even"] {
                let Some(&part) = rec.get(k) else { continue };
                if seen.contains(&part) {
                    continue;
                }
                seen.push(part);
                let name = format!("hf{n}");
                n += 1;
                if let Some(st) = view.hf_story(part) {
                    walk(&mut m, st, StoryRef::HeaderFooter(part), None, &name);
                }
            }
        }
    }
    for (kind, notes, prefix) in
        [(uo_docx::view::NoteKind::Footnote, &doc.notes.footnotes, "fn"), (uo_docx::view::NoteKind::Endnote, &doc.notes.endnotes, "en")]
    {
        for (id, st) in notes {
            let name = format!("{prefix}{}", uo_jsnum::format::to_js_string(id.get()));
            walk(&mut m, st, StoryRef::Note(kind, *id), None, &name);
        }
    }
    m
}

fn walk<'d>(m: &mut Model<'d>, story: &'d Story, sref: StoryRef, base: Option<&BlockPath>, prefix: &str) {
    for (i, b) in story.blocks().enumerate() {
        let i = u32::try_from(i).unwrap();
        let path = match base {
            None => BlockPath::root(i),
            Some(p) => p.clone(),
        };
        let path = if base.is_some() { cell_path(&path, i) } else { path };
        let name = format!("{prefix}/b{i}");
        match b {
            Block::Paragraph(p) => para(m, p, sref, path, &name),
            Block::Table(t) => {
                for (ri, row) in t.rows.iter().enumerate() {
                    for (ci, c) in row.cells.iter().enumerate() {
                        let cp = CellBase { table: path.clone(), row: u32::try_from(ri).unwrap(), cell: u32::try_from(ci).unwrap() };
                        walk_cell(m, &c.content, sref, &cp, &format!("{name}/r{ri}/c{ci}"));
                    }
                }
            }
        }
    }
}

struct CellBase {
    table: BlockPath,
    row: u32,
    cell: u32,
}

fn cell_path(p: &BlockPath, _i: u32) -> BlockPath {
    p.clone()
}

fn walk_cell<'d>(m: &mut Model<'d>, story: &'d Story, sref: StoryRef, c: &CellBase, prefix: &str) {
    for (i, b) in story.blocks().enumerate() {
        let i = u32::try_from(i).unwrap();
        let path = c.table.cell_block(c.row, c.cell, i);
        let name = format!("{prefix}/b{i}");
        match b {
            Block::Paragraph(p) => para(m, p, sref, path, &name),
            Block::Table(t) => {
                for (ri, row) in t.rows.iter().enumerate() {
                    for (ci, cc) in row.cells.iter().enumerate() {
                        let cb = CellBase { table: path.clone(), row: u32::try_from(ri).unwrap(), cell: u32::try_from(ci).unwrap() };
                        walk_cell(m, &cc.content, sref, &cb, &format!("{name}/r{ri}/c{ci}"));
                    }
                }
            }
        }
    }
}

fn strip(s: &str) -> String {
    s.strip_prefix('/').unwrap_or(s).to_owned()
}

fn para<'d>(m: &mut Model<'d>, p: &'d uo_docx::model::Paragraph, sref: StoryRef, path: BlockPath, name: &str) {
    let pname = strip(name);
    let mut runs = Vec::with_capacity(p.runs.len());
    for (i, r) in p.runs.iter().enumerate() {
        let i32_ = u32::try_from(i).unwrap();
        let key = RunKey(u32::try_from(m.ldoc.runs.len()).unwrap());
        m.ldoc.runs.push(RunSrc::Model { run: r, at: RunRef { story: sref, path: path.clone(), run: i32_, sub: RunSub::None } });
        let rname = format!("{pname}#{i}");
        m.run_names.entry(key).or_insert_with(|| rname.clone());
        runs.push(key);
        if let Some(fr) = r.ext().and_then(|e| e.field_result.as_deref()) {
            for (j, x) in fr.iter().enumerate() {
                let j32 = u32::try_from(j).unwrap();
                let k2 = RunKey(u32::try_from(m.ldoc.runs.len()).unwrap());
                m.ldoc.runs.push(RunSrc::Model {
                    run: x,
                    at: RunRef { story: sref, path: path.clone(), run: i32_, sub: RunSub::FieldResult(j32) },
                });
                m.ldoc.field_results.insert((key, j32), k2);
                m.run_names.insert(k2, format!("{rname}/f{j}"));
            }
        }
        if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
            walk(m, &tb.content, StoryRef::Textbox(tb.drawing), None, &format!("{rname}/tx"));
        }
        if let Some(g) = r.ext().and_then(|e| e.group.as_deref()) {
            for (j, it) in g.items.iter().enumerate() {
                if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                    walk(m, &tb.content, StoryRef::Textbox(tb.drawing), None, &format!("{rname}/g{j}#0/tx"));
                }
            }
        }
    }
    let id = LParaId(u32::try_from(m.ldoc.paras.len()).unwrap());
    m.ldoc.paras.push(LPara {
        src: ParaSrc::Model { para: p, id: p.id },
        ppr: None,
        props: None,
        runs,
        rev_joins: Vec::new(),
        flags: ParaFlags::default(),
        frame_carry: None,
        story: sref,
        path,
    });
    m.paras.entry(pname).or_insert(id);
}

// ── the replay ─────────────────────────────────────────────────────────────────────────────

enum Ev {
    Field { name: Vec<u16>, instr: Vec<u16>, at: f64, answer: Option<Vec<u16>> },
    Num { num_id: f64, ilvl: f64, marker: Option<NumberMarker> },
    Note { kind: String, id: f64, custom: bool, mark: Vec<u16> },
}

struct Flow {
    ctx: DocContext,
    events: VecDeque<Ev>,
    off: Vec<String>,
}

impl PrepFlow for Flow {
    fn ctx(&mut self) -> &mut DocContext {
        &mut self.ctx
    }

    fn field(&mut self, _cx: &mut Cx<'_, '_>, _src: FieldSource, name: &[u16], instr: &[u16], at: f64) -> LResult<Option<Box<[u16]>>> {
        match self.events.pop_front() {
            Some(Ev::Field { name: n, instr: i, at: a, answer }) => {
                if n != name || i != instr || a.to_bits() != at.to_bits() {
                    self.off.push(format!(
                        "field call differs: {} {} {at}",
                        String::from_utf16_lossy(name),
                        String::from_utf16_lossy(instr)
                    ));
                }
                Ok(answer.map(Vec::into_boxed_slice))
            }
            _ => {
                self.off.push(format!("unexpected field call {}", String::from_utf16_lossy(name)));
                Ok(None)
            }
        }
    }

    fn number_next(&mut self, _fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<NumberMarker> {
        match self.events.pop_front() {
            Some(Ev::Num { num_id: n, ilvl: l, marker }) => {
                if n.to_bits() != num_id.to_number().to_bits() || l.to_bits() != ilvl.to_number().to_bits() {
                    self.off.push("numbering call differs".into());
                }
                marker
            }
            _ => {
                self.off.push("unexpected numbering call".into());
                None
            }
        }
    }

    fn note_mark(&mut self, kind: NoteKindFlow, id: f64, custom: bool, _lang: &str, _cap: &mut word::TextCap) -> Box<[u16]> {
        match self.events.pop_front() {
            Some(Ev::Note { kind: k, id: i, custom: c, mark }) => {
                let want = if kind == NoteKindFlow::Footnote { "footnote" } else { "endnote" };
                if k != want || i.to_bits() != id.to_bits() || c != custom {
                    self.off.push("note mark call differs".into());
                }
                mark.into_boxed_slice()
            }
            _ => {
                self.off.push("unexpected note mark call".into());
                Box::default()
            }
        }
    }
}

// ── the TS value of a call (facets-f4.mjs `encodeValue`) ────────────────────────────────────

struct Enc<'e> {
    r: Raw,
    faces: HashMap<FaceId, u32>,
    styles: HashMap<u32, u32>,
    p: &'e PreparedPara,
    fonts: &'e FaceTable,
    res: &'e Resources,
    ldoc: &'e LDoc<'e>,
    names: &'e HashMap<RunKey, String>,
}

impl Enc<'_> {
    fn face(&mut self, f: FaceId) {
        if self.faces.contains_key(&f) {
            return;
        }
        let l = u32::try_from(self.faces.len()).unwrap();
        self.faces.insert(f, l);
        let a = &self.fonts.entry(f).unwrap().answer;
        self.r
            .tag(b'F')
            .u32(l)
            .str(&a.key)
            .str(&a.family)
            .bool(a.bold)
            .bool(a.italic)
            .bool(a.synth_bold)
            .bool(a.synth_italic)
            .bool(a.stand_in);
    }

    fn fill(&mut self, f: &ResolvedFill) {
        let res = self.res;
        match f {
            ResolvedFill::None => {
                self.r.tag(b'n');
            }
            ResolvedFill::Solid { hex, alpha } => {
                self.r.tag(b's').u16s(res.sym(*hex)).num(*alpha);
            }
            ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to } => {
                self.r.tag(b'g').u32(u32::try_from(stops.len()).unwrap());
                for s in stops {
                    self.r.num(s.pos).u16s(res.sym(s.hex)).num(s.alpha);
                }
                self.r.num(*angle_deg).bool(*scaled).opt_u16s(path.map(|p| res.sym(p)));
                self.r.bool(fill_to.is_some());
                if let Some(t) = fill_to {
                    self.r.num(t.l).num(t.t).num(t.r).num(t.b);
                }
            }
        }
    }

    fn textfx(&mut self, fx: &ResolvedTextFx) {
        let res = self.res;
        self.r.bool(fx.fill.is_some());
        if let Some(f) = &fx.fill {
            self.fill(f);
        }
        self.r.bool(fx.outline.is_some());
        if let Some(o) = &fx.outline {
            self.r.num(o.width_pt);
            self.fill(&o.fill);
            let cap = match o.cap {
                LineCap::Butt => "butt",
                LineCap::Round => "round",
                LineCap::Square => "square",
            };
            let join = match o.join {
                LineJoin::Miter => "miter",
                LineJoin::Round => "round",
                LineJoin::Bevel => "bevel",
            };
            self.r.opt_u16s(o.dash.map(|d| res.sym(d))).str(cap).str(join);
        }
        self.r.bool(fx.shadow.is_some());
        if let Some(h) = &fx.shadow {
            self.r.num(h.blur_pt).num(h.dist_pt).num(h.dir_deg).num(h.sx).num(h.sy).num(h.kx_deg).num(h.ky_deg);
            self.r.u16s(res.sym(h.algn)).u16s(res.sym(h.hex)).num(h.alpha);
        }
        self.r.bool(fx.glow.is_some());
        if let Some(g) = &fx.glow {
            self.r.num(g.rad_pt).u16s(res.sym(g.hex)).num(g.alpha);
        }
        self.r.bool(fx.reflection.is_some());
        if let Some(x) = &fx.reflection {
            self.r.num(x.blur_pt).num(x.st_a).num(x.st_pos).num(x.end_a).num(x.end_pos).num(x.dist_pt).num(x.dir_deg).u16s(res.sym(x.algn));
        }
    }

    fn style(&mut self, s: u32) {
        if self.styles.contains_key(&s) {
            return;
        }
        let l = u32::try_from(self.styles.len()).unwrap();
        self.styles.insert(s, l);
        let st: &RunStyle = &self.p.styles[s as usize];
        self.r.tag(b'S').u32(l).num(st.size_pt).u16s(&st.color).bool(st.auto_color).bool(st.bold).bool(st.italic);
        self.r.bool(st.underline.is_some());
        if let Some((v, c)) = &st.underline {
            self.r.str(v).opt_str(c.as_deref());
        }
        self.r
            .bool(st.strike)
            .bool(st.dstrike)
            .opt_str(st.highlight.as_deref())
            .opt_str(st.shading.as_deref())
            .num(st.shift)
            .bool(st.hidden);
        self.r.opt_u16s(st.link.as_deref()).opt_num(st.alpha.get()).bool(st.outline);
        self.r.bool(st.fx.is_some());
        if let Some(fx) = st.fx {
            let fx = self.res.text_fx(fx).unwrap().clone();
            self.textfx(&fx);
        }
        self.r.opt_str(st.fx_key.as_deref()).str(&digest(&*st.rpr));
    }

    fn refs(&mut self, g: &Glyph) {
        self.face(g.face);
        self.style(g.style.0);
        if let Some(m) = g.math {
            for it in &self.p.math[m as usize].items {
                self.face(it.face);
                self.style(it.style.0);
            }
        }
    }

    fn ix(v: u32) -> f64 {
        if v == MARKER_IX { -1.0 } else { f64::from(v) }
    }

    fn glyph(&mut self, g: &Glyph) {
        let f = |x: u16| g.flags.has(x);
        self.r.str(g.kind.name()).u16s(g.ch.as_units()).num(f64::from(g.cp)).u32(self.faces[&g.face]).u32(self.styles[&g.style.0]);
        self.r
            .num(g.adv)
            .num(g.asc)
            .num(g.desc)
            .opt_num(g.n_asc.get())
            .opt_num(g.n_desc.get())
            .num(Self::ix(g.offset))
            .num(Self::ix(g.run_idx));
        self.r.bool(f(GlyphFlags::BREAK_AFTER)).bool(f(GlyphFlags::GLUE)).bool(f(GlyphFlags::CLEAR)).bool(f(GlyphFlags::FIXED));
        self.r.bool(f(GlyphFlags::HANG)).bool(f(GlyphFlags::HYPH)).num(g.size_pt);
        self.r.opt_num(g.hr.get()).opt_num(g.fe_shrink.get()).opt_num(g.kern_adj.get()).opt_num(g.lig_start.map(f64::from));
        self.r.opt_num(g.lig_end.map(f64::from)).opt_num(g.sub_gid.map(f64::from)).opt_num(g.note_id.get());
        if let Some(pb) = &g.pic_bullet {
            self.r.tag(b'b').num(pb.width_twips).num(pb.height_twips);
        } else if let Some(k) = g.obj {
            match self.names.get(&k) {
                Some(n) => {
                    let n = n.clone();
                    self.r.tag(b'r').str(&n);
                }
                None => {
                    self.r.tag(b'?');
                }
            }
        } else if g.kind == uo_docx_layout::para::GlyphKind::Obj {
            self.r.tag(b'?');
        } else {
            self.r.tag(b'-');
        }
        let pt = g.ptab.and_then(|k| model_run(self.ldoc, k)).and_then(|r| r.ext()).and_then(|e| e.ptab.as_ref());
        self.r.bool(pt.is_some());
        if let Some(pt) = pt {
            self.r.str(&pt.alignment).str(&pt.relative_to).opt_str(pt.leader.as_deref());
        }
        self.r.bool(g.check_box.is_some());
        if let Some((s, c)) = g.check_box {
            self.r.num(s).bool(c);
        }
        self.r.bool(g.math.is_some());
        if let Some(m) = g.math {
            let mb = &self.p.math[m as usize];
            self.r.num(mb.w).num(mb.top).num(mb.bottom).num(mb.line_top).num(mb.line_bottom).u32(u32::try_from(mb.items.len()).unwrap());
            for it in &mb.items {
                self.r.num(f64::from(it.cp)).num(it.x).num(it.y).u32(self.faces[&it.face]).num(it.size_pt).u32(self.styles[&it.style.0]);
            }
            self.r.u32(u32::try_from(mb.rules.len()).unwrap());
            for x in &mb.rules {
                self.r.num(x.x).num(x.y).num(x.w).num(x.h).u16s(&x.color);
            }
        }
    }
}

fn model_run<'l>(ldoc: &'l LDoc<'_>, k: RunKey) -> Option<&'l uo_docx::model::Run> {
    match ldoc.run(k)? {
        RunSrc::Model { run, .. } => Some(run),
        RunSrc::Synth(r) => Some(r),
        RunSrc::Slice { base, .. } => model_run(ldoc, *base),
    }
}

fn level_name(l: &MarkerLevel) -> String {
    match l {
        MarkerLevel::Doc(LevelSrc::Override { num, ilvl }) => format!("o|{num}|{ilvl}"),
        MarkerLevel::Doc(LevelSrc::Abstract { abs, ilvl }) => format!("a|{abs}|{ilvl}"),
        MarkerLevel::Synthetic { ilvl, bullet } => {
            format!("s|{}|{}", uo_jsnum::format::to_js_string(*ilvl), if *bullet { "bullet" } else { "decimal" })
        }
    }
}

fn encode(p: &PreparedPara, fonts: &FaceTable, res: &Resources, ldoc: &LDoc<'_>, names: &HashMap<RunKey, String>) -> Vec<u8> {
    // Enc borrows for the call only
    let mut e = Enc { r: Raw::default(), faces: HashMap::new(), styles: HashMap::new(), p, fonts, res, ldoc: short_ldoc(ldoc), names };
    e.style(p.mark_style.0);
    e.face(p.mark_face);
    if let Some(mk) = &p.marker {
        for g in &mk.glyphs {
            e.refs(g);
        }
    }
    for g in &p.glyphs {
        e.refs(g);
    }
    e.r.tag(b'P').opt_str(p.style_id.as_deref()).str(&digest(&*p.ppr)).u32(e.styles[&p.mark_style.0]).u32(e.faces[&p.mark_face]);
    e.r.num(p.ind_left).num(p.ind_right).num(p.first_line).str(&p.jc).u32(u32::try_from(p.tabs.len()).unwrap());
    for t in &p.tabs {
        e.r.str(t.val.name()).num(t.pos).opt_str(t.leader.map(|l| l.name()));
    }
    e.r.num(p.space_before).num(p.space_after).bool(p.before_auto).bool(p.after_auto).str(p.line_rule.name()).num(p.line);
    e.r.bool(p.contextual).bool(p.keep_next).bool(p.keep_lines).bool(p.widow_control).bool(p.page_break_before).bool(p.empty);
    e.r.bool(p.rev_bars.is_some());
    if let Some(rb) = &p.rev_bars {
        e.r.u32(u32::try_from(rb.ranges.len()).unwrap());
        for &(a, b, m) in &rb.ranges {
            e.r.num(a).num(b).bool(m);
        }
        e.r.bool(rb.whole).bool(rb.mark);
    }
    e.r.bool(p.marker.is_some());
    if let Some(mk) = &p.marker {
        e.r.u16s(&mk.text).num(mk.width).str(&level_name(&mk.level)).str(mk.suff.name()).str(&mk.jc).bool(mk.pic_bullet.is_some());
        e.r.opt_u16s(mk.chapter.as_deref()).u32(u32::try_from(mk.glyphs.len()).unwrap());
        for g in &mk.glyphs {
            e.glyph(g);
        }
    }
    e.r.u32(u32::try_from(p.glyphs.len()).unwrap());
    for g in &p.glyphs {
        e.glyph(g);
    }
    e.r.0
}

/// `Enc` keeps one lifetime for its borrows; the LDoc's own parameter is shortened to it.
fn short_ldoc<'e>(l: &'e LDoc<'_>) -> &'e LDoc<'e> {
    l
}

// ── one document ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Tally {
    docs: usize,
    calls: usize,
    equal: usize,
    pending: usize,
    mismatch: usize,
    docs_equal: usize,
    first: Vec<String>,
    errors: Vec<String>,
}

fn read_env(r: &mut Rd<'_>, fmt: &DocFormatting, ctx: &mut DocContext) -> R<Option<ParaEnv>> {
    let mut env = ParaEnv::default();
    let mut ok = true;
    for k in 0..2 {
        if !r.bool()? {
            continue;
        }
        let key = r.string()?;
        if key == "?" {
            ok = false;
            continue;
        }
        // the literal `{}` of the nested-table measure (table.ts:537)
        if key == "{}" {
            if k == 0 {
                env.table_ppr = Some(Arc::new(PPr::default()));
            } else {
                env.table_rpr = Some(Arc::new(RPr::default()));
            }
            continue;
        }
        let n = r.u32()?;
        let conds: Vec<String> = (0..n).map(|_| r.string()).collect::<R<_>>()?;
        let st = ctx.resolver.table_style(fmt, Some(&key));
        if k == 0 {
            let mut p: PPr = st.p_pr.clone();
            for c in &conds {
                let part = st.cond.get(c).ok_or("cond")?;
                p = word::merge_ppr(Some(&p), part.p_pr.as_deref());
            }
            env.table_ppr = Some(Arc::new(p));
        } else {
            let mut p: RPr = st.r_pr.clone();
            for c in &conds {
                let part = st.cond.get(c).ok_or("cond")?;
                p = word::merge_rpr(Some(&p), part.r_pr.as_deref());
            }
            env.table_rpr = Some(Arc::new(p));
        }
    }
    if r.bool()? {
        env.field = Some(FieldSource::Page);
    }
    if r.bool()? {
        env.numbering = Some(r.bool()?);
    }
    env.dark_background = r.bool()?;
    env.no_page_breaks = r.bool()?;
    env.hf = r.bool()?;
    env.continuation = r.bool()?;
    env.default_color = r.opt_u16s()?.map(|u| String::from_utf16_lossy(&u).into_boxed_str());
    Ok(ok.then_some(env))
}

fn read_events(r: &mut Rd<'_>) -> R<(VecDeque<Ev>, Vec<u8>)> {
    let mut ev = VecDeque::new();
    loop {
        match r.tag()? {
            b'f' => {
                let name = r.u16s()?;
                let instr = r.u16s()?;
                let at = r.num()?;
                let answer = r.opt_u16s()?;
                ev.push_back(Ev::Field { name, instr, at, answer });
            }
            b'm' => {
                let num_id = r.num()?;
                let ilvl = r.num()?;
                let marker = if r.bool()? {
                    let text = r.u16s()?;
                    let kind = r.string()?;
                    let (k1, k2) = (r.string()?, r.string()?);
                    let (n, i, v) = (r.num()?, r.num()?, r.num()?);
                    let list_key = r.string()?;
                    let chapter = r.opt_u16s()?;
                    let parts = if r.bool()? {
                        let n = r.u32()?;
                        Some((0..n).map(|_| r.u16s().map(uo_docx::text::Utf16::from_units)).collect::<R<Vec<_>>>()?)
                    } else {
                        None
                    };
                    let level = if kind == "o" {
                        LevelSrc::Override { num: k1.into(), ilvl: k2.into() }
                    } else {
                        LevelSrc::Abstract { abs: k1.into(), ilvl: k2.into() }
                    };
                    Some(NumberMarker {
                        text: uo_docx::text::Utf16::from_units(text),
                        parts,
                        level,
                        num_id: n,
                        ilvl: i,
                        value: v,
                        list_key: list_key.into(),
                        chapter: chapter.map(uo_docx::text::Utf16::from_units),
                    })
                } else {
                    None
                };
                ev.push_back(Ev::Num { num_id, ilvl, marker });
            }
            b'n' => {
                let kind = r.string()?;
                let id = r.num()?;
                let custom = r.bool()?;
                let mark = r.u16s()?;
                ev.push_back(Ev::Note { kind, id, custom, mark });
            }
            b'E' => return Ok((ev, r.bytes()?)),
            t => return Err(format!("bad f4 event tag {t}")),
        }
    }
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

#[allow(clippy::too_many_lines)]
fn replay_doc(key: &str, dump: &[u8], docx: &Path, transcript: &Transcript, fonts: &FontIndex, t: &mut Tally) -> R<Option<String>> {
    let bytes = std::fs::read(docx).map_err(|e| format!("{}: {e}", docx.display()))?;
    let pkg =
        uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).map_err(|e| format!("{e:?}"))?;
    let doc = read_document(&pkg, &ReadOptions::default()).map_err(|e| format!("{e:?}"))?;
    let model = build_model(&doc);
    let fmt = model.ldoc.view.formatting();
    let provider = TranscriptProvider::new(transcript, load_answers(transcript, fonts));
    let mut r = Rd { b: dump, at: 0 };
    let mut opts = LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    };
    if r.tag()? != b'O' {
        return Err("f4 dump without options".into());
    }
    let mode = r.string()?;
    opts.lang = r.string()?.into();
    opts.hyphenation = Hyphenation::On;
    if !mode.is_empty() {
        let m = match mode.as_str() {
            "simple" => RevisionMode::Simple,
            "all" => RevisionMode::All,
            "original" => RevisionMode::Original,
            _ => RevisionMode::Final,
        };
        let order = (m != RevisionMode::Final).then(|| model.ldoc.view.revision_authors().into_iter().map(Into::into).collect());
        opts.revisions = Some(RevisionView { mode: m, order, ..RevisionView::default() });
    }
    let input = PassInput::default();
    let mut ftable = FaceTable::new();
    let mut resolver = FaceResolverCache::new();
    let mut res = Resources::new();
    let mut hyph = HyphenSessions::new(hyph_registry());
    let mut budget = Budget::unlimited();
    let mut line_seq = 0u32;
    let ctx0 = DocContext::new(
        word::StyleResolver::new(),
        word::numbering_state(&opts.lang, &opts.limits),
        word::revision_display(opts.revisions.as_ref()),
    );
    let mut flow = Flow { ctx: ctx0, events: VecDeque::new(), off: Vec::new() };
    let mut facet = Stream::new();
    let mut all = true;
    loop {
        match r.tag()? {
            b'Z' => break,
            b'Q' => {}
            x => return Err(format!("bad f4 record tag {x}")),
        }
        let _call = r.u32()?;
        let pref = r.string()?;
        let is_model = r.bool()?;
        let env = read_env(&mut r, fmt, &mut flow.ctx)?;
        let (events, ts) = read_events(&mut r)?;
        facet.bytes(&ts);
        t.calls += 1;
        let pid = if is_model { model.paras.get(&pref).copied() } else { None };
        let (Some(pid), Some(env)) = (pid, env) else {
            t.pending += 1;
            all = false;
            continue;
        };
        flow.events = events;
        flow.off.clear();
        let mut cx = Cx {
            ldoc: &model.ldoc,
            input: &input,
            opts: &opts,
            fonts: &mut ftable,
            provider: &provider,
            resolver: &mut resolver,
            res: &mut res,
            hyph: &mut hyph,
            budget: &mut budget,
            depth: 0,
            line_seq: &mut line_seq,
        };
        let out = prepare_paragraph(&mut cx, &mut flow, pid, &env);
        let p = match out {
            Ok(p) => p,
            Err(e) => {
                t.mismatch += 1;
                if t.first.len() < 20 {
                    t.first.push(format!("{key} {pref}: Rust error {e:?}"));
                }
                continue;
            }
        };
        let mine = encode(&p, &ftable, &res, &model.ldoc, &model.run_names);
        if mine == ts && flow.off.is_empty() && flow.events.is_empty() {
            t.equal += 1;
        } else {
            t.mismatch += 1;
            if let Ok(d) = std::env::var("UO_F4_DEBUG") {
                let _ = std::fs::write(format!("{d}/mine-{}.bin", t.mismatch), &mine);
                let _ = std::fs::write(format!("{d}/ts-{}.bin", t.mismatch), &ts);
            }
            if t.first.len() < 20 {
                let at = mine.iter().zip(&ts).position(|(a, b)| a != b).unwrap_or(mine.len().min(ts.len()));
                t.first.push(format!(
                    "{key} {pref}: differs at byte {at} of {}/{} (flow: {:?}, events left {})",
                    mine.len(),
                    ts.len(),
                    flow.off,
                    flow.events.len()
                ));
            }
        }
    }
    let n = facet.items();
    Ok(all.then(|| format!("{n}:{}", facet.finish().short())))
}

fn run(capture: &Path, dump_dir: &Path, docs: &Path) -> Tally {
    let cap = Capture::read(capture).unwrap();
    let mut rep = Replay::new(&cap);
    let mut fonts = FontIndex::open_fonts();
    let mut t = Tally::default();
    for rec in cap.records() {
        let key = rec.key.as_str();
        let safe = key.replace(['/', '\\', ':'], "__");
        let Ok(dump) = std::fs::read(dump_dir.join(format!("{safe}.f4.bin"))) else { continue };
        let Ok(tr) = Transcript::read(&dump_dir.join(format!("{safe}.transcript.tsv"))) else { continue };
        if tr.answers.iter().any(|a| a.embedded) {
            // embedded-font bytes are not in the index (as F0/F1)
            continue;
        }
        fonts.add_transcript_sources(&tr);
        let docx = [docs.join(format!("{key}.docx")), docs.join(format!("{safe}.docx"))].into_iter().find(|p| p.exists());
        let Some(docx) = docx else {
            t.errors.push(format!("{key}: docx not found"));
            continue;
        };
        t.docs += 1;
        match replay_doc(key, &dump, &docx, &tr, &fonts, &mut t) {
            Ok(Some(d)) => {
                if rep.check(key, "p3.prepare", &d) {
                    t.docs_equal += 1;
                }
            }
            Ok(None) => {}
            Err(e) => t.errors.push(format!("{key}: {e}")),
        }
    }
    t
}

#[test]
fn f4_prepare_committed_fixture() {
    let dir = common::manifest().join("tests/captures/f4");
    let t = run(&dir.join("layout-ci.capture"), &dir, &dir.join("docs"));
    eprintln!("{t:?}");
    assert!(t.equal > 0, "{t:?}");
    assert_eq!(t.mismatch, 0, "{:#?}", t.first);
    assert!(t.errors.is_empty(), "{t:?}");
}

#[test]
fn f4_prepare_against_a_fresh_capture() {
    let (Ok(cap), Ok(dump), Ok(docs)) = (std::env::var("UO_F4_CAPTURE"), std::env::var("UO_F4_DUMP"), std::env::var("UO_F4_DOCS")) else {
        eprintln!("f4 facets: set UO_F4_CAPTURE, UO_F4_DUMP and UO_F4_DOCS to replay a fresh capture");
        return;
    };
    let t = run(&PathBuf::from(cap), &PathBuf::from(dump), &PathBuf::from(docs));
    eprintln!(
        "f4: {} documents ({} with every call equal), {} calls: {} equal, {} pending, {} mismatches",
        t.docs, t.docs_equal, t.calls, t.equal, t.pending, t.mismatch
    );
    assert_eq!(t.mismatch, 0, "{:#?}", t.first);
    assert!(t.errors.is_empty(), "{:#?}", t.errors);
}
