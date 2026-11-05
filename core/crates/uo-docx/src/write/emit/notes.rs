//! Frontier **W4** (notes, write side; `notes.ts`): `ensureNoteParts` (parts, relationships,
//! content types and the separator references in the settings), `patchSettingsNotes`,
//! `NOTE_STYLES`, `noteMarkField`, `noteRefRun`, `newNoteParagraphs`. The comments part
//! (`commentXml`, `patchComments`) is the core's (`write::parts`, a dynamic-regex site).
//!
//! Ported already (the section surgery compares note props on every save): [`note_props_inner`],
//! [`note_props_xml`], [`same_note_props`], [`empty_notes_part`].

use std::collections::HashSet;
use std::sync::Arc;

use uo_jsnum::text::is_js_space;
use uo_opc::Package;

use crate::ids::IdAlloc;
use crate::js::{JsNumber, JsObject, Str};
use crate::model::formatting::NoteProps;
use crate::model::run::{NoteRef, Run};
use crate::model::{ParaProps, Paragraph};
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::write::fonts::ooxml::NextFind;
use crate::write::lossless::WResult;
use crate::write::matchers::find_from;
use crate::write::parts::{CONTENT_TYPES, DOC_RELS, EMPTY_RELS, insert_before_first, split_last_unit};

use super::settings::self_closing_matches;

/// `notePropsInner(p)`: CT_FtnProps / CT_EdnProps order (values unescaped, as the TS).
#[must_use]
pub fn note_props_inner(p: Option<&NoteProps>) -> String {
    let Some(p) = p else { return String::new() };
    let mut s = String::new();
    if let Some(v) = p.pos.as_deref().filter(|v| !v.is_empty()) {
        s.push_str(&format!("<w:pos w:val=\"{v}\"/>"));
    }
    if let Some(v) = p.num_fmt.as_deref().filter(|v| !v.is_empty()) {
        s.push_str(&format!("<w:numFmt w:val=\"{v}\"/>"));
    }
    if let Some(v) = p.num_start {
        s.push_str(&format!("<w:numStart w:val=\"{}\"/>", crate::write::js::num_str(v)));
    }
    if let Some(v) = p.num_restart.as_deref().filter(|v| !v.is_empty()) {
        s.push_str(&format!("<w:numRestart w:val=\"{v}\"/>"));
    }
    s
}

/// `notePropsXml(kind, p)`: `w:footnotePr` / `w:endnotePr` (empty = nothing).
#[must_use]
pub fn note_props_xml(kind: &str, p: Option<&NoteProps>) -> String {
    let inner = note_props_inner(p);
    if inner.is_empty() { inner } else { format!("<w:{kind}Pr>{inner}</w:{kind}Pr>") }
}

/// `sameNoteProps(a, b)`.
#[must_use]
pub fn same_note_props(a: Option<&NoteProps>, b: Option<&NoteProps>) -> bool {
    note_props_inner(a) == note_props_inner(b)
}

const SEP_PPR: &str = "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>";

const NOTES_NS: &str = concat!(
    "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" ",
    "xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\" mc:Ignorable=\"w14 w15\""
);

/// `emptyNotesPart(kind)`: a notes part with only the separators.
#[must_use]
pub fn empty_notes_part(kind: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:{kind}s {NOTES_NS}><w:{kind} w:type=\"separator\" w:id=\"-1\"><w:p>{SEP_PPR}<w:r><w:separator/></w:r></w:p></w:{kind}><w:{kind} w:type=\"continuationSeparator\" w:id=\"0\"><w:p>{SEP_PPR}<w:r><w:continuationSeparator/></w:r></w:p></w:{kind}></w:{kind}s>"
    )
}

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.";

