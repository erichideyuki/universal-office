//! `functions/text-format.ts`: TEXT's format code in the calculating language translated to the canonical (en-US)
//! code of the shared number-format engine ([`uo_numfmt`]), the TEXT rendering without a column width, and the fixed
//! digits of FIXED/DOLLAR/BAHTTEXT. Every rule is the TS one, read unit by unit (the TS indexes `raw[i]` by UTF-16
//! unit); its regular expressions are hand matchers with the semantics of a `RegExp` without the `u` flag (`/i` over
//! ASCII pattern text is ASCII-only, `.` excludes the line terminators, `\s` is the JS white space).

use std::cell::RefCell;
use std::rc::Rc;

use uo_numfmt::{DEFAULT_GENERAL_BUDGET, FmtPart, FormatLocale, FormatOptions, decimal_digits, format_general, format_value_parts};

use super::js::{Buf, eq_str, index_of, starts_ci, u};
use crate::locale::ValueLocale;
use crate::strings::JsString;

/// `MAX_TEXT`: the longest text a text function returns (`checkLen`).
pub(super) const MAX_TEXT: usize = 32_767;

/// Cap of the per-locale `FormatLocale` cache (the TS `fmtLocaleCache` is keyed by the locale id and never cleared).
const LOCALE_CACHE_CAP: usize = 8;

thread_local! {
    static FMT_LOCALES: RefCell<Vec<(Box<str>, Rc<FormatLocale>)>> = const { RefCell::new(Vec::new()) };
}

/// Empties the `FormatLocale` cache of this thread.
pub(super) fn clear_caches() {
    FMT_LOCALES.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            *c = Vec::new();
        }
    });
}

fn lossy(s: &JsString) -> String {
    s.as_js().to_string_lossy()
}

/// `formatLocaleOf(locale)`: the engine's `FormatLocale` from the value conventions, cached by the locale id (as the
/// TS: a second locale with the same id reads the first one's entry). pt-BR starts from the engine's `PT_BR` (its
/// built-ins and system formats); the copy is a new object, never `PT_BR` itself (decision D3 of `uo-numfmt`).
pub(super) fn format_locale_of(locale: &ValueLocale) -> Rc<FormatLocale> {
    FMT_LOCALES.with(|c| {
        let mut c = match c.try_borrow_mut() {
            Ok(c) => c,
            Err(_) => return Rc::new(build_format_locale(locale)),
        };
        if let Some((_, f)) = c.iter().find(|(id, _)| **id == *locale.id) {
            return Rc::clone(f);
        }
        let f = Rc::new(build_format_locale(locale));
        if c.len() >= LOCALE_CACHE_CAP {
            c.remove(0);
        }
        c.push((locale.id.clone(), Rc::clone(&f)));
        f
    })
}

fn build_format_locale(locale: &ValueLocale) -> FormatLocale {
    let base = if &*locale.id == "pt-BR" { Some(uo_numfmt::pt_br()) } else { None };
    FormatLocale {
        decimal: lossy(&locale.decimal),
        group: lossy(&locale.group),
        months_short: locale.month_abbr.each_ref().map(lossy),
        months_long: locale.month_names.each_ref().map(lossy),
        days_short: locale.day_abbr.each_ref().map(lossy),
        days_long: locale.day_names.each_ref().map(lossy),
        ampm: [lossy(&locale.am), lossy(&locale.pm)],
        builtins: base.map(|b| b.builtins.clone()).unwrap_or_default(),
        sys_date: base.and_then(|b| b.sys_date.clone()),
        sys_time: base.and_then(|b| b.sys_time.clone()),
    }
}

/// `sysFormat(locale, kind)`.
fn sys_format(locale: &ValueLocale, date: bool) -> Vec<u16> {
    let f = format_locale_of(locale);
    if date { u(f.sys_date.as_deref().unwrap_or("dddd, mmmm d, yyyy")) } else { u(f.sys_time.as_deref().unwrap_or("h:mm:ss AM/PM")) }
}

