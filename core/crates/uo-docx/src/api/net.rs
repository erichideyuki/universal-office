//! The .NET semantics the C# Editing API depends on (record `uo-docx-d3.md`, table U13), each
//! measured against .NET 10 by the harness (`tests/parity/d3/semantics.capture`):
//!
//! - strings are UTF-16 in .NET: positions, lengths and `char` tests are per UTF-16 unit
//!   ([`units`]); `char.IsLetterOrDigit`, `ToUpperInvariant` and the `OrdinalIgnoreCase` folding
//!   come from tables measured in the runtime (`net_tables.rs`, generated);
//! - `char.IsWhiteSpace` / `string.Trim` (the Unicode white space set, not JS's, not XML's);
//! - `int.TryParse` / `long.TryParse` with their number styles (ASCII digits, the .NET white
//!   set, trailing NULs);
//! - `Uri.UnescapeDataString` (UTF-8 runs decoded, invalid bytes kept escaped),
//!   `Uri.EscapeDataString`, `Uri.TryCreate(…, UriKind.Absolute)` with its scheme (a model of
//!   System.Uri for the schemes the API allows, see [`absolute_uri_scheme`]);
//! - `JsonDocument.Parse` validation (RFC 8259, depth 64), the custom `0.##` number format, and
//!   `Math.Round` (half to even) with the saturating double-to-integer conversion of .NET 9+.

use super::net_tables::{FOLD, LETTER_OR_DIGIT, UPPER};

/// The UTF-16 units of a string (a .NET `string`).
pub fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A string back from UTF-16 units that came from a valid string (cuts at pair boundaries).
pub fn from_units(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn in_ranges(table: &[(u16, u16)], u: u16) -> bool {
    match table.binary_search_by(|&(lo, _)| lo.cmp(&u)) {
        Ok(_) => true,
        Err(0) => false,
        Err(k) => table.get(k - 1).is_some_and(|&(_, hi)| u <= hi),
    }
}

fn mapped(table: &[(u16, u16)], u: u16) -> u16 {
    match table.binary_search_by(|&(from, _)| from.cmp(&u)) {
        Ok(k) => table.get(k).map_or(u, |&(_, to)| to),
        Err(_) => u,
    }
}

/// `char.IsLetterOrDigit` of one unit (a surrogate is never a letter).
pub fn is_letter_or_digit(u: u16) -> bool {
    in_ranges(&LETTER_OR_DIGIT, u)
}

/// `char.IsWhiteSpace` of one unit (checked against the runtime by the harness).
pub fn is_white_space(u: u16) -> bool {
    matches!(u, 0x09..=0x0D | 0x20 | 0x85 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000)
}

pub fn is_white_space_char(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_white_space)
}

/// .NET `\d` / `char.IsDigit` (Unicode Nd) of one unit: ASCII and the BMP decimal-digit blocks.
pub fn is_decimal_digit_unit(u: u16) -> bool {
    const ZEROS: &[u16] = &[
        0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66, 0x0DE6, 0x0E50, 0x0ED0,
        0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900,
        0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
    ];
    ZEROS.iter().any(|&z| (z..z + 10).contains(&u))
}

/// `char.IsControl` of one unit.
pub fn is_control(u: u16) -> bool {
    u < 0x20 || (0x7F..=0x9F).contains(&u)
}

/// `char.ToUpperInvariant` of one unit.
pub fn upper_unit(u: u16) -> u16 {
    mapped(&UPPER, u)
}

/// The `OrdinalIgnoreCase` representative of one unit.
pub fn fold_unit(u: u16) -> u16 {
    mapped(&FOLD, u)
}

/// `string.ToUpperInvariant()` (per BMP unit; supplementary characters are kept: accepted by
/// name, record "Accepted differences").
pub fn to_upper_invariant(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_uppercase();
    }
    let u: Vec<u16> = s.encode_utf16().map(upper_unit).collect();
    from_units(&u)
}

