//! The regex sites of the writer's core (G9, U7): hand matchers, linear, each the exact behaviour
//! of the TS `RegExp` on any text (checked against V8 by `js/regex-sites/d1b-core.mjs` and
//! `tests/regex_d1b_core.rs`). Two kinds:
//!
//! - **dynamic templates**, for the patterns the TS builds at run time from document values:
//!   - [`TagPattern`]: `<NAME\b[^>]*ATTR="(?:v1|v2|…)"<tail>` with the values matched as
//!     **literals** (the TS interpolates them unescaped: a value holding a metacharacter changes
//!     the match or throws `SyntaxError` there — accepted by name, G9). Sites: `dropCommentMarks`
//!     (serialize.ts:1449–1450), `patchComments` (1572, 1579, 1583), and for the emitter frontiers
//!     `page-background.ts:189/228`, `hf-ops.ts:71`, `notes.ts:105`, `settings-xml.ts:84`;
//!   - [`field_code_groups`]: `fieldCodeXml`'s `^lit([\s\S]*?)lit([\s\S]*?)…lit$`
//!     (serialize.ts:627–628) as a bounded matcher: the lexicographically smallest group lengths
//!     (what backtracking finds first) in O(k·n), with a work budget — V8 is O(n^k);
//!   - [`find_literal_ci`]: `new RegExp(literal, 'i')` without `u` (serialize.ts:183,
//!     page-background.ts:212): `Canonicalize` per UTF-16 unit;
//! - **static sites** of the core (the namespaces of a part root, revision ids, numbering ids,
//!   the table grid, the `.rels` scans, the `w:txbxContent` replacement).
//!
//! Offsets are UTF-8 byte offsets; every match starts and ends on a character boundary (each
//! pattern begins and ends with ASCII or a whole literal), so they are the TS UTF-16 matches.

use uo_jsnum::text::is_js_space;

pub use super::fonts::ooxml::{find_from, is_word_byte};

/// The JS `\w` test of the character at byte `at` (false at the end or inside a character).
fn word_at(s: &str, at: usize) -> bool {
    s.as_bytes().get(at).is_some_and(|&b| is_word_byte(b))
}

/// `\b` at byte `at`: exactly one of the bytes before and at `at` is `\w` (ASCII; a multi-byte
/// character is never `\w` without `u`).
#[must_use]
pub fn boundary_at(s: &str, at: usize) -> bool {
    let before = at > 0 && word_at(s, at - 1);
    before != word_at(s, at)
}

/// Whether the character ending right before byte `at` is JS white space (`\s`).
fn space_before(s: &str, at: usize) -> bool {
    s.get(..at).and_then(|h| h.chars().next_back()).is_some_and(is_js_space)
}

/// Skips JS white space (`\s*`) from `at`.
fn skip_space(s: &str, mut at: usize) -> usize {
    while let Some(c) = s.get(at..).and_then(|r| r.chars().next()) {
        if !is_js_space(c) {
            break;
        }
        at += c.len_utf8();
    }
    at
}

// ───────────────────────────── the element-by-attribute template ─────────────────────────────

/// What follows the attribute value in a [`TagPattern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail<'a> {
    /// `[^>]*\/>`: the tag must close with `/>`
    SelfClose,
    /// `[^>]*?(?:\/>|>[\s\S]*?<\/NAME>)`: self-closing, or the element up to the first `</NAME>`
    SelfCloseOrElement(&'a str),
}

/// `<NAME\b[^>]*ATTR="(?:v1|…)"<tail>` (see the module comment). `names` are the alternatives
/// after `<` in order (`<w:commentRange(?:Start|End)\b` is `["w:commentRangeStart",
/// "w:commentRangeEnd"]` with `name_boundary`); `attr_boundary` is a `\b` before `ATTR`.
///
/// Values are literals. A value holding `"` or `>` never matches here (in the TS it would match
/// across the tag's first `>`): such values cannot come from the callers (numbers, `[0-9A-F]+`,
/// `[^"]+` captures of the same files) except a `>` inside a `durableId` — accepted by name.
#[derive(Debug, Clone, Copy)]
pub struct TagPattern<'a> {
    pub names: &'a [&'a str],
    pub name_boundary: bool,
    pub attr: &'a str,
    pub attr_boundary: bool,
    pub values: &'a [&'a str],
    pub tail: Tail<'a>,
}

