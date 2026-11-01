//! Package relationships (`.rels` parts), unified from the TS copies: sheet-core `read.ts`
//! `readRels`/`findRelByType`, sheet-core `drawing-charts.ts` `partRels`, slides-core
//! `ppt/shapeconv.ts` `zipRels`, slides `deck-edit.ts` (`readRelList`, `addRel`, `removeRels`,
//! `setRelTarget`, `nextRelId`), docx-core `document.ts` `buildReadContext` and
//! `shared/fonts/src/ooxml.ts` (`relsPathOf`, `readRels`, `addRel`, `removeRelsOfType`).
//!
//! Reading goes through the shared XML reader (the caller passes its preset); the per-app
//! filters (non-empty id, `./` stripped, external skipped, type suffix) are left to the callers,
//! which have the presence flags to reproduce each one. Editing works on the text, so the
//! rest of the part stays byte for byte.

use std::collections::HashSet;

use crate::error::Result;
use crate::limits::Limits;
use crate::xml::raw::{Insert, expand_self_closing, insert_child};
use crate::xml::read::ParseOptions;
use crate::xml::tree::{Document, Element};
use crate::xml::write::{Escape, XML_DECL, escape_into};

/// Namespace of `.rels` parts.
pub const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// Prefix of the officeDocument relationship types.
pub const REL_TYPE_PREFIX: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// One `<Relationship>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Relationship {
    /// `Id` (empty when absent)
    pub id: String,
    /// `Type` (empty when absent)
    pub type_: String,
    /// `Target` as written (entities decoded; empty when absent)
    pub target: String,
    /// `TargetMode="External"`
    pub external: bool,
    /// the `Id` attribute exists (sheet-core skips only MISSING ids/targets, docx-core empty ones)
    pub has_id: bool,
    /// the `Target` attribute exists
    pub has_target: bool,
}

impl Relationship {
    /// An internal relationship.
    pub fn new(id: &str, type_: &str, target: &str) -> Self {
        Relationship { id: id.into(), type_: type_.into(), target: target.into(), external: false, has_id: true, has_target: true }
    }

    /// The same, external (`TargetMode="External"`).
    pub fn external(mut self) -> Self {
        self.external = true;
        self
    }

    /// The `<Relationship …/>` element: Id, Type, Target escaped (`& < > "`), then
    /// `TargetMode="External"` when external — the attribute order of every TS writer.
    pub fn to_xml(&self) -> String {
        let mut out = String::with_capacity(48 + self.id.len() + self.type_.len() + self.target.len());
        out.push_str("<Relationship Id=\"");
        escape_into(&mut out, &self.id, Escape::ATTR_VALID);
        out.push_str("\" Type=\"");
        escape_into(&mut out, &self.type_, Escape::ATTR_VALID);
        out.push_str("\" Target=\"");
        escape_into(&mut out, &self.target, Escape::ATTR_VALID);
        out.push('"');
        if self.external {
            out.push_str(" TargetMode=\"External\"");
        }
        out.push_str("/>");
        out
    }
}

/// The `.rels` part of a part (`word/document.xml` → `word/_rels/document.xml.rels`; a root
/// part `x.xml` → `_rels/x.xml.rels`; the package `""` → `_rels/.rels`). Fonts `relsPathOf`,
/// slides `relsPathOf`, numbering-store, page-background.
pub fn rels_part_for(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => format!("{}_rels/{}.rels", &part[..=i], &part[i + 1..]),
        None => format!("_rels/{part}.rels"),
    }
}

/// The sheet-core/slides-core `relsPathFor` (read.ts:52, drawing-charts.ts:9, shapeconv.ts:826):
/// same as [`rels_part_for`] except for a part without `/`, where `part.slice(0, -1)` drops its
/// last character (`foo.xml` → `foo.xm/_rels/foo.xml.rels`). Kept for parity (gate 1, G5).
pub fn rels_part_for_legacy(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
        None => {
            let mut dir: Vec<char> = part.chars().collect();
            // JS slices UTF-16 units; dropping one char matches unless the last is astral
            dir.pop();
            format!("{}/_rels/{part}.rels", dir.into_iter().collect::<String>())
        }
    }
}

