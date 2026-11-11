//! `editor/src/toc.ts` in full: the gallery templates, the dialog formats (styles `toc 1–9`),
//! the TC fields, the dialog options ↔ the field's options, and the "Add Text" plan.

use super::pure::{TocOptions, TocStyle};
use super::rx;
use crate::session::ops::styles::UiLang;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{max, min};

/// `{ en, pt, es }`.
#[derive(Debug, Clone, Copy)]
pub struct Labels {
    pub en: &'static str,
    pub pt: &'static str,
    pub es: &'static str,
}

impl Labels {
    /// `pickLabels(lang, labels)`.
    #[must_use]
    pub fn pick(&self, lang: UiLang) -> &'static str {
        match lang {
            UiLang::En => self.en,
            UiLang::Pt => self.pt,
            UiLang::Es => self.es,
        }
    }
}

// ───────────────────────── gallery ─────────────────────────

/// `TocTemplateId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocTemplateId {
    Auto1,
    Auto2,
    Manual,
}

/// `TocTemplate`.
#[derive(Debug, Clone, Copy)]
pub struct TocTemplate {
    pub id: TocTemplateId,
    pub name: Labels,
    /// the text of the heading (style "TOC Heading")
    pub heading: Labels,
    /// typed entries (no field)
    pub manual: bool,
}

/// `TOC_TEMPLATES`.
pub const TOC_TEMPLATES: [TocTemplate; 3] = [
    TocTemplate {
        id: TocTemplateId::Auto1,
        name: Labels { en: "Automatic Table 1", pt: "Sum\u{e1}rio Autom\u{e1}tico 1", es: "Tabla autom\u{e1}tica 1" },
        heading: Labels { en: "Contents", pt: "Conte\u{fa}do", es: "Tabla de contenido" },
        manual: false,
    },
    TocTemplate {
        id: TocTemplateId::Auto2,
        name: Labels { en: "Automatic Table 2", pt: "Sum\u{e1}rio Autom\u{e1}tico 2", es: "Tabla autom\u{e1}tica 2" },
        heading: Labels { en: "Table of Contents", pt: "Sum\u{e1}rio", es: "Tabla de contenido" },
        manual: false,
    },
    TocTemplate {
        id: TocTemplateId::Manual,
        name: Labels { en: "Manual Table", pt: "Sum\u{e1}rio Manual", es: "Tabla manual" },
        heading: Labels { en: "Table of Contents", pt: "Sum\u{e1}rio", es: "Tabla de contenido" },
        manual: true,
    },
];

/// `TOC_AUTO_INSTR`.
pub const TOC_AUTO_INSTR: &str = " TOC \\o \"1-3\" \\h \\z \\u ";

/// `manualPlaceholder(lang, level)`.
#[must_use]
pub fn manual_placeholder(lang: UiLang, level: f64) -> String {
    let l = to_js_string(level);
    match lang {
        UiLang::Pt => format!("Digite o t\u{ed}tulo do cap\u{ed}tulo (n\u{ed}vel {l})"),
        UiLang::En => format!("Type chapter title (level {l})"),
        UiLang::Es => format!("Escribe el t\u{ed}tulo del cap\u{ed}tulo (nivel {l})"),
    }
}

/// `MANUAL_LEVELS`.
pub const MANUAL_LEVELS: [f64; 6] = [1.0, 2.0, 3.0, 1.0, 2.0, 3.0];

/// `TOC_GALLERY`.
pub const TOC_GALLERY: &str = "Table of Contents";

/// `tocSdtPr(id)`: the `w:sdtPr` of the TOC content control.
#[must_use]
pub fn toc_sdt_pr(id: f64) -> String {
    format!(
        "<w:sdtPr><w:id w:val=\"{}\"/><w:docPartObj><w:docPartGallery w:val=\"{TOC_GALLERY}\"/><w:docPartUnique/></w:docPartObj></w:sdtPr>",
        to_js_string(id)
    )
}

// ───────────────────────── formats ─────────────────────────

/// `TocFormat` (TablesOfContents.Format 0–6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocFormat {
    Template,
    Classic,
    Distinctive,
    Fancy,
    Modern,
    Formal,
    Simple,
}

fn sp(before: Option<f64>, after: f64) -> String {
    match before {
        Some(b) => format!("<w:spacing w:before=\"{}\" w:after=\"{}\"/>", to_js_string(b), to_js_string(after)),
        None => format!("<w:spacing w:after=\"{}\"/>", to_js_string(after)),
    }
}

fn ind(left: f64) -> String {
    if left > 0.0 { format!("<w:ind w:left=\"{}\"/>", to_js_string(left)) } else { String::new() }
}

fn sz(hp: f64) -> String {
    let h = to_js_string(hp);
    format!("<w:sz w:val=\"{h}\"/><w:szCs w:val=\"{h}\"/>")
}

