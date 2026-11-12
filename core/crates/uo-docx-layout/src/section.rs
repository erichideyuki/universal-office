//! The Write section of a page message (bindings gate item G8; uo-render §3.4 "App sections", the
//! `AppSection` hook of frontier E; record "Write page section (G8)"): the Write geometry of one
//! page (`lines` with their `chars`, `pfmts`, `objects`, `cells`, `hfItems`, `section`, `number`,
//! `ClipItem.src`) as the `write.uos` `WritePage` message, plus what the viewer's text layer used
//! to read from the model and the shell no longer has after the switch: each body line's
//! paragraph — its text and its heading level (`paras`, `LaidOutLine.para`).
//!
//! The model-side part is gathered once per layout over the same view ([`SectionModel::new`]):
//! the layout owns no borrow of the model (LA1.5), so the binding keeps a [`SectionModel`] beside
//! its `Layout` and encodes a page on demand (layout LA5: only the pages being painted).
//!
//! Sources (the frozen TS): `universal-document-viewer.ts` `para` (`/^b\d/` refs through
//! `paragraphAtPath(full.blocks, …)`), `text-layer.ts` `fillTextLayer` (the text is
//! `p.runs.map((r) => r.text).join('')`), `annotations.ts` `outlineLevel`/`levelResolver`, and
//! `tools/lib/canon.mjs` `runRefs` (the text of `PlacedObject.run`).

use rustc_hash::FxHashMap;

use crate::codec;
use crate::model::{Block, DocView, DrawingId, HfPartId, NoteKind, ParaId, Paragraph, Run, RunRef, RunSub, Story, StoryRef};
use crate::output::{Layer, Layout, LayoutPage, LineGeom, LineStory, TabAlign, TabLeader};
use crate::refs::Locator;
use crate::word::StyleResolver;
use uo_render::AppSection;

/// The text layer's paragraph (G8): `p.runs.map((r) => r.text).join('')` and `levelOf(p)`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPara {
    /// UTF-16 code units (the lines' `start`/`end` and the chars' `o` index into it)
    pub text: Box<[u16]>,
    /// `levelResolver(full)(p)`: 1–9 for headings in practice, any JS number in general
    pub level: Option<f64>,
}

/// The model-side part of one page's section.
#[derive(Clone, Debug, Default)]
struct PageModel {
    /// per line of `LayoutPage::lines`: the index in [`SectionModel::paras`]
    line_para: Box<[Option<u32>]>,
    /// per object of `LayoutPage::objects`: the `runRefs` name of its run
    object_run: Box<[Option<Box<[u16]>>]>,
}

/// What the Write section reads from the model, gathered once per layout (each paragraph once,
/// whatever the number of pages it spans). Plain data: it outlives the model.
#[derive(Clone, Debug, Default)]
pub struct SectionModel {
    paras: Vec<TextPara>,
    pages: Vec<PageModel>,
}

/// Why a page could not be encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionError {
    /// no page with that index
    NoPage,
    /// the Write message (a count past 32 bits, columns of different lengths)
    Codec(uo_codec::EncodeError),
    /// the render message (uo-render's encoder)
    Render(uo_render::encode::EncodeError),
}

impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SectionError::NoPage => f.write_str("no such page"),
            SectionError::Codec(e) => write!(f, "write section: {e:?}"),
            SectionError::Render(e) => write!(f, "render message: {e}"),
        }
    }
}

impl std::error::Error for SectionError {}

/// Deeper story nesting (textboxes in textboxes, tables in cells) is not named (a run there has
/// no `run` text): the walk is recursive and the model depth is the reader's, not ours.
const MAX_NAME_DEPTH: u32 = 64;

impl SectionModel {
    /// No model: no text-layer paragraphs, no run names (a layout whose document is gone).
    #[must_use]
    pub fn empty() -> SectionModel {
        SectionModel::default()
    }

    /// Gathers, for every page of `layout` (laid out over `view` or over a view transform of it:
    /// Web and Draft keep the blocks), each line's paragraph (text and heading level) and each
    /// placed object's run name.
    #[must_use]
    pub fn new(view: DocView<'_>, layout: &Layout) -> SectionModel {
        let mut g = Gather::new(view);
        let pages = layout.pages.iter().map(|p| g.page(p)).collect();
        SectionModel { paras: g.paras, pages }
    }

