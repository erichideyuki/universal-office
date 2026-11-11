//! The pure rules of `editor/src/references.ts` that D2b-F6 did not port (cross-reference
//! instructions, captions, the TOC instruction and its entries, the empty-TOC messages, the
//! table-of-figures formats): no session, no layout. Every regex of the TS is a hand matcher
//! ([`super::rx`]) with V8 rows in `js/regex-sites/d2b-f10.mjs`.
//!
//! Strings of the model that are TS strings (`Run.field`, bookmark names, labels) are `&str`
//! here: a lone surrogate in a label cannot be represented (declared, F10.md "Decisions").

use std::sync::Arc;

use super::rx;
use crate::ids::IdAlloc;
use crate::js::{JsNumber, Str};
use crate::model::para::{ParaProps, ParaTabStop, Paragraph};
use crate::model::run::{Run, RunProps};
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::word::obj;
use uo_jsnum::format::to_js_string;

// ───────────────────────── runsBetween, hiddenBookmarkName ─────────────────────────

/// `runsBetween(p, from, to)`: the runs of `[from, to]` of ONE paragraph with their direct
/// formatting (the result of a REF copies it), without what is not formatting.
#[must_use]
pub fn runs_between(p: &Paragraph, from: usize, to: usize) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut acc = 0usize;
    for r in &p.runs {
        let rs = acc;
        acc += r.text.len();
        if let Some(e) = r.ext()
            && (e.image.is_some()
                || e.textbox.is_some()
                || e.shape.is_some()
                || e.group.is_some()
                || e.bookmark.is_some()
                || e.field_begin.is_some()
                || e.field_end == Some(true))
        {
            continue;
        }
        let a = from.saturating_sub(rs).min(r.text.len());
        let b = to.saturating_sub(rs).min(r.text.len());
        if a >= b {
            continue;
        }
        let t = Utf16::from_units(r.text.units().get(a..b).unwrap_or(&[]).to_vec());
        let mut props = (*r.props).clone();
        for k in ["src", "link", "linkTip", "commentIds", "ins", "del", "fmt", "noProof", "webHidden"] {
            obj::delete(&mut props, k);
        }
        if r.ext().is_some_and(|e| e.note_ref.is_some()) && props.vert_align.is_none() {
            obj::set(&mut props, "vertAlign", |x| x.vert_align = Some("superscript".into()));
        }
        let mut run = Run { text: t, props: Arc::new(props), ..Run::default() };
        run.rpr_link = r.rpr_link.clone();
        if let Some(rpr) = &r.rpr {
            run.rpr = Some(Arc::clone(rpr));
            crate::js::JsObject::mark(&mut run, "rpr");
        }
        out.push(run);
    }
    out
}

/// `hiddenBookmarkName(prefix, taken, seed)`: `_Ref`/`_Toc` + 9 digits, sequential from `seed`
/// (`String(seed.n++).padStart(9, '0').slice(-9)`).
pub fn hidden_bookmark_name(prefix: &str, taken: &mut dyn FnMut(&str) -> bool, seed: &mut f64) -> String {
    loop {
        let mut s = to_js_string(*seed);
        *seed += 1.0;
        while s.len() < 9 {
            s.insert(0, '0');
        }
        let tail = s.get(s.len().saturating_sub(9)..).unwrap_or(&s);
        let name = format!("{prefix}{tail}");
        if !taken(&name) {
            return name;
        }
    }
}

// ───────────────────────── cross-references ─────────────────────────

/// `CrossRefType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossRefType {
    Numbered,
    Heading,
    Bookmark,
    Footnote,
    Endnote,
    Caption,
}

/// `CrossRefKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossRefKind {
    Text,
    Page,
    Number,
    NumberNoContext,
    NumberFullContext,
    Position,
    NoteNumber,
    NoteNumberFormatted,
    CaptionEntire,
    CaptionLabel,
    CaptionText,
}

/// `crossRefKinds(type)`: the forms each type offers, in the order of Word's dialog.
#[must_use]
pub fn cross_ref_kinds(t: CrossRefType) -> Vec<CrossRefKind> {
    use CrossRefKind as K;
    match t {
        CrossRefType::Numbered | CrossRefType::Heading | CrossRefType::Bookmark => {
            vec![K::Text, K::Page, K::Number, K::NumberNoContext, K::NumberFullContext, K::Position]
        }
        CrossRefType::Footnote | CrossRefType::Endnote => vec![K::NoteNumber, K::Page, K::Position, K::NoteNumberFormatted],
        CrossRefType::Caption => vec![K::CaptionEntire, K::CaptionLabel, K::CaptionText, K::Page, K::Position],
    }
}