const B: &str = "<w:b/><w:bCs/>";
const I: &str = "<w:i/><w:iCs/>";

/// `tocFormatStyle(f, level)`: the `w:pPr`/`w:rPr` content of the style `toc N` in each format.
#[must_use]
pub fn toc_format_style(f: TocFormat, level: f64) -> (String, String) {
    let n = min(9.0, max(1.0, level));
    let at = |v: f64| n == v;
    match f {
        TocFormat::Classic => {
            if at(1.0) {
                (
                    sp(Some(360.0), 0.0),
                    format!("<w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/>{B}<w:caps/>{}", sz(24.0)),
                )
            } else if at(2.0) {
                (sp(Some(240.0), 0.0), format!("{B}{}", sz(20.0)))
            } else {
                (format!("{}{}", sp(None, 0.0), ind(220.0 * (n - 2.0))), sz(20.0))
            }
        }
        TocFormat::Distinctive => {
            if at(1.0) {
                (sp(Some(120.0), 0.0), format!("{B}{I}{}", sz(24.0)))
            } else if at(2.0) {
                (format!("{}{}", sp(Some(120.0), 0.0), ind(220.0)), B.to_owned())
            } else {
                (format!("{}{}", sp(None, 0.0), ind(220.0 * (n - 1.0))), sz(20.0))
            }
        }
        TocFormat::Fancy => {
            if at(1.0) {
                (sp(Some(360.0), 360.0), format!("{B}<w:caps/><w:u w:val=\"single\"/>"))
            } else if at(2.0) {
                (sp(None, 0.0), format!("{B}<w:smallCaps/>"))
            } else if at(3.0) {
                (sp(None, 0.0), "<w:smallCaps/>".to_owned())
            } else {
                (sp(None, 0.0), String::new())
            }
        }
        TocFormat::Modern => {
            let bdr = "<w:pBdr><w:between w:val=\"double\" w:sz=\"6\" w:space=\"0\" w:color=\"auto\"/></w:pBdr>";
            let jc = "<w:jc w:val=\"center\"/>";
            if at(1.0) {
                (format!("{bdr}{}{jc}", sp(Some(120.0), 120.0)), format!("{B}{I}{}", sz(24.0)))
            } else if at(2.0) {
                (format!("{bdr}{}{jc}", sp(Some(120.0), 120.0)), format!("{I}{}", sz(20.0)))
            } else {
                (format!("{bdr}{}{}{jc}", sp(Some(120.0), 120.0), ind(220.0 * (n - 2.0))), sz(20.0))
            }
        }
        TocFormat::Formal => {
            if at(1.0) {
                (sp(Some(120.0), 120.0), format!("{B}<w:caps/>{}", sz(20.0)))
            } else if at(2.0) {
                (format!("{}{}", sp(None, 0.0), ind(220.0)), format!("<w:smallCaps/>{}", sz(20.0)))
            } else if at(3.0) {
                (format!("{}{}", sp(None, 0.0), ind(440.0)), format!("{I}{}", sz(20.0)))
            } else {
                (format!("{}{}", sp(None, 0.0), ind(220.0 * (n - 1.0))), sz(18.0))
            }
        }
        TocFormat::Simple => {
            if at(1.0) {
                (sp(Some(240.0), 120.0), format!("{B}{}", sz(20.0)))
            } else if at(2.0) {
                (format!("{}{}", sp(Some(120.0), 0.0), ind(220.0)), format!("{I}{}", sz(20.0)))
            } else {
                (format!("{}{}", sp(None, 0.0), ind(220.0 * (n - 1.0))), sz(20.0))
            }
        }
        TocFormat::Template => (format!("{}{}", sp(None, 100.0), ind(220.0 * (n - 1.0))), String::new()),
    }
}

// ───────────────────────── TC fields ─────────────────────────

/// `TcEntry`: a TOC entry marked with a TC field.
#[derive(Debug, Clone, PartialEq)]
pub struct TcEntry {
    pub text: String,
    /// the identifier (`\f`; without it Word uses "C")
    pub id: String,
    /// the level (`\l`; 1 by default)
    pub level: f64,
    /// `\n`: no page number
    pub no_page: bool,
}

/// `parseTcInstr(instr)`: `TC "text" \f C \l "2" \n` (`None` if it is not a TC).
#[must_use]
pub fn parse_tc_instr(instr: Option<&str>) -> Option<TcEntry> {
    let instr = instr.unwrap_or("");
    let (raw, rest) = rx::tc_head(instr)?;
    let text = rx::unescape_tc(raw);
    let id = rx::tc_switch_f(rest).unwrap_or_else(|| "C".to_owned());
    let level = match rx::tc_switch_l(rest) {
        Some(d) => min(9.0, max(1.0, uo_jsnum::parse::to_number(d))),
        None => 1.0,
    };
    Some(TcEntry { text, id, level, no_page: rx::tc_switch_n(rest) })
}

