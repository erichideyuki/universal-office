//! Frontier **W4b** (settings and header/footer parts, write side): `settings-xml.ts`
//! (`writeDefaultTabStop`, `writeHyphenationSettings`, `writeEvenAndOddHeaders`,
//! `writeDisplayBackgroundShape`, `ensureSettingsPart`) and `hf-ops.ts` (`HF_NS`, `hfPartXml`,
//! `copyHfPartXml`, `dropUnusedHfParts`, `pageNumberGroupIds`). Called by the session's save (D2a)
//! and the D2b header/footer and page commands, not by `write_document`.
//!
//! The regex sites are hand matchers over the raw text, checked against V8 by
//! `js/regex-sites/w4.mjs` and `tests/regex_w4.rs`. The patterns the TS builds from values
//! (`String(Math.round(twips))`, a part path) are templates with the value as a literal, except the
//! number: its `.` and `+` are regex syntax in the TS and are kept as such ([`val_pattern_test`]).

use std::collections::HashSet;

use indexmap::IndexSet;
use uo_jsnum::text::is_js_space;
use uo_opc::Package;

use crate::error::DocxError;
use crate::js::NumKey;
use crate::model::sources::{SourcePart, StoryGroup, StorySource};
use crate::write::fonts::ooxml::NextFind;
use crate::write::lossless::WResult;
use crate::write::matchers::{boundary_at, element_matches, find_from};
use crate::write::parts::{CONTENT_TYPES, DOC_RELS, insert_before_first, part_by_rel_type};

/// `HF_NS` (hf-ops.ts:17): the namespaces Word writes on a new header/footer root.
pub const HF_NS: &str = concat!(
    "xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" ",
    "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ",
    "xmlns:o=\"urn:schemas-microsoft-com:office:office\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" ",
    "xmlns:v=\"urn:schemas-microsoft-com:vml\" ",
    "xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\" ",
    "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
    "xmlns:w10=\"urn:schemas-microsoft-com:office:word\" ",
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" ",
    "xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\" ",
    "xmlns:wpg=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\" ",
    "xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" ",
    "mc:Ignorable=\"w14 w15 wp14\""
);

const SETTINGS_PATH: &str = "word/settings.xml";

/// `partByRelType(pkg, 'settings', 'word/settings.xml')` and the text of that part (`None` when
/// there is no such part, or the text is empty: `!xml` in the TS). A relationship whose target the
/// OPC rule resolves to no part (uo-opc D6, accepted by name) counts as no part, and the path to
/// write to is the fallback.
fn settings_part(pkg: &Package) -> WResult<(String, Option<String>)> {
    match part_by_rel_type(pkg, "settings", SETTINGS_PATH)? {
        Some(path) => {
            let xml = pkg.part_text(&path)?.map(std::borrow::Cow::into_owned).filter(|t| !t.is_empty());
            Ok((path, xml))
        }
        None => Ok((SETTINGS_PATH.to_owned(), None)),
    }
}

/// `pkg.getPartText(path)`.
fn text_of(pkg: &Package, path: &str) -> WResult<Option<String>> {
    Ok(pkg.part_text(path)?.map(std::borrow::Cow::into_owned))
}

// ───────────────────────────── the text sites ─────────────────────────────

/// Every `<NAME\b[^>]*?\/>` of a text (global, non-overlapping; `name` without the `<`): the
/// self-closing tag, from the start of the name up to the first `>` after it, when a `/` comes
/// right before that `>` and not before the end of the name. `[^>]*` greedy, as in
/// `/<NAME\b[^>]*\/>/`, matches the same ranges. Byte ranges.
#[must_use]
pub fn self_closing_matches(s: &str, name: &str) -> Vec<(usize, usize)> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut from = 0;
    let mut gts = NextFind::new(s, ">", false);
    while let Some(i) = find_from(s, &open, from) {
        from = i + 1;
        let pos = i + open.len();
        if !boundary_at(s, pos) {
            continue;
        }
        let Some(gt) = gts.at(pos) else { break };
        if gt > pos && s.as_bytes().get(gt - 1) == Some(&b'/') {
            out.push((i, gt + 1));
            from = gt + 1;
        }
    }
    out
}

