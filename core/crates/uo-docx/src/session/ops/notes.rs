//! D2b family: notes (frontier D2b-F5): `insertFootnote`/`insertEndnote`, `setNoteOptions`,
//! `convertNotes`, the note navigation (`openNote`, `goToNoteRef`, `nextNote`, `showNotes`), the
//! note settings (`noteSettings` and its save through `patchSettingsNotes`) and the pure rules of
//! the editor's `notes.ts` (`effectiveNoteOptions`, `notePropsOf`, `ensureNoteStyles`,
//! `nextNoteId`, `convertNoteContent`). TS sources: `universal-write.ts` 18789–19240 (the note
//! block), 8629–8634 (the save), 2402–2406 (the install), 18847 (`syncNoteSettings`), editor
//! `notes.ts`, docx-core `notes.ts` (the emitters are W4's: `write::emit::notes`).
//!
//! The note being edited is the context story ([`EditContext::Note`], D2a): every helper reaches
//! the content of a note through [`note_story`]/[`note_story_mut`], the seam the note editing list
//! (the TS flattens the cells of a note into its editing paragraphs, D2a-C1) would replace.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use uo_opc::Package;

use super::super::command::{CommandError, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::settle;
use super::super::state::{CellHop, Selection};
use super::super::text::{CarriedProps, DocPosition, normalize_range};
use super::super::{ExecCx, Session};
use super::context::{EditContext, set_note_story, story_of_mut};
use super::styles::sheet::{put_style, read_style_index};
use super::styles::xform::Defaults;
use crate::error::DocxError;
use crate::js::{JsNumber, JsonWriter, NumKey, Str};
use crate::model::Document;
use crate::model::formatting::{NoteProps, Settings};
use crate::model::para::Paragraph;
use crate::model::run::Run;
use crate::model::section::SectPr;
use crate::model::story::Story;
use crate::text::Utf16;
use crate::word::obj;
use crate::write::emit::notes::{
    NotePropsArg, ensure_note_parts, new_note_paragraphs, note_mark_field, note_ref_run, note_styles, patch_settings_notes,
};

// ───────────────────────── the options ─────────────────────────

/// The note options the dialog shows (`NoteOptions`): footnotes have the place `pageBottom` |
/// `beneathText` and columns, endnotes `sectEnd` | `docEnd`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteOptions {
    pub position: String,
    /// decimal | lowerLetter | upperLetter | lowerRoman | upperRoman | chicago
    pub number_format: String,
    pub start_at: f64,
    /// continuous | eachSect | eachPage (eachPage only for footnotes)
    pub restart: String,
    /// footnotes only: the columns of the notes area (0 = the section's)
    pub columns: Option<f64>,
}

/// `Partial<NoteOptions>`: the keys the shell gives (`None` = absent).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoteOptionsPatch {
    pub position: Option<String>,
    pub number_format: Option<String>,
    pub start_at: Option<f64>,
    pub restart: Option<String>,
    pub columns: Option<f64>,
}

/// `setNoteOptions`' `scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteScope {
    Section,
    #[default]
    Document,
}

/// `convertNotes`' `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertMode {
    ToEndnotes,
    ToFootnotes,
    Swap,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum NoteCmd {
    /// `insertFootnote({ mark })` / `insertEndnote({ mark })`: a note at the end of the selection
    /// and the caret in it. `mark`: a custom (not numbered) mark. The id is the one of the
    /// `NoteMode` event that follows.
    Insert { footnote: bool, mark: Option<Utf16> },
    /// `setNoteOptions(kind, o, scope)`
    SetOptions { footnote: bool, patch: NoteOptionsPatch, scope: NoteScope },
    /// `convertNotes(mode, only?)`: `only` = `(footnote, id)` of the one note to convert; the
    /// number of notes converted is [`Session::last_converted_notes`]
    Convert { mode: ConvertMode, only: Option<(bool, f64)> },
    /// `openNote(kind, id)`: the note for editing, the caret at the end of its text
    Open { footnote: bool, id: f64 },
    /// `goToNoteRef(kind, id)`: the caret right after the reference in the text
    GoToRef { footnote: bool, id: f64 },
    /// `nextNote(kind, dir)`
    Next { footnote: bool, forward: bool },
    /// `showNotes(kind?)`
    Show { footnote: Option<bool> },
}

impl NoteCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads:
    /// the note methods are not guarded.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}

// ───────────────────────── the settings state ─────────────────────────

/// `noteSettings`: `{ footnote?: NoteProps; endnote?: NoteProps }`. The entries are the object's
/// keys in insertion order (`JSON.stringify(noteSettings)` decides whether the save writes the
/// settings, and `{ ...ns, [kind]: p }` appends a new key).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoteSettings {
    entries: Vec<(bool, NoteProps)>,
}

impl NoteSettings {
    /// The install: `{ ...(settings.footnotePr ? { footnote } : {}), ...(endnotePr ? { endnote } : {}) }`.
    #[must_use]
    pub fn of_settings(s: &Settings) -> NoteSettings {
        let mut entries = Vec::new();
        if let Some(p) = &s.footnote_pr {
            entries.push((true, p.clone()));
        }
        if let Some(p) = &s.endnote_pr {
            entries.push((false, p.clone()));
        }
        NoteSettings { entries }
    }

    #[must_use]
    pub fn get(&self, footnote: bool) -> Option<&NoteProps> {
        self.entries.iter().find(|(f, _)| *f == footnote).map(|(_, p)| p)
    }

