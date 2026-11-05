//! `writeDocument` / `writeParts` (serialize.ts:1587–1792) and the package edits around the
//! stories: comments (`patchComments`, a dynamic-regex site), header/footer parts, notes parts,
//! the body, the orphaned comment marks, the queued media (deduplicated by **full** bytes, G24),
//! numbering (`mergeNumbering`, `writeNumberingPart`), the root namespaces (`withNamespaces`,
//! `revRoot`), the new hyperlink and media relationships of the part being written
//! (`linkRelId`, `mediaRelId`).
//!
//! String edits keep the TS `String.prototype.replace` rules: a string replacement is a template
//! (`$&`, `` $` ``, `$'`, `$$` substituted — a `$` in a path or a comment's text does what it does
//! in the TS), a function replacement is literal. Parts are set only when their text changed
//! (the TS sets some unchanged; the decompressed bytes are the same — declared).

use std::borrow::Cow;
use std::collections::HashSet;

use indexmap::IndexMap;
use uo_opc::Package;

use super::ctx::{DEFAULT_NUM_IDS, GenNumIds, Lossless, SaveState, WriteCtx, Writer};
use super::emit;
use super::js::{esc_attr, esc_text};
use super::lossless::{HfRefXml, WResult, patch_sect_pr};
use super::matchers::{
    boundary_at, drop_comment_marks, find_from, find_literal_ci, has_prefixed_attr, has_xmlns, is_word_rels_path, rel_id_of,
    relationship_tags, remove_tags,
};
use super::sig::Pristine;
use crate::error::DocxError;
use crate::js::{NumKey, Str};
use crate::model::Document;
use crate::model::document::Comment;
use crate::model::section::SectPr;
use crate::model::story::{Block, Story};

pub const CONTENT_TYPES: &str = "[Content_Types].xml";
pub const DOC_RELS: &str = "word/_rels/document.xml.rels";
/// The `.rels` the TS starts from when a part has none (no line break after the declaration).
pub const EMPTY_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const W_NS: &str = concat!(
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\""
);
const W15_NS: &str = " xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\"";
const W_NS_HF: &str = concat!(
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\""
);

// ───────────────────────────── string edits ─────────────────────────────

/// `s.replace(needle, () => text)`: the first occurrence replaced literally (a function
/// replacement), unchanged when absent.
#[must_use]
pub fn insert_before_first(s: &str, needle: &str, text: &str) -> String {
    match s.find(needle) {
        Some(i) => format!("{}{text}{}", &s[..i], &s[i..]),
        None => s.to_owned(),
    }
}

/// `s.replace(needle, template)` with a string template: the first occurrence replaced with the
/// `GetSubstitution` of `template` (`$&`, `` $` ``, `$'`, `$$`).
#[must_use]
pub fn js_replace_first(s: &str, needle: &str, template: &str) -> String {
    match s.find(needle) {
        Some(i) => {
            let (before, after) = (&s[..i], &s[i + needle.len()..]);
            let rep = super::fonts::ooxml::substitute(template, needle, before, after);
            format!("{before}{rep}{after}")
        }
        None => s.to_owned(),
    }
}

/// `s.replace(/<\/ROOT>\s*$/, template)`: the last close tag when only white space follows (the
/// white space dropped), with a string template.
#[must_use]
pub fn replace_last_close(s: &str, root: &str, template: &str) -> String {
    match super::fonts::ooxml::last_close_trimmed(s, root) {
        Some(i) => {
            let before = &s[..i];
            let matched = &s[i..];
            let rep = super::fonts::ooxml::substitute(template, matched, before, "");
            format!("{before}{rep}")
        }
        None => s.to_owned(),
    }
}

fn part_text(pkg: &Package, name: &str) -> WResult<Option<String>> {
    Ok(pkg.part_text(name)?.map(Cow::into_owned))
}

/// Sets a part's text when it differs (see the module comment).
fn set_text(pkg: &mut Package, name: &str, text: String) -> WResult<()> {
    if pkg.part_text(name)?.is_some_and(|t| *t == *text) {
        return Ok(());
    }
    pkg.set_part_text(name, text);
    Ok(())
}

/// `partByRelType(pkg, type, fallback)` with uo-opc D6 resolution: `Some(fallback)` when no
/// relationship matches, `None` when the first matching one names no part (accepted with D6).
pub fn part_by_rel_type(pkg: &Package, type_: &str, fallback: &str) -> WResult<Option<String>> {
    let Some(text) = pkg.part_text(DOC_RELS)?.filter(|t| !t.is_empty()) else { return Ok(Some(fallback.to_owned())) };
    let doc = uo_opc::xml::parse(text.into_owned(), &pkg.limits().bounded(&crate::read::DOCX))?;
    let suffix = format!("/{type_}");
    for rel in uo_opc::rels::from_document(&doc) {
        if rel.type_.ends_with(&suffix) && rel.has_target && !rel.target.is_empty() && !rel.external {
            return Ok(pkg.resolve_target("word/document.xml", &rel.target).map(str::to_owned));
        }
    }
    Ok(Some(fallback.to_owned()))
}

// ───────────────────────────── root namespaces ─────────────────────────────

/// Edits the start tag of `root` in `open` (`at = indexOf('<root')`, `end = indexOf('>', at)`;
/// −1 cuts one unit before the end as the TS `slice(at, -1)` does).
fn edit_root_tag(open: &str, root: &str, f: impl FnOnce(&mut String)) -> String {
    let Some(at) = find_from(open, &format!("<{root}"), 0) else { return open.to_owned() };
    let (mut tag, tail) = match find_from(open, ">", at) {
        Some(end) => (open[at..end].to_owned(), open[end..].to_owned()),
        None => split_last_unit(&open[at..]),
    };
    f(&mut tag);
    format!("{}{tag}{tail}", &open[..at])
}

/// `[s.slice(0, -1), s.slice(-1)]`: the last UTF-16 unit split off. A non-BMP last character is
/// cut between its surrogates in the TS, and each half then encodes as U+FFFD (`TextEncoder`):
/// that is what this gives.
#[must_use]
pub fn split_last_unit(s: &str) -> (String, String) {
    match s.chars().next_back() {
        None => (String::new(), String::new()),
        Some(c) if c.len_utf16() == 2 => (format!("{}\u{FFFD}", &s[..s.len() - c.len_utf8()]), "\u{FFFD}".to_owned()),
        Some(c) => (s[..s.len() - c.len_utf8()].to_owned(), c.to_string()),
    }
}

