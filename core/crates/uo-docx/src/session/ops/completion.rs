//! Wave 7 (A2) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A2: lists, paste,
//! tables, objects and the page"): the read-only answers the TS shell still computed over its own (empty)
//! model under `engine="rust"`. Each is a 1:1 port of the TS method named in its doc comment
//! (`write/packages/editor/src/universal-write.ts` of the frozen TS root); none changes the session.
//!
//! The by-name sort of the table style gallery (`localeCompare`) stays in the shell, as the bookmarks' by-name
//! sort does (F6-D3): [`Session::table_style_listing`] answers the entries in the TS order BEFORE the sort.

use std::collections::HashSet;
use std::sync::Arc;

use super::super::Session;
use super::lists::{ParaNum, para_num_of};
use crate::js::JsNumber;
use crate::model::formatting::{DocFormatting, NumLevel};
use crate::view::DocView;

/// `ListLevelSpec` as the TS `listLevelSpec(l)` builds it (universal-write.ts 23468): the optional keys are
/// present only when the TS writes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelView {
    pub ilvl: f64,
    pub num_fmt: String,
    pub lvl_text: String,
    pub font: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub size_pt: Option<f64>,
    pub color: Option<String>,
    pub start: f64,
    pub p_style: Option<String>,
    pub ind_left: f64,
    pub hanging: f64,
    /// left | center | right
    pub jc: &'static str,
    pub suff: String,
    pub tab_pos: Option<f64>,
    pub is_lgl: bool,
    pub lvl_restart: Option<f64>,
}

/// `listLevelSpec(l)` (universal-write.ts 23468).
#[must_use]
pub fn level_view(l: &NumLevel) -> LevelView {
    let ind = l.p_pr.as_deref().and_then(|p| p.ind.as_deref());
    let jc = match l.lvl_jc.as_deref() {
        Some("center") => "center",
        Some("right" | "end") => "right",
        _ => "left",
    };
    let tab = l.p_pr.as_deref().and_then(|p| p.tabs.as_ref()).and_then(|t| t.iter().find(|t| &*t.val == "num"));
    let r = l.r_pr.as_deref();
    let num = |x: Option<JsNumber>| x.map(JsNumber::to_number);
    let truthy = |x: Option<f64>| x.filter(|v| uo_jsnum::conv::truthy(*v));
    LevelView {
        ilvl: l.ilvl.to_number(),
        num_fmt: l.num_fmt.as_deref().unwrap_or("decimal").to_owned(),
        lvl_text: if l.lvl_text_null == Some(true) { String::new() } else { l.lvl_text.as_deref().unwrap_or("").to_owned() },
        font: r.and_then(|r| r.r_fonts.as_deref()).and_then(|f| f.ascii.as_deref()).filter(|s| !s.is_empty()).map(str::to_owned),
        bold: r.and_then(|r| r.b) == Some(true),
        italic: r.and_then(|r| r.i) == Some(true),
        size_pt: truthy(r.and_then(|r| num(r.sz))).map(|s| s / 2.0),
        color: r
            .and_then(|r| r.color.as_deref())
            .and_then(|c| c.val.as_deref())
            .filter(|v| !v.is_empty() && *v != "auto")
            .map(uo_text::case::to_uppercase_str),
        start: match l.start {
            Some(s) => s.to_number(),
            None => {
                if l.num_fmt.as_deref() == Some("bullet") {
                    1.0
                } else {
                    0.0
                }
            }
        },
        p_style: l.p_style.as_deref().filter(|s| !s.is_empty()).map(str::to_owned),
        ind_left: num(ind.and_then(|i| i.left)).unwrap_or(0.0),
        hanging: match num(ind.and_then(|i| i.hanging)) {
            Some(h) => h,
            None => num(ind.and_then(|i| i.first_line)).map_or(0.0, |f| -f),
        },
        jc,
        suff: l.suff.as_deref().unwrap_or("tab").to_owned(),
        tab_pos: tab.map(|t| t.pos.to_number()),
        is_lgl: l.is_lgl == Some(true),
        lvl_restart: num(l.lvl_restart),
    }
}

