//! The table of contents and the table of figures (the SAME engine: the `TOC` field), the TOC
//! gallery, the Custom Table of Contents dialog, Remove, Add Text and the TC fields
//! (universal-write.ts 6592–7380). The TOC field spans paragraphs (`Run.fieldBegin`/`fieldEnd`).
//!
//! What the TS reads from the editor's last layout (`pageLabelOf`) arrives here as one oracle
//! question at the start of the command ([`Session::prepare_pages`]); everything else is the
//! model's.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::super::super::command::{CommandError, Outcome};
use super::super::super::settle::block_of_para;
use super::super::super::state::Selection;
use super::super::super::text::{
    CarriedProps, DocPosition, DocRange, insert_index, insert_run, normalize_range, para_length, para_text, split_paragraph,
};
use super::super::super::{ExecCx, Session};
use super::super::context::EditContext;
use super::super::revision_cmds::{StoryLoc, story_at};
use super::super::styles::UiLang;
use super::super::styles::sheet::{Ch, StyleHead, patch_style_xml, put_style, style_raw};
use super::TocMode;
use super::pure::{
    EmptyMsg, TOC_EMPTY, TOF_EMPTY, TocLook, TocOptions, TocSource, TofFormat, parse_toc_instr, ref_field_target, toc_entry_paragraphs,
    toc_instr, tof_format,
};
use super::rx;
use super::toc::{
    AddTextKind, AddTextPlan, MANUAL_LEVELS, TOC_AUTO_INSTR, TOC_GALLERY, TOC_TEMPLATES, TableOfContentsOptions, TocFormat, TocTemplateId,
    add_text_plan, dialog_options_from, manual_placeholder, parse_tc_instr, tc_in_toc, tc_instr, toc_format_style, toc_options_from,
    toc_sdt_pr,
};
use crate::ids::{IdAlloc, ParaId};
use crate::js::{JsNumber, Str};
use crate::model::Document;
use crate::model::para::{BlockSdt, ParaProps, Paragraph, SdtKey};
use crate::model::run::{Ptab, Run, RunProps};
use crate::model::story::{Block, Story};
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::view::{BlockPath, DocRef, RunRef, RunSub, StoryRef};
use crate::word::obj;

fn lower(s: &str) -> String {
    uo_jsnum::text::to_lowercase(s)
}

/// `TocField`: a TOC field of the body: the paragraphs of its begin and end, the runs of its
/// begin and end (their index in the paragraph) and the instruction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TocField {
    pub begin: usize,
    /// `None` = the field has no end (`-1`)
    pub end: Option<usize>,
    pub begin_run: usize,
    pub end_run: Option<usize>,
    pub instr: String,
}

impl TocField {
    pub(super) fn last(&self) -> usize {
        self.end.unwrap_or(self.begin)
    }
}

/// `TableOfFiguresOptions`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableOfFiguresOptions {
    /// the caption label
    pub label: String,
    /// include label and number (else `\a`)
    pub include_label: Option<bool>,
    /// show page numbers (else `\n`)
    pub page_numbers: Option<bool>,
    /// right-align the numbers (else `\p " "`)
    pub right_align: Option<bool>,
    /// the tab leader (`"none"` for none)
    pub leader: Option<String>,
    /// hyperlinks instead of page numbers (`\h \z`)
    pub hyperlinks: Option<bool>,
    pub format: Option<TofFormat>,
}

/// The model half of `paintTocTemplate`: the preview mini-document the shell lays out and paints
/// (the canvas, the crop and the scale stay in the shell).
#[derive(Debug, Clone)]
pub struct TocPreview {
    /// the document with one section: the heading and three entries
    pub document: Document,
    /// the narrow column width of the preview (twips)
    pub width_twips: f64,
    /// the left margin minus 240 (the crop)
    pub crop_left_twips: f64,
    /// the right margin the crop leaves
    pub margin_right_twips: f64,
}

/// What a TOC source row is made of (`tocSources`, read-only half).
struct TocRow {
    id: ParaId,
    plen: usize,
    entries: Vec<RowEntry>,
}

enum RowEntry {
    /// a heading (`\o`, `\u`, `\t`) or a caption: the level, the text and the list number
    Para { level: f64, text: Vec<u16>, number: Vec<u16> },
    /// a TC field: the offset and length of the field run
    Tc { at: usize, len: usize, level: f64, text: String, no_page: bool },
}

/// `TabLeader | 'none'` (docx-core `TabLeader` plus the dialog's "none"): the leader of the tab of
/// the entries, written into a `w:tab` attribute (F10-D20).
fn valid_leader(l: Option<&str>) -> bool {
    l.is_none_or(|l| matches!(l, "none" | "dot" | "hyphen" | "underscore" | "heavy" | "middleDot"))
}

/// `Math.min(9, Math.max(1, level))`.
fn clamp9(level: f64) -> f64 {
    uo_jsnum::math::min(9.0, uo_jsnum::math::max(1.0, level))
}

/// SameValueZero on numbers (the key of the style memo).
fn same_num(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn has_content(r: &Run) -> bool {
    !r.text.is_empty() || r.ext().is_some_and(|e| e.image.is_some() || e.field.as_deref().is_some_and(|f| !f.is_empty()))
}

/// How much of the body [`Session::prepare_pages`] asks the page labels of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllParas {
    /// the source paragraphs of the unchanged model
    No,
    /// every body paragraph, when the unchanged model has any source (the old gallery TOC is dropped first)
    IfSources,
    /// every body paragraph, even with no source now (a selection is deleted first and may merge a heading)
    Always,
}

impl AllParas {
    fn when(c: bool) -> AllParas {
        if c { AllParas::Always } else { AllParas::No }
    }
}

/// Bound of the live walk of a paragraph's TC fields (F10-D22): each visit either advances or finds the mark it made.
const TC_WALK_VISITS: u32 = 1 << 22;

impl Session {
    // ── the fields of the body ─────────────────────────────────────────────────────────────

    /// The body is edited directly (no cell, note, header/footer or textbox).
    fn in_body_ctx(&self) -> bool {
        matches!(self.ctx, EditContext::Body)
    }

    /// `tocFields()`: the TOC fields of the body, by their begin.
    pub(crate) fn toc_fields(&self) -> Vec<TocField> {
        let mut out: Vec<TocField> = Vec::new();
        struct Open {
            para: usize,
            run: usize,
            instr: String,
        }
        let mut stack: Vec<Open> = Vec::new();
        for (pi, p) in self.doc.body.paras.iter().enumerate() {
            for (ri, r) in p.runs.iter().enumerate() {
                let Some(e) = r.ext() else { continue };
                let field = e.field.as_deref().filter(|f| !f.is_empty());
                if e.field_begin.is_some() || (e.field_open == Some(true) && field.is_some()) {
                    let instr = e.field_begin.as_deref().or(field).unwrap_or("").to_owned();
                    stack.push(Open { para: pi, run: ri, instr });
                } else if e.field_end == Some(true)
                    && let Some(b) = stack.pop()
                    && rx::starts_with_word(&b.instr, "TOC")
                {
                    out.push(TocField { begin: b.para, end: Some(pi), begin_run: b.run, end_run: Some(ri), instr: b.instr });
                }
            }
        }
        for b in stack {
            if rx::starts_with_word(&b.instr, "TOC") {
                out.push(TocField { begin: b.para, end: None, begin_run: b.run, end_run: None, instr: b.instr });
            }
        }
        out.sort_by_key(|t| t.begin);
        out
    }

    /// `tocInSelection(t)`.
    pub(crate) fn toc_in_selection(&self, t: &TocField) -> bool {
        if !self.in_body_ctx() {
            return false;
        }
        let DocRange { start, end } = normalize_range(self.range());
        start.para as usize <= t.last() && end.para as usize >= t.begin
    }

    fn tof_fields(&self) -> Vec<TocField> {
        self.toc_fields().into_iter().filter(|t| parse_toc_instr(&t.instr).caption.is_some()).collect()
    }

    /// `tocTables()`: the TOC fields that are tables of contents (not of figures).
    fn toc_tables(&self) -> Vec<TocField> {
        self.toc_fields().into_iter().filter(|t| parse_toc_instr(&t.instr).caption.is_none()).collect()
    }

    /// `tofAtCaret()`: the table of figures at the caret, or the first.
    fn tof_at_caret(&self) -> Option<TocField> {
        let all = self.tof_fields();
        let hit = all.iter().find(|t| self.toc_in_selection(t)).cloned();
        hit.or_else(|| all.into_iter().next())
    }

    /// `tocSdtRange(pi)`: the paragraphs of the TOC content control around body paragraph `pi`.
    fn toc_sdt_range(&self, pi: usize) -> Option<(usize, usize)> {
        if !self.in_body_ctx() {
            return None;
        }
        let ps = &self.doc.body.paras;
        let s = ps.get(pi)?.props.block_sdt.as_ref()?;
        if s.gallery.as_deref() != Some(TOC_GALLERY) {
            return None;
        }
        let key = &s.key;
        let (mut a, mut b) = (pi, pi);
        while a > 0 && ps.get(a - 1).and_then(|p| p.props.block_sdt.as_ref()).is_some_and(|x| x.key == *key) {
            a -= 1;
        }
        while ps.get(b + 1).and_then(|p| p.props.block_sdt.as_ref()).is_some_and(|x| x.key == *key) {
            b += 1;
        }
        Some((a, b))
    }