/// `/\bATTR="([^"]+)"/.exec(tag)?.[1]`: the first non-empty value of `ATTR="` that follows a
/// non-word character (`ATTR` starts with a word character).
#[must_use]
pub fn quoted_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let mut from = 0;
    while let Some(i) = find_from(tag, &needle, from) {
        from = i + 1;
        if !boundary_at(tag, i) {
            continue;
        }
        let v = tag.get(i + needle.len()..)?;
        if let Some(q) = v.find('"')
            && q > 0
        {
            return v.get(..q);
        }
    }
    None
}

/// The width of the JS white space (`\s`) at the start of `s`.
fn space_at(s: &str) -> Option<usize> {
    s.chars().next().filter(|&c| is_js_space(c)).map(char::len_utf8)
}

/// A token of a `RegExp` built from `String(number)`: a literal, `.` (any unit but a line
/// terminator) or a literal followed by `+` (`1e+21` is `1`, `e+`, `2`, `1`).
#[derive(Clone, Copy)]
struct Tok {
    ch: char,
    any: bool,
    plus: bool,
}

impl Tok {
    fn accepts(self, c: char) -> bool {
        if self.any { !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') && c.len_utf16() == 1 } else { c == self.ch }
    }
}

fn tokens_of(val: &str) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    for c in val.chars() {
        match c {
            '+' => {
                if let Some(t) = toks.last_mut() {
                    t.plus = true;
                }
            }
            '.' => toks.push(Tok { ch: '.', any: true, plus: false }),
            c => toks.push(Tok { ch: c, any: false, plus: false }),
        }
    }
    toks.push(Tok { ch: '"', any: false, plus: false });
    toks
}

fn match_toks(rest: &str, toks: &[Tok]) -> bool {
    let Some((t, tail)) = toks.split_first() else { return true };
    if !t.plus {
        let mut it = rest.chars();
        return it.next().is_some_and(|c| t.accepts(c)) && match_toks(it.as_str(), tail);
    }
    let mut end = 0;
    for c in rest.chars() {
        if !t.accepts(c) {
            break;
        }
        end += c.len_utf8();
    }
    while end > 0 {
        if rest.get(end..).is_some_and(|r| match_toks(r, tail)) {
            return true;
        }
        end -= 1;
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
    }
    false
}

/// `new RegExp(`w:val="${val}"`).test(hay)` for a `val` that is `String(Math.round(n))`: digits,
/// `-`, `Infinity`, `NaN`, or the exponent form of a large number, where `.` and `+` are regex
/// syntax in the TS.
#[must_use]
pub fn val_pattern_test(hay: &str, val: &str) -> bool {
    let mut toks = vec![Tok { ch: '"', any: false, plus: false }];
    toks.extend(tokens_of(val));
    let head = "w:val=";
    let mut from = 0;
    while let Some(i) = find_from(hay, head, from) {
        from = i + 1;
        if hay.get(i + head.len()..).is_some_and(|r| match_toks(r, &toks)) {
            return true;
        }
    }
    false
}

/// `/w:val="(?:0|false|off)"/.test(s)`.
#[must_use]
pub fn val_is_off(s: &str) -> bool {
    ["w:val=\"0\"", "w:val=\"false\"", "w:val=\"off\""].iter().any(|n| s.contains(n))
}

// ───────────────────────────── settings.xml ─────────────────────────────

/// Elements of CT_Settings that come before `w:defaultTabStop` (`BEFORE_DEFAULT_TAB`).
const BEFORE_DEFAULT_TAB: &[&str] = &[
    "w:writeProtection",
    "w:view",
    "w:zoom",
    "w:removePersonalInformation",
    "w:removeDateAndTime",
    "w:doNotDisplayPageBoundaries",
    "w:displayBackgroundShape",
    "w:printPostScriptOverText",
    "w:printFractionalCharacterWidth",
    "w:printFormsData",
    "w:embedTrueTypeFonts",
    "w:embedSystemFonts",
    "w:saveSubsetFonts",
    "w:saveFormsData",
    "w:mirrorMargins",
    "w:alignBordersAndEdges",
    "w:bordersDoNotSurroundHeader",
    "w:bordersDoNotSurroundFooter",
    "w:gutterAtTop",
    "w:hideSpellingErrors",
    "w:hideGrammaticalErrors",
    "w:activeWritingStyle",
    "w:proofState",
    "w:formsDesign",
    "w:attachedTemplate",
    "w:linkStyles",
    "w:stylePaneFormatFilter",
    "w:stylePaneSortMethod",
    "w:documentType",
    "w:mailMerge",
    "w:revisionView",
    "w:trackRevisions",
    "w:doNotTrackMoves",
    "w:doNotTrackFormatting",
    "w:documentProtection",
    "w:autoFormatOverride",
    "w:styleLockTheme",
    "w:styleLockQFSet",
];

