//! D2b family (frontier D2b-F4): sections and page setup: `setPageSetup`, `setPageBorders`,
//! `insertSectionBreak`, `insertColumnBreak`, `setColumns`, `insertPageBreak`, `insertBlankPage`,
//! `setSectionStart`, `setVerticalAlignment`, `setLineNumbering`, `setLineNumberingOptions`,
//! `toggleLineNumberSuppression`, with docx-core `section-ops.ts` ([`section_ops`]) and the
//! line-numbering rules of `text-layout.ts` ([`super::text_layout`]). TS sources
//! (`universal-write.ts`): 5777–6030 (setPageSetup … insertPageBreak), 12143 (insertBlankPage),
//! 17448–17540 (line numbers), 17612–17640 (section start, vertical alignment).
//!
//! The section lists live in the paragraphs (`ParaProps.section_break`: the `sectPr` and the
//! references of each break) and in [`SectionState`](super::super::settle::SectionState) (the last
//! section's `sectPr` and references, the ids and read props); `syncSections` rebuilds
//! `doc.sections` at the settle step. A change of a `sectPr` makes a new `Arc` (the undo step keeps
//! the old one) and goes to the sections that held the old one, as the in-place mutation of the TS
//! does (notes.rs does the same for its options).
//!
//! What the TS reads from the layout comes as arguments (`hf_section`: the section of the page in
//! header/footer edit, `pageSections[hfActivePage]`).

pub mod section_ops;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uo_jsnum::conv::truthy;

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::settle::SectionRefs;
use super::super::state::Selection;
use super::super::text::{DocPosition, DocRange, normalize_range, para_length};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use super::text_layout::{
    LineNumberMemory, LineNumberMode, LineNumberingOptions, line_numbering_from_options, line_numbering_on, with_line_number_mode,
};
use crate::ids::ParaId;
use crate::js::{JsNumber, Nullable, Str};
use crate::model::background::PageBackground;
use crate::model::para::{HfRefEdit, Paragraph, SectionBreak};
use crate::model::run::Run;
use crate::model::section::{LineNumbering, PageBorders, SectPr};
use crate::word::obj;
use crate::write::emit::background::BackgroundAdded;
use crate::write::emit::settings::HyphenationSettings;
use section_ops::{ColumnPreset, ColumnSpec, Orientation, columns_for, preset_columns, section_index_at, section_ranges, set_orientation};

// ───────────────────────── the vocabulary ─────────────────────────

/// `SectionType` (`w:type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionType {
    #[default]
    NextPage,
    Continuous,
    NextColumn,
    EvenPage,
    OddPage,
}

impl SectionType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SectionType::NextPage => "nextPage",
            SectionType::Continuous => "continuous",
            SectionType::NextColumn => "nextColumn",
            SectionType::EvenPage => "evenPage",
            SectionType::OddPage => "oddPage",
        }
    }
}

/// `SectionApplyTo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionApplyTo {
    #[default]
    Section,
    Document,
    Forward,
    Selection,
}

/// `'top' | 'center' | 'both' | 'bottom'` (`w:vAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignment {
    Top,
    Center,
    Both,
    Bottom,
}

/// `size: 'A4' | 'Letter'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    A4,
    Letter,
}

/// `margins: 'normal' | 'narrow' | 'wide'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginPreset {
    Normal,
    Narrow,
    Wide,
}

/// The arguments of `setPageSetup(opts)`: the fields present change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageSetup {
    pub orientation: Option<Orientation>,
    pub size: Option<PageSize>,
    pub margins: Option<MarginPreset>,
    /// custom margins in twips: top, right, bottom, left
    pub custom_margins: Option<[f64; 4]>,
    pub section_start: Option<SectionType>,
    pub vertical_alignment: Option<VerticalAlignment>,
    pub line_numbering: Option<LineNumberingOptions>,
    pub apply_to: Option<SectionApplyTo>,
}

/// The `setColumns(spec)` argument: a gallery preset, or the More Columns specification.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnsArg {
    Preset(ColumnPreset),
    Spec { spec: ColumnSpec, apply_to: Option<SectionApplyTo>, start_new_column: bool },
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionCmd {
    /// `setPageSetup(opts)`
    SetPageSetup(Box<PageSetup>),
    /// `setPageBorders(pb, applyTo)`: `None` = none (`null`); `document_wide` = `'document'` (the default)
    SetPageBorders { borders: Option<Box<PageBorders>>, document_wide: bool },
    /// `insertSectionBreak(type)`
    InsertSectionBreak { kind: SectionType },
    /// `insertColumnBreak()`
    InsertColumnBreak,
    /// `insertPageBreak()`
    InsertPageBreak,
    /// `insertBlankPage()`
    InsertBlankPage,
    /// `setColumns(spec)`
    SetColumns(Box<ColumnsArg>),
    /// `setSectionStart(type, applyTo)`; `hf_section`: the layout's section of the page in header/footer edit
    SetSectionStart { kind: SectionType, apply_to: SectionApplyTo, hf_section: Option<u32> },
    /// `setVerticalAlignment(v, applyTo)`
    SetVerticalAlignment { v: VerticalAlignment, apply_to: SectionApplyTo, hf_section: Option<u32> },
    /// `setLineNumbering(mode)`
    SetLineNumbering { mode: LineNumberMode, hf_section: Option<u32> },
    /// `setLineNumberingOptions(o, applyTo)`
    SetLineNumberingOptions { o: LineNumberingOptions, apply_to: SectionApplyTo, hf_section: Option<u32> },
    /// `toggleLineNumberSuppression()`
    ToggleLineNumberSuppression,
}

