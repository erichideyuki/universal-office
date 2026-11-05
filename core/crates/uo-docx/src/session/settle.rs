//! The settle step (G21, U1) and the install normalisations: the model mutations the TS makes
//! outside the edit operations, by name.
//!
//! | TS | here | when |
//! |---|---|---|
//! | `syncRunRPr(paras)` (universal-write.ts 16068) | [`sync_run_rpr`] | install (body, cells, header/footer slots, notes), every settle (the context's paragraphs), every undo/redo (also the body tables, `syncTableRPr`) |
//! | `syncSections()` (11004) | [`SectionState::sync`] | install, every settle in the body context, save |
//! | `markNoteRuns` (editor notes.ts 132) | [`mark_note_runs`] | install |
//! | the DATE/PAGE text of empty field runs (installFullDoc) | [`field_placeholders`] | install |
//! | `markSectionBreaks` (fulldoc.ts) | `write::roundtrip::mark_section_breaks` | install |
//!
//! The layout-dependent mutations (field results, hyphenation) are the `LayoutOracle`'s (A4); the
//! TS fallback "layout failed → legacy layout + `exitCell()`" does not exist here (G21).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;

use crate::ids::HfPartId;
use crate::js::{JsNumber, JsRecord, Str};
use crate::model::Document;
use crate::model::para::{HfRefEdit, Paragraph, SectionBreak};
use crate::model::run::{RprLink, Run};
use crate::model::section::{HfRef, SectPr, Section, SectionProps};
use crate::model::story::Story;
use crate::pvec::PVec;
use crate::text::Utf16;
use crate::word::obj;

/// `syncRunRPr` for one run: the props keep a link to the original `w:rPr`, and a run that an edit
/// rebuilt without it gets it back. Whether the run changed (the TS writes the symbol keys every
/// time; only a change of the link or of the run counts here).
fn sync_run(r: &mut Run) -> bool {
    let mut changed = false;
    if let Some(rpr) = &r.rpr {
        let edited_link = r.rpr_link.as_deref().is_some_and(|l| l.edited);
        let same = r.rpr_link.as_deref().is_some_and(|l| l.rpr.as_ref().is_some_and(|x| Arc::ptr_eq(x, rpr)));
        if !same {
            let edited = edited_link;
            r.rpr_link = Some(Box::new(RprLink { rpr: Some(Arc::clone(rpr)), edited }));
            changed = true;
        }
        if r.rpr_edited == Some(true) {
            if let Some(l) = r.rpr_link.as_deref_mut()
                && !l.edited
            {
                l.edited = true;
                changed = true;
            }
        } else if edited_link {
            obj::set(r, "rprEdited", |x| x.rpr_edited = Some(true));
            changed = true;
        }
    } else if let Some(l) = r.rpr_link.as_deref()
        && let Some(rpr) = l.rpr.clone()
    {
        let edited = l.edited;
        obj::set(r, "rpr", |x| x.rpr = Some(rpr));
        if edited {
            obj::set(r, "rprEdited", |x| x.rpr_edited = Some(true));
        }
        changed = true;
    }
    changed
}

fn run_needs_sync(r: &Run) -> bool {
    let own = match (&r.rpr, r.rpr_link.as_deref()) {
        (Some(_), None) => true,
        (Some(rpr), Some(l)) => {
            let edited = r.rpr_edited == Some(true);
            !l.rpr.as_ref().is_some_and(|x| Arc::ptr_eq(x, rpr)) || edited != l.edited
        }
        (None, Some(l)) => l.rpr.is_some(),
        (None, None) => false,
    };
    own || r.ext().and_then(|e| e.textbox.as_deref()).is_some_and(|tb| story_needs_sync(&tb.content, true))
}

fn para_needs_sync(p: &Paragraph) -> bool {
    p.runs.iter().any(run_needs_sync)
}

fn story_needs_sync(s: &Story, cells: bool) -> bool {
    s.paras.iter().any(para_needs_sync)
        || (cells && s.tables.iter().any(|a| a.table.rows.iter().any(|r| r.cells.iter().any(|c| story_needs_sync(&c.content, true)))))
}

/// `syncRunRPr(paras)` over one paragraph, into its textboxes (the TS `r.textbox.paras`: the box's
/// paragraphs with those of its cells).
pub fn sync_paragraph(p: &mut Paragraph) {
    for r in &mut p.runs {
        sync_run(r);
        if let Some(tb) = r.ext.as_deref_mut().and_then(|e| e.textbox.as_deref_mut())
            && story_needs_sync(&tb.content, true)
        {
            sync_story(&mut tb.content, true);
        }
    }
}