    /// The model-side part of page `i` alone (wave 6, L: the editor's shell encodes only the pages it
    /// paints, so a keystroke no longer gathers every page): the same gathering as [`SectionModel::new`]
    /// restricted to that page, its paragraphs deduplicated within the page. The encoded section is
    /// the same bytes (`wire_page` re-indexes the paragraphs per page, in the order its lines first
    /// point at them); the pages before `i` are empty (no allocation).
    #[must_use]
    pub fn page(view: DocView<'_>, layout: &Layout, i: usize) -> SectionModel {
        let Some(p) = layout.pages.get(i) else { return SectionModel::default() };
        let mut g = Gather::new(view);
        let pm = g.page(p);
        let mut pages = vec![PageModel::default(); i];
        pages.push(pm);
        SectionModel { paras: g.paras, pages }
    }

    /// The text-layer paragraphs (each once per layout).
    #[must_use]
    pub fn paras(&self) -> &[TextPara] {
        &self.paras
    }

    /// Bytes held on the heap (M3).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let paras = self.paras.capacity() * std::mem::size_of::<TextPara>() + self.paras.iter().map(|p| p.text.len() * 2).sum::<usize>();
        let pages = self.pages.capacity() * std::mem::size_of::<PageModel>()
            + self
                .pages
                .iter()
                .map(|p| {
                    p.line_para.len() * std::mem::size_of::<Option<u32>>()
                        + p.object_run.len() * std::mem::size_of::<Option<Box<[u16]>>>()
                        + p.object_run.iter().flatten().map(|s| s.len() * 2).sum::<usize>()
                })
                .sum::<usize>();
        paras + pages
    }
}

/// The gathering of [`SectionModel::new`] (per page, each paragraph once).
struct Gather<'a> {
    view: DocView<'a>,
    loc: Locator<'a>,
    styles: StyleResolver,
    index: FxHashMap<ParaId, u32>,
    paras: Vec<TextPara>,
    namer: Option<RunNames>,
}

impl<'a> Gather<'a> {
    fn new(view: DocView<'a>) -> Gather<'a> {
        Gather { view, loc: Locator::new(view), styles: StyleResolver::new(), index: FxHashMap::default(), paras: Vec::new(), namer: None }
    }

    fn page(&mut self, p: &LayoutPage) -> PageModel {
        let view = self.view;
        let line_para = p
            .lines
            .iter()
            .map(|l| {
                let para = text_layer_paragraph(&self.loc, l)?;
                if let Some(&k) = self.index.get(&para.id) {
                    return Some(k);
                }
                let k = u32::try_from(self.paras.len()).ok()?;
                self.paras.push(TextPara { text: paragraph_text(para), level: outline_level(view, &mut self.styles, para) });
                self.index.insert(para.id, k);
                Some(k)
            })
            .collect();
        let object_run = p
            .objects
            .iter()
            .flatten()
            .map(|o| {
                let at = o.run.as_ref()?;
                self.namer.get_or_insert_with(|| RunNames::new(view)).name(at).map(|s| s.encode_utf16().collect())
            })
            .collect();
        PageModel { line_para, object_run }
    }
}

/// `para(line.ref)` of the viewer: `/^b\d/.test(ref) ? paragraphAtPath(full.blocks, ref.split('/'))
/// : null` (a body story with a non-empty path; every other story's ref starts with another letter).
pub(crate) fn text_layer_paragraph<'a>(loc: &Locator<'a>, l: &LineGeom) -> Option<&'a Paragraph> {
    if l.at.story != LineStory::Body {
        return None;
    }
    let path = loc.body_path(&l.at.path.0)?;
    loc.story_paragraph(&StoryRef::Body, &path)
}

/// `p.runs.map((r) => r.text).join('')`.
pub(crate) fn paragraph_text(p: &Paragraph) -> Box<[u16]> {
    p.runs.iter().flat_map(|r| r.text.units().iter().copied()).collect()
}

/// `outlineLevel(p, styles)` of annotations.ts (the viewer's heading level; not the outline rule
/// of the PDF navigation): the direct `outlineLvl`, else the paragraph style's (its `basedOn`
/// chain merged), `lvl >= 0 && lvl <= 8 ? lvl + 1 : undefined`; without either, the style id
/// matched by `/^(?:heading|t[íi]?tulo)\s*(\d)$/i`. `full.formatting` always exists after
/// `readFullDocument`, so the `styles` resolver is never null.
pub(crate) fn outline_level(view: DocView<'_>, styles: &mut StyleResolver, p: &Paragraph) -> Option<f64> {
    let id = p.ppr.as_ref().and_then(|x| x.p_style.as_deref()).or(p.props.style_id.as_deref());
    let lvl = match p.ppr.as_ref().and_then(|x| x.outline_lvl) {
        Some(l) => Some(l.to_number()),
        None => styles.para_style(view.formatting(), id).p_pr.outline_lvl.map(|l| l.to_number()),
    };
    if let Some(lvl) = lvl {
        return (0.0..=8.0).contains(&lvl).then_some(lvl + 1.0);
    }
    heading_name_level(id?)
}