/// The hyphenation options in schema order from `w:defaultTabStop` (`HYPH_ORDER`).
const HYPH_ORDER: [&str; 4] = ["w:autoHyphenation", "w:consecutiveHyphenLimit", "w:hyphenationZone", "w:doNotHyphenateCaps"];

/// Elements that come before `w:evenAndOddHeaders` besides [`BEFORE_DEFAULT_TAB`].
const BEFORE_EVEN_ODD_EXTRA: &[&str] = &[
    "w:defaultTabStop",
    "w:autoHyphenation",
    "w:consecutiveHyphenLimit",
    "w:hyphenationZone",
    "w:doNotHyphenateCaps",
    "w:showEnvelope",
    "w:summaryLength",
    "w:clickAndTypeStyle",
    "w:defaultTableStyle",
];

/// Elements that come before `w:displayBackgroundShape` (`BEFORE_DISPLAY_BG`).
const BEFORE_DISPLAY_BG: &[&str] =
    &["w:writeProtection", "w:view", "w:zoom", "w:removePersonalInformation", "w:removeDateAndTime", "w:doNotDisplayPageBoundaries"];

/// Where a new settings child goes: `parseXml(xml, { positions: true })`, then the end of the last
/// child that `before` accepts (or the start of the content). `None` when the root is
/// self-closing (`root.cs === root.e`: the TS leaves the file alone).
fn insertion_point(pkg: &Package, xml: &str, before: impl Fn(&str) -> bool) -> WResult<Option<usize>> {
    let doc = uo_opc::xml::parse(xml, &pkg.limits().bounded(&crate::read::DOCX_POS))?;
    let root = doc.root();
    let p = root.positions().ok_or(DocxError::Write("settings root without positions"))?;
    if p.content_start == p.end {
        return Ok(None);
    }
    let mut at = p.content_start;
    for k in root.child_elements() {
        if before(k.name())
            && let Some(kp) = k.positions()
        {
            at = at.max(kp.end);
        }
    }
    Ok(Some(at))
}

/// `xml.slice(0, at) + el + xml.slice(at)`.
fn insert_at(xml: &str, at: usize, el: &str) -> String {
    format!("{}{el}{}", xml.get(..at).unwrap_or(""), xml.get(at..).unwrap_or(""))
}

/// `xml.slice(0, a) + el + xml.slice(b)`.
fn splice(xml: &str, a: usize, b: usize, el: &str) -> String {
    format!("{}{el}{}", xml.get(..a).unwrap_or(""), xml.get(b..).unwrap_or(""))
}

/// `writeDefaultTabStop(pkg, twips)`: `w:defaultTabStop` in the settings part (twips rounded), the
/// value replaced or the element put at the schema position. No settings part: nothing.
pub fn write_default_tab_stop(pkg: &mut Package, twips: f64) -> WResult<()> {
    let (path, xml) = settings_part(pkg)?;
    let Some(mut xml) = xml else { return Ok(()) };
    let val = uo_jsnum::format::to_js_string(uo_jsnum::math::round(twips));
    let el = format!("<w:defaultTabStop w:val=\"{val}\"/>");
    if let Some(&(a, b)) = self_closing_matches(&xml, "w:defaultTabStop").first() {
        if val_pattern_test(xml.get(a..b).unwrap_or(""), &val) {
            return Ok(());
        }
        xml = splice(&xml, a, b, &el);
    } else {
        let Some(at) = insertion_point(pkg, &xml, |n| BEFORE_DEFAULT_TAB.contains(&n))? else { return Ok(()) };
        xml = insert_at(&xml, at, &el);
    }
    pkg.set_part_text(path, xml);
    Ok(())
}