impl SectionCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        Some(match self {
            SectionCmd::SetPageSetup(_) => ("setPageSetup", GuardArg::None),
            SectionCmd::SetPageBorders { .. } => ("setPageBorders", GuardArg::None),
            SectionCmd::InsertSectionBreak { kind } => ("insertSectionBreak", GuardArg::Str(kind.as_str())),
            SectionCmd::InsertColumnBreak => ("insertColumnBreak", GuardArg::None),
            SectionCmd::InsertPageBreak => ("insertPageBreak", GuardArg::None),
            SectionCmd::InsertBlankPage => ("insertBlankPage", GuardArg::None),
            SectionCmd::SetColumns(_) => ("setColumns", GuardArg::None),
            SectionCmd::SetSectionStart { .. } => ("setSectionStart", GuardArg::None),
            SectionCmd::SetVerticalAlignment { .. } => ("setVerticalAlignment", GuardArg::None),
            SectionCmd::SetLineNumbering { .. } => ("setLineNumbering", GuardArg::None),
            SectionCmd::SetLineNumberingOptions { .. } => ("setLineNumberingOptions", GuardArg::None),
            SectionCmd::ToggleLineNumberSuppression => ("toggleLineNumberSuppression", GuardArg::None),
        })
    }
}

// ───────────────────────── the family's state ─────────────────────────

/// What the file had at the install (the TS "as the document opened" values: `bgAtLoad`, the
/// settings the undo restores to when a step carries no edited value).
#[derive(Debug, Clone)]
pub struct LoadedSettings {
    pub even_odd: bool,
    pub default_tab: f64,
    pub hyph: HyphenationSettings,
    pub background: Option<Arc<PageBackground>>,
    pub display_background: bool,
}

impl Default for LoadedSettings {
    fn default() -> Self {
        LoadedSettings {
            even_odd: false,
            default_tab: 708.0,
            hyph: HyphenationSettings { auto: false, caps: true, zone: None, limit: 0.0 },
            background: None,
            display_background: false,
        }
    }
}

/// The sections / header-footer / page family's state of a session (the TS private fields
/// `lnMemory`, `pageNumberParas`, `hfRelsCopied`, `bgAtLoad`, `bgWritten`).
#[derive(Debug, Default)]
pub struct F4State {
    pub(crate) loaded: LoadedSettings,
    /// `lnMemory`: what a section whose numbering was switched off remembers, by section id (the TS
    /// keys the `sectPr` object, which an undo replaces: the memory is cleared at every restore)
    pub(crate) ln_memory: HashMap<u32, LineNumberMemory>,
    /// `pageNumberParas`: header/footer paragraphs with a PAGE field a gallery inserted in a
    /// document without sources (a `WeakSet` of objects; an undo makes new paragraphs, new ids)
    pub(crate) page_number_paras: HashSet<ParaId>,
    /// `hfRelsCopied`: relationship parts copied for unlinked parts (they leave the package if the
    /// part is not written)
    pub(crate) hf_rels_copied: HashSet<String>,
    /// `bgWritten`: what the last save added for the page background image
    pub(crate) bg_written: Option<BackgroundAdded>,
    /// the header/footer parts the file had (they are never freed) and the size of the index at
    /// which the unreferenced ones are freed next
    pub(crate) loaded_parts: usize,
    pub(crate) gc_at: usize,
}

impl F4State {
    /// `installFullDoc`: the file's values.
    pub(crate) fn install(doc: &crate::model::Document) -> F4State {
        let s = &doc.formatting.settings;
        F4State {
            loaded: LoadedSettings {
                even_odd: s.even_and_odd_headers,
                default_tab: doc.word_default_tab_fallback().unwrap_or_else(|| s.default_tab_stop.to_number()), // wave 7c (W, item 2)
                hyph: super::text_layout::hyphenation_of_formatting(s),
                background: doc.background.clone(),
                display_background: s.display_background_shape,
            },
            loaded_parts: doc.hf.len(),
            gc_at: (doc.hf.len() * 2).max(64),
            ..F4State::default()
        }
    }

    /// An undo/redo replaced every `sectPr` object (the JSON round trip): the memory keyed by them goes.
    pub(crate) fn on_restore(&mut self) {
        self.ln_memory.clear();
    }
}

type Res = Result<Outcome, CommandError>;

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The section id a break carries (`sb.id`): an integer that fits `u32`.
fn break_id_of(sb: &SectionBreak) -> Option<u32> {
    match sb.id {
        Some(JsNumber::Num(x)) if x >= 0.0 && x.fract() == 0.0 && x <= f64::from(u32::MAX) => Some(uo_jsnum::conv::to_uint32(x)),
        _ => None,
    }
}

// ───────────────────────── section accessors ─────────────────────────

impl Session {
    /// `canInsertSectionBreak()`: the body, editable.
    pub(crate) fn can_insert_section_break(&self) -> bool {
        !self.locked() && matches!(self.ctx, EditContext::Body)
    }

    /// The breaks of the BODY model (`bodyModel()`).
    pub(crate) fn body_section_breaks(&self) -> Vec<usize> {
        self.doc.body.paras.iter().enumerate().filter(|(_, p)| p.props.section_break.is_some()).map(|(i, _)| i).collect()
    }

    /// `sectionSectPr(m, k)` of the body.
    pub(crate) fn body_sect_pr_at(&self, k: usize) -> Arc<SectPr> {
        let breaks = self.body_section_breaks();
        match breaks.get(k).and_then(|&bi| self.doc.body.paras.get(bi)).and_then(|p| p.props.section_break.as_deref()) {
            Some(sb) => Arc::clone(&sb.sect_pr),
            None => Arc::clone(&self.sections.body_sect_pr),
        }
    }

    /// `sectionSectPr(m, k)` for a change, in the model of the CONTEXT (`body` = the body model:
    /// `bodyModel()`). In a cell, note or header/footer the context model has no breaks, so section 0
    /// is the body's `sectPr` (`this.model.sectPr`), as the TS does.
    pub(crate) fn edit_sect_pr_k(&mut self, body: bool, k: usize, f: impl FnOnce(&mut SectPr)) {
        let breaks = if body || matches!(self.ctx, EditContext::Body) { self.body_section_breaks() } else { Vec::new() };
        let bi = breaks.get(k).copied();
        let (old, new) = match bi {
            Some(bi) => {
                let Some(p) = self.doc.body.paras.make_mut(bi) else { return };
                let Some(sb) = Arc::make_mut(&mut p.props).section_break.as_deref_mut() else { return };
                let old = Arc::clone(&sb.sect_pr);
                f(Arc::make_mut(&mut sb.sect_pr));
                (old, Arc::clone(&sb.sect_pr))
            }
            None => {
                let old = Arc::clone(&self.sections.body_sect_pr);
                f(Arc::make_mut(&mut self.sections.body_sect_pr));
                (old, Arc::clone(&self.sections.body_sect_pr))
            }
        };
        self.retarget_sections(&old, &new);
    }

