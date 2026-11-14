//! The F6 facets (record "Stage 3 — F6"): `p3.table` (every `layoutTable` call), `p3.rowsplit`
//! (every `row.split(avail)` the engine makes) and `p3.splitcontent` (every `splitContent` the
//! engine makes), replayed from the inputs `tools/lib/facets-f6.mjs` dumped (`<key>.f6.bin`): the
//! docx is read by the uo-docx reader, the table found by its model position, the `CellLayouter`
//! answers from the TS transcript (the `layoutCell` contents decoded from their P1 encoding, the
//! measuring `prepareParagraph` and `paraVisibility` answers), the style cascade and the borders
//! are the Rust ones; the Rust `TableLayout` (and the split parts) are encoded as the TS ones and
//! compared byte for byte, then the per-document facet digests are rebuilt.
//!
//! Runs the committed fixture (`tests/captures/f6`) always; a fresh capture with
//! `UO_F6_CAPTURE=<capture> UO_F6_DUMP=<dump dir> UO_F6_DOCS=<dir the keys are relative to>`.
//! A call whose inputs cannot be rebuilt (a table outside the walked stories, a nested row whose
//! split closure came from a split the engine did not record) is counted as pending.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::decode::{Share, decode_p1_shared};
use common::{FontIndex, Transcript, TranscriptProvider, load_answers};
use uo_docx::DocView;
use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::ids::ParaId;
use uo_docx::js::JsValue;
use uo_docx::model::Document;
use uo_docx::model::story::{Block, Story};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx_layout::canon::canon_layout;
use uo_docx_layout::context::DocContext;
use uo_docx_layout::engine::{Cx, PassInput};
use uo_docx_layout::error::{LResult, LayoutError};
use uo_docx_layout::fonts::{FaceResolverCache, FaceTable, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::items::{ItemSide, PassItems, PassLine};
use uo_docx_layout::keys::{ItemMeta, LParaId, LineSeq, ObjId, TbKey};
use uo_docx_layout::ldoc::LDoc;
use uo_docx_layout::model::{Cell, CellKey, HfPartId, NoteKind, NumKey, Paragraph, Table};
use uo_docx_layout::options::{Budget, LayoutOptions};
use uo_docx_layout::output::{
    CharsSoA, HfSlot, HfWhich, Layout, LayoutPage, LineGeom, LineParaFormat, LineRef, LineStory, RefPath, TabAlign, TabStop,
};
use uo_docx_layout::para::{
    Ch, Glyph, GlyphFlags, GlyphKind, LineRule, MarkerLevel, MarkerSuffix, ParaEnv, ParaMarker, PreparedPara, RunStyle, StyleIx,
};
use uo_docx_layout::table::{
    CellContent, CellEnv, CellLayouter, MergeAt, MergeFill, MergeText, NestLine, NestRow, ParaSplitInfo, RowCell, RowLayout, RowSplit,
    TableLayout, layout_table, split_content,
};
use uo_docx_layout::word;
use uo_render::{FaceId, OptF64, Resources, Span};
use uo_testkit::{Capture, Replay, Stream};

// ── the raw Sink encoding ────────────────────────────────────────────────────────────────

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

type R<T> = Result<T, String>;

impl Rd<'_> {
    fn take(&mut self, n: usize) -> R<&[u8]> {
        let s = self.b.get(self.at..self.at + n).ok_or("truncated f6 dump")?;
        self.at += n;
        Ok(s)
    }
    fn tag(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> R<i64> {
        let v = self.u32()?;
        Ok(if v == u32::MAX { -1 } else { i64::from(v) })
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
        Ok(self.bytes()?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }
    fn string(&mut self) -> R<String> {
        Ok(String::from_utf16_lossy(&self.u16s()?))
    }
    fn opt_num(&mut self) -> R<OptF64> {
        Ok(if self.bool()? { OptF64::some(self.num()?) } else { OptF64::NONE })
    }
    fn opt_string(&mut self) -> R<Option<String>> {
        Ok(if self.bool()? { Some(self.string()?) } else { None })
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
    fn i32(&mut self, v: Option<usize>) -> &mut Self {
        self.u32(v.map_or(u32::MAX, |v| u32::try_from(v).unwrap()))
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
    fn str(&mut self, s: &str) -> &mut Self {
        let b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
        self.bytes(&b)
    }
    fn opt_num(&mut self, v: OptF64) -> &mut Self {
        self.bool(v.get().is_some());
        if let Some(v) = v.get() {
            self.num(v);
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

/// The uo-docx canonical digest of a model value (canon.mjs `digest`).
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

/// The P1 sink of a value (raw bytes).
#[derive(Default)]
struct RawSink(Vec<u8>);

impl uo_docx_layout::canon::CanonSink for RawSink {
    fn tag(&mut self, c: u8) {
        self.0.push(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bool(&mut self, b: bool) {
        self.0.push(u8::from(b));
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(&uo_testkit::Digest::of(b).0);
    }
}

// ── the model positions (facets-f6.mjs `positions`) ──────────────────────────────────────

struct Model<'d> {
    tables: HashMap<String, &'d Table>,
    paras: HashMap<ParaId, String>,
}

fn build_model(doc: &Document) -> Model<'_> {
    let view = DocView::new(doc);
    let mut m = Model { tables: HashMap::new(), paras: HashMap::new() };
    walk(&mut m, &doc.body, "");
    let mut seen = Vec::new();
    let mut n = 0usize;
    for s in doc.sections.iter() {
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
                    walk(&mut m, st, &name);
                }
            }
        }
    }
    for (notes, prefix) in [(&doc.notes.footnotes, "fn"), (&doc.notes.endnotes, "en")] {
        for (id, st) in notes {
            walk(&mut m, st, &format!("{prefix}{}", uo_jsnum::format::to_js_string(id.get())));
        }
    }
    m
}

fn strip(s: &str) -> String {
    s.strip_prefix('/').unwrap_or(s).to_owned()
}

fn walk<'d>(m: &mut Model<'d>, story: &'d Story, prefix: &str) {
    for (i, b) in story.blocks().enumerate() {
        let name = format!("{prefix}/b{i}");
        match b {
            Block::Paragraph(p) => {
                m.paras.entry(p.id).or_insert_with(|| strip(&name));
                for (k, r) in p.runs.iter().enumerate() {
                    let rname = format!("{name}#{k}");
                    if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                        walk(m, &tb.content, &format!("{rname}/tx"));
                    }
                    if let Some(g) = r.ext().and_then(|e| e.group.as_deref()) {
                        for (j, it) in g.items.iter().enumerate() {
                            if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                                walk(m, &tb.content, &format!("{rname}/g{j}#0/tx"));
                            }
                        }
                    }
                }
            }
            Block::Table(t) => {
                m.tables.entry(strip(&name)).or_insert(t);
                for (ri, row) in t.rows.iter().enumerate() {
                    for (ci, c) in row.cells.iter().enumerate() {
                        walk(m, &c.content, &format!("{name}/r{ri}/c{ci}"));
                    }
                }
            }
        }
    }
}

/// A TS ref string as a `LineRef` (the decoder's rule; `None` for a story it does not know).
fn parse_ref(s: &str) -> Option<LineRef> {
    let path = |s: &str| -> Option<RefPath> {
        if s.is_empty() {
            return Some(RefPath::default());
        }
        let segs = s.split('/').map(|g| g.get(1..).and_then(|n| n.parse().ok())).collect::<Option<Vec<u32>>>()?;
        Some(RefPath(segs.into()))
    };
    if let Some(r) = s.strip_prefix("txbx") {
        return Some(LineRef { story: LineStory::Textbox(TbKey(0)), path: path(r.trim_start_matches('/'))? });
    }
    if let Some(r) = s.strip_prefix("hdr:").or_else(|| s.strip_prefix("ftr:")) {
        let which = if s.starts_with("hdr:") { HfWhich::Header } else { HfWhich::Footer };
        let (slot, r) = r.split_once('/').map_or((r, ""), |(a, b)| (a, b));
        let slot = match slot {
            "first" => HfSlot::First,
            "even" => HfSlot::Even,
            _ => HfSlot::Default,
        };
        return Some(LineRef { story: LineStory::HeaderFooter { which, slot, part: HfPartId(0) }, path: path(r)? });
    }
    if let Some(r) = s.strip_prefix("fn").or_else(|| s.strip_prefix("en")) {
        let kind = if s.starts_with("fn") { NoteKind::Footnote } else { NoteKind::Endnote };
        let (id, r) = r.split_once('/').map_or((r, ""), |(a, b)| (a, b));
        return Some(LineRef { story: LineStory::Note(kind, NumKey::new(uo_jsnum::parse::to_number(id))), path: path(r)? });
    }
    if let Some(r) = s.strip_prefix("preview") {
        return Some(LineRef { story: LineStory::Preview, path: path(r.trim_start_matches('/'))? });
    }
    if !s.starts_with('b') {
        return None;
    }
    Some(LineRef { story: LineStory::Body, path: path(s)? })
}

// ── decoding the TS values ─────────────────────────────────────────────────────────────────

/// The `LATE_ITEMS` marks by value.
#[derive(Default)]
struct Marks {
    vals: Vec<(f64, f64, bool)>,
    by: HashMap<(u64, u64, bool), ItemMeta>,
}

impl Marks {
    fn of(&mut self, z: f64, seq: f64, behind: bool) -> ItemMeta {
        let k = (z.to_bits(), seq.to_bits(), behind);
        *self.by.entry(k).or_insert_with(|| {
            self.vals.push((z, seq, behind));
            ItemMeta(u32::try_from(self.vals.len() - 1).unwrap())
        })
    }
}

/// The per-document replay state: shared resources, marks, identities, the Rust rows by TS id.
struct Doc {
    share: Share,
    marks: Marks,
    next_obj: u32,
    splits: HashMap<String, Box<RowSplit>>,
}

impl Doc {
    fn fresh(&mut self) -> ObjId {
        self.next_obj += 1;
        ObjId(self.next_obj)
    }
}

struct Dec<'p> {
    pages: Vec<Option<LayoutPage>>,
    /// a nested row whose split closure has no known id
    unknown: bool,
    /// decoding an input (the split ids are written)
    ids: bool,
    _p: std::marker::PhantomData<&'p ()>,
}