    /// `firstTocSdt()`.
    fn first_toc_sdt(&self) -> Option<(usize, usize)> {
        let i = self
            .doc
            .body
            .paras
            .iter()
            .position(|p| p.props.block_sdt.as_ref().is_some_and(|s| s.gallery.as_deref() == Some(TOC_GALLERY)))?;
        if !self.in_body_ctx() { None } else { self.toc_sdt_range(i) }
    }

    /// `tocAtCaret()`: the TOC that holds the selection, or the one of the content control the
    /// caret is in (its heading, for example).
    fn toc_at_caret(&self) -> Option<TocField> {
        if self.paras().is_none() || !self.in_body_ctx() {
            return None;
        }
        let all = self.toc_tables();
        if let Some(h) = all.iter().find(|t| self.toc_in_selection(t)) {
            return Some(h.clone());
        }
        let r = self.toc_sdt_range(self.selection.focus.para as usize)?;
        all.into_iter().find(|t| t.begin >= r.0 && t.begin <= r.1)
    }

    /// `tocPageRuns(t)`: the PAGEREF runs of the entries of a TOC (Update page numbers only).
    pub(crate) fn toc_page_runs(&self, t: &TocField) -> Vec<RunRef> {
        let mut out = Vec::new();
        let body = &self.doc.body;
        for pi in t.begin..=t.last() {
            let Some(p) = body.paras.get(pi) else { continue };
            let Ok(block) = u32::try_from(block_of_para(body, pi)) else { continue };
            let path = BlockPath::root(block);
            for (ri, r) in p.runs.iter().enumerate() {
                let at = u32::try_from(ri).unwrap_or(u32::MAX);
                let Some(e) = r.ext() else { continue };
                if e.field.as_deref().is_some_and(|f| rx::starts_with_word(f, "PAGEREF")) {
                    out.push(RunRef { story: StoryRef::Body, path: path.clone(), run: at, sub: RunSub::None });
                }
                for (j, x) in e.field_result.iter().flatten().enumerate() {
                    if x.ext().and_then(|e| e.field.as_deref()).is_some_and(|f| rx::starts_with_word(f, "PAGEREF")) {
                        out.push(RunRef {
                            story: StoryRef::Body,
                            path: path.clone(),
                            run: at,
                            sub: RunSub::FieldResult(u32::try_from(j).unwrap_or(u32::MAX)),
                        });
                    }
                }
            }
        }
        out
    }

    // ── styles ─────────────────────────────────────────────────────────────────────────────

    /// `tocStyleId(name)`: the style of the `\t` switch by name: the `w:name`, the id, or the
    /// display name in any language.
    fn toc_style_id(&mut self, name: &str) -> Option<String> {
        let idx = self.style_index();
        let k = lower(name);
        if let Some(m) = idx.styles.iter().find(|s| lower(&s.name) == k) {
            return Some(m.id.clone());
        }
        if let Some(m) = idx.styles.iter().find(|s| s.id == name) {
            return Some(m.id.clone());
        }
        let k = lower(uo_jsnum::text::trim(name));
        for s in &idx.styles {
            if s.ty != "paragraph" {
                continue;
            }
            for lang in [UiLang::Pt, UiLang::En, UiLang::Es] {
                if lower(&super::super::styles::xform::display_name(s, lang)) == k {
                    return Some(s.id.clone());
                }
            }
        }
        None
    }

    /// `isHeadingStyle(id, level)`: the style is (or inherits from) the built-in heading.
    fn is_heading_style(idx: &super::super::styles::sheet::StyleIndex, id: Option<&str>, level: f64) -> bool {
        let want = format!("heading {}", uo_jsnum::format::to_js_string(level));
        let mut cur = id.map(str::to_owned);
        let mut guard = 0;
        while let Some(c) = cur {
            guard += 1;
            if guard > 12 {
                break;
            }
            let Some(m) = idx.styles.iter().find(|s| s.id == c) else { return false };
            if lower(&m.name) == want {
                return true;
            }
            cur = m.based_on.clone();
        }
        false
    }

    /// `tocLook(o, current, leader)` without the style callback: the Hyperlink style (defined now,
    /// as the TS does), the tab position and the leader.
    fn toc_look(&mut self, o: &TocOptions, cur_tab: Option<(f64, Option<Str>)>, leader: Option<&str>) -> Result<TocLook, CommandError> {
        let sp = &self.sections.body_sect_pr;
        let width = sp.page_width_twips.to_number() - sp.margin_left_twips.to_number() - sp.margin_right_twips.to_number();
        let hyperlink_style = if o.hyperlinks { self.builtin_style_id("Hyperlink")?.to_string() } else { String::new() };
        let leader = match leader {
            Some(l) => Some(l.to_owned()),
            None => Some(match &cur_tab {
                Some((_, l)) => l.as_deref().unwrap_or("none").to_owned(),
                None => "dot".to_owned(),
            }),
        };
        Ok(TocLook { hyperlink_style, tab_pos: cur_tab.map_or(width - 10.0, |t| t.0), leader })
    }

    /// The right tab stop of a paragraph (`tabStops.find(t => t.align === 'right')`).
    fn right_tab(p: Option<&Paragraph>) -> Option<(f64, Option<Str>)> {
        let t = p?.props.tab_stops.as_ref()?.iter().find(|t| &*t.align == "right")?;
        Some((t.pos_twips.to_number(), t.leader.clone()))
    }

    /// The entries of a TOC as paragraphs, defining the styles in the TS order (the first request
    /// of each level creates it).
    fn toc_entries(&mut self, o: &TocOptions, sources: &[TocSource], look: &TocLook) -> Result<(Vec<Paragraph>, bool), CommandError> {
        if !sources.is_empty() {
            let fig = o.caption.is_some();
            let mut memo: Vec<(f64, Str)> = Vec::new();
            let mut failure: Option<CommandError> = None;
            let mut ids = std::mem::take(&mut self.doc.ids);
            let paras = {
                let this = &mut *self;
                toc_entry_paragraphs(o, sources, look, &mut ids, &mut |level: f64| -> Str {
                    if let Some((_, s)) = memo.iter().find(|(l, _)| same_num(*l, level)) {
                        return s.clone();
                    }
                    let name =
                        if fig { "table of figures".to_owned() } else { format!("toc {}", uo_jsnum::format::to_js_string(clamp9(level))) };
                    let s: Str = match this.builtin_style_id(&name) {
                        Ok(s) => s,
                        Err(e) => {
                            failure.get_or_insert(e);
                            "".into()
                        }
                    };
                    memo.push((level, s.clone()));
                    s
                })
            };
            self.doc.ids = ids;
            if let Some(e) = failure {
                return Err(e);
            }
            return Ok((paras, false));
        }
        let lang = self.ui_lang();
        let EmptyMsg { en, pt, es } = if o.caption.is_some() { TOF_EMPTY } else { TOC_EMPTY };
        let msg = match lang {
            UiLang::En => en,
            UiLang::Pt => pt,
            UiLang::Es => es,
        };
        let mut props = default_run_props();
        obj::set(&mut props, "bold", |x| x.bold = Some(true));
        obj::set(&mut props, "noProof", |x| x.no_proof = Some(true));
        let run = Run { text: Utf16::from(msg), props: Arc::new(props), ..Run::default() };
        let id = self.doc.ids.para();
        Ok((vec![Paragraph { runs: vec![run], props: Arc::new(ParaProps::default()), id, ..Paragraph::default() }], true))
    }

    // ── the sources ────────────────────────────────────────────────────────────────────────

    /// The read-only half of `tocSources(o, skip)`: for every paragraph of the body and its cells
    /// that is a source, what the entry will say (the page and the `_Toc` mark are added after).
    fn collect_toc_rows(&mut self, o: &TocOptions, skip: Option<(usize, usize)>) -> Vec<TocRow> {
        let idx = self.style_index();
        let mut style_level: Vec<(String, f64)> = Vec::new();
        if o.caption.is_none() {
            for s in &o.styles {
                if let Some(id) = self.toc_style_id(&s.style) {
                    style_level.retain(|(k, _)| *k != id);
                    style_level.push((id, s.level));
                }
            }
        }
        // the paragraph ids of the skipped body range
        let skipped: HashSet<ParaId> = match skip {
            Some((a, b)) => (a..=b).filter_map(|i| self.doc.body.paras.get(i).map(|p| p.id)).collect(),
            None => HashSet::new(),
        };
        let view = crate::view::DocView::new(&self.doc);
        let all = crate::word::outline::outline_paragraphs(view, None);
        let mut rows: Vec<TocRow> = Vec::new();
        for h in &all {
            let p = h.paragraph;
            if skipped.contains(&p.id) {
                continue;
            }
            let mut entries: Vec<RowEntry> = Vec::new();
            if let Some(cap) = &o.caption {
                let want = lower(cap);
                let mut acc = 0usize;
                let mut seq_end: Option<usize> = None;
                for r in &p.runs {
                    acc += r.text.len();
                    if seq_end.is_none() && lower(&super::pure::seq_label(r.ext().and_then(|e| e.field.as_deref()))) == want {
                        seq_end = Some(acc);
                    }
                }
                let Some(se) = seq_end else { continue };
                let full = para_text(p);
                let slice: &[u16] =
                    if o.include_label == Some(false) { rx::trim_start_units(full.get(se..).unwrap_or(&[])) } else { &full };
                let text: Vec<u16> = slice.iter().map(|&u| if rx::is_tab_nl_ff_vt(u) { 32 } else { u }).collect();
                entries.push(RowEntry::Para { level: 1.0, text: rx::trim_units(&text).to_vec(), number: Vec::new() });
                rows.push(TocRow { id: p.id, plen: para_length(p), entries });
                continue;
            }
            let sid: Option<&str> = p.props.style_id.as_deref().or_else(|| p.ppr.as_ref().and_then(|x| x.p_style.as_deref()));
            let mut level = 0.0_f64;
            let hl = h.level;
            if let Some((_, l)) = sid.and_then(|s| style_level.iter().find(|(k, _)| k == s)) {
                level = *l;
            } else if hl != 0.0
                && let Some((a, b)) = o.outline
                && Self::is_heading_style(&idx, sid, hl)
                && hl >= a
                && hl <= b
            {
                level = hl;
            } else if hl != 0.0 && o.outline_levels && hl >= o.outline.map_or(1.0, |r| r.0) && hl <= o.outline.map_or(9.0, |r| r.1) {
                level = hl;
            }
            let full = para_text(p);
            let text: Vec<u16> = full.iter().map(|&u| if rx::is_tab_nl_ff_vt(u) { 32 } else { u }).collect();
            let text = rx::trim_units(&text).to_vec();
            if level != 0.0 && !level.is_nan() && !text.is_empty() {
                entries.push(RowEntry::Para { level, text, number: h.number.units().to_vec() });
            }
            if o.fields {
                let mut acc = 0usize;
                for r in &p.runs {
                    let at = acc;
                    acc += r.text.len();
                    let Some(tc) = parse_tc_instr(r.ext().and_then(|e| e.field.as_deref())) else { continue };
                    if !tc_in_toc(o, &tc) {
                        continue;
                    }
                    entries.push(RowEntry::Tc { at, len: r.text.len(), level: tc.level, text: tc.text, no_page: tc.no_page });
                }
            }
            if !entries.is_empty() {
                rows.push(TocRow { id: p.id, plen: para_length(p), entries });
            }
        }
        rows
    }

