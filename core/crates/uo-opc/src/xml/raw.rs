//! Raw surgery on XML text: the generic part of `write/packages/docx-core/src/lossless.ts`
//! (`splitElement`, `setKid`, `withAttrs`, lines 59–118) and the string edits the TS does on
//! `.rels` and `[Content_Types].xml` (insert before `</Types>`, after `<Types …>`, …).
//!
//! The TS regexes are matched by hand, linearly (no backtracking blow-ups); where a regex
//! behaves in a surprising way the comment says so.

use std::borrow::Cow;

use super::read::ParseOptions;
use super::tree::{Document, ElementId};
use super::write::{Escape, escape_into};
use crate::error::{Error, Result, XmlErrorKind};
use crate::limits::Limits;
use uo_jsnum::text::is_js_space;

/// A child element of a split element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kid {
    /// the (renamed) name
    pub name: String,
    /// its XML
    pub raw: String,
    /// the parsed child in [`Split::doc`] (`None` for kids added by [`set_kid`])
    pub el: Option<ElementId>,
}

/// An element split into start tag, children and end tag.
#[derive(Debug)]
pub struct Split {
    pub open: String,
    pub close: String,
    pub kids: Vec<Kid>,
    /// the parsed element
    pub doc: Document,
}

fn trim_js_end(s: &str) -> &str {
    s.trim_end_matches(is_js_space)
}

/// lossless.ts `splitElement`: parses `raw` (with `opts` — the caller's preset; positions are
/// forced on — within `limits`, see [`Limits::bounded`]), then the start tag, the child
/// elements' raw text and the end tag. Text between children is dropped, as in the TS. A
/// self-closing element gives `open` with `/\s*\/>\s*$/` → `>` and `close` = `</name>` (the
/// renamed name).
pub fn split_element(raw: &str, opts: &ParseOptions<'_>, limits: &Limits) -> Result<Split> {
    let doc = limits.parse(raw, &ParseOptions { positions: true, ..*opts })?;
    let root = doc.root();
    let p = root.positions().ok_or(Error::Xml { offset: 0, kind: XmlErrorKind::Unexpected, part: None })?;
    if p.content_start == p.end {
        let open = replace_self_close(raw).map_or_else(|| raw.to_owned(), |s| s.into_owned());
        let close = format!("</{}>", root.name());
        return Ok(Split { open, close, kids: Vec::new(), doc });
    }
    let kids = root
        .child_elements()
        .filter_map(|k| {
            let kp = k.positions()?;
            Some(Kid { name: k.name().to_owned(), raw: raw.get(kp.start..kp.end)?.to_owned(), el: Some(k.id()) })
        })
        .collect();
    let open = raw.get(..p.content_start).unwrap_or("").to_owned();
    let close = raw.get(p.content_end..).unwrap_or("").to_owned();
    Ok(Split { open, close, kids, doc })
}

/// `raw.replace(/\s*\/>\s*$/, '>')`: the last `/>` followed only by whitespace, with the
/// whitespace before it, becomes `>`; `None` when it does not match.
fn replace_self_close(raw: &str) -> Option<Cow<'_, str>> {
    let body = trim_js_end(raw);
    let before = body.strip_suffix("/>")?;
    let start = trim_js_end(before).len();
    Some(Cow::Owned(format!("{}>", &raw[..start])))
}

/// lossless.ts `setKid`: replaces (or removes, with `xml` `None`) the kids named `name` — the
/// first one keeps its place, later ones are dropped — or inserts a new kid before the first
/// kid that comes after it in `order` (a name absent from `order` ranks before every listed
/// one, as `indexOf` gives −1).
pub fn set_kid(kids: &mut Vec<Kid>, order: &[&str], name: &str, xml: Option<&str>) {
    let at = kids.iter().position(|k| k.name == name);
    if let Some(at) = at {
        // one pass (stage-4 finding F4: a `remove` per repeated kid was O(n²))
        let mut i = 0;
        kids.retain(|k| {
            let keep = i <= at || k.name != name;
            i += 1;
            keep
        });
        match xml {
            None => {
                kids.remove(at);
            }
            Some(x) => kids[at].raw = x.to_owned(),
        }
        return;
    }
    let Some(xml) = xml else { return };
    let rank = |n: &str| order.iter().position(|o| *o == n).map_or(-1, |r| r as i64);
    let r = rank(name);
    let pos = kids.iter().position(|k| rank(&k.name) > r).unwrap_or(kids.len());
    kids.insert(pos, Kid { name: name.to_owned(), raw: xml.to_owned(), el: None });
}

/// The kids joined (`join` of lossless.ts): empty when there are none.
pub fn join_kids(open: &str, kids: &[Kid], close: &str) -> String {
    if kids.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(open.len() + close.len() + kids.iter().map(|k| k.raw.len()).sum::<usize>());
    out.push_str(open);
    for k in kids {
        out.push_str(&k.raw);
    }
    out.push_str(close);
    out
}