/// `new RegExp(`Type="${REL}/${kind}s"`).test(rels)`: the `.` of the namespace in the TS pattern
/// is a wildcard (one unit that is not a line terminator), so the three literal pieces around
/// the two dots are matched with one character between them.
#[must_use]
pub fn has_note_relationship(rels: &str, kind: &str) -> bool {
    let tail = format!("org/officeDocument/2006/relationships/{kind}s\"");
    let pieces = ["Type=\"http://schemas", "openxmlformats", tail.as_str()];
    let wild = |c: char| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') && c.len_utf16() == 1;
    let mut from = 0;
    'starts: while let Some(i) = find_from(rels, pieces[0], from) {
        from = i + 1;
        let mut j = i + pieces[0].len();
        for piece in &pieces[1..] {
            let Some(c) = rels.get(j..).and_then(|r| r.chars().next()).filter(|&c| wild(c)) else { continue 'starts };
            j += c.len_utf8();
            if !rels.get(j..).is_some_and(|r| r.starts_with(piece)) {
                continue 'starts;
            }
            j += piece.len();
        }
        return true;
    }
    false
}

/// `let n = 1; while (rels.includes(`Id="rId${n}"`)) n++;`: the smallest `n` from 1 with no
/// `Id="rId<n>"` in the text (a scan of the ids present, not one search per `n`).
#[must_use]
pub fn free_rid_number(rels: &str) -> u64 {
    const PREFIX: &str = "Id=\"rId";
    let mut used: HashSet<u64> = HashSet::new();
    let mut from = 0;
    while let Some(i) = find_from(rels, PREFIX, from) {
        from = i + 1;
        let digits = rels.as_bytes().get(i + PREFIX.len()..).unwrap_or(&[]);
        let n = digits.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 || n > 15 || digits.get(n) != Some(&b'"') || digits.first() == Some(&b'0') {
            continue;
        }
        if let Some(v) = digits.get(..n).and_then(|d| std::str::from_utf8(d).ok()).and_then(|d| d.parse::<u64>().ok()) {
            used.insert(v);
        }
    }
    let mut n = 1;
    while used.contains(&n) {
        n += 1;
    }
    n
}

/// `ensureNoteParts(pkg)`: `footnotes.xml` and `endnotes.xml` (Word creates both when the first
/// note of either kind is inserted), their relationships, their content types and the references
/// to the separators in the settings. `true` when something changed.
pub fn ensure_note_parts(pkg: &mut Package) -> WResult<bool> {
    let mut changed = false;
    let text_of = |pkg: &Package, name: &str| -> WResult<Option<String>> { Ok(pkg.part_text(name)?.map(std::borrow::Cow::into_owned)) };
    let mut rels = text_of(pkg, DOC_RELS)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
    let mut ct = text_of(pkg, CONTENT_TYPES)?.unwrap_or_default();
    for kind in ["footnote", "endnote"] {
        let path = format!("word/{kind}s.xml");
        if pkg.part(&path)?.is_none() {
            pkg.set_part_text(path.clone(), empty_notes_part(kind));
            changed = true;
        }
        if !has_note_relationship(&rels, kind) {
            let n = free_rid_number(&rels);
            let rel = format!("<Relationship Id=\"rId{n}\" Type=\"{REL}/{kind}s\" Target=\"{kind}s.xml\"/>");
            rels = insert_before_first(&rels, "</Relationships>", &rel);
            changed = true;
        }
        if !ct.is_empty() && !ct.contains(&format!("PartName=\"/{path}\"")) {
            let over = format!("<Override PartName=\"/{path}\" ContentType=\"{CT}{kind}s+xml\"/>");
            ct = insert_before_first(&ct, "</Types>", &over);
            changed = true;
        }
    }
    if changed {
        pkg.set_part_text(DOC_RELS, rels);
        if !ct.is_empty() {
            pkg.set_part_text(CONTENT_TYPES, ct);
        }
    }
    if let Some(settings) = text_of(pkg, "word/settings.xml")?.filter(|t| !t.is_empty()) {
        let next = patch_settings_notes(&settings, NotePropsArg::Unchanged, NotePropsArg::Unchanged);
        if next != settings {
            pkg.set_part_text("word/settings.xml", next);
            changed = true;
        }
    }
    Ok(changed)
}