    /// The sections the layout holds that pointed to `old` follow the new object.
    fn retarget_sections(&mut self, old: &Arc<SectPr>, new: &Arc<SectPr>) {
        if !Arc::ptr_eq(old, new) && self.doc.sections.iter().any(|s| Arc::ptr_eq(&s.sect_pr, old)) {
            let v: Vec<crate::model::section::Section> = self
                .doc
                .sections
                .iter()
                .map(|s| {
                    if Arc::ptr_eq(&s.sect_pr, old) {
                        crate::model::section::Section { sect_pr: Arc::clone(new), ..s.clone() }
                    } else {
                        s.clone()
                    }
                })
                .collect();
            self.doc.sections = v.into();
        }
    }

    /// The `sectPr` of the break paragraph `bi` of the body, for a change.
    fn edit_break_sect_pr(&mut self, bi: usize, f: impl FnOnce(&mut SectPr)) {
        let Some(p) = self.doc.body.paras.make_mut(bi) else { return };
        let Some(sb) = Arc::make_mut(&mut p.props).section_break.as_deref_mut() else { return };
        let old = Arc::clone(&sb.sect_pr);
        f(Arc::make_mut(&mut sb.sect_pr));
        let new = Arc::clone(&sb.sect_pr);
        self.retarget_sections(&old, &new);
    }

    /// `sectionRefs(m, k)` of the body: the references of section `k` (the last: `bodyRefs`).
    pub(crate) fn body_section_refs(&self, k: usize) -> SectionRefs {
        let breaks = self.body_section_breaks();
        match breaks.get(k).and_then(|&bi| self.doc.body.paras.get(bi)).and_then(|p| p.props.section_break.as_deref()) {
            Some(sb) => SectionRefs { header_refs: sb.header_refs.clone(), footer_refs: sb.footer_refs.clone() },
            None => self.sections.body_refs.clone().unwrap_or_default(),
        }
    }

    /// `sectionRefs(m, k)` for a change (the last section creates `bodyRefs`, as `??=`).
    pub(crate) fn edit_section_refs(&mut self, k: usize, f: impl FnOnce(&mut Vec<HfRefEdit>, &mut Vec<HfRefEdit>)) {
        let breaks = self.body_section_breaks();
        match breaks.get(k) {
            Some(&bi) => {
                let Some(p) = self.doc.body.paras.make_mut(bi) else { return };
                let Some(sb) = Arc::make_mut(&mut p.props).section_break.as_deref_mut() else { return };
                f(&mut sb.header_refs, &mut sb.footer_refs);
            }
            None => {
                let r = self.sections.body_refs.get_or_insert_with(SectionRefs::default);
                f(&mut r.header_refs, &mut r.footer_refs);
            }
        }
    }

    /// `sectionRanges(m).length` of the body.
    pub(crate) fn body_section_count(&self) -> usize {
        self.body_section_breaks().len() + 1
    }

    /// The id of section `k` of the body (`sb.id`; 0 for the last).
    fn body_section_id(&self, k: usize) -> u32 {
        let breaks = self.body_section_breaks();
        breaks
            .get(k)
            .and_then(|&bi| self.doc.body.paras.get(bi))
            .and_then(|p| p.props.section_break.as_deref())
            .and_then(break_id_of)
            .unwrap_or(0)
    }

    /// `bodySectionRange()`: the sections of the BODY the caret/selection touches (in a cell, the
    /// table's; in a note and a text box, the point of the body; in header/footer, the section of the
    /// page in edit, which the layout gives).
    pub(crate) fn body_section_range(&self, hf_section: Option<u32>) -> (usize, usize) {
        let m = &self.doc.body.paras;
        let last = m.len().saturating_sub(1);
        let at = |p: u32| section_index_at(m, (p as usize).min(last));
        let ctx = match &self.ctx {
            EditContext::Textbox { outer, .. } if matches!(**outer, EditContext::HeaderFooter { .. }) => &**outer,
            c => c,
        };
        if matches!(ctx, EditContext::HeaderFooter { .. }) {
            let k = hf_section.unwrap_or(0) as usize;
            return (k, k);
        }
        let sel: Option<Selection> = match &self.ctx {
            EditContext::Note { saved, .. } | EditContext::Textbox { saved, .. } => Some(*saved),
            EditContext::Cell { .. } => None,
            _ => Some(self.selection),
        };
        match sel {
            None => {
                let before = match &self.ctx {
                    EditContext::Cell { anchor, .. } => self.doc.body.tables.get(*anchor as usize).map_or(0, |a| a.before_para),
                    _ => 0,
                };
                let k = at(before);
                (k, k)
            }
            Some(s) => {
                let r = normalize_range(DocRange::new(s.anchor, s.focus));
                (at(r.start.para), at(r.end.para))
            }
        }
    }

    /// `pageBorderTargets(applyTo)`: the sections of the BODY the borders go to.
    fn page_border_targets(&self, document_wide: bool) -> Vec<usize> {
        let n = self.body_section_count();
        if document_wide {
            return (0..n).collect();
        }
        let para = match &self.ctx {
            EditContext::Cell { anchor, .. } => self.doc.body.tables.get(*anchor as usize).map_or(0, |a| a.before_para),
            EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } => 0,
            _ => normalize_range(self.range()).start.para,
        };
        let m = &self.doc.body.paras;
        let last = m.len().saturating_sub(1);
        vec![section_index_at(m, (para as usize).min(last))]
    }

    /// What the editor shell's section queries read (wave 5, front O; record uo-bindings.md ED-201):
    /// additive, read-only, over the pub(crate) helpers above. `getSection()`, `getLineNumbering()`,
    /// `getPageBorders()` and `lineNumbersSuppressed()` of the TS are computed by the shell from it.
    #[must_use]
    pub fn shell_section_now(&self, hf_section: Option<u32>) -> ShellSection {
        let (k0, k1) = self.body_section_range(hf_section);
        let pb = self.page_border_targets(false).first().copied().unwrap_or(0);
        ShellSection {
            first: k0,
            count: self.body_section_count(),
            touched: (k0..=k1.max(k0)).map(|k| self.body_sect_pr_at(k)).collect(),
            page_border_section: self.body_sect_pr_at(pb),
            first_section_props: self.doc.sections.first().and_then(|s| s.sect.clone()),
            line_numbers_suppressed: self.line_numbers_suppressed(),
            title_pg: (0..self.body_section_count()).map(|k| self.body_sect_pr_at(k).title_pg.unwrap_or(false)).collect(),
        }
    }
}

