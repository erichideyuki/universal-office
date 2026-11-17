//! Save with the "fonts in the file" option (frontier B8): `deckFontUsage` of `universal-slides.ts`
//! and the `.pptx` side of `shared/fonts/src/ooxml.ts` (`readPptxEmbedding`, `writePptxEmbedding`),
//! over the part store. The plan itself (what to embed, subsets) is `uo_fonts::plan_font_embedding`
//! (Write's port, app-neutral); the host answers its face requests with the font files
//! (`uo-docx.md` "D1b - font embedding", the same two-phase protocol).
//!
//! The helpers of `ooxml.ts` are not the ones of `deck-edit.ts` (they scan with their own regexps and
//! number relations as `size + 1`): they are ported separately, as written.

use rustc_hash::FxHashSet;
use uo_fonts::{EmbeddingPlan, FaceToEmbed, FontEmbeddingSettings, FontUsage, Variant};
use uo_jsnum::text::{is_js_space, is_js_word, js_space_len};

use super::pkg::{E, EditError, PRES, XML_DECL};
use super::xml::escape_attr;
use crate::package::PartStore;

const REL_FONT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font";
const CT_FNTDATA: &str = "application/x-fontdata";
/// Elements that come AFTER `p:embeddedFontLst` in `CT_Presentation`.
const PRES_AFTER: [&str; 7] =
    ["p:custShowLst", "p:photoAlbum", "p:custDataLst", "p:kinsoku", "p:defaultTextStyle", "p:modifyVerifier", "p:extLst"];

/// What the file says about the option (`readPptxEmbedding().settings`): only what is stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StoredEmbedding {
    pub embed: Option<bool>,
    pub subset: Option<bool>,
}

impl StoredEmbedding {
    /// `{ ...defaults, ...stored }`.
    pub fn over(&self, d: FontEmbeddingSettings) -> FontEmbeddingSettings {
        FontEmbeddingSettings {
            embed: self.embed.unwrap_or(d.embed),
            subset: self.subset.unwrap_or(d.subset),
            skip_common_fonts: d.skip_common_fonts,
        }
    }
}

fn unesc_attr(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&apos;", "'").replace("&amp;", "&")
}

/// The first `<p:presentation\b[^>]*>` of the text: (start, end).
fn root_tag(xml: &str) -> Option<(usize, usize)> {
    let b = xml.as_bytes();
    let name = "<p:presentation";
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(name)).map(|p| from + p) {
        from = p + 1;
        let after = p + name.len();
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        if let Some(g) = b[after..].iter().position(|&c| c == b'>') {
            return Some((p, after + g + 1));
        }
    }
    None
}

/// `/^(1|true|on)$/i.test(new RegExp('\\b' + name + '="([^"]*)"').exec(root)?.[1] ?? '')`.
fn flag(root: &str, name: &str) -> bool {
    let b = root.as_bytes();
    let lit = format!("{name}=\"");
    let mut from = 0;
    while let Some(p) = root.get(from..).and_then(|s| s.find(&lit)).map(|p| from + p) {
        from = p + 1;
        if p > 0 && is_js_word(b[p - 1]) {
            continue;
        }
        let vs = p + lit.len();
        let Some(len) = b[vs..].iter().position(|&c| c == b'"') else { continue };
        let v = &root[vs..vs + len];
        return v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on");
    }
    false
}

/// `readPptxEmbedding`: the stored option and the families already embedded (lower case).
pub fn read_pptx_embedding(store: &PartStore) -> (StoredEmbedding, FxHashSet<String>) {
    let xml = store.text(PRES).unwrap_or_default();
    let root = root_tag(&xml).map_or("", |(s, e)| &xml[s..e]);
    let mut embedded = FxHashSet::default();
    // /<p:embeddedFont>\s*<p:font\b[^>]*typeface="([^"]*)"/g
    let b = xml.as_bytes();
    let head = "<p:embeddedFont>";
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(head)).map(|p| from + p) {
        from = p + 1;
        let mut i = p + head.len();
        while let n @ 1.. = js_space_len(b, i) {
            i += n;
        }
        let Some(rest) = xml.get(i..).and_then(|r| r.strip_prefix("<p:font")) else { continue };
        let at = xml.len() - rest.len();
        if rest.as_bytes().first().is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        let gt = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..gt];
        if let Some(t) = tag.rfind("typeface=\"") {
            let vs = t + 10;
            if let Some(len) = tag[vs..].find('"') {
                embedded.insert(uo_jsnum::text::to_lowercase(&unesc_attr(&tag[vs..vs + len])));
                from = at + vs + len;
            }
        }
    }
    if !flag(root, "embedTrueTypeFonts") {
        return (StoredEmbedding { embed: Some(false), subset: None }, embedded);
    }
    (StoredEmbedding { embed: Some(true), subset: Some(flag(root, "saveSubsetFonts")) }, embedded)
}

