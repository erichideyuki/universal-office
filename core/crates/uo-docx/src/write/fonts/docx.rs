//! The docx side of `shared/fonts/src/ooxml.ts`: `readDocxEmbedding` (the option as the file
//! stores it and the families it already embeds) and `writeDocxEmbedding` (the option written to
//! `settings.xml`, the faces of the plan written as `word/fonts/fontN.odttf` with their
//! relationships, `fontTable.xml` entries and content type — or every embedded font removed when
//! the option is off). Formats measured on what Word writes:
//! - `word/fonts/fontN.odttf`: the TTF obfuscated with the GUID of `w:fontKey` (uo-fonts `odttf`);
//! - a `font` relationship from the font table;
//! - `<w:embedRegular r:id w:fontKey [w:subsetted="1"]/>` at the end of the `<w:font>`;
//! - `<w:embedTrueTypeFonts/>` [`<w:embedSystemFonts/>`] [`<w:saveSubsetFonts/>`] in the settings.
//!
//! Editing rule: with the option on, what is already embedded stays as it is (Word writes
//! compressed EOT/obfuscation this code does not redo) and only the missing families are added;
//! with it off, everything goes — as in Office.

use uo_fonts::{EmbeddingPlan, EmbeddingPolicy, FontEmbeddingSettings, Variant, WORD_COMMON_FONTS};
use uo_jsnum::text::to_lowercase;
use uo_opc::Package;
use uo_opc::error::LimitKind;
use uo_opc::rels::REL_TYPE_PREFIX;
use uo_text::case::regexp_canonicalize;

use super::ooxml::{
    FontPartNames, NextFind, REL_FONT, TargetRule, add_rel, ensure_default_content_type, ensure_override, esc_attr, find_ascii_ci,
    find_element, find_from, find_start_tag, group_by_family, guid_of, is_word_byte, last_close_trimmed, part_text_nonempty,
    rel_target_of_type, remove_elements, remove_rels_of_type, replace_range, unesc_attr,
};
use crate::error::DocxError;

const DOCX_MAIN: &str = "word/document.xml";
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// The content type of `.odttf` parts.
pub const CT_ODTTF: &str = "application/vnd.openxmlformats-officedocument.obfuscatedFont";
const CT_SETTINGS: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CT_FONT_TABLE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";

/// The elements that come BEFORE `w:embedTrueTypeFonts` in `CT_Settings`.
const SETTINGS_BEFORE: [&str; 10] = [
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
    "displayBackgroundShape",
    "printPostScriptOverText",
    "printFractionalCharacterWidth",
    "printFormsData",
];

const SETTINGS_FLAGS: [&str; 3] = ["w:embedTrueTypeFonts", "w:embedSystemFonts", "w:saveSubsetFonts"];

fn w_embed(v: Variant) -> &'static str {
    match v {
        Variant::Regular => "w:embedRegular",
        Variant::Bold => "w:embedBold",
        Variant::Italic => "w:embedItalic",
        Variant::BoldItalic => "w:embedBoldItalic",
    }
}

/// The option as a docx stores it (`Partial<FontEmbeddingSettings>` of the TS): off is only
/// `embed: false` (the details stay with the app defaults, as Office does when the option is
/// turned on); on also gives `subset` and `skip_common_fonts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredEmbeddingSettings {
    pub embed: bool,
    pub subset: Option<bool>,
    pub skip_common_fonts: Option<bool>,
}

impl StoredEmbeddingSettings {
    /// `{ ...defaults, ...stored }` (`DEFAULT_EMBEDDING.write` is
    /// [`uo_fonts::WORD_DEFAULT_EMBEDDING`]).
    #[must_use]
    pub fn over(&self, defaults: FontEmbeddingSettings) -> FontEmbeddingSettings {
        FontEmbeddingSettings {
            embed: self.embed,
            subset: self.subset.unwrap_or(defaults.subset),
            skip_common_fonts: self.skip_common_fonts.unwrap_or(defaults.skip_common_fonts),
        }
    }
}

