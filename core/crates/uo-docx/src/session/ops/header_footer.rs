//! D2b family (frontier D2b-F4): headers and footers: the section options (`setDifferentFirstPage`,
//! `setDifferentOddEven`, `setHeaderFooterDistance`, `setLinkToPrevious`, `setPageNumberFormat`),
//! `removeHeaderFooter`, `removePageNumbers`, and the new part of `enterHeaderFooter`
//! (`CreateHeaderFooterPart`); docx-core `hf-ops.ts` (the pure half is in
//! `write/emit/settings.rs`: `hfPartXml`, `copyHfPartXml`, `dropUnusedHfParts`,
//! `pageNumberGroupIds`). TS sources: `universal-write.ts` 10967–11200 (`registerHfPart`,
//! `evenOddHeaders`, `restoreHfPart`, `hfStyleId`, `newHfParas`, `newHfTarget`, `hfPartFor`,
//! `hfSectionOp`, the options), 11200–11450 (link to previous, `createHfPart`, remove, page
//! numbers, the number format). Rules measured in Word (`write/tools/hf-lab`, memory
//! `word-header-footer-facts`): each option is ONE undo step of the document; in header mode the
//! step leaves the edit, changes the body (sectPr/refs live in the model) and comes back to the
//! same page, where the part shown can have changed.
//!
//! What the TS reads from the layout comes as arguments (D2a-C2, G21b): [`HfAt`] is the answer of
//! `hfContext()` (the section of the caret's page and the variant it shows) and `reenter` the
//! part the layout shows on the page in edit AFTER the change (`None`: the part edited before).
//! Not ported here (recorded in `F4.md`): the template galleries (`insertHeaderFooterTemplate`,
//! `hf-templates.ts`), `insertDateTime`, `insertDocumentInfo`, `insertAlignmentTab` (field runs and
//! `Intl` dates), `goToHeaderFooter`/`next`/`previous` (layout navigation), `showDocumentText`
//! (display).

use std::sync::Arc;

use uo_jsnum::math::{floor, max, min, round};

use super::super::command::{CommandError, Event, Outcome};
use super::super::policy::GuardArg;
use super::super::state::Selection;
use super::super::{ExecCx, Session};
use super::context::EditContext;
use crate::ids::HfPartId;
use crate::js::{JsNumber, Str};
use crate::model::Document;
use crate::model::para::{HfRefEdit, Paragraph};
use crate::model::section::{HfPart, SectPr};
use crate::model::story::Story;
use crate::word::obj;
use crate::write::emit::settings::{copy_hf_part_xml, page_number_group_ids};

/// The most header/footer parts a session holds (an undo leaves the parts an unlink created, as the
/// TS `hfParts`; the cap keeps a hostile script from growing them without end).
pub const MAX_HF_PARTS: usize = 10_000;

/// `'header' | 'footer'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfKind {
    Header,
    Footer,
}

impl HfKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            HfKind::Header => "header",
            HfKind::Footer => "footer",
        }
    }
}

/// `HfKind` of the TS (`'default' | 'first' | 'even'`): the variant a page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HfSlot {
    #[default]
    Default,
    First,
    Even,
}

impl HfSlot {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            HfSlot::Default => "default",
            HfSlot::First => "first",
            HfSlot::Even => "even",
        }
    }
}

/// The answer of `hfContext()` (the layout's): the section (index in the body) of the page the
/// caret is on, and the variant of the part in focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HfAt {
    pub section: u32,
    pub slot: HfSlot,
}

/// Where the header/footer edit returns to after an option taken in it (the layout's answer
/// AFTER the change, D2a-C2): the part the page shows now, or - when the section has none of that
/// variant yet - the part `enterHeaderFooter` creates for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfReenter {
    Part(HfPartId),
    Create { kind: HfKind, section: u32, slot: HfSlot },
}

/// `PageNumberFormat` patch of `setPageNumberFormat(f)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageNumberFormatPatch {
    /// `w:fmt` (`decimal` removes it)
    pub format: Option<String>,
    /// `Some(None)` = no chapter number; `Some(Some((level, separator)))`
    pub chapter: Option<Option<(f64, String)>>,
    /// `Some(None)` = continue from the previous section
    pub start: Option<Option<f64>>,
}

/// `PageNumberFormat` (the dialog).
#[derive(Debug, Clone, PartialEq)]
pub struct PageNumberFormat {
    pub format: String,
    pub chapter: Option<(f64, String)>,
    pub start: Option<f64>,
}

