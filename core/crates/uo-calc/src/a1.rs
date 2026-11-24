//! A1/R1C1 addresses and area helpers (`a1.ts`): text ↔ 0-based indices, area normalization, intersection and
//! the bounding area, the relative wrap of R1C1 and the quoting of sheet names. No evaluator dependency: used by
//! the parser, the printer, the evaluator and the reference functions (A4: `ADDRESS`, `INDIRECT`…).
//!
//! The `RegExp`s of the TS are ASCII matchers here (`[A-Za-z]`, `\d` and the ASCII `i` flag of a non-`u` pattern
//! are ASCII-only in JS); `quoteSheetName`'s `\p{L}`/`\p{N}` come from `uo_text::props`, decoded by code point as
//! the `u` flag does.

use crate::key::{Area, MAX_COL, MAX_ROW, SheetIx};
use crate::strings::{JsStr, JsString};

const fn is_ascii_alpha(u: u16) -> bool {
    matches!(u, 0x41..=0x5a | 0x61..=0x7a)
}

const fn is_ascii_digit(u: u16) -> bool {
    matches!(u, 0x30..=0x39)
}

/// `colFromLetters`: `"A"` → 0, `"XFD"` → 16383; `None` unless 1–3 ASCII letters naming a column ≤ XFD.
#[must_use]
pub fn col_from_letters(s: &[u16]) -> Option<u32> {
    if s.is_empty() || s.len() > 3 || !s.iter().all(|&u| is_ascii_alpha(u)) {
        return None;
    }
    let mut c: u32 = 0;
    for &u in s {
        c = c * 26 + u32::from((u & !0x20) - 64);
    }
    (c - 1 <= MAX_COL).then_some(c - 1)
}

/// Appends `colToLetters(col)` (`0` → `A`, `16383` → `XFD`).
pub fn push_col_letters(out: &mut String, col: u32) {
    let mut buf = [0u8; 8];
    let mut n = 0;
    let mut c = u64::from(col) + 1;
    while c > 0 {
        let r = (c - 1) % 26;
        buf[n] = b'A' + r as u8;
        n += 1;
        c = (c - 1) / 26;
    }
    for &b in buf[..n].iter().rev() {
        out.push(b as char);
    }
}

/// `colToLetters(col)`.
#[must_use]
pub fn col_to_letters(col: u32) -> String {
    let mut s = String::new();
    push_col_letters(&mut s, col);
    s
}

/// `cellToA1(row, col, rowAbs, colAbs)`: `(0, 0)` → `A1`, with optional `$`.
#[must_use]
pub fn cell_to_a1(row: u32, col: u32, row_abs: bool, col_abs: bool) -> String {
    let mut s = String::new();
    if col_abs {
        s.push('$');
    }
    push_col_letters(&mut s, col);
    if row_abs {
        s.push('$');
    }
    s.push_str(&(u64::from(row) + 1).to_string());
    s
}

use crate::jstext::digits_value as digits_number;

/// `a1ToCell`: `"A1"`/`"$B$2"` → `(row, col)` 0-based (`/^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/`, row 1..=1048576).
#[must_use]
pub fn a1_to_cell(s: JsStr<'_>) -> Option<(u32, u32)> {
    crate::jstext::with_u16(s, a1_to_cell_units)
}

fn a1_to_cell_units(s: &[u16]) -> Option<(u32, u32)> {
    let mut i = usize::from(s.first() == Some(&u16::from(b'$')));
    let l0 = i;
    while i < s.len() && is_ascii_alpha(s[i]) {
        i += 1;
    }
    let letters = &s[l0..i];
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    if s.get(i) == Some(&u16::from(b'$')) {
        i += 1;
    }
    let d0 = i;
    while i < s.len() && is_ascii_digit(s[i]) {
        i += 1;
    }
    let digits = &s[d0..i];
    if i != s.len() || digits.is_empty() || digits.len() > 7 {
        return None;
    }
    let col = col_from_letters(letters)?;
    let row = digits_number(digits) - 1.0;
    if !(0.0..=f64::from(MAX_ROW)).contains(&row) {
        return None;
    }
    Some((crate::cast::to_u32(row), col))
}

/// `normArea`: sorts the corners.
#[must_use]
pub fn norm_area(a: Area) -> Area {
    Area { sheet: a.sheet, r1: a.r1.min(a.r2), c1: a.c1.min(a.c2), r2: a.r1.max(a.r2), c2: a.c1.max(a.c2) }
}

/// `/^\$?\d+$/` then `+a.replace('$', '') - 1`, or NaN.
fn whole_row(s: &[u16]) -> f64 {
    let d = if s.first() == Some(&u16::from(b'$')) { &s[1..] } else { s };
    if d.is_empty() || !d.iter().all(|&u| is_ascii_digit(u)) {
        return f64::NAN;
    }
    digits_number(d) - 1.0
}

