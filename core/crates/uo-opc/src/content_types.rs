//! `[Content_Types].xml`: reading (defaults and overrides, `content_type_of`) and the edits the
//! TS does as string replacements, as operations on the text so the rest of the part stays byte
//! for byte:
//! - docx-core `serialize.ts` (~181, 1649, 1663, 1749, 1812), `notes.ts` ~63, `rev-xml.ts` ~43,
//!   `settings-xml.ts` ~173, `table-styles.ts` ~156, editor `numbering-store.ts` ~122 and
//!   `styles.ts` ~536: insert before the first `</Types>` ([`Insert::BeforeFirstClose`]);
//! - `page-background.ts` ~211, editor `universal-write.ts` ~11911, fonts `ooxml.ts`
//!   `ensureDefaultContentType`: a Default right after `<Types …>` ([`Insert::AfterRootOpen`]);
//! - fonts `ensureOverride`: before `</Types>` at the end, trailing whitespace dropped
//!   ([`Insert::BeforeLastCloseTrimmed`]);
//! - slides `deck-edit.ts` `addOverride` ([`Insert::EndOfRoot`]) and `ensureDefault`
//!   ([`Insert::BeforeFirst`]`("Override")`);
//! - `hf-ops.ts` ~61 and slides `removeOverride`: [`remove_override`].
//!
//! The existence checks of those callers differ (`includes('comments+xml')`, case-insensitive
//! regexes on `Extension="…"`, …) and stay with the callers; [`ContentTypes`] answers the
//! structured questions.

use crate::error::Result;
use crate::xml::raw::{Insert, insert_child, remove_range};
use crate::xml::read::{ParseOptions, parse};
use crate::xml::tree::Document;
use crate::xml::write::{Escape, escape_into};

/// The part name.
pub const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
/// Namespace of the part.
pub const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// `<Default Extension ContentType/>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultType {
    pub extension: String,
    pub content_type: String,
}

/// `<Override PartName ContentType/>` (the part name with its leading `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideType {
    pub part_name: String,
    pub content_type: String,
}

/// The parsed declarations, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentTypes {
    pub defaults: Vec<DefaultType>,
    pub overrides: Vec<OverrideType>,
}

impl ContentTypes {
    /// Reads the `Default` and `Override` children of the root. The format crates read
    /// `[Content_Types].xml` with [`crate::Package::xml_part`] and
    /// [`ContentTypes::from_document`] instead (gate 2, D3).
    pub fn parse(xml: &str, opts: &ParseOptions<'_>) -> Result<Self> {
        Ok(ContentTypes::from_document(&parse(xml, opts)?))
    }

    /// The `Default` and `Override` children of the root of a parsed `[Content_Types].xml`.
    pub fn from_document(doc: &Document) -> Self {
        let mut ct = ContentTypes::default();
        for e in doc.root().child_elements() {
            match e.name() {
                "Default" => ct.defaults.push(DefaultType {
                    extension: e.attr("Extension").unwrap_or("").to_owned(),
                    content_type: e.attr("ContentType").unwrap_or("").to_owned(),
                }),
                "Override" => ct.overrides.push(OverrideType {
                    part_name: e.attr("PartName").unwrap_or("").to_owned(),
                    content_type: e.attr("ContentType").unwrap_or("").to_owned(),
                }),
                _ => {}
            }
        }
        ct
    }

    /// The override of a part (`part` without the leading `/`; exact match, slides
    /// `overrideOf`).
    pub fn override_of(&self, part: &str) -> Option<&str> {
        self.overrides.iter().find(|o| o.part_name.strip_prefix('/') == Some(part)).map(|o| o.content_type.as_str())
    }

    /// The default of an extension (ASCII case-insensitive).
    pub fn default_of(&self, extension: &str) -> Option<&str> {
        self.defaults.iter().find(|d| d.extension.eq_ignore_ascii_case(extension)).map(|d| d.content_type.as_str())
    }

    /// slides `contentTypeOf`: the override (exact part name), else the default of the extension
    /// after the last `.` (the whole name when there is none, as `lastIndexOf` gives −1).
    pub fn content_type_of(&self, part: &str) -> Option<&str> {
        if let Some(t) = self.override_of(part) {
            return Some(t);
        }
        let ext = part.rfind('.').map_or(part, |i| &part[i + 1..]);
        self.default_of(ext)
    }
}

fn attr_into(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_into(out, value, Escape::ATTR);
    out.push('"');
}

/// `<Default Extension="ext" ContentType="ct"/>` (values escaped `& < > "`).
pub fn default_xml(extension: &str, content_type: &str) -> String {
    let mut out = String::from("<Default");
    attr_into(&mut out, "Extension", extension);
    attr_into(&mut out, "ContentType", content_type);
    out.push_str("/>");
    out
}

/// `<Override PartName="/part" ContentType="ct"/>`.
pub fn override_xml(part: &str, content_type: &str) -> String {
    let mut out = String::from("<Override");
    attr_into(&mut out, "PartName", &format!("/{part}"));
    attr_into(&mut out, "ContentType", content_type);
    out.push_str("/>");
    out
}

/// Adds a Default to the text (no existence check: callers check as their TS did). Returns the
/// text unchanged when the anchor is missing.
pub fn add_default(xml: &str, extension: &str, content_type: &str, at: Insert<'_>) -> Result<String> {
    Ok(insert_child(xml, "Types", &default_xml(extension, content_type), at)?.unwrap_or_else(|| xml.to_owned()))
}

/// Adds an Override for `part` (without leading `/`).
pub fn add_override(xml: &str, part: &str, content_type: &str, at: Insert<'_>) -> Result<String> {
    Ok(insert_child(xml, "Types", &override_xml(part, content_type), at)?.unwrap_or_else(|| xml.to_owned()))
}

/// Removes the FIRST Override of `part` (hf-ops `ct.replace(/<Override PartName="\/path"[^>]*\/>/, '')`,
/// slides `removeOverride`); `None` when there is none.
pub fn remove_override(xml: &str, opts: &ParseOptions<'_>, part: &str) -> Result<Option<String>> {
    remove_first(xml, opts, "Override", |e| e.attr("PartName").and_then(|p| p.strip_prefix('/')) == Some(part))
}

/// Removes the first Default of an extension (ASCII case-insensitive).
pub fn remove_default(xml: &str, opts: &ParseOptions<'_>, extension: &str) -> Result<Option<String>> {
    remove_first(xml, opts, "Default", |e| e.attr("Extension").is_some_and(|x| x.eq_ignore_ascii_case(extension)))
}

fn remove_first(xml: &str, opts: &ParseOptions<'_>, name: &str, pred: impl Fn(crate::xml::Element<'_>) -> bool) -> Result<Option<String>> {
    let doc = parse(xml, &ParseOptions { positions: true, ..*opts })?;
    let hit = doc.root().children_named(name).find(|e| pred(*e)).and_then(|e| e.positions());
    Ok(hit.map(|p| remove_range(xml, p.start, p.end)))
}
