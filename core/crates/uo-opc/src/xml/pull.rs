//! The XML pull tokenizer: the grammar of the sheet-core parser (`sheet/packages/sheet-core/src/xml.ts`) over a
//! byte stream that arrives in pieces (Sheet C1a: a worksheet is inflated 64 KiB at a time and tokenized as it
//! comes, never held as one text or one tree).
//!
//! Same quirks as the TS and as the lenient [`super::read`] preset it mirrors: close-tag names are not checked,
//! comments and CDATA are recognized inside content only, the prolog skips `<?…?>`, `<!--…-->` and `<!…>`, a name
//! ends at JS white space, `/` or `>` (an attribute name only at `=` or white space), content after the root is
//! ignored, an unclosed element at the end is an error. A processing instruction inside content is an element
//! named `?target` (as the TS). Text and attribute values are handed over RAW (a slice of the input): decoding
//! (end-of-line and attribute normalization, entities, `fromCodePoint` throwing past U+10FFFF) is
//! [`decode_text`]/[`decode_attr`], which produce UTF-16 units so a lone surrogate from `&#xD800;` survives as
//! in a JS string.
//!
//! Tokens borrow from the piece being fed (or from the carry of a token cut by a piece boundary), so the
//! callback must copy what it keeps. Offsets are absolute positions in the stream (BOM excluded).

use super::entities::{CharRefPolicy, RefMatch, dec_value, hex_value, match_ref, named};
use crate::error::{Error, LimitKind, Result, XmlErrorKind};
use uo_jsnum::text::js_space_len;

/// What a token callback asks the tokenizer to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// keep going
    Continue,
    /// stop after this token ([`FeedEnd::Stopped`])
    Stop,
}

/// An opening (or self-closing) tag.
#[derive(Debug, Clone, Copy)]
pub struct StartTag<'a> {
    /// the name as written (`x:sheetData`)
    pub name: &'a [u8],
    /// the bytes between the name and the closing `>`/`/>`
    pub attr_bytes: &'a [u8],
    /// `<name …/>`
    pub self_closing: bool,
    /// nesting depth of this element (the root is 1)
    pub depth: u32,
    /// absolute offset of its `<`
    pub start: u64,
    /// absolute offset after its `>`
    pub end: u64,
}

impl<'a> StartTag<'a> {
    /// The attributes in order (raw values; a repeated name is repeated, the last one wins as in the TS object).
    pub fn attrs(&self) -> Attrs<'a> {
        Attrs { b: self.attr_bytes, i: 0 }
    }

    /// The raw value of attribute `name` (the last of its repeats).
    pub fn attr(&self, name: &[u8]) -> Option<&'a [u8]> {
        self.attrs().filter(|(n, _)| *n == name).last().map(|(_, v)| v)
    }
}

/// Iterator over the attributes of a tag already validated by the tokenizer.
#[derive(Debug, Clone, Copy)]
pub struct Attrs<'a> {
    b: &'a [u8],
    i: usize,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    loop {
        let n = js_space_len(b, i);
        if n == 0 {
            return i;
        }
        i += n;
    }
}

impl<'a> Attrs<'a> {
    /// The attributes of a tag's attribute bytes ([`StartTag::attr_bytes`]) kept by the caller.
    pub fn from_bytes(b: &'a [u8]) -> Self {
        Attrs { b, i: 0 }
    }
}

impl<'a> Iterator for Attrs<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.b;
        let mut i = skip_ws(b, self.i);
        if i >= b.len() {
            return None;
        }
        let a0 = i;
        while i < b.len() && b[i] != b'=' && js_space_len(b, i) == 0 {
            i += 1;
        }
        let name = b.get(a0..i)?;
        i = skip_ws(b, i);
        i += 1; // '='
        i = skip_ws(b, i);
        let q = *b.get(i)?;
        i += 1;
        let v0 = i;
        let len = b.get(i..)?.iter().position(|&c| c == q)?;
        i += len;
        self.i = i + 1;
        Some((name, b.get(v0..i)?))
    }
}