impl TagPattern<'_> {
    /// Whether some attribute occurrence `ATTR="v"` with `v` among the values starts at `p`.
    fn attr_matches(&self, s: &str, p: usize) -> bool {
        let Some(rest) = s.get(p..) else { return false };
        if !rest.starts_with(self.attr) {
            return false;
        }
        if self.attr_boundary && !boundary_at(s, p) {
            return false;
        }
        let Some(after) = rest.get(self.attr.len()..).and_then(|r| r.strip_prefix("=\"")) else { return false };
        let Some(q) = after.find('"') else { return false };
        let value = &after[..q];
        self.values.iter().any(|v| *v == value && !v.contains('>'))
    }
}

/// All the matches of a [`TagPattern`], left to right, non-overlapping (a `g` regex): byte ranges.
#[must_use]
pub fn tag_matches(s: &str, pat: &TagPattern<'_>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if pat.attr.is_empty() || pat.names.is_empty() {
        return out;
    }
    // per tag end (`>`): the largest valid attribute start before it, computed once (every
    // candidate that ends its `[^>]*` at the same `>` shares it), so the scan stays linear
    let mut region: Option<(usize, Option<usize>)> = None;
    let mut close_memo: Option<(usize, Option<usize>)> = None;
    let mut from = 0;
    let mut gts = super::fonts::ooxml::NextFind::new(s, ">", false);
    while let Some(lt) = find_from(s, "<", from) {
        from = lt + 1;
        let Some(pos) = pat.names.iter().find_map(|n| {
            let end = lt + 1 + n.len();
            (s.get(lt + 1..end) == Some(*n) && (!pat.name_boundary || boundary_at(s, end))).then_some(end)
        }) else {
            continue;
        };
        let Some(gt) = gts.at(pos) else { break };
        let best = match region {
            Some((g, b)) if g == gt => b,
            _ => {
                let b = last_valid_attr(s, pat, lt + 1, gt);
                region = Some((gt, b));
                b
            }
        };
        if !best.is_some_and(|p| p >= pos) {
            continue;
        }
        let end = match pat.tail {
            Tail::SelfClose => (s.as_bytes().get(gt - 1) == Some(&b'/')).then_some(gt + 1),
            Tail::SelfCloseOrElement(name) => {
                if s.as_bytes().get(gt - 1) == Some(&b'/') {
                    Some(gt + 1)
                } else {
                    let close = format!("</{name}>");
                    let hit = match close_memo {
                        Some((x, h)) if x <= gt + 1 && h.is_none_or(|h| gt < h) => h,
                        _ => {
                            let h = find_from(s, &close, gt + 1);
                            close_memo = Some((gt + 1, h));
                            h
                        }
                    };
                    hit.map(|h| h + close.len())
                }
            }
        };
        if let Some(end) = end {
            out.push((lt, end));
            from = end;
        }
    }
    out
}

/// The largest `p` in `[start, gt)` where the attribute (with its value condition) matches.
fn last_valid_attr(s: &str, pat: &TagPattern<'_>, start: usize, gt: usize) -> Option<usize> {
    let region = s.get(start..gt)?;
    let mut best = None;
    let mut k = 0;
    while let Some(i) = region.get(k..).and_then(|r| r.find(pat.attr)) {
        let p = start + k + i;
        if pat.attr_matches(s, p) {
            best = Some(p);
        }
        k += i + 1;
        while !region.is_char_boundary(k) {
            k += 1;
        }
    }
    best
}

/// `s.replace(re, f)` for a global [`TagPattern`]: every match replaced by `f(match)`.
pub fn replace_tags(s: &str, pat: &TagPattern<'_>, mut f: impl FnMut(&str) -> String) -> String {
    let ms = tag_matches(s, pat);
    if ms.is_empty() {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b) in ms {
        out.push_str(&s[last..a]);
        out.push_str(&f(&s[a..b]));
        last = b;
    }
    out.push_str(&s[last..]);
    out
}