/// `crossRefInstr(name, kind, { hyperlink, separator })`: the instruction as Word writes it.
#[must_use]
pub fn cross_ref_instr(name: &str, kind: CrossRefKind, hyperlink: Option<bool>, separator: Option<&str>) -> String {
    use CrossRefKind as K;
    let h = if hyperlink == Some(false) { "" } else { " \\h" };
    match kind {
        K::Page => format!(" PAGEREF {name}{h} "),
        K::Number => format!(" REF {name} \\r{h} "),
        K::NumberNoContext => format!(" REF {name} \\n{h} "),
        K::NumberFullContext => {
            let d = match separator {
                Some(s) if !s.is_empty() => format!(" \\d \"{s}\""),
                _ => String::new(),
            };
            format!(" REF {name} \\w{h}{d} ")
        }
        K::Position => format!(" REF {name} \\p{h} "),
        K::NoteNumber => format!(" NOTEREF {name}{h} "),
        K::NoteNumberFormatted => format!(" NOTEREF {name} \\f{h} "),
        _ => format!(" REF {name}{h} "),
    }
}

/// `refFieldTarget(instr)`: the bookmark of a REF/PAGEREF/NOTEREF field ("" if it is not one).
#[must_use]
pub fn ref_field_target(instr: Option<&str>) -> String {
    rx::ref_target(instr.unwrap_or("")).unwrap_or("").to_owned()
}

/// `captionRange(kind, len, seqEnd)`: the stretch of the caption paragraph each form marks.
#[must_use]
pub fn caption_range(kind: CrossRefKind, len: usize, seq_end: usize) -> (usize, usize) {
    match kind {
        CrossRefKind::CaptionLabel => (0, len.min(seq_end)),
        CrossRefKind::CaptionText => (len.min(seq_end.saturating_add(1)), len),
        _ => (0, len),
    }
}

// ───────────────────────── captions ─────────────────────────

/// `CaptionFormat`: the `\*` of the SEQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionFormat {
    Arabic,
    AlphabeticUpper,
    AlphabeticLower,
    RomanUpper,
    RomanLower,
}

impl CaptionFormat {
    /// The TS string (`'ARABIC'`, `'ALPHABETIC'`, `'alphabetic'`, `'ROMAN'`, `'roman'`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CaptionFormat::Arabic => "ARABIC",
            CaptionFormat::AlphabeticUpper => "ALPHABETIC",
            CaptionFormat::AlphabeticLower => "alphabetic",
            CaptionFormat::RomanUpper => "ROMAN",
            CaptionFormat::RomanLower => "roman",
        }
    }

    /// `map[f] ?? 'decimal'` of `nextCaptionNumber`: the number format name.
    #[must_use]
    pub fn number_format(self) -> &'static str {
        match self {
            CaptionFormat::Arabic => "decimal",
            CaptionFormat::AlphabeticUpper => "upperLetter",
            CaptionFormat::AlphabeticLower => "lowerLetter",
            CaptionFormat::RomanUpper => "upperRoman",
            CaptionFormat::RomanLower => "lowerRoman",
        }
    }
}

/// `ChapterSeparator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterSeparator {
    Hyphen,
    Period,
    Colon,
    EmDash,
    EnDash,
}

/// `CHAPTER_SEPARATOR_TEXT[sep]` (U+2011 for the hyphen).
#[must_use]
pub fn chapter_separator_text(s: ChapterSeparator) -> &'static str {
    match s {
        ChapterSeparator::Hyphen => "\u{2011}",
        ChapterSeparator::Period => ".",
        ChapterSeparator::Colon => ":",
        ChapterSeparator::EmDash => "\u{2014}",
        ChapterSeparator::EnDash => "\u{2013}",
    }
}

/// The chapter part of a [`CaptionNumbering`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptionChapter {
    /// the heading level (1–9; a number, so hostile values print as JS prints them)
    pub level: f64,
    pub separator: ChapterSeparator,
}

/// `CaptionNumbering`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptionNumbering {
    pub format: CaptionFormat,
    pub chapter: Option<CaptionChapter>,
}

impl Default for CaptionNumbering {
    fn default() -> Self {
        CaptionNumbering { format: CaptionFormat::Arabic, chapter: None }
    }
}

/// `BUILTIN_LABELS` row: the label in each language and where it puts the caption.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinLabel {
    pub en: &'static str,
    pub pt: &'static str,
    pub es: &'static str,
    /// `'above'` (else below)
    pub above: bool,
}