    /// The path of each wanted body paragraph (the body and its cells).
    fn body_para_refs(&self, want: &HashSet<ParaId>) -> HashMap<ParaId, BlockPath> {
        fn walk(
            s: &Story,
            base: Option<(&BlockPath, u32, u32)>,
            want: &HashSet<ParaId>,
            out: &mut HashMap<ParaId, BlockPath>,
            depth: usize,
        ) {
            if depth > 64 {
                return;
            }
            for (bi, b) in s.blocks().enumerate() {
                let Ok(bi32) = u32::try_from(bi) else { continue };
                let path = match base {
                    None => BlockPath::root(bi32),
                    Some((tp, r, c)) => tp.cell_block(r, c, bi32),
                };
                match b {
                    Block::Paragraph(p) => {
                        if want.contains(&p.id) {
                            out.insert(p.id, path);
                        }
                    }
                    Block::Table(t) => {
                        for (ri, row) in t.rows.iter().enumerate() {
                            for (ci, cell) in row.cells.iter().enumerate() {
                                let (Ok(r), Ok(c)) = (u32::try_from(ri), u32::try_from(ci)) else { continue };
                                walk(&cell.content, Some((&path, r, c)), want, out, depth + 1);
                            }
                        }
                    }
                }
            }
        }
        let mut out = HashMap::new();
        walk(&self.doc.body, None, want, &mut out, 0);
        out
    }

    /// The page labels of the layout the shell last showed, for every paragraph a TOC built with
    /// `opts` could take an entry from: ONE oracle question at the start of the command, with the
    /// unchanged model (the TS `pageLabelOf` reads `eng.lines`, the last layout, which no step of
    /// the command refreshes).
    pub(crate) fn prepare_pages(
        &mut self,
        cx: &mut ExecCx<'_>,
        opts: &[(TocOptions, Option<(usize, usize)>)],
        everything: AllParas,
    ) -> Result<(), CommandError> {
        self.f10.pages.clear();
        let mut want: Vec<ParaId> = Vec::new();
        let mut seen: HashSet<ParaId> = HashSet::new();
        for (o, skip) in opts {
            for row in self.collect_toc_rows(o, *skip) {
                if seen.insert(row.id) {
                    want.push(row.id);
                }
            }
        }
        if want.is_empty() && everything != AllParas::Always {
            return Ok(());
        }
        if everything != AllParas::No {
            // the command deletes the selection (and, for the gallery, drops the old TOC) before it reads
            // the sources: the paragraphs that are sources then are not known from the model before it (a
            // paragraph the deletion merges becomes a heading), so the question covers every body
            // paragraph (still ONE call; F10-A4)
            want = self.doc.body.paras.iter().map(|p| p.id).collect();
            seen = want.iter().copied().collect();
        }
        let paths = self.body_para_refs(&seen);
        let mut ids: Vec<ParaId> = Vec::new();
        let mut refs: Vec<DocRef> = Vec::new();
        for id in want {
            if let Some(path) = paths.get(&id) {
                ids.push(id);
                refs.push(DocRef { story: StoryRef::Body, path: path.clone(), offset: 0 });
            }
        }
        let pages = cx.layout.shown_page_numbers(self.view(), &refs).map_err(CommandError::Layout)?;
        for (i, id) in ids.iter().enumerate() {
            let label: Str = match pages.get(i).copied().flatten() {
                Some(n) => uo_jsnum::format::to_js_string(n).into(),
                None => "1".into(),
            };
            self.f10.pages.insert(*id, label);
        }
        Ok(())
    }

    /// `pageLabelOf(p)`: the page the shown layout has the paragraph on ("1" when it has none).
    fn page_label(&self, id: ParaId) -> String {
        self.f10.pages.get(&id).map_or_else(|| "1".to_owned(), ToString::to_string)
    }