/// `/^(?:heading|t[íi]?tulo)\s*(\d)$/i` → `Number(m[1])`. Non-unicode `i`: ASCII letters fold, and
/// `í`/`Í` fold to each other (no non-ASCII character folds onto an ASCII one, ES Canonicalize).
fn heading_name_level(id: &str) -> Option<f64> {
    let u: Vec<u16> = id.encode_utf16().collect();
    let fold = |c: u16| -> u16 {
        match c {
            0x41..=0x5A => c + 0x20,
            0xCD => 0xED,
            _ => c,
        }
    };
    let eq = |at: usize, s: &str| -> bool { s.encode_utf16().enumerate().all(|(k, c)| u.get(at + k).is_some_and(|&x| fold(x) == c)) };
    let mut at = if eq(0, "heading") {
        7
    } else if u.first().is_some_and(|&c| fold(c) == u16::from(b't')) {
        // t[íi]?tulo
        let mut k = 1;
        if u.get(1).is_some_and(|&c| fold(c) == 0xED || fold(c) == u16::from(b'i')) && eq(2, "tulo") {
            k = 2;
        }
        if !eq(k, "tulo") {
            return None;
        }
        k + 4
    } else {
        return None;
    };
    while u.get(at).is_some_and(|&c| uo_jsnum::text::is_js_space_unit(c)) {
        at += 1;
    }
    let d = *u.get(at)?;
    if !(0x30..=0x39).contains(&d) || at + 1 != u.len() {
        return None;
    }
    Some(f64::from(d - 0x30))
}

/// `runRefs(full)` of tools/lib/canon.mjs for the runs a layout places as objects: the model
/// position printed `<story>/<block path>#<run>` — body `b3#0` (a cell by its index in the row),
/// header/footer parts `hf<n>` by first appearance over sections x [header default first even,
/// footer default first even], notes `fn<id>`/`en<id>`, a textbox's content under the name of
/// the run that holds it (first in walk order) + `/tx`, a group item `<group run>/g<j>#0`. A run
/// of a field result has no name (the TS walk does not enter `fieldResult`).
pub struct RunNames {
    hf: FxHashMap<HfPartId, u32>,
    /// a textbox's content story → `<holder run>/tx`
    tb: FxHashMap<DrawingId, String>,
}

impl RunNames {
    /// The header/footer order and the textbox holders of `view` (one walk of the model).
    #[must_use]
    pub fn new(view: DocView<'_>) -> RunNames {
        let mut n = RunNames { hf: FxHashMap::default(), tb: FxHashMap::default() };
        n.walk(view.body(), "", 0);
        for sec in view.sections() {
            for set in [&sec.headers, &sec.footers] {
                for k in ["default", "first", "even"] {
                    let Some(&part) = set.get(k) else { continue };
                    if n.hf.contains_key(&part) {
                        continue;
                    }
                    let i = u32::try_from(n.hf.len()).unwrap_or(u32::MAX);
                    n.hf.insert(part, i);
                    if let Some(st) = view.hf_story(part) {
                        n.walk(st, &format!("hf{i}"), 0);
                    }
                }
            }
        }
        for (kind, notes) in [("fn", &view.notes().footnotes), ("en", &view.notes().endnotes)] {
            for (id, st) in notes {
                n.walk(st, &format!("{kind}{}", uo_jsnum::format::to_js_string(id.get())), 0);
            }
        }
        n
    }

    /// `walk(blocks, base)`: only the textbox holders are kept (the rest is computed on demand).
    fn walk(&mut self, story: &Story, base: &str, depth: u32) {
        if depth > MAX_NAME_DEPTH {
            return;
        }
        for (i, b) in story.blocks().enumerate() {
            let p = format!("{base}/b{i}");
            match b {
                Block::Paragraph(para) => {
                    for (k, r) in para.runs.iter().enumerate() {
                        self.run(r, &format!("{p}#{k}"), depth);
                    }
                }
                Block::Table(t) => {
                    for (ri, row) in t.rows.iter().enumerate() {
                        for (ci, c) in row.cells.iter().enumerate() {
                            self.walk(&c.content, &format!("{p}/r{ri}/c{ci}"), depth + 1);
                        }
                    }
                }
            }
        }
    }