/// The part a `.rels` part describes, the inverse of [`rels_part_for`]
/// (`word/_rels/document.xml.rels` → `word/document.xml`, `_rels/.rels` → `""`, the package);
/// `None` for a name that is not a relationship part. With [`crate::Package::resolve_target`]
/// it is what a host validator needs to check every relationship of a package.
pub fn source_part_of(rels_part: &str) -> Option<String> {
    let (folder, file) = match rels_part.rsplit_once('/') {
        Some((dir, file)) => (dir, file),
        None => return None,
    };
    let source = file.strip_suffix(".rels")?;
    let folder = if folder == "_rels" { "" } else { folder.strip_suffix("/_rels")? };
    if folder.is_empty() { Some(source.to_owned()) } else { Some(format!("{folder}/{source}")) }
}

/// The `Relationship` children of the root, in order (duplicates kept). `opts` is the
/// caller's preset (sheet-core strips an `x:` prefix bound to the relationships namespace),
/// applied within `limits` ([`Limits::bounded`]). The format crates read a `.rels` part with
/// [`crate::Package::xml_part`] and [`from_document`] instead (gate 2, D3).
pub fn parse_rels(xml: &str, opts: &ParseOptions<'_>, limits: &Limits) -> Result<Vec<Relationship>> {
    Ok(from_document(&limits.parse(xml, opts)?))
}

/// The `Relationship` children of the root of a parsed `.rels` part, in order.
pub fn from_document(doc: &Document) -> Vec<Relationship> {
    doc.root().children_named("Relationship").map(relationship).collect()
}

/// A `<Relationship>` element as read.
fn relationship(r: Element<'_>) -> Relationship {
    let id = r.attr("Id");
    let target = r.attr("Target");
    Relationship {
        id: id.unwrap_or("").to_owned(),
        type_: r.attr("Type").unwrap_or("").to_owned(),
        target: target.unwrap_or("").to_owned(),
        external: r.attr("TargetMode") == Some("External"),
        has_id: id.is_some(),
        has_target: target.is_some(),
    }
}

/// The relationship an id resolves to: the LAST one with that id (the TS readers fill a `Map`,
/// where a later `set` wins).
pub fn find_by_id<'r>(rels: &'r [Relationship], id: &str) -> Option<&'r Relationship> {
    rels.iter().rev().find(|r| r.has_id && r.id == id)
}

/// The first relationship whose type ends with `suffix` (sheet-core `findRelByType`).
pub fn find_by_type_suffix<'r>(rels: &'r [Relationship], suffix: &str) -> Option<&'r Relationship> {
    rels.iter().find(|r| r.type_.ends_with(suffix))
}

/// A whole `.rels` part (docx `PartRels.xml()`: declaration, root, the elements in order).
pub fn write_rels(rels: &[Relationship], declaration: &str) -> String {
    let mut out = String::new();
    // a hint: a size that would wrap on wasm32 (or that the host refuses) only skips it
    let _ = out.try_reserve_exact(rels.len().saturating_mul(128).saturating_add(declaration.len() + 96));
    out.push_str(declaration);
    out.push_str("<Relationships xmlns=\"");
    out.push_str(RELS_NS);
    out.push_str("\">");
    for r in rels {
        out.push_str(&r.to_xml());
    }
    out.push_str("</Relationships>");
    out
}

/// An empty `.rels` part (`XML_DECL` with CRLF, as `doc/xmlw.ts`).
pub fn empty_rels() -> String {
    write_rels(&[], XML_DECL)
}

