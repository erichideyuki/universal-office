//! D2b family: numbering (F2): `toggleList`, `applyListLibrary` (list-library.ts), `removeList`,
//! `applyMultilevelLibrary`, `applyListLevels`, `setListLevel`, `changeListLevel`,
//! `restartNumbering`, `continueNumbering`, `setNumberingValue`, `defineListLevel`,
//! `defineMultilevelList`, the list styles; `numbering-store.ts`; the Enter/Backspace list hooks
//! of the text family; the save of the numbering part.
//!
//! The TS (`universal-write.ts` 3500–4150, `numbering-store.ts`): the part text is the truth and
//! is edited without loss ([`store`]); the session keeps it in the version store `numbering`
//! (U12), the lists the editor made or changed in [`F2State::num_touched`] (`numTouched`). The
//! regexes are hand matchers ([`rx`]). Decisions outside the TS semantics are in
//! `docs/modules/uo-docx-d2b/F2.md`.

#[rustfmt::skip]
pub(crate) mod library;
pub(crate) mod rx;
pub(crate) mod store;

use std::sync::Arc;

use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{max as js_max, min as js_min, round as js_round};

use super::super::command::{CommandError, Event, Outcome};
use super::super::policy::GuardArg;
use super::super::revisions;
use super::super::state::VersionId;
use super::super::text::{self, DocPosition, DocRange, normalize_range, para_length};
use super::super::{ExecCx, Session};
use super::styles::sheet::{StyleIndex, UiLang, esc};
use crate::error::DocxError;
use crate::js::{JsNumber, Str};
use crate::limits::DocxLimits;
use crate::model::Document;
use crate::model::formatting::{NumLevel, Numbering, StyleSheet};
use crate::model::media::ImageData;
use crate::model::para::{NumberingProps, ParaProps, Paragraph};
use crate::word::obj;
use library::{BULLET_LIBRARY, LibraryList, MULTILEVEL_LIBRARY, NUMBER_LIBRARY};
use store::{ListOpts, NumView};

// ───────────────────────── state ─────────────────────────

/// The state the lists and styles family keeps next to the stores (`numTouched`, `lastBullet`,
/// `lastNumber`, `listBackspaceTrail`, the language of the interface, and the parsed forms of the
/// two part texts, memoised by version).
#[derive(Debug, Default)]
pub struct F2State {
    /// `numTouched`: abstractNum ids the editor created or changed (`w:tentative` on save)
    pub(crate) num_touched: Vec<f64>,
    pub(crate) last_bullet: usize,
    pub(crate) last_number: usize,
    /// `listBackspaceTrail`: -1 = none
    pub(crate) backspace_trail: Option<u32>,
    /// `uiLang(this)`: the language of the interface (default English)
    pub(crate) lang: UiLang,
    parsed: Option<(VersionId, Arc<Numbering>)>,
    /// the picture bullets' images by relationship id (`DocNumbering.images`)
    images: Option<Vec<(Str, ImageData)>>,
    pub(crate) style_idx: Option<(VersionId, Arc<StyleIndex>)>,
    /// wave 7 (A2): the picture bullets added by the editor (`images` with `fresh` of numbering-store.ts): the relationship
    /// id, the bytes and the extension, written at save (`writeImages`)
    pub(crate) fresh_images: Vec<(Str, Arc<[u8]>, Str)>,
}

/// wave 7 (A2): the picture of a picture bullet (`defineListLevel({ picture })`): the bytes, the extension, the natural
/// size in points (`imageSize(data)` × 0.75, 96 × 0.75 without one: the shell measures it) and the title.
#[derive(Debug, Clone, PartialEq)]
pub struct PicBulletIn {
    pub data: Arc<[u8]>,
    pub ext: Str,
    pub w_pt: f64,
    pub h_pt: f64,
    pub name: Str,
}

// ───────────────────────── the public types ─────────────────────────

/// A list level for the dialogs (`ListLevelSpec`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListLevelSpec {
    pub ilvl: f64,
    pub num_fmt: String,
    pub lvl_text: String,
    pub font: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub size_pt: Option<f64>,
    pub color: Option<String>,
    pub start: Option<f64>,
    pub p_style: Option<String>,
    pub ind_left: f64,
    pub hanging: f64,
    pub jc: Option<String>,
    pub suff: Option<String>,
    pub tab_pos: Option<f64>,
    pub is_lgl: bool,
    pub lvl_restart: Option<f64>,
}

/// `Definir Novo Marcador / Formato de Número` (`ListLevelFormat`; the picture bullet is not part
/// of this port: it needs the shell's image size, see the record).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListLevelFormat {
    pub bullet: Option<String>,
    pub num_fmt: Option<String>,
    pub lvl_text: Option<String>,
    pub start: Option<f64>,
    pub align: Option<String>,
    pub font: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub size_pt: Option<f64>,
    pub color: Option<String>,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum ListCmd {
    /// `toggleList('bullet' | 'decimal')`
    ToggleList { bullet: bool },
    /// `applyListLibrary(kind, index)`; `index` `None` = Nenhum
    ApplyListLibrary { bullet: bool, index: Option<usize> },
    /// `removeList()`
    RemoveList,
    /// `applyMultilevelLibrary(index)`
    ApplyMultilevelLibrary { index: Option<usize> },
    /// `applyListLevels(levels)`
    ApplyListLevels { levels: Vec<ListLevelSpec> },
    /// `setListLevel(ilvl)`
    SetListLevel { ilvl: f64 },
    /// `changeListLevel(delta)` (Tab / Shift+Tab); [`Session::change_list_level`] tells whether the
    /// caret was in a list item
    ChangeListLevel { delta: i8 },
    /// `restartNumbering(start?)`
    RestartNumbering { start: Option<f64> },
    /// `continueNumbering()`; [`Session::continue_numbering`] tells whether there was a list
    ContinueNumbering,
    /// `setNumberingValue({ mode, value })`
    SetNumberingValue { new_list: bool, value: Option<f64> },
    /// `defineListLevel(o)`
    DefineListLevel { format: Box<ListLevelFormat> },
    /// `defineMultilevelList(levels, { applyTo })`
    DefineMultilevelList { levels: Vec<ListLevelSpec>, forward: bool },
    /// `createListStyle(name, levels, { apply })`
    CreateListStyle { name: String, levels: Vec<ListLevelSpec>, apply: bool },
    /// `applyListStyle(styleId)`
    ApplyListStyle { style_id: String },
    /// `modifyListStyle(styleId, levels)`
    ModifyListStyle { style_id: String, levels: Vec<ListLevelSpec> },
}

impl ListCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        Some(match self {
            ListCmd::ToggleList { bullet } => ("toggleList", GuardArg::Str(if *bullet { "bullet" } else { "decimal" })),
            ListCmd::ApplyListLibrary { bullet, index } => {
                ("applyListLibrary", GuardArg::ListLibrary { bullet: *bullet, index: index.map(|i| i as f64) })
            }
            ListCmd::ApplyMultilevelLibrary { index } => {
                ("applyMultilevelLibrary", index.map_or(GuardArg::None, |i| GuardArg::Num(i as f64)))
            }
            ListCmd::ApplyListLevels { .. } => ("applyListLevels", GuardArg::None),
            ListCmd::SetListLevel { ilvl } => ("setListLevel", GuardArg::Num(*ilvl)),
            ListCmd::SetNumberingValue { .. } => ("setNumberingValue", GuardArg::None),
            ListCmd::DefineListLevel { .. } => ("defineListLevel", GuardArg::None),
            ListCmd::DefineMultilevelList { .. } => ("defineMultilevelList", GuardArg::None),
            ListCmd::CreateListStyle { .. } => ("createListStyle", GuardArg::None),
            ListCmd::ApplyListStyle { .. } => ("applyListStyle", GuardArg::None),
            ListCmd::ModifyListStyle { .. } => ("modifyListStyle", GuardArg::None),
            ListCmd::RemoveList | ListCmd::ChangeListLevel { .. } | ListCmd::RestartNumbering { .. } | ListCmd::ContinueNumbering => {
                return None;
            }
        })
    }
}

/// The list of a paragraph (`ParaNum`): the direct `w:numPr` (numId 0 = none) or the style's
/// (linked headings); numId -1/-2 = a list without definition (the engine draws a synthetic one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParaNum {
    pub num_id: f64,
    pub ilvl: f64,
    pub from_style: bool,
}

/// `paraNum(p)`.
pub(crate) fn para_num_of(p: &Paragraph, sheet: &StyleSheet, defp: &str) -> Option<ParaNum> {
    if let Some(n) = p.props.numbering.as_ref() {
        // `n.numId ?? (n.kind === 'decimal' ? -2 : -1)`
        let id = match n.num_id {
            Some(JsNumber::Num(x)) => x,
            _ => {
                if &*n.kind == "decimal" {
                    -2.0
                } else {
                    -1.0
                }
            }
        };
        return if uo_jsnum::conv::truthy(id) { Some(ParaNum { num_id: id, ilvl: n.ilvl.to_number(), from_style: false }) } else { None };
    }
    let mut ilvl: Option<f64> = None;
    let mut id: Option<Str> = Some(p.props.style_id.clone().unwrap_or_else(|| defp.into()));
    for _ in 0..20 {
        let Some(cur) = id.take().filter(|s| !s.is_empty()) else { break };
        let def = sheet.styles.get(&cur);
        if let Some(np) = def.and_then(|d| d.p_pr.as_deref()).and_then(|pp| pp.num_pr.as_deref()) {
            if ilvl.is_none() {
                ilvl = np.ilvl.and_then(JsNumber::as_f64);
            }
            if let Some(n) = np.num_id {
                let v = n.to_number();
                return if uo_jsnum::conv::truthy(v) {
                    Some(ParaNum { num_id: v, ilvl: ilvl.unwrap_or(0.0), from_style: true })
                } else {
                    None
                };
            }
        }
        id = def.and_then(|d| d.based_on.clone());
    }
    None
}

fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn clamp_u32(x: usize) -> u32 {
    u32::try_from(x).unwrap_or(u32::MAX)
}

/// The text of the languages of the lists (`ListLocale`).
pub(crate) struct ListLocale {
    lcid: &'static str,
    article: &'static str,
    section: &'static str,
    chapter: &'static str,
    headings: Vec<String>,
}

/// `libraryLevels(def, loc)`.
pub(crate) fn library_levels(def: &LibraryList, loc: &ListLocale) -> Vec<String> {
    def.levels
        .iter()
        .map(|l| {
            let s = l.replace("{LCID}", loc.lcid);
            // `{H1}`…`{H9}`: the styleId of the heading of the document
            let mut out = String::with_capacity(s.len());
            let mut rest = s.as_str();
            while let Some(p) = rest.find("{H") {
                out.push_str(&rest[..p]);
                let tail = &rest[p + 2..];
                let digit = tail.as_bytes().first().copied().filter(u8::is_ascii_digit);
                if let Some(d) = digit
                    && tail.as_bytes().get(1) == Some(&b'}')
                {
                    let n = usize::from(d - b'0');
                    match loc.headings.get(n.wrapping_sub(1)) {
                        Some(h) => out.push_str(h),
                        None => out.push_str(&format!("Heading{n}")),
                    }
                    rest = &tail[2..];
                } else {
                    out.push_str("{H");
                    rest = tail;
                }
            }
            out.push_str(rest);
            out.replace("{ARTICLE}", loc.article).replace("{SECTION}", loc.section).replace("{CHAPTER}", loc.chapter)
        })
        .collect()
}

/// What `listInfo()` gives the commands: the list of the caret's paragraph.
#[derive(Debug, Clone, Copy)]
struct ListInfo {
    num_id: f64,
    ilvl: f64,
    /// -1 = no definition
    abs_id: f64,
    /// `level.start`
    start: Option<f64>,
}

// ───────────────────────── Session: the stores' glue ─────────────────────────

impl Session {
    /// The language of the interface (`uiLang(this)`): English until the shell says otherwise.
    pub fn set_ui_lang(&mut self, lang: UiLang) {
        self.f2.lang = lang;
    }

    /// `numbering.parsed()` (memoised by version): the part as the engine reads it, the picture
    /// bullets with their images.
    pub(crate) fn num_parsed(&mut self) -> Arc<Numbering> {
        let v = self.numbering.version();
        if let Some((pv, n)) = &self.f2.parsed
            && *pv == v
        {
            return Arc::clone(n);
        }
        if self.f2.images.is_none() {
            // the images the document was loaded with (`numberingImages(pkg)`)
            let imgs: Vec<(Str, ImageData)> =
                self.doc.formatting.numbering.pic_bullets.values().filter_map(|pb| Some((pb.rid.clone()?, pb.image.clone()?))).collect();
            self.f2.images = Some(imgs);
        }
        let xml = self.numbering.xml().map_or("", |x| &**x);
        let mut parsed = crate::read::parts::read_numbering_text(xml, &DocxLimits::default()).unwrap_or_default();
        if let Some(imgs) = &self.f2.images {
            for pb in parsed.pic_bullets.values_mut() {
                if let Some(rid) = pb.rid.as_deref()
                    && let Some((_, im)) = imgs.iter().find(|(r, _)| &**r == rid)
                {
                    pb.image = Some(im.clone());
                }
            }
        }
        let n = Arc::new(parsed);
        self.f2.parsed = Some((v, Arc::clone(&n)));
        n
    }

    /// `numbering.addPicBullet(img, wPt, hPt, title)` (wave 7, A2): the store's new `w:numPicBullet`; the picture joins the
    /// numbering's images (the parsed numbering attaches it; the save writes it). Answers the `w:numPicBulletId`.
    fn add_pic_bullet(&mut self, pb: &PicBulletIn) -> f64 {
        let _ = self.num_parsed();
        let imgs: Vec<String> = self.f2.images.as_ref().map(|v| v.iter().map(|(r, _)| r.to_string()).collect()).unwrap_or_default();
        let fresh: Vec<String> = self.f2.fresh_images.iter().map(|(r, _, _)| r.to_string()).collect();
        let taken = |r: &str| imgs.iter().any(|x| x == r) || fresh.iter().any(|x| x == r);
        let (id, rid) = store::add_pic_bullet(&mut self.numbering, &taken, pb.w_pt, pb.h_pt, &pb.name);
        let media = self.doc.media.add(&pb.data);
        let image = ImageData { data: crate::model::media::MediaRef(media), ext: pb.ext.clone(), ..ImageData::default() };
        self.f2.images.get_or_insert_with(Vec::new).push((rid.as_str().into(), image));
        self.f2.fresh_images.push((rid.as_str().into(), Arc::clone(&pb.data), pb.ext.clone()));
        self.f2.parsed = None;
        id
    }

    /// `syncNumbering()`: the engine uses the current numbering.
    pub(crate) fn sync_numbering(&mut self) {
        let n = self.num_parsed();
        let f = Arc::make_mut(&mut self.doc.formatting);
        f.numbering = (*n).clone();
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
    }

    /// `styleNum(styleId)`: the numId of a numbering style (the style sheet in sync with the store).
    fn style_num_of(doc: &Document, id: &str) -> Option<f64> {
        doc.formatting
            .styles
            .styles
            .get(id)
            .and_then(|s| s.p_pr.as_deref())
            .and_then(|p| p.num_pr.as_deref())
            .and_then(|n| n.num_id)
            .map(JsNumber::to_number)
    }

    /// `abstractOf(numId)`.
    pub(crate) fn abstract_of(&mut self, num_id: f64) -> Option<f64> {
        let parsed = self.num_parsed();
        let doc = &self.doc;
        let f = |s: &str| Session::style_num_of(doc, s);
        NumView { parsed: &parsed, style_num: &f }.abstract_of(num_id)
    }

    /// `numbering.level(numId, ilvl)`, cloned.
    pub(crate) fn num_level(&mut self, num_id: f64, ilvl: f64) -> Option<NumLevel> {
        let parsed = self.num_parsed();
        let doc = &self.doc;
        let f = |s: &str| Session::style_num_of(doc, s);
        NumView { parsed: &parsed, style_num: &f }.level(num_id, ilvl).cloned()
    }

    /// `paraNum(paragraphs[i])`.
    pub(crate) fn para_num_at(&mut self, i: usize) -> Option<ParaNum> {
        let defp = self.default_paragraph();
        let p = self.paras()?.get(i)?;
        para_num_of(p, &self.doc.formatting.styles, &defp)
    }

    /// `paraNumId(p)`.
    fn para_num_id_at(&mut self, i: usize) -> f64 {
        self.para_num_at(i).map_or(0.0, |n| n.num_id)
    }

    /// `selectedParas()`: the paragraphs of the selection, ascending (the extra ranges of a
    /// multi-selection are the find family's, F9).
    fn selected_paras(&self) -> Vec<usize> {
        let DocRange { start, end } = normalize_range(self.range());
        (start.para as usize..=end.para as usize).collect()
    }

    /// Whether the selection lies inside the context's paragraphs (D2a-T1: nothing changes
    /// otherwise).
    fn selection_in_range(&self) -> bool {
        let n = self.paras().map_or(0, |p| p.len());
        let s = self.selection;
        (s.anchor.para as usize) < n && (s.focus.para as usize) < n
    }

    /// The common gate of the model-changing commands: a story, in range, not locked.
    pub(crate) fn can_edit(&self) -> bool {
        self.paras().is_some() && !self.locked() && self.selection_in_range()
    }

    /// `applyPara(range, patch)` for one paragraph (`setParaAt`): tracked changes become
    /// `w:pPrChange`.
    pub(crate) fn set_para_at(&mut self, pi: usize, patch: &ParaProps) -> Result<(), CommandError> {
        let at = DocPosition { para: clamp_u32(pi), offset: 0 };
        self.apply_para_patch(DocRange { start: at, end: at }, patch)
    }

    /// `applyPara(range, patch)` (the extra ranges of a multi-selection are the find family's).
    pub(crate) fn apply_para_patch(&mut self, range: DocRange, patch: &ParaProps) -> Result<(), CommandError> {
        if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            revisions::track_apply_para_props(paras, range, patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            text::apply_para_props(paras, range, patch)?;
        }
        let r = normalize_range(range);
        for i in r.start.para..=r.end.para {
            self.touch_para(i as usize);
        }
        Ok(())
    }

    /// The policy guard of a TS method called from inside another (the wrapper of the prototype
    /// also guards those calls): `false` = refused, with `uw-blocked`.
    pub(crate) fn inner_guard(&mut self, method: &'static str, arg: GuardArg<'_>) -> bool {
        let allowed = match &self.policy {
            Some((_, rp)) => super::super::policy::guard_allows(rp, method, arg, self),
            None => true,
        };
        if !allowed {
            self.emit(Event::Blocked { command: method.into() });
        }
        allowed
    }

    /// `resolvedFormattingPolicy()?.lists`.
    fn policy_lists(&self) -> Option<super::super::policy::ResolvedLists> {
        self.policy.as_ref().and_then(|(_, rp)| rp.lists)
    }