/// `withNamespaces(open, rootName, content)`: the declarations the generated XML uses.
#[must_use]
pub fn with_namespaces(open: &str, root: &str, content: &str) -> String {
    let mut need: Vec<(&str, &str)> = vec![
        ("w", "http://schemas.openxmlformats.org/wordprocessingml/2006/main"),
        ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
        ("wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
    ];
    if content.contains("<w15:") {
        need.push(("w15", "http://schemas.microsoft.com/office/word/2012/wordml"));
    }
    let tests: [(&str, &str, bool); 8] = [
        ("v", "urn:schemas-microsoft-com:vml", content.contains("<v:")),
        ("o", "urn:schemas-microsoft-com:office:office", has_prefixed_attr(content, "o:") || content.contains("<o:")),
        ("w10", "urn:schemas-microsoft-com:office:word", content.contains("<w10:")),
        ("w14", "http://schemas.microsoft.com/office/word/2010/wordml", has_prefixed_attr(content, "w14:") || content.contains("<w14:")),
        ("mc", "http://schemas.openxmlformats.org/markup-compatibility/2006", content.contains("<mc:")),
        (
            "wps",
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
            content.contains("<wps:") || content.contains("Requires=\"wps\""),
        ),
        ("a", "http://schemas.openxmlformats.org/drawingml/2006/main", content.contains("<a:")),
        (
            "wp14",
            "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
            content.contains("<wp14:") || has_prefixed_attr(content, "wp14:"),
        ),
    ];
    for (pfx, uri, used) in tests {
        if used && !content.contains(&format!("xmlns:{pfx}=")) {
            need.push((pfx, uri));
        }
    }
    edit_root_tag(open, root, |tag| {
        for (pfx, uri) in need {
            if !has_xmlns(tag, pfx) {
                tag.push_str(&format!(" xmlns:{pfx}=\"{uri}\""));
            }
        }
    })
}

/// `drawingNsAttrs(content)`: the drawing namespaces a generated part uses.
fn drawing_ns_attrs(content: &str) -> String {
    const DRAWING_NS: &[(&str, &str)] = &[
        ("mc", "http://schemas.openxmlformats.org/markup-compatibility/2006"),
        ("wps", "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"),
        ("wp14", "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing"),
        ("w14", "http://schemas.microsoft.com/office/word/2010/wordml"),
        ("v", "urn:schemas-microsoft-com:vml"),
        ("o", "urn:schemas-microsoft-com:office:office"),
        ("w10", "urn:schemas-microsoft-com:office:word"),
    ];
    DRAWING_NS
        .iter()
        .filter(|(p, _)| content.contains(&format!("<{p}:")) || content.contains(&format!(" {p}:")))
        .map(|(p, u)| format!(" xmlns:{p}=\"{u}\""))
        .collect()
}

impl Writer<'_> {
    /// `revRoot(open, rootName)`: w16du declared when a generated revision used it.
    pub fn rev_root(&mut self, open: &str, root: &str) -> String {
        if self.cx.rev.take_used_date_utc() { super::rev::ensure_revision_namespaces(open, root) } else { open.to_owned() }
    }
}

// ───────────────────────────── relationships of the part being written ─────────────────────────────

impl Writer<'_> {
    fn rels_path(&self) -> WResult<String> {
        Ok(self.lossless.as_ref().ok_or(DocxError::Write("lossless state missing"))?.rels_path.clone())
    }

    /// `linkRelId(url)`: the relationship of an external hyperlink in the current part (an
    /// existing one with the same target reused); `None` outside the lossless save.
    pub fn link_rel_id(&mut self, url: &str) -> WResult<Option<String>> {
        if !self.active() {
            return Ok(None);
        }
        let rels_path = self.rels_path()?;
        let rels = part_text(self.pkg, &rels_path)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
        let want = format!("Target=\"{}\"", esc_attr(url));
        for (a, b) in relationship_tags(&rels) {
            let tag = &rels[a..b];
            if tag.contains("TargetMode=\"External\"") && tag.contains("/hyperlink\"") && tag.contains(&want) {
                return Ok(rel_id_of(tag).map(str::to_owned));
            }
        }
        let mut n = 1u64;
        while rels.contains(&format!("Id=\"rIdWnLink{n}\"")) {
            n += 1;
        }
        let id = format!("rIdWnLink{n}");
        let rel = format!(
            "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/></Relationships>",
            esc_attr(url)
        );
        let rels = js_replace_first(&rels, "</Relationships>", &rel);
        self.pkg.set_part_text(rels_path, rels);
        Ok(Some(id))
    }

    /// `mediaRelId(data, ext, relType)`: the relationship of NEW media in the part being written:
    /// an existing `word/media/` part with the same bytes (full equality, G24 — the TS compares
    /// the length, every 509th byte and the last) and its relationship reused, or
    /// `word/media/uwimageN.ext` with a relationship and its content type. Outside the lossless
    /// save, queued for `document.xml` (`rIdWnImg${n}`).
    pub fn media_rel_id(&mut self, data: &std::sync::Arc<[u8]>, ext: &str, rel_type: Option<&str>) -> WResult<String> {
        let rel_type = rel_type.unwrap_or(REL_IMAGE);
        if !self.active() {
            self.cx.drawing_id += 1;
            let n = self.cx.drawing_id;
            let rid = format!("rIdWnImg{n}");
            self.cx.media_out.push(super::ctx::MediaOut {
                rid: rid.clone(),
                path: format!("word/media/wnimage{n}.{ext}"),
                data: data.clone(),
                ext: ext.to_owned(),
                rel_type: (rel_type != REL_IMAGE).then(|| rel_type.to_owned()),
                mime: None,
            });
            return Ok(rid);
        }
        let rels_path = self.rels_path()?;
        let mut rels = part_text(self.pkg, &rels_path)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
        let names: Vec<String> = self.pkg.part_names().filter(|p| p.starts_with("word/media/")).map(str::to_owned).collect();
        let mut part = None;
        for p in names {
            if self.pkg.part(&p)?.is_some_and(|b| *b == **data) {
                part = Some(p);
                break;
            }
        }
        let part = match part {
            Some(p) => p,
            None => {
                let mut n = 1u64;
                while self.pkg.contains(&format!("word/media/uwimage{n}.{ext}")) {
                    n += 1;
                }
                let p = format!("word/media/uwimage{n}.{ext}");
                self.pkg.set_part(p.clone(), data.to_vec());
                let ct = part_text(self.pkg, CONTENT_TYPES)?.unwrap_or_default();
                let e = p.rsplit_once('.').map_or(p.as_str(), |(_, e)| e).to_owned();
                if !ct.is_empty() && !find_literal_ci(&ct, &format!("Extension=\"{e}\"")) {
                    let mime = media_mime(&uo_jsnum::text::to_lowercase(&e)).map_or_else(|| format!("image/{e}"), str::to_owned);
                    let ct = insert_before_first(&ct, "</Types>", &format!("<Default Extension=\"{e}\" ContentType=\"{mime}\"/>"));
                    self.pkg.set_part_text(CONTENT_TYPES, ct);
                }
                p
            }
        };
        let target =
            if is_word_rels_path(&rels_path) { part.strip_prefix("word/").unwrap_or(&part).to_owned() } else { format!("/{part}") };
        let want_target = format!("Target=\"{target}\"");
        let want_type = format!("Type=\"{rel_type}\"");
        for (a, b) in relationship_tags(&rels) {
            let tag = &rels[a..b];
            if tag.contains(&want_target) && tag.contains(&want_type) {
                return rel_id_of(tag).map(str::to_owned).ok_or(DocxError::Write("a relationship without Id (TS TypeError)"));
            }
        }
        let mut n = 1u64;
        while rels.contains(&format!("Id=\"rIdUwImg{n}\"")) {
            n += 1;
        }
        let id = format!("rIdUwImg{n}");
        rels =
            insert_before_first(&rels, "</Relationships>", &format!("<Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{target}\"/>"));
        self.pkg.set_part_text(rels_path, rels);
        Ok(id)
    }
}