/// slides `nextRelId`: `rId` + (largest `rId<digits>` + 1).
pub fn next_rel_id_after_max(rels: &[Relationship]) -> String {
    let max = rels
        .iter()
        .filter_map(|r| r.id.strip_prefix("rId"))
        .filter(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
        .map(|d| d.parse::<u64>().unwrap_or(u64::MAX - 1))
        .max()
        .unwrap_or(0);
    format!("rId{}", max.saturating_add(1))
}

/// The first `{prefix}{n}` (n from `start`) not used as an id (notes.ts with `rId`/1; fonts
/// `addRel` with `rId`/`ids + 1`; serialize.ts with `rIdUwImg`/1). The TS tests `Id="…"` on the
/// text; ids are compared here, which is the same for well-formed parts.
pub fn first_free_rel_id(rels: &[Relationship], prefix: &str, start: u64) -> String {
    // hashed: comparing every candidate with every id was O(n²) (stage-4 finding F3)
    let used: HashSet<&str> = rels.iter().map(|r| r.id.as_str()).collect();
    let mut n = start;
    loop {
        let id = format!("{prefix}{n}");
        if !used.contains(id.as_str()) {
            return id;
        }
        n = n.wrapping_add(1);
    }
}

/// Adds a relationship to the text of a `.rels` part (`None` = the part does not exist yet:
/// an empty one with `XML_DECL` is used). `Insert::BeforeLastCloseTrimmed` first opens a
/// self-closing root, as fonts `addRel`. Returns the new text, or the old one when the anchor
/// is missing (the TS `replace` leaves it). The anchors that parse the text do it within
/// `limits` ([`insert_child`]).
pub fn add_rel(xml: Option<&str>, rel: &Relationship, at: Insert<'_>, limits: &Limits) -> Result<String> {
    let base = xml.map_or_else(empty_rels, str::to_owned);
    let base = if at == Insert::BeforeLastCloseTrimmed { expand_self_closing(&base, "Relationships") } else { base };
    Ok(insert_child(&base, "Relationships", &rel.to_xml(), at, limits)?.unwrap_or(base))
}

/// Removes the relationships for which `drop` is true (slides `removeRels`, fonts
/// `removeRelsOfType`, hf-ops `dropUnusedHfParts`); returns the new text and the removed ones.
/// The text is parsed with `opts` within `limits`.
pub fn remove_rels(
    xml: &str,
    opts: &ParseOptions<'_>,
    limits: &Limits,
    mut drop: impl FnMut(&Relationship) -> bool,
) -> Result<(String, Vec<Relationship>)> {
    let doc = limits.parse(xml, &ParseOptions { positions: true, ..*opts })?;
    let mut cut: Vec<(usize, usize)> = Vec::new();
    let mut removed = Vec::new();
    for r in doc.root().children_named("Relationship") {
        let rel = relationship(r);
        if drop(&rel)
            && let Some(p) = r.positions()
        {
            cut.push((p.start, p.end));
            removed.push(rel);
        }
    }
    // one pass over the ranges, which come in document order and do not overlap (stage-4
    // finding F2: one `remove_range` per relationship was O(n²))
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for &(s, e) in &cut {
        out.push_str(xml.get(last..s).unwrap_or(""));
        last = e;
    }
    out.push_str(xml.get(last..).unwrap_or(""));
    Ok((out, removed))
}

/// slides `setRelTarget`: the `Target` of the first relationship with `id` replaced. The text
/// is parsed with `opts` within `limits`.
pub fn set_rel_target(xml: &str, opts: &ParseOptions<'_>, limits: &Limits, id: &str, target: &str) -> Result<String> {
    let doc = limits.parse(xml, &ParseOptions { positions: true, ..*opts })?;
    let Some(r) = doc.root().children_named("Relationship").find(|r| r.attr("Id") == Some(id)) else {
        return Ok(xml.to_owned());
    };
    let Some(p) = r.positions() else { return Ok(xml.to_owned()) };
    let tag = &xml[p.start..p.content_start];
    let Some(new_tag) = crate::xml::raw::with_attrs(tag, r.name(), &[("Target", Some(target))]) else {
        return Ok(xml.to_owned());
    };
    Ok(format!("{}{}{}", &xml[..p.start], new_tag, &xml[p.content_start..]))
}