fn split_value(v: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let mut r = Rd { b: v, at: 0 };
    Ok((r.bytes()?, r.bytes()?))
}

impl Dec<'_> {
    fn new(doc: &mut Doc, p1: &[u8], fonts: &FontIndex, ids: bool) -> R<Self> {
        let d = decode_p1_shared(p1, fonts, &mut doc.share)?;
        Ok(Dec { pages: d.layout.pages.into_iter().map(Some).collect(), unknown: false, ids, _p: std::marker::PhantomData })
    }

    fn page(&mut self, r: &mut Rd<'_>) -> R<LayoutPage> {
        let k = r.u32()? as usize;
        self.pages.get_mut(k).and_then(Option::take).ok_or_else(|| format!("page {k} missing or used twice"))
    }

    fn list(&mut self, r: &mut Rd<'_>, doc: &mut Doc) -> R<PassItems> {
        let p = self.page(r)?;
        let mut out = PassItems { side: vec![ItemSide::default(); p.page.list.items.len()], list: p.page.list };
        for (i, m) in p.item_meta {
            out.side[i as usize].glyph = Some(m);
        }
        for (i, s) in p.clip_src {
            out.side[i as usize].clip_src = Some(s);
        }
        for _ in 0..r.u32()? {
            let i = r.u32()? as usize;
            let (z, seq, behind) = (r.num()?, r.num()?, r.bool()?);
            out.side[i].meta = Some(doc.marks.of(z, seq, behind));
        }
        Ok(out)
    }

    fn lines(&mut self, r: &mut Rd<'_>) -> R<Vec<PassLine>> {
        let p = self.page(r)?;
        Ok(p.lines.iter().map(|l| line_of(&p, l)).collect())
    }

    /// `idx`: ids given to the objects of `items` at the indices (a fresh dead id for -1)
    fn idx_items(r: &mut Rd<'_>, doc: &mut Doc, items: &mut PassItems) -> R<Vec<ObjId>> {
        let n = r.u32()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let i = r.i32()?;
            let id = match usize::try_from(i).ok().and_then(|i| items.side.get_mut(i)) {
                Some(s) => *s.obj.get_or_insert_with(|| doc.fresh()),
                None => doc.fresh(),
            };
            out.push(id);
        }
        Ok(out)
    }

    fn idx_lines(r: &mut Rd<'_>, doc: &mut Doc, lines: &mut [PassLine]) -> R<Vec<ObjId>> {
        let n = r.u32()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let i = r.i32()?;
            let id = match usize::try_from(i).ok().and_then(|i| lines.get_mut(i)) {
                Some(l) => *l.obj.get_or_insert_with(|| doc.fresh()),
                None => doc.fresh(),
            };
            out.push(id);
        }
        Ok(out)
    }

    /// A row and its `split?` flag.
    fn row_split(&mut self, r: &mut Rd<'_>, doc: &mut Doc) -> R<(RowLayout, bool)> {
        let height = r.num()?;
        let (cant_split, header, keep_next) = (r.bool()?, r.bool()?, r.bool()?);
        let (natural, break_w) = (r.opt_num()?, r.opt_num()?);
        let has_split = r.bool()?;
        let (top_cut, cut) = (r.opt_num()?, r.opt_num()?);
        let mut items = self.list(r, doc)?;
        let mut lines = self.lines(r)?;
        let mut border_items = self.list(r, doc)?;
        let break_border_items = if r.bool()? { Some(self.list(r, doc)?) } else { None };
        let merge_at = if r.bool()? {
            let n = r.u32()?;
            Some(
                (0..n)
                    .map(|_| Ok(MergeAt { col: r.num()?, at: num_u32(r.num()?)?, end_row: num_u32(r.num()?)? }))
                    .collect::<R<Vec<_>>>()?,
            )
        } else {
            None
        };
        let merge_text = if r.bool()? {
            let n = r.u32()?;
            let mut v = Vec::new();
            for _ in 0..n {
                let col = r.num()?;
                let ids = Self::idx_items(r, doc, &mut items)?;
                let lids = Self::idx_lines(r, doc, &mut lines)?;
                let mut mi = PassItems::new();
                for id in &ids {
                    if let Some(i) = items.side.iter().position(|s| s.obj == Some(*id)) {
                        mi.push_same(&items, i).unwrap();
                    }
                }
                let ml: Vec<PassLine> = lids.iter().filter_map(|id| lines.iter().find(|l| l.obj == Some(*id)).cloned()).collect();
                let dy = r.num()?;
                let start = r.opt_num()?;
                let content = if r.bool()? { Some(Arc::new(self.content(r, doc)?)) } else { None };
                let (mar_t, band) = (r.opt_num()?, r.opt_num()?);
                let v_align = r.opt_string()?.map(Into::into);
                v.push(MergeText { col, items: mi, lines: ml, dy, start, content, mar_t, band, v_align });
            }
            Some(v)
        } else {
            None
        };
        let top_borders = if r.bool()? { Some(Self::idx_items(r, doc, &mut border_items)?) } else { None };
        let own_top_borders = if r.bool()? { Some(self.list(r, doc)?) } else { None };
        let merge_fills = if r.bool()? {
            let n = r.u32()?;
            let mut v = Vec::new();
            for _ in 0..n {
                let i = r.i32()?;
                let id = match usize::try_from(i).ok().and_then(|i| items.side.get_mut(i)) {
                    Some(s) => *s.obj.get_or_insert_with(|| doc.fresh()),
                    None => doc.fresh(),
                };
                v.push(MergeFill { item: id, end: num_u32(r.num()?)? });
            }
            Some(v)
        } else {
            None
        };
        let cells = if r.bool()? {
            let n = r.u32()?;
            let mut v = Vec::new();
            for _ in 0..n {
                let s = r.string()?;
                let at = parse_ref(&s).ok_or_else(|| format!("ref {s}"))?;
                v.push(RowCell { at, x: r.num()?, w: r.num()?, span: r.num()?, dy: r.opt_num()?, h: r.opt_num()? });
            }
            Some(v)
        } else {
            None
        };
        Ok((
            RowLayout {
                height,
                items,
                lines,
                cant_split,
                header,
                keep_next,
                natural,
                border_items,
                break_border_items,
                merge_at,
                merge_text,
                break_w,
                split: None,
                top_cut,
                top_borders,
                own_top_borders,
                cut,
                merge_fills,
                cells,
            },
            has_split,
        ))
    }

    fn content(&mut self, r: &mut Rd<'_>, doc: &mut Doc) -> R<CellContent> {
        let height = r.num()?;
        let keep_next = r.bool()?;
        let align_h = r.opt_num()?;
        let shape = r.bool()?;
        let mut items = self.list(r, doc)?;
        let mut lines = self.lines(r)?;
        let paras = if r.bool()? {
            let n = r.u32()?;
            let mut m = indexmap::IndexMap::new();
            for _ in 0..n {
                let s = r.string()?;
                let k = parse_ref(&s).ok_or_else(|| format!("ref {s}"))?;
                m.insert(k, ParaSplitInfo { lines: r.num()?, widow: r.bool()?, keep: false, after: r.num()?, band: r.opt_num()? });
            }
            Some(Arc::new(m))
        } else {
            None
        };
        let nest = if r.bool()? {
            let n = r.u32()?;
            let mut v = Vec::new();
            for _ in 0..n {
                let top = r.num()?;
                let (mut row, has_split) = self.row_split(r, doc)?;
                if self.ids {
                    let sid = r.string()?;
                    if has_split {
                        match doc.splits.get(&sid) {
                            Some(s) => row.split = Some(s.clone()),
                            None => self.unknown = true,
                        }
                    }
                }
                let ids = Self::idx_items(r, doc, &mut items)?;
                let m = r.u32()?;
                let mut nl = Vec::new();
                for _ in 0..m {
                    let i = r.i32()?;
                    let s = r.string()?;
                    let at = parse_ref(&s).ok_or_else(|| format!("ref {s}"))?;
                    let obj = match usize::try_from(i).ok().and_then(|i| lines.get_mut(i)) {
                        Some(l) => *l.obj.get_or_insert_with(|| doc.fresh()),
                        None => doc.fresh(),
                    };
                    nl.push(NestLine { obj, at });
                }
                v.push(NestRow { top, row: Box::new(row), items: ids, lines: nl });
            }
            Some(v)
        } else {
            None
        };
        Ok(CellContent { items, lines, height, keep_next, first_keep_next: false, paras, nest, align_h, shape })
    }
}