/// `syncRunRPr` over the paragraphs of a story (copy-on-write only where a run changes); with
/// `cells`, also every cell of its tables (`syncTableRPr`, the flat views of headers and textboxes).
pub fn sync_story(s: &mut Story, cells: bool) {
    sync_paras(&mut s.paras);
    if !cells {
        return;
    }
    for i in 0..s.tables.len() {
        let needs =
            s.tables.get(i).is_some_and(|a| a.table.rows.iter().any(|r| r.cells.iter().any(|c| story_needs_sync(&c.content, true))));
        if needs && let Some(a) = s.tables.make_mut(i) {
            let t = Arc::make_mut(&mut a.table);
            for row in &mut t.rows {
                for cell in &mut row.cells {
                    sync_story(&mut cell.content, true);
                }
            }
        }
    }
}

/// `syncRunRPr(paras)` over a paragraph list.
pub fn sync_paras(paras: &mut PVec<Paragraph>) {
    for i in 0..paras.len() {
        if paras.get(i).is_some_and(para_needs_sync)
            && let Some(p) = paras.make_mut(i)
        {
            sync_paragraph(p);
        }
    }
}

/// `NOTE_REF_TEXT` (docx-core notes.ts): the one character a note reference takes in the editor.
pub const NOTE_REF_TEXT: u16 = 0xFFFC;

/// `/^\s*(FOOTNOTE|ENDNOTE)REF_MARK\s*$/` (`isNoteMark`): JS white space trimmed.
#[must_use]
pub fn is_note_mark_field(field: &str) -> bool {
    let t = uo_jsnum::text::trim(field);
    t == "FOOTNOTEREF_MARK" || t == "ENDNOTEREF_MARK"
}

fn note_run_text(r: &Run) -> Option<Utf16> {
    let e = r.ext()?;
    let one = Utf16::from_units(vec![NOTE_REF_TEXT]);
    if let Some(n) = &e.note_ref {
        if n.custom != Some(true) {
            return (r.text != one).then_some(one);
        }
        return r.text.is_empty().then_some(one);
    }
    let f = e.field.as_deref().filter(|f| !f.is_empty())?;
    (r.text.is_empty() && is_note_mark_field(f)).then_some(one)
}

/// `markNoteRuns(paras)`: a note reference and the note's own mark take ONE character (`U+FFFC`);
/// into textboxes. `cells`: also the tables (`markNoteRunsInTable`, `flattenBlocks`).
pub fn mark_note_runs(s: &mut Story, cells: bool) {
    fn needs(s: &Story, cells: bool) -> bool {
        s.paras.iter().any(|p| {
            p.runs
                .iter()
                .any(|r| note_run_text(r).is_some() || r.ext().and_then(|e| e.textbox.as_deref()).is_some_and(|t| needs(&t.content, true)))
        }) || (cells && s.tables.iter().any(|a| a.table.rows.iter().any(|r| r.cells.iter().any(|c| needs(&c.content, true)))))
    }
    for i in 0..s.paras.len() {
        let n = s.paras.get(i).is_some_and(|p| {
            p.runs
                .iter()
                .any(|r| note_run_text(r).is_some() || r.ext().and_then(|e| e.textbox.as_deref()).is_some_and(|t| needs(&t.content, true)))
        });
        if n && let Some(p) = s.paras.make_mut(i) {
            for r in &mut p.runs {
                if let Some(t) = note_run_text(r) {
                    r.text = t;
                }
                if let Some(tb) = r.ext.as_deref_mut().and_then(|e| e.textbox.as_deref_mut()) {
                    mark_note_runs(&mut tb.content, true);
                }
            }
        }
    }
    if cells {
        for i in 0..s.tables.len() {
            let n = s.tables.get(i).is_some_and(|a| a.table.rows.iter().any(|r| r.cells.iter().any(|c| needs(&c.content, true))));
            if n && let Some(a) = s.tables.make_mut(i) {
                for row in &mut Arc::make_mut(&mut a.table).rows {
                    for cell in &mut row.cells {
                        mark_note_runs(&mut cell.content, true);
                    }
                }
            }
        }
    }
}