/// The answer of [`Session::shell_section_now`].
#[derive(Debug, Clone)]
pub struct ShellSection {
    /// `bodySectionRange()[0]`
    pub first: usize,
    /// `sectionRanges(bodyModel()).length`
    pub count: usize,
    /// `sectionSectPr(m, k)` for k in `bodySectionRange()` (first..=last)
    pub touched: Vec<Arc<SectPr>>,
    /// `pageBorderTargets('section')[0]`
    pub page_border_section: Arc<SectPr>,
    /// `fullDoc.sections[0].sect` (the fallback of `getPageBorders`)
    pub first_section_props: Option<Arc<crate::model::section::SectionProps>>,
    /// `lineNumbersSuppressed()`
    pub line_numbers_suppressed: bool,
    /// `sectPr.titlePg` of every body section (the header slot of a page, `hfTypeOfPage`)
    pub title_pg: Vec<bool>,
}

impl Session {
    /// `sectionTargets(applyTo, breakType)`: the sections (indices in the context's model) a setting
    /// reaches; `forward` and `selection` open the breaks they ask for.
    fn section_targets(&mut self, apply_to: SectionApplyTo, break_type: SectionType) -> Result<Vec<usize>, CommandError> {
        let r = normalize_range(self.range());
        let (start, end) = (r.start, r.end);
        let n_ranges = |s: &Session| section_ranges(s.paras().unwrap_or(&crate::pvec::PVec::new())).len();
        match apply_to {
            SectionApplyTo::Document => Ok((0..n_ranges(self)).collect()),
            SectionApplyTo::Forward => {
                let at = self.insert_section_break_at(start, break_type)?;
                self.selection = Selection::caret(at);
                let k = section_index_at(self.paras().ok_or(CommandError::Position)?, at.para as usize);
                Ok(vec![k])
            }
            SectionApplyTo::Selection if start != end => {
                // the end first (the start does not move); the middle section is the selection's
                self.insert_section_break_at(end, break_type)?;
                let at = self.insert_section_break_at(start, break_type)?;
                let paras = self.paras().ok_or(CommandError::Position)?;
                let k = section_index_at(paras, at.para as usize);
                let ranges = section_ranges(paras);
                let r = ranges.get(k).copied().ok_or(CommandError::Position)?;
                let last = paras.get(r.end).map_or(0, para_length);
                self.selection = Selection { anchor: at, focus: DocPosition::new(u32_of(r.end), u32_of(last)) };
                Ok(vec![k])
            }
            _ => {
                let paras = self.paras().ok_or(CommandError::Position)?;
                let (k0, k1) = (section_index_at(paras, start.para as usize), section_index_at(paras, end.para as usize));
                Ok((k0..=k1).collect())
            }
        }
    }

    /// `lineNumberTargets(applyTo)`: the sections line numbers / section start / vertical alignment
    /// reach (the selection's, or the dialog's "apply to"; only the body opens breaks).
    fn line_number_targets(&mut self, apply_to: SectionApplyTo, hf_section: Option<u32>) -> Result<(bool, Vec<usize>), CommandError> {
        if apply_to == SectionApplyTo::Document {
            return Ok((true, (0..self.body_section_count()).collect()));
        }
        if matches!(self.ctx, EditContext::Body) && apply_to != SectionApplyTo::Section {
            return Ok((true, self.section_targets(apply_to, SectionType::NextPage)?));
        }
        let (k0, k1) = self.body_section_range(hf_section);
        Ok((true, (k0..=k1).collect()))
    }

    /// `sectPrRaw(closing)`: the raw `w:sectPr` of the section the new break copies (the closing
    /// paragraph's, or the document's for the last section), the base the copy is written over.
    fn sect_pr_raw(&self, closing: Option<usize>) -> Option<String> {
        let store = self.doc.sources.as_deref()?;
        match closing {
            None => store.stories.get("word/document.xml").and_then(|s| s.sect_pr.as_ref()).map(|t| t.resolve(&store.parts).to_owned()),
            Some(bi) => {
                let p = self.doc.body.paras.get(bi)?;
                if let Some(raw) = p.props.section_break.as_deref().and_then(|b| b.raw.as_deref()).filter(|r| !r.is_empty()) {
                    return Some(raw.to_owned());
                }
                let src = p.props.src?;
                let pp = store.paras.get(&src)?.p_pr.as_ref()?;
                let raw = pp.resolve(&store.parts);
                let limits = crate::limits::DocxLimits::default();
                let split = crate::write::lossless::split(raw, &limits.package).ok()?;
                split.kids.into_iter().find(|k| k.name == "w:sectPr").map(|k| k.raw.to_string())
            }
        }
    }

