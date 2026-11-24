//! Family `text` (frontier A4d): `functions/text.ts` (this file: CONCATENATE … DETECTLANGUAGE), `text-format.ts`
//! ([`format`]: TEXT's format code, FIXED/DOLLAR digits), `regex.ts` ([`regex`]: REGEXTEST/EXTRACT/REPLACE over the
//! vendored `regress`, decision R1), `filterxml.ts` ([`xml`]) and `regional.ts` ([`regional`]). `tdl-util.ts` is
//! [`super::shared::tdl`].
//!
//! Strings are JS strings: positions count UTF-16 units or code points exactly where the TS does (`Array.from`,
//! `codePointAt`, `.length`), a lone surrogate is a character of its own, case mapping is V8's (`uo_text::case`).
//! Every JS argument rule the TS leans on is kept, `NaN` positions included (`slice(NaN)` = 0, `indexOf(x, NaN)`
//! from 0, `MID(s; NaN; 1)` = ""). Short results are built on the stack ([`js::Buf`]) and copied once into the
//! pass arena. The record (`uo-calc.md` "A4d — as built") lists the bounded TS hangs and the declared edges.

mod format;
mod js;
mod regex;
mod regional;
mod xml;

pub use regex::cache_len as regex_cache_len;
// what XLOOKUP/XMATCH match mode 3 (frontier A4e, `lookup.ts` → `compilePcre`) calls
#[allow(unused_imports)]
pub(crate) use regex::{Pcre, check_budget, compile_pcre, start_budget};

use uo_jsnum::math::{max as js_max, trunc};

use self::format::{FmtValue, MAX_TEXT, fixed_digits, format_for_text, translate_format_code};
use self::js::{Buf, chars, cp_count, has_surr, index_of, last_index_of, slice, slice_from};
use super::shared::tdl::{SPARSE_CELLS, as_array, existing_scalars, num, scalars_of, txt};
use super::shared::{R, Stop, arg, bodies, boolean, len, scalar, text};
use crate::coerce::{number_to_text, parse_number_text, to_bool};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::jstext::with_u16;
use crate::locale::ValueLocale;
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

/// The bodies of the family, one list per TS file (`text-format.ts` has none).
pub(super) const BODIES: &[&[(&str, FnBody)]] = &[TEXT, regex::REGEX, xml::FILTERXML, regional::REGIONAL];

/// Empties the family's per-thread caches (the TEXT `FormatLocale`s, the compiled patterns) and the number-format
/// parse cache TEXT fills.
pub fn clear_caches() {
    format::clear_caches();
    regex::clear_caches();
    uo_numfmt::clear_caches();
}

/// The bodies of `text.ts`.
const TEXT: &[(&str, FnBody)] = bodies![
    "CONCATENATE" => concatenate,
    "CONCAT" => concat,
    "TEXTJOIN" => textjoin,
    "LEFT" => left,
    "LEFTB" => leftb,
    "RIGHT" => right,
    "RIGHTB" => rightb,
    "MID" => mid,
    "MIDB" => midb,
    "LEN" => len_fn,
    "LENB" => lenb,
    "FIND" => find,
    "FINDB" => findb,
    "SEARCH" => search,
    "SEARCHB" => searchb,
    "REPLACE" => replace,
    "REPLACEB" => replaceb,
    "SUBSTITUTE" => substitute,
    "TEXT" => text_fn,
    "VALUE" => value_fn,
    "NUMBERVALUE" => numbervalue,
    "FIXED" => fixed,
    "DOLLAR" => dollar,
    "USDOLLAR" => dollar,
    "TRIM" => trim,
    "CLEAN" => clean,
    "PROPER" => proper_fn,
    "UPPER" => upper_fn,
    "LOWER" => lower_fn,
    "REPT" => rept,
    "CHAR" => char_fn,
    "CODE" => code,
    "UNICHAR" => unichar,
    "UNICODE" => unicode,
    "EXACT" => exact,
    "TEXTBEFORE" => textbefore,
    "TEXTAFTER" => textafter,
    "TEXTSPLIT" => textsplit,
    "VALUETOTEXT" => valuetotext,
    "ARRAYTOTEXT" => arraytotext,
    "ASC" => asc,
    "DBCS" => dbcs,
    "BAHTTEXT" => bahttext,
    "PHONETIC" => phonetic,
    "ENCODEURL" => encodeurl,
    "WEBSERVICE" => webservice,
    "TRANSLATE" => connect,
    "DETECTLANGUAGE" => connect,
];

/// Elements a JS call spread (`f(...xs)`, `push(...xs)`) takes before V8 throws "Maximum call stack size exceeded"
/// (a `RangeError`; stack dependent — declared A4d-6).
const SPREAD_MAX: usize = 120_000;

const fn c(b: u8) -> u16 {
    b as u16
}

// ───────────────────────────── arguments and outputs ─────────────────────────────

/// `args[i] ?? null` of a `value` argument.
fn sv(a: &CallArgs<'_>, i: usize) -> Scalar {
    scalar(a, i).unwrap_or(Scalar::Empty)
}

fn err<T>(e: ErrorCode) -> R<T> {
    Err(Stop::Err(e))
}

/// The units of `txt(v)` (an owned copy: for bodies that read several texts).
fn units(ctx: &dyn FnContext, v: Scalar) -> Result<Vec<u16>, ErrorCode> {
    Ok(txt(ctx, v)?.as_js().to_utf16())
}

/// A text value from units.
fn put(ctx: &mut dyn FnContext, s: &[u16]) -> R<Value> {
    text(ctx, JsStr::Utf16(s))
}

/// `checkLen(s)`.
fn put_checked(ctx: &mut dyn FnContext, s: &[u16]) -> R<Value> {
    if s.len() > MAX_TEXT { err(ErrorCode::Value) } else { put(ctx, s) }
}

/// A text value from a Rust string.
fn put_str(ctx: &mut dyn FnContext, s: &str) -> R<Value> {
    let u: Vec<u16> = s.encode_utf16().collect();
    put(ctx, &u)
}

fn number(x: f64) -> Value {
    super::shared::num(x)
}

/// `scalarsOf(v, ctx)` of `tdl-util.ts` with its spread: an area of more cells than [`SPREAD_MAX`] throws the TS
/// `RangeError` (`out.push(...ctx.values(v, k).data)`).
fn scalars_js(v: &Value, ctx: &mut dyn FnContext, out: &mut Vec<Scalar>) -> R<()> {
    if let Value::Ref(r) = v
        && r.areas().iter().any(|a| (u64::from(a.r2 - a.r1) + 1) * (u64::from(a.c2 - a.c1) + 1) > SPREAD_MAX as u64)
    {
        // the TS reads the area (`ctx.values`) before the spread throws
        for k in 0..r.areas().len() {
            let n = ctx.values(r, k)?.data.len();
            if n > SPREAD_MAX {
                return Err(Stop::Throw(Throw::Range));
            }
        }
    }
    scalars_of(v, ctx, out)
}

/// `delimList(v, ctx)`: the texts of a value (errors first, in order).
fn delim_list(v: &Value, ctx: &mut dyn FnContext) -> R<Vec<Vec<u16>>> {
    let mut xs = Vec::new();
    scalars_js(v, ctx, &mut xs)?;
    let mut out = Vec::with_capacity(xs.len());
    for x in xs {
        out.push(units(ctx, x)?);
    }
    Ok(out)
}

