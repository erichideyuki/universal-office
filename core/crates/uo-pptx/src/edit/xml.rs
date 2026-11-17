//! `xml-edit.ts`: XML surgery BY TEXT for the presentation editor (feature `edit`, frontier B8).
//!
//! Golden rule of the package (lossless round trip): only the edited span of a part changes, the rest
//! of the text stays byte for byte. So nothing here parses into a tree: every function scans the text
//! for ASCII tokens (`<name`, quotes, `>`), exactly as the TS does with its `RegExp`s, and the
//! offsets it returns are consumed by slice/splice. Rust offsets are **byte** offsets where the TS
//! has UTF-16 offsets; as every offset is produced by a scan for ASCII tokens, both select the same
//! text (`uo-pptx.md` P6). The few places where arithmetic with a length is not produced by a scan
//! (`end - len("</name>")`) go through [`sl`]/[`floor_boundary`], which never split a character: a
//! close tag written with a non-ASCII white space (`</p:sp\u{a0}>`) is the one input where the byte
//! and UTF-16 results could differ (declared in the record, hostile only).
//!
//! The `RegExp`s of the TS are hand matchers with the JS `\s` set ([`uo_jsnum::text::js_space_len`])
//! and `\w` = `[A-Za-z0-9_]`.

use uo_jsnum::text::{is_js_word, js_space_len};

/// A span `[start, end)` of an element in the text, with the end of its opening tag (`XmlRange`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XmlRange {
    pub start: usize,
    pub end: usize,
    /// Index right after the `>` of the opening tag.
    pub open_end: usize,
    /// `<x/>`
    pub self_closing: bool,
}

/// An element range with its qualified name (`childRanges`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NamedRange {
    pub range: XmlRange,
    pub name: String,
}

/// The largest char boundary `<= i` (and `<= s.len()`).
pub fn floor_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `s.slice(a, b)` for offsets that may fall outside or inside a character.
pub fn sl(s: &str, a: usize, b: usize) -> &str {
    let a = floor_boundary(s, a);
    let b = floor_boundary(s, b);
    if a >= b { "" } else { s.get(a..b).unwrap_or("") }
}

/// `s.slice(a)`.
pub fn sl_from(s: &str, a: usize) -> &str {
    s.get(floor_boundary(s, a)..).unwrap_or("")
}

/// Index right after the `>` of the tag that starts at `pos` (skips quoted values).
pub fn tag_end(xml: &str, pos: usize) -> usize {
    let b = xml.as_bytes();
    let mut q = 0u8;
    let mut i = pos + 1;
    while i < b.len() {
        let c = b[i];
        if q != 0 {
            if c == q {
                q = 0;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            q = c;
        } else if c == b'>' {
            return i + 1;
        }
        i += 1;
    }
    xml.len()
}

/// The next `<name` / `</name` at or after `from` followed by `[\s/>]`: (index of `<`, is a close tag).
fn next_tag(xml: &str, name: &str, from: usize) -> Option<(usize, bool)> {
    let b = xml.as_bytes();
    let nb = name.as_bytes();
    let mut i = from;
    while i < b.len() {
        let lt = i + b[i..].iter().position(|&c| c == b'<')?;
        let mut j = lt + 1;
        let close = b.get(j) == Some(&b'/');
        if close {
            j += 1;
        }
        if b.get(j..j + nb.len()) == Some(nb) {
            let k = j + nb.len();
            let ok = match b.get(k) {
                Some(&c) => c == b'/' || c == b'>' || js_space_len(b, k) > 0,
                None => false,
            };
            if ok {
                return Some((lt, close));
            }
        }
        i = lt + 1;
    }
    None
}

fn find_elements_n(xml: &str, name: &str, from: usize, to: usize, limit: usize) -> Vec<XmlRange> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cur: Option<(usize, usize)> = None;
    let mut at = from;
    while let Some((idx, close)) = next_tag(xml, name, at) {
        if idx >= to {
            break;
        }
        if close {
            let end = tag_end(xml, idx);
            if depth == 0 {
                at = idx + 2 + name.len(); // orphan close tag: the scan goes on after the match
                continue;
            }
            depth -= 1;
            if depth == 0
                && let Some((start, open_end)) = cur.take()
            {
                out.push(XmlRange { start, end, open_end, self_closing: false });
                if out.len() >= limit {
                    return out;
                }
            }
            at = end;
            continue;
        }
        let end = tag_end(xml, idx);
        let self_closing = end >= 2 && b.get(end - 2) == Some(&b'/');
        if depth == 0 {
            if self_closing {
                out.push(XmlRange { start: idx, end, open_end: end, self_closing: true });
                if out.len() >= limit {
                    return out;
                }
            } else {
                cur = Some((idx, end));
            }
        }
        if !self_closing {
            depth += 1;
        }
        at = end;
    }
    out
}

