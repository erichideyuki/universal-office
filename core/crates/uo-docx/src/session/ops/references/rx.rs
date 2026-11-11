//! The regular expressions of the layout family, as hand matchers (linear in the input, no
//! backtracking engine). Each one is the TS regex named in its doc comment; `js/regex-sites/
//! d2b-f10.mjs` holds the V8 rows the replay test (`regex_d2b_f10`) compares them with.
//!
//! Without the `u` flag a JS regex works on UTF-16 code units, and its `i` flag folds ASCII only
//! for the letters used here (the case folding of a non-ASCII letter never reaches an ASCII one in
//! non-`u` mode). `\s` is [`uo_jsnum::text::is_js_space`], `\w` and `\d` are ASCII.

use uo_jsnum::text::is_js_space;

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `s.trimStart()` after the `^\s*` of an anchored regex.
fn skip_space(s: &str) -> &str {
    s.trim_start_matches(is_js_space)
}

/// Strips an ASCII-case-insensitive keyword.
fn strip_kw<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let h = s.get(..kw.len())?;
    if h.eq_ignore_ascii_case(kw) { s.get(kw.len()..) } else { None }
}

/// `\b` after a word character `c` (the end of the match) at the start of `rest`.
fn boundary_after_word(rest: &str) -> bool {
    !rest.chars().next().is_some_and(is_word)
}

/// `/^\s*(?:REF|PAGEREF|NOTEREF)\s+([^\s\\]+)/i` → the group.
#[must_use]
pub fn ref_target(instr: &str) -> Option<&str> {
    let s = skip_space(instr);
    for kw in ["REF", "PAGEREF", "NOTEREF"] {
        let Some(rest) = strip_kw(s, kw) else { continue };
        let after = skip_space(rest);
        if after.len() == rest.len() {
            continue;
        }
        let end = after.find(|c: char| is_js_space(c) || c == '\\').unwrap_or(after.len());
        if end > 0 {
            return after.get(..end);
        }
    }
    None
}

/// `/^\s*SEQ\s+("([^"]+)"|\S+)/i` → `m[2] ?? m[1]`.
#[must_use]
pub fn seq_label(instr: &str) -> Option<&str> {
    let s = skip_space(instr);
    let rest = strip_kw(s, "SEQ")?;
    let after = skip_space(rest);
    if after.len() == rest.len() {
        return None;
    }
    if let Some(q) = after.strip_prefix('"')
        && let Some(end) = q.find('"')
        && end > 0
    {
        return q.get(..end);
    }
    let end = after.find(is_js_space).unwrap_or(after.len());
    if end > 0 { after.get(..end) } else { None }
}

/// `new RegExp('\\\\' + sw + '\\s+"([^"]*)"', 'i').exec(instr)?.[1]`.
#[must_use]
pub fn switch_quoted(instr: &str, sw: u8) -> Option<String> {
    let b = instr.as_bytes();
    for (i, _) in instr.match_indices('\\') {
        if !b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&sw)) {
            continue;
        }
        let rest = instr.get(i + 2..)?;
        let after = skip_space(rest);
        if after.len() == rest.len() {
            continue;
        }
        if let Some(q) = after.strip_prefix('"')
            && let Some(end) = q.find('"')
        {
            return q.get(..end).map(str::to_owned);
        }
    }
    None
}

/// `/\\SW(?![a-z"])/i.test(instr.replace(/"[^"]*"/g, '""'))`.
#[must_use]
pub fn has_switch(instr: &str, sw: u8) -> bool {
    // instr.replace(/"[^"]*"/g, '""')
    let mut flat = String::with_capacity(instr.len());
    let mut rest = instr;
    while let Some(i) = rest.find('"') {
        let Some(tail) = rest.get(i + 1..) else { break };
        match tail.find('"') {
            Some(j) => {
                flat.push_str(rest.get(..i).unwrap_or(""));
                flat.push_str("\"\"");
                rest = tail.get(j + 1..).unwrap_or("");
            }
            None => break,
        }
    }
    flat.push_str(rest);
    let b = flat.as_bytes();
    for (i, _) in flat.match_indices('\\') {
        if !b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&sw)) {
            continue;
        }
        match b.get(i + 2) {
            Some(c) if c.is_ascii_alphabetic() || *c == b'"' => {}
            _ => return true,
        }
    }
    false
}