/// `listInfo()` (universal-write.ts 3626), plus `canContinue` (`!!previousList(focus.para)`, what
/// `continueNumbering()` answers before it runs).
#[derive(Debug, Clone, PartialEq)]
pub struct ListInfoView {
    pub num_id: f64,
    pub ilvl: f64,
    pub abs_id: f64,
    pub bullet: bool,
    pub level: Option<LevelView>,
    pub multilevel: bool,
    pub levels: Vec<LevelView>,
    pub can_continue: bool,
}

/// One entry of `documentLists()` (universal-write.ts 3653).
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentListView {
    pub num_id: f64,
    pub bullet: bool,
    pub level: LevelView,
    pub levels: Vec<LevelView>,
}

/// One entry of `listStylesInDocument()` (universal-write.ts 4072).
#[derive(Debug, Clone, PartialEq)]
pub struct ListStyleView {
    pub id: String,
    pub name: String,
    pub num_id: f64,
    pub levels: Vec<LevelView>,
}

/// One entry of the table style gallery (`TableStyleInfo`, before the shell's sort).
#[derive(Debug, Clone, PartialEq)]
pub struct TableStyleView {
    pub name: String,
    pub en: String,
    pub pt: String,
    pub es: String,
    /// custom | plain | grid | list
    pub group: &'static str,
    pub id: Option<String>,
}

fn count_refs(t: &str) -> usize {
    t.as_bytes().windows(2).filter(|w| w[0] == b'%' && (b'1'..=b'9').contains(&w[1])).count()
}

/// The levels of an abstract sorted by `ilvl` (`Object.values(abs.levels).sort((a, b) => a.ilvl - b.ilvl)`):
/// a stable sort over the record's order, as V8's.
fn sorted_levels(levels: &crate::js::JsRecord<NumLevel>) -> Vec<LevelView> {
    let mut v: Vec<&NumLevel> = levels.values().collect();
    uo_jsnum::sort::js_sort_by(&mut v, |a, b| a.ilvl.to_number() - b.ilvl.to_number());
    v.into_iter().map(level_view).collect()
}

impl Session {
    /// The parsed abstract `abs` (`numbering.parsed().abstractNums[absId]`).
    fn abstract_levels(&mut self, abs: f64) -> Option<Vec<LevelView>> {
        let parsed = self.num_parsed();
        let key = uo_jsnum::format::to_js_string(abs);
        parsed.abstract_nums.get(&key).map(|a| sorted_levels(&a.levels))
    }

    /// `listInfo()` of the caret's paragraph (universal-write.ts 3626) and `canContinue`.
    pub fn list_info_view(&mut self) -> Option<ListInfoView> {
        let pi = self.selection.focus.para as usize;
        let n: ParaNum = self.para_num_at(pi)?;
        let num_id = n.num_id;
        let abs = if num_id > 0.0 { self.abstract_of(num_id) } else { None };
        let levels = abs.and_then(|a| self.abstract_levels(a)).unwrap_or_default();
        let lvl = if num_id > 0.0 { self.num_level(num_id, n.ilvl) } else { None };
        let bullet = match &lvl {
            Some(l) => l.num_fmt.as_deref() == Some("bullet"),
            None => num_id == -1.0,
        };
        let multilevel = levels.iter().any(|l| l.ilvl > 0.0 && count_refs(&l.lvl_text) > 1);
        let can_continue = self.previous_list(pi) != 0.0;
        Some(ListInfoView {
            num_id,
            ilvl: n.ilvl,
            abs_id: abs.unwrap_or(-1.0),
            bullet,
            level: lvl.as_ref().map(level_view),
            multilevel,
            levels,
            can_continue,
        })
    }

    /// `documentLists()` (universal-write.ts 3653): the lists of the BODY's paragraphs, in order.
    pub fn document_lists_view(&mut self) -> Vec<DocumentListView> {
        let defp = self.default_paragraph();
        let ids: Vec<f64> = {
            let sheet = &self.doc.formatting.styles;
            self.doc.body.paras.iter().map(|p| para_num_of(p, sheet, &defp).map_or(0.0, |n| n.num_id)).collect()
        };
        let mut seen: Vec<f64> = Vec::new();
        let mut out = Vec::new();
        for id in ids {
            if id <= 0.0 || seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let abs = self.abstract_of(id);
            let levels = abs.and_then(|a| self.abstract_levels(a));
            let l0 = self.num_level(id, 0.0);
            let (Some(levels), Some(l0)) = (levels, l0) else { continue };
            out.push(DocumentListView { num_id: id, bullet: l0.num_fmt.as_deref() == Some("bullet"), level: level_view(&l0), levels });
        }
        out
    }

