//! D2b-F4: the regex sites of the watermark, cover page and header/footer template commands,
//! as hand matchers. Each one names the TS expression (`universal-write.ts` unless said) and is
//! proven against V8 by `tests/regex_d2b_f4.rs` over `js/regex-sites/d2b-f4.mjs`.

use uo_jsnum::math::max;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, trim};

/// `/<w:pict\b[\s\S]*<\/w:pict>/.exec(xml)?.[0]` (11892): from the first `<w:pict` that is not
/// followed by a word character to the LAST `</w:pict>` after it.
#[must_use]
pub fn pict_span(xml: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(off) = xml.get(from..).and_then(|s| s.find("<w:pict")) {
        let i = from + off;
        let after = i + 7;
        let word_next = xml.get(after..).and_then(|s| s.chars().next()).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if !word_next {
            let tail = xml.get(after..)?;
            let last = tail.rfind("</w:pict>")?;
            return xml.get(i..after + last + 9);
        }
        from = i + 1;
    }
    None
}

/// `/^word\/(header|footer)\d*\.xml$/.test(n)` (11880).
#[must_use]
pub fn is_hf_part_name(n: &str) -> bool {
    let Some(rest) = n.strip_prefix("word/") else { return false };
    let Some(rest) = rest.strip_prefix("header").or_else(|| rest.strip_prefix("footer")) else { return false };
    let Some(digits) = rest.strip_suffix(".xml") else { return false };
    digits.bytes().all(|b| b.is_ascii_digit())
}

/// The loop of `watermarkRun` (11881): `spid = 1025`, then for every `/_x0000_s(\d+)/g` match of
/// `used`, `spid = Math.max(spid, parseInt(m, 10) + 1)`.
#[must_use]
pub fn next_spid(used: &str) -> f64 {
    let mut spid = 1025.0;
    let key = "_x0000_s";
    let mut from = 0;
    while let Some(off) = used.get(from..).and_then(|s| s.find(key)) {
        let start = from + off + key.len();
        let digits: &str = {
            let rest = used.get(start..).unwrap_or("");
            let n = rest.bytes().take_while(u8::is_ascii_digit).count();
            rest.get(..n).unwrap_or("")
        };
        if digits.is_empty() {
            from += off + 1;
            continue;
        }
        spid = max(spid, parse_int(digits, 10) + 1.0);
        from = start + digits.len();
    }
    spid
}

/// `/<w:docPartGallery\s+w:val="Cover Pages"/.test(wrapOpen)` (11973); `\s` is the JS white space.
#[must_use]
pub fn has_cover_gallery(s: &str) -> bool {
    let key = "<w:docPartGallery";
    let mut from = 0;
    while let Some(off) = s.get(from..).and_then(|x| x.find(key)) {
        let i = from + off;
        let rest = s.get(i + key.len()..).unwrap_or("");
        let trimmed = rest.trim_start_matches(is_js_space);
        if trimmed.len() < rest.len() && trimmed.starts_with("w:val=\"Cover Pages\"") {
            return true;
        }
        from = i + 1;
    }
    false
}

/// `prop(xml, tag)` of `coverEnv` (12010) WITHOUT the final `.trim()`:
/// `new RegExp('<' + tag + '[^>]*>([^<]*)<').exec(xml)` and, when it matches, the entities
/// `&lt; &gt; &quot; &amp;` replaced in that order; `None` when it does not match. `tag` has no regex
/// meta characters (the TS passes `dc:title`, `Company`…). F10's `insertDocumentInfo` uses the same
/// expression.
#[must_use]
pub fn xml_prop(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    // Only the FIRST `<tag` can match: a later one finds its `>` at or after this one's, so when this one
    // has no `>` (or no `<` after the `>`), no later one has either. No rescanning: linear.
    let i = xml.find(&open)?;
    let after = i + open.len();
    let gt = xml.get(after..)?.find('>')?;
    let text_start = after + gt + 1;
    let lt = xml.get(text_start..)?.find('<')?;
    let cap = xml.get(text_start..text_start + lt)?;
    Some(cap.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&"))
}

/// `xml_prop(..).trim()` or `''`: the `prop` closure of `coverEnv`.
#[must_use]
pub fn xml_prop_trimmed(xml: &str, tag: &str) -> String {
    xml_prop(xml, tag).map(|s| trim(&s).to_owned()).unwrap_or_default()
}

/// `/^\[.*\]$/.test(t)` (12058): `.` excludes `\n`, `\r`, U+2028 and U+2029.
#[must_use]
pub fn is_bracketed(t: &str) -> bool {
    let mut it = t.chars();
    if it.next() != Some('[') {
        return false;
    }
    let body: Vec<char> = it.collect();
    let Some((last, mid)) = body.split_last() else { return false };
    *last == ']' && mid.iter().all(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

/// `s.replace(/^#/, '')` (11502, 12005).
#[must_use]
pub fn strip_hash(s: &str) -> &str {
    s.strip_prefix('#').unwrap_or(s)
}

/// `/NAME/i.test(field)` for an upper-case ASCII `name` (11587): the non-unicode `i` flag folds
/// ASCII only.
#[must_use]
pub fn contains_ci_ascii(field: &str, name: &str) -> bool {
    let (f, n) = (field.as_bytes(), name.as_bytes());
    if n.is_empty() {
        return true;
    }
    f.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}