/// `optInt(v, def, ctx)`.
fn opt_int(ctx: &dyn FnContext, v: Option<Scalar>, def: f64) -> Result<f64, ErrorCode> {
    match v {
        None | Some(Scalar::Empty) => Ok(def),
        Some(v) => Ok(trunc(num(ctx, v)?)),
    }
}

/// `optBool(v, def, ctx)` (a text number is `#VALUE!`).
fn opt_bool(ctx: &dyn FnContext, v: Option<Scalar>, def: bool) -> Result<bool, ErrorCode> {
    match v {
        None | Some(Scalar::Empty) => Ok(def),
        Some(v) => to_bool(super::shared::view(ctx, v), ctx.locale()),
    }
}

/// `Math.min(...xs.map(len))` (+∞ for none; the spread throws past [`SPREAD_MAX`]).
fn min_len(ds: &[Vec<u16>]) -> R<f64> {
    if ds.len() > SPREAD_MAX {
        return Err(Stop::Throw(Throw::Range));
    }
    Ok(ds.iter().map(|d| d.len() as f64).fold(f64::INFINITY, uo_jsnum::math::min))
}

// ───────────────────────────── characters and units ─────────────────────────────

/// `isDbcs(locale)`: `/^(ja|zh|ko)\b/i`.
fn is_dbcs(l: &ValueLocale) -> bool {
    regional::lang_is(&l.id, "ja") || regional::lang_is(&l.id, "zh") || regional::lang_is(&l.id, "ko")
}

/// `charBytes(ch)` on the DBCS page (ASCII and half-width katakana = 1).
fn char_bytes(cp: u32) -> f64 {
    if cp < 0x80 || (0xFF61..=0xFF9F).contains(&cp) { 1.0 } else { 2.0 }
}

/// The position unit of a function (code points; for the …B functions bytes in a DBCS language, UTF-16 units in
/// the others — measured in pt-BR).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Cp,
    U16,
    Byte,
}

fn unit_of(bytes: bool, ctx: &dyn FnContext) -> Unit {
    if !bytes {
        Unit::Cp
    } else if is_dbcs(ctx.locale()) {
        Unit::Byte
    } else {
        Unit::U16
    }
}

/// `lenIn(s, u)`.
fn len_in(s: &[u16], u: Unit) -> f64 {
    match u {
        Unit::Cp => cp_count(s) as f64,
        Unit::U16 => s.len() as f64,
        Unit::Byte => chars(s).map(|(cp, ..)| char_bytes(cp)).sum(),
    }
}

/// `byteRange(s, startByte, nBytes)`: the character range (indices of `Array.from(s)`).
fn byte_range(s: &[u16], start: f64, n: f64) -> (usize, usize) {
    let cs: Vec<u32> = chars(s).map(|(cp, ..)| cp).collect();
    let (mut b, mut i) = (0.0, 0);
    while i < cs.len() && b + char_bytes(cs[i]) <= start {
        b += char_bytes(cs[i]);
        i += 1;
    }
    let from = i;
    let mut taken = 0.0;
    while i < cs.len() && taken + char_bytes(cs[i]) <= n {
        taken += char_bytes(cs[i]);
        i += 1;
    }
    (from, i)
}

/// The units of the code points `a..b` (`Array.from(s).slice(a, b).join('')` for valid indices).
fn cp_range(s: &[u16], a: usize, b: usize) -> &[u16] {
    if a >= b {
        return &[];
    }
    let mut lo = s.len();
    let mut hi = s.len();
    for (k, (_, at, _)) in chars(s).enumerate() {
        if k == a {
            lo = at;
        }
        if k == b {
            hi = at;
            break;
        }
    }
    &s[lo.min(hi)..hi]
}

/// `sliceIn(s, u, start, n)`: `[start, start + n)` in the unit.
fn slice_in(s: &[u16], u: Unit, start: f64, n: f64) -> &[u16] {
    match u {
        Unit::U16 => slice(s, start, start + n),
        Unit::Cp => {
            if !has_surr(s) {
                return slice(s, start, start + n);
            }
            let total = cp_count(s);
            let (a, b) = (js::rel(start, total), js::rel(start + n, total));
            cp_range(s, a, b)
        }
        Unit::Byte => {
            let (f, e) = byte_range(s, start, n);
            cp_range(s, f, e)
        }
    }
}

// ───────────────────────────── case ─────────────────────────────

/// `upper(s)`: per code point, `ch.toUpperCase()` when it is one code point, else `ch` (no expansion).
fn upper_into(s: &[u16], out: &mut Buf) {
    if s.iter().all(|&x| x < 0x80) {
        for &x in s {
            out.push(if (c(b'a')..=c(b'z')).contains(&x) { x - 32 } else { x });
        }
        return;
    }
    for (cp, at, w) in chars(s) {
        match uo_text::case::upper_mapping(cp) {
            uo_text::case::CaseMapping::One(m) => out.cp(m),
            uo_text::case::CaseMapping::Many(us) if chars(us).count() == 1 => out.extend(us),
            _ => out.extend(&s[at..at + w]),
        }
    }
}

/// `lower(s)`: `s.replace(/İ/g, 'I').toLowerCase()`.
fn lower_of(s: &[u16]) -> std::borrow::Cow<'_, [u16]> {
    if s.contains(&0x0130) {
        let t: Vec<u16> = s.iter().map(|&x| if x == 0x0130 { c(b'I') } else { x }).collect();
        return std::borrow::Cow::Owned(crate::jstext::lower(&t).into_owned());
    }
    crate::jstext::lower(s)
}

/// `proper(s)`: lower case, then upper case on a letter after a non-letter.
fn proper_into(s: &[u16], out: &mut Buf) {
    let l = lower_of(s);
    let mut prev_letter = false;
    for (cp, at, w) in chars(&l) {
        let is_letter = uo_text::props::is_letter(cp);
        if is_letter && !prev_letter {
            upper_into(&l[at..at + w], out);
        } else {
            out.extend(&l[at..at + w]);
        }
        prev_letter = is_letter;
    }
}

// ───────────────────────────── SEARCH wildcards ─────────────────────────────

/// The end of the segment of a SEARCH pattern that starts at `s` (the next `*` not escaped by `~`) and its number of
/// tokens.
fn wild_segment(p: &[u16], s: usize) -> (usize, usize) {
    let (mut i, mut n) = (s, 0);
    while i < p.len() {
        let ch = p[i];
        if ch == c(b'*') {
            break;
        }
        if ch == c(b'~') {
            // `~x` is the literal x; a `~` at the end vanishes
            if i + 1 < p.len() {
                n += 1;
            }
            i += 2;
            continue;
        }
        n += 1;
        i += 1;
    }
    (i.min(p.len()), n)
}

/// The segment `p[s..e]` (tokens: `?` any unit, `~x` the literal x, else the literal) matches at `hay[at..]`.
fn wild_at(p: &[u16], s: usize, e: usize, hay: &[u16], at: usize) -> bool {
    let canon = uo_text::case::regexp_canonicalize;
    let (mut i, mut h) = (s, at);
    while i < e {
        let (lit, any) = if p[i] == c(b'~') {
            if i + 1 >= p.len() {
                break;
            }
            i += 2;
            (p[i - 1], false)
        } else {
            i += 1;
            (p[i - 1], p[i - 1] == c(b'?'))
        };
        let Some(&x) = hay.get(h) else { return false };
        if !any && x != lit && canon(x) != canon(lit) {
            return false;
        }
        h += 1;
    }
    true
}

