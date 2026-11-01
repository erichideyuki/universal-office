//! OPC part names (ECMA-376 part 2) in the form of zip entry names: no leading slash
//! (`word/document.xml`), characters already unescaped. Port of the C#
//! `UniversalOffice.Core.OpcPartName` — THE single rule for resolving relationship targets
//! (gate 1, G3/G5): a host's validator must use the same function, so that "the part the
//! validator checked" and "the part the library opens" are always the same one.
//!
//! The TS apps resolve targets with several ad hoc rules today (Write `word/${target}`,
//! sheet/slides `resolveRelTarget`); during parity each format crate keeps its app's rule as a
//! policy, and switching them to this one is a stage-5 decision with oracle evidence.

/// Resolves the `Target` of an INTERNAL relationship to the name of the target part.
///
/// Rule: the fragment (`#…`) is dropped; a target starting with `/` is absolute, otherwise it is
/// relative to the folder of `source_part_name` (`""` = package root, for `_rels/.rels`); empty
/// and `.` segments vanish and `..` goes up one level; each segment is unescaped ONCE (`%2531`
/// becomes the text `%31`, never `1`).
///
/// Returns `None` for a target that names no part: empty, with a scheme (`http:`…), with a
/// query (`?`) or a backslash, `..` above the root, a malformed `%` escape, or a segment that
/// unescapes to empty, `.`, `..`, or contains `/`, `\` or a control character.
///
/// The returned name is compared with the zip names case-insensitively (as OPC requires) and
/// must not be unescaped again.
pub fn resolve_relationship_target(source_part_name: &str, target: &str) -> Option<String> {
    let reference = target.split('#').next().unwrap_or("");
    if reference.is_empty() || has_scheme(reference) || reference.contains('?') || reference.contains('\\') {
        return None;
    }
    let mut segments: Vec<String> = Vec::new();
    if !reference.starts_with('/') {
        let source = source_part_name.trim_start_matches('/');
        if let Some(slash) = source.rfind('/').filter(|&s| s > 0) {
            for folder in source[..slash].split('/') {
                if folder == "." || folder == ".." {
                    return None;
                }
                if !folder.is_empty() {
                    segments.push(folder.to_string());
                }
            }
        }
    }
    for raw in reference.split('/') {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            segments.pop()?;
            continue;
        }
        segments.push(decode_segment(raw)?);
    }
    if segments.is_empty() { None } else { Some(segments.join("/")) }
}

/// Escapes a part-name segment for writing into a `Target` (the inverse of the resolution:
/// [`resolve_relationship_target`] gives back the original text). Like .NET
/// `Uri.EscapeDataString`: everything but the unreserved characters becomes `%XX` of its UTF-8
/// bytes, uppercase hex — and a segment made only of unreserved characters is returned as is.
pub fn escape_segment(segment: &str) -> String {
    if segment.chars().all(is_unreserved) {
        return segment.to_string();
    }
    let mut out = String::with_capacity(segment.len() * 3);
    for b in segment.bytes() {
        if b.is_ascii() && is_unreserved(b as char) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize]);
            out.push(HEX[(b & 0x0f) as usize]);
        }
    }
    out
}

const HEX: [char; 16] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F'];

fn is_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