/// `string.Equals(a, b, StringComparison.OrdinalIgnoreCase)`.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    let mut x = a.encode_utf16();
    let mut y = b.encode_utf16();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if fold_unit(p) == fold_unit(q) => {}
            _ => return false,
        }
    }
}

/// The key of a string in an `OrdinalIgnoreCase` set or dictionary.
pub fn fold_key(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_uppercase();
    }
    let u: Vec<u16> = s.encode_utf16().map(fold_unit).collect();
    from_units(&u)
}

/// `s.EndsWith(suffix, OrdinalIgnoreCase)`.
pub fn ends_with_ignore_case(s: &str, suffix: &str) -> bool {
    let a = units(s);
    let b = units(suffix);
    a.len() >= b.len() && a.get(a.len() - b.len()..).is_some_and(|t| t.iter().zip(&b).all(|(&p, &q)| fold_unit(p) == fold_unit(q)))
}

/// `s.StartsWith(prefix, OrdinalIgnoreCase)`.
pub fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    let a = units(s);
    let b = units(prefix);
    a.len() >= b.len() && a.iter().zip(&b).all(|(&p, &q)| fold_unit(p) == fold_unit(q))
}

/// `s.Contains(needle, OrdinalIgnoreCase)`.
pub fn contains_ignore_case(s: &str, needle: &str) -> bool {
    index_of(&units(s), &units(needle), 0, true).is_some()
}

/// `hay.IndexOf(needle, from, Ordinal | OrdinalIgnoreCase)` over UTF-16 units.
pub fn index_of(hay: &[u16], needle: &[u16], from: usize, ignore_case: bool) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = from;
    while i <= last {
        let window = hay.get(i..i + needle.len())?;
        let hit = if ignore_case { window.iter().zip(needle).all(|(&p, &q)| fold_unit(p) == fold_unit(q)) } else { window == needle };
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `string.Trim()` (the `char.IsWhiteSpace` set, all BMP).
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_white_space_char)
}

/// `string.TrimStart()`.
pub fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_white_space_char)
}

/// `string.IsNullOrWhiteSpace` of a present string.
pub fn is_white_space_only(s: &str) -> bool {
    s.chars().all(is_white_space_char)
}

// ─────────────────────────────── numbers ───────────────────────────────

/// The white space of .NET number parsing (`IsWhite`: U+0009–U+000D and U+0020).
fn is_number_white(c: u8) -> bool {
    c == b' ' || (0x09..=0x0D).contains(&c)
}

/// The digits and sign of an integer in `NumberStyles.Integer` (or `None` with
/// `allow_white_sign = false`), trailing NULs allowed as .NET does; `None` when it does not
/// parse (overflow included).
fn parse_integer(s: &str, allow_white_sign: bool, min: i128, max: i128) -> Option<i128> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut end = b.len();
    // trailing NULs are ignored by .NET's number parser
    while end > 0 && b.get(end - 1) == Some(&0) {
        end -= 1;
    }
    if allow_white_sign {
        while i < end && b.get(i).is_some_and(|&c| is_number_white(c)) {
            i += 1;
        }
    }
    let mut negative = false;
    if allow_white_sign && i < end {
        match b.get(i) {
            Some(b'-') => {
                negative = true;
                i += 1;
            }
            Some(b'+') => i += 1,
            _ => {}
        }
    }
    let start = i;
    let mut value: i128 = 0;
    while i < end && b.get(i).is_some_and(u8::is_ascii_digit) {
        value = value * 10 + i128::from(b.get(i).copied().unwrap_or(b'0') - b'0');
        if value > max + 1 {
            // keep scanning digits without growing past the range
            value = max + 2;
        }
        i += 1;
    }
    if i == start {
        return None;
    }
    if allow_white_sign {
        while i < end && b.get(i).is_some_and(|&c| is_number_white(c)) {
            i += 1;
        }
    }
    if i != end {
        return None;
    }
    let v = if negative { -value } else { value };
    (min..=max).contains(&v).then_some(v)
}

