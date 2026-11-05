//! C# `DocxSanitizer` / `DocxSanitizeOptions`: the cleaning of a .docx before storing received
//! content or delivering a generated document — the security boundary of the host (record
//! `uo-docx-d3.md`, "Security review of the sanitizer").
//!
//! Steps, in this order: revisions → metadata → headers/footers → sections → unsafe content →
//! fields → hyperlink fields → plugin inlines (two-phase) → customXml.

use super::custom_inlines::{self, CustomInline};
use super::document::DocxDocument;
use super::dom::{Dom, NodeId, n};
use super::error::Result;
use super::fields::{self, DEFAULT_ALLOWED_FIELDS, DEFAULT_ALLOWED_SCHEMES, DEFAULT_OUTPUT_ALLOWED_FIELDS};
use super::net;
use super::ooxml::rel_types;
use super::paragraph_text::remove_if_empty;
use super::review;

/// What the sanitization does (C# `DocxSanitizeOptions`; use [`SanitizeOptions::content`] and
/// [`SanitizeOptions::output`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeOptions {
    pub accept_revisions: bool,
    pub remove_comments: bool,
    pub remove_macros: bool,
    pub remove_protection_and_attached_template: bool,
    pub remove_personal_metadata: bool,
    pub remove_headers_and_footers: bool,
    pub merge_sections: bool,
    pub remove_custom_xml: bool,
    pub remove_unsafe_content: bool,
    /// fields kept; the others become their cached result. `None` = fields untouched.
    pub allowed_fields: Option<Vec<String>>,
    pub allowed_hyperlink_schemes: Vec<String>,
    pub convert_hyperlink_fields: bool,
    /// plugin inlines are judged by the host (C# `IsCustomInlineValid` not null): the two-phase
    /// [`sanitize_begin`] lists them
    pub validate_custom_inlines: bool,
}

impl Default for SanitizeOptions {
    fn default() -> Self {
        SanitizeOptions {
            accept_revisions: true,
            remove_comments: true,
            remove_macros: true,
            remove_protection_and_attached_template: true,
            remove_personal_metadata: false,
            remove_headers_and_footers: false,
            merge_sections: false,
            remove_custom_xml: false,
            remove_unsafe_content: true,
            allowed_fields: Some(DEFAULT_ALLOWED_FIELDS.iter().map(|s| (*s).to_owned()).collect()),
            allowed_hyperlink_schemes: DEFAULT_ALLOWED_SCHEMES.iter().map(|s| (*s).to_owned()).collect(),
            convert_hyperlink_fields: true,
            validate_custom_inlines: false,
        }
    }
}

impl SanitizeOptions {
    /// Received content (uploaded, to store or apply to a template): revision, metadata,
    /// headers/footers, sections, unsafe content, fields and customXml.
    pub fn content() -> Self {
        SanitizeOptions {
            remove_personal_metadata: true,
            remove_headers_and_footers: true,
            merge_sections: true,
            remove_custom_xml: true,
            ..SanitizeOptions::default()
        }
    }

    /// A document generated from a template: revision, unsafe content and fields (the wider
    /// output list); headers, sections and metadata stay.
    pub fn output() -> Self {
        SanitizeOptions {
            allowed_fields: Some(DEFAULT_OUTPUT_ALLOWED_FIELDS.iter().map(|s| (*s).to_owned()).collect()),
            ..SanitizeOptions::default()
        }
    }
}

/// Between the two phases of a sanitization with host validation: the plugin inlines to judge.
#[derive(Debug, Clone)]
#[must_use = "finish the sanitization with the host's verdicts"]
pub struct SanitizeStage {
    inlines: Vec<CustomInline>,
    remove_custom_xml: bool,
}

impl SanitizeStage {
    /// The inlines to judge, in document order: `(type, json)` of each.
    pub fn inlines(&self) -> &[CustomInline] {
        &self.inlines
    }

    /// Applies the verdicts (one per inline: false unwraps it) and the remaining steps.
    pub fn finish(self, doc: &mut DocxDocument, verdicts: &[bool]) -> Result<usize> {
        let unwrapped = custom_inlines::unwrap_where(doc, &self.inlines, verdicts)?;
        if self.remove_custom_xml {
            remove_custom_xml(doc)?;
        }
        doc.mark_changed();
        Ok(unwrapped)
    }
}