    /// `listLocale()`.
    pub(crate) fn list_locale(&mut self) -> ListLocale {
        let lang = self.f2.lang;
        let idx = self.style_index();
        let all = super::styles::xform::list_styles_all(&idx, lang);
        let headings = (1..=9)
            .map(|n| {
                let want = format!("heading {n}");
                all.iter().find(|s| s.raw_name.to_lowercase() == want).map_or_else(|| format!("Heading{n}"), |s| s.id.clone())
            })
            .collect();
        match lang {
            UiLang::Pt => ListLocale { lcid: "0416", article: "Artigo", section: "Seção", chapter: "Capítulo", headings },
            UiLang::En => ListLocale { lcid: "0409", article: "Article", section: "Section", chapter: "Chapter", headings },
            UiLang::Es => ListLocale { lcid: "0C0A", article: "Artículo", section: "Sección", chapter: "Capítulo", headings },
        }
    }

    /// `builtinIdOf(name)`: the styleId of a built-in by its English name (defined or latent),
    /// without defining it.
    pub(crate) fn builtin_id_of(&mut self, name: &str) -> Option<String> {
        let idx = self.style_index();
        let lang = self.f2.lang;
        super::styles::xform::list_styles_all(&idx, lang).into_iter().find(|s| s.raw_name.to_lowercase() == name).map(|s| s.id)
    }

    /// `useBuiltinStyle(name)`: the built-in becomes defined and visible in the style sheet.
    fn use_builtin_style(&mut self, name: &str) -> Option<String> {
        // `this.stylesEditable() ? … : undefined` (3561; wave 7, A1)
        if !self.styles_editable() {
            return None;
        }
        self.builtin_style_id(name).ok().map(|s| s.to_string())
    }

    /// `newList(levels, multiLevelType, tmpl)`.
    fn new_list(&mut self, levels: &[String], multi_level_type: &str, tmpl: Option<&str>) -> f64 {
        let o = ListOpts { multi_level_type, tmpl, ..ListOpts::default() };
        let num_id = store::add_list(&mut self.numbering, &mut *self.env, levels, &o);
        if let Some(abs) = self.abstract_of(num_id) {
            self.touched(abs);
        }
        num_id
    }

    /// `numTouched.add(absId)`.
    fn touched(&mut self, abs: f64) {
        if !self.f2.num_touched.iter().any(|a| same_value_zero(*a, abs)) {
            self.f2.num_touched.push(abs);
        }
    }

    /// `putList(paras, numId, kind)`: the paragraphs join the list (at the level they had in a
    /// list, else 0); Normal → List Paragraph (the other styles stay, as in Word).
    fn put_list(&mut self, paras: &[usize], num_id: f64, bullet: bool) -> Result<(), CommandError> {
        let lp = self.use_builtin_style("list paragraph");
        let def = self.default_paragraph();
        for &pi in paras {
            let ilvl = self.para_num_at(pi).map_or(0.0, |n| n.ilvl);
            let style_id = self.paras().and_then(|ps| ps.get(pi)).and_then(|p| p.props.style_id.clone());
            let mut patch = ParaProps::default();
            obj::set(&mut patch, "numbering", |p| {
                p.numbering = Some(NumberingProps {
                    kind: if bullet { "bullet" } else { "decimal" }.into(),
                    ilvl: JsNumber::Num(ilvl),
                    num_id: Some(JsNumber::Num(num_id)),
                    ..NumberingProps::default()
                });
            });
            if let Some(lp) = &lp
                && style_id.as_deref().is_none_or(|s| s.is_empty() || s == def)
            {
                obj::set(&mut patch, "styleId", |p| p.style_id = Some(lp.as_str().into()));
            }
            self.set_para_at(pi, &patch)?;
        }
        Ok(())
    }

    /// `styleNumbered(styleId)`: the paragraph style carries a numbering (`w:numPr` in the style:
    /// linked headings).
    fn style_numbered(&self, style_id: Option<&str>) -> bool {
        let (Some(id), Some(xml)) = (style_id.filter(|s| !s.is_empty()), self.styles.xml()) else { return false };
        let Some(raw) = super::styles::sheet::style_raw(xml, id) else { return false };
        numbered_raw(&raw)
    }

    /// `listInfo()` of the caret's paragraph.
    fn list_info(&mut self) -> Option<ListInfo> {
        let pi = self.selection.focus.para as usize;
        let n = self.para_num_at(pi)?;
        let abs = if n.num_id > 0.0 { self.abstract_of(n.num_id) } else { None };
        let lvl = if n.num_id > 0.0 { self.num_level(n.num_id, n.ilvl) } else { None };
        // `listLevelSpec(l).start`: `l.start ?? (l.numFmt === 'bullet' ? 1 : 0)`
        let start = lvl.map(|l| match l.start {
            Some(s) => s.to_number(),
            None => {
                if l.num_fmt.as_deref() == Some("bullet") {
                    1.0
                } else {
                    0.0
                }
            }
        });
        Some(ListInfo { num_id: n.num_id, ilvl: n.ilvl, abs_id: abs.unwrap_or(-1.0), start })
    }

    // ───────────────────────── the commands ─────────────────────────

    pub(crate) fn exec_lists(&mut self, cmd: ListCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            ListCmd::ToggleList { bullet } => self.toggle_list(bullet)?,
            ListCmd::ApplyListLibrary { bullet, index } => self.apply_list_library(bullet, index)?,
            ListCmd::RemoveList => self.remove_list()?,
            ListCmd::ApplyMultilevelLibrary { index } => self.apply_multilevel_library(index)?,
            ListCmd::ApplyListLevels { levels } => self.apply_list_levels(&levels)?,
            ListCmd::SetListLevel { ilvl } => self.set_list_level(ilvl)?,
            ListCmd::ChangeListLevel { delta } => {
                self.change_list_level(i32::from(delta))?;
            }
            ListCmd::RestartNumbering { start } => self.restart_numbering(start)?,
            ListCmd::ContinueNumbering => {
                self.continue_numbering()?;
            }
            ListCmd::SetNumberingValue { new_list, value } => self.set_numbering_value(new_list, value)?,
            ListCmd::DefineListLevel { format } => self.define_list_level(&format)?,
            ListCmd::DefineMultilevelList { levels, forward } => self.define_multilevel_list(&levels, forward)?,
            ListCmd::CreateListStyle { name, levels, apply } => {
                self.create_list_style(&name, &levels, apply)?;
            }
            ListCmd::ApplyListStyle { style_id } => self.apply_list_style(&style_id)?,
            ListCmd::ModifyListStyle { style_id, levels } => self.modify_list_style(&style_id, &levels)?,
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `toggleList(kind)`: with the last item of the library used; if the whole selection is
    /// already a list of that kind, takes it off.
    pub(crate) fn toggle_list(&mut self, bullet: bool) -> Result<(), CommandError> {
        if self.paras().is_none() || !self.selection_in_range() {
            return Ok(());
        }
        let mut all = true;
        for pi in self.selected_paras() {
            let Some(n) = self.para_num_at(pi) else {
                all = false;
                break;
            };
            let l = if n.num_id > 0.0 { self.num_level(n.num_id, n.ilvl) } else { None };
            let is_bullet = match &l {
                Some(l) => l.num_fmt.as_deref() == Some("bullet"),
                None => n.num_id == -1.0,
            };
            if is_bullet != bullet {
                all = false;
                break;
            }
        }
        if all {
            self.remove_list()
        } else if self.policy_lists().is_some() {
            self.apply_list_library_inner(bullet, Some(0))
        } else {
            let idx = if bullet { self.f2.last_bullet } else { self.f2.last_number };
            self.apply_list_library_inner(bullet, Some(idx))
        }
    }

    fn apply_list_library(&mut self, bullet: bool, index: Option<usize>) -> Result<(), CommandError> {
        self.apply_list_library_inner(bullet, index)
    }

    /// `applyListLibrary(kind, index)`: an item of the Bullets or Numbering library for the
    /// selection (see the TS comment for Word's rules).
    pub(crate) fn apply_list_library_inner(&mut self, bullet: bool, index: Option<usize>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        let Some(index) = index else { return self.remove_list() };
        let lib = if bullet { BULLET_LIBRARY } else { NUMBER_LIBRARY };
        let Some(def) = lib.get(index) else { return Ok(()) };
        if bullet {
            self.f2.last_bullet = index;
        } else {
            self.f2.last_number = index;
        }
        let loc = self.list_locale();
        let levels = library_levels(def, &loc);
        self.begin_step(false, &[]);
        let paras = self.selected_paras();
        let mut ids: Vec<f64> = Vec::new();
        for &pi in &paras {
            let id = self.para_num_id_at(pi);
            if !ids.iter().any(|x| same_value_zero(*x, id)) {
                ids.push(id);
            }
        }
        let one = if ids.len() == 1 { ids[0] } else { 0.0 };
        let abs = if one > 0.0 { self.abstract_of(one) } else { None };
        let first = levels.first().cloned().unwrap_or_default();
        if let Some(abs) = abs {
            // the selection is in a list: its levels change format (the whole list); the
            // indentation is the one the level has there
            let mut ilvls: Vec<f64> = Vec::new();
            for &pi in &paras {
                let il = self.para_num_at(pi).map_or(0.0, |n| n.ilvl);
                if !ilvls.iter().any(|x| same_value_zero(*x, il)) {
                    ilvls.push(il);
                }
            }
            for ilvl in ilvls {
                let cur = store::level_xml(&self.numbering, abs, ilvl);
                let mut lvl = store::replace_ilvl(&first, ilvl);
                if let Some(ind) = cur.as_deref().and_then(store::ind_element) {
                    lvl = store::replace_ind(&lvl, ind);
                }
                store::set_level(&mut self.numbering, abs, ilvl, &lvl);
            }
            self.touched(abs);
        } else {
            // the closest list before (even with text between) continues if it has the same format
            let mut an: Option<ParaNum> = None;
            let mut k = paras.first().copied().unwrap_or(0);
            while k > 0 && an.is_none() {
                k -= 1;
                an = self.para_num_at(k);
            }
            let mut num_id = 0.0;
            if let Some(an) = an
                && an.num_id > 0.0
                && !an.from_style
            {
                let a = self.num_level(an.num_id, an.ilvl);
                let b = parse_level(&first);
                if store::same_level_format(a.as_ref(), b.as_ref()) {
                    num_id = an.num_id;
                }
            }
            if !uo_jsnum::conv::truthy(num_id) {
                let tmpl = def.tmpl.map(|t| t.replacen("{LCID}", loc.lcid, 1));
                num_id = self.new_list(&levels, def.multi_level_type, tmpl.as_deref());
            }
            self.put_list(&paras, num_id, bullet)?;
        }
        self.sync_numbering();
        Ok(())
    }

    /// `removeList()`: takes the list off the paragraphs of the selection (the number goes and
    /// List Paragraph goes back to Normal; a numbered style (linked heading) gets numId 0).
    pub(crate) fn remove_list(&mut self) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        let lp = self.builtin_id_of("list paragraph");
        for pi in self.selected_paras() {
            let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { continue };
            let sid = p.props.style_id.clone();
            let has_num = p.props.numbering.is_some();
            let styled = self.style_numbered(sid.as_deref());
            if !has_num && !styled {
                continue;
            }
            let mut patch = ParaProps::default();
            if styled {
                obj::set(&mut patch, "numbering", |p| {
                    p.numbering = Some(NumberingProps {
                        kind: "decimal".into(),
                        ilvl: JsNumber::Num(0.0),
                        num_id: Some(JsNumber::Num(0.0)),
                        ..NumberingProps::default()
                    });
                });
            } else {
                obj::set_undefined(&mut patch, "numbering", |p| p.numbering = None);
            }
            if let Some(lp) = &lp
                && sid.as_deref() == Some(lp.as_str())
            {
                obj::set_undefined(&mut patch, "styleId", |p| p.style_id = None);
            }
            self.set_para_at(pi, &patch)?;
        }
        Ok(())
    }

