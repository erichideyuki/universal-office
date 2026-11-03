//! The app-neutral pieces of the font-embedding writers of `shared/fonts/src/ooxml.ts`: the
//! `.rels` reader and edits, the content-type edits, the unique part names, `findElement` and the
//! removal loop over it, the GUID and the grouping of faces. Nothing here assumes Word: the pptx
//! writer (`writePptxEmbedding`, uo-pptx) reuses these as they are (the seam left for it); when
//! that crate lands, the pieces move to a crate both depend on (raw edits to uo-opc, the grouping
//! to uo-fonts).
//!
//! Every TS regular expression is a hand-written matcher with the exact JS semantics (no regex
//! engine; G9), linear in the text, checked against V8 by `tests/regex_d1b_fonts.rs` over the
//! inputs of `js/regex-sites/d1b-fonts.mjs`. The row of each site is in the record
//! (`core/docs/modules/uo-docx.md`, "D1b — font embedding").

use std::collections::{HashMap, HashSet};

use uo_fonts::{FaceToEmbed, Variant};
use uo_jsnum::text::{is_js_space, to_lowercase, trim_end};
use uo_opc::Package;
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::raw::Insert;

use crate::error::DocxError;

/// `${REL}/font`: the relationship from the font table (or the presentation) to a font part.
pub const REL_FONT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font";

/// The `.rels` part `addRel` starts from when there is none (declaration and LF, as ooxml.ts:80;
/// uo-opc's `empty_rels` has CRLF).
pub const RELS_TEMPLATE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";

/// The content types part.
pub const CONTENT_TYPES: &str = "[Content_Types].xml";

// ───────────────────────── character classes and small finders ─────────────────────────