/// Runs the steps up to the plugin-inline validation and lists the inlines to judge (empty when
/// the options do not validate).
pub fn sanitize_begin(doc: &mut DocxDocument, options: &SanitizeOptions) -> Result<SanitizeStage> {
    if options.accept_revisions {
        review::accept_all_revisions(doc)?;
        review::disable_track_changes(doc)?;
    }
    if options.remove_comments {
        review::remove_comments(doc)?;
    }
    if options.remove_macros {
        review::remove_macros(doc)?;
    }
    if options.remove_protection_and_attached_template {
        review::remove_protection(doc)?;
        review::remove_attached_template(doc)?;
    }
    if options.remove_personal_metadata {
        doc.properties().remove_personal_metadata()?;
    }
    if options.remove_headers_and_footers {
        remove_headers_and_footers(doc)?;
    }
    if options.merge_sections {
        merge_sections(doc)?;
    }
    let schemes: Vec<&str> = options.allowed_hyperlink_schemes.iter().map(String::as_str).collect();
    if options.remove_unsafe_content {
        remove_unsafe_content(doc, Some(&schemes))?;
    }
    if let Some(allowed) = &options.allowed_fields {
        let a: Vec<&str> = allowed.iter().map(String::as_str).collect();
        fields::replace_disallowed_with_result(doc, Some(&a), Some(&schemes))?;
    }
    if options.convert_hyperlink_fields {
        fields::convert_hyperlink_fields(doc, Some(&schemes))?;
    }
    let inlines = if options.validate_custom_inlines { custom_inlines::find(doc, None)? } else { Vec::new() };
    Ok(SanitizeStage { inlines, remove_custom_xml: options.remove_custom_xml })
}

/// The one-shot sanitization (every plugin inline judged valid when the options ask for
/// validation: use [`sanitize_begin`] to judge them).
pub fn sanitize(doc: &mut DocxDocument, options: &SanitizeOptions) -> Result<()> {
    let stage = sanitize_begin(doc, options)?;
    let verdicts = vec![true; stage.inlines.len()];
    stage.finish(doc, &verdicts)?;
    Ok(())
}

/// Removes the headers and footers of every section (references; the parts go at save).
pub fn remove_headers_and_footers(doc: &mut DocxDocument) -> Result<()> {
    let root = doc.main_root()?;
    let dom = doc.dom_mut();
    for sect in dom.descendants_named(root, n::sectPr) {
        let refs: Vec<NodeId> = dom.elements(sect).filter(|&e| dom.is_any(e, &[n::headerReference, n::footerReference])).collect();
        for r in refs {
            dom.remove_if_attached(r);
        }
    }
    doc.mark_changed();
    Ok(())
}

/// Merges every section into the last one: the section breaks (`w:sectPr` in paragraphs) leave.
pub fn merge_sections(doc: &mut DocxDocument) -> Result<()> {
    let body = doc.body()?;
    let dom = doc.dom_mut();
    let breaks: Vec<NodeId> =
        dom.descendants_named(body, n::sectPr).into_iter().filter(|&s| dom.parent(s).is_some_and(|p| dom.is(p, n::pPr))).collect();
    for s in breaks {
        dom.remove(s)?;
    }
    doc.mark_changed();
    Ok(())
}

/// Removes the custom XML data (`customXml/`) and the relationships to it.
pub fn remove_custom_xml(doc: &mut DocxDocument) -> Result<()> {
    for part in doc.part_names() {
        if net::starts_with_ignore_case(&part, "customXml/") && !net::ends_with_ignore_case(&part, ".rels") {
            doc.delete_part(&part)?;
        }
    }
    for part in doc.part_names().into_iter().filter(|p| net::ends_with_ignore_case(p, ".rels")) {
        let Some(d) = doc.try_xml(&part)? else { continue };
        let dom = doc.dom_mut();
        let Some(root) = dom.root(d) else { continue };
        let doomed: Vec<NodeId> = dom
            .elements_named(root, n::Relationship)
            .into_iter()
            .filter(|&r| {
                rel_types::is(dom.attr(r, n::Type).unwrap_or(""), "customXml")
                    || net::contains_ignore_case(dom.attr(r, n::Target).unwrap_or(""), "customXml/")
            })
            .collect();
        for r in doomed {
            dom.remove_if_attached(r);
        }
    }
    doc.mark_changed();
    Ok(())
}

const UNSAFE_RELATIONSHIP_TYPES: &[&str] = &[
    "oleObject",
    "package",
    "aFChunk",
    "control",
    "subDocument",
    "attachedTemplate",
    "frame",
    "vbaProject",
    "activeXControlBinary",
    "activeXControl",
];

