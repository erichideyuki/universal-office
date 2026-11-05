//! D2b family: styles (F2): `applyStyle`, `createStyle`, `modifyStyle`, `setDefaultFont`,
//! `updateStyleFromSelection`, `renameStyle`, `deleteStyle`, `setStyleInGallery`,
//! `clearParagraphStyle`, `clearCharacterStyle` (and the two direct-formatting twins of the
//! Style Inspector); `styles.ts` (the store's transformations), docx-core `style-sheet.ts`,
//! `word-styles.ts` (the built-ins, `data::word_styles`); the Enter `applyNextStyle` hook; the
//! save of the style sheet.
//!
//! The TS (`universal-write.ts` 4150–4660, `styles.ts`, `style-sheet.ts`): `word/styles.xml` is
//! edited without loss; every command takes ONE undo step, the sheet is re-read after each commit
//! (`syncStyleSheet`) and the paragraphs follow renames and deletions (`retargetStyles`).

pub mod sheet;
pub(crate) mod xform;

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::text::{self, CarriedProps, CharStyleOpt, ClearOptions, DocPosition, DocRange, normalize_range, para_length, para_text};
use super::super::{ExecCx, Session};
use crate::js::{JsObject, Slot, Str};
use crate::limits::DocxLimits;
use crate::model::formatting::StyleSheet;
use crate::model::para::{ParaProps, Paragraph};
use crate::model::run::RunProps;
use crate::model::story::Story;
use crate::read::plain::default_run_props;
use crate::word::obj;
pub use sheet::{Ch, StyleFormat, UiLang};
use sheet::{StyleIndex, StyleMeta};
use xform::{CreateStyle, Defaults, ModifyStyle};

pub(crate) use xform::write_styles_part;

const EMPTY_STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:styles>";

/// `CreateStyleOptions` (+ `fromSelection`, `apply`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CreateStyleSpec {
    pub name: String,
    /// `paragraph` | `character` | `linked` (default `linked`)
    pub kind: Option<String>,
    pub based_on: Option<String>,
    pub next: Option<String>,
    /// in the gallery (default true)
    pub quick: Option<bool>,
    pub auto_update: bool,
    pub format: Option<StyleFormat>,
    /// take the formatting of the selection (default true)
    pub from_selection: Option<bool>,
    /// apply to the selection (default true)
    pub apply: Option<bool>,
}

/// `ModifyStyleOptions`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModifyStyleSpec {
    pub name: Option<String>,
    pub based_on: Option<String>,
    /// `Null`: the next is the style itself
    pub next: Ch<String>,
    pub quick: Option<bool>,
    pub auto_update: Option<bool>,
    pub format: StyleFormat,
}

/// The part of `FontFormat` the "Set as Default" of the Font dialog writes into Normal (a themed
/// colour is sent resolved: the hex).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DefaultFont {
    pub font_family: Option<String>,
    pub size_pt: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// `underline !== 'none'`
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,
    pub color: Ch<String>,
    pub spacing_twips: Option<f64>,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleCmd {
    /// `applyStyle(styleId)`; `None` = Normal
    ApplyStyle { style_id: Option<String> },
    /// `createStyle(opts)` ([`Session::create_style`] gives the id)
    CreateStyle { spec: Box<CreateStyleSpec> },
    /// `modifyStyle(id, opts)`
    ModifyStyle { id: String, spec: Box<ModifyStyleSpec> },
    /// `setDefaultFont(f)`
    SetDefaultFont { font: Box<DefaultFont> },
    /// `updateStyleFromSelection(id?)`
    UpdateStyleFromSelection { id: Option<String> },
    /// `renameStyle(id, name)`
    RenameStyle { id: String, name: String },
    /// `deleteStyle(id)`
    DeleteStyle { id: String },
    /// `setStyleInGallery(id, on)`
    SetStyleInGallery { id: String, on: bool },
    /// `clearParagraphStyle()` (Style Inspector)
    ClearParagraphStyle,
    /// `clearParagraphFormatting()`
    ClearParagraphFormatting,
    /// `clearCharacterStyle()`
    ClearCharacterStyle,
    /// `clearCharacterFormatting()`
    ClearCharacterFormatting,
}

impl StyleCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        Some(match self {
            StyleCmd::ApplyStyle { style_id } => ("applyStyle", style_id.as_deref().map_or(GuardArg::None, GuardArg::Str)),
            StyleCmd::CreateStyle { .. } => ("createStyle", GuardArg::None),
            StyleCmd::ModifyStyle { .. } => ("modifyStyle", GuardArg::None),
            StyleCmd::SetDefaultFont { .. } => ("setDefaultFont", GuardArg::None),
            StyleCmd::UpdateStyleFromSelection { .. } => ("updateStyleFromSelection", GuardArg::None),
            StyleCmd::RenameStyle { .. } => ("renameStyle", GuardArg::None),
            StyleCmd::DeleteStyle { .. } => ("deleteStyle", GuardArg::None),
            StyleCmd::SetStyleInGallery { .. } => ("setStyleInGallery", GuardArg::None),
            StyleCmd::ClearParagraphStyle
            | StyleCmd::ClearParagraphFormatting
            | StyleCmd::ClearCharacterStyle
            | StyleCmd::ClearCharacterFormatting => return None,
        })
    }
}

/// The run props `applyStyle` lets the style take over when they are uniform in the paragraph.
const RUN_KEYS: [&str; 21] = [
    "szHalfPt",
    "fontAscii",
    "color",
    "bold",
    "italic",
    "underline",
    "underlineStyle",
    "underlineColor",
    "underlineTheme",
    "colorTheme",
    "strike",
    "doubleStrike",
    "allCaps",
    "smallCaps",
    "vertAlign",
    "highlight",
    "charSpacingTwips",
    "hidden",
    "scalePct",
    "positionHalfPt",
    "kernHalfPt",
];