/// The DATE/PAGE placeholders of `installFullDoc`: an empty field run of the top-level body
/// paragraphs shows today's date (`toLocaleDateString('pt-BR')`) for `/DATE/i`, `1` for `/PAGE/i`.
pub fn field_placeholders(paras: &mut PVec<Paragraph>, today: &str) {
    let contains_ci = |s: &str, w: &str| s.to_ascii_uppercase().contains(w);
    for i in 0..paras.len() {
        let hit = paras.get(i).is_some_and(|p| {
            p.runs.iter().any(|r| {
                r.text.is_empty()
                    && r.ext()
                        .and_then(|e| e.field.as_deref())
                        .is_some_and(|f| !f.is_empty() && (contains_ci(f, "DATE") || contains_ci(f, "PAGE")))
            })
        });
        if hit && let Some(p) = paras.make_mut(i) {
            for r in &mut p.runs {
                let Some(f) = r.ext().and_then(|e| e.field.as_deref()).filter(|f| !f.is_empty() && r.text.is_empty()) else { continue };
                if contains_ci(f, "DATE") {
                    r.text = Utf16::from(today);
                } else if contains_ci(f, "PAGE") {
                    r.text = Utf16::from("1");
                }
            }
        }
    }
}

/// The header/footer references a section keeps in the model (`SectionRefs`: the body's
/// `bodyRefs`, or a section break's).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SectionRefs {
    pub header_refs: Vec<HfRefEdit>,
    pub footer_refs: Vec<HfRefEdit>,
}

impl SectionRefs {
    /// `refsOfSection(s)`.
    #[must_use]
    pub fn of_section(s: &Section) -> SectionRefs {
        let edit =
            |kind: &str, r: &HfRef| HfRefEdit { kind: kind.into(), r#type: r.r#type.clone(), rid: r.rid.clone(), ..HfRefEdit::default() };
        SectionRefs {
            header_refs: s.header_refs.iter().map(|r| edit("header", r)).collect(),
            footer_refs: s.footer_refs.iter().map(|r| edit("footer", r)).collect(),
        }
    }
}

/// The session's section state (`resetSections`: `sectionProps`, `nextSectionId`, `hfParts`,
/// `lastSectionIds`, `lastSectionRefs`; and the body's `sectPr`/`bodyRefs` of the TS model).
#[derive(Debug, Clone, Default)]
pub struct SectionState {
    pub next_section_id: u32,
    /// section id → the `SectionProps` read for it (id 0: the last section)
    pub section_props: HashMap<u32, Option<Arc<SectionProps>>>,
    /// `kind:rId` → the part
    pub hf_parts: IndexMap<Str, HfPartId>,
    pub last_ids: Vec<u32>,
    pub last_refs: HashMap<u32, SectionRefs>,
    /// the body's `w:sectPr` (the last section's)
    pub body_sect_pr: Arc<SectPr>,
    /// the last section's references (`bodyRefs`; `None` until a section needs one)
    pub body_refs: Option<SectionRefs>,
    /// the rIds referenced at install (a part no section refers to any more leaves the package)
    pub hf_rids_at_load: HashSet<Str>,
}

/// `{ pageWidthTwips: 11906, … }`: the body `sectPr` of a document without sections (A4, margins
/// 3/2.5 cm).
fn default_sect_pr() -> SectPr {
    SectPr {
        page_width_twips: JsNumber::Num(11906.0),
        page_height_twips: JsNumber::Num(16838.0),
        margin_left_twips: JsNumber::Num(1701.0),
        margin_right_twips: JsNumber::Num(1701.0),
        margin_top_twips: JsNumber::Num(1417.0),
        margin_bottom_twips: JsNumber::Num(1417.0),
        ..SectPr::default()
    }
}

fn break_id(sb: &SectionBreak) -> Option<u32> {
    match sb.id {
        Some(JsNumber::Num(x)) if x >= 0.0 && x.fract() == 0.0 && x <= f64::from(u32::MAX) => Some(uo_jsnum::conv::to_uint32(x)),
        _ => None,
    }
}

impl SectionState {
    /// `resetSections` + the section part of `installFullDoc` (the section breaks already marked):
    /// part index, the rIds at load, ids for the breaks, the body's `sectPr` and references.
    pub fn install(doc: &mut Document) -> SectionState {
        let secs = Arc::clone(&doc.sections);
        let mut st = SectionState { next_section_id: 1, ..SectionState::default() };
        st.section_props.insert(0, None);
        for s in secs.iter() {
            for (kind, refs) in [("header", &s.header_refs), ("footer", &s.footer_refs)] {
                for r in refs {
                    let key: Str = format!("{kind}:{}", r.rid).into();
                    st.hf_parts.entry(key).or_insert(r.part);
                    st.hf_rids_at_load.insert(r.rid.clone());
                }
            }
        }
        for i in 0..doc.body.paras.len() {
            let Some(sb) = doc.body.paras.get(i).and_then(|p| p.props.section_break.as_deref()) else { continue };
            let sect = secs.iter().find(|s| Arc::ptr_eq(&s.sect_pr, &sb.sect_pr)).and_then(|s| s.sect.clone());
            let id = st.next_section_id;
            st.next_section_id = st.next_section_id.saturating_add(1);
            st.section_props.insert(id, sect);
            if let Some(p) = doc.body.paras.make_mut(i) {
                let props = Arc::make_mut(&mut p.props);
                if let Some(sb) = props.section_break.as_deref_mut() {
                    obj::set(sb, "id", |b| b.id = Some(JsNumber::Num(f64::from(id))));
                }
            }
        }
        let last = secs.last();
        st.section_props.insert(0, last.and_then(|s| s.sect.clone()));
        st.body_sect_pr = last.map_or_else(|| Arc::new(default_sect_pr()), |s| Arc::clone(&s.sect_pr));
        st.body_refs = last.map(SectionRefs::of_section);
        st
    }