/// ASCII case folding of a part name, the comparison OPC specifies. Hosts compare names with
/// this (the format crates choose it or the TS Unicode lowering as a parity policy).
pub fn fold_ascii(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// The segment unescaped once; `None` if the escape is malformed or the result cannot be a
/// segment.
fn decode_segment(raw: &str) -> Option<String> {
    let decoded = if raw.contains('%') { unescape_data_string(raw)? } else { raw.to_string() };
    if decoded.is_empty() || decoded == "." || decoded == ".." {
        return None;
    }
    if decoded.chars().any(|c| c == '/' || c == '\\' || c.is_control()) {
        return None;
    }
    Some(decoded)
}

/// .NET `Uri.UnescapeDataString` after the C# well-formedness check: every `%` must be
/// followed by two hex digits (else `None`). Runs of `%XX` are decoded as UTF-8; bytes that do
/// not form valid UTF-8 stay as their original `%XX` text (as .NET leaves them).
fn unescape_data_string(raw: &str) -> Option<String> {
    let b = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < b.len() {
        if b.get(i) != Some(&b'%') {
            // copy one char (multi-byte safe)
            let ch = raw.get(i..)?.chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // a run of %XX escapes
        let start = i;
        let mut bytes = Vec::new();
        while b.get(i) == Some(&b'%') {
            let hi = hex_val(*b.get(i + 1)?)?;
            let lo = hex_val(*b.get(i + 2)?)?;
            bytes.push(hi << 4 | lo);
            i += 3;
        }
        push_utf8_or_escapes(&mut out, &bytes, raw.get(start..i)?);
    }
    Some(out)
}

/// Appends the decoded run; invalid UTF-8 sequences keep their escaped text (3 chars per byte).
fn push_utf8_or_escapes(out: &mut String, bytes: &[u8], escaped: &str) {
    let mut pos = 0;
    while pos < bytes.len() {
        let rest = bytes.get(pos..).unwrap_or(&[]);
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if let Ok(s) = std::str::from_utf8(rest.get(..valid).unwrap_or(&[])) {
                    out.push_str(s);
                }
                let bad = e.error_len().unwrap_or(rest.len() - valid);
                let from = (pos + valid) * 3;
                if let Some(text) = escaped.get(from..from + bad * 3) {
                    out.push_str(text);
                }
                pos += valid + bad;
            }
        }
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// The first segment has a `:` (a scheme, or ambiguous with one in a relative reference).
fn has_scheme(reference: &str) -> bool {
    match reference.find(':') {
        None => false,
        Some(colon) => reference.find('/').is_none_or(|slash| colon < slash),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(src: &str, t: &str) -> Option<String> {
        resolve_relationship_target(src, t)
    }

    #[test]
    fn relative_absolute_and_root() {
        assert_eq!(r("word/document.xml", "media/image1.png").as_deref(), Some("word/media/image1.png"));
        assert_eq!(r("word/document.xml", "../customXml/item1.xml").as_deref(), Some("customXml/item1.xml"));
        assert_eq!(r("word/document.xml", "./media/x.png").as_deref(), Some("word/media/x.png"));
        assert_eq!(r("word/document.xml", "/word/styles.xml").as_deref(), Some("word/styles.xml"));
        assert_eq!(r("", "word/document.xml").as_deref(), Some("word/document.xml"));
        assert_eq!(r("/word/document.xml", "media//x.png").as_deref(), Some("word/media/x.png"));
        assert_eq!(r("word/document.xml", "media/x.png#frag").as_deref(), Some("word/media/x.png"));
    }

    #[test]
    fn rejections() {
        assert_eq!(r("word/document.xml", ""), None);
        assert_eq!(r("word/document.xml", "#only"), None);
        assert_eq!(r("word/document.xml", "http://example.com/x"), None);
        assert_eq!(r("word/document.xml", "c:x"), None);
        assert_eq!(r("word/document.xml", "media/x.png?y"), None);
        assert_eq!(r("word/document.xml", "media\\x.png"), None);
        assert_eq!(r("word/document.xml", "../../x"), None);
        assert_eq!(r("word/document.xml", "media/%2"), None);
        assert_eq!(r("word/document.xml", "media/%zz"), None);
        assert_eq!(r("word/document.xml", "media/%2e%2e"), None);
        assert_eq!(r("word/document.xml", "media/a%2Fb"), None);
        assert_eq!(r("word/document.xml", "media/a%01b"), None);
        assert_eq!(r("word/./document.xml", "x"), None);
        // like the C#: "." segments vanish, so the source folder itself is the result
        assert_eq!(r("word/document.xml", ".").as_deref(), Some("word"));
    }

    #[test]
    fn unescape_once_and_invalid_utf8_kept_literal() {
        assert_eq!(r("", "a%2531").as_deref(), Some("a%31"));
        assert_eq!(r("", "caf%C3%A9.xml").as_deref(), Some("café.xml"));
        assert_eq!(r("", "x%FF.xml").as_deref(), Some("x%FF.xml"));
        assert_eq!(r("", "x%C3%A9%FF.xml").as_deref(), Some("xé%FF.xml"));
        assert_eq!(r("", "x%c3.xml").as_deref(), Some("x%c3.xml"));
    }

    #[test]
    fn scheme_only_before_the_first_slash() {
        assert_eq!(r("", "a/b:c").as_deref(), Some("a/b:c"));
    }

    #[test]
    fn escape_segment_like_dotnet() {
        assert_eq!(escape_segment("image1.png"), "image1.png");
        assert_eq!(escape_segment("my file.png"), "my%20file.png");
        assert_eq!(escape_segment("café"), "caf%C3%A9");
        assert_eq!(escape_segment("100%"), "100%25");
        for s in ["my file.png", "café", "100%", "a~b-c_d.e"] {
            assert_eq!(r("", &escape_segment(s)).as_deref(), Some(s));
        }
    }

    #[test]
    fn no_leak() {
        uo_alloc::assert_no_leak(|| (r("word/document.xml", "../a/%C3%A9/x.png"), r("", "%FF"), escape_segment("a b")));
    }
}