/// `s.replace(re, '')` for a global [`TagPattern`].
#[must_use]
pub fn remove_tags(s: &str, pat: &TagPattern<'_>) -> String {
    replace_tags(s, pat, |_| String::new())
}

/// Every `<NAME\b[^>]*?(?:\/>|>[\s\S]*?<\/NAME>)` (global, non-overlapping): the element, self-
/// closing or up to the first `</NAME>` after its start tag (`writeTrackRevisions`, and the
/// `settings-xml.ts:84` / `notes.ts:105` shapes for the emitter frontiers).
#[must_use]
pub fn element_matches(s: &str, name: &str) -> Vec<(usize, usize)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut from = 0;
    let mut gts = super::fonts::ooxml::NextFind::new(s, ">", false);
    let mut close_memo: Option<(usize, Option<usize>)> = None;
    while let Some(i) = find_from(s, &open, from) {
        from = i + 1;
        let pos = i + open.len();
        if !boundary_at(s, pos) {
            continue;
        }
        let Some(gt) = gts.at(pos) else { break };
        let end = if gt > pos && s.as_bytes().get(gt - 1) == Some(&b'/') {
            Some(gt + 1)
        } else {
            let hit = match close_memo {
                Some((x, h)) if x <= gt + 1 && h.is_none_or(|h| gt < h) => h,
                _ => {
                    let h = find_from(s, &close, gt + 1);
                    close_memo = Some((gt + 1, h));
                    h
                }
            };
            hit.map(|h| h + close.len())
        };
        if let Some(end) = end {
            out.push((i, end));
            from = end;
        }
    }
    out
}

/// `s.replace(/<NAME\b[^>]*?(?:\/>|>[\s\S]*?<\/NAME>)/g, '')`.
#[must_use]
pub fn remove_elements_lazy(s: &str, name: &str) -> String {
    let ms = element_matches(s, name);
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b) in ms {
        out.push_str(&s[last..a]);
        last = b;
    }
    out.push_str(&s[last..]);
    out
}

// ───────────────────────────── dropCommentMarks (serialize.ts:1445–1451) ─────────────────────────────

