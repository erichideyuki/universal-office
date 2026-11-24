//! Excel date serials (`dates.ts`): serial = days, the fraction of the day is the time; the 1900 system counts the
//! fictitious 29/02/1900 (serial 60), serial 0 is "00/01/1900"; the 1904 system starts at 1904-01-01.
//!
//! The TS computes through JS `Date` (`Date.UTC`, `setUTCFullYear`, `getUTC*`), so this module ports V8's date
//! arithmetic (`MakeDay`/`MakeDate`/`TimeClip` of `src/date/date.cc` and the builtins): same year/month range checks,
//! same 32-bit day arithmetic, NaN where V8 gives NaN. Every function takes and returns `f64` like the TS, so
//! fractional, negative, huge and NaN arguments give the TS bits.
#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x >= y)` tests: NaN takes the negated branch")]

use uo_jsnum::math::{floor, rem, round, trunc};

use crate::cast::{trunc_i32, trunc_i64};

const MS_DAY: f64 = 86_400_000.0;
/// `Date.UTC(1899, 11, 31)`.
const BASE_1900: f64 = -2_209_075_200_000.0;
/// `Date.UTC(1904, 0, 1)`.
const BASE_1904: f64 = -2_082_844_800_000.0;

/// Largest serial Excel accepts (31/12/9999).
pub const MAX_DATE_SERIAL: f64 = 2_958_465.0;
/// Serial (1900 system) of 01/01/1904.
pub const DATE1904_OFFSET: f64 = 1462.0;

/// `maxSerial(date1904)`.
#[must_use]
pub fn max_serial(date1904: bool) -> f64 {
    if date1904 { MAX_DATE_SERIAL - DATE1904_OFFSET } else { MAX_DATE_SERIAL }
}

// ---------------------------------------------------------------- V8's Date arithmetic

/// ECMAScript `ToIntegerOrInfinity` (V8 `DoubleToInteger`): truncation, NaN → 0, `-0` → `+0`.
fn to_integer(x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    let t = trunc(x);
    if t == 0.0 { 0.0 } else { t }
}

/// V8 `MakeDay(year, month, date)` (`src/date/date.cc`): the range checks of V8 (|year| ≤ 1,000,000,
/// |month| ≤ 10,000,000), then 32-bit day arithmetic with `kYearDelta` exactly as the C++.
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    const MIN_YEAR: f64 = -1_000_000.0;
    const MAX_YEAR: f64 = 1_000_000.0;
    const MIN_MONTH: f64 = -10_000_000.0;
    const MAX_MONTH: f64 = 10_000_000.0;
    if !((MIN_YEAR..=MAX_YEAR).contains(&year) && (MIN_MONTH..=MAX_MONTH).contains(&month) && date.is_finite()) {
        return f64::NAN;
    }
    // FastD2I: truncation toward zero (both are in i32 range after the checks above)
    let mut y = trunc_i32(year);
    let mut m = trunc_i32(month);
    y += m / 12;
    m %= 12;
    if m < 0 {
        m += 12;
        y -= 1;
    }
    const YEAR_DELTA: i32 = 399_999;
    const BASE_DAY: i32 = 365 * (1970 + YEAR_DELTA) + (1970 + YEAR_DELTA) / 4 - (1970 + YEAR_DELTA) / 100 + (1970 + YEAR_DELTA) / 400;
    let yd = y + YEAR_DELTA;
    let mut day_from_year = 365 * yd + yd / 4 - yd / 100 + yd / 400 - BASE_DAY;
    let leap = !((y % 4 != 0) || (y % 100 == 0 && y % 400 != 0));
    const COMMON: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    const LEAP: [i32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
    let idx = m as usize; // 0..=11 after the adjustment above
    day_from_year += if leap { LEAP[idx] } else { COMMON[idx] };
    f64::from(day_from_year - 1) + to_integer(date)
}

/// `MakeDate(day, time)` followed by `TimeClip`.
fn make_time_clip(day: f64, time: f64) -> f64 {
    let t = day * MS_DAY + time;
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    to_integer(t)
}

/// `Date.UTC(year, month, date)` (years whose integer part is 0..=99 mean 1900 + that).
fn date_utc(year: f64, month: f64, date: f64) -> f64 {
    let mut year = year;
    if !year.is_nan() {
        let yi = to_integer(year);
        if (0.0..=99.0).contains(&yi) {
            year = 1900.0 + yi;
        }
    }
    make_time_clip(make_day(year, month, date), 0.0)
}