/// `int.TryParse(s, out v)` and `int.TryParse(s, NumberStyles.Integer, InvariantCulture, …)`.
pub fn parse_int(s: &str) -> Option<i32> {
    parse_integer(s, true, i128::from(i32::MIN), i128::from(i32::MAX)).and_then(|v| i32::try_from(v).ok())
}

/// `int.TryParse(s, NumberStyles.None, InvariantCulture, …)`.
pub fn parse_int_digits(s: &str) -> Option<i32> {
    parse_integer(s, false, i128::from(i32::MIN), i128::from(i32::MAX)).and_then(|v| i32::try_from(v).ok())
}

/// `long.TryParse(s, out v)`.
pub fn parse_long(s: &str) -> Option<i64> {
    parse_integer(s, true, i128::from(i64::MIN), i128::from(i64::MAX)).and_then(|v| i64::try_from(v).ok())
}

/// `Math.Round(x)` (MidpointRounding.ToEven).
#[expect(clippy::disallowed_methods, reason = ".NET Math.Round(double) rounds half to even (C#, not JS)")]
pub fn round_half_even(x: f64) -> f64 {
    x.round_ties_even()
}

/// `(long)x` on .NET 9+: saturating, NaN to 0 (the Rust `as` semantics, named).
#[allow(clippy::cast_possible_truncation)]
pub fn to_i64_saturating(x: f64) -> i64 {
    x as i64
}

/// `(long)x` of a `float` (C# `(long)(maxWidthPt * EmuPerPoint)` is float arithmetic).
#[allow(clippy::cast_possible_truncation)]
pub fn f32_to_i64_saturating(x: f32) -> i64 {
    x as i64
}

/// `x.ToString("0.##", InvariantCulture)`: the double to 15 significant digits, then rounded
/// half away from zero to two decimals, trailing zeros dropped ("-0" for a negative that
/// rounds to zero is "-0" in .NET Core 3.0+).
pub fn format_0_00(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".to_owned() } else { "-∞".to_owned() };
    }
    // 15 significant digits as the decimal digit string and its exponent
    let Ok(sci) = uo_jsnum::format::to_exponential(x.abs(), 14) else { return "0".to_owned() };
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.trim_start_matches('+').parse().unwrap_or(0);
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).map(|d| d - b'0').collect();
    if digits.iter().all(|&d| d == 0) {
        return if x.is_sign_negative() { "-0".to_owned() } else { "0".to_owned() };
    }
    // value = 0.d1d2…d15 × 10^(exp+1); integer part has exp+1 digits
    let point = exp + 1;
    // digits kept: those up to 2 decimals
    let keep = point + 2;
    let mut kept: Vec<u8> = Vec::new();
    let round_up = if keep <= 0 {
        keep == 0 && digits.first().is_some_and(|&d| d >= 5)
    } else {
        let k = usize::try_from(keep).unwrap_or(0);
        for i in 0..k {
            kept.push(digits.get(i).copied().unwrap_or(0));
        }
        digits.get(k).is_some_and(|&d| d >= 5)
    };
    // the kept digits represent value × 100 as an integer
    let mut n: Vec<u8> = kept;
    if round_up {
        let mut i = n.len();
        loop {
            if i == 0 {
                n.insert(0, 1);
                break;
            }
            i -= 1;
            if let Some(d) = n.get_mut(i) {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    break;
                }
            }
        }
    }
    while n.len() < 3 {
        n.insert(0, 0);
    }
    let split = n.len() - 2;
    let int_part: String = {
        let s: String = n.get(..split).unwrap_or(&[]).iter().map(|d| char::from(b'0' + d)).collect();
        let t = s.trim_start_matches('0');
        if t.is_empty() { "0".to_owned() } else { t.to_owned() }
    };
    let frac: String = n.get(split..).unwrap_or(&[]).iter().map(|d| char::from(b'0' + d)).collect();
    let frac = frac.trim_end_matches('0');
    // .NET Core 3.0+ keeps the sign of a negative that rounds to zero ("-0")
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    out.push_str(&int_part);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