/// `readDocxEmbedding`: the stored option and the families already embedded (lower-cased, in
/// first-appearance order: the TS `Set`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocxEmbedding {
    pub settings: StoredEmbeddingSettings,
    pub embedded: Vec<String>,
}

impl DocxEmbedding {
    /// The families as the `already_embedded` set of [`uo_fonts::embedding_requests`] and
    /// [`uo_fonts::plan_font_embedding`].
    #[must_use]
    pub fn embedded_set(&self) -> uo_fonts::FxHashSet<String> {
        self.embedded.iter().cloned().collect()
    }
}

/// The plan policy of Word (the TS `planFontEmbedding(…, 'write')`): Word's common fonts, the
/// used variants only, `skip_common_fonts` as the user set it.
#[must_use]
pub fn word_embedding_policy(settings: &FontEmbeddingSettings) -> EmbeddingPolicy<'static> {
    EmbeddingPolicy {
        embed: settings.embed,
        subset: settings.subset,
        skip_common_fonts: settings.skip_common_fonts,
        common_fonts: &WORD_COMMON_FONTS,
        all_variants: false,
    }
}

/// `docxPart`: the part of the main document's relationship of `type_` (`None`: none;
/// `Some(None)`: one whose target names no part).
fn docx_part(pkg: &Package, type_: &str, rule: TargetRule) -> Result<Option<Option<String>>, DocxError> {
    rel_target_of_type(pkg, DOCX_MAIN, &format!("{REL_TYPE_PREFIX}/{type_}"), rule)
}

/// `pkg.getPartText(docxPart(pkg, type_, fallback)) ?? ''`.
fn docx_part_text(pkg: &Package, type_: &str, fallback: &str, rule: TargetRule) -> Result<String, DocxError> {
    let name = match docx_part(pkg, type_, rule)? {
        None => Some(fallback.to_owned()),
        Some(found) => found,
    };
    match name {
        Some(n) => Ok(pkg.part_text(&n)?.map(std::borrow::Cow::into_owned).unwrap_or_default()),
        None => Ok(String::new()),
    }
}

/// `readDocxEmbedding`, relationship targets resolved with uo-opc D6.
pub fn read_docx_embedding(pkg: &Package) -> Result<DocxEmbedding, DocxError> {
    read_docx_embedding_with(pkg, TargetRule::Opc)
}

/// [`read_docx_embedding`] with a [`TargetRule`] (parity only: [`TargetRule::Fonts09`]).
pub fn read_docx_embedding_with(pkg: &Package, rule: TargetRule) -> Result<DocxEmbedding, DocxError> {
    let settings = docx_part_text(pkg, "settings", "word/settings.xml", rule)?;
    let table = docx_part_text(pkg, "fontTable", "word/fontTable.xml", rule)?;
    let mut embedded: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in font_entries(&table) {
        if has_embed_tag(table.get(e.content.0..e.content.1).unwrap_or("")) {
            let name = to_lowercase(&unesc_attr(table.get(e.value.0..e.value.1).unwrap_or("")));
            if seen.insert(name.clone()) {
                embedded.push(name);
            }
        }
    }
    let settings = if setting_on(&settings, "embedTrueTypeFonts") {
        StoredEmbeddingSettings {
            embed: true,
            subset: Some(setting_on(&settings, "saveSubsetFonts")),
            skip_common_fonts: Some(!setting_on(&settings, "embedSystemFonts")),
        }
    } else {
        StoredEmbeddingSettings { embed: false, subset: None, skip_common_fonts: None }
    };
    Ok(DocxEmbedding { settings, embedded })
}

fn limit(part: &str, pkg: &Package) -> DocxError {
    DocxError::Package(uo_opc::Error::Limit {
        kind: LimitKind::PartBytes,
        max: pkg.limits().max_xml_part_bytes,
        part: Some(part.to_owned()),
    })
}