pub const BUILTIN_LABELS: [BuiltinLabel; 3] = [
    BuiltinLabel { en: "Figure", pt: "Figura", es: "Ilustraci\u{f3}n", above: false },
    BuiltinLabel { en: "Table", pt: "Tabela", es: "Tabla", above: true },
    BuiltinLabel { en: "Equation", pt: "Equa\u{e7}\u{e3}o", es: "Ecuaci\u{f3}n", above: true },
];

/// The instructions of a caption's fields (`captionFields`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionFields {
    pub chapter: Option<String>,
    pub seq: String,
}

/// `captionFields(label, n)`.
#[must_use]
pub fn caption_fields(label: &str, n: &CaptionNumbering) -> CaptionFields {
    let id = if label.chars().any(uo_jsnum::text::is_js_space) { format!("\"{label}\"") } else { label.to_owned() };
    let f = n.format.as_str();
    match &n.chapter {
        None => CaptionFields { chapter: None, seq: format!(" SEQ {id} \\* {f} ") },
        Some(c) => {
            let lv = to_js_string(c.level);
            CaptionFields { chapter: Some(format!(" STYLEREF {lv} \\s ")), seq: format!(" SEQ {id} \\* {f} \\s {lv} ") }
        }
    }
}

/// `seqLabel(instr)`: the label of a SEQ field ("" if it is not one).
#[must_use]
pub fn seq_label(instr: Option<&str>) -> String {
    rx::seq_label(instr.unwrap_or("")).unwrap_or("").to_owned()
}

// ───────────────────────── the TOC field ─────────────────────────

/// A style of the `\t` switch: `{ style, level }`.
#[derive(Debug, Clone, PartialEq)]
pub struct TocStyle {
    pub style: String,
    pub level: f64,
}

/// `TocOptions`: the options of a TOC field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TocOptions {
    /// `\o "a-b"`
    pub outline: Option<(f64, f64)>,
    /// `\u`
    pub outline_levels: bool,
    /// `\t "Style;level;…"`
    pub styles: Vec<TocStyle>,
    /// `\c "Label"` (with label and number) or `\a "Label"` (the text only)
    pub caption: Option<String>,
    pub include_label: Option<bool>,
    /// `\f [id]`
    pub fields: bool,
    pub field_id: Option<String>,
    /// `\l "a-b"`
    pub field_levels: Option<(f64, f64)>,
    /// `\h`
    pub hyperlinks: bool,
    /// `\n`
    pub no_page_numbers: bool,
    /// `\n "a-b"`
    pub no_page_levels: Option<(f64, f64)>,
    /// `\p "x"`
    pub separator: Option<String>,
    /// `\z`
    pub hide_in_web: bool,
}

/// `parseTocInstr(instr)`.
#[must_use]
pub fn parse_toc_instr(instr: &str) -> TocOptions {
    let mut o = TocOptions::default();
    let q = |sw: u8| rx::switch_quoted(instr, sw);
    let has = |sw: u8| rx::has_switch(instr, sw);
    match q(b'o') {
        Some(ol) => o.outline = Some(rx::digit_range(&ol).unwrap_or((1.0, 9.0))),
        None if has(b'o') => o.outline = Some((1.0, 9.0)),
        None => {}
    }
    if has(b'u') {
        o.outline_levels = true;
    }
    if let Some(t) = q(b't').filter(|t| !t.is_empty()) {
        let parts: Vec<&str> = t.split([';', ',']).collect();
        let mut i = 0;
        while i + 1 < parts.len() {
            let level = uo_jsnum::parse::parse_int(parts.get(i + 1).copied().unwrap_or(""), 10);
            let level = if level == 0.0 || level.is_nan() { 1.0 } else { level };
            o.styles.push(TocStyle { style: uo_jsnum::text::trim(parts.get(i).copied().unwrap_or("")).to_owned(), level });
            i += 2;
        }
    }
    let (c, a) = (q(b'c'), q(b'a'));
    if let Some(c) = c {
        o.caption = Some(c);
        o.include_label = Some(true);
    } else if let Some(a) = a {
        o.caption = Some(a);
        o.include_label = Some(false);
    }
    if let Some(id) = rx::switch_f(instr) {
        o.fields = true;
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            o.field_id = Some(id);
        }
    }
    if let Some(l) = q(b'l').and_then(|v| rx::digit_range(&v)) {
        o.field_levels = Some(l);
    }
    if has(b'h') {
        o.hyperlinks = true;
    }
    if has(b'n') {
        match q(b'n').and_then(|v| rx::digit_range(&v)) {
            Some(r) => o.no_page_levels = Some(r),
            None => o.no_page_numbers = true,
        }
    }
    if let Some(p) = q(b'p') {
        o.separator = Some(p);
    }
    if has(b'z') {
        o.hide_in_web = true;
    }
    o
}

