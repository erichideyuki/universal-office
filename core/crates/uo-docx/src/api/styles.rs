//! C# `DocxStyles`: style queries and creation (`word/styles.xml`).

use super::document::DocxDocument;
use super::dom::{Dom, Name, NodeId, n};
use super::error::{ApiError, Result};
use super::net;
use super::ooxml::content_types;

/// The `w:styles` root (created empty when `create`).
pub(crate) fn root(doc: &mut DocxDocument, create: bool) -> Result<Option<NodeId>> {
    doc.main_related_root("styles", create, "word/styles.xml", content_types::STYLES, n::styles)
}

pub(crate) fn all(dom: &Dom, root: Option<NodeId>) -> Vec<NodeId> {
    root.map(|r| dom.elements_named(r, n::style)).unwrap_or_default()
}

pub(crate) fn id(dom: &Dom, style: NodeId) -> Option<String> {
    dom.attr(style, n::styleId).map(str::to_owned)
}

pub(crate) fn name(dom: &Dom, style: NodeId) -> Option<String> {
    dom.element(style, n::name).and_then(|e| dom.attr(e, n::val)).map(str::to_owned)
}

pub(crate) fn type_of(dom: &Dom, style: NodeId) -> String {
    dom.attr(style, n::type_).unwrap_or("paragraph").to_owned()
}

pub(crate) fn by_id(dom: &Dom, root: Option<NodeId>, sid: &str) -> Option<NodeId> {
    all(dom, root).into_iter().find(|&s| dom.attr(s, n::styleId) == Some(sid))
}

/// The style by NAME (OrdinalIgnoreCase; built-in names are the English ones).
pub(crate) fn by_name(dom: &Dom, root: Option<NodeId>, nm: &str, ty: Option<&str>) -> Option<NodeId> {
    all(dom, root)
        .into_iter()
        .find(|&s| name(dom, s).is_some_and(|x| net::eq_ignore_case(&x, nm)) && ty.is_none_or(|t| type_of(dom, s) == t))
}

pub(crate) fn is_on(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true" | "on"))
}

/// The default paragraph style id (`w:default="1"`).
pub(crate) fn default_paragraph_style_id(dom: &Dom, root: Option<NodeId>) -> Option<String> {
    all(dom, root)
        .into_iter()
        .filter(|&s| type_of(dom, s) == "paragraph" && is_on(dom.attr(s, n::default)))
        .map(|s| id(dom, s))
        .next()
        .flatten()
}

/// The display name of the paragraph's effective style.
pub fn paragraph_style_name(doc: &mut DocxDocument, paragraph: NodeId) -> Result<Option<String>> {
    doc.dom().check(paragraph)?;
    let dom = doc.dom();
    let direct =
        dom.element(paragraph, n::pPr).and_then(|p| dom.element(p, n::pStyle)).and_then(|s| dom.attr(s, n::val)).map(str::to_owned);
    paragraph_style_name_of_id(doc, direct.as_deref())
}

/// `paragraph_style_name` for a paragraph whose direct style id is `direct` (`None`: no `w:pStyle`), without a node:
/// the C ABI asks the effective name of an id and must not allocate a paragraph in the arena per call.
pub fn paragraph_style_name_of_id(doc: &mut DocxDocument, direct: Option<&str>) -> Result<Option<String>> {
    let r = root(doc, false)?;
    let dom = doc.dom();
    let Some(sid) = direct.map(str::to_owned).or_else(|| default_paragraph_style_id(dom, r)) else { return Ok(None) };
    Ok(Some(match by_id(dom, r, &sid) {
        None => sid,
        Some(s) => name(dom, s).unwrap_or(sid),
    }))
}

const BUILT_IN: &[(&str, &str)] = &[
    ("Normal", "Normal"),
    ("Title", "Title"),
    ("Subtitle", "Subtitle"),
    ("ListParagraph", "List Paragraph"),
    ("Quote", "Quote"),
    ("IntenseQuote", "Intense Quote"),
    ("Caption", "caption"),
    ("NoSpacing", "No Spacing"),
    ("Hyperlink", "Hyperlink"),
    ("FootnoteText", "footnote text"),
    ("FootnoteReference", "footnote reference"),
    ("Header", "header"),
    ("Footer", "footer"),
    ("TableGrid", "Table Grid"),
    ("Strong", "Strong"),
    ("Emphasis", "Emphasis"),
];