    /// `applyMultilevelLibrary(index)`.
    fn apply_multilevel_library(&mut self, index: Option<usize>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        let Some(index) = index else { return self.remove_list() };
        let Some(def) = MULTILEVEL_LIBRARY.get(index) else { return Ok(()) };
        let loc = self.list_locale();
        let tmpl = def.tmpl.map(|t| t.replacen("{LCID}", loc.lcid, 1));
        self.apply_levels_xml(&library_levels(def, &loc), def.multi_level_type, tmpl.as_deref(), def.linked)
    }

    /// `applyListLevels(levels)`.
    fn apply_list_levels(&mut self, levels: &[ListLevelSpec]) -> Result<(), CommandError> {
        if !self.can_edit() || levels.is_empty() {
            return Ok(());
        }
        let linked = levels.iter().any(|l| l.p_style.as_deref().is_some_and(|s| !s.is_empty()));
        let multi = linked || levels.iter().any(|l| count_level_refs(&l.lvl_text) > 1);
        let xs: Vec<String> = levels.iter().map(level_xml_of).collect();
        self.apply_levels_xml(&xs, if multi { "multilevel" } else { "hybridMultilevel" }, None, linked)
    }

    fn apply_levels_xml(
        &mut self,
        levels: &[String],
        multi_level_type: &str,
        tmpl: Option<&str>,
        linked: bool,
    ) -> Result<(), CommandError> {
        let loc = self.list_locale();
        self.begin_step(false, &[]);
        let paras = self.selected_paras();
        let cur = self.list_info();
        let num_id = match cur {
            Some(c) if c.abs_id >= 0.0 && !linked => {
                store::set_levels(&mut self.numbering, c.abs_id, levels, Some(multi_level_type), tmpl);
                self.touched(c.abs_id);
                c.num_id
            }
            _ => self.new_list(levels, multi_level_type, tmpl),
        };
        if linked {
            for n in 1..=9 {
                self.link_heading_style(n, num_id);
            }
            for &pi in &paras {
                let style = self.paras().and_then(|ps| ps.get(pi)).and_then(|p| p.props.style_id.clone());
                let lvl = match self.para_num_at(pi) {
                    Some(n) => n.ilvl,
                    None => self.heading_level(style.as_deref()).unwrap_or(0.0),
                };
                // the number comes from the heading style (the paragraph keeps only the style)
                let mut patch = ParaProps::default();
                obj::set_undefined(&mut patch, "numbering", |p| p.numbering = None);
                let hid = js_round(lvl) as usize;
                let heading = loc.headings.get(hid).cloned().unwrap_or_else(|| format!("Heading{}", hid + 1));
                obj::set(&mut patch, "styleId", |p| p.style_id = Some(heading.as_str().into()));
                self.set_para_at(pi, &patch)?;
            }
        } else {
            let bullet = self.num_level(num_id, 0.0).is_some_and(|l| l.num_fmt.as_deref() == Some("bullet"));
            self.put_list(&paras, num_id, bullet)?;
        }
        self.sync_numbering();
        Ok(())
    }

    /// `headingLevel(styleId)`: the level (0-8) of a heading style by its built-in name.
    fn heading_level(&mut self, style_id: Option<&str>) -> Option<f64> {
        let id = style_id.filter(|s| !s.is_empty())?;
        let idx = self.style_index();
        let lang = self.f2.lang;
        let s = super::styles::xform::list_styles_all(&idx, lang).into_iter().find(|x| x.id == id)?;
        let lower = s.raw_name.to_lowercase();
        let d = lower.strip_prefix("heading ")?;
        (d.len() == 1 && d.as_bytes()[0].is_ascii_digit()).then(|| f64::from(d.as_bytes()[0] - b'0') - 1.0)
    }