/// `HeaderFooterOptions` (the Header & Footer tab).
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooterOptions {
    pub section: usize,
    pub section_count: usize,
    pub part: Option<(HfKind, HfSlot)>,
    pub different_first_page: bool,
    pub different_odd_even: bool,
    pub header_distance: f64,
    pub footer_distance: f64,
    pub link_to_previous: bool,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderFooterCmd {
    /// `setDifferentFirstPage(on)` (`w:titlePg` of the section)
    SetDifferentFirstPage { on: bool, at: HfAt, reenter: Option<HfReenter> },
    /// `setDifferentOddEven(on)` (`w:evenAndOddHeaders`: the whole document)
    SetDifferentOddEven { on: bool, reenter: Option<HfReenter> },
    /// `setHeaderFooterDistance({ header, footer })`
    SetHeaderFooterDistance { header: Option<f64>, footer: Option<f64>, at: HfAt, reenter: Option<HfReenter> },
    /// `setLinkToPrevious(on, kind?)`; `kind` `None` = the kind in edit, else header
    SetLinkToPrevious { on: bool, kind: Option<HfKind>, at: HfAt, reenter: Option<HfReenter> },
    /// `removeHeaderFooter(kind)`: empties the part of the caret's page (`at`)
    RemoveHeaderFooter { kind: HfKind, at: HfAt },
    /// `removePageNumbers()` on the header and footer of the caret's page (`at`)
    RemovePageNumbers { at: HfAt },
    /// `setPageNumberFormat(f)`
    SetPageNumberFormat { f: PageNumberFormatPatch, at: HfAt, reenter: Option<HfReenter> },
    /// The new-part branch of `enterHeaderFooter(kind, page)`: the section defining the part
    /// (`section`: the layout's `hfPartOfPage`) has none of this variant yet; the part and its
    /// reference are made, the shell then enters it with `EnterHeaderFooter`.
    CreateHeaderFooterPart { kind: HfKind, section: u32, slot: HfSlot },
}

impl HeaderFooterCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match self {
            HeaderFooterCmd::SetDifferentFirstPage { .. } => Some(("setDifferentFirstPage", GuardArg::None)),
            HeaderFooterCmd::SetDifferentOddEven { .. } => Some(("setDifferentOddEven", GuardArg::None)),
            HeaderFooterCmd::SetHeaderFooterDistance { .. } => Some(("setHeaderFooterDistance", GuardArg::None)),
            HeaderFooterCmd::SetPageNumberFormat { .. } => Some(("setPageNumberFormat", GuardArg::None)),
            _ => None,
        }
    }
}

type Res = Result<Outcome, CommandError>;

fn kind_str(k: HfKind) -> Str {
    k.as_str().into()
}