// ─────────────────────────────── URI escapes ───────────────────────────────

fn hex_val(c: u16) -> Option<u8> {
    match c {
        0x30..=0x39 => u8::try_from(c - 0x30).ok(),
        0x41..=0x46 => u8::try_from(c - 0x41 + 10).ok(),
        0x61..=0x66 => u8::try_from(c - 0x61 + 10).ok(),
        _ => None,
    }
}

/// `Uri.IsHexDigit`.
pub fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// `Uri.UnescapeDataString`: `%XX` below 0x80 decoded; runs of `%XX` at or above 0x80 decoded
/// as UTF-8 where valid, the bytes of each invalid maximal subpart kept as written.
pub fn unescape_data_string(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let u = units(s);
    let mut out: Vec<u16> = Vec::with_capacity(u.len());
    let mut i = 0;
    let byte_at = |i: usize| -> Option<u8> {
        if u.get(i) != Some(&0x25) {
            return None;
        }
        let hi = hex_val(*u.get(i + 1)?)?;
        let lo = hex_val(*u.get(i + 2)?)?;
        Some(hi * 16 + lo)
    };
    while i < u.len() {
        match byte_at(i) {
            Some(b) if b < 0x80 => {
                out.push(u16::from(b));
                i += 3;
            }
            Some(_) => {
                // the run of escaped high bytes
                let mut bytes = Vec::new();
                let mut j = i;
                while let Some(b) = byte_at(j) {
                    if b < 0x80 {
                        break;
                    }
                    bytes.push(b);
                    j += 3;
                }
                let mut k = 0;
                while k < bytes.len() {
                    let rest = bytes.get(k..).unwrap_or(&[]);
                    match std::str::from_utf8(rest) {
                        Ok(text) => {
                            out.extend(text.encode_utf16());
                            k = bytes.len();
                        }
                        Err(e) => {
                            let good = e.valid_up_to();
                            if let Some(text) = rest.get(..good).and_then(|g| std::str::from_utf8(g).ok()) {
                                out.extend(text.encode_utf16());
                            }
                            k += good;
                            let bad = e.error_len().unwrap_or(rest.len() - good).max(1);
                            // the invalid bytes as written in the input
                            let from = i + k * 3;
                            if let Some(src) = u.get(from..from + bad * 3) {
                                out.extend_from_slice(src);
                            }
                            k += bad;
                        }
                    }
                }
                i = j;
            }
            None => {
                if let Some(&c) = u.get(i) {
                    out.push(c);
                }
                i += 1;
            }
        }
    }
    from_units(&out)
}

/// `Uri.EscapeDataString`: everything but the RFC 3986 unreserved characters as `%XX` of its
/// UTF-8 bytes, upper-case hex.
pub fn escape_data_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 15)]));
        }
    }
    out
}

// ─────────────────────────────── System.Uri ───────────────────────────────

fn is_lws(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0D)
}