/// `unescXml` of `universal-slides.ts` (the entities and numeric references of the TS regexp).
fn unesc_xml(s: &str) -> String {
    super::xml::unescape_xml(s)
}

/// `deckFontUsage`: the families the deck names (slides/layouts/masters) and those of the theme, each
/// with the text of the whole deck (a little larger than needed, never short of a character).
pub fn deck_font_usage(store: &PartStore) -> Vec<(String, String)> {
    let mut families: Vec<String> = Vec::new();
    let add = |families: &mut Vec<String>, f: String| {
        if !families.contains(&f) {
            families.push(f);
        }
    };
    for f in crate::fonts::collect_deck_typefaces(store) {
        add(&mut families, f.into_string());
    }
    let mut text = String::new();
    for part in store.names() {
        if !is_usage_part(part) {
            continue;
        }
        let Some(xml) = store.text(part).filter(|x| !x.is_empty()) else { continue };
        if part.starts_with("ppt/theme/") {
            for f in theme_latin_faces(&xml) {
                add(&mut families, unesc_xml(&f));
            }
            continue;
        }
        let mut rest = xml.as_str();
        while let Some(i) = rest.find("<a:t>") {
            let after = &rest[i + 5..];
            let Some(end) = after.find('<') else { break };
            if after[end..].starts_with("</a:t>") {
                text.push_str(&unesc_xml(&after[..end]));
                rest = &after[end + 6..];
            } else {
                rest = &after[end..];
            }
        }
    }
    families.into_iter().filter(|f| !f.is_empty() && !f.starts_with('+')).map(|f| (f, text.clone())).collect()
}

/// `/^ppt\/(slides|slideLayouts|slideMasters|theme)\/[^/]+\.xml$/`.
fn is_usage_part(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ppt/") else { return false };
    for dir in ["slides/", "slideLayouts/", "slideMasters/", "theme/"] {
        if let Some(file) = rest.strip_prefix(dir) {
            return file.len() > 4 && file.ends_with(".xml") && !file.contains('/');
        }
    }
    false
}

/// `/<a:(?:major|minor)Font>\s*<a:latin\b[^>]*typeface="([^"]+)"/g`.
fn theme_latin_faces(xml: &str) -> Vec<String> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while from < xml.len() {
        let next = ["<a:majorFont>", "<a:minorFont>"].iter().filter_map(|h| xml[from..].find(h).map(|p| (from + p, h.len()))).min();
        let Some((p, hl)) = next else { break };
        from = p + 1;
        let mut i = p + hl;
        while let n @ 1.. = js_space_len(b, i) {
            i += n;
        }
        let Some(rest) = xml.get(i..).and_then(|r| r.strip_prefix("<a:latin")) else { continue };
        if rest.as_bytes().first().is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        let gt = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..gt];
        // greedy `[^>]*` then `typeface="`: the last one with a value of at least one character
        let mut cut = tag.len();
        while let Some(t) = tag[..cut].rfind("typeface=\"") {
            let vs = t + 10;
            if let Some(len) = tag[vs..].find('"').filter(|l| *l > 0) {
                out.push(tag[vs..vs + len].to_owned());
                from = i + 7 + vs + len;
                break;
            }
            cut = t;
        }
    }
    out
}

// ───────────────────────── writePptxEmbedding ─────────────────────────

fn rels_path_of(part: &str) -> String {
    let i = part.rfind('/').map_or(0, |i| i + 1);
    format!("{}_rels/{}.rels", &part[..i], &part[i..])
}

/// `ooxml.ts` `resolveTarget` (handles `..` and `.` segments).
fn resolve_target(from: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        if seg == ".." {
            parts.pop();
        } else if seg != "." {
            parts.push(seg);
        }
    }
    parts.join("/")
}

struct RelTag {
    id: String,
    type_: String,
    target: String,
    external: bool,
    xml: String,
}

