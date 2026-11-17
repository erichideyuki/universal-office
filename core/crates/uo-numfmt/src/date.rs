//! The date/time section of `numfmt.ts` (`emitDate`, `serialToDate`) and the UTC calendar it
//! reads through `Date`. Every `Date` use of the TS is UTC (`Date.UTC`, `getUTC*`): the host time
//! zone never matters, so there is no time-zone input.

use uo_jsnum::format::{to_js_string, to_precision};
use uo_jsnum::math;
use uo_jsnum::parse::to_number;
use uo_text::case::{to_lowercase, to_uppercase};

use crate::digits::decimal_digits;
use crate::emit::{FmtPart, push_decor};
use crate::locale::{FormatLocale, lang_names, pt_br};
use crate::parse::{Section, Tok, U, parse_section, u};

/// Last valid serial of Excel (31/12/9999).
const MAX_SERIAL: f64 = 2_958_465.0;

/// Days from 1970-01-01 to the civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month 0-11, day 1-31) of a count of days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m - 1, d)
}

/// `Date.getUTCDay()` of a count of days since 1970-01-01.
fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// `serialToDate(serial).getTime()`: the OLE serial (days since 1899-12-30) as a UTC epoch time
/// value in milliseconds; NaN where the TS `Date` would be invalid (`|t| > 8.64e15`, or a NaN serial).
/// `Date.UTC(1899, 11, 30)` is -2209161600000.
#[must_use]
pub fn serial_to_date_ms(serial: f64) -> f64 {
    let ms = math::round(serial * 86_400_000.0);
    let t = -2_209_161_600_000.0 + ms;
    // TimeClip
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    math::trunc(t) + 0.0
}

/// The UTC fields of a `Date` time value (`getUTCFullYear` ...): `None` for NaN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtcFields {
    pub year: i64,
    /// 0-11
    pub month: i64,
    pub day: i64,
    /// 0 = Sunday
    pub weekday: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub millisecond: i64,
}

/// Splits a time value in the range of `Date` (|t| <= 8.64e15) into UTC fields.
#[must_use]
pub fn utc_fields(t: f64) -> Option<UtcFields> {
    if !t.is_finite() || t.abs() > 8.64e15 {
        return None;
    }
    let day_ms = 86_400_000.0;
    let days = math::floor(t / day_ms);
    let rem = t - days * day_ms; // 0 <= rem < 86_400_000
    let days = i64::from(uo_jsnum::conv::to_int32(days)); // |days| <= 1e8
    let rem = i64::from(uo_jsnum::conv::to_int32(rem));
    let (year, month, day) = civil_from_days(days);
    Some(UtcFields {
        year,
        month,
        day,
        weekday: weekday(days),
        hour: rem / 3_600_000,
        minute: (rem / 60_000) % 60,
        second: (rem / 1000) % 60,
        millisecond: rem % 1000,
    })
}

/// Names and separator of the locale a section formats with (`localeFor`).
struct Names<'a> {
    decimal: &'a str,
    months_short: &'a [String; 12],
    months_long: &'a [String; 12],
    days_short: &'a [String; 7],
    days_long: &'a [String; 7],
    ampm: &'a [String; 2],
}

fn names_of(loc: &FormatLocale) -> Names<'_> {
    Names {
        decimal: &loc.decimal,
        months_short: &loc.months_short,
        months_long: &loc.months_long,
        days_short: &loc.days_short,
        days_long: &loc.days_long,
        ampm: &loc.ampm,
    }
}

/// `localeFor(sec, loc)`: `{ ...loc, ...names, ampm: names.ampm ?? loc.ampm }`. For the pt entry the
/// TS spreads the `PT_BR` object itself, so every field comes from `PT_BR` (the decimal separator
/// too) unless `loc` IS `PT_BR` (identity).
fn locale_for(lcid: Option<u32>, loc: &FormatLocale) -> Names<'_> {
    let Some(l) = lcid else { return names_of(loc) };
    let id = l & 0x3ff;
    if id == 0x16 {
        let pt = pt_br();
        if std::ptr::eq(pt, loc) {
            return names_of(loc);
        }
        // `pt` is 'static; `loc` outlives `'a` by construction: both coerce to the shorter lifetime
        return Names {
            decimal: &pt.decimal,
            months_short: &pt.months_short,
            months_long: &pt.months_long,
            days_short: &pt.days_short,
            days_long: &pt.days_long,
            ampm: &pt.ampm,
        };
    }
    match lang_names(id) {
        None => names_of(loc),
        Some(n) => Names {
            decimal: &loc.decimal,
            months_short: &n.months_short,
            months_long: &n.months_long,
            days_short: &n.days_short,
            days_long: &n.days_long,
            ampm: n.ampm.as_ref().unwrap_or(&loc.ampm),
        },
    }
}

fn rank(c: u8) -> u8 {
    match c {
        b'h' => 3,
        b'm' => 2,
        _ => 1,
    }
}

fn is_hms(c: u8) -> bool {
    matches!(c, b'h' | b'm' | b's')
}