/// `props[key]` when it holds a value (not absent, not `undefined`): its JSON, to compare with `===`.
fn run_slot(props: &RunProps, key: &str) -> Option<String> {
    let i = RunProps::key_index(key)?;
    match props.slot(usize::from(i)) {
        Slot::Val(v) => Some(obj::json_of(Some(v)).unwrap_or_default()),
        _ => None,
    }
}

/// `CLEARED_PARA` with `styleId`.
fn cleared_para(style: Option<&str>) -> ParaProps {
    let mut p = ParaProps::default();
    for k in [
        "numbering",
        "alignment",
        "spaceBeforeTwips",
        "spaceAfterTwips",
        "lineTwentieths",
        "indentLeftTwips",
        "indentRightTwips",
        "indentFirstLineTwips",
        "borders",
        "shadingHex",
    ] {
        obj::set_undefined(&mut p, k, |_| {});
    }
    match style {
        Some(s) => obj::set(&mut p, "styleId", |x| x.style_id = Some(s.into())),
        None => obj::set_undefined(&mut p, "styleId", |_| {}),
    }
    p
}

/// `CLEARED_RUN` with `charStyle`.
fn cleared_run(char_style: Option<&str>) -> RunProps {
    let mut p = default_run_props();
    for k in ["szHalfPt", "color", "highlight", "fontAscii", "vertAlign", "charSpacingTwips"] {
        obj::set_undefined(&mut p, k, |_| {});
    }
    match char_style {
        Some(s) => obj::set(&mut p, "charStyle", |x| x.char_style = Some(s.into())),
        None => obj::set_undefined(&mut p, "charStyle", |_| {}),
    }
    p
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

impl Session {
    // ───────────────────────── the store's glue ─────────────────────────

    /// `docStyles.index()` (memoised by version).
    pub(crate) fn style_index(&mut self) -> Arc<StyleIndex> {
        let v = self.styles.version();
        if let Some((pv, i)) = &self.f2.style_idx
            && *pv == v
        {
            return Arc::clone(i);
        }
        let idx = Arc::new(self.styles.xml().map_or_else(StyleIndex::default, |x| sheet::read_style_index(x)));
        self.f2.style_idx = Some((v, Arc::clone(&idx)));
        idx
    }

    /// Builds the style index now (the open does it, so that the first Enter does not pay for the
    /// parse: the per-keystroke budget of D2a-M3).
    pub(crate) fn warm_style_index(&mut self) {
        let _ = self.style_index();
    }

    fn style_defaults(&mut self) -> Defaults {
        Defaults::of(&self.style_index())
    }

    /// `docStyles.defaultParagraph()`.
    pub(crate) fn default_paragraph(&mut self) -> String {
        self.style_defaults().paragraph
    }

    /// `docStyles.base()`: the text to edit (a document without `styles.xml` gets a minimal part).
    pub(crate) fn styles_base(&self) -> String {
        self.styles.xml().map_or_else(|| EMPTY_STYLES.to_owned(), |x| x.to_string())
    }

    fn meta(&mut self, id: &str) -> Option<StyleMeta> {
        self.style_index().styles.iter().find(|s| s.id == id).cloned()
    }

    /// `syncStyleSheet()`: re-reads the sheet (engine and editor) and tells the shell.
    pub(crate) fn sync_style_sheet(&mut self) {
        let xml = self.styles.xml().map(|x| x.to_string());
        let limits = DocxLimits::default();
        let (styles, sheet) = match xml.as_deref() {
            Some(x) => (crate::read::styles::read_styles_text(x, &limits).ok(), crate::read::parts::read_style_sheet_text(x).ok()),
            None => (crate::read::styles::read_styles_xml(None, &limits).ok(), Some(StyleSheet::default())),
        };
        if let Some(s) = styles {
            self.doc.styles = Arc::new(s);
        }
        if let Some(sh) = sheet {
            Arc::make_mut(&mut self.doc.formatting).styles = sh;
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.emit(Event::StylesChange);
    }

    /// `commitStyles(xml)`: a new version of `styles.xml` (also outside an undo step: a built-in
    /// becomes defined when used).
    pub(crate) fn commit_styles(&mut self, xml: String) {
        if self.styles.xml().is_some_and(|x| **x == *xml) {
            return;
        }
        self.styles.set(Some(Arc::from(xml)));
        self.sync_style_sheet();
        let c = self.history.set_dirty(true);
        self.dirty_event(c);
    }

    /// `builtinStyleId(name)`: the document's style for a built-in (by its canonical `w:name`),
    /// defined if missing.
    pub fn builtin_style_id(&mut self, name: &str) -> Result<Str, CommandError> {
        let b = sheet::builtin_style(name);
        let pt = b.map(|b| xform::id_of(b.xml)).filter(|s| !s.is_empty()).unwrap_or_else(|| name.to_owned());
        let base = self.styles_base();
        let id = xform::resolve_style_id(Some(&base), &pt);
        let out = xform::ensure_defined(&base, &id, true);
        self.commit_styles(out);
        Ok(id.into())
    }

    /// `ensureStyleUsed(id)`: using a style takes the `semiHidden` off (`unhideWhenUsed`).
    pub(crate) fn ensure_style_used(&mut self, id: Option<&str>) {
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            let base = self.styles_base();
            self.commit_styles(xform::ensure_defined(&base, id, true));
        }
    }

    /// `styleNameTaken(name, except?)`.
    pub(crate) fn style_name_taken(&mut self, name: &str, except: Option<&str>) -> bool {
        let k = lower(name);
        let idx = self.style_index();
        xform::list_styles_all(&idx, self.f2.lang)
            .iter()
            .any(|s| except != Some(s.id.as_str()) && (lower(&s.name) == k || lower(&s.raw_name) == k))
    }

    // ───────────────────────── the commands ─────────────────────────

    pub(crate) fn exec_styles(&mut self, cmd: StyleCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            StyleCmd::ApplyStyle { style_id } => self.apply_style(style_id.as_deref())?,
            StyleCmd::CreateStyle { spec } => {
                self.create_style(&spec)?;
            }
            StyleCmd::ModifyStyle { id, spec } => {
                self.modify_style(&id, &spec)?;
            }
            StyleCmd::SetDefaultFont { font } => {
                self.set_default_font(&font)?;
            }
            StyleCmd::UpdateStyleFromSelection { id } => {
                self.update_style_from_selection(id.as_deref())?;
            }
            StyleCmd::RenameStyle { id, name } => {
                self.rename_style(&id, &name)?;
            }
            StyleCmd::DeleteStyle { id } => {
                self.delete_style(&id)?;
            }
            StyleCmd::SetStyleInGallery { id, on } => self.set_style_in_gallery(&id, on)?,
            StyleCmd::ClearParagraphStyle => self.style_edit(|s| {
                let DocRange { start, end } = normalize_range(s.range());
                for pi in start.para..=end.para {
                    if let Some(p) = s.paras_mut().and_then(|ps| ps.make_mut(pi as usize)) {
                        obj::delete(Arc::make_mut(&mut p.props), "styleId");
                    }
                    s.touch_para(pi as usize);
                }
                Ok(())
            })?,
            StyleCmd::ClearParagraphFormatting => self.style_edit(|s| {
                let r = s.range();
                let paras = s.paras_mut().ok_or(CommandError::Position)?;
                text::clear_para_formatting(paras, r)?;
                let n = normalize_range(r);
                for pi in n.start.para..=n.end.para {
                    s.touch_para(pi as usize);
                }
                Ok(())
            })?,
            StyleCmd::ClearCharacterStyle => self.style_edit(|s| {
                let r = if s.has_selection() { Some(s.range()) } else { s.word_range_at(s.selection.focus, false) };
                match r {
                    Some(r) => {
                        let mut patch = RunProps::default();
                        obj::set_undefined(&mut patch, "charStyle", |p| p.char_style = None);
                        let paras = s.paras_mut().ok_or(CommandError::Position)?;
                        text::apply_run_props(paras, r, &patch)?;
                        s.touch_range(r);
                    }
                    None => {
                        if let Some(pp) = &mut s.pending_props {
                            obj::delete(&mut pp.props, "charStyle");
                        }
                    }
                }
                Ok(())
            })?,
            StyleCmd::ClearCharacterFormatting => self.style_edit(|s| {
                let r = if s.has_selection() { Some(s.range()) } else { s.word_range_at(s.selection.focus, true) };
                match r {
                    Some(r) => {
                        let paras = s.paras_mut().ok_or(CommandError::Position)?;
                        text::clear_run_formatting(paras, r, &ClearOptions { fields: None, char_style: CharStyleOpt::Keep })?;
                        s.touch_range(r);
                    }
                    None => {
                        let cur = s.current_props_f2();
                        let mut p = default_run_props();
                        if let Some(c) = cur.props.char_style.clone().filter(|c| !c.is_empty()) {
                            obj::set(&mut p, "charStyle", |x| x.char_style = Some(c));
                        }
                        s.pending_props = Some(CarriedProps::plain(p));
                    }
                }
                Ok(())
            })?,
        }
        Ok(Outcome::Done(self.finish()))
    }

    fn touch_range(&mut self, r: DocRange) {
        let n = normalize_range(r);
        for pi in n.start.para..=n.end.para {
            self.touch_para(pi as usize);
        }
    }

    /// `currentProps()`: the props at the caret.
    fn current_props_f2(&self) -> CarriedProps {
        match self.paras() {
            Some(ps) => text::props_at(ps, self.selection.focus),
            None => CarriedProps::plain(default_run_props()),
        }
    }

    /// `styleEdit(fn)`: one step around the change.
    fn style_edit(&mut self, f: impl FnOnce(&mut Session) -> Result<(), CommandError>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        f(self)
    }

    /// `wordRangeAt(pos, space)`: the word under the caret (`[\p{L}\p{N}_]`); `space` includes the
    /// spaces after it. `None`: the caret is outside a word.
    pub(crate) fn word_range_at(&self, pos: DocPosition, space: bool) -> Option<DocRange> {
        let p = self.paras()?.get(pos.para as usize)?;
        let t = para_text(p);
        let w = |i: i64| -> bool {
            usize::try_from(i).ok().and_then(|i| t.get(i)).is_some_and(|&u| {
                u == u16::from(b'_')
                    || char::from_u32(u32::from(u))
                        .is_some_and(|c| uo_text::props::is_letter(c as u32) || uo_text::props::is_number(c as u32))
            })
        };
        let off = i64::from(pos.offset);
        if !w(off) && !w(off - 1) {
            return None;
        }
        let (mut a, mut b) = (off, off);
        while w(a - 1) {
            a -= 1;
        }
        while w(b) {
            b += 1;
        }
        if space {
            while usize::try_from(b).ok().and_then(|i| t.get(i)) == Some(&u16::from(b' ')) {
                b += 1;
            }
        }
        let to = |x: i64| u32::try_from(x).unwrap_or(0);
        Some(DocRange { start: DocPosition { para: pos.para, offset: to(a) }, end: DocPosition { para: pos.para, offset: to(b) } })
    }

    /// `applyStyle(styleId)`.
    pub(crate) fn apply_style(&mut self, style_id: Option<&str>) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        self.begin_step(false, &[]);
        self.apply_style_core(style_id)?;
        self.pending_props = None;
        Ok(())
    }

    /// `applyStyleCore(styleId)`: the style as Word applies it (see the TS comment).
    fn apply_style_core(&mut self, style_id: Option<&str>) -> Result<(), CommandError> {
        let defp = self.default_paragraph();
        let base = self.styles_base();
        let sid = style_id.map(|s| xform::resolve_style_id(self.styles.xml().map(|x| &**x), s));
        let id: Option<String> = sid.filter(|s| *s != defp);
        if let Some(id) = &id {
            self.commit_styles(xform::ensure_defined(&base, id, true));
        }
        let meta = id.as_deref().and_then(|i| self.meta(i));
        let DocRange { start, end } = normalize_range(self.range());
        if meta.as_ref().is_some_and(|m| m.ty == "character") {
            return self.apply_char_style(id.as_deref());
        }
        let twin = meta.as_ref().and_then(|m| m.link.clone()).and_then(|l| self.meta(&l));
        if let Some(t) = &twin
            && t.ty == "character"
            && self.has_selection()
            && start.para == end.para
        {
            let len = self.paras().and_then(|ps| ps.get(start.para as usize)).map_or(0, para_length);
            if !(start.offset == 0 && end.offset as usize >= len) {
                return self.apply_char_style(Some(&t.id));
            }
        }
        for pi in start.para as usize..=end.para as usize {
            let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { continue };
            if self.opts.tracking {
                // with tracking on: the same changes, as format revisions
                let len = para_length(p);
                let mut patch = RunProps::default();
                {
                    let runs: Vec<&crate::model::run::Run> = p.runs.iter().filter(|r| r.props.del.is_none()).collect();
                    if let Some(first_run) = runs.first() {
                        for key in RUN_KEYS {
                            let first = run_slot(&first_run.props, key);
                            if first.is_some() && runs.iter().all(|r| run_slot(&r.props, key) == first) {
                                match key {
                                    "bold" => obj::set(&mut patch, key, |x| x.bold = Some(false)),
                                    "italic" => obj::set(&mut patch, key, |x| x.italic = Some(false)),
                                    "underline" => obj::set(&mut patch, key, |x| x.underline = Some(false)),
                                    "strike" => obj::set(&mut patch, key, |x| x.strike = Some(false)),
                                    "allCaps" => obj::set(&mut patch, key, |x| x.all_caps = Some(false)),
                                    "smallCaps" => obj::set(&mut patch, key, |x| x.small_caps = Some(false)),
                                    _ => obj::set_undefined(&mut patch, key, |_| {}),
                                }
                            }
                        }
                    }
                }
                let has_patch = !obj::key_indices(&patch).is_empty();
                let rev = self.new_rev();
                let at = DocPosition { para: pi as u32, offset: 0 };
                let cp = cleared_para(id.as_deref());
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                crate::session::revisions::track_apply_para_props(paras, DocRange { start: at, end: at }, &cp, &rev)?;
                if has_patch {
                    guarded::track_apply_run_props(
                        paras,
                        DocRange { start: at, end: DocPosition { para: pi as u32, offset: len as u32 } },
                        &patch,
                        &rev,
                    )?;
                }
                self.touch_para(pi);
                continue;
            }
            // direct PARAGRAPH formatting yields to the style
            let uniform: Vec<&'static str> = {
                let runs = &p.runs;
                RUN_KEYS
                    .iter()
                    .copied()
                    .filter(|key| {
                        let Some(first_run) = runs.first() else { return false };
                        let first = run_slot(&first_run.props, key);
                        // `first !== undefined && first !== false`
                        first.is_some() && first.as_deref() != Some("false") && runs.iter().all(|r| run_slot(&r.props, key) == first)
                    })
                    .collect()
            };
            let len = para_length(p);
            if let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(pi)) {
                text::clear_para(p);
                let pr = Arc::make_mut(&mut p.props);
                match &id {
                    Some(i) => obj::set(pr, "styleId", |x| x.style_id = Some(i.as_str().into())),
                    None => obj::set_undefined(pr, "styleId", |x| x.style_id = None),
                }
            }
            if !uniform.is_empty() {
                let r = DocRange {
                    start: DocPosition { para: pi as u32, offset: 0 },
                    end: DocPosition { para: pi as u32, offset: len as u32 },
                };
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                text::clear_run_formatting(paras, r, &ClearOptions { fields: Some(uniform), char_style: CharStyleOpt::Keep })?;
            }
            self.touch_para(pi);
        }
        Ok(())
    }

    /// `applyCharStyle(id)`: the character style on the stretch (at the caret: the word; outside a
    /// word: the text typed next), without its direct formatting.
    fn apply_char_style(&mut self, id: Option<&str>) -> Result<(), CommandError> {
        let r = if self.has_selection() { Some(self.range()) } else { self.word_range_at(self.selection.focus, false) };
        let Some(r) = r else {
            let mut p = default_run_props();
            if let Some(i) = id.filter(|s| !s.is_empty()) {
                obj::set(&mut p, "charStyle", |x| x.char_style = Some(i.into()));
            }
            self.pending_props = Some(CarriedProps::plain(p));
            return Ok(());
        };
        if self.opts.tracking {
            let rev = self.new_rev();
            let patch = cleared_run(id);
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_apply_run_props(paras, r, &patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            let cs = match id {
                Some(i) => CharStyleOpt::Set(i.into()),
                None => CharStyleOpt::Remove,
            };
            text::clear_run_formatting(paras, r, &ClearOptions { fields: None, char_style: cs })?;
        }
        self.touch_range(r);
        self.ensure_style_used(id);
        Ok(())
    }

    /// `selectionFormat(type)`: the format of the first character and paragraph of the selection.
    fn selection_format(&mut self, character_only: bool) -> StyleFormat {
        let DocRange { start, .. } = normalize_range(self.range());
        let run = if self.has_selection() {
            self.paras().map(|ps| text::props_at(ps, DocPosition { para: start.para, offset: start.offset + 1 })).unwrap_or_default()
        } else {
            self.pending_props.clone().unwrap_or_else(|| self.current_props_f2())
        };
        let r = &run.props;
        let mut f = StyleFormat::default();
        if let Some(font) = r.font_ascii.as_deref().filter(|s| !s.is_empty()) {
            f.font_ascii = Ch::Val(font.to_owned());
        }
        if let Some(s) = r.sz_half_pt {
            f.sz_half_pt = Ch::Val(s.to_number());
        }
        if r.bold == Some(true) {
            f.bold = Ch::Val(true);
        }
        if r.italic == Some(true) {
            f.italic = Ch::Val(true);
        }
        if r.underline == Some(true) && r.link.as_deref().is_none_or(str::is_empty) {
            f.underline = Ch::Val(true);
        }
        if r.strike == Some(true) {
            f.strike = Ch::Val(true);
        }
        if r.all_caps == Some(true) {
            f.all_caps = Ch::Val(true);
        }
        if r.small_caps == Some(true) {
            f.small_caps = Ch::Val(true);
        }
        if let Some(c) = r.color.as_deref().filter(|c| !c.is_empty())
            && !(r.link.as_deref().is_some_and(|l| !l.is_empty()) && c == "0563C1")
        {
            f.color = Ch::Val(c.to_owned());
        }
        if let Some(c) = r.char_spacing_twips.filter(|c| c.truthy()) {
            f.char_spacing_twips = Ch::Val(c.to_number());
        }
        if character_only {
            return f;
        }
        let Some(pp) = self.paras().and_then(|ps| ps.get(start.para as usize)).map(|p| Arc::clone(&p.props)) else { return f };
        if let Some(a) = pp.alignment.as_deref().filter(|a| !a.is_empty() && *a != "left") {
            f.alignment = Ch::Val(a.to_owned());
        }
        if let Some(v) = pp.space_before_twips {
            f.space_before_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.space_after_twips {
            f.space_after_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.line_twentieths {
            f.line_twentieths = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_left_twips.filter(|v| v.truthy()) {
            f.indent_left_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_right_twips.filter(|v| v.truthy()) {
            f.indent_right_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_first_line_twips.filter(|v| v.truthy()) {
            f.indent_first_line_twips = Ch::Val(v.to_number());
        }
        f
    }

    /// `createStyle(opts)`: a style (linked by default, in the gallery, based on the style of the
    /// current paragraph, with the formatting of the first character and paragraph of the
    /// selection) applied to the selection. The styleId (as Word generates it), or `None` when the
    /// name exists.
    pub fn create_style(&mut self, spec: &CreateStyleSpec) -> Result<Option<String>, CommandError> {
        if !self.can_edit() {
            return Ok(None);
        }
        let name = uo_jsnum::text::trim(&spec.name).to_owned();
        if name.is_empty() || self.style_name_taken(&name, None) {
            return Ok(None);
        }
        let ty = spec.kind.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| "linked".to_owned());
        let format = match &spec.format {
            Some(f) => f.clone(),
            None if spec.from_selection != Some(false) => self.selection_format(ty == "character"),
            None => StyleFormat::default(),
        };
        let based_on = match &spec.based_on {
            Some(b) => Some(b.clone()),
            None if ty == "character" => None,
            None => {
                let dp = self.default_paragraph();
                let sid = self.paras().and_then(|ps| ps.get(self.selection.focus.para as usize)).and_then(|p| p.props.style_id.clone());
                Some(sid.map_or(dp, |s| s.to_string()))
            }
        };
        self.begin_step(false, &[]);
        let ds = self.style_defaults();
        let o = CreateStyle { name, ty, based_on, next: spec.next.clone(), quick: spec.quick, auto_update: spec.auto_update, format };
        let (xml, id) = xform::create_style_xml(&ds, &self.styles_base(), &o);
        self.commit_styles(xml);
        if spec.apply != Some(false) {
            self.apply_style_core(Some(&id))?;
        }
        self.pending_props = None;
        Ok(Some(id))
    }

    /// `modifyStyle(id, opts)`: name, base, next, gallery, auto-update and the formatting
    /// (`format`: value = set in the style, null = inherit). One undo step.
    pub fn modify_style(&mut self, id: &str, spec: &ModifyStyleSpec) -> Result<bool, CommandError> {
        if !self.can_edit() {
            return Ok(false);
        }
        if let Some(n) = spec.name.as_deref().map(uo_jsnum::text::trim)
            && !n.is_empty()
            && self.style_name_taken(n, Some(id))
        {
            return Ok(false);
        }
        let id = xform::resolve_style_id(self.styles.xml().map(|x| &**x), id);
        self.begin_step(false, &[]);
        let o = ModifyStyle {
            name: spec.name.clone(),
            based_on: spec.based_on.clone(),
            next: spec.next.clone(),
            quick: spec.quick,
            auto_update: spec.auto_update,
            format: spec.format.clone(),
        };
        let r = xform::modify_style_xml(&self.styles_base(), &id, &o);
        // a name in use at the rename: the TS throws AFTER the step was taken (the step stays)
        let Some((xml, renamed)) = r else { return Ok(false) };
        if self.styles.xml().is_some_and(|c| **c == *xml) {
            self.discard_step();
            return Ok(false);
        }
        self.commit_styles(xml);
        self.retarget_styles(&renamed, &[]);
        Ok(true)
    }

    /// `setDefaultFont(f)`: "Set as Default" of the Font dialog writes the font into the rPr of the
    /// default paragraph style (Normal), not into `w:docDefaults` (measured).
    pub fn set_default_font(&mut self, f: &DefaultFont) -> Result<bool, CommandError> {
        let mut format = StyleFormat::default();
        if let Some(v) = &f.font_family {
            format.font_ascii = Ch::Val(v.clone());
        }
        if let Some(v) = f.size_pt {
            format.sz_half_pt = Ch::Val(uo_jsnum::math::round(v * 2.0));
        }
        if let Some(v) = f.bold {
            format.bold = Ch::Val(v);
        }
        if let Some(v) = f.italic {
            format.italic = Ch::Val(v);
        }
        if let Some(v) = f.underline {
            format.underline = Ch::Val(v);
        }
        if let Some(v) = f.strike {
            format.strike = Ch::Val(v);
        }
        if let Some(v) = f.all_caps {
            format.all_caps = Ch::Val(v);
        }
        if let Some(v) = f.small_caps {
            format.small_caps = Ch::Val(v);
        }
        format.color = f.color.clone();
        if let Some(v) = f.spacing_twips {
            format.char_spacing_twips = if v.is_nan() || v == 0.0 { Ch::Null } else { Ch::Val(v) };
        }
        let dp = self.default_paragraph();
        self.modify_style(&dp, &ModifyStyleSpec { format, ..ModifyStyleSpec::default() })
    }

    /// `updateStyleFromSelection(id?)`: the direct formatting of the current paragraph (the
    /// paragraph's and the uniform character formatting) goes into the style and out of the
    /// paragraph. Default: the style of the current paragraph.
    pub fn update_style_from_selection(&mut self, id: Option<&str>) -> Result<bool, CommandError> {
        if !self.can_edit() {
            return Ok(false);
        }
        let pi = self.selection.focus.para as usize;
        let Some(own_style) = self.paras().and_then(|ps| ps.get(pi)).map(|p| p.props.style_id.clone()) else { return Ok(false) };
        let dp = self.default_paragraph();
        let want = id.map(str::to_owned).or_else(|| own_style.as_ref().map(|s| s.to_string())).unwrap_or_else(|| dp.clone());
        let target = xform::resolve_style_id(self.styles.xml().map(|x| &**x), &want);
        let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { return Ok(false) };
        let pp = Arc::clone(&p.props);
        let mut format = StyleFormat::default();
        if let Some(a) = pp.alignment.as_deref().filter(|a| !a.is_empty()) {
            format.alignment = Ch::Val(a.to_owned());
        }
        if let Some(v) = pp.space_before_twips {
            format.space_before_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.space_after_twips {
            format.space_after_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.line_twentieths {
            format.line_twentieths = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_left_twips {
            format.indent_left_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_right_twips {
            format.indent_right_twips = Ch::Val(v.to_number());
        }
        if let Some(v) = pp.indent_first_line_twips {
            format.indent_first_line_twips = Ch::Val(v.to_number());
        }
        let runs = &p.runs;
        let mut fields: Vec<&'static str> = Vec::new();
        // `uniform(k)`: the first run's value when every run has it
        let uniform = |k: &str| -> Option<String> {
            let first = run_slot(&runs.first()?.props, k)?;
            runs.iter().all(|r| run_slot(&r.props, k).as_deref() == Some(first.as_str())).then_some(first)
        };
        let first_props = runs.first().map(|r| Arc::clone(&r.props));
        let get = |k: &str| uniform(k).and(first_props.as_deref());
        if let Some(r) = get("fontAscii")
            && let Some(v) = &r.font_ascii
        {
            format.font_ascii = Ch::Val(v.to_string());
            fields.push("fontAscii");
        }
        if let Some(r) = get("szHalfPt")
            && let Some(v) = r.sz_half_pt
        {
            format.sz_half_pt = Ch::Val(v.to_number());
            fields.push("szHalfPt");
        }
        if let Some(r) = get("color")
            && let Some(v) = &r.color
        {
            format.color = Ch::Val(v.to_string());
            fields.push("color");
        }
        if let Some(r) = get("charSpacingTwips")
            && let Some(v) = r.char_spacing_twips
        {
            format.char_spacing_twips = Ch::Val(v.to_number());
            fields.push("charSpacingTwips");
        }
        for (k, set) in [
            ("bold", &mut format.bold),
            ("italic", &mut format.italic),
            ("underline", &mut format.underline),
            ("strike", &mut format.strike),
            ("allCaps", &mut format.all_caps),
            ("smallCaps", &mut format.small_caps),
        ] {
            if let Some(r) = get(k) {
                let v = match k {
                    "bold" => r.bold,
                    "italic" => r.italic,
                    "underline" => r.underline,
                    "strike" => r.strike,
                    "allCaps" => r.all_caps,
                    _ => r.small_caps,
                };
                if v == Some(true) {
                    *set = Ch::Val(true);
                    fields.push(match k {
                        "bold" => "bold",
                        "italic" => "italic",
                        "underline" => "underline",
                        "strike" => "strike",
                        "allCaps" => "allCaps",
                        _ => "smallCaps",
                    });
                }
            }
        }
        let has_runs = !runs.is_empty();
        let len = para_length(p);
        self.begin_step(false, &[]);
        let o = ModifyStyle { format, ..ModifyStyle::default() };
        let base = self.styles_base();
        if let Some((xml, _)) = xform::modify_style_xml(&base, &target, &o) {
            self.commit_styles(xml);
        }
        if let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(pi)) {
            text::clear_para(p);
            if target != dp {
                obj::set(Arc::make_mut(&mut p.props), "styleId", |x| x.style_id = Some(target.as_str().into()));
            }
        }
        if !fields.is_empty() && has_runs {
            let r =
                DocRange { start: DocPosition { para: pi as u32, offset: 0 }, end: DocPosition { para: pi as u32, offset: len as u32 } };
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            text::clear_run_formatting(paras, r, &ClearOptions { fields: Some(fields), char_style: CharStyleOpt::Keep })?;
        }
        self.touch_para(pi);
        Ok(true)
    }

    /// `renameStyle(id, name)`: a custom style changes name AND styleId (the document follows); a
    /// built-in gets an alias (`w:aliases`), as Word.
    pub fn rename_style(&mut self, id: &str, name: &str) -> Result<bool, CommandError> {
        let n = uo_jsnum::text::trim(name).to_owned();
        if !self.can_edit() || n.is_empty() || self.style_name_taken(&n, Some(id)) {
            return Ok(false);
        }
        let id = xform::resolve_style_id(self.styles.xml().map(|x| &**x), id);
        self.begin_step(false, &[]);
        let r = xform::rename_style_xml(&self.styles_base(), &id, &n);
        // a name in use at the rename: the TS throws AFTER the step was taken (the step stays)
        let Some((xml, renamed)) = r else { return Ok(false) };
        if self.styles.xml().is_some_and(|c| **c == *xml) {
            self.discard_step();
            return Ok(false);
        }
        self.commit_styles(xml);
        self.retarget_styles(&renamed, &[]);
        Ok(true)
    }

    /// `deleteStyle(id)`: deletes a CUSTOM style: the paragraphs go back to Normal and the runs lose
    /// the character style. A built-in is not deleted (`false`).
    pub fn delete_style(&mut self, id: &str) -> Result<bool, CommandError> {
        if !self.can_edit() {
            return Ok(false);
        }
        let Some(xml) = self.styles.xml().map(|x| x.to_string()) else { return Ok(false) };
        let Some((out, removed)) = xform::delete_style_xml(&xml, id) else { return Ok(false) };
        self.begin_step(false, &[]);
        self.commit_styles(out);
        self.retarget_styles(&[], &removed);
        Ok(true)
    }

    /// `setStyleInGallery(id, on)`.
    fn set_style_in_gallery(&mut self, id: &str, on: bool) -> Result<(), CommandError> {
        if !self.can_edit() {
            return Ok(());
        }
        let rid = xform::resolve_style_id(self.styles.xml().map(|x| &**x), id);
        let xml = xform::set_quick_xml(&self.styles_base(), &rid, on);
        if self.styles.xml().is_some_and(|c| **c == *xml) {
            return Ok(());
        }
        self.begin_step(false, &[]);
        self.commit_styles(xml);
        Ok(())
    }

    /// `retargetStyles(renamed, removed)`: renamed/removed styles: the paragraphs and runs follow.
    fn retarget_styles(&mut self, renamed: &[(String, String)], removed: &[String]) {
        if renamed.is_empty() && removed.is_empty() {
            return;
        }
        let t = Targets { renamed, gone: removed };
        retarget_story(&mut self.doc.body, &t);
        for i in 0..self.doc.hf.len() {
            if self.doc.hf.get(i).is_some_and(|h| story_hits(&h.story, &t))
                && let Some(h) = self.doc.hf.make_mut(i)
            {
                retarget_story(&mut h.story, &t);
            }
        }
        let hits = |m: &crate::js::JsMap<crate::js::NumKey, Story>| m.values().any(|s| story_hits(s, &t));
        if hits(&self.doc.notes.footnotes) || hits(&self.doc.notes.endnotes) {
            let notes = Arc::make_mut(&mut self.doc.notes);
            for s in notes.footnotes.values_mut().chain(notes.endnotes.values_mut()) {
                retarget_story(s, &t);
            }
        }
        self.touch_all();
    }

    // ───────────────────────── the hook of the text family ─────────────────────────

    /// `applyNextStyle(prev, next)`: Enter at the end of a paragraph: the new one takes the style's
    /// "next" (`w:next`), without direct formatting, if the style has a next other than itself.
    /// `at` is the paragraph that ended (the new one is `at + 1`).
    pub(crate) fn apply_next_style(&mut self, at: usize) -> Result<(), CommandError> {
        let Some(prev_style) = self.paras().and_then(|ps| ps.get(at)).map(|p| p.props.style_id.clone()) else { return Ok(()) };
        if self.paras().and_then(|ps| ps.get(at + 1)).is_none() {
            return Ok(());
        }
        let dp = self.default_paragraph();
        let sid = prev_style.map_or_else(|| dp.clone(), |s| s.to_string());
        let Some(next) = self.meta(&sid).and_then(|m| m.next).filter(|n| !n.is_empty() && *n != sid) else { return Ok(()) };
        let style: Option<String> = if next == dp { None } else { Some(next) };
        if let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(at + 1)) {
            text::clear_para(p);
            let pr = Arc::make_mut(&mut p.props);
            match &style {
                Some(s) => obj::set(pr, "styleId", |x| x.style_id = Some(s.as_str().into())),
                None => obj::set_undefined(pr, "styleId", |x| x.style_id = None),
            }
        }
        self.touch_para(at + 1);
        self.ensure_style_used(style.as_deref());
        self.pending_props = Some(CarriedProps::plain(default_run_props()));
        Ok(())
    }
}

// ───────────────────────── retargetStyles ─────────────────────────

struct Targets<'a> {
    renamed: &'a [(String, String)],
    gone: &'a [String],
}

impl Targets<'_> {
    fn renamed(&self, s: &str) -> Option<&str> {
        self.renamed.iter().find(|(f, _)| f == s).map(|(_, t)| t.as_str())
    }

    fn hits(&self, s: &str) -> bool {
        !s.is_empty() && (self.renamed(s).is_some() || self.gone.iter().any(|g| g == s))
    }
}

fn para_hits(p: &Paragraph, t: &Targets<'_>) -> bool {
    p.props.style_id.as_deref().is_some_and(|s| t.hits(s))
        || p.runs.iter().any(|r| {
            r.props.char_style.as_deref().is_some_and(|c| t.hits(c))
                || r.ext().and_then(|e| e.textbox.as_deref()).is_some_and(|tb| story_hits(&tb.content, t))
        })
}

fn story_hits(s: &Story, t: &Targets<'_>) -> bool {
    s.paras.iter().any(|p| para_hits(p, t))
        || s.tables.iter().any(|a| a.table.rows.iter().any(|row| row.cells.iter().any(|c| story_hits(&c.content, t))))
}

fn retarget_story(s: &mut Story, t: &Targets<'_>) {
    for i in 0..s.paras.len() {
        if s.paras.get(i).is_some_and(|p| para_hits(p, t))
            && let Some(p) = s.paras.make_mut(i)
        {
            let cur: Option<Str> = p.props.style_id.clone();
            if let Some(cur) = cur.filter(|c| t.hits(c)) {
                let pr = Arc::make_mut(&mut p.props);
                match t.renamed(&cur) {
                    Some(to) => obj::set(pr, "styleId", |x| x.style_id = Some(to.into())),
                    None => obj::delete(pr, "styleId"),
                }
            }
            for r in &mut p.runs {
                let c: Option<Str> = r.props.char_style.clone();
                if let Some(c) = c.filter(|c| t.hits(c)) {
                    let pr = Arc::make_mut(&mut r.props);
                    match t.renamed(&c) {
                        Some(to) => obj::set(pr, "charStyle", |x| x.char_style = Some(to.into())),
                        None => obj::delete(pr, "charStyle"),
                    }
                }
                if let Some(tb) = r.ext.as_deref_mut().and_then(|e| e.textbox.as_deref_mut()) {
                    retarget_story(&mut tb.content, t);
                }
            }
        }
    }
    for i in 0..s.tables.len() {
        if s.tables.get(i).is_some_and(|a| a.table.rows.iter().any(|row| row.cells.iter().any(|c| story_hits(&c.content, t))))
            && let Some(a) = s.tables.make_mut(i)
        {
            for row in &mut Arc::make_mut(&mut a.table).rows {
                for cell in &mut row.cells {
                    retarget_story(&mut cell.content, t);
                }
            }
        }
    }
}

/// The pure model files of the family, for the parity capture of `styles.ts`
/// (`js/capture-d2b-f2-ops.mjs`): the transformations of the style sheet text. Not part of the API.
#[doc(hidden)]
pub mod testing {
    use super::ModifyStyleSpec;
    use super::sheet::{StyleFormat, read_style_index};
    use super::xform::{self, CreateStyle, Defaults, ModifyStyle};

    #[must_use]
    pub fn ensure_defined(xml: &str, id: &str, used: bool) -> String {
        xform::ensure_defined(xml, id, used)
    }

    /// `createStyleXml(new DocStyles(xml), xml, { name, type, format, basedOn })`: the text and the id.
    #[must_use]
    pub fn create(xml: &str, name: &str, ty: &str, format: StyleFormat, based_on: Option<&str>) -> (String, String) {
        let ds = Defaults::of(&read_style_index(xml));
        let o = CreateStyle {
            name: name.to_owned(),
            ty: ty.to_owned(),
            based_on: based_on.map(str::to_owned),
            format,
            ..CreateStyle::default()
        };
        xform::create_style_xml(&ds, xml, &o)
    }

    #[must_use]
    pub fn modify(xml: &str, id: &str, s: &ModifyStyleSpec) -> Option<(String, Vec<(String, String)>)> {
        let o = ModifyStyle {
            name: s.name.clone(),
            based_on: s.based_on.clone(),
            next: s.next.clone(),
            quick: s.quick,
            auto_update: s.auto_update,
            format: s.format.clone(),
        };
        xform::modify_style_xml(xml, id, &o)
    }

    #[must_use]
    pub fn rename(xml: &str, id: &str, name: &str) -> Option<(String, Vec<(String, String)>)> {
        xform::rename_style_xml(xml, id, name)
    }

    #[must_use]
    pub fn delete(xml: &str, id: &str) -> Option<(String, Vec<String>)> {
        xform::delete_style_xml(xml, id)
    }

    #[must_use]
    pub fn set_quick(xml: &str, id: &str, on: bool) -> String {
        xform::set_quick_xml(xml, id, on)
    }

    #[must_use]
    pub fn resolve(xml: &str, id: &str) -> String {
        xform::resolve_style_id(Some(xml), id)
    }
}