    /// `insertSectionBreakAt(pos, type)`: a break in the body (inside a step already open): splits
    /// the paragraph (guarded, tracked when tracking), the first half closes the new section — a
    /// full copy of the section's `sectPr` with the header/footer references, which leave the
    /// section after it — and the type goes to the section AFTER.
    pub(crate) fn insert_section_break_at(&mut self, pos: DocPosition, kind: SectionType) -> Result<DocPosition, CommandError> {
        let breaks = self.body_section_breaks();
        let m = &self.doc.body.paras;
        let k = section_index_at(m, pos.para as usize);
        let old_end = breaks.get(k).copied();
        let closing = old_end;
        // `sectionProps.set(id, sectionProps.get(range.sb?.id ?? 0))`
        let old_id = old_end.and_then(|bi| m.get(bi)).and_then(|p| p.props.section_break.as_deref()).and_then(break_id_of).unwrap_or(0);
        let id = self.sections.next_section_id;
        self.sections.next_section_id = id.saturating_add(1);
        let props = self.sections.section_props.get(&old_id).cloned().flatten();
        self.sections.section_props.insert(id, props);
        let raw = self.sect_pr_raw(closing);
        // coreInsertSectionBreak: the copy and the references BEFORE the split
        let (sp, refs) = match old_end.and_then(|bi| self.doc.body.paras.get(bi)).and_then(|p| p.props.section_break.as_deref()) {
            Some(sb) => (Arc::clone(&sb.sect_pr), SectionRefs { header_refs: sb.header_refs.clone(), footer_refs: sb.footer_refs.clone() }),
            None => (Arc::clone(&self.sections.body_sect_pr), self.sections.body_refs.clone().unwrap_or_default()),
        };
        let mut sb = SectionBreak {
            sect_pr: Arc::new((*sp).clone()),
            header_refs: refs.header_refs.clone(),
            footer_refs: refs.footer_refs.clone(),
            ..SectionBreak::default()
        };
        if let Some(raw) = raw.filter(|r| !r.is_empty()) {
            obj::set(&mut sb, "raw", |s| s.raw = Some(raw.as_str().into()));
        }
        obj::set(&mut sb, "id", |s| s.id = Some(JsNumber::Num(f64::from(id))));
        let next = self.split_typed(pos)?;
        {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            let first = paras.make_mut(pos.para as usize).ok_or(CommandError::Position)?;
            obj::set(Arc::make_mut(&mut first.props), "sectionBreak", |x| x.section_break = Some(Box::new(sb)));
        }
        // the section after starts with the break's type and is linked to the previous
        let after_bi = old_end.map(|bi| bi + 1);
        let set_type = |sp: &mut SectPr| {
            if kind == SectionType::NextPage {
                obj::delete(sp, "type");
            } else {
                obj::set(sp, "type", |s| s.r#type = Some(kind.as_str().into()));
            }
        };
        match after_bi {
            Some(bi) => {
                self.edit_break_sect_pr(bi, set_type);
                if let Some(p) = self.doc.body.paras.make_mut(bi)
                    && let Some(b) = Arc::make_mut(&mut p.props).section_break.as_deref_mut()
                {
                    b.header_refs = Vec::new();
                    b.footer_refs = Vec::new();
                }
            }
            None => {
                let old = Arc::clone(&self.sections.body_sect_pr);
                set_type(Arc::make_mut(&mut self.sections.body_sect_pr));
                let new = Arc::clone(&self.sections.body_sect_pr);
                self.retarget_sections(&old, &new);
                self.sections.body_refs = Some(SectionRefs::default());
            }
        }
        self.shift_anchors(pos.para, 1);
        self.touch_all();
        Ok(next)
    }

    /// `shiftAnchors(-1, delta)`: every table anchor and bookmark moves (`beforePara > -1`).
    pub(crate) fn shift_anchors_from_start(&mut self, delta: i64) {
        if matches!(self.ctx, EditContext::Cell { .. } | EditContext::Note { .. }) {
            return;
        }
        let shift = |v: u32| -> u32 { u32::try_from((i64::from(v) + delta).max(0)).unwrap_or(u32::MAX) };
        if let Some(story) = self.story_mut() {
            for i in 0..story.tables.len() {
                if let Some(a) = story.tables.make_mut(i) {
                    a.before_para = shift(a.before_para);
                }
            }
        }
        if matches!(self.ctx, EditContext::Body) {
            let map = self.bookmark_paras.map_mut();
            for (_, v) in map.iter_mut() {
                *v = shift(*v);
            }
        }
    }

    // ───────────────────────── the commands ─────────────────────────

    pub(crate) fn exec_sections(&mut self, cmd: SectionCmd, cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            SectionCmd::SetPageSetup(o) => self.set_page_setup(&o),
            SectionCmd::SetPageBorders { borders, document_wide } => self.set_page_borders(borders, document_wide),
            SectionCmd::InsertSectionBreak { kind } => self.insert_section_break(kind),
            SectionCmd::InsertColumnBreak => self.insert_column_break(),
            SectionCmd::InsertPageBreak => self.insert_page_break(),
            SectionCmd::InsertBlankPage => self.insert_blank_page(),
            SectionCmd::SetColumns(arg) => self.set_columns(&arg),
            SectionCmd::SetSectionStart { kind, apply_to, hf_section } => self.set_section_start(kind, apply_to, hf_section),
            SectionCmd::SetVerticalAlignment { v, apply_to, hf_section } => self.set_vertical_alignment(v, apply_to, hf_section),
            SectionCmd::SetLineNumbering { mode, hf_section } => self.set_line_numbering(mode, hf_section),
            SectionCmd::SetLineNumberingOptions { o, apply_to, hf_section } => self.set_line_numbering_options(&o, apply_to, hf_section),
            SectionCmd::ToggleLineNumberSuppression => self.toggle_line_number_suppression(cx),
        }
    }