    /// `{ ...ns, [kind]: p }` (`Some`) or the copy without the key (`delete`).
    #[must_use]
    pub fn with(&self, footnote: bool, props: Option<NoteProps>) -> NoteSettings {
        let mut entries = self.entries.clone();
        match (props, entries.iter().position(|(f, _)| *f == footnote)) {
            (Some(p), Some(i)) => {
                if let Some(e) = entries.get_mut(i) {
                    e.1 = p;
                }
            }
            (Some(p), None) => entries.push((footnote, p)),
            (None, Some(i)) => {
                entries.remove(i);
            }
            (None, None) => {}
        }
        NoteSettings { entries }
    }

    /// `JSON.stringify(noteSettings)`.
    #[must_use]
    pub fn json(&self) -> String {
        let mut out = String::from("{");
        for (i, (f, p)) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(if *f { "\"footnote\":" } else { "\"endnote\":" });
            out.push_str(&JsonWriter::stringify(p, None).unwrap_or_else(|| "null".to_owned()));
        }
        out.push('}');
        out
    }
}

/// The notes family's state of a session.
#[derive(Debug, Default)]
pub struct F5State {
    /// `noteSettings` as installed (`noteSettingsLoaded` is its JSON)
    loaded: NoteSettings,
    loaded_json: String,
    /// the number `convertNotes` returned the last time
    last_converted: u32,
}

impl F5State {
    /// `installFullDoc`: `noteSettings` from the file's settings.
    pub(crate) fn install(doc: &Document) -> F5State {
        let loaded = NoteSettings::of_settings(&doc.formatting.settings);
        let loaded_json = loaded.json();
        F5State { loaded, loaded_json, last_converted: 0 }
    }

    pub(crate) fn loaded_json(&self) -> &str {
        &self.loaded_json
    }
}

/// `patchSettingsNotes(settings, { footnote: np.footnote ?? null, endnote: np.endnote ?? null })`
/// into `word/settings.xml` (nothing without that part, as the TS).
pub(crate) fn save_note_settings(pkg: &mut Package, ns: &NoteSettings) -> Result<(), DocxError> {
    let Some(xml) = pkg.part_text("word/settings.xml")?.map(Cow::into_owned).filter(|t| !t.is_empty()) else { return Ok(()) };
    fn arg(p: Option<&NoteProps>) -> NotePropsArg<'_> {
        p.map_or(NotePropsArg::Remove, NotePropsArg::Set)
    }
    let next = patch_settings_notes(&xml, arg(ns.get(true)), arg(ns.get(false)));
    pkg.set_part_text("word/settings.xml", next);
    Ok(())
}

// ───────────────────────── the pure rules (editor notes.ts) ─────────────────────────

fn kind_name(footnote: bool) -> &'static str {
    if footnote { "footnote" } else { "endnote" }
}

/// `NOTE_DEFAULTS[kind]`.
fn defaults(footnote: bool) -> NoteOptions {
    if footnote {
        NoteOptions {
            position: "pageBottom".into(),
            number_format: "decimal".into(),
            start_at: 1.0,
            restart: "continuous".into(),
            columns: Some(0.0),
        }
    } else {
        NoteOptions {
            position: "docEnd".into(),
            number_format: "lowerRoman".into(),
            start_at: 1.0,
            restart: "continuous".into(),
            columns: None,
        }
    }
}

/// `effectiveNoteOptions(kind, doc, sect, cols)`: the section's over the document's (settings),
/// with the defaults of Word.
#[must_use]
pub fn effective_note_options(footnote: bool, doc: Option<&NoteProps>, sect: Option<&NoteProps>, cols: Option<f64>) -> NoteOptions {
    let d = defaults(footnote);
    // `{ ...doc, ...sect }`
    let pos =
        if footnote { sect.and_then(|s| s.pos.as_ref()).or(doc.and_then(|s| s.pos.as_ref())) } else { doc.and_then(|s| s.pos.as_ref()) };
    let fmt = sect.and_then(|s| s.num_fmt.as_ref()).or(doc.and_then(|s| s.num_fmt.as_ref()));
    let start = sect.and_then(|s| s.num_start).or(doc.and_then(|s| s.num_start));
    let restart = sect.and_then(|s| s.num_restart.as_ref()).or(doc.and_then(|s| s.num_restart.as_ref()));
    NoteOptions {
        position: pos.map_or(d.position, |s| s.to_string()),
        number_format: fmt.map_or(d.number_format, |s| s.to_string()),
        start_at: match start {
            Some(JsNumber::Num(v)) => v,
            _ => d.start_at,
        },
        restart: restart.map_or(d.restart, |s| s.to_string()),
        columns: footnote.then_some(cols.unwrap_or(0.0)),
    }
}

/// `notePropsOf(kind, o, withPos)`: the `NoteProps` Word writes for the options (the default is not
/// written; the place of the endnotes stays out: it is only in the settings).
#[must_use]
pub fn note_props_of(footnote: bool, o: &NoteOptions, with_pos: bool) -> Option<NoteProps> {
    let d = defaults(footnote);
    let mut p = NoteProps::default();
    let mut any = false;
    if with_pos && o.position != d.position {
        obj::set(&mut p, "pos", |x| x.pos = Some(Str::from(o.position.as_str())));
        any = true;
    }
    if o.number_format != d.number_format {
        obj::set(&mut p, "numFmt", |x| x.num_fmt = Some(Str::from(o.number_format.as_str())));
        any = true;
    }
    #[allow(clippy::float_cmp)]
    if o.start_at != d.start_at {
        obj::set(&mut p, "numStart", |x| x.num_start = Some(JsNumber::Num(o.start_at)));
        any = true;
    }
    if o.restart != d.restart {
        obj::set(&mut p, "numRestart", |x| x.num_restart = Some(Str::from(o.restart.as_str())));
        any = true;
    }
    any.then_some(p)
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

/// What `ensureNoteStyles` returns: the styles.xml text and the ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteStyleIds {
    pub xml: String,
    pub text: String,
    pub r#ref: String,
}

