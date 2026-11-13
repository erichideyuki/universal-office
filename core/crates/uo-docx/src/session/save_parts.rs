//! Wave 7c (frontier S): what a save puts into the session's package outside the writer, and what a later save takes
//! back first.
//!
//! - `addStylesXml(pkg, pendingTableStyles)` (docx-core table-styles.ts 149–171, called last by `saveDocxBody`, 8641): the
//!   built-in table styles a table took in a document whose style sheet is not editable (an `.odt`) or that has no styles
//!   part (`tableStyleId`, universal-write.ts 19283–19293), appended to `word/styles.xml` (the part, its content type and
//!   its relationship created when missing).
//! - **Word manda sobre o TS** (measured, Word 16.0 pt-BR, invisible COM under word-lock; `core/target/m2-w7b/s-plan/w-out`
//!   and `core/target/m2-w7b/s7c/w-out`): a part a save wrote for an edit that was then undone is gone at the next save —
//!   a table inserted, saved, undone, saved: "Tabela com grade" gone (styles 14 → 13 in the `.odt`, 5 → 4 in the
//!   `.docx`); a style created, saved, undone, saved: gone (6 → 4); a list applied, saved, undone, saved: the numbering
//!   part gone (1 num → no part); different odd and even pages, automatic hyphenation and the footnote number format
//!   applied, saved, undone, saved: `w:evenAndOddHeaders`, `w:autoHyphenation` and the `w:numFmt` of `w:footnotePr` gone
//!   from `settings.xml`; the default tab stop through the Tabs dialog's command (`WordBasic.FormatTabs DefTabs`, which
//!   records an undo step; `Document.DefaultTabStop` does not) the same: 1440 in the first copy, the loaded 708 after the
//!   undo (`core/target/m2-w7b/s7d/w-out`, fix round 1). A note option set and then set back: the second save has the
//!   current options (`w:numFmt` gone). The note styles a footnote insertion added stay after the undo, with or without
//!   a save in between ([`SaveParts::note_styles`]). The frozen TS writes these parts only when they differ from the
//!   load, into the package it reuses between saves, so the first save's part stays. `OpenOptions::ts_stale_parts` and
//!   `OpenOptions::ts_table_style_survives_undo` keep the TS behaviour for the strict replays.

use std::sync::Arc;

use indexmap::IndexMap;
use uo_opc::Package;

use crate::error::DocxError;

/// `const W = 'xmlns:w="…/wordprocessingml/2006/main"'` (table-styles.ts:11).
const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
const STYLES_PATH: &str = "word/styles.xml";
const DOC_RELS: &str = "word/_rels/document.xml.rels";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const STYLES_OVERRIDE: &str =
    "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>";
const EMPTY_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";
const NUMBERING_REL: &str = "<Relationship Id=\"rIdWnNum\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>";
const NUMBERING_OVERRIDE: &str = "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>";
const SETTINGS_PATH: &str = "word/settings.xml";
const SETTINGS_REL: &str = "<Relationship Id=\"rIdWnSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>";
const SETTINGS_OVERRIDE: &str = "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>";

/// What [`add_styles_xml`] changed in a package, so that the next save can take it back first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddedStyles {
    /// the text of `word/styles.xml` before (`None`: the part was created)
    pub before: Option<String>,
    /// the relationships part before the styles relationship was added (`None`: not touched; `Some(None)`: created)
    pub rels_before: Option<Option<String>>,
    /// whether the content type override was added
    pub ct_added: bool,
}

/// The parts the saves wrote outside the writer and the options of the TS behaviour (see the module comment).
#[derive(Debug, Clone, Default)]
pub(crate) struct SaveParts {
    /// `OpenOptions::ts_stale_parts`
    pub ts_stale_parts: bool,
    /// `OpenOptions::ts_table_style_survives_undo`
    pub ts_table_style_survives_undo: bool,
    /// `settings.xml` as loaded (path and text; `None`: no settings part at the open)
    pub loaded_settings: Option<(String, Arc<str>)>,
    /// a save wrote the numbering part (`save_numbering`)
    pub numbering_written: bool,
    /// the numbering part's path at the open (`None`: no numbering part)
    pub numbering_path: Option<String>,
    /// what the last save's [`add_styles_xml`] changed
    pub table_styles: Option<AddedStyles>,
    /// a command made the note parts (`ensureNoteParts`: the parts, and the separators' ids in `settings.xml`): a command-time
    /// change of the package, not an undoable one (the TS's too), made again after `settings.xml` goes back to the open's
    pub note_parts: bool,
    /// a save wrote `w:displayBackgroundShape` (a page colour): Word writes it from the application's "show background"
    /// view option, not from the document's state (memory: Word page background, measured 2026-10-22), so it stays after the
    /// colour goes, as the TS's reused package keeps it; made again after `settings.xml` goes back to the open's
    pub display_background: bool,
    /// wave 7c (S, fix round 1): the note styles the note commands defined (id, `w:style` element; [`added_note_styles`]),
    /// outside the history: Word keeps them in the file after the insertion is undone (a deleted style leaves the list)
    pub note_styles: Vec<(String, String)>,
}