/// `writeDocxSettings`.
fn write_docx_settings(pkg: &mut Package, settings: &FontEmbeddingSettings, rule: TargetRule) -> Result<(), DocxError> {
    let current = match docx_part(pkg, "settings", rule)? {
        Some(Some(name)) => part_text_nonempty(pkg, &name)?.map(|t| (name, t)),
        _ => None,
    };
    let (path, mut xml) = match current {
        Some(found) => found,
        None => {
            if !settings.embed {
                return Ok(());
            }
            add_rel(pkg, DOCX_MAIN, &format!("{REL_TYPE_PREFIX}/settings"), "settings.xml")?;
            ensure_override(pkg, "word/settings.xml", CT_SETTINGS)?;
            (
                "word/settings.xml".to_owned(),
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"{W_NS}\"></w:settings>"),
            )
        }
    };
    for name in SETTINGS_FLAGS {
        xml = remove_elements(&xml, name);
    }
    if settings.embed {
        let mut flags = String::from("<w:embedTrueTypeFonts/>");
        if !settings.skip_common_fonts {
            flags.push_str("<w:embedSystemFonts/>");
        }
        if settings.subset {
            flags.push_str("<w:saveSubsetFonts/>");
        }
        let mut at: Option<usize> = None;
        for n in SETTINGS_BEFORE {
            if let Some((_, end)) = find_element(&xml, &format!("w:{n}")) {
                at = Some(at.map_or(end, |a| a.max(end)));
            }
        }
        let at = match at {
            Some(a) => a,
            None => {
                // the TS throws a TypeError here (`exec(…)!` of a part without the start tag)
                let (s, e) =
                    find_start_tag(&xml, "w:settings").ok_or(DocxError::Corrupt("settings part without a w:settings start tag"))?;
                let root = xml.get(s..e).unwrap_or("");
                if let Some(open) = root.strip_suffix("/>") {
                    let template = format!("{open}></w:settings>");
                    xml = replace_range(&xml, s, e, &template, pkg.limits().max_xml_part_bytes).ok_or_else(|| limit(&path, pkg))?;
                }
                find_start_tag(&xml, "w:settings")
                    .map(|(_, e2)| e2)
                    .ok_or(DocxError::Corrupt("settings part without a w:settings start tag"))?
            }
        };
        let mut out = String::with_capacity(xml.len() + flags.len());
        out.push_str(xml.get(..at).unwrap_or(""));
        out.push_str(&flags);
        out.push_str(xml.get(at..).unwrap_or(""));
        xml = out;
    }
    pkg.set_part_text(path, xml);
    Ok(())
}

/// `writeDocxEmbedding`: applies the option to a saved docx package. On: the settings flags, then
/// every face of `plan` (grouped by family, `VARIANTS` order) as an obfuscated part with a new GUID
/// from `guid` (16 random bytes per face, the TS `crypto.getRandomValues`), its relationship from
/// the font table and its `w:embed*` element, merged into the family's `<w:font>` (matched
/// case-insensitively as the TS `RegExp` with `i` does) or in a new one at the end. Off: the flags
/// removed, every `font` relationship of the font table and its part removed, every self-closing
/// `w:embed*` element removed.
///
/// A two-phase step for the host (R-EMBED): [`read_docx_embedding`] → `embedding_requests` → the
/// host fetches the faces → `plan_font_embedding` → this. On error the package may be partly
/// edited (the TS edits it in place too): the caller drops it, as the editor does with the
/// package it loads only for this step.
pub fn write_docx_embedding(
    pkg: &mut Package,
    plan: &EmbeddingPlan,
    settings: &FontEmbeddingSettings,
    guid: &mut dyn FnMut() -> [u8; 16],
) -> Result<(), DocxError> {
    write_docx_embedding_with(pkg, plan, settings, guid, TargetRule::Opc)
}

