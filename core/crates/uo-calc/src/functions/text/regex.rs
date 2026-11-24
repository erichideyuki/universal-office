//! `functions/regex.ts`: REGEXTEST, REGEXEXTRACT, REGEXREPLACE. The TS translates the PCRE2 pattern into the source
//! of a JS `RegExp` (flags `gu` + `i`/`m`/`s`) and runs V8; here the same translation (unit by unit, the same
//! output text) is compiled by `regress` (decision R1: ECMAScript semantics, `u` flag, over UTF-16) and run with a
//! step budget per call (decision A4d-1: the vendored `regress` counts steps; past [`STEP_BUDGET`] the call throws
//! the `RangeError` path, `#NUM!` in the engine — the TS never returns on catastrophic backtracking).
//!
//! The compiled patterns are cached per thread ([`CACHE_CAP`] patterns, emptied when full as the TS `cache`, patterns
//! longer than [`CACHE_MAX_UNITS`] not cached); [`compile_pcre`] is also what XLOOKUP/XMATCH mode 3 use (A4e).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::format::MAX_TEXT;
use super::js::{Buf, index_of, u};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::tdl::{num, txt};
use crate::functions::shared::{R, Stop, bodies, boolean, scalar, text};
use crate::jstext::with_u16;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

/// The bodies of `regex.ts`.
pub(super) const REGEX: &[(&str, FnBody)] = bodies![
    "REGEXTEST" => regextest,
    "REGEXEXTRACT" => regexextract,
    "REGEXREPLACE" => regexreplace,
];

/// Backtracking steps one function call may spend (all its searches together).
pub(crate) const STEP_BUDGET: u64 = 1 << 24;

/// Compiled patterns kept per thread.
const CACHE_CAP: usize = 256;

/// Patterns longer than this are compiled on every call instead of cached (memory bound; same results).
const CACHE_MAX_UNITS: usize = 1024;

/// A compiled PCRE pattern (`Compiled` of the TS).
pub(crate) struct Pcre {
    re: regress::Regex,
    /// Capturing groups the translation counted.
    groups: u32,
    /// Named groups (`Map` order; a repeated name keeps its first position and the last number).
    names: Vec<(Vec<u16>, u32)>,
}

/// Key: a flag unit (case-insensitive) and the pattern; looked up from a stack copy (no allocation on a hit).
type Cache = HashMap<Box<[u16]>, Option<Rc<Pcre>>>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// Empties the compiled-pattern cache of this thread.
pub(super) fn clear_caches() {
    CACHE.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            *c = HashMap::new();
        }
    });
}

/// Number of cached patterns (M5 tests).
#[must_use]
pub fn cache_len() -> usize {
    CACHE.with(|c| c.try_borrow().map_or(0, |c| c.len()))
}

/// The translation flags (`flags` of the TS).
#[derive(Clone, Copy, Default)]
struct Flags {
    i: bool,
    m: bool,
    s: bool,
    x: bool,
}

impl Flags {
    fn set(&mut self, f: u16) {
        match f {
            0x69 => self.i = true,
            0x6D => self.m = true,
            0x73 => self.s = true,
            0x78 => self.x = true,
            _ => {}
        }
    }
}

/// `\p{L}\p{N}_` (the `WORD` class text).
const WORD: &str = "\\p{L}\\p{N}_";

/// `POSIX[name]` (a name that is not a member, or a member of `Object.prototype`, makes the pattern invalid: the
/// TS throws, or builds a class whose `]` text breaks the `u` syntax).
fn posix(name: &[u16]) -> Option<&'static str> {
    const T: [(&str, &str); 14] = [
        ("alpha", "\\p{L}"),
        ("digit", "0-9"),
        ("alnum", "\\p{L}\\p{N}"),
        ("space", "\\s"),
        ("upper", "\\p{Lu}"),
        ("lower", "\\p{Ll}"),
        ("punct", "!-\\/:-@\\[-`{-~"),
        ("xdigit", "0-9A-Fa-f"),
        ("word", WORD),
        ("blank", " \\t"),
        ("cntrl", "\\x00-\\x1f\\x7f"),
        ("print", "\\x20-\\x7e\\u00a0-\\uffff"),
        ("graph", "\\x21-\\x7e\\u00a1-\\uffff"),
        ("ascii", "\\x00-\\x7f"),
    ];
    T.iter().find(|(n, _)| super::js::eq_str(name, n)).map(|&(_, c)| c)
}

/// `^$\.*+?()[]{}|/`.
fn is_syntax(c: u16) -> bool {
    matches!(c, 0x5E | 0x24 | 0x5C | 0x2E | 0x2A | 0x2B | 0x3F | 0x28 | 0x29 | 0x5B | 0x5D | 0x7B | 0x7D | 0x7C | 0x2F)
}

fn is_ascii_alnum(c: u16) -> bool {
    c < 0x80 && (c as u8).is_ascii_alphanumeric()
}

/// `\w` without the `u` flag.
fn is_word(c: u16) -> bool {
    c < 0x80 && ((c as u8).is_ascii_alphanumeric() || c == u16::from(b'_'))
}

