//! The regular expressions of the clipboard family as hand matchers (D2b-F8), each with the TS
//! expression it replaces (`paste-html.ts`, `universal-write.ts`). The V8 rows that check them
//! are `js/regex-sites/d2b-f8.mjs` (`tests/regex_d2b_f8.rs`).
//!
//! JS semantics kept: `\s` is JS white space (`uo_jsnum::text::is_js_space_unit`), `\d` and `\w`
//! are ASCII, `/i` without `/u` folds ASCII letters only (a non-ASCII character never matches an
//! ASCII letter), `.` is one UTF-16 unit other than a line terminator, and the strings the TS
//! matches as UTF-16 are matched as units here.

use uo_jsnum::text::is_js_space_unit;

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

fn is_word(u: u16) -> bool {
    u8::try_from(u).is_ok_and(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn lower_ascii(u: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&u) { u + 32 } else { u }
}

/// `s.slice(at).startsWith(lit)` with ASCII case folding (`/i`).
fn starts_ci(s: &[u16], at: usize, lit: &str) -> bool {
    let b = lit.as_bytes();
    s.get(at..at + b.len()).is_some_and(|w| w.iter().zip(b).all(|(u, c)| lower_ascii(*u) == u16::from(c.to_ascii_lowercase())))
}

/// `s.slice(at).startsWith(lit)`.
fn starts(s: &[u16], at: usize, lit: &str) -> bool {
    let b = lit.as_bytes();
    s.get(at..at + b.len()).is_some_and(|w| w.iter().zip(b).all(|(u, c)| *u == u16::from(*c)))
}

/// Units of an ASCII literal.
#[must_use]
pub fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ───────────────────────── universal-write.ts ─────────────────────────

/// `t.replace(/\r\n?/g, '\n')` (5021, 5506).
#[must_use]
pub fn normalize_newlines(t: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while let Some(&u) = t.get(i) {
        if u == 13 {
            out.push(10);
            i += if t.get(i + 1) == Some(&10) { 2 } else { 1 };
        } else {
            out.push(u);
            i += 1;
        }
    }
    out
}

/// `d.text.split(/\r\n?|\n/)` (5592) = `t.replace(/\r\n?/g, '\n').split('\n')` (5506).
#[must_use]
pub fn split_lines(t: &[u16]) -> Vec<Vec<u16>> {
    normalize_newlines(t).split(|u| *u == 10).map(<[u16]>::to_vec).collect()
}

/// `/^\s*$/.test(html)` (5076).
#[must_use]
pub fn is_blank(t: &[u16]) -> bool {
    t.iter().all(|u| is_js_space_unit(*u))
}

/// `/<table[\s>]/i.test(html)` (5077).
#[must_use]
pub fn has_table_tag(t: &[u16]) -> bool {
    (0..t.len()).any(|i| starts_ci(t, i, "<table") && t.get(i + 6).is_some_and(|u| *u == u16::from(b'>') || is_js_space_unit(*u)))
}

/// `html.replace(/<table[\s\S]*?<\/table>/gi, '')` (5077).
#[must_use]
pub fn strip_tables(t: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(t.len());
    let mut at = 0;
    let mut i = 0;
    while i < t.len() {
        if starts_ci(t, i, "<table") {
            // lazy: the first closing tag after the opening
            let close = (i + 6..t.len()).find(|&k| starts_ci(t, k, "</table>"));
            match close {
                Some(k) => {
                    out.extend_from_slice(t.get(at..i).unwrap_or(&[]));
                    at = k + 8;
                    i = at;
                    continue;
                }
                // no closing tag after this one: none after a later one either
                None => break,
            }
        }
        i += 1;
    }
    out.extend_from_slice(t.get(at..).unwrap_or(&[]));
    out
}

/// `/<(?:p|h[1-6]|li|div)[\s>]/i.test(s)` (5077).
#[must_use]
pub fn has_block_tag(t: &[u16]) -> bool {
    let end_ok = |k: usize| t.get(k).is_some_and(|u| *u == u16::from(b'>') || is_js_space_unit(*u));
    (0..t.len()).any(|i| {
        if t.get(i) != Some(&u16::from(b'<')) {
            return false;
        }
        let a = i + 1;
        (starts_ci(t, a, "p") && end_ok(a + 1))
            || (starts_ci(t, a, "h") && t.get(a + 1).is_some_and(|u| (u16::from(b'1')..=u16::from(b'6')).contains(u)) && end_ok(a + 2))
            || (starts_ci(t, a, "li") && end_ok(a + 2))
            || (starts_ci(t, a, "div") && end_ok(a + 3))
    })
}

fn first_cp(t: &[u16]) -> Option<u32> {
    uo_text::utf16::code_points(t).next()
}

fn last_cp(t: &[u16]) -> Option<u32> {
    let n = t.len();
    let last = *t.last()?;
    if (0xDC00..0xE000).contains(&last)
        && n >= 2
        && let Some(&hi) = t.get(n - 2)
        && (0xD800..0xDC00).contains(&hi)
    {
        return Some(0x10000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(last) - 0xDC00));
    }
    Some(u32::from(last))
}