/// `<w:commentRange(?:Start|End)\b[^>]*\bw:id="(?:ids)"[^>]*/>`.
#[must_use]
pub fn comment_range_pattern<'a>(ids: &'a [&'a str]) -> TagPattern<'a> {
    TagPattern {
        names: &["w:commentRangeStart", "w:commentRangeEnd"],
        name_boundary: true,
        attr: "w:id",
        attr_boundary: true,
        values: ids,
        tail: Tail::SelfClose,
    }
}

/// The second regex of `dropCommentMarks`:
/// `<w:r\b[^>]*>(?:(?!</w:r>)[\s\S])*?<w:commentReference\b[^>]*\bw:id="(?:ids)"[^>]*/>(?:(?!</w:r>)[\s\S])*?</w:r>`
/// (global): a run, from its start tag to the first `</w:r>` after it, holding a matching
/// `w:commentReference` tag before that `</w:r>`.
#[must_use]
pub fn comment_reference_runs(s: &str, ids: &[&str]) -> Vec<(usize, usize)> {
    const CLOSE: &str = "</w:r>";
    let refpat = TagPattern {
        names: &["w:commentReference"],
        name_boundary: true,
        attr: "w:id",
        attr_boundary: true,
        values: ids,
        tail: Tail::SelfClose,
    };
    // the matching reference tags, once (start, end); a run region holds one when the first
    // reference starting at or after its content start ends before its `</w:r>`
    let refs = tag_matches_all_starts(s, &refpat);
    let mut out = Vec::new();
    let mut from = 0;
    let mut ri = 0;
    // the queries only grow: memoised, so the scan for `>` and `</w:r>` runs once in total
    let mut gts = super::fonts::ooxml::NextFind::new(s, ">", false);
    let mut closes = super::fonts::ooxml::NextFind::new(s, CLOSE, false);
    while let Some(lt) = find_from(s, "<w:r", from) {
        from = lt + 1;
        if !boundary_at(s, lt + 4) {
            continue;
        }
        let Some(gt) = gts.at(lt + 4) else { break };
        let Some(c) = closes.at(gt + 1) else { break };
        while refs.get(ri).is_some_and(|&(a, _)| a < gt + 1) {
            ri += 1;
        }
        // references are disjoint and sorted: the first one at or after the content start
        if let Some(&(a, b)) = refs.get(ri)
            && a < c
            && b <= c
        {
            out.push((lt, c + CLOSE.len()));
            from = c + CLOSE.len();
        }
    }
    out
}

/// Every start where a [`TagPattern`] matches (not only the non-overlapping ones of a global
/// replace): for the `(?:…)*?` lookups inside another pattern. Sorted.
fn tag_matches_all_starts(s: &str, pat: &TagPattern<'_>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    let mut gts = super::fonts::ooxml::NextFind::new(s, ">", false);
    let mut region: Option<(usize, Option<usize>)> = None;
    while let Some(lt) = find_from(s, "<", from) {
        from = lt + 1;
        let Some(pos) = pat.names.iter().find_map(|n| {
            let end = lt + 1 + n.len();
            (s.get(lt + 1..end) == Some(*n) && (!pat.name_boundary || boundary_at(s, end))).then_some(end)
        }) else {
            continue;
        };
        let Some(gt) = gts.at(pos) else { break };
        let best = match region {
            Some((g, b)) if g == gt => b,
            _ => {
                let b = last_valid_attr(s, pat, lt + 1, gt);
                region = Some((gt, b));
                b
            }
        };
        if best.is_some_and(|p| p >= pos) && s.as_bytes().get(gt - 1) == Some(&b'/') {
            out.push((lt, gt + 1));
        }
    }
    out
}

/// `dropCommentMarks(xml, gone)`: the raw marks of the deleted comments removed (`ids` are the
/// `String(n)` of the ids, in `Set` order; order does not change the result).
#[must_use]
pub fn drop_comment_marks(xml: &str, ids: &[&str]) -> String {
    if ids.is_empty() {
        return xml.to_owned();
    }
    let a = remove_tags(xml, &comment_range_pattern(ids));
    let runs = comment_reference_runs(&a, ids);
    if runs.is_empty() {
        return a;
    }
    let mut out = String::with_capacity(a.len());
    let mut last = 0;
    for (x, y) in runs {
        out.push_str(&a[last..x]);
        last = y;
    }
    out.push_str(&a[last..]);
    out
}

// ───────────────────────────── fieldCodeXml (serialize.ts:607–636) ─────────────────────────────

/// A first-level part of a field code (`codeParts`): text, or a nested field's code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodePart<'a> {
    Text(&'a str),
    Nested(&'a str),
}

/// `codeParts(code)`: the text and the first-level `{ … }` of a field code. Braces count by
/// code unit (`{`, `}` are ASCII, so bytes here).
#[must_use]
pub fn code_parts(code: &str) -> Vec<CodePart<'_>> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut from = 0;
    for (i, b) in code.bytes().enumerate() {
        if b == b'{' {
            if depth == 0 {
                if i > from {
                    out.push(CodePart::Text(&code[from..i]));
                }
                from = i + 1;
            }
            depth += 1;
        } else if b == b'}' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                out.push(CodePart::Nested(&code[from..i]));
                from = i + 1;
            }
        }
    }
    if from < code.len() {
        out.push(CodePart::Text(&code[from..]));
    }
    out
}

/// The work allowed to [`field_code_groups`], in bytes scanned (64 MiB): past it the cached
/// results are not aligned, as when the flat instruction does not fit the code.
pub const FIELD_CODE_BUDGET: usize = 64 << 20;