/// `new RegExp('\\b' + n + '="([^"]*)"').exec(tag)?.[1] ?? ''`.
fn attr_of(tag: &str, n: &str) -> String {
    let b = tag.as_bytes();
    let lit = format!("{n}=\"");
    let mut from = 0;
    while let Some(p) = tag.get(from..).and_then(|s| s.find(&lit)).map(|p| from + p) {
        from = p + 1;
        if p > 0 && is_js_word(b[p - 1]) {
            continue;
        }
        let vs = p + lit.len();
        if let Some(len) = b[vs..].iter().position(|&c| c == b'"') {
            return tag[vs..vs + len].to_owned();
        }
    }
    String::new()
}

/// `readRels`: `/<Relationship\b[^>]*\/?>/g`.
fn read_rels(store: &PartStore, part: &str) -> Vec<RelTag> {
    let Some(xml) = store.text(&rels_path_of(part)).filter(|x| !x.is_empty()) else { return Vec::new() };
    let b = xml.as_bytes();
    let name = "<Relationship";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(name)).map(|p| from + p) {
        let after = p + name.len();
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            from = p + 1;
            continue;
        }
        let Some(g) = b[after..].iter().position(|&c| c == b'>') else { break };
        let end = after + g + 1;
        let tag = &xml[p..end];
        out.push(RelTag {
            id: attr_of(tag, "Id"),
            type_: attr_of(tag, "Type"),
            target: unesc_attr(&attr_of(tag, "Target")),
            external: attr_of(tag, "TargetMode") == "External",
            xml: tag.to_owned(),
        });
        from = end;
    }
    out
}

/// `addRel` of `ooxml.ts`: the first free `rId` from `size + 1`.
fn add_rel(store: &mut PartStore, part: &str, type_: &str, target: &str) -> String {
    let path = rels_path_of(part);
    let mut xml = store.text(&path).unwrap_or_else(|| {
        format!("{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>")
    });
    let used: FxHashSet<String> = read_rels(store, part).into_iter().map(|r| r.id).collect();
    let mut n = used.len() + 1;
    while used.contains(&format!("rId{n}")) {
        n += 1;
    }
    let id = format!("rId{n}");
    // <Relationships ...\s*/> -> <Relationships ...></Relationships>
    if let Some(p) = xml.find("<Relationships") {
        let after = p + "<Relationships".len();
        if !xml.as_bytes().get(after).is_some_and(|&c| is_js_word(c))
            && let Some(g) = xml[after..].find('>')
        {
            let end = after + g;
            if xml[..end].ends_with('/') {
                let head = xml[after..end - 1].trim_end_matches(is_js_space).to_owned();
                xml = format!("{}<Relationships{head}></Relationships>{}", &xml[..p], &xml[end + 1..]);
            }
        }
    }
    // </Relationships>\s*$
    let trimmed_len = xml.trim_end_matches(is_js_space).len();
    if xml[..trimmed_len].ends_with("</Relationships>") {
        let at = trimmed_len - "</Relationships>".len();
        xml = format!("{}<Relationship Id=\"{id}\" Type=\"{type_}\" Target=\"{}\"/></Relationships>", &xml[..at], escape_attr(target));
    }
    store.set_text(&path, &xml);
    id
}

/// `removeRelsOfType`: the relations of the type and the parts they point to.
fn remove_rels_of_type(store: &mut PartStore, part: &str, type_: &str) {
    let path = rels_path_of(part);
    let Some(xml) = store.text(&path).filter(|x| !x.is_empty()) else { return };
    let mut out = xml;
    let mut any = false;
    for r in read_rels(store, part) {
        if r.type_ != type_ {
            continue;
        }
        any = true;
        if let Some(p) = out.find(&r.xml) {
            out.replace_range(p..p + r.xml.len(), "");
        }
        if !r.external {
            store.remove(&resolve_target(part, &r.target));
        }
    }
    if any {
        store.set_text(&path, &out);
    }
}

fn ensure_default_content_type(store: &mut PartStore, ext: &str, ct: &str) {
    let Some(xml) = store.text("[Content_Types].xml").filter(|x| !x.is_empty()) else { return };
    // /<Default\b[^>]*Extension="ext"/i
    let b = xml.as_bytes();
    let lower = xml.to_ascii_lowercase();
    let lit = format!("extension=\"{}\"", ext.to_ascii_lowercase());
    let mut from = 0;
    while let Some(p) = lower.get(from..).and_then(|s| s.find("<default")).map(|p| from + p) {
        from = p + 1;
        let after = p + 8;
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        let gt = b[after..].iter().position(|&c| c == b'>').map_or(b.len(), |g| after + g);
        if lower[after..gt].contains(&lit) {
            return;
        }
    }
    // (<Types\b[^>]*>) -> $1<Default .../>
    let tn = "<Types";
    let mut f = 0;
    while let Some(p) = xml.get(f..).and_then(|s| s.find(tn)).map(|p| f + p) {
        f = p + 1;
        let after = p + tn.len();
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        if let Some(g) = b[after..].iter().position(|&c| c == b'>') {
            let end = after + g + 1;
            let out = format!("{}<Default Extension=\"{ext}\" ContentType=\"{ct}\"/>{}", &xml[..end], &xml[end..]);
            store.set_text("[Content_Types].xml", &out);
            return;
        }
    }
}