    /// `listStylesInDocument()` (universal-write.ts 4072).
    pub fn list_styles_view(&mut self) -> Vec<ListStyleView> {
        let styles: Vec<(String, String, f64)> = self
            .doc
            .formatting
            .styles
            .styles
            .values()
            .filter(|s| &*s.r#type == "numbering")
            .filter_map(|s| {
                let n = s.p_pr.as_deref()?.num_pr.as_deref()?.num_id?.to_number();
                uo_jsnum::conv::truthy(n)
                    .then(|| (s.style_id.to_string(), s.name.as_deref().map_or_else(|| s.style_id.to_string(), str::to_owned), n))
            })
            .collect();
        let mut out = Vec::new();
        for (id, name, num_id) in styles {
            let Some(abs) = self.abstract_of(num_id) else { continue };
            let Some(levels) = self.abstract_levels(abs) else { continue };
            out.push(ListStyleView { id, name, num_id, levels });
        }
        out
    }

    /// `numberingValueInfo()` (universal-write.ts 3974): `(canContinue, value, numFmt)`; `None` on a bullet or
    /// outside a list. The value is the item's from the document's list values (`listItemValues(full)`).
    pub fn numbering_value_view(&mut self) -> Option<(bool, f64, String)> {
        let info = self.list_info_view()?;
        if info.bullet {
            return None;
        }
        let pi = self.selection.focus.para as usize;
        let id = self.paras()?.get(pi)?.id;
        let value = crate::word::list_item_values(DocView::new(&self.doc), None).get(&id).map(|v| v.value);
        let lv = info.level.as_ref();
        Some((
            info.can_continue,
            value.or(lv.map(|l| l.start)).unwrap_or(1.0),
            lv.map_or_else(|| "decimal".to_owned(), |l| l.num_fmt.clone()),
        ))
    }

    /// `listLibrary(kind)` (universal-write.ts 3646): 0 bullet, 1 number, 2 multilevel; in the session's
    /// language of the interface.
    pub fn list_library_view(&mut self, kind: u32) -> Vec<Vec<LevelView>> {
        use super::lists::library::{BULLET_LIBRARY, MULTILEVEL_LIBRARY, NUMBER_LIBRARY};
        let lib = match kind {
            0 => BULLET_LIBRARY,
            1 => NUMBER_LIBRARY,
            _ => MULTILEVEL_LIBRARY,
        };
        let loc = self.list_locale();
        lib.iter()
            .map(|d| {
                super::lists::library_levels(d, &loc).iter().filter_map(|x| super::lists::parse_level(x)).map(|l| level_view(&l)).collect()
            })
            .collect()
    }

    /// `formattingForTables()` without changing the session: the sheet, with the Office theme's colours when the
    /// document has none (a copy only then).
    pub(crate) fn tables_formatting(&self) -> Arc<DocFormatting> {
        if self.doc.formatting.theme.colors.is_empty() {
            let mut f = (*self.doc.formatting).clone();
            for (k, v) in super::tables::OFFICE_THEME {
                f.theme.colors.insert(k, v.into());
            }
            Arc::new(f)
        } else {
            Arc::clone(&self.doc.formatting)
        }
    }

    /// `tableStyleListNow()` (universal-write.ts 19925) before its sort: the document's table styles that are not
    /// built in, then the built-ins of the gallery (modern or Word 2010 by the compatibility mode).
    #[must_use]
    pub fn table_style_listing(&self) -> Vec<TableStyleView> {
        let f = self.tables_formatting();
        let builtins = crate::read::table_styles::builtin_table_styles();
        let lower = uo_text::case::to_lowercase_str;
        let names: HashSet<String> = builtins.iter().map(|b| lower(&b.name)).collect();
        let mut out = Vec::new();
        for s in f.styles.styles.values() {
            let Some(name) = s.name.as_deref().filter(|n| !n.is_empty()) else { continue };
            if &*s.r#type != "table" || names.contains(&lower(name)) {
                continue;
            }
            out.push(TableStyleView {
                name: name.to_owned(),
                en: name.to_owned(),
                pt: name.to_owned(),
                es: name.to_owned(),
                group: "custom",
                id: Some(s.style_id.to_string()),
            });
        }
        // wave 7c (W, item 2, fix round 1): a package without settings is mode 12 in Word (and in the layout)
        let cm = if self.doc.read_facts.settings_absent { 12.0 } else { f.settings.compatibility_mode.to_number() };
        let modern = (if cm.is_nan() { 15.0 } else { cm }) >= 15.0;
        let en = |n: &str| -> String {
            // `n.replace(/ Accent (\d)$/, ' - Accent $1')`
            let b = n.as_bytes();
            if b.len() >= 9
                && n.is_char_boundary(b.len() - 9)
                && n[b.len() - 9..b.len() - 1] == *" Accent "
                && b[b.len() - 1].is_ascii_digit()
            {
                format!("{} - Accent {}", &n[..b.len() - 9], char::from(b[b.len() - 1]))
            } else {
                n.to_owned()
            }
        };
        for b in &builtins {
            if b.name == "Normal Table" || b.modern != modern {
                continue;
            }
            let group = if b.name.starts_with("Grid Table ") && b.name.as_bytes().get(11).is_some_and(u8::is_ascii_digit) {
                "grid"
            } else if b.name.starts_with("List Table") {
                "list"
            } else {
                "plain"
            };
            out.push(TableStyleView {
                name: b.name.clone(),
                en: en(&b.name),
                pt: b.local.clone(),
                es: crate::read::style_names_es::table_style_name_es(&b.name).unwrap_or_else(|| en(&b.name)),
                group,
                id: crate::read::table_styles::style_id_by_name(Some(&*f), &b.name).map(str::to_owned),
            });
        }
        out
    }