/// What `Uri.TryCreate(s, UriKind.Absolute, out var u)` gives: `Some(u.Scheme)` (lower case)
/// when it succeeds. A model of System.Uri (.NET 10, Windows) for the schemes the Editing API
/// lets through (http, https, mailto) plus ftp and the generic parser; measured on the
/// `uri` site of the semantics capture, the remaining differences accepted by name (record
/// "Accepted differences": only schemes outside the defaults).
pub fn absolute_uri_scheme(s: &str) -> Option<String> {
    let u = units(s);
    let mut start = 0;
    let mut end = u.len();
    while start < end && u.get(start).is_some_and(|&c| is_lws(c)) {
        start += 1;
    }
    while end > start && u.get(end - 1).is_some_and(|&c| is_lws(c)) {
        end -= 1;
    }
    let t = u.get(start..end)?;
    if t.is_empty() {
        return None;
    }
    // implicit file paths (Windows): drive letter, UNC
    let at = |i: usize| t.get(i).copied().unwrap_or(0);
    let is_slash = |c: u16| c == 0x2F || c == 0x5C;
    if char::from_u32(u32::from(at(0))).is_some_and(|c| c.is_ascii_alphabetic()) && matches!(at(1), 0x3A | 0x7C) && is_slash(at(2)) {
        return Some("file".to_owned());
    }
    if is_slash(at(0)) && is_slash(at(1)) {
        return unc_ok(t).then(|| "file".to_owned());
    }
    // scheme
    let colon = t.iter().position(|&c| c == 0x3A)?;
    let scheme = t.get(..colon)?;
    if scheme.is_empty() || scheme.len() > 1024 {
        return None;
    }
    let first_ok = char::from_u32(u32::from(*scheme.first()?)).is_some_and(|c| c.is_ascii_alphabetic());
    if !first_ok
        || !scheme.iter().all(|&c| char::from_u32(u32::from(c)).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    {
        return None;
    }
    let lower: String = from_units(scheme).to_ascii_lowercase();
    let rest = t.get(colon + 1..)?;
    let ok = match lower.as_str() {
        "http" | "https" | "ws" | "wss" | "ftp" | "gopher" | "nntp" | "telnet" | "ldap" => authority_scheme(rest, lower == "ftp"),
        "mailto" => mailto(rest),
        "file" => file_scheme(rest),
        "news" => true,
        _ => generic(rest),
    };
    ok.then_some(lower)
}

/// `\\server\share` (or `//server`): a host must follow.
fn unc_ok(t: &[u16]) -> bool {
    let host: Vec<u16> = t.iter().skip(2).copied().take_while(|&c| c != 0x2F && c != 0x5C).collect();
    !host.is_empty() && host_ok(&host, false)
}

fn file_scheme(rest: &[u16]) -> bool {
    let _ = rest;
    true
}

/// A scheme whose authority is required (measured): two slashes, each `/` or `\`, then the
/// authority, which ends at `/`, `?` or `#` (a backslash there stays in the host, which fails).
fn authority_scheme(rest: &[u16], _ftp: bool) -> bool {
    let slash = |c: Option<&u16>| matches!(c, Some(&0x2F | &0x5C));
    if !(slash(rest.first()) && slash(rest.get(1))) {
        return false;
    }
    let auth = rest.get(2..).unwrap_or(&[]);
    let end = auth.iter().position(|&c| matches!(c, 0x2F | 0x3F | 0x23)).unwrap_or(auth.len());
    let authority = auth.get(..end).unwrap_or(&[]);
    authority_ok(authority, false)
}

/// userinfo (up to the first `@`) `@` host `:` port.
fn authority_ok(authority: &[u16], allow_empty: bool) -> bool {
    let host_port = match authority.iter().position(|&c| c == 0x40) {
        // a backslash is not allowed in the user part (measured)
        Some(k) if authority.get(..k).is_some_and(|u| u.contains(&0x5C)) => return false,
        Some(k) => authority.get(k + 1..).unwrap_or(&[]),
        None => authority,
    };
    let (host, port) = split_port(host_port);
    if host.is_empty() {
        return allow_empty && port.is_none();
    }
    if let Some(p) = port
        && !port_ok(p)
    {
        return false;
    }
    host_ok(host, true)
}

fn split_port(hp: &[u16]) -> (&[u16], Option<&[u16]>) {
    if hp.first() == Some(&0x5B) {
        // IPv6 literal: the port follows the bracket
        if let Some(close) = hp.iter().position(|&c| c == 0x5D) {
            let host = hp.get(..=close).unwrap_or(&[]);
            return match hp.get(close + 1) {
                Some(&0x3A) => (host, Some(hp.get(close + 2..).unwrap_or(&[]))),
                None => (host, None),
                Some(_) => (&[], None),
            };
        }
        return (&[], None);
    }
    match hp.iter().position(|&c| c == 0x3A) {
        Some(k) => (hp.get(..k).unwrap_or(&[]), Some(hp.get(k + 1..).unwrap_or(&[]))),
        None => (hp, None),
    }
}

fn port_ok(p: &[u16]) -> bool {
    if p.is_empty() {
        return true;
    }
    if !p.iter().all(|&c| (0x30..=0x39).contains(&c)) {
        return false;
    }
    let mut v: u32 = 0;
    for &c in p {
        v = v * 10 + u32::from(c - 0x30);
        if v > 65535 {
            return false;
        }
    }
    true
}

/// A host System.Uri accepts for an "internet host" scheme (measured): an IPv6 literal, or
/// dot-separated non-empty labels of ASCII letters, digits, `-`, `_` and any non-ASCII
/// character (the IDN path takes even U+200B, U+FEFF, U+3000), an optional final dot, no
/// length limits (a 64-character label and a 259-character name pass).
fn host_ok(host: &[u16], _idn: bool) -> bool {
    if host.first() == Some(&0x5B) {
        return host.last() == Some(&0x5D) && ipv6_ok(host.get(1..host.len() - 1).unwrap_or(&[]));
    }
    let s = from_units(host);
    let trimmed = s.strip_suffix('.').unwrap_or(&s);
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .split('.')
        .all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()))
}