/// Days of the epoch → (year, month 1..12, day): the proleptic Gregorian calendar.
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
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `new Date(t)` then `getUTCFullYear/Month()+1/Date()`: NaN fields for an invalid time.
fn utc_ymd(t: f64) -> Ymd {
    // `new Date(x)` applies TimeClip
    let t = if !t.is_finite() || t.abs() > 8.64e15 { f64::NAN } else { to_integer(t) };
    if t.is_nan() {
        return Ymd { y: f64::NAN, m: f64::NAN, d: f64::NAN };
    }
    let days = trunc_i64(t).div_euclid(86_400_000); // integral, |t| ≤ 8.64e15
    let (y, m, d) = civil_from_days(days);
    Ymd { y: y as f64, m: m as f64, d: d as f64 }
}

// ---------------------------------------------------------------- the TS API

/// A calendar date of the Excel calendar (`{ y, m, d }`); NaN fields where the TS gets NaN from `Date`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ymd {
    /// Year.
    pub y: f64,
    /// Month 1..12.
    pub m: f64,
    /// Day of the month (0 for serial 0, 29 for serial 60).
    pub d: f64,
}

/// Hours, minutes and rounded seconds of a day fraction (`serialToHms`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hms {
    /// Hour 0..23.
    pub h: f64,
    /// Minute.
    pub mi: f64,
    /// Second.
    pub s: f64,
}

/// `ymdToSerial(y, m, d, date1904)`: year/month/day → serial, month and day normalized as `DATE` does (month 13 =
/// January of the next year, day 0 = last day of the previous month), counting the fictitious 29/02/1900.
#[must_use]
pub fn ymd_to_serial(y: f64, m: f64, d: f64, date1904: bool) -> f64 {
    let yy = y + floor((m - 1.0) / 12.0);
    let mm = rem(rem(m - 1.0, 12.0) + 12.0, 12.0);
    let first = date_utc(yy, mm, 1.0);
    // Date.UTC reads years 0–99 as 1900–1999; the TS corrects with setUTCFullYear
    let t = if (0.0..100.0).contains(&yy) { set_utc_full_year(first, yy) } else { first };
    if date1904 {
        return round((t - BASE_1904) / MS_DAY) + d - 1.0;
    }
    let mut s = round((t - BASE_1900) / MS_DAY);
    if s >= 60.0 {
        s += 1.0;
    }
    s + d - 1.0
}

/// `new Date(t).setUTCFullYear(year)` (V8 builtin: month and date of `t`, its time within the day).
fn set_utc_full_year(t: f64, year: f64) -> f64 {
    let (m, dt, within) = if t.is_nan() {
        (0.0, 1.0, 0.0)
    } else {
        // `t` is a TimeClip result: integral, |t| ≤ 8.64e15
        let ms = trunc_i64(t);
        let days = ms.div_euclid(86_400_000);
        let (_, m, d) = civil_from_days(days);
        ((m - 1) as f64, d as f64, ms.rem_euclid(86_400_000) as f64)
    };
    make_time_clip(make_day(year, m, dt), within)
}

/// `serialToYmd(serial, date1904)` (integer part).
#[must_use]
pub fn serial_to_ymd(serial: f64, date1904: bool) -> Ymd {
    let mut s = floor(serial);
    if date1904 {
        return utc_ymd(BASE_1904 + s * MS_DAY);
    }
    if s == 0.0 {
        return Ymd { y: 1900.0, m: 1.0, d: 0.0 };
    }
    if s == 60.0 {
        return Ymd { y: 1900.0, m: 2.0, d: 29.0 };
    }
    if s > 60.0 {
        s -= 1.0;
    }
    utc_ymd(BASE_1900 + s * MS_DAY)
}

/// `weekday(serial, date1904)`: 0 = Sunday, with Excel's leap 1900.
#[must_use]
pub fn weekday(serial: f64, date1904: bool) -> f64 {
    let s = floor(serial) + if date1904 { DATE1904_OFFSET } else { 0.0 };
    rem(rem(s + 6.0, 7.0) + 7.0, 7.0)
}

/// `serialToHms(serial)`: seconds rounded as Excel displays them.
#[must_use]
pub fn serial_to_hms(serial: f64) -> Hms {
    let frac = serial - floor(serial);
    let total = round(frac * 86400.0);
    Hms { h: rem(floor(total / 3600.0), 24.0), mi: rem(floor(total / 60.0), 60.0), s: rem(total, 60.0) }
}

