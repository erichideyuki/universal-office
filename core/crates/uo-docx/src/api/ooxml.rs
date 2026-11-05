//! OOXML vocabulary and XML helpers of the Editing API (C# `Ooxml.cs`: `RelTypes`,
//! `ContentTypes`; `XmlHelpers.cs`: `XmlText`, `XmlOrder`).

use super::dom::{Dom, Name, NodeId, n};
use super::net;

/// Relationship types (compared by the last segment, stable across variants).
pub mod rel_types {
    pub const OFFICE_DOCUMENT_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
    pub const OFFICE_DOCUMENT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    pub const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    pub const HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    pub const CUSTOM_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
    pub const EXTENDED_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
    pub const CORE_PROPERTIES: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

    /// The type ends with `/` + suffix (OrdinalIgnoreCase).
    pub fn is(ty: &str, suffix: &str) -> bool {
        super::net::ends_with_ignore_case(ty, &format!("/{suffix}"))
    }

    /// The last segment of a type.
    pub fn last(ty: &str) -> &str {
        ty.rfind('/').map_or(ty, |k| ty.get(k + 1..).unwrap_or(""))
    }

    /// Types whose target only exists while an attribute of the part names the id (the others
    /// are implicit and never collected), OrdinalIgnoreCase.
    pub const REFERENCED: &[&str] = &[
        "image",
        "hyperlink",
        "oleObject",
        "package",
        "control",
        "aFChunk",
        "header",
        "footer",
        "subDocument",
        "chart",
        "diagramData",
        "diagramLayout",
        "diagramQuickStyle",
        "diagramColors",
        "diagramDrawing",
        "video",
        "audio",
        "media",
        "hdphoto",
        "font",
        "activeXControlBinary",
        "frame",
        "attachedTemplate",
        "mailMergeSource",
        "mailMergeHeaderSource",
        "recipientData",
        "slide",
    ];

    pub fn is_referenced(last: &str) -> bool {
        REFERENCED.iter().any(|r| super::net::eq_ignore_case(r, last))
    }
}

/// Content types of the parts the API creates.
pub mod content_types {
    pub const MAIN_DOCUMENT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
    pub const STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
    pub const NUMBERING: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
    pub const FOOTNOTES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
    pub const ENDNOTES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
    pub const FONT_TABLE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";
    pub const CORE_PROPERTIES: &str = "application/vnd.openxmlformats-package.core-properties+xml";
    pub const EXTENDED_PROPERTIES: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
    pub const CUSTOM_PROPERTIES: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
}

// ─────────────────────────────── XmlText ───────────────────────────────