fn is_flag(c: u16) -> bool {
    matches!(c, 0x69 | 0x6D | 0x73 | 0x78)
}

/// `hex(cp)`: `\u{…}` (lower-case hex). A value past U+10FFFF is a `SyntaxError` of the `u` flag wherever it
/// appears, so the translation fails at once.
fn push_hex(out: &mut Vec<u16>, cp: f64) -> Result<(), ()> {
    if !(0.0..=1_114_111.0).contains(&cp) {
        return Err(());
    }
    out.extend(u("\\u{"));
    out.extend(u(&uo_jsnum::format::to_radix_string(cp, 16).map_err(|_| ())?));
    out.push(u16::from(b'}'));
    Ok(())
}

fn push(out: &mut Vec<u16>, s: &str) {
    out.extend(s.encode_utf16());
}

/// A name `[A-Za-z_]\w*` at `i`: its length.
fn name_at(p: &[u16], i: usize) -> usize {
    match p.get(i) {
        Some(&c) if c < 0x80 && ((c as u8).is_ascii_alphabetic() || c == u16::from(b'_')) => {
            1 + p[i + 1..].iter().take_while(|&&c| is_word(c)).count()
        }
        _ => 0,
    }
}

/// The translation: the JS source, the group count and the names. `Err` = the TS `SyntaxError`.
struct Translated {
    src: Vec<u16>,
    groups: u32,
    names: Vec<(Vec<u16>, u32)>,
}