/// `SETTINGS_AFTER_NOTES`: the order of CT_Settings up to `w:compat` — `footnotePr` and `endnotePr`
/// come right before it.
const SETTINGS_AFTER_NOTES: [&str; 18] = [
    "w:compat",
    "w:docVars",
    "w:rsids",
    "m:mathPr",
    "w:attachedSchema",
    "w:themeFontLang",
    "w:clrSchemeMapping",
    "w:doNotIncludeSubdocsInStats",
    "w:doNotAutoCompressPictures",
    "w:forceUpgrade",
    "w:captions",
    "w:readModeInkLockDown",
    "w:smartTagType",
    "sl:schemaLibrary",
    "w:shapeDefaults",
    "w:doNotEmbedSmartTags",
    "w:decimalSymbol",
    "w:listSeparator",
];

/// What `patchSettingsNotes` does with the properties of a kind: `undefined` (leave as they are),
/// `null` (take them out) or the new properties.
#[derive(Debug, Clone, Copy)]
pub enum NotePropsArg<'a> {
    Unchanged,
    Remove,
    Set(&'a NoteProps),
}

/// `/<w:footnotePr(?:\s[^>]*)?(?:\/>|>[\s\S]*?<\/w:footnotePr>)/.exec(s)` (the first match, byte
/// range): the element, or a self-closing tag, or (when the start tag has white space and a `/>`
/// ends it, as the greedy `[^>]*` finds the longest first) the element to a later close tag.
#[must_use]
pub fn settings_props_element(s: &str, tag: &str) -> Option<(usize, usize)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut from = 0;
    let mut gts = NextFind::new(s, ">", false);
    let mut closes = NextFind::new(s, &close, false);
    while let Some(lt) = find_from(s, &open, from) {
        from = lt + 1;
        let pos = lt + open.len();
        let Some(c) = s.get(pos..).and_then(|r| r.chars().next()) else { continue };
        let end = if is_js_space(c) {
            let Some(gt) = gts.at(pos) else { continue };
            if let Some(h) = closes.at(gt + 1) {
                Some(h + close.len())
            } else if gt > pos && s.as_bytes().get(gt - 1) == Some(&b'/') {
                Some(gt + 1)
            } else {
                None
            }
        } else if c == '/' {
            (s.as_bytes().get(pos + 1) == Some(&b'>')).then_some(pos + 2)
        } else if c == '>' {
            closes.at(pos + 1).map(|h| h + close.len())
        } else {
            None
        };
        if let Some(end) = end {
            return Some((lt, end));
        }
    }
    None
}

/// `out.search(new RegExp(`<${name}[\s/>]`))`: the first `<NAME` followed by white space, `/` or `>`.
#[must_use]
pub fn first_tag_open(s: &str, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(i) = find_from(s, &open, from) {
        from = i + 1;
        let next = s.get(i + open.len()..).and_then(|r| r.chars().next());
        if next.is_some_and(|c| is_js_space(c) || c == '/' || c == '>') {
            return Some(i);
        }
    }
    None
}