/// `/(\d)\s*-\s*(\d)/.exec(v)` → the two digits as numbers.
#[must_use]
pub fn digit_range(v: &str) -> Option<(f64, f64)> {
    let b = v.as_bytes();
    for i in 0..b.len() {
        let Some(a) = b.get(i).filter(|c| c.is_ascii_digit()) else { continue };
        let rest = v.get(i + 1..)?;
        let rest = skip_space(rest);
        let Some(rest) = rest.strip_prefix('-') else { continue };
        let rest = skip_space(rest);
        if let Some(z) = rest.as_bytes().first().filter(|c| c.is_ascii_digit()) {
            return Some((f64::from(a - b'0'), f64::from(z - b'0')));
        }
    }
    None
}

/// `/\\f(?:\s+(?:"([^"]*)"|([^\s\\"]+)))?/i.exec(instr)`: `None` = no `\f`; `Some(None)` = `\f`
/// without an id; `Some(Some(id))` (`m[1] ?? m[2]`).
#[must_use]
pub fn switch_f(instr: &str) -> Option<Option<String>> {
    let b = instr.as_bytes();
    let i = instr.match_indices('\\').map(|(i, _)| i).find(|&i| b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'f')))?;
    let rest = instr.get(i + 2..).unwrap_or("");
    Some(f_id(rest))
}

/// The id group after `\f` (shared by the TOC field and the TC field).
fn f_id(rest: &str) -> Option<String> {
    let after = skip_space(rest);
    if after.len() == rest.len() {
        return None;
    }
    if let Some(q) = after.strip_prefix('"')
        && let Some(end) = q.find('"')
    {
        return q.get(..end).map(str::to_owned);
    }
    let end = after.find(|c: char| is_js_space(c) || c == '\\' || c == '"').unwrap_or(after.len());
    if end > 0 { after.get(..end).map(str::to_owned) } else { None }
}

/// `/\\f\s+(?:"([^"]*)"|([^\s\\"]+))/i.exec(rest)` (the TC field: the group is required, the
/// first `\f` that has one wins).
#[must_use]
pub fn tc_switch_f(rest: &str) -> Option<String> {
    let b = rest.as_bytes();
    for (i, _) in rest.match_indices('\\') {
        if !b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'f')) {
            continue;
        }
        if let Some(id) = f_id(rest.get(i + 2..).unwrap_or("")) {
            return Some(id);
        }
    }
    None
}

/// `/\\l\s+"?(\d+)/i.exec(rest)` → the digits.
#[must_use]
pub fn tc_switch_l(rest: &str) -> Option<&str> {
    let b = rest.as_bytes();
    for (i, _) in rest.match_indices('\\') {
        if !b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'l')) {
            continue;
        }
        let tail = rest.get(i + 2..).unwrap_or("");
        let after = skip_space(tail);
        if after.len() == tail.len() {
            continue;
        }
        let after = after.strip_prefix('"').unwrap_or(after);
        let n = after.bytes().take_while(u8::is_ascii_digit).count();
        if n > 0 {
            return after.get(..n);
        }
    }
    None
}

/// `/\\n\b/i.test(rest)`.
#[must_use]
pub fn tc_switch_n(rest: &str) -> bool {
    let b = rest.as_bytes();
    rest.match_indices('\\')
        .any(|(i, _)| b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'n')) && boundary_after_word(rest.get(i + 2..).unwrap_or("")))
}

/// `/^\s*TC\s+(?:"((?:[^"\\]|\\.)*)"|(\S+))/i`: the raw text (group 1 or 2) and the rest after the
/// match.
#[must_use]
pub fn tc_head(instr: &str) -> Option<(&str, &str)> {
    let s = skip_space(instr);
    let rest = strip_kw(s, "TC")?;
    let after = skip_space(rest);
    if after.len() == rest.len() {
        return None;
    }
    if let Some(q) = after.strip_prefix('"') {
        let mut it = q.char_indices();
        let mut end = None;
        while let Some((i, c)) = it.next() {
            match c {
                '"' => {
                    end = Some(i);
                    break;
                }
                '\\' => match it.next() {
                    Some((_, n)) if !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {}
                    _ => break,
                },
                _ => {}
            }
        }
        if let Some(e) = end {
            return Some((q.get(..e)?, q.get(e + 1..)?));
        }
    }
    let n = after.find(is_js_space).unwrap_or(after.len());
    if n == 0 {
        return None;
    }
    Some((after.get(..n)?, after.get(n..)?))
}

/// `text.replace(/\\(.)/g, '$1')` (`.` is any unit but a line terminator).
#[must_use]
pub fn unescape_tc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\'
            && let Some(&n) = it.peek()
            && !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}')
        {
            out.push(n);
            it.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// `text.replace(/(["\\])/g, '\\$1')`.
#[must_use]
pub fn escape_tc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `/^\s*(\w+)/.exec(instr)?.[1]?.toUpperCase()`.
#[must_use]
pub fn field_name(instr: &str) -> String {
    let s = skip_space(instr);
    let n = s.bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'_').count();
    s.get(..n).unwrap_or("").to_ascii_uppercase()
}

