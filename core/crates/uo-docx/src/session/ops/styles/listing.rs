//! Additive read-only accessors for the editor shell's style list and style look (milestone 2, the
//! editor switch, wave 5 frontier R; record `core/docs/modules/uo-bindings.md` "Wave 5 — R", ED-301).
//!
//! The pieces of `listStyles`/`styleLook` that need the private style index (`readStyleIndex`, the
//! built-in table, `latentOf`) are answered here over `&Session` (a query never mutates: the memoised
//! index is reused when it is fresh, else the part is read again). The filter, the order (priority then
//! the interface's collation) and the formatting policy stay in the shell (styles.ts `listStyles`'s last
//! lines; ED-302), and `lookOf` is ported by uo-wasm over [`Session::style_look_formatting`].

use std::collections::HashSet;
use std::sync::Arc;

use super::sheet::{self, StyleIndex, builtins};
use super::xform::{self, Defaults, StyleKind, list_styles_all};
use crate::model::formatting::DocFormatting;
use crate::session::Session;

/// One entry of `listStyles(ds, lang, inUse, 'all')` (`StyleInfo` of styles.ts) in definition order,
/// then the latent built-ins in the table's order.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleListEntry {
    pub id: String,
    pub name: String,
    pub raw_name: String,
    /// `paragraph` | `character` | `linked`
    pub kind: &'static str,
    pub built_in: bool,
    pub defined: bool,
    pub quick: bool,
    pub hidden: bool,
    pub priority: f64,
    pub in_use: bool,
    pub is_default: bool,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub link: Option<String>,
    /// the display name of `link` (the character twin), when defined
    pub link_name: Option<String>,
    pub auto_update: bool,
}

/// `xml.match(/<tag w:val="([^"]*)"/)?.[1]` over a built-in's XML.
fn val_of(xml: &str, tag: &str) -> Option<String> {
    let needle = format!("<{tag} w:val=\"");
    let p = xml.find(&needle)? + needle.len();
    let e = xml.get(p..)?.find('"')?;
    xml.get(p..p + e).map(str::to_owned)
}

/// `xml.match(/w:styleId="([^"]*)"/)?.[1]`.
fn style_id_of(xml: &str) -> Option<String> {
    let needle = "w:styleId=\"";
    let p = xml.find(needle)? + needle.len();
    let e = xml.get(p..)?.find('"')?;
    xml.get(p..p + e).map(str::to_owned)
}

impl Session {
    /// `docStyles.index()` without the memo's write: the memoised index when it is the current
    /// version's, else the part read again.
    fn style_index_now(&self) -> Arc<StyleIndex> {
        let v = self.styles.version();
        if let Some((pv, i)) = &self.f2.style_idx
            && *pv == v
        {
            return Arc::clone(i);
        }
        Arc::new(self.styles.xml().map_or_else(StyleIndex::default, |x| sheet::read_style_index(x)))
    }

    /// `docStyles.defaultParagraph()`: the index's first paragraph style with `w:default`, else
    /// `Normal`.
    #[must_use]
    pub fn default_paragraph_style(&self) -> String {
        Defaults::of(&self.style_index_now()).paragraph
    }

    /// `docStyles.defaultCharacter()`.
    #[must_use]
    pub fn default_character_style(&self) -> String {
        Defaults::of(&self.style_index_now()).character
    }

    /// `stylesInUse()` (universal-write.ts 4668): the `styleId` of every paragraph and the
    /// `charStyle` of every run of the document (`allParagraphs()`: body, its text boxes, the body
    /// tables' cells, the header/footer parts, the notes).
    #[must_use]
    pub fn styles_in_use(&self) -> HashSet<String> {
        let mut used = HashSet::new();
        self.all_paragraphs(&mut |p| {
            if let Some(s) = p.props.style_id.as_deref() {
                used.insert(s.to_owned());
            }
            for r in &p.runs {
                if let Some(c) = r.props.char_style.as_deref() {
                    used.insert(c.to_owned());
                }
            }
        });
        used
    }

