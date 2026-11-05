//! docx-core `table-sort.ts` (D2b-F3): Sort (table rows and paragraphs) as Word 365 pt-BR (measured
//! in `write/tools/table-lab`, experiments `sort-*`):
//!
//! - text: the Windows collation (`uo_text::collation`, the editor passes `compareText` of
//!   `@universal-office/collation`); without case sensitivity the equal ones keep the original
//!   order (stable); with it the lower case comes first ("a" < "A" < "á" < "Á"); empty first;
//! - number: the number at the start of the text ("2 itens" = 2, "R$ 3" = 3, "(4)" = −4, "5%" =
//!   0.05, "1.000" = 1000); what is not a number comes BEFORE when ascending and AFTER when
//!   descending, always in the original order;
//! - date: dd/mm/yyyy, d/m/yy (yy → 1930–2029) and yyyy-mm-dd; "1 de março de 2020" is no date;
//! - descending = the inverted comparison, stable (empty last);
//! - the row takes all its formatting; the header row stays; a table with merged cells is refused.
//!
//! Regex sites (hand matchers; V8 rows in `js/regex-sites/d2b-f3.mjs`): the two date patterns
//! `/^(\d{4})-(\d{1,2})-(\d{1,2})$/` and `/^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2}|\d{4})$/`.
//! Numbers: `Date.UTC` (years 0–99 are 1900–1999, as JS).

use std::cmp::Ordering;

use uo_jsnum::math::{floor, max};
use uo_jsnum::parse::to_number;
use uo_jsnum::text::is_js_space_unit;
use uo_text::collation::CollationKey;

use super::formula::{cell_text, parse_cell_number};
use super::table_ops::{map_table, span_of};
use crate::model::para::Paragraph;
use crate::model::table::Table;
use crate::word::obj;

/// Word's message for a table with merged cells.
pub const SORT_MERGED_ERROR_EN: &str = "Cannot sort a table that contains merged cells.";
/// The same in Portuguese.
pub const SORT_MERGED_ERROR_PT: &str = "Não é possível classificar uma tabela que contém células mescladas.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortType {
    Text,
    Number,
    Date,
}

/// `SortKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    /// the grid column (table) or the field (paragraphs with a separator; 0 = the whole paragraph)
    pub col: i64,
    pub ty: SortType,
    pub desc: bool,
}

/// The field separator of the paragraphs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SortSeparator {
    #[default]
    Tab,
    Comma,
    Char(Vec<u16>),
}

/// `SortOptions` (the text comparison is the Windows collation).
#[derive(Debug, Clone, Default)]
pub struct SortOptions {
    pub keys: Vec<SortKey>,
    pub case_sensitive: bool,
    pub separator: SortSeparator,
}

fn all_digits(t: &[u16]) -> bool {
    !t.is_empty() && t.iter().all(|u| (0x30..=0x39).contains(u))
}

fn num_of(t: &[u16]) -> f64 {
    to_number(&String::from_utf16_lossy(t))
}