    /// `tableStyleXml(nameOrId)` (universal-write.ts 20238): `(id, xml, inDoc)`.
    #[must_use]
    pub fn table_style_xml(&self, name_or_id: &str) -> Option<(String, String, bool)> {
        let f = self.tables_formatting();
        let by_id = if f.styles.styles.get(name_or_id).is_some_and(|s| &*s.r#type == "table") {
            Some(name_or_id.to_owned())
        } else {
            crate::read::table_styles::style_id_by_name(Some(&*f), name_or_id).map(str::to_owned)
        };
        if let (Some(id), Some(xml)) = (by_id.as_deref(), self.styles.xml())
            && let Some(raw) = super::styles::sheet::style_raw(xml, id)
        {
            return Some((id.to_owned(), raw, true));
        }
        let mut fc = (*f).clone();
        let mut pending: Vec<(String, String)> = Vec::new();
        let id = super::tables::ensure_table_style(&mut fc, name_or_id, &mut pending)?;
        let xml = pending.into_iter().find(|(k, _)| *k == id).map(|(_, x)| x)?;
        Some((id, xml, false))
    }

    /// `tableStyleBases()` (universal-write.ts 20255): `[id, name]` of the table styles with a name.
    #[must_use]
    pub fn table_style_bases(&self) -> Vec<(String, String)> {
        let f = self.tables_formatting();
        f.styles
            .styles
            .values()
            .filter(|s| &*s.r#type == "table")
            .filter_map(|s| s.name.as_deref().filter(|n| !n.is_empty()).map(|n| (s.style_id.to_string(), n.to_owned())))
            .collect()
    }

    /// `newTableStyleId(name)` (universal-write.ts 20261).
    #[must_use]
    pub fn new_table_style_id(&self, name: &str) -> String {
        let f = self.tables_formatting();
        let taken: HashSet<String> = f.styles.styles.keys().map(str::to_owned).collect();
        super::styles::sheet::style_id_from_name(name, &taken)
    }
    /// `saveSelectionToTextboxGallery(name)`'s model half (universal-write.ts 14636): the selected text box's raw
    /// `w:drawing` with its `w:txbxContent` replaced by its blocks as `serializeBlocks(blocks, model.sectPr)` writes
    /// them (a document without sources in a scratch package: the body between `<w:body>` and the `w:sectPr`), and
    /// the box's `docPr` name (the shell picks `name.trim() || docName || 'Text Box'` and stores the item).
    /// `None` without a selected text box (`drawRun()` with a `textbox` and a raw drawing).
    #[must_use]
    pub fn textbox_gallery_item(&self) -> Option<(String, Option<String>)> {
        use crate::write::parts::{WriteOptions, write_document};
        let path = self.draw_path()?;
        let run = self.run_ref(&path)?;
        let tb = run.ext()?.textbox.as_deref()?;
        let raw = super::drawing::raw_of(run)?;
        let mut d = self.doc.clone();
        d.sources = None;
        d.body = tb.content.clone();
        d.hf = crate::pvec::PVec::new();
        d.comments = Arc::from(Vec::new());
        let mut pkg = crate::read::blank::empty_docx_package();
        let opts = WriteOptions {
            comments: None,
            body_sect_refs: Some(Vec::new()),
            track_revisions: None,
            footnotes: None,
            endnotes: None,
            notes: false,
            blocks: true,
        };
        let mut save = crate::write::ctx::SaveState::default();
        let sp = Arc::clone(&self.sections.body_sect_pr);
        write_document(&mut pkg, &d, &d.body, &sp, &[], &opts, &mut save).ok()?;
        let doc = pkg.part_text("word/document.xml").ok().flatten().map(std::borrow::Cow::into_owned).unwrap_or_default();
        // `/<w:body>([\s\S]*?)<w:sectPr/.exec(doc)?.[1] ?? '<w:p/>'`
        let body = doc
            .find("<w:body>")
            .map(|i| i + "<w:body>".len())
            .and_then(|at| {
                let rest = doc.get(at..)?;
                let mut from = 0;
                while let Some(k) = rest.get(from..)?.find("<w:sectPr") {
                    let end = from + k + "<w:sectPr".len();
                    if !rest.as_bytes().get(end).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
                        return rest.get(..from + k).map(str::to_owned);
                    }
                    from = end;
                }
                None
            })
            .unwrap_or_else(|| "<w:p/>".to_owned());
        // `raw.replace(/<w:txbxContent[^>]*>[\s\S]*?<\/w:txbxContent>/, …)`: the first box content
        let xml = replace_txbx_content(raw, &body);
        let name = match super::drawing::format_of(run) {
            Some(f) => f.base().doc_pr.name.as_deref().map(str::to_owned),
            None => None,
        };
        Some((xml, name))
    }
}