    fn sdone(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    /// Ends a step whose body returned: an error drops the step (D2a-T1).
    fn end_step(&mut self, r: Result<(), CommandError>) -> Res {
        match r {
            Ok(()) => Ok(Outcome::Done(self.finish())),
            Err(e) => {
                self.discard_step();
                Err(e)
            }
        }
    }

    /// `ensureLineNumberStyle()`: the "line number" style enters `styles.xml` when the numbering
    /// turns on, as Word does (and stays after an undo: oracle ln-cont-undo).
    fn ensure_line_number_style(&mut self) {
        let idx = self.style_index();
        let k = uo_text::case::to_lowercase_str("line number");
        if idx.styles.iter().any(|s| uo_text::case::to_lowercase_str(&s.name) == k) {
            return;
        }
        let base = self.styles_base();
        let out = super::styles::xform::ensure_defined(&base, "Nmerodelinha", false);
        self.commit_styles(out);
    }

    /// `setSectionLineNumbering(sp, ln)` on section `k`: switching a section's numbering off
    /// remembers its start and distance.
    fn set_section_line_numbering(&mut self, body: bool, k: usize, ln: Option<LineNumbering>) {
        let id = self.section_id_for(body, k);
        let cur = self.sect_pr_for(body, k).ln_num_type.clone();
        if ln.is_none()
            && line_numbering_on(cur.as_ref())
            && let Some(c) = &cur
        {
            let start = c.start.map(JsNumber::to_number).filter(|s| truthy(*s));
            let distance = c.distance.map(JsNumber::to_number).filter(|d| truthy(*d));
            self.f4.ln_memory.insert(id, LineNumberMemory { start, distance });
        }
        self.edit_sect_pr_k(body, k, |sp| match ln {
            Some(l) => obj::set(sp, "lnNumType", |s| s.ln_num_type = Some(l)),
            None => obj::delete(sp, "lnNumType"),
        });
    }

    fn section_id_for(&self, body: bool, k: usize) -> u32 {
        if body || matches!(self.ctx, EditContext::Body) { self.body_section_id(k) } else { 0 }
    }

    /// The `sectPr` of section `k` of the context's model (see [`Session::edit_sect_pr_k`]).
    pub(crate) fn sect_pr_for(&self, body: bool, k: usize) -> Arc<SectPr> {
        if body || matches!(self.ctx, EditContext::Body) { self.body_sect_pr_at(k) } else { Arc::clone(&self.sections.body_sect_pr) }
    }

    // ── setPageSetup ───────────────────────────────────────────────────────────────────────

    fn set_page_setup(&mut self, o: &PageSetup) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        let ap = o.apply_to;
        if let Some(a) = ap
            && a != SectionApplyTo::Section
            && a != SectionApplyTo::Document
            && !self.can_insert_section_break()
        {
            return self.sdone();
        }
        if o.line_numbering.is_some_and(|l| l.on) {
            self.ensure_line_number_style();
        }
        self.begin_step(false, &[]);
        let r = self.set_page_setup_step(o);
        self.end_step(r)
    }

    fn set_page_setup_step(&mut self, o: &PageSetup) -> Result<(), CommandError> {
        let ln = o.line_numbering.as_ref().and_then(line_numbering_from_options);
        let targets = self.section_targets(o.apply_to.unwrap_or(SectionApplyTo::Section), SectionType::NextPage)?;
        for k in targets {
            if let Some(t) = o.section_start {
                self.edit_sect_pr_k(false, k, |s| set_section_type(s, t));
            }
            if let Some(v) = o.vertical_alignment {
                self.edit_sect_pr_k(false, k, |s| set_v_align(s, v));
            }
            if o.line_numbering.is_some() {
                self.set_section_line_numbering(false, k, ln.clone());
            }
            if let Some(size) = o.size {
                let (w, h) = if size == PageSize::A4 { (11906.0, 16838.0) } else { (12240.0, 15840.0) };
                self.edit_sect_pr_k(false, k, |s| {
                    let landscape = s.page_width_twips.to_number() > s.page_height_twips.to_number();
                    s.page_width_twips = JsNumber::Num(if landscape { h } else { w });
                    s.page_height_twips = JsNumber::Num(if landscape { w } else { h });
                });
            }
            if let Some(or) = o.orientation {
                self.edit_sect_pr_k(false, k, |s| set_orientation(s, or));
            }
            if let Some(m) = o.margins {
                let (tb, lr) = match m {
                    MarginPreset::Normal => (1417.0, 1701.0),
                    MarginPreset::Narrow => (720.0, 720.0),
                    MarginPreset::Wide => (1417.0, 2880.0),
                };
                self.edit_sect_pr_k(false, k, |s| {
                    s.margin_bottom_twips = JsNumber::Num(tb);
                    s.margin_top_twips = JsNumber::Num(tb);
                    s.margin_right_twips = JsNumber::Num(lr);
                    s.margin_left_twips = JsNumber::Num(lr);
                });
            }
            if let Some([t, r, b, l]) = o.custom_margins {
                self.edit_sect_pr_k(false, k, |s| {
                    s.margin_top_twips = JsNumber::Num(t);
                    s.margin_right_twips = JsNumber::Num(r);
                    s.margin_bottom_twips = JsNumber::Num(b);
                    s.margin_left_twips = JsNumber::Num(l);
                });
            }
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        Ok(())
    }

    // ── page borders ───────────────────────────────────────────────────────────────────────

    /// `setPageBorders(pb, applyTo)`: `None` = none (the property becomes `null`).
    fn set_page_borders(&mut self, pb: Option<Box<PageBorders>>, document_wide: bool) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        for k in self.page_border_targets(document_wide) {
            let v = match &pb {
                Some(b) => Nullable::Val(b.clone()),
                None => Nullable::Null,
            };
            self.edit_sect_pr_k(true, k, |s| obj::set(s, "pgBorders", |x| x.pg_borders = Some(v)));
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    // ── breaks ─────────────────────────────────────────────────────────────────────────────

    /// `insertSectionBreak(type)`.
    fn insert_section_break(&mut self, kind: SectionType) -> Res {
        if !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let pos = normalize_range(self.range()).start;
        match self.insert_section_break_at(pos, kind) {
            Ok(at) => {
                self.selection = Selection::caret(at);
                self.set_pending_props(None);
                Ok(Outcome::Done(self.finish()))
            }
            Err(e) => {
                self.discard_step();
                Err(e)
            }
        }
    }

    /// `insertColumnBreak()` (Ctrl+Shift+Enter): in the middle of the paragraph it is split and the
    /// break opens the one after; the selection is replaced.
    fn insert_column_break(&mut self) -> Res {
        if !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            if self.has_selection() {
                let p = self.delete_selection()?;
                self.selection = Selection::caret(p);
            }
            let mut pos = self.selection.focus;
            if pos.offset > 0 {
                pos = self.split_typed(pos)?;
                self.shift_anchors(pos.para.saturating_sub(1), 1);
            }
            let props = self.pending_props().cloned();
            pos = self.insert_typed(pos, &[0x0B], props.as_ref())?;
            self.set_pending_props(None);
            self.selection = Selection::caret(pos);
            Ok(())
        })();
        self.end_step(r)
    }