fn letter_or_number(cp: u32) -> bool {
    uo_text::props::is_letter(cp) || uo_text::props::is_number(cp)
}

/// `/^[\p{L}\p{N}]/u.test(t)` (5189, 5228).
#[must_use]
pub fn starts_word(t: &[u16]) -> bool {
    first_cp(t).is_some_and(letter_or_number)
}

/// `/[\p{L}\p{N}]$/u.test(t)` (5228).
#[must_use]
pub fn ends_word(t: &[u16]) -> bool {
    last_cp(t).is_some_and(letter_or_number)
}

/// `/[\p{L}\p{N}.,;:!?)\]}"'”’]$/u.test(t)` (5189).
#[must_use]
pub fn ends_word_or_punct(t: &[u16]) -> bool {
    last_cp(t).is_some_and(|cp| letter_or_number(cp) || ".,;:!?)]}\"'\u{201D}\u{2019}".chars().any(|c| u32::from(c) == cp))
}

/// `xml.replace(/(<wp:docPr\b[^>]*?\bname=")([^"]*?)\s*\d*"/, (_m, a, n) => `${a}${n} ${id}"`)`
/// (5123): the name keeps its text without a trailing number and gets the new id.
#[must_use]
pub fn renumber_doc_pr_name(xml: &str, id: &str) -> String {
    let u: Vec<u16> = xml.encode_utf16().collect();
    let mut f = 0usize;
    while let Some(i) = (f..u.len()).find(|&k| starts(&u, k, "<wp:docPr")) {
        let n = i + 9;
        if u.get(n).is_some_and(|c| is_word(*c)) {
            f = i + 1;
            continue;
        }
        let limit = (n..u.len()).find(|&k| u.get(k) == Some(&u16::from(b'>'))).unwrap_or(u.len());
        let mut j = n;
        while j < limit {
            if starts(&u, j, "name=\"") && (j == 0 || u.get(j - 1).is_none_or(|c| !is_word(*c))) {
                let v = j + 6;
                if let Some(q) = (v..u.len()).find(|&k| u.get(k) == Some(&u16::from(b'"'))) {
                    let val = u.get(v..q).unwrap_or(&[]);
                    // the longest suffix `\s*\d*`: the trailing digits, then the white space before them
                    let mut k = val.len();
                    while k > 0 && val.get(k - 1).is_some_and(|c| is_digit(*c)) {
                        k -= 1;
                    }
                    while k > 0 && val.get(k - 1).is_some_and(|c| is_js_space_unit(*c)) {
                        k -= 1;
                    }
                    let mut out: Vec<u16> = Vec::with_capacity(u.len() + 12);
                    out.extend_from_slice(u.get(..v).unwrap_or(&[]));
                    out.extend_from_slice(val.get(..k).unwrap_or(&[]));
                    out.push(u16::from(b' '));
                    out.extend(id.encode_utf16());
                    out.extend_from_slice(u.get(q..).unwrap_or(&[]));
                    return String::from_utf16_lossy(&out);
                }
            }
            j += 1;
        }
        f = i + 1;
    }
    xml.to_owned()
}

/// One atom of the `${ilvl}` part of the regex of `clipListKind` (5285).
#[derive(Clone, Copy)]
enum Atom {
    Lit(u16),
    /// `.`: any unit but a line terminator
    Any,
}

fn atom_ok(a: Atom, u: u16) -> bool {
    match a {
        Atom::Lit(c) => c == u,
        Atom::Any => !matches!(u, 10 | 13 | 0x2028 | 0x2029),
    }
}

