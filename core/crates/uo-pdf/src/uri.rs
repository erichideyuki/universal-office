//! `safePdfUri`: the addresses a link may carry.

use std::fmt::Write as _;

use uo_jsnum::text::trim;

/// The address for the `/URI` string: only `http:`, `https:` and `mailto:` (any case), written as
/// UTF-8 with the bytes at or below 0x20, at or above 0x7F, `"`, `<` and `>` as `%XX` (upper case)
/// and `\` before `(`, `)` and `\`. `None` for another scheme, an empty address or `None`.
///
/// A Rust `&str` has no lone surrogates, so the `TextEncoder` rule that turns them into U+FFFD
/// (`EF BF BD`) is the caller's conversion; trimming uses the JS white space set.
pub fn safe_pdf_uri(uri: Option<&str>) -> Option<String> {
    let u = trim(uri?);
    if u.is_empty() || !has_allowed_scheme(u) {
        return None;
    }
    let mut out = String::with_capacity(u.len());
    for &b in u.as_bytes() {
        if b <= 0x20 || b >= 0x7f || b == 0x22 || b == 0x3c || b == 0x3e {
            // writing into a String cannot fail
            let _ = write!(out, "%{b:02X}");
        } else if b == 0x28 || b == 0x29 || b == 0x5c {
            out.push('\\');
            out.push(char::from(b));
        } else {
            out.push(char::from(b));
        }
    }
    Some(out)
}

/// `/^(https?:\/\/|mailto:)/i`: ASCII case-insensitive (the JS `i` flag maps no non-ASCII letter
/// to an ASCII one).
fn has_allowed_scheme(u: &str) -> bool {
    let b = u.as_bytes();
    let starts = |prefix: &[u8]| b.len() >= prefix.len() && b.iter().zip(prefix).all(|(x, p)| x.eq_ignore_ascii_case(p));
    starts(b"http://") || starts(b"https://") || starts(b"mailto:")
}