    /// `linkHeadingStyle(n, numId)`: the list goes on the style Heading n (`w:numPr` in the style's
    /// `pPr`, Heading 1 with only the numId), as Word writes it.
    fn link_heading_style(&mut self, n: u32, num_id: f64) {
        let Some(id) = self.use_builtin_style(&format!("heading {n}")) else { return };
        let Some(xml) = self.styles.xml().map(|x| x.to_string()) else { return };
        let Some(raw) = super::styles::sheet::style_raw(&xml, &id) else { return };
        let nid = to_js_string(num_id);
        let num_pr = if n == 1 {
            format!("<w:numPr><w:numId w:val=\"{nid}\"/></w:numPr>")
        } else {
            format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{nid}\"/></w:numPr>", n - 1)
        };
        let next = put_num_pr(&raw, &num_pr);
        if next != raw
            && let Some(x) = super::styles::sheet::put_style(&xml, &id, &next)
        {
            self.commit_styles(x);
        }
    }

    /// `setListLevel(ilvl)`.
    pub(crate) fn set_list_level(&mut self, ilvl: f64) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        for pi in self.selected_paras() {
            self.para_list_level(pi, ilvl)?;
        }
        Ok(())
    }

    fn para_list_level(&mut self, pi: usize, ilvl: f64) -> Result<(), CommandError> {
        let Some(n) = self.para_num_at(pi) else { return Ok(()) };
        let lv = js_max(0.0, js_min(8.0, js_round(ilvl)));
        let p_style = if n.num_id > 0.0 { self.num_level(n.num_id, lv).and_then(|l| l.p_style).filter(|s| !s.is_empty()) } else { None };
        // a level tied to a style (Heading N): the paragraph changes style; numbered by the style,
        // it keeps only that
        if n.from_style {
            if let Some(ps) = p_style {
                let mut patch = ParaProps::default();
                obj::set(&mut patch, "styleId", |p| p.style_id = Some(ps));
                self.set_para_at(pi, &patch)?;
            }
            return Ok(());
        }
        let Some(cur) = self.paras().and_then(|ps| ps.get(pi)).and_then(|p| p.props.numbering.clone()) else { return Ok(()) };
        let mut num = cur;
        obj::set(&mut num, "ilvl", |x| x.ilvl = JsNumber::Num(lv));
        let mut patch = ParaProps::default();
        obj::set(&mut patch, "numbering", |p| p.numbering = Some(num));
        if let Some(ps) = p_style {
            obj::set(&mut patch, "styleId", |p| p.style_id = Some(ps));
        }
        self.set_para_at(pi, &patch)
    }

    /// `changeListLevel(delta)`: Tab / Shift+Tab on a list item. On the first item of a list at
    /// level 0 the Tab moves the WHOLE LIST to the next default tab stop. `false`: the caret is
    /// not in an item.
    pub fn change_list_level(&mut self, delta: i32) -> Result<bool, CommandError> {
        if !self.can_edit() {
            return Ok(false);
        }
        let mut paras = Vec::new();
        for pi in self.selected_paras() {
            if self.para_num_id_at(pi) != 0.0 {
                paras.push(pi);
            }
        }
        let Some(&pi) = paras.first() else { return Ok(false) };
        let Some(n) = self.para_num_at(pi) else { return Ok(false) };
        let id = n.num_id;
        let abs = if id > 0.0 { self.abstract_of(id) } else { None };
        let mut first_of_list = true;
        for q in 0..pi {
            if self.para_num_id_at(q) == id {
                first_of_list = false;
                break;
            }
        }
        // list policy: only the allowed levels, and the first item changes level (the list does not move)
        let lp = self.policy_lists();
        if let Some(l) = lp
            && delta > 0
        {
            for &k in &paras {
                if self.para_num_at(k).is_some_and(|x| x.ilvl + 1.0 >= l.levels) {
                    return Ok(true);
                }
            }
        }
        self.begin_step(false, &[]);
        if delta > 0 && first_of_list && abs.is_some() && n.ilvl == 0.0 && lp.is_none() {
            let abs = abs.unwrap_or(0.0);
            let lvl0 = self.num_level(id, 0.0);
            let (left, hanging) = store::level_indent(lvl0.as_ref());
            let pos = left - hanging;
            let tab = self.default_tab_stop();
            let shift = (uo_jsnum::math::floor(pos / tab) + 1.0) * tab - pos;
            for k in 0..=8 {
                let Some(raw) = store::level_xml(&self.numbering, abs, f64::from(k)) else { continue };
                let next = shift_ind_left(&raw, shift);
                store::set_level(&mut self.numbering, abs, f64::from(k), &next);
            }
            self.touched(abs);
            self.sync_numbering();
        } else {
            for &k in &paras {
                let il = self.para_num_at(k).map_or(0.0, |x| x.ilvl);
                self.para_list_level(k, il + f64::from(delta))?;
            }
        }
        Ok(true)
    }

    /// `defaultTabStop()`: the one the dialog set, the file's, or 708. Wave 7c (W, item 2, fix round 1): a file whose
    /// settings give no `w:defaultTabStop` answers Word's 708 (the value the layout uses; the frozen TS's model says 720,
    /// kept by a strict replay's `OpenOptions::ts_missing_settings`).
    pub(crate) fn default_tab_stop(&self) -> f64 {
        self.settings.default_tab.unwrap_or_else(|| {
            if let Some(w) = self.doc.word_default_tab_fallback() {
                return w;
            }
            let t = self.doc.formatting.settings.default_tab_stop;
            t.as_f64().unwrap_or(708.0)
        })
    }

    /// `restartNumbering(start?)`: a new abstractNum (a copy with a new nsid) and a new `w:num` for
    /// the item and the ones after it in the list (measured: NumberingRestart).
    pub(crate) fn restart_numbering(&mut self, start: Option<f64>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        let pi = self.selection.focus.para as usize;
        let Some(n) = self.para_num_at(pi) else { return Ok(()) };
        let id = n.num_id;
        let Some(abs) = (if id > 0.0 { self.abstract_of(id) } else { None }) else { return Ok(()) };
        self.begin_step(false, &[]);
        let Some(copy) = store::copy_abstract(&mut self.numbering, &mut *self.env, abs) else {
            self.discard_step();
            return Ok(());
        };
        if let Some(start) = start {
            // the start value: the level gets its `w:start`
            if let Some(raw) = store::level_xml(&self.numbering, copy, n.ilvl) {
                let s = format!("<w:start w:val=\"{}\"/>", to_js_string(js_max(0.0, js_round(start))));
                let next = set_level_start(&raw, &s);
                store::set_level(&mut self.numbering, copy, n.ilvl, &next);
            }
        }
        self.touched(copy);
        let new_num = store::add_num(&mut self.numbering, copy, "");
        self.retarget_list(pi, id, new_num)?;
        self.sync_numbering();
        Ok(())
    }

    /// `continueNumbering()`: the item and the ones after join the previous list of the same
    /// format. `false`: no compatible list before.
    pub fn continue_numbering(&mut self) -> Result<bool, CommandError> {
        if !self.can_edit() {
            return Ok(false);
        }
        let pi = self.selection.focus.para as usize;
        let target = self.previous_list(pi);
        if !uo_jsnum::conv::truthy(target) {
            return Ok(false);
        }
        self.begin_step(false, &[]);
        let cur = self.para_num_id_at(pi);
        self.retarget_list(pi, cur, target)?;
        Ok(true)
    }

    /// `previousList(pi)`: the list before `pi` with the same format as the item's level; 0 if none.
    pub(crate) fn previous_list(&mut self, pi: usize) -> f64 {
        let Some(n) = self.para_num_at(pi).filter(|n| n.num_id > 0.0) else { return 0.0 };
        let lvl = self.num_level(n.num_id, n.ilvl);
        for k in (0..pi).rev() {
            let qid = self.para_num_id_at(k);
            if qid > 0.0 && qid != n.num_id {
                let ql = self.num_level(qid, n.ilvl);
                if store::same_level_format(ql.as_ref(), lvl.as_ref()) {
                    return qid;
                }
            }
        }
        0.0
    }

    /// `retargetList(from, oldId, newId)`: the items from `from` of list `old_id` move to `new_id`
    /// (a style-numbered one gets its own `w:numPr`).
    fn retarget_list(&mut self, from: usize, old_id: f64, new_id: f64) -> Result<(), CommandError> {
        let n = self.paras().map_or(0, |p| p.len());
        for k in from..n {
            let Some(pn) = self.para_num_at(k) else { continue };
            if pn.num_id != old_id {
                continue;
            }
            let kind = self
                .paras()
                .and_then(|ps| ps.get(k))
                .and_then(|q| q.props.numbering.as_ref().map(|x| x.kind.clone()))
                .unwrap_or_else(|| "decimal".into());
            let mut patch = ParaProps::default();
            obj::set(&mut patch, "numbering", |p| {
                p.numbering = Some(NumberingProps {
                    kind,
                    ilvl: JsNumber::Num(pn.ilvl),
                    num_id: Some(JsNumber::Num(new_id)),
                    ..NumberingProps::default()
                });
            });
            self.set_para_at(k, &patch)?;
        }
        Ok(())
    }

    /// `setNumberingValue({ mode, value })`.
    pub(crate) fn set_numbering_value(&mut self, new_list: bool, value: Option<f64>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        if new_list {
            return self.restart_numbering(Some(value.unwrap_or(1.0)));
        }
        let pi = self.selection.focus.para as usize;
        let Some(n) = self.para_num_at(pi).filter(|n| n.num_id > 0.0) else { return Ok(()) };
        let id = n.num_id;
        let target = self.previous_list(pi);
        self.begin_step(false, &[]);
        let mut num_id = if uo_jsnum::conv::truthy(target) { target } else { id };
        if let Some(v) = value {
            let Some(abs) = self.abstract_of(num_id) else {
                // the TS returns here, after the step was taken
                return Ok(());
            };
            let ov = format!(
                "<w:lvlOverride w:ilvl=\"{}\"><w:startOverride w:val=\"{}\"/></w:lvlOverride>",
                to_js_string(n.ilvl),
                to_js_string(js_max(0.0, js_round(v)))
            );
            num_id = store::add_num(&mut self.numbering, abs, &ov);
        }
        self.retarget_list(pi, id, num_id)?;
        self.sync_numbering();
        Ok(())
    }

    /// `defineListLevel(o)`: the item's level (or a new list) with the bullet character, or the
    /// number format and text, its formatting and alignment. No `w:tplc` (a format of its own).
    pub(crate) fn define_list_level(&mut self, o: &ListLevelFormat) -> Result<(), CommandError> {
        self.define_list_level_with(o, None)
    }

    /// `defineListLevel(o)` with `o.picture` (wave 7, A2): a picture bullet keeps the Symbol character in reserve (as Word
    /// writes it) and gets a new `w:numPicBullet` (`addPicBullet`) and the level's `w:lvlPicBulletId`.
    pub(crate) fn define_list_level_with(&mut self, o: &ListLevelFormat, pic: Option<&PicBulletIn>) -> Result<(), CommandError> {
        if !self.can_edit() || (o.bullet.is_none() && o.num_fmt.as_deref().is_none_or(str::is_empty) && pic.is_none()) {
            return Ok(());
        }
        let picked;
        let o = if pic.is_some() && o.bullet.is_none() {
            picked = ListLevelFormat {
                bullet: Some("\u{F0B7}".to_owned()),
                font: Some(o.font.clone().unwrap_or_else(|| "Symbol".to_owned())),
                ..o.clone()
            };
            &picked
        } else {
            o
        };
        let had = self.list_info();
        if had.is_none_or(|h| h.abs_id < 0.0) {
            self.apply_list_library_inner(o.bullet.is_some(), Some(0))?;
        } else {
            self.begin_step(false, &[]);
        }
        let Some(cur) = self.list_info().filter(|c| c.abs_id >= 0.0) else { return Ok(()) };
        let raw = store::level_xml(&self.numbering, cur.abs_id, cur.ilvl).unwrap_or_default();
        let ind = store::ind_element(&raw)
            .map_or_else(|| format!("<w:ind w:left=\"{}\" w:hanging=\"360\"/>", to_js_string(720.0 * (cur.ilvl + 1.0))), str::to_owned);
        let bullet = o.bullet.is_some();
        let start = if bullet { 1.0 } else { js_max(0.0, js_round(o.start.or(cur.start).unwrap_or(1.0))) };
        let num_fmt = if bullet { "bullet".to_owned() } else { esc(o.num_fmt.as_deref().unwrap_or("undefined")) };
        let lvl_text = match (&o.bullet, &o.lvl_text) {
            (Some(b), _) => esc(b),
            (None, Some(t)) => esc(t),
            (None, None) => esc(&format!("%{}.", to_js_string(cur.ilvl + 1.0))),
        };
        // a picture bullet: a new w:numPicBullet (the picture's natural size) and the level's w:lvlPicBulletId
        let pic_id = pic.map(|pb| self.add_pic_bullet(pb));
        let lvl = format!(
            "<w:lvl w:ilvl=\"{}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{num_fmt}\"/><w:lvlText w:val=\"{lvl_text}\"/>{}<w:lvlJc w:val=\"{}\"/><w:pPr>{ind}</w:pPr>{}</w:lvl>",
            to_js_string(cur.ilvl),
            to_js_string(start),
            pic_id.map_or_else(String::new, |id| format!("<w:lvlPicBulletId w:val=\"{}\"/>", to_js_string(id))),
            o.align.as_deref().unwrap_or("left"),
            level_rpr_xml(o.font.as_deref(), o.bold, o.italic, o.size_pt, o.color.as_deref()),
        );
        store::set_level(&mut self.numbering, cur.abs_id, cur.ilvl, &lvl);
        self.touched(cur.abs_id);
        self.sync_numbering();
        Ok(())
    }

    /// `defineMultilevelList(levels, { applyTo })`.
    pub(crate) fn define_multilevel_list(&mut self, levels: &[ListLevelSpec], forward: bool) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        let had = self.list_info();
        if had.is_none_or(|h| h.abs_id < 0.0) {
            self.apply_multilevel_library(Some(0))?;
        } else {
            self.begin_step(false, &[]);
        }
        let Some(cur) = self.list_info().filter(|c| c.abs_id >= 0.0) else { return Ok(()) };
        let (mut abs, mut num_id) = (cur.abs_id, cur.num_id);
        if forward && had.is_some_and(|h| h.abs_id >= 0.0) {
            let Some(copy) = store::copy_abstract(&mut self.numbering, &mut *self.env, cur.abs_id) else { return Ok(()) };
            abs = copy;
            num_id = store::add_num(&mut self.numbering, abs, "");
            let at = self.selection.focus.para as usize;
            self.retarget_list(at, cur.num_id, num_id)?;
        }
        for l in levels {
            store::set_level(&mut self.numbering, abs, l.ilvl, &level_xml_of(l));
            let n = l.p_style.as_deref().filter(|s| !s.is_empty()).and_then(|s| self.heading_level(Some(s)));
            if let Some(n) = n {
                self.link_heading_style(js_round(n) as u32 + 1, num_id);
            }
        }
        // a multilevel list (Word writes `multilevel` when the model is its own)
        if let Some(abs_xml) = store::abstract_xml(&self.numbering, abs)
            && levels.iter().any(|l| l.p_style.as_deref().is_some_and(|s| !s.is_empty()))
        {
            let lvls = all_level_elements(&abs_xml);
            store::set_levels(&mut self.numbering, abs, &lvls, Some("multilevel"), None);
        }
        self.touched(abs);
        self.sync_numbering();
        Ok(())
    }

    /// `createListStyle(name, levels, { apply })`: a `w:style` of type numbering based on No List,
    /// with the style's `w:num` in the `w:numPr`, and the multilevel `w:abstractNum` with
    /// `w:styleLink` (the Word model "1) a) i)" with the given levels over it). The styleId, or
    /// `None` when it was refused.
    pub fn create_list_style(&mut self, name: &str, levels: &[ListLevelSpec], apply: bool) -> Result<Option<String>, CommandError> {
        let nm = uo_jsnum::text::trim(name);
        if !self.can_edit() || !self.styles_editable() || nm.is_empty() || self.style_name_taken(nm, None) {
            return Ok(None);
        }
        if !self.inner_guard("createListStyle", GuardArg::None) {
            return Ok(None);
        }
        self.begin_step(false, &[]);
        let loc = self.list_locale();
        let Some(def) = MULTILEVEL_LIBRARY.first() else { return Ok(None) };
        let mut lvls = library_levels(def, &loc);
        for l in levels {
            if l.ilvl >= 0.0
                && l.ilvl <= 8.0
                && let Some(slot) = lvls.get_mut(js_round(l.ilvl) as usize)
            {
                *slot = level_xml_of(l);
            }
        }
        let taken: std::collections::HashSet<String> = {
            let idx = self.style_index();
            super::styles::xform::list_styles_all(&idx, self.f2.lang).into_iter().map(|s| s.id).collect()
        };
        let id = super::styles::sheet::style_id_from_name(nm, &taken);
        let tmpl = if levels.is_empty() { def.tmpl.map(|t| t.replacen("{LCID}", loc.lcid, 1)) } else { None };
        let o = ListOpts { multi_level_type: "multilevel", tmpl: tmpl.as_deref(), style_link: Some(&id), num_style_link: None };
        let num_id = store::add_list(&mut self.numbering, &mut *self.env, &lvls, &o);
        let no_list = match self.doc.formatting.styles.defaults.numbering.clone() {
            Some(n) => Some(n.to_string()),
            None => self.builtin_style_id("No List").ok().map(|s| s.to_string()),
        };
        let xml = format!(
            "<w:style w:type=\"numbering\" w:customStyle=\"1\" w:styleId=\"{}\"><w:name w:val=\"{}\"/>{}<w:pPr><w:numPr><w:numId w:val=\"{}\"/></w:numPr></w:pPr></w:style>",
            esc(&id),
            esc(nm),
            no_list.as_deref().filter(|s| !s.is_empty()).map_or_else(String::new, |n| format!("<w:basedOn w:val=\"{}\"/>", esc(n))),
            to_js_string(num_id),
        );
        let base = self.styles_base();
        if let Some(x) = super::styles::sheet::put_style(&base, &id, &xml) {
            self.commit_styles(x);
        }
        if apply {
            self.link_list_style(&id)?;
        }
        self.sync_numbering();
        Ok(Some(id))
    }

    /// `applyListStyle(styleId)`: the abstractNum with only a `w:numStyleLink` (the style's
    /// `w:tmpl`) and a new `w:num`; Normal → List Paragraph (measured).
    pub(crate) fn apply_list_style(&mut self, style_id: &str) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        self.link_list_style(style_id)?;
        self.sync_numbering();
        Ok(())
    }

    fn link_list_style(&mut self, style_id: &str) -> Result<(), CommandError> {
        let own = Session::style_num_of(&self.doc, style_id).filter(|n| uo_jsnum::conv::truthy(*n));
        let Some(abs) = own.and_then(|o| self.abstract_of(o)) else { return Ok(()) };
        let tmpl = store::abstract_xml(&self.numbering, abs).and_then(|x| tmpl_of(&x));
        let o = ListOpts { multi_level_type: "multilevel", tmpl: tmpl.as_deref(), style_link: None, num_style_link: Some(style_id) };
        let num_id = store::add_list(&mut self.numbering, &mut *self.env, &[], &o);
        let bullet = self.num_level(num_id, 0.0).is_some_and(|l| l.num_fmt.as_deref() == Some("bullet"));
        let paras = self.selected_paras();
        self.put_list(&paras, num_id, bullet)
    }

    /// `modifyListStyle(styleId, levels)`: the levels in the style's abstractNum (every list that
    /// uses it changes, as in Word: ls-modify).
    pub(crate) fn modify_list_style(&mut self, style_id: &str, levels: &[ListLevelSpec]) -> Result<(), CommandError> {
        let own = Session::style_num_of(&self.doc, style_id).filter(|n| uo_jsnum::conv::truthy(*n));
        let abs = own.and_then(|o| self.abstract_of(o));
        let Some(abs) = abs else { return Ok(()) };
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        for l in levels {
            store::set_level(&mut self.numbering, abs, l.ilvl, &level_xml_of(l));
        }
        self.sync_numbering();
        Ok(())
    }

    // ───────────────────────── the hooks of the text family ─────────────────────────

    /// `listEnterOnEmpty()`: Enter on an EMPTY item (measured): level 2+ goes up a level; level 1
    /// leaves the list (Normal). `true`: handled.
    pub(crate) fn list_enter_on_empty(&mut self) -> Result<bool, CommandError> {
        if !self.selection_in_range() {
            return Ok(false);
        }
        let pi = self.selection.focus.para as usize;
        let Some(len) = self.paras().and_then(|ps| ps.get(pi)).map(para_length) else { return Ok(false) };
        // (the TS tests `!n || paraLength(p) > 0 || hasSelection()`: the cheap tests first, the list last)
        if len > 0 || self.has_selection() {
            return Ok(false);
        }
        let Some(n) = self.para_num_at(pi) else { return Ok(false) };
        if n.ilvl > 0.0 {
            let lv = n.ilvl - 1.0;
            if self.inner_guard("setListLevel", GuardArg::Num(lv)) {
                self.set_list_level(lv)?;
            }
        } else {
            self.remove_list()?;
        }
        Ok(true)
    }

    /// `listBackspaceAtStart()`: Backspace at the start (measured): a list item loses the number
    /// and stays List Paragraph (the text does not move); a List Paragraph without number becomes
    /// Normal with a 360 indent (where the number was); that indent goes away. `false` outside those
    /// cases (a normal Backspace).
    pub(crate) fn list_backspace_at_start(&mut self) -> Result<bool, CommandError> {
        if !self.selection_in_range() {
            return Ok(false);
        }
        let DocPosition { para, offset } = self.selection.focus;
        if offset != 0 || self.has_selection() {
            return Ok(false);
        }
        let pi = para as usize;
        let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { return Ok(false) };
        let (sid, ind_left, first_line) = (p.props.style_id.clone(), p.props.indent_left_twips, p.props.indent_first_line_twips);
        // `builtinIdOf('list paragraph')` only matters for a paragraph that has a style
        let lp = if sid.as_deref().is_some_and(|s| !s.is_empty()) { self.builtin_id_of("list paragraph") } else { None };
        let falsy = |n: Option<JsNumber>| n.is_none_or(|x| !x.truthy());
        let mut patch = ParaProps::default();
        if self.para_num_at(pi).is_some() {
            if self.style_numbered(sid.as_deref()) {
                obj::set(&mut patch, "numbering", |p| {
                    p.numbering = Some(NumberingProps {
                        kind: "decimal".into(),
                        ilvl: JsNumber::Num(0.0),
                        num_id: Some(JsNumber::Num(0.0)),
                        ..NumberingProps::default()
                    });
                });
            } else {
                obj::set_undefined(&mut patch, "numbering", |p| p.numbering = None);
            }
        } else if lp.as_deref().is_some_and(|l| !l.is_empty() && sid.as_deref() == Some(l)) && falsy(ind_left) {
            obj::set_undefined(&mut patch, "styleId", |p| p.style_id = None);
            obj::set(&mut patch, "indentLeftTwips", |p| p.indent_left_twips = Some(JsNumber::Num(360.0)));
        } else if ind_left == Some(JsNumber::Num(360.0))
            && falsy(first_line)
            && sid.as_deref().is_none_or(str::is_empty)
            && self.f2.backspace_trail == Some(para)
        {
            obj::set_undefined(&mut patch, "indentLeftTwips", |p| p.indent_left_twips = None);
        } else {
            return Ok(false);
        }
        self.begin_step(false, &[]);
        self.set_para_at(pi, &patch)?;
        self.f2.backspace_trail = Some(para);
        Ok(true)
    }
}