/// [`write_docx_embedding`] with a [`TargetRule`] (parity only: [`TargetRule::Fonts09`]).
pub fn write_docx_embedding_with(
    pkg: &mut Package,
    plan: &EmbeddingPlan,
    settings: &FontEmbeddingSettings,
    guid: &mut dyn FnMut() -> [u8; 16],
    rule: TargetRule,
) -> Result<(), DocxError> {
    write_docx_settings(pkg, settings, rule)?;
    let found = match docx_part(pkg, "fontTable", rule)? {
        Some(Some(name)) => part_text_nonempty(pkg, &name)?.map(|t| (name, t)),
        _ => None,
    };

    if !settings.embed {
        let Some((path, table)) = found else { return Ok(()) };
        remove_rels_of_type(pkg, &path, REL_FONT, rule)?;
        pkg.set_part_text(path, strip_embed_tags(&table));
        return Ok(());
    }
    if plan.faces.is_empty() {
        return Ok(());
    }
    let (path, mut table) = match found {
        Some(f) => f,
        None => {
            add_rel(pkg, DOCX_MAIN, &format!("{REL_TYPE_PREFIX}/fontTable"), "fontTable.xml")?;
            ensure_override(pkg, "word/fontTable.xml", CT_FONT_TABLE)?;
            (
                "word/fontTable.xml".to_owned(),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:fonts xmlns:w=\"{W_NS}\" xmlns:r=\"{REL_TYPE_PREFIX}\"></w:fonts>"
                ),
            )
        }
    };
    let cap = pkg.limits().max_xml_part_bytes;
    if !fonts_has_xmlns_r(&table) {
        table = fonts_add_xmlns_r(&table);
    }
    if let Some((slash, gt)) = fonts_self_closing(&table) {
        table = format!("{}></w:fonts>{}", table.get(..slash).unwrap_or(""), table.get(gt + 1..).unwrap_or(""));
    }
    ensure_default_content_type(pkg, "odttf", CT_ODTTF)?;
    let cut = path.rfind('/').map_or(0, |i| i + 1);
    let dir = format!("{}fonts/", path.get(..cut).unwrap_or(""));
    let mut names = FontPartNames::new(pkg);

    for (family, faces) in group_by_family(&plan.faces) {
        let mut embeds = String::new();
        for f in &faces {
            let key = guid_of(guid());
            let part = names.take(&dir, "odttf");
            pkg.set_part(part.clone(), uo_fonts::odttf::obfuscate(&f.bytes, &key));
            let id = add_rel(pkg, &path, REL_FONT, part.get(cut..).unwrap_or(""))?;
            embeds.push('<');
            embeds.push_str(w_embed(f.variant));
            embeds.push_str(" r:id=\"");
            embeds.push_str(&id);
            embeds.push('"');
            if f.subsetted {
                embeds.push_str(" w:subsetted=\"1\"");
            }
            embeds.push_str(" w:fontKey=\"");
            embeds.push_str(&key);
            embeds.push_str("\"/>");
        }
        let escaped = esc_attr(family);
        if let Some((s, e)) = find_font_element_ci(&table, &escaped) {
            let el = table.get(s..e).unwrap_or("");
            let merged = if let Some(open) = el.strip_suffix("/>") {
                format!("{open}>{embeds}</w:font>")
            } else if let Some(body) = el.strip_suffix("</w:font>") {
                format!("{body}{embeds}</w:font>")
            } else {
                // `</W:FONT>` matched case-insensitively: the case-sensitive `$` replace leaves it
                el.to_owned()
            };
            table = replace_range(&table, s, e, &merged, cap).ok_or_else(|| limit(&path, pkg))?;
        } else if let Some(p) = last_close_trimmed(&table, "w:fonts") {
            let panose = faces.first().map_or("", |f| f.info.panose.as_str());
            let entry = format!(
                "<w:font w:name=\"{escaped}\"><w:panose1 w:val=\"{panose}\"/><w:charset w:val=\"00\"/><w:family w:val=\"auto\"/><w:pitch w:val=\"variable\"/>{embeds}</w:font></w:fonts>"
            );
            table = replace_range(&table, p, table.len(), &entry, cap).ok_or_else(|| limit(&path, pkg))?;
        }
    }
    pkg.set_part_text(path, table);
    Ok(())
}

// ───────────────────────── the docx regex sites (hand matchers) ─────────────────────────