fn num_u32(v: f64) -> R<u32> {
    if v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX) { Ok(v as u32) } else { Err(format!("not a u32: {v}")) }
}

fn line_of(p: &LayoutPage, l: &LineGeom) -> PassLine {
    let chars = l.chars.and_then(|s| s.range()).map(|r| r.map(|i| (p.chars.o[i], p.chars.x[i], p.chars.w[i])).collect());
    PassLine {
        at: l.at.clone(),
        start: l.start,
        end: l.end,
        x: l.x,
        y: l.y,
        height: l.height,
        baseline: l.baseline,
        desc: l.desc,
        width: l.width,
        chars,
        hyph: l.hyph,
        rev: l.rev,
        frame: l.frame,
        pfmt: l.pfmt.map(|k| Arc::new(p.pfmts[k as usize].clone())),
        meta: None,
        obj: None,
    }
}

// ── encoding the Rust values (facets-f6.mjs `Enc`) ──────────────────────────────────────────

struct Enc<'m> {
    s: Raw,
    pages: Vec<LayoutPage>,
    marks: &'m Marks,
}

impl Enc<'_> {
    fn list(&mut self, items: &PassItems) {
        self.s.u32(u32::try_from(self.pages.len()).unwrap());
        let mut p = LayoutPage::default();
        p.page.list = items.list.clone();
        let mut marked = Vec::new();
        for (i, sd) in items.side.iter().enumerate() {
            let i32_ = u32::try_from(i).unwrap();
            if let Some(m) = sd.glyph {
                p.item_meta.push((i32_, m));
            }
            if let Some(c) = sd.clip_src {
                p.clip_src.push((i32_, c));
            }
            if let Some(m) = sd.meta {
                marked.push((i32_, self.marks.vals[m.0 as usize]));
            }
        }
        self.pages.push(p);
        self.s.u32(u32::try_from(marked.len()).unwrap());
        for (i, (z, seq, behind)) in marked {
            self.s.u32(i).num(z).num(seq).bool(behind);
        }
    }

    fn lines(&mut self, ls: &[PassLine]) {
        self.s.u32(u32::try_from(self.pages.len()).unwrap());
        let mut p = LayoutPage { chars: CharsSoA::default(), ..LayoutPage::default() };
        for l in ls {
            let chars = l.chars.as_ref().map(|cs| {
                let start = u32::try_from(p.chars.o.len()).unwrap();
                for &(o, x, w) in cs.iter() {
                    p.chars.o.push(o);
                    p.chars.x.push(x);
                    p.chars.w.push(w);
                }
                Span { start, len: u32::try_from(cs.len()).unwrap() }
            });
            let pfmt = l.pfmt.as_ref().map(|f| {
                p.pfmts.push(LineParaFormat { left: f.left, right: f.right, first_line: f.first_line, tabs: f.tabs.clone() });
                u32::try_from(p.pfmts.len() - 1).unwrap()
            });
            p.lines.push(LineGeom {
                at: l.at.clone(),
                start: l.start,
                end: l.end,
                x: l.x,
                y: l.y,
                height: l.height,
                baseline: l.baseline,
                desc: l.desc,
                width: l.width,
                chars,
                hyph: l.hyph,
                rev: l.rev,
                frame: l.frame,
                pfmt,
            });
        }
        self.pages.push(p);
    }

    fn idx_items(&mut self, ids: impl Iterator<Item = Option<ObjId>>, items: &PassItems) {
        let ids: Vec<Option<ObjId>> = ids.collect();
        self.s.u32(u32::try_from(ids.len()).unwrap());
        for id in ids {
            self.s.i32(id.and_then(|id| items.side.iter().position(|s| s.obj == Some(id))));
        }
    }

    fn idx_lines(&mut self, ids: impl Iterator<Item = Option<ObjId>>, lines: &[PassLine]) {
        let ids: Vec<Option<ObjId>> = ids.collect();
        self.s.u32(u32::try_from(ids.len()).unwrap());
        for id in ids {
            self.s.i32(id.and_then(|id| lines.iter().position(|l| l.obj == Some(id))));
        }
    }

    fn row(&mut self, r: &RowLayout) {
        self.s
            .num(r.height)
            .bool(r.cant_split)
            .bool(r.header)
            .bool(r.keep_next)
            .opt_num(r.natural)
            .opt_num(r.break_w)
            .bool(r.split.is_some());
        self.s.opt_num(r.top_cut).opt_num(r.cut);
        self.list(&r.items);
        self.lines(&r.lines);
        self.list(&r.border_items);
        self.s.bool(r.break_border_items.is_some());
        if let Some(b) = &r.break_border_items {
            self.list(b);
        }
        self.s.bool(r.merge_at.is_some());
        if let Some(ma) = &r.merge_at {
            self.s.u32(u32::try_from(ma.len()).unwrap());
            for m in ma {
                self.s.num(m.col).num(f64::from(m.at)).num(f64::from(m.end_row));
            }
        }
        self.s.bool(r.merge_text.is_some());
        if let Some(mt) = &r.merge_text {
            self.s.u32(u32::try_from(mt.len()).unwrap());
            for m in mt {
                self.s.num(m.col);
                self.idx_items(m.items.side.iter().map(|s| s.obj), &r.items);
                self.idx_lines(m.lines.iter().map(|l| l.obj), &r.lines);
                self.s.num(m.dy).opt_num(m.start);
                self.s.bool(m.content.is_some());
                if let Some(c) = &m.content {
                    self.content(c);
                }
                self.s.opt_num(m.mar_t).opt_num(m.band).opt_str(m.v_align.as_deref());
            }
        }
        self.s.bool(r.top_borders.is_some());
        if let Some(t) = &r.top_borders {
            self.idx_items(t.iter().map(|o| Some(*o)), &r.border_items);
        }
        self.s.bool(r.own_top_borders.is_some());
        if let Some(o) = &r.own_top_borders {
            self.list(o);
        }
        self.s.bool(r.merge_fills.is_some());
        if let Some(mf) = &r.merge_fills {
            self.s.u32(u32::try_from(mf.len()).unwrap());
            for f in mf {
                self.s.i32(r.items.side.iter().position(|s| s.obj == Some(f.item))).num(f64::from(f.end));
            }
        }
        self.s.bool(r.cells.is_some());
        if let Some(cs) = &r.cells {
            self.s.u32(u32::try_from(cs.len()).unwrap());
            for c in cs {
                self.s.str(&c.at.to_ts()).num(c.x).num(c.w).num(c.span).opt_num(c.dy).opt_num(c.h);
            }
        }
    }

    fn content(&mut self, c: &CellContent) {
        self.s.num(c.height).bool(c.keep_next).opt_num(c.align_h).bool(c.shape);
        self.list(&c.items);
        self.lines(&c.lines);
        self.s.bool(c.paras.is_some());
        if let Some(ps) = &c.paras {
            self.s.u32(u32::try_from(ps.len()).unwrap());
            for (k, v) in ps.iter() {
                self.s.str(&k.to_ts()).num(v.lines).bool(v.widow).num(v.after).opt_num(v.band);
            }
        }
        self.s.bool(c.nest.is_some());
        if let Some(ns) = &c.nest {
            self.s.u32(u32::try_from(ns.len()).unwrap());
            for n in ns {
                self.s.num(n.top);
                self.row(&n.row);
                self.idx_items(n.items.iter().map(|o| Some(*o)), &c.items);
                self.s.u32(u32::try_from(n.lines.len()).unwrap());
                for l in &n.lines {
                    self.s.i32(c.lines.iter().position(|x| x.obj == Some(l.obj))).str(&l.at.to_ts());
                }
            }
        }
    }
}