fn text(pkg: &Package, path: &str) -> Result<Option<String>, DocxError> {
    Ok(pkg.part_text(path)?.map(std::borrow::Cow::into_owned))
}

/// `s.replace(from, () => to)` (the first occurrence; no `$` patterns).
fn replace_first(s: &str, from: &str, to: &str) -> String {
    match s.find(from) {
        Some(i) => {
            let mut out = String::with_capacity(s.len() + to.len());
            out.push_str(&s[..i]);
            out.push_str(to);
            out.push_str(&s[i + from.len()..]);
            out
        }
        None => s.to_owned(),
    }
}

/// `new RegExp('w:styleId="' + escape(id) + '"').test(xml)`: a literal search.
fn has_style_id(xml: &str, id: &str) -> bool {
    xml.contains(&format!("w:styleId=\"{id}\""))
}

/// `addStylesXml(pkg, styles)` (table-styles.ts 149–171): the `w:style` definitions of `styles` the part does not hold yet,
/// appended before `</w:styles>` (the rest byte for byte); without a styles part, a minimal one with its content type
/// override and its relationship (the first free `rId<n>`). Returns what it changed (`None`: nothing).
///
/// # Errors
/// A part of the package that cannot be read as text.
pub(crate) fn add_styles_xml(pkg: &mut Package, styles: &IndexMap<String, String>) -> Result<Option<AddedStyles>, DocxError> {
    if styles.is_empty() {
        return Ok(None);
    }
    let before = text(pkg, STYLES_PATH)?.filter(|t| !t.is_empty());
    let mut rels_before: Option<Option<String>> = None;
    let mut ct_added = false;
    let xml = match &before {
        Some(x) => x.clone(),
        None => {
            let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles {W}></w:styles>");
            if let Some(ct) = text(pkg, CONTENT_TYPES)?.filter(|t| !t.is_empty())
                && !ct.contains("/word/styles.xml")
            {
                pkg.set_part_text(CONTENT_TYPES, replace_first(&ct, "</Types>", &format!("{STYLES_OVERRIDE}</Types>")));
                ct_added = true;
            }
            let had = text(pkg, DOC_RELS)?;
            let rels = had.clone().unwrap_or_else(|| EMPTY_RELS.to_owned());
            if !rels.contains("relationships/styles\"") {
                let mut n = 1u32;
                while rels.contains(&format!("Id=\"rId{n}\"")) {
                    n += 1;
                }
                let rel = format!(
                    "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>"
                );
                pkg.set_part_text(DOC_RELS, replace_first(&rels, "</Relationships>", &rel));
                rels_before = Some(had);
            }
            xml
        }
    };
    let add: String = styles.iter().filter(|(id, _)| !has_style_id(&xml, id)).map(|(_, x)| x.as_str()).collect();
    if add.is_empty() {
        if before.is_none() {
            // the TS returns before `setPartText`: the content type and the relationship stay without the part
            return Ok(Some(AddedStyles { before: None, rels_before, ct_added }));
        }
        return Ok(None);
    }
    // `xml.replace(/<\/w:styles>\s*$/, () => add + '</w:styles>')`
    let trimmed = uo_jsnum::text::trim_end(&xml);
    let next = match trimmed.strip_suffix("</w:styles>") {
        Some(head) => format!("{head}{add}</w:styles>"),
        None => xml.clone(),
    };
    pkg.set_part_text(STYLES_PATH, next);
    Ok(Some(AddedStyles { before, rels_before, ct_added }))
}

/// Takes back what an earlier [`add_styles_xml`] changed (Word: a style no step holds any more is not in the file).
pub(crate) fn take_back_styles(pkg: &mut Package, a: &AddedStyles) -> Result<(), DocxError> {
    match &a.before {
        Some(x) => pkg.set_part_text(STYLES_PATH, x.clone()),
        None => {
            pkg.remove_part(STYLES_PATH);
        }
    }
    if a.ct_added
        && let Some(ct) = text(pkg, CONTENT_TYPES)?
    {
        pkg.set_part_text(CONTENT_TYPES, replace_first(&ct, STYLES_OVERRIDE, ""));
    }
    match &a.rels_before {
        Some(Some(r)) => pkg.set_part_text(DOC_RELS, r.clone()),
        Some(None) => {
            pkg.remove_part(DOC_RELS);
        }
        None => {}
    }
    Ok(())
}

/// The names of the note styles a footnote or endnote insertion defines (`ensureNoteStyles`).
const NOTE_STYLE_NAMES: [&str; 4] = ["footnote text", "footnote reference", "endnote text", "endnote reference"];