/// `on(name)` of `readDocxEmbedding`: `/<w:NAME(?![^>]*w:val="(?:false|0|off)")[\s/>]/.test(xml)`
/// — some `<w:NAME` followed by JS white space, `/` or `>` whose text from just after the name up
/// to the next `>` (or the end) holds none of `w:val="false"`, `w:val="0"`, `w:val="off"`
/// (case-sensitive).
#[must_use]
pub fn setting_on(xml: &str, name: &str) -> bool {
    let open = format!("<w:{name}");
    let mut gts = NextFind::new(xml, ">", false);
    let mut offs = [
        NextFind::new(xml, "w:val=\"false\"", false),
        NextFind::new(xml, "w:val=\"0\"", false),
        NextFind::new(xml, "w:val=\"off\"", false),
    ];
    let mut from = 0;
    while let Some(p) = find_from(xml, &open, from) {
        from = p + 1;
        let q = p + open.len();
        let Some(c) = xml.get(q..).and_then(|r| r.chars().next()) else { continue };
        if !(uo_jsnum::text::is_js_space(c) || c == '/' || c == '>') {
            continue;
        }
        let end = gts.at(q).unwrap_or(xml.len());
        // a literal holds no `>`, so one that starts before `end` lies inside the segment
        if !offs.iter_mut().any(|f| f.at(q).is_some_and(|x| x < end)) {
            return true;
        }
    }
    false
}

/// One match of `/<w:font\b[^>]*w:name="([^"]*)"[^>]*>([\s\S]*?)<\/w:font>/g`: byte ranges of
/// group 1 (the name as written) and group 2 (the content), and of the whole match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontEntry {
    pub start: usize,
    pub end: usize,
    pub value: (usize, usize),
    pub content: (usize, usize),
}

/// Every match of the font-table regex of `readDocxEmbedding`, as V8 finds them. At a
/// `<w:font` + non-word start, with `gt` the first `>` after it: the greedy `[^>]*` takes the
/// LAST `w:name="` that starts before `gt`; its value runs to the next `"` (it may cross `>`),
/// then to the next `>`, then the content to the next `</w:font>`. If that fails, backtracking
/// reaches the second-to-last `w:name="` of the tag, whose value necessarily closes before `gt`
/// (at the latest on the quote of the last one): content from `gt`. A self-closing `<w:font/>`
/// therefore takes the next font's content (TS behaviour). Linear: the name positions are
/// listed once and every "next occurrence" query is memoised.
#[must_use]
pub fn font_entries(table: &str) -> Vec<FontEntry> {
    const OPEN: &str = "<w:font";
    const NAME: &str = "w:name=\"";
    const CLOSE: &str = "</w:font>";
    let mut names = Vec::new();
    let mut f = 0;
    while let Some(p) = find_from(table, NAME, f) {
        names.push(p);
        f = p + 1;
    }
    let last_close = table.rfind(CLOSE);
    let close_after = |x: usize| last_close.filter(|&c| c >= x).and_then(|_| find_from(table, CLOSE, x));
    let mut gts = NextFind::new(table, ">", false);
    let mut gt2s = NextFind::new(table, ">", false);
    let mut quotes = NextFind::new(table, "\"", false);
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(s) = find_from(table, OPEN, pos) {
        pos = s + 1;
        if table.as_bytes().get(s + OPEN.len()).is_some_and(|&b| is_word_byte(b)) {
            continue;
        }
        let Some(gt) = gts.at(s + OPEN.len()) else { break };
        // the `w:name="` that start at or after s + 7 and end at or before gt
        let lo = names.partition_point(|&a| a < s + OPEN.len());
        let hi = names.partition_point(|&a| a + NAME.len() <= gt);
        if lo >= hi {
            continue;
        }
        let Some(&last) = names.get(hi - 1) else { continue };
        let tried = quotes.at(last + NAME.len()).and_then(|v| gt2s.at(v + 1).map(|g2| (v, g2))).and_then(|(v, g2)| {
            close_after(g2 + 1).map(|c| FontEntry { start: s, end: c + CLOSE.len(), value: (last + NAME.len(), v), content: (g2 + 1, c) })
        });
        let entry = tried.or_else(|| {
            if hi - lo < 2 {
                return None;
            }
            let second = *names.get(hi - 2)?;
            let v = find_from(table, "\"", second + NAME.len())?;
            close_after(gt + 1).map(|c| FontEntry { start: s, end: c + CLOSE.len(), value: (second + NAME.len(), v), content: (gt + 1, c) })
        });
        if let Some(e) = entry {
            out.push(e);
            pos = e.end;
        }
    }
    out
}