fn ipv6_ok(inner: &[u16]) -> bool {
    let s = from_units(inner);
    let (addr, zone) = match s.split_once('%') {
        Some((a, z)) => (a, Some(z)),
        None => (s.as_str(), None),
    };
    if zone.is_some_and(str::is_empty) {
        return false;
    }
    let groups: Vec<&str> = addr.split(':').collect();
    let double = addr.matches("::").count();
    if double > 1 || addr.contains(":::") {
        return false;
    }
    let mut count = 0;
    for (k, g) in groups.iter().enumerate() {
        if g.is_empty() {
            continue;
        }
        if k == groups.len() - 1 && g.contains('.') {
            let parts: Vec<&str> = g.split('.').collect();
            if parts.len() != 4
                || !parts.iter().all(|p| {
                    !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|v| v <= 255)
                })
            {
                return false;
            }
            count += 2;
            continue;
        }
        if g.len() > 4 || !g.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        count += 1;
    }
    if double == 1 { count < 8 } else { count == 8 }
}

/// mailto (measured): a leading `/` makes the rest a path (valid); otherwise the address up to
/// `/`, `?` or `#` is `user@host` (the first `@`; the user part is free) or a bare host; empty
/// is valid, an empty host after `@` is not, a port must be at most 65535.
fn mailto(rest: &[u16]) -> bool {
    if rest.first() == Some(&0x2F) {
        return true;
    }
    let end = rest.iter().position(|&c| matches!(c, 0x2F | 0x3F | 0x23)).unwrap_or(rest.len());
    let addr = rest.get(..end).unwrap_or(&[]);
    if addr.is_empty() {
        return true;
    }
    let host = match addr.iter().position(|&c| c == 0x40) {
        Some(k) if addr.get(..k).is_some_and(|u| u.contains(&0x5C)) => return false,
        Some(k) => addr.get(k + 1..).unwrap_or(&[]),
        None => addr,
    };
    if host.is_empty() {
        return false;
    }
    let (h, port) = split_port(host);
    if let Some(p) = port
        && !port_ok(p)
    {
        return false;
    }
    !h.is_empty() && host_ok(h, true)
}

/// Schemes without a registered parser: `//` introduces an authority that must parse.
fn generic(rest: &[u16]) -> bool {
    if rest.first() == Some(&0x2F) && rest.get(1) == Some(&0x2F) {
        let auth = rest.get(2..).unwrap_or(&[]);
        let end = auth.iter().position(|&c| matches!(c, 0x2F | 0x5C | 0x3F | 0x23)).unwrap_or(auth.len());
        return authority_ok(auth.get(..end).unwrap_or(&[]), true);
    }
    true
}

// ─────────────────────────────── JSON ───────────────────────────────