/// C# `XmlText.RemoveInvalidChars`: drops what XML 1.0 refuses (the writer would throw).
pub fn remove_invalid_chars(s: &str) -> String {
    s.chars()
        .filter(|&c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'))
        .collect()
}

/// `w:t xml:space="preserve"` with the text.
pub fn t(dom: &mut Dom, text: &str) -> NodeId {
    let e = dom.new_element_with(n::t, &[(n::xml_space, "preserve")]);
    dom.append_text(e, text);
    e
}

/// The run content of a text: TAB → `w:tab`, line breaks (CRLF, CR, LF) → `w:br` (after
/// removing invalid characters, so a vertical tab never reaches the split).
pub fn run_content(dom: &mut Dom, text: &str) -> Vec<NodeId> {
    let text = remove_invalid_chars(text);
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if matches!(c, '\t' | '\n' | '\r' | '\u{B}') {
            if !buf.is_empty() {
                out.push(t(dom, &buf));
                buf.clear();
            }
            if c == '\t' {
                out.push(dom.new_element(n::tab));
            } else {
                if c == '\r' && it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(dom.new_element(n::br));
            }
            continue;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        out.push(t(dom, &buf));
    }
    out
}

/// A run with a copy of the properties and the text.
pub fn run(dom: &mut Dom, rpr: Option<NodeId>, text: &str) -> NodeId {
    let r = dom.new_element(n::r);
    if let Some(p) = rpr {
        let c = dom.deep_clone(p);
        dom.append(r, c);
    }
    let content = run_content(dom, text);
    dom.append_all(r, &content);
    r
}

// ─────────────────────────────── XmlOrder ───────────────────────────────

macro_rules! order {
    ($name:ident: $($local:ident),* $(,)?) => {
        pub const $name: &[Name] = &[$(n::$local),*];
    };
}

/// The child order the schema requires (C# `XmlOrder`), as names of the vocabulary.
pub mod order {
    use super::super::dom::{Name, n};
    order!(PPR: pStyle, keepNext, keepLines, pageBreakBefore, framePr, widowControl, numPr, suppressLineNumbers, pBdr, shd, tabs,
        suppressAutoHyphens, kinsoku, wordWrap, overflowPunct, topLinePunct, autoSpaceDE, autoSpaceDN, bidi, adjustRightInd, snapToGrid,
        spacing, ind, contextualSpacing, mirrorIndents, suppressOverlap, jc, textDirection, textAlignment, textboxTightWrap, outlineLvl,
        divId, cnfStyle, rPr, sectPr, pPrChange);
    order!(RPR: rStyle, rFonts, b, bCs, i, iCs, caps, smallCaps, strike, dstrike, outline, shadow, emboss, imprint, noProof, snapToGrid,
        vanish, webHidden, color, spacing, w, kern, position, sz, szCs, highlight, u, effect, bdr, shd, fitText, vertAlign, rtl, w_cs, em,
        lang, eastAsianLayout, specVanish, oMath);
    order!(NUMPR: ilvl, numId, numberingChange, ins);
    order!(SECTPR: headerReference, footerReference, footnotePr, endnotePr, type_, pgSz, pgMar, paperSrc, pgBorders, lnNumType, pgNumType,
        cols, formProt, vAlign, noEndnote, titlePg, textDirection, bidi, rtlGutter, docGrid, printerSettings, sectPrChange);
    order!(SETTINGS: writeProtection, view, zoom, removePersonalInformation, removeDateAndTime, doNotDisplayPageBoundaries,
        displayBackgroundShape, printPostScriptOverText, printFractionalCharacterWidth, printFormsData, embedTrueTypeFonts,
        embedSystemFonts, saveSubsetFonts, saveFormsData, mirrorMargins, alignBordersAndEdges, bordersDoNotSurroundHeader,
        bordersDoNotSurroundFooter, gutterAtTop, hideSpellingErrors, hideGrammaticalErrors, activeWritingStyle, proofState, formsDesign,
        attachedTemplate, linkStyles, stylePaneFormatFilter, stylePaneSortMethod, documentType, mailMerge, revisionView, trackRevisions,
        doNotTrackMoves, doNotTrackFormatting, documentProtection);
}

/// C# `XmlOrder.Set`: replaces the first child of the same name, or inserts before the first
/// child that comes later in `order` (unknown names stay where they are), else appends.
/// Returns the inserted node (a copy when `child` had a container, as XLinq inserts).
pub fn set(dom: &mut Dom, parent: NodeId, child: NodeId, order: &[Name]) -> NodeId {
    let Some(name) = dom.name_of(child) else { return child };
    if let Some(existing) = dom.element(parent, name) {
        return dom.replace_with(existing, &[child]).ok().and_then(|v| v.first().copied()).unwrap_or(child);
    }
    if let Some(rank) = order.iter().position(|&o| o == name) {
        let later =
            dom.elements(parent).find(|&s| dom.name_of(s).and_then(|sn| order.iter().position(|&o| o == sn)).is_some_and(|r| r > rank));
        if let Some(s) = later {
            return dom.add_before(s, &[child]).ok().and_then(|v| v.first().copied()).unwrap_or(child);
        }
    }
    dom.append(parent, child)
}

/// C# `XmlOrder.Ensure`: the child, created in schema position when missing.
pub fn ensure(dom: &mut Dom, parent: NodeId, name: Name, order: &[Name]) -> NodeId {
    if let Some(e) = dom.element(parent, name) {
        return e;
    }
    let c = dom.new_element(name);
    set(dom, parent, c, order)
}

/// `w:pPr` of the paragraph, created as the first child when missing.
pub fn ensure_ppr(dom: &mut Dom, p: NodeId) -> NodeId {
    if let Some(e) = dom.element(p, n::pPr) {
        return e;
    }
    let e = dom.new_element(n::pPr);
    dom.prepend(p, &[e]);
    e
}

/// `w:rPr` of the run, created as the first child when missing.
pub fn ensure_rpr(dom: &mut Dom, r: NodeId) -> NodeId {
    if let Some(e) = dom.element(r, n::rPr) {
        return e;
    }
    let e = dom.new_element(n::rPr);
    dom.prepend(r, &[e]);
    e
}

/// `Array.IndexOf`-style position of a name in an order list (tests).
pub fn rank(order: &[Name], name: Name) -> Option<usize> {
    order.iter().position(|&o| o == name)
}

/// `RelTypes.Is` over the vocabulary (alias kept for the callers' readability).
pub fn rel_is(ty: &str, suffix: &str) -> bool {
    rel_types::is(ty, suffix)
}

/// `string.Equals(a, b, OrdinalIgnoreCase)` (re-exported for the modules of the API).
pub fn eq_ic(a: &str, b: &str) -> bool {
    net::eq_ignore_case(a, b)
}