/// The name of `<w:embed(?:Regular|Bold|Italic|BoldItalic)\b` at `p` (a `<w:embed` start): the
/// alternative that is followed by a non-word character or the end (at most one is).
fn embed_name_end(s: &str, p: usize) -> Option<usize> {
    let after = p + "<w:embed".len();
    ["Regular", "Bold", "Italic", "BoldItalic"].into_iter().find_map(|n| {
        let end = after + n.len();
        (s.get(after..end) == Some(n) && s.as_bytes().get(end).is_none_or(|&b| !is_word_byte(b))).then_some(end)
    })
}

/// `/<w:embed(?:Regular|Bold|Italic|BoldItalic)\b/.test(s)`.
#[must_use]
pub fn has_embed_tag(s: &str) -> bool {
    let mut from = 0;
    while let Some(p) = find_from(s, "<w:embed", from) {
        if embed_name_end(s, p).is_some() {
            return true;
        }
        from = p + 1;
    }
    false
}

/// `s.replace(/<w:embed(?:Regular|Bold|Italic|BoldItalic)\b[^>]*\/>/g, '')`: every such start
/// whose first `>` after the name has `/` right before it is removed up to that `>`; a tag
/// closed by a plain `>` stays.
#[must_use]
pub fn strip_embed_tags(s: &str) -> String {
    let mut gts = NextFind::new(s, ">", false);
    let mut out = String::with_capacity(s.len());
    let (mut last, mut from) = (0, 0);
    while let Some(p) = find_from(s, "<w:embed", from) {
        from = p + 1;
        let Some(name_end) = embed_name_end(s, p) else { continue };
        let Some(gt) = gts.at(name_end) else { break };
        if gt > name_end && s.as_bytes().get(gt - 1) == Some(&b'/') {
            out.push_str(s.get(last..p).unwrap_or(""));
            last = gt + 1;
            from = gt + 1;
        }
    }
    out.push_str(s.get(last..).unwrap_or(""));
    out
}

/// The `<w:fonts` + non-word starts and the first `>` after each.
fn fonts_starts(table: &str) -> impl Iterator<Item = (usize, Option<usize>)> + '_ {
    let mut gts = NextFind::new(table, ">", false);
    let mut from = 0;
    std::iter::from_fn(move || {
        loop {
            let s = find_from(table, "<w:fonts", from)?;
            from = s + 1;
            if table.as_bytes().get(s + 8).is_some_and(|&b| is_word_byte(b)) {
                continue;
            }
            return Some((s, gts.at(s + 8)));
        }
    })
}

/// `/<w:fonts\b[^>]*xmlns:r=/.test(table)`: some `<w:fonts` + non-word start whose text up to the
/// next `>` (or the end) holds `xmlns:r=`.
#[must_use]
pub fn fonts_has_xmlns_r(table: &str) -> bool {
    let mut lits = NextFind::new(table, "xmlns:r=", false);
    fonts_starts(table).any(|(s, gt)| lits.at(s + 8).is_some_and(|l| l < gt.unwrap_or(table.len())))
}

/// `table.replace(/<w:fonts\b/, '<w:fonts xmlns:r="…"')`.
#[must_use]
pub fn fonts_add_xmlns_r(table: &str) -> String {
    match fonts_starts(table).next() {
        Some((s, _)) => {
            format!("{}<w:fonts xmlns:r=\"{REL_TYPE_PREFIX}\"{}", table.get(..s).unwrap_or(""), table.get(s + 8..).unwrap_or(""))
        }
        None => table.to_owned(),
    }
}

/// `/(<w:fonts\b[^>]*)\/>/` (the `test` and the `replace` find the same leftmost match): the `/`
/// and `>` of the first `<w:fonts` + non-word start whose first `>` has `/` right before it.
#[must_use]
pub fn fonts_self_closing(table: &str) -> Option<(usize, usize)> {
    for (s, gt) in fonts_starts(table) {
        let gt = gt?;
        if gt > s + 8 && table.as_bytes().get(gt - 1) == Some(&b'/') {
            return Some((gt - 1, gt));
        }
    }
    None
}