/// The elements `name` (exact qualified name) inside `[from, to)` in document order; nested ones of
/// the same name count only through the outer one (`findElements`).
pub fn find_elements(xml: &str, name: &str, from: usize, to: usize) -> Vec<XmlRange> {
    find_elements_n(xml, name, from, to, usize::MAX)
}

/// [`find_elements`] over the whole text.
pub fn find_elements_all(xml: &str, name: &str) -> Vec<XmlRange> {
    find_elements_n(xml, name, 0, xml.len(), usize::MAX)
}

/// EVERY element `name`, nested ones of the same name included (`findAllElements`).
pub fn find_all_elements(xml: &str, name: &str, from: usize, to: usize) -> Vec<XmlRange> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut at = from;
    while let Some((idx, close)) = next_tag(xml, name, at) {
        if idx >= to {
            break;
        }
        let end = tag_end(xml, idx);
        if close {
            if let Some((start, open_end)) = stack.pop() {
                out.push(XmlRange { start, end, open_end, self_closing: false });
            }
        } else if end >= 2 && b.get(end - 2) == Some(&b'/') {
            out.push(XmlRange { start: idx, end, open_end: end, self_closing: true });
        } else {
            stack.push((idx, end));
        }
        at = end;
    }
    out.sort_by_key(|r| r.start);
    out
}

/// The first element `name` inside `[from, to)` (`findElement`).
pub fn find_element(xml: &str, name: &str, from: usize, to: usize) -> Option<XmlRange> {
    find_elements_n(xml, name, from, to, 1).into_iter().next()
}

/// The first element `name` of the whole text.
pub fn find_first(xml: &str, name: &str) -> Option<XmlRange> {
    find_element(xml, name, 0, xml.len())
}

/// The first element `name` from `from` to the end.
pub fn find_from(xml: &str, name: &str, from: usize) -> Option<XmlRange> {
    find_element(xml, name, from, xml.len())
}

fn is_name_byte(c: u8) -> bool {
    is_js_word(c) || c == b':' || c == b'.' || c == b'-'
}

/// `/^<([\w:.-]+)/.exec(xml.slice(at, at + 80))?.[1]`: a name is at most 79 characters.
fn name_at(xml: &str, at: usize) -> Option<&str> {
    let b = xml.as_bytes();
    if b.get(at) != Some(&b'<') {
        return None;
    }
    let mut n = 0;
    while n < 79 && b.get(at + 1 + n).is_some_and(|&c| is_name_byte(c)) {
        n += 1;
    }
    if n == 0 { None } else { xml.get(at + 1..at + 1 + n) }
}

/// The qualified name of the element at `r.start` (`elementName`), `""` when there is none.
pub fn element_name(xml: &str, r: &XmlRange) -> String {
    name_at(xml, r.start).unwrap_or("").to_owned()
}

fn close_len(xml: &str, r: &XmlRange) -> usize {
    name_at(xml, r.start).map_or(0, str::len) + 3 // `</name>`
}

/// End of the content of the element (`r.end - '</name>'.length`).
fn inner_end(xml: &str, r: &XmlRange) -> usize {
    r.end.saturating_sub(close_len(xml, r))
}

/// The DIRECT children (any name) of the element in `r` (`childRanges`).
pub fn child_ranges(xml: &str, r: &XmlRange) -> Vec<NamedRange> {
    if r.self_closing {
        return Vec::new();
    }
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let inner_end = inner_end(xml, r);
    let mut i = r.open_end;
    while i < inner_end {
        let Some(off) = b.get(i..).and_then(|s| s.iter().position(|&c| c == b'<')) else { break };
        let lt = i + off;
        if lt >= inner_end {
            break;
        }
        let c = b.get(lt + 1).copied();
        if c == Some(b'/') {
            break;
        }
        if c == Some(b'!') || c == Some(b'?') {
            // comment / CDATA / instruction
            let close: isize = if xml[lt..].starts_with("<!--") {
                find_str(xml, "-->", lt).map_or(-1, |p| isize::try_from(p).unwrap_or(isize::MAX)) + 3
            } else if xml[lt..].starts_with("<![CDATA[") {
                find_str(xml, "]]>", lt).map_or(-1, |p| isize::try_from(p).unwrap_or(isize::MAX)) + 3
            } else {
                isize::try_from(tag_end(xml, lt)).unwrap_or(isize::MAX)
            };
            let lt_i = isize::try_from(lt).unwrap_or(isize::MAX);
            i = if close > lt_i { usize::try_from(close).unwrap_or(inner_end) } else { inner_end };
            continue;
        }
        let Some(name) = name_at(xml, lt) else {
            i = lt + 1;
            continue;
        };
        let el = find_element(xml, name, lt, inner_end);
        match el {
            Some(el) if el.start == lt => {
                out.push(NamedRange { range: el, name: name.to_owned() });
                i = el.end;
            }
            _ => i = tag_end(xml, lt),
        }
    }
    out
}