/// `${ilvl}` read as a regex: `.` is any unit, `x+` one or more `x` (the `e+` of `1e+21`).
fn compile(p: &str) -> Vec<(Atom, bool)> {
    let mut out: Vec<(Atom, bool)> = Vec::new();
    for c in p.encode_utf16() {
        if c == u16::from(b'+')
            && let Some(last) = out.last_mut()
            && !last.1
        {
            last.1 = true;
            continue;
        }
        out.push((if c == u16::from(b'.') { Atom::Any } else { Atom::Lit(c) }, false));
    }
    out
}

/// The end positions of `pat` matched at `at` (greedy order: longest first).
fn match_ends(pat: &[(Atom, bool)], s: &[u16], at: usize, out: &mut Vec<usize>) {
    match pat.split_first() {
        None => out.push(at),
        Some((&(a, plus), rest)) => {
            if plus {
                let mut k = at;
                while s.get(k).is_some_and(|u| atom_ok(a, *u)) {
                    k += 1;
                }
                while k > at {
                    match_ends(rest, s, k, out);
                    k -= 1;
                }
            } else if s.get(at).is_some_and(|u| atom_ok(a, *u)) {
                match_ends(rest, s, at + 1, out);
            }
        }
    }
}

/// `/<w:numFmt\s+w:val="([^"]+)"/` from `from`: the first match's value.
fn num_fmt_after(s: &[u16], from: usize) -> Option<String> {
    let mut i = from;
    while i < s.len() {
        if starts(s, i, "<w:numFmt") {
            let mut k = i + 9;
            let ws0 = k;
            while s.get(k).is_some_and(|u| is_js_space_unit(*u)) {
                k += 1;
            }
            if k > ws0 && starts(s, k, "w:val=\"") {
                let v = k + 7;
                if let Some(q) = (v..s.len()).find(|&j| s.get(j) == Some(&u16::from(b'"')))
                    && q > v
                {
                    return Some(String::from_utf16_lossy(s.get(v..q).unwrap_or(&[])));
                }
            }
        }
        i += 1;
    }
    None
}