fn unique_part_name(store: &PartStore, dir: &str, ext: &str) -> String {
    let names: FxHashSet<String> = store.names().map(uo_jsnum::text::to_lowercase).collect();
    let mut n = 1u64;
    while names.contains(&uo_jsnum::text::to_lowercase(&format!("{dir}font{n}.{ext}"))) {
        n += 1;
    }
    format!("{dir}font{n}.{ext}")
}

/// `elementEnd` of `ooxml.ts`: the end of `<qname ...>` that starts at `start` (self-closing or not).
fn element_end(xml: &str, start: usize, qname: &str) -> Option<usize> {
    let gt = start + xml[start..].find('>')?;
    if xml.as_bytes().get(gt.wrapping_sub(1)) == Some(&b'/') {
        return Some(gt + 1);
    }
    let close = format!("</{qname}>");
    let c = gt + xml[gt..].find(&close)?;
    Some(c + qname.len() + 3)
}

/// `findElement` of `ooxml.ts`: the first `<qname` followed by `[\s/>]` and its end.
fn find_element(xml: &str, qname: &str) -> Option<(usize, usize)> {
    let b = xml.as_bytes();
    let name = format!("<{qname}");
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(&name)).map(|p| from + p) {
        from = p + 1;
        let k = p + name.len();
        let ok = b.get(k).is_some_and(|&c| c == b'/' || c == b'>' || js_space_len(b, k) > 0);
        if ok {
            return element_end(xml, p, qname).map(|e| (p, e));
        }
    }
    None
}

fn p_embed(v: Variant) -> &'static str {
    match v {
        Variant::Regular => "p:regular",
        Variant::Bold => "p:bold",
        Variant::Italic => "p:italic",
        Variant::BoldItalic => "p:boldItalic",
    }
}

fn variant_order(v: Variant) -> usize {
    match v {
        Variant::Regular => 0,
        Variant::Bold => 1,
        Variant::Italic => 2,
        Variant::BoldItalic => 3,
    }
}

/// `groupByFamily`: by first appearance of the family, the faces in variant order.
fn group_by_family(faces: &[FaceToEmbed]) -> Vec<(&str, Vec<&FaceToEmbed>)> {
    let mut m: Vec<(&str, Vec<&FaceToEmbed>)> = Vec::new();
    for f in faces {
        match m.iter_mut().find(|(k, _)| *k == f.family) {
            Some((_, v)) => v.push(f),
            None => m.push((&f.family, vec![f])),
        }
    }
    for (_, v) in &mut m {
        v.sort_by_key(|f| variant_order(f.variant));
    }
    m
}