/// `tocInstr(o)`: the instruction in the order Word writes it.
#[must_use]
pub fn toc_instr(o: &TocOptions) -> String {
    let mut parts: Vec<String> = vec!["TOC".to_owned()];
    if let Some((a, b)) = o.outline {
        parts.push(format!("\\o \"{}-{}\"", to_js_string(a), to_js_string(b)));
    }
    if o.fields {
        match o.field_id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => {
                let quoted = id.chars().any(|c| uo_jsnum::text::is_js_space(c) || c == '"');
                parts.push(if quoted { format!("\\f \"{id}\"") } else { format!("\\f {id}") });
            }
            None => parts.push("\\f".to_owned()),
        }
    }
    if let Some((a, b)) = o.field_levels {
        parts.push(format!("\\l \"{}-{}\"", to_js_string(a), to_js_string(b)));
    }
    if let Some((a, b)) = o.no_page_levels {
        parts.push(format!("\\n \"{}-{}\"", to_js_string(a), to_js_string(b)));
    } else if o.no_page_numbers {
        parts.push("\\n".to_owned());
    }
    if let Some(s) = &o.separator {
        parts.push(format!("\\p \"{s}\""));
    }
    if o.hyperlinks {
        parts.push("\\h".to_owned());
    }
    if o.hide_in_web {
        parts.push("\\z".to_owned());
    }
    if let Some(c) = &o.caption {
        let sw = if o.include_label == Some(false) { 'a' } else { 'c' };
        parts.push(format!("\\{sw} \"{c}\""));
    }
    if !o.styles.is_empty() {
        let list: Vec<String> = o.styles.iter().map(|s| format!("{};{}", s.style, to_js_string(s.level))).collect();
        parts.push(format!("\\t \"{}\"", list.join(";")));
    }
    if o.outline_levels {
        parts.push("\\u".to_owned());
    }
    format!(" {} ", parts.join(" "))
}

/// `TocSource`: one entry of the index: the source paragraph and what shows.
#[derive(Debug, Clone, PartialEq)]
pub struct TocSource {
    /// the level (1 for captions)
    pub level: f64,
    pub text: Utf16,
    /// the heading's list number, goes before with a tab
    pub number: Option<Utf16>,
    /// the page shown
    pub page: String,
    /// the `_Toc` bookmark of the source paragraph
    pub bookmark: String,
    /// no page number (a TC with `\n`)
    pub no_page: bool,
}

/// `TocLook` without the style callback (that is `style_for` of [`toc_entry_paragraphs`]).
#[derive(Debug, Clone, PartialEq)]
pub struct TocLook {
    /// the document's character style `Hyperlink` ("" without `\h`)
    pub hyperlink_style: String,
    /// the right tab: the text width minus 10 twips
    pub tab_pos: f64,
    /// the tab leader (`Some("none")` = none)
    pub leader: Option<String>,
}

fn level_in(r: Option<(f64, f64)>, level: f64) -> bool {
    r.is_some_and(|(a, b)| level >= a && level <= b)
}