/// `new RegExp(`<w:lvl\\b[^>]*w:ilvl="${ilvl}"[\\s\\S]*?<w:numFmt\\s+w:val="([^"]+)"`).exec(raw)?.[1]`
/// (5285): `ilvl` is the number as JS prints it (`String(num.ilvl)`), read as a regex.
#[must_use]
pub fn level_num_fmt(raw: &str, ilvl: &str) -> Option<String> {
    let s: Vec<u16> = raw.encode_utf16().collect();
    let pat = compile(ilvl);
    let mut ends = Vec::new();
    for start in 0..s.len() {
        if !starts(&s, start, "<w:lvl") {
            continue;
        }
        let a = start + 6;
        if s.get(a).is_some_and(|u| is_word(*u)) {
            continue;
        }
        let gt = (a..s.len()).find(|&k| s.get(k) == Some(&u16::from(b'>'))).unwrap_or(s.len());
        // `[^>]*` greedy: the last `w:ilvl="` first
        for p in (a..gt).rev() {
            if !starts(&s, p, "w:ilvl=\"") {
                continue;
            }
            ends.clear();
            match_ends(&pat, &s, p + 8, &mut ends);
            for &e in &ends {
                if s.get(e) == Some(&u16::from(b'"'))
                    && let Some(v) = num_fmt_after(&s, e + 1)
                {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// `path.replace(/\\/g, '\\\\')` (5684).
#[must_use]
pub fn double_backslashes(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// `s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')` (20457).
#[must_use]
pub fn esc_html(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for &u in s {
        match u {
            38 => out.extend(units("&amp;")),
            60 => out.extend(units("&lt;")),
            62 => out.extend(units("&gt;")),
            _ => out.push(u),
        }
    }
    out
}

/// `t.replace(/\n/g, ' ')` (20458).
#[must_use]
pub fn newlines_to_spaces(t: &[u16]) -> Vec<u16> {
    t.iter().map(|&u| if u == 10 { 32 } else { u }).collect()
}

/// `t.replace(/\s+/g, ' ').trim()` (20564).
#[must_use]
pub fn squash_trim(t: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(t.len());
    let mut in_ws = false;
    for &u in t {
        if is_js_space_unit(u) {
            if !in_ws {
                out.push(32);
            }
            in_ws = true;
        } else {
            out.push(u);
            in_ws = false;
        }
    }
    let a = out.iter().position(|u| *u != 32).unwrap_or(out.len());
    let b = out.iter().rposition(|u| *u != 32).map_or(a, |k| k + 1);
    out.get(a..b.max(a)).unwrap_or(&[]).to_vec()
}

// ───────────────────────── paste-html.ts ─────────────────────────

/// `/^#([0-9a-f]{3})$/` (48; `s` is lower case).
#[must_use]
pub fn hex3(s: &[u16]) -> Option<&[u16]> {
    (s.len() == 4 && s.first() == Some(&u16::from(b'#')) && s.get(1..).is_some_and(|h| h.iter().all(|u| is_hex_lower(*u))))
        .then(|| s.get(1..).unwrap_or(&[]))
}

fn is_hex_lower(u: u16) -> bool {
    is_digit(u) || (u16::from(b'a')..=u16::from(b'f')).contains(&u)
}

/// `/^#([0-9a-f]{6})/` (50).
#[must_use]
pub fn hex6(s: &[u16]) -> Option<&[u16]> {
    (s.first() == Some(&u16::from(b'#')) && s.get(1..7).is_some_and(|h| h.iter().all(|u| is_hex_lower(*u))))
        .then(|| s.get(1..7).unwrap_or(&[]))
}

fn digits_at(s: &[u16], at: usize) -> usize {
    let mut k = at;
    while s.get(k).is_some_and(|u| is_digit(*u)) {
        k += 1;
    }
    k
}

/// The three numbers and the alpha of `rgb()`/`rgba()`.
pub type Rgba = ([Vec<u16>; 3], Option<Vec<u16>>);

/// `/^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)(?:[\s,/]+([\d.]+%?))?\s*\)$/` (52): the three numbers
/// and the alpha (`m[4]`). The character classes are disjoint, so the match is deterministic.
#[must_use]
pub fn rgba(s: &[u16]) -> Option<Rgba> {
    if !starts(s, 0, "rgb") {
        return None;
    }
    let mut i = 3;
    if s.get(i) == Some(&u16::from(b'a')) {
        i += 1;
    }
    if s.get(i) != Some(&u16::from(b'(')) {
        return None;
    }
    i += 1;
    while s.get(i).is_some_and(|u| is_js_space_unit(*u)) {
        i += 1;
    }
    let sep = |u: u16| u == u16::from(b',') || is_js_space_unit(u);
    let mut nums: [Vec<u16>; 3] = Default::default();
    for (k, slot) in nums.iter_mut().enumerate() {
        if k > 0 {
            let s0 = i;
            while s.get(i).is_some_and(|u| sep(*u)) {
                i += 1;
            }
            if i == s0 {
                return None;
            }
        }
        let e = digits_at(s, i);
        if e == i {
            return None;
        }
        *slot = s.get(i..e).unwrap_or(&[]).to_vec();
        i = e;
    }
    let close = |mut k: usize| -> bool {
        while s.get(k).is_some_and(|u| is_js_space_unit(*u)) {
            k += 1;
        }
        s.get(k) == Some(&u16::from(b')')) && k + 1 == s.len()
    };
    // the optional group first
    let mut k = i;
    while s.get(k).is_some_and(|u| sep(*u) || *u == u16::from(b'/')) {
        k += 1;
    }
    if k > i {
        let a0 = k;
        while s.get(k).is_some_and(|u| is_digit(*u) || *u == u16::from(b'.')) {
            k += 1;
        }
        if k > a0 {
            if s.get(k) == Some(&u16::from(b'%')) {
                k += 1;
            }
            if close(k) {
                return Some((nums, Some(s.get(a0..k).unwrap_or(&[]).to_vec())));
            }
        }
    }
    close(i).then_some((nums, None))
}

/// `/^([\d.]+)(pt|px)$/` (65): the number text and whether the unit is `pt`.
#[must_use]
pub fn size_unit(s: &[u16]) -> Option<(&[u16], bool)> {
    let n = s.len();
    if n < 3 {
        return None;
    }
    let (num, unit) = s.split_at(n - 2);
    let pt = unit == units("pt").as_slice();
    if !pt && unit != units("px").as_slice() {
        return None;
    }
    (!num.is_empty() && num.iter().all(|u| is_digit(*u) || *u == u16::from(b'.'))).then_some((num, pt))
}

/// `f.trim().replace(/^["']|["']$/g, '')` after the trim (74).
#[must_use]
pub fn strip_quotes(n: &[u16]) -> &[u16] {
    let q = |u: &u16| *u == u16::from(b'"') || *u == u16::from(b'\'');
    let mut a = 0;
    let mut b = n.len();
    if n.first().is_some_and(q) {
        a = 1;
    }
    if b > a && n.get(b - 1).is_some_and(q) {
        b -= 1;
    }
    n.get(a..b).unwrap_or(&[])
}

/// `/^(serif|sans-serif|monospace|cursive|fantasy|system-ui|ui-\w+|-apple-system|blinkmacsystemfont|inherit|initial)$/i` (75).
#[must_use]
pub fn generic_family(n: &[u16]) -> bool {
    let eq = |lit: &str| n.len() == lit.len() && starts_ci(n, 0, lit);
    if ["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "-apple-system", "blinkmacsystemfont", "inherit", "initial"]
        .iter()
        .any(|l| eq(l))
    {
        return true;
    }
    starts_ci(n, 0, "ui-") && n.len() > 3 && n.get(3..).is_some_and(|r| r.iter().all(|u| is_word(*u)))
}

/// `/^javascript:/i.test(href)` (100).
#[must_use]
pub fn is_javascript(href: &[u16]) -> bool {
    starts_ci(href, 0, "javascript:")
}

/// `/^data:image\/(png|jpe?g|gif|bmp|webp);base64,([\s\S]+)$/i` (124): the type as written and the
/// payload.
#[must_use]
pub fn data_url(src: &[u16]) -> Option<(&[u16], &[u16])> {
    if !starts_ci(src, 0, "data:image/") {
        return None;
    }
    let a = 11;
    for ty in ["png", "jpeg", "jpg", "gif", "bmp", "webp"] {
        if starts_ci(src, a, ty) && starts_ci(src, a + ty.len(), ";base64,") {
            let p = a + ty.len() + 8;
            if p < src.len() {
                return Some((src.get(a..a + ty.len()).unwrap_or(&[]), src.get(p..).unwrap_or(&[])));
            }
        }
    }
    None
}

/// `s.replace(/\s+/g, '')` (127, 162, 322).
#[must_use]
pub fn remove_spaces(s: &[u16]) -> Vec<u16> {
    s.iter().copied().filter(|u| !is_js_space_unit(*u)).collect()
}

/// `/@list\s+l(\d+):level(\d+)\s*\{([^}]*)\}/gi` (145) with
/// `/mso-level-number-format:\s*([\w-]+)/i` (148): `(list digits, level digits, format lower-cased)`.
#[must_use]
pub fn word_list_defs(html: &[u16]) -> Vec<(Vec<u16>, Vec<u16>, Option<String>)> {
    let mut out = Vec::new();
    // no `}` after a start = no match there nor at any later start: stop (the V8 regex would
    // rescan to the end from every `@list`, quadratic on hostile input; same matches)
    let Some(last_close) = html.iter().rposition(|&u| u == u16::from(b'}')) else {
        return out;
    };
    let mut i = 0;
    while i < last_close {
        if let Some((end, l, v, body)) = list_def_at(html, i) {
            out.push((l, v, number_format(body)));
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

#[allow(clippy::type_complexity)]
fn list_def_at(s: &[u16], at: usize) -> Option<(usize, Vec<u16>, Vec<u16>, &[u16])> {
    if !starts_ci(s, at, "@list") {
        return None;
    }
    let mut i = at + 5;
    let w0 = i;
    while s.get(i).is_some_and(|u| is_js_space_unit(*u)) {
        i += 1;
    }
    if i == w0 || !starts_ci(s, i, "l") {
        return None;
    }
    i += 1;
    let e = digits_at(s, i);
    if e == i {
        return None;
    }
    let l = s.get(i..e)?.to_vec();
    i = e;
    if !starts_ci(s, i, ":level") {
        return None;
    }
    i += 6;
    let e = digits_at(s, i);
    if e == i {
        return None;
    }
    let v = s.get(i..e)?.to_vec();
    i = e;
    while s.get(i).is_some_and(|u| is_js_space_unit(*u)) {
        i += 1;
    }
    if s.get(i) != Some(&u16::from(b'{')) {
        return None;
    }
    let b0 = i + 1;
    let close = (b0..s.len()).find(|&k| s.get(k) == Some(&u16::from(b'}')))?;
    Some((close + 1, l, v, s.get(b0..close)?))
}

fn number_format(body: &[u16]) -> Option<String> {
    let key = "mso-level-number-format:";
    for i in 0..body.len() {
        if starts_ci(body, i, key) {
            let mut k = i + key.len();
            while body.get(k).is_some_and(|u| is_js_space_unit(*u)) {
                k += 1;
            }
            let a = k;
            while body.get(k).is_some_and(|u| is_word(*u) || *u == u16::from(b'-')) {
                k += 1;
            }
            if k > a {
                return Some(String::from_utf16_lossy(body.get(a..k).unwrap_or(&[])).to_ascii_lowercase());
            }
        }
    }
    None
}

/// `/mso-list:\s*l(\d+)\s+level(\d+)\s+lfo(\d+)/i.exec(style)` (156): the three digit strings.
#[must_use]
pub fn word_list_of(style: &[u16]) -> Option<[Vec<u16>; 3]> {
    for at in 0..style.len() {
        if !starts_ci(style, at, "mso-list:") {
            continue;
        }
        let mut i = at + 9;
        while style.get(i).is_some_and(|u| is_js_space_unit(*u)) {
            i += 1;
        }
        let mut parts: [Vec<u16>; 3] = Default::default();
        let mut ok = true;
        for (k, (lit, slot)) in ["l", "level", "lfo"].iter().zip(parts.iter_mut()).enumerate() {
            if k > 0 {
                let w0 = i;
                while style.get(i).is_some_and(|u| is_js_space_unit(*u)) {
                    i += 1;
                }
                if i == w0 {
                    ok = false;
                    break;
                }
            }
            if !starts_ci(style, i, lit) {
                ok = false;
                break;
            }
            i += lit.len();
            let e = digits_at(style, i);
            if e == i {
                ok = false;
                break;
            }
            *slot = style.get(i..e).unwrap_or(&[]).to_vec();
            i = e;
        }
        if ok {
            return Some(parts);
        }
    }
    None
}

/// `/^\(?\d+[.)]?$/.test(t) || /^\(?[a-z]{1,4}[.)]$/i.test(t)` (163; `t` without white space).
#[must_use]
pub fn looks_ordered_units(t: &[u16]) -> bool {
    let mut i = usize::from(t.first() == Some(&u16::from(b'(')));
    let body = t.get(i..).unwrap_or(&[]);
    let n = body.len();
    let punct = |u: &u16| *u == u16::from(b'.') || *u == u16::from(b')');
    // digits, then one optional `.`/`)`
    let d = body.iter().take_while(|u| is_digit(**u)).count();
    if d > 0 && (d == n || (d + 1 == n && body.get(d).is_some_and(punct))) {
        return true;
    }
    // one to four ASCII letters, then `.`/`)`
    let l = body.iter().take_while(|u| u8::try_from(**u).is_ok_and(|b| b.is_ascii_alphabetic())).count();
    i = l;
    (1..=4).contains(&l) && i + 1 == n && body.get(i).is_some_and(punct)
}

/// `/^\[if !supportLists\]/i` (173, 229) on the trimmed comment.
#[must_use]
pub fn is_support_lists(v: &[u16]) -> bool {
    starts_ci(v, 0, "[if !supportLists]")
}

/// `/^\[endif\]/i` (174, 230).
#[must_use]
pub fn is_endif(v: &[u16]) -> bool {
    starts_ci(v, 0, "[endif]")
}

/// `/mso-list:\s*ignore/i` (180, 245).
#[must_use]
pub fn is_mso_list_ignore(style: &[u16]) -> bool {
    (0..style.len()).any(|at| {
        if !starts_ci(style, at, "mso-list:") {
            return false;
        }
        let mut i = at + 9;
        while style.get(i).is_some_and(|u| is_js_space_unit(*u)) {
            i += 1;
        }
        starts_ci(style, i, "ignore")
    })
}

/// `raw.replace(/[ \t\r\n\f]+/g, ' ')` (203).
#[must_use]
pub fn collapse_html_space(t: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(t.len());
    let mut in_ws = false;
    for &u in t {
        if matches!(u, 32 | 9 | 13 | 10 | 12) {
            if !in_ws {
                out.push(32);
            }
            in_ws = true;
        } else {
            out.push(u);
            in_ws = false;
        }
    }
    out
}

/// `/[ \n]$/.test(prev)` (208).
#[must_use]
pub fn ends_space_or_newline(t: &[u16]) -> bool {
    matches!(t.last(), Some(32 | 10))
}

/// `t.replace(/^ +/, '')` (208).
#[must_use]
pub fn trim_leading_spaces(t: &[u16]) -> &[u16] {
    let k = t.iter().take_while(|u| **u == 32).count();
    t.get(k..).unwrap_or(&[])
}

/// `/^H([1-6])$/.exec(tag)` (275).
#[must_use]
pub fn heading_tag(tag: &[u16]) -> Option<u8> {
    match tag {
        [72, d] if (u16::from(b'1')..=u16::from(b'6')).contains(d) => u8::try_from(*d - u16::from(b'0')).ok(),
        _ => None,
    }
}

/// A class name of `cls` that `/(?:^|\s)NAME(?:\s|$)/` finds, with `digit` the `[1-9]` after it
/// (280–283): `MsoHeading([1-9])`, `MsoTitle`, `MsoSubtitle`.
fn class_at(cls: &[u16], name: &str, digit: bool) -> Option<u16> {
    let nl = name.len();
    for k in 0..cls.len() {
        if !starts(cls, k, name) || !(k == 0 || cls.get(k - 1).is_some_and(|u| is_js_space_unit(*u))) {
            continue;
        }
        let mut e = k + nl;
        let mut d = 0u16;
        if digit {
            match cls.get(e) {
                Some(u) if (u16::from(b'1')..=u16::from(b'9')).contains(u) => {
                    d = *u - u16::from(b'0');
                    e += 1;
                }
                _ => continue,
            }
        }
        if e == cls.len() || cls.get(e).is_some_and(|u| is_js_space_unit(*u)) {
            return Some(d);
        }
    }
    None
}

/// `/(?:^|\s)MsoHeading([1-9])(?:\s|$)/.exec(cls)` (280).
#[must_use]
pub fn mso_heading(cls: &[u16]) -> Option<u16> {
    class_at(cls, "MsoHeading", true)
}

/// `/(?:^|\s)MsoTitle(?:\s|$)/` (282).
#[must_use]
pub fn mso_title(cls: &[u16]) -> bool {
    class_at(cls, "MsoTitle", false).is_some()
}

/// `/(?:^|\s)MsoSubtitle(?:\s|$)/` (283).
#[must_use]
pub fn mso_subtitle(cls: &[u16]) -> bool {
    class_at(cls, "MsoSubtitle", false).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        units(s)
    }

    #[test]
    fn samples() {
        assert!(rgba(&u("rgb(1, 2, 3)")).is_some());
        assert_eq!(rgba(&u("rgba(1, 2, 3, 0)")).and_then(|x| x.1), Some(u("0")));
        assert!(rgba(&u("rgb(1,2,3 )")).is_some_and(|x| x.1.is_none()));
        assert_eq!(renumber_doc_pr_name("<wp:docPr id=\"3\" name=\"Imagem 12\"/>", "7"), "<wp:docPr id=\"3\" name=\"Imagem 7\"/>");
        assert_eq!(level_num_fmt("<w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"bullet\"/>", "1").as_deref(), Some("bullet"));
        assert_eq!(level_num_fmt("<w:lvl w:ilvl=\"1x5\"><w:numFmt w:val=\"x\"/>", "1.5").as_deref(), Some("x"));
        assert_eq!(level_num_fmt("<w:lvl w:ilvl=\"1eee21\"><w:numFmt w:val=\"y\"/>", "1e+21").as_deref(), Some("y"));
        assert!(looks_ordered_units(&u("(iv)")));
        assert!(looks_ordered_units(&u("12.")));
        assert!(!looks_ordered_units(&u("abcde.")));
        assert_eq!(squash_trim(&u("  a \n b ")), u("a b"));
        assert_eq!(strip_tables(&u("x<table>y</table>z<TABLE a>w</Table>")), u("xz"));
        assert!(has_block_tag(&u("<div>")) && !has_block_tag(&u("<pre>")));
    }
}