/// `HyphenationSettings`: the hyphenation options of the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HyphenationSettings {
    /// `w:autoHyphenation`
    pub auto: bool,
    /// hyphenate words in capitals (false is `w:doNotHyphenateCaps`)
    pub caps: bool,
    /// `w:hyphenationZone` (twips)
    pub zone: Option<f64>,
    /// `w:consecutiveHyphenLimit` (0 = no limit)
    pub limit: f64,
}

/// `writeHyphenationSettings(pkg, s)`: the hyphenation options as Word writes them (automatic is
/// `<w:autoHyphenation/>`, none removes it; capitals off is `<w:doNotHyphenateCaps/>`; limit 0 or no
/// zone: no element), in schema order. No settings part: a minimal one when something is on.
pub fn write_hyphenation_settings(pkg: &mut Package, s: &HyphenationSettings) -> WResult<()> {
    let (path, xml) = settings_part(pkg)?;
    let mut xml = match xml {
        Some(x) => x,
        None => {
            if !s.auto && s.caps && !uo_jsnum::conv::truthy(s.limit) && s.zone.is_none() {
                return Ok(());
            }
            ensure_settings_part(pkg)?;
            match text_of(pkg, SETTINGS_PATH)?.filter(|t| !t.is_empty()) {
                Some(x) => x,
                None => return Ok(()),
            }
        }
    };
    let round = |x: f64| uo_jsnum::format::to_js_string(uo_jsnum::math::round(x));
    let want: [Option<String>; 4] = [
        s.auto.then(|| "<w:autoHyphenation/>".to_owned()),
        (s.limit > 0.0).then(|| format!("<w:consecutiveHyphenLimit w:val=\"{}\"/>", round(s.limit))),
        s.zone.map(|z| format!("<w:hyphenationZone w:val=\"{}\"/>", round(z))),
        (!s.caps).then(|| "<w:doNotHyphenateCaps/>".to_owned()),
    ];
    for (idx, name) in HYPH_ORDER.iter().enumerate() {
        let el = want.get(idx).and_then(Option::as_ref);
        if let Some(&(a, b)) = element_matches(&xml, name).first() {
            if el.is_some_and(|e| xml.get(a..b) == Some(e.as_str())) {
                continue;
            }
            xml = splice(&xml, a, b, el.map_or("", String::as_str));
            continue;
        }
        let Some(el) = el else { continue };
        let earlier = HYPH_ORDER.get(..idx).unwrap_or(&[]);
        let Some(at) = insertion_point(pkg, &xml, |n| BEFORE_DEFAULT_TAB.contains(&n) || n == "w:defaultTabStop" || earlier.contains(&n))?
        else {
            continue;
        };
        xml = insert_at(&xml, at, el);
    }
    pkg.set_part_text(path, xml);
    Ok(())
}

/// The shared body of `writeEvenAndOddHeaders` and `writeDisplayBackgroundShape`: a flag element
/// (`<NAME/>`, on unless a `w:val` of 0, false or off) switched on or off at its schema position.
fn write_flag(pkg: &mut Package, name: &str, on: bool, before: impl Fn(&str) -> bool) -> WResult<()> {
    let (path, xml) = settings_part(pkg)?;
    let mut xml = match xml {
        Some(x) => x,
        None => {
            if !on {
                return Ok(());
            }
            ensure_settings_part(pkg)?;
            match text_of(pkg, SETTINGS_PATH)?.filter(|t| !t.is_empty()) {
                Some(x) => x,
                None => return Ok(()),
            }
        }
    };
    let cur = element_matches(&xml, name).first().copied();
    let is_on = cur.is_some_and(|(a, b)| !val_is_off(xml.get(a..b).unwrap_or("")));
    if is_on == on {
        return Ok(());
    }
    let el = format!("<{name}/>");
    match cur {
        Some((a, b)) => xml = splice(&xml, a, b, if on { &el } else { "" }),
        None => {
            let Some(at) = insertion_point(pkg, &xml, before)? else { return Ok(()) };
            xml = insert_at(&xml, at, &el);
        }
    }
    pkg.set_part_text(path, xml);
    Ok(())
}