fn trim_units(t: &[u16]) -> &[u16] {
    let a = t.iter().position(|u| !is_js_space_unit(*u)).unwrap_or(t.len());
    let b = t.iter().rposition(|u| !is_js_space_unit(*u)).map_or(a, |p| p + 1);
    t.get(a..b.max(a)).unwrap_or(&[])
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant's algorithm over the proleptic Gregorian calendar (`m` 1..=12)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 => 28 + i64::from(is_leap(y)),
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `parseSortDate(text)`: day, month, year as a comparable number (`Date.UTC`); `None` = no date.
#[must_use]
pub fn parse_sort_date(text: &[u16]) -> Option<f64> {
    let s = trim_units(text);
    let (d, m, mut y): (i64, i64, i64);
    let sep = |u: u16| u == u16::from(b'/') || u == u16::from(b'.') || u == u16::from(b'-');
    // /^(\d{4})-(\d{1,2})-(\d{1,2})$/
    let iso = (|| {
        let dash = u16::from(b'-');
        if s.len() < 8 || !all_digits(s.get(..4)?) || *s.get(4)? != dash {
            return None;
        }
        let rest = s.get(5..)?;
        let p = rest.iter().position(|u| *u == dash)?;
        let (mm, dd) = (rest.get(..p)?, rest.get(p + 1..)?);
        ((1..=2).contains(&mm.len()) && all_digits(mm) && (1..=2).contains(&dd.len()) && all_digits(dd))
            .then(|| (num_of(s.get(..4).unwrap_or(&[])), num_of(mm), num_of(dd)))
    })();
    if let Some((yy, mm, dd)) = iso {
        (y, m, d) = (as_i(yy), as_i(mm), as_i(dd));
    } else {
        // /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2}|\d{4})$/
        let p1 = s.iter().position(|u| sep(*u))?;
        let rest = s.get(p1 + 1..)?;
        let p2 = rest.iter().position(|u| sep(*u))?;
        let (a, b, c) = (s.get(..p1)?, rest.get(..p2)?, rest.get(p2 + 1..)?);
        if !((1..=2).contains(&a.len())
            && all_digits(a)
            && (1..=2).contains(&b.len())
            && all_digits(b)
            && (c.len() == 2 || c.len() == 4)
            && all_digits(c))
        {
            return None;
        }
        (d, m, y) = (as_i(num_of(a)), as_i(num_of(b)), as_i(num_of(c)));
        if c.len() == 2 {
            y += if y < 30 { 2000 } else { 1900 };
        }
    }
    // `Date.UTC` reads the years 0–99 as 1900–1999
    let uy = if (0..=99).contains(&y) { y + 1900 } else { y };
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(uy, m) {
        return None;
    }
    let days = days_from_civil(uy, m, d);
    Some(f64::from(i32::try_from(days).ok()?) * 86_400_000.0)
}

/// A small non-negative integer of the model's own digits.
fn as_i(x: f64) -> i64 {
    i64::from(uo_jsnum::conv::to_int32(floor(max(0.0, x))))
}

/// `parseSortNumber(text)`: the number at the start (`5%` = 0.05); `None` = no number.
#[must_use]
pub fn parse_sort_number(text: &[u16]) -> Option<f64> {
    let x = parse_cell_number(text)?;
    Some(if x.suffix.as_deref() == Some("%") { x.n / 100.0 } else { x.n })
}

/// `Array.prototype.sort` order of the items by the keys, stable. `fields[i][k]` = the text of key
/// `k` of item `i`.
fn order(fields: &[Vec<Vec<u16>>], o: &SortOptions) -> Vec<usize> {
    struct Val {
        s: Vec<u16>,
        v: Option<f64>,
        /// the collation key of a text key (built once: the TS compares texts pairwise)
        key: Option<CollationKey>,
        empty: bool,
    }
    let vals: Vec<Vec<Val>> = fields
        .iter()
        .map(|f| {
            f.iter()
                .zip(&o.keys)
                .map(|(t, k)| {
                    let empty = trim_units(t).is_empty();
                    Val {
                        s: t.clone(),
                        v: match k.ty {
                            SortType::Number => parse_sort_number(t),
                            SortType::Date => parse_sort_date(t),
                            SortType::Text => None,
                        },
                        key: (k.ty == SortType::Text && !empty).then(|| CollationKey::new(t)),
                        empty,
                    }
                })
                .collect()
        })
        .collect();
    let one = |a: &Val, b: &Val, key: &SortKey| -> f64 {
        if key.ty == SortType::Text {
            let mut c: f64 = if a.empty || b.empty {
                if a.empty == b.empty {
                    0.0
                } else if a.empty {
                    -1.0
                } else {
                    1.0
                }
            } else if a.s == b.s {
                0.0
            } else {
                match (&a.key, &b.key) {
                    (Some(x), Some(y)) => ord(x.compare(y)),
                    _ => 0.0,
                }
            };
            if c == 0.0 && o.case_sensitive {
                c = case_compare(&a.s, &b.s);
            }
            return if key.desc { -c } else { c };
        }
        // what is no number or date: before when ascending, after when descending
        let (Some(x), Some(y)) = (a.v, b.v) else {
            if a.v.is_none() && b.v.is_none() {
                return 0.0;
            }
            return if a.v.is_none() != key.desc { -1.0 } else { 1.0 };
        };
        let c = if x == y {
            0.0
        } else if x < y {
            -1.0
        } else {
            1.0
        };
        if key.desc { -c } else { c }
    };
    let mut idx: Vec<usize> = (0..fields.len()).collect();
    obj::js_sort(&mut idx, |i, j| {
        let (Some(vi), Some(vj)) = (vals.get(*i), vals.get(*j)) else { return 0.0 };
        for (k, key) in o.keys.iter().enumerate() {
            if let (Some(a), Some(b)) = (vi.get(k), vj.get(k)) {
                let c = one(a, b, key);
                if c != 0.0 {
                    return c;
                }
            }
        }
        // the tie: the original order (`i - j`)
        f64::from(u32::try_from(*i).unwrap_or(u32::MAX)) - f64::from(u32::try_from(*j).unwrap_or(u32::MAX))
    });
    idx
}

fn ord(o: Ordering) -> f64 {
    match o {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

/// `caseCompare(a, b)`: the case tie-break (case sensitive): the lower case first, unit by unit.
fn case_compare(a: &[u16], b: &[u16]) -> f64 {
    for (x, y) in a.iter().zip(b) {
        if x == y {
            continue;
        }
        let (xs, ys) = (std::slice::from_ref(x), std::slice::from_ref(y));
        let lower = |u: &[u16]| uo_text::case::to_lowercase(u).into_owned();
        let upper = |u: &[u16]| uo_text::case::to_uppercase(u).into_owned();
        let xl = lower(xs) == xs && upper(xs) != xs;
        let yl = lower(ys) == ys && upper(ys) != ys;
        if xl != yl && lower(xs) == lower(ys) {
            return if xl { -1.0 } else { 1.0 };
        }
    }
    0.0
}

/// `canSortTable(t)`: no merged cells.
#[must_use]
pub fn can_sort_table(t: &Table) -> bool {
    t.rows.iter().all(|row| {
        row.cells.iter().all(|c| {
            span_of(c) == 1
                && c.tc_pr
                    .as_deref()
                    .is_none_or(|p| p.v_merge.as_deref().is_none_or(str::is_empty) && p.h_merge.as_deref().is_none_or(str::is_empty))
        })
    })
}

/// `sortTable(t, r1, r2, o)`: sorts rows `r1..r2` (whole rows, with their formatting); `false` =
/// merged cells or nothing to do.
pub fn sort_table(t: &mut Table, r1: i64, r2: i64, o: &SortOptions) -> bool {
    if !can_sort_table(t) || o.keys.is_empty() {
        return false;
    }
    // `t.rows.slice(r1, r2 + 1)`: a range past the rows is clamped, an empty one sorts nothing
    let a = usize::try_from(r1.max(0)).unwrap_or(usize::MAX);
    let b = usize::try_from(r2).map_or(0, |b| b.min(t.rows.len().saturating_sub(1)));
    if a > b || t.rows.is_empty() {
        return true;
    }
    let m = map_table(t);
    let fields: Vec<Vec<Vec<u16>>> = (a..=b)
        .map(|ri| {
            o.keys
                .iter()
                .map(|k| {
                    let g = m.rows.get(ri).and_then(|row| row.iter().find(|x| x.col <= k.col && k.col < x.col + x.span));
                    g.and_then(|g| t.rows.get(g.ri).and_then(|r| r.cells.get(g.ci)))
                        .map(|c| cell_text(c).into_iter().map(|u| if u == 10 { 32 } else { u }).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    let idx = order(&fields, o);
    let mut slots: Vec<Option<_>> = t.rows.drain(a..=b).map(Some).collect();
    let sorted: Vec<_> = idx.iter().filter_map(|i| slots.get_mut(*i).and_then(Option::take)).collect();
    let tail = t.rows.split_off(a);
    t.rows.extend(sorted);
    t.rows.extend(tail);
    true
}

fn plain(p: &Paragraph) -> Vec<u16> {
    p.runs.iter().flat_map(|r| r.text.units().iter().copied()).collect()
}

/// `sortParagraphsOrder(paras, o)`: the order of the paragraphs (indices) by the keys; field 0 =
/// the whole paragraph.
#[must_use]
pub fn sort_paragraphs_order(paras: &[&Paragraph], o: &SortOptions) -> Vec<usize> {
    let ch: Vec<u16> = match &o.separator {
        SortSeparator::Tab => vec![9],
        SortSeparator::Comma => vec![u16::from(b',')],
        SortSeparator::Char(c) if !c.is_empty() => c.clone(),
        SortSeparator::Char(_) => vec![9],
    };
    let fields: Vec<Vec<Vec<u16>>> = paras
        .iter()
        .map(|p| {
            let s = plain(p);
            let parts = split(&s, &ch);
            o.keys
                .iter()
                .map(|k| {
                    if k.col <= 0 {
                        s.clone()
                    } else {
                        usize::try_from(k.col - 1).ok().and_then(|i| parts.get(i)).map(|x| trim_units(x).to_vec()).unwrap_or_default()
                    }
                })
                .collect()
        })
        .collect();
    order(&fields, o)
}

fn split(text: &[u16], pat: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);
    while i + pat.len() <= text.len() {
        if text.get(i..i + pat.len()) == Some(pat) {
            out.push(text.get(start..i).map(<[u16]>::to_vec).unwrap_or_default());
            i += pat.len();
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(text.get(start..).map(<[u16]>::to_vec).unwrap_or_default());
    out
}
