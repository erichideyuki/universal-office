//! The format-code tokenizer of `numfmt.ts` (`splitSections`, `parse`, `parseSection`,
//! `resolveMinutes`, `analyzeNumber`) over UTF-16 code units, as the TS indexes strings.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use uo_jsnum::parse::{parse_float, parse_int, parse_int_utf16, to_number};
use uo_jsnum::text::is_js_space_unit;
use uo_text::case::to_lowercase;

pub(crate) type U = Vec<u16>;

pub(crate) fn u(s: &str) -> U {
    uo_text::utf16::from_str(s)
}

fn cu(c: char) -> u16 {
    // only called with ASCII characters
    u16::try_from(u32::from(c)).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CondOp {
    Lt,
    Gt,
    Eq,
    Le,
    Ge,
    Ne,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cond {
    pub op: CondOp,
    pub v: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// `den`: fixed denominator (`?/10`)
    Lit {
        s: U,
        den: bool,
    },
    /// placeholder: the unit of `0`, `#` or `?`
    Dig(u16),
    Pt,
    Comma,
    Pct,
    Slash,
    At,
    Gen,
    Exp {
        plus: bool,
    },
    Skip(u16),
    Fill(u16),
    /// date/time: c = y|M|d|h|m|s|e|g|b|a (M = month or minute, resolved later), n = repetitions
    Dt {
        c: u8,
        n: usize,
    },
    /// `[h]` `[mm]` `[ss]`
    El {
        c: u8,
        n: usize,
    },
    AmPm {
        full: bool,
        lower: bool,
    },
    /// `.0` `.00` `.000` after the seconds
    Subsec(usize),
}

impl Tok {
    pub(crate) fn lit(s: &str) -> Tok {
        Tok::Lit { s: u(s), den: false }
    }
    fn is_dig(&self) -> bool {
        matches!(self, Tok::Dig(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Num,
    Date,
    Gen,
    Text,
    Lit,
    Empty,
}

#[derive(Clone, Debug)]
pub(crate) struct Frac {
    pub num_idx: Vec<usize>,
    pub den_idx: Vec<usize>,
    /// 0 = none (the TS `fixedDen: number`, falsy when 0)
    pub fixed_den: f64,
    pub int_idx: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub toks: Vec<Tok>,
    pub cond: Option<Cond>,
    pub color: Option<String>,
    pub lcid: Option<u32>,
    pub kind: Kind,
    pub scale: f64,
    pub group: bool,
    pub n_int: usize,
    pub n_frac: usize,
    pub frac: Option<Frac>,
    pub exp: bool,
}

#[derive(Debug)]
pub(crate) struct Parsed {
    pub secs: Vec<Section>,
    /// index into `secs`
    pub text: Option<usize>,
    /// indices into `secs`
    pub nums: Vec<usize>,
    pub has_cond: bool,
}

/// Codes longer than this are parsed on every call instead of cached (memory bound; no effect on results).
pub(crate) const CACHE_MAX_CODE_UNITS: usize = 4096;
/// The TS clears the whole cache when it holds more than this many codes.
pub(crate) const CACHE_MAX_ENTRIES: usize = 2000;

thread_local! {
    static CACHE: RefCell<HashMap<U, Rc<Parsed>>> = RefCell::new(HashMap::new());
}

/// Empties the parse cache of the calling thread.
pub fn clear_caches() {
    CACHE.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            *c = HashMap::new();
        }
    });
}

/// Number of codes in the parse cache of the calling thread (M5 tests).
#[must_use]
pub fn cache_len() -> usize {
    CACHE.with(|c| c.try_borrow().map_or(0, |c| c.len()))
}

fn split_sections(code: &[u16]) -> Vec<U> {
    let mut out: Vec<U> = Vec::new();
    let mut cur: U = Vec::new();
    let mut i = 0;
    let len = code.len();
    let find = |ch: char, from: usize| code.get(from..).and_then(|r| r.iter().position(|&c| c == cu(ch))).map(|p| p + from);
    while i < len {
        let ch = code.get(i).copied().unwrap_or(0);
        if ch == cu('"') || ch == cu('[') {
            let close = if ch == cu('"') { '"' } else { ']' };
            let j = find(close, i + 1).unwrap_or(len);
            cur.extend_from_slice(code.get(i..(j + 1).min(len)).unwrap_or(&[]));
            i = j + 1;
            continue;
        }
        if ch == cu('\\') || ch == cu('_') || ch == cu('*') {
            cur.extend_from_slice(code.get(i..(i + 2).min(len)).unwrap_or(&[]));
            i += 2;
            continue;
        }
        if ch == cu(';') {
            out.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }
        cur.push(ch);
        i += 1;
    }
    out.push(cur);
    out
}

pub(crate) fn parse(code: &[u16]) -> Rc<Parsed> {
    if let Some(p) = CACHE.with(|c| c.try_borrow().ok().and_then(|c| c.get(code).cloned())) {
        return p;
    }
    let raws = split_sections(code);
    let secs: Vec<Section> = raws.iter().map(|r| parse_section(r)).collect();
    // text section: the 4th; with fewer sections, the one that contains @
    let mut text = None;
    let mut nums: Vec<usize> = (0..secs.len().min(3)).collect();
    if secs.len() >= 4 {
        text = Some(3);
    } else if let Some(ti) = secs.iter().position(|s| s.kind == Kind::Text) {
        text = Some(ti);
        nums = (0..secs.len()).filter(|&i| i != ti).collect();
    }
    let has_cond = nums.iter().any(|&i| secs.get(i).is_some_and(|s| s.cond.is_some()));
    let p = Rc::new(Parsed { secs, text, nums, has_cond });
    if code.len() <= CACHE_MAX_CODE_UNITS {
        CACHE.with(|c| {
            if let Ok(mut c) = c.try_borrow_mut() {
                if c.len() > CACHE_MAX_ENTRIES {
                    *c = HashMap::new();
                }
                c.insert(code.to_vec(), Rc::clone(&p));
            }
        });
    }
    p
}

/// `/[ymdhsegba]/i` over one code unit (non-unicode `i`: a non-ASCII unit never canonicalizes to ASCII).
fn is_date_ch(c: u16) -> bool {
    u8::try_from(c).is_ok_and(|b| matches!(b.to_ascii_lowercase(), b'y' | b'm' | b'd' | b'h' | b's' | b'e' | b'g' | b'b' | b'a'))
}

/// The ASCII-case-insensitive test `^lit` over the units of `s` (the TS regexes with `i` and an
/// ASCII-only pattern).
fn starts_with_ci(s: &[u16], lit: &str) -> bool {
    let n = lit.len();
    s.len() >= n && lit.bytes().zip(s).all(|(b, &c)| u8::try_from(c).is_ok_and(|x| x.to_ascii_lowercase() == b))
}

fn eq_ci(s: &[u16], lit: &str) -> bool {
    s.len() == lit.len() && starts_with_ci(s, lit)
}

/// `raw.replace(/"[^"]*"|\\.|_.|\*.|\[[^\]]*\]/g, '')` (`.` excludes the line terminators).
fn strip_quoted(raw: &[u16]) -> U {
    let is_lt = |c: u16| matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029);
    let mut out = U::new();
    let len = raw.len();
    let mut i = 0;
    let find = |ch: char, from: usize| raw.get(from..).and_then(|r| r.iter().position(|&c| c == cu(ch))).map(|p| p + from);
    while i < len {
        let c = raw.get(i).copied().unwrap_or(0);
        if c == cu('"')
            && let Some(j) = find('"', i + 1)
        {
            i = j + 1;
            continue;
        }
        if (c == cu('\\') || c == cu('_') || c == cu('*')) && raw.get(i + 1).is_some_and(|&n| !is_lt(n)) {
            i += 2;
            continue;
        }
        if c == cu('[')
            && let Some(j) = find(']', i + 1)
        {
            i = j + 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `s.replace(/<word>/gi, '')` for an ASCII word.
fn remove_ci(s: &[u16], word: &str) -> U {
    let n = word.len();
    let mut out = U::new();
    let mut i = 0;
    while i < s.len() {
        if s.get(i..).is_some_and(|r| starts_with_ci(r, word)) {
            i += n;
        } else {
            out.push(s.get(i).copied().unwrap_or(0));
            i += 1;
        }
    }
    out
}

/// `s.replace(/e[+-]/gi, '')`
fn remove_exp_marks(s: &[u16]) -> U {
    let mut out = U::new();
    let mut i = 0;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        let next = s.get(i + 1).copied();
        if (c == cu('e') || c == cu('E')) && (next == Some(cu('+')) || next == Some(cu('-'))) {
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `s.replace(/\.0+/g, '')`
fn remove_dot_zeros(s: &[u16]) -> U {
    let mut out = U::new();
    let mut i = 0;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        if c == cu('.') && s.get(i + 1) == Some(&cu('0')) {
            i += 1;
            while s.get(i) == Some(&cu('0')) {
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn push_lit(toks: &mut Vec<Tok>, s: &[u16]) {
    if let Some(Tok::Lit { s: last, .. }) = toks.last_mut() {
        last.extend_from_slice(s);
    } else {
        toks.push(Tok::Lit { s: s.to_vec(), den: false });
    }
}

const COLOR_N: [&str; 56] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "800000", "008000", "000080", "808000", "800080",
    "008080", "C0C0C0", "808080", "9999FF", "993366", "FFFFCC", "CCFFFF", "660066", "FF8080", "0066CC", "CCCCFF", "000080", "FF00FF",
    "FFFF00", "00FFFF", "800080", "800000", "008080", "0000FF", "00CCFF", "CCFFFF", "CCFFCC", "FFFF99", "99CCFF", "FF99CC", "CC99FF",
    "FFCC99", "3366FF", "33CCCC", "99CC00", "FFCC00", "FF9900", "FF6600", "666699", "969696", "003366", "339966", "003300", "333300",
    "993300", "993366", "333399", "333333",
];

fn color_name(low: &[u16]) -> Option<&'static str> {
    let s = String::from_utf16(low).ok()?;
    Some(match s.as_str() {
        "black" => "#000000",
        "blue" => "#0000FF",
        "cyan" => "#00FFFF",
        "green" => "#00FF00",
        "magenta" => "#FF00FF",
        "red" => "#FF0000",
        "white" => "#FFFFFF",
        "yellow" => "#FFFF00",
        _ => return None,
    })
}

fn is_ascii_digit_unit(c: u16) -> bool {
    (0x30..=0x39).contains(&c)
}

fn ascii_string(units: &[u16]) -> String {
    units.iter().map(|&c| char::from_u32(u32::from(c)).unwrap_or('\u{FFFD}')).collect()
}

/// `/^(h+|m+|s+)$/`
fn is_elapsed(low: &[u16]) -> bool {
    let Some(&f) = low.first() else { return false };
    (f == cu('h') || f == cu('m') || f == cu('s')) && low.iter().all(|&c| c == f)
}

/// `/^color\s*(\d+)$/` -> the digits
fn color_n(low: &[u16]) -> Option<&[u16]> {
    let rest = low.strip_prefix(u("color").as_slice())?;
    let ws = rest.iter().take_while(|&&c| is_js_space_unit(c)).count();
    let digits = rest.get(ws..)?;
    (!digits.is_empty() && digits.iter().all(|&c| is_ascii_digit_unit(c))).then_some(digits)
}

/// `/^(<=|>=|<>|<|>|=)\s*([-+]?[\d.]+(?:e[-+]?\d+)?)$/` -> (op, the number text)
fn condition(low: &[u16]) -> Option<(CondOp, String)> {
    for (pat, op) in [("<=", CondOp::Le), (">=", CondOp::Ge), ("<>", CondOp::Ne), ("<", CondOp::Lt), (">", CondOp::Gt), ("=", CondOp::Eq)] {
        let p = u(pat);
        let Some(rest) = low.strip_prefix(p.as_slice()) else { continue };
        let ws = rest.iter().take_while(|&&c| is_js_space_unit(c)).count();
        let num = rest.get(ws..).unwrap_or(&[]);
        let sign = usize::from(num.first().is_some_and(|&c| c == cu('-') || c == cu('+')));
        let body = num.get(sign..).unwrap_or(&[]);
        let run = body.iter().take_while(|&&c| is_ascii_digit_unit(c) || c == cu('.')).count();
        if run == 0 {
            continue;
        }
        let tail = body.get(run..).unwrap_or(&[]);
        let ok = if tail.is_empty() {
            true
        } else if tail.first() == Some(&cu('e')) {
            let t = tail.get(1..).unwrap_or(&[]);
            let s2 = usize::from(t.first().is_some_and(|&c| c == cu('-') || c == cu('+')));
            let d = t.get(s2..).unwrap_or(&[]);
            !d.is_empty() && d.iter().all(|&c| is_ascii_digit_unit(c))
        } else {
            false
        };
        if ok {
            return Some((op, ascii_string(num)));
        }
    }
    None
}

pub(crate) fn parse_section(raw: &[u16]) -> Section {
    let mut toks: Vec<Tok> = Vec::new();
    let mut cond: Option<Cond> = None;
    let mut color: Option<String> = None;
    let mut lcid: Option<u32> = None;
    let bare = strip_quoted(raw);
    // date section: the ".000" after the seconds is a fraction of a second, not a placeholder
    let probe = remove_exp_marks(&remove_ci(&bare, "general"));
    let date_like =
        probe.iter().any(|&c| u8::try_from(c).is_ok_and(|b| matches!(b.to_ascii_lowercase(), b'y' | b'm' | b'd' | b'h' | b's')))
            || (0..probe.len()).any(|i| probe.get(i..).is_some_and(|r| starts_with_ci(r, "am/pm") || starts_with_ci(r, "a/p")));
    let has_digit = {
        let b = if date_like { remove_dot_zeros(&bare) } else { bare };
        b.iter().any(|&c| c == cu('0') || c == cu('#') || c == cu('?'))
    };
    let mut after_slash = false; // after the slash, before the denominator
    let len = raw.len();
    let find = |ch: char, from: usize| raw.get(from..).and_then(|r| r.iter().position(|&c| c == cu(ch))).map(|p| p + from);
    let mut i = 0;
    while i < len {
        let ch = raw.get(i).copied().unwrap_or(0);
        let next = raw.get(i + 1).copied();
        if ch == cu('"') {
            let j = find('"', i + 1).unwrap_or(len);
            push_lit(&mut toks, raw.get(i + 1..j).unwrap_or(&[]));
            i = j + 1;
            continue;
        }
        if ch == cu('\\') {
            if let Some(n) = next {
                push_lit(&mut toks, &[n]);
            }
            i += 2;
            continue;
        }
        if ch == cu('_') {
            toks.push(Tok::Skip(next.unwrap_or(cu(' '))));
            i += 2;
            continue;
        }
        if ch == cu('*') {
            toks.push(Tok::Fill(next.unwrap_or(cu(' '))));
            i += 2;
            continue;
        }
        if ch == cu('[') {
            let j = find(']', i + 1).unwrap_or(len);
            let inner = raw.get(i + 1..j).unwrap_or(&[]);
            i = j + 1;
            let low = to_lowercase(inner);
            let low: &[u16] = &low;
            if is_elapsed(low) {
                let c = low.first().copied().unwrap_or(0);
                toks.push(Tok::El { c: u8::try_from(c).unwrap_or(0), n: low.len() });
                continue;
            }
            if let Some(c) = color_name(low) {
                if color.is_none() {
                    color = Some(c.to_owned());
                }
                continue;
            }
            if let Some(d) = color_n(low) {
                let n = to_number(&ascii_string(d));
                if (1.0..=56.0).contains(&n) && color.is_none() {
                    let idx = uo_jsnum::conv::js_index(n - 1.0, COLOR_N.len()).and_then(|k| COLOR_N.get(k));
                    color = Some(format!("#{}", idx.copied().unwrap_or("000000")));
                }
                continue;
            }
            if let Some((op, num)) = condition(low) {
                cond = Some(Cond { op, v: parse_float(&num) });
                continue;
            }
            if inner.first() == Some(&cu('$')) {
                let body = inner.get(1..).unwrap_or(&[]);
                let dash = body.iter().rposition(|&c| c == cu('-'));
                let sym = dash.map_or(body, |d| body.get(..d).unwrap_or(&[]));
                let code = dash.map_or(&[][..], |d| body.get(d + 1..).unwrap_or(&[]));
                if !sym.is_empty() {
                    push_lit(&mut toks, sym);
                }
                // `x-sysdate` / `x-systime` can never match: `code` has no '-' (the TS tests them anyway)
                if !code.is_empty() && code.iter().all(|&c| u8::try_from(c).is_ok_and(|b| b.is_ascii_hexdigit())) {
                    let v = parse_int(&ascii_string(code), 16);
                    lcid = Some(u32::try_from(uo_jsnum::conv::to_int32(v) & 0xffff).unwrap_or(0));
                }
                continue;
            }
            continue; // [DBNum1], [natural]... ignored
        }
        if (ch == cu('E') || ch == cu('e')) && (next == Some(cu('+')) || next == Some(cu('-'))) && has_digit {
            toks.push(Tok::Exp { plus: next == Some(cu('+')) });
            i += 2;
            continue;
        }
        if ch == cu('0') || ch == cu('#') || ch == cu('?') {
            toks.push(Tok::Dig(ch));
            after_slash = false;
            i += 1;
            continue;
        }
        if ch == cu('.') {
            // .0/.00/.000 right after the seconds = fraction of a second
            let n = raw.get(i + 1..).unwrap_or(&[]).iter().take_while(|&&c| c == cu('0')).count();
            let prev_s = matches!(toks.last(), Some(Tok::Dt { c: b's', .. } | Tok::El { c: b's', .. }));
            if n > 0 && date_like && prev_s {
                toks.push(Tok::Subsec(n));
                i += n + 1;
                continue;
            }
            toks.push(Tok::Pt);
            i += 1;
            continue;
        }
        if ch == cu(',') {
            toks.push(Tok::Comma);
            i += 1;
            continue;
        }
        if ch == cu('%') {
            toks.push(Tok::Pct);
            i += 1;
            continue;
        }
        if ch == cu('/') {
            toks.push(Tok::Slash);
            after_slash = has_digit;
            i += 1;
            continue;
        }
        // fixed denominator ("# ?/10", also after literals): the digits become one literal
        if after_slash && (0x31..=0x39).contains(&ch) {
            let rest = raw.get(i..).unwrap_or(&[]);
            let n = rest.iter().take_while(|&&c| is_ascii_digit_unit(c)).count();
            toks.push(Tok::Lit { s: rest.get(..n).unwrap_or(&[]).to_vec(), den: true });
            i += n;
            after_slash = false;
            continue;
        }
        if ch == cu('@') {
            toks.push(Tok::At);
            i += 1;
            continue;
        }
        let rest = raw.get(i..).unwrap_or(&[]);
        if (ch == cu('G') || ch == cu('g')) && starts_with_ci(rest.get(..7.min(rest.len())).unwrap_or(&[]), "general") {
            toks.push(Tok::Gen);
            i += 7;
            continue;
        }
        if starts_with_ci(rest.get(..5.min(rest.len())).unwrap_or(&[]), "am/pm") {
            toks.push(Tok::AmPm { full: true, lower: false });
            i += 5;
            continue;
        }
        if starts_with_ci(rest.get(..3.min(rest.len())).unwrap_or(&[]), "a/p") {
            toks.push(Tok::AmPm { full: false, lower: ch == cu('a') });
            i += 3;
            continue;
        }
        if is_date_ch(ch) {
            let lc = u8::try_from(ch).unwrap_or(0).to_ascii_lowercase();
            let mut n = 1;
            while raw.get(i + n).is_some_and(|&c| u8::try_from(c).is_ok_and(|b| b.to_ascii_lowercase() == lc)) {
                n += 1;
            }
            if lc == b'a' && n < 3 {
                push_lit(&mut toks, raw.get(i..i + n).unwrap_or(&[]));
                i += n;
                continue;
            }
            toks.push(Tok::Dt { c: if lc == b'm' { b'M' } else { lc }, n });
            i += n;
            continue;
        }
        push_lit(&mut toks, &[ch]);
        i += 1;
    }
    let mut sec =
        Section { toks, cond, color, lcid, kind: Kind::Lit, scale: 1.0, group: false, n_int: 0, n_frac: 0, frac: None, exp: false };
    let has = |toks: &[Tok], f: fn(&Tok) -> bool| toks.iter().any(f);
    if sec.toks.is_empty() {
        // only colour/condition ("[Red][<=100]") = General; a really empty section = nothing
        if raw.contains(&cu('[')) {
            sec.toks.push(Tok::Gen);
            sec.kind = Kind::Gen;
        } else {
            sec.kind = Kind::Empty;
        }
    } else if has(&sec.toks, |t| matches!(t, Tok::Dt { .. } | Tok::El { .. } | Tok::AmPm { .. } | Tok::Subsec(_))) {
        sec.kind = Kind::Date;
    } else if has(&sec.toks, |t| matches!(t, Tok::Gen)) {
        sec.kind = Kind::Gen;
    } else if has(&sec.toks, Tok::is_dig) {
        sec.kind = Kind::Num;
        analyze_number(&mut sec);
    } else if has(&sec.toks, |t| matches!(t, Tok::At)) {
        sec.kind = Kind::Text;
    }
    if sec.kind == Kind::Date {
        resolve_minutes(&mut sec.toks);
    }
    if sec.kind != Kind::Num {
        // outside the numeric section, loose numeric tokens are literals
        for t in &mut sec.toks {
            let s = match t {
                Tok::Pt => ".".to_owned(),
                Tok::Comma => ",".to_owned(),
                Tok::Slash => "/".to_owned(),
                Tok::Pct => "%".to_owned(),
                Tok::Dig(c) => ascii_string(&[*c]),
                Tok::Exp { plus } => if *plus { "E+" } else { "E-" }.to_owned(),
                _ => continue,
            };
            *t = Tok::lit(&s);
        }
    }
    sec
}

/// `m`/`mm` after h (or [h]), after s (or [s]) or before s = minute (c='m'); else month (c='M').
/// Right after [m] it is always month.
fn resolve_minutes(toks: &mut [Tok]) {
    let (mut after_h, mut after_s, mut after_el_m) = (false, false, false);
    for i in 0..toks.len() {
        let Some(t) = toks.get(i) else { break };
        match t {
            Tok::Dt { c: b'h', .. } | Tok::El { c: b'h', .. } => {
                after_h = true;
                after_s = false;
                after_el_m = false;
                continue;
            }
            Tok::Dt { c: b's', .. } | Tok::El { c: b's', .. } => {
                after_s = true;
                after_h = false;
                after_el_m = false;
                continue;
            }
            Tok::El { c: b'm', .. } => {
                after_el_m = true;
                after_h = false;
                after_s = false;
                continue;
            }
            Tok::Dt { c: b'M', n } => {
                let n = *n;
                let mut before_s = false;
                for u in toks.iter().skip(i + 1) {
                    if matches!(u, Tok::Lit { .. } | Tok::Skip(_)) {
                        continue;
                    }
                    before_s = matches!(u, Tok::Dt { c: b's', .. } | Tok::El { c: b's', .. });
                    break;
                }
                if !after_el_m
                    && (after_h || after_s || before_s)
                    && n <= 2
                    && let Some(Tok::Dt { c, .. }) = toks.get_mut(i)
                {
                    *c = b'm';
                }
                after_h = false;
                after_s = false;
                after_el_m = false;
                continue;
            }
            Tok::Dt { .. } | Tok::AmPm { .. } => {
                after_h = false;
                after_s = false;
                after_el_m = false;
            }
            _ => {}
        }
    }
}

fn tok_at(toks: &[Tok], q: i64) -> Option<&Tok> {
    usize::try_from(q).ok().and_then(|q| toks.get(q))
}

fn is_gap(t: &Tok) -> bool {
    matches!(t, Tok::Lit { den: false, .. } | Tok::Skip(_) | Tok::Pct)
}

fn analyze_number(sec: &mut Section) {
    let exp_at = sec.toks.iter().position(|t| matches!(t, Tok::Exp { .. }));
    sec.exp = exp_at.is_some();
    let end = exp_at.unwrap_or(sec.toks.len());
    // decimal point: the 1st before the exponent; the others are literals
    let mut pt_at: Option<usize> = None;
    for i in 0..end {
        if matches!(sec.toks.get(i), Some(Tok::Pt)) {
            if pt_at.is_none() {
                pt_at = Some(i);
            } else if let Some(t) = sec.toks.get_mut(i) {
                *t = Tok::lit(".");
            }
        }
    }
    let int_end = pt_at.unwrap_or(end);
    // fraction: placeholders / placeholders|number
    let slash_at: Option<usize> = if !sec.exp && pt_at.is_none() { sec.toks.iter().position(|t| matches!(t, Tok::Slash)) } else { None };
    match slash_at {
        Some(0) => {
            if let Some(t) = sec.toks.get_mut(0) {
                *t = Tok::lit("/");
            }
        }
        Some(sa) => {
            let toks = &mut sec.toks;
            let mut num_idx: Vec<usize> = Vec::new();
            let mut q = sa as i64 - 1;
            while q >= 0 && tok_at(toks, q).is_some_and(is_gap) {
                q -= 1;
            }
            while q >= 0 && tok_at(toks, q).is_some_and(Tok::is_dig) {
                num_idx.insert(0, q as usize);
                q -= 1;
            }
            let mut den_idx: Vec<usize> = Vec::new();
            let mut fixed_den = 0.0;
            let mut j = sa + 1;
            while toks.get(j).is_some_and(is_gap) {
                j += 1;
            }
            while toks.get(j).is_some_and(Tok::is_dig) {
                den_idx.push(j);
                j += 1;
            }
            if den_idx.is_empty()
                && let Some(Tok::Lit { s, .. }) = toks.get(j)
            {
                let n = s.iter().take_while(|&&c| is_ascii_digit_unit(c)).count();
                if n > 0 {
                    fixed_den = parse_int_utf16(s.get(..n).unwrap_or(&[]), 10);
                    let rest = s.get(n..).unwrap_or(&[]).to_vec();
                    toks[j] = Tok::lit("");
                    if !rest.is_empty() {
                        toks.insert(j + 1, Tok::Lit { s: rest, den: false });
                    }
                    den_idx.push(j); // marks the position of the fixed denominator
                }
            }
            if !num_idx.is_empty() && !den_idx.is_empty() {
                let first = num_idx.first().copied().unwrap_or(0);
                let int_idx: Vec<usize> = (0..first).filter(|&k| toks.get(k).is_some_and(Tok::is_dig)).collect();
                sec.frac = Some(Frac { num_idx, den_idx, fixed_den, int_idx });
            } else if let Some(t) = toks.get_mut(sa) {
                *t = Tok::lit("/");
            }
        }
        None => {}
    }
    for i in 0..sec.toks.len() {
        if matches!(sec.toks.get(i), Some(Tok::Slash))
            && Some(i) != slash_at
            && let Some(t) = sec.toks.get_mut(i)
        {
            *t = Tok::lit("/");
        }
    }
    // commas: between integer placeholders groups; right after the last placeholder scales /1000
    let toks = &mut sec.toks;
    let n_toks = toks.len();
    let dig_at = |toks: &[Tok], from: usize, to: usize| (from..to).any(|k| toks.get(k).is_some_and(Tok::is_dig));
    let mut last_dig: i64 = -1;
    for i in 0..n_toks {
        if toks.get(i).is_some_and(Tok::is_dig) && exp_at.is_none_or(|e| i < e) {
            last_dig = i as i64;
        }
    }
    #[derive(Clone, Copy, PartialEq)]
    enum CK {
        Group,
        Scale,
        Drop,
        Lit,
    }
    let mut scale = 1.0f64;
    let mut kinds: Vec<(usize, CK)> = Vec::new();
    for i in 0..n_toks {
        if !matches!(toks.get(i), Some(Tok::Comma)) {
            continue;
        }
        if i < int_end && dig_at(toks, 0, i) && dig_at(toks, i + 1, int_end) {
            kinds.push((i, CK::Group));
            continue;
        }
        // commas touching the last digit (before the point or the end) scale /1000 each
        let mut k = i as i64 - 1;
        while k >= 0 && matches!(toks.get(k as usize), Some(Tok::Comma)) {
            k -= 1;
        }
        let after_digit = k >= 0 && toks.get(k as usize).is_some_and(Tok::is_dig);
        let mut n = i + 1;
        while matches!(toks.get(n), Some(Tok::Comma)) {
            n += 1;
        }
        let next_dig = n < n_toks && toks.get(n).is_some_and(Tok::is_dig);
        if after_digit && !next_dig && ((i as i64) > last_dig || matches!(toks.get(n), Some(Tok::Pt))) {
            kinds.push((i, CK::Scale));
        } else if exp_at.is_some_and(|e| i < e) {
            kinds.push((i, CK::Drop)); // #,000E+00: only counts as a placeholder
        } else {
            kinds.push((i, CK::Lit));
        }
    }
    for (i, kd) in kinds {
        if kd == CK::Group {
            sec.group = true;
        }
        if kd == CK::Scale {
            scale /= 1000.0;
        }
        if let Some(t) = toks.get_mut(i) {
            *t = Tok::lit(if kd == CK::Lit { "," } else { "" });
        }
    }
    for t in toks.iter() {
        if matches!(t, Tok::Pct) {
            scale *= 100.0;
        }
    }
    sec.scale = scale;
    let stop = exp_at.unwrap_or(sec.toks.len());
    for i in 0..stop {
        if !sec.toks.get(i).is_some_and(Tok::is_dig) {
            continue;
        }
        if let Some(f) = &sec.frac
            && (f.num_idx.contains(&i) || f.den_idx.contains(&i))
        {
            continue;
        }
        if i < int_end {
            sec.n_int += 1;
        } else {
            sec.n_frac += 1;
        }
    }
}

/// `/^general$/i`
pub(crate) fn is_general(code: &[u16]) -> bool {
    eq_ci(code, "general")
}