/// An embedded object under the pointer (`oleRunAt(e)`, universal-write.ts 20371): its paragraph and run in the body,
/// the `progId`, the extension and the bytes of the embedded package.
#[derive(Debug, Clone, PartialEq)]
pub struct OleAt {
    pub para: u32,
    pub run: u32,
    pub prog_id: String,
    pub ext: String,
    pub data: Arc<[u8]>,
}

impl Session {
    /// `oleRunAt(e)` from the hit position `(para, offset)` of the body: the run with an OLE object that starts or
    /// ends at `offset`, the first in run order (`None` elsewhere, outside the body or in a text box).
    #[must_use]
    pub fn ole_at(&self, para: u32, offset: u32) -> Option<OleAt> {
        if !matches!(self.ctx, super::context::EditContext::Body) {
            return None;
        }
        let p = self.paras()?.get(para as usize)?;
        let mut acc: usize = 0;
        for (k, r) in p.runs.iter().enumerate() {
            let end = acc + r.text.len();
            if let Some(ole) = r.ext().and_then(|e| e.image.as_deref()).and_then(|i| i.ole.as_deref())
                && (offset as usize == acc || offset as usize == end)
            {
                let data: Arc<[u8]> = Arc::from(self.doc.media.get(ole.data.0).unwrap_or(&[]));
                return Some(OleAt {
                    para,
                    run: u32::try_from(k).unwrap_or(u32::MAX),
                    prog_id: ole.prog_id.to_string(),
                    ext: ole.ext.to_string(),
                    data,
                });
            }
            acc = end;
        }
        None
    }
}

/// `raw.replace(/<w:txbxContent[^>]*>[\s\S]*?<\/w:txbxContent>/, () => `<w:txbxContent>${body}</w:txbxContent>`)`.
fn replace_txbx_content(raw: &str, body: &str) -> String {
    const OPEN: &str = "<w:txbxContent";
    const CLOSE: &str = "</w:txbxContent>";
    let mut from = 0;
    while let Some(k) = raw.get(from..).and_then(|r| r.find(OPEN)) {
        let at = from + k;
        let after = at + OPEN.len();
        let next = raw.as_bytes().get(after).copied();
        if next.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
            from = after;
            continue;
        }
        let Some(gt) = raw.get(after..).and_then(|r| r.find('>')) else { break };
        let open_end = after + gt + 1;
        let Some(c) = raw.get(open_end..).and_then(|r| r.find(CLOSE)) else { break };
        let end = open_end + c + CLOSE.len();
        return format!("{}<w:txbxContent>{body}</w:txbxContent>{}", raw.get(..at).unwrap_or(""), raw.get(end..).unwrap_or(""));
    }
    raw.to_owned()
}