/// `new RegExp('^' + reSrc + '$').exec(flat.trim())` of `fieldCodeXml`: the pattern is the
/// parts in order, each text part an escaped literal and each nested part a lazy `([\s\S]*?)`.
/// Returns the groups (one per nested part, in order), or `None` when there is no match.
///
/// Backtracking finds the lexicographically smallest tuple of group lengths for which the whole
/// subject matches; computed here with one right-to-left pass (`limit[i]` = the largest start of
/// literal `i` from which the rest can still match) and one left-to-right pass (each literal at
/// its first occurrence that keeps the rest feasible). Consecutive groups are separated by an
/// empty literal. O(k·n) for k literals; `None` past [`FIELD_CODE_BUDGET`] (declared).
#[must_use]
pub fn field_code_groups<'s>(parts: &[CodePart<'_>], subject: &'s str) -> Option<Vec<&'s str>> {
    // literals L0 G1 L1 … Gk Lk (Li possibly empty)
    let mut lits: Vec<&str> = vec![""];
    for p in parts {
        match p {
            CodePart::Text(t) => {
                if let Some(last) = lits.last_mut() {
                    // two text parts never follow each other in `codeParts`; joined if they did
                    if last.is_empty() {
                        *last = t;
                    } else {
                        return join_literals_fallback(parts, subject);
                    }
                }
            }
            CodePart::Nested(_) => lits.push(""),
        }
    }
    let k = lits.len() - 1;
    let n = subject.len();
    if k.saturating_mul(n).saturating_mul(2) > FIELD_CODE_BUDGET {
        return None;
    }
    let first = lits.first().copied().unwrap_or("");
    if !subject.starts_with(first) {
        return None;
    }
    if k == 0 {
        return (first.len() == n).then(Vec::new);
    }
    // limit[i]: the largest feasible start of literal i (i ≥ 1)
    let mut limit = vec![0usize; k + 1];
    let last = lits[k];
    if last.len() > n || !subject.ends_with(last) {
        return None;
    }
    limit[k] = n - last.len();
    for i in (1..k).rev() {
        let li = lits[i];
        let bound = limit[i + 1].checked_sub(li.len())?;
        // the last occurrence of li starting at or before `bound`
        let hay = subject.get(..bound + li.len())?;
        limit[i] = hay.rfind(li)?;
    }
    if limit[1] < first.len() {
        return None;
    }
    let mut groups = Vec::with_capacity(k);
    let mut end = first.len();
    for i in 1..=k {
        let li = lits[i];
        let start = if i == k {
            limit[k]
        } else {
            let hit = subject.get(end..)?.find(li)? + end;
            if hit > limit[i] {
                return None;
            }
            hit
        };
        if start < end {
            return None;
        }
        groups.push(subject.get(end..start)?);
        end = start + li.len();
    }
    Some(groups)
}

/// Never reached through `codeParts` (it never yields two text parts in a row); kept total.
fn join_literals_fallback<'s>(parts: &[CodePart<'_>], subject: &'s str) -> Option<Vec<&'s str>> {
    let mut merged: Vec<CodePart<'_>> = Vec::new();
    let mut owned: Vec<String> = Vec::new();
    let mut cur = String::new();
    for p in parts {
        match p {
            CodePart::Text(t) => cur.push_str(t),
            CodePart::Nested(c) => {
                owned.push(std::mem::take(&mut cur));
                merged.push(CodePart::Nested(c));
            }
        }
    }
    owned.push(cur);
    let mut parts2: Vec<CodePart<'_>> = Vec::new();
    for (i, t) in owned.iter().enumerate() {
        if !t.is_empty() {
            parts2.push(CodePart::Text(t));
        }
        if let Some(n) = merged.get(i) {
            parts2.push(n.clone());
        }
    }
    field_code_groups(&parts2, subject)
}

// ───────────────────────────── case-insensitive literal (`/i`, no `u`) ─────────────────────────────

/// Whether `hay` holds `needle` under a `RegExp` with `i` and without `u` (`Canonicalize` per
/// UTF-16 code unit, uo-text): `new RegExp(needle, 'i').test(hay)` for a needle without
/// metacharacters (dynamic values: metacharacters accepted by name).
#[must_use]
pub fn find_literal_ci(hay: &str, needle: &str) -> bool {
    use uo_text::case::regexp_canonicalize as canon;
    let n: Vec<u16> = needle.encode_utf16().map(canon).collect();
    if n.is_empty() {
        return true;
    }
    let h: Vec<u16> = hay.encode_utf16().map(canon).collect();
    if h.len() < n.len() {
        return false;
    }
    // KMP over the canonical units: linear
    let mut fail = vec![0usize; n.len()];
    let mut j = 0;
    for i in 1..n.len() {
        while j > 0 && n[i] != n[j] {
            j = fail[j - 1];
        }
        if n[i] == n[j] {
            j += 1;
        }
        fail[i] = j;
    }
    j = 0;
    for &u in &h {
        while j > 0 && u != n[j] {
            j = fail[j - 1];
        }
        if u == n[j] {
            j += 1;
            if j == n.len() {
                return true;
            }
        }
    }
    false
}