/// ` k="esc(v)"` with the lossless.ts `esc` (`& < > "`, no stripping).
fn push_attr(out: &mut String, k: &str, v: &str) {
    out.push(' ');
    out.push_str(k);
    out.push_str("=\"");
    escape_into(out, v, Escape::ATTR);
    out.push('"');
}

/// Finds `\s{k}\s*=\s*("[^"]*"|'[^']*')` in `attrs`: the span (from the whitespace before the
/// name to after the closing quote) of the first match.
///
/// The TS builds the regex from `k.replace(':', '\\:')` without escaping other characters;
/// attribute names never hold regex metacharacters, so `k` is matched literally here.
fn find_attr(attrs: &str, k: &str) -> Option<(usize, usize)> {
    let b = attrs.as_bytes();
    for (i, c) in attrs.char_indices() {
        if !is_js_space(c) {
            continue;
        }
        let mut j = i + c.len_utf8();
        if !attrs[j..].starts_with(k) {
            continue;
        }
        j += k.len();
        j = skip_js_space(attrs, j);
        if b.get(j) != Some(&b'=') {
            continue;
        }
        j = skip_js_space(attrs, j + 1);
        let Some(&q) = b.get(j) else { continue };
        if q != b'"' && q != b'\'' {
            continue;
        }
        if let Some(end) = b[j + 1..].iter().position(|&x| x == q) {
            return Some((i, j + 1 + end + 1));
        }
    }
    None
}

fn skip_js_space(s: &str, mut j: usize) -> usize {
    while let Some(c) = s.get(j..).and_then(|r| r.chars().next()) {
        if !is_js_space(c) {
            break;
        }
        j += c.len_utf8();
    }
    j
}

/// lossless.ts `withAttrs`: the start tag with attributes set (`Some`) or removed (`None`),
/// children kept. With empty `raw`, a new `<name …/>` with the given values. `None` when `raw`
/// does not start with a tag (the TS crashes on its `!` assertion there).
pub fn with_attrs(raw: &str, name: &str, changes: &[(&str, Option<&str>)]) -> Option<String> {
    if raw.is_empty() {
        let mut out = format!("<{name}");
        for (k, v) in changes {
            if let Some(v) = v {
                push_attr(&mut out, k, v);
            }
        }
        out.push_str("/>");
        return Some(out);
    }
    // /^<([^\s/>]+)([^>]*?)(\/?)>/
    let rest = raw.strip_prefix('<')?;
    let name_len = rest.find(|c: char| c == '/' || c == '>' || is_js_space(c)).unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }
    let tag_name = &rest[..name_len];
    let gt = name_len + rest[name_len..].find('>')?;
    // the lazy attribute group stops before a '/' right before the first '>'
    let slash = gt > name_len && rest.as_bytes()[gt - 1] == b'/';
    let attrs_end = if slash { gt - 1 } else { gt };
    let mut attrs = rest[name_len..attrs_end].to_owned();
    for (k, v) in changes {
        let hit = find_attr(&attrs, k);
        match (v, hit) {
            (None, Some((s, e))) => attrs.replace_range(s..e, ""),
            (None, None) => {}
            (Some(v), Some((s, e))) => {
                let mut rep = String::new();
                push_attr(&mut rep, k, v);
                attrs.replace_range(s..e, &rep);
            }
            (Some(v), None) => push_attr(&mut attrs, k, v),
        }
    }
    Some(format!("<{tag_name}{attrs}{}>{}", if slash { "/" } else { "" }, &rest[gt + 1..]))
}

// ───────────────────── container string edits (.rels, content types) ─────────────────────

/// Where a string edit inserts a child into a root element (`Types`, `Relationships`), as the
/// TS callers do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert<'a> {
    /// before the FIRST `</root>` (`xml.replace('</Types>', …)`, docx-core serialize.ts,
    /// hf-ops.ts, notes.ts); unchanged when absent
    BeforeFirstClose,
    /// right after the first `<root\b[^>]*>` (`xml.replace(/(<Types\b[^>]*>)/, …)`, fonts
    /// ooxml.ts `ensureDefaultContentType`, page-background.ts); unchanged when absent
    AfterRootOpen,
    /// replacing `/<\/root>\s*$/` (trailing whitespace dropped; fonts ooxml.ts `ensureOverride`,
    /// `addRel` — which first expands a self-closing root, see [`expand_self_closing`])
    BeforeLastCloseTrimmed,
    /// at the end of the root's content, a self-closing root expanded (slides `insertChild`)
    EndOfRoot,
    /// before the first element with this name after the root's start tag, else
    /// [`Insert::EndOfRoot`] (slides `ensureDefault`: Defaults before Overrides)
    BeforeFirst(&'a str),
}

