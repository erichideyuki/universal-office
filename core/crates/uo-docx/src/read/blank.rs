//! Frontier R5 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `blank.ts`: a NEW document as
//! Word 365 pt-BR writes a blank one: styles (docDefaults, latent styles and those of Normal.dotm),
//! theme, settings and font table measured on the oracle (`data::word_styles`). The package is the
//! `DocxPackage.empty()` of the TS (content types, root relationships, a body-less `document.xml`) with
//! the parts replaced or added in the TS order.

use uo_opc::package::OpenOptions;
use uo_opc::{Limits, Package};

use crate::data::word_styles::{WORD_BLANK_FONT_TABLE, WORD_BLANK_SECTPR, WORD_BLANK_SETTINGS, WORD_BLANK_STYLES, WORD_BLANK_THEME};

const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// `BLANK_STYLES_XML`: the `styles.xml` of the blank document.
pub const BLANK_STYLES_XML: &str = WORD_BLANK_STYLES;

/// `BlankDocxOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlankDocxOptions<'a> {
    /// The language of the text (a BCP 47 tag, `es-ES`): the `w:lang` of the document defaults and the
    /// `w:themeFontLang`, as Word writes Normal in the editing language. Without it, or with an invalid
    /// tag, `pt-BR`.
    pub lang: Option<&'a str>,
}

/// `LANG_TAG = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/`: a language of 2–3 letters and alphanumeric
/// subtags of 2–8 characters. Every subtag is followed by `-` or the end, so the match is the split
/// at `-`; `$` is the end of the string only (no `m` flag).
fn matches_lang_tag(s: &str) -> bool {
    let mut parts = s.split('-');
    let Some(first) = parts.next() else { return false };
    (2..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// `isLanguageTag(value)`: a tag accepted in `w:lang`.
#[must_use]
pub fn is_language_tag(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && matches_lang_tag(v))
}

/// `DocxPackage.empty()` (content types, root relationships, a body-less `document.xml`): the package of a
/// document read from another format (`loadOdt`; wave 7, A1).
#[must_use]
pub fn empty_docx_package() -> Package {
    let mut pkg = Package::new_empty(&Limits::DEFAULT, &OpenOptions::default());
    pkg.set_part_text(
        "[Content_Types].xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>",
    );
    pkg.set_part_text(
        "_rels/.rels",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>",
    );
    pkg.set_part_text(
        "word/document.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>",
    );
    pkg
}

/// `blankDocxPackage(options)`: the package of a new blank document.
#[must_use]
pub fn blank_docx_package(options: &BlankDocxOptions<'_>) -> Package {
    let lang = options.lang.filter(|l| is_language_tag(Some(l))).unwrap_or("pt-BR");
    let mut pkg = Package::new_empty(&Limits::DEFAULT, &OpenOptions::default());
    // `DocxPackage.empty()`
    pkg.set_part_text(
        "[Content_Types].xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>",
    );
    pkg.set_part_text(
        "_rels/.rels",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>",
    );
    pkg.set_part_text(
        "word/document.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>",
    );
    // `blankDocxPackage`
    let ct = "application/vnd.openxmlformats-officedocument.";
    pkg.set_part_text(
        "[Content_Types].xml",
        format!(
            "{XML}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"{ct}wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"{ct}wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"{ct}wordprocessingml.settings+xml\"/>\
<Override PartName=\"/word/fontTable.xml\" ContentType=\"{ct}wordprocessingml.fontTable+xml\"/>\
<Override PartName=\"/word/theme/theme1.xml\" ContentType=\"{ct}theme+xml\"/>\
</Types>"
        ),
    );
    pkg.set_part_text(
        "word/_rels/document.xml.rels",
        format!(
            "{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>\
<Relationship Id=\"rId2\" Type=\"{REL}/settings\" Target=\"settings.xml\"/>\
<Relationship Id=\"rId4\" Type=\"{REL}/fontTable\" Target=\"fontTable.xml\"/>\
<Relationship Id=\"rId5\" Type=\"{REL}/theme\" Target=\"theme/theme1.xml\"/>\
</Relationships>"
        ),
    );
    pkg.set_part_text(
        "word/document.xml",
        format!("{XML}<w:document xmlns:w=\"{W}\" xmlns:r=\"{REL}\"><w:body><w:p/>{WORD_BLANK_SECTPR}</w:body></w:document>"),
    );
    pkg.set_part_text("word/styles.xml", WORD_BLANK_STYLES.replacen("<w:lang w:val=\"pt-BR\"", &format!("<w:lang w:val=\"{lang}\""), 1));
    pkg.set_part_text(
        "word/settings.xml",
        WORD_BLANK_SETTINGS.replacen("<w:themeFontLang w:val=\"pt-BR\"", &format!("<w:themeFontLang w:val=\"{lang}\""), 1),
    );
    pkg.set_part_text("word/fontTable.xml", WORD_BLANK_FONT_TABLE);
    pkg.set_part_text("word/theme/theme1.xml", WORD_BLANK_THEME);
    pkg
}