/// `searchRegExp(pattern).exec(hay)?.index`: `*`/`?` as in COUNTIF, `~` makes ANY next unit literal (a `~` at the
/// end vanishes), case folded as a `RegExp` with `i` and without `u` (unit by unit). The leftmost start where the
/// pattern matches a prefix: the first segment (before the first `*`) at its leftmost place, then each later
/// segment at its leftmost place after the previous — if that fails no later start can succeed. No allocation.
fn wild_search(p: &[u16], hay: &[u16]) -> Option<usize> {
    let find = |s: usize, e: usize, n: usize, from: usize| (from..=hay.len().checked_sub(n)?).find(|&q| wild_at(p, s, e, hay, q));
    let (e0, n0) = wild_segment(p, 0);
    let first = find(0, e0, n0, 0)?;
    let mut pos = first + n0;
    let mut s = e0;
    while s < p.len() {
        // `s` is at a `*`
        let (e, n) = wild_segment(p, s + 1);
        pos = find(s + 1, e, n, pos)? + n;
        s = e;
    }
    Some(first)
}

// ───────────────────────────── language tables ─────────────────────────────

/// `getLanguage(id)` → the pt-BR table (only `pt-BR` in any case resolves to it), the en-US one (any `en` id), or
/// none.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Pt,
    En,
    None,
}

fn lang(l: &ValueLocale) -> Lang {
    if l.id.eq_ignore_ascii_case("pt-br") {
        Lang::Pt
    } else if l.id.split('-').next().is_some_and(|b| b.eq_ignore_ascii_case("en")) {
        Lang::En
    } else {
        Lang::None
    }
}

/// `errorText(e, locale)`.
fn error_text(e: ErrorCode, l: &ValueLocale) -> &'static str {
    if lang(l) == Lang::Pt { crate::criteria::pt_error_name(e) } else { crate::cell::error_text(e) }
}

/// `listSep(locale)`.
fn list_sep(l: &ValueLocale) -> &'static str {
    match lang(l) {
        Lang::Pt => ";",
        Lang::En => ",",
        Lang::None => {
            if l.decimal.as_js().eq_str(",") {
                ";"
            } else {
                ","
            }
        }
    }
}

/// `arraySeps(locale)`.
fn array_seps(l: &ValueLocale) -> (&'static str, &'static str) {
    match lang(l) {
        Lang::Pt => (if l.decimal.as_js().eq_str("\\") { "@" } else { "\\" }, ";"),
        _ => (",", ";"),
    }
}

/// `valueText(v, strict, locale)` appended to `out`.
fn value_text(ctx: &dyn FnContext, v: Scalar, strict: bool, out: &mut Vec<u16>) {
    let l = ctx.locale();
    match v {
        Scalar::Empty => {}
        Scalar::Err(e) => out.extend(error_text(e, l).encode_utf16()),
        Scalar::Str(r) => {
            let s = ctx.text(r);
            if strict {
                out.push(c(b'"'));
                for x in s.units() {
                    out.push(x);
                    if x == c(b'"') {
                        out.push(x);
                    }
                }
                out.push(c(b'"'));
            } else {
                out.extend(s.units());
            }
        }
        Scalar::Bool(b) => out.extend((if b { &l.true_text } else { &l.false_text }).as_js().units()),
        Scalar::Num(x) => out.extend(number_to_text(x, l).as_js().units()),
    }
}

// ───────────────────────────── bodies ─────────────────────────────

fn concatenate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    for i in 0..len(a) {
        let t = txt(ctx, sv(a, i))?;
        with_u16(t.as_js(), |s| out.extend(s));
    }
    put_checked(ctx, out.as_slice())
}

fn concat(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    let mut xs = Vec::new();
    for i in 0..len(a) {
        let v = arg(a, i).unwrap_or(&super::shared::EMPTY);
        xs.clear();
        existing_scalars(v, ctx, &mut xs)?;
        // past the limit the text is no longer kept, but the errors of the argument still come first
        for &x in &xs {
            let t = txt(ctx, x)?;
            if out.len() <= MAX_TEXT {
                with_u16(t.as_js(), |s| out.extend(s));
            }
        }
        if out.len() > MAX_TEXT {
            return err(ErrorCode::Value);
        }
    }
    put(ctx, out.as_slice())
}

/// The joined text of TEXTJOIN, built while the items are collected (kept only up to a bound past the limit).
struct Joiner<'d> {
    ds: &'d [Vec<u16>],
    out: Vec<u16>,
    items: usize,
    total: f64,
}

impl Joiner<'_> {
    fn push(&mut self, t: &[u16]) {
        if self.items > 0 && !self.ds.is_empty() {
            let d = &self.ds[(self.items - 1) % self.ds.len()];
            self.total += d.len() as f64;
            if self.out.len() <= MAX_TEXT {
                self.out.extend_from_slice(d);
            }
        }
        self.items += 1;
        self.total += t.len() as f64;
        if self.out.len() <= MAX_TEXT {
            self.out.extend_from_slice(t);
        }
    }
}

fn textjoin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ds = delim_list(arg(a, 0).unwrap_or(&super::shared::EMPTY), ctx)?;
    let ig = opt_bool(ctx, scalar(a, 1), true)?;
    let min_delim = if ds.is_empty() { 0.0 } else { min_len(&ds)? };
    let mut j = Joiner { ds: &ds, out: Vec::new(), items: 0, total: 0.0 };
    let mut xs = Vec::new();
    for i in 2..len(a) {
        let v = arg(a, i).unwrap_or(&super::shared::EMPTY);
        xs.clear();
        if let Value::Ref(r) = v
            && r.areas().iter().any(|ar| f64::from(ar.r2 - ar.r1 + 1) * f64::from(ar.c2 - ar.c1 + 1) > SPARSE_CELLS as f64)
        {
            let total = existing_scalars(v, ctx, &mut xs)?;
            if let Some(&Scalar::Err(e)) = xs.iter().find(|x| matches!(x, Scalar::Err(_))) {
                return err(e);
            }
            if !ig && min_delim > 0.0 && total > MAX_TEXT as f64 {
                return err(ErrorCode::Value);
            }
            for &x in &xs {
                let t = units(ctx, x)?;
                if !(ig && t.is_empty()) {
                    j.push(&t);
                }
            }
            continue;
        }
        scalars_js(v, ctx, &mut xs)?;
        for &x in &xs {
            let t = units(ctx, x)?;
            if ig && t.is_empty() {
                continue;
            }
            j.push(&t);
        }
    }
    if j.total > MAX_TEXT as f64 {
        return err(ErrorCode::Value);
    }
    let out = std::mem::take(&mut j.out);
    put(ctx, &out)
}