/// `a1ToArea(s, sheet)`: an A1 area without a sheet (`"A1:B3"`, `"A:A"`, `"1:1"`, `"C5"`) on `sheet`.
#[must_use]
pub fn a1_to_area(s: JsStr<'_>, sheet: SheetIx) -> Option<Area> {
    crate::jstext::with_u16(s, |s| {
        // `const [a, b = a] = s.split(':')`: the first two parts
        let colon = u16::from(b':');
        let mut parts = s.split(|&u| u == colon);
        let a = parts.next().unwrap_or(&[]);
        let b = parts.next().unwrap_or(a);
        let has_colon = s.contains(&colon);
        if let (Some(p), Some(q)) = (a1_to_cell_units(a), a1_to_cell_units(b)) {
            return Some(norm_area(Area { sheet, r1: p.0, c1: p.1, r2: q.0, c2: q.1 }));
        }
        let strip = |x: &[u16]| -> Vec<u16> { x.iter().copied().filter(|&u| u != u16::from(b'$')).collect() };
        if has_colon && let (Some(c1), Some(c2)) = (col_from_letters(&strip(a)), col_from_letters(&strip(b))) {
            return Some(norm_area(Area { sheet, r1: 0, c1, r2: MAX_ROW, c2 }));
        }
        let (r1, r2) = (whole_row(a), whole_row(b));
        let max = f64::from(MAX_ROW);
        if has_colon && r1 >= 0.0 && r2 >= 0.0 && r1 <= max && r2 <= max {
            return Some(norm_area(Area { sheet, r1: crate::cast::to_u32(r1), c1: 0, r2: crate::cast::to_u32(r2), c2: MAX_COL }));
        }
        None
    })
}

/// `areaToA1`: `"A1"`, `"A1:B3"`, `"A:A"`, `"1:3"`.
#[must_use]
pub fn area_to_a1(a: Area) -> String {
    if a.r1 == 0 && a.r2 == MAX_ROW && !(a.c1 == 0 && a.c2 == MAX_COL) {
        return format!("{}:{}", col_to_letters(a.c1), col_to_letters(a.c2));
    }
    if a.c1 == 0 && a.c2 == MAX_COL && !(a.r1 == 0 && a.r2 == MAX_ROW) {
        return format!("{}:{}", u64::from(a.r1) + 1, u64::from(a.r2) + 1);
    }
    let tl = cell_to_a1(a.r1, a.c1, false, false);
    if a.r1 == a.r2 && a.c1 == a.c2 { tl } else { format!("{tl}:{}", cell_to_a1(a.r2, a.c2, false, false)) }
}

/// `areaRows`.
#[must_use]
pub const fn area_rows(a: Area) -> u32 {
    a.r2 - a.r1 + 1
}

/// `areaCols`.
#[must_use]
pub const fn area_cols(a: Area) -> u32 {
    a.c2 - a.c1 + 1
}

/// `areaContains`.
#[must_use]
pub fn area_contains(a: Area, sheet: SheetIx, row: u32, col: u32) -> bool {
    a.sheet == sheet && row >= a.r1 && row <= a.r2 && col >= a.c1 && col <= a.c2
}

/// `intersectAreas`: the common part of two areas of the same sheet.
#[must_use]
pub fn intersect_areas(a: Area, b: Area) -> Option<Area> {
    if a.sheet != b.sheet {
        return None;
    }
    let (r1, r2) = (a.r1.max(b.r1), a.r2.min(b.r2));
    let (c1, c2) = (a.c1.max(b.c1), a.c2.min(b.c2));
    (r1 <= r2 && c1 <= c2).then_some(Area { sheet: a.sheet, r1, c1, r2, c2 })
}

/// `areasOverlap`.
#[must_use]
pub fn areas_overlap(a: Area, b: Area) -> bool {
    a.sheet == b.sheet && a.r1 <= b.r2 && b.r1 <= a.r2 && a.c1 <= b.c2 && b.c1 <= a.c2
}

/// `boundingArea` (the `:` operator between references): the sheet of `a`.
#[must_use]
pub fn bounding_area(a: Area, b: Area) -> Area {
    Area { sheet: a.sheet, r1: a.r1.min(b.r1), c1: a.c1.min(b.c1), r2: a.r2.max(b.r2), c2: a.c2.max(b.c2) }
}

/// `wrapRow(r)` = `((r % n) + n) % n` with `n = MAX_ROW + 1` on doubles (R[-1] on row 1 = the last row).
#[must_use]
pub fn wrap_row(r: f64) -> f64 {
    let n = f64::from(MAX_ROW) + 1.0;
    ((r % n) + n) % n
}