    /// `syncSections()`: the sections again from the body (the single source): each paragraph with
    /// a section break closes one; the last is the body's `sectPr`. A break that disappeared in an
    /// edit passes its header/footer references to the section that now covers its text, in the
    /// types that one does not define (Word keeps the header). Rebuilds `doc.sections` (block
    /// ranges, references, slots) for the layout and the save.
    pub fn sync(&mut self, doc: &mut Document) {
        let mut breaks: Vec<(usize, u32)> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for pi in 0..doc.body.paras.len() {
            let Some(sb) = doc.body.paras.get(pi).and_then(|p| p.props.section_break.as_deref()) else { continue };
            let known = break_id(sb).filter(|id| self.section_props.contains_key(id) && !seen.contains(id));
            let id = match known {
                Some(id) => id,
                None => {
                    // a break without a record (or a repeated id): the props of the next section, as Word
                    let id = self.next_section_id;
                    self.next_section_id = self.next_section_id.saturating_add(1);
                    let props0 = self.section_props.get(&0).cloned().flatten();
                    self.section_props.insert(id, props0);
                    if let Some(p) = doc.body.paras.make_mut(pi) {
                        let mut props = (*p.props).clone();
                        if let Some(sb) = props.section_break.as_deref() {
                            let mut sb = sb.clone();
                            obj::set(&mut sb, "id", |b| b.id = Some(JsNumber::Num(f64::from(id))));
                            obj::set(&mut props, "sectionBreak", |x| x.section_break = Some(Box::new(sb)));
                        }
                        p.props = Arc::new(props);
                    }
                    id
                }
            };
            seen.insert(id);
            breaks.push((pi, id));
        }
        let mut ids: Vec<u32> = breaks.iter().map(|b| b.1).collect();
        ids.push(0);
        // nothing moved (the common case: an edit inside paragraphs): the sections the layout reads
        // are already these (D2a-S2: the TS rebuilds them at every layout, with the same result)
        if ids == self.last_ids {
            match self.sections_match(doc, &breaks) {
                Match::Same => return,
                // only paragraphs moved: the same sections with their new block ranges (what
                // `effectiveSection` gives: the rest depends only on the id, `sectPr` and refs)
                Match::Moved(ranges) => {
                    let out: Vec<Section> = doc
                        .sections
                        .iter()
                        .zip(ranges)
                        .map(|(s, (bs, be))| Section { block_start: JsNumber::Num(bs), block_end: JsNumber::Num(be), ..s.clone() })
                        .collect();
                    doc.sections = out.into();
                    return;
                }
                Match::Differ => {}
            }
        }
        let prev = std::mem::take(&mut self.last_ids);
        for (k, x) in prev.iter().enumerate() {
            if ids.contains(x) {
                continue;
            }
            let Some(gone) = self.last_refs.get(x).cloned() else { continue };
            if gone.header_refs.is_empty() && gone.footer_refs.is_empty() {
                continue;
            }
            let y = prev.iter().skip(k + 1).find(|id| ids.contains(id)).copied().unwrap_or(0);
            let add = |h: &mut SectionRefs| {
                for r in &gone.header_refs {
                    if !h.header_refs.iter().any(|t| t.r#type == r.r#type) {
                        h.header_refs.push(r.clone());
                    }
                }
                for r in &gone.footer_refs {
                    if !h.footer_refs.iter().any(|t| t.r#type == r.r#type) {
                        h.footer_refs.push(r.clone());
                    }
                }
            };
            if y == 0 {
                add(self.body_refs.get_or_insert_with(SectionRefs::default));
            } else if let Some(&(pi, _)) = breaks.iter().find(|b| b.1 == y)
                && let Some(p) = doc.body.paras.make_mut(pi)
            {
                let props = Arc::make_mut(&mut p.props);
                if let Some(sb) = props.section_break.as_deref_mut() {
                    let mut h =
                        SectionRefs { header_refs: std::mem::take(&mut sb.header_refs), footer_refs: std::mem::take(&mut sb.footer_refs) };
                    add(&mut h);
                    sb.header_refs = h.header_refs;
                    sb.footer_refs = h.footer_refs;
                }
            }
        }
        // the effective sections, with their block ranges in the body
        let mut out: Vec<Section> = Vec::with_capacity(breaks.len() + 1);
        let mut start_para = 0usize;
        for &(pi, id) in &breaks {
            let Some(sb) = doc.body.paras.get(pi).and_then(|p| p.props.section_break.as_deref()) else { continue };
            let refs = SectionRefs { header_refs: sb.header_refs.clone(), footer_refs: sb.footer_refs.clone() };
            out.push(self.effective(doc, id, &sb.sect_pr, Some(&refs), start_para, Some(pi)));
            start_para = pi + 1;
        }
        let body_refs = self.body_refs.clone();
        let sp = Arc::clone(&self.body_sect_pr);
        out.push(self.effective(doc, 0, &sp, body_refs.as_ref(), start_para, None));
        doc.sections = out.into();
        self.last_ids = ids.clone();
        self.last_refs = ids
            .iter()
            .map(|&id| {
                let r = if id == 0 {
                    self.body_refs.clone().unwrap_or_default()
                } else {
                    breaks
                        .iter()
                        .find(|b| b.1 == id)
                        .and_then(|b| doc.body.paras.get(b.0))
                        .and_then(|p| p.props.section_break.as_deref())
                        .map(|sb| SectionRefs { header_refs: sb.header_refs.clone(), footer_refs: sb.footer_refs.clone() })
                        .unwrap_or_default()
                };
                (id, r)
            })
            .collect();
    }