/// `writePptxEmbedding`: applies the option to the `.pptx` (attributes of `<p:presentation>`, removes or
/// adds fonts). With the option ON what came embedded stays as it is (PowerPoint's compressed EOT is
/// not redone) and only the missing families are added; OFF everything goes, as in Office.
pub fn write_pptx_embedding(store: &mut PartStore, plan: &EmbeddingPlan, settings: &FontEmbeddingSettings) -> E<()> {
    let Some(mut xml) = store.text(PRES).filter(|x| !x.is_empty()) else { return Ok(()) };
    // the attributes of the option in the root element
    if let Some((s, e)) = root_tag(&xml) {
        let tag = &xml[s..e];
        let mut t = remove_option_attrs(tag);
        if settings.embed {
            let attrs = format!(" embedTrueTypeFonts=\"1\"{}", if settings.subset { " saveSubsetFonts=\"1\"" } else { "" });
            // /\s*(\/?>)$/ -> attrs + $1
            let (close, head_end) = if t.ends_with("/>") { ("/>", t.len() - 2) } else { (">", t.len().saturating_sub(1)) };
            if t.ends_with('>') {
                let head = t[..head_end].trim_end_matches(is_js_space);
                t = format!("{head}{attrs}{close}");
            }
        }
        xml = format!("{}{t}{}", &xml[..s], &xml[e..]);
    }
    if !settings.embed {
        remove_rels_of_type(store, PRES, REL_FONT);
        if let Some((s, e)) = find_element(&xml, "p:embeddedFontLst") {
            xml = format!("{}{}", &xml[..s], &xml[e..]);
        }
        store.set_text(PRES, &xml);
        return Ok(());
    }
    if !plan.faces.is_empty() {
        ensure_default_content_type(store, "fntdata", CT_FNTDATA);
        let mut entries = String::new();
        for (family, faces) in group_by_family(&plan.faces) {
            let mut refs = String::new();
            for f in &faces {
                let part = unique_part_name(store, "ppt/fonts/", "fntdata");
                let eot = uo_fonts::sfnt_to_eot(&f.bytes, f.subsetted).map_err(|e| EditError::Fonts(e.to_string().into()))?;
                store.set(&part, std::sync::Arc::from(eot));
                let id = add_rel(store, PRES, REL_FONT, &part["ppt/".len()..]);
                refs.push_str(&format!("<{} r:id=\"{id}\"/>", p_embed(f.variant)));
            }
            let panose = faces.first().map_or("", |f| f.info.panose.as_str());
            entries.push_str(&format!(
                "<p:embeddedFont><p:font typeface=\"{}\" panose=\"{panose}\" charset=\"0\"/>{refs}</p:embeddedFont>",
                escape_attr(family)
            ));
        }
        match find_element(&xml, "p:embeddedFontLst") {
            Some((s, e)) => {
                let el = &xml[s..e];
                let merged = if el.ends_with("/>") {
                    format!("<p:embeddedFontLst>{entries}</p:embeddedFontLst>")
                } else {
                    match el.strip_suffix("</p:embeddedFontLst>") {
                        Some(h) => format!("{h}{entries}</p:embeddedFontLst>"),
                        None => el.to_owned(),
                    }
                };
                xml = format!("{}{merged}{}", &xml[..s], &xml[e..]);
            }
            None => {
                let block = format!("<p:embeddedFontLst>{entries}</p:embeddedFontLst>");
                let mut at: Option<usize> = None;
                for n in PRES_AFTER {
                    if let Some((s, _)) = find_element(&xml, n)
                        && at.is_none_or(|a| s < a)
                    {
                        at = Some(s);
                    }
                }
                // `xml.lastIndexOf(..)` = -1 slices before the last character
                let at = at
                    .or_else(|| xml.rfind("</p:presentation>"))
                    .unwrap_or_else(|| super::xml::floor_boundary(&xml, xml.len().saturating_sub(1)));
                xml = format!("{}{block}{}", &xml[..at], &xml[at..]);
            }
        }
    }
    store.set_text(PRES, &xml);
    Ok(())
}

/// `tag.replace(/\s(?:embedTrueTypeFonts|saveSubsetFonts)="[^"]*"/g, '')`.
fn remove_option_attrs(tag: &str) -> String {
    let b = tag.as_bytes();
    let mut out = String::new();
    let mut at = 0;
    let mut q = 0;
    while q < b.len() {
        let wl = js_space_len(b, q);
        let mut matched = None;
        if wl > 0 {
            for name in ["embedTrueTypeFonts", "saveSubsetFonts"] {
                let lit = format!("{name}=\"");
                if tag[q + wl..].starts_with(&lit) {
                    let vs = q + wl + lit.len();
                    if let Some(len) = b[vs..].iter().position(|&c| c == b'"') {
                        matched = Some(vs + len + 1);
                        break;
                    }
                }
            }
        }
        if let Some(end) = matched {
            out.push_str(&tag[at..q]);
            at = end;
            q = end;
            continue;
        }
        q += tag[q..].chars().next().map_or(1, char::len_utf8);
    }
    out.push_str(&tag[at..]);
    out
}

/// The PowerPoint plan policy (the TS `planFontEmbedding(..., 'slides')`): its common fonts, the four
/// variants of each family, common fonts always skipped.
pub fn powerpoint_embedding_policy(settings: &FontEmbeddingSettings) -> uo_fonts::EmbeddingPolicy<'static> {
    uo_fonts::EmbeddingPolicy {
        embed: settings.embed,
        subset: settings.subset,
        skip_common_fonts: true,
        common_fonts: &uo_fonts::POWERPOINT_COMMON_FONTS,
        all_variants: true,
    }
}

/// Builds the borrowed `FontUsage` list of [`deck_font_usage`].
pub fn as_usages<'a>(owned: &'a [(String, String)]) -> Vec<FontUsage<'a>> {
    owned.iter().map(|(family, text)| FontUsage { family, variants: &[Variant::Regular], text }).collect()
}