/// Inserts `text` into the root element named `root` of `xml`. Returns the new text, or
/// `None` when the anchor is absent (the TS `replace` then leaves the text unchanged). The
/// anchors [`Insert::EndOfRoot`] and [`Insert::BeforeFirst`] parse `xml` (names as written)
/// within `limits` (see [`Limits::bounded`]).
pub fn insert_child(xml: &str, root: &str, text: &str, at: Insert<'_>, limits: &Limits) -> Result<Option<String>> {
    let splice = |pos: usize, cut: usize| -> String {
        let mut out = String::with_capacity(xml.len() + text.len());
        out.push_str(&xml[..pos]);
        out.push_str(text);
        out.push_str(&xml[cut..]);
        out
    };
    match at {
        Insert::BeforeFirstClose => {
            let close = format!("</{root}>");
            Ok(xml.find(&close).map(|p| splice(p, p)))
        }
        Insert::AfterRootOpen => Ok(find_open_tag(xml, root).map(|(_, end)| splice(end, end))),
        Insert::BeforeLastCloseTrimmed => {
            let close = format!("</{root}>");
            let body = trim_js_end(xml);
            if !body.ends_with(&close) {
                return Ok(None);
            }
            let p = body.len() - close.len();
            let mut out = splice(p, p);
            out.truncate(p + text.len());
            out.push_str(&close);
            Ok(Some(out))
        }
        Insert::EndOfRoot => end_of_root(xml, text, &positions(xml, limits)?),
        Insert::BeforeFirst(name) => {
            let doc = positions(xml, limits)?;
            match doc.root().descend(name).and_then(|e| e.positions()) {
                Some(p) => Ok(Some(splice(p.start, p.start))),
                None => end_of_root(xml, text, &doc),
            }
        }
    }
}

/// `xml` parsed with element positions and the names as written (the TS edits match tags
/// literally), within `limits`.
fn positions(xml: &str, limits: &Limits) -> Result<Document> {
    limits.parse(xml, &ParseOptions { positions: true, ..ParseOptions::LENIENT })
}

fn end_of_root(xml: &str, text: &str, doc: &Document) -> Result<Option<String>> {
    let r = doc.root();
    let Some(p) = r.positions() else { return Ok(None) };
    let mut out = String::with_capacity(xml.len() + text.len() + r.name().len() + 3);
    if p.content_start == p.end {
        // setInner of a `<x/>`: `/\s*\/>$/` → `>` on the tag, then the content and `</x>`
        let tag = &xml[p.start..p.end];
        let open = replace_self_close(tag).map_or_else(|| tag.to_owned(), |s| s.into_owned());
        out.push_str(&xml[..p.start]);
        out.push_str(&open);
        out.push_str(text);
        out.push_str("</");
        out.push_str(r.name());
        out.push('>');
        out.push_str(&xml[p.end..]);
    } else {
        out.push_str(&xml[..p.content_end]);
        out.push_str(text);
        out.push_str(&xml[p.content_end..]);
    }
    Ok(Some(out))
}

/// The first `<root\b[^>]*>`: (start, end after `>`).
fn find_open_tag(xml: &str, root: &str) -> Option<(usize, usize)> {
    let open = format!("<{root}");
    let mut from = 0;
    while let Some(k) = xml[from..].find(&open) {
        let s = from + k;
        let after = s + open.len();
        // `\b`: the next character is not a word character (or the end)
        let boundary = xml.as_bytes().get(after).is_none_or(|&c| !(c.is_ascii_alphanumeric() || c == b'_'));
        if boundary {
            let gt = xml[after..].find('>')?;
            return Some((s, after + gt + 1));
        }
        from = after;
    }
    None
}

/// fonts ooxml.ts `addRel`: `xml.replace(/<root\b([^>]*?)\s*\/>/, '<root$1></root>')` — the first
/// self-closing `<root …/>` is opened.
pub fn expand_self_closing(xml: &str, root: &str) -> String {
    let open = format!("<{root}");
    let mut from = 0;
    // the first `>` at or after the last search: reused while it lies ahead, and no `>` at all
    // ends the search (stage-4 finding F5: a search from every `<root` was O(n²))
    let mut next_gt: Option<usize> = None;
    while let Some(k) = xml[from..].find(&open) {
        let s = from + k;
        let after = s + open.len();
        let boundary = xml.as_bytes().get(after).is_none_or(|&c| !(c.is_ascii_alphanumeric() || c == b'_'));
        if boundary {
            let gt = match next_gt {
                Some(g) if g >= after => g,
                _ => match xml[after..].find('>') {
                    Some(g) => after + g,
                    None => break,
                },
            };
            next_gt = Some(gt);
            if gt > after && xml.as_bytes()[gt - 1] == b'/' {
                let attrs = trim_js_end(&xml[after..gt - 1]);
                return format!("{}<{root}{attrs}></{root}>{}", &xml[..s], &xml[gt + 1..]);
            }
        }
        from = after;
    }
    xml.to_owned()
}

/// Removes `[start, end)` of `xml`.
pub fn remove_range(xml: &str, start: usize, end: usize) -> String {
    let mut out = String::with_capacity(xml.len());
    out.push_str(xml.get(..start).unwrap_or(""));
    out.push_str(xml.get(end..).unwrap_or(""));
    out
}