/// The style sheet `store` written back after an undo, with the note styles (and the styles linked to them) the
/// package's earlier text `held` defines and `store` does not: Word keeps the styles a footnote insertion added when the insertion is undone (measured: note-plain
/// with one footnote, `Footnotes.Add`, save, undo, save → the same 7 styles in both copies, `s7c/w-out/note-plain-footnote-*`)
/// while it drops a created style and an undone table's Table Grid (`s-plan/w-out`).
pub(crate) fn keep_note_styles(held: &str, store: &str) -> String {
    put_missing_styles(store, &note_styles_of(held))
}

/// The styles of `defs` (id, `w:style` element) that `store` does not define, put into it (`put_style`, the rest as is).
pub(crate) fn put_missing_styles(store: &str, defs: &[(String, String)]) -> String {
    let mut out = store.to_owned();
    for (id, raw) in defs {
        if has_style_id(&out, id) {
            continue;
        }
        if let Some(next) = super::ops::styles::sheet::put_style(&out, id, raw) {
            out = next;
        }
    }
    out
}

/// Wave 7c (S, fix round 1): the note styles a note command defined (`sheet` after it, `before` its ids before), kept
/// by the session for the saves: Word keeps them in the file after the insertion is undone, whether or not a save came
/// in between (measured: note-plain, `Footnotes.Add`, `Undo`, `SaveAs2` with no save before →
/// Textodenotaderodap, TextodenotaderodapChar, Refdenotaderodap in the file, `s7d/w-out/w-in-note-plain-footnote-nosave-*`).
pub(crate) fn added_note_styles(sheet: &str, before: &str) -> Vec<(String, String)> {
    note_styles_of(sheet).into_iter().filter(|(id, _)| !has_style_id(before, id)).collect()
}

/// The note styles of a sheet and the styles linked to them (`Texto de nota de rodapé Char`): (id, `w:style` element).
fn note_styles_of(held: &str) -> Vec<(String, String)> {
    use super::ops::styles::sheet::read_style_index;
    let idx = read_style_index(held);
    // the note styles and the styles linked to them (`Texto de nota de rodapé Char`)
    let mut keep: Vec<&str> = idx
        .styles
        .iter()
        .filter(|s| NOTE_STYLE_NAMES.contains(&uo_text::case::to_lowercase_str(&s.name).as_str()))
        .map(|s| s.id.as_str())
        .collect();
    let linked: Vec<&str> = idx
        .styles
        .iter()
        .filter(|s| !keep.contains(&s.id.as_str()))
        .filter(|s| {
            s.link.as_deref().is_some_and(|l| keep.contains(&l))
                || idx.styles.iter().any(|k| keep.contains(&k.id.as_str()) && k.link.as_deref() == Some(s.id.as_str()))
        })
        .map(|s| s.id.as_str())
        .collect();
    keep.extend(linked);
    idx.styles
        .iter()
        .filter(|s| keep.contains(&s.id.as_str()))
        .filter_map(|s| held.get(s.start..s.end).map(|raw| (s.id.clone(), raw.to_owned())))
        .collect()
}

/// The numbering part back to the open's (Word: after the list is undone the saved file has the loaded part, or none).
pub(crate) fn numbering_back(pkg: &mut Package, path: Option<&str>, loaded: Option<&str>) -> Result<(), DocxError> {
    match (path, loaded) {
        (Some(p), Some(x)) => pkg.set_part_text(p.to_owned(), x.to_owned()),
        _ => {
            // no part at the open: what `write_numbering_part` added goes (the part, its relationship, its content type)
            pkg.remove_part("word/numbering.xml");
            if let Some(r) = text(pkg, DOC_RELS)? {
                pkg.set_part_text(DOC_RELS, replace_first(&r, NUMBERING_REL, ""));
            }
            if let Some(ct) = text(pkg, CONTENT_TYPES)? {
                pkg.set_part_text(CONTENT_TYPES, replace_first(&ct, NUMBERING_OVERRIDE, ""));
            }
        }
    }
    Ok(())
}

/// `settings.xml` back to the open's before a save patches it again (Word: an undone setting is not in the file).
pub(crate) fn settings_back(pkg: &mut Package, loaded: Option<&(String, Arc<str>)>) -> Result<(), DocxError> {
    match loaded {
        Some((path, x)) => {
            if text(pkg, path)?.as_deref() != Some(&**x) {
                pkg.set_part_text(path.clone(), x.to_string());
            }
        }
        None => {
            if pkg.contains(SETTINGS_PATH) {
                pkg.remove_part(SETTINGS_PATH);
                if let Some(r) = text(pkg, DOC_RELS)? {
                    pkg.set_part_text(DOC_RELS, replace_first(&r, SETTINGS_REL, ""));
                }
                if let Some(ct) = text(pkg, CONTENT_TYPES)? {
                    pkg.set_part_text(CONTENT_TYPES, replace_first(&ct, SETTINGS_OVERRIDE, ""));
                }
            }
        }
    }
    Ok(())
}