/// `translate(p, flags, modifiers)`.
#[allow(clippy::too_many_lines)]
fn translate(p: &[u16], flags: &mut Flags, modifiers: bool) -> Result<Translated, ()> {
    let c = |b: u8| u16::from(b);
    let n = p.len();
    let at = |k: usize| p.get(k).copied();
    let mut out: Vec<u16> = Vec::with_capacity(n + 16);
    let mut i = 0;
    let mut in_class = false;
    let mut groups: u32 = 0;
    let mut names: Vec<(Vec<u16>, u32)> = Vec::new();
    let mut stack: Vec<u32> = vec![0];
    let set_name = |names: &mut Vec<(Vec<u16>, u32)>, name: &[u16], g: u32| match names.iter_mut().find(|(k, _)| k[..] == *name) {
        Some(e) => e.1 = g,
        None => names.push((name.to_vec(), g)),
    };
    // flags at the start of the pattern: `/^\(\?([imsx]+)\)/`
    loop {
        let k = p.get(i + 2..).map_or(0, |r| r.iter().take_while(|&&x| is_flag(x)).count());
        if at(i) == Some(c(b'(')) && at(i + 1) == Some(c(b'?')) && k > 0 && at(i + 2 + k) == Some(c(b')')) {
            for &f in &p[i + 2..i + 2 + k] {
                flags.set(f);
            }
            i += k + 3;
        } else {
            break;
        }
    }
    while i < n {
        let ch = p[i];
        if flags.x && !in_class {
            if uo_jsnum::text::is_js_space_unit(ch) {
                i += 1;
                continue;
            }
            if ch == c(b'#') {
                while i < n && p[i] != c(b'\n') {
                    i += 1;
                }
                continue;
            }
        }
        if ch == c(b'\\') {
            let Some(nx) = at(i + 1) else { return Err(()) };
            if nx == c(b'Q') {
                let end = index_of(p, &u("\\E"), i + 2);
                let lit = &p[i + 2..end.unwrap_or(n)];
                for (cp, a, w) in super::js::chars(lit) {
                    if w == 1 && is_ascii_alnum(lit[a]) {
                        out.push(lit[a]);
                    } else {
                        push_hex(&mut out, f64::from(cp))?;
                    }
                }
                i = end.map_or(n, |e| e + 2);
                continue;
            }
            if nx == c(b'E') {
                i += 2;
                continue;
            }
            if !in_class {
                let simple = match nx {
                    0x41 => Some("(?<![\\s\\S])".to_owned()),
                    0x7A => Some("(?![\\s\\S])".to_owned()),
                    0x5A => Some("(?=\\n?(?![\\s\\S]))".to_owned()),
                    0x47 => Some(String::new()),
                    0x62 => Some(format!("(?:(?<=[{WORD}])(?![{WORD}])|(?<![{WORD}])(?=[{WORD}]))")),
                    0x42 => Some(format!("(?:(?<=[{WORD}])(?=[{WORD}])|(?<![{WORD}])(?![{WORD}]))")),
                    0x52 => Some("(?:\\r\\n|[\\n\\v\\f\\r\\u0085\\u2028\\u2029])".to_owned()),
                    0x58 => Some("(?:\\P{M}\\p{M}*)".to_owned()),
                    _ => None,
                };
                if let Some(s) = simple {
                    push(&mut out, &s);
                    i += 2;
                    continue;
                }
                if nx == c(b'k') {
                    // `/^\\k(?:<([^>]+)>|'([^']+)'|\{([^}]+)\})/`
                    let (open, close) = match at(i + 2) {
                        Some(0x3C) => (c(b'<'), c(b'>')),
                        Some(0x27) => (c(b'\''), c(b'\'')),
                        Some(0x7B) => (c(b'{'), c(b'}')),
                        _ => return Err(()),
                    };
                    let _ = open;
                    let Some(end) = index_of(p, &[close], i + 3) else { return Err(()) };
                    if end == i + 3 {
                        return Err(());
                    }
                    push(&mut out, "\\k<");
                    out.extend_from_slice(&p[i + 3..end]);
                    out.push(c(b'>'));
                    i = end + 1;
                    continue;
                }
                if nx == c(b'g') {
                    // `/^\\g(?:\{(-?\d+)\}|(-?\d+)|\{([A-Za-z_]\w*)\})/`
                    let digits = |k: usize| -> Option<(usize, bool, usize)> {
                        let neg = at(k) == Some(c(b'-'));
                        let d0 = k + usize::from(neg);
                        let nd = crate::jstext::digit_run(p, d0);
                        (nd > 0).then_some((d0, neg, nd))
                    };
                    let mut done = None;
                    if at(i + 2) == Some(c(b'{')) {
                        if let Some((d0, neg, nd)) = digits(i + 3)
                            && at(d0 + nd) == Some(c(b'}'))
                        {
                            done = Some((Some((d0, neg, nd)), None, d0 + nd + 1));
                        } else {
                            let ln = name_at(p, i + 3);
                            if ln > 0 && at(i + 3 + ln) == Some(c(b'}')) {
                                done = Some((None, Some(i + 3..i + 3 + ln), i + 4 + ln));
                            }
                        }
                    } else if let Some((d0, neg, nd)) = digits(i + 2) {
                        done = Some((Some((d0, neg, nd)), None, d0 + nd));
                    }
                    let Some((num_part, name, next)) = done else { return Err(()) };
                    if let Some(r) = name {
                        push(&mut out, "\\k<");
                        out.extend_from_slice(&p[r]);
                        out.push(c(b'>'));
                    } else if let Some((d0, neg, nd)) = num_part {
                        let mut k = crate::jstext::digits_value(&p[d0..d0 + nd]);
                        if neg {
                            k = -k;
                        }
                        // `if (k < 0)`: `-0` stays (and prints "0")
                        if k < 0.0 {
                            k += f64::from(groups) + 1.0;
                        }
                        out.push(c(b'\\'));
                        out.extend(u(&uo_jsnum::format::to_js_string(k)));
                    }
                    i = next;
                    continue;
                }
            }
            let word_text = || if in_class { WORD.to_owned() } else { format!("[{WORD}]") };
            let class = |s: &str| if in_class { s.to_owned() } else { format!("[{s}]") };
            let h = "\\t \\u00a0\\u1680\\u180e\\u2000-\\u200a\\u202f\\u205f\\u3000";
            let vv = "\\n\\v\\f\\r\\u0085\\u2028\\u2029";
            let simple = match nx {
                0x77 => Some(word_text()),
                0x57 => Some(if in_class { "\\W".to_owned() } else { format!("[^{WORD}]") }),
                0x64 => Some("\\p{Nd}".to_owned()),
                0x44 => Some("\\P{Nd}".to_owned()),
                0x68 => Some(class(h)),
                0x48 => Some(if in_class { String::new() } else { format!("[^{h}]") }),
                0x76 => Some(class(vv)),
                0x65 => Some("\\x1b".to_owned()),
                0x61 => Some("\\x07".to_owned()),
                _ => None,
            };
            if let Some(s) = simple {
                push(&mut out, &s);
                i += 2;
                continue;
            }
            if nx == c(b'x') {
                // `/^\\x(?:\{([0-9A-Fa-f]+)\}|([0-9A-Fa-f]{1,2}))/`
                let hexd =
                    |k: usize| p.get(k..).map_or(0, |r| r.iter().take_while(|&&x| x < 0x80 && (x as u8).is_ascii_hexdigit()).count());
                let (digits, next) = if at(i + 2) == Some(c(b'{')) && hexd(i + 3) > 0 && at(i + 3 + hexd(i + 3)) == Some(c(b'}')) {
                    (&p[i + 3..i + 3 + hexd(i + 3)], i + 4 + hexd(i + 3))
                } else {
                    let k = hexd(i + 2).min(2);
                    if k == 0 {
                        return Err(());
                    }
                    (&p[i + 2..i + 2 + k], i + 2 + k)
                };
                let s: String = digits.iter().map(|&x| char::from(x as u8)).collect();
                push_hex(&mut out, uo_jsnum::parse::parse_int(&s, 16))?;
                i = next;
                continue;
            }
            if nx == c(b'p') || nx == c(b'P') {
                if at(i + 2) == Some(c(b'{')) {
                    // the TS loops for ever on a `\p{` without `}` (`i = -1 + 1`) and ends in a `RangeError` it
                    // catches: an invalid pattern (A4d-3)
                    let Some(end) = index_of(p, &[c(b'}')], i) else { return Err(()) };
                    let seg = &p[i..=end];
                    let mut removed = false;
                    for &x in seg {
                        if x == c(b'^') && !removed {
                            removed = true;
                            continue;
                        }
                        out.push(x);
                    }
                    i = end + 1;
                    continue;
                }
                let Some(x) = at(i + 2) else { return Err(()) };
                out.extend([c(b'\\'), nx, c(b'{'), x, c(b'}')]);
                i += 3;
                continue;
            }
            if (c(b'0')..=c(b'7')).contains(&nx) {
                // `/^\\([0-7]{3}|0[0-7]{0,2})/`
                let oct = |k: usize| p.get(k..).map_or(0, |r| r.iter().take_while(|&&x| (c(b'0')..=c(b'7')).contains(&x)).count());
                let run = oct(i + 1);
                let take = if run >= 3 {
                    3
                } else if nx == c(b'0') {
                    run
                } else {
                    0
                };
                if take > 0 {
                    let v = p[i + 1..i + 1 + take].iter().fold(0u32, |a, &x| a * 8 + u32::from(x - c(b'0')));
                    push_hex(&mut out, f64::from(v))?;
                    i += 1 + take;
                    continue;
                }
            }
            if (c(b'1')..=c(b'9')).contains(&nx) {
                let nd = crate::jstext::digit_run(p, i + 1);
                out.push(c(b'\\'));
                out.extend_from_slice(&p[i + 1..i + 1 + nd]);
                i += 1 + nd;
                continue;
            }
            if is_ascii_alnum(nx) {
                out.extend([c(b'\\'), nx]);
                i += 2;
                continue;
            }
            // a punctuation escape: the `u` flag only takes the syntax characters
            let (cp, w) = uo_text::utf16::decode_at(p, i + 1).unwrap_or((u32::from(nx), 1));
            if is_syntax(nx) || (in_class && nx == c(b'-')) {
                out.extend([c(b'\\'), nx]);
            } else {
                push_hex(&mut out, f64::from(cp))?;
            }
            i += 1 + w;
            continue;
        }
        if in_class {
            if ch == c(b'[') && at(i + 1) == Some(c(b':')) {
                let end = index_of(p, &u(":]"), i + 2);
                let name: &[u16] = end.map_or(&[], |e| &p[i + 2..e]);
                if name.first() == Some(&c(b'^')) {
                    return Err(());
                }
                let Some(cls) = posix(name) else { return Err(()) };
                push(&mut out, cls);
                i = end.map_or(n, |e| e + 2);
                continue;
            }
            if ch == c(b'[') {
                push(&mut out, "\\[");
            } else {
                if ch == c(b']') {
                    in_class = false;
                }
                out.push(ch);
            }
            i += 1;
            continue;
        }
        if ch == c(b'[') {
            in_class = true;
            out.push(ch);
            i += 1;
            if at(i) == Some(c(b'^')) {
                out.push(c(b'^'));
                i += 1;
            }
            if at(i) == Some(c(b']')) {
                push(&mut out, "\\]");
                i += 1;
            }
            continue;
        }
        if ch == c(b'(') {
            if at(i + 1) == Some(c(b'?')) {
                let rest = &p[i..];
                let r = |k: usize| rest.get(k).copied();
                // `(?#…)`
                if r(2) == Some(c(b'#')) {
                    if let Some(e) = index_of(rest, &[c(b')')], 3) {
                        i += e + 1;
                        continue;
                    }
                    return Err(());
                }
                // `(?P<name>` / `(?<name>` / `(?'name'`
                let named = {
                    let lt = if r(2) == Some(c(b'P')) && r(3) == Some(c(b'<')) {
                        Some(4)
                    } else if r(2) == Some(c(b'<')) {
                        Some(3)
                    } else {
                        None
                    };
                    match lt {
                        Some(k) => {
                            let ln = name_at(rest, k);
                            (ln > 0 && r(k + ln) == Some(c(b'>'))).then_some((k, ln, k + ln + 1))
                        }
                        None if r(2) == Some(c(b'\'')) => {
                            let ln = name_at(rest, 3);
                            (ln > 0 && r(3 + ln) == Some(c(b'\''))).then_some((3, ln, 4 + ln))
                        }
                        None => None,
                    }
                };
                if let Some((k, ln, len)) = named {
                    groups += 1;
                    set_name(&mut names, &rest[k..k + ln], groups);
                    push(&mut out, "(?<");
                    out.extend_from_slice(&rest[k..k + ln]);
                    out.push(c(b'>'));
                    i += len;
                    stack.push(0);
                    continue;
                }
                // `(?P=name)`
                if r(2) == Some(c(b'P')) && r(3) == Some(c(b'=')) {
                    let ln = name_at(rest, 4);
                    if ln > 0 && r(4 + ln) == Some(c(b')')) {
                        push(&mut out, "\\k<");
                        out.extend_from_slice(&rest[4..4 + ln]);
                        out.push(c(b'>'));
                        i += 5 + ln;
                        continue;
                    }
                }
                // `/^\(\?(P>|R\)|\d|&|\+|-\d|C|\(|\|)/`
                let r2 = r(2);
                let digit = |x: Option<u16>| x.is_some_and(crate::jstext::is_digit);
                let special = (r2 == Some(c(b'P')) && r(3) == Some(c(b'>')))
                    || (r2 == Some(c(b'R')) && r(3) == Some(c(b')')))
                    || digit(r2)
                    || r2 == Some(c(b'&'))
                    || r2 == Some(c(b'+'))
                    || (r2 == Some(c(b'-')) && digit(r(3)))
                    || r2 == Some(c(b'C'))
                    || r2 == Some(c(b'('))
                    || r2 == Some(c(b'|'));
                if special {
                    if r2 == Some(c(b'|')) {
                        push(&mut out, "(?:");
                        i += 3;
                        stack.push(0);
                        continue;
                    }
                    return Err(());
                }
                if r2 == Some(c(b'>')) {
                    push(&mut out, "(?:");
                    i += 3;
                    stack.push(0);
                    continue;
                }
                // `(?on-off)` / `(?on-off:`
                let on_len = rest.get(2..).map_or(0, |x| x.iter().take_while(|&&y| is_flag(y)).count());
                let mut k = 2 + on_len;
                let mut off: Option<std::ops::Range<usize>> = None;
                if r(k) == Some(c(b'-')) {
                    let ol = rest.get(k + 1..).map_or(0, |x| x.iter().take_while(|&&y| is_flag(y)).count());
                    off = Some(k + 1..k + 1 + ol);
                    k += 1 + ol;
                }
                let on_raw = &rest[2..2 + on_len];
                let strip_x = |s: &[u16]| s.iter().copied().filter(|&y| y != c(b'x')).collect::<Vec<u16>>();
                let on = strip_x(on_raw);
                let offv = off.clone().map_or_else(Vec::new, |r| strip_x(&rest[r]));
                let emit = |out: &mut Vec<u16>| {
                    push(out, "(?");
                    out.extend_from_slice(&on);
                    if !offv.is_empty() {
                        out.push(c(b'-'));
                        out.extend_from_slice(&offv);
                    }
                    out.push(c(b':'));
                };
                if r(k) == Some(c(b')')) && k > 2 {
                    if on_raw.contains(&c(b'x')) {
                        flags.x = true;
                    }
                    if !on.is_empty() || !offv.is_empty() {
                        if !modifiers {
                            for &f in &on {
                                flags.set(f);
                            }
                            i += k + 1;
                            continue;
                        }
                        emit(&mut out);
                        if let Some(top) = stack.last_mut() {
                            *top += 1;
                        }
                    }
                    i += k + 1;
                    continue;
                }
                if r(k) == Some(c(b':')) {
                    if !modifiers {
                        for &f in &on {
                            flags.set(f);
                        }
                        push(&mut out, "(?:");
                    } else if !on.is_empty() || !offv.is_empty() {
                        emit(&mut out);
                    } else {
                        push(&mut out, "(?:");
                    }
                    i += k + 1;
                    stack.push(0);
                    continue;
                }
                // lookarounds
                let look = if r2 == Some(c(b'=')) || r2 == Some(c(b'!')) {
                    Some(3)
                } else if r2 == Some(c(b'<')) && (r(3) == Some(c(b'=')) || r(3) == Some(c(b'!'))) {
                    Some(4)
                } else {
                    None
                };
                if let Some(len) = look {
                    out.extend_from_slice(&rest[..len]);
                    i += len;
                    stack.push(0);
                    continue;
                }
                return Err(());
            }
            groups += 1;
            out.push(ch);
            i += 1;
            stack.push(0);
            continue;
        }
        if ch == c(b')') {
            let extra = stack.pop().unwrap_or(0);
            if stack.is_empty() {
                return Err(());
            }
            for _ in 0..=extra {
                out.push(c(b')'));
            }
            i += 1;
            continue;
        }
        if ch == c(b'+') && i >= 1 && matches!(p[i - 1], 0x2A | 0x2B | 0x3F | 0x7D) && (i < 2 || p[i - 2] != c(b'\\')) {
            // possessive → greedy
            i += 1;
            continue;
        }
        if ch == c(b'$') && !flags.m {
            push(&mut out, "(?=\\n?(?![\\s\\S]))");
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    if in_class || stack.len() != 1 {
        return Err(());
    }
    for _ in 0..stack[0] {
        out.push(c(b')'));
    }
    Ok(Translated { src: out, groups, names })
}

/// `new RegExp(src, flags)` through `regress` (the source as code points: a lone surrogate is itself).
fn build(src: &[u16], f: Flags) -> Option<regress::Regex> {
    let flags = regress::Flags { icase: f.i, multiline: f.m, dot_all: f.s, unicode: true, ..regress::Flags::default() };
    let cps: Vec<u32> = super::js::chars(src).map(|(cp, _, _)| cp).collect();
    regress::Regex::from_unicode(cps.into_iter(), flags).ok()
}

/// `compile(pattern, ci)` without the cache.
fn compile_uncached(pattern: &[u16], ci: bool) -> Option<Pcre> {
    for modifiers in [true, false] {
        let mut flags = Flags { i: ci, ..Flags::default() };
        let Ok(t) = translate(pattern, &mut flags, modifiers) else { continue };
        if let Some(re) = build(&t.src, flags) {
            return Some(Pcre { re, groups: t.groups, names: t.names });
        }
    }
    None
}

/// `compilePcre(pattern, ci)` (also used by XLOOKUP/XMATCH match mode 3, frontier A4e): the compiled pattern, or
/// `None` when it is invalid. Cached per thread.
pub(crate) fn compile_pcre(pattern: &[u16], ci: bool) -> Option<Rc<Pcre>> {
    if pattern.len() > CACHE_MAX_UNITS {
        return compile_uncached(pattern, ci).map(Rc::new);
    }
    let mut buf = [0u16; CACHE_MAX_UNITS + 1];
    buf[0] = u16::from(ci);
    buf[1..=pattern.len()].copy_from_slice(pattern);
    let key = &buf[..=pattern.len()];
    if let Some(hit) = CACHE.with(|c| c.try_borrow().ok().and_then(|c| c.get(key).cloned())) {
        return hit;
    }
    let r = compile_uncached(pattern, ci).map(Rc::new);
    CACHE.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            if c.len() >= CACHE_CAP {
                c.clear();
            }
            c.insert(Box::from(key), r.clone());
        }
    });
    r
}