/// `bytes(structure) bytes(P1 of the pages)`.
fn finish(s: Raw, pages: Vec<LayoutPage>, res: &mut Resources) -> Vec<u8> {
    let layout = Layout { pages, resources: std::mem::take(res), ..Layout::default() };
    let mut p1 = RawSink::default();
    canon_layout(&layout, &|_| None, &mut p1);
    *res = layout.resources;
    let mut out = Raw::default();
    out.bytes(&s.0).bytes(&p1.0);
    out.0
}

fn table_value(t: &TableLayout, doc: &mut Doc) -> Vec<u8> {
    let mut e = Enc { s: Raw::default(), pages: Vec::new(), marks: &doc.marks };
    e.s.num(t.x).num(t.width).opt_num(t.border_left).opt_num(t.eff_r);
    let d = t.tblp_pr.as_deref().map(|p| digest(p));
    e.s.opt_str(d.as_deref()).u32(u32::try_from(t.rows.len()).unwrap());
    for r in &t.rows {
        e.row(r);
    }
    let (s, pages) = (e.s, e.pages);
    finish(s, pages, &mut doc.share.res)
}

fn parts_value<T>(p: Option<&(T, T)>, doc: &mut Doc, each: impl Fn(&mut Enc<'_>, &T)) -> Vec<u8> {
    let mut e = Enc { s: Raw::default(), pages: Vec::new(), marks: &doc.marks };
    e.s.bool(p.is_none());
    if let Some((a, b)) = p {
        e.s.tag(b'P');
        each(&mut e, a);
        each(&mut e, b);
    }
    let (s, pages) = (e.s, e.pages);
    finish(s, pages, &mut doc.share.res)
}

// ── the CellLayouter of the replay ──────────────────────────────────────────────────────────

enum Ev {
    Cell {
        row: u32,
        cell: u32,
        x: f64,
        width: f64,
        ppr: Option<String>,
        rpr: Option<String>,
        dark: bool,
        mar: Option<(f64, f64, OptF64)>,
        cref: String,
        pure: bool,
        content: CellContent,
    },
    Prep {
        pref: String,
        ppr: Option<String>,
        rpr: Option<String>,
        pure: bool,
        prep: PreparedPara,
    },
    Vis {
        pref: String,
        mark_hidden: bool,
        visible: bool,
    },
}

struct Host<'m> {
    ctx: DocContext,
    events: VecDeque<Ev>,
    off: Option<String>,
    pure: i32,
    paras: &'m HashMap<ParaId, String>,
}