/// `/\\[pnrw]\b/i.test(instr)`.
#[must_use]
pub fn has_pnrw(instr: &str) -> bool {
    let b = instr.as_bytes();
    instr.match_indices('\\').any(|(i, _)| {
        b.get(i + 1).is_some_and(|c| matches!(c.to_ascii_lowercase(), b'p' | b'n' | b'r' | b'w'))
            && boundary_after_word(instr.get(i + 2..).unwrap_or(""))
    })
}

/// `/\\f\b/i.test(instr)`.
#[must_use]
pub fn has_f_word(instr: &str) -> bool {
    let b = instr.as_bytes();
    instr
        .match_indices('\\')
        .any(|(i, _)| b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'f')) && boundary_after_word(instr.get(i + 2..).unwrap_or("")))
}

/// `/^\s*KW\b/i.test(instr)` for `TOC` and `PAGEREF`.
#[must_use]
pub fn starts_with_word(instr: &str, kw: &str) -> bool {
    strip_kw(skip_space(instr), kw).is_some_and(boundary_after_word)
}

/// `/^\s*=/.test(instr)`.
#[must_use]
pub fn starts_with_equals(instr: &str) -> bool {
    skip_space(instr).starts_with('=')
}

/// `/^heading [1-9]$/i.test(name)`.
#[must_use]
pub fn is_heading_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 9 && name.get(..8).is_some_and(|h| h.eq_ignore_ascii_case("heading ")) && b.get(8).is_some_and(|c| (b'1'..=b'9').contains(c))
}

/// `/^n:(\d+)$/.exec(item)?.[1]` → the digits.
#[must_use]
pub fn note_item(item: &str) -> Option<&str> {
    let d = item.strip_prefix("n:")?;
    (!d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())).then_some(d)
}

/// `/^p:(\d+)$/.exec(item)?.[1]` → the digits.
#[must_use]
pub fn para_item(item: &str) -> Option<&str> {
    let d = item.strip_prefix("p:")?;
    (!d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())).then_some(d)
}

/// `new RegExp('<' + tag + '[^>]*>([^<]*)<').exec(core)?.[1]` for a fixed tag (`dc:creator`,
/// `dc:title`), then the four entity replaces in the TS order; `""` when there is no match.
#[must_use]
pub fn core_prop(core: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    // the first `<tag`; a later one could only match if this one did (no `>` or no `<` after it
    // means none after the later ones either)
    let Some(i) = core.find(&open) else { return String::new() };
    let tail = core.get(i + open.len()..).unwrap_or("");
    // `[^>]*>`: everything up to the first `>` (a `<` inside is allowed by the class)
    let Some(gt) = tail.find('>') else { return String::new() };
    let body = tail.get(gt + 1..).unwrap_or("");
    // `([^<]*)<`
    let Some(lt) = body.find('<') else { return String::new() };
    body.get(..lt).unwrap_or("").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// The `[\t\n\f\u000B]` set of `tocSources` and the caption items (`\r` is not in it).
#[must_use]
pub fn is_tab_nl_ff_vt(u: u16) -> bool {
    matches!(u, 9..=12)
}

/// `s.replace(/\s+/…)`-free helper: `text.replace(/^\s+/, '')` over UTF-16 units.
#[must_use]
pub fn trim_start_units(u: &[u16]) -> &[u16] {
    let n = u.iter().take_while(|&&c| uo_jsnum::text::is_js_space_unit(c)).count();
    u.get(n..).unwrap_or(&[])
}

/// `s.trim()` over UTF-16 units.
#[must_use]
pub fn trim_units(u: &[u16]) -> &[u16] {
    let s = trim_start_units(u);
    let n = s.iter().rev().take_while(|&&c| uo_jsnum::text::is_js_space_unit(c)).count();
    s.get(..s.len() - n).unwrap_or(&[])
}

/// `text.replace(/\r\n?/g, '\n').split('\n')` (`pasteTextOnly`).
#[must_use]
pub fn split_paste_lines(text: &[u16]) -> Vec<Vec<u16>> {
    let mut parts: Vec<Vec<u16>> = vec![Vec::new()];
    let mut i = 0;
    while let Some(&u) = text.get(i) {
        match u {
            13 => {
                if text.get(i + 1) == Some(&10) {
                    i += 1;
                }
                parts.push(Vec::new());
            }
            10 => parts.push(Vec::new()),
            _ => {
                if let Some(l) = parts.last_mut() {
                    l.push(u);
                }
            }
        }
        i += 1;
    }
    parts
}

/// UTF-16 units in hex, `.`-joined (the V8 rows' result format).
fn hx(units: &[u16]) -> String {
    units.iter().map(|u| format!("{u:x}")).collect::<Vec<_>>().join(".")
}

fn hx_str(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<u16>>())
}