// ───────────────────────── the save ─────────────────────────

/// `writeNumberingPart(pkg, numberingOut(blocks, hfParts))`: the numbering text to write (levels
/// without a paragraph are `w:tentative="1"` in the lists the editor made) into the package.
pub(crate) fn save_numbering(
    pkg: &mut uo_opc::Package,
    store_: &super::super::versions::VersionStore,
    f2: &F2State,
    doc: &Document,
    hf: &[crate::write::parts::HfPartOut<'_>],
) -> Result<(), DocxError> {
    let xml = store_.xml().map_or("", |x| &**x);
    let parsed = crate::read::parts::read_numbering_text(xml, &DocxLimits::default()).unwrap_or_default();
    let doc_ref = doc;
    let style_num = |s: &str| Session::style_num_of(doc_ref, s);
    let view = NumView { parsed: &parsed, style_num: &style_num };
    let mut used: Vec<(f64, Vec<f64>)> = Vec::new();
    let mut see = |p: &Paragraph| {
        let Some(n) = p.props.numbering.as_ref() else { return };
        let Some(id) = n.num_id.filter(|x| x.truthy()) else { return };
        let Some(abs) = view.abstract_of(id.to_number()) else { return };
        let ilvl = n.ilvl.to_number();
        match used.iter_mut().find(|(a, _)| *a == abs) {
            Some((_, v)) => v.push(ilvl),
            None => used.push((abs, vec![ilvl])),
        }
    };
    for p in doc.body.flat_paragraphs() {
        see(p);
    }
    for h in hf {
        for p in h.story.flat_paragraphs() {
            see(p);
        }
    }
    let out = store::with_tentative(store_, &f2.num_touched, &used).unwrap_or_default();
    crate::write::parts::write_numbering_part(pkg, out)?;
    write_pic_bullet_images(pkg, f2, xml)
}

/// `numbering.writeImages(pkg, partPath)` (numbering-store.ts 115, wave 7 A2): the new picture bullets the part uses — the
/// media (`word/media/uwbulletN.ext`), the part's relationship and the content type of the extension.
fn write_pic_bullet_images(pkg: &mut uo_opc::Package, f2: &F2State, xml: &str) -> Result<(), DocxError> {
    let used: Vec<&(Str, Arc<[u8]>, Str)> = f2.fresh_images.iter().filter(|(rid, _, _)| xml.contains(&format!("r:id=\"{rid}\""))).collect();
    if used.is_empty() {
        return Ok(());
    }
    let part =
        crate::write::parts::part_by_rel_type(pkg, "numbering", "word/numbering.xml")?.unwrap_or_else(|| "word/numbering.xml".to_owned());
    let slash = part.rfind('/').map_or(0, |i| i + 1);
    let rels_path = format!("{}_rels/{}.rels", &part[..slash], &part[slash..]);
    let mut rels = pkg.part_text(&rels_path)?.map(std::borrow::Cow::into_owned).unwrap_or_else(|| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_owned()
    });
    let mut ct = pkg.part_text("[Content_Types].xml")?.map(std::borrow::Cow::into_owned);
    for (rid, data, ext0) in used {
        if rels.contains(&format!("Id=\"{rid}\"")) {
            continue;
        }
        let ext = if &**ext0 == "jpg" { "jpeg" } else { &**ext0 };
        let mut k = 1;
        while pkg.contains(&format!("word/media/uwbullet{k}.{ext}")) {
            k += 1;
        }
        pkg.set_part(format!("word/media/uwbullet{k}.{ext}"), data.to_vec());
        rels = rels.replacen(
            "</Relationships>",
            &format!("<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/uwbullet{k}.{ext}\"/></Relationships>"),
            1,
        );
        if let Some(c) = ct.as_mut()
            && !c.to_ascii_lowercase().contains(&format!("extension=\"{}\"", ext.to_ascii_lowercase()))
        {
            *c = c.replacen("</Types>", &format!("<Default Extension=\"{ext}\" ContentType=\"image/{ext}\"/></Types>"), 1);
        }
    }
    pkg.set_part_text(rels_path, rels);
    if let Some(c) = ct {
        pkg.set_part_text("[Content_Types].xml", c);
    }
    Ok(())
}