impl Session {
    pub(crate) fn exec_header_footer(&mut self, cmd: HeaderFooterCmd, _cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            HeaderFooterCmd::SetDifferentFirstPage { on, at, reenter } => {
                let sec = self.hf_section_of(at);
                self.hf_section_op(reenter, &[], |s| {
                    s.edit_sect_pr_k(true, sec, |sp| {
                        if on {
                            obj::set(sp, "titlePg", |x| x.title_pg = Some(true));
                        } else {
                            obj::delete(sp, "titlePg");
                        }
                    });
                    Ok(())
                })
            }
            HeaderFooterCmd::SetDifferentOddEven { on, reenter } => {
                if self.paras().is_none() || on == self.even_odd_headers() {
                    return Ok(Outcome::Done(self.change_set()));
                }
                self.hf_section_op(reenter, &[], |s| {
                    s.set_even_odd_flag(on);
                    Ok(())
                })
            }
            HeaderFooterCmd::SetHeaderFooterDistance { header, footer, at, reenter } => {
                let sec = self.hf_section_of(at);
                let clamp = |v: f64| max(0.0, min(round(v), 31680.0));
                self.hf_section_op(reenter, &[], |s| {
                    s.edit_sect_pr_k(true, sec, |sp| {
                        if let Some(h) = header {
                            obj::set(sp, "headerDistanceTwips", |x| x.header_distance_twips = Some(JsNumber::Num(clamp(h))));
                        }
                        if let Some(f) = footer {
                            obj::set(sp, "footerDistanceTwips", |x| x.footer_distance_twips = Some(JsNumber::Num(clamp(f))));
                        }
                    });
                    Ok(())
                })
            }
            HeaderFooterCmd::SetLinkToPrevious { on, kind, at, reenter } => self.set_link_to_previous(on, kind, at, reenter),
            HeaderFooterCmd::RemoveHeaderFooter { kind, at } => self.remove_header_footer(kind, at),
            HeaderFooterCmd::RemovePageNumbers { at } => self.remove_page_numbers(at),
            HeaderFooterCmd::SetPageNumberFormat { f, at, reenter } => {
                let sec = self.hf_section_of(at);
                self.hf_section_op(reenter, &[], |s| {
                    s.edit_sect_pr_k(true, sec, |sp| {
                        if let Some(fmt) = &f.format {
                            if fmt == "decimal" {
                                obj::delete(sp, "pgNumFmt");
                            } else {
                                obj::set(sp, "pgNumFmt", |x| x.pg_num_fmt = Some(fmt.as_str().into()));
                            }
                        }
                        if let Some(ch) = &f.chapter {
                            match ch {
                                Some((level, sep)) => {
                                    obj::set(sp, "pgNumChapStyle", |x| {
                                        x.pg_num_chap_style = Some(JsNumber::Num(max(1.0, min(9.0, *level))))
                                    });
                                    obj::set(sp, "pgNumChapSep", |x| x.pg_num_chap_sep = Some(sep.as_str().into()));
                                }
                                None => {
                                    obj::delete(sp, "pgNumChapStyle");
                                    obj::delete(sp, "pgNumChapSep");
                                }
                            }
                        }
                        if let Some(st) = &f.start {
                            match st {
                                None => obj::delete(sp, "pgNumStart"),
                                Some(v) => obj::set(sp, "pgNumStart", |x| x.pg_num_start = Some(JsNumber::Num(max(0.0, round(*v))))),
                            }
                        }
                    });
                    Ok(())
                })
            }
            HeaderFooterCmd::CreateHeaderFooterPart { kind, section, slot } => self.create_header_footer_part(kind, section, slot),
        }
    }

    // ── helpers of the TS ──────────────────────────────────────────────────────────────────

    /// `evenOddHeaders()`.
    #[must_use]
    pub fn even_odd_headers(&self) -> bool {
        self.settings.even_odd.unwrap_or(self.doc.formatting.settings.even_and_odd_headers)
    }

    /// `setEvenOddFlag(on)`: the state and the settings the engine reads.
    pub(crate) fn set_even_odd_flag(&mut self, on: bool) {
        let mut st = (*self.settings).clone();
        st.even_odd = Some(on);
        self.settings = Arc::new(st);
        let f = Arc::make_mut(&mut self.doc.formatting);
        obj::set(&mut f.settings, "evenAndOddHeaders", |s| s.even_and_odd_headers = on);
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
    }

    /// `Math.min(pageSections[page] ?? 0, sectionRanges(m).length - 1)`.
    fn hf_section_of(&self, at: HfAt) -> usize {
        (at.section as usize).min(self.body_section_count().saturating_sub(1))
    }

    /// `refsOfSection(m, sec, kind)`.
    fn refs_of_section(&self, sec: usize, kind: HfKind) -> Vec<HfRefEdit> {
        let r = self.body_section_refs(sec);
        if kind == HfKind::Header { r.header_refs } else { r.footer_refs }
    }

    /// `hfPartFor(sec, kind, type)`: the part section `sec` uses (its own or inherited from the
    /// previous ones): the key and the section that defines it.
    pub(crate) fn hf_part_for(&self, sec: usize, kind: HfKind, slot: &str) -> Option<(HfPartId, usize)> {
        let mut s = sec.min(self.body_section_count().saturating_sub(1)) as isize;
        while s >= 0 {
            let refs = self.refs_of_section(s as usize, kind);
            if let Some(r) = refs.iter().find(|x| &*x.r#type == slot) {
                let key: Str = format!("{}:{}", kind.as_str(), r.rid).into();
                if let Some(&p) = self.sections.hf_parts.get(&key) {
                    return Some((p, s as usize));
                }
            }
            s -= 1;
        }
        None
    }

    /// The part in edit (`hfEdit`) when it is a `kind`.
    fn hf_edit_part(&self, kind: HfKind) -> Option<HfPartId> {
        let EditContext::HeaderFooter { part, .. } = &self.ctx else { return None };
        let path = self.doc.hf.get(part.index())?.path.as_ref();
        let want = kind.as_str();
        let is = self.doc.sections.iter().any(|s| {
            let refs = if kind == HfKind::Header { &s.header_refs } else { &s.footer_refs };
            refs.iter().any(|r| &*r.path == path)
        });
        // the kind of the part in edit: the section references say it (a part is a header or a footer)
        let _ = want;
        is.then_some(*part)
    }

    /// `hfStyleId(kind)`: the document's Header/Footer style (Word creates it with the first part).
    fn hf_style_id(&mut self, kind: HfKind) -> Option<String> {
        let idx = self.style_index();
        let k = kind.as_str();
        let found = idx.styles.iter().find(|s| uo_text::case::to_lowercase_str(&s.name) == k).map(|s| s.id.to_string());
        let id = match found {
            Some(id) => id,
            None => {
                let pt = if kind == HfKind::Header { "Cabealho" } else { "Rodap" };
                super::styles::xform::resolve_style_id(self.styles.xml().map(|x| &**x), pt).to_string()
            }
        };
        self.ensure_style_used(Some(&id));
        Some(id)
    }

    /// `newHfParas(kind)`: an empty paragraph in the Header/Footer style (Word).
    fn new_hf_story(&mut self, kind: HfKind) -> Story {
        let style = self.hf_style_id(kind);
        let mut props = crate::model::para::ParaProps::default();
        if let Some(id) = style {
            obj::set(&mut props, "styleId", |p| p.style_id = Some(id.as_str().into()));
        }
        let mut st = Story::new();
        st.paras.push(Paragraph { id: self.doc.ids.para(), props: Arc::new(props), ..Paragraph::default() });
        st
    }

    /// `newHfTarget(kind)`: the path and rId free for a new part (`headerN.xml`, `rIdN`, as Word).
    fn new_hf_target(&self, kind: HfKind) -> (Str, Str) {
        let k = kind.as_str();
        let mut used: std::collections::HashSet<String> = self.pkg.part_names().map(str::to_owned).collect();
        for p in self.doc.hf.iter() {
            used.insert(p.path.to_string());
        }
        let mut n = 1u64;
        while used.contains(&format!("word/{k}{n}.xml")) {
            n += 1;
        }
        let rels = self.pkg.part_text("word/_rels/document.xml.rels").ok().flatten().map(std::borrow::Cow::into_owned).unwrap_or_default();
        let mut rids: std::collections::HashSet<String> = ids_in(&rels).into_iter().collect();
        for key in self.sections.hf_parts.keys() {
            rids.insert(key.split_once(':').map_or(key.to_string(), |(_, r)| r.to_owned()));
        }
        let mut m = 1u64;
        while rids.contains(&format!("rId{m}")) {
            m += 1;
        }
        (format!("rId{m}").into(), format!("word/{k}{n}.xml").into())
    }

    /// `hfSectionOp(fn, parts)`: an option of section/document as ONE undo step; in header mode the
    /// step leaves the edit, changes the body and returns to the page, in the part the layout shows
    /// there now (`reenter`; none: the one edited before).
    fn hf_section_op(
        &mut self,
        reenter: Option<HfReenter>,
        parts: &[HfPartId],
        f: impl FnOnce(&mut Session) -> Result<(), CommandError>,
    ) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        if matches!(self.ctx, EditContext::Textbox { .. }) {
            self.exit_textbox();
        }
        let hf = match &self.ctx {
            EditContext::HeaderFooter { part, page, .. } => Some((*part, *page)),
            _ => None,
        };
        self.begin_step(false, parts);
        if let EditContext::HeaderFooter { saved, .. } = self.ctx {
            // `exitHeaderFooter(true, false)`: silent
            self.ctx = EditContext::Body;
            self.selection = saved;
            self.set_pending_props(None);
        }
        let r = f(self);
        if let Err(e) = r {
            self.discard_step();
            return Err(e);
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        // `refresh()` (the body is in context here: `syncSections`), then back to the page
        self.settle();
        if let Some((part, page)) = hf {
            let target = match reenter {
                None => Some(part),
                Some(HfReenter::Part(p)) => Some(p),
                Some(HfReenter::Create { kind, section, slot }) => self.make_section_part(kind, section, slot),
            };
            match target.filter(|p| self.doc.hf.get(p.index()).is_some()) {
                Some(part) if !self.locked() => {
                    let saved = self.selection;
                    self.ctx = EditContext::HeaderFooter { part, page, saved };
                    self.selection = Selection::default();
                    self.set_pending_props(None);
                    self.emit(Event::HfMode { active: true });
                }
                _ => self.emit(Event::HfMode { active: false }),
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `createHfPart(kind, target, srcKey)`: a new part registered: a copy of the source (as Word
    /// does when unlinking) or an empty one. A source never edited comes from its XML (byte for
    /// byte, new drawing ids, the relationships repeated); an edited one from the model.
    fn create_hf_part(&mut self, kind: HfKind, target: (Str, Str), src: Option<HfPartId>) -> Result<HfPartId, CommandError> {
        if self.doc.hf.len() >= MAX_HF_PARTS {
            return Err(CommandError::Position);
        }
        let key: Str = format!("{}:{}", kind.as_str(), target.0).into();
        let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
        let src_path = src.and_then(|s| self.doc.hf.get(s.index())).map(|p| p.path.to_string());
        let copy_rels = |s: &mut Session, from: &str| {
            let rels = s.pkg.part_text(&format!("word/_rels/{}.rels", name(from))).ok().flatten().map(std::borrow::Cow::into_owned);
            if let Some(rels) = rels {
                let to = format!("word/_rels/{}.rels", name(&target.1));
                s.pkg.set_part_text(to.clone(), rels);
                s.f4.hf_rels_copied.insert(to);
            }
        };
        let mut story: Option<Story> = None;
        let mut dirty = true;
        if let (Some(sp), Some(sid)) = (&src_path, src)
            && !self.dirty_hf.contains(&sid)
            && self.doc.sources.is_some()
            && let Some(text) = self.pkg.part_text(sp).ok().flatten().map(std::borrow::Cow::into_owned)
        {
            copy_rels(self, sp);
            // `1 + Math.floor(Math.random() * 0x7ffffffe)` per drawing id, in the order of the text
            let env = &mut *self.env;
            let xml =
                copy_hf_part_xml(&text, &mut || 1.0 + floor(f64::from(env.random_u32()) / 4_294_967_296.0 * f64::from(0x7fff_fffe_u32)));
            if let Ok(Some(read)) = self.read_hf_part_story(&target.1, &xml) {
                story = Some(read);
                dirty = false;
            }
        }
        if story.is_none() {
            let have = src.and_then(|s| self.doc.hf.get(s.index())).map(|p| p.story.clone());
            match have {
                Some(mut st) => {
                    if let Some(sp) = &src_path
                        && self.pkg.contains(sp)
                    {
                        copy_rels(self, sp);
                    }
                    // `cloneBlocks`: new objects (new ids)
                    super::super::normalize::normalize(&mut st, &mut self.doc.ids);
                    story = Some(st);
                }
                None => story = Some(self.new_hf_story(kind)),
            }
        }
        let story = story.unwrap_or_default();
        if self.doc.hf.len() >= self.f4.gc_at {
            self.gc_hf_parts();
        }
        // a slot freed by `gc_hf_parts` is used again (ids are indices)
        let free = (self.f4.loaded_parts..self.doc.hf.len()).find(|i| self.doc.hf.get(*i).is_some_and(|h| h.path.is_empty()));
        let part = HfPart { path: target.1.clone(), story, ..HfPart::default() };
        let id = match free {
            Some(i) => {
                if let Some(h) = self.doc.hf.make_mut(i) {
                    *h = part;
                }
                HfPartId(u32::try_from(i).unwrap_or(u32::MAX))
            }
            None => {
                let id = HfPartId(u32::try_from(self.doc.hf.len()).unwrap_or(u32::MAX));
                self.doc.hf.push(part);
                id
            }
        };
        self.sections.hf_parts.insert(key, id);
        if dirty {
            self.dirty_hf.insert(id);
        }
        Ok(id)
    }

    /// `setLinkToPrevious(on, kind?)`: unlinking copies the inherited content into a new part of
    /// the section (Word); linking drops the section's part (it uses the previous one again).
    fn set_link_to_previous(&mut self, on: bool, kind: Option<HfKind>, at: HfAt, reenter: Option<HfReenter>) -> Res {
        let sec = self.hf_section_of(at);
        if self.paras().is_none() || sec == 0 || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let k = kind.unwrap_or_else(|| self.hf_edit_kind().unwrap_or(HfKind::Header));
        let slot = at.slot.as_str();
        let own = self.refs_of_section(sec, k).iter().any(|r| &*r.r#type == slot);
        if on != own {
            return Ok(Outcome::Done(self.change_set()));
        }
        if on {
            return self.hf_section_op(reenter, &[], |s| {
                s.edit_section_refs(sec, |h, f| {
                    let list = if k == HfKind::Header { h } else { f };
                    if let Some(i) = list.iter().position(|r| &*r.r#type == slot) {
                        list.remove(i);
                    }
                });
                Ok(())
            });
        }
        let src = self.hf_part_for(sec - 1, k, slot).map(|(p, _)| p);
        let target = self.new_hf_target(k);
        self.hf_section_op(reenter, &[], |s| {
            s.create_hf_part(k, target.clone(), src)?;
            s.edit_section_refs(sec, |h, f| {
                let list = if k == HfKind::Header { h } else { f };
                list.push(HfRefEdit { kind: kind_str(k), r#type: slot.into(), rid: target.0.clone(), ..HfRefEdit::default() });
            });
            Ok(())
        })
    }

    /// The kind (header/footer) of the part in edit.
    fn hf_edit_kind(&self) -> Option<HfKind> {
        if self.hf_edit_part(HfKind::Header).is_some() {
            Some(HfKind::Header)
        } else if self.hf_edit_part(HfKind::Footer).is_some() {
            Some(HfKind::Footer)
        } else {
            None
        }
    }

    /// The part of `kind` the page of `at` shows (`hfPartOfPageKey`), or the one in edit when it is
    /// of that kind.
    fn hf_part_of_page(&self, kind: HfKind, at: HfAt) -> Option<HfPartId> {
        if let Some(p) = self.hf_edit_part(kind) {
            return Some(p);
        }
        self.hf_part_for(self.hf_section_of(at), kind, at.slot.as_str()).map(|(p, _)| p)
    }

    /// `removeHeaderFooter(kind)`: empties the part of the caret's page (one empty paragraph in
    /// the style; the part stays: oracle e12).
    fn remove_header_footer(&mut self, kind: HfKind, at: HfAt) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let Some(part) = self.hf_part_of_page(kind, at) else { return Ok(Outcome::Done(self.change_set())) };
        self.begin_step(false, &[part]);
        let fresh = self.new_hf_story(kind);
        if let Some(h) = self.doc.hf.make_mut(part.index()) {
            h.story = fresh;
        }
        self.dirty_hf.insert(part);
        if matches!(&self.ctx, EditContext::HeaderFooter { part: p, .. } if *p == part) {
            self.selection = Selection::default();
        }
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// `removePageNumbers()`: the page-number blocks of the Page Numbers galleries (`w:sdt`
    /// "Page Numbers") leave the header and the footer of the caret's page (oracle h04/h05).
    fn remove_page_numbers(&mut self, at: HfAt) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let mut plan: Vec<(HfKind, HfPartId, Vec<usize>)> = Vec::new();
        for kind in [HfKind::Header, HfKind::Footer] {
            let Some(part) = self.hf_part_of_page(kind, at) else { continue };
            let Some(h) = self.doc.hf.get(part.index()) else { continue };
            let ids = match self.doc.sources.as_deref() {
                Some(store) => page_number_group_ids(store.stories.get(&h.path), &store.parts),
                None => Default::default(),
            };
            let gone: Vec<usize> = h
                .story
                .paras
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.props.src.is_some_and(|k| ids.contains(&crate::js::NumKey::new(f64::from(k.0))))
                        || self.f4.page_number_paras.contains(&p.id)
                })
                .map(|(i, _)| i)
                .collect();
            if !gone.is_empty() {
                plan.push((kind, part, gone));
            }
        }
        if plan.is_empty() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let parts: Vec<HfPartId> = plan.iter().map(|p| p.1).collect();
        self.begin_step(false, &parts);
        for (kind, part, gone) in plan {
            let fresh_needed;
            {
                let Some(h) = self.doc.hf.make_mut(part.index()) else { continue };
                let st = &mut h.story;
                // the table anchors follow the paragraphs that leave
                for i in 0..st.tables.len() {
                    let Some(b) = st.tables.get(i).map(|a| a.before_para) else { continue };
                    let removed_before = gone.iter().filter(|g| (**g as u64) < u64::from(b)).count();
                    if removed_before > 0
                        && let Some(a) = st.tables.make_mut(i)
                    {
                        a.before_para = b - u32::try_from(removed_before).unwrap_or(0);
                    }
                }
                for g in gone.iter().rev() {
                    st.paras.remove(*g);
                }
                fresh_needed = st.paras.is_empty();
            }
            if fresh_needed {
                // `if (!blocks.some(paragraph)) blocks = [...blocks, ...newHfParas(kind)]`
                let fresh = self.new_hf_story(kind);
                if let Some(h) = self.doc.hf.make_mut(part.index())
                    && let Some(p) = fresh.paras.get_arc(0)
                {
                    h.story.paras.push_arc(Arc::clone(p));
                }
            }
            self.dirty_hf.insert(part);
        }
        if matches!(self.ctx, EditContext::HeaderFooter { .. }) {
            let sel = self.selection;
            self.selection = super::context::clamp_selection(self.paras(), sel);
        }
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// The new-part branch of `enterHeaderFooter`: the part and its reference in the section's
    /// references; always written; the document is dirty (outside an undo step).
    fn create_header_footer_part(&mut self, kind: HfKind, section: u32, slot: HfSlot) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.make_section_part(kind, section, slot);
        Ok(Outcome::Done(self.finish()))
    }

    /// The part of `slot` of the section `section` when it has none: a new part (an empty paragraph
    /// in the style), its reference in the section's references and in the effective section the
    /// layout reads (the TS pushes it there too), always written; the document is dirty. `None`:
    /// the new part cannot be made.
    pub(crate) fn make_section_part(&mut self, kind: HfKind, section: u32, slot: HfSlot) -> Option<HfPartId> {
        let sec = self.hf_section_of(HfAt { section, slot });
        if let Some(r) = self.refs_of_section(sec, kind).iter().find(|r| &*r.r#type == slot.as_str()) {
            let key: Str = format!("{}:{}", kind.as_str(), r.rid).into();
            return self.sections.hf_parts.get(&key).copied();
        }
        let target = self.new_hf_target(kind);
        let id = self.create_hf_part(kind, target.clone(), None).ok()?;
        self.dirty_hf.insert(id);
        self.edit_section_refs(sec, |h, f| {
            let list = if kind == HfKind::Header { h } else { f };
            list.push(HfRefEdit { kind: kind_str(kind), r#type: slot.as_str().into(), rid: target.0.clone(), ..HfRefEdit::default() });
        });
        // the effective sections follow now (the TS patches the section object it is editing)
        self.sections.sync(&mut self.doc);
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        let c = self.history.set_dirty(true);
        if let Some(dirty) = c.dirty {
            self.emit(Event::DirtyChange { dirty });
        }
        Some(id)
    }

    /// `getHeaderFooterOptions()` with the layout's answer.
    #[must_use]
    pub fn header_footer_options(&self, at: HfAt) -> HeaderFooterOptions {
        let sec = self.hf_section_of(at);
        let sp = self.body_sect_pr_at(sec);
        let kind = self.hf_edit_kind();
        let num = |x: Option<JsNumber>| x.and_then(JsNumber::as_f64).unwrap_or(720.0);
        HeaderFooterOptions {
            section: sec,
            section_count: self.body_section_count(),
            part: kind.map(|k| (k, at.slot)),
            different_first_page: sp.title_pg == Some(true),
            different_odd_even: self.even_odd_headers(),
            header_distance: num(sp.header_distance_twips),
            footer_distance: num(sp.footer_distance_twips),
            link_to_previous: match kind {
                Some(k) if sec > 0 => !self.refs_of_section(sec, k).iter().any(|r| &*r.r#type == at.slot.as_str()),
                _ => false,
            },
        }
    }

    /// `getPageNumberFormat()`.
    #[must_use]
    pub fn page_number_format(&self, at: HfAt) -> PageNumberFormat {
        let sp: Arc<SectPr> = self.body_sect_pr_at(self.hf_section_of(at));
        PageNumberFormat {
            format: sp.pg_num_fmt.as_deref().unwrap_or("decimal").to_owned(),
            chapter: sp
                .pg_num_chap_style
                .filter(|c| c.truthy())
                .map(|c| (c.to_number(), sp.pg_num_chap_sep.as_deref().unwrap_or("hyphen").to_owned())),
            start: sp.pg_num_start.and_then(JsNumber::as_f64),
        }
    }

    /// The `HfPartId` registered for `kind:rid`.
    #[must_use]
    pub fn hf_part_of_ref(&self, kind: HfKind, rid: &str) -> Option<HfPartId> {
        self.sections.hf_parts.get(format!("{}:{rid}", kind.as_str()).as_str()).copied()
    }

    /// Reads a header/footer part from `xml` as if it came from the file at `path` (the reader with
    /// the document's sources); the signatures of the new nodes join the baseline.
    fn read_hf_part_story(&mut self, path: &str, xml: &str) -> Result<Option<Story>, CommandError> {
        use crate::read::ctx::ReadCtx;
        let limits = crate::limits::DocxLimits::default();
        let opts = crate::read::ReadOptions { sources: false, limits };
        let old_next = self.doc.sources.as_deref().map_or(0, |s| s.next);
        let story = {
            let doc: &mut Document = &mut self.doc;
            let mut ctx = ReadCtx::new(&self.pkg, &opts);
            ctx.ids = std::mem::take(&mut doc.ids);
            ctx.media = std::mem::take(&mut doc.media);
            ctx.sources = doc.sources.take().map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()));
            let r = crate::read::fulldoc::read_header_footer_part(&mut ctx, path, Some(xml));
            doc.ids = ctx.ids;
            doc.media = ctx.media;
            doc.sources = ctx.sources.map(Arc::new);
            r
        };
        let story = story.map_err(|_| CommandError::Position)?;
        if let Some(st) = &story {
            self.baseline_new_nodes(st, old_next);
        }
        Ok(story)
    }

    /// `readHeaderFooterPart(pkg, path, undefined, xml)`: the same reader with no source store (what
    /// the text box templates of D2b-F7 use): the nodes carry no `src`, so the save rebuilds them
    /// from the model.
    pub(crate) fn read_part_story_no_sources(&mut self, path: &str, xml: &str) -> Result<Option<Story>, CommandError> {
        use crate::read::ctx::ReadCtx;
        let limits = crate::limits::DocxLimits::default();
        let opts = crate::read::ReadOptions { sources: false, limits };
        let doc: &mut Document = &mut self.doc;
        let mut ctx = ReadCtx::new(&self.pkg, &opts);
        ctx.ids = std::mem::take(&mut doc.ids);
        ctx.media = std::mem::take(&mut doc.media);
        let r = crate::read::fulldoc::read_header_footer_part(&mut ctx, path, Some(xml));
        doc.ids = ctx.ids;
        doc.media = ctx.media;
        r.map_err(|_| CommandError::Position)
    }

    /// `store.baseline()` for the nodes a read just registered (source keys from `old_next`): the
    /// signatures of the new part's paragraphs and tables.
    fn baseline_new_nodes(&mut self, story: &Story, old_next: u32) {
        let mode = self.save.baseline.mode;
        let media = &self.doc.media;
        let Some(store) = self.doc.sources.as_deref() else { return };
        let mut stack: Vec<&Story> = vec![story];
        while let Some(s) = stack.pop() {
            for p in s.paras.iter_arcs() {
                if let Some(k) = p.props.src
                    && k.0 >= old_next
                    && store.paras.contains_key(&k)
                {
                    let mut sig = crate::write::sig::para_sig(p, Some(media), mode);
                    sig.node = Some(Arc::clone(p));
                    self.save.baseline.paras.insert(k, sig);
                }
            }
            for a in s.tables.iter() {
                let t = &a.table;
                if let Some(k) = t.src
                    && k.0 >= old_next
                    && store.tables.contains_key(&k)
                {
                    self.save.baseline.tables.insert(k, (crate::write::sig::table_sig(t, Some(media), mode), Arc::clone(t)));
                }
                for row in &t.rows {
                    for cell in &row.cells {
                        stack.push(&cell.content);
                    }
                }
            }
        }
    }
}

/// The `Id` attribute values of a relationships part (`/\bId="([^"]+)"/g`).
fn ids_in(rels: &str) -> Vec<String> {
    let b = rels.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = rels.get(i..).and_then(|s| s.find("Id=\"")) {
        let at = i + off;
        let prev_word = at > 0 && b.get(at - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        let start = at + 4;
        let end = rels.get(start..).and_then(|s| s.find('"')).map(|e| start + e);
        match end {
            Some(e) => {
                if !prev_word && e > start {
                    out.push(rels[start..e].to_owned());
                }
                i = e + 1;
            }
            None => break,
        }
    }
    out
}

// ───────────────────────── undo/redo in header/footer edit (D2a-C2) ─────────────────────────

impl Session {
    /// `undo()` taken in header/footer edit, with the layout's answer (D2a-C2): after the state is
    /// restored the TS returns to the same page, and the part the layout shows there decides where
    /// the edit goes (a part the undo took the reference of is made again). `None`: the part edited
    /// before. Isolated here so that the milestone-2 gate can replace it by an oracle method.
    pub fn undo_reentering(&mut self, cx: &mut ExecCx<'_>, reenter: Option<HfReenter>) -> super::super::ChangeSet {
        self.pop_reentering(true, cx, reenter)
    }

    /// `redo()` with the layout's answer, as [`Session::undo_reentering`].
    pub fn redo_reentering(&mut self, cx: &mut ExecCx<'_>, reenter: Option<HfReenter>) -> super::super::ChangeSet {
        self.pop_reentering(false, cx, reenter)
    }

    fn pop_reentering(&mut self, undo: bool, cx: &mut ExecCx<'_>, reenter: Option<HfReenter>) -> super::super::ChangeSet {
        let before = match &self.ctx {
            EditContext::HeaderFooter { part, .. } => Some(*part),
            _ => None,
        };
        // the step to restore, taken in header/footer edit: its part (`hp.k`) and the cursor in it
        let step_hp = {
            let top = if undo { self.history.peek_undo() } else { self.history.peek_redo() };
            top.and_then(|st| match &st.context {
                super::super::state::ContextStep::HeaderFooter { part, selection, .. } => Some((*part, *selection)),
                _ => None,
            })
        };
        let sel_before = self.selection;
        let mut cs = if undo { self.undo(cx) } else { self.redo(cx) };
        let (Some(old), Some(r)) = (before, reenter) else { return cs };
        let EditContext::HeaderFooter { page, saved, .. } = self.ctx.clone() else { return cs };
        self.tx = Default::default();
        let target = match r {
            HfReenter::Part(p) => Some(p),
            HfReenter::Create { kind, section, slot } => self.make_section_part(kind, section, slot),
        };
        match target.filter(|p| self.doc.hf.get(p.index()).is_some()) {
            Some(p) => {
                if p != old {
                    self.ctx = EditContext::HeaderFooter { part: p, page, saved };
                }
                // `if (state.hp && now === state.hp.k) selection = clampSel(state.hp.s) else if (now ===
                // hf.key) selection = clampSel(hf.sel)`; a part entered afresh has the caret at the start
                let want = match step_hp {
                    Some((k, sel)) if k == p => sel,
                    _ if p == old => sel_before,
                    _ => Selection::default(),
                };
                self.selection = super::context::clamp_selection(self.paras(), want);
            }
            None => {
                self.ctx = EditContext::Body;
                self.selection = saved;
                self.emit(Event::HfMode { active: false });
            }
        }
        self.touch_formatting();
        let extra = self.change_set();
        cs.events.extend(extra.events);
        cs.dirty = extra.dirty;
        cs.can_undo = extra.can_undo;
        cs.can_redo = extra.can_redo;
        cs.formatting |= extra.formatting;
        cs.selection = extra.selection;
        cs
    }
}

// ───────────────────────── the parts an unlink left behind ─────────────────────────

impl Session {
    /// Frees the parts no section and no undo/redo step refers to any more (an unlink that was
    /// undone leaves its part in the index, as the TS `hfParts` does; a long script would grow
    /// them without end). The file's own parts stay. The part's slot stays (ids are indices) with
    /// an empty part, its key leaves the index (so its rId and path can be used again: the TS
    /// would number on). Runs when the index doubles, so the scan of the history is amortised.
    fn gc_hf_parts(&mut self) {
        use super::super::state::ContextStep;
        use std::collections::HashSet;
        let mut rids: HashSet<Str> = HashSet::new();
        let mut keep: HashSet<HfPartId> = HashSet::new();
        let mut note = |paras: &crate::pvec::PVec<Paragraph>, body: &Option<super::super::settle::SectionRefs>| {
            for p in paras.iter() {
                if let Some(sb) = p.props.section_break.as_deref() {
                    rids.extend(sb.header_refs.iter().chain(sb.footer_refs.iter()).map(|r| r.rid.clone()));
                }
            }
            if let Some(b) = body {
                rids.extend(b.header_refs.iter().chain(b.footer_refs.iter()).map(|r| r.rid.clone()));
            }
        };
        note(&self.doc.body.paras, &self.sections.body_refs);
        for st in self.history.states() {
            note(&st.body.paras, &st.body_refs);
            keep.extend(st.hf_parts.iter().map(|(p, _)| *p));
            if let ContextStep::HeaderFooter { part, .. } = &st.context {
                keep.insert(*part);
            }
            if let Some(d) = &st.deep_hf {
                keep.extend(d.iter().map(|(p, _)| *p));
            }
        }
        if let EditContext::HeaderFooter { part, .. } = &self.ctx {
            keep.insert(*part);
        }
        let gone: Vec<(Str, HfPartId)> = self
            .sections
            .hf_parts
            .iter()
            .filter(|(key, id)| {
                let rid = key.split_once(':').map_or(&***key, |(_, r)| r);
                id.index() >= self.f4.loaded_parts && !keep.contains(id) && !rids.contains(rid)
            })
            .map(|(k, id)| (k.clone(), *id))
            .collect();
        for (key, id) in gone {
            self.sections.hf_parts.shift_remove(&key);
            self.dirty_hf.remove(&id);
            if let Some(h) = self.doc.hf.make_mut(id.index()) {
                *h = HfPart::default();
            }
        }
        self.f4.gc_at = (self.doc.hf.len() * 2).max(64);
    }
}