/// `\w` of a JS `RegExp` without `u` (ASCII).
#[must_use]
pub fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\b` right after a word character that ends at `at`: the byte at `at` is not `\w` (or the end).
fn boundary_after_word(s: &str, at: usize) -> bool {
    s.as_bytes().get(at).is_none_or(|&b| !is_word_byte(b))
}

/// `[\s/>]` at byte `at` (JS white space, `/` or `>`); false at the end.
fn is_space_slash_gt_at(s: &str, at: usize) -> bool {
    match s.get(at..).and_then(|r| r.chars().next()) {
        Some(c) => is_js_space(c) || c == '/' || c == '>',
        None => false,
    }
}

/// `hay.indexOf(needle, from)` (byte offsets; `from` past the end finds nothing).
#[must_use]
pub fn find_from(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.get(from..).and_then(|r| r.find(needle)).map(|k| from + k)
}

/// The first `needle` at or after `from`, ASCII letters compared case-insensitively — what a JS
/// `RegExp` with `i` and without `u` does with an ASCII pattern: Canonicalize never takes a unit
/// of 128 or more to ASCII, so only ASCII letters fold.
#[must_use]
pub fn find_ascii_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    let first = *n.first()?;
    let mut i = from;
    while i + n.len() <= h.len() {
        // a bounded memchr-like skip on the first byte, both cases
        let rest = h.get(i..)?;
        let k = rest.iter().position(|&b| b.eq_ignore_ascii_case(&first))?;
        i += k;
        if h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Memo of "the first occurrence at or after `x`" for queries that mostly increase: the answer
/// for `x` is the cached one when `x` lies between the cached query and its answer (there is no
/// occurrence in between), so a scan over the text runs once in total.
#[derive(Debug)]
pub(crate) struct NextFind<'a> {
    hay: &'a str,
    needle: &'a str,
    ci: bool,
    cache: Option<(usize, Option<usize>)>,
}

impl<'a> NextFind<'a> {
    pub(crate) fn new(hay: &'a str, needle: &'a str, ci: bool) -> Self {
        NextFind { hay, needle, ci, cache: None }
    }

    pub(crate) fn at(&mut self, x: usize) -> Option<usize> {
        if let Some((from, hit)) = self.cache
            && x >= from
            && hit.is_none_or(|h| x <= h)
        {
            return hit;
        }
        let hit = if self.ci { find_ascii_ci(self.hay, self.needle, x) } else { find_from(self.hay, self.needle, x) };
        self.cache = Some((x, hit));
        hit
    }
}

// ───────────────────────── attribute escaping (ooxml.ts:43–47) ─────────────────────────

/// `escAttr`: `&`, `<`, `>`, `"` escaped (the four chained replaces, `&` first: one pass).
#[must_use]
pub fn esc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `unescAttr`: the five replaces in TS order (`&quot;`, `&lt;`, `&gt;`, `&apos;`, then `&amp;`).
/// One left-to-right pass gives the same text: every reference starts with the only `&` it
/// holds, and no replacement produces `&` except the last one, so no pass creates or hides an
/// occurrence of a later one (`&amp;lt;` stays `&lt;`).
#[must_use]
pub fn unesc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        out.push_str(rest.get(..k).unwrap_or(""));
        let tail = rest.get(k..).unwrap_or("");
        let hit = [("&quot;", '"'), ("&lt;", '<'), ("&gt;", '>'), ("&apos;", '\''), ("&amp;", '&')]
            .into_iter()
            .find(|(e, _)| tail.starts_with(e));
        match hit {
            Some((e, c)) => {
                out.push(c);
                rest = tail.get(e.len()..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

// ───────────────────────── String.prototype.replace substitutions ─────────────────────────

/// The length of [`substitute`]'s result, saturating (to check a cap before building it).
#[must_use]
pub fn substitution_len(template: &str, matched: usize, before: usize, after: usize) -> usize {
    let b = template.as_bytes();
    let (mut i, mut n) = (0usize, 0usize);
    while let Some(&c) = b.get(i) {
        if c == b'$' {
            match b.get(i + 1) {
                Some(b'$') => {
                    n = n.saturating_add(1);
                    i += 2;
                    continue;
                }
                Some(b'&') => {
                    n = n.saturating_add(matched);
                    i += 2;
                    continue;
                }
                Some(b'`') => {
                    n = n.saturating_add(before);
                    i += 2;
                    continue;
                }
                Some(b'\'') => {
                    n = n.saturating_add(after);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        n = n.saturating_add(1);
        i += 1;
    }
    n
}

/// `GetSubstitution` (ECMA-262) for a replace without capture groups — a string pattern
/// (`text.replace(el, merged)`) or a regex without groups: `$$` → `$`, `$&` → the match, `` $` ``
/// → the text before it, `$'` → the text after it; `$n`, `$nn`, `$<` and a lone `$` stay as written
/// (no captures, no named groups).
#[must_use]
pub fn substitute(template: &str, matched: &str, before: &str, after: &str) -> String {
    let mut out = String::with_capacity(substitution_len(template, matched.len(), before.len(), after.len()).min(1 << 20));
    let mut rest = template;
    while let Some(k) = rest.find('$') {
        out.push_str(rest.get(..k).unwrap_or(""));
        let tail = rest.get(k..).unwrap_or("");
        let (insert, used): (&str, usize) = match tail.as_bytes().get(1) {
            Some(b'$') => ("$", 2),
            Some(b'&') => (matched, 2),
            Some(b'`') => (before, 2),
            Some(b'\'') => (after, 2),
            _ => ("$", 1),
        };
        out.push_str(insert);
        rest = tail.get(used..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// `text.replace(regexOrString, template)` for one match `[start, end)` of `text`, without
/// captures, refused (`None`) when the result would exceed `cap` bytes — a family name or an
/// attribute holding `` $` `` doubles the text each time it is substituted (the TS has no cap).
#[must_use]
pub fn replace_range(text: &str, start: usize, end: usize, template: &str, cap: u64) -> Option<String> {
    let (before, matched, after) = (text.get(..start)?, text.get(start..end)?, text.get(end..)?);
    let len = substitution_len(template, matched.len(), before.len(), after.len()).saturating_add(before.len()).saturating_add(after.len());
    if u64::try_from(len).unwrap_or(u64::MAX) > cap {
        return None;
    }
    let mut out = String::new();
    out.try_reserve_exact(len).ok()?;
    out.push_str(before);
    out.push_str(&substitute(template, matched, before, after));
    out.push_str(after);
    Some(out)
}

// ───────────────────────── .rels (ooxml.ts:49–111) ─────────────────────────

/// How a relationship target names a part. Production is [`TargetRule::Opc`] (uo-opc D6, the one
/// rule of the library and its hosts). [`TargetRule::Fonts09`] is the 0.9 rule of ooxml.ts
/// (`/x` → `x`; otherwise from the source's folder, `..` pops, `.` is skipped, empty segments
/// stay; exact lookup), kept only so that the parity replay can show that a difference from the
/// captured TS comes from D6 and from nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetRule {
    #[default]
    Opc,
    Fonts09,
}

impl TargetRule {
    /// The part `target` of `source` names, when it exists.
    #[must_use]
    pub fn resolve(self, pkg: &Package, source: &str, target: &str) -> Option<String> {
        match self {
            TargetRule::Opc => pkg.resolve_target(source, target).map(str::to_owned),
            TargetRule::Fonts09 => Some(fonts09_target(source, target)).filter(|p| pkg.contains(p)),
        }
    }
}

/// The `resolveTarget` of ooxml.ts:54.
#[must_use]
pub fn fonts09_target(source: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_owned();
    }
    let mut parts: Vec<&str> = source.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." => {}
            _ => parts.push(seg),
        }
    }
    parts.join("/")
}

/// One `<Relationship …>` as the regex reader of ooxml.ts reads it: the relationship and the
/// byte range of the matched tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelTag {
    pub rel: Relationship,
    pub start: usize,
    pub end: usize,
}

/// `new RegExp('\\b' + name + '="([^"]*)"').exec(tag)?.[1]`: the value of the first `name="…"`
/// preceded by a non-word character (or the start) and closed by a `"`.
#[must_use]
pub fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(q) = find_from(tag, name, from) {
        from = q + 1;
        let boundary = q == 0 || tag.as_bytes().get(q - 1).is_some_and(|&b| !is_word_byte(b));
        let open = q + name.len();
        if !boundary || tag.get(open..open + 2) != Some("=\"") {
            continue;
        }
        if let Some(close) = find_from(tag, "\"", open + 2) {
            return tag.get(open + 2..close);
        }
    }
    None
}

/// `readRels` over a `.rels` text: every `/<Relationship\b[^>]*\/?>/g` match (from
/// `<Relationship` followed by a non-word character up to the first `>`), with `Id`, `Type`,
/// `Target` (`unescAttr`) and `TargetMode === 'External'` read by [`attr_value`]; a missing
/// attribute is the empty string.
#[must_use]
pub fn read_rels_text(xml: &str) -> Vec<RelTag> {
    const OPEN: &str = "<Relationship";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(s) = find_from(xml, OPEN, from) {
        if !boundary_after_word(xml, s + OPEN.len()) {
            from = s + 1;
            continue;
        }
        // no `>` after this start: none after any later one either
        let Some(gt) = find_from(xml, ">", s + OPEN.len()) else { break };
        let tag = xml.get(s..=gt).unwrap_or("");
        let id = attr_value(tag, "Id");
        let target = attr_value(tag, "Target");
        out.push(RelTag {
            rel: Relationship {
                id: id.unwrap_or("").to_owned(),
                type_: attr_value(tag, "Type").unwrap_or("").to_owned(),
                target: unesc_attr(target.unwrap_or("")),
                external: attr_value(tag, "TargetMode") == Some("External"),
                has_id: id.is_some(),
                has_target: target.is_some(),
            },
            start: s,
            end: gt + 1,
        });
        from = gt + 1;
    }
    out
}

/// The text of a part when it is non-empty (the TS `if (!xml)` tests): `None` for a missing or
/// empty part (a part holding only a UTF-8 BOM decodes to `''`).
pub fn part_text_nonempty(pkg: &Package, name: &str) -> Result<Option<String>, DocxError> {
    Ok(pkg.part_text(name)?.map(std::borrow::Cow::into_owned).filter(|t| !t.is_empty()))
}

/// `readRels(pkg, part)`: the relationships of a part's `.rels` (none when it is missing or empty).
pub fn read_rels(pkg: &Package, part: &str) -> Result<Vec<RelTag>, DocxError> {
    Ok(part_text_nonempty(pkg, &rels::rels_part_for(part))?.map(|t| read_rels_text(&t)).unwrap_or_default())
}

/// `addRel`: a relationship appended to the part's `.rels` ([`RELS_TEMPLATE`] when it is
/// missing), with the first free `rId{n}` from n = (distinct ids + 1) — the empty id counts as
/// one — and uo-opc `add_rel` at [`Insert::BeforeLastCloseTrimmed`] (which first opens a
/// self-closing root, as the TS). The part is written even when the anchor is missing (the TS
/// `replace` leaves the text, `setPartText` still runs). Returns the id.
pub fn add_rel(pkg: &mut Package, part: &str, type_: &str, target: &str) -> Result<String, DocxError> {
    let path = rels::rels_part_for(part);
    let current = pkg.part_text(&path)?.map(std::borrow::Cow::into_owned);
    let tags = read_rels_text(current.as_deref().unwrap_or(""));
    let distinct = tags.iter().map(|t| t.rel.id.as_str()).collect::<HashSet<_>>().len();
    let rels_list: Vec<Relationship> = tags.into_iter().map(|t| t.rel).collect();
    let id = rels::first_free_rel_id(&rels_list, "rId", u64::try_from(distinct).unwrap_or(u64::MAX).saturating_add(1));
    let base = current.as_deref().unwrap_or(RELS_TEMPLATE);
    let xml = rels::add_rel(Some(base), &Relationship::new(&id, type_, target), Insert::BeforeLastCloseTrimmed, pkg.limits())?;
    pkg.set_part_text(path, xml);
    Ok(id)
}

/// `removeRelsOfType`: the relationships of `type_` cut from the part's `.rels` and the parts
/// they name removed. Two differences from the TS, accepted by name (record, D1b fonts):
/// - the TS removes `out.replace(r.xml, '')`, the **first occurrence** of the tag's text, which
///   is the tag itself unless the same text also occurs earlier (inside another tag's attribute
///   value, or joined across a removed tag — text no writer produces); here each tag is cut at its
///   own position, one pass;
/// - the targets are resolved with `rule` (production: uo-opc D6, [`TargetRule::Opc`]), not the
///   fonts `resolveTarget`.
pub fn remove_rels_of_type(pkg: &mut Package, part: &str, type_: &str, rule: TargetRule) -> Result<(), DocxError> {
    let path = rels::rels_part_for(part);
    let Some(xml) = part_text_nonempty(pkg, &path)? else { return Ok(()) };
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    let mut any = false;
    for t in read_rels_text(&xml) {
        if t.rel.type_ != type_ {
            continue;
        }
        any = true;
        out.push_str(xml.get(last..t.start).unwrap_or(""));
        last = t.end;
        if !t.rel.external
            && let Some(name) = rule.resolve(pkg, part, &t.rel.target)
        {
            pkg.remove_part(&name);
        }
    }
    out.push_str(xml.get(last..).unwrap_or(""));
    if any {
        pkg.set_part_text(path, out);
    }
    Ok(())
}

/// `relTargetOfType`: the part named by the first internal relationship of exactly `type_`.
/// `None` when there is no such relationship; `Some(None)` when there is one but its target
/// names no part (uo-opc D6: the TS keeps the resolved path, whose part is then missing — every
/// caller reads it as missing).
pub fn rel_target_of_type(pkg: &Package, part: &str, type_: &str, rule: TargetRule) -> Result<Option<Option<String>>, DocxError> {
    let tags = read_rels(pkg, part)?;
    Ok(tags.into_iter().find(|t| t.rel.type_ == type_ && !t.rel.external).map(|t| rule.resolve(pkg, part, &t.rel.target)))
}

// ───────────────────────── content types (ooxml.ts:113–123) ─────────────────────────

/// `/<Default\b[^>]*Extension="ext"/i.test(xml)` (`ext` matched literally): some `<default`
/// (ASCII case-insensitive) followed by a non-word character or the end, whose text up to the
/// next `>` (or the end) holds `extension="ext"` (ASCII case-insensitive).
#[must_use]
pub fn has_default_extension(xml: &str, ext: &str) -> bool {
    let lit = format!("Extension=\"{ext}\"");
    let mut gts = NextFind::new(xml, ">", false);
    let mut lits = NextFind::new(xml, &lit, true);
    let mut from = 0;
    while let Some(p) = find_ascii_ci(xml, "<Default", from) {
        from = p + 1;
        let after = p + "<Default".len();
        if !boundary_after_word(xml, after) {
            continue;
        }
        let end = gts.at(after).unwrap_or(xml.len());
        if lits.at(after).is_some_and(|l| l < end) {
            return true;
        }
    }
    false
}

/// `ensureDefaultContentType`: a `<Default>` for `ext` after the `<Types …>` start tag unless the
/// part already declares one (or is missing or empty).
pub fn ensure_default_content_type(pkg: &mut Package, ext: &str, content_type: &str) -> Result<(), DocxError> {
    let Some(xml) = part_text_nonempty(pkg, CONTENT_TYPES)? else { return Ok(()) };
    if has_default_extension(&xml, ext) {
        return Ok(());
    }
    let out = uo_opc::content_types::add_default(&xml, ext, content_type, Insert::AfterRootOpen, pkg.limits())?;
    pkg.set_part_text(CONTENT_TYPES, out);
    Ok(())
}

/// `ensureOverride`: an `<Override>` for `part` before the final `</Types>` unless the text
/// holds `PartName="/part"` (case-sensitive) or the part is missing or empty.
pub fn ensure_override(pkg: &mut Package, part: &str, content_type: &str) -> Result<(), DocxError> {
    let Some(xml) = part_text_nonempty(pkg, CONTENT_TYPES)? else { return Ok(()) };
    if xml.contains(&format!("PartName=\"/{part}\"")) {
        return Ok(());
    }
    let out = uo_opc::content_types::add_override(&xml, part, content_type, Insert::BeforeLastCloseTrimmed, pkg.limits())?;
    pkg.set_part_text(CONTENT_TYPES, out);
    Ok(())
}

// ───────────────────────── unique part names (ooxml.ts:125–130) ─────────────────────────

/// `uniquePartName` for a run of new font parts in one directory: the names of the package
/// lowercased (`toLowerCase`) once, `{dir}font{n}.{ext}` from n = 1. The TS rereads the part
/// names on every call; the names only grow while one writer runs (each chosen name is set
/// right away), so the next free `n` is past the previous one and the set is kept here.
#[derive(Debug)]
pub struct FontPartNames {
    lower: HashSet<String>,
    next: u64,
}

impl FontPartNames {
    #[must_use]
    pub fn new(pkg: &Package) -> Self {
        FontPartNames { lower: pkg.part_names().map(to_lowercase).collect(), next: 1 }
    }

    /// The next free name (and it is taken).
    pub fn take(&mut self, dir: &str, ext: &str) -> String {
        let mut n = self.next;
        loop {
            let name = format!("{dir}font{n}.{ext}");
            let lower = to_lowercase(&name);
            if !self.lower.contains(&lower) {
                self.lower.insert(lower);
                self.next = n.saturating_add(1);
                return name;
            }
            n = n.saturating_add(1);
        }
    }
}

// ───────────────────────── elements by name (ooxml.ts:132–146) ─────────────────────────

/// `elementEnd`: after the first `>` from `start` when the character before it is `/`, else
/// after the first `</qname>` at or after that `>`; `None` when either is missing.
#[must_use]
pub fn element_end(xml: &str, start: usize, qname: &str) -> Option<usize> {
    let gt = find_from(xml, ">", start)?;
    if gt > 0 && xml.as_bytes().get(gt - 1) == Some(&b'/') {
        return Some(gt + 1);
    }
    let close = format!("</{qname}>");
    find_from(xml, &close, gt).map(|c| c + close.len())
}

/// The first `<qname` followed by JS white space, `/` or `>`, at or after `from`.
fn find_open_ahead(xml: &str, open: &str, from: usize) -> Option<usize> {
    let mut f = from;
    while let Some(p) = find_from(xml, open, f) {
        if is_space_slash_gt_at(xml, p + open.len()) {
            return Some(p);
        }
        f = p + 1;
    }
    None
}

/// `findElement`: the first `/<qname(?=[\s/>])/` match and its [`element_end`]; `None` when there
/// is no match or the first match has no end (the TS does not look further).
#[must_use]
pub fn find_element(xml: &str, qname: &str) -> Option<(usize, usize)> {
    let start = find_open_ahead(xml, &format!("<{qname}"), 0)?;
    element_end(xml, start, qname).map(|end| (start, end))
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i.min(s.len())
}

/// `for (el = findElement(xml, q); el; el = findElement(xml, q)) xml = xml.slice(0, el.start) +
/// xml.slice(el.end)`, in one pass. After a removal the next match cannot start more than
/// `|<qname|` bytes before the cut (anything earlier was text before the first match), so the text
/// is the output so far plus the rest of the input, and only a `<qname` straddling the two (a
/// start joined to the text after the removed element) is looked for in a small window.
#[must_use]
pub fn remove_elements(xml: &str, qname: &str) -> String {
    let open = format!("<{qname}");
    let close = format!("</{qname}>");
    let ol = open.len();
    let mut out = String::with_capacity(xml.len());
    let mut i = 0usize;
    loop {
        // a match that starts in the output and ends in the rest of the input
        let mut straddle = None;
        if !out.is_empty() && i < xml.len() {
            let q0 = floor_boundary(&out, out.len().saturating_sub(ol));
            let head = ceil_boundary(xml, i.saturating_add(ol + 4).min(xml.len()));
            let split = out.len() - q0;
            let mut w = String::with_capacity(split + head - i);
            w.push_str(out.get(q0..).unwrap_or(""));
            w.push_str(xml.get(i..head).unwrap_or(""));
            let mut f = 0;
            while let Some(p) = find_from(&w, &open, f) {
                if p >= split {
                    break;
                }
                if is_space_slash_gt_at(&w, p + ol) {
                    straddle = Some(q0 + p);
                    break;
                }
                f = p + 1;
            }
        }
        if let Some(q) = straddle {
            // the `>` is in the input (the output part is a prefix of `<qname`)
            let Some(gt) = find_from(xml, ">", i) else { break };
            let slash = if gt > i { xml.as_bytes().get(gt - 1) == Some(&b'/') } else { out.as_bytes().last() == Some(&b'/') };
            let end = if slash {
                gt + 1
            } else {
                match find_from(xml, &close, gt) {
                    Some(c) => c + close.len(),
                    None => break,
                }
            };
            out.truncate(q);
            i = end;
            continue;
        }
        let Some(p) = find_open_ahead(xml, &open, i) else { break };
        let Some(end) = element_end(xml, p, qname) else { break };
        out.push_str(xml.get(i..p).unwrap_or(""));
        i = end;
    }
    out.push_str(xml.get(i..).unwrap_or(""));
    out
}

/// The first `/<name\b[^>]*>/` match: (start, end after `>`); `None` when the first `<name`
/// followed by a non-word character has no `>` after it (no later one can).
#[must_use]
pub fn find_start_tag(xml: &str, name: &str) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(s) = find_from(xml, &open, from) {
        if boundary_after_word(xml, s + open.len()) {
            let gt = find_from(xml, ">", s + open.len())?;
            return Some((s, gt + 1));
        }
        from = s + 1;
    }
    None
}

// ───────────────────────── GUID and grouping (ooxml.ts:148–173) ─────────────────────────

/// `randomGuid` over 16 random bytes: version 4 and variant bits set, upper-case hex,
/// `{8-4-4-4-12}`. The bytes come from the caller (the host's random source; tests use a seeded
/// one), the TS `crypto.getRandomValues`.
#[must_use]
pub fn guid_of(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(38);
    out.push('{');
    for (i, x) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX[usize::from(x >> 4)]));
        out.push(char::from(HEX[usize::from(x & 0x0f)]));
    }
    out.push('}');
    out
}