/// `ensureNoteStyles(ds, kind)` over the text of the sheet and the default style ids: the styles
/// the document already has by name, or the ones of Word pt-BR added as Word writes them when it
/// inserts the first note.
pub(crate) fn ensure_note_styles_xml(base: &str, d: &Defaults, footnote: bool) -> NoteStyleIds {
    let st = note_styles(kind_name(footnote)).copied();
    let Some(st) = st else { return NoteStyleIds { xml: base.to_owned(), text: String::new(), r#ref: String::new() } };
    let named = |xml: &str, name: &str| -> Option<String> {
        let k = lower(name);
        read_style_index(xml).styles.iter().find(|s| lower(&s.name) == k).map(|s| s.id.clone())
    };
    let map = |x: &str| -> String {
        x.replacen("<w:basedOn w:val=\"Normal\"/>", &format!("<w:basedOn w:val=\"{}\"/>", d.paragraph), 1).replacen(
            "<w:basedOn w:val=\"Fontepargpadro\"/>",
            &format!("<w:basedOn w:val=\"{}\"/>", d.character),
            1,
        )
    };
    let put = |xml: String, id: &str, style: &str| -> String { put_style(&xml, id, &map(style)).unwrap_or(xml) };
    let mut xml = base.to_owned();
    let text = match named(&xml, st.text.name) {
        Some(t) => t,
        None => {
            xml = put(xml, st.text.id, st.text.xml);
            if named(&xml, st.char.name).is_none() {
                xml = put(xml, st.char.id, st.char.xml);
            }
            st.text.id.to_owned()
        }
    };
    let r#ref = match named(&xml, st.r#ref.name) {
        Some(r) => r,
        None => {
            xml = put(xml, st.r#ref.id, st.r#ref.xml);
            st.r#ref.id.to_owned()
        }
    };
    NoteStyleIds { xml, text, r#ref }
}

/// `nextNoteId(full, kind)`: the next free id of a kind (ids from 1: -1 and 0 are the separators).
#[must_use]
pub fn next_note_id(doc: &Document, footnote: bool) -> f64 {
    let map = if footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
    let mut max = 0.0_f64;
    for k in map.keys() {
        max = uo_jsnum::math::max(max, k.get());
    }
    // (separator, continuation and notice notes have an id too)
    let special = if footnote { &doc.notes.special_ids.footnote } else { &doc.notes.special_ids.endnote };
    for id in special {
        max = uo_jsnum::math::max(max, id.to_number());
    }
    max + 1.0
}

/// The paragraphs of a story, cells included (`flattenBlocks`), for a change.
fn for_each_para_mut(story: &mut Story, depth: u32, f: &mut dyn FnMut(&mut Paragraph)) {
    for i in 0..story.paras.len() {
        if let Some(p) = story.paras.make_mut(i) {
            f(p);
        }
    }
    if depth >= 64 {
        return;
    }
    for i in 0..story.tables.len() {
        let Some(a) = story.tables.make_mut(i) else { continue };
        for row in &mut Arc::make_mut(&mut a.table).rows {
            for cell in &mut row.cells {
                for_each_para_mut(&mut cell.content, depth + 1, f);
            }
        }
    }
}

/// The note style ids of a kind, as `convertNoteContent` reads them.
pub(crate) struct StylePair<'a> {
    pub text: &'a str,
    pub r#ref: &'a str,
}

/// `convertNoteContent(blocks, to, styles)`: the mark and the note styles of the old kind become the
/// new kind's.
pub(crate) fn convert_note_content(story: &mut Story, to_footnote: bool, from: &StylePair<'_>, to: &StylePair<'_>) {
    for_each_para_mut(story, 0, &mut |p| {
        if !from.text.is_empty() && p.props.style_id.as_deref() == Some(from.text) {
            obj::set(Arc::make_mut(&mut p.props), "styleId", |x| x.style_id = Some(Str::from(to.text)));
        }
        for r in &mut p.runs {
            if r.ext().and_then(|e| e.field.as_deref()).is_some_and(settle::is_note_mark_field) {
                r.ext_mut().field = Some(Str::from(note_mark_field(kind_name(to_footnote))));
            }
            if !from.r#ref.is_empty() && r.props.char_style.as_deref() == Some(from.r#ref) {
                obj::set(Arc::make_mut(&mut r.props), "charStyle", |x| x.char_style = Some(Str::from(to.r#ref)));
            }
        }
    });
}

// ───────────────────────── the note story (the seam of D2a-C1) ─────────────────────────

/// `noteBlocks(full, kind, id)`: the content of a note.
#[must_use]
pub fn note_story(doc: &Document, footnote: bool, id: f64) -> Option<&Story> {
    let map = if footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
    map.get(&NumKey::new(id))
}

/// The content of a note, for a change.
pub fn note_story_mut(doc: &mut Document, footnote: bool, id: f64) -> Option<&mut Story> {
    story_of_mut(doc, &EditContext::Note { footnote, id, saved: Selection::default() })
}

// ───────────────────────── references in the body ─────────────────────────

/// A note reference in the body (`NoteRefLoc`): a paragraph of the body (`cell` `None`) or of a
/// cell of a top-level table (`cell` = the table's anchor and the hop of the cell).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRefLoc {
    pub footnote: bool,
    pub id: f64,
    pub para: u32,
    pub cell: Option<(u32, CellHop)>,
    pub offset: u32,
    /// the position in document order (`key`)
    pub key: Vec<f64>,
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `Math.max(1, c.tcPr?.gridSpan ?? c.gridSpan ?? 1)`.
fn grid_span(cell: &crate::model::table::Cell) -> f64 {
    let span = match cell.tc_pr.as_deref().and_then(|p| p.grid_span) {
        Some(JsNumber::Num(v)) => v,
        _ => match cell.grid_span {
            JsNumber::Num(v) => v,
            JsNumber::Null => 1.0,
        },
    };
    uo_jsnum::math::max(1.0, span)
}

/// The lexicographic comparison of two keys (missing = 0): the sign of the first difference.
fn cmp_key(a: &[f64], b: &[f64]) -> f64 {
    for i in 0..a.len().max(b.len()) {
        let d = a.get(i).copied().unwrap_or(0.0) - b.get(i).copied().unwrap_or(0.0);
        if d != 0.0 {
            return d;
        }
    }
    0.0
}

/// `trim()` over code units (the JS white space set).
fn trim_units(u: &[u16]) -> &[u16] {
    let is_space = uo_jsnum::text::is_js_space_unit;
    let start = u.iter().position(|&c| !is_space(c)).unwrap_or(u.len());
    let end = u.iter().rposition(|&c| !is_space(c)).map_or(start, |i| i + 1);
    u.get(start..end).unwrap_or(&[])
}

impl Session {
    // ── queries ────────────────────────────────────────────────────────────────────────────

    /// `editingNote()`: `(footnote, id)` of the note being edited.
    #[must_use]
    pub fn editing_note(&self) -> Option<(bool, f64)> {
        match &self.ctx {
            EditContext::Note { footnote, id, .. } => Some((*footnote, *id)),
            _ => None,
        }
    }

    /// The note options of the document as edited (`noteSettings`).
    #[must_use]
    pub fn note_settings(&self) -> &NoteSettings {
        self.settings.note_settings.as_ref().unwrap_or(&self.f5.loaded)
    }

    /// The number of notes the last `convertNotes` converted.
    #[must_use]
    pub fn last_converted_notes(&self) -> u32 {
        self.f5.last_converted
    }

    /// `canInsertNote()`: the body and its cells (Word refuses in headers/footers, text boxes and
    /// inside a note), not in the read view, not in a plain-text content control.
    #[must_use]
    pub fn can_insert_note(&self) -> bool {
        if self.paras().is_none()
            || self.locked()
            || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. })
        {
            return false;
        }
        !self.link_in_plain_text_control_at(&[normalize_range(self.range()).end])
    }

    /// `noteRefs(kind?)`: the references of the body (paragraphs and cells), in document order.
    #[must_use]
    pub fn note_refs(&self, kind: Option<bool>) -> Vec<NoteRefLoc> {
        let body = &self.doc.body;
        let mut out: Vec<NoteRefLoc> = Vec::new();
        let in_para = |p: &Paragraph, key: &[f64], para: u32, cell: Option<(u32, CellHop)>, out: &mut Vec<NoteRefLoc>| {
            let mut off = 0usize;
            for r in &p.runs {
                if let Some(n) = r.ext().and_then(|e| e.note_ref.as_ref()) {
                    let footnote = match &*n.kind {
                        "footnote" => Some(true),
                        "endnote" => Some(false),
                        _ => None,
                    };
                    if let Some(f) = footnote
                        && kind.is_none_or(|k| k == f)
                    {
                        let mut k = key.to_vec();
                        k.push(off as f64);
                        out.push(NoteRefLoc { footnote: f, id: n.id.to_number(), para, cell, offset: u32_of(off), key: k });
                    }
                }
                off += r.text.len();
            }
        };
        let n = body.paras.len();
        let mut ti = 0usize;
        for pi in 0..=n {
            while let Some(a) = body.tables.get(ti).filter(|a| a.before_para as usize <= pi) {
                let ai = u32_of(ti);
                let pp = f64::from(a.before_para);
                for (ri, row) in a.table.rows.iter().enumerate() {
                    let mut col = row.tr_pr.as_deref().and_then(|p| p.grid_before).map_or(0.0, |g| g.to_number());
                    for cell in &row.cells {
                        let hop = CellHop { row: u32_of(ri), col: uo_jsnum::conv::to_uint32(col), block: None };
                        for (k, p) in cell.content.paras.iter().enumerate() {
                            let key = [pp, 0.0, f64::from(ai), f64::from(hop.row), col, k as f64];
                            in_para(p, &key, u32_of(k), Some((ai, hop)), &mut out);
                        }
                        col += grid_span(cell);
                    }
                }
                ti += 1;
            }
            if let Some(p) = body.paras.get(pi) {
                in_para(p, &[pi as f64, 1.0, 0.0, 0.0, 0.0, 0.0], u32_of(pi), None, &mut out);
            }
        }
        out
    }

    /// `noteCount(kind)`: how many notes of a kind the document references.
    #[must_use]
    pub fn note_count(&self, footnote: bool) -> usize {
        self.note_refs(Some(footnote)).iter().map(|r| NumKey::new(r.id)).collect::<HashSet<_>>().len()
    }

    /// `bodyCaretPara()`: the body paragraph the caret is at (in a note, the one it was entered
    /// from; in a cell, the one before the table).
    fn body_caret_para(&self) -> u32 {
        match &self.ctx {
            EditContext::Note { saved, .. } => saved.focus.para,
            EditContext::Cell { anchor, .. } => self.doc.body.tables.get(*anchor as usize).map_or(0, |a| a.before_para),
            EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } => 0,
            EditContext::Body => self.selection.focus.para,
        }
    }

    /// The paragraphs of the body that close a section (`sectionRanges`' breaks).
    fn section_breaks(&self) -> Vec<usize> {
        self.doc.body.paras.iter().enumerate().filter(|(_, p)| p.props.section_break.is_some()).map(|(i, _)| i).collect()
    }

    /// `sectionIndexAt(model, para)`.
    fn section_index_at(breaks: &[usize], para: u32) -> usize {
        breaks.iter().position(|&b| para as usize <= b).unwrap_or(breaks.len())
    }

    /// `sectionSectPr(model, k)`.
    fn section_sect_pr(&self, breaks: &[usize], k: usize) -> &SectPr {
        match breaks.get(k).and_then(|&bi| self.doc.body.paras.get(bi)).and_then(|p| p.props.section_break.as_deref()) {
            Some(sb) => &sb.sect_pr,
            None => &self.sections.body_sect_pr,
        }
    }

    /// `sectionSectPr(model, k)` for a change. The TS mutates the object in place, and the object is
    /// also the `sectPr` of the effective section (`sectionStarts`, `fullDoc.sections`) until
    /// `syncSections` replaces it (it does not run in a cell or a note, and an undo puts new
    /// objects in the model but leaves the sections' ones). Here a change makes a new `Arc` (the
    /// undo step keeps the old one) and goes to the sections that held the old one, so what the
    /// layout and the save read follows the TS exactly.
    fn edit_sect_pr(&mut self, breaks: &[usize], k: usize, f: impl FnOnce(&mut SectPr)) {
        let old = match breaks.get(k) {
            Some(&bi) => {
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
        let (old, new) = old;
        if !Arc::ptr_eq(&old, &new) && self.doc.sections.iter().any(|s| Arc::ptr_eq(&s.sect_pr, &old)) {
            let v: Vec<crate::model::section::Section> = self
                .doc
                .sections
                .iter()
                .map(|s| {
                    if Arc::ptr_eq(&s.sect_pr, &old) {
                        crate::model::section::Section { sect_pr: Arc::clone(&new), ..s.clone() }
                    } else {
                        s.clone()
                    }
                })
                .collect();
            self.doc.sections = v.into();
        }
    }

    /// `getNoteOptions(kind)`: the options of the section of the caret.
    #[must_use]
    pub fn note_options(&self, footnote: bool) -> NoteOptions {
        let breaks = self.section_breaks();
        let k = Session::section_index_at(&breaks, self.body_caret_para());
        let sp = self.section_sect_pr(&breaks, k);
        let (sect, cols) =
            if footnote { (sp.footnote_pr.as_ref(), sp.footnote_columns.map(|c| c.to_number())) } else { (sp.endnote_pr.as_ref(), None) };
        effective_note_options(footnote, self.note_settings().get(footnote), sect, cols)
    }

    /// `syncNoteSettings()`: the options of the note settings in the formatting the engine reads.
    pub(crate) fn sync_note_settings(&mut self) {
        let (fp, ep) = {
            let ns = self.note_settings();
            let s = &self.doc.formatting.settings;
            if s.footnote_pr.as_ref() == ns.get(true) && s.endnote_pr.as_ref() == ns.get(false) {
                return;
            }
            (ns.get(true).cloned(), ns.get(false).cloned())
        };
        let s = &mut Arc::make_mut(&mut self.doc.formatting).settings;
        match fp {
            Some(p) => obj::set(s, "footnotePr", |x| x.footnote_pr = Some(p)),
            None => obj::delete(s, "footnotePr"),
        }
        match ep {
            Some(p) => obj::set(s, "endnotePr", |x| x.endnote_pr = Some(p)),
            None => obj::delete(s, "endnotePr"),
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
    }

    // ── the styles ─────────────────────────────────────────────────────────────────────────

    /// `ensureNoteStyles(this.docStyles, kind)`.
    fn ensure_note_styles(&mut self, footnote: bool) -> NoteStyleIds {
        let d = Defaults::of(&self.style_index());
        let base = self.styles_base();
        let st = ensure_note_styles_xml(&base, &d, footnote);
        // wave 7c (S, fix round 1; Word manda sobre o TS): the styles this defines stay in the saved file after an undo
        for (id, raw) in super::super::save_parts::added_note_styles(&st.xml, &base) {
            if !self.save_parts.note_styles.iter().any(|(k, _)| *k == id) {
                self.save_parts.note_styles.push((id, raw));
            }
        }
        st
    }

    // ── inserting ──────────────────────────────────────────────────────────────────────────

    /// `insertWholeRun(pos, run)`: tracked when the tracking is on.
    pub(crate) fn insert_whole_run(&mut self, pos: DocPosition, run: Run) -> Result<DocPosition, CommandError> {
        let out = if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_insert_run(paras, pos, run, &rev)?
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::insert_run(paras, pos, run)?
        };
        self.touch_para(pos.para as usize);
        Ok(out)
    }

    /// `insertNote(kind, opts)`.
    fn insert_note(&mut self, footnote: bool, mark: Option<&Utf16>) -> Result<Outcome, CommandError> {
        if !self.can_insert_note() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        let kind = kind_name(footnote);
        // the styles (the document's or Word's) and the parts, as Word writes them with the first
        // note (notes-lab fn-wb/en-wb)
        let st = self.ensure_note_styles(footnote);
        self.commit_styles(st.xml);
        // a part that cannot be read leaves the package as it is (the TS would throw); `if (this.pkg)` (19048; wave 7c,
        // S): not into an `.odt`'s scratch package
        if !self.pkg_scratch {
            let _ = ensure_note_parts(&mut self.pkg);
            self.save_parts.note_parts = true;
        }
        let id = next_note_id(&self.doc, footnote);
        let mark = mark.map(|m| trim_units(m.units())).filter(|m| !m.is_empty());
        let mut reference = note_ref_run(kind, id, Some(&st.r#ref));
        if mark.is_some()
            && let Some(n) = reference.ext_mut().note_ref.as_mut()
        {
            obj::set(n, "custom", |x| x.custom = Some(true));
        }
        let ref_props = (*reference.props).clone();
        let end = normalize_range(self.range()).end;
        let mut pos = self.insert_whole_run(end, reference)?;
        if let Some(m) = mark {
            pos = self.insert_typed(pos, m, Some(&CarriedProps::plain(ref_props)))?;
        }
        let mut paras = new_note_paragraphs(kind, Some(&st.text), Some(&st.r#ref), &mut self.doc.ids);
        if let Some(p) = paras.first_mut() {
            if let Some(m) = mark
                && let Some(r0) = p.runs.first_mut()
            {
                *r0 = Run { text: Utf16::from_units(m.to_vec()), props: Arc::new((*r0.props).clone()), ..Run::default() };
            }
            // tracking: the mark and the space enter as insertions (the paragraph mark does not)
            if self.opts.tracking {
                let rev = self.new_rev();
                for r in &mut p.runs {
                    let props = Arc::make_mut(&mut r.props);
                    obj::set(props, "ins", |x| x.ins = Some(Box::new(rev.clone())));
                }
            }
        }
        let mut story = Story::new();
        for p in paras {
            story.push_paragraph(p);
        }
        set_note_story(&mut self.doc, footnote, id, story);
        self.selection = Selection::caret(pos);
        // `refresh()` of the body, then into the note
        self.settle();
        self.enter_note(footnote, id, None, true);
        Ok(Outcome::Done(self.finish()))
    }

    // ── options ────────────────────────────────────────────────────────────────────────────

    /// `setNoteOptions(kind, o, scope)`.
    fn set_note_options(&mut self, footnote: bool, o: &NoteOptionsPatch, scope: NoteScope) -> Result<Outcome, CommandError> {
        if self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let cur = self.note_options(footnote);
        let mut next = NoteOptions {
            position: o.position.clone().unwrap_or(cur.position),
            number_format: o.number_format.clone().unwrap_or(cur.number_format),
            start_at: o.start_at.unwrap_or(cur.start_at),
            restart: o.restart.clone().unwrap_or(cur.restart),
            columns: o.columns.or(cur.columns),
        };
        if !footnote && next.restart == "eachPage" {
            next.restart = "continuous".into();
        }
        if next.restart != "continuous" {
            next.start_at = 1.0;
        }
        next.start_at = uo_jsnum::math::max(1.0, uo_jsnum::math::round(next.start_at));
        self.begin_step(false, &[]);
        let breaks = self.section_breaks();
        let n_sections = breaks.len() + 1;
        let k = Session::section_index_at(&breaks, self.body_caret_para());
        let targets: Vec<usize> = if scope == NoteScope::Document { (0..n_sections).collect() } else { vec![k] };
        let sect_props = note_props_of(footnote, &next, footnote);
        for i in targets {
            self.edit_sect_pr(&breaks, i, |sp| {
                if footnote {
                    match &sect_props {
                        Some(p) => obj::set(sp, "footnotePr", |x| x.footnote_pr = Some(p.clone())),
                        None => obj::delete(sp, "footnotePr"),
                    }
                    if o.columns.is_some() {
                        match next.columns.filter(|c| *c != 0.0 && !c.is_nan()) {
                            Some(c) => obj::set(sp, "footnoteColumns", |x| x.footnote_columns = Some(JsNumber::Num(c))),
                            None => obj::delete(sp, "footnoteColumns"),
                        }
                    }
                } else {
                    match &sect_props {
                        Some(p) => obj::set(sp, "endnotePr", |x| x.endnote_pr = Some(p.clone())),
                        None => obj::delete(sp, "endnotePr"),
                    }
                }
            });
        }
        // the settings: in the document, or when the section is the first (Word mirrors the first)
        if scope == NoteScope::Document || k == 0 || (!footnote && o.position.is_some()) {
            let whole = scope == NoteScope::Document || k == 0;
            let basis = if whole {
                next
            } else {
                NoteOptions {
                    position: next.position.clone(),
                    ..effective_note_options(footnote, self.note_settings().get(footnote), None, None)
                }
            };
            let doc_props = note_props_of(footnote, &basis, true);
            let ns = self.note_settings().with(footnote, doc_props);
            let mut step = (*self.settings).clone();
            step.note_settings = Some(ns);
            self.settings = Arc::new(step);
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        Ok(Outcome::Done(self.finish()))
    }

    // ── converting ─────────────────────────────────────────────────────────────────────────

    /// `convertNotes(mode, only?)`.
    fn convert_notes(&mut self, mode: ConvertMode, only: Option<(bool, f64)>) -> Result<Outcome, CommandError> {
        self.f5.last_converted = 0;
        if self.paras().is_none() || self.locked() || matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
            return Ok(Outcome::Done(self.change_set()));
        }
        let from = |k: bool| -> bool {
            match only {
                Some((ok, _)) => ok == k,
                None => mode == ConvertMode::Swap || (mode == ConvertMode::ToEndnotes) == k,
            }
        };
        let refs: Vec<NoteRefLoc> =
            self.note_refs(None).into_iter().filter(|r| from(r.footnote) && only.is_none_or(|(_, id)| r.id == id)).collect();
        if refs.is_empty() {
            return Ok(Outcome::Done(self.change_set()));
        }
        if matches!(self.ctx, EditContext::Note { .. }) {
            self.exit_note();
        }
        self.begin_step(false, &[]);
        // the two sheets: the second call starts from what the first one changed
        let fns = self.ensure_note_styles(true);
        self.commit_styles(fns.xml.clone());
        let ens = self.ensure_note_styles(false);
        self.commit_styles(ens.xml.clone());
        let ids = |footnote: bool| -> StylePair<'_> {
            let s = if footnote { &fns } else { &ens };
            StylePair { text: &s.text, r#ref: &s.r#ref }
        };
        let mut new_ids: Vec<((bool, NumKey), f64)> = Vec::new();
        let mut next = [next_note_id(&self.doc, true), next_note_id(&self.doc, false)];
        for r in &refs {
            let to = !r.footnote;
            let key = (r.footnote, NumKey::new(r.id));
            let nid = match new_ids.iter().find(|(k, _)| *k == key) {
                Some((_, n)) => *n,
                None => {
                    let slot = usize::from(!to);
                    let nid = next[slot];
                    next[slot] += 1.0;
                    new_ids.push((key, nid));
                    // a copy: undo returns the references to the old notes, intact
                    let mut story = note_story(&self.doc, r.footnote, r.id).cloned().unwrap_or_default();
                    super::super::normalize::normalize(&mut story, &mut self.doc.ids);
                    convert_note_content(&mut story, to, &ids(r.footnote), &ids(to));
                    set_note_story(&mut self.doc, to, nid, story);
                    nid
                }
            };
            // the reference run, in place
            let para = match &r.cell {
                Some((anchor, hop)) => {
                    let ctx = EditContext::Cell { anchor: *anchor, hops: Arc::from(vec![*hop]), saved: Selection::default() };
                    story_of_mut(&mut self.doc, &ctx).and_then(|s| s.paras.make_mut(r.para as usize))
                }
                None => self.paras_mut().and_then(|ps| ps.make_mut(r.para as usize)),
            };
            if let Some(p) = para {
                let mut acc = 0usize;
                for run in &mut p.runs {
                    if acc == r.offset as usize && run.ext().is_some_and(|e| e.note_ref.is_some()) {
                        if let Some(n) = run.ext_mut().note_ref.as_mut() {
                            obj::set(n, "kind", |x| x.kind = Str::from(kind_name(to)));
                            obj::set(n, "id", |x| x.id = JsNumber::Num(nid));
                        }
                        if run.props.char_style.as_deref() == Some(ids(r.footnote).r#ref) {
                            obj::set(Arc::make_mut(&mut run.props), "charStyle", |x| x.char_style = Some(Str::from(ids(to).r#ref)));
                        }
                        break;
                    }
                    acc += run.text.len();
                }
            }
        }
        // `if (this.pkg)` (19228; wave 7c, S)
        if !self.pkg_scratch {
            let _ = ensure_note_parts(&mut self.pkg);
            self.save_parts.note_parts = true;
        }
        self.touch_all();
        self.f5.last_converted = u32_of(new_ids.len());
        Ok(Outcome::Done(self.finish()))
    }

    // ── navigation ─────────────────────────────────────────────────────────────────────────

    /// `caretKey()`: the caret's place in the order of `noteRefs`.
    fn caret_key(&self) -> Vec<f64> {
        match &self.ctx {
            EditContext::Note { footnote, id, saved } => match self.note_refs(Some(*footnote)).into_iter().find(|x| x.id == *id) {
                Some(r) => r.key,
                None => vec![f64::from(saved.focus.para), 1.0, 0.0, 0.0, 0.0, 0.0, f64::from(saved.focus.offset)],
            },
            EditContext::Cell { anchor, hops, .. } => {
                let f = self.selection.focus;
                let before = self.doc.body.tables.get(*anchor as usize).map_or(0, |a| a.before_para);
                let h = hops.first().copied().unwrap_or_default();
                vec![f64::from(before), 0.0, f64::from(*anchor), f64::from(h.row), f64::from(h.col), f64::from(f.para), f64::from(f.offset)]
            }
            _ => {
                let f = self.selection.focus;
                vec![f64::from(f.para), 1.0, 0.0, 0.0, 0.0, 0.0, f64::from(f.offset)]
            }
        }
    }

    /// `placeBodyCaret(at)`: the caret at a position of the body or of a cell.
    fn place_body_caret(&mut self, at: &NoteRefLoc, offset: u32) {
        if matches!(self.ctx, EditContext::Note { .. }) {
            self.exit_note();
        }
        let caret = DocPosition::new(at.para, offset);
        match &at.cell {
            Some((anchor, hop)) => {
                let same = matches!(&self.ctx, EditContext::Cell { anchor: a, hops, .. } if a == anchor && **hops == [*hop]);
                if !same {
                    self.enter_cell(*anchor, vec![*hop], Some(caret));
                }
            }
            None => {
                if matches!(self.ctx, EditContext::Cell { .. }) {
                    self.exit_cell();
                }
            }
        }
        self.selection = Selection::caret(caret);
        self.pending_props = None;
    }

    /// `openNote(kind, id)`.
    fn open_note(&mut self, footnote: bool, id: f64) -> bool {
        if self.locked() || !self.note_refs(Some(footnote)).iter().any(|r| r.id == id) {
            return false;
        }
        self.enter_note(footnote, id, None, true)
    }

    /// `goToNoteRef(kind, id)`.
    fn go_to_note_ref(&mut self, footnote: bool, id: f64) -> bool {
        // the TS has no guard here and puts a body position in the header's selection (a later
        // `TypeError`); the header/footer and text box contexts refuse (F5-D5)
        if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
            return false;
        }
        if matches!(self.ctx, EditContext::Note { .. }) {
            self.exit_note();
        }
        let Some(r) = self.note_refs(Some(footnote)).into_iter().find(|x| x.id == id) else { return false };
        let mut len = 1usize;
        if r.cell.is_none()
            && let Some(p) = self.paras().and_then(|ps| ps.get(r.para as usize))
        {
            let mut acc = 0usize;
            for run in &p.runs {
                if acc == r.offset as usize && run.ext().is_some_and(|e| e.note_ref.is_some()) {
                    len = run.text.len();
                    break;
                }
                acc += run.text.len();
            }
        }
        let offset = r.offset.saturating_add(u32_of(len));
        self.place_body_caret(&r, offset);
        true
    }

    /// `nextNote(kind, dir)`.
    fn next_note(&mut self, footnote: bool, forward: bool) -> bool {
        if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
            return false;
        }
        let refs = self.note_refs(Some(footnote));
        if let EditContext::Note { footnote: f, id, .. } = self.ctx
            && f == footnote
        {
            let mut order: Vec<f64> = Vec::new();
            for r in &refs {
                if !order.contains(&r.id) {
                    order.push(r.id);
                }
            }
            let cur = order.iter().position(|x| *x == id).map_or(-1, |i| i as i64);
            let k = cur + if forward { 1 } else { -1 };
            let Some(target) = usize::try_from(k).ok().and_then(|k| order.get(k)).copied() else { return false };
            self.enter_note(footnote, target, None, false);
            return true;
        }
        let cur = self.caret_key();
        let target = if forward {
            refs.iter().find(|r| cmp_key(&r.key, &cur) > 0.0)
        } else {
            refs.iter().rev().find(|r| cmp_key(&r.key, &cur) < 0.0)
        };
        let Some(t) = target.cloned() else { return false };
        self.place_body_caret(&t, t.offset);
        true
    }

    /// `showNotes(kind?)`.
    fn show_notes(&mut self, footnote: Option<bool>) -> bool {
        if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. }) {
            return false;
        }
        if let EditContext::Note { footnote, id, .. } = self.ctx {
            return self.go_to_note_ref(footnote, id);
        }
        let k = footnote.unwrap_or_else(|| self.note_count(true) > 0);
        let refs = self.note_refs(Some(k));
        if refs.is_empty() {
            return false;
        }
        let cur = self.caret_key();
        // the reference right before the caret (the caret after it) or the next one
        let after = |r: &NoteRefLoc| -> Vec<f64> {
            let mut key = r.key.clone();
            if let Some(l) = key.last_mut() {
                *l += 1.0;
            }
            key
        };
        let before = refs.iter().rev().find(|r| cmp_key(&after(r), &cur) == 0.0);
        let Some(r) = before.or_else(|| refs.iter().find(|x| cmp_key(&x.key, &cur) >= 0.0)).or_else(|| refs.last()) else { return false };
        let (f, id) = (r.footnote, r.id);
        self.open_note(f, id)
    }

    pub(crate) fn exec_notes(&mut self, cmd: NoteCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            NoteCmd::Insert { footnote, mark } => self.insert_note(footnote, mark.as_ref()),
            NoteCmd::SetOptions { footnote, patch, scope } => self.set_note_options(footnote, &patch, scope),
            NoteCmd::Convert { mode, only } => self.convert_notes(mode, only),
            NoteCmd::Open { footnote, id } => {
                self.open_note(footnote, id);
                Ok(Outcome::Done(self.change_set()))
            }
            NoteCmd::GoToRef { footnote, id } => {
                self.go_to_note_ref(footnote, id);
                Ok(Outcome::Done(self.change_set()))
            }
            NoteCmd::Next { footnote, forward } => {
                self.next_note(footnote, forward);
                Ok(Outcome::Done(self.change_set()))
            }
            NoteCmd::Show { footnote } => {
                self.show_notes(footnote);
                Ok(Outcome::Done(self.change_set()))
            }
        }
    }
}

/// The pure model files of the family, for the parity capture of the editor's `notes.ts`
/// (`js/capture-d2b-f5-ops.mjs`). Not part of the API.
#[doc(hidden)]
pub mod testing {
    use super::{
        NoteOptions, StylePair, convert_note_content, effective_note_options, ensure_note_styles_xml, next_note_id, note_props_of,
    };
    use crate::model::Document;
    use crate::model::formatting::NoteProps;
    use crate::model::story::Story;
    use crate::session::ops::styles::sheet::read_style_index;
    use crate::session::ops::styles::xform::Defaults;

    /// `ensureNoteStyles(new DocStyles(xml), kind)`: the text and the two ids.
    #[must_use]
    pub fn ensure_note_styles(xml: &str, footnote: bool) -> (String, String, String) {
        let d = Defaults::of(&read_style_index(xml));
        let r = ensure_note_styles_xml(xml, &d, footnote);
        (r.xml, r.text, r.r#ref)
    }

    /// `nextNoteId(full, kind)`.
    #[must_use]
    pub fn next_id(doc: &Document, footnote: bool) -> f64 {
        next_note_id(doc, footnote)
    }

    /// `effectiveNoteOptions(kind, doc, sect, cols)` as the text the capture compares.
    #[must_use]
    pub fn effective(footnote: bool, doc: Option<&NoteProps>, sect: Option<&NoteProps>, cols: Option<f64>) -> String {
        options_text(&effective_note_options(footnote, doc, sect, cols))
    }

    /// `notePropsOf(kind, o, withPos)` (the JSON of the result, `-` for `undefined`).
    #[must_use]
    pub fn props_of(footnote: bool, o: &NoteOptions, with_pos: bool) -> String {
        note_props_of(footnote, o, with_pos).and_then(|p| crate::js::JsonWriter::stringify(&p, None)).unwrap_or_else(|| "-".to_owned())
    }

    /// `convertNoteContent(blocks, to, { from, to })` over a copy of the story.
    #[must_use]
    pub fn convert_content(story: &Story, to_footnote: bool, from: (&str, &str), to: (&str, &str)) -> Story {
        let mut s = story.clone();
        convert_note_content(&mut s, to_footnote, &StylePair { text: from.0, r#ref: from.1 }, &StylePair { text: to.0, r#ref: to.1 });
        s
    }

    #[must_use]
    pub fn options_text(o: &NoteOptions) -> String {
        format!(
            "{},{},{},{},{}",
            o.position,
            o.number_format,
            uo_jsnum::format::to_js_string(o.start_at),
            o.restart,
            o.columns.map_or_else(|| "-".to_owned(), uo_jsnum::format::to_js_string)
        )
    }
}