    /// Whether `doc.sections` already are the sections of these breaks: the same `sectPr`
    /// objects and header/footer references, and the same block ranges (or only these moved).
    fn sections_match(&self, doc: &Document, breaks: &[(usize, u32)]) -> Match {
        if doc.sections.len() != breaks.len() + 1 {
            return Match::Differ;
        }
        let empty: &[HfRefEdit] = &[];
        let mut start_para = 0usize;
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        let mut moved = false;
        for (k, sec) in doc.sections.iter().enumerate() {
            let (sp, hdr, ftr, end_para): (&Arc<SectPr>, &[HfRefEdit], &[HfRefEdit], Option<usize>) = match breaks.get(k) {
                Some(&(pi, _)) => {
                    let Some(sb) = doc.body.paras.get(pi).and_then(|p| p.props.section_break.as_deref()) else { return Match::Differ };
                    (&sb.sect_pr, &sb.header_refs, &sb.footer_refs, Some(pi))
                }
                None => match &self.body_refs {
                    Some(r) => (&self.body_sect_pr, &r.header_refs, &r.footer_refs, None),
                    None => (&self.body_sect_pr, empty, empty, None),
                },
            };
            let (bs, be) = block_range(&doc.body, start_para, end_para);
            if !Arc::ptr_eq(&sec.sect_pr, sp) {
                return Match::Differ;
            }
            if !(self.refs_kept(&sec.header_refs, hdr, "header") && self.refs_kept(&sec.footer_refs, ftr, "footer")) {
                return Match::Differ;
            }
            if !moved && (sec.block_start != JsNumber::Num(bs) || sec.block_end != JsNumber::Num(be)) {
                moved = true;
                ranges.reserve_exact(doc.sections.len());
                ranges.extend(doc.sections.iter().take(k).map(|s| (s.block_start.to_number(), s.block_end.to_number())));
            }
            if moved {
                ranges.push((bs, be));
            }
            if let Some(pi) = end_para {
                start_para = pi + 1;
            }
        }
        if moved { Match::Moved(ranges) } else { Match::Same }
    }