/// `tocEntryParagraphs(o, sources, look)`: the entry paragraphs as Word generates them. `style_for`
/// is the TS `look.styleFor(level)` (it defines the built-in style on first use).
pub fn toc_entry_paragraphs(
    o: &TocOptions,
    sources: &[TocSource],
    look: &TocLook,
    ids: &mut IdAlloc,
    style_for: &mut dyn FnMut(f64) -> Str,
) -> Vec<Paragraph> {
    let mut tab = ParaTabStop { pos_twips: JsNumber::Num(look.tab_pos), align: "right".into(), ..ParaTabStop::default() };
    if let Some(l) = look.leader.as_deref().filter(|l| *l != "none") {
        tab.leader = Some(l.into());
    }
    let tabs = vec![tab];
    let mut out = Vec::with_capacity(sources.len());
    for s in sources {
        let mut base = default_run_props();
        if o.hyperlinks {
            obj::set(&mut base, "link", |p| p.link = Some(format!("#{}", s.bookmark).into()));
        }
        obj::set(&mut base, "noProof", |p| p.no_proof = Some(true));
        let linked = |base: &RunProps| -> RunProps {
            let mut p = base.clone();
            if o.hyperlinks {
                obj::set(&mut p, "charStyle", |x| x.char_style = Some(look.hyperlink_style.as_str().into()));
            }
            p
        };
        let webbed = |base: &RunProps| -> RunProps {
            let mut p = base.clone();
            if o.hide_in_web {
                obj::set(&mut p, "webHidden", |x| x.web_hidden = Some(true));
            }
            p
        };
        let in_toc = |text: Utf16, props: RunProps| -> Run {
            let mut r = Run { text, props: Arc::new(props), ..Run::default() };
            r.ext_mut().in_toc = Some(true);
            r
        };
        let mut runs: Vec<Run> = Vec::new();
        if let Some(n) = s.number.as_ref().filter(|n| !n.is_empty()) {
            runs.push(in_toc(n.clone(), linked(&base)));
            runs.push(in_toc(Utf16::from("\t"), linked(&base)));
        }
        let text: Vec<u16> = s.text.units().iter().map(|&u| if matches!(u, 9..=13) { 32 } else { u }).collect();
        runs.push(in_toc(Utf16::from_units(text), linked(&base)));
        if !o.no_page_numbers && !s.no_page && !level_in(o.no_page_levels, s.level) {
            runs.push(in_toc(Utf16::from(o.separator.as_deref().unwrap_or("\t")), webbed(&base)));
            let instr = format!(" PAGEREF {} \\h ", s.bookmark);
            let result = Run { text: Utf16::from(s.page.as_str()), props: Arc::new(webbed(&base)), ..Run::default() };
            let mut r = Run { text: Utf16::from(s.page.as_str()), props: Arc::new(webbed(&base)), ..Run::default() };
            {
                let e = r.ext_mut();
                e.field = Some(instr.into());
                e.field_result = Some(vec![result]);
                e.in_toc = Some(true);
            }
            runs.push(r);
        }
        let props = ParaProps { style_id: Some(style_for(s.level)), tab_stops: Some(tabs.clone()), ..ParaProps::default() };
        out.push(Paragraph { runs, props: Arc::new(props), id: ids.para(), ..Paragraph::default() });
    }
    out
}

/// `TOF_EMPTY`/`TOC_EMPTY`: the empty-index message, bold, in each language.
pub struct EmptyMsg {
    pub en: &'static str,
    pub pt: &'static str,
    pub es: &'static str,
}

pub const TOF_EMPTY: EmptyMsg = EmptyMsg {
    en: "No table of figures entries found.",
    pt: "Nenhuma entrada de \u{ed}ndice de ilustra\u{e7}\u{f5}es foi encontrada.",
    es: "No se ha encontrado ninguna entrada de tabla de ilustraciones.",
};
pub const TOC_EMPTY: EmptyMsg = EmptyMsg {
    en: "No table of contents entries found.",
    pt: "Nenhuma entrada de sum\u{e1}rio foi encontrada.",
    es: "No se ha encontrado ninguna entrada de tabla de contenido.",
};

/// `TOF_FORMATS[f]`: what each format of the Table of Figures dialog puts in the "table of
/// figures" style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TofFormat {
    Template,
    Classic,
    Distinctive,
    Centered,
    Formal,
    Simple,
}

/// `(pPr, rPr)` of a [`TofFormat`].
#[must_use]
pub fn tof_format(f: TofFormat) -> (&'static str, &'static str) {
    match f {
        TofFormat::Template => ("<w:spacing w:after=\"0\"/>", ""),
        TofFormat::Classic => {
            ("<w:spacing w:after=\"0\"/><w:ind w:left=\"440\" w:hanging=\"440\"/>", "<w:caps/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/>")
        }
        TofFormat::Distinctive => ("<w:spacing w:after=\"0\"/>", "<w:i/><w:iCs/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/>"),
        TofFormat::Centered => (
            "<w:spacing w:after=\"0\"/><w:ind w:left=\"440\" w:hanging=\"440\"/><w:jc w:val=\"center\"/>",
            "<w:b/><w:bCs/><w:i/><w:iCs/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/>",
        ),
        TofFormat::Formal => (
            "<w:spacing w:after=\"0\"/><w:ind w:left=\"440\" w:hanging=\"440\"/>",
            "<w:smallCaps/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/>",
        ),
        TofFormat::Simple => (
            "<w:spacing w:after=\"0\"/><w:ind w:left=\"440\" w:hanging=\"440\"/>",
            "<w:b/><w:bCs/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/>",
        ),
    }
}