/// `patchSettingsNotes(xml, { footnote, endnote })`: the settings with the `w:footnotePr` /
/// `w:endnotePr` of the document: `Set` replaces the properties, `Remove` takes them out,
/// `Unchanged` leaves them; the references to the separators (-1, 0) stay always. The rest of the
/// file goes out as it was.
#[must_use]
pub fn patch_settings_notes(xml: &str, footnote: NotePropsArg<'_>, endnote: NotePropsArg<'_>) -> String {
    let mut out = xml.to_owned();
    for (kind, want) in [("footnote", footnote), ("endnote", endnote)] {
        let tag = format!("w:{kind}Pr");
        let seps = format!("<w:{kind} w:id=\"-1\"/><w:{kind} w:id=\"0\"/>");
        if let Some((a, b)) = settings_props_element(&out, &tag) {
            let whole = out.get(a..b).unwrap_or("");
            let close = format!("</{tag}>");
            let body = if whole.ends_with(&close) {
                let from = whole.find('>').map_or(0, |i| i + 1);
                whole.get(from..whole.len() - close.len()).unwrap_or("")
            } else {
                ""
            };
            let refs_at = self_closing_matches(body, &format!("w:{kind}"));
            let refs: String = refs_at.iter().filter_map(|&(x, y)| body.get(x..y)).collect();
            let p = match want {
                NotePropsArg::Unchanged => {
                    let mut rest = String::with_capacity(body.len());
                    let mut last = 0;
                    for &(x, y) in &refs_at {
                        rest.push_str(body.get(last..x).unwrap_or(""));
                        last = y;
                    }
                    rest.push_str(body.get(last..).unwrap_or(""));
                    rest
                }
                NotePropsArg::Remove => String::new(),
                NotePropsArg::Set(np) => note_props_inner(Some(np)),
            };
            let tail = if refs.is_empty() { seps } else { refs };
            let next = format!("<{tag}>{p}{tail}</{tag}>");
            if next != whole {
                out = format!("{}{next}{}", out.get(..a).unwrap_or(""), out.get(b..).unwrap_or(""));
            }
        } else {
            let inner = match want {
                NotePropsArg::Set(np) => note_props_inner(Some(np)),
                _ => String::new(),
            };
            let el = format!("<{tag}>{inner}{seps}</{tag}>");
            let mut at = if kind == "footnote" { out.find("<w:endnotePr") } else { None };
            if at.is_none() {
                for n in SETTINGS_AFTER_NOTES {
                    if let Some(k) = first_tag_open(&out, n)
                        && at.is_none_or(|x| k < x)
                    {
                        at = Some(k);
                    }
                }
            }
            out = match at.or_else(|| out.rfind("</w:settings>")) {
                Some(at) => format!("{}{el}{}", out.get(..at).unwrap_or(""), out.get(at..).unwrap_or("")),
                None => {
                    let (head, last) = split_last_unit(&out);
                    format!("{head}{el}{last}")
                }
            };
        }
    }
    out
}

/// One of the note styles Word adds with the first note (`NOTE_STYLES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteStyle {
    pub id: &'static str,
    pub name: &'static str,
    pub xml: &'static str,
}

/// The three styles of a kind (`NOTE_STYLES[kind]`): the text, its character style and the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteStyles {
    pub text: NoteStyle,
    pub char: NoteStyle,
    pub r#ref: NoteStyle,
}

/// `NOTE_STYLES.footnote`.
pub const FOOTNOTE_STYLES: NoteStyles = NoteStyles {
    text: NoteStyle {
        id: "Textodenotaderodap",
        name: "footnote text",
        xml: "<w:style w:type=\"paragraph\" w:styleId=\"Textodenotaderodap\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:link w:val=\"TextodenotaderodapChar\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>",
    },
    char: NoteStyle {
        id: "TextodenotaderodapChar",
        name: "Texto de nota de rodapé Char",
        xml: "<w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"TextodenotaderodapChar\"><w:name w:val=\"Texto de nota de rodapé Char\"/><w:basedOn w:val=\"Fontepargpadro\"/><w:link w:val=\"Textodenotaderodap\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>",
    },
    r#ref: NoteStyle {
        id: "Refdenotaderodap",
        name: "footnote reference",
        xml: "<w:style w:type=\"character\" w:styleId=\"Refdenotaderodap\"><w:name w:val=\"footnote reference\"/><w:basedOn w:val=\"Fontepargpadro\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr></w:style>",
    },
};