/// `xml.indexOf(needle, from)`.
pub fn find_str(xml: &str, needle: &str, from: usize) -> Option<usize> {
    xml.get(floor_boundary(xml, from)..)?.find(needle).map(|p| floor_boundary(xml, from) + p)
}

/// A matched attribute of an opening tag: the span of the whole match (with its leading white space)
/// and of the quoted value.
struct AttrMatch {
    start: usize,
    end: usize,
    value: (usize, usize),
}

/// `new RegExp('\\s' + attr + '\\s*=\\s*("([^"]*)"|\'([^\']*)\')').exec(open)`.
fn match_attr(open: &str, attr: &str) -> Option<AttrMatch> {
    let b = open.as_bytes();
    let mut p = 0;
    while p < b.len() {
        let sl_ = js_space_len(b, p);
        if sl_ > 0 && open.get(p + sl_..).is_some_and(|r| r.starts_with(attr)) {
            let mut q = p + sl_ + attr.len();
            while let n @ 1.. = js_space_len(b, q) {
                q += n;
            }
            if b.get(q) == Some(&b'=') {
                q += 1;
                while let n @ 1.. = js_space_len(b, q) {
                    q += n;
                }
                if let Some(&quote) = b.get(q)
                    && (quote == b'"' || quote == b'\'')
                    && let Some(rel) = b.get(q + 1..).and_then(|s| s.iter().position(|&c| c == quote))
                {
                    let close = q + 1 + rel;
                    return Some(AttrMatch { start: p, end: close + 1, value: (q + 1, close) });
                }
            }
        }
        p += if sl_ > 0 { sl_ } else { next_char_len(open, p) };
    }
    None
}

fn next_char_len(s: &str, p: usize) -> usize {
    s.get(p..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8)
}

/// The value of an attribute of the opening tag in `r` (unescaped), or `None` (`getAttr`).
pub fn get_attr(xml: &str, r: &XmlRange, attr: &str) -> Option<String> {
    let open = sl(xml, r.start, r.open_end);
    let m = match_attr(open, attr)?;
    Some(unescape_xml(sl(open, m.value.0, m.value.1)))
}