// ───────────────────────────── static sites of the core ─────────────────────────────

/// `new RegExp('\\sxmlns:' + pfx + '=').test(tag)` (serialize.ts:1440, page-background.ts:222;
/// `pfx` is a constant prefix).
#[must_use]
pub fn has_xmlns(tag: &str, pfx: &str) -> bool {
    let needle = format!("xmlns:{pfx}=");
    let mut from = 0;
    while let Some(i) = find_from(tag, &needle, from) {
        if space_before(tag, i) {
            return true;
        }
        from = i + 1;
    }
    false
}

/// `/\s<pfx>\w+=/` (`\so:\w+=`, `\sw14:\w+=`, `\swp14:\w+=` of `withNamespaces`): white space,
/// the prefix, one or more word characters, `=`.
#[must_use]
pub fn has_prefixed_attr(s: &str, pfx: &str) -> bool {
    let mut from = 0;
    while let Some(i) = find_from(s, pfx, from) {
        from = i + 1;
        if !space_before(s, i) {
            continue;
        }
        let b = s.as_bytes();
        let mut j = i + pfx.len();
        let start = j;
        while b.get(j).is_some_and(|&c| is_word_byte(c)) {
            j += 1;
        }
        if j > start && b.get(j) == Some(&b'=') {
            return true;
        }
    }
    false
}

/// `beginRevisionIds`: the largest `Number(m[1])` of `/\bw:id="(-?\d+)"/g` over the texts, with
/// 0 as the start (`Math.max`; ids are integers, so no NaN).
#[must_use]
pub fn max_w_id(texts: &[&str]) -> f64 {
    let mut max = 0.0f64;
    for x in texts {
        let mut from = 0;
        while let Some(i) = find_from(x, "w:id=\"", from) {
            from = i + 1;
            if !boundary_at(x, i) {
                continue;
            }
            let v = &x[i + 6..];
            let digits_start = usize::from(v.starts_with('-'));
            let d = v[digits_start..].bytes().take_while(u8::is_ascii_digit).count();
            if d == 0 || v.as_bytes().get(digits_start + d) != Some(&b'"') {
                continue;
            }
            let n = uo_jsnum::parse::to_number(&v[..digits_start + d]);
            max = uo_jsnum::math::max(max, n);
            // the regex continues after the closing quote
            from = i + 6 + digits_start + d + 1;
        }
    }
    max
}

/// `freeNumIds(xml)`: the first pair `n, n + 1` (from 1001, by twos) that no
/// `/w:(?:abstractNumId|numId)(?: w:val)?="(\d+)"/g` value uses.
#[must_use]
pub fn free_num_ids(xml: Option<&str>) -> (f64, f64) {
    let mut used: Vec<f64> = Vec::new();
    let x = xml.unwrap_or("");
    let mut from = 0;
    while let Some(i) = find_from(x, "w:", from) {
        from = i + 1;
        let rest = &x[i + 2..];
        let name_len = if rest.starts_with("abstractNumId") {
            13
        } else if rest.starts_with("numId") {
            5
        } else {
            continue;
        };
        let mut j = i + 2 + name_len;
        // (?: w:val)? is greedy: try with it, then without
        let try_at = |j: usize| -> Option<usize> {
            let r = x.get(j..)?.strip_prefix("=\"")?;
            let d = r.bytes().take_while(u8::is_ascii_digit).count();
            (d > 0 && r.as_bytes().get(d) == Some(&b'"')).then_some(d)
        };
        let mut hit = None;
        if x.get(j..).is_some_and(|r| r.starts_with(" w:val"))
            && let Some(d) = try_at(j + 6)
        {
            hit = Some((j + 6, d));
        }
        if hit.is_none() {
            hit = try_at(j).map(|d| (j, d));
        }
        if let Some((at, d)) = hit {
            used.push(uo_jsnum::parse::to_number(&x[at + 2..at + 2 + d]));
            j = at + 2 + d + 1;
            from = j;
        }
    }
    let has = |v: f64| used.contains(&v);
    let mut n = 1001.0;
    while has(n) || has(n + 1.0) {
        n += 2.0;
    }
    (n, n + 1.0)
}