/// The built-in (English) name of a Word built-in style id (`Heading1` → "heading 1", `TOC2` → "toc 2"…).
pub fn built_in_name(style_id: &str) -> Option<String> {
    if let Some((_, v)) = BUILT_IN.iter().find(|(k, _)| net::eq_ignore_case(k, style_id)) {
        return Some((*v).to_owned());
    }
    let u = net::units(style_id);
    for (prefix, display) in [("Heading", "heading "), ("TOC", "toc ")] {
        let last = u.last().copied().unwrap_or(0);
        if u.len() == prefix.len() + 1 && net::starts_with_ignore_case(style_id, prefix) && (0x30..=0x39).contains(&last) {
            return Some(format!("{display}{}", char::from(u8::try_from(last).unwrap_or(b'0'))));
        }
    }
    None
}

/// The id of the "Hyperlink" character style; created (theme blue, single underline) when missing.
pub(crate) fn ensure_hyperlink_character_style(doc: &mut DocxDocument) -> Result<String> {
    let r = root(doc, true)?.ok_or_else(|| ApiError::invalid_operation("no styles part"))?;
    let dom = doc.dom_mut();
    if let Some(existing) = by_name(dom, Some(r), "Hyperlink", Some("character")) {
        return id(dom, existing).ok_or_else(|| ApiError::invalid_operation("the Hyperlink style has no id"));
    }
    let sid = unique_id(dom, r, "Hyperlink");
    let style = dom.new_element_with(n::style, &[(n::type_, "character"), (n::styleId, &sid)]);
    let nm = dom.new_element_with(n::name, &[(n::val, "Hyperlink")]);
    dom.append(style, nm);
    let default_font = all(dom, Some(r)).into_iter().find(|&s| type_of(dom, s) == "character" && is_on(dom.attr(s, n::default)));
    if let Some(df) = default_font {
        let based = id(dom, df).unwrap_or_default();
        let b = dom.new_element_with(n::basedOn, &[(n::val, &based)]);
        dom.append(style, b);
    }
    let ui = dom.new_element_with(n::uiPriority, &[(n::val, "99")]);
    let unhide = dom.new_element(n::unhideWhenUsed);
    let color = dom.new_element_with(n::color, &[(n::val, "0563C1"), (n::themeColor, "hyperlink")]);
    let u = dom.new_element_with(n::u, &[(n::val, "single")]);
    let rpr = dom.new_element(n::rPr);
    dom.append_all(rpr, &[color, u]);
    dom.append_all(style, &[ui, unhide, rpr]);
    dom.append(r, style);
    Ok(sid)
}

/// A free id based on `desired` (`Desired`, `Desired1`, `Desired2`… OrdinalIgnoreCase).
pub(crate) fn unique_id(dom: &Dom, root: NodeId, desired: &str) -> String {
    let ids: std::collections::HashSet<String> =
        all(dom, Some(root)).into_iter().map(|s| net::fold_key(dom.attr(s, n::styleId).unwrap_or(""))).collect();
    if !ids.contains(&net::fold_key(desired)) {
        return desired.to_owned();
    }
    let mut i: u64 = 1;
    loop {
        let c = format!("{desired}{i}");
        if !ids.contains(&net::fold_key(&c)) {
            return c;
        }
        i += 1;
    }
}

/// A paragraph property through the style chain (direct → style → basedOn…).
pub(crate) fn resolve_ppr_child(doc: &mut DocxDocument, paragraph: NodeId, child: Name) -> Result<Option<NodeId>> {
    let dom = doc.dom();
    if let Some(d) = dom.element(paragraph, n::pPr).and_then(|p| dom.element(p, child)) {
        return Ok(Some(d));
    }
    let r = root(doc, false)?;
    let dom = doc.dom();
    let mut sid = dom
        .element(paragraph, n::pPr)
        .and_then(|p| dom.element(p, n::pStyle))
        .and_then(|s| dom.attr(s, n::val))
        .map(str::to_owned)
        .or_else(|| default_paragraph_style_id(dom, r));
    let mut seen = std::collections::HashSet::new();
    while let Some(s) = sid {
        if !seen.insert(s.clone()) {
            break;
        }
        let Some(style) = by_id(dom, r, &s) else { break };
        if let Some(found) = dom.element(style, n::pPr).and_then(|p| dom.element(p, child)) {
            return Ok(Some(found));
        }
        sid = dom.element(style, n::basedOn).and_then(|b| dom.attr(b, n::val)).map(str::to_owned);
    }
    Ok(None)
}