/// The family element of `writeDocxEmbedding`:
/// `new RegExp('<w:font\\b[^>]*w:name="' + escRe(escAttr(family)) + '"[^>]*?(\\/>|>[\\s\\S]*?<\\/w:font>)', 'i')`.
/// The `i` flag without `u` compares UTF-16 units by `Canonicalize` (uo-text
/// `regexp_canonicalize`: non-ASCII letters fold too, `é` matches `É`; nothing folds into ASCII).
/// At a `<w:font` + non-word start (ASCII case-insensitive) with `gt` the first `>` after it, the
/// match exists when `w:name="F"` occurs (case-insensitively) inside the tag, and ends after `/>`
/// when the tag is self-closing, else after the first `</w:font>` (case-insensitive) after `gt`.
/// The occurrence is searched once per tag with KMP over the canonical units.
#[must_use]
pub fn find_font_element_ci(table: &str, escaped_family: &str) -> Option<(usize, usize)> {
    let pattern: Vec<u16> = format!("w:name=\"{escaped_family}\"").encode_utf16().map(regexp_canonicalize).collect();
    let fail = kmp_table(&pattern);
    let mut gts = NextFind::new(table, ">", false);
    let mut closes = NextFind::new(table, "</w:font>", true);
    let mut group: Option<(usize, Option<usize>)> = None;
    let mut from = 0;
    while let Some(s) = find_ascii_ci(table, "<w:font", from) {
        from = s + 1;
        if table.as_bytes().get(s + 7).is_some_and(|&b| is_word_byte(b)) {
            continue;
        }
        let gt = gts.at(s + 7)?;
        let last = match group {
            Some((g, l)) if g == gt => l,
            _ => {
                let l = last_occurrence(table, s + 7, gt, &pattern, &fail);
                group = Some((gt, l));
                l
            }
        };
        if !last.is_some_and(|l| l >= s + 7) {
            continue;
        }
        if table.as_bytes().get(gt - 1) == Some(&b'/') {
            return Some((s, gt + 1));
        }
        if let Some(c) = closes.at(gt + 1) {
            return Some((s, c + "</w:font>".len()));
        }
    }
    None
}

fn kmp_table(p: &[u16]) -> Vec<usize> {
    let mut fail = vec![0usize; p.len()];
    let mut k = 0;
    for i in 1..p.len() {
        while k > 0 && p.get(i) != p.get(k) {
            k = fail.get(k - 1).copied().unwrap_or(0);
        }
        if p.get(i) == p.get(k) {
            k += 1;
        }
        if let Some(slot) = fail.get_mut(i) {
            *slot = k;
        }
    }
    fail
}

/// The byte offset of the last occurrence of `pattern` (canonical units) that lies entirely in
/// `table[from..to]`.
fn last_occurrence(table: &str, from: usize, to: usize, pattern: &[u16], fail: &[usize]) -> Option<usize> {
    let region = table.get(from..to)?;
    let plen = pattern.len();
    if plen == 0 {
        return None;
    }
    // byte offset of each of the last `plen` units, circular
    let mut ring = vec![0usize; plen];
    let (mut k, mut n, mut last) = (0usize, 0usize, None);
    let mut buf = [0u16; 2];
    for (off, c) in region.char_indices() {
        for &u in c.encode_utf16(&mut buf).iter() {
            let u = regexp_canonicalize(u);
            if let Some(slot) = ring.get_mut(n % plen) {
                *slot = from + off;
            }
            while k > 0 && pattern.get(k) != Some(&u) {
                k = fail.get(k - 1).copied().unwrap_or(0);
            }
            if pattern.get(k) == Some(&u) {
                k += 1;
            }
            n += 1;
            if k == plen {
                last = ring.get((n - plen) % plen).copied();
                k = fail.get(k - 1).copied().unwrap_or(0);
            }
        }
    }
    last
}

/// The case-insensitive (ASCII) `</w:font>` finder, exposed for the regex differential.
#[must_use]
pub fn find_close_font_ci(table: &str, from: usize) -> Option<usize> {
    find_ascii_ci(table, "</w:font>", from)
}