/// `leftRight(right, bytes, args, ctx)`.
fn left_right(a: &CallArgs<'_>, ctx: &mut dyn FnContext, right: bool, bytes: bool) -> R<Value> {
    let t0 = sv(a, 0);
    let mut n = 1.0;
    // `txt` first, then the count (errors in that order; `txt` fails only on an error value)
    if let Scalar::Err(e) = t0 {
        return err(e);
    }
    if len(a) > 1 {
        n = trunc(num(ctx, sv(a, 1))?);
    }
    if n < 0.0 {
        return err(ErrorCode::Value);
    }
    // LEFTB/RIGHTB in a language without DBCS count characters (measured), unlike MIDB/LENB/FINDB/REPLACEB
    let u = match unit_of(bytes, ctx) {
        Unit::U16 => Unit::Cp,
        x => x,
    };
    let mut out = Buf::new();
    {
        let t = txt(ctx, t0)?;
        with_u16(t.as_js(), |s| {
            if u == Unit::Byte && right {
                let cs: Vec<(u32, usize)> = chars(s).map(|(cp, at, _)| (cp, at)).collect();
                let (mut b, mut i) = (0.0, cs.len());
                while i > 0 && b + char_bytes(cs[i - 1].0) <= n {
                    b += char_bytes(cs[i - 1].0);
                    i -= 1;
                }
                out.extend(&s[cs.get(i).map_or(s.len(), |x| x.1)..]);
                return;
            }
            let l = len_in(s, u);
            let r = if right { slice_in(s, u, js_max(0.0, l - n), n) } else { slice_in(s, u, 0.0, n) };
            out.extend(r);
        });
    }
    put(ctx, out.as_slice())
}

fn left(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    left_right(a, ctx, false, false)
}
fn leftb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    left_right(a, ctx, false, true)
}
fn right(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    left_right(a, ctx, true, false)
}
fn rightb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    left_right(a, ctx, true, true)
}

fn mid_impl(a: &CallArgs<'_>, ctx: &mut dyn FnContext, bytes: bool) -> R<Value> {
    let t0 = sv(a, 0);
    if let Scalar::Err(e) = t0 {
        return err(e);
    }
    let x = num(ctx, sv(a, 1))?;
    let y = num(ctx, sv(a, 2))?;
    let (start, n) = (trunc(x), trunc(y));
    if start < 1.0 || n < 0.0 {
        return err(ErrorCode::Value);
    }
    let u = unit_of(bytes, ctx);
    let mut out = Buf::new();
    {
        let t = txt(ctx, t0)?;
        with_u16(t.as_js(), |s| out.extend(slice_in(s, u, start - 1.0, n)));
    }
    put(ctx, out.as_slice())
}

fn mid(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    mid_impl(a, ctx, false)
}
fn midb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    mid_impl(a, ctx, true)
}

fn len_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = txt(ctx, sv(a, 0))?;
    let n = match t.as_js() {
        JsStr::Latin1(b) => b.len(),
        JsStr::Utf16(s) => cp_count(s),
    };
    Ok(number(n as f64))
}

fn lenb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let u = unit_of(true, ctx);
    let t = txt(ctx, sv(a, 0))?;
    Ok(number(with_u16(t.as_js(), |s| len_in(s, u))))
}

/// `find(ci, bytes, args, ctx)`: FIND (case-sensitive, no wildcards; start up to LEN + 1) and SEARCH (wildcards,
/// case-insensitive; start up to LEN). The texts are read in place (no copy for short ones: the result is a number).
fn find_impl(a: &CallArgs<'_>, ctx: &mut dyn FnContext, ci: bool, bytes: bool) -> R<Value> {
    let (fv, sv1) = (sv(a, 0), sv(a, 1));
    for v in [fv, sv1] {
        if let Scalar::Err(e) = v {
            return err(e);
        }
    }
    let mut start = 1.0;
    if len(a) > 2 {
        start = trunc(num(ctx, sv(a, 2))?);
    }
    let u = unit_of(bytes, ctx);
    let ft = txt(ctx, fv)?;
    let st = txt(ctx, sv1)?;
    let r = with_u16(ft.as_js(), |f| with_u16(st.as_js(), |s| find_in(f, s, start, u, ci)));
    r.map_or(err(ErrorCode::Value), |x| Ok(number(x)))
}

/// The position FIND/SEARCH answer (`None` = `#VALUE!`).
fn find_in(f: &[u16], s: &[u16], start: f64, u: Unit, ci: bool) -> Option<f64> {
    let l = len_in(s, u);
    if start < 1.0 || start > l + if ci { 0.0 } else { 1.0 } {
        return None;
    }
    if f.is_empty() {
        return Some(start);
    }
    // the start in UTF-16 units (NaN stays NaN where the TS keeps it)
    let from: f64 = match u {
        Unit::U16 => start - 1.0,
        Unit::Cp => {
            if has_surr(s) {
                // `for (i = 0; i < cp && i < c.length; i++) u += c[i].length` (NaN → 0)
                let k = start - 1.0;
                let mut uu = 0usize;
                for (i, (_, _, w)) in chars(s).enumerate() {
                    // `i < k` is false for a NaN `k` too
                    if (i as f64).partial_cmp(&k) != Some(std::cmp::Ordering::Less) {
                        break;
                    }
                    uu += w;
                }
                uu as f64
            } else {
                start - 1.0
            }
        }
        Unit::Byte => slice_in(s, Unit::Byte, 0.0, start - 1.0).len() as f64,
    };
    let at: f64 = if ci {
        match wild_search(f, slice_from(s, from)) {
            Some(i) => from + i as f64,
            None => -1.0,
        }
    } else {
        index_of(s, f, js::clamp(from, s.len())).map_or(-1.0, |i| i as f64)
    };
    if at < 0.0 {
        return None;
    }
    Some(match u {
        Unit::U16 => at + 1.0,
        Unit::Cp => {
            if has_surr(s) {
                cp_count(slice(s, 0.0, at)) as f64 + 1.0
            } else {
                at + 1.0
            }
        }
        Unit::Byte => len_in(slice(s, 0.0, at), Unit::Byte) + 1.0,
    })
}

fn find(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    find_impl(a, ctx, false, false)
}
fn findb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    find_impl(a, ctx, false, true)
}
fn search(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    find_impl(a, ctx, true, false)
}
fn searchb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    find_impl(a, ctx, true, true)
}

fn replace_impl(a: &CallArgs<'_>, ctx: &mut dyn FnContext, bytes: bool) -> R<Value> {
    let s = units(ctx, sv(a, 0))?;
    let x = num(ctx, sv(a, 1))?;
    let y = num(ctx, sv(a, 2))?;
    let r = units(ctx, sv(a, 3))?;
    let (start, n) = (trunc(x), trunc(y));
    if start < 1.0 || n < 0.0 {
        return err(ErrorCode::Value);
    }
    let u = unit_of(bytes, ctx);
    let l = len_in(&s, u);
    let mut out = Buf::new();
    out.extend(slice_in(&s, u, 0.0, start - 1.0));
    out.extend(&r);
    out.extend(slice_in(&s, u, start - 1.0 + n, js_max(0.0, l - (start - 1.0 + n))));
    put_checked(ctx, out.as_slice())
}

fn replace(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    replace_impl(a, ctx, false)
}
fn replaceb(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    replace_impl(a, ctx, true)
}