fn same(a: f64, b: f64) -> bool {
    uo_jsnum::conv::canonicalize_nan(a).to_bits() == uo_jsnum::conv::canonicalize_nan(b).to_bits()
}

fn env_key(p: Option<&dyn JsValue>, empty: bool) -> Option<String> {
    p.map(|v| if empty { "{}".to_owned() } else { digest(v) })
}

impl Host<'_> {
    fn fail<T>(&mut self, what: String) -> LResult<T> {
        if self.off.is_none() {
            self.off = Some(what);
        }
        Err(LayoutError::Internal("f6 replay: transcript differs"))
    }
}

impl CellLayouter for Host<'_> {
    fn ctx(&mut self) -> &mut DocContext {
        &mut self.ctx
    }

    fn pure_depth(&mut self, delta: i32) {
        self.pure += delta;
    }

    fn layout_cell(
        &mut self,
        _cx: &mut Cx<'_, '_>,
        _cell: &Cell,
        key: CellKey,
        x: f64,
        width: f64,
        env: &CellEnv,
        at: &LineRef,
    ) -> LResult<CellContent> {
        let Some(Ev::Cell { row, cell, x: tx, width: tw, ppr, rpr, dark, mar, cref, pure, content }) = self.events.pop_front() else {
            return self.fail(format!("layoutCell {} not in the transcript", at.to_ts()));
        };
        let mine_ppr = env.table_ppr.as_deref().map(|p| digest(p));
        let mine_rpr = env.table_rpr.as_deref().map(|p| digest(p));
        let mar_eq = match (env.cell_mar, mar) {
            (None, None) => true,
            (Some((l, r, t)), Some((l2, r2, t2))) => same(l, l2) && same(r, r2) && t.get().map(f64::to_bits) == t2.get().map(f64::to_bits),
            _ => false,
        };
        let ok = key.row == row
            && key.cell == cell
            && same(x, tx)
            && same(width, tw)
            && mine_ppr == ppr
            && mine_rpr == rpr
            && env.dark_background == dark
            && mar_eq
            && at.to_ts() == cref
            && (self.pure > 0) == pure;
        if !ok {
            return self.fail(format!(
                "layoutCell differs: rust r{}c{} x={x} w={width} {} pure={} dark={} mar={:?} | ts r{row}c{cell} x={tx} w={tw} {cref} pure={pure} dark={dark} mar={mar:?} ppr {} rpr {}",
                key.row,
                key.cell,
                at.to_ts(),
                self.pure > 0,
                env.dark_background,
                env.cell_mar,
                mine_ppr == ppr,
                mine_rpr == rpr
            ));
        }
        Ok(content)
    }

    fn prepare(&mut self, _cx: &mut Cx<'_, '_>, para: &Paragraph, env: &ParaEnv) -> LResult<PreparedPara> {
        let Some(Ev::Prep { pref, ppr, rpr, pure, prep }) = self.events.pop_front() else {
            return self.fail("prepareParagraph not in the transcript".into());
        };
        let mine = self.paras.get(&para.id).cloned().unwrap_or_default();
        let empty_p = env.table_ppr.as_deref().is_some_and(|p| word::obj::key_indices(p).is_empty());
        let empty_r = env.table_rpr.as_deref().is_some_and(|p| word::obj::key_indices(p).is_empty());
        let mp = env_key(env.table_ppr.as_deref().map(|p| p as &dyn JsValue), empty_p);
        let mr = env_key(env.table_rpr.as_deref().map(|p| p as &dyn JsValue), empty_r);
        if mine != pref || mp != ppr || mr != rpr || (self.pure > 0) != pure {
            return self
                .fail(format!("prepareParagraph differs: rust {mine} {mp:?} {mr:?} {} | ts {pref} {ppr:?} {rpr:?} {pure}", self.pure > 0));
        }
        Ok(prep)
    }

    fn visibility(&mut self, _cx: &mut Cx<'_, '_>, para: &Paragraph) -> LResult<(bool, bool)> {
        let Some(Ev::Vis { pref, mark_hidden, visible }) = self.events.pop_front() else {
            return self.fail("paraVisibility not in the transcript".into());
        };
        let mine = self.paras.get(&para.id).cloned().unwrap_or_default();
        if mine != pref {
            return self.fail(format!("paraVisibility differs: rust {mine} | ts {pref}"));
        }
        Ok((mark_hidden, visible))
    }
}