    /// `listStyles(ds, lang, stylesInUse(), 'all')` before the final sort (see the module comment),
    /// in the session's interface language.
    #[must_use]
    pub fn style_list_entries(&self) -> Vec<StyleListEntry> {
        let idx = self.style_index_now();
        let in_use = self.styles_in_use();
        let def_para = Defaults::of(&idx).paragraph;
        let lang = self.f2.lang;
        let all = list_styles_all(&idx, lang);
        let mut out = Vec::with_capacity(all.len());
        for s in all {
            let kind = match s.kind {
                StyleKind::Paragraph => "paragraph",
                StyleKind::Character => "character",
                StyleKind::Linked => "linked",
            };
            if s.defined {
                let m = idx.styles.iter().find(|m| m.id == s.id);
                let link = m.and_then(|m| m.link.clone());
                let in_use_now = in_use.contains(&s.id) || s.id == def_para || link.as_ref().is_some_and(|l| in_use.contains(l));
                out.push(StyleListEntry {
                    in_use: in_use_now,
                    is_default: m.is_some_and(|m| m.is_default),
                    based_on: m.and_then(|m| m.based_on.clone()),
                    next: m.and_then(|m| m.next.clone()),
                    link_name: if kind == "linked" {
                        link.as_deref().and_then(|l| idx.styles.iter().find(|t| t.id == l)).map(|t| xform::display_name(t, lang))
                    } else {
                        None
                    },
                    link: if kind == "linked" { link } else { None },
                    auto_update: m.is_some_and(|m| m.auto_redefine),
                    id: s.id,
                    name: s.name,
                    raw_name: s.raw_name,
                    kind,
                    built_in: s.built_in,
                    defined: true,
                    quick: s.quick,
                    hidden: s.hidden,
                    priority: s.priority,
                });
            } else {
                let b = builtins().find(|b| b.name == s.raw_name);
                out.push(StyleListEntry {
                    in_use: false,
                    is_default: false,
                    based_on: b.and_then(|b| val_of(b.xml, "w:basedOn")),
                    next: b.and_then(|b| val_of(b.xml, "w:next")),
                    link: b.and_then(|b| b.twin).and_then(style_id_of),
                    link_name: None,
                    auto_update: false,
                    id: s.id,
                    name: s.name,
                    raw_name: s.raw_name,
                    kind,
                    built_in: true,
                    defined: false,
                    quick: s.quick,
                    hidden: s.hidden,
                    priority: s.priority,
                });
            }
        }
        out
    }

    /// The formatting `styleLook(ds, theme, resolveStyleId(xml, id))` resolves over (styles.ts
    /// `styleProps`): the document's style id for `id` and the document's formatting with the
    /// definitions of the latent built-ins the style's chain needs (`withLatent`). `None` without a
    /// styles part (the TS `styleFormatting` answers null for a document whose sheet is not editable;
    /// a `.docx` without `styles.xml` gets the empty sheet, as here).
    #[must_use]
    pub fn style_look_formatting(&self, id: &str) -> (String, DocFormatting) {
        let xml = self.styles.xml().map(|x| x.to_string());
        let rid = xform::resolve_style_id(xml.as_deref(), id);
        let mut fmt = (*self.doc.formatting).clone();
        if fmt.styles.styles.contains_key(&rid) {
            return (rid, fmt);
        }
        // withLatent: the built-in (or its twin) with this id and its `basedOn` chain, those the sheet lacks
        let mut extra = String::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut next = Some(rid.clone());
        let mut guard = 0;
        while let Some(sid) = next.take() {
            guard += 1;
            if guard > 64 || sid.is_empty() || seen.contains(&sid) || fmt.styles.styles.contains_key(&sid) {
                break;
            }
            seen.insert(sid.clone());
            let needle = format!("w:styleId=\"{sid}\"");
            let Some(b) = builtins().find(|x| x.xml.contains(&needle) || x.twin.is_some_and(|t| t.contains(&needle))) else { break };
            extra.push_str(b.xml);
            extra.push_str(b.twin.unwrap_or(""));
            next = val_of(b.xml, "w:basedOn");
        }
        if extra.is_empty() {
            return (rid, fmt);
        }
        let wrapped = format!("<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{extra}</w:styles>");
        if let Ok(more) = crate::read::parts::read_style_sheet_text(&wrapped) {
            for (k, v) in more.styles.iter() {
                if !fmt.styles.styles.contains_key(k) {
                    fmt.styles.styles.insert(k, v.clone());
                }
            }
        }
        (rid, fmt)
    }
}