// ───────────────────────── sectPrAt (wave 7, A2) ─────────────────────────

impl Session {
    /// `sectPrAt(sectionIdx)` (universal-write.ts 10985): the section properties of the body's section `k` (the last
    /// section's, the document's `sectPr`, past the end), as `JSON.stringify` of the TS object.
    #[must_use]
    pub fn sect_pr_json(&self, k: u32) -> String {
        let sp = self.body_sect_pr_at(k as usize);
        crate::js::JsonWriter::stringify(&*sp, Some(&self.doc.media)).unwrap_or_else(|| "null".to_owned())
    }
}

// ───────────────────────── the shape parts of selectedShape().format (wave 7, A2: ED-213) ─────────────────────────

fn color_json(out: &mut String, c: &crate::model::picture::PicColor) {
    match crate::js::JsonWriter::stringify(c, None) {
        Some(j) => out.push_str(&j),
        None => out.push_str("null"),
    }
}

fn opt_color_json(out: &mut String, c: Option<&crate::model::picture::PicColor>) {
    match c {
        Some(c) => color_json(out, c),
        None => out.push_str("null"),
    }
}

fn fill_json(out: &mut String, f: &crate::write::emit::object::ShapeFill) {
    use crate::write::emit::object::ShapeFill as F;
    use uo_jsnum::json::{push_number, push_string};
    match f {
        F::None => out.push_str("{\"kind\":\"none\"}"),
        F::Style => out.push_str("{\"kind\":\"style\"}"),
        F::Solid(c) => {
            out.push_str("{\"kind\":\"solid\",\"color\":");
            color_json(out, c);
            out.push('}');
        }
        F::Gradient(g) => {
            out.push_str("{\"kind\":\"gradient\",\"stops\":[");
            for (i, s) in g.stops.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"pos\":");
                push_number(out, s.pos);
                out.push_str(",\"color\":");
                color_json(out, &s.color);
                out.push('}');
            }
            out.push(']');
            if let Some((ang, scaled)) = g.lin {
                out.push_str(",\"lin\":{\"ang\":");
                push_number(out, ang);
                out.push_str(",\"scaled\":");
                out.push_str(if scaled { "true" } else { "false" });
                out.push('}');
            }
            if let Some((path, rect)) = &g.path {
                out.push_str(",\"path\":{\"path\":");
                push_string(out, path);
                if let Some(r) = rect {
                    out.push_str(",\"fillToRect\":{\"l\":");
                    push_number(out, r.l);
                    out.push_str(",\"t\":");
                    push_number(out, r.t);
                    out.push_str(",\"r\":");
                    push_number(out, r.r);
                    out.push_str(",\"b\":");
                    push_number(out, r.b);
                    out.push('}');
                }
                out.push('}');
            }
            if let Some(r) = g.rot_with_shape {
                out.push_str(if r { ",\"rotWithShape\":true" } else { ",\"rotWithShape\":false" });
            }
            if let Some(fl) = &g.flip {
                out.push_str(",\"flip\":");
                push_string(out, fl);
            }
            if g.tile_rect {
                out.push_str(",\"tileRect\":true");
            }
            out.push('}');
        }
        F::Pattern { prst, fg, bg } => {
            out.push_str("{\"kind\":\"pattern\",\"prst\":");
            push_string(out, prst);
            out.push_str(",\"fg\":");
            color_json(out, fg);
            out.push_str(",\"bg\":");
            color_json(out, bg);
            out.push('}');
        }
        F::Picture { embed, tile, alpha } => {
            out.push_str("{\"kind\":\"picture\",\"embed\":");
            push_string(out, embed);
            if let Some(t) = tile {
                out.push_str(",\"tile\":{\"tx\":");
                push_number(out, t.tx);
                out.push_str(",\"ty\":");
                push_number(out, t.ty);
                out.push_str(",\"sx\":");
                push_number(out, t.sx);
                out.push_str(",\"sy\":");
                push_number(out, t.sy);
                out.push_str(",\"flip\":");
                push_string(out, &t.flip);
                out.push_str(",\"algn\":");
                push_string(out, &t.algn);
                out.push('}');
            }
            if let Some(a) = alpha {
                out.push_str(",\"alpha\":");
                push_number(out, *a);
            }
            out.push('}');
        }
        F::Raw(xml) => {
            out.push_str("{\"kind\":\"raw\",\"xml\":");
            push_string(out, xml);
            out.push('}');
        }
    }
}