/// `MEDIA_MIME[ext]` (own keys only: a name such as `constructor` reads `Object.prototype` in the
/// TS — accepted by name).
fn media_mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "svg" => "image/svg+xml",
        "wdp" => "image/vnd.ms-photo",
        _ => return None,
    })
}

// ───────────────────────────── comments ─────────────────────────────

/// `commentXml(c)`.
fn comment_xml(c: &Comment) -> String {
    let author = c.author.as_deref().map_or_else(|| "Write".to_owned(), esc_attr);
    format!(
        "<w:comment w:id=\"{}\" w:author=\"{author}\" w:initials=\"WN\"><w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:comment>",
        super::js::num_str(c.id),
        esc_text(&c.text.to_string_lossy())
    )
}

/// The matches of `/<w:comment\b[^>]*?\bw:id="(-?\d+)"[^>]*?(?:\/>|>[\s\S]*?<\/w:comment>)/g`:
/// `(start, end, id text)`.
#[must_use]
pub fn comment_elements(xml: &str) -> Vec<(usize, usize, &str)> {
    const OPEN: &str = "<w:comment";
    const CLOSE: &str = "</w:comment>";
    let mut out = Vec::new();
    let mut from = 0;
    // the queries only grow: memoised (the `>` and `</w:comment>` scans run once in total)
    let mut gts = super::fonts::ooxml::NextFind::new(xml, ">", false);
    let mut closes = super::fonts::ooxml::NextFind::new(xml, CLOSE, false);
    let mut ids = super::fonts::ooxml::NextFind::new(xml, "w:id=\"", false);
    while let Some(i) = find_from(xml, OPEN, from) {
        from = i + 1;
        let pos = i + OPEN.len();
        if !boundary_at(xml, pos) {
            continue;
        }
        let Some(gt) = gts.at(pos) else { break };
        // the lazy `[^>]*?` takes the first `\bw:id="-?\d+"` before the `>`
        let mut id: Option<(usize, usize)> = None;
        let mut k = pos;
        while let Some(p) = ids.at(k) {
            if p >= gt {
                break;
            }
            k = p + 1;
            if !boundary_at(xml, p) {
                continue;
            }
            let v = p + 6;
            let neg = usize::from(xml.as_bytes().get(v) == Some(&b'-'));
            let d = xml[v + neg..].bytes().take_while(u8::is_ascii_digit).count();
            if d > 0 && xml.as_bytes().get(v + neg + d) == Some(&b'"') && v + neg + d < gt {
                id = Some((v, v + neg + d));
                break;
            }
        }
        let Some((a, b)) = id else { continue };
        let end =
            if gt > b + 1 && xml.as_bytes().get(gt - 1) == Some(&b'/') { Some(gt + 1) } else { closes.at(gt + 1).map(|c| c + CLOSE.len()) };
        if let Some(end) = end {
            out.push((i, end, &xml[a..b]));
            from = end;
        }
    }
    out
}

/// `/w14:paraId="([0-9A-Fa-f]+)"/g` values.
pub fn para_ids(m: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = find_from(m, "w14:paraId=\"", from) {
        from = i + 1;
        let v = i + 12;
        let d = m[v..].bytes().take_while(u8::is_ascii_hexdigit).count();
        if d > 0 && m.as_bytes().get(v + d) == Some(&b'"') {
            out.push(&m[v..v + d]);
            from = v + d + 1;
        }
    }
    out
}