/// `writeEvenAndOddHeaders(pkg, on)`: `w:evenAndOddHeaders` on or off (Word writes
/// `<w:evenAndOddHeaders/>`). No settings part: a minimal one when turning it on.
pub fn write_even_and_odd_headers(pkg: &mut Package, on: bool) -> WResult<()> {
    write_flag(pkg, "w:evenAndOddHeaders", on, |n| BEFORE_DEFAULT_TAB.contains(&n) || BEFORE_EVEN_ODD_EXTRA.contains(&n))
}

/// `writeDisplayBackgroundShape(pkg, on)`: `w:displayBackgroundShape` on or off. No settings part: a
/// minimal one when turning it on.
pub fn write_display_background_shape(pkg: &mut Package, on: bool) -> WResult<()> {
    write_flag(pkg, "w:displayBackgroundShape", on, |n| BEFORE_DISPLAY_BG.contains(&n))
}

/// `ensureSettingsPart(pkg)`: the minimal settings part of a document without one, with its
/// relationship and content type.
fn ensure_settings_part(pkg: &mut Package) -> WResult<()> {
    if pkg.part(SETTINGS_PATH)?.is_some() {
        return Ok(());
    }
    pkg.set_part_text(
        SETTINGS_PATH,
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:settings>"
        ),
    );
    if let Some(rels) = text_of(pkg, DOC_RELS)?.filter(|t| !t.is_empty())
        && !rels.contains("relationships/settings\"")
    {
        let rels = insert_before_first(
            &rels,
            "</Relationships>",
            "<Relationship Id=\"rIdWnSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>",
        );
        pkg.set_part_text(DOC_RELS, rels);
    }
    if let Some(ct) = text_of(pkg, CONTENT_TYPES)?.filter(|t| !t.is_empty())
        && !ct.contains("PartName=\"/word/settings.xml\"")
    {
        let ct = insert_before_first(
            &ct,
            "</Types>",
            "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>",
        );
        pkg.set_part_text(CONTENT_TYPES, ct);
    }
    Ok(())
}

// ───────────────────────────── header/footer parts (hf-ops.ts) ─────────────────────────────

/// `hfPartXml(kind, inner)`: a complete header/footer part around `inner` (`w:hdr` for `header`, `w:ftr`
/// for anything else).
#[must_use]
pub fn hf_part_xml(kind: &str, inner: &str) -> String {
    let tag = if kind == "header" { "w:hdr" } else { "w:ftr" };
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<{tag} {HF_NS}>{inner}</{tag}>")
}

/// For the `<wp:docPr ...>` tag that ends at `gt`: every valid `\bid="\d+"` in `[from, gt)`, as the
/// start of `id="` and the end of the digits.
fn docpr_id_starts(xml: &str, from: usize, gt: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut k = from;
    while let Some(i) = find_from(xml, "id=\"", k) {
        if i >= gt {
            break;
        }
        k = i + 1;
        if !boundary_at(xml, i) {
            continue;
        }
        let digits_from = i + 4;
        let n = xml.as_bytes().get(digits_from..).map_or(0, |r| r.iter().take_while(|b| b.is_ascii_digit()).count());
        if n > 0 && xml.as_bytes().get(digits_from + n) == Some(&b'"') {
            out.push((i, digits_from + n));
        }
    }
    out
}

/// `xml.replace(/(<wp:docPr\b[^>]*?\bid=")(\d+)(")/g, (_, a, _, b) => a + newId() + b)`.
fn renew_docpr_ids(xml: &str, new_id: &mut dyn FnMut() -> f64) -> String {
    let open = "<wp:docPr";
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    let mut from = 0;
    let mut gts = NextFind::new(xml, ">", false);
    let mut memo: Option<(usize, Vec<(usize, usize)>)> = None;
    while let Some(i) = find_from(xml, open, from) {
        from = i + 1;
        let pos = i + open.len();
        if !boundary_at(xml, pos) {
            continue;
        }
        let gt = gts.at(pos).unwrap_or(xml.len());
        if memo.as_ref().is_none_or(|(g, _)| *g != gt) {
            memo = Some((gt, docpr_id_starts(xml, pos, gt)));
        }
        let starts = memo.as_ref().map_or(&[][..], |(_, v)| v.as_slice());
        let Some(&(k, digits_end)) = starts.get(starts.partition_point(|&(k, _)| k < pos)) else { continue };
        out.push_str(xml.get(last..k + 4).unwrap_or(""));
        out.push_str(&uo_jsnum::format::to_js_string(new_id()));
        out.push('"');
        last = digits_end + 1;
        from = last;
    }
    out.push_str(xml.get(last..).unwrap_or(""));
    out
}