/// The expansion of a `String.prototype.replace` replacement string (`$$`, `$&`, `` $` ``, `$'`,
/// `$n`) for a match with one capture group.
fn expand_dollar(repl: &str, matched: &str, before: &str, after: &str, group1: &str) -> String {
    let b = repl.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' || i + 1 >= b.len() {
            let n = next_char_len(repl, i);
            out.push_str(&repl[i..i + n]);
            i += n;
            continue;
        }
        match b[i + 1] {
            b'$' => {
                out.push('$');
                i += 2;
            }
            b'&' => {
                out.push_str(matched);
                i += 2;
            }
            b'`' => {
                out.push_str(before);
                i += 2;
            }
            b'\'' => {
                out.push_str(after);
                i += 2;
            }
            d @ b'0'..=b'9' => {
                // two digits first (`$01`..`$99`), valid when 1 <= n <= groups (1)
                let two = b.get(i + 2).filter(|c| c.is_ascii_digit()).map(|c| u32::from(d - b'0') * 10 + u32::from(c - b'0'));
                if two == Some(1) {
                    out.push_str(group1);
                    i += 3;
                } else if d == b'1' {
                    out.push_str(group1);
                    i += 2;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// The tag with the attribute replaced (`None` removes it): the text of the TAG, not the document
/// (`setAttrInTag`). Keeps the TS's use of a replacement STRING: `$` patterns in the escaped value
/// expand as in `String.prototype.replace`.
pub fn set_attr_in_tag(tag: &str, attr: &str, value: Option<&str>) -> String {
    let m = match_attr(tag, attr);
    let Some(value) = value else {
        return match m {
            Some(m) => format!("{}{}", &tag[..m.start], &tag[m.end..]),
            None => tag.to_owned(),
        };
    };
    let v = format!(" {attr}=\"{}\"", escape_attr(value));
    if let Some(m) = m {
        // group 1 is the quoted value with its quotes
        let expanded = expand_dollar(&v, &tag[m.start..m.end], &tag[..m.start], &tag[m.end..], &tag[m.value.0 - 1..m.end]);
        return format!("{}{}{}", &tag[..m.start], expanded, &tag[m.end..]);
    }
    let close = if tag.ends_with("/>") { tag.len() - 2 } else { floor_boundary(tag, tag.len().saturating_sub(1)) };
    let head = &tag[..close];
    let trimmed = head.trim_end_matches(uo_jsnum::text::is_js_space);
    format!("{trimmed}{v}{}", &tag[close..])
}

/// The document with the attribute of the opening tag of `r` replaced (`None` removes it).
pub fn set_attr(xml: &str, r: &XmlRange, attr: &str, value: Option<&str>) -> String {
    let tag = set_attr_in_tag(sl(xml, r.start, r.open_end), attr, value);
    format!("{}{}{}", sl(xml, 0, r.start), tag, sl_from(xml, r.open_end))
}

/// Replaces `[start, end)` with `text` (`xml.slice(0, start) + text + xml.slice(end)`).
pub fn splice(xml: &str, start: usize, end: usize, text: &str) -> String {
    let mut out = String::with_capacity(xml.len() + text.len());
    out.push_str(sl(xml, 0, start));
    out.push_str(text);
    out.push_str(sl_from(xml, end));
    out
}

/// The content between the opening and the closing tag of the element (`innerXml`).
pub fn inner_xml<'a>(xml: &'a str, r: &XmlRange) -> &'a str {
    if r.self_closing {
        return "";
    }
    sl(xml, r.open_end, inner_end(xml, r))
}

/// Replaces the content of the element (a `<x/>` becomes `<x>…</x>`) (`setInner`).
pub fn set_inner(xml: &str, r: &XmlRange, inner: &str) -> String {
    let name = element_name(xml, r);
    if r.self_closing {
        let open = sl(xml, r.start, r.open_end);
        // /\s*\/>$/ -> '>'
        let open = match open.strip_suffix("/>") {
            Some(h) => format!("{}>", h.trim_end_matches(uo_jsnum::text::is_js_space)),
            None => open.to_owned(),
        };
        return splice(xml, r.start, r.end, &format!("{open}{inner}</{name}>"));
    }
    splice(xml, r.open_end, inner_end(xml, r), inner)
}

/// `&`, `<`, `>` escaped.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// `escapeText` plus `"`.
pub fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// `unescapeXml`: the five entities and numeric references (case-insensitive, as the `gi` regexp). A
/// code point outside Unicode scalar values (the TS throws a `RangeError` above U+10FFFF and keeps a lone
/// surrogate) becomes U+FFFD: declared in the record, hostile input only.
pub fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let tail = &rest[p..];
        if let Some((ch, used)) = entity_at(tail) {
            out.push(ch);
            rest = &tail[used..];
        } else {
            out.push('&');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The entity at the start of `t` (`&...;`): the character and the bytes it spans.
fn entity_at(t: &str) -> Option<(char, usize)> {
    let b = t.as_bytes();
    let mut n = 1;
    while b.get(n).is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'#') {
        n += 1;
    }
    if b.get(n) != Some(&b';') {
        return None;
    }
    let semi = n;
    let body = t.get(1..semi)?;
    if let Some(num) = body.strip_prefix('#') {
        let (digits, radix) = match num.strip_prefix(['x', 'X']) {
            Some(h) => (h, 16),
            None => (num, 10),
        };
        if digits.is_empty() || !digits.bytes().all(|c| char::from(c).is_digit(radix)) {
            return None;
        }
        let mut v: u32 = 0;
        for c in digits.bytes() {
            let d = char::from(c).to_digit(radix)?;
            v = v.saturating_mul(radix).saturating_add(d);
        }
        return Some((char::from_u32(v).unwrap_or('\u{fffd}'), semi + 1));
    }
    let ch = match body.to_ascii_lowercase().as_str() {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    };
    Some((ch, semi + 1))
}

/// Inserts `text` as a child of the element in `r`: before the first child whose name is in
/// `before` (schema order), else at the end (`insertChild`).
pub fn insert_child(xml: &str, r: &XmlRange, text: &str, before: &[&str]) -> String {
    if r.self_closing {
        return set_inner(xml, r, text);
    }
    let kids = child_ranges(xml, r);
    let pos = match kids.iter().find(|k| before.contains(&k.name.as_str())) {
        Some(k) => k.range.start,
        None => inner_end(xml, r),
    };
    splice(xml, pos, pos, text)
}

/// JS `String.prototype.trim` of a slice.
pub fn js_trim(s: &str) -> &str {
    uo_jsnum::text::trim(s)
}