fn opt_hx(s: Option<&str>) -> String {
    s.map_or_else(|| "~".to_owned(), hx_str)
}

/// The Rust matcher of each regex site of `js/regex-sites/d2b-f10.mjs` (the replay test of
/// `regex_d2b_f10` compares it with V8). `None`: no such site.
#[must_use]
pub fn regex_site(id: &str, input: &[u16]) -> Option<String> {
    let s = String::from_utf16_lossy(input);
    let s = s.as_str();
    let b = |v: bool| v.to_string();
    let sw = |c: char| u8::try_from(c).unwrap_or(b'?');
    let name = id.strip_prefix("d2b-f10.")?;
    Some(match name {
        "ref_target" => opt_hx(ref_target(s)),
        "seq_label" => opt_hx(seq_label(s)),
        "digit_range" => digit_range(s).map_or_else(|| "~".to_owned(), |(a, z)| format!("{a},{z}")),
        "switch_f" => match switch_f(s) {
            None => "~".to_owned(),
            Some(id) => format!("y|{}", opt_hx(id.as_deref())),
        },
        "id_quoted" => b(s.chars().any(|c| is_js_space(c) || c == '"')),
        "t_split" => s
            .split([';', ','])
            .map(|x| {
                let n = uo_jsnum::parse::parse_int(x, 10);
                let n = if n == 0.0 || n.is_nan() { 1.0 } else { n };
                format!("{}:{}", hx_str(uo_jsnum::text::trim(x)), uo_jsnum::format::to_js_string(n))
            })
            .collect::<Vec<_>>()
            .join(" "),
        "has_space" => b(s.chars().any(is_js_space)),
        "entry_text" => hx(&input.iter().map(|&u| if matches!(u, 9..=13) { 32 } else { u }).collect::<Vec<u16>>()),
        "tc" => match tc_head(s) {
            None => "~".to_owned(),
            Some((raw, rest)) => {
                let text = unescape_tc(raw);
                let f = tc_switch_f(rest);
                let l = tc_switch_l(rest).map(|d| uo_jsnum::math::min(9.0, uo_jsnum::math::max(1.0, uo_jsnum::parse::to_number(d))));
                format!(
                    "{}|{}|{}|{}",
                    hx_str(&text),
                    f.map_or_else(|| "C".to_owned(), |x| hx_str(&x)),
                    uo_jsnum::format::to_js_string(l.unwrap_or(1.0)),
                    tc_switch_n(rest)
                )
            }
        },
        "tc_escape" => hx_str(&escape_tc(s)),
        "field_name" => hx_str(&field_name(s)),
        "has_pnrw" => b(has_pnrw(s)),
        "has_f" => b(has_f_word(s)),
        "starts_toc" => b(starts_with_word(s, "TOC")),
        "starts_pageref" => b(starts_with_word(s, "PAGEREF")),
        "starts_eq" => b(starts_with_equals(s)),
        "trim_start" => hx(trim_start_units(input)),
        "source_text" => hx(trim_units(&input.iter().map(|&u| if is_tab_nl_ff_vt(u) { 32 } else { u }).collect::<Vec<u16>>())),
        "note_text" => hx(trim_units(&input.iter().map(|&u| if matches!(u, 9 | 10) { 32 } else { u }).collect::<Vec<u16>>())),
        "caption_text" => hx(trim_units(&input.iter().map(|&u| if matches!(u, 9 | 10 | 12) { 32 } else { u }).collect::<Vec<u16>>())),
        "note_item" => note_item(s).unwrap_or("~").to_owned(),
        "para_item" => para_item(s).unwrap_or("~").to_owned(),
        "heading_name" => b(is_heading_name(s)),
        "core_creator" => hx_str(&core_prop(s, "dc:creator")),
        "core_title" => hx_str(&core_prop(s, "dc:title")),
        "letter" => b(input.first().is_some_and(|&u| super::hyphen::is_letter_unit(u))),
        "shy" => hx(&input.iter().copied().filter(|&u| u != 0xAD).collect::<Vec<u16>>()),
        "paste_lines" => split_paste_lines(input).iter().map(|l| hx(l)).collect::<Vec<_>>().join("/"),
        n => {
            if let Some(c) = n.strip_prefix("q_").and_then(|r| r.chars().next()) {
                opt_hx(switch_quoted(s, sw(c)).as_deref())
            } else {
                let c = n.strip_prefix("has_").and_then(|r| r.chars().next())?;
                b(has_switch(s, sw(c)))
            }
        }
    })
}