/// `x.toLowerCase()` of a unit string.
fn lower(s: &[u16]) -> Vec<u16> {
    crate::jstext::lower(s).into_owned()
}

/// The text of a JS object property read from a plain object literal whose key is `low`: the `Object.prototype`
/// members whose names are lower case (`constructor`, `__proto__`) are truthy there (A4a-5 pattern).
fn proto_member(low: &[u16]) -> Option<&'static str> {
    if eq_str(low, "constructor") {
        Some("function Object() { [native code] }")
    } else if eq_str(low, "__proto__") {
        Some("[object Object]")
    } else {
        None
    }
}

/// `splitLocal(code)`: the sections of the local code (quotes, brackets and escapes respected).
fn split_local(code: &[u16]) -> Option<Vec<&[u16]>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let n = code.len();
    while i < n {
        let ch = code[i];
        if ch == u16::from(b'"') {
            let j = index_of(code, &[ch], i + 1).unwrap_or(n - 1);
            i = j + 1;
            continue;
        }
        if ch == u16::from(b'[') {
            i = index_of(code, &[u16::from(b']')], i + 1)? + 1;
            continue;
        }
        if ch == u16::from(b'\\') || ch == u16::from(b'_') || ch == u16::from(b'*') {
            if i + 1 >= n {
                return None;
            }
            i += 2;
            continue;
        }
        if ch == u16::from(b';') {
            out.push(&code[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&code[start..]);
    Some(out)
}

/// `/\[\$[^\]]*-(<a>|<b>)\]/i.test(raw)`.
fn sys_tag(raw: &[u16], a: &[u8], b: &[u8]) -> bool {
    let mut p = 0;
    while p + 1 < raw.len() {
        if raw[p] == u16::from(b'[') && raw[p + 1] == u16::from(b'$') {
            // `[^\]]*` cannot cross the first `]`, and the tail ends with it
            if let Some(r) = index_of(raw, &[u16::from(b']')], p + 2) {
                let ends = |t: &[u8]| r > p + 2 + t.len() && raw[r - t.len() - 1] == u16::from(b'-') && starts_ci(raw, r - t.len(), t);
                if ends(a) || ends(b) {
                    return true;
                }
            }
        }
        p += 1;
    }
    false
}

/// The JS line terminators (what `.` does not match without the `s` flag).
fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// `raw.replace(/"[^"]*"?|\\.|_.|\*.|\[[^\]]*\]/g, '')`.
fn strip_literals(raw: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c == u16::from(b'"') {
            i = index_of(raw, &[c], i + 1).map_or(raw.len(), |j| j + 1);
            continue;
        }
        if (c == u16::from(b'\\') || c == u16::from(b'_') || c == u16::from(b'*'))
            && raw.get(i + 1).is_some_and(|&n| !is_line_terminator(n))
        {
            i += 2;
            continue;
        }
        if c == u16::from(b'[')
            && let Some(j) = index_of(raw, &[u16::from(b']')], i + 1)
        {
            i = j + 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `s.replace(new RegExp(escaped(general), 'gi'), '')`: a `/gi` match without `u` (Canonicalize by unit).
fn remove_ci(s: &[u16], general: &[u16]) -> Vec<u16> {
    if general.is_empty() {
        return s.to_vec();
    }
    let canon = uo_text::case::regexp_canonicalize;
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if i + general.len() <= s.len() && s[i..i + general.len()].iter().zip(general).all(|(&a, &b)| canon(a) == canon(b)) {
            i += general.len();
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// The letters of the calculating language, lower case (`k` of the TS).
struct Letters {
    yl: Vec<u16>,
    dl: Vec<u16>,
    ml: Vec<u16>,
    hl: Vec<u16>,
    sl: Vec<u16>,
    mil: Vec<u16>,
    general: Vec<u16>,
    lower_sci: bool,
}

impl Letters {
    fn of(locale: &ValueLocale) -> Self {
        let pick = |f: fn(&crate::locale::FormatLetters) -> &JsString, d: &str| match &locale.format_letters {
            Some(l) => lower(&f(l).as_js().to_utf16()),
            None => u(d),
        };
        let yl = pick(|l| &l.year, "y");
        let general = match &locale.general_name {
            Some(g) => lower(&g.as_js().to_utf16()),
            None => u("general"),
        };
        Letters {
            lower_sci: eq_str(&yl, "y"),
            yl,
            dl: pick(|l| &l.day, "d"),
            ml: pick(|l| &l.month, "m"),
            hl: pick(|l| &l.hour, "h"),
            sl: pick(|l| &l.second, "s"),
            mil: pick(|l| &l.minute, "m"),
            general,
        }
    }

    /// `dateLetters.has(x)`.
    fn is_date_letter(&self, x: &[u16]) -> bool {
        [&self.yl, &self.dl, &self.ml, &self.hl, &self.sl, &self.mil].iter().any(|l| l[..] == *x)
            || eq_str(x, "e")
            || eq_str(x, "b")
            || eq_str(x, "g")
    }
}

/// `sciAt(s, i, lowerSci)`.
fn sci_at(s: &[u16], i: usize, lower_sci: bool) -> bool {
    let c = s.get(i).copied();
    (c == Some(u16::from(b'E')) || (lower_sci && c == Some(u16::from(b'e'))))
        && matches!(s.get(i + 1), Some(&n) if n == u16::from(b'+') || n == u16::from(b'-'))
}

/// `/[c]+/i` where `c` is a one-unit letter string, at `i` (Canonicalize without `u`): the run length.
fn run_ci(s: &[u16], i: usize, letter: &[u16]) -> usize {
    let [l] = letter else { return 0 };
    let canon = uo_text::case::regexp_canonicalize;
    s.get(i..).map_or(0, |r| r.iter().take_while(|&&c| canon(c) == canon(*l)).count())
}

/// `new RegExp('\\[(' + hl + '+|' + ml + '+|' + mil + '+|' + sl + '+)\\]', 'i').test(raw)` (letters of one unit; a
/// host letter of several units is matched as that unit sequence repeated is not supported — declared A4d-4).
fn has_elapsed(raw: &[u16], k: &Letters) -> bool {
    (0..raw.len()).any(|p| {
        raw[p] == u16::from(b'[')
            && [&k.hl, &k.ml, &k.mil, &k.sl].iter().any(|l| {
                let n = run_ci(raw, p + 1, l);
                n > 0 && raw.get(p + 1 + n) == Some(&u16::from(b']'))
            })
    })
}

/// `/am\/pm|a\/p/i.test(s)`.
fn has_ampm(s: &[u16]) -> bool {
    (0..s.len()).any(|i| starts_ci(s, i, b"am/pm") || starts_ci(s, i, b"a/p"))
}

/// A single-unit string equal to `c` (`ch === dec` with `dec` a locale string).
fn is_one(c: u16, s: &JsString) -> bool {
    matches!(s.as_js(), j if j.len() == 1 && j.unit(0) == Some(c))
}

/// `translateFormatCode(code, locale)`: the canonical code, or `None` (TEXT → `#VALUE!`).
pub(super) fn translate_format_code(code: &[u16], locale: &ValueLocale) -> Option<Vec<u16>> {
    let secs = split_local(code)?;
    if secs.len() > 4 {
        return None;
    }
    let k = Letters::of(locale);
    let mut out: Vec<u16> = Vec::new();
    for (n, raw) in secs.iter().enumerate() {
        if n > 0 {
            out.push(u16::from(b';'));
        }
        if sys_tag(raw, b"f800", b"x-sysdate") {
            out.extend(sys_format(locale, true));
            continue;
        }
        if sys_tag(raw, b"f400", b"x-systime") {
            out.extend(sys_format(locale, false));
            continue;
        }
        let bare = remove_ci(&strip_literals(raw), &k.general);
        let date = has_elapsed(raw, &k) || has_ampm(&bare) || {
            // `[...bare].some((c, i) => …)`: `i` counts code points but `sciAt` indexes units (as the TS)
            super::js::chars(&bare)
                .enumerate()
                .any(|(idx, (_, at, w))| k.is_date_letter(&lower(&bare[at..at + w])) && !sci_at(&bare, idx, k.lower_sci))
        };
        out.extend(translate_section(raw, date, locale, &k)?);
    }
    Some(out)
}

/// `translateSection({ raw, date }, locale, k)`.
#[allow(clippy::too_many_lines)]
fn translate_section(raw: &[u16], date: bool, locale: &ValueLocale, k: &Letters) -> Option<Vec<u16>> {
    let n = raw.len();
    let c = |b: u8| u16::from(b);
    let (dec, grp) = (&locale.decimal, &locale.group);
    let mut s: Vec<u16> = Vec::with_capacity(n + 8);
    let (mut has_digit, mut has_at, mut saw_dec, mut saw_exp) = (false, false, false, false);
    let mut last_time: u8 = 0;
    let mut subsec = false;
    let mut is_dig = vec![false; n];
    let mut i = 0;
    while i < n {
        let ch = raw[i];
        if ch == c(b'"') {
            i = index_of(raw, &[ch], i + 1).unwrap_or(n);
        } else if ch == c(b'\\') || ch == c(b'_') || ch == c(b'*') {
            i += 1;
        } else if ch == c(b'[') {
            i = index_of(raw, &[c(b']')], i + 1).unwrap_or(n);
        } else if ch == c(b'0') || ch == c(b'#') || ch == c(b'?') {
            is_dig[i] = true;
        }
        i += 1;
    }
    let digit_in = |a: usize, b: usize| (a..b.min(n)).any(|q| is_dig[q]);
    let exp_at = (0..n.saturating_sub(1)).find(|&q| sci_at(raw, q, k.lower_sci)).unwrap_or(n);
    let dec_u = dec.as_js().to_utf16();
    let first_dec = index_of(raw, &dec_u, 0);
    let int_end = first_dec.unwrap_or(n).min(exp_at);
    let mut i = 0;
    while i < n {
        let ch = raw[i];
        if subsec && ch != c(b'0') {
            subsec = false;
        }
        if ch == c(b'"') {
            let j = index_of(raw, &[ch], i + 1).unwrap_or(n);
            s.push(ch);
            s.extend_from_slice(&raw[i + 1..j]);
            s.push(ch);
            i = j + 1;
            continue;
        }
        if ch == c(b'\\') || ch == c(b'_') || ch == c(b'*') {
            s.extend_from_slice(&raw[i..(i + 2).min(n)]);
            i += 2;
            continue;
        }
        if ch == c(b'[') {
            // a bracket of a section is always closed (`splitLocal` checked it in the same lexical state)
            let end = index_of(raw, &[c(b']')], i + 1)?;
            let b = translate_bracket(&raw[i + 1..end], locale, k)?;
            i = end;
            // `/^\[(h+|m+|s+)\]$/`
            if b.len() >= 3 && b[0] == c(b'[') && b[b.len() - 1] == c(b']') {
                let body = &b[1..b.len() - 1];
                if let Some(&f) = body.first()
                    && (f == c(b'h') || f == c(b'm') || f == c(b's'))
                    && body.iter().all(|&x| x == f)
                {
                    last_time = f as u8;
                }
            }
            s.extend(b);
            i += 1;
            continue;
        }
        if !k.general.is_empty() && i + k.general.len() <= n && lower(&raw[i..i + k.general.len()]) == k.general {
            s.extend(u("General"));
            i += k.general.len();
            continue;
        }
        if starts_ci(raw, i, b"am/pm") {
            s.extend_from_slice(&raw[i..i + 5]);
            i += 5;
            last_time = b'h';
            continue;
        }
        if starts_ci(raw, i, b"a/p") {
            s.extend_from_slice(&raw[i..i + 3]);
            i += 3;
            last_time = b'h';
            continue;
        }
        if ch == c(b'0') || ch == c(b'#') || ch == c(b'?') {
            if !date {
                has_digit = true;
            } else if ch != c(b'0') || !subsec {
                return None;
            }
            s.push(ch);
            i += 1;
            continue;
        }
        if ch == c(b'@') {
            has_at = true;
            s.push(ch);
            i += 1;
            continue;
        }
        let is_dec = is_one(ch, dec);
        if is_dec || is_one(ch, grp) {
            i += 1;
            if date {
                if is_dec && raw.get(i) == Some(&c(b'0')) {
                    if last_time == 0 {
                        return None;
                    }
                    if last_time == b's' {
                        s.push(c(b'.'));
                    } else {
                        s.extend([c(b'\\'), ch]);
                    }
                    subsec = true;
                    continue;
                }
                s.extend([c(b'\\'), ch]);
                continue;
            }
            let i0 = i - 1;
            if is_dec {
                if saw_dec || saw_exp {
                    s.extend([c(b'\\'), ch]);
                    continue;
                }
                saw_dec = true;
                if !digit_in(0, n) {
                    s.push(c(b'#'));
                }
                s.push(c(b'.'));
                continue;
            }
            let mut pj = i0.checked_sub(1);
            while let Some(p) = pj
                && is_one(raw[p], grp)
            {
                pj = p.checked_sub(1);
            }
            let mut nj = i0 + 1;
            while nj < n && is_one(raw[nj], grp) {
                nj += 1;
            }
            let prev_dig = pj.is_some_and(|p| is_dig[p]);
            let next_dig = nj < n && is_dig[nj];
            if saw_dec && !saw_exp {
                let prev_dec_pt = pj.is_some() && pj == first_dec;
                if (prev_dig || prev_dec_pt) && digit_in(i0 + 1, exp_at) {
                    continue;
                }
                if prev_dig {
                    s.push(c(b','));
                    continue;
                }
                if prev_dec_pt {
                    if s.last() == Some(&c(b'.')) {
                        s.pop();
                        s.extend([c(b','), c(b'.')]);
                    } else {
                        s.push(c(b','));
                    }
                    continue;
                }
                s.extend([c(b'\\'), ch]);
                continue;
            }
            if i0 < int_end && (prev_dig || next_dig) && digit_in(0, i0) && digit_in(i0 + 1, int_end) {
                s.push(c(b','));
                continue;
            }
            if prev_dig && !next_dig {
                s.push(c(b','));
                continue;
            }
            s.extend([c(b'\\'), ch]);
            continue;
        }
        if ch == c(b'.') || ch == c(b',') {
            s.extend([c(b'\\'), ch]);
            i += 1;
            continue;
        }
        if sci_at(raw, i, k.lower_sci) && !date {
            if !digit_in(0, i) || !digit_in(i + 2, n) {
                return None;
            }
            s.extend([c(b'E'), raw[i + 1]]);
            i += 2;
            saw_exp = true;
            continue;
        }
        if ch < 0x80 && (ch as u8).is_ascii_alphabetic() {
            let low = lower(&[ch]);
            if eq_str(&low, "n") || ch == c(b'B') || ch == c(b'E') {
                return None;
            }
            let letter = if low == k.yl {
                Some(b'y')
            } else if low == k.dl {
                Some(b'd')
            } else if low == k.ml || low == k.mil {
                Some(b'm')
            } else if low == k.hl {
                Some(b'h')
            } else if low == k.sl {
                Some(b's')
            } else {
                None
            };
            if let Some(l) = letter {
                s.push(c(l));
                last_time = l;
                i += 1;
                continue;
            }
            if ch == c(b'e') {
                s.push(ch);
                last_time = b'y';
                if let Some(&nx) = raw.get(i + 1)
                    && (nx == c(b'+') || nx == c(b'-'))
                {
                    s.extend([c(b'\\'), nx]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            if ch == c(b'b') {
                s.push(ch);
                last_time = b'y';
                i += 1;
                continue;
            }
            if eq_str(&low, "g") {
                s.push(c(b'g'));
                i += 1;
                continue;
            }
            s.extend([c(b'\\'), ch]);
            i += 1;
            continue;
        }
        if bad_accented(ch) {
            return None;
        }
        if date && (ch == c(b'%') || (ch == c(b'/') && last_time == 0)) {
            return None;
        }
        if ch == c(b'/') && !date {
            let next_nonzero = raw.get(i + 1).is_some_and(|&x| (c(b'1')..=c(b'9')).contains(&x));
            if !digit_in(0, i) || !(digit_in(i + 1, n) || next_nonzero) || first_dec.is_some() {
                return None;
            }
            // a group separator inside the numerator → `#VALUE!`
            let mut j = i.checked_sub(1);
            while let Some(p) = j
                && !is_dig[p]
                && !is_one(raw[p], grp)
            {
                j = p.checked_sub(1);
            }
            while let Some(p) = j
                && (is_dig[p] || is_one(raw[p], grp))
            {
                if !is_dig[p] {
                    return None;
                }
                j = p.checked_sub(1);
            }
        }
        if ch == c(b':') && !date {
            let next_dig = raw.get(i + 1).is_some_and(|&x| x == c(b'0') || x == c(b'#') || x == c(b'?'));
            if next_dig || (digit_in(0, i) && digit_in(i + 1, n)) {
                return None;
            }
        }
        s.push(ch);
        i += 1;
    }
    if has_at && has_digit {
        return None;
    }
    Some(s)
}

/// `badAccented(ch)`: a non-ASCII unit whose NFD starts with a/e/n (á, ã, é, ñ…).
fn bad_accented(ch: u16) -> bool {
    if ch < 128 {
        return false;
    }
    let one = [ch];
    let d = uo_text::nfc::nfd(&one);
    d.len() > 1 && matches!(d[0], 0x61 | 0x65 | 0x6E | 0x41 | 0x45 | 0x4E)
}

/// The LCID of a BCP 47 tag of `[$-pt-BR]` (`LCID[tag.toLowerCase()]`).
fn lcid_of(tag_lower: &[u16]) -> Option<&'static str> {
    const LCID: [(&str, &str); 13] = [
        ("en-us", "409"),
        ("en-gb", "809"),
        ("pt-br", "416"),
        ("pt-pt", "816"),
        ("es-es", "C0A"),
        ("es-mx", "80A"),
        ("fr-fr", "40C"),
        ("de-de", "407"),
        ("it-it", "410"),
        ("ja-jp", "411"),
        ("zh-cn", "804"),
        ("ko-kr", "412"),
        ("th-th", "41E"),
    ];
    LCID.iter().find(|(t, _)| eq_str(tag_lower, t)).map(|&(_, l)| l).or_else(|| proto_member(tag_lower))
}

/// `translateBracket(inner, locale, k)`.
fn translate_bracket(inner: &[u16], locale: &ValueLocale, k: &Letters) -> Option<Vec<u16>> {
    let c = |b: u8| u16::from(b);
    let wrap = |body: &[u16]| {
        let mut v = Vec::with_capacity(body.len() + 2);
        v.push(c(b'['));
        v.extend_from_slice(body);
        v.push(c(b']'));
        v
    };
    let low = lower(inner);
    if inner.first() == Some(&c(b'$')) {
        let Some(dash) = index_of(inner, &[c(b'-')], 0) else { return Some(wrap(inner)) };
        let (sym, tag) = (&inner[1..dash], &inner[dash + 1..]);
        if !tag.is_empty() && tag.iter().all(|&x| x < 0x80 && (x as u8).is_ascii_hexdigit()) {
            return Some(wrap(inner));
        }
        return Some(match lcid_of(&lower(tag)) {
            Some(l) => {
                let mut b = vec![c(b'$')];
                b.extend_from_slice(sym);
                b.push(c(b'-'));
                b.extend(u(l));
                wrap(&b)
            }
            None if !sym.is_empty() => {
                let mut b = vec![c(b'$')];
                b.extend_from_slice(sym);
                wrap(&b)
            }
            None => Vec::new(),
        });
    }
    let starts = |p: &str| inner.len() >= p.len() && eq_str(&inner[..p.len()], p);
    if ["<=", ">=", "<>", "<", ">", "="].iter().any(|p| starts(p)) {
        // `inner.split(locale.decimal).join('.')`
        let d = locale.decimal.as_js().to_utf16();
        let mut b = Vec::with_capacity(inner.len());
        if d.is_empty() {
            for (n, &x) in inner.iter().enumerate() {
                if n > 0 {
                    b.push(c(b'.'));
                }
                b.push(x);
            }
        } else {
            let mut at = 0;
            while let Some(p) = index_of(inner, &d, at) {
                b.extend_from_slice(&inner[at..p]);
                b.push(c(b'.'));
                at = p + d.len();
            }
            b.extend_from_slice(&inner[at..]);
        }
        return Some(wrap(&b));
    }
    // elapsed time [h] [mm] [ss] with the letters of the language: `[...low].every(c => c === low[0])`
    if let Some(&l0) = low.first() {
        let all_same = super::js::chars(&low).all(|(_, at, w)| w == 1 && low[at] == l0);
        let is = |l: &[u16]| l.len() == 1 && l[0] == l0;
        if all_same && (is(&k.hl) || is(&k.ml) || is(&k.mil) || is(&k.sl)) {
            let letter = if is(&k.hl) {
                c(b'h')
            } else if is(&k.sl) {
                c(b's')
            } else {
                c(b'm')
            };
            return Some(wrap(&vec![letter; low.len()]));
        }
    }
    let dbnum = inner.len() == 6 && starts_ci(inner, 0, b"dbnum") && (c(b'0')..=c(b'9')).contains(&inner[5]);
    if dbnum || (inner.len() == 7 && starts_ci(inner, 0, b"natural")) {
        return Some(wrap(inner));
    }
    let pt = &*locale.id == "pt-BR";
    const PT: [(&str, &str); 8] = [
        ("preto", "Black"),
        ("azul", "Blue"),
        ("ciano", "Cyan"),
        ("verde", "Green"),
        ("magenta", "Magenta"),
        ("vermelho", "Red"),
        ("branco", "White"),
        ("amarelo", "Yellow"),
    ];
    const EN: [(&str, &str); 8] = [
        ("black", "Black"),
        ("blue", "Blue"),
        ("cyan", "Cyan"),
        ("green", "Green"),
        ("magenta", "Magenta"),
        ("red", "Red"),
        ("white", "White"),
        ("yellow", "Yellow"),
    ];
    let names: &[(&str, &str)] = if pt { &PT } else { &EN };
    if let Some(name) = names.iter().find(|(l, _)| eq_str(&low, l)).map(|&(_, n)| n).or_else(|| proto_member(&low)) {
        return Some(wrap(&u(name)));
    }
    // `^word\s*(\d+)$` (`/i`, no `u`)
    let word: &[u8] = if pt { b"cor" } else { b"color" };
    if starts_ci(inner, 0, word) {
        let mut p = word.len();
        while p < inner.len() && uo_jsnum::text::is_js_space_unit(inner[p]) {
            p += 1;
        }
        let digits = &inner[p..];
        if !digits.is_empty() && digits.iter().all(|&x| (c(b'0')..=c(b'9')).contains(&x)) {
            let n = crate::jstext::digits_value(digits);
            if (1.0..=56.0).contains(&n) {
                let mut b = u("Color");
                b.extend(u(&uo_jsnum::format::to_js_string(n)));
                return Some(wrap(&b));
            }
            return None;
        }
    }
    None
}

/// The value TEXT formats.
pub(super) enum FmtValue<'a> {
    /// A number.
    Num(f64),
    /// A text.
    Str(&'a [u16]),
}

/// `formatForText(value, canonical, locale, date1904)` into `out`: no column width (`*x` writes nothing, `_x` a
/// space), General with 11 characters. `false` = out of range (negative date or past 9999 → `#VALUE!`).
pub(super) fn format_for_text(value: FmtValue<'_>, canonical: &[u16], locale: &ValueLocale, date1904: bool, out: &mut Buf) -> bool {
    let loc = format_locale_of(locale);
    let v = match value {
        FmtValue::Num(x) => uo_numfmt::Value::Num(x),
        FmtValue::Str(s) => uo_numfmt::Value::Str(s),
    };
    let r =
        format_value_parts(v, Some(canonical), &FormatOptions { locale: Some(&loc), col_width_chars: None, date1904, show_zeros: true });
    if r.overflow {
        return false;
    }
    for p in &r.parts {
        match p {
            FmtPart::Text(s) => out.extend(s),
            FmtPart::Skip(_) => out.push(u16::from(b' ')),
            FmtPart::General(x) => out.ascii_or_units(&format_general(*x, &loc, DEFAULT_GENERAL_BUDGET)),
            FmtPart::Fill(_) => {}
        }
    }
    true
}

/// `fixedDigits(x, decimals, locale, group)`: the digits with `decimals` places (rounded half away from zero over
/// 15 significant digits; negative places round left of the point) and the locale's group separator. Returns
/// (negative and not all zero, the unsigned text).
pub(super) fn fixed_digits(x: f64, decimals: f64, decimal: &[u16], group: Option<&[u16]>) -> (bool, Vec<u16>) {
    let a = x.abs();
    let (mut int_s, frac_s) = if decimals >= 0.0 {
        // `decimals` ≤ 127 here (callers check it first)
        decimal_digits(a, crate::cast::trunc_usize(decimals))
    } else {
        let p = uo_jsnum::math::pow(10.0, -decimals);
        let (i, _) = decimal_digits(a / p, 0);
        let int_s = if i.is_empty() { i } else { i + &"0".repeat(crate::cast::trunc_usize(-decimals)) };
        (int_s, String::new())
    };
    if int_s.is_empty() {
        int_s.push('0');
    }
    let mut out: Vec<u16> = Vec::with_capacity(int_s.len() + frac_s.len() + 8);
    let digits = int_s.as_bytes();
    for (k, &d) in digits.iter().enumerate() {
        // `/\B(?=(\d{3})+(?!\d))/g` over a digit run: between two digits, a multiple of 3 digits after
        if let Some(g) = group
            && k > 0
            && (digits.len() - k).is_multiple_of(3)
        {
            out.extend_from_slice(g);
        }
        out.push(u16::from(d));
    }
    let nonzero = |s: &str| s.bytes().any(|b| (b'1'..=b'9').contains(&b));
    let neg = x < 0.0 && (nonzero(&int_s) || nonzero(&frac_s));
    if decimals > 0.0 {
        out.extend_from_slice(decimal);
        out.extend(frac_s.bytes().map(u16::from));
    }
    (neg, out)
}

impl Buf {
    /// Appends a Rust string.
    pub(super) fn ascii_or_units(&mut self, s: &str) {
        if s.is_ascii() {
            self.ascii(s);
        } else {
            for c in s.encode_utf16() {
                self.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(code: &str, l: &ValueLocale) -> Option<String> {
        translate_format_code(&u(code), l).map(|v| String::from_utf16_lossy(&v))
    }

    #[test]
    fn translates_measured_codes() {
        let pt = ValueLocale::pt_br();
        assert_eq!(tr("dd/mm/aaaa", &pt).as_deref(), Some("dd/mm/yyyy"));
        assert_eq!(tr("#.##0,00", &pt).as_deref(), Some("#,##0.00"));
        assert_eq!(tr("Geral", &pt).as_deref(), Some("General"));
        assert_eq!(tr("[Vermelho]0", &pt).as_deref(), Some("[Red]0"));
        assert_eq!(tr("[Red]0", &pt), None);
        assert_eq!(tr("General", &pt), None);
        assert_eq!(tr("0e+0", &pt), None);
        let en = ValueLocale::en_us();
        assert_eq!(tr("#,##0.00", &en).as_deref(), Some("#,##0.00"));
        assert_eq!(tr("[Color10]0", &en).as_deref(), Some("[Color10]0"));
    }
}