// ───────────────────────── text helpers ─────────────────────────

/// `listLevelSpec`-count: `(l.lvlText.match(/%[1-9]/g)?.length ?? 0)`.
fn count_level_refs(t: &str) -> usize {
    t.as_bytes().windows(2).filter(|w| w[0] == b'%' && (b'1'..=b'9').contains(&w[1])).count()
}

/// `parseLevel(lvl)`: a raw `w:lvl` read as the engine reads it.
pub(crate) fn parse_level(lvl: &str) -> Option<NumLevel> {
    let wrap = format!("<w:numbering><w:abstractNum w:abstractNumId=\"0\">{lvl}</w:abstractNum></w:numbering>");
    let n = crate::read::parts::read_numbering_text(&wrap, &DocxLimits::default()).ok()?;
    n.abstract_nums.get("0")?.levels.values().next().cloned()
}

/// `levelRPrXml(l)`: the `w:rPr` of the number or bullet (CT_RPr order); empty without formatting.
fn level_rpr_xml(font: Option<&str>, bold: bool, italic: bool, size_pt: Option<f64>, color: Option<&str>) -> String {
    let mut x = String::new();
    if let Some(f) = font.filter(|f| !f.is_empty()) {
        x.push_str(&format!("<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:hint=\"default\"/>", esc(f)));
    }
    if bold {
        x.push_str("<w:b/>");
    }
    if italic {
        x.push_str("<w:i/>");
    }
    if let Some(c) = color.filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit())) {
        x.push_str(&format!("<w:color w:val=\"{}\"/>", c.to_ascii_uppercase()));
    }
    if let Some(s) = size_pt.filter(|s| uo_jsnum::conv::truthy(*s)) {
        x.push_str(&format!("<w:sz w:val=\"{}\"/>", to_js_string(js_round(s * 2.0))));
    }
    if x.is_empty() { x } else { format!("<w:rPr>{x}</w:rPr>") }
}

/// `levelXmlOf(l)`: the `w:lvl` of a level, in the schema order and as Word writes it.
pub(crate) fn level_xml_of(l: &ListLevelSpec) -> String {
    let left = to_js_string(js_round(l.ind_left));
    let ind = if l.hanging >= 0.0 {
        format!("<w:ind w:left=\"{left}\" w:hanging=\"{}\"/>", to_js_string(js_round(l.hanging)))
    } else {
        format!("<w:ind w:left=\"{left}\" w:firstLine=\"{}\"/>", to_js_string(js_round(-l.hanging)))
    };
    let mut s = format!(
        "<w:lvl w:ilvl=\"{}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{}\"/>",
        to_js_string(l.ilvl),
        to_js_string(l.start.unwrap_or(1.0)),
        esc(&l.num_fmt)
    );
    if let Some(r) = l.lvl_restart {
        s.push_str(&format!("<w:lvlRestart w:val=\"{}\"/>", to_js_string(r)));
    }
    if let Some(p) = l.p_style.as_deref().filter(|p| !p.is_empty()) {
        s.push_str(&format!("<w:pStyle w:val=\"{}\"/>", esc(p)));
    }
    if l.is_lgl {
        s.push_str("<w:isLgl/>");
    }
    if let Some(f) = l.suff.as_deref().filter(|f| !f.is_empty() && *f != "tab") {
        s.push_str(&format!("<w:suff w:val=\"{f}\"/>"));
    }
    s.push_str(&format!("<w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"{}\"/><w:pPr>", esc(&l.lvl_text), l.jc.as_deref().unwrap_or("left")));
    if let Some(t) = l.tab_pos {
        s.push_str(&format!("<w:tabs><w:tab w:val=\"num\" w:pos=\"{}\"/></w:tabs>", to_js_string(js_round(t))));
    }
    s.push_str(&ind);
    s.push_str("</w:pPr>");
    s.push_str(&level_rpr_xml(l.font.as_deref(), l.bold, l.italic, l.size_pt, l.color.as_deref()));
    s.push_str("</w:lvl>");
    s
}

/// `raw.replace(/(<w:ind\b[^>]*\bw:left=")(\d+)(")/, (_m, a, v, b) => a + (Number(v) + shift) + b)`.
pub(crate) fn shift_ind_left(raw: &str, shift: f64) -> String {
    let mut at = 0;
    while let Some(s) = rx::next_open(raw, "w:ind", at) {
        let a = s + "<w:ind".len();
        let Some(gt) = raw.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
        let attrs = &raw[a..gt];
        // greedy `[^>]*`: the last `\bw:left="digits"` of the tag
        let needle = "w:left=\"";
        let mut best: Option<(usize, usize)> = None;
        let mut from = 0;
        while let Some(p) = attrs.get(from..).and_then(|r| r.find(needle)) {
            let q = from + p;
            let vs = q + needle.len();
            let digits = attrs.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
            if q > 0
                && !uo_jsnum::text::is_js_word(attrs.as_bytes()[q - 1])
                && digits > 0
                && attrs.as_bytes().get(vs + digits) == Some(&b'"')
            {
                best = Some((vs, vs + digits));
            }
            from = q + 1;
        }
        if let Some((vs, ve)) = best {
            let v = rx::digits_to_f64(&attrs[vs..ve]);
            return format!("{}{}{}", &raw[..a + vs], to_js_string(v + shift), &raw[a + ve..]);
        }
        at = s + 1;
    }
    raw.to_owned()
}

