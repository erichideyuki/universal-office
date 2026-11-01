//! XML writing: the escapes of the TS writers, byte for byte, and a small element writer with
//! the semantics of `write/packages/docx-core/src/doc/xmlw.ts` (`el`, `attrs`, `XML_DECL`).
//!
//! The TS has several escape variants (each a chain of `replace`); they are all here:
//! - [`esc_text`]: `& < >` (serialize.ts, export/model.ts, sheet serialize.ts, …);
//! - [`esc_attr`]: `& < > "` (lossless.ts `esc`, table-xml.ts, shared/fonts ooxml.ts, …);
//! - [`esc_attr_no_gt`]: `& " <`, `>` left as is (page-background.ts:125, numbering-store.ts:104);
//! - [`esc_all`]: `& < > " '` (odf/write-odt.ts:55, editor xps.ts:51);
//! - [`esc_text_valid`] / [`esc_attr_valid`]: `stripInvalid` first (doc/xmlw.ts, ppt/xmlw.ts).

use std::borrow::Cow;

/// The XML declaration of `doc/xmlw.ts` (CRLF after it, as Word writes).
pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
/// The XML declaration of `ppt/xmlw.ts` (LF).
pub const XML_DECL_LF: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
/// The declaration without line break (package.ts `empty()`, blank documents).
pub const XML_DECL_BARE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// Which characters an escape replaces (`&` and `<` always).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Escape {
    /// `>` → `&gt;`
    pub gt: bool,
    /// `"` → `&quot;`
    pub quot: bool,
    /// `'` → `&apos;`
    pub apos: bool,
    /// drop characters XML 1.0 forbids first (`stripInvalid`)
    pub strip_invalid: bool,
}

impl Escape {
    pub const TEXT: Escape = Escape { gt: true, quot: false, apos: false, strip_invalid: false };
    pub const ATTR: Escape = Escape { gt: true, quot: true, apos: false, strip_invalid: false };
    pub const ATTR_NO_GT: Escape = Escape { gt: false, quot: true, apos: false, strip_invalid: false };
    pub const ALL: Escape = Escape { gt: true, quot: true, apos: true, strip_invalid: false };
    pub const TEXT_VALID: Escape = Escape { gt: true, quot: false, apos: false, strip_invalid: true };
    pub const ATTR_VALID: Escape = Escape { gt: true, quot: true, apos: false, strip_invalid: true };
}

/// A character XML 1.0 forbids: C0 controls except TAB/LF/CR, U+FFFE, U+FFFF. Lone surrogates
/// (also dropped by docx-core `stripInvalid`) cannot exist in a Rust `str`; the slides-core copy
/// kept them, a difference that only a JS string could show.
#[inline]
fn invalid(c: char) -> bool {
    (c < '\u{20}' && !matches!(c, '\t' | '\n' | '\r')) || c == '\u{FFFE}' || c == '\u{FFFF}'
}

#[inline]
fn needs(c: char, e: Escape) -> bool {
    match c {
        '&' | '<' => true,
        '>' => e.gt,
        '"' => e.quot,
        '\'' => e.apos,
        _ => e.strip_invalid && invalid(c),
    }
}

/// Appends `s` escaped.
pub fn escape_into(out: &mut String, s: &str, e: Escape) {
    let mut last = 0;
    for (k, c) in s.char_indices() {
        if !needs(c, e) {
            continue;
        }
        out.push_str(&s[last..k]);
        last = k + c.len_utf8();
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => {}
        }
    }
    out.push_str(&s[last..]);
}

/// `s` escaped (borrowed when nothing changes).
pub fn escape(s: &str, e: Escape) -> Cow<'_, str> {
    if !s.chars().any(|c| needs(c, e)) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    escape_into(&mut out, s, e);
    Cow::Owned(out)
}

/// `stripInvalid` alone.
pub fn strip_invalid(s: &str) -> Cow<'_, str> {
    if !s.chars().any(invalid) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().filter(|&c| !invalid(c)).collect())
}