const WORD_ID_ATTRS: [&str; 4] = ["w14:paraId", "w14:textId", "wp14:anchorId", "wp14:editId"];

/// `xml.replace(/\s(?:w14:paraId|w14:textId|wp14:anchorId|wp14:editId)="[0-9A-Fa-f]+"/g, '')`.
fn strip_word_ids(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    let mut i = 0;
    while let Some(c) = xml.get(i..).and_then(|r| r.chars().next()) {
        let w = c.len_utf8();
        if is_js_space(c)
            && let Some(rest) = xml.get(i + w..)
            && let Some(end) = WORD_ID_ATTRS.iter().find_map(|name| attr_with_hex(rest, name))
        {
            out.push_str(xml.get(last..i).unwrap_or(""));
            last = i + w + end;
            i = last;
            continue;
        }
        i += w;
    }
    out.push_str(xml.get(last..).unwrap_or(""));
    out
}

/// `NAME="[0-9A-Fa-f]+"` at the start of `rest`: its length.
fn attr_with_hex(rest: &str, name: &str) -> Option<usize> {
    let after = rest.strip_prefix(name)?.strip_prefix("=\"")?;
    let n = after.bytes().take_while(u8::is_ascii_hexdigit).count();
    (n > 0 && after.as_bytes().get(n) == Some(&b'"')).then_some(name.len() + 2 + n + 1)
}

/// `copyHfPartXml(xml, newId)`: the copy of a part for unlinking: new ids on the drawings
/// (`wp:docPr`, as Word does), and without the Word 2010 paragraph and drawing ids
/// (`w14:paraId`/`textId`, `wp14:anchorId`/`editId`). `new_id` gives a free drawing id.
#[must_use]
pub fn copy_hf_part_xml(xml: &str, new_id: &mut dyn FnMut() -> f64) -> String {
    strip_word_ids(&renew_docpr_ids(xml, new_id))
}

/// `ct.replace(new RegExp(`<Override PartName="/${path}"[^>]*/>`), '')`, the path as a literal
/// (dynamic template, G9): the first such `Override` removed.
#[must_use]
pub fn remove_override(ct: &str, path: &str) -> String {
    let needle = format!("<Override PartName=\"/{path}\"");
    let mut from = 0;
    let mut gts = NextFind::new(ct, ">", false);
    while let Some(i) = find_from(ct, &needle, from) {
        from = i + 1;
        let pos = i + needle.len();
        let Some(gt) = gts.at(pos) else { break };
        if gt > pos && ct.as_bytes().get(gt - 1) == Some(&b'/') {
            return splice(ct, i, gt + 1, "");
        }
    }
    ct.to_owned()
}

/// `` `word/${target.replace(/^\.\//, '').replace(/^\/?word\//, '')}` ``: the part a header/footer
/// relationship target names.
#[must_use]
pub fn hf_target_path(target: &str) -> String {
    let t = target.strip_prefix("./").unwrap_or(target);
    let t = t.strip_prefix("/word/").or_else(|| t.strip_prefix("word/")).unwrap_or(t);
    format!("word/{t}")
}

/// `/relationships\/(?:header|footer)"/.test(tag)`.
#[must_use]
pub fn is_hf_relationship(tag: &str) -> bool {
    tag.contains("relationships/header\"") || tag.contains("relationships/footer\"")
}