/// The `w:start` edit of `restartNumbering`: replaces the `w:start` or adds it after the `w:lvl`
/// start tag.
pub(crate) fn set_level_start(raw: &str, s: &str) -> String {
    if rx::next_open(raw, "w:start", 0).is_some() {
        // `/<w:start w:val="-?\d+"\/>/`
        let prefix = "<w:start w:val=\"";
        let mut at = 0;
        while let Some(p) = raw.get(at..).and_then(|r| r.find(prefix)) {
            let q = at + p;
            let mut vs = q + prefix.len();
            if raw.as_bytes().get(vs) == Some(&b'-') {
                vs += 1;
            }
            let digits = raw.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
            if digits > 0 && raw.get(vs + digits..).is_some_and(|r| r.starts_with("\"/>")) {
                return format!("{}{s}{}", &raw[..q], &raw[vs + digits + 3..]);
            }
            at = q + 1;
        }
        return raw.to_owned();
    }
    // `/^<w:lvl\b[^>]*>/`
    if raw.starts_with("<w:lvl")
        && rx::boundary_after_word(raw, "<w:lvl".len())
        && let Some(gt) = raw.find('>')
    {
        return format!("{}{s}{}", &raw[..=gt], &raw[gt + 1..]);
    }
    raw.to_owned()
}

/// `/<w:tmpl w:val="([0-9A-Fa-f]{8})"\/>/.exec(x)?.[1]`.
pub(crate) fn tmpl_of(x: &str) -> Option<String> {
    let prefix = "<w:tmpl w:val=\"";
    let mut at = 0;
    while let Some(p) = x.get(at..).and_then(|r| r.find(prefix)) {
        let vs = at + p + prefix.len();
        let v = x.get(vs..vs + 8)?;
        if v.bytes().all(|b| b.is_ascii_hexdigit()) && x.get(vs + 8..).is_some_and(|r| r.starts_with("\"/>")) {
            return Some(v.to_owned());
        }
        at = vs;
    }
    None
}

/// `[...abs.matchAll(/<w:lvl\b[^>]*>[\s\S]*?<\/w:lvl>/g)].map((m) => m[0])`.
pub(crate) fn all_level_elements(abs: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(s) = rx::next_open(abs, "w:lvl", at) {
        let a = s + "<w:lvl".len();
        let Some(gt) = abs.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
        let Some(c) = abs.get(gt + 1..).and_then(|r| r.find("</w:lvl>")) else { break };
        let end = gt + 1 + c + "</w:lvl>".len();
        out.push(abs[s..end].to_owned());
        at = end;
    }
    out
}

/// The `linkHeadingStyle` edit of the style's raw text: the `w:numPr` replaced, or put in the
/// `w:pPr` after the leading flags (CT_PPrBase order), or a new `w:pPr`.
pub(crate) fn put_num_pr(raw: &str, num_pr: &str) -> String {
    // `/<w:numPr>[\s\S]*?<\/w:numPr>/`
    if let Some(s) = raw.find("<w:numPr>")
        && let Some(c) = raw[s..].find("</w:numPr>")
    {
        return format!("{}{num_pr}{}", &raw[..s], &raw[s + c + "</w:numPr>".len()..]);
    }
    if let Some(s) = raw.find("<w:pPr>") {
        // `(?:<w:(?:pStyle|keepNext|keepLines|pageBreakBefore|framePr|widowControl)\b[^>]*\/>)*`
        let mut at = s + "<w:pPr>".len();
        while let Some(name) = ["pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl"]
            .iter()
            .find(|n| raw.get(at..).is_some_and(|r| r.starts_with(&format!("<w:{n}"))) && rx::boundary_after_word(raw, at + 3 + n.len()))
        {
            let a = at + 3 + name.len();
            let Some(gt) = raw.get(a..).and_then(|r| r.find('>')).map(|p| a + p) else { break };
            if gt > a && raw.as_bytes().get(gt - 1) == Some(&b'/') {
                at = gt + 1;
            } else {
                break;
            }
        }
        return format!("{}{num_pr}{}", &raw[..at], &raw[at..]);
    }
    // `/<w:rPr>|<w:tblPr>|<\/w:style>/`
    let cands = ["<w:rPr>", "<w:tblPr>", "</w:style>"];
    let hit = cands.iter().filter_map(|c| raw.find(c).map(|p| (p, *c))).min_by_key(|(p, _)| *p);
    match hit {
        Some((p, _)) => format!("{}<w:pPr>{num_pr}</w:pPr>{}", &raw[..p], &raw[p..]),
        None => raw.to_owned(),
    }
}

/// `/<w:numPr>[\s\S]*?<w:numId w:val="[1-9]\d*"/.test(raw)`: the style carries a numbering.
pub(crate) fn numbered_raw(raw: &str) -> bool {
    let Some(p) = raw.find("<w:numPr>") else { return false };
    let rest = &raw[p + "<w:numPr>".len()..];
    let needle = "<w:numId w:val=\"";
    let mut at = 0;
    while let Some(q) = rest.get(at..).and_then(|r| r.find(needle)) {
        let vs = at + q + needle.len();
        let digits = rest.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if digits > 0 && rest.as_bytes().get(vs) != Some(&b'0') && rest.as_bytes().get(vs + digits) == Some(&b'"') {
            return true;
        }
        at = vs;
    }
    false
}

/// The pure model files of the family, for the parity capture of `numbering-store.ts`
/// (`js/capture-d2b-f2-ops.mjs`): the store operations over a [`VersionStore`] with the library in
/// the pt-BR locale. Not part of the API.
#[doc(hidden)]
pub mod testing {
    use super::super::super::env::Env;
    use super::super::super::versions::VersionStore;
    use super::library::{BULLET_LIBRARY, MULTILEVEL_LIBRARY, NUMBER_LIBRARY};
    use super::{ListLocale, ListOpts, NumView, library_levels, rx, store};
    use crate::limits::DocxLimits;

    /// `libraryLevels(def, loc)` of library `kind` (0 bullets, 1 numbers, 2 multilevel) item `idx`,
    /// its `multiLevelType` and its `tmpl` (with `{LCID}` filled).
    #[must_use]
    pub fn library(kind: usize, idx: usize) -> (Vec<String>, &'static str, Option<String>) {
        let lib = [BULLET_LIBRARY, NUMBER_LIBRARY, MULTILEVEL_LIBRARY][kind % 3];
        let d = &lib[idx % lib.len()];
        let loc = ListLocale {
            lcid: "0416",
            article: "Artigo",
            section: "Se\u{e7}\u{e3}o",
            chapter: "Cap\u{ed}tulo",
            headings: (1..=9).map(|n| format!("Ttulo{n}")).collect(),
        };
        (library_levels(d, &loc), d.multi_level_type, d.tmpl.map(|t| t.replacen("{LCID}", "0416", 1)))
    }

    pub fn add_list(st: &mut VersionStore, env: &mut dyn Env, levels: &[String], multi_level_type: &str, tmpl: Option<&str>) -> f64 {
        store::add_list(st, env, levels, &ListOpts { multi_level_type, tmpl, ..ListOpts::default() })
    }

    pub fn add_num(st: &mut VersionStore, abs: f64, overrides: &str) -> f64 {
        store::add_num(st, abs, overrides)
    }

    pub fn copy_abstract(st: &mut VersionStore, env: &mut dyn Env, abs: f64) -> Option<f64> {
        store::copy_abstract(st, env, abs)
    }

    pub fn set_level(st: &mut VersionStore, abs: f64, ilvl: f64, lvl: &str) {
        store::set_level(st, abs, ilvl, lvl);
    }

    pub fn set_levels(st: &mut VersionStore, abs: f64, levels: &[String], multi_level_type: Option<&str>, tmpl: Option<&str>) {
        store::set_levels(st, abs, levels, multi_level_type, tmpl);
    }

    #[must_use]
    pub fn replace_ilvl(lvl: &str, ilvl: f64) -> String {
        store::replace_ilvl(lvl, ilvl)
    }

    #[must_use]
    pub fn with_tentative(st: &VersionStore, touched: &[f64], used: &[(f64, Vec<f64>)]) -> Option<String> {
        store::with_tentative(st, touched, used)
    }

    /// `abstractOf(numId)` with no list styles (`DocNumbering.styleNum` unset).
    #[must_use]
    pub fn abstract_of(st: &VersionStore, num_id: f64) -> Option<f64> {
        let parsed = crate::read::parts::read_numbering_text(st.xml().map_or("", |x| &**x), &DocxLimits::default()).ok()?;
        let none = |_: &str| None;
        NumView { parsed: &parsed, style_num: &none }.abstract_of(num_id)
    }

    /// The abstract numbering ids of the part, as the TS `matchAll` over `w:abstractNumId` gives them.
    #[must_use]
    pub fn abstract_ids(st: &VersionStore) -> Vec<f64> {
        rx::all_digits_attr(st.xml().map_or("", |x| &**x), "w:abstractNum", "w:abstractNumId").into_iter().map(rx::digits_to_f64).collect()
    }
}

#[cfg(test)]
mod regex_tests;