fn kind_of(s: &str) -> GlyphKind {
    match s {
        "space" => GlyphKind::Space,
        "nbsp" => GlyphKind::Nbsp,
        "tab" => GlyphKind::Tab,
        "lb" => GlyphKind::Lb,
        "pb" => GlyphKind::Pb,
        "cb" => GlyphKind::Cb,
        "obj" => GlyphKind::Obj,
        "shy" => GlyphKind::Shy,
        "box" => GlyphKind::Box,
        _ => GlyphKind::Char,
    }
}

/// The `PreparedPara` of a `'p'` record: what `paraContentWidths` reads.
fn read_prep(r: &mut Rd<'_>) -> R<PreparedPara> {
    let (ind_left, ind_right, first_line) = (r.num()?, r.num()?, r.num()?);
    let n = r.u32()?;
    let mut tabs = Vec::new();
    for _ in 0..n {
        let val = if r.string()? == "bar" { TabAlign::Bar } else { TabAlign::Left };
        tabs.push(TabStop { val, pos: r.num()?, leader: None });
    }
    let marker = if r.bool()? {
        let width = r.num()?;
        let suff = match r.string()?.as_str() {
            "tab" => MarkerSuffix::Tab,
            "space" => MarkerSuffix::Space,
            _ => MarkerSuffix::Nothing,
        };
        Some(ParaMarker {
            text: Box::new([]),
            glyphs: Vec::new(),
            width,
            level: MarkerLevel::default(),
            suff,
            jc: "left".into(),
            pic_bullet: None,
            chapter: None,
        })
    } else {
        None
    };
    let size = r.num()?;
    let face = FaceId::from_index(0).unwrap();
    let n = r.u32()?;
    let mut glyphs = Vec::new();
    for _ in 0..n {
        let kind = kind_of(&r.string()?);
        let mut g = Glyph::new(kind, Ch::default(), 0, face, StyleIx(0));
        g.adv = r.num()?;
        if r.bool()? {
            g.flags = GlyphFlags(GlyphFlags::BREAK_AFTER);
        }
        glyphs.push(g);
    }
    Ok(PreparedPara {
        para: LParaId(0),
        ppr: Arc::default(),
        style_id: None,
        styles: vec![RunStyle { size_pt: size, ..RunStyle::default() }],
        mark_style: StyleIx(0),
        mark_face: face,
        glyphs,
        marker,
        ind_left,
        ind_right,
        first_line,
        jc: "left".into(),
        tabs,
        space_before: 0.0,
        space_after: 0.0,
        before_auto: false,
        after_auto: false,
        line_rule: LineRule::Auto,
        line: 0.0,
        contextual: false,
        keep_next: false,
        keep_lines: false,
        widow_control: false,
        page_break_before: false,
        empty: false,
        rev_bars: None,
        math: Vec::new(),
        char_grid: None,
    })
}

// ── the replay ─────────────────────────────────────────────────────────────────────────────

#[derive(Default, Debug)]
struct Tally {
    docs: usize,
    docs_equal: usize,
    tables: usize,
    splits: usize,
    contents: usize,
    equal: usize,
    pending: usize,
    mismatch: usize,
    first: Vec<String>,
    errors: Vec<String>,
    pendings: Vec<String>,
}

struct Facets {
    table: Stream,
    rowsplit: Stream,
    splitcontent: Stream,
    all: bool,
}