    /// `insertPageBreak()` (Ctrl+Enter): a paragraph of its own (Normal style); in the middle of the
    /// paragraph it is split and the break goes between the halves.
    fn insert_page_break(&mut self) -> Res {
        if !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            if self.has_selection() {
                let p = self.delete_selection()?;
                self.selection = Selection::caret(p);
            }
            let from = self.selection.focus;
            // insertPageBreakParagraph: split (when past the start), then the paragraph of the break
            let at = if from.offset > 0 {
                let tail = self.split_typed(from)?;
                self.shift_anchors(from.para, 1);
                tail
            } else {
                from
            };
            let index = at.para as usize;
            let para = self.page_break_paragraph();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            if index > paras.len() {
                return Err(CommandError::Position);
            }
            paras.insert(index, Arc::new(para));
            self.touch_structure();
            self.shift_anchors(u32_of(index), 1);
            if self.opts.tracking {
                // tracked changes: the break paragraph (run and mark) is an insertion
                let rev = self.new_rev();
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                if let Some(pb) = paras.make_mut(index) {
                    obj::set(Arc::make_mut(&mut pb.props), "markIns", |p| p.mark_ins = Some(Box::new(rev.clone())));
                    for r in &mut pb.runs {
                        let mut props = (*r.props).clone();
                        obj::set(&mut props, "ins", |p| p.ins = Some(Box::new(rev.clone())));
                        r.props = Arc::new(props);
                    }
                }
            }
            self.selection = Selection::caret(DocPosition::new(u32_of(index) + 1, 0));
            self.touch_para(index);
            Ok(())
        })();
        self.end_step(r)
    }

    /// `{ runs: [{ text: '\f', props: { ...DEFAULT_RUN_PROPS } }], props: {} }`.
    fn page_break_paragraph(&mut self) -> Paragraph {
        let run =
            Run { text: crate::text::Utf16::from("\u{0C}"), props: Arc::new(crate::read::plain::default_run_props()), ..Run::default() };
        Paragraph { id: self.doc.ids.para(), runs: vec![run], ..Paragraph::default() }
    }

    /// `insertBlankPage()` (Insert › Pages): splits the paragraph at the cursor (with a selection, at
    /// its START, deleting nothing) and puts [break, empty paragraph, break]; the caret goes to the
    /// empty one, which carries the paragraph's props. At the end of the document only [break, ¶]; at
    /// the start, only [¶, break] before the text, the caret where it was (oracle bp-*).
    fn insert_blank_page(&mut self) -> Res {
        if !self.can_insert_section_break() {
            return self.sdone();
        }
        let pos = normalize_range(self.range()).start;
        let n = match self.doc.body.paras.get(pos.para as usize) {
            Some(_) => self.doc.body.paras.len(),
            None => return self.sdone(),
        };
        let last = n - 1;
        let last_text_len = self.doc.body.paras.get(last).map_or(0, para_length);
        let at_end = pos.para as usize == last
            && pos.offset as usize >= last_text_len
            && !self.doc.body.tables.iter().any(|a| a.before_para as usize > last);
        let at_start = pos.para == 0 && pos.offset == 0 && !self.doc.body.tables.iter().any(|a| a.before_para == 0);
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            // `empty(P)`: the props of the paragraph as it is when called (a tracked split gave the first
            // half a new mark), without the section break and the source key
            let at_para = pos.para as usize;
            let empty = |s: &mut Session| -> Paragraph {
                let mut props = s.doc.body.paras.get(at_para).map_or_else(Default::default, |p| (*p.props).clone());
                obj::delete(&mut props, "sectionBreak");
                obj::delete(&mut props, "src");
                Paragraph { id: s.doc.ids.para(), props: Arc::new(props), ..Paragraph::default() }
            };
            if at_start {
                let (e, b) = (empty(self), self.page_break_paragraph());
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                paras.insert(0, Arc::new(e));
                paras.insert(1, Arc::new(b));
                self.touch_structure();
                self.shift_anchors_from_start(2);
                self.selection = Selection::caret(DocPosition::new(2, 0));
            } else if at_end {
                let (b, e) = (self.page_break_paragraph(), empty(self));
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                paras.insert(last + 1, Arc::new(b));
                paras.insert(last + 2, Arc::new(e));
                self.touch_structure();
                self.selection = Selection::caret(DocPosition::new(u32_of(last) + 2, 0));
            } else {
                let tail = self.split_typed(pos)?;
                self.shift_anchors(pos.para, 1);
                let at = tail.para as usize;
                let (b1, e, b2) = (self.page_break_paragraph(), empty(self), self.page_break_paragraph());
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                if at > paras.len() {
                    return Err(CommandError::Position);
                }
                paras.insert(at, Arc::new(b1));
                paras.insert(at + 1, Arc::new(e));
                paras.insert(at + 2, Arc::new(b2));
                self.touch_structure();
                self.shift_anchors(u32_of(at).saturating_sub(1), 3);
                self.selection = Selection::caret(DocPosition::new(u32_of(at) + 1, 0));
            }
            self.set_pending_props(None);
            Ok(())
        })();
        self.end_step(r)
    }

    // ── columns ────────────────────────────────────────────────────────────────────────────

    /// `setColumns(spec)`: a preset (with text selected: only for it, continuous breaks before and
    /// after) or the More Columns specification.
    fn set_columns(&mut self, arg: &ColumnsArg) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        let (preset, apply_to, new_col) = match arg {
            ColumnsArg::Preset(p) => {
                (Some(*p), if self.has_selection() { SectionApplyTo::Selection } else { SectionApplyTo::Section }, false)
            }
            ColumnsArg::Spec { apply_to, start_new_column, .. } => (None, apply_to.unwrap_or(SectionApplyTo::Section), *start_new_column),
        };
        if matches!(apply_to, SectionApplyTo::Forward | SectionApplyTo::Selection) && !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            let bt = if apply_to == SectionApplyTo::Forward && new_col { SectionType::NextColumn } else { SectionType::Continuous };
            let targets = self.section_targets(apply_to, bt)?;
            for k in targets {
                let sp = self.sect_pr_for(false, k);
                let spec = match arg {
                    ColumnsArg::Preset(p) => preset_columns(&sp, *p),
                    ColumnsArg::Spec { spec, .. } => spec.clone(),
                };
                let cols = columns_for(&sp, &spec);
                // one column: no `w:cols` in the model (the raw `w:space` and line stay, as in Word)
                self.edit_sect_pr_k(false, k, |s| match cols {
                    Some(c) => obj::set(s, "cols", |x| x.cols = Some(c)),
                    None => obj::set_undefined(s, "cols", |x| x.cols = None),
                });
            }
            let _ = preset;
            self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
            self.touch_formatting();
            self.touch_all();
            Ok(())
        })();
        self.end_step(r)
    }

    // ── section start, vertical alignment, line numbers ────────────────────────────────────

    /// `setSectionStart(type, applyTo)`.
    fn set_section_start(&mut self, kind: SectionType, apply_to: SectionApplyTo, hf_section: Option<u32>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        if matches!(apply_to, SectionApplyTo::Forward | SectionApplyTo::Selection) && !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            let (body, targets) = self.line_number_targets(apply_to, hf_section)?;
            for k in targets {
                self.edit_sect_pr_k(body, k, |s| set_section_type(s, kind));
            }
            self.after_section_change();
            Ok(())
        })();
        self.end_step(r)
    }

    /// `setVerticalAlignment(v, applyTo)`: top = no element (oracle sec-valign-*).
    fn set_vertical_alignment(&mut self, v: VerticalAlignment, apply_to: SectionApplyTo, hf_section: Option<u32>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        if matches!(apply_to, SectionApplyTo::Forward | SectionApplyTo::Selection) && !self.can_insert_section_break() {
            return self.sdone();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            let (body, targets) = self.line_number_targets(apply_to, hf_section)?;
            for k in targets {
                self.edit_sect_pr_k(body, k, |s| set_v_align(s, v));
            }
            self.after_section_change();
            Ok(())
        })();
        self.end_step(r)
    }

    fn after_section_change(&mut self) {
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
    }

    /// `setLineNumbering(mode)`: in the sections the selection touches; the other values stay.
    fn set_line_numbering(&mut self, mode: LineNumberMode, hf_section: Option<u32>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        if mode != LineNumberMode::None {
            self.ensure_line_number_style();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            let (body, targets) = self.line_number_targets(SectionApplyTo::Section, hf_section)?;
            for k in targets {
                let sp = self.sect_pr_for(body, k);
                let id = self.section_id_for(body, k);
                let memory = self.f4.ln_memory.get(&id).copied();
                let ln = with_line_number_mode(&sp, mode, memory);
                self.set_section_line_numbering(body, k, ln);
            }
            self.after_section_change();
            Ok(())
        })();
        self.end_step(r)
    }

    /// `setLineNumberingOptions(o, applyTo)`: the Line Numbers dialog.
    fn set_line_numbering_options(&mut self, o: &LineNumberingOptions, apply_to: SectionApplyTo, hf_section: Option<u32>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.sdone();
        }
        if matches!(apply_to, SectionApplyTo::Forward | SectionApplyTo::Selection) && !self.can_insert_section_break() {
            return self.sdone();
        }
        if o.on {
            self.ensure_line_number_style();
        }
        self.begin_step(false, &[]);
        let r = (|| -> Result<(), CommandError> {
            let ln = line_numbering_from_options(o);
            let (body, targets) = self.line_number_targets(apply_to, hf_section)?;
            for k in targets {
                self.set_section_line_numbering(body, k, ln.clone());
            }
            self.after_section_change();
            Ok(())
        })();
        self.end_step(r)
    }

    /// `lineNumbersSuppressed()`: some paragraph of the selection suppresses the numbers.
    fn line_numbers_suppressed(&self) -> bool {
        let Some(paras) = self.paras() else { return false };
        let r = normalize_range(self.range());
        let mut resolver = crate::word::resolve::StyleResolver::new();
        for pi in r.start.para..=r.end.para {
            let Some(p) = paras.get(pi as usize) else { continue };
            let eff = crate::word::direct::effective_ppr(&self.doc.formatting, &mut resolver, p.ppr.as_deref(), &p.props);
            if super::paragraph::flow_value(&eff, "suppressLineNumbers") {
                return true;
            }
        }
        false
    }

    /// `toggleLineNumberSuppression()` (Suppress for Current Paragraph): toggles; with some already
    /// suppressed, takes it off all of them (oracle ln-suppress-mso-mixed).
    fn toggle_line_number_suppression(&mut self, cx: &mut ExecCx<'_>) -> Res {
        let want = !self.line_numbers_suppressed();
        let f = super::paragraph::FlowArgs { suppress_line_numbers: Some(want), ..super::paragraph::FlowArgs::default() };
        self.exec_paragraph(super::paragraph::ParagraphCmd::SetParagraphFlow { f }, cx)
    }
}

/// `type`: `delete sp.type` for next page, else the type.
fn set_section_type(sp: &mut SectPr, t: SectionType) {
    if t == SectionType::NextPage {
        obj::delete(sp, "type");
    } else {
        obj::set(sp, "type", |s| s.r#type = Some(t.as_str().into()));
    }
}

/// `vAlign`: top = no element.
fn set_v_align(sp: &mut SectPr, v: VerticalAlignment) {
    let name: Option<&str> = match v {
        VerticalAlignment::Top => None,
        VerticalAlignment::Center => Some("center"),
        VerticalAlignment::Both => Some("both"),
        VerticalAlignment::Bottom => Some("bottom"),
    };
    match name {
        None => obj::delete(sp, "vAlign"),
        Some(n) => obj::set(sp, "vAlign", |s| s.v_align = Some(Str::from(n))),
    }
}