/// `gridOf(raw)`: `[...raw.matchAll(/<w:gridCol\b[^>]*w:w="(\d+)"/g)].map(m => m[1]).join(',')`.
/// The greedy `[^>]*` makes each match take the **last** `w:w="digits"` before the tag's `>`.
#[must_use]
pub fn grid_of(raw: &str) -> String {
    const OPEN: &str = "<w:gridCol";
    let mut out: Vec<&str> = Vec::new();
    let mut from = 0;
    let mut gts = super::fonts::ooxml::NextFind::new(raw, ">", false);
    // per region end: the last valid `w:w="digits"` of the region, computed once (candidates that
    // share the region share it: the greedy `[^>]*` takes the last one at or after their start)
    let mut region_best: Option<(usize, Option<(usize, usize)>)> = None;
    while let Some(i) = find_from(raw, OPEN, from) {
        from = i + 1;
        let pos = i + OPEN.len();
        if !boundary_at(raw, pos) {
            continue;
        }
        // `[^>]*` runs to the first `>` (or the end); the attribute must end before it
        let lim = gts.at(pos).unwrap_or(raw.len());
        let last = match region_best {
            Some((l, b)) if l == lim => b,
            _ => {
                let region = &raw[pos..lim];
                let mut best: Option<(usize, usize)> = None;
                let mut k = 0;
                while let Some(j) = region.get(k..).and_then(|r| r.find("w:w=\"")) {
                    let a = k + j + 5;
                    let d = region[a..].bytes().take_while(u8::is_ascii_digit).count();
                    if d > 0 && region.as_bytes().get(a + d) == Some(&b'"') {
                        best = Some((pos + a, d));
                    }
                    k += j + 1;
                }
                region_best = Some((lim, best));
                best
            }
        };
        let best = last.filter(|&(a, _)| a >= pos + 5);
        if let Some((a, d)) = best {
            out.push(&raw[a..a + d]);
            from = a + d + 1;
        }
    }
    out.join(",")
}

/// A `/<Relationship\b[^>]*>/g` tag of a `.rels` text (byte range).
#[must_use]
pub fn relationship_tags(xml: &str) -> Vec<(usize, usize)> {
    super::fonts::ooxml::read_rels_text(xml).into_iter().map(|t| (t.start, t.end)).collect()
}

/// `/\bId="([^"]+)"/.exec(tag)?.[1]`: the first non-empty `Id` value preceded by a non-word
/// character.
#[must_use]
pub fn rel_id_of(tag: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(i) = find_from(tag, "Id=\"", from) {
        from = i + 1;
        if !boundary_at(tag, i) {
            continue;
        }
        let v = &tag[i + 4..];
        if let Some(q) = v.find('"')
            && q > 0
        {
            return Some(&v[..q]);
        }
    }
    None
}

/// `/^word\/_rels\/[^/]+$/.test(path)`.
#[must_use]
pub fn is_word_rels_path(path: &str) -> bool {
    path.strip_prefix("word/_rels/").is_some_and(|r| !r.is_empty() && !r.contains('/'))
}

/// `/^<NAME\s*\/>$/.test(xml)`: an empty self-closing element (`w:ind`, `w:spacing`,
/// `w:pgNumType`).
#[must_use]
pub fn is_empty_element(xml: &str, name: &str) -> bool {
    let Some(rest) = xml.strip_prefix('<').and_then(|r| r.strip_prefix(name)) else { return false };
    let j = skip_space(rest, 0);
    &rest[j..] == "/>"
}

/// `raw.replace(/\s+w:(?:fmt|start|chapStyle|chapSep)="[^"]*"/g, '')` (`pgNumTypeXml`).
#[must_use]
pub fn strip_pg_num_attrs(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    let mut copied = 0;
    while i < raw.len() {
        let Some(c) = raw[i..].chars().next() else { break };
        if !is_js_space(c) {
            i += c.len_utf8();
            continue;
        }
        // the longest run of white space starting here
        let ws_end = skip_space(raw, i);
        let rest = &raw[ws_end..];
        let name = ["w:fmt", "w:start", "w:chapStyle", "w:chapSep"].iter().find(|n| rest.starts_with(**n)).copied();
        if let Some(n) = name
            && let Some(v) = rest[n.len()..].strip_prefix("=\"")
            && let Some(q) = v.find('"')
        {
            let end = ws_end + n.len() + 2 + q + 1;
            out.push_str(&raw[copied..i]);
            copied = end;
            i = end;
            continue;
        }
        i = ws_end.max(i + c.len_utf8());
    }
    out.push_str(&raw[copied..]);
    out
}