    fn run(&mut self, r: &Run, name: &str, depth: u32) {
        let Some(e) = r.ext() else { return };
        if let Some(tb) = e.textbox.as_deref() {
            let base = format!("{name}/tx");
            self.tb.entry(tb.drawing).or_insert_with(|| base.strip_prefix('/').unwrap_or(&base).to_owned());
            self.walk(&tb.content, &base, depth + 1);
        }
        if let Some(g) = &e.group {
            for (j, it) in g.items.iter().enumerate() {
                self.run(&it.run, &format!("{name}/g{j}#0"), depth + 1);
            }
        }
    }

    /// The `runRefs` text of the run at `at` (`None`: a field-result run, an unknown story).
    #[must_use]
    pub fn name(&self, at: &RunRef) -> Option<String> {
        let prefix = match at.story {
            StoryRef::Body => String::new(),
            StoryRef::HeaderFooter(p) => format!("hf{}", self.hf.get(&p)?),
            StoryRef::Note(kind, id) => {
                format!("{}{}", if kind == NoteKind::Footnote { "fn" } else { "en" }, uo_jsnum::format::to_js_string(id.get()))
            }
            StoryRef::Textbox(d) => self.tb.get(&d)?.clone(),
            StoryRef::Context => return None,
        };
        let mut s = prefix;
        for (i, seg) in at.path.steps().iter().enumerate() {
            if i > 0 || !s.is_empty() {
                s.push('/');
            }
            s.push(match i % 3 {
                0 => 'b',
                1 => 'r',
                _ => 'c',
            });
            s.push_str(&seg.to_string());
        }
        s.push('#');
        s.push_str(&at.run.to_string());
        match at.sub {
            RunSub::None => Some(s),
            RunSub::GroupItem(j) => Some(format!("{s}/g{j}#0")),
            RunSub::FieldResult(_) => None,
        }
    }
}

// ───────────────────────────── the wire value ─────────────────────────────

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn wire_line(p: &LayoutPage, l: &LineGeom, para: Option<u32>) -> codec::LaidOutLine {
    let chars = l.chars.map(|span| {
        let r = span.range().unwrap_or(0..0);
        let cut = |v: &[f64]| v.get(r.clone()).map(<[f64]>::to_vec).unwrap_or_default();
        codec::LineCharColumns {
            o: p.chars.o.get(r.clone()).map(<[u32]>::to_vec).unwrap_or_default(),
            x: cut(&p.chars.x),
            w: cut(&p.chars.w),
        }
    });
    codec::LaidOutLine {
        r#ref: units(&l.at.to_ts()),
        start: l.start,
        end: l.end,
        x: l.x,
        y: l.y,
        height: l.height,
        baseline: l.baseline,
        width: l.width,
        chars,
        hyph: l.hyph,
        rev: l.rev.map(|(before, after)| codec::RevBar { before, after }),
        frame: l.frame.map(|(l, r)| codec::LineFrame { l, r }),
        pfmt: l.pfmt,
        para,
    }
}

fn tab_align(a: TabAlign) -> codec::TabAlign {
    match a {
        TabAlign::Left => codec::TabAlign::Left,
        TabAlign::Center => codec::TabAlign::Center,
        TabAlign::Right => codec::TabAlign::Right,
        TabAlign::Decimal => codec::TabAlign::Decimal,
        TabAlign::Bar => codec::TabAlign::Bar,
        TabAlign::Clear => codec::TabAlign::Clear,
        TabAlign::Num => codec::TabAlign::Num,
    }
}

fn tab_leader(l: TabLeader) -> codec::TabLeader {
    match l {
        TabLeader::None => codec::TabLeader::None,
        TabLeader::Dot => codec::TabLeader::Dot,
        TabLeader::Hyphen => codec::TabLeader::Hyphen,
        TabLeader::Underscore => codec::TabLeader::Underscore,
        TabLeader::Heavy => codec::TabLeader::Heavy,
        TabLeader::MiddleDot => codec::TabLeader::MiddleDot,
    }
}

