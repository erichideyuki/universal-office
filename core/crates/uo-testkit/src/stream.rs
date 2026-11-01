//! Hashed canonical streams: both engines write the same sequence of typed items for the value
//! under comparison and only the SHA-256 of the sequence is kept. A capture stays small however
//! large the data (no giant JSON, no V8 string limits), and equal digests mean equal sequences.
//!
//! Encoding (little-endian, identical in `js/testkit.mjs`):
//!
//! | item | bytes |
//! |---|---|
//! | `tag(c)` | `c` |
//! | `u32(v)` | 4 bytes |
//! | `u64(v)` | 8 bytes |
//! | `i64(v)` | 8 bytes, two's complement |
//! | `f64(v)` | the 8 bytes of the IEEE-754 value (so `-0`, NaN payloads and every bit count) |
//! | `bool(v)` | 1 byte, 0 or 1 |
//! | `str(s)` | `u32` byte length, then the UTF-8 bytes |
//! | `bytes(b)` | `u64` length, then the bytes |
//! | `digest_of(b)` | `u64` length, then the 32 bytes of SHA-256(`b`) |
//!
//! A stream can also keep a readable trace of its items (one per line, same text on both sides)
//! to find the first diverging item when two digests differ.

use std::fmt::Write as _;

use crate::sha256::{Sha256, sha256};

/// A SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// Lowercase hex of all 32 bytes.
    pub fn hex(&self) -> String {
        hex(&self.0)
    }

    /// Lowercase hex of the first 16 bytes: the form kept in capture files.
    pub fn short(&self) -> String {
        hex(&self.0[..16])
    }

    /// The digest of `data`.
    pub fn of(data: &[u8]) -> Digest {
        Digest(sha256(data))
    }
}

impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.short())
    }
}

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// A canonical stream being hashed. See the module comment for the encoding.
#[derive(Debug, Default, Clone)]
pub struct Stream {
    hasher: Sha256,
    buf: Vec<u8>,
    items: u64,
    trace: Option<String>,
}

const FLUSH: usize = 1 << 16;

impl Stream {
    pub fn new() -> Self {
        Stream::default()
    }

    /// A stream that also keeps a readable trace of its items.
    pub fn traced() -> Self {
        Stream { trace: Some(String::new()), ..Stream::default() }
    }

    /// Number of items written so far.
    pub fn items(&self) -> u64 {
        self.items
    }

    /// The readable trace (empty unless built with [`Stream::traced`]).
    pub fn trace(&self) -> &str {
        self.trace.as_deref().unwrap_or("")
    }

    fn raw(&mut self, b: &[u8]) {
        if self.buf.len() + b.len() > FLUSH {
            self.hasher.update(&self.buf);
            self.buf.clear();
        }
        if b.len() > FLUSH {
            self.hasher.update(b);
        } else {
            self.buf.extend_from_slice(b);
        }
    }

    fn note(&mut self, line: std::fmt::Arguments<'_>) {
        self.items += 1;
        if let Some(t) = &mut self.trace {
            let _ = t.write_fmt(line);
            t.push('\n');
        }
    }

    pub fn tag(&mut self, c: u8) -> &mut Self {
        self.raw(&[c]);
        self.note(format_args!("tag {}", char::from(c)));
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.raw(&v.to_le_bytes());
        self.note(format_args!("u32 {v}"));
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.raw(&v.to_le_bytes());
        self.note(format_args!("u64 {v}"));
        self
    }

    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.raw(&v.to_le_bytes());
        self.note(format_args!("i64 {v}"));
        self
    }

    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.raw(&v.to_bits().to_le_bytes());
        self.note(format_args!("f64 {:016x}", v.to_bits()));
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.raw(&[u8::from(v)]);
        self.note(format_args!("bool {}", u8::from(v)));
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.raw(&(s.len() as u32).to_le_bytes());
        self.raw(s.as_bytes());
        if self.trace.is_some() {
            let shown = json_string(s);
            self.note(format_args!("str {shown}"));
        } else {
            self.items += 1;
        }
        self
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.raw(&(b.len() as u64).to_le_bytes());
        self.raw(b);
        if self.trace.is_some() {
            let shown = hex(b.get(..b.len().min(32)).unwrap_or(&[]));
            self.note(format_args!("bytes {} {shown}", b.len()));
        } else {
            self.items += 1;
        }
        self
    }

    /// The length and SHA-256 of `b` (large payloads such as part bytes).
    pub fn digest_of(&mut self, b: &[u8]) -> &mut Self {
        let d = sha256(b);
        self.raw(&(b.len() as u64).to_le_bytes());
        self.raw(&d);
        self.note(format_args!("digest {} {}", b.len(), hex(&d[..16])));
        self
    }

    /// The digest of everything written.
    pub fn finish(mut self) -> Digest {
        self.hasher.update(&self.buf);
        Digest(self.hasher.finish())
    }

    /// The digest of everything written so far, keeping the stream (and its trace) usable.
    pub fn digest(&self) -> Digest {
        let mut h = self.hasher.clone();
        h.update(&self.buf);
        Digest(h.finish())
    }
}

/// `JSON.stringify` of a string, as the JS trace writes it (control characters escaped as
/// `\uXXXX` except the short forms).
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