    /// The references `effective` keeps of `edits` (those whose part is known) are `refs`.
    fn refs_kept(&self, refs: &[HfRef], edits: &[HfRefEdit], kind: &str) -> bool {
        let mut it = refs.iter();
        for e in edits {
            let known = self.hf_parts.iter().any(|(k, _)| k.strip_prefix(kind).and_then(|r| r.strip_prefix(':')) == Some(&*e.rid));
            if !known {
                continue;
            }
            match it.next() {
                Some(r) if r.r#type == e.r#type && r.rid == e.rid => {}
                _ => return false,
            }
        }
        it.next().is_none()
    }

    /// `effectiveSection(id, sp, refs)`: the section as the layout and the save use it. D2a keeps
    /// the read `SectionProps` with the model's `type`; the rest of the TS function (line
    /// numbers, vertical alignment and columns refreshed from the model's `sectPr`) is the
    /// sections family's (D2b-F4).
    fn effective(
        &self,
        doc: &Document,
        id: u32,
        sp: &Arc<SectPr>,
        refs: Option<&SectionRefs>,
        start_para: usize,
        end_para: Option<usize>,
    ) -> Section {
        let props = self.section_props.get(&id).cloned().flatten();
        let mut sect = props.map_or_else(|| SectionProps { r#type: "nextPage".into(), ..SectionProps::default() }, |p| (*p).clone());
        sect.r#type = sp.r#type.clone().unwrap_or_else(|| "nextPage".into());
        let (block_start, block_end) = block_range(&doc.body, start_para, end_para);
        let mut sec = Section {
            sect_pr: Arc::clone(sp),
            sect: Some(Arc::new(sect)),
            block_start: JsNumber::Num(block_start),
            block_end: JsNumber::Num(block_end),
            header_refs: Vec::new(),
            footer_refs: Vec::new(),
            headers: JsRecord::new(),
            footers: JsRecord::new(),
            ..Section::default()
        };
        if let Some(refs) = refs {
            for (kind, list) in [("header", &refs.header_refs), ("footer", &refs.footer_refs)] {
                for r in list {
                    let key: Str = format!("{kind}:{}", r.rid).into();
                    let Some(&part) = self.hf_parts.get(&key) else { continue };
                    let Some(path) = doc.hf.get(part.index()).map(|p| p.path.clone()) else { continue };
                    let out = HfRef { r#type: r.r#type.clone(), rid: r.rid.clone(), path, part, ..HfRef::default() };
                    if kind == "header" {
                        sec.header_refs.push(out);
                        sec.headers.insert(&r.r#type, part);
                    } else {
                        sec.footer_refs.push(out);
                        sec.footers.insert(&r.r#type, part);
                    }
                }
            }
        }
        sec
    }
}

/// What [`SectionState::sync`] finds in the current sections.
enum Match {
    Same,
    /// the block ranges of every section, the rest unchanged
    Moved(Vec<(f64, f64)>),
    Differ,
}

/// The block index of paragraph `p`: `p` plus the anchors before it (those with
/// `before_para <= p`; the anchors are in non-decreasing order), by binary search.
fn block_of_para(s: &Story, p: usize) -> usize {
    let (mut lo, mut hi) = (0usize, s.tables.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if s.tables.get(mid).is_some_and(|a| (a.before_para as usize) <= p) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    p + lo
}

/// `[blockStart, blockEnd]` inclusive, as the reader gives them: a section ends at the block of
/// its break paragraph; the next starts right after it (the tables anchored before its first
/// paragraph included); the last ends at the last block (-1 for an empty body).
fn block_range(s: &Story, start_para: usize, end_para: Option<usize>) -> (f64, f64) {
    let block_start = match start_para.checked_sub(1).filter(|p| *p < s.paras.len()) {
        Some(p) => (block_of_para(s, p) + 1) as f64,
        None => 0.0,
    };
    let block_end = match end_para.filter(|p| *p < s.paras.len()) {
        Some(p) => block_of_para(s, p) as f64,
        None => s.block_count() as f64 - 1.0,
    };
    (block_start, block_end)
}