/// `daysInMonth(y, m, date1904)` (February 1900 has 29 days in the 1900 system; a month that is not an integer
/// 1..12 has 0, the TS `?? 0`).
#[must_use]
pub fn days_in_month(y: f64, m: f64, date1904: bool) -> f64 {
    if m == 2.0 {
        return if is_leap(y) || (y == 1900.0 && !date1904) { 29.0 } else { 28.0 };
    }
    const DAYS: [f64; 12] = [31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
    uo_jsnum::conv::js_index(m - 1.0, DAYS.len()).map_or(0.0, |i| DAYS[i])
}

/// `isLeap(y)`: the Gregorian rule (without the fictitious 1900).
#[must_use]
pub fn is_leap(y: f64) -> bool {
    (rem(y, 4.0) == 0.0 && rem(y, 100.0) != 0.0) || rem(y, 400.0) == 0.0
}

/// `splitSerial(serial, date1904)`: the serial rounded to the second as `YEAR`…`SECOND` read it: `(day, second of
/// the day)`; `None` outside the range (`#NUM!`).
#[must_use]
pub fn split_serial(serial: f64, date1904: bool) -> Option<(f64, f64)> {
    if !(serial >= 0.0) {
        return None;
    }
    let total = round(serial * 86400.0);
    let day = floor(total / 86400.0);
    if day > max_serial(date1904) {
        return None;
    }
    Some((day, total - day * 86400.0))
}

/// `excelDate(year, month, day, date1904)` = `DATE`: parts floored, year < 1900 adds 1900, out-of-range year/month
/// or serial → `None` (`#NUM!`), a day outside 16 bits becomes 32767.
#[must_use]
pub fn excel_date(year: f64, month: f64, day: f64, date1904: bool) -> Option<f64> {
    let mut y = floor(year);
    let m = floor(month);
    let mut d = floor(day);
    if !y.is_finite() || !m.is_finite() || !d.is_finite() {
        return None;
    }
    if !(0.0..10000.0).contains(&y) {
        return None;
    }
    if y < 1900.0 {
        y += 1900.0;
    }
    if m >= 32767.0 || m < -32768.0 {
        return None;
    }
    if d > 32767.0 || d < -32768.0 {
        d = 32767.0;
    }
    let s = ymd_to_serial(y, m, d, date1904);
    if s < 0.0 || s > max_serial(date1904) {
        return None;
    }
    Some(s)
}

/// The current year of a clock's local "now" serial (the TS `new Date().getFullYear()`, `coerce.ts:197`).
#[must_use]
pub fn year_of_serial(now_serial: f64) -> f64 {
    serial_to_ymd(now_serial, false).y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases() {
        assert_eq!(date_utc(1899.0, 11.0, 31.0), BASE_1900);
        assert_eq!(date_utc(1904.0, 0.0, 1.0), BASE_1904);
    }

    #[test]
    fn serials() {
        assert_eq!(ymd_to_serial(1900.0, 1.0, 1.0, false), 1.0);
        assert_eq!(ymd_to_serial(1900.0, 3.0, 1.0, false), 61.0);
        assert_eq!(ymd_to_serial(2025.0, 1.0, 1.0, false), 45658.0);
        assert_eq!(ymd_to_serial(1904.0, 1.0, 1.0, true), 0.0);
        assert_eq!(serial_to_ymd(60.0, false), Ymd { y: 1900.0, m: 2.0, d: 29.0 });
        assert_eq!(serial_to_ymd(45658.0, false), Ymd { y: 2025.0, m: 1.0, d: 1.0 });
        assert_eq!(weekday(1.0, false), 0.0);
        assert_eq!(weekday(45658.0, false), 3.0);
        assert_eq!(serial_to_hms(0.4375), Hms { h: 10.0, mi: 30.0, s: 0.0 });
        // year 50 (Date.UTC would read 1950: setUTCFullYear corrects it)
        assert_eq!(ymd_to_serial(50.0, 1.0, 1.0, false), round((date_utc_raw(50.0, 0.0) - BASE_1900) / MS_DAY));
        assert!(ymd_to_serial(2e6, 1.0, 1.0, false).is_nan());
        assert_eq!(year_of_serial(45658.5), 2025.0);
    }

    fn date_utc_raw(y: f64, m: f64) -> f64 {
        make_time_clip(make_day(y, m, 1.0), 0.0)
    }
}