/// One token.
#[derive(Debug, Clone, Copy)]
pub enum Token<'a> {
    /// `<name attrs>` or `<name attrs/>`
    Start(StartTag<'a>),
    /// `</name>` (the name is whatever stands between `</` and `>`)
    End {
        /// raw bytes after `</`
        name: &'a [u8],
        /// absolute offset of `<`
        start: u64,
        /// absolute offset after `>`
        end: u64,
    },
    /// raw text between markup (never empty)
    Text(&'a [u8]),
    /// the content of `<![CDATA[…]]>`
    CData(&'a [u8]),
}

/// How a [`Pull::feed`] call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedEnd {
    /// every complete token was handed over; feed more (or call with `last`)
    More,
    /// the callback returned [`Flow::Stop`] after the token that ends at this absolute offset
    Stopped(u64),
    /// the root element closed (content after it is ignored); the offset is after its end tag
    RootClosed(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Prolog,
    Content,
    Done(u64),
}

/// Resumable tokenizer.
#[derive(Debug)]
pub struct Pull {
    carry: Vec<u8>,
    carry_base: u64,
    fed: u64,
    state: State,
    depth: u32,
    max_depth: u32,
    max_token: usize,
    /// bytes of the pending token already searched for its terminator
    searched: usize,
    started: bool,
}

impl Default for Pull {
    fn default() -> Self {
        Self::new()
    }
}

enum Step {
    Token(usize),
    NeedMore,
}

/// `Some(true)`: `pat` is at `i`; `Some(false)`: it is not; `None`: the buffer ends inside a possible match.
fn prefix_at(b: &[u8], i: usize, pat: &[u8]) -> Option<bool> {
    let rest = b.get(i..).unwrap_or(&[]);
    let n = rest.len().min(pat.len());
    if rest.get(..n) != pat.get(..n) {
        return Some(false);
    }
    if n < pat.len() { None } else { Some(true) }
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    let hay = b.get(from..)?;
    match pat {
        [one] => hay.iter().position(|c| c == one).map(|p| from + p),
        _ => hay.windows(pat.len()).position(|w| w == pat).map(|p| from + p),
    }
}

impl Pull {
    /// A tokenizer at the start of a document.
    pub fn new() -> Self {
        Pull {
            carry: Vec::new(),
            carry_base: 0,
            fed: 0,
            state: State::Prolog,
            depth: 0,
            max_depth: 1_000,
            max_token: 8 << 20,
            searched: 0,
            started: false,
        }
    }

    /// Element nesting allowed (the TS recurses without a limit; the default is 1,000).
    #[must_use]
    pub fn with_max_depth(mut self, max_depth: u32) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// The largest single token (text run, tag, comment) kept across pieces; default 8 MiB.
    #[must_use]
    pub fn with_max_token(mut self, max_token: usize) -> Self {
        self.max_token = max_token;
        self
    }

    /// Bytes held between pieces (a token cut by a boundary).
    pub fn carried(&self) -> usize {
        self.carry.len()
    }

    /// Hands every complete token of what has been fed so far (this piece after the carry of the last one) to
    /// `f`. `last` marks the end of the document: whatever is unfinished then is an error.
    pub fn feed<F>(&mut self, data: &[u8], last: bool, mut f: F) -> Result<FeedEnd>
    where
        F: FnMut(Token<'_>) -> Result<Flow>,
    {
        if let State::Done(end) = self.state {
            return Ok(FeedEnd::RootClosed(end));
        }
        let mut data = data;
        if !self.started && !data.is_empty() {
            // the UTF-8 BOM (TextDecoder strips it); a BOM split across pieces is not handled by design: the
            // first piece of a part is 64 KiB
            self.started = true;
            if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
                data = rest;
            }
        }
        let abs = self.fed;
        self.fed += data.len() as u64;
        let mut out;
        if self.carry.is_empty() {
            let (used, o) = self.run(data, abs, last, &mut f)?;
            out = o;
            if !matches!(out, Some(FeedEnd::Stopped(_) | FeedEnd::RootClosed(_))) {
                self.carry.extend_from_slice(data.get(used..).unwrap_or(&[]));
                self.carry_base = abs + used as u64;
            } else {
                self.carry.clear();
            }
        } else {
            self.carry.extend_from_slice(data);
            let buf = std::mem::take(&mut self.carry);
            let r = self.run(&buf, self.carry_base, last, &mut f);
            self.carry = buf;
            let (used, o) = r?;
            out = o;
            if matches!(out, Some(FeedEnd::Stopped(_) | FeedEnd::RootClosed(_))) {
                self.carry.clear();
            } else {
                self.carry.drain(..used.min(self.carry.len()));
                self.carry_base += used as u64;
            }
        }
        if self.carry.len() > self.max_token {
            return Err(Error::Limit { kind: LimitKind::PartBytes, max: self.max_token as u64, part: None });
        }
        if let Some(FeedEnd::RootClosed(e)) = out {
            self.state = State::Done(e);
        }
        if out.is_none() && last {
            out = Some(FeedEnd::More);
        }
        Ok(out.unwrap_or(FeedEnd::More))
    }

    fn unclosed(at: u64) -> Error {
        Error::Xml { offset: usize::try_from(at).unwrap_or(usize::MAX), kind: XmlErrorKind::Unclosed, part: None }
    }

    /// Tokenizes `b` (absolute offset `base`); returns the bytes consumed and how it ended (`None` = ran out).
    fn run<F>(&mut self, b: &[u8], base: u64, last: bool, f: &mut F) -> Result<(usize, Option<FeedEnd>)>
    where
        F: FnMut(Token<'_>) -> Result<Flow>,
    {
        let mut i = 0usize;
        loop {
            let step = match self.state {
                State::Done(e) => return Ok((i, Some(FeedEnd::RootClosed(e)))),
                State::Prolog => self.prolog(b, i, base, last)?,
                State::Content => match self.content(b, i, base, last, f)? {
                    Ok2::Step(s) => s,
                    Ok2::End(next, end) => return Ok((next, Some(end))),
                },
            };
            match step {
                Step::Token(next) => {
                    i = next;
                    self.searched = 0;
                }
                Step::NeedMore => {
                    if last {
                        return Err(match self.state {
                            State::Prolog => {
                                Error::Xml { offset: usize::try_from(base).unwrap_or(0) + i, kind: XmlErrorKind::NoRoot, part: None }
                            }
                            _ => Self::unclosed(base + i as u64),
                        });
                    }
                    return Ok((i, None));
                }
            }
        }
    }

    /// Prolog: white space, `<?…?>`, `<!--…-->`, `<!…>`, then the root element's start tag (handled as content).
    fn prolog(&mut self, b: &[u8], i: usize, base: u64, last: bool) -> Result<Step> {
        let mut i = i;
        // white space (a possible multi-byte space cut by the end of the piece waits)
        loop {
            let n = js_space_len(b, i);
            if n > 0 {
                i += n;
                continue;
            }
            if i >= b.len() {
                return Ok(Step::NeedMore);
            }
            if b[i] >= 0xC2 && b.len() - i < 3 && !last {
                return Ok(Step::NeedMore);
            }
            break;
        }
        for (open, close, extra) in [(&b"<?"[..], &b"?>"[..], 2usize), (&b"<!--"[..], &b"-->"[..], 4), (&b"<!"[..], &b">"[..], 2)] {
            match prefix_at(b, i, open) {
                None => return Ok(Step::NeedMore),
                Some(true) => {
                    // `<!--` before the generic `<!`: the table order decides
                    return Ok(match find(b, i + extra + self.searched.min(b.len().saturating_sub(i + extra)), close) {
                        Some(p) => Step::Token(p + close.len()),
                        None => {
                            self.searched = b.len().saturating_sub(i + extra + close.len());
                            Step::NeedMore
                        }
                    });
                }
                Some(false) => {}
            }
        }
        if b[i] != b'<' {
            return Err(Error::Xml { offset: usize::try_from(base).unwrap_or(0) + i, kind: XmlErrorKind::Unexpected, part: None });
        }
        self.state = State::Content;
        // the next loop turn parses the start tag; `i` stays
        Ok(Step::Token(i))
    }

    fn content<F>(&mut self, b: &[u8], i: usize, base: u64, last: bool, f: &mut F) -> Result<Ok2>
    where
        F: FnMut(Token<'_>) -> Result<Flow>,
    {
        let Some(&c) = b.get(i) else { return Ok(Ok2::Step(Step::NeedMore)) };
        if c != b'<' {
            // text up to the next `<`
            let from = i + self.searched.min(b.len() - i);
            return match find(b, from, b"<") {
                Some(j) => {
                    let flow = f(Token::Text(b.get(i..j).unwrap_or(&[])))?;
                    if flow == Flow::Stop {
                        return Ok(Ok2::End(j, FeedEnd::Stopped(base + j as u64)));
                    }
                    Ok(Ok2::Step(Step::Token(j)))
                }
                None => {
                    self.searched = b.len() - i;
                    Ok(Ok2::Step(Step::NeedMore))
                }
            };
        }
        // `</`
        match prefix_at(b, i, b"</") {
            None => return Ok(Ok2::Step(Step::NeedMore)),
            Some(true) => {
                let from = i + 2 + self.searched.min(b.len() - i - 2);
                let Some(j) = find(b, from, b">") else {
                    self.searched = b.len() - i - 2;
                    return Ok(Ok2::Step(Step::NeedMore));
                };
                let end = base + j as u64 + 1;
                let flow = f(Token::End { name: b.get(i + 2..j).unwrap_or(&[]), start: base + i as u64, end })?;
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    return Ok(Ok2::End(j + 1, FeedEnd::RootClosed(end)));
                }
                if flow == Flow::Stop {
                    return Ok(Ok2::End(j + 1, FeedEnd::Stopped(end)));
                }
                return Ok(Ok2::Step(Step::Token(j + 1)));
            }
            Some(false) => {}
        }
        // comment
        match prefix_at(b, i, b"<!--") {
            None => return Ok(Ok2::Step(Step::NeedMore)),
            Some(true) => {
                let from = i + 4 + self.searched.min(b.len() - i - 4);
                return Ok(Ok2::Step(match find(b, from, b"-->") {
                    Some(p) => Step::Token(p + 3),
                    None => {
                        self.searched = (b.len() - i - 4).saturating_sub(2);
                        Step::NeedMore
                    }
                }));
            }
            Some(false) => {}
        }
        // CDATA
        match prefix_at(b, i, b"<![CDATA[") {
            None => return Ok(Ok2::Step(Step::NeedMore)),
            Some(true) => {
                let from = i + 9 + self.searched.min(b.len() - i - 9);
                let Some(p) = find(b, from, b"]]>") else {
                    self.searched = (b.len() - i - 9).saturating_sub(2);
                    return Ok(Ok2::Step(Step::NeedMore));
                };
                let flow = f(Token::CData(b.get(i + 9..p).unwrap_or(&[])))?;
                if flow == Flow::Stop {
                    return Ok(Ok2::End(p + 3, FeedEnd::Stopped(base + p as u64 + 3)));
                }
                return Ok(Ok2::Step(Step::Token(p + 3)));
            }
            Some(false) => {}
        }
        // start tag
        let unexpected =
            |at: usize| Error::Xml { offset: usize::try_from(base).unwrap_or(0) + at, kind: XmlErrorKind::Unexpected, part: None };
        let mut p = i + 1;
        let name0 = p;
        loop {
            if p >= b.len() {
                return Ok(Ok2::Step(Step::NeedMore));
            }
            if matches!(b[p], b'/' | b'>') || js_space_len(b, p) > 0 {
                break;
            }
            if b[p] >= 0xC2 && b.len() - p < 3 && !last {
                return Ok(Ok2::Step(Step::NeedMore));
            }
            p += 1;
        }
        let name = b.get(name0..p).unwrap_or(&[]);
        let attr0 = p;
        let self_closing;
        let attr_end;
        loop {
            // multi-byte space cut by the end
            if b.len() - p.min(b.len()) < 3 && b.get(p).is_some_and(|&c| c >= 0xC2) && !last {
                return Ok(Ok2::Step(Step::NeedMore));
            }
            p = skip_ws(b, p);
            let Some(&c) = b.get(p) else { return Ok(Ok2::Step(Step::NeedMore)) };
            if c == b'/' {
                let Some(&n) = b.get(p + 1) else { return Ok(Ok2::Step(Step::NeedMore)) };
                if n != b'>' {
                    return Err(unexpected(p));
                }
                attr_end = p;
                p += 2;
                self_closing = true;
                break;
            }
            if c == b'>' {
                attr_end = p;
                p += 1;
                self_closing = false;
                break;
            }
            // attribute name up to `=` or white space
            while p < b.len() && b[p] != b'=' && js_space_len(b, p) == 0 {
                if b[p] >= 0xC2 && b.len() - p < 3 && !last {
                    return Ok(Ok2::Step(Step::NeedMore));
                }
                p += 1;
            }
            if p >= b.len() {
                return Ok(Ok2::Step(Step::NeedMore));
            }
            p = skip_ws(b, p);
            let Some(&eq) = b.get(p) else { return Ok(Ok2::Step(Step::NeedMore)) };
            if eq != b'=' {
                return Err(unexpected(p));
            }
            p = skip_ws(b, p + 1);
            let Some(&q) = b.get(p) else { return Ok(Ok2::Step(Step::NeedMore)) };
            if q != b'"' && q != b'\'' {
                return Err(unexpected(p));
            }
            p += 1;
            match find(b, p, &[q]) {
                Some(e) => p = e + 1,
                None => return Ok(Ok2::Step(Step::NeedMore)),
            }
        }
        if self.depth + 1 > self.max_depth {
            return Err(Error::Limit { kind: LimitKind::XmlDepth, max: u64::from(self.max_depth), part: None });
        }
        let tag = StartTag {
            name,
            attr_bytes: b.get(attr0..attr_end).unwrap_or(&[]),
            self_closing,
            depth: self.depth + 1,
            start: base + i as u64,
            end: base + p as u64,
        };
        let end_abs = tag.end;
        let flow = f(Token::Start(tag))?;
        if self_closing {
            if self.depth == 0 {
                return Ok(Ok2::End(p, FeedEnd::RootClosed(end_abs)));
            }
        } else {
            self.depth += 1;
        }
        if flow == Flow::Stop {
            return Ok(Ok2::End(p, FeedEnd::Stopped(end_abs)));
        }
        Ok(Ok2::Step(Step::Token(p)))
    }
}

enum Ok2 {
    Step(Step),
    End(usize, FeedEnd),
}

// ---------------------------------------------------------------- decoding

/// What literal normalization a raw value gets before its references are decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Norm2 {
    Eol,
    Attr,
}

fn normalize(s: &str, norm: Norm2) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match norm {
        Norm2::Eol => {
            if !s.contains('\r') {
                return Cow::Borrowed(s);
            }
            Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
        }
        Norm2::Attr => {
            if !s.bytes().any(|c| matches!(c, b'\r' | b'\n' | b'\t')) {
                return Cow::Borrowed(s);
            }
            let mut o = String::with_capacity(s.len());
            let mut prev_cr = false;
            for ch in s.chars() {
                match ch {
                    '\r' => {
                        o.push(' ');
                        prev_cr = true;
                        continue;
                    }
                    '\n' if prev_cr => {}
                    '\n' | '\t' => o.push(' '),
                    c => o.push(c),
                }
                prev_cr = false;
            }
            Cow::Owned(o)
        }
    }
}

fn decode_units(raw: &[u8], norm: Norm2, refs: bool, out: &mut Vec<u16>) -> Result<()> {
    let text = String::from_utf8_lossy(raw);
    let s = normalize(&text, norm);
    if !refs {
        out.extend(s.encode_utf16());
        return Ok(());
    }
    let b = s.as_bytes();
    let mut lit = 0usize;
    let mut i = 0usize;
    while let Some(k) = b.get(i..).and_then(|r| r.iter().position(|&c| c == b'&')) {
        let amp = i + k;
        let Some((m, next)) = match_ref(b, amp) else {
            i = amp + 1;
            continue;
        };
        out.extend(s.get(lit..amp).unwrap_or("").encode_utf16());
        let cp = match m {
            RefMatch::Name(a, e) => {
                match named(b.get(a..e).unwrap_or(&[])) {
                    Some(c) => {
                        let mut buf = [0u16; 2];
                        out.extend_from_slice(c.encode_utf16(&mut buf));
                    }
                    None => out.extend(s.get(amp..next).unwrap_or("").encode_utf16()),
                }
                None
            }
            RefMatch::Hex(a, e) => Some(hex_value(b.get(a..e).unwrap_or(&[]))),
            RefMatch::Dec(a, e) => Some(dec_value(b.get(a..e).unwrap_or(&[]))),
        };
        match cp {
            Some(Some(cp)) => {
                if cp >= 0x1_0000 {
                    let v = cp - 0x1_0000;
                    out.push(0xD800 + u16::try_from(v >> 10).unwrap_or(0));
                    out.push(0xDC00 + u16::try_from(v & 0x3FF).unwrap_or(0));
                } else {
                    // a surrogate stays a lone unit (`String.fromCodePoint`)
                    out.push(u16::try_from(cp).unwrap_or(0xFFFD));
                }
            }
            Some(None) => {
                return Err(Error::Xml { offset: amp, kind: XmlErrorKind::InvalidCharRef, part: None });
            }
            None => {}
        }
        lit = next;
        i = next;
    }
    out.extend(s.get(lit..).unwrap_or("").encode_utf16());
    Ok(())
}

/// Text content as the TS decodes it (`decodeEntities(normalizeEol(text))`), appended to `out` as UTF-16.
/// A numeric reference past U+10FFFF is `Error::Xml` (`InvalidCharRef`), as the TS throws.
pub fn decode_text(raw: &[u8], out: &mut Vec<u16>) -> Result<()> {
    decode_units(raw, Norm2::Eol, true, out)
}

/// A CDATA section: only the end-of-line normalization, no references.
pub fn decode_cdata(raw: &[u8], out: &mut Vec<u16>) -> Result<()> {
    decode_units(raw, Norm2::Eol, false, out)
}

/// An attribute value (`decodeEntities(normalizeAttr(value))`).
pub fn decode_attr(raw: &[u8], out: &mut Vec<u16>) -> Result<()> {
    decode_units(raw, Norm2::Attr, true, out)
}

/// Whether `raw` decodes to itself (no reference, no CR, and for an attribute no TAB/LF) and is UTF-8: then
/// its text is the slice, no decoding needed.
pub fn is_plain(raw: &[u8], attr: bool) -> Option<&str> {
    let dirty = raw.iter().any(|&c| c == b'&' || c == b'\r' || (attr && matches!(c, b'\n' | b'\t')));
    if dirty { None } else { std::str::from_utf8(raw).ok() }
}

/// The policy the presets use for references the TS cannot decode (kept for symmetry with [`super::read`]).
pub const BAD_CHAR_REF: CharRefPolicy = CharRefPolicy::Error;

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(doc: &[u8], piece: usize) -> Result<Vec<String>> {
        let mut p = Pull::new();
        let mut ev = Vec::new();
        let mut chunks: Vec<&[u8]> = doc.chunks(piece.max(1)).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        let n = chunks.len();
        for (k, c) in chunks.into_iter().enumerate() {
            let end = p.feed(c, k + 1 == n, |t| {
                ev.push(match t {
                    Token::Start(s) => format!(
                        "<{}{} {}",
                        String::from_utf8_lossy(s.name),
                        s.attrs()
                            .map(|(a, v)| format!(" {}={}", String::from_utf8_lossy(a), String::from_utf8_lossy(v)))
                            .collect::<String>(),
                        if s.self_closing { "/" } else { "" }
                    ),
                    Token::End { name, .. } => format!("</{}", String::from_utf8_lossy(name)),
                    Token::Text(t) => format!("T{}", String::from_utf8_lossy(t)),
                    Token::CData(t) => format!("C{}", String::from_utf8_lossy(t)),
                });
                Ok(Flow::Continue)
            })?;
            if matches!(end, FeedEnd::RootClosed(_)) {
                break;
            }
        }
        Ok(ev)
    }

    #[test]
    fn same_tokens_for_any_piece_size() {
        let doc = "\u{feff}<?xml version=\"1.0\"?>\n<!-- c --><x:a xmlns:x='u' k = \"v &amp; w\"><b/> t<![CDATA[<z>]]><!--x--><c\td=\"1\">é&#x41;</c ></x:a>tail".as_bytes();
        let whole = collect(doc, doc.len()).unwrap();
        assert_eq!(whole[0], "<x:a xmlns:x=u k=v &amp; w ");
        assert_eq!(whole.last().unwrap(), "</x:a");
        for piece in 1..9 {
            assert_eq!(collect(doc, piece).unwrap(), whole, "piece {piece}");
        }
    }

    #[test]
    fn errors_like_the_ts() {
        for bad in ["<a b>", "<a b=1>", "<a/ >", "<a>", "text", "<a><b></a>"] {
            assert!(collect(bad.as_bytes(), 3).is_err(), "{bad}");
        }
        // closing names are not checked
        assert!(collect(b"<a><b></z></q>", 100).is_ok());
    }

    #[test]
    fn decoding_keeps_lone_surrogates() {
        let mut o = Vec::new();
        decode_text(b"a&#xD800;b&#x1F600;&amp;&foo;\r\nc", &mut o).unwrap();
        let mut want = vec![0x61, 0xD800, 0x62, 0xD83D, 0xDE00, 0x26];
        want.extend(
            "&foo;
c"
            .encode_utf16(),
        );
        assert_eq!(o, want);
        assert!(decode_text(b"&#x110000;", &mut Vec::new()).is_err());
        let mut o = Vec::new();
        decode_attr(b"a\tb\r\nc&#13;", &mut o).unwrap();
        assert_eq!(String::from_utf16(&o).unwrap(), "a b c\r");
    }
}