impl Session {
    /// The shape parts of `selectedShape().format` (ED-213): `{ fill, style?, textbox, body?, story?,
    /// normalEastAsianFlow? }` of the selected shape or text box as the TS `readShapeFormat` gives them (JSON); `None`
    /// for a picture, a group (their format is a `PictureFormat`) or without a selected drawing.
    #[must_use]
    pub fn selected_shape_parts_json(&self) -> Option<String> {
        use crate::write::emit::object::{BodyAnchor, BodyAutofit, BodyWrap, ShapeStory};
        use uo_jsnum::json::{push_number, push_string};
        let path = self.draw_path()?;
        let run = self.run_ref(&path)?;
        let super::drawing::Fmt::Shape(f) = super::drawing::format_of(run)? else { return None };
        let mut out = String::from("{\"fill\":");
        fill_json(&mut out, &f.fill);
        if let Some(st) = &f.style {
            out.push_str(",\"style\":{");
            for (i, (k, r)) in [("ln", &st.ln), ("fill", &st.fill), ("effect", &st.effect)].into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_string(&mut out, k);
                out.push_str(":{\"idx\":");
                push_number(&mut out, r.idx);
                out.push_str(",\"color\":");
                opt_color_json(&mut out, r.color.as_ref());
                out.push('}');
            }
            out.push_str(",\"font\":{\"idx\":");
            push_string(&mut out, &st.font.idx);
            out.push_str(",\"color\":");
            opt_color_json(&mut out, st.font.color.as_ref());
            out.push_str("}}");
        }
        out.push_str(",\"textbox\":");
        out.push_str(if f.textbox { "true" } else { "false" });
        if let Some(b) = &f.body {
            out.push_str(",\"body\":{\"lIns\":");
            push_number(&mut out, b.l_ins);
            out.push_str(",\"tIns\":");
            push_number(&mut out, b.t_ins);
            out.push_str(",\"rIns\":");
            push_number(&mut out, b.r_ins);
            out.push_str(",\"bIns\":");
            push_number(&mut out, b.b_ins);
            out.push_str(",\"anchor\":");
            out.push_str(match b.anchor {
                BodyAnchor::T => "\"t\"",
                BodyAnchor::Ctr => "\"ctr\"",
                BodyAnchor::B => "\"b\"",
            });
            out.push_str(",\"anchorCtr\":");
            out.push_str(if b.anchor_ctr { "true" } else { "false" });
            out.push_str(",\"vert\":");
            push_string(&mut out, &b.vert);
            out.push_str(",\"wrap\":");
            out.push_str(match b.wrap {
                BodyWrap::Square => "\"square\"",
                BodyWrap::None => "\"none\"",
            });
            out.push_str(",\"autofit\":");
            out.push_str(match b.autofit {
                BodyAutofit::None => "\"none\"",
                BodyAutofit::Shape => "\"shape\"",
                BodyAutofit::Norm => "\"norm\"",
            });
            out.push_str(",\"upright\":");
            out.push_str(if b.upright { "true" } else { "false" });
            out.push_str(",\"rot\":");
            push_number(&mut out, b.rot);
            out.push('}');
        }
        match &f.story {
            Some(ShapeStory::Txbx { id }) => {
                out.push_str(",\"story\":{\"kind\":\"txbx\"");
                if let Some(id) = id {
                    out.push_str(",\"id\":");
                    push_number(&mut out, *id);
                }
                out.push('}');
            }
            Some(ShapeStory::Linked { id, seq }) => {
                out.push_str(",\"story\":{\"kind\":\"linked\",\"id\":");
                push_number(&mut out, *id);
                out.push_str(",\"seq\":");
                push_number(&mut out, *seq);
                out.push('}');
            }
            None => {}
        }
        if f.normal_east_asian_flow {
            out.push_str(",\"normalEastAsianFlow\":true");
        }
        out.push('}');
        Some(out)
    }
}