/// `wrapCol(c)`.
#[must_use]
pub fn wrap_col(c: f64) -> f64 {
    let n = f64::from(MAX_COL) + 1.0;
    ((c % n) + n) % n
}

/// JS `\s` (`WhiteSpace` + `LineTerminator`), the set of `RegExp` classes.
#[must_use]
pub const fn is_js_space(u: u16) -> bool {
    matches!(u, 0x09..=0x0d | 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
}

/// `quoteSheetName(name) !== name`: the name must go between single quotes (white space, punctuation, starts
/// with a digit, looks like an A1/R1C1 reference or a boolean).
#[must_use]
pub fn sheet_name_needs_quote(name: JsStr<'_>) -> bool {
    crate::jstext::with_u16(name, |s| !plain_sheet_name(s))
}

fn plain_sheet_name(s: &[u16]) -> bool {
    // /^[\p{L}_\\][\p{L}\p{N}_.]*$/u (by code point; a lone surrogate is neither a letter nor a number)
    let mut first = true;
    for (cp, _) in CodePoints::new(s) {
        let ok = if first {
            cp == u32::from(b'_') || cp == u32::from(b'\\') || uo_text::props::is_letter(cp)
        } else {
            cp == u32::from(b'_') || cp == u32::from(b'.') || uo_text::props::is_letter(cp) || uo_text::props::is_number(cp)
        };
        if !ok {
            return false;
        }
        first = false;
    }
    if first {
        return false;
    }
    // !/^[A-Za-z]{1,3}\d+$/
    let letters = s.iter().take_while(|&&u| is_ascii_alpha(u)).count();
    if (1..=3).contains(&letters) && s.len() > letters && s[letters..].iter().all(|&u| is_ascii_digit(u)) {
        return false;
    }
    // !/^(R\d*C\d*|R\d*|C\d*)$/i
    if looks_r1c1(s) {
        return false;
    }
    // !/^(TRUE|FALSE)$/i
    !(ascii_ci_eq(s, b"TRUE") || ascii_ci_eq(s, b"FALSE"))
}

fn looks_r1c1(s: &[u16]) -> bool {
    let is = |u: u16, c: u8| u == u16::from(c) || u == u16::from(c.to_ascii_lowercase());
    let digits = |from: usize| from + s[from..].iter().take_while(|&&u| is_ascii_digit(u)).count();
    if s.first().is_some_and(|&u| is(u, b'R')) {
        let i = digits(1);
        if i == s.len() {
            return true;
        }
        if is(s[i], b'C') {
            return digits(i + 1) == s.len();
        }
        return false;
    }
    s.first().is_some_and(|&u| is(u, b'C')) && digits(1) == s.len()
}

/// ASCII case-insensitive equality with an upper-case ASCII word (a non-`u` `RegExp` `i` over ASCII letters).
#[must_use]
pub fn ascii_ci_eq(s: &[u16], word: &[u8]) -> bool {
    s.len() == word.len() && s.iter().zip(word).all(|(&u, &w)| u == u16::from(w) || u == u16::from(w.to_ascii_lowercase()))
}

/// `quoteSheetName(name)`: the name, or `'name'` with `'` doubled.
#[must_use]
pub fn quote_sheet_name(name: JsStr<'_>) -> JsString {
    if !sheet_name_needs_quote(name) {
        return name.to_owned_js();
    }
    let mut u = vec![u16::from(b'\'')];
    for x in name.units() {
        u.push(x);
        if x == u16::from(b'\'') {
            u.push(x);
        }
    }
    u.push(u16::from(b'\''));
    JsString::from_utf16(&u)
}

/// Code points of UTF-16 units with their start index; a lone surrogate is its own code point (the `u` flag).
pub(crate) struct CodePoints<'a> {
    s: &'a [u16],
    i: usize,
}

impl<'a> CodePoints<'a> {
    pub(crate) const fn new(s: &'a [u16]) -> Self {
        Self { s, i: 0 }
    }
}

impl Iterator for CodePoints<'_> {
    type Item = (u32, usize);
    fn next(&mut self) -> Option<(u32, usize)> {
        let (cp, n) = code_point_at(self.s, self.i)?;
        let at = self.i;
        self.i += n;
        Some((cp, at))
    }
}

/// The code point at `i` and its length in units (a lone surrogate is one code point of one unit).
#[must_use]
pub(crate) fn code_point_at(s: &[u16], i: usize) -> Option<(u32, usize)> {
    let u = *s.get(i)?;
    if (0xd800..0xdc00).contains(&u)
        && let Some(&v) = s.get(i + 1)
        && (0xdc00..0xe000).contains(&v)
    {
        return Some((0x10000 + ((u32::from(u) - 0xd800) << 10) + (u32::from(v) - 0xdc00), 2));
    }
    Some((u32::from(u), 1))
}