/// `JsonDocument.Parse(s)` succeeds (default options: RFC 8259, no comments, no trailing
/// commas, depth 64, one value).
pub fn json_valid(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let ws = |b: &[u8], i: &mut usize| {
        while matches!(b.get(*i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            *i += 1;
        }
    };
    // stack of containers: true = object
    let mut stack: Vec<bool> = Vec::new();
    ws(b, &mut i);
    loop {
        // a value
        match b.get(i) {
            Some(b'{') => {
                if stack.len() >= 64 {
                    return false;
                }
                stack.push(true);
                i += 1;
                ws(b, &mut i);
                if b.get(i) == Some(&b'}') {
                    stack.pop();
                    i += 1;
                } else {
                    if !json_string(b, &mut i) {
                        return false;
                    }
                    ws(b, &mut i);
                    if b.get(i) != Some(&b':') {
                        return false;
                    }
                    i += 1;
                    ws(b, &mut i);
                    continue;
                }
            }
            Some(b'[') => {
                if stack.len() >= 64 {
                    return false;
                }
                stack.push(false);
                i += 1;
                ws(b, &mut i);
                if b.get(i) == Some(&b']') {
                    stack.pop();
                    i += 1;
                } else {
                    continue;
                }
            }
            Some(b'"') => {
                if !json_string(b, &mut i) {
                    return false;
                }
            }
            Some(b't') => {
                if b.get(i..i + 4) != Some(b"true") {
                    return false;
                }
                i += 4;
            }
            Some(b'f') => {
                if b.get(i..i + 5) != Some(b"false") {
                    return false;
                }
                i += 5;
            }
            Some(b'n') => {
                if b.get(i..i + 4) != Some(b"null") {
                    return false;
                }
                i += 4;
            }
            Some(b'-' | b'0'..=b'9') => {
                if !json_number(b, &mut i) {
                    return false;
                }
            }
            _ => return false,
        }
        // after a value: close containers or continue with a separator
        loop {
            ws(b, &mut i);
            let Some(&top) = stack.last() else {
                return i == b.len();
            };
            match b.get(i) {
                Some(b',') => {
                    i += 1;
                    ws(b, &mut i);
                    if top {
                        if !json_string(b, &mut i) {
                            return false;
                        }
                        ws(b, &mut i);
                        if b.get(i) != Some(&b':') {
                            return false;
                        }
                        i += 1;
                        ws(b, &mut i);
                    }
                    break;
                }
                Some(b'}') if top => {
                    stack.pop();
                    i += 1;
                }
                Some(b']') if !top => {
                    stack.pop();
                    i += 1;
                }
                _ => return false,
            }
        }
    }
}

fn json_string(b: &[u8], i: &mut usize) -> bool {
    if b.get(*i) != Some(&b'"') {
        return false;
    }
    *i += 1;
    loop {
        match b.get(*i) {
            None => return false,
            Some(b'"') => {
                *i += 1;
                return true;
            }
            Some(b'\\') => match b.get(*i + 1) {
                Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => *i += 2,
                Some(b'u') => {
                    if !b.get(*i + 2..*i + 6).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                        return false;
                    }
                    *i += 6;
                }
                _ => return false,
            },
            Some(&c) if c < 0x20 => return false,
            Some(_) => *i += 1,
        }
    }
}

fn json_number(b: &[u8], i: &mut usize) -> bool {
    if b.get(*i) == Some(&b'-') {
        *i += 1;
    }
    match b.get(*i) {
        Some(b'0') => *i += 1,
        Some(b'1'..=b'9') => {
            while b.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
        }
        _ => return false,
    }
    if b.get(*i) == Some(&b'.') {
        *i += 1;
        if !b.get(*i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
    }
    if matches!(b.get(*i), Some(b'e' | b'E')) {
        *i += 1;
        if matches!(b.get(*i), Some(b'+' | b'-')) {
            *i += 1;
        }
        if !b.get(*i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
    }
    // a number must end at a delimiter
    matches!(b.get(*i), None | Some(b' ' | b'\t' | b'\n' | b'\r' | b',' | b']' | b'}'))
}