/// `tcInstr({ text, id, level })`: the instruction as "Mark Table of Contents Entry" writes it.
#[must_use]
pub fn tc_instr(text: &str, id: Option<&str>, level: Option<f64>) -> String {
    let id = uo_jsnum::text::trim(id.unwrap_or(""));
    let id = if id.is_empty() { "C" } else { id };
    let text = rx::escape_tc(text);
    let id_s = if id.chars().any(uo_jsnum::text::is_js_space) { format!("\"{id}\"") } else { id.to_owned() };
    format!(" TC \"{text}\" \\f {id_s} \\l \"{}\" ", to_js_string(min(9.0, max(1.0, level.unwrap_or(1.0)))))
}

/// `tcInToc(o, tc)`: does the TC go in the TOC?
#[must_use]
pub fn tc_in_toc(o: &TocOptions, tc: &TcEntry) -> bool {
    if !o.fields {
        return false;
    }
    if uo_jsnum::text::to_lowercase(o.field_id.as_deref().unwrap_or("C")) != uo_jsnum::text::to_lowercase(&tc.id) {
        return false;
    }
    match o.field_levels {
        None => true,
        Some((a, b)) => tc.level >= a && tc.level <= b,
    }
}

// ───────────────────────── dialog ─────────────────────────

/// `TableOfContentsOptions`: the options of the Custom Table of Contents dialog.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableOfContentsOptions {
    pub page_numbers: Option<bool>,
    pub right_align: Option<bool>,
    /// `TabLeaderChoice` (`"none"` for none)
    pub leader: Option<String>,
    pub hyperlinks: Option<bool>,
    pub format: Option<TocFormat>,
    pub levels: Option<f64>,
    pub use_styles: Option<bool>,
    pub styles: Option<Vec<TocStyle>>,
    pub outline_levels: Option<bool>,
    pub fields: Option<bool>,
    pub field_id: Option<String>,
}

/// `tocOptionsFrom(d)`: the dialog options → the field's options.
#[must_use]
pub fn toc_options_from(d: &TableOfContentsOptions) -> TocOptions {
    let levels = min(9.0, max(1.0, d.levels.unwrap_or(3.0)));
    let links = d.hyperlinks != Some(false);
    let mut o = TocOptions { hyperlinks: links, hide_in_web: true, ..TocOptions::default() };
    if d.use_styles != Some(false) {
        o.outline = Some((1.0, levels));
    }
    if d.fields == Some(true) {
        o.fields = true;
        if let Some(id) = d.field_id.as_deref().filter(|s| !s.is_empty()) {
            o.field_id = Some(id.to_owned());
        }
    }
    if d.page_numbers == Some(false) {
        o.no_page_numbers = true;
    } else if d.right_align == Some(false) {
        o.separator = Some(" ".to_owned());
    }
    if d.use_styles != Some(false)
        && let Some(st) = d.styles.as_ref().filter(|s| !s.is_empty())
    {
        o.styles = st.iter().filter(|s| s.level >= 1.0 && s.level <= 9.0).cloned().collect();
    }
    if d.outline_levels != Some(false) {
        o.outline_levels = true;
    }
    if !links {
        o.hyperlinks = false;
    }
    o
}

/// `dialogOptionsFrom(o)`: the field's options → the dialog (the format and the leader come from
/// outside).
#[must_use]
pub fn dialog_options_from(o: &TocOptions) -> TableOfContentsOptions {
    TableOfContentsOptions {
        page_numbers: Some(!o.no_page_numbers),
        right_align: Some(o.separator.is_none()),
        hyperlinks: Some(o.hyperlinks),
        levels: Some(o.outline.map(|r| r.1).or(o.field_levels.map(|r| r.1)).unwrap_or(3.0)),
        use_styles: Some(o.outline.is_some() || !o.styles.is_empty()),
        styles: Some(o.styles.clone()),
        outline_levels: Some(o.outline_levels),
        fields: Some(o.fields),
        field_id: o.field_id.clone().filter(|s| !s.is_empty()),
        ..TableOfContentsOptions::default()
    }
}

// ───────────────────────── Add Text ─────────────────────────

/// The style kind of a paragraph for Add Text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTextKind {
    Normal,
    Heading,
    Other,
}

/// What `addTextPlan` answers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AddTextPlan {
    /// `{ heading: level }`
    Heading(f64),
    /// `{ normal: true }`
    Normal,
    /// `{ outline: null }`
    OutlineNone,
    /// `{ outline: level }`
    Outline(f64),
}

/// `addTextPlan(level, styleKind)`.
#[must_use]
pub fn add_text_plan(level: f64, kind: AddTextKind) -> AddTextPlan {
    if level <= 0.0 {
        return if kind == AddTextKind::Heading { AddTextPlan::Normal } else { AddTextPlan::OutlineNone };
    }
    if kind == AddTextKind::Other { AddTextPlan::Outline(level) } else { AddTextPlan::Heading(level) }
}