fn substitute(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let s = units(ctx, sv(a, 0))?;
    let o = units(ctx, sv(a, 1))?;
    let r = units(ctx, sv(a, 2))?;
    let mut out = Buf::new();
    if len(a) > 3 {
        let n = trunc(num(ctx, sv(a, 3))?);
        if n < 1.0 {
            return err(ErrorCode::Value);
        }
        if o.is_empty() {
            return put(ctx, &s);
        }
        // occurrences counted with overlap (measured: "aaaa","aa","b",2 → "aba"); NaN runs no iteration
        let mut pos: f64 = -1.0;
        let mut from = 0;
        let mut i = 0.0;
        while i < n {
            match index_of(&s, &o, from) {
                None => return put(ctx, &s),
                Some(p) => {
                    pos = p as f64;
                    from = p + 1;
                }
            }
            i += 1.0;
        }
        out.extend(slice(&s, 0.0, pos));
        out.extend(&r);
        out.extend(slice_from(&s, pos + o.len() as f64));
        return put_checked(ctx, out.as_slice());
    }
    if o.is_empty() {
        return put(ctx, &s);
    }
    let mut at = 0;
    while let Some(p) = index_of(&s, &o, at) {
        out.extend(&s[at..p]);
        out.extend(&r);
        at = p + o.len();
        if out.len() > MAX_TEXT {
            return err(ErrorCode::Value);
        }
    }
    out.extend(&s[at..]);
    put_checked(ctx, out.as_slice())
}

fn text_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let code = units(ctx, sv(a, 1))?;
    let v = sv(a, 0);
    let l = ctx.locale();
    enum V {
        N(f64),
        S(Vec<u16>),
    }
    let value = match v {
        Scalar::Bool(b) => V::S((if b { &l.true_text } else { &l.false_text }).as_js().to_utf16()),
        Scalar::Empty => V::N(0.0),
        Scalar::Str(r) => {
            let s = ctx.text(r);
            match parse_number_text(s, l, ctx.date1904(), ctx.year()) {
                Some(n) => V::N(n),
                None => V::S(s.to_utf16()),
            }
        }
        Scalar::Num(n) => V::N(n),
        Scalar::Err(e) => return err(e),
    };
    // empty code: a number → "" ("-" if negative); text and booleans as they are (measured)
    if code.is_empty() {
        return match value {
            V::N(n) => put_str(ctx, if n < 0.0 { "-" } else { "" }),
            V::S(s) => put(ctx, &s),
        };
    }
    let Some(canon) = translate_format_code(&code, ctx.locale()) else { return err(ErrorCode::Value) };
    let mut out = Buf::new();
    let fv = match &value {
        V::N(n) => FmtValue::Num(*n),
        V::S(s) => FmtValue::Str(s),
    };
    if !format_for_text(fv, &canon, ctx.locale(), ctx.date1904(), &mut out) {
        return err(ErrorCode::Value);
    }
    put_checked(ctx, out.as_slice())
}

fn value_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    match sv(a, 0) {
        Scalar::Num(n) => Ok(number(n)),
        Scalar::Empty => Ok(number(0.0)),
        Scalar::Bool(_) => err(ErrorCode::Value),
        Scalar::Err(e) => err(e),
        Scalar::Str(r) => {
            let n = parse_number_text(ctx.text(r), ctx.locale(), ctx.date1904(), ctx.year());
            n.map_or(err(ErrorCode::Value), |n| Ok(number(n)))
        }
    }
}

/// NUMBERVALUE(text; [decimal]; [group]), measured: every space out, each trailing `%` divides by 100, two decimals
/// or a group after the decimal → `#VALUE!`, the group removed, the decimal turned into the locale's, then read as
/// VALUE.
fn numbervalue(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let tv = sv(a, 0);
    match tv {
        Scalar::Num(n) => return Ok(number(n)),
        Scalar::Bool(_) => return err(ErrorCode::Value),
        _ => {}
    }
    let s0 = units(ctx, tv)?;
    let l = ctx.locale();
    let ldec = l.decimal.as_js().to_utf16();
    let lgrp = l.group.as_js().to_utf16();
    let mut dec: Vec<u16> = ldec.clone();
    let mut grp: Option<Vec<u16>> = None;
    if len(a) > 1 && !matches!(sv(a, 1), Scalar::Empty) {
        let d = units(ctx, sv(a, 1))?;
        if d.is_empty() {
            return err(ErrorCode::Value);
        }
        dec = vec![d[0]];
    }
    if len(a) > 2 && !matches!(sv(a, 2), Scalar::Empty) {
        let g = units(ctx, sv(a, 2))?;
        if g.is_empty() {
            return err(ErrorCode::Value);
        }
        grp = Some(vec![g[0]]);
    }
    let grp = grp.unwrap_or_else(|| if dec == lgrp { ldec.clone() } else { lgrp.clone() });
    if dec == grp {
        return err(ErrorCode::Value);
    }
    let mut s: Vec<u16> = s0.into_iter().filter(|&x| !uo_jsnum::text::is_js_space_unit(x)).collect();
    let mut pct = 0;
    while s.last() == Some(&c(b'%')) {
        pct += 1;
        s.pop();
    }
    let di = index_of(&s, &dec, 0);
    if let Some(di) = di
        && (index_of(&s, &dec, di + 1).is_some() || index_of(&s, &grp, di + 1).is_some())
    {
        return err(ErrorCode::Value);
    }
    // `s.split(grp).join('')`
    if !grp.is_empty() {
        let mut t = Vec::with_capacity(s.len());
        let mut at = 0;
        while let Some(p) = index_of(&s, &grp, at) {
            t.extend_from_slice(&s[at..p]);
            at = p + grp.len();
        }
        t.extend_from_slice(&s[at..]);
        s = t;
    }
    if di.is_some() && dec != ldec {
        // `s.replace(dec, L.decimal)`: the first occurrence
        if let Some(p) = index_of(&s, &dec, 0) {
            s.splice(p..p + dec.len(), ldec.iter().copied());
        }
    }
    if s.is_empty() {
        return Ok(number(0.0));
    }
    let Some(n) = parse_number_text(JsStr::Utf16(&s), l, ctx.date1904(), ctx.year()) else { return err(ErrorCode::Value) };
    Ok(number(n / uo_jsnum::math::pow(100.0, f64::from(pct))))
}

/// `optIntArg(args, i, def, ctx, nullAs)`.
fn opt_int_arg(a: &CallArgs<'_>, ctx: &dyn FnContext, i: usize, def: f64, null_as: f64) -> Result<f64, ErrorCode> {
    if len(a) <= i {
        return Ok(def);
    }
    match sv(a, i) {
        Scalar::Empty => Ok(null_as),
        v => Ok(trunc(num(ctx, v)?)),
    }
}

fn fixed(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = num(ctx, sv(a, 0))?;
    let d = opt_int_arg(a, ctx, 1, 2.0, 2.0)?;
    let nc = if len(a) > 2 { opt_bool(ctx, scalar(a, 2), false)? } else { false };
    if d > 127.0 {
        return err(ErrorCode::Value);
    }
    let l = ctx.locale();
    let (dec, grp) = (l.decimal.as_js().to_utf16(), l.group.as_js().to_utf16());
    let (neg, body) = fixed_digits(x, d, &dec, if nc { None } else { Some(&grp) });
    let mut out = Buf::new();
    if neg {
        out.push(c(b'-'));
    }
    out.extend(&body);
    if out.len() > 255 {
        return err(ErrorCode::Value);
    }
    put(ctx, out.as_slice())
}