/// A match: the whole range and each group's (`None` = did not participate).
struct M {
    start: usize,
    end: usize,
    caps: Vec<Option<(usize, usize)>>,
}

/// Starts the step budget of one function call ([`STEP_BUDGET`]).
pub(crate) fn start_budget() {
    regress::budget::set_step_budget(STEP_BUDGET);
}

/// The budget check after the searches of a call: exhausted → the `RangeError` path (`#NUM!` in the engine).
pub(crate) fn check_budget() -> Result<(), Throw> {
    if regress::budget::budget_exhausted() { Err(Throw::Range) } else { Ok(()) }
}

impl Pcre {
    /// `re.lastIndex = from; re.exec(text)` (flag `g`).
    fn exec(&self, text: &[u16], from: usize) -> Option<M> {
        let m = self.re.find_from_utf16(text, from).next()?;
        let caps = (1..=self.groups as usize).map(|g| m.group(g).map(|r| (r.start, r.end))).collect();
        Some(M { start: m.start(), end: m.end(), caps })
    }

    /// `re.test(text)` from the start (budget: [`start_budget`]/[`check_budget`] around it).
    pub(crate) fn is_match(&self, text: &[u16]) -> bool {
        self.re.find_from_utf16(text, 0).next().is_some()
    }

    /// `allMatches(c, text)`: every match, advancing one code point after an empty one (as PCRE).
    fn all(&self, text: &[u16]) -> Vec<M> {
        // one executor for the whole scan: after an empty match it moves one code point (a surrogate pair is one),
        // after a non-empty one it starts at its end — the TS loop's `lastIndex` rule
        let mut out = Vec::new();
        for m in self.re.find_from_utf16(text, 0) {
            let caps = (1..=self.groups as usize).map(|g| m.group(g).map(|r| (r.start, r.end))).collect();
            out.push(M { start: m.start(), end: m.end(), caps });
            if regress::budget::budget_exhausted() {
                break;
            }
        }
        out
    }
}