fn is_unsafe_element(dom: &Dom, e: NodeId) -> bool {
    dom.is_any(e, &[n::object, n::control, n::altChunk, n::subDoc])
        || (dom.is(e, n::o_OLEObject) && dom.parent(e).is_some_and(|p| dom.is(p, n::pict)))
}

/// Removes OLE objects, ActiveX controls, altChunk, subdocuments, frames and their
/// relationships; external relationships stay only as hyperlinks with an allowed scheme (a
/// forbidden hyperlink becomes text, other external references leave).
pub fn remove_unsafe_content(doc: &mut DocxDocument, allowed_schemes: Option<&[&str]>) -> Result<()> {
    let schemes: Vec<&str> = allowed_schemes.unwrap_or(DEFAULT_ALLOWED_SCHEMES).to_vec();
    for (_, root) in doc.stories(true, true)? {
        let dom = doc.dom_mut();
        let found: Vec<NodeId> = dom.descendants(root).into_iter().filter(|&e| is_unsafe_element(dom, e)).collect();
        for e in found {
            let Some(run) = dom.parent(e) else { continue };
            let target = if dom.ns_of(e) == super::dom::NS_O || (dom.is(e, n::control) && dom.is(run, n::pict)) { run } else { e };
            let container = dom.parent(target);
            dom.remove(target)?;
            if let Some(c) = container {
                remove_if_empty(dom, c);
            }
        }
    }
    if let Some(ws) = doc.main_related_part("webSettings")? {
        let d = doc.xml(&ws)?;
        let dom = doc.dom_mut();
        if let Some(root) = dom.root(d) {
            let framesets: Vec<NodeId> = dom.elements_named(root, n::frameset);
            for f in framesets {
                dom.remove_if_attached(f);
            }
        }
    }
    for part in doc.part_names() {
        if net::ends_with_ignore_case(&part, ".rels") || part == "[Content_Types].xml" {
            continue;
        }
        let rels = doc.get_relationships(&part)?;
        let mut removed: Vec<String> = Vec::new();
        for rel in rels {
            let unsafe_type = UNSAFE_RELATIONSHIP_TYPES.iter().any(|t| net::eq_ignore_case(t, rel_types::last(&rel.rel_type)));
            let external_not_allowed =
                rel.external && !(rel_types::is(&rel.rel_type, "hyperlink") && fields::is_allowed_url(&rel.target, &schemes));
            if !unsafe_type && !external_not_allowed {
                continue;
            }
            doc.remove_relationship(&part, &rel.id)?;
            if !removed.contains(&rel.id) {
                removed.push(rel.id);
            }
        }
        if !removed.is_empty() {
            drop_references(doc, &part, &removed)?;
        }
    }
    doc.mark_changed();
    Ok(())
}

/// Takes out of the part's XML what pointed to removed relationships.
fn drop_references(doc: &mut DocxDocument, part: &str, removed: &[String]) -> Result<()> {
    let d = match doc.xml(part) {
        Ok(d) => d,
        Err(super::error::ApiError { kind: super::error::ErrorKind::Xml, .. }) => return Ok(()),
        Err(e) => return Err(e),
    };
    let dom = doc.dom_mut();
    let Some(root) = dom.root(d) else { return Ok(()) };
    let is_removed = |dom: &Dom, e: NodeId| removed.iter().any(|r| r == dom.attr(e, n::r_id).unwrap_or(""));
    let hs: Vec<NodeId> = dom.descendants_named(root, n::hyperlink).into_iter().filter(|&h| is_removed(dom, h)).collect();
    for h in hs {
        let kids = dom.children(h).to_vec();
        dom.replace_with(h, &kids)?;
    }
    let clicks: Vec<NodeId> = dom
        .descendants(root)
        .into_iter()
        .filter(|&e| matches!(dom.local_of(e), "hlinkClick" | "hlinkHover") && is_removed(dom, e))
        .collect();
    for e in clicks {
        dom.remove(e)?;
    }
    // settings that only exist with the relationship (attached template, mail merge)
    let settings: Vec<NodeId> = dom
        .descendants(root)
        .into_iter()
        .filter(|&e| dom.is_any(e, &[n::attachedTemplate, n::dataSource, n::headerSource]) && is_removed(dom, e))
        .collect();
    for e in settings {
        let mail = dom.ancestor(e, n::mailMerge);
        dom.remove(mail.unwrap_or(e))?;
    }
    for el in dom.descendants(root) {
        dom.remove_attrs_where(el, |d, a| (d.name_ns(a.name) == super::dom::NS_R || a.name == n::o_relid) && removed.contains(&a.value));
    }
    Ok(())
}