/// `& < >`.
pub fn esc_text(s: &str) -> Cow<'_, str> {
    escape(s, Escape::TEXT)
}
/// `& < > "`.
pub fn esc_attr(s: &str) -> Cow<'_, str> {
    escape(s, Escape::ATTR)
}
/// `& < "` (`>` kept).
pub fn esc_attr_no_gt(s: &str) -> Cow<'_, str> {
    escape(s, Escape::ATTR_NO_GT)
}
/// `& < > " '`.
pub fn esc_all(s: &str) -> Cow<'_, str> {
    escape(s, Escape::ALL)
}
/// xmlw.ts `escText`: invalid characters dropped, then `& < >`.
pub fn esc_text_valid(s: &str) -> Cow<'_, str> {
    escape(s, Escape::TEXT_VALID)
}
/// xmlw.ts `escAttr`: invalid characters dropped, then `& < > "`.
pub fn esc_attr_valid(s: &str) -> Cow<'_, str> {
    escape(s, Escape::ATTR_VALID)
}

/// xmlw.ts `attrs`: ` k="v"` for each attribute with a value (`None` is skipped, like
/// `undefined`/`null`/`false`), values escaped with [`esc_attr_valid`]. Callers format numbers
/// and turn `true` into `"1"`.
pub fn write_attrs(out: &mut String, attrs: &[(&str, Option<&str>)]) {
    for (k, v) in attrs {
        let Some(v) = v else { continue };
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_into(out, v, Escape::ATTR_VALID);
        out.push('"');
    }
}

/// xmlw.ts `el`: `<name attrs/>` when `content` is empty, else `<name attrs>content</name>`
/// (`content` is already XML).
pub fn el(name: &str, attrs: &[(&str, Option<&str>)], content: &str) -> String {
    let mut out = String::with_capacity(name.len() * 2 + content.len() + 8);
    out.push('<');
    out.push_str(name);
    write_attrs(&mut out, attrs);
    if content.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(content);
        out.push_str("</");
        out.push_str(name);
        out.push('>');
    }
    out
}

/// Streaming element writer with the `el` conventions: an element without content is written
/// `<x/>`; text and attribute values go through the xmlw.ts escapes (invalid characters
/// dropped). Names are written as given.
#[derive(Debug, Default)]
pub struct XmlWriter {
    out: String,
    stack: Vec<(usize, usize)>,
    /// the start tag of the innermost element is still open (`<x a="1"` without `>`)
    pending: bool,
}

impl XmlWriter {
    /// An empty writer.
    pub fn new() -> Self {
        XmlWriter::default()
    }

    /// A writer that starts with a declaration (e.g. [`XML_DECL`]).
    pub fn with_declaration(decl: &str) -> Self {
        XmlWriter { out: decl.to_owned(), ..XmlWriter::default() }
    }

    fn close_pending(&mut self) {
        if self.pending {
            self.out.push('>');
            self.pending = false;
        }
    }

    /// Opens an element.
    pub fn start(&mut self, name: &str) -> &mut Self {
        self.close_pending();
        self.out.push('<');
        let at = self.out.len();
        self.out.push_str(name);
        self.stack.push((at, name.len()));
        self.pending = true;
        self
    }

    /// An attribute of the element just opened (ignored once content was written).
    pub fn attr(&mut self, name: &str, value: &str) -> &mut Self {
        if self.pending {
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value, Escape::ATTR_VALID);
            self.out.push('"');
        }
        self
    }

    /// An optional attribute (`None` skipped, as `undefined` in `attrs`).
    pub fn attr_opt(&mut self, name: &str, value: Option<&str>) -> &mut Self {
        if let Some(v) = value {
            self.attr(name, v);
        }
        self
    }

    /// Escaped text; empty text writes nothing (the element stays `<x/>`).
    pub fn text(&mut self, s: &str) -> &mut Self {
        if !s.is_empty() {
            self.close_pending();
            escape_into(&mut self.out, s, Escape::TEXT_VALID);
        }
        self
    }

    /// XML written as is.
    pub fn raw(&mut self, xml: &str) -> &mut Self {
        if !xml.is_empty() {
            self.close_pending();
            self.out.push_str(xml);
        }
        self
    }

    /// Closes the innermost element (`/>` when it has no content).
    pub fn end(&mut self) -> &mut Self {
        let Some((at, len)) = self.stack.pop() else { return self };
        if self.pending {
            self.out.push_str("/>");
            self.pending = false;
        } else {
            self.out.push_str("</");
            let name = self.out.get(at..at + len).unwrap_or("").to_owned();
            self.out.push_str(&name);
            self.out.push('>');
        }
        self
    }

    /// Closes every open element and returns the text.
    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.end();
        }
        self.out
    }
}