/// The `WritePage` of page `p`; `m` its model-side part (absent: no text layer, no run names).
fn wire_page(p: &LayoutPage, m: Option<&PageModel>, all: &[TextPara]) -> codec::WritePage {
    // the page's paragraphs, re-indexed in the order the lines first point at them
    let mut local: FxHashMap<u32, u32> = FxHashMap::default();
    let mut paras = Vec::new();
    let lines = p
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let para = m.and_then(|m| m.line_para.get(i).copied().flatten()).and_then(|g| {
                if let Some(&k) = local.get(&g) {
                    return Some(k);
                }
                let tp = all.get(g as usize)?;
                let k = u32::try_from(paras.len()).ok()?;
                paras.push(codec::TextPara { text: tp.text.to_vec(), level: tp.level });
                local.insert(g, k);
                Some(k)
            });
            wire_line(p, l, para)
        })
        .collect();
    let pfmts = p
        .pfmts
        .iter()
        .map(|f| codec::LineParaFormat {
            left: f.left,
            right: f.right,
            first_line: f.first_line,
            tabs: f.tabs.iter().map(|t| codec::TabSpec { val: tab_align(t.val), pos: t.pos, leader: t.leader.map(tab_leader) }).collect(),
        })
        .collect();
    let objects = p.objects.as_ref().map(|os| {
        os.iter()
            .enumerate()
            .map(|(i, o)| codec::PlacedObject {
                run: m.and_then(|m| m.object_run.get(i)).and_then(|n| n.as_ref()).map(|n| n.to_vec()),
                drawing: o.drawing.map(|d| d.0),
                x: o.x,
                y: o.y,
                w: o.w,
                h: o.h,
                layer: match o.layer {
                    Layer::Behind => codec::Layer::Behind,
                    Layer::Front => codec::Layer::Front,
                },
                hf: o.hf,
                frame: o.frame.map(|f| codec::ObjectFrame {
                    col_x: f.col_x,
                    col_w: f.col_w,
                    para_top: f.para_top,
                    page_w: f.page_w,
                    page_h: f.page_h,
                    mar_l: f.mar_l,
                    mar_r: f.mar_r,
                    mar_t: f.mar_t,
                    mar_b: f.mar_b,
                    body_top: f.body_top,
                }),
                content: o.content.as_ref().map(|(range, xf)| codec::ObjectContent {
                    lines: p
                        .content_lines
                        .get(range.start as usize..range.end as usize)
                        .unwrap_or(&[])
                        .iter()
                        .map(|l| wire_line(p, l, None))
                        .collect(),
                    xf: xf.to_vec(),
                }),
            })
            .collect()
    });
    let cells = p.cells.as_ref().map(|cs| {
        cs.iter().map(|c| codec::PlacedCell { r#ref: units(&c.at.to_ts()), x: c.x, y: c.y, w: c.w, h: c.h, span: c.span }).collect()
    });
    let clip_src =
        p.clip_src.iter().filter_map(|&(item, id)| Some(codec::ClipSrc { item, image: u32::try_from(id.index()).ok()? })).collect();
    codec::WritePage { section: p.section, number: p.number, hf_items: p.hf_items, lines, pfmts, objects, cells, clip_src, paras }
}

/// The encoded Write section of one page: the `WritePage` message bytes, carried as the render
/// message's `app` ([`AppSection`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteSection(pub Vec<u8>);

impl AppSection for WriteSection {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
}

impl Layout {
    /// The `write.uos` `WritePage` of page `i` (the wire value; `None` past the last page).
    #[must_use]
    pub fn write_page(&self, i: usize, m: &SectionModel) -> Option<codec::WritePage> {
        Some(wire_page(self.pages.get(i)?, m.pages.get(i), &m.paras))
    }

    /// The Write section of page `i`: a `WritePage` message stamped with `generation` (the
    /// layout's id, the same as its render messages).
    ///
    /// # Errors
    /// [`SectionError::NoPage`], or the codec's refusal.
    pub fn write_section(&self, i: usize, m: &SectionModel, generation: u32) -> Result<WriteSection, SectionError> {
        let page = self.write_page(i, m).ok_or(SectionError::NoPage)?;
        let mut out = Vec::new();
        codec::encode_write_page(&mut out, &codec::Delta { generation }, &page).map_err(SectionError::Codec)?;
        Ok(WriteSection(out))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::heading_name_level;

    /// `/^(?:heading|t[íi]?tulo)s*(d)$/i` (cases checked against V8).
    #[test]
    fn heading_names_follow_the_js_regex() {
        for (id, lvl) in [
            ("Heading 1", 1.0),
            ("heading1", 1.0),
            ("HEADING 9", 9.0),
            ("Título 2", 2.0),
            ("TÍTULO 3", 3.0),
            ("titulo 4", 4.0),
            ("ttulo 5", 5.0),
            ("heading 6", 6.0),
            ("heading 7", 7.0),
        ] {
            assert_eq!(heading_name_level(id), Some(lvl), "{id:?}");
        }
        for id in ["Heading 10", "Heading", "Heading 1 ", "xheading 1", "Tıtulo 1", ""] {
            assert_eq!(heading_name_level(id), None, "{id:?}");
        }
    }
}