fn dollar(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = num(ctx, sv(a, 0))?;
    let d = opt_int_arg(a, ctx, 1, 2.0, 0.0)?;
    if d > 127.0 {
        return err(ErrorCode::Value);
    }
    let l = ctx.locale();
    let (dec, grp) = (l.decimal.as_js().to_utf16(), l.group.as_js().to_utf16());
    let (_, body) = fixed_digits(x, d, &dec, Some(&grp));
    let cur = l.currency.as_js().to_utf16();
    let neg = x < 0.0;
    let mut out = Buf::new();
    if &*l.id == "en-US" {
        if neg {
            out.push(c(b'('));
        }
        out.extend(&cur);
        out.extend(&body);
        if neg {
            out.push(c(b')'));
        }
    } else {
        if neg {
            out.push(c(b'-'));
        }
        out.extend(&cur);
        out.push(c(b' '));
        out.extend(&body);
    }
    if out.len() > 255 {
        return err(ErrorCode::Value);
    }
    put(ctx, out.as_slice())
}

fn trim(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| {
            let mut prev_space = false;
            for &x in s {
                if x == c(b' ') {
                    if prev_space {
                        continue;
                    }
                    prev_space = true;
                } else {
                    prev_space = false;
                }
                out.push(x);
            }
        });
    }
    let s = out.as_slice();
    let lo = usize::from(s.first() == Some(&c(b' ')));
    let hi = if s.len() > lo && s.last() == Some(&c(b' ')) { s.len() - 1 } else { s.len() };
    put(ctx, &s[lo..hi])
}

fn clean(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| {
            for &x in s {
                if !(x < 0x20 || matches!(x, 0x81 | 0x8D | 0x8F | 0x90 | 0x9D)) {
                    out.push(x);
                }
            }
        });
    }
    put(ctx, out.as_slice())
}

fn proper_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| proper_into(s, &mut out));
    }
    put(ctx, out.as_slice())
}

fn upper_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| upper_into(s, &mut out));
    }
    put(ctx, out.as_slice())
}

fn lower_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| out.extend(&lower_of(s)[..]));
    }
    put(ctx, out.as_slice())
}

fn rept(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let s = units(ctx, sv(a, 0))?;
    let n = trunc(num(ctx, sv(a, 1))?);
    if n < 0.0 || s.len() as f64 * n > MAX_TEXT as f64 {
        return err(ErrorCode::Value);
    }
    // `s.repeat(n)`: +∞ is a `RangeError` (reachable with an empty text), NaN is 0
    if n == f64::INFINITY {
        return Err(Stop::Throw(Throw::Range));
    }
    if s.is_empty() || n.is_nan() || n < 1.0 {
        return put(ctx, &[]);
    }
    let mut out = Buf::new();
    for _ in 0..crate::cast::trunc_usize(n) {
        out.extend(&s);
    }
    put(ctx, out.as_slice())
}

/// `CP1252_HI`: CHAR/CODE 128–159 on page 1252.
const CP1252_HI: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090,
    0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

fn char_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = trunc(num(ctx, sv(a, 0))?);
    if n < 1.0 || n > 255.0 {
        return err(ErrorCode::Value);
    }
    // `n >= 128 && n < 160 ? CP1252_HI[n - 128] : String.fromCharCode(n)` (NaN → "\0")
    let u = if (128.0..160.0).contains(&n) { CP1252_HI[crate::cast::trunc_usize(n - 128.0)] } else { js::from_char_code(n) };
    put(ctx, &[u])
}

fn code(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = txt(ctx, sv(a, 0))?;
    let s = t.as_js();
    let first = with_u16(s, |s| uo_text::utf16::decode_at(s, 0).map(|(cp, _)| cp));
    let Some(cp) = first else { return err(ErrorCode::Value) };
    let v = if cp < 128 || (160..256).contains(&cp) {
        cp
    } else {
        CP1252_HI.iter().position(|&h| u32::from(h) == cp).map_or(63, |i| 128 + i as u32)
    };
    Ok(number(f64::from(v)))
}

fn unichar(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = trunc(num(ctx, sv(a, 0))?);
    if n < 1.0 || n > 1_114_111.0 {
        return err(ErrorCode::Value);
    }
    if n.is_nan() {
        // `String.fromCodePoint(NaN)`: "Invalid code point"
        return Err(Stop::Throw(Throw::Range));
    }
    let cp = crate::cast::to_u32(n);
    if (0xD800..=0xDFFF).contains(&cp) || (cp & 0xFFFE) == 0xFFFE || (0xFDD0..=0xFDEF).contains(&cp) {
        return err(ErrorCode::NA);
    }
    let mut out = Buf::new();
    out.cp(cp);
    put(ctx, out.as_slice())
}

fn unicode(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = txt(ctx, sv(a, 0))?;
    let first = with_u16(t.as_js(), |s| uo_text::utf16::decode_at(s, 0).map(|(cp, _)| cp));
    first.map_or(err(ErrorCode::Value), |cp| Ok(number(f64::from(cp))))
}

fn exact(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = txt(ctx, sv(a, 0))?;
    let y = txt(ctx, sv(a, 1))?;
    Ok(boolean(x.as_js().eq_units(y.as_js())))
}

/// A hit of `nthIndex`: unit index and length.
#[derive(Clone, Copy)]
struct Hit {
    at: usize,
    len: usize,
}

/// The forward scan of `nthIndex`/`countHits` (no empty delimiter here): calls `f` with each hit in order until it
/// returns `false`. Each delimiter's next position is remembered while it stays ahead (same hits as the TS's
/// `indexOf` per step, without rescanning).
fn scan_forward(hay: &[u16], ds: &[Vec<u16>], mut f: impl FnMut(Hit) -> bool) {
    let mut next: Vec<Option<Option<usize>>> = vec![None; ds.len()];
    let mut pos = 0;
    while pos <= hay.len() {
        let mut best: Option<Hit> = None;
        for (k, d) in ds.iter().enumerate() {
            let i = match next[k] {
                Some(Some(i)) if i >= pos => Some(i),
                Some(None) => None,
                _ => {
                    let i = index_of(hay, d, pos);
                    next[k] = Some(i);
                    i
                }
            };
            if let Some(i) = i
                && best.is_none_or(|b| i < b.at || (i == b.at && d.len() > b.len))
            {
                best = Some(Hit { at: i, len: d.len() });
            }
        }
        let Some(b) = best else { break };
        if !f(b) {
            return;
        }
        pos = b.at + b.len.max(1);
    }
}

/// `nthIndex(text, delims, n, ci)` over the (case-folded) haystack and delimiters.
fn nth_index(hay: &[u16], ds: &[Vec<u16>], n: f64) -> Option<Hit> {
    if n > 0.0 {
        let mut k = 0.0;
        let mut out = None;
        scan_forward(hay, ds, |h| {
            k += 1.0;
            if k == n {
                out = Some(h);
                return false;
            }
            true
        });
        return out;
    }
    let mut end = hay.len() as f64;
    let mut k = 0.0;
    while end >= 0.0 {
        let mut best: Option<Hit> = None;
        for d in ds {
            if let Some(i) = last_index_of(hay, d, end - d.len() as f64)
                && (i + d.len()) as f64 <= end
                && best.is_none_or(|b| i > b.at)
            {
                best = Some(Hit { at: i, len: d.len() });
            }
        }
        let b = best?;
        k += 1.0;
        if k == -n {
            return Some(b);
        }
        end = b.at as f64 - if b.len == 0 { 1.0 } else { 0.0 };
    }
    None
}