/// `String(v).padStart(n, '0')`
fn pad(v: f64, n: usize) -> String {
    format!("{:0>n$}", to_js_string(v))
}

fn text(s: &str) -> FmtPart {
    FmtPart::Text(u(s))
}

/// `None` = out of range (negative date or beyond 9999) -> "####".
pub(crate) fn emit_date(sec0: &Section, serial: f64, loc0: &FormatLocale, date1904: bool, neg: bool) -> Option<Vec<FmtPart>> {
    let replaced;
    let mut sec = sec0;
    if sec0.lcid == Some(0xf800) || sec0.lcid == Some(0xf400) {
        // long date / system time replace the whole section
        let sys: &str = if sec0.lcid == Some(0xf800) {
            loc0.sys_date.as_deref().unwrap_or("dddd, mmmm d, yyyy")
        } else {
            loc0.sys_time.as_deref().unwrap_or("h:mm:ss AM/PM")
        };
        let mut s = parse_section(&u(sys));
        s.color.clone_from(&sec0.color);
        replaced = s;
        sec = &replaced;
    }
    let loc = locale_for(sec.lcid, loc0);
    // Elapsed time (measured in Excel): see numfmt.ts for the rules
    let els: Vec<(usize, u8, usize)> =
        sec.toks.iter().enumerate().filter_map(|(i, t)| if let Tok::El { c, n } = t { Some((i, *c, *n)) } else { None }).collect();
    let mut un: Option<u8> = None;
    if let Some(&(at, c0, _)) = els.first() {
        if els.len() > 1 {
            return None;
        }
        let mut uu = c0;
        for t in &sec.toks {
            if let Tok::Dt { c, .. } = t
                && is_hms(*c)
                && rank(*c) > rank(uu)
            {
                uu = *c;
            }
        }
        un = Some(uu);
        // common h or common part of the unit itself before the bracket -> invalid
        if sec.toks.iter().take(at).any(|t| matches!(t, Tok::Dt { c, .. } if *c == b'h' || *c == uu)) {
            return None;
        }
    }
    if neg {
        // negative: in 1904 any date/time gets "-"; in 1900 only if every date/time part is of the elapsed unit
        if !date1904 {
            let ok = un.is_some_and(|uu| {
                sec.toks.iter().all(|t| match t {
                    Tok::Dt { c, .. } | Tok::El { c, .. } => *c == uu,
                    Tok::AmPm { .. } | Tok::Subsec(_) => false,
                    _ => true,
                })
            });
            if !ok {
                return None;
            }
        }
        let mut r = emit_date(sec0, serial, loc0, date1904, false)?;
        r.insert(0, text("-"));
        return Some(r);
    }
    // precision: requested fraction of a second (.0/.00/.000) or whole second
    let mut sub = 0usize;
    for t in &sec.toks {
        if let Tok::Subsec(n) = t {
            sub = sub.max((*n).min(3));
        }
    }
    let unit = math::pow(10.0, sub as f64);
    // round the serial to the second (or the requested fraction) over 15 significant digits
    let (ticks_int, _) = decimal_digits(serial * 86_400.0 * unit, 0);
    let ticks = to_number(if ticks_int.is_empty() { "0" } else { &ticks_int });
    let day_n = math::floor(ticks / (86_400.0 * unit));
    let sod_ticks = ticks - day_n * 86_400.0 * unit;
    if day_n > MAX_SERIAL + if date1904 { -1462.0 } else { 0.0 } || serial < 0.0 {
        return None;
    }
    // calendar (the 1900 bug: 60 = 29/02/1900; serial 0 = 00/01/1900)
    let dn = i64::from(uo_jsnum::conv::to_int32(day_n)); // 0 <= day_n <= MAX_SERIAL here
    let (yr, mo, day, wd): (i64, i64, i64, i64);
    if date1904 {
        let days = days_from_civil(1904, 1, 1) + dn;
        (yr, mo, day) = civil_from_days(days);
        wd = weekday(days);
    } else if dn == 0 {
        (yr, mo, day, wd) = (1900, 0, 0, 6);
    } else if dn == 60 {
        (yr, mo, day, wd) = (1900, 1, 29, 3);
    } else {
        let days = days_from_civil(1899, 12, if dn < 60 { 31 } else { 30 }) + dn;
        (yr, mo, day) = civil_from_days(days);
        let w = weekday(days);
        wd = if dn < 60 { (w + 6) % 7 } else { w };
    }
    let sod = math::floor(sod_ticks / unit);
    let sub_val = sod_ticks - sod * unit;
    let h24 = math::floor(sod / 3600.0);
    let min = math::floor((sod % 3600.0) / 60.0);
    let sec2 = sod % 60.0;
    let has12 = sec.toks.iter().any(|t| matches!(t, Tok::AmPm { .. }));
    let h12 = if h24 % 12.0 == 0.0 { 12.0 } else { h24 % 12.0 };
    let total_sec = math::floor(ticks / unit);
    // register of the elapsed time: the value, or the remainder of the seconds rounding
    let mut from_value = true;
    let secs15 = to_precision(serial * 86_400.0 * unit, 15).map_or(f64::NAN, |s| to_number(&s));
    let rest = (secs15 + 0.5) % if sub > 0 { unit } else { 60.0 };
    let elapsed = |from_value: bool, c: u8| -> f64 {
        if from_value {
            return match c {
                b'h' => math::floor(total_sec / 3600.0),
                b'm' => math::floor(total_sec / 60.0),
                _ => total_sec,
            };
        }
        math::floor(
            rest * match c {
                b'h' => 24.0,
                b'm' => 1440.0,
                _ => 86_400.0,
            },
        )
    };
    let (yrf, mof, dayf) = (yr as f64, mo as f64, day as f64);
    let mut out: Vec<FmtPart> = Vec::new();
    for t0 in &sec.toks {
        let mut t = t0.clone();
        if let Some(uu) = un
            && let Tok::Dt { c, n } | Tok::El { c, n } = &t
            && is_hms(*c)
        {
            let (c, n, is_el) = (*c, *n, matches!(t, Tok::El { .. }));
            if c == uu {
                let width = if is_el {
                    n
                } else if n >= 2 {
                    2
                } else {
                    1
                };
                out.push(text(&pad(elapsed(from_value, uu), width)));
                continue;
            }
            if is_el {
                t = Tok::Dt { c, n }; // bracket of a smaller unit = common part
            }
        }
        if matches!(t, Tok::Dt { .. } | Tok::Subsec(_) | Tok::AmPm { .. }) {
            from_value = false;
        }
        match &t {
            Tok::Dt { c, n } => {
                let n = *n;
                match *c {
                    b'y' => out.push(text(&if n <= 2 { pad(yrf % 100.0, 2) } else { to_js_string(yrf) })),
                    b'e' => out.push(text(&to_js_string(yrf))),
                    b'g' => {} // era (Gregorian calendar: empty)
                    b'b' => out.push(text(&if n <= 2 { pad((yrf + 543.0) % 100.0, 2) } else { to_js_string(yrf + 543.0) })),
                    b'M' => {
                        let mi = usize::try_from(mo).unwrap_or(0);
                        let s: U = if n == 1 {
                            u(&to_js_string(mof + 1.0))
                        } else if n == 2 {
                            u(&pad(mof + 1.0, 2))
                        } else if n == 3 {
                            u(loc.months_short.get(mi).map_or("", String::as_str))
                        } else if n == 4 {
                            u(loc.months_long.get(mi).map_or("", String::as_str))
                        } else {
                            let l = u(loc.months_long.get(mi).map_or("", String::as_str));
                            l.get(..1).unwrap_or(&[]).to_vec()
                        };
                        out.push(FmtPart::Text(s));
                    }
                    b'm' => out.push(text(&if n >= 2 { pad(min, 2) } else { to_js_string(min) })),
                    b'd' => {
                        let wi = usize::try_from(wd).unwrap_or(0);
                        out.push(text(&if n == 1 {
                            to_js_string(dayf)
                        } else if n == 2 {
                            pad(dayf, 2)
                        } else if n == 3 {
                            loc.days_short.get(wi).cloned().unwrap_or_default()
                        } else {
                            loc.days_long.get(wi).cloned().unwrap_or_default()
                        }));
                    }
                    b'a' => {
                        let wi = usize::try_from(wd).unwrap_or(0);
                        out.push(text(if n == 3 {
                            loc.days_short.get(wi).map_or("", String::as_str)
                        } else {
                            loc.days_long.get(wi).map_or("", String::as_str)
                        }));
                    }
                    b'h' => {
                        let hv = if has12 { h12 } else { h24 };
                        out.push(text(&if n >= 2 { pad(hv, 2) } else { to_js_string(hv) }));
                    }
                    b's' => out.push(text(&if n >= 2 { pad(sec2, 2) } else { to_js_string(sec2) })),
                    _ => {}
                }
            }
            Tok::El { c, n } => {
                let val = match *c {
                    b'h' => math::floor(total_sec / 3600.0),
                    b'm' => math::floor(total_sec / 60.0),
                    _ => total_sec,
                };
                out.push(text(&pad(val, *n)));
            }
            Tok::Subsec(n) => {
                let digits = pad(sub_val, sub);
                let mut d: String = digits.chars().take(*n).collect();
                while d.chars().count() < *n {
                    d.push('0');
                }
                out.push(text(&format!("{}{d}", loc.decimal)));
            }
            Tok::AmPm { full, lower } => {
                let s = u(if h24 < 12.0 { &loc.ampm[0] } else { &loc.ampm[1] });
                if *full {
                    out.push(FmtPart::Text(s));
                } else {
                    // the first code unit; the TS throws a TypeError on an empty AM/PM name: nothing here
                    let first = s.get(..1).unwrap_or(&[]);
                    let cased = if *lower { to_lowercase(first) } else { to_uppercase(first) };
                    out.push(FmtPart::Text(cased.into_owned()));
                }
            }
            other => push_decor(other, &mut out),
        }
    }
    Some(out)
}
