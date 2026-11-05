//! C# `DocxText`: find, replace and extract text, per paragraph (as Word does); a replacement
//! takes the formatting of the first unit found. The `Regex` overloads are not ported (U13);
//! the one regex the API uses itself (`#\{[A-Za-z0-9]+\}`) is a hand matcher.

use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::{ApiError, Result};
use super::paragraph_text::{self as pt, ParagraphText};
use super::structure::is_block;

/// Where to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextScope {
    /// body, headers, footers and notes (DocIO's Replace)
    #[default]
    Document,
    /// the main part only (tables and text boxes included)
    Body,
}

/// The template parameter form `#{Name}`.
pub fn parameter(name: &str) -> String {
    format!("#{{{name}}}")
}

fn roots(doc: &mut DocxDocument, scope: TextScope) -> Result<Vec<NodeId>> {
    Ok(match scope {
        TextScope::Body => vec![doc.main_root()?],
        TextScope::Document => doc.stories(true, true)?.into_iter().map(|(_, r)| r).collect(),
    })
}

/// Replaces every occurrence of `find` (`None` replacement = empty); returns how many.
pub fn replace(
    doc: &mut DocxDocument,
    find: &str,
    replacement: Option<&str>,
    case_sensitive: bool,
    whole_word: bool,
    scope: TextScope,
) -> Result<usize> {
    if find.is_empty() {
        return Err(ApiError::argument("The value cannot be an empty string. (Parameter 'find')"));
    }
    let mut count = 0;
    for root in roots(doc, scope)? {
        for p in pt::paragraphs(&doc.dom, root) {
            count += replace_in_paragraph(&mut doc.dom, p, find, replacement.unwrap_or(""), case_sensitive, whole_word)?;
        }
    }
    Ok(count)
}

/// Replaces the parameter `#{name}` (case-insensitive, whole word).
pub fn replace_parameter(doc: &mut DocxDocument, name: &str, value: Option<&str>) -> Result<usize> {
    replace(doc, &parameter(name), value, false, true, TextScope::Document)
}

/// Replaces several parameters, in order.
pub fn replace_parameters(doc: &mut DocxDocument, parameters: &[(&str, Option<&str>)]) -> Result<usize> {
    let mut total = 0;
    for (k, v) in parameters {
        total += replace_parameter(doc, k, *v)?;
    }
    Ok(total)
}

pub(crate) fn replace_in_paragraph(
    dom: &mut Dom,
    p: NodeId,
    find: &str,
    replacement: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<usize> {
    let text = ParagraphText::build(dom, p);
    let matches = text.find(find, case_sensitive, whole_word);
    for &(s, l) in matches.iter().rev() {
        text.replace(dom, s, l, replacement)?;
    }
    Ok(matches.len())
}

/// `Replace(paragraph, AnyParameter, replacement)`.
pub(crate) fn replace_any_parameter(dom: &mut Dom, p: NodeId, replacement: &str) -> Result<usize> {
    let text = ParagraphText::build(dom, p);
    let matches = text.find_any_parameter();
    for &(s, l) in matches.iter().rev() {
        text.replace(dom, s, l, replacement)?;
    }
    Ok(matches.len())
}

/// Does the text occur in some paragraph?
pub fn contains(doc: &mut DocxDocument, find: &str, case_sensitive: bool, whole_word: bool, scope: TextScope) -> Result<bool> {
    for root in roots(doc, scope)? {
        for p in pt::paragraphs(&doc.dom, root) {
            if !ParagraphText::build(&doc.dom, p).find(find, case_sensitive, whole_word).is_empty() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The paragraphs containing the text, in document order (body first).
pub(crate) fn find_paragraphs(
    doc: &mut DocxDocument,
    find: &str,
    case_sensitive: bool,
    whole_word: bool,
    scope: TextScope,
) -> Result<Vec<NodeId>> {
    let mut out = Vec::new();
    for root in roots(doc, scope)? {
        for p in pt::paragraphs(&doc.dom, root) {
            if !ParagraphText::build(&doc.dom, p).find(find, case_sensitive, whole_word).is_empty() {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// Removes the paragraphs containing any of the texts (the only block of a cell or text box
/// is emptied instead); returns how many.
pub fn remove_paragraphs_containing(
    doc: &mut DocxDocument,
    texts: &[&str],
    case_sensitive: bool,
    whole_word: bool,
    scope: TextScope,
) -> Result<usize> {
    let mut count = 0;
    for root in roots(doc, scope)? {
        for p in pt::paragraphs(&doc.dom, root) {
            if doc.dom.parent(p).is_none() {
                continue;
            }
            let text = ParagraphText::build(&doc.dom, p);
            if !texts.iter().any(|t| !text.find(t, case_sensitive, whole_word).is_empty()) {
                continue;
            }
            remove_paragraph(&mut doc.dom, p)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Removes the paragraph; the only block of its container keeps an empty paragraph with the
/// same properties.
pub(crate) fn remove_paragraph(dom: &mut Dom, p: NodeId) -> Result<()> {
    if let Some(parent) = dom.parent(p)
        && !dom.is(parent, n::body)
        && !dom.elements_before(p).into_iter().any(|e| is_block(dom, e))
        && !dom.elements_after(p).into_iter().any(|e| is_block(dom, e))
    {
        let doomed: Vec<NodeId> = dom.elements(p).filter(|&e| !dom.is(e, n::pPr)).collect();
        for e in doomed {
            dom.remove_if_attached(e);
        }
        return Ok(());
    }
    dom.remove(p)
}

/// Plain text: one paragraph per line (CRLF), tab `\t`, line break `\n`; tables cell by cell;
/// text boxes after their paragraph; plugin inlines by their fallback text; field codes and
/// deleted text left out.
pub fn get_plain_text(doc: &mut DocxDocument, include_headers_and_footers: bool) -> Result<String> {
    let mut out = String::new();
    for (_, root) in doc.stories(include_headers_and_footers, false)? {
        let dom = &doc.dom;
        for p in pt::paragraphs(dom, root) {
            if dom.ancestors(p).into_iter().any(|a| dom.is(a, n::mc_Fallback) || dom.is(a, n::del)) {
                continue;
            }
            if !out.is_empty() {
                out.push_str("\r\n");
            }
            out.push_str(&pt::plain_text(dom, p));
        }
    }
    Ok(out)
}