/// `notFound(nf, ctx)`: the `if_not_found` value as text (an error or a text passes).
fn not_found(nf: Option<&Value>, ctx: &mut dyn FnContext) -> R<Value> {
    let Some(v) = nf else { return err(ErrorCode::NA) };
    let s = match v {
        Value::Ref(r) => match r.areas().first() {
            Some(a) => ctx.cell(a.sheet, a.r1, a.c1)?,
            None => Scalar::Err(ErrorCode::Ref),
        },
        Value::Array(x) => x.get(0, 0),
        Value::Scalar(s) => *s,
        // the TS converts the object (declared A4d-7)
        Value::Lambda(_) => Scalar::Err(ErrorCode::Value),
    };
    match s {
        Scalar::Err(e) => err(e),
        Scalar::Str(_) => Ok(Value::Scalar(s)),
        s => {
            let u = units(ctx, s)?;
            put(ctx, &u)
        }
    }
}

/// `textBeforeAfter(after, args, ctx)`.
fn before_after(a: &CallArgs<'_>, ctx: &mut dyn FnContext, after: bool) -> R<Value> {
    let tv = sv(a, 0);
    if let Scalar::Err(e) = tv {
        return err(e);
    }
    let t = units(ctx, tv)?;
    let ds = delim_list(arg(a, 1).unwrap_or(&super::shared::EMPTY), ctx)?;
    let n = opt_int(ctx, scalar(a, 2), 1.0)?;
    let mode = opt_int(ctx, scalar(a, 3), 0.0)?;
    let end = opt_int(ctx, scalar(a, 4), 0.0)?;
    if n == 0.0 || (mode != 0.0 && mode != 1.0) || (end != 0.0 && end != 1.0) {
        return err(ErrorCode::Value);
    }
    let nf = arg(a, 5);
    if t.is_empty() {
        return if end == 1.0 { err(ErrorCode::Value) } else { not_found(nf, ctx) };
    }
    // an empty delimiter matches at the start (n > 0) or the end (n < 0)
    if ds.iter().any(Vec::is_empty) {
        return if (n > 0.0) == after { put(ctx, &t) } else { put(ctx, &[]) };
    }
    if n.abs() > t.len() as f64 - min_len(&ds)? + 1.0 {
        return err(ErrorCode::Value);
    }
    let ci = mode == 1.0;
    let (hay, dsl): (std::borrow::Cow<'_, [u16]>, Vec<Vec<u16>>) =
        if ci { (lower_of(&t), ds.iter().map(|d| lower_of(d).into_owned()).collect()) } else { (std::borrow::Cow::Borrowed(&t[..]), ds) };
    let mut hit = nth_index(&hay, &dsl, n);
    if hit.is_none() && end == 1.0 {
        // the end (or the start, counting from the back) of the text counts as a delimiter
        let mut count = 0.0;
        scan_forward(&hay, &dsl, |_| {
            count += 1.0;
            true
        });
        if n > 0.0 && n == count + 1.0 {
            hit = Some(Hit { at: t.len(), len: 0 });
        } else if n < 0.0 && -n == count + 1.0 {
            hit = Some(Hit { at: 0, len: 0 });
        }
    }
    let Some(h) = hit else { return not_found(nf, ctx) };
    if after { put(ctx, slice_from(&t, (h.at + h.len) as f64)) } else { put(ctx, slice(&t, 0.0, h.at as f64)) }
}

fn textbefore(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    before_after(a, ctx, false)
}
fn textafter(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    before_after(a, ctx, true)
}

/// TEXTSPLIT(text; col; [row]; [ignore_empty]; [mode]; [pad]): one scan, earliest delimiter (the longest on a tie;
/// column before row — the same delimiter in both is a column), missing cells padded with `#N/A`.
fn textsplit(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let t = units(ctx, sv(a, 0))?;
    let cols = match arg(a, 1) {
        None | Some(Value::Scalar(Scalar::Empty)) => Vec::new(),
        Some(v) => delim_list(v, ctx)?,
    };
    let rows = match arg(a, 2) {
        None | Some(Value::Scalar(Scalar::Empty)) => Vec::new(),
        Some(v) => delim_list(v, ctx)?,
    };
    let ig = opt_bool(ctx, scalar(a, 3), false)?;
    let mode = opt_int(ctx, scalar(a, 4), 0.0)?;
    if mode != 0.0 && mode != 1.0 {
        return err(ErrorCode::Value);
    }
    if cols.is_empty() && rows.is_empty() {
        return err(ErrorCode::Value);
    }
    if cols.iter().chain(&rows).any(Vec::is_empty) {
        return err(ErrorCode::Value);
    }
    if t.is_empty() {
        return err(ErrorCode::Value);
    }
    let pad = match arg(a, 5) {
        None | Some(Value::Scalar(Scalar::Empty)) => Scalar::Err(ErrorCode::NA),
        Some(Value::Array(x)) => x.get(0, 0),
        Some(Value::Ref(r)) => match r.areas().first() {
            Some(ar) => ctx.cell(ar.sheet, ar.r1, ar.c1)?,
            None => Scalar::Err(ErrorCode::Ref),
        },
        Some(Value::Scalar(s)) => *s,
        Some(Value::Lambda(_)) => Scalar::Err(ErrorCode::Value),
    };
    let ci = mode == 1.0;
    let hay: std::borrow::Cow<'_, [u16]> = if ci { lower_of(&t) } else { std::borrow::Cow::Borrowed(&t[..]) };
    let ds: Vec<(Vec<u16>, bool)> = cols
        .iter()
        .map(|d| (d, false))
        .chain(rows.iter().map(|d| (d, true)))
        .map(|(d, row)| (if ci { lower_of(d).into_owned() } else { d.clone() }, row))
        .collect();
    // the cells as unit ranges of `t`
    let mut grid: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut line: Vec<(usize, usize)> = Vec::new();
    let (mut pos, mut last) = (0usize, 0usize);
    let mut next: Vec<Option<Option<usize>>> = vec![None; ds.len()];
    let cell = |line: &mut Vec<(usize, usize)>, a: usize, b: usize| {
        if !(ig && a >= b) {
            line.push((a, b));
        }
    };
    loop {
        let (mut best, mut blen, mut brow): (Option<usize>, usize, bool) = (None, 0, false);
        for (k, (d, row)) in ds.iter().enumerate() {
            let i = match next[k] {
                Some(Some(i)) if i >= pos => Some(i),
                Some(None) => None,
                _ => {
                    let i = index_of(&hay, d, pos);
                    next[k] = Some(i);
                    i
                }
            };
            if let Some(i) = i
                && best.is_none_or(|b| i < b || (i == b && d.len() > blen))
            {
                best = Some(i);
                blen = d.len();
                brow = *row;
            }
        }
        let Some(b) = best else { break };
        cell(&mut line, last.min(t.len()), b.min(t.len()));
        if brow {
            if !(ig && line.is_empty()) {
                grid.push(std::mem::take(&mut line));
            } else {
                line.clear();
            }
        }
        pos = b + blen;
        last = pos;
    }
    cell(&mut line, last.min(t.len()), t.len());
    if !(ig && line.is_empty()) {
        grid.push(line);
    }
    if grid.is_empty() {
        return err(ErrorCode::Calc);
    }
    if grid.len() > SPREAD_MAX {
        return Err(Stop::Throw(Throw::Range));
    }
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return err(ErrorCode::Calc);
    }
    let (r, w) = (u32::try_from(grid.len()).unwrap_or(u32::MAX), u32::try_from(width).unwrap_or(u32::MAX));
    if crate::value::array_too_big(r, w) {
        return Err(Stop::Throw(Throw::Range));
    }
    let mut data = Vec::with_capacity(grid.len() * width);
    for row in &grid {
        for k in 0..width {
            data.push(match row.get(k) {
                Some(&(x, y)) => Scalar::Str(ctx.new_text(JsStr::Utf16(&t[x..y.max(x)]))?),
                None => pad,
            });
        }
    }
    Ok(Value::Array(ArrayValue::new(r, w, data)))
}