/// `smallInt(v, max, ctx)`.
fn small_int(ctx: &dyn FnContext, v: Option<Scalar>, max: f64) -> Result<f64, ErrorCode> {
    let v = match v {
        None | Some(Scalar::Empty) => return Ok(0.0),
        Some(v) => v,
    };
    let k = uo_jsnum::math::trunc(num(ctx, v)?);
    if k < 0.0 || k > max { Err(ErrorCode::Value) } else { Ok(k) }
}

/// The text and pattern of a call (units).
fn text_and_pattern(a: &CallArgs<'_>, ctx: &dyn FnContext) -> Result<(Vec<u16>, Vec<u16>), ErrorCode> {
    let t = txt(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    let p = txt(ctx, scalar(a, 1).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    Ok((t, p))
}

fn regextest(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (tv, pv) = (scalar(a, 0).unwrap_or(Scalar::Empty), scalar(a, 1).unwrap_or(Scalar::Empty));
    // `txt` of both, then the case argument (errors in that order; `txt` fails only on an error value)
    for v in [tv, pv] {
        if let Scalar::Err(e) = v {
            return Err(Stop::Err(e));
        }
    }
    let k = small_int(ctx, scalar(a, 2), 1.0)?;
    let t = txt(ctx, tv)?;
    let p = txt(ctx, pv)?;
    // the texts are read in place (the result is a boolean)
    let r = with_u16(p.as_js(), |p| {
        let c = compile_pcre(p, k == 1.0)?;
        start_budget();
        Some(with_u16(t.as_js(), |t| c.is_match(t)))
    });
    let Some(r) = r else { return Err(Stop::Err(ErrorCode::Value)) };
    check_budget()?;
    Ok(boolean(r))
}

fn str_value(ctx: &mut dyn FnContext, s: &[u16]) -> R<Scalar> {
    Ok(Scalar::Str(ctx.new_text(crate::strings::JsStr::Utf16(s))?))
}

fn regexextract(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (t, p) = text_and_pattern(a, ctx)?;
    let mode = small_int(ctx, scalar(a, 2), 2.0)?;
    let k = small_int(ctx, scalar(a, 3), 1.0)?;
    let Some(c) = compile_pcre(&p, k == 1.0) else { return Err(Stop::Err(ErrorCode::Value)) };
    if mode == 2.0 && c.groups == 0 {
        return Err(Stop::Err(ErrorCode::Value));
    }
    start_budget();
    let ms: Vec<M> = if mode == 1.0 { c.all(&t) } else { c.exec(&t, 0).into_iter().collect() };
    check_budget()?;
    let Some(m0) = ms.first() else { return Err(Stop::Err(ErrorCode::NA)) };
    if mode == 0.0 {
        let s = &t[m0.start..m0.end];
        if s.len() > MAX_TEXT {
            return Err(Stop::Err(ErrorCode::Value));
        }
        return text(ctx, crate::strings::JsStr::Utf16(s));
    }
    if mode == 1.0 {
        if crate::value::array_too_big(1, u32::try_from(ms.len()).unwrap_or(u32::MAX)) {
            return Err(Stop::Throw(Throw::Range));
        }
        let mut data = Vec::with_capacity(ms.len());
        for m in &ms {
            data.push(str_value(ctx, &t[m.start..m.end])?);
        }
        return Ok(Value::Array(ArrayValue::new(1, u32::try_from(ms.len()).unwrap_or(u32::MAX), data)));
    }
    let mut data = Vec::with_capacity(m0.caps.len());
    for g in &m0.caps {
        data.push(match g {
            None => Scalar::Err(ErrorCode::NA),
            Some((s, e)) => str_value(ctx, &t[*s..*e])?,
        });
    }
    Ok(Value::Array(ArrayValue::new(1, c.groups, data)))
}

/// A part of a parsed replacement.
enum Part {
    Lit(Vec<u16>),
    Group(usize),
}

/// `parseReplacement(rep, c)`: `None` = a group that does not exist.
fn parse_replacement(rep: &[u16], c: &Pcre) -> Option<Vec<Part>> {
    let b = |x: u8| u16::from(x);
    let mut out: Vec<Part> = Vec::new();
    let mut lit: Vec<u16> = Vec::new();
    let flush = |out: &mut Vec<Part>, lit: &mut Vec<u16>| {
        if !lit.is_empty() {
            out.push(Part::Lit(std::mem::take(lit)));
        }
    };
    let mut i = 0;
    while i < rep.len() {
        let ch = rep[i];
        if ch == b(b'$') {
            let r = |k: usize| rep.get(i + 1 + k).copied();
            if r(0) == Some(b(b'$')) {
                lit.push(ch);
                i += 2;
                continue;
            }
            if r(0) == Some(b(b'&')) {
                flush(&mut out, &mut lit);
                out.push(Part::Group(0));
                i += 2;
                continue;
            }
            // `/^\{(\d+)\}|^(\d+)/`
            let (digits, len) = if r(0) == Some(b(b'{')) {
                let nd = crate::jstext::digit_run(rep, i + 2);
                if nd > 0 && r(1 + nd) == Some(b(b'}')) { (Some(&rep[i + 2..i + 2 + nd]), nd + 2) } else { (None, 0) }
            } else {
                let nd = crate::jstext::digit_run(rep, i + 1);
                if nd > 0 { (Some(&rep[i + 1..i + 1 + nd]), nd) } else { (None, 0) }
            };
            if let Some(d) = digits {
                let k = crate::jstext::digits_value(d);
                if k > f64::from(c.groups) {
                    return None;
                }
                flush(&mut out, &mut lit);
                out.push(Part::Group(crate::cast::trunc_usize(k)));
                i += 1 + len;
                continue;
            }
            // `/^\{([A-Za-z_]\w*)\}|^([A-Za-z_]\w*)/`
            let (name, len) = if r(0) == Some(b(b'{')) {
                let ln = name_at(rep, i + 2);
                if ln > 0 && r(1 + ln) == Some(b(b'}')) { (Some(&rep[i + 2..i + 2 + ln]), ln + 2) } else { (None, 0) }
            } else {
                let ln = name_at(rep, i + 1);
                if ln > 0 { (Some(&rep[i + 1..i + 1 + ln]), ln) } else { (None, 0) }
            };
            if let Some(nm) = name {
                let g = c.names.iter().find(|(k, _)| k[..] == *nm).map(|&(_, g)| g)?;
                flush(&mut out, &mut lit);
                out.push(Part::Group(g as usize));
                i += 1 + len;
                continue;
            }
            return None;
        }
        if ch == b(b'\\') && i + 1 < rep.len() {
            let nx = rep[i + 1];
            if crate::jstext::is_digit(nx) {
                let k = u32::from(nx - b(b'0'));
                if k > c.groups {
                    return None;
                }
                flush(&mut out, &mut lit);
                out.push(Part::Group(k as usize));
                i += 2;
                continue;
            }
            lit.push(match nx {
                0x6E => 0x0A,
                0x74 => 0x09,
                0x72 => 0x0D,
                0x61 => 0x07,
                0x65 => 0x1B,
                0x66 => 0x0C,
                x => x,
            });
            i += 2;
            continue;
        }
        lit.push(ch);
        i += 1;
    }
    flush(&mut out, &mut lit);
    Some(out)
}

fn regexreplace(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (t, p) = text_and_pattern(a, ctx)?;
    let rep = txt(ctx, scalar(a, 2).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    let mut occ = 0.0;
    match scalar(a, 3) {
        None | Some(Scalar::Empty) => {}
        Some(v) => occ = uo_jsnum::math::trunc(num(ctx, v)?),
    }
    let k = small_int(ctx, scalar(a, 4), 1.0)?;
    let Some(c) = compile_pcre(&p, k == 1.0) else { return Err(Stop::Err(ErrorCode::Value)) };
    let Some(parts) = parse_replacement(&rep, &c) else { return Err(Stop::Err(ErrorCode::Value)) };
    start_budget();
    let ms = c.all(&t);
    check_budget()?;
    // `occ === 0 ? ms : occ > 0 ? [ms[occ - 1]] : [ms[ms.length + occ]]` (NaN picks nothing)
    let len = ms.len() as f64;
    let pick: &[M] = if occ == 0.0 {
        &ms
    } else {
        let ix = if occ > 0.0 { occ - 1.0 } else { len + occ };
        match uo_jsnum::conv::js_index(ix, ms.len()) {
            Some(i) => &ms[i..=i],
            None => &[],
        }
    };
    let mut out = Buf::new();
    let mut last = 0;
    for m in pick {
        if last < m.start {
            out.extend(&t[last..m.start]);
        }
        for q in &parts {
            match q {
                Part::Lit(s) => out.extend(s),
                Part::Group(0) => out.extend(&t[m.start..m.end]),
                Part::Group(g) => {
                    if let Some(Some((s, e))) = m.caps.get(g - 1) {
                        out.extend(&t[*s..*e]);
                    }
                }
            }
        }
        last = m.end;
        if out.len() > MAX_TEXT {
            // the result can only grow: `checkLen` answers `#VALUE!` (bounded memory, same result)
            return Err(Stop::Err(ErrorCode::Value));
        }
    }
    out.extend(&t[last.min(t.len())..]);
    if out.len() > MAX_TEXT {
        return Err(Stop::Err(ErrorCode::Value));
    }
    text(ctx, crate::strings::JsStr::Utf16(out.as_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(p: &str, modifiers: bool) -> Option<String> {
        let mut f = Flags::default();
        translate(&u(p), &mut f, modifiers).ok().map(|t| String::from_utf16_lossy(&t.src))
    }

    #[test]
    fn translation_matches_the_ts() {
        assert_eq!(tr("a(?i)b", true).as_deref(), Some("a(?i:b)"));
        assert_eq!(tr("\\d+$", true).as_deref(), Some("\\p{Nd}+(?=\\n?(?![\\s\\S]))"));
        assert_eq!(tr("(?P<y>\\d{4})", true).as_deref(), Some("(?<y>\\p{Nd}{4})"));
        assert_eq!(tr("[[:alpha:]]", true).as_deref(), Some("[\\p{L}]"));
        assert_eq!(tr("a++", true).as_deref(), Some("a+"));
        assert_eq!(tr("\\x{78}\\170", true).as_deref(), Some("\\u{78}\\u{78}"));
        assert!(tr("(?R)", true).is_none());
        assert!(tr("\\p{L", true).is_none());
    }

    #[test]
    fn catastrophic_backtracking_is_bounded() {
        let c = compile_pcre(&u("(a+)+b"), false).unwrap();
        let t = u(&"a".repeat(40));
        let t0 = std::time::Instant::now();
        start_budget();
        let _ = c.is_match(&t);
        assert!(check_budget().is_err());
        assert!(t0.elapsed().as_secs_f64() < 2.0);
        start_budget();
        assert!(c.is_match(&u("aab")));
        assert!(check_budget().is_ok());
    }
}

#[cfg(test)]
mod unicode_version {
    use super::*;

    /// Decision R1's Unicode-version check: `regress`'s `\p{L}`/`\p{N}`/`\p{Ll}` tables against `uo_text`'s (Node
    /// 24.15 / V8 13.6, Unicode 17) over every code point (every 7th in a debug build). Differences are counted and
    /// listed; the record states the number.
    #[test]
    fn property_tables_match_v8() {
        let step = if cfg!(debug_assertions) { 7 } else { 1 };
        let mut diff = Vec::new();
        for (p, f) in [
            (r"^\p{L}$", uo_text::props::is_letter as fn(u32) -> bool),
            (r"^\p{N}$", uo_text::props::is_number),
            (r"^\p{Ll}$", uo_text::props::is_lowercase_letter),
        ] {
            let re = build(&u(p), Flags::default()).unwrap();
            let mut n = 0;
            for cp in (0..=0x10_FFFFu32).step_by(step) {
                if (0xD800..=0xDFFF).contains(&cp) {
                    continue;
                }
                let mut t = Vec::new();
                super::super::js::push_cp(&mut t, u64::from(cp));
                if re.find_from_utf16(&t, 0).next().is_some() != f(cp) {
                    n += 1;
                    if diff.len() < 20 {
                        diff.push(format!("{p} U+{cp:04X}"));
                    }
                }
            }
            eprintln!("{p}: {n} code points differ from V8");
        }
        assert!(diff.is_empty(), "{diff:?}");
    }
}