    /// `tocSources(o, skip)`: the entries' sources outside the field itself; creates the hidden
    /// `_Toc` bookmark on each (Word recreates every one at each update).
    fn toc_sources(&mut self, o: &TocOptions, skip: Option<(usize, usize)>) -> Result<Vec<TocSource>, CommandError> {
        let rows = self.collect_toc_rows(o, skip);
        let want: HashSet<ParaId> = rows.iter().map(|r| r.id).collect();
        // the host of each paragraph: its story and index (`markStories()`)
        let mut host: HashMap<ParaId, (StoryLoc, u32)> = HashMap::new();
        if !want.is_empty() {
            for s in self.mark_stories() {
                let Some(st) = story_at(&self.doc, &s.loc) else { continue };
                for (i, p) in st.paras.iter().enumerate() {
                    if want.contains(&p.id) {
                        host.insert(p.id, (s.loc.clone(), u32::try_from(i).unwrap_or(u32::MAX)));
                    }
                }
            }
        }
        let mut out: Vec<TocSource> = Vec::new();
        for row in rows {
            let h = host.get(&row.id).cloned();
            let mut tc_walked = false;
            for e in row.entries {
                match e {
                    RowEntry::Para { level, text, number } => {
                        let page = self.page_label(row.id);
                        let bookmark = match &h {
                            Some((loc, idx)) => self.ref_bookmark_in(loc, *idx, 0, u32::try_from(row.plen).unwrap_or(u32::MAX), "_Toc")?,
                            None => String::new(),
                        };
                        out.push(TocSource {
                            level,
                            text: Utf16::from_units(text),
                            number: (!number.is_empty()).then(|| Utf16::from_units(number)),
                            page,
                            bookmark,
                            no_page: false,
                        });
                    }
                    RowEntry::Tc { .. } if h.is_some() && tc_walked => {}
                    RowEntry::Tc { .. } if h.is_some() => {
                        // F10-D22: the TS reads the TC fields in `for (const r of p.runs)` and `refBookmark` splices
                        // the mark runs INTO that array (before the field's run), so the iterator meets the same TC
                        // run a second time (`at` carries on from `acc`). Walk the live paragraph by index.
                        tc_walked = true;
                        let Some((loc, idx)) = &h else { continue };
                        let (mut acc, mut i, mut guard) = (0usize, 0usize, 0u32);
                        loop {
                            guard += 1;
                            if guard > TC_WALK_VISITS {
                                break;
                            }
                            let step = story_at(&self.doc, loc)
                                .and_then(|st| st.paras.get(*idx as usize))
                                .and_then(|p| p.runs.get(i))
                                .map(|r| (r.text.len(), parse_tc_instr(r.ext().and_then(|e| e.field.as_deref()))));
                            let Some((len, tc)) = step else { break };
                            let at = acc;
                            acc += len;
                            i += 1;
                            let Some(tc) = tc else { continue };
                            if !tc_in_toc(o, &tc) {
                                continue;
                            }
                            let bm = self.ref_bookmark_in(
                                loc,
                                *idx,
                                u32::try_from(at).unwrap_or(u32::MAX),
                                u32::try_from(at + len).unwrap_or(u32::MAX),
                                "_Toc",
                            )?;
                            out.push(TocSource {
                                level: tc.level,
                                text: Utf16::from(tc.text.as_str()),
                                number: None,
                                page: self.page_label(row.id),
                                bookmark: bm,
                                no_page: tc.no_page,
                            });
                        }
                    }
                    RowEntry::Tc { at, len, level, text, no_page } => {
                        let bm = match &h {
                            Some((loc, idx)) => self.ref_bookmark_in(
                                loc,
                                *idx,
                                u32::try_from(at).unwrap_or(u32::MAX),
                                u32::try_from(at + len).unwrap_or(u32::MAX),
                                "_Toc",
                            )?,
                            None => String::new(),
                        };
                        out.push(TocSource {
                            level,
                            text: Utf16::from(text.as_str()),
                            number: None,
                            page: self.page_label(row.id),
                            bookmark: bm,
                            no_page,
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// `dropOldTocMarks(oldRuns)`: the `_Toc` bookmarks only the old entries (the body paragraphs
    /// `range`) used go; one a user hyperlink outside still uses stays.
    fn drop_old_toc_marks(&mut self, range: (usize, usize)) {
        let mut names: Vec<String> = Vec::new();
        fn scan(r: &Run, names: &mut Vec<String>) {
            if let Some(l) = r.props.link.as_deref().and_then(|l| l.strip_prefix("#_Toc"))
                && !names.iter().any(|n| n == &format!("_Toc{l}"))
            {
                names.push(format!("_Toc{l}"));
            }
            let t = ref_field_target(r.ext().and_then(|e| e.field.as_deref()));
            if t.starts_with("_Toc") && !names.contains(&t) {
                names.push(t);
            }
            for x in r.ext().and_then(|e| e.field_result.as_ref()).into_iter().flatten() {
                scan(x, names);
            }
        }
        for pi in range.0..=range.1 {
            if let Some(p) = self.doc.body.paras.get(pi) {
                for r in &p.runs {
                    scan(r, &mut names);
                }
            }
        }
        if names.is_empty() {
            return;
        }
        // still used outside the index (a user's hyperlink): it stays
        for s in self.mark_stories() {
            let Some(st) = story_at(&self.doc, &s.loc) else { continue };
            let is_body = matches!(s.loc, StoryLoc::Body);
            for (pi, p) in st.paras.iter().enumerate() {
                if is_body && pi >= range.0 && pi <= range.1 {
                    continue;
                }
                for r in &p.runs {
                    let t = match r.props.link.as_deref().and_then(|l| l.strip_prefix('#')) {
                        Some(x) => x.to_owned(),
                        None => ref_field_target(r.ext().and_then(|e| e.field.as_deref())),
                    };
                    if !t.is_empty() {
                        names.retain(|n| *n != t);
                    }
                }
            }
        }
        for n in names {
            self.drop_bookmark(&n);
        }
    }

    // ── building the field ─────────────────────────────────────────────────────────────────

    /// `tocParagraphs(bp, ep, before, begin, entries, after, same)`: the paragraphs of the TOC
    /// field as Word builds them (the field begins at the end of the lead text and the entries
    /// follow in new paragraphs; the end opens the paragraph after the entries; with no entries the
    /// field fits one paragraph with the message).
    #[allow(clippy::too_many_arguments)]
    fn toc_paragraphs(
        &mut self,
        mut bp: Paragraph,
        ep: &Paragraph,
        before: Vec<Run>,
        begin: Run,
        entries: Vec<Paragraph>,
        mut after: Vec<Run>,
        same: bool,
        empty_msg: bool,
    ) -> Vec<Paragraph> {
        let mut out: Vec<Paragraph> = Vec::new();
        let lead = before.iter().any(has_content);
        let sdt = bp.props.block_sdt.clone();
        let mut end_props = (*ep.props).clone();
        if same {
            obj::delete(&mut end_props, "src");
        }
        let mut first = (*bp.props).clone();
        obj::delete(&mut first, "sectionBreak");
        let in_sdt = |mut p: Paragraph| -> Paragraph {
            if let Some(s) = &sdt {
                let mut props = (*p.props).clone();
                obj::set(&mut props, "blockSdt", |x| x.block_sdt = Some(Arc::clone(s)));
                p.props = Arc::new(props);
            }
            p
        };
        let is_empty = !lead && entries.len() == 1 && empty_msg;
        let mut ep_out = ep.clone();
        let mut ep_ppr_kept = ep.ppr.clone();
        if lead {
            let mut runs = before;
            runs.push(begin);
            bp.runs = runs;
            bp.props = Arc::new(first);
            out.push(bp);
            out.extend(entries.into_iter().map(in_sdt));
        } else {
            let mut it = entries.into_iter();
            let Some(e0) = it.next() else { return out };
            let end_run = if is_empty && after.first().and_then(|r| r.ext()).is_some_and(|e| e.field_end == Some(true)) {
                Some(after.remove(0))
            } else {
                None
            };
            let mut runs = before;
            runs.push(begin);
            runs.extend(e0.runs.iter().cloned());
            if let Some(r) = end_run.clone() {
                runs.push(r);
            }
            bp.runs = runs;
            // `{ ...e0.props, ...(first.src ? { src } : {}), ...(sdt ? { blockSdt } : {}) }`
            let mut props = (*e0.props).clone();
            if let Some(src) = first.src {
                obj::set(&mut props, "src", |x| x.src = Some(src));
            }
            if let Some(s) = &sdt {
                obj::set(&mut props, "blockSdt", |x| x.block_sdt = Some(Arc::clone(s)));
            }
            bp.props = Arc::new(props);
            bp.ppr = None;
            obj::set(&mut bp, "pprEdited", |x| x.ppr_edited = Some(true));
            // the TS `ep.ppr` of a paragraph that is `bp` is gone (`delete bp.ppr`)
            if same {
                ep_ppr_kept = None;
            }
            out.push(bp);
            out.extend(it.map(in_sdt));
            // and the paragraph of the end goes if it only had the end of the field
            let rest = after.iter().any(|r| {
                !r.text.is_empty()
                    || r.ext().is_some_and(|e| {
                        e.image.is_some()
                            || e.field.as_deref().is_some_and(|f| !f.is_empty())
                            || e.field_begin.is_some()
                            || e.field_end == Some(true)
                            || e.bookmark.is_some()
                            || e.textbox.is_some()
                            || e.shape.is_some()
                            || e.group.is_some()
                    })
            });
            let has_break = if same { end_props.section_break.is_some() } else { ep.props.section_break.is_some() };
            if end_run.is_some() && !rest && !has_break {
                return out;
            }
        }
        if same {
            let id = self.doc.ids.para();
            let mut tail = Paragraph { runs: after, props: Arc::new(end_props), id, ..Paragraph::default() };
            if let Some(ppr) = ep_ppr_kept {
                tail.ppr = Some(ppr);
                crate::js::JsObject::mark(&mut tail, "ppr");
            }
            out.push(tail);
        } else {
            ep_out.runs = after;
            out.push(ep_out);
        }
        out
    }

    /// `ps.splice(at, remove, ...items)` on the body.
    fn splice_body(&mut self, at: usize, remove: usize, items: Vec<Paragraph>) {
        for _ in 0..remove {
            if self.doc.body.paras.remove(at).is_none() {
                break;
            }
        }
        for (i, p) in items.into_iter().enumerate() {
            let k = at + i;
            if !self.doc.body.paras.insert(k, Arc::new(p)) {
                break;
            }
        }
    }

    /// `rebuildToc(t, leader)`: the paragraphs from the begin to the end of the field give way to
    /// the new entries (the text before the begin and after the end stays; the paragraph of the
    /// begin keeps its identity).
    pub(crate) fn rebuild_toc(&mut self, t: &TocField, leader: Option<&str>) -> Result<bool, CommandError> {
        let ps = &self.doc.body.paras;
        if ps.get(t.begin).is_none() {
            return Ok(false);
        }
        let end_para = t.last();
        if ps.get(end_para).is_none() {
            return Ok(false);
        }
        let bi = self.field_run_index(t.begin, t.begin_run, &t.instr, false);
        let ei = t.end_run.and_then(|k| self.field_run_index(end_para, k, "", true));
        let o = parse_toc_instr(&t.instr);
        self.drop_old_toc_marks((t.begin, end_para));
        let sources = self.toc_sources(&o, Some((t.begin, end_para)))?;
        let (bp, ep) = {
            let ps = &self.doc.body.paras;
            (ps.get(t.begin).cloned().ok_or(CommandError::Position)?, ps.get(end_para).cloned().ok_or(CommandError::Position)?)
        };
        let lead_text = bi.is_some_and(|b| bp.runs.iter().take(b).any(|r| !r.text.is_empty()));
        let cur = self.doc.body.paras.get(t.begin + usize::from(lead_text)).cloned();
        let look = self.toc_look(&o, Self::right_tab(cur.as_ref()), leader)?;
        let (entries, empty_msg) = self.toc_entries(&o, &sources, &look)?;
        let before: Vec<Run> = match bi {
            Some(b) if b > 0 => bp.runs.iter().take(b).cloned().collect(),
            _ => Vec::new(),
        };
        let after: Vec<Run> = match ei {
            Some(e) => ep.runs.iter().skip(e).cloned().collect(),
            None => vec![Self::field_end_run()],
        };
        let begin = Self::field_begin_run(&format!(" {} ", uo_jsnum::text::trim(&t.instr)));
        let built = self.toc_paragraphs(bp, &ep, before, begin, entries, after, end_para == t.begin, empty_msg);
        let removed = end_para - t.begin + 1;
        let delta = i64::try_from(built.len()).unwrap_or(0) - i64::try_from(removed).unwrap_or(0);
        self.splice_body(t.begin, removed, built);
        self.shift_anchors(u32::try_from(t.begin).unwrap_or(u32::MAX), delta);
        Ok(true)
    }

    /// The index in the paragraph of the field's begin/end run the TocField recorded (the TS finds
    /// it by identity; here the index is kept while the run there is still one, else searched).
    fn field_run_index(&self, para: usize, idx: usize, instr: &str, end: bool) -> Option<usize> {
        let p = self.doc.body.paras.get(para)?;
        let ok = |r: &Run| -> bool {
            let Some(e) = r.ext() else { return false };
            if end {
                e.field_end == Some(true)
            } else {
                let field = e.field.as_deref().filter(|f| !f.is_empty());
                // (the instruction may have changed since: the run is found by being a begin)
                let _ = instr;
                e.field_begin.is_some() || (e.field_open == Some(true) && field.is_some())
            }
        };
        if p.runs.get(idx).is_some_and(ok) {
            return Some(idx);
        }
        p.runs.iter().position(ok)
    }

    fn field_begin_run(instr: &str) -> Run {
        let mut r = Run { props: Arc::new(default_run_props()), ..Run::default() };
        r.ext_mut().field_begin = Some(instr.into());
        r
    }

    fn field_end_run() -> Run {
        let mut r = Run { props: Arc::new(default_run_props()), ..Run::default() };
        r.ext_mut().field_end = Some(true);
        r
    }

    /// `newSdtId()`: a signed 32-bit content control id, as Word.
    fn new_sdt_id(&mut self) -> f64 {
        let r1 = self.random_unit();
        let n = uo_jsnum::math::floor(r1 * 2_147_483_647.0);
        let n = if n == 0.0 { 1.0 } else { n };
        let r2 = self.random_unit();
        if r2 < 0.5 { -n } else { n }
    }

    /// `blockInsertPoint()`: where a new gallery block goes: the selection goes; an empty paragraph
    /// gives its place; at the start, before; at the end, after; in the middle the paragraph splits.
    fn block_insert_point(&mut self) -> Result<usize, CommandError> {
        if self.has_selection() {
            let p = self.delete_selection()?;
            self.selection = Selection::caret(p);
        }
        let pos = self.selection.focus;
        let (len, bare) = {
            let p = self.doc.body.paras.get(pos.para as usize).ok_or(CommandError::Position)?;
            let len = para_length(p);
            let marks = p
                .runs
                .iter()
                .any(|r| r.ext().is_some_and(|e| e.bookmark.is_some() || e.field_begin.is_some() || e.field_end == Some(true)));
            (len, len == 0 && p.props.section_break.is_none() && !marks)
        };
        if bare {
            self.doc.body.paras.remove(pos.para as usize);
            self.shift_anchors(pos.para, -1);
            return Ok(pos.para as usize);
        }
        if pos.offset == 0 {
            return Ok(pos.para as usize);
        }
        if pos.offset as usize >= len {
            return Ok(pos.para as usize + 1);
        }
        let mut ids = std::mem::take(&mut self.doc.ids);
        let r = split_paragraph(&mut self.doc.body.paras, &mut ids, pos);
        self.doc.ids = ids;
        r?;
        self.shift_anchors(pos.para, 1);
        Ok(pos.para as usize + 1)
    }

    /// `dropBodyParas(a, b)`: takes the paragraphs `[a, b]` off the body (and the `_Toc` marks only
    /// their entries used).
    fn drop_body_paras(&mut self, a: usize, b: usize) {
        self.drop_old_toc_marks((a, b));
        for _ in a..=b {
            if self.doc.body.paras.remove(a).is_none() {
                break;
            }
        }
        self.shift_anchors(u32::try_from(a).unwrap_or(u32::MAX), -i64::try_from(b - a + 1).unwrap_or(0));
    }

    /// `manualTocParas(lang, sdt, styleOf)`: the entries of the Manual Table (Word's bb-3):
    /// styles `toc 1–3`, a placeholder, a right alignment tab with dot leader and the number.
    fn manual_toc_paras(
        &mut self,
        lang: UiLang,
        sdt: Option<&Arc<BlockSdt>>,
        style_of: &mut dyn FnMut(&mut Session, &str) -> Result<Str, CommandError>,
    ) -> Result<Vec<Paragraph>, CommandError> {
        let mut out = Vec::new();
        for (i, &level) in MANUAL_LEVELS.iter().enumerate() {
            let bold = level == 1.0;
            let with_bold = || {
                let mut p = default_run_props();
                if bold {
                    obj::set(&mut p, "bold", |x| x.bold = Some(true));
                }
                p
            };
            let r1 = Run { text: Utf16::from(manual_placeholder(lang, level).as_str()), props: Arc::new(with_bold()), ..Run::default() };
            let mut r2 = Run { text: Utf16::from("\t"), props: Arc::new(default_run_props()), ..Run::default() };
            r2.ext_mut().ptab =
                Some(Ptab { alignment: "right".into(), relative_to: "margin".into(), leader: Some("dot".into()), ..Ptab::default() });
            let r3 = Run {
                text: Utf16::from(uo_jsnum::format::to_js_string((i + 1) as f64).as_str()),
                props: Arc::new(with_bold()),
                ..Run::default()
            };
            let mut props = ParaProps {
                style_id: Some(style_of(self, &format!("toc {}", uo_jsnum::format::to_js_string(level)))?),
                ..ParaProps::default()
            };
            if level == 2.0 {
                props.indent_left_twips = Some(JsNumber::Num(216.0));
            } else if level == 3.0 {
                props.indent_left_twips = Some(JsNumber::Num(446.0));
            }
            if let Some(s) = sdt {
                props.block_sdt = Some(Arc::clone(s));
            }
            out.push(Paragraph { runs: vec![r1, r2, r3], props: Arc::new(props), id: self.doc.ids.para(), ..Paragraph::default() });
        }
        Ok(out)
    }

    // ── the table of figures ───────────────────────────────────────────────────────────────

    /// `applyTofFormat(f)`: the style "table of figures" takes the format's pPr/rPr.
    fn apply_tof_format(&mut self, f: TofFormat) -> Result<(), CommandError> {
        let id = self.builtin_style_id("table of figures")?;
        if f == TofFormat::Template {
            return Ok(());
        }
        let base = self.styles_base();
        let Some(raw) = style_raw(&base, &id) else { return Ok(()) };
        let (p_pr, r_pr) = tof_format(f);
        let head = StyleHead {
            p_pr: Ch::Val(format!("<w:pPr>{p_pr}</w:pPr>")),
            r_pr: if r_pr.is_empty() { Ch::Null } else { Ch::Val(format!("<w:rPr>{r_pr}</w:rPr>")) },
            ..StyleHead::default()
        };
        if let Some(x) = put_style(&base, &id, &patch_style_xml(&raw, &head)) {
            self.commit_styles(x);
        }
        Ok(())
    }

    /// The `o` of `insertTableOfFigures(opts)`.
    fn tof_options(opts: &TableOfFiguresOptions) -> TocOptions {
        let links = opts.hyperlinks != Some(false);
        let mut o = TocOptions {
            caption: Some(opts.label.clone()),
            include_label: Some(opts.include_label != Some(false)),
            hyperlinks: links,
            hide_in_web: links,
            ..TocOptions::default()
        };
        if opts.page_numbers == Some(false) {
            o.no_page_numbers = true;
        }
        if opts.page_numbers != Some(false) && opts.right_align == Some(false) {
            o.separator = Some(" ".to_owned());
        }
        o
    }

    /// The common failure path of a command after its step: a layout error rolls the step back,
    /// any other error drops it.
    fn fail_step<T>(&mut self, e: CommandError) -> Result<T, CommandError> {
        self.f10.pages.clear();
        if matches!(e, CommandError::Layout(_)) {
            self.rollback_step();
        } else {
            self.discard_step();
        }
        Err(e)
    }

    /// `insertTableOfFigures(opts)`.
    pub(crate) fn insert_table_of_figures(&mut self, cx: &mut ExecCx<'_>, opts: &TableOfFiguresOptions) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none()
            || self.locked()
            || opts.label.is_empty()
            || !valid_leader(opts.leader.as_deref())
            || !self.in_body_ctx()
            || !self.can_insert_field()
        {
            return self.done_nothing();
        }
        let o = Self::tof_options(opts);
        // with the caret in a table of figures the new one replaces it: its own paragraphs are no sources
        let skip = self.tof_at_caret().filter(|old| self.toc_in_selection(old)).map(|t| (t.begin, t.last()));
        self.prepare_pages(cx, &[(o.clone(), skip)], AllParas::when(skip.is_none() && self.has_selection()))?;
        self.begin_step(false, &[]);
        let leader = opts.leader.clone().unwrap_or_else(|| "dot".to_owned());
        let r = self.insert_table_of_figures_body(cx, opts, &o, &leader);
        self.f10.pages.clear();
        match r {
            Ok(()) => {
                self.answer_bool(true);
                self.touch_all();
                self.done_step()
            }
            Err(e) => self.fail_step(e),
        }
    }

    fn insert_table_of_figures_body(
        &mut self,
        cx: &mut ExecCx<'_>,
        opts: &TableOfFiguresOptions,
        o: &TocOptions,
        leader: &str,
    ) -> Result<(), CommandError> {
        let fmt = opts.format.unwrap_or(TofFormat::Template);
        if let Some(old) = self.tof_at_caret()
            && self.toc_in_selection(&old)
        {
            self.apply_tof_format(fmt)?;
            let t = TocField { instr: toc_instr(o), ..old };
            self.rebuild_toc(&t, Some(leader))?;
            self.refresh_field_cache(cx, None)?;
            return Ok(());
        }
        if self.has_selection() {
            let p = self.delete_selection()?;
            self.selection = Selection::caret(p);
        }
        self.apply_tof_format(fmt)?;
        self.insert_toc_field_at_caret(cx, o, leader)
    }

    /// The new TOC/TOF field at the caret (insertTableOfFigures and insertTocField): the field
    /// begins at the caret, the end opens the paragraph after the entries; the caret goes there.
    fn insert_toc_field_at_caret(&mut self, cx: &mut ExecCx<'_>, o: &TocOptions, leader: &str) -> Result<(), CommandError> {
        let at = self.selection.focus;
        let (before, after) = {
            let p = self.doc.body.paras.make_mut(at.para as usize).ok_or(CommandError::Position)?;
            let idx = insert_index(p, at.offset as usize);
            (p.runs.iter().take(idx).cloned().collect::<Vec<Run>>(), p.runs.iter().skip(idx).cloned().collect::<Vec<Run>>())
        };
        let sources = self.toc_sources(o, None)?;
        let look = self.toc_look(o, None, Some(leader))?;
        let (entries, empty_msg) = self.toc_entries(o, &sources, &look)?;
        let begin = Self::field_begin_run(&toc_instr(o));
        let mut tail = vec![Self::field_end_run()];
        tail.extend(after);
        let p = self.doc.body.paras.get(at.para as usize).cloned().ok_or(CommandError::Position)?;
        let built = self.toc_paragraphs(p.clone(), &p, before, begin, entries, tail, true, empty_msg);
        let n = built.len();
        self.splice_body(at.para as usize, 1, built);
        self.shift_anchors(at.para, i64::try_from(n).unwrap_or(1) - 1);
        // the caret goes to the paragraph of the end
        let end_at = DocPosition::new(at.para + u32::try_from(n).unwrap_or(1) - 1, 0);
        self.selection = Selection::caret(end_at);
        self.refresh_field_cache(cx, None)?;
        Ok(())
    }

    /// `getTableOfFiguresOptions()`.
    #[must_use]
    pub fn table_of_figures_options(&self) -> Option<TableOfFiguresOptions> {
        let t = self.tof_at_caret()?;
        let o = parse_toc_instr(&t.instr);
        let tab = Self::right_tab(self.doc.body.paras.get(t.begin));
        Some(TableOfFiguresOptions {
            label: o.caption.unwrap_or_default(),
            include_label: Some(o.include_label != Some(false)),
            page_numbers: Some(!o.no_page_numbers),
            right_align: Some(o.separator.is_none()),
            hyperlinks: Some(o.hyperlinks),
            leader: Some(match tab {
                Some((_, l)) => l.as_deref().unwrap_or("none").to_owned(),
                None => "dot".to_owned(),
            }),
            format: None,
        })
    }

    /// `updateTableOfFigures()`: the table of figures at the caret, or all of them.
    pub(crate) fn update_table_of_figures(&mut self, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let at = self.tof_at_caret();
        let all = self.tof_fields();
        let list: Vec<TocField> = match at {
            Some(a) if self.toc_in_selection(&a) => vec![a],
            _ => all,
        };
        if list.is_empty() {
            return self.done_nothing();
        }
        let opts: Vec<(TocOptions, Option<(usize, usize)>)> =
            list.iter().map(|t| (parse_toc_instr(&t.instr), Some((t.begin, t.last())))).collect();
        self.prepare_pages(cx, &opts, AllParas::No)?;
        self.set_quiet(true);
        self.begin_step(false, &[]);
        self.set_quiet(false);
        let r: Result<(), CommandError> = (|| {
            for t in list.iter().rev() {
                self.rebuild_toc(t, None)?;
            }
            self.refresh_field_cache(cx, None)?;
            Ok(())
        })();
        self.f10.pages.clear();
        match r {
            Ok(()) => {
                self.answer_bool(true);
                self.touch_all();
                self.done_step()
            }
            Err(e) => self.fail_step(e),
        }
    }

    // ── the table of contents ──────────────────────────────────────────────────────────────

    /// `updateToc(mode)`.
    pub(crate) fn update_toc(&mut self, cx: &mut ExecCx<'_>, mode: TocMode) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none() || self.locked() {
            return self.done_nothing();
        }
        let list: Vec<TocField> = match self.toc_at_caret() {
            Some(a) => vec![a],
            None => self.toc_fields(),
        };
        if list.is_empty() {
            return self.done_nothing();
        }
        if mode == TocMode::All {
            let opts: Vec<(TocOptions, Option<(usize, usize)>)> =
                list.iter().map(|t| (parse_toc_instr(&t.instr), Some((t.begin, t.last())))).collect();
            self.prepare_pages(cx, &opts, AllParas::No)?;
        }
        self.set_quiet(true);
        self.begin_step(false, &[]);
        self.set_quiet(false);
        let r: Result<(), CommandError> = (|| {
            if mode == TocMode::Pages {
                let mut runs: HashSet<RunRef> = HashSet::new();
                for t in &list {
                    runs.extend(self.toc_page_runs(t));
                }
                self.refresh_field_cache(cx, Some(runs))?;
            } else {
                for t in list.iter().rev() {
                    self.rebuild_toc(t, None)?;
                }
                self.refresh_field_cache(cx, None)?;
            }
            Ok(())
        })();
        self.f10.pages.clear();
        match r {
            Ok(()) => {
                self.answer_bool(true);
                self.touch_all();
                self.done_step()
            }
            Err(e) => self.fail_step(e),
        }
    }

    /// `hasTableOfContents()`.
    #[must_use]
    pub fn has_table_of_contents(&self) -> bool {
        !self.toc_tables().is_empty() || self.first_toc_sdt().is_some()
    }

    /// `inTableOfContents()`.
    #[must_use]
    pub fn in_table_of_contents(&self) -> bool {
        self.toc_at_caret().is_some() || self.toc_sdt_range(self.selection.focus.para as usize).is_some()
    }

    /// `tocTemplates()`: the gallery models with their name in the interface language.
    #[must_use]
    pub fn toc_templates(&self) -> Vec<(TocTemplateId, &'static str)> {
        let lang = self.ui_lang();
        TOC_TEMPLATES.iter().map(|t| (t.id, t.name.pick(lang))).collect()
    }

    /// `getTableOfContentsOptions()`: the options of the TOC at the caret (or the first).
    #[must_use]
    pub fn table_of_contents_options(&self) -> Option<TableOfContentsOptions> {
        let t = self.toc_at_caret().or_else(|| self.toc_tables().into_iter().next())?;
        let mut d = dialog_options_from(&parse_toc_instr(&t.instr));
        let ps = &self.doc.body.paras;
        let tab = [ps.get(t.begin), ps.get(t.begin + 1)].into_iter().find_map(|p| Self::right_tab(p));
        d.leader = Some(match tab {
            Some((_, l)) => l.as_deref().unwrap_or("none").to_owned(),
            None => "dot".to_owned(),
        });
        Some(d)
    }

    /// `getTocLevel()`: the level of the caret's paragraph in the table of contents (Add Text: 0 =
    /// Do not show in table of contents).
    #[must_use]
    pub fn toc_level(&self) -> f64 {
        if self.paras().is_none() || !self.in_body_ctx() {
            return 0.0;
        }
        let Some(p) = self.doc.body.paras.get(self.selection.focus.para as usize) else { return 0.0 };
        let view = crate::view::DocView::new(&self.doc);
        crate::word::outline::outline_paragraphs(view, None).into_iter().find(|h| h.paragraph.id == p.id).map_or(0.0, |h| h.level)
    }

    /// `insertTableOfContents(opts, replace)`.
    pub(crate) fn insert_table_of_contents(
        &mut self,
        cx: &mut ExecCx<'_>,
        opts: &TableOfContentsOptions,
        replace: Option<bool>,
    ) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none()
            || self.locked()
            || !valid_leader(opts.leader.as_deref())
            || !self.in_body_ctx()
            || !self.can_insert_field()
        {
            return self.done_nothing();
        }
        let o = toc_options_from(opts);
        let leader = opts.leader.clone().unwrap_or_else(|| "dot".to_owned());
        let old = if replace == Some(false) {
            None
        } else {
            self.toc_at_caret().or_else(|| if replace == Some(true) { self.toc_tables().into_iter().next() } else { None })
        };
        self.prepare_pages(
            cx,
            &[(o.clone(), old.as_ref().map(|t| (t.begin, t.last())))],
            AllParas::when(old.is_none() && self.has_selection()),
        )?;
        self.begin_step(false, &[]);
        let r: Result<(), CommandError> = (|| {
            if let Some(f) = opts.format {
                self.apply_toc_format(f, opts.levels.unwrap_or(3.0))?;
            }
            if let Some(old) = old {
                let t = TocField { instr: toc_instr(&o), ..old };
                self.rebuild_toc(&t, Some(&leader))?;
                self.refresh_field_cache(cx, None)?;
            } else {
                if self.has_selection() {
                    let p = self.delete_selection()?;
                    self.selection = Selection::caret(p);
                }
                self.insert_toc_field_at_caret(cx, &o, &leader)?;
            }
            Ok(())
        })();
        self.f10.pages.clear();
        match r {
            Ok(()) => {
                self.answer_bool(true);
                self.touch_all();
                self.done_step()
            }
            Err(e) => self.fail_step(e),
        }
    }

    /// `applyTocFormat(f, levels)`: the pPr/rPr of every style `toc N` (the levels shown and the
    /// ones already defined); "From template" goes back to the built-in definition.
    fn apply_toc_format(&mut self, f: TocFormat, levels: f64) -> Result<(), CommandError> {
        for n in 1..=9u32 {
            let name = format!("toc {n}");
            let k = lower(&name);
            let defined = self.style_index().styles.iter().any(|s| lower(&s.name) == k);
            if f64::from(n) > levels && !defined {
                continue;
            }
            if f == TocFormat::Template && !defined {
                continue;
            }
            let id = self.builtin_style_id(&name)?;
            let base = self.styles_base();
            let Some(raw) = style_raw(&base, &id) else { continue };
            let (p_pr, r_pr) = toc_format_style(f, f64::from(n));
            let head = StyleHead {
                p_pr: Ch::Val(format!("<w:pPr>{p_pr}</w:pPr>")),
                r_pr: if r_pr.is_empty() { Ch::Null } else { Ch::Val(format!("<w:rPr>{r_pr}</w:rPr>")) },
                ..StyleHead::default()
            };
            if let Some(x) = put_style(&base, &id, &patch_style_xml(&raw, &head)) {
                self.commit_styles(x);
            }
        }
        Ok(())
    }

    /// `insertTocTemplate(id)`: a gallery model: the content control of the TOC, the heading in the
    /// style TOC Heading and the field `TOC \o "1-3" \h \z \u` (or the entries of the Manual).
    /// One TOC of the gallery per document: the one there is replaced.
    pub(crate) fn insert_toc_template(&mut self, cx: &mut ExecCx<'_>, id: TocTemplateId) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        let Some(tpl) = TOC_TEMPLATES.iter().find(|t| t.id == id) else { return self.done_nothing() };
        if self.paras().is_none() || self.locked() || !self.in_body_ctx() || !self.can_insert_field() {
            return self.done_nothing();
        }
        if !tpl.manual {
            // an old TOC of the gallery is dropped before the sources are read
            let sdt = self.first_toc_sdt();
            let skip = sdt;
            self.prepare_pages(
                cx,
                &[(parse_toc_instr(TOC_AUTO_INSTR), skip)],
                if self.has_selection() {
                    AllParas::Always
                } else if sdt.is_some() {
                    AllParas::IfSources
                } else {
                    AllParas::No
                },
            )?;
        }
        self.begin_step(false, &[]);
        let r = self.insert_toc_template_body(cx, tpl);
        self.f10.pages.clear();
        match r {
            Ok(()) => {
                self.answer_bool(true);
                self.touch_all();
                self.done_step()
            }
            Err(e) => self.fail_step(e),
        }
    }

    fn insert_toc_template_body(&mut self, cx: &mut ExecCx<'_>, tpl: &super::toc::TocTemplate) -> Result<(), CommandError> {
        let at = match self.first_toc_sdt() {
            Some((a, b)) => {
                self.drop_body_paras(a, b);
                a
            }
            None => self.block_insert_point()?,
        };
        let lang = self.ui_lang();
        let key = format!("t{}", uo_jsnum::format::to_js_string(self.new_sdt_id()));
        let pr = toc_sdt_pr(self.new_sdt_id());
        let sdt =
            Arc::new(BlockSdt { key: SdtKey(key.into()), gallery: Some(TOC_GALLERY.into()), pr: Some(pr.into()), ..BlockSdt::default() });
        let hp =
            ParaProps { style_id: Some(self.builtin_style_id("TOC Heading")?), block_sdt: Some(Arc::clone(&sdt)), ..ParaProps::default() };
        let heading = Paragraph {
            runs: vec![Run { text: Utf16::from(tpl.heading.pick(lang)), props: Arc::new(default_run_props()), ..Run::default() }],
            props: Arc::new(hp),
            id: self.doc.ids.para(),
            ..Paragraph::default()
        };
        let body: Vec<Paragraph> = if tpl.manual {
            self.manual_toc_paras(lang, Some(&sdt), &mut |s, name| s.builtin_style_id(name))?
        } else {
            let o = parse_toc_instr(TOC_AUTO_INSTR);
            let sources = self.toc_sources(&o, None)?;
            let look = self.toc_look(&o, None, Some("dot"))?;
            let (entries, empty_msg) = self.toc_entries(&o, &sources, &look)?;
            let bp_props = ParaProps { block_sdt: Some(Arc::clone(&sdt)), ..ParaProps::default() };
            let bp = Paragraph { props: Arc::new(bp_props), id: self.doc.ids.para(), ..Paragraph::default() };
            let begin = Self::field_begin_run(TOC_AUTO_INSTR);
            let end = Self::field_end_run();
            self.toc_paragraphs(bp.clone(), &bp, Vec::new(), begin, entries, vec![end], true, empty_msg)
        };
        let mut built = vec![heading];
        built.extend(body);
        // the body does not end in the control: a paragraph after it (as Word)
        if at >= self.doc.body.paras.len() {
            built.push(Paragraph { id: self.doc.ids.para(), ..Paragraph::default() });
        }
        let n = built.len();
        self.splice_body(at, 0, built);
        self.shift_anchors(u32::try_from(at).unwrap_or(u32::MAX), i64::try_from(n).unwrap_or(0));
        // the caret goes to the paragraph after the TOC
        let len = self.doc.body.paras.len();
        let next = (at + n).min(len.saturating_sub(1));
        let off = if next == at + n { 0 } else { self.doc.body.paras.get(next).map_or(0, para_length) };
        self.selection =
            Selection::caret(DocPosition::new(u32::try_from(next).unwrap_or(u32::MAX), u32::try_from(off).unwrap_or(u32::MAX)));
        self.refresh_field_cache(cx, None)?;
        Ok(())
    }

    /// The model half of `paintTocTemplate(id, canvas, width)`: the preview mini-document (the
    /// heading and three entries, with the styles of the document; the missing ones by the
    /// built-in definition WITHOUT writing them to the document).
    #[must_use]
    pub fn toc_template_preview(&mut self, id: TocTemplateId) -> Option<TocPreview> {
        let tpl = TOC_TEMPLATES.iter().find(|t| t.id == id)?;
        let lang = self.ui_lang();
        let mut xml = self.styles_base();
        let mut style_of = |name: &str| -> String {
            let b = super::super::styles::sheet::builtin_style(name);
            let pt = b.map(|b| super::super::styles::xform::id_of(b.xml)).filter(|s| !s.is_empty()).unwrap_or_else(|| name.to_owned());
            let sid = super::super::styles::xform::resolve_style_id(Some(&xml), &pt);
            xml = super::super::styles::xform::ensure_defined(&xml, &sid, true);
            sid
        };
        let mut sp = (*self.sections.body_sect_pr).clone();
        obj::delete(&mut sp, "pgBorders");
        obj::delete(&mut sp, "cols");
        // a narrow column (~8.5 cm): the text of the preview stays legible in the gallery
        let width = 4800.0;
        let (ml, mr) = (sp.margin_left_twips.to_number(), sp.margin_right_twips.to_number());
        obj::set(&mut sp, "pageWidthTwips", |x| x.page_width_twips = JsNumber::Num(ml + mr + width));
        let hp = ParaProps { style_id: Some(style_of("TOC Heading").into()), ..ParaProps::default() };
        let mut ids = IdAlloc::new();
        let mut paras: Vec<Paragraph> = vec![Paragraph {
            runs: vec![Run { text: Utf16::from(tpl.heading.pick(lang)), props: Arc::new(default_run_props()), ..Run::default() }],
            props: Arc::new(hp),
            id: ids.para(),
            ..Paragraph::default()
        }];
        if tpl.manual {
            for (i, &level) in MANUAL_LEVELS.iter().take(3).enumerate() {
                let bold = level == 1.0;
                let mk = |t: &str| {
                    let mut p = default_run_props();
                    if bold {
                        obj::set(&mut p, "bold", |x| x.bold = Some(true));
                    }
                    Run { text: Utf16::from(t), props: Arc::new(p), ..Run::default() }
                };
                let mut tab = Run { text: Utf16::from("\t"), props: Arc::new(default_run_props()), ..Run::default() };
                tab.ext_mut().ptab =
                    Some(Ptab { alignment: "right".into(), relative_to: "margin".into(), leader: Some("dot".into()), ..Ptab::default() });
                let mut props = ParaProps {
                    style_id: Some(style_of(&format!("toc {}", uo_jsnum::format::to_js_string(level))).into()),
                    ..ParaProps::default()
                };
                if level == 2.0 {
                    props.indent_left_twips = Some(JsNumber::Num(216.0));
                } else if level == 3.0 {
                    props.indent_left_twips = Some(JsNumber::Num(446.0));
                }
                paras.push(Paragraph {
                    runs: vec![mk(&manual_placeholder(lang, level)), tab, mk(&uo_jsnum::format::to_js_string((i + 1) as f64))],
                    props: Arc::new(props),
                    id: ids.para(),
                    ..Paragraph::default()
                });
            }
        } else {
            for level in [1.0_f64, 2.0, 3.0] {
                let mk = |t: &str| Run { text: Utf16::from(t), props: Arc::new(default_run_props()), ..Run::default() };
                let head = if lang == UiLang::En {
                    format!("Heading {}", uo_jsnum::format::to_js_string(level))
                } else {
                    format!("T\u{ed}tulo {}", uo_jsnum::format::to_js_string(level))
                };
                let props = ParaProps {
                    style_id: Some(style_of(&format!("toc {}", uo_jsnum::format::to_js_string(level))).into()),
                    tab_stops: Some(vec![crate::model::para::ParaTabStop {
                        pos_twips: JsNumber::Num(width - 10.0),
                        align: "right".into(),
                        leader: Some("dot".into()),
                        ..crate::model::para::ParaTabStop::default()
                    }]),
                    ..ParaProps::default()
                };
                paras.push(Paragraph {
                    runs: vec![mk(&head), mk("\t"), mk("1")],
                    props: Arc::new(props),
                    id: ids.para(),
                    ..Paragraph::default()
                });
            }
        }
        let n = paras.len();
        let mut d = self.doc.clone();
        let mut body = Story::new();
        for p in paras {
            body.push_paragraph(p);
        }
        d.body = body;
        d.hf = crate::pvec::PVec::new();
        d.notes = Arc::new(crate::model::document::Notes::default());
        d.comments = Arc::from(Vec::new());
        d.bookmarks = Arc::new(crate::model::document::Bookmarks::default());
        let sect = crate::model::section::Section {
            sect_pr: Arc::new(sp),
            block_start: JsNumber::Num(0.0),
            block_end: JsNumber::Num(n as f64 - 1.0),
            ..crate::model::section::Section::default()
        };
        d.sections = Arc::from(vec![sect]);
        let styles = crate::read::parts::read_style_sheet_text(&xml).ok()?;
        let mut fmt = (*d.formatting).clone();
        fmt.styles = styles;
        d.formatting = Arc::new(fmt);
        d.ids = ids;
        Some(TocPreview { document: d, width_twips: width, crop_left_twips: uo_jsnum::math::max(0.0, ml - 240.0), margin_right_twips: mr })
    }

    // ── remove, add text, mark ─────────────────────────────────────────────────────────────

    /// `removeTableOfContents()`.
    pub(crate) fn remove_table_of_contents(&mut self) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none() || self.locked() || !self.in_body_ctx() {
            return self.done_nothing();
        }
        let t = self.toc_at_caret();
        let mut range = self.toc_sdt_range(self.selection.focus.para as usize);
        let mut field = if range.is_some() { None } else { t };
        if range.is_none() && field.is_none() {
            let s = self.first_toc_sdt();
            let f = self.toc_tables().into_iter().next();
            match (s, f) {
                (Some(s), f) if f.as_ref().is_none_or(|f| s.0 <= f.begin) => range = Some(s),
                (_, f) => field = f,
            }
        }
        if range.is_none() && field.is_none() {
            return self.done_nothing();
        }
        self.begin_step(false, &[]);
        let at: usize;
        if let Some((a, b)) = range {
            at = a;
            self.drop_body_paras(a, b);
        } else if let Some(f) = field {
            let last = f.last();
            let (Some(bp), Some(ep)) = (self.doc.body.paras.get(f.begin).cloned(), self.doc.body.paras.get(last).cloned()) else {
                self.discard_step();
                return Err(CommandError::Position);
            };
            let bi = self.field_run_index(f.begin, f.begin_run, &f.instr, false);
            let ei = f.end_run.and_then(|k| self.field_run_index(last, k, "", true));
            let lead: Vec<Run> = match bi {
                Some(b) if b > 0 => bp.runs.iter().take(b).cloned().collect(),
                _ => Vec::new(),
            };
            let tail: Vec<Run> = match ei {
                Some(e) => ep.runs.iter().skip(e + 1).cloned().collect(),
                None => Vec::new(),
            };
            self.drop_old_toc_marks((f.begin, last));
            let mut keep: Vec<Paragraph> = Vec::new();
            let has_lead = lead.iter().any(|r| {
                !r.text.is_empty()
                    || r.ext()
                        .is_some_and(|e| e.image.is_some() || e.field.as_deref().is_some_and(|x| !x.is_empty()) || e.bookmark.is_some())
            });
            if f.begin == last {
                let mut p = bp.clone();
                p.id = self.doc.ids.para();
                p.runs = lead.into_iter().chain(tail).collect();
                keep.push(p);
            } else {
                if has_lead {
                    let mut p = bp.clone();
                    p.id = self.doc.ids.para();
                    p.runs = lead;
                    keep.push(p);
                }
                if !tail.is_empty() || ep.props.section_break.is_some() {
                    let mut p = ep.clone();
                    p.id = self.doc.ids.para();
                    p.runs = tail;
                    keep.push(p);
                }
            }
            let removed = last - f.begin + 1;
            let n = keep.len();
            self.splice_body(f.begin, removed, keep);
            self.shift_anchors(
                u32::try_from(f.begin).unwrap_or(u32::MAX),
                i64::try_from(n).unwrap_or(0) - i64::try_from(removed).unwrap_or(0),
            );
            at = f.begin + usize::from(n > 1);
        } else {
            self.discard_step();
            return self.done_nothing();
        }
        if self.doc.body.paras.is_empty() {
            let id = self.doc.ids.para();
            self.doc.body.paras.push(Paragraph::empty(id));
        }
        let pi = at.min(self.doc.body.paras.len() - 1);
        self.selection = Selection::caret(DocPosition::new(u32::try_from(pi).unwrap_or(u32::MAX), 0));
        self.answer_bool(true);
        self.touch_all();
        self.done_step()
    }

    /// `addTextKind(p)`: the paragraph's style for Add Text: Normal, a built-in heading, other.
    fn add_text_kind(&mut self, p: &Paragraph) -> AddTextKind {
        let id: Option<String> =
            p.props.style_id.as_deref().or_else(|| p.ppr.as_ref().and_then(|x| x.p_style.as_deref())).map(str::to_owned);
        let defp = self.default_paragraph();
        let Some(id) = id.filter(|i| !i.is_empty() && *i != defp) else { return AddTextKind::Normal };
        let idx = self.style_index();
        let name = idx.styles.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
        if rx::is_heading_name(&name) { AddTextKind::Heading } else { AddTextKind::Other }
    }

    /// `setTocLevel(level)`: Add Text (0 = Do not show in table of contents; 1–9 = Level N) on the
    /// paragraphs of the selection.
    pub(crate) fn set_toc_level(&mut self, level: f64) -> Result<Outcome, CommandError> {
        if self.paras().is_none()
            || self.locked()
            || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. })
        {
            return self.done_nothing();
        }
        let lv = uo_jsnum::math::max(0.0, uo_jsnum::math::min(9.0, uo_jsnum::math::round(level)));
        self.begin_step(false, &[]);
        let DocRange { start, end } = normalize_range(self.range());
        let keep = self.selection;
        let now: HashMap<ParaId, f64> = {
            let view = crate::view::DocView::new(&self.doc);
            crate::word::outline::outline_paragraphs(view, None).into_iter().map(|h| (h.paragraph.id, h.level)).collect()
        };
        for pi in start.para..=end.para {
            let Some(p) = self.paras().and_then(|ps| ps.get(pi as usize)).cloned() else { continue };
            let kind = self.add_text_kind(&p);
            match add_text_plan(lv, kind) {
                AddTextPlan::Heading(h) if h != 0.0 && !h.is_nan() => {
                    self.selection = Selection::caret(DocPosition::new(pi, 0));
                    let id = match self.builtin_style_id(&format!("heading {}", uo_jsnum::format::to_js_string(h))) {
                        Ok(i) => i,
                        Err(e) => {
                            self.selection = keep;
                            self.discard_step();
                            return Err(e);
                        }
                    };
                    if let Err(e) = self.apply_style_core(Some(&id)) {
                        self.selection = keep;
                        self.discard_step();
                        return Err(e);
                    }
                }
                AddTextPlan::Heading(_) => {}
                AddTextPlan::Normal => {
                    self.selection = Selection::caret(DocPosition::new(pi, 0));
                    if let Err(e) = self.apply_style_core(None) {
                        self.selection = keep;
                        self.discard_step();
                        return Err(e);
                    }
                }
                plan @ (AddTextPlan::OutlineNone | AddTextPlan::Outline(_)) => {
                    // outside the headings: only the direct outline level (9 = body text)
                    if plan == AddTextPlan::OutlineNone && now.get(&p.id).copied().unwrap_or(0.0) == 0.0 {
                        continue;
                    }
                    let v = match plan {
                        AddTextPlan::Outline(l) => l - 1.0,
                        _ => 9.0,
                    };
                    if let Some(q) = self.paras_mut().and_then(|ps| ps.make_mut(pi as usize)) {
                        let mut props = (*q.props).clone();
                        obj::set(&mut props, "outlineLvl", |x| x.outline_lvl = Some(JsNumber::Num(v)));
                        q.props = Arc::new(props);
                        let mut ppr = q.ppr.as_deref().cloned().unwrap_or_default();
                        obj::set(&mut ppr, "outlineLvl", |x| x.outline_lvl = Some(JsNumber::Num(v)));
                        q.ppr = Some(Arc::new(ppr));
                        crate::js::JsObject::mark(q, "ppr");
                    }
                    self.touch_para(pi as usize);
                }
            }
        }
        self.selection = keep;
        self.pending_props = None;
        self.done_step()
    }

    /// `markTocEntry({ text, identifier, level })`: the TC field at the end of the selection.
    pub(crate) fn mark_toc_entry(&mut self, text: &str, identifier: Option<&str>, level: Option<f64>) -> Result<Outcome, CommandError> {
        self.answer_bool(false);
        if self.paras().is_none()
            || self.locked()
            || uo_jsnum::text::trim(text).is_empty()
            || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. })
            || !self.can_insert_field()
        {
            return self.done_nothing();
        }
        self.begin_step(false, &[]);
        let DocRange { end, .. } = normalize_range(self.range());
        let carried: CarriedProps = self.pending_props.clone().unwrap_or_else(|| self.current_props());
        let base: RunProps = self.new_run_props_from(&carried.props);
        let mut run = Run { props: Arc::new(base), ..Run::default() };
        run.rpr_link = carried.link.clone();
        let instr = tc_instr(text, identifier, level);
        obj::set(run.ext_mut(), "field", |e| e.field = Some(instr.as_str().into()));
        let Some(paras) = self.paras_mut() else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        match insert_run(paras, end, run) {
            Ok(pos) => {
                self.touch_para(end.para as usize);
                self.selection = Selection::caret(pos);
                self.answer_bool(true);
                self.done_step()
            }
            Err(e) => {
                self.discard_step();
                Err(e.into())
            }
        }
    }
}