fn valuetotext(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let fmt = match scalar(a, 1) {
        None => 0.0,
        Some(Scalar::Err(e)) => return err(e),
        f => opt_int(ctx, f, 0.0)?,
    };
    if fmt != 0.0 && fmt != 1.0 {
        return err(ErrorCode::Value);
    }
    let mut out = Vec::new();
    value_text(ctx, sv(a, 0), fmt == 1.0, &mut out);
    put(ctx, &out)
}

fn arraytotext(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let fmt = match scalar(a, 1) {
        None => 0.0,
        f => opt_int(ctx, f, 0.0)?,
    };
    if fmt != 0.0 && fmt != 1.0 {
        return err(ErrorCode::Value);
    }
    let v = arg(a, 0).unwrap_or(&super::shared::EMPTY);
    if let Value::Scalar(Scalar::Err(e)) = v {
        return err(*e);
    }
    let x = as_array(v, ctx)?;
    let mut out: Vec<u16> = Vec::new();
    // the text past the limit is never shown (`checkLen`): stop building it
    let over = |out: &Vec<u16>| out.len() > MAX_TEXT;
    if fmt == 0.0 {
        let sep: Vec<u16> = format!("{} ", list_sep(ctx.locale())).encode_utf16().collect();
        for (k, &s) in x.data.iter().enumerate() {
            if k > 0 {
                out.extend_from_slice(&sep);
            }
            value_text(ctx, s, false, &mut out);
            if over(&out) {
                return err(ErrorCode::Value);
            }
        }
        return put_checked(ctx, &out);
    }
    let (cs, rs) = array_seps(ctx.locale());
    out.push(c(b'{'));
    for r in 0..x.rows {
        if r > 0 {
            out.extend(rs.encode_utf16());
        }
        for col in 0..x.cols {
            if col > 0 {
                out.extend(cs.encode_utf16());
            }
            value_text(ctx, x.get(r, col), true, &mut out);
            if over(&out) {
                return err(ErrorCode::Value);
            }
        }
    }
    out.push(c(b'}'));
    put_checked(ctx, &out)
}

fn width_map(a: &CallArgs<'_>, ctx: &mut dyn FnContext, full: bool) -> R<Value> {
    let dbcs = is_dbcs(ctx.locale());
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        with_u16(t.as_js(), |s| {
            for &x in s {
                out.push(if !dbcs {
                    x
                } else if full {
                    if (0x21..=0x7E).contains(&x) {
                        x + 0xFEE0
                    } else if x == 0x20 {
                        0x3000
                    } else {
                        x
                    }
                } else if (0xFF01..=0xFF5E).contains(&x) {
                    x - 0xFEE0
                } else if x == 0x3000 {
                    0x20
                } else {
                    x
                });
            }
        });
    }
    put(ctx, out.as_slice())
}

fn asc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    width_map(a, ctx, false)
}
fn dbcs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    width_map(a, ctx, true)
}

fn bahttext(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let x = num(ctx, sv(a, 0))?;
    let (_, body) = fixed_digits(x.abs(), 2.0, &[c(b'.')], None);
    let dot = body.iter().position(|&u| u == c(b'.')).unwrap_or(body.len());
    let to_n = |s: &[u16]| uo_jsnum::parse::to_number(&String::from_utf16_lossy(s));
    let baht = to_n(&body[..dot]);
    let satang = to_n(body.get(dot + 1..).unwrap_or(&[]));
    if baht == 0.0 && satang == 0.0 {
        return put_str(ctx, "ศูนย์บาทถ้วน");
    }
    const TH_DIGIT: [&str; 10] = ["", "หนึ่ง", "สอง", "สาม", "สี่", "ห้า", "หก", "เจ็ด", "แปด", "เก้า"];
    let mut out = String::new();
    if baht > 0.0 {
        let groups = regional::million_groups(baht)?;
        for (i, &g) in groups.iter().enumerate() {
            regional::thai_below(&mut out, g, i > 0, &TH_DIGIT);
            if i + 1 < groups.len() {
                out.push_str("ล้าน");
            }
        }
        out.push_str("บาท");
    }
    if satang > 0.0 {
        regional::thai_below(&mut out, satang, false, &TH_DIGIT);
        out.push_str("สตางค์");
    } else {
        out.push_str("ถ้วน");
    }
    let s = if x < 0.0 { format!("ลบ{out}") } else { out };
    put_str(ctx, &s)
}

fn phonetic(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    err(ErrorCode::NA)
}

fn encodeurl(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut out = Buf::new();
    {
        let t = txt(ctx, sv(a, 0))?;
        // `new TextEncoder().encode(s)`: UTF-8, a lone surrogate as U+FFFD
        let s = t.as_js().to_string_lossy();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                out.push(u16::from(b));
            } else {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.extend(&[c(b'%'), u16::from(HEX[usize::from(b >> 4)]), u16::from(HEX[usize::from(b & 15)])]);
            }
            if out.len() > MAX_TEXT {
                break;
            }
        }
    }
    put_checked(ctx, out.as_slice())
}

fn webservice(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    err(ErrorCode::Value)
}

fn connect(_: &CallArgs<'_>, _: &mut dyn FnContext) -> R<Value> {
    err(ErrorCode::Connect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(p: &str, h: &str) -> Option<usize> {
        wild_search(&js::u(p), &js::u(h))
    }

    #[test]
    fn search_wildcards() {
        assert_eq!(w("b*d", "abcd"), Some(1));
        assert_eq!(w("?c", "abcd"), Some(1));
        assert_eq!(w("~*", "a*b"), Some(1));
        assert_eq!(w("B", "abc"), Some(1));
        assert_eq!(w("x*y", "xaxbz"), None);
        assert_eq!(w("~", "abc"), Some(0));
        assert_eq!(w("a*c*e", "xxabcde"), Some(2));
    }

    #[test]
    fn upper_keeps_expansions() {
        let mut b = Buf::new();
        upper_into(&js::u("straße ﬁ"), &mut b);
        assert_eq!(String::from_utf16_lossy(b.as_slice()), "STRAßE ﬁ");
        let mut b = Buf::new();
        proper_into(&js::u("o'neil 2nd x_y"), &mut b);
        assert_eq!(String::from_utf16_lossy(b.as_slice()), "O'Neil 2Nd X_Y");
    }
}