/// `patchComments(pkg, comments)`: the deleted comments removed from `comments.xml`, the new ones
/// added, and the deleted ones' entries removed from `commentsIds`/`commentsExtended`/
/// `commentsExtensible` (dynamic patterns: [`super::matchers::TagPattern`]).
fn patch_comments(pkg: &mut Package, comments: &[Comment]) -> WResult<()> {
    let xml = part_text(pkg, "word/comments.xml")?.unwrap_or_default();
    let want: HashSet<NumKey> = comments.iter().map(|c| NumKey::new(c.id.to_number())).collect();
    let mut have: HashSet<NumKey> = HashSet::new();
    let mut gone_para_ids: Vec<String> = Vec::new();
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for (a, b, id) in comment_elements(&xml) {
        let n = NumKey::new(uo_jsnum::parse::parse_int(id, 10));
        out.push_str(&xml[last..a]);
        if want.contains(&n) {
            have.insert(n);
            out.push_str(&xml[a..b]);
        } else {
            gone_para_ids.extend(para_ids(&xml[a..b]).into_iter().map(str::to_owned));
        }
        last = b;
    }
    out.push_str(&xml[last..]);
    let added: String = comments.iter().filter(|c| !have.contains(&NumKey::new(c.id.to_number()))).map(comment_xml).collect();
    if !added.is_empty() {
        out = replace_last_close(&out, "w:comments", &format!("{added}</w:comments>"));
    }
    if out != xml {
        pkg.set_part_text("word/comments.xml", out);
    }
    if gone_para_ids.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = gone_para_ids.iter().map(String::as_str).collect();
    let mut durable: Vec<String> = Vec::new();
    if let Some(cids) = part_text(pkg, "word/commentsIds.xml")?.filter(|t| !t.is_empty()) {
        let pat = super::matchers::TagPattern {
            names: &["w16cid:commentId"],
            name_boundary: true,
            attr: "w16cid:paraId",
            attr_boundary: false,
            values: &ids,
            tail: super::matchers::Tail::SelfClose,
        };
        let new = super::matchers::replace_tags(&cids, &pat, |m| {
            if let Some(d) = durable_id(m) {
                durable.push(d.to_owned());
            }
            String::new()
        });
        pkg.set_part_text("word/commentsIds.xml", new);
    }
    if let Some(ext) = part_text(pkg, "word/commentsExtended.xml")?.filter(|t| !t.is_empty()) {
        let pat = super::matchers::TagPattern {
            names: &["w15:commentEx"],
            name_boundary: true,
            attr: "w15:paraId",
            attr_boundary: false,
            values: &ids,
            tail: super::matchers::Tail::SelfClose,
        };
        pkg.set_part_text("word/commentsExtended.xml", remove_tags(&ext, &pat));
    }
    if let Some(exb) = part_text(pkg, "word/commentsExtensible.xml")?.filter(|t| !t.is_empty())
        && !durable.is_empty()
    {
        let ds: Vec<&str> = durable.iter().map(String::as_str).collect();
        let pat = super::matchers::TagPattern {
            names: &["w16cex:commentExtensible"],
            name_boundary: true,
            attr: "w16cex:durableId",
            attr_boundary: false,
            values: &ds,
            tail: super::matchers::Tail::SelfCloseOrElement("w16cex:commentExtensible"),
        };
        pkg.set_part_text("word/commentsExtensible.xml", remove_tags(&exb, &pat));
    }
    Ok(())
}

/// `/w16cid:durableId="([^"]+)"/.exec(m)?.[1]`.
pub fn durable_id(m: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(i) = find_from(m, "w16cid:durableId=\"", from) {
        from = i + 1;
        let v = &m[i + 18..];
        if let Some(q) = v.find('"')
            && q > 0
        {
            return Some(&v[..q]);
        }
    }
    None
}

// ───────────────────────────── numbering ─────────────────────────────

/// `numberingAbstracts(ids)`: the two abstract lists.
pub fn numbering_abstracts(ids: GenNumIds) -> String {
    let n = uo_jsnum::format::to_js_string;
    let mut bullet = String::new();
    let fonts = ["Symbol", "Courier New", "Wingdings"];
    let texts = ["\u{f0b7}", "o", "\u{f0a7}"];
    for i in 0..3u32 {
        let (t, f) = (texts[i as usize], fonts[i as usize]);
        bullet.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{t}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:hint=\"default\"/></w:rPr></w:lvl>",
            720 * (i + 1)
        ));
    }
    let mut decimal = String::new();
    for i in 0..3u32 {
        decimal.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            i + 1,
            720 * (i + 1)
        ));
    }
    format!(
        "<w:abstractNum w:abstractNumId=\"{}\">{bullet}</w:abstractNum><w:abstractNum w:abstractNumId=\"{}\">{decimal}</w:abstractNum>",
        n(ids.bullet),
        n(ids.decimal)
    )
}

/// `numberingNums(ids)`: the two `w:num`.
pub fn numbering_nums(ids: GenNumIds) -> String {
    let (b, d) = (uo_jsnum::format::to_js_string(ids.bullet), uo_jsnum::format::to_js_string(ids.decimal));
    format!(
        "<w:num w:numId=\"{b}\"><w:abstractNumId w:val=\"{b}\"/></w:num><w:num w:numId=\"{d}\"><w:abstractNumId w:val=\"{d}\"/></w:num>"
    )
}

/// `numberingXml(ids)`: a new numbering part with the two lists.
#[must_use]
pub fn numbering_xml(ids: GenNumIds) -> String {
    format!("{XML_DECL}<w:numbering {W_NS}>{}{}</w:numbering>", numbering_abstracts(ids), numbering_nums(ids))
}

/// `mergeNumbering(xml, ids)`: the two lists added to an existing numbering part (idempotent).
#[must_use]
pub fn merge_numbering(xml: &str, ids: GenNumIds) -> String {
    if xml.contains(&format!("<w:num w:numId=\"{}\">", uo_jsnum::format::to_js_string(ids.bullet))) {
        return xml.to_owned();
    }
    let first_num = first_tag(xml, "<w:num");
    let abstracts = numbering_abstracts(ids);
    let with_abs = match first_num {
        Some(i) => format!("{}{abstracts}{}", &xml[..i], &xml[i..]),
        None => replace_last_close(xml, "w:numbering", &format!("{abstracts}</w:numbering>")),
    };
    // /<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/: the leftmost of the two
    let cleanup = first_tag(&with_abs, "<w:numIdMacAtCleanup");
    let close = super::fonts::ooxml::last_close_trimmed(&with_abs, "w:numbering");
    let tail = match (cleanup, close) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let nums = numbering_nums(ids);
    match tail {
        Some(t) => format!("{}{nums}{}", &with_abs[..t], &with_abs[t..]),
        // search() = −1: slice(0, -1) + nums + slice(-1)
        None => {
            let (head, tail) = split_last_unit(&with_abs);
            format!("{head}{nums}{tail}")
        }
    }
}