/// `xml.replace(/<w:txbxContent\b[^>]*>[\s\S]*?<\/w:txbxContent>/g, m => …)` with
/// `/^<w:txbxContent\b[^>]*>/.exec(m)[0]` (the same start tag): each content replaced by
/// `f(k)` (the k-th match), keeping its start tag.
pub fn replace_txbx_contents(xml: &str, mut f: impl FnMut(usize) -> String) -> String {
    const OPEN: &str = "<w:txbxContent";
    const CLOSE: &str = "</w:txbxContent>";
    let mut gts = super::fonts::ooxml::NextFind::new(xml, ">", false);
    let mut closes = super::fonts::ooxml::NextFind::new(xml, CLOSE, false);
    let mut out = String::new();
    let mut last = 0;
    let mut from = 0;
    let mut k = 0;
    while let Some(i) = find_from(xml, OPEN, from) {
        from = i + 1;
        if !boundary_at(xml, i + OPEN.len()) {
            continue;
        }
        let Some(gt) = gts.at(i + OPEN.len()) else { break };
        let Some(c) = closes.at(gt + 1) else { break };
        out.push_str(&xml[last..i]);
        out.push_str(&xml[i..=gt]);
        out.push_str(&f(k));
        out.push_str(CLOSE);
        k += 1;
        last = c + CLOSE.len();
        from = last;
    }
    if k == 0 {
        return xml.to_owned();
    }
    out.push_str(&xml[last..]);
    out
}

/// `(xml.match(/<w:txbxContent\b/g) ?? []).length`.
#[must_use]
pub fn count_txbx_contents(xml: &str) -> usize {
    const OPEN: &str = "<w:txbxContent";
    let mut n = 0;
    let mut from = 0;
    while let Some(i) = find_from(xml, OPEN, from) {
        if boundary_at(xml, i + OPEN.len()) {
            n += 1;
        }
        from = i + OPEN.len();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_code_groups_are_the_lazy_ones() {
        let parts = code_parts(" IF { PAGE } = { NUMPAGES } \"last\" \"\" ");
        assert_eq!(parts.len(), 5);
        let g = field_code_groups(&parts, " IF 3 = 7 \"last\" \"\" ").unwrap_or_default();
        assert_eq!(g, ["3", "7"]);
        // consecutive groups: the first takes nothing
        let parts = code_parts("{A}{B}");
        assert_eq!(field_code_groups(&parts, "xy"), Some(vec!["", "xy"]));
        // no match
        let parts = code_parts("A{B}C");
        assert_eq!(field_code_groups(&parts, "AxD"), None);
        // the first group as short as possible with a literal repeated
        let parts = code_parts("a{x}b{y}b");
        assert_eq!(field_code_groups(&parts, "a1b2b3b"), Some(vec!["1", "2b3"]));
    }

    #[test]
    fn drop_comment_marks_like_the_regexes() {
        let xml = r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>x</w:t></w:r><w:commentRangeEnd w:id="1"/><w:r><w:rPr/><w:commentReference w:id="1"/></w:r><w:commentRangeStart w:id="12"/></w:p>"#;
        let out = drop_comment_marks(xml, &["1"]);
        assert_eq!(out, r#"<w:p><w:r><w:t>x</w:t></w:r><w:commentRangeStart w:id="12"/></w:p>"#);
    }

    #[test]
    fn free_num_ids_skip_used_pairs() {
        assert_eq!(free_num_ids(None), (1001.0, 1002.0));
        assert_eq!(free_num_ids(Some(r#"<w:num w:numId="1002"><w:abstractNumId w:val="7"/></w:num>"#)), (1003.0, 1004.0));
    }
}
