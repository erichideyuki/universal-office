//! DATE/TIME fields (`docx-layout/src/datefmt.ts`): Word's date/time pictures (`\@ "…"`) and the
//! text formats (`\* Upper/Lower/FirstCap/Caps`).
//!
//! The TS reads the local getters of a `Date` (`getFullYear`, `getHours`…) in the process time
//! zone. Here the instant arrives as its local parts, [`LocalDateTime`], computed by the caller
//! from the JS time value and the offset of the host's time zone at that instant (LQ10: the
//! layout's `Clock` + `TimeZoneRule`). An invalid `Date` is [`LocalDateTime::INVALID`]: every
//! getter is NaN, as in the TS.
//!
//! Strings are UTF-16 (the picture and the instruction index code units: `picture[i]`, the
//! `(\S)` of the `Caps` format is one unit).

use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{floor, rem};
use uo_jsnum::text::is_js_space_unit;

use super::num::pad_num;
use crate::text::Utf16;

/// The local parts of an instant, as the `Date` getters return them (NaN for an invalid date).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalDateTime {
    /// `getFullYear()`
    pub year: f64,
    /// `getMonth()` (0–11)
    pub month: f64,
    /// `getDate()` (1–31)
    pub date: f64,
    /// `getDay()` (0 = Sunday)
    pub day: f64,
    /// `getHours()`
    pub hours: f64,
    /// `getMinutes()`
    pub minutes: f64,
    /// `getSeconds()`
    pub seconds: f64,
}

const MS_PER_DAY: f64 = 86_400_000.0;

/// `TimeClip(t)` (ECMA-262 21.4.1.31): NaN outside ±8.64e15 ms, else the integer part (`+0`).
#[must_use]
pub fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    uo_jsnum::math::trunc(t) + 0.0
}

/// Days from the epoch to the first day of `y` (ECMA-262 `DayFromYear`).
fn day_from_year(y: f64) -> f64 {
    365.0 * (y - 1970.0) + floor((y - 1969.0) / 4.0) - floor((y - 1901.0) / 100.0) + floor((y - 1601.0) / 400.0)
}

fn in_leap_year(y: f64) -> bool {
    (rem(y, 4.0) == 0.0 && rem(y, 100.0) != 0.0) || rem(y, 400.0) == 0.0
}

impl LocalDateTime {
    /// The parts of an invalid `Date`.
    pub const INVALID: LocalDateTime = LocalDateTime {
        year: f64::NAN,
        month: f64::NAN,
        date: f64::NAN,
        day: f64::NAN,
        hours: f64::NAN,
        minutes: f64::NAN,
        seconds: f64::NAN,
    };