#[allow(clippy::too_many_lines)]
fn replay_doc(key: &str, dump: &[u8], docx: &Path, transcript: &Transcript, fonts: &FontIndex, t: &mut Tally) -> R<Option<Facets>> {
    let bytes = std::fs::read(docx).map_err(|e| format!("{}: {e}", docx.display()))?;
    let pkg =
        uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).map_err(|e| format!("{e:?}"))?;
    let doc = read_document(&pkg, &ReadOptions::default()).map_err(|e| format!("{e:?}"))?;
    let model = build_model(&doc);
    let ldoc = LDoc::empty(DocView::new(&doc));
    let provider = TranscriptProvider::new(transcript, load_answers(transcript, fonts));
    let opts = LayoutOptions::default().ts_strict(); // wave 7c (W): a replay of the frozen TS
    let input = PassInput::default();
    let mut ftable = FaceTable::new();
    let mut resolver = FaceResolverCache::new();
    let mut hyph = HyphenSessions::new(Arc::new(uo_fonts::hyphen::HyphenationRegistry::new()));
    let mut budget = Budget::unlimited();
    let mut line_seq = 0u32;
    let ctx = DocContext::new(word::StyleResolver::new(), word::numbering_state(&opts.lang, &opts.limits), word::revision_display(None));
    let mut st = Doc { share: Share::default(), marks: Marks::default(), next_obj: 0, splits: HashMap::new() };
    let mut host = Host { ctx, events: VecDeque::new(), off: None, pure: 0, paras: &model.paras };
    let mut f = Facets { table: Stream::new(), rowsplit: Stream::new(), splitcontent: Stream::new(), all: true };
    let mut n_split = 0usize;
    let mut r = Rd { b: dump, at: 0 };
    let report = |t: &mut Tally, ok: bool, what: String, mine: &[u8], ts: &[u8]| {
        if ok {
            t.equal += 1;
        } else {
            t.mismatch += 1;
            if let Ok(d) = std::env::var("UO_F6_DEBUG") {
                let _ = std::fs::write(format!("{d}/mine-{}.bin", t.mismatch), mine);
                let _ = std::fs::write(format!("{d}/ts-{}.bin", t.mismatch), ts);
            }
            if t.first.len() < 20 {
                let at = mine.iter().zip(ts).position(|(a, b)| a != b).unwrap_or(mine.len().min(ts.len()));
                t.first.push(format!("{key} {what}: differs at byte {at} of {}/{}", mine.len(), ts.len()));
            }
        }
    };
    loop {
        match r.tag()? {
            b'Z' => break,
            b'T' => {
                t.tables += 1;
                let call = r.u32()?;
                let tref = r.string()?;
                let (frame_x, frame_w) = (r.num()?, r.num()?);
                let sref = r.string()?;
                // the transcript, decoded before the call
                let mut events = VecDeque::new();
                let mut unknown = false;
                loop {
                    match r.tag()? {
                        b'E' => break,
                        b'c' => {
                            let (row, cell, x, width) = (r.u32()?, r.u32()?, r.num()?, r.num()?);
                            let (ppr, rpr, dark) = (r.opt_string()?, r.opt_string()?, r.bool()?);
                            let mar = if r.bool()? { Some((r.num()?, r.num()?, r.opt_num()?)) } else { None };
                            let cref = r.string()?;
                            let pure = r.bool()?;
                            let v = r.bytes()?;
                            let (s, p1) = split_value(&v)?;
                            let mut d = Dec::new(&mut st, &p1, fonts, true)?;
                            let content = d.content(&mut Rd { b: &s, at: 0 }, &mut st)?;
                            unknown |= d.unknown;
                            events.push_back(Ev::Cell { row, cell, x, width, ppr, rpr, dark, mar, cref, pure, content });
                        }
                        b'p' => {
                            let pref = r.string()?;
                            let ppr = if r.bool()? { Some(r.string()?) } else { None };
                            let rpr = if r.bool()? { Some(r.string()?) } else { None };
                            let pure = r.bool()?;
                            let prep = read_prep(&mut r)?;
                            events.push_back(Ev::Prep { pref, ppr, rpr, pure, prep });
                        }
                        b'v' => {
                            let pref = r.string()?;
                            events.push_back(Ev::Vis { pref, mark_hidden: r.bool()?, visible: r.bool()? });
                        }
                        x => return Err(format!("bad transcript tag {x}")),
                    }
                }
                let ts = r.bytes()?;
                f.table.bytes(&ts);
                // `<table>+r<k>`: the row-sliced copy a linked textbox chain lays out (F7d)
                let sliced: Option<Table> = tref.split_once("+r").and_then(|(b, k)| {
                    let (t, k) = (model.tables.get(b)?, k.parse::<usize>().ok()?);
                    let mut c = Table::clone(t);
                    c.rows.drain(..k.min(c.rows.len()));
                    Some(c)
                });
                let found = match &sliced {
                    Some(c) => Some(c),
                    None => model.tables.get(&tref).copied(),
                };
                let (Some(table), Some(at)) = (found, parse_ref(&sref)) else {
                    t.pending += 1;
                    t.pendings.push(format!("{key} table {call} at {tref:?} ref {sref}"));
                    f.all = false;
                    continue;
                };
                host.events = events;
                host.off = None;
                host.pure = 0;
                let out = {
                    let mut cx = Cx {
                        ldoc: &ldoc,
                        input: &input,
                        opts: &opts,
                        fonts: &mut ftable,
                        provider: &provider as &dyn FontProvider,
                        resolver: &mut resolver,
                        res: &mut st.share.res,
                        hyph: &mut hyph,
                        budget: &mut budget,
                        depth: 0,
                        line_seq: &mut line_seq,
                        images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
                        memo: None,
                    };
                    layout_table(&mut cx, &mut host, table, frame_x, frame_w, &at)
                };
                let tl = match out {
                    Ok(tl) => tl,
                    Err(e) => {
                        if unknown {
                            t.pending += 1;
                        } else {
                            t.mismatch += 1;
                            if t.first.len() < 20 {
                                t.first.push(format!("{key} table {call} {tref}: Rust error {e:?} ({:?})", host.off));
                            }
                        }
                        f.all = false;
                        continue;
                    }
                };
                for (ri, row) in tl.rows.iter().enumerate() {
                    if let Some(s) = &row.split {
                        st.splits.insert(format!("{call}.{ri}"), s.clone());
                    }
                }
                let mine = table_value(&tl, &mut st);
                let ok = mine == ts && host.events.is_empty() && host.off.is_none();
                if !ok {
                    f.all = false;
                }
                report(t, ok, format!("table {call} {tref} (events left {})", host.events.len()), &mine, &ts);
            }
            b'S' => {
                t.splits += 1;
                let id = r.string()?;
                let avail = r.num()?;
                if r.tag()? != b'E' {
                    return Err("split record".into());
                }
                let ts = r.bytes()?;
                f.rowsplit.bytes(&ts);
                let n = n_split;
                n_split += 1;
                let Some(sp) = st.splits.get(&id).cloned() else {
                    t.pending += 1;
                    f.all = false;
                    continue;
                };
                let out = {
                    let mut cx = Cx {
                        ldoc: &ldoc,
                        input: &input,
                        opts: &opts,
                        fonts: &mut ftable,
                        provider: &provider as &dyn FontProvider,
                        resolver: &mut resolver,
                        res: &mut st.share.res,
                        hyph: &mut hyph,
                        budget: &mut budget,
                        depth: 0,
                        line_seq: &mut line_seq,
                        images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
                        memo: None,
                    };
                    sp.split(&mut cx, avail)
                };
                let p = match out {
                    Ok(p) => p,
                    Err(e) => {
                        t.mismatch += 1;
                        f.all = false;
                        t.first.push(format!("{key} split {id}: Rust error {e:?}"));
                        continue;
                    }
                };
                if let Some((_, b)) = &p
                    && let Some(s) = &b.split
                {
                    st.splits.insert(format!("s{n}b"), s.clone());
                }
                let mine = parts_value(p.as_ref(), &mut st, |e, x| e.row(x));
                let ok = mine == ts;
                if !ok {
                    f.all = false;
                }
                report(t, ok, format!("split {id} at {avail}"), &mine, &ts);
            }
            b'C' => {
                t.contents += 1;
                let input_v = r.bytes()?;
                let avail = r.num()?;
                if r.tag()? != b'E' {
                    return Err("content record".into());
                }
                let ts = r.bytes()?;
                f.splitcontent.bytes(&ts);
                let (s, p1) = split_value(&input_v)?;
                let mut d = Dec::new(&mut st, &p1, fonts, true)?;
                let c = d.content(&mut Rd { b: &s, at: 0 }, &mut st)?;
                if d.unknown {
                    t.pending += 1;
                    f.all = false;
                    continue;
                }
                let out = {
                    let mut cx = Cx {
                        ldoc: &ldoc,
                        input: &input,
                        opts: &opts,
                        fonts: &mut ftable,
                        provider: &provider as &dyn FontProvider,
                        resolver: &mut resolver,
                        res: &mut st.share.res,
                        hyph: &mut hyph,
                        budget: &mut budget,
                        depth: 0,
                        line_seq: &mut line_seq,
                        images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
                        memo: None,
                    };
                    split_content(&mut cx, &c, avail)
                };
                let p = match out {
                    Ok(p) => p,
                    Err(e) => {
                        t.mismatch += 1;
                        f.all = false;
                        t.first.push(format!("{key} splitContent: Rust error {e:?}"));
                        continue;
                    }
                };
                let mine = parts_value(p.as_ref(), &mut st, |e, x| e.content(x));
                let ok = mine == ts;
                if !ok {
                    f.all = false;
                }
                report(t, ok, format!("splitContent at {avail}"), &mine, &ts);
            }
            x => return Err(format!("bad f6 record tag {x}")),
        }
    }
    let _ = LineSeq(0);
    Ok(Some(f))
}