/// The first `<name\b` (a `search` of the tag name followed by a non-word character).
fn first_tag(s: &str, open: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = find_from(s, open, from) {
        if boundary_at(s, i + open.len()) {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

/// `writeNumberingPart(pkg, xml)`: the numbering part (the one the document's relationship names,
/// when it exists under another name), its relationship and content type.
pub fn write_numbering_part(pkg: &mut Package, xml: String) -> WResult<()> {
    let at = part_by_rel_type(pkg, "numbering", "word/numbering.xml")?;
    if let Some(at) = at.filter(|a| a != "word/numbering.xml")
        && pkg.contains(&at)
    {
        pkg.set_part_text(at, xml);
        return Ok(());
    }
    pkg.set_part_text("word/numbering.xml", xml);
    let rels = part_text(pkg, DOC_RELS)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
    if !rels.contains("numbering.xml") {
        let rels = js_replace_first(
            &rels,
            "</Relationships>",
            "<Relationship Id=\"rIdWnNum\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>",
        );
        pkg.set_part_text(DOC_RELS, rels);
    }
    if let Some(ct) = part_text(pkg, CONTENT_TYPES)?.filter(|t| !t.is_empty())
        && !ct.contains("numbering+xml")
    {
        let ct = js_replace_first(
            &ct,
            "</Types>",
            "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
        );
        pkg.set_part_text(CONTENT_TYPES, ct);
    }
    Ok(())
}

// ───────────────────────────── options and entry ─────────────────────────────

/// A header/footer part to reference and, when dirty or new, to write (`HfPartInfo`).
#[derive(Debug, Clone)]
pub struct HfPartOut<'a> {
    /// header | footer
    pub kind: Str,
    /// default | first | even
    pub r#type: Str,
    pub rid: Str,
    pub path: Str,
    /// the part's content (`blocks`, or the paragraphs as blocks)
    pub story: &'a Story,
    /// only dirty parts are rewritten
    pub dirty: bool,
}

/// The notes of one kind (`NotesOut`): each note's content and the ids referenced in the body in
/// document order.
#[derive(Debug, Clone, Default)]
pub struct NotesOut<'a> {
    pub blocks: IndexMap<NumKey, &'a Story>,
    pub used: Vec<f64>,
}

/// `WriteOptions` of `writeDocument`.
#[derive(Debug, Clone, Default)]
pub struct WriteOptions<'a> {
    pub comments: Option<&'a [Comment]>,
    /// the header/footer references of the LAST section (the body `w:sectPr`); absent = all
    /// the parts
    pub body_sect_refs: Option<Vec<HfRefXml>>,
    /// `trackRevisions` (absent = untouched)
    pub track_revisions: Option<bool>,
    pub footnotes: Option<NotesOut<'a>>,
    pub endnotes: Option<NotesOut<'a>>,
    /// whether `opts.notes` was given (the TS creates the note parts only then)
    pub notes: bool,
    /// `opts.blocks` was given (the body as blocks); `false` writes the top-level paragraphs only
    /// (`serializeDocument`, the path of a document without tables or sources)
    pub blocks: bool,
}