    /// The parts of a local time value `lt` = the time value plus the local offset (ms), with the
    /// ECMA-262 date algorithms (`YearFromTime`, `MonthFromTime`, `DateFromTime`, `WeekDay`,
    /// `HourFromTime`…). NaN gives [`LocalDateTime::INVALID`].
    #[must_use]
    pub fn from_local_time(lt: f64) -> LocalDateTime {
        if !lt.is_finite() {
            return LocalDateTime::INVALID;
        }
        let day = floor(lt / MS_PER_DAY);
        let ms_in_day = lt - day * MS_PER_DAY;
        // YearFromTime: the largest y with DayFromYear(y) <= day
        let mut y = floor(day / 365.2425) + 1970.0;
        while day_from_year(y) > day {
            y -= 1.0;
        }
        while day_from_year(y + 1.0) <= day {
            y += 1.0;
        }
        let mut d = day - day_from_year(y);
        let leap = if in_leap_year(y) { 1.0 } else { 0.0 };
        let lengths = [31.0, 28.0 + leap, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
        let mut month = 0.0;
        for len in lengths {
            if d < len {
                break;
            }
            d -= len;
            month += 1.0;
        }
        LocalDateTime {
            year: y,
            month,
            date: d + 1.0,
            day: rem(rem(day + 4.0, 7.0) + 7.0, 7.0),
            hours: floor(ms_in_day / 3_600_000.0),
            minutes: rem(floor(ms_in_day / 60_000.0), 60.0),
            seconds: rem(floor(ms_in_day / 1000.0), 60.0),
        }
    }

    /// The parts of time value `t` (ms since the epoch, `TimeClip`ped first) at a local offset of
    /// `offset_ms` (the offset at `t`; a `Date`'s `LocalTime(t)`).
    #[must_use]
    pub fn at(t: f64, offset_ms: f64) -> LocalDateTime {
        let t = time_clip(t);
        if t.is_nan() { LocalDateTime::INVALID } else { LocalDateTime::from_local_time(t + offset_ms) }
    }
}

struct Lang {
    months: [&'static str; 12],
    months_short: [&'static str; 12],
    days: [&'static str; 7],
    days_short: [&'static str; 7],
    date: &'static str,
    time: &'static str,
    en: bool,
}

const PT: Lang = Lang {
    months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
    months_short: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
    days: ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"],
    days_short: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
    date: "dd/MM/yyyy",
    time: "HH:mm",
    en: false,
};
const EN: Lang = Lang {
    months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    months_short: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    days: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
    days_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    date: "M/d/yyyy",
    time: "h:mm am/pm",
    en: true,
};

fn lang_of(lang: &str) -> &'static Lang {
    if uo_text::case::to_lowercase_str(lang).starts_with("en") { &EN } else { &PT }
}

/// `arr[i]` with a numeric index: `undefined` (printed `"undefined"` by `+=`) when not an index.
fn name_at(arr: &[&'static str], i: f64) -> &'static str {
    if i >= 0.0 && uo_jsnum::conv::is_integer(i) {
        usize::try_from(uo_jsnum::conv::to_uint32(i)).ok().and_then(|i| arr.get(i)).copied().unwrap_or("undefined")
    } else {
        "undefined"
    }
}

fn push_str(out: &mut Vec<u16>, s: &str) {
    out.extend(s.encode_utf16());
}

/// `formatWordDate(d, picture, lang)` (datefmt.ts:23).
#[must_use]
pub fn format_word_date(d: &LocalDateTime, picture: &[u16], lang: &str) -> Vec<u16> {
    let l = lang_of(lang);
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0usize;
    let run = |i: usize, ch: u16| -> usize { picture.get(i..).map_or(0, |r| r.iter().take_while(|&&u| u == ch).count()) };
    while let Some(&c) = picture.get(i) {
        if c == u16::from(b'\'') {
            let j = picture.get(i + 1..).and_then(|r| r.iter().position(|&u| u == u16::from(b'\''))).map(|p| p + i + 1);
            match j {
                None => {
                    out.extend_from_slice(picture.get(i + 1..).unwrap_or(&[]));
                    i = picture.len();
                }
                Some(j) => {
                    out.extend_from_slice(picture.get(i + 1..j).unwrap_or(&[]));
                    i = j + 1;
                }
            }
            continue;
        }
        let lower = uo_text::case::to_lowercase(super::num::slice16(picture, i, i + 5));
        if *lower == *"am/pm".encode_utf16().collect::<Vec<u16>>() {
            // the designator of the language in any case of the picture: "PM" in English, empty
            // in pt-BR
            if l.en {
                push_str(&mut out, if d.hours >= 12.0 { "PM" } else { "AM" });
            }
            i += 5;
            continue;
        }
        let ch = char::from_u32(u32::from(c)).unwrap_or('\0');
        match ch {
            'd' | 'D' => {
                let n = run(i, c);
                let s: String = if n >= 4 {
                    name_at(&l.days, d.day).into()
                } else if n == 3 {
                    name_at(&l.days_short, d.day).into()
                } else if n == 2 {
                    pad_num(d.date, 2)
                } else {
                    to_js_string(d.date)
                };
                push_str(&mut out, &s);
                i += n;
            }
            'M' => {
                let n = run(i, c);
                let s: String = if n >= 4 {
                    name_at(&l.months, d.month).into()
                } else if n == 3 {
                    name_at(&l.months_short, d.month).into()
                } else if n == 2 {
                    pad_num(d.month + 1.0, 2)
                } else {
                    to_js_string(d.month + 1.0)
                };
                push_str(&mut out, &s);
                i += n;
            }
            'y' | 'Y' => {
                let n = run(i, c);
                let s = if n >= 3 { to_js_string(d.year) } else { pad_num(rem(d.year, 100.0), 2) };
                push_str(&mut out, &s);
                i += n;
            }
            'H' => {
                let n = run(i, c);
                push_str(&mut out, &if n >= 2 { pad_num(d.hours, 2) } else { to_js_string(d.hours) });
                i += n;
            }
            'h' => {
                let n = run(i, c);
                let h12 = rem(d.hours, 12.0);
                let h = if uo_jsnum::conv::truthy(h12) { h12 } else { 12.0 };
                push_str(&mut out, &if n >= 2 { pad_num(h, 2) } else { to_js_string(h) });
                i += n;
            }
            'm' => {
                let n = run(i, c);
                push_str(&mut out, &if n >= 2 { pad_num(d.minutes, 2) } else { to_js_string(d.minutes) });
                i += n;
            }
            's' | 'S' => {
                let n = run(i, c);
                push_str(&mut out, &if n >= 2 { pad_num(d.seconds, 2) } else { to_js_string(d.seconds) });
                i += n;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// `\@\s*(?:"([^"]*)"|(\S+))` (and the same with `#` for `\#`): the first match's picture.
#[must_use]
pub fn find_switch_picture(s: &[u16], sw: u16) -> Option<&[u16]> {
    let bs = u16::from(b'\\');
    let q = u16::from(b'"');
    let mut p = 0usize;
    while p + 1 < s.len() {
        if s.get(p) == Some(&bs) && s.get(p + 1) == Some(&sw) {
            let mut k = p + 2;
            while s.get(k).is_some_and(|&u| is_js_space_unit(u)) {
                k += 1;
            }
            if s.get(k) == Some(&q)
                && let Some(e) = s.get(k + 1..).and_then(|r| r.iter().position(|&u| u == q))
            {
                return s.get(k + 1..k + 1 + e);
            }
            if s.get(k).is_some_and(|&u| !is_js_space_unit(u)) {
                let e = s.get(k..).map_or(0, |r| r.iter().take_while(|&&u| !is_js_space_unit(u)).count());
                return s.get(k..k + e);
            }
        }
        p += 1;
    }
    None
}

/// ASCII `\w`.
fn is_word(u: u16) -> bool {
    u < 0x80 && uo_jsnum::text::is_js_word(u8::try_from(u).unwrap_or(0))
}

/// Every `\*\s*(\w+)` of `s` (global, in order): the word of each.
#[must_use]
pub fn format_switches(s: &[u16]) -> Vec<&[u16]> {
    let bs = u16::from(b'\\');
    let star = u16::from(b'*');
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 1 < s.len() {
        if s.get(p) == Some(&bs) && s.get(p + 1) == Some(&star) {
            let mut k = p + 2;
            while s.get(k).is_some_and(|&u| is_js_space_unit(u)) {
                k += 1;
            }
            let e = s.get(k..).map_or(0, |r| r.iter().take_while(|&&u| is_word(u)).count());
            if e > 0 {
                if let Some(w) = s.get(k..k + e) {
                    out.push(w);
                }
                p = k + e;
                continue;
            }
        }
        p += 1;
    }
    out
}

/// `v.replace(/(^|\s)(\S)/g, (_, a, b) => a + b.toUpperCase())` (the `Caps` format), per unit.
fn caps_words(v: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(v.len());
    let mut i = 0usize;
    let upper1 = |u: u16| -> Vec<u16> { uo_text::case::to_uppercase(std::slice::from_ref(&u)).into_owned() };
    while i < v.len() {
        let cur = v.get(i).copied().unwrap_or(0);
        if i == 0 && !is_js_space_unit(cur) {
            out.extend(upper1(cur));
            i += 1;
            continue;
        }
        if is_js_space_unit(cur)
            && let Some(&nx) = v.get(i + 1)
            && !is_js_space_unit(nx)
        {
            out.push(cur);
            out.extend(upper1(nx));
            i += 2;
            continue;
        }
        out.push(cur);
        i += 1;
    }
    out
}

/// `dateFieldValue(instr, now, lang)` (datefmt.ts:87): the value of a DATE/TIME field
/// (whole instruction) at `now`, cut at `cap` (`max_field_text`; the TS has no limit).
#[must_use]
pub fn date_field_value(instr: &[u16], now: &LocalDateTime, lang: &str, cap: &mut super::numbering::TextCap) -> Utf16 {
    let trimmed = uo_text::space::trim(instr);
    let first: Vec<u16> = trimmed.iter().copied().take_while(|&u| !is_js_space_unit(u)).collect();
    let name = uo_text::case::to_uppercase(&first).into_owned();
    let l = lang_of(lang);
    let time: Vec<u16> = "TIME".encode_utf16().collect();
    let default_pic: Vec<u16> = (if name == time { l.time } else { l.date }).encode_utf16().collect();
    let picture: &[u16] = find_switch_picture(instr, u16::from(b'@')).unwrap_or(&default_pic);
    let mut v = format_word_date(now, picture, lang);
    for f in format_switches(instr) {
        let k: Vec<u16> = uo_text::case::to_lowercase(f).into_owned();
        let k = String::from_utf16_lossy(&k);
        match k.as_str() {
            "upper" => v = uo_text::case::to_uppercase(&v).into_owned(),
            "lower" => v = uo_text::case::to_lowercase(&v).into_owned(),
            "firstcap" => v = super::numbering::first_upper(&v),
            "caps" => v = caps_words(&v),
            _ => {}
        }
    }
    if v.len() > cap.max {
        // a field text past `max_field_text` (K11b, LQ14): truncated, the caller warns
        v.truncate(cap.max);
        cap.mark_cut();
    }
    Utf16::from_units(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn local_parts() {
        // 2026-01-15T12:00:00Z, a Thursday
        let d = LocalDateTime::at(1_768_478_400_000.0, 0.0);
        assert_eq!((d.year, d.month, d.date, d.day, d.hours, d.minutes, d.seconds), (2026.0, 0.0, 15.0, 4.0, 12.0, 0.0, 0.0));
        // 1969-12-31T23:59:59.999Z
        let d = LocalDateTime::at(-1.0, 0.0);
        assert_eq!((d.year, d.month, d.date, d.day, d.hours, d.minutes, d.seconds), (1969.0, 11.0, 31.0, 3.0, 23.0, 59.0, 59.0));
        assert!(LocalDateTime::at(9e15, 0.0).year.is_nan());
    }

    #[test]
    fn pictures() {
        let d = LocalDateTime::at(1_768_478_400_000.0, -3.0 * 3_600_000.0);
        let f = |p: &str, l: &str| String::from_utf16_lossy(&format_word_date(&d, &u(p), l));
        assert_eq!(f("dddd, d 'de' MMMM 'de' yyyy", "pt-BR"), "quinta-feira, 15 de janeiro de 2026");
        assert_eq!(f("h:mm AM/PM", "en-US"), "9:00 AM");
        assert_eq!(f("HH:mm am/pm", "pt-BR"), "09:00 ");
        let v = date_field_value(&u(r#" DATE \@ "MMMM d" \* Upper "#), &d, "pt-BR", &mut super::super::numbering::TextCap::default());
        assert_eq!(String::from_utf16_lossy(v.units()), "JANEIRO 15");
        let v = date_field_value(&u(r" TIME \* Caps"), &d, "en-US", &mut super::super::numbering::TextCap::default());
        assert_eq!(String::from_utf16_lossy(v.units()), "9:00 AM");
        let bad = date_field_value(&u("DATE \\@ dddd"), &LocalDateTime::INVALID, "pt-BR", &mut super::super::numbering::TextCap::default());
        assert_eq!(String::from_utf16_lossy(bad.units()), "undefined");
    }
}