/// `groupByFamily`: families in first-appearance order (exact spelling), each family's faces
/// stable-sorted by the `VARIANTS` order.
#[must_use]
pub fn group_by_family(faces: &[FaceToEmbed]) -> Vec<(&str, Vec<&FaceToEmbed>)> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<(&str, Vec<&FaceToEmbed>)> = Vec::new();
    for f in faces {
        match index.get(f.family.as_str()) {
            Some(&i) => {
                if let Some(g) = out.get_mut(i) {
                    g.1.push(f);
                }
            }
            None => {
                index.insert(f.family.as_str(), out.len());
                out.push((f.family.as_str(), vec![f]));
            }
        }
    }
    for g in &mut out {
        g.1.sort_by_key(|f| variant_rank(f.variant));
    }
    out
}

fn variant_rank(v: Variant) -> u8 {
    match v {
        Variant::Regular => 0,
        Variant::Bold => 1,
        Variant::Italic => 2,
        Variant::BoldItalic => 3,
    }
}

/// `/<\/root>\s*$/`: the start of the final `</root>` when only JS white space follows it.
#[must_use]
pub fn last_close_trimmed(xml: &str, root: &str) -> Option<usize> {
    let close = format!("</{root}>");
    let body = trim_end(xml);
    body.ends_with(&close).then(|| body.len() - close.len())
}