/// `writeDocument(pkg, model, hfParts, opts)`: writes `doc` into `pkg` — `body` and `sect_pr`
/// are the TS `model` (`opts.blocks` the body's blocks), `save` what outlives one save. With
/// `doc.sources` the save is lossless. Errors are the package's, the writer's (`Write`) and the
/// raw XML parses'.
pub fn write_document(
    pkg: &mut Package,
    doc: &Document,
    body: &Story,
    sect_pr: &SectPr,
    hf_parts: &[HfPartOut<'_>],
    opts: &WriteOptions<'_>,
    save: &mut SaveState,
) -> Result<(), DocxError> {
    let limits = pkg.limits().clone();
    let mut cx = WriteCtx::new(limits, save.baseline.mode);
    // compatibility mode ≤ 12: rebuilt shapes in VML only
    let settings = match part_text(pkg, "word/settings.xml")?.filter(|t| !t.is_empty()) {
        Some(t) => crate::read::parts::read_settings_text(&t)?,
        None => crate::read::parts::read_settings(None),
    };
    cx.vml_only = settings.compatibility_mode.to_number() <= 12.0;
    let theme = match part_text(pkg, "word/theme/theme1.xml")?.filter(|t| !t.is_empty()) {
        Some(t) => crate::read::parts::read_theme_text(&t)?,
        None => crate::read::parts::read_theme(None)?,
    };
    for (k, v) in theme.colors.iter() {
        cx.shape_theme.insert(Str::from(k), v.clone());
    }
    let store = doc.sources.as_deref();
    if store.is_some() && save.gen_num_ids.is_none() {
        let (b, d) = super::matchers::free_num_ids(part_text(pkg, "word/numbering.xml")?.as_deref());
        save.gen_num_ids = Some(GenNumIds { bullet: b, decimal: d });
    }
    // the ids of the new revisions: above the ones the package uses
    {
        let mut max = 0.0f64;
        for n in pkg.part_names().filter(|p| is_rev_part(p)) {
            if let Some(t) = pkg.part_text(n)? {
                max = uo_jsnum::math::max(max, super::matchers::max_w_id(&[&t]));
            }
        }
        cx.rev.next_id = max + 1.0;
        cx.rev.used_utc = false;
    }
    let num_ids = save.gen_num_ids.unwrap_or(DEFAULT_NUM_IDS);
    let lossless = store.map(|s| Lossless {
        store: s,
        pristine: Pristine::new(&save.baseline, Some(&doc.media)),
        rels_path: DOC_RELS.to_owned(),
        claimed: HashSet::new(),
        gen_numbering: false,
        num_ids,
        claimed_segs: HashSet::new(),
        remaining: std::collections::HashMap::new(),
        para_runs: None,
        emitted_obj: HashSet::new(),
    });
    let mut w = Writer { doc, pkg, cx, fresh_id: &mut save.fresh_id, lossless };
    w.write_parts(body, sect_pr, hf_parts, opts)?;
    if let Some(on) = opts.track_revisions {
        let by_rel = part_by_rel_type(w.pkg, "settings", "")?.filter(|p| !p.is_empty());
        super::rev::write_track_revisions(w.pkg, by_rel.as_deref(), on)?;
    }
    Ok(())
}

/// `/^word\/(?:document|header\d*|footer\d*|footnotes|endnotes|comments)\.xml$/`.
fn is_rev_part(p: &str) -> bool {
    let Some(stem) = p.strip_prefix("word/").and_then(|r| r.strip_suffix(".xml")) else { return false };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    matches!(stem, "document" | "footnotes" | "endnotes" | "comments")
        || stem.strip_prefix("header").is_some_and(digits)
        || stem.strip_prefix("footer").is_some_and(digits)
}

/// `/^word\/(?:header\d*|footer\d*|footnotes|endnotes)\.xml$/`.
fn is_marks_part(p: &str) -> bool {
    is_rev_part(p) && !matches!(p, "word/document.xml" | "word/comments.xml")
}

impl<'a> Writer<'a> {
    fn gone_strings(gone: &[f64]) -> Vec<String> {
        gone.iter().map(|&x| uo_jsnum::format::to_js_string(x)).collect()
    }

    fn drop_marks(xml: String, gone: &[String]) -> String {
        if gone.is_empty() {
            return xml;
        }
        let ids: Vec<&str> = gone.iter().map(String::as_str).collect();
        drop_comment_marks(&xml, &ids)
    }

    /// `open + body + close` in the body's own buffer (its capacity has room for them).
    fn wrap_in_place(mut body: String, open: &str, close: &str) -> String {
        body.reserve(open.len() + close.len());
        body.insert_str(0, open);
        body.push_str(close);
        body
    }

    /// `open + body + close` in one buffer.
    fn join3(open: &str, body: &str, close: &str) -> String {
        let mut x = String::with_capacity(open.len() + body.len() + close.len());
        x.push_str(open);
        x.push_str(body);
        x.push_str(close);
        x
    }

    /// `writeParts(pkg, model, hfParts, opts, store)`.
    fn write_parts(&mut self, body: &'a Story, sect_pr: &SectPr, hf_parts: &[HfPartOut<'a>], opts: &WriteOptions<'a>) -> WResult<()> {
        let store = self.store();
        // comments deleted since the open: their raw marks go
        let mut gone: Vec<f64> = Vec::new();
        if let Some(s) = store {
            let now: HashSet<NumKey> = opts.comments.unwrap_or(&[]).iter().map(|c| NumKey::new(c.id.to_number())).collect();
            for k in &s.comment_ids.0 {
                if !now.contains(k) {
                    gone.push(k.get());
                }
            }
        }
        let gone_s = Self::gone_strings(&gone);
        let comments_json = comments_sig(opts.comments.unwrap_or(&[]));
        let unchanged = store.is_some_and(|s| s.comments_sig.as_deref() == Some(comments_json.as_str()));
        if store.is_some() && !unchanged && self.pkg.contains("word/comments.xml") {
            patch_comments(self.pkg, opts.comments.unwrap_or(&[]))?;
        } else if !unchanged && opts.comments.is_some_and(|c| !c.is_empty()) {
            let body_xml: String = opts.comments.unwrap_or(&[]).iter().map(comment_xml).collect();
            self.pkg.set_part_text("word/comments.xml", format!("{XML_DECL}<w:comments {W_NS_HF}>{body_xml}</w:comments>"));
            let rels = part_text(self.pkg, DOC_RELS)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
            if !rels.contains("comments.xml") {
                let rels = js_replace_first(
                    &rels,
                    "</Relationships>",
                    "<Relationship Id=\"rIdWnComments\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"comments.xml\"/></Relationships>",
                );
                self.pkg.set_part_text(DOC_RELS, rels);
            }
            if let Some(ct) = part_text(self.pkg, CONTENT_TYPES)?.filter(|t| !t.is_empty())
                && !ct.contains("comments+xml")
            {
                let ct = js_replace_first(
                    &ct,
                    "</Types>",
                    "<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/></Types>",
                );
                self.pkg.set_part_text(CONTENT_TYPES, ct);
            }
        }

        // headers and footers, before the document
        if !hf_parts.is_empty() {
            let mut rels = part_text(self.pkg, DOC_RELS)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
            let mut ct = part_text(self.pkg, CONTENT_TYPES)?.unwrap_or_default();
            for hf in hf_parts {
                if hf.dirty || !self.pkg.contains(&hf.path) {
                    let story = store.and_then(|s| s.stories.get(hf.path.as_ref()));
                    match (self.active(), story) {
                        (true, Some(story)) => {
                            let file = hf.path.rsplit('/').next().unwrap_or(&hf.path);
                            if let Some(l) = self.lossless.as_mut() {
                                l.rels_path = format!("word/_rels/{file}.rels");
                            }
                            let blocks: Vec<Block<'a>> = hf.story.blocks().collect();
                            let root = if hf.kind.as_ref() == "header" { "w:hdr" } else { "w:ftr" };
                            self.cx.rev.take_used_date_utc();
                            emit::object::begin_shape_context(self, &blocks)?;
                            let body_xml = self.story_blocks_xml(story, &blocks)?;
                            let open = with_namespaces(self.src_text(&story.open), root, &body_xml);
                            let open = self.rev_root(&open, root);
                            let xml = Self::wrap_in_place(body_xml, &open, self.src_text(&story.close));
                            let xml = Self::drop_marks(xml, &gone_s);
                            set_text(self.pkg, &hf.path, xml)?;
                            if let Some(l) = self.lossless.as_mut() {
                                l.rels_path = DOC_RELS.to_owned();
                            }
                        }
                        _ => {
                            let xml = self.serialize_hf_part(&hf.kind, hf.story)?;
                            self.pkg.set_part_text(hf.path.to_string(), xml);
                        }
                    }
                }
                let rel_target = hf.path.strip_prefix("word/").unwrap_or(&hf.path);
                if !rels.contains(&format!("Id=\"{}\"", hf.rid)) {
                    rels = js_replace_first(
                        &rels,
                        "</Relationships>",
                        &format!(
                            "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{}\" Target=\"{rel_target}\"/></Relationships>",
                            hf.rid, hf.kind
                        ),
                    );
                }
                let ct_type = format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{}+xml", hf.kind);
                if !ct.is_empty() && !ct.contains(&format!("PartName=\"/{}\"", hf.path)) {
                    ct = js_replace_first(
                        &ct,
                        "</Types>",
                        &format!("<Override PartName=\"/{}\" ContentType=\"{ct_type}\"/></Types>", hf.path),
                    );
                }
            }
            set_text(self.pkg, DOC_RELS, rels)?;
            if !ct.is_empty() {
                set_text(self.pkg, CONTENT_TYPES, ct)?;
            }
        }

        // notes (before the body, as the headers)
        if opts.notes {
            let any =
                opts.footnotes.as_ref().is_some_and(|n| !n.used.is_empty()) || opts.endnotes.as_ref().is_some_and(|n| !n.used.is_empty());
            if any && !self.pkg.contains("word/footnotes.xml") {
                emit::notes::ensure_note_parts(self.pkg)?;
            }
            if let Some(n) = opts.footnotes.as_ref() {
                self.write_notes_part("footnote", n, &gone_s)?;
            }
            if let Some(n) = opts.endnotes.as_ref() {
                self.write_notes_part("endnote", n, &gone_s)?;
            }
        }

        let body_refs: Vec<HfRefXml> = match opts.body_sect_refs.as_ref() {
            Some(r) => r.clone(),
            None => hf_parts.iter().map(|h| HfRefXml { kind: h.kind.clone(), r#type: h.r#type.clone(), rid: h.rid.clone() }).collect(),
        };
        let story = store.and_then(|s| s.stories.get("word/document.xml"));
        let blocks: Vec<Block<'a>> =
            if opts.blocks || store.is_some() { body.blocks().collect() } else { body.paras.iter().map(Block::Paragraph).collect() };
        match (self.active(), story) {
            (true, Some(story)) => {
                if let Some(l) = self.lossless.as_mut() {
                    l.rels_path = DOC_RELS.to_owned();
                }
                let sect = match story.sect_pr.as_ref() {
                    Some(raw) => {
                        let raw = self.src_text(raw);
                        patch_sect_pr(&mut self.cx, raw, sect_pr, Some(&body_refs))?
                    }
                    None => format!("<w:sectPr>{}</w:sectPr>", emit::run::sect_pr_inner(&self.cx, sect_pr, &body_refs)?),
                };
                self.cx.rev.take_used_date_utc();
                emit::object::begin_shape_context(self, &blocks)?;
                let mut body_xml = self.story_blocks_xml(story, &blocks)?;
                body_xml.push_str(&sect);
                let open = with_namespaces(self.src_text(&story.open), "w:document", &body_xml);
                let open = self.rev_root(&open, "w:document");
                let xml = Self::wrap_in_place(body_xml, &open, self.src_text(&story.close));
                let xml = Self::drop_marks(xml, &gone_s);
                set_text(self.pkg, "word/document.xml", xml)?;
            }
            _ => {
                let xml = self.serialize_blocks(&blocks, sect_pr, &body_refs)?;
                self.pkg.set_part_text("word/document.xml", xml);
            }
        }

        // deleted comments: the marks leave the headers/footers not rewritten and the notes too
        if !gone.is_empty() {
            let names: Vec<String> = self.pkg.part_names().filter(|p| is_marks_part(p)).map(str::to_owned).collect();
            for part in names {
                let Some(x) = part_text(self.pkg, &part)? else { continue };
                let y = Self::drop_marks(x.clone(), &gone_s);
                if y != x {
                    self.pkg.set_part_text(part, y);
                }
            }
        }

        if !self.cx.media_out.is_empty() {
            self.flush_media()?;
        }

        let has_lists = match self.lossless.as_ref() {
            Some(l) => l.gen_numbering,
            None => body.paras.iter().any(|p| p.props.numbering.as_ref().is_some_and(|n| n.num_id.is_none())),
        };
        if !has_lists {
            return Ok(());
        }
        let existing = part_text(self.pkg, "word/numbering.xml")?.filter(|t| !t.is_empty());
        let ids = self.lossless.as_ref().map_or(DEFAULT_NUM_IDS, |l| l.num_ids);
        let xml = match existing {
            Some(x) => merge_numbering(&x, ids),
            None => numbering_xml(ids),
        };
        write_numbering_part(self.pkg, xml)
    }

    /// The queued media of the generated body (`mediaOut`), deduplicated against the media
    /// already in the package by full bytes (G24).
    fn flush_media(&mut self) -> WResult<()> {
        let mut rels = part_text(self.pkg, DOC_RELS)?.unwrap_or_else(|| EMPTY_RELS.to_owned());
        let mut ct = part_text(self.pkg, CONTENT_TYPES)?.unwrap_or_default();
        let existing: Vec<String> = self.pkg.part_names().filter(|p| p.starts_with("word/media/")).map(str::to_owned).collect();
        let mut exts: Vec<String> = Vec::new();
        let mut mimes: IndexMap<String, String> = IndexMap::new();
        let media = std::mem::take(&mut self.cx.media_out);
        for m in &media {
            let mut target = m.path.strip_prefix("word/").unwrap_or(&m.path).to_owned();
            let mut dup = None;
            if m.rel_type.is_none() {
                for p in &existing {
                    if self.pkg.part(p)?.is_some_and(|b| *b == *m.data) {
                        dup = Some(p.clone());
                        break;
                    }
                }
            }
            match dup {
                Some(d) => target = d.strip_prefix("word/").unwrap_or(&d).to_owned(),
                None => self.pkg.set_part(m.path.clone(), m.data.to_vec()),
            }
            if !rels.contains(&format!("\"{}\"", m.rid)) {
                rels = js_replace_first(
                    &rels,
                    "</Relationships>",
                    &format!(
                        "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{target}\"/></Relationships>",
                        m.rid,
                        m.rel_type.as_deref().unwrap_or(REL_IMAGE)
                    ),
                );
            }
            if !exts.contains(&m.ext) {
                exts.push(m.ext.clone());
            }
            if let Some(mime) = m.mime.as_ref() {
                mimes.insert(m.ext.clone(), mime.clone());
            }
        }
        for ext in &exts {
            let mime = mimes.get(ext).cloned().unwrap_or_else(|| match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg".to_owned(),
                "emf" => "image/x-emf".to_owned(),
                e => format!("image/{e}"),
            });
            if !ct.is_empty() && !ct.contains(&format!("Extension=\"{ext}\"")) {
                ct = js_replace_first(&ct, "</Types>", &format!("<Default Extension=\"{ext}\" ContentType=\"{mime}\"/></Types>"));
            }
        }
        self.pkg.set_part_text(DOC_RELS, rels);
        if !ct.is_empty() {
            self.pkg.set_part_text(CONTENT_TYPES, ct);
        }
        Ok(())
    }

    /// `writeNotesPart(pkg, kind, notes, store, gone)`: separators and untouched notes raw, an
    /// edited note rewritten through its story, a note whose reference is gone dropped (an orphan
    /// already in the file stays), new notes at the end. Unchanged, the part stays byte for byte.
    fn write_notes_part(&mut self, kind: &str, notes: &NotesOut<'a>, gone: &[String]) -> WResult<()> {
        let path = format!("word/{kind}s.xml");
        let xml0 = part_text(self.pkg, &path)?.filter(|t| !t.is_empty());
        let used: HashSet<NumKey> = notes.used.iter().map(|&x| NumKey::new(x)).collect();
        if xml0.is_none() && used.is_empty() {
            return Ok(());
        }
        let store = self.store();
        let part_src = store.and_then(|s| s.note_parts.get(path.as_str()));
        // a part created in this session (or a document without sources): raw elements, no story
        struct Item<'s> {
            id: f64,
            raw: Cow<'s, str>,
            separator: bool,
            story: Option<&'s crate::model::sources::StorySource>,
        }
        let (open, close, items, refs_at_load): (String, String, Vec<Item<'a>>, HashSet<NumKey>) = match part_src {
            Some(p) => (
                self.src_text(&p.open).to_owned(),
                self.src_text(&p.close).to_owned(),
                p.items
                    .iter()
                    .map(|it| Item {
                        id: it.id.to_number(),
                        raw: Cow::Borrowed(self.src_text(&it.raw)),
                        separator: it.separator == Some(true),
                        story: it.story.as_ref(),
                    })
                    .collect(),
                p.refs_at_load.0.iter().copied().collect(),
            ),
            None => {
                let xml = xml0.clone().unwrap_or_else(|| emit::notes::empty_notes_part(kind));
                let doc = uo_opc::xml::parse(xml.as_str(), &self.cx.limits.bounded(&crate::read::DOCX_POS))?;
                let root = doc.root();
                let rp = root.positions().ok_or(DocxError::Write("notes root without positions"))?;
                let tag = format!("w:{kind}");
                let items = root
                    .children_named(&tag)
                    .filter_map(|e| {
                        let p = e.positions()?;
                        let id = uo_jsnum::parse::parse_int(e.attr("w:id").unwrap_or("0"), 10);
                        let separator = e.attr("w:type").is_some_and(|t| !t.is_empty() && t != "normal");
                        Some(Item { id, raw: Cow::Owned(xml[p.start..p.end].to_owned()), separator, story: None })
                    })
                    .collect();
                (xml[..rp.content_start].to_owned(), xml[rp.content_end..].to_owned(), items, HashSet::new())
            }
        };
        let rels_prev = self.lossless.as_ref().map(|l| l.rels_path.clone());
        if let Some(l) = self.lossless.as_mut() {
            l.rels_path = format!("word/_rels/{kind}s.xml.rels");
        }
        let r = (|| -> WResult<String> {
            self.cx.rev.take_used_date_utc();
            let mut seen: HashSet<NumKey> = HashSet::new();
            let mut body = String::new();
            for it in &items {
                if it.separator {
                    body.push_str(&it.raw);
                    continue;
                }
                let key = NumKey::new(it.id);
                seen.insert(key);
                if !used.contains(&key) {
                    if !refs_at_load.contains(&key) {
                        body.push_str(&it.raw);
                    }
                    continue;
                }
                let Some(story) = notes.blocks.get(&key).copied() else {
                    body.push_str(&it.raw);
                    continue;
                };
                match (it.story, self.active()) {
                    (Some(src_story), true) => {
                        let blocks: Vec<Block<'a>> = story.blocks().collect();
                        if self.story_pristine(src_story, &blocks)? {
                            body.push_str(&it.raw);
                        } else {
                            let inner = self.story_blocks_xml(src_story, &blocks)?;
                            body.push_str(self.src_text(&src_story.open));
                            body.push_str(&inner);
                            body.push_str(self.src_text(&src_story.close));
                        }
                    }
                    _ => body.push_str(&self.note_xml(kind, it.id, story)?),
                }
            }
            for &id in &notes.used {
                let key = NumKey::new(id);
                if !seen.contains(&key)
                    && let Some(story) = notes.blocks.get(&key).copied()
                {
                    body.push_str(&self.note_xml(kind, id, story)?);
                }
            }
            let open = self.rev_root(&open, &format!("w:{kind}s"));
            Ok(Self::drop_marks(Self::join3(&open, &body, &close), gone))
        })();
        if let (Some(l), Some(prev)) = (self.lossless.as_mut(), rels_prev) {
            l.rels_path = prev;
        }
        let out = r?;
        if xml0.as_deref() != Some(out.as_str()) {
            self.pkg.set_part_text(path, out);
        }
        Ok(())
    }

    /// `serializeBlocks(blocks, sectPr, hfRefs)` / `serializeDocument`: a document without
    /// sources.
    fn serialize_blocks(&mut self, blocks: &[Block<'a>], sect_pr: &SectPr, refs: &[HfRefXml]) -> WResult<String> {
        let sect = format!("<w:sectPr>{}</w:sectPr>", emit::run::sect_pr_inner(&self.cx, sect_pr, refs)?);
        emit::object::begin_shape_context(self, blocks)?;
        let body = self.blocks_xml(blocks)? + &sect;
        let w15 = if body.contains("<w15:") { W15_NS } else { "" };
        let open = self.rev_root(&format!("<w:document {W_NS}{w15}{}>", drawing_ns_attrs(&body)), "w:document");
        Ok(format!("{XML_DECL}{open}<w:body>{body}</w:body></w:document>"))
    }

    /// `serializeHfPart(kind, paragraphs)`: a new header/footer part (the paragraphs of `story`).
    fn serialize_hf_part(&mut self, kind: &str, story: &'a Story) -> WResult<String> {
        let tag = if kind == "header" { "w:hdr" } else { "w:ftr" };
        self.cx.rev.take_used_date_utc();
        // the TS `paragraphs` of a header: the paragraphs of its blocks, cells included
        let paras = story.flat_paragraphs();
        let blocks: Vec<Block<'a>> = paras.iter().map(|p| Block::Paragraph(p)).collect();
        emit::object::begin_shape_context(self, &blocks)?;
        let mut body = String::new();
        for p in paras {
            body.push_str(&self.para_xml(p)?);
        }
        let open = self.rev_root(&format!("<{tag} {}>", emit::settings::HF_NS), tag);
        Ok(format!("{XML_DECL}{open}{body}</{tag}>"))
    }
}

/// `JSON.stringify(comments)` as the reader recorded it (`commentsSig`).
fn comments_sig(comments: &[Comment]) -> String {
    use crate::js::{JsValue, JsonWriter};
    let mut w = JsonWriter::new(None);
    w.raw("[");
    for (i, c) in comments.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        c.json(&mut w);
    }
    w.raw("]");
    w.finish()
}