fn run(capture: &Path, dump_dir: &Path, docs: &Path) -> Tally {
    let cap = Capture::read(capture).unwrap();
    let mut rep = Replay::new(&cap);
    let mut fonts = FontIndex::open_fonts();
    let mut t = Tally::default();
    for rec in cap.records() {
        let key = rec.key.as_str();
        let safe = key.replace(['/', '\\', ':'], "__");
        let Ok(dump) = std::fs::read(dump_dir.join(format!("{safe}.f6.bin"))) else { continue };
        let Ok(tr) = Transcript::read(&dump_dir.join(format!("{safe}.transcript.tsv"))) else { continue };
        if tr.answers.iter().any(|a| a.embedded) {
            // embedded-font bytes are not in the index (as F0/F1)
            continue;
        }
        fonts.add_transcript_sources(&tr);
        let media = dump_dir.join(format!("{safe}.media"));
        if let Ok(rd) = std::fs::read_dir(&media) {
            for e in rd.flatten() {
                fonts.add_file(&e.path());
            }
        }
        let docx = [docs.join(format!("{key}.docx")), docs.join(format!("{safe}.docx"))].into_iter().find(|p| p.exists());
        let Some(docx) = docx else {
            t.errors.push(format!("{key}: docx not found"));
            continue;
        };
        t.docs += 1;
        match replay_doc(key, &dump, &docx, &tr, &fonts, &mut t) {
            Ok(Some(f)) if f.all => {
                let d = |s: Stream| {
                    let n = s.items();
                    format!("{n}:{}", s.finish().short())
                };
                let a = rep.check(key, "p3.table", &d(f.table));
                let b = rep.check(key, "p3.rowsplit", &d(f.rowsplit));
                let c = rep.check(key, "p3.splitcontent", &d(f.splitcontent));
                if a && b && c {
                    t.docs_equal += 1;
                } else {
                    t.errors.push(format!("{key}: facet digest differs from the capture (table {a}, rowsplit {b}, splitcontent {c})"));
                }
            }
            Ok(_) => {}
            Err(e) => t.errors.push(format!("{key}: {e}")),
        }
    }
    t
}

#[test]
fn f6_tables_committed_fixture() {
    let dir = common::manifest().join("tests/captures/f6");
    let t = run(&dir.join("layout-ci.capture"), &dir, &dir.join("docs"));
    eprintln!("{t:?}");
    assert!(t.equal > 0, "{t:?}");
    assert_eq!(t.mismatch, 0, "{:#?}", t.first);
    assert!(t.errors.is_empty(), "{t:?}");
}

#[test]
fn f6_tables_against_a_fresh_capture() {
    let (Ok(cap), Ok(dump), Ok(docs)) = (std::env::var("UO_F6_CAPTURE"), std::env::var("UO_F6_DUMP"), std::env::var("UO_F6_DOCS")) else {
        eprintln!("f6 facets: set UO_F6_CAPTURE, UO_F6_DUMP and UO_F6_DOCS to replay a fresh capture");
        return;
    };
    let t = run(&PathBuf::from(cap), &PathBuf::from(dump), &PathBuf::from(docs));
    eprintln!(
        "f6: {} documents ({} with every call equal), {} tables, {} row splits, {} content splits: {} equal, {} pending, {} mismatches",
        t.docs, t.docs_equal, t.tables, t.splits, t.contents, t.equal, t.pending, t.mismatch
    );
    for p in &t.pendings {
        eprintln!("pending: {p}");
    }
    assert_eq!(t.mismatch, 0, "{:#?}", t.first);
    assert!(t.errors.is_empty(), "{:#?}", t.errors);
}