/// `NOTE_STYLES.endnote`.
pub const ENDNOTE_STYLES: NoteStyles = NoteStyles {
    text: NoteStyle {
        id: "Textodenotadefim",
        name: "endnote text",
        xml: "<w:style w:type=\"paragraph\" w:styleId=\"Textodenotadefim\"><w:name w:val=\"endnote text\"/><w:basedOn w:val=\"Normal\"/><w:link w:val=\"TextodenotadefimChar\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>",
    },
    char: NoteStyle {
        id: "TextodenotadefimChar",
        name: "Texto de nota de fim Char",
        xml: "<w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"TextodenotadefimChar\"><w:name w:val=\"Texto de nota de fim Char\"/><w:basedOn w:val=\"Fontepargpadro\"/><w:link w:val=\"Textodenotadefim\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>",
    },
    r#ref: NoteStyle {
        id: "Refdenotadefim",
        name: "endnote reference",
        xml: "<w:style w:type=\"character\" w:styleId=\"Refdenotadefim\"><w:name w:val=\"endnote reference\"/><w:basedOn w:val=\"Fontepargpadro\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr></w:style>",
    },
};

/// `NOTE_STYLES[kind]`: `None` for a kind that is neither `footnote` nor `endnote`.
#[must_use]
pub fn note_styles(kind: &str) -> Option<&'static NoteStyles> {
    match kind {
        "footnote" => Some(&FOOTNOTE_STYLES),
        "endnote" => Some(&ENDNOTE_STYLES),
        _ => None,
    }
}

/// `NOTE_REF_TEXT`: the text of the reference run in the editor, one character (the number is the
/// engine's).
pub const NOTE_REF_TEXT: &str = "\u{FFFC}";

/// `noteMarkField(kind)`: the field of the note's own mark in the model.
#[must_use]
pub fn note_mark_field(kind: &str) -> &'static str {
    if kind == "footnote" { " FOOTNOTEREF_MARK " } else { " ENDNOTEREF_MARK " }
}

/// `{ ...DEFAULT_RUN_PROPS, ...(refStyle ? { charStyle: refStyle } : { vertAlign: 'superscript' }) }`.
fn ref_run_props(ref_style: Option<&str>) -> Arc<crate::model::RunProps> {
    let mut props = default_run_props();
    match ref_style.filter(|s| !s.is_empty()) {
        Some(style) => props.char_style = Some(Str::from(style)),
        None => props.vert_align = Some(Str::from("superscript")),
    }
    Arc::new(props)
}

/// `noteRefRun(kind, id, refStyle)`: the run of the reference in the text (reference style; the
/// number is the engine's).
#[must_use]
pub fn note_ref_run(kind: &str, id: f64, ref_style: Option<&str>) -> Run {
    let mut run = Run { text: Utf16::from(NOTE_REF_TEXT), props: ref_run_props(ref_style), ..Run::default() };
    run.ext_mut().note_ref = Some(NoteRef { kind: Str::from(kind), id: JsNumber::Num(id), ..NoteRef::default() });
    run
}

/// `newNoteParagraphs(kind, textStyle, refStyle)`: the content of a new note: the mark and a space
/// (the cursor goes after it). The paragraph is a new object: it takes a new id from `ids`.
#[must_use]
pub fn new_note_paragraphs(kind: &str, text_style: Option<&str>, ref_style: Option<&str>, ids: &mut IdAlloc) -> Vec<Paragraph> {
    let mut mark = Run { text: Utf16::from(NOTE_REF_TEXT), props: ref_run_props(ref_style), ..Run::default() };
    mark.ext_mut().field = Some(Str::from(note_mark_field(kind)));
    let space = Run { text: Utf16::from(" "), props: Arc::new(default_run_props()), ..Run::default() };
    let mut props = ParaProps::default();
    if let Some(style) = text_style.filter(|s| !s.is_empty()) {
        props.style_id = Some(Str::from(style));
    }
    let mut para = Paragraph { runs: vec![mark, space], props: Arc::new(props), ..Paragraph::empty(ids.para()) };
    para.mark("props");
    para.mark("runs");
    vec![para]
}