/// The text half of `dropUnusedHfParts`: the document relationships and the content types without
/// the header/footer relationships whose id is in `drop` and their `Override`s, and the paths of the
/// parts they named, in order (the TS loop over the relationships of the original text, each tag
/// taken out by its first occurrence in the current text).
#[must_use]
pub fn drop_hf_text(rels: &str, ct: &str, drop: &HashSet<String>) -> (String, String, Vec<String>) {
    let mut rels = rels.to_owned();
    let mut ct = ct.to_owned();
    let mut gone: Vec<(String, String)> = Vec::new();
    for (a, b) in self_closing_matches(&rels, "Relationship") {
        let Some(tag) = rels.get(a..b) else { continue };
        if !is_hf_relationship(tag) {
            continue;
        }
        let (Some(id), Some(target)) = (quoted_attr(tag, "Id"), quoted_attr(tag, "Target")) else { continue };
        if !drop.contains(id) {
            continue;
        }
        gone.push((tag.to_owned(), hf_target_path(target)));
    }
    let mut paths = Vec::with_capacity(gone.len());
    for (tag, path) in gone {
        if let Some(i) = rels.find(&tag) {
            rels = splice(&rels, i, i + tag.len(), "");
        }
        ct = remove_override(&ct, &path);
        paths.push(path);
    }
    (rels, ct, paths)
}

/// `dropUnusedHfParts(pkg, drop)`: the header/footer parts no longer referenced are taken out of the
/// package, with the document relationship, the content type and the part's own relationships
/// (`drop`: the relationship ids of the document that went away). Media stay.
pub fn drop_unused_hf_parts(pkg: &mut Package, drop: &HashSet<String>) -> WResult<()> {
    let rels = text_of(pkg, DOC_RELS)?.unwrap_or_default();
    let ct = text_of(pkg, CONTENT_TYPES)?.unwrap_or_default();
    let (rels, ct, paths) = drop_hf_text(&rels, &ct, drop);
    for path in &paths {
        pkg.remove_part(path);
        let last = path.rsplit('/').next().unwrap_or("");
        pkg.remove_part(&format!("word/_rels/{last}.rels"));
    }
    if !paths.is_empty() {
        pkg.set_part_text(DOC_RELS, rels);
        if !ct.is_empty() {
            pkg.set_part_text(CONTENT_TYPES, ct);
        }
    }
    Ok(())
}

/// `/<w:docPartGallery\s+w:val="Page Numbers/.test(wrapOpen)`.
#[must_use]
pub fn is_page_number_gallery(wrap_open: &str) -> bool {
    let open = "<w:docPartGallery";
    let mut from = 0;
    while let Some(i) = find_from(wrap_open, open, from) {
        from = i + 1;
        let mut j = i + open.len();
        let mut spaces = 0;
        while let Some(w) = wrap_open.get(j..).and_then(space_at) {
            j += w;
            spaces += 1;
        }
        if spaces > 0 && wrap_open.get(j..).is_some_and(|r| r.starts_with("w:val=\"Page Numbers")) {
            return true;
        }
    }
    false
}

/// `pageNumberGroupIds(story)`: the `src` ids of the blocks inside the gallery page numbers (`w:sdt`
/// with `docPartGallery` "Page Numbers (...)") of a part read with sources, in the order of the TS
/// `Set` (depth first; ids that are falsy, 0 or NaN, are left out). `parts` are the store's parts
/// the raw texts resolve against. Iterative: the nesting of the groups is the model's.
#[must_use]
pub fn page_number_group_ids(story: Option<&StorySource>, parts: &[SourcePart]) -> IndexSet<NumKey> {
    let mut out = IndexSet::new();
    let Some(story) = story else { return out };
    let mut stack: Vec<(std::slice::Iter<'_, StoryGroup>, bool)> = vec![(story.groups.iter(), false)];
    while let Some((it, inside)) = stack.last_mut() {
        let inside = *inside;
        let Some(g) = it.next() else {
            stack.pop();
            continue;
        };
        let mine = inside || g.wrap_open.as_ref().is_some_and(|w| is_page_number_gallery(w.resolve(parts)));
        if mine {
            for id in &g.blocks {
                if id.truthy() {
                    out.insert(NumKey::new(id.to_number()));
                }
            }
        }
        if let Some(inner) = g.inner.as_ref() {
            stack.push((inner.iter(), mine));
        }
    }
    out
}
